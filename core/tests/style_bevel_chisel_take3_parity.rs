//! #2: the Chisel law's shipped arms (Chisel Hard and Chisel Soft Pillow
//! Emboss, `style_bevel_chisel`) against Photoshop's own identify-578 take3
//! profiles, at 8 and 16 bits. The fixture
//! (`tests/fixtures/psd_photoshop/identify_578/chisel_take3_profiles.json`,
//! README) holds each shipped arm's 96 take3 rows, written by
//! `scripts/photoshop-probe/bevel_578_law_fit.py --fixtures-chisel-take3` on
//! the SHIP of its `--judge-chisel-take3` (TAKE3 CHISEL MEMBERS). Only a
//! shipping Inner arm would add identify take1 and take2 rows; none ships, so
//! every row is `take` 3.
//!
//! Each row is rebuilt as `identify_578_take3.py` builds it
//! (`common/identify_578.rs`: the first take's `half` 320² picture and `disc`
//! of radius 40, black and binary-alpha over opaque black), with a white
//! Normal 100 % highlight (`_h`) or a white Normal 100 % shadow (`_s`, the
//! other at opacity 0) of the row's Chisel Pillow Emboss, Up, Depth 100 %,
//! angle 0, at the row's altitude, Soften and Size. The profile is the third
//! take's on the red channel in 8-bit codes (16-bit samples / 257,
//! `profile3`): `half` the column means over rows [128, 192) at x = 0..=319,
//! `disc` every pixel's mean per bin `floor(40 − dist)`, -32..=39. A row's
//! error is the max and the mean of `|ours − Photoshop|` over the whole
//! profile.
//!
//! The bound is max <= 2 and mean <= 0.5. A row that misses it asserts its
//! own measured number (`PINNED`, rounded up in the third decimal), never a
//! wider one, and that it still misses.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::identify_578::{alpha, profile3, render16, render8};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

/// One take3 bevel row, parsed from its name
/// (`bv3_<half|disc>_pillow_<chard|csoft>_<h|s>_e<A>_f<F>_s<S>_<8|16>`).
struct Row {
    shape: String,
    technique: &'static str,
    highlight: bool,
    altitude: u32,
    soften: u32,
    size: u32,
    depth: u32,
}

fn parse(name: &str) -> Row {
    let parts: Vec<&str> = name.split('_').collect();
    assert_eq!(parts.len(), 9, "{name}");
    assert_eq!(parts[0], "bv3", "{name}");
    assert_eq!(parts[2], "pillow", "{name}: only Pillow Emboss ships");
    let num = |s: &str| {
        s[1..]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    Row {
        shape: parts[1].to_string(),
        technique: match parts[3] {
            "chard" => "chisel_hard",
            "csoft" => "chisel_soft",
            other => panic!("{name}: unknown technique {other}"),
        },
        highlight: match parts[4] {
            "h" => true,
            "s" => false,
            other => panic!("{name}: unknown side {other}"),
        },
        altitude: num(parts[5]),
        soften: num(parts[6]),
        size: num(parts[7]),
        depth: parts[8].parse().expect("depth"),
    }
}

fn style(row: &Row) -> Value {
    json!({"effects":[{"type":"bevel_emboss","style":"pillow_emboss",
        "technique":row.technique,"depth":1.0,"direction":"up","size":row.size,
        "soften":row.soften,"angle":0,"altitude":row.altitude,
        "use_global_light":false,
        "highlight_blend":"normal","highlight_color":"#ffffff",
        "highlight_opacity":if row.highlight {1} else {0},
        "shadow_blend":"normal","shadow_color":"#ffffff",
        "shadow_opacity":if row.highlight {0} else {1}}]})
}

fn floats(v: &Value, key: &str, name: &str) -> Vec<f64> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: no {key}"))
        .iter()
        .map(|x| x.as_f64().expect("value"))
        .collect()
}

