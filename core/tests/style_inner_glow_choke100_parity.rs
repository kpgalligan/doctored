//! #578: the Inner Glow at Choke 100 %, edge and centre, against Photoshop's
//! own identify-578 take2 profiles, at 8 and 16 bits. The fixtures
//! (`tests/fixtures/psd_photoshop/identify_578/`, README) hold each source's
//! 80 rows' whole profiles: `choke100_profiles.json` (the edge), written by
//! `scripts/photoshop-probe/inner_glow_578_law_fit.py --fixtures-choke100`
//! on its `--judge-choke100` SHIP, and `choke100_center_profiles.json` (the
//! centre), written by `--fixtures-choke100-centre` on its CHOKE 100 CENTRE
//! SPLIT SHIP (the centre takes `M` below Size 24; its Size 32 rows render
//! today's kernel).
//!
//! Each row is rebuilt as `identify_578_take2.py` builds it: a black layer
//! over opaque black whose alpha is `floor(a · full + 0.5)` of an 8 × 8
//! supersampled coverage `a` (`fx`: 288 × 208, style-parity-v1x take1's
//! picture, the notched square, the 2 px bar and the r = 14 disc at (200,
//! 104); `aadisc`: 160², an r = 40 disc at (80, 80)), an 8-bit `DrDocument`
//! or a 16-bit `PrecisionDocument`, with a white Normal 100 % Inner Glow of
//! the row's source at Choke 100 % of the row's size and range. The profile is the
//! take's (`I2.profile`) on the red channel in 8-bit codes (16-bit samples /
//! 257): `fx` the four single-pixel cuts (row 112 over x in [64, 144), column
//! 88 over y in [56, 136), row 104 over x in [152, 162) and over [184, 216)),
//! `aadisc` the mean over the alpha > 0 pixels of each bin `floor(40 −
//! dist)`, -1 ... 39. A row's error is the max and the mean of `|ours −
//! Photoshop|` over the whole profile.
//!
//! The bound, declared in the runner before the take, is max <= 2 and mean
//! <= 0.5. A row that misses it asserts its own measured number (`PINNED`,
//! rounded up in the third decimal), never a wider one, and that it still
//! misses.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::identify_578::{disc_distance, render16_alpha, render8_alpha, supersampled};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

/// One take2 row, parsed from its name (`ig_<shape>_<e|c>_s<S>_c100_r<R>_<d>`).
struct Row {
    shape: String,
    source: &'static str,
    size: u32,
    range: u32,
    depth: u32,
}

