//! The conditional `.rz` words 61 (byte) and 62 (typed) that a
//! Levels adjustment carrying an output range or a per-channel stage writes
//! (`core/src/levels_native.rs`), following `color_samplers`' 59/60 tests:
//! where the word sits, the round trip, the byte-identical master-only file,
//! the combination with the samplers' word, both refusals in both
//! directions, and a typed adjustment found inside a group.

use doctored_core::doc::DrDocument;
use doctored_core::ffi_sampler::dr_doc_with_color_samplers;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn master_only() -> Value {
    json!({"black": 0.1, "white": 0.9, "gamma": 1.4})
}

fn per_channel() -> Value {
    json!({"black": 0.1, "white": 0.9, "gamma": 1.4, "output_black": 0.05,
           "g": {"black": 0.15, "white": 1.0, "gamma": 0.7}})
}

fn byte_meta(params: &Value) -> String {
    json!({"type": "adjust", "op": "levels", "params": params}).to_string()
}

fn typed_meta(params: &Value) -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": "levels", "params": params})
        .to_string()
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + 4 * index;
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// `bytes` with the word at `index` removed.
fn without_word(bytes: &[u8], index: usize) -> Vec<u8> {
    let at = 4 + 4 * index;
    [&bytes[..at], &bytes[at + 4..]].concat()
}

/// `bytes` with `value` inserted as the word at `index`.
fn with_word(bytes: &[u8], index: usize, value: u32) -> Vec<u8> {
    let at = 4 + 4 * index;
    [&bytes[..at], &value.to_le_bytes()[..], &bytes[at..]].concat()
}

// --------------------------------------------------------------- byte --

/// A 6 x 4 base under one levels layer carrying `params`.
fn byte_doc(params: &Value) -> DrDocument {
    let base = RgbaImage::from_fn(6, 4, |x, y| Rgba([(x * 40) as u8, (y * 60) as u8, 90, 255]));
    let mut doc = DrDocument::from_pixels(base)
        .adding_image_layer(0, RgbaImage::new(6, 4), "Levels")
        .expect("add layer");
    doc.layers[1].meta = Some(byte_meta(params));
    doc
}

fn save_byte(dir: &TempDir, name: &str, doc: &DrDocument) -> Vec<u8> {
    let path = dir.path().join(name);
    doc.save_native(path.to_str().unwrap()).expect("save");
    std::fs::read(path).expect("read back")
}

fn open_byte(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<DrDocument, String> {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write");
    DrDocument::open(path.to_str().unwrap())
}

#[test]
fn byte_rz_writes_word_61_only_for_per_channel_or_output_levels() {
    let dir = TempDir::new().unwrap();
    let plain_doc = byte_doc(&master_only());
    let plain = save_byte(&dir, "plain.rz", &plain_doc);
    assert_eq!(word(&plain, 0), 45);
    assert_ne!(word(&plain, 1), 61, "a master-only document writes no word");

    let mut carried = plain_doc.clone();
    carried.layers[1].meta = Some(byte_meta(&per_channel()));
    let bytes = save_byte(&dir, "carried.rz", &carried);
    assert_eq!((word(&bytes, 0), word(&bytes, 1)), (45, 61));
    let reopened = open_byte(&dir, "again.rz", &bytes).expect("reopen");
    assert_eq!(reopened.layers[1].meta, carried.layers[1].meta);
    assert_eq!(
        save_byte(&dir, "resave.rz", &reopened),
        bytes,
        "reopen → resave is byte-identical"
    );
    assert_eq!(
        reopened.flattened().unwrap().as_raw(),
        carried.flattened().unwrap().as_raw()
    );

    // Editing the layer back to master-only restores the prior bytes.
    let mut back = carried.clone();
    back.layers[1].meta = Some(byte_meta(&master_only()));
    assert_eq!(save_byte(&dir, "back.rz", &back), plain);
}

#[test]
fn every_carried_key_alone_writes_the_byte_word() {
    let dir = TempDir::new().unwrap();
    for params in [
        json!({"output_black": 0.2}),
        json!({"output_white": 0.8}),
        json!({"r": {"black": 0.1}}),
        json!({"g": {"gamma": 2.0}}),
        json!({"b": {}}),
    ] {
        let bytes = save_byte(&dir, "one.rz", &byte_doc(&params));
        assert_eq!(word(&bytes, 1), 61, "{params}");
        open_byte(&dir, "one-again.rz", &bytes).expect("reopen");
    }
}

#[test]
fn the_levels_word_is_peeled_before_the_samplers_word() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&per_channel());
    let raw = [1u32, 2, 3];
    let set = unsafe { dr_doc_with_color_samplers(&doc, raw.as_ptr(), 1) };
    assert!(!set.is_null(), "set samplers");
    let set = *unsafe { Box::from_raw(set) };
    let bytes = save_byte(&dir, "both.rz", &set);
    assert_eq!(
        (word(&bytes, 0), word(&bytes, 1), word(&bytes, 2)),
        (45, 61, 59)
    );
    let reopened = open_byte(&dir, "both-again.rz", &bytes).expect("reopen");
    assert_eq!(reopened.layers[1].meta, doc.layers[1].meta);
    assert_eq!(save_byte(&dir, "both-resave.rz", &reopened), bytes);
}

