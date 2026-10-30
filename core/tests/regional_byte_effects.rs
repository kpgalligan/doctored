//! Phase 13A-3b-i: the 8-bit regional document render over layer styles
//! WITH ENABLED EFFECTS on a raster — every effect, the package model
//! (opacity below 1, not Dissolve), fill + Blend If beside effects, masks,
//! a styled clip base and a styled member, styled rasters inside groups and
//! the document's global light — against the full-frame projection, plus the
//! plan's `Global` rows, the reach they declare, their formulas, the new
//! refusals and the concurrent cancels at BOTH depths (the u16 twin lives
//! here, as 13A-3a's does, so one command measures both). The oracle is
//! INDEPENDENT of the regional code: `dr_doc_flattened`'s projection cropped
//! and top-left-cell sampled here in plain index arithmetic, byte for byte.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_precision::dr_precision_image_free;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::{GlobalLight, LayerStyle};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::{adjust_meta, img_dims, img_pixels, plasma, styled, take_err_string};

use crate::region_plan;

const W: u32 = 41;
const H: u32 = 29;

const BLEND_IF_GRAY: &str =
    "\"blend_if\":{\"channel\":\"gray\",\"this_layer\":[10,60,180,240],\"underlying\":[0,40,200,255]}";

// ------------------------------------------------------------- plumbing --

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

fn full(doc: &DrDocument) -> Vec<u8> {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_flattened(h, 0, std::ptr::null(), &mut err) };
    assert!(!img.is_null(), "flattened: {}", take_err_string(err));
    let out = img_pixels(img);
    unsafe {
        dr_image_free(img);
        dr_doc_free(h);
    }
    out
}

/// The independent oracle: pixel (x + i·d, y + j·d) of the full projection.
fn expected(
    full: &[u8],
    canvas_w: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> (u32, u32, Vec<u8>) {
    let ow = w.div_ceil(d);
    let oh = h.div_ceil(d);
    let mut out = Vec::new();
    for j in 0..oh {
        for i in 0..ow {
            let cx = u64::from(x) + u64::from(i) * u64::from(d);
            let cy = u64::from(y) + u64::from(j) * u64::from(d);
            let at = ((cy * u64::from(canvas_w) + cx) * 4) as usize;
            out.extend_from_slice(&full[at..at + 4]);
        }
    }
    (ow, oh, out)
}

fn region(
    doc: &DrDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> Result<(u32, u32, Vec<u8>), String> {
    let hd = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, d, u64::MAX, ptr::null(), &mut err) };
    unsafe { dr_doc_free(hd) };
    if img.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null(), "err_out set on success");
    let (ow, oh) = img_dims(img);
    let px = img_pixels(img);
    unsafe { dr_image_free(img) };
    Ok((ow, oh, px))
}

fn plan(doc: &DrDocument, (x, y, w, h): (u32, u32, u32, u32), d: u32) -> Result<Value, String> {
    region_plan::byte(doc, (x, y, w, h), d, u64::MAX)
}

/// The canvas windows every case is checked at — edges, corners, interior,
/// 1×1, the whole canvas — plus `extra`, at divisors {1, 2, 3, 7, MAX}.
fn assert_regions_match_with(doc: &DrDocument, what: &str, extra: &[(u32, u32, u32, u32)]) {
    let reference = full(doc);
    let (cw, ch) = (doc.width, doc.height);
    let mut windows = vec![
        (0, 0, cw, ch),
        (3, 2, 11, 7),
        (0, 0, 5, 4),
        (cw - 6, 0, 6, 5),
        (0, ch - 3, 7, 3),
        (cw - 4, ch - 5, 4, 5),
        (0, 4, cw, 3),
        (7, 0, 2, ch),
        (cw - 1, ch - 1, 1, 1),
        (0, 0, 1, 1),
        (19, 13, 1, 1),
    ];
    windows.extend_from_slice(extra);
    for window in windows {
        for d in [1, 2, 3, 7, u32::MAX] {
            let got =
                region(doc, window, d).unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, cw, window, d),
                "{what}: window {window:?} divisor {d}"
            );
        }
    }
}

fn assert_regions_match(doc: &DrDocument, what: &str) {
    assert_regions_match_with(doc, what, &[]);
}

fn base() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        let c = plasma(x, y);
        let alpha = if y < 4 {
            (90 + (x * 20) % 120) as u8
        } else {
            255
        };
        Rgba([c[0], c[1], c[2], alpha])
    }))
}

/// A layer with a real SHAPE: transparent outside an ellipse and in a hole,
/// graded alpha inside, so every effect has edges, an interior and partial
/// coverage.
fn shape_pixels(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let dx = (x as f32 + 0.5 - cx) / cx;
        let dy = (y as f32 + 0.5 - cy) / cy;
        let r = dx * dx + dy * dy;
        let hole = x == w / 2 + 1 && (y == h / 2 || y == h / 2 + 1);
        let alpha = if r > 1.0 || hole {
            0
        } else if r > 0.7 {
            (60 + (x * 13 + y * 29) % 150) as u8
        } else {
            255
        };
        Rgba([
            (x * 37 + y * 11) as u8,
            (200 - x * 5) as u8,
            (y * 23 + 7) as u8,
            alpha,
        ])
    })
}

