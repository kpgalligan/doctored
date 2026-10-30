//! A per-layer STORAGE GROWTH that moves a raster's buffer origin —
//! native paint past the layer's left/top edge, a retouch with Expand
//! destination, a selection moved past the layer's left/top edge — shifts
//! that layer's recorded comp offsets by the same delta, at 16 bits as at 8,
//! so Apply after the growth puts the content where it was. Black-box
//! through `dr_precision_doc_edit` / `dr_doc_selected_pixels_edit` /
//! `dr_doc_edit_layer_comps`.
//!
//! The oracles are hand-computed origins: the layer is a 30×24 raster at
//! (12, 14) on a 60×60 canvas, so ink at canvas (2, 3) grows its origin to
//! (2, 3), a retouch expansion to the canvas gives (0, 0), and the layer's
//! x 12..22, y 14..24 moved by (−10, −11) lands at x 2..12, y 3..13, giving
//! (2, 3).

use crate::common;

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use common::selected_pixels::{edit16, edit8};
use common::{flat_pixels, opaque_pattern, take_doc};

const CANVAS: u32 = 60;
const ORIGIN: (i32, i32) = (12, 14);
const SIZE: (u32, u32) = (30, 24);

// --------------------------------------------------------------- fixtures --

/// The opaque 30×24 layer's straight samples at 16 bits.
fn layer_samples16() -> Vec<[u16; 4]> {
    (0..SIZE.0 * SIZE.1)
        .map(|i| {
            let (x, y) = ((i % SIZE.0) as u16, (i / SIZE.0) as u16);
            [x * 2000 + 7, y * 2500 + 3, 30000 + x * y, 65535]
        })
        .collect()
}

fn raster16(
    name: &str,
    size: (u32, u32),
    pixels: Vec<[u16; 4]>,
    offset: (i32, i32),
) -> PrecisionDocumentLayer {
    let image =
        PrecisionImage::from_rgba16(size.0, size.1, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, offset, 1.0, true).unwrap()
}

