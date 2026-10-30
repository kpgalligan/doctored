//! Byte cutout assembly and its projection: Select and Mask's New Layer
//! outputs (the sheet and the `refine_selection` MCP tool).
use crate::{
    doc::DrDocument,
    ffi_util::{boxed, doc_get, fallible_op},
    job_control::{DrJobControl, JobContext},
    DrImage,
};
use std::{ffi::c_char, ptr};

/// Conservative assembly scratch, including new pixel/mask storage and cloned
/// document structure, excluding borrowed inputs and later display rendering.
/// Returns zero for NULL or panic.
/// # Safety
/// document is NULL or a live immutable handle for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_refinement_cutout_scratch_bytes(
    document: *const DrDocument,
) -> u64 {
    unsafe {
        doc_get(document, 0, |doc| {
            Some(doc.refinement_cutout_scratch_bytes())
        })
    }
}

/// Adds a canvas-sized foreground (RGB in the document profile; its own alpha
/// is kept on the new layer) and separate alpha as a top-level "Refined
/// Cutout" layer with a linked, enabled, density-one, feather-zero mask. Hides
/// previous top-level entries, preserving their pixels/masks/structure. A
/// result whose alpha times pixel alpha is zero everywhere returns NULL
/// without error. Failure/cancellation returns NULL and optional owned error;
/// inputs unchanged. Optional job control is single-use.
/// # Safety
/// Handles are NULL or live; alpha is NULL or readable for document width*height
/// bytes. Control stays alive until all concurrent poll/cancel calls complete.
/// err_out is NULL or writable; free its owned string with dr_string_free.
/// The returned owned document is freed with dr_doc_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_add_refinement_cutout(
    document: *const DrDocument,
    foreground: *const DrImage,
    alpha: *const u8,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    if document.is_null() || foreground.is_null() || alpha.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "refinement output assembly failed",
            ptr::null_mut(),
            || {
                let token = control.as_ref();
                let _run = token.map(DrJobControl::begin).transpose()?;
                let job = token.map_or(JobContext::NONE, JobContext::new);
                let document = &*document;
                let n = document.width as usize * document.height as usize;
                let result = document.add_refinement_cutout(
                    &*foreground,
                    std::slice::from_raw_parts(alpha, n),
                    job,
                )?;
                job.report(1.0, 5)?;
                Ok(result)
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Plan the exact byte-cutout projection shape. Scratch includes its
/// final RGBA8 output, bounded compositor/quantizer chunks and forecast context
/// parse workspace, not borrowed source/records or later host display storage.
/// The plan parses no metadata JSON. NULL returns zero.
/// # Safety
/// Document is NULL or live; err_out is NULL or writable. Free any owned error
/// with dr_string_free. No pixels are allocated by this plan.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_refinement_cutout_render_scratch_bytes(
    document: *const DrDocument,
    err_out: *mut *mut c_char,
) -> u64 {
    if document.is_null() {
        return 0;
    }
    unsafe {
        fallible_op(
            err_out,
            "planning cutout projection",
            0,
            || (&*document).refinement_cutout_render_scratch_bytes(),
            |v| v,
        )
    }
}

/// Project one plain visible cutout root plus default mask through the
/// existing byte compositor/quantizer in bounded chunks. Prior roots must be
/// hidden and all shape/profile preconditions must hold; the root's own alpha
/// is composited as stored. A fresh optional token bounds scratch and polls
/// cancellation; no fallback renderer.
/// # Safety
/// Document/control are NULL or live throughout all calls. err_out is NULL or
/// writable. Free output with dr_image_free and any error with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_render_refinement_cutout(
    document: *const DrDocument,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    if document.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "rendering cutout projection",
            ptr::null_mut(),
            || {
                let token = control.as_ref();
                let _run = token.map(DrJobControl::begin).transpose()?;
                let job = token.map_or(JobContext::NONE, JobContext::new);
                Ok(DrImage {
                    pixels: (&*document).render_refinement_cutout(job)?,
                })
            },
            boxed,
        )
    }
}
