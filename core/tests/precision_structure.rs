use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

fn fixture() -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[10001, 20002, 30003, 1], [10002, 20003, 30004, 65535]],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let raster = Layer::raster("Source", image, (0, 0), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Parent", vec![raster], 0.5, true).unwrap();
    PrecisionDocument::from_parts(
        2,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![group],
    )
    .unwrap()
}
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
    limit: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            request.as_ptr(),
            ptr::null(),
            0,
            limit,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    Ok((!result.is_null()).then(|| unsafe { Box::from_raw(result) }))
}

#[test]
fn nested_properties_share_every_source_code_and_roundtrip_native() {
    let source = fixture();
    let original = source.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    let renamed = edit(
        &source,
        json!({"op":"rename_layer","layer":0,"name":"Fine16 Ω"}),
        REFERENCE_WORKING_BYTES,
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(source.layer(0).unwrap().name(), "Source");
    assert_eq!(renamed.layer(0).unwrap().name(), "Fine16 Ω");
    assert_eq!(renamed.layer(1).unwrap().name(), "Parent");
    assert!(PrecisionImage::ptr_eq(
        renamed.layer_source(0).unwrap(),
        source.layer_source(0).unwrap()
    ));
    let hidden = edit(
        &renamed,
        json!({"op":"layer_visible","layer":1,"visible":false}),
        REFERENCE_WORKING_BYTES,
        None,
    )
    .unwrap()
    .unwrap();
    assert!(source.layer(1).unwrap().visible());
    assert!(!hidden.layer(1).unwrap().visible());
    assert_eq!(
        hidden.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        original
    );
    assert!(hidden
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .all(|p| *p == [0; 4]));
    let native = hidden
        .encode_native(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    let reopened =
        PrecisionDocument::parse_native(&native, REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert_eq!(reopened.layer(0).unwrap().name(), "Fine16 Ω");
    assert!(!reopened.layer(1).unwrap().visible());
    assert_eq!(
        reopened.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        original
    );
}

#[test]
fn strict_schema_target_and_exact_noop_contracts() {
    let source = fixture();
    for request in [
        json!({"op":"rename_layer","layer":0,"name":"Source"}),
        json!({"op":"layer_visible","layer":1,"visible":true}),
    ] {
        assert!(edit(&source, request, REFERENCE_WORKING_BYTES, None)
            .unwrap()
            .is_none());
    }
    for request in [
        json!({"op":"rename_layer","layer":2,"name":"Bad"}),
        json!({"op":"rename_layer","layer":0,"name":"Bad","unknown":1}),
        json!({"op":"layer_visible","layer":0,"visible":"false"}),
        json!({"op":"rename_layer","layer":0,"name":"x".repeat(65536)}),
    ] {
        assert!(edit(&source, request, REFERENCE_WORKING_BYTES, None).is_err());
    }
    assert_eq!(source.layer(0).unwrap().name(), "Source");
    assert!(source.layer(0).unwrap().visible());
}

#[test]
fn plan_admission_and_precancellation_publish_nothing() {
    let source = fixture();
    let request = json!({"op":"rename_layer","layer":0,"name":"Changed"});
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_edit_plan(
            &source,
            text.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            &mut error,
        )
    };
    assert!(take_error(error).is_none());
    assert!(!output.is_null());
    let plan: Value = unsafe { serde_json::from_slice(CStr::from_ptr(output).to_bytes()).unwrap() };
    unsafe { dr_string_free(output) };
    assert_eq!(plan["input_samples"], 0);
    let bound = plan["retained_bytes"].as_u64().unwrap() + plan["scratch_bytes"].as_u64().unwrap();
    assert!(edit(&source, request.clone(), bound - 1, None).is_err());
    let control = DrJobControl::default();
    control.cancel();
    assert!(edit(&source, request, REFERENCE_WORKING_BYTES, Some(&control)).is_err());
    assert_eq!(control.scratch_bytes(), 0);
    assert_eq!(source.layer(0).unwrap().name(), "Source");
}

#[test]
fn every_structure_op_is_null_safe_and_named_on_a_missing_document() {
    // The five 12E-1 structure ops join the four property ops on the same
    // sweep: a NULL handle must report through `err_out` rather than reach a
    // core method, and a request this build does not know must name itself.
    for request in [
        json!({"op":"move_layer_to","from":0,"to":0,"depth":0}),
        json!({"op":"arrange_layer","layer":0,"how":"front"}),
        json!({"op":"group_layers","layers":[0],"name":"G"}),
        json!({"op":"ungroup_layer","layer":0}),
        json!({"op":"remove_layers","layers":[0]}),
        json!({"op":"rename_layer","layer":0,"name":"N"}),
        json!({"op":"layer_visible","layer":0,"visible":false}),
        json!({"op":"layer_clipped","layer":0,"clipped":true}),
        json!({"op":"layer_label","layer":0,"label":1}),
    ] {
        let text = CString::new(request.to_string()).unwrap();
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit(
                ptr::null(),
                text.as_ptr(),
                ptr::null(),
                0,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(out.is_null(), "{request} on a NULL document");
        assert!(take_error(error).is_some(), "{request} reports a message");
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit_plan(
                ptr::null(),
                text.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                &mut error,
            )
        };
        assert!(out.is_null());
        assert!(take_error(error).is_some());
        // And an unknown field on each is refused rather than dropped.
        let mut bad = request.clone();
        bad["surprise"] = json!(1);
        let text = CString::new(bad.to_string()).unwrap();
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit_plan(
                &fixture(),
                text.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                &mut error,
            )
        };
        assert!(out.is_null(), "{bad} must be refused");
        assert!(
            take_error(error).is_some_and(|message| message.contains("unknown field")),
            "{bad}"
        );
    }
}
