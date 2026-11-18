//! #2: the Layer Style Center Stroke on an anti-aliased shape, against a
//! brute-force oracle written from the rule — the stroke's two halves and
//! the layer join as a DISJOINT UNION of the pixel. Under a pixel of shape
//! byte `c` (as a unit), with `outer` and `inner` the two half bands as
//! units, Fill `f` and stroke opacity `o`, the composite over the stack `B`
//! is `B + (outer + inner)·o·(R − B) + (c − inner)·f·(S − B)`: the stroke
//! holds `(outer + inner)·o` of the pixel, the layer what the inner half
//! leaves of its own coverage, the stack the rest. Rendered at 8, 16 and 32
//! bits through `identify_578::render_rgb_alpha` (a #00ff00 layer over
//! opaque #0000ff, a #ff0000 Normal Center stroke), so a pixel's red, green
//! and blue codes are the three shares × 255.
//!
//! The bands are brute force in f64 on the padded byte plane, from their
//! definitions (`style_stroke_distance`'s module doc): the inner half NA1,
//! `max(c − round(255 · clamp01(D − n)), 0)` with `D = min_q (c_q / 255 +
//! chamfer(q, p))` over `c_q ≤ 254`; the outer half A1K0, `grown =
//! round(255 · clamp01(n + 1 − D))` with `D = min_q (1 − c_q / 255 +
//! chamfer(q, p))` over `c_q ≥ 1`, knocked out by the shape, `(grown (255 −
//! c) + 127) / 255`; `n` is half the Size. At a half-integer `n` a straight
//! edge's distance puts the rounding exactly on a byte's half, where the
//! kernel's f32 and this f64 may round apart, so there either byte is the
//! oracle's.
//!
//! And against Photoshop's own Center Stroke take
//! (`scripts/photoshop-probe/stroke_center_2_take.py`, take1, Photoshop
//! 26.10.0; private archive): `stroke_center2_profiles.json`
//! (`tests/fixtures/psd_photoshop/identify_578/`, README), its 288 rows on
//! four pictures no earlier take showed (`identify_578::center2_coverage`),
//! Size 1 – 10, Fill 100 / 50 / 0, stroke opacity 100 / 50, 8 and 16 bits,
//! profiled as the runner profiles them and scored over its gate by
//! `style_stroke_aa_parity::check_rows` (bound 2 / 0.5, declared before the
//! take). A row that misses asserts its own measured number, never a wider
//! one. At Size 1 the two halves leave part of a partial pixel uncovered,
//! and the take keeps a share of the layer there, `c·f·(1 − t)` with `t` the
//! two halves' coverage, which the union above gives no share: those rows
//! are pinned.

use serde_json::{json, Value};

use crate::common::byte_unit;
use crate::identify_578::{center2_bin, center2_coverage, render_rgb_alpha};
use crate::style_stroke_aa_parity::{
    chamfer, check_rows, oracle_disc, oracle_edge, oracle_ramp, score, PAD,
};

const SOURCE: [f64; 3] = [0.0, 1.0, 0.0];
const BACKDROP: [f64; 3] = [0.0, 0.0, 1.0];
const STROKE: [f64; 3] = [1.0, 0.0, 0.0];

fn center_style(size: u32, fill: f64, opacity: f64) -> Value {
    json!({"fill_opacity":fill,"effects":[{"type":"stroke","size":size,"position":"center",
        "color":"#ff0000","opacity":opacity}]})
}

/// A byte from the f64 value `v` (already × 255): both neighbours where `v`
/// sits on a half (module doc), else the nearest.
fn byte_candidates(v: f64) -> [f64; 2] {
    if (v - v.floor() - 0.5).abs() < 1e-6 {
        [v.floor(), v.floor() + 1.0]
    } else {
        [v.round(); 2]
    }
}

/// The least `seed(c_q) + chamfer(q, p)` over the pixels `q` `keep` admits,
/// for every pixel `p` of the padded plane (module doc).
fn brute_distance(
    c: &[u8],
    pw: usize,
    keep: impl Fn(u8) -> bool,
    seed: impl Fn(u8) -> f64,
) -> Vec<f64> {
    let seeds: Vec<(i64, i64, f64)> = (0..c.len())
        .filter(|&i| keep(c[i]))
        .map(|i| ((i % pw) as i64, (i / pw) as i64, seed(c[i])))
        .collect();
    (0..c.len())
        .map(|i| {
            let (px, py) = ((i % pw) as i64, (i / pw) as i64);
            seeds
                .iter()
                .map(|&(qx, qy, s)| s + chamfer(qx - px, qy - py))
                .fold(f64::INFINITY, f64::min)
        })
        .collect()
}

