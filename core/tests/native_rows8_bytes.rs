//! The byte native writer's PNG records (13C-2a, `png_rows8`): every layer,
//! channel and embedded 8-bit source PNG a saved file holds equals the PNG
//! `image`'s `PngEncoder` writes for the same plane at its defaults (the
//! oracle, called here on the plane the test built), over the shapes the
//! row encoder must get right: noise (the stored fallback), a 1×1 plane (the
//! fallback again), a smooth plane, a clear one (zero-cost rows), three
//! rows, an odd size with varying alpha, a flat plane (a filter cost tie in
//! its first row), a flat and a noisy channel, a masked layer and a placed
//! 8-bit source (a u64 length).

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_save_native;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::SourceEdit;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use serde_json::json;
use std::ffi::{CStr, CString};
use std::ptr;

use crate::common;

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Every byte of every pixel from the generator.
fn noise(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        p.0 = (splitmix(&mut state) as u32).to_le_bytes();
    }
    out
}

/// A photo-like plane: smooth ramps, opaque.
fn smooth(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 255 / w.max(1)) as u8,
            (y * 255 / h.max(1)) as u8,
            ((x + y) / 3) as u8,
            255,
        ])
    })
}

/// Plasma with an alpha that varies across the plane.
fn odd(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let [r, g, b] = common::plasma(x, y);
        Rgba([r, g, b, (x * 7 + y * 3) as u8])
    })
}

/// `PngEncoder`'s PNG of `raw` at its defaults: the oracle.
fn oracle(raw: &[u8], w: u32, h: u32, color: ExtendedColorType) -> Vec<u8> {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(raw, w, h, color)
        .unwrap();
    png
}

fn rgba(image: &RgbaImage) -> Vec<u8> {
    oracle(
        image.as_raw(),
        image.width(),
        image.height(),
        ExtendedColorType::Rgba8,
    )
}

fn gray(raw: &[u8], w: u32, h: u32) -> Vec<u8> {
    oracle(raw, w, h, ExtendedColorType::L8)
}

/// The file `dr_doc_save_native` writes for `doc`.
fn saved(doc: &DrDocument) -> Vec<u8> {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("rows.doctored");
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, name.as_ptr(), ptr::null(), &mut error) };
    if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("{message}");
    }
    assert!(ok);
    std::fs::read(path).unwrap()
}

/// Every PNG record of `file` in order: at each signature, the length the
/// record's prefix gives (a u32, or a u64 when the u32 before the signature
/// is its zero high half), and the scan resumes after the record.
fn records(file: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 8;
    while let Some(found) = file[at..].windows(8).position(|w| w == SIGNATURE) {
        let start = at + found;
        let short = u32::from_le_bytes(file[start - 4..start].try_into().unwrap()) as usize;
        let len = if short == 0 {
            u64::from_le_bytes(file[start - 8..start].try_into().unwrap()) as usize
        } else {
            short
        };
        out.push(&file[start..start + len]);
        at = start + len;
    }
    out
}

/// The file's PNG records equal `expected`, in order.
fn check(name: &str, doc: &DrDocument, expected: &[Vec<u8>]) {
    let file = saved(doc);
    let found = records(&file);
    assert_eq!(found.len(), expected.len(), "{name}: PNG record count");
    for (index, (record, want)) in found.iter().zip(expected).enumerate() {
        assert!(
            *record == want.as_slice(),
            "{name}: PNG record {index} ({} bytes) differs from PngEncoder's ({} bytes)",
            record.len(),
            want.len()
        );
    }
}

#[test]
fn every_layer_png_is_png_encoder_s() {
    for (name, image) in [
        ("noise", noise(300, 200, 1)),
        ("one pixel", noise(1, 1, 2)),
        ("smooth", smooth(700, 500)),
        ("clear", RgbaImage::new(64, 64)),
        ("three rows", odd(64, 3)),
        ("odd", odd(37, 53)),
        (
            "flat",
            RgbaImage::from_pixel(90, 40, Rgba([200, 4, 77, 255])),
        ),
    ] {
        let expected = vec![rgba(&image)];
        check(name, &DrDocument::from_pixels(image), &expected);
    }
}

#[test]
fn channel_pngs_and_a_masked_layer_are_png_encoder_s() {
    let (w, h) = (300, 200);
    let base = noise(w, h, 3);
    let layer = odd(120, 80);
    let flat = vec![200_u8; (w * h) as usize];
    let noisy: Vec<u8> = noise(w, h, 4).into_raw().into_iter().step_by(4).collect();
    let doc = DrDocument::from_pixels(base.clone())
        .adding_image_layer(0, layer.clone(), "Masked")
        .unwrap()
        .add_mask(1, MaskKind::RevealAll)
        .unwrap()
        .add_channel("Flat", &flat, w, h, [255, 0, 0], 0.5)
        .unwrap()
        .add_channel("Noisy", &noisy, w, h, [0, 0, 255], 0.5)
        .unwrap();
    check(
        "channels",
        &doc,
        &[
            rgba(&base),
            rgba(&layer),
            gray(&flat, w, h),
            gray(&noisy, w, h),
        ],
    );
}

#[test]
fn a_placed_8_bit_source_png_is_png_encoder_s() {
    let (w, h) = (130, 90);
    let picture = odd(w, h);
    let background = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([255; 4])));
    let source = DrDocument::from_pixels(picture.clone());
    let edit = SourceEdit::parse(
        &json!({"op":"place","source_id":"p","revision_id":"a","name":"Placed","x":0,"y":0})
            .to_string(),
    )
    .unwrap();
    let doc = background
        .source_edit(Some(&source), &edit, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(doc.layers.len(), 2);
    let white = RgbaImage::from_pixel(w, h, Rgba([255; 4]));
    let placed = doc.layers[1].pixels.to_contiguous().unwrap();
    // The Background, the placed layer, then its 8-bit original in the source
    // table (whose length prefix is a u64).
    check(
        "placed",
        &doc,
        &[rgba(&white), rgba(&placed), rgba(&picture)],
    );
}
