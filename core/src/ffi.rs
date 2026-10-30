//! C FFI layer implementing `include/doctored_core.h` exactly.
//!
//! Every function is panic-safe: bodies run under [`catch_unwind`] so no
//! unwinding ever crosses the FFI boundary. Panics become NULL/0/false
//! returns (with an error message where the contract provides `err_out`).
//! NULL handles are always tolerated.

use std::ffi::{c_char, c_int, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use crate::doc::MAX_PIXELS;
use crate::ffi_util::{boxed, fallible_op, filter_from_c, produce_op, pure_op, read_cstr};
use crate::{ops, DrImage, Format};

/// Opens the image file at `path` (UTF-8), sniffing the container format;
/// `8BPS` files decode as flattened Photoshop composites. A grayscale file
/// with a Gray ICC profile and a CMYK TIFF with a CMYK profile are converted
/// from that profile to sRGB as they are read. Returns NULL on
/// failure and writes a message through `err_out` (caller frees with
/// [`dr_string_free`]).
///
/// # Safety
/// `path` must be NULL or a valid NUL-terminated C string; `err_out` must be
/// NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_open(
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    let body = || {
        let path = unsafe { read_cstr(path, "path") }?;
        DrImage::open(&path)
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while opening image",
            ptr::null_mut(),
            body,
            boxed,
        )
    }
}

/// Builds an image from a caller-owned STRAIGHT (non-premultiplied) RGBA8
/// buffer — row 0 = top, exactly `w * h * 4` bytes — copying it. The
/// in-memory twin of [`dr_image_open`], for pixels this library cannot decode
/// itself (a HEIC still or a vector page the host decoded). Returns NULL on
/// a NULL buffer, a zero dimension, or a size past the 100 MP cap.
///
/// # Safety
/// `src` must be NULL or a valid pointer to at least `w * h * 4` readable
/// bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_image_from_rgba(src: *const u8, w: u32, h: u32) -> *mut DrImage {
    // The dimensions ARE the buffer's length here (no image-side size to
    // check them against), so they are bounded before a slice is built from
    // them, exactly as dr_doc_set_layer_content bounds its own.
    if src.is_null() || w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return ptr::null_mut();
    }
    produce_op(|| {
        // Length computed from the same dimensions that size the image, with
        // checked arithmetic — never a separate length argument.
        let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let src = unsafe { std::slice::from_raw_parts(src, len) };
        DrImage::from_rgba(w, h, src.to_vec())
    })
}

/// Frees an image. NULL is a safe no-op.
///
/// # Safety
/// `img` must be NULL or a pointer previously returned by this library that
/// has not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn dr_image_free(img: *mut DrImage) {
    if img.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(img) })));
}

/// Image width in pixels; 0 if `img` is NULL.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_width(img: *const DrImage) -> u32 {
    if img.is_null() {
        return 0;
    }
    let image = unsafe { &*img };
    catch_unwind(AssertUnwindSafe(|| image.pixels.width())).unwrap_or(0)
}

/// Image height in pixels; 0 if `img` is NULL.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_height(img: *const DrImage) -> u32 {
    if img.is_null() {
        return 0;
    }
    let image = unsafe { &*img };
    catch_unwind(AssertUnwindSafe(|| image.pixels.height())).unwrap_or(0)
}

/// Borrowed pointer to `width * height * 4` bytes of non-premultiplied RGBA8,
/// valid until the image is freed. NULL if `img` is NULL.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_pixels_rgba(img: *const DrImage) -> *const u8 {
    if img.is_null() {
        return ptr::null();
    }
    let image = unsafe { &*img };
    catch_unwind(AssertUnwindSafe(|| image.pixels.as_raw().as_ptr())).unwrap_or(ptr::null())
}

/// Rotates 90 degrees clockwise.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_rotate90(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops::rotate90(&i.pixels),
            })
        })
    }
}

/// Rotates 180 degrees.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_rotate180(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops::rotate180(&i.pixels),
            })
        })
    }
}

/// Rotates 90 degrees counter-clockwise.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_rotate270(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops::rotate270(&i.pixels),
            })
        })
    }
}

/// Mirrors left-right.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_flip_horizontal(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops::flip_horizontal(&i.pixels),
            })
        })
    }
}

/// Mirrors top-bottom.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_flip_vertical(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops::flip_vertical(&i.pixels),
            })
        })
    }
}

