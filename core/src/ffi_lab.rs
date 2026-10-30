//! The Lab Color document across the FFI (#547 phase 1): File ▸ New ▸ Lab
//! Color, Image ▸ Mode's two conversions and their plans, and the view;
//! since phase 1b the fit-first layer thumbnail and the Lab TIFF save and
//! its plan; since phase 2d the value-only [`dr_lab_kernel_rgb`], what a
//! colour op's kernel reads for a sample (the sheets' samplers); since phase
//! 3a the flat Lab TIFF's source test and open (`lab_tiff_read`); since
//! phase 5a 8/16 Bits/Channel and its plan (`lab_depth`); since phase 6b-iii
//! the value-only [`dr_lab_neural_scratch_bytes`] (`lab_neural`).
//!
//! The conventions are the surface's: NULL tolerance first, `catch_unwind`
//! through the shared `ffi_util` helpers (the value-only export has
//! `dr_lab_to_srgb`'s shape), `Err` through `err_out`. `lab_document`,
//! `lab_convert`, `lab_rgb`, `lab_save` and `lab_adjust` own every decision
//! this file exports.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::ffi_color::blob;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, read_cstr, required_handle};
use crate::job_control::DrJobControl;
use crate::lab_convert::lab_display;
use crate::lab_rgb::lab_thumbnail_rgba8;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::DrImage;

/// The largest options or request JSON this surface accepts.
const OPTIONS_MAX: usize = 4096;

fn string(value: serde_json::Value) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| format!("cannot report Lab JSON: {e}"))
}

/// An optional bounded JSON argument: NULL is the empty object.
unsafe fn options<'a>(text: *const c_char, what: &str) -> Result<&'a str, String> {
    if text.is_null() {
        return Ok("");
    }
    unsafe { borrow_cstr_bounded(text, what, OPTIONS_MAX) }
}

/// `DR_LAB_SOURCE_*` for the file at `path`: TIFF 1 (a flat CIELab or
/// ICCLab TIFF `dr_precision_doc_open_lab` accepts), else 0 (NULL,
/// unreadable, or any other file). Header work only
/// (`lab_tiff_read::lab_source_at`, #547 Lab phase 3a).
///
/// # Safety
/// `path` must be NULL or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn dr_path_lab_source(path: *const c_char) -> i32 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "inspecting a Lab file",
            0,
            || {
                let path = read_cstr(path, "path")?;
                Ok(crate::lab_tiff_read::lab_source_at(&path)?
                    .map_or(0, |_| crate::lab_tiff_read::LabSource::Tiff as i32))
            },
            |value| value,
        )
    }
}

/// A flat Lab TIFF (CIELab or ICCLab, 8 or 16 bits) as a Lab Color document
/// (#547 Lab phase 3a); NULL on refusal with the reason in `err_out`.
///
/// # Safety
/// `path` NULL or NUL-terminated; `control` NULL or a live single-use token;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_open_lab(
    path: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening a Lab Color file",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                PrecisionDocument::open_lab(&path, limit, other_retained_bytes, control.as_ref())
            },
            boxed,
        )
    }
}

/// File ▸ New ▸ Lab Color: the request `{"depth":8|16,"fill_srgb",
/// "layer_name","ppi"}`; NULL on refusal with the reason in `err_out`.
///
/// # Safety
/// `request` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_new_lab(
    width: u32,
    height: u32,
    request: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "making a Lab Color document",
            ptr::null_mut(),
            || {
                let request = read_cstr(request, "request")?;
                PrecisionDocument::new_lab(width, height, &request, limit, other_retained_bytes)
            },
            boxed,
        )
    }
}

