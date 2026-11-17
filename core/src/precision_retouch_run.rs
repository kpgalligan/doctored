//! The run of a native retouch request (`precision_retouch::RetouchEdit`):
//! the lock gate, the shared coverage prelude, the CMYK and float dispatch
//! and the u16 arm.
//!
//! Since 13B-3d-2 the IN-PLACE arm (a Layer target at its own extent) is a
//! RECT writer: the op's kernel (`precision_retouch_apply`) bounds its write,
//! one `read_rect_rgba16` + `with_rect_rgba16` (no view, ≤ 4 new tiles per
//! dab); a new layer or a grown extent keeps the whole buffer (rows in).
//!
//! Scalar stroke coverage stays separate from source pixels. A compound job's reservation floor
//! keeps nested sampling/render stages from hiding already-live buffers. Expansion preserves
//! existing samples and independent mask placement; healing retains destination alpha.
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage,
};
use crate::precision_inpaint::SynthesisPlate;
use crate::precision_paint::canvas_index;
use crate::precision_retouch::{RetouchEdit, RetouchOp, RetouchOutput};
use crate::precision_retouch_apply::RetouchKernel;
use crate::precision_retouch_source::SynthesisSource;
use crate::retouch_coverage::{selection_domain, CoveragePlane};
use crate::retouch_sampling::RetouchSampling;