fn top_pixels(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 37 + y * 11) as u8,
            (200 - x * 5) as u8,
            (y * 23 + 7) as u8,
            (40 + (x * 13 + y * 29) % 216) as u8,
        ])
    })
}

fn with_raster(
    doc: &DrDocument,
    pixels: RgbaImage,
    offset: (i32, i32),
    blend: BlendMode,
    opacity: f32,
) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, pixels, "Raster")
        .expect("add layer");
    let top = out.layers.len() - 1;
    let layer = &mut out.layers[top];
    layer.offset = offset;
    layer.blend = blend;
    layer.opacity = opacity;
    out
}

/// A gradient coverage plane with a hard hole.
fn gradient(w: u32, h: u32) -> GrayImage {
    GrayImage::from_fn(w, h, |x, y| {
        if x == w / 2 && y == h / 2 {
            Luma([0])
        } else {
            Luma([((x * 255) / (w - 1).max(1)) as u8 ^ (y * 9) as u8])
        }
    })
}

fn masked(
    doc: &DrDocument,
    idx: usize,
    plane: GrayImage,
    offset: (i32, i32),
    density: f32,
    feather: f32,
) -> DrDocument {
    let mut out = doc.clone();
    let mut properties = MaskProperties::at(offset);
    properties.density = density;
    properties.feather = feather;
    let layer = &mut out.layers[idx];
    layer.mask = Some(Arc::new(plane));
    layer.mask_enabled = true;
    layer.mask_properties = properties;
    out
}

/// Windows straddling the edges of a rect grown by `grow` on every side,
/// clamped into the canvas.
fn around(offset: (i32, i32), w: u32, h: u32, grow: i64) -> Vec<(u32, u32, u32, u32)> {
    let clamp = |v: i64, hi: u32| v.clamp(0, i64::from(hi) - 1) as u32;
    let (x0, y0) = (i64::from(offset.0) - grow, i64::from(offset.1) - grow);
    let (x1, y1) = (
        i64::from(offset.0) + i64::from(w) + grow,
        i64::from(offset.1) + i64::from(h) + grow,
    );
    let mut out = Vec::new();
    for (cx, cy) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
        let x = clamp(cx - 3, W);
        let y = clamp(cy - 2, H);
        out.push((x, y, (W - x).min(6), (H - y).min(5)));
    }
    out
}

