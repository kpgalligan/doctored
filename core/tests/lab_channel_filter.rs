//! #1 Lab phase 2f, the luma-reading filters on a Lab Color layer
//! (`lab_channel_filter`), through the FFI. Oracles written here, never the
//! Lab path:
//!
//! - Lens Blur, Reduce Noise and Smart Sharpen: each Lab channel k of the
//!   result equals channel G of the same filter run on an sRGB document of
//!   `[c_k, c_k, c_k, α]`, exactly, at 8 and 16 bits, everywhere and under a
//!   half-canvas coverage; α equals the L\* document's α (Lens Blur's
//!   alpha reads each pass's colour, and the result keeps pass 0's);
//! - Emboss and Edge Detect: L\* equals that G; a\* and b\* are literal 32896
//!   at full coverage, and under the half coverage within 1 code of the hand
//!   `old + t·(32896 − old)`, t the coverage ÷ 65535, on a layer whose a\*
//!   and b\* start away from 0;
//! - Reduce Noise `{luminance: 0, color: 1}` changes nothing, and
//!   `{luminance: 0.05, color: 0}` equals `{luminance: 0.05, color: 1}` code
//!   for code (`color` has no effect on Lab);
//! - Offset with `undefined_areas` fill: the vacated columns are literal
//!   `[32768, 32896, 32896, 65535]` (fill 0.5's L\*, a\* = b\* = 0), the rest
//!   the source shifted;
//! - the plan equals its formula (the RGB plan of an sRGB layer of the same
//!   size + layer × 26 + canvas × 21 + 2 MiB), the descriptor's
//!   `lab.filter_plans` names it, and a limit one byte short refuses;
//! - Levels Auto (a backdrop scoped analysis's `auto`, tone, clip 0) answers
//!   black[0] and white[0] = the layer's lowest and highest L\* code ÷ 65535
//!   and a\*, b\* (0, 1, 1); Auto Color refuses by name.
//!
//! Bounds declared before measuring.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::ffi_scoped_statistics::{
    dr_precision_doc_scoped_analysis, dr_precision_doc_scoped_analysis_plan,
};
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{free, plan_json, result, selected, LIMIT};

const W: u32 = 24;
const H: u32 = 16;
/// a\* = b\* = 0 (the ICC v4 16-bit Lab encoding's 128 × 257).
const NEUTRAL: u16 = 32896;
/// `precision_image::BOOKKEEPING_BYTES`, 1 MiB.
const BOOKKEEPING: u64 = 1024 * 1024;

/// An sRGB document of one layer holding exactly `pixels` (W × H).
fn srgb_doc(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

/// A pattern of strong colours (a\* and b\* well away from 0) with an L\*
/// edge, opaque but (`band`) for a band of partial alpha on the right,
/// through Mode ▸ Lab Color at `depth`.
fn lab_pattern(depth: u32, band: bool) -> *mut PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let bright = x >= W / 3;
            let r = if bright {
                60000
            } else {
                9000 + (y * 900) as u16
            };
            let g = ((y * 4096 + x * 977) % 30000) as u16;
            let b = (((W - 1 - x) * 2731 + y * 3001) % 65536) as u16;
            let a = if band && x >= W - 5 {
                (20000 + y * 2500) as u16
            } else {
                65535
            };
            [r, g, b, a]
        })
        .collect();
    let rgb = srgb_doc(pixels);
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "to Lab: {}", take_err_string(err));
    free(rgb);
    lab
}

fn lab_doc(depth: u32) -> *mut PrecisionDocument {
    lab_pattern(depth, true)
}

