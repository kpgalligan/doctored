//! #547 Indexed Color phase 2c: the paint family (Fill, Stroke, the Paint
//! Bucket, the Eraser, Clear), Cut and the Image-menu geometry on an Indexed
//! document (`indexed_paint`, `indexed_geometry`, the generalised
//! `indexed_edge`).
//!
//! The oracles are independent of the kernel: a hand-computed case with
//! literal indices; for each paint op the expansion route (Image ▸ Mode ▸
//! RGB Color, the same request on that RGB document, then this file's own
//! re-index of its canvas window, written from the rule's definition); for
//! the geometry that only moves pixels or picks them Nearest Neighbor, the
//! index plane moved by hand (or, on a palette with no duplicate, the RGB op
//! read back by exact colour); for the resampling geometry the RGB op mapped
//! to each pixel's nearest entry by this file's own search.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_clipboard_payload_free, dr_precision_doc_clipboard_copy,
    dr_precision_doc_clipboard_cut, DrClipboardPayload,
};
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::indexed_edge::{reindex_codes_with, EdgeInk};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{
    background, color_table, edit_plan, free, indices, layer16, open_rgb, oracle, paint,
    palette_of, render16, rgb_doc, set_table, to_indexed, to_rgb, window, Ink, LIMIT,
};

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

/// `doc` with its table replaced (`set_table`).
fn retabled(doc: *mut PrecisionDocument, table: Value) -> *mut PrecisionDocument {
    let out = set_table(doc, &table).unwrap().unwrap();
    free(doc);
    out
}

/// The clear-row scene with a table of distinct colours: t's matte made unique.
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

