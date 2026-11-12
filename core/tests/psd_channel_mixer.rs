//! The Channel Mixer record (`mixr`) carried as a Doctored
//! `channel_mixer` adjustment layer, black-box through the C API on both
//! depths: hand-encoded 44-byte bodies import with their exact metas, every
//! decline reason is exact, Doctored's op exports as the record and reopens
//! equal, the hidden-row loss is reported, and an imported body round-trips
//! byte for byte.
//!
//! The record order is pinned against PHOTOSHOP-AUTHORED files
//! (`tests/fixtures/psd_photoshop/channel_mixer/`, Photoshop 2025 26.10.0,
//! `scripts/photoshop-probe`'s `author` job with the `channel_mixer` kind):
//! their bodies and the Photoshop read-back of Doctored's own exports of them
//! (`reopen_8.json` / `reopen_16.json`, the `report` job). Every other body
//! here is an independent byte construction from that layout; the expected
//! render is the mix written out by hand.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_open_psd,
    dr_precision_doc_psd_export_report, dr_precision_doc_save_psd, dr_psd_import_report,
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
use common::{cpath, pixels, take_err_string};

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;
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
const PHOTOSHOP: &str = "tests/fixtures/psd_photoshop/channel_mixer";

const HIDDEN_COLOR: &str = "keeps Red, Green and Blue output rows hidden under Monochrome; Doctored writes only the gray row (Photoshop keeps Gray in the Red record), so they are not written and reopen at their defaults.";
const HIDDEN_GRAY: &str = "keeps a Monochrome gray row hidden while Monochrome is off; a Photoshop mixr record holds only the colour rows, so it is not written and reopens at its defaults.";

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
    let raw = unsafe {
        dr_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    unsafe { Box::from_raw(raw) }
}

/// A typed document handle freed through the C API on drop.
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

fn export_report(doc: &DrDocument, mode: i32) -> Value {
    let mut error: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, 0, mode, &mut error) };
    assert!(!json.is_null(), "export report: {}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

fn export_report16(document: &PrecisionDocument, mode: i32) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(document, 0, mode, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

fn save(doc: &DrDocument, path: &Path) {
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            LAYERS,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "save: {}", take_err_string(error));
}

fn save16(document: &PrecisionDocument, path: &Path) {
    let projection = document
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render");
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            document,
            &projection,
            cpath(path).as_ptr(),
            0,
            LAYERS,
            REFERENCE_WORKING_BYTES,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "save16: {}", take_err_string(error));
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array"))
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    if let Ok(directory) = std::env::var("DR_PSD_ADJUSTMENT_FIXTURES") {
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
    path
}

/// Every `8BIM{key}` block body in a written file, found by scanning for the
/// signature and key and reading the declared length.
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

// ------------------------------------------------------ hand-encoded bodies --

/// A `mixr` body from its version, monochrome word and four
/// `[R, G, B, K, constant]` records, big-endian two's-complement words.
fn mixr(version: i32, monochrome: i32, records: [[i32; 5]; 4]) -> Vec<u8> {
    [version, monochrome]
        .into_iter()
        .chain(records.into_iter().flatten())
        .flat_map(|v| (v as i16).to_be_bytes())
        .collect()
}

const ZERO: [i32; 5] = [0; 5];

/// Monochrome off with three distinct, non-default rows.
fn colour_body() -> Vec<u8> {
    mixr(
        1,
        0,
        [
            [80, 30, -10, 0, 5],
            [-20, 110, 15, 0, -4],
            [25, -35, 120, 0, 8],
            ZERO,
        ],
    )
}

fn colour_params() -> Value {
    json!({
        "monochrome": false,
        "red": {"r": 0.8, "g": 0.3, "b": -0.1, "constant": 0.05},
        "green": {"r": -0.2, "g": 1.1, "b": 0.15, "constant": -0.04},
        "blue": {"r": 0.25, "g": -0.35, "b": 1.2, "constant": 0.08},
    })
}

/// Monochrome on, gray at the extremes the range admits.
fn gray_body() -> Vec<u8> {
    mixr(1, 1, [[-200, 200, 35, 0, -6], ZERO, ZERO, ZERO])
}

fn gray_params() -> Value {
    json!({"monochrome": true, "gray": {"r": -2.0, "g": 2.0, "b": 0.35, "constant": -0.06}})
}

fn meta(params: &Value) -> String {
    json!({"type": "adjust", "op": "channel_mixer", "params": params}).to_string()
}

fn typed_meta(params: &Value) -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": "channel_mixer", "params": params})
        .to_string()
}

