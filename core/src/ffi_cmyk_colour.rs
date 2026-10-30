//! The CMYK document's colour workflow across the FFI (#1 phase 5): Assign
//! and Convert to Profile, 8/16 Bits/Channel and File ▸ New in CMYK
//! (`cmyk_profile`), the CMYK proof and the one-colour separation
//! (`cmyk_proof`), Levels and Curves' histogram, Auto, eyedroppers and ⌥
//! clipping display (`cmyk_tone_analysis`), and the CMYK preset decode
//! (`cmyk_preset_file`) — the "CMYK documents (#1 phase 5)" section of
//! `include/doctored_core.h`.
//!
//! The conventions are the surface's: NULL tolerance first, `catch_unwind`
//! through the shared `ffi_util` helpers, `Err` through `err_out`, a coverage
//! length recomputed from the document's own dimensions.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::cmyk_color::ConversionOptions;
use crate::ffi_color::blob;
use crate::ffi_util::{
    borrow_cstr_bounded, boxed, fallible_op, read_cstr, required_handle, write_string,
};
use crate::icc::IccProfile;
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::DrImage;

/// The largest request or setup JSON this surface accepts.
const REQUEST_MAX: usize = 4096;

fn string(value: serde_json::Value) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| format!("cannot report CMYK JSON: {e}"))
}

/// An optional bounded JSON argument: NULL is the empty string.
unsafe fn request<'a>(text: *const c_char, what: &str) -> Result<&'a str, String> {
    if text.is_null() {
        return Ok("");
    }
    unsafe { borrow_cstr_bounded(text, what, REQUEST_MAX) }
}

/// Assign / Convert to Profile and 8/16 Bits/Channel's plan:
/// `{"working_memory_plan_bytes","refusal","flatten_required"}`. `icc` NULL
/// is the working CMYK profile. Owned JSON; free with `dr_string_free`.
///
/// # Safety
/// `document` NULL or a live `DrPrecisionDocument`; `icc` NULL or valid for
/// `icc_len` bytes; `request` NULL or NUL-terminated; `err_out` NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_profile_plan(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a CMYK profile edit",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                string(
                    document
                        .cmyk_profile_plan(blob(icc, icc_len), request(request_json, "request")?)?,
                )
            },
            CString::into_raw,
        )
    }
}

/// Assign, Convert or Depth (`cmyk_profile`): a new CMYK document; NULL with
/// no error when the edit changes nothing.
///
/// # Safety
/// As [`dr_precision_doc_cmyk_profile_plan`]; `control` NULL or a live
/// single-use token.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_cmyk_profile_edit(
    document: *const PrecisionDocument,
    icc: *const u8,
    icc_len: usize,
    request_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "editing a CMYK profile",
            ptr::null_mut(),
            || {
                required_handle(document)?.cmyk_profile_edit(
                    blob(icc, icc_len),
                    request(request_json, "request")?,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// File ▸ New in CMYK: a `width` × `height` CMYK document of one filled
/// record (`cmyk_profile`); request `{"depth":8|16,"fill":[c,m,y,k,a],
/// "layer_name","ppi"}`, NULL the defaults; `icc` NULL the working CMYK.
///
/// # Safety
/// `icc` NULL or valid for `icc_len` bytes; `request` NULL or
/// NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_new_cmyk(
    width: u32,
    height: u32,
    icc: *const u8,
    icc_len: usize,
    request_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "creating a CMYK document",
            ptr::null_mut(),
            || {
                PrecisionDocument::new_cmyk(
                    width,
                    height,
                    blob(icc, icc_len),
                    request(request_json, "request")?,
                    limit,
                    other_retained_bytes,
                )
            },
            boxed,
        )
    }
}

