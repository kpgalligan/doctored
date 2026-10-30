//! Align and Distribute at 16-bit and 32-bit float, the typed
//! `align_set` / `distribute_set` requests of `dr_precision_doc_edit`.
//! Black-box through the FFI. The 16-bit expectations are the byte answers
//! (`dr_doc_align_layers` / `dr_doc_distribute_layers`) on the same fixture
//! promoted by `dr_doc_promote_structure`; the float expectations are
//! offsets written HERE from the stated rule, never computed by the code
//! under test.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, Layer, LayerKind};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_group::{dr_doc_align_layers, dr_doc_distribute_layers};
use doctored_core::ffi_precision::{
    dr_precision_doc_layer_source, dr_precision_image_free, dr_precision_image_storage_identity,
};
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::SourceEdit;
use doctored_core::tiff_float::FloatTiff;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 64;
const H: u32 = 48;
/// `DrLockFlags`' Position bit, restated rather than read out of the core.
const LOCK_POSITION: u32 = 4;
/// `DrAlign`, restated: left, horizontal centre, right, top, vertical
/// centre, bottom.
const EDGES: [(i32, &str); 6] = [
    (0, "left"),
    (1, "horizontal_center"),
    (2, "right"),
    (3, "top"),
    (4, "vertical_center"),
    (5, "bottom"),
];

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    unsafe {
        let text = CStr::from_ptr(error).to_string_lossy().into_owned();
        dr_string_free(error);
        Some(text)
    }
}

fn edit(
    document: &PrecisionDocument,
    request: &Value,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    Ok((!result.is_null()).then(|| unsafe { Box::from_raw(result) }))
}

fn plan(document: &PrecisionDocument, request: &Value) -> Result<Value, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result =
        unsafe { dr_precision_doc_edit_plan(document, request.as_ptr(), LIMIT, 0, &mut error) };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    assert!(!result.is_null());
    let text = unsafe { CStr::from_ptr(result) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(result) };
    Ok(serde_json::from_str(&text).unwrap())
}

fn must(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    match edit(document, &request) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn refusal(document: &PrecisionDocument, request: Value) -> String {
    match edit(document, &request) {
        Err(message) => message,
        Ok(Some(_)) => panic!("{request} must be refused"),
        Ok(None) => panic!("{request} must be refused, not answered as a no-op"),
    }
}

fn no_op(document: &PrecisionDocument, request: Value) {
    match edit(document, &request) {
        Ok(None) => {}
        Ok(Some(_)) => panic!("{request} must be a no-op"),
        Err(message) => panic!("{request} must be a no-op: {message}"),
    }
    plan(document, &request).unwrap_or_else(|e| panic!("{request} plan: {e}"));
}

fn rows(document: &PrecisionDocument) -> Vec<Value> {
    let value: Value =
        serde_json::from_str(&document.info().expect("info").to_string()).expect("info JSON");
    value["layers"].as_array().expect("layer rows").clone()
}

fn offset(row: &Value) -> (i32, i32) {
    let o = &row["offset"];
    (o[0].as_i64().unwrap() as i32, o[1].as_i64().unwrap() as i32)
}

fn mask_offset(row: &Value) -> Option<(i32, i32)> {
    let o = row["mask"].get("offset")?;
    Some((o[0].as_i64().unwrap() as i32, o[1].as_i64().unwrap() as i32))
}

fn align(layers: &[usize], edge: &str, to_canvas: bool) -> Value {
    json!({"op": "align_set", "layers": layers, "edge": edge, "to_canvas": to_canvas})
}

fn distribute(layers: &[usize], vertical: bool) -> Value {
    json!({"op": "distribute_set", "layers": layers, "vertical": vertical})
}

/// The host's typed offset write: an integer `affine_set` that does not
/// follow links.
fn shift(document: &PrecisionDocument, layer: usize, dx: i32, dy: i32) -> Box<PrecisionDocument> {
    must(
        document,
        json!({"op": "affine_set", "layers": [layer], "matrix": [1, 0, 0, 1, dx, dy],
            "filter": "nearest", "follow_links": false}),
    )
}

// ------------------------------------------------------- byte fixture --

fn opaque(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([50, 60, 70, 255]))
}

