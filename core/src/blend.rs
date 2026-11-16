//! The blend-mode table and all blend math: the W3C separable and
//! non-separable blend functions, the Dissolve dither, the shared f32 W3C
//! compositing kernel the projection loops run, and the shared straight-alpha
//! source-over/erase pixel primitives. `doc` drives these from its
//! projection; `ops` and `doc_select` route their per-pixel paint math
//! through the primitives here so every paint path shares one
//! implementation. Also home to the Rec. 709 luma coefficients and to `lum`'s
//! 30/59/11 weights in hundredths (`LUM100`).
//!
//! `composite_source_frozen`, THE frozen-alpha rule (Preserve Transparency / Lock Transparency,
//! `lerp(Cb, B(Cb, Cs), sa)` on a semi-transparent pixel with its alpha kept, alpha 0
//! untouched) every paint path at 8, 16 and 32 bits routes through.

use crate::job_control::JobContext;
use crate::ops::CompositeMode;

/// Rec. 709 luma coefficients — the ONE copy, shared by `ops`,
/// `ops_filters`, `adjust` and `doc`.
pub(crate) const LUMA_R: f32 = 0.2126;
pub(crate) const LUMA_G: f32 = 0.7152;
pub(crate) const LUMA_B: f32 = 0.0722;

/// The Rec. 709 luma of RGBA16 codes as a u16 code — the ONE implementation
/// of the typed selection's `luminance` kind (`precision_selection`) and of
/// a CMYK document's composite luminosity (`cmyk_select`): the f64 sum of
/// the f32 coefficients, rounded and clamped. Alpha is not read.
pub(crate) fn luma_code(p: [u16; 4]) -> u16 {
    let value = f64::from(LUMA_R) * f64::from(p[0])
        + f64::from(LUMA_G) * f64::from(p[1])
        + f64::from(LUMA_B) * f64::from(p[2]);
    value.round().clamp(0., 65535.) as u16
}

/// [`lum`]'s 0.30 / 0.59 / 0.11 weights in hundredths — `lum` is the master
/// copy; this is the same triple as whole numbers, for the one caller that
/// rounds the luma of 8-bit codes to a code (`plane_swatch::plane_ink_u8`):
/// integer weights make that rounding exact, where the f32 sum of a `.5` luma
/// could land on either side. `adjust_math::threshold`'s continuous form
/// sums over them in f64, so a grey's luma is the grey itself.
pub(crate) const LUM100: [u32; 3] = [30, 59, 11];

/// [`lum`]'s weights in 2^15 fixed point: 0.30 and 0.11 floored, green the
/// remainder, so the three sum to 32768 and a grey's luma is the grey
/// itself. [`lum_fixed`] is their one use.
const LUM32768: [u32; 3] = [9830, 19334, 3604];

/// The 30/59/11 luma of one pixel's codes on an integer grid, rounded half
/// up to that grid: `(9830 r + 19334 g + 3604 b + 16384) >> 15`. 8-bit codes
/// give an 8-bit code and 15-bit values a 15-bit value. Photoshop 26.10.0's
/// Threshold (8-bit codes; 16-bit documents on its 15-bit grid) and Photo
/// Filter's 8-bit Preserve Luminosity were measured to this form, which
/// differs from [`LUM100`]'s rounding only on a pixel whose luma is an
/// exact half code (`adjust_math::threshold` has the takes). An 8-bit
/// document's Darker Color and Lighter Color compare their two triples by
/// it ([`blend_kind_byte`]).
pub(crate) fn lum_fixed(c: [u32; 3]) -> u32 {
    (LUM32768[0] * c[0] + LUM32768[1] * c[1] + LUM32768[2] * c[2] + 16384) >> 15
}

/// The blend-mode set, mirroring `DrBlendMode` in the C header: 0-13 and
/// 15-22 are separable (per-channel W3C formulas), 23-26 non-separable
/// (whole-RGB-triple SetLum/SetSat math), and Dissolve (14) replaces alpha
/// compositing with a deterministic per-canvas-position dither. Pass Through
/// (27) is the odd one out: it is not a blend function at all but a GROUP's
/// declaration that it has no footprint of its own (see `doc_group`), which
/// is why [`BlendMode::is_group_only`] exists and every pixel-blending entry
/// point refuses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum BlendMode {
    Normal = 0,
    Multiply = 1,
    Screen = 2,
    Overlay = 3,
    SoftLight = 4,
    HardLight = 5,
    Darken = 6,
    Lighten = 7,
    Difference = 8,
    Exclusion = 9,
    ColorDodge = 10,
    ColorBurn = 11,
    Addition = 12,
    Subtract = 13,
    Dissolve = 14,
    LinearBurn = 15,
    DarkerColor = 16,
    LighterColor = 17,
    VividLight = 18,
    LinearLight = 19,
    PinLight = 20,
    HardMix = 21,
    Divide = 22,
    Hue = 23,
    Saturation = 24,
    Color = 25,
    Luminosity = 26,
    /// A GROUP whose children composite straight onto the backdrop below the
    /// group instead of into a private buffer — "Pass Through", and the
    /// default for a new group. Meaningless on a raster layer, which
    /// is why the setter refuses to put it on one.
    PassThrough = 27,
}

