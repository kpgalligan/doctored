//! The Magnetic Lasso tracer's step cost (`edge_trace`, steps 2 and 4), the
//! rule written in `designs/phase-6-selection-mask-status.md` ("Magnetic
//! Lasso cost rule"): a step of length ℓ along lattice direction `u` into
//! pixel `q` costs `ℓ·(LAMBDA + 1 − A·D·C + TRAIL_PULL·pull)`.
//!
//! - **Edge sample.** The Sobel gradient (the one `ops_filters::sobel_gradient`)
//!   of each of R·a, G·a, B·a and A in unit range, borders replicated; the
//!   pixel's gradient `g` is the FIRST of the four with the largest magnitude,
//!   and its strength `s = min(1, |g|/4)` — a full unit step's Sobel
//!   magnitude is exactly 4 (`1 + 2 + 1`), so a black/white step reads 1.
//!   Colour is weighted by alpha so a colour change under clear pixels is no
//!   edge; alpha is its own channel so a cutout's silhouette attracts.
//! - **Attraction** `A = s/c_max` where `s ≥ t` (Contrast), else 0.
//! - **Direction** `D`: with `φ` the angle between the step and the tangent
//!   (`g` turned 90°), 1 to 22.5°, 0 from 45°, linear in `cos φ` between. The
//!   lattice directions are 45° apart, so every tangent lies within 22.5° of
//!   one and keeps the whole attraction there; a step 45° or more off the
//!   tangent (a diagonal across an axis-aligned edge, a zigzag over hatching)
//!   gains nothing and costs flat ground's price. Linear in the cosine rather
//!   than the angle so the per-pixel work needs no inverse trigonometry.
//! - **Sub-pixel** `C = 1 − SUBPIXEL·o`: `o` is twice the distance, in
//!   samples along the lattice normal nearest `g`, from the pixel centre to
//!   the vertex of the parabola through the strengths at `q − n, q, q + n`,
//!   and 1 wherever the pixel is not a strict-curvature maximum ([`offset`]).
//!
//! Every factor lies in 0…1, so a unit cost stays in `[LAMBDA, LAMBDA + 1 +
//! TRAIL_PULL]`, the range `edge_trace`'s bucket queue relies on.
//!
//! **Vertex placement** ([`place`], the ADR's rule of that name) uses the
//! same parabola, signed ([`crest`]): a path pixel with `s ≥ t` that is a
//! crest along `n` gets its vertex at `centre + δ·n`, the edge its three
//! samples locate; any other keeps its centre. Each coordinate stays inside
//! its pixel's half-open range, so `floor` of a vertex names its pixel. Pure
//! numbers in and out; the samples come from `edge_map`.

use crate::edge_trace::{LAMBDA, TRAIL_PULL};
use crate::ops_filters::sobel_gradient;
use std::f64::consts::{FRAC_1_SQRT_2, SQRT_2};

/// One pixel's edge sample: the winning channel's Sobel gradient `[gx, gy]`
/// (x right, y down).
pub(crate) type Gradient = [f32; 2];

/// The sub-pixel weight: `2·LAMBDA`, so a pixel half a sample off its edge
/// costs as much more as two pixels of perfect path — enough to choose the
/// row the edge runs through, far too little to leave an edge for flat
/// ground (≥ `LAMBDA + 1`).
pub(crate) const SUBPIXEL: f64 = 0.1;

/// Each neighbour slot's lattice axis, in `edge_trace_corridor::STEPS` order
/// (up-left, up, up-right, left, right, down-left, down, down-right): 0
/// horizontal, 1 vertical, 2 the diagonal with `dx·dy > 0`, 3 the other.
pub(crate) const AXIS: [usize; 8] = [2, 1, 3, 0, 0, 3, 1, 2];

/// `cos 22.5°`, where the direction term starts to fall.
const COS_FULL: f64 = 0.923_879_532_511_286_7;
/// `cos 45°`, where it reaches 0.
const COS_NONE: f64 = FRAC_1_SQRT_2;

