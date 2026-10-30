//! Generative Layer variations' bridge (M3b phase 2): the info, the
//! select, the input of a new set and its add, at 8 bits and on the typed
//! document (16-bit RGB and 8-bit CMYK), over `doc_gen_variations`. Every
//! buffer length is recomputed from the core's own stored native size
//! before a slice is formed.

use std::ffi::c_char;
use std::ptr;

use crate::artwork_document::MAX_DESCRIPTION;
use crate::doc::DrDocument;
use crate::doc_gen_variations::no_variations;
use crate::ffi_gen_fill::{exact_mut, result_slices, write_u32s};
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_doc_op, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

/// Writes `info` to `info_out` (8 u32s) when present; false with no error
/// when absent.
///
/// # Safety
/// `info_out` must be NULL or writable for 8 u32s.
unsafe fn write_info(info_out: *mut u32, info: Option<[u32; 8]>) -> Result<bool, String> {
    match info {
        Some(values) => unsafe { write_u32s(info_out, &values) },
        None => Ok(false),
    }
}

/// Byte layer `layer`'s live variations: writes `info_out` `[count,
/// selected, native w, native h, crop x, y, w, h]` and answers true; false
/// with no error for a layer holding none; false with `err_out` for a NULL
/// handle or buffer and an out-of-range layer.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `info_out` NULL or writable for
/// 8 u32s; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_gen_fill_variations(
    doc: *const DrDocument,
    layer: usize,
    info_out: *mut u32,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "reading the generative variations",
            false,
            || write_info(info_out, required_handle(doc)?.gen_fill_variations(layer)?),
            |found| found,
        )
    }
}

/// `dr_doc_gen_fill_variations` for a typed document.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_gen_fill_variations`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gen_fill_variations(
    doc: *const PrecisionDocument,
    layer: usize,
    info_out: *mut u32,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "reading the generative variations",
            false,
            || write_info(info_out, required_handle(doc)?.gen_fill_variations(layer)?),
            |found| found,
        )
    }
}

/// Byte layer `layer` re-rendered from variation `index` (its tag swapped
/// for that picture's, its mask kept), under the layer's Pixels lock. NULL
/// with no error when `index` is already selected; NULL with `err_out` for a
/// NULL `doc`, an out-of-range layer or variation, a layer holding no
/// variations, a lock, a plan over `limit` beside `other` and a cancel.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `control` NULL or live until
/// the call returns; `err_out` NULL or writable. Free with `dr_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_gen_fill_select_variation(
    doc: *const DrDocument,
    layer: usize,
    index: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    if doc.is_null() {
        unsafe { crate::ffi_util::set_err(err_out, "required handle is NULL") };
        return ptr::null_mut();
    }
    unsafe {
        fallible_doc_op(doc, err_out, "selecting the generative variation", |d| {
            d.gen_fill_select_variation(layer, index, limit, other, control.as_ref())
        })
    }
}

/// `dr_doc_gen_fill_select_variation` for a typed document; a 32-bit float
/// or 16-bit CMYK document refuses by name.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_gen_fill_select_variation`. Free with `dr_precision_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gen_fill_select_variation(
    doc: *const PrecisionDocument,
    layer: usize,
    index: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "selecting the generative variation",
            ptr::null_mut(),
            || {
                required_handle(doc)?.gen_fill_select_variation(
                    layer,
                    index,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// The RGBA8 bytes of one picture at a stored native size.
fn native_bytes(info: Option<[u32; 8]>, layer: usize) -> Result<u64, String> {
    let info = info.ok_or_else(|| no_variations(layer))?;
    Ok(4 * u64::from(info[2]) * u64::from(info[3]))
}

/// The provider's two pictures for a new set of byte layer `layer`'s
/// variations, at its stored native size (`4·nw·nh` bytes each, the
/// `dr_doc_gen_fill_input` pictures over its stored crop, the coverage its
/// own mask, the plate with the layer hidden). False with `err_out` for a
/// NULL handle or buffer, a length mismatch, an out-of-range layer, a layer
/// holding no variations, a plan over `limit` beside `other` and a cancel.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `image_out` / `mask_out` NULL
/// or writable for `image_count` / `mask_count` bytes; `control` NULL or live
/// until the call returns; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_gen_fill_more_input(
    doc: *const DrDocument,
    layer: usize,
    image_out: *mut u8,
    image_count: usize,
    mask_out: *mut u8,
    mask_count: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "preparing the generative variations input",
            false,
            || {
                let document = required_handle(doc)?;
                let bytes = native_bytes(document.gen_fill_variations(layer)?, layer)?;
                let image = exact_mut(image_out, image_count, bytes)?;
                let mask = exact_mut(mask_out, mask_count, bytes)?;
                document.gen_fill_more_input(layer, image, mask, limit, other, control.as_ref())?;
                Ok(true)
            },
            |done| done,
        )
    }
}