impl BlendMode {
    /// Maps a raw `DrBlendMode` value coming across the FFI. 28 (Behind) and
    /// 29 (Clear) are paint-only codes, not layer modes, and stay `None` here:
    /// the Brush, Fill and Stroke entry points read them through
    /// `paint_mode::PaintBlend::from_c`.
    pub fn from_c(value: i32) -> Option<Self> {
        match value {
            0 => Some(BlendMode::Normal),
            1 => Some(BlendMode::Multiply),
            2 => Some(BlendMode::Screen),
            3 => Some(BlendMode::Overlay),
            4 => Some(BlendMode::SoftLight),
            5 => Some(BlendMode::HardLight),
            6 => Some(BlendMode::Darken),
            7 => Some(BlendMode::Lighten),
            8 => Some(BlendMode::Difference),
            9 => Some(BlendMode::Exclusion),
            10 => Some(BlendMode::ColorDodge),
            11 => Some(BlendMode::ColorBurn),
            12 => Some(BlendMode::Addition),
            13 => Some(BlendMode::Subtract),
            14 => Some(BlendMode::Dissolve),
            15 => Some(BlendMode::LinearBurn),
            16 => Some(BlendMode::DarkerColor),
            17 => Some(BlendMode::LighterColor),
            18 => Some(BlendMode::VividLight),
            19 => Some(BlendMode::LinearLight),
            20 => Some(BlendMode::PinLight),
            21 => Some(BlendMode::HardMix),
            22 => Some(BlendMode::Divide),
            23 => Some(BlendMode::Hue),
            24 => Some(BlendMode::Saturation),
            25 => Some(BlendMode::Color),
            26 => Some(BlendMode::Luminosity),
            27 => Some(BlendMode::PassThrough),
            _ => None,
        }
    }

    /// The `DrBlendMode` value for the FFI.
    pub fn to_c(self) -> i32 {
        self as i32
    }

    /// True for a mode that means something only on a GROUP entry. A
    /// pass-through group is never composited as a unit, so the compositor
    /// never asks it for a blend function; everything that blends real
    /// pixels — the layer blend-mode setter, the paint blends, Apply Image
    /// and Calculations — refuses it rather than quietly behaving as Normal.
    pub fn is_group_only(self) -> bool {
        matches!(self, BlendMode::PassThrough)
    }
}

// --------------------------------------------------------- blend functions --
// W3C separable blend functions B(cb, cs); all inputs and outputs in [0, 1].

fn b_normal(_cb: f32, cs: f32) -> f32 {
    cs
}

pub(crate) fn b_multiply(cb: f32, cs: f32) -> f32 {
    cb * cs
}

pub(crate) fn b_screen(cb: f32, cs: f32) -> f32 {
    cb + cs - cb * cs
}

fn b_hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        2.0 * cb * cs
    } else {
        1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
    }
}

fn b_overlay(cb: f32, cs: f32) -> f32 {
    b_hard_light(cs, cb)
}

fn b_darken(cb: f32, cs: f32) -> f32 {
    cb.min(cs)
}

fn b_lighten(cb: f32, cs: f32) -> f32 {
    cb.max(cs)
}

fn b_difference(cb: f32, cs: f32) -> f32 {
    (cb - cs).abs()
}

fn b_exclusion(cb: f32, cs: f32) -> f32 {
    cb + cs - 2.0 * cb * cs
}

pub(crate) fn b_color_dodge(cb: f32, cs: f32) -> f32 {
    if cb == 0.0 {
        0.0
    } else if cs == 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
}

pub(crate) fn b_color_burn(cb: f32, cs: f32) -> f32 {
    if cb == 1.0 {
        1.0
    } else if cs == 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - cb) / cs).min(1.0)
    }
}

fn soft_light_d(cb: f32) -> f32 {
    if cb <= 0.25 {
        ((16.0 * cb - 12.0) * cb + 4.0) * cb
    } else {
        cb.sqrt()
    }
}

fn b_soft_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        cb + (2.0 * cs - 1.0) * (soft_light_d(cb) - cb)
    }
}

