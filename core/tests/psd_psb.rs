//! Large-document PSB through the C API, both directions and both
//! depths: a PSB built by the independent fixture opens as its layers and as
//! its stored composite, and a PSB the exporter writes is read back by the
//! fixture's own parser (`common/psb_fixture.rs`, written from Adobe's table
//! and sharing no code with `src/`).
//!
//! The picture oracle is arithmetic the test owns: every fixture layer is
//! fully opaque, so the document's projection is the upper rectangle written
//! over the lower one, with no blending to borrow from the crate.
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::RgbaImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::psb_fixture::{
    self as fixture, two_layers, File, Layer, FLATTENED, H, LAYERS, PSB, PSD, W,
};

fn bytes8(pixels: &[[u16; 4]]) -> Vec<u8> {
    pixels.iter().flatten().map(|v| *v as u8).collect()
}

fn write(directory: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).expect("write");
    path
}

// ------------------------------------------------------------------ read --

#[test]
fn an_eight_bit_psb_opens_as_its_layers_and_as_its_composite() {
    let directory = TempDir::new().unwrap();
    for word in [0u16, 1] {
        let file = two_layers(2, 8, word);
        let path = write(&directory, &format!("in{word}.psb"), &file.build());
        let report = fixture::import_report(&path);
        assert_eq!(report["format"], Value::from("PSB"), "{report}");
        assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
        assert_eq!(report["stored_composite_available"], Value::Bool(true));
        assert_eq!(report["layer_count"], Value::from(2));

        let layered = fixture::open8(&path, LAYERS).expect("layers");
        let names: Vec<&str> = layered.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Lower", "Upper"]);
        assert_eq!(layered.layers[1].offset, (3, 1));
        assert_eq!(layered.layers[1].pixels.dimensions(), (4, 2));
        assert_eq!(
            layered.flattened().unwrap().into_raw(),
            bytes8(&file.composite),
            "word {word}: the layers composite to the fixture's own picture"
        );
        let stored = fixture::open8(&path, FLATTENED).expect("composite");
        assert_eq!(stored.layers.len(), 1);
        assert_eq!(
            stored.flattened().unwrap().into_raw(),
            bytes8(&file.composite)
        );
    }
}

#[test]
fn a_sixteen_bit_psb_opens_as_a_typed_document() {
    let directory = TempDir::new().unwrap();
    for word in [0u16, 1, 3] {
        let file = two_layers(2, 16, word);
        let path = write(&directory, &format!("in{word}.psb"), &file.build());
        let report = fixture::import_report(&path);
        assert_eq!(report["format"], Value::from("PSB"));
        assert_eq!(report["import_precision"], Value::from("rgba16"));
        assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
        let layered = fixture::open16(&path, LAYERS).expect("layers");
        assert_eq!(fixture::rendered16(&layered), file.composite, "word {word}");
        let stored = fixture::open16(&path, FLATTENED).expect("composite");
        assert_eq!(fixture::rendered16(&stored), file.composite);
    }
}

#[test]
fn the_same_bytes_marked_version_one_do_not_open_and_version_three_refuses() {
    let directory = TempDir::new().unwrap();
    let mut bytes = two_layers(2, 8, 1).build();
    // A PSB's u64 lengths read as a classic file's u32s frame nothing valid.
    bytes[5] = 1;
    let path = write(&directory, "v1.psd", &bytes);
    let mut error = ptr::null_mut();
    let json =
        unsafe { doctored_core::ffi_psd::dr_psd_import_report(cpath(&path).as_ptr(), &mut error) };
    if !json.is_null() {
        let report: Value = serde_json::from_str(&take_err_string(json)).unwrap();
        assert_eq!(report["layered_available"], Value::Bool(false), "{report}");
    } else {
        take_err_string(error);
    }

    bytes[5] = 3;
    let path = write(&directory, "v3.psd", &bytes);
    let mut error = ptr::null_mut();
    let json =
        unsafe { doctored_core::ffi_psd::dr_psd_import_report(cpath(&path).as_ptr(), &mut error) };
    assert!(json.is_null());
    assert!(
        take_err_string(error).contains("only PSD (1) and PSB (2) are read"),
        "an unknown version is refused by name"
    );
}

