//! The typed cutout bridge: Select and Mask's New Layer outputs at 16 and 32
//! bits (the sheet and the `refine_selection` MCP tool).
use crate::{
    ffi_util::{boxed, fallible_op, required_handle},
    job_control::DrJobControl,
    precision_document::PrecisionDocument,
    precision_image::PrecisionImage,
};
use std::{
    ffi::{c_char, CString},
    ptr,
};

/// Owned JSON describing native refinement assembly and its complete inputs.
/// The foreground is RGBA16, or RGBA32 float in a 32-bit float document; its
/// alpha stays separate from the refinement matte. Free JSON with
/// dr_string_free. NULL or invalid handles refuse through the shared boundary.
/// # Safety
/// Handles are NULL/live immutable owners; err_out is NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_refinement_cutout_plan(
    document: *const PrecisionDocument,
    foreground: *const PrecisionImage,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning native refinement cutout",
            ptr::null_mut(),
            || {
                let plan = required_handle(document)?
                    .refinement_cutout_plan(required_handle(foreground)?)?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Adds the native foreground (RGBA16, or RGBA32 float in a 32-bit float
/// document) unchanged plus an independent u16 mask on a
/// new top root, hiding prior roots while preserving their complete state.
/// The matte is copied after admission; source alpha is neither replaced nor
/// multiplied into it. Empty effective coverage returns NULL without error.
/// # Safety
/// Handles are NULL/live; matte is NULL or readable for document width*height
/// native-endian u16 samples. Control is NULL/live until all work/polls end;
/// err_out is NULL/writable. Free the result with dr_precision_doc_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_add_refinement_cutout(
    document: *const PrecisionDocument,
    foreground: *const PrecisionImage,
    matte: *const u16,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "assembling native refinement cutout",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                let image = required_handle(foreground)?;
                if matte.is_null() {
                    return Err("native refinement matte is missing".into());
                }
                let (w, h) = doc.dimensions();
                let n = w as usize * h as usize;
                doc.add_refinement_cutout(
                    image,
                    std::slice::from_raw_parts(matte, n),
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}
