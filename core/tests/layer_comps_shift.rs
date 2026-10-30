//! The host's `shift` comp request. After a whole-canvas geometry op the
//! host re-renders or refits text and shape layers to an offset
//! the core's geometry rule did not give them, and sends one
//! `{"op":"layer_comp","action":"shift","layer":N,"dx":DX,"dy":DY}` per such
//! layer so its recorded offsets follow. Black-box through
//! `dr_doc_edit_layer_comps` at 8 bits and `PrecisionDocument::edit_layer_comps`
//! over the one shared parser at 16 bits.
//!
//! The oracles are hand-computed: on a 40×40 canvas, layer A sits at (5, 6)
//! and layer B at (20, 3); a shift of A by (3, −2) must record A at (8, 4) in
//! every comp that captured position and touch nothing else.

use crate::common;

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::{DrDocument, LayerKind};
use doctored_core::doc_comps::{CompRequest, LayerComp};
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use common::{opaque_pattern, take_doc};

const CANVAS: u32 = 40;
const A_AT: (i32, i32) = (5, 6);
const B_AT: (i32, i32) = (20, 3);
const DELTA: (i32, i32) = (3, -2);
const A_SHIFTED: (i32, i32) = (8, 4);
/// `DR_LOCK_POSITION`, restated here rather than read out of the core.
const LOCK_POSITION: u32 = 4;

// --------------------------------------------------------------- requests --

fn save(name: &str, position: bool) -> Value {
    json!({"op":"layer_comp","action":"save","name":name,"position":position})
}

fn shift(layer: usize, dx: i32, dy: i32) -> Value {
    json!({"op":"layer_comp","action":"shift","layer":layer,"dx":dx,"dy":dy})
}

fn apply(comp: usize) -> Value {
    json!({"op":"layer_comp","action":"apply","comp":comp})
}

// ------------------------------------------------------------ byte depth --

/// Background (0), A 10×8 at (5, 6) (1), B 6×6 at (20, 3) (2).
fn byte_doc() -> DrDocument {
    let a = RgbaImage::from_fn(10, 8, |x, y| {
        Rgba([(x * 20) as u8, (y * 25) as u8, 90, 255])
    });
    let b = RgbaImage::from_fn(6, 6, |x, y| {
        Rgba([200, (x * 30) as u8, (y * 30) as u8, 255])
    });
    let mut doc = DrDocument::from_pixels(opaque_pattern(CANVAS, CANVAS))
        .adding_image_layer(0, a, "A")
        .expect("layer A")
        .adding_image_layer(1, b, "B")
        .expect("layer B");
    doc.layers[1].offset = A_AT;
    doc.layers[2].offset = B_AT;
    doc
}

/// One byte comp edit through the FFI: the document (or `None` for the
/// exact no-op) and the error sentence, if any.
fn comp8_raw(doc: &DrDocument, request: Value) -> (Option<DrDocument>, Option<String>) {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, request.as_ptr(), &mut err) };
    take_doc(out, err)
}

/// A byte comp edit that must change the document.
fn comp8(doc: &DrDocument, request: Value) -> DrDocument {
    let (out, err) = comp8_raw(doc, request.clone());
    assert!(err.is_none(), "{request}: {err:?}");
    out.unwrap_or_else(|| panic!("{request} changed nothing"))
}

/// A byte comp edit that must answer the exact no-op: NULL and no error.
fn assert_noop8(doc: &DrDocument, request: Value) {
    let (out, err) = comp8_raw(doc, request.clone());
    assert!(err.is_none(), "{request}: {err:?}");
    assert!(out.is_none(), "{request} must be the exact no-op");
}

/// The byte comp edit's refusal sentence.
fn refusal8(doc: &DrDocument, request: Value) -> String {
    let (out, err) = comp8_raw(doc, request.clone());
    assert!(out.is_none(), "{request} must be refused");
    err.unwrap_or_else(|| panic!("{request} answered no error"))
}

