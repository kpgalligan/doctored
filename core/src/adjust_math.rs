//! The pixel-math helpers the adjustment kernels share, and the ONE
//! destructive kernel `dr_image_adjust_op` runs.
//!
//! **This file is shared and append-only, ordered by first caller** — a
//! helper lands here with the op that first needs it, so nothing in it is
//! ever unused and `cargo clippy -D warnings` stays green at every step.
//! Present: the Rec. 709 luma (over `blend`'s `LUMA_*`, the one home for
//! those constants), the sRGB transfer function pair, [`apply_to_image`],
//! [`posterize`] and [`threshold`] (each the ONE kernel of its op),
//! and — added with the colour and mapping ops that first needed them — the
//! HSL round trip and the [`hexcone`] under it, [`hue_distance`],
//! [`two_sided`] and the [`two_sided_extended`] under it,
//! [`decompose`] and the CMYK round trip. There is
//! deliberately NO HSV pair: nothing in the core needs one (the Info
//! panel's HSB readout is display-only and computed host-side), and a
//! conversion with no caller is dead code at `cargo clippy -D warnings`.
//!
//! # Why the sRGB transfer function specifically
//!
//! The shipped numeric Exposure/White Balance descriptions keep the sRGB
//! transfer pair below byte-exactly. Their explicit `document_v2` evaluation
//! uses `adjust_calibrated` and the shared ICC matrix/TRC forward encoder.
//! Both prepare an `Adjustment` before entering `apply_to_image`, so this
//! destructive loop and adjustment-layer compositing share the same kernel.

use image::RgbaImage;

use crate::adjust::Adjustment;
use crate::blend::{lum_fixed, LUM100, LUMA_B, LUMA_G, LUMA_R};

/// Rec. 709 luma of a straight RGB triple in [0, 1] — the ONE luma in this
/// crate, over `blend`'s shared coefficients.
pub(crate) fn luma(rgb: [f32; 3]) -> f32 {
    LUMA_R * rgb[0] + LUMA_G * rgb[1] + LUMA_B * rgb[2]
}

/// The sRGB EOTF: one encoded channel in [0, 1] to light-linear.
pub(crate) fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The sRGB OETF: light-linear back to an encoded channel, the input
/// clamped into [0, 1] first so an op that overshoots (Exposure's gain,
/// White Balance's out-of-gamut corner) lands on black or white rather than
/// on a NaN from a negative base.
pub(crate) fn linear_to_srgb(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    if u <= 0.0031308 {
        12.92 * u
    } else {
        1.055 * u.powf(1.0 / 2.4) - 0.055
    }
}

/// THE destructive kernel: `adjustment` applied to every pixel of `img`,
/// returning a new image with ALPHA COPIED UNTOUCHED. This is the identical
/// code path the compositor runs for an adjustment layer
/// (`doc::composite_adjustment_into`), which is what makes
/// `dr_image_adjust_op` and `add_adjustment_layer` incapable of drifting:
/// the guide plane is built once from THIS image and each pixel is mapped
/// through [`Adjustment::apply_at`] with its own position.
///
/// Two deliberate differences from the layer path, both written up in
/// `adjust`'s module doc as exceptions to invariant 1. A SPATIAL op reads
/// its neighbourhood from this image, while the layer reads it from the
/// backdrop below itself; the two agree on the same input and are different
/// pictures on different inputs. And the position handed to `apply_at` is
/// the position within THIS image, where the layer path hands the CANVAS
/// position, so `gradient_map`'s position-keyed dither lands on different
/// pixels when the layer's offset is not (0, 0) (`dither: false` makes the
/// two exact at any offset).
pub(crate) fn apply_to_image(adjustment: &Adjustment, img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let raw = img.as_raw();
    let guide = adjustment.guide(w, h, &|i| {
        [
            f32::from(raw[i * 4]) / 255.0,
            f32::from(raw[i * 4 + 1]) / 255.0,
            f32::from(raw[i * 4 + 2]) / 255.0,
            f32::from(raw[i * 4 + 3]) / 255.0,
        ]
    });
    let mut out = img.clone();
    let width = i64::from(w);
    for (i, px) in out.pixels_mut().enumerate() {
        let rgb = [
            f32::from(px[0]) / 255.0,
            f32::from(px[1]) / 255.0,
            f32::from(px[2]) / 255.0,
        ];
        let xy = (i as i64 % width, i as i64 / width);
        // 0.0 for an op with no plane: `apply_at`'s guide argument is only
        // ever read by an op whose `guide` returned `Some`.
        let g = guide
            .as_ref()
            .map_or(0.0, |plane| plane.at(xy.0 as u32, xy.1 as u32));
        let adjusted = adjustment.apply_at(rgb, g, xy);
        for (dst, v) in px.0.iter_mut().zip(adjusted) {
            *dst = (v * 255.0).round() as u8;
        }
    }
    out
}

