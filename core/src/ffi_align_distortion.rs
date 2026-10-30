//! C FFI for Edit ▸ Auto-Align Layers' Geometric Distortion: the
//! `fit_distortion` plan and fit at both depths
//! (`dr_doc_fit_distortion(_plan)`, `dr_precision_doc_fit_distortion(_plan)`).
//!
//! Thin shims over the shared `ffi_util` helpers; every export answers a JSON
//! string through `fallible_op` and `ffi_align_vignette::json_string`, so
//! nothing unwinds across the boundary.
use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_align_distortion::FitDistortionRequest;
use crate::doc_puppet::REQUEST_LIMIT;
use crate::ffi_align_vignette::json_string;
use crate::ffi_util::{borrow_cstr_bounded, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

macro_rules! distortion_ffi {
    ($doc:ty, $plan:ident, $fit:ident) => {
        /// The plan of a `fit_distortion` request:
        /// `{"retained_bytes","scratch_bytes"}`; it checks every listed layer
        /// and runs no matching. NULL document returns NULL with no error; a
        /// refusal returns NULL with its sentence.
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
                    "planning the distortion fit",
                    ptr::null_mut(),
                    || {
                        let request = FitDistortionRequest::parse(borrow_cstr_bounded(
                            request,
                            "request",
                            REQUEST_LIMIT,
                        )?)?;
                        json_string(
                            required_handle(document)?
                                .fit_distortion_plan(&request, limit, other)?,
                        )
                    },
                    |s| s,
                )
            }
        }

        /// The fit of a `fit_distortion` request: `{"coefficient","matches",
        /// "residual","layers":[N…],"maps":[[9]…]}`, each map a layer's
        /// corrected canvas point to the reference's. The document is not
        /// changed. NULL with no error for a NULL document; NULL with a
        /// message for a refusal or a cancel.
        ///
        /// # Safety
        /// Handles are NULL or live for the call; `request` is NULL or NUL
        /// terminated; `err_out` is NULL or writable. Free the string with
        /// dr_string_free.
        #[no_mangle]
        pub unsafe extern "C" fn $fit(
            document: *const $doc,
            request: *const c_char,
            limit: u64,
            other: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            if document.is_null() {
                return ptr::null_mut();
            }
            unsafe {
                fallible_op(
                    err_out,
                    "fitting the distortion",
                    ptr::null_mut(),
                    || {
                        let request = FitDistortionRequest::parse(borrow_cstr_bounded(
                            request,
                            "request",
                            REQUEST_LIMIT,
                        )?)?;
                        json_string(required_handle(document)?.fit_distortion(
                            &request,
                            limit,
                            other,
                            control.as_ref(),
                        )?)
                    },
                    |s| s,
                )
            }
        }
    };
}
distortion_ffi!(
    DrDocument,
    dr_doc_fit_distortion_plan,
    dr_doc_fit_distortion
);
distortion_ffi!(
    PrecisionDocument,
    dr_precision_doc_fit_distortion_plan,
    dr_precision_doc_fit_distortion
);
