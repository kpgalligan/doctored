//! #547 Lab Color phase 1: Image ▸ Mode ▸ Lab Color and RGB Color back,
//! against the CIE formulas computed here — the ICC PCS white (0.9642, 1.0,
//! 0.8249), the published Bradford-adapted sRGB → XYZ (D50) matrix and the
//! IEC 61966-2-1 curve — with the bounds declared in the spec before
//! measuring: sRGB patches → Lab within 1 code per channel at 8 bits and
//! |ΔL\*|, |Δa\*|, |Δb\*| ≤ 0.05 at 16; the document's own Lab patches →
//! sRGB within 1 code at 8 bits and 32/65535 at 16; sRGB16 → Lab16 → sRGB16
//! within 64/65535 for L\* ≥ 10; a Gray 8 document → Lab with a = b = 128
//! exactly and L within 1 code of `116 f(kTRC(code)) − 16`; alpha exact
//! everywhere; the Lab view in Display P3 within 32/65535 of the inverse
//! formula at u16. Then the flatten blocker and the hard refusals, and (phase 2's
//! take) Photoshop's own readings of 26 patches at 8 and 16 bits, held to the take's gate.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_gray::{dr_precision_doc_convert_to_gray, dr_precision_doc_open_gray};
use doctored_core::ffi_lab::*;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_render,
    dr_precision_image_free,
};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, take_err_string};
use crate::gray_fixture::png;
use crate::lab_fixture::{decode, f, grid, lab_to_rgb, lab_to_srgb, srgb_to_lab, P3_TO_XYZ};

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