fn rows(p: &Value) -> Vec<u64> {
    p["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["index"].as_u64().unwrap())
        .collect()
}

fn fx(effects: &[&str]) -> String {
    format!("{{\"effects\":[{}]}}", effects.join(","))
}

/// Every effect the renderer knows, each in the configurations the spec
/// names: drop shadow near and far (distance 9 crosses the window and the
/// canvas edge), outer glow, the three strokes, a Multiply colour overlay,
/// both gradient alignments, inner shadow at distance 0 and 40 (size 0),
/// inner glow, a two-part bevel and satin.
fn every_effect() -> Vec<(&'static str, String)> {
    vec![
        (
            "drop shadow 3/2",
            fx(&["{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}"]),
        ),
        (
            "drop shadow 9",
            fx(&["{\"type\":\"drop_shadow\",\"distance\":9,\"size\":3,\"spread\":0.3}"]),
        ),
        (
            "outer glow",
            fx(&["{\"type\":\"outer_glow\",\"size\":4,\"color\":\"#40ff80\"}"]),
        ),
        (
            "stroke outside",
            fx(&["{\"type\":\"stroke\",\"size\":3,\"position\":\"outside\",\"color\":\"#ff2000\"}"]),
        ),
        (
            "stroke inside",
            fx(&["{\"type\":\"stroke\",\"size\":2,\"position\":\"inside\",\"color\":\"#20ff00\",\"opacity\":0.7}"]),
        ),
        (
            "stroke center",
            fx(&["{\"type\":\"stroke\",\"size\":3,\"position\":\"center\",\"blend\":\"screen\"}"]),
        ),
        (
            "colour overlay multiply",
            fx(&["{\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#3080c0\",\"opacity\":0.8}"]),
        ),
        (
            "gradient overlay aligned",
            fx(&["{\"type\":\"gradient_overlay\",\"gradient\":{\"angle\":30,\"align_with_layer\":true,\
                  \"stops\":[{\"position\":0,\"color\":\"#ff0000\",\"opacity\":1},\
                  {\"position\":1,\"color\":\"#0000ff\",\"opacity\":0.6}]}}"]),
        ),
        (
            "gradient overlay canvas",
            fx(&["{\"type\":\"gradient_overlay\",\"blend\":\"overlay\",\"gradient\":{\"style\":\"radial\",\
                  \"align_with_layer\":false}}"]),
        ),
        (
            "inner shadow 0",
            fx(&["{\"type\":\"inner_shadow\",\"distance\":0,\"size\":4}"]),
        ),
        (
            "inner shadow 40",
            fx(&["{\"type\":\"inner_shadow\",\"distance\":40,\"size\":0,\"opacity\":0.9}"]),
        ),
        (
            "inner glow",
            fx(&["{\"type\":\"inner_glow\",\"size\":3,\"source\":\"center\"}"]),
        ),
        (
            "bevel",
            fx(&["{\"type\":\"bevel_emboss\",\"size\":4,\"depth\":2,\"highlight_opacity\":0.9,\
                  \"shadow_opacity\":0.8}"]),
        ),
        (
            "satin",
            fx(&["{\"type\":\"satin\",\"distance\":5,\"size\":4,\"color\":\"#602000\"}"]),
        ),
    ]
}

/// Opacity 1, the PACKAGE at 0.6 in Normal and Multiply, and Dissolve 0.6
/// (the direct path).
const VARIANTS: [(BlendMode, f32); 4] = [
    (BlendMode::Normal, 1.0),
    (BlendMode::Normal, 0.6),
    (BlendMode::Multiply, 0.6),
    (BlendMode::Dissolve, 0.6),
];

// ------------------------------------------------------------- lone --

#[test]
fn every_effect_on_a_lone_raster_matches_the_projection() {
    for (label, style) in every_effect() {
        for (mode, opacity) in VARIANTS {
            // Near the bottom-right corner, so every reach and the far drop
            // shadow cross the canvas edge.
            let doc = with_raster(&base(), shape_pixels(17, 12), (20, 12), mode, opacity);
            let doc = styled(&doc, 1, &style);
            let what = format!("{label} {mode:?} {opacity}");
            assert_regions_match_with(&doc, &what, &around((20, 12), 17, 12, 4));
        }
    }
}

#[test]
fn a_partly_off_canvas_styled_raster_matches() {
    let style = fx(&[
        "{\"type\":\"drop_shadow\",\"distance\":9,\"size\":2}",
        "{\"type\":\"stroke\",\"size\":2}",
        "{\"type\":\"inner_glow\",\"size\":3}",
    ]);
    for (mode, opacity) in VARIANTS {
        let doc = with_raster(&base(), shape_pixels(17, 12), (-6, -4), mode, opacity);
        let doc = styled(&doc, 1, &style);
        assert_regions_match(&doc, &format!("off canvas {mode:?} {opacity}"));
    }
}

#[test]
fn fill_and_blend_if_beside_effects_match() {
    let style = format!(
        "{{\"fill_opacity\":0.5,{BLEND_IF_GRAY},\"effects\":[\
         {{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}},\
         {{\"type\":\"color_overlay\",\"opacity\":0.5}},\
         {{\"type\":\"inner_glow\",\"size\":3}}]}}"
    );
    for (mode, opacity) in VARIANTS {
        let doc = with_raster(&base(), shape_pixels(17, 12), (8, 6), mode, opacity);
        let doc = styled(&doc, 1, &style);
        let what = format!("fill + blend if + effects {mode:?} {opacity}");
        assert_regions_match_with(&doc, &what, &around((8, 6), 17, 12, 3));
    }
}

#[test]
fn a_masked_styled_raster_matches_with_feather_0_and_6() {
    let style = fx(&[
        "{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}",
        "{\"type\":\"stroke\",\"size\":2,\"position\":\"center\"}",
        "{\"type\":\"inner_shadow\",\"distance\":2,\"size\":2}",
    ]);
    for feather in [0.0, 6.0] {
        for (mode, opacity) in [(BlendMode::Normal, 1.0), (BlendMode::Multiply, 0.6)] {
            let doc = with_raster(&base(), shape_pixels(17, 12), (8, 6), mode, opacity);
            let doc = styled(&doc, 1, &style);
            let doc = masked(&doc, 1, gradient(14, 10), (10, 7), 0.8, feather);
            let what = format!("masked feather {feather} {mode:?} {opacity}");
            assert_regions_match_with(&doc, &what, &around((10, 7), 14, 10, 0));
        }
    }
}

#[test]
fn the_documents_global_light_drives_use_global_light() {
    let style = fx(&[
        "{\"type\":\"drop_shadow\",\"distance\":6,\"size\":2,\"use_global_light\":true}",
        "{\"type\":\"bevel_emboss\",\"size\":3,\"use_global_light\":true}",
    ]);
    let doc = with_raster(
        &base(),
        shape_pixels(17, 12),
        (10, 7),
        BlendMode::Normal,
        0.8,
    );
    let doc = styled(&doc, 1, &style);
    let lit = doc
        .set_global_light(GlobalLight {
            angle: 30.0,
            altitude: 55.0,
        })
        .expect("a light change");
    assert_ne!(full(&lit), full(&doc), "the light moves the effects");
    assert_regions_match(&doc, "default light");
    assert_regions_match(&lit, "global light 30/55");
}

// ------------------------------------------------------------ clip units --

/// Background, a base at (6, 5) and above it a raster Multiply 0.6 member
/// 15×15 at (14, 8) — which exceeds the base rect — then a VISIBLE
/// zero-opacity member (a clamp-only step).
fn clip_stack(base_mode: BlendMode, base_opacity: f32) -> DrDocument {
    let doc = with_raster(
        &base(),
        shape_pixels(19, 13),
        (6, 5),
        base_mode,
        base_opacity,
    );
    let mut doc = with_raster(&doc, top_pixels(15, 15), (14, 8), BlendMode::Multiply, 0.6);
    doc.layers[2].clipped = true;
    let mut doc = with_raster(&doc, top_pixels(9, 9), (2, 2), BlendMode::Normal, 0.0);
    doc.layers[3].clipped = true;
    doc
}

const BASE_STYLE: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                          {\"type\":\"stroke\",\"size\":3},\
                          {\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#80c0ff\"}]}";

#[test]
fn a_styled_clip_base_clamps_to_its_padded_shape() {
    for (mode, opacity) in VARIANTS {
        let doc = styled(&clip_stack(mode, opacity), 1, BASE_STYLE);
        let what = format!("styled base {mode:?} {opacity}");
        assert_regions_match_with(&doc, &what, &around((6, 5), 19, 13, 4));
        // The clamp-only step alone (the visible member hidden).
        let mut only_clamp = doc.clone();
        only_clamp.layers[2].visible = false;
        assert_regions_match(&only_clamp, &format!("{what}, clamp-only step"));
    }
    // The member reaches beyond the base rect, where the padded shape is 0:
    // the member is clipped there even inside the stroke's pad.
    let doc = styled(&clip_stack(BlendMode::Normal, 1.0), 1, BASE_STYLE);
    let mut bare = doc.clone();
    bare.layers[2].visible = false;
    assert_ne!(full(&doc), full(&bare), "the member shows inside the shape");
    // Fill + Blend If on the base beside effects.
    let fill = format!(
        "{{\"fill_opacity\":0.3,{BLEND_IF_GRAY},\"effects\":[{{\"type\":\"stroke\",\"size\":2}},\
         {{\"type\":\"inner_shadow\",\"distance\":3,\"size\":2}}]}}"
    );
    let doc = styled(&clip_stack(BlendMode::Multiply, 0.6), 1, &fill);
    assert_regions_match_with(&doc, "fill + blend if base", &around((6, 5), 19, 13, 3));
}

#[test]
fn a_styled_member_matches_under_an_unstyled_and_a_styled_base() {
    let member = fx(&[
        "{\"type\":\"stroke\",\"size\":2,\"color\":\"#ffffff\"}",
        "{\"type\":\"drop_shadow\",\"distance\":3,\"size\":1}",
    ]);
    for opacity in [1.0, 0.6] {
        let mut stack = clip_stack(BlendMode::Normal, 1.0);
        stack.layers[2].opacity = opacity;
        let doc = styled(&stack, 2, &member);
        let what = format!("styled member {opacity}");
        assert_regions_match_with(&doc, &what, &around((14, 8), 15, 15, 3));
        let doc = styled(&doc, 1, BASE_STYLE);
        assert_regions_match(&doc, &format!("{what}, styled base"));
    }
}

// ---------------------------------------------------------------- groups --

fn group(doc: &DrDocument, idx: &[usize], name: &str) -> (DrDocument, usize) {
    let (out, g, ..) = doc.group_layers(idx, name).expect("group");
    (out, g)
}

#[test]
fn a_styled_raster_inside_isolated_and_pass_through_groups_matches() {
    let style = fx(&[
        "{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}",
        "{\"type\":\"outer_glow\",\"size\":3}",
        "{\"type\":\"satin\",\"distance\":3,\"size\":3}",
    ]);
    let doc = with_raster(&base(), top_pixels(15, 10), (3, 3), BlendMode::Normal, 1.0);
    let doc = with_raster(&doc, shape_pixels(17, 12), (12, 9), BlendMode::Screen, 0.7);
    let doc = styled(&doc, 2, &style);
    let (grouped, g) = group(&doc, &[1, 2], "Group");
    // Pass Through, plain and gated.
    assert_regions_match(&grouped, "pass-through group");
    let mut gated = grouped.clone();
    gated.layers[g].opacity = 0.7;
    assert_regions_match(&gated, "gated pass-through group");
    // Isolated: Multiply, and Normal at 0.8.
    let mut isolated = grouped.clone();
    isolated.layers[g].blend = BlendMode::Multiply;
    assert_regions_match(&isolated, "isolated multiply group");
    isolated.layers[g].blend = BlendMode::Normal;
    isolated.layers[g].opacity = 0.8;
    assert_regions_match(&isolated, "isolated normal group at 0.8");
    // Nested once more, the inner one isolated.
    let (nested, outer) = group(&isolated, &[g], "Outer");
    let mut nested = nested;
    nested.layers[outer].opacity = 0.9;
    assert_regions_match(&nested, "nested groups");
}

// ------------------------------------------------------------------ plan --

#[test]
fn an_effects_row_is_global_and_the_plan_reads_the_whole_canvas() {
    let (cw, ch) = (120u32, 90u32);
    let big = DrDocument::from_pixels(RgbaImage::from_fn(cw, ch, |x, y| {
        let c = plasma(x, y);
        Rgba([c[0], c[1], c[2], 255])
    }));
    let doc = with_raster(
        &big,
        shape_pixels(100, 70),
        (10, 10),
        BlendMode::Normal,
        1.0,
    );
    let doc = styled(
        &doc,
        1,
        &fx(&["{\"type\":\"gradient_overlay\",\"gradient\":{\"align_with_layer\":true}}"]),
    );
    let window = (55, 40, 6, 5);
    let p = plan(&doc, window, 1).unwrap();
    let row = &p["layers"][1];
    assert_eq!(row["style"], "effects");
    assert_eq!(
        row["neighborhood"],
        json!({"kind":"global","halo_pixels":null})
    );
    assert_eq!(row["global_analysis"], "layer_style_planes");
    assert_eq!(p["layers"][0]["style"], Value::Null);
    assert_eq!(p["layers"][0]["global_analysis"], false);
    assert_eq!(p["global_analysis"], json!(["layer_style_planes"]));
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":cw,"height":ch})
    );
    assert_eq!(
        p["requested_region"],
        json!({"x":55,"y":40,"width":6,"height":5})
    );
    assert_eq!(
        p["neighborhood"],
        json!({"kind":"pointwise","halo_pixels":0})
    );
    assert_eq!(p["contract_version"], 1);
    // A feathered mask on the styled layer adds no halo (the row stays
    // Global), and a feathered UNSTYLED layer beside it still sets the
    // top-level neighbourhood; the analysis list stays one entry for two
    // effect rows.
    let doc = masked(&doc, 1, gradient(40, 30), (30, 20), 1.0, 6.0);
    let doc = with_raster(&doc, top_pixels(30, 20), (50, 30), BlendMode::Normal, 1.0);
    let doc = masked(&doc, 2, gradient(30, 20), (50, 30), 1.0, 3.0);
    let doc = with_raster(&doc, shape_pixels(20, 20), (5, 60), BlendMode::Normal, 1.0);
    let doc = styled(&doc, 3, &fx(&["{\"type\":\"stroke\",\"size\":2}"]));
    let p = plan(&doc, window, 1).unwrap();
    assert_eq!(p["layers"][1]["neighborhood"]["kind"], "global");
    assert_eq!(p["layers"][1]["mask"], true);
    let halo = p["layers"][2]["neighborhood"]["halo_pixels"]
        .as_u64()
        .unwrap();
    assert_eq!(p["layers"][2]["neighborhood"]["kind"], "spatial");
    assert_eq!(p["layers"][2]["style"], Value::Null);
    assert!(halo > 0);
    assert_eq!(
        p["neighborhood"],
        json!({"kind":"spatial","halo_pixels":halo})
    );
    assert_eq!(p["layers"][3]["style"], "effects");
    assert_eq!(p["global_analysis"], json!(["layer_style_planes"]));
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":cw,"height":ch})
    );
    // Every one of them still renders equal to the crop.
    for w in [window, (0, 0, 7, 5), (cw - 9, ch - 7, 9, 7), (0, 0, cw, ch)] {
        let reference = full(&doc);
        for d in [1, 3] {
            assert_eq!(
                region(&doc, w, d).unwrap(),
                expected(&reference, cw, w, d),
                "{w:?}/{d}"
            );
        }
    }
}

