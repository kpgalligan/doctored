//! The three adjustment kinds slice 2 phase 2 added to the codec — `expA`
//! (a binary body), `vibA` and `blwh` (descriptor bodies) — black-box
//! through the C API on both depths: hand-encoded records import as
//! `exposure`/`vibrance`/`black_and_white` layers with the expected metas,
//! declined bodies are omitted with their exact reason, unread descriptor
//! item types are skipped, and the three ops export as those records and
//! reopen equal.
//!
//! Every PSD read here is an independent byte construction
//! (`common/psd_mask_fixture.rs` at 8 bits, `common/psd16_fixture.rs` at 16)
//! and every expected body is hand-encoded with this file's OWN descriptor
//! builder, never the crate's writer.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument};
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
use crate::psd_descriptor_fixture as builder;
use crate::psd_mask_fixture as fixture8;
use builder::{body, boolean, doub, enumeration, item, long, long_list, rgbc, tdta, text, untf};
use common::{cpath, pixels, take_err_string};

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

// ------------------------------------------------------------- C plumbing --

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    assert!(error.is_null());
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
    assert!(error.is_null());
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

fn typed_info(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { doctored_core::ffi_precision::dr_precision_doc_info(document, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
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

fn save(doc: &DrDocument, path: &Path, mode: i32) -> Result<(), String> {
    let c = cpath(path);
    let mut error: *mut c_char = ptr::null_mut();
    if unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            c.as_ptr(),
            0,
            mode,
            std::ptr::null(),
            &mut error,
        )
    } {
        assert!(error.is_null(), "err_out set on success");
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

fn save16(
    document: &PrecisionDocument,
    projection: &PrecisionImage,
    path: &Path,
    mode: i32,
) -> Result<(), String> {
    let mut error = ptr::null_mut();
    if unsafe {
        dr_precision_doc_save_psd(
            document,
            projection,
            cpath(path).as_ptr(),
            0,
            mode,
            REFERENCE_WORKING_BYTES,
            0,
            std::ptr::null(),
            &mut error,
        )
    } {
        assert!(error.is_null(), "err_out set on success");
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array"))
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
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

// ----------------------------------------------------- hand-encoded bodies --

fn expa(version: u16, exposure: f32, offset: f32, gamma: f32) -> Vec<u8> {
    let mut out = version.to_be_bytes().to_vec();
    out.extend(exposure.to_be_bytes());
    out.extend(offset.to_be_bytes());
    out.extend(gamma.to_be_bytes());
    out.extend([0, 0]);
    out
}

fn viba(vibrance: i32, saturation: i32) -> Vec<u8> {
    body(
        16,
        b"null",
        &[long(b"vibrance", vibrance), long(b"Strt", saturation)],
    )
}

/// `blwh` items in Photoshop's order; `tint` is the pre-encoded `tintColor`
/// item so the callers can substitute a malformed one.
fn blwh_items(weights: [i32; 6], use_tint: u8, tint: Vec<u8>) -> Vec<Vec<u8>> {
    let mut items = vec![
        long(b"Rd  ", weights[0]),
        long(b"Yllw", weights[1]),
        long(b"Grn ", weights[2]),
        long(b"Cyn ", weights[3]),
        long(b"Bl  ", weights[4]),
        long(b"Mgnt", weights[5]),
        boolean(b"useTint", use_tint),
        tint,
    ];
    items.push(long(b"bwPresetKind", 1));
    items.push(text(b"blackAndWhitePresetFileName", ""));
    items
}

fn blwh(weights: [i32; 6], use_tint: u8, tint: [f64; 3]) -> Vec<u8> {
    body(
        16,
        b"null",
        &blwh_items(
            weights,
            use_tint,
            rgbc(b"tintColor", tint[0], tint[1], tint[2]),
        ),
    )
}

fn meta(op: &str, params: &Value) -> String {
    json!({"type": "adjust", "op": op, "params": params}).to_string()
}

fn typed_meta(op: &str, params: &Value) -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": op, "params": params}).to_string()
}

fn bw_params() -> Value {
    json!({
        "reds": 0.3, "yellows": -0.5, "greens": 1.2, "cyans": 0.6, "blues": 2.5, "magentas": 0.0,
        "tint": true, "tint_color": "#998a66"
    })
}

/// One record per kind — name, key, kind, op, body, and the params the
/// importer must produce (which are also the params export is authored with,
/// so the same body is expected back).
struct Kind {
    name: &'static str,
    key: [u8; 4],
    kind: &'static str,
    op: &'static str,
    body: Vec<u8>,
    params: Value,
}

fn kinds() -> Vec<Kind> {
    vec![
        Kind {
            name: "Exposure",
            key: *b"expA",
            kind: "Exposure",
            op: "exposure",
            body: expa(1, 1.5, -0.25, 2.0),
            params: json!({"evaluation": "document_v2", "exposure": 1.5, "gamma": 2.0, "offset": -0.25}),
        },
        Kind {
            name: "Vibrance",
            key: *b"vibA",
            kind: "Vibrance",
            op: "vibrance",
            body: viba(40, -20),
            params: json!({"saturation": -0.2, "vibrance": 0.4}),
        },
        Kind {
            name: "BlackWhite",
            key: *b"blwh",
            kind: "Black & White",
            op: "black_and_white",
            body: blwh([30, -50, 120, 60, 250, 0], 1, [153.0, 138.0, 102.0]),
            params: bw_params(),
        },
    ]
}

// ------------------------------------------------------------ 8-bit import --

fn empty8(name: &'static str, key: &[u8; 4], body: &[u8]) -> fixture8::Record {
    let mut record = fixture8::Record::raster([0; 4], &[], false);
    record.name = name;
    record.blocks = fixture8::block(key, body);
    record
}

fn mask8() -> fixture8::ExpectedMask {
    fixture8::ExpectedMask {
        bounds: [0, 0, 2, 2],
        coverage: vec![255, 128, 64, 0],
        outside: 0,
        enabled: true,
    }
}

fn base8() -> fixture8::Record {
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &BASE, false);
    base.name = "Base";
    base
}

/// Base plus the three kinds: Black & White masked, Vibrance clipped,
/// Exposure Multiply.
fn carried8() -> Vec<u8> {
    let mut records = vec![base8()];
    for kind in kinds() {
        let mut record = empty8(kind.name, &kind.key, &kind.body);
        match kind.name {
            "BlackWhite" => record = record.mask(&mask8(), false),
            "Vibrance" => record.clipping = 1,
            "Exposure" => record.blend = *b"mul ",
            _ => {}
        }
        records.push(record);
    }
    fixture8::fixture(&records, CANVAS, false)
}

#[test]
fn eight_bit_records_import_as_adjustment_layers_with_their_expected_metas() {
    let dir = TempDir::new().unwrap();
    let path = write(&dir, "descriptors8.psd", &carried8());
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");

    let losses = strings(&report, "verified_losses");
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("adjustment/fill metadata")),
        "a carried record has no ADJUSTMENT marker loss: {losses:?}"
    );
    assert!(
        !losses.iter().any(|l| l.contains("omits it")),
        "nothing is omitted: {losses:?}"
    );
    let observations = strings(&report, "observations");
    for kind in kinds() {
        let key = String::from_utf8_lossy(&kind.key).to_string();
        assert!(
            observations.iter().any(|o| o.contains(&format!("(“{}”) is a Photoshop {} adjustment layer ({key}); layered import carries it as a Doctored {} adjustment layer with its mask, opacity, blend mode and clipping. Its parameters are mapped onto Doctored's own arithmetic, so its appearance can differ from Photoshop's.", kind.name, kind.kind, kind.op))),
            "the {key} carried observation: {observations:?}"
        );
    }
    assert_eq!(
        observations
            .iter()
            .filter(|o| o.contains("layered import carries it"))
            .count(),
        3
    );
    assert!(
        !observations.iter().any(|o| o.contains("extends outside")),
        "{observations:?}"
    );

    let doc = open_psd(&path, &report);
    assert_eq!(doc.layers.len(), 4);
    assert_eq!(doc.layers[0].name, "Base");
    for kind in kinds() {
        let layer = doc
            .layers
            .iter()
            .find(|l| l.name == kind.name)
            .unwrap_or_else(|| panic!("layer {}", kind.name));
        assert_eq!(
            layer.meta.as_deref(),
            Some(meta(kind.op, &kind.params).as_str()),
            "{} meta",
            kind.name
        );
        assert_eq!(layer.pixels.dimensions(), (CANVAS[0], CANVAS[1]));
        assert_eq!(layer.offset, (0, 0));
        assert!(layer
            .pixels
            .contiguous_image()
            .as_raw()
            .iter()
            .all(|s| *s == 0));
        assert_eq!(layer.opacity, 1.0);
        assert!(layer.visible);
        match kind.name {
            "BlackWhite" => {
                let mask = layer.mask.as_ref().expect("the Black & White mask");
                assert_eq!(mask.as_raw(), &mask8().coverage);
                assert!(!layer.clipped);
                assert_eq!(layer.blend, BlendMode::Normal);
            }
            "Vibrance" => {
                assert!(layer.clipped, "the Vibrance clipping byte");
                assert!(layer.mask.is_none());
                assert_eq!(layer.blend, BlendMode::Normal);
            }
            "Exposure" => {
                assert_eq!(layer.blend, BlendMode::Multiply);
                assert!(layer.mask.is_none());
                assert!(!layer.clipped);
            }
            _ => unreachable!(),
        }
    }
    // Every carried meta evaluates on the 8-bit document (exposure through
    // the document profile, which the PSD import sets to sRGB).
    doc.flattened().expect("flatten");
}

#[test]
fn a_carried_vibrance_projects_exactly_as_the_destructive_op() {
    let dir = TempDir::new().unwrap();
    let vibrance = empty8("Vibrance", b"vibA", &viba(40, -20));
    let path = write(
        &dir,
        "vibrance8.psd",
        &fixture8::fixture(&[base8(), vibrance], CANVAS, false),
    );
    let report = import_report(&path);
    let doc = open_psd(&path, &report);
    let flat = doc.flattened().expect("flatten");

    let carried: Value = serde_json::from_str(doc.layers[1].meta.as_deref().unwrap()).unwrap();
    let params = CString::new(carried["params"].to_string()).unwrap();
    let op = CString::new("vibrance").unwrap();
    let image = unsafe { dr_image_from_rgba(BASE.as_ptr(), CANVAS[0], CANVAS[1]) };
    assert!(!image.is_null());
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(image, op.as_ptr(), params.as_ptr(), &mut error) };
    assert!(!out.is_null(), "{}", take_err_string(error));
    assert_eq!(flat.as_raw(), &pixels(out));
    unsafe {
        dr_image_free(out);
        dr_image_free(image);
    }
}

