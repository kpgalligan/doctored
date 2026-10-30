//! Gradient Smoothness and a Gradient Fill layer's Align with Layer
//! box.
//!
//! Smoothness: the style JSON's `smoothness` (a fraction, 1 = Photoshop's
//! 100 %) is written only below 1 and clamped into 0..=1, a glow's resets;
//! a Gradient Overlay at Smoothness `k` is `common::smooth_ramp` (the linear
//! and Classic ramps blended by `k`, written from the take-2 law, never
//! read off the core) at 8 and 16 bits; a Stroke Gradient's colour is linear
//! in `k` (its Smoothness-½ render is the mean of its 0 and 1 renders within
//! a code), and Diamond ignores it.
//!
//! The box: an artwork `gradient` component's `box` spans the gradient over
//! that rectangle, so a pixel inside it equals the same pixel of a raster the
//! box's size rendered without one (RGB and CMYK ink), and a box with no area
//! refuses. `dr_doc_fill_align_box` and `dr_precision_doc_fill_align_box`
//! answer the bounds of the mask's raw coverage, clipped to the canvas, at 8
//! and 16 bits and in CMYK: a canvas-sized mask made from a selection gives
//! the selection's rectangle; a disabled mask, outside coverage 255, an
//! all-zero mask or no mask gives the canvas; a mask past the canvas is
//! clipped; feather does not move it. Then the null-safety sweep.

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_artwork::dr_artwork_render;
use doctored_core::ffi_cmyk_artwork::dr_precision_doc_cmyk_artwork;
use doctored_core::ffi_fill_align::{dr_doc_fill_align_box, dr_precision_doc_fill_align_box};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free, varied, with_mask, RgbLayer};
use crate::common::{ffi_style, rect_layer_doc, smooth_ramp, solid, styled, take_err_string};
use crate::identify_578::{render16, render8};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (64, 48);
const RECT: [i32; 4] = [8, 6, 40, 30];
const WHOLE: [i32; 4] = [0, 0, 64, 48];

// ------------------------------------------------------------------- JSON --

fn effect0(doc: &DrDocument) -> Value {
    let text = ffi_style(doc as *const DrDocument, 1).expect("a style");
    serde_json::from_str::<Value>(&text).unwrap()["effects"][0].clone()
}

fn overlay_style(gradient: Value) -> String {
    json!({"effects": [{"type": "gradient_overlay", "gradient": gradient}]}).to_string()
}

#[test]
fn smoothness_is_written_only_below_100_and_clamped() {
    let doc = rect_layer_doc((24, 16), (4, 4, 12, 8), [200, 40, 40, 255]);
    let read = |gradient: Value| effect0(&styled(&doc, 1, &overlay_style(gradient)));
    assert!(
        read(json!({})).get("smoothness").is_none(),
        "none by default"
    );
    let at = |s: f64| {
        read(json!({"smoothness": s}))["gradient"]
            .get("smoothness")
            .cloned()
    };
    assert_eq!(at(1.0), None, "100 % is the default and writes nothing");
    assert_eq!(at(0.5), Some(json!(0.5)));
    assert_eq!(at(0.0), Some(json!(0.0)));
    assert_eq!(at(1.7), None, "clamped to 100 %");
    assert_eq!(at(-0.2), Some(json!(0.0)), "clamped to 0 %");
    assert_eq!(at(0.123_456), Some(json!(0.1235)), "four decimals");
    // A stroke's gradient carries it too, whatever its fill type.
    let stroke = json!({"effects": [{"type": "stroke", "size": 3, "fill_type": "color",
        "gradient": {"smoothness": 0.25}}]});
    let e = effect0(&styled(&doc, 1, &stroke.to_string()));
    assert_eq!(e["gradient"]["smoothness"], json!(0.25), "{e}");
    // A glow's gradient keeps its stops only.
    let stops = json!([{"position": 0, "color": "#ff0000", "opacity": 1},
        {"position": 1, "color": "#0000ff", "opacity": 1}]);
    let glow = json!({"effects": [{"type": "outer_glow", "fill_type": "gradient",
        "gradient": {"stops": stops, "smoothness": 0.5}}]});
    let e = effect0(&styled(&doc, 1, &glow.to_string()));
    assert!(e["gradient"].is_object(), "{e}");
    assert!(e["gradient"].get("smoothness").is_none(), "{e}");
}

