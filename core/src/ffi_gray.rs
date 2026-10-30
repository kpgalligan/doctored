//! The Grayscale document across the FFI (#547 phase 1a): which files open
//! as one, the open, File ▸ New ▸ Grayscale, Image ▸ Mode's two
//! conversions and their plans, and (phase 5a) the profile and depth edit
//! and its plan.
//!
//! The conventions are the surface's: NULL tolerance first, `catch_unwind`
//! through the shared `ffi_util` helpers, `Err` through `err_out`.
//! `gray_document`, `gray_io`, `gray_convert` and `gray_profile` own every
//! decision this file exports.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::ffi_color::blob;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, read_cstr, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

/// The largest options or request JSON this surface accepts.
const OPTIONS_MAX: usize = 4096;

fn string(value: serde_json::Value) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| format!("cannot report Gray JSON: {e}"))
}

/// An optional bounded JSON argument: NULL is the empty object.
unsafe fn options<'a>(text: *const c_char, what: &str) -> Result<&'a str, String> {
    if text.is_null() {
        return Ok("");
    }
    unsafe { borrow_cstr_bounded(text, what, OPTIONS_MAX) }
}

/// `DR_GRAY_SOURCE_*` for the file at `path`: PNG 1, TIFF 2, JPEG 3, else 0
/// (NULL, unreadable, or not a gray file `dr_precision_doc_open_gray`
/// accepts). Header work only (`gray_io::gray_source_at`).
///
/// # Safety
/// `path` must be NULL or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn dr_path_gray_source(path: *const c_char) -> i32 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "inspecting a gray file",
            0,
            || {
                let path = read_cstr(path, "path")?;
                Ok(crate::gray_io::gray_source_at(&path)?.map_or(0, |source| source as i32))
            },
            |value| value,
        )
    }
}

/// A gray PNG, TIFF or JPEG as a Grayscale document; `working_gray` is the
/// working Gray's wire name, assigned to an untagged file.
///
/// # Safety
/// `path` and `working_gray` NULL or NUL-terminated; `control` NULL or a live
/// single-use token; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_open_gray(
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
            "opening a gray file",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let working = read_cstr(working_gray, "working_gray")?;
                PrecisionDocument::open_gray(
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

/// File ▸ New ▸ Grayscale: `icc` (NULL for the request's `space`) and the
/// request `{"depth","space"?,"fill_srgb","layer_name","ppi"}`.
///
/// # Safety
/// `icc` NULL or valid for `icc_len` bytes; `request` NULL or
/// NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_new_gray(
    width: u32,
    height: u32,
    icc: *const u8,
    icc_len: usize,
    request: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "making a Grayscale document",
            ptr::null_mut(),
            || {
                let request = read_cstr(request, "request")?;
                PrecisionDocument::new_gray(
                    width,
                    height,
                    blob(icc, icc_len),
                    &request,
                    limit,
                    other_retained_bytes,
                )
            },
            boxed,
        )
    }
}

