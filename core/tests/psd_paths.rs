//! Paths and vector masks in PSD, PSB and the layered TIFF (`psd_paths`,
//! `psd_path_records`), through the FFI.
//!
//! - Photoshop's own `vmsk` (the "Shape" record of
//!   `cmyk-psd-547/take1/unsupported/unsupported8.psd`, below verbatim) on a
//!   `SoCo` record imports as a solid fill layer under that vector mask, the
//!   rectangle the 8.24 definition gives, computed here.
//! - A saved path's resource is this file's own 26-byte records computed from
//!   Adobe's layout, not the writer's.
//! - Saved paths, the work path and a raster-less vector mask round-trip
//!   through PSD 8 and 16, PSB and layered TIFF within the fixed-point
//!   resolution (`canvas side / 2²⁴`, at most 1e-4 px here); the reopened
//!   document's `.doctored` keeps them.
//! - A vector mask beside a raster mask keeps its baked mask and its
//!   sentence.
//! - A `vmsk` whose subpaths subtract, or that starts from all pixels, is not
//!   carried: the shape record keeps its own pixels and the dropped-mask
//!   sentence. One that would take the file's paths and vector masks past
//!   the document's anchor cap is not carried either: on a raster record it
//!   is drawn into the raster mask, and the opened file then saves.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_path::PathEntry;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_path::dr_doc_with_paths_json;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_psd::{
    dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
};
use doctored_core::ffi_vector_mask::dr_doc_set_vector_mask;
use doctored_core::path_geometry::{FillRule, PathShape};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use crate::psd_mask_fixture as fixture;
use common::psd_photoshop::{import_report, open_psd, soco_body, LAYERS};
use common::{cpath, doc_from, solid, take_err_string};

/// Photoshop 26.10.0's `vmsk` for a 24 × 16 rectangle at (32, 0) on a
/// 64 × 48 canvas, byte for byte (the "Shape" record named above).
const AUTHORED_VMSK: &str = "0000000300000000000600000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000400010001000000000000000000000000000000000000000200000000008000000000000000800000000000000080000000020000000000e000000000000000e000000000000000e0000000020055555500e000000055555500e000000055555500e0000000020055555500800000005555550080000000555555008000000000";

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

/// An 8.24 fraction as Adobe defines it: a signed 32-bit number over 2²⁴.
fn fixed(bytes: &[u8]) -> f64 {
    f64::from(i32::from_be_bytes(bytes.try_into().unwrap())) / 16_777_216.0
}

pub fn byte_open(path: &Path) -> *mut DrDocument {
    Box::into_raw(open_psd(path, &import_report(path), LAYERS))
}

pub fn typed_open(path: &Path) -> Box<PrecisionDocument> {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "typed open: {}", take_err_string(err));
    unsafe { Box::from_raw(doc) }
}

pub fn byte_save(doc: *const DrDocument, path: &Path, container: i32) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            container,
            LAYERS,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "save: {}", take_err_string(err));
    std::fs::read(path).unwrap()
}

pub fn typed_save(doc: &PrecisionDocument, path: &Path) {
    let projection = doc.render(LIMIT, 0, None).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            0,
            LAYERS,
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "typed save: {}", take_err_string(err));
}

// ---------------------------------------------- Photoshop's own `vmsk` --

