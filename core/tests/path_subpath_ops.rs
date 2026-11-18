//! PER-SUBPATH OPERATIONS held on a path (`designs/v1-paths-held-subpaths.md`,
//! slice 2): the shape JSON's `op`, the coverage it dispatches to, the
//! conditional DRDC word 116 at both depths, the byte and typed vector-mask
//! bakes, the unbounded slot's outside coverage under a canvas enlargement,
//! and the PSD export report.
//!
//! The coverage oracle is the kernel's own seam, `ops_coverage`, over verbs
//! and points this file writes from its corner lists (`path_shared_edge.rs`
//! holds that routine to exact areas); everything else is read back through
//! the C API. A composite is checked against reference pixels of the same
//! document (an inverted and an untouched one), never against a number the
//! core printed.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::artwork_ops_coverage::ops_coverage;
use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::ffi_doc::{dr_doc_canvas_resize, dr_doc_rotate90};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::dr_doc_psd_export_report;
use doctored_core::ffi_vector_mask::{
    dr_doc_set_vector_mask, dr_doc_set_vector_mask_properties, dr_doc_vector_mask_json,
    dr_precision_doc_set_vector_mask, dr_precision_doc_vector_mask_json,
};
use doctored_core::path_geometry::{Anchor, FillRule, PathShape, Subpath, SubpathOp};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{drdc_word, selection, take_err_string};
use crate::path_format::{open_byte, open_typed, path_table, save_byte, save_typed, with_table};
use crate::path_geometry::{
    byte, byte_paths, changed, float, refused, set_byte, set_typed, take, typed, typed_paths,
    with_byte, with_typed, without_ids, LIMIT,
};
use crate::path_shape::coverage;

const W: u32 = 48;
const H: u32 = 40;
/// How far a baked mask may sit from the kernel's fold over the canvas: the
/// plane is drawn over its own window, a different floating-point origin.
const BAKE: u16 = 1;
const WORD: u32 = 116;
const WORD_REFUSAL: &str = "native subpath operations require DRDC 116";
const INVALID_WORD: &str = "invalid native subpath operations word";

/// A rectangle and a triangle that overlap it.
const RECT: [[f64; 2]; 4] = [[6.25, 5.5], [30.75, 5.5], [30.75, 28.25], [6.25, 28.25]];
const TRI: [[f64; 2]; 3] = [[18.5, 12.25], [44.0, 20.5], [24.25, 36.75]];

/// The three operation lists the bakes are held to: a difference, an
/// intersection and a first subtract alone (unbounded).
const CASES: [&[&str]; 3] = [
    &["unite", "subtract"],
    &["unite", "intersect"],
    &["subtract"],
];

fn op(name: &str) -> SubpathOp {
    match name {
        "unite" => SubpathOp::Unite,
        "subtract" => SubpathOp::Subtract,
        "intersect" => SubpathOp::Intersect,
        "exclude" => SubpathOp::Exclude,
        _ => panic!("no operation {name}"),
    }
}

/// The polygons a case folds: RECT, then TRI when there are two.
fn polygons(count: usize) -> Vec<Vec<[f64; 2]>> {
    [RECT.to_vec(), TRI.to_vec()]
        .into_iter()
        .take(count)
        .collect()
}

/// A closed corner subpath folded by `op`.
fn held(corners: &[[f64; 2]], op: &str) -> Value {
    json!({"closed": true, "op": op, "anchors": corners})
}

/// A closed corner subpath with no operation.
fn plain(corners: &[[f64; 2]]) -> Value {
    json!({"closed": true, "anchors": corners})
}

/// The shape JSON of `polygons` folded by `ops`, nonzero.
fn shape(polygons: &[Vec<[f64; 2]>], ops: &[&str]) -> Value {
    let subpaths: Vec<Value> = polygons.iter().zip(ops).map(|(p, o)| held(p, o)).collect();
    json!({"fill_rule": "nonzero", "subpaths": subpaths})
}

/// The kernel's fold of `polygons` by `ops` over a `w` × `h` canvas, the
/// verbs written here: a move, a line to each next corner and back to the
/// first, a close.
fn oracle(polygons: &[Vec<[f64; 2]>], ops: &[&str], w: u32, h: u32) -> Vec<u16> {
    let (mut verbs, mut points) = (Vec::new(), Vec::new());
    for corners in polygons {
        verbs.push(0);
        points.push(corners[0]);
        for k in 0..corners.len() {
            verbs.push(1);
            points.push(corners[(k + 1) % corners.len()]);
        }
        verbs.push(4);
    }
    let ops: Vec<SubpathOp> = ops.iter().map(|name| op(name)).collect();
    ops_coverage(&verbs, &points, &ops, w, h, false).expect("the kernel's fold")
}

