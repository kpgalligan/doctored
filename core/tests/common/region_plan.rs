//! The regional render plan, for the tests that pin its admission and its
//! numbers, read through the plan exports MCP `render` reports from
//! (`dr_doc_render_region_plan`, `dr_precision_doc_render_region_plan`): the
//! owned JSON string parsed and freed, a NULL return's `err_out` as the
//! refusal. A standalone `#[path]` module, not part of `common/mod.rs`.
#![allow(dead_code)]

use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_regional::{dr_doc_render_region_plan, dr_precision_doc_render_region_plan};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;

/// The export's string parsed, or its `err_out` message; both freed.
fn taken(out: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    if out.is_null() {
        assert!(!err.is_null(), "a NULL plan carries its refusal");
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        return Err(message);
    }
    assert!(err.is_null(), "err_out set on success");
    let text = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    Ok(serde_json::from_str(&text).expect("the plan is JSON"))
}

/// The 8-bit plan's JSON, or the refusal the render would raise.
pub fn byte(
    doc: &DrDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
    max_output_bytes: u64,
) -> Result<Value, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_render_region_plan(doc, x, y, w, h, d, max_output_bytes, &mut err) };
    taken(out, err)
}

/// The typed plan's JSON, or the refusal the render would raise.
pub fn typed(
    doc: &PrecisionDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> Result<Value, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_render_region_plan(doc, x, y, w, h, d, &mut err) };
    taken(out, err)
}