/// Writes the edge samples of the pixels `x0 .. x0 + out.len()` of row `y`
/// of the straight RGBA8 buffer `raw` (`w × h`). The 3×3 neighbourhood
/// slides along the run, so each step converts one new column of samples
/// rather than nine pixels.
pub(crate) fn gradients(raw: &[u8], w: i64, h: i64, x0: i64, y: i64, out: &mut [Gradient]) {
    // One column's samples at rows y − 1, y, y + 1 per channel (R·a, G·a,
    // B·a, A in unit range), borders replicated.
    let column = |x: i64| {
        let mut out = [[0f32; 3]; 4];
        let px = x.clamp(0, w - 1);
        for (row, dy) in (-1..=1).enumerate() {
            let i = (((y + dy).clamp(0, h - 1) * w + px) * 4) as usize;
            let a = f32::from(raw[i + 3]) / 255.0;
            for (c, plane) in out.iter_mut().enumerate() {
                plane[row] = if c == 3 {
                    a
                } else {
                    f32::from(raw[i + c]) / 255.0 * a
                };
            }
        }
        out
    };
    // Per channel, the three columns x − 1, x, x + 1 of three rows each.
    let mut window = [[[0f32; 3]; 3]; 4];
    let columns = [column(x0 - 1), column(x0), column(x0 + 1)];
    for (c, plane) in window.iter_mut().enumerate() {
        *plane = [columns[0][c], columns[1][c], columns[2][c]];
    }
    for (k, slot) in out.iter_mut().enumerate() {
        if k > 0 {
            let next = column(x0 + k as i64 + 1);
            for (plane, new) in window.iter_mut().zip(next) {
                *plane = [plane[1], plane[2], new];
            }
        }
        // The first channel with the largest magnitude: a strictly larger
        // one replaces it, so ties keep the earlier channel.
        let mut best = ([0f32; 2], -1f32);
        for plane in &window {
            let g = sobel_gradient(|dx, dy| plane[(dx + 1) as usize][(dy + 1) as usize]);
            let magnitude = g[0] * g[0] + g[1] * g[1];
            if magnitude > best.1 {
                best = (g, magnitude);
            }
        }
        *slot = best.0;
    }
}

/// Edge strength `s = min(1, |g|/4)`.
pub(crate) fn strength(g: Gradient) -> f32 {
    ((g[0] * g[0] + g[1] * g[1]).sqrt() / 4.0).min(1.0)
}

/// The lattice step `n` nearest the gradient's line: horizontal when
/// `|gy| ≤ (√2−1)·|gx|` (within 22.5° of it), vertical when
/// `|gx| ≤ (√2−1)·|gy|`, else the diagonal whose sign is that of `gx·gy`.
pub(crate) fn normal(g: Gradient) -> (i64, i64) {
    let (gx, gy) = (f64::from(g[0]), f64::from(g[1]));
    let k = SQRT_2 - 1.0;
    if gy.abs() <= k * gx.abs() {
        (1, 0)
    } else if gx.abs() <= k * gy.abs() {
        (0, 1)
    } else if gx * gy > 0.0 {
        (1, 1)
    } else {
        (1, -1)
    }
}

/// The crest `δ` from the strengths at `q − n`, `q`, `q + n`: where `q` is a
/// maximum (`here` ≥ both) with strictly negative curvature
/// `k = minus − 2·here + plus`, the vertex of the parabola through the three,
/// `(plus − minus) / (−2k)` samples from the centre toward `q + n`; `None`
/// anywhere else (a slope, a plateau). `|plus − minus| ≤ −k` at a maximum, so
/// `δ` already lies in `[−½, ½]`; the clamp only keeps rounding there.
pub(crate) fn crest(minus: f32, here: f32, plus: f32) -> Option<f64> {
    let (minus, here, plus) = (f64::from(minus), f64::from(here), f64::from(plus));
    let curve = minus - 2.0 * here + plus;
    (here >= minus && here >= plus && curve < 0.0)
        .then(|| ((plus - minus) / (-2.0 * curve)).clamp(-0.5, 0.5))
}

/// The sub-pixel offset `o` = `2·|δ|` ([`crest`]), 0…1; where `q` is no
/// crest `o = 1`, as far off as a sample can be.
pub(crate) fn offset(minus: f32, here: f32, plus: f32) -> f64 {
    crest(minus, here, plus).map_or(1.0, |delta| 2.0 * delta.abs())
}

/// A path pixel's vertex ([`place`]).
pub(crate) struct Placed {
    pub(crate) point: [f32; 2],
    /// The pixel located an edge (`s ≥ t` and a [`crest`]); otherwise the
    /// point is the pixel's centre.
    pub(crate) located: bool,
}

