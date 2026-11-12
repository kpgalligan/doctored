//! The committed Photoshop 2025 (26.10.0) per-channel Levels / Curves sample,
//! through the C API.
//!
//! `core/tests/fixtures/psd_photoshop/adjustments_channels/` is a document
//! Photoshop itself authored (`scripts/verify_phase10_photoshop_channels.py`
//! over `scripts/photoshop-probe/`): the 64 x 64 base picture under five
//! adjustment layers, saved at 8 bit (`source.psd`) and 16 bit
//! (`source16.psd`), with Photoshop's render of each case at both depths and
//! the readout of every `levl` / `curv` body (`blocks.json`).
//!
//! Measured on 2026-10-02, identical at both depths:
//!
//! - `levl` is 632 bytes: version 2, then 29 records of `(in_floor, in_ceil,
//!   out_floor, out_ceil, gamma×100)` — record 0 the composite, records 1, 2,
//!   3 red, green, blue (Adobe's order) — then a 340-byte tail: `Lvls`,
//!   u16 version 3, u16 total record count 62, the 33 further records (all
//!   identity) and 2 pad bytes. Per-channel Levels land in records 1..=3;
//!   `levl_output` writes `(0, 255, 20, 235, 100)` in record 0. All three
//!   CARRY (`adjust_levels`): their metas are built here
//!   from `blocks.json`'s record codes, and each renders within
//!   [`TOLERANCE`] codes of Photoshop's own render at both depths.
//! - A `Mpng` curve writes `is_map` 1, version 1, bitmask 1 and the 256 map
//!   bytes exactly as authored, then a `Crv ` tail (version 3, one map for
//!   channel 0) and a pad byte (532 bytes). It CARRIES as
//!   `{"rgb_map": mapping}` (`adjust_curves`' map form), renders within
//!   [`TOLERANCE`] codes of Photoshop's own render at both depths, and
//!   exports back to a `curv` body byte-identical to Photoshop's.
//! - A composite + red point curve writes `is_map` 0, version 1, bitmask 3
//!   (composite and red only: no identity green or blue entry), the points as
//!   (output, input), then a `Crv ` tail at version 4 carrying the same two
//!   curves by channel index (64 bytes). It carries.
//! - Photoshop applies the per-channel stage FIRST and the composite second:
//!   `levl_both` matches `channel_then_composite` within 2.53 codes at 8 bit
//!   and 1.34 at 16 (`composite_then_channel` misses by 43.5), and
//!   `curv_channels` within 1.19 / 0.05 (the other order misses by 83.7 /
//!   82.6); the tolerance was 3 codes, fixed before the take.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_doc::{dr_doc_flattened, dr_doc_free, dr_doc_with_layer_visible};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

use crate::common;
use common::psd_photoshop::{blocks_in, import_report, open_psd, strings, LAYERS};
use common::{cpath, pixels, take_err_string};

const FAMILY: &str = "adjustments_channels";

/// A2's render-parity tolerance in 8-bit codes, fixed before the first run
/// of the gate below and never widened.
const TOLERANCE: u8 = 3; // A2's, fixed before the first run

/// The records this file declines: (layer, kind report name, key, reason).
/// None: `curv_map` is carried.
const DECLINED: [(&str, &str, &str, &str); 0] = [];

/// The three Levels cases, in stack order.
const LEVELS: [&str; 3] = ["levl_channels", "levl_both", "levl_output"];

/// The render-gated cases, in stack order: the Levels three, then the map
/// curve (stack index 4).
const GATED: [&str; 4] = ["levl_channels", "levl_both", "levl_output", "curv_map"];

/// The carried stack, bottom first: every record carries.
const CARRIED: [&str; 6] = [
    "Base",
    "levl_channels",
    "levl_both",
    "levl_output",
    "curv_map",
    "curv_channels",
];

fn fixture(name: &str) -> PathBuf {
    let path = common::psd_photoshop::fixture(FAMILY, name);
    assert!(path.is_file(), "missing fixture {}", path.display());
    path
}

/// The measured `curv_channels` params: bitmask 3, so composite and red only.
fn curv_channels_params() -> Value {
    json!({"rgb": [[0, 0], [160, 255]], "r": [[90, 0], [255, 255]]})
}

