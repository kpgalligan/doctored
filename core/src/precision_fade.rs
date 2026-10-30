//! Edit ▸ Fade on the typed document: the 16-bit and 32-bit float arms of
//! `doc_fade` (the byte adapter, whose walk this mirrors), over the `fade`
//! kernel. `self` is the faded step's result, `before` the state before it,
//! same layer index in both.
//!
//! The walk covers the whole of the result's layer rect, never grows it, and
//! reads `before` at the same canvas position (transparent outside its
//! layer). The weight is the canvas-sized u16 `coverage` of a region step (1
//! when empty), or for a stroke (`changed_only`) 1 where the stored sample
//! differs from `before`'s, 0 elsewhere. A u16 result rounds as the History
//! Brush does (`(v.clamp(0, 1) · 65535).round()`); an f32 result keeps its
//! range (colour held only to the stored bound ±`FLOAT_RANGE_MAX`, alpha to
//! 0…1). A float document takes only the modes `float_document::
//! admitted_blend` admits and refuses the rest by name.
//!
//! The plan is the History Brush's (`coverage_plan` with
//! `HISTORY_BRUSH_BYTES_PER_PIXEL` / `FLOAT_HISTORY_BRUSH_BYTES_PER_PIXEL`):
//! the same walk shape — the layer rect being rewritten and `before`'s part
//! of it — so its measured term covers this one and no new plan is minted.
//!
//! A CMYK document's Fade (#1) is `cmyk_fade`'s: `fade_plan` and `fade`
//! route there, and its walk shares this module's `Overlap` and
//! `fade_weight`.
use serde_json::Value;

use crate::blend::BlendMode;
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::fade::{fade_blend, fade_pixel};
use crate::float_document::{admitted_blend, blend_refusal, FLOAT_RANGE_MAX};
use crate::float_retouch_brush::{
    coverage_plan as float_coverage_plan, paintable_f32, FLOAT_HISTORY_BRUSH_BYTES_PER_PIXEL,
};
use crate::job_control::JobContext;
use crate::lab_blend::lab_fade_blend;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, PrecisionImage, PrecisionStorage};
use crate::precision_retouch_brush::{coverage_plan, paintable_u16, HISTORY_BRUSH_BYTES_PER_PIXEL};

const ALLOCATION: &str = "could not allocate precision output";

/// The rect of `before`'s layer that the result's layer rect overlaps, in
/// `before`'s own pixels: the result's origin there and the clipped bounds.
/// `cmyk_fade` walks the same rect.
pub(crate) struct Overlap {
    origin: (i64, i64),
    x: (i64, i64),
    y: (i64, i64),
}

impl Overlap {
    pub(crate) fn new(target: ((i32, i32), (u32, u32)), from: ((i32, i32), (u32, u32))) -> Self {
        let ((ox, oy), (lw, lh)) = target;
        let ((fx, fy), (sw, sh)) = from;
        let (sx, sy) = (i64::from(ox) - i64::from(fx), i64::from(oy) - i64::from(fy));
        Overlap {
            origin: (sx, sy),
            x: (sx.max(0), (sx + i64::from(lw)).min(i64::from(sw))),
            y: (sy.max(0), (sy + i64::from(lh)).min(i64::from(sh))),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.x.0 >= self.x.1 || self.y.0 >= self.y.1
    }

    pub(crate) fn rect(&self) -> (u32, u32, u32, u32) {
        (
            self.x.0 as u32,
            self.y.0 as u32,
            (self.x.1 - self.x.0) as u32,
            (self.y.1 - self.y.0) as u32,
        )
    }

    /// Index into the held rect of the result's layer pixel (column, row),
    /// or `None` where `before`'s layer holds nothing.
    pub(crate) fn index(&self, column: i64, row: i64) -> Option<usize> {
        let (x, y) = (self.origin.0 + column, self.origin.1 + row);
        ((self.x.0..self.x.1).contains(&x) && (self.y.0..self.y.1).contains(&y)).then(|| {
            (y - self.y.0) as usize * (self.x.1 - self.x.0) as usize + (x - self.x.0) as usize
        })
    }
}

impl PrecisionDocument {
    /// `before`'s layer `layer`: a raster of this document's own storage and
    /// canvas size.
    fn fade_source<'a>(
        &self,
        before: &'a PrecisionDocument,
        layer: usize,
    ) -> Result<(&'a PrecisionImage, (i32, i32)), String> {
        if before.dimensions() != self.dimensions() {
            return Err("fade source has a different canvas size".into());
        }
        let entry = before
            .layer(layer)
            .ok_or("fade source layer is out of range")?;
        let image = entry.source().ok_or("fade source is not a raster layer")?;
        let storage = if self.is_float() {
            PrecisionStorage::Rgba32Float
        } else {
            PrecisionStorage::Rgba16Unorm
        };
        if before.is_float() != self.is_float() || image.storage() != storage {
            return Err("fade source has a different bit depth".into());
        }
        Ok((image, entry.offset()))
    }

