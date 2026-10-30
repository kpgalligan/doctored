//! C FFI for the adjustment ops, the image statistics and the Lab readout —
//! [`dr_doc_lab`] for a document's pixels, and the colour picker's
//! value-only pair [`dr_srgb_to_lab`] / [`dr_lab_to_srgb`] on the built-in
//! sRGB profile.
//! Same conventions as `ffi_doc` and `ffi_filters`: the shared helpers in
//! `ffi_util` supply `catch_unwind` and NULL tolerance, and the params JSON
//! is the contract documented in `adjust`'s schema table.
//!
//! The image-only destructive adjustment export [`dr_image_adjust_op`]
//! covers every legacy op; `ffi_calibrated::dr_doc_adjust_image` adds the
//! document colour context for explicit version-2 evaluation. Twelve exports with up to
//! thirty-odd float parameters each is not a C surface anyone can maintain,
//! and — the structural win — the destructive path IS the adjustment-layer
//! path, so the two can never drift.

use std::ffi::{c_char, c_int, CString};
use std::panic::catch_unwind;
use std::ptr;

use serde_json::{Map, Value};

use crate::adjust::Adjustment;
use crate::adjust_lut;
use crate::adjust_math::apply_to_image;
use crate::doc::DrDocument;
use crate::ffi_util::{doc_get, fallible_op, img_get, mask_slice, pure_op, read_cstr};
use crate::icc::IccProfile;
use crate::icc_lookup;
use crate::ops_auto::{self, AutoMode};
use crate::ops_filters;
use crate::ops_stats;
use crate::DrImage;

/// Applies the adjustment `op` with `params_json` — the SAME object an
/// adjustment layer's meta carries — to `img`'s pixels, destructively.
/// `params_json` may be NULL for "every default". NULL with a message
/// through `err_out` when the op is unknown, the params are not valid
/// UTF-8/JSON/an object, or the parameters are not valid for the op; NULL
/// with NO message on a NULL image. Alpha untouched.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`; `op` and
/// `params_json` must be NULL or valid NUL-terminated C strings; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_adjust_op(
    img: *const DrImage,
    op: *const c_char,
    params_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    if img.is_null() {
        return ptr::null_mut();
    }
    let body = || -> Result<DrImage, String> {
        let adjustment = unsafe { parse_adjustment(op, params_json) }?;
        if adjustment.requires_profile() {
            return Err(
                "document_v2 evaluation requires dr_doc_adjust_image with document colour context"
                    .into(),
            );
        }
        let image = unsafe { &*img };
        Ok(DrImage {
            pixels: apply_to_image(&adjustment, &image.pixels),
        })
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while applying an adjustment",
            ptr::null_mut(),
            body,
            crate::ffi_util::boxed,
        )
    }
}

