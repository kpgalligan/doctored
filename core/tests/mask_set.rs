//! The byte set-mask primitive: `dr_doc_set_mask` places caller
//! coverage at an arbitrary canvas rectangle with a linked flag and outside
//! coverage, never cropped to the layer's pixel rect.
//!
//! Oracles are independent: properties are read back through
//! `dr_doc_layer_mask_properties`, the effective mask through the canvas-sized
//! `DrDocument::layer_effective_mask_image`, and projections are compared with the
//! test's own f64 lerp of two opaque colours (±1 code for the byte rounding).
use std::ffi::CString;
use std::ptr;

use crate::common;
use common::*;
use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::*;
use doctored_core::ffi_mask::*;
use serde_json::{json, Value};
use tempfile::TempDir;

const CANVAS: (u32, u32) = (16, 12);
const LAYER: (u32, u32) = (6, 4);
const OFFSET: (i32, i32) = (3, 2);
const LOCK_ALL: u32 = 7;

fn fixture(dir: &TempDir, tag: &str) -> *mut DrDocument {
    ffi_mask_fixture(dir, tag, CANVAS, LAYER, OFFSET)
}

#[allow(clippy::too_many_arguments)]
fn set_mask(
    doc: *const DrDocument,
    idx: usize,
    coverage: &[u8],
    w: u32,
    h: u32,
    at: (i32, i32),
    linked: bool,
    outside: u8,
) -> *mut DrDocument {
    unsafe {
        dr_doc_set_mask(
            doc,
            idx,
            coverage.as_ptr(),
            w,
            h,
            at.0,
            at.1,
            linked,
            outside,
        )
    }
}

fn properties(doc: *const DrDocument, idx: usize) -> Value {
    serde_json::from_str(&take_err_string(unsafe {
        dr_doc_layer_mask_properties(doc, idx)
    }))
    .unwrap()
}

fn effective(doc: *const DrDocument, idx: usize) -> Vec<u8> {
    unsafe { &*doc }
        .layer_effective_mask_image(idx)
        .expect("an effective mask")
        .pixels()
        .map(|p| p[0])
        .collect()
}

/// Blue at `m`/255 over red, as the test computes it.
fn lerp(m: f64) -> [f64; 4] {
    let t = m / 255.0;
    [255.0 * (1.0 - t), 0.0, 255.0 * t, 255.0]
}

fn assert_projection(doc: *const DrDocument, cover: impl Fn(u32, u32) -> f64, what: &str) {
    let flat = flat_pixels(doc);
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let got = pixel(&flat, CANVAS.0, x, y);
            let want = lerp(cover(x, y));
            for c in 0..4 {
                assert!(
                    (f64::from(got[c]) - want[c]).abs() <= 1.0,
                    "{what}: ({x},{y}) channel {c}: {got:?} vs {want:?}"
                );
            }
        }
    }
}

fn in_layer(x: u32, y: u32, offset: (i32, i32)) -> bool {
    let (x, y) = (x as i32, y as i32);
    x >= offset.0 && y >= offset.1 && x < offset.0 + LAYER.0 as i32 && y < offset.1 + LAYER.1 as i32
}

#[test]
fn a_canvas_sized_mask_is_stored_verbatim_and_unlinked() {
    let dir = tempfile::tempdir().unwrap();
    let doc = fixture(&dir, "canvas");
    let (w, h) = CANVAS;
    let coverage = selection(w, h, |x, y| ((x * 17 + y * 29) % 256) as u8);
    let before = layer_pixels(doc, 1);
    let out = set_mask(doc, 1, &coverage, w, h, (0, 0), false, 0);
    assert!(!out.is_null());
    assert_eq!(
        properties(out, 1),
        json!({"linked":false,"density":1.0,"feather":0.0,"outside_coverage":0,
            "offset_x":0,"offset_y":0,"width":w,"height":h})
    );
    assert!(unsafe { dr_doc_layer_has_mask(out, 1) && dr_doc_layer_mask_enabled(out, 1) });
    assert_eq!(effective(out, 1), coverage);
    assert_eq!(layer_pixels(out, 1), before, "pixels are untouched");
    assert_eq!(layer_offset(out, 1), OFFSET);
    assert_projection(
        out,
        |x, y| {
            if in_layer(x, y, OFFSET) {
                f64::from(coverage[(y * w + x) as usize])
            } else {
                0.0
            }
        },
        "canvas mask",
    );
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}

#[test]
fn an_arbitrary_rect_reads_back_with_outside_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let doc = fixture(&dir, "rect");
    // 5×3 at (-2, 4): partly off the canvas, straddling the layer's edge.
    let coverage: Vec<u8> = (0..15).map(|i| (i * 18) as u8).collect();
    let rect = (-2i32, 4i32, 5u32, 3u32);
    let out = set_mask(
        doc,
        1,
        &coverage,
        rect.2,
        rect.3,
        (rect.0, rect.1),
        true,
        255,
    );
    assert!(!out.is_null());
    let p = properties(out, 1);
    assert_eq!(p["offset_x"], -2);
    assert_eq!(p["offset_y"], 4);
    assert_eq!(p["width"], 5);
    assert_eq!(p["height"], 3);
    assert_eq!(p["linked"], true);
    assert_eq!(p["outside_coverage"], 255);
    let cover = |x: u32, y: u32| -> u8 {
        let (mx, my) = (x as i32 - rect.0, y as i32 - rect.1);
        if mx >= 0 && my >= 0 && mx < rect.2 as i32 && my < rect.3 as i32 {
            coverage[(my * rect.2 as i32 + mx) as usize]
        } else {
            255
        }
    };
    let expected = selection(CANVAS.0, CANVAS.1, cover);
    assert_eq!(effective(out, 1), expected);
    assert_projection(
        out,
        |x, y| {
            if in_layer(x, y, OFFSET) {
                f64::from(cover(x, y))
            } else {
                0.0
            }
        },
        "outside 255",
    );
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}

