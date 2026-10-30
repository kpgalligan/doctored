//! The blend functions a 32-bit FLOAT document composites with where Photoshop's
//! 32-bit formula differs from the `blend.rs` master, and [`blend_kind_at`], the
//! depth-aware selector the float render and Fill…/Stroke… paths ask instead of
//! `blend::blend_kind`.
//!
//! Every formula here was MEASURED, not chosen: `scripts/photoshop-probe/float_blend.py`
//! composited float sources over float backdrops (values 0, above 1, negative,
//! and tiny sources) in Photoshop 26.10.0 and fitted each mode's candidates at
//! 1e-4 relative (receipt `doctored-fixtures/photoshop/float-blend-473/take3`).
//! The verdicts:
//!
//! * Linear Dodge (Add) and Subtract are UNCLAMPED: `cb + cs` and `cb − cs`.
//!   The masters clamp at 1 and at 0, which a float scene's highlights and
//!   negatives would cut.
//! * Hue, Saturation, Color and Luminosity use the master's W3C luminosity
//!   weights (0.3, 0.59, 0.11) in the document's own linear primaries — the
//!   Rec. 709 and profile-Y-row candidates both missed, on sRGB and on ProPhoto
//!   alike — and NO ClipColor: SetLum is the plain shift. The master's
//!   ClipColor pulls every triple into [0, 1], which is not Photoshop at 32
//!   bits.
//! * Darker Color, Lighter Color and Dissolve match their masters exactly
//!   (W3C weights, a tie keeps the backdrop; the canvas-absolute dither), so
//!   they get no variant: [`blend_kind_at`] hands back `blend_kind` for them.
//! * Linear Burn `cb + cs − 1` and Linear Light `cb + 2·cs − 1` are
//!   UNCLAMPED (decision B, not measured): the masters clamp to 0…1,
//!   which would cut a highlight and a negative. Pin Light keeps its master
//!   (`min` / `max` against `2·cs` and `2·cs − 1`, no clamp of its own).
//! * Divide is `clamp(cb / max(cs, 1/65504), −65504, 65504)` (decision
//!   B): the master's `min(·, 1)` would cut a highlight, and the floor
//!   on the divisor keeps a black or negative source from dividing by zero or
//!   flipping the sign, so a black source writes `cb·65504` held to the
//!   stored range.
//!
//! The nine modes whose formula inverts about white (`1 − v`) or divides by
//! `1 − v` (Screen, Overlay, Soft Light, Hard Light, Color Dodge, Color Burn,
//! Vivid Light, Hard Mix, Exclusion) have no meaning above 1.0 and are
//! refused (`float_document::admitted_blend`).
//!
//! A LAYER EFFECT's colour composites through [`effect_blend_kind_at`], which
//! is [`blend_kind_at`] except for two measured effect variants
//! (`scripts/photoshop-probe/float_styles.py`, receipt
//! `doctored-fixtures/photoshop/float-styles-473/take6`): a 32-bit effect in
//! Darken keeps the backdrop wherever the effect's component is 1 — 4.0 under
//! a white component reads 4.0, not 1 — and one in Lighten wherever it is 0
//! (−0.25 under a black component reads −0.25). Components of 254/255 and
//! 1/255 take the masters' `min` / `max`, and a LAYER in Darken or Lighten
//! takes the masters everywhere (`layer_modes`, take 5: a source of 1.0 over
//! 8.0 reads 1.0), so the variants are the effect's alone.
//!
//! Every variant is finite for every finite input with `|v| ≤ 65504`: the sums
//! and differences are, `lum` is a weighted sum, and the one division (inside
//! the master's `set_sat`) divides a span by a larger-or-equal span. A result
//! outside the extended range is refused downstream
//! (`precision_image::extended_pixel`); nothing here repairs one.

use crate::blend::{blend_kind, lum, sat, set_sat, BlendKind, BlendMode};

/// The blend behaviour for `mode` at the scene's depth. `float == false` is
/// `blend::blend_kind(mode)` untouched, so every byte and u16 caller stays
/// bit-identical; `float == true` swaps in the measured float variant for a
/// mode whose Photoshop verdict differs from its master and otherwise answers
/// the master too. Whether the mode is ADMITTED at float is
/// `float_document::admitted_blend`'s question, not this one's.
pub(crate) fn blend_kind_at(mode: BlendMode, float: bool) -> BlendKind {
    if !float {
        return blend_kind(mode);
    }
    match mode {
        BlendMode::Addition => BlendKind::Separable(b_addition_float),
        BlendMode::Subtract => BlendKind::Separable(b_subtract_float),
        BlendMode::LinearBurn => BlendKind::Separable(b_linear_burn_float),
        BlendMode::LinearLight => BlendKind::Separable(b_linear_light_float),
        BlendMode::Divide => BlendKind::Separable(b_divide_float),
        BlendMode::Hue => BlendKind::NonSeparable(b_hue_float),
        BlendMode::Saturation => BlendKind::NonSeparable(b_saturation_float),
        BlendMode::Color => BlendKind::NonSeparable(b_color_float),
        BlendMode::Luminosity => BlendKind::NonSeparable(b_luminosity_float),
        _ => blend_kind(mode),
    }
}

