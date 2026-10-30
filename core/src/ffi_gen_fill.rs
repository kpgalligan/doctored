//! Generative Fill's bridge (M3b): the frame, the plan, the provider's
//! input and the apply at 8 and 16 bits (and 8-bit CMYK), over
//! `doc_gen_fill`; the variations' bridge is `ffi_gen_variations`. The host sends
//! the cloud request between input and apply. Every buffer length is
//! recomputed from the core's own canvas and the requested native size before
//! a slice is formed.

use std::ffi::c_char;
use std::ptr;

use crate::artwork_document::MAX_DESCRIPTION;
use crate::doc::DrDocument;
use crate::doc_gen_fill::{gen_fill_frame, GenResult, MAX_RESULTS};
use crate::drdc::MAX_DRDC_NAME_LEN;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::gen_fill::check_native;
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

/// A read-only slice of exactly `expected` samples, else `what`.
///
/// # Safety
/// `data` must be NULL or readable for `count` elements.
pub(crate) unsafe fn exact<'a, T>(
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

/// A writable slice of exactly `expected` bytes, else the input refusal.
///
/// # Safety
/// `data` must be NULL or writable for `count` bytes.
pub(crate) unsafe fn exact_mut<'a>(
    data: *mut u8,
    count: usize,
    expected: u64,
) -> Result<&'a mut [u8], String> {
    if data.is_null() || count as u64 != expected || count == 0 {
        return Err("invalid generative fill input buffers".into());
    }
    // The length is the caller's, checked against the core's own figure.
    Ok(unsafe { std::slice::from_raw_parts_mut(data, count) })
}

/// The RGBA8 bytes of one native picture.
fn native_bytes(native: (u32, u32)) -> Result<u64, String> {
    check_native(native)?;
    Ok(4 * u64::from(native.0) * u64::from(native.1))
}

/// Writes `values` to `out` (`values.len()` u32s).
///
/// # Safety
/// `out` must be NULL or writable for `values.len()` u32s.
pub(crate) unsafe fn write_u32s(out: *mut u32, values: &[u32]) -> Result<bool, String> {
    if out.is_null() {
        return Err("invalid generative fill input buffers".into());
    }
    unsafe { std::slice::from_raw_parts_mut(out, values.len()) }.copy_from_slice(values);
    Ok(true)
}

/// The native size and crop for the canvas-sized `coverage`
/// (`coverage_count == width·height`) among the provider's `sizes`
/// (`size_count` u32s, an even number: `(w, h)` pairs, each side 64…2048):
/// writes `frame_out` `[crop x, y, w, h, native w, native h]`. False with
/// `err_out` for a NULL or mis-sized buffer, no size, a side out of range and
/// no covered pixel.
///
/// # Safety
/// `coverage` NULL or readable for `coverage_count` u16s; `sizes` NULL or
/// readable for `size_count` u32s; `frame_out` NULL or writable for 6 u32s;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_gen_fill_frame(
    coverage: *const u16,
    coverage_count: usize,
    width: u32,
    height: u32,
    sizes: *const u32,
    size_count: usize,
    frame_out: *mut u32,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "framing the generative fill",
            false,
            || {
                let n = u64::from(width) * u64::from(height);
                let coverage = exact(
                    coverage,
                    coverage_count,
                    n,
                    "invalid generative fill coverage buffer",
                )?;
                if !size_count.is_multiple_of(2) || size_count > 64 {
                    return Err("invalid generative fill native sizes".into());
                }
                let sizes = exact(
                    sizes,
                    size_count,
                    size_count as u64,
                    "invalid generative fill native sizes",
                )?;
                let pairs: Vec<(u32, u32)> = sizes.chunks_exact(2).map(|p| (p[0], p[1])).collect();
                let frame = gen_fill_frame(coverage, (width, height), &pairs)?;
                write_u32s(frame_out, &frame)
            },
            |written| written,
        )
    }
}

/// The Generative Fill job's working set on a byte document at
/// `native_w × native_h` for `variations` stored pictures: the core's figure
/// plus the plate's. 0 with `err_out` for a NULL document or a native side
/// out of range.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_gen_fill_plan(
    doc: *const DrDocument,
    native_w: u32,
    native_h: u32,
    variations: u32,
    err_out: *mut *mut c_char,
) -> u64 {
    unsafe {
        fallible_op(
            err_out,
            "planning the generative fill",
            0,
            || required_handle(doc)?.gen_fill_plan((native_w, native_h), variations),
            |bytes| bytes,
        )
    }
}

/// `dr_doc_gen_fill_plan` for a 16-bit or 8-bit CMYK document; a 32-bit
/// float or 16-bit CMYK document refuses by name.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; `err_out` NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gen_fill_plan(
    doc: *const PrecisionDocument,
    native_w: u32,
    native_h: u32,
    variations: u32,
    err_out: *mut *mut c_char,
) -> u64 {
    unsafe {
        fallible_op(
            err_out,
            "planning the generative fill",
            0,
            || required_handle(doc)?.gen_fill_plan((native_w, native_h), variations),
            |bytes| bytes,
        )
    }
}

