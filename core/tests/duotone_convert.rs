//! #547 Duotone phase 1b (`duotone_convert`, the Duotone export arms,
//! through the C API): the render through the inks, its blockers and its
//! flatten, Image ▸ Mode ▸ RGB, CMYK, Lab and Indexed Color from Duotone,
//! and an export through the inks.
//!
//! Oracles written here: the declared Neugebauer view (CIE Lab → XYZ, D50,
//! the ICC sRGB D50 colorants inverted, the IEC 61966-2-1 curve both ways:
//! sGray's tone curve for the coverage and the twin's encoder), worked per
//! leaf from each gray layer's own codes; each mode route is held against
//! the SAME route applied to an RGB document this file builds from the
//! oracle's codes. Tolerances, fixed before measuring: the view 32/65535 a
//! channel (1a's), a route 257 (one 8-bit code).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_duotone::*;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_encode,
    dr_precision_doc_export_plan,
};
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_indexed::dr_precision_doc_convert_to_indexed;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::duotone_document::{duotone, free, ink, view16, LIMIT};

const VIEW_TOLERANCE: i32 = 32;
const ROUTE_TOLERANCE: i32 = 257;
const W: u32 = 6;
const H: u32 = 2;

// ---- the oracle ---------------------------------------------------------------

const D50: [f64; 3] = [0.9642, 1.0, 0.8249];
const SRGB: [[f64; 3]; 3] = [
    [0.436035, 0.385117, 0.143036],
    [0.222485, 0.716888, 0.060608],
    [0.013916, 0.097061, 0.713928],
];

fn lab_xyz(lab: [f64; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let f = [fy + lab[1] / 500.0, fy, fy - lab[2] / 200.0];
    std::array::from_fn(|i| {
        let t = f[i];
        let v = if t * t * t > 216.0 / 24389.0 {
            t * t * t
        } else {
            (116.0 * t - 16.0) * 27.0 / 24389.0
        };
        v * D50[i]
    })
}

fn xyz_linear(xyz: [f64; 3]) -> [f64; 3] {
    let m = SRGB;
    let cof = |r: usize, c: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
        m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
    };
    let det: f64 = (0..3).map(|c| m[0][c] * cof(0, c)).sum();
    std::array::from_fn(|i| (0..3).map(|j| cof(j, i) / det * xyz[j]).sum())
}

fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(y: f64) -> f64 {
    if y <= 0.0031308 {
        12.92 * y
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    }
}

/// The declared view of u16 gray `g` under `inks` (straight curves, default
/// overprints), u16 RGB.
fn oracle(g: u16, inks: &[Value]) -> [u16; 3] {
    let k = 1.0 - f64::from(g) / 65535.0;
    let cover: Vec<f64> = inks
        .iter()
        .map(|ink| {
            let low = ink["curve"][0].as_f64().unwrap();
            let high = ink["curve"][12].as_f64().unwrap();
            1.0 - decode(1.0 - (low + (high - low) * k) / 100.0)
        })
        .collect();
    let paper = lab_xyz([100.0, 0.0, 0.0]);
    let colours: Vec<[f64; 3]> = inks
        .iter()
        .map(|ink| lab_xyz([0, 1, 2].map(|i| ink["lab"][i].as_f64().unwrap())))
        .collect();
    let mut xyz = [0.0; 3];
    for mask in 0..1usize << inks.len() {
        let mut primary = paper;
        let mut weight = 1.0;
        for (i, &c) in cover.iter().enumerate() {
            if mask & (1 << i) != 0 {
                weight *= c;
                for ch in 0..3 {
                    primary[ch] *= colours[i][ch] / paper[ch];
                }
            } else {
                weight *= 1.0 - c;
            }
        }
        for ch in 0..3 {
            xyz[ch] += weight * primary[ch];
        }
    }
    xyz_linear(xyz).map(|v| (encode(v.clamp(0.0, 1.0)) * 65535.0).round() as u16)
}

fn near(a: &[u16], b: &[u16], tolerance: i32, what: &str) {
    for (x, y) in a.iter().zip(b) {
        let d = (i32::from(*x) - i32::from(*y)).abs();
        assert!(d <= tolerance, "{what}: {a:?} against {b:?} (|Δ| {d})");
    }
}

// ---- documents ------------------------------------------------------------------

fn boxed(doc: PrecisionDocument) -> *mut PrecisionDocument {
    Box::into_raw(Box::new(doc))
}

fn neutral(name: &str, w: u32, h: u32, offset: (i32, i32), seed: u16) -> PrecisionDocumentLayer {
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let v = (i as u16).wrapping_mul(9001).wrapping_add(seed) % 65535;
            let a = if i % 4 == 3 { 40000 } else { 65535 };
            [v, v, v, a]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, offset, 1.0, true).unwrap()
}

