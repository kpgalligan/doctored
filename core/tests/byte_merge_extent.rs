//! An 8-bit merge keeps a GROUP member's off-canvas content. A group
//! operand of `dr_doc_merging_down` / `dr_doc_merge_layers` renders over its
//! own extent, not the canvas, so cropping, merging and growing the canvas
//! back reveals exactly the picture the crop hid — the rule a raster
//! operand's retained pixels already follow. Black-box through the FFI.

use std::ffi::CString;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::*;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

/// `[Base (8×8 opaque, depth 0), G { Top }]`: Top is 8×8 half-transparent in
/// a different colour at (0,0) inside G, so it covers pixels outside every
/// crop window below. Returns the document and G's index.
fn fixture(dir: &TempDir, blend: i32) -> (*mut DrDocument, usize) {
    let base = RgbaImage::from_fn(8, 8, |x, y| {
        Rgba([(x * 30) as u8, (y * 30) as u8, 120, 255])
    });
    let mut doc = doc_from(dir, "base.png", &base);
    let top = RgbaImage::from_fn(8, 8, |x, y| {
        Rgba([200, (x * 20 + 40) as u8, (y * 25) as u8, 128])
    });
    doc = add_layer(dir, "top.png", doc, 0, &top, "Top");
    let name = CString::new("G").unwrap();
    let cap = unsafe { dr_doc_layer_count(doc) } + 1;
    let mut group = usize::MAX;
    let mut cleared = vec![0usize; cap];
    let mut cleared_len = 0usize;
    let mut reordered = vec![0usize; cap];
    let mut reordered_len = 0usize;
    let idx = [1usize];
    let out = unsafe {
        dr_doc_group_layers(
            doc,
            idx.as_ptr(),
            idx.len(),
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            cap,
        )
    };
    assert!(!out.is_null(), "group_layers refused");
    unsafe { dr_doc_free(doc) };
    let doc = if blend == BLEND_PASS_THROUGH {
        out
    } else {
        apply(out, |d| unsafe {
            dr_doc_with_layer_blend_mode(d, group, blend)
        })
    };
    (doc, group)
}

/// Crop to `(x, y, 4, 4)`, merge through `merge`, grow the canvas back to 8×8
/// with the old corner at `(x, y)`: the flatten must equal the pre-crop one
/// on every pixel, and the merged layer must be 8×8 at (0,0).
fn round_trip(
    blend: i32,
    at: (u32, u32),
    merge: impl Fn(*const DrDocument, usize) -> *mut DrDocument,
) {
    let dir = TempDir::new().unwrap();
    let (doc, group) = fixture(&dir, blend);
    let before = flat_pixels(doc);
    let doc = apply(doc, |d| unsafe { dr_doc_crop(d, at.0, at.1, 4, 4) });
    let doc = apply(doc, |d| merge(d, group));
    assert_eq!(unsafe { dr_doc_layer_count(doc) }, 1, "one merged layer");
    let doc = apply(doc, |d| unsafe {
        dr_doc_canvas_resize(d, 8, 8, at.0 as i32, at.1 as i32)
    });
    assert_eq!(
        layer_dims(doc, 0),
        (8, 8),
        "the merged layer keeps G's off-canvas rows"
    );
    assert_eq!(layer_offset(doc, 0), (0, 0));
    let after = flat_pixels(doc);
    assert_eq!(before.len(), after.len());
    assert_eq!(
        before, after,
        "blend {blend} crop at {at:?}: merging must keep the group's off-canvas content"
    );
    unsafe { dr_doc_free(doc) };
}

fn merge_down(d: *const DrDocument, group: usize) -> *mut DrDocument {
    unsafe { dr_doc_merging_down(d, group) }
}

fn merge_pair(d: *const DrDocument, group: usize) -> *mut DrDocument {
    let idx = [0usize, group];
    unsafe { dr_doc_merge_layers(d, idx.as_ptr(), idx.len()) }
}

#[test]
fn merging_a_group_down_keeps_its_off_canvas_content() {
    for blend in [BLEND_PASS_THROUGH, BLEND_NORMAL] {
        round_trip(blend, (0, 0), merge_down);
    }
}

#[test]
fn merging_a_group_down_keeps_content_at_a_negative_offset() {
    for blend in [BLEND_PASS_THROUGH, BLEND_NORMAL] {
        round_trip(blend, (2, 2), merge_down);
    }
}

#[test]
fn merge_layers_keeps_a_group_members_off_canvas_content() {
    for blend in [BLEND_PASS_THROUGH, BLEND_NORMAL] {
        round_trip(blend, (0, 0), merge_pair);
        round_trip(blend, (2, 2), merge_pair);
    }
}