/// The provider's two pictures for the canvas-sized `coverage` on a byte
/// document, anchored at `layer` (any kind): `image_out` (RGBA8
/// `4·native_w·native_h`, the full composite's area-weighted mean per native
/// cell, alpha 0 on a hole cell, 255 elsewhere), `mask_out` (RGBA8, the same
/// size: (255, 255, 255, 0) where a cell's footprint touches a covered pixel,
/// else (0, 0, 0, 255)) and `crop_out` (`[x, y, w, h]`). False with `err_out`
/// for a NULL handle or buffer, a length mismatch, an out-of-range layer, a
/// native side out of range, no covered pixel, a plan over `limit` beside
/// `other` or a cancel.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `coverage` NULL or readable for
/// `coverage_count` u16s; `crop_out` NULL or writable for 4 u32s; `image_out`
/// / `mask_out` NULL or writable for `image_count` / `mask_count` bytes;
/// `control` NULL or live until the call returns; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_gen_fill_input(
    doc: *const DrDocument,
    layer: usize,
    coverage: *const u16,
    coverage_count: usize,
    native_w: u32,
    native_h: u32,
    crop_out: *mut u32,
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
            "preparing the generative fill input",
            false,
            || {
                let document = required_handle(doc)?;
                let n = u64::from(document.width) * u64::from(document.height);
                let coverage = exact(
                    coverage,
                    coverage_count,
                    n,
                    "invalid generative fill coverage buffer",
                )?;
                let bytes = native_bytes((native_w, native_h))?;
                let image = exact_mut(image_out, image_count, bytes)?;
                let mask = exact_mut(mask_out, mask_count, bytes)?;
                if crop_out.is_null() {
                    return Err("invalid generative fill input buffers".into());
                }
                let crop = document.gen_fill_input(
                    layer,
                    coverage,
                    (native_w, native_h),
                    image,
                    mask,
                    limit,
                    other,
                    control.as_ref(),
                )?;
                write_u32s(crop_out, &crop)
            },
            |written| written,
        )
    }
}

/// `dr_doc_gen_fill_input` for a 16-bit or 8-bit CMYK document (whose plate
/// is converted to sRGB); a 32-bit float or 16-bit CMYK document refuses by
/// name.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_gen_fill_input`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gen_fill_input(
    doc: *const PrecisionDocument,
    layer: usize,
    coverage: *const u16,
    coverage_count: usize,
    native_w: u32,
    native_h: u32,
    crop_out: *mut u32,
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
            "preparing the generative fill input",
            false,
            || {
                let document = required_handle(doc)?;
                let (w, h) = document.dimensions();
                let n = u64::from(w) * u64::from(h);
                let coverage = exact(
                    coverage,
                    coverage_count,
                    n,
                    "invalid generative fill coverage buffer",
                )?;
                let bytes = native_bytes((native_w, native_h))?;
                let image = exact_mut(image_out, image_count, bytes)?;
                let mask = exact_mut(mask_out, mask_count, bytes)?;
                if crop_out.is_null() {
                    return Err("invalid generative fill input buffers".into());
                }
                let crop = document.gen_fill_input(
                    layer,
                    coverage,
                    (native_w, native_h),
                    image,
                    mask,
                    limit,
                    other,
                    control.as_ref(),
                )?;
                write_u32s(crop_out, &crop)
            },
            |written| written,
        )
    }
}

/// The checked `name` and `tag` strings.
///
/// # Safety
/// `name` and `tag` must be NULL or NUL-terminated.
pub(crate) unsafe fn strings<'a>(
    name: *const c_char,
    tag: *const c_char,
) -> Result<(&'a str, &'a str), String> {
    let name = unsafe { borrow_cstr_bounded(name, "name", MAX_DRDC_NAME_LEN as usize) }?;
    let tag = unsafe { borrow_cstr_bounded(tag, "tag", MAX_DESCRIPTION) }?;
    Ok((name, tag))
}

/// Writes the new layer's index to `layer_out` when it is not NULL.
///
/// # Safety
/// `layer_out` must be NULL or writable.
unsafe fn write_layer(layer_out: *mut usize, index: usize) {
    if let Some(out) = unsafe { layer_out.as_mut() } {
        *out = index;
    }
}

