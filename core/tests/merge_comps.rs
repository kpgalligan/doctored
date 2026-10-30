//! Merge Down, Merge Layers and Merge Visible keep the lowest merged
//! entry's id, so a comp saved before the merge still names the merged
//! layer. The merge moves that layer's buffer origin without moving its
//! content, so the comp's recorded offset follows it (the storage-growth
//! shift), and the merged layer's appearance is baked into its pixels, so
//! the comp's recorded appearance for it becomes the merged layer's own
//! (opacity 1, Normal, no style). Black-box through `dr_doc_merging_down` /
//! `dr_doc_merge_layers` / `dr_doc_merge_visible` /
//! `dr_doc_edit_layer_comps` at 8 bits and `dr_precision_doc_edit` at 16.
//!
//! The oracles are hand-computed origins. The canvas is 60×60 with an
//! opaque background; L is 30×24 at (12, 14) and U is 10×10 at (2, 3). A
//! Merge Down or Merge Layers covers the union (2, 3)…(42, 38) at both
//! depths, so the merged origin is (2, 3); a Merge Visible gives a
//! canvas-sized layer at (0, 0) at both depths.

use crate::common;

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_doc::dr_doc_merging_down;
use doctored_core::ffi_group::{dr_doc_merge_layers, dr_doc_merge_visible};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use common::selected_pixels::edit16;
use common::{flat_pixels, opaque_pattern, styled, take_doc, BLEND_MULTIPLY};

const CANVAS: u32 = 60;
const L_AT: (i32, i32) = (12, 14);
const U_AT: (i32, i32) = (2, 3);
const SHADOW: &str =
    "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":2,\"size\":0,\"spread\":0}]}";

#[derive(Clone, Copy, Debug)]
enum Merge {
    Down,
    Layers,
    Visible,
}
const MERGES: [Merge; 3] = [Merge::Down, Merge::Layers, Merge::Visible];

fn save(position: bool) -> Value {
    json!({"op":"layer_comp","action":"save","name":"Here","position":position})
}

fn apply() -> Value {
    json!({"op":"layer_comp","action":"apply","comp":0})
}

// ------------------------------------------------------------ 8 bits --

fn l_pixels() -> RgbaImage {
    RgbaImage::from_fn(30, 24, |x, y| {
        Rgba([(x * 8) as u8, (y * 10) as u8, 120, 255])
    })
}

fn u_pixels() -> RgbaImage {
    RgbaImage::from_fn(10, 10, |x, y| {
        Rgba([200, (x * 20) as u8, (y * 20) as u8, 255])
    })
}

/// Background (0), L (1) at (12, 14), U (2) at (2, 3).
fn byte_fixture() -> DrDocument {
    let mut doc = DrDocument::from_pixels(opaque_pattern(CANVAS, CANVAS))
        .adding_image_layer(0, l_pixels(), "L")
        .expect("L")
        .adding_image_layer(1, u_pixels(), "U")
        .expect("U");
    doc.layers[1].offset = L_AT;
    doc.layers[2].offset = U_AT;
    doc
}

/// A byte comp edit: the document it made, or `None` for the exact no-op.
fn comp8_try(doc: &DrDocument, request: Value) -> Option<DrDocument> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, request.as_ptr(), &mut err) };
    let (out, err) = take_doc(out, err);
    assert!(err.is_none(), "{err:?}");
    out
}

fn comp8(doc: &DrDocument, request: Value) -> DrDocument {
    comp8_try(doc, request).expect("the comp edit changed the document")
}

fn merge8(doc: &DrDocument, merge: Merge) -> DrDocument {
    let members = [1usize, 2];
    let out = unsafe {
        match merge {
            Merge::Down => dr_doc_merging_down(doc, 2),
            Merge::Layers => dr_doc_merge_layers(doc, members.as_ptr(), members.len()),
            Merge::Visible => dr_doc_merge_visible(doc),
        }
    };
    assert!(!out.is_null(), "{merge:?} merged");
    *unsafe { Box::from_raw(out) }
}

/// The fixture as each merge row starts it: Merge Visible hides the
/// background first, so L is the bottom-most contributing leaf it keeps.
fn byte_start(merge: Merge, doc: DrDocument) -> DrDocument {
    match merge {
        Merge::Visible => doc.with_layer_visible(0, false).expect("hidden"),
        _ => doc,
    }
}

