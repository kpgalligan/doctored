//! #1 Indexed Color phase 2b: the Brush on an Indexed document
//! (`indexed_paint`) and its edge rule (`indexed_edge`, THRESH at k = 131,
//! the controller's RULING-1 fallback with its residual disclosed in
//! `designs/indexed-mode.md`).
//!
//! The oracles are independent of the kernel: a hand-computed case with
//! literal indices, and for the Brush the expansion route (Image ▸ Mode ▸
//! RGB Color, the same request on that RGB document, then this file's own
//! re-index of its pixels, written from the rule's definition).

use std::ptr;

use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::indexed_edge::reindex_codes;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{
    color_table, edit_plan, free, indices, layer16, open_rgb, paint, palette_of, render16, rgb_doc,
    to_indexed, LIMIT,
};

/// The rule's threshold, written here from the docstring's fallback
/// (`scripts/photoshop-probe/indexed_mode_take.py`, "Build table").
const K: i128 = 131;

#[test]
fn the_rule_on_a_hand_computed_case() {
    // Palette: 0 black, 1 white, 2 (200, 0, 0), 3 (100, 100, 100) the
    // transparent index (its render is alpha 0).
    let palette = [[0, 0, 0], [255, 255, 255], [200, 0, 0], [100, 100, 100]];
    let t = Some(3);
    // Ink (210, 10, 10): E = 2, (200, 0, 0) at distance 300; black is
    // 44,300 and white 2 × 60,025 + 2,025 away.
    let before = [0, 1, 1, 1, 3, 3, 0];
    let after = [
        // Unchanged: black's own render keeps 0.
        [0, 0, 0, 255],
        // Full ink on white: c = ink, so num = den and alpha = 1: E = 2.
        [210, 10, 10, 255],
        // On white, c - s = (-27, -147, -147) against ink - s = (-45, -245,
        // -245): num = 1,215 + 2 × 36,015 = 73,245, den = 122,075;
        // 255 num = 18,677,475 >= 131 den = 15,991,825: E = 2.
        [228, 108, 108, 255],
        // On white, c - s = (-18, -98, -98): num = 810 + 2 × 24,010 =
        // 48,830; 255 num = 12,451,650 < 15,991,825: white's 1 stays.
        [237, 157, 157, 255],
        // On the transparent index at alpha 100 (< 128): t = 3.
        [210, 10, 10, 100],
        // On the transparent index, alpha 0 and the matte colour: its own
        // render, unchanged: 3.
        [100, 100, 100, 0],
        // On black, c - s = (0, 0, 40) against (210, 10, 10): num = 400,
        // den = 44,300; 102,000 < 5,803,300: black's 0 stays.
        [0, 0, 40, 255],
    ];
    let out = reindex_codes(&palette, t, 7, 1, &before, &after, [210, 10, 10]).unwrap();
    assert_eq!(out, vec![0, 2, 2, 1, 3, 3, 0]);

    // A t backdrop painted to alpha >= 128 with an ink equal to t's matte:
    // E before the rule (no 0/0). E = nearest (100, 100, 100) over the
    // opaque entries: black and (200, 0, 0) tie at 30,000, the lower index
    // wins: 0.
    let out = reindex_codes(
        &palette,
        t,
        1,
        1,
        &[3],
        &[[100, 100, 100, 200]],
        [100, 100, 100],
    )
    .unwrap();
    assert_eq!(out, vec![0]);

    // The threshold itself, on black with ink (255, 0, 0) (E = 2): den =
    // 65,025; c = (131, 0, 0) is alpha = 131/255 exactly, E; (130, 0, 0) is
    // below it, black stays.
    let out = reindex_codes(
        &palette,
        None,
        2,
        1,
        &[0, 0],
        &[[131, 0, 0, 255], [130, 0, 0, 255]],
        [255, 0, 0],
    )
    .unwrap();
    assert_eq!(out, vec![2, 0]);

    // A plane that does not match the canvas refuses.
    assert!(reindex_codes(&palette, t, 2, 1, &[0], &after[..1], [0, 0, 0]).is_err());
}

