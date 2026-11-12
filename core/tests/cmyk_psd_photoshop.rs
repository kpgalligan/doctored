//! Photoshop-authored layered CMYK files (#1 phase 3): take1 of
//! `scripts/photoshop-probe/cmyk_files_take.py`
//! (`~/devel/doctored-fixtures/photoshop/cmyk-psd-547/take1`, Photoshop
//! 26.10.0), whose docstring fixed every bound below before the take. SKIPS
//! with an eprintln when the take is absent.
//!
//! Per file: the manifest's structure EXACTLY (Photoshop's own readback:
//! names, order and depth, kinds, Pass Through, clipping, modes, opacity and
//! Fill bytes, visibility, mask presence and enabled, and the Shape's vector
//! mask); every imported raster and mask plane equal to the independent
//! reader's, and the Shape's fill within one 8-bit code of its stored ink
//! (`common/cmyk_psd_fixture.rs`); the authored region inks exactly at 8 bits
//! and within 1 code of `code × 257` at 16; the `layers*` composite within 1
//! code per plate (8-bit) of Photoshop's stored composite at every cell's
//! centre 2 × 2; the adjustment parameters against Photoshop's descriptors;
//! the `unsupported8` and Brightness/Contrast report lines by name; and the
//! flat TIFFs (associated alpha, planar, an extra sample) against the
//! independent TIFF reader.

use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::cmyk_image::CmykSample;
use doctored_core::doc::BlendMode;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_info, dr_precision_doc_open,
};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use serde_json::Value;

use crate::cmyk_psd_fixture::{read, read_flat_tiff, ReadFile};
use crate::cmyk_psd_import::{open_psd, report};
use crate::cmyk_structure_fixture::composite;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn take() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var("HOME").ok()?)
        .join("devel/doctored-fixtures/photoshop/cmyk-psd-547/take1");
    if root.join("manifest.json").is_file() {
        Some(root)
    } else {
        eprintln!("SKIP: the Photoshop take {} is absent", root.display());
        None
    }
}

fn manifest(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join("manifest.json")).unwrap()).unwrap()
}

fn mode_of(constant: &str) -> Option<BlendMode> {
    Some(match constant {
        "BlendMode.NORMAL" => BlendMode::Normal,
        "BlendMode.DISSOLVE" => BlendMode::Dissolve,
        "BlendMode.DARKEN" => BlendMode::Darken,
        "BlendMode.MULTIPLY" => BlendMode::Multiply,
        "BlendMode.COLORBURN" => BlendMode::ColorBurn,
        "BlendMode.LINEARBURN" => BlendMode::LinearBurn,
        "BlendMode.DARKERCOLOR" => BlendMode::DarkerColor,
        "BlendMode.LIGHTEN" => BlendMode::Lighten,
        "BlendMode.SCREEN" => BlendMode::Screen,
        "BlendMode.COLORDODGE" => BlendMode::ColorDodge,
        "BlendMode.LINEARDODGE" => BlendMode::Addition,
        "BlendMode.LIGHTERCOLOR" => BlendMode::LighterColor,
        "BlendMode.OVERLAY" => BlendMode::Overlay,
        "BlendMode.SOFTLIGHT" => BlendMode::SoftLight,
        "BlendMode.HARDLIGHT" => BlendMode::HardLight,
        "BlendMode.VIVIDLIGHT" => BlendMode::VividLight,
        "BlendMode.LINEARLIGHT" => BlendMode::LinearLight,
        "BlendMode.PINLIGHT" => BlendMode::PinLight,
        "BlendMode.HARDMIX" => BlendMode::HardMix,
        "BlendMode.DIFFERENCE" => BlendMode::Difference,
        "BlendMode.EXCLUSION" => BlendMode::Exclusion,
        "BlendMode.SUBTRACT" => BlendMode::Subtract,
        "BlendMode.DIVIDE" => BlendMode::Divide,
        "BlendMode.HUE" => BlendMode::Hue,
        "BlendMode.SATURATION" => BlendMode::Saturation,
        "BlendMode.COLORBLEND" => BlendMode::Color,
        "BlendMode.LUMINOSITY" => BlendMode::Luminosity,
        "BlendMode.PASSTHROUGH" => BlendMode::PassThrough,
        _ => return None,
    })
}