/// A u16 sample as a byte, rounded to nearest (`doc_vector_mask`'s rule).
fn narrowed(sample: u16) -> u8 {
    ((u32::from(sample) * 255 + 32767) / 65535) as u8
}

/// The largest difference between two planes, and where.
fn worst<T: Copy + Into<i64>>(a: &[T], b: &[T]) -> (i64, usize) {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .enumerate()
        .map(|(i, (p, q))| (((*p).into() - (*q).into()).abs(), i))
        .max()
        .unwrap_or((0, 0))
}

/// Each subpath's `op` in a shape JSON the core wrote (`None` for a subpath
/// without the key).
fn ops_of(shape: &Value) -> Vec<Option<String>> {
    shape["subpaths"]
        .as_array()
        .expect("subpaths")
        .iter()
        .map(|s| s.get("op").map(|o| o.as_str().expect("a name").to_string()))
        .collect()
}

fn names(list: &[&str]) -> Vec<Option<String>> {
    list.iter().map(|name| Some(name.to_string())).collect()
}

/// A path list of one saved path "Held" with `subpaths`, optionally the
/// clipping path.
fn list(subpaths: Value, clipping: bool) -> Value {
    let mut entry = json!({"kind": "saved", "name": "Held", "fill_rule": "nonzero",
                           "subpaths": subpaths});
    if clipping {
        entry["clipping"] = json!({});
    }
    json!({"paths": [entry]})
}

/// Every `op` key removed from a value.
fn without_ops(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "op")
                .map(|(key, v)| (key.clone(), without_ops(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_ops).collect()),
        other => other.clone(),
    }
}

/// `bytes` with the word at index 1 removed.
fn drained(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes[..8].to_vec();
    out.extend(&bytes[12..]);
    out
}

/// `bytes` with `word` inserted at index 1.
fn spliced(bytes: &[u8], word: u32) -> Vec<u8> {
    let mut out = bytes[..8].to_vec();
    out.extend(word.to_le_bytes());
    out.extend(&bytes[8..]);
    out
}

// ------------------------------------------------------- byte document --