/// A buffer whose content box is its interior: alpha 0 on a one-pixel
/// (two at the left) border.
fn framed(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let inside = x >= 2 && x + 1 < w && y >= 1 && y + 1 < h;
        Rgba([90, 80, 70, if inside { 200 } else { 0 }])
    })
}

fn fresh(image: RgbaImage, name: &str, offset: (i32, i32)) -> Layer {
    let mut layer = DrDocument::from_pixels(image).layers.remove(0);
    layer.name = name.into();
    layer.offset = offset;
    layer
}

fn linked_mask(mut layer: Layer, size: (u32, u32), at: (i32, i32)) -> Layer {
    layer.mask = Some(Arc::new(GrayImage::from_fn(size.0, size.1, |x, y| {
        Luma([((x * 31 + y * 17 + 7) % 256) as u8])
    })));
    let mut properties = MaskProperties::at(at);
    properties.linked = true;
    layer.mask_properties = properties;
    layer.mask_enabled = true;
    layer
}

fn nested(mut layer: Layer) -> Layer {
    layer.depth = 1;
    layer
}

fn linked(mut layer: Layer, link: u32) -> Layer {
    layer.link = link;
    layer
}

/// Bottom-first:
///  0 Empty (transparent, canvas-sized — nothing opaque), 1 A, 2 B, 3 C
///  (framed: its content box is its interior), 4 Child1 (depth 1, link 2),
///  5 Child2 (depth 1), 6 G (group of 4 and 5 with a linked mask), 7 Adj
///  (Levels with a linked mask, link 1), 8 L1 (link 1), 9 X (link 2, outside
///  the group), 10 P (link 3), 11 Q (link 3), 12 Wide (wider than the
///  span the distribute sets give it).
fn byte_fixture() -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::new(W, H));
    doc.layers[0].name = "Empty".into();
    let mut group = fresh(RgbaImage::new(1, 1), "G", (0, 0));
    group.kind = LayerKind::Group;
    let group = linked_mask(group, (20, 12), (6, 22));
    // A transparent canvas-sized buffer, as the host's
    // `addingAdjustmentLayer` makes one: no content box at either depth.
    let mut adj = fresh(RgbaImage::new(W, H), "Adj", (0, 0));
    adj.meta = Some(r#"{"type":"adjust","op":"levels","params":{"gamma":0.8}}"#.into());
    let adj = linked(linked_mask(adj, (10, 10), (40, 20)), 1);
    doc.layers.extend([
        fresh(opaque(10, 6), "A", (5, 4)),
        fresh(opaque(7, 9), "B", (20, 12)),
        fresh(framed(12, 8), "C", (33, 3)),
        linked(nested(fresh(opaque(6, 5), "Child1", (8, 25))), 2),
        nested(fresh(opaque(5, 5), "Child2", (18, 28))),
        group,
        adj,
        linked(fresh(opaque(4, 4), "L1", (44, 30)), 1),
        linked(fresh(opaque(6, 3), "X", (50, 5)), 2),
        linked(fresh(opaque(9, 4), "P", (2, 33)), 3),
        linked(fresh(opaque(3, 7), "Q", (28, 30)), 3),
        fresh(opaque(30, 3), "Wide", (15, 36)),
    ]);
    doc
}

fn promoted(byte: &DrDocument) -> Box<PrecisionDocument> {
    let mut error = ptr::null_mut();
    let typed = unsafe { dr_doc_promote_structure(byte, LIMIT, 0, ptr::null(), &mut error) };
    assert!(take_error(error).is_none(), "promotion failed");
    assert!(!typed.is_null());
    unsafe { Box::from_raw(typed) }
}

/// One record's raster offset (rasters only) and mask offset.
type Placement = (Option<(i32, i32)>, Option<(i32, i32)>);

/// The byte result's raster offsets and every mask offset, in the shape the
/// typed rows answer: raster rows by offset, every row by mask offset.
fn byte_state(doc: &DrDocument) -> Vec<Placement> {
    doc.layers
        .iter()
        .map(|layer| {
            let raster = layer.kind == LayerKind::Raster && layer.meta.is_none();
            (
                raster.then_some(layer.offset),
                layer.mask.as_ref().map(|_| layer.mask_properties.offset),
            )
        })
        .collect()
}

