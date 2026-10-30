//! The typed retouch in a 32-bit float document: the f32 arm of
//! `retouch_clone`, `retouch_heal`, `spot_heal` and `content_aware_fill`,
//! which is what the Clone Stamp, the Healing Brush, Spot Healing, Patch,
//! Remove, Content-Aware Fill and Content-Aware Move send.
//!
//! `precision_retouch` is the MASTER of every step here but the sample type.
//! Its shared prelude (the plan and admission, the coverage domain and
//! bounds checks, the lock gate) runs first and then hands a float document
//! to [`PrecisionDocument::float_retouching`], which mirrors the master's two
//! arms over f32: in place, the kernel's `bound` is ONE `read_rect_rgba32`
//! written back through `with_rect_rgba32`; a new layer or a grown extent
//! keeps the whole extent buffer, compares it with its baseline by bits,
//! blanks a new layer's untouched backdrop and builds the image through
//! `from_rgba32_extended` / `rebuilt_rgba32_extended`. The plate cut, the
//! source mapping, the component walk, the membrane, the synthesis and the
//! blend are the shared implementations, called and never copied.
//!
//! # What each op does at f32
//!
//! - **Clone** copies in LINEAR light: the plate through
//!   `retouch_transform::transform_extended` (premultiplied bilinear), then
//!   `blend::composite_source_into` / `composite_source_frozen` at each
//!   mode's kind from `blend_float::blend_kind_at`. No encoding and no clamp:
//!   a cloned 8.0 reads 8.0. Its modes are `float_document::admitted_blend`;
//!   another refuses with `blend_refusal` ([`plan_refusal`]). Ink alpha under
//!   `sampling::EXTENDED_ALPHA_FLOOR` is skipped, as the float Brush skips
//!   it. A result outside the stored range (a Multiply of two large values)
//!   refuses through the f32 writers, as the float Brush's does.
//! - **Heal, Spot Heal, Content-Aware Fill** compare and solve in W codes
//!   (`retouch_encoding`, whose module doc says why): the destination rect is
//!   encoded, the shared kernel runs on it (`precision_retouch_heal::apply`
//!   over the encoded plate; `precision_inpaint::apply`, which encodes the
//!   linear plate at its read), and `retouch_encoding::decode_changed`
//!   decodes only the samples the kernel changed, so every other sample keeps
//!   its bits. The membrane writeback clamps `source + u` to the stored range
//!   as the 8- and 16-bit writeback clamps to its own (`heal_writeback`'s f32
//!   sample), and `decode` answers inside `±FLOAT_RANGE_MAX`, so an overshoot
//!   lands at the stored bound instead of refusing. That bound is the only
//!   clamp: this module adds none.
//! - **Red Eye** (the Red Eye tool and Filter ▸ Remove Red Eye) runs the
//!   shared kernel (`precision_redeye::apply::<f32>`) on its rect's
//!   DISPLAY-ENCODED copy (`display_encoding`): the red test reads the
//!   encoded values clamped to 0…1, as at 16 bits, the correction is applied
//!   to the encoded values unclamped, and `DisplayEncoding::decode_changed`
//!   decodes only what it changed. It builds no sampling plate, as the
//!   16-bit master builds none.
//!
//! # Admission
//!
//! `retouch_plan` charges a float document `2·(n·(40|44) + output·16)` for
//! the pixel buffers (the master's figure at f32 width), plus `output·16` for
//! the W plane of a heal, spot heal or fill; the heal, synthesis and plate
//! terms are the master's (the plate at 16 bytes a pixel).
//!
//! Measured in Release, one case per process, `MallocLargeCache=0`, a
//! baseline twin and three runs (`examples/float_retouch_admission.rs`), on a
//! 6000×4000 opaque float layer; `E` is the run's `phys_footprint` peak less
//! the footprint read just before the call, the largest of the three runs.
//! The clone covers the canvas under a translation by (64, 0); the others
//! cover the 1000×1000 box at (2500, 1500) (heal: the same translation; spot
//! heal and fill: ring 0, seed 7; the new-layer fill samples `all`).
//!
//! | case | plan A (`scratch_bytes`) | max E | ⌈1.25 E⌉ |
//! |---|---|---|---|
//! | RC32 (`retouch_clone`) | 3,084,582,912 | 1,539,933,288 | 1,924,916,610 |
//! | RH32 (`retouch_heal`) | 3,629,223,552 | 1,154,171,696 | 1,442,714,620 |
//! | RS32 (`spot_heal`) | 3,947,924,992 | 769,966,632 | 962,458,290 |
//! | RF32 (`content_aware_fill`) | 3,947,924,992 | 769,950,224 | 962,437,780 |
//! | RN32 (`content_aware_fill`, new layer) | 6,306,216,448 | 2,136,524,336 | 2,670,655,420 |
//! | RE32 (`red_eye`, the whole canvas) | 2,696,388,608 | 984,662,736 | 1,230,828,420 |
//!
//! `A ≥ ⌈1.25 E⌉` in each, so the multipliers stand; the floors are pinned by
//! `selected_color_planes_24mp.rs` (`FLOOR_RC32`, `FLOOR_RH32`, `FLOOR_RS32`,
//! `FLOOR_RF32`, `FLOOR_RN32`, `FLOOR_RE32`). Red Eye's display-encoded copy
//! of its rect (n×16 at most) sits inside the f32 buffer term the plan
//! already doubles (2026-10-12; its twin peaks at 1,155,384,424, below the
//! run's 1,421,411,648, so E is the retouch's own). The twin's own peak (building the document)
//! is 1,155,400,832, above the pre-call footprint of 436.7 MB, so the spot
//! heal's and the fill's `E` is an upper bound on what the retouch holds.

