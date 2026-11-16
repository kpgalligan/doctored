//! Photoshop 26.10.0's layered CMYK, Grayscale and Lab Color TIFFs saved with
//! byte order IBM PC (little-endian, `II`) at 8 and 16 bits, through the C
//! API, each against its Macintosh-order twin.
//!
//! `core/tests/fixtures/psd_photoshop/tiff_little_endian_modes/photoshop.json`
//! lists every IBM file of the takes of the private harness's
//! `tiff_le_77_take.py` (its `README`): each a document Photoshop built in RGB
//! from the runner's PNGs — Bottom; Top, transparent past a column, in a
//! declared blend mode and opacity; a pass-through group holding Inner; Masked,
//! a Reveal Selection user mask, hidden — converted to the model without
//! flattening and saved with layer compression RLE and ZIP in both byte
//! orders. The Gray and Lab files and their twins are in the folder; the CMYK
//! files (each carrying its output profile) are read from the take's receipts
//! under `~/devel/doctored-fixtures/photoshop/tiff-little-endian-77/`, and each
//! one SKIPS with an eprintln when the receipts are absent. The oracles share
//! no code with `src/`:
//! - the twin, Photoshop's big-endian save of the same document in the same
//!   job, opened through the big-endian layered open: every layer's record,
//!   source samples and mask plane equal exactly;
//! - the twin's records (`twin_records`: rectangles, blend keys, opacity
//!   bytes, flags, mask rectangles, the SHA-256 of each decoded plane), and
//!   for CMYK the independent reader of `common/cmyk_psd_fixture.rs`;
//! - the runner's judge (`judge`): every channel decoded under the declared
//!   rule with no miss against the twin;
//! - Photoshop's own rows of each reopened file and of its twin.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_gray_psd, dr_precision_doc_open_lab_psd, dr_precision_doc_open_psd,
    dr_psd_import_report, dr_tiff_has_layer_block, dr_tiff_has_layers,
};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::Value;

use crate::cmyk_psd_fixture::read;
use crate::common::psd_photoshop::{fixture, import_report};
use crate::common::sha256::sha256_hex;
use crate::common::{cpath, take_err_string};

const FAMILY: &str = "tiff_little_endian_modes";
/// The receipts under `$HOME` that hold the "store" files (`README`).
const STORE: &str = "devel/doctored-fixtures/photoshop/tiff-little-endian-77";
const NO_COMPOSITE: &str = "a TIFF's flat image opens through the ordinary TIFF open";
const LAYERS: i32 = 0;
const MODELS: [&str; 3] = ["cmyk", "gray", "lab"];

fn manifest() -> Value {
    serde_json::from_slice(&std::fs::read(fixture(FAMILY, "photoshop.json")).unwrap()).unwrap()
}

/// The import report, or its refusal.
fn report(path: &Path) -> Result<Value, String> {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    if json.is_null() {
        return Err(take_err_string(error));
    }
    Ok(serde_json::from_str(&take_err_string(json)).expect("report JSON"))
}

/// One listed IBM file and its twin, where this machine holds them.
struct Listed {
    name: String,
    entry: Value,
    scene: Value,
    path: PathBuf,
    twin: PathBuf,
}

impl Listed {
    fn model(&self) -> &str {
        self.entry["model"].as_str().unwrap()
    }

    fn bits(&self) -> u16 {
        self.entry["bits"].as_u64().unwrap() as u16
    }

    fn scene_name(&self, layer: &str) -> String {
        self.scene["names"][layer].as_str().unwrap().to_string()
    }
}

fn store() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var("HOME").ok()?).join(STORE);
    root.is_dir().then_some(root)
}

