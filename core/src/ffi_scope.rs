//! Thin FFI for full-storage destructive results written through an optional
//! canvas selection. All pointer lengths come from validated core dimensions.

use std::ffi::c_int;
use std::ptr;

use image::RgbaImage;

use crate::doc::{DrDocument, MAX_PIXELS};
use crate::doc_plane::Plane;
use crate::ffi_util::{boxed, doc_get, doc_op, fallible_op, mask_slice};
use crate::DrImage;

/// Shared pointer/size validation for the four scoped writers. `w`/`h`
/// describe the selection's canvas, never the result image's storage.
///
/// # Safety
/// Handles must be NULL or live; selection must be NULL or readable for
/// `w * h` bytes. The dimensions must describe that allocation accurately.
unsafe fn scoped_op(
    doc: *const DrDocument,
    result: *const DrImage,
    selection: *const u8,
    w: u32,
    h: u32,
    op: impl FnOnce(&DrDocument, &RgbaImage, Option<&[u8]>) -> Option<DrDocument>,
) -> *mut DrDocument {
    if result.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        doc_op(doc, |d| {
            if (w, h) != (d.width, d.height) {
                return None;
            }
            let len = (d.width as usize).checked_mul(d.height as usize)?;
            op(d, &(*result).pixels, mask_slice(selection, len))
        })
    }
}

/// Blends a full-layer result through optional canvas selection coverage in
/// encoded premultiplied color. NULL for bad target/dimensions, pixel locks
/// or unchanged bytes. NULL selection edits the full stored layer.
///
/// # Safety
/// `doc`/`result` must be NULL or live handles. `selection` must be NULL or
/// readable for `w * h` bytes, with `w`/`h` equal to the canvas dimensions.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_apply_layer_pixels(
    doc: *const DrDocument,
    idx: usize,
    result: *const DrImage,
    selection: *const u8,
    w: u32,
    h: u32,
) -> *mut DrDocument {
    unsafe {
        scoped_op(doc, result, selection, w, h, |d, r, s| {
            d.apply_layer_pixels(idx, r, s)
        })
    }
}

/// Writes a result's gray into an existing layer/group mask at its native
/// storage size, mapping optional canvas coverage through its stored offset.
/// NULL for bad target/dimensions, Lock All or unchanged bytes.
///
/// # Safety
/// `doc`/`result` must be NULL or live handles. `selection` must be NULL or
/// readable for `w * h` bytes, with `w`/`h` equal to the canvas dimensions.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_apply_layer_mask(
    doc: *const DrDocument,
    idx: usize,
    result: *const DrImage,
    selection: *const u8,
    w: u32,
    h: u32,
) -> *mut DrDocument {
    unsafe {
        scoped_op(doc, result, selection, w, h, |d, r, s| {
            d.apply_layer_mask(idx, r, s)
        })
    }
}

/// Writes a result's gray into one RGBA plane at full layer size through
/// optional canvas coverage. Derived Luma/Mask planes refuse. NULL for bad
/// target/dimensions, pixel locks or unchanged bytes; alpha edits follow the
/// existing plane writer's rule of clearing RGB when alpha changes to zero.
///
/// # Safety
/// `doc`/`result` must be NULL or live handles. `selection` must be NULL or
/// readable for `w * h` bytes, with `w`/`h` equal to the canvas dimensions.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_apply_layer_plane(
    doc: *const DrDocument,
    idx: usize,
    plane: c_int,
    result: *const DrImage,
    selection: *const u8,
    w: u32,
    h: u32,
) -> *mut DrDocument {
    unsafe {
        scoped_op(doc, result, selection, w, h, |d, r, s| {
            d.apply_layer_plane(idx, Plane::from_c(plane)?, r, s)
        })
    }
}

