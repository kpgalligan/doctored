//! The per-op kernels of a native retouch (13B-3d-2), split out of
//! `precision_retouch`'s dispatch at the destination write: `prepare` is
//! everything each op computes before it touches the destination (the
//! transformed source for Clone / Heal, the synthesis plan and the source
//! permission for Spot Heal / Content-Aware Fill), `apply` is the write
//! itself over caller-owned destination storage described by its plane
//! geometry `((w, h), canvas offset)`, and `bound` is the canvas rect the
//! write can read or change (exclusive ends), so the in-place arm reads and
//! writes ONE rect of the layer instead of the whole plane.
//!
//! Every statement is HEAD `a33e203`'s, in HEAD's order: the destination
//! write only ever indexes the plane through canvas coordinates (Clone's
//! `canvas_index` walk, the membrane writeback's `layer_byte`, red-eye's
//! rect `index`), so a plane covering every destination sample the write
//! reads or changes answers the same codes as the whole layer.
//!
//! Since 13B-3d-2 the IN-PLACE arm (a Layer target at its own extent: the app's 16-bit Clone Stamp
//! / Healing Brush / Spot Heal / Content-Aware Fill / red-eye) is a RECT writer:
//! `precision_retouch_apply::RetouchKernel` holds HEAD's per-op `prepare` (the transformed source;
//! the synthesis plan and source permission), destination write (`apply`, over a plane given as
//! `((w, h), canvas offset)`, an empty plane running the solve with nothing written) and `bound`
//! (the canvas rect the write reads or changes: Clone the `coverage > 0` bbox, Heal it grown by 1,
//! Spot Heal / Content-Aware Fill `InpaintPlan::window_union`, red-eye its rect), so the layer's
//! part of the bound is ONE `read_rect_rgba16` written back through `with_rect_rgba16` (the changed
//! latch; ≤ 4 new tiles per dab, no view); a new layer or a grown `expand_destination` extent keeps
//! the whole extent buffer, filled from the layer's rows.
//!
//! Since 13B-9a a Clone or Heal is RECT-FIRST at every destination and both depths: the bound
//! comes first, from the coverage alone ([`RetouchKernel::sampled_frames`]); the source is read
//! as the TAPS of one canvas rect (`retouch_taps16` / `retouch_taps32`, `retouch_transform::
//! tap_rect`) and transformed over the bound only (`retouch_transform::transform_rect`), so the
//! `Sampled` plate carries its frame ([`SampledPlate`]). Clone reads it by canvas position; Heal
//! runs `precision_retouch_heal::apply` in the frame's coordinates (coverage and selection
//! cropped to it, the plane's offset moved by its origin), which finds the same components and
//! windows because every covered pixel and its padded window lie inside the frame. Every kept
//! sample is the whole-canvas transform's to the bit; `RetouchEdit::full_frame` (a test seam) makes
//! the frame and the taps the canvas, the path before 13B-9a. `prepare` keeps Spot Heal,
//! Content-Aware Fill and red-eye, whose plate is still the whole canvas.

use crate::blend::BlendMode;
use crate::heal_writeback::HealSample;
use crate::job_control::JobContext;
use crate::lab_blend::kind_in;
use crate::layer_rect::coverage_rect16;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{reserved_vec, PrecisionImage};
use crate::precision_paint::{blend_preserving, canvas_index, store};
use crate::precision_retouch::{RetouchEdit, RetouchOp, RetouchOutput};
use crate::retouch_transform::CanvasRect;

/// A Clone or Heal's transformed source over its `frame` (13B-9a, module
/// doc): `samples` row-major over the canvas rect `frame`; `bound` is the
/// kernel's [`RetouchKernel::bound`], `None` when the coverage has none.
pub(crate) struct SampledPlate<P> {
    pub samples: Vec<[P; 4]>,
    pub frame: CanvasRect,
    pub bound: Option<CanvasRect>,
}

impl<P: Copy> SampledPlate<P> {
    /// The transformed sample at canvas `(cx, cy)`; `None` outside the
    /// frame.
    pub(crate) fn at(&self, cx: i64, cy: i64) -> Option<[P; 4]> {
        let (x0, y0, x1, y1) = self.frame;
        if cx < x0 || cx >= x1 || cy < y0 || cy >= y1 {
            return None;
        }
        Some(self.samples[(cy - y0) as usize * (x1 - x0) as usize + (cx - x0) as usize])
    }
}

