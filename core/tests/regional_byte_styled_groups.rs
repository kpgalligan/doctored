//! Phase 13A-3b-ii: the 8-bit regional document render over styled GROUPS
//! WITH ENABLED EFFECTS — materialized by `doc_group::rendered_group` itself
//! and composited as its synthetic styled raster — every effect, the package
//! model, fill + Blend If beside effects, the group's mask, mixed children, the
//! styled group as a clip base and a clip member, inside other groups and
//! nested; the plan's `Global {"group_materialization"}` rows and formulas;
//! the refusals; the `RenderBudget` counterpart through the
//! `…_with_group_budget` twins against `flattened_with_group_budget`; and the
//! concurrent cancels at BOTH depths (the u16 twin lives here, as 13A-3b-i's
//! does, so one command measures both). The oracle is INDEPENDENT of the
//! regional code: the full projection cropped and top-left-cell sampled here
//! in plain index arithmetic, byte for byte.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_evaluation::Evaluation;
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
use doctored_core::style::LayerStyle;
use doctored_core::RegionRequest;
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

fn full_result(doc: &DrDocument) -> Result<Vec<u8>, String> {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_flattened(h, 0, std::ptr::null(), &mut err) };
    unsafe { dr_doc_free(h) };
    if img.is_null() {
        return Err(take_err_string(err));
    }
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    Ok(out)
}

fn full(doc: &DrDocument) -> Vec<u8> {
    full_result(doc).unwrap_or_else(|e| panic!("flattened: {e}"))
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
fn windows(extra: &[(u32, u32, u32, u32)]) -> Vec<(u32, u32, u32, u32)> {
    let mut windows = vec![
        (0, 0, W, H),
        (3, 2, 11, 7),
        (0, 0, 5, 4),
        (W - 6, 0, 6, 5),
        (0, H - 3, 7, 3),
        (W - 4, H - 5, 4, 5),
        (0, 4, W, 3),
        (7, 0, 2, H),
        (W - 1, H - 1, 1, 1),
        (0, 0, 1, 1),
        (19, 13, 1, 1),
    ];
    windows.extend_from_slice(extra);
    windows
}

const DIVISORS: [u32; 5] = [1, 2, 3, 7, u32::MAX];

fn assert_regions_match_with(doc: &DrDocument, what: &str, extra: &[(u32, u32, u32, u32)]) {
    let reference = full(doc);
    for window in windows(extra) {
        for d in DIVISORS {
            let got =
                region(doc, window, d).unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, W, window, d),
                "{what}: window {window:?} divisor {d}"
            );
        }
    }
}

fn assert_regions_match(doc: &DrDocument, what: &str) {
    assert_regions_match_with(doc, what, &[]);
}

// ------------------------------------------------------------ budgets --

fn request((x, y, width, height): (u32, u32, u32, u32), d: u32) -> RegionRequest {
    RegionRequest {
        x,
        y,
        width,
        height,
        preview_divisor: d,
    }
}

fn region_budget(
    doc: &DrDocument,
    window: (u32, u32, u32, u32),
    d: u32,
    group_pixels: u64,
) -> Result<(u32, u32, Vec<u8>), String> {
    doc.render_region_with_group_budget(request(window, d), u64::MAX, group_pixels, None)
        .map(|img| (img.width(), img.height(), img.into_raw()))
}

fn plan_budget(
    doc: &DrDocument,
    window: (u32, u32, u32, u32),
    group_pixels: u64,
) -> Result<Value, String> {
    doc.render_region_plan_with_group_budget(request(window, 1), u64::MAX, group_pixels)
        .map(|p| p.json())
}

fn budget_sentence(group_pixels: u64) -> String {
    format!(
        "rendering exceeds the group buffer budget ({group_pixels} pixels); hide or simplify \
         nested groups, or save as Doctored (.doctored) to preserve the document"
    )
}

/// At `group_pixels` the reference, the plan and the regional render all
/// refuse with the ONE budget sentence.
fn assert_budget_refuses(doc: &DrDocument, group_pixels: u64, what: &str) {
    let sentence = budget_sentence(group_pixels);
    assert_eq!(
        doc.flattened_with_group_budget(group_pixels).unwrap_err(),
        sentence,
        "{what}: the reference"
    );
    assert_eq!(
        plan_budget(doc, (2, 1, 9, 6), group_pixels).unwrap_err(),
        sentence,
        "{what}: the plan"
    );
    assert_eq!(
        region_budget(doc, (2, 1, 9, 6), 1, group_pixels).unwrap_err(),
        sentence,
        "{what}: the render"
    );
}