/// The 12 × 8 scene: black, white and (190, 50, 50) bands, its last row
/// clear (the transparent index).
fn scene() -> *mut PrecisionDocument {
    let mut pixels = Vec::new();
    for y in 0..8 {
        for x in 0..12 {
            pixels.push(if y == 7 {
                [0, 0, 0, 0]
            } else if x < 4 {
                [0, 0, 0, 255]
            } else if x < 8 {
                [255, 255, 255, 255]
            } else {
                [190, 50, 50, 255]
            });
        }
    }
    let rgb = rgb_doc(12, 8, &pixels);
    let out = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    out
}

/// A graded disc of coverage around (6, 4), radius 4.5, ramping over 2 px.
fn disc() -> Vec<u16> {
    let mut out = Vec::new();
    for y in 0..8 {
        for x in 0..12 {
            let d = ((f64::from(x) - 6.0).powi(2) + (f64::from(y) - 4.0).powi(2)).sqrt();
            out.push((((4.5 - d) / 2.0).clamp(0.0, 1.0) * 65535.0).round() as u16);
        }
    }
    out
}

fn brush(selection: bool) -> Value {
    json!({"op":"paint_blend","layer":0,"color":[220.0/255.0, 30.0/255.0, 30.0/255.0, 1.0],
           "color_space":"document","opacity":1.0,"blend_mode":0,"selection":selection})
}

/// This file's own re-index (the rule's definition, independent of the
/// kernel's code): unchanged keeps; alpha < 128 takes t; a t backdrop at
/// alpha >= 128 takes E; else E when clamp(num / den, 0, 1) >= K / 255.
fn oracle(
    palette: &[[u8; 3]],
    t: Option<u8>,
    before: &[i32],
    after: &[[u8; 4]],
    ink: [u8; 3],
) -> Vec<i32> {
    let distance = |a: [u8; 3], b: [u8; 3]| -> i128 {
        (0..3)
            .map(|c| (i128::from(a[c]) - i128::from(b[c])).pow(2))
            .sum()
    };
    let e = (0..palette.len())
        .filter(|&i| Some(i as u8) != t)
        .min_by_key(|&i| (distance(palette[i], ink), i))
        .unwrap() as i32;
    before
        .iter()
        .zip(after)
        .map(|(&old, px)| {
            let s = palette[old as usize];
            let clear = Some(old as u8) == t;
            if *px == [s[0], s[1], s[2], if clear { 0 } else { 255 }] {
                return old;
            }
            if px[3] < 128 {
                if let Some(t) = t {
                    return i32::from(t);
                }
            }
            if clear {
                return e;
            }
            let num: i128 = (0..3)
                .map(|c| {
                    (i128::from(px[c]) - i128::from(s[c])) * (i128::from(ink[c]) - i128::from(s[c]))
                })
                .sum();
            let den = distance(ink, s);
            if den == 0 || 255 * num.clamp(0, den) >= K * den {
                e
            } else {
                old
            }
        })
        .collect()
}