#[test]
fn a_byte_levels_claim_that_does_not_match_the_records_refuses() {
    let dir = TempDir::new().unwrap();
    let carried = save_byte(&dir, "carried.rz", &byte_doc(&per_channel()));
    let missing = without_word(&carried, 1);
    let message = open_byte(&dir, "missing.rz", &missing)
        .err()
        .expect("refused");
    assert!(
        message.ends_with("native per-channel levels require DRDC 61"),
        "{message}"
    );

    let plain = save_byte(&dir, "plain.rz", &byte_doc(&master_only()));
    let stray = with_word(&plain, 1, 61);
    let message = open_byte(&dir, "stray.rz", &stray).err().expect("refused");
    assert!(message.ends_with("invalid native levels word"), "{message}");
}

// -------------------------------------------------------------- typed --

fn base16() -> PrecisionDocumentLayer {
    let pixels: Vec<[u16; 4]> = (0..24u16)
        .map(|i| [i * 2000, 60000 - i * 1500, 30000, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(6, 4, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap()
}

fn typed_doc(layers: Vec<PrecisionDocumentLayer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        6,
        4,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn typed_levels(params: &Value) -> PrecisionDocumentLayer {
    PrecisionDocumentLayer::adjustment(
        "Levels",
        &typed_meta(params),
        1.0,
        true,
        &IccProfile::srgb(),
    )
    .unwrap()
}

fn descriptions(doc: &PrecisionDocument) -> Vec<Option<String>> {
    (0..doc.layer_count())
        .map(|i| doc.adjustment_description(i).map(str::to_string))
        .collect()
}

#[test]
fn typed_rz_writes_word_62_only_for_per_channel_or_output_levels() {
    let plain_doc = typed_doc(vec![base16(), typed_levels(&master_only())]);
    let plain = plain_doc.encode_native(LIMIT, 0, None).expect("plain");
    assert_eq!(word(&plain, 0), 46);
    assert_ne!(word(&plain, 1), 62, "a master-only document writes no word");

    let carried = typed_doc(vec![base16(), typed_levels(&per_channel())]);
    let bytes = carried.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!((word(&bytes, 0), word(&bytes, 1)), (46, 62));
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(descriptions(&reopened), descriptions(&carried));
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes
    );

    // A plan never undercounts what the writer emits.
    let plan = carried.native_plan().expect("plan");
    assert!(plan.encoded_bound >= bytes.len() as u64);

    // The word's two refusals, naming 62.
    let message = PrecisionDocument::parse_native(&without_word(&bytes, 1), LIMIT, 0, None)
        .err()
        .expect("refused");
    assert_eq!(message, "native per-channel levels require DRDC 62");
    let message = PrecisionDocument::parse_native(&with_word(&plain, 1, 62), LIMIT, 0, None)
        .err()
        .expect("refused");
    assert_eq!(message, "invalid native levels word");
}

#[test]
fn a_typed_levels_adjustment_inside_a_group_is_found() {
    let group = PrecisionDocumentLayer::isolated_normal(
        "Group",
        vec![base16(), typed_levels(&per_channel())],
        1.0,
        true,
    )
    .unwrap();
    let doc = typed_doc(vec![group]);
    let bytes = doc.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!((word(&bytes, 0), word(&bytes, 1)), (46, 62));
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(descriptions(&reopened), descriptions(&doc));
    assert!(
        descriptions(&reopened)
            .iter()
            .flatten()
            .any(|d| d.contains("output_black")),
        "the nested description survives"
    );
    let message = PrecisionDocument::parse_native(&without_word(&bytes, 1), LIMIT, 0, None)
        .err()
        .expect("refused");
    assert_eq!(message, "native per-channel levels require DRDC 62");
}
