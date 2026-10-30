//! #578: the Inner Glow against Photoshop's own identify-578 take1 profiles,
//! edge and centre, at 8 and 16 bits. The fixture
//! (`tests/fixtures/psd_photoshop/identify_578/profiles.json`, README) holds
//! every `ig_*` row's profile on its fixed gate, written by
//! `scripts/photoshop-probe/inner_glow_578_law_fit.py --fixtures` for the
//! sources its `--judge-split` ships.
//!
//! Each row is rebuilt here as `identify_578_take.py` builds it: a black,
//! binary-alpha layer over opaque black (`half` 320² with the rect x in
//! [32, 256), y in [32, 288); `disc` 160², a pixel whose centre is within 40
//! of (80, 80); `corner` 224², the rect [32, 192)²), an 8-bit `DrDocument`
//! or a 16-bit `PrecisionDocument`, with a white Normal 100 % Inner Glow of
//! the row's size, choke, range and source. The profile is the take's
//! (`I.profile`) on the red channel in 8-bit codes (16-bit samples / 257):
//! `half` the column means over rows [128, 192) on columns 128..=255,
//! `disc` the inside pixels' means per `floor(dist)` bin 0..39, `corner`
//! the diagonal `(32 + k, 32 + k)`, k < 64. A row's error is the max and the
//! mean of `|ours − Photoshop|` over the gate.
//!
//! The bound, declared in the runner before the take, is max <= 2 and mean
//! <= 0.5. A row that misses it asserts its own measured number (`PINNED`,
//! rounded up in the third decimal), never a wider one, and that it still
//! misses. The pinned rows are the named bins and today's kernel at Size·(1 −
//! Choke) == 0 (the centre at Choke 100 % from Size 24, Size 0); a row the
//! measured law runs on outside a bin may not be pinned.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::identify_578::{alpha, profile, render16, render8};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

/// One identify row, parsed from its name (`ig_<shape>_<e|c>_s<S>_c<C>[_r<R>]_<d>`).
struct Row {
    shape: String,
    center: bool,
    size: u32,
    choke: u32,
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
    let range = if parts[5].starts_with('r') {
        num(parts[5])
    } else {
        100
    };
    Row {
        shape: parts[1].to_string(),
        center: parts[2] == "c",
        size: num(parts[3]),
        choke: num(parts[4]),
        range,
        depth: parts[parts.len() - 1].parse().expect("depth"),
    }
}

fn style(row: &Row) -> Value {
    json!({"effects":[{"type":"inner_glow","blend":"normal","color":"#ffffff","opacity":1,
        "size":row.size,"choke":f64::from(row.choke) / 100.0,
        "range":f64::from(row.range) / 100.0,
        "source":if row.center {"center"} else {"edge"},"anti_aliased":false}]})
}

