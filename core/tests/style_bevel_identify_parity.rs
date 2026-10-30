//! #578: the Smooth Inner Bevel against Photoshop's own identify-578 take1
//! profiles, at 8 and 16 bits. The fixture
//! (`tests/fixtures/psd_photoshop/identify_578/bevel_profiles.json`, README)
//! holds every Smooth `bv_*` row's profile on its fixed gate, written by
//! `scripts/photoshop-probe/bevel_578_law_fit.py --fixtures` on the SHIP of
//! its `--judge-style-split` (the measured law `B`, `style_bevel_smooth`).
//!
//! Each row is rebuilt as `identify_578_take.py` builds it
//! (`common/identify_578.rs`: the `half` 320² rect and the `disc` of radius
//! 40, black and binary-alpha over opaque black), with a white Normal 100 %
//! highlight (`_h`) or a white Normal 100 % shadow (`_s`, the other at
//! opacity 0) of a Smooth Inner Bevel, Up, of the row's Size, Soften and
//! Depth, under the light `a0e0` (angle 0, altitude 0), `a90e0` (90, 0) or
//! `e90` (0, 90). The profile is the take's on the red channel in 8-bit
//! codes (16-bit samples / 257): `half` the column means over rows [128,
//! 192) on columns `gate[0]..=gate[1]` (128..=255), `disc` the inside
//! pixels' means per `floor(dist)` bin `gate[0]..=gate[1]` (0..=39). A row's
//! error is the max and the mean of `|ours − Photoshop|` over the gate.
//!
//! The bound, declared in the runner before the take, is max <= 2 and mean
//! <= 0.5. A row that misses it is one of the runner's `missed` rows and
//! asserts its own measured number (`PINNED`, rounded up in the third
//! decimal), never a wider one, and that it still misses.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::identify_578::{alpha, profile, render16, render8};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

/// One identify row, parsed from its name
/// (`bv_<shape>_smooth_<h|s>_<a0e0|a90e0|e90>_s<S>[_f<F>|_d<D>]_<8|16>`).
struct Row {
    shape: String,
    highlight: bool,
    angle: u32,
    altitude: u32,
    size: u32,
    soften: u32,
    depth_percent: u32,
    depth: u32,
}

fn parse(name: &str) -> Row {
    let parts: Vec<&str> = name.split('_').collect();
    assert_eq!(parts[0], "bv", "{name}");
    assert_eq!(parts[2], "smooth", "{name}");
    let num = |s: &str| {
        s[1..]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let (angle, altitude) = match parts[4] {
        "a0e0" => (0, 0),
        "a90e0" => (90, 0),
        "e90" => (0, 90),
        other => panic!("{name}: unknown light {other}"),
    };
    let (mut soften, mut depth_percent) = (0, 100);
    if parts.len() == 8 {
        match &parts[6][..1] {
            "f" => soften = num(parts[6]),
            "d" => depth_percent = num(parts[6]),
            other => panic!("{name}: unknown suffix {other}"),
        }
    } else {
        assert_eq!(parts.len(), 7, "{name}");
    }
    Row {
        shape: parts[1].to_string(),
        highlight: match parts[3] {
            "h" => true,
            "s" => false,
            other => panic!("{name}: unknown side {other}"),
        },
        angle,
        altitude,
        size: num(parts[5]),
        soften,
        depth_percent,
        depth: parts[parts.len() - 1].parse().expect("depth"),
    }
}

fn style(row: &Row) -> Value {
    json!({"effects":[{"type":"bevel_emboss","style":"inner_bevel","technique":"smooth",
        "depth":f64::from(row.depth_percent) / 100.0,"direction":"up","size":row.size,
        "soften":row.soften,"angle":row.angle,"altitude":row.altitude,
        "use_global_light":false,
        "highlight_blend":"normal","highlight_color":"#ffffff",
        "highlight_opacity":if row.highlight {1} else {0},
        "shadow_blend":"normal","shadow_color":"#ffffff",
        "shadow_opacity":if row.highlight {0} else {1}}]})
}

/// `(max, mean)` of one row in 8-bit codes on the gate (module doc).
fn measure(name: &str, observed: &Value) -> (f64, f64) {
    let row = parse(name);
    assert_eq!(observed["shape"], row.shape.as_str(), "{name}: shape");
    assert_eq!(observed["depth"], row.depth, "{name}: depth");
    let gate = observed["gate"].as_array().expect("gate");
    let (lo, hi) = (
        gate[0].as_u64().expect("gate lo") as usize,
        gate[1].as_u64().expect("gate hi") as usize,
    );
    // `profile` covers columns 128..=255 of the half plane and bins 0..=39
    // of the disc; the gate is sliced from that span.
    let first = if row.shape == "half" { 128 } else { 0 };
    let (w, h, inside) = alpha(&row.shape);
    let s = style(&row);
    let codes = if row.depth == 8 {
        render8(w, h, &inside, &s)
    } else {
        render16(w, h, &inside, &s)
    };
    let full = profile(&row.shape, w, &codes, &inside);
    assert!(
        lo >= first && hi < first + full.len() && lo <= hi,
        "{name}: gate {lo}..={hi} outside the profile"
    );
    let ours = &full[lo - first..=hi - first];
    let ps: Vec<f64> = observed["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(|v| v.as_f64().expect("value"))
        .collect();
    assert_eq!(ours.len(), ps.len(), "{name}: the gate's length");
    let (mut worst, mut total) = (0.0f64, 0.0);
    for (o, p) in ours.iter().zip(&ps) {
        let e = (o - p).abs();
        worst = worst.max(e);
        total += e;
    }
    (worst, total / ps.len() as f64)
}

// Measured on the shipped law against take1, max and mean in codes rounded up
// in the third decimal: every row here misses 2 / 0.5, and each is in the
// runner's `--judge-style-split` `missed` list: the Size 32 disc's overhead
// shadow, about one code low across the band as on the half plane (the
// runner's DESCRIPTION).
const PINNED: &[(&str, f64, f64)] = &[
    ("bv_disc_smooth_s_e90_s32_16", 1.001, 0.636),
    ("bv_disc_smooth_s_e90_s32_8", 1.001, 0.638),
];

#[test]
fn every_identify_bevel_row_meets_the_bound_or_its_measured_residual() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/identify_578/bevel_profiles.json");
    let text = std::fs::read_to_string(&path).expect("bevel_profiles.json");
    let rows: BTreeMap<String, Value> = serde_json::from_str(&text).expect("json");
    let names: Vec<&String> = rows.keys().filter(|k| k.starts_with("bv_")).collect();
    assert_eq!(names.len(), 144, "72 Smooth rows per depth");
    for (name, ..) in PINNED {
        assert!(
            rows.contains_key(*name),
            "{name}: pinned but not in the fixture"
        );
    }
    let mut failures = Vec::new();
    for name in names {
        let (max, mean) = measure(name, &rows[name]);
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