#[test]
fn a_fill_row_keeps_the_pointwise_contract() {
    // No enabled effect (a disabled one, fill 0.5): "fill", not Global.
    let doc = with_raster(
        &base(),
        shape_pixels(17, 12),
        (8, 6),
        BlendMode::Normal,
        1.0,
    );
    let doc = styled(
        &doc,
        1,
        "{\"fill_opacity\":0.5,\"effects\":[{\"type\":\"stroke\",\"enabled\":false}]}",
    );
    let doc = masked(&doc, 1, gradient(14, 10), (10, 7), 1.0, 4.0);
    let window = (12, 9, 5, 4);
    let p = plan(&doc, window, 1).unwrap();
    let row = &p["layers"][1];
    assert_eq!(row["style"], "fill");
    assert_eq!(row["global_analysis"], false);
    assert_eq!(row["neighborhood"]["kind"], "spatial");
    let halo = row["neighborhood"]["halo_pixels"].as_u64().unwrap() as u32;
    assert_eq!(p["global_analysis"], false);
    let x0 = 12u32.saturating_sub(halo);
    let y0 = 9u32.saturating_sub(halo);
    let x1 = (12 + 5 + halo).min(W);
    let y1 = (9 + 4 + halo).min(H);
    assert_eq!(
        p["required_input_region"],
        json!({"x":x0,"y":y0,"width":x1 - x0,"height":y1 - y0})
    );
    // An enabled effect whose padded plane exceeds MAX_PIXELS renders no
    // planes at all (`Shape::of_layer` is `None`): also "fill". A 30000 ×
    // 1073 layer with an inner shadow at distance 2000 (pad capped at 1024):
    // (30000 + 2048)·(1073 + 2048) = 100,021,808 > 100,000,000.
    let wide = with_raster(
        &base(),
        RgbaImage::new(30000, 1073),
        (0, 0),
        BlendMode::Normal,
        1.0,
    );
    let wide = styled(
        &wide,
        1,
        &fx(&["{\"type\":\"inner_shadow\",\"distance\":2000,\"size\":0}"]),
    );
    let p = plan(&wide, (0, 0, 4, 4), 1).unwrap();
    assert_eq!(p["layers"][1]["style"], "fill");
    assert_eq!(p["layers"][1]["neighborhood"]["kind"], "pointwise");
    assert_eq!(p["global_analysis"], false);
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":4,"height":4})
    );
    assert_eq!(p["scratch_heap_bytes"], 16u64 * 16 + 16 * 4);
}

