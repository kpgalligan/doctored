//! C FFI for Edit ▸ Auto-Align Layers' Cylindrical and Spherical
//! reprojection: one layer's `project_layer` plan and edit
//! at both depths (`dr_doc_project_layer(_plan)`,
//! `dr_precision_doc_project_layer(_plan)`).
//!
//! Thin shims over the shared `ffi_util` helpers, in `ffi_perspective_warp`'s
//! `warp_ffi!` shape; nothing unwinds across the boundary.
use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_align_projection::ProjectLayerRequest;
use crate::doc_puppet::REQUEST_LIMIT;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_doc_op, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

fn json_string(value: serde_json::Value) -> Result<*mut c_char, String> {
    CString::new(value.to_string())
        .map(CString::into_raw)
        .map_err(|e| e.to_string())
}

macro_rules! project_ffi {
    ($doc:ty, $plan:ident, $edit:ident, $op:path) => {
        /// The plan of a `project_layer` request:
        /// `{"retained_bytes","scratch_bytes","rect":[x,y,w,h],"vertices":0}`,
        /// `rect` the projected layer's. NULL document returns NULL with no
        /// error; a refusal returns NULL with its sentence.
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
                    "planning a layer projection",
                    ptr::null_mut(),
                    || {
                        let request = ProjectLayerRequest::parse(borrow_cstr_bounded(
                            request,
                            "request",
                            REQUEST_LIMIT,
                        )?)?;
                        json_string(
                            required_handle(document)?
                                .project_layer_plan(&request, limit, other)?,
                        )
                    },
                    |s| s,
                )
            }
        }

        /// The edit of a `project_layer` request: a new document with the
        /// layer reprojected (never NULL without an error for a live
        /// document). NULL document returns NULL with no error; a refusal
        /// returns NULL with its sentence.
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
                $op(
                    document,
                    err_out,
                    "editing a layer projection",
                    |doc: &$doc| {
                        let request = ProjectLayerRequest::parse(borrow_cstr_bounded(
                            request,
                            "request",
                            REQUEST_LIMIT,
                        )?)?;
                        doc.project_layer(&request, limit, other, control.as_ref())
                    },
                )
            }
        }
    };
}
project_ffi!(
    DrDocument,
    dr_doc_project_layer_plan,
    dr_doc_project_layer,
    fallible_doc_op
);
project_ffi!(
    PrecisionDocument,
    dr_precision_doc_project_layer_plan,
    dr_precision_doc_project_layer,
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