/// A two-layer 16-bit sGray document (an RGB document of neutral layers
/// through Mode ▸ Grayscale).
fn layered_gray() -> *mut PrecisionDocument {
    gray_of(vec![
        neutral("Base", W, H, (0, 0), 1200),
        neutral("Top", 3, 2, (2, 0), 30000),
    ])
}

/// `layers` (neutral, sRGB) through Mode ▸ Grayscale into 16-bit sGray.
fn gray_of(layers: Vec<PrecisionDocumentLayer>) -> *mut PrecisionDocument {
    let rgb = boxed(
        PrecisionDocument::from_parts(
            W,
            H,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap(),
    );
    let options = CString::new(r#"{"space":"sgray","depth":16}"#).unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            rgb,
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
    free(rgb);
    gray
}

fn inks2() -> Vec<Value> {
    vec![ink(0), ink(1)]
}

fn json_of(out: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

fn doc_of(out: *mut PrecisionDocument, err: *mut c_char) -> Result<*mut PrecisionDocument, String> {
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn render_plan(doc: *const PrecisionDocument, flatten: bool) -> Value {
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_duotone_render_plan(doc, flatten, &mut err) },
        err,
    )
    .unwrap()
}

fn render(doc: *const PrecisionDocument, flatten: bool) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_duotone_render(doc, flatten, LIMIT, 0, ptr::null(), &mut err) };
    doc_of(out, err)
}

fn layer(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .expect("a raster")
        .to_rgba16()
        .unwrap()
}

fn profile(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_profile(doc, &mut length) };
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

/// An RGB document in `like`'s profile whose layers are the oracle of
/// `gray`'s layers, offsets and alpha kept.
fn oracle_doc(gray: *const PrecisionDocument, inks: &[Value]) -> *mut PrecisionDocument {
    let source = unsafe { &*gray };
    let twin = IccProfile::parse(&profile(gray))
        .map(std::sync::Arc::new)
        .unwrap();
    let mut layers = Vec::new();
    for index in 0..source.layer_count() {
        let record = source.layer(index).unwrap();
        let image = source.layer_source(index).unwrap();
        let (w, h) = image.dimensions();
        let pixels: Vec<[u16; 4]> = image
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| {
                let [r, g, b] = oracle(p[0], inks);
                [r, g, b, p[3]]
            })
            .collect();
        let image = PrecisionImage::from_rgba16(w, h, pixels, twin.clone(), None).unwrap();
        layers.push(
            PrecisionDocumentLayer::raster(record.name(), image, record.offset(), 1.0, true)
                .unwrap(),
        );
    }
    boxed(
        PrecisionDocument::from_parts(
            W,
            H,
            twin,
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap(),
    )
}

// ---- the tests --------------------------------------------------------------------

#[test]
fn the_render_is_the_declared_view_per_leaf() {
    let gray = layered_gray();
    let doc = duotone(gray, &json!({ "inks": inks2() }));
    let plan = render_plan(doc, false);
    assert_eq!(plan["refusal"], Value::Null);
    assert_eq!(plan["flatten_required"], false);
    let pixels = 6 * 2 + 3 * 2;
    assert_eq!(
        plan["working_memory_plan_bytes"],
        json!(pixels * 8 + 65_536 * 6 + 1024 * 1024)
    );
    let out = render(doc, false).unwrap();
    let descriptor = unsafe { &*out }.descriptor().unwrap();
    assert_ne!(descriptor["color_mode"], "gray", "{descriptor}");
    assert!(descriptor.get("duotone").is_none());
    assert_eq!(profile(out), profile(doc), "the render is in the twin");
    let source = unsafe { &*doc };
    assert_eq!(unsafe { &*out }.layer_count(), source.layer_count());
    for index in 0..source.layer_count() {
        let record = unsafe { &*out }.layer(index).unwrap();
        assert_eq!(record.name(), source.layer(index).unwrap().name());
        assert_eq!(record.offset(), source.layer(index).unwrap().offset());
        let grays = layer(doc, index);
        for (pixel, g) in layer(out, index).iter().zip(&grays) {
            near(&pixel[..3], &oracle(g[0], &inks2()), VIEW_TOLERANCE, "leaf");
            assert_eq!(pixel[3], g[3], "alpha kept");
        }
    }
    free(out);
    free(doc);
    free(gray);
}

