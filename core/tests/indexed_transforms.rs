//! #547 Indexed Color phase 2d: Free Transform and Transform on the one
//! layer (`affine`, `perspective`, `affine_set`, `perspective_set`), the
//! selected pixels' transform, Warp, the layer row (rename, label, lock),
//! Export Layers and the refusals around them (`indexed_geometry`,
//! `indexed_transform`, `indexed_export`).
//!
//! The oracles are independent of the adapters: for a move or a Nearest pick
//! the index plane moved by hand (on a palette with a duplicate entry, which
//! no colour lookup could keep); for a resample the expansion route (Image ▸
//! Mode ▸ RGB Color, the same request on that RGB document, then the shared
//! fixture's own re-index of its canvas window against the old plane).

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_layer_encode,
};
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision::{
    dr_precision_doc_duplicate_layer, dr_precision_doc_save_native,
};
use doctored_core::ffi_warp::{
    dr_precision_doc_warp_edit, dr_precision_doc_warp_edit_plan, dr_precision_doc_warp_info,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{
    background, color_table, edit_plan, free, indices, layer16, open_indexed, open_rgb, oracle,
    paint, palette_of, render16, rgb_doc, set_table, to_indexed, to_rgb, window, Ink, LIMIT,
};

const MASK: &str = "transform_mask is not available in Indexed Color.";

const W: u32 = 12;
const H: u32 = 8;

/// The 12 × 8 scene: black, white and (190, 50, 50) bands with two marker
/// pixels, its last row clear when `clear_row` (the transparent index).
fn scene(clear_row: bool) -> *mut PrecisionDocument {
    let mut pixels = Vec::new();
    for y in 0..H {
        for x in 0..W {
            pixels.push(if clear_row && y == H - 1 {
                [0, 0, 0, 0]
            } else if (x, y) == (1, 1) {
                [190, 50, 50, 255]
            } else if x < 4 || (x, y) == (10, 2) {
                [0, 0, 0, 255]
            } else if x < 8 {
                [255, 255, 255, 255]
            } else {
                [190, 50, 50, 255]
            });
        }
    }
    let rgb = rgb_doc(W, H, &pixels);
    let out = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    out
}

fn retabled(doc: *mut PrecisionDocument, table: Value) -> *mut PrecisionDocument {
    let out = set_table(doc, &table).unwrap().unwrap();
    free(doc);
    out
}

/// The clear-row scene with distinct colours, t (3) unique.
fn distinct_t() -> *mut PrecisionDocument {
    retabled(
        scene(true),
        json!({"colors":[[0,0,0],[255,255,255],[190,50,50],[9,9,9]],"transparent_index":3}),
    )
}

/// The clear-row scene with entry 1 (the white band) a duplicate of entry 0.
fn duplicate_t() -> *mut PrecisionDocument {
    retabled(
        scene(true),
        json!({"colors":[[0,0,0],[0,0,0],[190,50,50],[255,255,255]],"transparent_index":3}),
    )
}

/// The opaque scene (no transparent index) with entry 1 a duplicate of 0.
fn duplicate_opaque() -> *mut PrecisionDocument {
    retabled(
        scene(false),
        json!({"colors":[[0,0,0],[0,0,0],[190,50,50],[255,255,255]],"transparent_index":null}),
    )
}

/// `before` moved on the same canvas: each pixel reads the one `source`
/// names, `fill` where that is off the canvas.
fn moved(before: &[i32], source: impl Fn(i64, i64) -> (i64, i64), fill: i32) -> Vec<i32> {
    let (w, h) = (i64::from(W), i64::from(H));
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = source(x, y);
            out.push(if sx >= 0 && sy >= 0 && sx < w && sy < h {
                before[(sy * w + sx) as usize]
            } else {
                fill
            });
        }
    }
    out
}

/// The layer of an Indexed document is its palette's render of its plane.
fn assert_renders(doc: *const PrecisionDocument) {
    let (palette, t) = palette_of(doc);
    for (pixel, &index) in layer16(doc).iter().zip(&indices(doc)) {
        let c = palette[index as usize];
        let alpha = if Some(index as u8) == t { 0 } else { 65535 };
        assert_eq!(
            *pixel,
            [
                u16::from(c[0]) * 257,
                u16::from(c[1]) * 257,
                u16::from(c[2]) * 257,
                alpha
            ]
        );
    }
}