pub(crate) fn b_addition(cb: f32, cs: f32) -> f32 {
    (cb + cs).min(1.0)
}

fn b_subtract(cb: f32, cs: f32) -> f32 {
    (cb - cs).max(0.0)
}

pub(crate) fn b_linear_burn(cb: f32, cs: f32) -> f32 {
    (cb + cs - 1.0).max(0.0)
}

pub(crate) fn b_vivid_light(cb: f32, cs: f32) -> f32 {
    // Photoshop's halves test the SOURCE's ends first: a black source is
    // Burn(cb, 0) = 0 even over white, a white one Dodge(cb, 1) = 1 even
    // over black, where the stand-alone W3C Color Burn / Color Dodge test the
    // backdrop's end first (Photoshop 26.10.0 at RGB 8, RGB 16 and CMYK,
    // `scripts/photoshop-probe/blend_edge_take.py` take1, VL-PS: every edge
    // patch exact, the stand-alone modes unchanged).
    if cs == 0.0 {
        return 0.0;
    }
    if cs == 1.0 {
        return 1.0;
    }
    // At cs == 0.5 exactly, burn(cb, 1.0) == cb, matching the dodge branch's
    // limit, so the seam is continuous.
    if cs <= 0.5 {
        b_color_burn(cb, 2.0 * cs)
    } else {
        b_color_dodge(cb, 2.0 * cs - 1.0)
    }
}

pub(crate) fn b_linear_light(cb: f32, cs: f32) -> f32 {
    (cb + 2.0 * cs - 1.0).clamp(0.0, 1.0)
}

fn b_pin_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb.min(2.0 * cs)
    } else {
        cb.max(2.0 * cs - 1.0)
    }
}

/// Hard Mix's exact tie `cb + cs = 1`: within 2⁻²², a few f32 ulps of 1, so
/// an 8-bit pair `b + s = 255` lands in it whatever its rounding, while the
/// nearest 16-bit non-tie (1/65535 away) stays outside.
const HARD_MIX_TIE: f32 = 1.0 / (1u32 << 22) as f32;

pub(crate) fn b_hard_mix(cb: f32, cs: f32) -> f32 {
    // Photoshop 26.10.0 breaks the exact tie by order: 1 where the backdrop
    // is the lighter by more than one 15-bit step (`cb − cs > 1/32768`), else
    // 0 — so 16-bit 32767 / 32768, one 15-bit value, reads 0
    // (`scripts/photoshop-probe/gray_luminance_take.py` take1, HM-ORD at RGB
    // 8, RGB 16 and CMYK: every tie patch exact). Off the tie it is `≥`.
    if (cb + cs - 1.0).abs() <= HARD_MIX_TIE {
        return if cb - cs > 1.0 / 32768.0 { 1.0 } else { 0.0 };
    }
    if cb + cs >= 1.0 {
        1.0
    } else {
        0.0
    }
}

fn b_divide(cb: f32, cs: f32) -> f32 {
    // A black source divides to white, except over black: Photoshop keeps the
    // backdrop's 0 at 0 / 0 (Photoshop 26.10.0 at RGB 8, RGB 16 and CMYK,
    // `scripts/photoshop-probe/blend_edge_take.py` take1, D-KEEP: every edge
    // patch exact).
    if cs == 0.0 {
        if cb == 0.0 {
            0.0
        } else {
            1.0
        }
    } else {
        (cb / cs).min(1.0)
    }
}

// W3C non-separable blend machinery (compositing-1 spec pseudocode) operating
// on RGB triples in [0, 1]. Lum uses the spec's 0.3/0.59/0.11 weights.

pub(crate) fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// Which branches of [`clip_color_within`] fired: `low` when the triple's
/// minimum was below 0, `high` when its maximum was above the ceiling. Black
/// & White's Tint reads them to add Photoshop's floor (`adjust_mix`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Clipped {
    pub low: bool,
    pub high: bool,
}

/// ClipColor with the upper bound at `ceiling` (1.0 is the W3C spec's):
/// pulls out-of-gamut channels back toward the triple's luminosity,
/// preserving it. `n` and `x` are taken once, before either branch, as the
/// spec's pseudocode does. The divisors are safe: `l` is a real colour's
/// luminosity (at least 0), so `n < 0` implies `l - n > 0`, and the high
/// branch runs only when `x > l` as well — a neutral triple that rounding
/// left a hair above the ceiling has `x == l` and nothing to scale.
pub(crate) fn clip_color_within(mut c: [f32; 3], ceiling: f32) -> ([f32; 3], Clipped) {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut clipped = Clipped::default();
    if n < 0.0 {
        clipped.low = true;
        for ch in &mut c {
            *ch = l + (*ch - l) * l / (l - n);
        }
    }
    if x > ceiling && x > l {
        clipped.high = true;
        for ch in &mut c {
            *ch = l + (*ch - l) * (ceiling - l) / (x - l);
        }
    }
    (c, clipped)
}

