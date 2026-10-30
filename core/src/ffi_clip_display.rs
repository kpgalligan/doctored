//! The ⌥-drag clipping display: byte and typed threshold
//! renders of an edited composite. Display only; nothing is persisted.
use crate::clip_display::{clip_display_rgba8, ClipDisplay};
use crate::ffi_util::{boxed, fallible_op, pure_op, required_handle};
use crate::precision_image::PrecisionImage;
use crate::DrImage;
use std::ffi::c_char;
use std::ptr;

/// Threshold display of a byte image: mode 1 (highlights) is a black ground
/// with each RGB channel at 255 lit to 255; mode 2 (shadows) is a white ground
/// with each RGB channel at 0 dropped to 0. Alpha-0 pixels show the ground;
/// output alpha is 255. Mode 3 (overlay, View ▸ Show Clipping) is transparent
/// except where a channel is clipped: any at 0 blue (0,0,255,255), any at 255
/// red (255,0,0,255), both magenta (255,0,255,255). NULL on a NULL image or
/// any other mode.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_clip_display(img: *const DrImage, mode: u32) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            ClipDisplay::from_code(mode).map(|mode| DrImage {
                pixels: clip_display_rgba8(&i.pixels, mode),
            })
        })
    }
}

/// The same display (modes 1, 2 and 3) over exact native u16 codes (0 and 65535 only), read
/// from the tiles. f32 storage tests linear values instead: a channel >= 1.0 is
/// at the high endpoint and one <= 0.0 at the low one; a NaN or non-positive
/// alpha shows the ground. Admits w*h*4 plus 1MiB scratch against `limit` with
/// `other_retained`; a bad mode or a refused admission report err_out. Result
/// uses dr_image_free.
///
/// # Safety
/// `image` NULL/live through return; `err_out` NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_clip_display(
    image: *const PrecisionImage,
    mode: u32,
    limit: u64,
    other_retained: u64,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering clipping display",
            ptr::null_mut(),
            || {
                let image = required_handle(image)?;
                let mode = ClipDisplay::from_code(mode).ok_or(
                    "clipping display mode must be 1 (highlights), 2 (shadows) or 3 (overlay)",
                )?;
                Ok(DrImage {
                    pixels: image.clip_display(mode, limit, other_retained)?,
                })
            },
            boxed,
        )
    }
}
