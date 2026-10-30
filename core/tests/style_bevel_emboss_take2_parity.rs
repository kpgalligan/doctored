//! #578: the Smooth Emboss and Pillow Emboss against Photoshop's own
//! identify-578 take2 profiles, at 8 and 16 bits. The fixture
//! (`tests/fixtures/psd_photoshop/identify_578/emboss_pillow_profiles.json`,
//! README) holds each style's 112 rows' whole profiles, written by
//! `scripts/photoshop-probe/bevel_578_law_fit.py --fixtures-take2` on the
//! SHIP of its `--judge-take2` (TAKE2 MEMBERS: `B`'s height at half the
//! Size, and for the Pillow that height with its outside reversed,
//! `style_bevel_smooth`).
//!
//! Each row is rebuilt as `identify_578_take2.py` builds it
//! (`common/identify_578.rs`: the first take's `half` 320² rect and `disc`
//! of radius 40, black and binary-alpha over opaque black), with a white
//! Normal 100 % highlight (`_h`) or a white Normal 100 % shadow (`_s`, the
//! other at opacity 0) of a Smooth Emboss or Pillow Emboss, Up, Soften 0, of
//! the row's Size and Depth, under the light `a0e0` (angle 0, altitude 0) or
//! `e90` (0, 90). The profile is the second take's on the red channel in
//! 8-bit codes (16-bit samples / 257, `profile2`): `half` the column means
//! over rows [128, 192) at x = 128..=319, `disc` every pixel's mean per bin
//! `floor(40 − dist)`, -32..=39. A row's error is the max and the mean of
//! `|ours − Photoshop|` over the whole profile.
//!
//! The bound is max <= 2 and mean <= 0.5. A row that misses it asserts its
//! own measured number (`PINNED`, rounded up in the third decimal), never a
//! wider one, and that it still misses.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::identify_578::{alpha, profile2, render16, render8};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

/// One take2 bevel row, parsed from its name
/// (`bv_<half|disc>_<emboss|pillow>_<h|s>_<a0e0|e90>_s<S>[_d<D>]_<8|16>`).
struct Row {
    shape: String,
    style: &'static str,
    highlight: bool,
    altitude: u32,
    size: u32,
    depth_percent: u32,
    depth: u32,
}

