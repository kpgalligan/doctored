//! A 32-bit float document's TRIMMED per-layer export (File ▸ Export
//! Layers…, MCP `export_layers`) is supported. The trim scans the float
//! render's alpha (`alpha_bounds32`, polling the export's job once per
//! 256-row band), crops ONE `read_rect_rgba32`, and the cropped samples go
//! through the same export chain a canvas-framed item takes — exact at a
//! 32-bit TIFF, tone-mapped at 16 bits.
//!
//! Black-box through `dr_precision_doc_export_layer_encode` / `_plan`; the
//! fixtures are float TIFFs opened with `open_preserving`, and every oracle
//! is a literal.
use crate::common;
use common::probe::entry_count;
use common::take_err_string;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_export_result_info,
    dr_precision_doc_export_layer_encode, dr_precision_doc_export_layer_plan,
};
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::tiff_float::{self, FloatTiff};

use serde_json::Value;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const TIFF32: &str = r#"{"format":"tiff","bit_depth":32}"#;
const PNG16: &str = r#"{"format":"png","bit_depth":16}"#;
const EMPTY: &str = r#"layer "Clear" renders no visible pixels; nothing to trim"#;

/// The four visible pixels of the 6×5 fixture, row by row.
const VISIBLE: [((u32, u32), [f32; 4]); 4] = [
    ((3, 1), [8.0, -0.25, 1.0, 1.0]),
    ((4, 1), [0.0, 1.0, 0.0, 1.0]),
    ((3, 2), [1.0, 1.0, 1.0, 0.25]),
    ((4, 2), [0.0, 0.0, 1.0, 0.5]),
];