/// Parses an Adobe Cube LUT (.cube) at `path` into the params object a
/// `"color_lookup"` adjustment stores, as a heap JSON string freed with
/// `dr_string_free`. A table larger than this build stores is resampled down
/// to the cap, and `"source_size"` — always emitted — is the size the FILE
/// declared. NULL with a message through `err_out` for a
/// missing/oversized/malformed file; never panics.
///
/// # Safety
/// `path` must be NULL or a valid NUL-terminated C string; `err_out` must be
/// NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_lut_parse_cube(
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let body = || -> Result<String, String> {
        let p = unsafe { read_cstr(path, "path") }?;
        adjust_lut::parse_cube_file(&p)
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while parsing a cube LUT",
            ptr::null_mut(),
            body,
            |json| CString::new(json).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// Reads the ICC profile at `path` as a Color Lookup LOOK and returns
/// the params object a `"color_lookup"` adjustment stores — the same shape
/// `dr_lut_parse_cube` returns, a 3D table — as a heap JSON string freed
/// with `dr_string_free`. An RGB→RGB device link whose `A2B0` is linear
/// curves around one 3-input `n³` CLUT with no matrix and `n` ≤ 33 is stored
/// at its own grid (`size` `n`); any other link is sampled at 33³; an
/// abstract profile from Lab or XYZ to Lab or XYZ is baked at 33³ through
/// `document_icc`, relative colorimetric, and its params add
/// `abstract_profile` (standard base64 of the file's bytes, at most 4 MiB),
/// which every later Assign, Convert or Don't-Convert paste re-bakes the
/// table through. `source_size` is the larger of `size` and the
/// link's own CLUT grid (33 for an abstract). NULL or empty `document_icc`
/// means no document profile: links still load and abstracts refuse. NULL
/// with a message through `err_out` naming the path and the reason for every
/// other class, space or table; never panics.
///
/// # Safety
/// `path` must be NULL or a valid NUL-terminated C string; `document_icc`
/// must be NULL or readable for `document_len` bytes; `err_out` must be NULL
/// or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_lut_parse_profile(
    path: *const c_char,
    document_icc: *const u8,
    document_len: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let body = || -> Result<String, String> {
        let p = unsafe { read_cstr(path, "path") }?;
        let document = if document_icc.is_null() || document_len == 0 {
            None
        } else {
            IccProfile::parse(unsafe { std::slice::from_raw_parts(document_icc, document_len) })
        };
        icc_lookup::parse_profile_file(&p, document.as_ref())
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while parsing an ICC look",
            ptr::null_mut(),
            body,
            |json| CString::new(json).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// `dr_lut_parse_profile`'s params from the profile's BYTES rather than a
/// path: the same JSON, the same refusals, with no path in any error
/// message. `look` NULL or empty is refused by name; `document_icc` follows
/// `dr_lut_parse_profile`'s rule. A heap JSON string freed with
/// `dr_string_free`; never panics.
///
/// # Safety
/// `look` must be NULL or readable for `look_len` bytes; `document_icc` must
/// be NULL or readable for `document_len` bytes; `err_out` must be NULL or a
/// valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_lut_bake_profile(
    look: *const u8,
    look_len: usize,
    document_icc: *const u8,
    document_len: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let body = || -> Result<String, String> {
        if look.is_null() || look_len == 0 {
            return Err("the look profile buffer is NULL or empty".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(look, look_len) };
        let document = if document_icc.is_null() || document_len == 0 {
            None
        } else {
            IccProfile::parse(unsafe { std::slice::from_raw_parts(document_icc, document_len) })
        };
        icc_lookup::bake_profile_bytes(bytes, document.as_ref())
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while baking an ICC look",
            ptr::null_mut(),
            body,
            |json| CString::new(json).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// Fills `bins_out` with 1024 counts — 256 red, then green, then blue, then
/// Rec. 709 luma — and `total_out` with the number of pixels counted. A
/// pixel counts when its alpha is non-zero and, when `mask` is given, its
/// coverage is >= 128. `stride` counts every stride-th pixel in row-major
/// order (0 and 1 both mean every pixel). A NULL `total_out` is tolerated —
/// the bins still come back — but a NULL `bins_out` is not: false on that,
/// or on a NULL image.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`; `mask` must be
/// NULL or readable for the image's width*height bytes; `bins_out` must be
/// writable for 1024 `uint32_t` and `total_out` NULL or writable for one
/// `uint64_t`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_histogram(
    img: *const DrImage,
    mask: *const u8,
    stride: u32,
    bins_out: *mut u32,
    total_out: *mut u64,
) -> bool {
    if bins_out.is_null() {
        return false;
    }
    unsafe {
        img_get(img, false, |i| {
            let (w, h) = i.pixels.dimensions();
            let len = (w as usize) * (h as usize);
            let coverage = mask_slice(mask, len);
            let (bins, total) = ops_stats::histogram(&i.pixels, coverage, stride);
            ptr::copy_nonoverlapping(bins.as_ptr(), bins_out, bins.len());
            if !total_out.is_null() {
                *total_out = total;
            }
            Some(true)
        })
    }
}

/// The straight RGBA at (x, y): the plain mean of the (2*reach + 1) square
/// about it with out-of-bounds pixels dropped (reach 0, 1 or 2 = point, 3x3,
/// 5x5), truncating like the eyedropper it serves. Writes 4 bytes to
/// `rgba_out`. false only when NO pixel of the block is inside the image,
/// `reach` is above 2, or `img`/`rgba_out` is NULL — the CENTRE itself may
/// be outside, which is what the eyedropper does today.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`; `rgba_out`
/// must be NULL or writable for 4 bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_image_sample(
    img: *const DrImage,
    x: i32,
    y: i32,
    reach: u32,
    rgba_out: *mut u8,
) -> bool {
    if rgba_out.is_null() {
        return false;
    }
    unsafe {
        img_get(img, false, |i| {
            let px = ops_stats::sample_mean(&i.pixels, x, y, reach)?;
            ptr::copy_nonoverlapping(px.as_ptr(), rgba_out, px.len());
            Some(true)
        })
    }
}

/// Derives Levels parameters from `img`'s own histogram — the same counting
/// rule `dr_image_histogram` uses — clipping `clip` of the counted pixels at
/// each end, and writes nine floats to `params_out`: black[3], white[3],
/// gamma[3]. `mask` is optional. false on a NULL image, an unknown mode, an
/// out-of-range clip, an empty image, or when the result would be the
/// identity.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`; `mask` must be
/// NULL or readable for the image's width*height bytes; `params_out` must be
/// NULL or writable for 9 `float`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_auto_levels(
    img: *const DrImage,
    mask: *const u8,
    mode: c_int,
    clip: f32,
    params_out: *mut f32,
) -> bool {
    if params_out.is_null() {
        return false;
    }
    unsafe {
        img_get(img, false, |i| {
            let mode = AutoMode::from_c(mode)?;
            let (w, h) = i.pixels.dimensions();
            let coverage = mask_slice(mask, (w as usize) * (h as usize));
            let (black, white, gamma) = ops_auto::auto_levels(&i.pixels, coverage, mode, clip)?;
            let mut out = [0.0f32; 9];
            out[..3].copy_from_slice(&black);
            out[3..6].copy_from_slice(&white);
            out[6..].copy_from_slice(&gamma);
            ptr::copy_nonoverlapping(out.as_ptr(), params_out, out.len());
            Some(true)
        })
    }
}

/// Levels with a black point, white point and gamma PER channel (three
/// floats each, R, G, B): maps [black, white] to [0, 1], then applies gamma
/// (`out = t^(1/gamma)`). NULL unless every channel has
/// `0 <= black < white <= 1` and gamma in [0.1, 10]. Alpha untouched.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`; `black`,
/// `white` and `gamma` must each be NULL or readable for 3 `float`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_levels_channels(
    img: *const DrImage,
    black: *const f32,
    white: *const f32,
    gamma: *const f32,
) -> *mut DrImage {
    if black.is_null() || white.is_null() || gamma.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        pure_op(img, |i| {
            let read = |p: *const f32| -> [f32; 3] {
                let slice = std::slice::from_raw_parts(p, 3);
                [slice[0], slice[1], slice[2]]
            };
            ops_filters::levels_channels(&i.pixels, read(black), read(white), read(gamma))
                .map(|pixels| DrImage { pixels })
        })
    }
}

