//! #1 Lab phase 5a-ii: Image ▸ Mode ▸ Grayscale from a Lab Color
//! document (`lab_gray`), and Lab → RGB's pipeline built once, through the
//! FFI.
//!
//! The oracle is written here from the CIE formulas: each pixel's L\*
//! (`lab_fixture::decode`), `Y = f⁻¹((L* + 16) / 116)`
//! (`lab_fixture::f_inverse`), then a Gamma 2.2 Gray profile's tone curve
//! `v = Y^(1/2.2)` (`gray_fixture::gray_icc`).
//!
//! Bounds, declared before measuring:
//!
//! * (a) at 16 bits every gray code is within 32 of `65535 v`; at 8 bits
//!   every code is on the ×257 grid and within 257 of it; two pixels of equal
//!   L\* and different a\*/b\* give equal codes; alpha is kept (on the grid at
//!   8 bits);
//! * (b) the requested depth 8 or 16 is the result's, and a mask, a group,
//!   clipping and an alpha channel carry;
//! * (c) a Darker Color layer, a Blend If on Lightness and an adjustment
//!   layer each refuse with `flatten_required` and the exact sentence (the
//!   Blend If naming "Lightness"); `flatten: true` converts to one record;
//! * (d) a Color Overlay carries greyed exactly as RGB → Gray greys the same
//!   style on an sRGB document;
//! * (e) the plan's `refusal` is null with bytes > 0; a CMYK document
//!   refuses with the exact sentence;
//! * (f) `dr_precision_doc_convert_lab_to_rgb` into Display P3 (relative
//!   colorimetric, no BPC) equals `dr_precision_image_lab_display` of the
//!   same layer, sample for sample;
//! * (g) Photoshop's take (`~/devel/doctored-fixtures/photoshop/
//!   lab-gray-547/take1`, never committed; `scripts/photoshop-probe/
//!   lab_gray_take.py`): with `decided.json` VALID, each `codesN.tif` opened
//!   as Lab and converted into Dot Gain 20 % at its depth has every cell's
//!   centre 4 × 4 mean within 1 8-bit code of Photoshop's `gray.tif`. SKIPs
//!   when the take is absent.
//! * (h) phase 6a-ii, Convert to Profile's Gray destination: the conversion
//!   into a Gray working space by name (`dot_gain_20`) and into that
//!   space's own ICC bytes (`dr_precision_doc_gray_profile` of the first
//!   result) give equal codes and profile names at 8 and 16 bits.

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_gray::{
    dr_precision_doc_convert_to_gray, dr_precision_doc_convert_to_gray_plan,
    dr_precision_doc_gray_profile,
};
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_new_lab, dr_precision_doc_open_lab,
    dr_precision_image_lab_display,
};
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_image_free};
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::gray_fixture::gray_icc;
use crate::gray_ops::{edit, free, plan_json, result, LIMIT};
use crate::lab_fixture::{decode, f_inverse, grid};

/// (L\*, a\*, b\*) patches, one a pixel: the ends, a dark one under the
/// CIE knee, and two of equal L\* with different a\*/b\* (indices 3 and 4).
const PATCHES: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [5.0, 10.0, -10.0],
    [20.0, 40.0, 30.0],
    [50.0, 0.0, 0.0],
    [50.0, 60.0, -70.0],
    [75.0, -30.0, 50.0],
    [95.0, 5.0, 5.0],
    [100.0, 0.0, 0.0],
];
const W: u32 = 8;
const H: u32 = 2;

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    plan_json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err).unwrap()
}