fn codes(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn run(
    doc: *const PrecisionDocument,
    op: &str,
    params: &Value,
    coverage: &[u16],
) -> Result<Option<*mut PrecisionDocument>, String> {
    selected(doc, &json!({"layer":0,"op":op,"params":params}), coverage)
}

fn changed(
    doc: *const PrecisionDocument,
    op: &str,
    params: &Value,
    coverage: &[u16],
) -> Vec<[u16; 4]> {
    let out = run(doc, op, params, coverage)
        .unwrap_or_else(|e| panic!("{op}: {e}"))
        .unwrap_or_else(|| panic!("{op} changed nothing"));
    let got = codes(out);
    free(out);
    got
}

/// The plan's JSON for `op` with `params` on layer 0 under `limit`.
fn plan(
    doc: *const PrecisionDocument,
    op: &str,
    params: &Value,
    limit: u64,
) -> Result<Value, String> {
    let text = CString::new(json!({"layer":0,"op":op,"params":params}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_selected_edit_plan(doc, text.as_ptr(), limit, 0, &mut err) };
    plan_json(out, err)
}

fn text_of(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { dr_string_free(text) };
    out
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&text_of(text)).unwrap()
}

/// The left half uncovered, the right half a ramp to full.
fn half_coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let x = i % W;
            if x < W / 2 {
                0
            } else {
                ((x - W / 2 + 1) * 65535 / (W / 2)) as u16
            }
        })
        .collect()
}

/// The filter's one-channel result on channel `k`: the sRGB document of
/// `[c_k, c_k, c_k, α]` filtered by the RGB path.
fn one_channel(
    source: &[[u16; 4]],
    k: usize,
    op: &str,
    params: &Value,
    coverage: &[u16],
) -> Vec<[u16; 4]> {
    let one: Vec<[u16; 4]> = source.iter().map(|p| [p[k], p[k], p[k], p[3]]).collect();
    let rgb = srgb_doc(one.clone());
    let out = run(rgb, op, params, coverage).unwrap();
    let want = out.map_or(one, |d| {
        let c = codes(d);
        free(d);
        c
    });
    free(rgb);
    want
}

#[test]
fn every_channel_is_the_filters_gray_pass() {
    let filters = [
        (
            "lens_blur",
            json!({"radius":3,"brightness":20,"threshold":200}),
        ),
        (
            "reduce_noise",
            json!({"radius":2,"luminance":0.08,"detail":0.2}),
        ),
        ("smart_sharpen", json!({"amount":2.0,"radius":1.5})),
    ];
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let source = codes(lab);
        for coverage in [Vec::new(), half_coverage()] {
            for (op, params) in &filters {
                let what = format!("{op} at {depth}, coverage {}", !coverage.is_empty());
                let got = changed(lab, op, params, &coverage);
                for k in 0..3 {
                    let want = one_channel(&source, k, op, params, &coverage);
                    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                        assert_eq!(g[k], w[1], "{what} pixel {i} channel {k}");
                        // Alpha is the L* pass's (module doc).
                        if k == 0 {
                            assert_eq!(g[3], w[3], "{what} pixel {i} alpha");
                        }
                    }
                }
            }
        }
        free(lab);
    }
}

#[test]
fn emboss_and_edge_detect_run_on_lightness_and_neutralize_a_and_b() {
    let lab = lab_doc(16);
    let source = codes(lab);
    // The layer's a* and b* start away from 0.
    assert!(source
        .iter()
        .any(|p| p[1].abs_diff(NEUTRAL) > 2000 && p[2].abs_diff(NEUTRAL) > 2000));
    let half = half_coverage();
    for op in ["emboss", "edge_detect"] {
        let params = json!({});
        // Full coverage: a* = b* = 0 exactly.
        let got = changed(lab, op, &params, &[]);
        let want = one_channel(&source, 0, op, &params, &[]);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(g[0], w[1], "{op} pixel {i} L*");
            assert_eq!(g[3], w[3], "{op} pixel {i} alpha");
            assert_eq!([g[1], g[2]], [NEUTRAL; 2], "{op} pixel {i} a*, b*");
        }
        // Half coverage: a* and b* toward the neutral by the coverage.
        let got = changed(lab, op, &params, &half);
        let want = one_channel(&source, 0, op, &params, &half);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(g[0], w[1], "{op} half pixel {i} L*");
            let t = f64::from(half[i]) / 65535.0;
            for c in 1..3 {
                let old = f64::from(source[i][c]);
                let hand = (old + t * (f64::from(NEUTRAL) - old)).round();
                assert!(
                    (f64::from(g[c]) - hand).abs() <= 1.0,
                    "{op} half pixel {i} channel {c}: {} against {hand}",
                    g[c]
                );
            }
        }
    }
    free(lab);
}

