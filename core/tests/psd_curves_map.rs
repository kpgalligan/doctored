//! The committed Photoshop 2025 (26.10.0) per-channel MAP curve sample,
//! through the C API.
//!
//! `core/tests/fixtures/psd_photoshop/curves_map/` is a document Photoshop
//! itself authored (`scripts/verify_curves_map_photoshop.py` over
//! `scripts/photoshop-probe/`): the 64 x 64 base picture under ONE Curves
//! layer, `map_channels`, drawn in the mapped (`Mpng`, pencil) form — a
//! composite square-root map, a red map `255 - i` and a green map with a
//! plateau at 200 over 100…115 — saved at 8 bit (`source.psd`) and 16 bit
//! (`source16.psd`), with Photoshop's render at both depths and the readout
//! of the `curv` body (`blocks.json`).
//!
//! Measured on 2026-10-03, identical at both depths: the body is 1560 bytes —
//! `is_map` 1, version 1, bitmask 7, the three maps in bit order, then the
//! `Crv ` tail (u16 3, u32 3, then u16 bit index + the same map, three
//! times) and one pad byte — exactly the form fixed before the take.
//! Photoshop applies the channel map FIRST: `channel_then_composite` is
//! within 0 codes at 8 bit and 0.98 at 16 (the other order misses by 106);
//! the tolerance was 3 codes, fixed before the take.
//!
//! Here: both depths carry exactly the authored maps, render within
//! [`TOLERANCE`] codes of Photoshop's own render at 8 bits and exactly
//! Photoshop's 16-bit samples at 16 (the 15-bit stage tables), and
//! export back to a `curv` body byte-identical to Photoshop's.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_doc::dr_doc_flattened;
use doctored_core::ffi_psd::{
    dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::Value;

use crate::common;
use common::psd_photoshop::{blocks_in, import_report, open_psd, LAYERS};
use common::{cpath, pixels, take_err_string};

const FAMILY: &str = "curves_map";

/// The take's render-parity tolerance in 8-bit codes, fixed before the take
/// and never widened.
const TOLERANCE: f64 = 3.0;

fn fixture(name: &str) -> PathBuf {
    let path = common::psd_photoshop::fixture(FAMILY, name);
    assert!(path.is_file(), "missing fixture {}", path.display());
    path
}

/// The authored maps, from `description.json`: the composite `mapping` and
/// one `mapping` per channel entry.
fn authored_params() -> Value {
    let description: Value = serde_json::from_slice(
        &std::fs::read(fixture("description.json")).expect("description.json"),
    )
    .unwrap();
    let params = &description["layers"][0]["params"];
    assert_eq!(description["layers"][0]["name"], "map_channels");
    let mut out = serde_json::Map::new();
    out.insert("rgb_map".into(), params["mapping"].clone());
    for entry in params["channels"].as_array().expect("channels") {
        let key = match entry["channel"].as_str() {
            Some("red") => "r_map",
            Some("green") => "g_map",
            Some("blue") => "b_map",
            other => panic!("channel {other:?}"),
        };
        out.insert(key.into(), entry["mapping"].clone());
    }
    assert_eq!(out.len(), 3, "composite, red and green");
    Value::Object(out)
}

/// Photoshop's own `curv` body at `depth` (`blocks.json`).
fn photoshop_body(depth: &str) -> Vec<u8> {
    let blocks: Value =
        serde_json::from_slice(&std::fs::read(fixture("blocks.json")).expect("blocks.json"))
            .unwrap();
    let row = blocks[depth]["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "map_channels")
        .unwrap_or_else(|| panic!("map_channels at {depth}"));
    let hex = row["hex"].as_str().expect("hex");
    let body: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex digit"))
        .collect();
    assert_eq!(body.len(), 1560, "the measured length");
    body
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

fn open_typed(path: &Path) -> Owned {
    let report = import_report(path);
    assert_eq!(report["import_precision"], Value::from("rgba16"));
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

fn params_of(meta: &str) -> Value {
    let value: Value = serde_json::from_str(meta).expect("meta JSON");
    assert_eq!(value["type"], "adjust");
    assert_eq!(value["op"], "curves");
    value["params"].clone()
}

/// The worst RGB difference in 8-bit codes between `ours` (RGBA) and
/// Photoshop's RGB render, after `scale`.
fn worst(ours: &[u32], theirs: &[u32], scale: f64) -> f64 {
    assert_eq!(ours.len() / 4, theirs.len() / 3, "pixel counts differ");
    ours.chunks_exact(4)
        .zip(theirs.chunks_exact(3))
        .flat_map(|(a, b)| (0..3).map(move |c| f64::from(a[c].abs_diff(b[c])) / scale))
        .fold(0.0, f64::max)
}

#[test]
fn both_depths_carry_exactly_the_authored_maps() {
    let path = fixture("source.psd");
    let report = import_report(&path);
    assert_eq!(report["import_precision"], Value::from("rgba8"));
    let doc = open_psd(&path, &report, LAYERS);
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Base", "map_channels"]);
    let meta = doc.layers[1].meta.as_deref().expect("a curves meta");
    assert_eq!(params_of(meta), authored_params());

    let layered = open_typed(&fixture("source16.psd"));
    assert_eq!(layered.layer_count(), 2);
    let description = layered.adjustment_description(1).expect("a description");
    let value: Value = serde_json::from_str(description).unwrap();
    assert_eq!(value["evaluation"], "precision_v1");
    assert_eq!(params_of(description), authored_params());
}

#[test]
fn the_8_bit_render_is_within_tolerance_of_photoshop() {
    let path = fixture("source.psd");
    let report = import_report(&path);
    let doc = open_psd(&path, &report, LAYERS);
    let mut error: *mut c_char = ptr::null_mut();
    let flat = unsafe { dr_doc_flattened(&*doc, 0, std::ptr::null(), &mut error) };
    assert!(!flat.is_null(), "{}", take_err_string(error));
    let ours: Vec<u32> = pixels(flat).into_iter().map(u32::from).collect();
    unsafe { dr_image_free(flat) };
    let theirs: Vec<u32> = image::open(fixture("map_channels_8.png"))
        .expect("map_channels_8.png")
        .to_rgb8()
        .into_raw()
        .into_iter()
        .map(u32::from)
        .collect();
    let worst = worst(&ours, &theirs, 1.0);
    assert!(worst <= TOLERANCE, "8 bits: worst difference {worst} codes");
}

/// The 16-bit render reads the 15-bit stage tables (`adjust_curves`),
/// so every 16-bit sample equals Photoshop's render of the same file.
#[test]
fn the_16_bit_render_matches_photoshops_render_sample_for_sample() {
    let layered = open_typed(&fixture("source16.psd"));
    let projection = layered
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render");
    let ours: Vec<u32> = projection
        .to_rgba16()
        .expect("u16 samples")
        .into_iter()
        .flat_map(|p| p.map(u32::from))
        .collect();
    let theirs: Vec<u32> = image::open(fixture("map_channels_16.png"))
        .expect("map_channels_16.png")
        .to_rgb16()
        .into_raw()
        .into_iter()
        .map(u32::from)
        .collect();
    assert_eq!(ours.len() / 4, theirs.len() / 3, "pixel counts differ");
    let mut compared = 0;
    for (i, (a, b)) in ours.chunks_exact(4).zip(theirs.chunks_exact(3)).enumerate() {
        assert_eq!(&a[..3], b, "pixel {i}: ours vs Photoshop's 16-bit samples");
        compared += 3;
    }
    assert_eq!(compared, 64 * 64 * 3, "every 16-bit sample compared");
}

#[test]
fn the_export_writes_photoshops_own_curv_body_at_both_depths() {
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
            &mut error,
        )
    };
    assert!(ok, "save: {}", take_err_string(error));
    let bodies = blocks_in(&std::fs::read(&out).unwrap(), b"curv");
    assert_eq!(bodies, vec![photoshop_body("8")], "8-bit export");

    let layered = open_typed(&fixture("source16.psd"));
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
            &mut error,
        )
    };
    assert!(ok, "save16: {}", take_err_string(error));
    let bodies = blocks_in(&std::fs::read(&out).unwrap(), b"curv");
    assert_eq!(bodies, vec![photoshop_body("16")], "16-bit export");
}
