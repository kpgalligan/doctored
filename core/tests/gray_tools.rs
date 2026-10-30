//! #547 phase 2e, the rest of phase 2 on a Grayscale document: Stroke… on the
//! layer edge (`stroke_layer_edge`), Content-Aware Fill, the Remove tool's
//! model apply, the Art History Brush at 8 bits, RGB → Gray carrying
//! adjustment layers, HDR Toning at 8 and 16 bits, Import Channel, and the
//! default phase 6 of every refusal no phase names.
//!
//! The oracles are independent of the Gray arms: the RGB TWIN (the Gray
//! document converted to RGB through its own profile bytes, which keeps every
//! sample) running the same op with the Gray document's greyed ink, the byte
//! Art History Brush (`dr_doc_art_history_brush_layer`) on an 8-bit RGB
//! document of the same codes, the Rec. 709 luma (0.2126, 0.7152, 0.0722,
//! written here from the standard) of the twin's Remove result, sRGB's
//! published D50 Y row over the IEC decode through the Gray Gamma 2.2 curve
//! (`Y^(1/2.2)`) for a carried Gradient Map's stops, and a pixels-only
//! conversion with the same adjustments added through the adjustment FFI.
//!
//! Bounds, declared before measuring: exact equality with the twin for
//! Stroke…, Content-Aware Fill and HDR Toning; 1 code of 65535 against the
//! luma for Remove; 1 code of 255 against the byte master for the Art History
//! Brush and against the stop oracle; exact equality for the carried
//! adjustment render and for the imported channel's samples.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_gray::{
    dr_precision_doc_convert_to_gray, dr_precision_doc_convert_to_gray_plan,
};
use doctored_core::ffi_hdr_toning::{
    dr_precision_doc_hdr_toning, dr_precision_doc_hdr_toning_plan,
};
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_inspect::dr_precision_doc_adjustment_description;
use doctored_core::ffi_remove_model::dr_precision_doc_remove_model_apply;
use doctored_core::ffi_retouch_brush::{
    dr_doc_art_history_brush_layer, dr_precision_doc_art_history_brush,
    dr_precision_doc_art_history_brush_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{layer_pixels, take_doc, take_err_string};
use crate::gray_fixture::{srgb_decode, SRGB_Y_ROW};
use crate::gray_ops::{
    assert_neutral, edit, free, gray_doc, layer, plan_json, plan_samples, result, twin,
    uniform_gray, values, LIMIT, LUMA,
};

const W: u32 = 24;
const H: u32 = 16;
const RED: [f64; 4] = [1.0, 0.0, 0.0, 1.0];
const HUE: u32 = 23;
const LUMINOSITY: u32 = 26;
const HUE_REFUSAL: &str = "The Hue blend mode is unavailable in a Grayscale document.";

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

/// `request` with every input sample at full coverage.
fn covered(
    doc: *const PrecisionDocument,
    request: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let n = plan_samples(doc, request)?;
    edit(doc, request, &vec![65535; n]).map(|d| d.expect("the edit changed nothing"))
}

/// The greyed ink of `rgb` on `gray`, in the twin's terms, read back from a
/// full-coverage Normal Brush stroke (which stores the ink).
fn greyed(gray: *const PrecisionDocument, rgb: [f64; 4], depth: u32) -> [f64; 4] {
    let request = json!({"op":"paint_blend","layer":0,"color":rgb,"color_space":"srgb",
        "opacity":1.0,"blend_mode":0});
    let out = covered(gray, &request).unwrap();
    let p = values(out, 0, depth)[0];
    free(out);
    let g = if depth == 32 { p[0] } else { p[0] / 65535.0 };
    [g, g, g, rgb[3]]
}

fn stroke(color: [f64; 4], space: &str, mode: u32) -> Value {
    json!({"op":"stroke_layer_edge","layer":0,"width":3.0,"location":"center","color":color,
        "color_space":space,"opacity":0.75,"blend_mode":mode,"preserve_alpha":false})
}

#[test]
fn stroke_on_the_layer_edge_equals_the_twin_and_stays_neutral() {
    for depth in [16, 32] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let ink = greyed(gray, RED, depth);
        let space = if depth == 32 { "linear" } else { "document" };
        let what = format!("stroke at {depth}");
        let a = covered(gray, &stroke(RED, "srgb", 0)).unwrap_or_else(|e| panic!("{what}: {e}"));
        let b = covered(rgb, &stroke(ink, space, 0)).unwrap_or_else(|e| panic!("{what}: {e}"));
        let out = layer(a, 0, depth);
        assert_neutral(&out, &what);
        assert_ne!(out, layer(gray, 0, depth), "{what} changed nothing");
        let twin_out = layer(b, 0, depth);
        assert_eq!(out.len(), twin_out.len(), "{what}");
        for (i, (p, q)) in out.iter().zip(&twin_out).enumerate() {
            assert_eq!(p[0], q[0], "{what} pixel {i}");
            assert_eq!(p[3], q[3], "{what} pixel {i}");
        }
        assert_eq!(
            edit(gray, &stroke(RED, "srgb", HUE), &[]).unwrap_err(),
            HUE_REFUSAL,
            "{what}"
        );
        for d in [a, b, rgb, gray] {
            free(d);
        }
    }
}

