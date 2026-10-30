//! #1 phase 1b, a Grayscale document's layer structure: the 21 blend
//! modes of `GRAY_BLEND_MODES` (measured by phase 2's take) composite
//! through the one kernel on R = G = B and land
//! within 2 u16 codes of the independent W3C oracle
//! (`common/native_blend_oracle.rs`) on the gray value; Darker Color,
//! Lighter Color, Hue, Saturation, Color and Luminosity refuse permanently by
//! name; a 32-bit Gray document refuses the
//! float-refused modes with the float sentence; a mask composites as
//! `b + (s − b)·α·m`; Edit ▸ Fill lands on the hand-computed luminance of
//! the colour (sRGB's published D50 row over the IEC decode, then Gray
//! Gamma 2.2's curve) — ≤ 4/65535 at 16 bits, ≤ 1 code on the ×257 grid at
//! 8, ≤ 1e-4 linear at 32; blending-options styles and (phase 2d) effects
//! are admitted, a Blend If off Gray refused; merges and stamps stay R = G =
//! B; the `.rz` round
//! trip of a group, a mask, Multiply, a clip and Fill Opacity; and the gate's
//! phase sentences. Bounds declared before measuring.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gray::*;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_open,
    dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::require_gray;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{srgb_decode, SRGB_Y_ROW};
use crate::native_blend_oracle::{self, DISSOLVE_BAND};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// The 21 modes a Gray document offers, as their C codes, menu order.
const GRAY_MODES: [i32; 21] = [
    0, 14, 6, 1, 11, 15, 7, 2, 10, 12, 3, 4, 5, 18, 19, 20, 21, 8, 9, 13, 22,
];

/// The sentence of a mode a Grayscale document does not offer.
fn not_offered(name: &str) -> String {
    format!("The {name} blend mode is unavailable in a Grayscale document.")
}

/// (backdrop, source) pairs, unit gray: interior values, no luminance tie
/// and every Hard Mix sum at least 0.05 from 1.
const PAIRS: [(f64, f64); 8] = [
    (0.25, 0.70),
    (0.70, 0.25),
    (0.50, 0.35),
    (0.40, 0.80),
    (0.85, 0.55),
    (0.15, 0.45),
    (0.60, 0.90),
    (0.33, 0.10),
];

const OPACITY: f64 = 0.6;

fn json_of(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err)
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn edit_with(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Result<*mut PrecisionDocument, String> {
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
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn edit(doc: *const PrecisionDocument, request: &Value) -> Result<*mut PrecisionDocument, String> {
    edit_with(doc, request, &[])
}

/// An edit whose payload is full coverage of the plan's `input_samples`.
fn edit_covered(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = json_of(
        unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) },
        err,
    );
    let n = plan["input_samples"].as_u64().unwrap() as usize;
    edit_with(doc, request, &vec![65535u16; n]).unwrap()
}