#[test]
fn unread_descriptor_item_types_before_the_keys_are_skipped() {
    let dir = TempDir::new().unwrap();
    let noisy = body(
        16,
        b"null",
        &[
            text(b"presetName", "Custom"),
            enumeration(b"presetKind", b"presetKindType", b"presetKindCustom"),
            untf(b"Amnt", b"#Prc", 12.5),
            tdta(b"blob", &[1, 2, 3]),
            long_list(b"numbers", &[1, -2, 3]),
            long(b"vibrance", 40),
            long(b"Strt", -20),
        ],
    );
    assert!(noisy.len().is_multiple_of(2), "{}", noisy.len());
    let vibrance = empty8("Vibrance", b"vibA", &noisy);
    let path = write(
        &dir,
        "noisy8.psd",
        &fixture8::fixture(&[base8(), vibrance], CANVAS, false),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert!(
        strings(&report, "observations")
            .iter()
            .any(|o| o.contains("(“Vibrance”) is a Photoshop Vibrance adjustment layer (vibA); layered import carries it")),
        "{report}"
    );
    let doc = open_psd(&path, &report);
    assert_eq!(
        doc.layers[1].meta.as_deref(),
        Some(meta("vibrance", &json!({"saturation": -0.2, "vibrance": 0.4})).as_str())
    );
}

// ---------------------------------------------------------- declined bodies --

/// Each declined variant, with the exact clause that follows "whose".
fn declined_records() -> Vec<(&'static str, [u8; 4], Vec<u8>, &'static str)> {
    let tint_long = long(b"tintColor", 0x998a66);
    let bad_tint = body(
        16,
        b"null",
        &blwh_items([40, 60, 40, 60, 20, 80], 1, tint_long),
    );
    let mut with_obj = blwh_items(
        [40, 60, 40, 60, 20, 80],
        0,
        rgbc(b"tintColor", 1.0, 2.0, 3.0),
    );
    with_obj.push(item(b"Ref ", b"obj ", &[0, 0, 0, 0]));
    let with_obj = body(16, b"null", &with_obj);
    let mut truncated = viba(40, -20);
    truncated.truncate(50);
    vec![
        (
            "ExpV2",
            *b"expA",
            expa(2, 0.0, 0.0, 1.0),
            "version is not 1",
        ),
        (
            "ExpShort",
            *b"expA",
            expa(1, 0.0, 0.0, 1.0)[..10].to_vec(),
            "body is shorter than its layout",
        ),
        (
            "Exp25",
            *b"expA",
            expa(1, 25.0, 0.0, 1.0),
            "parameters fall outside Doctored's ranges",
        ),
        (
            "VibV15",
            *b"vibA",
            body(15, b"null", &[long(b"vibrance", 40), long(b"Strt", -20)]),
            "descriptor version is not 16",
        ),
        (
            "VibDouble",
            *b"vibA",
            body(16, b"null", &[doub(b"vibrance", 0.4), long(b"Strt", -20)]),
            "descriptor key vibrance has an unexpected type",
        ),
        (
            "Vib150",
            *b"vibA",
            viba(150, 0),
            "parameters fall outside Doctored's ranges",
        ),
        (
            "Red350",
            *b"blwh",
            blwh([350, 60, 40, 60, 20, 80], 0, [153.0, 138.0, 102.0]),
            "parameters fall outside Doctored's ranges",
        ),
        (
            "TintLong",
            *b"blwh",
            bad_tint,
            "tint colour is not an RGB descriptor",
        ),
        (
            "WithRef",
            *b"blwh",
            with_obj,
            "descriptor uses an item type this importer does not read",
        ),
        (
            "Truncated",
            *b"vibA",
            truncated,
            "body is shorter than its layout",
        ),
    ]
}

#[test]
fn declined_bodies_are_omitted_with_their_exact_reason() {
    let dir = TempDir::new().unwrap();
    let mut records = vec![base8()];
    for (name, key, body, _) in declined_records() {
        assert!(body.len().is_multiple_of(2), "{name} body is even");
        records.push(empty8(name, &key, &body));
    }
    // Photoshop's own `SoCo` body (`depth16/source.psd`): carried.
    let soco = common::psd_photoshop::soco_body();
    records.push(empty8("Fill", b"SoCo", &soco));
    let path = write(
        &dir,
        "declined_descriptors8.psd",
        &fixture8::fixture(&records, CANVAS, false),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let losses = strings(&report, "verified_losses");
    for (name, key, _, reason) in declined_records() {
        let key = String::from_utf8_lossy(&key).to_string();
        assert!(
            losses.iter().any(|l| l.contains(&format!("(“{name}”) is a Photoshop"))
                && l.contains(&format!("adjustment layer ({key}) with no pixels whose {reason}; layered import omits it and its mask, so the layers beneath it appear without that adjustment. The stored composite keeps the saved appearance."))),
            "the {name} reason: {losses:?}"
        );
        assert!(
            losses
                .iter()
                .any(|l| l.contains(&format!("(“{name}”) has adjustment/fill metadata"))),
            "a declined record keeps its marker loss: {losses:?}"
        );
    }
    assert!(
        !losses.iter().any(|l| l.contains("(“Fill”)")),
        "the carried SoCo costs no loss: {losses:?}"
    );
    let observations = strings(&report, "observations");
    assert!(
        observations.iter().any(|o| o.contains("(“Fill”) is a Photoshop Solid Color Fill layer (SoCo) with no pixels; layered import carries it as a Doctored solid color fill layer")),
        "the SoCo is carried: {observations:?}"
    );
    for (name, _, _, _) in declined_records() {
        assert!(
            !observations
                .iter()
                .any(|o| o.contains(&format!("(“{name}”)"))
                    && o.contains("layered import carries it")),
            "{name} is not carried"
        );
    }
    let doc = open_psd(&path, &report);
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Base", "Fill"]);
    let numbers = common::psd_photoshop::rgbc_numbers(&soco);
    let fill = &doc.layers[1];
    common::psd_photoshop::assert_solid_description(
        fill.meta.as_deref().expect("the fill's description"),
        numbers,
        (CANVAS[0], CANVAS[1]),
        "declined_descriptors8",
    );
    for pixel in fill.pixels.to_contiguous().unwrap().pixels() {
        for c in 0..3 {
            assert!(
                (f64::from(pixel.0[c]) - numbers[c]).abs() <= 1.0,
                "{pixel:?} vs {numbers:?}"
            );
        }
        assert_eq!(pixel.0[3], 255);
    }
}

// ----------------------------------------------------------- 16-bit import --

fn empty16(name: &str, key: &[u8; 4], body: &[u8]) -> fixture16::Record {
    let mut record = fixture16::Record::raster(
        name,
        [0; 4],
        [-1i16, 0, 1, 2]
            .into_iter()
            .map(|id| (id, fixture16::raw_plane(&[])))
            .collect(),
    );
    record.blocks = fixture16::block(key, body);
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

fn carried16() -> Vec<u8> {
    let mut records = vec![fixture16::Record::raster(
        "Base",
        [0, 0, 2, 2],
        planes16(&BASE16),
    )];
    for kind in kinds() {
        let mut record = empty16(kind.name, &kind.key, &kind.body);
        match kind.name {
            "BlackWhite" => {
                record = record.mask(
                    [0, 0, 2, 2],
                    0,
                    true,
                    fixture16::raw_plane(&[65535, 32896, 16448, 0]),
                )
            }
            "Vibrance" => record.clipping = 1,
            "Exposure" => record.blend = *b"mul ",
            _ => {}
        }
        records.push(record);
    }
    fixture16::fixture(
        (CANVAS[0], CANVAS[1]),
        &records,
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &BASE16,
        true,
        false,
    )
}

#[test]
fn sixteen_bit_records_import_as_typed_adjustment_layers() {
    let dir = TempDir::new().unwrap();
    let path = write(&dir, "descriptors16.psd", &carried16());
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert_eq!(report["import_precision"], Value::from("rgba16"));
    let losses = strings(&report, "verified_losses");
    // A typed adjustment carries the blend key now (v37 / LBLD schema7).
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("16-bit adjustment layer cannot represent")),
        "{losses:?}"
    );
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("adjustment/fill metadata")),
        "{losses:?}"
    );
    assert_eq!(
        strings(&report, "observations")
            .iter()
            .filter(|o| o.contains("layered import carries it"))
            .count(),
        3
    );

    let document = open_typed(&path, &report);
    let info = typed_info(&document);
    let layers = info["layers"].as_array().expect("layers");
    assert_eq!(layers.len(), 4);
    for kind in kinds() {
        let index = layers
            .iter()
            .position(|l| l["name"].as_str() == Some(kind.name))
            .unwrap_or_else(|| panic!("layer {}", kind.name));
        assert_eq!(layers[index]["kind"], Value::from("adjustment"));
        assert_eq!(
            document.adjustment_description(index),
            Some(typed_meta(kind.op, &kind.params).as_str()),
            "{} description",
            kind.name
        );
        let layer = document.layer(index).unwrap();
        assert_eq!(
            layer.blend_mode(),
            if kind.name == "Exposure" {
                BlendMode::Multiply
            } else {
                BlendMode::Normal
            }
        );
        assert_eq!(
            layers[index]["blend_mode"],
            Value::from(if kind.name == "Exposure" { 1 } else { 0 })
        );
        assert_eq!(layer.clipped(), kind.name == "Vibrance");
        assert_eq!(layer.mask().is_some(), kind.name == "BlackWhite");
        if let Some(mask) = layer.mask() {
            assert_eq!(mask.plane().samples(), &[65535, 32896, 16448, 0]);
        }
    }
}