/// The edit's result, which must exist.
fn edited(doc: *const PrecisionDocument, request: &Value, input: &[u16]) -> *mut PrecisionDocument {
    paint(doc, request, input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request}: nothing moved"))
}

/// The plane after an edit: the result's, or `before` when it changed nothing.
fn plane_after(doc: *const PrecisionDocument, request: &Value, input: &[u16]) -> Vec<i32> {
    match paint(doc, request, input).unwrap_or_else(|e| panic!("{request}: {e}")) {
        Some(out) => {
            assert_renders(out);
            let got = indices(out);
            free(out);
            got
        }
        None => indices(doc),
    }
}

/// The expansion oracle of `request` (module doc): the RGB op's canvas
/// window re-indexed against the old plane with each pixel's own colour.
fn rgb_oracle(doc: *const PrecisionDocument, request: &Value, input: &[u16]) -> Vec<i32> {
    let (palette, t) = palette_of(doc);
    let before = indices(doc);
    let rgb = to_rgb(doc);
    let out = match paint(rgb, request, input).unwrap_or_else(|e| panic!("RGB {request}: {e}")) {
        Some(twin) => {
            let got = oracle(&palette, t, Some(&before), &window(twin), Ink::Own).unwrap();
            free(twin);
            got
        }
        None => before,
    };
    free(rgb);
    out
}

fn affine(matrix: [f64; 6], filter: &str) -> Value {
    json!({"op":"affine","layer":0,"target":"pixels","matrix":matrix,"filter":filter})
}

#[test]
fn moves_and_nearest_picks_move_the_index_plane() {
    let doc = duplicate_t();
    let before = indices(doc);
    assert!(before.contains(&1), "the duplicate entry is in use");
    type Map = Box<dyn Fn(i64, i64) -> (i64, i64)>;
    let cases: Vec<(Value, Map)> = vec![
        // A whole-pixel translation under any filter.
        (
            affine([1.0, 0.0, 0.0, 1.0, 2.0, 1.0], "bicubic"),
            Box::new(|x, y| (x - 2, y - 1)),
        ),
        (
            json!({"op":"affine_set","layers":[0],"matrix":[1.0,0.0,0.0,1.0,-3.0,2.0],
                   "filter":"bilinear"}),
            Box::new(|x, y| (x + 3, y - 2)),
        ),
        // A Nearest quarter turn (CGAffineTransform order): pixel (x, y)
        // lands at (9 - y, x).
        (
            affine([0.0, 1.0, -1.0, 0.0, 10.0, 0.0], "nearest"),
            Box::new(|x, y| (y, 9 - x)),
        ),
        // A Nearest perspective whose quad mirrors the layer.
        (
            json!({"op":"perspective","layer":0,"target":"pixels",
                   "quad":[12.0,0.0,0.0,0.0,0.0,8.0,12.0,8.0],"filter":"nearest"}),
            Box::new(|x, y| (11 - x, y)),
        ),
    ];
    for (request, source) in cases {
        let out = edited(doc, &request, &[]);
        assert_eq!(indices(out), moved(&before, source, 3), "{request}");
        assert_renders(out);
        free(out);
    }
    free(doc);

    // With no transparent index the uncovered area takes the entry nearest
    // white (phase 3a), the unused 3.
    let opaque = duplicate_opaque();
    let (palette, t) = palette_of(opaque);
    let before = indices(opaque);
    let out = edited(
        opaque,
        &affine([1.0, 0.0, 0.0, 1.0, 1.0, 0.0], "nearest"),
        &[],
    );
    assert_eq!(
        indices(out),
        moved(&before, |x, y| (x - 1, y), background(&palette, t))
    );
    assert_renders(out);
    free(out);
    free(opaque);
}

#[test]
fn resampled_transforms_are_the_rgb_op_reindexed() {
    let doc = distinct_t();
    let (c, s) = (10f64.to_radians().cos(), 10f64.to_radians().sin());
    for request in [
        affine([c, s, -s, c, 1.5, -0.5], "bicubic"),
        json!({"op":"perspective_set","layers":[0],"source_rect":[0.0,0.0,12.0,8.0],
               "quad":[1.0,0.5,11.5,1.0,12.0,8.0,0.0,7.5],"filter":"bicubic"}),
    ] {
        let out = edited(doc, &request, &[]);
        assert_eq!(indices(out), rgb_oracle(doc, &request, &[]), "{request}");
        assert_renders(out);
        free(out);
    }
    free(doc);
}

