//! Edit ▸ Fade's per-pixel blend (v1): the last step's
//! result (`after`) blended back with the state before it (`before`) at an
//! opacity and in a blend mode. The kernel names no document type; the byte
//! adapter is `doc_fade`, the 16-bit and float arms `precision_fade`.
//!
//! The blend math is `blend`'s and nothing else: the mode is composited with
//! [`composite_source_into`] (the projection's own W3C step, Dissolve's dither
//! included) and both interpolations are [`gate_source_into`]'s premultiplied
//! lerp. Per pixel:
//!
//! 1. Where `before` and `after` are both transparent the pixel is `after`
//!    verbatim: there is nothing to blend, and the compositor's `0 / 0` would
//!    put NaN into the result.
//! 2. `E` is the step's result seen through the mode: `after` for Normal;
//!    for any other mode `before` where `after` is transparent
//!    (`composite_source_into` needs a source alpha above 0, so its callers
//!    skip such a source, as here), else `after` (its colour, its alpha)
//!    composited over `before` in the mode.
//! 3. `E′` is `after` moved toward `E` by `weight` — the selection's coverage
//!    for a region step, 1 where a stroke changed the pixel and 0 elsewhere —
//!    so the mode applies only where the step did.
//! 4. The result is `before` moved toward `E′` by `opacity`. Normal at
//!    opacity 1 is therefore `after` exactly, opacity 0 is `before` exactly,
//!    and weight 0 leaves the plain opacity lerp.
//!
//! Under a transparency lock the result keeps `after`'s alpha.
use crate::blend::{composite_source_into, gate_source_into, BlendKind, BlendMode};
use crate::blend_float::blend_kind_at;

/// How the mode turns `after` into the faded target: Normal replaces, every
/// other mode composites through its kind.
#[derive(Clone, Copy)]
pub(crate) enum FadeBlend {
    Replace,
    Mode(BlendKind),
}

/// The fade blend for `mode` at the document's depth (`blend_kind_at`'s
/// float variants when `float`). `None` for Pass Through, which is a group's
/// declaration and blends nothing. Whether a float document admits the mode
/// is the adapter's question (`float_document::admitted_blend`).
pub(crate) fn fade_blend(mode: BlendMode, float: bool) -> Option<FadeBlend> {
    match mode {
        BlendMode::PassThrough => None,
        BlendMode::Normal => Some(FadeBlend::Replace),
        _ => Some(FadeBlend::Mode(blend_kind_at(mode, float))),
    }
}

/// One faded pixel, straight RGBA in unit range (colour may exceed it at
/// float). `weight` and `opacity` are in [0, 1]; `canvas_xy` keeps
/// Dissolve's dither canvas-absolute. The module doc states the rule.
pub(crate) fn fade_pixel(
    before: [f32; 4],
    after: [f32; 4],
    blend: &FadeBlend,
    weight: f32,
    opacity: f32,
    frozen_alpha: bool,
    canvas_xy: (i64, i64),
) -> [f32; 4] {
    if before[3] <= 0.0 && after[3] <= 0.0 {
        return after;
    }
    // An unchanged pixel the step did not touch stays bit for bit: the lerp
    // of a value with itself can round one ulp away at f32.
    if weight <= 0.0 && before == after {
        return after;
    }
    let target = match blend {
        FadeBlend::Replace => after,
        // A transparent source composites to the backdrop unchanged.
        FadeBlend::Mode(_) if after[3] <= 0.0 => before,
        FadeBlend::Mode(kind) => {
            let mut acc = [before];
            composite_source_into(
                &mut acc,
                0,
                [after[0], after[1], after[2]],
                after[3],
                *kind,
                canvas_xy,
            );
            acc[0]
        }
    };
    let mut weighted = after;
    gate_source_into(&mut weighted, target, weight);
    let mut out = before;
    gate_source_into(&mut out, weighted, opacity);
    if frozen_alpha {
        out[3] = after[3];
    }
    out
}
