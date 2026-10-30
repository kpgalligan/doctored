//! `.rz` word 111 (`gradient_map_method_native`): a Gradient Map
//! adjustment whose params hold `method` (`"classic"`). No table. A byte
//! file writes it as the FIRST word inside 45, a typed file right after 46,
//! a CMYK file right after its rung; a legacy map (no `method`) writes no
//! word, so its bytes are the prior writer's. Classic round-trips its
//! description and pixels at both depths and in CMYK. The claim is checked
//! both ways on crafted files: the word deleted from a file that needs it is
//! refused by its number, the word inserted into a file that does not is
//! refused as invalid.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const WORD: u32 = 111;
const REFUSAL: &str = "native gradient map method requires DRDC 111";
const INVALID: &str = "invalid native gradient map method word";

fn rgb_params(classic: bool) -> Value {
    let mut p = json!({"dither": false, "gradient": {"smoothness": 0.5, "stops": [
        {"position": 0, "color": "#28c8dc", "midpoint": 0.3},
        {"position": 0.4, "color": "#78501e", "midpoint": 0.7},
        {"position": 1, "color": "#fa143c"}]}});
    if classic {
        p["method"] = json!("classic");
    }
    p
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_string()
}

// ------------------------------------------------------------------ byte --

fn byte_doc(classic: bool) -> DrDocument {
    let base = RgbaImage::from_fn(8, 4, |x, y| Rgba([(x * 30) as u8, (y * 60) as u8, 90, 255]));
    let mut doc = DrDocument::from_pixels(base)
        .adding_image_layer(0, RgbaImage::new(8, 4), "Gradient Map")
        .expect("add layer");
    doc.layers[1].meta = Some(
        json!({"type": "adjust", "op": "gradient_map", "params": rgb_params(classic)}).to_string(),
    );
    doc
}

fn byte_save(dir: &TempDir, name: &str, doc: &DrDocument) -> Vec<u8> {
    let path = dir.path().join(name);
    doc.save_native(path.to_str().unwrap()).unwrap();
    std::fs::read(path).unwrap()
}

#[test]
fn the_byte_word_111_comes_first_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let legacy = byte_save(&dir, "legacy.rz", &byte_doc(false));
    assert_eq!(word(&legacy, 4), 45);
    assert_ne!(word(&legacy, 8), WORD, "a legacy map writes no word");
    let doc = byte_doc(true);
    let bytes = byte_save(&dir, "classic.rz", &doc);
    assert_eq!(word(&bytes, 4), 45);
    assert_eq!(word(&bytes, 8), WORD, "the first word inside 45");
    assert_eq!(
        word(&bytes, 12),
        word(&legacy, 8),
        "then the legacy file's word"
    );
    let back = DrDocument::open(&write(&dir, "again.rz", &bytes)).unwrap();
    assert_eq!(back.layers[1].meta, doc.layers[1].meta);
    assert_eq!(back.flattened().unwrap(), doc.flattened().unwrap());
    assert_ne!(
        doc.flattened().unwrap(),
        byte_doc(false).flattened().unwrap(),
        "Classic renders apart from legacy"
    );
    let mut cut = bytes.clone();
    cut.drain(8..12);
    let message = DrDocument::open(&write(&dir, "cut.rz", &cut))
        .err()
        .unwrap();
    assert!(message.ends_with(REFUSAL), "{message}");
    let mut forged = legacy.clone();
    forged.splice(8..8, WORD.to_le_bytes());
    let message = DrDocument::open(&write(&dir, "forged.rz", &forged))
        .err()
        .unwrap();
    assert!(message.ends_with(INVALID), "{message}");
}

// ----------------------------------------------------------------- typed --

