//! The Remove tool's model engine bridge (M3a): the plan, the native
//! input and the composite at 8 and 16 bits, over `doc_remove_model`. The
//! host runs the model between input and apply. Every buffer length is
//! recomputed from the core's own canvas and the requested native `side`
//! before a slice is formed.

use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::remove_model::check_side;
use crate::retouch_sampling::RetouchSampling;

/// The sampling mode `sample` names.
///
/// # Safety
/// `sample` must be NULL or NUL-terminated.
unsafe fn sampling(sample: *const c_char) -> Result<RetouchSampling, String> {
    match unsafe { borrow_cstr_bounded(sample, "sample", 64) }? {
        "current" => Ok(RetouchSampling::Current),
        "current_and_below" => Ok(RetouchSampling::CurrentAndBelow),
        "all" => Ok(RetouchSampling::All),
        _ => Err("sample must be current, current_and_below or all".into()),
    }
}

/// A read-only slice of exactly `expected` samples, else `what`.
///
/// # Safety
/// `data` must be NULL or readable for `count` elements.
unsafe fn exact<'a, T>(
    data: *const T,
    count: usize,
    expected: u64,
    what: &str,
) -> Result<&'a [T], String> {
    if data.is_null() || count as u64 != expected || count == 0 {
        return Err(what.into());
    }
    // The length is the caller's, checked against the core's own figure.
    Ok(unsafe { std::slice::from_raw_parts(data, count) })
}

/// A writable slice of exactly `expected` samples, else the input refusal.
///
/// # Safety
/// `data` must be NULL or writable for `count` elements.
unsafe fn exact_mut<'a, T>(
    data: *mut T,
    count: usize,
    expected: u64,
) -> Result<&'a mut [T], String> {
    if data.is_null() || count as u64 != expected || count == 0 {
        return Err("invalid remove model input buffers".into());
    }
    // The length is the caller's, checked against the core's own figure.
    Ok(unsafe { std::slice::from_raw_parts_mut(data, count) })
}

fn cells(side: u32) -> Result<u64, String> {
    check_side(side)?;
    Ok(u64::from(side) * u64::from(side))
}

/// Writes the crop to `crop_out` (4 u32s).
///
/// # Safety
/// `crop_out` must be NULL or writable for 4 u32s.
unsafe fn write_crop(crop_out: *mut u32, crop: [u32; 4]) -> bool {
    if crop_out.is_null() {
        return false;
    }
    unsafe { std::slice::from_raw_parts_mut(crop_out, 4) }.copy_from_slice(&crop);
    true
}

/// The Remove model job's working set on byte layer `layer` for `sample`
/// ("current" | "current_and_below" | "all"), a model of native `side`
/// (64…1024) and `new_layer` output: the core's figure plus the plate's. 0
/// with `err_out` for a NULL document, an out-of-range or non-raster layer,
/// a bad sample or side.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `sample` NULL or
/// NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_remove_model_plan(
    doc: *const DrDocument,
    layer: usize,
    sample: *const c_char,
    side: u32,
    new_layer: bool,
    err_out: *mut *mut c_char,
) -> u64 {
    unsafe {
        fallible_op(
            err_out,
            "planning the remove model",
            0,
            || {
                let document = required_handle(doc)?;
                let mode = sampling(sample)?;
                document.remove_model_plan(layer, mode, side, new_layer)
            },
            |bytes| bytes,
        )
    }
}

