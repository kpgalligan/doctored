//! Attach captured camera provenance only to a private, newly retained u16
//! revision. Replacement and affine evaluation stay in the shared source engine.
//! Copying original bytes is admitted and cancellable; redevelopment shares them.
//! In a 32-bit float document a 16-bit SDR development keeps its u16 master and
//! its fallback is the f32 render of it into linear light (`retained_source_float`);
//! a 32-bit float (linear) development keeps an f32 master, which
//! attaches only there (`RawDocument::admits_float_master`).
use crate::doc::DrDocument;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, reserved_vec, BOOKKEEPING_BYTES};
use crate::raw_source::{
    validate_length, RawDescriptor, RawSource, MAX_RAW_TOTAL_BYTES, RAW_DESCRIPTOR_EXPANSION,
};
use crate::resources::Inventory;
use crate::retained_source::SourcePlan;
use crate::source_asset::SourceAsset;
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) trait RawDocument: Sized {
    /// The refusal a layer LOCK phrases for an ATTACHMENT write, or `Ok(())`
    /// when nothing blocks. An attachment rebinds the record's retained
    /// master, which is where its pixels come from, so it answers to the
    /// PIXEL lock — `doc_lock`'s rule, at both depths. The two sentences
    /// differ because each depth's reporting shape does (`doc_lock`'s `None`
    /// is phrased by its caller, `precision_lock` names the bit); the RULE is
    /// the same, and it is consulted FIRST by every attachment writer.
    fn attachment_lock_refusal(&self, layer: usize) -> Result<(), String>;
    /// Whether a camera original may attach to a 32-bit float developed
    /// master here: only a 32-bit float document holds one (a scene-linear
    /// development with values above white). An 8- or 16-bit document
    /// answers false, so the master it retains is always u16.
    fn admits_float_master(&self) -> bool;
    fn source_attachment(
        &self,
        layer: usize,
    ) -> Option<Arc<crate::source_attachment::SourceAttachment>>;
    fn raw_asset(&self, layer: usize) -> Result<Arc<SourceAsset>, String>;
    fn raw_assets(&self) -> Vec<Arc<SourceAsset>>;
    fn raw_inventory(&self, inventory: &mut Inventory);
    fn raw_clone_bytes(&self) -> u64;
    fn with_source_asset(&self, layer: usize, asset: Arc<SourceAsset>) -> Result<Self, String>;
    fn with_raw(&self, layer: usize, raw: Arc<RawSource>) -> Result<Self, String> {
        let mut asset = (*self.raw_asset(layer)?).clone();
        asset.raw = Some(raw);
        self.with_source_asset(layer, Arc::new(asset))
    }
}
impl RawDocument for DrDocument {
    fn attachment_lock_refusal(&self, layer: usize) -> Result<(), String> {
        if self.lock_block(layer, crate::doc_lock::EditKind::Pixels) != 0 {
            return Err("source attachment is blocked by the layer's pixel lock".into());
        }
        Ok(())
    }
    fn admits_float_master(&self) -> bool {
        false
    }
    fn source_attachment(
        &self,
        layer: usize,
    ) -> Option<Arc<crate::source_attachment::SourceAttachment>> {
        self.layers
            .get(layer)?
            .effective_source(&self.profile)?
            .asset
            .attachment
            .clone()
    }
    fn raw_asset(&self, layer: usize) -> Result<Arc<SourceAsset>, String> {
        self.layers
            .get(layer)
            .and_then(|l| l.effective_source(&self.profile))
            .map(|s| s.asset)
            .ok_or_else(|| "layer has no retained source".into())
    }
    fn raw_assets(&self) -> Vec<Arc<SourceAsset>> {
        self.layers
            .iter()
            .filter_map(|l| l.effective_source(&self.profile).map(|s| s.asset))
            .collect()
    }
    fn raw_inventory(&self, inventory: &mut Inventory) {
        inventory.document(self);
    }
    fn raw_clone_bytes(&self) -> u64 {
        self.cloned_structure_bytes()
    }
    fn with_source_asset(&self, layer: usize, asset: Arc<SourceAsset>) -> Result<Self, String> {
        let mut source = self
            .layers
            .get(layer)
            .and_then(|l| l.effective_source(&self.profile))
            .ok_or("layer has no retained source")?;
        source.asset = asset;
        let mut next = self.clone();
        next.layers[layer].retained_source = Some(source);
        if !self.native_edit_fits(&next) {
            return Err("RAW attachment exceeds native capacity".into());
        }
        next.detach_invalid_sources();
        Ok(next)
    }
}
impl RawDocument for PrecisionDocument {
    fn attachment_lock_refusal(&self, layer: usize) -> Result<(), String> {
        self.lock_refusal(layer, crate::doc_lock::EditKind::Pixels)
    }
    fn admits_float_master(&self) -> bool {
        self.is_float()
    }
    fn source_attachment(
        &self,
        layer: usize,
    ) -> Option<Arc<crate::source_attachment::SourceAttachment>> {
        self.layer(layer)?
            .effective_source()?
            .asset
            .attachment
            .clone()
    }
    fn raw_asset(&self, layer: usize) -> Result<Arc<SourceAsset>, String> {
        self.layer(layer)
            .and_then(|l| l.effective_source())
            .map(|s| s.asset)
            .ok_or_else(|| "layer has no retained source".into())
    }
    fn raw_assets(&self) -> Vec<Arc<SourceAsset>> {
        (0..self.layer_count())
            .filter_map(|i| self.layer(i)?.effective_source().map(|s| s.asset))
            .collect()
    }
    fn raw_inventory(&self, inventory: &mut Inventory) {
        self.record_resources(inventory);
    }
    fn raw_clone_bytes(&self) -> u64 {
        // A replacement clones only its ancestor paths. Charging all current
        // structure/name allocations also covers its bounded intermediate paths.
        crate::precision_inspect::inspection_bytes(self)
    }
    fn with_source_asset(&self, layer: usize, asset: Arc<SourceAsset>) -> Result<Self, String> {
        let mut target = self
            .layer(layer)
            .ok_or("RAW layer is out of range")?
            .clone();
        let mut source = target
            .effective_source()
            .ok_or("layer has no retained source")?;
        source.asset = asset;
        target.retained_source = Some(source);
        self.replacing_layer(layer, target)?
            .ok_or_else(|| "RAW attachment produced no revision".into())
    }
}
pub(crate) fn raw<D: RawDocument>(doc: &D, layer: usize) -> Result<Arc<RawSource>, String> {
    doc.raw_asset(layer)?
        .raw
        .clone()
        .ok_or_else(|| "layer has no retained RAW original".into())
}
/// Bound opaque descriptor parsing before serde allocates its map/string state.
pub(crate) fn admit_descriptor<D: RawDocument>(
    document: &D,
    original: Option<&D>,
    sizes: (usize, usize),
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    job.check()?;
    let mut inventory = Inventory::default();
    document.raw_inventory(&mut inventory);
    if let Some(original) = original {
        original.raw_inventory(&mut inventory);
    }
    let scratch = 4 * BOOKKEEPING_BYTES + 2 * RAW_DESCRIPTOR_EXPANSION * sizes.1 as u64;
    admit(
        inventory.unique_bytes() + sizes.0 as u64,
        scratch,
        other,
        limit,
    )?;
    job.reserve(scratch)
}
pub(crate) fn plan<D: RawDocument>(
    document: &D,
    layer: usize,
    length: usize,
    descriptor: &RawDescriptor,
    original: Option<(&D, usize)>,
) -> Result<SourcePlan, String> {
    // The LOCK gate, consulted FIRST so a locked layer refuses by name before
    // any descriptor or payload is measured.
    document.attachment_lock_refusal(layer)?;
    descriptor.validate()?;
    validate_length(length)?;
    let asset = document.raw_asset(layer)?;
    if asset.depth() == 32 && !document.admits_float_master() {
        return Err(
            "RAW attachment of a 32-bit float developed master requires a 32-bit float document"
                .into(),
        );
    }
    if !matches!(asset.depth(), 16 | 32) {
        return Err("RAW attachment requires a genuine u16 or f32 developed master".into());
    }
    if asset.raw.is_some() {
        return Err(
            "RAW attachment requires a fresh source revision; redevelop through Replace Contents"
                .into(),
        );
    }
    if asset.attachment.is_some() {
        return Err("RAW attachment requires a source revision without another attachment".into());
    }
    let mut inventory = Inventory::default();
    document.raw_inventory(&mut inventory);
    let shared = original.map(|(doc, index)| {
        doc.raw_inventory(&mut inventory);
        let previous = doc.raw_asset(index)?;
        let raw = previous.raw.as_ref().ok_or("original layer has no retained RAW")?;
        if asset.id != previous.id || descriptor.original_filename != raw.descriptor.original_filename
            || descriptor.content_sha256 != raw.descriptor.content_sha256 || length != raw.encoded.len() {
            return Err("RAW redevelopment must preserve the original source identity, filename and fingerprint".into());
        }
        Ok::<_, String>(raw.encoded.clone())
    }).transpose()?;
    let mut payloads = HashSet::new();
    let mut assets = HashSet::new();
    let mut total = 0_u64;
    let mut resource_capacity = crate::source_attachment::ResourceCapacity::default();
    let mut target_bindings = 0;
    for existing in document.raw_assets() {
        if existing.id == asset.id && existing.revision_id == asset.revision_id {
            target_bindings += 1;
            if target_bindings > 1 || !Arc::ptr_eq(&existing, &asset) {
                return Err("RAW attachment requires an unshared source revision identity".into());
            }
        }
        if !assets.insert(Arc::as_ptr(&existing) as usize) {
            continue;
        }
        resource_capacity.asset(&existing)?;
        if let Some(raw) = &existing.raw {
            raw.validate()?;
            if payloads.insert(Arc::as_ptr(&raw.encoded) as usize) {
                total += raw.encoded.len() as u64;
            }
        }
    }
    if shared
        .as_ref()
        .is_none_or(|p| payloads.insert(Arc::as_ptr(p) as usize))
    {
        total += length as u64;
    }
    if total > MAX_RAW_TOTAL_BYTES {
        return Err("retained RAW originals exceed the 512 MiB aggregate limit".into());
    }
    if let Some(shared) = &shared {
        resource_capacity.add(shared)?;
    } else {
        resource_capacity.add_length(length as u64)?;
    }
    let descriptor_bytes = descriptor.encoded_len()? as u64;
    let copied_payload = if shared.is_none() { length as u64 } else { 0 };
    Ok(SourcePlan {
        retained_bytes: inventory.unique_bytes() + copied_payload,
        scratch_bytes: 4 * BOOKKEEPING_BYTES
            + document.raw_clone_bytes()
            + copied_payload
            + 2 * RAW_DESCRIPTOR_EXPANSION * descriptor_bytes,
        work_units: copied_payload,
    })
}
pub(crate) fn attach<D: RawDocument>(
    document: &D,
    layer: usize,
    encoded: &[u8],
    descriptor: &RawDescriptor,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<D, String> {
    job.check()?;
    let plan = plan(document, layer, encoded.len(), descriptor, None)?;
    admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
    job.reserve(plan.scratch_bytes)?;
    let mut owned = reserved_vec(encoded.len())?;
    for chunk in encoded.chunks(1024 * 1024) {
        job.check()?;
        owned.extend_from_slice(chunk);
    }
    let result = document.with_raw(
        layer,
        Arc::new(RawSource {
            encoded: Arc::new(owned),
            descriptor: descriptor.clone(),
        }),
    )?;
    job.report(1., 5)?;
    Ok(result)
}
pub(crate) fn attach_from<D: RawDocument>(
    document: &D,
    layer: usize,
    original: (&D, usize),
    descriptor: &RawDescriptor,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<D, String> {
    let (original, original_layer) = original;
    job.check()?;
    let captured = raw(original, original_layer)?;
    let plan = plan(
        document,
        layer,
        captured.encoded.len(),
        descriptor,
        Some((original, original_layer)),
    )?;
    admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
    job.reserve(plan.scratch_bytes)?;
    let result = document.with_raw(
        layer,
        Arc::new(RawSource {
            encoded: captured.encoded.clone(),
            descriptor: descriptor.clone(),
        }),
    )?;
    job.report(1., 5)?;
    Ok(result)
}