/// The typed fixture: record 0 is the 30×24 raster at (12, 14); with
/// `source`, record 1 is an opaque full-canvas raster to clone from.
fn typed(source: bool) -> PrecisionDocument {
    let mut layers = vec![raster16("Layer", SIZE, layer_samples16(), ORIGIN)];
    if source {
        let n = (CANVAS * CANVAS) as usize;
        let pixels = (0..n)
            .map(|i| {
                [
                    (i * 13 % 65536) as u16,
                    40000,
                    (i * 7 % 65536) as u16,
                    65535,
                ]
            })
            .collect();
        layers.push(raster16("Source", (CANVAS, CANVAS), pixels, (0, 0)));
    }
    PrecisionDocument::from_parts(
        CANVAS,
        CANVAS,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// Canvas-sized u16 coverage: 65535 where `f` holds, 0 elsewhere.
fn coverage(f: impl Fn(u32, u32) -> bool) -> Vec<u16> {
    (0..CANVAS * CANVAS)
        .map(|i| if f(i % CANVAS, i / CANVAS) { 65535 } else { 0 })
        .collect()
}

/// Coverage at canvas (x, y) only.
fn dot(x: u32, y: u32) -> Vec<u16> {
    coverage(|cx, cy| (cx, cy) == (x, y))
}

/// The layer's x 12..22, y 14..24, the selection the moves carry.
fn selection() -> Vec<u16> {
    coverage(|x, y| (12..22).contains(&x) && (14..24).contains(&y))
}

fn paint_blend() -> Value {
    json!({"op":"paint_blend","layer":0,"color":[0.2,0.6,0.9,1.0],
        "color_space":"document","opacity":1.0,"blend_mode":0})
}

fn move_selected(layer: usize) -> Value {
    json!({"op":"move_selected","layer":layer,"dx":-10,"dy":-11})
}

fn save(position: bool) -> Value {
    json!({"op":"layer_comp","action":"save","name":"Here","position":position})
}

fn apply() -> Value {
    json!({"op":"layer_comp","action":"apply","comp":0})
}

/// A typed edit that must change the document.
fn typed_edit(doc: &PrecisionDocument, request: Value, input: &[u16]) -> PrecisionDocument {
    edit16(doc, &request.to_string(), input, None)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn typed_offset(doc: &PrecisionDocument) -> (i32, i32) {
    doc.layer(0).unwrap().offset()
}

fn typed_pixels(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.layer_source(0).unwrap().to_rgba16().unwrap()
}

/// A byte comp edit that must change the document.
fn comp8(doc: &DrDocument, request: Value) -> DrDocument {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, request.as_ptr(), &mut err) };
    let (out, err) = take_doc(out, err);
    assert!(err.is_none(), "{err:?}");
    out.expect("the comp edit changed the document")
}

// ------------------------------------------------------------ survival --

/// Saves a comp with Position on, grows record 0 through `grow` (which must
/// move its origin to `want_origin`), moves it by (9, 4), applies the comp
/// and asserts the grown content is back where the growth left it.
fn typed_comp_survives(
    doc: PrecisionDocument,
    grow: impl Fn(&PrecisionDocument) -> PrecisionDocument,
    want_origin: (i32, i32),
    what: &str,
) {
    let saved = typed_edit(&doc, save(true), &[]);
    let grown = grow(&saved);
    assert_eq!(
        typed_offset(&grown),
        want_origin,
        "{what}: the grown origin"
    );
    let moved = typed_edit(
        &grown,
        json!({"op":"affine_set","layers":[0],"matrix":[1,0,0,1,9,4],"filter":"nearest"}),
        &[],
    );
    let applied = typed_edit(&moved, apply(), &[]);
    assert_eq!(
        typed_offset(&applied),
        want_origin,
        "{what}: content lands where it was"
    );
    assert_eq!(typed_pixels(&applied), typed_pixels(&grown), "{what}");
}

#[test]
fn typed_paint_past_the_left_top_edge_keeps_the_comp() {
    typed_comp_survives(
        typed(false),
        |d| typed_edit(d, paint_blend(), &dot(2, 3)),
        (2, 3),
        "paint_blend",
    );
}

#[test]
fn typed_retouch_expansion_keeps_the_comp() {
    let mut request = json!({"op":"retouch_clone","layer":0,"source_layer":1,"sample":"current"});
    request["expand_destination"] = json!(true);
    typed_comp_survives(
        typed(true),
        |d| typed_edit(d, request.clone(), &dot(0, 0)),
        (0, 0),
        "retouch_clone expand_destination",
    );
}

#[test]
fn typed_selection_moved_past_the_left_top_edge_keeps_the_comp() {
    typed_comp_survives(
        typed(false),
        |d| typed_edit(d, move_selected(0), &selection()),
        (2, 3),
        "move_selected",
    );
}

#[test]
fn byte_selection_moved_past_the_left_top_edge_keeps_the_comp() {
    let layer = RgbaImage::from_fn(SIZE.0, SIZE.1, |x, y| {
        Rgba([(x * 8) as u8, (y * 10) as u8, 120, 255])
    });
    let mut doc = DrDocument::from_pixels(opaque_pattern(CANVAS, CANVAS))
        .adding_image_layer(0, layer, "Layer")
        .expect("a layer");
    doc.layers[1].offset = ORIGIN;
    let saved = comp8(&doc, save(true));
    let grown = edit8(&saved, &move_selected(1).to_string(), &selection(), None)
        .expect("the move runs")
        .expect("the move changes something");
    assert_eq!(grown.layers[1].offset, (2, 3), "the grown origin");
    let moved = grown.with_layer_offset(1, 2 + 9, 3 + 4).unwrap();
    let applied = comp8(&moved, apply());
    assert_eq!(
        applied.layers[1].offset,
        (2, 3),
        "content lands where it was"
    );
    assert_eq!(flat_pixels(&applied), flat_pixels(&grown));
}

// ------------------------------------------------------------ negatives --

#[test]
fn a_growth_that_keeps_the_origin_leaves_comps_identical() {
    let saved = typed_edit(&typed(false), save(true), &[]);
    // (50, 50) is right of and below the layer (x 12..42, y 14..38): the
    // extent grows to x 12..51, y 14..51 and the origin stays.
    let grown = typed_edit(&saved, paint_blend(), &dot(50, 50));
    assert_eq!(typed_offset(&grown), ORIGIN);
    assert_eq!(
        grown.layer_source(0).unwrap().dimensions(),
        (51 - 12, 51 - 14)
    );
    assert_eq!(grown.layer_comps(), saved.layer_comps());
}

#[test]
fn a_comp_without_position_is_left_alone_by_a_left_growth() {
    let saved = typed_edit(&typed(false), save(false), &[]);
    let grown = typed_edit(&saved, paint_blend(), &dot(2, 3));
    assert_eq!(typed_offset(&grown), (2, 3), "the growth moved the origin");
    let applied = edit16(&grown, &apply().to_string(), &[], None);
    assert!(
        matches!(applied, Ok(None)),
        "no position was recorded: {:?}",
        applied.map(|d| d.is_some())
    );
}