use crate::blend::BlendMode;
use crate::blend_float::blend_kind_at;
use crate::doc_lock::LOCK_TRANSPARENCY;
use crate::float_document::{admitted_blend, blend_refusal};
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    pixel_count, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage,
};
use crate::precision_paint::{blend_preserving, canvas_index};
use crate::precision_retouch::{RetouchEdit, RetouchOp, RetouchOutput};
use crate::precision_retouch_apply::RetouchKernel;
use crate::retouch_encoding::{decode_changed, encode, encode_plane};
use crate::retouch_sampling::RetouchSampling;
use crate::sampling::EXTENDED_ALPHA_FLOOR;

const STORAGE: &str = "retouch in a 32-bit float document requires f32 storage";

/// A float document's own refusal of a typed retouch, asked first by
/// `retouch_plan`: a clone mode outside `admitted_blend` by the float blend
/// sentence. A mode number that names no mode is left to the shared
/// validation.
pub(crate) fn plan_refusal(edit: &RetouchEdit) -> Result<(), String> {
    match BlendMode::from_c(edit.blend_mode as i32) {
        Some(mode) if !mode.is_group_only() && !admitted_blend(mode) => Err(blend_refusal(mode)),
        _ => Ok(()),
    }
}

/// `a` and `b` differ in some bit.
fn differs(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a.map(f32::to_bits) != b.map(f32::to_bits)
}

impl RetouchKernel<f32> {
    /// `RetouchKernel::prepare` over f32 (module doc): Clone's plate is the
    /// transformed linear samples; Heal's is those with RGB encoded in place;
    /// Spot Heal and Content-Aware Fill share the master's synthesis plan.
    pub(crate) fn prepare_float(
        doc: &PrecisionDocument,
        edit: &RetouchEdit,
        source: Option<&PrecisionImage>,
        coverage: &[u16],
        scratch_bytes: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        match edit.op {
            RetouchOp::RetouchClone | RetouchOp::RetouchHeal => {
                let samples = source
                    .filter(|s| s.storage() == PrecisionStorage::Rgba32Float)
                    .ok_or("missing native retouch source")?
                    .transient_rgba32()?;
                let mut sampled = crate::retouch_transform::transform_extended(
                    &samples,
                    doc.dimensions(),
                    &edit.matrix,
                    job,
                )?;
                if edit.op == RetouchOp::RetouchHeal {
                    for (i, pixel) in sampled.iter_mut().enumerate() {
                        job.every(i)?;
                        for value in &mut pixel[..3] {
                            *value = encode(*value);
                        }
                    }
                }
                Ok(Self::Sampled(sampled))
            }
            RetouchOp::SpotHeal | RetouchOp::ContentAwareFill => {
                Self::synthesis(doc, edit, coverage, scratch_bytes, job)
            }
            RetouchOp::RedEye => Ok(Self::RedEye),
        }
    }