impl PrecisionDocument {
    pub fn retouch(
        &self,
        edit: &RetouchEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        self.retouch_with_context(
            edit,
            coverage,
            limit,
            other,
            control.map_or(JobContext::NONE, JobContext::new),
        )
    }
    /// The LOCK gate (`precision_lock`): a retouch writes its target's
    /// pixels, so it runs under a `Pixels` edit. NEW-LAYER output only
    /// CONSULTS the lock and never restores the target's alpha, exactly as
    /// `doc_retouch_output::retouch_onto_new_layer` does: the target's own
    /// buffer is not written, so there is nothing to freeze — and its own
    /// no-op latch would otherwise throw the new layer away.
    pub(crate) fn retouch_with_context(
        &self,
        edit: &RetouchEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if edit.output == RetouchOutput::NewLayer {
            self.lock_refusal(edit.layer, crate::doc_lock::EditKind::Pixels)?;
            return self.retouching_unlocked(self, edit, coverage, limit, other, job);
        }
        self.under_locks(edit.layer, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.retouching_unlocked(doc, edit, coverage, limit, other, job)
        })
    }

    /// `retouch`, with the sampling plate rendered from `plates` instead of
    /// from `self`: Content-Aware Move heals the moved pixels onto the
    /// FILLED document while reading them from the picture before the fill.
    /// Only the plate comes from `plates`; the lock gate, the plan, the
    /// admission and the rect writer are `self`'s, so `retouch_from(self, …)`
    /// is `retouch` to the byte. The plate's samples are read as they are:
    /// `plates` is taken to be in `self`'s working space.
    ///
    /// `Err` unless op is `retouch_clone` or `retouch_heal`, output is
    /// `layer`, `expand_destination` is false, and `plates` has `self`'s
    /// canvas size, its bit depth (16-bit or 32-bit float) and a raster
    /// layer at the sampled index (`source_layer`, else `layer`). `Ok(None)`
    /// when nothing would change, as `retouch`.
    pub fn retouch_from(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        self.retouch_from_refusal(plates, edit)?;
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.under_locks(edit.layer, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.retouching_unlocked(plates, edit, coverage, limit, other, job)
        })
    }

    /// The refusals [`Self::retouch_from`] documents, ahead of any plan: the
    /// export asks them first so a request it cannot run is named as such
    /// rather than by whatever its plan would have refused.
    pub(crate) fn retouch_from_refusal(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
    ) -> Result<(), String> {
        if self.is_cmyk() {
            return self.cmyk_retouch_from_refusal(plates, edit);
        }
        if !matches!(edit.op, RetouchOp::RetouchClone | RetouchOp::RetouchHeal) {
            return Err(
                "retouch from another document's plates requires retouch_clone or retouch_heal"
                    .into(),
            );
        }
        if edit.output != RetouchOutput::Layer || edit.expand_destination {
            return Err(
                "retouch from another document's plates writes its target layer in place: output must be layer and expand_destination false"
                    .into(),
            );
        }
        if plates.dimensions() != self.dimensions() {
            return Err("retouch plates must have the document's canvas size".into());
        }
        if plates.is_float() != self.is_float() {
            return Err("retouch plates must have the document's bit depth".into());
        }
        if plates
            .layer_source(edit.source_layer.unwrap_or(edit.layer))
            .is_none()
        {
            return Err("retouch plates have no raster layer at the sampled index".into());
        }
        Ok(())
    }

    /// `plates` is the document the sampling plate is rendered from: `self`
    /// for `retouch`, the caller's for `retouch_from`. Nothing else reads it.
    /// A CMYK document's two-pass retouch (`cmyk_retouch`) runs it on its
    /// pass documents.
    pub(crate) fn retouching_unlocked(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let plan = self.retouch_plan(edit)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        let job = job.with_reservation_floor(plan.scratch_bytes);
        if coverage.len() != plan.input_samples {
            return Err("retouch coverage extent mismatch".into());
        }
        // The planes over the request's coverage rect (the canvas without
        // one), 13B-9c: no canvas-sized allocation or scan when it is named.
        let (canvas, area) = (self.dimensions(), edit.coverage_area(self)?);
        let m = area.2 as usize * area.3 as usize;
        let raw = CoveragePlane::new(&coverage[..m], area, canvas)?;
        let selection = edit
            .selection
            .then(|| CoveragePlane::new(&coverage[m..], area, canvas))
            .transpose()?;
        // Synthesis retains the published half-coverage stroke contour. A tiny
        // positive selection remains in the domain and fades only at writeback.
        // The domain covers the stroke's bbox only: outside it the stroke is 0.
        let stroke = raw.bbox();
        let domain = match (selection, stroke) {
            (Some(selection), Some(bbox)) => {
                Some((selection_domain(raw, selection, bbox, job)?, bbox))
            }
            _ => None,
        };
        let coverage = match &domain {
            Some((samples, bbox)) => CoveragePlane::new(samples, *bbox, canvas)?,
            None => raw,
        };
        // The admitted-bounds check and the no-op test read the WORKING
        // plane (the domain under a selection), in that order: a sample the
        // selection zeroes is not coverage. A bbox inside the bounds holds
        // every covered sample, and none outside it is covered.
        let covered = if domain.is_some() {
            coverage.bbox()
        } else {
            stroke
        };
        if edit.width > 0 && edit.height > 0 && edit.op != RetouchOp::RedEye {
            let (x, y, w, h) = edit.bounds(self)?;
            if let Some((cx, cy, cw, ch)) = covered {
                if cx < x
                    || cy < y
                    || u64::from(cx) + u64::from(cw) > u64::from(x) + u64::from(w)
                    || u64::from(cy) + u64::from(ch) > u64::from(y) + u64::from(h)
                {
                    return Err("retouch coverage exceeds its admitted bounds".into());
                }
            }
        }
        if edit.opacity == 0. || covered.is_none() {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        if self.is_cmyk() {
            return self.cmyk_retouching(
                plates,
                edit,
                coverage,
                selection,
                plan.scratch_bytes,
                limit,
                other,
                job,
            );
        }
        if self.is_float() {
            // The f32 arm (`float_retouch`), after the shared prelude above.
            return self.float_retouching(
                plates,
                edit,
                coverage,
                selection,
                plan.scratch_bytes,
                limit,
                other,
                job,
            );
        }
        let target = self.layer(edit.layer).ok_or("retouch target missing")?;
        let original = target.source().ok_or("retouch target requires pixels")?;
        if original.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("retouch requires u16 storage".into());
        }
        let (dimensions, offset) = edit.extent(self)?;
        let in_place = edit.output == RetouchOutput::Layer
            && (dimensions, offset) == (original.dimensions(), target.offset());
        let staged = if in_place {
            None
        } else {
            Some(self.retouch_destination(edit, target, (dimensions, offset), limit, other, job)?)
        };
        // A Clone / Heal reads its taps over its frame only (13B-9a).
        let sampled = matches!(edit.op, RetouchOp::RetouchClone | RetouchOp::RetouchHeal);
        let frames = sampled
            .then(|| RetouchKernel::<u16>::sampled_frames(edit, coverage, self.dimensions()))
            .transpose()?;
        let (index, mode) = (edit.source_layer.unwrap_or(edit.layer), edit.mode()?);
        let (ignore, plate_job) = (edit.ignore_adjustments, job.range(0.08, 0.2));
        let taps = frames
            .map(|f| plates.retouch_taps16(index, mode, ignore, f.2, limit, other, plate_job))
            .transpose()?;
        let source = plates.synthesis_source(edit, limit, other, plate_job)?;
        job.reserve(plan.scratch_bytes)?;
        // A plate render reaching its own end is only preparation of this
        // operation. Preserve room for synthesis and atomic publication.
        let edit_job = job.range(0.2, 0.98);
        edit_job.report(0.0, 2)?;
        let src = source.as_ref().map(SynthesisSource::plate);
        let kernel = match frames.zip(taps) {
            Some((frames, taps)) => RetouchKernel::sampled(self, edit, &taps, frames, edit_job)?,
            None => RetouchKernel::prepare(self, edit, coverage, plan.scratch_bytes, edit_job)?,
        };
        let Some((mut pixels, baseline, backdrop)) = staged else {
            let io = (src, coverage, selection);
            return self.retouched_in_place(edit, &kernel, io, target, [job, edit_job]);
        };
        let plane = (dimensions, offset);
        kernel.apply(
            self,
            edit,
            src,
            coverage,
            selection,
            &mut pixels,
            plane,
            edit_job,
        )?;
        let mut changed = false;
        for (i, (a, b)) in pixels.iter().zip(&baseline).enumerate() {
            job.every(i)?;
            changed |= a != b;
        }
        if !changed {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        if backdrop.is_some() {
            for (i, p) in pixels.iter_mut().enumerate() {
                job.every(i)?;
                let threshold = if edit.op == RetouchOp::ContentAwareFill {
                    1
                } else {
                    32768
                };
                if coverage.at(i) < threshold || *p == baseline[i] {
                    *p = [0; 4];
                }
            }
        }
        let result = if edit.output == RetouchOutput::NewLayer {
            let image = PrecisionImage::from_rgba16_unseeded(
                dimensions.0,
                dimensions.1,
                pixels,
                self.profile.clone(),
                None,
            )?;
            Some(self.retouch_insert(edit.layer, image, edit.output_name())?)
        } else {
            let image = original.rebuilt_rgba16(
                dimensions.0,
                dimensions.1,
                pixels,
                self.profile.clone(),
            )?;
            let mut next = self.replacing_content(edit.layer, Content::Raster { image, offset })?;
            if let Some(next) = next.as_mut() {
                if let Some(comps) = self.comps_after_growth(edit.layer, offset) {
                    next.comps = comps;
                }
            }
            next
        };
        job.report(1., 5)?;
        Ok(result)
    }

    /// A NOT-in-place destination (a new layer, or a grown extent): HEAD's
    /// extent buffer, its baseline copy and the new layer's backdrop plate.
    #[allow(clippy::type_complexity)]
    fn retouch_destination(
        &self,
        edit: &RetouchEdit,
        target: &PrecisionDocumentLayer,
        (dimensions, offset): ((u32, u32), (i32, i32)),
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(Vec<[u16; 4]>, Vec<[u16; 4]>, Option<PrecisionImage>), String> {
        let original = target.source().ok_or("retouch target requires pixels")?;
        let mut pixels = reserved_vec(pixel_count(dimensions.0, dimensions.1)?)?;
        pixels.resize(dimensions.0 as usize * dimensions.1 as usize, [0; 4]);
        let backdrop =
            if edit.output == RetouchOutput::NewLayer && edit.op != RetouchOp::RetouchClone {
                Some(self.retouch_source_with_context(
                    edit.layer,
                    RetouchSampling::CurrentAndBelow,
                    false,
                    limit,
                    other,
                    job.range(0.0, 0.08),
                )?)
            } else {
                None
            };
        if let Some(plate) = &backdrop {
            if plate.storage() != PrecisionStorage::Rgba16Unorm {
                return Err("retouch backdrop requires u16".into());
            }
            let (pw, ph) = plate.dimensions();
            debug_assert_eq!((pw, ph), dimensions);
            let stride = dimensions.0 as usize;
            for y in 0..ph {
                for (x, run) in plate.row_segments(y, 0, pw) {
                    let PrecisionRun::Rgba16(run) = run else {
                        continue;
                    };
                    let at = y as usize * stride + x as usize;
                    pixels[at..at + run.len()].copy_from_slice(run);
                }
            }
        } else if edit.output == RetouchOutput::Layer {
            let ow = original.dimensions().0;
            for y in 0..original.dimensions().1 {
                for (x, run) in original.row_segments(y, 0, ow) {
                    let PrecisionRun::Rgba16(run) = run else {
                        return Err("retouch requires u16 storage".into());
                    };
                    for (k, p) in run.iter().enumerate() {
                        let i = y as usize * ow as usize + x as usize + k;
                        job.every(i)?;
                        let dx = (i % ow as usize) as i64 + i64::from(target.offset().0)
                            - i64::from(offset.0);
                        let dy = (i / ow as usize) as i64 + i64::from(target.offset().1)
                            - i64::from(offset.1);
                        if let Some(di) = canvas_index(dx, dy, dimensions.0, dimensions.1) {
                            pixels[di] = *p;
                        }
                    }
                }
            }
        }
        let baseline = pixels.clone();
        Ok((pixels, baseline, backdrop))
    }

    /// The IN-PLACE arm: the kernel's `bound` on the layer is ONE rect read
    /// and written back through `with_rect_rgba16` (the changed latch); a
    /// bound missing the layer runs the write over an EMPTY plane (HEAD's
    /// solve and refusals, nothing written) and answers `Ok(None)`.
    fn retouched_in_place(
        &self,
        edit: &RetouchEdit,
        kernel: &RetouchKernel,
        (source, coverage, selection): (
            Option<SynthesisPlate<'_>>,
            CoveragePlane<'_>,
            Option<CoveragePlane<'_>>,
        ),
        target: &PrecisionDocumentLayer,
        [job, edit_job]: [JobContext<'_>; 2],
    ) -> Result<Option<Self>, String> {
        let original = target.source().ok_or("retouch target requires pixels")?;
        let (canvas, offset) = (self.dimensions(), target.offset());
        let rect = kernel
            .bound(edit)
            .and_then(|b| canvas_rect_on_plane(canvas, original.dimensions(), offset, b));
        let (mut pixels, plane) = match rect {
            Some((x, y, w, h)) => (
                original
                    .read_rect_rgba16(x, y, w, h)
                    .ok_or("could not allocate precision output")?,
                ((w, h), plane_rect_offset(offset, (x, y, w, h))),
            ),
            None => (Vec::new(), ((0, 0), offset)),
        };
        kernel.apply(
            self,
            edit,
            source,
            coverage,
            selection,
            &mut pixels,
            plane,
            edit_job,
        )?;
        let written = rect.map(|(x, y, w, h)| original.with_rect_rgba16(x, y, w, h, &pixels));
        let Some(image) = written.transpose()?.flatten() else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let result = self.replacing_content(edit.layer, Content::Raster { image, offset })?;
        job.report(1.0, 5)?;
        Ok(result)
    }
}
