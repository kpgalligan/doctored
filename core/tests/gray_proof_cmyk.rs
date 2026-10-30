//! #1 phase 5b: a Grayscale document's soft proof and its direct
//! conversion to CMYK Color.
//!
//! The oracles are independent of the Gray path: the RGB TWIN (the Gray
//! document converted to RGB through its own profile bytes, which keeps every
//! sample) through the same RGB exports, and a closed form written here from
//! the profiles' own definitions — a Gray Gamma 1.8 `kTRC` (u8Fixed8
//! 461/256), the CIE L* formula, and the synthetic press's affine `B2A1`
//! (`cmyk_fixture`).
//!
//! Bounds, declared before measuring: Gray → CMYK equals the twin's RGB →
//! CMYK exactly at 8 and 16 bits; the closed form within 4 codes of 65535 per
//! ink (the CMM's 16-bit pipeline); the proof's bytes, info, masks and counts
//! equal the twin's exactly on both the typed and the byte path.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_cmyk::{
    dr_precision_doc_convert_to_cmyk, dr_precision_doc_convert_to_cmyk_plan,
};
use doctored_core::ffi_gray::dr_precision_doc_gray_profile_edit;
use doctored_core::ffi_output_color::{
    dr_precision_proof_gamut_mask, dr_precision_proof_render, dr_proof_gamut_mask, dr_proof_render,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_render, dr_precision_image_free,
    dr_precision_image_preview_rgba8,
};
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;
use serde_json::{json, Value};

use crate::cmyk_fixture::{b2a1_codes, encode_lab, press_profile};
use crate::common::take_err_string;
use crate::gray_fixture::gray_icc;
use crate::gray_ops::{assert_neutral, free, gray_doc, layer, plan_json, result, twin, LIMIT};

const W: u32 = 24;
const H: u32 = 16;
const FLOAT_REFUSAL: &str =
    "CMYK Color is unavailable in a 32-bit float document; convert to 16 Bits/Channel first";
const ADJUSTMENT_REFUSAL: &str = "is an adjustment layer, which a CMYK document cannot hold yet";

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

/// `dr_precision_doc_convert_to_cmyk` into `press` under `options`.
fn to_cmyk(
    doc: *const PrecisionDocument,
    press: &[u8],
    options: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            press.as_ptr(),
            press.len(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(|d| d.expect("a CMYK conversion always yields a document"))
}

fn cmyk_plan(doc: *const PrecisionDocument, press: &[u8], options: &Value) -> Value {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk_plan(
            doc,
            press.as_ptr(),
            press.len(),
            text.as_ptr(),
            &mut err,
        )
    };
    plan_json(out, err).expect("the plan answers")
}

fn inks(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(index)
        .expect("a CMYK record")
}

fn profile_bytes(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_profile(doc, &mut length) };
    assert!(!bytes.is_null());
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