#[test]
fn a_record_holding_a_long_length_block_refuses_layers_and_keeps_the_composite() {
    let directory = TempDir::new().unwrap();
    let mut file = two_layers(2, 8, 0);
    // `lnk2` is one of the keys whose length is 8 bytes in a PSB.
    let mut block = b"8B64lnk2".to_vec();
    block.extend_from_slice(&4u64.to_be_bytes());
    block.extend_from_slice(&[0; 4]);
    file.layers[1].extra_blocks = block;
    let path = write(&directory, "long.psb", &file.build());
    let report = fixture::import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(false), "{report}");
    let refusal = report["layered_refusal"].as_str().unwrap();
    assert!(refusal.contains("64-bit-length block (lnk2)"), "{refusal}");
    assert_eq!(report["stored_composite_available"], Value::Bool(true));
    assert!(fixture::open8(&path, LAYERS).is_err());
    let stored = fixture::open8(&path, FLATTENED).expect("the composite still opens");
    assert_eq!(
        stored.flattened().unwrap().into_raw(),
        bytes8(&file.composite)
    );
}

// ----------------------------------------------------------------- write --

fn byte_document() -> (DrDocument, File) {
    let file = two_layers(2, 8, 0);
    let image = |layer: &Layer, w: u32, h: u32| {
        RgbaImage::from_raw(w, h, bytes8(&layer.rgba)).expect("image")
    };
    let mut document = DrDocument::from_pixels(image(&file.layers[0], W, H))
        .adding_image_layer(0, image(&file.layers[1], 4, 2), "Upper")
        .expect("add layer");
    document.layers[0].name = "Lower".into();
    document.layers[1].offset = (3, 1);
    (document, file)
}

