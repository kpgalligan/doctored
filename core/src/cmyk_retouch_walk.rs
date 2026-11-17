//! `cmyk_retouching`'s walks over the whole destination (a new layer, a
//! grown extent, the full-frame seam) and the four-ink synthesis;
//! `cmyk_retouch`'s module doc holds their rules.

use crate::blend::BlendMode;
use crate::cmyk_image::CmykImage;
use crate::cmyk_passes::{join_images, pass_document, pass_image, pass_pair, pass_result, InkPass};
use crate::cmyk_retouch::{cloned_sample, pass_edit, Planes};
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::precision_inpaint::SynthesisPlate;
use crate::precision_retouch::{RetouchEdit, RetouchOp};
use crate::precision_retouch_plan::synthesis_caller;

impl PrecisionDocument {
    /// Runs `edit`'s RGB walk on `dest`'s two passes (blank for a clone)
    /// over the plate's passes; each result's layer-0 image.
    #[allow(clippy::too_many_arguments)]
    fn cmyk_retouch_passes(
        &self,
        edit: &RetouchEdit,
        dest: &CmykImage,
        offset: (i32, i32),
        plate: &CmykImage,
        io: Planes<'_>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<[PrecisionImage; 2]>, String> {
        let blank = edit.op == RetouchOp::RetouchClone;
        let mut pass = pass_edit(edit);
        let (input, rect) = self.cmyk_pass_input(edit, io)?;
        pass.coverage_rect = rect;
        let held = self.retained_bytes().saturating_add(other);
        let mut out = Vec::with_capacity(2);
        let mut changed = false;
        for (k, which) in [InkPass::Cmy, InkPass::K].into_iter().enumerate() {
            let range = job.range(k as f64 * 0.45, (k + 1) as f64 * 0.45);
            let doc = pass_document(
                self.dimensions(),
                pass_image(dest, which, blank, range)?,
                offset,
                0,
            )?;
            let plates = Self::cmyk_plate_document(plate, which)?;
            let result = doc.retouching_unlocked(&plates, &pass, &input, limit, held, range)?;
            changed |= result.is_some();
            out.push(pass_result(result.as_ref().unwrap_or(&doc))?.clone());
        }
        if !changed {
            return Ok(None);
        }
        let b = out.pop().ok_or("internal error: a CMYK pass is missing")?;
        let a = out.pop().ok_or("internal error: a CMYK pass is missing")?;
        Ok(Some([a, b]))
    }

    /// The Clone Stamp on ink (`cmyk_retouch`'s module doc).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_cloned(
        &self,
        edit: &RetouchEdit,
        dest: &CmykImage,
        offset: (i32, i32),
        plate: &CmykImage,
        io: Planes<'_>,
        frozen: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<CmykImage>, String> {
        let Some([a, b]) = self.cmyk_retouch_passes(
            edit,
            dest,
            offset,
            plate,
            io,
            limit,
            other,
            job.range(0.0, 0.9),
        )?
        else {
            return Ok(None);
        };
        let mode =
            BlendMode::from_c(edit.blend_mode as i32).ok_or("retouch blend mode is invalid")?;
        let (w, h) = dest.dimensions();
        let mut changed = false;
        let out = CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                let mut x = 0u32;
                for (_, run) in dest.row_segments(y, 0, w) {
                    for s in run {
                        let passes = pass_pair(&a, Some(&b), (x, y))?;
                        let xy = (
                            i64::from(x) + i64::from(offset.0),
                            i64::from(y) + i64::from(offset.1),
                        );
                        let next = cloned_sample(*s, passes, (edit.opacity, mode), xy, frozen);
                        changed |= next != *s;
                        row[x as usize] = next;
                        x += 1;
                    }
                }
                job.every(y as usize * w as usize)
            },
            job.range(0.9, 1.0),
        )?;
        Ok(changed.then_some(out))
    }

    /// The Healing Brush on ink (`cmyk_retouch`'s module doc).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_healed(
        &self,
        edit: &RetouchEdit,
        dest: &CmykImage,
        offset: (i32, i32),
        plate: &CmykImage,
        io: Planes<'_>,
        frozen: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<CmykImage>, String> {
        let Some([a, b]) = self.cmyk_retouch_passes(
            edit,
            dest,
            offset,
            plate,
            io,
            limit,
            other,
            job.range(0.0, 0.9),
        )?
        else {
            return Ok(None);
        };
        join_images(dest, &a, Some(&b), frozen, job.range(0.9, 1.0))
    }

    /// Spot Healing and Content-Aware Fill on the four inks (`cmyk_retouch`'s module doc).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_synthesized(
        &self,
        edit: &RetouchEdit,
        dest: &CmykImage,
        offset: (i32, i32),
        plate: SynthesisPlate<'_>,
        (coverage, selection): Planes<'_>,
        scratch: u64,
        job: JobContext<'_>,
    ) -> Result<Option<CmykImage>, String> {
        let bounds = edit.bounds(self)?;
        let domain = edit.synthesis_domain(self)?;
        let estimate = crate::precision_inpaint::estimate(
            self.dimensions(),
            bounds,
            u64::from(bounds.2) * u64::from(bounds.3),
            edit.ring,
            synthesis_caller(edit),
            edit.spot_type,
            4,
            u64::from(domain.2) * u64::from(domain.3),
        )?
        .0;
        let synthesis = crate::precision_inpaint::plan(
            self.dimensions(),
            domain,
            coverage,
            edit.ring,
            edit.op == RetouchOp::ContentAwareFill,
            edit.spot_type,
            scratch.saturating_sub(estimate),
            4,
            job,
        )?;
        let eligibility = edit
            .source_mask
            .as_ref()
            .map(|m| m.compile(self.width, self.height, job))
            .transpose()?;
        let canvas = self.dimensions();
        let rect = synthesis
            .window_union()
            .and_then(|b| canvas_rect_on_plane(canvas, dest.dimensions(), offset, b));
        let (mut pixels, plane) = match rect {
            Some(rect) => (
                dest.read_rect(rect, job)?,
                ((rect.2, rect.3), plane_rect_offset(offset, rect)),
            ),
            None => (Vec::new(), ((0, 0), offset)),
        };
        crate::precision_inpaint::apply::<u16, 4>(
            &synthesis,
            plate,
            coverage,
            eligibility.as_deref(),
            edit.opacity,
            edit.seed,
            pixels.as_flattened_mut(),
            plane.0,
            plane.1,
            selection,
            job,
        )?;
        match rect {
            Some(rect) => dest.with_rect(rect, &pixels, job),
            None => Ok(None),
        }
    }
}