// ------------------------------------------------------------ 8-bit export --

fn exported_document(params_of: impl Fn(&Kind) -> Value) -> DrDocument {
    let base = RgbaImage::from_raw(CANVAS[0], CANVAS[1], BASE.to_vec()).unwrap();
    let mut doc = DrDocument::from_pixels(base);
    for (index, kind) in kinds().iter().enumerate() {
        doc = doc
            .adding_image_layer(index, RgbaImage::new(CANVAS[0], CANVAS[1]), kind.name)
            .expect("add layer");
        doc.layers[index + 1].meta = Some(meta(kind.op, &params_of(kind)));
    }
    doc
}

#[test]
fn eight_bit_export_writes_each_record_and_reopens_equal() {
    let dir = TempDir::new().unwrap();
    let doc = exported_document(|kind| kind.params.clone());
    let report = export_report(&doc, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    let observations = strings(&report, "observations");
    for kind in kinds() {
        let key = String::from_utf8_lossy(&kind.key).to_string();
        assert!(
            observations.iter().any(|o| o.contains(&format!("Adjustment layer “{}” ({}) is written as a Photoshop {} adjustment ({key}) with its mask, opacity, blend key and clipping; its parameters are mapped onto Photoshop's own arithmetic, so its appearance there is not qualified.", kind.name, kind.op, kind.kind))),
            "the {key} export observation: {observations:?}"
        );
    }
    assert_eq!(
        observations
            .iter()
            .filter(|o| o.contains("is written as a Photoshop"))
            .count(),
        3
    );

    let path = dir.path().join("exported_descriptors8.psd");
    save(&doc, &path, LAYERS).expect("save");
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        report["worst_case_bytes"].as_u64().unwrap() >= bytes.len() as u64,
        "worst case {} < written {}",
        report["worst_case_bytes"],
        bytes.len()
    );
    for kind in kinds() {
        let found = blocks_in(&bytes, &kind.key);
        assert_eq!(found, vec![kind.body.clone()], "the {} block", kind.name);
        assert!(found[0].len().is_multiple_of(2));
    }
    assert_eq!(blocks_in(&bytes, b"expA")[0].len(), 16);
    assert_eq!(blocks_in(&bytes, b"vibA")[0].len(), 58);
    assert_eq!(blocks_in(&bytes, b"blwh")[0].len(), 294);
    write(&dir, "exported_descriptors8.psd", &bytes);

    let reopened_report = import_report(&path);
    assert_eq!(
        reopened_report["layered_available"],
        Value::Bool(true),
        "{reopened_report}"
    );
    let reopened = open_psd(&path, &reopened_report);
    assert_eq!(reopened.layers.len(), 4);
    for (index, kind) in kinds().iter().enumerate() {
        assert_eq!(reopened.layers[index + 1].name, kind.name);
        assert_eq!(
            reopened.layers[index + 1].meta,
            doc.layers[index + 1].meta,
            "{} meta round trip",
            kind.name
        );
    }
    assert_eq!(
        reopened.flattened().unwrap().as_raw(),
        doc.flattened().unwrap().as_raw()
    );
}

