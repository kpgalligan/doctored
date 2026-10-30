//! The CMYK document across the FFI (#547 phase 1): the file and mode
//! queries, the profile, the capability table, Image ▸ Mode's two
//! conversions and their plans, the ink readout, the Channels panel's ink
//! planes, the Layers panel's thumbnail and identity, and the CMYK saves.
//!
//! The conventions are the surface's: NULL tolerance first, `catch_unwind`
//! through the shared `ffi_util` helpers, `Err` through `err_out`.
//! `cmyk_document`, `cmyk_render`, `cmyk_convert` and `cmyk_io` own every
//! decision this file exports.

use std::ffi::{c_char, CString};
use std::ptr;

use serde_json::json;

use crate::ffi_color::blob;
use crate::ffi_util::{
    borrow_cstr_bounded, boxed, fallible_op, handle_get, read_cstr, required_handle,
};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::DrImage;

/// The largest options or request JSON this surface accepts.
const OPTIONS_MAX: usize = 4096;

fn string(value: serde_json::Value) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| format!("cannot report CMYK JSON: {e}"))
}

/// An optional bounded JSON argument: NULL is the empty object.
unsafe fn options<'a>(text: *const c_char, what: &str) -> Result<&'a str, String> {
    if text.is_null() {
        return Ok("");
    }
    unsafe { borrow_cstr_bounded(text, what, OPTIONS_MAX) }
}

/// `DR_CMYK_SOURCE_*` for the file at `path`: TIFF 1, JPEG 2, else 0
/// (NULL, unreadable, not CMYK, a PSD — the PSD importer's question —
/// refused by the CMYK gate, or untagged while no working CMYK profile is
/// set). Header work only: the file is never read whole
/// (`cmyk_source::cmyk_source_at`).
///
/// # Safety
/// `path` must be NULL or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn dr_path_cmyk_source(path: *const c_char) -> i32 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "inspecting a CMYK file",
            0,
            || {
                let path = read_cstr(path, "path")?;
                Ok(crate::cmyk_source::cmyk_source_at(&path)?.map_or(0, |source| source as i32))
            },
            |value| value,
        )
    }
}

/// True when `document` is a CMYK document; false for NULL.
///
/// # Safety
/// `document` must be NULL or a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_is_cmyk(document: *const PrecisionDocument) -> bool {
    unsafe { handle_get(document, false, PrecisionDocument::is_cmyk) }
}

/// `{"profile","depth","storage","view_profile"}` as owned JSON; NULL for
/// NULL and for an RGB document. Free with `dr_string_free`.
///
/// # Safety
/// `document` must be NULL or a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_info(
    document: *const PrecisionDocument,
) -> *mut c_char {
    unsafe {
        handle_get(document, ptr::null_mut(), |d| {
            d.cmyk_policy()
                .and_then(|policy| {
                    CString::new(
                        json!({"profile":policy.name(),"depth":policy.depth().bits(),
                            "storage":crate::cmyk_document::STORAGE_CMYK,
                            "view_profile":d.profile().name()})
                        .to_string(),
                    )
                    .ok()
                })
                .map_or(ptr::null_mut(), CString::into_raw)
        })
    }
}

/// The CMYK profile's bytes, BORROWED from the document (valid while it
/// lives), with their count in `*length`; NULL (and 0) for NULL and for an
/// RGB document.
///
/// # Safety
/// `document` must be NULL or a live `DrPrecisionDocument`; `length` NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_profile(
    document: *const PrecisionDocument,
    length: *mut usize,
) -> *const u8 {
    let (pointer, count) = unsafe {
        handle_get(document, (ptr::null(), 0), |d| {
            d.cmyk_policy().map_or((ptr::null(), 0), |policy| {
                let bytes = policy.profile_bytes();
                (bytes.as_ptr(), bytes.len())
            })
        })
    };
    if let Some(slot) = unsafe { length.as_mut() } {
        *slot = count;
    }
    pointer
}

/// Image ▸ Mode ▸ CMYK Color's plan: `{"working_memory_plan_bytes",
/// "refusal","flatten_required"}`. `icc` NULL is the working CMYK profile.
///
/// # Safety
/// `document` NULL or live; `icc` NULL or valid for `icc_len` bytes;
/// `options` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_to_cmyk_plan(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    options_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning CMYK Color",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                string(
                    document.convert_to_cmyk_plan(
                        blob(icc, icc_len),
                        options(options_json, "options")?,
                    )?,
                )
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ CMYK Color: a new CMYK document (`cmyk_convert`).
///
/// # Safety
/// As [`dr_precision_doc_convert_to_cmyk_plan`]; `control` NULL or a live
/// single-use token.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_convert_to_cmyk(
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
            "converting to CMYK",
            ptr::null_mut(),
            || {
                required_handle(document)?.convert_to_cmyk(
                    blob(icc, icc_len),
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

/// Image ▸ Mode ▸ RGB Color's plan; `icc` NULL is the built-in sRGB.
///
/// # Safety
/// As [`dr_precision_doc_convert_to_cmyk_plan`].
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_cmyk_to_rgb_plan(
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
                string(document.convert_cmyk_to_rgb_plan(
                    blob(icc, icc_len),
                    options(options_json, "options")?,
                )?)
            },
            CString::into_raw,
        )
    }
}