/// The document's records top-first with their depth — Photoshop's row
/// order.
fn rows<'a>(
    layers: &'a [PrecisionDocumentLayer],
    depth: u64,
    out: &mut Vec<(u64, &'a PrecisionDocumentLayer)>,
) {
    for layer in layers.iter().rev() {
        out.push((depth, layer));
        if let Some(children) = layer.children() {
            rows(children, depth + 1, out);
        }
    }
}

/// `layer`'s postorder index in `doc`.
fn index_of(doc: &PrecisionDocument, layer: &PrecisionDocumentLayer) -> usize {
    (0..doc.layer_count())
        .find(|&i| std::ptr::eq(doc.layer(i).unwrap(), layer))
        .unwrap()
}

fn kind_of(doc: &PrecisionDocument, layer: &PrecisionDocumentLayer) -> &'static str {
    if layer.children().is_some() {
        return "LayerKind.LAYERSET";
    }
    if layer.cmyk_source().is_some() {
        return "raster";
    }
    let index = index_of(doc, layer);
    let description: Value =
        serde_json::from_str(doc.adjustment_description(index).unwrap()).unwrap();
    match description["op"].as_str().unwrap() {
        "levels" => "LayerKind.LEVELS",
        "curves" => "LayerKind.CURVES",
        "hue_saturation" => "LayerKind.HUESATURATION",
        // Carried since the styles batch (#1).
        "bcs" => "LayerKind.BRIGHTNESSCONTRAST",
        other => panic!("unexpected op {other}"),
    }
}

fn byte(v: f32) -> u64 {
    (f64::from(v) * 255.0).round() as u64
}