/// At `group_pixels` the plan admits and every window equals the crop of
/// the reference rendered at `reference_pixels`.
fn assert_budget_admits(doc: &DrDocument, group_pixels: u64, reference_pixels: u64, what: &str) {
    let reference = doc
        .flattened_with_group_budget(reference_pixels)
        .unwrap_or_else(|e| panic!("{what}: reference: {e}"))
        .into_raw();
    plan_budget(doc, (0, 0, W, H), group_pixels).unwrap_or_else(|e| panic!("{what}: plan: {e}"));
    for window in windows(&[]) {
        for d in [1, 3] {
            let got = region_budget(doc, window, d, group_pixels)
                .unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, W, window, d),
                "{what}: {window:?}/{d}"
            );
        }
    }
}

// ------------------------------------------------------------- fixtures --

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
/// graded alpha near the rim.
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

fn opaque(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([(x * 23) as u8, (y * 41) as u8, ((x ^ y) * 9) as u8, 255])
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

/// An unmasked adjustment layer added at the top of the stack.
fn with_adjustment(doc: &DrDocument, op: &str, params: &str) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, RgbaImage::new(1, 1), op)
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].meta = Some(adjust_meta(op, params));
    out
}

/// `group_layers` over `idx` (a new Pass Through group), and its index.
fn group(doc: &DrDocument, idx: &[usize], name: &str) -> (DrDocument, usize) {
    let (out, g, ..) = doc.group_layers(idx, name).expect("group");
    (out, g)
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

/// Every effect the renderer knows (the 13A-3b-i list).
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

const GROUP_STYLE: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                           {\"type\":\"stroke\",\"size\":3},\
                           {\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#80c0ff\"}]}";

/// Background (0), a shaped raster A at (6, 5) (1) and a translucent Screen
/// 0.8 raster B partly off the canvas at (-4, 15) (2), grouped (3) with
/// `mode`/`opacity` and `style` on the GROUP.
fn styled_group(style: &str, mode: BlendMode, opacity: f32) -> (DrDocument, usize) {
    let doc = with_raster(
        &base(),
        shape_pixels(17, 12),
        (6, 5),
        BlendMode::Normal,
        1.0,
    );
    let doc = with_raster(&doc, top_pixels(13, 10), (-4, 15), BlendMode::Screen, 0.8);
    let (mut doc, g) = group(&doc, &[1, 2], "Group");
    doc.layers[g].blend = mode;
    doc.layers[g].opacity = opacity;
    (styled(&doc, g, style), g)
}

// ------------------------------------------------------------- lone --

#[test]
fn every_effect_on_a_lone_styled_group_matches_the_projection() {
    for (label, style) in every_effect() {
        for (mode, opacity) in VARIANTS {
            let (doc, _) = styled_group(&style, mode, opacity);
            let what = format!("{label} {mode:?} {opacity}");
            assert_regions_match_with(&doc, &what, &around((-4, 5), 27, 20, 4));
        }
    }
}

#[test]
fn fill_and_blend_if_beside_group_effects_match() {
    let style = format!(
        "{{\"fill_opacity\":0.5,{BLEND_IF_GRAY},\"effects\":[\
         {{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}},\
         {{\"type\":\"color_overlay\",\"opacity\":0.5}},\
         {{\"type\":\"inner_glow\",\"size\":3}}]}}"
    );
    for (mode, opacity) in VARIANTS {
        let (doc, _) = styled_group(&style, mode, opacity);
        let what = format!("fill + blend if + effects {mode:?} {opacity}");
        assert_regions_match_with(&doc, &what, &around((-4, 5), 27, 20, 3));
    }
}

#[test]
fn a_masked_styled_group_matches_with_feather_0_and_6() {
    for feather in [0.0, 6.0] {
        for (mode, opacity) in [(BlendMode::Normal, 1.0), (BlendMode::Multiply, 0.6)] {
            let (doc, g) = styled_group(GROUP_STYLE, mode, opacity);
            let doc = masked(&doc, g, gradient(20, 14), (4, 6), 0.8, feather);
            let what = format!("masked group feather {feather} {mode:?} {opacity}");
            assert_regions_match_with(&doc, &what, &around((4, 6), 20, 14, 0));
        }
    }
}

/// A styled group over MIXED children: a raster base A (1) with a clipped
/// Multiply member (2), a hue/saturation adjustment (3), a feather-6 masked
/// raster (4), a gated (0.6) pass-through subgroup (5, 6), a Multiply
/// isolated subgroup (7, 8), a hidden raster (9) and a zero-opacity raster
/// (10) — grouped (11).
fn mixed() -> (DrDocument, usize) {
    let doc = with_raster(
        &base(),
        shape_pixels(17, 12),
        (6, 5),
        BlendMode::Normal,
        1.0,
    );
    let mut doc = with_raster(&doc, top_pixels(15, 15), (14, 8), BlendMode::Multiply, 0.6);
    doc.layers[2].clipped = true;
    let doc = with_adjustment(&doc, "hue_saturation", "{\"hue\":-60,\"saturation\":0.2}");
    let doc = with_raster(&doc, top_pixels(12, 9), (20, 3), BlendMode::Normal, 0.9);
    let doc = masked(&doc, 4, gradient(10, 8), (21, 4), 1.0, 6.0);
    let doc = with_raster(&doc, shape_pixels(11, 8), (2, 16), BlendMode::Normal, 1.0);
    let (mut doc, gated) = group(&doc, &[5], "Gated");
    doc.layers[gated].opacity = 0.6;
    let doc = with_raster(&doc, top_pixels(10, 7), (25, 17), BlendMode::Normal, 1.0);
    let (mut doc, multiply) = group(&doc, &[7], "Multiply");
    doc.layers[multiply].blend = BlendMode::Multiply;
    let mut doc = with_raster(&doc, top_pixels(8, 8), (30, 2), BlendMode::Normal, 1.0);
    doc.layers[9].visible = false;
    let doc = with_raster(&doc, top_pixels(8, 8), (1, 1), BlendMode::Normal, 0.0);
    let (doc, g) = group(&doc, &[1, 2, 3, 4, gated, multiply, 9, 10], "Styled");
    assert!(doc.layers[2].clipped, "the member stays clipped inside");
    (doc, g)
}

#[test]
fn a_styled_group_over_mixed_children_matches() {
    let (doc, g) = mixed();
    for (mode, opacity) in VARIANTS {
        let mut doc = styled(&doc, g, GROUP_STYLE);
        doc.layers[g].blend = mode;
        doc.layers[g].opacity = opacity;
        assert_regions_match(&doc, &format!("mixed children {mode:?} {opacity}"));
    }
}

// ------------------------------------------------------------ clip units --

#[test]
fn a_styled_group_as_a_clip_base_clamps_its_members() {
    for (mode, opacity) in VARIANTS {
        let (doc, _) = styled_group(GROUP_STYLE, mode, opacity);
        // A Multiply 0.6 member exceeding the group's extent, then a
        // VISIBLE zero-opacity member (a clamp-only step).
        let mut doc = with_raster(&doc, top_pixels(15, 15), (20, 12), BlendMode::Multiply, 0.6);
        doc.layers[4].clipped = true;
        let mut doc = with_raster(&doc, top_pixels(9, 9), (2, 2), BlendMode::Normal, 0.0);
        doc.layers[5].clipped = true;
        let what = format!("styled group base {mode:?} {opacity}");
        assert_regions_match_with(&doc, &what, &around((20, 12), 15, 15, 2));
        let mut only_clamp = doc.clone();
        only_clamp.layers[4].visible = false;
        assert_regions_match(&only_clamp, &format!("{what}, clamp-only step"));
    }
}

#[test]
fn a_styled_group_member_matches_under_an_unstyled_and_a_styled_base() {
    for (mode, opacity) in [(BlendMode::Normal, 1.0), (BlendMode::Multiply, 0.6)] {
        let doc = with_raster(
            &base(),
            shape_pixels(19, 13),
            (6, 5),
            BlendMode::Normal,
            1.0,
        );
        let doc = with_raster(&doc, shape_pixels(17, 12), (12, 9), BlendMode::Normal, 1.0);
        let doc = with_raster(&doc, top_pixels(13, 10), (2, 14), BlendMode::Screen, 0.8);
        let (mut doc, g) = group(&doc, &[2, 3], "Member");
        doc.layers[g].clipped = true;
        doc.layers[g].blend = mode;
        doc.layers[g].opacity = opacity;
        let doc = styled(&doc, g, GROUP_STYLE);
        let what = format!("styled group member {mode:?} {opacity}");
        assert_regions_match(&doc, &what);
        let doc = styled(
            &doc,
            1,
            &fx(&["{\"type\":\"stroke\",\"size\":2,\"color\":\"#ffffff\"}"]),
        );
        assert_regions_match(&doc, &format!("{what}, styled base"));
    }
}

// ---------------------------------------------------------------- groups --

#[test]
fn a_styled_group_inside_other_groups_and_nested_matches() {
    let (doc, g) = styled_group(GROUP_STYLE, BlendMode::Normal, 0.8);
    let (outer, o) = group(&doc, &[g], "Outer");
    assert_regions_match(&outer, "inside a plain pass-through group");
    let mut gated = outer.clone();
    gated.layers[o].opacity = 0.6;
    assert_regions_match(&gated, "inside a gated pass-through group");
    let mut isolated = outer.clone();
    isolated.layers[o].blend = BlendMode::Multiply;
    assert_regions_match(&isolated, "inside a Multiply isolated group");
    // NESTED: styled inside styled.
    let nested = styled(
        &outer,
        o,
        &fx(&[
            "{\"type\":\"outer_glow\",\"size\":3}",
            "{\"type\":\"inner_shadow\",\"distance\":3,\"size\":2}",
        ]),
    );
    assert_regions_match(&nested, "styled inside styled");
    let mut nested = nested;
    nested.layers[o].opacity = 0.7;
    assert_regions_match(&nested, "styled inside styled, outer package");
}

// ------------------------------------------------------------------ plan --

/// A group styled with an outside stroke 3 + colour overlay (pad 5) over
/// children whose union is (10, 8)–(30, 23): `rendered_group`'s rect is that
/// union (a group's OWN style grows only its parent's extent), 20×15.
/// Inside: a Multiply isolated subgroup over a 6×3 raster (1, 2) and a gated
/// (0.5) pass-through subgroup over a 20×15 raster (3, 4); the styled group
/// is 5. The nest's peak is 300 + max(18, 300) = 600.
fn plan_fixture() -> (DrDocument, usize) {
    let doc = with_raster(&base(), opaque(6, 3), (12, 9), BlendMode::Normal, 1.0);
    let (mut doc, a) = group(&doc, &[1], "A");
    doc.layers[a].blend = BlendMode::Multiply;
    let doc = with_raster(&doc, opaque(20, 15), (10, 8), BlendMode::Normal, 1.0);
    let (mut doc, b) = group(&doc, &[3], "B");
    doc.layers[b].opacity = 0.5;
    let (doc, g) = group(&doc, &[a, b], "Styled");
    let doc = styled(
        &doc,
        g,
        &fx(&[
            "{\"type\":\"stroke\",\"size\":3}",
            "{\"type\":\"color_overlay\"}",
        ]),
    );
    (doc, g)
}

#[test]
fn a_materialized_row_is_global_and_charges_its_nest() {
    let (doc, g) = plan_fixture();
    assert_eq!(g, 5);
    let window = (4, 3, 9, 7);
    let p = plan(&doc, window, 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3, 4, 5]);
    let row = &p["layers"][5];
    assert_eq!(row["kind"], "group");
    assert_eq!(row["operator"], "isolated");
    assert_eq!(row["style"], "effects");
    assert_eq!(
        row["neighborhood"],
        json!({"kind":"global","halo_pixels":null})
    );
    assert_eq!(row["global_analysis"], "group_materialization");
    assert_eq!(p["global_analysis"], json!(["group_materialization"]));
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":W,"height":H})
    );
    // Children rows are ordinary rows naming their enclosing group.
    assert_eq!(p["layers"][1]["group"], 2);
    assert_eq!(p["layers"][2]["group"], 5);
    assert_eq!(p["layers"][3]["group"], 4);
    assert_eq!(p["layers"][4]["group"], 5);
    assert_eq!(p["layers"][4]["operator"], "pass_through");
    assert_eq!(p["layers"][1]["global_analysis"], false);
    // Step 3 restated: peak·16 + |rect|·4 + planes, the planes restating
    // `style_scratch` over the 20×15 synthetic (pad 5, two effects, 3
    // taps): P = 30·25; P·(2 + 2·2) + P·32 + 3·4 + 4096 + 16·min(25, 256)·30.
    let (wp, hp) = (30u64, 25u64);
    let planes = wp * hp;
    let style = planes * 6 + planes * 32 + 12 + 4096 + 16 * hp.min(256) * wp;
    // 13A-5c-3: plus the nest's uncharged peak — subgroup A's 6×3 synthetic
    // RGBA8, held while its unit composites (B's gated pass over its lone
    // unstyled raster allocates nothing uncharged).
    let materialization = 600 * 16 + 300 * 4 + style + 18 * 4;
    let pixels = 9u64 * 7;
    assert_eq!(
        p["scratch_heap_bytes"],
        pixels * 16 + pixels * 4 + materialization
    );
    // Work: the fill, one pass per row and — 13A-5b-iii-b-2 — the
    // materialization, |rect|·(1 + 4 rows inside), and the synthetic's
    // planes, P·(2 + 8·2) over the 30×25 padded plane.
    assert_eq!(
        p["work_units"],
        pixels * (6 + 1) + 300 * (1 + 4) + planes * (2 + 8 * 2)
    );
    // And it renders equal to the crop.
    assert_regions_match(&doc, "plan fixture");
}