/// Posterize to `levels` (2…64) values at the depth whose code range is
/// `top`, measured against Photoshop 26.10.0 (#2; `scripts/photoshop-probe/
/// adjust_v1x_take.py` and `adjust_v1x_round2.py`, takes 1-4 of
/// `adjust-parity-v1x`):
///
/// * `top` 255 (8 bits): the code `c = round(v·255)` falls in the bin `k =
///   floor(c·n/256)` of `n` equal 256-code bins, and the output is the code
///   `floor(255·k/(n − 1))` (member P256·Ofloor). Exact on every 8-bit
///   component of takes 2, 3 and 4 (round 2's bound, ±1, met); the `n`-bin
///   rule on `v·n` it replaced missed by up to 128 codes (take3).
/// * `top` 1 (32-bit float, the Mapped display value): the continuous bin
///   `k = min(n − 1, floor(v·n))`, output `k/(n − 1)`.
/// * `top` 65535 (16 bits): the code first takes Photoshop's 15-bit grid,
///   `v15 = round(c·32768/65535)`, then `k = min(n − 1, floor(v15·n/
///   32768))` (member Q15a·Uexact), output `k/(n − 1)`. take2: within
///   2/65535 of every sample; the 16-bit `floor(c·n/65535)` misses a whole
///   level at bin edges.
///
/// The caller's encoder rounds the result to its codes. The ONE posterize:
/// the destructive filter and the adjustment layer both call it. At 8 and 16
/// bits the code is recovered exactly (`round(v·top)`) and the bins are
/// integer arithmetic. At float the product is formed in f64 with a 1e-5
/// lift, so a value on a bin edge (`v·n` an integer) is not dropped a bin by
/// the f32 rounding of `v`.
pub(crate) fn posterize(v: f32, levels: u32, top: u32) -> f32 {
    let n = u64::from(levels);
    if top == u32::from(u16::MAX) {
        let k = ((grid15(v) * n) / 32768).min(n - 1);
        return (k as f64 / f64::from(levels - 1)) as f32;
    }
    if top == 255 {
        let code = (f64::from(v.clamp(0.0, 1.0)) * 255.0).round() as u64;
        let k = code * n / 256;
        return (((255 * k) / (n - 1)) as f64 / 255.0) as f32;
    }
    let top_bin = f64::from(levels) - 1.0;
    let k = (f64::from(v) * f64::from(levels) + 1e-5)
        .floor()
        .clamp(0.0, top_bin);
    (k / top_bin) as f32
}

/// A 16-bit sample's value on Photoshop's 15-bit grid, `round(c·32768/
/// 65535)` half up over the code `c = round(v·65535)`: the one conversion
/// [`posterize`] and [`threshold`] read their 16-bit bins and luma on.
fn grid15(v: f32) -> u64 {
    let code = (f64::from(v.clamp(0.0, 1.0)) * 65535.0).round() as u64;
    (code * 65536 + 65535) / 131_070
}

/// Threshold at `level` (in [0, 1]) at the depth whose code range is `top`:
/// 1.0 (white) when the pixel's `0.30 r + 0.59 g + 0.11 b` luma reaches the
/// level, else 0.0, measured against Photoshop 26.10.0 (#2;
/// `scripts/photoshop-probe/adjust_render_2_take.py` and `_take2.py`, takes
/// 1-2 of `adjust-render-2`):
///
/// * `top` 255 (8 bits): [`lum_fixed`] of the pixel's codes against the
///   level rounded to a code.
/// * `top` 65535 (16 bits): [`lum_fixed`] of the pixel's 15-bit values
///   ([`grid15`]) against `round(level·32768)`.
/// * `top` 1 (the 32-bit Mapped display value): the luma unrounded against
///   the level, summed in f64 over the whole-number weights (`LUM100`), so a
///   grey's luma is the grey itself bit for bit.
///
/// Rec. 709 luma, which this replaced, put 5.3 % of the phase-10 `thrs`
/// picture on the wrong side; the fixed-point form is exact on every
/// sample of take2 at both depths, the half-code ties included, where the
/// `LUM100` rounding of the same luma missed 6 ties of take1. The ONE
/// threshold: the destructive filter and the adjustment layer both call it.
pub(crate) fn threshold(rgb: [f32; 3], level: f32, top: u32) -> f32 {
    let level = f64::from(level.clamp(0.0, 1.0));
    let white = if top == 255 {
        let codes = rgb.map(|v| (f64::from(v.clamp(0.0, 1.0)) * 255.0).round() as u32);
        f64::from(lum_fixed(codes)) >= (level * 255.0).round()
    } else if top == u32::from(u16::MAX) {
        let grid = rgb.map(|v| grid15(v) as u32);
        f64::from(lum_fixed(grid)) >= (level * 32768.0).round()
    } else {
        let sum: f64 = rgb
            .iter()
            .zip(LUM100)
            .map(|(&v, w)| f64::from(w) * f64::from(v))
            .sum();
        sum / 100.0 >= level
    };
    if white {
        1.0
    } else {
        0.0
    }
}