/// `dr_doc_gen_fill_more_input` for a typed document (the record at opacity
/// 0 in the plate); a 32-bit float or 16-bit CMYK document refuses by name.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_gen_fill_more_input`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gen_fill_more_input(
    doc: *const PrecisionDocument,
    layer: usize,
    image_out: *mut u8,
    image_count: usize,
    mask_out: *mut u8,
    mask_count: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "preparing the generative variations input",
            false,
            || {
                let document = required_handle(doc)?;
                let bytes = native_bytes(document.gen_fill_variations(layer)?, layer)?;
                let image = exact_mut(image_out, image_count, bytes)?;
                let mask = exact_mut(mask_out, mask_count, bytes)?;
                document.gen_fill_more_input(layer, image, mask, limit, other, control.as_ref())?;
                Ok(true)
            },
            |done| done,
        )
    }
}

/// The provider's `k` new results (`dr_doc_gen_fill_apply`'s results
/// contract) appended to byte layer `layer`'s variations, tagged `tag`, the
/// first new one selected and rendered. NULL with `err_out` for a NULL `doc`
/// or buffer, a length mismatch, an out-of-range layer, a layer holding no
/// variations, a bad result or tag, a lock, a variation cap, a plan over
/// `limit` beside `other` and a cancel.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `results` NULL or readable for
/// `results_count` bytes; `result_sizes` NULL or readable for `size_count`
/// u32s; `tag` NULL or NUL-terminated; `control` NULL or live until the call
/// returns; `err_out` NULL or writable. Free with `dr_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_gen_fill_add_variations(
    doc: *const DrDocument,
    layer: usize,
    results: *const u8,
    results_count: usize,
    result_sizes: *const u32,
    size_count: usize,
    tag: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    if doc.is_null() {
        unsafe { crate::ffi_util::set_err(err_out, "required handle is NULL") };
        return ptr::null_mut();
    }
    unsafe {
        fallible_doc_op(doc, err_out, "adding the generative variations", |d| {
            let results = result_slices(results, results_count, result_sizes, size_count)?;
            let tag = borrow_cstr_bounded(tag, "tag", MAX_DESCRIPTION)?;
            d.gen_fill_add_variations(layer, &results, tag, limit, other, control.as_ref())
        })
    }
}

/// `dr_doc_gen_fill_add_variations` for a typed document; a 32-bit float or
/// 16-bit CMYK document refuses by name.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_gen_fill_add_variations`. Free with `dr_precision_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gen_fill_add_variations(
    doc: *const PrecisionDocument,
    layer: usize,
    results: *const u8,
    results_count: usize,
    result_sizes: *const u32,
    size_count: usize,
    tag: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "adding the generative variations",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let results = result_slices(results, results_count, result_sizes, size_count)?;
                let tag = borrow_cstr_bounded(tag, "tag", MAX_DESCRIPTION)?;
                document.gen_fill_add_variations(
                    layer,
                    &results,
                    tag,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}