/// SetLum with ClipColor's upper bound at `ceiling`: shifts the triple to
/// luminosity `l`, then clips (see [`clip_color_within`]).
pub(crate) fn set_lum_within(c: [f32; 3], l: f32, ceiling: f32) -> ([f32; 3], Clipped) {
    let d = l - lum(c);
    clip_color_within([c[0] + d, c[1] + d, c[2] + d], ceiling)
}

/// SetLum: shifts the triple to luminosity `l`, then clips into gamut.
pub(crate) fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    set_lum_within(c, l, 1.0).0
}

pub(crate) fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// SetSat: rescales the triple to saturation `s` — min channel to 0, max to
/// `s`, mid proportionally between them (spec pseudocode).
pub(crate) fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&a, &b| c[a].total_cmp(&c[b]));
    let [lo, mid, hi] = idx;
    let mut out = [0.0f32; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}

fn b_hue(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    set_lum(set_sat(cs, sat(cb)), lum(cb))
}

fn b_saturation(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    set_lum(set_sat(cb, sat(cs)), lum(cb))
}

fn b_color(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    set_lum(cs, lum(cb))
}

fn b_luminosity(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    set_lum(cb, lum(cs))
}

/// Whole-pixel pick: the lower-luma triple wins (backdrop on a tie).
fn b_darker_color(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    if lum(cs) < lum(cb) {
        cs
    } else {
        cb
    }
}

/// Whole-pixel pick: the higher-luma triple wins (backdrop on a tie).
fn b_lighter_color(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    if lum(cs) > lum(cb) {
        cs
    } else {
        cb
    }
}

/// A straight [0, 1] component as its 8-bit code, rounded: the one quantizer
/// of the 8-bit kinds ([`lum8`] and the code kernels below), which read the
/// backdrop and the source as the codes the document stores.
fn code8(v: f32) -> u32 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u32
}

/// `n / d` rounded half up, on codes (`d > 0`).
fn rdiv(n: u32, d: u32) -> u32 {
    (2 * n + d) / (2 * d)
}

/// A code as a straight [0, 1] component.
fn unit8(code: u32) -> f32 {
    code as f32 / 255.0
}

/// A straight [0, 1] triple's luma as an 8-bit code: each component rounded
/// to its code, then [`lum_fixed`].
fn lum8(c: [f32; 3]) -> u32 {
    lum_fixed(c.map(code8))
}

/// The 8-bit variant of [`b_linear_light`] (the master): `b + 2s − 256` on
/// the two codes, clamped. Its neutral is code 128, where the master's ½ is
/// 127.5 codes; a recorded take measured the code form on every 8-bit tile
/// (`psd_appearance_578_take.py` takes 1 and 2, where the float form missed
/// a clip tile by 2 codes). At 16 bits the master matched.
pub(crate) fn b_linear_light8(cb: f32, cs: f32) -> f32 {
    let (b, s) = (code8(cb), code8(cs));
    unit8((b + 2 * s).saturating_sub(256).min(255))
}

/// The 8-bit variant of [`b_divide`] (the master): `255·b / s` on the two
/// codes, rounded half up and capped at 255, the master's black-source rule
/// kept. On code inputs it differs from the master only where a half code
/// rounds; it matters for a source that is not on a code, a clip group's
/// float buffer, which it reads as the code that buffer stores (the takes of
/// [`b_linear_light8`]: the float form missed that clip tile by 7 codes).
fn b_divide8(cb: f32, cs: f32) -> f32 {
    let (b, s) = (code8(cb), code8(cs));
    if s == 0 {
        return if b == 0 { 0.0 } else { 1.0 };
    }
    unit8(rdiv(255 * b, s).min(255))
}