/// `(max, mean)` of one row in 8-bit codes over its whole profile.
fn measure(name: &str, observed: &Value) -> (f64, f64) {
    let row = parse(name);
    assert_eq!(observed["take"], 3, "{name}: take");
    assert_eq!(observed["shape"], row.shape.as_str(), "{name}: shape");
    assert_eq!(observed["depth"], row.depth, "{name}: depth");
    let (w, h, inside) = alpha(&row.shape);
    let s = style(&row);
    let codes = if row.depth == 8 {
        render8(w, h, &inside, &s)
    } else {
        render16(w, h, &inside, &s)
    };
    let ours = profile3(&row.shape, w, &codes);
    let mut ps = if row.shape == "half" {
        floats(observed, "left", name)
    } else {
        Vec::new()
    };
    ps.extend(floats(observed, "values", name));
    assert_eq!(ours.len(), ps.len(), "{name}: the profile's length");
    let (mut worst, mut total) = (0.0f64, 0.0);
    for (o, p) in ours.iter().zip(&ps) {
        let e = (o - p).abs();
        worst = worst.max(e);
        total += e;
    }
    (worst, total / ps.len() as f64)
}

// Measured on the shipped law against take3, max and mean in codes rounded up
// in the third decimal: no row meets 2 / 0.5 (the runner's `--judge-chisel-
// take3` reads none within it either, and the kernel reads every row within
// 0.009 / 0.0001 of the runner's model). The maxima reach 220 (Hard) and 180
// (Soft) codes on the half plane and 60 and 64 on the disc, at a few edge
// columns or bins; the means are 0.59 ... 5.34.
const PINNED: &[(&str, f64, f64)] = &[
    ("bv3_disc_pillow_chard_h_e30_f0_s16_16", 40.766, 2.519),
    ("bv3_disc_pillow_chard_h_e30_f0_s16_8", 40.766, 2.519),
    ("bv3_disc_pillow_chard_h_e30_f0_s4_16", 36.001, 2.266),
    ("bv3_disc_pillow_chard_h_e30_f0_s4_8", 36.000, 2.266),
    ("bv3_disc_pillow_chard_h_e30_f4_s16_16", 34.027, 3.106),
    ("bv3_disc_pillow_chard_h_e30_f4_s16_8", 34.026, 3.105),
    ("bv3_disc_pillow_chard_h_e30_f4_s4_16", 31.909, 2.747),
    ("bv3_disc_pillow_chard_h_e30_f4_s4_8", 31.908, 2.747),
    ("bv3_disc_pillow_chard_h_e45_f0_s16_16", 36.916, 2.261),
    ("bv3_disc_pillow_chard_h_e45_f0_s16_8", 36.933, 2.260),
    ("bv3_disc_pillow_chard_h_e45_f0_s4_16", 40.072, 2.425),
    ("bv3_disc_pillow_chard_h_e45_f0_s4_8", 40.065, 2.426),
    ("bv3_disc_pillow_chard_h_e45_f4_s16_16", 38.655, 2.710),
    ("bv3_disc_pillow_chard_h_e45_f4_s16_8", 38.656, 2.709),
    ("bv3_disc_pillow_chard_h_e45_f4_s4_16", 39.274, 2.833),
    ("bv3_disc_pillow_chard_h_e45_f4_s4_8", 39.289, 2.834),
    ("bv3_disc_pillow_chard_h_e60_f0_s16_16", 38.932, 2.220),
    ("bv3_disc_pillow_chard_h_e60_f0_s16_8", 38.932, 2.221),
    ("bv3_disc_pillow_chard_h_e60_f0_s4_16", 48.568, 2.881),
    ("bv3_disc_pillow_chard_h_e60_f0_s4_8", 48.569, 2.879),
    ("bv3_disc_pillow_chard_h_e60_f4_s16_16", 39.345, 2.131),
    ("bv3_disc_pillow_chard_h_e60_f4_s16_8", 39.345, 2.130),
    ("bv3_disc_pillow_chard_h_e60_f4_s4_16", 48.292, 3.454),
    ("bv3_disc_pillow_chard_h_e60_f4_s4_8", 48.304, 3.454),
    ("bv3_disc_pillow_chard_s_e30_f0_s16_16", 60.050, 3.713),
    ("bv3_disc_pillow_chard_s_e30_f0_s16_8", 60.050, 3.713),
    ("bv3_disc_pillow_chard_s_e30_f0_s4_16", 47.050, 3.063),
    ("bv3_disc_pillow_chard_s_e30_f0_s4_8", 47.050, 3.063),
    ("bv3_disc_pillow_chard_s_e30_f4_s16_16", 52.888, 5.336),
    ("bv3_disc_pillow_chard_s_e30_f4_s16_8", 52.888, 5.336),
    ("bv3_disc_pillow_chard_s_e30_f4_s4_16", 42.324, 3.700),
    ("bv3_disc_pillow_chard_s_e30_f4_s4_8", 42.324, 3.700),
    ("bv3_disc_pillow_chard_s_e45_f0_s16_16", 38.186, 2.150),
    ("bv3_disc_pillow_chard_s_e45_f0_s16_8", 38.210, 2.151),
    ("bv3_disc_pillow_chard_s_e45_f0_s4_16", 29.227, 1.799),
    ("bv3_disc_pillow_chard_s_e45_f0_s4_8", 29.220, 1.799),
    ("bv3_disc_pillow_chard_s_e45_f4_s16_16", 34.898, 3.154),
    ("bv3_disc_pillow_chard_s_e45_f4_s16_8", 34.897, 3.154),
    ("bv3_disc_pillow_chard_s_e45_f4_s4_16", 26.891, 2.032),
    ("bv3_disc_pillow_chard_s_e45_f4_s4_8", 26.893, 2.031),
    ("bv3_disc_pillow_chard_s_e60_f0_s16_16", 27.407, 1.563),
    ("bv3_disc_pillow_chard_s_e60_f0_s16_8", 27.439, 1.565),
    ("bv3_disc_pillow_chard_s_e60_f0_s4_16", 19.558, 1.218),
    ("bv3_disc_pillow_chard_s_e60_f0_s4_8", 19.557, 1.218),
    ("bv3_disc_pillow_chard_s_e60_f4_s16_16", 25.002, 2.381),
    ("bv3_disc_pillow_chard_s_e60_f4_s16_8", 25.001, 2.382),
    ("bv3_disc_pillow_chard_s_e60_f4_s4_16", 18.184, 1.370),
    ("bv3_disc_pillow_chard_s_e60_f4_s4_8", 18.204, 1.370),
    ("bv3_disc_pillow_csoft_h_e30_f0_s16_16", 20.233, 1.769),
    ("bv3_disc_pillow_csoft_h_e30_f0_s16_8", 20.233, 1.769),
    ("bv3_disc_pillow_csoft_h_e30_f0_s4_16", 20.992, 1.404),
    ("bv3_disc_pillow_csoft_h_e30_f0_s4_8", 20.992, 1.404),
    ("bv3_disc_pillow_csoft_h_e30_f4_s16_16", 37.802, 2.289),
    ("bv3_disc_pillow_csoft_h_e30_f4_s16_8", 37.802, 2.289),
    ("bv3_disc_pillow_csoft_h_e30_f4_s4_16", 33.294, 2.200),
    ("bv3_disc_pillow_csoft_h_e30_f4_s4_8", 33.294, 2.200),
    ("bv3_disc_pillow_csoft_h_e45_f0_s16_16", 22.109, 1.349),
    ("bv3_disc_pillow_csoft_h_e45_f0_s16_8", 22.108, 1.349),
    ("bv3_disc_pillow_csoft_h_e45_f0_s4_16", 22.095, 1.366),
    ("bv3_disc_pillow_csoft_h_e45_f0_s4_8", 22.095, 1.366),
    ("bv3_disc_pillow_csoft_h_e45_f4_s16_16", 40.612, 2.452),
    ("bv3_disc_pillow_csoft_h_e45_f4_s16_8", 40.613, 2.452),
    ("bv3_disc_pillow_csoft_h_e45_f4_s4_16", 39.363, 2.377),
    ("bv3_disc_pillow_csoft_h_e45_f4_s4_8", 39.363, 2.377),
    ("bv3_disc_pillow_csoft_h_e60_f0_s16_16", 25.461, 1.243),
    ("bv3_disc_pillow_csoft_h_e60_f0_s16_8", 25.461, 1.243),
    ("bv3_disc_pillow_csoft_h_e60_f0_s4_16", 27.894, 1.280),
    ("bv3_disc_pillow_csoft_h_e60_f0_s4_8", 27.894, 1.280),
    ("bv3_disc_pillow_csoft_h_e60_f4_s16_16", 34.000, 2.163),
    ("bv3_disc_pillow_csoft_h_e60_f4_s16_8", 34.000, 2.161),
    ("bv3_disc_pillow_csoft_h_e60_f4_s4_16", 44.466, 2.705),
    ("bv3_disc_pillow_csoft_h_e60_f4_s4_8", 44.466, 2.705),
    ("bv3_disc_pillow_csoft_s_e30_f0_s16_16", 43.777, 3.246),
    ("bv3_disc_pillow_csoft_s_e30_f0_s16_8", 43.776, 3.246),
    ("bv3_disc_pillow_csoft_s_e30_f0_s4_16", 38.561, 2.136),
    ("bv3_disc_pillow_csoft_s_e30_f0_s4_8", 38.561, 2.136),
    ("bv3_disc_pillow_csoft_s_e30_f4_s16_16", 64.293, 4.391),
    ("bv3_disc_pillow_csoft_s_e30_f4_s16_8", 64.294, 4.391),
    ("bv3_disc_pillow_csoft_s_e30_f4_s4_16", 48.190, 3.267),
    ("bv3_disc_pillow_csoft_s_e30_f4_s4_8", 48.190, 3.267),
    ("bv3_disc_pillow_csoft_s_e45_f0_s16_16", 33.113, 2.055),
    ("bv3_disc_pillow_csoft_s_e45_f0_s16_8", 33.113, 2.054),
    ("bv3_disc_pillow_csoft_s_e45_f0_s4_16", 26.699, 1.293),
    ("bv3_disc_pillow_csoft_s_e45_f0_s4_8", 26.699, 1.291),
    ("bv3_disc_pillow_csoft_s_e45_f4_s16_16", 44.294, 2.981),
    ("bv3_disc_pillow_csoft_s_e45_f4_s16_8", 44.294, 2.981),
    ("bv3_disc_pillow_csoft_s_e45_f4_s4_16", 31.252, 1.953),
    ("bv3_disc_pillow_csoft_s_e45_f4_s4_8", 31.250, 1.953),
    ("bv3_disc_pillow_csoft_s_e60_f0_s16_16", 26.390, 1.624),
    ("bv3_disc_pillow_csoft_s_e60_f0_s16_8", 26.388, 1.624),
    ("bv3_disc_pillow_csoft_s_e60_f0_s4_16", 19.924, 0.927),
    ("bv3_disc_pillow_csoft_s_e60_f0_s4_8", 19.923, 0.926),
    ("bv3_disc_pillow_csoft_s_e60_f4_s16_16", 32.778, 2.229),
    ("bv3_disc_pillow_csoft_s_e60_f4_s16_8", 32.776, 2.230),
    ("bv3_disc_pillow_csoft_s_e60_f4_s4_16", 21.864, 1.366),
    ("bv3_disc_pillow_csoft_s_e60_f4_s4_8", 21.863, 1.365),
    ("bv3_half_pillow_chard_h_e30_f0_s16_16", 157.997, 1.907),
    ("bv3_half_pillow_chard_h_e30_f0_s16_8", 158.001, 1.907),
    ("bv3_half_pillow_chard_h_e30_f0_s4_16", 127.004, 1.657),
    ("bv3_half_pillow_chard_h_e30_f0_s4_8", 127.001, 1.657),
    ("bv3_half_pillow_chard_h_e30_f4_s16_16", 115.997, 2.513),
    ("bv3_half_pillow_chard_h_e30_f4_s16_8", 116.001, 2.513),
    ("bv3_half_pillow_chard_h_e30_f4_s4_16", 113.004, 2.054),
    ("bv3_half_pillow_chard_h_e30_f4_s4_8", 113.001, 2.054),
    ("bv3_half_pillow_chard_h_e45_f0_s16_16", 120.001, 1.771),
    ("bv3_half_pillow_chard_h_e45_f0_s16_8", 120.001, 1.769),
    ("bv3_half_pillow_chard_h_e45_f0_s4_16", 164.028, 1.883),
    ("bv3_half_pillow_chard_h_e45_f0_s4_8", 164.001, 1.882),
    ("bv3_half_pillow_chard_h_e45_f4_s16_16", 141.004, 2.491),
    ("bv3_half_pillow_chard_h_e45_f4_s16_8", 141.001, 2.491),
    ("bv3_half_pillow_chard_h_e45_f4_s4_16", 145.417, 2.303),
    ("bv3_half_pillow_chard_h_e45_f4_s4_8", 145.001, 2.304),
    ("bv3_half_pillow_chard_h_e60_f0_s16_16", 169.989, 1.979),
    ("bv3_half_pillow_chard_h_e60_f0_s16_8", 170.001, 1.976),
    ("bv3_half_pillow_chard_h_e60_f0_s4_16", 219.880, 2.425),
    ("bv3_half_pillow_chard_h_e60_f0_s4_8", 220.001, 2.426),
    ("bv3_half_pillow_chard_h_e60_f4_s16_16", 163.074, 2.546),
    ("bv3_half_pillow_chard_h_e60_f4_s16_8", 163.001, 2.544),
    ("bv3_half_pillow_chard_h_e60_f4_s4_16", 193.036, 3.071),
    ("bv3_half_pillow_chard_h_e60_f4_s4_8", 193.001, 3.069),
    ("bv3_half_pillow_chard_s_e30_f0_s16_16", 201.001, 2.541),
    ("bv3_half_pillow_chard_s_e30_f0_s16_8", 201.001, 2.541),
    ("bv3_half_pillow_chard_s_e30_f0_s4_16", 151.997, 2.038),
    ("bv3_half_pillow_chard_s_e30_f0_s4_8", 152.001, 2.038),
    ("bv3_half_pillow_chard_s_e30_f4_s16_16", 157.001, 3.535),
    ("bv3_half_pillow_chard_s_e30_f4_s16_8", 157.001, 3.535),
    ("bv3_half_pillow_chard_s_e30_f4_s4_16", 135.001, 2.516),
    ("bv3_half_pillow_chard_s_e30_f4_s4_8", 135.001, 2.516),
    ("bv3_half_pillow_chard_s_e45_f0_s16_16", 118.234, 1.387),
    ("bv3_half_pillow_chard_s_e45_f0_s16_8", 118.001, 1.385),
    ("bv3_half_pillow_chard_s_e45_f0_s4_16", 93.292, 1.110),
    ("bv3_half_pillow_chard_s_e45_f0_s4_8", 93.001, 1.107),
    ("bv3_half_pillow_chard_s_e45_f4_s16_16", 99.004, 1.996),
    ("bv3_half_pillow_chard_s_e45_f4_s16_8", 99.001, 1.997),
    ("bv3_half_pillow_chard_s_e45_f4_s4_16", 82.285, 1.330),
    ("bv3_half_pillow_chard_s_e45_f4_s4_8", 82.001, 1.329),
    ("bv3_half_pillow_chard_s_e60_f0_s16_16", 81.300, 0.945),
    ("bv3_half_pillow_chard_s_e60_f0_s16_8", 81.001, 0.947),
    ("bv3_half_pillow_chard_s_e60_f0_s4_16", 59.292, 0.722),
    ("bv3_half_pillow_chard_s_e60_f0_s4_8", 59.001, 0.719),
    ("bv3_half_pillow_chard_s_e60_f4_s16_16", 65.300, 1.357),
    ("bv3_half_pillow_chard_s_e60_f4_s16_8", 65.001, 1.354),
    ("bv3_half_pillow_chard_s_e60_f4_s4_16", 52.316, 0.854),
    ("bv3_half_pillow_chard_s_e60_f4_s4_8", 52.001, 0.854),
    ("bv3_half_pillow_csoft_h_e30_f0_s16_16", 90.001, 1.688),
    ("bv3_half_pillow_csoft_h_e30_f0_s16_8", 90.001, 1.688),
    ("bv3_half_pillow_csoft_h_e30_f0_s4_16", 93.001, 1.138),
    ("bv3_half_pillow_csoft_h_e30_f0_s4_8", 93.001, 1.138),
    ("bv3_half_pillow_csoft_h_e30_f4_s16_16", 130.997, 1.932),
    ("bv3_half_pillow_csoft_h_e30_f4_s16_8", 131.001, 1.932),
    ("bv3_half_pillow_csoft_h_e30_f4_s4_16", 115.997, 1.713),
    ("bv3_half_pillow_csoft_h_e30_f4_s4_8", 116.001, 1.713),
    ("bv3_half_pillow_csoft_h_e45_f0_s16_16", 91.148, 1.487),
    ("bv3_half_pillow_csoft_h_e45_f0_s16_8", 91.001, 1.488),
    ("bv3_half_pillow_csoft_h_e45_f0_s4_16", 107.405, 1.230),
    ("bv3_half_pillow_csoft_h_e45_f0_s4_8", 107.001, 1.229),
    ("bv3_half_pillow_csoft_h_e45_f4_s16_16", 151.997, 2.308),
    ("bv3_half_pillow_csoft_h_e45_f4_s16_8", 152.001, 2.310),
    ("bv3_half_pillow_csoft_h_e45_f4_s4_16", 144.257, 2.060),
    ("bv3_half_pillow_csoft_h_e45_f4_s4_8", 144.001, 2.060),
    ("bv3_half_pillow_csoft_h_e60_f0_s16_16", 118.989, 1.349),
    ("bv3_half_pillow_csoft_h_e60_f0_s16_8", 119.001, 1.351),
    ("bv3_half_pillow_csoft_h_e60_f0_s4_16", 105.993, 1.406),
    ("bv3_half_pillow_csoft_h_e60_f0_s4_8", 106.001, 1.407),
    ("bv3_half_pillow_csoft_h_e60_f4_s16_16", 156.993, 2.500),
    ("bv3_half_pillow_csoft_h_e60_f4_s16_8", 157.001, 2.501),
    ("bv3_half_pillow_csoft_h_e60_f4_s4_16", 179.989, 2.594),
    ("bv3_half_pillow_csoft_h_e60_f4_s4_8", 180.001, 2.594),
    ("bv3_half_pillow_csoft_s_e30_f0_s16_16", 121.001, 2.388),
    ("bv3_half_pillow_csoft_s_e30_f0_s16_8", 121.001, 2.388),
    ("bv3_half_pillow_csoft_s_e30_f0_s4_16", 136.997, 1.532),
    ("bv3_half_pillow_csoft_s_e30_f0_s4_8", 137.001, 1.532),
    ("bv3_half_pillow_csoft_s_e30_f4_s16_16", 175.001, 2.788),
    ("bv3_half_pillow_csoft_s_e30_f4_s16_8", 175.001, 2.788),
    ("bv3_half_pillow_csoft_s_e30_f4_s4_16", 146.997, 2.219),
    ("bv3_half_pillow_csoft_s_e30_f4_s4_8", 147.001, 2.219),
    ("bv3_half_pillow_csoft_s_e45_f0_s16_16", 102.499, 1.577),
    ("bv3_half_pillow_csoft_s_e45_f0_s16_8", 102.001, 1.576),
    ("bv3_half_pillow_csoft_s_e45_f0_s4_16", 88.355, 0.881),
    ("bv3_half_pillow_csoft_s_e45_f0_s4_8", 88.001, 0.879),
    ("bv3_half_pillow_csoft_s_e45_f4_s16_16", 121.993, 1.962),
    ("bv3_half_pillow_csoft_s_e45_f4_s16_8", 122.001, 1.963),
    ("bv3_half_pillow_csoft_s_e45_f4_s4_16", 92.036, 1.290),
    ("bv3_half_pillow_csoft_s_e45_f4_s4_8", 92.001, 1.288),
    ("bv3_half_pillow_csoft_s_e60_f0_s16_16", 73.001, 1.115),
    ("bv3_half_pillow_csoft_s_e60_f0_s16_8", 73.001, 1.119),
    ("bv3_half_pillow_csoft_s_e60_f0_s4_16", 60.265, 0.587),
    ("bv3_half_pillow_csoft_s_e60_f0_s4_8", 60.001, 0.585),
    ("bv3_half_pillow_csoft_s_e60_f4_s16_16", 82.001, 1.334),
    ("bv3_half_pillow_csoft_s_e60_f4_s16_8", 82.001, 1.338),
    ("bv3_half_pillow_csoft_s_e60_f4_s4_16", 60.047, 0.843),
    ("bv3_half_pillow_csoft_s_e60_f4_s4_8", 60.001, 0.844),
];

#[test]
fn every_take3_chisel_pillow_row_meets_the_bound_or_its_measured_residual() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/identify_578/chisel_take3_profiles.json");
    let text = std::fs::read_to_string(&path).expect("chisel_take3_profiles.json");
    let rows: BTreeMap<String, Value> = serde_json::from_str(&text).expect("json");
    for tag in ["_chard_", "_csoft_"] {
        assert_eq!(
            rows.keys().filter(|k| k.contains(tag)).count(),
            96,
            "96 {tag} rows"
        );
    }
    assert_eq!(rows.len(), 192, "only the two Pillow Emboss arms");
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