// ------------------------------------------------------- the HSL family --

/// The standard hexcone RGB -> HSL: hue in DEGREES in [0, 360), saturation
/// and lightness in [0, 1]. A neutral pixel has no hue and reports 0 by the
/// usual convention; its callers test for a grey before they read it.
pub(crate) fn rgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let mx = rgb[0].max(rgb[1]).max(rgb[2]);
    let mn = rgb[0].min(rgb[1]).min(rgb[2]);
    let d = mx - mn;
    let l = (mx + mn) / 2.0;
    let s = if d == 0.0 {
        0.0
    } else if l <= 0.5 {
        d / (mx + mn)
    } else {
        d / (2.0 - mx - mn)
    };
    let h = if d == 0.0 {
        0.0
    } else if mx == rgb[0] {
        60.0 * ((rgb[1] - rgb[2]) / d % 6.0)
    } else if mx == rgb[1] {
        60.0 * ((rgb[2] - rgb[0]) / d + 2.0)
    } else {
        60.0 * ((rgb[0] - rgb[1]) / d + 4.0)
    };
    [h.rem_euclid(360.0), s, l]
}

/// The inverse hexcone, HSL -> RGB. The hue is wrapped rather than refused,
/// so a caller may add a signed shift to it and hand the sum straight back.
/// The clamped caller of [`hexcone`]: saturation and lightness are held in
/// [0, 1] and the chroma follows from them.
pub(crate) fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
    let s = hsl[1].clamp(0.0, 1.0);
    let l = hsl[2].clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    hexcone(l, c, hsl[0])
}

/// THE hexcone rebuild: the RGB triple whose `(max + min) / 2` is
/// `lightness`, whose `max - min` is `chroma` and whose hue is `hue` degrees
/// (wrapped). Nothing is clamped and `chroma` may be negative, which swaps
/// the max and the min across the hue: Photoshop's 32-bit Colorize
/// (`adjust_hue_saturation`'s `apply_float`) produces exactly that above
/// lightness 1.
/// [`hsl_to_rgb`] is its clamped caller, so the 8/16-bit HSL path and the
/// 32-bit Colorize share these statements in this order, bit for bit.
pub(crate) fn hexcone(lightness: f32, chroma: f32, hue: f32) -> [f32; 3] {
    let hp = hue.rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        // 5, and the unreachable 6 a hue of exactly 360 would produce if
        // `rem_euclid` ever rounded up to it.
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    [r + m, g + m, b + m]
}

/// The absolute angular distance between two hues in degrees, always in
/// [0, 180] — the short way round the wheel, which is what a hue band's
/// ramp is measured along.
pub(crate) fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

/// The two-sided slider curve Hue/Saturation's Lightness (Photoshop's F-Lm)
/// and Colorize lightness use: `+1` reaches the top of the range, `-1`
/// reaches the bottom, and the two halves meet continuously at 0, where the
/// result is `value` EXACTLY — which is what lets an untouched slider be a
/// byte-exact no-op.
///
/// `bcs`'s saturation is deliberately a DIFFERENT curve (a scale about the
/// luma, `1 + amount`); its meaning is frozen by the ops that already ship.
///
/// The clamped caller of [`two_sided_extended`]: inside [0, 1] that
/// function's hold never fires, so this is the same curve bit for bit.
pub(crate) fn two_sided(value: f32, amount: f32) -> f32 {
    two_sided_extended(value.clamp(0.0, 1.0), amount.clamp(-1.0, 1.0)).clamp(0.0, 1.0)
}

/// THE two-sided curve, with no clamps and Photoshop's 32-bit hold: a
/// value already past the target is left where it is (`amount >= 0` leaves
/// a value above 1 unchanged, `amount < 0` a value below 0), and every
/// other value moves `value + amount·(1 − value)` or `value·(1 + amount)`.
/// The hold is measured, not derived (take3, master Lightness and
/// Colorize's lightness at 32 bits): Photoshop lifts a negative component
/// toward 1 and darkens an over-range one toward 0, but never pulls a
/// component back from beyond the end it is heading for. [`two_sided`] is
/// its clamped caller.
pub(crate) fn two_sided_extended(value: f32, amount: f32) -> f32 {
    if amount >= 0.0 {
        if value > 1.0 {
            value
        } else {
            value + amount * (1.0 - value)
        }
    } else if value < 0.0 {
        value
    } else {
        value * (1.0 + amount)
    }
}

