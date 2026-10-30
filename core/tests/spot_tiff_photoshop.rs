//! #1 spot channels 1c-2a against Photoshop's own flat TIFFs
//! (`tests/fixtures/photoshop_spot_tiff`, authored by
//! `scripts/photoshop-probe/spot_tiff_take.py`'s take2 on Photoshop
//! 26.10.0: RGB at 8 and 16 bits, one with a transparency, and CMYK at 8
//! and 16 bits, one or two spot channels each). SKIP when the folder is
//! absent.
//!
//! Bounds, declared before measuring: each file's length, ExtraSamples,
//! channel resources (1045, 1077, 1053 bodies) and stored spot samples hold
//! `photoshop.json` through `spot_tiff`'s own reader (an independent parser
//! from the runner's); the stored samples hold the SCENE the take built
//! under Photoshop's polarity `max − coverage` (the ramp spot's coverage
//! v(x) = round(255 x / 31), exact at 8 bits and within 4 codes of 257 v at
//! 16; the block spot full inside x in [8, 24), y in [2, 6), empty
//! outside). Doctored's opens (byte; typed RGB 16 and CMYK) keep every spot
//! channel's name, Lab (the stored words / 100, within 0.5 of Photoshop's
//! own readback), Solidity exactly and coverage exactly (`max − stored`, a
//! 16-bit file's narrowed by round(v / 257) in the byte document), and the
//! report names them. Doctored's re-save writes Photoshop's 1045, 1077 and
//! 1053 bodies byte for byte, ExtraSamples 0 on every spot sample and the
//! same stored samples; its transparency is ExtraSamples 2 (straight), where
//! Photoshop writes 1.
//!
//! The layered TIFFs (1c-2b: `rgb8_l.tif`, `cmyk16_l.tif`) open layered
//! (byte; typed CMYK) with every spot channel's name, Lab (the stored words
//! / 100, within 0.5 of the readback), Solidity exactly and coverage
//! `max − stored_rows` exactly, and the report names them.

use std::path::{Path, PathBuf};

use doctored_core::doc_export::ExportRequest;
use serde_json::Value;
use tempfile::TempDir;

use crate::psb_fixture::{import_report, open16, open8, strings, LAYERS};
use crate::spot_tiff::{
    byte_spots, import_warnings, open_byte, open_typed, read_tiff, resources, save_byte,
    save_cmyk_tiff, typed_spots, Tiff,
};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const TIFF: i32 = 2;
const FLAT: [&str; 5] = [
    "rgb8.tif",
    "rgb8_t.tif",
    "rgb16.tif",
    "cmyk8.tif",
    "cmyk16.tif",
];

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/photoshop_spot_tiff")
}

/// `photoshop.json`, or `None` (SKIP) when the folder is absent.
fn manifest() -> Option<Value> {
    let path = folder().join("photoshop.json");
    if !path.is_file() {
        eprintln!("SKIP: {} is absent", path.display());
        return None;
    }
    Some(serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap())
}

fn entry<'a>(manifest: &'a Value, name: &str) -> &'a Value {
    manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == name)
        .unwrap()
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn numbers(value: &Value) -> Vec<u64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect()
}

/// The scene's coverage of `spot` at pixel `p` in the file's codes.
fn scene(spot: &Value, p: usize, bits: u64) -> (u16, u16) {
    let (x, y) = ((p % 32) as f64, p / 32);
    let v = (255.0 * x / 31.0).round() as u16;
    if spot["source"] == "red" {
        return if bits == 16 { (v * 257, 4) } else { (v, 0) };
    }
    let inside = (8.0..24.0).contains(&x) && (2..6).contains(&y);
    let full = if bits == 16 { 65535 } else { 255 };
    (if inside { full } else { 0 }, 0)
}

/// The file's spot samples (the trailing extra samples), stored.
fn stored_planes(tiff: &Tiff, spots: usize) -> Vec<Vec<u16>> {
    tiff.planes[tiff.planes.len() - spots..].to_vec()
}

fn assert_resources_equal(ours: &Tiff, theirs: &Value, what: &str) {
    let found = resources(ours.bytes.get(&34377).expect("tag 34377"));
    for id in ["1045", "1077", "1053"] {
        let want = hex(theirs["layout"]["channel_resources"][id].as_str().unwrap());
        assert_eq!(
            found.get(&id.parse::<u16>().unwrap()),
            Some(&want),
            "{what}: resource {id}"
        );
    }
}