fn base8() -> fixture8::Record {
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &BASE, false);
    base.name = "Base";
    base
}

fn empty8(name: &'static str, body: &[u8]) -> fixture8::Record {
    let mut record = fixture8::Record::raster([0; 4], &[], false);
    record.name = name;
    record.blocks = fixture8::block(b"mixr", body);
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
    record.blocks = fixture16::block(b"mixr", body);
    record
}

fn base16() -> fixture16::Record {
    let planes = [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            (
                id,
                fixture16::raw_plane(&BASE16.iter().map(|p| p[component]).collect::<Vec<_>>()),
            )
        })
        .collect();
    fixture16::Record::raster("Base", [0, 0, 2, 2], planes)
}

fn psd16(records: &[fixture16::Record]) -> Vec<u8> {
    fixture16::fixture(
        (CANVAS[0], CANVAS[1]),
        records,
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &BASE16,
        true,
        false,
    )
}

fn carried_sentence(name: &str) -> String {
    format!("(“{name}”) is a Photoshop Channel Mixer adjustment layer (mixr); layered import carries it as a Doctored channel_mixer adjustment layer with its mask, opacity, blend mode and clipping. Its parameters are mapped onto Doctored's own arithmetic, so its appearance can differ from Photoshop's.")
}

// ------------------------------------------------------------ 8-bit import --

