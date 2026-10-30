//! Independent f64 W3C nonseparable and threshold equations; no product blend
//! helpers. Each blend binary uses its own subset, hence the file-wide allow.
#![allow(dead_code)]
pub fn luminance(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}
fn saturation(c: [f64; 3]) -> f64 {
    c.into_iter().fold(f64::NEG_INFINITY, f64::max) - c.into_iter().fold(f64::INFINITY, f64::min)
}
fn set_saturation(c: [f64; 3], s: f64) -> [f64; 3] {
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| c[*a].total_cmp(&c[*b]));
    let [lo, mid, hi] = order;
    let mut out = [0.; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}
fn set_luminance(c: [f64; 3], l: f64) -> [f64; 3] {
    let delta = l - luminance(c);
    let mut c = c.map(|v| v + delta);
    let luminance = luminance(c);
    let low = c.into_iter().fold(f64::INFINITY, f64::min);
    let high = c.into_iter().fold(f64::NEG_INFINITY, f64::max);
    if low < 0. {
        c = c.map(|v| luminance + (v - luminance) * luminance / (luminance - low));
    }
    if high > 1. {
        c = c.map(|v| luminance + (v - luminance) * (1. - luminance) / (high - luminance));
    }
    c
}
pub fn hsl(mode: i32, base: [f64; 3], source: [f64; 3]) -> [f64; 3] {
    match mode {
        23 => set_luminance(set_saturation(source, saturation(base)), luminance(base)),
        24 => set_luminance(set_saturation(base, saturation(source)), luminance(base)),
        25 => set_luminance(source, luminance(base)),
        26 => set_luminance(base, luminance(source)),
        _ => panic!("test HSLmode"),
    }
}

/// Normalized band half-widths for the v34 threshold policy (direct inputs).
/// Hard Mix: one u16->f32 conversion per operand (<= 2^-25 each) plus one f32
/// sum (<= 2^-24), doubled. Darker/Lighter Color: `lum` is three products and
/// two sums with f32 constants per side (<= 4 * 2^-24 per side), doubled.
/// Dissolve: `checked_alpha` is two f32 products on direct operands (<= 3 *
/// 2^-25) and the 24-bit threshold is exact in f32; the band keeps 2^-20.
pub const HARD_MIX_BAND: f64 = 1. / 2097152.;
pub const COLOR_BAND: f64 = 1. / 1048576.;
pub const DISSOLVE_BAND: f64 = 1. / 1048576.;
/// Hard Mix's exact tie, `|cb + cs - 1| <= 2^-22` (inside `HARD_MIX_BAND`):
/// Photoshop 26.10.0 gives 1 there iff `cb - cs > 1/32768`
/// (`gray_luminance_take.py` take1, HM-ORD).
pub fn hard_mix(cb: f64, cs: f64) -> f64 {
    let sum = cb + cs;
    if (sum - 1.).abs() <= 1. / 4194304. {
        if cb - cs > 1. / 32768. {
            1.
        } else {
            0.
        }
    } else if sum >= 1. {
        1.
    } else {
        0.
    }
}
/// `(B(Cb, Cs), margin)`: mode 21 per channel (`hard_mix`: `cb + cs >= 1` ->
/// 1, an exact tie by order) with margin `|cb + cs - 1|`; modes 16/17 pick the whole triple by luminance
/// (backdrop on a tie) with margin `|lum(cs) - lum(cb)|` in all three slots.
pub fn threshold(mode: i32, base: [f64; 3], source: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    match mode {
        21 => {
            let mut out = [0.; 3];
            let mut margin = [0.; 3];
            for c in 0..3 {
                out[c] = hard_mix(base[c], source[c]);
                margin[c] = (base[c] + source[c] - 1.).abs();
            }
            (out, margin)
        }
        16 | 17 => {
            let (lb, ls) = (luminance(base), luminance(source));
            let pick_source = if mode == 16 { ls < lb } else { ls > lb };
            let margin = (ls - lb).abs();
            (if pick_source { source } else { base }, [margin; 3])
        }
        _ => panic!("test threshold mode"),
    }
}
/// The documented canvas-absolute Dissolve threshold, re-implemented on
/// integers: Murmur3-style mix of `(x, y)` wrapping in u32, low 24 bits.
pub fn dissolve_threshold(x: i64, y: i64) -> f64 {
    let m = 0xFFFF_FFFFu64;
    let mut h = ((x as u32 as u64 * 0x9E37_79B9) & m) + ((y as u32 as u64 * 0x85EB_CA6B) & m);
    h &= m;
    h ^= h >> 16;
    h = (h * 0x85EB_CA6B) & m;
    h ^= h >> 13;
    h = (h * 0xC2B2_AE35) & m;
    h ^= h >> 16;
    (h >> 8) as f64 / 16_777_216.
}