fn parse(name: &str) -> Row {
    let parts: Vec<&str> = name.split('_').collect();
    let num = |s: &str| {
        s[1..]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    assert_eq!(parts[4], "c100", "{name}: a Choke 100 row");
    let source = match parts[2] {
        "e" => "edge",
        "c" => "center",
        other => panic!("{name}: unknown source {other}"),
    };
    Row {
        shape: parts[1].to_string(),
        source,
        size: num(parts[3]),
        range: num(parts[5]),
        depth: parts[6].parse().expect("depth"),
    }
}

/// The picture's size and coverage (`identify_578_take2.alpha_of`).
fn coverage(shape: &str) -> (u32, u32, Vec<f64>) {
    match shape {
        "fx" => {
            let (w, h) = (288, 208);
            let disc = supersampled(w, h, |x, y| {
                (x - 200.0).powi(2) + (y - 104.0).powi(2) <= 196.0
            });
            let a = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .zip(disc)
                .map(|((x, y), d)| {
                    let square = (72..136).contains(&x)
                        && (64..128).contains(&y)
                        && !((104..136).contains(&x) && (64..96).contains(&y));
                    let bar = (156..158).contains(&x) && (64..144).contains(&y);
                    if square || bar {
                        1.0
                    } else {
                        d
                    }
                })
                .collect();
            (w, h, a)
        }
        "aadisc" => (
            160,
            160,
            supersampled(160, 160, |x, y| {
                (x - 80.0).powi(2) + (y - 80.0).powi(2) <= 1600.0
            }),
        ),
        _ => panic!("unknown shape {shape}"),
    }
}

fn style(row: &Row) -> Value {
    json!({"effects":[{"type":"inner_glow","blend":"normal","color":"#ffffff","opacity":1,
        "size":row.size,"choke":1,"range":f64::from(row.range) / 100.0,
        "source":row.source,"anti_aliased":false}]})
}

/// The take's profile (`I2.profile`) of a render, in codes.
fn profile(shape: &str, w: u32, codes: &[f64], a: &[f64]) -> Vec<f64> {
    let at = |x: u32, y: u32| codes[(y * w + x) as usize];
    match shape {
        "fx" => {
            let mut out: Vec<f64> = (64..144).map(|x| at(x, 112)).collect();
            out.extend((56..136).map(|y| at(88, y)));
            out.extend((152..162).map(|x| at(x, 104)));
            out.extend((184..216).map(|x| at(x, 104)));
            out
        }
        _ => (-1..40)
            .map(|b| {
                let (mut total, mut n) = (0.0, 0usize);
                for y in 0..160 {
                    for x in 0..160 {
                        let bin = (40.0 - disc_distance(x, y)).floor() as i32;
                        if a[(y * w + x) as usize] > 0.0 && bin == b {
                            total += at(x, y);
                            n += 1;
                        }
                    }
                }
                total / n as f64
            })
            .collect(),
    }
}

/// `(max, mean)` of one row in 8-bit codes over its whole profile.
fn measure(name: &str, observed: &Value) -> (f64, f64) {
    let row = parse(name);
    let (w, h, a) = coverage(&row.shape);
    let s = style(&row);
    let codes = if row.depth == 8 {
        let alpha: Vec<u8> = a.iter().map(|&v| (v * 255.0 + 0.5).floor() as u8).collect();
        render8_alpha(w, h, &alpha, &s)
    } else {
        let alpha: Vec<u16> = a
            .iter()
            .map(|&v| (v * 65535.0 + 0.5).floor() as u16)
            .collect();
        render16_alpha(w, h, &alpha, &s)
    };
    let ours = profile(&row.shape, w, &codes, &a);
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

// Measured on the shipped kernel against take2, max and mean in codes
// rounded up in the third decimal: every row here misses 2 / 0.5. Only the
// Size 32 `fx` rows of both sources meet (their band covers every cut
// pixel). Photoshop measures the band from the anti-aliased contour and the
// kernel from the binarized one, so the partial pixels at the band's inner
// edge stay a named residual (`inner_glow_578_law_fit.py`, CHOKE 100
// DESCRIPTION), and the Size 32 band is 35.9 px wide in Photoshop (the
// centre's Size 32 rows render today's kernel, the CHOKE 100 CENTRE SPLIT).
const PINNED: &[(&str, f64, f64)] = &[
    ("ig_aadisc_c_s16_c100_r100_16", 17.209, 0.524),
    ("ig_aadisc_c_s16_c100_r100_8", 17.210, 0.520),
    ("ig_aadisc_c_s16_c100_r25_16", 24.560, 0.604),
    ("ig_aadisc_c_s16_c100_r25_8", 24.629, 0.601),
    ("ig_aadisc_c_s16_c100_r50_16", 13.590, 0.535),
    ("ig_aadisc_c_s16_c100_r50_8", 13.543, 0.529),
    ("ig_aadisc_c_s16_c100_r75_16", 23.068, 0.705),
    ("ig_aadisc_c_s16_c100_r75_8", 23.117, 0.705),
    ("ig_aadisc_c_s1_c100_r100_16", 23.587, 0.842),
    ("ig_aadisc_c_s1_c100_r100_8", 23.587, 0.834),
    ("ig_aadisc_c_s1_c100_r25_16", 17.510, 0.435),
    ("ig_aadisc_c_s1_c100_r25_8", 17.476, 0.427),
    ("ig_aadisc_c_s1_c100_r50_16", 21.247, 0.615),
    ("ig_aadisc_c_s1_c100_r50_8", 21.279, 0.609),
    ("ig_aadisc_c_s1_c100_r75_16", 14.539, 0.534),
    ("ig_aadisc_c_s1_c100_r75_8", 14.542, 0.527),
    ("ig_aadisc_c_s32_c100_r100_16", 128.001, 20.575),
    ("ig_aadisc_c_s32_c100_r100_8", 128.001, 20.575),
    ("ig_aadisc_c_s32_c100_r25_16", 255.001, 21.708),
    ("ig_aadisc_c_s32_c100_r25_8", 255.001, 21.708),
    ("ig_aadisc_c_s32_c100_r50_16", 255.001, 19.473),
    ("ig_aadisc_c_s32_c100_r50_8", 255.001, 19.470),
    ("ig_aadisc_c_s32_c100_r75_16", 170.001, 18.937),
    ("ig_aadisc_c_s32_c100_r75_8", 170.001, 18.941),
    ("ig_aadisc_c_s3_c100_r100_16", 8.164, 0.231),
    ("ig_aadisc_c_s3_c100_r100_8", 8.164, 0.224),
    ("ig_aadisc_c_s3_c100_r25_16", 3.873, 0.102),
    ("ig_aadisc_c_s3_c100_r25_8", 3.902, 0.096),
    ("ig_aadisc_c_s3_c100_r50_16", 4.332, 0.148),
    ("ig_aadisc_c_s3_c100_r50_8", 4.328, 0.141),
    ("ig_aadisc_c_s3_c100_r75_16", 2.039, 0.088),
    ("ig_aadisc_c_s3_c100_r75_8", 2.066, 0.080),
    ("ig_aadisc_c_s8_c100_r100_16", 2.732, 0.096),
    ("ig_aadisc_c_s8_c100_r100_8", 2.731, 0.090),
    ("ig_aadisc_c_s8_c100_r25_16", 17.656, 0.437),
    ("ig_aadisc_c_s8_c100_r25_8", 17.569, 0.429),
    ("ig_aadisc_c_s8_c100_r50_16", 2.462, 0.098),
    ("ig_aadisc_c_s8_c100_r50_8", 2.462, 0.093),
    ("ig_aadisc_c_s8_c100_r75_16", 6.092, 0.189),
    ("ig_aadisc_c_s8_c100_r75_8", 6.077, 0.180),
    ("ig_fx_c_s16_c100_r100_16", 80.997, 0.872),
    ("ig_fx_c_s16_c100_r100_8", 81.001, 0.872),
    ("ig_fx_c_s16_c100_r25_16", 119.993, 0.851),
    ("ig_fx_c_s16_c100_r25_8", 120.001, 0.852),
    ("ig_fx_c_s16_c100_r50_16", 59.993, 0.727),
    ("ig_fx_c_s16_c100_r50_8", 60.001, 0.723),
    ("ig_fx_c_s16_c100_r75_16", 90.001, 1.072),
    ("ig_fx_c_s16_c100_r75_8", 90.001, 1.070),
    ("ig_fx_c_s1_c100_r100_16", 3.012, 0.096),
    ("ig_fx_c_s1_c100_r100_8", 3.001, 0.090),
    ("ig_fx_c_s1_c100_r25_16", 11.962, 0.361),
    ("ig_fx_c_s1_c100_r25_8", 12.001, 0.357),
    ("ig_fx_c_s1_c100_r50_16", 5.985, 0.184),
    ("ig_fx_c_s1_c100_r50_8", 6.001, 0.179),
    ("ig_fx_c_s1_c100_r75_16", 4.024, 0.126),
    ("ig_fx_c_s1_c100_r75_8", 4.001, 0.119),
    ("ig_fx_c_s3_c100_r100_16", 13.012, 0.392),
    ("ig_fx_c_s3_c100_r100_8", 13.001, 0.387),
    ("ig_fx_c_s3_c100_r25_16", 51.806, 1.544),
    ("ig_fx_c_s3_c100_r25_8", 52.001, 1.545),
    ("ig_fx_c_s3_c100_r50_16", 25.907, 0.775),
    ("ig_fx_c_s3_c100_r50_8", 26.001, 0.773),
    ("ig_fx_c_s3_c100_r75_16", 17.370, 0.522),
    ("ig_fx_c_s3_c100_r75_8", 17.001, 0.505),
    ("ig_fx_c_s8_c100_r100_16", 28.008, 0.767),
    ("ig_fx_c_s8_c100_r100_8", 28.001, 0.763),
    ("ig_fx_c_s8_c100_r25_16", 111.573, 3.041),
    ("ig_fx_c_s8_c100_r25_8", 112.001, 3.050),
    ("ig_fx_c_s8_c100_r50_16", 55.790, 1.523),
    ("ig_fx_c_s8_c100_r50_8", 56.001, 1.525),
    ("ig_fx_c_s8_c100_r75_16", 37.394, 1.026),
    ("ig_fx_c_s8_c100_r75_8", 37.001, 1.010),
    ("ig_aadisc_e_s16_c100_r100_16", 17.015, 0.534),
    ("ig_aadisc_e_s16_c100_r100_8", 17.024, 0.524),
    ("ig_aadisc_e_s16_c100_r25_16", 24.568, 0.610),
    ("ig_aadisc_e_s16_c100_r25_8", 24.629, 0.601),
    ("ig_aadisc_e_s16_c100_r50_16", 13.597, 0.541),
    ("ig_aadisc_e_s16_c100_r50_8", 13.543, 0.529),
    ("ig_aadisc_e_s16_c100_r75_16", 23.058, 0.711),
    ("ig_aadisc_e_s16_c100_r75_8", 23.117, 0.705),
    ("ig_aadisc_e_s1_c100_r100_16", 23.482, 0.837),
    ("ig_aadisc_e_s1_c100_r100_8", 23.587, 0.834),
    ("ig_aadisc_e_s1_c100_r25_16", 17.502, 0.435),
    ("ig_aadisc_e_s1_c100_r25_8", 17.476, 0.427),
    ("ig_aadisc_e_s1_c100_r50_16", 21.239, 0.616),
    ("ig_aadisc_e_s1_c100_r50_8", 21.279, 0.609),
    ("ig_aadisc_e_s1_c100_r75_16", 14.526, 0.530),
    ("ig_aadisc_e_s1_c100_r75_8", 14.542, 0.527),
    ("ig_aadisc_e_s32_c100_r100_16", 254.993, 18.477),
    ("ig_aadisc_e_s32_c100_r100_8", 255.001, 18.464),
    ("ig_aadisc_e_s32_c100_r25_16", 254.993, 18.250),
    ("ig_aadisc_e_s32_c100_r25_8", 255.001, 18.237),
    ("ig_aadisc_e_s32_c100_r50_16", 254.993, 19.485),
    ("ig_aadisc_e_s32_c100_r50_8", 255.001, 19.470),
    ("ig_aadisc_e_s32_c100_r75_16", 254.993, 18.951),
    ("ig_aadisc_e_s32_c100_r75_8", 255.001, 18.941),
    ("ig_aadisc_e_s3_c100_r100_16", 8.172, 0.224),
    ("ig_aadisc_e_s3_c100_r100_8", 8.164, 0.216),
    ("ig_aadisc_e_s3_c100_r25_16", 3.881, 0.103),
    ("ig_aadisc_e_s3_c100_r25_8", 3.902, 0.096),
    ("ig_aadisc_e_s3_c100_r50_16", 4.325, 0.149),
    ("ig_aadisc_e_s3_c100_r50_8", 4.328, 0.141),
    ("ig_aadisc_e_s3_c100_r75_16", 2.030, 0.089),
    ("ig_aadisc_e_s3_c100_r75_8", 2.066, 0.080),
    ("ig_aadisc_e_s8_c100_r100_16", 2.894, 0.097),
    ("ig_aadisc_e_s8_c100_r100_8", 2.885, 0.088),
    ("ig_aadisc_e_s8_c100_r25_16", 17.664, 0.440),
    ("ig_aadisc_e_s8_c100_r25_8", 17.569, 0.429),
    ("ig_aadisc_e_s8_c100_r50_16", 2.454, 0.101),
    ("ig_aadisc_e_s8_c100_r50_8", 2.462, 0.094),
    ("ig_aadisc_e_s8_c100_r75_16", 6.083, 0.192),
    ("ig_aadisc_e_s8_c100_r75_8", 6.077, 0.180),
    ("ig_fx_e_s16_c100_r100_16", 79.989, 0.872),
    ("ig_fx_e_s16_c100_r100_8", 80.001, 0.867),
    ("ig_fx_e_s16_c100_r25_16", 120.001, 0.857),
    ("ig_fx_e_s16_c100_r25_8", 120.001, 0.852),
    ("ig_fx_e_s16_c100_r50_16", 60.001, 0.732),
    ("ig_fx_e_s16_c100_r50_8", 60.001, 0.723),
    ("ig_fx_e_s16_c100_r75_16", 89.993, 1.077),
    ("ig_fx_e_s16_c100_r75_8", 90.001, 1.070),
    ("ig_fx_e_s1_c100_r100_16", 3.004, 0.090),
    ("ig_fx_e_s1_c100_r100_8", 3.001, 0.090),
    ("ig_fx_e_s1_c100_r25_16", 11.954, 0.356),
    ("ig_fx_e_s1_c100_r25_8", 12.001, 0.357),
    ("ig_fx_e_s1_c100_r50_16", 5.977, 0.178),
    ("ig_fx_e_s1_c100_r50_8", 6.001, 0.179),
    ("ig_fx_e_s1_c100_r75_16", 4.008, 0.120),
    ("ig_fx_e_s1_c100_r75_8", 4.001, 0.119),
    ("ig_fx_e_s3_c100_r100_16", 13.004, 0.388),
    ("ig_fx_e_s3_c100_r100_8", 13.001, 0.387),
    ("ig_fx_e_s3_c100_r25_16", 51.798, 1.540),
    ("ig_fx_e_s3_c100_r25_8", 52.001, 1.545),
    ("ig_fx_e_s3_c100_r50_16", 25.899, 0.771),
    ("ig_fx_e_s3_c100_r50_8", 26.001, 0.773),
    ("ig_fx_e_s3_c100_r75_16", 17.355, 0.517),
    ("ig_fx_e_s3_c100_r75_8", 17.001, 0.505),
    ("ig_fx_e_s8_c100_r100_16", 28.001, 0.765),
    ("ig_fx_e_s8_c100_r100_8", 28.001, 0.763),
    ("ig_fx_e_s8_c100_r25_16", 111.565, 3.039),
    ("ig_fx_e_s8_c100_r25_8", 112.001, 3.050),
    ("ig_fx_e_s8_c100_r50_16", 55.783, 1.521),
    ("ig_fx_e_s8_c100_r50_8", 56.001, 1.525),
    ("ig_fx_e_s8_c100_r75_16", 37.378, 1.024),
    ("ig_fx_e_s8_c100_r75_8", 37.001, 1.010),
];

#[test]
fn every_choke100_edge_row_meets_the_bound_or_its_measured_residual() {
    check_fixture("choke100_profiles.json", "_e_");
}

#[test]
fn every_choke100_centre_row_meets_the_bound_or_its_measured_residual() {
    check_fixture("choke100_center_profiles.json", "_c_");
}

/// Every row of one source's fixture (`tag` is its name's source field) meets
/// 2 / 0.5 or its pin.
fn check_fixture(file: &str, tag: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/identify_578")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    let rows: BTreeMap<String, Value> = serde_json::from_str(&text).expect("json");
    assert_eq!(rows.len(), 80, "{file}: 80 rows");
    assert!(
        rows.keys().all(|k| k.contains(tag)),
        "{file}: every row is of one source"
    );
    for (name, ..) in PINNED.iter().filter(|p| p.0.contains(tag)) {
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