fn flat8(doc: &DrDocument) -> Vec<u8> {
    flat_pixels(doc)
}

#[test]
fn byte_merges_keep_a_position_comp() {
    for (merge, want) in [
        (Merge::Down, (2, 3)),
        (Merge::Layers, (2, 3)),
        (Merge::Visible, (0, 0)),
    ] {
        let doc = comp8(&byte_start(merge, byte_fixture()), save(true));
        let merged = merge8(&doc, merge);
        assert_eq!(
            merged.layers[1].offset, want,
            "{merge:?}: the merged origin"
        );
        let moved = merged.with_layer_offset(1, want.0 + 9, want.1 + 4).unwrap();
        let applied = comp8(&moved, apply());
        assert_eq!(
            applied.layers[1].offset, want,
            "{merge:?}: content lands where the merge left it"
        );
        assert_eq!(flat8(&applied), flat8(&merged), "{merge:?}");
    }
}

#[test]
fn byte_merge_down_after_a_move_keeps_the_recorded_place() {
    let doc = comp8(&byte_fixture(), save(true));
    let moved = doc.with_layer_offset(1, L_AT.0 + 8, L_AT.1 + 6).unwrap();
    let merged = merge8(&moved, Merge::Down);
    assert_eq!(merged.layers[1].offset, (2, 3), "the merged origin");
    let applied = comp8(&merged, apply());
    // L recorded at (12, 14), sat at (20, 20) when merged into a buffer at
    // (2, 3): the comp puts that buffer at (2 − 20 + 12, 3 − 20 + 14).
    assert_eq!(applied.layers[1].offset, (-6, -3));
}

#[test]
fn byte_merges_reset_the_kept_appearance() {
    for merge in MERGES {
        let mut doc = byte_fixture();
        doc.layers[1].opacity = 0.5;
        doc.layers[1].blend = BlendMode::from_c(BLEND_MULTIPLY).unwrap();
        let doc = styled(&doc, 1, SHADOW);
        let doc = comp8(
            &byte_start(merge, doc),
            json!({"op":"layer_comp","action":"save","name":"Look"}),
        );
        let merged = merge8(&doc, merge);
        let applied = comp8_try(&merged, apply()).unwrap_or_else(|| merged.clone());
        let layer = &applied.layers[1];
        assert_eq!(layer.opacity, 1.0, "{merge:?}: opacity stays merged");
        assert_eq!(
            layer.blend,
            BlendMode::Normal,
            "{merge:?}: blend stays merged"
        );
        assert!(layer.style.is_none(), "{merge:?}: no style comes back");
        assert_eq!(flat8(&applied), flat8(&merged), "{merge:?}");
    }
}

#[test]
fn byte_merge_leaves_a_comp_without_position_alone() {
    for merge in MERGES {
        let doc = comp8(&byte_start(merge, byte_fixture()), save(false));
        let merged = merge8(&doc, merge);
        assert!(
            comp8_try(&merged, apply()).is_none(),
            "{merge:?}: no position was recorded"
        );
    }
}

// ----------------------------------------------------------- 16 bits --

fn image16(size: (u32, u32), f: impl Fn(u32, u32) -> [u16; 4]) -> PrecisionImage {
    let samples = (0..size.1)
        .flat_map(|y| (0..size.0).map(move |x| (x, y)))
        .map(|(x, y)| f(x, y))
        .collect();
    PrecisionImage::from_rgba16(size.0, size.1, samples, IccProfile::srgb(), None).unwrap()
}

fn raster16(
    name: &str,
    image: PrecisionImage,
    at: (i32, i32),
    opacity: f32,
) -> PrecisionDocumentLayer {
    PrecisionDocumentLayer::raster(name, image, at, opacity, true).unwrap()
}

/// Background (0), L (1) at (12, 14) at `opacity`, U (2) at (2, 3).
fn typed_fixture(opacity: f32) -> PrecisionDocument {
    let bg = image16((CANVAS, CANVAS), |x, y| {
        [(x * 1000) as u16, (y * 1000) as u16, 20000, 65535]
    });
    let l = image16((30, 24), |x, y| {
        [(x * 2000 + 7) as u16, (y * 2500 + 3) as u16, 30000, 65535]
    });
    let u = image16((10, 10), |x, y| {
        [50000, (x * 5000) as u16, (y * 6000) as u16, 65535]
    });
    PrecisionDocument::from_parts(
        CANVAS,
        CANVAS,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster16("Background", bg, (0, 0), 1.0),
            raster16("L", l, L_AT, opacity),
            raster16("U", u, U_AT, 1.0),
        ],
    )
    .unwrap()
}

