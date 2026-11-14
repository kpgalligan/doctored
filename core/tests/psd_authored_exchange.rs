//! PSD exchange against the Photoshop-authored `authored_exchange` family
//! (`provenance.json` beside it), through the FFI: layer comps
//! (`psd_comps`, `psd_comp_records`) and vector masks on raster layers
//! (`psd_paths`, `psd_raster_masks`).
//!
//! Oracle: `photoshop.json`, Photoshop's own reading of each file: the
//! comps' names, comments and capture flags, and the state of every layer
//! when the file is opened and after applying each comp in file order (the
//! step table); and, for the vector masks, the file's stored composite,
//! Photoshop's own render.
//!
//! - Every comps file opens with Photoshop's comps, comments and flags, and
//!   applying them in order gives Photoshop's visibility, opacity, fill, mode,
//!   drop shadow and pixel-layer origin at every step, at 8 and 16 bits in
//!   RGB, Grayscale and Lab, and on the held-out documents. (The shape layer
//!   SH, which Photoshop moves by its vector mask's offset, is not compared:
//!   no PSD comp's position is imported.)
//! - Resource 1065 with a comment decodes and re-encodes to Photoshop's bytes.
//! - A document's comps (visibility, position, appearance) round-trip
//!   through PSD at 8 and 16 bits with their names, comments, flags,
//!   visibility and appearance; this file's walker reads each comp's
//!   `capturedInfo` and each entry's recorded offset, which the import does
//!   not keep.
//! - Every raster layer under a vector mask composites as the file's stored
//!   composite does: a plain vector mask and one beside a user mask are kept
//!   as vector masks (the second with its raster part), and the ones whose
//!   subpaths subtract, intersect or exclude become the raster mask the file
//!   rendered, with a sentence.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{dr_precision_doc_open_gray_psd, dr_precision_doc_open_lab_psd};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use crate::psd_comps::{lyr_i, record_blocks, resource_body};
use crate::psd_paths::{byte_open, byte_save, typed_open, typed_save};
use common::psd_photoshop::{fixture, import_report, open_psd, strings, COMPOSITE, LAYERS};
use common::{cpath, doc_from, solid, take_err_string};

const FAMILY: &str = "authored_exchange";

fn family(name: &str) -> PathBuf {
    fixture(FAMILY, name)
}

fn take() -> Value {
    serde_json::from_str(&std::fs::read_to_string(family("photoshop.json")).unwrap()).unwrap()
}

/// One layer's state as the step table records it.
#[derive(Debug, PartialEq)]
struct Row {
    visible: bool,
    opacity: i64,
    fill: Option<i64>,
    mode: String,
    shadow: Option<f64>,
    /// A pixel layer's origin (a group's: none).
    origin: Option<(i64, i64)>,
}

/// The drop shadow's distance of a style JSON, when it has an enabled one.
fn shadow_of(style: Option<&str>) -> Option<f64> {
    let style: Value = serde_json::from_str(style?).ok()?;
    style["effects"]
        .as_array()?
        .iter()
        .find(|e| e["type"] == "drop_shadow" && e["enabled"] != false)?["distance"]
        .as_f64()
}

fn fill_of(style: Option<&str>) -> i64 {
    style
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|s| s["fill_opacity"].as_f64())
        .map_or(100, |f| (f * 100.0).round() as i64)
}

/// The document's layers as `(name, is group, row)`, either depth.
enum Doc {
    Byte(*mut DrDocument),
    Typed(Box<PrecisionDocument>),
}

impl Doc {
    fn rows(&self) -> Vec<(String, Row)> {
        match self {
            Doc::Byte(doc) => unsafe { &**doc }
                .layers
                .iter()
                .map(|l| {
                    let style = l.style.as_ref().map(|s| s.to_json());
                    let group = l.kind == doctored_core::doc::LayerKind::Group;
                    (
                        l.name.clone(),
                        Row {
                            visible: l.visible,
                            opacity: (f64::from(l.opacity) * 100.0).round() as i64,
                            fill: (!group).then(|| fill_of(style.as_deref())),
                            mode: format!("{:?}", l.blend).to_uppercase(),
                            shadow: shadow_of(style.as_deref()),
                            origin: (!group)
                                .then(|| (i64::from(l.offset.0), i64::from(l.offset.1))),
                        },
                    )
                })
                .collect(),
            Doc::Typed(doc) => (0..doc.layer_count())
                .filter_map(|i| doc.layer(i))
                .map(|l| {
                    let style = l.reported_style();
                    (
                        l.name().to_string(),
                        Row {
                            visible: l.visible(),
                            opacity: (f64::from(l.opacity()) * 100.0).round() as i64,
                            fill: l.children().is_none().then(|| fill_of(style.as_deref())),
                            mode: format!("{:?}", l.blend_mode()).to_uppercase(),
                            shadow: shadow_of(style.as_deref()),
                            origin: l.children().is_none().then(|| {
                                let (x, y) = l.offset();
                                (i64::from(x), i64::from(y))
                            }),
                        },
                    )
                })
                .collect(),
        }
    }

