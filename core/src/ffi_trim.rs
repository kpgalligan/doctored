//! Image ▸ Trim's kept rectangle across the FFI: the byte composite
//! (`DrImage`) and the typed projection (`DrPrecisionImage`, u16 or float),
//! both through the one scan in `trim_bounds`; and (#1 phase 4b) a CMYK
//! document's composite ink (`cmyk_trim`).

use std::ffi::{c_char, c_int};

use crate::ffi_util::{fallible_op, handle_get, img_get, required_handle};
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{PrecisionImage, PrecisionStorage};
use crate::trim_bounds::trim_rect;
use crate::DrImage;

/// Writes `x, y, w, h` into the caller's four slots.
///
/// # Safety
/// `rect_out` must be writable for four `u32`.
unsafe fn write_rect(rect_out: *mut u32, rect: (u32, u32, u32, u32)) {
    let out = unsafe { std::slice::from_raw_parts_mut(rect_out, 4) };
    out.copy_from_slice(&[rect.0, rect.1, rect.2, rect.3]);
}

/// A negative C enum value is no basis at all; `trim_rect` refuses any
/// value it does not know.
fn basis_value(basis: c_int) -> Option<u32> {
    u32::try_from(basis).ok()
}

/// The rectangle Image ▸ Trim keeps on an RGBA8 image: true with `x, y, w, h`
/// written to `rect_out`; false (rect_out untouched) for NULL input, an
/// unknown basis, or an image whose every pixel is trimmable. Basis 0 trims
/// alpha-0 pixels, 1 pixels equal to the top-left pixel, 2 pixels equal to
/// the bottom-right one; `edges` bits 1 top, 2 bottom, 4 left, 8 right — an
/// unchecked edge never moves.
///
/// # Safety
/// `img` must be NULL or a live `DrImage`; `rect_out` NULL or writable for
/// four `u32`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_trim_rect(
    img: *const DrImage,
    basis: c_int,
    edges: u32,
    rect_out: *mut u32,
) -> bool {
    if rect_out.is_null() {
        return false;
    }
    let rect = unsafe {
        img_get(img, None, |i| {
            let (w, h) = i.pixels.dimensions();
            let raw = i.pixels.as_raw();
            Some(trim_rect(
                w,
                h,
                |p| raw[p * 4 + 3] == 0,
                |p| [raw[p * 4], raw[p * 4 + 1], raw[p * 4 + 2], raw[p * 4 + 3]],
                basis_value(basis)?,
                edges,
            ))
        })
    };
    match rect {
        Some(rect) => {
            unsafe { write_rect(rect_out, rect) };
            true
        }
        None => false,
    }
}

/// The typed twin of `dr_image_trim_rect` over a u16 or float projection.
/// Float samples compare by bit pattern; a float alpha is "transparent" when
/// it equals zero.
///
/// # Safety
/// `image` must be NULL or a live `DrPrecisionImage`; `rect_out` NULL or
/// writable for four `u32`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_trim_rect(
    image: *const PrecisionImage,
    basis: c_int,
    edges: u32,
    rect_out: *mut u32,
) -> bool {
    if rect_out.is_null() {
        return false;
    }
    let rect = unsafe {
        handle_get(image, None, |i| {
            let basis = basis_value(basis)?;
            let (w, h) = i.dimensions();
            match i.storage() {
                PrecisionStorage::Rgba16Unorm => {
                    let px = i.transient_rgba16().ok()?;
                    trim_rect(w, h, |p| px[p][3] == 0, |p| px[p], basis, edges)
                }
                PrecisionStorage::Rgba32Float => {
                    let px = i.transient_rgba32().ok()?;
                    trim_rect(
                        w,
                        h,
                        |p| px[p][3] == 0.0,
                        |p| px[p].map(f32::to_bits),
                        basis,
                        edges,
                    )
                }
            }
        })
    };
    match rect {
        Some(rect) => {
            unsafe { write_rect(rect_out, rect) };
            true
        }
        None => false,
    }
}

/// The rectangle Image ▸ Trim keeps on a CMYK document (#1 phase 4b): the
/// composite ink's, `dr_image_trim_rect`'s bases and edge bits (a corner
/// basis compares all five codes). True with `x, y, w, h` written to
/// `rect_out`; false with `*err_out` NULL when every pixel is trimmable (or
/// the basis is unknown); false with `*err_out` set when refused (NULL
/// input, an RGB document, a refused admission against `limit` beside
/// `other_retained_bytes`).
///
/// # Safety
/// `document` must be NULL or a live `DrPrecisionDocument`; `rect_out` NULL
/// or writable for four `u32`; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_trim_rect(
    document: *const PrecisionDocument,
    basis: c_int,
    edges: u32,
    limit: u64,
    other_retained_bytes: u64,
    rect_out: *mut u32,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "finding the CMYK trim rectangle",
            false,
            || {
                if rect_out.is_null() {
                    return Err("the trim rectangle output is NULL".into());
                }
                let doc = required_handle(document)?;
                let Some(basis) = basis_value(basis) else {
                    return Ok(None);
                };
                doc.cmyk_trim_rect(basis, edges, limit, other_retained_bytes, JobContext::NONE)
            },
            |rect| match rect {
                Some(rect) => {
                    write_rect(rect_out, rect);
                    true
                }
                None => false,
            },
        )
    }
}
