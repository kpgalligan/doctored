//! The RECT path of a CMYK document's in-place Clone and Heal (13B-9c,
//! `cmyk_retouch`'s module doc has the two passes): the passes run over the
//! stroke's rect, not the canvas.
//!
//! `RetouchKernel::sampled_frames` on the working coverage gives the bound
//! and the taps; `R` is the bound on the layer (`canvas_rect_on_plane`; none:
//! nothing changes). Each pass document is the canvas with layer 0 the
//! layer's rect `R` (`cmyk_passes::pass_rect_image`, blank for a clone) at
//! its canvas position, and the plates document holds the plate over the
//! taps only (`cmyk_retouch_plate_rect`) at the taps' origin. The RGB walk
//! reads and writes only destination samples inside the bound and taps only
//! inside the taps (`precision_retouch_apply`'s module doc), so each pass
//! answers the canvas pass's samples over `R`. The pass input is the rect
//! form (`cmyk_pass_input`). A Heal joins over `R` (`cmyk_passes::join_rect`);
//! a Clone composites over `R` at canvas positions (`cloned_sample`) and
//! writes the layer back through `with_rect`. Every per-pixel statement is
//! `join_sample` / `over_mode*`'s, so the answer is the full-frame path's to
//! the bit (`core/tests/cmyk_retouch_rect.rs`).
//!
//! When every tap is off the canvas (`tap_rect` answers the empty rect) no
//! plate is built: the plates document holds one PAPER, α 0 pixel at the
//! canvas origin, which no tap reads, and the passes run as usual.
//!
//! The plan's terms ([`PrecisionDocument::cmyk_rect_retouch_scratch`]): one
//! pass's RGB plan on an `R`-sized stand-in twice, `R × 34` for the
//! destination's passes, its read and its rewritten ink, the taps' two passes
//! (`taps × 16`) and a `Current` plate over the taps (`taps × 10`, at least
//! one pixel), the latter three at the in-place plan's 5/4 margin; `R` is the
//! planned frame (`precision_retouch_plan`). A rendered plate keeps its own
//! plan (`cmyk_plate_scratch`). An in-place Spot Heal or Content-Aware Fill
//! plans the RGB arm's in-place terms at four channels
//! ([`PrecisionDocument::cmyk_synthesis_in_place_scratch`]).

use crate::blend::BlendMode;
use crate::cmyk_image::{CmykImage, PAPER};
use crate::cmyk_passes::{
    join_rect, pass_document, pass_image, pass_pair, pass_rect_image, pass_result, InkPass,
    BLANK_PASS,
};
use crate::cmyk_retouch::{cloned_sample, pass_edit, Planes};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::precision_retouch::{RetouchEdit, RetouchOp};
use crate::precision_retouch_apply::RetouchKernel;
use crate::precision_retouch_plan::{
    domain_bytes, margin, rect_tiles, synthesis_caller, TILE_PIXELS,
};
use crate::retouch_sampling::RetouchSampling;

