//! C FFI for File Info: the getter and the setter at both depths,
//! over `doc_file_info`. The value crosses as the JSON `FileInfo::parse`
//! reads and `FileInfo::to_json` writes.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{boxed, doc_get, fallible_doc_op, fallible_op, handle_get, read_cstr};
use crate::file_info::FileInfo;
use crate::precision_document::PrecisionDocument;

/// The JSON of a file info value as an owned C string.
fn json_string(info: &FileInfo) -> Option<*mut c_char> {
    CString::new(info.to_json()).ok().map(CString::into_raw)
}

/// The document's file info as JSON with all four keys:
/// `{"creator":[string],"copyright":string,"description":string,
/// "keywords":[string]}`. Per field the XMP value, else the IIM dataset,
/// else the EXIF tag. NULL on a NULL doc. Free with dr_string_free.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_file_info(doc: *const DrDocument) -> *mut c_char {
    unsafe { doc_get(doc, ptr::null_mut(), |d| json_string(&d.file_info())) }
}

/// Pure setter for the document's file info from that same JSON, which must
/// hold exactly the four keys. NULL on a NULL doc or when the metadata would
/// not change, with err_out untouched; NULL with err_out set for NULL or
/// malformed JSON, a value past the limits (a list entry 256 characters, 256
/// entries per list, copyright 2000, description 8000; control characters
/// other than a line feed in the description) or a packet that cannot take
/// the edit. Pixels never change.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `json` must
/// be NULL or a NUL-terminated string; `err_out` must be NULL or a writable
/// CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_file_info(
    doc: *const DrDocument,
    json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "setting file info", |d| {
            d.set_file_info(&FileInfo::parse(&read_cstr(json, "file info json")?)?)
        })
    }
}

/// The typed twin of dr_doc_file_info: the same JSON. NULL on a NULL
/// document. Free with dr_string_free.
///
/// # Safety
/// `document` must be NULL or a live typed document.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_file_info(
    document: *const PrecisionDocument,
) -> *mut c_char {
    unsafe {
        handle_get(document, ptr::null_mut(), |d| {
            json_string(&d.file_info()).unwrap_or(ptr::null_mut())
        })
    }
}

/// The typed twin of dr_doc_set_file_info, the same contract: NULL on a NULL
/// document or no change, with err_out untouched; NULL with err_out set for
/// NULL or malformed JSON, a value past the limits or a packet that cannot
/// take the edit.
///
/// # Safety
/// `document` must be NULL or a live typed document; `json` must be NULL or
/// a NUL-terminated string; `err_out` must be NULL or a writable CString
/// pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_file_info(
    document: *const PrecisionDocument,
    json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    if document.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "setting high-depth file info",
            ptr::null_mut(),
            || (*document).set_file_info(&FileInfo::parse(&read_cstr(json, "file info json")?)?),
            |r| r.map_or(ptr::null_mut(), boxed),
        )
    }
}
