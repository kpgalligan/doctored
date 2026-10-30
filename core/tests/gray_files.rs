//! #1 phase 1a: which gray PNG, TIFF and JPEG files open as Grayscale
//! documents (`dr_path_gray_source`, header work only) and what the open
//! keeps — the codes copied into R = G = B exactly, the embedded Gray ICC,
//! a PNG `sRGB` chunk as sGray, the working Gray with its sentence — and the
//! `.rz` round trip at 8, 16 and 32 bits. Every fixture is written here
//! (`common/gray_fixture.rs`); the expected samples are the file's own codes.

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gray::*;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_info,
    dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{gama_chunk, gray_icc, gray_tiff, iccp, png, srgb_chunk};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn source(path: &Path) -> i32 {
    let c = cpath(path);
    unsafe { dr_path_gray_source(c.as_ptr()) }
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn open_gray(path: &Path, working: &str) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let working = CString::new(working).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn json(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err)
}

fn info(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json(unsafe { dr_precision_doc_info(doc, &mut err) }, err)
}

fn samples(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn ramp(count: usize, step: u16) -> Vec<u16> {
    (0..count).map(|i| (i as u16).wrapping_mul(step)).collect()
}

#[test]
fn the_source_question_answers_exactly_the_gray_files_open_gray_accepts() {
    let dir = TempDir::new().unwrap();
    let gray8 = ramp(16, 17);
    let gray16 = ramp(16, 4099);
    let rgb_icc = IccProfile::srgb().bytes().to_vec();
    let none = [
        ("one_bit.png", png(4, 4, 1, 0, 1, &[], &[])),
        ("four_bit.png", png(4, 4, 4, 0, 1, &[], &[])),
        (
            "rgb_iccp.png",
            png(4, 4, 8, 0, 1, &gray8, &[iccp(&rgb_icc)]),
        ),
        (
            "bare_gama.png",
            png(4, 4, 16, 0, 1, &gray16, &[gama_chunk()]),
        ),
        ("rgb.png", png(2, 2, 8, 2, 3, &ramp(12, 20), &[])),
        (
            "white_is_zero.tif",
            gray_tiff(4, 4, 8, 1, 0, None, None, &[0u8; 16], None),
        ),
        (
            "float.tif",
            gray_tiff(2, 2, 32, 1, 1, None, Some(3), &[0u8; 16], None),
        ),
    ];
    for (name, bytes) in &none {
        assert_eq!(source(&write(&dir, name, bytes)), 0, "{name}");
    }
    let gray_a16: Vec<u16> = gray16.iter().flat_map(|&g| [g, 65535 - g]).collect();
    let tiff16: Vec<u8> = gray16.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
        .encode(&[90u8; 64], 8, 8, image::ExtendedColorType::L8)
        .unwrap();
    let some = [
        ("gray8.png", png(4, 4, 8, 0, 1, &gray8, &[]), 1),
        ("gray16.png", png(4, 4, 16, 0, 1, &gray16, &[]), 1),
        ("gray_a16.png", png(4, 4, 16, 4, 2, &gray_a16, &[]), 1),
        (
            "gray16.tif",
            gray_tiff(4, 4, 16, 1, 1, None, None, &tiff16, None),
            2,
        ),
        ("gray.jpg", jpeg, 3),
    ];
    for (name, bytes, expected) in &some {
        assert_eq!(source(&write(&dir, name, bytes)), *expected, "{name}");
    }
    assert_eq!(unsafe { dr_path_gray_source(ptr::null()) }, 0);
    assert_eq!(source(&dir.path().join("missing.png")), 0);
}

#[test]
fn an_open_copies_the_codes_and_keeps_or_assigns_the_profile() {
    let dir = TempDir::new().unwrap();
    // 8-bit, untagged: the codes ×257 in R = G = B, the working Gray assigned
    // under its sentence.
    let gray8 = ramp(16, 17);
    let doc = open_gray(
        &write(&dir, "u8.png", &png(4, 4, 8, 0, 1, &gray8, &[])),
        "dot_gain_20",
    )
    .unwrap();
    let d = descriptor(doc);
    assert_eq!(d["color_mode"], "gray");
    assert_eq!(d["gray"]["depth"], 8);
    assert_eq!(d["gray"]["profile"], "Dot Gain 20%");
    for (pixel, code) in samples(doc).iter().zip(&gray8) {
        assert_eq!(*pixel, [code * 257, code * 257, code * 257, 65535]);
    }
    let warnings = info(doc)["import_warnings"].clone();
    assert_eq!(
        warnings[0],
        "The image is grayscale with no usable Gray profile, so the working Gray “Dot Gain 20%” was assigned as it was read."
    );
    unsafe { dr_precision_doc_free(doc) };
    // 16-bit GrayA with an embedded Gray ICC: kept, codes and alpha exact.
    let icc = gray_icc("Test Gray 1.8", 1.8);
    let gray16 = ramp(16, 4099);
    let gray_a: Vec<u16> = gray16.iter().flat_map(|&g| [g, 65535 - g]).collect();
    let doc = open_gray(
        &write(
            &dir,
            "a16.png",
            &png(4, 4, 16, 4, 2, &gray_a, &[iccp(&icc)]),
        ),
        "dot_gain_20",
    )
    .unwrap();
    let d = descriptor(doc);
    assert_eq!(d["gray"]["profile"], "Test Gray 1.8");
    assert_eq!(d["gray"]["depth"], 16);
    for (pixel, g) in samples(doc).iter().zip(&gray16) {
        assert_eq!(*pixel, [*g, *g, *g, 65535 - g]);
    }
    assert!(info(doc)["import_warnings"].as_array().unwrap().is_empty());
    unsafe { dr_precision_doc_free(doc) };
    // An sRGB chunk is sGray.
    let doc = open_gray(
        &write(
            &dir,
            "srgb.png",
            &png(4, 4, 8, 0, 1, &gray8, &[srgb_chunk(), gama_chunk()]),
        ),
        "dot_gain_20",
    )
    .unwrap();
    assert_eq!(descriptor(doc)["gray"]["profile"], "sGray");
    unsafe { dr_precision_doc_free(doc) };
    // A BlackIsZero TIFF 16 with a Gray ICC, and a one-component JPEG.
    let tiff16: Vec<u8> = gray16.iter().flat_map(|v| v.to_le_bytes()).collect();
    let doc = open_gray(
        &write(
            &dir,
            "g.tif",
            &gray_tiff(4, 4, 16, 1, 1, None, None, &tiff16, Some(&icc)),
        ),
        "gray_gamma_22",
    )
    .unwrap();
    assert_eq!(descriptor(doc)["gray"]["profile"], "Test Gray 1.8");
    for (pixel, g) in samples(doc).iter().zip(&gray16) {
        assert_eq!(*pixel, [*g, *g, *g, 65535]);
    }
    unsafe { dr_precision_doc_free(doc) };
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
        .encode(&[90u8; 64], 8, 8, image::ExtendedColorType::L8)
        .unwrap();
    let doc = open_gray(&write(&dir, "g.jpg", &jpeg), "gray_gamma_22").unwrap();
    let d = descriptor(doc);
    assert_eq!(
        (d["gray"]["depth"].clone(), d["gray"]["profile"].clone()),
        (8.into(), "Gray Gamma 2.2".into())
    );
    for pixel in samples(doc) {
        assert!(pixel[0] == pixel[1] && pixel[1] == pixel[2] && pixel[0] % 257 == 0);
    }
    unsafe { dr_precision_doc_free(doc) };
    // Not a gray file, and an unknown working space, refuse by name.
    let rgb = write(&dir, "rgb.png", &png(2, 2, 8, 2, 3, &ramp(12, 20), &[]));
    assert!(open_gray(&rgb, "dot_gain_20")
        .unwrap_err()
        .contains("is not a grayscale PNG, TIFF or JPEG"));
    assert_eq!(
        open_gray(
            &write(&dir, "x.png", &png(4, 4, 8, 0, 1, &gray8, &[])),
            "dot_gain_99"
        )
        .unwrap_err(),
        "\"dot_gain_99\" is not a Gray working space"
    );
}

#[test]
fn the_native_file_round_trips_a_gray_document_at_8_16_and_32_bits() {
    let dir = TempDir::new().unwrap();
    let icc = gray_icc("Round Trip Gray", 2.2);
    let gray16 = ramp(16, 4099);
    let gray8 = ramp(16, 17);
    for (name, file) in [
        ("n16.png", png(4, 4, 16, 0, 1, &gray16, &[iccp(&icc)])),
        ("n8.png", png(4, 4, 8, 0, 1, &gray8, &[])),
    ] {
        let doc = open_gray(&write(&dir, name, &file), "gray_gamma_18").unwrap();
        let before = (descriptor(doc)["gray"].clone(), samples(doc));
        let rz = dir.path().join(format!("{name}.rz"));
        let c = cpath(&rz);
        let mut err = ptr::null_mut();
        assert!(
            unsafe {
                dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
            },
            "{}",
            take_err_string(err)
        );
        let reopened =
            unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
        assert!(!reopened.is_null(), "{}", take_err_string(err));
        let d = descriptor(reopened);
        assert_eq!(d["color_mode"], "gray");
        assert_eq!((d["gray"].clone(), samples(reopened)), before, "{name}");
        unsafe { dr_precision_doc_free(reopened) };
        unsafe { dr_precision_doc_free(doc) };
    }
    // 32 bits: New Grayscale at 32 promotes to float and keeps its depth.
    let request = CString::new(
        r#"{"depth":32,"space":"dot_gain_15","fill_srgb":[0.5,0.5,0.5,1],"layer_name":"Background","ppi":300}"#,
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_gray(4, 3, ptr::null(), 0, request.as_ptr(), LIMIT, 0, &mut err)
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let rz = dir.path().join("n32.rz");
    let c = cpath(&rz);
    assert!(unsafe {
        dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    });
    let reopened = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    let d = descriptor(reopened);
    assert_eq!(d["gray"]["depth"], 32);
    assert_eq!(d["storage"], descriptor(doc)["storage"]);
    let a = unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap();
    let b = unsafe { &*reopened }
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap();
    assert_eq!(a, b);
    unsafe { dr_precision_doc_free(reopened) };
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn the_native_reader_refuses_a_gray_table_that_disagrees_with_the_body() {
    let dir = TempDir::new().unwrap();
    let file = png(4, 4, 16, 0, 1, &ramp(16, 4099), &[]);
    let doc = open_gray(&write(&dir, "r16.png", &file), "gray_gamma_22").unwrap();
    let rz = dir.path().join("r16.rz");
    let c = cpath(&rz);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    unsafe { dr_precision_doc_free(doc) };
    let bytes = std::fs::read(&rz).unwrap();
    // The GRAY table: tag, schema u32 = 1, depth u8, profile length u32,
    // then the Gray ICC (whose header's minor version byte is at 9).
    let at = bytes
        .windows(8)
        .rposition(|w| w == b"GRAY\x01\x00\x00\x00")
        .expect("GRAY table");
    assert_eq!(bytes[at + 8], 16);
    let reopen = |patch: &dyn Fn(&mut Vec<u8>), name: &str| {
        let mut patched = bytes.clone();
        patch(&mut patched);
        let path = write(&dir, name, &patched);
        let c = cpath(&path);
        let mut err = ptr::null_mut();
        let out = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
        assert!(out.is_null(), "{name} opened");
        take_err_string(err)
    };
    assert_eq!(
        reopen(&|b| b[at + 4] = 2, "schema.rz"),
        "unsupported native Gray schema"
    );
    assert_eq!(
        reopen(&|b| b[at + 8] = 32, "depth.rz"),
        "native Gray depth does not match the document's storage"
    );
    assert_eq!(
        reopen(&|b| b[at + 13 + 9] ^= 0x10, "profile.rz"),
        "native Gray document profile is not its Gray profile's twin"
    );
}
