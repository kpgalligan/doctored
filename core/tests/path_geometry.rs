//! PATHS as document state at 8, 16 and 32-bit float — the shape JSON
//! and its caps, the list rules and their sentences, path identity, and
//! every canvas-geometry op at each depth.
//!
//! The geometry oracles are written out here: each op's canvas map as an
//! affine matrix `[a, b, c, d, tx, ty]` (`x' = a·x + c·y + tx`,
//! `y' = b·x + d·y + ty`) worked out by hand from what the op does to a
//! pixel, the filtered turn as a rotation about the canvas centre shifted to
//! its own bounding box, and the perspective crop as the homography
//! `common/perspective_oracle.rs` solves. None of them is read from the
//! core. The `pub` helpers are shared with `path_format.rs` and
//! `path_shape.rs`, which holds the shape operations (coverage, flatten,
//! hit, insert, convert).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_path::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::path_geometry::{
    Anchor, FillRule, PathShape, Subpath, MAX_COORDINATE, MAX_PATH_ANCHORS, MAX_SUBPATHS,
};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{byte_geometry_edit, take_err_string};
use crate::perspective_oracle::{project, solve};

pub const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 64;
const H: u32 = 48;
/// A mapped coordinate is quantized to four decimals, so it may sit half a
/// step from the oracle's.
const TOLERANCE: f64 = 1e-4;

// ---------------------------------------------------------------- helpers --

pub fn byte(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([200, 100, 50, 255])))
}

pub fn typed(w: u32, h: u32) -> PrecisionDocument {
    let pixels = vec![[12345u16, 23456, 34567, 65535]; (w * h) as usize];
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

/// A two-record 32-bit float document.
pub fn float(w: u32, h: u32) -> PrecisionDocument {
    let doc = crate::float_fixture::shaped_pair(w, h);
    assert!(doc.is_float());
    doc
}

pub fn take(text: *mut c_char) -> String {
    assert!(!text.is_null());
    let owned = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    owned
}

/// The setter's three answers: a new document, `Ok(None)` for NULL with no
/// message (unchanged), `Err` for NULL with a sentence.
pub fn set_byte(doc: &DrDocument, list: &Value) -> Result<Option<DrDocument>, String> {
    let text = CString::new(list.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_with_paths_json(doc, text.as_ptr(), &mut error) };
    if !out.is_null() {
        assert!(error.is_null());
        return Ok(Some(*unsafe { Box::from_raw(out) }));
    }
    if error.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(error))
    }
}

pub fn set_typed(
    doc: &PrecisionDocument,
    list: &Value,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(list.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_with_paths_json(doc, text.as_ptr(), &mut error) };
    if !out.is_null() {
        assert!(error.is_null());
        return Ok(Some(*unsafe { Box::from_raw(out) }));
    }
    if error.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(error))
    }
}

/// The document a setter or an edit answered, which must be a change.
pub fn changed<D>(result: Result<Option<D>, String>) -> D {
    match result {
        Ok(Some(document)) => document,
        Ok(None) => panic!("unchanged, where a new document was expected"),
        Err(message) => panic!("refused: {message}"),
    }
}

/// The sentence a setter or an edit refused with.
pub fn refused<D>(result: Result<Option<D>, String>) -> String {
    match result {
        Err(message) => message,
        Ok(Some(_)) => panic!("accepted, where a refusal was expected"),
        Ok(None) => panic!("unchanged with no sentence, where a refusal was expected"),
    }
}

/// Whether a setter or an edit answered NULL with no message.
pub fn unchanged<D>(result: Result<Option<D>, String>) -> bool {
    matches!(result, Ok(None))
}

pub fn with_byte(doc: &DrDocument, list: &Value) -> DrDocument {
    changed(set_byte(doc, list))
}

pub fn with_typed(doc: &PrecisionDocument, list: &Value) -> PrecisionDocument {
    changed(set_typed(doc, list))
}

pub fn byte_paths(doc: &DrDocument) -> Value {
    serde_json::from_str(&take(unsafe { dr_doc_paths_json(doc) })).expect("path-list JSON")
}

pub fn typed_paths(doc: &PrecisionDocument) -> Value {
    serde_json::from_str(&take(unsafe { dr_precision_doc_paths_json(doc) }))
        .expect("path-list JSON")
}

/// The entries of a path-list JSON.
pub fn entries(list: &Value) -> &Vec<Value> {
    list["paths"].as_array().expect("a paths array")
}

pub fn ids(list: &Value) -> Vec<u64> {
    entries(list)
        .iter()
        .map(|e| e["id"].as_u64().expect("an id"))
        .collect()
}