/// A canvas coverage of 65535 inside `[x0, x1) × [y0, y1)`, else 0.
fn rect(x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<u16> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            if (x0..x1).contains(&x) && (y0..y1).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect()
}

/// A graded disc of coverage around (6, 4), radius 3.5, ramping over 2 px.
fn disc() -> Vec<u16> {
    let mut out = Vec::new();
    for y in 0..H {
        for x in 0..W {
            let d = ((f64::from(x) - 6.0).powi(2) + (f64::from(y) - 4.0).powi(2)).sqrt();
            out.push((((3.5 - d) / 2.0).clamp(0.0, 1.0) * 65535.0).round() as u16);
        }
    }
    out
}

fn selected(dx: f64, dy: f64, filter: &str, duplicate: bool) -> Value {
    json!({"op":"transform_selected","layer":0,"matrix":[1.0,0.0,0.0,1.0,dx,dy],
           "filter":filter,"duplicate":duplicate})
}

#[test]
fn the_selected_pixels_move_exactly_on_a_hard_selection() {
    let doc = duplicate_t();
    let before = indices(doc);
    let t = 3;
    let coverage = rect(2, 1, 6, 4);
    for duplicate in [false, true] {
        // By hand: the hole (t) unless duplicated, then every selected
        // pixel but a t one lands (2, 1) further on.
        let mut expected = before.clone();
        if !duplicate {
            for (i, &c) in coverage.iter().enumerate() {
                if c == 65535 {
                    expected[i] = t;
                }
            }
        }
        for y in 0..H {
            for x in 0..W {
                let i = (y * W + x) as usize;
                let (nx, ny) = (x + 2, y + 1);
                if coverage[i] == 65535 && before[i] != t && nx < W && ny < H {
                    expected[(ny * W + nx) as usize] = before[i];
                }
            }
        }
        let out = edited(doc, &selected(2.0, 1.0, "bicubic", duplicate), &coverage);
        assert_eq!(indices(out), expected, "duplicate {duplicate}");
        assert_renders(out);
        free(out);
    }
    free(doc);
}

#[test]
fn a_floated_t_lands_nothing_as_on_the_rgb_storage() {
    // [t, A, B], moved one to the right Nearest.
    let rgb = rgb_doc(3, 1, &[[0, 0, 0, 0], [200, 0, 0, 255], [0, 0, 200, 255]]);
    let doc = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    let before = indices(doc);
    let (_, t) = palette_of(doc);
    let t = i32::from(t.expect("a transparent index"));
    let a = before[1];
    assert_eq!(before[0], t);
    let request = |duplicate: bool| {
        json!({"op":"transform_selected","layer":0,"matrix":[1.0,0.0,0.0,1.0,1.0,0.0],
               "filter":"nearest","duplicate":duplicate})
    };
    // x0 alone floats nothing: refused as on the RGB storage.
    let rgb = to_rgb(doc);
    for duplicate in [false, true] {
        let coverage = [65535, 0, 0];
        assert_eq!(
            paint(doc, &request(duplicate), &coverage).unwrap_err(),
            paint(rgb, &request(duplicate), &coverage).unwrap_err()
        );
    }
    free(rgb);
    // x0 and x1: the floated t lands nothing, A lands on B.
    for (duplicate, hand) in [(false, vec![t, t, a]), (true, vec![t, a, a])] {
        let coverage = [65535, 65535, 0];
        let got = plane_after(doc, &request(duplicate), &coverage);
        assert_eq!(got, hand, "duplicate {duplicate}");
        assert_ne!(hand, before);
        assert_eq!(
            got,
            rgb_oracle(doc, &request(duplicate), &coverage),
            "duplicate {duplicate}"
        );
    }
    free(doc);
}

#[test]
fn a_feathered_selection_is_the_rgb_op_reindexed() {
    let doc = distinct_t();
    let coverage = disc();
    for request in [
        selected(2.0, 1.0, "nearest", false),
        selected(1.5, 0.5, "bicubic", true),
    ] {
        let out = edited(doc, &request, &coverage);
        assert_eq!(
            indices(out),
            rgb_oracle(doc, &request, &coverage),
            "{request}"
        );
        assert_renders(out);
        free(out);
    }
    // A hard selection under a resampling filter is the RGB op too.
    let request = selected(1.5, 0.0, "bilinear", false);
    let hard = rect(2, 1, 6, 4);
    let out = edited(doc, &request, &hard);
    assert_eq!(indices(out), rgb_oracle(doc, &request, &hard));
    free(out);
    free(doc);
}

