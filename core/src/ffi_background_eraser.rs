//! The C exports of the Background Eraser (`background_eraser`): the byte layer's erase,
//! and a typed document's coverage plane with its plan (the host erases that plane with
//! `erase_native`).
use std::ffi::{c_char, CString};
use std::ptr;

use crate::background_eraser::BackgroundEraserRequest;
use crate::brush_coverage::MAX_BRUSH_DABS;
use crate::doc::DrDocument;
use crate::ffi_retouch_brush::{request_bytes, stroke};
use crate::ffi_util::{borrow_cstr_bounded, fallible_doc_op, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count};

/// The tool's request, bounded before it is read.
///
/// # Safety
/// `text` NULL or NUL-terminated UTF-8.
unsafe fn eraser_request(text: *const c_char) -> Result<BackgroundEraserRequest, String> {
    let text = unsafe { borrow_cstr_bounded(text, "background eraser request", 4096) }?;
    BackgroundEraserRequest::parse(text)
}

/// Erases layer `idx` with the Background Eraser along a stroke. `request` is
/// JSON `{"version":1,"tolerance":0..65535,"limits":"contiguous"|"discontiguous",
/// "sampling":"continuous"|"once"|"swatch"}` with the optional `swatch` (required
/// with `"swatch"` sampling, refused otherwise) and `protect` colours, each
/// `{"color":[r,g,b,a]}` straight in the document's own space, 0…1 (`color_space`
/// absent or `"document"`); the tolerance is in u16 codes (8-bit × 257).
/// `brush_request` and `dabs` are `dr_brush_u16`'s (`dab_count` packed x / y /
/// diameter / opacity tuples, at most 50000); `selection` is canvas-sized u8
/// coverage or NULL with `selection_len` 0. Under each dab the pixels like its
/// reference colour lose alpha by the tip's coverage (`background_eraser`'s module
/// doc), colour kept. NULL with no error: NULL `doc`, an out-of-range or group idx,
/// no dab reaching the canvas, a pixels or transparency lock, or no byte changing.
/// NULL with a message: a bad request, dab list or colour, a stroke past 256
/// million work units, or a selection whose length is not the canvas pixel count.
///
/// # Safety
/// `doc` NULL or a live `DrDocument`; `request` and `brush_request` NULL or
/// NUL-terminated UTF-8; `dabs` NULL for count 0 or readable and aligned for
/// `dab_count * 4` doubles; `selection` NULL or readable for `selection_len`
/// bytes; `err_out` NULL or writable. Free the message with dr_string_free.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_background_eraser_layer(
    doc: *const DrDocument,
    idx: usize,
    request: *const c_char,
    brush_request: *const c_char,
    dabs: *const f64,
    dab_count: usize,
    selection: *const u8,
    selection_len: usize,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "applying the background eraser", |d| {
            let eraser = eraser_request(request)?;
            let (brush, dabs) = stroke(brush_request, dabs, dab_count)?;
            // The selection's length is the core's own canvas pixel count.
            let n = pixel_count(d.width, d.height)?;
            let selection = if selection.is_null() && selection_len == 0 {
                None
            } else if selection.is_null() || selection_len != n {
                return Err("background eraser selection length differs from the canvas".into());
            } else {
                Some(std::slice::from_raw_parts(selection, n))
            };
            d.background_eraser_layer(idx, &eraser, &brush, dabs, selection)
        })
    }
}

/// Plans `dr_precision_doc_background_eraser_coverage` for the same layer,
/// requests and dabs: JSON with `retained_bytes`, `scratch_bytes`,
/// `output_samples` (the canvas pixel count) and `work_units`, admitted against
/// `limit`. Refuses, before allocating, what the run refuses. Free the JSON
/// with dr_string_free.
///
/// # Safety
/// `document` NULL or live; `request` and `brush_request` NULL or
/// NUL-terminated UTF-8; `dabs` NULL for count 0 or readable and aligned for
/// `dab_count * 4` doubles; `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_background_eraser_coverage_plan(
    document: *const PrecisionDocument,
    layer: usize,
    request: *const c_char,
    brush_request: *const c_char,
    dabs: *const f64,
    dab_count: usize,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning the background eraser",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                admit(
                    doc.retained_bytes(),
                    request_bytes(dab_count.min(MAX_BRUSH_DABS)),
                    other_retained_bytes,
                    limit,
                )?;
                let eraser = eraser_request(request)?;
                let (brush, dabs) = stroke(brush_request, dabs, dab_count)?;
                let plan = doc.background_eraser_coverage_plan(layer, &eraser, &brush, dabs)?;
                admit(
                    doc.retained_bytes(),
                    plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX),
                    other_retained_bytes,
                    limit,
                )?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Writes the Background Eraser's coverage of raster `layer` of a 16-bit, 32-bit
/// float or colour-mode document into `out` (canvas-sized u16, row-major, 0
/// outside the stroke): `request` is `dr_doc_background_eraser_layer`'s, but each
/// colour is `{"color":[r,g,b,a],"color_space":"document"|"srgb"|"linear"}`
/// (`linear` on a float document only) or, on a CMYK document,
/// `{"ink":[c,m,y,k,alpha]}`, all in 0…1. Nothing is erased: the host hands the
/// plane to `erase_native`. A Bitmap document refuses by name. Errors and
/// cancellation leave `out` unchanged; `control` is an optional single-use job.
///
/// # Safety
/// `document`/`control` NULL or live; `request` and `brush_request` NULL or
/// NUL-terminated UTF-8; `dabs` as the plan's; `out` NULL or writable and
/// aligned for `out_len` u16s, not aliasing any input; `err_out` NULL or
/// writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_background_eraser_coverage(
    document: *const PrecisionDocument,
    layer: usize,
    request: *const c_char,
    brush_request: *const c_char,
    dabs: *const f64,
    dab_count: usize,
    out: *mut u16,
    out_len: usize,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "computing the background eraser",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let doc = required_handle(document)?;
                let parse = request_bytes(dab_count.min(MAX_BRUSH_DABS));
                admit(doc.retained_bytes(), parse, other_retained_bytes, limit)?;
                job.reserve(parse)?;
                let eraser = eraser_request(request)?;
                let (brush, dabs) = stroke(brush_request, dabs, dab_count)?;
                // The output's length is the core's own canvas pixel count.
                let (width, height) = doc.dimensions();
                let n = pixel_count(width, height)?;
                if out.is_null() || out_len != n {
                    return Err("background eraser output length differs from the canvas".into());
                }
                let coverage = doc.background_eraser_coverage(
                    layer,
                    &eraser,
                    &brush,
                    dabs,
                    limit,
                    other_retained_bytes,
                    job,
                )?;
                ptr::copy_nonoverlapping(coverage.as_ptr(), out, n);
                Ok(())
            },
            |()| true,
        )
    }
}
