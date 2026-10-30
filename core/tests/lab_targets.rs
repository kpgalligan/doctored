//! #1 Lab phases 2d and 2e, the Lab document's targets, through the FFI:
//! the scalar statistics of the red, green, blue and alpha planes (the L\*,
//! a\* and b\* codes and alpha), their plane read, a scalar paint and a Blur
//! on each (phase 2e) equal the same request on a 16-bit sRGB document
//! holding identical codes exactly; a filter on a layer
//! mask and on an alpha channel equals its sRGB-document twin exactly (Blur,
//! Unsharp Mask, and Emboss and Smart Sharpen, which run in passes on the
//! layer since phase 2f), and the rows that refuse for good refuse there in
//! their words; RGB → Lab carries Hue/Saturation, composite Levels and
//! grouped Brightness/Contrast layers byte-equal, rendering exactly as the
//! converted raster with the same layers added, and Gray → Lab carries a
//! Levels layer; an Exposure layer refuses in the plan and the conversion,
//! and Flatten converts (a Red/Green/Blue Levels or Curves layer carries
//! since phase 2f: `lab_blend_if.rs`).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_to_lab, dr_precision_doc_convert_to_lab_plan,
};
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_read_plane, dr_precision_doc_scalar_statistics,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{edit, free, gray_doc, plan_json, plan_samples, result, LIMIT};

pub(crate) const W: u32 = 32;
pub(crate) const H: u32 = 24;

fn text_of(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { dr_string_free(text) };
    out
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&text_of(text)).unwrap()
}

/// An sRGB document of one W × H layer holding exactly `pixels`.
pub(crate) fn srgb_doc(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

/// A pattern whose three channels all vary, opaque.
pub(crate) fn pattern() -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            [
                (x * 65535 / (W - 1)) as u16,
                ((y * 7001 + x * 977) % 65536) as u16,
                (((W - 1 - x) * 3731 + y * 5003) % 65536) as u16,
                65535,
            ]
        })
        .collect()
}

pub(crate) fn to_lab(
    doc: *const PrecisionDocument,
    options: &str,
) -> Result<*mut PrecisionDocument, String> {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err).map(|d| d.expect("the conversion answered nothing"))
}

fn to_lab_plan(doc: *const PrecisionDocument, options: &str) -> Value {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_convert_to_lab_plan(doc, options.as_ptr(), &mut err) };
    plan_json(out, err).unwrap()
}

