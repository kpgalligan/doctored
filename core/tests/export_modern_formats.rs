//! The export workspace's two modern formats through the C API: AVIF,
//! host-encoded exactly as HEIC is, and lossless JPEG XL, encoded by the core.
//!
//! The core decodes neither, so nothing here trusts a decoder of its own: the
//! AVIF result is checked as the pending TIFF intermediate it is (byte-equal
//! to the `tiff` request's), and the JPEG XL bytes are decoded by the
//! reference `djxl` when it is installed (skipped otherwise) and compared
//! with the source samples, which a lossless encode must return exactly.
use std::ffi::{c_char, CString};
use std::process::Command;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_open, dr_image_save};
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_export_result_bytes, dr_export_result_free, dr_export_result_info,
    dr_export_result_preview, dr_export_result_write,
};
use doctored_core::icc::IccProfile;
use image::RgbaImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};

const W: u32 = 7;
const H: u32 = 5;

/// Varied samples; `alpha` selects an opaque plate or one with soft alpha.
fn plate(alpha: bool) -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        let i = x + y * W;
        let a = if alpha {
            255 - (i * 7 % 200) as u8
        } else {
            255
        };
        image::Rgba([
            (i * 31 % 256) as u8,
            (i * 57 % 256) as u8,
            (i * 91 % 256) as u8,
            a,
        ])
    })
}

fn encode(document: &DrDocument, request: &str) -> Result<*mut ExportResult, String> {
    let json = CString::new(request).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_doc_export_encode(
            document,
            ptr::null(),
            json.as_ptr(),
            ptr::null(),
            &mut error,
        )
    };
    if result.is_null() {
        Err(take_err_string(error))
    } else {
        Ok(result)
    }
}

fn bytes(result: *const ExportResult) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

