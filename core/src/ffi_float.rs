//! The 32-bit float document across the FFI (Phase 11F): the mode query, the
//! tone map (read, table and switch), the refusal query, the display and
//! conversion paths, the extended-range (EDR) view and the synchronous
//! extended-range sample readout (the float histogram and scoped readouts go
//! through `ffi_scoped_statistics`).
//!
//! The conventions are the surface's: NULL tolerance first, `catch_unwind`
//! through the shared `ffi_util` helpers, `Option` for a domain refusal and
//! `Result` for an error reported through `err_out`. `float_document`,
//! `float_stats` and `float_io` own every decision this file exports.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::ffi_util::{borrow_cstr_bounded, fallible_op, handle_get, required_handle};
use crate::float_document::{float_operators_json, ToneMap};
use crate::float_view::FloatView;
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::DrImage;

/// The largest tone map description this surface accepts.
const TONE_MAP_JSON_MAX: usize = 4096;

/// True when `document` is a 32-bit float document; false for NULL.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_is_float(document: *const PrecisionDocument) -> bool {
    unsafe { handle_get(document, false, PrecisionDocument::is_float) }
}

/// The float mode's operator table as owned JSON — what it evaluates, what it
/// refuses and why, and the tone maps it offers. Free it with
/// `dr_string_free`; NULL only on an allocation failure.
///
/// # Safety
/// The returned pointer is owned by the caller.
#[no_mangle]
pub unsafe extern "C" fn dr_float_operators() -> *mut c_char {
    match std::panic::catch_unwind(|| CString::new(float_operators_json()).ok()) {
        Ok(Some(value)) => value.into_raw(),
        _ => ptr::null_mut(),
    }
}

/// The document's tone map as owned JSON; NULL for NULL and for a document
/// that is not a float document. Free a non-NULL result with `dr_string_free`.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_tone_map(
    document: *const PrecisionDocument,
) -> *mut c_char {
    unsafe {
        handle_get(document, ptr::null_mut(), |d| {
            d.tone_map()
                .and_then(|map| CString::new(map.to_json()).ok())
                .map_or(ptr::null_mut(), CString::into_raw)
        })
    }
}

/// The document under the tone map `json` describes. NULL WITHOUT an error
/// when the map is unchanged (the core's no-op rule); NULL WITH one for a
/// malformed description, an unknown operator or field, and for a document
/// that is not a float document.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `json` must be a valid NUL-terminated C string; `err_out` must be NULL or
/// a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_tone_map(
    document: *const PrecisionDocument,
    json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "setting the tone map",
            ptr::null_mut(),
            || {
                let text = borrow_cstr_bounded(json, "tone map", TONE_MAP_JSON_MAX)?;
                let map = ToneMap::parse(text)?;
                required_handle(document)?.with_tone_map(map)
            },
            |result| result.map_or(ptr::null_mut(), crate::ffi_util::boxed),
        )
    }
}

/// The first operator in this document the float mode cannot evaluate, as an
/// owned sentence; NULL when there is nothing to refuse — which is every u16
/// document, answered without a walk. Free a non-NULL result with
/// `dr_string_free`.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_float_refusal(
    document: *const PrecisionDocument,
) -> *mut c_char {
    unsafe {
        handle_get(document, ptr::null_mut(), |d| {
            d.float_refusal()
                .and_then(|reason| CString::new(reason).ok())
                .map_or(ptr::null_mut(), CString::into_raw)
        })
    }
}

/// The float projection `image` as display RGBA8 through `document`'s own
/// tone map, freed by `dr_image_free`. This is the ONLY way a float sample
/// becomes display bytes; `dr_precision_image_preview_rgba8` refuses F32
/// storage so a clamped preview can never be shown instead.
///
/// # Safety
/// `document`, `image` and `control` must be NULL or valid pointers to live
/// handles; `control` is single-use; `err_out` must be NULL or a valid
/// pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_display_rgba8(
    document: *const PrecisionDocument,
    image: *const PrecisionImage,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "creating a tone-mapped display image",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let image = required_handle(image)?;
                Ok(DrImage {
                    pixels: document.display_rgba8(
                        image,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                })
            },
            crate::ffi_util::boxed,
        )
    }
}

/// The float projection `image` as display RGBA8 under a VIEW: the tone map
/// `tone_map_json` describes (NULL = `document`'s own) and `exposure_stops`
/// of exposure, `gain = 2^stops` on RGB before the map, alpha untouched;
/// freed by `dr_image_free`. `(NULL, 0.0)` is byte for byte
/// `dr_precision_doc_display_rgba8`. Changes nothing in `document`. Refuses
/// a malformed map, a non-finite exposure or one outside ±20 stops, and
/// everything `dr_precision_doc_display_rgba8` refuses.
///
/// # Safety
/// `document`, `image` and `control` must be NULL or valid pointers to live
/// handles; `tone_map_json` must be NULL or a valid NUL-terminated C string;
/// `control` is single-use; `err_out` must be NULL or a valid pointer to
/// writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_display_rgba8_view(
    document: *const PrecisionDocument,
    image: *const PrecisionImage,
    tone_map_json: *const c_char,
    exposure_stops: f64,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "creating a tone-mapped display image",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let image = required_handle(image)?;
                let map = if tone_map_json.is_null() {
                    None
                } else {
                    Some(borrow_cstr_bounded(
                        tone_map_json,
                        "tone map",
                        TONE_MAP_JSON_MAX,
                    )?)
                };
                let view = FloatView::parse(map, exposure_stops)?;
                Ok(DrImage {
                    pixels: document.display_rgba8_view(
                        image,
                        view,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                })
            },
            crate::ffi_util::boxed,
        )
    }
}

