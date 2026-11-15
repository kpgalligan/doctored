//! Photoshop 26.10.0's layered RGB TIFFs saved with byte order IBM PC
//! (little-endian, `II`) at 8 and 16 bits, through the C API, and the
//! little-endian records the layered open refuses by name.
//!
//! `core/tests/fixtures/psd_photoshop/tiff_little_endian/` holds the IBM
//! files of two takes (its `README`): each a document Photoshop built from the
//! runner's PNGs — Bottom; Top, transparent past a column, in a declared
//! blend mode and opacity; a pass-through group holding Inner, transparent
//! outside a rectangle with partial alpha in its lower rows; Masked, a
//! Reveal Selection user mask, hidden — saved with layer compression RLE and
//! ZIP. The oracles share no code with `src/`:
//! - the scene formula, restated here from the runner's declaration
//!   (`photoshop.json`'s `scenes`): every colour sample where alpha is not
//!   zero, every alpha and mask sample, 8 bits exact and 16 bits within 2
//!   codes (Photoshop's 15-bit store);
//! - the Macintosh-order twin Photoshop saved from the same document in the
//!   same job: its records (`twin_records`: rectangles, blend keys, opacity
//!   bytes, flags, mask rectangles) and the SHA-256 of each channel's decoded
//!   big-endian plane;
//! - Photoshop's own rows of each reopened file.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::{BlendMode, LayerKind};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_info, dr_precision_doc_layer_source,
    dr_precision_image_free, dr_precision_image_pixels_rgba16,
};
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_precision_doc_open_psd, dr_psd_import_report, dr_tiff_has_layer_block,
    dr_tiff_has_layers,
};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::psd_photoshop::{fixture, import_report, open_psd, LAYERS};
use crate::common::sha256::sha256_hex;
use crate::common::{cpath, take_err_string};

const FAMILY: &str = "tiff_little_endian";
const PREFIX: &[u8] = b"Adobe Photoshop Document Data Block\0";
const NO_COMPOSITE: &str = "a TIFF's flat image opens through the ordinary TIFF open";
const READ_TAIL: &str =
    "is not read; open it flat, or resave it from Photoshop with Macintosh byte order";

fn manifest() -> Value {
    serde_json::from_slice(&std::fs::read(fixture(FAMILY, "photoshop.json")).unwrap()).unwrap()
}

fn report(path: &Path) -> Result<Value, String> {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    if json.is_null() {
        return Err(take_err_string(error));
    }
    Ok(serde_json::from_str(&take_err_string(json)).expect("report JSON"))
}

// ------------------------------------------------------------ the scene --

/// One declared scene (the runner's `SCENES`, as `photoshop.json` holds it).
struct Scene<'a>(&'a Value);

impl Scene<'_> {
    fn int(&self, key: &str) -> i64 {
        self.0[key].as_i64().unwrap()
    }
    fn rect(&self, key: &str) -> [i64; 4] {
        let r = self.0[key].as_array().unwrap();
        [0, 1, 2, 3].map(|k| r[k].as_i64().unwrap())
    }
    fn name(&self, layer: &str) -> String {
        self.0["names"][layer].as_str().unwrap().to_string()
    }
}

fn inside(rect: [i64; 4], x: i64, y: i64) -> bool {
    x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3]
}

/// A 15-bit code's 16-bit image, as the runner wrote it and Photoshop's
/// 15-bit store keeps it.
fn image16(k: i64) -> i64 {
    (65535 * k + 16384) / 32768
}

/// Layer `layer` (0 Bottom, 1 Top, 2 Inner, 3 Masked), channel `c`, at
/// (x, y): the runner's declared formula.
fn colour(scene: &Scene, layer: i64, c: i64, x: i64, y: i64, bits: u16) -> i64 {
    let seed = scene.int("seed");
    let a = (29 * seed + 61 * layer + 97 * c + 11) % 256;
    let b = 1 + (seed + 3 * layer + 5 * c) % 13;
    let cc = 1 + (2 * seed + 7 * layer + 3 * c) % 11;
    let e = (seed + layer + c) % 4;
    if bits == 8 {
        (a + b * x + cc * y + e * x * y) % 256
    } else {
        image16((127 * a + 59 + 331 * b * x + 523 * cc * y + 97 * e * x * y) % 32769)
    }
}