fn render16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn codes(doc: *const PrecisionDocument, layer: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(layer)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn to_gray(doc: *const PrecisionDocument, options: &str) -> *mut PrecisionDocument {
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
    assert!(!out.is_null(), "to gray: {}", take_err_string(err));
    out
}

fn new_gray(depth: u32, w: u32, h: u32) -> *mut PrecisionDocument {
    let request = CString::new(format!(
        r#"{{"depth":{depth},"space":"gray_gamma_22","fill_srgb":[1,1,1,1]}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_gray(w, h, ptr::null(), 0, request.as_ptr(), LIMIT, 0, &mut err)
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn neutral_layer(name: &str, units: &[f64], opacity: f32) -> Layer {
    let pixels: Vec<[u16; 4]> = units
        .iter()
        .map(|u| {
            let c = (u * 65535.0).round() as u16;
            [c, c, c, 65535]
        })
        .collect();
    Layer::raster(
        name,
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap(),
        (0, 0),
        opacity,
        true,
    )
    .unwrap()
}

/// A Gray document (sGray, `depth`) of two neutral rows: the backdrop and
/// the source at 0.6 opacity, each pair of `PAIRS` per pixel.
fn two_layer_gray(depth: u32) -> *mut PrecisionDocument {
    let base: Vec<f64> = PAIRS.iter().map(|p| p.0).collect();
    let top: Vec<f64> = PAIRS.iter().map(|p| p.1).collect();
    let rgb = PrecisionDocument::from_parts(
        PAIRS.len() as u32,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            neutral_layer("Backdrop", &base, 1.0),
            neutral_layer("Source", &top, OPACITY as f32),
        ],
    )
    .unwrap();
    let rgb = Box::into_raw(Box::new(rgb));
    let gray = if depth == 32 {
        let mut err = ptr::null_mut();
        let float =
            unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!float.is_null(), "{}", take_err_string(err));
        let gray = to_gray(float, r#"{"space":"sgray"}"#);
        free(float);
        gray
    } else {
        to_gray(rgb, &format!(r#"{{"space":"sgray","depth":{depth}}}"#))
    };
    free(rgb);
    assert_eq!(descriptor(gray)["color_mode"], "gray");
    gray
}

fn blend(layer: usize, mode: i32) -> Value {
    json!({"op":"layer_blend","version":1,"layer":layer,"blend_mode":mode})
}

fn assert_neutral(pixels: &[[u16; 4]], what: &str) {
    for (x, p) in pixels.iter().enumerate() {
        assert!(p[0] == p[1] && p[1] == p[2], "{what} x {x}: {p:?}");
    }
}

#[test]
fn every_gray_mode_composites_the_gray_value_within_two_codes_of_the_oracle() {
    let gray = two_layer_gray(16);
    let base = codes(gray, 0);
    let top = codes(gray, 1);
    for mode in GRAY_MODES {
        // Normal is the source's own mode: the edit changes nothing.
        let doc = if mode == 0 {
            gray
        } else {
            edit(gray, &blend(1, mode)).unwrap_or_else(|e| panic!("mode {mode}: {e}"))
        };
        let out = render16(doc);
        assert_neutral(&out, &format!("mode {mode}"));
        for x in 0..PAIRS.len() {
            let cb = f64::from(base[x][0]) / 65535.0;
            let cs = f64::from(top[x][0]) / 65535.0;
            let got = f64::from(out[x][0]);
            if mode == 14 {
                // Dissolve: the source shown whole, or the backdrop untouched.
                let t = native_blend_oracle::dissolve_threshold(x as i64, 0);
                let shown = out[x][0] == top[x][0];
                assert!(shown || out[x][0] == base[x][0], "dissolve x {x}");
                if (OPACITY - t).abs() >= DISSOLVE_BAND {
                    assert_eq!(shown, t < OPACITY, "dissolve x {x} t {t}");
                }
                continue;
            }
            let b = match mode {
                21 => native_blend_oracle::threshold(mode, [cb; 3], [cs; 3]).0[0],
                _ => native_blend_oracle::separable(mode, cb, cs),
            };
            let want = (cb + (b - cb) * OPACITY) * 65535.0;
            assert!(
                (got - want).abs() <= 2.0,
                "mode {mode} x {x}: {got} against {want}"
            );
        }
        assert_eq!(descriptor(doc)["color_mode"], "gray");
        if mode != 0 {
            free(doc);
        }
    }
    free(gray);
}

#[test]
fn the_colour_modes_refuse_by_name_and_32_bits_refuses_the_float_refused_modes() {
    let gray = two_layer_gray(16);
    for (mode, name) in [
        (16, "Darker Color"),
        (17, "Lighter Color"),
        (23, "Hue"),
        (24, "Saturation"),
        (25, "Color"),
        (26, "Luminosity"),
    ] {
        assert_eq!(edit(gray, &blend(1, mode)).unwrap_err(), not_offered(name));
    }
    free(gray);
    let gray = two_layer_gray(32);
    assert_eq!(descriptor(gray)["gray"]["depth"], 32);
    assert_eq!(
        edit(gray, &blend(1, 2)).unwrap_err(),
        "the screen blend mode is unavailable in a 32-bit float document"
    );
    assert_eq!(edit(gray, &blend(1, 23)).unwrap_err(), not_offered("Hue"));
    assert_eq!(
        edit(gray, &blend(1, 16)).unwrap_err(),
        not_offered("Darker Color")
    );
    let multiplied = edit(gray, &blend(1, 1)).unwrap();
    let out = unsafe { &*multiplied }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .unwrap();
    for (x, p) in out.iter().enumerate() {
        assert!(p[0] == p[1] && p[1] == p[2], "32-bit Multiply x {x}: {p:?}");
    }
    free(multiplied);
    free(gray);
}

#[test]
fn a_masked_source_composites_by_its_mask() {
    let gray = two_layer_gray(16);
    let base = codes(gray, 0);
    let top = codes(gray, 1);
    let mask: Vec<u16> = (0..PAIRS.len() as u16).map(|i| i * 9000).collect();
    let masked = edit_with(
        gray,
        &json!({"op":"set_mask","layer":1,"width":PAIRS.len(),"height":1,"x":0,"y":0,"linked":true}),
        &mask,
    )
    .unwrap();
    let out = render16(masked);
    assert_neutral(&out, "mask");
    for x in 0..PAIRS.len() {
        let (b, s) = (f64::from(base[x][0]), f64::from(top[x][0]));
        let m = f64::from(mask[x]) / 65535.0;
        let want = b + (s - b) * OPACITY * m;
        let got = f64::from(out[x][0]);
        assert!((got - want).abs() <= 2.0, "x {x}: {got} against {want}");
    }
    // Edit ▸ Fill of the mask with black hides the source.
    let hidden = edit(
        masked,
        &json!({"op":"scalar_edit","target":"mask","index":1,
            "operation":{"kind":"fill","value":0,"opacity":1.0},"selection":false}),
    )
    .unwrap();
    let base_only = gray_base_only(gray);
    assert_eq!(render16(hidden), render16(base_only));
    free(base_only);
    free(hidden);
    free(masked);
    free(gray);
}

/// `gray` with its source hidden: the backdrop alone.
fn gray_base_only(gray: *mut PrecisionDocument) -> *mut PrecisionDocument {
    edit(
        gray,
        &json!({"op":"layer_visible","layer":1,"visible":false}),
    )
    .unwrap()
}

/// sRGB red's hand-computed relative luminance: the published D50 Y row
/// over the IEC decode of (1, 0, 0).
fn red_y() -> f64 {
    SRGB_Y_ROW[0] * srgb_decode(1.0)
        + SRGB_Y_ROW[1] * srgb_decode(0.0)
        + SRGB_Y_ROW[2] * srgb_decode(0.0)
}

fn fill_red(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    edit_covered(
        doc,
        &json!({"op":"paint_blend","layer":0,"color":[1.0,0.0,0.0,1.0],"color_space":"srgb",
            "opacity":1.0,"blend_mode":0}),
    )
}

#[test]
fn fill_paints_the_colours_luminance_by_mode_grayscales_rule() {
    let y = red_y();
    let encoded = y.powf(1.0 / 2.2);
    // 16 bits: ≤ 4 of 65535.
    let doc = new_gray(16, 3, 2);
    let filled = fill_red(doc);
    for p in codes(filled, 0) {
        assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
        let want = encoded * 65535.0;
        assert!(
            (f64::from(p[0]) - want).abs() <= 4.0,
            "{} against {want}",
            p[0]
        );
    }
    free(filled);
    free(doc);
    // 8 bits: on the ×257 grid, ≤ 1 code.
    let doc = new_gray(8, 3, 2);
    let filled = fill_red(doc);
    for p in codes(filled, 0) {
        assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
        assert_eq!(p[0] % 257, 0, "{p:?}");
        let want = encoded * 255.0;
        assert!(
            (f64::from(p[0] / 257) - want).abs() <= 1.0,
            "{p:?} against {want}"
        );
    }
    free(filled);
    free(doc);
    // 32 bits: linear Y, ≤ 1e-4.
    let doc = new_gray(32, 3, 2);
    let filled = fill_red(doc);
    let samples = unsafe { &*filled }
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap();
    for p in samples {
        assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
        assert!((f64::from(p[0]) - y).abs() <= 1e-4, "{} against {y}", p[0]);
    }
    free(filled);
    free(doc);
    // A colour plane and a refused mode refuse by name.
    let doc = new_gray(16, 3, 2);
    assert_eq!(
        edit(
            doc,
            &json!({"op":"paint_blend","layer":0,"color":[1.0,0.0,0.0,1.0],"color_space":"srgb",
                "opacity":1.0,"blend_mode":0,"plane":"red"})
        )
        .unwrap_err(),
        "A colour plane is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, and Doctored offers the Red, Green, Blue and Alpha targets on RGB documents only."
    );
    assert_eq!(
        edit(
            doc,
            &json!({"op":"paint_blend","layer":0,"color":[1.0,0.0,0.0,1.0],"color_space":"srgb",
                "opacity":1.0,"blend_mode":25})
        )
        .unwrap_err(),
        not_offered("Color")
    );
    free(doc);
}

fn style(layer: usize, style: Value) -> Value {
    json!({"op":"layer_style","version":1,"layer":layer,"style":style})
}

#[test]
fn blending_options_and_effects_are_admitted_and_merges_stay_neutral() {
    let gray = two_layer_gray(16);
    let filled = edit(gray, &style(1, json!({"fill_opacity":0.5}))).unwrap();
    assert_neutral(&render16(filled), "fill opacity");
    // Phase 2d: an effect is admitted and composites R = G = B
    // (`gray_style.rs` holds its oracle), and Blend If is Gray only.
    let shadowed = edit(
        gray,
        &style(
            1,
            json!({"effects":[{"type":"drop_shadow","color":"#ff0000","distance":3,"size":2}]}),
        ),
    )
    .unwrap();
    assert_neutral(&render16(shadowed), "drop shadow");
    free(shadowed);
    assert_eq!(
        edit(gray, &style(1, json!({"blend_if":{"channel":"red"}}))).unwrap_err(),
        "Blend If on the Red channel is unavailable in a Grayscale document."
    );
    let cleared = edit(filled, &style(1, Value::Null)).unwrap();
    free(cleared);
    // Multiply, then Merge Down and Stamp Visible.
    let multiplied = edit(filled, &blend(1, 1)).unwrap();
    let merged = edit(multiplied, &json!({"op":"merge_down","layer":1})).unwrap();
    assert_eq!(descriptor(merged)["layer_count"], 1);
    assert_neutral(&codes(merged, 0), "merge down");
    assert_eq!(render16(merged), render16(multiplied));
    let stamped = edit(
        multiplied,
        &json!({"op":"stamp_visible","name":"Stamp","anchor":1}),
    )
    .unwrap();
    assert_eq!(descriptor(stamped)["layer_count"], 3);
    assert_neutral(&codes(stamped, 2), "stamp");
    assert_eq!(descriptor(stamped)["color_mode"], "gray");
    for doc in [stamped, merged, multiplied, filled, gray] {
        free(doc);
    }
}

#[test]
fn a_group_mask_multiply_clip_and_fill_opacity_round_trip_through_rz() {
    let gray = two_layer_gray(16);
    let added = edit(
        gray,
        &json!({"op":"add_layer","name":"Clip","anchor":1,"position":"above"}),
    )
    .unwrap();
    let painted = fill_red_layer(added, 2);
    let mask: Vec<u16> = (0..PAIRS.len() as u16).map(|i| 65535 - i * 7000).collect();
    let masked = edit_with(
        painted,
        &json!({"op":"set_mask","layer":1,"width":PAIRS.len(),"height":1,"x":0,"y":0,"linked":true}),
        &mask,
    )
    .unwrap();
    let multiplied = edit(masked, &blend(1, 1)).unwrap();
    let clipped = edit(
        multiplied,
        &json!({"op":"layer_clipped","layer":2,"clipped":true}),
    )
    .unwrap();
    let styled = edit(clipped, &style(1, json!({"fill_opacity":0.5}))).unwrap();
    let grouped = edit(
        styled,
        &json!({"op":"group_layers","layers":[1,2],"name":"Group"}),
    )
    .unwrap();
    let before = render16(grouped);
    assert_neutral(&before, "structure");
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("structure.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(grouped, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let reopened = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    let d = descriptor(reopened);
    assert_eq!(d["color_mode"], "gray");
    assert_eq!(d["layer_count"], descriptor(grouped)["layer_count"]);
    assert_eq!(render16(reopened), before);
    for doc in [
        reopened, grouped, styled, clipped, multiplied, masked, painted, added, gray,
    ] {
        free(doc);
    }
}

fn fill_red_layer(doc: *const PrecisionDocument, layer: usize) -> *mut PrecisionDocument {
    edit_covered(
        doc,
        &json!({"op":"paint_blend","layer":layer,"color":[1.0,0.0,0.0,1.0],"color_space":"srgb",
            "opacity":1.0,"blend_mode":0}),
    )
}

#[test]
fn every_remaining_refusal_names_its_phase() {
    for (family, target) in [
        ("layer_add", "document"),
        ("layer_group", "group"),
        ("layer_ungroup", "group"),
        ("layer_blend", "raster"),
        ("layer_clipped", "group"),
        ("layer_merge", "raster"),
        ("layer_merge_visible", "document"),
        ("layer_stamp", "document"),
        ("layer_style", "group"),
        ("mask_set", "raster"),
        ("mask_remove", "mask"),
        ("mask_properties", "mask"),
        ("mask_apply", "mask"),
        ("read_plane", "mask"),
        ("fill_stroke", "mask"),
        ("export", "document"),
        ("layer_name", "group"),
        ("clone", "raster"),
        ("heal", "raster"),
        // Phase 2d.
        ("layer_blend_if", "raster"),
        ("global_light", "document"),
        ("channel_set", "channel"),
        ("channel_remove", "channel"),
        ("channel_read", "channel"),
        ("read_plane", "channel"),
        ("fill_stroke", "channel"),
        ("adjustment_apply", "channel"),
    ] {
        assert!(require_gray(family, target).is_ok(), "{family} {target}");
    }
    assert!(require_gray("layer_blend", "adjustment").is_ok());
    assert_eq!(
        require_gray("layer_blend", "mask").unwrap_err(),
        "Grayscale layer_blend does not support target mask"
    );
    // Phase 4's families are admitted.
    for family in ["geometry_resize", "clipboard_paste", "selection"] {
        assert!(require_gray(family, "document").is_ok(), "{family}");
    }
    // Phase 5a's profile family, phase 5b's proof and phase 6a-ii's comps
    // and links and phase 6b's Histogram and Stack Modes are admitted; every
    // other family is RGB-only, refused for good.
    assert!(require_gray("profile", "document").is_ok());
    assert!(require_gray("proof", "document").is_ok());
    assert!(require_gray("layer_comp", "document").is_ok());
    assert!(require_gray("layer_link", "raster").is_ok());
    assert!(require_gray("histogram", "document").is_ok());
    assert!(require_gray("layer_stack_mode", "raster").is_ok());
    for family in ["color_convert", "color_assign"] {
        assert_eq!(
            require_gray(family, "document").unwrap_err(),
            format!("{family} is unavailable in a Grayscale document: Doctored offers it on RGB documents only.")
        );
    }
    let gray = two_layer_gray(16);
    // Phase 4's edits pass the gate and reach their own parsers.
    for request in [
        json!({"op":"crop"}),
        json!({"op":"rotate90"}),
        json!({"op":"move_selected"}),
        json!({"op":"stamp_selected"}),
    ] {
        if let Err(e) = edit(gray, &request) {
            assert!(
                !e.contains("unavailable in a Grayscale document"),
                "{request}: {e}"
            );
        }
    }
    assert_eq!(
        edit(gray, &json!({"op":"read_plane"})).unwrap_err(),
        "read_plane of the Gray layer is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, so a scalar read takes target mask or channel."
    );
    assert_eq!(
        edit(gray, &json!({"op":"read_plane","target":"red","index":0})).unwrap_err(),
        "A colour plane is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, and Doctored offers the Red, Green, Blue and Alpha targets on RGB documents only."
    );
    assert_eq!(
        edit(gray, &json!({"op":"read_plane","target":"layer","index":0})).unwrap_err(),
        "read_plane of the Gray layer is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, so a scalar read takes target mask or channel."
    );
    free(gray);
}
