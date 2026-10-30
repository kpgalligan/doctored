//! An export's side limit is the FORMAT's own — 65535 for JPEG and GIF,
//! 16384 for WebP, 130048 for HEIC (ImageIO's encoder, measured) — and PNG, TIFF and
//! BMP are bounded by the pixel count alone, so a long panorama strip exports.
//! Black-box through the FFI.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_string_free};
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_export::*;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

fn output_size(w: u32, h: u32, request: &str) -> Result<(u32, u32), String> {
    let json = CString::new(request).expect("request");
    let mut err: *mut c_char = ptr::null_mut();
    let text = unsafe { dr_export_output_size(w, h, json.as_ptr(), &mut err) };
    if text.is_null() {
        return Err(take_err_string(err));
    }
    let owned = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    let value: serde_json::Value = serde_json::from_str(&owned).expect("size json");
    Ok((
        value["width"].as_u64().expect("width") as u32,
        value["height"].as_u64().expect("height") as u32,
    ))
}

#[test]
fn each_capped_format_accepts_its_cap_and_refuses_one_more() {
    for (format, cap) in [
        ("jpeg", 65_535u32),
        ("gif", 65_535),
        ("webp", 16_384),
        ("heic", 130_048),
    ] {
        let request = format!(r#"{{"format":"{format}"}}"#);
        assert_eq!(
            output_size(cap, 2, &request).unwrap(),
            (cap, 2),
            "{format} accepts {cap} on a side"
        );
        assert_eq!(
            output_size(2, cap, &request).unwrap(),
            (2, cap),
            "{format} accepts {cap} tall"
        );
        for (w, h) in [(cap + 1, 2), (2, cap + 1)] {
            let err = output_size(w, h, &request).unwrap_err();
            assert_eq!(
                err,
                format!("export dimensions must be at most {cap} pixels on a side for {format}"),
                "{format} refuses {w}x{h}"
            );
        }
    }
}

#[test]
fn png_tiff_and_bmp_have_no_side_cap() {
    for format in ["png", "tiff", "bmp"] {
        let request = format!(r#"{{"format":"{format}"}}"#);
        assert_eq!(
            output_size(40_000, 2_400, &request).unwrap(),
            (40_000, 2_400),
            "{format} accepts a 40000 px strip"
        );
        assert_eq!(
            output_size(2_400, 40_000, &request).unwrap(),
            (2_400, 40_000)
        );
        // The pixel ceiling still refuses.
        let err = output_size(100_000, 1_001, &request).unwrap_err();
        assert!(err.contains("pixel limit"), "{format}: {err}");
        let resized =
            format!(r#"{{"format":"{format}","size":{{"mode":"width","value":200000}}}}"#);
        let err = output_size(40_000, 2_400, &resized).unwrap_err();
        assert!(err.contains("pixel limit"), "{format}: {err}");
    }
}

#[test]
fn a_long_byte_document_exports_to_png_at_full_size() {
    let dir = TempDir::new().unwrap();
    let strip = RgbaImage::from_fn(40_000, 2, |x, y| {
        Rgba([(x % 251) as u8, (y * 90) as u8, 40, 255])
    });
    let doc = doc_from(&dir, "strip.png", &strip);
    let json = CString::new(r#"{"format":"png"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result =
        unsafe { dr_doc_export_encode(doc, ptr::null(), json.as_ptr(), ptr::null(), &mut err) };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let mut err: *mut c_char = ptr::null_mut();
    let preview = unsafe { dr_export_result_preview(result, &mut err) };
    assert!(!preview.is_null(), "{}", take_err_string(err));
    assert_eq!(img_dims(preview), (40_000, 2));
    assert_eq!(img_pixels(preview), strip.into_raw());
    unsafe {
        dr_image_free(preview);
        dr_export_result_free(result);
        dr_doc_free(doc);
    }
}