fn meta() -> String {
    json!({"type": "adjust", "op": "curves", "params": curv_channels_params()}).to_string()
}

fn typed_meta() -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": "curves",
           "params": curv_channels_params()})
    .to_string()
}

/// `curv_map`'s params: the composite map exactly as authored
/// (`description.json`'s `mapping`), bitmask 1 so no channel maps.
fn curv_map_params() -> Value {
    let description: Value = serde_json::from_slice(
        &std::fs::read(fixture("description.json")).expect("description.json"),
    )
    .unwrap();
    let layer = description["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["name"] == "curv_map")
        .expect("curv_map");
    json!({"rgb_map": layer["params"]["mapping"]})
}

fn curv_map_meta() -> String {
    json!({"type": "adjust", "op": "curves", "params": curv_map_params()}).to_string()
}

fn curv_map_typed_meta() -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": "curves",
           "params": curv_map_params()})
    .to_string()
}

/// Photoshop's own `curv_map` body at `depth` (`blocks.json`).
fn photoshop_curv_map_body(depth: &str) -> Vec<u8> {
    let blocks: Value =
        serde_json::from_slice(&std::fs::read(fixture("blocks.json")).expect("blocks.json"))
            .unwrap();
    let row = blocks[depth]["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "curv_map")
        .unwrap_or_else(|| panic!("curv_map at {depth}"));
    let hex = row["hex"].as_str().expect("hex");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex digit"))
        .collect()
}

/// One `levl` record `(in_floor, in_ceil, out_floor, out_ceil, gamma×100)`
/// as a stage's keys: `code/255` and `g/100`, the output pair only when it
/// is not (0, 255).
fn stage(record: &Value) -> serde_json::Map<String, Value> {
    let code = |i: usize| record[i].as_f64().expect("a record code");
    let mut out = serde_json::Map::new();
    out.insert("black".into(), json!(code(0) / 255.0));
    out.insert("white".into(), json!(code(1) / 255.0));
    out.insert("gamma".into(), json!(code(4) / 100.0));
    if (code(2), code(3)) != (0.0, 255.0) {
        out.insert("output_black".into(), json!(code(2) / 255.0));
        out.insert("output_white".into(), json!(code(3) / 255.0));
    }
    out
}

/// `case`'s levels params, built from the record codes Photoshop wrote
/// (`blocks.json` at `depth`): the master from record 0, and `r` / `g` / `b`
/// from records 1..=3 wherever the record is not the identity.
fn levels_params(case: &str, depth: &str) -> Value {
    let blocks: Value =
        serde_json::from_slice(&std::fs::read(fixture("blocks.json")).expect("blocks.json"))
            .unwrap();
    let row = blocks[depth]["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == case)
        .unwrap_or_else(|| panic!("{case} at {depth}"));
    let records = row["records"].as_array().expect("records");
    let mut params = stage(&records[0]);
    for (name, record) in ["r", "g", "b"].into_iter().zip(&records[1..=3]) {
        if *record != json!([0, 255, 0, 255, 100]) {
            params.insert(name.into(), Value::Object(stage(record)));
        }
    }
    Value::Object(params)
}

fn levels_meta(case: &str, depth: &str) -> String {
    json!({"type": "adjust", "op": "levels", "params": levels_params(case, depth)}).to_string()
}

fn levels_typed_meta(case: &str, depth: &str) -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": "levels",
           "params": levels_params(case, depth)})
    .to_string()
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

