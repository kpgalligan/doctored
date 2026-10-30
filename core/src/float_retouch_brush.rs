//! The Blur, Sharpen, Sponge and Smudge brushes and the History Brush in a
//! 32-bit float document: the f32 arm of the three typed adapters
//! (`RetouchBrushEdit`, `PrecisionDocument::smudge` and `history_brush`).
//!
//! `precision_retouch_brush` and `precision_smudge` are the MASTERS of every
//! step here but the sample type: the plan, `admit`, `job.reserve`, the
//! input-length check, the coverage rect or the stroke's footprint, the
//! `Ok(None)` cases and the progress are theirs, mirrored in order. Their
//! entry points hand a float document here at their top, after the lock gate.
//! The arithmetic is the shared kernels', called and never copied:
//! `retouch_brush::targets_in` / `mix` and `retouch_smudge::smudge`. Each walk
//! reads its rect with `read_rect_rgba32` and writes it back through
//! `with_rect_rgba32`, never grows the layer, and answers `Ok(None)` when no
//! stored bit changes.
//!
//! # What each brush does at f32
//!
//! The samples are straight LINEAR light and keep their range: colour is never
//! clamped to 0…1, and the only clamp is the stored bound ±`FLOAT_RANGE_MAX`.
//! Alpha and coverage stay in 0…1. A non-finite sample refuses through the
//! f32 writer; nothing repairs it.
//!
//! - **Blur, Sharpen**: `targets_in` at `SampleRange::Extended`, so the
//!   Gaussian is the float FIR (tails under 1e-7 of the weight) and Sharpen's
//!   overshoot is kept: a blur beside a pixel far above white lifts its
//!   neighbours above 1, and a sharpened edge may go below 0, which the layer
//!   stores. A Sharpen target past ±65504 is written as the bound. Alpha is
//!   never written, and alpha-0 and coverage-0 pixels keep their bits.
//! - **Sponge** (`saturate`, `desaturate`, `display_encoding`'s R5):
//!   `targets_in` at `SampleRange::Extended` over the linear samples, so
//!   Desaturate writes the luma and Saturate `Y + 2·(rgb − Y)` with no clamp
//!   to 0…1, held to ±65504 by the one clamp below.
//! - **Smudge**: the premultiplied f32 field through `retouch_smudge::smudge`
//!   unchanged, so a value above white is carried along the stroke as it is.
//!   The field is written back with no quantizer: a sample the walk left alone
//!   keeps its bits; alpha is clamped to 0…1 and, below
//!   `sampling::EXTENDED_ALPHA_FLOOR`, the pixel is transparent black (or
//!   stays as it was when it already held nothing); colour is `sample / alpha`
//!   clamped to the stored bound. Under a transparency lock alpha is kept,
//!   alpha-0 pixels stay, and a sample smeared below the floor leaves the
//!   colour alone (`precision_smudge`'s frozen rules, the floor standing in
//!   for its zero).
//! - **History Brush**, `t = coverage · opacity`: a pixel whose bits already
//!   equal the source's is skipped; `t ≥ 1` takes the source sample's bits,
//!   so a full-coverage stroke at opacity 1 restores the state exactly,
//!   above-white, negative and partly transparent samples alike (transparent
//!   black where the source layer holds nothing); below 1 it is the
//!   premultiplied mix in f64 rounded once to f32
//!   (`doc_retouch_brush.rs` holds the master copy of that mix; this is its
//!   f32 twin), transparent black when the mixed alpha is under the floor.
//!   Under a transparency lock alpha is kept and alpha-0 pixels stay. The
//!   source is a float document's f32 raster of the same canvas size.
//!
//! # Admission
//!
//! The plans are the masters' at f32 width (`float_paint::plan_in`'s walk
//! figure, `8·BOOKKEEPING + n·48 + output·16`, in place of the u16 walk's
//! `n·24 + output·8`), plus each brush's own per-pixel term:
//! [`FLOAT_RETOUCH_BRUSH_BYTES_PER_PIXEL`] and
//! [`FLOAT_HISTORY_BRUSH_BYTES_PER_PIXEL`] over the layer. Smudge's plan is
//! not the walk's: it is [`FLOAT_SMUDGE_BYTES_PER_PIXEL`] over the stroke's
//! footprint, [`FLOAT_SMUDGE_WRITE_BYTES_PER_PIXEL`] over
//! `PrecisionImage::rect_write_pixels` of that footprint, and the stage and
//! the layer-space dabs beside them.
//!
//! Measured in Release, one case per process, `MallocLargeCache=0`, a
//! baseline twin and three runs, twice (`examples/float_retouch_brush_admission.rs`),
//! on a 6000×4000 opaque float layer (2026-10-12, Apple M2 Max); `E` is the
//! process's `phys_footprint` peak over the interval that starts just before
//! the call (`ri_interval_max_phys_footprint`) less the footprint read there,
//! the largest of the six runs. Blur and Sharpen run under canvas-wide
//! coverage of 65535 at strength 1, the History Brush under the same coverage
//! at opacity 0.5 from a second document of the same size. Smudge runs at
//! strength 0.5 three ways, each with a baseline twin and three runs, twice:
//! SM32 is the 16-bit record's stroke, a hard size-200 tip from (1000, 1000)
//! to (1800, 1800); SM32L a hard size-1000 tip from (2000, 2000) to
//! (2150, 2000), the longest move of that tip inside the work bound; SM32T a
//! hard size-20 tip from (100, 250) to (5900, 250), a 5820×20 footprint
//! across the tile rows at y = 256 whose write-back rewrites 48 whole tiles.
//!
//! | case | plan A (`scratch_bytes`) | max E | ⌈1.25 E⌉ |
//! |---|---|---|---|
//! | BB32 (`retouch_brush` blur) | 3,368,388,608 | 1,445,037,064 | 1,806,296,330 |
//! | SB32 (`retouch_brush` sharpen) | 3,368,388,608 | 1,442,743,304 | 1,803,429,130 |
//! | SM32 (`dr_precision_doc_smudge`) | 81,809,536 | 45,105,176 | 56,381,470 |
//! | SM32L (size 1000) | 109,774,336 | 62,177,328 | 77,721,660 |
//! | SM32T (size 20, 48 tiles) | 74,492,416 | 53,149,720 | 66,437,150 |
//! | HB32 (`dr_precision_doc_history_brush`) | 2,312,388,608 | 1,154,204,512 | 1,442,755,640 |
//! | SP32 (`retouch_brush` saturate) | 3,368,388,608 | 1,442,399,264 | 1,802,999,080 |
//! | BS32 (`blursal`, Blur + Sample All Layers) | 4,741,631,488 | 2,215,020,080 | 2,768,775,100 |
//! | MS32 (`smudgesal`, SM32 + Sample All Layers) | 1,455,052,416 | 769,999,400 | 962,499,250 |
//!
//! BS32 and MS32 (2026-10-15, the same method, one set of three runs
//! and a twin reading 0) add the sampled picture's plan
//! (`retouch_brush_sampling::sampled_plate_plan`, the canvas render's) to the
//! walk's: the picture is charged once, and the walk is admitted beside it.
//!
//! SP32 is the Sponge's row (2026-10-12, the same method, one set of
//! three runs and a twin reading 0): canvas-wide coverage at strength 1, the
//! case `sponge`; it shares Blur's and Sharpen's term, which it is well
//! inside, and `FLOOR_SP32` pins it.
//!
//! `A ≥ ⌈1.25 E⌉` in each, so the constants stand; the floors are pinned by
//! `selected_color_planes_24mp.rs` (`FLOOR_BB32`, `FLOOR_SB32`, `FLOOR_SM32`,
//! `FLOOR_SM32L`, `FLOOR_SM32T`, `FLOOR_HB32`). Every twin read E ≤ 32,768.
//! Blur and Sharpen peak at 60 bytes a layer pixel (the halo read, the row
//! strip and the targets together) and the History Brush at 48 (the rect, the
//! source's part and the rewritten tiles). Smudge holds no canvas- or
//! layer-sized buffer, so its plan charges what it does allocate: the field
//! and the rect read over its footprint, and the tiles the write-back
//! rebuilds (`with_rect_rgba32` copies every touched 256×256 tile whole, and
//! builds a whole tiled plane when the target is contiguous). The thin stroke
//! is the write term's case: its 116,400 footprint pixels touch 3,072,000
//! tile pixels (48 tiles, the last column 112 wide).
use serde_json::{json, Value};

