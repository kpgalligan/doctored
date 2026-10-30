//! RAW storage is decoder-independent: independent u16 code patterns prove that
//! attachment, redevelopment and retained geometry never reduce the master.
use doctored_core::doc::{BlendMode, DrDocument, LayerRaster};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::ffi_raw_source::*;
use doctored_core::ffi_retained_source::{
    dr_doc_has_retained_source, dr_doc_source_info_scratch_bytes,
    dr_precision_doc_has_retained_source, dr_precision_doc_source_info_scratch_bytes,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::raw_source::{RawDescriptor, MAX_RAW_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use crate::resource_report;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
fn description(exposure: f64) -> Value {
    json!({"schema_version":1,"original_filename":"camera.dng","content_sha256":"b".repeat(64),"decoder":{"identifier":"test.decoder","version":"1.0","implementation":"test-platform"},"recipe":{"schema_version":1,"exposure":exposure,"white_balance":[5500.0,0.0],"output_space":"srgb"}})
}
fn c(value: Value) -> CString {
    CString::new(value.to_string()).unwrap()
}
unsafe fn text(value: *mut c_char) -> String {
    if value.is_null() {
        return String::new();
    }
    let out = unsafe { CStr::from_ptr(value) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { dr_string_free(value) };
    out
}
fn parsed(value: Value) -> SourceEdit {
    SourceEdit::parse(&value.to_string()).unwrap()
}
fn samples(seed: u16) -> Vec<u16> {
    (0..24_u16)
        .flat_map(|i| {
            [
                i * 791 + seed,
                i * 223 + seed,
                i * 1117 + seed,
                if i % 4 == 0 { 0 } else { 65535 },
            ]
        })
        .collect()
}
fn typed(seed: u16) -> PrecisionDocument {
    let samples = samples(seed);
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr(),
            samples.len(),
            6,
            4,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", unsafe { text(err) });
    assert!(!out.is_null());
    let out = *unsafe { Box::from_raw(out) };
    assert_eq!(
        out.layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>(),
        samples
    );
    out
}
fn retained16(seed: u16) -> PrecisionDocument {
    typed(seed)
        .source_edit(
            None,
            &parsed(json!({"op":"retain","source_id":"camera","revision_id":"initial"})),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap()
}
fn retained8(seed: u16) -> DrDocument {
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(6, 4, Rgba([0; 4])));
    base.source_edit_input(Some(SourceInput::Typed(&typed(seed))),&parsed(json!({"op":"place","source_id":"camera","revision_id":"initial","name":"Camera","x":0,"y":0,"depth_conversion":"convert_fallback"})),LIMIT,0,None).unwrap().unwrap()
}
fn attach16(doc: &PrecisionDocument, bytes: &[u8], exposure: f64) -> PrecisionDocument {
    let descriptor = c(description(exposure));
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_attach_raw_source(
            doc,
            0,
            bytes.as_ptr(),
            bytes.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", unsafe { text(err) });
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}
fn attach8(doc: &DrDocument, bytes: &[u8], exposure: f64) -> DrDocument {
    let descriptor = c(description(exposure));
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_attach_raw_source(
            doc,
            1,
            bytes.as_ptr(),
            bytes.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", unsafe { text(err) });
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}
#[test]
fn exact_host_intake_and_initial_capture_keep_one_layer_and_original_codes() {
    let original = typed(17);
    let retained = retained16(17);
    let bytes = b"II camera original\0\xff\x11";
    let attached = attach16(&retained, bytes, 0.);
    assert_eq!(attached.layer_count(), 1);
    assert_eq!(
        attached.layer_source(0).unwrap().to_rgba16().ok(),
        original.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert!(retained.source_info(0).unwrap()["raw"].is_null());
    let info = attached.source_info(0).unwrap();
    assert_eq!(info["raw"]["encoded_bytes"], bytes.len());
    assert_eq!(info["raw"]["recipe"], description(0.)["recipe"]);
    let mut output = vec![0; bytes.len()];
    let mut err = ptr::null_mut();
    assert_eq!(
        unsafe { dr_precision_doc_raw_source_length(&attached, 0, &mut err) },
        bytes.len()
    );
    assert!(unsafe {
        dr_precision_doc_raw_source_copy(&attached, 0, output.as_mut_ptr(), output.len(), &mut err)
    });
    assert_eq!(output, bytes);
    assert!(err.is_null());
}
#[test]
fn byte_redevelopment_keeps_master_transform_layer_state_and_retouch_above() {
    let bytes = vec![43; 1024 * 1024];
    let mut old = attach8(&retained8(17), &bytes, 0.);
    old.layers[1].name = "Developed photo".into();
    old.layers[1].opacity = 0.7;
    old.layers[1].blend = BlendMode::Multiply;
    old.layers[1].mask = Some(Arc::new(GrayImage::from_pixel(6, 4, Luma([177]))));
    old = old
        .source_edit(
            None,
            &parsed(
                json!({"op":"transform","layer":1,"matrix":[0.5,0,0,0.5,1,2],"filter":"bilinear"}),
            ),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let top = RgbaImage::from_pixel(6, 4, Rgba([213, 17, 43, 61]));
    let mut top = DrDocument::from_pixels(top).layers.remove(0);
    top.name = "Retouch".into();
    old.layers.push(top);
    let replacement=old.source_edit_input(Some(SourceInput::Typed(&typed(149))),&parsed(json!({"op":"replace","layer":1,"revision_id":"developed","depth_conversion":"convert_fallback"})),LIMIT,0,None).unwrap().unwrap();
    let descriptor = c(description(1.));
    let mut err = ptr::null_mut();
    let plan = unsafe {
        dr_doc_raw_source_from_plan(&replacement, 1, &old, 1, descriptor.as_ptr(), &mut err)
    };
    assert!(err.is_null(), "{}", unsafe { text(err) });
    let plan: Value = serde_json::from_str(&unsafe { text(plan) }).unwrap();
    assert_eq!(plan["work_units"], 0);
    let next = unsafe {
        dr_doc_attach_raw_source_from(
            &replacement,
            1,
            &old,
            1,
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", unsafe { text(err) });
    assert!(!next.is_null());
    let next = *unsafe { Box::from_raw(next) };
    assert_eq!(next.layers.len(), 3);
    assert!(LayerRaster::ptr_eq(
        &next.layers[2].pixels,
        &old.layers[2].pixels
    ));
    assert_eq!(next.layers[1].name, old.layers[1].name);
    assert_eq!(next.layers[1].opacity, old.layers[1].opacity);
    assert_eq!(next.layers[1].blend, old.layers[1].blend);
    assert!(Arc::ptr_eq(
        next.layers[1].mask.as_ref().unwrap(),
        old.layers[1].mask.as_ref().unwrap()
    ));
    let a = old.layers[1].effective_source(&old.profile).unwrap();
    let b = next.layers[1].effective_source(&next.profile).unwrap();
    assert_eq!(a.transform, b.transform);
    assert!(Arc::ptr_eq(
        &a.asset.raw.as_ref().unwrap().encoded,
        &b.asset.raw.as_ref().unwrap().encoded
    ));
    assert_ne!(a.asset.revision_id, b.asset.revision_id);
    assert_eq!(a.asset.id, b.asset.id);
    assert_eq!(
        b.asset
            .rgba16()
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>(),
        samples(149)
    );
    assert_eq!(
        a.asset.raw.as_ref().unwrap().descriptor.recipe["exposure"],
        0.
    );
    let documents = [&old as *const DrDocument, &next as *const DrDocument];
    let report =
        resource_report::report(&documents, &[], &[], &[], 0, LIMIT).expect("a resource report");
    assert_eq!(
        report["categories"]["raw_source_encoded"]["unique_bytes"],
        bytes.len()
    );
    let mut output = vec![0; bytes.len()];
    assert_eq!(
        unsafe { dr_doc_raw_source_length(&next, 1, &mut err) },
        bytes.len()
    );
    assert!(unsafe {
        dr_doc_raw_source_copy(&next, 1, output.as_mut_ptr(), output.len(), &mut err)
    });
    assert_eq!(output, bytes);
}
#[test]
fn typed_redevelopment_shares_original_and_refuses_identity_substitution() {
    let bytes = b"retained original";
    let old = attach16(&retained16(3), bytes, 0.);
    let next = old
        .source_edit(
            Some(&typed(101)),
            &parsed(json!({"op":"replace","layer":0,"revision_id":"second"})),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let descriptor = c(description(2.));
    let mut err = ptr::null_mut();
    let plan = unsafe {
        dr_precision_doc_raw_source_from_plan(&next, 0, &old, 0, descriptor.as_ptr(), &mut err)
    };
    assert!(err.is_null());
    assert!(!plan.is_null());
    unsafe { dr_string_free(plan) };
    let out = unsafe {
        dr_precision_doc_attach_raw_source_from(
            &next,
            0,
            &old,
            0,
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", unsafe { text(err) });
    assert!(!out.is_null());
    let out = *unsafe { Box::from_raw(out) };
    assert_eq!(out.source_info(0).unwrap()["raw"]["recipe"]["exposure"], 2.);
    assert_eq!(
        out.layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>(),
        samples(101)
    );
    for key in ["original_filename", "content_sha256"] {
        let mut wrong = description(1.);
        wrong[key] = json!(if key == "original_filename" {
            "different.dng".into()
        } else {
            "c".repeat(64)
        });
        let wrong = c(wrong);
        let out = unsafe {
            dr_precision_doc_attach_raw_source_from(
                &next,
                0,
                &old,
                0,
                wrong.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(out.is_null());
        assert!(unsafe { text(err) }.contains("preserve"));
        err = ptr::null_mut();
    }
}
#[test]
fn strict_descriptors_lengths_and_atomic_admission_refuse_invalid_inputs() {
    let doc = retained16(7);
    let descriptor = c(description(0.));
    let bytes = [7; 256];
    let mut err = ptr::null_mut();
    let plan = unsafe {
        dr_precision_doc_raw_source_plan(&doc, 0, bytes.len(), descriptor.as_ptr(), &mut err)
    };
    assert!(err.is_null());
    let plan: Value = serde_json::from_str(&unsafe { text(plan) }).unwrap();
    let need = plan["retained_bytes"].as_u64().unwrap() + plan["scratch_bytes"].as_u64().unwrap();
    let token = DrJobControl::default();
    let out = unsafe {
        dr_precision_doc_attach_raw_source(
            &doc,
            0,
            bytes.as_ptr(),
            bytes.len(),
            descriptor.as_ptr(),
            need - 1,
            0,
            &token,
            &mut err,
        )
    };
    assert!(out.is_null());
    assert!(unsafe { text(err) }.contains("working limit"));
    assert_eq!(token.scratch_bytes(), 0);
    err = ptr::null_mut();
    let token = DrJobControl::default();
    token.cancel();
    let out = unsafe {
        dr_precision_doc_attach_raw_source(
            &doc,
            0,
            bytes.as_ptr(),
            bytes.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            &token,
            &mut err,
        )
    };
    assert!(out.is_null());
    assert!(unsafe { text(err) }.contains("cancelled"));
    err = ptr::null_mut();
    for length in [0, MAX_RAW_BYTES as usize + 1] {
        let out = unsafe {
            dr_precision_doc_raw_source_plan(&doc, 0, length, descriptor.as_ptr(), &mut err)
        };
        assert!(out.is_null());
        assert!(unsafe { text(err) }.contains("256 MiB"));
        err = ptr::null_mut();
    }
    let attached = attach16(&doc, &bytes, 0.);
    let mut output = vec![91; bytes.len() + 1];
    assert!(!unsafe {
        dr_precision_doc_raw_source_copy(&attached, 0, output.as_mut_ptr(), output.len(), &mut err)
    });
    assert!(output.iter().all(|v| *v == 91));
    unsafe { text(err) };
    let mut invalid = Vec::new();
    for (key, value) in [
        ("schema_version", json!(2)),
        ("original_filename", json!("../camera.dng")),
        ("content_sha256", json!("B".repeat(64))),
        ("recipe", json!({})),
        ("recipe", json!([])),
        ("extra", json!(true)),
    ] {
        let mut item = description(0.);
        item[key] = value;
        invalid.push(item);
    }
    let mut unknown = description(0.);
    unknown["decoder"]["extra"] = json!(true);
    invalid.push(unknown);
    for item in invalid {
        assert!(RawDescriptor::parse(&item.to_string()).is_err(), "{item}");
    }
    let mut future = description(0.);
    future["decoder"]["identifier"] = json!("future.decoder");
    assert!(RawDescriptor::parse(&future.to_string()).is_ok());
}
#[test]
fn byte_attachment_plan_counts_input_and_refuses_byte_master_or_shared_revision() {
    let descriptor = c(description(0.));
    let mut err = ptr::null_mut();
    let bytes = [1; 42];
    let doc = retained8(3);
    let plan =
        unsafe { dr_doc_raw_source_plan(&doc, 1, bytes.len(), descriptor.as_ptr(), &mut err) };
    assert!(err.is_null());
    assert!(!plan.is_null());
    unsafe { dr_string_free(plan) };
    let mut duplicate = doc.clone();
    duplicate.layers.push(duplicate.layers[1].clone());
    let refused = unsafe {
        dr_doc_attach_raw_source(
            &duplicate,
            1,
            bytes.as_ptr(),
            bytes.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(refused.is_null());
    assert!(unsafe { text(err) }.contains("unshared"));
    err = ptr::null_mut();
    let plain = DrDocument::from_pixels(RgbaImage::from_pixel(6, 4, Rgba([17; 4])));
    let byte = plain
        .source_edit(
            Some(&plain),
            &parsed(json!({"op":"place","source_id":"camera","name":"Camera","x":0,"y":0})),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let refused = unsafe {
        dr_doc_attach_raw_source(
            &byte,
            1,
            bytes.as_ptr(),
            bytes.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(refused.is_null());
    assert!(unsafe { text(err) }.contains("genuine u16"));
}
#[test]
fn host_intake_refuses_lengths_profiles_and_budget_before_copying() {
    let samples = samples(23);
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    for (count, length, limit) in [
        (samples.len() - 1, profile.bytes().len(), LIMIT),
        (samples.len(), profile.bytes().len() + 1, LIMIT),
        (samples.len(), profile.bytes().len(), 1),
    ] {
        let mut padded = profile.bytes().to_vec();
        padded.push(0);
        let out = unsafe {
            dr_precision_doc_from_rgba16(
                samples.as_ptr(),
                count,
                6,
                4,
                padded.as_ptr(),
                length,
                ptr::null(),
                limit,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(out.is_null());
        assert!(!unsafe { text(err) }.is_empty());
        err = ptr::null_mut();
    }
}
#[test]
fn every_new_export_is_null_safe() {
    let descriptor = c(description(0.));
    unsafe {
        assert!(!dr_doc_has_retained_source(ptr::null(), 0));
        assert!(!dr_precision_doc_has_retained_source(ptr::null(), 0));
        assert_eq!(dr_doc_source_info_scratch_bytes(ptr::null(), 0), 64 * 1024);
        assert_eq!(
            dr_precision_doc_source_info_scratch_bytes(ptr::null(), 0),
            64 * 1024
        );
        assert!(dr_precision_doc_from_rgba16(
            ptr::null(),
            0,
            0,
            0,
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        macro_rules! nulls {
            ($plan:ident,$attach:ident,$from_plan:ident,$from:ident,$len:ident,$copy:ident) => {{
                assert!($plan(ptr::null(), 0, 1, descriptor.as_ptr(), ptr::null_mut()).is_null());
                assert!($attach(
                    ptr::null(),
                    0,
                    ptr::null(),
                    0,
                    descriptor.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    ptr::null_mut()
                )
                .is_null());
                assert!($from_plan(
                    ptr::null(),
                    0,
                    ptr::null(),
                    0,
                    descriptor.as_ptr(),
                    ptr::null_mut()
                )
                .is_null());
                assert!($from(
                    ptr::null(),
                    0,
                    ptr::null(),
                    0,
                    descriptor.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    ptr::null_mut()
                )
                .is_null());
                assert_eq!($len(ptr::null(), 0, ptr::null_mut()), 0);
                assert!(!$copy(ptr::null(), 0, ptr::null_mut(), 0, ptr::null_mut()));
            }};
        }
        nulls!(
            dr_doc_raw_source_plan,
            dr_doc_attach_raw_source,
            dr_doc_raw_source_from_plan,
            dr_doc_attach_raw_source_from,
            dr_doc_raw_source_length,
            dr_doc_raw_source_copy
        );
        nulls!(
            dr_precision_doc_raw_source_plan,
            dr_precision_doc_attach_raw_source,
            dr_precision_doc_raw_source_from_plan,
            dr_precision_doc_attach_raw_source_from,
            dr_precision_doc_raw_source_length,
            dr_precision_doc_raw_source_copy
        );
    }
}

#[test]
fn source_info_admission_includes_full_raw_recipe_expansion_at_both_depths() {
    let plain = typed(17);
    assert!(!unsafe { dr_precision_doc_has_retained_source(&plain, 0) });
    assert_eq!(
        unsafe { dr_precision_doc_source_info_scratch_bytes(&plain, 0) },
        64 * 1024
    );
    let low = retained8(17);
    assert_eq!(
        unsafe { dr_doc_source_info_scratch_bytes(&low, 0) },
        64 * 1024
    );
    let high = attach16(&retained16(17), b"raw", 0.);
    let low = attach8(&low, b"raw", 0.);
    assert!(unsafe { dr_precision_doc_has_retained_source(&high, 0) });
    assert!(unsafe { dr_doc_has_retained_source(&low, 1) });
    assert!(!unsafe { dr_doc_has_retained_source(&low, 0) });
    assert!(!unsafe { dr_doc_has_retained_source(&low, 99) });
    let mut invalid = low.clone();
    invalid.layers[1].meta = Some("edited pixels".into());
    assert!(!unsafe { dr_doc_has_retained_source(&invalid, 1) });
    let size = RawDescriptor::parse(&description(0.).to_string())
        .unwrap()
        .encoded_len()
        .unwrap();
    assert!(
        unsafe { dr_precision_doc_source_info_scratch_bytes(&high, 0) }
            >= 64 * 1024 + size as u64 * 768
    );
    assert!(unsafe { dr_doc_source_info_scratch_bytes(&low, 1) } >= 64 * 1024 + size as u64 * 768);
    assert_eq!(
        unsafe { dr_doc_source_info_scratch_bytes(&low, 99) },
        64 * 1024
    );
}
