//! MOVE SELECTED PIXELS and TRANSFORM SELECTED at 16 bits and at
//! 32-bit float: the typed plan and kernel over the byte module's ONE walk
//! (`selected_pixels::move_samples` → `moved_pixel`), with u16 storage and a single quantize, or
//! f32 storage and none (a filtered transform lands under `float_document`'s D3 clamp,
//! `selected_pixels_transform::landing_extended`). The request, the extent rule and the
//! measured contract live in `selected_pixels`. The publication goes through
//! `replacing_content`, which clears a described layer's description.
//!
//! # Admission
//!
//! The f32 arm charges [`FLOAT_SAMPLE_BYTES`] per output pixel (the u16 arm's 8 stays): the
//! window or grown plane at 16 bytes, the rebuilt tiles of the published image at 16 more, with
//! margin. Measured in Release on a 6000×4000 opaque float sRGB document (`float_adjust_apply_24mp`'s
//! RZCB ramps) with a canvas coverage of rect (300, 200, 5400, 3600) at 65535, through
//! `dr_precision_doc_edit` at limit 8,589,934,592 with the result held until E is read; one case
//! per process, `MallocLargeCache=0`, three runs. E = `ri_interval_max_phys_footprint` −
//! `ri_phys_footprint` read right after `proc_reset_footprint_interval`; A = the plan's
//! `scratch_bytes`. M = `move_selected` dx 37 dy 23 with `bounds` = that rect; T =
//! `transform_selected` 7° about the canvas centre, bilinear, the same `bounds`. The baseline twin
//! (same setup, no call) read E 0 for both. Apple M2 Max, 96 GiB, 2026-10-10.
//!
//! | case | A | E ×3 | A / max E |
//! |---|---|---|---|
//! | M | 1,260,750,352 | 667,730,424 · 667,714,040 · 667,697,656 | 1.888 |
//! | T | 1,800,546,304 | 1,059,095,296 · 1,059,078,912 · 1,059,095,296 | 1.700 |
//!
//! Both clear the ⌈1.25 × max E⌉ gate (834,663,030 and 1,323,869,120), which
//! `tests/float_selected_pixels_24mp.rs` pins.

use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, PrecisionImage, PrecisionStorage};
use crate::selected_pixels::{
    move_samples, plan_scratch, CanvasRect, MovedSamples, SelectedPixelsEdit, SelectedPixelsPlan,
    SelectedTarget,
};
use crate::selected_pixels_float::bounded_scratch;

/// Written bytes per pixel at f32 (window + rebuilt tiles, with margin); fitted by `# Admission`.
pub(crate) const FLOAT_SAMPLE_BYTES: u64 = 48;

/// The stored bytes per output pixel the plan charges: the u16 arm's 8, or [`FLOAT_SAMPLE_BYTES`].
pub(crate) fn sample_bytes(storage: PrecisionStorage) -> u64 {
    match storage {
        PrecisionStorage::Rgba16Unorm => 8,
        PrecisionStorage::Rgba32Float => FLOAT_SAMPLE_BYTES,
    }
}

impl PrecisionDocument {
    /// The mode's refusal of a plane target (#1): an ink plate on an RGB
    /// document, and Red, Green, Blue or Alpha on a CMYK one.
    fn plane_target_refusal(&self, edit: &SelectedPixelsEdit) -> Result<(), String> {
        let target = edit.target();
        if target.ink_plate().is_some() && !self.is_cmyk() {
            return Err(format!(
                "The {} channel requires a CMYK document",
                target.plane_name()
            ));
        }
        if target.component().is_some() && self.is_cmyk() {
            return Err(crate::cmyk_document::refused_sentence(&format!(
                "Moving or transforming the \"{}\" channel's selected pixels",
                target.plane_name()
            )));
        }
        Ok(())
    }

