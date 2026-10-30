//! KERNEL: the Lab arms of the one compositor (#1 phase 3), the Lab
//! sibling of `blend.rs` as `blend_float.rs` is its float one. A Lab Color
//! document's layers, merges, flatten, render, paint, Fade, the Clone Stamp,
//! the Art History Brush and layer effects composite through [`kind_in`] /
//! [`fill_in`] / [`lab_fade_blend`]; every other document gets exactly the
//! `blend.rs` / `blend_float.rs` / `blend_fill.rs` answer it got before.
//!
//! Inputs are the stored unit codes `u = [L̂, â, b̂]` (`lab_document`): L\* =
//! 100·L̂, a\* = 255·â − 128. Each arm is the first candidate in declared order
//! that met Photoshop 26.10.0 within one 8-bit code per channel at every
//! cell centre at 8 AND 16 bits, in ONE declared take
//! (`scripts/photoshop-probe/lab_blend_take.py`, `lab-blend-681/take2`,
//! VALID; take1 was INVALID by its readback check and decided nothing):
//!
//! - Dissolve, Overlay, Soft Light (plain Fill), Hard Light, Linear Light and
//!   Pin Light — C0, `blend.rs` on every code: [`blend::blend_kind`] itself.
//! - Multiply and Screen — C5: L̂ by the mode, a\* = a\*b + a\*s − |a\*b|·a\*s/128
//!   (and b\*), the same expression for both modes ([`chroma_c5`]).
//! - Linear Burn and Linear Dodge (Add) — C2b with the folded Fill: L̂ by the
//!   mode, a\* = a\*b + a\*s ([`chroma_sum`]).
//! - Vivid Light — VL-INT8 with the folded Fill: at 8 bits `b_vivid_light`'s
//!   halves on the 8-bit codes, truncated to an integer code
//!   ([`vivid_int8`]); at 16 bits C4, L̂ by `b_vivid_light` and a\*/b\* by its
//!   unclamped halves on y = ½ − a\*/256 ([`chroma_vivid16`]).
//! - Hard Mix — HM-DEPTH: at 8 bits C0; at 16 bits C2a, `b_hard_mix` on
//!   y = ½ + a\*/255 ([`chroma_hard_mix16`]). Photoshop thresholds each
//!   depth's own integer codes, so a\*b = 1 over a\*s = −2 is a\* max at 8
//!   bits and a\* min at 16.
//! - Darker Color / Lighter Color — the source where its L\* is lower / higher,
//!   a tie keeping the backdrop. Color — (L_b, a_s, b_s). Luminosity — (L_s,
//!   a_b, b_b). Hue — (L_b, (a_s, b_s)·M_b/M_s) and Saturation — (L_b, (a_b,
//!   b_b)·M_s/M_b), M = max(|a\*|, |b\*|) (CHEB-H, CHEB-S).
//!
//! Every a\*/b\* result is clipped to −128…127. The separable functions are
//! `blend.rs`'s (the master); the chroma forms here are their own formulas.

use crate::blend::{
    b_addition, b_hard_mix, b_linear_burn, b_multiply, b_screen, b_vivid_light, blend_kind,
    BlendKind, BlendMode,
};
use crate::blend_fill::{fill_blend, FillBlend};
use crate::blend_float::blend_kind_at;
use crate::fade::FadeBlend;

/// The grid a Lab document stores: its declared depth. Vivid Light and Hard
/// Mix depend on it (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabGrid {
    Eight,
    Sixteen,
}

/// The unit code of a\* = b\* = 0.
const NEUTRAL_AB: f32 = 128.0 / 255.0;

/// A chroma magnitude below this many a\* units is neutral (0). The finest
/// stored a\* step is 255/65535 ≈ 0.0039 units, and a neutral code read back
/// through f32 lands within ~1e-5 of 0; 1e-3 sits between the two, so the
/// test is exact arithmetic on the stored codes.
const NEUTRAL_CHROMA: f32 = 1.0e-3;

/// a\* (or b\*) of a unit code.
fn star(u: f32) -> f32 {
    255.0 * u - 128.0
}

/// The unit code of a\* (or b\*), clipped to −128…127.
fn unit(a: f32) -> f32 {
    (a.clamp(-128.0, 127.0) + 128.0) / 255.0
}

/// Multiply's and Screen's a\*/b\* (C5).
fn chroma_c5(b: f32, s: f32) -> f32 {
    let (p, q) = (star(b), star(s));
    unit(p + q - p.abs() * q / 128.0)
}

/// Linear Burn's and Linear Dodge's a\*/b\* (C2b): the plain sum.
fn chroma_sum(b: f32, s: f32) -> f32 {
    unit(star(b) + star(s))
}

/// `b_vivid_light`'s halves without their clamp (C4's unclamped form): the
/// source's ends first, as the master.
fn vivid_unclamped(b: f32, s: f32) -> f32 {
    if s == 0.0 {
        return 0.0;
    }
    if s == 1.0 {
        return 1.0;
    }
    if s <= 0.5 {
        1.0 - (1.0 - b) / (2.0 * s)
    } else {
        b / (2.0 - 2.0 * s)
    }
}