#[test]
fn photoshops_shape_layer_imports_as_a_fill_under_its_vector_mask() {
    let body = hex(AUTHORED_VMSK);
    assert_eq!(body.len(), 192);
    // The rectangle, from Adobe's definition: each knot's anchor is the
    // second (vertical, horizontal) pair of 8.24 fractions, over 48 × 64.
    let corners: Vec<[f64; 2]> = (0..4)
        .map(|k| {
            let knot = &body[8 + 3 * 26 + 26 * k..8 + 3 * 26 + 26 * (k + 1)];
            [fixed(&knot[14..18]) * 64.0, fixed(&knot[10..14]) * 48.0]
        })
        .collect();
    // 1/3 of 48 is 16 to within the fixed-point step Photoshop rounded to.
    let close = |a: &[[f64; 2]], b: &[[f64; 2]]| {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(p, q)| (p[0] - q[0]).abs() <= 1e-4 && (p[1] - q[1]).abs() <= 1e-4)
    };
    assert!(
        close(
            &corners,
            &[[32.0, 0.0], [56.0, 0.0], [56.0, 16.0], [32.0, 16.0]]
        ),
        "{corners:?}"
    );

    // A blue `SoCo` record whose pixels are Photoshop's pre-masked form.
    let mut record =
        fixture::Record::raster([0, 32, 16, 56], &[40, 40, 200, 255].repeat(24 * 16), false);
    record.flags = 0x18;
    record.blocks = [
        fixture::block(b"SoCo", &soco_body()),
        fixture::block(b"vmsk", &body),
    ]
    .concat();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("shape.psd");
    std::fs::write(&path, fixture::fixture(&[record], [64, 48], false)).unwrap();
    let doc = byte_open(&path);
    let document = unsafe { &*doc };
    let view = document.vector_mask(0).expect("a vector mask");
    let anchors: Vec<[f64; 2]> = view.shape.subpaths[0]
        .anchors
        .iter()
        .map(|a| a.point)
        .collect();
    assert!(close(&anchors, &corners), "{anchors:?}");
    assert!(view.enabled && view.linked && !view.has_raster);
    let meta = common::ffi_meta(doc, 0).expect("a fill description");
    assert!(meta.contains("\"type\":\"fill\""), "{meta}");
    // Its picture: the fill inside the rectangle, nothing outside it.
    let flat = common::flat_pixels(doc);
    assert_eq!(common::pixel(&flat, 64, 40, 8)[3], 255);
    assert_eq!(common::pixel(&flat, 64, 10, 30)[3], 0);
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------- the path resources --

/// A saved path with a curve, a corner, an open and a closed subpath, and a
/// work path, on a 60 × 40 canvas.
fn path_list() -> Value {
    json!({"paths": [
        {"kind": "work", "fill_rule": "nonzero", "subpaths": [
            {"closed": true, "anchors": [[5, 5], [20, 5], [20, 15]]}]},
        {"kind": "saved", "name": "Curve", "fill_rule": "evenodd", "subpaths": [
            {"closed": true, "anchors": [
                {"x": 10, "y": 10, "in": [8, 14], "out": [12, 6], "smooth": true},
                [40, 10],
                {"x": 40, "y": 30, "in": [44, 26], "out": [36, 34]}]},
            {"closed": false, "anchors": [[2.5, 37.25], [57.75, 1.5]]}]},
        {"kind": "saved", "name": "Box", "fill_rule": "nonzero", "subpaths": [
            {"closed": true, "anchors": [[30, 2], [58, 2], [58, 20], [30, 20]]}]}
    ]})
}

