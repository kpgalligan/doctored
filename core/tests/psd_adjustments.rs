//! Photoshop binary-body adjustment records carried as Doctored adjustment
//! layers, black-box through the C API on both depths: hand-encoded
//! `nvrt`/`thrs`/`post`/`levl`/`curv`/`blnc`/`hue2`/`phfl` bodies import with
//! the expected metas, declined bodies are omitted with their exact reason,
//! and Doctored's eight ops export as those records and reopen equal.
//!
//! Every PSD read here is an independent byte construction
//! (`common/psd_mask_fixture.rs` at 8 bits, `common/psd16_fixture.rs` at 16)
//! and every expected body is hand-encoded from Adobe's layout table.

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
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use crate::psd16_fixture as fixture16;
use crate::psd_mask_fixture as fixture8;
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
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), LAYERS, token.as_ptr(), &mut error) };
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
    if unsafe { dr_doc_save_psd(doc, ptr::null(), c.as_ptr(), 0, mode, &mut error) } {
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

// -------------------------------------------------------- hand-encoded bodies --

/// Big-endian 16-bit words; a negative value is its two's-complement `i16`.
fn w16(values: &[i32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|v| (*v as u16).to_be_bytes())
        .collect()
}

fn thrs(level: i32) -> Vec<u8> {
    w16(&[level, 0])
}

fn post(levels: i32) -> Vec<u8> {
    w16(&[levels, 0])
}

fn blnc(values: [i32; 9], preserve: u8) -> Vec<u8> {
    let mut body = w16(&values);
    body.extend([preserve, 0]);
    body
}

fn levl(version: i32, record0: [i32; 5]) -> Vec<u8> {
    let mut body = w16(&[version]);
    body.extend(w16(&record0));
    for _ in 1..29 {
        body.extend(w16(&[0, 255, 0, 255, 100]));
    }
    body
}

/// `channels` are `(input, output)` pairs per SET bit of `bits`, ascending;
/// the file stores each point as OUTPUT then INPUT, and one pad byte follows.
fn curv(is_map: u8, version: i32, bits: u32, channels: &[&[(i32, i32)]]) -> Vec<u8> {
    let mut body = vec![is_map];
    body.extend(w16(&[version]));
    body.extend(bits.to_be_bytes());
    for points in channels {
        body.extend(w16(&[points.len() as i32]));
        for (input, output) in *points {
            body.extend(w16(&[*output, *input]));
        }
    }
    if !body.len().is_multiple_of(2) {
        body.push(0);
    }
    body
}

fn hue2(
    version: i32,
    colorize: u8,
    tint: [i32; 3],
    master: [i32; 3],
    bands: [[i32; 7]; 6],
) -> Vec<u8> {
    let mut body = w16(&[version]);
    body.extend([colorize, 0]);
    body.extend(w16(&tint));
    body.extend(w16(&master));
    for band in bands {
        body.extend(w16(&band));
    }
    body
}

fn phfl_v2(space: i32, components: [i32; 4], density: u32, preserve: u8) -> Vec<u8> {
    let mut body = w16(&[2, space]);
    body.extend(w16(&components));
    body.extend(density.to_be_bytes());
    body.extend([preserve, 0, 0, 0]);
    body
}

fn phfl_v3() -> Vec<u8> {
    let mut body = w16(&[3]);
    body.extend([0; 12]);
    body.extend(25u32.to_be_bytes());
    body.extend([1, 0, 0, 0]);
    body
}

/// Photoshop's factory wheel: six symmetric bands, 15 deg inner and 30 deg
/// falloff, centred every 60 deg — except Reds, which the callers vary.
fn wheel(reds: [i32; 7]) -> [[i32; 7]; 6] {
    [
        reds,
        [15, 45, 75, 105, 0, 0, 0],
        [75, 105, 135, 165, 0, 0, 0],
        [135, 165, 195, 225, 0, 0, 0],
        [195, 225, 255, 285, 0, 0, 0],
        [255, 285, 315, 345, 0, 0, 0],
    ]
}

fn band(hue: f64, saturation: f64, lightness: f64, center: f64) -> Value {
    json!({"hue": hue, "saturation": saturation, "lightness": lightness,
        "center": center, "inner": 15.0, "falloff": 30.0})
}

fn hue_params() -> Value {
    json!({
        "hue": 10.0, "saturation": 0.2, "lightness": -0.1,
        "colorize": false, "colorize_hue": 0.0, "colorize_saturation": 0.25, "colorize_lightness": 0.0,
        "bands": {
            "reds": band(5.0, 0.1, -0.05, 5.0),
            "yellows": band(0.0, 0.0, 0.0, 60.0),
            "greens": band(0.0, 0.0, 0.0, 120.0),
            "cyans": band(0.0, 0.0, 0.0, 180.0),
            "blues": band(0.0, 0.0, 0.0, 240.0),
            "magentas": band(0.0, 0.0, 0.0, 300.0),
        }
    })
}

fn balance_params() -> Value {
    json!({
        "shadows": {"cyan_red": 0.1, "magenta_green": -0.2, "yellow_blue": 0.3},
        "midtones": {"cyan_red": 0.0, "magenta_green": 0.0, "yellow_blue": 0.0},
        "highlights": {"cyan_red": -0.05, "magenta_green": 0.05, "yellow_blue": 0.0},
        "preserve_luminosity": true
    })
}

fn meta(op: &str, params: &Value) -> String {
    json!({"type": "adjust", "op": op, "params": params}).to_string()
}

fn typed_meta(op: &str, params: &Value) -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": op, "params": params}).to_string()
}