// ---------------------------------------------------------------- render --

const THREE: [(f64, f64); 3] = [(0.0, 0.0), (0.3, 1.0), (1.0, 64.0 / 255.0)];

fn three_stops() -> Value {
    Value::from(
        THREE
            .iter()
            .map(|&(p, v)| {
                let code = (v * 255.0).round() as u8;
                json!({"position": p, "color": format!("#{code:02x}{code:02x}{code:02x}"),
                    "opacity": 1})
            })
            .collect::<Vec<_>>(),
    )
}

#[test]
fn an_overlay_blends_the_linear_and_classic_ramps_by_its_smoothness() {
    let (w, h) = (64u32, 4u32);
    let inside = vec![true; (w * h) as usize];
    for k in [0.0, 0.35, 0.8] {
        let style = json!({"effects": [{"type": "gradient_overlay", "blend": "normal",
            "opacity": 1, "gradient": {"stops": three_stops(), "style": "linear", "angle": 0,
            "scale": 1, "align_with_layer": true, "smoothness": k}}]});
        let (eight, sixteen) = (
            render8(w, h, &inside, &style),
            render16(w, h, &inside, &style),
        );
        for y in 0..h {
            for x in 0..w {
                // Linear at 0°: the pixel's top-left corner, E the box width.
                let t = (0.5 + (f64::from(x) - f64::from(w) / 2.0) / f64::from(w)).clamp(0.0, 1.0);
                let want = smooth_ramp(&THREE, t, k).clamp(0.0, 1.0) * 255.0;
                let at = (y * w + x) as usize;
                assert!(
                    (eight[at] - want).abs() <= 1.0,
                    "k {k} ({x},{y}): 8-bit {} vs {want}",
                    eight[at]
                );
                assert!(
                    (sixteen[at] - want).abs() <= 2.0 / 257.0,
                    "k {k} ({x},{y}): 16-bit {} vs {want}",
                    sixteen[at]
                );
            }
        }
    }
}

fn stroke_style(style: &str, smoothness: f64) -> String {
    json!({"effects": [{"type": "stroke", "size": 6, "position": "outside",
        "fill_type": "gradient", "gradient": {"stops": three_stops(), "style": style,
        "angle": 20, "smoothness": smoothness}}]})
    .to_string()
}

#[test]
fn a_stroke_gradient_is_linear_in_its_smoothness_and_diamond_ignores_it() {
    let doc = rect_layer_doc((48, 40), (12, 10, 22, 18), [200, 40, 40, 255]);
    let flat = |style: &str, k: f64| -> Vec<u8> {
        styled(&doc, 1, &stroke_style(style, k))
            .flattened()
            .unwrap()
            .into_raw()
    };
    let (zero, half, one) = (
        flat("linear", 0.0),
        flat("linear", 0.5),
        flat("linear", 1.0),
    );
    assert_ne!(zero, one, "Smoothness moves the stroke");
    for (i, ((a, m), b)) in zero.iter().zip(&half).zip(&one).enumerate() {
        let mean = (f64::from(*a) + f64::from(*b)) / 2.0;
        assert!(
            (f64::from(*m) - mean).abs() <= 1.0,
            "sample {i}: {m} vs the mean {mean}"
        );
    }
    // Diamond keeps the linear ramp at any Smoothness.
    assert_eq!(flat("diamond", 0.3), flat("diamond", 1.0));
    let overlay = |k: f64| {
        let style = json!({"effects": [{"type": "gradient_overlay", "gradient":
            {"stops": three_stops(), "style": "diamond", "smoothness": k}}]});
        styled(&doc, 1, &style.to_string())
            .flattened()
            .unwrap()
            .into_raw()
    };
    assert_eq!(overlay(0.0), overlay(1.0), "a Diamond overlay too");
}

// ------------------------------------------------------------ artwork box --

fn gradient_object() -> Value {
    json!({"stops": three_stops(), "style": "linear", "angle": 30, "scale": 0.9,
        "reverse": false, "align_with_layer": true, "smoothness": 0.6})
}

