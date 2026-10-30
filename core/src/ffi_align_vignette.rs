//! C FFI for Edit ▸ Auto-Align Layers' Vignette Removal:
//! the `remove_vignette` plan and edit at both depths
//! (`dr_doc_remove_vignette(_plan)`, `dr_precision_doc_remove_vignette(_plan)`).
//!
//! Thin shims over the shared `ffi_util` helpers, in `ffi_align_projection`'s
//! `project_ffi!` shape; nothing unwinds across the boundary.
use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_align_vignette::RemoveVignetteRequest;
use crate::doc_puppet::REQUEST_LIMIT;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_doc_op, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

pub(crate) fn json_string(value: serde_json::Value) -> Result<*mut c_char, String> {
    CString::new(value.to_string())
        .map(CString::into_raw)
        .map_err(|e| e.to_string())
}

macro_rules! vignette_ffi {
    ($doc:ty, $plan:ident, $edit:ident, $op:path) => {
        /// The plan of a `remove_vignette` request:
        /// `{"retained_bytes","scratch_bytes","output_bytes","coefficient",
        /// "gain","samples","layers":[N…]}`; it runs the fit. NULL document
        /// returns NULL with no error; a refusal returns NULL with its
        /// sentence.
        ///
        /// # Safety
        /// `document` is NULL or live; `request` is NULL or NUL terminated;
        /// `err_out` is NULL or writable. Free the string with
        /// dr_string_free.
        #[no_mangle]
        pub unsafe extern "C" fn $plan(
            document: *const $doc,
            request: *const c_char,
            limit: u64,
            other: u64,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            if document.is_null() {
                return ptr::null_mut();
            }
            unsafe {
                fallible_op(
                    err_out,
                    "planning vignette removal",
                    ptr::null_mut(),
                    || {
                        let request = RemoveVignetteRequest::parse(borrow_cstr_bounded(
                            request,
                            "request",
                            REQUEST_LIMIT,
                        )?)?;
                        json_string(
                            required_handle(document)?
                                .remove_vignette_plan(&request, limit, other)?,
                        )
                    },
                    |s| s,
                )
            }
        }

        /// The edit of a `remove_vignette` request: a new document with the
        /// fitted vignette divided out of every listed layer. NULL with no
        /// error for a NULL document and when no sample changed; NULL with
        /// a message for a refusal.
        ///
        /// # Safety
        /// Handles are NULL or live for the call; `request` is NULL or NUL
        /// terminated; `err_out` is NULL or writable.
        #[no_mangle]
        pub unsafe extern "C" fn $edit(
            document: *const $doc,
            request: *const c_char,
            limit: u64,
            other: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut $doc {
            if document.is_null() {
                return ptr::null_mut();
            }
            unsafe {
                $op(document, err_out, "removing the vignette", |doc: &$doc| {
                    let request = RemoveVignetteRequest::parse(borrow_cstr_bounded(
                        request,
                        "request",
                        REQUEST_LIMIT,
                    )?)?;
                    doc.remove_vignette(&request, limit, other, control.as_ref())
                })
            }
        }
    };
}
vignette_ffi!(
    DrDocument,
    dr_doc_remove_vignette_plan,
    dr_doc_remove_vignette,
    fallible_doc_op
);
vignette_ffi!(
    PrecisionDocument,
    dr_precision_doc_remove_vignette_plan,
    dr_precision_doc_remove_vignette,
    precision_doc_op
);

/// [`fallible_doc_op`]'s typed twin for this file: NULL document yields
/// NULL, `Ok(None)` NULL with no message, `Err` its message. A copy of
/// `ffi_puppet`'s private `precision_doc_op` (the master), kept private as
/// that one is.
///
/// # Safety
/// `doc` is NULL or live; `err_out` is NULL or writable.
unsafe fn precision_doc_op(
    doc: *const PrecisionDocument,
    err_out: *mut *mut c_char,
    what: &str,
    op: impl FnOnce(&PrecisionDocument) -> Result<Option<PrecisionDocument>, String>,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            what,
            ptr::null_mut(),
            || op(required_handle(doc)?),
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}
