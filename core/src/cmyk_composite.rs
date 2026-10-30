//! KERNEL: the CMYK document's compositing (#1 phases 1 and 2) — Normal
//! source-over of straight CMYKA at an effective alpha (a layer's opacity ×
//! its mask, or a brush's ink alpha × opacity × coverage), the same over with
//! the backdrop's alpha frozen, an adjustment record's mix, and the flatten
//! of a composite onto paper. Free functions over plain arrays; the
//! document adapters are `cmyk_stack` (the stack), `cmyk_paint` (the tools)
//! and `cmyk_render` (the projections).
//!
//! The master formula is `blend.rs`'s `composite_source_into` with the
//! Normal kind, where `B(Cb, Cs) = Cs` reduces it to
//! `Co = (αs·Cs + (1 − αs)·αb·Cb) / αo`, `αo = αs + αb·(1 − αs)`, applied
//! here to the four INK components in place of three colour ones. Ink is
//! additive coverage of the same paper, so it composites exactly as a
//! colour component does (Photoshop's CMYK Normal is this same W3C
//! source-over on the ink numbers).
//!
//! Paper is ZERO INK. Flattening a straight composite onto it multiplies
//! each ink by the composite's alpha — `C·α + 0·(1 − α)` — and makes alpha
//! 1; a fully transparent pixel flattens to bare paper.
//!
//! # Blend modes on ink (phase 4a)
//!
//! A mode other than Normal is evaluated on the STORED domain `L = 1 − ink`
//! (`ink_blended`), where `blend.rs`'s functions are defined, so Multiply
//! darkens a print as it darkens a screen: a SEPARABLE mode runs `B` per
//! plate on all four plates, `ink = 1 − B(1 − cb, 1 − cs)`; Hue, Saturation,
//! Color and Luminosity run `blend.rs`'s non-separable function on the CMY
//! `L` triple, with K the backdrop's (Hue, Saturation, Color) or the
//! source's (Luminosity); Darker and Lighter Color pick one WHOLE sample,
//! all four plates, by `lum(L_CMY) · L_K` (the backdrop on a tie). The W3C
//! step is linear in each plane, so `blend::composite_blended` runs on the
//! inks with that blended ink (`over_mode`). Dissolve shows the source
//! opaque where `blend::dissolve_threshold` falls below its alpha. The
//! Photoshop take that fixes these choices is
//! `scripts/photoshop-probe/cmyk_blend_take.py`.

use crate::blend::{blend_kind, composite_blended, dissolve_threshold, gate_planes, lum};
use crate::blend::{BlendKind, BlendMode};

/// One straight CMYKA sample in [0, 1]: C, M, Y, K, A.
pub(crate) type InkPixel = [f32; 5];

/// A u16 component in [0, 1].
pub(crate) fn unit(value: u16) -> f32 {
    f32::from(value) / 65535.0
}

