//! The document-context destructive adjustment seam. Parameter parsing and
//! per-pixel evaluation are shared with the original image-only export and
//! with adjustment-layer compositing respectively.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::adjust_calibrated::calibrated_refusal;
use crate::doc::DrDocument;
use crate::ffi_adjust::parse_adjustment;
use crate::ffi_util::{boxed, doc_get, fallible_op, handle_get, read_cstr};
use crate::precision_document::PrecisionDocument;
use crate::DrImage;

/// Apply an adjustment to the full image in this document's colour context.
/// `document_v2` Exposure/WB requires a supported RGB matrix/TRC profile;
/// missing evaluation or `legacy_srgb` retains the original numeric kernel.
/// A `document_v2` request is refused with `dr_doc_calibrated_refusal`'s
/// sentence for its op (no matrix/TRC model, or tone curves failing the
/// precision check where that op needs them) before anything is evaluated.
/// Alpha untouched. Selection/target writeback is a separate operation.
/// NULL on NULL doc/image. Invalid params/profile: NULL and `err_out`.
///
/// # Safety
/// `doc` and `img` must be NULL or valid live handles. The strings must be
/// NULL or valid NUL-terminated strings. `err_out` must be NULL or writable
/// for one pointer; a returned error is freed with `dr_string_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_adjust_image(
    doc: *const DrDocument,
    img: *const DrImage,
    op: *const c_char,
    params_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    if doc.is_null() || img.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "panic while applying a calibrated adjustment",
            ptr::null_mut(),
            || {
                let adjustment = parse_adjustment(op, params_json)?;
                (&*doc).adjust_image(&*img, adjustment)
            },
            boxed,
        )
    }
}

/// `op` as `calibrated_refusal` reads it: the string, or `"white_balance"`
/// (the stricter op) for NULL or non-UTF-8.
///
/// # Safety
/// `op` must be NULL or a valid NUL-terminated C string.
unsafe fn refusal_op(op: *const c_char) -> String {
    unsafe { read_cstr(op, "op") }.unwrap_or_else(|_| "white_balance".to_owned())
}

/// Why a NEW or EDITED `document_v2` `op` ("exposure" | "white_balance";
/// NULL or any other string answers as "white_balance") — layer create,
/// layer edit, `dr_doc_adjust_image`, editable paste — cannot evaluate in the
/// document's profile: an owned sentence (free with `dr_string_free`) when the
/// profile has no matrix/TRC model (both ops), or when its tone curves fail
/// the precision check (the own-profile sentence naming it) — for Exposure
/// only where the curves are repaired or unusable, since Exposure needs one
/// exponent per channel and no encoder (#2); NULL when admitted and on a
/// NULL doc. Existing layers are not judged: an 8-bit document already
/// holding one keeps rendering.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `op` NULL or
/// a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_calibrated_refusal(
    doc: *const DrDocument,
    op: *const c_char,
) -> *mut c_char {
    if doc.is_null() {
        return ptr::null_mut();
    }
    let op = unsafe { refusal_op(op) };
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            calibrated_refusal(&d.profile, &op)
                .and_then(|reason| CString::new(reason).ok())
                .map(CString::into_raw)
        })
    }
}

/// The typed twin of `dr_doc_calibrated_refusal`: the same sentences for the
/// same profile and `op`. Free a non-NULL result with `dr_string_free`.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `op` NULL or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_calibrated_refusal(
    document: *const PrecisionDocument,
    op: *const c_char,
) -> *mut c_char {
    if document.is_null() {
        return ptr::null_mut();
    }
    let op = unsafe { refusal_op(op) };
    unsafe {
        handle_get(document, ptr::null_mut(), |d| {
            calibrated_refusal(d.profile(), &op)
                .and_then(|reason| CString::new(reason).ok())
                .map_or(ptr::null_mut(), CString::into_raw)
        })
    }
}
