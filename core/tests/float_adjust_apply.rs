//! Destructive Image ▸ Adjustments in a 32-bit float
//! document, through `dr_precision_doc_selected_edit` (and its plan) and
//! `dr_precision_doc_apply_adjustment`. Photoshop 26.10.0 applies each
//! adjustment it keeps at 32 bits with the same result as its layer
//! (`judged.json`, Applied-same), so the oracle for a full-coverage apply is
//! the layer render itself (whose own forms `float_adjustments.rs` checks
//! against binary64 oracles), and a partial coverage is checked against an
//! f64 lerp written here. Every op the layer refuses (Invert,
//! Shadows/Highlights, the legacy White Balance) refuses by name; every op
//! the layer admits applies, equal to its layer (Hue/Saturation in every
//! setting, the `document_v2` White Balance and the closed and display
//! forms of `float_display_adjustments.rs`). Curves applies in point form
//! and in map (pencil) form (take3: Applied-same); Color Lookup applies the
//! tables its layer admits (take3: CL-apply `same`); Black & White with Tint
//! applies (take6: BT32-apply `same`); and `bcs`, legacy and modern, applies
//! (destructive equals layer, by decision).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::ffi_precision_color::{
    dr_precision_doc_adjustment_plan, dr_precision_doc_apply_adjustment,
};
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_capability::require;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

use crate::float_fixture::float_doc;
use crate::float_lookup_tint_map::{cross_5_params, m1, map_params, sqrt_map};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// Neutrals, colours, over-range and negative patches.
const PATCHES: [[f32; 3]; 16] = [
    [0.0, 0.0, 0.0],
    [0.02, 0.02, 0.02],
    [0.18, 0.18, 0.18],
    [0.5, 0.5, 0.5],
    [1.0, 1.0, 1.0],
    [0.5, 0.2, 0.1],
    [0.05, 0.2, 0.6],
    [0.3, 0.6, 0.2],
    [1.5, 1.5, 1.5],
    [16.0, 16.0, 16.0],
    [2.0, 0.5, 0.25],
    [4.0, 0.3, 2.0],
    [-0.25, 0.5, 1.0],
    [0.5, -0.1, 0.2],
    [-0.05, -0.05, -0.05],
    [0.8, 0.7, 0.05],
];
const N: u32 = PATCHES.len() as u32;

/// Row 0 opaque, row 1 at alpha 0.5, row 2 at alpha 2^-20, row 3
/// transparent with its colour left in place.
fn alphas() -> [f32; 4] {
    [1.0, 0.5, 2f32.powi(-20), 0.0]
}

fn scene() -> PrecisionDocument {
    let pixels: Vec<[f32; 4]> = alphas()
        .iter()
        .flat_map(|&a| PATCHES.map(|p| [p[0], p[1], p[2], a]))
        .collect();
    float_doc(N, 4, &IccProfile::srgb(), &[pixels])
}