    /// The raster record and its canvas rect, or the named refusal.
    pub(crate) fn selected_pixels_target(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<(&PrecisionImage, CanvasRect), String> {
        let layer = edit.layer();
        let record = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = match &record.content {
            // A CMYK layer target routes to `cmyk_selected_pixels` first
            // (#1 phase 4b); only a colour-plane target reaches here.
            Content::CmykRaster { .. } => {
                return Err(crate::cmyk_document::refused_sentence(
                    "Moving or transforming one colour channel's selected pixels",
                ))
            }
            Content::Raster { image, .. } => image,
            Content::Adjustment(_) | Content::CmykAdjustment(_) => {
                return Err(edit.target_refusal(layer, "an adjustment layer"))
            }
            Content::Group(_) | Content::PassThrough(_) => {
                return Err(edit.target_refusal(layer, "a group"))
            }
        };
        let (w, h) = image.dimensions();
        let offset = record.offset();
        Ok((image, (i64::from(offset.0), i64::from(offset.1), w, h)))
    }

    /// Sample-free (a move's plan is delta-independent, a transform's landing
    /// term follows its geometry): index, raster target, storage and a
    /// transform's geometry. No lock check. The
    /// per-pixel term follows the raster's storage ([`sample_bytes`]).
    pub fn selected_pixels_plan(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<SelectedPixelsPlan, String> {
        self.plane_target_refusal(edit)?;
        if edit.target().ink_plate().is_some() {
            return self.cmyk_selected_plate_plan(edit);
        }
        if edit.target().component().is_some() {
            return self.selected_color_plane_plan(edit);
        }
        if edit.target() != SelectedTarget::Layer {
            return self.selected_plane_plan(edit);
        }
        if self.is_cmyk() {
            return self.cmyk_selected_pixels_plan(edit);
        }
        let (image, rect) = self.selected_pixels_target(edit)?;
        let bytes = sample_bytes(image.storage());
        let (w, h) = self.dimensions();
        let scratch_bytes = match edit.bounds() {
            Some(bounds) => bounded_scratch(edit, (w, h), rect, bounds, bytes)?,
            None => plan_scratch((w, h), rect, bytes)
                .saturating_add(edit.landing_scratch((w, h), rect)?),
        };
        Ok(SelectedPixelsPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes,
            input_samples: u64::from(w) * u64::from(h),
        })
    }

    /// The typed twin of `DrDocument::selected_pixels_edit`, same refusals in
    /// the same order.
    pub(crate) fn selected_pixels_with_context(
        &self,
        edit: &SelectedPixelsEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.plane_target_refusal(edit)?;
        if edit.target().ink_plate().is_some() {
            return self.cmyk_selected_plate_with_context(edit, coverage, limit, other, job);
        }
        if edit.target().component().is_some() {
            return self.selected_color_plane_with_context(edit, coverage, limit, other, job);
        }
        if edit.target() != SelectedTarget::Layer {
            return self.selected_plane_with_context(edit, coverage, limit, other, job);
        }
        if self.is_cmyk() {
            return self.cmyk_selected_pixels_with_context(edit, coverage, limit, other, job);
        }
        let canvas = self.dimensions();
        if coverage.len() as u64 != u64::from(canvas.0) * u64::from(canvas.1) {
            return Err("selected pixels coverage must be canvas-sized u16".into());
        }
        let index = edit.layer();
        let (image, rect) = self.selected_pixels_target(edit)?;
        if self.has_retained_source(index) {
            return Err(edit.retained_refusal());
        }
        self.lock_refusal(index, EditKind::Pixels)?;
        let Some(landing) = edit.landing(canvas)? else {
            return Ok(None);
        };
        let plan = self.selected_pixels_plan(edit)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let mut grown_comps = None;
        let profile = image.profile().clone();
        let published = match image.storage() {
            PrecisionStorage::Rgba16Unorm => {
                let moved = move_samples::<[u16; 4]>(
                    |x, y, w, h| image.read_rect_rgba16(x, y, w, h),
                    rect,
                    canvas,
                    coverage,
                    &landing,
                    edit.empty_area(),
                    edit.hole(),
                    edit.bounds(),
                    job,
                )?;
                match moved {
                    MovedSamples::Rect((x, y, w, h), samples) => image
                        .with_rect_rgba16(x, y, w, h, &samples)?
                        .map(|image| (image, (rect.0 as i32, rect.1 as i32))),
                    MovedSamples::Plane(offset, (w, h), samples) => {
                        pixel_count(w, h)?;
                        grown_comps = self.comps_after_growth(index, offset);
                        let image =
                            PrecisionImage::from_rgba16_unseeded(w, h, samples, profile, None)?;
                        Some((image, offset))
                    }
                }
            }
            PrecisionStorage::Rgba32Float => {
                let moved = move_samples::<[f32; 4]>(
                    |x, y, w, h| image.read_rect_rgba32(x, y, w, h),
                    rect,
                    canvas,
                    coverage,
                    &landing,
                    edit.empty_area(),
                    edit.hole(),
                    edit.bounds(),
                    job,
                )?;
                match moved {
                    MovedSamples::Rect((x, y, w, h), samples) => image
                        .with_rect_rgba32(x, y, w, h, &samples)?
                        .map(|image| (image, (rect.0 as i32, rect.1 as i32))),
                    MovedSamples::Plane(offset, (w, h), samples) => {
                        pixel_count(w, h)?;
                        grown_comps = self.comps_after_growth(index, offset);
                        let image =
                            PrecisionImage::from_rgba32_extended(w, h, samples, profile, None)?;
                        Some((image, offset))
                    }
                }
            }
        };
        let Some((image, offset)) = published else {
            return Ok(None);
        };
        let content = Content::Raster { image, offset };
        let mut result = self.replacing_content(index, content)?;
        if let (Some(next), Some(comps)) = (result.as_mut(), grown_comps) {
            next.comps = comps;
        }
        job.report(1.0, 5)?;
        Ok(result)
    }
}