fn rgb_component(frame: Option<[i64; 4]>) -> String {
    let colors: Vec<[f64; 3]> = THREE.iter().map(|s| [s.1; 3]).collect();
    let mut c = json!({"kind": "gradient", "gradient": gradient_object(), "colors": colors});
    if let Some(frame) = frame {
        c["box"] = json!(frame);
    }
    json!([c]).to_string()
}

fn artwork(size: (u32, u32), components: &str) -> Result<Vec<[u16; 4]>, String> {
    let components = CString::new(components).unwrap();
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            size.0,
            size.1,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let image = *unsafe { Box::from_raw(out) };
    Ok(image.to_rgba16().unwrap())
}

const BOX_REFUSAL: &str = "artwork gradient box requires a positive width and height";

#[test]
fn an_artwork_gradient_spans_its_box() {
    let (bx, by, bw, bh) = (5u32, 4u32, 20u32, 12u32);
    let boxed = artwork(
        (40, 30),
        &rgb_component(Some([5, 4, i64::from(bw), i64::from(bh)])),
    )
    .unwrap();
    let small = artwork((bw, bh), &rgb_component(None)).unwrap();
    for y in 0..bh {
        for x in 0..bw {
            assert_eq!(
                boxed[((y + by) * 40 + x + bx) as usize],
                small[(y * bw + x) as usize],
                "({x},{y}) inside the box"
            );
        }
    }
    // Outside the box the ramp is clamped to its end stops.
    let whole = artwork((40, 30), &rgb_component(None)).unwrap();
    assert_ne!(boxed, whole, "the box moves the gradient");
    for frame in [
        [0, 0, 0, 5],
        [0, 0, 5, -1],
        [1 << 31, 0, 4, 4],
        [0, 0, 4, 1 << 40],
    ] {
        assert_eq!(
            artwork((40, 30), &rgb_component(Some(frame)))
                .err()
                .as_deref(),
            Some(BOX_REFUSAL),
            "{frame:?}"
        );
    }
}

fn cmyk_component(frame: Option<[i64; 4]>) -> String {
    let inks = [
        [0.7, 0.2, 0.1, 0.05],
        [0.1, 0.55, 0.9, 0.0],
        [0.0, 0.3, 0.6, 0.2],
    ];
    let mut c = json!({"kind": "gradient", "gradient": gradient_object(), "inks": inks});
    if let Some(frame) = frame {
        c["box"] = json!(frame);
    }
    json!([c]).to_string()
}

fn cmyk_publish(size: (u32, u32), components: &str) -> Result<*mut PrecisionDocument, String> {
    let base = cmyk_layers(
        size.0,
        size.1,
        vec![RgbLayer {
            name: "Bottom",
            w: size.0,
            h: size.1,
            offset: (0, 0),
            pixels: varied(size.0, size.1, 3),
        }],
    );
    let (components, name) = (
        CString::new(components).unwrap(),
        CString::new("G").unwrap(),
    );
    let description = CString::new(
        json!({"type": "fill", "version": 2, "ink": "cmyk", "fill": "gradient",
            "gradient": gradient_object(), "stop_inks": [[0.7, 0.2, 0.1, 0.05],
            [0.1, 0.55, 0.9, 0.0], [0.0, 0.3, 0.6, 0.2]], "canvas": [size.0, size.1]})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_artwork(
            base,
            0,
            false,
            0,
            size.0,
            size.1,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    free(base);
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

#[test]
fn a_cmyk_artwork_gradient_spans_its_box() {
    let (bx, by, bw, bh) = (5u32, 4u32, 20u32, 12u32);
    let boxed = cmyk_publish(
        (40, 30),
        &cmyk_component(Some([5, 4, i64::from(bw), i64::from(bh)])),
    )
    .unwrap();
    let small = cmyk_publish((bw, bh), &cmyk_component(None)).unwrap();
    let (a, b) = unsafe {
        (
            (*boxed).cmyk_layer_samples(1).unwrap(),
            (*small).cmyk_layer_samples(1).unwrap(),
        )
    };
    for y in 0..bh {
        for x in 0..bw {
            assert_eq!(
                a[((y + by) * 40 + x + bx) as usize],
                b[(y * bw + x) as usize],
                "({x},{y}) inside the box"
            );
        }
    }
    free(boxed);
    free(small);
    assert_eq!(
        cmyk_publish((40, 30), &cmyk_component(Some([0, 0, 0, 3])))
            .err()
            .as_deref(),
        Some(BOX_REFUSAL)
    );
}

// --------------------------------------------------------------- the box --

fn selection_rect(rect: [i32; 4]) -> Vec<u8> {
    let (w, h) = CANVAS;
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x as i32, y as i32)))
        .map(|(x, y)| {
            let inside =
                x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3];
            if inside {
                255
            } else {
                0
            }
        })
        .collect()
}

