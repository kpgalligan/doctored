//! C FFI for landmark regions (Select ▸ Face Parts…) — the "Landmark
//! regions" section of `include/doctored_core.h`, over the kernel
//! `landmark_region`. It takes no document: the host sends point loops in
//! canvas coordinates and reads back a canvas-sized u16 coverage plane,
//! admitted against its working limit as `dr_path_coverage` is.

use std::ffi::c_char;
use std::ptr;

use crate::ffi_path::shape_scratch;
use crate::ffi_util::{borrow_cstr_bounded, fallible_op};
use crate::job_control::{DrJobControl, JobContext};
use crate::landmark_region::{coverage_scratch, landmark_coverage, regions_from_json};
use crate::precision_image::{admit, pixel_count};

/// The longest request JSON read: every point at its cap (262,144) printed
/// at full f64 precision is under 12 MiB. The cap bounds the parse before
/// any of it is read.
const REGIONS_JSON_LIMIT: usize = 16 * 1024 * 1024;

/// The u16 coverage (65535 = covered) of the landmark regions in
/// `regions_json` over the `width` x `height` canvas, written row-major into
/// `output`: the union (per-sample max) of each region's
/// `include × clip × (65535 − exclude)`, every loop a closed centripetal
/// Catmull–Rom spline through its points (a `half_width` line, the band
/// around it). `limit` / `other` are the caller's working limit and retained
/// bytes; the charge is `landmark_region::coverage_scratch` and the parsed
/// request. False with `err_out` set, and `output` untouched, for NULL or
/// malformed JSON, a broken cap, `output_count != width * height`, a zero or
/// oversized canvas, a refused admission, the rasterizer's caps, or a
/// cancellation. `control` may be NULL. An empty `regions` covers nothing.
///
/// # Safety
/// `regions_json` must be NULL or a NUL-terminated string; `output` must be
/// NULL or writable for `output_count` u16s; `control` must be NULL or a live
/// single-use job control; `err_out` must be NULL or a writable CString
/// pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_landmark_coverage(
    regions_json: *const c_char,
    width: u32,
    height: u32,
    output: *mut u16,
    output_count: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "rasterizing landmark coverage",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                let n = pixel_count(width, height)?;
                if output.is_null() || output_count != n {
                    return Err("invalid landmark coverage buffers".into());
                }
                let text =
                    borrow_cstr_bounded(regions_json, "landmark regions json", REGIONS_JSON_LIMIT)?;
                let input = text.len() as u64 + n as u64 * 2;
                let scratch = coverage_scratch(width, height) + shape_scratch(text.len());
                admit(input, scratch, other, limit)?;
                job.reserve(scratch)?;
                let regions = regions_from_json(text)?;
                let coverage = landmark_coverage(&regions, width, height, job)?;
                job.check()?;
                // The length is the canvas's, recomputed above.
                ptr::copy_nonoverlapping(coverage.as_ptr(), output, n);
                Ok(())
            },
            |()| true,
        )
    }
}