#[test]
fn photoshops_flat_tiffs_hold_the_take_and_open_with_their_spot_channels() {
    let Some(manifest) = manifest() else {
        return;
    };
    assert_eq!(manifest["build"]["polarity"]["rgb"], "H1");
    assert_eq!(manifest["build"]["polarity"]["cmyk"], "H1");
    assert_eq!(manifest["build"]["lead"], "L1");
    for name in FLAT {
        let file = entry(&manifest, name);
        let path = folder().join(name);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            bytes.len() as u64,
            file["bytes"].as_u64().unwrap(),
            "{name}"
        );
        let tiff = read_tiff(&bytes);
        let bits = file["depth"].as_u64().unwrap();
        let max: u16 = if bits == 16 { 65535 } else { 255 };
        let spots = file["spots"].as_array().unwrap();
        assert_eq!(
            tiff.tag(338)
                .iter()
                .map(|&v| u64::from(v))
                .collect::<Vec<_>>(),
            numbers(&file["layout"]["extra_samples"]),
            "{name}"
        );
        let stored = stored_planes(&tiff, spots.len());
        for (spot, plane) in spots.iter().zip(&stored) {
            let rows: Vec<u16> = spot["stored_rows"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|row| numbers(row).into_iter().map(|v| v as u16))
                .collect();
            assert_eq!(plane, &rows, "{name}: two parsers agree");
            for (p, &s) in plane.iter().enumerate() {
                let (want, slack) = scene(spot, p, bits);
                assert!(
                    (max - s).abs_diff(want) <= slack,
                    "{name} {}: pixel {p}",
                    spot["name"]
                );
            }
        }
        // The byte open: names, inks, solidity, coverage narrowed.
        let back = open_byte(&path);
        let got = byte_spots(&back);
        assert_eq!(got.len(), spots.len(), "{name}");
        for ((got_name, ink, plane), (spot, stored)) in got.iter().zip(spots.iter().zip(&stored)) {
            assert_eq!(got_name, spot["name"].as_str().unwrap(), "{name}");
            let ink = ink.expect("a spot ink");
            let lab: Vec<f64> = spot["stored_lab"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            assert_eq!(ink.lab().to_vec(), lab, "{name}: the stored words / 100");
            for (a, b) in ink
                .lab()
                .iter()
                .zip(spot["readback_lab"].as_array().unwrap())
            {
                assert!((a - b.as_f64().unwrap()).abs() <= 0.5, "{name}: readback");
            }
            assert_eq!(
                u64::from(ink.solidity()),
                spot["solidity"].as_u64().unwrap(),
                "{name}"
            );
            let want: Vec<u8> = stored
                .iter()
                .map(|&s| {
                    let c = max - s;
                    if bits == 16 {
                        ((u32::from(c) + 128) / 257) as u8
                    } else {
                        c as u8
                    }
                })
                .collect();
            assert_eq!(plane, &want, "{name}: byte coverage");
        }
        let names: Vec<String> = spots
            .iter()
            .map(|s| format!("\u{201c}{}\u{201d}", s["name"].as_str().unwrap()))
            .collect();
        let observation = format!(
            "{} spot channels open as spot channels with their names, inks (D50 Lab) and solidity: {}.",
            spots.len(),
            names.join(", ")
        );
        // The typed opens: RGB 16 and both CMYK files (an 8-bit RGB TIFF is
        // the byte document's).
        if name == "rgb8.tif" || name == "rgb8_t.tif" {
            continue;
        }
        let typed = open_typed(&path).unwrap();
        assert_eq!(typed.is_cmyk(), name.starts_with("cmyk"), "{name}");
        let got = typed_spots(&typed);
        for ((got_name, _, plane), (spot, stored)) in got.iter().zip(spots.iter().zip(&stored)) {
            assert_eq!(got_name, spot["name"].as_str().unwrap(), "{name}");
            let scale = if bits == 16 { 1 } else { 257 };
            let want: Vec<u16> = stored.iter().map(|&s| (max - s) * scale).collect();
            assert_eq!(plane, &want, "{name}: typed coverage");
        }
        let warnings = import_warnings(&typed);
        assert!(warnings.contains(&observation), "{name}: {warnings:?}");
        assert!(
            !warnings.iter().any(|w| w.contains("extra samples")),
            "{name}: {warnings:?}"
        );
    }
}