    fn comps(&self) -> Value {
        let mut err: *mut c_char = ptr::null_mut();
        let raw = match self {
            Doc::Byte(doc) => unsafe {
                doctored_core::ffi_comps::dr_doc_layer_comps_json(*doc, &mut err)
            },
            Doc::Typed(doc) => unsafe {
                doctored_core::ffi_comps::dr_precision_doc_layer_comps_json(&**doc, &mut err)
            },
        };
        assert!(!raw.is_null(), "{}", take_err_string(err));
        serde_json::from_str(&take_err_string(raw)).unwrap()
    }

    /// Applies comp `comp`; an exact no-op (NULL with no error) keeps the
    /// document.
    fn apply(&mut self, comp: usize) {
        let request = json!({"op": "layer_comp", "action": "apply", "comp": comp});
        let text = CString::new(request.to_string()).unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        match self {
            Doc::Byte(doc) => {
                let out = unsafe { dr_doc_edit_layer_comps(*doc, text.as_ptr(), &mut err) };
                let message = (!err.is_null()).then(|| take_err_string(err));
                assert!(message.is_none(), "apply {comp}: {message:?}");
                if !out.is_null() {
                    unsafe { dr_doc_free(*doc) };
                    *doc = out;
                }
            }
            Doc::Typed(doc) => {
                let out = unsafe {
                    dr_precision_doc_edit(
                        &**doc,
                        text.as_ptr(),
                        ptr::null(),
                        0,
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                };
                let message = (!err.is_null()).then(|| take_err_string(err));
                assert!(message.is_none(), "apply {comp}: {message:?}");
                if !out.is_null() {
                    *doc = unsafe { Box::from_raw(out) };
                }
            }
        }
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        if let Doc::Byte(doc) = self {
            unsafe { dr_doc_free(*doc) };
        }
    }
}

/// The file's own open: the byte reader for RGB 8, the 16-bit reader for
/// RGB 16, and the Grayscale and Lab documents' readers.
fn open(name: &str) -> Doc {
    let path = family(name);
    if name.ends_with("rgb8.psd") {
        return Doc::Byte(byte_open(&path));
    }
    if name.ends_with("rgb16.psd") {
        return Doc::Typed(typed_open(&path));
    }
    let fingerprint =
        CString::new(import_report(&path)["source_fingerprint"].as_str().unwrap()).unwrap();
    let working = CString::new("dot_gain_20").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe {
        if name.contains("gray") {
            dr_precision_doc_open_gray_psd(
                cpath(&path).as_ptr(),
                LAYERS,
                fingerprint.as_ptr(),
                working.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        } else {
            dr_precision_doc_open_lab_psd(
                cpath(&path).as_ptr(),
                LAYERS,
                fingerprint.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        }
    };
    assert!(!doc.is_null(), "{name}: {}", take_err_string(err));
    Doc::Typed(unsafe { Box::from_raw(doc) })
}

fn expected_row(row: &Value) -> Row {
    Row {
        visible: row["visible"].as_bool().unwrap(),
        opacity: row["opacity"].as_i64().unwrap(),
        fill: row["fill"].as_i64(),
        mode: row["mode"].as_str().unwrap().to_string(),
        shadow: row["shadow"].as_f64(),
        origin: (!row["group"].as_bool().unwrap()).then(|| {
            let at = row["origin"].as_array().unwrap();
            (at[0].as_i64().unwrap(), at[1].as_i64().unwrap())
        }),
    }
}

/// Every layer of `doc` the step names, against the step.
fn check_step(doc: &Doc, step: &Value, what: &str) {
    let rows = doc.rows();
    for (name, want) in step["layers"].as_object().unwrap() {
        let got = rows
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{what}: no layer {name}"));
        let want = expected_row(want);
        // A fill layer's or a group's fill is not a raster's fill opacity.
        let got_fill = if want.fill.is_none() {
            None
        } else {
            got.1.fill
        };
        assert_eq!(
            (got.1.visible, got.1.opacity, got_fill, &got.1.mode),
            (want.visible, want.opacity, want.fill, &want.mode),
            "{what}: {name}"
        );
        if let Some(distance) = want.shadow {
            assert_eq!(got.1.shadow, Some(distance), "{what}: {name}'s drop shadow");
        }
        if name != "SH" {
            assert_eq!(got.1.origin, want.origin, "{what}: {name}'s origin");
        }
    }
}

// --------------------------------------------------------------- comps --

#[test]
fn photoshops_comps_apply_as_photoshop_applied_them_in_every_mode() {
    let take = take();
    for (name, file) in take["comps"].as_object().unwrap() {
        let mut doc = open(name);
        let comps = doc.comps();
        let got: Vec<Value> = comps["comps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| json!([c["name"], c["comment"], c["captures"]]))
            .collect();
        let want: Vec<Value> = file["comps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                json!([c["name"], c["comment"].as_str().unwrap_or(""), {
                    "visibility": c["visibility"], "position": c["position"],
                    "appearance": c["appearance"]}])
            })
            .collect();
        assert_eq!(got, want, "{name}: the comps");
        let steps = file["steps"].as_array().unwrap();
        check_step(&doc, &steps[0], &format!("{name} opened"));
        for (k, step) in steps[1..].iter().enumerate() {
            doc.apply(k);
            check_step(&doc, step, &format!("{name} after {}", step["state"]));
        }
    }
}