fn open_typed(path: &Path, report: &Value, mode: i32) -> Owned {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
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

/// No decline sentence, and the five carries.
fn assert_declines_and_carry(report: &Value) {
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let losses = strings(report, "verified_losses");
    for (layer, kind, key, reason) in DECLINED {
        let sentence = format!(
            "(“{layer}”) is a Photoshop {kind} adjustment layer ({key}) with no pixels whose {reason};"
        );
        assert!(
            losses.iter().any(|l| l.contains(&sentence)),
            "the {layer} decline: {losses:?}"
        );
    }
    assert_eq!(
        losses
            .iter()
            .filter(|l| l.contains("adjustment layer (") && l.contains("with no pixels whose"))
            .count(),
        DECLINED.len(),
        "{losses:?}"
    );
    let observations = strings(report, "observations");
    assert!(
        observations.iter().any(|o| o.contains(
            "(“curv_channels”) is a Photoshop Curves adjustment layer (curv); layered import carries it as a Doctored curves adjustment layer"
        )),
        "the curv_channels carry: {observations:?}"
    );
    assert!(
        observations.iter().any(|o| o.contains(
            "(“curv_map”) is a Photoshop Curves adjustment layer (curv); layered import carries it as a Doctored curves adjustment layer"
        )),
        "the curv_map carry: {observations:?}"
    );
    for case in LEVELS {
        assert!(
            observations.iter().any(|o| o.contains(&format!(
                "(“{case}”) is a Photoshop Levels adjustment layer (levl); layered import carries it as a Doctored levels adjustment layer"
            ))),
            "the {case} carry: {observations:?}"
        );
    }
}

#[test]
fn the_eight_bit_file_carries_every_record() {
    let path = fixture("source.psd");
    let report = import_report(&path);
    assert_eq!(report["import_precision"], Value::from("rgba8"));
    assert_declines_and_carry(&report);

    let doc = open_psd(&path, &report, LAYERS);
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, CARRIED);
    for (index, case) in LEVELS.into_iter().enumerate() {
        assert_eq!(
            doc.layers[index + 1].meta.as_deref(),
            Some(levels_meta(case, "8").as_str()),
            "{case}"
        );
    }
    assert_eq!(
        doc.layers[4].meta.as_deref(),
        Some(curv_map_meta().as_str())
    );
    assert_eq!(doc.layers[5].meta.as_deref(), Some(meta().as_str()));
}