use crate::brush_coverage::{footprint, BrushRequest};
use crate::doc_lock::LOCK_TRANSPARENCY;
use crate::float_document::FLOAT_RANGE_MAX;
use crate::float_filter_apply::PLAN_RANGE;
use crate::job_control::JobContext;
use crate::layer_rect::coverage_rect16;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_retouch_brush::{coverage_at, RetouchBrushEdit};
use crate::precision_smudge::layer_dabs;
use crate::retouch_brush::{halo_in, mix, refuse_options, targets_in, Vibrancy};
use crate::retouch_brush_sampling::{clip_footprint, float_sampled_walk, sampled_field};
use crate::retouch_smudge::{smudge, smudge_stage_pixels, smudge_work};
use crate::sampling::EXTENDED_ALPHA_FLOOR;

/// Blur's and Sharpen's per-pixel plan term, over the LAYER (a bound on the
/// walk rect, as in the master): the rect read with its halo (16 B), the
/// Gaussian's binary64 row strip (32 B), the targets (12 B) and the rect
/// being rewritten (16 B). The record is the module doc's table.
const FLOAT_RETOUCH_BRUSH_BYTES_PER_PIXEL: u64 = 76;

/// The History Brush's per-pixel plan term, over the layer: the rect being
/// rewritten and the source layer's part of it, 16 B each.
pub(crate) const FLOAT_HISTORY_BRUSH_BYTES_PER_PIXEL: u64 = 32;

