//! Additive byte weighted-Auto analysis; the historical entry remains unchanged.
use crate::dr_image::DrImage;
use crate::ffi_util::{fallible_op, required_handle};
use crate::ops_auto::{self, AutoMode};
use std::ffi::{c_char, CString};
use std::ptr;

/// Owned derived Levels JSON; empty coverage uses historical whole-source Auto.
/// # Safety
/// image NULL/live; coverage NULL/length0 or readable for length bytes;
/// err_out NULL/writable. Successful JSON uses dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_image_selected_auto_levels(
    image: *const DrImage,
    coverage: *const u8,
    length: usize,
    mode: u32,
    clip: f64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "deriving selection-weighted Auto levels",
            ptr::null_mut(),
            || {
                let image = required_handle(image)?;
                if !clip.is_finite() || !(0.0..=0.1).contains(&clip) {
                    return Err("clip must be0...0.1".into());
                }
                if length != 0
                    && (coverage.is_null()
                        || length != image.pixels.width() as usize * image.pixels.height() as usize)
                {
                    return Err("invalid selected Auto coverage".into());
                }
                let coverage = if length == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(coverage, length)
                };
                let mode = AutoMode::from_c(i32::try_from(mode).map_err(|_| "invalid Auto mode")?)
                    .ok_or("invalid Auto mode")?;
                let values =
                    ops_auto::selected_auto_levels(&image.pixels, coverage, mode, clip as f32)?;
                let (black, white, gamma) = values.unwrap_or(([0.; 3], [1.; 3], [1.; 3]));
                CString::new(serde_json::json!({"black":black,"white":white,"gamma":gamma,"identity":values.is_none()}).to_string()).map_err(|e|e.to_string())
            },
            CString::into_raw,
        )
    }
}
