//! Foreground-colour estimation at both depths: the estimate behind Select
//! and Mask's Decontaminate Colors (`SelectionRefinement+NewLayer.swift`
//! and the `refine_selection` MCP tool).

use crate::{
    doc::DrDocument,
    ffi_util::{boxed, fallible_op, read_cstr, required_handle},
    job_control::{DrJobControl, JobContext},
    precision_document::PrecisionDocument,
    precision_image::PrecisionImage,
    DrImage,
};
use std::{
    ffi::{c_char, CStr, CString},
    ptr,
};

/// Estimates foreground RGB using an opaque source and supplied alpha coverage.
/// Strict params: required color_space "encoded" or "linear", strength0..1
/// (default1). The source must match document dimensions/profile. Output retains
/// source alpha and RGB where matte0/255; the caller applies the matte separately.
/// A "linear" estimate in a document whose own matrix/TRC profile fails the
/// precision check refuses with the own-profile sentence; "encoded"
/// never does. NULL without an error means no byte changes. Error/cancel
/// also return NULL, with an owned error when err_out is provided. All inputs are immutable.
///
/// # Safety
/// Handles live for the call; alpha readable for source width*height bytes;
/// params a NUL-terminated UTF8 string. Optional control retained until every
/// concurrent poll/cancel finishes. err_out NULL or writable for an owned string
/// freed using dr_string_free. Returned handle owned and freed by dr_image_free.
#[no_mangle]
pub unsafe extern "C" fn dr_image_estimate_foreground(
    document: *const DrDocument,
    source: *const DrImage,
    alpha: *const u8,
    params: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    if document.is_null() || source.is_null() || alpha.is_null() || params.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "foreground estimation failed",
            ptr::null_mut(),
            || {
                let token = control.as_ref();
                let _run = token.map(DrJobControl::begin).transpose()?;
                let job = token.map_or(JobContext::NONE, JobContext::new);
                if CStr::from_ptr(params).to_bytes().len() > 128 * 1024 {
                    return Err("foreground parameters exceed 128 KiB".into());
                }
                let source = &*source;
                let n = source.pixels.width() as usize * source.pixels.height() as usize;
                let result = source.estimate_foreground(
                    &*document,
                    std::slice::from_raw_parts(alpha, n),
                    &read_cstr(params, "foreground parameters")?,
                    job,
                )?;
                job.check()?;
                job.report(1.0, 5)?;
                Ok(result)
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Plans `dr_precision_doc_estimate_foreground` for `source`: owned JSON with
/// `retained_bytes`, `scratch_bytes` and `input_samples` (the canvas pixel
/// count: the matte). Refuses a source that is not canvas-sized RGBA16 in the
/// document's exact profile and a profile with no matrix/TRC model. NULL
/// handles refuse through the shared boundary. Free the JSON with
/// dr_string_free.
///
/// # Safety
/// `document` and `source` NULL or live for the call; `err_out` NULL or
/// writable for an owned error string (dr_string_free).
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_estimate_foreground_plan(
    document: *const PrecisionDocument,
    source: *const PrecisionImage,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning high-depth foreground estimation",
            ptr::null_mut(),
            || {
                let plan = required_handle(document)?
                    .foreground_estimate_plan(required_handle(source)?)?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Estimates foreground RGB at 16 bits: `source` is the document's opaque
/// canvas-sized RGBA16 composite in its exact profile, `matte` the supplied
/// u16 coverage (`matte_len` must be the canvas pixel count), `params` the
/// strict JSON `{"color_space":"encoded","strength":0..1}` (strength defaults
/// to 1; "linear" refuses by name). The result keeps source alpha and the RGB
/// at matte 0 and 65535 exactly and shares the tiles it leaves unchanged; the
/// caller applies the matte separately. NULL without an error means no code
/// changes (strength 0, a binary matte, an identical result). A source pixel
/// that is not opaque, a constant fractional matte, a 32-bit float composite
/// and a failed admission against `limit` refuse with an owned error.
/// `control` is an optional single-use job.
///
/// # Safety
/// `document`, `source` and `control` NULL or live for the call; `matte` NULL
/// or readable and aligned for `matte_len` u16s; `params` NULL or a
/// NUL-terminated UTF8 string; `err_out` NULL or writable for an owned error
/// string (dr_string_free). Free the result with dr_precision_image_free.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_estimate_foreground(
    document: *const PrecisionDocument,
    source: *const PrecisionImage,
    matte: *const u16,
    matte_len: usize,
    params: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "estimating a high-depth foreground",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let doc = required_handle(document)?;
                let image = required_handle(source)?;
                if matte.is_null() || params.is_null() {
                    return Err("foreground matte or parameters are missing".into());
                }
                if CStr::from_ptr(params).to_bytes().len() > 128 * 1024 {
                    return Err("foreground parameters exceed 128 KiB".into());
                }
                // The matte's length is the core's own canvas pixel count.
                let (width, height) = doc.dimensions();
                let n = crate::precision_image::pixel_count(width, height)?;
                if matte_len != n {
                    return Err("foreground alpha dimensions do not match the source".into());
                }
                let result = doc.estimate_foreground(
                    image,
                    std::slice::from_raw_parts(matte, n),
                    &read_cstr(params, "foreground parameters")?,
                    limit,
                    other,
                    job,
                )?;
                job.check()?;
                Ok(result)
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}