fn with_paths(doc: *mut DrDocument) -> *mut DrDocument {
    let text = CString::new(path_list().to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_with_paths_json(doc, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(doc) };
    out
}

fn with_vector_mask(doc: *mut DrDocument) -> *mut DrDocument {
    let shape = json!({"fill_rule": "nonzero", "subpaths": [
        {"closed": true, "anchors": [[12, 4], [48, 8], [30, 36]]}]});
    let text = CString::new(shape.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_set_vector_mask(doc, 1, text.as_ptr(), ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(doc) };
    out
}

pub fn document(dir: &TempDir) -> *mut DrDocument {
    let doc = doc_from(dir, "base.png", &solid(60, 40, [10, 20, 30, 255]));
    let doc = common::add_layer(
        dir,
        "top.png",
        doc,
        0,
        &solid(60, 40, [200, 0, 0, 255]),
        "Top",
    );
    with_vector_mask(with_paths(doc))
}

/// Every coordinate of two path lists equal within the fixed-point step, and
/// the names, kinds, order, closure and fill rules equal.
pub fn assert_paths_close(got: &[PathEntry], want: &[PathEntry], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: count");
    for (g, w) in got.iter().zip(want) {
        assert_eq!((&g.name, g.kind), (&w.name, w.kind), "{what}");
        assert_shapes_close(&g.shape, &w.shape, what);
    }
}

pub fn assert_shapes_close(got: &PathShape, want: &PathShape, what: &str) {
    assert_eq!(got.fill_rule, want.fill_rule, "{what}: fill rule");
    assert_eq!(got.subpaths.len(), want.subpaths.len(), "{what}");
    for (g, w) in got.subpaths.iter().zip(&want.subpaths) {
        assert_eq!(g.closed, w.closed, "{what}");
        assert_eq!(g.anchors.len(), w.anchors.len(), "{what}");
        for (a, b) in g.anchors.iter().zip(&w.anchors) {
            assert_eq!(a.smooth, b.smooth, "{what}");
            for (p, q) in [
                (a.point, b.point),
                (a.handle_in, b.handle_in),
                (a.handle_out, b.handle_out),
            ] {
                assert!(
                    (p[0] - q[0]).abs() <= 1e-4 && (p[1] - q[1]).abs() <= 1e-4,
                    "{what}: {p:?} vs {q:?}"
                );
            }
        }
    }
}

/// The 26-byte records of one subpath, computed here from Adobe's layout.
pub fn expected_records(shape: &PathShape, canvas: (f64, f64)) -> Vec<u8> {
    let mut out = Vec::new();
    let mut record = |selector: u16, body: Vec<u8>| {
        out.extend_from_slice(&selector.to_be_bytes());
        out.extend_from_slice(&body);
        out.resize(out.len() + 24 - body.len(), 0);
    };
    record(6, vec![]);
    record(8, vec![0, 0]);
    let op: i16 = if shape.fill_rule == FillRule::EvenOdd {
        0
    } else {
        1
    };
    for sub in &shape.subpaths {
        let mut length = (sub.anchors.len() as u16).to_be_bytes().to_vec();
        length.extend_from_slice(&op.to_be_bytes());
        length.extend_from_slice(&1u16.to_be_bytes());
        record(if sub.closed { 0 } else { 3 }, length);
        for a in &sub.anchors {
            let mut knot = Vec::new();
            for p in [a.handle_in, a.point, a.handle_out] {
                for (v, side) in [(p[1], canvas.1), (p[0], canvas.0)] {
                    knot.extend_from_slice(
                        &((v / side * 16_777_216.0).round() as i32).to_be_bytes(),
                    );
                }
            }
            let selector = match (sub.closed, a.smooth) {
                (true, true) => 1,
                (true, false) => 2,
                (false, true) => 4,
                (false, false) => 5,
            };
            record(selector, knot);
        }
    }
    out
}

/// Resource `id`'s (Pascal name, body) in a classic PSD, by Adobe's layout.
pub fn resource(bytes: &[u8], id: u16) -> Option<(String, Vec<u8>)> {
    let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut at = 26;
    at += 4 + be32(at);
    let end = at + 4 + be32(at);
    at += 4;
    while at < end {
        let rid = u16::from_be_bytes(bytes[at + 4..at + 6].try_into().unwrap());
        let n = usize::from(bytes[at + 6]);
        let name = String::from_utf8(bytes[at + 7..at + 7 + n].to_vec()).unwrap();
        at += 6 + ((n + 2) & !1);
        let len = be32(at);
        if rid == id {
            return Some((name, bytes[at + 4..at + 4 + len].to_vec()));
        }
        at += 4 + len + len % 2;
    }
    None
}

#[test]
fn a_saved_paths_resource_is_adobes_record_layout() {
    let dir = TempDir::new().unwrap();
    let doc = document(&dir);
    let bytes = byte_save(doc, &dir.path().join("p.psd"), 0);
    let paths = unsafe { &*doc }.paths().to_vec();
    let (name, body) = resource(&bytes, 2000).expect("resource 2000");
    assert_eq!(name, "Curve");
    assert_eq!(body, expected_records(&paths[1].shape, (60.0, 40.0)));
    let (name, body) = resource(&bytes, 2001).expect("resource 2001");
    assert_eq!(name, "Box");
    assert_eq!(body, expected_records(&paths[2].shape, (60.0, 40.0)));
    let (_, body) = resource(&bytes, 1025).expect("the work path");
    assert_eq!(body, expected_records(&paths[0].shape, (60.0, 40.0)));
    unsafe { dr_doc_free(doc) };
}

#[test]
fn paths_and_a_vector_mask_round_trip_through_every_rgb_container() {
    let dir = TempDir::new().unwrap();
    let doc = document(&dir);
    let want_paths = unsafe { &*doc }.paths().to_vec();
    let want_mask = unsafe { &*doc }
        .vector_mask(1)
        .expect("the vector mask")
        .shape;
    for (container, name) in [(0, "r.psd"), (1, "r.psb"), (2, "r.tif")] {
        let path = dir.path().join(name);
        byte_save(doc, &path, container);
        let reopened = byte_open(&path);
        let back = unsafe { &*reopened };
        assert_paths_close(back.paths(), &want_paths, name);
        let view = back.vector_mask(1).expect("the vector mask comes back");
        assert!(view.enabled && view.linked && !view.has_raster, "{name}");
        assert_shapes_close(&view.shape, &want_mask, name);
        if container == 0 {
            // …and the reopened document keeps both through `.doctored`.
            let native = dir.path().join("r.doctored");
            back.save_native(native.to_str().unwrap()).unwrap();
            let again = DrDocument::open(native.to_str().unwrap()).unwrap();
            assert_paths_close(again.paths(), &want_paths, "native");
            assert!(again.vector_mask(1).is_some(), "native vector mask");
        }
        unsafe { dr_doc_free(reopened) };
    }

    // 16 bits, through the promoted document.
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!wide.is_null(), "{}", take_err_string(err));
    let wide = unsafe { Box::from_raw(wide) };
    let path = dir.path().join("r16.psd");
    typed_save(&wide, &path);
    let back = typed_open(&path);
    assert_paths_close(back.paths(), &want_paths, "16 bits");
    let view = back.vector_mask(1).expect("the 16-bit vector mask");
    assert_shapes_close(&view.shape, &want_mask, "16 bits");
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_vector_mask_beside_a_raster_mask_keeps_its_baked_mask() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "b.png", &solid(60, 40, [10, 20, 30, 255]));
    let doc = common::add_layer(
        &dir,
        "t.png",
        doc,
        0,
        &solid(60, 40, [200, 0, 0, 255]),
        "Top",
    );
    let masked = unsafe { &*doc }
        .add_mask(1, MaskKind::RevealAll)
        .expect("a raster mask");
    unsafe { dr_doc_free(doc) };
    let both = with_vector_mask(Box::into_raw(Box::new(masked)));
    let path = dir.path().join("both.psd");
    let bytes = byte_save(both, &path, 0);
    assert!(
        !bytes.windows(8).any(|w| w == b"8BIMvmsk"),
        "no vmsk beside a raster part"
    );
    let reopened = byte_open(&path);
    assert!(unsafe { &*reopened }.vector_mask(1).is_none());
    assert!(
        unsafe { &*reopened }.layers[1].mask.is_some(),
        "the baked mask"
    );
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(both) };
}