#[test]
fn a_styled_raster_below_lists_both_analyses_and_a_feathered_group_mask_adds_no_halo() {
    let (doc, g) = styled_group(GROUP_STYLE, BlendMode::Normal, 1.0);
    let doc = masked(&doc, g, gradient(20, 14), (4, 6), 1.0, 6.0);
    // A styled raster BELOW the group (index 1).
    let doc = styled(&doc, 1, &fx(&["{\"type\":\"stroke\",\"size\":2}"]));
    let window = (12, 9, 5, 4);
    let p = plan(&doc, window, 1).unwrap();
    assert_eq!(p["layers"][1]["global_analysis"], "layer_style_planes");
    assert_eq!(p["layers"][3]["global_analysis"], "group_materialization");
    assert_eq!(p["layers"][3]["mask"], true);
    assert_eq!(
        p["global_analysis"],
        json!(["layer_style_planes", "group_materialization"])
    );
    assert_eq!(
        p["neighborhood"],
        json!({"kind":"pointwise","halo_pixels":0})
    );
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":W,"height":H})
    );
    assert_regions_match(&doc, "both analyses");
}

#[test]
fn a_fill_group_past_max_pixels_is_global_with_no_style_term() {
    // The group's rect is the 30000 × 1073 canvas; an inner shadow at
    // distance 2000 caps the pad at 1024: (30000 + 2048)·(1073 + 2048) =
    // 100,021,808 > MAX_PIXELS, so the synthetic renders no planes ("fill"),
    // but `rendered_group` still reads the whole extent (Global).
    let big = DrDocument::from_pixels(RgbaImage::new(30000, 1073));
    let (doc, g) = group(&big, &[0], "Wide");
    let doc = styled(
        &doc,
        g,
        &fx(&["{\"type\":\"inner_shadow\",\"distance\":2000,\"size\":0}"]),
    );
    let p = plan(&doc, (0, 0, 4, 4), 1).unwrap();
    assert_eq!(p["layers"][1]["style"], "fill");
    assert_eq!(p["layers"][1]["neighborhood"]["kind"], "global");
    assert_eq!(p["global_analysis"], json!(["group_materialization"]));
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":30000,"height":1073})
    );
    let rect = 30000u64 * 1073;
    assert_eq!(
        p["scratch_heap_bytes"],
        16u64 * 16 + 16 * 4 + rect * 16 + rect * 4
    );
}

