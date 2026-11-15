//! The Background Eraser's coverage on a typed document (16-bit and 32-bit float RGB,
//! Grayscale and Duotone, Lab, CMYK and Indexed): `background_eraser`'s field over the
//! stroke's footprint as a canvas-sized u16 plane, which the host hands to `erase_native`
//! as its coverage, the selection beside it, exactly as the Magic Eraser hands it the
//! wand's region. This adapter erases nothing.
//!
//! It reads the layer's codes where the Magic Wand reads them on each document: the stored
//! u16 codes (RGB, Gray, Duotone, Indexed; a Lab document's codes compared by
//! `lab_select`'s key), a float layer's R1 display codes (`display_encoding::display_code`,
//! so a value above white compares as white), and a CMYK layer's ink as its naive RGB
//! (`selection_region::ink_as_rgb`) at the document's depth. A reference colour reaches the
//! same codes the way the document's own paint reaches its ink: `precision_paint::
//! prepare_colors` (greyed on a Gray document, Lab codes on a Lab one), a float document's
//! `float_paint_ink::linear_ink` then its display code, a CMYK document's ink through
//! `ink_as_rgb`. A Bitmap document refuses by name.
use serde_json::{json, Value};

use crate::background_eraser::{
    field, placed, plan, BackgroundEraserRequest, LikeRule, Reference, ReferenceCodes,
};
use crate::brush_coverage::BrushRequest;
use crate::display_encoding::display_code;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, reserved_vec, unit_to_u16, PrecisionRun};
use crate::precision_paint::PaintColorSpace;
use crate::selection_region::ink_as_rgb;

/// A layer the tool cannot erase: a group, an adjustment layer or a described one.
const RASTER_REFUSAL: &str = "The Background Eraser erases a raster layer's pixels";

impl PrecisionDocument {
    /// The Bitmap refusal, then the target layer's refusal, then the stroke's
    /// plan (`background_eraser::plan`).
    fn background_eraser_checked(
        &self,
        layer: usize,
        brush: &BrushRequest,
        dabs: &[f64],
    ) -> Result<crate::background_eraser::Plan, String> {
        if self.is_bitmap() {
            return Err(crate::bitmap_document::refused_sentence(
                "background_eraser",
            ));
        }
        let record = self
            .layer(layer)
            .ok_or("background eraser target is out of range")?;
        if record.source().is_none() && record.cmyk_source().is_none() {
            return Err(RASTER_REFUSAL.into());
        }
        plan(brush, self.dimensions(), dabs)
    }

    /// Plans [`Self::background_eraser_coverage`]: JSON with `retained_bytes`,
    /// `scratch_bytes` (the kernel's, the canvas-sized output, and the
    /// reference conversion's), `output_samples` (the canvas pixel count) and
    /// `work_units`. Refuses, before allocating, what the run refuses.
    pub(crate) fn background_eraser_coverage_plan(
        &self,
        layer: usize,
        request: &BackgroundEraserRequest,
        brush: &BrushRequest,
        dabs: &[f64],
    ) -> Result<Value, String> {
        let planned = self.background_eraser_checked(layer, brush, dabs)?;
        // The colours refuse here, as the run would.
        self.background_eraser_references(request)?;
        let n = pixel_count(self.width, self.height)? as u64;
        let srgb = [request.swatch, request.protect].iter().flatten().any(|r| {
            r.color.is_some() && matches!(r.color_space, None | Some(PaintColorSpace::Srgb))
        });
        let conversion = if srgb {
            crate::precision_paint::srgb_ink_scratch(self.profile())?
        } else {
            0
        };
        Ok(json!({
            "retained_bytes": self.retained_bytes(),
            "scratch_bytes": planned.scratch_bytes + n * 2 + conversion,
            "output_samples": n,
            "work_units": planned.work_units,
        }))
    }

