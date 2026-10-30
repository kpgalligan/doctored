//! Strict scalar-edit requests shared by typed FFI and internal callers. Raw
//! u16 input is borrowed until complete admission, then copied cooperatively.
//!
//! Strict edits and native capacity checks precede source copies.
use crate::doc_mask::MaskProperties;
use crate::drdc::MAX_DRDC_NAME_LEN;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use serde::Deserialize;

fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}
#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaskEdit {
    SetMask {
        layer: usize,
        width: u32,
        height: u32,
        #[serde(default)]
        x: i32,
        #[serde(default)]
        y: i32,
        #[serde(default = "yes")]
        linked: bool,
        #[serde(default = "one")]
        density: f32,
        #[serde(default)]
        feather: f32,
        #[serde(default = "yes")]
        enabled: bool,
        #[serde(default)]
        outside_coverage: u8,
    },
    RemoveMask {
        layer: usize,
    },
    ApplyMask {
        layer: usize,
    },
    MaskProperties {
        layer: usize,
        x: i32,
        y: i32,
        linked: bool,
        density: f32,
        feather: f32,
        enabled: bool,
    },
    SetChannel {
        index: Option<usize>,
        name: String,
    },
    RemoveChannel {
        index: usize,
    },
}
impl MaskEdit {
    pub fn parse(json: &str) -> Result<Self, String> {
        if json.len() > 64 * 1024 {
            return Err("typed mask request exceeds64KiB".into());
        }
        serde_json::from_str(json).map_err(|e| format!("invalid typed mask edit: {e}"))
    }
    pub fn expected_samples(&self, doc: &PrecisionDocument) -> Result<usize, String> {
        match self {
            Self::SetMask { width, height, .. } => pixel_count(*width, *height),
            Self::SetChannel { .. } => pixel_count(doc.width, doc.height),
            _ => Ok(0),
        }
    }
    pub fn scratch_bytes(&self, doc: &PrecisionDocument) -> Result<u64, String> {
        self.validate(doc)?;
        if let Self::ApplyMask { layer } = self {
            // A CMYK raster bakes too (#1, `precision_mask_apply`'s `# CMYK`).
            return doc.apply_mask_plan(*layer);
        }
        // Four MiB covers the already parsed command plus new ancestor/name
        // metadata and bounded inventory work; borrowed input plus source copy is4N.
        Ok(4 * BOOKKEEPING_BYTES + self.expected_samples(doc)? as u64 * 4)
    }
    fn validate(&self, doc: &PrecisionDocument) -> Result<(), String> {
        match self {
            Self::SetMask {
                layer,
                width,
                height,
                x,
                y,
                linked,
                density,
                feather,
                outside_coverage,
                ..
            } => {
                doc.layer(*layer)
                    .ok_or("typed mask layer is out of range")?;
                let mut properties = properties((*x, *y), *linked, *density, *feather);
                properties.outside_coverage = *outside_coverage;
                if !properties.valid((*width, *height)) {
                    return Err("invalid typed mask properties or dimensions".into());
                }
                let (r, mut m) = dimensions_without_mask(doc, Some(*layer));
                m.push((*width, *height));
                doc.native_fits_geometry(doc.dimensions(), &r, &m, doc.channels.len())?;
            }
            Self::RemoveMask { layer }
            | Self::ApplyMask { layer }
            | Self::MaskProperties { layer, .. } => {
                let node = doc
                    .layer(*layer)
                    .ok_or("typed mask layer is out of range")?;
                if let Self::MaskProperties {
                    x,
                    y,
                    linked,
                    density,
                    feather,
                    ..
                } = self
                {
                    let mask = node.mask().ok_or("typed layer has no mask")?;
                    if !properties((*x, *y), *linked, *density, *feather)
                        .valid(mask.plane().dimensions())
                    {
                        return Err("invalid typed mask properties or dimensions".into());
                    }
                }
            }
            Self::SetChannel { index, name } => {
                if name.len() > MAX_DRDC_NAME_LEN as usize {
                    return Err("typed channel name exceeds native limit".into());
                }
                if index.is_some_and(|i| i >= doc.channels.len()) {
                    return Err("typed channel index is out of range".into());
                }
                let (r, m) = doc.native_dimensions();
                doc.native_fits_geometry(
                    doc.dimensions(),
                    &r,
                    &m,
                    doc.channels.len() + usize::from(index.is_none()),
                )?;
            }
            Self::RemoveChannel { index } => {
                if *index >= doc.channels.len() {
                    return Err("typed channel index is out of range".into());
                }
            }
        }
        Ok(())
    }
}
fn properties(offset: (i32, i32), linked: bool, density: f32, feather: f32) -> MaskProperties {
    let mut p = MaskProperties::at(offset);
    p.linked = linked;
    p.density = density;
    p.feather = feather;
    p
}
fn dimensions_without_mask(
    doc: &PrecisionDocument,
    skip: Option<usize>,
) -> crate::precision_native_plan::NativeDimensions {
    let mut r = Vec::new();
    let mut m = Vec::new();
    for index in 0..doc.layer_count() {
        if let Some(layer) = doc.layer(index) {
            if let Some(image) = layer.source() {
                r.push(image.dimensions());
            }
            if let Some(image) = layer.cmyk_source() {
                r.push(image.dimensions());
            }
            if Some(index) != skip {
                if let Some(mask) = layer.mask() {
                    m.push(mask.plane().dimensions());
                }
            }
        }
    }
    (r, m)
}
impl PrecisionDocument {
    pub fn mask_edit(
        &self,
        edit: &MaskEdit,
        samples: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.mask_edit_with_context(edit, samples, limit, other, job)
    }
    /// The LOCK gate (`precision_lock`): adding, removing and re-describing
    /// a layer MASK is a `Mask` edit, which only Lock All blocks — Lock
    /// Pixels deliberately does not, so a pixel-locked layer's mask stays
    /// paintable (`doc_lock`'s module doc). APPLYING a mask
    /// is the one mask operation that writes the layer's ALPHA, so it has
    /// its own `MaskApply` gate in `precision_mask_apply`; the CHANNEL edits
    /// belong to no layer, so no lock addresses them.
    pub(crate) fn mask_edit_with_context(
        &self,
        edit: &MaskEdit,
        samples: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let gate = match edit {
            MaskEdit::SetMask { layer, .. }
            | MaskEdit::RemoveMask { layer }
            | MaskEdit::MaskProperties { layer, .. } => Some(*layer),
            MaskEdit::ApplyMask { .. }
            | MaskEdit::SetChannel { .. }
            | MaskEdit::RemoveChannel { .. } => None,
        };
        let Some(index) = gate else {
            return self.mask_edit_unlocked(edit, samples, limit, other, job);
        };
        self.under_locks(index, crate::doc_lock::EditKind::Mask, |doc| {
            doc.mask_edit_unlocked(edit, samples, limit, other, job)
        })
    }

