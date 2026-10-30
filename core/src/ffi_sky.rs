//! The Sky Replacement bridge (M2a; New Layers M2b): the plan, the dialog's image
//! preview and the edit at both depths, over `doc_sky_replace`. The params
//! are the strict JSON `sky_harmonize::SkyParams::parse` reads, borrowed
//! under its 4 KiB cap; every coverage length is recomputed from the core's
//! own dimensions before the slice is formed.

use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_sky_replace::sky_replace_image;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_doc_op, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::sky_group::new_layers_bytes;
use crate::sky_harmonize::{scratch_bytes, MAX_PARAMS_BYTES};
use crate::DrImage;

/// The coverage as a slice of exactly `expected` samples.
///
/// # Safety
/// `coverage` must be NULL or readable for `count` u16s.
unsafe fn coverage_slice<'a>(
    coverage: *const u16,
    count: usize,
    expected: u64,
) -> Result<&'a [u16], String> {
    if coverage.is_null() || count as u64 != expected || count == 0 {
        return Err("invalid sky replacement coverage buffer".into());
    }
    // The length is the caller's grid, checked against the core's own size.
    Ok(unsafe { std::slice::from_raw_parts(coverage, count) })
}

/// The working set one Sky Replacement plans on a `width` × `height` canvas
/// with a `sky_width` × `sky_height` sky, writing `bits` 8 or 16. `output` 0
/// (Duplicate Layer) writes 4 or 8 bytes a pixel (32 bits is planned at 16
/// though the doc ops refuse it) and ignores `decontaminate`; `output` 1
/// (New Layers) writes `sky_group::new_layers_bytes` (the "Foreground Edge"
/// layer's bytes too when `decontaminate`) and plans 0 at other bits. 0 for
/// a zero size, an unknown `output` or other `bits`.
///
/// # Safety
/// Takes plain values only; always safe to call.
#[no_mangle]
pub unsafe extern "C" fn dr_sky_replace_scratch_bytes(
    width: u32,
    height: u32,
    sky_width: u32,
    sky_height: u32,
    bits: u32,
    output: u32,
    decontaminate: bool,
) -> u64 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "planning sky replacement",
            0,
            || {
                let output_bytes = match (output, bits) {
                    (0, 8) => 4,
                    (0, 16) => 8,
                    (0, 32) => 16,
                    (1, 8) => new_layers_bytes(false, decontaminate),
                    (1, 16) => new_layers_bytes(true, decontaminate),
                    _ => return Ok(0),
                };
                Ok(scratch_bytes(
                    (width, height),
                    (sky_width, sky_height),
                    output_bytes,
                ))
            },
            |bytes| bytes,
        )
    }
}

/// The dialog's preview: `foreground` (any size) with its sky replaced by
/// `sky`, the `coverage_width` × `coverage_height` coverage
/// (`coverage_count` u16s) read bilinearly at each foreground pixel centre
/// and the edge widths taken from the coverage's size. NULL with `err_out`
/// for a NULL handle, bad params or coverage, a plan over `limit` beside
/// `other`, no sky weight or a cancel.
///
/// # Safety
/// `foreground` and `sky` must be NULL or live `DrImage`s; `coverage` NULL or
/// readable for `coverage_count` u16s; `params_json` NULL or NUL-terminated;
/// `control` NULL or live until the call returns; `err_out` NULL or
/// writable. Free the result with `dr_image_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_sky_replace(
    foreground: *const DrImage,
    sky: *const DrImage,
    coverage: *const u16,
    coverage_count: usize,
    coverage_width: u32,
    coverage_height: u32,
    params_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "replacing the sky",
            ptr::null_mut(),
            || {
                let foreground = required_handle(foreground)?;
                let sky = required_handle(sky)?;
                let params = borrow_cstr_bounded(params_json, "params_json", MAX_PARAMS_BYTES)?;
                let expected = u64::from(coverage_width) * u64::from(coverage_height);
                let coverage = coverage_slice(coverage, coverage_count, expected)?;
                sky_replace_image(
                    &foreground.pixels,
                    &sky.pixels,
                    coverage,
                    (coverage_width, coverage_height),
                    params,
                    limit,
                    other,
                    control.as_ref(),
                )
                .map(|pixels| DrImage { pixels })
            },
            boxed,
        )
    }
}

/// Sky Replacement on byte document layer `layer`: a new document with ONE
/// canvas-sized raster layer named `params.name` directly above it, or for
/// `"output": "new_layers"` the Sky Replacement Group above its clip run. The
/// coverage is canvas-sized (`coverage_count == width·height`). NULL with
/// `err_out` for a refusal (a group or adjustment layer, bad params or
/// coverage, growth past native capacity, a plan over `limit`, no sky
/// weight) or a cancel; NULL `doc` returns NULL with `err_out` untouched.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `sky` NULL or a live
/// `DrImage`; `coverage` NULL or readable for `coverage_count` u16s;
/// `params_json` NULL or NUL-terminated; `control` NULL or live until the
/// call returns; `err_out` NULL or writable. Free with `dr_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_sky_replace(
    doc: *const DrDocument,
    layer: usize,
    sky: *const DrImage,
    coverage: *const u16,
    coverage_count: usize,
    params_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "replacing the sky", |document| {
            let sky = required_handle(sky)?;
            let params = borrow_cstr_bounded(params_json, "params_json", MAX_PARAMS_BYTES)?;
            let expected = u64::from(document.width) * u64::from(document.height);
            let coverage = coverage_slice(coverage, coverage_count, expected)?;
            document
                .sky_replace(
                    layer,
                    &sky.pixels,
                    coverage,
                    params,
                    limit,
                    other,
                    control.as_ref(),
                )
                .map(Some)
        })
    }
}

/// `dr_doc_sky_replace` for a 16-bit document: the new record is a u16
/// raster. A CMYK or 32-bit float document refuses by name. NULL always
/// carries an error.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_sky_replace`. Free with `dr_precision_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_sky_replace(
    doc: *const PrecisionDocument,
    layer: usize,
    sky: *const DrImage,
    coverage: *const u16,
    coverage_count: usize,
    params_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "replacing the sky",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let sky = required_handle(sky)?;
                let params = borrow_cstr_bounded(params_json, "params_json", MAX_PARAMS_BYTES)?;
                let (w, h) = document.dimensions();
                let coverage =
                    coverage_slice(coverage, coverage_count, u64::from(w) * u64::from(h))?;
                document.sky_replace(
                    layer,
                    &sky.pixels,
                    coverage,
                    params,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}
