//! Platform capture metadata bridge; all parsing lives in the shared scanner.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_metadata_import::MAX_METADATA_CARRIER;
use crate::ffi_util::{boxed, fallible_op};

/// Replaces capture packets from a bounded JPEG/PNG/TIFF carrier, preserving
/// the document's pixels, ICC and resolution. NULL for a NULL input, unchanged
/// packets or an error; only errors set err_out. At most 49 MiB of carrier data.
/// warnings_out receives a JSON array of detected omissions, including on
/// a no-op; free with dr_string_free. NULL warnings_out discards this report.
///
/// # Safety
/// doc must be NULL or a live document; bytes must be NULL or readable for len
/// bytes. warnings_out and err_out must be NULL or writable CString pointers, freed by
/// dr_string_free. The input handles/buffers remain owned by the caller.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_import_metadata(
    doc: *const DrDocument,
    bytes: *const u8,
    len: usize,
    warnings_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    if !warnings_out.is_null() {
        unsafe {
            *warnings_out = ptr::null_mut();
        }
    }
    if doc.is_null() || bytes.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "importing platform metadata",
            ptr::null_mut(),
            || {
                if len > MAX_METADATA_CARRIER {
                    return Err("metadata carrier exceeds the bounded packet import limit".into());
                }
                let bytes = std::slice::from_raw_parts(bytes, len);
                let (document, warnings) = (&*doc).import_metadata(bytes)?;
                let warnings = CString::new(serde_json::json!(warnings).to_string())
                    .map_err(|e| format!("cannot report metadata import omissions: {e}"))?;
                Ok((document, warnings))
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
