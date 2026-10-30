//! #578 part 2, phase 2: the shipped outside cut of the Smooth Outer Bevel
//! (`style_bevel_smooth::cut_outside_with_job`, the runner
//! `scripts/photoshop-probe/effect_kernels_578_p2_take.py`, its candidate
//! SO1) against Photoshop 26.10's own pixels, at 8 and 16 bits.
//!
//! - THE TAKE3 ROWS (always run): identify-578 take3's 160 Smooth Outer
//!   Bevel profiles, committed as
//!   `fixtures/psd_photoshop/identify_578/bevel_take3_outside_profiles.json`
//!   (README). Each row is rebuilt as `identify_578_take3.py` builds it
//!   (`common/identify_578.rs`: the first take's `half` 320² picture and
//!   `disc` of radius 40, black and binary-alpha over opaque black) with a
//!   white Normal 100 % highlight (`_h`) or shadow (`_s`, the other at
//!   opacity 0) of the row's Smooth Outer Bevel, Up, Depth 100 %, angle 0,
//!   at the row's altitude, Soften and Size, and read as the third take's
//!   profile on red (`profile3`: `half` the column means over rows [128,
//!   192) at x = 0..=319, `disc` the 72 bins), against Photoshop's `left ++
//!   values` over the whole profile.
//! - THE CONFIRM TAKE (SKIP when absent): `effect-kernels-578-p2/take1` under
//!   `~/devel/doctored-fixtures/photoshop`, its 96 Smooth Outer Bevel rows
//!   (the built arm's; the take's Emboss and Pillow Emboss rows keep today's
//!   kernel) rebuilt from the take's own `inputs/pictures` (a black source
//!   over opaque black at the file's depth) with each row's Doctored style
//!   (`rows.json`), against Photoshop's readback `rows/<case>.png`: the red
//!   channel (`_h`, highlight (255, 226, 170)) or the blue (`_s`, shadow
//!   (170, 214, 255)) over the runner's ROW, the pixels whose Euclidean
//!   distance between pixel centres to the nearest pixel of the other side
//!   of the 0.5 contour is at most `2 (size + soften) + 2`.
//!
//! Every row meets the runner's gate, max <= 2 and mean <= 0.5 codes (16
//! bits as `/ 257`), or asserts its own measured number (`PINNED`, rounded
//! up in the third decimal), never a wider one.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::identify_578::{alpha, profile3, render16, render8, render_rgb_alpha};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

fn take3_fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/identify_578/bevel_take3_outside_profiles.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the committed fixture"))
        .expect("json")
}

/// One take3 row, parsed from its name
/// (`bv3_<half|disc>_outer_smooth_<h|s>_e<A>_f<F>_s<S>_<8|16>`).
struct Row {
    shape: String,
    highlight: bool,
    altitude: u32,
    soften: u32,
    size: u32,
    depth: u32,
}

