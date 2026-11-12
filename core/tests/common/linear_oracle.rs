//! The independent linear-light oracle of P7-24's phases 2 and 3, shared by
//! the `evaluation_nonseparable`, `evaluation_guide`, `evaluation_styles` and
//! `evaluation_resample` files. A suite root mounts it with
//! `#[path = "common/linear_oracle.rs"] mod linear_oracle;`.
//!
//! Nothing here calls the crate's colour, blend or resampling code: the decode
//! is the analytic sRGB curve, the byte encode is the crate's forward-threshold
//! rule written independently (the output code counts the thresholds
//! `curve((k + 0.5) / 255)` strictly below the linear value), the typed encode
//! is the analytic inverse rounded once (what `PrecisionImage` stores), and
//! the non-separable functions are the W3C compositing-1 pseudocode with its
//! 0.3 / 0.59 / 0.11 weights in f64. The FFI wrappers at the end are the
//! switch, the projections and the typed edit every one of those files drives.
//!
//! Gates are phase 1's: 1 byte code, and [`TYPED_GATE`] of 65535 at 16 bits
//! (its derivation is `evaluation_composite`'s: the built-in sRGB profile's
//! TRC is 1024 rounded samples of this analytic curve).
#![allow(dead_code)]

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::*;
use doctored_core::ffi_precision::{
    dr_precision_doc_render, dr_precision_image_free, dr_precision_image_height,
    dr_precision_image_pixels_rgba16, dr_precision_image_width,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::{dr_doc_render_region, dr_precision_doc_render_region};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

pub const ENCODED: c_int = 0;
pub const LINEAR: c_int = 1;

/// 24 codes of 65535: `evaluation_composite`'s derived typed gate.
pub const TYPED_GATE: i64 = 24;

// ------------------------------------------------------------- transfer --

/// The analytic sRGB decode.
pub fn decode(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// The byte forward-threshold encode, counted.
pub fn encode(linear: f64) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    (0..255)
        .filter(|k| decode((f64::from(*k) + 0.5) / 255.0) < linear)
        .count() as u8
}

/// A stored byte as the straight [0, 1] number it encodes.
pub fn code(v: u8) -> f64 {
    f64::from(v) / 255.0
}

/// A stored byte as the linear value it decodes to.
pub fn lin(v: u8) -> f64 {
    decode(code(v))
}

/// The analytic continuous encode (the inverse of [`decode`]).
pub fn srgb_encode(linear: f64) -> f64 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// The typed encode: continuous, then the one `round(v · 65535)` the store
/// makes.
pub fn typed_encode(linear: f64) -> u16 {
    (srgb_encode(linear) * 65535.0).round() as u16
}

/// An 8-bit code as the 16-bit lattice code it promotes to (`257 · k`).
pub fn typed_code(v: u8) -> u16 {
    u16::from(v) * 257
}

/// A stored 16-bit code as the linear value it decodes to.
pub fn typed_lin(v: u16) -> f64 {
    decode(f64::from(v) / 65535.0)
}

// ------------------------------------------------- W3C non-separable --

pub fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f64; 3]) -> [f64; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        out = out.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 && x > l {
        out = out.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    out
}

fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: [f64; 3]) -> f64 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| c[a].total_cmp(&c[b]));
    let [lo, mid, hi] = order;
    let mut out = [0.0; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}

/// `B(Cb, Cs)` of one of the six non-separable modes (the W3C pseudocode;
/// the two picks keep the backdrop on a tie).
pub fn nonseparable(mode: BlendMode, cb: [f64; 3], cs: [f64; 3]) -> [f64; 3] {
    match mode {
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
        BlendMode::DarkerColor => {
            if lum(cs) < lum(cb) {
                cs
            } else {
                cb
            }
        }
        BlendMode::LighterColor => {
            if lum(cs) > lum(cb) {
                cs
            } else {
                cb
            }
        }
        other => panic!("{other:?} is not a non-separable mode"),
    }
}

