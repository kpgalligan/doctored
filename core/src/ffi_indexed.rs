//! The Indexed Color document across the FFI (#547 Indexed phase 1a):
//! which files open as one, the open, Image ▸ Mode ▸ Indexed Color and its
//! plan, RGB Color back, and the index under a point; since phase 1c, Image ▸
//! Mode ▸ Color Table (the table read, planned and replaced) and the `.act`
//! file read and written; since phase 2d, Paste and Paste Special planned
//! and applied (`indexed_clipboard`); and the open of an Indexed Color PSD
//! or PSB's stored composite (`indexed_psd`; the export arms ride on the PSD
//! exports).
//!
//! The conventions are the surface's: NULL tolerance first, `catch_unwind`
//! through the shared `ffi_util` helpers, `Err` through `err_out`.
//! `indexed_document`, `indexed_io`, `indexed_convert`, `indexed_table` and
//! `indexed_clipboard` own every decision this file exports.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::ffi_clipboard::paste_request;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, read_cstr, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

/// The largest options or table JSON this surface accepts: a 256-entry
/// table is at most about 3.6 KB of JSON (a parse bound, not a memory plan).
const OPTIONS_MAX: usize = 16384;

/// An optional bounded JSON argument: NULL is the empty object.
unsafe fn options<'a>(text: *const c_char) -> Result<&'a str, String> {
    if text.is_null() {
        return Ok("");
    }
    unsafe { borrow_cstr_bounded(text, "options", OPTIONS_MAX) }
}

/// `DR_INDEXED_SOURCE_*` for the file at `path`: PNG 1, GIF 2, TIFF 3, else 0 (NULL,
/// unreadable, or not an indexed file `dr_precision_doc_open_indexed`
/// accepts). Header work only (`indexed_io::indexed_source_at`).
///
/// # Safety
/// `path` must be NULL or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn dr_path_indexed_source(path: *const c_char) -> i32 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "inspecting an indexed file",
            0,
            || {
                let path = read_cstr(path, "path")?;
                Ok(crate::indexed_io::indexed_source_at(&path)?.map_or(0, |source| source as i32))
            },
            |value| value,
        )
    }
}

/// An indexed PNG, GIF or palette TIFF as an Indexed Color document, its palette,
/// transparent index and indices kept exactly.
///
/// # Safety
/// `path` NULL or NUL-terminated; `control` NULL or a live single-use
/// token; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_open_indexed(
    path: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening an indexed file",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                PrecisionDocument::open_indexed(
                    &path,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// An Indexed Color (colour mode 2) PSD or PSB's stored composite as an
/// Indexed Color document, its palette, transparent index and every index
/// kept (`indexed_psd`). The file must still be the one
/// `dr_psd_import_report` fingerprinted.
///
/// # Safety
/// `path` and `expected_fingerprint` NULL or NUL-terminated; `control` NULL
/// or a live single-use token; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_open_indexed_psd(
    path: *const c_char,
    expected_fingerprint: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening Indexed Color PSD",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let bytes =
                    std::fs::read(&path).map_err(|e| format!("failed to read {path}: {e}"))?;
                let expected = read_cstr(expected_fingerprint, "expected_fingerprint")?;
                if expected != crate::psd_import::fingerprint(&bytes) {
                    return Err(format!(
                        "PSD {path}: file changed since compatibility preflight; inspect it again"
                    ));
                }
                PrecisionDocument::open_indexed_psd(
                    &bytes,
                    &path,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Image ▸ Mode ▸ Indexed Color's plan: owned JSON
/// `{"working_memory_plan_bytes","refusal","flatten_required","flattens"}`.
/// Free it with `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `options` NULL or NUL-terminated; `err_out` NULL
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_to_indexed_plan(
    document: *const PrecisionDocument,
    options_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Indexed Color",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let plan = document.convert_to_indexed_plan(options(options_json)?)?;
                CString::new(plan.to_string())
                    .map_err(|e| format!("cannot report Indexed Color JSON: {e}"))
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ Indexed Color: a new Indexed Color document from an 8-bit
/// RGB or Grayscale document (`indexed_convert`).
///
/// # Safety
/// As [`dr_precision_doc_convert_to_indexed_plan`]; `control` NULL or a live
/// single-use token.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_to_indexed(
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
            "converting to Indexed Color",
            ptr::null_mut(),
            || {
                required_handle(document)?.convert_to_indexed(
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

/// Image ▸ Mode ▸ RGB Color on an Indexed Color document: the RGB document
/// its storage is, every sample shared.
///
/// # Safety
/// `document` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_indexed_to_rgb(
    document: *const PrecisionDocument,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting Indexed Color to RGB",
            ptr::null_mut(),
            || required_handle(document)?.convert_indexed_to_rgb(),
            boxed,
        )
    }
}

/// The palette index at `(x, y)` of an Indexed Color document; -1 for a NULL
/// handle, a point off the canvas, or any other document.
///
/// # Safety
/// `document` NULL or live.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_indexed_index_at(
    document: *const PrecisionDocument,
    x: u32,
    y: u32,
) -> i32 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "reading an Indexed Color index",
            -1,
            || {
                Ok(document
                    .as_ref()
                    .and_then(|d| d.indexed_index_at(x, y))
                    .map_or(-1, i32::from))
            },
            |value| value,
        )
    }
}

