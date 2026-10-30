//! #547 phase 1a, the Grayscale document mode: Image ▸ Mode ▸ Grayscale
//! against a hand-computed luminance (sRGB's published D50 luminance row and
//! the IEC curve, then each Gray space's own definition: `Y^e` for Dot Gain
//! and Gray Gamma, the IEC encode for sGray), with the bounds declared in
//! the spec before measuring — ≤ 1 code at 8 bits, ≤ 4 of 65535 at 16, ≤
//! 1e-4 absolute at 32 with R = G = B and alpha exact; RGB Color back
//! (sGray at 8 bits returns every code exactly; Gray 32 → sRGB 32 returns
//! R = G = B = Y within 1e-4); the flatten blocker; New Grayscale; the edit
//! gate's sentences; the descriptor; and the null-safety sweep.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gray::*;
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_free};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::require_gray;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{png, srgb_chunk, srgb_decode, srgb_encode, SRGB_Y_ROW};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// The sRGB patches every oracle case converts (8-bit codes).
const PATCHES: [[u16; 3]; 10] = [
    [200, 40, 90],
    [30, 180, 220],
    [128, 128, 128],
    [250, 250, 10],
    [64, 0, 255],
    [0, 0, 0],
    [255, 255, 255],
    [90, 160, 40],
    [17, 34, 51],
    [240, 128, 200],
];

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

/// A 16-bit sRGB document of one row of the patches, alpha ramping.
fn rgb_doc() -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = PATCHES
        .iter()
        .enumerate()
        .map(|(i, p)| [p[0] * 257, p[1] * 257, p[2] * 257, 65535 - i as u16 * 1000])
        .collect();
    let image =
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ))
}