/// W3C source-over of a straight source at alpha `sa` over a straight
/// backdrop, with `blended` the mode's `B(Cb, Cs)`.
pub fn over_blended(bg: [f64; 4], cs: [f64; 3], blended: [f64; 3], sa: f64) -> [f64; 4] {
    if sa <= 0.0 {
        return bg;
    }
    let ab = bg[3];
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (sa * (1.0 - ab) * cs[c] + sa * ab * blended[c] + (1.0 - sa) * ab * bg[c]) / ao;
    }
    out[3] = ao;
    out
}

/// Normal source-over.
pub fn over(bg: [f64; 4], cs: [f64; 3], sa: f64) -> [f64; 4] {
    over_blended(bg, cs, cs, sa)
}

/// `a + (b − a)·t` per component.
pub fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * t)
}

/// An opaque linear triple as the stored RGBA8 the byte projection makes.
pub fn encode_opaque(linear: [f64; 3]) -> [u8; 4] {
    [encode(linear[0]), encode(linear[1]), encode(linear[2]), 255]
}

/// An opaque linear triple as the stored RGBA16 the typed projection makes.
pub fn typed_encode_opaque(linear: [f64; 3]) -> [u16; 4] {
    [
        typed_encode(linear[0]),
        typed_encode(linear[1]),
        typed_encode(linear[2]),
        65535,
    ]
}

// ----------------------------------------------------------------- gates --

pub fn assert_within(actual: [u8; 4], expected: [u8; 4], gate: i32, what: &str) {
    for c in 0..4 {
        let delta = i32::from(actual[c]) - i32::from(expected[c]);
        assert!(
            delta.abs() <= gate,
            "{what}: channel {c} got {} want {} (gate {gate} codes)",
            actual[c],
            expected[c]
        );
    }
}

pub fn typed_within(actual: [u16; 4], expected: [u16; 4], what: &str) {
    for c in 0..4 {
        let delta = i64::from(actual[c]) - i64::from(expected[c]);
        assert!(
            delta.abs() <= TYPED_GATE,
            "{what}: channel {c} got {} want {} (gate {TYPED_GATE} codes of 65535)",
            actual[c],
            expected[c]
        );
    }
}

// ------------------------------------------------------------------ FFI --

pub fn take_err(err: *mut c_char) -> String {
    assert!(!err.is_null(), "a refusal must carry a sentence");
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

/// `dr_doc_set_evaluation`: `Ok(None)` is the unchanged-policy NULL.
pub fn switch(doc: &DrDocument, policy: c_int) -> Result<Option<DrDocument>, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_set_evaluation(doc, policy, &mut err) };
    if out.is_null() {
        if err.is_null() {
            Ok(None)
        } else {
            Err(take_err(err))
        }
    } else {
        assert!(err.is_null(), "err_out set on success");
        Ok(Some(*unsafe { Box::from_raw(out) }))
    }
}

/// The document switched to linear light; panics on a refusal.
pub fn linear(doc: &DrDocument) -> DrDocument {
    switch(doc, LINEAR)
        .unwrap_or_else(|e| panic!("switch refused: {e}"))
        .expect("an encoded document changes policy")
}

/// `dr_doc_flattened`, the projection as the host sees it.
pub fn flatten(doc: &DrDocument) -> Result<Vec<u8>, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_flattened(doc, u64::MAX, ptr::null(), &mut err) };
    if out.is_null() {
        return Err(take_err(err));
    }
    let (w, h) = unsafe { (dr_image_width(out), dr_image_height(out)) };
    let pixels = unsafe { dr_image_pixels_rgba(out) };
    let bytes = unsafe { std::slice::from_raw_parts(pixels, (w * h * 4) as usize) }.to_vec();
    unsafe { dr_image_free(out) };
    Ok(bytes)
}

/// `dr_doc_render_region` over the whole canvas at divisor 1: the regional
/// compositor's answer, which must equal [`flatten`]'s.
pub fn region(doc: &DrDocument) -> Result<Vec<u8>, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_render_region(
            doc,
            0,
            0,
            doc.width,
            doc.height,
            1,
            u64::MAX,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err(err));
    }
    let (w, h) = unsafe { (dr_image_width(out), dr_image_height(out)) };
    let pixels = unsafe { dr_image_pixels_rgba(out) };
    let bytes = unsafe { std::slice::from_raw_parts(pixels, (w * h * 4) as usize) }.to_vec();
    unsafe { dr_image_free(out) };
    Ok(bytes)
}

