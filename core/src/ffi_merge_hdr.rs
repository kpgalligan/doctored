//! Merge to HDR across the FFI: the exposure estimate, the plan and the
//! merge, each for an 8-bit document and for a typed one (`doc_merge_hdr`
//! holds the request, the refusals and the memory plan; `merge_hdr` the pixel
//! math). Every export is a `fallible_op` shim: a NULL handle, a NULL or
//! oversized request, a refusal and a cancel all answer NULL with a message.

use crate::doc::DrDocument;
use crate::doc_merge_hdr::MAX_REQUEST_BYTES;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle, write_string};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use std::ffi::{c_char, CString};
use std::ptr;

/// The request argument, bounded before it is read.
///
/// # Safety
/// `request` must be NULL or a NUL-terminated string readable for the borrow.
unsafe fn request_text<'a>(request: *const c_char) -> Result<&'a str, String> {
    unsafe { borrow_cstr_bounded(request, "merge to hdr request", MAX_REQUEST_BYTES) }
}

fn owned(text: String) -> Result<CString, String> {
    CString::new(text).map_err(|e| e.to_string())
}

/// The relative exposures of 2…16 raster layers of an 8-bit document,
/// estimated from their pixels: owned JSON `{"evs":[…]}` in stops relative to
/// the first listed layer, or `{"evs":null,"reason":…}` when a neighbouring
/// pair shares too few well-exposed pixels. `request_json` is
/// `{"version":1,"layers":[int…]}` (at most 64 KiB, unknown keys refused).
/// NULL with a message for a NULL handle or request, a malformed request, a
/// group or adjustment layer, a profile that cannot carry float, and a
/// cancel. Free the result with `dr_string_free`.
///
/// # Safety
/// `doc` and `control` must each be NULL or live for the call (`control`
/// single-use); `request_json` NULL or a NUL-terminated string; `err_out`
/// NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_merge_hdr_exposures(
    doc: *const DrDocument,
    request_json: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "estimating merge to hdr exposures",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let request = request_text(request_json)?;
                owned(document.merge_hdr_exposures(request, control.as_ref())?)
            },
            CString::into_raw,
        )
    }
}

/// The plan of `dr_doc_merge_hdr` for the same request: owned JSON
/// `{"width","height","frames","output_bytes","scratch_bytes"}`, after every
/// refusal the merge gives before it allocates. `request_json` is
/// `{"version":1,"layers":[int…],"evs":[number…],"reference":int,
/// "deghost":bool}`. NULL with a message on a NULL argument or a refusal.
/// Free the result with `dr_string_free`.
///
/// # Safety
/// `doc` must be NULL or live for the call; `request_json` NULL or a
/// NUL-terminated string; `err_out` NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_merge_hdr_plan(
    doc: *const DrDocument,
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning merge to hdr",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                owned(document.merge_hdr_plan(request_text(request_json)?)?)
            },
            CString::into_raw,
        )
    }
}

/// The listed raster layers of an 8-bit document merged into a NEW 32-bit
/// float document: one raster layer "Merged HDR" holding the exposure-
/// weighted scene-linear merge on the reference layer's scale, with the
/// source's profile and resolution, no metadata and the clip tone map. `doc`
/// is unchanged. `evs` are stops (larger = more light), one per layer, each
/// within 15 of the reference's; `deghost` gives a pixel where a frame
/// disagrees with the reference to the reference alone. Admitted against
/// `limit` with `other_retained_bytes` held elsewhere, and reserved on
/// `control`, before a row is read. `report_out` (may be NULL) receives, on
/// success only, owned JSON `{"reference","all_clipped_pixels",
/// "all_dark_pixels","deghosted_pixels","noise_sigma":[number|null…]}`; free
/// it with `dr_string_free`. NULL with a message for a NULL handle or
/// request, a malformed request, a group or adjustment layer, a profile that
/// cannot carry float, an EV past the span, a working-memory refusal and a
/// cancel. Free the result with `dr_precision_doc_free`.
///
/// # Safety
/// `doc` and `control` must each be NULL or live for the call (`control`
/// single-use); `request_json` NULL or a NUL-terminated string; `report_out`
/// and `err_out` each NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_merge_hdr(
    doc: *const DrDocument,
    request_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    report_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "merging to hdr",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let request = request_text(request_json)?;
                document.merge_hdr(request, limit, other_retained_bytes, control.as_ref())
            },
            |(merged, report)| {
                write_string(report_out, &report);
                boxed(merged)
            },
        )
    }
}

/// `dr_doc_merge_hdr_exposures` over the raster records of a 16-bit
/// document. A 32-bit float document refuses, naming the layer ("… is already
/// 32-bit float; Merge to HDR takes 8- or 16-bit exposures").
///
/// # Safety
/// `document` and `control` must each be NULL or live for the call (`control`
/// single-use); `request_json` NULL or a NUL-terminated string; `err_out`
/// NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_merge_hdr_exposures(
    document: *const PrecisionDocument,
    request_json: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "estimating merge to hdr exposures",
            ptr::null_mut(),
            || {
                let source = required_handle(document)?;
                let request = request_text(request_json)?;
                owned(source.merge_hdr_exposures(request, control.as_ref())?)
            },
            CString::into_raw,
        )
    }
}

/// `dr_doc_merge_hdr_plan` over the raster records of a 16-bit document; a
/// 32-bit float document refuses as the merge does.
///
/// # Safety
/// `document` must be NULL or live for the call; `request_json` NULL or a
/// NUL-terminated string; `err_out` NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_merge_hdr_plan(
    document: *const PrecisionDocument,
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning merge to hdr",
            ptr::null_mut(),
            || {
                let source = required_handle(document)?;
                owned(source.merge_hdr_plan(request_text(request_json)?)?)
            },
            CString::into_raw,
        )
    }
}

/// `dr_doc_merge_hdr` over the raster records of a 16-bit document: the same
/// request, report, admission and result. A 32-bit float document refuses,
/// naming the layer ("… is already 32-bit float; Merge to HDR takes 8- or
/// 16-bit exposures"). `document` is unchanged.
///
/// # Safety
/// `document` and `control` must each be NULL or live for the call (`control`
/// single-use); `request_json` NULL or a NUL-terminated string; `report_out`
/// and `err_out` each NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_merge_hdr(
    document: *const PrecisionDocument,
    request_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    report_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "merging to hdr",
            ptr::null_mut(),
            || {
                let source = required_handle(document)?;
                let request = request_text(request_json)?;
                source.merge_hdr(request, limit, other_retained_bytes, control.as_ref())
            },
            |(merged, report)| {
                write_string(report_out, &report);
                boxed(merged)
            },
        )
    }
}