fn description(op: &str, params: &Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

/// One parameter set per op the float mode applies (take2's sets).
fn applied() -> Vec<(&'static str, Value)> {
    vec![
        (
            "levels",
            json!({"black": 20.0 / 255.0, "white": 230.0 / 255.0, "gamma": 1.4,
                "output_black": 10.0 / 255.0, "output_white": 245.0 / 255.0,
                "r": {"white": 220.0 / 255.0}}),
        ),
        ("black_and_white", json!({"reds": 1.2, "blues": -0.5})),
        // Take6's B3 (BT32-apply `same`).
        (
            "black_and_white",
            json!({"reds": 1.2, "blues": -0.5, "tint": true, "tint_color": "#c83c5a"}),
        ),
        (
            "channel_mixer",
            json!({"red": {"r": 1, "g": 0, "b": 0, "constant": 0.1}}),
        ),
        (
            "photo_filter",
            json!({"color": "#ec8a00", "density": 0.25, "preserve_luminosity": true}),
        ),
        (
            "exposure",
            json!({"exposure": 0.5, "offset": 0.02, "gamma": 0.8}),
        ),
        (
            "curves",
            json!({"rgb": [[0, 0], [64, 48], [192, 214], [255, 255]], "r": [[0, 30], [192, 255]]}),
        ),
        // Take3's M3: a channel map under a master map.
        ("curves", map_params(&m1(), Some(&sqrt_map()))),
        (
            "color_balance",
            json!({"shadows": {"cyan_red": -0.4, "magenta_green": 0.3},
                "midtones": {"cyan_red": 0.2, "magenta_green": -0.5, "yellow_blue": 0.8},
                "highlights": {"cyan_red": 0.6, "yellow_blue": -0.7},
                "preserve_luminosity": false}),
        ),
        // Take3's L4: a 3D table within 0…1 (CL-apply `same`).
        ("color_lookup", cross_5_params()),
        // Both forms apply as their layer does.
        (
            "bcs",
            json!({"brightness": 0.1, "contrast": 0.2, "saturation": 0.1}),
        ),
        (
            "bcs",
            json!({"brightness": 0.3, "contrast": 0.25, "saturation": -0.2, "legacy": false}),
        ),
        // Every op the layer admits applies as its layer does.
        (
            "hue_saturation",
            json!({"colorize": true, "colorize_hue": 310, "colorize_saturation": 0.8}),
        ),
        ("hue_saturation", json!({"saturation": 0.25})),
        ("white_balance", json!({"temperature": 5000})),
        ("hue_rotate", json!({"degrees": 30})),
        ("grayscale", json!({})),
        ("sepia", json!({})),
        ("threshold", json!({"level": 0.5})),
        ("posterize", json!({"levels": 4})),
        ("vibrance", json!({"vibrance": 0.3})),
        ("selective_color", json!({"reds": {"c": -0.3, "y": 0.2}})),
        (
            "gradient_map",
            json!({"gradient": {"stops":[{"position":0,"color":"#200000"},{"position":1,"color":"#ffe0c0"}]},
                "dither": false}),
        ),
        ("replace_color", json!({"colors": ["#808080"], "hue": 30})),
    ]
}

fn take(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

fn request(op: &str, params: &Value) -> CString {
    let text = json!({"layer": 0, "op": "adjustment", "description": description(op, params)});
    CString::new(text.to_string()).unwrap()
}

/// `None` is the exact no-op answer (NULL with no error).
fn owned(
    out: *mut PrecisionDocument,
    err: *mut c_char,
) -> Result<Option<PrecisionDocument>, String> {
    let error = take(err);
    if out.is_null() {
        return if error.is_empty() {
            Ok(None)
        } else {
            Err(error)
        };
    }
    assert!(error.is_empty(), "{error}");
    Ok(Some(*unsafe { Box::from_raw(out) }))
}

fn selected_raw(
    doc: &PrecisionDocument,
    request: &CString,
    coverage: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            request.as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    owned(out, err)
}

fn selected(
    doc: &PrecisionDocument,
    op: &str,
    params: &Value,
    coverage: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    selected_raw(doc, &request(op, params), coverage)
}

fn plan(doc: &PrecisionDocument, op: &str, params: &Value) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit_plan(doc, request(op, params).as_ptr(), LIMIT, 0, &mut err)
    };
    let error = take(err);
    if out.is_null() {
        return Err(error);
    }
    let text = take(out);
    Ok(serde_json::from_str(&text).unwrap())
}

/// `dr_precision_doc_adjustment_plan`'s destructive report for layer 0.
fn destructive_plan(doc: &PrecisionDocument, op: &str, params: &Value) -> Result<Value, String> {
    let text = CString::new(description(op, params)).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_adjustment_plan(doc, 0, text.as_ptr(), true, &mut err) };
    let error = take(err);
    if out.is_null() {
        return Err(error);
    }
    Ok(serde_json::from_str(&take(out)).unwrap())
}

