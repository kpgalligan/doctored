//! The per-op kernels of a native retouch (13B-3d-2), split out of
//! `precision_retouch_run`'s dispatch at the destination write: `prepare` is
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
//! Content-Aware Fill and red-eye.
//!
//! Since 13B-9b a Spot Heal or Content-Aware Fill plans over `RetouchEdit::synthesis_domain` (the
//! request's bounds; the canvas under `full_frame`) and reads its `SynthesisPlate`: with `sample:
//! current` the source layer in place at its offset, otherwise the rendered canvas plate.

use crate::blend::BlendMode;
use crate::heal_writeback::HealSample;
use crate::job_control::JobContext;
use crate::lab_blend::kind_in;
use crate::precision_document::PrecisionDocument;
use crate::precision_inpaint::SynthesisPlate;
use crate::precision_paint::{blend_preserving, canvas_index, store};
use crate::precision_retouch::{RetouchEdit, RetouchOp, RetouchOutput};
use crate::retouch_coverage::CoveragePlane;
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
    /// `precision_retouch_heal::apply` over this plate (module doc), in the
    /// frame's coordinates: `coverage` and `selection` cropped to the frame
    /// (`CoveragePlane::crop`, 0 outside their rect), the frame's size as the
    /// canvas and the plane's offset moved by the frame's origin. An empty
    /// frame covers nothing, so nothing is healed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn heal(
        &self,
        coverage: CoveragePlane<'_>,
        selection: Option<CoveragePlane<'_>>,
        opacity: f32,
        pixels: &mut [[P; 4]],
        (dimensions, offset): ((u32, u32), (i32, i32)),
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let (x0, y0, x1, y1) = self.frame;
        let (Ok(fw), Ok(fh)) = (u32::try_from(x1 - x0), u32::try_from(y1 - y0)) else {
            return Ok(());
        };
        if fw == 0 || fh == 0 {
            return Ok(());
        }
        let coverage = coverage.crop(self.frame)?;
        let selection = selection.map(|s| s.crop(self.frame)).transpose()?;
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

/// Red Eye's coverage, canvas-sized as its kernel reads it: Red Eye takes
/// neither `coverage_rect` nor a selection, so its plane is the canvas.
pub(crate) fn red_eye_coverage<'a>(
    doc: &PrecisionDocument,
    coverage: CoveragePlane<'a>,
) -> Result<&'a [u16], String> {
    if coverage.rect() != (0, 0, doc.width, doc.height) {
        return Err("red eye reads canvas-sized coverage".into());
    }
    Ok(coverage.samples())
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
        coverage: CoveragePlane<'_>,
        scratch_bytes: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let bounds = edit.bounds(doc)?;
        let domain = edit.synthesis_domain(doc)?;
        let caller = if edit.op == RetouchOp::ContentAwareFill {
            crate::doc_inpaint::Caller::Fill
        } else {
            crate::doc_inpaint::Caller::SpotHeal
        };
        // The estimate `plan` reserves within: its hole/walk term over the
        // domain it plans, as `plan`'s own figure counts it.
        let estimate = crate::precision_inpaint::estimate(
            doc.dimensions(),
            bounds,
            u64::from(bounds.2) * u64::from(bounds.3),
            edit.ring,
            caller,
            edit.spot_type,
            3,
            u64::from(domain.2) * u64::from(domain.3),
        )?
        .0;
        let synthesis = crate::precision_inpaint::plan(
            doc.dimensions(),
            domain,
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
        coverage: CoveragePlane<'_>,
        canvas: (u32, u32),
    ) -> Result<(Option<CanvasRect>, CanvasRect, CanvasRect), String> {
        let whole = (0, 0, i64::from(canvas.0), i64::from(canvas.1));
        let grow = i64::from(edit.op == RetouchOp::RetouchHeal);
        let bound = coverage.bbox().map(|(x, y, w, h)| {
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
        coverage: CoveragePlane<'_>,
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
        source: Option<SynthesisPlate<'_>>,
        coverage: CoveragePlane<'_>,
        selection: Option<CoveragePlane<'_>>,
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
                    let c = coverage.at(ci);
                    if c == 0 || sampled[3] == 0 {
                        continue;
                    }
                    let src = sampled.map(|n| f32::from(n) / 65535.);
                    let mut dest = p.map(|n| f32::from(n) / 65535.);
                    blend_preserving(
                        &mut dest,
                        src,
                        if let Some(selection) = selection {
                            (f64::from(src[3]) * f64::from(edit.opacity) * f64::from(c) / 65535.
                                * f64::from(selection.at(ci))
                                / 65535.) as f32
                        } else {
                            src[3] * edit.opacity * f32::from(c) / 65535.
                        },
                        kind,
                        (cx, cy),
                        frozen,
                    );
                    *p = store(dest);
                }
            }
            Self::Sampled(plate) => plate.heal(
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
                        |ci| coverage.at(ci) > 0,
                        u16::MAX,
                    );
                }
                crate::precision_inpaint::apply::<_, 3>(
                    synthesis,
                    source.ok_or("missing spot source")?,
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
                red_eye_coverage(doc, coverage)?,
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
                red_eye_coverage(doc, coverage)?,
                (edit.x, edit.y, edit.width, edit.height),
                edit.pupil_size,
                edit.darken,
                job,
            )?,
        }
        Ok(())
    }
}