/// The two halves' bytes at `n` on the padded plane `c`, each as its
/// candidates (module doc): `(inner, outer)` per pixel. The integer-`n`
/// NA1 of `style_stroke_aa_parity::oracle_band` is the master copy of the
/// inner half; this one adds the half-integer `n`'s ties.
fn half_bands(c: &[u8], pw: usize, n: f64) -> Vec<([f64; 2], [f64; 2])> {
    let to_inside = brute_distance(c, pw, |v| v <= 254, |v| f64::from(v) / 255.0);
    let to_shape = brute_distance(c, pw, |v| v >= 1, |v| 1.0 - f64::from(v) / 255.0);
    (0..c.len())
        .map(|i| {
            let cov = f64::from(c[i]);
            let cut = byte_candidates(255.0 * (to_inside[i] - n).clamp(0.0, 1.0));
            let inner = cut.map(|k| (cov - k).max(0.0));
            let grown = byte_candidates(255.0 * (n + 1.0 - to_shape[i]).clamp(0.0, 1.0));
            let outer = grown.map(|g| ((g * (255.0 - cov) + 127.0) / 255.0).floor());
            (inner, outer)
        })
        .collect()
}

/// The disjoint union's codes per channel (module doc) for one pixel of
/// shape byte `cov` with the half bands `inner` and `outer` (bytes).
fn union_codes(cov: f64, inner: f64, outer: f64, f: f64, o: f64) -> [f64; 3] {
    let inner = inner.min(cov) / 255.0;
    let stroke = (outer / 255.0 + inner) * o;
    let layer = (cov / 255.0 - inner) * f;
    [0, 1, 2].map(|k| {
        255.0
            * (BACKDROP[k] + stroke * (STROKE[k] - BACKDROP[k]) + layer * (SOURCE[k] - BACKDROP[k]))
    })
}

/// The worst pixel of `got` against the oracle: per pixel the least, over
/// the band candidates, of the largest channel difference.
fn worst_against_oracle(
    w: u32,
    h: u32,
    alpha: &[u8],
    bands: &[([f64; 2], [f64; 2])],
    got: &[[f64; 3]],
    f: f64,
    o: f64,
) -> (f64, (u32, u32)) {
    let (wu, hu) = (w as usize, h as usize);
    let pw = wu + 2 * PAD;
    let mut worst = (0.0f64, (0, 0));
    for y in 0..hu {
        for x in 0..wu {
            let cov = f64::from(alpha[y * wu + x]);
            let (inner, outer) = bands[(y + PAD) * pw + x + PAD];
            let g = got[y * wu + x];
            let e = inner
                .iter()
                .flat_map(|&i| outer.iter().map(move |&b| (i, b)))
                .map(|(i, b)| {
                    let want = union_codes(cov, i, b, f, o);
                    (0..3)
                        .map(|k| (g[k] - want[k]).abs())
                        .fold(0.0f64, f64::max)
                })
                .fold(f64::INFINITY, f64::min);
            if e > worst.0 {
                worst = (e, (x as u32, y as u32));
            }
        }
    }
    worst
}