fn warp(
    doc: *const PrecisionDocument,
    request: &Value,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_warp_edit(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if !out.is_null() {
        return Ok(Some(out));
    }
    if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

fn json_of(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn warp_info(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(unsafe { dr_precision_doc_warp_info(doc, 0, &mut err) }, err)
}

fn warp_plan(doc: *const PrecisionDocument, request: &Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_warp_edit_plan(doc, text.as_ptr(), &mut err) },
        err,
    )
}

fn field(displacements: [[f64; 2]; 4], filter: &str) -> Value {
    json!({"op":"set","layer":0,"target":"pixels","columns":2,"rows":2,
           "displacements":displacements,"filter":filter})
}

#[test]
fn warp_keeps_no_binding() {
    // A whole-pixel translation field moves the plane exactly.
    let doc = duplicate_t();
    let before = indices(doc);
    let out = warp(doc, &field([[2.0, 1.0]; 4], "bicubic"))
        .unwrap()
        .unwrap();
    assert_eq!(indices(out), moved(&before, |x, y| (x - 2, y - 1), 3));
    assert_renders(out);
    assert_eq!(warp_info(out)["field"], Value::Null);
    free(out);
    free(doc);

    // A bending field is the RGB warp re-indexed, with no binding after.
    let doc = distinct_t();
    let (palette, t) = palette_of(doc);
    let before = indices(doc);
    let request = field([[0.0, 0.0], [1.5, 0.5], [0.5, 1.0], [0.0, 0.0]], "bicubic");
    let out = warp(doc, &request).unwrap().unwrap();
    let rgb = to_rgb(doc);
    let twin = warp(rgb, &request).unwrap().unwrap();
    let expected = oracle(&palette, t, Some(&before), &window(twin), Ink::Own).unwrap();
    assert_eq!(indices(out), expected);
    assert_renders(out);
    assert_eq!(warp_info(out)["field"], Value::Null);
    assert!(
        !warp_info(twin)["field"].is_null(),
        "the RGB warp keeps one"
    );
    // Clear and Bake have nothing to change.
    for op in ["clear", "bake"] {
        assert_eq!(warp(out, &json!({"op":op,"layer":0})), Ok(None), "{op}");
    }
    // The plan covers the RGB warp's and the adapter's term.
    let indexed = warp_plan(doc, &request);
    let floor = warp_plan(rgb, &request)["scratch_bytes"].as_u64().unwrap() + 2 * 96 * 10;
    assert!(indexed["scratch_bytes"].as_u64().unwrap() >= floor);
    free(twin);
    free(rgb);
    free(out);
    free(doc);
}

fn save_and_open(
    doc: *const PrecisionDocument,
    dir: &TempDir,
    name: &str,
) -> *mut PrecisionDocument {
    let path = dir.path().join(name);
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    open_rgb(&path).unwrap()
}

#[test]
fn the_layer_row_keeps_the_pixels_and_round_trips() {
    let dir = TempDir::new().unwrap();
    let doc = distinct_t();
    let renamed = edited(
        doc,
        &json!({"op":"rename_layer","layer":0,"name":"Plate"}),
        &[],
    );
    let labelled = edited(
        renamed,
        &json!({"op":"layer_label","layer":0,"label":3}),
        &[],
    );
    let locked = edited(
        labelled,
        &json!({"op":"layer_lock","layer":0,"locks":4}),
        &[],
    );
    for out in [renamed, labelled, locked] {
        assert_eq!(indices(out), indices(doc));
        assert_eq!(render16(out), render16(doc));
        assert_renders(out);
    }
    let layer = unsafe { &*locked }.layer(0).unwrap();
    assert_eq!(
        (layer.name(), layer.label(), layer.locks()),
        ("Plate", 3, 4)
    );
    let back = save_and_open(locked, &dir, "row.rz");
    let layer = unsafe { &*back }.layer(0).unwrap();
    assert_eq!(
        (layer.name(), layer.label(), layer.locks()),
        ("Plate", 3, 4)
    );
    assert_eq!(indices(back), indices(doc));
    assert_eq!(color_table(back), color_table(doc));
    // Lock Position refuses the layer transform.
    let refused = paint(
        locked,
        &json!({"op":"affine_set","layers":[0],"matrix":[1.0,0.0,0.0,1.0,1.0,0.0],
                "filter":"nearest"}),
        &[],
    )
    .unwrap_err();
    assert!(refused.contains("locked"), "{refused}");
    // A transformed document round-trips too.
    let turned = edited(doc, &affine([1.0, 0.0, 0.0, 1.0, 1.0, 1.0], "nearest"), &[]);
    let back_turned = save_and_open(turned, &dir, "turned.rz");
    assert_eq!(indices(back_turned), indices(turned));
    for d in [back_turned, turned, back, locked, labelled, renamed, doc] {
        free(d);
    }
}

fn export_layer(doc: *const PrecisionDocument, trimmed: bool, request: &Value) -> Vec<u8> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_layer_encode(
            doc,
            0,
            trimmed,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    unsafe { dr_export_result_free(result) };
    bytes
}

#[test]
fn export_layers_keeps_the_plane_untrimmed() {
    let dir = TempDir::new().unwrap();
    let doc = duplicate_t();
    let request = json!({"format":"png","profile":"document","dither":"none"});
    let bytes = export_layer(doc, false, &request);
    let path = dir.path().join("index.png");
    std::fs::write(&path, bytes).unwrap();
    let back = open_indexed(&path).unwrap();
    assert_eq!(indices(back), indices(doc));
    assert_eq!(color_table(back), color_table(doc));
    free(back);
    // Trimmed, it is the RGB storage's trimmed layer export.
    let rgb = to_rgb(doc);
    assert_eq!(
        export_layer(doc, true, &request),
        export_layer(rgb, true, &request)
    );
    free(rgb);
    free(doc);
}

#[test]
fn the_rest_refuses_by_name() {
    let doc = distinct_t();
    // The Index layer holds no mask: for good.
    for request in [
        json!({"op":"affine","layer":0,"target":"mask","matrix":[1.0,0.0,0.0,1.0,1.0,0.0],
               "filter":"nearest"}),
        json!({"op":"transform_selected","layer":0,"target":"mask",
               "matrix":[1.0,0.0,0.0,1.0,1.0,0.0],"filter":"nearest"}),
    ] {
        assert_eq!(
            paint(doc, &request, &rect(0, 0, 2, 2)),
            Err(MASK.into()),
            "{request}"
        );
    }
    assert_eq!(
        paint(doc, &json!({"op":"gradient","layer":0}), &[]),
        Err("gradient is not available in Indexed Color.".into())
    );
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_precision_doc_duplicate_layer(doc, 0, &mut err) }.is_null());
    assert_eq!(
        take_err_string(err),
        "duplicate_layer is not available in Indexed Color."
    );
    let layers = rgb_doc(2, 2, &[[1, 2, 3, 255]; 4]);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            layers,
            0,
            0,
            0,
            0,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "paste_layers is not available in Indexed Color."
    );
    free(layers);
    free(doc);
}

