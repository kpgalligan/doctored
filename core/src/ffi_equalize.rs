//! The C export of Equalize on a byte image (#1): `dr_image_equalize`.
//! The typed documents run it as the selected edit `{"op":"equalize"}`
//! (`selected_adjustment` → `equalize_apply`).

use std::ffi::c_char;
use std::ptr;

use crate::dr_image::DrImage;
use crate::ffi_util::{boxed, fallible_op, required_handle};

/// Equalizes `image` (`equalize_apply::equalize_image`): one table from the
/// summed R, G and B histogram at 256 levels, applied to every pixel. `mask`
/// is image-sized (`w*h`) u8 histogram weight, never a write gate; NULL with
/// `mask_len` 0 weights every pixel. NULL with no error when nothing would
/// change; NULL with a message for a NULL image or a mask of another length.
///
/// # Safety
/// `image` NULL or live; `mask` NULL or readable for `mask_len` bytes;
/// `err_out` NULL or writable. Free the message with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_image_equalize(
    image: *const DrImage,
    mask: *const u8,
    mask_len: usize,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "equalizing an image",
            ptr::null_mut(),
            || {
                let image = required_handle(image)?;
                let (w, h) = image.pixels.dimensions();
                // The mask's length is the core's own pixel count.
                let n = w as usize * h as usize;
                let weight = if mask.is_null() && mask_len == 0 {
                    None
                } else if mask.is_null() || mask_len != n {
                    return Err("equalize mask length differs from the image".to_string());
                } else {
                    Some(std::slice::from_raw_parts(mask, n))
                };
                Ok(crate::equalize_apply::equalize_image(image, weight))
            },
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}
