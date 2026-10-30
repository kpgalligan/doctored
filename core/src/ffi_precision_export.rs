//! Captured authoritative projection export through the shared typed codec.

use crate::ffi_util::{borrow_cstr_bounded, fallible_op, required_handle};
use crate::precision_codec::PrecisionFormat;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use std::ffi::{c_char, CString};
use std::ptr;

/// Owned plan JSON for the unique document/projection union plus codec scratch.
/// Free with dr_string_free. No pixel allocation or rendering is performed.
/// # Safety
/// document/image NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_projection_export_plan(
    document: *const PrecisionDocument,
    image: *const PrecisionImage,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning high-depth projection export",
            ptr::null_mut(),
            || {
                let plan =
                    required_handle(document)?.projection_export_plan(required_handle(image)?)?;
                CString::new(plan.json().to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Save a captured RGBA16 projection as PNG16 (0) or TIFF16 (1), without another
/// render. Requires exact canvas dimensions and ICC bytes; the host guarantees
/// source/revision pairing. other excludes the union of BOTH input owners.
/// Noncancellable dependency codecs; report_out receives CARRIES bits on success.
/// # Safety
/// document/image NULL/live; path NULL/terminated UTF-8 of at most 64 KiB;
/// report_out/err_out NULL/writable. Keep both immutable inputs live until return.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_save_projection16(
    document: *const PrecisionDocument,
    image: *const PrecisionImage,
    path: *const c_char,
    format: u32,
    embed_profile: bool,
    strip_metadata: bool,
    limit: u64,
    other: u64,
    report_out: *mut u32,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "saving high-depth projection",
            false,
            || {
                let document = required_handle(document)?;
                let image = required_handle(image)?;
                let path = borrow_cstr_bounded(path, "path", 64 * 1024)?;
                let format = match format {
                    0 => PrecisionFormat::Png,
                    1 => PrecisionFormat::Tiff,
                    _ => return Err("unsupported high-depth export format".into()),
                };
                document.save_projection16(
                    image,
                    path,
                    format,
                    embed_profile,
                    strip_metadata,
                    limit,
                    other,
                )
            },
            |report| {
                if !report_out.is_null() {
                    *report_out = report;
                }
                true
            },
        )
    }
}