/// CIE L*a*b* of one straight RGB triple of THIS document's pixels, through
/// the document's own profile, against the D50 PCS white — so the same bytes
/// read differently in an sRGB and a Display P3 document. Writes L, a, b to
/// `lab_out`. false on a NULL doc/out pointer or a profile this build cannot
/// model (a LUT profile), where the host must say so rather than assume
/// sRGB.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `lab_out`
/// must be NULL or writable for 3 `float`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_lab(
    doc: *const DrDocument,
    r: u8,
    g: u8,
    b: u8,
    lab_out: *mut f32,
) -> bool {
    if lab_out.is_null() {
        return false;
    }
    unsafe {
        doc_get(doc, false, |d| {
            let lab = d.lab([r, g, b])?;
            ptr::copy_nonoverlapping(lab.as_ptr(), lab_out, lab.len());
            Some(true)
        })
    }
}

/// CIE L*a*b* of one ENCODED sRGB colour — `rgb` is 3 floats, each clamped
/// into [0, 1] — through the BUILT-IN sRGB profile against the D50 PCS
/// white: `dr_doc_lab`'s pipeline on an sRGB document, in float. Writes L,
/// a, b to `lab_out`. false on a NULL pointer, a non-finite component, or a
/// built-in sRGB profile with no matrix/TRC model.
///
/// # Safety
/// `rgb` must be NULL or readable for 3 `float`; `lab_out` must be NULL or
/// writable for 3 `float`.
#[no_mangle]
pub unsafe extern "C" fn dr_srgb_to_lab(rgb: *const f32, lab_out: *mut f32) -> bool {
    if rgb.is_null() || lab_out.is_null() {
        return false;
    }
    catch_unwind(|| {
        let input = unsafe { std::slice::from_raw_parts(rgb, 3) };
        let Some(lab) = crate::lab::srgb_to_lab([input[0], input[1], input[2]]) else {
            return false;
        };
        unsafe { ptr::copy_nonoverlapping(lab.as_ptr(), lab_out, lab.len()) };
        true
    })
    .unwrap_or(false)
}