/// Crops to the given rect; NULL if the rect is empty or out of bounds.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_crop(
    img: *const DrImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            ops::crop(&i.pixels, x, y, w, h).map(|pixels| DrImage { pixels })
        })
    }
}

/// Resizes to `w` x `h`; NULL if a dimension is zero, the target is over
/// 100000000 pixels, or the filter value is unknown.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_resize(
    img: *const DrImage,
    w: u32,
    h: u32,
    filter: c_int,
) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            ops::resize(&i.pixels, w, h, filter_from_c(filter)?).map(|pixels| DrImage { pixels })
        })
    }
}

/// Applies brightness, contrast, then saturation (each clamped to [-1, 1],
/// 0 = identity). Alpha untouched.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_adjust(
    img: *const DrImage,
    brightness: f32,
    contrast: f32,
    saturation: f32,
) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops::adjust(&i.pixels, brightness, contrast, saturation),
            })
        })
    }
}

/// Converts to grayscale (r = g = b = luma), alpha untouched.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_grayscale(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops::grayscale(&i.pixels),
            })
        })
    }
}

/// Inverts the colour channels: RGB → 255 − c on the encoded bytes; straight
/// alpha, and latent RGB under zero alpha, are kept. No ICC conversion.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_invert(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops::invert(&i.pixels),
            })
        })
    }
}

/// Applies the standard sepia matrix, alpha untouched.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_sepia(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops::sepia(&i.pixels),
            })
        })
    }
}

/// Multiplies each pixel's alpha by a full-frame u8 coverage mask (`mask`,
/// `w * h` bytes, row-major, one byte per pixel — the selection convention:
/// 0 hides, 255 keeps, intermediate values scale proportionally). Color
/// bytes pass through, except where the scaled alpha lands on 0, which
/// clears the pixel to transparent black; full-coverage (255) pixels pass
/// through byte-for-byte, an already transparent pixel's latent color
/// included. Returns NULL if `img` or `mask` is NULL or on a dimension
/// mismatch.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`; `mask` must be
/// NULL or a valid pointer to at least `w * h` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_image_apply_mask(
    img: *const DrImage,
    mask: *const u8,
    w: u32,
    h: u32,
) -> *mut DrImage {
    if img.is_null() || mask.is_null() {
        return ptr::null_mut();
    }
    let image = unsafe { &*img };
    produce_op(|| {
        // Validate against the image dimensions before touching `mask`, so
        // the raw read below is bounded by the image's own size.
        if w != image.pixels.width() || h != image.pixels.height() {
            return None;
        }
        let len = (w as usize).checked_mul(h as usize)?;
        let mask = unsafe { std::slice::from_raw_parts(mask, len) };
        ops::apply_mask(&image.pixels, mask).map(|pixels| DrImage { pixels })
    })
}

/// Unsharp-mask sharpen with `amount` clamped to (0, 5]; NULL if `amount` is
/// not finite or not positive.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_sharpen(img: *const DrImage, amount: f32) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            ops::sharpen(&i.pixels, amount).map(|pixels| DrImage { pixels })
        })
    }
}

/// Encodes the image to `path`. `jpeg_quality` (clamped to 1-100) applies to
/// JPEG only; JPEG output is composited over white. Returns false on failure
/// and writes a message through `err_out`. Every core file writer publishes
/// atomically and durably: temp file, F_BARRIERFSYNC, rename, then a
/// best-effort F_FULLFSYNC of the parent directory.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`; `path` must be
/// NULL or a valid NUL-terminated C string; `err_out` must be NULL or a valid
/// pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_save(
    img: *const DrImage,
    path: *const c_char,
    format: c_int,
    jpeg_quality: u8,
    err_out: *mut *mut c_char,
) -> bool {
    let body = || {
        if img.is_null() {
            return Err("image is NULL".to_string());
        }
        let image = unsafe { &*img };
        let path = unsafe { read_cstr(path, "path") }?;
        let format =
            Format::from_c(format).ok_or_else(|| format!("unknown format value {format}"))?;
        image.save(&path, format, jpeg_quality)
    };
    unsafe { fallible_op(err_out, "panic while saving image", false, body, |()| true) }
}

/// Frees strings returned via `err_out` parameters. NULL is a safe no-op.
///
/// # Safety
/// `s` must be NULL or a string pointer previously produced by this library
/// that has not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn dr_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { CString::from_raw(s) })));
}
