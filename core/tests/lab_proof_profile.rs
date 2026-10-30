//! #1 Lab phase 5b: a Lab Color document's soft proof and its profile
//! commands, through the FFI.
//!
//! The oracle of the proof is the RGB proof of the document's Lab → RGB
//! rendering: the Lab document converted by `dr_precision_doc_convert_lab_to_rgb`
//! into Display P3 (relative colorimetric, no BPC) — the route
//! `lab_gray.rs` (f) proves equal to the Lab view — rendered and proofed by
//! the same typed exports.
//!
//! Bounds, declared before measuring:
//!
//! * (a) at 8 and 16 bits, on a scene with one colour outside the synthetic
//!   press's gamut (`cmyk_fixture::press_profile`, built by
//!   `common/icc_lut_builders.rs`), `dr_precision_proof_render` and
//!   `dr_precision_proof_gamut_mask` of `dr_precision_image_lab_display`
//!   of the document's projection equal, byte for byte (image, info, mask
//!   and count), the same calls on the projection of the Display P3
//!   conversion, under three setups, and the gamut warning's count is > 0;
//! * (b) both typed proof exports on the Lab projection itself, and the byte
//!   `dr_proof_render` / `dr_proof_gamut_mask` with `doc_icc` the twin,
//!   refuse with exactly `PROOF_VIEW`;
//! * (c) the four profile exports refuse with exactly `PROFILE_EDIT`; the
//!   `profile`, `color_assign` and `color_convert` edits refuse with the
//!   "no document profile" sentence; `supported_operations` holds `proof`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_new_lab, dr_precision_image_lab_display,
};
use doctored_core::ffi_output_color::{
    dr_precision_doc_assign_profile_rasterizing, dr_precision_doc_convert_to_profile_with,
    dr_precision_proof_gamut_mask, dr_precision_proof_render, dr_proof_gamut_mask, dr_proof_render,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_render, dr_precision_image_free,
    dr_precision_image_preview_rgba8,
};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_assign_profile, dr_precision_doc_convert_to_profile,
};
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::gray_ops::{edit, free, plan_json, LIMIT};

const W: u32 = 8;
const H: u32 = 2;
const PROOF_VIEW: &str =
    "a Lab Color projection proofs through its Display P3 view (dr_precision_image_lab_display)";
const PROFILE_EDIT: &str = "a Lab Color document has no document profile: it converts through \
     Image ▸ Mode (dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_convert_to_cmyk, \
     dr_precision_doc_convert_to_gray)";

/// (L\*, a\*, b\*) patches: neutrals, moderate colours, and (index 5) a
/// saturated blue far outside the synthetic press's gamut.
const PATCHES: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [50.0, 0.0, 0.0],
    [100.0, 0.0, 0.0],
    [60.0, 20.0, 10.0],
    [40.0, -10.0, 30.0],
    [32.0, 70.0, -100.0],
    [75.0, -30.0, 50.0],
    [20.0, 40.0, 30.0],
];

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
    plan_json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err).unwrap()
}

/// The Lab twin profile, from a new Lab document.
fn twin() -> Arc<IccProfile> {
    let request = CString::new(r#"{"depth":16,"fill_srgb":[1,1,1,1]}"#).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(1, 1, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let profile = unsafe { &*doc }.profile().clone();
    free(doc);
    profile
}

/// One Lab colour as twin codes at `bits` (8 bits: on the ×257 grid).
fn lab_codes(lab: [f64; 3], bits: u32) -> [u16; 4] {
    let unit = [
        lab[0] / 100.0,
        (lab[1] + 128.0) / 255.0,
        (lab[2] + 128.0) / 255.0,
    ];
    let code = |u: f64| {
        if bits == 8 {
            ((u * 255.0).round() * 257.0) as u16
        } else {
            (u * 65535.0).round() as u16
        }
    };
    [code(unit[0]), code(unit[1]), code(unit[2]), 65535]
}

/// A one-layer Lab document of the patches (row 1 the patches reversed).
fn lab_doc(bits: u32) -> *mut PrecisionDocument {
    let profile = twin();
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let k = if i < W { i } else { 2 * W - 1 - i } as usize;
            lab_codes(PATCHES[k], bits)
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, profile.clone(), None).unwrap();
    let layer = Layer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
    .with_lab_depth(bits)
    .unwrap();
    Box::into_raw(Box::new(doc))
}

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

/// A typed proof's (bytes, info, mask, count).
type TypedProof = (Vec<u8>, String, Vec<u8>, u64);

/// The typed proof's (bytes, info, mask, count), or the first refusal.
fn typed_proof(
    image: *const PrecisionImage,
    press: &[u8],
    setup: &str,
) -> Result<TypedProof, (String, String)> {
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
    let render_err = if out.is_null() {
        Some(take_err_string(err))
    } else {
        None
    };
    let mut mask = vec![0u8; (W * H) as usize];
    let mut count = 0u64;
    let mut err: *mut c_char = ptr::null_mut();
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
    let mask_err = if ok { None } else { Some(take_err_string(err)) };
    match (render_err, mask_err) {
        (None, None) => Ok((image_bytes(out), text_of(info), mask, count)),
        (r, m) => {
            if !out.is_null() {
                unsafe { dr_image_free(out) };
            }
            Err((r.unwrap_or_default(), m.unwrap_or_default()))
        }
    }
}

