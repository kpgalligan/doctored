//! Photoshop's `levl` record with OUTPUT levels and PER-CHANNEL
//! records (`psd_adjustment.rs`), black-box through the C API at both
//! depths: hand-encoded bodies decode to the canonical `levels` meta, the
//! declines that remain keep their reasons, and a per-channel/output Levels
//! layer encodes to the hand-encoded body and reopens equal.
//!
//! The bodies follow Adobe's table and the layout measured against
//! Photoshop 2025: u16 version 2, then 29 records of `(in_floor, in_ceil,
//! out_floor, out_ceil, gamma×100)` — record 0 the composite, 1, 2, 3 red,
//! green, blue. Kept out of `psd_adjustments.rs`, which is past the size cap.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use crate::psd16_fixture as fixture16;
use crate::psd_mask_fixture as fixture8;
use common::{cpath, take_err_string};

const LAYERS: i32 = 0;
const CANVAS: [u32; 2] = [2, 2];
const BASE: [u8; 16] = [
    10, 20, 30, 255, 200, 100, 50, 255, 90, 160, 240, 255, 128, 128, 128, 128,
];
const BASE16: [[u16; 4]; 4] = [
    [1000, 2000, 3000, 65535],
    [40000, 20000, 10000, 65535],
    [20000, 40000, 60000, 65535],
    [32768, 32768, 32768, 32768],
];
const IDENTITY: [i32; 5] = [0, 255, 0, 255, 100];

// ------------------------------------------------------------- C plumbing --

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

fn open_psd(path: &Path, report: &Value) -> Box<DrDocument> {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), LAYERS, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    unsafe { Box::from_raw(raw) }
}

struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { doctored_core::ffi_precision::dr_precision_doc_free(self.0) };
    }
}
impl std::ops::Deref for Owned {
    type Target = PrecisionDocument;
    fn deref(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

fn open_typed(path: &Path, report: &Value) -> Owned {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "typed open: {}", take_err_string(error));
    Owned(raw)
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array"))
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

/// Every `8BIM{key}` block body in a written file.
fn blocks_in(bytes: &[u8], key: &[u8; 4]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 12 <= bytes.len() {
        if &bytes[at..at + 4] == b"8BIM" && &bytes[at + 4..at + 8] == key {
            let len = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            out.push(bytes[at + 12..at + 12 + len].to_vec());
            at += 12 + len;
        } else {
            at += 1;
        }
    }
    out
}

// ---------------------------------------------------------- the bodies --

fn w16(values: &[i32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|v| (*v as u16).to_be_bytes())
        .collect()
}

/// A `levl` body: `version`, then `records` in order, identity after them
/// up to 29.
fn levl(version: i32, records: &[[i32; 5]]) -> Vec<u8> {
    let mut body = w16(&[version]);
    for index in 0..29 {
        body.extend(w16(records.get(index).unwrap_or(&IDENTITY)));
    }
    body
}

/// The expected stage keys of one record: `code/255`, `g/100`, the output
/// pair only when it is not (0, 255).
fn stage(record: [i32; 5]) -> serde_json::Map<String, Value> {
    let [floor, ceil, out_floor, out_ceil, gamma] = record.map(f64::from);
    let mut out = serde_json::Map::new();
    out.insert("black".into(), json!(floor / 255.0));
    out.insert("white".into(), json!(ceil / 255.0));
    out.insert("gamma".into(), json!(gamma / 100.0));
    if (out_floor, out_ceil) != (0.0, 255.0) {
        out.insert("output_black".into(), json!(out_floor / 255.0));
        out.insert("output_white".into(), json!(out_ceil / 255.0));
    }
    out
}

fn meta(params: &Value) -> String {
    json!({"type": "adjust", "op": "levels", "params": params}).to_string()
}

fn typed_meta(params: &Value) -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": "levels", "params": params})
        .to_string()
}

/// The carried cases: name, records, and the params the importer must
/// produce from them.
fn carried_cases() -> Vec<(&'static str, Vec<[i32; 5]>, Value)> {
    let output = [0, 255, 20, 235, 100];
    let red = [0, 200, 0, 255, 100];
    let blue = [50, 255, 0, 255, 80];
    let master = [30, 220, 0, 255, 130];
    let green_out = [40, 255, 10, 245, 70];
    let mut both = stage(master);
    both.insert("g".into(), Value::Object(stage(green_out)));
    let mut channels = stage(IDENTITY);
    channels.insert("r".into(), Value::Object(stage(red)));
    channels.insert("b".into(), Value::Object(stage(blue)));
    let mut tail = stage(master);
    tail.insert("r".into(), Value::Object(stage(red)));
    let mut garbage = vec![master, red, IDENTITY, IDENTITY];
    garbage.extend([[9, 3, 400, 7, 0]; 25]);
    vec![
        ("Output", vec![output], Value::Object(stage(output))),
        (
            "Channels",
            vec![IDENTITY, red, IDENTITY, blue],
            Value::Object(channels),
        ),
        (
            "Both",
            vec![master, IDENTITY, green_out],
            Value::Object(both),
        ),
        // Records 4..=28 are not read at all, whatever they hold.
        ("Tail", garbage, Value::Object(tail)),
    ]
}

/// The bodies that still decline, with the clause after "whose".
fn declined_cases() -> Vec<(&'static str, Vec<u8>, &'static str)> {
    vec![
        ("V3", levl(3, &[IDENTITY]), "version is not 2"),
        (
            "GreenCeil",
            levl(2, &[IDENTITY, IDENTITY, [200, 200, 0, 255, 100]]),
            "input ceiling does not exceed its floor",
        ),
        (
            "BlueGamma",
            levl(2, &[IDENTITY, IDENTITY, IDENTITY, [0, 255, 0, 255, 5]]),
            "gamma is outside 0.10…9.99",
        ),
        (
            "OutCode",
            levl(2, &[[0, 255, 0, 300, 100]]),
            "parameters fall outside Doctored's ranges",
        ),
        (
            "RedOutCode",
            levl(2, &[IDENTITY, [0, 255, 0, 256, 100]]),
            "parameters fall outside Doctored's ranges",
        ),
    ]
}

fn empty8(name: &'static str, body: &[u8]) -> fixture8::Record {
    let mut record = fixture8::Record::raster([0; 4], &[], false);
    record.name = name;
    record.blocks = fixture8::block(b"levl", body);
    record
}

fn empty16(name: &str, body: &[u8]) -> fixture16::Record {
    let mut record = fixture16::Record::raster(
        name,
        [0; 4],
        [-1i16, 0, 1, 2]
            .into_iter()
            .map(|id| (id, fixture16::raw_plane(&[])))
            .collect(),
    );
    record.blocks = fixture16::block(b"levl", body);
    record
}

fn planes16(pixels: &[[u16; 4]]) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            (
                id,
                fixture16::raw_plane(&pixels.iter().map(|p| p[component]).collect::<Vec<_>>()),
            )
        })
        .collect()
}