/// The opaque scene (no transparent index) with entry 1 a duplicate of entry
/// 0 and an unused white entry 3.
fn duplicate_opaque() -> *mut PrecisionDocument {
    retabled(
        scene(false),
        json!({"colors":[[0,0,0],[0,0,0],[190,50,50],[255,255,255]],"transparent_index":null}),
    )
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

const INK: [u8; 3] = [220, 30, 30];

fn ink_json() -> Value {
    json!([220.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0, 1.0])
}

#[test]
fn the_kernel_rule_by_hand() {
    // 0 black, 1 white, 2 (200, 0, 0), 3 (100, 100, 100) the transparent index.
    let palette = [[0, 0, 0], [255, 255, 255], [200, 0, 0], [100, 100, 100]];
    let t = Some(3);
    // Own against a backdrop: unchanged black keeps 0; (250, 250, 250) on
    // black is nearest white 1; alpha 50 takes t; on t, (190, 10, 10) is
    // nearest (200, 0, 0), 2.
    let out = reindex_codes_with(
        &palette,
        t,
        4,
        1,
        Some(&[0, 0, 0, 3]),
        &[
            [0, 0, 0, 255],
            [250, 250, 250, 255],
            [9, 9, 9, 50],
            [190, 10, 10, 255],
        ],
        EdgeInk::Own,
    )
    .unwrap();
    assert_eq!(out, vec![0, 1, 3, 2]);
    // Clear: white at alpha 200 keeps 1, at alpha 100 takes t, red unchanged 2.
    let out = reindex_codes_with(
        &palette,
        t,
        3,
        1,
        Some(&[1, 1, 2]),
        &[[255, 255, 255, 200], [255, 255, 255, 100], [200, 0, 0, 255]],
        EdgeInk::Clear,
    )
    .unwrap();
    assert_eq!(out, vec![1, 3, 2]);
    // No backdrop, Own: every pixel is changed. (100, 100, 100) is t's colour
    // but never t: black (30,000) and (200, 0, 0) (30,000) tie, the lower 0.
    let out = reindex_codes_with(
        &palette,
        t,
        4,
        1,
        None,
        &[
            [10, 10, 10, 255],
            [240, 240, 240, 255],
            [0, 0, 0, 0],
            [100, 100, 100, 255],
        ],
        EdgeInk::Own,
    )
    .unwrap();
    assert_eq!(out, vec![0, 1, 3, 0]);
    // No backdrop, Fixed: every opaque pixel the ink's nearest.
    let out = reindex_codes_with(
        &palette,
        t,
        2,
        1,
        None,
        &[[0, 0, 0, 255], [255, 255, 255, 200]],
        EdgeInk::Fixed([210, 10, 10]),
    )
    .unwrap();
    assert_eq!(out, vec![2, 2]);
    // A Fixed ink with a backdrop is the 2b rule: on white, (237, 157, 157)
    // keeps 1 (255 num = 12,451,650 < 131 den = 15,991,825).
    let out = reindex_codes_with(
        &palette,
        t,
        1,
        1,
        Some(&[1]),
        &[[237, 157, 157, 255]],
        EdgeInk::Fixed([210, 10, 10]),
    )
    .unwrap();
    assert_eq!(out, vec![1]);
    // No transparent index (phase 3a): a clear pixel takes the entry nearest
    // white, 1, an unchanged one keeps its index.
    let opaque = [[0, 0, 0], [255, 255, 255]];
    assert_eq!(background(&opaque, None), 1);
    assert_eq!(
        reindex_codes_with(
            &opaque,
            None,
            2,
            1,
            Some(&[0, 0]),
            &[[0, 0, 0, 255], [0, 0, 0, 10]],
            EdgeInk::Clear
        ),
        Ok(vec![0, 1])
    );
    assert_eq!(
        reindex_codes_with(
            &opaque,
            None,
            1,
            1,
            Some(&[0]),
            &[[0, 0, 0, 255]],
            EdgeInk::Clear
        ),
        Ok(vec![0])
    );
    // Clear needs a backdrop.
    assert!(
        reindex_codes_with(&palette, t, 1, 1, None, &[[0, 0, 0, 255]], EdgeInk::Clear).is_err()
    );
}

/// The paint requests of (b), each with its input and its ink.
fn paint_cases() -> Vec<(&'static str, Value, Vec<u16>, Ink)> {
    let n = (W * H) as usize;
    vec![
        (
            "bucket_fill",
            json!({"op":"bucket_fill","layer":0,"color":ink_json(),"color_space":"document",
                   "opacity":1.0,"blend_mode":0,"x":5,"y":2,"tolerance":0,"contiguous":true}),
            vec![65535; n],
            Ink::Fixed(INK),
        ),
        (
            "paint",
            json!({"op":"paint","layer":0,"color":ink_json(),"color_space":"document","opacity":1.0}),
            disc(),
            Ink::Fixed(INK),
        ),
        (
            "fill",
            json!({"op":"paint_blend","layer":0,"color":ink_json(),"color_space":"document",
                   "opacity":0.6,"blend_mode":0}),
            vec![65535; n],
            Ink::Fixed(INK),
        ),
        (
            "stroke_layer_edge",
            json!({"op":"stroke_layer_edge","layer":0,"width":2.0,"location":"outside",
                   "color":ink_json(),"color_space":"document","opacity":1.0,"blend_mode":0}),
            vec![],
            Ink::Fixed(INK),
        ),
        (
            "erase_native",
            json!({"op":"erase_native","layer":0,"opacity":1.0}),
            disc(),
            Ink::Clear,
        ),
        (
            "erase",
            json!({"op":"erase","layer":0,"opacity":1.0}),
            disc(),
            Ink::Clear,
        ),
    ]
}

#[test]
fn each_paint_op_is_the_rgb_op_reindexed() {
    let doc = scene(true);
    let (palette, t) = palette_of(doc);
    assert!(t.is_some());
    let before = indices(doc);
    for (name, request, input, ink) in paint_cases() {
        let painted = paint(doc, &request, &input)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .unwrap_or_else(|| panic!("{name}: no index moved"));
        let rgb = to_rgb(doc);
        let twin = paint(rgb, &request, &input)
            .unwrap_or_else(|e| panic!("{name} on RGB: {e}"))
            .unwrap_or_else(|| panic!("{name} on RGB: nothing changed"));
        let expected = oracle(&palette, t, Some(&before), &window(twin), ink).unwrap();
        let got = indices(painted);
        assert_eq!(got, expected, "{name}");
        assert_ne!(got, before, "{name}");
        assert_renders(painted);
        free(twin);
        free(rgb);
        free(painted);
    }
    free(doc);
}

fn copy(doc: *const PrecisionDocument, request: &Value, coverage: &[u16]) -> Vec<u8> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let payload = unsafe {
        dr_precision_doc_clipboard_copy(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!payload.is_null(), "{}", take_err_string(err));
    let bytes = payload_bytes(payload);
    unsafe { dr_clipboard_payload_free(payload) };
    bytes
}

fn payload_bytes(payload: *const DrClipboardPayload) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_clipboard_payload_bytes(payload, &mut length) };
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

