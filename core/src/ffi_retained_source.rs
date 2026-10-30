//! Distinct byte and typed retained-source owners share strict request semantics.
//! The byte document plans and edits through `dr_doc_mixed_source_plan` /
//! `_edit` with `source16` NULL.
//!
//! `ffi_retained_source.rs` exposes strict same-depth compatibility and mixed-input APIs with
//! bounded plans.
use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, handle_get, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::retained_source::{SourceEdit, SourceInput};
use std::ffi::{c_char, CString};
use std::ptr;

macro_rules! source_ffi {
    ($doc:ty,$info:ident,$info_scratch:ident,$has:ident) => {
        /// Whether the layer's retained binding is still valid, without JSON or
        /// recipe allocation. NULL/out-of-range/plain/invalid bindings are false.
        /// # Safety
        /// document is NULL or a live depth-correct owner through return.
        #[no_mangle]
        pub unsafe extern "C" fn $has(document: *const $doc, index: usize) -> bool {
            unsafe { handle_get(document, false, |d| d.has_retained_source(index)) }
        }
        /// Conservative core/host source-info scratch, counted without JSON copies.
        /// NULL/plain/out-of-range returns 64 KiB; overflow returns u64::MAX.
        /// # Safety
        /// document is NULL or a live depth-correct owner through return.
        #[no_mangle]
        pub unsafe extern "C" fn $info_scratch(document: *const $doc, index: usize) -> u64 {
            unsafe { handle_get(document, 64 * 1024, |d| d.source_info_scratch_bytes(index)) }
        }
        /// Owned source metadata JSON; a plain layer reports valid:false.
        /// # Safety
        /// document NULL/live and err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $info(
            document: *const $doc,
            index: usize,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            unsafe {
                fallible_op(
                    err_out,
                    "reading retained source",
                    ptr::null_mut(),
                    || {
                        CString::new(required_handle(document)?.source_info(index)?.to_string())
                            .map_err(|e| e.to_string())
                    },
                    CString::into_raw,
                )
            }
        }
    };
}
source_ffi!(
    DrDocument,
    dr_doc_source_info,
    dr_doc_source_info_scratch_bytes,
    dr_doc_has_retained_source
);
source_ffi!(
    PrecisionDocument,
    dr_precision_doc_source_info,
    dr_precision_doc_source_info_scratch_bytes,
    dr_precision_doc_has_retained_source
);

/// Owned source plan JSON; source is required only for placement.
/// # Safety
/// Handles are NULL/live; request NULL/terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_source_plan(
    document: *const PrecisionDocument,
    source: *const PrecisionDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning retained source",
            ptr::null_mut(),
            || {
                let edit = SourceEdit::parse(borrow_cstr_bounded(request, "request", 64 * 1024)?)?;
                let plan = required_handle(document)?.source_plan(source.as_ref(), &edit)?;
                CString::new(plan.json().to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}
/// Atomic source placement, retained affine, or explicit rasterization.
/// # Safety
/// Handles/token NULL/live through return; request terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_source_edit(
    document: *const PrecisionDocument,
    source: *const PrecisionDocument,
    request: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "editing retained source",
            ptr::null_mut(),
            || {
                let edit = SourceEdit::parse(borrow_cstr_bounded(request, "request", 64 * 1024)?)?;
                required_handle(document)?.source_edit(
                    source.as_ref(),
                    &edit,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

macro_rules! mixed_source_ffi {
    ($doc:ty,$plan:ident,$edit:ident) => {
        /// Plan an immutable source edit using exactly one input depth when required.
        /// # Safety
        /// Handles NULL/live; request terminated UTF8; err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $plan(
            document: *const $doc,
            source8: *const DrDocument,
            source16: *const PrecisionDocument,
            request: *const c_char,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            unsafe {
                fallible_op(
                    err_out,
                    "planning mixed-depth source",
                    ptr::null_mut(),
                    || {
                        let input = SourceInput::from_handles(source8.as_ref(), source16.as_ref())?;
                        let edit =
                            SourceEdit::parse(borrow_cstr_bounded(request, "request", 64 * 1024)?)?;
                        CString::new(
                            required_handle(document)?
                                .source_plan_input(
                                    input,
                                    &edit,
                                    crate::sampling_gaussian::CONTROLLED_JOB_WORK,
                                )?
                                .json()
                                .to_string(),
                        )
                        .map_err(|e| e.to_string())
                    },
                    CString::into_raw,
                )
            }
        }
        /// Apply source placement/replacement, capture/rasterize, or existing geometry.
        /// A 32-bit float document renders the master into linear light
        /// with no depth authorization and admits a float source document as
        /// a 32-bit float master; an 8- or 16-bit document
        /// admits one under `convert_fallback` and renders it clipped to its
        /// range. In a float document an 8- or 16-bit master
        /// in a LUT profile renders through its table; a document
        /// profile, or a float master's, with no matrix/TRC model refuses
        /// (the header states the sentences).
        /// # Safety
        /// Handles/token NULL/live through return; request terminated UTF8; err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $edit(
            document: *const $doc,
            source8: *const DrDocument,
            source16: *const PrecisionDocument,
            request: *const c_char,
            limit: u64,
            other: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut $doc {
            unsafe {
                fallible_op(
                    err_out,
                    "editing mixed-depth source",
                    ptr::null_mut(),
                    || {
                        let input = SourceInput::from_handles(source8.as_ref(), source16.as_ref())?;
                        let edit =
                            SourceEdit::parse(borrow_cstr_bounded(request, "request", 64 * 1024)?)?;
                        required_handle(document)?.source_edit_input(
                            input,
                            &edit,
                            limit,
                            other,
                            control.as_ref(),
                        )
                    },
                    |d| d.map_or(ptr::null_mut(), boxed),
                )
            }
        }
    };
}
mixed_source_ffi!(
    DrDocument,
    dr_doc_mixed_source_plan,
    dr_doc_mixed_source_edit
);
mixed_source_ffi!(
    PrecisionDocument,
    dr_precision_doc_mixed_source_plan,
    dr_precision_doc_mixed_source_edit
);