/// Where path pixel `(x, y)`'s vertex goes (the ADR's "Vertex placement"):
/// a located edge pixel (`s ≥ t` and a [`crest`] along its lattice normal
/// `n`) at `centre + δ·n`, every other pixel at its centre, each coordinate
/// written inside the pixel's half-open range ([`inside`]). Gradients are
/// read through `at`, which clamps to the image, as in [`edge_terms`].
pub(crate) fn place(x: i64, y: i64, t: f32, mut at: impl FnMut(i64, i64) -> Gradient) -> Placed {
    let gradient = at(x, y);
    let (dx, dy) = normal(gradient);
    let here = strength(gradient);
    let delta = if here >= t {
        let minus = strength(at(x - dx, y - dy));
        let plus = strength(at(x + dx, y + dy));
        crest(minus, here, plus)
    } else {
        None
    };
    let shift = delta.unwrap_or(0.0);
    Placed {
        point: [
            inside(x, x as f64 + 0.5 + shift * dx as f64),
            inside(y, y as f64 + 0.5 + shift * dy as f64),
        ],
        located: delta.is_some(),
    }
}

/// `v` (within pixel `i`'s closed range) as an `f32` in `[i, i + 1)`: a value
/// that reaches `i + 1` — a tie's `+½`, or one just under it rounded up by
/// the narrowing — is written as the largest `f32` below it, so `floor` of
/// the vertex is still `i` and a host may pass it back as `from`. Pixel
/// coordinates stay below 2²⁴, where every integer is an `f32`.
fn inside(i: i64, v: f64) -> f32 {
    let (low, high) = (i as f32, (i + 1) as f32);
    let v = v as f32;
    if v >= high {
        high.next_down()
    } else {
        v.max(low)
    }
}

/// The direction term `D` per lattice axis ([`AXIS`]): `|cos φ|` of the
/// angle between the axis and the tangent `(−gy, gx)/|g|`, mapped to 1 at
/// 22.5° or less, 0 at 45° or more, linearly between. No gradient, no
/// tangent: 0 on every axis.
pub(crate) fn directions(g: Gradient) -> [f64; 4] {
    let (gx, gy) = (f64::from(g[0]), f64::from(g[1]));
    let magnitude = gx.hypot(gy);
    if magnitude == 0.0 {
        return [0.0; 4];
    }
    let along = [
        gy.abs() / magnitude,
        gx.abs() / magnitude,
        (gx - gy).abs() / (SQRT_2 * magnitude),
        (gx + gy).abs() / (SQRT_2 * magnitude),
    ];
    along.map(|c| ((c - COS_NONE) / (COS_FULL - COS_NONE)).clamp(0.0, 1.0))
}

/// One corridor pixel's edge terms.
pub(crate) struct EdgeTerms {
    pub(crate) gradient: Gradient,
    pub(crate) strength: f32,
    /// The sub-pixel offset `o`.
    pub(crate) offset: f64,
}

/// The edge terms of pixel `(x, y)`, reading every gradient it needs through
/// `at`, which clamps its coordinates to the image (the rule's `q ± n`).
pub(crate) fn edge_terms(x: i64, y: i64, mut at: impl FnMut(i64, i64) -> Gradient) -> EdgeTerms {
    let gradient = at(x, y);
    let (dx, dy) = normal(gradient);
    let here = strength(gradient);
    let minus = strength(at(x - dx, y - dy));
    let plus = strength(at(x + dx, y + dy));
    EdgeTerms {
        gradient,
        strength: here,
        offset: offset(minus, here, plus),
    }
}

/// The unit cost of entering a pixel along each lattice axis: `LAMBDA + 1 −
/// A·D·C + TRAIL_PULL·pull`, with `t` the Contrast floor, `c_max` the
/// corridor's strongest `s` and `pull` = `min(d, width)/width`.
pub(crate) fn unit_costs(terms: &EdgeTerms, t: f32, c_max: f32, pull: f64) -> [f64; 4] {
    let attraction = if terms.strength >= t && c_max > 0.0 {
        f64::from(terms.strength) / f64::from(c_max)
    } else {
        0.0
    };
    let centred = 1.0 - SUBPIXEL * terms.offset;
    directions(terms.gradient)
        .map(|direction| LAMBDA + (1.0 - attraction * direction * centred) + TRAIL_PULL * pull)
}