    /// The target layer: `paintable_u16` / `paintable_f32` by depth.
    fn fade_target(&self, layer: usize) -> Result<(&PrecisionImage, (i32, i32)), String> {
        if self.is_float() {
            paintable_f32(self, layer)
        } else {
            paintable_u16(self, layer)
        }
    }

    /// Fade's plan for layer `layer` against `before`: `retained_bytes`,
    /// `scratch_bytes`, and `input_samples` (the canvas pixel count when
    /// `coverage`, else 0). Refuses, before allocating, a target that is not
    /// a raster of the document's own storage, a retained source, and a
    /// `before` of another canvas size or depth.
    pub(crate) fn fade_plan(
        &self,
        layer: usize,
        before: &PrecisionDocument,
        coverage: bool,
    ) -> Result<Value, String> {
        if self.is_cmyk() {
            return self.cmyk_fade_plan(layer, before, coverage);
        }
        let (image, _) = self.fade_target(layer)?;
        self.fade_source(before, layer)?;
        let mut plan = if self.is_float() {
            float_coverage_plan(self, image, FLOAT_HISTORY_BRUSH_BYTES_PER_PIXEL, false)?
        } else {
            coverage_plan(self, image, HISTORY_BRUSH_BYTES_PER_PIXEL, false)?
        };
        if !coverage {
            plan["input_samples"] = 0.into();
            plan["input_planes"] = 0.into();
        }
        Ok(plan)
    }