fn assign(doc: *const PrecisionDocument, icc: &[u8]) -> *mut PrecisionDocument {
    let request = CString::new(r#"{"op":"assign"}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_gray_profile_edit(
            doc,
            icc.as_ptr(),
            icc.len(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
        .unwrap_or_else(|e| panic!("assign: {e}"))
        .expect("assigning another profile changes the document")
}

/// Gray `doc` with a Levels adjustment layer on top (as `gray_adjust.rs`
/// adds one).
fn with_adjustment(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let name = CString::new("Adjustment").unwrap();
    let meta = CString::new(
        json!({"type":"adjust","evaluation":"precision_v1","op":"levels",
            "params":{"gamma":1.2}})
        .to_string(),
    )
    .unwrap();
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
    result(out, err)
        .unwrap_or_else(|e| panic!("add adjustment: {e}"))
        .unwrap()
}

/// The CIE L* of a relative luminance `y` (white = 1), from the standard.
fn lstar(y: f64) -> f64 {
    let epsilon = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    if y > epsilon {
        116.0 * y.cbrt() - 16.0
    } else {
        kappa * y
    }
}

#[test]
fn gray_to_cmyk_equals_the_twins_rgb_to_cmyk() {
    let press = press_profile("Press");
    for depth in [8u32, 16] {
        let gray = gray_doc(W, H, depth);
        assert_neutral(&layer(gray, 0, depth), "gray source");
        let rgb = twin(gray);
        for options in [
            json!({"depth":depth}),
            json!({"depth":depth,"intent":"perceptual","black_point_compensation":false}),
        ] {
            let direct = to_cmyk(gray, &press, &options).unwrap();
            let via = to_cmyk(rgb, &press, &options).unwrap();
            let got = inks(direct, 0);
            assert_eq!(got, inks(via, 0), "{depth} {options}");
            let d = descriptor(direct);
            assert_eq!(d["color_mode"], "cmyk", "{depth} {options}");
            assert_eq!(d["cmyk_depth"], depth, "{depth} {options}");
            if depth == 8 {
                for (i, p) in got.iter().enumerate() {
                    for c in 0..4 {
                        assert_eq!(p[c] % 257, 0, "pixel {i} ink {c}: {p:?}");
                    }
                }
            }
            free(direct);
            free(via);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn gray_to_cmyk_matches_the_closed_form_of_its_profile() {
    let press = press_profile("Press");
    let source = gray_doc(W, H, 16);
    let gray = assign(source, &gray_icc("Gamma 1.8", 1.8));
    free(source);
    let cmyk = to_cmyk(
        gray,
        &press,
        &json!({"intent":"relative","black_point_compensation":false}),
    )
    .unwrap();
    let before = layer(gray, 0, 16);
    let after = inks(cmyk, 0);
    assert_eq!(before.len(), after.len());
    for (i, (g, k)) in before.iter().zip(&after).enumerate() {
        assert_eq!(u32::from(k[4]), g[3], "alpha at {i}");
        if g[3] != 65535 {
            continue;
        }
        let y = (f64::from(g[0]) / 65535.0).powf(461.0 / 256.0);
        let expected = b2a1_codes(encode_lab([lstar(y), 0.0, 0.0]));
        for c in 0..4 {
            let want = expected[c].clamp(0.0, 65535.0);
            assert!(
                (f64::from(k[c]) - want).abs() <= 4.0,
                "pixel {i} ink {c}: {} vs {want} (gray {})",
                k[c],
                g[0]
            );
        }
    }
    free(cmyk);
    free(gray);
}

#[test]
fn gray_to_cmyk_refuses_what_rgb_to_cmyk_refuses() {
    let press = press_profile("Press");
    let float = gray_doc(W, H, 32);
    assert_eq!(
        to_cmyk(float, &press, &json!({})).unwrap_err(),
        FLOAT_REFUSAL
    );
    assert_eq!(
        cmyk_plan(float, &press, &json!({}))["refusal"],
        FLOAT_REFUSAL
    );
    free(float);

    let gray = gray_doc(W, H, 16);
    let layered = with_adjustment(gray);
    let plan = cmyk_plan(layered, &press, &json!({}));
    assert_eq!(plan["flatten_required"], true);
    assert!(plan["refusal"]
        .as_str()
        .unwrap()
        .contains(ADJUSTMENT_REFUSAL));
    let refusal = to_cmyk(layered, &press, &json!({})).unwrap_err();
    assert!(refusal.contains(ADJUSTMENT_REFUSAL), "{refusal}");
    let flatten = json!({"flatten":true});
    let direct = to_cmyk(layered, &press, &flatten).unwrap();
    let rgb = twin(layered);
    let via = to_cmyk(rgb, &press, &flatten).unwrap();
    assert_eq!(descriptor(direct)["layer_count"], 1);
    assert_eq!(inks(direct, 0), inks(via, 0));
    for doc in [direct, via, rgb, layered, gray] {
        free(doc);
    }
}

// ------------------------------------------------------------ the proof --

fn render(doc: *const PrecisionDocument) -> *mut PrecisionImage {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "render: {}", take_err_string(err));
    out
}

fn image_bytes(image: *mut DrImage) -> Vec<u8> {
    let (w, h) = unsafe { (dr_image_width(image), dr_image_height(image)) };
    let bytes = unsafe {
        std::slice::from_raw_parts(dr_image_pixels_rgba(image), w as usize * h as usize * 4)
    }
    .to_vec();
    unsafe { dr_image_free(image) };
    bytes
}

/// The typed proof's bytes, info, mask and count.
fn typed_proof(
    image: *const PrecisionImage,
    press: &[u8],
    setup: &str,
) -> (Vec<u8>, String, Vec<u8>, u64) {
    let setup = CString::new(setup).unwrap();
    let mut info: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_proof_render(
            image,
            press.as_ptr(),
            press.len(),
            setup.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    assert!(!out.is_null(), "typed proof: {}", take_err_string(err));
    let bytes = image_bytes(out);
    let info = text_of(info);
    let mut mask = vec![0u8; (W * H) as usize];
    let mut count = 0u64;
    let ok = unsafe {
        dr_precision_proof_gamut_mask(
            image,
            press.as_ptr(),
            press.len(),
            setup.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            mask.as_mut_ptr(),
            mask.len(),
            &mut count,
            &mut err,
        )
    };
    assert!(ok, "typed mask: {}", take_err_string(err));
    (bytes, info, mask, count)
}

/// The byte path: the explicit preview, then the byte proof with the
/// document's profile bytes.
fn byte_proof(
    image: *const PrecisionImage,
    document: &[u8],
    press: &[u8],
    setup: &str,
) -> (Vec<u8>, String, Vec<u8>, u64) {
    let mut err: *mut c_char = ptr::null_mut();
    let flat = unsafe { dr_precision_image_preview_rgba8(image, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!flat.is_null(), "preview: {}", take_err_string(err));
    let setup = CString::new(setup).unwrap();
    let mut info: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_proof_render(
            flat,
            document.as_ptr(),
            document.len(),
            press.as_ptr(),
            press.len(),
            setup.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    assert!(!out.is_null(), "byte proof: {}", take_err_string(err));
    let bytes = image_bytes(out);
    let info = text_of(info);
    let mut mask = vec![0u8; (W * H) as usize];
    let mut count = 0u64;
    let ok = unsafe {
        dr_proof_gamut_mask(
            flat,
            document.as_ptr(),
            document.len(),
            press.as_ptr(),
            press.len(),
            setup.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            mask.as_mut_ptr(),
            mask.len(),
            &mut count,
            &mut err,
        )
    };
    assert!(ok, "byte mask: {}", take_err_string(err));
    unsafe { dr_image_free(flat) };
    (bytes, info, mask, count)
}

#[test]
fn a_gray_proof_is_the_rgb_proof_of_its_twin() {
    let press = press_profile("Press");
    let setups = [
        "{}",
        r#"{"simulate_paper_white":true,"simulate_ink_black":true,"gamut_warning":true}"#,
        r#"{"plate":"black"}"#,
    ];
    for depth in [8u32, 16] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let profile = profile_bytes(gray);
        assert_eq!(profile, profile_bytes(rgb), "{depth}: the twin's profile");
        let (gray_image, rgb_image) = (render(gray), render(rgb));
        for setup in setups {
            let what = format!("{depth} {setup}");
            assert_eq!(
                typed_proof(gray_image, &press, setup),
                typed_proof(rgb_image, &press, setup),
                "typed {what}"
            );
            assert_eq!(
                byte_proof(gray_image, &profile, &press, setup),
                byte_proof(rgb_image, &profile, &press, setup),
                "byte {what}"
            );
        }
        unsafe {
            dr_precision_image_free(gray_image);
            dr_precision_image_free(rgb_image);
        }
        free(rgb);
        free(gray);
    }
}
