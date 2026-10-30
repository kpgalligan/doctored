//! Construction-time naming of the host u16 intake: the optional
//! `layer_name` of `dr_precision_doc_from_rgba16` names the one raster layer
//! when the document is built, NULL keeps today's "Background", and a name the
//! DRDC writer could not persist is refused by name with nothing built.
//!
//! Every fact is read back through the FFI (`dr_precision_doc_info`'s
//! `layers[0].name`), never through the crate's own structs.
use crate::common;
use common::take_err_string;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_info};
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::precision_document::PrecisionDocument;

use serde_json::Value;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 4;
const H: u32 = 3;
/// The DRDC name cap (`drdc::MAX_DRDC_NAME_LEN`), restated independently.
const NAME_LIMIT: usize = 64 * 1024;

fn profile() -> Vec<u8> {
    let len = dr_builtin_profile_len(0);
    let mut profile = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(0, profile.as_mut_ptr(), len) });
    profile
}

/// Builds a document through the FFI; `Err` carries the core's sentence.
fn intake(name: *const c_char) -> Result<*mut PrecisionDocument, String> {
    let samples: Vec<u16> = (0..W * H * 4).map(|i| (i * 1000) as u16).collect();
    let profile = profile();
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr(),
            samples.len(),
            W,
            H,
            profile.as_ptr(),
            profile.len(),
            name,
            LIMIT,
            0,
            ptr::null(),
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

fn layer_names(document: *mut PrecisionDocument) -> Vec<String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_info(document, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_owned();
    unsafe { dr_string_free(out) };
    let info: Value = serde_json::from_str(&text).unwrap();
    info["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn null_layer_name_keeps_background() {
    let document = intake(ptr::null()).unwrap();
    assert_eq!(layer_names(document), vec!["Background".to_owned()]);
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn layer_name_names_the_one_layer_at_construction() {
    for name in ["Layer 1", "Ébauche — 夜", ""] {
        let c_name = CString::new(name).unwrap();
        let document = intake(c_name.as_ptr()).unwrap();
        assert_eq!(layer_names(document), vec![name.to_owned()]);
        unsafe { dr_precision_doc_free(document) };
    }
}

#[test]
fn name_at_the_limit_is_accepted_and_one_byte_over_is_refused() {
    let at_limit = CString::new("n".repeat(NAME_LIMIT)).unwrap();
    let document = intake(at_limit.as_ptr()).unwrap();
    assert_eq!(layer_names(document)[0].len(), NAME_LIMIT);
    unsafe { dr_precision_doc_free(document) };

    let over = CString::new("n".repeat(NAME_LIMIT + 1)).unwrap();
    let message = intake(over.as_ptr()).unwrap_err();
    assert!(
        message.contains("layer name exceeds the supported byte limit"),
        "{message}"
    );
}

#[test]
fn invalid_utf8_name_is_refused() {
    let bad = CString::new(vec![b'L', 0xFF, b'x']).unwrap();
    let message = intake(bad.as_ptr()).unwrap_err();
    assert!(
        message.contains("layer name is not valid UTF-8"),
        "{message}"
    );
}