/// Every listed file whose bytes this machine holds: the folder's always, the
/// receipts' ("store") when they are present, each one left out named.
fn listed() -> Vec<Listed> {
    let json = manifest();
    let store = store();
    let mut out = Vec::new();
    for (name, entry) in json["files"].as_object().unwrap() {
        let scene = json["scenes"][entry["scene"].as_str().unwrap()].clone();
        let twin = &entry["twin"];
        let (path, twin) = match entry["location"].as_str().unwrap() {
            "repo" => (
                fixture(FAMILY, name),
                fixture(FAMILY, twin["file"].as_str().unwrap()),
            ),
            "store" => match &store {
                Some(root) => (
                    root.join(entry["path"].as_str().unwrap()),
                    root.join(twin["path"].as_str().unwrap()),
                ),
                None => {
                    eprintln!("SKIP: {name}: the Photoshop receipts ~/{STORE} are absent");
                    continue;
                }
            },
            other => panic!("{name}: location {other}"),
        };
        out.push(Listed {
            name: name.clone(),
            entry: entry.clone(),
            scene,
            path,
            twin,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// A typed document handle freed through the C API on drop.
struct Owned(*mut PrecisionDocument);

impl Owned {
    fn doc(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) };
    }
}

/// `path` opened as `model`'s layered open does (mode 0 its layers, 1 its
/// stored composite): CMYK through the typed PSD open, Gray through the Gray
/// one (working Gray Dot Gain 20 %), Lab through the Lab one.
fn open(path: &Path, model: &str, mode: i32) -> Result<Owned, String> {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let file = cpath(path);
    let working = CString::new("dot_gain_20").unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        match model {
            "cmyk" => dr_precision_doc_open_psd(
                file.as_ptr(),
                mode,
                token.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            ),
            "gray" => dr_precision_doc_open_gray_psd(
                file.as_ptr(),
                mode,
                token.as_ptr(),
                working.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            ),
            "lab" => dr_precision_doc_open_lab_psd(
                file.as_ptr(),
                mode,
                token.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            ),
            other => panic!("model {other}"),
        }
    };
    if raw.is_null() {
        return Err(take_err_string(error));
    }
    Ok(Owned(raw))
}

/// The tree bottom-first in postorder (a group after its members), with each
/// layer's depth.
fn postorder<'a>(
    layers: &'a [PrecisionDocumentLayer],
    depth: u16,
    out: &mut Vec<(u16, &'a PrecisionDocumentLayer)>,
) {
    for layer in layers {
        if let Some(children) = layer.children() {
            postorder(children, depth + 1, out);
        }
        out.push((depth, layer));
    }
}

/// Every layer's record and samples, top-first, for an equality check (as
/// `lab_psd_photoshop.rs` compares a file with its twin; a CMYK layer's inks
/// beside the other models' codes).
fn snapshot(layers: &[PrecisionDocumentLayer], out: &mut Vec<String>) {
    for layer in layers.iter().rev() {
        let pixels = layer
            .source()
            .map(|image| (image.dimensions(), image.to_rgba16().unwrap()));
        let ink = layer
            .cmyk_source()
            .map(|image| (image.dimensions(), image.to_contiguous().unwrap()));
        let mask = layer.mask().map(|m| {
            let p = m.properties();
            (
                m.enabled(),
                m.plane().dimensions(),
                m.plane().samples().to_vec(),
                p.offset,
                p.outside_coverage,
            )
        });
        out.push(format!(
            "{} {:?} {} {} {} {} {:?} {:?} {:?} {:?} {:?}",
            layer.name(),
            layer.blend_mode(),
            layer.opacity(),
            layer.visible(),
            layer.clipped(),
            layer.is_pass_through(),
            layer.offset(),
            mask,
            layer.blend_if(),
            pixels,
            ink
        ));
        if let Some(children) = layer.children() {
            snapshot(children, out);
        }
    }
}

fn ints(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect()
}

fn blend_of(twin: &Value) -> BlendMode {
    match twin["blend"].as_str().unwrap() {
        "norm" => BlendMode::Normal,
        "mul " => BlendMode::Multiply,
        "scrn" => BlendMode::Screen,
        other => panic!("blend key {other}"),
    }
}

/// The twin's records by the role they play, bottom first: Bottom, Top,
/// Inner, the group, Masked.
fn twin_roles(file: &Listed) -> [&Value; 5] {
    let records = file.entry["twin_records"].as_array().unwrap();
    ["bottom", "top", "inner", "set", "masked"].map(|layer| {
        let name = file.scene_name(layer);
        records
            .iter()
            .find(|r| r["name"] == name.as_str())
            .unwrap_or_else(|| panic!("{}: no twin record {name}", file.name))
    })
}

/// One raster's record against the twin's (rectangles are top, left, bottom,
/// right).
fn check_record(label: &str, layer: &PrecisionDocumentLayer, twin: &Value) {
    let rect = ints(&twin["rect"]);
    assert_eq!(
        layer.offset(),
        (rect[1] as i32, rect[0] as i32),
        "{label}: offset"
    );
    let size = match (layer.cmyk_source(), layer.source()) {
        (Some(ink), _) => ink.dimensions(),
        (None, Some(image)) => image.dimensions(),
        (None, None) => panic!("{label}: not a raster"),
    };
    assert_eq!(
        size,
        ((rect[3] - rect[1]) as u32, (rect[2] - rect[0]) as u32),
        "{label}: size"
    );
    assert_eq!(layer.blend_mode(), blend_of(twin), "{label}: blend");
    assert_eq!(
        layer.opacity(),
        twin["opacity"].as_f64().unwrap() as f32 / 255.0,
        "{label}: opacity"
    );
    assert_eq!(
        layer.visible(),
        twin["flags"].as_u64().unwrap() & 2 == 0,
        "{label}: visible"
    );
    match (layer.mask(), twin["mask"].is_null()) {
        (None, true) => {}
        (Some(mask), false) => {
            let m = ints(&twin["mask"]["rect"]);
            assert_eq!(
                mask.properties().offset,
                (m[1] as i32, m[0] as i32),
                "{label}: mask offset"
            );
            assert_eq!(
                mask.plane().dimensions(),
                ((m[3] - m[1]) as u32, (m[2] - m[0]) as u32),
                "{label}: mask size"
            );
        }
        (mine, _) => panic!("{label}: mask {} against the twin's", mine.is_some()),
    }
}

/// A plane's SHA-256 as `photoshop.json` hashes the twin's: its samples over
/// the rectangle, big-endian at `bits` (an 8-bit code is a 16-bit sample / 257).
fn plane_hash(values: impl Iterator<Item = u16>, bits: u16) -> String {
    let bytes: Vec<u8> = values
        .flat_map(|v| {
            if bits == 8 {
                vec![(v / 257) as u8]
            } else {
                v.to_be_bytes().to_vec()
            }
        })
        .collect();
    sha256_hex(&bytes)
}

/// A Gray raster's gray (R of its RGBA16, `gray_psd_photoshop.rs`'s `own`),
/// alpha and mask planes against the twin's plane hashes.
fn check_gray_planes(label: &str, layer: &PrecisionDocumentLayer, twin: &Value, bits: u16) {
    let samples = layer.source().unwrap().to_rgba16().unwrap();
    let planes = &twin["planes"];
    for (id, k) in [("0", 0usize), ("-1", 3)] {
        assert_eq!(
            planes[id]["sha256"].as_str().unwrap(),
            plane_hash(samples.iter().map(|p| p[k]), bits),
            "{label} channel {id}: the twin's plane"
        );
    }
    if let Some(mask) = layer.mask() {
        assert_eq!(
            planes["-2"]["sha256"].as_str().unwrap(),
            plane_hash(mask.plane().samples().iter().copied(), bits),
            "{label}: the twin's mask plane"
        );
    }
}

#[test]
fn every_listed_file_is_little_endian_and_reports_its_layers() {
    let json = manifest();
    // Every take lists each model at both depths, RLE and ZIP.
    for take in json["takes"].as_array().unwrap() {
        let name = take["take"].as_str().unwrap();
        let mut seen: Vec<(String, u64, String)> = json["files"]
            .as_object()
            .unwrap()
            .values()
            .filter(|e| e["take"] == name)
            .map(|e| {
                (
                    e["model"].as_str().unwrap().to_string(),
                    e["bits"].as_u64().unwrap(),
                    e["compression"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        seen.sort();
        let mut want = Vec::new();
        for model in MODELS {
            for bits in [8, 16] {
                for compression in ["rle", "zip"] {
                    want.push((model.to_string(), bits, compression.to_string()));
                }
            }
        }
        assert_eq!(seen, want, "{name}: three models, two depths, RLE and ZIP");
    }
    let files = listed();
    // Every file first, so a failure names each one the layered open misses.
    let missed: Vec<String> = files
        .iter()
        .filter(|file| !unsafe { dr_tiff_has_layers(cpath(&file.path).as_ptr()) })
        .map(|file| {
            let answer = match report(&file.path) {
                Ok(report) => format!("reports {}", report["import_precision"]),
                Err(error) => format!("the report refuses: {error}"),
            };
            format!("{}: dr_tiff_has_layers false; {answer}", file.name)
        })
        .collect();
    assert!(
        missed.is_empty(),
        "the layered open misses {} of {} files: {missed:#?}",
        missed.len(),
        files.len()
    );
    for file in files {
        let name = &file.name;
        let bytes = std::fs::read(&file.path).unwrap();
        assert_eq!(&bytes[..4], b"II*\0", "{name}: byte order IBM PC");
        assert_eq!(
            sha256_hex(&bytes),
            file.entry["sha256"].as_str().unwrap(),
            "{name}"
        );
        let twin = std::fs::read(&file.twin).unwrap();
        assert_eq!(&twin[..4], b"MM\0*", "{name}'s twin: byte order Macintosh");
        assert_eq!(
            sha256_hex(&twin),
            file.entry["twin"]["sha256"].as_str().unwrap(),
            "{name}'s twin"
        );
        // The judge's reading: every channel under the declared rule, no miss.
        for channel in file.entry["judge"].as_array().unwrap() {
            assert_eq!(channel["verdict"], "CONFIRMED", "{name}: {channel}");
            assert_eq!(channel["misses"], 0, "{name}: {channel}");
        }
        assert!(
            unsafe { dr_tiff_has_layer_block(cpath(&file.path).as_ptr()) },
            "{name}"
        );
        let report = import_report(&file.path);
        assert_eq!(report["format"], "TIFF", "{name}: {report}");
        assert_eq!(report["import_precision"], file.model(), "{name}: {report}");
        assert_eq!(report["layered_available"], true, "{name}: {report}");
        assert_eq!(report["layer_count"], 4, "{name}: {report}");
        assert_eq!(report["group_count"], 1, "{name}: {report}");
        assert_eq!(report["stored_composite_available"], false, "{name}");
    }
}

#[test]
fn every_listed_file_opens_to_its_twin() {
    for file in listed() {
        let name = &file.name;
        let (model, bits) = (file.model(), file.bits());
        let mine = open(&file.path, model, LAYERS).unwrap_or_else(|e| panic!("{name}: {e}"));
        let theirs =
            open(&file.twin, model, LAYERS).unwrap_or_else(|e| panic!("{name}'s twin: {e}"));
        let mut layers = Vec::new();
        postorder(mine.doc().root_layers(), 0, &mut layers);
        let names: Vec<&str> = layers.iter().map(|(_, l)| l.name()).collect();
        let wanted = ["bottom", "top", "inner", "set", "masked"].map(|l| file.scene_name(l));
        assert_eq!(names, wanted, "{name}: the stack, bottom first");
        let depths: Vec<u16> = layers.iter().map(|(d, _)| *d).collect();
        assert_eq!(depths, [0, 0, 1, 0, 0], "{name}: Inner inside the group");
        let group = layers[3].1;
        assert!(group.children().is_some(), "{name}: a group");
        assert!(group.is_pass_through(), "{name}: Pass Through");
        let roles = twin_roles(&file);
        let ink = (model == "cmyk").then(|| read(&std::fs::read(&file.twin).unwrap()));
        for k in [0usize, 1, 2, 4] {
            let (layer, twin) = (layers[k].1, roles[k]);
            let label = format!("{name} {}", layer.name());
            check_record(&label, layer, twin);
            if model == "gray" {
                check_gray_planes(&label, layer, twin, bits);
            }
            if let Some(read) = &ink {
                let record = read.by_name(layer.name());
                assert_eq!(
                    layer.cmyk_source().unwrap().to_contiguous().unwrap(),
                    record.ink(read.depth),
                    "{label}: ink"
                );
                assert_eq!(
                    layer.mask().map(|m| m.plane().samples().to_vec()),
                    record.mask16(read.depth),
                    "{label}: mask"
                );
            }
        }
        assert!(!layers[4].1.visible(), "{name}: Masked is hidden");
        // Every record and sample equals the twin's, opened the same way.
        let (mut left, mut right) = (Vec::new(), Vec::new());
        snapshot(mine.doc().root_layers(), &mut left);
        snapshot(theirs.doc().root_layers(), &mut right);
        assert_eq!(left.len(), right.len(), "{name}");
        for (index, (l, r)) in left.iter().zip(&right).enumerate() {
            assert!(l == r, "{name}: record {index} differs from the twin's");
        }
        // The stored composite: a TIFF has none here.
        let refused = open(&file.path, model, 1)
            .err()
            .expect("no stored composite");
        assert!(refused.contains(NO_COMPOSITE), "{name}: {refused}");
    }
}

#[test]
fn photoshops_rows_of_each_reopened_file_are_its_twins() {
    let json = manifest();
    for (name, entry) in json["files"].as_object().unwrap() {
        let scene = &json["scenes"][entry["scene"].as_str().unwrap()];
        let names = |layer: &str| scene["names"][layer].as_str().unwrap().to_string();
        let rows = entry["photoshop_rows"]
            .as_array()
            .expect("Photoshop's rows");
        assert_eq!(
            Some(rows),
            entry["twin_rows"].as_array(),
            "{name}: the twin's rows"
        );
        let got: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
        let wanted = ["masked", "set", "inner", "top", "bottom"].map(names);
        assert_eq!(got, wanted, "{name}: top first");
        assert_eq!(rows[0]["visible"], false, "{name}");
        assert_eq!(rows[0]["has_user_mask"], true, "{name}");
        assert_eq!(rows[1]["mode"], "BlendMode.PASSTHROUGH", "{name}");
        assert_eq!(rows[2]["parent"], names("set").as_str(), "{name}");
        assert_eq!(
            rows[3]["mode"],
            format!("BlendMode.{}", scene["top_mode"].as_str().unwrap()).as_str(),
            "{name}: the declared mode"
        );
        // The DOM's percent of the stored byte (45 % is byte 115, 45.098 %).
        assert_eq!(
            rows[3]["opacity"].as_f64().unwrap().round() as i64,
            scene["top_opacity"].as_i64().unwrap(),
            "{name}"
        );
    }
}