fn scratch(plan: &Value) -> u64 {
    plan["scratch_bytes"].as_u64().unwrap()
}

#[test]
fn each_plan_covers_its_term() {
    let doc = distinct_t();
    let rgb = to_rgb(doc);
    let canvas = u64::from(W * H);
    for request in [
        affine([0.9, 0.1, -0.1, 0.9, 1.0, 0.0], "bicubic"),
        affine([1.0, 0.0, 0.0, 1.0, 2.0, 1.0], "nearest"),
        json!({"op":"affine_set","layers":[0],"matrix":[1.0,0.0,0.0,1.0,1.0,0.0],
               "filter":"bicubic"}),
        json!({"op":"perspective","layer":0,"target":"pixels",
               "quad":[1.0,0.5,11.5,1.0,12.0,8.0,0.0,7.5],"filter":"bicubic"}),
        selected(1.5, 0.5, "bicubic", false),
    ] {
        let indexed = edit_plan(doc, &request).unwrap();
        let twin = edit_plan(rgb, &request).unwrap();
        assert_eq!(indexed["input_samples"], twin["input_samples"], "{request}");
        assert_eq!(indexed["input_width"], twin["input_width"], "{request}");
        let floor = scratch(&twin) + 2 * canvas * 10;
        assert!(
            scratch(&indexed) >= floor,
            "{request}: {} < {floor}",
            scratch(&indexed)
        );
    }
    free(rgb);
    free(doc);
}