/// One CIE L*a*b* colour (D50 PCS white) as ENCODED sRGB through the
/// BUILT-IN sRGB profile's inverse matrix and the sRGB OETF. Writes the 3
/// components, each clamped into [0, 1], to `rgb_out`, and to
/// `in_gamut_out` whether every linear component was within a 1e-3
/// tolerance of [0, 1] before that clamp. false on a NULL pointer, a
/// non-finite component, or a built-in sRGB profile with no matrix/TRC
/// model.
///
/// # Safety
/// `lab` must be NULL or readable for 3 `float`; `rgb_out` must be NULL or
/// writable for 3 `float`; `in_gamut_out` must be NULL or writable for one
/// `bool`.
#[no_mangle]
pub unsafe extern "C" fn dr_lab_to_srgb(
    lab: *const f32,
    rgb_out: *mut f32,
    in_gamut_out: *mut bool,
) -> bool {
    if lab.is_null() || rgb_out.is_null() || in_gamut_out.is_null() {
        return false;
    }
    catch_unwind(|| {
        let input = unsafe { std::slice::from_raw_parts(lab, 3) };
        let Some((rgb, in_gamut)) = crate::lab::lab_to_srgb([input[0], input[1], input[2]]) else {
            return false;
        };
        unsafe {
            ptr::copy_nonoverlapping(rgb.as_ptr(), rgb_out, rgb.len());
            *in_gamut_out = in_gamut;
        }
        true
    })
    .unwrap_or(false)
}

/// Shared UTF-8/JSON and adjustment validation for both destructive surfaces.
/// # Safety
/// Both strings are NULL or readable NUL-terminated strings.
pub(crate) unsafe fn parse_adjustment(
    op: *const c_char,
    params_json: *const c_char,
) -> Result<Adjustment, String> {
    let name = unsafe { read_cstr(op, "op") }?;
    let parsed: Option<Value> = if params_json.is_null() {
        None
    } else {
        let text = unsafe { read_cstr(params_json, "params") }?;
        Some(serde_json::from_str(&text).map_err(|e| format!("params is not valid JSON: {e}"))?)
    };
    let empty = Map::new();
    let params = match &parsed {
        None => &empty,
        Some(value) => value
            .as_object()
            .ok_or_else(|| "params is not a JSON object".to_string())?,
    };
    Adjustment::from_op(&name, params).ok_or_else(|| {
        format!("`{name}` is not a known adjustment op, or its parameters are not valid for it")
    })
}