fn alpha(scene: &Scene, layer: i64, x: i64, y: i64, bits: u16) -> i64 {
    let full = if bits == 8 { 255 } else { 65535 };
    match layer {
        1 if x < scene.int("top_keep") => full,
        1 => 0,
        2 => {
            let rect = scene.rect("inner_rect");
            if !inside(rect, x, y) {
                0
            } else if y < rect[1] + scene.int("inner_opaque_rows") {
                full
            } else if bits == 8 {
                60 + 7 * (x - rect[0])
            } else {
                image16(7000 + 900 * (x - rect[0]))
            }
        }
        _ => full,
    }
}

fn mask_value(scene: &Scene, x: i64, y: i64, bits: u16) -> i64 {
    match (inside(scene.rect("mask_rect"), x, y), bits) {
        (false, _) => 0,
        (true, 8) => 255,
        (true, _) => 65535,
    }
}

/// A mask plane as the open gives it: its offset, its size and its samples.
type MaskPlane = ((i64, i64), (i64, i64), Vec<i64>);

/// One layer's samples as the open gives them: `rgba` interleaved over its
/// rectangle at `offset`, and its mask plane (`None` without one).
struct Opened {
    name: String,
    offset: (i64, i64),
    size: (i64, i64),
    rgba: Vec<i64>,
    mask: Option<MaskPlane>,
}

/// The planes' SHA-256 as `photoshop.json` hashes the twin's: each channel's
/// samples over the rectangle, big-endian at `bits`.
fn plane_hash(values: impl Iterator<Item = i64>, bits: u16) -> String {
    let bytes: Vec<u8> = values
        .flat_map(|v| {
            if bits == 8 {
                vec![v as u8]
            } else {
                (v as u16).to_be_bytes().to_vec()
            }
        })
        .collect();
    sha256_hex(&bytes)
}

