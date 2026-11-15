//! A pattern tile's `rgb16` samples in PSD (`psd_patterns`' READ and WRITE,
//! `designs/pattern-tiles.md`): a 16-bit document whose Pattern Overlay
//! tile holds codes off the byte grid writes them in 16-bit arrays under
//! `Pat2`, and the import keeps them as the tile's samples. The format
//! stores 0…32768, so the oracle is the module doc's two roundings written
//! out here: code c comes back round(round(c · 32768 / 65535) · 65535 /
//! 32768), within one of c, and the codes a first import gives are fixed
//! points of a second export and import. A record made from 8-bit codes
//! keeps reading with no samples (Photoshop's own P16 is
//! `psd_pattern_fill_layer`'s fixture test).

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::take_err_string;
use crate::psd_pattern_fill_layer::{contains, open16, save16};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// Two by two, opaque, every colour code off the byte grid but 65535.
const CODES: [u16; 16] = [
    12345, 40000, 777, 65535, 1, 2, 3, 65535, 65534, 32768, 257, 65535, 100, 200, 300, 65535,
];

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
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

fn unbase64(text: &str) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digits: Vec<u32> = text
        .bytes()
        .filter(|&c| c != b'=')
        .map(|c| T.iter().position(|&t| t == c).expect("a base64 digit") as u32)
        .collect();
    let mut out = Vec::new();
    for chunk in digits.chunks(4) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, d)| n | (d << (18 - 6 * i)));
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

/// A 16-bit document whose one layer carries a Pattern Overlay of `codes`.
fn styled(codes: &[u16]) -> PrecisionDocument {
    let white =
        PrecisionImage::from_rgba16(6, 4, vec![[65535; 4]; 24], IccProfile::srgb(), None).unwrap();
    let doc = PrecisionDocument::from_parts(
        6,
        4,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Layer", white, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let raster: Vec<u8> = codes
        .iter()
        .map(|&c| ((u32::from(c) + 128) / 257) as u8)
        .collect();
    let data: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
    let style = json!({"effects": [{"type": "pattern_overlay", "pattern": {
        "name": "Sixteen", "width": 2, "height": 2, "rgba": base64(&raster),
        "native": {"kind": "rgb16", "data": base64(&data)}}}]});
    let request = CString::new(
        json!({"op": "layer_style", "version": 1, "layer": 0, "style": style}).to_string(),
    )
    .unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

/// The codes the one layer's overlay tile holds as samples; `None` without.
fn sample_codes(doc: &PrecisionDocument) -> Option<Vec<u16>> {
    let json: Value =
        serde_json::from_str(&doc.layer(0).unwrap().style().unwrap().to_json()).unwrap();
    let native = &json["effects"][0]["pattern"]["native"];
    if native.is_null() {
        return None;
    }
    assert_eq!(native["kind"], "rgb16");
    let bytes = unbase64(native["data"].as_str().unwrap());
    Some(
        bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect(),
    )
}

/// The module doc's two roundings: c stored round(c · 32768 / 65535), read
/// round(v · 65535 / 32768).
fn through_the_format(c: u16) -> u16 {
    let stored = (f64::from(c) * 32768.0 / 65535.0).round();
    (stored * 65535.0 / 32768.0).round() as u16
}

#[test]
fn rgb16_samples_cross_a_16_bit_psd_within_one_code_and_then_hold() {
    let dir = TempDir::new().unwrap();
    let first_path = dir.path().join("first.psd");
    save16(&styled(&CODES), &first_path);
    let bytes = std::fs::read(&first_path).unwrap();
    assert!(contains(&bytes, b"8BIMPat2"), "the 16-bit pattern table");
    let first = sample_codes(&open16(&first_path)).expect("the import keeps the 16-bit codes");
    for (i, (&got, &code)) in first.iter().zip(&CODES).enumerate() {
        assert_eq!(got, through_the_format(code), "sample {i}");
        assert!(got.abs_diff(code) <= 1, "sample {i}: {got} from {code}");
    }
    // A second trip changes nothing: every first-import code is a stored
    // level's own.
    let second_path = dir.path().join("second.psd");
    save16(&open16(&first_path), &second_path);
    assert_eq!(sample_codes(&open16(&second_path)), Some(first));
}

#[test]
fn a_16_bit_tile_of_8_bit_codes_imports_without_samples() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("byte.psd");
    let on_grid: Vec<u16> = CODES.iter().map(|c| c / 257 * 257).collect();
    // A tile whose samples are the raster's own codes × 257 writes and reads
    // as a tile with none.
    save16(&styled(&on_grid), &path);
    assert_eq!(sample_codes(&open16(&path)), None);
}