// ------------------------------------------------------------ refusals --

#[test]
fn refusals_inside_a_materialized_group_come_in_order() {
    let knockout = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}";
    // Since 13A-3c a knockout on the materialized group and on its child are
    // admitted and equal the reference (the child's hole stops at the
    // group's buffer, D4's hard stop).
    let (doc, g) = styled_group(GROUP_STYLE, BlendMode::Normal, 1.0);
    let both = "{\"knockout\":\"shallow\",\"effects\":[{\"type\":\"drop_shadow\"}]}";
    let child_too = styled(&doc, 1, knockout);
    assert_regions_match(&styled(&child_too, g, both), "a knockout group and child");
    assert_regions_match(&child_too, "a knockout child in a materialized group");
    // A guided Shadows/Highlights child: admitted since 13A-4b-i (the
    // reference renders the materialized subtree) and equal to it.
    let guided = with_raster(
        &base(),
        shape_pixels(17, 12),
        (6, 5),
        BlendMode::Normal,
        1.0,
    );
    let guided = with_adjustment(
        &guided,
        "shadows_highlights",
        "{\"shadows\":{\"amount\":0.6,\"tone\":0.4},\"radius\":12}",
    );
    let (guided, gg) = group(&guided, &[1, 2], "Group");
    assert_regions_match(
        &styled(&guided, gg, GROUP_STYLE),
        "a guided child in a materialized group",
    );
    // A HIDDEN styled group is skipped unchecked.
    let mut hidden = child_too.clone();
    hidden.layers[g].visible = false;
    assert_regions_match(&hidden, "hidden styled group");
}