/// Asserts `after` is `before` with exactly the entries bound to `id` that
/// recorded an offset moved by `delta`: names, captures, visibility,
/// appearance and every other entry identical.
fn assert_only_shifted(before: &[LayerComp], after: &[LayerComp], id: u64, delta: (i32, i32)) {
    assert_eq!(before.len(), after.len(), "the comp count");
    for (b, a) in before.iter().zip(after) {
        assert_eq!(b.name, a.name);
        assert_eq!(b.comment, a.comment);
        assert_eq!(b.captures, a.captures);
        assert_eq!(b.entries.len(), a.entries.len());
        for (be, ae) in b.entries.iter().zip(&a.entries) {
            assert_eq!(be.layer_id, ae.layer_id);
            assert_eq!(be.layer_name, ae.layer_name);
            assert_eq!(be.visible, ae.visible, "{}: visibility", be.layer_name);
            assert_eq!(
                be.appearance, ae.appearance,
                "{}: appearance",
                be.layer_name
            );
            let want = match be.offset {
                Some((x, y)) if be.layer_id == id => Some((x + delta.0, y + delta.1)),
                other => other,
            };
            assert_eq!(
                ae.offset, want,
                "{} in comp {}: offset",
                be.layer_name, b.name
            );
        }
    }
}

#[test]
fn byte_shift_moves_only_that_layers_recorded_offsets() {
    let saved = comp8(&comp8(&byte_doc(), save("Here", true)), save("Look", false));
    let shifted = comp8(&saved, shift(1, DELTA.0, DELTA.1));
    let id = saved.layers[1].id;
    assert_only_shifted(saved.layer_comps(), shifted.layer_comps(), id, DELTA);
    let entry = |comp: usize| {
        shifted.layer_comps()[comp]
            .entries
            .iter()
            .find(|e| e.layer_id == id)
            .unwrap()
            .offset
    };
    assert_eq!(entry(0), Some(A_SHIFTED), "the position comp");
    assert_eq!(entry(1), None, "the position-less comp records none");
    // The layers themselves do not move: a shift is comp bookkeeping.
    assert_eq!(shifted.layers[1].offset, A_AT);
    assert_eq!(shifted.layers[2].offset, B_AT);

    let applied = comp8(&shifted, apply(0));
    assert_eq!(
        applied.layers[1].offset, A_SHIFTED,
        "Apply lands A at recorded + delta"
    );
    assert_eq!(applied.layers[2].offset, B_AT, "B stays");
}

#[test]
fn byte_shift_is_lock_exempt() {
    let saved = comp8(&byte_doc(), save("Here", true));
    let locked = saved
        .with_layer_locks(1, LOCK_POSITION)
        .expect("the lock lands");
    let shifted = comp8(&locked, shift(1, DELTA.0, DELTA.1));
    let id = saved.layers[1].id;
    assert_only_shifted(locked.layer_comps(), shifted.layer_comps(), id, DELTA);
}

#[test]
fn byte_shift_no_ops() {
    let saved = comp8(&byte_doc(), save("Here", true));
    assert_noop8(&saved, shift(1, 0, 0));
    assert_noop8(&byte_doc(), shift(1, DELTA.0, DELTA.1));
    let position_less = comp8(&byte_doc(), save("Look", false));
    assert_noop8(&position_less, shift(1, DELTA.0, DELTA.1));

    let (grouped, ..) = saved.group_layers(&[2], "G").expect("the group");
    let group = grouped
        .layers
        .iter()
        .position(|l| l.kind == LayerKind::Group)
        .expect("a group record");
    let regrouped = comp8(&grouped, save("Grouped", true));
    assert_noop8(&regrouped, shift(group, DELTA.0, DELTA.1));
}

#[test]
fn byte_shift_refusals() {
    let saved = comp8(&byte_doc(), save("Here", true));
    assert_eq!(
        refusal8(
            &saved,
            json!({"op":"layer_comp","action":"shift","dx":1,"dy":1})
        ),
        "layer comp shift requires a layer index"
    );
    assert_eq!(
        refusal8(
            &saved,
            json!({"op":"layer_comp","action":"shift","layer":1,"dy":1})
        ),
        "layer comp shift requires dx and dy"
    );
    assert_eq!(
        refusal8(
            &saved,
            json!({"op":"layer_comp","action":"shift","layer":1,"dx":1})
        ),
        "layer comp shift requires dx and dy"
    );
    assert_eq!(
        refusal8(&saved, shift(3, DELTA.0, DELTA.1)),
        "layer comp shift layer index is out of range"
    );
}

// ----------------------------------------------------------- typed depth --

fn raster16(name: &str, size: (u32, u32), offset: (i32, i32), seed: u16) -> PrecisionDocumentLayer {
    let pixels = (0..size.0 * size.1)
        .map(|i| {
            let (x, y) = ((i % size.0) as u16, (i / size.0) as u16);
            [x * 3000 + seed, y * 2700 + 11, 30000 + seed, 65535]
        })
        .collect();
    let image =
        PrecisionImage::from_rgba16(size.0, size.1, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, offset, 1.0, true).unwrap()
}