impl<P: HealSample + Into<f32>> SampledPlate<P> {
    /// `precision_retouch_heal::apply` over this plate (module doc): as is
    /// when the frame is the `canvas`, else in the frame's coordinates —
    /// `coverage` and `selection` (canvas-sized) cropped to the frame, the
    /// frame's size as the canvas and the plane's offset moved by the
    /// frame's origin. An empty frame covers nothing, so nothing is healed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn heal(
        &self,
        canvas: (u32, u32),
        coverage: &[u16],
        selection: Option<&[u16]>,
        opacity: f32,
        pixels: &mut [[P; 4]],
        (dimensions, offset): ((u32, u32), (i32, i32)),
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let (x0, y0, x1, y1) = self.frame;
        if self.frame == (0, 0, i64::from(canvas.0), i64::from(canvas.1)) {
            return crate::precision_retouch_heal::apply(
                canvas,
                &self.samples,
                coverage,
                opacity,
                pixels,
                dimensions,
                offset,
                selection,
                job,
            );
        }
        let (Ok(fw), Ok(fh)) = (u32::try_from(x1 - x0), u32::try_from(y1 - y0)) else {
            return Ok(());
        };
        if fw == 0 || fh == 0 {
            return Ok(());
        }
        let stride = canvas.0 as usize;
        let crop = |plane: &[u16]| -> Result<Vec<u16>, String> {
            let mut out = reserved_vec(fw as usize * fh as usize)?;
            for y in y0..y1 {
                let at = y as usize * stride + x0 as usize;
                out.extend_from_slice(&plane[at..at + fw as usize]);
            }
            Ok(out)
        };
        let coverage = crop(coverage)?;
        let selection = selection.map(crop).transpose()?;
        let moved = |o: i32, by: i64| {
            i32::try_from(i64::from(o) - by).map_err(|_| "retouch frame offset overflow")
        };
        crate::precision_retouch_heal::apply(
            (fw, fh),
            &self.samples,
            &coverage,
            opacity,
            pixels,
            dimensions,
            (moved(offset.0, x0)?, moved(offset.1, y0)?),
            selection.as_deref(),
            job,
        )
    }
}

/// `P` is the destination's sample: u16 here, f32 in a 32-bit float document
/// (`float_retouch`), whose `Sampled` plate is linear light for a clone
/// and W codes (`retouch_encoding`) for a heal.
pub(crate) enum RetouchKernel<P = u16> {
    /// Clone / Heal: the source transformed over the kernel's frame.
    Sampled(SampledPlate<P>),
    /// Spot Heal / Content-Aware Fill: the exact synthesis plan and the
    /// compiled source permission.
    Synthesis(crate::precision_inpaint::InpaintPlan, Option<Vec<bool>>),
    RedEye,
}

impl<P> RetouchKernel<P> {
    /// Spot Heal / Content-Aware Fill's statements before the destination
    /// write (`scratch_bytes` is the retouch plan's): the same plan and
    /// source permission at every depth, since both read u16 coverage only.
    pub(crate) fn synthesis(
        doc: &PrecisionDocument,
        edit: &RetouchEdit,
        coverage: &[u16],
        scratch_bytes: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let bounds = edit.bounds(doc)?;
        let caller = if edit.op == RetouchOp::ContentAwareFill {
            crate::doc_inpaint::Caller::Fill
        } else {
            crate::doc_inpaint::Caller::SpotHeal
        };
        let estimate = crate::precision_inpaint::estimate(
            doc.dimensions(),
            bounds,
            u64::from(bounds.2) * u64::from(bounds.3),
            edit.ring,
            caller,
            edit.spot_type,
            3,
        )?
        .0;
        let synthesis = crate::precision_inpaint::plan(
            doc.dimensions(),
            coverage,
            edit.ring,
            edit.op == RetouchOp::ContentAwareFill,
            edit.spot_type,
            scratch_bytes - estimate,
            3,
            job,
        )?;
        let eligibility = edit
            .source_mask
            .as_ref()
            .map(|m| m.compile(doc.width, doc.height, job))
            .transpose()?;
        Ok(Self::Synthesis(synthesis, eligibility))
    }

