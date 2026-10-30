//! The typed metadata bridge at the C boundary (Phase 11D): the three shims
//! over `precision_metadata_edit`, each the byte twin's contract verbatim.

use std::ffi::{c_char, c_int, CString};
use std::ptr;

use crate::doc_color::MetadataKind;
use crate::doc_metadata_import::MAX_METADATA_CARRIER;
use crate::ffi_util::{boxed, fallible_op};
use crate::precision_document::PrecisionDocument;

/// Replaces capture packets from a bounded JPEG/PNG/TIFF carrier, preserving
/// the typed document's pixels, ICC and resolution. NULL for a NULL input,
/// unchanged packets or an error; only errors set err_out. At most 49 MiB of
/// carrier data. warnings_out receives a JSON array of detected omissions,
/// including on a no-op; free with dr_string_free. NULL warnings_out discards
/// this report.
///
/// # Safety
/// `document` must be NULL or a live typed document; `bytes` must be NULL or
/// readable for `len` bytes; `warnings_out` and `err_out` must be NULL or
/// writable CString pointers, freed with dr_string_free. Every input handle
/// and buffer remains owned by the caller.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_import_metadata(
    document: *const PrecisionDocument,
    bytes: *const u8,
    len: usize,
    warnings_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    if !warnings_out.is_null() {
        unsafe {
            *warnings_out = ptr::null_mut();
        }
    }
    if document.is_null() || bytes.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "importing platform metadata into a high-depth document",
            ptr::null_mut(),
            || {
                if len > MAX_METADATA_CARRIER {
                    return Err("metadata carrier exceeds the bounded packet import limit".into());
                }
                let bytes = std::slice::from_raw_parts(bytes, len);
                let (result, warnings) = (*document).import_metadata(bytes)?;
                let warnings = CString::new(serde_json::json!(warnings).to_string())
                    .map_err(|e| format!("cannot report metadata import omissions: {e}"))?;
                Ok((result, warnings))
            },
            |(result, warnings)| {
                if !warnings_out.is_null() {
                    *warnings_out = warnings.into_raw();
                }
                result.map_or(ptr::null_mut(), boxed)
            },
        )
    }
}

/// Stores one metadata packet verbatim, or CLEARS it when `bytes` is NULL —
/// a zero `len` with a non-NULL pointer stores an EMPTY packet, which the
/// format distinguishes from an absent one. NULL on a NULL document or a
/// value the document already carries, with err_out untouched; NULL with
/// err_out set for a value outside `DrMetadataKind` or a payload over 16 MiB.
///
/// # Safety
/// `document` must be NULL or a live typed document; `bytes` must be NULL or
/// readable for `len` bytes; `err_out` must be NULL or a writable CString
/// pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_metadata(
    document: *const PrecisionDocument,
    kind: c_int,
    bytes: *const u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    if document.is_null() {
        return ptr::null_mut();
    }
    let value = if bytes.is_null() {
        None
    } else {
        // `from_raw_parts` accepts a zero length for any non-NULL, aligned
        // pointer, and u8 is aligned everywhere.
        Some(unsafe { std::slice::from_raw_parts(bytes, len) })
    };
    unsafe {
        fallible_op(
            err_out,
            "setting high-depth metadata",
            ptr::null_mut(),
            || (*document).set_metadata(MetadataKind::from_c_checked(kind)?, value),
            |r| r.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Pure setter for the typed document's print resolution. NULL on a NULL
/// document or no change after sanitizing, with err_out untouched; NULL with
/// err_out set for a non-finite or non-positive component. PIXELS NEVER
/// CHANGE: only the print size does.
///
/// # Safety
/// `document` must be NULL or a live typed document; `err_out` must be NULL
/// or a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_resolution(
    document: *const PrecisionDocument,
    ppi_x: f32,
    ppi_y: f32,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    if document.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "setting high-depth print resolution",
            ptr::null_mut(),
            || (*document).set_resolution(ppi_x, ppi_y),
            |r| r.map_or(ptr::null_mut(), boxed),
        )
    }
}