/// Vivid Light's a\*/b\* at 16 bits (C4): y = ½ − a\*/256, a\* = −256·(B − ½).
fn chroma_vivid16(b: f32, s: f32) -> f32 {
    let y = |u: f32| 0.5 - star(u) / 256.0;
    unit(-256.0 * (vivid_unclamped(y(b), y(s)) - 0.5))
}

/// Hard Mix's a\*/b\* at 16 bits (C2a): `b_hard_mix` on y = ½ + a\*/255.
fn chroma_hard_mix16(b: f32, s: f32) -> f32 {
    let y = |u: f32| 0.5 + star(u) / 255.0;
    unit(255.0 * (b_hard_mix(y(b), y(s)) - 0.5))
}

/// A unit sample as its 8-bit code.
fn code8(u: f32) -> i32 {
    (255.0 * u.clamp(0.0, 1.0) + 0.5).floor() as i32
}

/// `b_vivid_light` on 8-bit codes, its result truncated to an integer code
/// (VL-INT8): the burn half 255 − ⌈255·(255 − b)/(2s)⌉ floored at 0, the
/// dodge half ⌊255·b/(510 − 2s)⌋ capped at 255, the source's ends first.
fn vivid_int8(b: i32, s: i32) -> i32 {
    if s == 0 {
        return 0;
    }
    if s == 255 {
        return 255;
    }
    if s <= 127 {
        let d = 2 * s;
        (255 - (255 * (255 - b) + d - 1) / d).max(0)
    } else {
        (255 * b / (510 - 2 * s)).min(255)
    }
}

fn lab_multiply(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    [
        b_multiply(cb[0], cs[0]),
        chroma_c5(cb[1], cs[1]),
        chroma_c5(cb[2], cs[2]),
    ]
}

fn lab_screen(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    [
        b_screen(cb[0], cs[0]),
        chroma_c5(cb[1], cs[1]),
        chroma_c5(cb[2], cs[2]),
    ]
}

fn lab_linear_burn(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    [
        b_linear_burn(cb[0], cs[0]),
        chroma_sum(cb[1], cs[1]),
        chroma_sum(cb[2], cs[2]),
    ]
}

fn lab_linear_dodge(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    [
        b_addition(cb[0], cs[0]),
        chroma_sum(cb[1], cs[1]),
        chroma_sum(cb[2], cs[2]),
    ]
}

fn lab_vivid_light8(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|k| vivid_int8(code8(cb[k]), code8(cs[k])) as f32 / 255.0)
}

fn lab_vivid_light16(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    [
        b_vivid_light(cb[0], cs[0]),
        chroma_vivid16(cb[1], cs[1]),
        chroma_vivid16(cb[2], cs[2]),
    ]
}

fn lab_hard_mix16(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    [
        b_hard_mix(cb[0], cs[0]),
        chroma_hard_mix16(cb[1], cs[1]),
        chroma_hard_mix16(cb[2], cs[2]),
    ]
}

fn lab_darker_color(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    if cs[0] < cb[0] {
        cs
    } else {
        cb
    }
}

fn lab_lighter_color(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    if cs[0] > cb[0] {
        cs
    } else {
        cb
    }
}

/// (a\*, b\*, max(|a\*|, |b\*|)) of a sample, the magnitude 0 when neutral.
fn chebyshev(c: [f32; 3]) -> (f32, f32, f32) {
    let (a, b) = (star(c[1]), star(c[2]));
    let m = a.abs().max(b.abs());
    (a, b, if m < NEUTRAL_CHROMA { 0.0 } else { m })
}

fn lab_hue(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    let (_, _, mb) = chebyshev(cb);
    let (a, b, ms) = chebyshev(cs);
    let k = if ms > 0.0 { mb / ms } else { 0.0 };
    [cb[0], unit(a * k), unit(b * k)]
}

fn lab_saturation(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    let (a, b, mb) = chebyshev(cb);
    let (_, _, ms) = chebyshev(cs);
    let k = if mb > 0.0 { ms / mb } else { 0.0 };
    [cb[0], unit(a * k), unit(b * k)]
}

fn lab_color(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    [cb[0], cs[1], cs[2]]
}

fn lab_luminosity(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    [cs[0], cb[1], cb[2]]
}

