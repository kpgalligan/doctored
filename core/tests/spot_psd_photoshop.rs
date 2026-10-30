//! #547 spot channels 1b-1 against Photoshop's own files
//! (`tests/fixtures/psd_photoshop/spot_547`, authored by
//! `scripts/photoshop-probe/spot_psd_take.py`'s take1 on Photoshop 26.10.0:
//! RGB, CMYK and Grayscale at 8 and 16 bits, PSD and PSB, one or two spot
//! channels each). SKIP when the folder is absent.
//!
//! Each fixture's length, channel count, saved-channel resources and stored
//! spot rows hold `photoshop.json` through `spot_psd`'s own reader; the
//! stored rows hold the SCENE the take built (the ramp spot's coverage
//! v(x) = round(255 x / 31), exact at 8 bits and within 4 codes of 257 v at
//! 16; the block spot full inside x in [8, 24), y in [2, 6), empty outside),
//! under Photoshop's polarity `max − coverage`. Doctored's opens (byte,
//! typed RGB and CMYK, Gray), both choices, keep every spot channel's name,
//! Lab (the stored words / 100, within 0.5 of Photoshop's own readback),
//! Solidity and coverage exactly, and the report names them; Doctored's
//! re-save of the Grayscale, flattened CMYK and transparent RGB files writes
//! Photoshop's 1045, 1077 and 1053 bodies byte for byte (and 1006 where the
//! names are ASCII: Photoshop's 1006 is Mac Roman) and the same stored rows.
//!
//! 1b-2, the readback takes in place (`~/devel/doctored-fixtures/photoshop/
//! spot-psd-readback-547/take1` and `take2`, `scripts/photoshop-probe/
//! spot_psd_readback_take.py`, Photoshop 26.10.0; each SKIPPED with an
//! eprintln when `HOME` is unset or its `judged.json` is absent). For every
//! file a take judged PASS: Photoshop's raw copy, opened by the core as
//! `rasterize_opens_photoshops_spot_channels` opens (byte for RGB 8, Gray for
//! Gray and Duotone, typed otherwise; layers), holds each declared spot's
//! name, Lab within 0.01, Solidity exactly and coverage within the take's
//! bounds (the block exact, the ramp v(x) within 1 code at 8 bits and 4 at
//! 16, in the file's codes); and our input, opened the same way and re-saved
//! by the current writer in its container, still writes the input's 1006,
//! 1045, 1077 and 1053 bodies and spot planes. take1 missed only 16-bit
//! Duotone (Photoshop opened none of those files, spots or not); by the
//! controller's ruling a 16-bit Duotone document's PSD/PSB is now written at
//! 8 bits (`psd_export_gray`), and take2 judged those files PASS.

use std::path::{Path, PathBuf};

use doctored_core::precision_document::PrecisionDocument;
use doctored_core::spot_channel::SpotInk;
use serde_json::Value;
use tempfile::TempDir;

use crate::psb_fixture::{
    import_report, open16, open8, save16, strings, FLATTENED, LAYERS, PSB, PSD,
};
use crate::spot_psd::{open_gray, parse, save_null_projection};

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/spot_547")
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

fn files(manifest: &Value) -> &Vec<Value> {
    manifest["files"].as_array().unwrap()
}

fn spots(file: &Value) -> &Vec<Value> {
    file["spots"].as_array().unwrap()
}

fn rows(spot: &Value) -> Vec<u16> {
    spot["stored_rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| {
            row.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16)
        })
        .collect()
}

fn floats(value: &Value) -> [f64; 3] {
    let a = value.as_array().unwrap();
    [0, 1, 2].map(|i| a[i].as_f64().unwrap())
}

fn max(file: &Value) -> u16 {
    if file["depth"] == 16 {
        65535
    } else {
        255
    }
}

/// The spot's coverage as Doctored holds it: `max − stored`, ×257 in a
/// typed document at 8 bits.
fn coverage(file: &Value, spot: &Value, typed: bool) -> Vec<u16> {
    let top = max(file);
    rows(spot)
        .iter()
        .map(|&s| {
            let c = top - s;
            if typed && top == 255 {
                c * 257
            } else {
                c
            }
        })
        .collect()
}

