//! #2: the Inner Glow law's band measured from the anti-aliased contour
//! (`style_aa_band`), against Photoshop's own identify-578 take4 profiles at
//! 8 and 16 bits, against a brute-force oracle, and across 8, 16 and 32
//! bits. The fixture (`tests/fixtures/psd_photoshop/identify_578/`, README)
//! is `take4_inner_profiles.json`: the 144 take4 rows the law runs on (Choke
//! 0, 50 and 80 %, both sources), written by
//! `scripts/photoshop-probe/glow_aa_578_law_fit.py --fixtures-inner` on its
//! INNER JUDGE SHIP for `La/edge` and `La/center` (`Ma`, Choke 100 %, did not
//! ship, so no Choke 100 % row is here).
//!
//! Each row is rebuilt as `identify_578_take4.py` builds it (`alpha_of`): a
//! black layer over opaque black whose alpha is `floor(a · full + 0.5)` of
//! the coverage `a` (`aahalf`: 320 × 544, the rect x in [32, 256), y in [32,
//! 512), its x = 256 column at 0.25 / 0.5 / 0.75 over y [32, 192) / [192,
//! 352) / [352, 512); `aadisc`: 160², the 8 × 8 supersampled r = 40 disc at
//! (80, 80)), with a white Normal 100 % Inner Glow of the row's source, Size,
//! Choke and Range. The profile is the take's (`I4.profile`) on the red
//! channel in 8-bit codes (16-bit samples / 257): `aahalf` the column means
//! over each coverage's `BAND_ROWS` at x = 128..=319, concatenated 25, 50,
//! 75; `aadisc` the mean of every pixel per bin `floor(40 − dist)`, -32 ...
//! 39. A row's error is the max and the mean of `|ours − Photoshop|` over the
//! whole profile.
//!
//! The bound, declared in the runner before the take, is max <= 2 and mean
//! <= 0.5. A row that misses it asserts its own measured number (`PINNED`,
//! rounded up in the third decimal), never a wider one, and that it still
//! misses.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::common::byte_unit;
use crate::identify_578::{
    profile2, render16_alpha, render32_alpha, render8_alpha, supersampled, take4_coverage,
};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

/// The rows each take4 `aahalf` band's profile averages (`I4.BAND_ROWS`).
const BAND_ROWS: [(u32, u32); 3] = [(80, 144), (240, 304), (400, 464)];

/// One take4 row, parsed from its name
/// (`ig4_<shape>_<e|c>_s<S>_c<C>_r<R>_<d>`).
struct Row {
    shape: String,
    source: &'static str,
    size: u32,
    choke: u32,
    range: u32,
    depth: u32,
}