/// Every check of one opened layer against the formula and the twin's record
/// (`twin` is its `twin_records` entry; rectangles are top, left, bottom,
/// right).
fn check_layer(scene: &Scene, index: i64, layer: &Opened, twin: &Value, bits: u16) {
    let name = &layer.name;
    let rect: Vec<i64> = twin["rect"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(layer.offset, (rect[1], rect[0]), "{name}: offset");
    assert_eq!(
        layer.size,
        (rect[3] - rect[1], rect[2] - rect[0]),
        "{name}: size"
    );
    let tolerance = if bits == 8 { 0 } else { 2 };
    let (w, h) = layer.size;
    for j in 0..h {
        for i in 0..w {
            let (x, y) = (layer.offset.0 + i, layer.offset.1 + j);
            let at = ((j * w + i) * 4) as usize;
            let a = alpha(scene, index, x, y, bits);
            assert!(
                (layer.rgba[at + 3] - a).abs() <= tolerance,
                "{name} alpha at ({x}, {y}): {} against {a}",
                layer.rgba[at + 3]
            );
            if a == 0 {
                continue;
            }
            for c in 0..3 {
                let want = colour(scene, index, c, x, y, bits);
                let got = layer.rgba[at + c as usize];
                assert!(
                    (got - want).abs() <= tolerance,
                    "{name} channel {c} at ({x}, {y}): {got} against {want}"
                );
            }
        }
    }
    let planes = &twin["planes"];
    for (id, k) in [("-1", 3usize), ("0", 0), ("1", 1), ("2", 2)] {
        let hash = plane_hash(layer.rgba.iter().skip(k).step_by(4).copied(), bits);
        assert_eq!(
            planes[id]["sha256"].as_str().unwrap(),
            hash,
            "{name} channel {id}: the twin's plane"
        );
    }
    match (&layer.mask, twin["mask"].is_null()) {
        (None, true) => {}
        (Some((offset, size, samples)), false) => {
            let m: Vec<i64> = twin["mask"]["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap())
                .collect();
            assert_eq!(*offset, (m[1], m[0]), "{name}: mask offset");
            assert_eq!(*size, (m[3] - m[1], m[2] - m[0]), "{name}: mask size");
            for j in 0..size.1 {
                for i in 0..size.0 {
                    let want = mask_value(scene, offset.0 + i, offset.1 + j, bits);
                    assert_eq!(samples[(j * size.0 + i) as usize], want, "{name}: mask");
                }
            }
            assert_eq!(
                planes["-2"]["sha256"].as_str().unwrap(),
                plane_hash(samples.iter().copied(), bits),
                "{name}: the twin's mask plane"
            );
        }
        (mine, _) => panic!("{name}: mask {:?} against the twin's", mine.is_some()),
    }
}

/// The opacity byte a `twin_records` entry holds.
fn opacity_byte(twin: &Value) -> f32 {
    twin["opacity"].as_f64().unwrap() as f32 / 255.0
}

fn blend_of(twin: &Value) -> BlendMode {
    match twin["blend"].as_str().unwrap() {
        "norm" => BlendMode::Normal,
        "mul " => BlendMode::Multiply,
        "scrn" => BlendMode::Screen,
        other => panic!("blend key {other}"),
    }
}

// ------------------------------------------------------------- the files --

/// Every committed file, its manifest entry and its scene.
fn files(json: &Value) -> Vec<(String, Value, Value)> {
    let mut out: Vec<(String, Value, Value)> = json["files"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, entry)| {
            let scene = json["scenes"][entry["scene"].as_str().unwrap()].clone();
            (name.clone(), entry.clone(), scene)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The twin's records by the role they play, bottom first: Bottom, Top,
/// Inner, the group, Masked (the divider dropped).
fn twin_roles<'a>(entry: &'a Value, scene: &Scene) -> [&'a Value; 5] {
    let records = entry["twin_records"].as_array().unwrap();
    let find = |layer: &str| {
        let name = scene.name(layer);
        records
            .iter()
            .find(|r| r["name"] == name.as_str())
            .unwrap_or_else(|| panic!("no twin record {name}"))
    };
    [
        find("bottom"),
        find("top"),
        find("inner"),
        find("set"),
        find("masked"),
    ]
}

#[test]
fn every_committed_file_is_little_endian_and_reports_its_layers() {
    let json = manifest();
    let all = files(&json);
    assert_eq!(all.len(), 8, "two takes, two depths, RLE and ZIP");
    for (name, entry, _) in &all {
        let path = fixture(FAMILY, name);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"II*\0", "{name}: byte order IBM PC");
        assert_eq!(
            sha256_hex(&bytes),
            entry["sha256"].as_str().unwrap(),
            "{name}"
        );
        assert!(
            unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) },
            "{name}"
        );
        let report = import_report(&path);
        let bits = entry["bits"].as_u64().unwrap();
        assert_eq!(report["format"], "TIFF", "{name}: {report}");
        assert_eq!(
            report["import_precision"],
            if bits == 8 { "rgba8" } else { "rgba16" },
            "{name}"
        );
        assert_eq!(report["layered_available"], true, "{name}: {report}");
        assert_eq!(report["layer_count"], 4, "{name}: {report}");
        assert_eq!(report["group_count"], 1, "{name}: {report}");
        assert_eq!(report["stored_composite_available"], false, "{name}");
    }
    // The two twins committed beside their IBM files (for the app's drive) are
    // the files the manifest names.
    let mut twins = 0;
    for (name, entry, _) in &all {
        let twin = fixture(FAMILY, entry["twin"]["file"].as_str().unwrap());
        let Ok(bytes) = std::fs::read(&twin) else {
            continue;
        };
        assert_eq!(&bytes[..4], b"MM\0*", "{name}'s twin: byte order Macintosh");
        assert_eq!(
            sha256_hex(&bytes),
            entry["twin"]["sha256"].as_str().unwrap(),
            "{name}'s twin"
        );
        twins += 1;
    }
    assert_eq!(
        twins, 2,
        "the 8-bit RLE and 16-bit ZIP twins of the held-out scene"
    );
}