/// A canvas-sized coverage of 65535 inside `(x, y, w, h)`.
fn hole(rect: (u32, u32, u32, u32)) -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let inside = x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3;
            if inside {
                65535
            } else {
                0
            }
        })
        .collect()
}

#[test]
fn content_aware_fill_equals_the_twin_and_refuses_a_plane() {
    let rect = (8, 5, 6, 5);
    let request = json!({"op":"content_aware_fill","layer":0,"x":rect.0,"y":rect.1,
        "width":rect.2,"height":rect.3,"seed":7});
    for depth in [8, 16, 32] {
        let what = format!("content-aware fill at {depth}");
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let n = plan_samples(gray, &request).unwrap_or_else(|e| panic!("{what}: {e}"));
        let input = if n == (W * H) as usize {
            hole(rect)
        } else {
            vec![65535; n]
        };
        let a = edit(gray, &request, &input).unwrap().expect("changed");
        let b = edit(rgb, &request, &input).unwrap().expect("twin changed");
        let out = layer(a, 0, depth);
        assert_neutral(&out, &what);
        assert_eq!(out, layer(b, 0, depth), "{what}");
        let mut plane = request.clone();
        plane["plane"] = json!("red");
        let refusal = edit(gray, &plane, &input).unwrap_err();
        assert!(
            refusal.starts_with("A colour plane is unavailable in a Grayscale document"),
            "{refusal}"
        );
        for d in [a, b, rgb, gray] {
            free(d);
        }
    }
}

const SIDE: u32 = 64;
const RW: u32 = 80;
const RH: u32 = 60;

/// A disc of full coverage with a half-covered rim.
fn disc() -> Vec<u16> {
    (0..RW * RH)
        .map(|i| {
            let (x, y) = (f64::from(i % RW) + 0.5, f64::from(i / RW) + 0.5);
            let d = (x - 40.0).powi(2) + (y - 30.0).powi(2);
            if d <= 49.0 {
                65535
            } else if d <= 81.0 {
                32768
            } else {
                0
            }
        })
        .collect()
}

/// A coloured stand-in for the model's plane.
fn coloured_plane() -> Vec<f32> {
    (0..3 * SIDE * SIDE)
        .map(|k| match k % 3 {
            0 => (k % 7) as f32 / 7.0,
            1 => (k % 5) as f32 / 5.0,
            _ => (k % 11) as f32 / 11.0,
        })
        .collect()
}

fn remove_apply(doc: *const PrecisionDocument, new_layer: bool) -> *mut PrecisionDocument {
    let coverage = disc();
    let plane = coloured_plane();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_remove_model_apply(
            doc,
            0,
            coverage.as_ptr(),
            coverage.len(),
            SIDE,
            plane.as_ptr(),
            plane.len(),
            new_layer,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "remove: {}", take_err_string(err));
    out
}

