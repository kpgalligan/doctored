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
//!   as vector masks (the second with its raster part), and so are the ones
//!   whose subpaths subtract, intersect or exclude, holding the file's
//!   operations. At 16 bits, where the file holds no rendering, each of
//!   those masks is held within one 8-bit step of the 8-bit file's own
//!   rendering of it (its `-2` channel, read here from the file's bytes),
//!   and the run prints how far one coverage of the shape and the per-pixel
//!   product of each subpath's coverage (`vmsk_bake`) each sit from it.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{dr_precision_doc_open_gray_psd, dr_precision_doc_open_lab_psd};
use doctored_core::ffi_vector_mask::dr_doc_vector_mask_json;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use doctored_core::psd_vector_mask_bake::vmsk_bake;
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
    // Kept as vector masks holding the file's operations: the masks whose
    // subpaths subtract, intersect or exclude, or start by subtracting.
    let bytes8 = std::fs::read(&path).unwrap();
    for name in HELD {
        let i = index(name);
        assert!(document.vector_mask(i).is_some(), "{name}: a vector mask");
        let json: Value =
            serde_json::from_str(&take_err_string(unsafe { dr_doc_vector_mask_json(doc, i) }))
                .unwrap();
        let ops: Vec<String> = json["subpaths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["op"].as_str().unwrap_or("none").to_string())
            .collect();
        assert_eq!(ops, vmsk_ops(&vmsk_of(&bytes8, name)), "{name}");
    }
    let losses = strings(&import_report(&path), "verified_losses");
    assert!(
        !losses.iter().any(|l| l.contains("does not keep")),
        "{losses:?}"
    );
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
    // The file holds no rendering of these masks: each is held to the 8-bit
    // file's own rendering of it (P), within one 8-bit step, and the run
    // prints how far one coverage of the shape (the vector mask the open
    // keeps) and the per-pixel product of each subpath's coverage (the bake
    // of the record's own `vmsk`) each sit from it.
    let bytes16 = std::fs::read(family("paths_rgb16.psd")).unwrap();
    let (w, h) = typed.dimensions();
    for name in HELD {
        let i = named(name);
        assert!(
            typed.vector_mask(i).is_some(),
            "16 bits {name}: a vector mask"
        );
        let eight = rendered(&bytes8, name);
        assert!(
            eight.contains(&0) && eight.contains(&255),
            "{name}: P covers a part of the canvas"
        );
        let mask = typed
            .layer(i)
            .unwrap()
            .mask()
            .unwrap_or_else(|| panic!("16 bits {name}: the vector mask's plane"));
        let plane = mask.plane();
        let (ox, oy) = mask.properties().offset;
        let outside = u16::from(mask.properties().outside_coverage) * 257;
        let samples = plane.samples();
        let live: Vec<u16> = (0..h as i32)
            .flat_map(|y| (0..w as i32).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (u, v) = (x - ox, y - oy);
                if u >= 0 && v >= 0 && (u as u32) < plane.width() && (v as u32) < plane.height() {
                    samples[(v as u32 * plane.width() + u as u32) as usize]
                } else {
                    outside
                }
            })
            .collect();
        let product = vmsk_bake(&vmsk_of(&bytes16, name), (w, h))
            .expect("the record's vmsk bakes")
            .unwrap_or_else(|e| panic!("16 bits {name}: {e}"));
        let from_eight = |plane: &[u16]| -> i32 {
            assert_eq!(plane.len(), eight.len(), "{name}: a canvas-sized plane");
            plane
                .iter()
                .zip(&eight)
                .map(|(&b, &a)| (i32::from(a) * 257 - i32::from(b)).abs())
                .max()
                .unwrap()
        };
        let (one, folded) = (from_eight(&live), from_eight(&product));
        println!("16 bits {name}: one coverage {one}, product rule {folded}");
        assert!(folded <= 257, "16 bits {name}: the product rule {folded}");
        assert!(one <= 257, "16 bits {name}: one coverage {one}");
    }
}

/// The layers whose vector masks combine their subpaths by subtracting,
/// intersecting or excluding them, or start by subtracting (`provenance.json`).
const HELD: [&str; 4] = ["Vc", "Vd", "Vx", "Ve"];

/// One layer record of a classic PSD by Adobe's layout: its Pascal name,
/// its mask body (the layer mask data), its additional blocks and each
/// channel's id and stored bytes (the compression word first).
struct FileRecord {
    name: String,
    mask: Vec<u8>,
    blocks: Vec<([u8; 4], Vec<u8>)>,
    channels: Vec<(i16, Vec<u8>)>,
}