/// `(max, mean)` of one row in 8-bit codes on the gate (module doc).
fn measure(name: &str, observed: &Value) -> (f64, f64) {
    let row = parse(name);
    let (w, h, inside) = alpha(&row.shape);
    let s = style(&row);
    let codes = if row.depth == 8 {
        render8(w, h, &inside, &s)
    } else {
        render16(w, h, &inside, &s)
    };
    let ours = profile(&row.shape, w, &codes, &inside);
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

/// Whether the measured law runs on the row: Size·(1 − Choke) > 0.
fn law(row: &Row) -> bool {
    row.size > 0 && row.choke < 100
}

/// The runner's named bins (`in_bin`): Size 1, 8 or 32 with `n == Size`,
/// `n = floor(Size · Choke + 0.5)`.
fn in_bin(row: &Row) -> bool {
    let n = (row.size * row.choke + 50) / 100;
    [1, 8, 32].contains(&row.size) && n == row.size
}

// Measured on the shipped kernel against take1, max and mean in codes rounded
// up in the third decimal: every row here misses 2 / 0.5. They are the named
// bins (Size 1 at Choke 50, 80 and 100; Size 8 and 32 at Choke 100) and
// today's kernel at Size·(1 − Choke) == 0 for the centre: Choke 100 % from
// Size 24 (127 codes inside the band at Range 100 %) and Size 0 at Range 100
// %. The Choke 100 % take (identify-578 take2, `inner_glow_578_law_fit.py
// --judge-choke100`) shipped the edge's member `M` (a full-band pixel glows
// full at every Range), which lowered the edge's Choke 100 % rows here and
// took Size 2, 4 and 16 at Range 100 % to 0 / 0; its CHOKE 100 CENTRE SPLIT
// (`--judge-choke100-centre`) shipped `M` for the centre below Size 24, which
// did the same for the centre's rows (Size 32 keeps today's kernel).
const PINNED: &[(&str, f64, f64)] = &[
    ("ig_corner_c_s32_c100_16", 128.001, 61.825),
    ("ig_corner_c_s32_c100_8", 128.001, 61.829),
    ("ig_corner_c_s8_c100_16", 28.008, 0.445),
    ("ig_corner_c_s8_c100_8", 28.001, 0.438),
    ("ig_corner_e_s32_c100_16", 254.993, 13.676),
    ("ig_corner_e_s32_c100_8", 255.001, 13.672),
    ("ig_corner_e_s8_c100_16", 28.001, 0.439),
    ("ig_corner_e_s8_c100_8", 28.001, 0.438),
    ("ig_disc_c_s32_c100_16", 128.001, 21.188),
    ("ig_disc_c_s32_c100_8", 128.001, 21.189),
    ("ig_disc_c_s8_c100_16", 40.045, 1.462),
    ("ig_disc_c_s8_c100_8", 40.040, 1.455),
    ("ig_disc_e_s32_c100_16", 254.993, 19.295),
    ("ig_disc_e_s32_c100_8", 255.001, 19.289),
    ("ig_disc_e_s8_c100_16", 39.782, 1.446),
    ("ig_disc_e_s8_c100_8", 39.785, 1.445),
    ("ig_half_c_s0_c0_16", 126.993, 126.993),
    ("ig_half_c_s0_c0_8", 127.001, 127.001),
    ("ig_half_c_s0_c100_16", 126.993, 126.993),
    ("ig_half_c_s0_c100_8", 127.001, 127.001),
    ("ig_half_c_s0_c50_16", 126.993, 126.993),
    ("ig_half_c_s0_c50_8", 127.001, 127.001),
    ("ig_half_c_s0_c80_16", 126.993, 126.993),
    ("ig_half_c_s0_c80_8", 127.001, 127.001),
    ("ig_half_c_s1_c100_16", 3.012, 0.032),
    ("ig_half_c_s1_c100_8", 3.001, 0.024),
    ("ig_half_c_s1_c100_r50_16", 5.985, 0.055),
    ("ig_half_c_s1_c100_r50_8", 6.001, 0.047),
    ("ig_half_c_s1_c50_16", 3.012, 0.032),
    ("ig_half_c_s1_c50_8", 3.001, 0.024),
    ("ig_half_c_s1_c50_r50_16", 5.985, 0.055),
    ("ig_half_c_s1_c50_r50_8", 6.001, 0.047),
    ("ig_half_c_s1_c80_16", 3.012, 0.032),
    ("ig_half_c_s1_c80_8", 3.001, 0.024),
    ("ig_half_c_s1_c80_r50_16", 5.985, 0.055),
    ("ig_half_c_s1_c80_r50_8", 6.001, 0.047),
    ("ig_half_c_s32_c100_16", 128.001, 94.409),
    ("ig_half_c_s32_c100_8", 128.001, 94.415),
    ("ig_half_c_s32_c100_r50_16", 255.001, 7.695),
    ("ig_half_c_s32_c100_r50_8", 255.001, 7.688),
    ("ig_half_c_s8_c100_16", 28.008, 0.227),
    ("ig_half_c_s8_c100_8", 28.001, 0.219),
    ("ig_half_c_s8_c100_r50_16", 55.790, 0.444),
    ("ig_half_c_s8_c100_r50_8", 56.001, 0.438),
    ("ig_half_e_s1_c100_16", 3.004, 0.024),
    ("ig_half_e_s1_c100_8", 3.001, 0.024),
    ("ig_half_e_s1_c100_r50_16", 5.977, 0.047),
    ("ig_half_e_s1_c100_r50_8", 6.001, 0.047),
    ("ig_half_e_s1_c50_16", 3.004, 0.024),
    ("ig_half_e_s1_c50_8", 3.001, 0.024),
    ("ig_half_e_s1_c50_r50_16", 5.977, 0.047),
    ("ig_half_e_s1_c50_r50_8", 6.001, 0.047),
    ("ig_half_e_s1_c80_16", 3.004, 0.024),
    ("ig_half_e_s1_c80_8", 3.001, 0.024),
    ("ig_half_e_s1_c80_r50_16", 5.977, 0.047),
    ("ig_half_e_s1_c80_r50_8", 6.001, 0.047),
    ("ig_half_e_s32_c100_16", 254.993, 6.838),
    ("ig_half_e_s32_c100_8", 255.001, 6.836),
    ("ig_half_e_s32_c100_r50_16", 254.993, 7.691),
    ("ig_half_e_s32_c100_r50_8", 255.001, 7.688),
    ("ig_half_e_s8_c100_16", 28.001, 0.220),
    ("ig_half_e_s8_c100_8", 28.001, 0.219),
    ("ig_half_e_s8_c100_r50_16", 55.783, 0.437),
    ("ig_half_e_s8_c100_r50_8", 56.001, 0.438),
];

#[test]
fn every_identify_glow_row_meets_the_bound_or_its_measured_residual() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/identify_578/profiles.json");
    let text = std::fs::read_to_string(&path).expect("profiles.json");
    let rows: BTreeMap<String, Value> = serde_json::from_str(&text).expect("json");
    let names: Vec<&String> = rows.keys().filter(|k| k.starts_with("ig_")).collect();
    assert_eq!(names.len(), 264, "132 rows per source");
    for (name, ..) in PINNED {
        let row = parse(name);
        assert!(
            !law(&row) || in_bin(&row),
            "{name}: a law row outside the bins may not be pinned"
        );
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
