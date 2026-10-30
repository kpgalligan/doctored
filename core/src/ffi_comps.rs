//! LAYER COMP exports — thin shims over `doc_comps` / `precision_comps`.
//! Every rule, refusal and no-op lives there; this file only crosses the
//! boundary. The typed MUTATION has no export of its own: it goes through
//! `dr_precision_doc_edit` with the same `{"op":"layer_comp", …}` request the
//! byte side takes, which is the shape every other typed layer property uses.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_comps::CompRequest;
use crate::ffi_util::{borrow_cstr_bounded, fallible_op, required_handle};
use crate::precision_document::PrecisionDocument;

/// The longest comp request this build accepts. A request carries a name and
/// a comment, never a table, so 64 KiB is far beyond what one can need and
/// bounds the copy before it is parsed.
const REQUEST_LIMIT: usize = 64 * 1024;

/// Every layer comp of `doc` in the REPORT form, as JSON; free with
/// `dr_string_free`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_comps_json(
    doc: *const DrDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reporting layer comps",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let json = document.layer_comps_json()?;
                CString::new(json).map_err(|e| format!("could not report layer comps: {e}"))
            },
            CString::into_raw,
        )
    }
}

/// Runs one comp request against `doc`. NULL WITH NO ERROR is the exact
/// no-op (`doc_comps`'s purity rule); NULL with an error is a refusal whose
/// sentence the host shows verbatim.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `request`
/// must be NULL or a NUL-terminated UTF-8 string; `err_out` must be NULL or a
/// valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_edit_layer_comps(
    doc: *const DrDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "editing layer comps",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let text = borrow_cstr_bounded(request, "request", REQUEST_LIMIT)?;
                let parsed = CompRequest::parse(text)?;
                document.edit_layer_comps(&parsed)
            },
            |result| result.map_or(ptr::null_mut(), |d| Box::into_raw(Box::new(d))),
        )
    }
}

/// [`dr_doc_layer_comps_json`]'s twin on a typed document.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `err_out` must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_layer_comps_json(
    doc: *const PrecisionDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reporting high-depth layer comps",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let json = document.layer_comps_json()?;
                CString::new(json).map_err(|e| format!("could not report layer comps: {e}"))
            },
            CString::into_raw,
        )
    }
}