/// The pattern as a 16-bit Lab document.
fn lab_doc() -> *mut PrecisionDocument {
    let rgb = srgb_doc(pattern());
    let lab = to_lab(rgb, r#"{"depth":16}"#).unwrap();
    free(rgb);
    lab
}

pub(crate) fn codes(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn statistics(doc: *const PrecisionDocument, target: &str) -> Result<Value, String> {
    let request = json!({"op":"scalar_statistics","target":target,"index":0});
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_scalar_statistics(
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
    plan_json(raw, err)
}

fn read_plane(doc: *const PrecisionDocument, target: &str) -> Result<Vec<u16>, String> {
    let request = json!({"op":"read_plane","target":target,"index":0});
    let text = CString::new(request.to_string()).unwrap();
    let mut out = vec![0u16; (W * H) as usize];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_read_plane(
            doc,
            text.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

fn changed(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> *mut PrecisionDocument {
    edit(doc, request, input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn description(op: &str, params: Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

fn add(doc: *const PrecisionDocument, meta: &str) -> *mut PrecisionDocument {
    let name = CString::new("Adjustment").unwrap();
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            meta.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
        .unwrap_or_else(|e| panic!("add {e}"))
        .unwrap()
}

/// Every record's adjustment description, in postorder.
fn records(doc: *const PrecisionDocument) -> Vec<Option<String>> {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .map(|i| doc.adjustment_description(i).map(str::to_string))
        .collect()
}

#[test]
fn every_plane_reads_and_edits_as_on_the_srgb_twin() {
    // The pattern with an alpha that varies, so a Blur on the alpha moves it.
    let translucent: Vec<[u16; 4]> = pattern()
        .into_iter()
        .enumerate()
        .map(|(i, p)| [p[0], p[1], p[2], (20000 + (i as u32 * 4099) % 45000) as u16])
        .collect();
    let rgb = srgb_doc(translucent);
    let lab = to_lab(rgb, r#"{"depth":16}"#).unwrap();
    free(rgb);
    let twin = srgb_doc(codes(lab, 0));
    for target in ["red", "green", "blue", "alpha"] {
        assert_eq!(
            statistics(lab, target).unwrap(),
            statistics(twin, target).unwrap(),
            "statistics {target}"
        );
        assert_eq!(
            read_plane(lab, target).unwrap(),
            read_plane(twin, target).unwrap(),
            "read_plane {target}"
        );
        let paint = json!({"op":"scalar_edit","target":target,"index":0,
            "operation":{"kind":"paint","value":40000,"opacity":0.7}});
        let n = plan_samples(lab, &paint).unwrap_or_else(|e| panic!("{target}: {e}"));
        let mut coverage = plane(3);
        coverage.resize(n, 65535);
        let blur = scalar_filter(target, "blur", json!({"sigma":2}));
        for (request, input) in [(paint, coverage), (blur, Vec::new())] {
            let [l, t] = [lab, twin].map(|doc| changed(doc, &request, &input));
            assert_eq!(codes(l, 0), codes(t, 0), "{request}");
            assert!(unsafe { &*l }.is_lab(), "{request}");
            free(l);
            free(t);
        }
    }
    free(twin);
    free(lab);
}

fn plane(seed: u32) -> Vec<u16> {
    (0..W * H)
        .map(|i| (i.wrapping_mul(2_654_435_761).wrapping_add(seed * 7919) % 65536) as u16)
        .collect()
}

/// `filter` on target `target` of both documents.
fn scalar_filter(target: &str, op: &str, params: Value) -> Value {
    json!({"op":"scalar_edit","target":target,"index":0,
        "operation":{"kind":"filter","edit":{"layer":0,"op":op,"params":params}}})
}

#[test]
fn a_mask_and_an_alpha_channel_filter_as_on_rgb() {
    let lab = lab_doc();
    let twin = srgb_doc(codes(lab, 0));
    let mask = json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true});
    let channel = json!({"op":"set_channel","index":null,"name":"Alpha 1"});
    let docs = [lab, twin].map(|doc| {
        let masked = changed(doc, &mask, &plane(1));
        let out = changed(masked, &channel, &plane(2));
        free(masked);
        out
    });
    let filters = [
        ("blur", json!({"sigma":2})),
        (
            "unsharp_mask",
            json!({"radius":2,"amount":1.5,"threshold":0}),
        ),
        ("emboss", json!({})),
        ("smart_sharpen", json!({})),
    ];
    for target in ["mask", "channel"] {
        for (op, params) in &filters {
            let request = scalar_filter(target, op, params.clone());
            let [l, t] = docs.map(|doc| changed(doc, &request, &[]));
            let got = read_plane(l, target).unwrap();
            assert_eq!(got, read_plane(t, target).unwrap(), "{target} {op}");
            assert_ne!(
                got,
                read_plane(docs[0], target).unwrap(),
                "{target} {op} moved"
            );
            // The layer itself is untouched.
            assert_eq!(codes(l, 0), codes(lab, 0), "{target} {op} layer");
            free(l);
            free(t);
        }
    }
    let refusals = [
        (
            "lens_correction",
            "Lens Correction is unavailable in a Lab Color document.",
        ),
        (
            "presence",
            "Presence is unavailable in a Lab Color document: it is a Develop Filter control, and the Develop Filter is unavailable in Lab.",
        ),
        (
            "depth_blur",
            "Depth Blur is unavailable in a Lab Color document: Doctored offers it on RGB documents only.",
        ),
        ("liquify", "liquify edits layer pixels only"),
    ];
    for (op, sentence) in refusals {
        // Liquify's strokes parse before the scalar path answers, as on RGB.
        let params = if op == "liquify" {
            json!({"strokes":[{"tool":"forward_warp","size":8,"strength":0.8,
                "points":[[4.0,4.0],[9.0,6.0]]}]})
        } else {
            json!({})
        };
        assert_eq!(
            edit(docs[0], &scalar_filter("mask", op, params), &[]).unwrap_err(),
            sentence,
            "mask {op}"
        );
    }
    for doc in docs {
        free(doc);
    }
    free(twin);
    free(lab);
}

/// The pattern as 16-bit sRGB with `layers` added in order (each a
/// description), then the records named by `group` grouped.
fn with_layers(
    base: *mut PrecisionDocument,
    layers: &[String],
    group: Option<usize>,
) -> *mut PrecisionDocument {
    let mut doc = base;
    for meta in layers {
        let next = add(doc, meta);
        free(doc);
        doc = next;
    }
    if let Some(index) = group {
        let next = changed(
            doc,
            &json!({"op":"group_layers","layers":[index],"name":"Group"}),
            &[],
        );
        free(doc);
        doc = next;
    }
    doc
}

#[test]
fn rgb_to_lab_carries_adjustment_layers() {
    let layers = [
        description("hue_saturation", json!({"hue":30})),
        description("levels", json!({"gamma":1.4})),
        description("bcs", json!({"brightness":0.1,"contrast":0.2})),
    ];
    let rgb = with_layers(srgb_doc(pattern()), &layers, Some(3));
    let plan = to_lab_plan(rgb, "{}");
    assert!(plan["refusal"].is_null(), "{plan}");
    assert_eq!(plan["flatten_required"], false);
    let lab = to_lab(rgb, "{}").unwrap();
    assert_eq!(descriptor(lab)["color_mode"], "lab");
    assert_eq!(records(lab), records(rgb), "every record byte-equal");
    // Converting the raster alone, then adding the same layers.
    let raster = srgb_doc(pattern());
    let converted = to_lab(raster, "{}").unwrap();
    let rebuilt = with_layers(converted, &layers, Some(3));
    assert_eq!(records(rebuilt), records(lab));
    assert_eq!(render(lab), render(rebuilt));
    for doc in [rebuilt, raster, lab, rgb] {
        free(doc);
    }
    // Gray → Lab carries a Levels layer.
    let gray = gray_doc(W, H, 16);
    let levels = description("levels", json!({"gamma":1.3}));
    let layered = with_layers(gray, std::slice::from_ref(&levels), None);
    let lab = to_lab(layered, "{}").unwrap();
    assert_eq!(descriptor(lab)["color_mode"], "lab");
    assert_eq!(records(lab)[1].as_deref(), Some(levels.as_str()));
    free(lab);
    free(layered);
}

#[test]
fn rgb_to_lab_names_the_layers_it_cannot_carry() {
    let cases = [
        (
            vec![description("exposure", json!({"exposure":0.5}))],
            None,
            "“Adjustment” cannot convert to Lab Color: Exposure is unavailable in a Lab Color document. Flatten the image to convert it.".to_string(),
        ),
    ];
    for (layers, group, sentence) in cases {
        let rgb = with_layers(srgb_doc(pattern()), &layers, group);
        let before = render(rgb);
        let plan = to_lab_plan(rgb, "{}");
        assert_eq!(plan["refusal"], sentence.as_str(), "{plan}");
        assert_eq!(plan["flatten_required"], true);
        assert_eq!(to_lab(rgb, "{}").unwrap_err(), sentence);
        assert_eq!(descriptor(rgb)["color_mode"], "rgb");
        assert_eq!(render(rgb), before);
        let flattened = to_lab(rgb, r#"{"flatten":true}"#).unwrap();
        let d = descriptor(flattened);
        assert_eq!(d["color_mode"], "lab");
        assert_eq!(d["layer_count"], 1);
        free(flattened);
        free(rgb);
    }
}
