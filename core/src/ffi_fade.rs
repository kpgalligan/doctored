//! The C exports of Edit ▸ Fade (v1): the byte document's `dr_doc_fade`
//! (`doc_fade`) and the typed document's plan and edit (`precision_fade`, 16
//! bits and 32-bit float).
use std::ffi::{c_char, c_int, CString};
use std::ptr;

use crate::blend::BlendMode;
use crate::doc::DrDocument;
use crate::ffi_util::{boxed, doc_op, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count};

/// Fades layer `idx` of `doc` (the result of the step being faded) back
/// toward the same layer of `before` (the state before that step) at
/// `opacity` (0…1) in blend mode `mode` (a `DrBlendMode`) — Edit ▸ Fade.
/// `coverage` is a region step's canvas-sized u8 weight, or NULL
/// with `coverage_len` 0 for 1 everywhere; `changed_only` weighs a stroke by
/// the pixels it changed instead. The layer does not grow; under a
/// transparency lock it keeps its alpha. NULL on NULL handles, an
/// out-of-range or non-raster idx in either document, a retained source, a
/// `before` of another canvas size, Pass Through or an unknown mode, an
/// opacity outside 0…1, a coverage length other than the canvas pixel count,
/// a lock that forbids a pixel edit, or when no byte would change.
///
/// # Safety
/// `doc` and `before` must be NULL or valid pointers to live `DrDocument`s;
/// `coverage` NULL or readable for `coverage_len` bytes.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_fade(
    doc: *const DrDocument,
    before: *const DrDocument,
    idx: usize,
    mode: c_int,
    opacity: f32,
    coverage: *const u8,
    coverage_len: usize,
    changed_only: bool,
) -> *mut DrDocument {
    let Some(before) = (unsafe { before.as_ref() }) else {
        return ptr::null_mut();
    };
    let body = |d: &DrDocument| {
        let mode = BlendMode::from_c(mode)?;
        // The coverage length is the core's own canvas pixel count.
        let n = (d.width as usize).checked_mul(d.height as usize)?;
        let plane = if coverage.is_null() && coverage_len == 0 {
            None
        } else if coverage.is_null() || coverage_len != n {
            return None;
        } else {
            Some(unsafe { std::slice::from_raw_parts(coverage, n) })
        };
        d.fade(before, idx, mode, opacity, plane, changed_only)
    };
    unsafe { doc_op(doc, body) }
}

/// Plans `dr_precision_doc_fade` for layer `layer` against `before`: JSON with
/// `retained_bytes`, `scratch_bytes` and `input_samples` (the canvas pixel
/// count when `coverage`, else 0), admitted against `limit`. Refuses, before
/// allocating, a target that is not a raster of the document's own storage, a
/// retained source, and a `before` of another canvas size or bit depth. Free
/// the JSON with dr_string_free.
///
/// # Safety
/// `document` and `before` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_fade_plan(
    document: *const PrecisionDocument,
    layer: usize,
    before: *const PrecisionDocument,
    coverage: bool,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning high-depth fade",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                let before = required_handle(before)?;
                let plan = doc.fade_plan(layer, before, coverage)?;
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

/// Fades 16-bit or 32-bit float layer `layer` back toward the same layer of
/// `before` (the state before the faded step, same depth and canvas) at
/// `opacity` (0…1) in `mode` (a `DrBlendMode`; at float only the modes a
/// float document admits). `coverage` is the canvas-sized u16 weight of a
/// region step or NULL with `coverage_len` 0; `changed_only` weighs a stroke
/// by the samples it changed. An exact no-op is NULL with no error. `control`
/// is an optional single-use job. On a Grayscale document a mode the Gray
/// document does not offer refuses with `gray_document::blend_refusal`'s
/// sentence before any work (#1 phase 2c), and on a Lab Color document a
/// mode outside its list with `lab_document::blend_refusal`'s (#1 Lab
/// phase 2c).
///
/// # Safety
/// `document`/`before`/`control` NULL or live; `coverage` NULL or readable and
/// aligned for `coverage_len` u16s; `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_fade(
    document: *const PrecisionDocument,
    layer: usize,
    before: *const PrecisionDocument,
    mode: c_int,
    opacity: f64,
    coverage: *const u16,
    coverage_len: usize,
    changed_only: bool,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "fading a high-depth layer",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let doc = required_handle(document)?;
                let before = required_handle(before)?;
                let mode = BlendMode::from_c(mode).ok_or("fade blend mode is invalid")?;
                if doc.is_gray() {
                    if let Some(refusal) = crate::gray_document::blend_refusal(mode, doc.is_float())
                    {
                        return Err(refusal);
                    }
                }
                if doc.is_lab() {
                    if let Some(refusal) = crate::lab_document::blend_refusal(mode) {
                        return Err(refusal);
                    }
                }
                // The coverage length is the core's own canvas pixel count.
                let (width, height) = doc.dimensions();
                let n = pixel_count(width, height)?;
                let plane: &[u16] = if coverage.is_null() && coverage_len == 0 {
                    &[]
                } else if coverage.is_null() || coverage_len != n {
                    return Err("fade coverage length differs from the canvas".into());
                } else {
                    std::slice::from_raw_parts(coverage, n)
                };
                doc.fade(
                    layer,
                    before,
                    mode,
                    opacity,
                    plane,
                    changed_only,
                    limit,
                    other_retained_bytes,
                    job,
                )
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}