/// Writes a canvas-sized result's gray into a saved channel through optional
/// canvas coverage. NULL for bad target/dimensions or unchanged bytes.
///
/// # Safety
/// `doc`/`result` must be NULL or live handles. `selection` must be NULL or
/// readable for `w * h` bytes, with `w`/`h` equal to the canvas dimensions.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_apply_channel(
    doc: *const DrDocument,
    idx: usize,
    result: *const DrImage,
    selection: *const u8,
    w: u32,
    h: u32,
) -> *mut DrDocument {
    unsafe {
        scoped_op(doc, result, selection, w, h, |d, r, s| {
            d.apply_channel(idx, r, s)
        })
    }
}

/// Full stored layer/group mask as opaque grayscale, without baking or
/// clipping a group's offset. Writes its stored canvas offset through each
/// non-NULL output on success only. NULL for missing/invalid mask or index.
///
/// # Safety
/// `doc` must be NULL or live; offsets must be NULL or writable `int32_t`
/// pointers. Returned image is freed with `dr_image_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_mask_storage_image(
    doc: *const DrDocument,
    idx: usize,
    offset_x: *mut i32,
    offset_y: *mut i32,
) -> *mut DrImage {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let pixels = d.layer_mask_storage_image(idx)?;
            let offset = d.layers.get(idx)?.mask_properties.offset;
            if !offset_x.is_null() {
                *offset_x = offset.0;
            }
            if !offset_y.is_null() {
                *offset_y = offset.1;
            }
            Some(boxed(DrImage { pixels }))
        })
    }
}

/// Paints caller-owned canvas selection coverage with a premultiplied RGBA
/// overlay. Red is gray, `scale` multiplies gray/alpha by scale/255 with
/// integer flooring; source-over rounds the old coverage contribution once.
/// Validates before mutation. False for NULL, zero/over-budget dimensions,
/// or red greater than alpha. Valid unchanged strokes return true.
///
/// # Safety
/// `mask` must be NULL or writable for w*h bytes; `overlay` must be NULL or
/// readable for w*h*4 bytes. These two allocations must not overlap.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_paint_quick_mask(
    mask: *mut u8,
    overlay: *const u8,
    w: u32,
    h: u32,
    scale: u8,
) -> bool {
    if mask.is_null()
        || overlay.is_null()
        || w == 0
        || h == 0
        || u64::from(w) * u64::from(h) > MAX_PIXELS
    {
        return false;
    }
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "quick mask stroke",
            false,
            || {
                let len = w as usize * h as usize;
                let coverage = std::slice::from_raw_parts_mut(mask, len);
                let overlay = std::slice::from_raw_parts(overlay, len * 4);
                Ok(crate::selection_paint::paint_overlay(
                    coverage, overlay, scale,
                ))
            },
            |ok| ok,
        )
    }
}

/// Replays a Quick Mask stroke encoded as six-byte runs: positive UInt32LE
/// length, premultiplied gray, alpha. Runs must cover w*h exactly and each
/// gray must not exceed alpha. Validates all runs before mutating `mask`.
/// False for NULL/bad dimensions/invalid runs; valid no-ops return true.
///
/// # Safety
/// `mask` must be NULL or writable for w*h bytes; `runs` must be NULL or
/// readable for `len` bytes. These two allocations must not overlap.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_paint_quick_mask_rle(
    mask: *mut u8,
    w: u32,
    h: u32,
    runs: *const u8,
    len: usize,
) -> bool {
    let pixels = u64::from(w) * u64::from(h);
    if mask.is_null()
        || runs.is_null()
        || w == 0
        || h == 0
        || pixels > MAX_PIXELS
        || len == 0
        || len as u64 > pixels * 6
        || !len.is_multiple_of(6)
    {
        return false;
    }
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "quick mask replay",
            false,
            || {
                let coverage = std::slice::from_raw_parts_mut(mask, pixels as usize);
                let runs = std::slice::from_raw_parts(runs, len);
                Ok(crate::selection_paint::paint_runs(coverage, runs))
            },
            |ok| ok,
        )
    }
}