/// The 8-bit variant of [`b_vivid_light`] (the master), on the two codes
/// with the master's source ends first: below code 128 the burn half on
/// `t = (255·s + 64) >> 7`, `255 − rdiv(255·(255 − b), t)` floored at 0;
/// from 128 the dodge half on `t = rdiv(255·(s − 128), 127)`,
/// `rdiv(255·b, 255 − t)` capped at 255. The halves split at code 128, the
/// master's at ½. The takes of [`b_linear_light8`] measured it (the float
/// form missed that clip tile by 4 codes), and the blend-edge take
/// (`blend_edge_take.py` take1), held out from that fit, is exact under it on
/// every 8-bit sample, where the float form is up to 2 codes off. Lab's 8-bit
/// Vivid Light is a separate measured form (`lab_blend::vivid_int8`,
/// truncated).
pub(crate) fn b_vivid_light8(cb: f32, cs: f32) -> f32 {
    let (b, s) = (code8(cb), code8(cs));
    if s == 0 {
        return 0.0;
    }
    if s == 255 {
        return 1.0;
    }
    if s < 128 {
        let t = (255 * s + 64) >> 7;
        return unit8(255u32.saturating_sub(rdiv(255 * (255 - b), t)));
    }
    let t = rdiv(255 * (s - 128), 127);
    if t >= 255 {
        1.0
    } else {
        unit8(rdiv(255 * b, 255 - t).min(255))
    }
}

/// The 8-bit variant of [`b_darker_color`] (the master): the two lumas
/// compared as codes ([`lum8`]), so two triples whose lumas round to one code
/// tie and the backdrop is kept. A recorded take measured this at 8 bits
/// (`psd_appearance_578_take.py` take2, a clip tile whose float lumas
/// 151.32 and 150.79 both round to 151); at 16 bits the master's float
/// compare matched.
fn b_darker_color8(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    if lum8(cs) < lum8(cb) {
        cs
    } else {
        cb
    }
}

/// The 8-bit variant of [`b_lighter_color`] (the master), as
/// [`b_darker_color8`] (take1's tile whose float lumas 190.87 and 191 both
/// round to 191).
fn b_lighter_color8(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    if lum8(cs) > lum8(cb) {
        cs
    } else {
        cb
    }
}

/// How a blend mode participates in compositing: per-channel `B(cb, cs)`,
/// whole-RGB-triple `B(Cb, Cs)`, or the Dissolve dither (which replaces the
/// compositing formula entirely).
#[derive(Clone, Copy)]
pub(crate) enum BlendKind {
    Separable(fn(f32, f32) -> f32),
    NonSeparable(fn([f32; 3], [f32; 3]) -> [f32; 3]),
    Dissolve,
}

/// `B(Cb, Cs) = Cb`: the paint-only Behind mode (`paint_mode`). Put into the
/// W3C step it gives `co·ao = sa(1 − ab)Cs + sa·ab·Cb + (1 − sa)ab·Cb =
/// sa(1 − ab)Cs + ab·Cb` with `ao = ab + sa(1 − ab)` — destination-over, the
/// paint landing only where the backdrop is transparent. Not a `BlendMode`:
/// no layer can carry it.
fn b_behind(cb: f32, _cs: f32) -> f32 {
    cb
}

/// The Behind kind every paint walk composites through (`b_behind`).
pub(crate) const BEHIND: BlendKind = BlendKind::Separable(b_behind);

/// The blend behavior for a mode (data table, resolved once per layer rather
/// than per pixel).
pub(crate) fn blend_kind(mode: BlendMode) -> BlendKind {
    match mode {
        BlendMode::Normal => BlendKind::Separable(b_normal),
        BlendMode::Multiply => BlendKind::Separable(b_multiply),
        BlendMode::Screen => BlendKind::Separable(b_screen),
        BlendMode::Overlay => BlendKind::Separable(b_overlay),
        BlendMode::SoftLight => BlendKind::Separable(b_soft_light),
        BlendMode::HardLight => BlendKind::Separable(b_hard_light),
        BlendMode::Darken => BlendKind::Separable(b_darken),
        BlendMode::Lighten => BlendKind::Separable(b_lighten),
        BlendMode::Difference => BlendKind::Separable(b_difference),
        BlendMode::Exclusion => BlendKind::Separable(b_exclusion),
        BlendMode::ColorDodge => BlendKind::Separable(b_color_dodge),
        BlendMode::ColorBurn => BlendKind::Separable(b_color_burn),
        BlendMode::Addition => BlendKind::Separable(b_addition),
        BlendMode::Subtract => BlendKind::Separable(b_subtract),
        BlendMode::Dissolve => BlendKind::Dissolve,
        BlendMode::LinearBurn => BlendKind::Separable(b_linear_burn),
        BlendMode::DarkerColor => BlendKind::NonSeparable(b_darker_color),
        BlendMode::LighterColor => BlendKind::NonSeparable(b_lighter_color),
        BlendMode::VividLight => BlendKind::Separable(b_vivid_light),
        BlendMode::LinearLight => BlendKind::Separable(b_linear_light),
        BlendMode::PinLight => BlendKind::Separable(b_pin_light),
        BlendMode::HardMix => BlendKind::Separable(b_hard_mix),
        BlendMode::Divide => BlendKind::Separable(b_divide),
        BlendMode::Hue => BlendKind::NonSeparable(b_hue),
        BlendMode::Saturation => BlendKind::NonSeparable(b_saturation),
        BlendMode::Color => BlendKind::NonSeparable(b_color),
        BlendMode::Luminosity => BlendKind::NonSeparable(b_luminosity),
        // Pass Through has meaning only on a GROUP, where the compositor
        // never asks for a kind because a pass-through group is never
        // composited as a unit — its children go straight onto the enclosing
        // accumulator (`doc_group::composite_level_into`). A leaf that
        // somehow carries it (a v7 file written by a newer build, say)
        // composites as Normal; the setter refuses to create one.
        BlendMode::PassThrough => BlendKind::Separable(b_normal),
    }
}