#[test]
fn the_plan_charges_the_style_planes_by_style_scratch() {
    // Outside stroke size 3 (reach 4, pad 5) and a colour overlay (reach 0):
    // no blur (sigma 0 → factor 1, 3 taps), two enabled effects. Restating
    // `style_scratch`: P = (w + 10)(h + 10); P·(2 + 2·2) u8 planes +
    // `plane_blur::float_plan` (P·32 + 3·4 + 4096) + a strip-sized f32
    // window 16·min(hp, 256)·wp.
    let doc = with_raster(
        &base(),
        shape_pixels(17, 12),
        (8, 6),
        BlendMode::Normal,
        0.6,
    );
    let doc = styled(
        &doc,
        1,
        &fx(&[
            "{\"type\":\"stroke\",\"size\":3}",
            "{\"type\":\"color_overlay\"}",
        ]),
    );
    let (wp, hp) = (17u64 + 10, 12u64 + 10);
    let planes = wp * hp;
    let style = planes * 6 + planes * 32 + 12 + 4096 + 16 * hp.min(256) * wp;
    let window = (4, 3, 9, 7);
    let p = plan(&doc, window, 1).unwrap();
    let pixels = 9u64 * 7;
    assert_eq!(p["scratch_heap_bytes"], pixels * 16 + pixels * 4 + style);
    // Work: the accumulator fill, one pass per row and — 13A-5b-iii-b-2 —
    // the planes' `style_scratch` work, P·(2 + 8·enabled) with two effects.
    assert_eq!(p["work_units"], pixels * (2 + 1) + planes * (2 + 8 * 2));
    assert_eq!(rows(&p), [0, 1]);
}