fn ink(spot: &Value) -> SpotInk {
    let solidity = spot["stored_opacity"].as_u64().unwrap();
    assert_eq!(
        spot["solidity"].as_f64(),
        Some(solidity as f64),
        "Photoshop's own readback"
    );
    SpotInk::new(floats(&spot["stored_lab"]), solidity as u8).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn v(x: u32) -> u16 {
    ((255 * x * 2 + 31) / 62) as u16
}

#[test]
fn photoshops_files_hold_their_manifest_and_their_scene() {
    let Some(manifest) = manifest() else {
        return;
    };
    assert_eq!(files(&manifest).len(), 7);
    for file in files(&manifest) {
        let name = file["name"].as_str().unwrap();
        let bytes = std::fs::read(folder().join(name)).unwrap();
        assert_eq!(
            bytes.len() as u64,
            file["bytes"].as_u64().unwrap(),
            "{name}"
        );
        let parsed = parse(&bytes);
        assert_eq!(
            parsed.channels as u64,
            file["channels"].as_u64().unwrap(),
            "{name}"
        );
        for (id, body) in file["channel_resources"].as_object().unwrap() {
            let id: u16 = id.parse().unwrap();
            assert_eq!(
                parsed.resource(id).map(hex).as_deref(),
                body.as_str(),
                "{name}: {id}"
            );
        }
        let first = file["first_saved_plane"].as_u64().unwrap() as usize;
        let top = max(file);
        for (k, spot) in spots(file).iter().enumerate() {
            let what = format!("{name}: {}", spot["name"]);
            assert_eq!(parsed.samples(first + k), rows(spot), "{what}");
            let lab = floats(&spot["stored_lab"]);
            for (got, want) in lab.iter().zip(floats(&spot["readback_lab"])) {
                assert!((got - want).abs() <= 0.5, "{what}: Lab {lab:?}");
            }
            for (i, stored) in rows(spot).iter().enumerate() {
                let (x, y) = (i as u32 % 32, i as u32 / 32);
                let c = top - stored;
                if spot["source"] == "red" {
                    let want = if top == 255 { v(x) } else { v(x) * 257 };
                    let slack = if top == 255 { 0 } else { 4 };
                    assert!(
                        c.abs_diff(want) <= slack,
                        "{what}: ({x}, {y}) {c} for {want}"
                    );
                } else {
                    let inside = (8..24).contains(&x) && (2..6).contains(&y);
                    assert_eq!(c, if inside { top } else { 0 }, "{what}: ({x}, {y})");
                }
            }
        }
    }
}

fn typed_spots(doc: &PrecisionDocument) -> Vec<(String, Option<SpotInk>, Vec<u16>)> {
    doc.channels()
        .iter()
        .map(|c| {
            (
                c.name().to_string(),
                c.metadata().spot,
                c.plane().samples().to_vec(),
            )
        })
        .collect()
}

#[test]
fn rasterize_opens_photoshops_spot_channels() {
    let Some(manifest) = manifest() else {
        return;
    };
    for file in files(&manifest) {
        let name = file["name"].as_str().unwrap();
        let path = folder().join(name);
        let report = import_report(&path);
        let names: Vec<String> = spots(file)
            .iter()
            .map(|s| format!("\u{201c}{}\u{201d}", s["name"].as_str().unwrap()))
            .collect();
        let sentence = format!(
            "{} spot channels open as spot channels with their names, inks (D50 Lab) and solidity: {}.",
            names.len(),
            names.join(", ")
        );
        assert!(
            strings(&report, "observations").contains(&sentence),
            "{name}: {report}"
        );
        assert!(
            !strings(&report, "verified_losses")
                .iter()
                .any(|l| l.contains("saved channels")),
            "{name}: every saved channel is a spot channel: {report}"
        );
        let typed = file["mode"] != "rgb" || file["depth"] == 16;
        let want: Vec<(String, Option<SpotInk>, Vec<u16>)> = spots(file)
            .iter()
            .map(|s| {
                (
                    s["name"].as_str().unwrap().to_string(),
                    Some(ink(s)),
                    coverage(file, s, typed),
                )
            })
            .collect();
        for mode in [LAYERS, FLATTENED] {
            let got = if !typed {
                let doc = open8(&path, mode).unwrap();
                doc.channels
                    .iter()
                    .map(|c| {
                        let plane = c.data.as_raw().iter().map(|&v| u16::from(v)).collect();
                        (c.name.clone(), c.spot, plane)
                    })
                    .collect()
            } else if file["mode"] == "gray" {
                typed_spots(&open_gray(&path, mode))
            } else {
                typed_spots(&open16(&path, mode).unwrap())
            };
            assert_eq!(got, want, "{name}, mode {mode}");
        }
    }
}

#[test]
fn rasterize_writes_photoshops_resource_forms() {
    let Some(manifest) = manifest() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let entry = |name: &str| {
        files(&manifest)
            .iter()
            .find(|f| f["name"] == name)
            .unwrap()
            .clone()
    };
    // (fixture, our container and mode): the forms whose transparency entry
    // matches Photoshop's own (Gray positive, flattened CMYK no records,
    // the transparent RGB PSB merged as ours always is).
    for (name, container, mode) in [
        ("gray8.psd", PSD, LAYERS),
        ("gray16.psb", PSB, LAYERS),
        ("cmyk8.psd", PSD, FLATTENED),
        ("cmyk16.psb", PSB, FLATTENED),
        ("rgb8_t.psb", PSB, LAYERS),
    ] {
        let file = entry(name);
        let source = folder().join(name);
        let out = dir.path().join(name);
        if file["mode"] == "gray" {
            save16(&open_gray(&source, LAYERS), &out, container, mode).unwrap();
        } else if file["mode"] == "cmyk" {
            save_null_projection(&open16(&source, LAYERS).unwrap(), &out, container, mode);
        } else {
            crate::psb_fixture::save8(&open8(&source, LAYERS).unwrap(), &out, container, mode)
                .unwrap();
        }
        let ours = parse(&std::fs::read(&out).unwrap());
        let resources = file["channel_resources"].as_object().unwrap();
        for id in [1045u16, 1077, 1053] {
            assert_eq!(
                ours.resource(id).map(hex).as_deref(),
                resources[&id.to_string()].as_str(),
                "{name}: {id}"
            );
        }
        let ascii = spots(&file)
            .iter()
            .all(|s| s["name"].as_str().unwrap().is_ascii());
        if ascii {
            assert_eq!(
                ours.resource(1006).map(hex).as_deref(),
                resources["1006"].as_str(),
                "{name}"
            );
        }
        let first = ours.channels - spots(&file).len();
        for (k, spot) in spots(&file).iter().enumerate() {
            assert_eq!(
                ours.samples(first + k),
                rows(spot),
                "{name}: {}",
                spot["name"]
            );
        }
    }
}

// ------------------------------------------------------------- 1b-2 --

/// `take` (take1 or take2) of `spot_psd_readback_take.py` in place: its
/// folder and `judged.json`, or `None` (SKIP) when `HOME` is unset or the
/// take is absent.
fn readback_take(take: &str) -> Option<(PathBuf, Value)> {
    let Some(home) = std::env::var("HOME").ok() else {
        eprintln!("SKIP: HOME is unset, so the readback take cannot be found");
        return None;
    };
    let root = PathBuf::from(home)
        .join("devel/doctored-fixtures/photoshop/spot-psd-readback-547")
        .join(take);
    let Ok(text) = std::fs::read_to_string(root.join("judged.json")) else {
        eprintln!("SKIP: the readback take {} is absent", root.display());
        return None;
    };
    Some((root, serde_json::from_str(&text).unwrap()))
}

/// A take's PASS files: (file name, declared row, judged row).
type Passed = Vec<(String, Value, Value)>;

/// The take's PASS files.
fn passed(judged: &Value) -> Passed {
    let declared = judged["declared"].as_object().unwrap();
    judged["files"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, file)| file["verdict"] == "PASS")
        .map(|(name, file)| (name.clone(), declared[name].clone(), file.clone()))
        .collect()
}