/// The kind a layer's blend composites with in an 8-bit RGB document: with
/// `encoded` (no linear-light transfer) Darker Color and Lighter Color compare
/// their lumas as codes ([`b_darker_color8`], [`b_lighter_color8`]), and
/// Vivid Light, Linear Light and Divide blend on codes ([`b_vivid_light8`],
/// [`b_linear_light8`], [`b_divide8`]); everything else, and all five under
/// `linear_light`, is [`blend_kind`]. A blended byte paint walk (Brush, Fill,
/// Stroke, the blended overlay) takes it too, since it lands what a layer in
/// that mode would flatten to; gradients, Fade and effect inks keep
/// [`blend_kind`].
pub(crate) fn blend_kind_byte(mode: BlendMode, encoded: bool) -> BlendKind {
    match mode {
        BlendMode::DarkerColor if encoded => BlendKind::NonSeparable(b_darker_color8),
        BlendMode::LighterColor if encoded => BlendKind::NonSeparable(b_lighter_color8),
        BlendMode::VividLight if encoded => BlendKind::Separable(b_vivid_light8),
        BlendMode::LinearLight if encoded => BlendKind::Separable(b_linear_light8),
        BlendMode::Divide if encoded => BlendKind::Separable(b_divide8),
        _ => blend_kind(mode),
    }
}

/// Deterministic Dissolve threshold for a canvas position, in [0, 1).
/// Murmur3-style integer mix of the coordinates; canvas-absolute so the
/// dither pattern is stable for a given position regardless of layer offset
/// or projection window. Only the low 24 hash bits are used so the result is
/// exact in f32 and strictly below 1 (a fully opaque pixel always shows).
pub(crate) fn dissolve_threshold(x: i64, y: i64) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add((y as u32).wrapping_mul(0x85EB_CA6B));
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    ((h >> 8) as f32) * (1.0 / 16_777_216.0)
}

// ------------------------------------------------- shared pixel kernels --

/// One W3C compositing step of a straight-color source (`cs`, effective
/// alpha `sa` > 0, both in [0, 1]) onto accumulator pixel `acc[ai]` — THE
/// inner loop shared by `doc`'s layer and clip-group compositors.
///
/// Dissolve skips the compositing formula: the source shows fully opaque
/// with probability `sa` (its effective alpha), decided by the deterministic
/// canvas-absolute threshold at `canvas_xy`; otherwise the backdrop pixel
/// stays untouched.
pub(crate) fn composite_source_into(
    acc: &mut [[f32; 4]],
    ai: usize,
    cs: [f32; 3],
    sa: f32,
    kind: BlendKind,
    canvas_xy: (i64, i64),
) {
    if let BlendKind::Dissolve = kind {
        if dissolve_threshold(canvas_xy.0, canvas_xy.1) < sa {
            acc[ai] = [cs[0], cs[1], cs[2], 1.0];
        }
        return;
    }
    let bg = acc[ai];
    let blended = match kind {
        BlendKind::Separable(f) => [f(bg[0], cs[0]), f(bg[1], cs[1]), f(bg[2], cs[2])],
        BlendKind::NonSeparable(f) => f([bg[0], bg[1], bg[2]], cs),
        BlendKind::Dissolve => unreachable!("dissolve handled above"),
    };
    composite_blended(&mut acc[ai][..], &cs, &blended, sa);
}

