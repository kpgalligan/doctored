//! The Phase 11A export workspace at the C boundary.
//!
//! The headline checks are the two the slice's completion gate rests on:
//!
//! * **The default request reproduces today's file.** `dr_doc_export_encode`
//!   with nothing but a format equals `dr_doc_save_image`'s bytes for every
//!   byte format, and `dr_precision_doc_export_encode` at `bit_depth: 16`
//!   equals `dr_precision_doc_save_projection16`'s bytes for BOTH PNG16 (so
//!   the `pHYs`/XMP chunks travel) and TIFF16.
//! * **The stage order is resample → sharpen → convert → reduce → encode.**
//!   The pipeline's bytes equal the same three shipped exports composed IN
//!   THAT ORDER, and differ from the sharpen-before-resample composition.
//!
//! Everything else is refusals, cancellation, the naming grammar, the
//! overwrite rule and the NULL sweep. The dither and nearest checks are
//! against arithmetic written here, never against a recorded output.
use crate::common;
use common::{cpath, take_err_string};

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::{
    dr_image_free, dr_image_from_rgba, dr_image_height, dr_image_pixels_rgba, dr_image_resize,
    dr_image_width, dr_string_free,
};
use doctored_core::ffi_color::{
    dr_builtin_profile, dr_builtin_profile_len, dr_doc_assign_profile, dr_doc_convert_to_profile,
    dr_doc_save_image,
};
use doctored_core::ffi_doc::{dr_doc_flattened, dr_doc_free, dr_doc_from_image};
use doctored_core::ffi_export::*;
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_precision::{
    dr_precision_doc_export16, dr_precision_doc_free, dr_precision_doc_render,
    dr_precision_image_free,
};
use doctored_core::ffi_precision_color::dr_precision_doc_convert_to_profile;
use doctored_core::ffi_precision_export::dr_precision_doc_save_projection16;
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;

use std::ffi::{c_char, CString};
use std::ptr;
use tempfile::TempDir;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const PROFILE_SRGB: i32 = 0;
const PROFILE_P3: i32 = 1;
const FORMAT_PNG: i32 = 0;
const FORMAT_JPEG: i32 = 1;
const FORMAT_TIFF: i32 = 2;
const FORMAT_BMP: i32 = 3;
const FORMAT_GIF: i32 = 4;
const FORMAT_WEBP: i32 = 5;
const FILTER_LANCZOS3: i32 = 3;

// ------------------------------------------------------------- the fixture --

const W: u32 = 24;
const H: u32 = 18;

/// Coprime ramps with hard edges, so a resample and a sharpen both have real
/// detail to move, plus fully opaque alpha so the JPEG/GIF comparisons are
/// about the codecs rather than about matting.
fn plate() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            let edge = if (i / W).is_multiple_of(3) { 250 } else { 12 };
            [
                ((i * 37 + 11) % 256) as u8,
                edge,
                ((i * 91 + 3) % 256) as u8,
                255,
            ]
        })
        .collect()
}

fn samples16() -> Vec<u16> {
    plate().iter().map(|v| u16::from(*v) * 257).collect()
}

fn builtin(which: i32) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    assert!(len > 0);
    let mut bytes = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, bytes.as_mut_ptr(), len) });
    bytes
}

/// The byte Unsharp Mask: `dr_image_filter_op("unsharp_mask")` with a NULL
/// control (the dedicated export was folded into it).
unsafe fn unsharp_mask(
    img: *const DrImage,
    radius: f64,
    amount: f64,
    threshold: f64,
    err: *mut *mut c_char,
) -> *mut DrImage {
    let op = CString::new("unsharp_mask").unwrap();
    let params = CString::new(
        serde_json::json!({"radius": radius, "amount": amount, "threshold": threshold}).to_string(),
    )
    .unwrap();
    unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), ptr::null(), err) }
}

fn image_of(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let handle = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!handle.is_null());
    handle
}

fn image_bytes(image: *const DrImage) -> Vec<u8> {
    let w = unsafe { dr_image_width(image) } as usize;
    let h = unsafe { dr_image_height(image) } as usize;
    let p = unsafe { dr_image_pixels_rgba(image) };
    assert!(!p.is_null());
    unsafe { std::slice::from_raw_parts(p, w * h * 4) }.to_vec()
}

/// A one-layer byte document carrying `which` as its profile, with no
/// inherited packets and the default 72 ppi — so the sidecar a comparison
/// composes is the same one the pipeline composes.
fn byte_doc(pixels: &[u8], which: i32) -> *mut DrDocument {
    let image = image_of(W, H, pixels);
    let plain = unsafe { dr_doc_from_image(image) };
    unsafe { dr_image_free(image) };
    assert!(!plain.is_null());
    let profile = builtin(which);
    // `dr_doc_from_image` already tags sRGB, and an assign that changes
    // nothing is a refusal rather than an identical copy.
    let tagged = unsafe {
        dr_doc_assign_profile(plain, profile.as_ptr(), profile.len(), std::ptr::null_mut())
    };
    if tagged.is_null() {
        return plain;
    }
    unsafe { dr_doc_free(plain) };
    tagged
}