    /// Fades layer `layer` of this document (the faded step's result) back
    /// toward the same layer of `before` at `opacity` (0…1) in `mode` —
    /// Edit ▸ Fade. `coverage` is the canvas-sized u16 weight of
    /// a region step, or empty for 1 everywhere; `changed_only` weighs a
    /// stroke by the samples it changed instead. `Ok(None)` when no stored
    /// sample changes. Refuses by name: Pass Through; at float a mode
    /// `admitted_blend` refuses; the plan's refusals; an invalid opacity; a
    /// coverage of another length. A PIXELS edit under `precision_lock`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fade(
        &self,
        layer: usize,
        before: &PrecisionDocument,
        mode: BlendMode,
        opacity: f64,
        coverage: &[u16],
        changed_only: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if self.is_cmyk() {
            return self.cmyk_fade(
                layer,
                before,
                mode,
                opacity,
                coverage,
                changed_only,
                limit,
                other,
                job,
            );
        }
        self.under_locks(layer, EditKind::Pixels, |doc| {
            doc.fade_unlocked(
                layer,
                before,
                mode,
                opacity,
                coverage,
                changed_only,
                limit,
                other,
                job,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn fade_unlocked(
        &self,
        index: usize,
        before: &PrecisionDocument,
        mode: BlendMode,
        opacity: f64,
        coverage: &[u16],
        changed_only: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if mode.is_group_only() {
            return Err("fade cannot use Pass Through, which applies to groups only".into());
        }
        let float = self.is_float();
        if float && !admitted_blend(mode) {
            return Err(blend_refusal(mode));
        }
        let blend = match self.lab_grid() {
            Some(grid) => lab_fade_blend(mode, grid),
            None => fade_blend(mode, float),
        }
        .ok_or("fade blend mode is invalid")?;
        if !opacity.is_finite() || !(0. ..=1.).contains(&opacity) {
            return Err("fade opacity is invalid".into());
        }
        let canvas = self.dimensions();
        let n = pixel_count(canvas.0, canvas.1)?;
        if !coverage.is_empty() && coverage.len() != n {
            return Err("fade coverage length differs from the canvas".into());
        }
        let plan = self.fade_plan(index, before, !coverage.is_empty())?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let (image, offset) = self.fade_target(index)?;
        let (from, from_offset) = self.fade_source(before, index)?;
        let (lw, lh) = image.dimensions();
        if lw == 0 || lh == 0 {
            job.report(1., 5)?;
            return Ok(None);
        }
        let overlap = Overlap::new((offset, (lw, lh)), (from_offset, from.dimensions()));
        let frozen = self.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        let weigh = |column: i64, row: i64, changed: bool| -> f32 {
            fade_weight(
                coverage,
                canvas,
                offset,
                changed_only,
                (column, row),
                changed,
            )
        };
        let opacity = opacity as f32;
        let image = if float {
            let held = if overlap.is_empty() {
                Vec::new()
            } else {
                let (x, y, w, h) = overlap.rect();
                from.read_rect_rgba32(x, y, w, h).ok_or(ALLOCATION)?
            };
            let mut px = image.read_rect_rgba32(0, 0, lw, lh).ok_or(ALLOCATION)?;
            let mut changed = false;
            let area = px.len();
            for (at, slot) in px.iter_mut().enumerate() {
                job.every(at)?;
                if at & 65535 == 0 {
                    job.report(0.8 * at as f64 / area as f64, 3)?;
                }
                let (column, row) = ((at % lw as usize) as i64, (at / lw as usize) as i64);
                let old = *slot;
                let prior = overlap.index(column, row).map_or([0.; 4], |i| held[i]);
                if prior[3] <= 0. && old[3] <= 0. {
                    continue;
                }
                let differs = prior.map(f32::to_bits) != old.map(f32::to_bits);
                let out = fade_pixel(
                    prior,
                    old,
                    &blend,
                    weigh(column, row, differs),
                    opacity,
                    frozen,
                    (i64::from(offset.0) + column, i64::from(offset.1) + row),
                );
                let mut new = [
                    out[0].clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX),
                    out[1].clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX),
                    out[2].clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX),
                    out[3].clamp(0., 1.),
                ];
                if frozen {
                    new[3] = old[3];
                }
                if new.map(f32::to_bits) != old.map(f32::to_bits) {
                    *slot = new;
                    changed = true;
                }
            }
            if !changed {
                job.report(1., 5)?;
                return Ok(None);
            }
            image.with_rect_rgba32(0, 0, lw, lh, &px)?
        } else {
            let held = if overlap.is_empty() {
                Vec::new()
            } else {
                let (x, y, w, h) = overlap.rect();
                from.read_rect_rgba16(x, y, w, h).ok_or(ALLOCATION)?
            };
            let mut px = image.read_rect_rgba16(0, 0, lw, lh).ok_or(ALLOCATION)?;
            let mut changed = false;
            let area = px.len();
            let unit = |p: [u16; 4]| p.map(|v| f32::from(v) / 65535.);
            let code = |v: f32| (f64::from(v).clamp(0., 1.) * 65535.).round() as u16;
            for (at, slot) in px.iter_mut().enumerate() {
                job.every(at)?;
                if at & 65535 == 0 {
                    job.report(0.8 * at as f64 / area as f64, 3)?;
                }
                let (column, row) = ((at % lw as usize) as i64, (at / lw as usize) as i64);
                let old = *slot;
                let prior = overlap.index(column, row).map_or([0; 4], |i| held[i]);
                if prior[3] == 0 && old[3] == 0 {
                    continue;
                }
                let out = fade_pixel(
                    unit(prior),
                    unit(old),
                    &blend,
                    weigh(column, row, prior != old),
                    opacity,
                    frozen,
                    (i64::from(offset.0) + column, i64::from(offset.1) + row),
                );
                let mut new = out.map(code);
                if frozen {
                    new[3] = old[3];
                }
                if new != old {
                    *slot = new;
                    changed = true;
                }
            }
            if !changed {
                job.report(1., 5)?;
                return Ok(None);
            }
            image.with_rect_rgba16(0, 0, lw, lh, &px)?
        };
        let Some(image) = image else {
            job.report(1., 5)?;
            return Ok(None);
        };
        job.check()?;
        let next = self.replacing_content(index, Content::Raster { image, offset })?;
        job.report(1., 5)?;
        Ok(next)
    }
}

/// The weight of the result's layer pixel `(column, row)` (module doc): for
/// a stroke (`changed_only`) 1 where `changed`, else 0; for a region step
/// the canvas `coverage` there (1 when empty, 0 off the canvas). The CMYK
/// walk (`cmyk_fade`) weighs by it too.
pub(crate) fn fade_weight(
    coverage: &[u16],
    canvas: (u32, u32),
    offset: (i32, i32),
    changed_only: bool,
    (column, row): (i64, i64),
    changed: bool,
) -> f32 {
    if changed_only {
        return if changed { 1. } else { 0. };
    }
    if coverage.is_empty() {
        return 1.;
    }
    let (cx, cy) = (i64::from(offset.0) + column, i64::from(offset.1) + row);
    if (0..i64::from(canvas.0)).contains(&cx) && (0..i64::from(canvas.1)).contains(&cy) {
        f32::from(coverage[cy as usize * canvas.0 as usize + cx as usize]) / 65535.
    } else {
        0.
    }
}