/// The W3C compositing step over ANY channel count, once the blend function
/// has been evaluated — [`composite_source_into`]'s arithmetic, the MASTER
/// copy (it delegates here): `dst` is straight colour planes then alpha,
/// `cs` the source colour and `blended` `B(Cb, Cs)` per plane
/// (`cs.len() == blended.len() == dst.len() - 1`), `sa` > 0 the source's
/// effective alpha. `cmyk_composite` runs it on the four inks.
pub(crate) fn composite_blended(dst: &mut [f32], cs: &[f32], blended: &[f32], sa: f32) {
    let n = cs.len();
    let ab = dst[n];
    let ao = sa + ab * (1.0 - sa);
    for c in 0..n {
        dst[c] = (sa * (1.0 - ab) * cs[c] + sa * ab * blended[c] + (1.0 - sa) * ab * dst[c]) / ao;
    }
    dst[n] = ao;
}

/// [`composite_blended`] with the source's coverage split from its blend
/// strength — `blend_fill`'s form Fb (#2): `sa_bare` weights the source
/// where the backdrop is empty (the `(1 − ab)` term, so a special Fill still
/// fades the source over transparency) and `sa_over` weights the blend over
/// the backdrop. `αo = sa_bare(1 − ab) + ab`, `co·αo = sa_bare(1 − ab)cs +
/// sa_over·ab·B + (1 − sa_over)·ab·cb`; equal to `composite_blended` at
/// `sa_bare == sa_over`, which stays the master of the W3C step.
pub(crate) fn composite_blended_split(
    dst: &mut [f32],
    cs: &[f32],
    blended: &[f32],
    sa_bare: f32,
    sa_over: f32,
) {
    let n = cs.len();
    let ab = dst[n];
    let ao = sa_bare * (1.0 - ab) + ab;
    if ao <= 0.0 {
        return;
    }
    for c in 0..n {
        dst[c] = (sa_bare * (1.0 - ab) * cs[c]
            + sa_over * ab * blended[c]
            + (1.0 - sa_over) * ab * dst[c])
            / ao;
    }
    dst[n] = ao;
}

/// [`composite_source_into`] onto ONE straight pixel whose alpha is frozen —
/// Preserve Transparency and Lock Transparency, and the ONE copy of
/// that rule every paint path at every depth routes through.
///
/// A pixel at alpha 0 is untouched (nothing there to recolour). A
/// semi-transparent pixel takes `lerp(Cb, B(Cb, Cs), sa)` in its colour and
/// keeps its alpha exactly: the W3C step run against the same colour seen at
/// alpha 1, where `(sa·B + (1 − sa)·Cb) / 1` is that lerp, so the paint mixes
/// in at its own strength instead of being boosted by an alpha restore that
/// divides out the backdrop's transparency. An opaque pixel (alpha ≥ 1) is
/// the plain W3C step, whose output alpha is 1 already. Dissolve keeps its
/// canvas-absolute dither: the source colour shows where it fires.
pub(crate) fn composite_source_frozen(
    px: &mut [f32; 4],
    cs: [f32; 3],
    sa: f32,
    kind: BlendKind,
    canvas_xy: (i64, i64),
) {
    let ab = px[3];
    if ab <= 0.0 {
        return;
    }
    if ab >= 1.0 {
        composite_source_into(std::slice::from_mut(px), 0, cs, sa, kind, canvas_xy);
        return;
    }
    let mut seen = [[px[0], px[1], px[2], 1.0]];
    composite_source_into(&mut seen, 0, cs, sa, kind, canvas_xy);
    px[..3].copy_from_slice(&seen[0][..3]);
}