fn typed_doc(which: i32) -> *mut PrecisionDocument {
    let samples = samples16();
    let profile = builtin(which);
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr(),
            samples.len(),
            W,
            H,
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!document.is_null(), "{}", take_err_string(err));
    document
}

fn typed_projection(document: *const PrecisionDocument) -> *mut PrecisionImage {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(document, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    image
}

// --------------------------------------------------------------- utilities --

fn encode_byte(
    doc: *const DrDocument,
    flat: *const DrImage,
    request: &str,
) -> Result<*mut ExportResult, String> {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe { dr_doc_export_encode(doc, flat, json.as_ptr(), ptr::null(), &mut err) };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null());
    Ok(result)
}

fn encode_typed(
    document: *const PrecisionDocument,
    projection: *const PrecisionImage,
    request: &str,
) -> Result<*mut ExportResult, String> {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            document,
            projection,
            json.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null());
    Ok(result)
}

fn result_bytes(result: *const ExportResult) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    assert!(!pointer.is_null());
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

fn result_info(result: *const ExportResult) -> serde_json::Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_export_result_info(result, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    serde_json::from_str(&text).expect("info json")
}

fn output_size(w: u32, h: u32, request: &str) -> Result<(u32, u32), String> {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let text = unsafe { dr_export_output_size(w, h, json.as_ptr(), &mut err) };
    if text.is_null() {
        return Err(take_err_string(err));
    }
    let owned = unsafe { std::ffi::CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    let value: serde_json::Value = serde_json::from_str(&owned).expect("size json");
    Ok((
        value["width"].as_u64().expect("width") as u32,
        value["height"].as_u64().expect("height") as u32,
    ))
}

fn expand(pattern: &str, tokens: &str) -> Result<String, String> {
    let pattern = CString::new(pattern).expect("pattern");
    let tokens = CString::new(tokens).expect("tokens");
    let mut err: *mut c_char = ptr::null_mut();
    let text = unsafe { dr_export_expand_name(pattern.as_ptr(), tokens.as_ptr(), &mut err) };
    if text.is_null() {
        return Err(take_err_string(err));
    }
    let owned = unsafe { std::ffi::CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    Ok(owned)
}

const TOKENS: &str = r#"{"name":"Beach","width":800,"height":600,"format":"jpeg","date":"2026-09-17","preset":"Web"}"#;

// ------------------------------------------------- (a) output dimensions --

#[test]
fn output_dimensions_keep_the_aspect_and_refuse_impossible_sizes() {
    // Aspect kept, round() then max(1).
    assert_eq!(
        output_size(1000, 400, r#"{"format":"png"}"#).unwrap(),
        (1000, 400)
    );
    assert_eq!(
        output_size(
            1000,
            400,
            r#"{"format":"png","size":{"mode":"width","value":250}}"#
        )
        .unwrap(),
        (250, 100)
    );
    assert_eq!(
        output_size(
            1000,
            400,
            r#"{"format":"png","size":{"mode":"height","value":100}}"#
        )
        .unwrap(),
        (250, 100)
    );
    assert_eq!(
        output_size(
            1000,
            400,
            r#"{"format":"png","size":{"mode":"long_edge","value":500}}"#
        )
        .unwrap(),
        (500, 200)
    );
    assert_eq!(
        output_size(
            400,
            1000,
            r#"{"format":"png","size":{"mode":"long_edge","value":500}}"#
        )
        .unwrap(),
        (200, 500)
    );
    assert_eq!(
        output_size(
            1000,
            400,
            r#"{"format":"png","size":{"mode":"scale","value":0.5}}"#
        )
        .unwrap(),
        (500, 200)
    );
    // A side that would round to zero is held at one pixel.
    assert_eq!(
        output_size(
            1000,
            3,
            r#"{"format":"png","size":{"mode":"width","value":10}}"#
        )
        .unwrap(),
        (10, 1)
    );
    // Rounding, not truncation: 400 * 7 / 1000 = 2.8.
    assert_eq!(
        output_size(
            1000,
            400,
            r#"{"format":"png","size":{"mode":"width","value":7}}"#
        )
        .unwrap(),
        (7, 3)
    );

    for (request, needle) in [
        (
            r#"{"format":"png","size":{"mode":"width","value":0}}"#,
            "positive",
        ),
        (
            r#"{"format":"png","size":{"mode":"scale","value":0}}"#,
            "scale",
        ),
        (
            r#"{"format":"png","size":{"mode":"scale","value":17}}"#,
            "scale",
        ),
        (
            r#"{"format":"png","size":{"mode":"width","value":40000}}"#,
            "pixel limit",
        ),
        (
            r#"{"format":"png","size":{"mode":"long_edge","value":29000}}"#,
            "pixel limit",
        ),
        (
            r#"{"format":"png","size":{"mode":"diagonal","value":10}}"#,
            "not one of",
        ),
        (
            r#"{"format":"png","size":{"mode":"width"}}"#,
            "requires a value",
        ),
    ] {
        let error = output_size(4000, 3000, request).expect_err(request);
        assert!(error.contains(needle), "{request}: {error}");
    }
    // The request grammar itself.
    // (`avif` and `jxl` are real formats, so the unknown one here
    // is JPEG 2000, which the export workspace does not write.)
    assert!(output_size(10, 10, r#"{"format":"jp2"}"#)
        .expect_err("format")
        .contains("not one of png"));
    // HEIC is a real format since Phase 11D — host-encoded, so the export
    // workspace accepts it and the flat writer refuses it (the equalities
    // and the refusal live in `export_host_formats`).
    assert_eq!(
        output_size(10, 10, r#"{"format":"heic"}"#).expect("heic"),
        (10, 10)
    );
    assert!(output_size(10, 10, r#"{"format":"png","jpeg_qualtiy":50}"#)
        .expect_err("unknown key")
        .contains("unknown key"));
    assert!(output_size(10, 10, r#"{"format":"jpeg","bit_depth":16}"#)
        .expect_err("depth")
        .contains("png, tiff, heic or avif"));
    assert!(output_size(
        10,
        10,
        r#"{"format":"png","bit_depth":16,"dither":"ordered"}"#
    )
    .expect_err("dither")
    .contains("only when reducing"));
}

// -------------------------------------------------------- (b) stage order --

#[test]
fn the_pipeline_is_resample_then_sharpen_then_convert() {
    let dir = TempDir::new().expect("temp");
    let pixels = plate();
    let doc = byte_doc(&pixels, PROFILE_P3);
    let srgb = builtin(PROFILE_SRGB);
    let (w, h) = (12u32, 9u32);
    let request = format!(
        r#"{{"format":"png","size":{{"mode":"width","value":{w}}},"filter":"lanczos3",
            "sharpen":{{"radius":1.4,"amount":1.75,"threshold":0.0}},"profile":"srgb"}}"#
    );
    let result = encode_byte(doc, ptr::null(), &request).expect("encode");
    let pipeline = result_bytes(result);

    // The same three shipped exports composed in the pipeline's own order.
    let compose = |sharpen_first: bool| -> Vec<u8> {
        let mut err: *mut c_char = ptr::null_mut();
        let flat = unsafe { dr_doc_flattened(doc, 0, std::ptr::null(), &mut err) };
        assert!(!flat.is_null(), "{}", take_err_string(err));
        let staged = if sharpen_first {
            let sharpened = unsafe { unsharp_mask(flat, 1.4, 1.75, 0.0, &mut err) };
            assert!(!sharpened.is_null(), "{}", take_err_string(err));
            let resized = unsafe { dr_image_resize(sharpened, w, h, FILTER_LANCZOS3) };
            unsafe { dr_image_free(sharpened) };
            resized
        } else {
            let resized = unsafe { dr_image_resize(flat, w, h, FILTER_LANCZOS3) };
            assert!(!resized.is_null());
            let sharpened = unsafe { unsharp_mask(resized, 1.4, 1.75, 0.0, &mut err) };
            unsafe { dr_image_free(resized) };
            assert!(!sharpened.is_null(), "{}", take_err_string(err));
            sharpened
        };
        unsafe { dr_image_free(flat) };
        assert!(!staged.is_null());
        let plain = unsafe { dr_doc_from_image(staged) };
        unsafe { dr_image_free(staged) };
        let p3 = builtin(PROFILE_P3);
        let tagged =
            unsafe { dr_doc_assign_profile(plain, p3.as_ptr(), p3.len(), std::ptr::null_mut()) };
        assert!(!tagged.is_null());
        unsafe { dr_doc_free(plain) };
        let converted = unsafe {
            dr_doc_convert_to_profile(tagged, srgb.as_ptr(), srgb.len(), std::ptr::null_mut())
        };
        unsafe { dr_doc_free(tagged) };
        assert!(!converted.is_null());
        let path = dir.path().join(if sharpen_first {
            "sharpen-first.png"
        } else {
            "resize-first.png"
        });
        let c = cpath(&path);
        let mut carried = 0u32;
        assert!(unsafe {
            dr_doc_save_image(
                converted,
                ptr::null(),
                c.as_ptr(),
                FORMAT_PNG,
                90,
                true,
                false,
                &mut carried,
                std::ptr::null(),
                &mut err,
            )
        });
        unsafe { dr_doc_free(converted) };
        std::fs::read(&path).expect("read")
    };

    assert_eq!(
        pipeline,
        compose(false),
        "resample then sharpen then convert"
    );
    assert_ne!(
        pipeline,
        compose(true),
        "sharpening before the resample must not produce the same file"
    );

    unsafe { dr_export_result_free(result) };
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------ (c) the defaults are today's --

#[test]
fn default_requests_reproduce_the_existing_byte_export() {
    let dir = TempDir::new().expect("temp");
    let pixels = plate();
    let doc = byte_doc(&pixels, PROFILE_P3);
    for (name, format) in [
        ("png", FORMAT_PNG),
        ("jpeg", FORMAT_JPEG),
        ("tiff", FORMAT_TIFF),
        ("bmp", FORMAT_BMP),
        ("gif", FORMAT_GIF),
        ("webp", FORMAT_WEBP),
    ] {
        let path = dir.path().join(format!("today.{name}"));
        let c = cpath(&path);
        let mut carried = 0u32;
        let mut err: *mut c_char = ptr::null_mut();
        assert!(
            unsafe {
                dr_doc_save_image(
                    doc,
                    ptr::null(),
                    c.as_ptr(),
                    format,
                    90,
                    true,
                    false,
                    &mut carried,
                    std::ptr::null(),
                    &mut err,
                )
            },
            "{}",
            take_err_string(err)
        );
        let expected = std::fs::read(&path).expect("read");
        let result = encode_byte(doc, ptr::null(), &format!(r#"{{"format":"{name}"}}"#))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(result_bytes(result), expected, "{name} default bytes");
        let info = result_info(result);
        assert_eq!(info["carried"].as_u64().unwrap() as u32, carried, "{name}");
        assert_eq!(info["width"].as_u64().unwrap(), u64::from(W));
        assert_eq!(info["height"].as_u64().unwrap(), u64::from(H));
        assert_eq!(info["bit_depth"].as_u64().unwrap(), 8);
        assert_eq!(info["bytes"].as_u64().unwrap() as usize, expected.len());
        unsafe { dr_export_result_free(result) };
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn default_requests_reproduce_the_existing_projection16_export() {
    let dir = TempDir::new().expect("temp");
    let document = typed_doc(PROFILE_P3);
    let projection = typed_projection(document);
    for (name, code) in [("png", 0u32), ("tiff", 1u32)] {
        let path = dir.path().join(format!("today16.{name}"));
        let c = cpath(&path);
        let mut carried = 0u32;
        let mut err: *mut c_char = ptr::null_mut();
        assert!(
            unsafe {
                dr_precision_doc_save_projection16(
                    document,
                    projection,
                    c.as_ptr(),
                    code,
                    true,
                    false,
                    LIMIT,
                    0,
                    &mut carried,
                    std::ptr::null(),
                    &mut err,
                )
            },
            "{}",
            take_err_string(err)
        );
        let expected = std::fs::read(&path).expect("read");
        let result = encode_typed(
            document,
            projection,
            &format!(r#"{{"format":"{name}","bit_depth":16}}"#),
        )
        .unwrap_or_else(|e| panic!("{name}16: {e}"));
        assert_eq!(result_bytes(result), expected, "{name}16 default bytes");
        let info = result_info(result);
        assert_eq!(info["bit_depth"].as_u64().unwrap(), 16);
        assert_eq!(info["carried"].as_u64().unwrap() as u32, carried);
        unsafe { dr_export_result_free(result) };
    }
    unsafe { dr_precision_image_free(projection) };
    unsafe { dr_precision_doc_free(document) };
}

// ------------------------------------------------------ (d) the u16 chain --

#[test]
fn convert_only_tiff16_matches_the_shipped_conversion_and_export() {
    let dir = TempDir::new().expect("temp");
    let document = typed_doc(PROFILE_P3);
    let srgb = builtin(PROFILE_SRGB);
    let mut err: *mut c_char = ptr::null_mut();
    let converted = unsafe {
        dr_precision_doc_convert_to_profile(
            document,
            srgb.as_ptr(),
            srgb.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!converted.is_null(), "{}", take_err_string(err));
    let path = dir.path().join("converted.tiff");
    let c = cpath(&path);
    let mut carried = 0u32;
    assert!(
        unsafe {
            dr_precision_doc_export16(
                converted,
                c.as_ptr(),
                1,
                true,
                false,
                LIMIT,
                0,
                &mut carried,
                std::ptr::null(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    let expected = std::fs::read(&path).expect("read");
    let result = encode_typed(
        document,
        ptr::null(),
        r#"{"format":"tiff","bit_depth":16,"profile":"srgb"}"#,
    )
    .expect("encode");
    assert_eq!(result_bytes(result), expected);
    unsafe { dr_export_result_free(result) };
    unsafe { dr_precision_doc_free(converted) };
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn nearest_half_size_with_ordered_dither_matches_an_independent_oracle() {
    const BAYER: [u32; 64] = [
        0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38,
        60, 28, 52, 20, 62, 30, 54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57,
        25, 15, 47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
    ];
    let document = typed_doc(PROFILE_SRGB);
    let projection = typed_projection(document);
    let result = encode_typed(
        document,
        projection,
        r#"{"format":"png","bit_depth":8,"dither":"ordered",
            "filter":"nearest","size":{"mode":"scale","value":0.5}}"#,
    )
    .expect("encode");
    let (ow, oh) = (W / 2, H / 2);
    assert_eq!(
        (
            result_info(result)["width"].as_u64().unwrap() as u32,
            result_info(result)["height"].as_u64().unwrap() as u32
        ),
        (ow, oh)
    );
    let mut err: *mut c_char = ptr::null_mut();
    let decoded = unsafe { dr_export_result_preview(result, &mut err) };
    assert!(!decoded.is_null(), "{}", take_err_string(err));
    let actual = image_bytes(decoded);

    // Independent point selection (the exact rational pixel-centre rule) and
    // the ordered-dither formula, both written from their documented
    // definitions rather than from a recorded output.
    let source = samples16();
    let mut expected = Vec::new();
    for y in 0..oh as u64 {
        let sy = (2 * y + 1) * u64::from(H) / (2 * u64::from(oh));
        for x in 0..ow as u64 {
            let sx = (2 * x + 1) * u64::from(W) / (2 * u64::from(ow));
            let at = (sy * u64::from(W) + sx) as usize * 4;
            let index = (y * u64::from(ow) + x) as usize;
            let rank = BAYER[((index / ow as usize) & 7) * 8 + ((index % ow as usize) & 7)];
            for channel in 0..3 {
                let value = u32::from(source[at + channel]);
                expected.push(((value * 128 + (2 * rank + 1) * 257) / (257 * 128)).min(255) as u8);
            }
            expected.push(((u32::from(source[at + 3]) + 128) / 257) as u8);
        }
    }
    assert_eq!(actual, expected);

    unsafe { dr_image_free(decoded) };
    unsafe { dr_export_result_free(result) };
    unsafe { dr_precision_image_free(projection) };
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn the_u16_sharpen_agrees_with_its_byte_twin_within_one_code() {
    let document = typed_doc(PROFILE_SRGB);
    let projection = typed_projection(document);
    let request = r#"{"format":"png","bit_depth":16,
        "sharpen":{"radius":1.6,"amount":1.25,"threshold":0.02}}"#;
    let result = encode_typed(document, projection, request).expect("encode");
    let mut err: *mut c_char = ptr::null_mut();
    let decoded_typed = unsafe { dr_export_result_preview(result, &mut err) };
    assert!(!decoded_typed.is_null(), "{}", take_err_string(err));
    // `DrImage::open_bytes` reduces a 16-bit PNG to RGBA8 for the preview, so
    // the comparison is against the BYTE twin of the same kernel, one code.
    let typed_preview = image_bytes(decoded_typed);

    let pixels = plate();
    let source = image_of(W, H, &pixels);
    let byte = unsafe { unsharp_mask(source, 1.6, 1.25, 0.02, &mut err) };
    assert!(!byte.is_null(), "{}", take_err_string(err));
    let byte_pixels = image_bytes(byte);
    for (index, (a, b)) in typed_preview.iter().zip(byte_pixels.iter()).enumerate() {
        assert!(
            i32::from(*a) - i32::from(*b) <= 1 && i32::from(*b) - i32::from(*a) <= 1,
            "component {index}: typed {a} vs byte {b}"
        );
    }
    unsafe { dr_image_free(byte) };
    unsafe { dr_image_free(source) };
    unsafe { dr_image_free(decoded_typed) };
    unsafe { dr_export_result_free(result) };
    unsafe { dr_precision_image_free(projection) };
    unsafe { dr_precision_doc_free(document) };
}

// ------------------------------------------------------ (e) name patterns --

#[test]
fn name_patterns_expand_and_refuse() {
    assert_eq!(
        expand("{name}-{width}x{height}", TOKENS).unwrap(),
        "Beach-800x600"
    );
    assert_eq!(
        expand("{date} {preset} {name}.{format}", TOKENS).unwrap(),
        "2026-09-17 Web Beach.jpeg"
    );
    assert_eq!(expand("{name}", TOKENS).unwrap(), "Beach");
    for (pattern, needle) in [
        ("web-{width}", "must contain {name}"),
        ("{name}-{size}", "unknown token"),
        ("{name}-{width", "unterminated"),
        ("a/{name}", "plain file name"),
        ("a:{name}", "plain file name"),
        (".{name}", "beginning with a dot"),
        (
            "{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}{name}",
            "200 characters",
        ),
    ] {
        let error = expand(pattern, TOKENS).expect_err(pattern);
        assert!(error.contains(needle), "{pattern}: {error}");
    }
    // An empty {name} with nothing else expands to nothing.
    let empty =
        r#"{"name":"","width":1,"height":1,"format":"png","date":"2026-09-17","preset":""}"#;
    assert!(expand("{name}", empty)
        .expect_err("empty")
        .contains("non-empty"));
}

// ------------------------------------------------------- (f) cancellation --

#[test]
fn a_pre_cancelled_control_refuses_at_both_depths() {
    let pixels = plate();
    let doc = byte_doc(&pixels, PROFILE_SRGB);
    let token = unsafe { dr_job_control_create() };
    assert!(!token.is_null());
    unsafe { dr_job_control_cancel(token) };
    let json = CString::new(r#"{"format":"png"}"#).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_doc_export_encode(
            doc,
            ptr::null(),
            json.as_ptr(),
            token as *const DrJobControl,
            &mut err,
        )
    };
    assert!(result.is_null());
    assert_eq!(take_err_string(err), "operation cancelled");
    unsafe { dr_job_control_free(token) };
    unsafe { dr_doc_free(doc) };

    let document = typed_doc(PROFILE_SRGB);
    let projection = typed_projection(document);
    let token = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(token) };
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            document,
            projection,
            json.as_ptr(),
            LIMIT,
            0,
            token as *const DrJobControl,
            &mut err,
        )
    };
    assert!(result.is_null());
    assert_eq!(take_err_string(err), "operation cancelled");
    unsafe { dr_job_control_free(token) };
    unsafe { dr_precision_image_free(projection) };
    unsafe { dr_precision_doc_free(document) };
}

// ------------------------------------------------------------- (g) writes --

#[test]
fn writing_refuses_an_existing_file_and_is_exact_otherwise() {
    let dir = TempDir::new().expect("temp");
    let pixels = plate();
    let doc = byte_doc(&pixels, PROFILE_SRGB);
    let result = encode_byte(doc, ptr::null(), r#"{"format":"png"}"#).expect("encode");
    let bytes = result_bytes(result);

    let occupied = dir.path().join("occupied.png");
    std::fs::write(&occupied, b"not a png").expect("seed");
    let c = cpath(&occupied);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(!unsafe { dr_export_result_write(result, c.as_ptr(), false, &mut err) });
    assert!(take_err_string(err).contains("refusing to overwrite"));
    assert_eq!(std::fs::read(&occupied).expect("read"), b"not a png");

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_export_result_write(result, c.as_ptr(), true, &mut err) });
    assert!(err.is_null());
    assert_eq!(std::fs::read(&occupied).expect("read"), bytes);

    let fresh = dir.path().join("fresh.png");
    let c = cpath(&fresh);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_export_result_write(result, c.as_ptr(), false, &mut err) });
    assert_eq!(std::fs::read(&fresh).expect("read"), bytes);

    unsafe { dr_export_result_free(result) };
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------------ (h) preview --

#[test]
fn the_preview_decodes_the_bytes_that_will_be_written() {
    let pixels = plate();
    let doc = byte_doc(&pixels, PROFILE_SRGB);
    for request in [
        r#"{"format":"png","size":{"mode":"long_edge","value":13}}"#,
        r#"{"format":"jpeg","jpeg_quality":55,"size":{"mode":"scale","value":0.75}}"#,
        r#"{"format":"tiff","size":{"mode":"height","value":5}}"#,
    ] {
        let result = encode_byte(doc, ptr::null(), request).expect(request);
        let info = result_info(result);
        let mut err: *mut c_char = ptr::null_mut();
        let preview = unsafe { dr_export_result_preview(result, &mut err) };
        assert!(!preview.is_null(), "{}", take_err_string(err));
        assert_eq!(
            u64::from(unsafe { dr_image_width(preview) }),
            info["width"].as_u64().unwrap(),
            "{request}"
        );
        assert_eq!(
            u64::from(unsafe { dr_image_height(preview) }),
            info["height"].as_u64().unwrap(),
            "{request}"
        );
        unsafe { dr_image_free(preview) };
        unsafe { dr_export_result_free(result) };
    }
    unsafe { dr_doc_free(doc) };
}

// --------------------------------------------------------------- (i) NULL --

#[test]
fn every_export_export_tolerates_null() {
    let json = CString::new(r#"{"format":"png"}"#).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_export_output_size(4, 4, ptr::null(), &mut err) }.is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_export_expand_name(ptr::null(), ptr::null(), &mut err) }.is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_doc_export_encode(
            ptr::null(),
            ptr::null(),
            json.as_ptr(),
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert!(!take_err_string(err).is_empty());
    // A NULL request with a NULL document is still a clean refusal.
    assert!(unsafe {
        dr_doc_export_encode(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_export_plan(ptr::null(), ptr::null(), json.as_ptr(), &mut err)
    }
    .is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_export_encode(
            ptr::null(),
            ptr::null(),
            json.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert!(!take_err_string(err).is_empty());

    let mut length = 7usize;
    assert!(unsafe { dr_export_result_bytes(ptr::null(), &mut length) }.is_null());
    assert_eq!(length, 0);
    assert!(unsafe { dr_export_result_bytes(ptr::null(), ptr::null_mut()) }.is_null());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_export_result_info(ptr::null(), &mut err) }.is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_export_result_preview(ptr::null(), &mut err) }.is_null());
    assert!(!take_err_string(err).is_empty());

    let mut err: *mut c_char = ptr::null_mut();
    assert!(!unsafe { dr_export_result_write(ptr::null(), ptr::null(), false, &mut err) });
    assert!(!take_err_string(err).is_empty());

    unsafe { dr_export_result_free(ptr::null_mut()) };
}

// ----------------------------------------------------------------- (j) plan --

#[test]
fn the_typed_plan_charges_every_sharpen_plane() {
    let document = typed_doc(PROFILE_SRGB);
    let plan = |request: &str| -> serde_json::Value {
        let json = CString::new(request).expect("request");
        let mut err: *mut c_char = ptr::null_mut();
        let text =
            unsafe { dr_precision_doc_export_plan(document, ptr::null(), json.as_ptr(), &mut err) };
        assert!(!text.is_null(), "{}", take_err_string(err));
        let owned = unsafe { std::ffi::CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(text) };
        serde_json::from_str(&owned).expect("plan json")
    };
    let plain = plan(r#"{"format":"png","bit_depth":16}"#);
    let sharp = plan(
        r#"{"format":"png","bit_depth":16,"sharpen":{"radius":2.0,"amount":1.0,"threshold":0.0}}"#,
    );
    assert_eq!(plain["width"].as_u64().unwrap(), u64::from(W));
    assert_eq!(plain["height"].as_u64().unwrap(), u64::from(H));
    let pixels = u64::from(W) * u64::from(H);
    let delta = sharp["scratch_bytes"].as_u64().unwrap() - plain["scratch_bytes"].as_u64().unwrap();
    // The Gaussian's output plane plus its row ring, the
    // straight f32 input and the u16 output: at least 40 B/px.
    assert!(
        delta >= pixels * 40,
        "sharpen must charge at least 40 B/px: {delta} for {pixels} pixels"
    );
    // A bypass spends nothing.
    let bypass = plan(
        r#"{"format":"png","bit_depth":16,"sharpen":{"radius":0.0,"amount":2.0,"threshold":0.0}}"#,
    );
    assert_eq!(bypass["scratch_bytes"], plain["scratch_bytes"]);
    unsafe { dr_precision_doc_free(document) };
}

// ------------------------------------------- (k) the Phase 11C request keys --

/// An ICC profile assembled byte by byte, so a destination file's PARSE-time
/// refusals can be provoked without depending on what is installed. A THIRD
/// copy of the layout (the `icc_lut_tests` builder's own rule): if two of
/// them ever disagree, one is wrong.
fn icc_profile(space: &[u8; 4], tags: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let payload_start = 132 + 12 * tags.len();
    let mut table = Vec::new();
    let mut payload: Vec<u8> = Vec::new();
    for (sig, data) in tags {
        let offset = (payload_start + payload.len()) as u32;
        payload.extend_from_slice(data);
        while !payload.len().is_multiple_of(4) {
            payload.push(0);
        }
        table.extend_from_slice(sig.as_slice());
        table.extend_from_slice(&offset.to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    let total = (payload_start + payload.len()) as u32;
    let mut out = vec![0u8; 128];
    out[0..4].copy_from_slice(&total.to_be_bytes());
    out[8..12].copy_from_slice(&[0x02, 0x20, 0, 0]);
    out[12..16].copy_from_slice(b"spac");
    out[16..20].copy_from_slice(space);
    out[20..24].copy_from_slice(b"Lab ");
    out[36..40].copy_from_slice(b"acsp");
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&payload);
    out
}

fn tag_desc(name: &str) -> Vec<u8> {
    let mut t = b"desc\0\0\0\0".to_vec();
    t.extend_from_slice(&((name.len() + 1) as u32).to_be_bytes());
    t.extend_from_slice(name.as_bytes());
    t.push(0);
    t
}

/// A 2-point `mft2` identity table, `inputs` in and 3 out.
fn tag_mft2(inputs: usize) -> Vec<u8> {
    let mut t = b"mft2\0\0\0\0".to_vec();
    t.extend_from_slice(&[inputs as u8, 3, 2, 0]);
    for v in [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
        t.extend_from_slice(&((v * 65536.0).round() as i32).to_be_bytes());
    }
    t.extend_from_slice(&2u16.to_be_bytes());
    t.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..inputs {
        t.extend_from_slice(&0u16.to_be_bytes());
        t.extend_from_slice(&65535u16.to_be_bytes());
    }
    for _ in 0..2usize.pow(inputs as u32) * 3 {
        t.extend_from_slice(&32768u16.to_be_bytes());
    }
    for _ in 0..3 {
        t.extend_from_slice(&0u16.to_be_bytes());
        t.extend_from_slice(&65535u16.to_be_bytes());
    }
    t
}

/// The two new colour keys are parsed, reported and validated. The BYTES
/// they produce are `color_intent_tests`' subject; what is checked here is
/// that the request carries them through to the result's info, and that the
/// default request still says relative with BPC off.
#[test]
fn the_intent_and_bpc_keys_parse_and_reach_the_info() {
    let doc = byte_doc(&plate(), PROFILE_SRGB);
    // The pipeline renders the document itself when no projection is handed in.
    let flat = ptr::null();

    let info = |request: &str| -> serde_json::Value {
        let result = encode_byte(doc, flat, request).expect(request);
        let info = result_info(result);
        unsafe { dr_export_result_free(result) };
        info
    };

    // The default: today's conversion, named.
    let base = info(r#"{"format":"png","profile":"display-p3"}"#);
    assert_eq!(base["intent"], "relative");
    assert_eq!(base["black_point_compensation"], false);
    assert_eq!(base["profile"], "display-p3");

    for name in ["perceptual", "relative", "saturation", "absolute"] {
        let one = info(&format!(
            r#"{{"format":"png","profile":"display-p3","intent":"{name}","black_point_compensation":true}}"#
        ));
        assert_eq!(one["intent"], name);
        assert_eq!(one["black_point_compensation"], true);
    }

    // An unknown intent names the four, and a non-boolean BPC refuses
    // rather than being read as true.
    let refusal = encode_byte(
        doc,
        flat,
        r#"{"format":"png","profile":"display-p3","intent":"colorimetric"}"#,
    )
    .unwrap_err();
    assert!(
        refusal.contains("perceptual") && refusal.contains("absolute"),
        "{refusal}"
    );
    assert!(encode_byte(
        doc,
        flat,
        r#"{"format":"png","black_point_compensation":"yes"}"#
    )
    .is_err());

    unsafe { dr_doc_free(doc) };
}

/// `profile: "file"` with a profile_path is the same destination as naming
/// the built-in — the bytes are equal — and the info reports the file's own
/// name.
#[test]
fn a_destination_profile_read_from_a_file_equals_the_builtin_path() {
    let doc = byte_doc(&plate(), PROFILE_SRGB);
    // The pipeline renders the document itself when no projection is handed in.
    let flat = ptr::null();
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("p3.icc");
    std::fs::write(&path, builtin(PROFILE_P3)).expect("write profile");

    let built_in = encode_byte(doc, flat, r#"{"format":"png","profile":"display-p3"}"#).unwrap();
    let from_file = encode_byte(
        doc,
        flat,
        &format!(
            r#"{{"format":"png","profile":"file","profile_path":{}}}"#,
            serde_json::Value::from(path.to_string_lossy().as_ref())
        ),
    )
    .unwrap();
    assert_eq!(result_bytes(built_in), result_bytes(from_file));
    let info = result_info(from_file);
    assert_eq!(info["profile"], "file");
    assert_eq!(info["profile_name"], result_info(built_in)["profile_name"]);
    unsafe { dr_export_result_free(built_in) };
    unsafe { dr_export_result_free(from_file) };

    unsafe { dr_doc_free(doc) };
}

/// Every one of `OutputProfile::from_path`'s refusals fires at PARSE time,
/// so a bad destination costs no render and no encode.
#[test]
fn a_destination_profile_file_refuses_before_anything_is_encoded() {
    let doc = byte_doc(&plate(), PROFILE_SRGB);
    // The pipeline renders the document itself when no projection is handed in.
    let flat = ptr::null();
    let dir = TempDir::new().expect("tempdir");

    let refuse =
        |request: String| -> String { encode_byte(doc, flat, &request).expect_err(&request) };
    let with_path = |path: &std::path::Path| -> String {
        format!(
            r#"{{"format":"png","profile":"file","profile_path":{}}}"#,
            serde_json::Value::from(path.to_string_lossy().as_ref())
        )
    };

    // No path at all.
    let missing = refuse(r#"{"format":"png","profile":"file"}"#.into());
    assert!(missing.contains("profile_path"), "{missing}");

    // Unreadable.
    let absent = dir.path().join("nope.icc");
    let unreadable = refuse(with_path(&absent));
    assert!(unreadable.contains("failed to read"), "{unreadable}");

    // Larger than the 16 MiB limit a document may carry.
    let huge = dir.path().join("huge.icc");
    std::fs::write(&huge, vec![0u8; 16 * 1024 * 1024 + 1]).expect("write huge");
    let too_big = refuse(with_path(&huge));
    assert!(too_big.contains("16 MiB limit"), "{too_big}");

    // Not an RGB profile: a Gray one parses as a profile and is refused as
    // a destination this build can carry.
    let gray = dir.path().join("gray.icc");
    std::fs::write(
        &gray,
        icc_profile(b"GRAY", &[(b"desc", tag_desc("Gray LUT"))]),
    )
    .expect("write gray");
    let not_rgb = refuse(with_path(&gray));
    assert!(not_rgb.contains("not an RGB ICC profile"), "{not_rgb}");

    // An RGB profile with an A2B0 and NO B2A: decodable, not encodable, so
    // it may tag a document but may not be converted INTO.
    let decode_only = dir.path().join("decode-only.icc");
    std::fs::write(
        &decode_only,
        icc_profile(
            b"RGB ",
            &[(b"desc", tag_desc("Decode only")), (b"A2B0", tag_mft2(3))],
        ),
    )
    .expect("write decode-only");
    let unencodable = refuse(with_path(&decode_only));
    assert!(unencodable.contains("convert INTO"), "{unencodable}");

    unsafe { dr_doc_free(doc) };
}
