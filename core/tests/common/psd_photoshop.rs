//! The committed Adobe Photoshop 2025 (26.10.0) fixtures, through the C API.
//!
//! `core/tests/fixtures/psd_photoshop/<family>/` holds files Photoshop itself
//! authored (`scripts/photoshop-probe/`, whose README carries the provenance
//! and the pin). Every other PSD fixture in this crate is either hand-built
//! from Adobe's tables or written by Doctored, so these are the ones whose
//! framing no Doctored code chose — which is why two test files read them and
//! share the plumbing here.
#![allow(dead_code)]

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
use serde_json::Value;

use super::{cpath, take_err_string};

/// `dr_doc_open_psd`'s two import choices.
pub const LAYERS: i32 = 0;
pub const COMPOSITE: i32 = 1;

/// One file of one committed Photoshop-authored family.
pub fn fixture(family: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop")
        .join(family)
        .join(name)
}

pub fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    assert!(error.is_null());
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

pub fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array"))
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

pub fn open_psd(path: &Path, report: &Value, mode: i32) -> Box<DrDocument> {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), mode, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    unsafe { Box::from_raw(raw) }
}

/// Every `8BIM{key}` block body in a written file, found by scanning for the
/// signature and key and reading the declared length (a copy of
/// `psd_channel_mixer.rs`'s, which keeps its own).
pub fn blocks_in(bytes: &[u8], key: &[u8; 4]) -> Vec<Vec<u8>> {
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

/// The `SoCo` body Photoshop wrote for `depth16/source.psd`'s “Fill” layer:
/// `u32 16`, class `null`, `Clr ` an `RGBC` of 40/40/200. The four
/// hand-built fixtures that once pinned `SoCo` as omitted carry it.
pub fn soco_body() -> Vec<u8> {
    let bytes = std::fs::read(fixture("depth16", "source.psd")).expect("depth16/source.psd");
    blocks_in(&bytes, b"SoCo")
        .into_iter()
        .next()
        .expect("depth16/source.psd's SoCo block")
}

/// An `RGBC` descriptor's `Rd  `, `Grn ` and `Bl  ` doubles, read straight
/// off `body`'s bytes (each key is a 0 length word, the key and `doub`, then
/// the big-endian f64), so a test compares against Photoshop's own numbers
/// without the core's descriptor reader.
pub fn rgbc_numbers(body: &[u8]) -> [f64; 3] {
    [b"Rd  doub", b"Grn doub", b"Bl  doub"].map(|key| {
        let at = body
            .windows(8)
            .position(|w| w == key)
            .unwrap_or_else(|| panic!("{} in the RGBC", String::from_utf8_lossy(key)));
        f64::from_be_bytes(body[at + 8..at + 16].try_into().unwrap())
    })
}

/// A carried `SoCo`'s description: a version-1 solid fill over
/// `canvas` whose colour is `numbers` (an `RGBC`, 0…255) / 255 within 1e-6 —
/// an sRGB file's numbers carried unchanged — at alpha 1, with no wide
/// colour.
pub fn assert_solid_description(
    description: &str,
    numbers: [f64; 3],
    canvas: (u32, u32),
    what: &str,
) {
    let d: Value = serde_json::from_str(description).expect("description JSON");
    assert_eq!(d["type"], "fill", "{what}");
    assert_eq!(d["version"], 1, "{what}");
    assert_eq!(d["fill"], "solid", "{what}");
    assert_eq!(
        d["canvas"],
        serde_json::json!([canvas.0, canvas.1]),
        "{what}"
    );
    assert!(d.get("linear").is_none(), "{what}: no wide colour");
    assert!(d.get("ink").is_none(), "{what}: no ink");
    let color: Vec<f64> = d["color"]
        .as_array()
        .expect("color")
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(color.len(), 4, "{what}");
    for (c, n) in color.iter().zip(numbers) {
        assert!(
            (c - n / 255.0).abs() <= 1e-6,
            "{what}: {color:?} vs {numbers:?}"
        );
    }
    assert_eq!(color[3], 1.0, "{what}: alpha");
}