fn parse(name: &str) -> Row {
    let parts: Vec<&str> = name.split('_').collect();
    assert_eq!(parts.len(), 9, "{name}");
    assert_eq!(
        (parts[0], parts[2], parts[3]),
        ("bv3", "outer", "smooth"),
        "{name}"
    );
    let num = |s: &str| {
        s[1..]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    Row {
        shape: parts[1].to_string(),
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

fn take3_style(row: &Row) -> Value {
    json!({"effects":[{"type":"bevel_emboss","style":"outer_bevel",
        "technique":"smooth","depth":1.0,"direction":"up","size":row.size,
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

fn error(ours: &[f64], theirs: &[f64], what: &str) -> (f64, f64) {
    assert_eq!(ours.len(), theirs.len(), "{what}: length");
    assert!(!ours.is_empty(), "{what}: empty");
    let e: Vec<f64> = ours
        .iter()
        .zip(theirs)
        .map(|(a, b)| (a - b).abs())
        .collect();
    (
        e.iter().copied().fold(0.0, f64::max),
        e.iter().sum::<f64>() / e.len() as f64,
    )
}

fn take3_row(name: &str, observed: &Value) -> (f64, f64) {
    let row = parse(name);
    assert_eq!(observed["take"], 3, "{name}: take");
    assert_eq!(observed["shape"], row.shape.as_str(), "{name}: shape");
    assert_eq!(observed["depth"], row.depth, "{name}: depth");
    let (w, h, inside) = alpha(&row.shape);
    let s = take3_style(&row);
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
    error(&ours, &ps, name)
}

/// The rows that do not meet the gate, each with its measured `(max,
/// mean)` in codes, rounded up in the third decimal (module doc). The core
/// reads every row within 0.0015 code of the runner's model of SO1 in its
/// mean (`fit.json`, `judged.json`).
const PINNED: &[(&str, f64, f64)] = &[
    // take3 (74 of 160): the altitude 0 rows, which no cut touches (Size 1,
    // 62 codes on the half plane); the highlight at 45 / 60, Photoshop's
    // darker outside half, which no declared form models (115 / 1.84 on the
    // half plane at Size 16, from today's 115 / 1.93); the shadow at Soften
    // 4 and at Size 16; the disc's Size 1 and 4 rows at 30, 45 and 90.
    ("bv3_disc_outer_smooth_h_e0_f0_s1_16", 8.871, 0.169),
    ("bv3_disc_outer_smooth_h_e0_f0_s1_8", 8.877, 0.169),
    ("bv3_disc_outer_smooth_h_e0_f0_s4_16", 2.219, 0.067),
    ("bv3_disc_outer_smooth_h_e0_f0_s4_8", 2.275, 0.068),
    ("bv3_disc_outer_smooth_h_e30_f0_s4_16", 2.065, 0.069),
    ("bv3_disc_outer_smooth_h_e30_f0_s4_8", 2.064, 0.069),
    ("bv3_disc_outer_smooth_h_e45_f0_s16_16", 9.560, 0.730),
    ("bv3_disc_outer_smooth_h_e45_f0_s16_8", 9.488, 0.729),
    ("bv3_disc_outer_smooth_h_e45_f0_s4_16", 7.310, 0.189),
    ("bv3_disc_outer_smooth_h_e45_f0_s4_8", 7.310, 0.189),
    ("bv3_disc_outer_smooth_h_e45_f4_s16_16", 9.548, 0.784),
    ("bv3_disc_outer_smooth_h_e45_f4_s16_8", 9.488, 0.783),
    ("bv3_disc_outer_smooth_h_e45_f4_s4_16", 11.575, 0.223),
    ("bv3_disc_outer_smooth_h_e45_f4_s4_8", 11.578, 0.224),
    ("bv3_disc_outer_smooth_h_e60_f0_s16_16", 19.677, 1.667),
    ("bv3_disc_outer_smooth_h_e60_f0_s16_8", 19.682, 1.667),
    ("bv3_disc_outer_smooth_h_e60_f0_s4_16", 14.707, 0.444),
    ("bv3_disc_outer_smooth_h_e60_f0_s4_8", 14.682, 0.444),
    ("bv3_disc_outer_smooth_h_e60_f4_s16_16", 18.937, 1.659),
    ("bv3_disc_outer_smooth_h_e60_f4_s16_8", 18.866, 1.659),
    ("bv3_disc_outer_smooth_h_e60_f4_s4_16", 13.069, 0.287),
    ("bv3_disc_outer_smooth_h_e60_f4_s4_8", 13.078, 0.287),
    ("bv3_disc_outer_smooth_s_e30_f0_s4_16", 2.326, 0.099),
    ("bv3_disc_outer_smooth_s_e30_f0_s4_8", 2.325, 0.099),
    ("bv3_disc_outer_smooth_s_e30_f4_s16_16", 2.385, 0.134),
    ("bv3_disc_outer_smooth_s_e30_f4_s16_8", 2.384, 0.134),
    ("bv3_disc_outer_smooth_s_e30_f4_s4_16", 7.870, 0.267),
    ("bv3_disc_outer_smooth_s_e30_f4_s4_8", 7.870, 0.267),
    ("bv3_disc_outer_smooth_s_e45_f0_s4_16", 2.152, 0.057),
    ("bv3_disc_outer_smooth_s_e45_f0_s4_8", 2.185, 0.058),
    ("bv3_disc_outer_smooth_s_e45_f4_s4_16", 8.053, 0.146),
    ("bv3_disc_outer_smooth_s_e45_f4_s4_8", 8.050, 0.147),
    ("bv3_disc_outer_smooth_s_e60_f4_s4_16", 3.407, 0.057),
    ("bv3_disc_outer_smooth_s_e60_f4_s4_8", 3.437, 0.057),
    ("bv3_disc_outer_smooth_s_e90_f0_s1_16", 5.721, 0.088),
    ("bv3_disc_outer_smooth_s_e90_f0_s1_8", 5.739, 0.088),
    ("bv3_disc_outer_smooth_s_e90_f0_s4_16", 2.634, 0.063),
    ("bv3_disc_outer_smooth_s_e90_f0_s4_8", 2.662, 0.064),
    ("bv3_half_outer_smooth_h_e0_f0_s16_16", 15.697, 0.139),
    ("bv3_half_outer_smooth_h_e0_f0_s16_8", 16.001, 0.132),
    ("bv3_half_outer_smooth_h_e0_f0_s1_16", 62.001, 0.194),
    ("bv3_half_outer_smooth_h_e0_f0_s1_8", 62.001, 0.194),
    ("bv3_half_outer_smooth_h_e0_f0_s32_16", 15.565, 0.263),
    ("bv3_half_outer_smooth_h_e0_f0_s32_8", 16.001, 0.244),
    ("bv3_half_outer_smooth_h_e0_f0_s4_16", 32.001, 0.103),
    ("bv3_half_outer_smooth_h_e0_f0_s4_8", 32.001, 0.101),
    ("bv3_half_outer_smooth_h_e45_f0_s16_16", 67.195, 0.935),
    ("bv3_half_outer_smooth_h_e45_f0_s16_8", 67.001, 0.938),
    ("bv3_half_outer_smooth_h_e45_f0_s4_16", 44.180, 0.144),
    ("bv3_half_outer_smooth_h_e45_f0_s4_8", 44.001, 0.144),
    ("bv3_half_outer_smooth_h_e45_f4_s16_16", 63.195, 0.885),
    ("bv3_half_outer_smooth_h_e45_f4_s16_8", 63.001, 0.882),
    ("bv3_half_outer_smooth_h_e45_f4_s4_16", 16.180, 0.057),
    ("bv3_half_outer_smooth_h_e45_f4_s4_8", 16.001, 0.057),
    ("bv3_half_outer_smooth_h_e60_f0_s16_16", 115.195, 1.839),
    ("bv3_half_outer_smooth_h_e60_f0_s16_8", 115.001, 1.841),
    ("bv3_half_outer_smooth_h_e60_f0_s4_16", 96.180, 0.307),
    ("bv3_half_outer_smooth_h_e60_f0_s4_8", 96.001, 0.307),
    ("bv3_half_outer_smooth_h_e60_f4_s16_16", 106.195, 1.695),
    ("bv3_half_outer_smooth_h_e60_f4_s16_8", 106.001, 1.697),
    ("bv3_half_outer_smooth_h_e60_f4_s4_16", 33.180, 0.113),
    ("bv3_half_outer_smooth_h_e60_f4_s4_8", 33.001, 0.110),
    ("bv3_half_outer_smooth_s_e30_f4_s16_16", 13.004, 0.098),
    ("bv3_half_outer_smooth_s_e30_f4_s16_8", 13.001, 0.097),
    ("bv3_half_outer_smooth_s_e30_f4_s4_16", 37.004, 0.304),
    ("bv3_half_outer_smooth_s_e30_f4_s4_8", 37.001, 0.304),
    ("bv3_half_outer_smooth_s_e45_f0_s16_16", 17.098, 0.166),
    ("bv3_half_outer_smooth_s_e45_f0_s16_8", 17.001, 0.169),
    ("bv3_half_outer_smooth_s_e45_f4_s16_16", 19.697, 0.186),
    ("bv3_half_outer_smooth_s_e45_f4_s16_8", 20.001, 0.188),
    ("bv3_half_outer_smooth_s_e60_f0_s16_16", 6.697, 0.050),
    ("bv3_half_outer_smooth_s_e60_f0_s16_8", 7.001, 0.054),
    ("bv3_half_outer_smooth_s_e60_f4_s16_16", 8.697, 0.056),
    ("bv3_half_outer_smooth_s_e60_f4_s16_8", 9.001, 0.057),
    // The confirm take (93 of 96; within the gate: `hp` Size 12 at 70
    // Soften 0 `_s` at both depths and `hp` Size 6 at 70 Soften 0 `_s` at 8
    // bits): the highlight at 70 keeps Photoshop's darker outside half (up
    // to 167 / 3.08); the 32 rows at altitude 40 are today's kernel, which
    // the cut leaves unchanged.
    ("bv_ell_outer_s12_e40_f0_h_16", 62.001, 1.169),
    ("bv_ell_outer_s12_e40_f0_h_8", 62.001, 1.170),
    ("bv_ell_outer_s12_e40_f0_s_16", 64.004, 0.657),
    ("bv_ell_outer_s12_e40_f0_s_8", 64.001, 0.650),
    ("bv_ell_outer_s12_e40_f3_h_16", 61.736, 1.041),
    ("bv_ell_outer_s12_e40_f3_h_8", 62.001, 1.041),
    ("bv_ell_outer_s12_e40_f3_s_16", 64.004, 0.682),
    ("bv_ell_outer_s12_e40_f3_s_8", 64.001, 0.677),
    ("bv_ell_outer_s12_e50_f0_h_16", 82.067, 1.682),
    ("bv_ell_outer_s12_e50_f0_h_8", 82.001, 1.683),
    ("bv_ell_outer_s12_e50_f0_s_16", 58.787, 0.420),
    ("bv_ell_outer_s12_e50_f0_s_8", 59.001, 0.409),
    ("bv_ell_outer_s12_e50_f3_h_16", 77.748, 1.401),
    ("bv_ell_outer_s12_e50_f3_h_8", 78.001, 1.400),
    ("bv_ell_outer_s12_e50_f3_s_16", 57.359, 0.385),
    ("bv_ell_outer_s12_e50_f3_s_8", 57.001, 0.378),
    ("bv_ell_outer_s12_e70_f0_h_16", 163.553, 3.078),
    ("bv_ell_outer_s12_e70_f0_h_8", 164.001, 3.080),
    ("bv_ell_outer_s12_e70_f0_s_16", 37.534, 0.346),
    ("bv_ell_outer_s12_e70_f0_s_8", 38.001, 0.333),
    ("bv_ell_outer_s12_e70_f3_h_16", 146.156, 2.541),
    ("bv_ell_outer_s12_e70_f3_h_8", 146.001, 2.542),
    ("bv_ell_outer_s12_e70_f3_s_16", 36.308, 0.335),
    ("bv_ell_outer_s12_e70_f3_s_8", 36.001, 0.326),
    ("bv_ell_outer_s6_e40_f0_h_16", 61.915, 1.324),
    ("bv_ell_outer_s6_e40_f0_h_8", 62.001, 1.324),
    ("bv_ell_outer_s6_e40_f0_s_16", 64.004, 1.019),
    ("bv_ell_outer_s6_e40_f0_s_8", 64.001, 1.017),
    ("bv_ell_outer_s6_e40_f3_h_16", 61.218, 1.185),
    ("bv_ell_outer_s6_e40_f3_h_8", 61.001, 1.185),
    ("bv_ell_outer_s6_e40_f3_s_16", 61.674, 1.104),
    ("bv_ell_outer_s6_e40_f3_s_8", 62.001, 1.103),
    ("bv_ell_outer_s6_e50_f0_h_16", 87.012, 1.634),
    ("bv_ell_outer_s6_e50_f0_h_8", 87.001, 1.634),
    ("bv_ell_outer_s6_e50_f0_s_16", 57.312, 0.698),
    ("bv_ell_outer_s6_e50_f0_s_8", 58.001, 0.694),
    ("bv_ell_outer_s6_e50_f3_h_16", 78.646, 1.156),
    ("bv_ell_outer_s6_e50_f3_h_8", 79.001, 1.155),
    ("bv_ell_outer_s6_e50_f3_s_16", 53.541, 0.575),
    ("bv_ell_outer_s6_e50_f3_s_8", 54.001, 0.574),
    ("bv_ell_outer_s6_e70_f0_h_16", 167.502, 2.561),
    ("bv_ell_outer_s6_e70_f0_h_8", 167.001, 2.560),
    ("bv_ell_outer_s6_e70_f0_s_16", 36.125, 0.569),
    ("bv_ell_outer_s6_e70_f0_s_8", 36.001, 0.565),
    ("bv_ell_outer_s6_e70_f3_h_16", 119.604, 1.830),
    ("bv_ell_outer_s6_e70_f3_h_8", 120.001, 1.830),
    ("bv_ell_outer_s6_e70_f3_s_16", 33.386, 0.527),
    ("bv_ell_outer_s6_e70_f3_s_8", 33.001, 0.523),
    ("bv_hp_outer_s12_e40_f0_h_16", 57.670, 1.338),
    ("bv_hp_outer_s12_e40_f0_h_8", 58.001, 1.331),
    ("bv_hp_outer_s12_e40_f0_s_16", 27.728, 0.585),
    ("bv_hp_outer_s12_e40_f0_s_8", 28.001, 0.573),
    ("bv_hp_outer_s12_e40_f3_h_16", 53.670, 1.242),
    ("bv_hp_outer_s12_e40_f3_h_8", 54.001, 1.240),
    ("bv_hp_outer_s12_e40_f3_s_16", 29.386, 0.720),
    ("bv_hp_outer_s12_e40_f3_s_8", 29.001, 0.705),
    ("bv_hp_outer_s12_e50_f0_h_16", 79.055, 1.786),
    ("bv_hp_outer_s12_e50_f0_h_8", 79.001, 1.788),
    ("bv_hp_outer_s12_e50_f0_s_16", 15.580, 0.206),
    ("bv_hp_outer_s12_e50_f0_s_8", 16.001, 0.204),
    ("bv_hp_outer_s12_e50_f3_h_16", 72.670, 1.414),
    ("bv_hp_outer_s12_e50_f3_h_8", 73.001, 1.408),
    ("bv_hp_outer_s12_e50_f3_s_16", 17.580, 0.190),
    ("bv_hp_outer_s12_e50_f3_s_8", 18.001, 0.183),
    ("bv_hp_outer_s12_e70_f0_h_16", 147.390, 2.960),
    ("bv_hp_outer_s12_e70_f0_h_8", 147.001, 2.957),
    ("bv_hp_outer_s12_e70_f3_h_16", 136.390, 2.338),
    ("bv_hp_outer_s12_e70_f3_h_8", 136.001, 2.340),
    ("bv_hp_outer_s12_e70_f3_s_16", 7.464, 0.135),
    ("bv_hp_outer_s12_e70_f3_s_8", 7.001, 0.125),
    ("bv_hp_outer_s6_e40_f0_h_16", 58.931, 1.376),
    ("bv_hp_outer_s6_e40_f0_h_8", 59.001, 1.393),
    ("bv_hp_outer_s6_e40_f0_s_16", 27.981, 0.752),
    ("bv_hp_outer_s6_e40_f0_s_8", 28.001, 0.747),
    ("bv_hp_outer_s6_e40_f3_h_16", 59.001, 1.306),
    ("bv_hp_outer_s6_e40_f3_h_8", 59.001, 1.302),
    ("bv_hp_outer_s6_e40_f3_s_16", 44.001, 1.099),
    ("bv_hp_outer_s6_e40_f3_s_8", 44.001, 1.085),
    ("bv_hp_outer_s6_e50_f0_h_16", 81.931, 1.081),
    ("bv_hp_outer_s6_e50_f0_h_8", 82.001, 1.079),
    ("bv_hp_outer_s6_e50_f0_s_16", 5.931, 0.099),
    ("bv_hp_outer_s6_e50_f0_s_8", 6.001, 0.092),
    ("bv_hp_outer_s6_e50_f3_h_16", 67.841, 0.626),
    ("bv_hp_outer_s6_e50_f3_h_8", 68.001, 0.629),
    ("bv_hp_outer_s6_e50_f3_s_16", 11.324, 0.097),
    ("bv_hp_outer_s6_e50_f3_s_8", 11.001, 0.097),
    ("bv_hp_outer_s6_e70_f0_h_16", 149.386, 1.631),
    ("bv_hp_outer_s6_e70_f0_h_8", 149.001, 1.630),
    ("bv_hp_outer_s6_e70_f0_s_16", 2.160, 0.093),
    ("bv_hp_outer_s6_e70_f3_h_16", 109.588, 1.224),
    ("bv_hp_outer_s6_e70_f3_h_8", 110.001, 1.225),
    ("bv_hp_outer_s6_e70_f3_s_16", 14.250, 0.233),
    ("bv_hp_outer_s6_e70_f3_s_8", 14.001, 0.232),
];

/// `None` when the row meets the gate or its pin, else why not.
fn check_row(name: &str, (max, mean): (f64, f64)) -> Option<String> {
    eprintln!("{name}: {max:.4} / {mean:.4}");
    if let Some(&(_, pm, pn)) = PINNED.iter().find(|(n, _, _)| *n == name) {
        return (max > pm || mean > pn)
            .then(|| format!("{name}: max {max:.4} (pinned {pm}), mean {mean:.4} (pinned {pn})"));
    }
    (max > MAX || mean > MEAN)
        .then(|| format!("{name}: max {max:.4}, mean {mean:.4} against {MAX} / {MEAN}"))
}

fn assert_none_failed(failed: &[String]) {
    assert!(
        failed.is_empty(),
        "{} rows fail:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

#[test]
fn the_take3_outer_bevel_rows_meet_the_gate_or_their_pins() {
    let fixture = take3_fixture();
    let rows = fixture.as_object().unwrap();
    assert_eq!(rows.len(), 160, "take3's 160 Smooth Outer Bevel rows");
    let failed: Vec<String> = rows
        .iter()
        .filter_map(|(name, row)| check_row(name, take3_row(name, row)))
        .collect();
    assert_none_failed(&failed);
}

// ------------------------------------------------------ the confirm take --

fn take_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h).join("devel/doctored-fixtures/photoshop/effect-kernels-578-p2/take1")
    })
}

/// The unit alpha of every pixel of a picture.
fn picture_alpha(path: &Path) -> (u32, u32, Vec<f64>) {
    let img = image::open(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .to_rgba16();
    let (w, h) = img.dimensions();
    let a = img.pixels().map(|p| f64::from(p.0[3]) / 65535.0).collect();
    (w, h, a)
}

/// Each pixel's Euclidean distance between pixel centres to the nearest pixel
/// on the other side of the 0.5 contour (the runner's `max(d_out, d_in)`),
/// by brute force over the pixels that touch the other side.
fn contour_distance(w: u32, h: u32, a: &[f64]) -> Vec<f64> {
    let inside = |x: i64, y: i64| a[(y * i64::from(w) + x) as usize] >= 0.5;
    let (wi, hi) = (i64::from(w), i64::from(h));
    let mut edge: [Vec<(i64, i64)>; 2] = [Vec::new(), Vec::new()];
    for y in 0..hi {
        for x in 0..wi {
            let me = inside(x, y);
            let touches = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                (0..wi).contains(&nx) && (0..hi).contains(&ny) && inside(nx, ny) != me
            });
            if touches {
                edge[usize::from(me)].push((x, y));
            }
        }
    }
    let mut out = Vec::with_capacity(a.len());
    for y in 0..hi {
        for x in 0..wi {
            let other = &edge[usize::from(!inside(x, y))];
            let d2 = other
                .iter()
                .map(|&(qx, qy)| (qx - x).pow(2) + (qy - y).pow(2))
                .min()
                .unwrap_or(i64::MAX);
            out.push((d2 as f64).sqrt());
        }
    }
    out
}

/// One confirm row: the render of its picture with its style against
/// Photoshop's readback, on its channel over the ROW (module doc).
fn take_row(take: &Path, name: &str, row: &Value, fields: &mut Fields) -> (f64, f64) {
    let pic = row["picture"].as_str().unwrap();
    let depth = row["depth"].as_u64().unwrap() as u32;
    let channel = match row["side"].as_str().unwrap() {
        "h" => 0,
        "s" => 2,
        other => panic!("{name}: unknown side {other}"),
    };
    let radius = 2.0 * (row["size"].as_f64().unwrap() + row["soften"].as_f64().unwrap()) + 2.0;
    let path = take.join(format!("inputs/pictures/{pic}_source_{depth}.png"));
    let key = (pic.to_string(), depth);
    if !fields.iter().any(|(k, _)| *k == key) {
        let (w, h, a) = picture_alpha(&path);
        let d = contour_distance(w, h, &a);
        fields.push((key.clone(), (w, h, a, d)));
    }
    let (w, h, a, dist) = &fields.iter().find(|(k, _)| *k == key).unwrap().1;
    let ours = render_rgb_alpha(depth, *w, *h, a, [0.0; 3], [0.0; 3], &row["style"]);
    let theirs = image::open(take.join(format!("rows/{name}.png")))
        .expect("the row's readback")
        .to_rgba16();
    assert_eq!(
        theirs.dimensions(),
        (*w, *h),
        "{name}: the readback's size is the picture's"
    );
    let (mut o, mut t) = (Vec::new(), Vec::new());
    for ((mine, ps), &d) in ours.iter().zip(theirs.pixels()).zip(dist) {
        if d <= radius {
            o.push(mine[channel]);
            t.push(f64::from(ps.0[channel]) / 257.0);
        }
    }
    error(&o, &t, name)
}

type Fields = Vec<((String, u32), (u32, u32, Vec<f64>, Vec<f64>))>;

#[test]
fn the_confirm_take_outer_bevel_rows_meet_the_gate_or_their_pins() {
    let Some(take) = take_dir() else {
        eprintln!("SKIP: HOME is unset, so the confirm take cannot be found");
        return;
    };
    let rows_path = take.join("rows.json");
    if !rows_path.is_file() {
        eprintln!("SKIP: {} absent", rows_path.display());
        return;
    }
    let rows: Value = serde_json::from_str(&std::fs::read_to_string(&rows_path).unwrap()).unwrap();
    let rows = rows.as_object().unwrap();
    assert_eq!(rows.len(), 288, "144 cases per depth");
    let mut fields = Fields::new();
    let mut seen = 0;
    let mut failed = Vec::new();
    for (name, row) in rows {
        if !name.starts_with("bv_ell_outer_") && !name.starts_with("bv_hp_outer_") {
            continue;
        }
        seen += 1;
        failed.extend(check_row(name, take_row(&take, name, row, &mut fields)));
    }
    assert_eq!(seen, 96, "48 Smooth Outer Bevel cases per depth");
    assert_none_failed(&failed);
}
