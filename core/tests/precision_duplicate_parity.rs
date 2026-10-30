//! Duplicate Layer at 16 bits (`dr_precision_doc_duplicate_layer`)
//! agrees with the byte `DrDocument::duplicating_layer` on the two pieces of
//! bookkeeping a copy changes — every copied entry is UNLINKED, and only the
//! copied ROOT gains " copy" (a duplicated group's children keep their
//! names). Each case builds the same structure at both depths and compares
//! the typed result's names and links with the byte result's.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_duplicate_layer;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

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

fn must(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            text.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        panic!("{request}: {error}");
    }
    assert!(!result.is_null(), "{request} reported no change");
    unsafe { Box::from_raw(result) }
}

fn typed_duplicate(document: &PrecisionDocument, index: usize) -> Box<PrecisionDocument> {
    let mut error = ptr::null_mut();
    let result = unsafe { dr_precision_doc_duplicate_layer(document, index, &mut error) };
    if let Some(error) = take_error(error) {
        panic!("duplicate {index}: {error}");
    }
    assert!(!result.is_null(), "duplicate {index} answered no document");
    unsafe { Box::from_raw(result) }
}

/// Bottom-first `(name, link)` per record, from the typed info rows.
fn typed_rows(document: &PrecisionDocument) -> Vec<(String, u64)> {
    let value: Value =
        serde_json::from_str(&document.info().expect("info").to_string()).expect("info JSON");
    value["layers"]
        .as_array()
        .expect("layer rows")
        .iter()
        .map(|row| {
            (
                row["name"].as_str().expect("name").to_string(),
                row["link"].as_u64().expect("link"),
            )
        })
        .collect()
}

/// Bottom-first `(name, link)` per entry of a byte document.
fn byte_rows(document: &DrDocument) -> Vec<(String, u64)> {
    document
        .layers
        .iter()
        .map(|layer| (layer.name.clone(), u64::from(layer.link)))
        .collect()
}

// ------------------------------------------------------------- fixtures --

fn tile(value: u8) -> RgbaImage {
    RgbaImage::from_pixel(2, 2, Rgba([value, 40, 90, 255]))
}

fn byte_base() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(8, 8, Rgba([255; 4])))
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

// ---------------------------------------------------------------- cases --

#[test]
fn single_layer_copy_is_named_copy_and_unlinked_at_both_depths() {
    // Background(0), A(1); duplicate A.
    let byte = byte_base().adding_image_layer(0, tile(10), "A").unwrap();
    let byte = byte.duplicating_layer(1).expect("byte duplicate");
    let doc = typed(vec![raster("Background", 65535), raster("A", 1000)]);
    let out = typed_duplicate(&doc, 1);

    let expected = vec![
        ("Background".to_string(), 0),
        ("A".to_string(), 0),
        ("A copy".to_string(), 0),
    ];
    assert_eq!(byte_rows(&byte), expected, "the byte rule");
    assert_eq!(typed_rows(&out), byte_rows(&byte), "16 bits equals 8 bits");
}

#[test]
fn linked_layer_copy_is_unlinked_and_the_pair_stays_linked_at_both_depths() {
    // Background(0), A(1), B(2); A and B linked; duplicate A.
    let byte = byte_base().adding_image_layer(0, tile(10), "A").unwrap();
    let byte = byte.adding_image_layer(1, tile(20), "B").unwrap();
    let byte = byte.link_layers(&[1, 2]).expect("byte link");
    let byte = byte.duplicating_layer(1).expect("byte duplicate");

    let doc = typed(vec![
        raster("Background", 65535),
        raster("A", 1000),
        raster("B", 2000),
    ]);
    let doc = must(&doc, json!({"op": "link_layers", "layers": [1, 2]}));
    let before = typed_rows(&doc);
    assert_ne!(before[1].1, 0, "the typed pair is linked");
    let out = typed_duplicate(&doc, 1);

    let rows = byte_rows(&byte);
    assert_eq!(rows[2], ("A copy".to_string(), 0), "the byte copy");
    assert_ne!(rows[1].1, 0, "the byte original keeps its link");
    assert_eq!(rows[1].1, rows[3].1, "the byte pair stays linked");
    assert_eq!(typed_rows(&out), rows, "16 bits equals 8 bits");
}

#[test]
fn group_copy_unlinks_every_copied_child_and_renames_only_the_root() {
    // Background(0), A(1), B(2) inside G(3); A and B linked; duplicate G.
    let byte = byte_base().adding_image_layer(0, tile(10), "A").unwrap();
    let byte = byte.adding_image_layer(1, tile(20), "B").unwrap();
    let (byte, group, _, _) = byte.group_layers(&[1, 2], "G").unwrap();
    assert_eq!(group, 3);
    let byte = byte.link_layers(&[1, 2]).expect("byte link");
    let byte = byte.duplicating_layer(3).expect("byte duplicate");

    let group =
        Layer::isolated_normal("G", vec![raster("A", 1000), raster("B", 2000)], 1.0, true).unwrap();
    let doc = typed(vec![raster("Background", 65535), group]);
    let doc = must(&doc, json!({"op": "link_layers", "layers": [1, 2]}));
    let out = typed_duplicate(&doc, 3);

    let rows = byte_rows(&byte);
    let names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["Background", "A", "B", "G", "A", "B", "G copy"]);
    assert_ne!(rows[1].1, 0, "the byte originals stay linked");
    assert_eq!(rows[1].1, rows[2].1);
    assert!(
        rows[4..].iter().all(|(_, link)| *link == 0),
        "every byte copy is unlinked: {rows:?}"
    );
    assert_eq!(typed_rows(&out), rows, "16 bits equals 8 bits");
}
