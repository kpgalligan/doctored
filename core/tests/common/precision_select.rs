//! Shared helper for the Lab selection tests (`lab_wand.rs`,
//! `lab_wand_photoshop.rs`): one `dr_precision_doc_select` call, planned
//! through `dr_precision_doc_edit_plan` first.
use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::LIMIT;

/// `dr_precision_doc_select` with `input` (empty: none).
pub(crate) fn select(
    doc: *const PrecisionDocument,
    operation: &Value,
    input: &[u16],
) -> Result<Vec<u16>, String> {
    let text = CString::new(json!({"op":"selection","operation":operation}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if plan.is_null() {
        return Err(take_err_string(err));
    }
    let planned: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(plan) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(plan) };
    let mut out = vec![0u16; planned["output_samples"].as_u64().unwrap() as usize];
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}