// ----------------------------------------------------------------- reach --

/// A 120×90 canvas with a 100×70 opaque layer at (10, 10) whose first row
/// and column are transparent except the corner pixel (10, 10): erasing that
/// ONE pixel moves the shape's bounding box.
fn reach_fixture(corner: u8, style: &str) -> DrDocument {
    let big = DrDocument::from_pixels(RgbaImage::from_fn(120, 90, |x, y| {
        let c = plasma(x, y);
        Rgba([c[0], c[1], c[2], 255])
    }));
    let pixels = RgbaImage::from_fn(100, 70, |x, y| {
        let alpha = match (x, y) {
            (0, 0) => corner,
            (0, _) | (_, 0) => 0,
            _ => 255,
        };
        Rgba([(x * 2) as u8, (y * 3) as u8, 90, alpha])
    });
    let doc = with_raster(&big, pixels, (10, 10), BlendMode::Normal, 1.0);
    styled(&doc, 1, style)
}

fn crop(doc: &DrDocument, window: (u32, u32, u32, u32)) -> Vec<u8> {
    expected(&full(doc), doc.width, window, 1).2
}

#[test]
fn a_far_pixel_outside_the_window_reaches_it_and_the_region_follows() {
    let aligned = fx(
        &["{\"type\":\"gradient_overlay\",\"gradient\":{\"angle\":30,\
                        \"align_with_layer\":true}}"],
    );
    let inner = fx(&["{\"type\":\"inner_shadow\",\"distance\":40,\"size\":0,\"opacity\":1}"]);
    // The inner shadow at 120° is shifted by (20, 35): the corner pixel's
    // hole lands at (30, 45), 40 px from it.
    for (label, style, window) in [
        ("aligned gradient", aligned.as_str(), (60, 45, 8, 6)),
        ("inner shadow 40", inner.as_str(), (27, 42, 7, 7)),
    ] {
        let on = reach_fixture(255, style);
        let off = reach_fixture(0, style);
        assert_ne!(
            crop(&on, window),
            crop(&off, window),
            "{label}: the far pixel changes the window"
        );
        for doc in [&on, &off] {
            let p = plan(doc, window, 1).unwrap();
            assert_eq!(
                p["required_input_region"],
                json!({"x":0,"y":0,"width":120,"height":90}),
                "{label}: the declared input contains the far pixel"
            );
            for d in [1, 2, 3] {
                assert_eq!(
                    region(doc, window, d).unwrap(),
                    expected(&full(doc), 120, window, d),
                    "{label} {window:?}/{d}"
                );
            }
        }
    }
}

// ------------------------------------------------------------ refusals --