/// A [0, 1] component as u16, `round(v · 65535)` after the clamp.
pub(crate) fn code(value: f32) -> u16 {
    (value.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// `run` (straight CMYKA16) over `acc[at..]` at `opacity`, Normal
/// (module doc). A source sample with no effective alpha leaves the
/// accumulator untouched.
pub(crate) fn over_row(acc: &mut [InkPixel], at: usize, run: &[[u16; 5]], opacity: f32) {
    for (dst, src) in acc[at..at + run.len()].iter_mut().zip(run) {
        let sa = unit(src[4]) * opacity;
        over(
            dst,
            [unit(src[0]), unit(src[1]), unit(src[2]), unit(src[3])],
            sa,
        );
    }
}

/// Straight ink `src` over the straight sample `dst` at effective alpha
/// `sa`, Normal: the module doc's reduction of `blend::composite_source_into`
/// (its master) on the four inks. `sa <= 0` leaves `dst` untouched.
pub(crate) fn over(dst: &mut InkPixel, src: [f32; 4], sa: f32) {
    if sa <= 0.0 {
        return;
    }
    let ab = dst[4];
    let ao = sa + ab * (1.0 - sa);
    for c in 0..4 {
        dst[c] = (sa * src[c] + (1.0 - sa) * ab * dst[c]) / ao;
    }
    dst[4] = ao;
}

/// An adjustment record's step on one composited sample: `acc`'s ink moves
/// toward `adjusted` by `k` (opacity × mask), alpha kept —
/// `blend::mix_adjusted_rgb` (its master) on the four inks, full strength
/// copying `adjusted` verbatim as it does.
pub(crate) fn tone_mix(acc: &mut InkPixel, adjusted: [f32; 4], k: f32) {
    if k >= 1.0 {
        acc[..4].copy_from_slice(&adjusted);
        return;
    }
    for c in 0..4 {
        acc[c] += (adjusted[c] - acc[c]) * k;
    }
}

/// A straight composite sample flattened onto paper (module doc): ink × α,
/// alpha 1.
pub(crate) fn on_paper(pixel: InkPixel) -> [f32; 4] {
    let a = pixel[4].clamp(0.0, 1.0);
    std::array::from_fn(|c| pixel[c] * a)
}

/// The straight composite sample as stored codes.
pub(crate) fn codes(pixel: InkPixel) -> [u16; 5] {
    std::array::from_fn(|c| code(pixel[c]))
}

/// The selection an ink plate loads (Select ▸ Load Selection, Ctrl-click on
/// the plate): its STORED domain `1 − ink`, `65535 − sample[plate]`, plate
/// C 0 … K 3 — exact against Photoshop 26.10.0 on every pixel of every
/// plate (`scripts/photoshop-probe/cmyk_channels_take.py`, take1).
pub(crate) fn plate_coverage(sample: [u16; 5], plate: usize) -> u16 {
    65535 - sample[plate]
}

/// The weight difference [`ink_blended`]'s Darker/Lighter Color pick reads
/// as a tie.
const PICK_TIE: f32 = 1e-6;

/// `B(Cb, Cs)` of `mode` on ink (module doc): the blended ink the W3C step
/// mixes in. Normal answers `cs`; Dissolve, which has no blend function,
/// answers `cs` too (its callers branch first).
pub(crate) fn ink_blended(mode: BlendMode, cb: [f32; 4], cs: [f32; 4]) -> [f32; 4] {
    let l = |ink: [f32; 4]| ink.map(|v| 1.0 - v);
    match mode {
        BlendMode::Normal | BlendMode::Dissolve | BlendMode::PassThrough => cs,
        // A whole-sample pick: the four plates move together, by the CMY
        // luminosity of `L` scaled by the K plate's `L`.
        BlendMode::DarkerColor | BlendMode::LighterColor => {
            let weight = |ink: [f32; 4]| {
                let paper = l(ink);
                lum([paper[0], paper[1], paper[2]]) * paper[3]
            };
            let (b, s) = (weight(cb), weight(cs));
            // A tie keeps the backdrop. A neutral CMY grey and the K of the
            // same value tie EXACTLY (both weigh `L`), which f32 rounding of
            // the 0.3/0.59/0.11 sum must not break, so a difference within
            // PICK_TIE — far below one 16-bit code's 1.5e-5 — is a tie
            // (Photoshop take1 keeps the backdrop there).
            let source = if mode == BlendMode::DarkerColor {
                s < b - PICK_TIE
            } else {
                s > b + PICK_TIE
            };
            if source {
                cs
            } else {
                cb
            }
        }
        _ => match blend_kind(mode) {
            BlendKind::Separable(f) => std::array::from_fn(|c| 1.0 - f(1.0 - cb[c], 1.0 - cs[c])),
            BlendKind::NonSeparable(f) => {
                let (lb, ls) = (l(cb), l(cs));
                let out = f([lb[0], lb[1], lb[2]], [ls[0], ls[1], ls[2]]);
                let k = if mode == BlendMode::Luminosity {
                    cs[3]
                } else {
                    cb[3]
                };
                [1.0 - out[0], 1.0 - out[1], 1.0 - out[2], k]
            }
            BlendKind::Dissolve => cs,
        },
    }
}

/// Straight ink `src` over `dst` at effective alpha `sa` through `mode`
/// (module doc): Normal is [`over`] verbatim, Dissolve shows `src` opaque
/// where `dissolve_threshold(xy) < sa`, every other mode is
/// `blend::composite_blended` with [`ink_blended`]. `sa <= 0` leaves `dst`
/// untouched.
pub(crate) fn over_mode(
    dst: &mut InkPixel,
    src: [f32; 4],
    sa: f32,
    mode: BlendMode,
    xy: (i64, i64),
) {
    if sa <= 0.0 {
        return;
    }
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => over(dst, src, sa),
        BlendMode::Dissolve => {
            if dissolve_threshold(xy.0, xy.1) < sa {
                *dst = [src[0], src[1], src[2], src[3], 1.0];
            }
        }
        _ => {
            let blended = ink_blended(mode, [dst[0], dst[1], dst[2], dst[3]], src);
            composite_blended(&mut dst[..], &src, &blended, sa);
        }
    }
}

/// [`over_mode`] with `dst`'s alpha FROZEN — Preserve Transparency and a
/// transparency lock — `blend::composite_source_frozen`'s rule (its master)
/// on the inks: a pixel at alpha 0 is
/// untouched, an opaque one takes the plain moded over, and a
/// semi-transparent one takes the moded over run against the same ink seen
/// at alpha 1, keeping its own alpha.
pub(crate) fn over_mode_frozen(
    dst: &mut InkPixel,
    src: [f32; 4],
    sa: f32,
    mode: BlendMode,
    xy: (i64, i64),
) {
    let ab = dst[4];
    if ab <= 0.0 || sa <= 0.0 {
        return;
    }
    if ab >= 1.0 {
        over_mode(dst, src, sa, mode, xy);
        return;
    }
    let mut seen = [dst[0], dst[1], dst[2], dst[3], 1.0];
    over_mode(&mut seen, src, sa, mode, xy);
    dst[..4].copy_from_slice(&seen[..4]);
}

/// A Pass Through group's gate on one sample: `dst` moves to `evaluated`
/// (the children's result over a copy of it) by `t` = opacity × mask —
/// `blend::gate_planes` (the master `gate_source_into`'s arithmetic) on the
/// four inks.
pub(crate) fn gate(dst: &mut InkPixel, evaluated: InkPixel, t: f32) {
    gate_planes(&mut dst[..], &evaluated, t);
}

/// A MODED adjustment record's step (`precision_render`'s
/// `adjust_backdrop_sample16` rule on ink): the adjusted ink is pushed
/// through `mode` as the source against the backdrop, then mixed back by
/// `k` through [`tone_mix`]; alpha never changes. Normal is [`tone_mix`]
/// verbatim; Dissolve copies `adjusted` where `dissolve_threshold(xy) < k`.
pub(crate) fn tone_mix_mode(
    acc: &mut InkPixel,
    adjusted: [f32; 4],
    k: f32,
    mode: BlendMode,
    xy: (i64, i64),
) {
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => tone_mix(acc, adjusted, k),
        BlendMode::Dissolve => {
            if dissolve_threshold(xy.0, xy.1) < k {
                acc[..4].copy_from_slice(&adjusted);
            }
        }
        _ => {
            let effective = ink_blended(mode, [acc[0], acc[1], acc[2], acc[3]], adjusted);
            tone_mix(acc, effective, k);
        }
    }
}

