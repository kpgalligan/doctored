//! Byte seam for Move Selected Pixels and Free Transform of the
//! selected pixels: a delta-independent plan and the edit. The typed
//! depth rides `dr_precision_doc_edit`'s `move_selected` /
//! `transform_selected` arm. Both depths also answer the FLOAT's own rect
//! (`dr_doc_selected_pixels_bounds` / `dr_precision_doc_selected_pixels_bounds`):
//! ⌘T's opaque-bounds box and a Move drag's early empty refusal.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::selected_pixels::{CanvasRect, SelectedPixelsEdit};

/// Plan a byte selected-pixels edit. The returned JSON is
/// `{retained_bytes, scratch_bytes, input_samples}`, sample-free and
/// independent of a move's delta; a transform's scratch adds 16 bytes per
/// pixel of the landing of the layer's on-canvas rect grown by the filter
/// reach (0 for an identity or integer translation, the whole canvas at the
/// horizon). Free it with `dr_string_free`. A `transform_selected` plan also
/// refuses bad geometry; it has no work refusal.
/// # Safety
/// document/request are NULL or live/terminated; err_out is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_selected_pixels_edit_plan(
    document: *const DrDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning selected pixels",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", 64 * 1024)?;
                let edit = SelectedPixelsEdit::parse(request)?;
                CString::new(document.selected_pixels_plan(&edit)?.json().to_string())
                    .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Float the layer's pixels × canvas u16 coverage (length must equal
/// width*height) and move them by (dx, dy), or (`transform_selected`)
/// resample them as premultiplied f32 through the request's matrix or
/// source_rect→quad map: hole a·(1−c), Normal source-over, landing clipped to
/// the canvas, mask untouched. NULL with no error means identity or no stored
/// sample changed; errors are refusal sentences. No work refusal: the
/// run is bounded by its plan's memory and the control's cancel.
/// # Safety
/// document/control are NULL or live; request is NULL or terminated; coverage
/// is NULL or readable for `length` u16s; err_out is NULL or writable. The
/// caller owns every handle and buffer for the full call.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_doc_selected_pixels_edit(
    document: *const DrDocument,
    request: *const c_char,
    coverage: *const u16,
    length: usize,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "moving selected pixels",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", 64 * 1024)?;
                let edit = SelectedPixelsEdit::parse(request)?;
                let expected = document.width as usize * document.height as usize;
                if coverage.is_null() || length != expected {
                    return Err("selected pixels coverage must be canvas-sized u16".into());
                }
                let coverage = std::slice::from_raw_parts(coverage, expected);
                document.selected_pixels_edit(
                    &edit,
                    coverage,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |result| {
                result.map_or(ptr::null_mut(), |document| {
                    Box::into_raw(Box::new(document))
                })
            },
        )
    }
}

/// Writes a found rect to `out` (NULL writes nothing): true for a rect,
/// false for an empty float.
unsafe fn write_rect(rect: Option<CanvasRect>, out: *mut u32) -> bool {
    let Some((x, y, w, h)) = rect else {
        return false;
    };
    if !out.is_null() {
        // F lies on the canvas, so its origin is never negative.
        let values = [x as u32, y as u32, w, h];
        unsafe { std::slice::from_raw_parts_mut(out, 4) }.copy_from_slice(&values);
    }
    true
}

/// The FLOAT's canvas rect for a byte layer × canvas u16 coverage (length
/// must equal width*height): the bbox of the pixels with alpha > 0 and
/// coverage > 0. True writes `x, y, w, h` to `out_rect` (NULL writes
/// nothing); false with no error is an empty float; false with an error is a
/// refusal (length, index, group, adjustment) or "operation cancelled". Reads
/// the layer in place and allocates nothing; no lock check.
/// # Safety
/// document/control are NULL or live; coverage is NULL or readable for
/// `length` u16s; out_rect is NULL or writable for 4 u32s; err_out is NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_selected_pixels_bounds(
    document: *const DrDocument,
    layer: usize,
    coverage: *const u16,
    length: usize,
    control: *const DrJobControl,
    out_rect: *mut u32,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "finding the selected pixels bounds",
            false,
            || {
                let document = required_handle(document)?;
                let expected = document.width as usize * document.height as usize;
                if coverage.is_null() || length != expected {
                    return Err("selected pixels coverage must be canvas-sized u16".into());
                }
                let coverage = std::slice::from_raw_parts(coverage, expected);
                let rect = document.selected_pixels_bounds(layer, coverage, control.as_ref())?;
                Ok(write_rect(rect, out_rect))
            },
            |found| found,
        )
    }
}

/// The typed twin of `dr_doc_selected_pixels_bounds`; `layer` is the
/// postorder record index, and a 32-bit float document refuses by name.
/// # Safety
/// As `dr_doc_selected_pixels_bounds`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_selected_pixels_bounds(
    document: *const PrecisionDocument,
    layer: usize,
    coverage: *const u16,
    length: usize,
    control: *const DrJobControl,
    out_rect: *mut u32,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "finding the selected pixels bounds",
            false,
            || {
                let document = required_handle(document)?;
                let (w, h) = document.dimensions();
                let expected = w as usize * h as usize;
                if coverage.is_null() || length != expected {
                    return Err("selected pixels coverage must be canvas-sized u16".into());
                }
                let coverage = std::slice::from_raw_parts(coverage, expected);
                let rect = document.selected_pixels_bounds(layer, coverage, control.as_ref())?;
                Ok(write_rect(rect, out_rect))
            },
            |found| found,
        )
    }
}