/// The `k` pictures of `results` (`results_count` bytes, back to back)
/// sized by `result_sizes` (`size_count == 2k` u32s, `(w, h)` pairs), each
/// side 1…4096 and `1 ≤ k ≤ 4`; every length is recomputed from the sizes.
///
/// # Safety
/// `results` NULL or readable for `results_count` bytes; `result_sizes` NULL
/// or readable for `size_count` u32s.
pub(crate) unsafe fn result_slices<'a>(
    results: *const u8,
    results_count: usize,
    result_sizes: *const u32,
    size_count: usize,
) -> Result<Vec<GenResult<'a>>, String> {
    if size_count == 0 || !size_count.is_multiple_of(2) || size_count > 2 * MAX_RESULTS {
        return Err(format!("generative fill takes 1 to {MAX_RESULTS} results"));
    }
    let sizes = unsafe {
        exact(
            result_sizes,
            size_count,
            size_count as u64,
            "invalid generative fill result size",
        )
    }?;
    let mut lengths = Vec::with_capacity(size_count / 2);
    let mut total = 0u64;
    for pair in sizes.chunks_exact(2) {
        let bytes = u64::from(pair[0])
            .checked_mul(u64::from(pair[1]))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("invalid generative fill result size")?;
        total = total
            .checked_add(bytes)
            .ok_or("invalid generative fill result size")?;
        lengths.push(((pair[0], pair[1]), bytes as usize));
    }
    let all = unsafe {
        exact(
            results,
            results_count,
            total,
            "invalid generative fill result buffer",
        )
    }?;
    let mut out = Vec::with_capacity(lengths.len());
    let mut at = 0;
    for (size, bytes) in lengths {
        out.push((&all[at..at + bytes], size));
        at += bytes;
    }
    Ok(out)
}

/// The provider's `k` results (straight RGBA8, back to back, sized by
/// `result_sizes`, `result_slices`' contract) on a NEW canvas-sized raster
/// directly above byte `layer`: the first one opaque on the covered rect of
/// the canvas-sized `coverage`, transparent elsewhere, its mask the coverage
/// rounded to bytes, named `name` and with `tag` (a `generated` artwork
/// description) as its meta, every result kept as the layer's variations
/// (the first selected); `layer_out` takes its index. NULL with `err_out` for
/// a NULL `doc`, a NULL or mis-sized buffer, an out-of-range layer, a bad
/// native size or result, an empty name, a tag that is not a generated
/// description, no covered pixel, growth past native capacity, a variation
/// cap, a plan over `limit` beside `other`, a cancel.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `coverage` NULL or readable for
/// `coverage_count` u16s; `results` NULL or readable for `results_count`
/// bytes; `result_sizes` NULL or readable for `size_count` u32s; `name` /
/// `tag` NULL or NUL-terminated; `layer_out` NULL or writable; `control` NULL
/// or live until the call returns; `err_out` NULL or writable. Free with
/// `dr_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_gen_fill_apply(
    doc: *const DrDocument,
    layer: usize,
    coverage: *const u16,
    coverage_count: usize,
    native_w: u32,
    native_h: u32,
    results: *const u8,
    results_count: usize,
    result_sizes: *const u32,
    size_count: usize,
    name: *const c_char,
    tag: *const c_char,
    layer_out: *mut usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying the generative fill",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let n = u64::from(document.width) * u64::from(document.height);
                let coverage = exact(
                    coverage,
                    coverage_count,
                    n,
                    "invalid generative fill coverage buffer",
                )?;
                let results = result_slices(results, results_count, result_sizes, size_count)?;
                let (name, tag) = strings(name, tag)?;
                document.gen_fill_apply(
                    layer,
                    coverage,
                    (native_w, native_h),
                    &results,
                    name,
                    tag,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |out| match out {
                Some((doc, index)) => {
                    write_layer(layer_out, index);
                    boxed(doc)
                }
                None => ptr::null_mut(),
            },
        )
    }
}

/// `dr_doc_gen_fill_apply` for a 16-bit document: the new record is a u16
/// raster with a u16 mask (the coverage itself) and `tag` as its typed
/// description; on an 8-bit CMYK document an ink raster (the sRGB results
/// separated through the document's profile). A 32-bit float or 16-bit CMYK
/// document refuses by name.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_gen_fill_apply`. Free with `dr_precision_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gen_fill_apply(
    doc: *const PrecisionDocument,
    layer: usize,
    coverage: *const u16,
    coverage_count: usize,
    native_w: u32,
    native_h: u32,
    results: *const u8,
    results_count: usize,
    result_sizes: *const u32,
    size_count: usize,
    name: *const c_char,
    tag: *const c_char,
    layer_out: *mut usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying the generative fill",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let (w, h) = document.dimensions();
                let n = u64::from(w) * u64::from(h);
                let coverage = exact(
                    coverage,
                    coverage_count,
                    n,
                    "invalid generative fill coverage buffer",
                )?;
                let results = result_slices(results, results_count, result_sizes, size_count)?;
                let (name, tag) = strings(name, tag)?;
                document.gen_fill_apply(
                    layer,
                    coverage,
                    (native_w, native_h),
                    &results,
                    name,
                    tag,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |out| match out {
                Some((doc, index)) => {
                    write_layer(layer_out, index);
                    boxed(doc)
                }
                None => ptr::null_mut(),
            },
        )
    }
}