fn file8(records: Vec<fixture8::Record>) -> Vec<u8> {
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &BASE, false);
    base.name = "Base";
    let mut all = vec![base];
    all.extend(records);
    fixture8::fixture(&all, CANVAS, false)
}

fn file16(records: Vec<fixture16::Record>) -> Vec<u8> {
    let mut all = vec![fixture16::Record::raster(
        "Base",
        [0, 0, 2, 2],
        planes16(&BASE16),
    )];
    all.extend(records);
    fixture16::fixture(
        (CANVAS[0], CANVAS[1]),
        &all,
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &BASE16,
        true,
        false,
    )
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

// ------------------------------------------------------------- decoding --

#[test]
fn per_channel_and_output_records_carry_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let cases = carried_cases();
    let path8 = write(
        &dir,
        "carried8.psd",
        &file8(
            cases
                .iter()
                .map(|(n, r, _)| empty8(n, &levl(2, r)))
                .collect(),
        ),
    );
    let report = import_report(&path8);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert!(
        !strings(&report, "verified_losses")
            .iter()
            .any(|l| l.contains("with no pixels whose")),
        "nothing declines: {report}"
    );
    let doc = open_psd(&path8, &report);
    let path16 = write(
        &dir,
        "carried16.psd",
        &file16(
            cases
                .iter()
                .map(|(n, r, _)| empty16(n, &levl(2, r)))
                .collect(),
        ),
    );
    let typed = open_typed(&path16, &import_report(&path16));
    for (index, (name, _, params)) in cases.iter().enumerate() {
        assert_eq!(doc.layers[index + 1].name, *name);
        assert_eq!(
            doc.layers[index + 1].meta.as_deref(),
            Some(meta(params).as_str()),
            "{name} at 8 bits"
        );
        assert_eq!(
            typed.adjustment_description(index + 1),
            Some(typed_meta(params).as_str()),
            "{name} at 16 bits"
        );
    }
}

#[test]
fn the_remaining_declines_keep_their_reasons() {
    let dir = TempDir::new().unwrap();
    let path = write(
        &dir,
        "declined8.psd",
        &file8(
            declined_cases()
                .iter()
                .map(|(n, b, _)| empty8(n, b))
                .collect(),
        ),
    );
    let report = import_report(&path);
    let losses = strings(&report, "verified_losses");
    for (name, _, reason) in declined_cases() {
        assert!(
            losses.iter().any(|l| l.contains(&format!(
                "(“{name}”) is a Photoshop Levels adjustment layer (levl) with no pixels whose {reason};"
            ))),
            "the {name} reason: {losses:?}"
        );
    }
    let doc = open_psd(&path, &report);
    assert_eq!(doc.layers.len(), 1, "every declined record is omitted");
}