#[test]
fn remove_folds_the_models_colour_to_the_luma_of_the_twins_result() {
    for depth in [8, 16] {
        let gray = gray_doc(RW, RH, depth);
        let rgb = twin(gray);
        for new_layer in [false, true] {
            let what = format!("remove at {depth}, new layer {new_layer}");
            let index = usize::from(new_layer);
            let a = remove_apply(gray, new_layer);
            let b = remove_apply(rgb, new_layer);
            let out = layer(a, index, depth);
            assert_neutral(&out, &what);
            let twin_out = layer(b, index, depth);
            let mut coloured = 0;
            for (i, (p, q)) in out.iter().zip(&twin_out).enumerate() {
                let luma: f64 = (0..3).map(|c| LUMA[c] * f64::from(q[c])).sum();
                coloured += usize::from(q[0] != q[1] || q[1] != q[2]);
                assert!(
                    (f64::from(p[0]) - luma).abs() <= 1.0,
                    "{what} pixel {i}: {p:?} against the luma {luma} of {q:?}"
                );
                assert_eq!(p[3], q[3], "{what} pixel {i}");
            }
            assert!(coloured > 0, "{what}: the plane made no colour on the twin");
            free(a);
            free(b);
        }
        free(rgb);
        free(gray);
    }
}

const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

fn art_request(mode: u32) -> String {
    format!(
        r#"{{"version":1,"style":"tight_long","area":30,"tolerance":0,"opacity":1,"blend_mode":{mode},"seed":4}}"#
    )
}

const DABS: [f64; 8] = [32., 32., 5., 1., 40., 36., 5., 1.];

fn art_plan(
    doc: *const PrecisionDocument,
    target: usize,
    source: *const PrecisionDocument,
    request: &str,
) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_art_history_brush_plan(
            doc,
            target,
            source,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            DABS.as_ptr(),
            DABS.len() / 4,
            false,
            LIMIT,
            0,
            &mut err,
        )
    };
    plan_json(out, err)
}

fn art_typed(
    doc: *const PrecisionDocument,
    source: *const PrecisionDocument,
    request: &str,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_art_history_brush(
            doc,
            0,
            source,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            DABS.as_ptr(),
            DABS.len() / 4,
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
}

/// An 8-bit byte document of a Gray document's codes, `/257`.
fn byte_twin(gray: *const PrecisionDocument) -> DrDocument {
    let codes = layer(gray, 0, 8);
    let image = RgbaImage::from_fn(64, 64, |x, y| {
        let p = codes[(y * 64 + x) as usize];
        Rgba(p.map(|v| (v / 257) as u8))
    });
    DrDocument::from_pixels(image)
}

fn byte_art(current: &DrDocument, source: &DrDocument, request: &str) -> DrDocument {
    let (d, s) = (
        Box::into_raw(Box::new(current.clone())),
        Box::into_raw(Box::new(source.clone())),
    );
    let text = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_art_history_brush_layer(
            d,
            0,
            s,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            DABS.as_ptr(),
            DABS.len() / 4,
            ptr::null(),
            0,
            &mut err,
        )
    };
    unsafe {
        dr_doc_free(d);
        dr_doc_free(s);
    }
    let (doc, message) = take_doc(out, err);
    doc.unwrap_or_else(|| panic!("byte art history: {message:?}"))
}

#[test]
fn art_history_at_eight_bits_equals_the_byte_master() {
    let source = gray_doc(64, 64, 8);
    // Mid gray, so Darken and Lighten each change some pixels.
    let current = uniform_gray(64, 64, 8, 0.5);
    for mode in [0, 6, 7] {
        let what = format!("art history mode {mode}");
        let request = art_request(mode);
        let planned = art_plan(current, 0, source, &request).unwrap();
        assert_eq!(planned["input_samples"], 0, "{what}");
        let out = art_typed(current, source, &request)
            .unwrap()
            .expect("painted");
        let painted = layer(out, 0, 8);
        assert_neutral(&painted, &what);
        let bytes = byte_art(&byte_twin(current), &byte_twin(source), &request);
        let handle = Box::into_raw(Box::new(bytes));
        let master = layer_pixels(handle, 0);
        unsafe { dr_doc_free(handle) };
        let before = layer(current, 0, 8);
        let mut changed = 0;
        for (i, (p, q)) in painted.iter().zip(master.chunks_exact(4)).enumerate() {
            changed += usize::from(*p != before[i]);
            for c in [0, 3] {
                let got = f64::from(p[c]) / 257.0;
                assert!(
                    (got - f64::from(q[c])).abs() <= 1.0,
                    "{what} pixel {i} channel {c}: {p:?} against {q:?}"
                );
            }
        }
        assert!(changed > 0, "{what} changed nothing");
        free(out);
    }
    free(current);
    free(source);
}