/// `dr_precision_doc_clipboard_cut`: the payload's bytes and the document,
/// or the error with the payload checked NULL.
type Cut = Result<(Vec<u8>, Option<*mut PrecisionDocument>), String>;

fn cut(doc: *const PrecisionDocument, coverage: &[u16]) -> Cut {
    let text =
        CString::new(json!({"op":"cut","layer":0,"x":0,"y":0,"width":W,"height":H}).to_string())
            .unwrap();
    let mut err = ptr::null_mut();
    let mut payload = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_cut(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut payload,
            &mut err,
        )
    };
    if !err.is_null() {
        assert!(payload.is_null());
        assert!(out.is_null());
        return Err(take_err_string(err));
    }
    let bytes = payload_bytes(payload);
    unsafe { dr_clipboard_payload_free(payload) };
    Ok((bytes, (!out.is_null()).then_some(out)))
}

#[test]
fn cut_is_copy_then_the_erase() {
    let doc = scene(true);
    let coverage = disc();
    let copied = copy(
        doc,
        &json!({"op":"copy","layer":0,"x":0,"y":0,"width":W,"height":H}),
        &coverage,
    );
    let (bytes, next) = cut(doc, &coverage).unwrap();
    assert_eq!(bytes, copied);
    let next = next.expect("the cut moves indices");
    let erased = paint(
        doc,
        &json!({"op":"erase","layer":0,"opacity":1.0}),
        &coverage,
    )
    .unwrap()
    .unwrap();
    assert_eq!(indices(next), indices(erased));
    assert_eq!(render16(next), render16(erased));
    assert_renders(next);
    free(erased);
    free(next);
    free(doc);

    // No transparent index (phase 3a): each cut pixel takes the entry
    // nearest white, the RGB erase re-indexed by the background rule.
    let opaque = scene(false);
    let (palette, t) = palette_of(opaque);
    assert_eq!(t, None);
    let before = indices(opaque);
    let (bytes, next) = cut(opaque, &coverage).unwrap();
    assert_eq!(
        bytes,
        copy(
            opaque,
            &json!({"op":"copy","layer":0,"x":0,"y":0,"width":W,"height":H}),
            &coverage,
        )
    );
    let next = next.expect("the cut moves indices");
    let rgb = to_rgb(opaque);
    let twin = paint(
        rgb,
        &json!({"op":"erase","layer":0,"opacity":1.0}),
        &coverage,
    )
    .unwrap()
    .unwrap();
    let expected = oracle(&palette, t, Some(&before), &window(twin), Ink::Clear).unwrap();
    assert_eq!(indices(next), expected);
    let bg = background(&palette, None);
    assert!(expected
        .iter()
        .zip(&before)
        .any(|(e, b)| *e == bg && *b != bg));
    assert_renders(next);
    free(twin);
    free(rgb);
    free(next);
    free(opaque);
}