fn apply(
    doc: &PrecisionDocument,
    op: &str,
    params: &Value,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(description(op, params)).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(doc, 0, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    owned(out, err)
}

fn samples(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn render(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    assert_eq!(doc.float_refusal(), None);
    doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn sentence(feature: &str) -> String {
    format!("{feature} is unavailable in a 32-bit float document")
}

#[test]
fn a_full_coverage_apply_equals_the_layer_render_bit_for_bit() {
    let doc = scene();
    for (op, params) in applied() {
        let layered = doc
            .add_adjustment("Adjust", &description(op, &params), 1.0, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let want = render(&layered);
        let by_selection = selected(&doc, op, &params, &[]).unwrap().expect(op);
        assert_eq!(
            bits(&render(&by_selection)),
            bits(&want),
            "{op}: selected edit"
        );
        let full = vec![u16::MAX; (N * 4) as usize];
        let covered = selected(&doc, op, &params, &full).unwrap().expect(op);
        assert_eq!(
            bits(&samples(&covered)),
            bits(&samples(&by_selection)),
            "{op}: a full coverage is no coverage"
        );
        let whole = apply(&doc, op, &params).unwrap().expect(op);
        assert_eq!(
            bits(&samples(&whole)),
            bits(&samples(&by_selection)),
            "{op}: apply_adjustment"
        );
    }
}

#[test]
fn a_partial_coverage_is_the_f64_lerp_toward_the_full_apply() {
    let doc = scene();
    let before = samples(&doc);
    let count = (N * 4) as usize;
    // Half everywhere, then the two exact ends and a sliver mixed in.
    let weights: Vec<u16> = (0..count).map(|i| [32768, 0, u16::MAX, 1][i % 4]).collect();
    for (op, params) in applied() {
        let full = samples(&selected(&doc, op, &params, &[]).unwrap().unwrap());
        for coverage in [vec![32768u16; count], weights.clone()] {
            let out = samples(&selected(&doc, op, &params, &coverage).unwrap().unwrap());
            for i in 0..count {
                let (old, adjusted, got) = (before[i], full[i], out[i]);
                assert_eq!(got[3].to_bits(), old[3].to_bits(), "{op} {i}: alpha");
                match coverage[i] {
                    0 => assert_eq!(got.map(f32::to_bits), old.map(f32::to_bits), "{op} {i}"),
                    u16::MAX => assert_eq!(
                        got.map(f32::to_bits),
                        adjusted.map(f32::to_bits),
                        "{op} {i}"
                    ),
                    weight => {
                        let t = f64::from(weight) / 65535.0;
                        for c in 0..3 {
                            let (o, a) = (f64::from(old[c]), f64::from(adjusted[c]));
                            let want = o + t * (a - o);
                            let got = f64::from(got[c]);
                            assert!(
                                (got - want).abs() <= 1e-6 * want.abs().max(1.0),
                                "{op} {i} c{c} weight {weight}: got {got}, want {want}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn alpha_is_kept_and_a_transparent_pixel_is_untouched() {
    let doc = scene();
    let before = samples(&doc);
    for (op, params) in applied() {
        let out = samples(&selected(&doc, op, &params, &[]).unwrap().unwrap());
        for (i, (got, old)) in out.iter().zip(&before).enumerate() {
            assert_eq!(got[3].to_bits(), old[3].to_bits(), "{op} {i}: alpha");
            if old[3] == 0.0 {
                assert_eq!(got.map(f32::to_bits), old.map(f32::to_bits), "{op} {i}");
            }
        }
        assert!(
            out[..N as usize] != before[..N as usize],
            "{op}: the opaque row changed"
        );
    }
}

#[test]
fn an_identity_or_unchanged_apply_gives_none() {
    let doc = scene();
    for (op, params) in [
        ("levels", json!({})),
        ("channel_mixer", json!({})),
        ("photo_filter", json!({"density": 0.0})),
        ("exposure", json!({})),
        ("curves", json!({})),
        ("bcs", json!({})),
        ("bcs", json!({"legacy": false})),
    ] {
        assert!(selected(&doc, op, &params, &[]).unwrap().is_none(), "{op}");
        assert!(apply(&doc, op, &params).unwrap().is_none(), "{op}");
    }
    let unselected = vec![0u16; (N * 4) as usize];
    for (op, params) in applied() {
        assert!(
            selected(&doc, op, &params, &unselected).unwrap().is_none(),
            "{op}: nothing selected"
        );
    }
}

#[test]
fn every_refused_op_refuses_by_name() {
    let doc = scene();
    let cases = [
        ("invert", json!({}), sentence("the invert adjustment")),
        (
            "shadows_highlights",
            json!({"shadows": {"amount": 0.35}}),
            sentence("the shadows_highlights adjustment"),
        ),
        (
            "white_balance",
            json!({"temperature": 5000, "evaluation": "legacy_srgb"}),
            sentence("the white_balance adjustment with legacy evaluation"),
        ),
    ];
    for (op, params, want) in cases {
        assert_eq!(
            plan(&doc, op, &params).err(),
            Some(want.clone()),
            "{op} plan"
        );
        assert_eq!(
            selected(&doc, op, &params, &[]).err(),
            Some(want.clone()),
            "{op} selected"
        );
        assert_eq!(
            destructive_plan(&doc, op, &params).err(),
            Some(want.clone()),
            "{op} destructive plan"
        );
        assert_eq!(apply(&doc, op, &params).err(), Some(want), "{op} apply");
    }
    // The automatic tones refuse by the operator table's own row.
    let row = "automatic tone correction (Auto Tone, Auto Contrast, Auto Color and the Levels dialog's Auto buttons)";
    for op in ["auto_tone", "auto_contrast", "auto_color"] {
        let auto = CString::new(json!({"layer": 0, "op": op, "params": {}}).to_string()).unwrap();
        assert_eq!(
            selected_raw(&doc, &auto, &[]).err(),
            Some(sentence(row)),
            "{op}"
        );
    }
    let table: Value = serde_json::from_str(&take(unsafe { dr_float_operators() })).unwrap();
    let reason = table["refused"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["operator"] == row)
        .and_then(|entry| entry["reason"].as_str())
        .expect("the table names REFUSED_AUTO");
    assert!(
        reason.starts_with(
            "Auto Tone, Auto Contrast and Auto Color are unavailable on a 32-bit document"
        ),
        "{reason}"
    );
}

#[test]
fn an_overflow_refuses_as_nonfinite_or_out_of_range() {
    let pixels = vec![[65504.0, 1.0, 1.0, 1.0]; 4];
    let doc = float_doc(4, 1, &IccProfile::srgb(), &[pixels]);
    assert_eq!(
        selected(&doc, "levels", &json!({"gamma": 0.1}), &[])
            .err()
            .as_deref(),
        Some("high-depth adjustment produced nonfinite RGB")
    );
    assert_eq!(
        selected(&doc, "exposure", &json!({"exposure": 4.0}), &[])
            .err()
            .as_deref(),
        Some("32-bit float samples must be finite within ±65504 with alpha in [0,1]")
    );
}

#[test]
fn the_plan_reports_the_float_output_and_the_table_names_the_apply() {
    let doc = scene();
    let count = u64::from(N) * 4;
    for (op, params) in applied() {
        let plan = plan(&doc, op, &params).unwrap();
        assert_eq!(plan["output_bytes"], count * 16, "{op}");
        assert_eq!(plan["input_samples"], count, "{op}");
        assert_eq!(plan["retained_bytes"], doc.retained_bytes(), "{op}");
    }
    let table: Value = serde_json::from_str(&take(unsafe { dr_float_operators() })).unwrap();
    let admitted = table["admitted"].as_array().unwrap();
    for op in [
        "levels",
        "black_and_white",
        "channel_mixer",
        "photo_filter",
        "exposure",
        "curves",
        "color_balance",
        "color_lookup",
        "bcs",
        "white_balance",
        "hue_saturation",
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
        assert!(admitted.contains(&json!(format!("{op}_apply"))), "{op}");
    }
    for op in ["invert", "shadows_highlights"] {
        assert!(!admitted.contains(&json!(format!("{op}_apply"))), "{op}");
    }
    let refused: Vec<&str> = table["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["operator"].as_str().unwrap())
        .collect();
    for row in [
        "the invert adjustment",
        "the shadows_highlights adjustment",
        "the white_balance adjustment with legacy evaluation",
    ] {
        assert!(refused.contains(&row), "{row}");
    }
    assert!(!refused.iter().any(|row| row.starts_with("applying")));
    assert!(require("adjustment_apply", "raster", 32).is_ok());
    assert!(require("adjustment_apply", "mask", 32).is_err());
}