#[test]
fn a_nest_past_the_limit_under_a_styled_outermost_group_refuses_in_its_words() {
    let mut doc = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Normal, 1.0);
    for level in 0..10 {
        let top = doc.layers.len() - 1;
        doc = group(&doc, &[top], &format!("G{level}")).0;
    }
    let innermost = doc
        .layers
        .iter()
        .position(|l| l.kind == doctored_core::doc::LayerKind::Group)
        .unwrap();
    let mut entry = doc.layers[innermost].clone();
    entry.depth = 10;
    entry.name = "Deep".into();
    doc.layers.insert(innermost, entry);
    let outermost = doc.layers.len() - 1;
    let doc = styled(&doc, outermost, GROUP_STYLE);
    let reference = full_result(&doc).unwrap_err();
    assert_eq!(reference, "rendering exceeds the group nesting limit");
    assert_eq!(plan(&doc, (0, 0, 4, 4), 1).unwrap_err(), reference);
    assert_eq!(region(&doc, (0, 0, 4, 4), 1).unwrap_err(), reference);
}

#[test]
fn a_styled_group_document_refuses_linear_light() {
    let (doc, _) = styled_group(GROUP_STYLE, BlendMode::Normal, 1.0);
    assert!(doc.with_evaluation(Evaluation::LinearLight).is_err());
}