/// The manifest's rows the import keeps — every row but an omitted
/// Brightness/Contrast record — against the opened tree, exactly.
fn assert_structure(file: &str, entry: &Value, doc: &PrecisionDocument) {
    let expected: Vec<&Value> = entry["rows"].as_array().unwrap().iter().collect();
    let mut got = Vec::new();
    rows(doc.root_layers(), 0, &mut got);
    assert_eq!(
        got.iter().map(|(_, l)| l.name()).collect::<Vec<_>>(),
        expected
            .iter()
            .map(|r| r["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        "{file}: names and order"
    );
    for ((depth, layer), row) in got.iter().zip(&expected) {
        let what = format!("{file}: {}", layer.name());
        assert_eq!(*depth, row["depth"].as_u64().unwrap(), "{what}: depth");
        let kind = row["kind"].as_str().unwrap();
        let want = match kind {
            "LayerKind.LAYERSET"
            | "LayerKind.LEVELS"
            | "LayerKind.CURVES"
            | "LayerKind.HUESATURATION"
            | "LayerKind.BRIGHTNESSCONTRAST" => kind,
            // Type and smart-object records import as their pixels, and the
            // Shape as its Solid Color fill (a raster and its description).
            "LayerKind.NORMAL"
            | "LayerKind.TEXT"
            | "LayerKind.SOLIDFILL"
            | "LayerKind.SMARTOBJECT" => "raster",
            other => panic!("{what}: unexpected kind {other}"),
        };
        assert_eq!(kind_of(doc, layer), want, "{what}: kind");
        let mode = mode_of(row["blend"].as_str().unwrap()).unwrap();
        assert_eq!(layer.blend_mode(), mode, "{what}: mode");
        assert_eq!(
            layer.is_pass_through(),
            mode == BlendMode::PassThrough,
            "{what}"
        );
        assert_eq!(
            byte(layer.opacity()),
            row["opacity_255"].as_u64().unwrap(),
            "{what}: opacity"
        );
        let fill = layer.style().map_or(1.0, |s| s.fill_opacity);
        assert_eq!(
            byte(fill),
            row["fill_opacity_255"].as_u64().unwrap_or(255),
            "{what}: Fill"
        );
        assert_eq!(
            layer.visible(),
            row["visible"].as_bool().unwrap(),
            "{what}: visible"
        );
        assert_eq!(
            layer.clipped(),
            row["clipped"].as_bool().unwrap_or(false),
            "{what}: clipped"
        );
        // A Solid Color fill record under its `vmsk` (the Shape) imports as
        // the fill under its vector mask, whose bake holds the mask slot.
        let vector = doc.vector_mask(index_of(doc, layer)).is_some();
        assert_eq!(vector, kind == "LayerKind.SOLIDFILL", "{what}: vector mask");
        assert_eq!(
            layer.mask().is_some() && !vector,
            row["has_user_mask"].as_bool().unwrap_or(false),
            "{what}: mask"
        );
        if let Some(mask) = layer.mask().filter(|_| !vector) {
            assert_eq!(
                mask.enabled(),
                row["user_mask_enabled"].as_bool().unwrap(),
                "{what}: mask enabled"
            );
        }
    }
}

/// Every raster and mask plane against the independent reader's.
fn assert_planes(file: &str, doc: &PrecisionDocument, read: &ReadFile) {
    let mut got = Vec::new();
    rows(doc.root_layers(), 0, &mut got);
    for (_, layer) in got {
        let record = read.by_name(layer.name());
        let what = format!("{file}: {}", layer.name());
        // The Shape: Photoshop stores its fill already masked over the
        // record's rectangle, and the import renders the fill over the canvas
        // under its vector mask, so the two inks agree inside the shape,
        // within one 8-bit code.
        if doc.vector_mask(index_of(doc, layer)).is_some() {
            let [top, left, bottom, right] = record.bounds;
            let (x, y) = ((left + right) / 2, (top + bottom) / 2);
            let width = (right - left) as usize;
            let stored = record.ink(read.depth)[(y - top) as usize * width + (x - left) as usize];
            let got = sample(layer, x, y);
            for c in 0..5 {
                let diff = (i64::from(got[c]) - i64::from(stored[c])).abs();
                assert!(diff <= 257, "{what}: plate {c} {} vs {}", got[c], stored[c]);
            }
            continue;
        }
        if let Some(image) = layer.cmyk_source() {
            assert_eq!(
                layer.offset(),
                (record.bounds[1], record.bounds[0]),
                "{what}: offset"
            );
            assert_eq!(
                image.to_contiguous().unwrap(),
                record.ink(read.depth),
                "{what}: ink"
            );
        }
        if let Some(mask) = layer.mask() {
            let body = record.mask.as_ref().expect("a mask body");
            assert_eq!(
                mask.properties().offset,
                (body.bounds[1], body.bounds[0]),
                "{what}: mask offset"
            );
            assert_eq!(
                mask.properties().outside_coverage,
                body.outside,
                "{what}: mask outside"
            );
            assert_eq!(
                Some(mask.plane().samples().to_vec()),
                record.mask16(read.depth),
                "{what}: mask"
            );
        }
    }
}

/// The layer's sample at canvas `(x, y)`.
fn sample(layer: &PrecisionDocumentLayer, x: i32, y: i32) -> CmykSample {
    let (ox, oy) = layer.offset();
    layer
        .cmyk_source()
        .unwrap()
        .sample_at((x - ox) as u32, (y - oy) as u32)
        .unwrap()
}

fn assert_ink(got: CmykSample, code: &Value, depth: u16, what: &str) {
    for c in 0..4 {
        let want = i64::try_from(code[c].as_u64().unwrap() * 257).unwrap();
        let diff = (i64::from(got[c]) - want).abs();
        let bound = if depth == 8 { 0 } else { 1 };
        assert!(diff <= bound, "{what}: plate {c} {} vs {want}", got[c]);
    }
}

fn find<'a>(doc: &'a PrecisionDocument, name: &str) -> &'a PrecisionDocumentLayer {
    let mut all = Vec::new();
    rows(doc.root_layers(), 0, &mut all);
    all.into_iter()
        .find(|(_, l)| l.name() == name)
        .unwrap_or_else(|| panic!("no layer {name}"))
        .1
}