#[test]
fn the_eight_bit_files_open_to_the_scene_and_the_twin() {
    let json = manifest();
    for (name, entry, scene) in files(&json) {
        if entry["bits"] != 8 {
            continue;
        }
        let scene = Scene(&scene);
        let path = fixture(FAMILY, &name);
        let report = import_report(&path);
        let doc = open_psd(&path, &report, LAYERS);
        let roles = twin_roles(&entry, &scene);
        let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
        let wanted = ["bottom", "top", "inner", "set", "masked"].map(|l| scene.name(l));
        assert_eq!(names, wanted, "{name}: the stack, bottom first");
        let depths: Vec<u16> = doc.layers.iter().map(|l| l.depth).collect();
        assert_eq!(depths, [0, 0, 1, 0, 0], "{name}: Inner inside the group");
        let group = &doc.layers[3];
        assert_eq!(group.kind, LayerKind::Group, "{name}");
        assert_eq!(group.blend, BlendMode::PassThrough, "{name}: Pass Through");
        for (index, (layer, twin)) in [0usize, 1, 2, 4]
            .map(|k| (&doc.layers[k], roles[k]))
            .iter()
            .enumerate()
        {
            let label = format!("{name} {}", layer.name);
            assert_eq!(layer.kind, LayerKind::Raster, "{label}");
            assert_eq!(layer.opacity, opacity_byte(twin), "{label}: opacity");
            assert_eq!(layer.blend, blend_of(twin), "{label}: blend");
            assert_eq!(
                layer.visible,
                twin["flags"].as_u64().unwrap() & 2 == 0,
                "{label}"
            );
            let image = layer.pixels.contiguous_image();
            let (w, h) = image.dimensions();
            let mask = layer.mask.as_ref().map(|mask| {
                let (mw, mh) = mask.dimensions();
                let offset = layer.mask_properties.offset;
                assert_eq!(layer.mask_properties.outside_coverage, 0, "{label}");
                assert!(layer.mask_enabled, "{label}");
                (
                    (i64::from(offset.0), i64::from(offset.1)),
                    (i64::from(mw), i64::from(mh)),
                    mask.samples().iter().map(|v| i64::from(*v)).collect(),
                )
            });
            let opened = Opened {
                name: label,
                offset: (i64::from(layer.offset.0), i64::from(layer.offset.1)),
                size: (i64::from(w), i64::from(h)),
                rgba: image.as_raw().iter().map(|v| i64::from(*v)).collect(),
                mask,
            };
            check_layer(&scene, index as i64, &opened, twin, 8);
        }
        assert!(!doc.layers[4].visible, "{name}: Masked is hidden");
        assert_eq!(
            doc.layers[1].blend,
            match scene.0["top_mode"].as_str().unwrap() {
                "MULTIPLY" => BlendMode::Multiply,
                _ => BlendMode::Screen,
            },
            "{name}: the declared mode"
        );
        assert_eq!(
            (doc.layers[1].opacity * 100.0).round() as i64,
            scene.int("top_opacity"),
            "{name}: the declared opacity"
        );
        // The stored composite: a TIFF has none here.
        let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
        let mut error = ptr::null_mut();
        let none = unsafe {
            dr_doc_open_psd(
                cpath(&path).as_ptr(),
                1,
                token.as_ptr(),
                ptr::null(),
                &mut error,
            )
        };
        assert!(none.is_null(), "{name}");
        assert!(take_err_string(error).contains(NO_COMPOSITE), "{name}");
    }
}

/// A typed document handle freed through the C API on drop.
struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) };
    }
}

fn open_typed(path: &Path, report: &Value, mode: i32) -> Result<Owned, String> {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            token.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if raw.is_null() {
        return Err(take_err_string(error));
    }
    Ok(Owned(raw))
}