fn to_rgb(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_indexed::dr_precision_doc_convert_indexed_to_rgb(doc, &mut err)
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

fn code8(v: u16) -> u8 {
    ((u32::from(v) + 128) / 257) as u8
}

#[test]
fn the_brush_is_the_rgb_stroke_reindexed() {
    let doc = scene();
    let (palette, t) = palette_of(doc);
    assert!(
        t.is_some(),
        "the scene's clear row is the transparent index"
    );
    let before = indices(doc);
    let coverage = disc();
    for selection in [false, true] {
        let mut input = coverage.clone();
        if selection {
            // Fully selected left of x = 9, a quarter selected beyond it.
            input.extend((0..96).map(|i| if i % 12 < 9 { 65535 } else { 16384 }));
        }
        let request = brush(selection);
        let painted = paint(doc, &request, &input)
            .unwrap()
            .expect("the stroke moves indices");
        let rgb = to_rgb(doc);
        let twin = paint(rgb, &request, &input)
            .unwrap()
            .expect("the RGB stroke");
        let after: Vec<[u8; 4]> = layer16(twin).iter().map(|p| p.map(code8)).collect();
        let expected = oracle(&palette, t, &before, &after, [220, 30, 30]);
        let got = indices(painted);
        assert_eq!(got, expected, "selection {selection}");
        assert_ne!(got, before, "selection {selection}");
        // Edges both ways: some changed pixel kept its index, some took E.
        let changed = after
            .iter()
            .zip(&before)
            .filter(|(px, &old)| {
                let s = palette[old as usize];
                let clear = Some(old as u8) == t;
                **px != [s[0], s[1], s[2], if clear { 0 } else { 255 }]
            })
            .count();
        let moved = got.iter().zip(&before).filter(|(a, b)| a != b).count();
        assert!(moved > 0 && moved < changed, "{moved} of {changed}");
        // The layer is the palette's render of the new plane.
        let layer = layer16(painted);
        for (pixel, &index) in layer.iter().zip(&got) {
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
        free(twin);
        free(rgb);
        free(painted);
    }
    free(doc);
}

#[test]
fn a_faint_stroke_that_maps_back_changes_nothing() {
    let doc = scene();
    let input = vec![1500u16; 96];
    let rgb = to_rgb(doc);
    let twin = paint(rgb, &brush(false), &input).unwrap();
    assert!(twin.is_some(), "the RGB stroke changes pixels");
    assert_eq!(paint(doc, &brush(false), &input), Ok(None));
    free(twin.unwrap());
    free(rgb);
    free(doc);
}

#[test]
fn other_modes_paint_and_other_edits_refuse_by_name() {
    let doc = scene();
    let input = disc();
    // Phase 3b: Multiply paints (`indexed_blend_modes.rs` checks the rule).
    let mut multiply = brush(false);
    multiply["blend_mode"] = json!(1);
    let painted = paint(doc, &multiply, &input)
        .unwrap()
        .expect("Multiply moves indices");
    assert!(edit_plan(doc, &multiply).is_ok());
    free(painted);
    // Phase 3c: the Gradient refuses for good; the scalar clone is offered
    // on RGB only.
    assert_eq!(
        paint(doc, &json!({"op":"gradient","layer":0}), &[]),
        Err("gradient is not available in Indexed Color.".into())
    );
    assert_eq!(
        paint(doc, &json!({"op":"clone","layer":0}), &[]),
        Err(
            "clone is unavailable in an Indexed Color document: Doctored offers it on RGB \
             documents only."
                .into()
        )
    );
    free(doc);
}

#[test]
fn the_plan_covers_the_reindex() {
    let doc = scene();
    let rgb = to_rgb(doc);
    for selection in [false, true] {
        let indexed = edit_plan(doc, &brush(selection)).unwrap();
        let twin = edit_plan(rgb, &brush(selection)).unwrap();
        assert_eq!(indexed["input_samples"], twin["input_samples"]);
        let scratch = indexed["scratch_bytes"].as_u64().unwrap();
        let floor = twin["scratch_bytes"].as_u64().unwrap() + 12 * 8 * 10;
        assert!(scratch >= floor, "{scratch} < {floor}");
    }
    free(rgb);
    free(doc);
}

#[test]
fn a_painted_document_round_trips() {
    let dir = TempDir::new().unwrap();
    let doc = scene();
    let painted = paint(doc, &brush(false), &disc()).unwrap().unwrap();
    let path = dir.path().join("painted.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(painted, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let back = open_rgb(&path).unwrap();
    assert_eq!(indices(back), indices(painted));
    assert_eq!(render16(back), render16(painted));
    assert_eq!(color_table(back), color_table(painted));
    free(back);
    free(painted);
    free(doc);
}