#[test]
fn knockout_and_grouped_interiors_are_admitted_beside_group_effects() {
    // Since 13A-3c a knockout and a grouped interior take the unit pass
    // regionally and equal the reference (they refused before).
    let knockout = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}";
    let grouped = "{\"blend_interior_as_group\":true,\"effects\":[{\"type\":\"color_overlay\"}]}";
    let lone = with_raster(
        &base(),
        shape_pixels(17, 12),
        (5, 3),
        BlendMode::Normal,
        0.9,
    );
    assert_regions_match(&styled(&lone, 1, knockout), "a Shallow knockout");
    assert_regions_match(&styled(&lone, 1, grouped), "a grouped interior");
    let both = "{\"knockout\":\"deep\",\"blend_interior_as_group\":true,\
                \"effects\":[{\"type\":\"color_overlay\"}]}";
    assert_regions_match(&styled(&lone, 1, both), "Deep beside a grouped interior");
    // The flag without a grouped interior effect (a stroke is not one of the
    // four).
    let stroke_only = "{\"fill_opacity\":0.5,\"blend_interior_as_group\":true,\
                       \"effects\":[{\"type\":\"stroke\",\"size\":2}]}";
    assert_regions_match(
        &styled(&lone, 1, stroke_only),
        "flag without a grouped interior",
    );
    // A group with an enabled effect is admitted since 13A-3b-ii (the
    // materialized group), and so is a knockout beside the effect.
    let doc = with_raster(&lone, top_pixels(9, 9), (2, 2), BlendMode::Normal, 1.0);
    let (doc, g) = group(&doc, &[1, 2], "Group");
    let shadow = fx(&["{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}"]);
    assert_regions_match(&styled(&doc, g, &shadow), "a group with effects");
    let group_both = "{\"knockout\":\"shallow\",\"effects\":[{\"type\":\"drop_shadow\"}]}";
    assert_regions_match(
        &styled(&doc, g, group_both),
        "a knockout group with effects",
    );
    // …an effect-less styled group is admitted (13A-2b/13A-3a).
    assert_regions_match(
        &styled(&doc, g, "{\"fill_opacity\":0.5}"),
        "fill-only group",
    );
    // A knockout base under a grouped-interior member.
    let mut unit = with_raster(&lone, top_pixels(9, 9), (2, 2), BlendMode::Normal, 1.0);
    unit.layers[2].clipped = true;
    let unit = styled(&unit, 2, grouped);
    let unit = styled(&unit, 1, knockout);
    assert_regions_match(&unit, "a knockout base with a grouped member");
    // A guided Shadows/Highlights member of the same unit: under the
    // knockout (routed) base it is staged since 13A-4b-ii-b-2 and equals the
    // reference.
    let mut guided = unit
        .adding_image_layer(unit.layers.len() - 1, RgbaImage::new(1, 1), "Guided")
        .expect("add layer");
    let top = guided.layers.len() - 1;
    guided.layers[top].clipped = true;
    guided.layers[top].meta = Some(adjust_meta(
        "shadows_highlights",
        "{\"shadows\":{\"amount\":0.6,\"tone\":0.4},\"radius\":12}",
    ));
    assert_regions_match(&guided, "a guided member under a knockout base");
    // A hidden styled group is skipped unchecked.
    let mut hidden = styled(&doc, g, &shadow);
    hidden.layers[g].visible = false;
    assert_regions_match(&hidden, "hidden styled group");
}

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_styled_render_returns_no_output() {
    let doc = styled(&clip_stack(BlendMode::Normal, 0.6), 1, BASE_STYLE);
    let h = handle(&doc);
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_render_region(h, 0, 0, W, H, 1, u64::MAX, control, &mut err) };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), "operation cancelled");
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe {
        dr_job_control_free(control);
        dr_doc_free(h);
    }
}

const CANCEL_STYLE: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2},\
                            {\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#80c0ff\"}]}";

/// Runs `render(control)` on a worker, polls until the progress exceeds
/// `loop_start` (the fraction done before the sample loop — the preparation
/// stages, plane rendering included, are the reference's own and not
/// cancellable) and returns (time from the loop's start to the return,
/// uncancelled) or, cancelling there, the cancel-to-return latency.
fn timed<F>(control: *mut DrJobControl, loop_start: f64, cancel: bool, render: F) -> Duration
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let token = control as usize;
    thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let (ok, message) = render(token);
            (ok, message, Instant::now())
        });
        let deadline = Instant::now() + Duration::from_secs(120);
        while unsafe { dr_job_control_progress(control) } <= loop_start && !worker.is_finished() {
            assert!(Instant::now() < deadline, "the loop never started");
            thread::sleep(Duration::from_micros(200));
        }
        assert!(!worker.is_finished(), "render completed before the probe");
        let at = Instant::now();
        if cancel {
            unsafe { dr_job_control_cancel(control) };
        }
        let (ok, message, returned) = worker.join().unwrap();
        if cancel {
            assert!(!ok, "a cancelled render returns no image");
            assert_eq!(message, "operation cancelled");
        } else {
            assert!(ok, "{message}");
        }
        returned.saturating_duration_since(at)
    })
}

/// The 8-bit concurrent cancel. With `bench` it calibrates and measures the
/// cancel-to-return latency (a wall-clock record, `make cancel-bench`);
/// without it the loop's first report's checkpoint count comes from a
/// fresh twin's own report log and the worker is cancelled PARKED at the
/// next checkpoint (`cancelled_past_each`), no poller.
fn cancel_styled_render(bench: bool) {
    let (w, h) = (3000u32, 2000u32);
    let build = || {
        let doc = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
            Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
        }));
        let doc = with_raster(
            &doc,
            RgbaImage::from_fn(w - 40, h - 40, |x, y| {
                Rgba([
                    (x * 3) as u8,
                    90,
                    (y * 5) as u8,
                    if (x + y) % 7 == 0 { 0 } else { 200 },
                ])
            }),
            (20, 20),
            BlendMode::SoftLight,
            0.9,
        );
        styled(&doc, 1, CANCEL_STYLE)
    };
    let render_at = move |address: usize| {
        move |token: usize| {
            let mut err: *mut c_char = ptr::null_mut();
            let out = unsafe {
                dr_doc_render_region(
                    address as *const DrDocument,
                    0,
                    0,
                    w,
                    h,
                    1,
                    u64::MAX,
                    token as *const _,
                    &mut err,
                )
            };
            let ok = !out.is_null();
            if ok {
                unsafe { dr_image_free(out) };
            }
            (ok, take_err_string(err))
        }
    };
    let doc = build();
    let hd = handle(&doc);
    let render = render_at(hd as usize);
    // The byte loop reports progress after its first row: anything above the
    // plan's loop start (13A-5b-iii-b-2: the fill and the preparation report).
    let start = common::probe::loop_start(&plan(&doc, (0, 0, w, h), 1).unwrap()) + 1e-6;
    if bench {
        let control = unsafe { dr_job_control_create() };
        let looped = timed(control, start, false, render);
        unsafe { dr_job_control_free(control) };
        let per_sample = looped.as_secs_f64() / (f64::from(w) * f64::from(h));
        let control = unsafe { dr_job_control_create() };
        let latency = timed(control, start, true, render);
        eprintln!(
            "13A-3b-i 8-bit cancel: {:.3} ms cancel-to-return; {:.2} ns per root sample \
             (loop {:.1} ms); ≈{:.0} root samples after the cancel (declared bound 4096)",
            latency.as_secs_f64() * 1e3,
            per_sample * 1e9,
            looped.as_secs_f64() * 1e3,
            latency.as_secs_f64() / per_sample,
        );
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        unsafe { dr_job_control_free(control) };
    } else {
        let twin = |token: usize| {
            let twin = handle(&build());
            let out = render_at(twin as usize)(token);
            unsafe { dr_doc_free(twin) };
            out
        };
        let parked = common::probe::cancelled_past_each(&[start], twin, render);
        eprintln!(
            "8-bit cancel inside the loop: {}",
            common::probe::parked_line(&parked[0])
        );
    }
    unsafe { dr_doc_free(hd) };
}