#[test]
fn an_adjustment_layer_needs_the_flatten_and_the_flatten_is_the_view_of_the_composite() {
    let gray = layered_gray();
    let name = CString::new("Levels").unwrap();
    let levels = CString::new(
        r#"{"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"black":0.1,"white":0.9,"gamma":1.25}}"#,
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let adjusted = unsafe {
        dr_precision_doc_add_adjustment(
            gray,
            name.as_ptr(),
            levels.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!adjusted.is_null(), "{}", take_err_string(err));
    let doc = duotone(adjusted, &json!({ "inks": inks2() }));
    let plan = render_plan(doc, false);
    let sentence = "“Levels” is an adjustment layer, which a Duotone document's conversion through its inks cannot carry; flatten the image to convert it";
    assert_eq!(plan["refusal"], sentence);
    assert_eq!(plan["flatten_required"], true);
    assert_eq!(plan["flattens"], false);
    assert_eq!(render(doc, false).unwrap_err(), sentence);
    let flattening = render_plan(doc, true);
    assert_eq!(flattening["refusal"], Value::Null);
    assert_eq!(flattening["flattens"], true);
    let out = render(doc, true).unwrap();
    assert_eq!(unsafe { &*out }.layer_count(), 1);
    assert_eq!(
        layer(out, 0),
        view16(doc).unwrap(),
        "the view of the composite"
    );
    free(out);
    free(doc);
    free(adjusted);
    free(gray);
}

#[test]
fn a_document_that_is_not_duotone_refuses_the_render() {
    let gray = layered_gray();
    assert_eq!(
        render_plan(gray, false)["refusal"],
        "the Duotone render takes a Duotone document"
    );
    assert_eq!(
        render(gray, false).unwrap_err(),
        "the Duotone render takes a Duotone document"
    );
    free(gray);
}

fn to_rgb(doc: *const PrecisionDocument, options: &str) -> Result<*mut PrecisionDocument, String> {
    let icc = IccProfile::srgb();
    let bytes = icc.bytes();
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_duotone_to_rgb(
            doc,
            bytes.as_ptr(),
            bytes.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    doc_of(out, err)
}

fn profile_convert(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let icc = IccProfile::srgb();
    let bytes = icc.bytes();
    let mut err = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_precision_color::dr_precision_doc_convert_to_profile(
            doc,
            bytes.as_ptr(),
            bytes.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        assert!(err.is_null(), "{}", take_err_string(err));
        // An identity pipeline changes nothing; the samples are the doc's.
        return boxed(unsafe { &*doc }.clone());
    }
    out
}

fn to_cmyk(doc: *const PrecisionDocument, icc: &[u8]) -> *mut PrecisionDocument {
    let options = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            icc.as_ptr(),
            icc.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    doc_of(out, err).unwrap()
}

fn to_lab(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let options = CString::new(r#"{"depth":16}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    doc_of(out, err).unwrap()
}

fn to_indexed(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let options = CString::new(r#"{"palette":"exact","dither":"none"}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_indexed(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    doc_of(out, err).unwrap()
}

fn flat(samples: Vec<[u16; 4]>) -> Vec<u16> {
    samples.into_iter().flatten().collect()
}

#[test]
fn rgb_cmyk_lab_and_indexed_are_their_routes_over_the_render() {
    let gray = layered_gray();
    let doc = duotone(gray, &json!({ "inks": inks2() }));
    let expected = oracle_doc(doc, &inks2());
    // RGB Color: the render, then Convert to Profile into sRGB.
    let rgb = to_rgb(doc, "{}").unwrap();
    let want = profile_convert(expected);
    assert_eq!(profile(rgb), IccProfile::srgb().bytes().to_vec());
    for index in 0..2 {
        near(
            &flat(layer(rgb, index)),
            &flat(layer(want, index)),
            ROUTE_TOLERANCE,
            "rgb",
        );
    }
    let mut err = ptr::null_mut();
    let icc = IccProfile::srgb();
    let options = CString::new(r#"{"flatten":false}"#).unwrap();
    let plan = json_of(
        unsafe {
            dr_precision_doc_convert_duotone_to_rgb_plan(
                doc,
                icc.bytes().as_ptr(),
                icc.bytes().len(),
                options.as_ptr(),
                &mut err,
            )
        },
        err,
    )
    .unwrap();
    assert_eq!(plan["refusal"], Value::Null);
    assert!(
        plan["working_memory_plan_bytes"].as_u64().unwrap()
            > render_plan(doc, false)["working_memory_plan_bytes"]
                .as_u64()
                .unwrap()
    );
    assert!(to_rgb(doc, r#"{"bogus":1}"#).unwrap_err().contains("bogus"));
    // CMYK, Lab and Indexed: the existing route over the render.
    let rendered = render(doc, false).unwrap();
    let press = press_profile("Duotone Press");
    let (a, b) = (to_cmyk(rendered, &press), to_cmyk(expected, &press));
    for index in 0..2 {
        let left: Vec<u16> = unsafe { &*a }
            .cmyk_layer_samples(index)
            .unwrap()
            .into_iter()
            .flatten()
            .collect();
        let right: Vec<u16> = unsafe { &*b }
            .cmyk_layer_samples(index)
            .unwrap()
            .into_iter()
            .flatten()
            .collect();
        near(&left, &right, ROUTE_TOLERANCE, "cmyk");
    }
    free(a);
    free(b);
    let (a, b) = (to_lab(rendered), to_lab(expected));
    for index in 0..2 {
        near(
            &flat(layer(a, index)),
            &flat(layer(b, index)),
            ROUTE_TOLERANCE,
            "lab",
        );
    }
    free(a);
    free(b);
    // Indexed Color holds one layer: a one-layer, opaque document.
    let single_gray = gray_of(vec![opaque("Base")]);
    let single = duotone(single_gray, &json!({ "inks": inks2() }));
    let one = render(single, false).unwrap();
    let expected_one = oracle_doc(single, &inks2());
    let (a, b) = (to_indexed(one), to_indexed(expected_one));
    near(
        &flat(layer(a, 0)),
        &flat(layer(b, 0)),
        ROUTE_TOLERANCE,
        "indexed",
    );
    free(a);
    free(b);
    free(one);
    free(expected_one);
    free(single);
    free(single_gray);
    free(rendered);
    free(want);
    free(rgb);
    free(expected);
    free(doc);
    free(gray);
}

/// An opaque canvas-sized neutral layer.
fn opaque(name: &str) -> PrecisionDocumentLayer {
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let v = (i as u16) * 5000;
            [v, v, v, 65535]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, (0, 0), 1.0, true).unwrap()
}

fn export(doc: *const PrecisionDocument, request: &Value) -> Result<Vec<u8>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let result: *mut ExportResult = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    let mut len = 0usize;
    let bytes = unsafe {
        let p = dr_export_result_bytes(result, &mut len);
        std::slice::from_raw_parts(p, len).to_vec()
    };
    unsafe { dr_export_result_free(result) };
    Ok(bytes)
}

fn export_plan(doc: *const PrecisionDocument, request: &Value) -> Result<Value, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_export_plan(doc, ptr::null(), request.as_ptr(), &mut err) },
        err,
    )
}

#[test]
fn exports_go_through_the_inks_in_every_format() {
    let gray = layered_gray();
    let doc = duotone(gray, &json!({ "inks": inks2() }));
    // PNG16, profile document: the view of the projection, exactly.
    let png = export(
        doc,
        &json!({"format":"png","bit_depth":16,"profile":"document"}),
    )
    .unwrap();
    let decoded = image::load_from_memory(&png).unwrap().to_rgba16();
    assert_eq!((decoded.width(), decoded.height()), (W, H));
    let pixels: Vec<[u16; 4]> = decoded.pixels().map(|p| p.0).collect();
    assert_eq!(pixels, view16(doc).unwrap());
    // WebP, profile document: RGB, no Grayscale one-channel refusal, and the
    // plan is the RGB document's plus the view's scratch.
    let webp = json!({"format":"webp","bit_depth":8,"profile":"document"});
    let bytes = export(doc, &webp).unwrap();
    assert_eq!(&bytes[8..12], b"WEBP");
    let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
    assert_eq!((decoded.width(), decoded.height()), (W, H));
    let plan = export_plan(doc, &webp).unwrap();
    let rgb = render(doc, true).unwrap();
    // The RGB document with the same storage: the Gray source the Duotone
    // document shares every sample with, then its twin.
    let plain = doctored_core::ffi_gray::dr_precision_doc_convert_gray_to_rgb;
    let back = gray;
    let twin_bytes = profile(back);
    let mut err = ptr::null_mut();
    let twin = unsafe {
        plain(
            back,
            twin_bytes.as_ptr(),
            twin_bytes.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!twin.is_null(), "{}", take_err_string(err));
    let twin_plan = export_plan(twin, &webp).unwrap();
    assert_eq!(
        plan["scratch_bytes"].as_u64().unwrap(),
        twin_plan["scratch_bytes"].as_u64().unwrap()
            + u64::from(W * H) * 8
            + 65_536 * 6
            + 1024 * 1024
    );
    // A plain Gray document's WebP with profile document still refuses.
    assert!(export(back, &webp).unwrap_err().contains("one-channel"));
    free(twin);
    free(rgb);
    free(doc);
    free(gray);
}