/// Image ▸ Mode ▸ Lab Color's plan: `{"working_memory_plan_bytes",
/// "refusal","flatten_required","flattens"}` for the options
/// `{"flatten"?,"depth"?}`.
///
/// # Safety
/// `document` NULL or live; `options` NULL or NUL-terminated; `err_out` NULL
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_to_lab_plan(
    document: *const PrecisionDocument,
    options_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Lab Color",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                string(document.convert_to_lab_plan(options(options_json, "options")?)?)
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ Lab Color: a new Lab document (`lab_convert`).
///
/// # Safety
/// As [`dr_precision_doc_convert_to_lab_plan`]; `control` NULL or a live
/// single-use token.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_to_lab(
    document: *const PrecisionDocument,
    options_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting to Lab Color",
            ptr::null_mut(),
            || {
                required_handle(document)?.convert_to_lab(
                    options(options_json, "options")?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Image ▸ Mode ▸ RGB Color's plan on a Lab document; `icc` is the
/// destination RGB profile (required), the options `{"intent"?,
/// "black_point_compensation"?}`.
///
/// # Safety
/// `document` NULL or live; `icc` NULL or valid for `icc_len` bytes;
/// `options` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_lab_to_rgb_plan(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    options_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning RGB Color",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let icc = blob(icc, icc_len).ok_or("RGB Color needs an RGB profile")?;
                string(document.convert_lab_to_rgb_plan(icc, options(options_json, "options")?)?)
            },
            CString::into_raw,
        )
    }
}