    /// `RetouchKernel::apply` over f32 (module doc): `pixels` is a `plane.0`
    /// plane of straight linear samples standing at canvas offset `plane.1`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_float(
        &self,
        doc: &PrecisionDocument,
        edit: &RetouchEdit,
        source: Option<&PrecisionImage>,
        coverage: &[u16],
        selection: Option<&[u16]>,
        pixels: &mut [[f32; 4]],
        (dimensions, offset): ((u32, u32), (i32, i32)),
        job: JobContext<'_>,
    ) -> Result<(), String> {
        match self {
            Self::Sampled(sampled) if edit.op == RetouchOp::RetouchClone => {
                // The master's walk: the frozen-alpha rule in place
                // on a transparency-locked layer, the mode's kind, the same
                // effective alpha. The f32 pixel is the backdrop itself, so
                // nothing is stored back through a quantizer.
                let frozen = edit.output == RetouchOutput::Layer
                    && doc.lock_owner(edit.layer, LOCK_TRANSPARENCY).is_some();
                let kind = blend_kind_at(
                    BlendMode::from_c(edit.blend_mode as i32)
                        .ok_or("retouch blend mode is invalid")?,
                    true,
                );
                for (i, p) in pixels.iter_mut().enumerate() {
                    job.every(i)?;
                    let cx = (i % dimensions.0 as usize) as i64 + i64::from(offset.0);
                    let cy = (i / dimensions.0 as usize) as i64 + i64::from(offset.1);
                    let Some(ci) = canvas_index(cx, cy, doc.width, doc.height) else {
                        continue;
                    };
                    if coverage[ci] == 0 || sampled[ci][3] == 0.0 {
                        continue;
                    }
                    let src = sampled[ci];
                    let alpha = if let Some(selection) = selection {
                        (f64::from(src[3]) * f64::from(edit.opacity) * f64::from(coverage[ci])
                            / 65535.
                            * f64::from(selection[ci])
                            / 65535.) as f32
                    } else {
                        src[3] * edit.opacity * f32::from(coverage[ci]) / 65535.
                    };
                    // The float Brush's floor (`float_paint`): ink this faint
                    // would only leave an alpha the renderer refuses.
                    if alpha < EXTENDED_ALPHA_FLOOR {
                        continue;
                    }
                    blend_preserving(p, src, alpha, kind, (cx, cy), frozen);
                }
            }
            Self::Sampled(sampled) => {
                let mut work = encode_plane(pixels)?;
                crate::precision_retouch_heal::apply(
                    doc.dimensions(),
                    sampled,
                    coverage,
                    edit.opacity,
                    &mut work,
                    dimensions,
                    offset,
                    selection,
                    job,
                )?;
                decode_changed(&work, pixels);
            }
            Self::Synthesis(synthesis, eligibility) => {
                // The pad goes on the linear plane BEFORE it is encoded:
                // `encode_plane` copies alpha into `work`, so the kernel
                // sees an opaque hole, and `decode_changed` (RGB only)
                // leaves the padded alpha where it is.
                if edit.fill_transparent {
                    crate::precision_inpaint::opaque_hole::<_, 3>(
                        pixels.as_flattened_mut(),
                        dimensions,
                        offset,
                        doc.dimensions(),
                        |ci| coverage[ci] > 0,
                        1.0_f32,
                    );
                }
                let mut work = encode_plane(pixels)?;
                crate::precision_inpaint::apply::<_, 3>(
                    synthesis,
                    crate::precision_inpaint::SynthesisPlate::Precision(
                        source.ok_or("missing spot source")?,
                    ),
                    coverage,
                    eligibility.as_deref(),
                    edit.opacity,
                    edit.seed,
                    work.as_flattened_mut(),
                    dimensions,
                    offset,
                    selection,
                    job,
                )?;
                decode_changed(&work, pixels);
            }
            Self::RedEye => {
                // The rect's display-encoded copy (`display_encoding`), the
                // shared kernel on it, and only the changed components
                // decoded back; the encoding is recomputed at the write
                // rather than held, so the copy is the one extra plane.
                let encoding = doc.display_encoding()?;
                let mut work: Vec<[f32; 4]> = reserved_vec(pixels.len())?;
                work.extend(pixels.iter().map(|p| encoding.encode(*p)));
                crate::precision_redeye::apply(
                    doc.dimensions(),
                    &mut work,
                    dimensions,
                    offset,
                    coverage,
                    (edit.x, edit.y, edit.width, edit.height),
                    edit.pupil_size,
                    edit.darken,
                    job,
                )?;
                for (i, (pixel, result)) in pixels.iter_mut().zip(&work).enumerate() {
                    job.every(i)?;
                    *pixel = encoding.decode_changed(*pixel, encoding.encode(*pixel), *result);
                }
            }
        }
        Ok(())
    }
}