/// Composites an accumulator-ALIGNED straight f32 buffer (same size, same
/// origin as `acc`) onto `acc` with one blend mode and opacity — the clip
/// group's final step, and the styled clip base's. Per pixel
/// `sa = buf alpha * opacity`; pixels at alpha 0 are skipped, so colour a
/// clipped layer left outside the footprint at forced alpha 0 never shows.
/// `origin` keeps Dissolve's dither canvas-absolute.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_buffer_into(
    acc: &mut [[f32; 4]],
    buf: &[[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    kind: BlendKind,
    opacity: f32,
    job: JobContext<'_>,
) -> Result<(), String> {
    for ay in 0..i64::from(acc_h) {
        job.check()?;
        for ax in 0..i64::from(acc_w) {
            let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
            let src = buf[ai];
            let sa = src[3] * opacity;
            if sa <= 0.0 {
                continue;
            }
            let cs = [src[0], src[1], src[2]];
            let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
            composite_source_into(acc, ai, cs, sa, kind, canvas_xy);
        }
    }
    Ok(())
}

/// The shared straight-alpha source-over write: composites a source with
/// effective alpha `sa` and PREMULTIPLIED f32 color `scp` (straight color
/// times `sa`, all in [0, 1]) onto the straight RGBA8 pixel `dp`. Callers
/// have already handled `sa <= 0` and a vanishing output alpha, so the
/// division is safe.
pub(crate) fn source_over_rgba8(dp: &mut [u8], scp: [f32; 3], sa: f32) {
    let da = f32::from(dp[3]) / 255.0;
    let out_a = sa + da * (1.0 - sa);
    for c in 0..3 {
        let dc = f32::from(dp[c]) / 255.0;
        let v = (scp[c] + dc * da * (1.0 - sa)) / out_a;
        dp[c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    dp[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
}

/// Per-pixel core of the layer paint path — the ONE implementation of
/// premultiplied-overlay compositing: `DrDocument::painting_layer` maps a
/// canvas-frame overlay through a layer offset onto it.
/// (`doc_select`'s straight-source painter shares [`source_over_rgba8`].)
/// `dp` is a straight-alpha RGBA8 pixel, `sp` a premultiplied overlay pixel,
/// `a` the pre-clamped global alpha.
pub(crate) fn paint_pixel(dp: &mut [u8], sp: [u8; 4], mode: CompositeMode, a: f32) {
    // Fast path: a fully transparent overlay pixel passes the destination
    // bytes through exactly (no float round-trip). For OVER the color bytes
    // must also be zero — they always are in well-formed premultiplied data;
    // malformed pixels fall through to the math.
    let src_transparent = sp[3] == 0
        && match mode {
            CompositeMode::Over => sp[0] == 0 && sp[1] == 0 && sp[2] == 0,
            CompositeMode::Erase => true,
        };
    if src_transparent {
        return;
    }
    let sa = (f32::from(sp[3]) / 255.0) * a;
    let da = f32::from(dp[3]) / 255.0;
    match mode {
        CompositeMode::Over => {
            let out_a = sa + da * (1.0 - sa);
            if out_a < 1e-6 {
                // Keep the destination color bytes so fully transparent
                // regions do not invent color fringes.
                dp[3] = 0;
            } else {
                let scp = [
                    (f32::from(sp[0]) / 255.0) * a,
                    (f32::from(sp[1]) / 255.0) * a,
                    (f32::from(sp[2]) / 255.0) * a,
                ];
                source_over_rgba8(dp, scp, sa);
            }
        }
        CompositeMode::Erase => {
            let out_a = erase_alpha(da, sa);
            dp[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// Shared eraser coverage rule. RGB, including latent transparent RGB, is
/// untouched; each storage boundary owns its established final quantization.
pub(crate) fn erase_alpha(destination: f32, coverage: f32) -> f32 {
    destination * (1.0 - coverage)
}
/// Shared adjustment-opacity interpolation, extracted unchanged from
/// doc::composite_adjustment_into. Alpha stays exact. Callers skip k<=0 and
/// noncontributing backdrop pixels; destructive edits pass k=1.
///
/// Full strength copies verbatim: cb+(effective-cb)*1 can round one ULP below
/// effective at an exact half-byte boundary. This branch preserves the byte
/// adjustment/destructive equality while typed callers retain f32 values.
pub(crate) fn mix_adjusted_rgb(out: &mut [f32; 4], effective: [f32; 3], k: f32) {
    if k >= 1.0 {
        out[..3].copy_from_slice(&effective);
    } else {
        for c in 0..3 {
            out[c] = out[c] + (effective[c] - out[c]) * k;
        }
    }
}

/// Gate an evaluated pass-through backdrop against its original straight RGBA
/// value. Shared by the legacy and typed group renderers. The arithmetic and
/// exact endpoint branches are extracted unchanged from doc_group's original
/// composite_gated_pass: interpolate premultiplied colors, then unpremultiply.
pub(crate) fn gate_source_into(out: &mut [f32; 4], evaluated: [f32; 4], t: f32) {
    gate_planes(&mut out[..], &evaluated, t);
}

/// [`gate_source_into`]'s arithmetic over ANY channel count — the MASTER
/// copy, which it delegates to: `out` and `evaluated` are straight colour
/// planes then alpha, of one length. `cmyk_composite::gate` runs it on the
/// four inks.
pub(crate) fn gate_planes(out: &mut [f32], evaluated: &[f32], t: f32) {
    if t <= 0.0 {
        return;
    }
    if t >= 1.0 {
        out.copy_from_slice(evaluated);
        return;
    }
    let n = out.len() - 1;
    let (a0, a1) = (out[n], evaluated[n]);
    let a = a0 + (a1 - a0) * t;
    if a <= 0.0 {
        out.fill(0.0);
        return;
    }
    for c in 0..n {
        let (p0, p1) = (out[c] * a0, evaluated[c] * a1);
        out[c] = (p0 + (p1 - p0) * t) / a;
    }
    out[n] = a;
}