/// Smudge's per-pixel plan term, over the stroke's FOOTPRINT on the layer:
/// the premultiplied f32 field and the f32 rect read, 16 B each, × 1.25.
const FLOAT_SMUDGE_BYTES_PER_PIXEL: u64 = 40;

/// Smudge's write term, over `PrecisionImage::rect_write_pixels` of the
/// footprint: a rewritten f32 tile pixel, 16 B × 1.25. A tiled plane copies
/// every tile the footprint touches whole, so a thin stroke along a tile
/// boundary rewrites far more than its own area.
const FLOAT_SMUDGE_WRITE_BYTES_PER_PIXEL: u64 = 20;

/// `float_paint`'s sentence for a target that is not f32.
const STORAGE: &str = "native paint in a 32-bit float document requires f32 storage";
const ALLOCATION: &str = "could not allocate precision output";

/// Layer `index` as a paintable f32 raster and its canvas offset:
/// `precision_retouch_brush::paintable_u16` over the other storage, in the
/// same sentences.
pub(crate) fn paintable_f32(
    doc: &PrecisionDocument,
    index: usize,
) -> Result<(&PrecisionImage, (i32, i32)), String> {
    let layer = doc
        .layer(index)
        .ok_or("native paint target is out of range")?;
    let image = layer
        .source()
        .ok_or("native paint requires a raster target")?;
    if image.storage() != PrecisionStorage::Rgba32Float {
        return Err(STORAGE.into());
    }
    if layer.retained_source.is_some() {
        return Err("Rasterize the retained source before painting its pixels".into());
    }
    Ok((image, layer.offset()))
}