#[test]
fn a_comment_crosses_resource_1065_byte_for_byte() {
    for name in ["comps_rgb8.psd", "heldout_rgb8.psd"] {
        let bytes = std::fs::read(family(name)).unwrap();
        let body = resource_body(&bytes, 1065).expect("1065");
        assert_eq!(
            doctored_core::psd_comp_records::rewritten_comps(&body).unwrap(),
            body,
            "{name}"
        );
    }
}

/// The chain's states of the authored RGB 8 file's LB record (`shmd`), as
/// `(compIDs, visible, offset, opacity, fill, blend code)`.
#[test]
fn the_chain_reads_photoshops_sparse_settings() {
    let bytes = std::fs::read(family("comps_rgb8.psd")).unwrap();
    let blocks = record_blocks(&bytes);
    // FILE order: Background, LA, LB, the closer, GC, G, LS, SH.
    let shmd = |r: usize| {
        blocks[r]
            .iter()
            .find(|(k, _)| k == b"shmd")
            .unwrap()
            .1
            .clone()
    };
    let lb = doctored_core::psd_comp_records::chained_settings(&shmd(2))
        .unwrap()
        .unwrap();
    let rows: Vec<(bool, f64, f64, i32)> = lb.iter().map(|s| (s.1, s.3, s.4, s.5)).collect();
    // The header's DR_BLEND_NORMAL, DR_BLEND_MULTIPLY and DR_BLEND_SCREEN.
    let (normal, multiply, screen) = (0, 1, 2);
    assert_eq!(
        rows,
        [
            (true, 100.0, 100.0, normal),
            (true, 40.0, 100.0, multiply),
            (true, 40.0, 60.0, multiply),
            (true, 40.0, 60.0, screen),
            (true, 80.0, 60.0, screen),
        ]
    );
    let la = doctored_core::psd_comp_records::chained_settings(&shmd(1))
        .unwrap()
        .unwrap();
    let offsets: Vec<(f64, f64)> = la.iter().map(|s| s.2).collect();
    assert_eq!(
        offsets,
        [
            (0.0, 0.0),
            (5.0, 3.0),
            (-4.0, 5.0),
            (-4.0, 5.0),
            (-2.0, 7.0)
        ]
    );
}

// ------------------------------------------------------- the writers --

fn comp_edit(doc: *mut DrDocument, request: Value) -> *mut DrDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    unsafe { dr_doc_free(doc) };
    out
}

/// `capturedInfo` of each comp in a 1065 body, by Adobe's layout: each
/// `capturedInfolong` key's value.
fn captured_infos(body: &[u8]) -> Vec<i32> {
    body.windows(16)
        .enumerate()
        .filter(|(_, w)| &w[..16] == b"capturedInfolong")
        .map(|(at, _)| i32::from_be_bytes(body[at + 16..at + 20].try_into().unwrap()))
        .collect()
}