#[test]
fn reduce_noise_color_has_no_effect_on_lab() {
    let lab = lab_doc(16);
    assert!(
        run(lab, "reduce_noise", &json!({"luminance":0,"color":1}), &[])
            .unwrap()
            .is_none(),
        "the luminance half off changes nothing"
    );
    let a = changed(
        lab,
        "reduce_noise",
        &json!({"luminance":0.05,"color":0}),
        &[],
    );
    let b = changed(
        lab,
        "reduce_noise",
        &json!({"luminance":0.05,"color":1}),
        &[],
    );
    assert_eq!(a, b);
    free(lab);
}

#[test]
fn offset_fill_fills_the_neutral_of_its_lightness() {
    let lab = lab_doc(16);
    let source = codes(lab);
    let got = changed(
        lab,
        "offset",
        &json!({"horizontal":3,"vertical":0,"undefined_areas":"fill","fill":0.5}),
        &[],
    );
    for (i, g) in got.iter().enumerate() {
        let x = i as u32 % W;
        if x < 3 {
            assert_eq!(*g, [32768, NEUTRAL, NEUTRAL, 65535], "vacated pixel {i}");
        } else {
            assert_eq!(*g, source[i - 3], "moved pixel {i}");
        }
    }
    free(lab);
}

#[test]
fn the_plan_is_its_formula_and_the_descriptor_names_it() {
    let lab = lab_doc(16);
    let stand_in = srgb_doc(vec![[0; 4]; (W * H) as usize]);
    let d = descriptor(lab);
    for op in [
        "emboss",
        "edge_detect",
        "lens_blur",
        "reduce_noise",
        "smart_sharpen",
    ] {
        let params = json!({});
        let rgb = plan(stand_in, op, &params, LIMIT).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap();
        let pixels = u64::from(W * H);
        let want = rgb + pixels * 26 + pixels * 21 + 2 * BOOKKEEPING;
        let got = plan(lab, op, &params, LIMIT).unwrap();
        assert_eq!(got["scratch_bytes"], want, "{op}");
        assert_eq!(got["output_bytes"], pixels * 8, "{op}");
        assert_eq!(got["input_samples"], pixels, "{op}");
        assert_eq!(d["lab"]["filter_plans"][op], want, "{op} descriptor");
        // One byte short of retained + scratch refuses, at the plan and the edit.
        let short = got["retained_bytes"].as_u64().unwrap() + want - 1;
        assert!(plan(lab, op, &params, short).is_err(), "{op} short plan");
        let text = CString::new(json!({"layer":0,"op":op,"params":params}).to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_selected_edit(
                lab,
                text.as_ptr(),
                ptr::null(),
                0,
                short,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(result(out, err).is_err(), "{op} short edit");
    }
    free(stand_in);
    free(lab);
}

fn analysis(doc: *const PrecisionDocument, mode: &str) -> Result<Value, String> {
    let request = json!({"operation":"histogram","source":"backdrop","below":1,
        "auto":{"mode":mode,"clip":0,"insert":{"anchor":0,"position":"above"}}});
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_scoped_analysis_plan(doc, text.as_ptr(), &mut err) };
    plan_json(plan, err)?;
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_scoped_analysis(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    plan_json(out, err)
}

#[test]
fn levels_auto_tone_stretches_lightness_and_auto_color_refuses() {
    // Opaque, so the backdrop's codes are the layer's.
    let lab = lab_pattern(16, false);
    let source = codes(lab);
    let lows = source.iter().map(|p| p[0]);
    let lo = lows.clone().min().unwrap();
    let hi = lows.max().unwrap();
    assert!(lo < hi);
    let auto = analysis(lab, "tone").unwrap()["auto"].clone();
    let unit = |code: u16| json!(f32::from(code) / 65535.0);
    assert_eq!(auto["black"], json!([unit(lo), 0.0, 0.0]), "{auto}");
    assert_eq!(auto["white"], json!([unit(hi), 1.0, 1.0]), "{auto}");
    assert_eq!(auto["gamma"], json!([1.0, 1.0, 1.0]), "{auto}");
    assert_eq!(
        analysis(lab, "color").unwrap_err(),
        "Auto Color is unavailable in a Lab Color document."
    );
    free(lab);
}