/// Image ▸ Mode ▸ Grayscale's plan: `{"working_memory_plan_bytes",
/// "refusal","flatten_required","flattens"}`. `icc` NULL takes the options'
/// `space`. An 8- or 16-bit Lab Color document converts too (`lab_gray`,
/// #547 Lab phase 5a-ii).
///
/// # Safety
/// `document` NULL or live; `icc` NULL or valid for `icc_len` bytes;
/// `options` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_to_gray_plan(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    options_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Grayscale",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                string(
                    document.convert_to_gray_plan(
                        blob(icc, icc_len),
                        options(options_json, "options")?,
                    )?,
                )
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ Grayscale: a new Grayscale document (`gray_convert`), from
/// an RGB or an 8- or 16-bit Lab Color document (`lab_gray`, #547 Lab phase
/// 5a-ii).
///
/// # Safety
/// As [`dr_precision_doc_convert_to_gray_plan`]; `control` NULL or a live
/// single-use token.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_convert_to_gray(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    options_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting to Grayscale",
            ptr::null_mut(),
            || {
                required_handle(document)?.convert_to_gray(
                    blob(icc, icc_len),
                    options(options_json, "options")?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Image ▸ Mode ▸ RGB Color's plan on a Grayscale document; `icc` is the
/// destination RGB profile (required).
///
/// # Safety
/// As [`dr_precision_doc_convert_to_gray_plan`].
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_gray_to_rgb_plan(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    options_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning RGB Color",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let icc = blob(icc, icc_len).ok_or("RGB Color needs an RGB profile")?;
                string(document.convert_gray_to_rgb_plan(icc, options(options_json, "options")?)?)
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ RGB Color on a Grayscale document: Convert to Profile from
/// its twin into `icc`, the Gray policy dropped.
///
/// # Safety
/// As [`dr_precision_doc_convert_to_gray`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_convert_gray_to_rgb(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    options_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting to RGB",
            ptr::null_mut(),
            || {
                let icc = blob(icc, icc_len).ok_or("RGB Color needs an RGB profile")?;
                required_handle(document)?.convert_gray_to_rgb(
                    icc,
                    options(options_json, "options")?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// A Grayscale document's Gray ICC bytes, verbatim, borrowed until the
/// document is freed (#547 phase 4: the host's clipboard compares and
/// converts with them); NULL with length 0 for a NULL handle or a document
/// that is not Grayscale.
///
/// # Safety
/// `document` NULL or live; `length` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gray_profile(
    document: *const PrecisionDocument,
    length: *mut usize,
) -> *const u8 {
    unsafe {
        let (pointer, count) = match document.as_ref().and_then(PrecisionDocument::gray_policy) {
            Some(policy) => (policy.bytes().as_ptr(), policy.bytes().len()),
            None => (ptr::null(), 0),
        };
        if !length.is_null() {
            *length = count;
        }
        pointer
    }
}

/// Assign Profile, Convert to Profile and 8/16/32 Bits/Channel's plan on a
/// Grayscale document (`gray_profile`): owned JSON
/// `{"working_memory_plan_bytes","refusal","changes"}`. `icc` NULL takes the
/// request's `space`.
///
/// # Safety
/// `document` NULL or live; `icc` NULL or valid for `icc_len` bytes;
/// `request` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gray_profile_plan(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a Gray profile edit",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                string(
                    document.gray_profile_plan(blob(icc, icc_len), options(request, "request")?)?,
                )
            },
            CString::into_raw,
        )
    }
}

/// Assign, Convert or Depth on a Grayscale document (`gray_profile`): a new
/// Grayscale document; NULL with no error when the edit changes nothing.
///
/// # Safety
/// As [`dr_precision_doc_gray_profile_plan`]; `control` NULL or a live
/// single-use token.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_gray_profile_edit(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    request: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "editing a Gray profile",
            ptr::null_mut(),
            || {
                required_handle(document)?.gray_profile_edit(
                    blob(icc, icc_len),
                    options(request, "request")?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// #547 Gray phase 6a-i: a type, shape or fill layer's colours as their gray
/// (Mode ▸ Grayscale's rule, `gray_artwork`). `request` is
/// `{"srgb":[[r,g,b],…],"linear":[[r,g,b],…],"pattern":{…}}`, each key
/// optional, each list at most 1024, srgb 0…1, linear finite. Answers the
/// same keys: a triple with r = g = b verbatim, else `[v,v,v]` (srgb: the
/// sRGB value of its luminance; linear: its luminance `Y`); a preset pattern
/// verbatim, a custom tile's non-neutral texels greyed through the twin. A
/// non-Gray document refuses. Free the answer with `dr_string_free`.
///
/// # Safety
/// `document` NULL or a live handle; `request` NULL or NUL-terminated;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gray_artwork_colors(
    document: *const PrecisionDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "greying Grayscale artwork colours",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(
                    request,
                    "request",
                    crate::artwork_document::MAX_LARGE_DESCRIPTION,
                )?;
                let answer = crate::gray_artwork::gray_artwork_colors(document, request)?;
                CString::new(answer)
                    .map_err(|e| format!("cannot report Grayscale artwork colours: {e}"))
            },
            CString::into_raw,
        )
    }
}