impl PrecisionDocument {
    /// The rect path (module doc) of an in-place Clone or Heal on `dest`
    /// standing at `offset` (`frozen`: a transparency lock): the layer's new
    /// ink, or `None` when nothing changes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_rect_retouched(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
        dest: &CmykImage,
        (offset, frozen): ((i32, i32), bool),
        io: Planes<'_>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<CmykImage>, String> {
        let canvas = self.dimensions();
        let (bound, _, taps) = RetouchKernel::<u16>::sampled_frames(edit, io.0, canvas)?;
        let Some(r) =
            bound.and_then(|b| canvas_rect_on_plane(canvas, dest.dimensions(), offset, b))
        else {
            return Ok(None);
        };
        let at = plane_rect_offset(offset, r);
        let (plate, plate_at) = if taps.0 < taps.2 && taps.1 < taps.3 {
            let (index, mode) = (edit.source_layer.unwrap_or(edit.layer), edit.mode()?);
            let ignore = edit.ignore_adjustments;
            let plate =
                plates.cmyk_retouch_plate_rect(index, mode, ignore, taps, job.range(0.0, 0.1))?;
            (plate, (taps.0 as i32, taps.1 as i32))
        } else {
            (CmykImage::filled(1, 1, PAPER)?, (0, 0))
        };
        let blank = edit.op == RetouchOp::RetouchClone;
        let mut pass = pass_edit(edit);
        let (input, rect) = self.cmyk_pass_input(edit, io)?;
        pass.coverage_rect = rect;
        let held = self.retained_bytes().saturating_add(other);
        let mut out = Vec::with_capacity(2);
        let mut changed = false;
        for (k, which) in [InkPass::Cmy, InkPass::K].into_iter().enumerate() {
            let range = job.range(0.1 + k as f64 * 0.4, 0.1 + (k + 1) as f64 * 0.4);
            let doc = pass_document(
                canvas,
                pass_rect_image(dest, which, blank, r, range)?,
                at,
                0,
            )?;
            let taps = pass_document(
                canvas,
                pass_image(&plate, which, false, range)?,
                plate_at,
                0,
            )?;
            let result = doc.retouching_unlocked(&taps, &pass, &input, limit, held, range)?;
            changed |= result.is_some();
            out.push(pass_result(result.as_ref().unwrap_or(&doc))?.clone());
        }
        if !changed {
            return Ok(None);
        }
        let b = out.pop().ok_or("internal error: a CMYK pass is missing")?;
        let a = out.pop().ok_or("internal error: a CMYK pass is missing")?;
        let stage = job.range(0.9, 1.0);
        if edit.op == RetouchOp::RetouchHeal {
            return join_rect(dest, r, &a, Some(&b), frozen, stage);
        }
        clone_rect(dest, r, offset, [&a, &b], edit, frozen, stage)
    }

    /// The rect path's plan terms (module doc), in place of the canvas
    /// two-pass term and, for `sample: current`, the canvas plate.
    pub(crate) fn cmyk_rect_retouch_scratch(
        &self,
        edit: &RetouchEdit,
        mode: RetouchSampling,
    ) -> Result<u64, String> {
        let canvas = self.dimensions();
        let frame = self.retouch_planned_frame(edit)?;
        let (rw, rh) = ((frame.2 - frame.0) as u32, (frame.3 - frame.1) as u32);
        // The stand-in of a pass document: an `R`-sized blank pass at the
        // frame's origin (one pixel when the frame is empty).
        let image =
            PrecisionImage::filled_rgba16(rw.max(1), rh.max(1), BLANK_PASS, IccProfile::srgb())?;
        let stand_in = pass_document(canvas, image, (frame.0 as i32, frame.1 as i32), 0)?;
        let rgb = stand_in.retouch_plan(&pass_edit(edit))?.scratch_bytes;
        let taps = crate::retouch_transform::tap_rect(canvas, &edit.matrix, frame)?;
        let taps = ((taps.2 - taps.0) * (taps.3 - taps.1)) as u64;
        let plate = if mode == RetouchSampling::Current {
            taps.max(1) * 10
        } else {
            0
        };
        let r = u64::from(rw) * u64::from(rh);
        Ok(2 * rgb + margin(r * 34 + taps * 16 + plate))
    }

    /// An in-place Spot Heal or Content-Aware Fill's terms on ink (module
    /// doc): the RGB arm's in-place terms (`precision_retouch_plan`) at four
    /// channels, the destination rect and its rewritten tiles at 10 bytes a
    /// pixel.
    pub(crate) fn cmyk_synthesis_in_place_scratch(
        &self,
        edit: &RetouchEdit,
    ) -> Result<u64, String> {
        let (bounds, (ww, wh)) = self.synthesis_window(edit)?;
        let area = u64::from(bounds.2) * u64::from(bounds.3);
        let window = u64::from(ww) * u64::from(wh);
        let tiles = rect_tiles(ww, wh) * TILE_PIXELS;
        let mut scratch = margin(domain_bytes(self, edit)? + (window + tiles) * 10);
        scratch += crate::precision_inpaint::estimate(
            self.dimensions(),
            bounds,
            area,
            edit.ring,
            synthesis_caller(edit),
            edit.spot_type,
            4,
            area,
        )?
        .0;
        if edit.source_mask.is_some() {
            scratch += u64::from(self.width) * u64::from(self.height);
        }
        Ok(scratch)
    }
}

/// The Clone's composite over `dest`'s rect `r` (module doc): each sample of
/// the rect under pass results `a` and `b` of its size, at its canvas
/// position, written back through `with_rect`.
fn clone_rect(
    dest: &CmykImage,
    r: (u32, u32, u32, u32),
    offset: (i32, i32),
    [a, b]: [&PrecisionImage; 2],
    edit: &RetouchEdit,
    frozen: bool,
    job: JobContext<'_>,
) -> Result<Option<CmykImage>, String> {
    let mode = BlendMode::from_c(edit.blend_mode as i32).ok_or("retouch blend mode is invalid")?;
    let (x, y, w, _) = r;
    let mut pixels = dest.read_rect(r, job)?;
    for (i, s) in pixels.iter_mut().enumerate() {
        job.every(i)?;
        let (lx, ly) = ((i % w as usize) as u32, (i / w as usize) as u32);
        let passes = pass_pair(a, Some(b), (lx, ly))?;
        let xy = (
            i64::from(x + lx) + i64::from(offset.0),
            i64::from(y + ly) + i64::from(offset.1),
        );
        *s = cloned_sample(*s, passes, (edit.opacity, mode), xy, frozen);
    }
    dest.with_rect(r, &pixels, job)
}