/// `dr_doc_remove_model_plan` for a 16-bit document; a 32-bit float or CMYK
/// document refuses by name. A Lab Color document's plan adds its view
/// pipeline's tables (#547 Lab 2f).
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_remove_model_plan`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_remove_model_plan(
    doc: *const PrecisionDocument,
    layer: usize,
    sample: *const c_char,
    side: u32,
    new_layer: bool,
    err_out: *mut *mut c_char,
) -> u64 {
    unsafe {
        fallible_op(
            err_out,
            "planning the remove model",
            0,
            || {
                let document = required_handle(doc)?;
                let mode = sampling(sample)?;
                document.remove_model_plan(layer, mode, side, new_layer)
            },
            |bytes| bytes,
        )
    }
}

/// The model's native input for the canvas-sized `coverage`
/// (`coverage_count == width·height`) on byte layer `layer`: `image_out`
/// (`3·side²` f32, channel-major RGB 0…1, the plate `sample` names, with
/// adjustment layers left out when `ignore_adjustments`), `mask_out`
/// (`side²`, 1 = remove) and `crop_out` (`[x, y, w, h]`, the canvas crop the
/// native frame covers). False with `err_out` for a NULL handle or buffer, a
/// length mismatch, an out-of-range or non-raster layer, a bad sample or
/// side, no covered pixel, nothing to sample around the stroke, a plan over
/// `limit` beside `other` or a cancel.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `sample` NULL or
/// NUL-terminated; `coverage` NULL or readable for `coverage_count` u16s;
/// `crop_out` NULL or writable for 4 u32s; `image_out` / `mask_out` NULL or
/// writable for `image_count` / `mask_count` f32s; `control` NULL or live
/// until the call returns; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_remove_model_input(
    doc: *const DrDocument,
    layer: usize,
    sample: *const c_char,
    ignore_adjustments: bool,
    coverage: *const u16,
    coverage_count: usize,
    side: u32,
    crop_out: *mut u32,
    image_out: *mut f32,
    image_count: usize,
    mask_out: *mut f32,
    mask_count: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "preparing the remove model input",
            false,
            || {
                let document = required_handle(doc)?;
                let mode = sampling(sample)?;
                let n = u64::from(document.width) * u64::from(document.height);
                let coverage = exact(
                    coverage,
                    coverage_count,
                    n,
                    "invalid remove coverage buffer",
                )?;
                let cells = cells(side)?;
                let image = exact_mut(image_out, image_count, 3 * cells)?;
                let mask = exact_mut(mask_out, mask_count, cells)?;
                if crop_out.is_null() {
                    return Err("invalid remove model input buffers".into());
                }
                let crop = document.remove_model_input(
                    layer,
                    mode,
                    ignore_adjustments,
                    coverage,
                    side,
                    image,
                    mask,
                    limit,
                    other,
                    control.as_ref(),
                )?;
                Ok(write_crop(crop_out, crop))
            },
            |written| written,
        )
    }
}

/// `dr_doc_remove_model_input` for a 16-bit document; a 32-bit float or CMYK
/// document refuses by name. On a Lab Color document `image_out` is the
/// plate's Display P3 view (`lab_convert::view_pipeline`, #547 Lab 2f).
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_remove_model_input`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_remove_model_input(
    doc: *const PrecisionDocument,
    layer: usize,
    sample: *const c_char,
    ignore_adjustments: bool,
    coverage: *const u16,
    coverage_count: usize,
    side: u32,
    crop_out: *mut u32,
    image_out: *mut f32,
    image_count: usize,
    mask_out: *mut f32,
    mask_count: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "preparing the remove model input",
            false,
            || {
                let document = required_handle(doc)?;
                let mode = sampling(sample)?;
                let (w, h) = document.dimensions();
                let n = u64::from(w) * u64::from(h);
                let coverage = exact(
                    coverage,
                    coverage_count,
                    n,
                    "invalid remove coverage buffer",
                )?;
                let cells = cells(side)?;
                let image = exact_mut(image_out, image_count, 3 * cells)?;
                let mask = exact_mut(mask_out, mask_count, cells)?;
                if crop_out.is_null() {
                    return Err("invalid remove model input buffers".into());
                }
                let crop = document.remove_model_input(
                    layer,
                    mode,
                    ignore_adjustments,
                    coverage,
                    side,
                    image,
                    mask,
                    limit,
                    other,
                    control.as_ref(),
                )?;
                Ok(write_crop(crop_out, crop))
            },
            |written| written,
        )
    }
}

/// The model's `plane` (`plane_count == 3·side²` f32, channel-major RGB
/// 0…1, the crop `dr_doc_remove_model_input` answered for the same
/// `coverage` and `side`) composited over byte layer `layer` by the
/// canvas-sized `coverage`, inside the layer's extent on the canvas, or with
/// `new_layer` onto a new canvas-sized raster "Remove" directly above it.
/// NULL with `err_out` untouched when nothing changed; every other NULL
/// carries a message: a NULL `doc`, a NULL or mis-sized buffer, an
/// out-of-range or non-raster layer, a bad side, no covered pixel, a
/// non-finite plane, a plan over `limit` beside `other`, a cancel.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `coverage` NULL or readable for
/// `coverage_count` u16s; `plane` NULL or readable for `plane_count` f32s;
/// `control` NULL or live until the call returns; `err_out` NULL or writable.
/// Free with `dr_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_remove_model_apply(
    doc: *const DrDocument,
    layer: usize,
    coverage: *const u16,
    coverage_count: usize,
    side: u32,
    plane: *const f32,
    plane_count: usize,
    new_layer: bool,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying the remove model",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let n = u64::from(document.width) * u64::from(document.height);
                let coverage = exact(
                    coverage,
                    coverage_count,
                    n,
                    "invalid remove coverage buffer",
                )?;
                let cells = cells(side)?;
                let plane = exact(plane, plane_count, 3 * cells, "invalid remove model plane")?;
                document.remove_model_apply(
                    layer,
                    coverage,
                    side,
                    plane,
                    new_layer,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// `dr_doc_remove_model_apply` for a 16-bit document: the new record is a
/// u16 raster. A 32-bit float or CMYK document refuses by name. On a Lab
/// Color document the plane is Display P3, converted to Lab codes before the
/// coverage mix (`lab_convert::from_view_pipeline`, #547 Lab 2f). NULL with
/// `err_out` untouched when nothing changed; every other NULL carries a
/// message (a NULL `doc` too).
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_remove_model_apply`. Free with `dr_precision_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_remove_model_apply(
    doc: *const PrecisionDocument,
    layer: usize,
    coverage: *const u16,
    coverage_count: usize,
    side: u32,
    plane: *const f32,
    plane_count: usize,
    new_layer: bool,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying the remove model",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let (w, h) = document.dimensions();
                let n = u64::from(w) * u64::from(h);
                let coverage = exact(
                    coverage,
                    coverage_count,
                    n,
                    "invalid remove coverage buffer",
                )?;
                let cells = cells(side)?;
                let plane = exact(plane, plane_count, 3 * cells, "invalid remove model plane")?;
                document.remove_model_apply(
                    layer,
                    coverage,
                    side,
                    plane,
                    new_layer,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}
