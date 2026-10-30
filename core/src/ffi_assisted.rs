//! Caller-buffer boundary for assisted annotation replays. No handle, no
//! document and no image: a replay document goes in, a categorical plane or a
//! bookkeeping summary comes out.

use crate::{
    assisted_replay,
    ffi_util::{fallible_op, read_cstr, set_err},
    job_control::DrJobControl,
};
use std::ffi::{c_char, CString};

/// Rasterizes the active strokes of one validated replay into a categorical
/// 0/128/255 plane of exactly width*height bytes. The replay's own declared
/// dimensions must match. Failure, refusal or cancellation leaves out_plane
/// untouched; a refusal from the shared stroke rasterizer keeps its
/// `rasterizer_limit: ` prefix so a host can tell a bounded work limit from
/// an invalid document.
///
/// # Safety
/// replay must be a NUL-terminated string. out_plane must be valid for
/// width*height writes (nonzero, MAX_PIXELS-bounded). Optional single-use
/// control lives through concurrent queries/cancel. err_out is NULL or
/// writable and strings use dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_assisted_replay_plane(
    replay: *const c_char,
    width: u32,
    height: u32,
    out_plane: *mut u8,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    if replay.is_null() || out_plane.is_null() {
        return false;
    }
    unsafe {
        fallible_op(
            err_out,
            "assisted replay rasterization failed",
            false,
            || {
                let document = assisted_replay::parse(&read_cstr(replay, "assisted replay")?)?;
                let plane =
                    assisted_replay::label_plane(&document, width, height, control.as_ref())?;
                Ok(plane)
            },
            |plane| {
                std::ptr::copy_nonoverlapping(plane.as_ptr(), out_plane, plane.len());
                true
            },
        )
    }
}

/// The replay's bookkeeping as JSON: `final`, `outcome`, `modifying_gestures`,
/// `active_strokes`, `human_origin`, `scoreable` and `elapsed_ms`. An invalid
/// or in-progress-but-malformed document returns NULL and reports err_out;
/// a valid in-progress document succeeds with `final: false`.
///
/// # Safety
/// replay must be a NUL-terminated string. err_out is NULL or writable, and
/// both the returned string and any error use dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_assisted_replay_summary(
    replay: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    if replay.is_null() {
        unsafe { set_err(err_out, "assisted replay is NULL") };
        return std::ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "assisted replay summary failed",
            std::ptr::null_mut(),
            || {
                let document = assisted_replay::parse(&read_cstr(replay, "assisted replay")?)?;
                let summary = assisted_replay::summary(&document);
                let text = serde_json::to_string(&summary)
                    .map_err(|e| format!("assisted replay summary is not serializable: {e}"))?;
                CString::new(text).map_err(|_| "assisted replay summary is not a C string".into())
            },
            CString::into_raw,
        )
    }
}
