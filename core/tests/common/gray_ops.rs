//! Shared helpers for #547 phase 2c's Grayscale filter and retouch tests
//! (`gray_filter.rs`, `gray_retouch.rs`): a Gray Gamma 2.2 document of a
//! pattern with partial alpha at 8, 16 or 32 bits, a uniform one, its RGB
//! TWIN (the Gray document converted to RGB through its own profile bytes,
//! which keeps every sample), the selected-edit and edit FFI wrappers, a
//! layer's samples and the R = G = B check.
#![allow(dead_code)]

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gray::{
    dr_precision_doc_convert_gray_to_rgb, dr_precision_doc_convert_to_gray,
    dr_precision_doc_new_gray,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;

use crate::common::take_err_string;

pub const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// Rec. 709 luma weights, from the standard.
pub const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];

pub fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

/// A document result: `Ok(None)` is NULL with no error (no change).
pub fn result(
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

/// A plan's JSON, or its refusal.
pub fn plan_json(out: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

/// One pattern pixel: sRGB 16-bit codes that vary in every channel, every
/// seventh pixel at partial alpha.
fn pattern(i: u32) -> [u16; 4] {
    let r = (i * 7919 + 1013) % 251;
    let g = (i * 104_729 + 37) % 241;
    let b = (i * 1299 + 501) % 239;
    let a = if i % 7 == 3 { 40_000 } else { 65_535 };
    [(r * 257 + 40) as u16, (g * 265) as u16, (b * 270) as u16, a]
}

/// `rgb` (an sRGB 16-bit document) as a Gray Gamma 2.2 document of `depth`;
/// frees `rgb`.
fn to_gray(rgb: *mut PrecisionDocument, depth: u32) -> *mut PrecisionDocument {
    let source = if depth == 32 {
        let mut err = ptr::null_mut();
        let float =
            unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!float.is_null(), "{}", take_err_string(err));
        free(rgb);
        float
    } else {
        rgb
    };
    let options = if depth == 32 {
        r#"{"space":"gray_gamma_22"}"#.to_string()
    } else {
        format!(r#"{{"space":"gray_gamma_22","depth":{depth}}}"#)
    };
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            source,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "to gray: {}", take_err_string(err));
    free(source);
    gray
}

/// A `w × h` Gray Gamma 2.2 document of `depth`: one layer of the pattern's
/// grays, partial alpha included.
pub fn gray_doc(w: u32, h: u32, depth: u32) -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = (0..w * h).map(pattern).collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    to_gray(rgb, depth)
}

/// A `w × h` Gray Gamma 2.2 document of `depth`, opaque, every pixel the
/// luminance of the sRGB gray `fill`.
pub fn uniform_gray(w: u32, h: u32, depth: u32, fill: f64) -> *mut PrecisionDocument {
    let request = CString::new(format!(
        r#"{{"depth":{depth},"space":"gray_gamma_22","fill_srgb":[{fill},{fill},{fill},1]}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_gray(w, h, ptr::null(), 0, request.as_ptr(), LIMIT, 0, &mut err)
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

/// The RGB twin: `gray` converted to RGB through its own profile's bytes.
pub fn twin(gray: *const PrecisionDocument) -> *mut PrecisionDocument {
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_profile(gray, &mut length) };
    assert!(!bytes.is_null());
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_gray_to_rgb(
            gray,
            bytes,
            length,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "twin: {}", take_err_string(err));
    out
}

/// `dr_precision_doc_selected_edit` under `coverage` (empty: everywhere).
pub fn selected(
    doc: *const PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            text.as_ptr(),
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
    result(out, err)
}

/// The planned `input_samples` of a `dr_precision_doc_edit` request.
pub fn plan_samples(doc: *const PrecisionDocument, request: &Value) -> Result<usize, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    Ok(plan_json(plan, err)?["input_samples"].as_u64().unwrap() as usize)
}

/// `dr_precision_doc_edit` with `input` verbatim.
pub fn edit(
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

/// Layer `index`'s samples: u16 codes, or the f32 bits at 32 bits.
pub fn layer(doc: *const PrecisionDocument, index: usize, depth: u32) -> Vec<[u32; 4]> {
    let image = unsafe { &*doc }.layer_source(index).unwrap();
    if depth == 32 {
        let pixels = image.to_rgba32().unwrap();
        pixels.iter().map(|p| p.map(f32::to_bits)).collect()
    } else {
        let pixels = image.to_rgba16().unwrap();
        pixels.iter().map(|p| p.map(u32::from)).collect()
    }
}

/// Layer `index`'s samples as numbers: codes at 8 and 16 bits, linear at 32.
pub fn values(doc: *const PrecisionDocument, index: usize, depth: u32) -> Vec<[f64; 4]> {
    layer(doc, index, depth)
        .iter()
        .map(|p| {
            if depth == 32 {
                p.map(|b| f64::from(f32::from_bits(b)))
            } else {
                p.map(f64::from)
            }
        })
        .collect()
}

pub fn assert_neutral(pixels: &[[u32; 4]], what: &str) {
    for (i, p) in pixels.iter().enumerate() {
        assert!(p[0] == p[1] && p[1] == p[2], "{what} pixel {i}: {p:?}");
    }
}
