//! A blank 16-bit document (File ▸ New… at 16 bits) is built by the
//! core from four fill samples. Since the banded reopen the reopen
//! envelope no longer bounds a blank canvas, and neither does the
//! decoder cap (a 16-bit plane never passes through that decoder) or the
//! stored-plane record limit (1 GiB): every canvas within `MAX_PIXELS` builds
//! at the 2 GiB reference limit, 8192² and the 100 MP 10000² included, and a
//! canvas past it refuses with `pixel_count`'s sentence.
//!
//! Black-box through `dr_precision_doc_create_blank`; every oracle is a
//! literal. The sRGB profile comes from `dr_builtin_profile(0, …)`.
use crate::common;
use common::take_err_string;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_info, dr_precision_doc_layer_source,
    dr_precision_image_free,
};
use doctored_core::ffi_precision_input::dr_precision_doc_create_blank;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

use serde_json::Value;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const WHITE: [u16; 4] = [65535, 65535, 65535, 65535];

fn profile() -> Vec<u8> {
    let len = dr_builtin_profile_len(0);
    let mut profile = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(0, profile.as_mut_ptr(), len) });
    profile
}

/// `pixel_count`'s sentence, written out independently of the core.
const PAST_THE_PIXEL_LIMIT: &str =
    "precision prototype dimensions exceed the supported pixel limit";

const NO_PROFILE: &str = "A 16-bit document needs an RGB matrix/TRC ICC profile.";

/// One FFI call; `Err` carries the core's sentence.
fn create(
    w: u32,
    h: u32,
    fill: *const u16,
    profile: &[u8],
    name: *const c_char,
) -> Result<*mut PrecisionDocument, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_create_blank(
            w,
            h,
            fill,
            profile.as_ptr(),
            profile.len(),
            name,
            REFERENCE_WORKING_BYTES,
            0,
            &mut err,
        )
    };
    if document.is_null() {
        Err(take_err_string(err))
    } else {
        assert!(err.is_null());
        Ok(document)
    }
}

fn blank(w: u32, h: u32) -> Result<*mut PrecisionDocument, String> {
    create(w, h, WHITE.as_ptr(), &profile(), ptr::null())
}

fn info(document: *mut PrecisionDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_info(document, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_owned();
    unsafe { dr_string_free(out) };
    serde_json::from_str(&text).unwrap()
}

#[test]
fn a_canvas_past_the_pixel_limit_refuses() {
    // 10001 × 10000 = 100,010,000 pixels, past `MAX_PIXELS`.
    assert_eq!(blank(10001, 10000).unwrap_err(), PAST_THE_PIXEL_LIMIT);
    assert_eq!(blank(12500, 8001).unwrap_err(), PAST_THE_PIXEL_LIMIT);
}

#[test]
fn an_8000_square_builds() {
    let document = blank(8000, 8000).unwrap();
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn the_ceiling_is_the_pixel_limit() {
    // 8192 · (8 · 8192 + 1) = 536,879,104 bytes of stored rows exceeded the
    // old 512 MiB record limit; under the 1 GiB limit 8192² builds, and
    // so does 10000², the 100 MP document limit (800,010,000 stored bytes).
    for (w, h) in [(8191, 8191), (8192, 8191), (8192, 8192), (10000, 10000)] {
        let document = blank(w, h).unwrap();
        unsafe { dr_precision_doc_free(document) };
    }
}

#[test]
fn a_7000_square_is_built() {
    let document = blank(7000, 7000).unwrap();
    let info = info(document);
    assert_eq!(info["layers"].as_array().unwrap().len(), 1);
    assert_eq!(info["layer_count"], 1);
    assert_eq!(
        (info["width"].as_u64(), info["height"].as_u64()),
        (Some(7000), Some(7000))
    );
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn the_fill_and_the_layer_name_are_the_callers() {
    let fill: [u16; 4] = [65535, 0, 32896, 65535];
    let document = create(3, 2, fill.as_ptr(), &profile(), ptr::null()).unwrap();
    let source = unsafe { dr_precision_doc_layer_source(document, 0) };
    assert!(!source.is_null());
    let samples = unsafe { &*source }.to_rgba16().unwrap();
    assert_eq!(samples, vec![[65535, 0, 32896, 65535]; 6]);
    unsafe { dr_precision_image_free(source) };
    assert_eq!(info(document)["layers"][0]["name"], "Background");
    unsafe { dr_precision_doc_free(document) };

    let name = CString::new("Layer 1").unwrap();
    let document = create(3, 2, fill.as_ptr(), &profile(), name.as_ptr()).unwrap();
    assert_eq!(info(document)["layers"][0]["name"], "Layer 1");
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn a_garbage_profile_is_refused_by_name() {
    assert_eq!(
        create(3, 2, WHITE.as_ptr(), &[7u8], ptr::null()).unwrap_err(),
        NO_PROFILE
    );
}

#[test]
fn null_arguments_refuse_without_crashing() {
    let profile = profile();
    assert_eq!(
        create(3, 2, ptr::null(), &profile, ptr::null()).unwrap_err(),
        "a blank document needs four fill samples"
    );
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_create_blank(
            3,
            2,
            WHITE.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            &mut err,
        )
    };
    assert!(document.is_null());
    assert_eq!(take_err_string(err), NO_PROFILE);
    let document = unsafe {
        dr_precision_doc_create_blank(
            3,
            2,
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null_mut(),
        )
    };
    assert!(document.is_null());
}

#[test]
fn the_refusal_is_the_documents_own_need() {
    // Nothing else is open (`other = 0`) and the whole limit is offered,
    // yet a canvas past the pixel limit still refuses: closing documents
    // cannot help.
    let mut err: *mut c_char = ptr::null_mut();
    let profile = profile();
    let document = unsafe {
        dr_precision_doc_create_blank(
            10001,
            10000,
            WHITE.as_ptr(),
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            &mut err,
        )
    };
    assert!(document.is_null());
    assert_eq!(take_err_string(err), PAST_THE_PIXEL_LIMIT);
}
