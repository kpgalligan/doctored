//! The conditional `.rz` words 63 (byte) and 64 (typed) that a
//! MAP-FORM Curves adjustment writes (`core/src/curves_native.rs`),
//! following `levels_native`'s 61/62 tests: where the word sits, the round
//! trip, the byte-identical point-only file, the order against the levels
//! word, both refusals in both directions, and a typed map curve found
//! inside a group.

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn points() -> Value {
    json!({"rgb": [[0, 0], [128, 160], [255, 255]], "r": [[0, 20], [255, 235]]})
}

fn maps() -> Value {
    let rgb: Vec<u8> = (0..256)
        .map(|i| (255.0 * (f64::from(i) / 255.0).sqrt()).round() as u8)
        .collect();
    let green: Vec<u8> = (0..256)
        .map(|i| {
            if (100..=115).contains(&i) {
                200
            } else {
                i as u8
            }
        })
        .collect();
    json!({"rgb_map": rgb, "g_map": green})
}

fn per_channel_levels() -> Value {
    json!({"black": 0.1, "g": {"black": 0.15, "white": 1.0, "gamma": 0.7}})
}

fn meta(op: &str, params: &Value) -> String {
    json!({"type": "adjust", "op": op, "params": params}).to_string()
}

fn typed_meta(op: &str, params: &Value) -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": op, "params": params}).to_string()
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

/// A 6 x 4 base under one adjustment layer per `(op, params)`.
fn byte_doc(layers: &[(&str, Value)]) -> DrDocument {
    let base = RgbaImage::from_fn(6, 4, |x, y| Rgba([(x * 40) as u8, (y * 60) as u8, 90, 255]));
    let mut doc = DrDocument::from_pixels(base);
    for (i, (op, params)) in layers.iter().enumerate() {
        doc = doc
            .adding_image_layer(i, RgbaImage::new(6, 4), op)
            .expect("add layer");
        doc.layers[i + 1].meta = Some(meta(op, params));
    }
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
fn byte_rz_writes_word_63_only_for_map_curves_and_round_trips() {
    let dir = TempDir::new().unwrap();
    let plain_doc = byte_doc(&[("curves", points())]);
    let plain = save_byte(&dir, "plain.rz", &plain_doc);
    assert_eq!(word(&plain, 0), 45);
    assert_ne!(word(&plain, 1), 63, "a point-only document writes no word");

    let mut carried = plain_doc.clone();
    carried.layers[1].meta = Some(meta("curves", &maps()));
    let bytes = save_byte(&dir, "carried.rz", &carried);
    assert_eq!((word(&bytes, 0), word(&bytes, 1)), (45, 63));
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

    // Editing the layer back to points restores the point-only bytes, so a
    // map-free file is exactly what the writer without the word produced.
    let mut back = carried.clone();
    back.layers[1].meta = Some(meta("curves", &points()));
    assert_eq!(save_byte(&dir, "back.rz", &back), plain);
}

#[test]
fn the_curves_word_is_peeled_before_the_levels_word() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&[("levels", per_channel_levels()), ("curves", maps())]);
    let bytes = save_byte(&dir, "both.rz", &doc);
    assert_eq!(
        (word(&bytes, 0), word(&bytes, 1), word(&bytes, 2)),
        (45, 63, 61)
    );
    let reopened = open_byte(&dir, "both-again.rz", &bytes).expect("reopen");
    assert_eq!(reopened.layers[1].meta, doc.layers[1].meta);
    assert_eq!(reopened.layers[2].meta, doc.layers[2].meta);
    assert_eq!(save_byte(&dir, "both-resave.rz", &reopened), bytes);
}

#[test]
fn a_byte_curves_claim_that_does_not_match_the_records_refuses() {
    let dir = TempDir::new().unwrap();
    let carried = save_byte(&dir, "carried.rz", &byte_doc(&[("curves", maps())]));
    let missing = without_word(&carried, 1);
    let message = open_byte(&dir, "missing.rz", &missing)
        .err()
        .expect("refused");
    assert!(
        message.ends_with("native curve maps require DRDC 63"),
        "{message}"
    );

    let plain = save_byte(&dir, "plain.rz", &byte_doc(&[("curves", points())]));
    let stray = with_word(&plain, 1, 63);
    let message = open_byte(&dir, "stray.rz", &stray).err().expect("refused");
    assert!(message.ends_with("invalid native curves word"), "{message}");
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

fn typed_adjustment(op: &str, params: &Value) -> PrecisionDocumentLayer {
    PrecisionDocumentLayer::adjustment(op, &typed_meta(op, params), 1.0, true, &IccProfile::srgb())
        .unwrap()
}

fn descriptions(doc: &PrecisionDocument) -> Vec<Option<String>> {
    (0..doc.layer_count())
        .map(|i| doc.adjustment_description(i).map(str::to_string))
        .collect()
}

#[test]
fn typed_rz_writes_word_64_only_for_map_curves() {
    let plain_doc = typed_doc(vec![base16(), typed_adjustment("curves", &points())]);
    let plain = plain_doc.encode_native(LIMIT, 0, None).expect("plain");
    assert_eq!(word(&plain, 0), 46);
    assert_ne!(word(&plain, 1), 64, "a point-only document writes no word");

    let carried = typed_doc(vec![base16(), typed_adjustment("curves", &maps())]);
    let bytes = carried.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!((word(&bytes, 0), word(&bytes, 1)), (46, 64));
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(descriptions(&reopened), descriptions(&carried));
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes
    );

    // A plan never undercounts what the writer emits.
    let plan = carried.native_plan().expect("plan");
    assert!(plan.encoded_bound >= bytes.len() as u64);

    // The word's two refusals, naming 64.
    let message = PrecisionDocument::parse_native(&without_word(&bytes, 1), LIMIT, 0, None)
        .err()
        .expect("refused");
    assert_eq!(message, "native curve maps require DRDC 64");
    let message = PrecisionDocument::parse_native(&with_word(&plain, 1, 64), LIMIT, 0, None)
        .err()
        .expect("refused");
    assert_eq!(message, "invalid native curves word");
}

#[test]
fn the_typed_curves_word_is_peeled_before_the_levels_word() {
    let doc = typed_doc(vec![
        base16(),
        typed_adjustment("levels", &per_channel_levels()),
        typed_adjustment("curves", &maps()),
    ]);
    let bytes = doc.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(
        (word(&bytes, 0), word(&bytes, 1), word(&bytes, 2)),
        (46, 64, 62)
    );
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(descriptions(&reopened), descriptions(&doc));
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes
    );
}

#[test]
fn a_typed_map_curve_inside_a_group_is_found() {
    let group = PrecisionDocumentLayer::isolated_normal(
        "Group",
        vec![base16(), typed_adjustment("curves", &maps())],
        1.0,
        true,
    )
    .unwrap();
    let doc = typed_doc(vec![group]);
    let bytes = doc.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!((word(&bytes, 0), word(&bytes, 1)), (46, 64));
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(descriptions(&reopened), descriptions(&doc));
    assert!(
        descriptions(&reopened)
            .iter()
            .flatten()
            .any(|d| d.contains("g_map")),
        "the nested description survives"
    );
    let message = PrecisionDocument::parse_native(&without_word(&bytes, 1), LIMIT, 0, None)
        .err()
        .expect("refused");
    assert_eq!(message, "native curve maps require DRDC 64");
}