/// `precision_retouch_brush::coverage_plan` at f32 width: the float paint
/// walk's `8·BOOKKEEPING_BYTES + n·48 + output·16`, plus `kernel` bytes for
/// each layer pixel, under the same keys.
pub(crate) fn coverage_plan(
    doc: &PrecisionDocument,
    image: &PrecisionImage,
    kernel: u64,
    selection: bool,
) -> Result<Value, String> {
    let (width, height) = doc.dimensions();
    let n = pixel_count(width, height)? as u64;
    let (w, h) = image.dimensions();
    let output = pixel_count(w, h)? as u64;
    let scratch = 8 * BOOKKEEPING_BYTES + n * 48 + output * 16 + output * kernel;
    let planes = if selection { 2 } else { 1 };
    Ok(json!({
        "retained_bytes": doc.retained_bytes(),
        "scratch_bytes": scratch,
        "input_samples": n * planes,
        "input_width": width,
        "input_height": height,
        "input_planes": planes,
    }))
}

/// A colour component at the stored bound: the one clamp this module adds.
fn bounded(v: f32) -> f32 {
    v.clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX)
}

fn bits(p: [f32; 4]) -> [u32; 4] {
    p.map(f32::to_bits)
}

/// The float arm of `RetouchBrushEdit::plan`: the master's strength check,
/// then the target and the plan.
pub(crate) fn retouch_brush_plan(
    edit: &RetouchBrushEdit,
    doc: &PrecisionDocument,
) -> Result<Value, String> {
    if !edit.strength.is_finite() || !(0. ..=1.).contains(&edit.strength) {
        return Err("retouch brush strength is invalid".into());
    }
    let (image, _) = paintable_f32(doc, edit.layer)?;
    coverage_plan(
        doc,
        image,
        FLOAT_RETOUCH_BRUSH_BYTES_PER_PIXEL,
        edit.selection,
    )
}