fn samples(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn boxed(doc: PrecisionDocument) -> *mut PrecisionDocument {
    Box::into_raw(Box::new(doc))
}

/// A 16-bit sRGB document of one row of `pixels`.
fn srgb_doc(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image =
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap();
    boxed(PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap())
}

/// The patches at 16 bits, alpha ramping.
fn patch_doc() -> *mut PrecisionDocument {
    srgb_doc(
        PATCHES
            .iter()
            .enumerate()
            .map(|(i, p)| [p[0] * 257, p[1] * 257, p[2] * 257, 65535 - i as u16 * 1000])
            .collect(),
    )
}

fn to_lab(doc: *const PrecisionDocument, options: &str) -> Result<*mut PrecisionDocument, String> {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn to_lab_plan(doc: *const PrecisionDocument, options: &str) -> Value {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    json(
        unsafe { dr_precision_doc_convert_to_lab_plan(doc, options.as_ptr(), &mut err) },
        err,
    )
}

fn to_srgb(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let icc = srgb.bytes();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
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

#[test]
fn srgb_patches_land_on_the_cie_formula_at_8_and_16_bits() {
    let rgb = patch_doc();
    for depth in [16, 8] {
        let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
        let d = descriptor(lab);
        assert_eq!(d["color_mode"], "lab");
        assert_eq!(d["lab"]["depth"], depth);
        for (i, (pixel, patch)) in samples(lab).iter().zip(PATCHES).enumerate() {
            let oracle = srgb_to_lab(patch.map(|c| f64::from(c) / 255.0));
            let alpha = 65535 - i as u16 * 1000;
            if depth == 16 {
                let got = decode(*pixel);
                for c in 0..3 {
                    assert!(
                        (got[c] - oracle[c]).abs() <= 0.05,
                        "16 patch {i} channel {c}: {got:?} vs {oracle:?}"
                    );
                }
                assert_eq!(pixel[3], alpha);
            } else {
                let codes = [
                    oracle[0] * 255.0 / 100.0,
                    oracle[1] + 128.0,
                    oracle[2] + 128.0,
                ];
                for c in 0..3 {
                    assert_eq!(pixel[c] % 257, 0, "8 patch {i} on the grid");
                    let expected = codes[c].round().clamp(0.0, 255.0) as i64;
                    assert!(
                        (i64::from(pixel[c] / 257) - expected).abs() <= 1,
                        "8 patch {i} channel {c}: {} vs {expected}",
                        pixel[c] / 257
                    );
                }
                assert_eq!(pixel[3], grid(alpha));
            }
        }
        free(lab);
    }
    free(rgb);
}

#[test]
fn the_documents_lab_patches_return_to_srgb_by_the_inverse_formula() {
    let rgb = patch_doc();
    for depth in [16, 8] {
        let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
        let codes = samples(lab);
        let back = to_srgb(lab);
        assert_eq!(descriptor(back)["color_mode"], "rgb");
        for (i, (pixel, lab_sample)) in samples(back).iter().zip(&codes).enumerate() {
            // Every patch came from sRGB, so its Lab is in gamut up to the
            // Lab quantization, which the clamp absorbs.
            let oracle = lab_to_srgb(decode(*lab_sample)).map(|v| v.clamp(0.0, 1.0));
            for c in 0..3 {
                if depth == 16 {
                    let expected = (oracle[c] * 65535.0).round() as i64;
                    assert!(
                        (i64::from(pixel[c]) - expected).abs() <= 32,
                        "16 patch {i} channel {c}: {} vs {expected}",
                        pixel[c]
                    );
                } else {
                    let expected = (oracle[c] * 255.0).round() as i64;
                    let got = (f64::from(pixel[c]) / 257.0).round() as i64;
                    assert!(
                        (got - expected).abs() <= 1,
                        "8 patch {i} channel {c}: {got} vs {expected}"
                    );
                }
            }
            assert_eq!(pixel[3], lab_sample[3], "alpha {i}");
        }
        free(back);
        free(lab);
    }
    free(rgb);
}

fn render(doc: *const PrecisionDocument) -> *mut PrecisionImage {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "render: {}", take_err_string(err));
    image
}

fn lab_view(image: *const PrecisionImage) -> Result<*mut PrecisionImage, String> {
    let mut err = ptr::null_mut();
    let view = unsafe { dr_precision_image_lab_display(image, LIMIT, 0, ptr::null(), &mut err) };
    if view.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(view)
    }
}

/// The Lab view, against the inverse formula into Apple's published
/// Display P3 with the bound declared before measuring (the Lab → RGB one):
/// every patch of the 8- and 16-bit documents' composite within 32/65535 of
/// the oracle at u16, before any narrowing; a neutral stays neutral within
/// the same; alpha exact; the result tagged the built-in Display P3. An
/// image that is not Lab refuses.
#[test]
fn the_view_is_display_p3_by_the_inverse_formula_before_any_narrowing() {
    let rgb = patch_doc();
    for depth in [16, 8] {
        let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
        let projection = render(lab);
        let view = lab_view(projection).unwrap();
        assert_eq!(
            unsafe { &*view }.profile().bytes(),
            IccProfile::display_p3().bytes()
        );
        let codes = unsafe { &*projection }.to_rgba16().unwrap();
        let shown = unsafe { &*view }.to_rgba16().unwrap();
        assert_eq!(codes.len(), PATCHES.len());
        for (i, (pixel, code)) in shown.iter().zip(&codes).enumerate() {
            let oracle = lab_to_rgb(decode(*code), P3_TO_XYZ).map(|v| v.clamp(0.0, 1.0));
            for c in 0..3 {
                let expected = (oracle[c] * 65535.0).round() as i64;
                assert!(
                    (i64::from(pixel[c]) - expected).abs() <= 32,
                    "{depth} patch {i} channel {c}: {} vs {expected}",
                    pixel[c]
                );
            }
            if PATCHES[i][0] == PATCHES[i][1] && PATCHES[i][1] == PATCHES[i][2] {
                let spread = pixel[..3].iter().max().unwrap() - pixel[..3].iter().min().unwrap();
                assert!(spread <= 32, "{depth} neutral patch {i}: {pixel:?}");
            }
            assert_eq!(pixel[3], code[3], "alpha {i}");
        }
        unsafe {
            dr_precision_image_free(view);
            dr_precision_image_free(projection);
        }
        free(lab);
    }
    let projection = render(rgb);
    assert_eq!(
        lab_view(projection).unwrap_err(),
        "the Lab Color view takes a Lab Color image"
    );
    unsafe { dr_precision_image_free(projection) };
    free(rgb);
}

#[test]
fn srgb16_through_lab16_returns_within_64_codes_above_l_10() {
    let pixels: Vec<[u16; 4]> = (0..216u32)
        .map(|i| {
            let (r, g, b) = (i % 6, i / 6 % 6, i / 36);
            [
                (r * 13107) as u16,
                (g * 13107) as u16,
                (b * 13107) as u16,
                (i * 300) as u16,
            ]
        })
        .collect();
    let rgb = srgb_doc(pixels.clone());
    let lab = to_lab(rgb, "{}").unwrap();
    let back = to_srgb(lab);
    let mut checked = 0;
    for (i, (pixel, source)) in samples(back).iter().zip(&pixels).enumerate() {
        assert_eq!(pixel[3], source[3], "alpha {i}");
        let l = srgb_to_lab([0, 1, 2].map(|c| f64::from(source[c]) / 65535.0))[0];
        if l < 10.0 {
            continue;
        }
        checked += 1;
        for c in 0..3 {
            assert!(
                (i64::from(pixel[c]) - i64::from(source[c])).abs() <= 64,
                "pixel {i} channel {c}: {pixel:?} vs {source:?}"
            );
        }
    }
    assert!(checked > 150);
    for doc in [back, lab, rgb] {
        free(doc);
    }
}

#[test]
fn a_gray_8_document_becomes_neutral_lab_on_its_curve() {
    let dir = TempDir::new().unwrap();
    let codes: Vec<u16> = (0..256).collect();
    let path = dir.path().join("codes.png");
    std::fs::write(&path, png(256, 1, 8, 0, 1, &codes, &[])).unwrap();
    let c = cpath(&path);
    let working = CString::new("gray_gamma_22").unwrap();
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
    let lab = to_lab(gray, r#"{"depth":8}"#).unwrap();
    for (pixel, code) in samples(lab).iter().zip(codes) {
        assert_eq!((pixel[1], pixel[2], pixel[3]), (32896, 32896, 65535));
        // Gray Gamma 2.2's kTRC: Y = v^2.2.
        let y = (f64::from(code) / 255.0).powf(2.2);
        let expected = ((116.0 * f(y) - 16.0) * 255.0 / 100.0).round() as i64;
        assert!(
            (i64::from(pixel[0] / 257) - expected).abs() <= 1,
            "code {code}: {} vs {expected}",
            pixel[0] / 257
        );
    }
    free(lab);
    free(gray);
}

#[test]
fn a_record_lab_cannot_hold_is_named_and_flatten_converts() {
    let rgb = patch_doc();
    let edit = |doc: *const PrecisionDocument, request: &str| {
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
        assert!(!out.is_null(), "{}", take_err_string(err));
        out
    };
    // Darken: a Lab Color document does not offer it (every mode it does
    // offer is admitted since phase 3).
    let multiplied = edit(
        rgb,
        r#"{"op":"layer_blend","version":1,"layer":0,"blend_mode":6}"#,
    );
    let sentence = "“Background” is a layer in the Darken blend mode, which a Lab Color document cannot hold; flatten the image to convert it";
    let plan = to_lab_plan(multiplied, "{}");
    assert_eq!(
        (
            plan["refusal"].clone(),
            plan["flatten_required"].clone(),
            plan["flattens"].clone()
        ),
        (sentence.into(), true.into(), false.into())
    );
    assert_eq!(to_lab(multiplied, "{}").unwrap_err(), sentence);
    let plan = to_lab_plan(multiplied, r#"{"flatten":true}"#);
    assert_eq!(
        (plan["refusal"].clone(), plan["flattens"].clone()),
        (Value::Null, true.into())
    );
    assert!(plan["working_memory_plan_bytes"].as_u64().unwrap() > 0);
    let lab = to_lab(multiplied, r#"{"flatten":true}"#).unwrap();
    let d = descriptor(lab);
    assert_eq!(
        (d["color_mode"].clone(), d["layer_count"].clone()),
        ("lab".into(), 1.into())
    );
    free(lab);
    // Phase 2e: an effect carries; one in a mode the Lab list lacks is named.
    let styled = edit(
        rgb,
        r#"{"op":"layer_style","version":1,"layer":0,"style":{"effects":[{"type":"drop_shadow","distance":3,"size":2,"blend":"darken"}]}}"#,
    );
    assert_eq!(
        to_lab(styled, "{}").unwrap_err(),
        "“Background” cannot convert to Lab Color: The Darken blend mode is unavailable in a Lab Color document. Flatten the image to convert it."
    );
    free(styled);
    // Phase 1b: a Multiply layer with a Fill Opacity style carries.
    let multiply = edit(
        rgb,
        r#"{"op":"layer_blend","version":1,"layer":0,"blend_mode":1}"#,
    );
    let filled = edit(
        multiply,
        r#"{"op":"layer_style","version":1,"layer":0,"style":{"fill_opacity":0.5}}"#,
    );
    let plan = to_lab_plan(filled, "{}");
    assert_eq!(
        (plan["refusal"].clone(), plan["flatten_required"].clone()),
        (Value::Null, false.into())
    );
    let lab = to_lab(filled, "{}").unwrap();
    let layer = unsafe { &*lab }.layer(0).unwrap();
    assert_eq!(layer.blend_mode(), BlendMode::Multiply);
    assert_eq!(descriptor(lab)["color_mode"], "lab");
    free(lab);
    free(filled);
    free(multiply);
    free(multiplied);
    // The plain document needs no flatten.
    let plan = to_lab_plan(rgb, "{}");
    assert_eq!(
        (plan["refusal"].clone(), plan["flatten_required"].clone()),
        (Value::Null, false.into())
    );
    // Options.
    assert_eq!(
        to_lab(rgb, r#"{"bogus":1}"#).unwrap_err(),
        "mode options have an unknown key \"bogus\""
    );
    assert_eq!(
        to_lab(rgb, r#"{"depth":32}"#).unwrap_err(),
        "mode option depth must be 8 or 16"
    );
    free(rgb);
}

#[test]
fn the_hard_refusals_name_the_route() {
    let rgb = patch_doc();
    let lab = to_lab(rgb, "{}").unwrap();
    assert_eq!(
        to_lab(lab, "{}").unwrap_err(),
        "the document is already Lab Color"
    );
    assert_eq!(
        to_lab_plan(lab, "{}")["refusal"],
        "the document is already Lab Color"
    );
    // Lab → Grayscale converts directly since #547 Lab phase 5a-ii
    // (`lab_gray.rs` holds its oracles); Lab → CMYK since phase 5a
    // (`lab_cmyk_depth.rs`).
    let options = CString::new(r#"{"space":"sgray"}"#).unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            lab,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "{}", take_err_string(err));
    free(gray);
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            lab,
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "{}", take_err_string(err));
    free(cmyk);
    // A CMYK document goes through RGB Color first.
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "{}", take_err_string(err));
    let plan = to_lab_plan(cmyk, "{}");
    assert_eq!(
        plan["refusal"],
        "Lab Color converts an RGB or Grayscale document; convert to RGB Color first"
    );
    assert!(plan["working_memory_plan_bytes"].as_u64().unwrap() > 0);
    free(cmyk);
    // A 32-bit document converts to 16 Bits/Channel first.
    let mut err = ptr::null_mut();
    let float = unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!float.is_null(), "{}", take_err_string(err));
    assert_eq!(
        to_lab(float, "{}").unwrap_err(),
        "Lab Color is 8 or 16 bits per channel; convert to 16 Bits/Channel first"
    );
    free(float);
    // RGB Color on a document that is not Lab.
    let srgb = IccProfile::srgb();
    let icc = srgb.bytes();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            rgb,
            icc.as_ptr(),
            icc.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), "the document is not Lab Color");
    let mut err = ptr::null_mut();
    let plan = json(
        unsafe {
            dr_precision_doc_convert_lab_to_rgb_plan(
                lab,
                icc.as_ptr(),
                icc.len(),
                ptr::null(),
                &mut err,
            )
        },
        err,
    );
    assert_eq!(plan["refusal"], Value::Null);
    assert!(plan["working_memory_plan_bytes"].as_u64().unwrap() > 0);
    free(lab);
    free(rgb);
}

/// #547 Lab phase 2's Photoshop take (`scripts/photoshop-probe/lab_mode_take.py`,
/// `lab-mode-547/take1`, Photoshop 26.10.0): the 26 sRGB patches (8-bit codes;
/// `gray_luminance_take.item1_patches`, seed 4421) it converted by Image ▸ Mode ▸ Lab Color.
const TAKE_PATCHES: [[u16; 3]; 26] = [
    [8, 8, 8],
    [16, 16, 16],
    [24, 24, 24],
    [48, 48, 48],
    [96, 96, 96],
    [128, 128, 128],
    [160, 160, 160],
    [224, 224, 224],
    [232, 68, 88],
    [58, 28, 182],
    [65, 144, 48],
    [179, 95, 102],
    [200, 31, 174],
    [198, 35, 22],
    [145, 10, 8],
    [146, 74, 252],
    [255, 195, 65],
    [226, 13, 100],
    [186, 50, 213],
    [126, 91, 155],
    [44, 95, 115],
    [189, 217, 241],
    [58, 52, 132],
    [122, 227, 175],
    [50, 135, 99],
    [174, 27, 143],
];

/// Photoshop's L\*, a\*, b\* of each patch at 8 bits: the mean of four colour samplers.
const PHOTOSHOP_LAB8: [[f64; 3]; 26] = [
    [2.0575, 0.0, 0.0],
    [4.71, 0.0, 0.0],
    [8.24, 0.0, 0.0],
    [20.0, 0.0, 0.0],
    [40.6825, 0.0, 0.0],
    [53.53, 0.0, 0.0],
    [65.88, 0.0, 0.0],
    [89.215, 0.0, 0.0],
    [54.705, 64.25, 27.75],
    [25.49, 47.25, -76.75],
    [53.33, -39.5, 42.0],
    [50.785, 35.5, 12.0],
    [47.45, 71.75, -35.0],
    [44.0175, 62.0, 50.5],
    [30.6875, 51.25, 41.25],
    [48.63, 57.0, -78.75],
    [82.75, 13.25, 70.0],
    [49.5075, 74.75, 13.25],
    [48.63, 68.25, -56.0],
    [44.31, 24.0, -29.5],
    [37.455, -12.5, -17.0],
    [85.1975, -6.0, -15.0],
    [26.08, 21.5, -45.0],
    [82.75, -40.5, 16.0],
    [50.4925, -33.25, 11.25],
    [41.18, 63.25, -27.0],
];

/// The same at 16 bits.
const PHOTOSHOP_LAB16: [[f64; 3]; 26] = [
    [2.2, 0.0, 0.0],
    [4.68, 0.0, 0.0],
    [8.25, 0.0, 0.0],
    [19.86, 0.0, 0.0],
    [40.73, 0.0, 0.0],
    [53.58, 0.0, 0.0],
    [65.87, 0.0, 0.0],
    [89.18, 0.0, 0.0],
    [54.75, 64.2, 27.66],
    [25.59, 47.41, -76.5],
    [53.33, -39.41, 41.55],
    [50.73, 35.23, 12.19],
    [47.49, 71.66, -35.09],
    [43.97, 61.97, 50.55],
    [30.72, 51.68, 41.68],
    [48.52, 57.28, -78.62],
    [82.75, 13.62, 69.77],
    [49.47, 74.93, 13.26],
    [48.57, 68.44, -56.23],
    [44.42, 24.36, -29.65],
    [37.49, -12.86, -16.87],
    [85.2, -5.81, -15.09],
    [26.16, 21.43, -45.12],
    [82.91, -40.81, 15.63],
    [50.49, -33.52, 11.53],
    [41.0, 63.52, -27.06],
];

/// The take's gate in 8-bit codes (L = 2.55·L\*, a = a\* + 128, b = b\* + 128), fixed in the
/// runner before the smoke: per channel max |r| ≤ 2 codes and mean |r| ≤ 0.5 over the 26
/// patches; on a channel whose readings were all whole units (the instrument rule) the reference
/// is first rounded half-up to whole units and the max is one reading step if larger (2.55 for
/// L). `TAKE_INSTRUMENT` is the rule per depth and channel as `decided.json` recorded it.
const TAKE_MAX: f64 = 2.0;
const TAKE_MEAN: f64 = 0.5;
const TAKE_STEP: [f64; 3] = [2.55, 1.0, 1.0];
const TAKE_INSTRUMENT: [(u32, [bool; 3]); 2] =
    [(16, [false, false, false]), (8, [false, true, true])];

#[test]
fn photoshops_lab_mode_readings_meet_the_encoding_gate() {
    let rgb = srgb_doc(
        TAKE_PATCHES
            .iter()
            .map(|p| [p[0] * 257, p[1] * 257, p[2] * 257, 65535])
            .collect(),
    );
    for (depth, instrument) in TAKE_INSTRUMENT {
        let readings = if depth == 16 {
            &PHOTOSHOP_LAB16
        } else {
            &PHOTOSHOP_LAB8
        };
        let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
        let ours = samples(lab);
        assert_eq!(ours.len(), readings.len());
        for c in 0..3 {
            let bound = if instrument[c] {
                TAKE_MAX.max(TAKE_STEP[c])
            } else {
                TAKE_MAX
            };
            let residuals: Vec<f64> = ours
                .iter()
                .zip(readings.iter())
                .map(|(pixel, photoshop)| {
                    let mut ours = decode(*pixel)[c];
                    if instrument[c] {
                        ours = (ours + 0.5).floor();
                    }
                    // Photoshop − Doctored in codes; the a/b offset of 128 cancels.
                    TAKE_STEP[c] * (photoshop[c] - ours)
                })
                .collect();
            let worst = residuals.iter().fold(0.0f64, |m, r| m.max(r.abs()));
            let mean = residuals.iter().map(|r| r.abs()).sum::<f64>() / residuals.len() as f64;
            assert!(
                worst <= bound + 1e-9 && mean <= TAKE_MEAN + 1e-9,
                "{depth}-bit channel {c}: max |r| {worst} (bound {bound}), mean |r| {mean}"
            );
        }
        free(lab);
    }
    free(rgb);
}
