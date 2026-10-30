//! #1 phase 2b, a Grayscale document's adjustments (`gray_adjust`): every
//! op the table runs, destructively and as a layer, equals the same op on
//! the RGB TWIN (the Gray document converted to RGB through its own profile
//! bytes, which keeps every sample) and keeps R = G = B — the oracle is the
//! RGB kernel on R = G = B, which is what a one-channel kernel must give; a
//! Gradient Map is compared against the twin running the stored, greyed
//! description. The adjustments Grayscale disables, the RGB-only ones and
//! unknown ops refuse with their exact sentences at plan, add, set and
//! apply; Shadows/Highlights and Replace Color refuse as a layer; Levels and
//! Curves refuse unequal r, g and b; the selected Auto Tone, Auto Contrast
//! and Equalize, and the host's two-call Auto Tone, equal the twin; Auto
//! Color and a filter refuse by name; a Levels layer survives `.rz` and Gray
//! → RGB, and takes a layer mask that blends its effect. Bounds (exact
//! equality; 2 codes for the masked blend) declared before measuring.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gray::*;
use doctored_core::ffi_insert::dr_precision_doc_add_adjustment_at;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_open,
    dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_adjustment_plan,
    dr_precision_doc_apply_adjustment, dr_precision_doc_set_adjustment,
};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_precision_inspect::{
    dr_precision_doc_adjustment_description, dr_precision_doc_profile,
};
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_auto_levels, dr_precision_doc_selected_edit,
    dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// sRGB colours (8-bit codes) whose grays spread over the range.
const PATCHES: [[u16; 3]; 12] = [
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
    [10, 20, 5],
    [220, 230, 240],
];

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

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

/// A document result: `Ok(None)` is NULL with no error (no change).
fn result(
    out: *mut PrecisionDocument,
    err: *mut c_char,
) -> Result<Option<*mut PrecisionDocument>, String> {
    if !out.is_null() {
        Ok(Some(out))
    } else if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

/// A Gray Gamma 2.2 document of `depth`: one row of the patches' grays.
fn gray_doc(depth: u32) -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = PATCHES
        .iter()
        .map(|p| [p[0] * 257, p[1] * 257, p[2] * 257, 65535])
        .collect();
    let image =
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let source = if depth == 32 {
        let mut err = ptr::null_mut();
        let float =
            unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!float.is_null(), "{}", take_err_string(err));
        free(rgb);
        float
    } else {
        rgb
    };
    let options = if depth == 32 {
        r#"{"space":"gray_gamma_22"}"#.to_string()
    } else {
        format!(r#"{{"space":"gray_gamma_22","depth":{depth}}}"#)
    };
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            source,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "to gray: {}", take_err_string(err));
    free(source);
    gray
}

/// The RGB twin: `gray` converted to RGB through its own profile's bytes.
fn twin(gray: *const PrecisionDocument) -> *mut PrecisionDocument {
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_profile(gray, &mut length) };
    to_rgb(gray, bytes, length)
}

fn to_rgb(
    gray: *const PrecisionDocument,
    bytes: *const u8,
    length: usize,
) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_gray_to_rgb(
            gray,
            bytes,
            length,
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

fn description(op: &str, params: Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

fn apply(doc: *const PrecisionDocument, meta: &str) -> Result<*mut PrecisionDocument, String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(doc, 0, meta.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err).map(|d| d.expect("the adjustment changed nothing"))
}

fn add(doc: *const PrecisionDocument, meta: &str) -> Result<*mut PrecisionDocument, String> {
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
    result(out, err).map(Option::unwrap)
}

fn add_at(doc: *const PrecisionDocument, meta: &str) -> Result<*mut PrecisionDocument, String> {
    let name = CString::new("Adjustment").unwrap();
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment_at(
            doc,
            name.as_ptr(),
            meta.as_ptr(),
            1.0,
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(Option::unwrap)
}

fn set(
    doc: *const PrecisionDocument,
    index: usize,
    meta: &str,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_adjustment(doc, index, meta.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err)
}

fn plan(
    doc: *const PrecisionDocument,
    index: i64,
    meta: &str,
    destructive: bool,
) -> Result<Value, String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_adjustment_plan(doc, index, meta.as_ptr(), destructive, &mut err)
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(serde_json::from_str(&text_of(out)).unwrap())
    }
}

