//! Image ▸ Adjustments on one Red, Green, Blue or Alpha component of an
//! `Rgba32Float` layer, through `precision_scalar_edit`'s `scalar_edit` request
//! (an `adjustment` filter edit on target red, green, blue or alpha). An
//! adapter: the per-pixel forms are the layer's and the mask's own, and the
//! read, gate, write and rebuild are `float_plane_filter`'s plane rule
//! (`PrecisionDocument::float_plane_unlocked`), so nothing here is pixel math.
//!
//! # The rule
//! - **Red, Green and Blue are light.** An adjustment is its layer's float
//!   form, `PrecisionAdjustment::apply_linear`, on the gray triple `[v, v, v]`
//!   at the pixel's image position, then the plane rule's Result
//!   (`float_plane_filter::result_of`), unclamped. Invert, Shadows/Highlights
//!   and the legacy White Balance refuse with the layer's own sentence, and an
//!   op with no `<op>_apply` row with the apply's
//!   (`float_adjust_apply::apply_refusal`): a colour plane holds values above
//!   1.0, which is what those rows' reasons are about.
//! - **Alpha is coverage.** An adjustment runs through the interpreter a mask
//!   uses (`PrecisionAdjustment::parse_unbound`, its guide, `apply_at`) on
//!   `[a, a, a]` with the stored f32 alpha and no quantization, then the
//!   Result, clamped to 0…1, so every adjustment a mask takes runs and Invert
//!   is `1 − a`.
//! - **Read and write** are the plane rule's: the plane is read at stored
//!   alpha 0 too; weight 0 keeps the bits, 65535 takes the result, between is
//!   the f64 lerp rounded once; an unchanged plane, and an identity
//!   description, are `None`. A non-finite result refuses with
//!   `float_adjust_apply`'s sentence, and a colour result outside ±65504
//!   through `rebuilt_rgba32_extended`.
//! - **Auto refuses by name** on all four, and so does `levels_channels`, the
//!   write Auto's derived values are applied with. On Red, Green and Blue the
//!   sentence is the layer's (the operator table's Auto row: a value above 1.0
//!   lies past every end of the histogram Auto reads). On Alpha Auto reads a
//!   16-bit histogram, which a float alpha has no form of without quantizing.
//!
//! # Admission
//! The term replaces `ScalarEdit::plan`'s `n·32` u16 plane, as the float plane
//! filter's does: `n·APPLY_BYTES_PER_PIXEL` (the pixel copy beside the rebuilt
//! tiles, the walk `float_plane_filter`'s table measured; the stage before it
//! holds the read and the result, two f32 planes, 8 bytes a pixel), plus the
//! adjustment's preparation, twice the description, and on Alpha the guide's
//! plan. The walk is the measured one, so no new measurement is taken.
use crate::adjust_precision::PrecisionAdjustment;
use crate::float_adjust_apply::{apply_refusal, APPLY_BYTES_PER_PIXEL, NONFINITE};
use crate::float_plane_filter::result_of;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::reserved_vec;
use crate::selected_adjustment::SelectedEdit;

/// The Alpha component.
const ALPHA: usize = 3;

/// The sentence Auto (and its `levels_channels` write) carries on
/// `component` (module doc, `# The rule`).
pub(crate) fn auto_refusal(component: usize) -> String {
    if component == ALPHA {
        return "Auto Tone, Auto Contrast and Auto Color are unavailable on a 32-bit float \
                layer's Alpha: Auto reads a 16-bit histogram, which a float alpha has no form \
                of without quantizing; run it on a layer mask or an alpha channel"
            .into();
    }
    crate::float_document::auto_refusal()
}

/// The request's adjustment description; Auto and `levels_channels` refuse
/// by name first.
fn description(edit: &SelectedEdit, component: usize) -> Result<&str, String> {
    if edit.is_auto() || edit.op == "levels_channels" {
        return Err(auto_refusal(component));
    }
    edit.description
        .as_deref()
        .filter(|_| edit.op == "adjustment")
        .ok_or_else(|| "scalar adjustment needs a description".into())
}

/// The adjustment, parsed by the rule's side, refused by name where the rule
/// refuses it.
fn tone(
    description: &str,
    component: usize,
    doc: &PrecisionDocument,
) -> Result<PrecisionAdjustment, String> {
    if component == ALPHA {
        return PrecisionAdjustment::parse_unbound(description, doc.profile());
    }
    let tone = PrecisionAdjustment::parse(description, doc.profile())?;
    match apply_refusal(&tone) {
        Some(reason) => Err(reason),
        None => Ok(tone),
    }
}

/// `ScalarEdit::plan`'s term for `edit` on `component` of a float layer of
/// `dimensions` (module doc, `# Admission`); refuses as the run refuses.
pub(crate) fn plan(
    edit: &SelectedEdit,
    dimensions: (u32, u32),
    component: usize,
    doc: &PrecisionDocument,
) -> Result<u64, String> {
    let description = description(edit, component)?;
    tone(description, component, doc)?;
    let (w, h) = dimensions;
    let guide = if component == ALPHA {
        PrecisionAdjustment::guide_plan_for_description(description, w, h)?.0
    } else {
        0
    };
    let preparation = PrecisionAdjustment::prepare_scratch_bytes(description, doc.profile())?;
    (u64::from(w) * u64::from(h))
        .checked_mul(APPLY_BYTES_PER_PIXEL)
        .and_then(|n| n.checked_add(preparation))
        .and_then(|n| n.checked_add(description.len() as u64 * 2))
        .and_then(|n| n.checked_add(guide))
        .ok_or_else(|| "adjustment scratch overflow".into())
}

/// `edit` on `component` (0..=3) of float layer `layer` under `selection`
/// (canvas coverage; empty is the whole plane). The caller has planned,
/// admitted and reserved.
pub(crate) fn apply(
    doc: &PrecisionDocument,
    layer: usize,
    component: usize,
    edit: &SelectedEdit,
    selection: &[u16],
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    let tone = tone(description(edit, component)?, component, doc)?;
    job.check()?;
    if tone.is_identity() {
        job.report(1., 5)?;
        return Ok(None);
    }
    doc.float_plane_unlocked(
        layer,
        component,
        selection,
        job,
        |values, (w, h), weight_at| {
            let guide = if component == ALPHA {
                tone.guide(
                    w,
                    h,
                    &|i| {
                        let a = values[i];
                        [a, a, a, 1.0]
                    },
                    job.range(0., 0.3),
                )?
            } else {
                None
            };
            let mut out = reserved_vec(values.len())?;
            for (i, &v) in values.iter().enumerate() {
                job.every(i)?;
                if weight_at(i) == 0 {
                    out.push(v);
                    continue;
                }
                // The IMAGE position, the destructive convention.
                let (x, y) = ((i % w as usize) as u32, (i / w as usize) as u32);
                let xy = (i64::from(x), i64::from(y));
                let rgb = if component == ALPHA {
                    tone.apply_at([v, v, v], guide.as_ref().map_or(0., |g| g.at(x, y)), xy)
                } else {
                    tone.apply_linear([v, v, v], xy)?
                };
                let result = result_of([rgb[0], rgb[1], rgb[2], 1.0]);
                if rgb.iter().any(|c| !c.is_finite()) || !result.is_finite() {
                    return Err(NONFINITE.into());
                }
                out.push(if component == ALPHA {
                    result.clamp(0.0, 1.0)
                } else {
                    result
                });
            }
            job.check()?;
            Ok(Some(out))
        },
    )
}