fn typed_try(doc: &PrecisionDocument, request: Value) -> Option<PrecisionDocument> {
    edit16(doc, &request.to_string(), &[], None).unwrap_or_else(|e| panic!("{request}: {e}"))
}

fn typed_edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    typed_try(doc, request.clone()).unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn merge16(doc: &PrecisionDocument, merge: Merge) -> PrecisionDocument {
    typed_edit(
        doc,
        match merge {
            Merge::Down => json!({"op":"merge_down","layer":2}),
            Merge::Layers => json!({"op":"merge_layers","layers":[1, 2]}),
            Merge::Visible => json!({"op":"merge_visible"}),
        },
    )
}

fn typed_start(merge: Merge, doc: PrecisionDocument) -> PrecisionDocument {
    match merge {
        Merge::Visible => typed_edit(
            &doc,
            json!({"op":"layer_visible","layer":0,"visible":false}),
        ),
        _ => doc,
    }
}

fn typed_offset(doc: &PrecisionDocument) -> (i32, i32) {
    doc.layer(1).unwrap().offset()
}

fn typed_move(doc: &PrecisionDocument, dx: i32, dy: i32) -> PrecisionDocument {
    typed_edit(
        doc,
        json!({"op":"affine_set","layers":[1],"matrix":[1,0,0,1,dx,dy],"filter":"nearest"}),
    )
}

fn flat16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

#[test]
fn typed_merges_keep_a_position_comp() {
    for (merge, want) in [
        (Merge::Down, (2, 3)),
        (Merge::Layers, (2, 3)),
        (Merge::Visible, (0, 0)),
    ] {
        let doc = typed_edit(&typed_start(merge, typed_fixture(1.0)), save(true));
        let merged = merge16(&doc, merge);
        assert_eq!(typed_offset(&merged), want, "{merge:?}: the merged origin");
        let moved = typed_move(&merged, 9, 4);
        let applied = typed_edit(&moved, apply());
        assert_eq!(
            typed_offset(&applied),
            want,
            "{merge:?}: content lands where the merge left it"
        );
        assert_eq!(flat16(&applied), flat16(&merged), "{merge:?}");
    }
}

#[test]
fn typed_merge_down_after_a_move_keeps_the_recorded_place() {
    let doc = typed_edit(&typed_fixture(1.0), save(true));
    let moved = typed_move(&doc, 8, 6);
    let merged = merge16(&moved, Merge::Down);
    assert_eq!(typed_offset(&merged), (2, 3), "the merged origin");
    let applied = typed_edit(&merged, apply());
    // L recorded at (12, 14), sat at (20, 20) when merged into a buffer at
    // (2, 3): the comp puts that buffer at (2 − 20 + 12, 3 − 20 + 14).
    assert_eq!(typed_offset(&applied), (-6, -3));
}

#[test]
fn typed_merges_reset_the_kept_appearance() {
    for merge in MERGES {
        let doc = typed_edit(
            &typed_fixture(0.5),
            json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":BLEND_MULTIPLY}),
        );
        let doc = typed_edit(
            &typed_start(merge, doc),
            json!({"op":"layer_comp","action":"save","name":"Look"}),
        );
        let merged = merge16(&doc, merge);
        let applied = typed_try(&merged, apply()).unwrap_or_else(|| merged.clone());
        let layer = applied.layer(1).unwrap();
        assert_eq!(layer.opacity(), 1.0, "{merge:?}: opacity stays merged");
        assert_eq!(
            layer.blend_mode(),
            BlendMode::Normal,
            "{merge:?}: blend stays merged"
        );
        assert!(
            layer.reported_style().is_none(),
            "{merge:?}: no style comes back"
        );
        assert_eq!(flat16(&applied), flat16(&merged), "{merge:?}");
    }
}

#[test]
fn typed_merge_leaves_a_comp_without_position_alone() {
    for merge in MERGES {
        let doc = typed_edit(&typed_start(merge, typed_fixture(1.0)), save(false));
        let merged = merge16(&doc, merge);
        assert!(
            typed_try(&merged, apply()).is_none(),
            "{merge:?}: no position was recorded"
        );
    }
}