/// Float layer `image`'s Layers-panel thumbnail as RGBA8 through
/// `document`'s own tone map, freed by `dr_image_free`; no ICC. Aspect-fit
/// with the longest side `max_side`, NEVER upscaled. Below 16x the thumbnail
/// per axis it is byte for byte `dr_precision_doc_display_rgba8` followed by
/// `dr_image_resize` (FILTER_BILINEAR); a larger image is box-reduced out of
/// its tiles through the tone map, resized at u16 and quantized once,
/// measured within 2/255 of that pair, with no whole-layer copy. Refuses a
/// document that is not a float document, an image that is not
/// rgba32_float, scratch past `limit` and a cancel.
///
/// # Safety
/// `document`, `image` and `control` must be NULL or valid pointers to live
/// handles; `control` is single-use; `err_out` must be NULL or a valid
/// pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_display_thumbnail_rgba8(
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
            "creating a tone-mapped layer thumbnail",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let image = required_handle(image)?;
                Ok(DrImage {
                    pixels: document.display_thumbnail_rgba8(
                        image,
                        max_side,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                })
            },
            crate::ffi_util::boxed,
        )
    }
}

/// The deliberate conversion of a float document to the u16 mode through its
/// own tone map; the result is one raster and evaluates in linear light.
///
/// # Safety
/// `document` and `control` must be NULL or valid pointers to live handles;
/// `control` is single-use; `err_out` must be NULL or a valid pointer to
/// writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_float_to_u16(
    document: *const PrecisionDocument,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting a 32-bit float document to 16-bit",
            ptr::null_mut(),
            || {
                required_handle(document)?.float_to_u16(
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            crate::ffi_util::boxed,
        )
    }
}

/// The extended-range sample readout as owned JSON; free with
/// `dr_string_free`.
///
/// # Safety
/// `image` must be NULL or a valid pointer to a live `DrPrecisionImage`;
/// `err_out` must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_float_sample(
    image: *const PrecisionImage,
    x: i32,
    y: i32,
    reach: u32,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading a 32-bit float sample",
            ptr::null_mut(),
            || {
                Ok(required_handle(image)?
                    .float_sample(x, y, reach)?
                    .to_string())
            },
            |value| CString::new(value).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// The float projection `image` as the EXTENDED-RANGE view an EDR canvas
/// shows: `display_encoding`'s rule X at `exposure_stops`
/// (`gain = 2^stops`, the same range as the SDR view), no tone map, written
/// into `out` as RGBA binary16 little-endian, premultiplied, linear in the
/// document primaries. `out_len` must equal width·height·8 of `image`.
/// `peak_out` (nullable) receives the largest RGB component written. Returns
/// false and sets `err_out` for a document that is not a float document, an
/// image that is not rgba32_float, a wrong length, a bad exposure, scratch
/// past `limit` beside `other_retained` and a cancel. Changes nothing in
/// `document`.
///
/// # Safety
/// `document`, `image` and `control` must be NULL or valid pointers to live
/// handles; `out` NULL or writable for `out_len` bytes; `peak_out` NULL or
/// a valid pointer to a writable `float`; `control` is single-use; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_display_extended(
    document: *const PrecisionDocument,
    image: *const PrecisionImage,
    exposure_stops: f64,
    out: *mut u8,
    out_len: usize,
    peak_out: *mut f32,
    limit: u64,
    other_retained: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "creating an extended-range display image",
            false,
            || {
                let document = required_handle(document)?;
                let image = required_handle(image)?;
                if out.is_null() {
                    return Err("the extended-range display buffer is NULL".into());
                }
                let (width, height) = image.dimensions();
                let expected = (width as u64)
                    .checked_mul(u64::from(height))
                    // RGBA binary16: four 2-byte samples a pixel (the format).
                    .and_then(|v| v.checked_mul(4 * 2))
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or("extended display working-memory calculation overflow")?;
                if out_len != expected {
                    return Err(
                        "the extended-range display buffer must hold width × height × 8 bytes"
                            .into(),
                    );
                }
                let buffer = std::slice::from_raw_parts_mut(out, expected);
                document.display_extended(
                    image,
                    exposure_stops,
                    buffer,
                    limit,
                    other_retained,
                    control.as_ref(),
                )
            },
            |peak| {
                if !peak_out.is_null() {
                    *peak_out = peak;
                }
                true
            },
        )
    }
}