fn info(result: *const ExportResult) -> Value {
    let mut error: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_export_result_info(result, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}

fn preview_refusal(result: *const ExportResult) -> String {
    let mut error: *mut c_char = ptr::null_mut();
    let preview = unsafe { dr_export_result_preview(result, &mut error) };
    assert!(preview.is_null(), "the core decodes neither format");
    take_err_string(error)
}

/// `djxl` decoding `jxl` to straight RGBA8, or `None` when it is not installed.
fn djxl(directory: &TempDir, jxl: &[u8]) -> Option<Vec<u8>> {
    let (input, output) = (
        directory.path().join("in.jxl"),
        directory.path().join("out.png"),
    );
    std::fs::write(&input, jxl).unwrap();
    let status = Command::new("djxl")
        .arg(&input)
        .arg(&output)
        .output()
        .ok()?;
    assert!(
        status.status.success(),
        "djxl refused the codestream: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let mut error = ptr::null_mut();
    let image = unsafe { dr_image_open(cpath(&output).as_ptr(), &mut error) };
    assert!(!image.is_null(), "{}", take_err_string(error));
    let pixels = unsafe {
        let (w, h) = (
            doctored_core::ffi::dr_image_width(image),
            doctored_core::ffi::dr_image_height(image),
        );
        std::slice::from_raw_parts(
            doctored_core::ffi::dr_image_pixels_rgba(image),
            w as usize * h as usize * 4,
        )
        .to_vec()
    };
    unsafe { dr_image_free(image) };
    Some(pixels)
}

// --------------------------------------------------------------- JPEG XL --

#[test]
fn a_jxl_result_is_finished_and_is_previewed_by_the_host() {
    let directory = TempDir::new().unwrap();
    for alpha in [false, true] {
        let source = plate(alpha);
        let document = DrDocument::from_pixels(source.clone());
        let result = encode(&document, r#"{"format":"jxl"}"#).expect("jxl");
        let info = info(result);
        assert_eq!(info["format"], Value::from("jxl"));
        assert_eq!(info["bit_depth"], Value::from(8));
        assert_eq!(info["host_preview"], Value::Bool(true));
        assert_eq!(
            info["host_encoded"],
            Value::Bool(false),
            "encoded by the core"
        );
        assert_eq!(info["intermediate"], Value::Null, "never pending");
        assert_eq!(
            info["carried"],
            Value::from(0),
            "a bare codestream carries nothing"
        );
        assert_eq!(
            preview_refusal(result),
            "a JPEG XL export is previewed by the host"
        );

        let encoded = bytes(result);
        assert_eq!(&encoded[..2], &[0xff, 0x0a], "a bare JPEG XL codestream");
        // It is written as it is: no adoption step.
        let path = directory.path().join(format!("out{alpha}.jxl"));
        let mut error: *mut c_char = ptr::null_mut();
        assert!(
            unsafe { dr_export_result_write(result, cpath(&path).as_ptr(), false, &mut error) },
            "{}",
            take_err_string(error)
        );
        assert_eq!(std::fs::read(&path).unwrap(), encoded);

        match djxl(&directory, &encoded) {
            Some(decoded) => assert_eq!(decoded, source.into_raw(), "lossless, alpha {alpha}"),
            None => eprintln!("SKIP: djxl is not installed; the JPEG XL decode check did not run"),
        }
        unsafe { dr_export_result_free(result) };
    }
}

#[test]
fn jxl_refuses_sixteen_bits_a_one_pixel_side_and_pixels_that_are_not_srgb() {
    let document = DrDocument::from_pixels(plate(false));
    assert_eq!(
        encode(&document, r#"{"format":"jxl","bit_depth":16}"#).unwrap_err(),
        "a JPEG XL export is 8-bit only; set bit_depth to 8"
    );

    let strip = DrDocument::from_pixels(RgbaImage::new(9, 1));
    assert_eq!(
        encode(&strip, r#"{"format":"jxl"}"#).unwrap_err(),
        "a JPEG XL export needs at least 2 pixels on each side"
    );

    let mut wide = DrDocument::from_pixels(plate(false));
    wide.profile = IccProfile::display_p3();
    assert_eq!(
        encode(&wide, r#"{"format":"jxl"}"#).unwrap_err(),
        "JPEG XL is written as sRGB; set the export profile to sRGB"
    );
    // Converting to sRGB is the route the sentence names.
    let converted = encode(&wide, r#"{"format":"jxl","profile":"srgb"}"#).expect("converted");
    assert_eq!(info(converted)["profile"], Value::from("srgb"));
    unsafe { dr_export_result_free(converted) };
}

// ------------------------------------------------------------------ AVIF --

#[test]
fn avif_is_a_pending_host_export_holding_the_tiff_requests_bytes() {
    let directory = TempDir::new().unwrap();
    let document = DrDocument::from_pixels(plate(true));
    let avif = encode(&document, r#"{"format":"avif"}"#).expect("avif");
    let tiff = encode(&document, r#"{"format":"tiff"}"#).expect("tiff");
    let info = info(avif);
    assert_eq!(info["format"], Value::from("avif"));
    assert_eq!(info["host_encoded"], Value::Bool(true));
    assert_eq!(info["host_preview"], Value::Bool(true));
    assert_eq!(info["intermediate"], Value::from("tiff"));
    assert_eq!(
        bytes(avif),
        bytes(tiff),
        "the intermediate is the tiff request's bytes"
    );
    assert_eq!(
        preview_refusal(avif),
        "a host-encoded export is previewed by the host"
    );
    let path = directory.path().join("pending.avif");
    let mut error: *mut c_char = ptr::null_mut();
    assert!(!unsafe { dr_export_result_write(avif, cpath(&path).as_ptr(), false, &mut error) });
    assert_eq!(
        take_err_string(error),
        "a host-encoded export must adopt the host encoder's bytes before it is written"
    );
    assert!(!path.exists());
    // An ordinary result is previewed by the core and says so.
    assert_eq!(self::info(tiff)["host_preview"], Value::Bool(false));
    unsafe { dr_export_result_free(avif) };
    unsafe { dr_export_result_free(tiff) };

    // 16 bits is admitted, as for HEIC; the side cap is AVIF's own.
    // (An 8-bit document cannot export at 16 at all, which is the refusal a
    // 16-bit AVIF request reaches here; a format that is 8-bit only is refused
    // earlier, by the format rule that now lists avif.)
    assert_eq!(
        encode(&document, r#"{"format":"avif","bit_depth":16}"#).unwrap_err(),
        "an 8-bit document exports at 8 bits; convert it to 16-bit first"
    );
    assert_eq!(
        encode(&document, r#"{"format":"bmp","bit_depth":16}"#).unwrap_err(),
        "a 16-bit export requires png, tiff, heic or avif; every other format is 8-bit only"
    );
    let long = DrDocument::from_pixels(RgbaImage::new(16_385, 2));
    assert_eq!(
        encode(&long, r#"{"format":"avif"}"#).unwrap_err(),
        "export dimensions must be at most 16384 pixels on a side for avif"
    );
}

#[test]
fn the_flat_writers_refuse_both_formats_by_name() {
    let directory = TempDir::new().unwrap();
    let pixels = plate(false).into_raw();
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), W, H) };
    for (format, name, sentence) in [
        (
            7,
            "flat.avif",
            "AVIF is encoded by the host through the export workspace (format \"avif\")",
        ),
        (
            8,
            "flat.jxl",
            "JPEG XL is written through the export workspace (format \"jxl\")",
        ),
        (
            9,
            "flat.exr",
            "OpenEXR is a 32-bit float deliverable written through the export workspace (format \"exr\")",
        ),
        (
            10,
            "flat.hdr",
            "Radiance HDR is a 32-bit float deliverable written through the export workspace (format \"hdr\")",
        ),
    ] {
        let path = directory.path().join(name);
        let mut error: *mut c_char = ptr::null_mut();
        assert!(!unsafe { dr_image_save(image, cpath(&path).as_ptr(), format, 90, &mut error) });
        assert_eq!(take_err_string(error), sentence);
        assert!(!path.exists(), "a refused save writes nothing");
    }
    unsafe { dr_image_free(image) };

    // An unknown export format names the whole list.
    let document = DrDocument::from_pixels(plate(false));
    assert_eq!(
        encode(&document, r#"{"format":"jp2"}"#).unwrap_err(),
        "export format \"jp2\" is not one of png, jpeg, tiff, bmp, gif, webp, heic, avif, jxl, exr or hdr"
    );
    // NULL is tolerated by the preview of either format.
    let mut error: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_export_result_preview(ptr::null(), &mut error) }.is_null());
    take_err_string(error);
}
