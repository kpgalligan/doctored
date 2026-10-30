use std::ffi::CStr;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_info};
use doctored_core::ffi_precision_inspect::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

fn source() -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[10001, 20002, 30003, 65535], [10002, 20003, 30004, 1]],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let layer = Layer::raster("Leaf\nwith \"quotes\"", image, (-3, 4), 1.0, true).unwrap();
    let nested = Layer::pass_through("Inner", vec![layer], 0.5, true).unwrap();
    let outer = Layer::isolated_normal("Outer", vec![nested], 0.75, true).unwrap();
    let metadata = Metadata {
        xmp: Some(b"bounded metadata".to_vec().into()),
        ..Metadata::default()
    };
    PrecisionDocument::from_parts(
        2,
        1,
        IccProfile::srgb(),
        metadata,
        Resolution::default(),
        vec![outer],
    )
    .unwrap()
}

unsafe fn json_result(pointer: *mut std::ffi::c_char) -> Value {
    assert!(!pointer.is_null());
    let value = unsafe { serde_json::from_slice(CStr::from_ptr(pointer).to_bytes()).unwrap() };
    unsafe { dr_string_free(pointer) };
    value
}

#[test]
fn inspection_exposes_postorder_hierarchy_bounds_and_bounded_metadata() {
    let source = source();
    let mut error = ptr::null_mut();
    let info = unsafe { json_result(dr_precision_doc_info(&source, &mut error)) };
    assert!(error.is_null());
    let layers = info["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3);
    for (index, depth) in [2, 1, 0].into_iter().enumerate() {
        assert_eq!(layers[index]["index"], index);
        assert_eq!(layers[index]["depth"], depth);
        assert_eq!(layers[index]["subtree_start"], 0);
        assert_eq!(layers[index]["bounds"], json!([-3, 4, 2, 1]));
    }
    assert_eq!(info["metadata_bytes"]["xmp"], 16);
    let descriptor = unsafe { json_result(dr_precision_doc_descriptor(&source, &mut error)) };
    assert!(descriptor["layers"].is_null());
    assert!(descriptor["inspection_bytes"].as_u64().unwrap() > info.to_string().len() as u64);
    assert!(descriptor["inspection_bytes"].as_u64().unwrap() < 2 * 1024 * 1024);
}

#[test]
fn borrowed_metadata_and_owned_canonical_adjustment_preserve_source() {
    let source = source();
    let description =
        json!({"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}}).to_string();
    let source = source
        .add_adjustment(
            "Invert",
            &description,
            1.0,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let mut length = 999;
    unsafe {
        let profile = dr_precision_doc_profile(&source, &mut length);
        assert_eq!(
            std::slice::from_raw_parts(profile, length),
            source.profile().bytes().as_ref()
        );
        let metadata = dr_precision_doc_metadata(&source, 1, &mut length);
        assert_eq!(
            std::slice::from_raw_parts(metadata, length),
            b"bounded metadata"
        );
        assert!(dr_precision_doc_metadata(&source, 0, &mut length).is_null());
        assert_eq!(length, 0);
        let mut error = ptr::null_mut();
        assert!(dr_precision_doc_adjustment_description(&source, 0, &mut error).is_null());
        assert!(error.is_null());
        let actual = json_result(dr_precision_doc_adjustment_description(
            &source, 3, &mut error,
        ));
        assert!(error.is_null());
        assert_eq!(actual["op"], "invert");
        assert_eq!(actual["evaluation"], "precision_v1");
        assert_eq!(
            source.layer_source(0).unwrap().to_rgba16().ok().unwrap()[1],
            [10002, 20003, 30004, 1]
        );
    }
}

#[test]
fn descriptor_reports_render_output_and_display_scratch_at_both_depths() {
    let mut error = ptr::null_mut();
    let source = source();
    let descriptor = unsafe { json_result(dr_precision_doc_descriptor(&source, &mut error)) };
    assert!(error.is_null());
    let output = descriptor["render_output_bytes"].as_u64().unwrap();
    assert_eq!(output, 16);
    assert_eq!(descriptor["display_scratch_bytes"], 8 + 1_048_576);
    assert!(descriptor["render_scratch_bytes"].as_u64().unwrap() >= output);
    assert_display_overhead(&descriptor, 1);

    let float = PrecisionDocument::open_preserving(
        "tests/fixtures/float_11f/scene.tiff",
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap();
    let descriptor = unsafe { json_result(dr_precision_doc_descriptor(&float, &mut error)) };
    assert!(error.is_null());
    assert_eq!(descriptor["width"], 48);
    assert_eq!(descriptor["height"], 32);
    assert_eq!(descriptor["render_output_bytes"], 48 * 32 * 16);
    assert_eq!(
        descriptor["display_scratch_bytes"],
        48 * 32 * 12 + 1_048_576
    );
    assert_display_overhead(&descriptor, 1);

    // 600×300 spans 3×2 tiles: 3 + 2 − 1 = 4 clipped edge tiles.
    let image =
        PrecisionImage::from_rgba16(600, 300, vec![[0; 4]; 600 * 300], IccProfile::srgb(), None)
            .unwrap();
    let layer = Layer::raster("Wide", image, (0, 0), 1.0, true).unwrap();
    let wide = PrecisionDocument::from_parts(
        600,
        300,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    let descriptor = unsafe { json_result(dr_precision_doc_descriptor(&wide, &mut error)) };
    assert!(error.is_null());
    assert_display_overhead(&descriptor, 4);
}

/// `display_overhead_bytes` = the display preparation's peak / 2048 rounded
/// up (page tables) plus one 32 KiB slack per clipped edge tile.
fn assert_display_overhead(descriptor: &Value, edge_tiles: u64) {
    let field = |key: &str| descriptor[key].as_u64().unwrap();
    let staged = field("render_output_bytes") + field("display_scratch_bytes");
    let peak = field("render_scratch_bytes").max(staged);
    assert_eq!(
        field("display_overhead_bytes"),
        peak.div_ceil(2048) + edge_tiles * 32 * 1024
    );
}

#[test]
fn new_inspection_exports_are_null_safe_and_invalid_kinds_are_absent() {
    let source = source();
    let mut length = 999;
    unsafe {
        assert!(dr_precision_doc_profile(ptr::null(), &mut length).is_null());
        assert_eq!(length, 0);
        assert!(dr_precision_doc_profile(&source, ptr::null_mut()).is_null());
        assert!(dr_precision_doc_metadata(ptr::null(), 1, &mut length).is_null());
        assert_eq!(length, 0);
        assert!(dr_precision_doc_metadata(&source, u32::MAX, &mut length).is_null());
        assert_eq!(length, 0);
        assert!(dr_precision_doc_metadata(&source, 1, ptr::null_mut()).is_null());
        let mut error = ptr::null_mut();
        assert!(dr_precision_doc_adjustment_description(ptr::null(), 0, &mut error).is_null());
        assert!(!error.is_null());
        dr_string_free(error);
        assert!(dr_precision_doc_adjustment_description(ptr::null(), 0, ptr::null_mut()).is_null());
    }
}