/// The Lab Color document's view: `image` (a Lab projection or layer
/// source, its profile the twin) as Display P3 RGBA16 tagged Display P3,
/// for the host to narrow (`lab_convert::lab_display`). Freed with
/// `dr_precision_image_free`; NULL with the reason on refusal.
///
/// # Safety
/// `image` NULL or live; `control` NULL or a live single-use token;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_lab_display(
    image: *const PrecisionImage,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "making the Lab Color view",
            ptr::null_mut(),
            || {
                lab_display(
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

/// Image ▸ Mode ▸ RGB Color on a Lab document: the twin into `icc`, the Lab
/// policy dropped.
///
/// # Safety
/// As [`dr_precision_doc_convert_lab_to_rgb_plan`]; `control` NULL or a live
/// single-use token.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_convert_lab_to_rgb(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    options_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting to RGB",
            ptr::null_mut(),
            || {
                let icc = blob(icc, icc_len).ok_or("RGB Color needs an RGB profile")?;
                required_handle(document)?.convert_lab_to_rgb(
                    icc,
                    options(options_json, "options")?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Image ▸ Mode ▸ 8/16 Bits/Channel's plan on a Lab document (#547 Lab phase
/// 5a, `lab_depth`): `{"working_memory_plan_bytes","refusal","changes"}` for
/// `bits` 8 or 16. Free with `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_lab_depth_plan(
    document: *const PrecisionDocument,
    bits: u32,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a Lab Color depth change",
            ptr::null_mut(),
            || string(required_handle(document)?.lab_depth_plan(bits)?),
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ 8/16 Bits/Channel on a Lab document (`lab_depth`): a new
/// Lab document at `bits`; NULL with no error when it changes nothing, NULL
/// with the reason on refusal.
///
/// # Safety
/// `document` NULL or live; `control` NULL or a live single-use token;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_lab_depth_edit(
    document: *const PrecisionDocument,
    bits: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "changing a Lab Color document's depth",
            ptr::null_mut(),
            || {
                required_handle(document)?.lab_depth_edit(
                    bits,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// A Lab layer's Layers-panel thumbnail (#547 phase 1b, `lab_rgb`): `image`
/// (u16, its profile the twin) fitted at u16 first — longest side
/// `max_side`, never upscaled — then converted into Display P3 and
/// quantized once without dither. Freed with `dr_image_free`; carries no
/// ICC (the host tags it Display P3). NULL with the reason for an image that
/// is not Lab or not u16, a working-limit overflow and a cancel.
///
/// # Safety
/// `image` NULL or live; `control` NULL or a live single-use token;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_lab_thumbnail_rgba8(
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
            "making the Lab Color thumbnail",
            ptr::null_mut(),
            || {
                Ok(DrImage {
                    pixels: lab_thumbnail_rgba8(
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

/// The Lab TIFF save's plan (`lab_save`): `{"working_memory_plan_bytes",
/// "refusal"}` for `request` `{"format":"tiff","strip_metadata"?}` over
/// `projection` (NULL: the save renders the composite itself).
///
/// # Safety
/// `document`/`projection` NULL or live; `request` NULL or NUL-terminated;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_save_lab_plan(
    document: *const PrecisionDocument,
    projection: *const PrecisionImage,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a Lab TIFF save",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let request = borrow_cstr_bounded(request, "request", OPTIONS_MAX)?;
                string(document.save_lab_plan(projection.as_ref(), request)?)
            },
            CString::into_raw,
        )
    }
}

/// Writes the composite (`projection`, else a fresh render) to `path` as a
/// TIFF 6.0 CIELab file at the document's depth, atomically (`lab_save`).
/// False with the reason on refusal.
///
/// # Safety
/// `document`/`projection` NULL or live; `path`/`request` NULL or
/// NUL-terminated; `control` NULL or a live single-use token; `err_out`
/// NULL or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_save_lab(
    document: *const PrecisionDocument,
    projection: *const PrecisionImage,
    path: *const c_char,
    request: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "saving a Lab TIFF",
            false,
            || {
                let document = required_handle(document)?;
                let path = read_cstr(path, "path")?;
                document.save_lab(
                    projection.as_ref(),
                    &path,
                    borrow_cstr_bounded(request, "request", OPTIONS_MAX)?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |()| true,
        )
    }
}

/// One CIE L\*a\*b\* colour (D50) as the ENCODED RGB of the space a Lab
/// document's `op` runs its Color form in (ProPhoto RGB for hue_saturation,
/// color_balance, photo_filter, shadows_highlights, replace_color; sRGB for
/// color_lookup, gradient_map), each clamped into [0, 1]: what that kernel
/// reads for the sample (`lab_adjust::kernel_rgb`, phase 2d). Writes the 3
/// components to `rgb_out`. false on a NULL pointer, a non-finite component
/// or an op without the Color form. Value-only: no handle.
///
/// # Safety
/// `op` must be NULL or a NUL-terminated string; `lab` must be NULL or
/// readable for 3 `float`; `rgb_out` must be NULL or writable for 3 `float`.
#[no_mangle]
pub unsafe extern "C" fn dr_lab_kernel_rgb(
    op: *const c_char,
    lab: *const f32,
    rgb_out: *mut f32,
) -> bool {
    if op.is_null() || lab.is_null() || rgb_out.is_null() {
        return false;
    }
    std::panic::catch_unwind(|| {
        let Ok(op) = (unsafe { read_cstr(op, "op") }) else {
            return false;
        };
        let input = unsafe { std::slice::from_raw_parts(lab, 3) };
        let Some(rgb) = crate::lab_adjust::kernel_rgb(&op, [input[0], input[1], input[2]]) else {
            return false;
        };
        unsafe { ptr::copy_nonoverlapping(rgb.as_ptr(), rgb_out, rgb.len()) };
        true
    })
    .unwrap_or(false)
}

/// The bytes a neural run on a Lab document reserves beside its model's own
/// plan (#547 Lab phase 6b-iii, `lab_neural::neural_scratch_bytes`): the
/// Display P3 view of an `in_width` × `in_height` input, the Lab image of an
/// `out_width` × `out_height` result at 8 bytes a pixel, the way back's
/// tables and bookkeeping. `UINT64_MAX` on error. Value-only: no handle.
///
/// # Safety
/// Takes no pointer; `unsafe` only for the surface's uniform signature.
#[no_mangle]
pub unsafe extern "C" fn dr_lab_neural_scratch_bytes(
    in_width: u32,
    in_height: u32,
    out_width: u32,
    out_height: u32,
) -> u64 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "planning the Lab neural scratch",
            u64::MAX,
            || {
                crate::lab_neural::neural_scratch_bytes(
                    (in_width, in_height),
                    (out_width, out_height),
                )
            },
            |bytes| bytes,
        )
    }
}
