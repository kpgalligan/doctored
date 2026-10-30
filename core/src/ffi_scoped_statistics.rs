//! Scoped statistics bridge. Empty coverage means the whole named source;
//! otherwise native u16 canvas coverage is borrowed unchanged for the call.
//! The byte projection pair is written out by hand below the macro:
//! its run borrows a document AND a projection.
use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::scoped_statistics::AnalysisRequest;
use crate::DrImage;
use std::ffi::{c_char, CString};
use std::ptr;

macro_rules! analysis_bridge {
    ($doc:ty, $plan:ident, $run:ident, $pixels:expr) => {
        /// Owned JSON scratch plan; successful strings use dr_string_free.
        /// # Safety
        /// Handles NULL/live, request NULL/terminated UTF-8, err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $plan(document: *const $doc, request: *const c_char, err_out: *mut *mut c_char) -> *mut c_char {
            unsafe { fallible_op(err_out, "planning scoped statistics", ptr::null_mut(), || {
                let document = required_handle(document)?;
                let request = AnalysisRequest::parse(borrow_cstr_bounded(request, "analysis request", 4096)?)?;
                let scratch = document.scoped_analysis_plan(&request)?;
                CString::new(serde_json::json!({"scratch_bytes":scratch,"coverage_denominator":65535}).to_string()).map_err(|e|e.to_string())
            }, CString::into_raw) }
        }
        /// Complete scoped native-code statistics; only display bins use stride.
        /// # Safety
        /// Handles NULL/live; request NULL/terminated UTF-8; coverage NULL with
        /// length0, or readable/aligned for length u16; err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $run(document: *const $doc, request: *const c_char, coverage: *const u16, length: usize, limit: u64, other: u64, control: *const DrJobControl, err_out: *mut *mut c_char) -> *mut c_char {
            unsafe { fallible_op(err_out, "reading scoped statistics", ptr::null_mut(), || {
                let document = required_handle(document)?;
                let request = AnalysisRequest::parse(borrow_cstr_bounded(request, "analysis request", 4096)?)?;
                let pixels: fn(&$doc) -> usize = $pixels;
                let expected = pixels(document);
                if length != 0 && (length != expected || coverage.is_null()) { return Err("invalid scoped analysis coverage pointer or length".into()); }
                let coverage = if length == 0 { &[] } else { std::slice::from_raw_parts(coverage, expected) };
                CString::new(document.scoped_analysis(&request, coverage, limit, other, control.as_ref())?.to_string()).map_err(|e|e.to_string())
            }, CString::into_raw) }
        }
    }
}
analysis_bridge!(
    PrecisionDocument,
    dr_precision_doc_scoped_analysis_plan,
    dr_precision_doc_scoped_analysis,
    |d| d.width as usize * d.height as usize
);
analysis_bridge!(
    DrDocument,
    dr_doc_scoped_analysis_plan,
    dr_doc_scoped_analysis,
    |d| d.width as usize * d.height as usize
);
analysis_bridge!(
    PrecisionImage,
    dr_precision_image_scoped_analysis_plan,
    dr_precision_image_scoped_analysis,
    |i| {
        let (width, height) = i.dimensions();
        width as usize * height as usize
    }
);

/// Owned JSON plan of the composite scope over a supplied 8-bit projection:
/// `{"scratch_bytes", "coverage_denominator"}`, no render charged;
/// layer/backdrop refuse "projection analysis takes the composite source
/// only". Hand-written beside the macro because the run takes two handles.
/// # Safety
/// Handles NULL/live, request NULL/terminated UTF-8, err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_projection_scoped_analysis_plan(
    document: *const DrDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning scoped statistics",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = AnalysisRequest::parse(borrow_cstr_bounded(
                    request,
                    "analysis request",
                    4096,
                )?)?;
                let scratch = document.projection_scoped_analysis_plan(&request)?;
                CString::new(
                    serde_json::json!({"scratch_bytes":scratch,"coverage_denominator":65535})
                        .to_string(),
                )
                .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// The composite scope's complete statistics counted over `projection`, the
/// document's supplied (retained) canvas-sized 8-bit projection, instead of a
/// render: `dr_doc_scoped_analysis`'s schema and answers. Coverage is
/// canvas-sized (its length recomputed from the document). Refuses
/// layer/backdrop sources, a projection that is not canvas-sized ("projection
/// is not the canvas size"), bad coverage, a working-limit overflow and a
/// cancel.
/// # Safety
/// Handles NULL/live; request NULL/terminated UTF-8; coverage NULL with
/// length0, or readable/aligned for length u16; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_projection_scoped_analysis(
    document: *const DrDocument,
    projection: *const DrImage,
    request: *const c_char,
    coverage: *const u16,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading scoped statistics",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let projection = required_handle(projection)?;
                let request = AnalysisRequest::parse(borrow_cstr_bounded(
                    request,
                    "analysis request",
                    4096,
                )?)?;
                let expected = document.width as usize * document.height as usize;
                if length != 0 && (length != expected || coverage.is_null()) {
                    return Err("invalid scoped analysis coverage pointer or length".into());
                }
                let coverage = if length == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(coverage, expected)
                };
                CString::new(
                    document
                        .projection_scoped_analysis(
                            &projection.pixels,
                            &request,
                            coverage,
                            limit,
                            other,
                            control.as_ref(),
                        )?
                        .to_string(),
                )
                .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}