/// The imported fixture: one hand-encoded record per kind — name, key, kind,
/// op, body, and the params the importer must produce.
struct Kind {
    name: &'static str,
    key: [u8; 4],
    kind: &'static str,
    op: &'static str,
    body: Vec<u8>,
    params: Value,
}

fn imported_kinds() -> Vec<Kind> {
    vec![
        Kind {
            name: "Invert",
            key: *b"nvrt",
            kind: "Invert",
            op: "invert",
            body: vec![],
            params: json!({}),
        },
        Kind {
            name: "Threshold",
            key: *b"thrs",
            kind: "Threshold",
            op: "threshold",
            body: thrs(128),
            params: json!({"level": 128.0 / 255.0}),
        },
        Kind {
            name: "Posterize",
            key: *b"post",
            kind: "Posterize",
            op: "posterize",
            body: post(4),
            params: json!({"levels": 4}),
        },
        Kind {
            name: "Levels",
            key: *b"levl",
            kind: "Levels",
            op: "levels",
            body: levl(2, [10, 240, 0, 255, 150]),
            params: json!({"black": 10.0 / 255.0, "white": 240.0 / 255.0, "gamma": 1.5}),
        },
        Kind {
            name: "Curves",
            key: *b"curv",
            kind: "Curves",
            op: "curves",
            body: curv(
                0,
                1,
                0b11,
                &[&[(0, 0), (255, 255)], &[(0, 0), (128, 160), (255, 255)]],
            ),
            params: json!({"rgb": [[0, 0], [255, 255]], "r": [[0, 0], [128, 160], [255, 255]]}),
        },
        Kind {
            name: "Balance",
            key: *b"blnc",
            kind: "Color Balance",
            op: "color_balance",
            body: blnc([10, -20, 30, 0, 0, 0, -5, 5, 0], 1),
            params: balance_params(),
        },
        Kind {
            name: "Hue",
            key: *b"hue2",
            kind: "Hue/Saturation",
            op: "hue_saturation",
            // Reds is ASYMMETRIC (arcs 20 and 40): it maps to inner 15,
            // centre 5 and the averaged falloff 30.
            body: hue2(
                2,
                0,
                [0, 25, 0],
                [10, 20, -10],
                wheel([330, 350, 20, 60, 5, 10, -5]),
            ),
            params: hue_params(),
        },
        Kind {
            name: "Filter",
            key: *b"phfl",
            kind: "Photo Filter",
            op: "photo_filter",
            body: phfl_v2(0, [0xec * 257, 0x8a * 257, 0, 0], 25, 0),
            params: json!({"color": "#ec8a00", "density": 0.25, "preserve_luminosity": false}),
        },
    ]
}

/// The exported document: Doctored metas with exactly representable
/// parameters, and the record bytes each must be written as.
struct Exported {
    name: &'static str,
    key: [u8; 4],
    op: &'static str,
    params: Value,
    body: Vec<u8>,
}