/// Reports `value` as an owned JSON string.
fn json_string(value: serde_json::Value, what: &str) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| format!("cannot report {what} JSON: {e}"))
}

/// Image ▸ Mode ▸ Color Table's table: owned JSON
/// `{"colors":[[r,g,b],…],"transparent_index":n|null}`; a non-Indexed
/// document refuses. Free it with `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_indexed_color_table(
    document: *const PrecisionDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading an Indexed Color table",
            ptr::null_mut(),
            || {
                json_string(
                    required_handle(document)?.indexed_color_table()?,
                    "colour table",
                )
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ Color Table's plan for `table` (the JSON
/// `dr_precision_doc_indexed_color_table` answers): owned JSON
/// `{"working_memory_plan_bytes","refusal","changes"}`. Free it with
/// `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `table` NULL or NUL-terminated; `err_out` NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_color_table_plan(
    document: *const PrecisionDocument,
    table: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning an Indexed Color table",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let table = borrow_cstr_bounded(table, "table", OPTIONS_MAX)?;
                json_string(document.set_color_table_plan(table)?, "colour table plan")
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ Color Table: a new Indexed document whose palette and
/// transparent index are `table`'s, every index kept. NULL with no error when
/// `table` equals the document's.
///
/// # Safety
/// As [`dr_precision_doc_set_color_table_plan`]; `control` NULL or a live
/// single-use token.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_color_table(
    document: *const PrecisionDocument,
    table: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "replacing an Indexed Color table",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let table = borrow_cstr_bounded(table, "table", OPTIONS_MAX)?;
                document.set_color_table(table, limit, other_retained_bytes, control.as_ref())
            },
            |next| next.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// The table in the `.act` file at `path` (768 or 772 bytes), as
/// `dr_precision_doc_indexed_color_table` answers it. Free it with
/// `dr_string_free`.
///
/// # Safety
/// `path` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_indexed_read_act(
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading a colour table file",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                json_string(crate::indexed_table::read_act(&path)?, "colour table")
            },
            CString::into_raw,
        )
    }
}

/// Paste and Paste Special on an Indexed Color document, planned
/// (`indexed_clipboard::paste_plan`, #547 Indexed phase 2d): owned JSON
/// `{"retained_bytes","scratch_bytes"}`. Free it with `dr_string_free`.
///
/// # Safety
/// `document` and `payload` NULL or live; `request` NULL or NUL-terminated;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_indexed_paste_plan(
    document: *const PrecisionDocument,
    payload: *const crate::ffi_clipboard::DrClipboardPayload,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning an Indexed Color paste",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let payload = required_handle(payload)?;
                let request = paste_request(request)?;
                json_string(
                    crate::indexed_clipboard::paste_plan(document, &payload.payload, &request)?,
                    "paste plan",
                )
            },
            CString::into_raw,
        )
    }
}

/// Paste and Paste Special on an Indexed Color document
/// (`indexed_clipboard::paste`): the payload pasted on the RGB storage
/// above the Index layer, masked by `coverage` when it is not NULL (the
/// canvas's u16 count, applied by `set_mask` at `outside_coverage`: Paste
/// Into and Paste Outside), merged down and re-indexed. A NULL `coverage`
/// with length 0 pastes unmasked.
///
/// # Safety
/// As [`dr_precision_doc_indexed_paste_plan`]; `coverage` NULL or readable
/// for `coverage_len` u16 values; `control` NULL or a live single-use token.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_indexed_paste(
    document: *const PrecisionDocument,
    payload: *const crate::ffi_clipboard::DrClipboardPayload,
    request: *const c_char,
    coverage: *const u16,
    coverage_len: usize,
    outside_coverage: u8,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "pasting into an Indexed Color document",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let payload = required_handle(payload)?;
                let request = paste_request(request)?;
                let coverage =
                    crate::ffi_clipboard::canvas_coverage(document, coverage, coverage_len)?;
                crate::indexed_clipboard::paste(
                    document,
                    &payload.payload,
                    &request,
                    coverage.map(|c| (c, outside_coverage)),
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Writes `table` (the colour table JSON) to `path` as a 772-byte `.act`;
/// false with the error on failure.
///
/// # Safety
/// `path` and `table` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_indexed_write_act(
    path: *const c_char,
    table: *const c_char,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "writing a colour table file",
            false,
            || {
                let path = read_cstr(path, "path")?;
                let table = borrow_cstr_bounded(table, "table", OPTIONS_MAX)?;
                crate::indexed_table::write_act(&path, table)
            },
            |()| true,
        )
    }
}
