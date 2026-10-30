//! #1 Indexed Color phase 3b: the background colour a clear pixel takes on
//! a document with no transparent index (`indexed_paint::BACKGROUND_KEY`,
//! `indexed_edge::clear_index`): the request's `indexed_background` (the
//! host's background swatch), else white. Phase 3c adds the request shapes
//! of the host's merged-key branch and Cut's own JSON error prefix.
//!
//! The oracles are independent of the kernel: the key colour's 8-bit codes
//! computed here from `CropBackground`'s `document` preparation (×65535,
//! rounded, then the 8-bit code), the entry nearest them by this file's
//! search (`background_of`), the index plane moved by hand for the exact
//! moves, and the RGB erase re-indexed by the shared oracle (`oracle_with`)
//! for the erasers and Cut.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_clipboard::{dr_clipboard_payload_free, dr_precision_doc_clipboard_cut};
use doctored_core::ffi_warp::dr_precision_doc_warp_edit;
use doctored_core::indexed_edge::{reindex_codes_on, EdgeInk};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::indexed_fixture::{
    background_of, free, indices, oracle_with, paint, palette_of, rgb_doc, to_indexed, to_rgb,
    window, Ink, LIMIT,
};

const W: u32 = 12;
const H: u32 = 8;

/// The key colour, document space.
const KEY: [f64; 3] = [0.2, 0.6, 0.1];

/// The key colour's 8-bit codes: each value stored as a 16-bit sample, then
/// read back as the nearest 8-bit code.
fn key_codes() -> [u8; 3] {
    KEY.map(|v| {
        let wide = (v * 65535.0).round() as u32;
        ((wide + 128) / 257) as u8
    })
}

fn key() -> Value {
    json!({"color":KEY,"color_space":"document"})
}

/// `request` with the key added.
fn keyed(request: &Value) -> Value {
    let mut out = request.clone();
    out["indexed_background"] = key();
    out
}

/// The 12 × 8 scene: black, white, (190, 50, 50) and (40, 160, 60) bands
/// with a marker, its last row clear (the transparent index) when
/// `clear_row`.
fn scene(clear_row: bool) -> *mut PrecisionDocument {
    let mut pixels = Vec::new();
    for y in 0..H {
        for x in 0..W {
            pixels.push(if clear_row && y == H - 1 {
                [0, 0, 0, 0]
            } else if (x, y) == (1, 1) {
                [190, 50, 50, 255]
            } else if x < 3 {
                [0, 0, 0, 255]
            } else if x < 6 {
                [255, 255, 255, 255]
            } else if x < 9 {
                [190, 50, 50, 255]
            } else {
                [40, 160, 60, 255]
            });
        }
    }
    let rgb = rgb_doc(W, H, &pixels);
    let out = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    out
}