// --------------------------------------------------------------- budget --

const OVERLAY: &str = "{\"effects\":[{\"type\":\"color_overlay\",\"color\":\"#30c080\"}]}";

/// A group styled with a colour overlay over a 20×15 raster at (10, 8): its
/// `rendered_group` rect is the child's, (10, 8)–(30, 23) = 300 pixels.
fn lone_300() -> (DrDocument, usize) {
    let doc = with_raster(&base(), opaque(20, 15), (10, 8), BlendMode::Normal, 1.0);
    let (doc, g) = group(&doc, &[1], "Lone");
    (styled(&doc, g, OVERLAY), g)
}

#[test]
fn a_lone_materialized_group_is_charged_as_the_reference_charges_it() {
    let (doc, _) = lone_300();
    assert_budget_refuses(&doc, 299, "lone 299");
    assert_budget_admits(&doc, 300, 300, "lone 300");
}

#[test]
fn nested_isolated_and_gated_subgroups_peak_at_600() {
    // Inside the 300-pixel group: a Multiply isolated subgroup over a 10×8
    // raster (80) and a gated (0.5) pass-through subgroup, whose copy is the
    // whole 300: peak 300 + max(80, 300) = 600.
    let doc = with_raster(&base(), opaque(10, 8), (10, 8), BlendMode::Normal, 1.0);
    let (mut doc, a) = group(&doc, &[1], "A");
    doc.layers[a].blend = BlendMode::Multiply;
    let doc = with_raster(&doc, top_pixels(20, 15), (10, 8), BlendMode::Normal, 1.0);
    let (mut doc, b) = group(&doc, &[3], "B");
    doc.layers[b].opacity = 0.5;
    let (doc, g) = group(&doc, &[a, b], "Styled");
    let doc = styled(&doc, g, OVERLAY);
    assert_budget_refuses(&doc, 599, "nested 599");
    assert_budget_admits(&doc, 600, 600, "nested 600");
}