/// `before` (a `w`×`h` plane) moved onto an `nw`×`nh` canvas: each new
/// pixel reads the old one `source` names, `fill` where it names none or
/// one off the old canvas.
fn moved(
    before: &[i32],
    (w, h): (u32, u32),
    (nw, nh): (u32, u32),
    source: impl Fn(i64, i64) -> (i64, i64),
    fill: i32,
) -> Vec<i32> {
    let mut out = Vec::new();
    for y in 0..i64::from(nh) {
        for x in 0..i64::from(nw) {
            let (sx, sy) = source(x, y);
            out.push(
                if sx >= 0 && sy >= 0 && sx < i64::from(w) && sy < i64::from(h) {
                    before[(sy * i64::from(w) + sx) as usize]
                } else {
                    fill
                },
            );
        }
    }
    out
}

/// The geometry's result, which must exist.
fn geometry(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    paint(doc, request, &[])
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request}: nothing moved"))
}

#[test]
fn the_exact_geometry_moves_the_index_plane() {
    let doc = duplicate_t();
    let before = indices(doc);
    assert!(before.contains(&1), "the duplicate entry is in use");
    let (w, h) = (i64::from(W), i64::from(H));
    let size = (W, H);
    type Map = Box<dyn Fn(i64, i64) -> (i64, i64)>;
    let cases: Vec<(Value, (u32, u32), Map)> = vec![
        (
            json!({"op":"rotate90"}),
            (H, W),
            Box::new(move |x, y| (y, h - 1 - x)),
        ),
        (
            json!({"op":"rotate180"}),
            size,
            Box::new(move |x, y| (w - 1 - x, h - 1 - y)),
        ),
        (
            json!({"op":"rotate270"}),
            (H, W),
            Box::new(move |x, y| (w - 1 - y, x)),
        ),
        (
            json!({"op":"flip_horizontal"}),
            size,
            Box::new(move |x, y| (w - 1 - x, y)),
        ),
        (
            json!({"op":"flip_vertical"}),
            size,
            Box::new(move |x, y| (x, h - 1 - y)),
        ),
        (
            json!({"op":"crop","x":2,"y":1,"width":6,"height":5}),
            (6, 5),
            Box::new(|x, y| (x + 2, y + 1)),
        ),
        (
            json!({"op":"canvas_size","width":16,"height":10,"x":2,"y":1}),
            (16, 10),
            Box::new(|x, y| (x - 2, y - 1)),
        ),
    ];
    for (request, new_size, source) in cases {
        let out = geometry(doc, &request);
        assert_eq!(unsafe { &*out }.dimensions(), new_size, "{request}");
        assert_eq!(
            indices(out),
            moved(&before, size, new_size, source, 3),
            "{request}"
        );
        assert_renders(out);
        free(out);
    }
    free(doc);
}

/// The zero-angle crop past the canvas the Crop tool and MCP `crop` send.
fn extended_crop(delete_cropped: bool) -> Value {
    json!({"op":"straighten","matrix":[1.0,0.0,0.0,1.0,0.0,0.0],"filter":"nearest",
           "x":-2,"y":-1,"width":16,"height":10,"delete_cropped":delete_cropped,
           "background":{"color":[1.0,1.0,1.0]}})
}

#[test]
fn a_crop_past_the_canvas_keeps_every_index() {
    let source = |x: i64, y: i64| (x - 2, y - 1);
    // An opaque document with Delete Cropped Pixels: the new area is the
    // entry nearest white (3, unused before), as the RGB route pads it.
    let opaque = duplicate_opaque();
    let before = indices(opaque);
    let out = geometry(opaque, &extended_crop(true));
    assert_eq!(indices(out), moved(&before, (W, H), (16, 10), source, 3));
    assert_renders(out);
    free(out);
    // Without Delete Cropped Pixels there is no pad: the new area takes the
    // background rule's entry (phase 3a), here the same unused white 3.
    let (palette, t) = palette_of(opaque);
    let out = geometry(opaque, &extended_crop(false));
    assert_eq!(
        indices(out),
        moved(&before, (W, H), (16, 10), source, background(&palette, t))
    );
    assert_renders(out);
    free(out);
    free(opaque);
    // A document with t: the new area is clear.
    let doc = duplicate_t();
    let before = indices(doc);
    let out = geometry(doc, &extended_crop(false));
    assert_eq!(indices(out), moved(&before, (W, H), (16, 10), source, 3));
    assert_renders(out);
    free(out);
    free(doc);
}