/// The authored inks at every region's centre.
fn assert_regions(file: &str, entry: &Value, doc: &PrecisionDocument, depth: u16) {
    let regions = &entry["regions"];
    let base = find(doc, "Base");
    for cell in regions["base"].as_array().unwrap() {
        let (c, r) = (
            cell["cell"][0].as_i64().unwrap() as i32,
            cell["cell"][1].as_i64().unwrap() as i32,
        );
        assert_ink(
            sample(base, c * 8 + 4, r * 8 + 4),
            &cell["code"],
            depth,
            &format!("{file}: base cell {c},{r}"),
        );
    }
    for fill in regions["fills"].as_array().unwrap() {
        let layer = find(doc, fill["layer"].as_str().unwrap());
        let rect: Vec<i32> = fill["rect"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        let (x, y) = (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2);
        let got = sample(layer, x, y);
        assert_eq!(
            got[4],
            65535,
            "{file}: {} at ({x}, {y}) opaque",
            layer.name()
        );
        assert_ink(
            got,
            &fill["code"],
            depth,
            &format!("{file}: {} at ({x}, {y})", layer.name()),
        );
    }
}

/// The composite at every cell's centre 2 × 2 against Photoshop's stored one.
fn assert_composite(file: &str, doc: *const PrecisionDocument, read: &ReadFile) {
    let width = read.width as usize;
    let max: i64 = if read.depth == 8 { 255 } else { 65535 };
    let to8 = |v: i64| if read.depth == 8 { v } else { (v + 128) / 257 };
    for cy in 0..(read.height / 8) as usize {
        for cx in 0..(read.width / 8) as usize {
            for (dx, dy) in [(3, 3), (4, 3), (3, 4), (4, 4)] {
                let (x, y) = (cx * 8 + dx, cy * 8 + dy);
                let ours = composite(doc, x as i32, y as i32);
                assert_eq!(ours[4], 65535, "{file}: opaque at ({x}, {y})");
                for (c, ink) in ours.iter().take(4).enumerate() {
                    let stored = i64::from(read.composite[c][y * width + x]);
                    // A PSD stores `max − ink`; a TIFF's strip is ink.
                    let theirs = to8(if read.tiff { stored } else { max - stored });
                    let mine = (i64::from(*ink) + 128) / 257;
                    assert!(
                        (mine - theirs).abs() <= 1,
                        "{file}: plate {c} at ({x}, {y}): {mine} vs Photoshop {theirs}"
                    );
                }
            }
        }
    }
}

fn has(list: &Value, text: &str) -> bool {
    list.as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str().unwrap().contains(text))
}

fn approx(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 1e-9, "{what}: {a} vs {b}");
}

/// The Levels, Curves and Hue/Saturation records against Photoshop's own
/// descriptors (their entries in authoring order: composite, Cyan, Black).
fn assert_adjustments(entry: &Value, doc: &PrecisionDocument) {
    let row = |name: &str| -> Value {
        entry["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()["adjustment"][0]["value"]
            .clone()
    };
    let description = |name: &str| -> Value {
        let index = (0..doc.layer_count())
            .find(|&i| doc.layer(i).unwrap().name() == name)
            .unwrap();
        serde_json::from_str(doc.adjustment_description(index).unwrap()).unwrap()
    };
    let levels = description("Levels");
    let entries = row("Levels")["adjustment"].as_array().unwrap().clone();
    for (entry, key) in entries.iter().zip(["", "c", "k"]) {
        let e = &entry["value"];
        let stage = if key.is_empty() {
            &levels["params"]
        } else {
            &levels["params"][key]
        };
        let code = |v: &Value| (v.as_f64().unwrap() * 255.0).round() as u64;
        assert_eq!(
            code(&stage["black"]),
            e["input"][0].as_u64().unwrap(),
            "levels {key} black"
        );
        assert_eq!(
            code(&stage["white"]),
            e["input"][1].as_u64().unwrap(),
            "levels {key} white"
        );
        approx(
            stage["gamma"].as_f64().unwrap(),
            e["gamma"].as_f64().unwrap(),
            "levels gamma",
        );
        let out = |k: &str, default: u64| stage.get(k).map_or(default, &code);
        assert_eq!(
            out("output_black", 0),
            e["output"][0].as_u64().unwrap(),
            "levels {key} out black"
        );
        assert_eq!(
            out("output_white", 255),
            e["output"][1].as_u64().unwrap(),
            "levels {key} out white"
        );
    }
    let curves = description("Curves");
    let entries = row("Curves")["adjustment"].as_array().unwrap().clone();
    for (entry, key) in entries.iter().zip(["cmyk", "c", "k"]) {
        // Photoshop's (horizontal, vertical) in the stored `L` domain,
        // reflected onto ink and ascending again.
        let mut want: Vec<Value> = entry["value"]["curve"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let (h, v) = (
                    p["value"]["horizontal"].as_u64().unwrap(),
                    p["value"]["vertical"].as_u64().unwrap(),
                );
                serde_json::json!([255 - h, 255 - v])
            })
            .collect();
        want.reverse();
        assert_eq!(curves["params"][key], Value::Array(want), "curves {key}");
    }
    let hue = description("Hue/Sat");
    let entries = row("Hue/Sat")["adjustment"].as_array().unwrap().clone();
    let master = &entries[0]["value"];
    approx(
        hue["params"]["hue"].as_f64().unwrap(),
        master["hue"].as_f64().unwrap(),
        "hue",
    );
    approx(
        hue["params"]["saturation"].as_f64().unwrap(),
        master["saturation"].as_f64().unwrap() / 100.0,
        "saturation",
    );
    let band = &entries[1]["value"];
    assert_eq!(band["localRange"], 1, "the Reds band");
    let reds = &hue["params"]["bands"]["reds"];
    let arc = |x: f64, y: f64| (y - x).rem_euclid(360.0);
    let (a, b, c, d) = (
        band["beginRamp"].as_f64().unwrap(),
        band["beginSustain"].as_f64().unwrap(),
        band["endSustain"].as_f64().unwrap(),
        band["endRamp"].as_f64().unwrap(),
    );
    approx(
        reds["inner"].as_f64().unwrap(),
        arc(b, c) / 2.0,
        "reds inner",
    );
    approx(
        reds["center"].as_f64().unwrap(),
        (b + arc(b, c) / 2.0).rem_euclid(360.0),
        "reds centre",
    );
    approx(
        reds["falloff"].as_f64().unwrap(),
        (arc(a, b) + arc(c, d)) / 2.0,
        "reds falloff",
    );
    approx(
        reds["hue"].as_f64().unwrap(),
        band["hue"].as_f64().unwrap(),
        "reds hue",
    );
    approx(
        reds["saturation"].as_f64().unwrap(),
        band["saturation"].as_f64().unwrap() / 100.0,
        "reds saturation",
    );
}

