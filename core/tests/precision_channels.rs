//! Independent native channel metadata, auxiliary coverage and framing checks.
use doctored_core::{
    ffi::dr_string_free,
    ffi_precision::{dr_precision_doc_info, dr_precision_doc_open, dr_precision_doc_save_native},
    ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan},
    icc::IccProfile,
    job_control::DrJobControl,
    metadata::{Metadata, Resolution},
    precision_document::PrecisionDocument,
    precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES},
    retained_source::SourceEdit,
};
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    ptr,
};

use crate::identity;

/// Every native encode below goes through here. Phase 12E-1 made the
/// always-written identity wrapper the OUTERMOST `.rz` word and its `LIDS`
/// table the file's last block; this file's subject is an EARLIER wrapper,
/// so the outer one is asserted and then peeled, leaving byte-for-byte what
/// the writer these tests were written against produced.
fn encoded(doc: &PrecisionDocument) -> Vec<u8> {
    identity::typed_prior_body(&doc.encode_native(LIMIT, 0, None).unwrap())
}

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
fn document(width: u32) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            width,
            1,
            vec![[1001, 2002, 3003, 1]; width as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
fn provenance(width: u32, format: &str, bits: u32) -> String {
    json!({"version":1,"kind":"camera_auxiliary","name":"Portrait Matte",
        "source_format":format,"source_bits":bits,"source_width":width,"source_height":1,
        "precision_basis":"decoded_auxiliary_buffer","encoded_precision":"not_inspected",
        "exif_orientation":6,"orientation_applied":true,"coverage_mapping":"identity_unorm",
        "alignment":"oriented_full_image_bilinear"})
    .to_string()
}
fn edit(
    doc: &PrecisionDocument,
    request: Value,
    samples: &[u16],
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let text = CString::new(request.to_string()).unwrap();
    unsafe {
        let mut err = ptr::null_mut();
        let out = dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        if !err.is_null() {
            let message = CStr::from_ptr(err).to_string_lossy().into_owned();
            dr_string_free(err);
            assert!(out.is_null());
            return Err(message);
        }
        Ok((!out.is_null()).then(|| Box::from_raw(out)))
    }
}
fn added(doc: &PrecisionDocument, samples: &[u16]) -> Box<PrecisionDocument> {
    edit(
        doc,
        json!({"op":"import_channel","name":"Coverage","width":samples.len(),"height":1,
        "provenance":provenance(samples.len() as u32,"one_component16",16)}),
        samples,
    )
    .unwrap()
    .unwrap()
}
fn version(doc: &PrecisionDocument) -> u32 {
    let bytes = encoded(doc);
    u32::from_le_bytes(bytes[4..8].try_into().unwrap())
}

#[test]
fn channel_management_keeps_identity_metadata_and_exact_storage() {
    let source = document(6);
    let codes = [0, 1, 257, 32769, 65534, 65535];
    let imported = added(&source, &codes);
    let id = imported.channels()[0].id();
    let request = json!({"op":"channel_properties","index":0,"overlay_color":[17,83,149],
        "overlay_opacity":0.375,"color_indicates_selected":true,"name":"Matte"});
    let styled = edit(&imported, request.clone(), &[]).unwrap().unwrap();
    assert_eq!(styled.channels()[0].id(), id);
    assert!(edit(&styled, request, &[]).unwrap().is_none());
    let renamed = edit(
        &styled,
        json!({"op":"rename_channel","index":0,"name":"Fine"}),
        &[],
    )
    .unwrap()
    .unwrap();
    let duplicate = edit(&renamed, json!({"op":"duplicate_channel","index":0}), &[])
        .unwrap()
        .unwrap();
    assert_eq!(duplicate.channels()[0].id(), id);
    assert_ne!(duplicate.channels()[1].id(), id);
    assert_eq!(duplicate.channels()[1].name(), "Fine copy");
    assert_eq!(
        duplicate.channels()[1].metadata(),
        styled.channels()[0].metadata()
    );
    let replaced = edit(
        &duplicate,
        json!({"op":"set_channel","index":0,"name":"Replaced"}),
        &[2; 6],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        replaced.channels()[0].metadata(),
        styled.channels()[0].metadata()
    );
    assert_eq!(replaced.channels()[0].id(), id);
    assert_eq!(replaced.channels()[1].plane().samples(), codes);
    assert_eq!(source.channels().len(), 0);
    assert_eq!(
        styled.layer_source(0).unwrap().to_rgba16().ok(),
        source.layer_source(0).unwrap().to_rgba16().ok()
    );
    unsafe {
        let mut err = ptr::null_mut();
        let raw = dr_precision_doc_info(&*replaced, &mut err);
        assert!(err.is_null() && !raw.is_null());
        let info: Value = serde_json::from_str(CStr::from_ptr(raw).to_str().unwrap()).unwrap();
        dr_string_free(raw);
        assert_eq!(info["channels"][0]["overlay_color"], json!([17, 83, 149]));
        assert_eq!(info["channels"][0]["overlay_opacity"], 0.375);
        assert_eq!(info["channels"][0]["color_indicates_selected"], true);
        assert!(info["channels"][0]["provenance"]
            .as_str()
            .unwrap()
            .contains("one_component16"));
    }
}

#[test]
fn auxiliary_bilinear_alignment_and_declared_byte_precision_are_independent() {
    let source = document(5);
    let imported = added(&source, &[1, 65535]);
    // Pixel centres map (x+.5)*2/5-.5 to the two original samples. The
    // endpoint extension is constant, with one terminal native quantization.
    let expected: Vec<_> = (0..5)
        .map(|x| {
            let t = ((f64::from(x) + 0.5) * 2.0 / 5.0 - 0.5).clamp(0.0, 1.0);
            (1.0 + (65535.0 - 1.0) * t).round() as u16
        })
        .collect();
    assert_eq!(imported.channels()[0].plane().samples(), expected);
    let byte = json!({"op":"import_channel","name":"Byte matte","width":2,"height":1,
        "provenance":provenance(2,"one_component8",8)});
    assert!(edit(&source, byte.clone(), &[1, 65535])
        .err()
        .unwrap()
        .contains("multiplication"));
    let byte = edit(&source, byte, &[257, 65535]).unwrap().unwrap();
    assert!(byte.channels()[0]
        .metadata()
        .provenance
        .as_ref()
        .unwrap()
        .contains("\"source_bits\":8"));
    assert!(edit(
        &source,
        json!({"op":"import_channel","name":"Mismatch","width":2,"height":1,
        "provenance":provenance(3,"one_component16",16)}),
        &[1, 2]
    )
    .is_err());
}

#[test]
fn native25_roundtrips_channels_with_and_without_retained_sources() {
    for retained in [false, true] {
        let original = document(6);
        let source = if retained {
            original
                .source_edit(
                    None,
                    &SourceEdit::Retain {
                        source_id: "channel-source".into(),
                        revision_id: None,
                    },
                    LIMIT,
                    0,
                    None,
                )
                .unwrap()
                .unwrap()
        } else {
            original
        };
        let old_version = version(&source);
        assert_ne!(old_version, 25);
        let imported = added(&source, &[0, 1, 255, 257, 32769, 65535]);
        let bytes = encoded(&imported);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 25);
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            old_version
        );
        let plan = imported.native_plan().unwrap();
        assert!(bytes.len() as u64 <= plan.encoded_bound);
        let directory = tempfile::tempdir().unwrap();
        let path = CString::new(directory.path().join("channels.rz").to_str().unwrap()).unwrap();
        unsafe {
            let mut err = ptr::null_mut();
            assert!(dr_precision_doc_save_native(
                &*imported,
                path.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err
            ));
            assert!(err.is_null());
            let raw = dr_precision_doc_open(path.as_ptr(), LIMIT, 0, ptr::null(), &mut err);
            assert!(err.is_null() && !raw.is_null());
            let reopened = Box::from_raw(raw);
            assert_eq!(
                reopened.channels()[0].plane().samples(),
                imported.channels()[0].plane().samples()
            );
            assert_eq!(
                reopened.channels()[0].metadata(),
                imported.channels()[0].metadata()
            );
            assert_ne!(reopened.channels()[0].id(), imported.channels()[0].id());
            assert_eq!(
                reopened.layer_source(0).unwrap().to_rgba16().ok(),
                source.layer_source(0).unwrap().to_rgba16().ok()
            );
            assert_eq!(reopened.has_retained_source(0), retained);
            assert_eq!(encoded(&reopened), bytes);
        }
        let legacy = PrecisionDocument::parse_native(&encoded(&source), LIMIT, 0, None).unwrap();
        assert_eq!(legacy.has_retained_source(0), retained);
    }
}

