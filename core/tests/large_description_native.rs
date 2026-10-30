//! `.rz` word 102 (`large_description_native`): an artwork description
//! over 64 KiB — a pattern fill layer carrying a 300 × 300 custom tile — is
//! published, rendered and saved, and the writer claims the word: the FIRST
//! word inside 45 in a byte file (ahead of 101), right after 46 in a typed
//! file, and after the CMYK rung in a CMYK file. Each round-trips its
//! description and pixels. A file without one keeps its prior bytes (no
//! word). The refusals are checked on crafted files: the word inserted into
//! a file without a large description is refused by number at both depths;
//! the word deleted from a typed file leaves its large entry to the size
//! refusal, while a byte file's meta has always been opaque up to the meta
//! cap and reads as before.
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_artwork_render, dr_doc_artwork, dr_precision_doc_artwork,
    dr_precision_doc_artwork_description, dr_precision_doc_artwork_metadata,
};
use doctored_core::ffi_doc::dr_doc_layer_meta;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_structure_fixture::free;
use crate::cmyk_style_fixture::backdrop;
use crate::common::{layer_pixels, solid, styled};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 8;
const H: u32 = 6;
const LARGE_WORD: &str = "invalid native large artwork description word";
const SIZE_REFUSAL: &str = "invalid typed artwork description reference or size";

/// Standard base64 (RFC 4648, padded), written here so the test does not
/// borrow the core's encoder.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The pattern fill keys of a `side × side` opaque custom tile.
fn pattern_fill(side: u32) -> Value {
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            rgba.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]);
        }
    }
    json!({"pattern": {"name": "Big", "width": side, "height": side, "rgba": base64(&rgba)},
        "angle": 0, "scale": 1, "link_with_layer": false, "phase": [0, 0]})
}

/// The fill layer description and the rendered image of a `side` tile.
fn fill_layer(side: u32) -> (String, PrecisionImage) {
    let fill = pattern_fill(side);
    let description = json!({"type": "fill", "version": 1, "fill": "pattern",
        "pattern_fill": fill, "canvas": [W, H]})
    .to_string();
    let components = CString::new(json!([{"kind": "pattern", "fill": fill}]).to_string()).unwrap();
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            W,
            H,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    (description, *unsafe { Box::from_raw(out) })
}

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_string()
}

// ------------------------------------------------------------------ byte --

fn byte_fill(side: u32) -> (DrDocument, String) {
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]));
    let (description, image) = fill_layer(side);
    let name = CString::new("Pattern Fill").unwrap();
    let text = CString::new(description.as_str()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            &doc,
            0,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    (*unsafe { Box::from_raw(out) }, description)
}

fn byte_save(doc: &DrDocument, path: &Path) -> Vec<u8> {
    doc.save_native(path.to_str().unwrap()).unwrap();
    std::fs::read(path).unwrap()
}

#[test]
fn the_byte_word_102_comes_first_round_trips_and_refuses_a_forgery() {
    let dir = TempDir::new().unwrap();
    let (small, _) = byte_fill(4);
    let plain = byte_save(&small, &dir.path().join("small.rz"));
    assert_eq!(word(&plain, 4), 45);
    assert_ne!(word(&plain, 8), 102, "no 102 for a 64 KiB description");

    let (large, description) = byte_fill(300);
    assert!(description.len() > 64 * 1024);
    let bytes = byte_save(&large, &dir.path().join("large.rz"));
    assert_eq!(word(&bytes, 4), 45);
    assert_eq!(word(&bytes, 8), 102, "FIRST inside 45");
    let back = DrDocument::open(&write(&dir, "again.rz", &bytes)).unwrap();
    assert_eq!(
        owned(unsafe { dr_doc_layer_meta(&back, 1) }).as_deref(),
        Some(description.as_str())
    );
    assert_eq!(layer_pixels(&back, 1), layer_pixels(&large, 1));

    // Deleted: a byte meta has always been opaque up to the meta cap.
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let back = DrDocument::open(&write(&dir, "cut.rz", &stripped)).unwrap();
    assert_eq!(
        owned(unsafe { dr_doc_layer_meta(&back, 1) }).as_deref(),
        Some(description.as_str())
    );

    // Inserted into a file without a large description: refused by number.
    let mut forged = plain.clone();
    forged.splice(8..8, 102u32.to_le_bytes());
    let Err(message) = DrDocument::open(&write(&dir, "forged.rz", &forged)) else {
        panic!("the word without a large description must refuse");
    };
    assert!(message.ends_with(LARGE_WORD), "{message}");

    // With a multichannel Blend If too, 102 comes before 101.
    let ranges = json!({"blend_if": {"channel": "gray", "this_layer": [0, 0, 96, 192],
        "underlying": [0, 0, 255, 255], "extra": [{"channel": "red",
        "this_layer": [0, 0, 64, 224], "underlying": [0, 0, 255, 255]}]}});
    let both = styled(&large, 1, &ranges.to_string());
    let bytes = byte_save(&both, &dir.path().join("both.rz"));
    assert_eq!(word(&bytes, 8), 102);
    assert_eq!(word(&bytes, 12), 101, "102 ahead of 101");
    assert!(DrDocument::open(&write(&dir, "both-again.rz", &bytes)).is_ok());
}