fn typed_state(doc: &PrecisionDocument) -> Vec<Placement> {
    rows(doc)
        .iter()
        .map(|row| {
            let raster = row["kind"] == "raster";
            (raster.then(|| offset(row)), mask_offset(row))
        })
        .collect()
}

fn byte_answer(out: *mut DrDocument) -> Option<Box<DrDocument>> {
    (!out.is_null()).then(|| unsafe { Box::from_raw(out) })
}

/// The typed request and the byte call agree: both no-ops, or the same
/// raster and mask offsets everywhere.
fn agree(
    byte: &DrDocument,
    typed: &PrecisionDocument,
    byte_out: Option<Box<DrDocument>>,
    request: Value,
) -> bool {
    let typed_out = edit(typed, &request).unwrap_or_else(|e| panic!("{request}: {e}"));
    match (byte_out, typed_out) {
        (None, None) => {
            plan(typed, &request).unwrap_or_else(|e| panic!("{request} plan: {e}"));
            false
        }
        (Some(b), Some(t)) => {
            assert_eq!(typed_state(&t), byte_state(&b), "{request}");
            assert_ne!(
                byte_state(&b),
                byte_state(byte),
                "{request} moved something"
            );
            true
        }
        (b, t) => panic!(
            "{request}: byte answered {}, typed {}",
            if b.is_some() { "a document" } else { "NULL" },
            if t.is_some() { "a document" } else { "NULL" }
        ),
    }
}

// --------------------------------------------------- 16-bit parity --

#[test]
fn a_promoted_16_bit_document_aligns_and_distributes_as_the_byte_document_does() {
    let byte = byte_fixture();
    let typed = promoted(&byte);
    assert_eq!(typed.layer_count(), byte.layers.len());
    assert_eq!(typed_state(&typed), byte_state(&byte), "the promotion");

    let sets: [&[usize]; 8] = [
        &[1, 2, 3],
        &[0, 1, 2, 3],  // an entry with nothing opaque
        &[6, 4, 2],     // a group and its own child: one root
        &[10, 11, 3],   // two linked roots
        &[6, 2],        // a group child linked outside the group
        &[7, 8, 1],     // an adjustment root with a linked mask
        &[12, 1, 2, 3], // wider than its span
        &[1],           // alone
    ];
    let mut moved = 0;
    for set in sets {
        for (code, name) in EDGES {
            for to_canvas in [false, true] {
                let out =
                    unsafe { dr_doc_align_layers(&byte, set.as_ptr(), set.len(), code, to_canvas) };
                let out = byte_answer(out);
                moved += usize::from(agree(&byte, &typed, out, align(set, name, to_canvas)));
            }
        }
        for vertical in [false, true] {
            let out = unsafe { dr_doc_distribute_layers(&byte, set.as_ptr(), set.len(), vertical) };
            let out = byte_answer(out);
            moved += usize::from(agree(&byte, &typed, out, distribute(set, vertical)));
        }
    }
    assert!(moved > 60, "the fixture exercises real moves ({moved})");
}

#[test]
fn the_parity_cases_move_what_the_rule_says() {
    let byte = byte_fixture();
    let typed = promoted(&byte);
    // A group and its own child: one root, G, whose content box is the union
    // of Child1 (8..14) and Child2 (18..23); B (20..27) is the other root.
    // Left: G's box already starts at 8, so B moves to 8 and G stays.
    let out = must(&typed, align(&[6, 4, 2], "left", false));
    let state = typed_state(&out);
    assert_eq!(state[2].0, Some((8, 12)), "B to the union's left edge");
    assert_eq!(state[4].0, Some((8, 25)), "Child1 moved once, not twice");
    // Two linked roots: P and Q each claim both; the LAST root (Q, index
    // 11) wins. Left to the canvas: P's delta -2, Q's -28, so both take -28.
    let out = must(&typed, align(&[10, 11], "left", true));
    let state = typed_state(&out);
    assert_eq!(state[10].0, Some((2 - 28, 33)));
    assert_eq!(state[11].0, Some((0, 30)));
    // An adjustment root has no box; aligning L1 to the canvas's top carries
    // Adj's linked mask by L1's delta (-30) through their link.
    let out = must(&typed, align(&[7, 8], "top", true));
    let state = typed_state(&out);
    assert_eq!(state[8].0, Some((44, 0)));
    assert_eq!(state[7].1, Some((40, -10)));
}

