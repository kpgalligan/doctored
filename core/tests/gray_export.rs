//! #1 phase 1b, a Grayscale document's one-channel export: profile
//! `document` with PNG (8/16; opaque → gray, transparent → gray + alpha),
//! TIFF (8/16) and JPEG (8) writes ONE channel with the document's Gray ICC
//! verbatim, each header parsed by hand here from the PNG, TIFF 6.0 and
//! JPEG specifications; the file is a gray source (`dr_path_gray_source`)
//! and reopens through `dr_precision_doc_open_gray` with the same profile
//! name and the same codes (PNG and TIFF exact, JPEG quality 100 within 2 of
//! 255); profile `srgb` writes RGB; a kept WebP and a 32-bit kept TIFF
//! refuse with the exact sentence. Bounds declared before measuring.

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_export_result_info,
    dr_precision_doc_export_encode,
};
use doctored_core::ffi_gray::*;
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_free};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::gray_icc;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 6;
const H: u32 = 3;

fn json_of(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err)
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

/// A Gray document (the test profile, `depth`) of a colour ramp, alpha
/// ramping from opaque when `transparent` (never to zero).
fn gray_doc(icc: &[u8], depth: u32, transparent: bool) -> *mut PrecisionDocument {
    let n = (W * H) as usize;
    let pixels: Vec<[u16; 4]> = (0..n)
        .map(|i| {
            let i = i as u16;
            let alpha = if transparent { 65535 - i * 3000 } else { 65535 };
            [i * 3500, 60000 - i * 2900, (i * 7919) % 65535, alpha]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let mut err = ptr::null_mut();
    // 32 bits converts from the promoted float document.
    let rgb = if depth == 32 {
        let float =
            unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!float.is_null(), "{}", take_err_string(err));
        free(rgb);
        float
    } else {
        rgb
    };
    let options = if depth == 32 {
        "{}".to_string()
    } else {
        format!(r#"{{"depth":{depth}}}"#)
    };
    let options = CString::new(options).unwrap();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            rgb,
            icc.as_ptr(),
            icc.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "{}", take_err_string(err));
    free(rgb);
    gray
}

fn export(doc: *const PrecisionDocument, request: &str) -> Result<(Vec<u8>, Value), String> {
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let result: *mut ExportResult = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    let mut len = 0usize;
    let bytes = unsafe {
        let p = dr_export_result_bytes(result, &mut len);
        std::slice::from_raw_parts(p, len).to_vec()
    };
    let info = json_of(unsafe { dr_export_result_info(result, &mut err) }, err);
    unsafe { dr_export_result_free(result) };
    Ok((bytes, info))
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// PNG: (bit depth, colour type, inflated iCCP).
fn png_header(bytes: &[u8]) -> (u8, u8, Vec<u8>) {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut at = 8;
    let (mut depth, mut colour, mut icc) = (0, 0, Vec::new());
    while at + 8 <= bytes.len() {
        let len = be32(&bytes[at..]) as usize;
        let kind = &bytes[at + 4..at + 8];
        let data = &bytes[at + 8..at + 8 + len];
        if kind == b"IHDR" {
            depth = data[8];
            colour = data[9];
        } else if kind == b"iCCP" {
            let name_end = data.iter().position(|&b| b == 0).unwrap();
            assert_eq!(data[name_end + 1], 0, "compression method");
            icc = miniz_oxide::inflate::decompress_to_vec_zlib(&data[name_end + 2..]).unwrap();
        }
        at += 12 + len;
    }
    (depth, colour, icc)
}

/// TIFF (either byte order): (SamplesPerPixel, Photometric, BitsPerSample
/// first value, ExtraSamples first value, ICC tag 34675).
fn tiff_header(bytes: &[u8]) -> (u16, u16, u16, Option<u16>, Vec<u8>) {
    let little = &bytes[..2] == b"II";
    let u16_at = |at: usize| {
        if little {
            u16::from_le_bytes([bytes[at], bytes[at + 1]])
        } else {
            be16(&bytes[at..])
        }
    };
    let u32_at = |at: usize| {
        let b = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
        if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    assert_eq!(u16_at(2), 42);
    let ifd = u32_at(4) as usize;
    let count = u16_at(ifd) as usize;
    let (mut spp, mut photometric, mut bits, mut extra, mut icc) = (1, 99, 0, None, Vec::new());
    for e in 0..count {
        let at = ifd + 2 + e * 12;
        let tag = u16_at(at);
        let n = u32_at(at + 4) as usize;
        match tag {
            277 => spp = u16_at(at + 8),
            262 => photometric = u16_at(at + 8),
            258 => {
                bits = if n <= 2 {
                    u16_at(at + 8)
                } else {
                    u16_at(u32_at(at + 8) as usize)
                }
            }
            338 => extra = Some(u16_at(at + 8)),
            34675 => {
                let off = if n <= 4 {
                    at + 8
                } else {
                    u32_at(at + 8) as usize
                };
                icc = bytes[off..off + n].to_vec();
            }
            _ => {}
        }
    }
    (spp, photometric, bits, extra, icc)
}

/// JPEG: (SOF component count, the APP2 ICC_PROFILE chunks joined).
fn jpeg_header(bytes: &[u8]) -> (u8, Vec<u8>) {
    assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    let mut at = 2;
    let (mut nf, mut icc) = (0, Vec::new());
    while at + 4 <= bytes.len() {
        assert_eq!(bytes[at], 0xFF, "marker at {at}");
        let marker = bytes[at + 1];
        if marker == 0xDA {
            break;
        }
        let len = be16(&bytes[at + 2..]) as usize;
        let data = &bytes[at + 4..at + 2 + len];
        if (0xC0..=0xC2).contains(&marker) {
            nf = data[5];
        } else if marker == 0xE2 && data.starts_with(b"ICC_PROFILE\0") {
            icc.extend_from_slice(&data[14..]);
        }
        at += 2 + len;
    }
    (nf, icc)
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn reopen(path: &Path) -> *mut PrecisionDocument {
    let c = cpath(path);
    assert_ne!(unsafe { dr_path_gray_source(c.as_ptr()) }, 0, "{path:?}");
    let working = CString::new("gray_gamma_22").unwrap();
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
    assert!(!doc.is_null(), "{path:?}: {}", take_err_string(err));
    doc
}

/// The composite as the file should hold it: R = G = B = gray, alpha.
fn composite(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn layer(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn assert_profile(icc: &[u8], expected: &[u8], what: &str) {
    assert_eq!(icc, expected, "{what}: the embedded profile");
    assert_eq!(&icc[16..20], b"GRAY", "{what}");
}

#[test]
fn png_and_tiff_keep_one_channel_and_the_gray_profile_and_reopen_exactly() {
    let icc = gray_icc("Test Gray 1.8", 1.8);
    let dir = TempDir::new().unwrap();
    for depth in [16u32, 8] {
        for transparent in [false, true] {
            let doc = gray_doc(&icc, depth, transparent);
            let expected = composite(doc);
            let name = descriptor(doc)["gray"]["profile"].clone();
            assert_eq!(name, "Test Gray 1.8");
            let what = format!("{depth}-bit, transparent {transparent}");
            // PNG.
            let request = format!(
                r#"{{"format":"png","bit_depth":{depth},"profile":"document","dither":"none"}}"#
            );
            let (bytes, info) = export(doc, &request).unwrap();
            assert_eq!(info["profile_name"], "Test Gray 1.8", "{what}");
            let (bits, colour, embedded) = png_header(&bytes);
            assert_eq!(
                (u32::from(bits), colour),
                (depth, if transparent { 4 } else { 0 }),
                "{what} png"
            );
            assert_profile(&embedded, &icc, &what);
            let path = write(&dir, &format!("{depth}_{transparent}.png"), &bytes);
            let reopened = reopen(&path);
            assert_eq!(descriptor(reopened)["gray"]["profile"], name, "{what} png");
            assert_eq!(layer(reopened), expected, "{what} png");
            free(reopened);
            // TIFF.
            let request = format!(
                r#"{{"format":"tiff","bit_depth":{depth},"profile":"document","dither":"none"}}"#
            );
            let (bytes, _) = export(doc, &request).unwrap();
            let (spp, photometric, bits, extra, embedded) = tiff_header(&bytes);
            assert_eq!(
                (spp, photometric, u32::from(bits)),
                (if transparent { 2 } else { 1 }, 1, depth),
                "{what} tiff"
            );
            assert_eq!(extra, transparent.then_some(2), "{what} tiff");
            assert_profile(&embedded, &icc, &what);
            let path = write(&dir, &format!("{depth}_{transparent}.tif"), &bytes);
            let reopened = reopen(&path);
            assert_eq!(descriptor(reopened)["gray"]["profile"], name, "{what} tiff");
            assert_eq!(layer(reopened), expected, "{what} tiff");
            free(reopened);
            free(doc);
        }
    }
}

#[test]
fn jpeg_keeps_one_channel_within_two_of_255() {
    let icc = gray_icc("Test Gray 1.8", 1.8);
    let dir = TempDir::new().unwrap();
    let doc = gray_doc(&icc, 8, false);
    let expected = composite(doc);
    let (bytes, info) = export(
        doc,
        r#"{"format":"jpeg","bit_depth":8,"jpeg_quality":100,"profile":"document","dither":"none"}"#,
    )
    .unwrap();
    assert_eq!(info["profile_name"], "Test Gray 1.8");
    let (nf, embedded) = jpeg_header(&bytes);
    assert_eq!(nf, 1);
    assert_profile(&embedded, &icc, "jpeg");
    let reopened = reopen(&write(&dir, "g.jpg", &bytes));
    assert_eq!(descriptor(reopened)["gray"]["profile"], "Test Gray 1.8");
    for (got, want) in layer(reopened).iter().zip(&expected) {
        assert!(got[0] == got[1] && got[1] == got[2], "{got:?}");
        let delta = (i32::from(got[0] / 257) - i32::from(want[0] / 257)).abs();
        assert!(delta <= 2, "{got:?} against {want:?}");
    }
    free(reopened);
    free(doc);
}

#[test]
fn srgb_writes_rgb_and_the_unkept_requests_refuse_by_name() {
    let icc = gray_icc("Test Gray 1.8", 1.8);
    let doc = gray_doc(&icc, 16, false);
    let (bytes, _) = export(doc, r#"{"format":"png","bit_depth":8,"profile":"srgb"}"#).unwrap();
    let (_, colour, _) = png_header(&bytes);
    assert!(matches!(colour, 2 | 6), "colour type {colour}");
    assert_eq!(
        export(
            doc,
            r#"{"format":"webp","bit_depth":8,"profile":"document"}"#
        )
        .unwrap_err(),
        "A Grayscale document exports one-channel PNG, TIFF or JPEG with profile document; webp \
         has no one-channel export in Doctored. Choose profile srgb, display-p3 or file to \
         export RGB."
    );
    free(doc);
    let doc = gray_doc(&icc, 32, false);
    // #1 phase 3c: a 32-bit kept-gray TIFF is written (`gray32_files.rs`).
    assert!(export(
        doc,
        r#"{"format":"tiff","bit_depth":32,"profile":"document"}"#
    )
    .is_ok());
    // A 32-bit Gray document's 16-bit export keeps one channel.
    let (bytes, _) = export(
        doc,
        r#"{"format":"png","bit_depth":16,"profile":"document"}"#,
    )
    .unwrap();
    let (bits, colour, embedded) = png_header(&bytes);
    assert_eq!((bits, colour), (16, 0));
    assert_profile(&embedded, &icc, "32-bit");
    free(doc);
}
