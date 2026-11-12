//! Save-preservation query shared by UI and automation; never renders pixels.

use std::ffi::{c_char, c_int, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::fallible_op;
use crate::precision_document::PrecisionDocument;
use crate::Format;

/// JSON array of state a flat save loses. Empty means no detected state
/// loss, not a claim of lossless JPEG/GIF encoding. Free with dr_string_free.
/// NULL on invalid input/panic, with err_out explaining the failure.
///
/// # Safety
/// doc must be NULL or a live document; err_out must be NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_flat_save_losses(
    doc: *const DrDocument,
    format: c_int,
    embed_profile: bool,
    strip_metadata: bool,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "checking save preservation",
            ptr::null_mut(),
            || {
                let doc = doc.as_ref().ok_or("document is NULL")?;
                let format = Format::from_c(format).ok_or("unknown output format")?;
                let json = serde_json::to_string(&doc.flat_save_losses(
                    format,
                    embed_profile,
                    strip_metadata,
                ))
                .map_err(|e| format!("cannot report save preservation: {e}"))?;
                CString::new(json).map_err(|e| format!("cannot report save preservation: {e}"))
            },
            CString::into_raw,
        )
    }
}

/// JSON array of the editable state a flattened file of a typed document
/// has no place for (`precision_preservation`): `alpha_kept` says whether
/// the file carries an alpha sample, `tiff` whether it is a flat TIFF (which
/// keeps the paths of every mode but Duotone, `tiff_paths`). `[]` when the file
/// reopens as this document. Free with dr_string_free. NULL on a NULL
/// document or a panic, with err_out explaining the failure.
///
/// # Safety
/// doc must be NULL or a live typed document; err_out must be NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_flat_state_losses(
    doc: *const PrecisionDocument,
    alpha_kept: bool,
    tiff: bool,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "checking save preservation",
            ptr::null_mut(),
            || {
                let doc = doc.as_ref().ok_or("document is NULL")?;
                let json = serde_json::to_string(&doc.flat_state_losses(alpha_kept, tiff))
                    .map_err(|e| format!("cannot report save preservation: {e}"))?;
                CString::new(json).map_err(|e| format!("cannot report save preservation: {e}"))
            },
            CString::into_raw,
        )
    }
}