/// Postorder records: background (0), A at (5, 6) (1), C at (12, 20) (2)
/// inside group G (3), B at (20, 3) (4).
fn typed_doc() -> PrecisionDocument {
    let group = PrecisionDocumentLayer::isolated_normal(
        "G",
        vec![raster16("C", (4, 4), (12, 20), 5)],
        1.0,
        true,
    )
    .unwrap();
    PrecisionDocument::from_parts(
        CANVAS,
        CANVAS,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster16("Background", (CANVAS, CANVAS), (0, 0), 1),
            raster16("A", (10, 8), A_AT, 2),
            group,
            raster16("B", (6, 6), B_AT, 3),
        ],
    )
    .unwrap()
}

fn comp16_raw(
    doc: &PrecisionDocument,
    request: Value,
) -> Result<Option<PrecisionDocument>, String> {
    let parsed = CompRequest::parse(&request.to_string())?;
    doc.edit_layer_comps(&parsed)
}

fn comp16(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    comp16_raw(doc, request.clone())
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn assert_noop16(doc: &PrecisionDocument, request: Value) {
    match comp16_raw(doc, request.clone()) {
        Ok(None) => {}
        Ok(Some(_)) => panic!("{request} must be the exact no-op"),
        Err(e) => panic!("{request}: {e}"),
    }
}

fn refusal16(doc: &PrecisionDocument, request: Value) -> String {
    match comp16_raw(doc, request.clone()) {
        Err(e) => e,
        Ok(_) => panic!("{request} must be refused"),
    }
}

#[test]
fn typed_shift_moves_only_that_layers_recorded_offsets() {
    let doc = typed_doc();
    assert_eq!(doc.layer(1).unwrap().name(), "A");
    let saved = comp16(&comp16(&doc, save("Here", true)), save("Look", false));
    let shifted = comp16(&saved, shift(1, DELTA.0, DELTA.1));
    let id = saved.layer(1).unwrap().id();
    assert_only_shifted(saved.layer_comps(), shifted.layer_comps(), id, DELTA);
    let entry = |comp: usize| {
        shifted.layer_comps()[comp]
            .entries
            .iter()
            .find(|e| e.layer_id == id)
            .unwrap()
            .offset
    };
    assert_eq!(entry(0), Some(A_SHIFTED), "the position comp");
    assert_eq!(entry(1), None, "the position-less comp records none");
    assert_eq!(shifted.layer(1).unwrap().offset(), A_AT);

    let applied = comp16(&shifted, apply(0));
    assert_eq!(
        applied.layer(1).unwrap().offset(),
        A_SHIFTED,
        "Apply lands A at recorded + delta"
    );
    assert_eq!(applied.layer(4).unwrap().offset(), B_AT, "B stays");
    assert_eq!(applied.layer(2).unwrap().offset(), (12, 20), "C stays");
}

#[test]
fn typed_shift_no_ops() {
    let saved = comp16(&typed_doc(), save("Here", true));
    assert_noop16(&saved, shift(1, 0, 0));
    assert_noop16(&typed_doc(), shift(1, DELTA.0, DELTA.1));
    let position_less = comp16(&typed_doc(), save("Look", false));
    assert_noop16(&position_less, shift(1, DELTA.0, DELTA.1));
    assert_eq!(saved.layer(3).unwrap().name(), "G");
    assert_noop16(&saved, shift(3, DELTA.0, DELTA.1));
}

#[test]
fn typed_shift_refusals() {
    let saved = comp16(&typed_doc(), save("Here", true));
    assert_eq!(
        refusal16(
            &saved,
            json!({"op":"layer_comp","action":"shift","dx":1,"dy":1})
        ),
        "layer comp shift requires a layer index"
    );
    assert_eq!(
        refusal16(
            &saved,
            json!({"op":"layer_comp","action":"shift","layer":1,"dy":1})
        ),
        "layer comp shift requires dx and dy"
    );
    assert_eq!(
        refusal16(
            &saved,
            json!({"op":"layer_comp","action":"shift","layer":1,"dx":1})
        ),
        "layer comp shift requires dx and dy"
    );
    assert_eq!(
        refusal16(&saved, shift(5, DELTA.0, DELTA.1)),
        "layer comp shift layer index is out of range"
    );
}