#[test]
fn a_zero_opacity_shallow_child_snapshots_in_a_plain_pass_through_subgroup() {
    // A plain pass-through subgroup holding a VISIBLE zero-opacity Shallow
    // child snapshots the 300-pixel buffer (`level_wants_shallow`): peak
    // 600. The background is below opacity 1, so D5's root snapshot is off.
    let mut doc = with_raster(&base(), top_pixels(20, 15), (10, 8), BlendMode::Normal, 1.0);
    doc.layers[0].opacity = 0.95;
    let doc = with_raster(&doc, opaque(6, 6), (12, 9), BlendMode::Normal, 0.0);
    let doc = styled(&doc, 2, "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}");
    let (doc, p) = group(&doc, &[1, 2], "Pass");
    let (doc, g) = group(&doc, &[p], "Styled");
    let doc = styled(&doc, g, OVERLAY);
    assert_budget_refuses(&doc, 599, "shallow snapshot 599");
    assert_budget_admits(&doc, 600, 600, "shallow snapshot 600");
}

#[test]
fn a_styled_group_member_is_charged_after_its_base_released() {
    // A styled group base (300) and a styled group member (300), rendered
    // one after the other: the peak is the max, not the sum.
    let (doc, _) = lone_300();
    let doc = with_raster(&doc, opaque(20, 15), (12, 9), BlendMode::Normal, 1.0);
    let (mut doc, m) = group(&doc, &[3], "Member");
    doc.layers[m].clipped = true;
    let doc = styled(&doc, m, OVERLAY);
    assert_budget_refuses(&doc, 299, "member 299");
    assert_budget_admits(&doc, 300, 300, "member 300");
}

#[test]
fn a_member_under_an_empty_base_group_is_never_charged() {
    // A visible isolated base group whose only child is hidden (no extent)
    // and a 20×15 drop-shadow styled group member: the reference returns
    // before resolving the member, so 299 admits everywhere and the unit
    // renders nothing.
    let mut doc = with_raster(&base(), opaque(9, 9), (3, 3), BlendMode::Normal, 1.0);
    doc.layers[1].visible = false;
    let (doc, empty) = group(&doc, &[1], "Empty");
    let doc = with_raster(&doc, opaque(20, 15), (10, 8), BlendMode::Normal, 1.0);
    let (mut doc, m) = group(&doc, &[3], "Member");
    doc.layers[m].clipped = true;
    let doc = styled(
        &doc,
        m,
        &fx(&["{\"type\":\"drop_shadow\",\"distance\":0,\"size\":0}"]),
    );
    assert_eq!(empty, 2);
    let p = plan_budget(&doc, (0, 0, W, H), 299).unwrap();
    assert_eq!(p["layers"].as_array().unwrap().len(), 4);
    assert_budget_admits(&doc, 299, 299, "empty base 299");
    assert_eq!(full(&doc), full(&base()), "the unit renders nothing");
    // Standalone (unclipped), the same member costs its 300.
    let mut lone = doc.clone();
    lone.layers[m].clipped = false;
    assert_budget_refuses(&lone, 299, "unclipped member 299");
}

#[test]
fn only_materialized_nests_are_charged_regionally() {
    // The stated difference: an UNSTYLED Multiply outer group 30×20 — its
    // extent is the styled group's grown by the overlay's pad of 1,
    // (9, 7)–(31, 24), and a 30×20 raster at (9, 7) — over the 300-pixel
    // styled group costs the reference 600 + 300 = 900, so
    // 899 refuses the reference; the regional path allocates no outer
    // buffer, charges only the materialized 300, and renders the reference
    // image (taken at the production cap).
    let (doc, g) = lone_300();
    let doc = with_raster(&doc, top_pixels(30, 20), (9, 7), BlendMode::Normal, 0.5);
    let (mut doc, o) = group(&doc, &[g, 3], "Outer");
    doc.layers[o].blend = BlendMode::Multiply;
    assert_eq!(
        doc.flattened_with_group_budget(899).unwrap_err(),
        budget_sentence(899)
    );
    doc.flattened_with_group_budget(900).unwrap();
    assert_budget_admits(
        &doc,
        899,
        doctored_core::doc_group::MAX_GROUP_BUFFER_PIXELS,
        "899",
    );
    assert_budget_refuses_regionally(&doc, 299);
}