fn byte_answer(out: *mut DrDocument, error: *mut c_char) -> Result<Option<DrDocument>, String> {
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

fn set_byte_mask(doc: &DrDocument, layer: usize, shape: &Value) -> DrDocument {
    let text = CString::new(shape.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_set_vector_mask(doc, layer, text.as_ptr(), ptr::null(), &mut error) };
    changed(byte_answer(out, error))
}

fn byte_view(doc: &DrDocument, layer: usize) -> Value {
    let out = unsafe { dr_doc_vector_mask_json(doc, layer) };
    serde_json::from_str(&take(out)).expect("vector mask JSON")
}

fn byte_alpha(doc: &DrDocument) -> Vec<u8> {
    doc.flattened()
        .expect("flatten")
        .pixels()
        .map(|p| p[3])
        .collect()
}

fn byte_pixel(doc: &DrDocument, x: u32, y: u32) -> [u8; 4] {
    doc.flattened().expect("flatten").get_pixel(x, y).0
}

fn byte_canvas(doc: &DrDocument, w: u32, h: u32, origin: (i32, i32)) -> DrDocument {
    let out = unsafe { dr_doc_canvas_resize(doc, w, h, origin.0, origin.1) };
    assert!(!out.is_null(), "the canvas resize");
    *unsafe { Box::from_raw(out) }
}

fn byte_turned(doc: &DrDocument) -> DrDocument {
    let out = unsafe { dr_doc_rotate90(doc) };
    assert!(!out.is_null(), "the quarter turn");
    *unsafe { Box::from_raw(out) }
}

// ------------------------------------------------------ typed document --

fn typed_answer(
    out: *mut PrecisionDocument,
    error: *mut c_char,
) -> Result<Option<PrecisionDocument>, String> {
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

fn set_typed_mask(doc: &PrecisionDocument, layer: usize, shape: &Value) -> PrecisionDocument {
    let text = CString::new(shape.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_vector_mask(
            doc,
            layer,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    changed(typed_answer(out, error))
}

fn typed_view(doc: &PrecisionDocument, layer: usize) -> Value {
    let out = unsafe { dr_precision_doc_vector_mask_json(doc, layer) };
    serde_json::from_str(&take(out)).expect("vector mask JSON")
}

fn typed_pixels(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None)
        .expect("render")
        .to_rgba16()
        .expect("16-bit pixels")
}

fn typed_alpha(doc: &PrecisionDocument) -> Vec<u16> {
    typed_pixels(doc).iter().map(|p| p[3]).collect()
}

/// One `dr_precision_doc_edit` request with its u16 payload.
fn edit(doc: &PrecisionDocument, request: &Value, samples: &[u16]) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let payload = if samples.is_empty() {
        ptr::null()
    } else {
        samples.as_ptr()
    };
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            payload,
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    changed(typed_answer(out, error))
}

// ------------------------------------------------------------ coverage --

#[test]
fn path_coverage_of_held_operations_is_the_kernels_fold() {
    let all: [&[&str]; 5] = [
        &["unite", "subtract"],
        &["unite", "intersect"],
        &["unite", "exclude"],
        &["unite", "unite"],
        &["subtract"],
    ];
    for ops in all {
        let polygons = polygons(ops.len());
        let want = oracle(&polygons, ops, W, H);
        let json = shape(&polygons, ops);
        let soft = coverage(&json, (0, 0), (W, H), true, None).expect("coverage");
        assert_eq!(soft, want, "{ops:?}: anti-aliased");
        let hard = coverage(&json, (0, 0), (W, H), false, None).expect("hard coverage");
        let cut: Vec<u16> = want
            .iter()
            .map(|&v| if v >= 32768 { u16::MAX } else { 0 })
            .collect();
        assert_eq!(hard, cut, "{ops:?}: the hard edge");
    }
    // Held unite is not the summed winding: a counter-wound inner rectangle
    // is a hole without operations and filled with them.
    let outer = [[4.0, 4.0], [40.0, 4.0], [40.0, 34.0], [4.0, 34.0]];
    let inner = [[12.0, 10.0], [12.0, 26.0], [30.0, 26.0], [30.0, 10.0]];
    let summed = json!({"subpaths": [plain(&outer), plain(&inner)]});
    let united = json!({"subpaths": [held(&outer, "unite"), held(&inner, "unite")]});
    let at = (18 * W + 20) as usize;
    assert_eq!(
        coverage(&summed, (0, 0), (W, H), true, None).unwrap()[at],
        0
    );
    assert_eq!(
        coverage(&united, (0, 0), (W, H), true, None).unwrap()[at],
        u16::MAX
    );
}

#[test]
fn an_in_memory_operation_list_must_be_one_per_subpath() {
    let one = Subpath {
        anchors: vec![
            Anchor::corner([1.0, 1.0]),
            Anchor::corner([9.0, 1.0]),
            Anchor::corner([5.0, 8.0]),
        ],
        closed: true,
    };
    let short = PathShape {
        subpaths: vec![one.clone(), one.clone()],
        fill_rule: FillRule::NonZero,
        ops: Some(vec![SubpathOp::Unite]),
    };
    assert_eq!(
        short.refusal().as_deref(),
        Some("a path's operations do not match its subpaths")
    );
    assert!(short.validated().is_err());
    let empty = PathShape {
        subpaths: Vec::new(),
        fill_rule: FillRule::NonZero,
        ops: Some(Vec::new()),
    };
    assert_eq!(empty.validated().expect("an empty shape").ops, None);
}

// ---------------------------------------------------------------- JSON --

/// The JSON rules at one depth: `set` sets a list on `base` and `read`
/// reads a document's list back.
fn json_rules<D>(
    base: &D,
    set: impl Fn(&D, &Value) -> Result<Option<D>, String>,
    read: impl Fn(&D) -> Value,
    what: &str,
) {
    let both = |a: &str, b: &str| json!([held(&RECT, a), held(&TRI, b)]);
    let held_doc = changed(set(base, &list(both("unite", "subtract"), false)));
    let seen = &read(&held_doc)["paths"][0];
    assert_eq!(ops_of(seen), names(&["unite", "subtract"]), "{what}");

    // A shape without operations reads back without the key.
    let plain_list = list(json!([plain(&RECT), plain(&TRI)]), false);
    let plain_doc = changed(set(base, &plain_list));
    assert_eq!(
        ops_of(&read(&plain_doc)["paths"][0]),
        vec![None, None],
        "{what}"
    );
    // Every operation unite is not the shape without operations: it is a
    // change, and each reads back as itself.
    let united = changed(set(&plain_doc, &list(both("unite", "unite"), false)));
    assert_eq!(
        ops_of(&read(&united)["paths"][0]),
        names(&["unite", "unite"]),
        "{what}"
    );
    let back = changed(set(&united, &plain_list));
    assert_eq!(ops_of(&read(&back)["paths"][0]), vec![None, None], "{what}");

    // All or none, and only the four names.
    let mixed = list(json!([held(&RECT, "unite"), plain(&TRI)]), false);
    let message = refused(set(base, &mixed));
    assert!(
        message.contains("subpath 1 has no op while another subpath has one"),
        "{what}: {message}"
    );
    let mixed = list(json!([plain(&RECT), held(&TRI, "unite")]), false);
    let message = refused(set(base, &mixed));
    assert!(
        message.contains("subpath 0 has no op while another subpath has one"),
        "{what}: {message}"
    );
    let unknown = list(json!([held(&RECT, "union")]), false);
    let message = refused(set(base, &unknown));
    assert!(
        message.contains("a subpath's op must be unite, subtract, intersect or exclude"),
        "{what}: {message}"
    );
}

#[test]
fn a_held_operation_reads_back_per_subpath_at_every_depth() {
    json_rules(&byte(W, H), set_byte, byte_paths, "8-bit");
    json_rules(&typed(W, H), set_typed, typed_paths, "16-bit");
    json_rules(&float(W, H), set_typed, typed_paths, "float");
}

// ------------------------------------------------------------- word 116 --

#[test]
fn held_operations_take_word_116_in_a_byte_file() {
    let dir = TempDir::new().unwrap();
    let base = byte(W, H);
    let both = json!([held(&RECT, "unite"), held(&TRI, "subtract")]);
    let doc = with_byte(&base, &list(both.clone(), false));
    let bytes = save_byte(&dir, "held.rz", &doc);
    assert_eq!(
        (
            drdc_word(&bytes, 0),
            drdc_word(&bytes, 1),
            drdc_word(&bytes, 2)
        ),
        (45, WORD, 74)
    );
    let reopened = open_byte(&dir, "held-again.rz", &bytes).expect("reopen");
    assert_eq!(
        without_ids(&byte_paths(&reopened)),
        without_ids(&byte_paths(&doc))
    );
    assert_eq!(save_byte(&dir, "held-resave.rz", &reopened), bytes);

    // Ahead of the clipping path's word.
    let clipped = with_byte(&base, &list(both.clone(), true));
    let clipped_bytes = save_byte(&dir, "clipped.rz", &clipped);
    assert_eq!(
        (
            drdc_word(&clipped_bytes, 1),
            drdc_word(&clipped_bytes, 2),
            drdc_word(&clipped_bytes, 3)
        ),
        (WORD, 114, 74)
    );
    assert!(open_byte(&dir, "clipped-again.rz", &clipped_bytes).is_ok());

    // The word both ways.
    let message = open_byte(&dir, "drained.rz", &drained(&bytes))
        .err()
        .expect("refused");
    assert!(message.contains(WORD_REFUSAL), "{message}");
    let plain_doc = with_byte(&base, &without_ops(&list(both, false)));
    let plain_bytes = save_byte(&dir, "plain.rz", &plain_doc);
    assert_eq!(drdc_word(&plain_bytes, 1), 74, "no operations, no word");
    let message = open_byte(&dir, "spliced.rz", &spliced(&plain_bytes, WORD))
        .err()
        .expect("refused");
    assert!(message.contains(INVALID_WORD), "{message}");

    // Without the word and the keys the file is the prior writer's.
    let stripped = drained(&bytes);
    let (_, table) = path_table(&stripped, 1);
    let rebuilt = with_table(&stripped, 1, &without_ops(&table).to_string(), 0);
    assert_eq!(rebuilt, plain_bytes, "the prior writer's bytes");

    // A live vector mask alone writes the word.
    let masked = set_byte_mask(&base, 0, &shape(&polygons(2), &["unite", "subtract"]));
    let mask_bytes = save_byte(&dir, "mask.rz", &masked);
    assert_eq!(
        (drdc_word(&mask_bytes, 1), drdc_word(&mask_bytes, 2)),
        (WORD, 74)
    );
    let reopened = open_byte(&dir, "mask-again.rz", &mask_bytes).expect("reopen");
    assert_eq!(byte_view(&reopened, 0), byte_view(&masked, 0));
}

#[test]
fn held_operations_take_word_116_in_a_typed_file() {
    let dir = TempDir::new().unwrap();
    for (name, base) in [("16", typed(W, H)), ("float", float(W, H))] {
        let records = base.layer_count();
        let both = json!([held(&RECT, "unite"), held(&TRI, "subtract")]);
        let doc = with_typed(&base, &list(both.clone(), false));
        let bytes = save_typed(&dir, &format!("held-{name}.rz"), &doc);
        assert_eq!(
            (
                drdc_word(&bytes, 0),
                drdc_word(&bytes, 1),
                drdc_word(&bytes, 2)
            ),
            (46, WORD, 75),
            "{name}"
        );
        let reopened = open_typed(&dir, &format!("again-{name}.rz"), &bytes).expect("reopen");
        assert_eq!(
            without_ids(&typed_paths(&reopened)),
            without_ids(&typed_paths(&doc)),
            "{name}"
        );
        assert_eq!(
            save_typed(&dir, &format!("resave-{name}.rz"), &reopened),
            bytes,
            "{name}"
        );

        let clipped = with_typed(&base, &list(both.clone(), true));
        let clipped_bytes = save_typed(&dir, &format!("clipped-{name}.rz"), &clipped);
        assert_eq!(
            (drdc_word(&clipped_bytes, 1), drdc_word(&clipped_bytes, 2)),
            (WORD, 114),
            "{name}"
        );

        let message = open_typed(&dir, &format!("drained-{name}.rz"), &drained(&bytes))
            .err()
            .expect("refused");
        assert!(message.contains(WORD_REFUSAL), "{name}: {message}");
        let plain_doc = with_typed(&base, &without_ops(&list(both, false)));
        let plain_bytes = save_typed(&dir, &format!("plain-{name}.rz"), &plain_doc);
        assert_eq!(drdc_word(&plain_bytes, 1), 75, "{name}");
        let message = open_typed(
            &dir,
            &format!("spliced-{name}.rz"),
            &spliced(&plain_bytes, WORD),
        )
        .err()
        .expect("refused");
        assert!(message.contains(INVALID_WORD), "{name}: {message}");

        let stripped = drained(&bytes);
        let (_, table) = path_table(&stripped, records);
        let rebuilt = with_table(&stripped, records, &without_ops(&table).to_string(), 0);
        assert_eq!(rebuilt, plain_bytes, "{name}: the prior writer's bytes");

        let masked = set_typed_mask(&base, 0, &shape(&polygons(2), &["unite", "subtract"]));
        let mask_bytes = save_typed(&dir, &format!("mask-{name}.rz"), &masked);
        assert_eq!(
            (drdc_word(&mask_bytes, 1), drdc_word(&mask_bytes, 2)),
            (WORD, 75),
            "{name}"
        );
        let reopened =
            open_typed(&dir, &format!("mask-again-{name}.rz"), &mask_bytes).expect("reopen");
        assert_eq!(typed_view(&reopened, 0), typed_view(&masked, 0), "{name}");
    }
}

// ---------------------------------------------------------------- bakes --

/// `polygons` turned a quarter clockwise on a `W` × `H` canvas: (x, y) →
/// (H − y, x).
fn turned(polygons: &[Vec<[f64; 2]>]) -> Vec<Vec<[f64; 2]>> {
    polygons
        .iter()
        .map(|p| p.iter().map(|q| [f64::from(H) - q[1], q[0]]).collect())
        .collect()
}

#[test]
fn a_held_vector_mask_bakes_the_kernels_fold_in_a_byte_document() {
    let dir = TempDir::new().unwrap();
    for ops in CASES {
        let polygons = polygons(ops.len());
        let doc = set_byte_mask(&byte(W, H), 0, &shape(&polygons, ops));
        let want: Vec<u8> = oracle(&polygons, ops, W, H)
            .into_iter()
            .map(narrowed)
            .collect();
        let (most, at) = worst(&byte_alpha(&doc), &want);
        println!("byte {ops:?}: worst difference {most} at sample {at}");
        assert!(
            most <= i64::from(BAKE),
            "byte {ops:?}: sample {at} by {most}"
        );
        assert_eq!(ops_of(&byte_view(&doc, 0)), names(ops), "byte {ops:?}");

        let bytes = save_byte(&dir, "bake.rz", &doc);
        let reopened = open_byte(&dir, "bake-again.rz", &bytes).expect("reopen");
        assert_eq!(byte_view(&reopened, 0), byte_view(&doc, 0), "byte {ops:?}");
        assert_eq!(byte_alpha(&reopened), byte_alpha(&doc), "byte {ops:?}");

        let quarter = byte_turned(&doc);
        assert_eq!(
            ops_of(&byte_view(&quarter, 0)),
            names(ops),
            "byte {ops:?} turned"
        );
        let want: Vec<u8> = oracle(&turned(&polygons), ops, H, W)
            .into_iter()
            .map(narrowed)
            .collect();
        let (most, at) = worst(&byte_alpha(&quarter), &want);
        assert!(
            most <= i64::from(BAKE),
            "byte {ops:?} turned: sample {at} by {most}"
        );
    }
}

#[test]
fn a_held_vector_mask_bakes_the_kernels_fold_in_a_typed_document() {
    let dir = TempDir::new().unwrap();
    for ops in CASES {
        let polygons = polygons(ops.len());
        let doc = set_typed_mask(&typed(W, H), 0, &shape(&polygons, ops));
        let want = oracle(&polygons, ops, W, H);
        let (most, at) = worst(&typed_alpha(&doc), &want);
        println!("typed {ops:?}: worst difference {most} at sample {at}");
        assert!(
            most <= i64::from(BAKE),
            "typed {ops:?}: sample {at} by {most}"
        );
        assert_eq!(ops_of(&typed_view(&doc, 0)), names(ops), "typed {ops:?}");

        let bytes = save_typed(&dir, "bake.rz", &doc);
        let reopened = open_typed(&dir, "bake-again.rz", &bytes).expect("reopen");
        assert_eq!(
            typed_view(&reopened, 0),
            typed_view(&doc, 0),
            "typed {ops:?}"
        );
        assert_eq!(typed_alpha(&reopened), typed_alpha(&doc), "typed {ops:?}");

        let quarter = edit(&doc, &json!({"op": "rotate90"}), &[]);
        assert_eq!(
            ops_of(&typed_view(&quarter, 0)),
            names(ops),
            "typed {ops:?} turned"
        );
        let want = oracle(&turned(&polygons), ops, H, W);
        let (most, at) = worst(&typed_alpha(&quarter), &want);
        assert!(
            most <= i64::from(BAKE),
            "typed {ops:?} turned: sample {at} by {most}"
        );
    }
}

// --------------------------------------------------------- canvas growth --

/// The unbounded case's subtracted rectangle, whole pixels, and a bounded
/// frame around it.
const HOLE: [[f64; 2]; 4] = [[12.0, 10.0], [30.0, 10.0], [30.0, 26.0], [12.0, 26.0]];
const FRAME: [[f64; 2]; 4] = [[4.0, 4.0], [40.0, 4.0], [40.0, 34.0], [4.0, 34.0]];
/// How far the canvas grows on every side.
const GROW: u32 = 8;

fn unbounded() -> Value {
    json!({"subpaths": [held(&HOLE, "subtract")]})
}

fn bounded() -> Value {
    json!({"subpaths": [held(&FRAME, "unite"), held(&HOLE, "subtract")]})
}

/// A first subtract that a later intersect bounds: FRAME less HOLE, which
/// covers nothing beyond FRAME, so it is not unbounded.
fn intersected() -> Value {
    json!({"subpaths": [held(&HOLE, "subtract"), held(&FRAME, "intersect")]})
}

/// The bounded shapes the growth cases hold to outside coverage 0.
fn bounded_shapes() -> [(&'static str, Value); 2] {
    [("bounded", bounded()), ("intersected", intersected())]
}

/// The pixels a grown composite is read at, in the grown canvas: two in the
/// new border, one in the old canvas outside the hole and one inside it.
const NEW_AREA: [(u32, u32); 2] = [(2, 2), (W + 2 * GROW - 3, H + 2 * GROW - 3)];
const OUTSIDE: (u32, u32) = (2 + GROW, 2 + GROW);
const INSIDE: (u32, u32) = (20 + GROW, 18 + GROW);

/// The vector mask's first anchor, as reported.
fn first_anchor(view: &Value) -> [f64; 2] {
    let anchor = &view["subpaths"][0]["anchors"][0];
    [anchor["x"].as_f64().unwrap(), anchor["y"].as_f64().unwrap()]
}

/// The canvas-sized opaque layer under an UNLINKED vector mask `shape` (so
/// its plane is the canvas), the layer then moved `GROW` up and left out from
/// under the plane, which stays: its pixels past the plane are off the
/// canvas until the canvas grows `GROW` on every side. (A byte adjustment
/// layer would not show this: an enabled mask confines a byte adjustment to
/// the adjustment's own pixel rect, `doc::composite_adjustment_into`.)
fn byte_unlinked(shape: &Value) -> DrDocument {
    let masked = set_byte_mask(&byte(W, H), 0, shape);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_doc_set_vector_mask_properties(&masked, 0, true, false, ptr::null(), &mut error)
    };
    let loose = changed(byte_answer(out, error));
    let at = -(GROW as i32);
    let moved = loose.move_layers(&[0], at, at).expect("moved");
    byte_canvas(
        &moved,
        W + 2 * GROW,
        H + 2 * GROW,
        (GROW as i32, GROW as i32),
    )
}

/// The byte case's new area: border pixels the moved layer still covers.
const BYTE_NEW_AREA: [(u32, u32); 3] = [(2, 2), (2, 30), (30, 2)];

#[test]
fn canvas_growth_byte_shows_an_unbounded_mask_in_the_new_area() {
    let dir = TempDir::new().unwrap();
    let grown = byte_unlinked(&unbounded());
    let reopened =
        open_byte(&dir, "grown.rz", &save_byte(&dir, "grown.rz", &grown)).expect("reopen");
    for (what, d) in [("grown", &grown), ("reopened", &reopened)] {
        for (x, y) in BYTE_NEW_AREA {
            assert_eq!(byte_pixel(d, x, y)[3], 255, "{what}: new area ({x}, {y})");
        }
        assert_eq!(byte_pixel(d, OUTSIDE.0, OUTSIDE.1)[3], 255, "{what}");
        assert_eq!(byte_pixel(d, INSIDE.0, INSIDE.1)[3], 0, "{what}");
        let view = byte_view(d, 0);
        assert_eq!(first_anchor(&view), [12.0 + 8.0, 10.0 + 8.0], "{what}");
        assert_eq!(ops_of(&view), names(&["subtract"]), "{what}");
        assert_eq!(d.layers[0].mask_properties.outside_coverage, 255, "{what}");
    }

    // A bounded shape keeps outside coverage 0 and hides the new area.
    for (what, shape) in bounded_shapes() {
        let grown = byte_unlinked(&shape);
        assert_eq!(
            grown.layers[0].mask_properties.outside_coverage, 0,
            "{what}"
        );
        for (x, y) in BYTE_NEW_AREA {
            assert_eq!(byte_pixel(&grown, x, y)[3], 0, "{what}: ({x}, {y})");
        }
    }

    // Beside an enabled, aligned raster part the slot is the product, 0
    // beyond the raster's rectangle.
    let rastered = byte(W, H)
        .add_mask(0, MaskKind::FromSelection(&selection(W, H, |_, _| 255)))
        .expect("a raster mask");
    let both = set_byte_mask(&rastered, 0, &unbounded());
    assert_eq!(both.layers[0].mask_properties.outside_coverage, 0);
}

/// An opaque layer reaching `GROW` past the canvas on every side, and an
/// Invert adjustment over it whose vector mask is `shape` (a typed
/// adjustment's mask is read over the canvas).
fn typed_adjusted(shape: &Value) -> PrecisionDocument {
    let big = typed(W + 2 * GROW, H + 2 * GROW);
    let at = -(GROW as i32);
    let base = edit(
        &big,
        &json!({"op": "canvas_size", "width": W, "height": H, "x": at, "y": at}),
        &[],
    );
    let invert = json!({"type": "adjust", "evaluation": "precision_v1", "op": "invert",
                        "params": {}})
    .to_string();
    let doc = base
        .add_adjustment("Tone", &invert, 1.0, LIMIT, 0, None)
        .expect("the adjustment layer")
        .expect("a new layer");
    set_typed_mask(&doc, 1, shape)
}

fn typed_grown(doc: &PrecisionDocument) -> PrecisionDocument {
    let at = GROW as i32;
    edit(
        doc,
        &json!({"op": "canvas_size", "width": W + 2 * GROW, "height": H + 2 * GROW,
                "x": at, "y": at}),
        &[],
    )
}

fn typed_outside(doc: &PrecisionDocument, layer: usize) -> u8 {
    doc.layer(layer)
        .unwrap()
        .mask()
        .expect("a mask")
        .properties()
        .outside_coverage
}

#[test]
fn canvas_growth_typed_shows_an_unbounded_mask_in_the_new_area() {
    let dir = TempDir::new().unwrap();
    let doc = typed_adjusted(&unbounded());
    let pixels = typed_pixels(&doc);
    let (inverted, untouched) = (pixels[(2 * W + 2) as usize], pixels[(18 * W + 20) as usize]);
    assert_ne!(inverted, untouched, "the adjustment shows outside the hole");
    let grown = typed_grown(&doc);
    let reopened =
        open_typed(&dir, "grown.rz", &save_typed(&dir, "grown.rz", &grown)).expect("reopen");
    let gw = W + 2 * GROW;
    for (what, d) in [("grown", &grown), ("reopened", &reopened)] {
        let pixels = typed_pixels(d);
        let at = |(x, y): (u32, u32)| pixels[(y * gw + x) as usize];
        for point in NEW_AREA {
            assert_eq!(at(point), inverted, "{what}: new area {point:?}");
        }
        assert_eq!(at(OUTSIDE), inverted, "{what}");
        assert_eq!(at(INSIDE), untouched, "{what}");
        let view = typed_view(d, 1);
        assert_eq!(first_anchor(&view), [12.0 + 8.0, 10.0 + 8.0], "{what}");
        assert_eq!(ops_of(&view), names(&["subtract"]), "{what}");
        assert_eq!(typed_outside(d, 1), 255, "{what}");
    }

    for (what, shape) in bounded_shapes() {
        let grown = typed_grown(&typed_adjusted(&shape));
        assert_eq!(typed_outside(&grown, 1), 0, "{what}");
        let pixels = typed_pixels(&grown);
        for (x, y) in NEW_AREA {
            assert_eq!(
                pixels[(y * gw + x) as usize],
                untouched,
                "{what}: ({x}, {y})"
            );
        }
    }

    let full = vec![u16::MAX; (W * H) as usize];
    let rastered = edit(
        &typed(W, H),
        &json!({"op": "set_mask", "layer": 0, "width": W, "height": H}),
        &full,
    );
    let both = set_typed_mask(&rastered, 0, &unbounded());
    assert_eq!(typed_outside(&both, 0), 0);
}

// ------------------------------------------------------------ PSD report --

/// `dr_doc_psd_export_report`'s losses for a layered PSD.
fn psd_losses(doc: &DrDocument) -> Vec<String> {
    let mut error = ptr::null_mut();
    let text = take(unsafe { dr_doc_psd_export_report(doc, 0, 0, &mut error) });
    let report: Value = serde_json::from_str(&text).expect("report JSON");
    report["losses"]
        .as_array()
        .expect("losses")
        .iter()
        .map(|line| line.as_str().expect("a sentence").to_string())
        .collect()
}

/// A PSD path holds each subpath's operation as its word under the nonzero
/// rule only: a path and a vector mask holding operations under it are
/// written with no sentence; under even-odd the path is not written and the
/// vector mask is written as pixels, each with its sentence.
#[test]
fn psd_export_writes_held_operations_only_under_the_nonzero_rule() {
    let path_sentence = "Path “Held” holds per-subpath operations under the even-odd rule, which a PSD path cannot hold; it is not written.";
    let both = json!([held(&RECT, "unite"), held(&TRI, "subtract")]);
    let nonzero = list(both, false);
    let mut even = nonzero.clone();
    even["paths"][0]["fill_rule"] = json!("evenodd");
    let losses = psd_losses(&with_byte(&byte(W, H), &nonzero));
    assert!(
        !losses.iter().any(|l| l.starts_with("Path “Held”")),
        "{losses:?}"
    );
    let losses = psd_losses(&with_byte(&byte(W, H), &even));
    assert!(losses.iter().any(|l| l == path_sentence), "{losses:?}");
    let losses = psd_losses(&with_byte(&byte(W, H), &without_ops(&even)));
    assert!(
        !losses.iter().any(|l| l.starts_with("Path “Held”")),
        "{losses:?}"
    );

    let mask = shape(&polygons(2), &["unite", "subtract"]);
    let mut even_mask = mask.clone();
    even_mask["fill_rule"] = json!("evenodd");
    for (rule, shape, pixels) in [("nonzero", &mask, false), ("evenodd", &even_mask, true)] {
        let masked = set_byte_mask(&byte(W, H), 0, shape);
        let name = masked.layers[0].name.clone();
        let sentence = format!(
            "The vector mask on “{name}” is written as pixels, an ordinary layer mask of its rendered shape; its path is not written."
        );
        let losses = psd_losses(&masked);
        assert_eq!(losses.contains(&sentence), pixels, "{rule}: {losses:?}");
    }
}
