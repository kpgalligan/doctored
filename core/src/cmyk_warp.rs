//! Layer ▸ Warp and Filter ▸ Liquify on a CMYK document's straight CMYKA16
//! ink (#1 phase 4b).
//!
//! WARP. The float rule (`precision_warp`): a CMYK warp is applied at
//! once and keeps no binding (`build_cmyk` refuses one, so no format change),
//! so `warp_info` answers `null`, Clear and Bake change nothing (`None`), an
//! identity set changes nothing, and each set warps the current pixels
//! again. The pixels run `sampling::warp_into` (the persisted warp's walk)
//! with the five-plane binary64 adapter (`precision_warp::accumulate_planes16`
//! / `finish_planes16`, whose C = 4 is the RGBA16 arm); a mask target and a
//! linked mask run `precision_warp::mask`, the RGB walk verbatim. The plan is
//! `warp_plan`'s with 10 B/px output plus the CMYK source and adoption copies.
//! Refusals and locks in the RGB order.
//!
//! LIQUIFY. The RGB kernel (`liquify`): the field is built and stroked ONCE,
//! then `LiquifyField::render` runs over two four-plane views of the ink,
//! `[C, M, Y, A]` and `[K, 0, 0, A]`, each mixed with the selection coverage
//! by `selected_adjustment::mix_codes` (the RGB 16-bit filter's mix), and the
//! two are joined `[c, m, y, k, a]`. Every step is per plane with alpha read
//! from A only, so the join is exactly a five-plane render; a disagreeing
//! alpha is an internal error. A transparency lock restores alpha as
//! `PrecisionDocument::restoring_alpha` does for RGB. Plan: `liquify::plan`
//! (one f32 render and the field), the second render (16 B/px), the ink's
//! contiguous copy and the output tiles (10 B/px each), the canvas coverage,
//! `selected_edit_plan`'s bookkeeping, and the host's display preparation of
//! the result, `cmyk_paint::CANVAS_BYTES_PER_PIXEL` over the canvas: without
//! that term `scripts/cmyk-admission`'s `liquify-16` (24 MP, one size-500
//! Forward Warp stroke, Release, 3 runs) measured max E 1,282,343,872
//! against 1,357,748,608 (1.059×), a miss.

use serde_json::{json, Value};

use crate::cmyk_image::{CmykImage, CmykSample, CMYK_BYTES_PER_PIXEL};
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::liquify::{LiquifyField, LiquifyStroke};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::precision_sampling::{cmyk_adoption_copy_bytes, cmyk_source_copy_bytes};
use crate::precision_structure_ops::{flatten, rebuild};
use crate::precision_warp::{
    accumulate_planes16, finish_planes16, mask as warped_mask, rect, shifted,
};
use crate::retained_source::SourcePlan;
use crate::sampling::warp_into;
use crate::selected_adjustment::{coverage_at, mix_codes, validate_coverage};
use crate::warp::{WarpEdit, WarpField, WarpTarget};

/// The no-op plan: Clear, Bake and an identity set.
const NOTHING: SourcePlan = SourcePlan {
    retained_bytes: 0,
    scratch_bytes: 0,
    work_units: 0,
};

/// One four-plane view of the ink as unit samples: `[C, M, Y, A]` or
/// `[K, 0, 0, A]` (the `PrecisionImage::pixel` normalization).
fn unit_view(sample: &CmykSample, black: bool) -> [f32; 4] {
    codes_view(sample, black).map(|c| f32::from(c) / 65535.0)
}

/// [`unit_view`] as codes.
fn codes_view(sample: &CmykSample, black: bool) -> [u16; 4] {
    if black {
        [sample[3], 0, 0, sample[4]]
    } else {
        [sample[0], sample[1], sample[2], sample[4]]
    }
}

/// The warped ink of `source` standing at `origin`, over `dest`.
fn warped_ink(
    source: &CmykImage,
    origin: (i32, i32),
    dest: (i32, i32, u32, u32),
    field: &WarpField,
    job: JobContext<'_>,
) -> Result<CmykImage, String> {
    let samples = source.to_contiguous()?;
    let mut out = vec![[0u16; 5]; dest.2 as usize * dest.3 as usize];
    warp_into(
        rect(origin, source.dimensions()),
        dest,
        field,
        |i| samples[i],
        accumulate_planes16::<5>,
        finish_planes16::<5>,
        |i, p| out[i] = p,
        job,
    )?;
    drop(samples);
    CmykImage::from_contiguous(dest.2, dest.3, &out, job)
}