    /// The canvas-sized u16 coverage of the Background Eraser along `dabs` on
    /// raster `layer` (module doc), 0 outside the stroke. `Err`: a Bitmap
    /// document, an out-of-range or non-raster layer, a bad request or dab
    /// list, a reference colour of the wrong kind for the document, the
    /// stroke past 256 million work units, a refused reservation, a cancel.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn background_eraser_coverage(
        &self,
        layer: usize,
        request: &BackgroundEraserRequest,
        brush: &BrushRequest,
        dabs: &[f64],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let plan = self.background_eraser_coverage_plan(layer, request, brush, dabs)?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let dimensions = self.dimensions();
        let footprint = crate::brush_coverage::footprint(dimensions, dabs);
        let references = self.background_eraser_references(request)?;
        let n = pixel_count(dimensions.0, dimensions.1)?;
        let mut out = reserved_vec(n)?;
        out.resize(n, 0u16);
        let Some(footprint) = footprint else {
            job.report(1., 5)?;
            return Ok(out);
        };
        let record = self
            .layer(layer)
            .ok_or("background eraser target is out of range")?;
        let read = job.range(0., 0.1);
        let samples = if let Some(image) = record.cmyk_source() {
            let depth = self
                .require_cmyk_policy("The Background Eraser")?
                .depth
                .bits();
            placed(
                footprint,
                image.dimensions(),
                record.offset(),
                |ly, x0, x1, out| {
                    for (_, run) in image.row_segments(ly, x0, x1) {
                        out.extend(run.iter().map(|p| ink_as_rgb(*p, depth)));
                    }
                    Ok(())
                },
                read,
            )?
        } else {
            let image = record.source().ok_or(RASTER_REFUSAL)?;
            let float = self.float.as_deref();
            placed(
                footprint,
                image.dimensions(),
                record.offset(),
                |ly, x0, x1, out| {
                    for (_, run) in image.row_segments(ly, x0, x1) {
                        match (run, float) {
                            (PrecisionRun::Rgba16(run), _) => out.extend_from_slice(run),
                            (PrecisionRun::Rgba32(run), Some(policy)) => {
                                out.extend(run.iter().map(|p| display_code(policy.encoder(), *p)))
                            }
                            (PrecisionRun::Rgba32(run), None) => {
                                out.extend(run.iter().map(|p| p.map(unit_to_u16)))
                            }
                        }
                    }
                    Ok(())
                },
                read,
            )?
        };
        let rule = self.lab_grid().map_or(LikeRule::Rgb, LikeRule::Lab);
        let coverage = field(
            request,
            brush,
            dimensions,
            dabs,
            footprint,
            &samples,
            rule,
            references,
            job.range(0.1, 0.95),
        )?;
        drop(samples);
        let [fx0, fy0, fx1, _] = footprint;
        let fw = fx1 - fx0;
        let width = dimensions.0 as usize;
        for (k, value) in coverage.iter().enumerate() {
            job.every(k)?;
            out[(fy0 + k / fw) * width + fx0 + k % fw] = brush.quantize(*value);
        }
        job.report(1., 5)?;
        Ok(out)
    }

    /// The request's swatch and protect colours as this document's compare
    /// codes (module doc).
    fn background_eraser_references(
        &self,
        request: &BackgroundEraserRequest,
    ) -> Result<ReferenceCodes, String> {
        Ok(ReferenceCodes {
            swatch: request
                .swatch
                .as_ref()
                .map(|r| self.background_eraser_codes(r))
                .transpose()?,
            protect: request
                .protect
                .as_ref()
                .map(|r| self.background_eraser_codes(r))
                .transpose()?,
        })
    }

    fn background_eraser_codes(&self, reference: &Reference) -> Result<[u16; 4], String> {
        let unit = |v: &f32| v.is_finite() && (0.0..=1.0).contains(v);
        if self.is_cmyk() {
            let depth = self
                .require_cmyk_policy("The Background Eraser")?
                .depth
                .bits();
            return match (reference.ink, reference.color) {
                (Some(ink), None) if ink.iter().all(unit) => {
                    Ok(ink_as_rgb(ink.map(|v| (v * 65535.).round() as u16), depth))
                }
                _ => Err(
                    "a CMYK document's background eraser colours are ink: [c, m, y, k, alpha] in 0…1"
                        .into(),
                ),
            };
        }
        let (Some(color), None) = (reference.color, reference.ink) else {
            return Err("a background eraser colour is straight RGBA with its color_space".into());
        };
        let space = reference.color_space.unwrap_or(PaintColorSpace::Document);
        if let Some(policy) = self.float.as_deref() {
            if !matches!(space, PaintColorSpace::Linear) && !color.iter().all(unit) {
                return Err("background eraser colours must be finite in [0,1]".into());
            }
            let linear = crate::float_paint_ink::linear_ink(color, space, self)?;
            return Ok(display_code(policy.encoder(), linear));
        }
        if matches!(space, PaintColorSpace::Linear) {
            return Err(crate::float_paint::LINEAR_ONLY.into());
        }
        if !color.iter().all(unit) {
            return Err("background eraser colours must be finite in [0,1]".into());
        }
        let prepared = crate::precision_paint::prepare_colors(&[color], space, self)?;
        Ok(prepared[0].map(unit_to_u16))
    }
}