    /// A Clone or Heal's `(bound, frame, taps)` before any source is read
    /// (module doc). `bound` is the canvas rect holding every destination
    /// sample the write reads or changes — the bbox of `coverage > 0` for a
    /// Clone (it writes nowhere else and reads only what it writes), that
    /// bbox grown by 1 for a Heal (each component's padded window, the
    /// membrane's reach, lies inside it) — clamped to the canvas, the ONE
    /// rule [`Self::bound`] answers (`canvas_rect_on_plane` clamps the
    /// written rect again, to the same rect); `None` with no coverage.
    /// `frame` is the bound (the empty rect for `None`), or the canvas when
    /// `edit.full_frame`; `taps` is `retouch_transform::tap_rect` of the
    /// frame, or the canvas when `edit.full_frame`.
    pub(crate) fn sampled_frames(
        edit: &RetouchEdit,
        coverage: &[u16],
        canvas: (u32, u32),
    ) -> Result<(Option<CanvasRect>, CanvasRect, CanvasRect), String> {
        let whole = (0, 0, i64::from(canvas.0), i64::from(canvas.1));
        let grow = i64::from(edit.op == RetouchOp::RetouchHeal);
        let bound = coverage_rect16(coverage, canvas, canvas, (0, 0)).map(|(x, y, w, h)| {
            let (x0, y0) = (i64::from(x), i64::from(y));
            (
                (x0 - grow).max(0),
                (y0 - grow).max(0),
                (x0 + i64::from(w) + grow).min(whole.2),
                (y0 + i64::from(h) + grow).min(whole.3),
            )
        });
        if edit.full_frame {
            return Ok((bound, whole, whole));
        }
        let frame = bound.unwrap_or((0, 0, 0, 0));
        let taps = crate::retouch_transform::tap_rect(canvas, &edit.matrix, frame)?;
        Ok((bound, frame, taps))
    }

    /// The canvas rect (exclusive ends) holding every destination sample
    /// the write reads or changes: Clone / Heal the plate's stored bound
    /// ([`Self::sampled_frames`]); Spot Heal / Content-Aware Fill the union
    /// of the plan's windows; red-eye its rect. `None` when there is none.
    pub(crate) fn bound(&self, edit: &RetouchEdit) -> Option<CanvasRect> {
        match self {
            Self::Sampled(plate) => plate.bound,
            Self::Synthesis(plan, _) => plan.window_union(),
            Self::RedEye => {
                let (x, y) = (i64::from(edit.x), i64::from(edit.y));
                Some((x, y, x + i64::from(edit.width), y + i64::from(edit.height)))
            }
        }
    }
}

impl RetouchKernel {
    /// A Clone or Heal's kernel (module doc): `taps` (the source over
    /// `frames.2`) transformed over `frames.1`, with `frames.0` its bound.
    pub(crate) fn sampled(
        doc: &PrecisionDocument,
        edit: &RetouchEdit,
        taps: &[[u16; 4]],
        (bound, frame, taps_at): (Option<CanvasRect>, CanvasRect, CanvasRect),
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let samples = crate::retouch_transform::transform_rect(
            taps,
            taps_at,
            doc.dimensions(),
            &edit.matrix,
            frame,
            job,
        )?;
        Ok(Self::Sampled(SampledPlate {
            samples,
            frame,
            bound,
        }))
    }

    /// HEAD's per-op statements before the destination write, verbatim and
    /// in order (`scratch_bytes` is the retouch plan's), for Spot Heal,
    /// Content-Aware Fill and red-eye; a Clone or Heal is built by
    /// [`Self::sampled`].
    pub(crate) fn prepare(
        doc: &PrecisionDocument,
        edit: &RetouchEdit,
        coverage: &[u16],
        scratch_bytes: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        match edit.op {
            // The dispatch builds Clone and Heal through `sampled`, so this
            // arm only guards a wrong caller.
            RetouchOp::RetouchClone | RetouchOp::RetouchHeal => {
                Err("missing native retouch source".into())
            }
            RetouchOp::SpotHeal | RetouchOp::ContentAwareFill => {
                Self::synthesis(doc, edit, coverage, scratch_bytes, job)
            }
            RetouchOp::RedEye => Ok(Self::RedEye),
        }
    }