/// The list with every `id` removed — what a save keeps.
pub fn without_ids(list: &Value) -> Value {
    let mut list = list.clone();
    for entry in list["paths"].as_array_mut().expect("a paths array") {
        entry.as_object_mut().unwrap().remove("id");
    }
    list
}

/// Every point of a list in file order: each anchor's point, `in`, `out`.
pub fn points(list: &Value) -> Vec<[f64; 2]> {
    let pair = |v: &Value| [v[0].as_f64().unwrap(), v[1].as_f64().unwrap()];
    let mut out = Vec::new();
    for entry in entries(list) {
        for subpath in entry["subpaths"].as_array().unwrap() {
            for anchor in subpath["anchors"].as_array().unwrap() {
                out.push([anchor["x"].as_f64().unwrap(), anchor["y"].as_f64().unwrap()]);
                out.push(pair(&anchor["in"]));
                out.push(pair(&anchor["out"]));
            }
        }
    }
    out
}

/// A corner-only subpath.
pub fn polygon(corners: &[[f64; 2]], closed: bool) -> Value {
    json!({"closed": closed, "anchors": corners})
}

pub fn saved(name: &str, subpaths: Value) -> Value {
    json!({"kind": "saved", "name": name, "subpaths": subpaths})
}

/// The two paths every geometry case starts from: a curved work path inside
/// the canvas and a saved one that lies outside it on both sides.
pub fn start() -> Value {
    json!({"paths": [
        {"kind": "work", "fill_rule": "evenodd", "subpaths": [{"closed": true, "anchors": [
            {"x": 10.5, "y": 20.25, "in": [8.0, 18.0], "out": [13.0, 22.5], "smooth": true},
            {"x": 40.0, "y": 5.0},
            [60.75, 44.5],
        ]}]},
        {"kind": "saved", "name": "Outside", "subpaths": [{"closed": false, "anchors": [
            {"x": -20.5, "y": 70.25, "out": [-25.0, 80.0]},
            {"x": 100.0, "y": -3.5, "in": [90.0, -10.0]},
        ]}]},
    ]})
}

fn typed_edit(
    doc: &PrecisionDocument,
    request: &Value,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !out.is_null() {
        return Ok(Some(*unsafe { Box::from_raw(out) }));
    }
    if error.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(error))
    }
}