// ------------------------------------------------------------- encoding --

fn exported_params() -> Value {
    json!({"black": 0.2, "white": 0.8, "gamma": 1.5, "output_black": 0.1, "output_white": 0.9,
           "r": {"black": 0.0, "white": 0.6, "gamma": 1.0},
           "b": {"black": 0.1, "white": 1.0, "gamma": 0.8, "output_black": 1.0,
                 "output_white": 0.0}})
}

/// The hand-encoded body `exported_params` must be written as.
fn exported_body() -> Vec<u8> {
    levl(
        2,
        &[
            [51, 204, 26, 230, 150],
            [0, 153, 0, 255, 100],
            IDENTITY,
            [26, 255, 255, 0, 80],
        ],
    )
}

#[test]
fn a_per_channel_levels_layer_writes_its_records_and_reopens_equal_at_8_bits() {
    let dir = TempDir::new().unwrap();
    let base = RgbaImage::from_raw(CANVAS[0], CANVAS[1], BASE.to_vec()).unwrap();
    let mut doc = DrDocument::from_pixels(base)
        .adding_image_layer(0, RgbaImage::new(CANVAS[0], CANVAS[1]), "Levels")
        .expect("add layer");
    doc.layers[1].meta = Some(meta(&exported_params()));
    let path = dir.path().join("exported8.psd");
    let mut error: *mut c_char = ptr::null_mut();
    assert!(
        unsafe {
            dr_doc_save_psd(
                &doc,
                ptr::null(),
                cpath(&path).as_ptr(),
                0,
                LAYERS,
                &mut error,
            )
        },
        "{}",
        take_err_string(error)
    );
    let bytes = std::fs::read(&path).unwrap();
    let bodies = blocks_in(&bytes, b"levl");
    assert_eq!(bodies, vec![exported_body()]);
    assert_eq!(bodies[0].len(), 292);

    let reopened = open_psd(&path, &import_report(&path));
    let carried: Value =
        serde_json::from_str(reopened.layers[1].meta.as_deref().expect("carried")).unwrap();
    // Equal to code precision: each value is the one its code stands for.
    let expected: Value = serde_json::from_str(&meta(&json!({
        "black": 51.0 / 255.0, "white": 204.0 / 255.0, "gamma": 1.5,
        "output_black": 26.0 / 255.0, "output_white": 230.0 / 255.0,
        "r": {"black": 0.0, "white": 153.0 / 255.0, "gamma": 1.0},
        "b": {"black": 26.0 / 255.0, "white": 1.0, "gamma": 0.8,
              "output_black": 1.0, "output_white": 0.0}})))
    .unwrap();
    assert_eq!(carried, expected);
}

#[test]
fn a_per_channel_levels_layer_writes_its_records_and_reopens_equal_at_16_bits() {
    let dir = TempDir::new().unwrap();
    let profile: Arc<IccProfile> = IccProfile::srgb();
    let image = PrecisionImage::from_rgba16(
        CANVAS[0],
        CANVAS[1],
        BASE16.to_vec(),
        Arc::clone(&profile),
        None,
    )
    .unwrap();
    let document = PrecisionDocument::from_parts(
        CANVAS[0],
        CANVAS[1],
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![
            PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap(),
            PrecisionDocumentLayer::adjustment(
                "Levels",
                &typed_meta(&exported_params()),
                1.0,
                true,
                &profile,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let projection = document
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render");
    let path = dir.path().join("exported16.psd");
    let mut error = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_psd(
                &document,
                &projection,
                cpath(&path).as_ptr(),
                0,
                LAYERS,
                REFERENCE_WORKING_BYTES,
                0,
                &mut error,
            )
        },
        "{}",
        take_err_string(error)
    );
    let file = fixture16::read(&std::fs::read(&path).unwrap());
    let record = fixture16::by_name(&file)["Levels"];
    let block = record
        .blocks
        .iter()
        .find(|(key, _)| key == b"levl")
        .expect("the levl block");
    assert_eq!(block.1, exported_body());

    let reopened = open_typed(&path, &import_report(&path));
    let carried: Value =
        serde_json::from_str(reopened.adjustment_description(1).expect("carried")).unwrap();
    assert_eq!(carried["params"]["r"]["white"], json!(153.0 / 255.0));
    assert_eq!(carried["params"]["b"]["output_black"], json!(1.0));
    assert_eq!(carried["params"]["output_white"], json!(230.0 / 255.0));
}
