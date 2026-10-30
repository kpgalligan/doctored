//! The Duotone document across the FFI (#547, `designs/duotone-mode.md`):
//! the options normalized and read, Image ▸ Mode ▸ Duotone…'s plan and edit
//! (set or replace the description) and the view; the render through the
//! inks and its plan, Duotone → RGB and its plan, and a Duotone Options
//! (`.ado`) file read and written; the thumbnail through the inks; Duotone →
//! Grayscale and Duotone → Bitmap and their plans.
//!
//! The conventions are the surface's: NULL tolerance first, `catch_unwind`
//! through the shared `ffi_util` helpers, `Err` through `err_out`.
//! `duotone_ink`, `duotone_document`, `duotone_convert`, `duotone_gray` and
//! `duotone_files` own every decision this file exports.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::duotone_ink::DuotoneInks;
use crate::ffi_color::blob;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, read_cstr, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::DrImage;

/// The largest options JSON this surface accepts: four inks of 64-character
/// names, 13 slots and eleven overprints are well under 8 KB (a parse
/// bound, not a memory plan).
const OPTIONS_MAX: usize = 16384;

fn json_string(value: serde_json::Value) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| format!("cannot report Duotone JSON: {e}"))
}

/// `options` checked and normalized: owned JSON with the type, every ink,
/// every overprint key (null where default) and `overprint_display`. Free it
/// with `dr_string_free`; NULL with the reason when the options refuse.
///
/// # Safety
/// `options` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_duotone_options_normalize(
    options: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "normalizing Duotone options",
            ptr::null_mut(),
            || {
                let text = borrow_cstr_bounded(options, "options", OPTIONS_MAX)?;
                json_string(DuotoneInks::parse(text)?.to_json())
            },
            CString::into_raw,
        )
    }
}

/// A Duotone document's normalized options; NULL with no error for every
/// other document. Free it with `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_duotone_options(
    document: *const PrecisionDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading Duotone options",
            ptr::null_mut(),
            || {
                required_handle(document)?
                    .duotone_options()
                    .map(json_string)
                    .transpose()
            },
            |text| text.map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// Image ▸ Mode ▸ Duotone…'s plan for `options` (NULL refuses): owned JSON
/// `{"working_memory_plan_bytes","refusal","changes"}`. Free it with
/// `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `options` NULL or NUL-terminated; `err_out`
/// NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_duotone_plan(
    document: *const PrecisionDocument,
    options: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Duotone options",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let options = borrow_cstr_bounded(options, "options", OPTIONS_MAX)?;
                json_string(document.set_duotone_plan(options)?)
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ Duotone…: a new document with `options` as its Duotone
/// description, every sample shared (NULL options refuse). NULL with no
/// error when nothing changes.
///
/// # Safety
/// As [`dr_precision_doc_set_duotone_plan`].
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_duotone(
    document: *const PrecisionDocument,
    options: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "setting Duotone options",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                document.set_duotone(borrow_cstr_bounded(options, "options", OPTIONS_MAX)?)
            },
            |next| next.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// The Duotone view of `image` (a projection or layer source of
/// `document`): RGBA16 in the document's profile, alpha kept
/// (`duotone_document`). Freed with `dr_precision_image_free`; NULL with
/// the reason on refusal.
///
/// # Safety
/// `document` and `image` NULL or live; `control` NULL or a live
/// single-use token; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_duotone_view(
    document: *const PrecisionDocument,
    image: *const PrecisionImage,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "making the Duotone view",
            ptr::null_mut(),
            || {
                required_handle(document)?.duotone_view(
                    required_handle(image)?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// A Duotone layer's (or composite's) thumbnail (#547 Duotone phase 2a,
/// `duotone_document`): `image` (u16, this document's twin) fitted at u16
/// first — longest side `max_side`, never upscaled — then through the inks'
/// view, quantized once without dither; RGBA8 in the document's profile,
/// carrying no ICC. Freed with `dr_image_free`; NULL with the reason for a
/// document that is not Duotone, a non-u16 image, the working limit and a
/// cancel.
///
/// # Safety
/// `document` and `image` NULL or live; `control` NULL or a live
/// single-use token; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_duotone_thumbnail_rgba8(
    document: *const PrecisionDocument,
    image: *const PrecisionImage,
    max_side: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "making the Duotone thumbnail",
            ptr::null_mut(),
            || {
                Ok(DrImage {
                    pixels: required_handle(document)?.duotone_thumbnail_rgba8(
                        required_handle(image)?,
                        max_side,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                })
            },
            boxed,
        )
    }
}

/// An optional bounded mode-options argument: NULL is the empty object.
unsafe fn mode_options<'a>(text: *const c_char) -> Result<&'a str, String> {
    if text.is_null() {
        return Ok("");
    }
    unsafe { borrow_cstr_bounded(text, "options", OPTIONS_MAX) }
}

/// The render through the inks, planned (`duotone_convert`): owned JSON
/// `{"working_memory_plan_bytes","refusal","flatten_required","flattens"}`.
/// Free it with `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_duotone_render_plan(
    document: *const PrecisionDocument,
    flatten: bool,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning the Duotone render",
            ptr::null_mut(),
            || json_string(required_handle(document)?.duotone_render_plan(flatten)?),
            CString::into_raw,
        )
    }
}

