//! C FFI for model masks (Select ▸ Sky, Select ▸ Person Parts…,
//! Object Finder) — the "Model masks" section of `include/doctored_core.h`,
//! over the kernel `model_mask`. It takes no document: the host sends a
//! segmentation model's output plane and the canvas rectangle it covers, and
//! reads back a canvas-sized u16 coverage plane, admitted against its working
//! limit as `dr_landmark_coverage` is.

use std::ffi::c_char;
use std::ptr;
use std::slice;

use crate::ffi_util::{borrow_cstr_bounded, fallible_op};
use crate::job_control::{DrJobControl, JobContext};
use crate::model_mask::{check_plane, coverage_scratch, model_mask_coverage, request_from_json};
use crate::precision_image::{admit, pixel_count};

/// The longest request JSON read: a frame, a values word and at most
/// sixteen channels fit in a few hundred bytes.
const REQUEST_JSON_LIMIT: usize = 64 * 1024;

/// The u16 coverage (65535 = covered) of a model output `plane` over the
/// `width` x `height` canvas, written row-major into `output`, and in
/// `covered_out` the count of samples at or above 32768. `plane` holds
/// `plane_channels` channel-major planes of `plane_width` x `plane_height`
/// f32 samples; `request_json` is `{"frame":[x,y,w,h],"values":"logit"|
/// "probability","channels":[c…]}` (see `model_mask::request_from_json`).
/// `clip` is NULL or a canvas-sized byte plane multiplied in. `limit` /
/// `other` are the caller's working limit and retained bytes; the charge is
/// `model_mask::coverage_scratch`. False with `err_out` set, and `output`
/// untouched, for a NULL plane, request or output, a count mismatch, a
/// malformed request, a frame outside the canvas, a non-finite sample, a
/// broken cap, a refused admission or a cancellation. `control` may be NULL.
///
/// # Safety
/// `plane` must be NULL or readable for `plane_count` f32s; `request_json`
/// must be NULL or a NUL-terminated string; `clip` must be NULL or readable
/// for `clip_count` bytes; `output` must be NULL or writable for
/// `output_count` u16s; `covered_out` must be NULL or writable; `control`
/// must be NULL or a live single-use job control; `err_out` must be NULL or a
/// writable CString pointer, freed with dr_string_free.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_model_mask_coverage(
    plane: *const f32,
    plane_count: usize,
    plane_width: u32,
    plane_height: u32,
    plane_channels: u32,
    request_json: *const c_char,
    clip: *const u8,
    clip_count: usize,
    width: u32,
    height: u32,
    output: *mut u16,
    output_count: usize,
    covered_out: *mut u64,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "computing model mask coverage",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                let n = pixel_count(width, height)?;
                if output.is_null() || output_count != n {
                    return Err("invalid model mask coverage buffers".into());
                }
                if plane.is_null() {
                    return Err("model output plane is NULL".into());
                }
                // The caps and the count are checked before anything is
                // admitted or borrowed, so a claimed length past them is
                // never read.
                check_plane(plane_count, (plane_width, plane_height, plane_channels))?;
                if !clip.is_null() && clip_count != n {
                    return Err("model mask clip must be canvas-sized".into());
                }
                let text = borrow_cstr_bounded(
                    request_json,
                    "model mask request json",
                    REQUEST_JSON_LIMIT,
                )?;
                let input = plane_count as u64 * 4
                    + text.len() as u64
                    + if clip.is_null() { 0 } else { n as u64 }
                    + n as u64 * 2;
                let scratch = coverage_scratch(width, height, plane_count);
                admit(input, scratch, other, limit)?;
                job.reserve(scratch)?;
                let request = request_from_json(text, plane_channels, (width, height))?;
                // Both lengths are checked against the core's own sizes above.
                let samples = slice::from_raw_parts(plane, plane_count);
                let clip = (!clip.is_null()).then(|| slice::from_raw_parts(clip, n));
                let (coverage, covered) = model_mask_coverage(
                    samples,
                    (plane_width, plane_height, plane_channels),
                    &request,
                    clip,
                    (width, height),
                    job,
                )?;
                job.check()?;
                ptr::copy_nonoverlapping(coverage.as_ptr(), output, n);
                if !covered_out.is_null() {
                    *covered_out = covered;
                }
                Ok(())
            },
            |()| true,
        )
    }
}