/// The CMYK proof's plan: `{"working_memory_plan_bytes","refusal"}`.
///
/// # Safety
/// `document` NULL or live; `proof_icc` NULL or valid for `proof_len` bytes;
/// `setup_json` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_proof_plan(
    document: *const PrecisionDocument,
    proof_icc: *const u8,
    proof_len: usize,
    setup_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a CMYK proof",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let proof = blob(proof_icc, proof_len).ok_or("the proof profile is NULL")?;
                string(document.cmyk_proof_plan(proof, request(setup_json, "setup_json")?)?)
            },
            CString::into_raw,
        )
    }
}

/// The CMYK document's soft proof (`cmyk_proof`): RGBA8 in the built-in
/// Display P3; `info_out` (may be NULL) receives `{"space":"display-p3",
/// "out_of_gamut":0}`, left untouched on refusal or cancel.
///
/// # Safety
/// As [`dr_precision_doc_cmyk_proof_plan`]; `control` NULL or a live
/// single-use token; `info_out` NULL or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_cmyk_proof(
    document: *const PrecisionDocument,
    proof_icc: *const u8,
    proof_len: usize,
    setup_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    info_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering a CMYK proof",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let proof = blob(proof_icc, proof_len).ok_or("the proof profile is NULL")?;
                let (image, info) = document.cmyk_proof(
                    proof,
                    request(setup_json, "setup_json")?,
                    limit,
                    other,
                    control.as_ref(),
                )?;
                let (w, h) = image.dimensions();
                let image = DrImage::from_rgba(w, h, image.into_raw())
                    .ok_or("the proof produced invalid dimensions")?;
                Ok((info, image))
            },
            |(info, image)| {
                write_string(info_out, &info.to_string());
                boxed(image)
            },
        )
    }
}

/// Levels and Curves' analysis plan: `{"working_memory_plan_bytes",
/// "refusal"}`.
///
/// # Safety
/// `document` NULL or live; `request` NULL or NUL-terminated; `err_out` NULL
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_tone_analysis_plan(
    document: *const PrecisionDocument,
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a CMYK tone analysis",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                string(document.cmyk_tone_analysis_plan(request(request_json, "request")?)?)
            },
            CString::into_raw,
        )
    }
}

/// The histograms, Auto's endpoints and an eyedropper's sample and target
/// (`cmyk_tone_analysis`). `coverage` NULL or `coverage_len` 0 is no
/// selection; otherwise it must hold exactly the canvas's samples.
///
/// # Safety
/// `document` NULL or live; `request` NULL or NUL-terminated; `coverage`
/// NULL or readable for `coverage_len` u16 samples; `control` NULL or a live
/// single-use token; `err_out` NULL or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_cmyk_tone_analysis(
    document: *const PrecisionDocument,
    request_json: *const c_char,
    coverage: *const u16,
    coverage_len: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "analysing CMYK tones",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let samples: &[u16] = if coverage.is_null() || coverage_len == 0 {
                    &[]
                } else {
                    // The length is the document's own, never the caller's.
                    let (w, h) = document.dimensions();
                    let count = w as usize * h as usize;
                    if coverage_len != count {
                        return Err(
                            "selection coverage must be empty or canvas-sized u16 samples".into(),
                        );
                    }
                    std::slice::from_raw_parts(coverage, count)
                };
                string(document.cmyk_tone_analysis(
                    request(request_json, "request")?,
                    samples,
                    limit,
                    other,
                    control.as_ref(),
                )?)
            },
            CString::into_raw,
        )
    }
}

/// Curves' ⌥ clipping display of `document`'s composite: opaque RGBA8,
/// request `{"mode":"shadows"|"highlights","plates":["c","m","y","k"]}`.
///
/// # Safety
/// `document` NULL or live; `request` NULL or NUL-terminated; `control` NULL
/// or a live single-use token; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_clip_display(
    document: *const PrecisionDocument,
    request_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering a CMYK clipping display",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let text = borrow_cstr_bounded(request_json, "request", REQUEST_MAX)?;
                document.cmyk_clip_display(text, limit, other, control.as_ref())
            },
            boxed,
        )
    }
}

