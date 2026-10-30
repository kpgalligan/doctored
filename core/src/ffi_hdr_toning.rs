//! Image ▸ Adjustments ▸ HDR Toning… across the FFI:
//! the plan for a typed and for an 8-bit document, the toning of a typed one
//! and the Toning Curve's histogram (`doc_hdr_toning` holds the refusals and
//! the memory plan; `hdr_toning` and `hdr_local_adaptation` the request and
//! the pixel math). Every export is a `fallible_op` shim: a
//! NULL handle, a NULL or oversized request, a refusal and a cancel all
//! answer NULL with a message.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_hdr_toning::MAX_REQUEST_BYTES;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

/// The request argument, bounded before it is read.
///
/// # Safety
/// `request` must be NULL or a NUL-terminated string readable for the borrow.
unsafe fn request_text<'a>(request: *const c_char) -> Result<&'a str, String> {
    unsafe { borrow_cstr_bounded(request, "hdr toning request", MAX_REQUEST_BYTES) }
}

fn owned(text: String) -> Result<CString, String> {
    CString::new(text).map_err(|e| e.to_string())
}

/// The plan of `dr_precision_doc_hdr_toning` into `bit_depth` bits (16, or 8
/// for a 32-bit float document, narrowed by the host): owned JSON
/// `{"working_memory_plan_bytes","output_bytes"}`, after every refusal the
/// toning gives before it allocates. NULL with a message on a NULL argument,
/// a malformed request, a CMYK document, a depth it does not take and a
/// profile that cannot carry float. A Grayscale document (#547 phase 2e)
/// takes only its own depth (8 or 16, no narrowing); a 32-bit one (phase 5a)
/// takes 8 or 16, its 8-bit plan holding the host's Gray depth edit
/// (`gray_profile::depth_eight_bytes`). Free the result with
/// `dr_string_free`.
///
/// # Safety
/// `document` must be NULL or live for the call; `request_json` NULL or a
/// NUL-terminated string; `err_out` NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_hdr_toning_plan(
    document: *const PrecisionDocument,
    request_json: *const c_char,
    bit_depth: u32,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning hdr toning",
            ptr::null_mut(),
            || {
                let source = required_handle(document)?;
                let request = request_text(request_json)?;
                owned(source.hdr_toning_plan(request, bit_depth)?.json())
            },
            CString::into_raw,
        )
    }
}

/// The plan of toning an 8-bit document as the host runs it (the flattened
/// promotion, `dr_precision_doc_hdr_toning` on it, the narrowing back to 8
/// bits): owned JSON `{"working_memory_plan_bytes","output_bytes"}`. NULL
/// with a message on a NULL argument, a malformed request and a profile that
/// cannot carry float. Free the result with `dr_string_free`.
///
/// # Safety
/// `doc` must be NULL or live for the call; `request_json` NULL or a
/// NUL-terminated string; `err_out` NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_hdr_toning_plan(
    doc: *const DrDocument,
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning hdr toning",
            ptr::null_mut(),
            || {
                let source = required_handle(doc)?;
                let request = request_text(request_json)?;
                owned(source.hdr_toning_plan(request)?.json())
            },
            CString::into_raw,
        )
    }
}

/// `document`'s composite flattened and tone mapped by `request_json`
/// (`{"method":"exposure_gamma","exposure":f,"gamma":f}`,
/// `{"method":"highlight_compression"}`, `{"method":"equalize_histogram"}`
/// or `{"method":"local_adaptation", …}` with its keys in
/// `hdr_local_adaptation`'s module doc; unknown keys refused) into a NEW
/// one-raster 16-bit document "Background" with the source's profile,
/// metadata, resolution, channels, guides and
/// paths. `document` is unchanged. Admitted against `limit` with
/// `other_retained_bytes` held elsewhere, and reserved on `control`, before
/// it renders. NULL with a message on a NULL handle or request, a malformed
/// request, a CMYK document, a profile that cannot carry float, a
/// working-memory refusal and a cancel. A Grayscale document at 8 or 16 bits
/// (#547 phase 2e) answers a Grayscale document at its own depth, each mapped
/// pixel folded back to one gray value (`doc_hdr_toning`'s "Grayscale"); a
/// 32-bit one (phase 5a) answers a 16-bit Grayscale document. Free the
/// result with `dr_precision_doc_free`.
///
/// # Safety
/// `document` and `control` must each be NULL or live for the call
/// (`control` single-use); `request_json` NULL or a NUL-terminated string;
/// `err_out` NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_hdr_toning(
    document: *const PrecisionDocument,
    request_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying hdr toning",
            ptr::null_mut(),
            || {
                let source = required_handle(document)?;
                let request = request_text(request_json)?;
                source.hdr_toning(request, limit, other_retained_bytes, control.as_ref())
            },
            boxed,
        )
    }
}

/// The Toning Curve's histogram of `document`'s composite: owned JSON
/// `{"bins":[256 alpha-weighted bins of the clamped log-luminance position],
/// "low_stops","high_stops"}` (the range's ends, log2 of linear luminance).
/// Admitted against `limit` with `other_retained_bytes` held elsewhere, and
/// reserved on `control` under the default Local Adaptation plan, before it
/// renders. NULL with a message on a NULL handle, a CMYK document, a profile
/// that cannot carry float, a working-memory refusal and a cancel. Free the result with
/// `dr_string_free`.
///
/// # Safety
/// `document` and `control` must each be NULL or live for the call
/// (`control` single-use); `err_out` NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_hdr_toning_histogram(
    document: *const PrecisionDocument,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading the hdr toning histogram",
            ptr::null_mut(),
            || {
                let source = required_handle(document)?;
                let histogram =
                    source.hdr_toning_histogram(limit, other_retained_bytes, control.as_ref())?;
                owned(histogram.json())
            },
            CString::into_raw,
        )
    }
}