#[test]
fn eight_bit_bodies_import_as_channel_mixer_layers() {
    let dir = TempDir::new().unwrap();
    let mut tailed = colour_body();
    tailed.extend([0x12, 0x34, 0x56, 0x78]);
    let records = vec![
        base8(),
        empty8("Colour", &colour_body()),
        empty8("Gray", &gray_body()),
        empty8("Tailed", &tailed),
    ];
    let path = write(
        &dir,
        "mixer8.psd",
        &fixture8::fixture(&records, CANVAS, false),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let observations = strings(&report, "observations");
    for name in ["Colour", "Gray", "Tailed"] {
        assert!(
            observations
                .iter()
                .any(|o| o.ends_with(&carried_sentence(name))),
            "{name}: {observations:?}"
        );
    }
    assert!(
        !strings(&report, "verified_losses")
            .iter()
            .any(|l| l.contains("adjustment/fill metadata") || l.contains("omits it")),
        "{report}"
    );
    let doc = open_psd(&path, &report);
    assert_eq!(doc.layers.len(), 4);
    for (index, params) in [
        (1, colour_params()),
        (2, gray_params()),
        (3, colour_params()),
    ] {
        assert_eq!(
            doc.layers[index].meta.as_deref(),
            Some(meta(&params).as_str()),
            "{}",
            doc.layers[index].name
        );
    }
}

#[test]
fn a_carried_mixer_projects_exactly_as_the_destructive_op() {
    let dir = TempDir::new().unwrap();
    for (tag, body) in [("colour", colour_body()), ("gray", gray_body())] {
        let path = write(
            &dir,
            &format!("project_{tag}.psd"),
            &fixture8::fixture(&[base8(), empty8("Mixer", &body)], CANVAS, false),
        );
        let report = import_report(&path);
        let doc = open_psd(&path, &report);
        let flat = doc.flattened().expect("flatten");
        let carried: Value = serde_json::from_str(doc.layers[1].meta.as_deref().unwrap()).unwrap();
        let params = CString::new(carried["params"].to_string()).unwrap();
        let op = CString::new("channel_mixer").unwrap();
        let image = unsafe { dr_image_from_rgba(BASE.as_ptr(), CANVAS[0], CANVAS[1]) };
        assert!(!image.is_null());
        let mut error: *mut c_char = ptr::null_mut();
        let out = unsafe { dr_image_adjust_op(image, op.as_ptr(), params.as_ptr(), &mut error) };
        assert!(!out.is_null(), "{}", take_err_string(error));
        assert_eq!(flat.as_raw(), &pixels(out), "{tag}");
        unsafe {
            dr_image_free(out);
            dr_image_free(image);
        }
    }
}

// ---------------------------------------------------------- declined bodies --

/// Each declined variant, with the exact clause that follows "whose".
fn declined_records() -> Vec<(&'static str, Vec<u8>, &'static str)> {
    let mut short = colour_body();
    short.truncate(42);
    vec![
        ("Short", short, "body is shorter than its layout"),
        (
            "Version2",
            mixr(2, 0, [[100, 0, 0, 0, 0], ZERO, ZERO, ZERO]),
            "version is not 1",
        ),
        (
            "Record3",
            mixr(
                1,
                0,
                [
                    [100, 0, 0, 0, 0],
                    [0, 100, 0, 0, 0],
                    [0, 0, 100, 0, 0],
                    [0, 0, 0, 100, 0],
                ],
            ),
            "body holds CMYK channel settings",
        ),
        (
            "MonoK",
            mixr(1, 1, [[40, 40, 20, 10, 0], ZERO, ZERO, ZERO]),
            "body holds CMYK channel settings",
        ),
        (
            "Source201",
            mixr(
                1,
                0,
                [
                    [201, 0, 0, 0, 0],
                    [0, 100, 0, 0, 0],
                    [0, 0, 100, 0, 0],
                    ZERO,
                ],
            ),
            "source or constant percentage is outside -200…200",
        ),
        (
            "Constant201",
            mixr(1, 1, [[40, 40, 20, 0, -201], ZERO, ZERO, ZERO]),
            "source or constant percentage is outside -200…200",
        ),
    ]
}

#[test]
fn declined_bodies_are_omitted_with_their_exact_reason() {
    let dir = TempDir::new().unwrap();
    let mut records = vec![base8()];
    for (name, body, _) in declined_records() {
        records.push(empty8(name, &body));
    }
    // ±200 is inside the range: the shipped Infrared preset uses 200.
    records.push(empty8("Edge", &gray_body()));
    let path = write(
        &dir,
        "declined8.psd",
        &fixture8::fixture(&records, CANVAS, false),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let losses = strings(&report, "verified_losses");
    for (name, _, reason) in declined_records() {
        let sentence = format!("(“{name}”) is a Photoshop Channel Mixer adjustment layer (mixr) with no pixels whose {reason}; layered import omits it and its mask, so the layers beneath it appear without that adjustment. The stored composite keeps the saved appearance.");
        assert!(
            losses.iter().any(|l| l.ends_with(&sentence)),
            "{name}: {losses:?}"
        );
    }
    let doc = open_psd(&path, &report);
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Base", "Edge"]);
    assert_eq!(
        doc.layers[1].meta.as_deref(),
        Some(meta(&gray_params()).as_str())
    );
}

// ----------------------------------------------------------- 16-bit import --

#[test]
fn sixteen_bit_bodies_import_as_typed_channel_mixer_layers() {
    let dir = TempDir::new().unwrap();
    let path = write(
        &dir,
        "mixer16.psd",
        &psd16(&[
            base16(),
            empty16("Colour", &colour_body()),
            empty16("Gray", &gray_body()),
        ]),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert_eq!(report["import_precision"], Value::from("rgba16"));
    let document = open_typed(&path, &report);
    assert_eq!(
        document.adjustment_description(1),
        Some(typed_meta(&colour_params()).as_str())
    );
    assert_eq!(
        document.adjustment_description(2),
        Some(typed_meta(&gray_params()).as_str())
    );
}

// ------------------------------------------------------------------ export --

/// One exported case: the layer's params, the body it must be written as and
/// the params it must reopen with.
struct Exported {
    name: &'static str,
    params: Value,
    body: Vec<u8>,
    reopened: Value,
}

fn exported() -> Vec<Exported> {
    vec![
        // Blue is omitted, so the identity default row is written.
        Exported {
            name: "Colour",
            params: json!({
                "monochrome": false,
                "red": {"r": 0.8, "g": 0.3, "b": -0.1, "constant": 0.05},
                "green": {"r": -0.2, "g": 1.1, "b": 0.15, "constant": -0.04},
            }),
            body: mixr(
                1,
                0,
                [
                    [80, 30, -10, 0, 5],
                    [-20, 110, 15, 0, -4],
                    [0, 0, 100, 0, 0],
                    ZERO,
                ],
            ),
            reopened: json!({
                "monochrome": false,
                "red": {"r": 0.8, "g": 0.3, "b": -0.1, "constant": 0.05},
                "green": {"r": -0.2, "g": 1.1, "b": 0.15, "constant": -0.04},
                "blue": {"r": 0.0, "g": 0.0, "b": 1.0, "constant": 0.0},
            }),
        },
        Exported {
            name: "Gray",
            params: gray_params(),
            body: gray_body(),
            reopened: gray_params(),
        },
        // Monochrome on with gray omitted: the default gray row is written.
        Exported {
            name: "DefaultGray",
            params: json!({"monochrome": true}),
            body: mixr(1, 1, [[40, 40, 20, 0, 0], ZERO, ZERO, ZERO]),
            reopened: json!({"monochrome": true, "gray": {"r": 0.4, "g": 0.4, "b": 0.2, "constant": 0.0}}),
        },
    ]
}

fn exported_document() -> DrDocument {
    let base = RgbaImage::from_raw(CANVAS[0], CANVAS[1], BASE.to_vec()).unwrap();
    let mut doc = DrDocument::from_pixels(base);
    for (index, case) in exported().iter().enumerate() {
        doc = doc
            .adding_image_layer(index, RgbaImage::new(CANVAS[0], CANVAS[1]), case.name)
            .expect("add layer");
        doc.layers[index + 1].meta = Some(meta(&case.params));
    }
    doc
}

fn exported_document16() -> PrecisionDocument {
    typed_document(
        &exported()
            .iter()
            .map(|case| (case.name, case.params.clone()))
            .collect::<Vec<_>>(),
    )
}

fn typed_document(adjustments: &[(&str, Value)]) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let image = PrecisionImage::from_rgba16(
        CANVAS[0],
        CANVAS[1],
        BASE16.to_vec(),
        Arc::clone(&profile),
        None,
    )
    .unwrap();
    let mut layers =
        vec![PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap()];
    for (name, params) in adjustments {
        layers.push(
            PrecisionDocumentLayer::adjustment(name, &typed_meta(params), 1.0, true, &profile)
                .unwrap(),
        );
    }
    PrecisionDocument::from_parts(
        CANVAS[0],
        CANVAS[1],
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

#[test]
fn eight_bit_export_writes_each_mixr_and_reopens_with_its_params() {
    let dir = TempDir::new().unwrap();
    let doc = exported_document();
    let report = export_report(&doc, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    let observations = strings(&report, "observations");
    for case in exported() {
        assert!(
            observations.iter().any(|o| o.starts_with(&format!("Adjustment layer “{}” (channel_mixer) is written as a Photoshop Channel Mixer adjustment (mixr)", case.name))),
            "{}: {observations:?}",
            case.name
        );
    }
    let path = dir.path().join("exported8.psd");
    save(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    write(&dir, "exported8.psd", &bytes);
    assert_eq!(
        blocks_in(&bytes, b"mixr"),
        exported().into_iter().map(|c| c.body).collect::<Vec<_>>()
    );
    let reopened = open_psd(&path, &import_report(&path));
    for (index, case) in exported().iter().enumerate() {
        assert_eq!(reopened.layers[index + 1].name, case.name);
        assert_eq!(
            reopened.layers[index + 1].meta.as_deref(),
            Some(meta(&case.reopened).as_str()),
            "{}",
            case.name
        );
    }
    assert_eq!(
        reopened.flattened().unwrap().as_raw(),
        doc.flattened().unwrap().as_raw()
    );
}

#[test]
fn sixteen_bit_export_writes_each_mixr_and_reopens_with_its_params() {
    let dir = TempDir::new().unwrap();
    let document = exported_document16();
    let report = export_report16(&document, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    let path = dir.path().join("exported16.psd");
    save16(&document, &path);
    let bytes = std::fs::read(&path).unwrap();
    write(&dir, "exported16.psd", &bytes);
    let file = fixture16::read(&bytes);
    assert_eq!(file.depth, 16);
    let by_name = fixture16::by_name(&file);
    for case in exported() {
        let block = by_name[case.name]
            .blocks
            .iter()
            .find(|(key, _)| key == b"mixr")
            .unwrap_or_else(|| panic!("the {} block", case.name));
        assert_eq!(block.1, case.body, "{}", case.name);
    }
    let reopened = open_typed(&path, &import_report(&path));
    for (index, case) in exported().iter().enumerate() {
        assert_eq!(
            reopened.adjustment_description(index + 1),
            Some(typed_meta(&case.reopened).as_str()),
            "{}",
            case.name
        );
    }
}

/// The hidden-row cases: name, params, and the clause the layered export
/// reports (`None` = no hidden-row loss).
fn hidden_cases() -> Vec<(&'static str, Value, Option<&'static str>)> {
    vec![
        (
            "RedHidden",
            json!({"monochrome": true, "red": {"r": 1.5}}),
            Some(HIDDEN_COLOR),
        ),
        (
            "GrayHidden",
            json!({"monochrome": false, "gray": {"r": 0.5, "g": 0.3, "b": 0.2, "constant": 0.0}}),
            Some(HIDDEN_GRAY),
        ),
        // Hidden rows spelled out AT their defaults lose nothing.
        (
            "RowsAtDefault",
            json!({"monochrome": true, "red": {"r": 1.0, "g": 0.0, "b": 0.0, "constant": 0.0}}),
            None,
        ),
        (
            "GrayAtDefault",
            json!({"monochrome": false, "gray": {"r": 0.4, "g": 0.4, "b": 0.2, "constant": 0.0}}),
            None,
        ),
        ("Plain", json!({"monochrome": true}), None),
    ]
}

fn hidden_losses(report: &Value) -> Vec<String> {
    strings(report, "losses")
        .into_iter()
        .filter(|l| l.contains("(channel_mixer) keeps"))
        .collect()
}

fn expected_hidden_losses() -> Vec<String> {
    hidden_cases()
        .into_iter()
        .filter_map(|(name, _, clause)| {
            clause.map(|clause| format!("Adjustment layer “{name}” (channel_mixer) {clause}"))
        })
        .collect()
}

#[test]
fn a_hidden_row_that_differs_from_its_default_is_a_reported_loss_at_both_depths() {
    let base = RgbaImage::from_raw(CANVAS[0], CANVAS[1], BASE.to_vec()).unwrap();
    let mut doc = DrDocument::from_pixels(base);
    for (index, (name, params, _)) in hidden_cases().iter().enumerate() {
        doc = doc
            .adding_image_layer(index, RgbaImage::new(CANVAS[0], CANVAS[1]), name)
            .expect("add layer");
        doc.layers[index + 1].meta = Some(meta(params));
    }
    let layered = export_report(&doc, LAYERS);
    assert_eq!(layered["available"], Value::Bool(true), "{layered}");
    assert_eq!(hidden_losses(&layered), expected_hidden_losses());
    assert!(hidden_losses(&export_report(&doc, FLATTENED)).is_empty());

    let typed = typed_document(
        &hidden_cases()
            .into_iter()
            .map(|(name, params, _)| (name, params))
            .collect::<Vec<_>>(),
    );
    let layered16 = export_report16(&typed, LAYERS);
    assert_eq!(layered16["available"], Value::Bool(true), "{layered16}");
    assert_eq!(hidden_losses(&layered16), expected_hidden_losses());
    assert!(hidden_losses(&export_report16(&typed, FLATTENED)).is_empty());
}

#[test]
fn an_imported_body_is_written_back_byte_for_byte_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let bodies = [colour_body(), gray_body()];
    let path = write(
        &dir,
        "roundtrip8.psd",
        &fixture8::fixture(
            &[
                base8(),
                empty8("Colour", &bodies[0]),
                empty8("Gray", &bodies[1]),
            ],
            CANVAS,
            false,
        ),
    );
    let doc = open_psd(&path, &import_report(&path));
    let out = dir.path().join("roundtrip8_out.psd");
    save(&doc, &out);
    assert_eq!(blocks_in(&std::fs::read(&out).unwrap(), b"mixr"), bodies);

    let path16 = write(
        &dir,
        "roundtrip16.psd",
        &psd16(&[
            base16(),
            empty16("Colour", &bodies[0]),
            empty16("Gray", &bodies[1]),
        ]),
    );
    let typed = open_typed(&path16, &import_report(&path16));
    let out16 = dir.path().join("roundtrip16_out.psd");
    save16(&typed, &out16);
    assert_eq!(blocks_in(&std::fs::read(&out16).unwrap(), b"mixr"), bodies);
}

// ------------------------------------------------- Photoshop-authored files --

/// The three layers Photoshop authored (bottom-first after the Base) and the
/// exact `mixr` bodies it wrote for them, identical at 8 and 16 bits.
/// `MonoRows` was given Gray AND Red/Green/Blue rows: Photoshop keeps Gray in
/// the Red slot (record 0) and leaves the other two rows in records 1–2.
fn photoshop_bodies() -> [(&'static str, Vec<u8>); 3] {
    [
        ("Mixer", colour_body()),
        (
            "Mono",
            mixr(1, 1, [[-60, 150, 35, 0, -6], ZERO, ZERO, ZERO]),
        ),
        (
            "MonoRows",
            mixr(
                1,
                1,
                [
                    [70, 20, 10, 0, -2],
                    [5, 90, 5, 0, 1],
                    [-15, 10, 105, 0, 4],
                    ZERO,
                ],
            ),
        ),
    ]
}

/// The params each Photoshop layer carries in Doctored: records 1–3 are not
/// read under Monochrome.
fn photoshop_params() -> [(&'static str, Value); 3] {
    [
        ("Mixer", colour_params()),
        (
            "Mono",
            json!({"monochrome": true, "gray": {"r": -0.6, "g": 1.5, "b": 0.35, "constant": -0.06}}),
        ),
        (
            "MonoRows",
            json!({"monochrome": true, "gray": {"r": 0.7, "g": 0.2, "b": 0.1, "constant": -0.02}}),
        ),
    ]
}

fn photoshop_file(name: &str) -> PathBuf {
    Path::new(PHOTOSHOP).join(name)
}

#[test]
fn photoshop_written_bodies_pin_the_record_order_and_import_at_both_depths() {
    for name in ["source.psd", "source16.psd"] {
        let bytes = std::fs::read(photoshop_file(name)).expect(name);
        assert_eq!(
            blocks_in(&bytes, b"mixr"),
            photoshop_bodies()
                .into_iter()
                .map(|(_, body)| body)
                .collect::<Vec<_>>(),
            "{name}"
        );
    }
    let path = photoshop_file("source.psd");
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let doc = open_psd(&path, &report);
    let path16 = photoshop_file("source16.psd");
    let report16 = import_report(&path16);
    assert_eq!(
        report16["layered_available"],
        Value::Bool(true),
        "{report16}"
    );
    let typed = open_typed(&path16, &report16);
    for (index, (name, params)) in photoshop_params().into_iter().enumerate() {
        assert_eq!(doc.layers[index + 1].name, name);
        assert_eq!(
            doc.layers[index + 1].meta.as_deref(),
            Some(meta(&params).as_str()),
            "{name}"
        );
        assert_eq!(
            typed.adjustment_description(index + 1),
            Some(typed_meta(&params).as_str()),
            "{name} at 16 bits"
        );
    }
}

/// Photoshop's own flattened render of Base + one mixer (`mixer_8.png`,
/// `mono_8.png`) against Doctored's render of the same imported layer: the
/// mix is linear on encoded values in both, so they agree to one 8-bit code
/// (Photoshop's own rounding), a bound declared before measuring.
#[test]
fn the_carried_mixer_renders_within_one_code_of_photoshop() {
    let doc = open_psd(
        &photoshop_file("source.psd"),
        &import_report(&photoshop_file("source.psd")),
    );
    for (render, keep) in [("mixer_8.png", 1usize), ("mono_8.png", 2)] {
        let mut shown = (*doc).clone();
        for index in 1..shown.layers.len() {
            shown.layers[index].visible = index == keep;
        }
        let ours = shown.flattened().expect("flatten");
        let theirs = image::open(photoshop_file(render))
            .expect(render)
            .to_rgba8();
        assert_eq!(ours.dimensions(), theirs.dimensions());
        let worst = ours
            .pixels()
            .zip(theirs.pixels())
            .flat_map(|(a, b)| (0..3).map(move |c| a.0[c].abs_diff(b.0[c])))
            .max()
            .unwrap();
        assert!(worst <= 1, "{render}: worst difference {worst}");
    }
}

/// `rasterize_8.psd` / `rasterize_16.psd` are THIS writer's exports of the
/// Photoshop files, reopened in Photoshop 2025 to read the mixer settings
/// back (`reopen_8.json` / `reopen_16.json`, the probe's `report` job). The
/// check keeps that verdict tied to the writer — their `mixr` bodies must be
/// what it writes today — and reads Photoshop's verdict out of the read-back.
/// `DR_CHANNEL_MIXER_PSD_FIXTURES=<dir>` regenerates the two files.
#[test]
fn photoshop_reads_back_rasterize_written_mixers() {
    let doc = open_psd(
        &photoshop_file("source.psd"),
        &import_report(&photoshop_file("source.psd")),
    );
    let typed = open_typed(
        &photoshop_file("source16.psd"),
        &import_report(&photoshop_file("source16.psd")),
    );
    let dir = TempDir::new().unwrap();
    let ours8 = dir.path().join("rasterize_8.psd");
    let ours16 = dir.path().join("rasterize_16.psd");
    save(&doc, &ours8);
    save16(&typed, &ours16);
    if let Ok(out) = std::env::var("DR_CHANNEL_MIXER_PSD_FIXTURES") {
        let out = Path::new(&out);
        std::fs::copy(&ours8, out.join("rasterize_8.psd")).unwrap();
        std::fs::copy(&ours16, out.join("rasterize_16.psd")).unwrap();
    }
    // Monochrome drops MonoRows' records 1–2 (not read, not written).
    let written: Vec<Vec<u8>> = vec![
        colour_body(),
        mixr(1, 1, [[-60, 150, 35, 0, -6], ZERO, ZERO, ZERO]),
        mixr(1, 1, [[70, 20, 10, 0, -2], ZERO, ZERO, ZERO]),
    ];
    for (fresh, committed) in [(&ours8, "rasterize_8.psd"), (&ours16, "rasterize_16.psd")] {
        assert_eq!(blocks_in(&std::fs::read(fresh).unwrap(), b"mixr"), written);
        let bytes = std::fs::read(photoshop_file(committed)).expect(committed);
        assert_eq!(blocks_in(&bytes, b"mixr"), written, "{committed}");
    }

    // Photoshop's read-back, row by row in whole percent.
    let row = |matrix: &Value| {
        let values = &matrix["value"];
        [
            values["red"]["value"].as_f64().unwrap(),
            values["grain"]["value"].as_f64().unwrap(),
            values["blue"]["value"].as_f64().unwrap(),
            values["constant"]["value"].as_f64().unwrap(),
        ]
    };
    for name in ["reopen_8.json", "reopen_16.json"] {
        let reopen: Value =
            serde_json::from_slice(&std::fs::read(photoshop_file(name)).expect(name)).unwrap();
        assert_eq!(reopen["application"]["version"], json!("26.10.0"));
        assert_eq!(reopen["error"], Value::Null);
        let layer = |wanted: &str| -> Value {
            let rows = reopen["layers"].as_array().unwrap();
            let found = rows.iter().find(|l| l["name"] == json!(wanted)).unwrap();
            let list = found["adjustment"].as_array().expect("adjustment");
            assert_eq!(list[0]["object"], json!("channelMixer"), "{name} {wanted}");
            list[0]["value"].clone()
        };
        let mixer = layer("Mixer");
        assert_eq!(mixer["monochromatic"], json!(false), "{name}");
        assert_eq!(row(&mixer["red"]), [80.0, 30.0, -10.0, 5.0], "{name}");
        assert_eq!(row(&mixer["grain"]), [-20.0, 110.0, 15.0, -4.0], "{name}");
        assert_eq!(row(&mixer["blue"]), [25.0, -35.0, 120.0, 8.0], "{name}");
        let mono = layer("Mono");
        assert_eq!(mono["monochromatic"], json!(true), "{name}");
        assert_eq!(row(&mono["gray"]), [-60.0, 150.0, 35.0, -6.0], "{name}");
        let rows = layer("MonoRows");
        assert_eq!(rows["monochromatic"], json!(true), "{name}");
        assert_eq!(row(&rows["gray"]), [70.0, 20.0, 10.0, -2.0], "{name}");
    }
}