#[test]
fn the_sixteen_bit_file_carries_every_record() {
    let path = fixture("source16.psd");
    let report = import_report(&path);
    assert_eq!(report["import_precision"], Value::from("rgba16"));
    assert_declines_and_carry(&report);

    // The byte entry point refuses depth 16 and names the typed path.
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_doc_open_psd(
            cpath(&path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(raw.is_null(), "the byte entry point must refuse depth 16");
    assert!(take_err_string(error).contains("16-bit PSD opens only as a 16-bit document"));

    let layered = open_typed(&path, &report, LAYERS);
    let names: Vec<String> = (0..layered.layer_count())
        .map(|index| layered.layer(index).expect("layer").name().to_string())
        .collect();
    assert_eq!(names, CARRIED);
    for (index, case) in LEVELS.into_iter().enumerate() {
        assert_eq!(
            layered.adjustment_description(index + 1),
            Some(levels_typed_meta(case, "16").as_str()),
            "{case}"
        );
    }
    assert_eq!(
        layered.adjustment_description(4),
        Some(curv_map_typed_meta().as_str())
    );
    assert_eq!(
        layered.adjustment_description(5),
        Some(typed_meta().as_str())
    );
}

// ------------------------------------------------- the render-parity gate --

/// The worst RGB difference in 8-bit codes between `ours` (`stride` samples
/// per pixel) and Photoshop's RGB render, after `scale`.
fn worst(ours: &[u32], stride: usize, theirs: &[u32], scale: f64) -> f64 {
    assert_eq!(ours.len() / stride, theirs.len() / 3, "pixel counts differ");
    ours.chunks_exact(stride)
        .zip(theirs.chunks_exact(3))
        .flat_map(|(a, b)| (0..3).map(move |c| f64::from(a[c].abs_diff(b[c])) / scale))
        .fold(0.0, f64::max)
}

/// Each Levels case and the map curve shown ALONE over Base, flattened at 8
/// bits, against Photoshop's own render of the same case (`<case>_8.png`).
#[test]
fn each_carried_levels_and_map_case_renders_within_tolerance_of_photoshop_at_8_bits() {
    let path = fixture("source.psd");
    let report = import_report(&path);
    let doc = open_psd(&path, &report, LAYERS);
    for (index, case) in GATED.into_iter().enumerate() {
        let keep = index + 1;
        let mut shown: *mut DrDocument = Box::into_raw(Box::new((*doc).clone()));
        for other in 1..CARRIED.len() {
            if other == keep {
                continue;
            }
            let next = unsafe { dr_doc_with_layer_visible(shown, other, false) };
            assert!(!next.is_null(), "hide {other}");
            unsafe { dr_doc_free(shown) };
            shown = next;
        }
        let mut error: *mut c_char = ptr::null_mut();
        let flat = unsafe { dr_doc_flattened(shown, 0, std::ptr::null(), &mut error) };
        assert!(!flat.is_null(), "{case}: {}", take_err_string(error));
        let ours: Vec<u32> = pixels(flat).into_iter().map(u32::from).collect();
        unsafe {
            dr_image_free(flat);
            dr_doc_free(shown);
        }
        let render = format!("{case}_8.png");
        let theirs: Vec<u32> = image::open(fixture(&render))
            .expect(&render)
            .to_rgb8()
            .into_raw()
            .into_iter()
            .map(u32::from)
            .collect();
        let worst = worst(&ours, 4, &theirs, 1.0);
        assert!(
            worst <= f64::from(TOLERANCE),
            "{case} at 8 bits: worst difference {worst} codes"
        );
    }
}

/// The same gate at 16 bits: the case shown alone through the typed edit,
/// rendered, against `<case>_16.png`, the difference measured in 8-bit codes
/// (`|a − b| / 257`).
#[test]
fn each_carried_levels_and_map_case_renders_within_tolerance_of_photoshop_at_16_bits() {
    let path = fixture("source16.psd");
    let report = import_report(&path);
    let layered = open_typed(&path, &report, LAYERS);
    for (index, case) in GATED.into_iter().enumerate() {
        let keep = index + 1;
        let mut shown = Owned(Box::into_raw(Box::new((*layered).clone())));
        for other in 1..CARRIED.len() {
            if other == keep {
                continue;
            }
            let request = CString::new(
                json!({"op": "layer_visible", "layer": other, "visible": false}).to_string(),
            )
            .unwrap();
            let mut error = ptr::null_mut();
            let next = unsafe {
                dr_precision_doc_edit(
                    shown.0,
                    request.as_ptr(),
                    ptr::null(),
                    0,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            assert!(!next.is_null(), "hide {other}: {}", take_err_string(error));
            shown = Owned(next);
        }
        let projection = shown
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .expect("render");
        let ours: Vec<u32> = projection
            .to_rgba16()
            .expect("u16 samples")
            .into_iter()
            .flat_map(|p| p.map(u32::from))
            .collect();
        let render = format!("{case}_16.png");
        let theirs: Vec<u32> = image::open(fixture(&render))
            .expect(&render)
            .to_rgb16()
            .into_raw()
            .into_iter()
            .map(u32::from)
            .collect();
        let worst = worst(&ours, 4, &theirs, 257.0);
        assert!(
            worst <= f64::from(TOLERANCE),
            "{case} at 16 bits: worst difference {worst} codes"
        );
    }
}

// ------------------------------------------------------ the export bytes --

/// The carried map curve exports to a `curv` body byte-identical to the one
/// Photoshop wrote, at both depths (`curve_map_body`).
#[test]
fn the_map_curve_exports_photoshops_own_curv_body_at_both_depths() {
    let dir = tempfile::tempdir().unwrap();

    let path = fixture("source.psd");
    let report = import_report(&path);
    let doc = open_psd(&path, &report, LAYERS);
    let out = dir.path().join("map8.psd");
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            &*doc,
            ptr::null(),
            cpath(&out).as_ptr(),
            0,
            LAYERS,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "save: {}", take_err_string(error));
    let bodies = blocks_in(&std::fs::read(&out).unwrap(), b"curv");
    assert!(
        bodies.contains(&photoshop_curv_map_body("8")),
        "8-bit export: {} curv bodies, none Photoshop's",
        bodies.len()
    );

    let path = fixture("source16.psd");
    let report = import_report(&path);
    let layered = open_typed(&path, &report, LAYERS);
    let projection = layered
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render");
    let out = dir.path().join("map16.psd");
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            &*layered,
            &projection,
            cpath(&out).as_ptr(),
            0,
            LAYERS,
            REFERENCE_WORKING_BYTES,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "save16: {}", take_err_string(error));
    let bodies = blocks_in(&std::fs::read(&out).unwrap(), b"curv");
    assert!(
        bodies.contains(&photoshop_curv_map_body("16")),
        "16-bit export: {} curv bodies, none Photoshop's",
        bodies.len()
    );
}
