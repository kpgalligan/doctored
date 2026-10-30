//! #547 Lab phase 2b, a Lab Color document's adjustments (`lab_adjust`,
//! `lab_tone`). The oracle is the RGB kernel on an sRGB document of chosen
//! codes, never the Lab path: a Lightness op (Levels gamma, Curves on the
//! composite, Brightness/Contrast) keeps a\*/b\* bit for bit and its L\* is
//! the R of the same description on an sRGB document of `(L, L, L, α)`;
//! Invert gives `65535 − c` on every channel; Posterize equals the sRGB
//! document of the same `(L, a, b, α)` codes; Threshold gives a\* = b\* =
//! 0x8080 exactly and its L\* is the oracle's R. Each also as an adjustment
//! layer renders equal to the destructive result. Every refusal sentence is
//! exact at plan, add, add_at, set, apply and the selected edit (phase 2c's
//! forms are `lab_color_adjust.rs`'s); a Levels
//! layer survives `.rz` and Lab → RGB keeps it as an adjustment; and the
//! scalar path (a layer mask and an alpha channel) answers exactly as on an
//! sRGB document. Bounds (exact equality) declared before measuring.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_insert::dr_precision_doc_add_adjustment_at;
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_convert_to_lab,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_open,
    dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_adjustment_plan,
    dr_precision_doc_apply_adjustment, dr_precision_doc_set_adjustment,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_precision_inspect::dr_precision_doc_adjustment_description;
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// sRGB colours (8-bit codes) and alphas whose L\*a\*b\* spread over the
/// range, a\* and b\* both signs and neutral; no alpha is 0.
const PATCHES: [[u16; 4]; 12] = [
    [200, 40, 90, 255],
    [30, 180, 220, 255],
    [128, 128, 128, 255],
    [250, 250, 10, 200],
    [64, 0, 255, 255],
    [0, 0, 0, 255],
    [255, 255, 255, 255],
    [90, 160, 40, 120],
    [17, 34, 51, 255],
    [240, 128, 200, 255],
    [10, 20, 5, 60],
    [220, 230, 240, 255],
];

const NEUTRAL: u16 = 0x8080;

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
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

/// A document result: `Ok(None)` is NULL with no error (no change).
fn result(
    out: *mut PrecisionDocument,
    err: *mut c_char,
) -> Result<Option<*mut PrecisionDocument>, String> {
    if !out.is_null() {
        Ok(Some(out))
    } else if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

/// An sRGB document of one layer holding exactly `pixels`, one row.
fn srgb_doc(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let w = pixels.len() as u32;
    let image = PrecisionImage::from_rgba16(w, 1, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        w,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

/// A Lab document of `depth`: the patches through Mode ▸ Lab Color.
fn lab_doc(depth: u32) -> *mut PrecisionDocument {
    let rgb = srgb_doc(PATCHES.iter().map(|p| p.map(|c| c * 257)).collect());
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "to Lab: {}", take_err_string(err));
    free(rgb);
    assert_eq!(descriptor(lab)["color_mode"], "lab");
    lab
}

fn description(op: &str, params: Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

fn apply(doc: *const PrecisionDocument, meta: &str) -> Result<*mut PrecisionDocument, String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(doc, 0, meta.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err).map(|d| d.expect("the adjustment changed nothing"))
}

fn add(doc: *const PrecisionDocument, meta: &str) -> Result<*mut PrecisionDocument, String> {
    let name = CString::new("Adjustment").unwrap();
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            meta.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(Option::unwrap)
}

fn add_at(doc: *const PrecisionDocument, meta: &str) -> Result<*mut PrecisionDocument, String> {
    let name = CString::new("Adjustment").unwrap();
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment_at(
            doc,
            name.as_ptr(),
            meta.as_ptr(),
            1.0,
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(Option::unwrap)
}

fn set(
    doc: *const PrecisionDocument,
    index: usize,
    meta: &str,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_adjustment(doc, index, meta.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err)
}

fn plan(
    doc: *const PrecisionDocument,
    index: i64,
    meta: &str,
    destructive: bool,
) -> Result<Value, String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_adjustment_plan(doc, index, meta.as_ptr(), destructive, &mut err)
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(serde_json::from_str(&text_of(out)).unwrap())
    }
}

fn stored(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_adjustment_description(doc, index, &mut err) };
    if out.is_null() {
        None
    } else {
        Some(text_of(out))
    }
}