fn assert_budget_refuses_regionally(doc: &DrDocument, group_pixels: u64) {
    let sentence = budget_sentence(group_pixels);
    assert_eq!(
        plan_budget(doc, (2, 1, 9, 6), group_pixels).unwrap_err(),
        sentence
    );
    assert_eq!(
        region_budget(doc, (2, 1, 9, 6), 1, group_pixels).unwrap_err(),
        sentence
    );
}

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_styled_group_render_returns_no_output() {
    let (doc, _) = styled_group(GROUP_STYLE, BlendMode::Normal, 0.6);
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

/// 13A-3b-i's probe: runs `render(control)` on a worker, polls until the
/// progress exceeds `loop_start` (the fraction done before the sample loop —
/// the materialization and the planes are the reference's own preparation)
/// and returns (time from the loop's start to the return, uncancelled) or,
/// cancelling there, the cancel-to-return latency.
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

/// The 8-bit concurrent cancel: with `bench` the measured
/// wall-clock record (`make cancel-bench`); without it the
/// load-independent default, each stage cancelled with the worker PARKED
/// past its opening report (`cancelled_past_each`, the count from a fresh
/// twin), no poller.
fn cancel_styled_group_render(bench: bool) {
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
            BlendMode::Normal,
            1.0,
        );
        let (mut doc, g) = group(&doc, &[1], "Group");
        doc.layers[g].blend = BlendMode::SoftLight;
        doc.layers[g].opacity = 0.9;
        styled(&doc, g, CANCEL_STYLE)
    };
    let doc = build();
    let hd = handle(&doc);
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
            "13A-3b-ii 8-bit cancel: {:.3} ms cancel-to-return; {:.2} ns per root sample \
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
            "8-bit cancel past the loop's start: {}",
            common::probe::parked_line(&parked[0])
        );
    }
    unsafe { dr_doc_free(hd) };
}

#[test]
fn a_concurrently_cancelled_styled_group_render_returns_operation_cancelled() {
    cancel_styled_group_render(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_styled_group_render_is_measured() {
    cancel_styled_group_render(true);
}

/// The u16 concurrent cancel: with `bench` the measured
/// wall-clock record (`make cancel-bench`); without it the
/// load-independent default, each stage cancelled with the worker PARKED
/// past its opening report (`cancelled_past_each`, the count from a fresh
/// twin), no poller.
fn cancel_styled_group_render_at_u16(bench: bool) {
    let (w, h) = (3000u32, 2000u32);
    let build = || {
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
        let style = LayerStyle::from_json(CANCEL_STYLE).unwrap();
        let child = PrecisionDocumentLayer::raster(
            "Top",
            image(w - 40, h - 40, 3, true),
            (20, 20),
            1.0,
            true,
        )
        .unwrap();
        let styled_group = PrecisionDocumentLayer::isolated_normal("Group", vec![child], 0.9, true)
            .unwrap()
            .with_blend_mode(BlendMode::SoftLight)
            .unwrap()
            .with_layer_style(Some(&style), (w, h))
            .unwrap();
        let layers = vec![
            PrecisionDocumentLayer::raster("Base", image(w, h, 1, false), (0, 0), 1.0, true)
                .unwrap(),
            styled_group,
        ];
        PrecisionDocument::from_parts(
            w,
            h,
            profile,
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap()
    };
    let d = build();
    // The loop begins once the fill, the materialization and the planes are
    // advanced: (work − pixels·rows) / work.
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
            "13A-3b-ii u16 cancel: {:.3} ms cancel-to-return; {:.2} ns per root sample \
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
            "u16 cancel past the loop's start: {}",
            common::probe::parked_line(&parked[0])
        );
    }
}

#[test]
fn a_concurrently_cancelled_styled_group_render_returns_operation_cancelled_at_u16() {
    cancel_styled_group_render_at_u16(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_styled_group_render_is_measured_at_u16() {
    cancel_styled_group_render_at_u16(true);
}