/// A float document holding one raster of `samples`, through a float TIFF
/// opened with `open_preserving`.
fn float_document(width: u32, height: u32, samples: Vec<f32>) -> PrecisionDocument {
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
    let bytes = tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("layer.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(
        path.to_str().unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// 6×5, transparent zeros except the four `VISIBLE` pixels.
fn fixture() -> PrecisionDocument {
    let mut samples = vec![0.0f32; 6 * 5 * 4];
    for ((x, y), pixel) in VISIBLE {
        let at = ((y * 6 + x) * 4) as usize;
        samples[at..at + 4].copy_from_slice(&pixel);
    }
    float_document(6, 5, samples)
}

/// `doc` with a transparent "Clear" on top (postorder index 1).
fn with_clear(doc: &PrecisionDocument) -> PrecisionDocument {
    doc.add_layer_at("Clear", 0, InsertPosition::Top)
        .expect("a layer")
        .expect("a change")
}

/// One encode of item `idx`: the bytes and the info JSON, or the core's
/// sentence.
fn export(
    doc: &PrecisionDocument,
    idx: usize,
    trimmed: bool,
    request: &str,
    control: *const DrJobControl,
) -> Result<(Vec<u8>, Value), String> {
    let request = CString::new(request).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result: *mut ExportResult = unsafe {
        dr_precision_doc_export_layer_encode(
            doc,
            idx,
            trimmed,
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            control,
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_export_result_info(result, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
    unsafe { dr_string_free(json) };
    unsafe { dr_export_result_free(result) };
    Ok((bytes, serde_json::from_str(&text).unwrap()))
}

fn plan_scratch(doc: &PrecisionDocument, trimmed: bool) -> u64 {
    let request = CString::new(TIFF32).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let json =
        unsafe { dr_precision_doc_export_layer_plan(doc, 0, trimmed, request.as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
    unsafe { dr_string_free(json) };
    let plan: Value = serde_json::from_str(&text).unwrap();
    plan["scratch_bytes"].as_u64().expect("scratch_bytes")
}

#[test]
fn a_trimmed_32_bit_tiff_is_the_exact_box() {
    let (bytes, info) = export(&fixture(), 0, true, TIFF32, ptr::null()).unwrap();
    assert_eq!(info["layer"]["origin"], serde_json::json!([3, 1]));
    assert_eq!(info["layer"]["size"], serde_json::json!([2, 2]));
    assert_eq!(
        (info["width"].as_u64(), info["height"].as_u64()),
        (Some(2), Some(2))
    );
    assert_eq!(info["bit_depth"], 32);
    let decoded =
        tiff_float::decode(&bytes, "trimmed", REFERENCE_WORKING_BYTES).expect("float TIFF");
    assert_eq!((decoded.width, decoded.height), (2, 2));
    let got: Vec<u32> = decoded.samples.iter().map(|v| v.to_bits()).collect();
    let wanted: Vec<u32> = VISIBLE
        .iter()
        .flat_map(|(_, pixel)| pixel.map(f32::to_bits))
        .collect();
    assert_eq!(got, wanted, "the box's samples, bitwise");
}

#[test]
fn a_trimmed_16_bit_png_is_tone_mapped() {
    let (bytes, info) = export(&fixture(), 0, true, PNG16, ptr::null()).unwrap();
    assert_eq!(info["layer"]["origin"], serde_json::json!([3, 1]));
    let decoded = image::load_from_memory(&bytes).expect("PNG").to_rgba16();
    assert_eq!(decoded.dimensions(), (2, 2));
    let got: Vec<[u16; 4]> = decoded.pixels().map(|p| p.0).collect();
    assert_eq!(
        got,
        vec![
            [65535, 0, 65535, 65535],
            [0, 65535, 0, 65535],
            [65535, 65535, 65535, 16384],
            [0, 0, 65535, 32768],
        ]
    );
}

#[test]
fn a_fully_transparent_float_item_refuses_trimmed() {
    let doc = with_clear(&fixture());
    assert_eq!(
        export(&doc, 1, true, TIFF32, ptr::null()).unwrap_err(),
        EMPTY
    );
}

#[test]
fn the_trim_charges_one_float_canvas() {
    let doc = fixture();
    assert_eq!(
        plan_scratch(&doc, true) - plan_scratch(&doc, false),
        6 * 5 * 16
    );
}

/// A 300×2000 opaque float base under "Clear": the trimmed export of
/// "Clear" scans a 2000-row render — 8 tile bands — and refuses after it.
fn tall() -> PrecisionDocument {
    let base = [0.5f32, 0.25, 0.125, 1.0];
    let samples = base.iter().copied().cycle().take(300 * 2000 * 4).collect();
    with_clear(&float_document(300, 2000, samples))
}

#[test]
fn the_float_trim_scan_polls_once_per_tile_band() {
    let r5 = entry_count(&|p, s, _| s == 5 && p == 1.0, |token| {
        match export(&tall(), 1, false, TIFF32, token as *const DrJobControl) {
            Ok(_) => (true, String::new()),
            Err(e) => (false, e),
        }
    });
    let control = DrJobControl::default();
    assert_eq!(
        export(&tall(), 1, true, TIFF32, &control).unwrap_err(),
        EMPTY
    );
    // One check after the render, then one per 256-row band of 2000 rows;
    // the refusal follows the scan with no further checkpoint.
    assert_eq!(control.checkpoints(), r5 + 1 + 8);
}

/// A 1000×4 float canvas whose one item is an opaque 4×4 patch: trimmed, a
/// width of 1000 makes the 4×4 cut a 1000×1000 deliverable, far past the
/// canvas-based plan. The export re-admits with the cut's own plan.
#[test]
fn a_trimmed_resample_re_admits_with_the_cut_plan() {
    let (w, h) = (1000u32, 4u32);
    let mut samples = vec![0.0f32; (w * h * 4) as usize];
    for y in 0..4 {
        for x in 100..104 {
            let at = ((y * w + x) * 4) as usize;
            samples[at..at + 4].copy_from_slice(&[2.5, 0.5, -0.25, 1.0]);
        }
    }
    let doc = float_document(w, h, samples);
    let wide = doctored_core::doc_export::ExportRequest::parse(
        r#"{"format":"tiff","bit_depth":32,"size":{"mode":"width","value":1000}}"#,
    )
    .unwrap();
    let exact = doctored_core::doc_export::ExportRequest::parse(TIFF32).unwrap();
    let tight = |trimmed: bool, request: &doctored_core::doc_export::ExportRequest| {
        let plan = doc.export_layer_plan(0, trimmed, request).unwrap();
        plan["retained_bytes"].as_u64().unwrap()
            + plan["scratch_bytes"].as_u64().unwrap()
            + (1 << 20)
    };
    let error = doc
        .export_layer_encoded(0, true, &wide, tight(true, &wide), 0, None)
        .err()
        .expect("the cut's 1000×1000 output does not fit the canvas plan");
    assert!(error.contains("working limit"), "{error}");
    assert!(error.contains("trimmed to 4 × 4 and resized to"), "{error}");
    let result = doc
        .export_layer_encoded(0, true, &wide, REFERENCE_WORKING_BYTES, 0, None)
        .expect("the full limit admits it");
    assert_eq!(
        (
            result.info_json()["width"].as_u64(),
            result.info_json()["height"].as_u64()
        ),
        (Some(1000), Some(1000))
    );
    doc.export_layer_encoded(0, false, &wide, tight(false, &wide), 0, None)
        .expect("untrimmed, the canvas plan is the plan");
    doc.export_layer_encoded(0, true, &exact, tight(true, &exact), 0, None)
        .expect("trimmed with size none, the cut is smaller than the canvas");
}