fn selected(
    doc: *const PrecisionDocument,
    request: &Value,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
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
    result(out, err)
}

fn selected_plan(doc: *const PrecisionDocument, request: &Value) -> Result<(), String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_selected_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        unsafe { dr_string_free(out) };
        Ok(())
    }
}

fn edit(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
}

fn codes(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// The sRGB oracle of `meta` on `pixels`: the same description applied to
/// an sRGB document holding exactly those codes.
fn oracle(pixels: Vec<[u16; 4]>, meta: &str) -> Vec<[u16; 4]> {
    let rgb = srgb_doc(pixels);
    let out = apply(rgb, meta).unwrap_or_else(|e| panic!("oracle {meta}: {e}"));
    let result = codes(out, 0);
    free(out);
    free(rgb);
    result
}

fn gray_of(pixels: &[[u16; 4]]) -> Vec<[u16; 4]> {
    pixels.iter().map(|p| [p[0], p[0], p[0], p[3]]).collect()
}

/// The ops and parameters the table runs, with the depths each is checked at.
fn running() -> Vec<(&'static str, Value, &'static [u32])> {
    vec![
        ("levels", json!({"gamma":1.4}), &[8, 16]),
        (
            "levels",
            json!({"black":0.1,"white":0.9,"gamma":0.8}),
            &[16],
        ),
        (
            "curves",
            json!({"rgb":[[0,0],[64,40],[192,220],[255,255]]}),
            &[16],
        ),
        ("bcs", json!({"brightness":0.2,"contrast":0.3}), &[16]),
        (
            "bcs",
            json!({"brightness":-0.1,"contrast":0.2,"saturation":0}),
            &[16],
        ),
        ("invert", json!({}), &[8, 16]),
        ("posterize", json!({"levels":5}), &[16]),
        ("threshold", json!({"level":0.5}), &[16]),
    ]
}

/// `out` against the oracle for `op` on the Lab source `source`.
fn check_form(op: &str, meta: &str, source: &[[u16; 4]], out: &[[u16; 4]], what: &str) {
    match op {
        "levels" | "curves" | "bcs" => {
            let want = oracle(gray_of(source), meta);
            for (i, p) in out.iter().enumerate() {
                assert_eq!(p[0], want[i][0], "{what} pixel {i} L");
                assert_eq!(p[1..], source[i][1..], "{what} pixel {i} a, b and alpha");
            }
        }
        "invert" => {
            for (i, p) in out.iter().enumerate() {
                for c in 0..3 {
                    assert_eq!(p[c], 65535 - source[i][c], "{what} pixel {i} channel {c}");
                }
                assert_eq!(p[3], source[i][3], "{what} pixel {i} alpha");
            }
        }
        "posterize" => {
            assert_eq!(out, oracle(source.to_vec(), meta), "{what}");
        }
        "threshold" => {
            let want = oracle(gray_of(source), meta);
            for (i, p) in out.iter().enumerate() {
                assert!(p[0] == 0 || p[0] == 65535, "{what} pixel {i} L {}", p[0]);
                assert_eq!(p[0], want[i][0], "{what} pixel {i} L");
                assert_eq!(p[1], NEUTRAL, "{what} pixel {i} a");
                assert_eq!(p[2], NEUTRAL, "{what} pixel {i} b");
                assert_eq!(p[3], source[i][3], "{what} pixel {i} alpha");
            }
        }
        _ => unreachable!("{op}"),
    }
}

#[test]
fn every_running_op_applied_matches_its_lab_form_on_the_srgb_oracle() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let source = codes(lab, 0);
        for (op, params, depths) in running() {
            if !depths.contains(&depth) {
                continue;
            }
            let what = format!("{op} {params} at {depth}");
            let meta = description(op, params);
            let out = apply(lab, &meta).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(descriptor(out)["color_mode"], "lab", "{what}");
            check_form(op, &meta, &source, &codes(out, 0), &what);
            free(out);
        }
        free(lab);
    }
}

