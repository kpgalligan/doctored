//! Bounded JSON seam for byte document set and retained-source geometry.
//!
//! Byte geometry has bounded plan/edit exports; typed geometry uses the existing strict request
//! ABI. `{"op":"perspective_crop",…}` is answered by `doc_perspective_crop`.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::perspective_crop::PerspectiveCropRequest;
use crate::precision_geometry::GeometryEdit;

/// Plan a byte geometry edit. Returned JSON uses the shared source-plan
/// fields; free it with `dr_string_free`.
/// # Safety
/// document/request are NULL or live/terminated; err_out is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_geometry_edit_plan(
    document: *const DrDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning byte geometry",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", 64 * 1024)?;
                // Perspective Crop is routed before the shared parser,
                // whose exhaustive `GeometryEdit` never names it.
                let plan = if PerspectiveCropRequest::names(request) {
                    document.perspective_crop_plan(&PerspectiveCropRequest::parse(request)?)?
                } else {
                    document
                        .geometry_edit_plan(&GeometryEdit::parse(request)?)?
                        .json()
                };
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Apply a byte geometry edit. NULL/no error means unchanged.
/// # Safety
/// document/control are NULL or live; request is NULL or terminated; err_out
/// is NULL or writable. The caller owns every handle for the full call.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_geometry_edit(
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
            "applying byte geometry",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", 64 * 1024)?;
                if PerspectiveCropRequest::names(request) {
                    return document.perspective_crop(
                        &PerspectiveCropRequest::parse(request)?,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    );
                }
                let edit = GeometryEdit::parse(request)?;
                document.geometry_edit(&edit, limit, other_retained_bytes, control.as_ref())
            },
            |result| {
                result.map_or(ptr::null_mut(), |document| {
                    Box::into_raw(Box::new(document))
                })
            },
        )
    }
}
