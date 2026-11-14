//! Select ▸ Focus Area's C entry point (`focus_area`, `designs/focus-area.md`).

use crate::{
    ffi_util::{fallible_op, read_cstr},
    job_control::{DrJobControl, JobContext},
    DrImage,
};
use std::ffi::{c_char, CStr};

/// Byte coverage of the in-focus part of `source` (straight RGBA8), written
/// to `out_mask` (`w * h` bytes, the source's own dimensions), and the
/// `range` it used to `*out_range`: the request's own, or Auto's resolved
/// value, which maps back onto the same cut. Strict JSON `{range?: 0..100
/// (absent or null: Auto), soften_edge?: bool (true)}`. The `focus_area`
/// module doc states every rule. NULL source/params/out_mask/out_range
/// return false with err_out untouched; any other failure, a cancel included,
/// leaves both outputs untouched. A NULL control disables cancellation.
///
/// # Safety
/// `source` must be a live image for the call, `params` a NUL-terminated UTF8
/// string, `out_mask` writable for the source's `w * h` bytes, `out_range`
/// writable, `control` NULL or live (it may be cancelled concurrently), and
/// `err_out` NULL or writable for an owned error string (dr_string_free).
#[no_mangle]
pub unsafe extern "C" fn dr_selection_focus_area(
    source: *const DrImage,
    params: *const c_char,
    out_mask: *mut u8,
    out_range: *mut f64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    if source.is_null() || params.is_null() || out_mask.is_null() || out_range.is_null() {
        return false;
    }
    unsafe {
        fallible_op(
            err_out,
            "focus area failed",
            false,
            || {
                if CStr::from_ptr(params).to_bytes().len() > 4096 {
                    return Err("focus area parameters exceed 4 KiB".into());
                }
                let request =
                    crate::focus_area::parse(&read_cstr(params, "focus area parameters")?)?;
                let token = control.as_ref();
                let _run = token.map(DrJobControl::begin).transpose()?;
                let job = token.map_or(JobContext::NONE, JobContext::new);
                let image = &(*source).pixels;
                let (mask, range) = crate::focus_area::focus_area(
                    image.as_raw(),
                    image.dimensions(),
                    &request,
                    job,
                )?;
                job.check()?;
                job.report(1.0, 5)?;
                std::ptr::copy_nonoverlapping(mask.as_ptr(), out_mask, mask.len());
                *out_range = range;
                Ok(())
            },
            |()| true,
        )
    }
}
