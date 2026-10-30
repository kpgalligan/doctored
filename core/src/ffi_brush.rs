//! Standalone bounded native brush geometry transfer, with atomic u16 output.
use crate::brush_coverage::{BrushPlan, BrushRequest, MAX_BRUSH_DABS};
use crate::ffi_util::{borrow_cstr_bounded, fallible_op};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use std::ffi::{c_char, CString};
use std::ptr;

#[allow(clippy::too_many_arguments)]
unsafe fn request<'a>(
    width: u32,
    height: u32,
    text: *const c_char,
    dabs: *const f64,
    count: usize,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<(BrushRequest, &'a [f64], BrushPlan), String> {
    pixel_count(width, height)?;
    if count > MAX_BRUSH_DABS || (count > 0 && dabs.is_null()) {
        return Err(
            "native brush requires a valid array of at most 50000 dabs; use a shorter stroke or wider spacing"
                .into(),
        );
    }
    admit(0, 4 * BOOKKEEPING_BYTES + count as u64 * 32, other, limit)?;
    job.reserve(4 * BOOKKEEPING_BYTES + count as u64 * 32)?;
    let text = unsafe { borrow_cstr_bounded(text, "native brush request", 4096) }?;
    let request = BrushRequest::parse(text)?;
    let dabs = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(dabs, count * 4) }
    };
    let plan = request.plan((width, height), dabs)?;
    admit(0, plan.scratch_bytes, other, limit)?;
    job.reserve(plan.scratch_bytes)?;
    Ok((request, dabs, plan))
}
/// Plan version1 native brush coverage with packed x/y/diameter/opacity f64
/// tuples, at most50000. Free returned JSON with dr_string_free.
/// # Safety
/// text NULL/terminated UTF8; dabs NULL for count0 or readable/aligned for
/// count*4 doubles; err_out NULL/writable. Inputs remain immutable during call.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_u16_plan(
    width: u32,
    height: u32,
    text: *const c_char,
    dabs: *const f64,
    count: usize,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning native brush",
            ptr::null_mut(),
            || {
                let (_, _, p) = request(
                    width,
                    height,
                    text,
                    dabs,
                    count,
                    limit,
                    other,
                    JobContext::NONE,
                )?;
                CString::new(serde_json::json!({"retained_bytes":0,"scratch_bytes":p.scratch_bytes,"output_samples":p.output_samples,"work_units":p.work_units}).to_string()).map_err(|e|e.to_string())
            },
            CString::into_raw,
        )
    }
}
/// Rasterize the same bounded brush into canvas u16 coverage. The complete
/// stroke accumulates continuously and quantizes once. Errors/cancellation
/// leave caller output unchanged; control is an optional single-use job.
/// # Safety
/// text/dabs follow plan contract; out NULL or writable/aligned for out_len
/// u16 and nonaliasing inputs; control NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_u16(
    width: u32,
    height: u32,
    text: *const c_char,
    dabs: *const f64,
    count: usize,
    out: *mut u16,
    out_len: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "rasterizing native brush",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (request, dabs, plan) =
                    request(width, height, text, dabs, count, limit, other, job)?;
                if out.is_null() || out_len != plan.output_samples {
                    return Err("native brush output length differs from canvas".into());
                }
                let output = request.rasterize((width, height), dabs, job)?;
                job.report(1., 5)?;
                ptr::copy_nonoverlapping(output.as_ptr(), out, plan.output_samples);
                Ok(())
            },
            |()| true,
        )
    }
}