/// The float arm of `RetouchBrushEdit::applying_unlocked` (module doc): Blur
/// or Sharpen over the layer rect the stroke coverage reaches, reading
/// `sampled` under Sample All Layers (`retouch_brush_sampling`).
pub(crate) fn retouch_brush_unlocked(
    edit: &RetouchBrushEdit,
    doc: &PrecisionDocument,
    input: &[u16],
    limit: u64,
    other: u64,
    job: JobContext<'_>,
    sampled: Option<&PrecisionImage>,
) -> Result<Option<PrecisionDocument>, String> {
    refuse_options(edit.kind, edit.vibrance, edit.sample_all_layers)?;
    let plan = retouch_brush_plan(edit, doc)?;
    let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
    admit(doc.retained_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    if input.len() as u64 != plan["input_samples"].as_u64().unwrap_or(u64::MAX) {
        return Err("native paint input length differs from plan".into());
    }
    if edit.strength == 0. {
        job.report(1., 5)?;
        return Ok(None);
    }
    let (image, (ox, oy)) = paintable_f32(doc, edit.layer)?;
    let (lw, lh) = image.dimensions();
    let canvas = doc.dimensions();
    let n = pixel_count(canvas.0, canvas.1)?;
    let Some((rx, ry, rw, rh)) = coverage_rect16(&input[..n], canvas, (lw, lh), (ox, oy)) else {
        job.report(1., 5)?;
        return Ok(None);
    };
    let amount_at = |at: usize| -> f64 {
        let cx = (i64::from(rx) + (at % rw as usize) as i64 + i64::from(ox)) as usize;
        let cy = (i64::from(ry) + (at / rw as usize) as i64 + i64::from(oy)) as usize;
        coverage_at(input, n, edit.selection, cy * canvas.0 as usize + cx)
            * f64::from(edit.strength)
    };
    if let Some(plate) = sampled {
        let mut px = image.read_rect_rgba32(rx, ry, rw, rh).ok_or(ALLOCATION)?;
        let frozen = doc.lock_owner(edit.layer, LOCK_TRANSPARENCY).is_some();
        let walk = (rx, ry, rw, rh);
        if !float_sampled_walk(
            edit.kind,
            &mut px,
            walk,
            (ox, oy),
            canvas,
            plate,
            amount_at,
            frozen,
            job,
        )? {
            job.report(1., 5)?;
            return Ok(None);
        }
        return write_rect(doc, edit.layer, image, (ox, oy), walk, &px, job);
    }
    // The Sponge's Vibrance at float: the display encoding's Scaled form; a
    // Grayscale document's Sponge is the middle-gray rule, Vibrance ignored
    // (`retouch_brush`'s module doc).
    let pivot = crate::gray_filter::sponge_pivot(doc);
    let encoding = if edit.vibrance && pivot.is_none() {
        Some(doc.display_encoding()?)
    } else {
        None
    };
    let vibrancy = match pivot {
        Some(m) => Vibrancy::Gray(m),
        None => encoding.as_ref().map_or(Vibrancy::Off, Vibrancy::Scaled),
    };
    // The rect plus the kind's float halo, clipped to the layer: what the
    // targets read.
    let reach = halo_in(edit.kind, &PLAN_RANGE);
    let (hx, hy) = (rx.saturating_sub(reach), ry.saturating_sub(reach));
    let hw = (rx + rw).saturating_add(reach).min(lw) - hx;
    let hh = (ry + rh).saturating_add(reach).min(lh) - hy;
    let source = image.read_rect_rgba32(hx, hy, hw, hh).ok_or(ALLOCATION)?;
    let read = |x: i64, y: i64| -> [f32; 4] {
        let lx = (i64::from(rx) + x).clamp(0, i64::from(lw) - 1) - i64::from(hx);
        let ly = (i64::from(ry) + y).clamp(0, i64::from(lh) - 1) - i64::from(hy);
        source[ly as usize * hw as usize + lx as usize]
    };
    // The kernel reads only which domain the range is (the taps, no 0…1
    // clamp), never its colours, so the plan's range serves the walk.
    let target = targets_in(
        edit.kind,
        vibrancy,
        (rw, rh),
        &PLAN_RANGE,
        read,
        job.range(0., 0.6),
    )?;
    job.report(0.6, 3)?;
    let mut px = image.read_rect_rgba32(rx, ry, rw, rh).ok_or(ALLOCATION)?;
    let mut changed = false;
    for (at, slot) in px.iter_mut().enumerate() {
        job.every(at)?;
        let old = *slot;
        if old[3] == 0. {
            continue;
        }
        let amount = amount_at(at);
        if amount == 0. {
            continue;
        }
        let out = mix(old, target[at], amount as f32);
        let new = [bounded(out[0]), bounded(out[1]), bounded(out[2]), old[3]];
        if bits(new) != bits(old) {
            *slot = new;
            changed = true;
        }
    }
    if !changed {
        job.report(1., 5)?;
        return Ok(None);
    }
    write_rect(doc, edit.layer, image, (ox, oy), (rx, ry, rw, rh), &px, job)
}

/// `px` written back over `rect` of layer `index`'s f32 `image`.
fn write_rect(
    doc: &PrecisionDocument,
    index: usize,
    image: &PrecisionImage,
    offset: (i32, i32),
    (rx, ry, rw, rh): (u32, u32, u32, u32),
    px: &[[f32; 4]],
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    let Some(image) = image.with_rect_rgba32(rx, ry, rw, rh, px)? else {
        job.report(1., 5)?;
        return Ok(None);
    };
    job.check()?;
    let next = doc.replacing_content(index, Content::Raster { image, offset })?;
    job.report(1., 5)?;
    Ok(next)
}

fn premultiplied(p: [f32; 4]) -> [f32; 4] {
    [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
}

impl PrecisionDocument {
    /// The float arm of `smudge_plan`, under its keys: the same refusals in
    /// the same order over an f32 target, and the footprint charged at f32
    /// width.
    pub(crate) fn float_smudge_plan(
        &self,
        layer: usize,
        request: &BrushRequest,
        dabs: &[f64],
        selection: bool,
    ) -> Result<Value, String> {
        let (image, offset) = paintable_f32(self, layer)?;
        let (width, height) = self.dimensions();
        let n = pixel_count(width, height)? as u64;
        let dimensions = image.dimensions();
        pixel_count(dimensions.0, dimensions.1)?;
        let local = layer_dabs(dabs, offset)?;
        smudge_work(request, dimensions, &local)?;
        let rect = footprint(dimensions, &local);
        let field = rect.map_or(0, |f| (f[2] - f[0]) as u64 * (f[3] - f[1]) as u64);
        // What the write-back can allocate: every tile the footprint touches,
        // whole, or the whole plane when the target is contiguous.
        let written = rect.map_or(0, |f| {
            image.rect_write_pixels(
                f[0] as u32,
                f[1] as u32,
                (f[2] - f[0]) as u32,
                (f[3] - f[1]) as u32,
            )
        });
        let scratch = 8 * BOOKKEEPING_BYTES
            + written * FLOAT_SMUDGE_WRITE_BYTES_PER_PIXEL
            + field * FLOAT_SMUDGE_BYTES_PER_PIXEL
            + smudge_stage_pixels(field, &local) * 16
            + dabs.len() as u64 * 8;
        Ok(json!({
            "retained_bytes": self.retained_bytes(),
            "scratch_bytes": scratch,
            "input_samples": if selection { n } else { 0 },
        }))
    }

    /// The float arm of `smudge_unlocked` (module doc): the stroke's
    /// footprint as a premultiplied f32 field through the shared walk, written
    /// back unquantized; under Sample All Layers the footprint clipped to the
    /// canvas and the field started from `sampled` (`precision_smudge`'s rule).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn float_smudge_unlocked(
        &self,
        index: usize,
        request: &BrushRequest,
        dabs: &[f64],
        strength: f64,
        selection: &[u16],
        sampled: Option<&PrecisionImage>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if !strength.is_finite() || !(0. ..=1.).contains(&strength) {
            return Err("smudge strength is invalid".into());
        }
        let plan = self.float_smudge_plan(index, request, dabs, !selection.is_empty())?;
        if selection.len() as u64 != plan["input_samples"].as_u64().unwrap_or(u64::MAX) {
            return Err("smudge selection length differs from the canvas".into());
        }
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let (image, (ox, oy)) = paintable_f32(self, index)?;
        let dimensions = image.dimensions();
        let local = layer_dabs(dabs, (ox, oy))?;
        let canvas = self.dimensions();
        let rect = footprint(dimensions, &local).and_then(|f| match sampled {
            Some(_) => clip_footprint(f, (ox, oy), canvas),
            None => Some(f),
        });
        let Some(rect) = rect else {
            job.report(1., 5)?;
            return Ok(None);
        };
        if strength == 0. {
            job.report(1., 5)?;
            return Ok(None);
        }
        let (rx, ry) = (rect[0] as u32, rect[1] as u32);
        let (rw, rh) = ((rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32);
        let mut px = image.read_rect_rgba32(rx, ry, rw, rh).ok_or(ALLOCATION)?;
        let start = sampled
            .map(|plate| sampled_field(plate, rect, (ox, oy)))
            .transpose()?;
        let mut field: Vec<[f32; 4]> = reserved_vec(px.len())?;
        match &start {
            Some(start) => field.extend_from_slice(start),
            None => field.extend(px.iter().map(|p| premultiplied(*p))),
        }
        let weight = |x: usize, y: usize| -> f64 {
            if selection.is_empty() {
                return 1.0;
            }
            let (cx, cy) = (x as i64 + i64::from(ox), y as i64 + i64::from(oy));
            if cx < 0 || cy < 0 || cx >= i64::from(canvas.0) || cy >= i64::from(canvas.1) {
                return 0.0;
            }
            f64::from(selection[cy as usize * canvas.0 as usize + cx as usize]) / 65535.0
        };
        smudge(&mut field, rect, request, &local, strength, weight, job)?;
        let frozen = self.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        let mut changed = false;
        for (at, (old, sample)) in px.iter_mut().zip(&field).enumerate() {
            job.every(at)?;
            let first = start
                .as_ref()
                .map_or_else(|| premultiplied(*old), |s| s[at]);
            // A sample the walk left alone keeps its stored bits exactly.
            if first == *sample || (frozen && old[3] == 0.) {
                continue;
            }
            // `precision_smudge`'s write-back rule (the master copy) with no
            // quantizer: the alpha floor stands where its code 0 does.
            // Each test is written so a non-finite sample falls through to the
            // colour arm and refuses in the writer.
            let alpha = if frozen {
                old[3]
            } else {
                sample[3].clamp(0., 1.)
            };
            let new = if frozen && sample[3] < EXTENDED_ALPHA_FLOOR {
                // Frozen alpha over a sample smeared to nothing: the colour
                // has no source, so it stays.
                *old
            } else if !frozen && alpha < EXTENDED_ALPHA_FLOOR {
                // Nothing visible is left: transparent black, or the pixel as
                // it was when it already held nothing.
                if old[3] == 0. {
                    *old
                } else {
                    [0.; 4]
                }
            } else {
                [
                    bounded(sample[0] / sample[3]),
                    bounded(sample[1] / sample[3]),
                    bounded(sample[2] / sample[3]),
                    alpha,
                ]
            };
            if bits(new) != bits(*old) {
                *old = new;
                changed = true;
            }
        }
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let Some(image) = image.with_rect_rgba32(rx, ry, rw, rh, &px)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        job.check()?;
        let next = self.replacing_content(
            index,
            Content::Raster {
                image,
                offset: (ox, oy),
            },
        )?;
        job.report(1., 5)?;
        Ok(next)
    }

    /// The History Brush's source layer for a float target: an f32 raster of
    /// a float document with this one's canvas size.
    fn float_history_source<'a>(
        &self,
        source: &'a PrecisionDocument,
        source_layer: usize,
    ) -> Result<(&'a PrecisionImage, (i32, i32)), String> {
        if source.dimensions() != self.dimensions() {
            return Err("history brush source has a different canvas size".into());
        }
        let layer = source
            .layer(source_layer)
            .ok_or("history brush source layer is out of range")?;
        let image = layer
            .source()
            .ok_or("history brush source is not a raster layer")?;
        if !source.is_float() || image.storage() != PrecisionStorage::Rgba32Float {
            return Err("history brush source has a different bit depth".into());
        }
        Ok((image, layer.offset()))
    }

    /// The float arm of `history_brush_plan`, under its keys. Refuses, before
    /// allocating, a target that is not an f32 raster, a source of another
    /// canvas size and a source that is not a float document's f32 raster.
    pub(crate) fn float_history_brush_plan(
        &self,
        layer: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        selection: bool,
    ) -> Result<Value, String> {
        let (image, _) = paintable_f32(self, layer)?;
        self.float_history_source(source, source_layer)?;
        coverage_plan(self, image, FLOAT_HISTORY_BRUSH_BYTES_PER_PIXEL, selection)
    }

    /// The float arm of `history_brush_unlocked` (module doc).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn float_history_brush_unlocked(
        &self,
        index: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        opacity: f64,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if !opacity.is_finite() || !(0. ..=1.).contains(&opacity) {
            return Err("history brush opacity is invalid".into());
        }
        let canvas = self.dimensions();
        let n = pixel_count(canvas.0, canvas.1)?;
        let selected = match input.len() {
            len if len == n => false,
            len if len == 2 * n => true,
            _ => return Err("native paint input length differs from plan".into()),
        };
        let plan = self.float_history_brush_plan(index, source, source_layer, selected)?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        if opacity == 0. {
            job.report(1., 5)?;
            return Ok(None);
        }
        let (image, (ox, oy)) = paintable_f32(self, index)?;
        let (from, (sox, soy)) = self.float_history_source(source, source_layer)?;
        let (lw, lh) = image.dimensions();
        let Some((rx, ry, rw, rh)) = coverage_rect16(&input[..n], canvas, (lw, lh), (ox, oy))
        else {
            job.report(1., 5)?;
            return Ok(None);
        };
        // The rect's canvas position in the source layer's own pixels, and the
        // part of it that layer holds, read once.
        let (sx, sy) = (
            i64::from(rx) + i64::from(ox) - i64::from(sox),
            i64::from(ry) + i64::from(oy) - i64::from(soy),
        );
        let (sw, sh) = from.dimensions();
        let (ix0, iy0) = (sx.max(0), sy.max(0));
        let (ix1, iy1) = (
            (sx + i64::from(rw)).min(i64::from(sw)),
            (sy + i64::from(rh)).min(i64::from(sh)),
        );
        let held = if ix0 < ix1 && iy0 < iy1 {
            from.read_rect_rgba32(
                ix0 as u32,
                iy0 as u32,
                (ix1 - ix0) as u32,
                (iy1 - iy0) as u32,
            )
            .ok_or(ALLOCATION)?
        } else {
            Vec::new()
        };
        let frozen = self.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        let mut px = image.read_rect_rgba32(rx, ry, rw, rh).ok_or(ALLOCATION)?;
        let mut changed = false;
        let area = px.len();
        for (at, slot) in px.iter_mut().enumerate() {
            job.every(at)?;
            if at & 65535 == 0 {
                job.report(0.8 * at as f64 / area as f64, 3)?;
            }
            let (column, row) = ((at % rw as usize) as i64, (at / rw as usize) as i64);
            let cx = (i64::from(rx) + column + i64::from(ox)) as usize;
            let cy = (i64::from(ry) + row + i64::from(oy)) as usize;
            let coverage = coverage_at(input, n, selected, cy * canvas.0 as usize + cx);
            let old = *slot;
            let t = coverage * opacity;
            if t == 0. || (frozen && old[3] == 0.) {
                continue;
            }
            let (source_x, source_y) = (sx + column, sy + row);
            let wanted = if (ix0..ix1).contains(&source_x) && (iy0..iy1).contains(&source_y) {
                held[(source_y - iy0) as usize * (ix1 - ix0) as usize + (source_x - ix0) as usize]
            } else {
                [0.; 4]
            };
            if bits(wanted) == bits(old) {
                continue;
            }
            let new = if t >= 1. && !frozen {
                // The state's own sample, bit for bit.
                wanted
            } else if t >= 1. {
                // Frozen alpha: the state's colour where it has one.
                if wanted[3] > 0. {
                    [wanted[0], wanted[1], wanted[2], old[3]]
                } else {
                    old
                }
            } else {
                let (a, b) = (old.map(f64::from), wanted.map(f64::from));
                // Premultiplied mix: a transparent side contributes no colour.
                let alpha = (1. - t) * a[3] + t * b[3];
                let mut new = old;
                if alpha > 0. {
                    for c in 0..3 {
                        new[c] =
                            bounded((((1. - t) * a[c] * a[3] + t * b[c] * b[3]) / alpha) as f32);
                    }
                }
                if !frozen {
                    new[3] = (alpha as f32).min(1.);
                    if new[3] < EXTENDED_ALPHA_FLOOR {
                        // Nothing visible is left: transparent black, or the
                        // pixel as it was when it already held nothing.
                        new = if old[3] == 0. { old } else { [0.; 4] };
                    }
                }
                new
            };
            if bits(new) != bits(old) {
                *slot = new;
                changed = true;
            }
        }
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let Some(image) = image.with_rect_rgba32(rx, ry, rw, rh, &px)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        job.check()?;
        let next = self.replacing_content(
            index,
            Content::Raster {
                image,
                offset: (ox, oy),
            },
        )?;
        job.report(1., 5)?;
        Ok(next)
    }
}