#[test]
fn photoshops_layered_cmyk_files_import_exactly() {
    let Some(root) = take() else { return };
    let manifest = manifest(&root);
    for file in [
        "layers8.psd",
        "layers8.tif",
        "layers16.psd",
        "layers16.psb",
        "modes8.psd",
        "adjust16.psd",
        "unsupported8.psd",
    ] {
        let entry = &manifest["files"][file];
        let path = root.join(entry["path"].as_str().unwrap());
        let bytes = std::fs::read(&path).unwrap();
        let independent = read(&bytes);
        let depth = independent.depth;
        assert_eq!(u64::from(depth), entry["bits"].as_u64().unwrap(), "{file}");
        let r = report(&path);
        assert_eq!(r["import_precision"], "cmyk", "{file}");
        assert_eq!(
            r["layered_available"], true,
            "{file}: {}",
            r["layered_refusal"]
        );
        assert_eq!(r["profile"], "U.S. Web Coated (SWOP) v2", "{file}");
        let doc = open_psd(&path, 0).unwrap_or_else(|e| panic!("{file}: {e}"));
        let document = unsafe { &*doc };
        assert_structure(file, entry, document);
        assert_planes(file, document, &independent);
        if file != "adjust16.psd" {
            assert_regions(file, entry, document, depth);
        }
        if file.starts_with("layers") {
            assert_composite(file, doc, &independent);
        }
        match file {
            "adjust16.psd" => {
                assert_adjustments(entry, document);
                assert!(has(&r["observations"], "(“Bright/Contrast”) is a Photoshop Brightness/Contrast adjustment layer (brit); layered import carries it as a Doctored bcs adjustment layer"));
            }
            "unsupported8.psd" => {
                let losses = &r["verified_losses"];
                for line in [
                    "(“Type”) has text/type metadata; layered import discards these records and does not retain editable text.",
                    "(“Shape”) has vector stroke/origination metadata",
                    "(“Smart”) has placed/smart-object or linked-layer metadata; layered import discards these records",
                    "2 colour samplers are not imported into a CMYK document.",
                ] {
                    assert!(has(losses, line), "{file}: no “{line}” in {losses}");
                }
                // The Shape's `vmsk` is carried as its vector mask, so no
                // mask is dropped.
                assert!(
                    !has(losses, "(“Shape”) has a raster mask channel"),
                    "{file}: {losses}"
                );
                // The drop shadow's CMYK colour CARRIES since the styles
                // batch (#1), the gray Blend If since the Blend If batch.
                let observations = &r["observations"];
                for line in [
                    "(“Shadow”) has Photoshop layer effects (drop_shadow",
                    "and Blend If ranges on gray); layered import carries them",
                ] {
                    assert!(
                        has(observations, line),
                        "{file}: no “{line}” in {observations}"
                    );
                }
                assert!(!has(losses, "Blend If"), "{file}: {losses}");
            }
            _ => {}
        }
        unsafe { dr_precision_doc_free(doc) };
    }
}