#[test]
fn doctored_resaves_photoshops_flat_tiffs_in_photoshops_layout() {
    let Some(manifest) = manifest() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    for name in FLAT {
        let file = entry(&manifest, name);
        let path = folder().join(name);
        let theirs = read_tiff(&std::fs::read(&path).unwrap());
        let spots = file["spots"].as_array().unwrap().len();
        let out = dir.path().join(name);
        match name {
            "rgb8.tif" | "rgb8_t.tif" => save_byte(&open_byte(&path), &out, TIFF),
            "rgb16.tif" => {
                let request = ExportRequest::parse(
                    r#"{"format":"tiff","bit_depth":16,"spot_channels":"keep"}"#,
                )
                .unwrap();
                let doc = open_typed(&path).unwrap();
                let result = doc.export_encoded(None, &request, LIMIT, 0, None).unwrap();
                std::fs::write(&out, result.bytes()).unwrap();
            }
            _ => save_cmyk_tiff(&open_typed(&path).unwrap(), &out),
        }
        let ours = read_tiff(&std::fs::read(&out).unwrap());
        let (want, got) = (theirs.tag(338), ours.tag(338));
        assert_eq!(got.len(), want.len(), "{name}: the extra samples");
        assert_eq!(
            got[got.len() - spots..],
            want[want.len() - spots..],
            "{name}: spot samples 0"
        );
        if got.len() > spots {
            assert_eq!(
                (want[0], got[0]),
                (1, 2),
                "{name}: the transparency, straight here"
            );
        }
        assert_resources_equal(&ours, file, name);
        assert_eq!(
            stored_planes(&ours, spots),
            stored_planes(&theirs, spots),
            "{name}: Photoshop's stored samples"
        );
    }
}

#[test]
fn photoshops_layered_tiffs_open_layered_with_their_spot_channels() {
    let Some(manifest) = manifest() else {
        return;
    };
    for name in ["rgb8_l.tif", "cmyk16_l.tif"] {
        let file = entry(&manifest, name);
        assert_eq!(file["kind"], "layered", "{name}");
        let path = folder().join(name);
        let bits = file["depth"].as_u64().unwrap();
        let max: u16 = if bits == 16 { 65535 } else { 255 };
        let spots = file["spots"].as_array().unwrap();
        let tiff = read_tiff(&std::fs::read(&path).unwrap());
        assert!(tiff.bytes.contains_key(&37724), "{name}: the layer block");
        let stored = stored_planes(&tiff, spots.len());
        // (name, D50 Lab, solidity, coverage at the file's depth) per spot.
        let got: Vec<(String, [f64; 3], u8, Vec<u16>)> = if name == "rgb8_l.tif" {
            byte_spots(&open8(&path, LAYERS).unwrap())
                .into_iter()
                .map(|(n, ink, plane)| {
                    let ink = ink.expect("a spot ink");
                    (
                        n,
                        ink.lab(),
                        ink.solidity(),
                        plane.into_iter().map(u16::from).collect(),
                    )
                })
                .collect()
        } else {
            let doc = open16(&path, LAYERS).unwrap();
            assert!(doc.is_cmyk(), "{name}");
            typed_spots(&doc)
                .into_iter()
                .map(|(n, ink, plane)| {
                    let ink = ink.expect("a spot ink");
                    (n, ink.lab(), ink.solidity(), plane)
                })
                .collect()
        };
        assert_eq!(got.len(), spots.len(), "{name}: every spot kept");
        for ((got_name, lab, solidity, plane), (spot, stored)) in
            got.iter().zip(spots.iter().zip(&stored))
        {
            assert_eq!(got_name, spot["name"].as_str().unwrap(), "{name}");
            let want: Vec<f64> = spot["stored_lab"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            assert_eq!(lab.to_vec(), want, "{name}: the stored words / 100");
            for (a, b) in lab.iter().zip(spot["readback_lab"].as_array().unwrap()) {
                assert!((a - b.as_f64().unwrap()).abs() <= 0.5, "{name}: readback");
            }
            assert_eq!(
                u64::from(*solidity),
                spot["solidity"].as_u64().unwrap(),
                "{name}"
            );
            let rows: Vec<u16> = spot["stored_rows"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|row| numbers(row).into_iter().map(|v| v as u16))
                .collect();
            assert_eq!(stored, &rows, "{name}: two parsers agree");
            let want: Vec<u16> = rows.iter().map(|&s| max - s).collect();
            assert_eq!(plane, &want, "{name}: coverage max − stored");
        }
        let names: Vec<String> = spots
            .iter()
            .map(|s| format!("\u{201c}{}\u{201d}", s["name"].as_str().unwrap()))
            .collect();
        let observation = format!(
            "{} spot channels open as spot channels with their names, inks (D50 Lab) and solidity: {}.",
            spots.len(),
            names.join(", ")
        );
        let report = import_report(&path);
        assert!(
            strings(&report, "observations").contains(&observation),
            "{name}: {report}"
        );
    }
}