/// The RGB op on `doc`'s expansion, read back by exact colour (a palette
/// with no duplicate): alpha 255 is its one opaque entry, alpha 0 t, or with
/// no t the background rule's entry.
fn exact_oracle(doc: *const PrecisionDocument, request: &Value) -> Vec<i32> {
    let (palette, t) = palette_of(doc);
    let rgb = to_rgb(doc);
    let twin = geometry(rgb, request);
    let out = window(twin)
        .iter()
        .map(|px| match px[3] {
            255 => (0..palette.len())
                .find(|&i| Some(i as u8) != t && palette[i] == [px[0], px[1], px[2]])
                .unwrap_or_else(|| panic!("{px:?} is no entry")) as i32,
            0 => background(&palette, t),
            _ => panic!("{request}: a Nearest pick left alpha {}", px[3]),
        })
        .collect();
    free(twin);
    free(rgb);
    out
}

#[test]
fn nearest_resize_and_rotation_are_the_rgb_op() {
    let doc = distinct_t();
    for request in [
        json!({"op":"resize","width":24,"height":13,"filter":"nearest"}),
        json!({"op":"rotate_canvas","degrees":30.0,"filter":"nearest"}),
    ] {
        let out = geometry(doc, &request);
        assert_eq!(indices(out), exact_oracle(doc, &request), "{request}");
        assert_renders(out);
        free(out);
    }
    free(doc);
}

#[test]
fn resampling_is_the_rgb_op_to_the_nearest_entry() {
    let doc = distinct_t();
    let (palette, t) = palette_of(doc);
    for request in [
        json!({"op":"resize","width":18,"height":11,"filter":"bicubic"}),
        json!({"op":"rotate_canvas","degrees":10.0,"filter":"bicubic"}),
        json!({"op":"perspective_crop","quad":[1.0,1.0,11.0,0.0,12.0,8.0,0.0,7.0],
               "width":10,"height":6,"filter":"bicubic"}),
    ] {
        let out = geometry(doc, &request);
        let rgb = to_rgb(doc);
        let twin = geometry(rgb, &request);
        let expected = oracle(&palette, t, None, &window(twin), Ink::Own).unwrap();
        assert_eq!(indices(out), expected, "{request}");
        assert_renders(out);
        free(twin);
        free(rgb);
        free(out);
    }
    free(doc);
}

#[test]
fn a_document_with_no_transparent_index_clears_to_the_background_entry() {
    let opaque = scene(false);
    let (palette, t) = palette_of(opaque);
    assert_eq!(t, None);
    let bg = background(&palette, None);
    let before = indices(opaque);
    let out = geometry(
        opaque,
        &json!({"op":"canvas_size","width":14,"height":9,"x":1,"y":0}),
    );
    assert_eq!(
        indices(out),
        moved(&before, (W, H), (14, 9), |x, y| (x - 1, y), bg)
    );
    assert_renders(out);
    free(out);
    let request = json!({"op":"rotate_canvas","degrees":30.0,"filter":"nearest"});
    let out = geometry(opaque, &request);
    assert_eq!(indices(out), exact_oracle(opaque, &request));
    assert_renders(out);
    free(out);
    let request = json!({"op":"erase_native","layer":0,"opacity":1.0});
    let out = paint(opaque, &request, &disc()).unwrap().unwrap();
    let rgb = to_rgb(opaque);
    let twin = paint(rgb, &request, &disc()).unwrap().unwrap();
    let expected = oracle(&palette, t, Some(&before), &window(twin), Ink::Clear).unwrap();
    assert_eq!(indices(out), expected);
    assert!(expected
        .iter()
        .zip(&before)
        .any(|(e, b)| *e == bg && *b != bg));
    assert_renders(out);
    free(twin);
    free(rgb);
    free(out);
    free(opaque);
}