// ------------------------------------------ the hue-wheel decomposition --

/// The six-slot hue wheel's names, in the ONE order the decomposition's
/// slots, `black_and_white`'s weights, `hue_saturation`'s bands and
/// `selective_color`'s first six ranges all use — so the same word never
/// means a different slot in two schema rows.
pub(crate) const HUE_WHEEL: [&str; 6] = ["reds", "yellows", "greens", "cyans", "blues", "magentas"];

/// One pixel split into a neutral part plus AT MOST two adjacent hue
/// components — the model both `black_and_white` and `selective_color`
/// stand on, written once here (`core/CLAUDE.md`'s one-implementation rule).
///
/// `primary` and `secondary` index the six-slot hue wheel in the order both
/// ops name their parameters: 0 reds, 1 yellows, 2 greens, 3 cyans,
/// 4 blues, 5 magentas — so a primary is always even and a secondary always
/// odd. A neutral pixel has both amounts 0 and reports slots 0 and 1, which
/// no caller may read a meaning into.
pub(crate) struct Decomposition {
    /// `min(r, g, b)`: the white part no hue weight may tint.
    pub neutral: f32,
    /// The R/G/B slot holding the maximum (0, 2 or 4).
    pub primary: usize,
    /// `max - mid`: how much of the pixel is that pure primary.
    pub primary_amount: f32,
    /// The C/M/Y slot the two largest channels form (1, 3 or 5).
    pub secondary: usize,
    /// `mid - min`: how much of the pixel is that pure secondary.
    pub secondary_amount: f32,
}

/// Decomposes one straight RGB triple in [0, 1] into [`Decomposition`].
///
/// The secondary is the C/M/Y the two largest channels make — red+green is
/// yellow, green+blue is cyan, red+blue is magenta — and the amounts
/// partition the chroma: `neutral + primary_amount + secondary_amount` is
/// the maximum channel. Ties are broken toward the lower wheel slot so the
/// result is deterministic; where they can occur (`max == mid`) the
/// primary's amount is 0, so the choice cannot change any output.
pub(crate) fn decompose(rgb: [f32; 3]) -> Decomposition {
    // Channel order by value, ties toward the lower index (R, G, B).
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| rgb[b].total_cmp(&rgb[a]).then(a.cmp(&b)));
    let (hi, mid, lo) = (order[0], order[1], order[2]);
    // R -> 0, G -> 2, B -> 4 on the six-slot wheel.
    let primary = hi * 2;
    // The C/M/Y between the two largest channels: R+G = yellow (1),
    // G+B = cyan (3), R+B = magenta (5).
    let secondary = match (hi.min(mid), hi.max(mid)) {
        (0, 1) => 1,
        (1, 2) => 3,
        _ => 5,
    };
    Decomposition {
        neutral: rgb[lo],
        primary,
        primary_amount: rgb[hi] - rgb[mid],
        secondary,
        secondary_amount: rgb[mid] - rgb[lo],
    }
}

// ----------------------------------------------------- the CMYK round trip --

/// Straight RGB in [0, 1] to the naive CMYK `selective_color` works in
/// (there is no profile in an adjustment — see `adjust`'s colour note — so
/// this is the algebraic separation, not a printer's).
///
/// `k = 1 - max(r, g, b)`; the `k >= 1` guard is the divide-by-zero the
/// formula hides: pure black has no ink ratios, and every C/M/Y is 0.
pub(crate) fn rgb_to_cmyk(rgb: [f32; 3]) -> [f32; 4] {
    let k = 1.0 - rgb[0].max(rgb[1]).max(rgb[2]);
    if k >= 1.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let scale = 1.0 - k;
    [
        (1.0 - rgb[0] - k) / scale,
        (1.0 - rgb[1] - k) / scale,
        (1.0 - rgb[2] - k) / scale,
        k,
    ]
}

/// The exact inverse of [`rgb_to_cmyk`] for every triple it can produce.
pub(crate) fn cmyk_to_rgb(cmyk: [f32; 4]) -> [f32; 3] {
    let scale = 1.0 - cmyk[3];
    [
        (1.0 - cmyk[0]) * scale,
        (1.0 - cmyk[1]) * scale,
        (1.0 - cmyk[2]) * scale,
    ]
}