#[test]
fn metadata_native_preflight_rejects_invalid_framing_and_values() {
    let imported = added(&document(2), &[1, 65535]);
    let bytes = encoded(&imported);
    let at = bytes.windows(4).position(|v| v == b"CHPR").unwrap();
    let mut cases = Vec::new();
    let mut bad = bytes.clone();
    bad[8..12].copy_from_slice(&99u32.to_le_bytes());
    cases.push(bad);
    let mut bad = bytes.clone();
    bad[at + 4..at + 8].copy_from_slice(&2u32.to_le_bytes());
    cases.push(bad);
    let mut bad = bytes.clone();
    bad[at + 11..at + 15].copy_from_slice(&f32::NAN.to_le_bytes());
    cases.push(bad);
    let mut bad = bytes.clone();
    bad[at + 15] = 2;
    cases.push(bad);
    let mut bad = bytes.clone();
    bad[at + 16..at + 20].copy_from_slice(&65536u32.to_le_bytes());
    cases.push(bad);
    let mut bad = bytes.clone();
    bad.truncate(at);
    cases.push(bad);
    let mut bad = bytes.clone();
    bad.push(0);
    cases.push(bad);
    for bad in cases {
        assert!(PrecisionDocument::parse_native(&bad, LIMIT, 0, None).is_err());
    }
    assert_eq!(imported.channels()[0].plane().samples(), &[1, 65535]);
}