/// The byte path with `doc_icc` = `document`: (render refusal, mask refusal).
fn byte_refusals(image: *const PrecisionImage, document: &[u8], press: &[u8]) -> (String, String) {
    let mut err: *mut c_char = ptr::null_mut();
    let flat = unsafe { dr_precision_image_preview_rgba8(image, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!flat.is_null(), "preview: {}", take_err_string(err));
    let setup = CString::new("{}").unwrap();
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
    assert!(out.is_null(), "the byte proof of the twin refuses");
    let render_err = take_err_string(err);
    let mut mask = vec![0u8; (W * H) as usize];
    let mut count = 0u64;
    let mut err: *mut c_char = ptr::null_mut();
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
    assert!(!ok, "the byte gamut mask of the twin refuses");
    unsafe { dr_image_free(flat) };
    (render_err, take_err_string(err))
}

fn to_p3(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let p3 = IccProfile::display_p3();
    let options =
        CString::new(r#"{"intent":"relative","black_point_compensation":false}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            doc,
            p3.bytes().as_ptr(),
            p3.bytes().len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to P3: {}", take_err_string(err));
    out
}

fn lab_view(projection: *const PrecisionImage) -> *mut PrecisionImage {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_image_lab_display(projection, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

fn profile_bytes(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0;
    let pointer = unsafe { dr_precision_doc_profile(doc, &mut length) };
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

// ------------------------------------------------------------ (a), (b) --

#[test]
fn a_lab_proof_is_the_rgb_proof_of_its_display_p3_rendering() {
    let press = press_profile("Press");
    let setups = [
        "{}",
        r#"{"simulate_paper_white":true,"simulate_ink_black":true,"gamut_warning":true}"#,
        r#"{"gamut_warning":true,"intent":"perceptual"}"#,
    ];
    for bits in [8u32, 16] {
        let lab = lab_doc(bits);
        let rgb = to_p3(lab);
        let projection = render(lab);
        let view = lab_view(projection);
        let oracle = render(rgb);
        for setup in setups {
            let ours = typed_proof(view, &press, setup).unwrap();
            let theirs = typed_proof(oracle, &press, setup).unwrap();
            assert_eq!(ours, theirs, "{bits} bits, {setup}");
            assert!(
                ours.3 > 0,
                "{bits} bits, {setup}: one colour is out of gamut"
            );
        }
        // (b) The projection in the twin itself refuses on both paths.
        let refusal = typed_proof(projection, &press, "{}").unwrap_err();
        assert_eq!(
            refusal,
            (PROOF_VIEW.into(), PROOF_VIEW.into()),
            "{bits} bits"
        );
        let twin = profile_bytes(lab);
        assert_eq!(
            byte_refusals(projection, &twin, &press),
            (PROOF_VIEW.into(), PROOF_VIEW.into()),
            "{bits} bits, byte path"
        );
        unsafe {
            dr_precision_image_free(oracle);
            dr_precision_image_free(view);
            dr_precision_image_free(projection);
        }
        free(rgb);
        free(lab);
    }
}

// ----------------------------------------------------------------- (c) --

#[test]
fn the_profile_commands_refuse_for_good_and_proof_is_admitted() {
    let lab = lab_doc(16);
    let srgb = IccProfile::srgb();
    let icc = srgb.bytes();
    let options = CString::new(r#"{"intent":"relative"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let assigned = unsafe {
        dr_precision_doc_assign_profile(
            lab,
            icc.as_ptr(),
            icc.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(assigned.is_null());
    assert_eq!(take_err_string(err), PROFILE_EDIT);
    let mut err: *mut c_char = ptr::null_mut();
    let converted = unsafe {
        dr_precision_doc_convert_to_profile(
            lab,
            icc.as_ptr(),
            icc.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(converted.is_null());
    assert_eq!(take_err_string(err), PROFILE_EDIT);
    let mut err: *mut c_char = ptr::null_mut();
    let mut report: *mut c_char = ptr::null_mut();
    let with = unsafe {
        dr_precision_doc_convert_to_profile_with(
            lab,
            icc.as_ptr(),
            icc.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    assert!(with.is_null() && report.is_null());
    assert_eq!(take_err_string(err), PROFILE_EDIT);
    let mut err: *mut c_char = ptr::null_mut();
    let rasterizing = unsafe {
        dr_precision_doc_assign_profile_rasterizing(
            lab,
            icc.as_ptr(),
            icc.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(rasterizing.is_null());
    assert_eq!(take_err_string(err), PROFILE_EDIT);
    for op in ["profile", "color_assign", "color_convert"] {
        assert_eq!(
            edit(lab, &json!({ "op": op }), &[]).unwrap_err(),
            format!("{op} is unavailable in a Lab Color document, which has no document profile.")
        );
    }
    let d = descriptor(lab);
    assert!(
        d["supported_operations"]
            .as_array()
            .unwrap()
            .contains(&"proof".into()),
        "{}",
        d["supported_operations"]
    );
    free(lab);
}
