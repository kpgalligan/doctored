//! KERNEL: the clip unit's member rule (#2, "CL2"), Photoshop's clipping —
//! the clipped members composite onto the base with the base's alpha
//! FROZEN. Its master statement is `blend::composite_source_frozen`: a
//! semi-transparent pixel takes `lerp(Cb, B(Cb, Cs), sa)` in its colour and
//! keeps its alpha. This module reproduces that rule through the ORDINARY
//! member kernel, so every member kind (raster, group, adjustment, styled,
//! knockout) keeps its own arithmetic:
//!
//! * `Open`, once after the base renders: the buffer's alpha becomes its
//!   share of the unit's FOOTPRINT `f` (the base's alpha × mask, no Fill or
//!   Blend If weight), `r = min(a / f, 1)` — 1 for a plain base, the Blend If
//!   weight for a weighted one, 0 outside the footprint.
//! * `Settle`, after each member: `r` clamped to `[0, 1]`, 0 outside.
//! * `Close`, after the last member: the alpha becomes `f · r`.
//!
//! A member composited by the W3C step onto `(Cb, 1)` gives exactly the
//! frozen lerp, and `Close` puts the footprint back, so a plain base at alpha
//! `f` shows the member at its own strength instead of the old rule's (the
//! alpha forced back to `f` after an absolute W3C step, which boosted the
//! member by `1 / f`; #2 take1: Normal 11 codes off Photoshop at 8 bits).
//! A Blend If weighted base keeps its weight RELATIVE to the footprint. At
//! `f = 1` the steps are identities on the old rule's numbers (`Open` gives
//! the old alpha, the W3C step at alpha 1 returns 1, `Close` multiplies by
//! 1), so every opaque-base unit renders bit for bit as before.
//!
//! CL3, a base below 100 % Fill: the unit pays the base's Fill ONCE, as a
//! whole. With a visible member the unit is built as at Fill 1 (the base's
//! pixels enter the buffer at Fill 1, the members land on them by the steps
//! above) and the FINISHED unit lands on its backdrop through the base's
//! mode at its alpha × the joint weight of the Fill and the Opacity
//! (`opacity_byte::joint`), so a Fill 0 unit lands nothing. A unit with no
//! visible member keeps the Fill on the base's own pixels, as an unclipped
//! layer does. [`unit_fill`] is the one statement of the rule; every member
//! loop that holds base pixels (byte, typed, float, regional, CMYK, direct
//! and routed) reads it. A recorded take measured it at 8 and 16 bits
//! (`clip_base_fill_2_take.py` take1, in the private archive). An ADJUSTMENT
//! base is outside it: it puts no pixels in the unit, and its Fill weighs
//! the adjustment alone (`interior_into`, `tone_interior`). Routed (a
//! knockout or grouped interiors), its interior frame is the unit's pixels,
//! and its members never open the unit, so unless D6 groups it the frame
//! keeps its Fill.

/// One step of the rule (module doc).
#[derive(Clone, Copy)]
pub(crate) enum ClipStep {
    Open,
    Settle,
    Close,
}

/// `f` = the unit's footprint (base alpha × mask, no Fill or Blend If weight).
/// Open: a → min(a / f, 1), 0 where f ≤ 0 · Settle: r → r.clamp(0, 1), 0
/// where f ≤ 0 · Close: r → f · r.
pub(crate) fn clip_alpha(alpha: f32, footprint: f32, step: ClipStep) -> f32 {
    match step {
        ClipStep::Open if footprint <= 0.0 => 0.0,
        ClipStep::Open => (alpha / footprint).min(1.0),
        ClipStep::Settle if footprint <= 0.0 => 0.0,
        ClipStep::Settle => alpha.clamp(0.0, 1.0),
        ClipStep::Close => footprint.max(0.0) * alpha,
    }
}

/// CL3 (module doc): `(fill_in, fill_out)`, the Fill the base's pixels enter
/// the unit with and the Fill the finished unit lands with. `opened` = the
/// unit has a visible member over a pixel base, never over an adjustment's
/// frame (or, routed, its grouped interiors moved in): `(1, fill)`; else
/// `(fill, 1)`. `fill` is the value the composite already
/// reads (`opacity_byte::composite`), so a direct landing weighs
/// `joint(fill_out, opacity)` and a routed one `q = fill_out · min(1, U.a /
/// s)`.
pub(crate) fn unit_fill(fill: f32, opened: bool) -> (f32, f32) {
    if opened {
        (1.0, fill)
    } else {
        (fill, 1.0)
    }
}

/// [`clip_alpha`] on the last channel of each straight pixel (`[f32; 4]`
/// RGB, `[f32; 5]` ink), `footprint` per pixel in the same order.
pub(crate) fn clip_pass<const N: usize>(buf: &mut [[f32; N]], footprint: &[f32], step: ClipStep) {
    for (px, &f) in buf.iter_mut().zip(footprint) {
        px[N - 1] = clip_alpha(px[N - 1], f, step);
    }
}