/// The blend behaviour of `mode` in a Lab document on `grid` (module doc):
/// a Lab arm where the take's verdict is not C0, else `blend::blend_kind`.
pub(crate) fn lab_blend_kind(mode: BlendMode, grid: LabGrid) -> BlendKind {
    match (mode, grid) {
        (BlendMode::Multiply, _) => BlendKind::NonSeparable(lab_multiply),
        (BlendMode::Screen, _) => BlendKind::NonSeparable(lab_screen),
        (BlendMode::LinearBurn, _) => BlendKind::NonSeparable(lab_linear_burn),
        (BlendMode::Addition, _) => BlendKind::NonSeparable(lab_linear_dodge),
        (BlendMode::VividLight, LabGrid::Eight) => BlendKind::NonSeparable(lab_vivid_light8),
        (BlendMode::VividLight, LabGrid::Sixteen) => BlendKind::NonSeparable(lab_vivid_light16),
        (BlendMode::HardMix, LabGrid::Sixteen) => BlendKind::NonSeparable(lab_hard_mix16),
        (BlendMode::DarkerColor, _) => BlendKind::NonSeparable(lab_darker_color),
        (BlendMode::LighterColor, _) => BlendKind::NonSeparable(lab_lighter_color),
        (BlendMode::Hue, _) => BlendKind::NonSeparable(lab_hue),
        (BlendMode::Saturation, _) => BlendKind::NonSeparable(lab_saturation),
        (BlendMode::Color, _) => BlendKind::NonSeparable(lab_color),
        (BlendMode::Luminosity, _) => BlendKind::NonSeparable(lab_luminosity),
        _ => blend_kind(mode),
    }
}

/// [`lab_blend_kind`] on the 8-bit grid: the Art History Brush's, which
/// paints only an 8-bit Lab document (`precision_art_history`).
pub(crate) fn lab_blend_kind8(mode: BlendMode) -> BlendKind {
    lab_blend_kind(mode, LabGrid::Eight)
}

/// The kind every compositing site asks: [`lab_blend_kind`] in a Lab
/// document, else `blend_float::blend_kind_at(mode, float)` unchanged.
pub(crate) fn kind_in(mode: BlendMode, float: bool, lab: Option<LabGrid>) -> BlendKind {
    match lab {
        Some(grid) => lab_blend_kind(mode, grid),
        None => blend_kind_at(mode, float),
    }
}

/// `cs` folded toward the neutral `n` by the Fill (form Fb's source, as
/// `blend_fill`'s): `n + fill·(cs − n)` per code.
fn fold(cs: [f32; 3], n: [f32; 3], fill: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| n[k] + fill * (cs[k] - n[k]))
}

fn fill_linear_burn(cb: [f32; 3], cs: [f32; 3], fill: f32) -> [f32; 3] {
    lab_linear_burn(cb, fold(cs, [1.0, NEUTRAL_AB, NEUTRAL_AB], fill))
}

fn fill_linear_dodge(cb: [f32; 3], cs: [f32; 3], fill: f32) -> [f32; 3] {
    lab_linear_dodge(cb, fold(cs, [0.0, NEUTRAL_AB, NEUTRAL_AB], fill))
}

fn fill_vivid_light8(cb: [f32; 3], cs: [f32; 3], fill: f32) -> [f32; 3] {
    lab_vivid_light8(cb, fold(cs, [0.5, NEUTRAL_AB, NEUTRAL_AB], fill))
}

fn fill_vivid_light16(cb: [f32; 3], cs: [f32; 3], fill: f32) -> [f32; 3] {
    lab_vivid_light16(cb, fold(cs, [0.5, NEUTRAL_AB, NEUTRAL_AB], fill))
}

/// A Lab layer's special Fill: Linear Burn, Linear Dodge (Add) and Vivid
/// Light fold the source toward the neutral colour (L\* 100, 0 and 50, a\* =
/// b\* = 0) and composite by form Fb (`blend_fill`'s split); every other
/// mode's Fill is the plain alpha factor (`None`), Soft Light's as measured.
fn lab_fill_blend(mode: BlendMode, fill: f32, grid: LabGrid) -> Option<FillBlend> {
    if fill.is_nan() || fill >= 1.0 {
        return None;
    }
    let f: fn([f32; 3], [f32; 3], f32) -> [f32; 3] = match (mode, grid) {
        (BlendMode::LinearBurn, _) => fill_linear_burn,
        (BlendMode::Addition, _) => fill_linear_dodge,
        (BlendMode::VividLight, LabGrid::Eight) => fill_vivid_light8,
        (BlendMode::VividLight, LabGrid::Sixteen) => fill_vivid_light16,
        _ => return None,
    };
    Some(FillBlend {
        f,
        fill,
        split: true,
    })
}

/// The special Fill every compositing site asks: [`lab_fill_blend`] in a Lab
/// document, else `blend_fill::fill_blend(mode, fill, float)` unchanged.
pub(crate) fn fill_in(
    mode: BlendMode,
    fill: f32,
    float: bool,
    lab: Option<LabGrid>,
) -> Option<FillBlend> {
    match lab {
        Some(grid) => lab_fill_blend(mode, fill, grid),
        None => fill_blend(mode, fill, float),
    }
}

/// Edit ▸ Fade's blend in a Lab document (`fade::fade_blend`'s shape): Pass
/// Through blends nothing, Normal replaces, every other mode composites by
/// [`lab_blend_kind`].
pub(crate) fn lab_fade_blend(mode: BlendMode, grid: LabGrid) -> Option<FadeBlend> {
    match mode {
        BlendMode::PassThrough => None,
        BlendMode::Normal => Some(FadeBlend::Replace),
        _ => Some(FadeBlend::Mode(lab_blend_kind(mode, grid))),
    }
}