fn typed_document() -> (PrecisionDocument, File) {
    let file = two_layers(2, 16, 0);
    let profile = IccProfile::srgb();
    let image = |layer: &Layer, w: u32, h: u32| {
        PrecisionImage::from_rgba16(w, h, layer.rgba.clone(), Arc::clone(&profile), None)
            .expect("image")
    };
    let document = PrecisionDocument::from_parts(
        W,
        H,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![
            PrecisionDocumentLayer::raster(
                "Lower",
                image(&file.layers[0], W, H),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            PrecisionDocumentLayer::raster(
                "Upper",
                image(&file.layers[1], 4, 2),
                (3, 1),
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    (document, file)
}

/// What the exporter wrote, read by the fixture's parser: version 2, the two
/// records with their own rectangles and samples, and the composite.
fn check_written(bytes: &[u8], file: &File, depth: u16) {
    let parsed = fixture::parse(bytes);
    assert_eq!(parsed.version, 2, "a PSB is version 2");
    assert_eq!((parsed.width, parsed.height, parsed.depth), (W, H, depth));
    let sample_bytes = usize::from(depth / 8);
    assert_eq!(parsed.layers.len(), 2);
    for (written, expected) in parsed.layers.iter().zip(&file.layers) {
        assert_eq!(written.name, expected.name);
        assert_eq!(written.bounds, expected.bounds);
        assert_eq!(written.rgba(sample_bytes).as_ref(), Some(&expected.rgba));
    }
    // Opaque everywhere, so the white matte leaves the composite untouched.
    for (component, plane) in parsed.composite.iter().enumerate() {
        assert_eq!(
            plane,
            &fixture::plane_of(&file.composite, component, sample_bytes),
            "composite plane {component}"
        );
    }
}

#[test]
fn an_eight_bit_document_is_written_as_a_psb_and_reopens() {
    let directory = TempDir::new().unwrap();
    let (document, file) = byte_document();
    let report = fixture::export_report8(&document, PSB, LAYERS);
    assert_eq!(report["format"], Value::from("PSB"));
    assert_eq!(report["container"], Value::from("PSB"));
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert_eq!(report["bake_scratch_bytes"], Value::from(0));
    assert!(fixture::strings(&report, "general_limitations")[0].contains("large-document PSB"));
    let path = directory.path().join("out.psb");
    fixture::save8(&document, &path, PSB, LAYERS).expect("save");
    check_written(&std::fs::read(&path).unwrap(), &file, 8);

    let reopened = fixture::open8(&path, LAYERS).expect("reopen");
    assert_eq!(fixture::import_report(&path)["format"], Value::from("PSB"));
    assert_eq!(reopened.layers.len(), 2);
    assert_eq!(
        reopened.flattened().unwrap().into_raw(),
        bytes8(&file.composite)
    );

    // The flattened mode writes one canvas-sized record.
    let flat = directory.path().join("flat.psb");
    fixture::save8(&document, &flat, PSB, FLATTENED).expect("save flattened");
    let parsed = fixture::parse(&std::fs::read(&flat).unwrap());
    assert_eq!(parsed.layers.len(), 1);
    assert_eq!(parsed.layers[0].rgba(1).unwrap(), file.composite);
}

#[test]
fn a_sixteen_bit_document_is_written_as_a_psb_and_reopens() {
    let directory = TempDir::new().unwrap();
    let (document, file) = typed_document();
    let report = fixture::export_report16(&document, PSB, LAYERS);
    assert_eq!(report["format"], Value::from("PSB"));
    assert_eq!(report["depth"], Value::from(16));
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    let path = directory.path().join("out16.psb");
    fixture::save16(&document, &path, PSB, LAYERS).expect("save");
    check_written(&std::fs::read(&path).unwrap(), &file, 16);
    let reopened = fixture::open16(&path, LAYERS).expect("reopen");
    assert_eq!(fixture::rendered16(&reopened), file.composite);
}

#[test]
fn a_canvas_past_thirty_thousand_is_refused_as_psd_naming_psb_and_written_as_psb() {
    let directory = TempDir::new().unwrap();
    let wide = DrDocument::from_pixels(RgbaImage::from_pixel(
        30_016,
        4,
        image::Rgba([10, 20, 30, 255]),
    ));
    let classic = fixture::export_report8(&wide, PSD, LAYERS);
    assert_eq!(classic["available"], Value::Bool(false));
    let refusal = classic["refusal"].as_str().unwrap();
    assert!(refusal.contains("30000 pixels per side"), "{refusal}");
    assert!(refusal.contains("Large Document (PSB)"), "{refusal}");
    let sentinel = directory.path().join("kept.psd");
    std::fs::write(&sentinel, b"kept").unwrap();
    assert_eq!(
        fixture::save8(&wide, &sentinel, PSD, LAYERS).unwrap_err(),
        refusal
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"kept");

    let large = fixture::export_report8(&wide, PSB, LAYERS);
    assert_eq!(large["available"], Value::Bool(true), "{large}");
    let path = directory.path().join("wide.psb");
    fixture::save8(&wide, &path, PSB, LAYERS).expect("save");
    let parsed = fixture::parse(&std::fs::read(&path).unwrap());
    assert_eq!(
        (parsed.version, parsed.width, parsed.height),
        (2, 30_016, 4)
    );
    let report = fixture::import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let reopened = fixture::open8(&path, LAYERS).expect("reopen");
    assert_eq!((reopened.width, reopened.height), (30_016, 4));
    assert!(reopened
        .flattened()
        .unwrap()
        .pixels()
        .all(|p| p.0 == [10, 20, 30, 255]));
}

// -------------------------------------------------------------- the edges --

#[test]
fn the_changed_exports_refuse_null_and_an_unknown_container() {
    let (document, _) = byte_document();
    let (typed, _) = typed_document();
    let path = cpath(&std::env::temp_dir().join("rz-never-written.psd"));
    unsafe {
        use doctored_core::ffi_psd::{
            dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_psd_export_report,
            dr_precision_doc_save_psd,
        };
        let mut error = ptr::null_mut();
        assert!(dr_doc_psd_export_report(ptr::null(), PSB, LAYERS, &mut error).is_null());
        assert!(!take_err_string(error).is_empty());
        let mut error = ptr::null_mut();
        assert!(dr_doc_psd_export_report(&document, 3, LAYERS, &mut error).is_null());
        assert_eq!(
            take_err_string(error),
            "unknown layered container; use 0 (PSD), 1 (PSB) or 2 (TIFF)"
        );
        let mut error = ptr::null_mut();
        assert!(!dr_doc_save_psd(
            ptr::null(),
            ptr::null(),
            path.as_ptr(),
            PSB,
            LAYERS,
            &mut error
        ));
        take_err_string(error);
        let mut error = ptr::null_mut();
        assert!(!dr_doc_save_psd(
            &document,
            ptr::null(),
            ptr::null(),
            PSB,
            LAYERS,
            &mut error
        ));
        take_err_string(error);
        let mut error = ptr::null_mut();
        assert!(!dr_doc_save_psd(
            &document,
            ptr::null(),
            path.as_ptr(),
            -1,
            LAYERS,
            &mut error
        ));
        assert!(take_err_string(error).contains("unknown layered container"));
        // NULL err_out is tolerated on every refusal.
        assert!(dr_doc_psd_export_report(ptr::null(), PSB, LAYERS, ptr::null_mut()).is_null());

        let mut error = ptr::null_mut();
        assert!(dr_precision_doc_psd_export_report(ptr::null(), PSB, LAYERS, &mut error).is_null());
        take_err_string(error);
        let mut error = ptr::null_mut();
        assert!(dr_precision_doc_psd_export_report(&typed, 9, LAYERS, &mut error).is_null());
        assert!(take_err_string(error).contains("unknown layered container"));
        let mut error = ptr::null_mut();
        assert!(!dr_precision_doc_save_psd(
            &typed,
            ptr::null(),
            path.as_ptr(),
            PSB,
            LAYERS,
            u64::MAX,
            0,
            &mut error
        ));
        take_err_string(error);
        assert!(!dr_precision_doc_save_psd(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            PSB,
            LAYERS,
            0,
            0,
            ptr::null_mut()
        ));
    }
}
