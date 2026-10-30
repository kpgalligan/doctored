//! A layer OFFSET is a property write, not a move, so it never follows
//! LINKS — at 8 bits (`dr_doc_with_layer_offset`) and at 16 bits, where the
//! host sends the typed `affine_set` with `follow_links: false`. A group still
//! carries its own subtree. The move paths (`dr_doc_move_layers`, a plain
//! `affine_set`) keep following links, and a test here says so too.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_with_layer_offset;
use doctored_core::ffi_geometry::dr_doc_geometry_edit;
use doctored_core::ffi_group::dr_doc_move_layers;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// `DrLockFlags`' Position bit, restated rather than read out of the core.
const LOCK_POSITION: u32 = 4;

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
    request: Value,
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

fn must(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    match edit(document, request.clone()) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn refusal(document: &PrecisionDocument, request: Value) -> String {
    match edit(document, request.clone()) {
        Err(message) => message,
        Ok(Some(_)) => panic!("{request} must be refused"),
        Ok(None) => panic!("{request} must be refused, not answered as a no-op"),
    }
}

fn info(document: &PrecisionDocument) -> Vec<Value> {
    let value: Value =
        serde_json::from_str(&document.info().expect("info").to_string()).expect("info JSON");
    value["layers"].as_array().expect("layer rows").clone()
}

fn offsets(document: &PrecisionDocument) -> Vec<Value> {
    info(document)
        .iter()
        .map(|row| row["offset"].clone())
        .collect()
}

fn links(document: &PrecisionDocument) -> Vec<Value> {
    info(document)
        .iter()
        .map(|row| row["link"].clone())
        .collect()
}

/// The host's typed offset: an integer `affine_set` that does not follow links.
fn offset(layer: usize, dx: i32, dy: i32) -> Value {
    json!({"op": "affine_set", "layers": [layer], "matrix": [1, 0, 0, 1, dx, dy],
        "filter": "nearest", "follow_links": false})
}

/// The Move tool's typed set: the same translation, following links.
fn moved(layer: usize, dx: i32, dy: i32) -> Value {
    json!({"op": "affine_set", "layers": [layer], "matrix": [1, 0, 0, 1, dx, dy],
        "filter": "nearest"})
}

fn byte_offset(doc: &DrDocument, idx: usize, x: i32, y: i32) -> Box<DrDocument> {
    let out = unsafe { dr_doc_with_layer_offset(doc, idx, x, y) };
    assert!(!out.is_null(), "the byte offset of {idx} changed nothing");
    unsafe { Box::from_raw(out) }
}

// ------------------------------------------------------------- fixtures --

fn tile(value: u8) -> RgbaImage {
    RgbaImage::from_pixel(2, 2, Rgba([value, 40, 90, 255]))
}

/// Background(0), A(1), B(2), C(3); A and B linked.
fn byte_linked() -> DrDocument {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(8, 8, Rgba([255; 4])));
    let doc = doc.adding_image_layer(0, tile(10), "A").unwrap();
    let doc = doc.adding_image_layer(1, tile(20), "B").unwrap();
    let doc = doc.adding_image_layer(2, tile(30), "C").unwrap();
    doc.link_layers(&[1, 2]).unwrap()
}

/// Background(0), A(1) inside G(2), B(3) outside; A and B linked.
fn byte_grouped() -> DrDocument {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(8, 8, Rgba([255; 4])));
    let doc = doc.adding_image_layer(0, tile(10), "A").unwrap();
    let doc = doc.adding_image_layer(1, tile(20), "B").unwrap();
    let (doc, group, _, _) = doc.group_layers(&[1], "G").unwrap();
    assert_eq!(group, 2);
    doc.link_layers(&[1, 3]).unwrap()
}