#[test]
fn a_concurrently_cancelled_styled_render_returns_operation_cancelled() {
    cancel_styled_render(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_styled_render_is_measured() {
    cancel_styled_render(true);
}

/// The u16 concurrent cancel, `cancel_styled_render`'s two paths.
fn cancel_styled_render_at_u16(bench: bool) {
    let (w, h) = (3000u32, 2000u32);
    let profile = IccProfile::srgb();
    let image = |iw: u32, ih: u32, seed: u32, translucent: bool| {
        let pixels = (0..iw * ih)
            .map(|i| {
                let (x, y) = (i % iw, i / iw);
                [
                    ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                    ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                    ((x ^ y) * 1999 % 65536 + seed) as u16,
                    if !translucent {
                        65535
                    } else if (x + y) % 7 == 0 {
                        0
                    } else {
                        51000
                    },
                ]
            })
            .collect();
        PrecisionImage::from_rgba16(iw, ih, pixels, profile.clone(), None).unwrap()
    };
    let build = || {
        let style = LayerStyle::from_json(CANCEL_STYLE).unwrap();
        let top = PrecisionDocumentLayer::raster(
            "Top",
            image(w - 40, h - 40, 3, true),
            (20, 20),
            0.9,
            true,
        )
        .unwrap()
        .with_blend_mode(BlendMode::SoftLight)
        .unwrap()
        .with_layer_style(Some(&style), (w, h))
        .unwrap();
        let layers = vec![
            PrecisionDocumentLayer::raster("Base", image(w, h, 1, false), (0, 0), 1.0, true)
                .unwrap(),
            top,
        ];
        PrecisionDocument::from_parts(
            w,
            h,
            profile.clone(),
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap()
    };
    let d = build();
    // The loop begins once the accumulator fill and the plane rendering
    // are advanced: (work − pixels·rows) / work.
    let p = region_plan::typed(&d, (0, 0, w, h), 1).unwrap_or_else(|e| panic!("{e}"));
    let work = p["work_units"].as_u64().unwrap() as f64;
    let pixels = f64::from(w) * f64::from(h);
    let rows = p["layers"].as_array().unwrap().len() as f64;
    let loop_start = (work - pixels * rows) / work;
    let limit = REFERENCE_WORKING_BYTES;
    let render_at = move |address: usize| {
        move |token: usize| {
            let mut err: *mut c_char = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_render_region(
                    address as *const PrecisionDocument,
                    0,
                    0,
                    w,
                    h,
                    1,
                    limit,
                    0,
                    token as *const _,
                    &mut err,
                )
            };
            let ok = !out.is_null();
            if ok {
                unsafe { dr_precision_image_free(out) };
            }
            (ok, take_err_string(err))
        }
    };
    let render = render_at(&d as *const PrecisionDocument as usize);
    if bench {
        let control = unsafe { dr_job_control_create() };
        let looped = timed(control, loop_start, false, render);
        unsafe { dr_job_control_free(control) };
        let per_sample = looped.as_secs_f64() / pixels;
        let control = unsafe { dr_job_control_create() };
        let latency = timed(control, loop_start, true, render);
        eprintln!(
            "13A-3b-i u16 cancel: {:.3} ms cancel-to-return; {:.2} ns per root sample \
             (loop {:.1} ms); ≈{:.0} root samples after the cancel (declared bound 4096)",
            latency.as_secs_f64() * 1e3,
            per_sample * 1e9,
            looped.as_secs_f64() * 1e3,
            latency.as_secs_f64() / per_sample,
        );
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        unsafe { dr_job_control_free(control) };
    } else {
        let twin = |token: usize| {
            let twin = build();
            render_at(&twin as *const PrecisionDocument as usize)(token)
        };
        let parked = common::probe::cancelled_past_each(&[loop_start], twin, render);
        eprintln!(
            "u16 cancel inside the loop: {}",
            common::probe::parked_line(&parked[0])
        );
    }
}

#[test]
fn a_concurrently_cancelled_styled_render_returns_operation_cancelled_at_u16() {
    cancel_styled_render_at_u16(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_styled_render_is_measured_at_u16() {
    cancel_styled_render_at_u16(true);
}