/// One saved-path resource: `8BIM`, the id, the Pascal name padded even,
/// the body (Adobe's image resource block).
fn path_resource(id: u16, name: &str, records: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend_from_slice(&id.to_be_bytes());
    out.push(name.len() as u8);
    out.extend_from_slice(name.as_bytes());
    if name.len().is_multiple_of(2) {
        out.push(0);
    }
    out.extend_from_slice(&(records.len() as u32).to_be_bytes());
    out.extend_from_slice(records);
    out
}

/// Import is entry by entry: a saved path whose shape the model refuses (a
/// subpath with no knot) is dropped with its sentence and claims nothing, so
/// a later path of the same name is kept.
#[test]
fn a_refused_path_is_dropped_alone() {
    let record = |selector: u16, body: &[u8]| {
        let mut r = selector.to_be_bytes().to_vec();
        r.extend_from_slice(body);
        r.resize(26, 0);
        r
    };
    let empty = [
        record(6, &[]),
        record(8, &[]),
        record(0, &[0, 0, 0, 1, 0, 1]),
    ]
    .concat();
    let square = {
        let pairs = [(0.25, 0.25), (0.25, 0.75), (0.75, 0.75), (0.75, 0.25)];
        let mut out = [
            record(6, &[]),
            record(8, &[]),
            record(0, &[0, 4, 0, 1, 0, 1]),
        ]
        .concat();
        for (v, h) in pairs {
            let mut knot = Vec::new();
            for _ in 0..3 {
                knot.extend_from_slice(&((v * 16_777_216.0) as i32).to_be_bytes());
                knot.extend_from_slice(&((h * 16_777_216.0) as i32).to_be_bytes());
            }
            out.extend(record(2, &knot));
        }
        out
    };
    let resources = [
        path_resource(2000, "Dup", &empty),
        path_resource(2001, "Dup", &square),
    ]
    .concat();
    let base = fixture::fixture(
        &[fixture::Record::raster(
            [0, 0, 8, 8],
            &[9, 9, 9, 255].repeat(64),
            false,
        )],
        [8, 8],
        false,
    );
    // Adobe's header (26 bytes), the empty colour-mode section, then the
    // resource section this test fills.
    let mut bytes = base[..30].to_vec();
    bytes.extend_from_slice(&(resources.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&resources);
    bytes.extend_from_slice(&base[34..]);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("dup.psd");
    std::fs::write(&path, bytes).unwrap();
    let losses = common::psd_photoshop::strings(&import_report(&path), "verified_losses");
    assert!(
        losses
            .iter()
            .any(|l| l.starts_with("Path “Dup” is not imported:")),
        "{losses:?}"
    );
    let doc = byte_open(&path);
    let paths = unsafe { &*doc }.paths();
    assert_eq!(paths.len(), 1, "the second Dup is kept");
    assert_eq!(paths[0].name, "Dup");
    assert_eq!(paths[0].shape.subpaths[0].anchors.len(), 4);
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------- a `vmsk` that is not carried --

/// The report's sentence for a record whose mask the layered import drops.
pub const DROPPED: &str = "(“Duplicate”) has a raster mask channel or advanced mask metadata; layered import drops it: vector, real-user (-3), or advanced mask behavior is unsupported.";

/// One 26-byte path record: the selector, then `body`, zero-padded.
fn path_record(selector: u16, body: &[u8]) -> Vec<u8> {
    let mut out = selector.to_be_bytes().to_vec();
    out.extend_from_slice(body);
    out.resize(26, 0);
    out
}

/// A closed subpath by Adobe's layout: its length record (the knot count,
/// the combine `operation`, then 1) and an unlinked knot per corner, each of
/// its three points the corner as a (vertical, horizontal) pair of 8.24
/// fractions of a `canvas`.
pub fn closed_subpath(corners: &[(f64, f64)], operation: i16, canvas: (f64, f64)) -> Vec<u8> {
    let mut length = (corners.len() as u16).to_be_bytes().to_vec();
    length.extend_from_slice(&operation.to_be_bytes());
    length.extend_from_slice(&1u16.to_be_bytes());
    let mut out = path_record(0, &length);
    for &(x, y) in corners {
        let mut knot = Vec::new();
        for _ in 0..3 {
            knot.extend_from_slice(&((y / canvas.1 * 16_777_216.0).round() as i32).to_be_bytes());
            knot.extend_from_slice(&((x / canvas.0 * 16_777_216.0).round() as i32).to_be_bytes());
        }
        out.extend(path_record(2, &knot));
    }
    out
}

/// A path's records: the fill rule record, the initial fill record
/// (`initial` 1: the path starts from all pixels), then `subpaths`.
fn path_body(initial: u16, subpaths: &[Vec<u8>]) -> Vec<u8> {
    let mut out = path_record(6, &[]);
    out.extend(path_record(8, &initial.to_be_bytes()));
    for subpath in subpaths {
        out.extend_from_slice(subpath);
    }
    out
}

/// A `vmsk` body: version 3, flags 0 (enabled, linked, not inverted), then
/// the path's records.
pub fn vmsk(initial: u16, subpaths: &[Vec<u8>]) -> Vec<u8> {
    let mut out = 3u32.to_be_bytes().to_vec();
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend(path_body(initial, subpaths));
    out
}

/// `base` (a `fixture::fixture` file) with `resources` as its image
/// resource section: Adobe's header (26 bytes) and the empty colour-mode
/// section stay, the empty resource section is replaced.
fn with_resources(base: &[u8], resources: &[u8]) -> Vec<u8> {
    let mut bytes = base[..30].to_vec();
    bytes.extend_from_slice(&(resources.len() as u32).to_be_bytes());
    bytes.extend_from_slice(resources);
    bytes.extend_from_slice(&base[34..]);
    bytes
}

/// A shape layer whose second subpath SUBTRACTS (operation 2), and one whose
/// path starts from all pixels (an initial fill record of 1), cannot be held
/// under Doctored's one fill rule: neither `vmsk` is carried, so the record
/// keeps the pixels Photoshop stored (already masked: a ring around a
/// transparent hole) and the dropped-mask sentence. Carried, either would
/// fill the whole outer rectangle.
#[test]
fn a_vector_mask_one_fill_rule_cannot_hold_keeps_the_records_pixels() {
    let canvas = (64.0, 48.0);
    let outer = closed_subpath(
        &[(8.0, 8.0), (56.0, 8.0), (56.0, 40.0), (8.0, 40.0)],
        1,
        canvas,
    );
    let hole = closed_subpath(
        &[(24.0, 16.0), (40.0, 16.0), (40.0, 32.0), (24.0, 32.0)],
        2,
        canvas,
    );
    let pixels: Vec<u8> = (8..40)
        .flat_map(|y| {
            (8..56).flat_map(move |x| {
                if (24..40).contains(&x) && (16..32).contains(&y) {
                    [0, 0, 0, 0]
                } else {
                    [40, 40, 200, 255]
                }
            })
        })
        .collect();
    let dir = TempDir::new().unwrap();
    for (what, body) in [
        ("subtracted", vmsk(0, &[outer.clone(), hole])),
        ("from all pixels", vmsk(1, &[outer])),
    ] {
        let mut record = fixture::Record::raster([8, 8, 40, 56], &pixels, false);
        record.flags = 0x18;
        record.blocks = [
            fixture::block(b"SoCo", &soco_body()),
            fixture::block(b"vmsk", &body),
        ]
        .concat();
        let path = dir.path().join(format!("{what}.psd"));
        std::fs::write(&path, fixture::fixture(&[record], [64, 48], false)).unwrap();
        let losses = common::psd_photoshop::strings(&import_report(&path), "verified_losses");
        assert!(
            losses.iter().any(|l| l.contains(DROPPED)),
            "{what}: {losses:?}"
        );
        let doc = byte_open(&path);
        assert!(unsafe { &*doc }.vector_mask(0).is_none(), "{what}");
        assert!(
            !common::ffi_meta(doc, 0).is_some_and(|m| m.contains("\"type\":\"fill\"")),
            "{what}: no fill layer"
        );
        let flat = common::flat_pixels(doc);
        assert_eq!(common::pixel(&flat, 64, 12, 12)[3], 255, "{what}: the ring");
        assert_eq!(common::pixel(&flat, 64, 32, 24)[3], 0, "{what}: the hole");
        unsafe { dr_doc_free(doc) };
    }
}

/// The file's paths and its vector masks share the document's one anchor
/// cap (65,536). Two saved paths of 32,768 anchors fill it, so a raster
/// record's 4-anchor `vmsk` is not carried: it is drawn into the layer's
/// raster mask with its sentence, both paths are kept, and the opened
/// document saves as `.doctored`, which refuses a document past the cap.
#[test]
fn a_vector_mask_past_the_anchor_cap_the_paths_fill_is_not_carried() {
    let canvas = (64.0, 48.0);
    let full: Vec<(f64, f64)> = (0..32_768u32)
        .map(|k| (f64::from(k % 64), f64::from(k / 64 % 48)))
        .collect();
    let path = path_body(0, &[closed_subpath(&full, 1, canvas)]);
    let resources = [
        path_resource(2000, "A", &path),
        path_resource(2001, "B", &path),
    ]
    .concat();
    let mut record =
        fixture::Record::raster([0, 0, 48, 64], &[9, 9, 9, 255].repeat(64 * 48), false);
    let rectangle = closed_subpath(
        &[(8.0, 8.0), (56.0, 8.0), (56.0, 40.0), (8.0, 40.0)],
        1,
        canvas,
    );
    record.blocks = fixture::block(b"vmsk", &vmsk(0, &[rectangle]));
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("cap.psd");
    std::fs::write(
        &file,
        with_resources(&fixture::fixture(&[record], [64, 48], false), &resources),
    )
    .unwrap();
    // Not carried, the path is drawn into a raster mask with its sentence.
    let losses = common::psd_photoshop::strings(&import_report(&file), "verified_losses");
    assert!(
        losses
            .iter()
            .any(|l| l.contains("is imported as a raster mask drawn from its path")),
        "{losses:?}"
    );
    let doc = byte_open(&file);
    let document = unsafe { &*doc };
    assert!(document.vector_mask(0).is_none());
    let flat = common::flat_pixels(doc);
    assert_eq!(
        common::pixel(&flat, 64, 30, 20)[3],
        255,
        "inside the rectangle"
    );
    assert_eq!(common::pixel(&flat, 64, 2, 2)[3], 0, "outside it");
    assert_eq!(document.paths().len(), 2, "both paths are kept");
    let native = dir.path().join("cap.doctored");
    document
        .save_native(native.to_str().unwrap())
        .expect("the opened file saves");
    unsafe { dr_doc_free(doc) };
}

/// A raster record (the fixture's “Duplicate”) over a `canvas`, under `body`
/// as its `vmsk`.
fn masked_raster(canvas: [u32; 2], body: &[u8]) -> Vec<u8> {
    let mut record = fixture::Record::raster([0, 0, 4, 4], &[9, 9, 9, 255].repeat(16), false);
    record.blocks = fixture::block(b"vmsk", body);
    fixture::fixture(&[record], canvas, false)
}

/// The import draws a vector mask it does not carry one subpath at a time,
/// each a pass over the canvas, so it draws only a mask within the path
/// caps (4,096 subpaths, 32,768 anchors) and within 2³⁰ samples of passes;
/// any other is dropped with the dropped-mask sentence and never drawn.
#[test]
fn a_vector_mask_past_the_drawing_caps_is_dropped_with_its_sentence() {
    let canvas = (64.0, 48.0);
    let triangle = |k: usize, operation: i16| {
        let x = (k % 60) as f64;
        closed_subpath(
            &[(x, 2.0), (x + 3.0, 2.0), (x + 1.5, 40.0)],
            operation,
            canvas,
        )
    };
    // Two subpaths, the second subtracting: drawn on a small canvas, and not
    // on one where two passes are past the sample cap.
    let two = vmsk(0, &[triangle(0, 1), triangle(1, 2)]);
    assert!(doctored_core::psd_paths::vmsk_bakes(&two, (64, 48)));
    assert!(!doctored_core::psd_paths::vmsk_bakes(
        &two,
        (30_000, 30_000)
    ));
    // One subpath past the subpath cap.
    let many: Vec<Vec<u8>> = (0..4097)
        .map(|k| triangle(k, if k == 0 { 1 } else { 2 }))
        .collect();
    let body = vmsk(0, &many);
    assert!(!doctored_core::psd_paths::vmsk_bakes(&body, (64, 48)));
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("many.psd");
    std::fs::write(&file, masked_raster([64, 48], &body)).unwrap();
    let losses = common::psd_photoshop::strings(&import_report(&file), "verified_losses");
    assert!(losses.iter().any(|l| l.contains(DROPPED)), "{losses:?}");
    assert!(
        !losses.iter().any(|l| l.contains("drawn from its path")),
        "{losses:?}"
    );
    let doc = byte_open(&file);
    assert!(
        unsafe { &*doc }.layers[0].mask.is_none(),
        "no mask is drawn"
    );
    unsafe { dr_doc_free(doc) };
}

/// A vector mask within the drawing caps that the rasterizer refuses (32,760
/// edges that each cross all 8,000 rows are past its work limit) refuses the
/// layered open by name: the report said the mask is drawn, so the layer
/// must not open unmasked.
#[test]
fn a_vector_mask_the_rasterizer_refuses_refuses_the_layered_open() {
    let canvas = (8.0, 8000.0);
    let zigzag: Vec<(f64, f64)> = (0..32_760u32)
        .map(|k| (f64::from(k % 8), if k % 2 == 0 { 0.0 } else { 8000.0 }))
        .collect();
    let hole = closed_subpath(&[(1.0, 10.0), (6.0, 10.0), (3.0, 90.0)], 2, canvas);
    let body = vmsk(0, &[closed_subpath(&zigzag, 1, canvas), hole]);
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("zigzag.psd");
    std::fs::write(&file, masked_raster([8, 8000], &body)).unwrap();
    let report = import_report(&file);
    let losses = common::psd_photoshop::strings(&report, "verified_losses");
    assert!(
        losses.iter().any(|l| l.contains("drawn from its path")),
        "{losses:?}"
    );
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe {
        doctored_core::ffi_psd::dr_doc_open_psd(
            cpath(&file).as_ptr(),
            LAYERS,
            token.as_ptr(),
            ptr::null(),
            &mut err,
        )
    };
    if !doc.is_null() {
        let masked = unsafe { &*doc }.layers[0].mask.is_some();
        unsafe { dr_doc_free(doc) };
        panic!("the open must refuse; it opened with a mask: {masked}");
    }
    let reason = take_err_string(err);
    assert!(
        reason.contains("PSD vector mask on layer “Duplicate” could not be drawn"),
        "{reason}"
    );
}
