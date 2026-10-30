//! #1's Duotone close (`duotone_gray`, through `ffi_duotone`): Image ▸
//! Mode ▸ Grayscale on a Duotone document renders the inks into the gray,
//! and Bitmap… is that rule followed by Gray → Bitmap.
//!
//! Oracles, each independent of the code under test, tolerances fixed before
//! measuring: Monotone Black on a straight curve keeps every gray code
//! (exact at 8 bits, ±1 u16 at 16, since the ink's coverage is the tone
//! curve's decode and the result its encode); for in-gamut inks the result is
//! the render through the inks (`dr_precision_doc_duotone_render`) then RGB →
//! Gray (`dr_precision_doc_convert_to_gray`) into the same Gray profile,
//! ±257 u16; each layer of a two-layer document is the one-layer conversion
//! of its own pixels (`remove_layers` of the other), its mask, opacity, mode,
//! alpha channel and the guides carried; a blocker's flatten is the
//! conversion of `{"op":"flatten"}`'s document; Bitmap is
//! `dr_precision_doc_convert_to_bitmap` of the Grayscale result, exactly; the
//! plans are their declared terms.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_bitmap::{
    dr_precision_doc_convert_to_bitmap, dr_precision_doc_convert_to_bitmap_plan,
};
use doctored_core::ffi_duotone::*;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_guide::{
    dr_precision_doc_add_guide, dr_precision_doc_guide_count, dr_precision_doc_guide_position,
};
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::duotone_document::{
    descriptor, duotone, free, gray_doc, ink, line, monotone_black, render16, LIMIT,
};

const W: u32 = 6;
const H: u32 = 4;
const BOOKKEEPING: u64 = 1024 * 1024;
const NOT_DUOTONE_GRAY: &str = "the Duotone Grayscale conversion takes a Duotone document";
const NOT_DUOTONE_BITMAP: &str = "the Duotone Bitmap conversion takes a Duotone document";
const FLATTEN: &str = "Bitmap holds one layer: flatten the image.";
const EIGHT_FIRST: &str =
    "Bitmap converts an 8-bit Grayscale document; convert to 8 Bits/Channel first.";

fn inks2() -> Value {
    json!({"inks": [ink(0), ink(1)]})
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

fn gray_plan(doc: *const PrecisionDocument, flatten: bool) -> Value {
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_convert_duotone_to_gray_plan(doc, flatten, &mut err) },
        err,
    )
    .unwrap()
}

fn to_gray(doc: *const PrecisionDocument, flatten: bool) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_duotone_to_gray(doc, flatten, LIMIT, 0, ptr::null(), &mut err)
    };
    doc_of(out, err)
}

fn bitmap_plan(doc: *const PrecisionDocument, options: &Value) -> Value {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_convert_duotone_to_bitmap_plan(doc, text.as_ptr(), &mut err) },
        err,
    )
    .unwrap()
}

fn to_bitmap(
    doc: *const PrecisionDocument,
    options: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_duotone_to_bitmap(
            doc,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    doc_of(out, err)
}

/// Gray → Bitmap, the direct route (the oracle).
fn plain_bitmap(doc: *const PrecisionDocument, options: &Value) -> *mut PrecisionDocument {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_bitmap(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    doc_of(out, err).unwrap()
}

fn plain_bitmap_plan(doc: *const PrecisionDocument, options: &Value) -> Value {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_convert_to_bitmap_plan(doc, text.as_ptr(), &mut err) },
        err,
    )
    .unwrap()
}

fn edit_with(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    doc_of(out, err).unwrap_or_else(|e| panic!("{request}: {e}"))
}

fn layer16(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
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

fn neutral(
    name: &str,
    (w, h): (u32, u32),
    offset: (i32, i32),
    seed: u16,
    opacity: f32,
) -> PrecisionDocumentLayer {
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let v = (i as u16).wrapping_mul(9001).wrapping_add(seed) % 65535;
            let a = if i % 4 == 3 { 40000 } else { 65535 };
            [v, v, v, a]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, offset, opacity, true).unwrap()
}

