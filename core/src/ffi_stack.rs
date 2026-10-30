//! Bounded JSON seam for byte Stack Modes: `stack_stat`'s request
//! against `doc_stack_mode`'s plan and edit. The typed request rides
//! `dr_precision_doc_edit[_plan]` as the `stack_mode` op.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::stack_stat::StackModeRequest;

/// Plan a byte Stack Mode. Returned JSON holds `retained_bytes`,
/// `scratch_bytes` and `work_units`; free it with `dr_string_free`. NULL with
/// an error for every refusal the edit gives before it reads a pixel.
/// # Safety
/// document/request are NULL or live/terminated; err_out is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_stack_mode_plan(
    document: *const DrDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning stack mode",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", 64 * 1024)?;
                let plan = document.stack_mode_plan(&StackModeRequest::parse(request)?)?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Apply a byte Stack Mode: one new raster holding the statistic directly
/// above the request's `above` layer, with every member hidden. NULL always
/// carries an error (a refusal or a cancel); the edit is never a no-op.
/// # Safety
/// document/control are NULL or live; request is NULL or terminated; err_out
/// is NULL or writable. The caller owns every handle for the full call.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_stack_mode(
    document: *const DrDocument,
    request: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying stack mode",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", 64 * 1024)?;
                document.stack_mode(
                    &StackModeRequest::parse(request)?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |result| {
                result.map_or(ptr::null_mut(), |document| {
                    Box::into_raw(Box::new(document))
                })
            },
        )
    }
}