// ----------------------------------------------------------------- typed --

fn typed_doc() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            W,
            H,
            vec![[1000, 2000, 3000, 65535]; (W * H) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap()
}

fn typed_fill(side: u32) -> (PrecisionDocument, String, PrecisionImage) {
    let (description, image) = fill_layer(side);
    let name = CString::new("Pattern Fill").unwrap();
    let text = CString::new(description.as_str()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            &typed_doc(),
            0,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    (*unsafe { Box::from_raw(out) }, description, image)
}

fn typed_save(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    std::fs::read(path).unwrap()
}

fn typed_open(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::open_native(&write(dir, name, bytes), LIMIT, 0, None)
}

fn description_of(doc: &PrecisionDocument, layer: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_artwork_description(doc, layer) })
}

#[test]
fn the_typed_word_102_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let (small, _, _) = typed_fill(4);
    let plain = typed_save(&small, &dir.path().join("small.rz"));
    assert_eq!(word(&plain, 4), 46);
    assert_ne!(word(&plain, 8), 102, "no 102 for a 64 KiB description");

    let (large, description, image) = typed_fill(300);
    let bytes = typed_save(&large, &dir.path().join("large.rz"));
    assert_eq!(word(&bytes, 4), 46);
    assert_eq!(word(&bytes, 8), 102, "right after 46 on RGB");
    let back = typed_open(&dir, "again.rz", &bytes).unwrap();
    assert_eq!(description_of(&back, 1), Some(description));
    assert_eq!(
        back.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
        image.to_rgba16().ok().unwrap()
    );

    // Deleted: the large entry meets the 64 KiB cap's sentence.
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let message = typed_open(&dir, "cut.rz", &stripped)
        .err()
        .expect("a refusal");
    assert!(message.ends_with(SIZE_REFUSAL), "{message}");

    // Inserted into a file without a large description: refused by number.
    let mut forged = plain.clone();
    forged.splice(8..8, 102u32.to_le_bytes());
    let message = typed_open(&dir, "forged.rz", &forged)
        .err()
        .expect("a refusal");
    assert!(message.ends_with(LARGE_WORD), "{message}");
}

#[test]
fn a_cmyk_file_claims_word_102_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let base = backdrop(W, H, 16, &[[0.0; 4]]);
    let (description, _) = fill_layer(300);
    let text = CString::new(description.as_str()).unwrap();
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_artwork_metadata(base, 0, text.as_ptr(), LIMIT, 0, &mut err) };
    assert_eq!(owned(err), None);
    assert!(!doc.is_null());
    let bytes = typed_save(unsafe { &*doc }, &dir.path().join("cmyk.rz"));
    assert_eq!(word(&bytes, 4), 46);
    assert!((82..=88).contains(&word(&bytes, 8)), "the CMYK rung");
    assert_eq!(word(&bytes, 12), 102, "102 after the rung");
    let back = typed_open(&dir, "cmyk-again.rz", &bytes).unwrap();
    assert!(back.is_cmyk());
    assert_eq!(description_of(&back, 0), Some(description));

    let mut stripped = bytes.clone();
    stripped.drain(12..16);
    let message = typed_open(&dir, "cmyk-cut.rz", &stripped)
        .err()
        .expect("a refusal");
    assert!(message.ends_with(SIZE_REFUSAL), "{message}");
    free(doc);
    free(base);
}