/// A two-layer sGray document of `depth` (an sRGB document of neutral
/// layers through Mode ▸ Grayscale), the top at 0.6 opacity.
fn layered_gray(depth: u32) -> *mut PrecisionDocument {
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W,
            H,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![
                neutral("Base", (W, H), (0, 0), 1200, 1.0),
                neutral("Top", (3, 2), (2, 1), 30000, 0.6),
            ],
        )
        .unwrap(),
    ));
    let options = CString::new(format!(r#"{{"space":"sgray","depth":{depth}}}"#)).unwrap();
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

/// `layered_gray(16)` with a mask and Multiply on the top layer, an alpha
/// channel and a vertical guide.
fn dressed_gray() -> *mut PrecisionDocument {
    let gray = layered_gray(16);
    let mask: Vec<u16> = (0..6u16).map(|i| i * 11000).collect();
    let masked = edit_with(
        gray,
        &json!({"op":"set_mask","layer":1,"width":3,"height":2,"x":2,"y":1,"linked":true}),
        &mask,
    );
    let multiplied = edit_with(
        masked,
        &json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":1}),
        &[],
    );
    let plane: Vec<u16> = (0..W * H).map(|i| (i * 2500) as u16).collect();
    let channel = edit_with(
        multiplied,
        &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
        &plane,
    );
    let guided = unsafe { dr_precision_doc_add_guide(channel, 1, 3.5) };
    assert!(!guided.is_null());
    for doc in [channel, multiplied, masked, gray] {
        free(doc);
    }
    guided
}

fn mask_samples(doc: *const PrecisionDocument, index: usize) -> Option<Vec<u16>> {
    unsafe { &*doc }
        .layer(index)
        .unwrap()
        .mask()
        .map(|m| m.plane().samples().to_vec())
}

// ---- Grayscale --------------------------------------------------------------------

#[test]
fn monotone_black_keeps_every_gray_code() {
    let eight: Vec<u16> = (0..=255).collect();
    let mut sixteen: Vec<u16> = (0..255).map(|i| 128 + i * 257).collect();
    sixteen.extend([0, 65535]);
    for (codes, depth) in [(eight, 8u8), (sixteen, 16u8)] {
        let gray = gray_doc(&codes, depth, None);
        let doc = duotone(gray, &monotone_black());
        let out = to_gray(doc, false).unwrap();
        let shown = descriptor(out);
        assert_eq!(shown["color_mode"], "gray");
        assert!(shown.get("duotone").is_none(), "the inks are dropped");
        assert_eq!(
            shown["gray"],
            descriptor(gray)["gray"],
            "the same profile and depth"
        );
        assert_eq!(profile(out), profile(gray));
        let (want, got) = (render16(gray), render16(out));
        for (w, g) in want.iter().zip(&got) {
            let d = (i32::from(w[0]) - i32::from(g[0])).abs();
            let bound = if depth == 8 { 0 } else { 1 };
            assert!(d <= bound, "{depth} bits: {} became {}", w[0], g[0]);
            assert!(g[0] == g[1] && g[1] == g[2], "R = G = B");
            assert_eq!(w[3], g[3], "alpha kept");
        }
        free(out);
        free(doc);
        free(gray);
    }
}

#[test]
fn in_gamut_inks_are_the_render_then_rgb_to_gray() {
    let codes: Vec<u16> = (0..64).map(|i| i * 1040).chain([65535]).collect();
    let gray = gray_doc(&codes, 16, None);
    let options = json!({"inks": [
        {"name": "Warm Black", "lab": [20, 2, 4], "curve": line(0.0, 100.0)},
        {"name": "Sand", "lab": [60, 10, 20], "curve": line(0.0, 70.0)},
    ]});
    let doc = duotone(gray, &options);
    let out = to_gray(doc, false).unwrap();
    let mut err = ptr::null_mut();
    let rendered = doc_of(
        unsafe { dr_precision_doc_duotone_render(doc, false, LIMIT, 0, ptr::null(), &mut err) },
        err,
    )
    .unwrap();
    let request = CString::new(r#"{"space":"sgray","depth":16}"#).unwrap();
    let mut err = ptr::null_mut();
    let oracle = doc_of(
        unsafe {
            dr_precision_doc_convert_to_gray(
                rendered,
                ptr::null(),
                0,
                request.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        err,
    )
    .unwrap();
    assert_eq!(
        descriptor(oracle)["gray"],
        descriptor(out)["gray"],
        "the same Gray profile"
    );
    for (o, g) in render16(oracle).iter().zip(render16(out)) {
        let d = (i32::from(o[0]) - i32::from(g[0])).abs();
        assert!(d <= 257, "{} against the route's {}", g[0], o[0]);
    }
    for d in [oracle, rendered, out, doc, gray] {
        free(d);
    }
}

#[test]
fn each_layer_is_its_own_conversion_and_the_structure_carries() {
    let gray = dressed_gray();
    let doc = duotone(gray, &inks2());
    let out = to_gray(doc, false).unwrap();
    let (source, result) = unsafe { (&*doc, &*out) };
    assert!(descriptor(out).get("duotone").is_none());
    assert_eq!(descriptor(out)["gray"], descriptor(doc)["gray"]);
    assert_eq!(profile(out), profile(doc));
    assert_eq!(result.layer_count(), 2);
    for index in 0..2 {
        let (a, b) = (source.layer(index).unwrap(), result.layer(index).unwrap());
        assert_eq!(a.name(), b.name());
        assert_eq!(a.offset(), b.offset());
        assert_eq!(a.opacity(), b.opacity());
        assert!(a.blend_mode() == b.blend_mode(), "layer {index}: the mode");
        assert_eq!(
            mask_samples(doc, index),
            mask_samples(out, index),
            "layer {index}: the mask"
        );
        // The one-layer conversion of this layer's own pixels.
        let single = edit_with(
            doc,
            &json!({"op":"remove_layers","layers":[1 - index]}),
            &[],
        );
        let alone = to_gray(single, false).unwrap();
        assert_eq!(layer16(out, index), layer16(alone, 0), "layer {index}");
        free(alone);
        free(single);
    }
    assert_eq!(result.layer(1).unwrap().opacity(), 0.6);
    assert!(mask_samples(out, 1).is_some());
    let (a, b) = (source.channels(), result.channels());
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].name(), b[0].name());
    assert_eq!(a[0].plane().samples(), b[0].plane().samples());
    unsafe {
        assert_eq!(dr_precision_doc_guide_count(out), 1);
        assert_eq!(dr_precision_doc_guide_position(out, 0), 3.5);
    }
    // Undo is the host's: the source is unchanged.
    assert!(descriptor(doc).get("duotone").is_some());
    for d in [out, doc, gray] {
        free(d);
    }
}

#[test]
fn a_blocker_needs_the_flatten_and_the_flatten_is_the_flattened_documents_conversion() {
    let gray = layered_gray(16);
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
    let doc = duotone(adjusted, &inks2());
    let sentence = "“Levels” is an adjustment layer, which a Duotone document's conversion through its inks cannot carry; flatten the image to convert it";
    let plan = gray_plan(doc, false);
    assert_eq!(plan["refusal"], sentence);
    assert_eq!(plan["flatten_required"], true);
    assert_eq!(plan["flattens"], false);
    assert_eq!(to_gray(doc, false).unwrap_err(), sentence);
    let plan = gray_plan(doc, true);
    assert_eq!(plan["refusal"], Value::Null);
    assert_eq!(plan["flattens"], true);
    let out = to_gray(doc, true).unwrap();
    let flattened = edit_with(doc, &json!({"op":"flatten"}), &[]);
    let oracle = to_gray(flattened, false).unwrap();
    assert_eq!(unsafe { &*out }.layer_count(), 1);
    assert_eq!(render16(out), render16(oracle));
    assert!(descriptor(out).get("duotone").is_none());
    for d in [oracle, flattened, out, doc, adjusted, gray] {
        free(d);
    }
}

#[test]
fn a_document_that_is_not_duotone_refuses_grayscale_and_bitmap() {
    let gray = gray_doc(&[0, 128, 255], 8, None);
    assert_eq!(gray_plan(gray, false)["refusal"], NOT_DUOTONE_GRAY);
    assert_eq!(to_gray(gray, false).unwrap_err(), NOT_DUOTONE_GRAY);
    assert_eq!(bitmap_plan(gray, &json!({}))["refusal"], NOT_DUOTONE_BITMAP);
    assert_eq!(to_bitmap(gray, &json!({})).unwrap_err(), NOT_DUOTONE_BITMAP);
    free(gray);
}

// ---- Bitmap -----------------------------------------------------------------------

#[test]
fn bitmap_is_bitmap_of_the_grayscale_result() {
    let codes: Vec<u16> = (0..=255).step_by(3).collect();
    let gray = gray_doc(&codes, 8, None);
    let doc = duotone(gray, &inks2());
    let toned = to_gray(doc, false).unwrap();
    for options in [
        json!({"method":"threshold"}),
        json!({}),
        json!({"method":"pattern"}),
    ] {
        let plan = bitmap_plan(doc, &options);
        assert_eq!(plan["refusal"], Value::Null, "{options}");
        assert_eq!(plan["flatten_required"], false);
        assert_eq!(
            (plan["width"].clone(), plan["height"].clone()),
            (json!(codes.len()), json!(1))
        );
        let out = to_bitmap(doc, &options).unwrap();
        let oracle = plain_bitmap(toned, &options);
        let shown = descriptor(out);
        assert_eq!(
            shown["color_mode"],
            descriptor(oracle)["color_mode"],
            "{options}"
        );
        assert!(shown.get("duotone").is_none(), "{options}: no inks");
        assert_eq!(render16(out), render16(oracle), "{options}");
        free(oracle);
        free(out);
    }
    for d in [toned, doc, gray] {
        free(d);
    }
}

#[test]
fn bitmap_refuses_16_bits_and_more_than_one_layer_without_the_flatten() {
    let gray16 = gray_doc(&[0, 30000, 65535], 16, None);
    let doc16 = duotone(gray16, &inks2());
    assert_eq!(bitmap_plan(doc16, &json!({}))["refusal"], EIGHT_FIRST);
    assert_eq!(to_bitmap(doc16, &json!({})).unwrap_err(), EIGHT_FIRST);
    let layered = layered_gray(8);
    let doc = duotone(layered, &inks2());
    let plan = bitmap_plan(doc, &json!({}));
    assert_eq!(plan["refusal"], FLATTEN);
    assert_eq!(plan["flatten_required"], true);
    assert_eq!(to_bitmap(doc, &json!({})).unwrap_err(), FLATTEN);
    let flattening = json!({"flatten": true});
    let plan = bitmap_plan(doc, &flattening);
    assert_eq!(plan["refusal"], Value::Null);
    assert_eq!(plan["flattens"], true);
    let out = to_bitmap(doc, &flattening).unwrap();
    // The oracle: Flatten Image on the Duotone document, Grayscale, Bitmap.
    let flattened = edit_with(doc, &json!({"op":"flatten"}), &[]);
    let toned = to_gray(flattened, false).unwrap();
    let oracle = plain_bitmap(toned, &json!({}));
    assert_eq!(render16(out), render16(oracle));
    for d in [oracle, toned, flattened, out, doc, layered, doc16, gray16] {
        free(d);
    }
}

// ---- plans and NULL ---------------------------------------------------------------

#[test]
fn the_plans_are_their_declared_terms() {
    let gray = layered_gray(16);
    let doc = duotone(gray, &inks2());
    let pixels = u64::from(W * H) + 3 * 2;
    let canvas = u64::from(W * H);
    assert_eq!(
        gray_plan(doc, false)["working_memory_plan_bytes"],
        json!(pixels * 8 + 65_536 * 2 + canvas * 31 + BOOKKEEPING)
    );
    let codes: Vec<u16> = (0..=255).step_by(5).collect();
    let gray8 = gray_doc(&codes, 8, None);
    let doc8 = duotone(gray8, &inks2());
    let options = json!({"method":"diffusion"});
    // The twin is the Gray source: the same samples without the inks.
    let twin = plain_bitmap_plan(gray8, &options)["working_memory_plan_bytes"]
        .as_u64()
        .unwrap();
    let n = codes.len() as u64;
    assert_eq!(
        bitmap_plan(doc8, &options)["working_memory_plan_bytes"],
        json!(n * 8 + 65_536 * 2 + BOOKKEEPING + twin)
    );
    for d in [doc8, gray8, doc, gray] {
        free(d);
    }
}

#[test]
fn every_new_export_tolerates_null() {
    unsafe {
        let mut err = ptr::null_mut();
        assert!(
            dr_precision_doc_convert_duotone_to_gray_plan(ptr::null(), false, &mut err).is_null()
        );
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(
            dr_precision_doc_convert_duotone_to_gray_plan(ptr::null(), true, ptr::null_mut())
                .is_null()
        );
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_convert_duotone_to_gray(
            ptr::null(),
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_convert_duotone_to_gray(
            ptr::null(),
            false,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_convert_duotone_to_bitmap_plan(
            ptr::null(),
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_convert_duotone_to_bitmap_plan(
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_convert_duotone_to_bitmap(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_convert_duotone_to_bitmap(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        // NULL Bitmap options on a live Duotone document are the defaults.
        let gray = gray_doc(&[0, 128, 255], 8, None);
        let doc = duotone(gray, &inks2());
        let mut err = ptr::null_mut();
        let plan = json_of(
            dr_precision_doc_convert_duotone_to_bitmap_plan(doc, ptr::null(), &mut err),
            err,
        )
        .unwrap();
        assert_eq!(plan["refusal"], Value::Null);
        free(doc);
        free(gray);
    }
}