#[test]
fn the_sixteen_bit_files_open_to_the_scene_and_the_twin() {
    let json = manifest();
    for (name, entry, scene) in files(&json) {
        if entry["bits"] != 16 {
            continue;
        }
        let scene = Scene(&scene);
        let path = fixture(FAMILY, &name);
        let report = import_report(&path);
        let document = open_typed(&path, &report, LAYERS).expect("the layered open");
        let mut error = ptr::null_mut();
        let info = unsafe { dr_precision_doc_info(document.0, &mut error) };
        assert!(!info.is_null(), "{}", take_err_string(error));
        let info: Value = serde_json::from_str(&take_err_string(info)).unwrap();
        let layers = info["layers"].as_array().unwrap();
        let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
        let wanted = ["bottom", "top", "inner", "set", "masked"].map(|l| scene.name(l));
        assert_eq!(names, wanted, "{name}: the stack, bottom first");
        let depths: Vec<u64> = layers
            .iter()
            .map(|l| l["depth"].as_u64().unwrap())
            .collect();
        assert_eq!(depths, [0, 0, 1, 0, 0], "{name}: Inner inside the group");
        assert_eq!(layers[3]["kind"], "pass_through_group", "{name}");
        let roles = twin_roles(&entry, &scene);
        for (index, k) in [0usize, 1, 2, 4].into_iter().enumerate() {
            let row = &layers[k];
            let twin = roles[k];
            let label = format!("{name} {}", row["name"]);
            assert_eq!(row["kind"], "raster", "{label}");
            assert_eq!(row["opacity"], Value::from(opacity_byte(twin)), "{label}");
            assert_eq!(row["blend_mode"], blend_of(twin).to_c(), "{label}");
            assert_eq!(
                row["visible"],
                twin["flags"].as_u64().unwrap() & 2 == 0,
                "{label}"
            );
            let image = unsafe { dr_precision_doc_layer_source(document.0, k) };
            assert!(!image.is_null(), "{label}: a raster");
            let (w, h) = unsafe { (*image).dimensions() };
            let base = unsafe { dr_precision_image_pixels_rgba16(image) };
            let rgba: Vec<i64> =
                unsafe { std::slice::from_raw_parts(base, w as usize * h as usize * 4) }
                    .iter()
                    .map(|v| i64::from(*v))
                    .collect();
            unsafe { dr_precision_image_free(image) };
            let offset = row["offset"].as_array().unwrap();
            let layer = unsafe { &*document.0 }.layer(k).expect("a layer");
            let mask = layer.mask().map(|mask| {
                let (mw, mh) = mask.plane().dimensions();
                let at = mask.properties().offset;
                assert_eq!(mask.properties().outside_coverage, 0, "{label}");
                assert!(mask.enabled(), "{label}");
                (
                    (i64::from(at.0), i64::from(at.1)),
                    (i64::from(mw), i64::from(mh)),
                    mask.plane()
                        .samples()
                        .iter()
                        .map(|v| i64::from(*v))
                        .collect(),
                )
            });
            let opened = Opened {
                name: label,
                offset: (offset[0].as_i64().unwrap(), offset[1].as_i64().unwrap()),
                size: (i64::from(w), i64::from(h)),
                rgba,
                mask,
            };
            check_layer(&scene, index as i64, &opened, twin, 16);
        }
        assert_eq!(layers[4]["visible"], false, "{name}: Masked is hidden");
        let refused = open_typed(&path, &report, 1)
            .err()
            .expect("no stored composite");
        assert!(refused.contains(NO_COMPOSITE), "{name}: {refused}");
    }
}

