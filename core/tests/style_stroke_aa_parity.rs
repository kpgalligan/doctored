//! #578: the Layer Style Inside Stroke's band on an anti-aliased shape,
//! measured from the anti-aliased edge by NA1
//! (`style_stroke_distance::inside_corner_band`, Center's inner half),
//! against Photoshop's own fourth Stroke take and its confirm take at 8 and
//! 16 bits, against a brute-force oracle, and at 16 against 32 bits. The
//! fixtures (`tests/fixtures/psd_photoshop/identify_578/`, README) are
//! `stroke_take4_profiles.json`, all 64 rows of the fourth take
//! (`scripts/photoshop-probe/stroke_aa_578_take4.py`, Inside and Center), and
//! `stroke_take5_profiles.json`, all 160 rows of the confirm take
//! (`stroke_aa_578_confirm5.py`, Inside only), written by the confirm
//! runner's `--fixtures` on its JUDGE SHIP for `Sn` (NA1 / `o · c`, rule
//! (i)). Center was KEEP in the fourth take, so its rows measure today's
//! Center, which already took NA1.
//!
//! Each row is rebuilt as its runner builds it: a #00ff00 layer over opaque
//! #0000ff whose alpha is the take's picture, at the row's Fill, with a
//! #ff0000 Normal stroke of the row's position, Size and opacity. The take4
//! pictures are `identify_578::take4_coverage` (the 320 × 544 half plane
//! whose x = 256 column has coverage 0.25 / 0.5 / 0.75, or the supersampled
//! r = 40 disc); the confirm take's are `identify_578::take5_coverage` (the
//! 320 × 704 half plane at 0.125 / 0.375 / 0.625 / 0.875, the r 17.5 and r 30
//! discs and the rounded rect). The profile is the take's, per channel and
//! concatenated r | g | b, in 8-bit codes (16-bit samples / 257): take4
//! `aahalf` the column means over each coverage's `BAND_ROWS` at x =
//! 128..=319, `aadisc` the mean of every pixel per bin `floor(40 − dist)`,
//! -32 ... 39; confirm `cfhalf` the column means over each band's rows at x
//! = 224..=287, the curved shapes the mean per bin `floor(−sdf)`
//! (`identify_578::take5_bin`), -8 ... 15. A row's error is the max and the
//! mean of `|ours − Photoshop|` over the fixture's gate (each runner's GATE).
//!
//! The bound, declared in each runner before its take, is max <= 2 and mean
//! <= 0.5. A row that misses it asserts its own measured number (`PINNED`,
//! `PINNED5`, rounded up in the third decimal), never a wider one, and that
//! it still misses.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::common::byte_unit;
use crate::identify_578::{
    profile2, render_rgb_alpha, supersampled, take4_coverage, take5_bin, take5_coverage,
};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;
const SOURCE: [f64; 3] = [0.0, 1.0, 0.0];
const BACKDROP: [f64; 3] = [0.0, 0.0, 1.0];
const STROKE: [f64; 3] = [1.0, 0.0, 0.0];

/// The rows each take4 `aahalf` band's profile averages (`I4.BAND_ROWS`).
const BAND_ROWS: [(u32, u32); 3] = [(80, 144), (240, 304), (400, 464)];

/// The rows each confirm `cfhalf` band's profile averages
/// (`stroke_aa_578_confirm5.BAND_ROWS`).
const BAND_ROWS5: [(u32, u32); 4] = [(80, 144), (240, 304), (400, 464), (560, 624)];

/// One take4 row, parsed from its name (`sk4_<shape>_<i|c>_s<S>_f<F>_<d>`).
struct Row {
    shape: String,
    position: &'static str,
    size: u32,
    fill: u32,
    depth: u32,
}