fn byte_box(doc: &DrDocument, idx: usize) -> Option<[i32; 4]> {
    let mut out = [-7i32; 4];
    unsafe { dr_doc_fill_align_box(doc, idx, out.as_mut_ptr()) }.then_some(out)
}

fn typed_box(doc: *const PrecisionDocument, idx: usize) -> Option<[i32; 4]> {
    let mut out = [-7i32; 4];
    unsafe { dr_precision_doc_fill_align_box(doc, idx, out.as_mut_ptr()) }.then_some(out)
}

#[test]
fn the_byte_box_is_the_masks_coverage_bounds_clipped_to_the_canvas() {
    let doc = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, [90, 90, 90, 255]));
    assert_eq!(byte_box(&doc, 0), Some(WHOLE), "no mask: the canvas");
    let masked = doc
        .add_mask(0, MaskKind::FromSelection(&selection_rect(RECT)))
        .unwrap();
    assert_eq!(
        masked.layers[0].mask.as_deref().map(|m| m.dimensions()),
        Some(CANVAS),
        "a canvas-sized mask"
    );
    assert_eq!(
        byte_box(&masked, 0),
        Some(RECT),
        "the selection's rectangle"
    );
    let mut feathered = masked.clone();
    feathered.layers[0].mask_properties.feather = 6.0;
    feathered.layers[0].mask_properties.density = 0.4;
    assert_eq!(
        byte_box(&feathered, 0),
        Some(RECT),
        "feather and density do not move it"
    );
    let mut disabled = masked.clone();
    disabled.layers[0].mask_enabled = false;
    assert_eq!(byte_box(&disabled, 0), Some(WHOLE), "disabled");
    let mut outside = masked.clone();
    outside.layers[0].mask_properties.outside_coverage = 255;
    assert_eq!(byte_box(&outside, 0), Some(WHOLE), "outside coverage 255");
    let hidden = doc.add_mask(0, MaskKind::HideAll).unwrap();
    assert_eq!(byte_box(&hidden, 0), Some(WHOLE), "no coverage");
    // A soft edge sample above zero widens the box.
    let mut soft = masked.clone();
    let mut plane: GrayImage = (*soft.layers[0].mask.clone().unwrap()).clone();
    plane.put_pixel(50, 40, Luma([1]));
    soft.layers[0].mask = Some(Arc::new(plane));
    assert_eq!(byte_box(&soft, 0), Some([8, 6, 43, 35]));
    // A layer past the canvas, revealed in full: clipped to the canvas.
    for ((size, offset), want) in [
        (((30, 20), (50, 40)), [50, 40, 14, 8]),
        (((20, 10), (-5, -4)), [0, 0, 15, 6]),
    ] {
        let placed = doc
            .adding_image_layer(0, solid(size.0, size.1, [10, 20, 30, 255]), "L")
            .unwrap();
        let mut placed = placed.add_mask(1, MaskKind::RevealAll).unwrap();
        placed.layers[1].offset = offset;
        placed.layers[1].mask_properties = MaskProperties::at(offset);
        assert_eq!(byte_box(&placed, 1), Some(want), "{size:?} at {offset:?}");
    }
    assert_eq!(byte_box(&masked, 1), None, "out of range");
}

fn typed_doc(mask: Option<PrecisionMask>) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        CANVAS.0,
        CANVAS.1,
        vec![[30000, 20000, 10000, 65535]; (CANVAS.0 * CANVAS.1) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let layer = PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true)
        .unwrap()
        .with_mask(mask);
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