fn byte_edit(doc: &DrDocument, request: &Value) -> Option<DrDocument> {
    let (out, error) = byte_geometry_edit(doc, request);
    assert_eq!(error, None, "{request}");
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn affine(m: [f64; 6]) -> impl Fn([f64; 2]) -> [f64; 2] {
    move |p| {
        [
            m[0] * p[0] + m[2] * p[1] + m[4],
            m[1] * p[0] + m[3] * p[1] + m[5],
        ]
    }
}

/// `after` is `before` with every point through `map`, and nothing else
/// about the list — ids, names, kinds, flags, order — has changed.
fn assert_mapped(before: &Value, after: &Value, map: impl Fn([f64; 2]) -> [f64; 2], what: &str) {
    assert_eq!(ids(before), ids(after), "{what}: identity is kept");
    let (from, to) = (points(before), points(after));
    assert_eq!(from.len(), to.len(), "{what}: no point is dropped");
    for (p, q) in from.iter().zip(&to) {
        let want = map(*p);
        assert!(
            (want[0] - q[0]).abs() <= TOLERANCE && (want[1] - q[1]).abs() <= TOLERANCE,
            "{what}: {p:?} should land on {want:?}, not {q:?}"
        );
    }
    assert_eq!(skeleton(before), skeleton(after), "{what}");
}

/// A list with every coordinate blanked: what a point map must leave alone.
fn skeleton(list: &Value) -> Value {
    let mut list = list.clone();
    for entry in list["paths"].as_array_mut().unwrap() {
        for subpath in entry["subpaths"].as_array_mut().unwrap() {
            for anchor in subpath["anchors"].as_array_mut().unwrap() {
                for key in ["x", "y", "in", "out"] {
                    anchor[key] = Value::Null;
                }
            }
        }
    }
    list
}

fn shape(v: Value) -> Result<PathShape, String> {
    PathShape::from_value(&v)
}

// ---------------------------------------------------------- the shape (T1) --

#[test]
fn the_shape_json_reads_its_defaults_quantizes_and_round_trips() {
    let read = shape(json!({"subpaths": [
        {"anchors": [{"x": 1.00004, "y": 2.00006}, [3.5, -4.25]]},
        {"closed": true, "anchors": [
            {"x": 5, "y": 6, "in": [4.123449, 6], "out": [7, 8.5], "smooth": true},
        ]},
    ]}))
    .expect("a shape");
    assert_eq!(read.fill_rule, FillRule::NonZero);
    assert_eq!(read.anchor_count(), 3);
    let first = &read.subpaths[0];
    assert!(!first.closed, "closed defaults to false");
    // Four decimals: 1.00004 rounds down, 2.00006 up.
    assert_eq!(first.anchors[0], Anchor::corner([1.0, 2.0001]));
    assert_eq!(first.anchors[1], Anchor::corner([3.5, -4.25]));
    let curve = &read.subpaths[1];
    assert!(curve.closed);
    assert_eq!(
        curve.anchors[0],
        Anchor {
            point: [5.0, 6.0],
            handle_in: [4.1234, 6.0],
            handle_out: [7.0, 8.5],
            smooth: true,
        }
    );

    // The written form names every key, and reading it back is the shape.
    let written = read.to_value();
    assert_eq!(
        written,
        json!({"fill_rule": "nonzero", "subpaths": [
            {"closed": false, "anchors": [
                {"x": 1.0, "y": 2.0001, "in": [1.0, 2.0001], "out": [1.0, 2.0001], "smooth": false},
                {"x": 3.5, "y": -4.25, "in": [3.5, -4.25], "out": [3.5, -4.25], "smooth": false},
            ]},
            {"closed": true, "anchors": [
                {"x": 5.0, "y": 6.0, "in": [4.1234, 6.0], "out": [7.0, 8.5], "smooth": true},
            ]},
        ]})
    );
    assert_eq!(shape(written).expect("the echo"), read);

    let even = shape(json!({"fill_rule": "evenodd", "subpaths": []})).expect("an empty shape");
    assert_eq!(even.fill_rule, FillRule::EvenOdd);
    assert_eq!(even.anchor_count(), 0);
    assert_eq!(even.bounds(), None);
    assert_eq!(even.commands((0.0, 0.0)), (vec![], vec![]));
    assert_eq!(PathShape::default().to_value()["fill_rule"], "nonzero");
}

#[test]
fn the_shape_refuses_what_the_model_cannot_hold() {
    let one = |anchor: Value| json!({"subpaths": [{"anchors": [anchor]}]});
    let cases: Vec<(Value, &str)> = vec![
        (json!([]), "must be an object"),
        (json!({}), "subpaths array"),
        (json!({"fill_rule": "winding", "subpaths": []}), "fill_rule"),
        (
            json!({"subpaths": [{"anchors": []}]}),
            "at least one anchor",
        ),
        (json!({"subpaths": [{"closed": true}]}), "anchors array"),
        (
            json!({"subpaths": [{"closed": 1, "anchors": [[0, 0]]}]}),
            "closed",
        ),
        (one(json!({"x": 1})), "needs y"),
        (one(json!({"x": "1", "y": 2})), "must be a number"),
        (one(json!({"x": null, "y": 2})), "must be a number"),
        (one(json!([1, 2, 3])), "[x, y] pair"),
        (one(json!({"x": 1, "y": 2, "in": [1]})), "[x, y] pair"),
        (one(json!({"x": 1, "y": 2, "smooth": "yes"})), "smooth"),
        (one(json!({"x": 1000000.0001, "y": 0})), "outside"),
        (one(json!({"x": 0, "y": -1000000.0001})), "outside"),
        (one(json!({"x": 0, "y": 0, "out": [0, 2e6]})), "outside"),
    ];
    for (value, sentence) in cases {
        let message = shape(value.clone()).expect_err("refused");
        assert!(message.contains(sentence), "{value}: {message}");
    }
    // The wall itself is inside, and a value that quantizes onto it too.
    let wall = shape(one(json!({"x": MAX_COORDINATE, "y": -1000000.00004}))).expect("the wall");
    assert_eq!(
        wall.subpaths[0].anchors[0].point,
        [1_000_000.0, -1_000_000.0]
    );

    // The two per-shape caps, each at its limit and one past it.
    let corner = json!([1.5, 2.5]);
    let full_subpaths = json!({"subpaths": vec![json!({"anchors": [corner]}); MAX_SUBPATHS]});
    assert_eq!(
        shape(full_subpaths).expect("4096 subpaths").subpaths.len(),
        MAX_SUBPATHS
    );
    let over = json!({"subpaths": vec![json!({"anchors": [corner]}); MAX_SUBPATHS + 1]});
    assert_eq!(
        shape(over).unwrap_err(),
        "too many subpaths in a path (max 4096)"
    );
    let full = json!({"subpaths": [{"anchors": vec![corner.clone(); MAX_PATH_ANCHORS]}]});
    assert_eq!(
        shape(full).expect("32768 anchors").anchor_count(),
        MAX_PATH_ANCHORS
    );
    let over = json!({"subpaths": [
        {"anchors": vec![corner.clone(); MAX_PATH_ANCHORS]},
        {"anchors": [corner]},
    ]});
    assert_eq!(
        shape(over).unwrap_err(),
        "too many anchors in a path (max 32768)"
    );

    // A shape built in memory answers the same rules.
    let nan = PathShape {
        subpaths: vec![Subpath {
            anchors: vec![Anchor::corner([f64::NAN, 0.0])],
            closed: false,
        }],
        fill_rule: FillRule::NonZero,
    };
    assert!(nan.refusal().is_some());
    assert!(nan.validated().is_err());
    let hollow = PathShape {
        subpaths: vec![Subpath {
            anchors: Vec::new(),
            closed: true,
        }],
        fill_rule: FillRule::NonZero,
    };
    assert_eq!(
        hollow.refusal().as_deref(),
        Some("a subpath needs at least one anchor")
    );
}

#[test]
fn the_outline_verbs_follow_the_segments_and_close_every_subpath() {
    // A closed triangle of corners, an open two-anchor curve, a one-anchor
    // loop: 0 move, 1 line, 3 cubic, 4 close.
    let read = shape(json!({"subpaths": [
        {"closed": true, "anchors": [[10, 10], [30, 10], [20, 25]]},
        {"closed": false, "anchors": [
            {"x": 40, "y": 40, "out": [45, 35]},
            {"x": 60, "y": 40, "in": [55, 35]},
        ]},
        {"closed": true, "anchors": [{"x": 5, "y": 50, "in": [0, 60], "out": [10, 60]}]},
        {"closed": true, "anchors": [[70, 70]]},
    ]}))
    .unwrap();
    let (verbs, points) = read.commands((10.0, 5.0));
    assert_eq!(
        verbs,
        vec![
            0, 1, 1, 1, 4, // the triangle and its closing line
            0, 3, 4, // the open curve, closed straight for fill
            0, 3, 4, // the loop through its own handles
            0, 4, // a lone corner has no segment
        ]
    );
    let shifted: Vec<[f64; 2]> = [
        [10.0, 10.0],
        [30.0, 10.0],
        [20.0, 25.0],
        [10.0, 10.0],
        [40.0, 40.0],
        [45.0, 35.0],
        [55.0, 35.0],
        [60.0, 40.0],
        [5.0, 50.0],
        [10.0, 60.0],
        [0.0, 60.0],
        [5.0, 50.0],
        [70.0, 70.0],
    ]
    .iter()
    .map(|p| [p[0] - 10.0, p[1] - 5.0])
    .collect();
    assert_eq!(points, shifted);
    // The control box holds every point and handle.
    assert_eq!(read.bounds(), Some([0.0, 10.0, 70.0, 70.0]));
}

#[test]
fn an_affine_map_moves_every_point_and_handle_and_lands_on_the_wall() {
    let read = shape(
        json!({"fill_rule": "evenodd", "subpaths": [{"closed": true, "anchors": [
            {"x": 10.5, "y": 20.25, "in": [8, 18], "out": [13, 22.5], "smooth": true},
            [40, 5],
        ]}]}),
    )
    .unwrap();
    // A scale, a shear and a shift, applied by hand.
    let m = [1.5, 0.25, -0.5, 2.0, 7.0, -3.0];
    let map = affine(m);
    let moved = read.transformed(m);
    assert_eq!(moved.fill_rule, FillRule::EvenOdd);
    assert!(moved.subpaths[0].closed && moved.subpaths[0].anchors[0].smooth);
    for (a, b) in read.subpaths[0]
        .anchors
        .iter()
        .zip(&moved.subpaths[0].anchors)
    {
        for (p, q) in [
            (a.point, b.point),
            (a.handle_in, b.handle_in),
            (a.handle_out, b.handle_out),
        ] {
            let want = map(p);
            assert!((want[0] - q[0]).abs() <= TOLERANCE && (want[1] - q[1]).abs() <= TOLERANCE);
        }
    }
    assert_eq!(read.transformed([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]), read);

    // A map that leaves the coordinate wall, or has no finite answer, keeps
    // the anchor: it lands on the wall (0 for a NaN) and stays storable.
    let far = read.transformed([1e9, 0.0, 0.0, -1e9, 0.0, 0.0]);
    assert_eq!(
        far.subpaths[0].anchors[1].point,
        [1_000_000.0, -1_000_000.0]
    );
    assert_eq!(far.refusal(), None);
    let broken = read.mapped(|x, _| (f64::NAN, x / 0.0));
    assert_eq!(broken.subpaths[0].anchors[1].point, [0.0, 1_000_000.0]);
    assert_eq!(broken.anchor_count(), read.anchor_count());
    assert_eq!(broken.refusal(), None);
}

// ----------------------------------------------------------- the list (T2) --

#[test]
fn the_setter_mints_ids_puts_the_work_path_first_and_refuses_the_same_list() {
    let b = byte(W, H);
    let t = typed(W, H);
    assert_eq!(byte_paths(&b), json!({"paths": []}));
    assert_eq!(typed_paths(&t), json!({"paths": []}));
    assert!(
        unchanged(set_byte(&b, &json!({"paths": []}))),
        "clearing nothing"
    );

    // The work path arrives second and unnamed; it is stored first, named.
    let list = json!({"paths": [
        saved("Outline", json!([polygon(&[[1.0, 2.0], [30.0, 4.0], [9.0, 40.0]], true)])),
        {"kind": "work", "subpaths": [polygon(&[[5.0, 5.0], [6.0, 7.0]], false)]},
        saved("Empty", json!([])),
    ]});
    let set = with_byte(&b, &list);
    let stored = byte_paths(&set);
    let names: Vec<&str> = entries(&stored)
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Work Path", "Outline", "Empty"]);
    assert_eq!(entries(&stored)[0]["kind"], "work");
    let minted = ids(&stored);
    assert!(minted.iter().all(|id| *id != 0), "{minted:?}");
    assert_ne!(minted[0], minted[1]);
    assert_ne!(minted[1], minted[2]);
    assert_ne!(minted[0], minted[2]);
    assert_eq!(
        byte_paths(&b),
        json!({"paths": []}),
        "the input is untouched"
    );
    assert_eq!(dr_max_paths(), 256);

    // The typed twin stores the same list.
    let set16 = with_typed(&t, &list);
    assert_eq!(without_ids(&typed_paths(&set16)), without_ids(&stored));

    // What was read back is the list the document holds: not an edit.
    assert!(unchanged(set_byte(&set, &stored)));
    assert!(unchanged(set_typed(&set16, &typed_paths(&set16))));

    // An edit keeps every id it names; a new entry and a repeated id mint.
    let mut edited = stored.clone();
    edited["paths"][1]["name"] = json!("Renamed");
    edited["paths"][1]["subpaths"][0]["anchors"][0]["x"] = json!(2.5);
    let mut copy = edited["paths"][1].clone();
    copy["name"] = json!("Renamed copy");
    edited["paths"].as_array_mut().unwrap().push(copy);
    edited["paths"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind": "saved", "name": "New", "id": 0, "subpaths": []}));
    let next = byte_paths(&with_byte(&set, &edited));
    let next_ids = ids(&next);
    assert_eq!(&next_ids[..3], &minted[..]);
    assert!(!minted.contains(&next_ids[3]) && !minted.contains(&next_ids[4]));
    assert_ne!(next_ids[3], next_ids[4]);
    assert_eq!(entries(&next)[1]["name"], "Renamed");
    assert_eq!(entries(&next)[1]["subpaths"][0]["anchors"][0]["x"], 2.5);

    // Deleting everything is one call, and the model's Rust reader agrees.
    let cleared = with_byte(&set, &json!({"paths": []}));
    assert!(cleared.paths().is_empty());
    assert_eq!(set.paths().len(), 3);
    assert_eq!(set.paths()[0].name, doctored_core::doc_path::WORK_PATH_NAME);
}

#[test]
fn the_list_rules_refuse_by_sentence_at_both_depths() {
    let b = byte(W, H);
    let t = typed(W, H);
    let corner = json!([polygon(&[[1.0, 1.0]], false)]);
    let many: Vec<Value> = (0..257)
        .map(|i| saved(&format!("p{i}"), json!([])))
        .collect();
    let cases: Vec<(Value, &str)> = vec![
        (json!({"paths": many}), "too many paths (max 256)"),
        (
            json!({"paths": [{"kind": "work", "subpaths": []}, {"kind": "work", "subpaths": []}]}),
            "a document holds at most one work path",
        ),
        (
            json!({"paths": [saved("", corner.clone())]}),
            "a saved path needs a name",
        ),
        (
            json!({"paths": [saved(&"n".repeat(256), corner.clone())]}),
            "a path name is longer than 255 bytes",
        ),
        (
            json!({"paths": [saved("Work Path", corner.clone())]}),
            "a saved path cannot be named \"Work Path\"",
        ),
        (
            json!({"paths": [saved("Twin", corner.clone()), saved("Twin", json!([]))]}),
            "two paths are named \"Twin\"",
        ),
    ];
    for (list, sentence) in &cases {
        assert_eq!(refused(set_byte(&b, list)), *sentence);
        assert_eq!(refused(set_typed(&t, list)), *sentence);
    }
    // Malformed requests and refused shapes name themselves too.
    for (list, sentence) in [
        (json!({}), "paths array"),
        (json!({"paths": [{"name": "x", "subpaths": []}]}), "kind"),
        (
            json!({"paths": [{"kind": "saved", "subpaths": []}]}),
            "name",
        ),
        (
            json!({"paths": [{"kind": "saved", "name": "x", "id": -1, "subpaths": []}]}),
            "id",
        ),
        (
            json!({"paths": [saved("x", json!([{"anchors": []}]))]}),
            "at least one anchor",
        ),
    ] {
        let message = refused(set_byte(&b, &list));
        assert!(message.contains(sentence), "{list}: {message}");
        assert!(refused(set_typed(&t, &list)).contains(sentence));
    }
    let mut error = ptr::null_mut();
    let garbage = CString::new("{not json").unwrap();
    assert!(unsafe { dr_doc_with_paths_json(&b, garbage.as_ptr(), &mut error) }.is_null());
    assert!(take_err_string(error).contains("malformed"));
    let mut error = ptr::null_mut();
    assert!(unsafe { dr_doc_with_paths_json(&b, ptr::null(), &mut error) }.is_null());
    assert!(take_err_string(error).contains("NULL"));

    // The limits themselves are inside: 256 paths, a 255-byte name.
    let full: Vec<Value> = (0..255)
        .map(|i| saved(&format!("p{i}"), json!([])))
        .chain([saved(&"n".repeat(255), corner)])
        .collect();
    assert_eq!(with_byte(&b, &json!({"paths": full})).paths().len(), 256);
}

// ------------------------------------------------------- the geometry (T3) --

/// One exact op: its name, request, result canvas and hand-written matrix.
type GeometryCase = (&'static str, Value, (u32, u32), [f64; 6]);

fn geometry_cases() -> Vec<GeometryCase> {
    let (w, h) = (f64::from(W), f64::from(H));
    vec![
        // A quarter turn clockwise sends (x, y) to (H − y, x).
        (
            "rotate90",
            json!({"op": "rotate90"}),
            (H, W),
            [0., 1., -1., 0., h, 0.],
        ),
        (
            "rotate180",
            json!({"op": "rotate180"}),
            (W, H),
            [-1., 0., 0., -1., w, h],
        ),
        // Counter-clockwise: (x, y) to (y, W − x).
        (
            "rotate270",
            json!({"op": "rotate270"}),
            (H, W),
            [0., -1., 1., 0., 0., w],
        ),
        (
            "flip_horizontal",
            json!({"op": "flip_horizontal"}),
            (W, H),
            [-1., 0., 0., 1., w, 0.],
        ),
        (
            "flip_vertical",
            json!({"op": "flip_vertical"}),
            (W, H),
            [1., 0., 0., -1., 0., h],
        ),
        (
            "crop",
            json!({"op": "crop", "x": 8, "y": 4, "width": 40, "height": 30}),
            (40, 30),
            [1., 0., 0., 1., -8., -4.],
        ),
        (
            "canvas_size",
            json!({"op": "canvas_size", "width": 50, "height": 40, "x": -10, "y": -5}),
            (50, 40),
            [1., 0., 0., 1., -10., -5.],
        ),
        (
            "resize",
            json!({"op": "resize", "width": 100, "height": 30, "filter": "bilinear"}),
            (100, 30),
            [100. / 64., 0., 0., 30. / 48., 0., 0.],
        ),
    ]
}

#[test]
fn every_exact_geometry_op_maps_the_paths_by_its_matrix_at_every_depth() {
    let b = with_byte(&byte(W, H), &start());
    let t = with_typed(&typed(W, H), &start());
    let f = with_typed(&float(W, H), &start());
    let (before8, before16, before32) = (byte_paths(&b), typed_paths(&t), typed_paths(&f));
    for (name, request, canvas, m) in geometry_cases() {
        let moved = byte_edit(&b, &request).unwrap_or_else(|| panic!("byte {name}"));
        assert_eq!((moved.width, moved.height), canvas, "{name}");
        assert_mapped(
            &before8,
            &byte_paths(&moved),
            affine(m),
            &format!("byte {name}"),
        );
        let moved16 = typed_edit(&t, &request).unwrap().expect(name);
        assert_eq!(moved16.dimensions(), canvas, "{name}");
        assert_mapped(
            &before16,
            &typed_paths(&moved16),
            affine(m),
            &format!("16-bit {name}"),
        );
        let moved32 = typed_edit(&f, &request).unwrap().expect(name);
        assert_eq!(moved32.dimensions(), canvas, "{name}");
        assert_mapped(
            &before32,
            &typed_paths(&moved32),
            affine(m),
            &format!("float {name}"),
        );
    }
    // The crop keeps the path that now lies wholly outside the window.
    let cropped = byte_paths(&b.crop(8, 4, 40, 30).expect("crop"));
    assert_eq!(entries(&cropped).len(), 2);
    assert_eq!(
        entries(&cropped)[1]["subpaths"][0]["anchors"][0]["x"],
        -28.5
    );
    // The byte methods the host's flip and rotate items call agree.
    assert_mapped(
        &before8,
        &byte_paths(&b.rotate90().expect("turn")),
        affine(geometry_cases()[0].3),
        "rotate90()",
    );
}

#[test]
fn a_filtered_turn_and_a_straighten_map_the_paths_with_the_content() {
    let b = with_byte(&byte(W, H), &start());
    let t = with_typed(&typed(W, H), &start());

    // Rotate Canvas by 10°: clockwise on a y-down canvas about the canvas
    // centre, then shifted so the old canvas's top-left lands at
    // (floor((W'-W)/2), floor((H'-H)/2)), W' x H' the turned corners' extent
    // rounded up.
    let (s, c) = 10f64.to_radians().sin_cos();
    let (cx, cy) = (f64::from(W) / 2.0, f64::from(H) / 2.0);
    let turn = |p: [f64; 2]| {
        let (dx, dy) = (p[0] - cx, p[1] - cy);
        [c * dx - s * dy + cx, s * dx + c * dy + cy]
    };
    let corners = [[0.0, 0.0], [64.0, 0.0], [0.0, 48.0], [64.0, 48.0]].map(turn);
    let start_of = |axis: usize, old: u32| {
        let min = corners.iter().map(|p| p[axis]).fold(f64::MAX, f64::min);
        let max = corners.iter().map(|p| p[axis]).fold(f64::MIN, f64::max);
        let grown = (max - min).ceil() as i64;
        -((grown - i64::from(old)).div_euclid(2) as f64)
    };
    let (left, top) = (start_of(0, W), start_of(1, H));
    let expected = |p: [f64; 2]| {
        let q = turn(p);
        [q[0] - left, q[1] - top]
    };
    let request = json!({"op": "rotate_canvas", "degrees": 10, "filter": "bilinear"});
    let moved = byte_edit(&b, &request).expect("the byte turn");
    assert!(moved.width > W && moved.height > H);
    assert_mapped(
        &byte_paths(&b),
        &byte_paths(&moved),
        expected,
        "byte rotate_canvas",
    );
    let moved16 = typed_edit(&t, &request).unwrap().expect("the typed turn");
    assert_eq!(moved16.dimensions(), (moved.width, moved.height));
    assert_mapped(
        &typed_paths(&t),
        &typed_paths(&moved16),
        expected,
        "typed rotate_canvas",
    );

    // A whole-canvas quarter turn asked for as an angle is the exact op.
    let quarter = json!({"op": "rotate_canvas", "degrees": 90, "filter": "bilinear"});
    assert_mapped(
        &byte_paths(&b),
        &byte_paths(&byte_edit(&b, &quarter).expect("a quarter")),
        affine(geometry_cases()[0].3),
        "rotate_canvas 90",
    );

    // Straighten: a 3° turn about the centre (CGAffineTransform order), then
    // the crop window's shift.
    let (s, c) = 3f64.to_radians().sin_cos();
    let m = [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c];
    let (wx, wy, ww, wh) = (3u32, 2u32, 58u32, 44u32);
    let request = json!({"op": "straighten", "matrix": m, "filter": "bilinear",
                         "x": wx, "y": wy, "width": ww, "height": wh});
    let windowed = [
        m[0],
        m[1],
        m[2],
        m[3],
        m[4] - f64::from(wx),
        m[5] - f64::from(wy),
    ];
    let moved = byte_edit(&b, &request).expect("the byte straighten");
    assert_eq!((moved.width, moved.height), (ww, wh));
    assert_mapped(
        &byte_paths(&b),
        &byte_paths(&moved),
        affine(windowed),
        "byte straighten",
    );
    let moved16 = typed_edit(&t, &request)
        .unwrap()
        .expect("the typed straighten");
    assert_mapped(
        &typed_paths(&t),
        &typed_paths(&moved16),
        affine(windowed),
        "typed straighten",
    );
}

#[test]
fn a_perspective_crop_maps_the_paths_by_its_homography() {
    let list = json!({"paths": [
        saved("Inside", json!([{"closed": true, "anchors": [
            {"x": 10.0, "y": 8.0, "out": [14.0, 9.5]},
            {"x": 20.5, "y": 12.25, "in": [22.0, 14.0]},
            [1.0, 1.0],
        ]}])),
    ]});
    let quad = [5.3, 3.7, 26.1, 2.2, 29.4, 21.6, 2.8, 19.9];
    let (w, h) = (20u32, 15u32);
    let request = json!({"op": "perspective_crop", "quad": quad, "width": w, "height": h,
                         "filter": "bilinear"});
    let homography = solve(
        [
            (quad[0], quad[1]),
            (quad[2], quad[3]),
            (quad[4], quad[5]),
            (quad[6], quad[7]),
        ],
        [
            (0.0, 0.0),
            (f64::from(w), 0.0),
            (f64::from(w), f64::from(h)),
            (0.0, f64::from(h)),
        ],
    );
    let expected = |p: [f64; 2]| {
        let (x, y) = project(&homography, p[0], p[1]);
        [x, y]
    };
    let b = with_byte(&byte(32, 24), &list);
    let moved = byte_edit(&b, &request).expect("the byte crop");
    assert_eq!((moved.width, moved.height), (w, h));
    assert_mapped(
        &byte_paths(&b),
        &byte_paths(&moved),
        expected,
        "byte perspective crop",
    );
    let t = with_typed(&typed(32, 24), &list);
    let moved16 = typed_edit(&t, &request).unwrap().expect("the typed crop");
    assert_eq!(moved16.dimensions(), (w, h));
    assert_mapped(
        &typed_paths(&t),
        &typed_paths(&moved16),
        expected,
        "typed perspective crop",
    );
}

#[test]
fn a_turn_that_moves_only_a_path_is_an_edit_and_a_layer_transform_leaves_paths() {
    // A uniform canvas with its ruler origin at the centre, turned half way
    // round, holds the same samples and the same chrome, so the moved path
    // is the whole edit; without one the turn changes nothing.
    let request = json!({"op": "rotate180"});
    let centred = byte(W, H).set_ruler_origin(32.0, 24.0).expect("an origin");
    let centred16 = typed(W, H).set_ruler_origin(32.0, 24.0).expect("an origin");
    assert!(byte_edit(&centred, &request).is_none());
    assert!(unchanged(typed_edit(&centred16, &request)));
    let list = json!({"paths": [saved("One", json!([polygon(&[[5.0, 7.0]], false)]))]});
    let b = with_byte(&centred, &list);
    let turned = byte_edit(&b, &request).expect("the path moved");
    assert_eq!(points(&byte_paths(&turned))[0], [59.0, 41.0]);
    let t = with_typed(&centred16, &list);
    let turned16 = typed_edit(&t, &request).unwrap().expect("the path moved");
    assert_eq!(points(&typed_paths(&turned16))[0], [59.0, 41.0]);

    // A per-layer transform moves nothing about the canvas.
    let moved = byte_edit(
        &b,
        &json!({"op": "affine", "layer": 0, "target": "pixels",
                "matrix": [1, 0, 0, 1, 5, 3], "filter": "nearest"}),
    )
    .expect("the layer moved");
    assert_eq!(byte_paths(&moved), byte_paths(&b));
}

#[test]
fn flatten_conversion_and_profile_edits_carry_the_paths_with_their_ids() {
    let b = with_byte(&byte(W, H), &start());
    let stored = byte_paths(&b);
    assert_eq!(byte_paths(&b.flattening().expect("flatten")), stored);
    let promoted = {
        let mut error = ptr::null_mut();
        let out = unsafe {
            doctored_core::ffi_precision_conversion::dr_doc_promote_structure(
                &b,
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(!out.is_null(), "{}", take_err_string(error));
        unsafe { Box::from_raw(out) }
    };
    assert_eq!(
        typed_paths(&promoted),
        stored,
        "8 → 16 keeps the list and its ids"
    );
    let narrowed = promoted
        .downconvert(
            doctored_core::precision_quantize::PrecisionDither::None,
            LIMIT,
            0,
            None,
        )
        .expect("16 → 8");
    assert_eq!(byte_paths(&narrowed), stored);
    // A typed edit that rebuilds the document keeps them too.
    let lit = typed_edit(
        &promoted,
        &json!({"op": "global_light", "angle": 45, "altitude": 60}),
    )
    .unwrap()
    .expect("the light changed");
    assert_eq!(typed_paths(&lit), stored);
}