fn parse(name: &str) -> Row {
    let parts: Vec<&str> = name.split('_').collect();
    assert_eq!(parts.len(), 6, "{name}: a take4 stroke row");
    let num = |s: &str| {
        s[1..]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let position = match parts[2] {
        "i" => "inside",
        "c" => "center",
        other => panic!("{name}: unknown position {other}"),
    };
    Row {
        shape: parts[1].to_string(),
        position,
        size: num(parts[3]),
        fill: num(parts[4]),
        depth: parts[5].parse().expect("depth"),
    }
}

fn style(position: &str, size: u32, fill: f64) -> Value {
    style_at(position, size, fill, 1.0)
}

fn style_at(position: &str, size: u32, fill: f64, opacity: f64) -> Value {
    json!({"fill_opacity":fill,"effects":[{"type":"stroke","size":size,"position":position,
        "color":"#ff0000","opacity":opacity}]})
}

/// The take4 profile of a render, r | g | b (module doc).
fn profile(shape: &str, w: u32, codes: &[[f64; 3]]) -> Vec<f64> {
    let mut out = Vec::new();
    for k in 0..3 {
        let channel: Vec<f64> = codes.iter().map(|p| p[k]).collect();
        let at = |x: u32, y: u32| channel[(y * w + x) as usize];
        match shape {
            "aahalf" => {
                for &(y0, y1) in &BAND_ROWS {
                    for x in 128..320 {
                        out.push((y0..y1).map(|y| at(x, y)).sum::<f64>() / 64.0);
                    }
                }
            }
            _ => out.extend(profile2("disc", w, &channel)),
        }
    }
    out
}

/// The confirm take's profile of a render, r | g | b (module doc).
fn profile5(shape: &str, w: u32, h: u32, codes: &[[f64; 3]]) -> Vec<f64> {
    let bins: Vec<i32> = if shape == "cfhalf" {
        Vec::new()
    } else {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| take5_bin(shape, x, y))
            .collect()
    };
    let mut out = Vec::new();
    for k in 0..3 {
        if shape == "cfhalf" {
            for &(y0, y1) in &BAND_ROWS5 {
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

/// `(max, mean)` of `ours` against a fixture row in 8-bit codes over its
/// gate.
fn score(name: &str, ours: &[f64], observed: &Value) -> (f64, f64) {
    let ps: Vec<f64> = observed["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(|v| v.as_f64().expect("value"))
        .collect();
    assert_eq!(ours.len(), ps.len(), "{name}: the profile's length");
    let (mut worst, mut total, mut n) = (0.0f64, 0.0, 0usize);
    for range in observed["gate"].as_array().expect("gate") {
        let lo = range[0].as_u64().expect("lo") as usize;
        let hi = range[1].as_u64().expect("hi") as usize;
        for i in lo..=hi {
            let e = (ours[i] - ps[i]).abs();
            worst = worst.max(e);
            total += e;
            n += 1;
        }
    }
    assert!(n > 0, "{name}: an empty gate");
    (worst, total / n as f64)
}

/// `(max, mean)` of one take4 row (module doc).
fn measure(name: &str, observed: &Value) -> (f64, f64) {
    let row = parse(name);
    let (w, h, a) = take4_coverage(&row.shape);
    let s = style(row.position, row.size, f64::from(row.fill) / 100.0);
    let codes = render_rgb_alpha(row.depth, w, h, &a, SOURCE, BACKDROP, &s);
    score(name, &profile(&row.shape, w, &codes), observed)
}

/// `(max, mean)` of one confirm row, parsed from its name
/// (`sk5_<shape>_s<S>_f<F>_o<O>_<d>`; module doc).
fn measure5(name: &str, observed: &Value) -> (f64, f64) {
    let parts: Vec<&str> = name.split('_').collect();
    assert_eq!(parts.len(), 6, "{name}: a confirm stroke row");
    let num = |s: &str| {
        s[1..]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let (shape, size, fill, opacity) = (parts[1], num(parts[2]), num(parts[3]), num(parts[4]));
    let depth: u32 = parts[5].parse().expect("depth");
    let (w, h, a) = take5_coverage(shape);
    let s = style_at(
        "inside",
        size,
        f64::from(fill) / 100.0,
        f64::from(opacity) / 100.0,
    );
    let codes = render_rgb_alpha(depth, w, h, &a, SOURCE, BACKDROP, &s);
    score(name, &profile5(shape, w, h, &codes), observed)
}

/// Every row of `file` against the bound or its pin (module doc).
fn check_rows(
    file: &str,
    count: usize,
    pinned: &[(&str, f64, f64)],
    measure: fn(&str, &Value) -> (f64, f64),
) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/identify_578")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    let rows: BTreeMap<String, Value> = serde_json::from_str(&text).expect("json");
    assert_eq!(rows.len(), count, "{file}: {count} rows");
    for (name, ..) in pinned {
        assert!(
            rows.contains_key(*name),
            "{name}: pinned but not in the fixture"
        );
    }
    let mut failures = Vec::new();
    for (name, observed) in &rows {
        let (max, mean) = measure(name, observed);
        eprintln!("{name}: max {max:.6} mean {mean:.6}");
        match pinned.iter().find(|p| p.0 == name.as_str()) {
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

// Measured on the shipped kernel against the take, max and mean in codes
// rounded up in the third decimal: every row here misses 2 / 0.5. All are
// Center rows (today's kernel, `Ca` KEEP): Size 1 (JUDGE's bin: no declared
// pair reads it) and every Fill 50 % row (Photoshop fills the partial pixel
// with the whole stroke, `o`; the replacement draws `o · c`). Every Inside
// row meets on NA1 (`Sn`), the disc included, and so does every Center Fill
// 100 % row of Size 3, 4 and 8.
const PINNED: &[(&str, f64, f64)] = &[
    ("sk4_aadisc_c_s1_f100_16", 7.520, 0.417),
    ("sk4_aadisc_c_s1_f100_8", 7.587, 0.413),
    ("sk4_aadisc_c_s1_f50_16", 12.372, 0.646),
    ("sk4_aadisc_c_s1_f50_8", 12.449, 0.644),
    ("sk4_aadisc_c_s3_f50_16", 17.010, 0.974),
    ("sk4_aadisc_c_s3_f50_8", 17.001, 0.967),
    ("sk4_aadisc_c_s4_f50_16", 17.010, 0.967),
    ("sk4_aadisc_c_s4_f50_8", 17.001, 0.961),
    ("sk4_aadisc_c_s8_f50_16", 17.010, 0.969),
    ("sk4_aadisc_c_s8_f50_8", 17.001, 0.964),
    ("sk4_aahalf_c_s1_f100_16", 16.829, 0.283),
    ("sk4_aahalf_c_s1_f100_8", 17.001, 0.278),
    ("sk4_aahalf_c_s1_f50_16", 42.444, 0.670),
    ("sk4_aahalf_c_s1_f50_8", 42.001, 0.659),
    ("sk4_aahalf_c_s3_f50_16", 42.444, 0.889),
    ("sk4_aahalf_c_s3_f50_8", 42.001, 0.874),
    ("sk4_aahalf_c_s4_f50_16", 42.444, 0.889),
    ("sk4_aahalf_c_s4_f50_8", 42.001, 0.874),
    ("sk4_aahalf_c_s8_f50_16", 42.444, 0.888),
    ("sk4_aahalf_c_s8_f50_8", 42.001, 0.874),
];

#[test]
fn every_take4_stroke_row_meets_the_bound_or_its_measured_residual() {
    check_rows("stroke_take4_profiles.json", 64, PINNED, measure);
}

// Measured on the shipped kernel against the confirm take, rounded up in the
// third decimal: no row misses 2 / 0.5 (the runner's DESCRIPTION predicted
// every row within 1.02 / 0.25).
const PINNED5: &[(&str, f64, f64)] = &[];

#[test]
fn every_take5_stroke_row_meets_the_bound_or_its_measured_residual() {
    check_rows("stroke_take5_profiles.json", 160, PINNED5, measure5);
}

// ------------------------------------------------------------- oracle --

/// The oracle's margin around the canvas, past every reach these sizes have,
/// so the oracle's band sees the transparent outside as the kernel's padded
/// plane does.
const PAD: usize = 8;

/// The pictures of `style_inner_glow_aa_parity.rs` (the master copies):
/// a 40 × 32 supersampled r = 9 disc at (20, 16).
fn oracle_disc() -> (u32, u32, Vec<u8>) {
    let a = supersampled(40, 32, |x, y| {
        (x - 20.0).powi(2) + (y - 16.0).powi(2) <= 81.0
    });
    (
        40,
        32,
        a.iter().map(|&v| (v * 255.0 + 0.5).floor() as u8).collect(),
    )
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
/// inside its nearest side.
fn oracle_ramp() -> (u32, u32, Vec<u8>) {
    let (w, h) = (48u32, 32u32);
    let a = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (cx, cy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let m = (cx - 2.0).min(46.0 - cx).min(cy - 2.0).min(30.0 - cy);
            ((m / 12.0).clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
        })
        .collect();
    (w, h, a)
}

/// The 5 × 5 chamfer's path length between two pixel centres `(dx, dy)`
/// apart in closed form (steps 1, √2, √5): with `a = max(|dx|, |dy|)` and
/// `b = min`, `(a − 2b) + b√5` when `a ≥ 2b`, else `(a − b)√5 + (2b − a)√2`.
fn chamfer(dx: i64, dy: i64) -> f64 {
    let (a, b) = (dx.abs().max(dy.abs()) as f64, dx.abs().min(dy.abs()) as f64);
    if a >= 2.0 * b {
        (a - 2.0 * b) + b * 5f64.sqrt()
    } else {
        (a - b) * 5f64.sqrt() + (2.0 * b - a) * 2f64.sqrt()
    }
}

/// NA1 brute force in f64 on the padded byte plane `c`: `D(p) = min_q (c_q
/// / 255 + chamfer(q, p))` over every q with `c_q ≤ 254`, the band `max(c −
/// round(255 · clamp01(D − n)), 0)` in bytes.
fn oracle_band(c: &[u8], pw: usize, ph: usize, n: usize) -> Vec<f64> {
    let seeds: Vec<(i64, i64, f64)> = (0..pw * ph)
        .filter(|&i| c[i] <= 254)
        .map(|i| ((i % pw) as i64, (i / pw) as i64, f64::from(c[i]) / 255.0))
        .collect();
    let nf = n as f64;
    (0..pw * ph)
        .map(|i| {
            let (px, py) = ((i % pw) as i64, (i / pw) as i64);
            let d = seeds
                .iter()
                .map(|&(qx, qy, s)| s + chamfer(qx - px, qy - py))
                .fold(f64::INFINITY, f64::min);
            let cut = (255.0 * (d - nf).clamp(0.0, 1.0)).round();
            (f64::from(c[i]) - cut).max(0.0)
        })
        .collect()
}

/// The oracle's codes per channel: the NA1 band in bytes, then the
/// replacement at stroke opacity 1: `X` the layer over the backdrop
/// at `c / 255 · Fill` (Fill at its byte), `s = min(1, band / c)`,
/// `Y` the stroke over the backdrop at `c / 255`, the pixel `X + (Y − X) s`.
fn oracle_codes(w: u32, h: u32, alpha: &[u8], size: u32, fill: f64) -> Vec<[f64; 3]> {
    let (wu, hu) = (w as usize, h as usize);
    let (pw, ph) = (wu + 2 * PAD, hu + 2 * PAD);
    let mut padded = vec![0u8; pw * ph];
    for y in 0..hu {
        for x in 0..wu {
            padded[(y + PAD) * pw + x + PAD] = alpha[y * wu + x];
        }
    }
    let band = oracle_band(&padded, pw, ph, size as usize);
    let f = byte_unit(fill);
    let mut out = Vec::with_capacity(wu * hu);
    for y in 0..hu {
        for x in 0..wu {
            let c = f64::from(alpha[y * wu + x]);
            let b = band[(y + PAD) * pw + x + PAD];
            let a = c / 255.0 * f;
            let px = [0, 1, 2].map(|k| {
                let xk = a * SOURCE[k] + (1.0 - a) * BACKDROP[k];
                if c == 0.0 || b == 0.0 {
                    return xk * 255.0;
                }
                let s = (b / c).min(1.0);
                let yk = c / 255.0 * STROKE[k] + (1.0 - c / 255.0) * BACKDROP[k];
                (xk + (yk - xk) * s) * 255.0
            });
            out.push(px);
        }
    }
    out
}

#[test]
fn the_inside_band_matches_a_brute_force_oracle() {
    let pictures = [
        ("disc", oracle_disc()),
        ("edge", oracle_edge()),
        ("ramp", oracle_ramp()),
    ];
    let mut failures = Vec::new();
    let mut compared = 0usize;
    for (label, (w, h, alpha)) in &pictures {
        let unit: Vec<f64> = alpha.iter().map(|&v| f64::from(v) / 255.0).collect();
        for size in [1u32, 3, 4] {
            for fill in [1.0f64, 0.5] {
                let s = style("inside", size, fill);
                let got = render_rgb_alpha(8, *w, *h, &unit, SOURCE, BACKDROP, &s);
                let want = oracle_codes(*w, *h, alpha, size, fill);
                let worst = got
                    .iter()
                    .zip(&want)
                    .flat_map(|(g, o)| (0..3).map(move |k| (g[k] - o[k]).abs()))
                    .fold(0.0f64, f64::max);
                compared += 1;
                if worst > 1.0 {
                    failures.push(format!("{label} size {size} fill {fill}: worst {worst}"));
                }
            }
        }
    }
    assert_eq!(compared, 3 * 3 * 2);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_member_draws_one_share_at_16_and_32_bits() {
    // One row per shipped regime: Inside (`Sn`); Center did not ship.
    let (w, h, a) = take4_coverage("aahalf");
    let s = style("inside", 4, 0.5);
    let c16 = profile(
        "aahalf",
        w,
        &render_rgb_alpha(16, w, h, &a, SOURCE, BACKDROP, &s),
    );
    let c32 = profile(
        "aahalf",
        w,
        &render_rgb_alpha(32, w, h, &a, SOURCE, BACKDROP, &s),
    );
    let worst = |x: &[f64], y: &[f64]| {
        x.iter()
            .zip(y)
            .map(|(p, q)| (p - q).abs())
            .fold(0.0f64, f64::max)
    };
    let d32 = worst(&c32, &c16);
    eprintln!("inside: 32 vs 16 {d32:.4}");
    assert!(d32 <= 1.0, "inside: 32 bits {d32} codes from 16 bits");
}