#[test]
fn the_refusals_name_their_reason() {
    let opaque = scene(false);
    let n = (W * H) as usize;
    // Phase 3b: the Paint Bucket and Stroke… in Multiply are admitted
    // (`indexed_blend_modes.rs` checks the rule).
    let bucket = json!({"op":"bucket_fill","layer":0,"color":ink_json(),"color_space":"document",
        "opacity":1.0,"blend_mode":1,"x":5,"y":2,"tolerance":0,"contiguous":true});
    let filled = paint(opaque, &bucket, &vec![65535; n]).unwrap();
    filled.into_iter().for_each(free);
    let stroke = json!({"op":"stroke_layer_edge","layer":0,"width":2.0,"location":"inside",
        "color":ink_json(),"color_space":"document","opacity":1.0,"blend_mode":1});
    let stroked = paint(opaque, &stroke, &[]).unwrap();
    stroked.into_iter().for_each(free);
    assert!(edit_plan(opaque, &stroke).is_ok());
    // Phase 3c: the Gradient and Dodge/Burn refuse for good; a selection of
    // no kind is offered on RGB only.
    for (request, op) in [
        (json!({"op":"gradient","layer":0}), "gradient"),
        (json!({"op":"dodge_burn","layer":0}), "dodge_burn"),
    ] {
        assert_eq!(
            paint(opaque, &request, &[]),
            Err(format!("{op} is not available in Indexed Color."))
        );
    }
    assert_eq!(
        paint(opaque, &json!({"op":"selection"}), &[]),
        Err(
            "selection is unavailable in an Indexed Color document: Doctored offers it on RGB \
             documents only."
                .into()
        )
    );
    free(opaque);
}

fn scratch(plan: &Value) -> u64 {
    plan["scratch_bytes"].as_u64().unwrap()
}

#[test]
fn each_plan_covers_its_reindex() {
    let doc = scene(true);
    let rgb = to_rgb(doc);
    let canvas = u64::from(W * H);
    for (name, request, _, _) in paint_cases() {
        let indexed = edit_plan(doc, &request).unwrap();
        let twin = edit_plan(rgb, &request).unwrap();
        assert_eq!(indexed["input_samples"], twin["input_samples"], "{name}");
        let floor = scratch(&twin) + canvas * 10;
        assert!(
            scratch(&indexed) >= floor,
            "{name}: {} < {floor}",
            scratch(&indexed)
        );
    }
    for (request, new) in [
        (json!({"op":"rotate90"}), canvas),
        (
            json!({"op":"resize","width":24,"height":16,"filter":"bicubic"}),
            4 * canvas,
        ),
        (json!({"op":"crop","x":2,"y":1,"width":6,"height":5}), 30),
    ] {
        let indexed = edit_plan(doc, &request).unwrap();
        let twin = edit_plan(rgb, &request).unwrap();
        assert_eq!(indexed["input_samples"], json!(0));
        let floor = scratch(&twin) + (canvas + new) * 10;
        assert!(
            scratch(&indexed) >= floor,
            "{request}: {} < {floor}",
            scratch(&indexed)
        );
    }
    free(rgb);
    free(doc);
}

#[test]
fn a_cropped_document_round_trips() {
    let dir = TempDir::new().unwrap();
    let doc = duplicate_t();
    let cropped = geometry(doc, &json!({"op":"crop","x":1,"y":2,"width":9,"height":6}));
    let path = dir.path().join("cropped.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(cropped, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let back = open_rgb(&path).unwrap();
    assert_eq!(indices(back), indices(cropped));
    assert_eq!(render16(back), render16(cropped));
    assert_eq!(color_table(back), color_table(cropped));
    free(back);
    free(cropped);
    free(doc);
}
