//! #547 Lab Color phase 1, the document: the Lab twin is an RGB-data,
//! Lab-PCS LUT profile (`RgbLut`, no matrix model, D50 white, named "Lab
//! Color"); New Lab Color fills white as (255, 128, 128, 255)×257 exactly at
//! 8 bits and within 2 codes of (65535, 32896, 32896) at 16 (the bounds
//! declared in the spec), refusing 32 bits and unknown keys; the edit gate
//! admits the layer-structure edits (phase 1b's too) and, since the close,
//! answers every other by name, never naming a phase; the descriptor reports
//! the mode; and every export (the
//! view's, and phase 1b's thumbnail and Lab TIFF save) tolerates NULL.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::*;
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_free};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::dr_precision_doc_open_lab_psd;
use doctored_core::icc::{inspect, IccProfile, Inspect};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;

use crate::common::take_err_string;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn json(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err)
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn samples(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn new_lab(request: &str) -> Result<*mut PrecisionDocument, String> {
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(5, 4, request.as_ptr(), LIMIT, 0, &mut err) };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn edit(doc: *const PrecisionDocument, request: &str) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

#[test]
fn new_lab_color_fills_white_and_refuses_32_bits() {
    let doc = new_lab(r#"{"depth":8,"fill_srgb":[1,1,1,1],"layer_name":"Background","ppi":300}"#)
        .unwrap();
    assert!(samples(doc)
        .iter()
        .all(|p| *p == [255 * 257, 128 * 257, 128 * 257, 255 * 257]));
    let d = descriptor(doc);
    assert_eq!(
        (
            d["color_mode"].clone(),
            d["lab"]["depth"].clone(),
            d["resolution"]["x_ppi"].clone()
        ),
        ("lab".into(), 8.into(), 300.0.into())
    );
    free(doc);
    let doc = new_lab(r#"{"depth":16,"fill_srgb":[1,1,1,1],"layer_name":"Background","ppi":72}"#)
        .unwrap();
    for pixel in samples(doc) {
        for (got, want) in pixel.iter().zip([65535i64, 32896, 32896, 65535]) {
            assert!((i64::from(*got) - want).abs() <= 2, "{pixel:?}");
        }
        assert_eq!(pixel[3], 65535);
    }
    assert_eq!(descriptor(doc)["lab"]["depth"], 16);
    free(doc);
    // Transparent: alpha 0, the colour still white's Lab.
    let doc =
        new_lab(r#"{"depth":8,"fill_srgb":[1,1,1,0],"layer_name":"Layer 1","ppi":72}"#).unwrap();
    assert!(samples(doc).iter().all(|p| p[3] == 0));
    free(doc);
    assert_eq!(
        new_lab(r#"{"depth":32,"fill_srgb":[1,1,1,1],"layer_name":"x","ppi":72}"#).unwrap_err(),
        "A Lab Color document is 8 or 16 bits per channel."
    );
    assert_eq!(
        new_lab(r#"{"depth":8,"space":"sgray","fill_srgb":[1,1,1,1]}"#).unwrap_err(),
        "new Lab Color document request has an unknown key \"space\""
    );
    assert_eq!(
        new_lab(r#"{"depth":8,"fill_srgb":[1,1,1]}"#).unwrap_err(),
        "new Lab Color document fill_srgb must be four numbers in 0…1"
    );
}

#[test]
fn the_lab_twin_is_an_rgb_lut_profile_with_a_lab_pcs() {
    let doc = new_lab(r#"{"depth":16,"fill_srgb":[0.5,0.2,0.8,1]}"#).unwrap();
    let profile = unsafe { &*doc }.profile().clone();
    let bytes = profile.bytes().to_vec();
    assert_eq!(&bytes[12..24], b"mntrRGB Lab ");
    assert_eq!(&bytes[8..10], &[4, 0x30]);
    let mut name = String::new();
    assert_eq!(inspect(&bytes, &mut name), Inspect::RgbLut);
    assert_eq!(name, "Lab Color");
    let parsed = IccProfile::parse(&bytes).unwrap();
    assert!(parsed.model().is_none());
    assert!(parsed.can_decode() && parsed.can_encode());
    // Every layer shares the document's twin.
    assert_eq!(
        unsafe { &*doc }.layer_source(0).unwrap().profile().bytes(),
        profile.bytes()
    );
    free(doc);
}

#[test]
fn the_gate_admits_structure_edits_and_answers_every_other_by_name() {
    let lab = new_lab(r#"{"depth":16,"fill_srgb":[1,1,1,1]}"#).unwrap();
    let renamed = edit(lab, r#"{"op":"rename_layer","layer":0,"name":"Plate"}"#).unwrap();
    assert_eq!(descriptor(renamed)["color_mode"], "lab");
    free(renamed);
    let hidden = edit(lab, r#"{"op":"layer_visible","layer":0,"visible":false}"#).unwrap();
    assert_eq!(descriptor(hidden)["lab"]["depth"], 16);
    free(hidden);
    // The group disclosure passes the gate and reaches its own check.
    assert_eq!(
        edit(lab, r#"{"op":"layer_open","layer":0,"open":false}"#).unwrap_err(),
        "high-depth layer open state applies to groups only"
    );
    // Phase 1b's structure edits pass the gate and reach their own checks.
    let added = edit(
        lab,
        r#"{"op":"add_layer","name":"Layer 1","anchor":0,"position":"above"}"#,
    )
    .unwrap();
    assert_eq!(descriptor(added)["layer_count"], 2);
    free(added);
    // The close: an op no parser knows answers as on RGB, and a scalar read
    // of a target Lab has no plane for names the targets it takes.
    for op in ["sample", "fill_stroke", "adjustment"] {
        assert_eq!(
            edit(lab, &format!(r#"{{"op":"{op}"}}"#)).unwrap_err(),
            "operation is unavailable in the high-depth editing request API"
        );
    }
    for op in ["read_plane", "scalar_statistics"] {
        assert_eq!(
            edit(lab, &format!(r#"{{"op":"{op}"}}"#)).unwrap_err(),
            format!(
                "{op} on a Lab Color document takes target mask, channel, red (Lightness), \
                 green (a), blue (b) or alpha."
            )
        );
    }
    // Phase 5b: no document profile.
    assert_eq!(
        edit(lab, r#"{"op":"color_convert"}"#).unwrap_err(),
        "color_convert is unavailable in a Lab Color document, which has no document profile."
    );
    let d = descriptor(lab);
    let operations = d["supported_operations"].as_array().unwrap();
    assert!(operations.contains(&"layer_flatten".into()));
    assert!(operations.contains(&"convert".into()));
    assert!(operations.contains(&"layer_add".into()));
    assert!(operations.contains(&"export".into()));
    assert!(operations.contains(&"paint".into()));
    assert!(operations.contains(&"clone".into()));
    assert_eq!(d["storage"], "rgba16_unorm");
    free(lab);
}

#[test]
fn every_lab_export_tolerates_null() {
    let mut err = ptr::null_mut();
    unsafe {
        // #547 Lab phase 3a's opens.
        assert_eq!(dr_path_lab_source(ptr::null()), 0, "DR_LAB_SOURCE_NONE");
        assert!(dr_precision_doc_open_lab(ptr::null(), LIMIT, 0, ptr::null(), &mut err).is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_open_lab_psd(
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(
            dr_precision_doc_open_lab(ptr::null(), LIMIT, 0, ptr::null(), ptr::null_mut())
                .is_null()
        );
        assert!(dr_precision_doc_open_lab_psd(
            ptr::null(),
            1,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_new_lab(1, 1, ptr::null(), LIMIT, 0, &mut err).is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_to_lab_plan(ptr::null(), ptr::null(), &mut err).is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_to_lab(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_lab_to_rgb_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_lab_to_rgb(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(
            dr_precision_image_lab_display(ptr::null(), LIMIT, 0, ptr::null(), &mut err).is_null()
        );
        assert!(!err.is_null());
        dr_string_free(err);
        // Every NULL with a NULL err_out too.
        assert!(dr_precision_image_lab_display(
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_new_lab(1, 1, ptr::null(), LIMIT, 0, ptr::null_mut()).is_null());
        assert!(
            dr_precision_doc_convert_to_lab_plan(ptr::null(), ptr::null(), ptr::null_mut())
                .is_null()
        );
        assert!(dr_precision_doc_convert_to_lab(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_convert_lab_to_rgb_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_convert_lab_to_rgb(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
}

#[test]
fn every_new_lab_export_tolerates_null() {
    let mut err = ptr::null_mut();
    unsafe {
        assert!(dr_precision_image_lab_thumbnail_rgba8(
            ptr::null(),
            64,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(
            dr_precision_doc_save_lab_plan(ptr::null(), ptr::null(), ptr::null(), &mut err)
                .is_null()
        );
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(!dr_precision_doc_save_lab(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        dr_string_free(err);
        assert!(dr_precision_image_lab_thumbnail_rgba8(
            ptr::null(),
            64,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_save_lab_plan(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(!dr_precision_doc_save_lab(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        // Phase 5a's depth plan and edit: NULL with the reason, and with no
        // error slot.
        err = ptr::null_mut();
        assert!(dr_precision_doc_lab_depth_plan(ptr::null(), 8, &mut err).is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(
            dr_precision_doc_lab_depth_edit(ptr::null(), 8, LIMIT, 0, ptr::null(), &mut err)
                .is_null()
        );
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_lab_depth_plan(ptr::null(), 8, ptr::null_mut()).is_null());
        assert!(dr_precision_doc_lab_depth_edit(
            ptr::null(),
            8,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        // Phase 2d's value-only kernel sample: each NULL pointer is false.
        let op = CString::new("hue_saturation").unwrap();
        let lab = [50.0f32, 10.0, -10.0];
        let mut rgb = [0.0f32; 3];
        assert!(!dr_lab_kernel_rgb(
            ptr::null(),
            lab.as_ptr(),
            rgb.as_mut_ptr()
        ));
        assert!(!dr_lab_kernel_rgb(
            op.as_ptr(),
            ptr::null(),
            rgb.as_mut_ptr()
        ));
        assert!(!dr_lab_kernel_rgb(
            op.as_ptr(),
            lab.as_ptr(),
            ptr::null_mut()
        ));
        assert!(dr_lab_kernel_rgb(
            op.as_ptr(),
            lab.as_ptr(),
            rgb.as_mut_ptr()
        ));
    }
}
