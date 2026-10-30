//! Work admission follows the executed source sampler, separately from linked
//! masks. These are metadata-plan tests, not large rendering/codec qualification.
use doctored_core::doc::DrDocument;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_retained_source::{dr_doc_mixed_source_plan, dr_precision_doc_source_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::retained_source::SourceEdit;
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

fn byte(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([31, 79, 131, 251])))
}
fn typed(w: u32, h: u32) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![[1001, 3001, 7001, 65531]; w as usize * h as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
fn place() -> SourceEdit {
    SourceEdit::parse(r#"{"op":"place","source_id":"work-fixture","name":"Original","x":0,"y":0}"#)
        .unwrap()
}
fn transform(x: f64, y: f64, filter: &str) -> SourceEdit {
    SourceEdit::parse(
        &json!({"op":"transform","layer":1,"matrix":[x,0,0,y,0,0],"filter":filter}).to_string(),
    )
    .unwrap()
}
/// A ⌘T PREVIEW's source request.
fn interactive_transform(x: f64, y: f64, filter: &str) -> SourceEdit {
    SourceEdit::parse(
        &json!({"op":"transform","layer":1,"matrix":[x,0,0,y,0,0],"filter":filter,
            "interactive":true})
        .to_string(),
    )
    .unwrap()
}
unsafe fn text(value: *mut c_char) -> String {
    if value.is_null() {
        return String::new();
    }
    let string = unsafe { CStr::from_ptr(value) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(value) };
    string
}
fn plan_json(value: *mut c_char, error: *mut c_char) -> Value {
    let error = unsafe { text(error) };
    assert!(error.is_empty(), "{error}");
    assert!(!value.is_null());
    serde_json::from_str(&unsafe { text(value) }).unwrap()
}
fn assert_photo_plan(plan: &Value) {
    // Placement shares samples. The planner conservatively retains one source
    // color visit but must not charge the unused default 6×6 Lanczos kernel.
    assert_eq!(plan["work_units"], 600_u64 * 400 * 4);
}

#[test]
fn public_plans_charge_identity_placement_by_its_source_visits_at_both_depths() {
    let request =
        CString::new(r#"{"op":"place","source_id":"photo","name":"Photo","x":0,"y":0}"#).unwrap();
    // No retained edit, projection, transform output, or encoded file is created.
    {
        let source = byte(600, 400);
        let destination = byte(1, 1);
        let mut error = ptr::null_mut();
        let value = unsafe {
            dr_doc_mixed_source_plan(
                &destination,
                &source,
                ptr::null(),
                request.as_ptr(),
                &mut error,
            )
        };
        assert_photo_plan(&plan_json(value, error));
    }
    {
        let source = typed(600, 400);
        let destination = typed(1, 1);
        let mut error = ptr::null_mut();
        let value = unsafe {
            dr_precision_doc_source_plan(&destination, &source, request.as_ptr(), &mut error)
        };
        assert_photo_plan(&plan_json(value, error));
    }
}

#[test]
fn filtered_24mp_destination_keeps_original_work_refusal_without_allocating_output() {
    let source = byte(4, 4);
    let placed = source
        .source_edit(Some(&source), &place(), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    // The byte ⌘T PREVIEW keeps the refusal; the byte COMMIT is a
    // one-shot job bounded by `MAX_DESTRUCTIVE_WORK`, as the typed one is.
    let filtered = placed.source_plan(None, &interactive_transform(1500., 1000., "lanczos3"));
    assert!(filtered
        .err()
        .unwrap()
        .contains("bounded sampling work limit"));
    let commit = placed
        .source_plan(None, &transform(1500., 1000., "lanczos3"))
        .unwrap();
    assert_eq!(commit.work_units, 24_000_000_u64 * 4 * (6 * 6 + 2) + 16 * 4);
    assert_eq!(
        placed
            .source_plan(None, &transform(1500., 1000., "nearest"))
            .unwrap()
            .work_units,
        24_000_000_u64 * 4 * (1 + 2) + 16 * 4
    );
    let source = typed(4, 4);
    let placed = source
        .source_edit(Some(&source), &place(), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    // The typed ⌘T PREVIEW keeps the refusal; the typed COMMIT is a
    // one-shot job bounded by `MAX_DESTRUCTIVE_WORK`, planned without
    // allocating its output.
    let filtered = placed.source_plan(None, &interactive_transform(1500., 1000., "lanczos3"));
    assert!(filtered
        .err()
        .unwrap()
        .contains("bounded sampling work limit"));
    let commit = placed
        .source_plan(None, &transform(1500., 1000., "lanczos3"))
        .unwrap();
    assert_eq!(commit.work_units, 24_000_000_u64 * 4 * (6 * 6 + 2) + 16 * 4);
    assert_eq!(
        placed
            .source_plan(None, &transform(1500., 1000., "nearest"))
            .unwrap()
            .work_units,
        24_000_000_u64 * 4 * (1 + 2) + 16 * 4
    );
}

#[test]
fn exact_accumulated_source_still_charges_filtered_incremental_mask_sampling() {
    let source = byte(16, 16);
    let placed = source
        .source_edit(Some(&source), &place(), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let mut reduced = placed
        .source_edit(
            None,
            &transform(0.5, 0.5, "lanczos3"),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    reduced.layers[1].mask = Some(Arc::new(GrayImage::from_pixel(10, 10, image::Luma([127]))));
    reduced.layers[1].mask_properties = MaskProperties::at((0, 0));
    // RGBA returns to identity (no sample taps), while the independent mask
    // grows from10×10 to20×20 through the requested6×6 kernel and two visits.
    let expected = 16 * 16 * 4 + 20 * 20 * (6 * 6 + 2);
    assert_eq!(
        reduced
            .source_plan(None, &transform(2., 2., "lanczos3"))
            .unwrap()
            .work_units,
        expected
    );
    let source = typed(16, 16);
    let placed = source
        .source_edit(Some(&source), &place(), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let reduced = placed
        .source_edit(
            None,
            &transform(0.5, 0.5, "lanczos3"),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(10, 10, vec![30001; 100], None).unwrap(),
        MaskProperties::at((0, 0)),
        true,
    )
    .unwrap();
    let reduced = reduced
        .set_layer_mask(1, Some(mask), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        reduced
            .source_plan(None, &transform(2., 2., "lanczos3"))
            .unwrap()
            .work_units,
        expected
    );
}

#[test]
fn identity_placement_scratch_charges_the_tiling_transient_at_both_depths() {
    // An identity placement at matching depth and profile:
    // `profile_cost` answers 0 scratch for equal profile bytes, and no mask is
    // sampled, so between a 256×256 and a 512×256 asset (Δs = Δn = 65536)
    // `scratch_bytes` moves by exactly the input and output planes
    // `(Δs + Δn)·4·depth` plus the tiling transient `Δn·4·depth`: 65536·12
    // at 8 bits (depth 1) and 65536·24 at 16 bits (depth 2).
    let destination = byte(1, 1);
    let scratch = |w: u32, h: u32| {
        destination
            .source_plan(Some(&byte(w, h)), &place())
            .unwrap()
            .scratch_bytes
    };
    assert_eq!(scratch(512, 256) - scratch(256, 256), 65536 * 12);
    let destination = typed(1, 1);
    let scratch = |w: u32, h: u32| {
        destination
            .source_plan(Some(&typed(w, h)), &place())
            .unwrap()
            .scratch_bytes
    };
    assert_eq!(scratch(512, 256) - scratch(256, 256), 65536 * 24);
}