/// Every spot channel of the file at `path` as the core opens it in layers
/// mode (byte for RGB 8, Gray for Gray and Duotone, typed otherwise):
/// (name, ink, coverage in the file's own codes).
fn opened_spots(path: &Path, mode: &str, bits: u64) -> Vec<(String, Option<SpotInk>, Vec<u16>)> {
    if mode == "rgb" && bits == 8 {
        let doc = open8(path, LAYERS).unwrap();
        return doc
            .channels
            .iter()
            .map(|c| {
                let plane = c.data.as_raw().iter().map(|&v| u16::from(v)).collect();
                (c.name.clone(), c.spot, plane)
            })
            .collect();
    }
    let doc = if mode == "gray" || mode == "duo" {
        open_gray(path, LAYERS)
    } else {
        open16(path, LAYERS).unwrap()
    };
    typed_spots(&doc)
        .into_iter()
        .map(|(name, ink, plane)| {
            // A typed document holds an 8-bit file's coverage ×257.
            let plane = if bits == 8 {
                plane.iter().map(|c| c / 257).collect()
            } else {
                plane
            };
            (name, ink, plane)
        })
        .collect()
}

/// The depth a declared file is written at: take2's `file_bits` (a 16-bit
/// Duotone document written at 8), else its document's `bits` (take1).
fn file_bits(declared: &Value) -> u64 {
    declared
        .get("file_bits")
        .unwrap_or(&declared["bits"])
        .as_u64()
        .unwrap()
}

