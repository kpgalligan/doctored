//! Image ▸ Mode ▸ 8/16 Bits/Channel on a Lab Color document (#547 Lab phase
//! 5a): the depth edit, its plan and its refusals. A Grayscale document's
//! depth op is the template (`gray_profile`); nothing here converts a
//! colour.
//!
//! The storage is the RGB precision document in the Lab twin, u16 only
//! (`lab_document`'s module doc), so the edit runs on
//! [`PrecisionDocument::without_lab`] and the result takes the new depth's
//! policy back by `with_lab`:
//!
//! * **16 → 8** puts every raster sample (L\*, a\*, b\* and alpha, each
//!   channel on its own), every mask sample and every alpha-channel sample
//!   on the 8-bit grid (`cmyk_profile::eight`, CMYK's and Gray's order:
//!   `map_cmyk_tree`, `channels_on_grid`, `rebuilt`, then
//!   `seeding_vector_masks`): each 16-bit code to its nearest 8-bit one,
//!   with no dither (Gray's rule).
//! * **8 → 16** changes only the declared depth: an 8-bit Lab document's
//!   codes already are 16-bit ones on the ×257 grid.
//! * The same depth is `None`. 32 bits refuses: there is no 32-bit Lab
//!   ([`DEPTH_REFUSAL`]).
//!
//! Adjustment records and layer styles carry unchanged: they act on the
//! codes whatever the depth. None of this is a format change: the `LAB `
//! table already holds the depth.
//!
//! # Memory
//!
//! The plan is the larger of the edit's stage — 16 → 8's rounded planes,
//! 8 B a raster pixel and 2 B a mask or alpha-channel sample; nothing for
//! 8 → 16 — and the result's display preparation the host runs right after
//! ([`DEPTH_BYTES_PER_PIXEL`] a raster pixel), plus bookkeeping. Held by
//! `scripts/lab-admission/measure.py` (`depth-lab-8`); a miss raises
//! [`DEPTH_BYTES_PER_PIXEL`].

use std::sync::Arc;

use serde_json::{json, Value};

use crate::cmyk_profile::{channels_on_grid, eight, map_cmyk_tree};
use crate::job_control::{DrJobControl, JobContext};
use crate::lab_document::{LabDepth, LabPolicy};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES};
use crate::tile_store::TilePlane;

/// Bytes a raster pixel the plan charges for the result's display
/// preparation: the Lab mode conversions' measured term
/// (`lab_convert::TO_LAB_BYTES_PER_PIXEL`, the same RGBA16 shape and view)
/// as this edit's own constant, so a miss here raises this term alone.
pub(crate) const DEPTH_BYTES_PER_PIXEL: u64 = 41;

/// The refusal of any depth but 8 or 16 (module doc).
const DEPTH_REFUSAL: &str = "A Lab Color document is 8 or 16 bits per channel.";

/// The refusal on any other document.
const NOT_LAB: &str = "the document is not Lab Color";

impl PrecisionDocument {
    /// The plan of a change to `bits` (module doc, "Memory"): its bytes, its
    /// refusal and whether it would change the document.
    fn lab_depth_scratch(&self, bits: u32) -> (u64, Option<String>, bool) {
        let refuse = |reason: &str| (BOOKKEEPING_BYTES, Some(reason.to_string()), false);
        let Some(policy) = self.lab_policy() else {
            return refuse(NOT_LAB);
        };
        let Some(depth) = LabDepth::from_bits(bits) else {
            return refuse(DEPTH_REFUSAL);
        };
        let from = policy.depth();
        if from == depth {
            return (BOOKKEEPING_BYTES, None, false);
        }
        let pixels = Self::gray_raster_pixels(self.root_layers());
        let display = pixels.saturating_mul(DEPTH_BYTES_PER_PIXEL);
        let stage = if depth == LabDepth::Eight {
            let (w, h) = self.dimensions();
            let planes = self.cmyk_mask_samples().saturating_add(
                (self.channels().len() as u64).saturating_mul(u64::from(w) * u64::from(h)),
            );
            pixels
                .saturating_mul(8)
                .saturating_add(planes.saturating_mul(2))
        } else {
            0
        };
        (
            stage.max(display).saturating_add(BOOKKEEPING_BYTES),
            None,
            true,
        )
    }

    /// `dr_precision_doc_lab_depth_plan`'s answer:
    /// `{"working_memory_plan_bytes","refusal","changes"}` (`changes` false
    /// = the edit would answer `None`).
    pub fn lab_depth_plan(&self, bits: u32) -> Result<Value, String> {
        let (bytes, refusal, changes) = self.lab_depth_scratch(bits);
        Ok(json!({"working_memory_plan_bytes":bytes,"refusal":refusal,"changes":changes}))
    }

    /// 8/16 Bits/Channel (module doc); `None` when `bits` is the declared
    /// depth.
    pub fn lab_depth_edit(
        &self,
        bits: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (scratch, refusal, changes) = self.lab_depth_scratch(bits);
        if let Some(reason) = refusal {
            return Err(reason);
        }
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let out = match LabDepth::from_bits(bits) {
            Some(depth) if changes => {
                let rgb = self.without_lab();
                let next = if depth == LabDepth::Eight {
                    rgb.on_lab_grid(job)?.seeding_vector_masks(
                        self.vector_mask_seeds(Clone::clone),
                        limit,
                        other,
                    )
                } else {
                    rgb
                };
                Some(next.with_lab(Arc::new(LabPolicy::for_depth(depth)))?)
            }
            _ => None,
        };
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// This u16 document with every raster sample, mask sample and
    /// alpha-channel sample on the 8-bit grid, groups rebuilt around their
    /// children (module doc, 16 → 8).
    fn on_lab_grid(&self, job: JobContext<'_>) -> Result<Self, String> {
        let layers = map_cmyk_tree(self.root_layers(), true, &mut |layer| {
            let Content::Raster { image, offset } = &layer.content else {
                return Ok(layer.clone());
            };
            let mut next: PrecisionDocumentLayer = layer.clone();
            next.content = Content::Raster {
                image: on_grid(image, job)?,
                offset: *offset,
            };
            Ok(next)
        })?;
        let channels = channels_on_grid(self.channels())?;
        self.rebuilt(self.width, self.height, layers, channels)
    }
}

/// One u16 raster with each of its four channels on the 8-bit grid; also
/// a 16-bit Lab payload pasted into an 8-bit Lab document
/// (`lab_clipboard::LabClipboardPayload::quantized_to`).
pub(crate) fn on_grid(
    image: &PrecisionImage,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = image.dimensions();
    pixel_count(w, h)?;
    let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
        w,
        h,
        |y, row| {
            let mut x = 0usize;
            for (_, run) in image.row_segments(y, 0, w) {
                let PrecisionRun::Rgba16(run) = run else {
                    return Err("a Lab Color depth change requires 16-bit storage".into());
                };
                for pixel in run {
                    row[x] = pixel.map(eight);
                    x += 1;
                }
            }
            job.every(y as usize * w as usize)
        },
        job,
    )?;
    PrecisionImage::from_rgba16_tiles_unseeded(plane, image.profile().clone())
}
