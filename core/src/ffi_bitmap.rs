//! The Bitmap document across the FFI (#1 Bitmap phase 1a): Image ▸ Mode
//! ▸ Bitmap and its plan, and Grayscale back with a Size Ratio and its plan.
//! Phase 1c adds the 1-bit files: which PNG, TIFF and BMP files open as a
//! Bitmap document, their open, and the open of a Bitmap PSD or PSB's
//! stored composite (the export arms ride on the typed export and PSD
//! exports).
//!
//! The conventions are the surface's: NULL tolerance first, `catch_unwind`
//! through the shared `ffi_util` helpers, `Err` through `err_out`.
//! `bitmap_document`, `bitmap_convert`, `bitmap_io` and `bitmap_psd` own
//! every decision this file exports.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, read_cstr, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

/// The largest options JSON this surface accepts (a parse bound, not a
/// memory plan): 2 MiB, because Custom Pattern's inline tile rides in it and
/// a 512 × 512 tile's base64 is 1,398,104 bytes.
const OPTIONS_MAX: usize = 2 << 20;

/// An optional bounded JSON argument: NULL is the empty object.
unsafe fn options<'a>(text: *const c_char) -> Result<&'a str, String> {
    if text.is_null() {
        return Ok("");
    }
    unsafe { borrow_cstr_bounded(text, "options", OPTIONS_MAX) }
}

/// Reports `value` as an owned JSON string.
fn json_string(value: serde_json::Value, what: &str) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| format!("cannot report {what} JSON: {e}"))
}

/// Image ▸ Mode ▸ Bitmap's plan: owned JSON
/// `{"working_memory_plan_bytes","refusal","flatten_required","flattens",
/// "width","height"}`. The options are
/// `{"method"?,"resolution"?,"flatten"?,"frequency"?,"angle"?,"shape"?,
/// "pattern"?}` (`bitmap_convert`); a malformed one is `Err`. Free it with
/// `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `options_json` NULL or NUL-terminated; `err_out`
/// NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_to_bitmap_plan(
    document: *const PrecisionDocument,
    options_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Bitmap",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                json_string(
                    document.convert_to_bitmap_plan(options(options_json)?)?,
                    "Bitmap plan",
                )
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ Bitmap: a new Bitmap document from an 8-bit Grayscale
/// document (`bitmap_convert`).
///
/// # Safety
/// As [`dr_precision_doc_convert_to_bitmap_plan`]; `control` NULL or a live
/// single-use token.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_to_bitmap(
    document: *const PrecisionDocument,
    options_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting to Bitmap",
            ptr::null_mut(),
            || {
                required_handle(document)?.convert_to_bitmap(
                    options(options_json)?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Image ▸ Mode ▸ Grayscale on a Bitmap document, planned: owned JSON
/// `{"working_memory_plan_bytes","refusal","width","height"}`. Free it with
/// `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `options_json` NULL or NUL-terminated; `err_out`
/// NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_bitmap_to_gray_plan(
    document: *const PrecisionDocument,
    options_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Bitmap to Grayscale",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                json_string(
                    document.convert_bitmap_to_gray_plan(options(options_json)?)?,
                    "Grayscale plan",
                )
            },
            CString::into_raw,
        )
    }
}

/// The 1-bit source kind of the file at `path`: 0 none, 1 PNG, 2 TIFF, 3
/// BMP (0 for NULL, an unreadable file, or not a 1-bit file
/// `dr_precision_doc_open_bitmap` accepts). Header work only (`bitmap_io::bitmap_source_at`).
///
/// # Safety
/// `path` must be NULL or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn dr_path_bitmap_source(path: *const c_char) -> i32 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "inspecting a 1-bit file",
            0,
            || {
                let path = read_cstr(path, "path")?;
                Ok(crate::bitmap_io::bitmap_source_at(&path)?.map_or(0, |source| source as i32))
            },
            |value| value,
        )
    }
}

/// A 1-bit PNG, TIFF or BMP as a Bitmap document; `working_gray` is the
/// working Gray's wire name, the profile of a file with none.
///
/// # Safety
/// `path` and `working_gray` NULL or NUL-terminated; `control` NULL or a
/// live single-use token; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_open_bitmap(
    path: *const c_char,
    working_gray: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening a 1-bit file",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let working = read_cstr(working_gray, "working_gray")?;
                PrecisionDocument::open_bitmap(
                    &path,
                    &working,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// A Bitmap (colour mode 0) PSD or PSB's stored composite as a Bitmap
/// document (`bitmap_psd`; the file holds no layers). The file must still
/// be the one `dr_psd_import_report` fingerprinted; `working_gray` is the
/// profile of a file with no Gray profile.
///
/// # Safety
/// `path`, `expected_fingerprint` and `working_gray` NULL or
/// NUL-terminated; `control` NULL or a live single-use token; `err_out`
/// NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_open_bitmap_psd(
    path: *const c_char,
    expected_fingerprint: *const c_char,
    working_gray: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening Bitmap PSD",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let working = read_cstr(working_gray, "working_gray")?;
                let bytes =
                    std::fs::read(&path).map_err(|e| format!("failed to read {path}: {e}"))?;
                let expected = read_cstr(expected_fingerprint, "expected_fingerprint")?;
                if expected != crate::psd_import::fingerprint(&bytes) {
                    return Err(format!(
                        "PSD {path}: file changed since compatibility preflight; inspect it again"
                    ));
                }
                PrecisionDocument::open_bitmap_psd(
                    &bytes,
                    &path,
                    &working,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Image ▸ Mode ▸ Grayscale on a Bitmap document: the 8-bit Gray document,
/// each Size Ratio block averaged (`bitmap_convert`).
///
/// # Safety
/// As [`dr_precision_doc_convert_bitmap_to_gray_plan`]; `control` NULL or a
/// live single-use token.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_bitmap_to_gray(
    document: *const PrecisionDocument,
    options_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting Bitmap to Grayscale",
            ptr::null_mut(),
            || {
                required_handle(document)?.convert_bitmap_to_gray(
                    options(options_json)?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}