/// The render through the inks (`duotone_convert`): a new RGB document in
/// the twin, each raster through the view; `flatten` runs Flatten Image first
/// when a record needs it. Freed with `dr_precision_doc_free`; NULL with the
/// reason on refusal.
///
/// # Safety
/// `document` NULL or live; `control` NULL or a live single-use token;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_duotone_render(
    document: *const PrecisionDocument,
    flatten: bool,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "rendering the Duotone document",
            ptr::null_mut(),
            || {
                required_handle(document)?.duotone_render(
                    flatten,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Duotone → RGB Color, planned: owned JSON (the render's keys), under
/// options `{"intent"?,"black_point_compensation"?,"flatten"?}` (NULL: the
/// defaults). Free it with `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `icc` NULL or `icc_len` readable bytes; `options`
/// NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_duotone_to_rgb_plan(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    options: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Duotone to RGB",
            ptr::null_mut(),
            || {
                let icc = blob(icc, icc_len).ok_or("RGB Color needs an RGB profile")?;
                json_string(
                    required_handle(document)?
                        .convert_duotone_to_rgb_plan(icc, mode_options(options)?)?,
                )
            },
            CString::into_raw,
        )
    }
}

/// Duotone → RGB Color (`duotone_convert`): the render, then Convert to
/// Profile from the twin into `icc`. Freed with `dr_precision_doc_free`;
/// NULL with the reason on refusal.
///
/// # Safety
/// As [`dr_precision_doc_convert_duotone_to_rgb_plan`]; `control` NULL or a
/// live single-use token.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_convert_duotone_to_rgb(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    options: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting Duotone to RGB",
            ptr::null_mut(),
            || {
                let icc = blob(icc, icc_len).ok_or("RGB Color needs an RGB profile")?;
                required_handle(document)?.convert_duotone_to_rgb(
                    icc,
                    mode_options(options)?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// The largest Bitmap options JSON: `ffi_bitmap`'s `OPTIONS_MAX` (the master
/// copy), 2 MiB, because Custom Pattern's inline tile rides in it.
const BITMAP_OPTIONS_MAX: usize = 2 << 20;

/// An optional bounded Bitmap options argument: NULL is the empty object.
unsafe fn bitmap_options<'a>(text: *const c_char) -> Result<&'a str, String> {
    if text.is_null() {
        return Ok("");
    }
    unsafe { borrow_cstr_bounded(text, "options", BITMAP_OPTIONS_MAX) }
}

/// Duotone → Grayscale, planned (`duotone_gray`): owned JSON
/// `{"working_memory_plan_bytes","refusal","flatten_required","flattens"}`.
/// Free it with `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_duotone_to_gray_plan(
    document: *const PrecisionDocument,
    flatten: bool,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Duotone to Grayscale",
            ptr::null_mut(),
            || json_string(required_handle(document)?.convert_duotone_to_gray_plan(flatten)?),
            CString::into_raw,
        )
    }
}

/// Duotone → Grayscale (`duotone_gray`): the same Gray document, each
/// raster through the inks' luminance in its own Gray profile, the inks
/// dropped; `flatten` runs Flatten Image first when a record needs it. Freed
/// with `dr_precision_doc_free`; NULL with the reason on refusal.
///
/// # Safety
/// `document` NULL or live; `control` NULL or a live single-use token;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_duotone_to_gray(
    document: *const PrecisionDocument,
    flatten: bool,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting Duotone to Grayscale",
            ptr::null_mut(),
            || {
                required_handle(document)?.convert_duotone_to_gray(
                    flatten,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Duotone → Bitmap, planned (`duotone_gray`), under Bitmap's options (NULL:
/// the defaults): owned JSON with Bitmap's plan keys, `width` and `height`
/// included. Free it with `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `options` NULL or NUL-terminated; `err_out`
/// NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_duotone_to_bitmap_plan(
    document: *const PrecisionDocument,
    options: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Duotone to Bitmap",
            ptr::null_mut(),
            || {
                json_string(
                    required_handle(document)?
                        .convert_duotone_to_bitmap_plan(bitmap_options(options)?)?,
                )
            },
            CString::into_raw,
        )
    }
}

/// Duotone → Bitmap (`duotone_gray`): the Grayscale rule, then Gray →
/// Bitmap under `options`. Freed with `dr_precision_doc_free`; NULL with the
/// reason on refusal.
///
/// # Safety
/// As [`dr_precision_doc_convert_duotone_to_bitmap_plan`]; `control` NULL or
/// a live single-use token.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_duotone_to_bitmap(
    document: *const PrecisionDocument,
    options: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting Duotone to Bitmap",
            ptr::null_mut(),
            || {
                required_handle(document)?.convert_duotone_to_bitmap(
                    bitmap_options(options)?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// A Duotone Options (`.ado`) file's description as normalized options JSON
/// (`dr_duotone_options_normalize`'s form). Free it with `dr_string_free`;
/// NULL with the reason when the file cannot be read.
///
/// # Safety
/// `path` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_duotone_read_ado(
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading a Duotone Options file",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                json_string(crate::duotone_files::read_ado(&path)?.to_json())
            },
            CString::into_raw,
        )
    }
}

/// `options` (Duotone options JSON) written to `path` as a Duotone Options
/// (`.ado`) file, atomically; false with the reason on refusal.
///
/// # Safety
/// `path` and `options` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_duotone_write_ado(
    path: *const c_char,
    options: *const c_char,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "writing a Duotone Options file",
            false,
            || {
                let path = read_cstr(path, "path")?;
                let options = borrow_cstr_bounded(options, "options", OPTIONS_MAX)?;
                crate::duotone_files::write_ado(&path, &DuotoneInks::parse(options)?)
            },
            |()| true,
        )
    }
}
