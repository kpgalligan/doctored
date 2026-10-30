//! Atomic caller-buffer boundary for categorical trimap stroke construction.

use crate::{
    ffi_util::{fallible_op, read_cstr},
    job_control::{DrJobControl, JobContext},
    trimap_strokes,
};
use std::ffi::{c_char, CStr};

/// Applies version1 ordered round-cap strokes to an existing categorical trimap.
/// Only 0/128/255 labels are valid; untouched pixels retain their original label.
/// Pixel centers at distance <= radius are overwritten, without AA or blending.
/// Strict JSON {version,width,height,strokes:[{label,radius,points}]} permits at
/// most256 strokes,8192 points/stroke,65536 points,total4MiB; radius0.5..512 and
/// finite coordinates±100000. Sum of clipped segment-box visits <=64 million.
/// Failure/cancellation leaves out_trimap untouched. A successful identity
/// returns true and identical bytes; host history decides whether it is a no-op.
///
/// # Safety
/// Input/output must be valid for width*height bytes (nonzero/MAX_PIXELS-bounded).
/// Buffers may alias: publication occurs after all input reads. params must be
/// a NUL-terminated string. Optional single-use control lives through concurrent
/// queries/cancel. err_out is NULL or writable and strings use dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn rz_trimap_strokes(
    trimap: *const u8,
    width: u32,
    height: u32,
    params: *const c_char,
    out_trimap: *mut u8,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    if trimap.is_null() || params.is_null() || out_trimap.is_null() {
        return false;
    }
    unsafe {
        fallible_op(
            err_out,
            "trimap stroke construction failed",
            false,
            || {
                let n = trimap_strokes::pixel_count(width, height)?;
                let token = control.as_ref();
                let _run = token.map(DrJobControl::begin).transpose()?;
                let job = token.map_or(JobContext::NONE, JobContext::new);
                let bytes = CStr::from_ptr(params).to_bytes().len();
                if bytes > trimap_strokes::MAX_JSON_BYTES {
                    return Err("trimap stroke payload exceeds 4 MiB".into());
                }
                job.reserve(trimap_strokes::scratch_bytes(n, bytes))?;
                let input = std::slice::from_raw_parts(trimap, n);
                let output = trimap_strokes::apply_strokes(
                    input,
                    width,
                    height,
                    &read_cstr(params, "trimap strokes")?,
                    job,
                )?;
                job.check()?;
                job.report(1.0, 5)?;
                std::ptr::copy_nonoverlapping(output.as_ptr(), out_trimap, n);
                Ok(())
            },
            |()| true,
        )
    }
}