/// Image ▸ Mode ▸ RGB Color: a new u16 RGB document, layers kept.
///
/// # Safety
/// As [`dr_precision_doc_convert_to_cmyk`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_convert_cmyk_to_rgb(
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
                required_handle(document)?.convert_cmyk_to_rgb(
                    blob(icc, icc_len),
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

/// The composite's ink at `(x, y)` over a `reach` square (0–2) as owned
/// JSON: `{"x","y","reach","ink_percent","ink16","alpha16",
/// "total_ink_percent",...}`; the stack's masks are admitted against
/// `limit` beside `other_retained_bytes` first.
///
/// # Safety
/// `document` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_sample(
    document: *const PrecisionDocument,
    x: i32,
    y: i32,
    reach: u32,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "sampling CMYK ink",
            ptr::null_mut(),
            || {
                string(required_handle(document)?.cmyk_sample(
                    x,
                    y,
                    reach,
                    limit,
                    other_retained_bytes,
                )?)
            },
            CString::into_raw,
        )
    }
}

/// Ink plane `plane` (0–3 = C, M, Y, K) of the composite on paper as opaque
/// gray RGBA8, ink dark, fitted to `max_side` (0 = full size).
///
/// # Safety
/// `document` NULL or live; `control` NULL or a live single-use token;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_plane_view(
    document: *const PrecisionDocument,
    plane: u32,
    max_side: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering a CMYK ink plane",
            ptr::null_mut(),
            || {
                required_handle(document)?.cmyk_plane_view(
                    plane,
                    max_side,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Record `index`'s Layers-panel thumbnail through the CMYK view, RGBA8,
/// fitted to `max_side`.
///
/// # Safety
/// As [`dr_precision_doc_cmyk_plane_view`].
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_layer_thumbnail_rgba8(
    document: *const PrecisionDocument,
    index: usize,
    max_side: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering a CMYK layer thumbnail",
            ptr::null_mut(),
            || {
                required_handle(document)?.cmyk_layer_thumbnail_rgba8(
                    index,
                    max_side,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Record `index`'s CMYK pixel identity (stable while the pixels are); 0 for
/// NULL, an RGB document or an index that holds no CMYK raster.
///
/// # Safety
/// `document` NULL or live.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_layer_identity(
    document: *const PrecisionDocument,
    index: usize,
) -> u64 {
    unsafe {
        handle_get(document, 0, |d| {
            d.layer(index)
                .and_then(|layer| layer.cmyk_source())
                .map_or(0, |image| image.identity_key())
        })
    }
}

/// The CMYK save's plan for `request` (`{"format":"tiff|psd|
/// jpeg_intermediate","embed_profile","strip_metadata"}`).
///
/// # Safety
/// `document` NULL or live; `request` NULL or NUL-terminated; `err_out` NULL
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_save_cmyk_plan(
    document: *const PrecisionDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a CMYK save",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                string(document.save_cmyk_plan(borrow_cstr_bounded(
                    request,
                    "request",
                    OPTIONS_MAX,
                )?)?)
            },
            CString::into_raw,
        )
    }
}

/// Writes the composite flattened onto paper to `path` (`cmyk_io`).
///
/// # Safety
/// `document` NULL or live; `path`/`request` NULL or NUL-terminated;
/// `control` NULL or a live single-use token; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_save_cmyk(
    document: *const PrecisionDocument,
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
            "saving a CMYK file",
            false,
            || {
                let document = required_handle(document)?;
                let path = read_cstr(path, "path")?;
                document.save_cmyk(
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

/// Select and Mask's CMYK New Layer plan (`cmyk_cutout`): the RGB cutout
/// plan's keys with `foreground_storage` `"cmyka16_unorm"`. `decontaminate`
/// is 0 (off) or a strength in (0, 1]. Owned JSON; free with
/// `dr_string_free`.
///
/// # Safety
/// `document` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_cutout_plan(
    document: *const PrecisionDocument,
    decontaminate: f32,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a CMYK cutout",
            ptr::null_mut(),
            || string(required_handle(document)?.cmyk_cutout_plan(decontaminate)?),
            CString::into_raw,
        )
    }
}