/// The blend behaviour of a LAYER EFFECT's colour at the scene's depth (module
/// doc): `float == false` is `blend::blend_kind(mode)` untouched; at float the
/// two measured effect variants, else [`blend_kind_at`]. Whether the mode is
/// ADMITTED on an effect is `float_document::style_refusal`'s question.
pub(crate) fn effect_blend_kind_at(mode: BlendMode, float: bool) -> BlendKind {
    if !float {
        return blend_kind(mode);
    }
    match mode {
        BlendMode::Darken => BlendKind::Separable(b_darken_effect_float),
        BlendMode::Lighten => BlendKind::Separable(b_lighten_effect_float),
        _ => blend_kind_at(mode, true),
    }
}

// Master: `blend::b_darken` (`min(cb, cs)`). A 32-bit effect component of 1
// leaves the backdrop as it is, above 1 included (take 6); every smaller
// component takes the master's minimum.
fn b_darken_effect_float(cb: f32, cs: f32) -> f32 {
    if cs >= 1.0 {
        cb
    } else {
        cb.min(cs)
    }
}

// Master: `blend::b_lighten` (`max(cb, cs)`). A 32-bit effect component of 0
// leaves the backdrop as it is, below 0 included (take 6); every larger
// component takes the master's maximum.
fn b_lighten_effect_float(cb: f32, cs: f32) -> f32 {
    if cs <= 0.0 {
        cb
    } else {
        cb.max(cs)
    }
}

// Master: `blend::b_addition` (`min(cb + cs, 1)`). Photoshop 32-bit Linear
// Dodge is the unclamped sum: 8 + 8 reads 16 (take 3, every opacity pair).
fn b_addition_float(cb: f32, cs: f32) -> f32 {
    cb + cs
}

// Master: `blend::b_subtract` (`max(cb − cs, 0)`). Photoshop 32-bit Subtract
// is the unclamped difference: 0.18 − 8 reads −7.82 (take 3).
fn b_subtract_float(cb: f32, cs: f32) -> f32 {
    cb - cs
}

// Master: `blend::b_linear_burn` (`max(cb + cs − 1, 0)`). Unclamped at 32
// bits (decision B): 4.0 under 0.5 reads 3.5, −0.25 under 0.5 −0.75.
fn b_linear_burn_float(cb: f32, cs: f32) -> f32 {
    cb + cs - 1.0
}

// Master: `blend::b_linear_light` (`clamp(cb + 2·cs − 1, 0, 1)`). Unclamped
// at 32 bits (decision B).
fn b_linear_light_float(cb: f32, cs: f32) -> f32 {
    cb + 2.0 * cs - 1.0
}

// The smallest divisor Divide takes at 32 bits: the reciprocal of the stored
// bound, so a source at or below it multiplies the backdrop by at most 65504.
const DIVIDE_FLOOR: f32 = 1.0 / 65504.0;

// Master: `blend::b_divide` (`min(cb / cs, 1)`, 1 for a black source but 0 at
// 0 / 0). At 32
// bits (decision B) `clamp(cb / max(cs, 1/65504), −65504, 65504)`: no
// cap at white, a divisor floored above zero and the result held to the
// stored range.
fn b_divide_float(cb: f32, cs: f32) -> f32 {
    (cb / cs.max(DIVIDE_FLOOR)).clamp(-65504.0, 65504.0)
}

// Master: `blend::set_lum` (shift to luminosity `l`, then ClipColor). At 32
// bits Photoshop keeps the shift and drops the clip: a Luminosity of 8 over
// (0.5, 0.2, 0.1) reads (8.221, 7.921, 7.821), not a triple pulled into [0, 1].
fn set_lum_float(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    [c[0] + d, c[1] + d, c[2] + d]
}

// Master: `blend::b_hue`, with `set_lum_float` for `set_lum`.
fn b_hue_float(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    set_lum_float(set_sat(cs, sat(cb)), lum(cb))
}

// Master: `blend::b_saturation`, with `set_lum_float` for `set_lum`.
fn b_saturation_float(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    set_lum_float(set_sat(cb, sat(cs)), lum(cb))
}

// Master: `blend::b_color`, with `set_lum_float` for `set_lum`.
fn b_color_float(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    set_lum_float(cs, lum(cb))
}

// Master: `blend::b_luminosity`, with `set_lum_float` for `set_lum`.
fn b_luminosity_float(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    set_lum_float(cb, lum(cs))
}