/// Blend If on 16-bit codes (designs/phase-10d-blend-if-status.md): a knee
/// `k` is the code `257 k`, every channel value is rounded to a code before
/// the ramp, and a value whose exact position is within the band of a
/// half-integer may round to the neighbouring code. Derivation, direct
/// inputs: unit rounding <= 2^-25 * 65535 (~0.002 codes) plus the product
/// rounding 2^-8; gray adds three products and two sums (<= 0.016); doubled.
pub const BLEND_IF_BAND: f64 = 1.0 / 32.0;
/// The five-row ramp table over codes, knees at `257 k`, in f64.
pub fn ramp_weight_16(code: f64, ramp: [u8; 4]) -> f64 {
    let [lo0, lo1, hi0, hi1] = ramp.map(|k| f64::from(k) * 257.);
    if code < lo0 {
        0.
    } else if code < lo1 {
        (code - lo0) / (lo1 - lo0)
    } else if code <= hi0 {
        1.
    } else if code <= hi1 {
        (hi1 - code) / (hi1 - hi0)
    } else {
        0.
    }
}
/// `(unrounded code value, margin to the nearest half-integer)` for one side:
/// gray is Photoshop's Gray, `0.30 R + 0.59 G + 0.11 B` of the codes
/// (measured: `scripts/photoshop-probe/style_v1x_take.py`), the others one
/// channel.
pub fn blend_if_channel_16(channel: &str, rgb_codes: [f64; 3]) -> (f64, f64) {
    let value = match channel {
        "gray" => 0.30 * rgb_codes[0] + 0.59 * rgb_codes[1] + 0.11 * rgb_codes[2],
        "red" => rgb_codes[0],
        "green" => rgb_codes[1],
        "blue" => rgb_codes[2],
        other => panic!("unknown Blend If channel {other}"),
    }
    .clamp(0., 65535.);
    let margin = ((value - value.floor()) - 0.5).abs();
    (value, margin)
}
pub struct BlendIfOracle {
    /// The product of both ramps at the rounded codes.
    pub weight: f64,
    /// Every other product reachable when an in-band side rounds to its
    /// neighbouring code (empty when neither side is in band).
    pub alternatives: Vec<f64>,
    /// The smaller of the two sides' margins.
    pub margin: f64,
}
/// The oracle weight for `source` (this layer) and `backdrop` (underlying),
/// both as codes: the rounded code's ramp on each side, plus the alternatives
/// an in-band side admits.
pub fn blend_if_weight_16(
    channel: &str,
    this: [u8; 4],
    under: [u8; 4],
    source: [f64; 3],
    backdrop: [f64; 3],
) -> BlendIfOracle {
    let side = |codes: [f64; 3], ramp: [u8; 4]| {
        let (value, margin) = blend_if_channel_16(channel, codes);
        let primary = value.round();
        let mut weights = vec![ramp_weight_16(primary, ramp)];
        if margin < BLEND_IF_BAND {
            let neighbour = if value - value.floor() >= 0.5 {
                value.floor()
            } else {
                value.ceil()
            };
            weights.push(ramp_weight_16(neighbour, ramp));
        }
        (weights, margin)
    };
    let (this_weights, this_margin) = side(source, this);
    let (under_weights, under_margin) = side(backdrop, under);
    let weight = this_weights[0] * under_weights[0];
    let mut alternatives = Vec::new();
    for (i, a) in this_weights.iter().enumerate() {
        for (j, b) in under_weights.iter().enumerate() {
            let product = a * b;
            if (i, j) != (0, 0) && product != weight && !alternatives.contains(&product) {
                alternatives.push(product);
            }
        }
    }
    BlendIfOracle {
        weight,
        alternatives,
        margin: this_margin.min(under_margin),
    }
}