fn open_flat(path: &Path) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_open(cpath(path).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(
        !doc.is_null(),
        "{}: {}",
        path.display(),
        take_err_string(err)
    );
    doc
}

fn warnings(doc: *const PrecisionDocument) -> Vec<String> {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_info(doc, &mut err) };
    let info: Value = serde_json::from_str(&take_err_string(text)).unwrap();
    info["import_warnings"]
        .as_array()
        .map(|w| w.iter().map(|v| v.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

#[test]
fn photoshops_cmyk_tiffs_open_with_their_transparency_and_planes() {
    let Some(root) = take() else { return };
    let manifest = manifest(&root);
    for file in ["alpha8.tif", "alpha16.tif", "planar16.tif", "extra8.tif"] {
        let entry = &manifest["files"][file];
        let path = root.join(entry["path"].as_str().unwrap());
        let tiff = read_flat_tiff(&std::fs::read(&path).unwrap());
        let doc = open_flat(&path);
        let document = unsafe { &*doc };
        assert!(document.is_cmyk(), "{file}");
        let got = document.cmyk_layer_samples(0).unwrap();
        let max: u32 = if tiff.depth == 8 { 255 } else { 65535 };
        let scale: u32 = if tiff.depth == 8 { 257 } else { 1 };
        let alpha = match tiff.extra_samples.first() {
            Some(1 | 2) => Some(tiff.extra_samples[0]),
            _ => None,
        };
        for (i, sample) in got.iter().enumerate() {
            let a = alpha.map_or(max, |_| u32::from(tiff.planes[4][i]));
            let mut want = [0u16; 5];
            for (c, slot) in want.iter_mut().take(4).enumerate() {
                let v = u32::from(tiff.planes[c][i]);
                // Associated (premultiplied) ink unpremultiplied at the
                // file's depth; alpha 0 is paper.
                let straight = match alpha {
                    Some(1) if a == 0 => 0,
                    Some(1) => {
                        ((f64::from(v) * f64::from(max) / f64::from(a)).round() as u32).min(max)
                    }
                    _ => v,
                };
                *slot = (straight * scale) as u16;
            }
            want[4] = (a * scale) as u16;
            assert_eq!(*sample, want, "{file}: pixel {i}");
        }
        let extra = tiff.planes.len() - 4 - usize::from(alpha.is_some());
        let sentence = format!("This CMYK TIFF has {extra} extra samples (spot or alpha channels) beyond its transparency; they are not read.");
        assert_eq!(
            warnings(doc).contains(&sentence),
            extra > 0,
            "{file}: {:?}",
            warnings(doc)
        );
        // The opaque bases' cells (the alpha files' base was cleared).
        if alpha.is_none() {
            for cell in entry["regions"]["base"].as_array().unwrap() {
                let (c, r) = (
                    cell["cell"][0].as_u64().unwrap() as u32,
                    cell["cell"][1].as_u64().unwrap() as u32,
                );
                let at = got[((r * 8 + 4) * tiff.width + c * 8 + 4) as usize];
                assert_ink(
                    at,
                    &cell["code"],
                    tiff.depth,
                    &format!("{file}: base cell {c},{r}"),
                );
            }
        }
        for fill in entry["regions"]["fills"].as_array().unwrap() {
            let rect: Vec<u32> = fill["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            let (x, y) = (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2);
            let at = got[(y * tiff.width + x) as usize];
            assert_ink(
                at,
                &fill["code"],
                tiff.depth,
                &format!("{file} at ({x}, {y})"),
            );
        }
        unsafe { dr_precision_doc_free(doc) };
    }
}