impl PrecisionDocument {
    /// `warp_info` on a CMYK document: never a field (module doc).
    pub(crate) fn cmyk_warp_info(&self, index: usize) -> Result<Value, String> {
        self.layer(index).ok_or("invalid warp layer index")?;
        Ok(json!({"schema_version":1,"field":null}))
    }

    /// `warp_plan` on a CMYK document (module doc).
    pub(crate) fn cmyk_warp_plan(&self, edit: &WarpEdit) -> Result<SourcePlan, String> {
        let index = edit.layer();
        let layer = self.layer(index).ok_or("invalid warp layer index")?;
        let WarpEdit::Set { target, field, .. } = edit else {
            return Ok(NOTHING);
        };
        let (size, offset) = match target {
            WarpTarget::Pixels => {
                let image = layer
                    .cmyk_source()
                    .ok_or("warp pixels require a raster layer")?;
                self.lock_refusal(index, EditKind::Pixels)?;
                self.lock_refusal(index, EditKind::Position)?;
                (image.dimensions(), layer.offset())
            }
            WarpTarget::Mask => {
                self.lock_refusal(index, EditKind::Mask)?;
                let mask = layer.mask.as_ref().ok_or("warp target has no mask")?;
                (mask.plane.dimensions(), mask.properties.offset)
            }
        };
        let dest = field.bounds(rect(offset, size))?;
        let mut output = 0u64;
        let mut work = 0u64;
        let mut copies = 0u64;
        if field.translation().is_none() {
            let pixels = u64::from(dest.2) * u64::from(dest.3);
            if *target == WarpTarget::Pixels {
                output = pixels * CMYK_BYTES_PER_PIXEL;
                copies = cmyk_source_copy_bytes(size)?
                    .saturating_add(cmyk_adoption_copy_bytes((dest.2, dest.3))?);
            } else {
                output = pixels * 2;
            }
            work = pixels * u64::from(field.filter.kernel().taps() as u32).pow(2) * 64;
        }
        if *target == WarpTarget::Pixels && layer.mask.as_ref().is_some_and(|m| m.properties.linked)
        {
            let m = layer.mask.as_ref().ok_or("warp linked mask is missing")?;
            let d = field.bounds(rect(m.properties.offset, m.plane.dimensions()))?;
            if field.translation().is_none() {
                output = output.saturating_add(u64::from(d.2) * u64::from(d.3) * 2);
                work = work.saturating_add(u64::from(d.2) * u64::from(d.3) * 64);
            }
        }
        if field.identity() {
            return Ok(NOTHING);
        }
        Ok(SourcePlan {
            retained_bytes: self
                .retained_bytes()
                .saturating_add(output)
                .saturating_add(64 * 1024),
            scratch_bytes: output.saturating_add(64 * 1024).saturating_add(copies),
            work_units: work,
        })
    }

    /// `warp_edit` on a CMYK document: applied at once, no binding.
    pub(crate) fn cmyk_warp_edit(
        &self,
        edit: &WarpEdit,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let plan = self.cmyk_warp_plan(edit)?;
        if plan.scratch_bytes == 0 {
            return Ok(None);
        }
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        let WarpEdit::Set { target, field, .. } = edit else {
            return Ok(None);
        };
        let mut field = field.clone();
        field.normalize();
        let mut flat = flatten(&self.layers);
        let layer = &mut flat
            .get_mut(edit.layer())
            .ok_or("invalid warp layer index")?
            .layer;
        let linked = layer.mask.as_ref().is_some_and(|m| m.properties.linked);
        if *target == WarpTarget::Pixels {
            let image = layer
                .cmyk_source()
                .cloned()
                .ok_or("warp pixels require a raster layer")?;
            let origin = layer.offset();
            let (image, offset) = match field.translation() {
                Some(delta) => (image, shifted(origin, delta)?),
                None => {
                    let d = field.bounds(rect(origin, image.dimensions()))?;
                    (warped_ink(&image, origin, d, &field, job)?, (d.0, d.1))
                }
            };
            layer.content = Content::CmykRaster { image, offset };
            // A warp is applied at once, so the pixels are no longer a
            // description's rendering (#1's artwork batch).
            layer.description = None;
        }
        if *target == WarpTarget::Mask || linked {
            let mask = layer.mask.as_mut().ok_or("warp target has no mask")?;
            let origin = mask.properties.offset;
            let (plane, offset) = match field.translation() {
                Some(delta) => (mask.plane.clone(), shifted(origin, delta)?),
                None => {
                    let d = field.bounds(rect(origin, mask.plane.dimensions()))?;
                    let outside = u16::from(mask.properties.outside_coverage) * 257;
                    (
                        warped_mask(&mask.plane, origin, d, &field, outside, job)?,
                        (d.0, d.1),
                    )
                }
            };
            mask.plane = plane;
            mask.properties.offset = offset;
        }
        layer.warp = None;
        let next = self.rebuilt(
            self.width,
            self.height,
            rebuild(flat)?,
            self.channels.as_ref().clone(),
        )?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }

    /// The Liquify plan on a CMYK layer, with `selected_edit_plan`'s keys
    /// (module doc).
    pub(crate) fn cmyk_liquify_plan(
        &self,
        layer: usize,
        strokes: &[LiquifyStroke],
    ) -> Result<Value, String> {
        let image = self
            .layer(layer)
            .and_then(crate::precision_document::PrecisionDocumentLayer::cmyk_source)
            .ok_or("selected edit requires a raster layer")?;
        let (w, h) = image.dimensions();
        let n = u64::from(w) * u64::from(h);
        let (cw, ch) = self.dimensions();
        let canvas = u64::from(cw) * u64::from(ch);
        let output = n * CMYK_BYTES_PER_PIXEL;
        let scratch = (6 * BOOKKEEPING_BYTES)
            .saturating_add(crate::liquify::plan((w, h), strokes)?)
            .saturating_add(n * 16)
            .saturating_add(cmyk_source_copy_bytes((w, h))?)
            .saturating_add(output)
            .saturating_add(canvas * 2)
            .saturating_add(canvas * crate::cmyk_paint::CANVAS_BYTES_PER_PIXEL);
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "output_bytes":output,"input_samples":canvas,"input_width":cw,"input_height":ch}),
        )
    }

    /// Liquify on a CMYK layer (module doc), under the layer's Pixels locks.
    pub(crate) fn cmyk_liquify(
        &self,
        layer: usize,
        strokes: &[LiquifyStroke],
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        validate_coverage(self, coverage)?;
        self.under_locks(layer, EditKind::Pixels, |doc| {
            doc.cmyk_liquify_unlocked(layer, strokes, coverage, limit, other, job)
        })
    }

    fn cmyk_liquify_unlocked(
        &self,
        index: usize,
        strokes: &[LiquifyStroke],
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let scratch = self.cmyk_liquify_plan(index, strokes)?["scratch_bytes"]
            .as_u64()
            .ok_or("invalid CMYK liquify plan")?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let record = self.layer(index).ok_or("invalid selected layer")?;
        let image = record
            .cmyk_source()
            .ok_or("selected edit requires raster source")?;
        let offset = record.offset();
        let (w, h) = image.dimensions();
        let canvas = self.dimensions();
        let mut field = LiquifyField::new((w, h))?;
        field.apply(strokes, job.range(0.0, 0.3))?;
        if field.is_identity() {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let samples = image.to_contiguous()?;
        let render = |black: bool, part: JobContext<'_>| {
            field.render(
                |i| unit_view(&samples[i], black),
                (w, h),
                (0, 0, w, h),
                &SampleRange::Unit,
                // Ink has no transfer.
                None,
                part,
            )
        };
        let cmy = render(false, job.range(0.3, 0.6))?;
        let k = render(true, job.range(0.6, 0.9))?;
        let frozen = self.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        let mut changed = false;
        let out = CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                for (x, slot) in row.iter_mut().enumerate() {
                    let i = y as usize * w as usize + x;
                    let before = samples[i];
                    let weight = coverage_at(coverage, canvas, (w, h), offset, i);
                    let a = mix_codes(codes_view(&before, false), cmy[i], weight)?;
                    let b = mix_codes(codes_view(&before, true), k[i], weight)?;
                    if a[3] != b[3] {
                        return Err("internal error: CMYK liquify views disagree".into());
                    }
                    let mut after = [a[0], a[1], a[2], b[0], a[3]];
                    // `restoring_alpha`'s rule under a transparency lock.
                    if frozen {
                        after = if before[4] == 0 || after[4] == 0 {
                            before
                        } else {
                            [after[0], after[1], after[2], after[3], before[4]]
                        };
                    }
                    changed |= after != before;
                    *slot = after;
                }
                job.every(y as usize * w as usize)
            },
            job,
        )?;
        let result = if changed {
            self.replacing_content(index, Content::CmykRaster { image: out, offset })?
        } else {
            None
        };
        job.report(1.0, 5)?;
        Ok(result)
    }
}