#[test]
fn the_center_stroke_joins_its_halves_and_the_layer_as_a_disjoint_union() {
    let pictures = [
        ("disc", oracle_disc()),
        ("edge", oracle_edge()),
        ("ramp", oracle_ramp()),
    ];
    let mut failures = Vec::new();
    let mut compared = 0usize;
    for (label, (w, h, alpha)) in &pictures {
        let (wu, hu) = (*w as usize, *h as usize);
        let (pw, ph) = (wu + 2 * PAD, hu + 2 * PAD);
        let mut padded = vec![0u8; pw * ph];
        for y in 0..hu {
            for x in 0..wu {
                padded[(y + PAD) * pw + x + PAD] = alpha[y * wu + x];
            }
        }
        let unit: Vec<f64> = alpha.iter().map(|&v| f64::from(v) / 255.0).collect();
        for size in [3u32, 4] {
            let band = half_bands(&padded, pw, f64::from(size) / 2.0);
            for fill in [1.0f64, 0.5, 0.0] {
                for opacity in [1.0f64, 0.5] {
                    let s = center_style(size, fill, opacity);
                    for depth in [8u32, 16, 32] {
                        // The Fill is a byte below 32 bits (`opacity_byte`);
                        // the stroke's opacity never is.
                        let f = if depth == 32 { fill } else { byte_unit(fill) };
                        let got = render_rgb_alpha(depth, *w, *h, &unit, SOURCE, BACKDROP, &s);
                        let (worst, at) =
                            worst_against_oracle(*w, *h, alpha, &band, &got, f, opacity);
                        compared += 1;
                        if worst > 1.0 {
                            failures.push(format!(
                                "{label} size {size} fill {fill} opacity {opacity} at {depth} \
                                 bits: worst {worst:.3} at {at:?}"
                            ));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(compared, 3 * 2 * 3 * 2 * 3);
    assert!(failures.is_empty(), "{failures:#?}");
}

// ------------------------------------------------------- the take's rows --

/// The rows each half shape's band averages (`stroke_center_2_take.BAND_ROWS`).
const BAND_ROWS: [(u32, u32); 4] = [(80, 144), (240, 304), (400, 464), (560, 624)];

/// The Center Stroke take's profile of a render, r | g | b
/// (`stroke_center_2_take.profile`): the half shapes' column means over each
/// band's rows at x = 224..=287, the curved shapes' mean per bin
/// `floor(−sdf)` (`identify_578::center2_bin`), −8 ... 15.
fn profile_center2(shape: &str, w: u32, h: u32, codes: &[[f64; 3]]) -> Vec<f64> {
    let half = matches!(shape, "cnhalf" | "cnsoft");
    let bins: Vec<i32> = if half {
        Vec::new()
    } else {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| center2_bin(shape, x, y))
            .collect()
    };
    let mut out = Vec::new();
    for k in 0..3 {
        if half {
            for &(y0, y1) in &BAND_ROWS {
                for x in 224..288u32 {
                    let total: f64 = (y0..y1).map(|y| codes[(y * w + x) as usize][k]).sum();
                    out.push(total / f64::from(y1 - y0));
                }
            }
            continue;
        }
        for b in -8..=15 {
            let (mut total, mut n) = (0.0, 0usize);
            for (p, &bin) in codes.iter().zip(&bins) {
                if bin == b {
                    total += p[k];
                    n += 1;
                }
            }
            assert!(n > 0, "{shape}: bin {b} is empty");
            out.push(total / n as f64);
        }
    }
    out
}

/// `(max, mean)` of one take row, parsed from its name
/// (`sc1_<shape>_s<S>_f<F>_o<O>_<d>`).
fn measure_center2(name: &str, observed: &Value) -> (f64, f64) {
    let parts: Vec<&str> = name.split('_').collect();
    assert_eq!(parts.len(), 6, "{name}: a Center Stroke take row");
    let num = |s: &str| {
        s[1..]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let (shape, size, fill, opacity) = (parts[1], num(parts[2]), num(parts[3]), num(parts[4]));
    let depth: u32 = parts[5].parse().expect("depth");
    let (w, h, a) = center2_coverage(shape);
    let s = center_style(size, f64::from(fill) / 100.0, f64::from(opacity) / 100.0);
    let codes = render_rgb_alpha(depth, w, h, &a, SOURCE, BACKDROP, &s);
    score(name, &profile_center2(shape, w, h, &codes), observed)
}

// Measured on the shipped kernel against the take, max and mean in codes
// rounded up in the third decimal: every row here misses 2 / 0.5. All are
// the Size 1 rows, where the two halves leave part of a partial pixel and
// the take keeps a share of the layer there (module doc); every row of Size
// 2, 3, 5, 6 and 10 meets.
const PINNED: &[(&str, f64, f64)] = &[
    ("sc1_cndisc23_s1_f0_o100_16", 5.348, 0.179),
    ("sc1_cndisc23_s1_f0_o100_8", 5.346, 0.174),
    ("sc1_cndisc23_s1_f0_o50_16", 2.444, 0.093),
    ("sc1_cndisc23_s1_f0_o50_8", 2.782, 0.088),
    ("sc1_cndisc23_s1_f100_o100_16", 6.853, 0.346),
    ("sc1_cndisc23_s1_f100_o100_8", 6.838, 0.339),
    ("sc1_cndisc23_s1_f100_o50_16", 6.853, 0.345),
    ("sc1_cndisc23_s1_f100_o50_8", 6.838, 0.338),
    ("sc1_cndisc23_s1_f50_o100_16", 5.348, 0.253),
    ("sc1_cndisc23_s1_f50_o100_8", 5.346, 0.234),
    ("sc1_cndisc23_s1_f50_o50_16", 3.442, 0.178),
    ("sc1_cndisc23_s1_f50_o50_8", 3.043, 0.164),
    ("sc1_cnhalf_s1_f0_o100_16", 15.001, 0.162),
    ("sc1_cnhalf_s1_f0_o100_8", 15.001, 0.157),
    ("sc1_cnhalf_s1_f0_o50_16", 7.148, 0.086),
    ("sc1_cnhalf_s1_f0_o50_8", 7.001, 0.084),
    ("sc1_cnhalf_s1_f100_o100_16", 16.199, 0.295),
    ("sc1_cnhalf_s1_f100_o100_8", 16.001, 0.287),
    ("sc1_cnhalf_s1_f100_o50_16", 16.199, 0.294),
    ("sc1_cnhalf_s1_f100_o50_8", 16.001, 0.284),
    ("sc1_cnhalf_s1_f50_o100_16", 15.001, 0.225),
    ("sc1_cnhalf_s1_f50_o100_8", 15.001, 0.219),
    ("sc1_cnhalf_s1_f50_o50_16", 8.129, 0.155),
    ("sc1_cnhalf_s1_f50_o50_8", 9.001, 0.152),
    ("sc1_cnrrect_s1_f0_o100_16", 5.500, 0.172),
    ("sc1_cnrrect_s1_f0_o100_8", 5.501, 0.167),
    ("sc1_cnrrect_s1_f0_o50_16", 2.581, 0.093),
    ("sc1_cnrrect_s1_f0_o50_8", 2.547, 0.084),
    ("sc1_cnrrect_s1_f100_o100_16", 5.897, 0.220),
    ("sc1_cnrrect_s1_f100_o100_8", 5.969, 0.215),
    ("sc1_cnrrect_s1_f100_o50_16", 5.897, 0.220),
    ("sc1_cnrrect_s1_f100_o50_8", 5.969, 0.211),
    ("sc1_cnrrect_s1_f50_o100_16", 5.500, 0.186),
    ("sc1_cnrrect_s1_f50_o100_8", 5.501, 0.171),
    ("sc1_cnrrect_s1_f50_o50_16", 2.960, 0.115),
    ("sc1_cnrrect_s1_f50_o50_8", 2.985, 0.101),
    ("sc1_cnsoft_s1_f0_o100_16", 16.004, 0.359),
    ("sc1_cnsoft_s1_f0_o100_8", 16.001, 0.355),
    ("sc1_cnsoft_s1_f0_o50_16", 7.627, 0.193),
    ("sc1_cnsoft_s1_f0_o50_8", 8.001, 0.188),
    ("sc1_cnsoft_s1_f100_o100_16", 16.829, 0.691),
    ("sc1_cnsoft_s1_f100_o100_8", 17.001, 0.688),
    ("sc1_cnsoft_s1_f100_o50_16", 16.829, 0.696),
    ("sc1_cnsoft_s1_f100_o50_8", 17.001, 0.688),
    ("sc1_cnsoft_s1_f50_o100_16", 16.004, 0.517),
    ("sc1_cnsoft_s1_f50_o100_8", 16.001, 0.501),
    ("sc1_cnsoft_s1_f50_o50_16", 8.448, 0.359),
    ("sc1_cnsoft_s1_f50_o50_8", 8.001, 0.334),
];

#[test]
fn every_center_take_row_meets_the_bound_or_its_measured_residual() {
    check_rows("stroke_center2_profiles.json", 288, PINNED, measure_center2);
}