fn stored(doc: *const PrecisionDocument, index: usize) -> String {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_adjustment_description(doc, index, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    text_of(out)
}

fn selected(
    doc: *const PrecisionDocument,
    request: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
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
    result(out, err).map(|d| d.expect("the selected edit changed nothing"))
}

fn selected_plan(doc: *const PrecisionDocument, request: &Value) -> Result<(), String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_selected_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        unsafe { dr_string_free(out) };
        Ok(())
    }
}

fn auto_levels(doc: *const PrecisionDocument, request: &Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_auto_levels(
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
    serde_json::from_str(&text_of(out)).unwrap()
}

/// Layer `index`'s samples: u16 codes, or the f32 bits at 32 bits.
fn layer(doc: *const PrecisionDocument, index: usize, depth: u32) -> Vec<[u32; 4]> {
    let image = unsafe { &*doc }.layer_source(index).unwrap();
    if depth == 32 {
        let pixels = image.to_rgba32().unwrap();
        pixels.iter().map(|p| p.map(f32::to_bits)).collect()
    } else {
        let pixels = image.to_rgba16().unwrap();
        pixels.iter().map(|p| p.map(u32::from)).collect()
    }
}

/// The composite: u16 codes, or the f32 bits at 32 bits.
fn render(doc: *const PrecisionDocument, depth: u32) -> Vec<[u32; 4]> {
    let image = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
    if depth == 32 {
        let pixels = image.to_rgba32().unwrap();
        pixels.iter().map(|p| p.map(f32::to_bits)).collect()
    } else {
        let pixels = image.to_rgba16().unwrap();
        pixels.iter().map(|p| p.map(u32::from)).collect()
    }
}

fn assert_neutral(pixels: &[[u32; 4]], what: &str) {
    for (i, p) in pixels.iter().enumerate() {
        assert!(p[0] == p[1] && p[1] == p[2], "{what} pixel {i}: {p:?}");
    }
}

/// Every op the table runs, with parameters that move every channel.
fn running() -> Vec<(&'static str, Value)> {
    vec![
        (
            "bcs",
            json!({"brightness":0.2,"contrast":0.3,"saturation":0.5}),
        ),
        ("levels", json!({"black":0.1,"white":0.9,"gamma":1.3})),
        (
            "levels",
            json!({"gamma":1.1,"r":{"gamma":0.8},"g":{"gamma":0.8},"b":{"gamma":0.8}}),
        ),
        ("curves", json!({"rgb":[[0,0],[64,40],[192,220],[255,255]]})),
        (
            "curves",
            json!({"r":[[0,10],[255,240]],"g":[[0,10],[255,240]],"b":[[0,10],[255,240]]}),
        ),
        (
            "exposure",
            json!({"exposure":0.5,"offset":0.01,"gamma":1.1}),
        ),
        ("invert", json!({})),
        ("posterize", json!({"levels":5})),
        ("threshold", json!({"level":0.4})),
        (
            "gradient_map",
            json!({"gradient":{"stops":[{"position":0.0,"color":"#ff0000"},
                {"position":0.6,"color":"#20c040"},{"position":1.0,"color":"#00ffff"}]}}),
        ),
        (
            "shadows_highlights",
            json!({"shadows":{"amount":0.5,"tone":0.5},"highlights":{"amount":0.3,"tone":0.5},
                "radius":2,"color":0.4}),
        ),
        (
            "replace_color",
            json!({"colors":["#808080"],"fuzziness":120,"hue":40,"saturation":0.5,
                "lightness":0.3}),
        ),
    ]
}

/// The description the twin runs for `meta` on `gray`: the gray document's
/// own stored description (a Gradient Map's stops greyed), read back from a
/// layer of it.
fn as_stored(gray: *const PrecisionDocument, meta: &str) -> String {
    let layered = add(gray, meta).unwrap();
    let out = stored(layered, 1);
    free(layered);
    out
}

#[test]
fn every_running_op_applied_equals_the_rgb_twin() {
    for depth in [8, 16, 32] {
        let gray = gray_doc(depth);
        let rgb = twin(gray);
        assert_eq!(
            layer(rgb, 0, depth),
            layer(gray, 0, depth),
            "the twin keeps every sample"
        );
        for (op, params) in running() {
            if depth != 16 && !matches!(op, "levels" | "curves" | "invert" | "gradient_map") {
                continue;
            }
            let what = format!("{op} at {depth}");
            let meta = description(op, params);
            let on_twin = if op == "gradient_map" {
                as_stored(gray, &meta)
            } else {
                meta.clone()
            };
            // An op the float document refuses (Invert at 32 bits) refuses
            // alike on the Gray document.
            let b = match apply(rgb, &on_twin) {
                Ok(b) => b,
                Err(e) if depth == 32 => {
                    assert_eq!(apply(gray, &meta).unwrap_err(), e, "{what}");
                    continue;
                }
                Err(e) => panic!("{what} twin: {e}"),
            };
            let a = apply(gray, &meta).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_neutral(&layer(a, 0, depth), &what);
            assert_eq!(layer(a, 0, depth), layer(b, 0, depth), "{what}");
            assert_eq!(descriptor(a)["color_mode"], "gray", "{what}");
            free(a);
            free(b);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn every_layer_op_renders_equal_to_the_rgb_twin() {
    for depth in [16, 32] {
        let gray = gray_doc(depth);
        let rgb = twin(gray);
        for (op, params) in running() {
            if matches!(op, "shadows_highlights" | "replace_color") {
                continue;
            }
            let what = format!("{op} layer at {depth}");
            let meta = description(op, params);
            let a = match add(gray, &meta) {
                Ok(a) => a,
                Err(e) if depth == 32 => {
                    assert_eq!(add(rgb, &meta).unwrap_err(), e, "{what}");
                    continue;
                }
                Err(e) => panic!("{what}: {e}"),
            };
            let b = add(rgb, &stored(a, 1)).unwrap_or_else(|e| panic!("{what} twin: {e}"));
            // A layer the float document cannot render (Invert at 32 bits)
            // refuses alike.
            let twin_render = unsafe { &*b }.render(LIMIT, 0, None).err();
            if let Some(e) = twin_render.filter(|_| depth == 32) {
                assert_eq!(
                    unsafe { &*a }.render(LIMIT, 0, None).err(),
                    Some(e),
                    "{what}"
                );
                free(a);
                free(b);
                continue;
            }
            let out = render(a, depth);
            assert_neutral(&out, &what);
            assert_eq!(out, render(b, depth), "{what}");
            free(a);
            free(b);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn a_gradient_maps_stored_stops_are_grey() {
    let gray = gray_doc(16);
    let meta = description(
        "gradient_map",
        json!({"gradient":{"stops":[{"position":0.0,"color":"#ff0000"},
            {"position":1.0,"color":"#00ffff"}]}}),
    );
    let stored: Value = serde_json::from_str(&as_stored(gray, &meta)).unwrap();
    let stops = stored["params"]["gradient"]["stops"].as_array().unwrap();
    assert_eq!(stops.len(), 2);
    for stop in stops {
        let hex = stop["color"].as_str().unwrap();
        assert_eq!(hex.len(), 7, "{hex}");
        assert_eq!(&hex[1..3], &hex[3..5], "{hex}");
        assert_eq!(&hex[3..5], &hex[5..7], "{hex}");
    }
    // Red is darker than cyan.
    assert!(stops[0]["color"].as_str().unwrap() < stops[1]["color"].as_str().unwrap());
    free(gray);
}

#[test]
fn refused_adjustments_name_their_reason_at_plan_add_set_and_apply() {
    let gray = gray_doc(16);
    let levels = add(gray, &description("levels", json!({"gamma":1.2}))).unwrap();
    let cases = [
        (
            description("hue_saturation", json!({"hue":10})),
            "Hue/Saturation is unavailable in a Grayscale document.",
        ),
        (
            description("black_and_white", json!({})),
            "Black & White is unavailable in a Grayscale document.",
        ),
        (
            description("sepia", json!({})),
            "Sepia is unavailable in a Grayscale document: Doctored offers it on RGB documents only.",
        ),
        (
            description("bogus", json!({})),
            "unsupported high-depth adjustment bogus",
        ),
        (
            description("levels", json!({"r":{"gamma":1.2}})),
            "Levels in a Grayscale document has one channel, Gray: set the composite, or r, g and b alike.",
        ),
        (
            description("levels", json!({"r":{"gamma":1.2},"g":{"gamma":1.2},"b":{"gamma":1.3}})),
            "Levels in a Grayscale document has one channel, Gray: set the composite, or r, g and b alike.",
        ),
        (
            description("curves", json!({"r_map":(0..256).collect::<Vec<u32>>()})),
            "Curves in a Grayscale document has one channel, Gray: set the composite, or r, g and b alike.",
        ),
    ];
    for (meta, sentence) in &cases {
        assert_eq!(
            plan(gray, -1, meta, false).unwrap_err(),
            *sentence,
            "{meta}"
        );
        assert_eq!(plan(gray, 0, meta, true).unwrap_err(), *sentence, "{meta}");
        assert_eq!(add(gray, meta).unwrap_err(), *sentence, "{meta}");
        assert_eq!(add_at(gray, meta).unwrap_err(), *sentence, "{meta}");
        assert_eq!(set(levels, 1, meta).unwrap_err(), *sentence, "{meta}");
        assert_eq!(apply(gray, meta).unwrap_err(), *sentence, "{meta}");
    }
    for (op, name) in [
        ("shadows_highlights", "Shadows/Highlights"),
        ("replace_color", "Replace Color"),
    ] {
        let params = running().into_iter().find(|(o, _)| *o == op).unwrap().1;
        let meta = description(op, params);
        let sentence = format!(
            "A {name} adjustment layer is unavailable in a Grayscale document: {name} applies to pixels only."
        );
        assert_eq!(plan(gray, -1, &meta, false).unwrap_err(), sentence);
        assert_eq!(add(gray, &meta).unwrap_err(), sentence);
        assert_eq!(add_at(gray, &meta).unwrap_err(), sentence);
        assert_eq!(set(levels, 1, &meta).unwrap_err(), sentence);
        assert!(
            plan(gray, 0, &meta, true).is_ok(),
            "{op} applies destructively"
        );
    }
    // Equal channels and the composite are admitted; an RGB document is untouched.
    let rgb = twin(gray);
    assert!(plan(rgb, -1, &cases[0].0, false).is_ok());
    let set_ok = set(levels, 1, &description("levels", json!({"gamma":0.9})))
        .unwrap()
        .unwrap();
    assert_eq!(descriptor(set_ok)["color_mode"], "gray");
    for doc in [set_ok, rgb, levels, gray] {
        free(doc);
    }
}

#[test]
fn selected_auto_tone_contrast_and_equalize_equal_the_rgb_twin() {
    let gray = gray_doc(16);
    let rgb = twin(gray);
    for request in [
        json!({"layer":0,"op":"auto_tone"}),
        json!({"layer":0,"op":"auto_contrast"}),
        json!({"layer":0,"op":"equalize","params":{"scope":"entire"}}),
        json!({"layer":0,"op":"adjustment","description":description("invert", json!({}))}),
    ] {
        let a = selected(gray, &request).unwrap_or_else(|e| panic!("{request}: {e}"));
        let b = selected(rgb, &request).unwrap_or_else(|e| panic!("{request} twin: {e}"));
        assert_neutral(&layer(a, 0, 16), &request.to_string());
        assert_eq!(layer(a, 0, 16), layer(b, 0, 16), "{request}");
        free(a);
        free(b);
    }
    // The host's Auto Tone and Auto Contrast: derive, then write the arrays.
    for op in ["auto_tone", "auto_contrast"] {
        let request = json!({"layer":0,"op":op});
        let derived = auto_levels(gray, &request);
        assert_eq!(derived, auto_levels(rgb, &request), "{op}");
        let write = json!({"layer":0,"op":"levels_channels","params":{
            "black":derived["black"],"white":derived["white"],"gamma":derived["gamma"]}});
        let a = selected(gray, &write).unwrap_or_else(|e| panic!("{op}: {e}"));
        let b = selected(rgb, &write).unwrap();
        assert_neutral(&layer(a, 0, 16), op);
        assert_eq!(layer(a, 0, 16), layer(b, 0, 16), "{op}");
        free(a);
        free(b);
    }
    let unequal = json!({"layer":0,"op":"levels_channels","params":{
        "black":[0.1,0.1,0.2],"white":[0.9,0.9,0.9],"gamma":[1.0,1.0,1.0]}});
    let levels =
        "Levels in a Grayscale document has one channel, Gray: set the composite, or r, g and b alike.";
    assert_eq!(selected(gray, &unequal).unwrap_err(), levels);
    assert_eq!(selected_plan(gray, &unequal).unwrap_err(), levels);
    assert_eq!(
        selected(gray, &json!({"layer":0,"op":"auto_color"})).unwrap_err(),
        "Auto Color is unavailable in a Grayscale document."
    );
    // Phase 4: Liquify is admitted and keeps R = G = B.
    let liquified = selected(
        gray,
        &json!({"layer":0,"op":"liquify","params":{"strokes":[
            {"tool":"forward_warp","size":18,"strength":0.8,"points":[[1.0,1.0],[2.0,1.0]]}]}}),
    )
    .unwrap();
    assert_neutral(&layer(liquified, 0, 16), "liquify");
    free(liquified);
    let hue = json!({"layer":0,"op":"adjustment",
        "description":description("hue_saturation", json!({"hue":10}))});
    assert_eq!(
        selected(gray, &hue).unwrap_err(),
        "Hue/Saturation is unavailable in a Grayscale document."
    );
    free(rgb);
    free(gray);
}

#[test]
fn a_levels_layer_survives_rz_and_gray_to_rgb() {
    let gray = gray_doc(16);
    let layered = add(gray, &description("levels", json!({"gamma":1.4}))).unwrap();
    let before = render(layered, 16);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("levels.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(layered, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let reopened = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    let d = descriptor(reopened);
    assert_eq!(d["color_mode"], "gray");
    assert_eq!(d["layer_count"], 2);
    assert_eq!(render(reopened, 16), before);
    let srgb = IccProfile::srgb();
    let rgb = to_rgb(layered, srgb.bytes().as_ptr(), srgb.bytes().len());
    let d = descriptor(rgb);
    assert_eq!(d["color_mode"], "rgb");
    assert_eq!(d["layer_count"], 2);
    for doc in [rgb, reopened, layered, gray] {
        free(doc);
    }
}

/// Layer ▸ Layer Mask on a Gray Levels layer: `mask_set` admits the
/// adjustment target as RGB and CMYK do, and the layer's effect is blended
/// by the mask — base + (adjusted − base) × m, within 2 codes of rounding.
#[test]
fn a_levels_layer_takes_a_mask() {
    let gray = gray_doc(16);
    let base = render(gray, 16);
    let layered = add(gray, &description("levels", json!({"gamma":1.4}))).unwrap();
    let adjusted = render(layered, 16);
    let mask: Vec<u16> = (0..PATCHES.len() as u16).map(|i| i * 5000).collect();
    let request = json!({"op":"set_mask","layer":1,"width":PATCHES.len(),"height":1,
        "x":0,"y":0,"linked":true})
    .to_string();
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let masked = unsafe {
        dr_precision_doc_edit(
            layered,
            request.as_ptr(),
            mask.as_ptr(),
            mask.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!masked.is_null(), "{}", take_err_string(err));
    assert_eq!(descriptor(masked)["color_mode"], "gray");
    let out = render(masked, 16);
    assert_neutral(&out, "masked levels");
    for x in 0..PATCHES.len() {
        let (b, a) = (f64::from(base[x][0]), f64::from(adjusted[x][0]));
        let want = b + (a - b) * f64::from(mask[x]) / 65535.0;
        let got = f64::from(out[x][0]);
        assert!((got - want).abs() <= 2.0, "x {x}: {got} against {want}");
    }
    for doc in [masked, layered, gray] {
        free(doc);
    }
}