#[test]
fn strict_requests_noops_and_cancelled_jobs_preserve_inputs() {
    let source = document(2);
    let imported = added(&source, &[1, 65535]);
    for request in [
        json!({"op":"rename_channel","index":9,"name":"x"}),
        json!({"op":"duplicate_channel","index":9}),
        json!({"op":"channel_properties","index":0,"overlay_opacity":1.01}),
        json!({"op":"channel_properties","index":0,"unknown":1}),
        json!({"op":"import_channel","name":"x","width":0,"height":1}),
    ] {
        assert!(edit(&imported, request, &[]).is_err());
    }
    assert!(edit(
        &imported,
        json!({"op":"rename_channel","index":0,"name":"Coverage"}),
        &[]
    )
    .unwrap()
    .is_none());
    unsafe {
        let text = CString::new(json!({"op":"duplicate_channel","index":0}).to_string()).unwrap();
        let mut err = ptr::null_mut();
        let plan = dr_precision_doc_edit_plan(&*imported, text.as_ptr(), 1, 0, &mut err);
        assert!(plan.is_null() && !err.is_null());
        dr_string_free(err);
        let control = DrJobControl::default();
        control.cancel();
        err = ptr::null_mut();
        let out = dr_precision_doc_edit(
            &*imported,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            &control,
            &mut err,
        );
        assert!(out.is_null() && !err.is_null());
        dr_string_free(err);
    }
    assert_eq!(imported.channels()[0].plane().samples(), &[1, 65535]);
}