    fn mask_edit_unlocked(
        &self,
        edit: &MaskEdit,
        samples: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        job.check()?;
        admit(self.retained_bytes(), 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let scratch = edit.scratch_bytes(self)?;
        if samples.len() != edit.expected_samples(self)? {
            return Err("typed mask edit sample count differs from target dimensions".into());
        }
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let result = match edit {
            MaskEdit::SetMask {
                layer,
                width,
                height,
                x,
                y,
                linked,
                density,
                feather,
                enabled,
                outside_coverage,
            } => {
                let raw = PrecisionPlane::copy(*width, *height, samples, job)?;
                let mut p = properties((*x, *y), *linked, *density, *feather);
                p.outside_coverage = *outside_coverage;
                let mask = PrecisionMask::new(raw, p, *enabled)?;
                self.set_mask_with_context(*layer, Some(mask), job)?
            }
            MaskEdit::RemoveMask { layer } => self.set_mask_with_context(*layer, None, job)?,
            MaskEdit::ApplyMask { layer } => self.apply_mask_with_context(*layer, job)?,
            MaskEdit::MaskProperties {
                layer,
                x,
                y,
                linked,
                density,
                feather,
                enabled,
            } => {
                let mask = self
                    .layer(*layer)
                    .and_then(PrecisionDocumentLayer::mask)
                    .ok_or("typed layer has no mask")?;
                let mut p = properties((*x, *y), *linked, *density, *feather);
                p.outside_coverage = mask.properties().outside_coverage;
                let next = mask.with_properties(p, *enabled)?;
                self.set_mask_with_context(*layer, Some(next), job)?
            }
            MaskEdit::SetChannel { index, name } => {
                let plane = PrecisionPlane::copy(self.width, self.height, samples, job)?;
                if let Some(i) = index {
                    let old = &self.channels[*i];
                    if old.name() == name && old.plane.equals(&plane, job)? {
                        job.report(1.0, 5)?;
                        return Ok(None);
                    }
                }
                let mut channels = self.channels.as_ref().clone();
                let mut channel = PrecisionChannel::new(name, plane)?;
                if let Some(i) = index {
                    channel.id = channels[*i].id;
                    channel.metadata = channels[*i].metadata.clone();
                    channels[*i] = channel;
                } else {
                    channels.push(channel);
                }
                Some(self.rebuilt(
                    self.width,
                    self.height,
                    self.layers.as_ref().clone(),
                    channels,
                )?)
            }
            MaskEdit::RemoveChannel { index } => {
                let mut channels = self.channels.as_ref().clone();
                channels.remove(*index);
                Some(self.rebuilt(
                    self.width,
                    self.height,
                    self.layers.as_ref().clone(),
                    channels,
                )?)
            }
        };
        job.report(1.0, 5)?;
        Ok(result)
    }
    pub fn set_layer_mask(
        &self,
        index: usize,
        mask: Option<PrecisionMask>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let mut inventory = crate::resources::Inventory::default();
        self.record_resources(&mut inventory);
        if let Some(m) = &mask {
            m.record_resources(&mut inventory);
        }
        admit(
            inventory.unique_bytes(),
            4 * BOOKKEEPING_BYTES,
            other,
            limit,
        )?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let result = self.set_mask_with_context(index, mask, job)?;
        job.report(1.0, 5)?;
        Ok(result)
    }
    pub(crate) fn set_mask_with_context(
        &self,
        index: usize,
        mask: Option<PrecisionMask>,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let original = self
            .layer(index)
            .ok_or("typed mask layer is out of range")?;
        let equal = match (&original.mask, &mask) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.enabled == b.enabled
                    && a.properties == b.properties
                    && a.plane.equals(&b.plane, job)?
            }
            _ => false,
        };
        if equal {
            return Ok(None);
        }
        let mut layer = original.clone();
        layer.mask = mask;
        self.replacing_layer(index, layer)
    }
}