fn exported_ops() -> Vec<Exported> {
    vec![
        Exported {
            name: "Invert",
            key: *b"nvrt",
            op: "invert",
            params: json!({}),
            body: vec![],
        },
        Exported {
            name: "Threshold",
            key: *b"thrs",
            op: "threshold",
            params: json!({"level": 0.2}),
            body: thrs(51),
        },
        Exported {
            name: "Posterize",
            key: *b"post",
            op: "posterize",
            params: json!({"levels": 4}),
            body: post(4),
        },
        Exported {
            name: "Levels",
            key: *b"levl",
            op: "levels",
            params: json!({"black": 0.2, "white": 0.8, "gamma": 1.5}),
            body: levl(2, [51, 204, 0, 255, 150]),
        },
        Exported {
            name: "Curves",
            key: *b"curv",
            op: "curves",
            params: json!({"rgb": [[0, 0], [255, 255]]}),
            body: curv(0, 1, 1, &[&[(0, 0), (255, 255)]]),
        },
        Exported {
            name: "Balance",
            key: *b"blnc",
            op: "color_balance",
            params: balance_params(),
            body: blnc([10, -20, 30, 0, 0, 0, -5, 5, 0], 1),
        },
        Exported {
            name: "Hue",
            key: *b"hue2",
            op: "hue_saturation",
            params: hue_params(),
            body: hue2(
                2,
                0,
                [0, 25, 0],
                [10, 20, -10],
                wheel([320, 350, 20, 50, 5, 10, -5]),
            ),
        },
        Exported {
            name: "Filter",
            key: *b"phfl",
            op: "photo_filter",
            params: json!({"color": "#ec8a00", "density": 0.25, "preserve_luminosity": false}),
            body: phfl_v2(0, [0xec * 257, 0x8a * 257, 0, 0], 25, 0),
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

/// Base plus the eight kinds: Levels masked, Posterize clipped, Balance
/// Multiply.
fn carried8() -> Vec<u8> {
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &BASE, false);
    base.name = "Base";
    let mut records = vec![base];
    for kind in imported_kinds() {
        let mut record = empty8(kind.name, &kind.key, &kind.body);
        match kind.name {
            "Levels" => record = record.mask(&mask8(), false),
            "Posterize" => record.clipping = 1,
            "Balance" => record.blend = *b"mul ",
            _ => {}
        }
        records.push(record);
    }
    fixture8::fixture(&records, CANVAS, false)
}

#[test]
fn eight_bit_bodies_import_as_adjustment_layers_with_their_expected_metas() {
    let dir = TempDir::new().unwrap();
    let path = write(&dir, "carried8.psd", &carried8());
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
    for kind in imported_kinds() {
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
        8
    );
    assert!(
        !observations.iter().any(|o| o.contains("extends outside")),
        "{observations:?}"
    );

    let doc = open_psd(&path, &report);
    assert_eq!(doc.layers.len(), 9);
    assert_eq!(doc.layers[0].name, "Base");
    for kind in imported_kinds() {
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
            "Levels" => {
                let mask = layer.mask.as_ref().expect("the Levels mask");
                assert_eq!(mask.as_raw(), &mask8().coverage);
            }
            "Posterize" => assert!(layer.clipped, "the Posterize clipping byte"),
            "Balance" => assert_eq!(layer.blend, BlendMode::Multiply),
            _ => {
                assert!(layer.mask.is_none());
                assert!(!layer.clipped);
                assert_eq!(layer.blend, BlendMode::Normal);
            }
        }
    }
}

#[test]
fn a_carried_threshold_projects_exactly_as_the_destructive_op() {
    let dir = TempDir::new().unwrap();
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &BASE, false);
    base.name = "Base";
    let threshold = empty8("Threshold", b"thrs", &thrs(128));
    let path = write(
        &dir,
        "threshold8.psd",
        &fixture8::fixture(&[base, threshold], CANVAS, false),
    );
    let report = import_report(&path);
    let doc = open_psd(&path, &report);
    let flat = doc.flattened().expect("flatten");

    // The destructive twin over the same base, with the params the layer
    // carries — black box on both sides.
    let carried: Value = serde_json::from_str(doc.layers[1].meta.as_deref().unwrap()).unwrap();
    let params = CString::new(carried["params"].to_string()).unwrap();
    let op = CString::new("threshold").unwrap();
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

// ---------------------------------------------------------- declined bodies --

/// Each declined variant, with the exact clause that follows "whose".
fn declined_records() -> Vec<(&'static str, [u8; 4], Vec<u8>, &'static str)> {
    vec![
        ("Post200", *b"post", post(200), "level count exceeds 64"),
        (
            "CurveMap",
            *b"curv",
            curv(2, 1, 1, &[&[(0, 0), (255, 255)]]),
            "map flag is not 0 or 1",
        ),
        (
            "FilterXYZ",
            *b"phfl",
            phfl_v3(),
            "XYZ colour form is not mapped",
        ),
        (
            "FilterCMYK",
            *b"phfl",
            phfl_v2(2, [0, 0, 0, 0], 25, 1),
            "colour space is not RGB or Lab",
        ),
        (
            "HueV3",
            *b"hue2",
            hue2(
                3,
                0,
                [0, 25, 0],
                [0, 0, 0],
                wheel([315, 345, 15, 45, 0, 0, 0]),
            ),
            "version is not 2",
        ),
        (
            "Thrs300",
            *b"thrs",
            thrs(300),
            "parameters fall outside Doctored's ranges",
        ),
        (
            "HueFalloff",
            *b"hue2",
            hue2(
                2,
                0,
                [0, 25, 0],
                [0, 0, 0],
                wheel([0, 200, 210, 50, 0, 0, 0]),
            ),
            "parameters fall outside Doctored's ranges",
        ),
        // An odd block length is refused by the record walk itself, so the
        // shortest legal body is the empty one.
        (
            "ThrsShort",
            *b"thrs",
            vec![],
            "body is shorter than its layout",
        ),
    ]
}

#[test]
fn declined_bodies_are_omitted_with_their_exact_reason() {
    let dir = TempDir::new().unwrap();
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &BASE, false);
    base.name = "Base";
    let mut records = vec![base];
    for (name, key, body, _) in declined_records() {
        records.push(empty8(name, &key, &body));
    }
    // Photoshop's own `SoCo` body (`depth16/source.psd`): carried.
    let soco = common::psd_photoshop::soco_body();
    records.push(empty8("Fill", b"SoCo", &soco));
    let path = write(
        &dir,
        "declined8.psd",
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
        "declined8",
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
    for kind in imported_kinds() {
        let mut record = empty16(kind.name, &kind.key, &kind.body);
        match kind.name {
            "Levels" => {
                record = record.mask(
                    [0, 0, 2, 2],
                    0,
                    true,
                    fixture16::raw_plane(&[65535, 32896, 16448, 0]),
                )
            }
            "Posterize" => record.clipping = 1,
            "Balance" => record.blend = *b"mul ",
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
fn sixteen_bit_bodies_import_as_typed_adjustment_layers() {
    let dir = TempDir::new().unwrap();
    let path = write(&dir, "carried16.psd", &carried16());
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
        8
    );

    let document = open_typed(&path, &report);
    let info = typed_info(&document);
    let layers = info["layers"].as_array().expect("layers");
    assert_eq!(layers.len(), 9);
    for kind in imported_kinds() {
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
            if kind.name == "Balance" {
                BlendMode::Multiply
            } else {
                BlendMode::Normal
            }
        );
        assert_eq!(
            layers[index]["blend_mode"],
            Value::from(if kind.name == "Balance" { 1 } else { 0 })
        );
        assert_eq!(layer.clipped(), kind.name == "Posterize");
        assert_eq!(layer.mask().is_some(), kind.name == "Levels");
        if let Some(mask) = layer.mask() {
            assert_eq!(mask.plane().samples(), &[65535, 32896, 16448, 0]);
        }
    }
}

// ------------------------------------------------------------ 8-bit export --

fn exported_document() -> DrDocument {
    let base = RgbaImage::from_raw(CANVAS[0], CANVAS[1], BASE.to_vec()).unwrap();
    let mut doc = DrDocument::from_pixels(base);
    for (index, op) in exported_ops().iter().enumerate() {
        doc = doc
            .adding_image_layer(index, RgbaImage::new(CANVAS[0], CANVAS[1]), op.name)
            .expect("add layer");
        doc.layers[index + 1].meta = Some(meta(op.op, &op.params));
    }
    doc
}

#[test]
fn eight_bit_export_writes_each_record_and_reopens_equal() {
    let dir = TempDir::new().unwrap();
    let doc = exported_document();
    let report = export_report(&doc, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    let observations = strings(&report, "observations");
    for op in exported_ops() {
        let key = String::from_utf8_lossy(&op.key).to_string();
        assert!(
            observations.iter().any(|o| o.contains(&format!("Adjustment layer “{}” ({}) is written as a Photoshop", op.name, op.op))
                && o.contains(&format!("adjustment ({key}) with its mask, opacity, blend key and clipping; its parameters are mapped onto Photoshop's own arithmetic, so its appearance there is not qualified."))),
            "the {key} export observation: {observations:?}"
        );
    }
    assert_eq!(
        observations
            .iter()
            .filter(|o| o.contains("is written as a Photoshop"))
            .count(),
        8
    );
    assert!(
        !strings(&report, "losses")
            .iter()
            .any(|l| l.contains("re-editable description")),
        "an adjustment layer is not a described layer"
    );

    let path = dir.path().join("exported8.psd");
    save(&doc, &path, LAYERS).expect("save");
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        report["worst_case_bytes"].as_u64().unwrap() >= bytes.len() as u64,
        "worst case {} < written {}",
        report["worst_case_bytes"],
        bytes.len()
    );
    for op in exported_ops() {
        let found = blocks_in(&bytes, &op.key);
        assert_eq!(found, vec![op.body.clone()], "the {} block", op.name);
        assert!(found[0].len().is_multiple_of(2));
    }
    assert_eq!(blocks_in(&bytes, b"curv")[0].len(), 18);
    write(&dir, "exported8.psd", &bytes);

    let reopened_report = import_report(&path);
    assert_eq!(
        reopened_report["layered_available"],
        Value::Bool(true),
        "{reopened_report}"
    );
    let reopened = open_psd(&path, &reopened_report);
    assert_eq!(reopened.layers.len(), 9);
    for (index, op) in exported_ops().iter().enumerate() {
        assert_eq!(reopened.layers[index + 1].name, op.name);
        assert_eq!(
            reopened.layers[index + 1].meta,
            doc.layers[index + 1].meta,
            "{} meta round trip",
            op.name
        );
    }
    assert_eq!(
        reopened.flattened().unwrap().as_raw(),
        doc.flattened().unwrap().as_raw()
    );
}

#[test]
fn curves_blocks_are_even_for_one_two_and_four_channels() {
    let dir = TempDir::new().unwrap();
    // An `r`-only meta is written with bit 0's identity (bit 0 is always
    // present), so it reopens with that identity `rgb` spelled out.
    for (tag, params, reopened_params, expected) in [
        (
            "one",
            json!({"rgb": [[0, 0], [255, 255]]}),
            json!({"rgb": [[0, 0], [255, 255]]}),
            curv(0, 1, 0b1, &[&[(0, 0), (255, 255)]]),
        ),
        (
            "two",
            json!({"r": [[0, 0], [128, 160], [255, 255]]}),
            json!({"r": [[0, 0], [128, 160], [255, 255]], "rgb": [[0, 0], [255, 255]]}),
            curv(
                0,
                1,
                0b11,
                &[&[(0, 0), (255, 255)], &[(0, 0), (128, 160), (255, 255)]],
            ),
        ),
        (
            "four",
            json!({"rgb": [[0, 10], [255, 245]], "r": [[0, 0], [255, 255]], "g": [[0, 0], [64, 80], [192, 176], [255, 255]], "b": [[0, 0], [255, 255]]}),
            json!({"rgb": [[0, 10], [255, 245]], "r": [[0, 0], [255, 255]], "g": [[0, 0], [64, 80], [192, 176], [255, 255]], "b": [[0, 0], [255, 255]]}),
            curv(
                0,
                1,
                0b1111,
                &[
                    &[(0, 10), (255, 245)],
                    &[(0, 0), (255, 255)],
                    &[(0, 0), (64, 80), (192, 176), (255, 255)],
                    &[(0, 0), (255, 255)],
                ],
            ),
        ),
    ] {
        let base = RgbaImage::from_raw(CANVAS[0], CANVAS[1], BASE.to_vec()).unwrap();
        let mut doc = DrDocument::from_pixels(base)
            .adding_image_layer(0, RgbaImage::new(CANVAS[0], CANVAS[1]), "Curves")
            .unwrap();
        doc.layers[1].meta = Some(meta("curves", &params));
        let path = dir.path().join(format!("curves_{tag}.psd"));
        save(&doc, &path, LAYERS).expect("save");
        let bytes = std::fs::read(&path).unwrap();
        let found = blocks_in(&bytes, b"curv");
        assert_eq!(found, vec![expected], "{tag}");
        assert!(found[0].len().is_multiple_of(2), "{tag}");
        let report = import_report(&path);
        assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
        let reopened = open_psd(&path, &report);
        assert_eq!(
            reopened.layers[1].meta.as_deref(),
            Some(meta("curves", &reopened_params).as_str()),
            "{tag}"
        );
    }
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
    for op in exported_ops() {
        layers.push(
            PrecisionDocumentLayer::adjustment(
                op.name,
                &typed_meta(op.op, &op.params),
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
        8
    );
    let projection = document
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render");
    let path = dir.path().join("exported16.psd");
    save16(&document, &projection, &path, LAYERS).expect("save");
    let bytes = std::fs::read(&path).unwrap();
    assert!(report["worst_case_bytes"].as_u64().unwrap() >= bytes.len() as u64);
    write(&dir, "exported16.psd", &bytes);

    let file = fixture16::read(&bytes);
    assert_eq!(file.depth, 16);
    let by_name = fixture16::by_name(&file);
    for op in exported_ops() {
        let record = by_name[op.name];
        assert_eq!(record.bounds, [0; 4]);
        let block = record
            .blocks
            .iter()
            .find(|(key, _)| *key == op.key)
            .unwrap_or_else(|| panic!("the {} block", op.name));
        assert_eq!(block.1, op.body, "the {} body", op.name);
    }

    let reopened_report = import_report(&path);
    assert_eq!(
        reopened_report["layered_available"],
        Value::Bool(true),
        "{reopened_report}"
    );
    let reopened = open_typed(&path, &reopened_report);
    for (index, op) in exported_ops().iter().enumerate() {
        assert_eq!(
            reopened.adjustment_description(index + 1),
            document.adjustment_description(index + 1),
            "{} description round trip",
            op.name
        );
        assert_eq!(
            reopened.adjustment_description(index + 1),
            Some(typed_meta(op.op, &op.params).as_str())
        );
    }
}

// --------------------------------------------------------------- refusals --

/// An op with no Photoshop record no longer refuses: it is BAKED into a pixel
/// layer and reported as a loss naming the op, at both depths. `bcs`
/// has a record only without saturation, so a saturation makes it record-less.
#[test]
fn an_op_without_a_record_is_baked_and_named_as_a_loss_on_both_depths() {
    let dir = TempDir::new().unwrap();
    let baked =
        "Adjustment layer “Tone” (bcs) has no Photoshop record and is written as a pixel layer.";
    let params = json!({"brightness": 0.2, "saturation": 0.3});

    let mut doc = DrDocument::from_pixels(RgbaImage::new(2, 2))
        .adding_image_layer(0, RgbaImage::new(2, 2), "Tone")
        .unwrap();
    doc.layers[1].meta = Some(meta("bcs", &params));
    let report = export_report(&doc, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert_eq!(report["refusal"], Value::Null);
    assert!(
        report["losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss.as_str().unwrap().starts_with(baked)),
        "{report}"
    );
    // Three canvas-sized RGBA8 planes for the stamp.
    assert_eq!(report["bake_scratch_bytes"], json!(3 * 4 * 2 * 2));
    save(&doc, &dir.path().join("baked8.psd"), LAYERS).expect("a baked adjustment exports");

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
                &typed_meta("bcs", &params),
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
        report["losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss.as_str().unwrap().starts_with(baked)),
        "{report}"
    );
    assert_eq!(report["bake_scratch_bytes"], json!(3 * 4 * 2 * 2 * 2));
    let projection = typed.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    save16(&typed, &projection, &dir.path().join("baked16.psd"), LAYERS)
        .expect("a baked typed adjustment exports");
}

// ------------------------------------------- Photoshop 2025's own forms --
//
// The three cases below are the forms a real Photoshop 26.10.0 file carries
// that a hand-built fixture never produced: a `phfl` in colour space 7 (Lab,
// which is what Photoshop writes for EVERY photo filter — RGB picker, Lab
// picker, preset and default alike), a 20-byte mask body with an empty
// rectangle and a bare -2 channel on every adjustment record, and a layer
// information section rounded to a multiple of 4 rather than to an even
// count.

/// A `phfl` v2 body in colour space 7: the four components are the PSD colour
/// structure's Lab, SIGNED 16-bit hundredths.
fn phfl_lab(l: i32, a: i32, b: i32, density: u32, preserve: u8) -> Vec<u8> {
    phfl_v2(7, [l, a, b, 0], density, preserve)
}

/// Name, body, and the sRGB colour and density the importer must produce.
/// Every expected channel is at least 0.27 of a code from a rounding
/// boundary, so the f32 arithmetic cannot move one.
fn lab_cases() -> Vec<(&'static str, Vec<u8>, &'static str, f64)> {
    vec![
        // L 67.19 a 33.14 b 72.79 — Photoshop's own Warming Filter (85).
        ("LabWarm", phfl_lab(6719, 3314, 7279, 40, 1), "#ec8a00", 0.4),
        // The same hue pushed past the sRGB gamut: blue clamps at 0.
        (
            "LabClamp",
            phfl_lab(6706, 3200, 12000, 25, 1),
            "#ec8a00",
            0.25,
        ),
        ("LabTan", phfl_lab(7400, 1200, 4400, 25, 1), "#ddad64", 0.25),
        // Negative a and b: the two's-complement read is what makes a
        // cooling filter cool rather than absurd.
        (
            "LabCool",
            phfl_lab(8000, -1000, -2000, 25, 1),
            "#a0cdeb",
            0.25,
        ),
    ]
}

fn lab_params(color: &str, density: f64) -> Value {
    json!({"color": color, "density": density, "preserve_luminosity": true})
}

/// The colour `rgb` carried from `from`'s space into `to`'s through the same
/// document conversion a host would run — the independent oracle for the
/// decoded Lab colour's conversion into a non-sRGB document.
fn converted(rgb: [u8; 3], from: &Arc<IccProfile>, to: &Arc<IccProfile>) -> [u8; 3] {
    let mut pixels = RgbaImage::new(1, 1);
    pixels.put_pixel(0, 0, Rgba([rgb[0], rgb[1], rgb[2], 255]));
    let mut document = DrDocument::from_pixels(pixels);
    document.profile = Arc::clone(from);
    let out = document
        .convert_to_profile(to)
        .expect("the conversion succeeds")
        .expect("a conversion between two different profiles");
    let pixel_image = out.layers[0].pixels.contiguous_image();
    let pixel = pixel_image.get_pixel(0, 0);
    [pixel[0], pixel[1], pixel[2]]
}

fn resource(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend(id.to_be_bytes());
    out.extend([0, 0]);
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    if !body.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

/// `extra` appended to the file's image-resource section, whose length field
/// sits at offset 30, immediately after the empty colour-mode data.
fn with_resources(mut bytes: Vec<u8>, extra: &[u8]) -> Vec<u8> {
    assert_eq!(
        &bytes[26..30],
        &[0; 4],
        "the fixture writes no colour-mode data"
    );
    let len = u32::from_be_bytes(bytes[30..34].try_into().unwrap()) as usize;
    bytes.splice(34 + len..34 + len, extra.iter().copied());
    bytes[30..34].copy_from_slice(&((len + extra.len()) as u32).to_be_bytes());
    bytes
}

/// The same file with `pad` zero bytes appended to its layer INFORMATION
/// section, which is how Photoshop rounds that section to a multiple of 4.
fn with_layer_info_padding(mut bytes: Vec<u8>, pad: usize) -> Vec<u8> {
    let resources = u32::from_be_bytes(bytes[30..34].try_into().unwrap()) as usize;
    let section = 34 + resources;
    let total = u32::from_be_bytes(bytes[section..section + 4].try_into().unwrap()) as usize;
    let info_at = section + 4;
    let info = u32::from_be_bytes(bytes[info_at..info_at + 4].try_into().unwrap()) as usize;
    let end = info_at + 4 + info;
    bytes.splice(end..end, std::iter::repeat_n(0u8, pad));
    bytes[section..section + 4].copy_from_slice(&((total + pad) as u32).to_be_bytes());
    bytes[info_at..info_at + 4].copy_from_slice(&((info + pad) as u32).to_be_bytes());
    bytes
}

#[test]
fn lab_photo_filter_bodies_carry_their_colour_as_srgb_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &BASE, false);
    base.name = "Base";
    let mut records = vec![base];
    for (name, body, _, _) in lab_cases() {
        records.push(empty8(name, b"phfl", &body));
    }
    let path = write(
        &dir,
        "lab8.psd",
        &fixture8::fixture(&records, CANVAS, false),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert!(
        !strings(&report, "verified_losses")
            .iter()
            .any(|l| l.contains("omits it")),
        "{report}"
    );
    let doc = open_psd(&path, &report);
    for (name, _, color, density) in lab_cases() {
        let layer = doc
            .layers
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("layer {name}"));
        assert_eq!(
            layer.meta.as_deref(),
            Some(meta("photo_filter", &lab_params(color, density)).as_str()),
            "{name}"
        );
    }

    // The 16-bit twin, through the typed importer.
    let mut records16 = vec![fixture16::Record::raster(
        "Base",
        [0, 0, 2, 2],
        planes16(&BASE16),
    )];
    for (name, body, _, _) in lab_cases() {
        records16.push(empty16(name, b"phfl", &body));
    }
    let bytes = fixture16::fixture(
        (CANVAS[0], CANVAS[1]),
        &records16,
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &BASE16,
        true,
        false,
    );
    let path = write(&dir, "lab16.psd", &bytes);
    let report = import_report(&path);
    assert_eq!(
        report["import_precision"],
        Value::from("rgba16"),
        "{report}"
    );
    let document = open_typed(&path, &report);
    let info = typed_info(&document);
    let layers = info["layers"].as_array().expect("layers");
    for (name, _, color, density) in lab_cases() {
        let index = layers
            .iter()
            .position(|l| l["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("layer {name}"));
        assert_eq!(
            document.adjustment_description(index),
            Some(typed_meta("photo_filter", &lab_params(color, density)).as_str()),
            "{name}"
        );
    }
}

#[test]
fn a_lab_photo_filter_colour_converts_into_a_display_p3_document() {
    let dir = TempDir::new().unwrap();
    let (name, body, hex, density) = lab_cases().remove(0);
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &BASE, false);
    base.name = "Base";
    let p3 = IccProfile::display_p3();
    let bytes = with_resources(
        fixture8::fixture(&[base, empty8(name, b"phfl", &body)], CANVAS, false),
        &resource(1039, p3.bytes()),
    );
    let path = write(&dir, "lab-p3.psd", &bytes);
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let doc = open_psd(&path, &report);
    let layer = doc.layers.iter().find(|l| l.name == name).expect("layer");
    // The sRGB colour the Lab components decode to, then the document's own
    // conversion of it — never the sRGB numbers on a P3 document.
    let expected = converted([0xec, 0x8a, 0x00], &IccProfile::srgb(), &p3);
    let expected = format!("#{:02x}{:02x}{:02x}", expected[0], expected[1], expected[2]);
    assert_ne!(expected, hex, "the P3 conversion must move the colour");
    assert_eq!(
        layer.meta.as_deref(),
        Some(meta("photo_filter", &lab_params(&expected, density)).as_str())
    );
}

/// One pixel-less record carrying `key`, with the 20-byte EMPTY-rectangle
/// mask body, bare -2 channel and 0x18 flags Photoshop writes on every
/// adjustment layer.
fn photoshop_shaped8(
    name: &'static str,
    key: &[u8; 4],
    body: &[u8],
    outside: u8,
) -> fixture8::Record {
    let mut record = empty8(name, key, body);
    record.flags = 0x18;
    record.mask_body = [[0u8; 16].to_vec(), vec![outside, 0, 0, 0]].concat();
    record.channels.push((-2, vec![0, 0]));
    record
}

#[test]
fn photoshops_empty_mask_rectangle_carries_the_record_with_no_mask_and_no_marker() {
    let dir = TempDir::new().unwrap();
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &BASE, false);
    base.name = "Base";
    let path = write(
        &dir,
        "empty-mask8.psd",
        &fixture8::fixture(
            &[
                base.clone(),
                photoshop_shaped8("Keeps", b"nvrt", &[], 255),
                photoshop_shaped8("Hides", b"nvrt", &[], 0),
            ],
            CANVAS,
            false,
        ),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let losses = strings(&report, "verified_losses");
    assert!(
        !losses.iter().any(|l| l.contains("omits it")),
        "both records carry: {losses:?}"
    );
    assert!(
        !losses.iter().any(|l| l.contains("pixels irrelevant")),
        "a pixel-less record declares no alternative appearance: {losses:?}"
    );
    assert!(
        !losses.iter().any(|l| l.contains("(“Keeps”)")),
        "outside 255 is no mask at all: {losses:?}"
    );
    assert!(
        losses.iter().any(|l| l.contains("(“Hides”)")
            && l.contains(
                "empty mask rectangle with outside default 0 would hide the whole layer"
            )),
        "outside 0 is a reported drop: {losses:?}"
    );
    let doc = open_psd(&path, &report);
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Base", "Keeps", "Hides"]);
    for layer in &doc.layers[1..] {
        assert_eq!(
            layer.meta.as_deref(),
            Some(meta("invert", &json!({})).as_str())
        );
        assert!(layer.mask.is_none(), "{}", layer.name);
    }

    // The 16-bit twin, at both outside defaults.
    let mut keeps = empty16("Keeps", b"nvrt", &[]);
    keeps.flags = 0x18;
    keeps.mask_body = [[0u8; 16].to_vec(), vec![255, 0, 0, 0]].concat();
    keeps.channels.push((-2, vec![0, 0]));
    let mut hides = empty16("Hides", b"nvrt", &[]);
    hides.flags = 0x18;
    hides.mask_body = [[0u8; 16].to_vec(), vec![0, 0, 0, 0]].concat();
    hides.channels.push((-2, vec![0, 0]));
    let bytes = fixture16::fixture(
        (CANVAS[0], CANVAS[1]),
        &[
            fixture16::Record::raster("Base", [0, 0, 2, 2], planes16(&BASE16)),
            keeps,
            hides,
        ],
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &BASE16,
        true,
        false,
    );
    let path = write(&dir, "empty-mask16.psd", &bytes);
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let losses = strings(&report, "verified_losses");
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("pixels irrelevant") || l.contains("omits it")),
        "{report}"
    );
    assert!(
        !losses.iter().any(|l| l.contains("(“Keeps”)")),
        "outside 255 is no mask at all: {losses:?}"
    );
    assert!(
        losses.iter().any(|l| l.contains("(“Hides”)")
            && l.contains(
                "empty mask rectangle with outside default 0 would hide the whole layer"
            )),
        "outside 0 is the same reported drop at 16 bits: {losses:?}"
    );
    let document = open_typed(&path, &report);
    let info = typed_info(&document);
    for name in ["Keeps", "Hides"] {
        let index = info["layers"]
            .as_array()
            .unwrap()
            .iter()
            .position(|l| l["name"].as_str() == Some(name))
            .expect("the record carries");
        assert_eq!(
            document.adjustment_description(index),
            Some(typed_meta("invert", &json!({})).as_str()),
            "{name}"
        );
        assert!(document.layer(index).unwrap().mask().is_none(), "{name}");
    }
}

#[test]
fn layer_information_padding_is_admitted_up_to_three_bytes() {
    let dir = TempDir::new().unwrap();
    // Two zero bytes is exactly what Photoshop's multiple-of-4 rounding
    // leaves on this run's fixture; four is a whole unclaimed record start.
    let path = write(&dir, "pad2.psd", &with_layer_info_padding(carried8(), 2));
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert_eq!(open_psd(&path, &report).layers.len(), 9);

    let path = write(&dir, "pad4.psd", &with_layer_info_padding(carried8(), 4));
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(false), "{report}");
    assert!(
        report["layered_refusal"]
            .as_str()
            .unwrap_or_default()
            .contains("unclaimed record/channel bytes"),
        "{report}"
    );
}
