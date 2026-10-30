//! The Remove tool's model engine (M3a): the adapters over
//! `remove_model` at 8 bits (`DrDocument`) and 16 bits
//! (`PrecisionDocument`). The host calls them in one job, around the model:
//!
//! - `remove_model_plan` — the job's working set: the kernel's own figure
//!   (`remove_model::scratch_bytes`) plus the plate's term, which is the one
//!   `doc_content_aware` charges for the same sampling modes (byte: the
//!   canvas plate `4n` plus `inpaint_source::plate_scratch_bound` when the
//!   mode composites; typed: `retouch_source_plan`). `ignore_adjustments`
//!   only removes contributors, so the plan does not take it.
//! - `remove_model_input` — the sampling plate the mode names (every mode
//!   through the one plate builder each depth has: byte
//!   `inpaint_source::sampling_source`, typed `retouch_source_with_context`),
//!   read one crop row at a time into the native image and mask.
//! - `remove_model_apply` — the model's plane composited over the layer by
//!   the coverage, in place inside the layer's extent on the canvas (the
//!   paint tools' clip; the layer never grows), or onto a new canvas-sized
//!   raster layer "Remove" directly above it (`InsertPosition::Above`, as
//!   Sky Replacement's Duplicate Layer output inserts).
//!
//! Every call admits its plan against `limit` beside `other` and reserves it
//! on its job control before allocating; the inputs are never changed. A 32-bit
//! float or CMYK document refuses by name (`NOT_FLOAT`, `NOT_CMYK`): the model
//! runs on encoded RGB, so both keep refusing.
//!
//! # Grayscale
//!
//! A Gray document at 8 or 16 bits (#1 phase 2e; its 32 bits refuse as
//! float) feeds the model its R = G = B plate unchanged; the model's plane is
//! colour, so each composited pixel is folded back to one channel by the
//! filters' rule (`gray_filter::fold`: the value when the three agree bit for
//! bit, else the Rec. 709 luma) before it is coded.
//!
//! # Lab Color
//!
//! A Lab document at 8 or 16 bits (#1 Lab 2f, third part) feeds the model
//! what the person sees: each plate sample's L\*a\*b\* codes through the
//! document's own view (`lab_convert::view_pipeline`, the twin into Display
//! P3, clamped to 0…1, alpha kept), as Gray's model reads its view. The
//! model's Display P3 fill returns to Lab unit codes through
//! `lab_convert::from_view_pipeline` (clamped) before `composite`, so the
//! coverage mix is on the L\*a\*b\* values, as Normal compositing mixes Lab
//! codes. Each plan adds the pipeline's prepared tables.

use image::RgbaImage;

use crate::doc::{DrDocument, Layer, LayerRaster};
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::icc_intent::ColorPipeline;
use crate::inpaint_source::{plate_scratch_bound, sampling_source};
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::InsertPosition;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, reserved_vec, PrecisionImage};
use crate::remove_model::{
    check_plane, check_side, composite, covered_rect, crop, fill_at, native_input, scratch_bytes,
    Rect, NO_COVERAGE,
};
use crate::retouch_brush_sampling::plate_samples;
use crate::retouch_sampling::{is_adjustment, RetouchSampling};

/// The new layer's name.
pub(crate) const LAYER_NAME: &str = "Remove";

fn out_of_range(layer: usize) -> String {
    format!("remove layer {layer} is out of range")
}

fn not_raster(layer: usize) -> String {
    format!("remove needs a raster layer; layer {layer} is not one")
}

const NOT_FLOAT: &str = "remove's model is not available in a 32-bit float document";
const NOT_CMYK: &str = "remove's model is not available in a CMYK document";

/// `coverage` holds exactly `n` samples, else the refusal.
fn coverage_length(coverage: &[u16], n: u64) -> Result<(), String> {
    if coverage.len() as u64 != n {
        return Err("invalid remove coverage buffer".into());
    }
    Ok(())
}

/// `plane` holds the model's `3·side²` samples, all finite.
fn plane_checked(plane: &[f32], side: u32, job: JobContext<'_>) -> Result<(), String> {
    if plane.len() as u64 != 3 * u64::from(side) * u64::from(side) {
        return Err("invalid remove model plane".into());
    }
    check_plane(plane, job)
}

/// The covered rect and the crop around it, `Err(NO_COVERAGE)` when nothing
/// is covered.
fn hole(
    coverage: &[u16],
    canvas: (u32, u32),
    side: u32,
    job: JobContext<'_>,
) -> Result<(Rect, Rect), String> {
    let covered = covered_rect(coverage, canvas.0, job)?.ok_or(NO_COVERAGE)?;
    Ok((covered, crop(covered, canvas, side)))
}