fn raster(name: &str, value: u16) -> Layer {
    let image = PrecisionImage::from_rgba16(
        2,
        2,
        vec![[value, 9000, 30000, 65535]; 4],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    Layer::raster(name, image, (0, 0), 1.0, true).unwrap()
}

fn typed(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        8,
        8,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// A(0), B(1), C(2); A and B linked.
fn typed_linked() -> Box<PrecisionDocument> {
    let doc = typed(vec![
        raster("A", 1000),
        raster("B", 2000),
        raster("C", 3000),
    ]);
    must(&doc, json!({"op": "link_layers", "layers": [0, 1]}))
}

/// A(0) inside G(1), B(2) outside; A and B linked.
fn typed_grouped() -> Box<PrecisionDocument> {
    let group = Layer::isolated_normal("G", vec![raster("A", 1000)], 1.0, true).unwrap();
    let doc = typed(vec![group, raster("B", 2000)]);
    must(&doc, json!({"op": "link_layers", "layers": [0, 2]}))
}

// ----------------------------------------------------------------- byte --

#[test]
fn byte_offset_moves_the_entry_and_not_its_link_partner() {
    let doc = byte_linked();
    assert_ne!(doc.layers[1].link, 0);
    assert_eq!(doc.layers[1].link, doc.layers[2].link);
    let out = byte_offset(&doc, 1, 3, 2);
    assert_eq!(out.layers[1].offset, (3, 2), "A moved");
    assert_eq!(out.layers[2].offset, (0, 0), "linked B stays");
    assert_eq!(out.layers[3].offset, (0, 0), "unlinked C stays");
    assert_eq!(out.layers[1].link, doc.layers[1].link, "the link survives");

    // The MOVE still follows the link: moving A carries B.
    let moved = unsafe { dr_doc_move_layers(&doc, [1usize].as_ptr(), 1, 3, 2) };
    assert!(!moved.is_null());
    let moved = unsafe { Box::from_raw(moved) };
    assert_eq!(moved.layers[2].offset, (3, 2), "a move carries linked B");
}

#[test]
fn byte_group_offset_moves_its_subtree_and_not_an_outside_partner() {
    let doc = byte_grouped();
    assert_ne!(doc.layers[1].link, 0);
    assert_eq!(doc.layers[1].link, doc.layers[3].link);
    let out = byte_offset(&doc, 2, 3, 1);
    assert_eq!(out.layers[1].offset, (3, 1), "A inside G moved");
    assert_eq!(out.layers[3].offset, (0, 0), "B outside G stays");
}

#[test]
fn byte_affine_set_refuses_the_no_link_flag_by_name() {
    let doc = byte_linked();
    let request = CString::new(offset(1, 3, 2).to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out =
        unsafe { dr_doc_geometry_edit(&doc, request.as_ptr(), LIMIT, 0, ptr::null(), &mut error) };
    assert!(out.is_null());
    let message = take_error(error).expect("a named refusal");
    assert!(message.contains("dr_doc_with_layer_offset"), "{message}");
}

// ---------------------------------------------------------------- typed --

#[test]
fn typed_offset_moves_the_entry_and_not_its_link_partner() {
    let doc = typed_linked();
    let before = links(&doc);
    assert_ne!(before[0], json!(0));
    assert_eq!(before[0], before[1]);

    let out = must(&doc, offset(0, 3, 2));
    assert_eq!(
        offsets(&out),
        vec![json!([3, 2]), json!([0, 0]), json!([0, 0])],
        "only A moved"
    );
    assert_eq!(links(&out), before, "every link id survives");

    // A plain affine set is the MOVE and still carries linked B.
    let moved = must(&doc, moved(0, 3, 2));
    assert_eq!(
        offsets(&moved),
        vec![json!([3, 2]), json!([3, 2]), json!([0, 0])]
    );
}

#[test]
fn typed_group_offset_moves_its_subtree_and_not_an_outside_partner() {
    let doc = typed_grouped();
    let before = links(&doc);
    assert_ne!(before[0], json!(0));
    assert_eq!(before[0], before[2]);

    let out = must(&doc, offset(1, 3, 1));
    let rows = info(&out);
    assert_eq!(rows[0]["offset"], json!([3, 1]), "A inside G moved");
    assert_eq!(rows[2]["offset"], json!([0, 0]), "B outside G stays");
    assert_eq!(links(&out), before, "every link id survives");
}

#[test]
fn typed_offset_zero_is_a_no_op_and_only_the_members_answer_to_position_locks() {
    let doc = typed_linked();
    assert!(
        matches!(edit(&doc, offset(0, 0, 0)), Ok(None)),
        "a zero offset is NULL with no error"
    );

    let member = must(
        &doc,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_POSITION}),
    );
    let message = refusal(&member, offset(0, 3, 2));
    assert!(message.to_lowercase().contains("position"), "{message}");

    // A position-locked link PARTNER is not a member of the no-link set, so
    // the offset goes through; the move, which carries it, still refuses.
    let partner = must(
        &doc,
        json!({"op": "layer_lock", "layer": 1, "locks": LOCK_POSITION}),
    );
    let out = must(&partner, offset(0, 3, 2));
    assert_eq!(offsets(&out)[0], json!([3, 2]));
    let message = refusal(&partner, moved(0, 3, 2));
    assert!(message.to_lowercase().contains("position"), "{message}");
}

#[test]
fn typed_no_link_flag_is_strict() {
    let doc = typed_linked();
    let mut unknown = offset(0, 3, 2);
    unknown["bogus"] = json!(1);
    refusal(&doc, unknown);

    let mut wrong_type = offset(0, 3, 2);
    wrong_type["follow_links"] = json!("no");
    refusal(&doc, wrong_type);

    // The flag belongs to the affine set only.
    refusal(
        &doc,
        json!({"op": "perspective_set", "layers": [0], "source_rect": [0, 0, 2, 2],
            "quad": [1, 1, 3, 1, 3, 3, 1, 3], "filter": "nearest", "follow_links": false}),
    );
}