#[test]
fn default_params_encode_the_default_bytes() {
    let dir = TempDir::new().unwrap();
    let doc = exported_document(|_| json!({}));
    let path = dir.path().join("defaults8.psd");
    save(&doc, &path, LAYERS).expect("save");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        blocks_in(&bytes, b"expA"),
        vec![expa(1, 0.0, 0.0, 1.0)],
        "the Exposure defaults"
    );
    assert_eq!(
        blocks_in(&bytes, b"vibA"),
        vec![viba(0, 0)],
        "the Vibrance defaults"
    );
    assert_eq!(
        blocks_in(&bytes, b"blwh"),
        vec![blwh([40, 60, 40, 60, 20, 80], 0, [153.0, 138.0, 102.0])],
        "the Black & White defaults"
    );
}

// ----------------------------------------------------------- 16-bit export --

fn exported_document16(profile: &Arc<IccProfile>) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        CANVAS[0],
        CANVAS[1],
        BASE16.to_vec(),
        Arc::clone(profile),
        None,
    )
    .unwrap();
    let mut layers =
        vec![PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap()];
    for kind in kinds() {
        layers.push(
            PrecisionDocumentLayer::adjustment(
                kind.name,
                &typed_meta(kind.op, &kind.params),
                1.0,
                true,
                profile,
            )
            .unwrap(),
        );
    }
    PrecisionDocument::from_parts(
        CANVAS[0],
        CANVAS[1],
        Arc::clone(profile),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

#[test]
fn sixteen_bit_export_writes_each_record_and_reopens_equal() {
    let dir = TempDir::new().unwrap();
    let profile = IccProfile::srgb();
    let document = exported_document16(&profile);
    let report = export_report16(&document, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert_eq!(
        strings(&report, "observations")
            .iter()
            .filter(|o| o.contains("is written as a Photoshop"))
            .count(),
        3
    );
    let projection = document
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render");
    let path = dir.path().join("exported_descriptors16.psd");
    save16(&document, &projection, &path, LAYERS).expect("save");
    let bytes = std::fs::read(&path).unwrap();
    assert!(report["worst_case_bytes"].as_u64().unwrap() >= bytes.len() as u64);
    write(&dir, "exported_descriptors16.psd", &bytes);

    let file = fixture16::read(&bytes);
    assert_eq!(file.depth, 16);
    let by_name = fixture16::by_name(&file);
    for kind in kinds() {
        let record = by_name[kind.name];
        assert_eq!(record.bounds, [0; 4]);
        let block = record
            .blocks
            .iter()
            .find(|(key, _)| *key == kind.key)
            .unwrap_or_else(|| panic!("the {} block", kind.name));
        assert_eq!(block.1, kind.body, "the {} body", kind.name);
    }

    let reopened_report = import_report(&path);
    assert_eq!(
        reopened_report["layered_available"],
        Value::Bool(true),
        "{reopened_report}"
    );
    let reopened = open_typed(&path, &reopened_report);
    for (index, kind) in kinds().iter().enumerate() {
        assert_eq!(
            reopened.adjustment_description(index + 1),
            document.adjustment_description(index + 1),
            "{} description round trip",
            kind.name
        );
        assert_eq!(
            reopened.adjustment_description(index + 1),
            Some(typed_meta(kind.op, &kind.params).as_str())
        );
    }
}

// --------------------------------------------------------------- refusals --

#[test]
fn brightness_contrast_without_saturation_is_written_as_a_record_on_both_depths() {
    let mut doc = DrDocument::from_pixels(RgbaImage::new(2, 2))
        .adding_image_layer(0, RgbaImage::new(2, 2), "Tone")
        .unwrap();
    doc.layers[1].meta = Some(meta("bcs", &json!({"brightness": 0.2})));
    let report = export_report(&doc, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert!(
        report["observations"].as_array().unwrap().iter().any(|line| line
            .as_str()
            .unwrap()
            .contains("Adjustment layer “Tone” (bcs) is written as a Photoshop Brightness/Contrast adjustment (brit)")),
        "{report}"
    );

    let profile = IccProfile::srgb();
    let image =
        PrecisionImage::from_rgba16(2, 2, vec![[1, 2, 3, 65535]; 4], Arc::clone(&profile), None)
            .unwrap();
    let typed = PrecisionDocument::from_parts(
        2,
        2,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![
            PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap(),
            PrecisionDocumentLayer::adjustment(
                "Tone",
                &typed_meta("bcs", &json!({"brightness": 0.2})),
                1.0,
                true,
                &profile,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let report = export_report16(&typed, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert!(
        report["observations"].as_array().unwrap().iter().any(|line| line
            .as_str()
            .unwrap()
            .contains("Adjustment layer “Tone” (bcs) is written as a Photoshop Brightness/Contrast adjustment (brit)")),
        "{report}"
    );
}