fn mask16(size: (u32, u32), samples: Vec<u16>, at: (i32, i32), enabled: bool) -> PrecisionMask {
    let plane = PrecisionPlane::from_u16(size.0, size.1, samples, None).unwrap();
    PrecisionMask::new(plane, MaskProperties::at(at), enabled).unwrap()
}

fn selection16(rect: [i32; 4]) -> Vec<u16> {
    selection_rect(rect)
        .iter()
        .map(|&v| u16::from(v) * 257)
        .collect()
}

#[test]
fn the_typed_box_follows_the_same_rule_at_16_bits_and_in_cmyk() {
    assert_eq!(typed_box(&typed_doc(None), 0), Some(WHOLE), "no mask");
    let masked = typed_doc(Some(mask16(CANVAS, selection16(RECT), (0, 0), true)));
    assert_eq!(typed_box(&masked, 0), Some(RECT));
    let disabled = typed_doc(Some(mask16(CANVAS, selection16(RECT), (0, 0), false)));
    assert_eq!(typed_box(&disabled, 0), Some(WHOLE), "disabled");
    let zero = typed_doc(Some(mask16(CANVAS, vec![0; 64 * 48], (0, 0), true)));
    assert_eq!(typed_box(&zero, 0), Some(WHOLE), "no coverage");
    let mut outside = mask16(CANVAS, selection16(RECT), (0, 0), true);
    let mut props = outside.properties().clone();
    props.outside_coverage = 255;
    outside = outside.with_properties(props, true).unwrap();
    assert_eq!(
        typed_box(&typed_doc(Some(outside)), 0),
        Some(WHOLE),
        "outside coverage 255"
    );
    let past = typed_doc(Some(mask16((30, 20), vec![65535; 600], (50, 40), true)));
    assert_eq!(typed_box(&past, 0), Some([50, 40, 14, 8]), "clipped");
    assert_eq!(typed_box(&masked, 1), None, "out of range");

    // CMYK: a canvas-sized mask through `set_mask`.
    let doc = cmyk_layers(
        CANVAS.0,
        CANVAS.1,
        vec![RgbLayer {
            name: "Bottom",
            w: CANVAS.0,
            h: CANVAS.1,
            offset: (0, 0),
            pixels: varied(CANVAS.0, CANVAS.1, 5),
        }],
    );
    assert_eq!(typed_box(doc, 0), Some(WHOLE), "CMYK, no mask");
    let doc = with_mask(doc, 0, &selection16(RECT));
    assert!(unsafe { &*doc }.is_cmyk());
    assert_eq!(typed_box(doc, 0), Some(RECT), "CMYK");
    free(doc);
}

#[test]
fn the_align_box_exports_are_null_safe() {
    let byte = DrDocument::from_pixels(RgbaImage::from_pixel(4, 3, Rgba([1, 2, 3, 255])));
    let typed = typed_doc(None);
    let mut out = [-7i32; 4];
    assert!(!unsafe { dr_doc_fill_align_box(ptr::null(), 0, out.as_mut_ptr()) });
    assert!(!unsafe { dr_precision_doc_fill_align_box(ptr::null(), 0, out.as_mut_ptr()) });
    assert!(!unsafe { dr_doc_fill_align_box(ptr::null(), 0, ptr::null_mut()) });
    assert!(!unsafe { dr_precision_doc_fill_align_box(ptr::null(), 0, ptr::null_mut()) });
    assert_eq!(out, [-7; 4], "nothing written");
    assert!(!unsafe { dr_doc_fill_align_box(&byte, 9, out.as_mut_ptr()) });
    assert!(!unsafe { dr_precision_doc_fill_align_box(&typed, 9, out.as_mut_ptr()) });
    assert_eq!(out, [-7; 4], "nothing written out of range");
    assert!(unsafe { dr_doc_fill_align_box(&byte, 0, ptr::null_mut()) });
    assert!(unsafe { dr_precision_doc_fill_align_box(&typed, 0, ptr::null_mut()) });
    assert!(unsafe { dr_doc_fill_align_box(&byte, 0, out.as_mut_ptr()) });
    assert_eq!(out, [0, 0, 4, 3]);
}