    /// HEAD's destination write over `pixels`, a `plane.0` plane standing at
    /// canvas offset `plane.1` (the whole destination, a rect of it, or an
    /// empty plane — the solve and its refusals run, nothing is written).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        edit: &RetouchEdit,
        source: Option<&PrecisionImage>,
        coverage: &[u16],
        selection: Option<&[u16]>,
        pixels: &mut [[u16; 4]],
        (dimensions, offset): ((u32, u32), (i32, i32)),
        job: JobContext<'_>,
    ) -> Result<(), String> {
        match self {
            Self::Sampled(plate) if edit.op == RetouchOp::RetouchClone => {
                // In place on a transparency-locked layer: the frozen-alpha
                // rule. New-layer output writes a fresh layer, never
                // the locked one.
                let frozen = edit.output == RetouchOutput::Layer
                    && doc
                        .lock_owner(edit.layer, crate::doc_lock::LOCK_TRANSPARENCY)
                        .is_some();
                // `retouch_plan` admitted the mode (any `BlendMode` that is not
                // group-only); Normal is the source-over this arm always wrote.
                let kind = kind_in(
                    BlendMode::from_c(edit.blend_mode as i32)
                        .ok_or("retouch blend mode is invalid")?,
                    false,
                    doc.lab_grid(),
                );
                for (i, p) in pixels.iter_mut().enumerate() {
                    job.every(i)?;
                    let cx = (i % dimensions.0 as usize) as i64 + i64::from(offset.0);
                    let cy = (i / dimensions.0 as usize) as i64 + i64::from(offset.1);
                    let Some(ci) = canvas_index(cx, cy, doc.width, doc.height) else {
                        continue;
                    };
                    // Outside the frame the coverage is 0 (the frame holds
                    // its bbox), so nothing there is written either way.
                    let Some(sampled) = plate.at(cx, cy) else {
                        continue;
                    };
                    if coverage[ci] == 0 || sampled[3] == 0 {
                        continue;
                    }
                    let src = sampled.map(|n| f32::from(n) / 65535.);
                    let mut dest = p.map(|n| f32::from(n) / 65535.);
                    blend_preserving(
                        &mut dest,
                        src,
                        if let Some(selection) = selection {
                            (f64::from(src[3]) * f64::from(edit.opacity) * f64::from(coverage[ci])
                                / 65535.
                                * f64::from(selection[ci])
                                / 65535.) as f32
                        } else {
                            src[3] * edit.opacity * f32::from(coverage[ci]) / 65535.
                        },
                        kind,
                        (cx, cy),
                        frozen,
                    );
                    *p = store(dest);
                }
            }
            Self::Sampled(plate) => plate.heal(
                doc.dimensions(),
                coverage,
                selection,
                edit.opacity,
                pixels,
                (dimensions, offset),
                job,
            )?,
            Self::Synthesis(synthesis, eligibility) => {
                if edit.fill_transparent {
                    crate::precision_inpaint::opaque_hole::<_, 3>(
                        pixels.as_flattened_mut(),
                        dimensions,
                        offset,
                        doc.dimensions(),
                        |ci| coverage[ci] > 0,
                        u16::MAX,
                    );
                }
                crate::precision_inpaint::apply::<_, 3>(
                    synthesis,
                    crate::precision_inpaint::SynthesisPlate::Precision(
                        source.ok_or("missing spot source")?,
                    ),
                    coverage,
                    eligibility.as_deref(),
                    edit.opacity,
                    edit.seed,
                    pixels.as_flattened_mut(),
                    dimensions,
                    offset,
                    selection,
                    job,
                )?;
            }
            // A Lab layer corrects its ProPhoto RGB colour (#1 Lab 2f).
            Self::RedEye if doc.is_lab() => crate::lab_retouch::red_eye(
                doc.dimensions(),
                pixels,
                dimensions,
                offset,
                coverage,
                (edit.x, edit.y, edit.width, edit.height),
                edit.pupil_size,
                edit.darken,
                job,
            )?,
            Self::RedEye => crate::precision_redeye::apply(
                doc.dimensions(),
                pixels,
                dimensions,
                offset,
                coverage,
                (edit.x, edit.y, edit.width, edit.height),
                edit.pupil_size,
                edit.darken,
                job,
            )?,
        }
        Ok(())
    }
}