/// The key's entry on the opaque scene, checked to differ from white's.
fn key_entry(palette: &[[u8; 3]]) -> i32 {
    let bg = background_of(palette, None, key_codes());
    let white = background_of(palette, None, [255, 255, 255]);
    assert_ne!(bg, white, "the key's entry is not white's");
    bg
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

/// `before` (a W×H plane) moved onto an `nw`×`nh` canvas: each new pixel
/// reads the old one `source` names, `fill` where it names one off the old
/// canvas.
fn moved(
    before: &[i32],
    (nw, nh): (u32, u32),
    source: impl Fn(i64, i64) -> (i64, i64),
    fill: i32,
) -> Vec<i32> {
    let mut out = Vec::new();
    for y in 0..i64::from(nh) {
        for x in 0..i64::from(nw) {
            let (sx, sy) = source(x, y);
            out.push(
                if sx >= 0 && sy >= 0 && sx < i64::from(W) && sy < i64::from(H) {
                    before[(sy * i64::from(W) + sx) as usize]
                } else {
                    fill
                },
            );
        }
    }
    out
}

fn edited(doc: *const PrecisionDocument, request: &Value, input: &[u16]) -> *mut PrecisionDocument {
    paint(doc, request, input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request}: nothing moved"))
}

fn warp(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_warp_edit(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

fn cut(doc: *const PrecisionDocument, request: &Value, coverage: &[u16]) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
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
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    unsafe { dr_clipboard_payload_free(payload) };
    out
}

/// The RGB erase of `doc` (`erase_native`, `coverage`) re-indexed with the
/// `Clear` ink and the `background` colour.
fn erase_oracle(doc: *const PrecisionDocument, coverage: &[u16], background: [u8; 3]) -> Vec<i32> {
    let (palette, t) = palette_of(doc);
    let rgb = to_rgb(doc);
    let twin = edited(
        rgb,
        &json!({"op":"erase_native","layer":0,"opacity":1.0}),
        coverage,
    );
    let out = oracle_with(
        &palette,
        t,
        Some(&indices(doc)),
        &window(twin),
        Ink::Clear,
        background,
    )
    .unwrap();
    free(twin);
    free(rgb);
    out
}

#[test]
fn the_erasers_and_cut_clear_to_the_keys_entry() {
    let doc = scene(false);
    let (palette, t) = palette_of(doc);
    assert_eq!(t, None);
    let bg = key_entry(&palette);
    let before = indices(doc);
    let coverage = disc();
    let expected = erase_oracle(doc, &coverage, key_codes());
    assert!(
        expected
            .iter()
            .zip(&before)
            .any(|(e, b)| *e == bg && *b != bg),
        "some erased pixel takes the key's entry"
    );
    for request in [
        json!({"op":"erase_native","layer":0,"opacity":1.0}),
        json!({"op":"erase","layer":0,"opacity":1.0}),
    ] {
        let out = edited(doc, &keyed(&request), &coverage);
        assert_eq!(indices(out), expected, "{request}");
        free(out);
    }
    let request = json!({"op":"cut","layer":0,"x":0,"y":0,"width":W,"height":H});
    let out = cut(doc, &keyed(&request), &coverage);
    assert_eq!(indices(out), expected, "cut");
    free(out);
    // Absent, the colour is white, as before.
    let out = cut(doc, &request, &coverage);
    assert_eq!(
        indices(out),
        erase_oracle(doc, &coverage, [255, 255, 255]),
        "cut, no key"
    );
    free(out);
    free(doc);
}

#[test]
fn the_moves_uncover_the_keys_entry() {
    let doc = scene(false);
    let (palette, _) = palette_of(doc);
    let bg = key_entry(&palette);
    let before = indices(doc);
    // Canvas Size growth.
    let out = edited(
        doc,
        &keyed(&json!({"op":"canvas_size","width":14,"height":9,"x":1,"y":0})),
        &[],
    );
    assert_eq!(indices(out), moved(&before, (14, 9), |x, y| (x - 1, y), bg));
    free(out);
    // A whole-pixel layer translation.
    let request = json!({"op":"affine_set","layers":[0],"matrix":[1.0,0.0,0.0,1.0,2.0,1.0],
                         "filter":"bilinear"});
    let out = edited(doc, &keyed(&request), &[]);
    assert_eq!(
        indices(out),
        moved(&before, (W, H), |x, y| (x - 2, y - 1), bg)
    );
    free(out);
    // A warp set that translates.
    let request = json!({"op":"set","layer":0,"target":"pixels","columns":2,"rows":2,
        "displacements":[[2.0,1.0],[2.0,1.0],[2.0,1.0],[2.0,1.0]],"filter":"bicubic",
        "indexed_background":key()});
    let out = warp(doc, &request);
    assert_eq!(
        indices(out),
        moved(&before, (W, H), |x, y| (x - 2, y - 1), bg)
    );
    free(out);
    // The Move tool's hole.
    let coverage = rect(2, 1, 6, 4);
    let mut expected = before.clone();
    for (i, &c) in coverage.iter().enumerate() {
        if c == 65535 {
            expected[i] = bg;
        }
    }
    for y in 0..H {
        for x in 0..W {
            let i = (y * W + x) as usize;
            let (nx, ny) = (x + 2, y + 1);
            if coverage[i] == 65535 && nx < W && ny < H {
                expected[(ny * W + nx) as usize] = before[i];
            }
        }
    }
    let request = json!({"op":"move_selected","layer":0,"dx":2,"dy":1,"duplicate":false});
    let out = edited(doc, &keyed(&request), &coverage);
    assert_eq!(indices(out), expected, "move_selected");
    free(out);
    free(doc);
}

/// The zero-angle crop past the canvas with its own `background`.
fn extended_crop(delete_cropped: bool) -> Value {
    json!({"op":"straighten","matrix":[1.0,0.0,0.0,1.0,0.0,0.0],"filter":"nearest",
           "x":-2,"y":-1,"width":16,"height":10,"delete_cropped":delete_cropped,
           "background":key()})
}

#[test]
fn a_straighten_takes_its_own_background() {
    let doc = scene(false);
    let (palette, _) = palette_of(doc);
    let bg = key_entry(&palette);
    let before = indices(doc);
    for delete_cropped in [true, false] {
        let out = edited(doc, &extended_crop(delete_cropped), &[]);
        assert_eq!(
            indices(out),
            moved(&before, (16, 10), |x, y| (x - 2, y - 1), bg),
            "delete_cropped {delete_cropped}"
        );
        free(out);
    }
    free(doc);
}

/// The selected pixels' hole and landing for a whole-pixel move of `coverage`
/// by (2, 1), the hole taking `bg`.
fn moved_selection(before: &[i32], coverage: &[u16], bg: i32) -> Vec<i32> {
    let mut expected = before.to_vec();
    for (i, &c) in coverage.iter().enumerate() {
        if c == 65535 {
            expected[i] = bg;
        }
    }
    for y in 0..H {
        for x in 0..W {
            let i = (y * W + x) as usize;
            let (nx, ny) = (x + 2, y + 1);
            if coverage[i] == 65535 && nx < W && ny < H {
                expected[(ny * W + nx) as usize] = before[i];
            }
        }
    }
    expected
}

/// Phase 3c: the request shapes `PrecisionGeometryWork.compute`'s merged-key
/// branch sends (the Move tool and Free Transform with the swatch): the
/// layer's `affine` translation (`.transform`), `transform_selected` and
/// `move_selected` (the selected pixels), and the Crop tool's window past
/// the canvas at angle 0 (`straighten` with its own background and the key).
#[test]
fn the_merged_key_branchs_requests_uncover_the_keys_entry() {
    let doc = scene(false);
    let (palette, _) = palette_of(doc);
    let bg = key_entry(&palette);
    let before = indices(doc);
    let request = json!({"op":"affine","layer":0,"target":"pixels",
                         "matrix":[1.0,0.0,0.0,1.0,2.0,1.0],"filter":"bilinear"});
    let out = edited(doc, &keyed(&request), &[]);
    assert_eq!(
        indices(out),
        moved(&before, (W, H), |x, y| (x - 2, y - 1), bg),
        "affine"
    );
    free(out);
    let coverage = rect(2, 1, 6, 4);
    let expected = moved_selection(&before, &coverage, bg);
    for request in [
        json!({"op":"transform_selected","layer":0,"matrix":[1.0,0.0,0.0,1.0,2.0,1.0],
               "filter":"bilinear","duplicate":false}),
        json!({"op":"move_selected","layer":0,"dx":2,"dy":1,"duplicate":false}),
    ] {
        let out = edited(doc, &keyed(&request), &coverage);
        assert_eq!(indices(out), expected, "{request}");
        free(out);
    }
    let out = edited(doc, &keyed(&extended_crop(false)), &[]);
    assert_eq!(
        indices(out),
        moved(&before, (16, 10), |x, y| (x - 2, y - 1), bg),
        "crop"
    );
    free(out);
    free(doc);
}

#[test]
fn an_invalid_cut_names_the_cuts_own_prefix() {
    let doc = scene(false);
    let text = CString::new("{not json").unwrap();
    let coverage = disc();
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
    assert!(out.is_null());
    let message = take_err_string(err);
    assert!(
        message.starts_with("invalid clipboard cut request: "),
        "{message}"
    );
    free(doc);
}

#[test]
fn with_a_transparent_index_the_key_changes_nothing() {
    let doc = scene(true);
    let (_, t) = palette_of(doc);
    assert!(t.is_some());
    let coverage = disc();
    let erase = json!({"op":"erase_native","layer":0,"opacity":1.0});
    let grow = json!({"op":"canvas_size","width":14,"height":9,"x":1,"y":0});
    for (request, input) in [(erase, coverage.clone()), (grow, vec![])] {
        let plain = edited(doc, &request, &input);
        let with_key = edited(doc, &keyed(&request), &input);
        assert_eq!(indices(with_key), indices(plain), "{request}");
        free(with_key);
        free(plain);
    }
    free(doc);
}

#[test]
fn the_key_is_indexed_only() {
    let rgb = rgb_doc(2, 1, &[[1, 2, 3, 255], [4, 5, 6, 255]]);
    let request = keyed(
        &json!({"op":"paint_blend","layer":0,"color":[1.0,0.0,0.0,1.0],
        "color_space":"document","opacity":1.0,"blend_mode":0}),
    );
    assert!(paint(rgb, &request, &[65535, 65535]).is_err());
    free(rgb);
    // An ink-shaped key refuses on an Indexed document.
    let doc = scene(false);
    let mut request = json!({"op":"erase_native","layer":0,"opacity":1.0});
    request["indexed_background"] = json!({"ink":[0.0,0.0,0.0,1.0]});
    assert_eq!(
        paint(doc, &request, &disc()),
        Err("crop background takes color on an RGB document and ink on a CMYK one".into())
    );
    free(doc);
}

#[test]
fn the_background_is_read_only_for_a_clear_pixel() {
    // No pixel ends clear: any background gives the same plane.
    let palette = [[0, 0, 0], [255, 255, 255], [40, 160, 60]];
    let before = [0u8, 1, 2, 1];
    let after = [
        [0, 0, 0, 255],
        [220, 30, 30, 255],
        [40, 160, 60, 255],
        [250, 250, 250, 255],
    ];
    let ink = EdgeInk::Fixed([220, 30, 30]);
    let a = reindex_codes_on(&palette, None, 4, 1, Some(&before), &after, ink, [0, 0, 0]);
    let b = reindex_codes_on(
        &palette,
        None,
        4,
        1,
        Some(&before),
        &after,
        ink,
        [40, 160, 60],
    );
    assert_eq!(a, b);
    // A clear pixel takes the background's entry.
    let mut cleared = after;
    cleared[3] = [250, 250, 250, 10];
    let out = reindex_codes_on(
        &palette,
        None,
        4,
        1,
        Some(&before),
        &cleared,
        ink,
        [50, 150, 50],
    )
    .unwrap();
    assert_eq!(out[3], 2);
    assert_eq!(
        i32::from(out[3]),
        background_of(&palette, None, [50, 150, 50])
    );
}
