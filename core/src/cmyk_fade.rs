//! Edit ▸ Fade on a CMYK raster (#1): `precision_fade`'s walk over
//! CMYKA16 — the same overlap (`precision_fade::Overlap`), the same weights
//! (`precision_fade::fade_weight`: a region step's coverage, a stroke's
//! changed samples), the same Pass Through refusal and no-op rule — with
//! each pixel through `cmyk_composite::fade_ink`, `fade::fade_pixel`'s rule
//! on the four inks (any mode of `CMYK_BLEND_MODES`, composited by
//! `over_mode`). `self` is the faded step's result, `before` the state
//! before it, same layer index in both; a transparency lock keeps the
//! result's alpha.
//!
//! The plan is layer px × 20 (the target's rect read and `before`'s part of
//! it, 10 bytes a pixel each) + canvas × 2 (the coverage) + canvas ×
//! `DISPLAY_BYTES_PER_PIXEL` (the host's display preparation) + bookkeeping,
//! with `precision_fade`'s other keys.

use serde_json::{json, Value};

use crate::blend::BlendMode;
use crate::cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL;
use crate::cmyk_composite::{codes, fade_ink, unit, InkPixel};
use crate::cmyk_image::{CmykImage, CmykSample};
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_fade::{fade_weight, Overlap};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};

fn ink_pixel(sample: CmykSample) -> InkPixel {
    sample.map(unit)
}

impl PrecisionDocument {
    /// The target CMYK raster and its offset.
    fn cmyk_fade_target(&self, layer: usize) -> Result<(&CmykImage, (i32, i32)), String> {
        let entry = self
            .layer(layer)
            .ok_or("fade target layer is out of range")?;
        let image = entry
            .cmyk_source()
            .ok_or("fade requires a CMYK raster layer")?;
        Ok((image, entry.offset()))
    }

    /// `before`'s CMYK raster at `layer`, of this document's canvas size.
    fn cmyk_fade_source<'a>(
        &self,
        before: &'a PrecisionDocument,
        layer: usize,
    ) -> Result<(&'a CmykImage, (i32, i32)), String> {
        if before.dimensions() != self.dimensions() {
            return Err("fade source has a different canvas size".into());
        }
        if !before.is_cmyk() {
            return Err("fade source is not a CMYK document".into());
        }
        let entry = before
            .layer(layer)
            .ok_or("fade source layer is out of range")?;
        let image = entry
            .cmyk_source()
            .ok_or("fade source is not a CMYK raster layer")?;
        Ok((image, entry.offset()))
    }

    /// Fade's plan on a CMYK document (module doc).
    pub(crate) fn cmyk_fade_plan(
        &self,
        layer: usize,
        before: &PrecisionDocument,
        coverage: bool,
    ) -> Result<Value, String> {
        let (image, _) = self.cmyk_fade_target(layer)?;
        self.cmyk_fade_source(before, layer)?;
        let (w, h) = image.dimensions();
        let pixels = pixel_count(w, h)? as u64;
        let (cw, ch) = self.dimensions();
        let canvas = pixel_count(cw, ch)? as u64;
        let scratch =
            pixels * 20 + canvas * 2 + canvas * DISPLAY_BYTES_PER_PIXEL + 4 * BOOKKEEPING_BYTES;
        let planes = u64::from(coverage);
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "input_samples":canvas * planes,"input_width":cw,"input_height":ch,
            "input_planes":planes}),
        )
    }

    /// The descriptor's `cmyk_fade_scratch_bytes`: a region Fade of the top
    /// CMYK raster against this document itself.
    pub(crate) fn cmyk_fade_scratch_bytes(&self) -> Option<u64> {
        let top = (0..self.layer_count())
            .rev()
            .find(|&i| self.layer(i).and_then(|l| l.cmyk_source()).is_some())?;
        self.cmyk_fade_plan(top, self, true)
            .ok()
            .and_then(|plan| plan["scratch_bytes"].as_u64())
    }

    /// Edit ▸ Fade of CMYK raster `layer` toward `before` (module doc);
    /// `precision_fade::fade`'s contract. A PIXELS edit under the locks.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_fade(
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
        self.under_locks(layer, EditKind::Pixels, |doc| {
            doc.cmyk_fade_unlocked(
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
    fn cmyk_fade_unlocked(
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
        if let Some(refusal) = crate::cmyk_document::blend_refusal(mode) {
            return Err(refusal);
        }
        if !opacity.is_finite() || !(0. ..=1.).contains(&opacity) {
            return Err("fade opacity is invalid".into());
        }
        let canvas = self.dimensions();
        let n = pixel_count(canvas.0, canvas.1)?;
        if !coverage.is_empty() && coverage.len() != n {
            return Err("fade coverage length differs from the canvas".into());
        }
        let plan = self.cmyk_fade_plan(index, before, !coverage.is_empty())?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let (image, offset) = self.cmyk_fade_target(index)?;
        let (from, from_offset) = self.cmyk_fade_source(before, index)?;
        let (lw, lh) = image.dimensions();
        if lw == 0 || lh == 0 {
            job.report(1., 5)?;
            return Ok(None);
        }
        let overlap = Overlap::new((offset, (lw, lh)), (from_offset, from.dimensions()));
        let frozen = self.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        let opacity = opacity as f32;
        let held = if overlap.is_empty() {
            Vec::new()
        } else {
            from.read_rect(overlap.rect(), job.checkpoints_only())?
        };
        let mut px = image.read_rect((0, 0, lw, lh), job.checkpoints_only())?;
        let mut changed = false;
        let area = px.len();
        for (at, slot) in px.iter_mut().enumerate() {
            job.every(at)?;
            if at & 65535 == 0 {
                job.report(0.8 * at as f64 / area as f64, 3)?;
            }
            let (column, row) = ((at % lw as usize) as i64, (at / lw as usize) as i64);
            let old = *slot;
            let prior = overlap.index(column, row).map_or([0; 5], |i| held[i]);
            if prior[4] == 0 && old[4] == 0 {
                continue;
            }
            let weight = fade_weight(
                coverage,
                canvas,
                offset,
                changed_only,
                (column, row),
                prior != old,
            );
            let out = fade_ink(
                ink_pixel(prior),
                ink_pixel(old),
                mode,
                weight,
                opacity,
                frozen,
                (i64::from(offset.0) + column, i64::from(offset.1) + row),
            );
            let mut new = codes(out);
            if frozen {
                new[4] = old[4];
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
        let Some(out) = image.with_rect((0, 0, lw, lh), &px, job.checkpoints_only())? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        job.check()?;
        let next = self.replacing_content(index, Content::CmykRaster { image: out, offset })?;
        job.report(1., 5)?;
        Ok(next)
    }
}