// --------------------------------------------------------------- float --

/// A float document holding one 40×20 raster whose only opaque pixels are a
/// 4×3 block at its top-left, through a float TIFF opened with
/// `open_preserving` — the only way a float raster is made.
fn float_document() -> PrecisionDocument {
    let (width, height) = (40u32, 20u32);
    let samples = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let a = if x < 4 && y < 3 { 1.0 } else { 0.0 };
            [1.75, -0.5, 0.25, a]
        })
        .collect();
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples,
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("align.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// Three copies of the float raster at offsets (0, 0), (10, 5), (22, 9).
fn float_three() -> Box<PrecisionDocument> {
    let doc = float_document();
    let doc = doc.duplicate_layer(0).unwrap().unwrap();
    let doc = doc.duplicate_layer(1).unwrap().unwrap();
    let doc = shift(&doc, 1, 10, 5);
    shift(&doc, 2, 22, 9)
}

fn offsets(doc: &PrecisionDocument) -> Vec<(i32, i32)> {
    rows(doc).iter().map(offset).collect()
}

#[test]
fn float_documents_align_and_distribute_by_the_stated_offsets() {
    let doc = float_three();
    assert_eq!(offsets(&doc), [(0, 0), (10, 5), (22, 9)]);
    // Left: every content box (4×3 at the offset) to the union's left, 0.
    let out = must(&doc, align(&[0, 1, 2], "left", false));
    assert_eq!(offsets(&out), [(0, 0), (0, 5), (0, 9)]);
    // Bottom to the canvas (40×20): every box's bottom (offset + 3) to 20.
    let out = must(&doc, align(&[0, 1, 2], "bottom", true));
    assert_eq!(offsets(&out), [(0, 17), (10, 17), (22, 17)]);
    // Horizontal centre of the union 0..26 is 13; a 4-wide box centres at
    // offset 11.
    let out = must(&doc, align(&[0, 1, 2], "horizontal_center", false));
    assert_eq!(offsets(&out), [(11, 0), (11, 5), (11, 9)]);
    // Distribute: the outermost keep 0 and 22, the middle lands at
    // round((0 + 22) / 2) = 11; vertically round((0 + 9) / 2) = 5 already.
    let out = must(&doc, distribute(&[0, 1, 2], false));
    assert_eq!(offsets(&out), [(0, 0), (11, 5), (22, 9)]);
    no_op(&doc, distribute(&[0, 1, 2], true));
    assert!(out.is_float());
}

// ---------------------------------------------------------- invariants --

fn storage_identity(doc: &PrecisionDocument, index: usize) -> u64 {
    let image = unsafe { dr_precision_doc_layer_source(doc, index) };
    assert!(!image.is_null());
    let identity = unsafe { dr_precision_image_storage_identity(image) };
    unsafe { dr_precision_image_free(image) };
    identity
}

#[test]
fn an_align_moves_storage_untouched_at_both_typed_depths() {
    let byte = byte_fixture();
    let typed = promoted(&byte);
    let out = must(&typed, distribute(&[1, 2, 3, 12], false));
    for index in [1, 2, 3, 12] {
        assert_eq!(
            storage_identity(&out, index),
            storage_identity(&typed, index),
            "record {index} keeps its storage"
        );
    }
    let float = float_three();
    let out = must(&float, align(&[0, 1, 2], "top", false));
    for index in 0..3 {
        assert_eq!(
            storage_identity(&out, index),
            storage_identity(&float, index)
        );
    }
}

#[test]
fn an_align_keeps_an_artwork_description() {
    let byte = byte_fixture();
    let typed = promoted(&byte);
    let described = {
        let description = CString::new(r#"{"type":"shape","version":3,"probe":"align"}"#).unwrap();
        let name = CString::new("A").unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let source = typed.layer_source(1).unwrap();
        let out = unsafe {
            doctored_core::ffi_artwork::dr_precision_doc_artwork(
                &*typed,
                1,
                true,
                source,
                5,
                4,
                name.as_ptr(),
                description.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(take_error(err).is_none());
        assert!(!out.is_null(), "the artwork publication is admitted");
        unsafe { Box::from_raw(out) }
    };
    assert!(described.artwork_description(1).is_some());
    let out = must(&described, align(&[1, 2], "right", false));
    assert_ne!(typed_state(&out)[1], typed_state(&described)[1], "A moved");
    assert_eq!(
        out.artwork_description(1),
        described.artwork_description(1),
        "an integer translation keeps the description"
    );
}

fn retained_document() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let base = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(16, 16, vec![[2, 4, 6, 65535]; 256], profile.clone(), None)
            .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let original = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            4,
            4,
            (0..16)
                .map(|v| [v * 101 + 1, v * 67 + 3, 65535 - v * 71, 65535])
                .collect(),
            profile,
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"master","revision_id":"first","name":"Source",
            "x":3,"y":5})
        .to_string(),
    )
    .unwrap();
    base.source_edit(Some(&original), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

#[test]
fn a_retained_source_binding_moves_as_a_nearest_affine_set_does() {
    let doc = retained_document();
    // The placed record (1) to the canvas's right edge: its 4-wide content
    // box at x 3 moves by 16 - 7 = +9.
    let aligned = must(&doc, align(&[1], "right", true));
    let moved = must(
        &doc,
        json!({"op": "affine_set", "layers": [1], "matrix": [1, 0, 0, 1, 9, 0],
            "filter": "nearest"}),
    );
    let a = aligned.layer(1).unwrap().effective_source().unwrap();
    let m = moved.layer(1).unwrap().effective_source().unwrap();
    assert_eq!(a.transform, m.transform);
    assert_eq!(
        std::mem::discriminant(&a.filter),
        std::mem::discriminant(&m.filter)
    );
    assert_eq!(a.rendered_offset, m.rendered_offset);
    assert_eq!(offsets(&aligned)[1], (12, 5));
}

// ------------------------------------------------------------ refusals --

#[test]
fn a_position_lock_refuses_by_name_even_where_nothing_would_move() {
    let byte = byte_fixture();
    let typed = promoted(&byte);
    // A defines the left edge of {A, B}: its delta is zero, yet its lock
    // refuses the whole call.
    let locked = must(
        &typed,
        json!({"op": "layer_lock", "layer": 1, "locks": LOCK_POSITION}),
    );
    let message = refusal(&locked, align(&[1, 2], "left", false));
    assert!(message.contains("locked (position)"), "{message}");
    // L1 is a link-only partner of Adj (7); aligning Adj alone moves
    // nothing (no box) but the lock on its partner still refuses.
    let locked = must(
        &typed,
        json!({"op": "layer_lock", "layer": 8, "locks": LOCK_POSITION}),
    );
    let message = refusal(&locked, align(&[7, 2], "left", false));
    assert!(
        message.contains("layer 8") && message.contains("position"),
        "{message}"
    );
}

#[test]
fn an_empty_or_out_of_range_set_refuses() {
    let byte = byte_fixture();
    let typed = promoted(&byte);
    refusal(&typed, align(&[], "left", true));
    refusal(&typed, distribute(&[], false));
    refusal(&typed, align(&[1, 99], "left", false));
    refusal(&typed, distribute(&[1, 2, 99], true));
    assert!(plan(&typed, &align(&[99], "left", true)).is_err());
    let message = refusal(
        &typed,
        json!({"op": "align_set", "layers": [1], "edge": "middle",
        "to_canvas": true}),
    );
    assert!(message.contains("invalid typed align request"), "{message}");
    refusal(
        &typed,
        json!({"op": "distribute_set", "layers": [1, 2, 3], "vertical": true,
        "extra": 1}),
    );
}

// -------------------------------------------------------------- no-ops --

#[test]
fn nothing_to_do_is_a_null_with_no_error_and_a_plan() {
    let byte = byte_fixture();
    let typed = promoted(&byte);
    let aligned = must(&typed, align(&[1, 2, 3], "top", false));
    no_op(&aligned, align(&[1, 2, 3], "top", false));
    no_op(&typed, align(&[1], "left", false));
    no_op(&typed, align(&[0, 1], "left", false));
    no_op(&typed, distribute(&[1, 2], false));
    no_op(&typed, distribute(&[0, 1, 2], true));
    // Fewer than three boxes once the empty entry is skipped.
    no_op(&typed, distribute(&[0, 7, 1, 2], false));
}
