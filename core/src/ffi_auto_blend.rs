//! The Auto-Blend Layers bridge: the plan and the edit at both depths,
//! for both methods (Stack Images, Panorama), over `doc_auto_blend` and
//! `precision_auto_blend`. The request is the strict JSON
//! `AutoBlendRequest::parse` reads (`"method"` optional, `stack_images` when
//! absent), borrowed under a 64 KiB cap.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_auto_blend::{AutoBlendReport, AutoBlendRequest};
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle, write_string};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

/// The request's byte cap, the one `AutoBlendRequest::parse` enforces again.
const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// The edit's answer with its report already serialized, so nothing that can
/// fail is left for the success arm.
fn serialized<T>(result: (T, AutoBlendReport)) -> (T, String) {
    (result.0, result.1.json().to_string())
}

/// Hands the document to the host and its report through `report_out`.
///
/// # Safety
/// `report_out` must be NULL or a valid pointer to writable `*mut c_char`.
unsafe fn publish<T>(result: (T, String), report_out: *mut *mut c_char) -> *mut T {
    unsafe { write_string(report_out, &result.1) };
    boxed(result.0)
}

/// The plan of `dr_doc_auto_blend` as owned JSON `{"retained_bytes",
/// "scratch_bytes","work_units","merged_index"}`; free it with
/// `dr_string_free`. NULL with `err_out` set for a NULL document or request
/// and for every refusal the edit would give.
///
/// # Safety
/// `document` must be NULL or a live `DrDocument`; `request` NULL or a
/// NUL-terminated string; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_auto_blend_plan(
    document: *const DrDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning auto-blend layers",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", MAX_REQUEST_BYTES)?;
                let plan = document.auto_blend_plan(&AutoBlendRequest::parse(request)?)?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Auto-Blend Layers on a byte document: a new document with a layer mask on
/// each requested layer and, with `seamless`, the merged layer above the top
/// requested layer's clip run (Stack Images) or the frames' pixels blended
/// across the seams (Panorama). `report_out` (may be NULL) receives, on
/// success only, owned JSON `{"method","layers":[{"layer","won_pixels"}],
/// "covered_pixels","canvas_pixels","merged_layer"}`, each `layers` row with
/// `"gain":[r,g,b]` for Panorama with `seamless`. NULL always carries an
/// error: a NULL document or request, a refusal, a plan over `limit`, or a
/// cancel (`operation cancelled`). The input is never changed.
///
/// # Safety
/// `document` must be NULL or a live `DrDocument`; `request` NULL or a
/// NUL-terminated string; `control` NULL or live until the call returns;
/// `report_out` and `err_out` NULL or writable. Free the result with
/// `dr_doc_free` and the strings with `dr_string_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_auto_blend(
    document: *const DrDocument,
    request: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    report_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "auto-blending layers",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", MAX_REQUEST_BYTES)?;
                document
                    .auto_blend(
                        &AutoBlendRequest::parse(request)?,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )
                    .map(serialized)
            },
            |result| publish(result, report_out),
        )
    }
}

/// `dr_doc_auto_blend_plan` for a 16-bit or 32-bit float document: the same
/// JSON, with the masks at 2 bytes a pixel and the merged record (or
/// Panorama's written-back frames) at 8, or 16 in a float document.
///
/// # Safety
/// `document` must be NULL or a live `PrecisionDocument`; `request` NULL or
/// a NUL-terminated string; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_auto_blend_plan(
    document: *const PrecisionDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning auto-blend layers",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", MAX_REQUEST_BYTES)?;
                let plan = document.auto_blend_plan(&AutoBlendRequest::parse(request)?)?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// `dr_doc_auto_blend` for a 16-bit or 32-bit float document: u16 masks and
/// a merged record (or Panorama's frames) in the document's own storage; a
/// float document scores, cuts and blends in the retouch encoding and keeps
/// values above white. The
/// report, the refusals and the NULL rule are the byte export's.
///
/// # Safety
/// `document` must be NULL or a live `PrecisionDocument`; `request` NULL or
/// a NUL-terminated string; `control` NULL or live until the call returns;
/// `report_out` and `err_out` NULL or writable. Free the result with
/// `dr_precision_doc_free` and the strings with `dr_string_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_auto_blend(
    document: *const PrecisionDocument,
    request: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    report_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "auto-blending layers",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", MAX_REQUEST_BYTES)?;
                document
                    .auto_blend(
                        &AutoBlendRequest::parse(request)?,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )
                    .map(serialized)
            },
            |result| publish(result, report_out),
        )
    }
}