#[test]
fn art_history_refuses_other_depths_modes_sources_and_targets() {
    let request = art_request(0);
    for (depth, sentence) in [
        (
            16,
            "The Art History Brush is unavailable in a 16-bit document.",
        ),
        (
            32,
            "The Art History Brush is unavailable in a 32-bit document.",
        ),
    ] {
        let gray = gray_doc(64, 64, depth);
        assert_eq!(art_plan(gray, 0, gray, &request).unwrap_err(), sentence);
        assert_eq!(art_typed(gray, gray, &request).unwrap_err(), sentence);
        free(gray);
    }
    let gray = gray_doc(64, 64, 8);
    assert_eq!(
        art_plan(gray, 0, gray, &art_request(HUE)).unwrap_err(),
        HUE_REFUSAL
    );
    assert_eq!(
        art_typed(gray, gray, &art_request(HUE)).unwrap_err(),
        HUE_REFUSAL
    );
    let rgb = twin(gray);
    let source = "The Art History Brush's source state is not a Grayscale raster";
    assert_eq!(art_plan(gray, 0, rgb, &request).unwrap_err(), source);
    assert_eq!(art_typed(gray, rgb, &request).unwrap_err(), source);
    let layered = add_adjustment(gray, "Levels", &levels(), 1.0);
    assert_eq!(
        art_plan(layered, 1, layered, &request).unwrap_err(),
        "The Art History Brush paints only a Grayscale raster layer"
    );
    for d in [layered, rgb, gray] {
        free(d);
    }
}

fn adjust(op: &str, params: Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

fn levels() -> String {
    adjust("levels", json!({"black":0.1,"white":0.9,"gamma":1.4}))
}

fn gradient_map() -> String {
    adjust(
        "gradient_map",
        json!({"gradient":{"stops":[{"position":0.0,"color":"#ff0000"},
            {"position":0.6,"color":"#20c040"},{"position":1.0,"color":"#00ffff"}]}}),
    )
}

fn add_adjustment(
    doc: *const PrecisionDocument,
    name: &str,
    meta: &str,
    opacity: f32,
) -> *mut PrecisionDocument {
    let name = CString::new(name).unwrap();
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            meta.as_ptr(),
            opacity,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "add adjustment: {}", take_err_string(err));
    out
}

fn stored(doc: *const PrecisionDocument, index: usize) -> String {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_adjustment_description(doc, index, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    text_of(out)
}

/// A 16-bit sRGB document of colours that vary in every channel.
fn rgb16() -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let r = (i * 7919 + 1013) % 251;
            let g = (i * 104_729 + 37) % 241;
            let b = (i * 1299 + 501) % 239;
            [(r * 257) as u16, (g * 265) as u16, (b * 270) as u16, 65535]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ))
}

const TO_GRAY: &str = r#"{"space":"gray_gamma_22","depth":16}"#;
const TO_GRAY_FLATTEN: &str = r#"{"space":"gray_gamma_22","depth":16,"flatten":true}"#;

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
    result(out, err).map(|d| d.expect("a conversion answers a document"))
}

fn to_gray_plan(doc: *const PrecisionDocument, options: &str) -> Value {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray_plan(doc, ptr::null(), 0, options.as_ptr(), &mut err)
    };
    plan_json(out, err).unwrap()
}