#[test]
fn outside_255_reveals_the_layer_beyond_a_hiding_rect() {
    let dir = tempfile::tempdir().unwrap();
    let doc = fixture(&dir, "hide");
    // A 2×2 hole at (4, 3), everything else revealed.
    let out = set_mask(doc, 1, &[0; 4], 2, 2, (4, 3), false, 255);
    assert!(!out.is_null());
    let flat = flat_pixels(out);
    assert_eq!(pixel(&flat, CANVAS.0, 4, 3), RED);
    assert_eq!(pixel(&flat, CANVAS.0, 5, 4), RED);
    assert_eq!(pixel(&flat, CANVAS.0, 8, 4), BLUE);
    assert_eq!(pixel(&flat, CANVAS.0, 3, 2), BLUE);
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}

#[test]
fn moved_content_stays_revealed_inside_an_unlinked_canvas_mask() {
    let dir = tempfile::tempdir().unwrap();
    let doc = fixture(&dir, "move");
    let (w, h) = CANVAS;
    // The selection x ∈ [2, 12), y ∈ [1, 10): wider than the 6×4 layer.
    let inside = |x: u32, y: u32| (2..12).contains(&x) && (1..10).contains(&y);
    let sel = selection(w, h, |x, y| if inside(x, y) { 255 } else { 0 });
    let masked = set_mask(doc, 1, &sel, w, h, (0, 0), false, 0);
    assert!(!masked.is_null());
    let moved = unsafe { dr_doc_move_layers(masked, [1usize].as_ptr(), 1, 4, 0) };
    assert!(!moved.is_null());
    let moved_offset = (OFFSET.0 + 4, OFFSET.1);
    assert_eq!(layer_offset(moved, 1), moved_offset);
    let p = properties(moved, 1);
    assert_eq!(
        (p["offset_x"].clone(), p["offset_y"].clone()),
        (json!(0), json!(0))
    );
    // (10, 3) lies past the layer's ORIGINAL rect (3..9) but inside the
    // selection: a mask cropped to the layer would hide it.
    let flat = flat_pixels(moved);
    assert_eq!(pixel(&flat, w, 10, 3), BLUE);
    assert_eq!(pixel(&flat, w, 12, 3), RED, "past the selection");
    assert_projection(
        moved,
        |x, y| {
            if in_layer(x, y, moved_offset) && inside(x, y) {
                255.0
            } else {
                0.0
            }
        },
        "moved",
    );
    unsafe {
        dr_doc_free(moved);
        dr_doc_free(masked);
        dr_doc_free(doc);
    }
}

#[test]
fn refusals_answer_null() {
    let dir = tempfile::tempdir().unwrap();
    let doc = fixture(&dir, "refuse");
    let (w, h) = CANVAS;
    let sel = selection(w, h, |x, _| if x < 8 { 255 } else { 0 });

    // NULL coverage, zero and oversized dimensions, out-of-range index,
    // an outside coverage that is not a PSD endpoint.
    assert!(unsafe { dr_doc_set_mask(doc, 1, ptr::null(), w, h, 0, 0, false, 0) }.is_null());
    assert!(set_mask(doc, 1, &sel, 0, h, (0, 0), false, 0).is_null());
    assert!(set_mask(doc, 1, &sel, w, 0, (0, 0), false, 0).is_null());
    assert!(set_mask(doc, 1, &sel, 100_000, 100_000, (0, 0), false, 0).is_null());
    assert!(set_mask(doc, 5, &sel, w, h, (0, 0), false, 0).is_null());
    assert!(set_mask(doc, 1, &sel, w, h, (0, 0), false, 7).is_null());

    // An identical re-set is the purity rule's no-op; any difference is not.
    let once = set_mask(doc, 1, &sel, w, h, (0, 0), false, 0);
    assert!(!once.is_null());
    assert!(set_mask(once, 1, &sel, w, h, (0, 0), false, 0).is_null());
    let relinked = set_mask(once, 1, &sel, w, h, (0, 0), true, 0);
    assert!(!relinked.is_null());
    let moved = set_mask(once, 1, &sel, w, h, (1, 0), false, 0);
    assert!(!moved.is_null());

    // Lock All refuses; Lock Pixels does not (a mask edit).
    let locked = unsafe { dr_doc_with_layer_locks(doc, 1, LOCK_ALL) };
    assert!(!locked.is_null());
    assert!(set_mask(locked, 1, &sel, w, h, (0, 0), false, 0).is_null());
    let pixels_locked = unsafe { dr_doc_with_layer_locks(doc, 1, 2) };
    let edited = set_mask(pixels_locked, 1, &sel, w, h, (0, 0), false, 0);
    assert!(!edited.is_null());

    // A group keeps add_mask.
    let name = CString::new("G").unwrap();
    let mut group = 0usize;
    let grouped = unsafe {
        dr_doc_group_layers(
            doc,
            [1usize].as_ptr(),
            1,
            name.as_ptr(),
            &mut group,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };
    assert!(!grouped.is_null());
    assert!(unsafe { dr_doc_layer_is_group(grouped, group) });
    assert!(set_mask(grouped, group, &sel, w, h, (0, 0), false, 0).is_null());

    unsafe {
        for d in [
            once,
            relinked,
            moved,
            locked,
            pixels_locked,
            edited,
            grouped,
            doc,
        ] {
            dr_doc_free(d);
        }
    }
}
