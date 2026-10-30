//! Bake effective native mask coverage into raster alpha without a byte plane.
//! Apply honors density/feather even for disabled masks, matching remove_mask's
//! explicit byte Apply action. Hidden RGB and independent storage stay exact.
//!
//! # 32-bit float
//!
//! A float layer takes the same coverage in f32: `alpha' = alpha ·
//! clamp(coverage, 0, 1)`, one f32 product, with every RGB component kept bit
//! for bit (above white and below zero included). The mask and the description
//! are dropped and the lock gate is the u16 one.
//!
//! # Admission (float)
//!
//! The u16 plan is `6·BOOKKEEPING_BYTES + n·8 + the mask's plan`. The float
//! plan is `6·BOOKKEEPING_BYTES + n·FLOAT_APPLY_BYTES_PER_PIXEL + the mask's
//! plan`: the edit holds two 16 B/px copies at its peak, the output vector and
//! the tiles `rebuilt_rgba32_extended` builds from it, and the plan must be at
//! least 1.25 × the measured peak, which two copies cannot reach, so the
//! factor is three copies (48).
//!
//! Measured with `MallocLargeCache=0 DR_FLOAT_CASE=apply_mask make t
//! F=float_layer_edits T=admission_measure A='--ignored --nocapture'`
//! (Release, one case per process, three runs, working limit 8,589,934,592):
//! a 4000×3000 float document with one opaque non-uniform RGBA32F layer under
//! a canvas-sized mask that is 32768 inside the rect (200, 150, 3600, 2700)
//! and 0 outside, so every pixel's alpha changes and no tile of the result is
//! shared with the source. `E` is `ri_interval_max_phys_footprint − ri_phys_footprint`
//! after `proc_reset_footprint_interval`, with the result held until it is
//! read.
//!
//! | case       | scratch (plan) | max E of 3  | ⌈1.25 · max E⌉ |
//! |------------|----------------|-------------|----------------|
//! | baseline   | —              | 0           | —              |
//! | apply_mask | 582,291,456    | 384,991,496 | 481,239,370    |
//!
//! `baseline` is the twin: the same fixture with no edit run. E is the two
//! copies (384,000,000) plus under 1 MB, so the factor of 48 passed its first
//! round and stays. A first round under a mask of 65535 inside the rect read
//! max E 248,053,952: the tiles the apply leaves bitwise unchanged are shared
//! with the source, so that mask is not the worst case and the table is the
//! all-tiles one.
//!
//! `core/tests/float_layer_edits.rs`'s `plans_cover_the_measured_peaks` holds
//! the same `max E` and fails when the plan drops below its floor.
//!
//! # CMYK
//!
//! A CMYK raster (#547) takes the u16 rule on its straight alpha: `alpha' =
//! round(alpha · clamp(PreparedMask::at, 0, 1))`, every ink bit kept. Its rows
//! are rebuilt once (`CmykImage::from_rows_with_job`) into
//! `Content::CmykRaster`, and the mask and description are dropped. The plan
//! is `6·BOOKKEEPING_BYTES + n·10 + canvas·cmyk_paint::CANVAS_BYTES_PER_PIXEL +
//! the mask's plan`: the rebuilt raster, the canvas term every CMYK pixel edit
//! charges for the host's display preparation of its result, and the mask's
//! preparation. `scripts/cmyk-admission`'s `apply-mask-16` (Release, 24 MP, a
//! from-selection mask over half the layer, the whole MCP call) read max E
//! 368,591,136 against the first plan of `6·BOOKKEEPING + n·10 + mask`
//! (246,291,456, a miss), so the canvas term was added.
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{reserved_vec, PrecisionStorage, BOOKKEEPING_BYTES};

/// Scratch bytes per target pixel of a float apply: three 16 B/px copies
/// (module doc's `# Admission (float)`). A measured miss raises this.
const FLOAT_APPLY_BYTES_PER_PIXEL: u64 = 48;

