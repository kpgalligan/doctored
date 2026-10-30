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

use crate::blend::BlendMode;
use crate::job_control::JobContext;
use crate::lab_blend::kind_in;
use crate::layer_rect::coverage_rect16;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{PrecisionImage, PrecisionStorage};
use crate::precision_paint::{blend_preserving, canvas_index, store};
use crate::precision_retouch::{RetouchEdit, RetouchOp, RetouchOutput};

/// `P` is the destination's sample: u16 here, f32 in a 32-bit float document
/// (`float_retouch`), whose `Sampled` plate is linear light for a clone
/// and W codes (`retouch_encoding`) for a heal.
pub(crate) enum RetouchKernel<P = u16> {
    /// Clone / Heal: the source plate transformed onto the canvas.
    Sampled(Vec<[P; 4]>),
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

    /// The canvas rect (exclusive ends) holding every destination sample
    /// the write reads or changes: Clone the bbox of `coverage > 0` (it
    /// writes nowhere else and reads only what it writes); Heal that bbox
    /// grown by 1 (each component's padded window — the membrane's reach —
    /// lies inside it); Spot Heal / Content-Aware Fill the union of the
    /// plan's windows; red-eye its rect. `None` when there is none.
    pub(crate) fn bound(
        &self,
        edit: &RetouchEdit,
        coverage: &[u16],
        canvas: (u32, u32),
    ) -> Option<(i64, i64, i64, i64)> {
        match self {
            Self::Sampled(_) => {
                let (x, y, w, h) = coverage_rect16(coverage, canvas, canvas, (0, 0))?;
                let (x0, y0) = (i64::from(x), i64::from(y));
                let (x1, y1) = (x0 + i64::from(w), y0 + i64::from(h));
                if edit.op == RetouchOp::RetouchHeal {
                    Some((x0 - 1, y0 - 1, x1 + 1, y1 + 1))
                } else {
                    Some((x0, y0, x1, y1))
                }
            }
            Self::Synthesis(plan, _) => plan.window_union(),
            Self::RedEye => {
                let (x, y) = (i64::from(edit.x), i64::from(edit.y));
                Some((x, y, x + i64::from(edit.width), y + i64::from(edit.height)))
            }
        }
    }
}

impl RetouchKernel {
    /// HEAD's per-op statements before the destination write, verbatim and
    /// in order (`scratch_bytes` is the retouch plan's).
    pub(crate) fn prepare(
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
                    .filter(|s| s.storage() == PrecisionStorage::Rgba16Unorm)
                    .ok_or("missing native retouch source")?
                    .transient_rgba16()?;
                let sampled = crate::retouch_transform::transform(
                    &samples,
                    doc.dimensions(),
                    &edit.matrix,
                    job,
                )?;
                Ok(Self::Sampled(sampled))
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
            Self::Sampled(sampled) if edit.op == RetouchOp::RetouchClone => {
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
                    if coverage[ci] == 0 || sampled[ci][3] == 0 {
                        continue;
                    }
                    let src = sampled[ci].map(|n| f32::from(n) / 65535.);
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
            Self::Sampled(sampled) => crate::precision_retouch_heal::apply(
                doc.dimensions(),
                sampled,
                coverage,
                edit.opacity,
                pixels,
                dimensions,
                offset,
                selection,
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
            // A Lab layer corrects its ProPhoto RGB colour (#547 Lab 2f).
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