/// One RGB colour's separation (`cmyk_proof::separate`): the Info panel's
/// CMYK row with its "!" and the Color Picker's gamut alarm. `cmyk_document`
/// NULL or RGB is the working CMYK profile (refused by name when none is
/// set); `rgb_icc` NULL is the built-in sRGB. Request `{"values":[r,g,b]
/// 0…1,"intent","black_point_compensation"}` (relative with BPC by
/// default) → `{"ink":[c,m,y,k],"out_of_gamut","delta_e","in_gamut":[r,g,b],
/// "profile"}`. Owned JSON; free with `dr_string_free`.
///
/// # Safety
/// `cmyk_document` NULL or live; `rgb_icc` NULL or valid for `rgb_icc_len`
/// bytes; `request` NULL or NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_cmyk_separate(
    cmyk_document: *const PrecisionDocument,
    rgb_icc: *const u8,
    rgb_icc_len: usize,
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "separating a colour",
            ptr::null_mut(),
            || {
                let text = borrow_cstr_bounded(request_json, "request", REQUEST_MAX)?;
                let profile = match cmyk_document
                    .as_ref()
                    .and_then(|d| d.cmyk_policy())
                    .map(|policy| policy.profile.clone())
                {
                    Some(profile) => profile,
                    None => crate::working_cmyk::working_cmyk_profile()
                        .ok_or("no working CMYK profile is set, so a colour cannot be separated")?,
                };
                let rgb = match blob(rgb_icc, rgb_icc_len) {
                    Some(bytes) => std::sync::Arc::new(
                        IccProfile::parse(bytes)
                            .ok_or("the RGB profile is not an RGB ICC profile")?,
                    ),
                    None => IccProfile::srgb(),
                };
                let (values, options) = separate_request(text)?;
                string(crate::cmyk_proof::separate(
                    &rgb, &profile, values, options,
                )?)
            },
            CString::into_raw,
        )
    }
}

/// `dr_cmyk_separate`'s request (`dr_cmyk_color`'s keys and defaults).
fn separate_request(text: &str) -> Result<([f64; 3], ConversionOptions), String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| format!("the separation request is not valid JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or("the separation request must be a JSON object")?;
    if let Some(key) = object
        .keys()
        .find(|k| !matches!(k.as_str(), "values" | "intent" | "black_point_compensation"))
    {
        return Err(format!(
            "the separation request has an unknown key \"{key}\""
        ));
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
        .filter(|a| a.len() == 3)
        .and_then(|a| {
            a.iter()
                .map(|v| v.as_f64().filter(|v| v.is_finite()))
                .collect()
        })
        .ok_or("the separation request needs \"values\": [r, g, b], each 0…1")?;
    Ok(([values[0], values[1], values[2]], options))
}

/// Decodes a Photoshop `.alv` or `.acv` with ink channels for a CMYK
/// document (`cmyk_preset_file`): `{"op","params"}` in `cmyk_adjust`'s
/// schema. NULL `bytes` with `len` 0 is an empty file. Owned JSON; free with
/// `dr_string_free`.
///
/// # Safety
/// `bytes` NULL or readable for `len` bytes; `extension` NULL or
/// NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_adjustment_preset_file_decode_cmyk(
    bytes: *const u8,
    len: usize,
    extension: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "decoding a CMYK Photoshop preset file",
            ptr::null_mut(),
            || {
                let extension = read_cstr(extension, "extension")?;
                if len > crate::adjustment_preset_file::MAX_FILE_BYTES {
                    return Err(crate::adjustment_preset_file::too_large(len));
                }
                let file: &[u8] = if bytes.is_null() {
                    if len != 0 {
                        return Err("bytes is NULL".to_string());
                    }
                    &[]
                } else {
                    std::slice::from_raw_parts(bytes, len)
                };
                string(crate::cmyk_preset_file::decode_cmyk(file, &extension)?)
            },
            CString::into_raw,
        )
    }
}