/// Edit ▸ Fade on one CMYK sample — `fade::fade_pixel` (the master) on the
/// four inks: `before` is the state before the faded step, `after` its
/// result. Where both are transparent, or nothing moved and `weight` is 0,
/// `after` verbatim. Else `E` = `after` (Normal) or `after` composited over
/// `before` through `mode` by [`over_mode`] (a transparent `after` gives
/// `before`); `E′` = `after` gated toward `E` by `weight` ([`gate`]); the
/// result `before` gated toward `E′` by `opacity`, alpha `after`'s when
/// frozen.
pub(crate) fn fade_ink(
    before: InkPixel,
    after: InkPixel,
    mode: BlendMode,
    weight: f32,
    opacity: f32,
    frozen_alpha: bool,
    xy: (i64, i64),
) -> InkPixel {
    if before[4] <= 0.0 && after[4] <= 0.0 {
        return after;
    }
    if weight <= 0.0 && before == after {
        return after;
    }
    let target = match mode {
        BlendMode::Normal => after,
        _ if after[4] <= 0.0 => before,
        _ => {
            let mut acc = before;
            over_mode(
                &mut acc,
                [after[0], after[1], after[2], after[3]],
                after[4],
                mode,
                xy,
            );
            acc
        }
    };
    let mut weighted = after;
    gate(&mut weighted, target, weight);
    let mut out = before;
    gate(&mut out, weighted, opacity);
    if frozen_alpha {
        out[4] = after[4];
    }
    out
}