/// Adds Select and Mask's CMYK New Layer output (`cmyk_cutout`): the visible
/// composite's ink as a new top CMYK raster masked by `matte` (canvas-sized
/// u16 coverage, its length the document's own pixel count), the other roots
/// hidden; `decontaminate` 0 or a strength in (0, 1]. NULL with no error
/// when no pixel has both composite alpha and matte coverage.
///
/// # Safety
/// `document` NULL or live; `matte` NULL or readable for width × height u16
/// samples; `control` NULL or a live single-use token; `err_out` NULL or
/// writable. Free the result with `dr_precision_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_add_cmyk_cutout(
    document: *const PrecisionDocument,
    matte: *const u16,
    decontaminate: f32,
    limit: u64,
    other_retained: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "assembling a CMYK cutout",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                if matte.is_null() {
                    return Err("native refinement matte is missing".into());
                }
                let (w, h) = document.dimensions();
                let n = crate::precision_image::pixel_count(w, h)?;
                document.add_cmyk_cutout(
                    std::slice::from_raw_parts(matte, n),
                    decontaminate,
                    limit,
                    other_retained,
                    control.as_ref(),
                )
            },
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// One colour across the CMYK boundary (#547 phase 2: the Color Picker, the
/// eyedropper and MCP ink): `{"from":"srgb","values":[r,g,b]}` answers
/// `{"ink":[c,m,y,k],"profile"}` and `{"from":"ink","values":[c,m,y,k]}`
/// answers `{"srgb":[r,g,b],"profile"}`, every value 0…1; optional `intent`
/// and `black_point_compensation` (defaults relative colorimetric, true).
/// sRGB is the built-in profile. The CMYK profile is `document`'s own for a
/// CMYK document, else (NULL or RGB) the working CMYK profile, refused by
/// name when none is set. Owned JSON; free with `dr_string_free`.
///
/// # Safety
/// `document` NULL or a live `DrPrecisionDocument`; `request` NULL or
/// NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_cmyk_color(
    document: *const PrecisionDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "converting a CMYK colour",
            ptr::null_mut(),
            || {
                let request = borrow_cstr_bounded(request, "request", OPTIONS_MAX)?;
                let policy = document
                    .as_ref()
                    .and_then(|d| d.cmyk_policy())
                    .map(|policy| policy.profile.clone());
                let profile = match policy {
                    Some(profile) => profile,
                    None => crate::working_cmyk::working_cmyk_profile().ok_or(
                        "no working CMYK profile is set, so a colour cannot be converted to or from CMYK ink",
                    )?,
                };
                string(color_reply(&profile, request)?)
            },
            CString::into_raw,
        )
    }
}

/// `dr_cmyk_color`'s request read and answered through `cmyk_color`'s two
/// single-colour helpers.
fn color_reply(
    profile: &crate::icc_lut::ProofProfile,
    request: &str,
) -> Result<serde_json::Value, String> {
    use crate::cmyk_color::{ink_to_srgb, srgb_to_ink, ConversionOptions};
    let value: serde_json::Value = serde_json::from_str(request)
        .map_err(|e| format!("CMYK colour request is not valid JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or("CMYK colour request must be a JSON object")?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "from" | "values" | "intent" | "black_point_compensation"
        ) {
            return Err(format!("CMYK colour request has an unknown key \"{key}\""));
        }
    }
    let mut options = ConversionOptions::default();
    if let Some(intent) = object.get("intent").filter(|v| !v.is_null()) {
        let text = intent.as_str().ok_or("intent must be a string")?;
        options.intent = crate::icc_intent::RenderingIntent::parse(text).ok_or_else(|| {
            format!("intent \"{text}\" is not one of perceptual, relative, saturation or absolute")
        })?;
    }
    match object.get("black_point_compensation") {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::Bool(v)) => options.black_point_compensation = *v,
        Some(_) => return Err("black_point_compensation must be a boolean".into()),
    }
    let values: Vec<f64> = object
        .get("values")
        .and_then(serde_json::Value::as_array)
        .ok_or("CMYK colour request needs \"values\"")?
        .iter()
        .map(|v| v.as_f64().filter(|v| v.is_finite()))
        .collect::<Option<_>>()
        .ok_or("CMYK colour values must be finite numbers")?;
    let from = object.get("from").and_then(serde_json::Value::as_str);
    match (from, values.len()) {
        (Some("srgb"), 3) => {
            let ink = srgb_to_ink(profile, [values[0], values[1], values[2]], options)?;
            Ok(json!({"ink":ink,"profile":profile.name()}))
        }
        (Some("ink"), 4) => {
            let rgb = ink_to_srgb(
                profile,
                [values[0], values[1], values[2], values[3]],
                options,
            )?;
            Ok(json!({"srgb":rgb,"profile":profile.name()}))
        }
        _ => Err(
            "a CMYK colour request is {\"from\":\"srgb\",\"values\":[r,g,b]} or \
                  {\"from\":\"ink\",\"values\":[c,m,y,k]}"
                .into(),
        ),
    }
}
