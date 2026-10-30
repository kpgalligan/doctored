//! The document side of Filter ▸ Neural Filters (M4), over
//! `neural_stage`'s images:
//!
//! - `super_zoom_document` — Super Zoom's output, a NEW document: one raster
//!   layer "Background" holding the zoomed image, in the source document's
//!   profile at its resolution (the print size grows with the pixels, as a
//!   resample to more pixels at the same ppi does), at 8 bits (`DrDocument`)
//!   or 16 (`PrecisionDocument`). The source is never changed. Colorize's New
//!   Document output (`neural_colorize_run`'s colorized canvas, the canvas's
//!   own size) reuses it unchanged.
//! - `neural_layer_apply` — JPEG Artifacts Removal's typed output: `pixels`
//!   (the layer's own size) replace the layer's raster in place, dropping any
//!   typed description (`replacing_content`), or land on a new raster layer
//!   `name` directly above it at the same offset. The byte output publishes
//!   through the host's existing layer-pixel exports.
//!
//! A float or CMYK document refuses by name (the menu and MCP gate both
//! filters away from them first). On a Grayscale document (#547 phase 6b)
//! the model's output, whose colour comes from outside the document, is
//! folded to its gray by the Mode rule (`gray_artwork::folded_result`) before
//! it lands: Super Zoom's new document is Gray in the source's policy, and
//! the layer keeps R = G = B. Both admit the fold's image
//! (`gray_artwork::fold_bytes`). On a Lab Color document (#547 Lab phase
//! 6b-iii) the model read the document's Display P3 view, so its output,
//! tagged Display P3 and never retagged here, returns to L\*a\*b\* codes by
//! the Mode rule (`lab_neural::published`): Super Zoom's new document is Lab
//! in the source's policy. Both admit that image and its tables
//! (`lab_neural::published_bytes`, Gray's fold on Gray).
use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::job_control::{DrJobControl, JobContext};
use crate::lab_neural::{published, published_bytes};
use crate::layer_insert::InsertPosition;
use crate::metadata::Metadata;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES};

const NOT_FLOAT: &str = "neural filters are not available in a 32-bit document";
const NOT_CMYK: &str = "neural filters are not available in a CMYK document";

/// The zoomed image's size, refusing an empty one or one past
/// `doc::MAX_PIXELS`.
fn zoomed_size(width: u32, height: u32) -> Result<(), String> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > crate::doc::MAX_PIXELS {
        return Err(format!(
            "the zoomed image would be {width} × {height} ({:.1} megapixels), past the \
             100-megapixel limit: choose a smaller zoom",
            pixels as f64 / 1e6
        ));
    }
    Ok(())
}

impl DrDocument {
    /// Super Zoom's new 8-bit document (module doc).
    pub fn super_zoom_document(&self, zoomed: &image::RgbaImage) -> Result<DrDocument, String> {
        let (w, h) = zoomed.dimensions();
        zoomed_size(w, h)?;
        let mut doc = DrDocument::from_pixels(zoomed.clone());
        doc.profile = self.profile.clone();
        doc.resolution = self.resolution;
        Ok(doc)
    }
}

impl PrecisionDocument {
    fn neural_depth(&self) -> Result<(), String> {
        if self.is_float() {
            return Err(NOT_FLOAT.into());
        }
        if self.is_cmyk() {
            return Err(NOT_CMYK.into());
        }
        Ok(())
    }

    /// Super Zoom's new 16-bit document (module doc), admitted against
    /// `limit` beside `other`.
    pub fn super_zoom_document(
        &self,
        zoomed: &PrecisionImage,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionDocument, String> {
        self.neural_depth()?;
        if zoomed.storage() != PrecisionStorage::Rgba16Unorm {
            return Err(NOT_FLOAT.into());
        }
        let (w, h) = zoomed.dimensions();
        zoomed_size(w, h)?;
        let _run = control.map(DrJobControl::begin).transpose()?;
        let fold = published_bytes(self, u64::from(w) * u64::from(h));
        admit(
            zoomed.retained_bytes().saturating_add(fold),
            BOOKKEEPING_BYTES,
            other,
            limit,
        )?;
        // The fold's allocation is admitted above and reserved here, as
        // `neural_layer_apply` reserves its own.
        let scratch = BOOKKEEPING_BYTES.saturating_add(fold);
        if let Some(control) = control {
            control.reserve(scratch)?;
        }
        let job = control.map_or(JobContext::NONE, JobContext::new);
        // A Lab result keeps its Display P3 tag for `published` (module doc).
        let zoomed = if self.is_lab() {
            zoomed.clone()
        } else {
            zoomed.with_profile(self.profile().clone())
        };
        let image = published(self, zoomed, job.with_reservation_floor(scratch))?;
        let doc = PrecisionDocument::from_image(image, Metadata::default(), self.resolution())?;
        let doc = match (self.gray_policy(), self.lab_policy()) {
            (Some(policy), _) => doc.with_gray(policy.clone())?,
            (None, Some(policy)) => doc.with_lab(policy.clone())?,
            (None, None) => doc,
        };
        if let Some(control) = control {
            control.report(1.0, 5);
        }
        Ok(doc)
    }

    /// JPEG Artifacts Removal's typed apply (module doc). `None` when the
    /// layer's pixels are locked or `pixels` equals its raster.
    #[allow(clippy::too_many_arguments)]
    pub fn neural_layer_apply(
        &self,
        layer: usize,
        pixels: &PrecisionImage,
        new_layer: bool,
        name: Option<&str>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<PrecisionDocument>, String> {
        self.neural_depth()?;
        let record = self
            .layer(layer)
            .ok_or_else(|| format!("neural filter layer {layer} is out of range"))?;
        let source = match record.source() {
            Some(source) if record.children().is_none() => source,
            _ => {
                return Err(format!(
                    "neural filters need a raster layer; layer {layer} is not one"
                ))
            }
        };
        if pixels.storage() != PrecisionStorage::Rgba16Unorm {
            return Err(NOT_FLOAT.into());
        }
        if pixels.dimensions() != source.dimensions() {
            return Err("the neural filter's pixels must be the layer's own size".into());
        }
        let offset = record.offset();
        let _run = control.map(DrJobControl::begin).transpose()?;
        let (w, h) = pixels.dimensions();
        let scratch =
            BOOKKEEPING_BYTES.saturating_add(published_bytes(self, u64::from(w) * u64::from(h)));
        admit(self.retained_bytes(), scratch, other, limit)?;
        if let Some(control) = control {
            control.reserve(scratch)?;
        }
        let job = control.map_or(JobContext::NONE, JobContext::new);
        // A Lab result keeps its Display P3 tag for `published` (module doc).
        let pixels = if self.is_lab() {
            pixels.clone()
        } else {
            pixels.with_profile(self.profile().clone())
        };
        let image = published(self, pixels, job.with_reservation_floor(scratch))?;
        let doc = if new_layer {
            let name = name.ok_or("a new neural filter layer needs a name")?;
            let slot = self.insert_slot(layer, InsertPosition::Above)?;
            self.preflight_raster_addition(image.dimensions(), name)?;
            let added = PrecisionDocumentLayer::raster(name, image, offset, 1.0, true)?;
            Some(self.inserting_record(added, slot)?)
        } else {
            if self.lock_block(layer, EditKind::Pixels) != 0 || source.same_samples(&image) {
                return Ok(None);
            }
            self.replacing_content(layer, Content::Raster { image, offset })?
        };
        if let Some(control) = control {
            control.report(1.0, 5);
        }
        Ok(doc)
    }
}