fn to_gray(doc: *const PrecisionDocument, options: &str) -> Result<*mut PrecisionDocument, String> {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray(
            doc,
            ptr::null(),
            0,
            options.as_ptr(),
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

fn to_gray_plan(doc: *const PrecisionDocument, options: &str) -> Value {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    json(
        unsafe {
            dr_precision_doc_convert_to_gray_plan(doc, ptr::null(), 0, options.as_ptr(), &mut err)
        },
        err,
    )
}

fn to_srgb(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let icc = srgb.bytes();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_gray_to_rgb(
            doc,
            icc.as_ptr(),
            icc.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to RGB: {}", take_err_string(err));
    out
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

fn u16_samples(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn f32_samples(doc: *const PrecisionDocument) -> Vec<[f32; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap()
}

/// The hand-computed relative luminance of an 8-bit sRGB patch.
fn luminance(patch: [u16; 3]) -> f64 {
    (0..3)
        .map(|c| SRGB_Y_ROW[c] * srgb_decode(f64::from(patch[c]) / 255.0))
        .sum()
}

/// Each space's own definition (`gray_working`'s module doc).
fn gray_of(space: &str, y: f64) -> f64 {
    let dot_gain = |d: f64| 0.5f64.ln() / (0.5 - d / 100.0).ln();
    match space {
        "dot_gain_20" => y.powf(dot_gain(20.0)),
        "gray_gamma_22" => y.powf(1.0 / 2.2),
        "sgray" => srgb_encode(y),
        _ => unreachable!(),
    }
}

#[test]
fn rgb_to_gray_lands_on_the_hand_computed_luminance_at_8_and_16_bits() {
    let rgb = rgb_doc();
    for space in ["dot_gain_20", "gray_gamma_22", "sgray"] {
        for depth in [16, 8] {
            let gray = to_gray(rgb, &format!(r#"{{"space":"{space}","depth":{depth}}}"#)).unwrap();
            let d = descriptor(gray);
            assert_eq!(d["color_mode"], "gray");
            assert_eq!(d["gray"]["depth"], depth);
            for (i, (pixel, patch)) in u16_samples(gray).iter().zip(PATCHES).enumerate() {
                assert!(
                    pixel[0] == pixel[1] && pixel[1] == pixel[2],
                    "{space} {depth} {i}"
                );
                let expected = gray_of(space, luminance(patch));
                let alpha = 65535 - i as u16 * 1000;
                if depth == 16 {
                    let oracle = (expected * 65535.0).round() as i64;
                    assert!(
                        (i64::from(pixel[0]) - oracle).abs() <= 4,
                        "{space} 16 patch {i}: {} vs {oracle}",
                        pixel[0]
                    );
                    assert_eq!(pixel[3], alpha);
                } else {
                    assert_eq!(pixel[0] % 257, 0, "{space} 8 patch {i} on the grid");
                    let oracle = (expected * 255.0).round() as i64;
                    assert!(
                        (i64::from(pixel[0] / 257) - oracle).abs() <= 1,
                        "{space} 8 patch {i}: {} vs {oracle}",
                        pixel[0] / 257
                    );
                    assert_eq!(pixel[3], ((u32::from(alpha) + 128) / 257 * 257) as u16);
                }
            }
            free(gray);
        }
    }
    free(rgb);
}

#[test]
fn rgb_32_to_gray_32_writes_linear_luminance_and_comes_back_neutral() {
    let rgb = rgb_doc();
    let mut err = ptr::null_mut();
    let float = unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!float.is_null(), "{}", take_err_string(err));
    let source = f32_samples(float);
    for space in ["dot_gain_20", "gray_gamma_22", "sgray"] {
        let gray = to_gray(float, &format!(r#"{{"space":"{space}"}}"#)).unwrap();
        assert_eq!(descriptor(gray)["gray"]["depth"], 32);
        let out = f32_samples(gray);
        for (i, (pixel, src)) in out.iter().zip(&source).enumerate() {
            let y: f64 = (0..3).map(|c| SRGB_Y_ROW[c] * f64::from(src[c])).sum();
            assert!(pixel[0] == pixel[1] && pixel[1] == pixel[2], "{space} {i}");
            assert!(
                (f64::from(pixel[0]) - y).abs() <= 1e-4,
                "{space} {i}: {} vs {y}",
                pixel[0]
            );
            assert_eq!(pixel[3], src[3]);
        }
        let back = to_srgb(gray);
        assert_eq!(descriptor(back)["color_mode"], "rgb");
        for (pixel, gray_pixel) in f32_samples(back).iter().zip(&out) {
            for c in 0..3 {
                assert!(
                    (pixel[c] - gray_pixel[0]).abs() <= 1e-4,
                    "{space}: {pixel:?}"
                );
            }
        }
        free(back);
        free(gray);
    }
    free(float);
    free(rgb);
}

#[test]
fn an_sgray_document_of_every_8_bit_code_returns_each_code_in_srgb() {
    let dir = TempDir::new().unwrap();
    let codes: Vec<u16> = (0..256).collect();
    let path = dir.path().join("codes.png");
    std::fs::write(&path, png(256, 1, 8, 0, 1, &codes, &[srgb_chunk()])).unwrap();
    let c = cpath(&path);
    let working = CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_open_gray(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "{}", take_err_string(err));
    let rgb = to_srgb(gray);
    for (pixel, code) in u16_samples(rgb).iter().zip(codes) {
        assert_eq!(*pixel, [code * 257, code * 257, code * 257, 65535]);
    }
    // No pixel changed, and the document is in sRGB by name too.
    assert_eq!(
        unsafe { &*rgb }.profile().bytes(),
        IccProfile::srgb().bytes()
    );
    free(rgb);
    free(gray);
}

#[test]
fn a_record_gray_cannot_hold_is_named_and_flatten_converts() {
    let rgb = rgb_doc();
    let styled = edit(
        rgb,
        r#"{"op":"layer_style","version":1,"layer":0,"style":{"effects":[{"type":"drop_shadow","blend":"hue","distance":3,"size":2}]}}"#,
    )
    .unwrap();
    let sentence = "“Background” is a layer with a layer effect in the Hue blend mode, which a Grayscale document cannot hold; flatten the image to convert it";
    let plan = to_gray_plan(styled, r#"{"space":"dot_gain_20"}"#);
    assert_eq!(plan["flatten_required"], true);
    assert_eq!(plan["flattens"], false);
    assert_eq!(plan["refusal"], sentence);
    assert_eq!(
        to_gray(styled, r#"{"space":"dot_gain_20"}"#).unwrap_err(),
        sentence
    );
    let plan = to_gray_plan(styled, r#"{"space":"dot_gain_20","flatten":true}"#);
    assert_eq!(
        (plan["refusal"].clone(), plan["flattens"].clone()),
        (Value::Null, true.into())
    );
    assert!(plan["working_memory_plan_bytes"].as_u64().unwrap() > 0);
    let gray = to_gray(styled, r#"{"space":"dot_gain_20","flatten":true}"#).unwrap();
    let d = descriptor(gray);
    assert_eq!(
        (d["color_mode"].clone(), d["layer_count"].clone()),
        ("gray".into(), 1.into())
    );
    free(gray);
    // The plain document needs no flatten.
    let plan = to_gray_plan(rgb, r#"{"space":"dot_gain_20"}"#);
    assert_eq!(
        (plan["refusal"].clone(), plan["flatten_required"].clone()),
        (Value::Null, false.into())
    );
    // Hard refusals.
    assert_eq!(
        to_gray(rgb, "{}").unwrap_err(),
        "Grayscale needs a Gray profile or space to convert into"
    );
    assert_eq!(
        to_gray(rgb, r#"{"space":"dot_gain_20","bogus":1}"#).unwrap_err(),
        "mode options have an unknown key \"bogus\""
    );
    let gray = to_gray(rgb, r#"{"space":"dot_gain_20"}"#).unwrap();
    assert_eq!(
        to_gray(gray, r#"{"space":"dot_gain_20"}"#).unwrap_err(),
        "the document is already Grayscale"
    );
    free(gray);
    free(styled);
    free(rgb);
}

#[test]
fn the_gate_admits_structure_edits_and_names_the_phase_of_every_other() {
    let rgb = rgb_doc();
    let gray = to_gray(rgb, r#"{"space":"gray_gamma_22"}"#).unwrap();
    let renamed = edit(gray, r#"{"op":"rename_layer","layer":0,"name":"Plate"}"#).unwrap();
    assert_eq!(descriptor(renamed)["color_mode"], "gray");
    free(renamed);
    // The group disclosure passes the gate and reaches its own check.
    assert_eq!(
        edit(gray, r#"{"op":"layer_open","layer":0,"open":false}"#).unwrap_err(),
        "high-depth layer open state applies to groups only"
    );
    // Phase 4: geometry is admitted and keeps the mode.
    let resized = edit(
        gray,
        r#"{"op":"resize","width":2,"height":2,"filter":"bilinear"}"#,
    )
    .unwrap();
    assert_eq!(descriptor(resized)["color_mode"], "gray");
    free(resized);
    assert_eq!(
        edit(gray, r#"{"op":"read_plane"}"#).unwrap_err(),
        "read_plane of the Gray layer is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, so a scalar read takes target mask or channel."
    );
    assert!(require_gray("layer_comp", "document").is_ok());
    assert!(require_gray("layer_link", "raster").is_ok());
    assert!(require_gray("layer_name", "raster").is_ok());
    assert!(require_gray("convert", "document").is_ok());
    assert!(require_gray("geometry_crop", "document").is_ok());
    assert!(require_gray("profile", "document").is_ok());
    assert!(require_gray("proof", "document").is_ok());
    assert!(require_gray("paint", "raster").is_ok());
    let d = descriptor(gray);
    assert_eq!(d["gray"]["profile"], "Gray Gamma 2.2");
    assert!(d["supported_operations"]
        .as_array()
        .unwrap()
        .contains(&"layer_flatten".into()));
    assert_eq!(descriptor(rgb)["color_mode"], "rgb");
    free(gray);
    free(rgb);
}

#[test]
fn new_grayscale_fills_with_the_srgb_colours_luminance() {
    let new = |request: &str| {
        let request = CString::new(request).unwrap();
        let mut err = ptr::null_mut();
        let doc = unsafe {
            dr_precision_doc_new_gray(5, 4, ptr::null(), 0, request.as_ptr(), LIMIT, 0, &mut err)
        };
        if doc.is_null() {
            Err(take_err_string(err))
        } else {
            Ok(doc)
        }
    };
    let doc = new(
        r#"{"depth":16,"space":"gray_gamma_22","fill_srgb":[1,0,0,1],"layer_name":"Background","ppi":300}"#,
    )
    .unwrap();
    let y = SRGB_Y_ROW[0];
    let oracle = (y.powf(1.0 / 2.2) * 65535.0).round() as i64;
    for pixel in u16_samples(doc) {
        assert!(pixel[0] == pixel[1] && pixel[1] == pixel[2] && pixel[3] == 65535);
        assert!(
            (i64::from(pixel[0]) - oracle).abs() <= 4,
            "{} vs {oracle}",
            pixel[0]
        );
    }
    let d = descriptor(doc);
    assert_eq!(
        (d["gray"]["depth"].clone(), d["resolution"]["x_ppi"].clone()),
        (16.into(), 300.0.into())
    );
    free(doc);
    let doc =
        new(r#"{"depth":8,"space":"sgray","fill_srgb":[0,0,0,0],"layer_name":"Layer 1","ppi":72}"#)
            .unwrap();
    assert!(u16_samples(doc).iter().all(|p| *p == [0, 0, 0, 0]));
    free(doc);
    assert_eq!(
        new(r#"{"depth":12,"space":"sgray","fill_srgb":[0,0,0,1],"layer_name":"x","ppi":72}"#)
            .unwrap_err(),
        "new Grayscale document depth must be 8, 16 or 32"
    );
    assert_eq!(
        new(r#"{"depth":8,"fill_srgb":[0,0,0,1],"layer_name":"x","ppi":72}"#).unwrap_err(),
        "a new Grayscale document needs a Gray profile or space"
    );
}

#[test]
fn every_gray_export_tolerates_null() {
    let mut err = ptr::null_mut();
    unsafe {
        assert_eq!(dr_path_gray_source(ptr::null()), 0);
        assert!(dr_precision_doc_open_gray(
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
        assert!(
            dr_precision_doc_new_gray(1, 1, ptr::null(), 0, ptr::null(), LIMIT, 0, &mut err)
                .is_null()
        );
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_to_gray_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_to_gray(
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
        assert!(dr_precision_doc_convert_gray_to_rgb_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_gray_to_rgb(
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
        // Every NULL with a NULL err_out too.
        assert!(dr_precision_doc_open_gray(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_convert_to_gray(
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
        // #547 phase 3a: the gray PSD open.
        assert!(doctored_core::ffi_psd::dr_precision_doc_open_gray_psd(
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        assert!(doctored_core::ffi_psd::dr_precision_doc_open_gray_psd(
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
}
