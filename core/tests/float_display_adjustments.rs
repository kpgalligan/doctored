//! The adjustments a 32-bit float document admits beyond the fitted
//! forms, each under one stated linear-light rule, rendered as
//! adjustment layers through `add_adjustment`:
//!
//! - Hue Rotate and Sepia: their 3×3 matrix on the stored values, no clamp;
//!   Grayscale: the Rec. 709 luma on all three channels — against the closed
//!   forms written here (the W3C `feColorMatrix` hue rotation, the classic
//!   sepia matrix).
//! - Threshold and Posterize (display-mapped): `D(master(E(clamp(v,0,1))))`,
//!   against their closed forms through the sRGB curve written out.
//! - Vibrance, Selective Color, Replace Color, Gradient Map and a
//!   Hue/Saturation band (display-scaled, Gradient Map display-mapped): the
//!   oracle is the master run by a 16-bit ENCODED document's render of the
//!   `E(c)` codes, read back, decoded by this file's D and rule-scaled
//!   (`m·(D(·) + min(c,0))` with `m = max(1, max rgb)`, `c = rgb/m`), within
//!   `5e-4·max(1, |want|)`.
//! - Homogeneity: out(k·p) = k·out(p) for p with max 1 and k = 2 and 8.
//!
//! Every patch set includes 1.0, 2.0, 8.0, 0.999, −0.25 and 0.18, declared
//! before running. The destructive apply of each of these equals its layer:
//! `float_adjust_apply.rs`'s `applied()` carries every one of them through
//! `a_full_coverage_apply_equals_the_layer_render_bit_for_bit`. Invert,
//! Shadows/Highlights and the legacy White Balance refuse by name
//! (`float_adjustments.rs`), and their operator-table rows give a reason
//! that does not cite Photoshop (below).

use std::ffi::CStr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::icc::IccProfile;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

use crate::float_adjustments::{add, float_row, render};
use crate::native_scalar as native;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// The patches, declared before running: every listed value on each channel
/// at least once, colours and grays, above white and below zero.
const PATCHES: [[f32; 3]; 8] = [
    [1.0, 1.0, 1.0],
    [2.0, 0.5, 0.25],
    [8.0, 1.0, -0.25],
    [0.999, 0.18, 0.5],
    [0.18, -0.25, 1.0],
    [-0.25, 2.0, 0.18],
    [0.5, 8.0, 0.999],
    [0.18, 0.18, 0.18],
];