fn render16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// The stop oracle: sRGB's D50 Y row over the IEC decode, through Gray
/// Gamma 2.2, as an 8-bit code.
fn stop_gray(hex: &str) -> f64 {
    let rgb: Vec<f64> = (0..3)
        .map(|c| f64::from(u8::from_str_radix(&hex[1 + 2 * c..3 + 2 * c], 16).unwrap()) / 255.0)
        .collect();
    let y: f64 = (0..3).map(|c| SRGB_Y_ROW[c] * srgb_decode(rgb[c])).sum();
    y.powf(1.0 / 2.2) * 255.0
}

#[test]
fn rgb_to_gray_carries_adjustment_layers_by_the_mode_rule() {
    let rgb = rgb16();
    let with_levels = add_adjustment(rgb, "Levels", &levels(), 1.0);
    let layered = add_adjustment(with_levels, "Map", &gradient_map(), 0.5);
    let pixels_plan = to_gray_plan(rgb, TO_GRAY);
    let plan = to_gray_plan(layered, TO_GRAY);
    assert_eq!(plan["refusal"], Value::Null);
    assert_eq!(plan["flatten_required"], false);
    assert!(
        plan["working_memory_plan_bytes"].as_u64()
            > pixels_plan["working_memory_plan_bytes"].as_u64(),
        "the plan charges the carried records' preparation"
    );
    let gray = to_gray(layered, TO_GRAY).unwrap();
    let d = descriptor(gray);
    assert_eq!(d["color_mode"], "gray");
    assert_eq!(d["layer_count"], 3);
    assert_eq!(
        stored(gray, 1),
        stored(layered, 1),
        "Levels carries unchanged"
    );
    let map: Value = serde_json::from_str(&stored(gray, 2)).unwrap();
    let original: Value = serde_json::from_str(&gradient_map()).unwrap();
    assert_eq!(map["op"], "gradient_map");
    let stops = map["params"]["gradient"]["stops"].as_array().unwrap();
    let before = original["params"]["gradient"]["stops"].as_array().unwrap();
    assert_eq!(stops.len(), before.len());
    for (stop, was) in stops.iter().zip(before) {
        assert_eq!(stop["position"], was["position"]);
        let hex = stop["color"].as_str().unwrap();
        assert_eq!(&hex[1..3], &hex[3..5], "{hex}");
        assert_eq!(&hex[3..5], &hex[5..7], "{hex}");
        let got = f64::from(u8::from_str_radix(&hex[1..3], 16).unwrap());
        let want = stop_gray(was["color"].as_str().unwrap());
        assert!((got - want).abs() <= 1.0, "{hex} against {want}");
    }
    // The oracle: the pixels converted alone, then the same records added.
    let pixels = to_gray(rgb, TO_GRAY).unwrap();
    let a = add_adjustment(pixels, "Levels", &stored(gray, 1), 1.0);
    let b = add_adjustment(a, "Map", &stored(gray, 2), 0.5);
    let render = render16(gray);
    assert!(render.iter().all(|p| p[0] == p[1] && p[1] == p[2]));
    assert_eq!(render, render16(b));
    for doc in [b, a, pixels, gray, layered, with_levels, rgb] {
        free(doc);
    }
}

#[test]
fn rgb_to_gray_names_an_adjustment_layer_it_cannot_hold_and_flatten_converts() {
    let rgb = rgb16();
    let hue = add_adjustment(
        rgb,
        "Hue",
        &adjust(
            "hue_saturation",
            json!({"hue":20,"saturation":0.1,"lightness":0}),
        ),
        1.0,
    );
    let unlike = add_adjustment(
        rgb,
        "Levels",
        &adjust("levels", json!({"r":{"gamma":1.2}})),
        1.0,
    );
    let alike = add_adjustment(rgb, "Levels", &levels(), 1.0);
    let request = json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":LUMINOSITY});
    let luminosity = edit(alike, &request, &[])
        .unwrap()
        .expect("the mode changed");
    for (doc, sentence) in [
        (
            hue,
            "“Hue” cannot become Grayscale: Hue/Saturation is unavailable in a Grayscale document. Flatten the image to convert it",
        ),
        (
            unlike,
            "“Levels” cannot become Grayscale: Levels in a Grayscale document has one channel, Gray: set the composite, or r, g and b alike. Flatten the image to convert it",
        ),
        (
            luminosity,
            "“Levels” is a layer in the Luminosity blend mode, which a Grayscale document cannot hold; flatten the image to convert it",
        ),
    ] {
        let plan = to_gray_plan(doc, TO_GRAY);
        assert_eq!(plan["flatten_required"], true, "{sentence}");
        assert_eq!(plan["refusal"], sentence);
        assert_eq!(to_gray(doc, TO_GRAY).unwrap_err(), sentence);
        let flat = to_gray(doc, TO_GRAY_FLATTEN).unwrap();
        let d = descriptor(flat);
        assert_eq!(d["color_mode"], "gray");
        assert_eq!(d["layer_count"], 1);
        free(flat);
    }
    for doc in [luminosity, alike, unlike, hue, rgb] {
        free(doc);
    }
}