/// The part of `covered` (canvas) inside a `size` layer at `offset`, in
/// layer coordinates; `None` when they miss.
fn layer_part(covered: Rect, offset: (i32, i32), size: (u32, u32)) -> Option<Rect> {
    let (x, y, w, h) = covered;
    let x0 = (i64::from(x) - i64::from(offset.0)).max(0);
    let y0 = (i64::from(y) - i64::from(offset.1)).max(0);
    let x1 = (i64::from(x) + i64::from(w) - i64::from(offset.0)).min(i64::from(size.0));
    let y1 = (i64::from(y) + i64::from(h) - i64::from(offset.1)).min(i64::from(size.1));
    (x0 < x1 && y0 < y1).then(|| (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

/// The composite of one covered canvas pixel at layer-rect index `k` of
/// `rect` (layer coordinates, at `offset`): `None` for coverage 0, else the
/// new straight unit pixel over `under`. `frozen` (a transparency lock)
/// keeps `under`'s alpha, and leaves an empty pixel empty. `fill_space`, on
/// a Lab document, carries the model's Display P3 fill into Lab codes
/// (module doc, "Lab Color"); the byte callers pass `None`.
#[allow(clippy::too_many_arguments)]
fn covered_pixel(
    k: usize,
    rect: Rect,
    offset: (i32, i32),
    canvas_width: u32,
    coverage: &[u16],
    plane: &[f32],
    side: u32,
    crop: Rect,
    under: [f64; 4],
    frozen: bool,
    fill_space: Option<&ColorPipeline>,
) -> Option<[f64; 4]> {
    let (rx, ry, rw, _) = rect;
    let lx = rx + (k % rw as usize) as u32;
    let ly = ry + (k / rw as usize) as u32;
    let px = (i64::from(lx) + i64::from(offset.0)) as u32;
    let py = (i64::from(ly) + i64::from(offset.1)) as u32;
    let c = coverage[py as usize * canvas_width as usize + px as usize];
    if c == 0 || (frozen && under[3] <= 0.0) {
        return None;
    }
    let mut fill = fill_at(plane, side, crop, px, py);
    if let Some(pipeline) = fill_space {
        let lab = pipeline.color(fill.map(|v| v as f32));
        fill = lab.map(|v| f64::from(v).clamp(0.0, 1.0));
    }
    let mut out = composite(f64::from(c) / 65535.0, fill, under);
    if frozen {
        out[3] = under[3];
    }
    Some(out)
}

fn begin<'a>(
    control: Option<&'a DrJobControl>,
) -> Result<(Option<crate::job_control::JobRun<'a>>, JobContext<'a>), String> {
    let run = control.map(DrJobControl::begin).transpose()?;
    Ok((run, control.map_or(JobContext::NONE, JobContext::new)))
}

impl DrDocument {
    /// Entry `layer` when it is a raster layer that is no adjustment.
    fn remove_target(&self, layer: usize) -> Result<&Layer, String> {
        let entry = self.layers.get(layer).ok_or_else(|| out_of_range(layer))?;
        if self.raster_layer(layer).is_none() || is_adjustment(entry) {
            return Err(not_raster(layer));
        }
        Ok(entry)
    }

    fn remove_retained(&self) -> u64 {
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        inventory.unique_bytes()
    }

    /// The Remove model job's working set on `layer` (module doc). `Err` for
    /// an out-of-range or non-raster layer, a bad side and a metadata parse
    /// the plate bound refuses.
    pub fn remove_model_plan(
        &self,
        layer: usize,
        mode: RetouchSampling,
        side: u32,
        new_layer: bool,
    ) -> Result<u64, String> {
        self.remove_target(layer)?;
        check_side(side)?;
        let n = u64::from(self.width) * u64::from(self.height);
        let plate = if mode.is_layer_only() {
            0
        } else {
            plate_scratch_bound(self)?
        };
        Ok(scratch_bytes(self.width, self.height, side, 4, new_layer)
            .saturating_add(n.saturating_mul(4))
            .saturating_add(plate))
    }

    /// The model's native input for the canvas-sized `coverage` on `layer`:
    /// writes `image_out` and `mask_out` (`remove_model::native_input`) and
    /// answers the crop `[x, y, w, h]`. `Err` for the module doc's refusals,
    /// a refused plan, nothing to sample and a cancel.
    #[allow(clippy::too_many_arguments)]
    pub fn remove_model_input(
        &self,
        layer: usize,
        mode: RetouchSampling,
        ignore_adjustments: bool,
        coverage: &[u16],
        side: u32,
        image_out: &mut [f32],
        mask_out: &mut [f32],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<[u32; 4], String> {
        self.remove_target(layer)?;
        check_side(side)?;
        let canvas = (self.width, self.height);
        coverage_length(coverage, u64::from(canvas.0) * u64::from(canvas.1))?;
        let plan = self.remove_model_plan(layer, mode, side, false)?;
        let (_run, job) = begin(control)?;
        let (_, crop) = hole(coverage, canvas, side, job)?;
        admit(self.remove_retained(), plan, other, limit)?;
        job.reserve(plan)?;
        let job = job.with_reservation_floor(plan);
        let plate_term = if mode.is_layer_only() {
            0
        } else {
            plate_scratch_bound(self)?
        };
        let plate = sampling_source(
            self,
            layer,
            mode,
            ignore_adjustments,
            plan - plate_term,
            job.range(0.0, 0.5),
        )?
        .ok_or_else(|| not_raster(layer))?;
        let (cx, cy, cw, _) = crop;
        let stride = canvas.0 as usize * 4;
        let raw = plate.as_raw();
        let row = |r: u32| {
            let at = (cy + r) as usize * stride + cx as usize * 4;
            Ok(raw[at..at + cw as usize * 4]
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2], p[3]].map(|v| f32::from(v) / 255.0))
                .collect())
        };
        native_input(
            coverage,
            canvas.0,
            crop,
            side,
            row,
            image_out,
            mask_out,
            job.range(0.5, 1.0),
        )?;
        job.report(1.0, 5)?;
        Ok([crop.0, crop.1, crop.2, crop.3])
    }

    /// The model's `plane` composited over `layer` by `coverage` (module
    /// doc): `Ok(None)` when no sample changed (or the layer is locked);
    /// `Err` for the module doc's refusals, a refused plan and a cancel.
    #[allow(clippy::too_many_arguments)]
    pub fn remove_model_apply(
        &self,
        layer: usize,
        coverage: &[u16],
        side: u32,
        plane: &[f32],
        new_layer: bool,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<DrDocument>, String> {
        let target = self.remove_target(layer)?;
        check_side(side)?;
        let canvas = (self.width, self.height);
        coverage_length(coverage, u64::from(canvas.0) * u64::from(canvas.1))?;
        let (_run, job) = begin(control)?;
        plane_checked(plane, side, job)?;
        let (covered, crop) = hole(coverage, canvas, side, job)?;
        let plan = scratch_bytes(canvas.0, canvas.1, side, 4, new_layer);
        admit(self.remove_retained(), plan, other, limit)?;
        job.reserve(plan)?;
        if new_layer {
            let slot = self.insert_slot(layer, InsertPosition::Above)?;
            if !self.native_layers_fit_dimensions(
                self.layers
                    .iter()
                    .map(|l| l.pixels.dimensions())
                    .chain(std::iter::once(canvas)),
            ) {
                return Err("the edited document exceeds native capacity".into());
            }
            let mut image = RgbaImage::new(canvas.0, canvas.1);
            let size = covered.2 as usize * covered.3 as usize;
            let mut any = false;
            for k in 0..size {
                job.every(k)?;
                let Some(out) = covered_pixel(
                    k,
                    covered,
                    (0, 0),
                    canvas.0,
                    coverage,
                    plane,
                    side,
                    crop,
                    [0.0; 4],
                    false,
                    None,
                ) else {
                    continue;
                };
                let bytes = out.map(|v| (v * 255.0).round() as u8);
                any |= bytes[3] > 0;
                let (x, y) = (
                    covered.0 + (k % covered.2 as usize) as u32,
                    covered.1 + (k / covered.2 as usize) as u32,
                );
                image.put_pixel(x, y, image::Rgba(bytes));
            }
            if !any {
                return Ok(None);
            }
            let added = Layer::from_raster(LayerRaster::unseeded(image), LAYER_NAME);
            let doc = self
                .inserting_layer(added, slot)
                .ok_or("remove could not insert its layer")?;
            job.report(1.0, 5)?;
            return Ok(Some(doc));
        }
        let Some(rect) = layer_part(covered, target.offset, target.pixels.dimensions()) else {
            return Ok(None);
        };
        let (rx, ry, rw, rh) = rect;
        let mut pixels = target
            .pixels
            .read_rect_with_job(rx, ry, rw, rh, job)?
            .ok_or("could not read the remove layer")?;
        for (k, pixel) in pixels.iter_mut().enumerate() {
            job.every(k)?;
            let under = pixel.map(|v| f64::from(v) / 255.0);
            if let Some(out) = covered_pixel(
                k,
                rect,
                target.offset,
                canvas.0,
                coverage,
                plane,
                side,
                crop,
                under,
                false,
                None,
            ) {
                *pixel = out.map(|v| (v * 255.0).round() as u8);
            }
        }
        let Some(raster) = target
            .pixels
            .with_rect_with_job(rx, ry, rw, rh, &pixels, job)?
        else {
            return Ok(None);
        };
        drop(pixels);
        // The lock gate and the transparency lock's frozen alpha live in
        // the funnel every byte pixel writer ends in.
        let doc = self.with_layer_raster(layer, raster);
        job.report(1.0, 5)?;
        Ok(doc)
    }
}

impl PrecisionDocument {
    /// The record of entry `layer`, refusing a float or CMYK document, an
    /// out-of-range index and a group or adjustment record by name.
    fn remove_target(&self, layer: usize) -> Result<&PrecisionDocumentLayer, String> {
        // The depth first, as Sky Replacement's: a CMYK record keeps its
        // planes outside `source()`, so it would otherwise read as no raster.
        if self.is_float() {
            return Err(NOT_FLOAT.into());
        }
        if self.is_cmyk() {
            return Err(NOT_CMYK.into());
        }
        let record = self.layer(layer).ok_or_else(|| out_of_range(layer))?;
        if record.children().is_some() || record.source().is_none() {
            return Err(not_raster(layer));
        }
        Ok(record)
    }

    /// The 16-bit twin of [`DrDocument::remove_model_plan`]: the plate term
    /// is `retouch_source_plan`; a Lab document adds its view pipeline's
    /// tables (module doc, "Lab Color").
    pub fn remove_model_plan(
        &self,
        layer: usize,
        mode: RetouchSampling,
        side: u32,
        new_layer: bool,
    ) -> Result<u64, String> {
        self.remove_target(layer)?;
        check_side(side)?;
        let (w, h) = self.dimensions();
        let view = if self.is_lab() {
            crate::lab_convert::view_pipeline()?.scratch_bytes()
        } else {
            0
        };
        Ok(scratch_bytes(w, h, side, 8, new_layer)
            .saturating_add(self.retouch_source_plan(layer, mode)?)
            .saturating_add(view))
    }

    /// The 16-bit twin of [`DrDocument::remove_model_input`].
    #[allow(clippy::too_many_arguments)]
    pub fn remove_model_input(
        &self,
        layer: usize,
        mode: RetouchSampling,
        ignore_adjustments: bool,
        coverage: &[u16],
        side: u32,
        image_out: &mut [f32],
        mask_out: &mut [f32],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<[u32; 4], String> {
        self.remove_target(layer)?;
        check_side(side)?;
        let canvas = self.dimensions();
        coverage_length(coverage, u64::from(canvas.0) * u64::from(canvas.1))?;
        let plan = self.remove_model_plan(layer, mode, side, false)?;
        let (_run, job) = begin(control)?;
        let (_, crop) = hole(coverage, canvas, side, job)?;
        admit(self.retained_bytes(), plan, other, limit)?;
        job.reserve(plan)?;
        let job = job.with_reservation_floor(plan);
        let plate = self.retouch_source_with_context(
            layer,
            mode,
            ignore_adjustments,
            limit,
            other,
            job.range(0.0, 0.5),
        )?;
        let (cx, cy, cw, _) = crop;
        // A Lab plate's codes through the document's view (module doc).
        let view = if self.is_lab() {
            Some(crate::lab_convert::view_pipeline()?)
        } else {
            None
        };
        let row = |r: u32| {
            let mut samples = plate_samples(&plate, (cx, cy + r, cw, 1))?;
            if let Some(view) = &view {
                crate::lab_neural::view_samples(view, &mut samples);
            }
            Ok(samples)
        };
        native_input(
            coverage,
            canvas.0,
            crop,
            side,
            row,
            image_out,
            mask_out,
            job.range(0.5, 1.0),
        )?;
        job.report(1.0, 5)?;
        Ok([crop.0, crop.1, crop.2, crop.3])
    }

    /// The 16-bit twin of [`DrDocument::remove_model_apply`]: the new record
    /// is a u16 raster in the document's profile. A Lab document converts
    /// the model's fill into Lab codes first, and its plan adds that
    /// pipeline's tables (module doc, "Lab Color").
    #[allow(clippy::too_many_arguments)]
    pub fn remove_model_apply(
        &self,
        layer: usize,
        coverage: &[u16],
        side: u32,
        plane: &[f32],
        new_layer: bool,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let record = self.remove_target(layer)?;
        check_side(side)?;
        let canvas = self.dimensions();
        coverage_length(coverage, u64::from(canvas.0) * u64::from(canvas.1))?;
        let (_run, job) = begin(control)?;
        plane_checked(plane, side, job)?;
        let (covered, crop) = hole(coverage, canvas, side, job)?;
        let fill_space = if self.is_lab() {
            Some(crate::lab_convert::from_view_pipeline()?)
        } else {
            None
        };
        let plan = scratch_bytes(canvas.0, canvas.1, side, 8, new_layer)
            .saturating_add(fill_space.as_ref().map_or(0, ColorPipeline::scratch_bytes));
        admit(self.retained_bytes(), plan, other, limit)?;
        job.reserve(plan)?;
        let code = |v: f64| (v * 65535.0).round() as u16;
        // A Gray document's composited pixel folds back to one channel by the
        // filters' rule (module doc, "Grayscale"); it allocates nothing.
        let gray = self.is_gray();
        let codes_of = |out: [f64; 4]| {
            if gray {
                crate::gray_filter::fold(out.map(|v| v as f32), false).map(|v| code(f64::from(v)))
            } else {
                out.map(code)
            }
        };
        if new_layer {
            let slot = self.insert_slot(layer, InsertPosition::Above)?;
            self.preflight_raster_addition(canvas, LAYER_NAME)?;
            let n = canvas.0 as usize * canvas.1 as usize;
            let mut codes: Vec<[u16; 4]> = reserved_vec(n)?;
            codes.resize(n, [0; 4]);
            let size = covered.2 as usize * covered.3 as usize;
            let mut any = false;
            for k in 0..size {
                job.every(k)?;
                let Some(out) = covered_pixel(
                    k,
                    covered,
                    (0, 0),
                    canvas.0,
                    coverage,
                    plane,
                    side,
                    crop,
                    [0.0; 4],
                    false,
                    fill_space.as_ref(),
                ) else {
                    continue;
                };
                let pixel = codes_of(out);
                any |= pixel[3] > 0;
                let (x, y) = (
                    covered.0 as usize + k % covered.2 as usize,
                    covered.1 as usize + k / covered.2 as usize,
                );
                codes[y * canvas.0 as usize + x] = pixel;
            }
            if !any {
                return Ok(None);
            }
            let image = PrecisionImage::from_rgba16_unseeded(
                canvas.0,
                canvas.1,
                codes,
                self.profile().clone(),
                None,
            )?;
            let added = PrecisionDocumentLayer::raster(LAYER_NAME, image, (0, 0), 1.0, true)?;
            let doc = self.inserting_record(added, slot)?;
            job.report(1.0, 5)?;
            return Ok(Some(doc));
        }
        if self.lock_block(layer, EditKind::Pixels) != 0 {
            return Ok(None);
        }
        let frozen = self.lock_owner(layer, LOCK_TRANSPARENCY).is_some();
        let image = record.source().ok_or_else(|| not_raster(layer))?;
        let offset = record.offset();
        let Some(rect) = layer_part(covered, offset, image.dimensions()) else {
            return Ok(None);
        };
        let (rx, ry, rw, rh) = rect;
        let mut pixels = image
            .read_rect_rgba16(rx, ry, rw, rh)
            .ok_or("could not read the remove layer")?;
        for (k, pixel) in pixels.iter_mut().enumerate() {
            job.every(k)?;
            let under = pixel.map(|v| f64::from(v) / 65535.0);
            if let Some(out) = covered_pixel(
                k,
                rect,
                offset,
                canvas.0,
                coverage,
                plane,
                side,
                crop,
                under,
                frozen,
                fill_space.as_ref(),
            ) {
                *pixel = codes_of(out);
            }
        }
        let Some(image) = image.with_rect_rgba16(rx, ry, rw, rh, &pixels)? else {
            return Ok(None);
        };
        drop(pixels);
        job.check()?;
        let doc = self.replacing_content(layer, Content::Raster { image, offset })?;
        job.report(1.0, 5)?;
        Ok(doc)
    }
}