/// Each readback take present, with the files it judged PASS. take1's
/// recorded result: every gated file PASS but 16-bit Duotone, which
/// Photoshop did not open; take2's (RULING-1: those files now written at 8
/// bits): both of them PASS.
fn readback_takes() -> Vec<(PathBuf, Passed)> {
    let mut out = Vec::new();
    if let Some((root, judged)) = readback_take("take1") {
        assert_eq!(judged["refuse_forms"], serde_json::json!([["duo", 16]]));
        let files = passed(&judged);
        assert_eq!(files.len(), 15, "take1's PASS files");
        out.push((root, files));
    }
    if let Some((root, judged)) = readback_take("take2") {
        assert_eq!(judged["verdict"], "PASS", "take2");
        assert_eq!(judged["refuse_forms"], serde_json::json!([]));
        let files = passed(&judged);
        let mut names: Vec<&str> = files.iter().map(|(n, _, _)| n.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["duo16.psb", "duo16.psd"], "take2's PASS files");
        assert!(files.iter().all(|(_, d, _)| file_bits(d) == 8));
        out.push((root, files));
    }
    out
}

#[test]
fn photoshops_copies_of_rasterizes_files_keep_their_spot_channels() {
    for (root, files) in readback_takes() {
        check_copies(&root, files);
    }
}

/// One take's PASS files: Photoshop's copy holds each declared spot.
fn check_copies(root: &Path, files: Passed) {
    for (name, declared, file) in files {
        let copy = root
            .join("jobs/readback")
            .join(file["copy"].as_str().unwrap());
        let bits = file_bits(&declared);
        let got = opened_spots(&copy, declared["mode"].as_str().unwrap(), bits);
        let top: u16 = if bits == 16 { 65535 } else { 255 };
        for spot in declared["spots"].as_array().unwrap() {
            let spot_name = spot["name"].as_str().unwrap();
            let what = format!("{name}: {spot_name}");
            let (_, ink, plane) = got
                .iter()
                .find(|(n, _, _)| n == spot_name)
                .unwrap_or_else(|| panic!("{what}: absent"));
            let ink = ink.unwrap_or_else(|| panic!("{what}: not a spot channel"));
            for (got, want) in ink.lab().iter().zip(floats(&spot["lab"])) {
                assert!((got - want).abs() <= 0.01, "{what}: Lab {:?}", ink.lab());
            }
            assert_eq!(
                u64::from(ink.solidity()),
                spot["solidity"].as_u64().unwrap(),
                "{what}"
            );
            assert_eq!(plane.len(), 32 * 8, "{what}");
            for (i, &c) in plane.iter().enumerate() {
                let (x, y) = (i as u32 % 32, i as u32 / 32);
                if spot["source"] == "red" {
                    let want = if bits == 16 { v(x) * 257 } else { v(x) };
                    let slack = if bits == 16 { 4 } else { 1 };
                    assert!(
                        c.abs_diff(want) <= slack,
                        "{what}: ({x}, {y}) {c} for {want}"
                    );
                } else {
                    let inside = (8..24).contains(&x) && (2..6).contains(&y);
                    assert_eq!(c, if inside { top } else { 0 }, "{what}: ({x}, {y})");
                }
            }
        }
    }
}

#[test]
fn rasterizes_writer_still_writes_the_files_photoshop_read() {
    let dir = TempDir::new().unwrap();
    for (name, declared, root) in readback_takes()
        .into_iter()
        .flat_map(|(root, files)| files.into_iter().map(move |(n, d, _)| (n, d, root.clone())))
    {
        let input = root.join("inputs").join(&name);
        let mode = declared["mode"].as_str().unwrap();
        let bits = file_bits(&declared);
        let container = if declared["container"] == "psb" {
            PSB
        } else {
            PSD
        };
        let out = dir.path().join(&name);
        if mode == "rgb" && bits == 8 {
            crate::psb_fixture::save8(&open8(&input, LAYERS).unwrap(), &out, container, LAYERS)
                .unwrap();
        } else if mode == "gray" || mode == "duo" {
            save16(&open_gray(&input, LAYERS), &out, container, LAYERS).unwrap();
        } else if mode == "cmyk" {
            save_null_projection(&open16(&input, LAYERS).unwrap(), &out, container, LAYERS);
        } else {
            save16(&open16(&input, LAYERS).unwrap(), &out, container, LAYERS).unwrap();
        }
        let theirs = parse(&std::fs::read(&input).unwrap());
        let ours = parse(&std::fs::read(&out).unwrap());
        for id in [1006u16, 1045, 1077, 1053] {
            assert!(theirs.resource(id).is_some(), "{name}: {id}");
            assert_eq!(
                ours.resource(id).map(hex),
                theirs.resource(id).map(hex),
                "{name}: {id}"
            );
        }
        let n = declared["spots"].as_array().unwrap().len();
        for k in 0..n {
            assert_eq!(
                ours.samples(ours.channels - n + k),
                theirs.samples(theirs.channels - n + k),
                "{name}: spot {k}"
            );
        }
    }
}