impl PrecisionDocument {
    /// The f32 arm of `retouching_unlocked`, entered after its shared
    /// prelude: `coverage` is the admitted stroke domain, `selection` the
    /// request's second plane, `scratch_bytes` the plan's (already admitted
    /// and reserved, `job` carrying it as its reservation floor). `plates`
    /// is the document the sampling plate is rendered from. `Ok(None)` when
    /// nothing would change.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn float_retouching(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
        coverage: &[u16],
        selection: Option<&[u16]>,
        scratch_bytes: u64,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let target = self.layer(edit.layer).ok_or("retouch target missing")?;
        let original = target.source().ok_or("retouch target requires pixels")?;
        if original.storage() != PrecisionStorage::Rgba32Float {
            return Err(STORAGE.into());
        }
        let (dimensions, offset) = edit.extent(self)?;
        let in_place = edit.output == RetouchOutput::Layer
            && (dimensions, offset) == (original.dimensions(), target.offset());
        let staged = if in_place {
            None
        } else {
            Some(self.float_retouch_destination(
                edit,
                target,
                (dimensions, offset),
                limit,
                other,
                job,
            )?)
        };
        // Red Eye reads only its target's own pixels: no plate, as the
        // 16-bit master builds none (and its plan charges none).
        let source = if edit.op != RetouchOp::RedEye {
            Some(plates.retouch_source_with_context(
                edit.source_layer.unwrap_or(edit.layer),
                edit.mode()?,
                edit.ignore_adjustments,
                limit,
                other,
                job.range(0.08, 0.2),
            )?)
        } else {
            None
        };
        job.reserve(scratch_bytes)?;
        let edit_job = job.range(0.2, 0.98);
        edit_job.report(0.0, 2)?;
        let src = source.as_ref();
        let kernel =
            RetouchKernel::prepare_float(self, edit, src, coverage, scratch_bytes, edit_job)?;
        let Some((mut pixels, baseline, backdrop)) = staged else {
            let (canvas, offset) = (self.dimensions(), target.offset());
            let rect = kernel
                .bound(edit, coverage, canvas)
                .and_then(|b| canvas_rect_on_plane(canvas, original.dimensions(), offset, b));
            let (mut pixels, plane) = match rect {
                Some((x, y, w, h)) => (
                    original
                        .read_rect_rgba32(x, y, w, h)
                        .ok_or("could not allocate precision output")?,
                    ((w, h), plane_rect_offset(offset, (x, y, w, h))),
                ),
                None => (Vec::new(), ((0, 0), offset)),
            };
            kernel.apply_float(
                self,
                edit,
                src,
                coverage,
                selection,
                &mut pixels,
                plane,
                edit_job,
            )?;
            let written = rect.map(|(x, y, w, h)| original.with_rect_rgba32(x, y, w, h, &pixels));
            let Some(image) = written.transpose()?.flatten() else {
                job.report(1.0, 5)?;
                return Ok(None);
            };
            let result = self.replacing_content(edit.layer, Content::Raster { image, offset })?;
            job.report(1.0, 5)?;
            return Ok(result);
        };
        kernel.apply_float(
            self,
            edit,
            src,
            coverage,
            selection,
            &mut pixels,
            (dimensions, offset),
            edit_job,
        )?;
        let mut changed = false;
        for (i, (a, b)) in pixels.iter().zip(&baseline).enumerate() {
            job.every(i)?;
            changed |= differs(a, b);
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
                if coverage[i] < threshold || !differs(p, &baseline[i]) {
                    *p = [0.0; 4];
                }
            }
        }
        let result = if edit.output == RetouchOutput::NewLayer {
            let image = PrecisionImage::from_rgba32_extended(
                dimensions.0,
                dimensions.1,
                pixels,
                self.profile.clone(),
                None,
            )?;
            Some(self.retouch_insert(edit.layer, image, edit.output_name())?)
        } else {
            let image = original.rebuilt_rgba32_extended(
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

    /// `retouch_destination` over f32: the extent buffer of a new layer or a
    /// grown extent, its baseline copy and the new layer's backdrop plate.
    #[allow(clippy::type_complexity)]
    fn float_retouch_destination(
        &self,
        edit: &RetouchEdit,
        target: &PrecisionDocumentLayer,
        (dimensions, offset): ((u32, u32), (i32, i32)),
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(Vec<[f32; 4]>, Vec<[f32; 4]>, Option<PrecisionImage>), String> {
        let original = target.source().ok_or("retouch target requires pixels")?;
        let mut pixels = reserved_vec(pixel_count(dimensions.0, dimensions.1)?)?;
        pixels.resize(dimensions.0 as usize * dimensions.1 as usize, [0f32; 4]);
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
            if plate.storage() != PrecisionStorage::Rgba32Float {
                return Err(STORAGE.into());
            }
            let (pw, ph) = plate.dimensions();
            debug_assert_eq!((pw, ph), dimensions);
            let stride = dimensions.0 as usize;
            for y in 0..ph {
                for (x, run) in plate.row_segments(y, 0, pw) {
                    let PrecisionRun::Rgba32(run) = run else {
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
                    let PrecisionRun::Rgba32(run) = run else {
                        return Err(STORAGE.into());
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
}