/// The Lab twin profile, from a new Lab document.
fn twin() -> Arc<IccProfile> {
    let request = CString::new(r#"{"depth":16,"fill_srgb":[1,1,1,1]}"#).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(1, 1, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let profile = unsafe { &*doc }.profile().clone();
    free(doc);
    profile
}

/// One Lab colour as twin codes at `bits` (8 bits: on the ×257 grid).
fn lab_codes(lab: [f64; 3], alpha: u16, bits: u32) -> [u16; 4] {
    let unit = [
        lab[0] / 100.0,
        (lab[1] + 128.0) / 255.0,
        (lab[2] + 128.0) / 255.0,
    ];
    let code = |u: f64| {
        if bits == 8 {
            ((u * 255.0).round() * 257.0) as u16
        } else {
            (u * 65535.0).round() as u16
        }
    };
    [code(unit[0]), code(unit[1]), code(unit[2]), alpha]
}

/// Row 0 the patches opaque, row 1 the same at partial alpha (`seed` shifts
/// the patch order).
fn lab_pixels(bits: u32, seed: usize) -> Vec<[u16; 4]> {
    let partial = if bits == 8 { 160 * 257 } else { 40_123 };
    (0..W * H)
        .map(|i| {
            let patch = PATCHES[(i as usize % PATCHES.len() + seed) % PATCHES.len()];
            lab_codes(patch, if i < W { 65535 } else { partial }, bits)
        })
        .collect()
}

/// A Lab document of `names.len()` layers at `bits`.
fn lab_doc(bits: u32, names: &[&str]) -> *mut PrecisionDocument {
    let profile = twin();
    let layers = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let image =
                PrecisionImage::from_rgba16(W, H, lab_pixels(bits, i), profile.clone(), None)
                    .unwrap();
            Layer::raster(name, image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        profile,
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
    .with_lab_depth(bits)
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn changed(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    edit(doc, request, &[])
        .unwrap()
        .expect("the edit changes the document")
}

/// `dr_precision_doc_convert_to_gray` with `icc` (or none) and `options`.
fn to_gray_with(
    doc: *const PrecisionDocument,
    icc: Option<&[u8]>,
    options: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(options.to_string()).unwrap();
    let (data, len) = icc.map_or((ptr::null(), 0), |b| (b.as_ptr(), b.len()));
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray(
            doc,
            data,
            len,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(|d| d.expect("a Gray conversion always yields a document"))
}

fn gray_plan(doc: *const PrecisionDocument, icc: Option<&[u8]>, options: &Value) -> Value {
    let text = CString::new(options.to_string()).unwrap();
    let (data, len) = icc.map_or((ptr::null(), 0), |b| (b.as_ptr(), b.len()));
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_convert_to_gray_plan(doc, data, len, text.as_ptr(), &mut err) };
    plan_json(out, err).unwrap()
}

fn gamma22() -> Vec<u8> {
    gray_icc("Gamma 2.2 test", 2.2)
}

fn codes(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn mask_samples(doc: *const PrecisionDocument, index: usize) -> Vec<u16> {
    let layer = unsafe { &*doc }.layer(index).unwrap();
    layer.mask().unwrap().plane().samples().to_vec()
}

fn channel_samples(doc: *const PrecisionDocument) -> Vec<u16> {
    unsafe { &*doc }.channels()[0].plane().samples().to_vec()
}

/// The oracle's unit gray of a twin sample (module doc).
fn oracle(sample: [u16; 4]) -> f64 {
    let l_star = decode(sample)[0];
    f_inverse((l_star + 16.0) / 116.0).powf(1.0 / 2.2)
}

// ------------------------------------------------------------- (a) --

#[test]
fn lab_to_gray_is_lstar_through_the_gray_curve() {
    let icc = gamma22();
    for bits in [8u32, 16] {
        let lab = lab_doc(bits, &["Background"]);
        let source = codes(lab, 0);
        let gray = to_gray_with(lab, Some(&icc), &json!({ "depth": bits })).unwrap();
        let d = descriptor(gray);
        assert_eq!(d["color_mode"], "gray");
        let got = codes(gray, 0);
        for (i, (g, s)) in got.iter().zip(&source).enumerate() {
            assert!(g[0] == g[1] && g[1] == g[2], "pixel {i}: {g:?}");
            let want = 65535.0 * oracle(*s);
            let delta = (f64::from(g[0]) - want).abs();
            if bits == 8 {
                assert_eq!(g[0] % 257, 0, "8-bit pixel {i} off the grid: {}", g[0]);
                assert!(delta <= 257.0, "8-bit pixel {i}: {} against {want}", g[0]);
                assert_eq!(g[3], grid(s[3]));
            } else {
                assert!(delta <= 32.0, "16-bit pixel {i}: {} against {want}", g[0]);
                assert_eq!(g[3], s[3]);
            }
        }
        // Equal L*, different a*/b*: equal codes.
        assert_ne!(source[3][1], source[4][1]);
        assert_eq!(got[3][0], got[4][0], "{bits} bits");
        for doc in [gray, lab] {
            free(doc);
        }
    }
}

// ------------------------------------------------------------- (b) --

/// `doc` with an on-grid mask on layer `layer` and one on-grid alpha
/// channel; frees `doc`.
fn with_planes(doc: *mut PrecisionDocument, layer: usize) -> *mut PrecisionDocument {
    let mask: Vec<u16> = (0..W * H)
        .map(|i| ((i * 97 + 13) % 256 * 257) as u16)
        .collect();
    let masked = edit(
        doc,
        &json!({"op":"set_mask","layer":layer,"width":W,"height":H,"x":0,"y":0,"linked":true}),
        &mask,
    )
    .unwrap()
    .unwrap();
    let channel: Vec<u16> = (0..W * H)
        .map(|i| ((i * 41 + 7) % 256 * 257) as u16)
        .collect();
    let out = edit(
        masked,
        &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
        &channel,
    )
    .unwrap()
    .unwrap();
    free(masked);
    free(doc);
    out
}

#[test]
fn lab_to_gray_keeps_the_depth_and_carries_the_tree() {
    for bits in [8u32, 16] {
        let lab = with_planes(lab_doc(bits, &["Background", "Source", "Top"]), 1);
        let clipped = changed(lab, &json!({"op":"layer_clipped","layer":2,"clipped":true}));
        let grouped = changed(
            clipped,
            &json!({"op":"group_layers","layers":[1,2],"name":"Group","pass_through":false}),
        );
        for depth in [8u32, 16] {
            let gray = to_gray_with(
                grouped,
                None,
                &json!({"space":"gray_gamma_22","depth":depth}),
            )
            .unwrap();
            let d = descriptor(gray);
            assert_eq!(d["color_mode"], "gray", "Lab {bits} → Gray {depth}");
            assert_eq!(d["gray"]["depth"], depth);
            assert_eq!(d["layer_count"], descriptor(grouped)["layer_count"]);
            let doc = unsafe { &*gray };
            assert_eq!(doc.root_layers().len(), 2);
            assert_eq!(doc.root_layers()[1].children().unwrap().len(), 2);
            assert!(doc.layer(2).unwrap().clipped());
            assert_eq!(mask_samples(gray, 1), mask_samples(grouped, 1));
            assert_eq!(channel_samples(gray), channel_samples(grouped));
            free(gray);
        }
        for doc in [grouped, clipped, lab] {
            free(doc);
        }
    }
}

// ------------------------------------------------------------- (c) --

#[test]
fn darker_color_a_lightness_blend_if_and_an_adjustment_refuse_and_flatten_converts() {
    let icc = gamma22();
    let lab = lab_doc(16, &["Background", "Source", "Top"]);
    let darker = changed(
        lab,
        &json!({"op":"layer_blend","version":1,"layer":2,
            "blend_mode":BlendMode::DarkerColor.to_c()}),
    );
    let blend_if = changed(
        lab,
        &json!({"op":"layer_blend_if","version":1,"layer":1,
            "blend_if":{"channel":"red","this_layer":[20,60,255,255]}}),
    );
    let name = CString::new("Adjustment").unwrap();
    let meta = CString::new(
        json!({"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"gamma":1.2}})
            .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let adjusted = unsafe {
        dr_precision_doc_add_adjustment(
            lab,
            name.as_ptr(),
            meta.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let adjusted = result(adjusted, err).unwrap().unwrap();
    let cases = [
        (
            darker,
            "“Top” is a layer in the Darker Color blend mode, which a Grayscale document \
             cannot hold; flatten the image to convert it",
        ),
        (
            blend_if,
            "“Source” is a layer with Blend If on the Lightness channel, which a Grayscale \
             document cannot hold; flatten the image to convert it",
        ),
        (
            adjusted,
            "“Adjustment” is a Lab Color adjustment layer, which a Grayscale document \
             cannot hold; flatten the image to convert it",
        ),
    ];
    for (doc, sentence) in cases {
        let plan = gray_plan(doc, Some(&icc), &json!({}));
        assert_eq!(plan["refusal"], sentence);
        assert_eq!(plan["flatten_required"], true);
        assert_eq!(plan["flattens"], false);
        assert_eq!(
            to_gray_with(doc, Some(&icc), &json!({})).unwrap_err(),
            sentence
        );
        let flattening = gray_plan(doc, Some(&icc), &json!({"flatten":true}));
        assert_eq!(flattening["refusal"], Value::Null);
        assert_eq!(flattening["flattens"], true);
        let flat = to_gray_with(doc, Some(&icc), &json!({"flatten":true})).unwrap();
        assert_eq!(descriptor(flat)["color_mode"], "gray");
        assert_eq!(unsafe { &*flat }.root_layers().len(), 1);
        free(flat);
    }
    for doc in [adjusted, blend_if, darker, lab] {
        free(doc);
    }
}

// ------------------------------------------------------------- (d) --

fn stored(doc: *const PrecisionDocument, index: usize) -> Value {
    let text = unsafe { &*doc }
        .layer(index)
        .unwrap()
        .reported_style()
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn a_color_overlay_greys_as_rgb_to_gray_greys_it_on_srgb() {
    let icc = gamma22();
    let style = json!({"effects":[{"type":"color_overlay","color":"#d04020","opacity":0.6}]});
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let lab = lab_doc(16, &["Background", "Top"]);
    let lab_styled = changed(lab, &request);
    let srgb = IccProfile::srgb();
    let layers = (0..2)
        .map(|i| {
            let pixels: Vec<[u16; 4]> = (0..W * H)
                .map(|p| [(p * 3000) as u16, 20_000, (i * 9000) as u16, 65535])
                .collect();
            let image = PrecisionImage::from_rgba16(W, H, pixels, srgb.clone(), None).unwrap();
            Layer::raster(["Background", "Top"][i as usize], image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W,
            H,
            srgb.clone(),
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap(),
    ));
    let rgb_styled = changed(rgb, &request);
    let options = json!({"depth":16});
    let from_lab = to_gray_with(lab_styled, Some(&icc), &options).unwrap();
    let from_rgb = to_gray_with(rgb_styled, Some(&icc), &options).unwrap();
    let greyed = stored(from_rgb, 1);
    assert_ne!(greyed, stored(rgb_styled, 1), "RGB → Gray greys the colour");
    assert_eq!(stored(from_lab, 1), greyed);
    for doc in [from_rgb, from_lab, rgb_styled, rgb, lab_styled, lab] {
        free(doc);
    }
}

// ------------------------------------------------------------- (e) --

#[test]
fn the_plan_admits_a_lab_document_and_cmyk_refuses_by_the_new_sentence() {
    let lab = lab_doc(16, &["Background", "Top"]);
    let plan = gray_plan(lab, None, &json!({"space":"dot_gain_20","depth":16}));
    assert_eq!(plan["refusal"], Value::Null);
    assert_eq!(plan["flatten_required"], false);
    assert!(plan["working_memory_plan_bytes"].as_u64().unwrap() > 0);
    let press = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            lab,
            press.as_ptr(),
            press.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "{}", take_err_string(err));
    let sentence = "Grayscale converts an RGB or Lab Color document; convert to RGB Color first";
    let options = json!({"space":"dot_gain_20"});
    assert_eq!(gray_plan(cmyk, None, &options)["refusal"], sentence);
    assert_eq!(to_gray_with(cmyk, None, &options).unwrap_err(), sentence);
    for doc in [cmyk, lab] {
        free(doc);
    }
}

// ------------------------------------------------------------- (f) --

#[test]
fn lab_to_rgb_into_display_p3_equals_the_lab_view() {
    let lab = lab_doc(16, &["Background"]);
    let p3 = IccProfile::display_p3();
    let options =
        CString::new(r#"{"intent":"relative","black_point_compensation":false}"#).unwrap();
    let mut err = ptr::null_mut();
    let rgb = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            lab,
            p3.bytes().as_ptr(),
            p3.bytes().len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!rgb.is_null(), "{}", take_err_string(err));
    let source: *const PrecisionImage = unsafe { &*lab }.layer_source(0).unwrap();
    let mut err = ptr::null_mut();
    let view = unsafe { dr_precision_image_lab_display(source, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!view.is_null(), "{}", take_err_string(err));
    let want = unsafe { &*view }.to_rgba16().unwrap();
    assert_eq!(codes(rgb, 0), want);
    unsafe { dr_precision_image_free(view) };
    for doc in [rgb, lab] {
        free(doc);
    }
}

// ------------------------------------------------------------- (g) --

/// IFD 0 of an uncompressed one-sample 8/16-bit TIFF as unit values
/// (WhiteIsZero inverted): (width, height, plane).
fn read_gray_tiff(data: &[u8]) -> (usize, usize, Vec<f64>) {
    let little = &data[..2] == b"II";
    let u16_at = |at: usize| {
        let b = [data[at], data[at + 1]];
        if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    };
    let u32_at = |at: usize| {
        let b = [data[at], data[at + 1], data[at + 2], data[at + 3]];
        if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    let ifd = u32_at(4) as usize;
    let mut tags = std::collections::HashMap::new();
    for i in 0..usize::from(u16_at(ifd)) {
        let at = ifd + 2 + 12 * i;
        let (tag, kind, count) = (u16_at(at), u16_at(at + 2), u32_at(at + 4));
        let size = match kind {
            3 => 2,
            4 => 4,
            _ => 1,
        } * count as usize;
        let place = if size <= 4 {
            at + 8
        } else {
            u32_at(at + 8) as usize
        };
        let values: Vec<u32> = (0..count as usize)
            .map(|k| match kind {
                3 => u32::from(u16_at(place + 2 * k)),
                4 => u32_at(place + 4 * k),
                _ => u32::from(data[place + k]),
            })
            .collect();
        tags.insert(tag, values);
    }
    let one = |tag: u16, default: u32| tags.get(&tag).map_or(default, |v| v[0]);
    let (width, height) = (one(256, 0) as usize, one(257, 0) as usize);
    assert_eq!(one(259, 1), 1, "compressed");
    assert_eq!(one(277, 1), 1, "not one sample");
    let bits = one(258, 1);
    let offsets = &tags[&273];
    let counts = &tags[&279];
    let strips: Vec<u8> = offsets
        .iter()
        .zip(counts)
        .flat_map(|(&o, &n)| data[o as usize..(o + n) as usize].to_vec())
        .collect();
    let plane: Vec<f64> = match bits {
        8 => strips.iter().map(|&v| f64::from(v) / 255.0).collect(),
        16 => strips
            .chunks(2)
            .map(|b| {
                let v = if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                };
                f64::from(v) / 65535.0
            })
            .collect(),
        other => panic!("{other}-bit gray"),
    };
    let plane = if one(262, 1) == 0 {
        plane.iter().map(|v| 1.0 - v).collect()
    } else {
        plane
    };
    (width, height, plane[..width * height].to_vec())
}

/// Patch `k`'s centre 4 × 4 mean of `plane` (`float_selection.centre`).
fn centre(plane: &[f64], width: usize, k: usize) -> f64 {
    let (i, band) = (k % 13, k / 13);
    let mut sum = 0.0;
    for y in 8 * band + 2..8 * band + 6 {
        for x in 8 * i + 2..8 * i + 6 {
            sum += plane[y * width + x];
        }
    }
    sum / 16.0
}

#[test]
fn the_photoshop_take_reads_back_in_place() {
    let Some(root) = std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home).join("devel/doctored-fixtures/photoshop/lab-gray-547/take1")
    }) else {
        eprintln!("SKIP: HOME is unset, so the Photoshop take cannot be found");
        return;
    };
    let Some(decided) = std::fs::read_to_string(root.join("decided.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        eprintln!("SKIP: {} has no decided.json", root.display());
        return;
    };
    assert_eq!(decided["verdict"], "VALID", "{}", decided["verdict"]);
    for bits in [8u32, 16] {
        let folder = root.join(format!("jobs/lab{bits}"));
        let path = CString::new(
            folder
                .join(format!("codes{bits}.tif"))
                .to_str()
                .unwrap()
                .to_string(),
        )
        .unwrap();
        let mut err = ptr::null_mut();
        let lab =
            unsafe { dr_precision_doc_open_lab(path.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
        assert!(!lab.is_null(), "{}", take_err_string(err));
        assert_eq!(descriptor(lab)["lab"]["depth"], bits);
        let gray = to_gray_with(lab, None, &json!({"space":"dot_gain_20","depth":bits})).unwrap();
        let ours: Vec<f64> = codes(gray, 0)
            .iter()
            .map(|p| f64::from(p[0]) / 65535.0)
            .collect();
        let (width, height, theirs) =
            read_gray_tiff(&std::fs::read(folder.join("gray.tif")).unwrap());
        assert_eq!((width, height), (104, 24));
        assert_eq!(ours.len(), width * height);
        for k in 0..26 {
            let r = 255.0 * (centre(&ours, width, k) - centre(&theirs, width, k));
            assert!(r.abs() <= 1.0, "{bits} bits, cell {k}: r = {r}");
        }
        for doc in [gray, lab] {
            free(doc);
        }
    }
}

// ------------------------------------------------------------- (h) --

#[test]
fn convert_to_profile_into_a_gray_profile_equals_the_mode_route() {
    for bits in [8u32, 16] {
        let lab = lab_doc(bits, &["Background", "Top"]);
        let by_space =
            to_gray_with(lab, None, &json!({"space":"dot_gain_20","depth":bits})).unwrap();
        let mut length = 0usize;
        let pointer = unsafe { dr_precision_doc_gray_profile(by_space, &mut length) };
        assert!(!pointer.is_null() && length > 0, "{bits}: the Gray profile");
        let icc = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
        let by_profile = to_gray_with(lab, Some(&icc), &json!({ "depth": bits })).unwrap();
        let (a, b) = (descriptor(by_space), descriptor(by_profile));
        assert_eq!(a["color_mode"], "gray");
        assert_eq!(a["gray"]["depth"], bits);
        assert_eq!(
            a["gray"]["profile"], b["gray"]["profile"],
            "{bits}: profile name"
        );
        assert_eq!(a["gray"], b["gray"], "{bits}: gray descriptor");
        for index in 0..2 {
            assert_eq!(
                codes(by_space, index),
                codes(by_profile, index),
                "{bits}: layer {index}"
            );
        }
        for doc in [by_profile, by_space, lab] {
            free(doc);
        }
    }
}
