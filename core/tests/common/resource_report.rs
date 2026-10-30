//! The app's own resource inventory and cache eviction, for the tests that
//! measure retained bytes: `dr_mixed_resources_report_with_raw_masks` with no
//! local masks, and `dr_mixed_resources_evict_caches` with no typed documents.
//! Both are the exports `DoctoredCore.swift` calls, so these tests stay
//! black-box. A standalone `#[path]` module, not part of `common/mod.rs`.
#![allow(dead_code)]

use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_resources::{
    dr_mixed_resources_evict_caches, dr_mixed_resources_report_with_raw_masks,
};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;
use serde_json::Value;

/// Parses and frees an owned JSON result, or takes and frees its error.
fn owned(result: *mut c_char, error: *mut c_char) -> Result<Value, String> {
    if result.is_null() {
        if error.is_null() {
            return Err("NULL without an error".into());
        }
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        return Err(message);
    }
    let text = unsafe { CStr::from_ptr(result) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(result) };
    if !error.is_null() {
        unsafe { dr_string_free(error) };
    }
    serde_json::from_str(&text).map_err(|e| format!("{e}: {text}"))
}

fn array<T>(values: &[*const T]) -> *const *const T {
    if values.is_empty() {
        ptr::null()
    } else {
        values.as_ptr()
    }
}

/// One unique inventory over every supplied handle, with no local masks.
pub fn report(
    documents: &[*const DrDocument],
    images: &[*const DrImage],
    typed_documents: &[*const PrecisionDocument],
    typed_images: &[*const PrecisionImage],
    transient: u64,
    limit: u64,
) -> Result<Value, String> {
    let mut error: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_mixed_resources_report_with_raw_masks(
            array(documents),
            documents.len(),
            array(images),
            images.len(),
            array(typed_documents),
            typed_documents.len(),
            array(typed_images),
            typed_images.len(),
            ptr::null(),
            0,
            transient,
            limit,
            &mut error,
        )
    };
    owned(result, error)
}

/// Evicts the supplied documents' caches and the process-wide memos.
pub fn evict(documents: &[*const DrDocument]) -> Result<Value, String> {
    let mut error: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_mixed_resources_evict_caches(
            array(documents),
            documents.len(),
            ptr::null(),
            0,
            &mut error,
        )
    };
    owned(result, error)
}