fn parse(name: &str) -> Row {
    let parts: Vec<&str> = name.split('_').collect();
    assert_eq!(parts[0], "bv", "{name}");
    let num = |s: &str| {
        s[1..]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let style = match parts[2] {
        "emboss" => "emboss",
        "pillow" => "pillow_emboss",
        other => panic!("{name}: unknown style {other}"),
    };
    let altitude = match parts[4] {
        "a0e0" => 0,
        "e90" => 90,
        other => panic!("{name}: unknown light {other}"),
    };
    let depth_percent = if parts.len() == 8 {
        assert_eq!(&parts[6][..1], "d", "{name}");
        num(parts[6])
    } else {
        assert_eq!(parts.len(), 7, "{name}");
        100
    };
    Row {
        shape: parts[1].to_string(),
        style,
        highlight: match parts[3] {
            "h" => true,
            "s" => false,
            other => panic!("{name}: unknown side {other}"),
        },
        altitude,
        size: num(parts[5]),
        depth_percent,
        depth: parts[parts.len() - 1].parse().expect("depth"),
    }
}

fn style(row: &Row) -> Value {
    json!({"effects":[{"type":"bevel_emboss","style":row.style,"technique":"smooth",
        "depth":f64::from(row.depth_percent) / 100.0,"direction":"up","size":row.size,
        "soften":0,"angle":0,"altitude":row.altitude,
        "use_global_light":false,
        "highlight_blend":"normal","highlight_color":"#ffffff",
        "highlight_opacity":if row.highlight {1} else {0},
        "shadow_blend":"normal","shadow_color":"#ffffff",
        "shadow_opacity":if row.highlight {0} else {1}}]})
}

/// `(max, mean)` of one row in 8-bit codes over its whole profile.
fn measure(name: &str, observed: &Value) -> (f64, f64) {
    let row = parse(name);
    assert_eq!(observed["shape"], row.shape.as_str(), "{name}: shape");
    assert_eq!(observed["depth"], row.depth, "{name}: depth");
    let (w, h, inside) = alpha(&row.shape);
    let s = style(&row);
    let codes = if row.depth == 8 {
        render8(w, h, &inside, &s)
    } else {
        render16(w, h, &inside, &s)
    };
    let ours = profile2(&row.shape, w, &codes);
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

// Measured on the shipped law against take2, max and mean in codes rounded up
// in the third decimal: every row here misses 2 / 0.5, and each is in the
// runner's `--judge-take2` `missed` list (36 Emboss, 56 Pillow Emboss). Most
// miss on the max alone, one column or bin near the edge: the half plane's
// angle-0 highlights (up to 62 codes for Emboss, 91 for Pillow, every mean
// under 0.5), the overhead shadows (8 and 6.5 for Emboss; 77 and 68 for
// Pillow, means to 0.87 and 1.73) and the disc's angle-0 highlights, where
// the light's angle to the contour turns (9 and 27).
const PINNED: &[(&str, f64, f64)] = &[
    ("bv_disc_emboss_h_a0e0_s1_16", 8.871, 0.286),
    ("bv_disc_emboss_h_a0e0_s1_8", 8.877, 0.284),
    ("bv_disc_emboss_h_a0e0_s4_16", 8.871, 0.286),
    ("bv_disc_emboss_h_a0e0_s4_8", 8.877, 0.284),
    ("bv_disc_emboss_h_a0e0_s8_16", 2.219, 0.125),
    ("bv_disc_emboss_h_a0e0_s8_8", 2.275, 0.127),
    ("bv_disc_emboss_h_a0e0_s8_d200_16", 9.075, 0.285),
    ("bv_disc_emboss_h_a0e0_s8_d200_8", 9.078, 0.283),
    ("bv_disc_emboss_s_e90_s1_16", 6.474, 0.224),
    ("bv_disc_emboss_s_e90_s1_8", 6.449, 0.222),
    ("bv_disc_emboss_s_e90_s4_16", 6.474, 0.224),
    ("bv_disc_emboss_s_e90_s4_8", 6.449, 0.222),
    ("bv_disc_emboss_s_e90_s8_16", 2.778, 0.138),
    ("bv_disc_emboss_s_e90_s8_8", 2.863, 0.140),
    ("bv_disc_emboss_s_e90_s8_d200_16", 2.800, 0.149),
    ("bv_disc_emboss_s_e90_s8_d200_8", 2.759, 0.151),
    ("bv_disc_pillow_h_a0e0_s16_16", 26.897, 0.577),
    ("bv_disc_pillow_h_a0e0_s16_8", 26.897, 0.576),
    ("bv_disc_pillow_h_a0e0_s1_16", 20.829, 0.743),
    ("bv_disc_pillow_h_a0e0_s1_8", 20.828, 0.743),
    ("bv_disc_pillow_h_a0e0_s32_16", 25.397, 0.484),
    ("bv_disc_pillow_h_a0e0_s32_8", 25.397, 0.486),
    ("bv_disc_pillow_h_a0e0_s4_16", 20.829, 0.743),
    ("bv_disc_pillow_h_a0e0_s4_8", 20.828, 0.743),
    ("bv_disc_pillow_h_a0e0_s8_16", 20.975, 0.595),
    ("bv_disc_pillow_h_a0e0_s8_8", 20.975, 0.596),
    ("bv_disc_pillow_h_a0e0_s8_d200_16", 19.406, 0.703),
    ("bv_disc_pillow_h_a0e0_s8_d200_8", 19.406, 0.704),
    ("bv_disc_pillow_h_a0e0_s8_d50_16", 14.605, 0.391),
    ("bv_disc_pillow_h_a0e0_s8_d50_8", 14.604, 0.391),
    ("bv_disc_pillow_s_e90_s16_16", 46.969, 1.123),
    ("bv_disc_pillow_s_e90_s16_8", 46.966, 1.121),
    ("bv_disc_pillow_s_e90_s1_16", 34.106, 0.940),
    ("bv_disc_pillow_s_e90_s1_8", 34.104, 0.939),
    ("bv_disc_pillow_s_e90_s32_16", 48.589, 1.189),
    ("bv_disc_pillow_s_e90_s32_8", 48.587, 1.190),
    ("bv_disc_pillow_s_e90_s4_16", 34.106, 0.940),
    ("bv_disc_pillow_s_e90_s4_8", 34.104, 0.939),
    ("bv_disc_pillow_s_e90_s8_16", 39.175, 1.029),
    ("bv_disc_pillow_s_e90_s8_8", 39.173, 1.029),
    ("bv_disc_pillow_s_e90_s8_d200_16", 68.310, 1.727),
    ("bv_disc_pillow_s_e90_s8_d200_8", 68.311, 1.729),
    ("bv_disc_pillow_s_e90_s8_d50_16", 13.694, 0.403),
    ("bv_disc_pillow_s_e90_s8_d50_8", 13.690, 0.402),
    ("bv_half_emboss_h_a0e0_s16_16", 16.001, 0.170),
    ("bv_half_emboss_h_a0e0_s16_8", 16.001, 0.157),
    ("bv_half_emboss_h_a0e0_s1_16", 62.001, 0.324),
    ("bv_half_emboss_h_a0e0_s1_8", 62.001, 0.323),
    ("bv_half_emboss_h_a0e0_s32_16", 15.697, 0.249),
    ("bv_half_emboss_h_a0e0_s32_8", 16.001, 0.219),
    ("bv_half_emboss_h_a0e0_s4_16", 62.001, 0.324),
    ("bv_half_emboss_h_a0e0_s4_8", 62.001, 0.323),
    ("bv_half_emboss_h_a0e0_s8_16", 32.001, 0.176),
    ("bv_half_emboss_h_a0e0_s8_8", 32.001, 0.167),
    ("bv_half_emboss_h_a0e0_s8_d200_16", 62.001, 0.435),
    ("bv_half_emboss_h_a0e0_s8_d200_8", 62.001, 0.428),
    ("bv_half_emboss_h_a0e0_s8_d50_16", 16.001, 0.087),
    ("bv_half_emboss_h_a0e0_s8_d50_8", 16.001, 0.084),
    ("bv_half_emboss_s_e90_s1_16", 8.001, 0.056),
    ("bv_half_emboss_s_e90_s1_8", 8.001, 0.058),
    ("bv_half_emboss_s_e90_s4_16", 8.001, 0.056),
    ("bv_half_emboss_s_e90_s4_8", 8.001, 0.058),
    ("bv_half_emboss_s_e90_s8_d200_16", 8.001, 0.065),
    ("bv_half_emboss_s_e90_s8_d200_8", 8.001, 0.068),
    ("bv_half_pillow_h_a0e0_s16_16", 71.989, 0.376),
    ("bv_half_pillow_h_a0e0_s16_8", 72.001, 0.376),
    ("bv_half_pillow_h_a0e0_s1_16", 90.993, 0.474),
    ("bv_half_pillow_h_a0e0_s1_8", 91.001, 0.474),
    ("bv_half_pillow_h_a0e0_s32_16", 68.993, 0.360),
    ("bv_half_pillow_h_a0e0_s32_8", 69.001, 0.360),
    ("bv_half_pillow_h_a0e0_s4_16", 90.993, 0.474),
    ("bv_half_pillow_h_a0e0_s4_8", 91.001, 0.474),
    ("bv_half_pillow_h_a0e0_s8_16", 77.989, 0.407),
    ("bv_half_pillow_h_a0e0_s8_8", 78.001, 0.407),
    ("bv_half_pillow_h_a0e0_s8_d200_16", 67.993, 0.355),
    ("bv_half_pillow_h_a0e0_s8_d200_8", 68.001, 0.355),
    ("bv_half_pillow_h_a0e0_s8_d50_16", 55.001, 0.287),
    ("bv_half_pillow_h_a0e0_s8_d50_8", 55.001, 0.287),
    ("bv_half_pillow_s_e90_s16_16", 47.320, 0.542),
    ("bv_half_pillow_s_e90_s16_8", 47.001, 0.542),
    ("bv_half_pillow_s_e90_s1_16", 42.281, 0.486),
    ("bv_half_pillow_s_e90_s1_8", 42.001, 0.485),
    ("bv_half_pillow_s_e90_s32_16", 47.207, 0.628),
    ("bv_half_pillow_s_e90_s32_8", 47.001, 0.636),
    ("bv_half_pillow_s_e90_s4_16", 42.281, 0.486),
    ("bv_half_pillow_s_e90_s4_8", 42.001, 0.485),
    ("bv_half_pillow_s_e90_s8_16", 46.421, 0.516),
    ("bv_half_pillow_s_e90_s8_8", 46.001, 0.516),
    ("bv_half_pillow_s_e90_s8_d200_16", 77.043, 0.868),
    ("bv_half_pillow_s_e90_s8_d200_8", 77.001, 0.870),
    ("bv_half_pillow_s_e90_s8_d50_16", 17.004, 0.195),
    ("bv_half_pillow_s_e90_s8_d50_8", 17.001, 0.198),
];

#[test]
fn every_take2_emboss_and_pillow_row_meets_the_bound_or_its_measured_residual() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/identify_578/emboss_pillow_profiles.json");
    let text = std::fs::read_to_string(&path).expect("emboss_pillow_profiles.json");
    let rows: BTreeMap<String, Value> = serde_json::from_str(&text).expect("json");
    for tag in ["_emboss_", "_pillow_"] {
        assert_eq!(
            rows.keys().filter(|k| k.contains(tag)).count(),
            112,
            "112 {tag} rows"
        );
    }
    assert_eq!(rows.len(), 224, "only Emboss and Pillow Emboss rows");
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