#[test]
fn photoshops_rows_of_each_reopened_file_are_the_scenes() {
    let json = manifest();
    for (name, entry, scene) in files(&json) {
        let scene = Scene(&scene);
        let rows = entry["photoshop_rows"]
            .as_array()
            .expect("Photoshop's rows");
        let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
        let wanted = ["masked", "set", "inner", "top", "bottom"].map(|l| scene.name(l));
        assert_eq!(names, wanted, "{name}: top first");
        assert_eq!(rows[0]["visible"], false, "{name}");
        assert_eq!(rows[0]["has_user_mask"], true, "{name}");
        assert_eq!(rows[1]["mode"], "BlendMode.PASSTHROUGH", "{name}");
        assert_eq!(rows[2]["parent"], scene.name("set").as_str(), "{name}");
        // The DOM's percent of the stored byte (45 % is byte 115, 45.098 %).
        assert_eq!(
            rows[3]["opacity"].as_f64().unwrap().round() as i64,
            scene.int("top_opacity"),
            "{name}"
        );
    }
}

// ----------------------------------------------------- the refusals --

fn le16(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// A minimal little-endian layered TIFF in Photoshop's form: a 1 × 1 flat
/// image of `samples` samples at `bits` under `photometric` (`extra` its
/// ExtraSamples, none when empty), `resources` as tag 34377 (none when
/// empty), and tag 37724 holding `info` keyed for the depth, the codes
/// byte-reversed (`MIB8`, `ryaL`, `61rL`).
fn little_file(
    bits: u16,
    photometric: u16,
    samples: u16,
    extra: &[u16],
    resources: &[u8],
    info: &[u8],
) -> Vec<u8> {
    let mut out = b"II*\0".to_vec();
    out.extend([0; 4]);
    let strip_len = u32::from(samples) * u32::from(bits / 8);
    out.extend(std::iter::repeat_n(9u8, strip_len as usize));
    let shorts = |values: &[u16]| -> Vec<u8> { values.iter().flat_map(|v| le16(*v)).collect() };
    let mut block = PREFIX.to_vec();
    block.extend(b"MIB8");
    block.extend(match bits {
        8 => b"ryaL",
        _ => b"61rL",
    });
    block.extend(le32(info.len() as u32));
    block.extend(info);
    while !block.len().is_multiple_of(4) {
        block.push(0);
    }
    let mut fields: Vec<(u16, u16, u32, Vec<u8>)> = vec![
        (256, 4, 1, le32(1).to_vec()),
        (257, 4, 1, le32(1).to_vec()),
        (
            258,
            3,
            u32::from(samples),
            shorts(&vec![bits; usize::from(samples)]),
        ),
        (259, 3, 1, shorts(&[1])),
        (262, 3, 1, shorts(&[photometric])),
        (273, 4, 1, le32(8).to_vec()),
        (277, 3, 1, shorts(&[samples])),
        (278, 4, 1, le32(1).to_vec()),
        (279, 4, 1, le32(strip_len).to_vec()),
    ];
    if !extra.is_empty() {
        fields.push((338, 3, extra.len() as u32, shorts(extra)));
    }
    if !resources.is_empty() {
        fields.push((34377, 7, resources.len() as u32, resources.to_vec()));
    }
    fields.push((37724, 7, block.len() as u32, block));
    let mut slots = Vec::new();
    for (_, _, _, bytes) in &fields {
        if bytes.len() <= 4 {
            let mut slot = bytes.clone();
            slot.resize(4, 0);
            slots.push(slot);
        } else {
            if !out.len().is_multiple_of(2) {
                out.push(0);
            }
            slots.push(le32(out.len() as u32).to_vec());
            out.extend(bytes);
        }
    }
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    let directory = out.len() as u32;
    out.extend(le16(fields.len() as u16));
    for ((tag, kind, count, _), slot) in fields.iter().zip(&slots) {
        out.extend(le16(*tag));
        out.extend(le16(*kind));
        out.extend(le32(*count));
        out.extend(slot);
    }
    out.extend([0; 4]);
    out[4..8].copy_from_slice(&le32(directory));
    out
}

/// One little-endian record named "A" over a 1 × 1 rectangle: `blocks`
/// (little-endian, after its `luni`) and `channels` (id, word, payload).
fn little_info(blocks: &[u8], channels: &[(i16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut info = 1i16.to_le_bytes().to_vec();
    for edge in [0i32, 0, 1, 1] {
        info.extend(edge.to_le_bytes());
    }
    info.extend(le16(channels.len() as u16));
    for (id, _, payload) in channels {
        info.extend(id.to_le_bytes());
        info.extend(le32(2 + payload.len() as u32));
    }
    info.extend(b"MIB8");
    info.extend(b"mron");
    info.extend([255, 0, 8, 0]);
    let mut extra = le32(0).to_vec(); // no mask
    extra.extend(le32(0)); // no blending ranges
    extra.extend([1, b'A', 0, 0]);
    extra.extend(b"MIB8inul");
    extra.extend(le32(8));
    extra.extend(le32(1));
    extra.extend([b'A', 0, 0, 0]);
    extra.extend(blocks);
    info.extend(le32(extra.len() as u32));
    info.extend(extra);
    for (_, word, payload) in channels {
        info.extend(le16(*word));
        info.extend(payload);
    }
    info
}

/// Channels -1, 0, 1 and 2 of a 1 × 1 plane, each Raw at `bits` (one
/// sample, big-endian at 16 as Photoshop writes them).
fn raw_channels(bits: u16) -> Vec<(i16, u16, Vec<u8>)> {
    [-1i16, 0, 1, 2]
        .into_iter()
        .map(|id| {
            (
                id,
                0,
                if bits == 8 {
                    vec![200]
                } else {
                    vec![0xc8, 0x37]
                },
            )
        })
        .collect()
}

/// A little-endian layer metadata block (`shmd`) holding one item keyed
/// `key` (reversed, as the file stores it) with `data`.
fn shmd(key: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut body = le32(1).to_vec();
    body.extend(b"MIB8");
    body.extend(key.iter().rev());
    body.extend([1, 0, 0, 0]);
    body.extend(le32(data.len() as u32));
    body.extend(data);
    let mut out = b"MIB8dmhs".to_vec();
    out.extend(le32(body.len() as u32));
    out.extend(body);
    out
}

/// One image resource as a PSD's resource section lays it out (always
/// big-endian, in either byte order of TIFF).
fn resource(id: u16, data: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend(id.to_be_bytes());
    out.extend([0, 0]);
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(data);
    if !data.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

/// Resource 1065 (the layer comps) of Photoshop's own
/// `layer_comps/source.psd`, read from its resource section.
fn photoshops_comps() -> Vec<u8> {
    let bytes = std::fs::read(fixture("layer_comps", "source.psd")).unwrap();
    let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let colour = be32(26);
    let start = 30 + colour + 4;
    let end = start + be32(30 + colour);
    let mut at = start;
    while at < end {
        assert_eq!(&bytes[at..at + 4], b"8BIM");
        let id = u16::from_be_bytes([bytes[at + 4], bytes[at + 5]]);
        let name = (1 + usize::from(bytes[at + 6])).next_multiple_of(2);
        let size = be32(at + 6 + name);
        let data = at + 10 + name;
        if id == 1065 {
            return bytes[data..data + size].to_vec();
        }
        at = data + size.next_multiple_of(2);
    }
    panic!("source.psd holds no resource 1065");
}

fn write(directory: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn assert_refused(directory: &TempDir, name: &str, bytes: &[u8], thing: &str) {
    let path = write(directory, name, bytes);
    assert!(
        unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) },
        "{name}: offered the layered open"
    );
    let error = report(&path).expect_err("refused");
    assert!(
        error.ends_with(&format!(
            "a little-endian (IBM PC) layered TIFF's {thing} {READ_TAIL}"
        )),
        "{name}: {error}"
    );
}

#[test]
fn a_little_endian_file_refuses_what_its_walk_does_not_read_by_name() {
    let directory = TempDir::new().unwrap();
    // The builders' own positive cases: the plain record reads at both depths.
    for (bits, precision) in [(8, "rgba8"), (16, "rgba16")] {
        let plain = write(
            &directory,
            &format!("plain{bits}.tif"),
            &little_file(
                bits,
                2,
                4,
                &[2],
                &[],
                &little_info(&[], &raw_channels(bits)),
            ),
        );
        let plain = report(&plain).expect("a plain little-endian record reads");
        assert_eq!(plain["import_precision"], precision, "{plain}");
        assert_eq!(plain["layered_available"], true, "{plain}");
    }

    // A layer style (`lfx2`, a descriptor): not walked in little-endian.
    let mut lfx2 = b"MIB82xfl".to_vec();
    lfx2.extend(le32(4));
    lfx2.extend([0; 4]);
    assert_refused(
        &directory,
        "lfx2.tif",
        &little_file(8, 2, 4, &[2], &[], &little_info(&lfx2, &raw_channels(8))),
        "“lfx2” record",
    );

    // The 16-bit words Photoshop did not write little-endian: their sample
    // order is unmeasured.
    for (word, thing) in [(1u16, "RLE 16-bit channel"), (2, "ZIP 16-bit channel")] {
        let mut channels = raw_channels(16);
        channels[1] = (0, word, vec![2, 0, 1, 0x12, 0x34]);
        assert_refused(
            &directory,
            &format!("word{word}.tif"),
            &little_file(16, 2, 4, &[2], &[], &little_info(&[], &channels)),
            thing,
        );
    }

    // Saved channels: the flat image's fifth sample described by a
    // DisplayInfo (1077), which the big-endian form has no strip to keep.
    let mut display = 1u32.to_be_bytes().to_vec();
    display.extend([0, 0, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 50, 2, 0]);
    assert_refused(
        &directory,
        "spot.tif",
        &little_file(
            8,
            2,
            5,
            &[2, 0],
            &resource(1077, &display),
            &little_info(&[], &raw_channels(8)),
        ),
        "spot or alpha channel data",
    );

    // CMYK and gray little-endian files are not read as layers; their
    // blocks still hold a layer.
    for (photometric, samples, name) in [(5u16, 4u16, "cmyk"), (1, 1, "gray")] {
        let path = write(
            &directory,
            &format!("{name}.tif"),
            &little_file(
                8,
                photometric,
                samples,
                &[],
                &[],
                &little_info(&[], &raw_channels(8)),
            ),
        );
        let path = cpath(&path);
        assert!(!unsafe { dr_tiff_has_layers(path.as_ptr()) }, "{name}");
        assert!(unsafe { dr_tiff_has_layer_block(path.as_ptr()) }, "{name}");
    }
}

#[test]
fn a_little_endian_records_metadata_reads_only_where_it_holds_comp_settings() {
    let directory = TempDir::new().unwrap();
    let comps = resource(1065, &photoshops_comps());
    let unread = "layer comp settings that could not be read";
    // A `cust` item (every plain Photoshop layer's): its head re-laid, it is
    // no comp setting, so nothing is lost.
    let path = write(
        &directory,
        "cust.tif",
        &little_file(
            8,
            2,
            4,
            &[2],
            &comps,
            &little_info(&shmd(b"cust", &[0; 4]), &raw_channels(8)),
        ),
    );
    let read = report(&path).expect("the record reads");
    assert_eq!(read["layered_available"], true, "{read}");
    assert!(
        !read["verified_losses"].to_string().contains(unread),
        "{read}"
    );

    // A `cmls` item: its little-endian descriptor does not decode, and the
    // report says so as for any record's unreadable comp settings.
    let mut settings = le32(16).to_vec();
    settings.extend([0; 12]);
    let path = write(
        &directory,
        "cmls.tif",
        &little_file(
            8,
            2,
            4,
            &[2],
            &comps,
            &little_info(&shmd(b"cmls", &settings), &raw_channels(8)),
        ),
    );
    let read = report(&path).expect("the record reads");
    assert_eq!(read["layered_available"], true, "{read}");
    assert!(
        read["verified_losses"]
            .to_string()
            .contains(&format!("1 layer record carries {unread}")),
        "{read}"
    );
}
