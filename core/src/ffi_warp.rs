//! Bounded byte and typed persisted-warp FFI.
use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::warp::WarpEdit;

macro_rules! warp_ffi {
    ($doc:ty,$info:ident,$plan:ident,$edit:ident) => {
        /// # Safety
        /// The handle is NULL or live; err_out is NULL or writable.
        #[no_mangle]
        pub unsafe extern "C" fn $info(
            document: *const $doc,
            layer: usize,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            unsafe {
                fallible_op(
                    err_out,
                    "reading warp",
                    ptr::null_mut(),
                    || {
                        CString::new(required_handle(document)?.warp_info(layer)?.to_string())
                            .map_err(|e| e.to_string())
                    },
                    CString::into_raw,
                )
            }
        }
        /// # Safety
        /// The handle is NULL or live, request is NUL terminated, and err_out is NULL or writable.
        #[no_mangle]
        pub unsafe extern "C" fn $plan(
            document: *const $doc,
            request: *const c_char,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            unsafe {
                fallible_op(
                    err_out,
                    "planning warp",
                    ptr::null_mut(),
                    || {
                        let edit =
                            WarpEdit::parse(borrow_cstr_bounded(request, "request", 64 * 1024)?)?;
                        CString::new(
                            required_handle(document)?
                                .warp_plan(&edit)?
                                .json()
                                .to_string(),
                        )
                        .map_err(|e| e.to_string())
                    },
                    CString::into_raw,
                )
            }
        }
        /// # Safety
        /// Handles are NULL or live for the call; request is terminated; err_out is NULL or writable.
        #[no_mangle]
        pub unsafe extern "C" fn $edit(
            document: *const $doc,
            request: *const c_char,
            limit: u64,
            other_retained_bytes: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut $doc {
            unsafe {
                fallible_op(
                    err_out,
                    "editing warp",
                    ptr::null_mut(),
                    || {
                        let edit =
                            WarpEdit::parse(borrow_cstr_bounded(request, "request", 64 * 1024)?)?;
                        required_handle(document)?.warp_edit(
                            &edit,
                            limit,
                            other_retained_bytes,
                            control.as_ref(),
                        )
                    },
                    |result| result.map_or(ptr::null_mut(), |doc| Box::into_raw(Box::new(doc))),
                )
            }
        }
    };
}
warp_ffi!(
    DrDocument,
    dr_doc_warp_info,
    dr_doc_warp_edit_plan,
    dr_doc_warp_edit
);
warp_ffi!(
    PrecisionDocument,
    dr_precision_doc_warp_info,
    dr_precision_doc_warp_edit_plan,
    dr_precision_doc_warp_edit
);