/// Each `FXRefPoint`'s (Hrzn, Vrtc) in a `shmd` body, in order.
fn ref_points(shmd: &[u8]) -> Vec<(f64, f64)> {
    let key = b"FXRefPointObjc";
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = shmd[at..].windows(key.len()).position(|w| w == key) {
        let start = at + found + key.len();
        let double = |name: &[u8; 4]| {
            let p = start
                + shmd[start..]
                    .windows(8)
                    .position(|w| &w[..4] == name)
                    .unwrap()
                + 8;
            f64::from_be_bytes(shmd[p..p + 8].try_into().unwrap())
        };
        out.push((double(b"Hrzn"), double(b"Vrtc")));
        at = start;
    }
    out
}

#[test]
fn position_and_appearance_comps_round_trip_through_psd() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "bottom.png", &solid(8, 6, [10, 20, 30, 255]));
    doc = common::add_layer(&dir, "a.png", doc, 0, &solid(3, 2, [200, 0, 0, 255]), "A");
    doc = comp_edit(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "Start", "comment": "kept"}),
    );
    let moved = unsafe { dr_doc_with_layer_offset(doc, 1, 4, 3) };
    assert!(!moved.is_null());
    unsafe { dr_doc_free(doc) };
    let faded = unsafe { dr_doc_with_layer_opacity(moved, 1, 0.5) };
    unsafe { dr_doc_free(moved) };
    doc = comp_edit(
        faded,
        json!({"op": "layer_comp", "action": "save", "name": "Where",
               "visibility": false, "position": true, "appearance": false}),
    );
    doc = comp_edit(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "Look",
               "visibility": false, "position": false, "appearance": true}),
    );
    let want: Value = {
        let mut err: *mut c_char = ptr::null_mut();
        let raw = unsafe { doctored_core::ffi_comps::dr_doc_layer_comps_json(doc, &mut err) };
        serde_json::from_str(&take_err_string(raw)).unwrap()
    };
    let path = dir.path().join("c.psd");
    let bytes = byte_save(doc, &path, 0);
    let body = resource_body(&bytes, 1065).expect("1065");
    assert_eq!(captured_infos(&body), [7, 2, 4]);
    let blocks = record_blocks(&bytes);
    let shmd = &blocks[1]
        .iter()
        .find(|(k, _)| k == b"shmd")
        .expect("A's shmd")
        .1;
    assert_eq!(lyr_i(shmd), 2);
    // A sits at (4, 3) now; Start recorded it at (0, 0): -4, -3 from here.
    assert_eq!(
        ref_points(shmd),
        [(0.0, 0.0), (-4.0, -3.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]
    );
    // The reopened comps hold everything but the recorded offsets.
    let mut want = want;
    for comp in want["comps"].as_array_mut().unwrap() {
        for e in comp["entries"].as_array_mut().unwrap() {
            e.as_object_mut().unwrap().remove("offset");
        }
    }
    let reopened = Doc::Byte(byte_open(&path));
    let got = reopened.comps();
    // Entries without the session ids a reopen mints anew.
    let unbound = |v: &Value| -> Vec<Value> {
        v["comps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let mut c = c.clone();
                for e in c["entries"].as_array_mut().unwrap() {
                    e.as_object_mut().unwrap().remove("id");
                }
                c
            })
            .collect()
    };
    assert_eq!(unbound(&got), unbound(&want));
    drop(reopened);
    // 16 bits: the promoted document writes and reopens the same comps.
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe {
        doctored_core::ffi_precision_conversion::dr_doc_promote_structure(
            doc,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!wide.is_null(), "{}", take_err_string(err));
    let wide = unsafe { Box::from_raw(wide) };
    let path16 = dir.path().join("c16.psd");
    typed_save(&wide, &path16);
    let typed = Doc::Typed(typed_open(&path16));
    let got = typed.comps();
    for key in ["name", "comment", "captures"] {
        let pick = |v: &Value| -> Vec<Value> {
            v["comps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c[key].clone())
                .collect()
        };
        assert_eq!(pick(&got), pick(&want), "16 bits: {key}");
    }
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------- vector masks --

/// The layered open's flattened pixels against the file's stored composite:
/// the largest difference of any channel at any pixel.
fn worst_against_composite(path: &Path) -> u8 {
    let report = import_report(path);
    let layered = open_psd(path, &report, LAYERS);
    let stored = open_psd(path, &report, COMPOSITE);
    let a = common::flat_pixels(&*layered);
    let b = common::flat_pixels(&*stored);
    assert_eq!(a.len(), b.len());
    a.iter().zip(&b).map(|(x, y)| x.abs_diff(*y)).max().unwrap()
}

#[test]
fn raster_layers_under_photoshops_vector_masks_composite_as_the_file_does() {
    let path = family("paths_rgb8.psd");
    assert!(
        worst_against_composite(&path) <= 1,
        "{}",
        worst_against_composite(&path)
    );
    let doc = byte_open(&path);
    let document = unsafe { &*doc };
    let index = |name: &str| {
        (0..document.layers.len())
            .find(|&i| document.layers[i].name == name)
            .unwrap_or_else(|| panic!("{name}"))
    };
    // Kept as vector masks: the plain one, the one beside a user mask (with
    // its raster part), the disabled one, the unlinked one and two adds.
    for (name, raster) in [
        ("Va", false),
        ("Vb", true),
        ("Vh", false),
        ("Vi", false),
        ("Vj", false),
    ] {
        let view = document
            .vector_mask(index(name))
            .unwrap_or_else(|| panic!("{name}: a vector mask"));
        assert_eq!(view.has_raster, raster, "{name}");
    }
    assert!(!document.vector_mask(index("Vh")).unwrap().enabled);
    assert!(!document.vector_mask(index("Vi")).unwrap().linked);
    // Rendered: the masks whose subpaths subtract, intersect or exclude, or
    // start by subtracting, and the stroked shape keeps its pixels.
    for name in ["Vc", "Vd", "Vx", "Ve"] {
        assert!(document.vector_mask(index(name)).is_none(), "{name}");
        assert!(
            document.layers[index(name)].mask.is_some(),
            "{name}: a raster mask"
        );
    }
    let losses = strings(&import_report(&path), "verified_losses");
    for name in ["Vc", "Vd", "Vx", "Ve"] {
        assert!(
            losses.iter().any(|l| l.contains(&format!(
                "(“{name}”) has a vector mask Doctored does not keep"
            ))),
            "{name}: {losses:?}"
        );
    }
    // Photoshop's 8-bit renderings of the masks it does not keep live, as
    // canvas coverage, for the 16-bit file below (which holds none).
    let rendered: Vec<(&str, Vec<u8>)> = ["Vc", "Vd", "Vx", "Ve"]
        .into_iter()
        .map(|name| {
            let layer = &document.layers[index(name)];
            let plane = layer.mask.as_ref().unwrap();
            let (ox, oy) = layer.mask_properties.offset;
            let outside = layer.mask_properties.outside_coverage;
            let (w, h) = (document.width as i32, document.height as i32);
            let canvas = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let (u, v) = (x - ox, y - oy);
                    if u >= 0 && v >= 0 && (u as u32) < plane.width() && (v as u32) < plane.height()
                    {
                        plane.get_pixel(u as u32, v as u32)[0]
                    } else {
                        outside
                    }
                })
                .collect();
            (name, canvas)
        })
        .collect();
    unsafe { dr_doc_free(doc) };
    // 16 bits: the same layers, through the typed open.
    let typed = typed_open(&family("paths_rgb16.psd"));
    let named = |name: &str| {
        (0..typed.layer_count())
            .find(|&i| typed.layer(i).unwrap().name() == name)
            .unwrap_or_else(|| panic!("{name}"))
    };
    for (name, raster) in [
        ("Va", false),
        ("Vb", true),
        ("Vh", false),
        ("Vi", false),
        ("Vj", false),
    ] {
        let view = typed
            .vector_mask(named(name))
            .unwrap_or_else(|| panic!("16 bits {name}: a vector mask"));
        assert_eq!(view.has_raster, raster, "16 bits {name}");
    }
    // Baked from the path (the file holds no rendering): the 8-bit file's
    // rendering of the same mask, within one 8-bit step.
    for (name, eight) in &rendered {
        assert!(typed.vector_mask(named(name)).is_none(), "16 bits {name}");
        let mask = typed
            .layer(named(name))
            .unwrap()
            .mask()
            .unwrap_or_else(|| panic!("16 bits {name}: a baked mask"));
        let samples = mask.plane().samples();
        assert_eq!(samples.len(), eight.len(), "{name}: a canvas-sized mask");
        let worst = samples
            .iter()
            .zip(eight)
            .map(|(&b, &a)| (i32::from(a) * 257 - i32::from(b)).abs())
            .max()
            .unwrap();
        assert!(worst <= 257, "16 bits {name}: {worst}");
    }
}