#[test]
fn every_running_op_as_a_layer_renders_equal_to_the_destructive_result() {
    let lab = lab_doc(16);
    for (op, params, _) in running() {
        let what = format!("{op} {params} layer");
        let meta = description(op, params);
        let destructive = apply(lab, &meta).unwrap();
        for layered in [add(lab, &meta), add_at(lab, &meta)] {
            let layered = layered.unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(render(layered), render(destructive), "{what}");
            assert_eq!(stored(layered, 1).as_deref(), Some(meta.as_str()), "{what}");
            free(layered);
        }
        free(destructive);
    }
    // The selected edit runs the same form.
    let meta = description("levels", json!({"gamma":1.4}));
    let destructive = apply(lab, &meta).unwrap();
    let request = json!({"layer":0,"op":"adjustment","description":meta});
    selected_plan(lab, &request).unwrap();
    let out = selected(lab, &request).unwrap().unwrap();
    assert_eq!(codes(out, 0), codes(destructive, 0));
    for doc in [out, destructive, lab] {
        free(doc);
    }
}

#[test]
fn refused_adjustments_name_their_reason_at_every_entry() {
    let lab = lab_doc(16);
    let levels = add(lab, &description("levels", json!({"gamma":1.2}))).unwrap();
    let cases = [
        (
            description("exposure", json!({"exposure":0.5})),
            "Exposure is unavailable in a Lab Color document.".to_string(),
        ),
        (
            description("black_and_white", json!({})),
            "Black & White is unavailable in a Lab Color document.".to_string(),
        ),
        (
            description("sepia", json!({})),
            "Sepia is unavailable in a Lab Color document: Doctored offers it on RGB documents only."
                .to_string(),
        ),
        (
            description("bogus", json!({})),
            // The close: an unknown op answers as on RGB.
            "unsupported high-depth adjustment bogus".to_string(),
        ),
    ];
    for (meta, sentence) in &cases {
        assert_eq!(&plan(lab, -1, meta, false).unwrap_err(), sentence, "{meta}");
        assert_eq!(&plan(lab, 0, meta, true).unwrap_err(), sentence, "{meta}");
        assert_eq!(&add(lab, meta).unwrap_err(), sentence, "{meta}");
        assert_eq!(&add_at(lab, meta).unwrap_err(), sentence, "{meta}");
        assert_eq!(&set(levels, 1, meta).unwrap_err(), sentence, "{meta}");
        assert_eq!(&apply(lab, meta).unwrap_err(), sentence, "{meta}");
        let request = json!({"layer":0,"op":"adjustment","description":meta});
        assert_eq!(&selected(lab, &request).unwrap_err(), sentence, "{meta}");
        assert_eq!(
            &selected_plan(lab, &request).unwrap_err(),
            sentence,
            "{meta}"
        );
    }
    // The selected edit's other op that refuses (Emboss runs since phase 2f:
    // `lab_channel_filter.rs`).
    let request = json!({"layer":0,"op":"auto_color"});
    let sentence = "Auto Color is unavailable in a Lab Color document.";
    assert_eq!(selected(lab, &request).unwrap_err(), sentence);
    assert_eq!(selected_plan(lab, &request).unwrap_err(), sentence);
    // The prepared kernel on the twin refuses a disabled op itself.
    let twin = unsafe { &*lab }.profile().clone();
    let Err(e) = PrecisionAdjustment::parse(&cases[0].0, &twin) else {
        panic!("Exposure parsed on the twin");
    };
    assert_eq!(e, cases[0].1);
    assert!(PrecisionAdjustment::parse(&cases[0].0, &IccProfile::srgb()).is_ok());
    // The composite and a zero saturation are admitted; an RGB document is untouched.
    let rgb = srgb_doc(vec![[1000, 2000, 3000, 65535]]);
    assert!(plan(rgb, -1, &cases[0].0, false).is_ok());
    let set_ok = set(levels, 1, &description("levels", json!({"gamma":0.9})))
        .unwrap()
        .unwrap();
    assert_eq!(descriptor(set_ok)["color_mode"], "lab");
    for doc in [set_ok, rgb, levels, lab] {
        free(doc);
    }
}

