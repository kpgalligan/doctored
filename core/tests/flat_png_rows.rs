//! The 8-bit flat PNG (13C-5, `flat_png` over `png_rows8`) keeps the bytes
//! `image`'s `PngEncoder` wrote behind the metadata splice. Black-box through
//! the FFI, against the test's own `PngEncoder` encode of the same request.
//!
//! - With nothing to attach (`dr_image_save`), the file IS `PngEncoder`'s
//!   PNG of the pixels: noise (the stored fallback), a smooth plane and a
//!   flat one.
//! - With a Display P3 profile, EXIF, XMP and a resolution, the saved file
//!   and the exported bytes are `PngEncoder`'s PNG of the pixels with that
//!   profile and that `eXIf` body, with the splice's own chunks (`pHYs`, then
//!   the XMP `iTXt`) straight after IHDR: chunk for chunk, byte for byte.

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_save};
use doctored_core::ffi_color::dr_doc_save_image;
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_export_result_bytes, dr_export_result_free,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};

const FORMAT_PNG: i32 = 0;

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn noise(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        p.0 = (splitmix(&mut state) as u32).to_le_bytes();
    }
    out
}

fn smooth(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 255 / w) as u8,
            (y * 255 / h) as u8,
            ((x + y) / 4) as u8,
            255,
        ])
    })
}

/// `PngEncoder`'s PNG of `pixels`, with `icc` and `exif` when given.
fn oracle(pixels: &RgbaImage, icc: Option<&[u8]>, exif: Option<&[u8]>) -> Vec<u8> {
    let mut png = Vec::new();
    let mut encoder = PngEncoder::new(&mut png);
    if let Some(icc) = icc {
        encoder.set_icc_profile(icc.to_vec()).unwrap();
    }
    if let Some(exif) = exif {
        encoder.set_exif_metadata(exif.to_vec()).unwrap();
    }
    encoder
        .write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            ExtendedColorType::Rgba8,
        )
        .unwrap();
    png
}

/// A PNG's chunks after the signature: (type, whole chunk bytes).
fn chunks(png: &[u8]) -> Vec<([u8; 4], &[u8])> {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut out = Vec::new();
    let mut at = 8;
    while at < png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = png[at + 4..at + 8].try_into().unwrap();
        out.push((kind, &png[at..at + 12 + len]));
        at += 12 + len;
    }
    assert_eq!(at, png.len(), "the chunks end at the file's end");
    out
}

/// A chunk's data (between its type and CRC).
fn data(chunk: &[u8]) -> &[u8] {
    &chunk[8..chunk.len() - 4]
}

#[test]
fn a_plain_png_is_png_encoder_s() {
    let dir = TempDir::new().unwrap();
    for (name, pixels) in [
        ("noise", noise(300, 200, 9)),
        ("smooth", smooth(700, 500)),
        (
            "flat",
            RgbaImage::from_pixel(90, 40, Rgba([12, 200, 4, 255])),
        ),
    ] {
        let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), pixels.width(), pixels.height()) };
        assert!(!image.is_null());
        let path = dir.path().join(format!("{name}.png"));
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe { dr_image_save(image, cpath(&path).as_ptr(), FORMAT_PNG, 90, &mut err) };
        unsafe { dr_image_free(image) };
        assert!(ok, "{name}: {}", take_err_string(err));
        assert!(
            std::fs::read(&path).unwrap() == oracle(&pixels, None, None),
            "{name}: the saved PNG differs from PngEncoder's"
        );
    }
}

#[test]
fn a_png_with_a_profile_metadata_and_a_resolution_keeps_its_chunks() {
    let pixels = smooth(320, 180);
    let mut doc = DrDocument::from_pixels(pixels.clone());
    doc.profile = IccProfile::display_p3();
    // A little-endian TIFF packet whose IFD0 holds one entry, Orientation 1.
    let exif: &[u8] = &[
        0x49, 0x49, 0x2A, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
    ];
    doc.metadata = Metadata {
        exif: Some(Arc::from(exif)),
        xmp: Some(Arc::from(&b"<x:xmpmeta>probe</x:xmpmeta>"[..])),
        iptc: None,
    };
    doc.resolution = Resolution { x: 300.0, y: 150.0 };
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("meta.png");
    let mut err: *mut c_char = ptr::null_mut();
    let mut carried = 0_u32;
    let ok = unsafe {
        dr_doc_save_image(
            &doc,
            ptr::null(),
            cpath(&path).as_ptr(),
            FORMAT_PNG,
            90,
            true,
            false,
            &mut carried,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    let saved = std::fs::read(&path).unwrap();
    let request = CString::new(r#"{"format":"png"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result =
        unsafe { dr_doc_export_encode(&doc, ptr::null(), request.as_ptr(), ptr::null(), &mut err) };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let mut length = 0_usize;
    let bytes = unsafe { dr_export_result_bytes(result, &mut length) };
    let exported = unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec();
    unsafe { dr_export_result_free(result) };
    assert!(exported == saved, "the export and the save differ");

    let found = chunks(&saved);
    let types: Vec<&[u8]> = found.iter().map(|(kind, _)| &kind[..]).collect();
    assert_eq!(
        types,
        [
            &b"IHDR"[..],
            b"pHYs",
            b"iTXt",
            b"iCCP",
            b"eXIf",
            b"IDAT",
            b"IEND"
        ],
        "the chunk order"
    );
    // The splice's chunks, by this file's own formulas: pixels per metre
    // (round(ppi / 0.0254)) and unit 1; the XMP keyword, five zero bytes and
    // the packet.
    let mut phys = Vec::new();
    phys.extend_from_slice(&11_811_u32.to_be_bytes());
    phys.extend_from_slice(&5_906_u32.to_be_bytes());
    phys.push(1);
    assert_eq!(data(found[1].1), &phys[..], "pHYs");
    let itxt = data(found[2].1);
    assert!(
        itxt.starts_with(b"XML:com.adobe.xmp\0\0\0\0\0"),
        "iTXt keyword"
    );
    assert!(
        itxt.windows(5).any(|w| w == b"probe"),
        "iTXt carries the packet"
    );
    // Everything else is `PngEncoder`'s, given the profile and the eXIf
    // body the file carries.
    let profile = IccProfile::display_p3();
    let expected = oracle(&pixels, Some(&profile.bytes()[..]), Some(data(found[4].1)));
    let mut rebuilt = b"\x89PNG\r\n\x1a\n".to_vec();
    let theirs = chunks(&expected);
    assert_eq!(&theirs[0].0, b"IHDR");
    rebuilt.extend_from_slice(theirs[0].1);
    rebuilt.extend_from_slice(found[1].1);
    rebuilt.extend_from_slice(found[2].1);
    for (_, chunk) in &theirs[1..] {
        rebuilt.extend_from_slice(chunk);
    }
    assert!(
        rebuilt == saved,
        "the PNG differs from PngEncoder's with the splice"
    );
}