const TONING: [&str; 2] = [
    r#"{"method":"exposure_gamma","exposure":0.5,"gamma":1.2}"#,
    r#"{"method":"local_adaptation"}"#,
];

fn tone(doc: *const PrecisionDocument, request: &str) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_hdr_toning(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    result(out, err).map(|d| d.expect("toning answers a document"))
}

fn tone_plan(doc: *const PrecisionDocument, request: &str, bits: u32) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_hdr_toning_plan(doc, text.as_ptr(), bits, &mut err) };
    plan_json(out, err)
}

#[test]
fn hdr_toning_keeps_gray_and_its_depth_and_equals_the_twin() {
    for depth in [8, 16] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        for request in TONING {
            let what = format!("{request} at {depth}");
            let plan = tone_plan(gray, request, depth).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(plan["output_bytes"], u64::from(W * H) * 8, "{what}");
            let other = if depth == 8 { 16 } else { 8 };
            assert_eq!(
                tone_plan(gray, request, other).unwrap_err(),
                format!("a {depth}-bit Grayscale document keeps its depth through hdr toning")
            );
            let a = tone(gray, request).unwrap_or_else(|e| panic!("{what}: {e}"));
            let b = tone(rgb, request).unwrap_or_else(|e| panic!("{what} twin: {e}"));
            let d = descriptor(a);
            assert_eq!(d["color_mode"], "gray", "{what}");
            assert_eq!(d["gray"]["depth"], depth, "{what}");
            let out = layer(a, 0, depth);
            assert_neutral(&out, &what);
            let twin_out = layer(b, 0, 16);
            for (i, (p, q)) in out.iter().zip(&twin_out).enumerate() {
                assert_eq!((p[0], p[3]), (q[0], q[3]), "{what} pixel {i}");
            }
            free(a);
            free(b);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn import_channel_adds_a_channel_whose_samples_read_back() {
    for depth in [8, 16, 32] {
        let gray = gray_doc(W, H, depth);
        let samples: Vec<u16> = (0..W * H).map(|i| (i * 97 % 65536) as u16).collect();
        let provenance = json!({"version":1,"kind":"camera_auxiliary","name":"Portrait Matte",
            "source_format":"one_component16","source_bits":16,"source_width":W,
            "source_height":H,"precision_basis":"decoded_auxiliary_buffer",
            "encoded_precision":"not_inspected","exif_orientation":1,"orientation_applied":true,
            "coverage_mapping":"identity_unorm","alignment":"oriented_full_image_bilinear"})
        .to_string();
        let request = json!({"op":"import_channel","name":"Matte","width":W,"height":H,
            "provenance":provenance});
        let out = edit(gray, &request, &samples)
            .unwrap_or_else(|e| panic!("import at {depth}: {e}"))
            .expect("a channel was added");
        let doc = unsafe { &*out };
        assert!(doc.is_gray());
        assert_eq!(doc.channels().len(), 1);
        assert_eq!(doc.channels()[0].plane().samples(), samples.as_slice());
        free(out);
        free(gray);
    }
}

#[test]
fn a_scalar_read_of_the_gray_layer_refuses_for_good() {
    let gray = gray_doc(W, H, 16);
    assert_eq!(
        edit(gray, &json!({"op":"read_plane"}), &[]).unwrap_err(),
        "read_plane of the Gray layer is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, so a scalar read takes target mask or channel."
    );
    free(gray);
}