#[test]
fn a_levels_layer_survives_rz_and_lab_to_rgb_keeps_it() {
    let lab = lab_doc(16);
    let meta = description("levels", json!({"gamma":1.4}));
    let layered = add(lab, &meta).unwrap();
    let before = render(layered);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("levels.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(layered, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let reopened = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    let d = descriptor(reopened);
    assert_eq!(d["color_mode"], "lab");
    assert_eq!(d["layer_count"], 2);
    assert_eq!(stored(reopened, 1).as_deref(), Some(meta.as_str()));
    assert_eq!(render(reopened), before);
    let srgb = IccProfile::srgb();
    let rgb = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            layered,
            srgb.bytes().as_ptr(),
            srgb.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!rgb.is_null(), "to RGB: {}", take_err_string(err));
    let d = descriptor(rgb);
    assert_eq!(d["color_mode"], "rgb");
    assert_eq!(d["layer_count"], 2);
    assert_eq!(stored(rgb, 1).as_deref(), Some(meta.as_str()));
    // Its settings now act on the RGB composite: the RGB kernel on its codes.
    assert_eq!(render(rgb), oracle(codes(rgb, 0), &meta));
    for doc in [rgb, reopened, layered, lab] {
        free(doc);
    }
}

/// A layer mask and an alpha channel of `doc` set to `plane`.
fn with_planes(doc: *const PrecisionDocument, plane: &[u16]) -> *mut PrecisionDocument {
    let masked = edit(
        doc,
        &json!({"op":"set_mask","layer":0,"width":plane.len(),"height":1,"x":0,"y":0,"linked":true}),
        plane,
    )
    .unwrap()
    .unwrap();
    let channel = edit(
        masked,
        &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
        plane,
    )
    .unwrap()
    .unwrap();
    free(masked);
    channel
}

fn planes(doc: *const PrecisionDocument) -> (Vec<u16>, Vec<u16>) {
    let d = unsafe { &*doc };
    let mask = d
        .layer(0)
        .unwrap()
        .mask()
        .unwrap()
        .plane()
        .samples()
        .to_vec();
    let channel = d.channels()[0].plane().samples().to_vec();
    (mask, channel)
}

#[test]
fn the_scalar_path_on_a_mask_and_an_alpha_channel_answers_as_on_rgb() {
    let plane: Vec<u16> = (0..PATCHES.len() as u16).map(|i| i * 5900 + 77).collect();
    let lab = lab_doc(16);
    let rgb = srgb_doc(PATCHES.iter().map(|p| p.map(|c| c * 257)).collect());
    let lab_planes = with_planes(lab, &plane);
    let rgb_planes = with_planes(rgb, &plane);
    assert_eq!(planes(lab_planes), planes(rgb_planes));
    for meta in [
        description("levels", json!({"gamma":1.4})),
        description("threshold", json!({"level":0.5})),
        description("hue_saturation", json!({"lightness":0.3})),
    ] {
        for (target, index) in [("mask", 0), ("channel", 0)] {
            let request = json!({"op":"scalar_edit","target":target,"index":index,
                "operation":{"kind":"filter","edit":{"layer":0,"op":"adjustment",
                "description":meta}},"selection":false});
            let what = format!("{target} {meta}");
            let a = edit(lab_planes, &request, &[]).unwrap_or_else(|e| panic!("{what}: {e}"));
            let b = edit(rgb_planes, &request, &[]).unwrap_or_else(|e| panic!("{what} rgb: {e}"));
            let (a, b) = (a.expect("the edit changed the plane"), b.unwrap());
            assert_eq!(planes(a), planes(b), "{what}");
            assert_eq!(
                codes(a, 0),
                codes(lab_planes, 0),
                "{what}: the layer is untouched"
            );
            free(a);
            free(b);
        }
    }
    for doc in [rgb_planes, lab_planes, rgb, lab] {
        free(doc);
    }
}
