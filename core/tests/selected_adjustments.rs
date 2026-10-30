//! Independent native selected writeback and operator qualification. Expectations
//! use original u16 codes, direct premultiplied arithmetic, and independent
//! neighborhood sums; display pixels never enter these fixtures.
use doctored_core::ffi_selected_adjustment::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
fn doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
fn request(op: &str, p: Value) -> SelectedEdit {
    SelectedEdit::parse(&json!({"layer":0,"op":op,"params":p}).to_string()).unwrap()
}
fn tone(op: &str, p: Value) -> SelectedEdit {
    SelectedEdit::parse(&json!({"layer":0,"op":"adjustment","description":json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":p}).to_string()}).to_string()).unwrap()
}
fn apply(d: &PrecisionDocument, e: &SelectedEdit, c: &[u16]) -> Vec<[u16; 4]> {
    let out = d.selected_edit(e, c, LIMIT, 0, None).unwrap();
    out.as_ref()
        .unwrap_or(d)
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}
fn quantize(v: f64) -> u16 {
    (v.clamp(0., 65535.)).round() as u16
}
#[test]
fn native_code_tone_preserves_exact_unselected_hidden_and_tiny_alpha() {
    let input = vec![
        [10001, 20002, 30003, 0],
        [10002, 20003, 30004, 1],
        [10003, 20004, 30005, 65535],
        [10004, 20005, 30006, 23333],
    ];
    let d = doc(4, 1, input.clone());
    let coverage = [65535, 65535, 32768, 0];
    let out = apply(&d, &tone("bcs", json!({"brightness":2./65535.})), &coverage);
    assert_eq!(out[0], input[0]);
    assert_eq!(out[3], input[3]);
    for i in 1..3 {
        for c in 0..3 {
            assert_eq!(out[i][c], input[i][c] + if i == 1 { 2 } else { 1 });
        }
        assert_eq!(out[i][3], input[i][3]);
    }
    let invert = apply(&d, &tone("invert", json!({})), &coverage);
    for c in 0..3 {
        assert_eq!(invert[1][c], 65535 - input[1][c]);
        let expected = quantize(
            f64::from(input[2][c]) + (65535. - 2. * f64::from(input[2][c])) * 32768. / 65535.,
        );
        assert_eq!(invert[2][c], expected);
    }
    assert_eq!(d.layer_source(0).unwrap().to_rgba16().ok().unwrap(), input);
}
#[test]
fn translated_group_source_uses_canvas_selection_but_whole_original_neighborhood() {
    let source = PrecisionImage::from_rgba16(
        4,
        1,
        vec![
            [0, 0, 0, 65535],
            [20000, 20000, 20000, 65535],
            [40000, 40000, 40000, 65535],
            [65535; 4],
        ],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let layer = Layer::raster("Offset", source, (-1, 0), 1., true).unwrap();
    let group = Layer::pass_through("Group", vec![layer], 0.4, true).unwrap();
    let d = PrecisionDocument::from_parts(
        2,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![group],
    )
    .unwrap();
    let out = apply(&d, &request("pixelate", json!({"block":4})), &[65535, 0]);
    assert_eq!(out[0], [0, 0, 0, 65535]);
    assert_eq!(out[1], [31384, 31384, 31384, 65535]);
    assert_eq!(out[2], [40000, 40000, 40000, 65535]);
    assert_eq!(out[3], [65535; 4]);
}
#[test]
fn every_spatial_filter_obeys_selection_zero_and_native_alpha_policy() {
    let pixels: Vec<_> = (0..35)
        .map(|i| {
            [
                (10001 + i * 271) as u16,
                (22002 + i * 421) as u16,
                (44003 - i * 331) as u16,
                if i == 0 {
                    0
                } else if i == 1 {
                    1
                } else {
                    65535
                },
            ]
        })
        .collect();
    let d = doc(7, 5, pixels.clone());
    let mut selection = vec![0; 35];
    selection[1] = 65535;
    selection[17] = 65535;
    for (op, params, color_only) in [
        ("blur", json!({"sigma":1.5}), false),
        ("sharpen", json!({"amount":1.5}), false),
        ("gaussian_blur", json!({"radius":1.5}), false),
        ("unsharp_mask", json!({"radius":1.5}), true),
        ("high_pass", json!({"radius":1.5}), true),
        ("reduce_noise", json!({"radius":2.}), true),
        ("dust_and_scratches", json!({"radius":2.}), true),
        ("edge_detect", json!({}), false),
        ("emboss", json!({}), false),
        ("pixelate", json!({"block":3}), false),
        ("add_noise", json!({"amount":0.1,"seed":73}), true),
        ("box_blur", json!({"radius":2}), false),
        ("surface_blur", json!({"radius":2,"threshold":30}), true),
        ("add_grain", json!({"amount":50,"seed":73}), true),
        ("radial_blur", json!({"amount":40}), false),
        ("clouds", json!({"seed":73}), false),
        ("difference_clouds", json!({"seed":73}), true),
        ("offset", json!({"horizontal":1,"vertical":1}), false),
    ] {
        let e = request(op, params);
        let full = apply(&d, &e, &[]);
        let selected = apply(&d, &e, &selection);
        // Radial Blur and Offset act within the selection's bounding box, so
        // only the zero-coverage law compares them with the unselected run.
        let framed = matches!(op, "radial_blur" | "offset");
        for i in 0..pixels.len() {
            if selection[i] == 0 {
                assert_eq!(selected[i], pixels[i], "{op} sample{i}");
            } else if !framed {
                assert_eq!(selected[i], full[i], "{op} source scope");
            }
        }
        if color_only {
            assert_eq!(selected[1][3], 1, "{op}");
        }
    }
}
#[test]
fn fractional_alpha_changing_write_is_premultiplied_and_reads_hidden_for_relief() {
    let d = doc(2, 1, vec![[65535, 0, 0, 1], [0, 65535, 0, 65535]]);
    let coverage = [32768, 0];
    let out = apply(&d, &request("pixelate", json!({"block":2})), &coverage);
    let t = 32768. / 65535.;
    let a0 = 1. / 65535.;
    let a1 = (1. + 65535.) / 2. / 65535.;
    let alpha = a0 * (1. - t) + a1 * t;
    let rgb = [1. / 65536., 65535. / 65536., 0.];
    for c in 0..3 {
        let expected = ((if c == 0 { a0 } else { 0. }) * (1. - t) + rgb[c] * a1 * t) / alpha;
        assert!(out[0][c].abs_diff(quantize(expected * 65535.)) <= 1);
    }
    assert_eq!(out[0][3], quantize(alpha * 65535.));
    assert_eq!(out[1], [0, 65535, 0, 65535]);
    let hidden = doc(1, 1, vec![[20000, 30000, 40000, 0]]);
    let out = apply(&hidden, &request("emboss", json!({})), &[])[0];
    let expected = (0.2126 * 20000. + 0.7152 * 30000. + 0.0722 * 40000. + 32767.5_f64)
        .round()
        .min(65535.) as u16;
    assert!(out[0].abs_diff(expected) <= 1);
    assert_eq!(out[3], 65535);
}
#[test]
fn gaussian_photographic_operators_match_independent_2d_premultiplied_reference() {
    let input = vec![
        [10001, 24002, 35003, 1],
        [65535, 0, 0, 0],
        [45007, 35008, 25009, 65535],
    ];
    let d = doc(3, 1, input.clone());
    let mut low = Vec::new();
    for x in 0..3_i64 {
        let mut sum = [0.; 4];
        let mut weights = 0.;
        for dx in -3..=3_i64 {
            let weight = (-0.5 * (dx * dx) as f64).exp();
            let p = input[(x + dx).clamp(0, 2) as usize].map(|v| f64::from(v) / 65535.);
            for c in 0..3 {
                sum[c] += weight * p[c] * p[3];
            }
            sum[3] += weight * p[3];
            weights += weight;
        }
        let alpha = sum[3] / weights;
        low.push([sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3], alpha]);
    }
    for op in ["gaussian_blur", "unsharp_mask", "high_pass"] {
        let out = apply(&d, &request(op, json!({"radius":1.})), &[]);
        for i in 0..3 {
            for c in 0..4 {
                let s = f64::from(input[i][c]) / 65535.;
                let expected = if op == "gaussian_blur" {
                    low[i][c]
                } else if c == 3 || input[i][3] == 0 {
                    s
                } else if op == "unsharp_mask" {
                    (2. * s - low[i][c]).clamp(0., 1.)
                } else {
                    (0.5 + s - low[i][c]).clamp(0., 1.)
                };
                assert!(
                    out[i][c].abs_diff(quantize(expected * 65535.)) <= 1,
                    "{op} pixel{i} channel{c}"
                );
            }
        }
    }
}
#[test]
fn native_gaussian_tiles_match_unbroken_library_kernel_and_legacy_sharpen_formula() {
    let (w, h) = (263_u32, 137_u32);
    let input: Vec<_> = (0..w * h)
        .map(|i| {
            [
                (i * 109 % 65536) as u16,
                (i * 97 % 65536) as u16,
                (i * 47 % 65536) as u16,
                ((i * 131) % 65535 + 1) as u16,
            ]
        })
        .collect();
    let d = doc(w, h, input.clone());
    let float = image::Rgba32FImage::from_fn(w, h, |x, y| {
        let p = input[(y * w + x) as usize].map(|v| f32::from(v) / 65535.);
        image::Rgba([p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
    });
    let low = image::imageops::blur(&float, 1.5);
    for op in ["blur", "sharpen"] {
        let p = if op == "blur" {
            json!({"sigma":1.5})
        } else {
            json!({"amount":1.5})
        };
        let out = apply(&d, &request(op, p), &[]);
        for (i, p) in low.pixels().enumerate() {
            let mut expected = [0; 4];
            for c in 0..4 {
                let value = if c == 3 { p[c] } else { p[c] / p[3] };
                let q = (value * 65535.).clamp(0., 65535.).round() as u16;
                expected[c] = if op == "blur" {
                    q
                } else {
                    (2 * i32::from(input[i][c]) - i32::from(q)).clamp(0, 65535) as u16
                };
            }
            if op == "sharpen" && expected[3] == 0 {
                expected = [0; 4];
            }
            for c in 0..4 {
                assert!(
                    out[i][c].abs_diff(expected[c]) <= 1,
                    "{op} sample{i} channel{c}"
                );
            }
        }
    }
}
fn auto(d: &PrecisionDocument, scope: &str, coverage: &[u16], op: &str, clip: f64) -> Value {
    let request = CString::new(
        json!({"layer":0,"op":op,"params":{"clip":clip},"analysis_scope":scope}).to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_selected_auto_levels(
            d,
            request.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null());
    assert!(!result.is_null());
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
    unsafe { doctored_core::ffi::dr_string_free(result) };
    value
}
#[test]
fn auto_scope_uses_native_weighted_endpoints_independent_from_write_scope() {
    let pixels = vec![
        [100; 4],
        [20000, 20000, 20000, 1],
        [20001, 20001, 20001, 65535],
        [64000; 4],
    ];
    // 300 of each, so the clip (`ops_auto::tone_endpoints`) is small
    // beside a group: selected, the coverage-1 groups weigh 300 and are
    // clipped (the black is 20000, the first code whose running weight
    // exceeds the cut; the white walk over 8-bit bins clips 64000's bin 249
    // and stops on 20000-20001's bin 78); whole, they weigh 300 full pixels
    // and stay (black 100, white bin 249).
    let many: Vec<[u16; 4]> = pixels.iter().flat_map(|p| [*p; 300]).collect();
    let coverage: Vec<u16> = [1, 65535, 65535, 1]
        .iter()
        .flat_map(|c| [*c; 300])
        .collect();
    let d_many = doc(1200, 1, many);
    let selected = auto(&d_many, "selection", &coverage, "auto_tone", 0.001);
    let whole = auto(&d_many, "whole_target", &coverage, "auto_tone", 0.001);
    assert!((selected["black"][0].as_f64().unwrap() * 65535. - 20000.).abs() < 0.01);
    assert!((selected["white"][0].as_f64().unwrap() * 255. - 78.).abs() < 0.001);
    assert!((whole["black"][0].as_f64().unwrap() * 65535. - 100.).abs() < 0.01);
    assert!((whole["white"][0].as_f64().unwrap() * 255. - 249.).abs() < 0.001);
    // The write scope, at clip 0 (the exact occupied ends): the selection
    // measures 20000…20001 and only its pixels move.
    let d = doc(4, 1, pixels.clone());
    let e = SelectedEdit::parse(
        &json!({"layer":0,"op":"auto_tone","analysis_scope":"selection","params":{"clip":0.0}})
            .to_string(),
    )
    .unwrap();
    let out = apply(&d, &e, &[0, 65535, 65535, 0]);
    assert_eq!(out[0], pixels[0]);
    assert_eq!(out[3], pixels[3]);
    assert_eq!(out[1], [0, 0, 0, 1]);
    assert_eq!(out[2], [65535; 4]);
}
#[test]
fn strict_requests_admission_cancellation_and_noops_are_atomic() {
    let d = doc(3, 1, vec![[12345, 23456, 34567, 1]; 3]);
    let e = request("high_pass", json!({"radius":2.}));
    assert!(d.selected_edit(&e, &[1, 2], LIMIT, 0, None).is_err());
    let err = d.selected_edit(&e, &[], 1, 0, None).err().unwrap();
    assert!(err.contains("working limit"));
    let job = DrJobControl::default();
    job.cancel();
    assert!(d
        .selected_edit(&e, &[], LIMIT, 0, Some(&job))
        .err()
        .unwrap()
        .contains("cancel"));
    assert!(d
        .selected_edit(&e, &[0, 0, 0], LIMIT, 0, None)
        .unwrap()
        .is_none());
    for text in [
        r#"{"layer":0,"op":"pixelate","params":{"block":1.5}}"#,
        r#"{"layer":0,"op":"emboss","params":{"sigma":2}}"#,
        r#"{"layer":0,"op":"high_pass","analysis_scope":"selection"}"#,
        r#"{"layer":0,"op":"auto_tone","params":{"clip":0.5}}"#,
        r#"{"layer":0,"op":"box_blur","params":{"radius":1.5}}"#,
        r#"{"layer":0,"op":"surface_blur","params":{"threshold":1}}"#,
        r#"{"layer":0,"op":"add_grain","params":{"sigma":2}}"#,
        r#"{"layer":0,"op":"radial_blur","params":{"method":"twist"}}"#,
        r#"{"layer":0,"op":"clouds","params":{"high_contrast":"yes"}}"#,
        r#"{"layer":0,"op":"difference_clouds","params":{"foreground_color":"red"}}"#,
        r#"{"layer":0,"op":"offset","params":{"undefined_areas":"background"}}"#,
    ] {
        assert!(SelectedEdit::parse(text).is_err());
    }
    let text = CString::new(r#"{"layer":0,"op":"high_pass","params":{"radius":2}}"#).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            &d,
            text.as_ptr(),
            ptr::null(),
            3,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(out.is_null());
    assert!(!error.is_null());
    unsafe { doctored_core::ffi::dr_string_free(error) };
    assert_eq!(
        d.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        &[[12345, 23456, 34567, 1]; 3]
    );
}

#[test]
fn native_rank_disc_keeps_adjacent_codes_and_ignores_alpha_zero_voters() {
    let values = [
        10001, 10002, 10003, 10004, 65535, 10006, 10007, 10008, 10009,
    ];
    let pixels: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(i, v)| [*v, *v, *v, if i == 1 { 0 } else { 1 }])
        .collect();
    let d = doc(3, 3, pixels.clone());
    let out = apply(
        &d,
        &request("dust_and_scratches", json!({"radius":1.,"threshold":0.})),
        &[],
    );
    // Radius1 disc at center holds up/left/center/right/down. The hidden up
    // voter is omitted; sorted visible votes [10004,10006,10008,65535].
    assert_eq!(out[4], [10006, 10006, 10006, 1]);
    assert_eq!(out[1], pixels[1]);
}

#[test]
fn native_noise_stream_includes_hidden_and_unselected_pixels() {
    let pixels = vec![
        [20000, 30000, 40000, 0],
        [20001, 30001, 40001, 1],
        [20002, 30002, 40002, 65535],
    ];
    let d = doc(3, 1, pixels.clone());
    let all = apply(
        &d,
        &request("add_noise", json!({"seed":-1,"amount":0.01})),
        &[],
    );
    let selected = apply(
        &d,
        &request("add_noise", json!({"seed":-1,"amount":0.01})),
        &[0, 0, 65535],
    );
    assert_ne!(all[0], pixels[0]);
    assert_eq!(all[0][3], 0);
    assert_eq!(selected[0], pixels[0]);
    assert_eq!(selected[1], pixels[1]);
    assert_eq!(selected[2], all[2]);
    // Published SplitMix64 top24 output for seed−1, generated independently.
    let expected_units = [0.893_942_9_f32, 0.912_597_2, 0.219_481_95];
    for c in 0..3 {
        let expected = ((f32::from(pixels[0][c]) / 65535. + (2. * expected_units[c] - 1.) * 0.01)
            .clamp(0., 1.)
            * 65535.)
            .round() as u16;
        assert_eq!(all[0][c], expected);
    }
}

#[test]
fn cancellation_during_neighborhood_work_releases_reservation_without_publishing() {
    // Below σ4 the neighbourhood stays the ceil(3σ)
    // FIR, here with at least the work the former radius 16 on 768² had.
    let fixture = || doc(1536, 1536, vec![[20001, 30002, 40003, 65535]; 1536 * 1536]);
    let blur = |d: &PrecisionDocument, token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match d.selected_edit(
            &request("gaussian_blur", json!({"radius":3.9})),
            &[],
            LIMIT,
            0,
            Some(control),
        ) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    // A Gaussian row report proves we reached the neighborhood pass, rather
    // than cancelling between initial admission and the first allocation.
    let twin = fixture();
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress > 0.05, |token| {
        blur(&twin, token)
    });
    drop(twin);
    let n0 = entry.checkpoints;
    let k = 1;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 1 + k,
        "the entry's kernel polls fewer than {} checkpoints before its next report (entry {n0}, next {next:?})",
        k + 1
    );
    let d = fixture();
    let source = d.layer_source(0).unwrap().clone();
    let parked = cancelled_parked_at(
        n0 + 1 + k,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        |token| blur(&d, token),
    );
    println!("neighbourhood cancellation: {}", parked_line(&parked));
    assert!(parked.progress > 0.05);
    assert!(parked.scratch > 0);
    assert!(PrecisionImage::ptr_eq(d.layer_source(0).unwrap(), &source));
}

#[test]
fn captured_lut_payload_above_legacy_request_cap_is_admitted_and_selection_gated() {
    let d = doc(
        2,
        1,
        vec![[12345, 23456, 34567, 1], [12346, 23457, 34568, 65535]],
    );
    // A legal33³ zero LUT is ~575 KiB in the inherited base64 representation.
    let table = "A".repeat(33_usize.pow(3) * 3 * 4 / 3 * 4);
    let description = json!({"type":"adjust","evaluation":"precision_v1","op":"color_lookup","params":{"kind":"3d","size":33,"table":table}}).to_string();
    let text =
        CString::new(json!({"layer":0,"op":"adjustment","description":description}).to_string())
            .unwrap();
    assert!(text.as_bytes().len() > 256 * 1024);
    let mut error = ptr::null_mut();
    let plan =
        unsafe { dr_precision_doc_selected_edit_plan(&d, text.as_ptr(), LIMIT, 0, &mut error) };
    assert!(error.is_null());
    assert!(!plan.is_null());
    let info: Value = serde_json::from_slice(unsafe { CStr::from_ptr(plan) }.to_bytes()).unwrap();
    assert!(info["scratch_bytes"].as_u64().unwrap() > text.as_bytes().len() as u64);
    unsafe { doctored_core::ffi::dr_string_free(plan) };
    let coverage = [65535_u16, 0];
    let output = unsafe {
        dr_precision_doc_selected_edit(
            &d,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null());
    assert!(!output.is_null());
    let out = unsafe { Box::from_raw(output) };
    assert_eq!(
        out.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        &[[0, 0, 0, 1], [12346, 23457, 34568, 65535]]
    );
}

#[test]
fn legacy_gaussian_native_zero_alpha_boundary_precedes_sharpen_subtraction() {
    let d = doc(
        3,
        1,
        vec![[0, 0, 0, 0], [10001, 20002, 30003, 1], [0, 0, 0, 0]],
    );
    let blurred = apply(&d, &request("blur", json!({"sigma":1.})), &[]);
    assert_eq!(blurred, vec![[0; 4]; 3]);
    let sharpened = apply(&d, &request("sharpen", json!({"amount":1.})), &[]);
    assert_eq!(sharpened[1], [20002, 40004, 60006, 2]);
    assert_eq!(sharpened[0], [0; 4]);
    assert_eq!(sharpened[2], [0; 4]);
}

#[test]
fn byte_weighted_auto_keeps_whole_target_actions_exact_and_counts_feather_one() {
    use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
    use doctored_core::ffi_adjust::dr_image_auto_levels;
    use doctored_core::ffi_selected_auto::dr_image_selected_auto_levels;
    // 300 pixels of each code, so the clip is small beside a group.
    let bytes: Vec<u8> = [10u8, 80, 160, 250]
        .iter()
        .flat_map(|&v| [[v, v, v, 255]; 300])
        .flatten()
        .collect();
    let image = unsafe { dr_image_from_rgba(bytes.as_ptr(), 1200, 1) };
    assert!(!image.is_null());
    let derive = |coverage: &[u8], mode: u32, clip: f64| {
        let mut error = ptr::null_mut();
        let text = unsafe {
            dr_image_selected_auto_levels(
                image,
                coverage.as_ptr(),
                coverage.len(),
                mode,
                clip,
                &mut error,
            )
        };
        assert!(error.is_null());
        assert!(!text.is_null());
        let value: Value =
            serde_json::from_slice(unsafe { CStr::from_ptr(text) }.to_bytes()).unwrap();
        unsafe { dr_string_free(text) };
        value
    };
    for mode in 0..3 {
        let mut legacy = [0_f32; 9];
        assert!(unsafe {
            dr_image_auto_levels(image, ptr::null(), mode, 0.01, legacy.as_mut_ptr())
        });
        let whole = derive(&[], mode as u32, 0.01);
        for (part, key) in ["black", "white", "gamma"].into_iter().enumerate() {
            for c in 0..3 {
                assert_eq!(
                    (whole[key][c].as_f64().unwrap() as f32).to_bits(),
                    legacy[part * 3 + c].to_bits()
                );
            }
        }
    }
    // The coverage-1 groups weigh 300 of a 153,600 mass: the black passes
    // 10 (300 under the 1,536 cut) to 80, the white walk clips 250 and stops
    // on 160.
    let spread = |c: [u8; 4]| -> Vec<u8> { c.iter().flat_map(|&v| [v; 300]).collect() };
    let clipped = derive(&spread([1, 255, 255, 1]), 0, 0.01);
    assert_eq!(clipped["black"][0].as_f64().unwrap() as f32, 80. / 255.);
    assert_eq!(clipped["white"][0].as_f64().unwrap() as f32, 160. / 255.);
    // At clip0 a coverage1 sample really votes and sets the black point.
    let feather = derive(&spread([1, 255, 255, 0]), 0, 0.);
    assert_eq!(feather["black"][0].as_f64().unwrap() as f32, 10. / 255.);
    unsafe { dr_image_free(image) };
}

#[test]
fn weighted_auto_color_gamma_uses_coverage_mass_for_neutral_candidates() {
    use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
    use doctored_core::ffi_selected_auto::dr_image_selected_auto_levels;
    let bytes = [
        0, 0, 0, 255, 255, 255, 255, 255, 110, 120, 125, 255, 150, 140, 135, 255,
    ];
    let image = unsafe { dr_image_from_rgba(bytes.as_ptr(), 4, 1) };
    let coverage = [255, 255, 255, 1];
    let mut error = ptr::null_mut();
    let text = unsafe {
        dr_image_selected_auto_levels(image, coverage.as_ptr(), coverage.len(), 2, 0., &mut error)
    };
    assert!(error.is_null());
    assert!(!text.is_null());
    let value: Value = serde_json::from_slice(unsafe { CStr::from_ptr(text) }.to_bytes()).unwrap();
    let means = [
        (110. * 255. + 150.) / 256. / 255.,
        (120. * 255. + 140.) / 256. / 255.,
        (125. * 255. + 135.) / 256. / 255.,
    ];
    let mean: f64 = (means[0] + means[1] + means[2]) / 3.;
    for (c, m) in means.iter().enumerate() {
        let expected = (m.ln() / mean.ln()).clamp(0.5, 2.);
        assert!((value["gamma"][c].as_f64().unwrap() - expected).abs() < 1e-6);
    }
    unsafe {
        dr_string_free(text);
        dr_image_free(image)
    };
}
