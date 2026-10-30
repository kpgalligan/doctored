//! RAW-local mask attachment validation and zero-copy revision publication.
//! The known RZLM contract is preflighted before native PNG decode; unknown
//! attachment kinds continue to be opaque and retain their cached rendering.
//!
//! Fresh source revisions share unchanged mask blobs and RAW originals; known mask state/payload
//! versions validate all IDs/dimensions/lengths before native pixel decode, while unknown versions
//! retain opaque cached rendering and refuse mask materialization. The `raw_local_masks` attachment
//! reuses byte v21 / typed v22 and their shared RBIN pool; it introduces no new native document
//! version. Mixed resource accounting includes mask handles and their document aliases once. The
//! cheap attachment-kind query selects RAW versus host-media behavior independently of admitted
//! descriptor reads.
use crate::job_control::JobContext;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::raw_local_mask::{validate_stored_blob, RawLocalMask};
use crate::raw_source_edit::RawDocument;
use crate::resources::Inventory;
use crate::source_attachment::{
    AttachmentDescriptor, ResourceCapacity, SourceAttachment, ATTACHMENT_EXPANSION,
};
use std::collections::HashSet;
use std::sync::Arc;

/// The media type a RAW local mask resource is written with.
pub(crate) const RAW_LOCAL_MEDIA_TYPE: &str = "com.kgalligan.doctored.raw-local-mask";
/// The media type written before the Rasterize → Doctored rename; still read.
pub(crate) const LEGACY_RAW_LOCAL_MEDIA_TYPE: &str = "com.rasterize.raw-local-mask";

pub(crate) fn validate<'a>(
    descriptor: &AttachmentDescriptor,
    resources: impl ExactSizeIterator<Item = &'a [u8]>,
    has_raw: bool,
    dimensions: (u32, u32),
) -> Result<(), String> {
    if descriptor.kind != "raw_local_masks" {
        return Ok(());
    }
    if !has_raw || resources.len() != descriptor.resources.len() {
        return Err("local masks require a retained RAW master and matching references".into());
    }
    let state = descriptor
        .state
        .as_object()
        .ok_or("local-mask state is not an object")?;
    let version = state
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or("local-mask state requires an integer schema version")?;
    if version != 1 {
        // Future state cannot authorize editing, but its cached document stays
        // usable. Known binary framing still cannot hide malformed samples.
        for bytes in resources {
            validate_stored_blob(bytes)?;
        }
        return Ok(());
    }
    let keys = ["schema_version", "coordinates", "width", "height", "nodes"];
    let nodes = state
        .get("nodes")
        .and_then(|v| v.as_array())
        .ok_or("local-mask state requires ordered node identifiers")?;
    if state.len() != keys.len()
        || state.keys().any(|k| !keys.contains(&k.as_str()))
        || state.get("schema_version").and_then(|v| v.as_u64()) != Some(1)
        || state.get("coordinates").and_then(|v| v.as_str())
            != Some("oriented-source-pixel-centers-v1")
        || state.get("width").and_then(|v| v.as_u64()) != Some(dimensions.0 as u64)
        || state.get("height").and_then(|v| v.as_u64()) != Some(dimensions.1 as u64)
        || nodes.len() != descriptor.resources.len()
    {
        return Err(
            "local-mask state differs from its RAW master or supported coordinate contract".into(),
        );
    }
    for ((node, info), bytes) in nodes.iter().zip(&descriptor.resources).zip(resources) {
        if node.as_str() != Some(info.id.as_str())
            || (info.media_type != RAW_LOCAL_MEDIA_TYPE
                && info.media_type != LEGACY_RAW_LOCAL_MEDIA_TYPE)
            || info.byte_length != bytes.len() as u64
            || validate_stored_blob(bytes)?.is_some_and(|d| d != dimensions)
        {
            return Err("local-mask nodes, dimensions or binary resources are inconsistent".into());
        }
    }
    Ok(())
}
pub(crate) fn from_source<D: RawDocument>(
    document: &D,
    layer: usize,
    resource: usize,
) -> Result<RawLocalMask, String> {
    let asset = document.raw_asset(layer)?;
    let attachment = asset
        .attachment
        .as_ref()
        .ok_or("RAW source has no local masks")?;
    if attachment.descriptor.kind != "raw_local_masks" {
        return Err("source attachment is not local RAW coverage".into());
    }
    if attachment
        .descriptor
        .state
        .get("schema_version")
        .and_then(|v| v.as_u64())
        != Some(1)
    {
        return Err(
            "local-mask state version is unavailable; its saved rendering remains intact".into(),
        );
    }
    attachment.validate()?;
    validate(
        &attachment.descriptor,
        attachment.resources.iter().map(|r| r.as_slice()),
        asset.raw.is_some(),
        asset.dimensions(),
    )?;
    RawLocalMask::from_blob(
        attachment
            .resources
            .get(resource)
            .ok_or("local mask resource index is out of range")?
            .clone(),
    )
}
pub(crate) fn attach<D: RawDocument>(
    document: &D,
    layer: usize,
    descriptor: Option<AttachmentDescriptor>,
    masks: &[&RawLocalMask],
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<D>, String> {
    job.check()?;
    // The LOCK gate (`raw_source_edit::RawDocument::attachment_lock_refusal`),
    // consulted FIRST so a locked layer refuses by name before any mask blob is
    // measured.
    document.attachment_lock_refusal(layer)?;
    let asset = document.raw_asset(layer)?;
    if asset.raw.is_none() || asset.attachment.is_some() {
        return Err(
            "local-mask publication requires a fresh retained RAW revision without an attachment"
                .into(),
        );
    }
    let Some(descriptor) = descriptor else {
        if !masks.is_empty() {
            return Err("local masks require an attachment descriptor".into());
        }
        return Ok(None);
    };
    descriptor.validate()?;
    if descriptor.kind != "raw_local_masks" {
        return Err("local-mask publication requires its exact attachment kind".into());
    }
    if descriptor
        .state
        .get("schema_version")
        .and_then(|v| v.as_u64())
        != Some(1)
    {
        return Err("local-mask publication requires supported state version1".into());
    }
    validate(
        &descriptor,
        masks.iter().map(|m| m.encoded()),
        true,
        asset.dimensions(),
    )?;
    let mut inventory = Inventory::default();
    document.raw_inventory(&mut inventory);
    for mask in masks {
        mask.record_resources(&mut inventory);
    }
    let mut capacity = ResourceCapacity::default();
    let mut seen = HashSet::new();
    let mut targets = 0;
    for existing in document.raw_assets() {
        if existing.id == asset.id && existing.revision_id == asset.revision_id {
            targets += 1;
            if targets > 1 || !Arc::ptr_eq(&existing, &asset) {
                return Err("local masks require an unshared source revision identity".into());
            }
        }
        if seen.insert(Arc::as_ptr(&existing) as usize) {
            capacity.asset(&existing)?;
        }
    }
    for mask in masks {
        capacity.add(&mask.blob)?;
    }
    let scratch = 4 * BOOKKEEPING_BYTES
        + document.raw_clone_bytes() * 2
        + 2 * ATTACHMENT_EXPANSION * descriptor.encoded_len()? as u64;
    admit(inventory.unique_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    job.check()?;
    let mut asset = (*asset).clone();
    asset.attachment = Some(Arc::new(SourceAttachment {
        descriptor,
        resources: masks.iter().map(|m| m.blob.clone()).collect(),
    }));
    let next = document.with_source_asset(layer, Arc::new(asset))?;
    job.report(1.0, 5)?;
    Ok(Some(next))
}
