//! Independent mask properties and mask-only transforms across the C boundary.

use crate::doc::DrDocument;
use crate::doc_transform::Affine;
use crate::ffi_util::{doc_get, doc_op, filter_from_c};

/// Whether the same layer index in two documents has equal optional raw mask
/// bytes/dimensions, mask-enabled flag and all independent mask properties.
/// False for NULL documents or an invalid index. Does not copy pixel buffers.
/// # Safety
/// Both documents must be NULL or live immutable handles for the whole call.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_masks_equal(
    first: *const DrDocument,
    second: *const DrDocument,
    idx: usize,
) -> bool {
    if second.is_null() {
        return false;
    }
    unsafe {
        doc_get(first, false, |a| {
            Some(crate::doc_lock::same_mask(
                a.layers.get(idx)?,
                (&*second).layers.get(idx)?,
            ))
        })
    }
}
use std::ffi::{c_char, c_int, CString};
use std::ptr;

/// Raw mask storage placement and nondestructive properties as JSON.
/// NULL for missing mask/index/document. Free with dr_string_free.
/// # Safety
/// `doc` must be NULL or a live document handle.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_mask_properties(
    doc: *const DrDocument,
    idx: usize,
) -> *mut c_char {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let layer = d.layers.get(idx)?;
            let mask = layer.mask.as_ref()?;
            let p = &layer.mask_properties;
            CString::new(serde_json::json!({"linked":p.linked,"density":p.density,"feather":p.feather,"outside_coverage":p.outside_coverage,
            "offset_x":p.offset.0,"offset_y":p.offset.1,"width":mask.width(),"height":mask.height()}).to_string())
            .ok().map(CString::into_raw)
        })
    }
}

/// Pure property setter. Finite density 0..1 and feather radius 0..1000.
/// NULL on invalid/missing/locked mask, over-cap feather extent or no-op.
/// # Safety
/// `doc` must be NULL or a live document handle.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_mask_properties(
    doc: *const DrDocument,
    idx: usize,
    linked: bool,
    density: f32,
    feather: f32,
) -> *mut DrDocument {
    unsafe {
        doc_op(doc, |d| {
            d.set_mask_properties(idx, linked, density, feather)
        })
    }
}

/// Pure mask-only affine in canvas space, matrix order a,b,c,d,tx,ty;
/// kernel is a DrResizeFilter value. Pixels and mask properties are retained.
/// NULL for invalid geometry/missing mask/Lock All/no-op.
/// # Safety
/// `doc` must be NULL or live; matrix must be NULL or readable for six doubles.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_transform_mask(
    doc: *const DrDocument,
    idx: usize,
    matrix: *const f64,
    filter: c_int,
) -> *mut DrDocument {
    if matrix.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        doc_op(doc, |d| {
            let mut values = [0.0; 6];
            values.copy_from_slice(std::slice::from_raw_parts(matrix, 6));
            d.transform_mask(idx, Affine::from_array(values), filter_from_c(filter)?)
        })
    }
}

/// Replaces raster pixels and offset, retaining raw mask/placement/properties
/// exactly in canvas coordinates. Linking does not move it a second time.
/// NULL on invalid dimensions, group, lock refusal or no-op.
/// # Safety
/// doc must be NULL or live; src must be NULL or readable for w*h*4 bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_layer_content_preserving_mask(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    x: i32,
    y: i32,
) -> *mut DrDocument {
    if src.is_null() || w == 0 || h == 0 || u64::from(w) * u64::from(h) > crate::doc::MAX_PIXELS {
        return ptr::null_mut();
    }
    unsafe {
        doc_op(doc, |d| {
            d.raster_layer(idx)?;
            let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
            let pixels =
                image::RgbaImage::from_raw(w, h, std::slice::from_raw_parts(src, len).to_vec())?;
            d.set_layer_content_preserving_mask(idx, pixels, (x, y))
        })
    }
}

/// Pure byte set-mask: replaces raster entry `idx`'s mask with the
/// `w*h` coverage plane placed at canvas (`x`, `y`), enabled, with the given
/// `linked` flag and `outside_coverage` (0 or 255); density 1, feather 0.
/// NULL for NULL/empty/oversized coverage, a group or out-of-range index,
/// invalid properties, a mask-budget refusal, Lock All or an identical mask.
/// # Safety
/// `doc` must be NULL or live; `coverage` must be NULL or readable for w*h bytes.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_doc_set_mask(
    doc: *const DrDocument,
    idx: usize,
    coverage: *const u8,
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    linked: bool,
    outside_coverage: u8,
) -> *mut DrDocument {
    if coverage.is_null()
        || w == 0
        || h == 0
        || u64::from(w) * u64::from(h) > crate::doc::MAX_PIXELS
    {
        return ptr::null_mut();
    }
    unsafe {
        doc_op(doc, |d| {
            let len = (w as usize).checked_mul(h as usize)?;
            let plane = image::GrayImage::from_raw(
                w,
                h,
                std::slice::from_raw_parts(coverage, len).to_vec(),
            )?;
            d.set_mask(idx, plane, (x, y), linked, outside_coverage)
        })
    }
}

/// Pure whole-canvas mask Invert: inverts entry `idx`'s mask coverage
/// inside the stored rectangle and outside it. `selection` NULL inverts
/// everything and flips the outside coverage; otherwise it is canvas `w*h`
/// coverage bytes, `selection_len` must equal `w*h`, and a raster entry's
/// stored plane grows to hold the canvas before the scoped write. NULL for a
/// NULL document, a bad index, a missing mask, a wrong `selection_len`, Lock
/// All, a mask-budget refusal or a selection that changes nothing.
/// # Safety
/// `doc` must be NULL or live; `selection` must be NULL or readable for
/// `selection_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_invert_mask(
    doc: *const DrDocument,
    idx: usize,
    selection: *const u8,
    selection_len: usize,
) -> *mut DrDocument {
    unsafe {
        doc_op(doc, |d| {
            if selection.is_null() {
                return d.invert_mask(idx, None);
            }
            let len = (d.width as usize).checked_mul(d.height as usize)?;
            if selection_len != len {
                return None;
            }
            d.invert_mask(idx, Some(std::slice::from_raw_parts(selection, len)))
        })
    }
}