impl PrecisionDocument {
    pub(crate) fn apply_mask_plan(&self, index: usize) -> Result<u64, String> {
        let layer = self
            .layer(index)
            .ok_or("native mask layer is out of range")?;
        if let Some(image) = layer.cmyk_source() {
            // The CMYK arm (module doc): the ink rows rebuilt once.
            let mask = layer.mask().ok_or("native layer has no mask to apply")?;
            let inspection = mask.with_properties(mask.properties().clone(), true)?;
            let (scratch, _) = inspection.plan()?;
            let (w, h) = image.dimensions();
            let canvas = u64::from(self.width) * u64::from(self.height);
            return Ok(6 * BOOKKEEPING_BYTES
                + u64::from(w) * u64::from(h) * crate::cmyk_image::CMYK_BYTES_PER_PIXEL
                + canvas * crate::cmyk_paint::CANVAS_BYTES_PER_PIXEL
                + scratch);
        }
        let source = layer
            .source()
            .ok_or("applying a mask requires raster pixels; groups have no pixels to bake")?;
        let mask = layer.mask().ok_or("native layer has no mask to apply")?;
        let inspection = mask.with_properties(mask.properties().clone(), true)?;
        let (scratch, _) = inspection.plan()?;
        let n = u64::from(source.dimensions().0) * u64::from(source.dimensions().1);
        let per_pixel = match source.storage() {
            PrecisionStorage::Rgba16Unorm => 8,
            PrecisionStorage::Rgba32Float => FLOAT_APPLY_BYTES_PER_PIXEL,
        };
        Ok(6 * BOOKKEEPING_BYTES + n * per_pixel + scratch)
    }
    /// The LOCK gate (`precision_lock`): APPLYING a mask is the one mask
    /// operation that bakes coverage straight into the layer's ALPHA, so it
    /// is a `MaskApply` edit — blocked by Lock Transparency as well as by
    /// Lock All, and NAMED rather than silently un-applied (`doc_lock`'s
    /// module doc). `lock_refusal` rather than `under_locks`, because the
    /// alpha restore the wrapper would run is exactly what must not happen
    /// here.
    pub(crate) fn apply_mask_with_context(
        &self,
        index: usize,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.lock_refusal(index, crate::doc_lock::EditKind::MaskApply)?;
        let scratch = self.apply_mask_plan(index)?;
        let job = job.with_reservation_floor(scratch);
        let layer = self.layer(index).unwrap();
        if let Some(image) = layer.cmyk_source() {
            return self.apply_cmyk_mask(index, image, job);
        }
        let source = layer.source().unwrap();
        let mask = layer.mask().unwrap();
        let inspection = mask.with_properties(mask.properties().clone(), true)?;
        let prepared = inspection.prepare(job.range(0., 0.7))?;
        let (w, h) = source.dimensions();
        let offset = layer.offset();
        let image = if let Some(runs) = source.runs_rgba32() {
            // The float arm (module doc): one f32 product on alpha, RGB bits
            // untouched.
            let mut pixels = reserved_vec(w as usize * h as usize)?;
            let mut next = 0;
            for run in runs {
                for &p in run {
                    let i = next;
                    next += 1;
                    job.every(i)?;
                    let mut pixel = p;
                    let x = (i % w as usize) as i64 + i64::from(offset.0);
                    let y = (i / w as usize) as i64 + i64::from(offset.1);
                    pixel[3] = p[3] * prepared.at(x, y).clamp(0., 1.);
                    pixels.push(pixel);
                }
            }
            job.check()?;
            source.rebuilt_rgba32_extended(w, h, pixels, self.profile().clone())?
        } else {
            let runs = source
                .runs_rgba16()
                .ok_or("mask apply requires native u16 raster")?;
            let mut pixels = reserved_vec(w as usize * h as usize)?;
            let mut next = 0;
            for run in runs {
                for &p in run {
                    let i = next;
                    next += 1;
                    job.every(i)?;
                    let mut pixel = p;
                    let x = (i % w as usize) as i64 + i64::from(offset.0);
                    let y = (i / w as usize) as i64 + i64::from(offset.1);
                    pixel[3] = (f64::from(p[3]) * f64::from(prepared.at(x, y).clamp(0., 1.)))
                        .round() as u16;
                    pixels.push(pixel);
                }
            }
            job.check()?;
            source.rebuilt_rgba16(w, h, pixels, self.profile().clone())?
        };
        let mut next = layer.clone();
        next.content = Content::Raster { image, offset };
        next.description = None;
        next.mask = None;
        let result = self.replacing_layer(index, next)?;
        job.report(1., 5)?;
        Ok(result)
    }

    /// The CMYK arm (module doc, `# CMYK`): `alpha' = round(alpha ·
    /// clamp(coverage, 0, 1))` in f64, the ink untouched bit for bit.
    fn apply_cmyk_mask(
        &self,
        index: usize,
        image: &crate::cmyk_image::CmykImage,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let layer = self
            .layer(index)
            .ok_or("native mask layer is out of range")?;
        let mask = layer.mask().ok_or("native layer has no mask to apply")?;
        let inspection = mask.with_properties(mask.properties().clone(), true)?;
        let prepared = inspection.prepare(job.range(0., 0.7))?;
        let (w, h) = image.dimensions();
        let offset = layer.offset();
        let baked = crate::cmyk_image::CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                job.check()?;
                for (x, run) in image.row_segments(y, 0, w) {
                    for (k, (dst, p)) in row[x as usize..x as usize + run.len()]
                        .iter_mut()
                        .zip(run)
                        .enumerate()
                    {
                        let cx = i64::from(x) + k as i64 + i64::from(offset.0);
                        let cy = i64::from(y) + i64::from(offset.1);
                        let coverage = f64::from(prepared.at(cx, cy).clamp(0., 1.));
                        let mut pixel = *p;
                        pixel[4] = (f64::from(p[4]) * coverage).round() as u16;
                        *dst = pixel;
                    }
                }
                Ok(())
            },
            job,
        )?;
        let mut next = layer.clone();
        next.content = Content::CmykRaster {
            image: baked,
            offset,
        };
        next.description = None;
        next.mask = None;
        let result = self.replacing_layer(index, next)?;
        job.report(1., 5)?;
        Ok(result)
    }
}