fn typed_doc(classic: bool) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        8,
        4,
        (0..32u32)
            .map(|i| [(i * 2000) as u16, 30000, (65535 - i * 1500) as u16, 65535])
            .collect(),
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let description = json!({"type": "adjust", "evaluation": "precision_v1",
        "op": "gradient_map", "params": rgb_params(classic)})
    .to_string();
    PrecisionDocument::from_parts(
        8,
        4,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true).unwrap(),
            PrecisionDocumentLayer::adjustment(
                "Gradient Map",
                &description,
                1.0,
                true,
                &IccProfile::srgb(),
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn typed_save(dir: &TempDir, name: &str, doc: &PrecisionDocument) -> Vec<u8> {
    let path = dir.path().join(name);
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    std::fs::read(path).unwrap()
}

fn typed_open(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::open_native(&write(dir, name, bytes), LIMIT, 0, None)
}

fn rendered(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

#[test]
fn the_typed_word_111_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let legacy = typed_save(&dir, "legacy.rz", &typed_doc(false));
    assert_eq!(word(&legacy, 4), 46);
    assert_ne!(word(&legacy, 8), WORD, "a legacy map writes no word");
    let doc = typed_doc(true);
    let bytes = typed_save(&dir, "classic.rz", &doc);
    assert_eq!(word(&bytes, 4), 46);
    assert_eq!(word(&bytes, 8), WORD, "right after 46");
    assert_eq!(word(&bytes, 12), word(&legacy, 8));
    let back = typed_open(&dir, "again.rz", &bytes).unwrap();
    assert_eq!(
        back.adjustment_description(1),
        doc.adjustment_description(1)
    );
    assert_eq!(rendered(&back), rendered(&doc));
    assert_ne!(rendered(&doc), rendered(&typed_doc(false)));
    let mut cut = bytes.clone();
    cut.drain(8..12);
    let message = typed_open(&dir, "cut.rz", &cut).err().unwrap();
    assert!(message.ends_with(REFUSAL), "{message}");
    let mut forged = legacy.clone();
    forged.splice(8..8, WORD.to_le_bytes());
    let message = typed_open(&dir, "forged.rz", &forged).err().unwrap();
    assert!(message.ends_with(INVALID), "{message}");
}

/// A Perceptual map claims the same word: the key-presence
/// predicate covers every `method` value. It round-trips its description and
/// pixels in a byte and a typed file, and renders apart from Classic.
#[test]
fn a_perceptual_map_writes_word_111_and_round_trips_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let mut params = rgb_params(true);
    params["method"] = json!("perceptual");
    let meta = json!({"type": "adjust", "op": "gradient_map", "params": params}).to_string();
    let mut doc = byte_doc(true);
    doc.layers[1].meta = Some(meta);
    let bytes = byte_save(&dir, "perceptual.rz", &doc);
    assert_eq!(word(&bytes, 4), 45);
    assert_eq!(word(&bytes, 8), WORD, "the first word inside 45");
    let back = DrDocument::open(&write(&dir, "perceptual-again.rz", &bytes)).unwrap();
    assert_eq!(back.layers[1].meta, doc.layers[1].meta);
    assert_eq!(back.flattened().unwrap(), doc.flattened().unwrap());
    assert_ne!(
        doc.flattened().unwrap(),
        byte_doc(true).flattened().unwrap(),
        "Perceptual renders apart from Classic"
    );

    let classic = typed_doc(true);
    let description = json!({"type": "adjust", "evaluation": "precision_v1",
        "op": "gradient_map", "params": params})
    .to_string();
    let typed = PrecisionDocument::from_parts(
        8,
        4,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            classic.layer(0).unwrap().clone(),
            PrecisionDocumentLayer::adjustment(
                "Gradient Map",
                &description,
                1.0,
                true,
                &IccProfile::srgb(),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let bytes = typed_save(&dir, "perceptual16.rz", &typed);
    assert_eq!(word(&bytes, 4), 46);
    assert_eq!(word(&bytes, 8), WORD, "right after 46");
    let back = typed_open(&dir, "perceptual16-again.rz", &bytes).unwrap();
    assert_eq!(
        back.adjustment_description(1),
        typed.adjustment_description(1)
    );
    assert_eq!(rendered(&back), rendered(&typed));
    assert_ne!(rendered(&typed), rendered(&classic));
}

// ------------------------------------------------------------------ CMYK --

fn cmyk_doc() -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        8,
        2,
        (0..16u32)
            .map(|i| [(i * 4000) as u16, (65535 - i * 3000) as u16, 20000, 65535])
            .collect(),
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    unsafe { dr_precision_doc_free(rgb) };
    out
}

fn cmyk_map(base: *const PrecisionDocument, classic: bool) -> PrecisionDocument {
    let mut params = json!({"dither": false, "gradient": {"smoothness": 0.25, "stops": [
        {"position": 0, "ink": [0, 0, 0, 1], "midpoint": 0.3},
        {"position": 0.5, "ink": [0.8, 0.1, 0, 0.2], "opacity": 0.5, "opacity_midpoint": 0.7},
        {"position": 1, "ink": [0, 0, 0, 0]}]}});
    if classic {
        params["method"] = json!("classic");
    }
    let description = json!({"type": "adjust", "evaluation": "precision_v1",
        "op": "gradient_map", "params": params})
    .to_string();
    let text = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            base,
            c"Gradient Map".as_ptr(),
            text.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

#[test]
fn a_cmyk_file_claims_word_111_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let base = cmyk_doc();
    let legacy = typed_save(&dir, "legacy.rz", &cmyk_map(base, false));
    assert!((82..=88).contains(&word(&legacy, 8)), "the CMYK rung");
    assert_ne!(word(&legacy, 12), WORD, "a legacy map writes no word");
    let doc = cmyk_map(base, true);
    let bytes = typed_save(&dir, "cmyk.rz", &doc);
    assert!((82..=88).contains(&word(&bytes, 8)), "the CMYK rung");
    assert_eq!(word(&bytes, 12), WORD, "111 after the rung");
    let back = typed_open(&dir, "cmyk-again.rz", &bytes).unwrap();
    assert!(back.is_cmyk());
    assert_eq!(rendered(&back), rendered(&doc));
    let mut cut = bytes.clone();
    cut.drain(12..16);
    let message = typed_open(&dir, "cmyk-cut.rz", &cut).err().unwrap();
    assert!(message.ends_with(REFUSAL), "{message}");
    let mut forged = legacy.clone();
    forged.splice(12..12, WORD.to_le_bytes());
    let message = typed_open(&dir, "cmyk-forged.rz", &forged).err().unwrap();
    assert!(message.ends_with(INVALID), "{message}");
    unsafe { dr_precision_doc_free(base) };
}