/// Pixel `i` (row-major) of an RGBA8 buffer.
pub fn pixel(bytes: &[u8], i: usize) -> [u8; 4] {
    [
        bytes[i * 4],
        bytes[i * 4 + 1],
        bytes[i * 4 + 2],
        bytes[i * 4 + 3],
    ]
}

/// Layer `idx`'s meta, through the FFI setter.
pub fn with_meta(doc: &DrDocument, idx: usize, meta: &str) -> DrDocument {
    let c = CString::new(meta).expect("no interior NUL");
    let out = unsafe { dr_doc_with_layer_meta(doc, idx, c.as_ptr()) };
    assert!(!out.is_null(), "the meta was refused: {meta}");
    *unsafe { Box::from_raw(out) }
}

/// `dr_precision_doc_set_evaluation`.
pub fn typed_switch(
    doc: &PrecisionDocument,
    policy: c_int,
) -> Result<Option<PrecisionDocument>, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_evaluation(doc, policy, &mut err) };
    if out.is_null() {
        if err.is_null() {
            Ok(None)
        } else {
            Err(take_err(err))
        }
    } else {
        assert!(err.is_null(), "err_out set on success");
        Ok(Some(*unsafe { Box::from_raw(out) }))
    }
}

pub fn typed_linear(doc: &PrecisionDocument) -> PrecisionDocument {
    typed_switch(doc, LINEAR)
        .unwrap_or_else(|e| panic!("typed switch refused: {e}"))
        .expect("an encoded typed document changes policy")
}

/// `dr_precision_doc_render`, the authoritative u16 projection.
pub fn typed_render(doc: &PrecisionDocument) -> Result<Vec<[u16; 4]>, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let image =
        unsafe { dr_precision_doc_render(doc, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut err) };
    if image.is_null() {
        return Err(take_err(err));
    }
    let count =
        unsafe { dr_precision_image_width(image) * dr_precision_image_height(image) } as usize;
    let samples = unsafe { dr_precision_image_pixels_rgba16(image) };
    assert!(!samples.is_null(), "a u16 projection");
    let samples = unsafe { std::slice::from_raw_parts(samples, count * 4) }.to_vec();
    unsafe { dr_precision_image_free(image) };
    Ok(samples
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect())
}

/// `dr_precision_doc_render_region` over the whole canvas at divisor 1.
pub fn typed_region(doc: &PrecisionDocument) -> Result<Vec<[u16; 4]>, String> {
    let (w, h) = doc.dimensions();
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_render_region(
            doc,
            0,
            0,
            w,
            h,
            1,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if image.is_null() {
        return Err(take_err(err));
    }
    let count =
        unsafe { dr_precision_image_width(image) * dr_precision_image_height(image) } as usize;
    let samples = unsafe { dr_precision_image_pixels_rgba16(image) };
    assert!(!samples.is_null(), "a u16 region");
    let samples = unsafe { std::slice::from_raw_parts(samples, count * 4) }.to_vec();
    unsafe { dr_precision_image_free(image) };
    Ok(samples
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect())
}

/// `dr_precision_doc_edit`, panicking on a refusal; `None` is the no-op.
pub fn typed_edit(doc: &PrecisionDocument, request: &str) -> Option<PrecisionDocument> {
    typed_try_edit(doc, request).unwrap_or_else(|e| panic!("the edit refused: {e}: {request}"))
}

/// `dr_precision_doc_edit`: `Ok(None)` is the no-op, `Err` a refusal.
pub fn typed_try_edit(
    doc: &PrecisionDocument,
    request: &str,
) -> Result<Option<PrecisionDocument>, String> {
    let request = CString::new(request).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        if err.is_null() {
            return Ok(None);
        }
        return Err(take_err(err));
    }
    Ok(Some(*unsafe { Box::from_raw(out) }))
}

/// The typed `layer_blend` edit.
pub fn typed_blend(doc: &PrecisionDocument, layer: usize, mode: BlendMode) -> PrecisionDocument {
    let request = format!(
        r#"{{"op":"layer_blend","version":1,"layer":{layer},"blend_mode":{}}}"#,
        mode.to_c()
    );
    typed_edit(doc, &request).unwrap_or_else(|| doc.clone())
}