/// The sRGB encode (E), written out from IEC 61966-2.1.
fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// The sRGB decode (D), written out from IEC 61966-2.1.
fn decode(e: f64) -> f64 {
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

fn wide(p: [f32; 3]) -> [f64; 3] {
    p.map(f64::from)
}

fn assert_near(got: [f32; 4], want: [f64; 3], tol: f64, what: &str) {
    for c in 0..3 {
        let err = (f64::from(got[c]) - want[c]).abs();
        assert!(
            err <= tol * want[c].abs().max(1.0),
            "{what} channel {c}: got {got:?}, want {want:?}"
        );
    }
    assert_eq!(got[3], 1.0, "{what}: alpha");
}

/// `op` over [`PATCHES`] in an sRGB float document.
fn rendered(op: &str, params: &Value) -> Vec<[f32; 4]> {
    let srgb = IccProfile::srgb();
    render(&add(&float_row(srgb.bytes(), &PATCHES), op, params, 1.0))
}

// --------------------------------------------------------- closed forms --

/// The W3C `feColorMatrix` hue rotation by `degrees`.
fn hue_matrix(degrees: f64) -> [[f64; 3]; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    [
        [
            0.213 + c * 0.787 - s * 0.213,
            0.715 - c * 0.715 - s * 0.715,
            0.072 - c * 0.072 + s * 0.928,
        ],
        [
            0.213 - c * 0.213 + s * 0.143,
            0.715 + c * 0.285 + s * 0.140,
            0.072 - c * 0.072 - s * 0.283,
        ],
        [
            0.213 - c * 0.213 - s * 0.787,
            0.715 - c * 0.715 + s * 0.715,
            0.072 + c * 0.928 + s * 0.072,
        ],
    ]
}

/// A closed form over one linear triple.
type Oracle = Box<dyn Fn([f64; 3]) -> [f64; 3]>;

fn times(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

#[test]
fn hue_rotate_sepia_and_grayscale_follow_their_closed_forms_unclamped() {
    let sepia = [
        [0.393, 0.769, 0.189],
        [0.349, 0.686, 0.168],
        [0.272, 0.534, 0.131],
    ];
    let luma = |v: [f64; 3]| 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
    let cases: [(&str, Value, Oracle); 4] = [
        (
            "hue_rotate",
            json!({"degrees": 30}),
            Box::new(|v| times(hue_matrix(30.0), v)),
        ),
        (
            "hue_rotate",
            json!({"degrees": -120}),
            Box::new(|v| times(hue_matrix(-120.0), v)),
        ),
        ("sepia", json!({}), Box::new(move |v| times(sepia, v))),
        ("grayscale", json!({}), Box::new(move |v| [luma(v); 3])),
    ];
    for (op, params, oracle) in cases {
        let out = rendered(op, &params);
        for (patch, got) in PATCHES.iter().zip(&out) {
            assert_near(
                *got,
                oracle(wide(*patch)),
                1e-5,
                &format!("{op} {params} {patch:?}"),
            );
        }
    }
    // Above white survives: 8.0 under Sepia is far above 1.
    let out = rendered("sepia", &json!({}));
    assert!(out[2][0] > 3.0, "{:?}", out[2]);
}

#[test]
fn threshold_and_posterize_map_through_the_display_curve() {
    let luma = |e: [f64; 3]| 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
    // No patch's encoded luma lies within 0.01 of the level.
    let level = 0.5;
    for patch in PATCHES {
        assert!(
            (luma(wide(patch).map(encode)) - level).abs() > 0.01,
            "{patch:?}"
        );
    }
    let out = rendered("threshold", &json!({"level": level}));
    for (patch, got) in PATCHES.iter().zip(&out) {
        let v = if luma(wide(*patch).map(encode)) >= level {
            1.0
        } else {
            0.0
        };
        // D(1) = 1 and D(0) = 0: above white classifies as white, wholly
        // the master's output.
        assert_near(*got, [v; 3], 1e-6, &format!("threshold {patch:?}"));
    }
    let out = rendered("posterize", &json!({"levels": 4}));
    for (patch, got) in PATCHES.iter().zip(&out) {
        // Photoshop's bins (#578): k = min(3, floor(4 E)), out k/3.
        let want = wide(*patch).map(|v| decode((encode(v) * 4.0).floor().clamp(0.0, 3.0) / 3.0));
        assert_near(*got, want, 5e-5, &format!("posterize {patch:?}"));
    }
}

// --------------------------------------------- the 16-bit master oracle --

#[derive(Clone, Copy, PartialEq, Debug)]
enum Rule {
    Mapped,
    Scaled,
}

/// The oracle of a display form (module doc): the 16-bit encoded render of
/// `op` over each patch's `E(c)` codes, decoded and rule-scaled.
fn display_oracle(op: &str, params: &Value, rule: Rule) -> Vec<[f64; 3]> {
    let parts: Vec<(f64, [f64; 3])> = PATCHES
        .iter()
        .map(|p| {
            let p = wide(*p);
            match rule {
                Rule::Mapped => (1.0, p.map(|v| v.clamp(0.0, 1.0))),
                Rule::Scaled => {
                    let m = p.iter().fold(1.0_f64, |m, v| m.max(*v));
                    (m, p.map(|v| v / m))
                }
            }
        })
        .collect();
    let codes: Vec<[u16; 4]> = parts
        .iter()
        .map(|(_, c)| {
            let e = c.map(|v| (encode(v.max(0.0)) * 65535.0).round() as u16);
            [e[0], e[1], e[2], 65535]
        })
        .collect();
    let sixteen = native::doc(PATCHES.len() as u32, 1, codes);
    let description =
        json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string();
    let layered = sixteen
        .add_adjustment("Adjust", &description, 1.0, LIMIT, 0, None)
        .unwrap()
        .expect("the 16-bit stack changed");
    let out = layered.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    parts
        .iter()
        .zip(out)
        .map(|((m, c), q)| {
            std::array::from_fn(|k| {
                let decoded = decode(f64::from(q[k]) / 65535.0);
                match rule {
                    Rule::Mapped => decoded,
                    Rule::Scaled => m * (decoded + c[k].min(0.0)),
                }
            })
        })
        .collect()
}

fn display_cases() -> Vec<(&'static str, Value, Rule)> {
    let stops = json!({"stops":[{"position":0,"color":"#102040"},{"position":0.5,"color":"#c04020"},
        {"position":1,"color":"#fff0c0"}]});
    vec![
        ("vibrance", json!({"vibrance": 0.4}), Rule::Scaled),
        (
            "selective_color",
            json!({"reds": {"c": -0.3, "y": 0.2}, "neutrals": {"k": 0.1}}),
            Rule::Scaled,
        ),
        (
            "replace_color",
            json!({"colors": ["#c08060"], "hue": 30, "saturation": 0.2}),
            Rule::Scaled,
        ),
        (
            "gradient_map",
            json!({"gradient": stops, "dither": false}),
            Rule::Mapped,
        ),
        (
            "hue_saturation",
            json!({"bands": {"reds": {"hue": 20, "saturation": 0.3}}}),
            Rule::Scaled,
        ),
        ("hue_saturation", json!({"saturation": 0.35}), Rule::Scaled),
    ]
}

#[test]
fn the_display_forms_match_the_16_bit_master_through_the_curve() {
    for (op, params, rule) in display_cases() {
        let want = display_oracle(op, &params, rule);
        let out = rendered(op, &params);
        for ((patch, got), want) in PATCHES.iter().zip(&out).zip(&want) {
            assert_near(*got, *want, 5e-4, &format!("{op} {params} {patch:?}"));
        }
    }
}

#[test]
fn the_scaled_forms_are_homogeneous_above_white() {
    // Patches whose largest component is exactly 1.
    let unit: [[f32; 3]; 4] = [
        [1.0, 0.5, 0.25],
        [0.18, 1.0, -0.25],
        [0.999, 0.18, 1.0],
        [1.0, 1.0, 1.0],
    ];
    let srgb = IccProfile::srgb();
    for (op, params, rule) in display_cases() {
        if rule != Rule::Scaled {
            continue;
        }
        let base = render(&add(&float_row(srgb.bytes(), &unit), op, &params, 1.0));
        for k in [2.0f32, 8.0] {
            let scaled: Vec<[f32; 3]> = unit.iter().map(|p| p.map(|v| v * k)).collect();
            let out = render(&add(&float_row(srgb.bytes(), &scaled), op, &params, 1.0));
            for (i, (got, one)) in out.iter().zip(&base).enumerate() {
                let want = [one[0], one[1], one[2]].map(|v| f64::from(v) * f64::from(k));
                assert_near(*got, want, 1e-6, &format!("{op} {params} k {k} patch {i}"));
            }
        }
    }
}

// -------------------------------------------------------- the refusals --

#[test]
fn the_refused_adjustments_give_reasons_that_do_not_cite_photoshop() {
    let raw = unsafe { dr_float_operators() };
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(raw) };
    let table: Value = serde_json::from_str(&text).unwrap();
    let refused = table["refused"].as_array().unwrap();
    for operator in [
        "the invert adjustment",
        "the shadows_highlights adjustment",
        "the white_balance adjustment with legacy evaluation",
    ] {
        let row = refused
            .iter()
            .find(|row| row["operator"] == operator)
            .unwrap_or_else(|| panic!("{operator} has a row"));
        assert!(
            !row["reason"].as_str().unwrap().contains("Photoshop"),
            "{operator}: {}",
            row["reason"]
        );
    }
    for op in [
        "hue_rotate",
        "grayscale",
        "sepia",
        "threshold",
        "posterize",
        "vibrance",
        "selective_color",
        "gradient_map",
        "replace_color",
    ] {
        let admitted = table["admitted"].as_array().unwrap();
        assert!(
            admitted.contains(&json!(format!("{op}_adjustment"))),
            "{op}"
        );
        assert!(admitted.contains(&json!(format!("{op}_apply"))), "{op}");
    }
}