fn parse(name: &str) -> Row {
    let parts: Vec<&str> = name.split('_').collect();
    assert_eq!(parts.len(), 7, "{name}: a take4 inner row");
    let num = |s: &str| {
        s[1..]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let source = match parts[2] {
        "e" => "edge",
        "c" => "center",
        other => panic!("{name}: unknown source {other}"),
    };
    Row {
        shape: parts[1].to_string(),
        source,
        size: num(parts[3]),
        choke: num(parts[4]),
        range: num(parts[5]),
        depth: parts[6].parse().expect("depth"),
    }
}

fn style(source: &str, size: u32, choke: u32, range: u32) -> Value {
    json!({"effects":[{"type":"inner_glow","blend":"normal","color":"#ffffff","opacity":1,
        "size":size,"choke":f64::from(choke) / 100.0,"range":f64::from(range) / 100.0,
        "source":source,"anti_aliased":false}]})
}

fn alpha8(a: &[f64]) -> Vec<u8> {
    a.iter().map(|&v| (v * 255.0 + 0.5).floor() as u8).collect()
}

fn alpha16(a: &[f64]) -> Vec<u16> {
    a.iter()
        .map(|&v| (v * 65535.0 + 0.5).floor() as u16)
        .collect()
}

/// The take's profile (`I4.profile`) of a render, in codes.
fn profile(shape: &str, w: u32, codes: &[f64]) -> Vec<f64> {
    let at = |x: u32, y: u32| codes[(y * w + x) as usize];
    match shape {
        "aahalf" => BAND_ROWS
            .iter()
            .flat_map(|&(y0, y1)| {
                (128..320).map(move |x| (y0..y1).map(|y| at(x, y)).sum::<f64>() / 64.0)
            })
            .collect(),
        _ => profile2("disc", w, codes),
    }
}

/// `(max, mean)` of one row in 8-bit codes over its whole profile.
fn measure(name: &str, observed: &Value) -> (f64, f64) {
    let row = parse(name);
    let (w, h, a) = take4_coverage(&row.shape);
    let s = style(row.source, row.size, row.choke, row.range);
    let codes = if row.depth == 8 {
        render8_alpha(w, h, &alpha8(&a), &s)
    } else {
        render16_alpha(w, h, &alpha16(&a), &s)
    };
    let ours = profile(&row.shape, w, &codes);
    let ps: Vec<f64> = observed["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(|v| v.as_f64().expect("value"))
        .collect();
    assert_eq!(ours.len(), ps.len(), "{name}: the profile's length");
    let (mut worst, mut total) = (0.0f64, 0.0);
    for (o, p) in ours.iter().zip(&ps) {
        let e = (o - p).abs();
        worst = worst.max(e);
        total += e;
    }
    (worst, total / ps.len() as f64)
}

// Measured on the shipped kernel against take4, max and mean in codes
// rounded up in the third decimal: every row here misses 2 / 0.5. They are
// all the disc at Choke 50 and 80 %: INNER JUDGE's bin at Size 4 and 32 (no
// declared form reads the curved contour, the runner's DESCRIPTION: THE
// DISC) and its FIT rows at Size 12. Every half-plane row and every Choke 0
// row meets.
const PINNED: &[(&str, f64, f64)] = &[
    ("ig4_aadisc_c_s12_c50_r50_16", 4.184, 0.196),
    ("ig4_aadisc_c_s12_c50_r50_8", 4.177, 0.195),
    ("ig4_aadisc_c_s12_c80_r100_16", 8.481, 0.272),
    ("ig4_aadisc_c_s12_c80_r100_8", 8.490, 0.270),
    ("ig4_aadisc_c_s12_c80_r50_16", 14.603, 0.291),
    ("ig4_aadisc_c_s12_c80_r50_8", 14.457, 0.288),
    ("ig4_aadisc_c_s32_c50_r50_16", 3.416, 0.397),
    ("ig4_aadisc_c_s32_c50_r50_8", 3.425, 0.393),
    ("ig4_aadisc_c_s32_c80_r100_16", 4.203, 0.371),
    ("ig4_aadisc_c_s32_c80_r100_8", 4.158, 0.373),
    ("ig4_aadisc_c_s32_c80_r50_16", 8.097, 0.414),
    ("ig4_aadisc_c_s32_c80_r50_8", 8.211, 0.419),
    ("ig4_aadisc_c_s4_c80_r100_16", 2.315, 0.081),
    ("ig4_aadisc_c_s4_c80_r100_8", 2.394, 0.081),
    ("ig4_aadisc_c_s4_c80_r50_16", 5.062, 0.102),
    ("ig4_aadisc_c_s4_c80_r50_8", 5.005, 0.100),
    ("ig4_aadisc_e_s12_c50_r50_16", 4.192, 0.200),
    ("ig4_aadisc_e_s12_c50_r50_8", 4.177, 0.196),
    ("ig4_aadisc_e_s12_c80_r100_16", 8.581, 0.275),
    ("ig4_aadisc_e_s12_c80_r100_8", 8.460, 0.270),
    ("ig4_aadisc_e_s12_c80_r50_16", 14.617, 0.292),
    ("ig4_aadisc_e_s12_c80_r50_8", 14.462, 0.288),
    ("ig4_aadisc_e_s32_c50_r50_16", 3.424, 0.405),
    ("ig4_aadisc_e_s32_c50_r50_8", 3.425, 0.402),
    ("ig4_aadisc_e_s32_c80_r100_16", 4.211, 0.377),
    ("ig4_aadisc_e_s32_c80_r100_8", 4.158, 0.373),
    ("ig4_aadisc_e_s32_c80_r50_16", 8.105, 0.422),
    ("ig4_aadisc_e_s32_c80_r50_8", 8.211, 0.419),
    ("ig4_aadisc_e_s4_c80_r100_16", 2.351, 0.082),
    ("ig4_aadisc_e_s4_c80_r100_8", 2.324, 0.079),
    ("ig4_aadisc_e_s4_c80_r50_16", 5.168, 0.104),
    ("ig4_aadisc_e_s4_c80_r50_8", 5.103, 0.102),
];

#[test]
fn every_take4_row_meets_the_bound_or_its_measured_residual() {
    let file = "take4_inner_profiles.json";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/identify_578")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    let rows: BTreeMap<String, Value> = serde_json::from_str(&text).expect("json");
    assert_eq!(rows.len(), 144, "{file}: 144 rows");
    assert!(
        rows.keys().all(|k| !k.contains("_c100_")),
        "{file}: no Choke 100 % row"
    );
    for (name, ..) in PINNED {
        assert!(
            rows.contains_key(*name),
            "{name}: pinned but not in the fixture"
        );
    }
    let mut failures = Vec::new();
    for (name, observed) in &rows {
        let (max, mean) = measure(name, observed);
        eprintln!("{name}: max {max:.6} mean {mean:.6}");
        match PINNED.iter().find(|p| p.0 == name.as_str()) {
            None => {
                if !(max <= MAX && mean <= MEAN) {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} misses 2 / 0.5"
                    ));
                }
            }
            Some((_, max_pin, mean_pin)) => {
                if !(max <= *max_pin && mean <= *mean_pin) {
                    failures.push(format!(
                        "{name}: max {max:.6} (pinned {max_pin}), mean {mean:.6} (pinned \
                         {mean_pin})"
                    ));
                }
                if max <= MAX && mean <= MEAN {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} now meets 2 / 0.5: unpin it"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// ------------------------------------------------------------- oracle --

/// The oracle's margin around the canvas, past every reach these sizes have
/// (`n + 1 + w − 1 <= 5` at Size 4), so both box passes clamp onto the
/// transparent pad as the kernel's padded plane does.
const PAD: usize = 8;

/// A 40 × 32 supersampled r = 9 disc at (20, 16).
fn oracle_disc() -> (u32, u32, Vec<u8>) {
    let a = supersampled(40, 32, |x, y| {
        (x - 20.0).powi(2) + (y - 16.0).powi(2) <= 81.0
    });
    (40, 32, alpha8(&a))
}

/// A 40 × 32 straight edge with three levels: the rect x in [4, 20), y in
/// [4, 28) opaque, its x = 20 column at 64 / 128 / 192 over y [4, 12) /
/// [12, 20) / [20, 28).
fn oracle_edge() -> (u32, u32, Vec<u8>) {
    let (w, h) = (40u32, 32u32);
    let a = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| match (x, y) {
            (4..=19, 4..=27) => 255,
            (20, 4..=11) => 64,
            (20, 12..=19) => 128,
            (20, 20..=27) => 192,
            _ => 0,
        })
        .collect();
    (w, h, a)
}

/// A 48 × 32 rect whose alpha ramps linearly over 12 px: the outer rect [2,
/// 46) × [2, 30), alpha `clamp(m / 12)` with `m` the pixel centre's distance
/// inside its nearest side, so the core is opaque and the margin
/// transparent.
fn oracle_ramp() -> (u32, u32, Vec<u8>) {
    let (w, h) = (48u32, 32u32);
    let a: Vec<f64> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (cx, cy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let m = (cx - 2.0).min(46.0 - cx).min(cy - 2.0).min(30.0 - cy);
            (m / 12.0).clamp(0.0, 1.0)
        })
        .collect();
    (w, h, alpha8(&a))
}

/// The law's `(n, w)`: `n = floor(S · p + 0.5)`, `w = S − n`, 1 acting as 2.
fn split(size: u32, choke: f64) -> (usize, usize) {
    let n = (f64::from(size) * choke + 0.5).floor() as usize;
    let w = size as usize - n;
    (n, if w == 1 { 2 } else { w })
}

/// The band of the anti-aliased contour, brute force in f64 on the padded
/// coverage `u = 1 − a / 255` (transparent outside the canvas): `b_full =
/// clamp(n + 1 − d(p, {u = 1}))`, `b_part = clamp(n − min(|p − q| − u_q))`
/// over the partial q within `n + 1`, their max.
fn oracle_band(u: &[f64], pw: usize, ph: usize, n: usize) -> Vec<f64> {
    let reach = n as i64 + 1;
    let nf = n as f64;
    let mut out = vec![0.0; pw * ph];
    for py in 0..ph as i64 {
        for px in 0..pw as i64 {
            let (mut d_full, mut d_part) = (f64::INFINITY, f64::INFINITY);
            for qy in (py - reach).max(0)..=(py + reach).min(ph as i64 - 1) {
                for qx in (px - reach).max(0)..=(px + reach).min(pw as i64 - 1) {
                    let dist = ((qx - px) as f64).hypot((qy - py) as f64);
                    let uq = u[qy as usize * pw + qx as usize];
                    if uq >= 1.0 {
                        d_full = d_full.min(dist);
                    } else if uq > 0.0 && dist <= nf + 1.0 {
                        d_part = d_part.min(dist - uq);
                    }
                }
            }
            let full = (nf + 1.0 - d_full).clamp(0.0, 1.0);
            let part = if d_part.is_finite() {
                (nf - d_part).clamp(0.0, 1.0)
            } else {
                0.0
            };
            out[py as usize * pw + px as usize] = full.max(part);
        }
    }
    out
}

/// One edge-clamped triangle pass `(w − |j|) / w²` along an axis.
fn triangle(x: &[f64], pw: usize, ph: usize, w: usize, horizontal: bool) -> Vec<f64> {
    let k = w as i64 - 1;
    let mut out = vec![0.0; pw * ph];
    for y in 0..ph as i64 {
        for xx in 0..pw as i64 {
            let mut sum = 0.0;
            for j in -k..=k {
                let (sx, sy) = if horizontal {
                    ((xx + j).clamp(0, pw as i64 - 1), y)
                } else {
                    (xx, (y + j).clamp(0, ph as i64 - 1))
                };
                sum += (w as f64 - j.abs() as f64) * x[sy as usize * pw + sx as usize];
            }
            out[y as usize * pw + xx as usize] = sum / (w * w) as f64;
        }
    }
    out
}

/// The oracle's red codes of the canvas: band, box pair, Range, mirror, one
/// rounding to the glow byte, then the glow byte times the coverage byte
/// (one rounding) at the byte opacity.
fn oracle_codes(
    w: u32,
    h: u32,
    alpha: &[u8],
    source: &str,
    size: u32,
    choke: f64,
    range: f64,
) -> Vec<f64> {
    let (wu, hu) = (w as usize, h as usize);
    let (pw, ph) = (wu + 2 * PAD, hu + 2 * PAD);
    let mut u = vec![1.0; pw * ph];
    for y in 0..hu {
        for x in 0..wu {
            u[(y + PAD) * pw + x + PAD] = 1.0 - f64::from(alpha[y * wu + x]) / 255.0;
        }
    }
    let (n, bw) = split(size, choke);
    let band = oracle_band(&u, pw, ph, n);
    let b = if bw > 0 {
        let first = triangle(&band, pw, ph, bw, true);
        triangle(&first, pw, ph, bw, false)
    } else {
        band
    };
    let opacity = byte_unit(1.0);
    let mut out = Vec::with_capacity(wu * hu);
    for y in 0..hu {
        for x in 0..wu {
            let e = (b[(y + PAD) * pw + x + PAD] / range).min(1.0);
            let g = if source == "center" { 1.0 - e } else { e };
            let glow = (255.0 * g + 0.5).floor();
            let plane = (glow * f64::from(alpha[y * wu + x]) / 255.0).round();
            out.push(plane * opacity);
        }
    }
    out
}

#[test]
fn the_aa_band_matches_a_brute_force_oracle() {
    // Choke 0 runs today's band (`aa(0) = u`), Choke 50 % the anti-aliased
    // one; Choke 100 % (`Ma`) did not ship and is not the law.
    let pictures = [
        ("disc", oracle_disc()),
        ("edge", oracle_edge()),
        ("ramp", oracle_ramp()),
    ];
    let mut failures = Vec::new();
    let mut compared = 0usize;
    for (label, (w, h, alpha)) in &pictures {
        for source in ["edge", "center"] {
            for size in [1u32, 3, 4] {
                for choke in [0.0f64, 0.5] {
                    for range in [0.5f64, 1.0] {
                        let s = json!({"effects":[{"type":"inner_glow","blend":"normal",
                            "color":"#ffffff","opacity":1,"size":size,"choke":choke,
                            "range":range,"source":source,"anti_aliased":false}]});
                        let got = render8_alpha(*w, *h, alpha, &s);
                        let want = oracle_codes(*w, *h, alpha, source, size, choke, range);
                        let worst = got
                            .iter()
                            .zip(&want)
                            .map(|(g, o)| (g - o).abs())
                            .fold(0.0f64, f64::max);
                        compared += 1;
                        if worst > 1.0 {
                            failures.push(format!(
                                "{label} {source} size {size} choke {choke} range {range}: \
                                 worst {worst}"
                            ));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(compared, 3 * 2 * 3 * 2 * 2);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_aa_member_draws_one_coverage_at_8_16_and_32_bits() {
    let (w, h, a) = take4_coverage("aahalf");
    let a32: Vec<f32> = alpha16(&a)
        .iter()
        .map(|&v| f32::from(v) / 65535.0)
        .collect();
    for source in ["edge", "center"] {
        let s = style(source, 12, 50, 50);
        let c8 = profile("aahalf", w, &render8_alpha(w, h, &alpha8(&a), &s));
        let c16 = profile("aahalf", w, &render16_alpha(w, h, &alpha16(&a), &s));
        let c32 = profile("aahalf", w, &render32_alpha(w, h, &a32, &s));
        let worst = |x: &[f64], y: &[f64]| {
            x.iter()
                .zip(y)
                .map(|(p, q)| (p - q).abs())
                .fold(0.0f64, f64::max)
        };
        let (d32, d8) = (worst(&c32, &c16), worst(&c8, &c16));
        assert!(d32 <= 1.0, "{source}: 32 bits {d32} codes from 16 bits");
        eprintln!("{source}: 32 vs 16 {d32:.4}, 8 vs 16 {d8:.4}");
    }
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    /// A 2048² layer: an opaque 1500² square centred, its rim a 64 px
    /// linear alpha ramp (`feathered`), or the same square with no ramp.
    fn layer(feathered: bool) -> Vec<u8> {
        let side = 2048u32;
        (0..side)
            .flat_map(|y| (0..side).map(move |x| (x, y)))
            .map(|(x, y)| {
                let dx = (f64::from(x) + 0.5 - 1024.0).abs() - 750.0;
                let dy = (f64::from(y) + 0.5 - 1024.0).abs() - 750.0;
                let out = dx.max(dy);
                if out <= 0.0 {
                    255
                } else if feathered {
                    ((1.0 - out / 64.0).clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
                } else {
                    0
                }
            })
            .collect()
    }

    fn median_of_three(alpha: &[u8], style: &Value) -> Duration {
        let mut runs: Vec<Duration> = (0..3)
            .map(|_| {
                let start = Instant::now();
                let out = render8_alpha(2048, 2048, alpha, style);
                let took = start.elapsed();
                assert_eq!(out.len(), 2048 * 2048);
                took
            })
            .collect();
        runs.sort();
        runs[1]
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict (#2)"]
    fn a_feathered_layer_costs_about_the_binary_one() {
        // Bound, declared before measuring: feathered <= 3 × binary + 50 ms.
        // Edge, Size 250, Choke 50 % (`La`; `Ma` did not ship), Range 50 %.
        let s = style("edge", 250, 50, 50);
        let (feathered, binary) = (layer(true), layer(false));
        let f = median_of_three(&feathered, &s);
        let b = median_of_three(&binary, &s);
        let bound = b * 3 + Duration::from_millis(50);
        eprintln!("choke 50: feathered {f:?}, binary {b:?}, bound {bound:?}");
        assert!(
            f <= bound,
            "feathered {f:?} exceeds 3 × binary {b:?} + 50 ms"
        );
    }
}