/// Every layer record of the classic PSD `bytes` in FILE order, as
/// `record_blocks` walks them, with the channels' stored bytes, which
/// follow the records, record by record and channel by channel. A 16-bit
/// file's layer info is empty and its records are the `Lr16` block's,
/// after the global layer mask info.
fn file_records(bytes: &[u8]) -> Vec<FileRecord> {
    let be16 = |at: usize| usize::from(u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap()));
    let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut at = 26;
    at += 4 + be32(at);
    at += 4 + be32(at);
    if be32(at + 4) == 0 {
        let global = at + 8;
        let from = global + 4 + be32(global);
        at = from
            + bytes[from..]
                .windows(8)
                .position(|w| w == b"8BIMLr16")
                .expect("an Lr16 block")
            + 12;
    } else {
        at += 8;
    }
    let count = i16::from_be_bytes(bytes[at..at + 2].try_into().unwrap()).unsigned_abs();
    at += 2;
    let mut heads = Vec::new();
    for _ in 0..count {
        at += 16;
        let n = be16(at);
        at += 2;
        let lengths: Vec<(i16, usize)> = (0..n)
            .map(|k| {
                let c = at + 6 * k;
                let id = i16::from_be_bytes(bytes[c..c + 2].try_into().unwrap());
                (id, be32(c + 2))
            })
            .collect();
        at += 6 * n + 12;
        let extra = at + 4;
        at = extra + be32(at);
        let mask = bytes[extra + 4..extra + 4 + be32(extra)].to_vec();
        let ranges = extra + 4 + mask.len();
        let name_at = ranges + 4 + be32(ranges);
        let name_len = usize::from(bytes[name_at]);
        let name = String::from_utf8_lossy(&bytes[name_at + 1..name_at + 1 + name_len]);
        // The additional blocks follow the name, padded to four bytes.
        let mut cursor = name_at + ((name_len + 4) & !3);
        let mut blocks = Vec::new();
        while cursor + 12 <= at {
            let key: [u8; 4] = bytes[cursor + 4..cursor + 8].try_into().unwrap();
            let len = be32(cursor + 8);
            blocks.push((key, bytes[cursor + 12..cursor + 12 + len].to_vec()));
            cursor += 12 + len + len % 2;
        }
        heads.push((name.into_owned(), mask, blocks, lengths));
    }
    let mut records = Vec::new();
    for (name, mask, blocks, lengths) in heads {
        let mut channels = Vec::new();
        for (id, len) in lengths {
            channels.push((id, bytes[at..at + len].to_vec()));
            at += len;
        }
        records.push(FileRecord {
            name,
            mask,
            blocks,
            channels,
        });
    }
    records
}

/// The record named `name` in the classic PSD `bytes`.
fn file_record(bytes: &[u8], name: &str) -> FileRecord {
    let records = file_records(bytes);
    let names: Vec<String> = records.iter().map(|r| r.name.clone()).collect();
    records
        .into_iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("{name}: no record among {names:?}"))
}

/// The `vmsk` body of the record named `name` in the classic PSD `bytes`.
fn vmsk_of(bytes: &[u8], name: &str) -> Vec<u8> {
    file_record(bytes, name)
        .blocks
        .into_iter()
        .find(|(key, _)| key == b"vmsk")
        .unwrap_or_else(|| panic!("{name}: no vmsk"))
        .1
}

/// The operation each subpath of a `vmsk` body names, by Adobe's layout:
/// after the version and flags, 26-byte records, of which selector 0 or 3
/// is a subpath's length record holding its operation word at byte 4 (0
/// exclude, 1 unite, 2 subtract, 3 intersect).
fn vmsk_ops(body: &[u8]) -> Vec<String> {
    body[8..]
        .chunks_exact(26)
        .filter(|r| matches!(u16::from_be_bytes([r[0], r[1]]), 0 | 3))
        .map(|r| match i16::from_be_bytes([r[4], r[5]]) {
            0 => "exclude".to_string(),
            1 => "unite".to_string(),
            2 => "subtract".to_string(),
            3 => "intersect".to_string(),
            word => format!("word {word}"),
        })
        .collect()
}

/// The 8-bit PSD `bytes`' own rendering of the vector mask on the record
/// named `name` (P), spread to the canvas: the record's `-2` channel over
/// the mask body's rectangle (top, left, bottom, right), the body's default
/// colour outside it. The channel is stored raw (compression 0) or as
/// PackBits rows after a u16 byte count per row (compression 1).
fn rendered(bytes: &[u8], name: &str) -> Vec<u8> {
    let canvas_h = u32::from_be_bytes(bytes[14..18].try_into().unwrap()) as i32;
    let canvas_w = u32::from_be_bytes(bytes[18..22].try_into().unwrap()) as i32;
    let record = file_record(bytes, name);
    let side = |at: usize| i32::from_be_bytes(record.mask[at..at + 4].try_into().unwrap());
    let (top, left, bottom, right) = (side(0), side(4), side(8), side(12));
    let default = record.mask[16];
    let (w, h) = ((right - left) as usize, (bottom - top) as usize);
    let data = &record
        .channels
        .iter()
        .find(|(id, _)| *id == -2)
        .unwrap_or_else(|| panic!("{name}: no -2 channel"))
        .1;
    let samples: Vec<u8> = match u16::from_be_bytes([data[0], data[1]]) {
        0 => data[2..2 + w * h].to_vec(),
        1 => {
            let mut out = Vec::with_capacity(w * h);
            let mut at = 2 + 2 * h;
            for row in 0..h {
                let end =
                    at + usize::from(u16::from_be_bytes([data[2 + 2 * row], data[3 + 2 * row]]));
                let start = out.len();
                while at < end {
                    let n = data[at] as i8;
                    at += 1;
                    if n >= 0 {
                        let k = n as usize + 1;
                        out.extend_from_slice(&data[at..at + k]);
                        at += k;
                    } else if n != -128 {
                        out.extend(std::iter::repeat_n(data[at], (1 - i32::from(n)) as usize));
                        at += 1;
                    }
                }
                assert_eq!(out.len() - start, w, "{name}: row {row}");
            }
            out
        }
        other => panic!("{name}: compression {other}"),
    };
    (0..canvas_h)
        .flat_map(|y| (0..canvas_w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (u, v) = (x - left, y - top);
            if u >= 0 && v >= 0 && (u as usize) < w && (v as usize) < h {
                samples[v as usize * w + u as usize]
            } else {
                default
            }
        })
        .collect()
}