/// The independent f64 W3C/Photoshop separable blend function `B(cb, cs)` for
/// the twenty modes that have one: 0-13, 15 and 18-22. Written from the
/// compositing-1 pseudocode and Photoshop's documented definitions (and its
/// measured Divide / Vivid Light edges), not from `blend.rs`. Dissolve (14) has no function, and 16/17/23-26 are whole-triple
/// modes (`threshold`/`hsl` above).
pub fn separable(mode: i32, cb: f64, cs: f64) -> f64 {
    fn hard_light(cb: f64, cs: f64) -> f64 {
        if cs <= 0.5 {
            2. * cb * cs
        } else {
            1. - 2. * (1. - cb) * (1. - cs)
        }
    }
    fn dodge(cb: f64, cs: f64) -> f64 {
        if cb == 0. {
            0.
        } else if cs == 1. {
            1.
        } else {
            (cb / (1. - cs)).min(1.)
        }
    }
    fn burn(cb: f64, cs: f64) -> f64 {
        if cb == 1. {
            1.
        } else if cs == 0. {
            0.
        } else {
            1. - ((1. - cb) / cs).min(1.)
        }
    }
    match mode {
        0 => cs,
        1 => cb * cs,
        2 => cb + cs - cb * cs,
        3 => hard_light(cs, cb),
        4 => {
            let d = if cb <= 0.25 {
                ((16. * cb - 12.) * cb + 4.) * cb
            } else {
                cb.sqrt()
            };
            if cs <= 0.5 {
                cb - (1. - 2. * cs) * cb * (1. - cb)
            } else {
                cb + (2. * cs - 1.) * (d - cb)
            }
        }
        5 => hard_light(cb, cs),
        6 => cb.min(cs),
        7 => cb.max(cs),
        8 => (cb - cs).abs(),
        9 => cb + cs - 2. * cb * cs,
        10 => dodge(cb, cs),
        11 => burn(cb, cs),
        12 => (cb + cs).min(1.),
        13 => (cb - cs).max(0.),
        15 => (cb + cs - 1.).max(0.),
        // Vivid Light's halves take a black source to 0 and a white one to 1
        // before the backdrop's ends, and Divide keeps 0 at 0 / 0 (measured
        // against Photoshop, `scripts/photoshop-probe/blend_edge_take.py` take1).
        18 => {
            if cs == 0. {
                0.
            } else if cs == 1. {
                1.
            } else if cs <= 0.5 {
                burn(cb, 2. * cs)
            } else {
                dodge(cb, 2. * cs - 1.)
            }
        }
        19 => (cb + 2. * cs - 1.).clamp(0., 1.),
        20 => {
            if cs <= 0.5 {
                cb.min(2. * cs)
            } else {
                cb.max(2. * cs - 1.)
            }
        }
        21 => hard_mix(cb, cs),
        22 => {
            if cs == 0. && cb == 0. {
                0.
            } else if cs == 0. {
                1.
            } else {
                (cb / cs).min(1.)
            }
        }
        _ => panic!("test separable mode {mode}"),
    }
}
