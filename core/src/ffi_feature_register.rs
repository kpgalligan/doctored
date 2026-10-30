//! C FFI for Auto-Align's feature registration:
//! `dr_feature_register` (the kernel `feature_register::register`) and its
//! working-memory figure, `dr_feature_register_scratch_bytes`. Plain buffers
//! in, numbers out; no handle and no job control (the size cap and the
//! kernel's caps bound the work).
use std::ffi::c_char;

use crate::feature_register::{register, scratch_bytes, MAX_SIDE, MIN_SIDE};
use crate::ffi_util::{fallible_op, mask_slice};

/// The homography taking a point of `floating` to its place in `reference`:
/// both `width × height` grey bytes, row-major, unpadded, 16 ≤ width, height
/// ≤ 4096. On true `map_out[9]` is the row-major map (m[8] = 1) in
/// continuous pixels (pixel (i, j)'s centre at (i + 0.5, j + 0.5), y down)
/// and `inliers_out` (when not NULL) the number of its RANSAC inliers, at
/// most `MAX_FEATURES` (2000). `pairs_out` (when not NULL) receives the
/// first min(inliers, `pairs_capacity`) inliers, four doubles each: floating
/// x, y, then reference x, y, in the same pixel convention.
/// `departure_out` (when not NULL) receives the lens test's departure on
/// those inliers: the worst inlier's distance, pixels, between the plain homography's
/// prediction and Geometric Distortion's model's (one radial coefficient,
/// each frame's lens at its centre, R = max(width, height) / 2), or NaN when
/// no lens model fits.
/// false with no error for a NULL buffer or no estimate; false with a
/// message for a size outside the range. Deterministic.
///
/// # Safety
/// `floating` and `reference` are NULL or valid for `width × height` bytes;
/// `map_out` is NULL or writable for 9 doubles; `pairs_out` is NULL or
/// writable for `4 × pairs_capacity` doubles; `inliers_out` and
/// `departure_out` are NULL or writable; `err_out` is NULL or writable. Free a message
/// with dr_string_free.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_feature_register(
    floating: *const u8,
    reference: *const u8,
    width: u32,
    height: u32,
    map_out: *mut f64,
    pairs_out: *mut f64,
    pairs_capacity: u32,
    inliers_out: *mut u32,
    departure_out: *mut f64,
    err_out: *mut *mut c_char,
) -> bool {
    if floating.is_null() || reference.is_null() || map_out.is_null() {
        return false;
    }
    let (w, h) = (width as usize, height as usize);
    let fit = unsafe {
        fallible_op(
            err_out,
            "registering features",
            None,
            || {
                if !(MIN_SIDE..=MAX_SIDE).contains(&w) || !(MIN_SIDE..=MAX_SIDE).contains(&h) {
                    return Err(format!(
                        "feature registration takes frames {MIN_SIDE} to {MAX_SIDE} pixels a \
                         side, not {width} × {height}"
                    ));
                }
                // The length comes from the checked size, never the caller.
                let len = w * h;
                let (Some(first), Some(second)) =
                    (mask_slice(floating, len), mask_slice(reference, len))
                else {
                    return Ok(None);
                };
                Ok(register(first, second, w, h))
            },
            |fit| fit,
        )
    };
    let Some(fit) = fit else {
        return false;
    };
    unsafe {
        std::slice::from_raw_parts_mut(map_out, 9).copy_from_slice(&fit.map);
        if !inliers_out.is_null() {
            *inliers_out = u32::try_from(fit.pairs.len()).unwrap_or(u32::MAX);
        }
        if !departure_out.is_null() {
            *departure_out = fit.departure.unwrap_or(f64::NAN);
        }
    }
    if !pairs_out.is_null() {
        // The length comes from the kernel's own count, capped by the caller's.
        let count = fit.pairs.len().min(pairs_capacity as usize);
        let out = unsafe { std::slice::from_raw_parts_mut(pairs_out, 4 * count) };
        for (chunk, (p, q)) in out.chunks_exact_mut(4).zip(&fit.pairs) {
            chunk.copy_from_slice(&[p[0], p[1], q[0], q[1]]);
        }
    }
    true
}

/// `dr_feature_register`'s peak working bytes at `width × height`; 0 outside
/// the size range.
///
/// # Safety
/// Takes no pointers; `unsafe` only for the export convention.
#[no_mangle]
pub unsafe extern "C" fn dr_feature_register_scratch_bytes(width: u32, height: u32) -> u64 {
    scratch_bytes(width as usize, height as usize)
}
