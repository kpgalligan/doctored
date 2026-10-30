//! Phase 13A-3c: the 8-bit regional document render over KNOCKOUT (Shallow /
//! Deep) and "Blend Interior Effects as Group" — D5's Background at the root
//! and every one of its disqualifiers, Blend If, Dissolve, masked and
//! off-canvas knockout layers, the package, D4's pass-through (plain, gated,
//! masked, nested), isolated and materialized hard stops, knockout members
//! under every kind of base, a knockout clip base, D6 on and off; the budget
//! (a Shallow snapshot inside a materialized nest, and the pinned D5 /
//! unmaterialized pass-through DIVERGENCE the regional path states); the
//! concurrent cancels at BOTH depths (the u16 twin lives here, as the earlier
//! sub-slices' do, so one command measures both). The oracle is INDEPENDENT
//! of the regional code: the full projection cropped and top-left-cell
//! sampled here in plain index arithmetic, byte for byte.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_group::MAX_GROUP_BUFFER_PIXELS;
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
use serde_json::Value;

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

/// The 13A-2 windows — edges, corners, interior, 1×1, the whole canvas — of
/// a `cw`×`ch` canvas, plus `extra`, at divisors {1, 2, 3, 7, MAX}.
fn windows_of(cw: u32, ch: u32, extra: &[(u32, u32, u32, u32)]) -> Vec<(u32, u32, u32, u32)> {
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
    windows
}

const DIVISORS: [u32; 5] = [1, 2, 3, 7, u32::MAX];

fn assert_regions_match_with(doc: &DrDocument, what: &str, extra: &[(u32, u32, u32, u32)]) {
    let reference = full(doc);
    for window in windows_of(W, H, extra) {
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
    let (cw, ch) = (doc.width, doc.height);
    let reference = doc
        .flattened_with_group_budget(reference_pixels)
        .unwrap_or_else(|e| panic!("{what}: reference: {e}"))
        .into_raw();
    plan_budget(doc, (0, 0, cw, ch), group_pixels).unwrap_or_else(|e| panic!("{what}: plan: {e}"));
    for window in windows_of(cw, ch, &[]) {
        for d in [1, 3] {
            let got = region_budget(doc, window, d, group_pixels)
                .unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, cw, window, d),
                "{what}: {window:?}/{d}"
            );
        }
    }
}

// ------------------------------------------------------------- fixtures --

/// A canvas-sized raster at (0, 0), Normal, opacity 1, unmasked, unstyled —
/// entry 0 of every fixture below, so it QUALIFIES as D5's Background. Its
/// top rows are translucent, so a hole to it shows its alpha.
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

fn fx(effects: &[&str]) -> String {
    format!("{{\"effects\":[{}]}}", effects.join(","))
}

/// The knockout styles every lone case runs: Shallow and Deep at fill 0.5
/// and at fill 0 (the HOLE), and Deep with the pixels at full fill.
const KNOCKOUTS: [(&str, &str); 5] = [
    (
        "shallow 0.5",
        "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}",
    ),
    (
        "shallow hole",
        "{\"fill_opacity\":0,\"knockout\":\"shallow\"}",
    ),
    ("deep 0.5", "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}"),
    ("deep hole", "{\"fill_opacity\":0,\"knockout\":\"deep\"}"),
    ("deep full", "{\"knockout\":\"deep\"}"),
];

const TOP_AT: (i32, i32) = (6, 5);

/// Background (0), a translucent Screen 0.8 raster across the middle (1),
/// then a shaped raster at `TOP_AT` (2) with `json`, in `mode`/`opacity`.
fn over_background(json: &str, mode: BlendMode, opacity: f32) -> DrDocument {
    let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    let doc = with_raster(&doc, shape_pixels(17, 12), TOP_AT, mode, opacity);
    styled(&doc, 2, json)
}

fn at(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

// ----------------------------------------------------------------- D5 --

#[test]
fn a_lone_knockout_over_the_background_matches() {
    for (label, json) in KNOCKOUTS {
        for opacity in [1.0, 0.7] {
            let doc = over_background(json, BlendMode::Normal, opacity);
            let what = format!("{label} at {opacity}");
            assert_regions_match_with(&doc, &what, &around(TOP_AT, 17, 12, 1));
        }
    }
    // The hole at the shape's opaque centre is the BACKGROUND's own pixel
    // (Shallow at the root is Deep), never the Screen layer between.
    let hole = over_background(KNOCKOUTS[1].1, BlendMode::Normal, 1.0);
    let (cx, cy) = (14, 11);
    assert_eq!(at(&full(&hole), cx, cy), at(&full(&base()), cx, cy));
}

/// Each clause of `background_entry` broken on entry 0.
fn disqualified() -> Vec<(&'static str, DrDocument)> {
    let deep = KNOCKOUTS[3].1;
    let doc = over_background(deep, BlendMode::Normal, 1.0);
    let mut out = Vec::new();
    let mut offset = doc.clone();
    offset.layers[0].offset = (1, 0);
    out.push(("offset", offset));
    let mut opacity = doc.clone();
    opacity.layers[0].opacity = 0.9;
    out.push(("opacity 0.9", opacity));
    out.push((
        "enabled mask",
        masked(
            &doc,
            0,
            GrayImage::from_pixel(W, H, Luma([255])),
            (0, 0),
            1.0,
            0.0,
        ),
    ));
    let mut clipped = doc.clone();
    clipped.layers[1].clipped = true;
    out.push(("entry 1 clipped", clipped));
    let mut hidden = doc.clone();
    hidden.layers[0].visible = false;
    out.push(("hidden", hidden));
    let mut multiply = doc.clone();
    multiply.layers[0].blend = BlendMode::Multiply;
    out.push(("multiply", multiply));
    out.push(("a style", styled(&doc, 0, "{\"fill_opacity\":0.9}")));
    out.push((
        "a Blend If",
        styled(&doc, 0, &format!("{{{BLEND_IF_GRAY}}}")),
    ));
    out
}

#[test]
fn each_background_disqualifier_punches_to_transparency() {
    let (cx, cy) = (14, 11);
    let qualified = over_background(KNOCKOUTS[3].1, BlendMode::Normal, 1.0);
    assert_eq!(
        at(&full(&qualified), cx, cy)[3],
        255,
        "the Background shows"
    );
    for (label, doc) in disqualified() {
        assert_eq!(
            at(&full(&doc), cx, cy)[3],
            0,
            "{label}: the hole is transparent"
        );
        assert_regions_match_with(&doc, label, &around(TOP_AT, 17, 12, 0));
        // Shallow at the root rides the same (absent) backdrop.
        let mut shallow = doc.clone();
        let top = shallow.layers.len() - 1;
        shallow = styled(&shallow, top, KNOCKOUTS[0].1);
        assert_regions_match(&shallow, &format!("{label}, shallow"));
    }
}

#[test]
fn blend_if_beside_a_knockout_matches() {
    for knockout in ["shallow", "deep"] {
        let json = format!("{{\"fill_opacity\":0.6,\"knockout\":\"{knockout}\",{BLEND_IF_GRAY}}}");
        for (mode, opacity) in [(BlendMode::Normal, 1.0), (BlendMode::Multiply, 0.8)] {
            let doc = over_background(&json, mode, opacity);
            assert_regions_match_with(
                &doc,
                &format!("blend if {knockout} {mode:?} {opacity}"),
                &around(TOP_AT, 17, 12, 0),
            );
        }
        // With a Below effect the Blend If still reads the sample beneath.
        let json = format!(
            "{{\"knockout\":\"{knockout}\",{BLEND_IF_GRAY},\"effects\":[\
             {{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}}]}}"
        );
        let doc = over_background(&json, BlendMode::Normal, 1.0);
        assert_regions_match(&doc, &format!("blend if + shadow {knockout}"));
    }
}

#[test]
fn a_dissolve_knockout_matches() {
    for (label, json) in [
        KNOCKOUTS[0],
        KNOCKOUTS[3],
        (
            "deep + effects",
            "{\"fill_opacity\":0.5,\"knockout\":\"deep\",\"effects\":[\
             {\"type\":\"drop_shadow\",\"distance\":3,\"size\":2},{\"type\":\"color_overlay\"}]}",
        ),
    ] {
        let doc = over_background(json, BlendMode::Dissolve, 0.6);
        assert_regions_match_with(
            &doc,
            &format!("dissolve {label}"),
            &around(TOP_AT, 17, 12, 2),
        );
    }
}

#[test]
fn masked_and_off_canvas_knockout_layers_match() {
    for feather in [0.0, 6.0] {
        for (label, json) in [KNOCKOUTS[0], KNOCKOUTS[3]] {
            let doc = over_background(json, BlendMode::Normal, 1.0);
            let doc = masked(&doc, 2, gradient(14, 10), (8, 6), 0.8, feather);
            assert_regions_match_with(
                &doc,
                &format!("masked {label} feather {feather}"),
                &around((8, 6), 14, 10, 0),
            );
        }
    }
    // Partly off the canvas at both corners.
    for offset in [(-6, -4), (30, 20)] {
        let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
        let doc = with_raster(&doc, shape_pixels(17, 12), offset, BlendMode::Normal, 1.0);
        for (label, json) in [KNOCKOUTS[1], KNOCKOUTS[2]] {
            let doc = styled(&doc, 2, json);
            assert_regions_match_with(
                &doc,
                &format!("off canvas {offset:?} {label}"),
                &around(offset, 17, 12, 0),
            );
        }
    }
}

#[test]
fn the_package_with_a_knockout_matches() {
    for knockout in ["shallow", "deep"] {
        let json = format!(
            "{{\"fill_opacity\":0.5,\"knockout\":\"{knockout}\",\"effects\":[\
             {{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}},\
             {{\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#3080c0\"}},\
             {{\"type\":\"inner_shadow\",\"distance\":2,\"size\":2}}]}}"
        );
        for mode in [BlendMode::Normal, BlendMode::Multiply] {
            let doc = over_background(&json, mode, 0.6);
            assert_regions_match_with(
                &doc,
                &format!("package {knockout} {mode:?}"),
                &around(TOP_AT, 17, 12, 5),
            );
        }
    }
}

// ----------------------------------------------------------------- D4 --

/// Background (0), a Screen raster (1) and a pass-through group (4) over a
/// Multiply raster (2) and a shaped knockout child (3) carrying `json`.
fn pass_through(json: &str) -> (DrDocument, usize) {
    let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    let doc = with_raster(&doc, top_pixels(20, 16), (10, 2), BlendMode::Multiply, 1.0);
    let doc = with_raster(&doc, shape_pixels(17, 12), TOP_AT, BlendMode::Normal, 1.0);
    let doc = styled(&doc, 3, json);
    group(&doc, &[2, 3], "Pass")
}

#[test]
fn a_shallow_or_deep_child_in_plain_gated_and_masked_pass_through_groups_matches() {
    for (label, json) in KNOCKOUTS {
        let (doc, g) = pass_through(json);
        assert_regions_match_with(&doc, &format!("plain {label}"), &around(TOP_AT, 17, 12, 0));
        let mut gated = doc.clone();
        gated.layers[g].opacity = 0.5;
        assert_regions_match(&gated, &format!("gated 0.5 {label}"));
        let masked_group = masked(&doc, g, gradient(22, 15), (5, 4), 1.0, 0.0);
        assert_regions_match(&masked_group, &format!("masked group {label}"));
    }
    // The Shallow hole inside a plain pass-through group shows the sample as
    // the group's children began (Background + Screen), not the Background.
    let (doc, _) = pass_through(KNOCKOUTS[1].1);
    let (cx, cy) = (14, 11);
    let before = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    assert_eq!(at(&full(&doc), cx, cy), at(&full(&before), cx, cy));
}

#[test]
fn nested_pass_through_groups_snapshot_their_own_level_and_deep_rides_to_the_background() {
    // Outer pass-through (6) over a Multiply raster (2) and an inner
    // pass-through (5) holding a Screen raster (3) and the knockout (4).
    for (label, json) in KNOCKOUTS {
        let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
        let doc = with_raster(&doc, top_pixels(20, 16), (10, 2), BlendMode::Multiply, 1.0);
        let doc = with_raster(&doc, top_pixels(12, 12), (3, 6), BlendMode::Screen, 0.7);
        let doc = with_raster(&doc, shape_pixels(17, 12), TOP_AT, BlendMode::Normal, 1.0);
        let doc = styled(&doc, 4, json);
        let (doc, inner) = group(&doc, &[3, 4], "Inner");
        let (doc, outer) = group(&doc, &[2, inner], "Outer");
        assert_eq!((inner, outer), (5, 6));
        assert_regions_match(&doc, &format!("nested {label}"));
        let mut gated = doc.clone();
        gated.layers[inner].opacity = 0.6;
        assert_regions_match(&gated, &format!("nested, inner gated {label}"));
    }
}

#[test]
fn a_pass_through_group_without_a_shallow_child_under_a_level_with_one_matches() {
    // The outer pass-through level holds a Shallow child (2); the inner
    // pass-through group (5) holds none — only a Deep child (4) — so its
    // Shallow plane is never read.
    let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    let doc = with_raster(&doc, shape_pixels(17, 12), TOP_AT, BlendMode::Normal, 1.0);
    let doc = styled(&doc, 2, KNOCKOUTS[0].1);
    let doc = with_raster(&doc, top_pixels(12, 12), (20, 6), BlendMode::Multiply, 1.0);
    let doc = with_raster(&doc, shape_pixels(15, 11), (18, 12), BlendMode::Normal, 1.0);
    let doc = styled(&doc, 4, KNOCKOUTS[2].1);
    let (doc, inner) = group(&doc, &[3, 4], "Inner");
    let (doc, _) = group(&doc, &[2, inner], "Outer");
    assert_regions_match(&doc, "no Shallow child in the inner level");
}

#[test]
fn knockouts_inside_isolated_and_materialized_groups_stop_at_the_buffer() {
    for (label, json) in KNOCKOUTS {
        let (doc, g) = pass_through(json);
        let mut isolated = doc.clone();
        isolated.layers[g].blend = BlendMode::Multiply;
        assert_regions_match(&isolated, &format!("isolated Multiply {label}"));
        let mut normal = doc.clone();
        normal.layers[g].blend = BlendMode::Normal;
        normal.layers[g].opacity = 0.8;
        assert_regions_match(&normal, &format!("isolated Normal 0.8 {label}"));
        let materialized = styled(
            &doc,
            g,
            &fx(&["{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}"]),
        );
        assert_regions_match(&materialized, &format!("materialized {label}"));
    }
    // The Deep hole inside an isolated group is transparent in the buffer,
    // so the group shows what lies beneath it (Background + Screen).
    let (mut doc, g) = pass_through(KNOCKOUTS[3].1);
    doc.layers[g].blend = BlendMode::Normal;
    let (cx, cy) = (14, 11);
    let beneath = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    assert_eq!(at(&full(&doc), cx, cy), at(&full(&beneath), cx, cy));
}

// ---------------------------------------------------------- clip members --

/// Background (0), a Screen raster (1), a shaped clip base (2) and a
/// clipped member (3) carrying `json`.
fn clip_unit(json: &str) -> DrDocument {
    let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    let doc = with_raster(&doc, shape_pixels(19, 13), (4, 4), BlendMode::Normal, 0.9);
    let mut doc = with_raster(&doc, shape_pixels(17, 12), (12, 9), BlendMode::Normal, 1.0);
    doc.layers[3].clipped = true;
    styled(&doc, 3, json)
}

#[test]
fn a_knockout_member_punches_to_the_base_only_sample_under_every_base() {
    for (label, json) in KNOCKOUTS {
        let doc = clip_unit(json);
        assert_regions_match_with(
            &doc,
            &format!("unstyled base {label}"),
            &around((12, 9), 17, 12, 0),
        );
        for (base_label, base_style) in [
            ("fill", "{\"fill_opacity\":0.6}".to_string()),
            ("blend if", format!("{{{BLEND_IF_GRAY}}}")),
            (
                "effects",
                fx(&[
                    "{\"type\":\"stroke\",\"size\":2,\"color\":\"#ffffff\"}",
                    "{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}",
                ]),
            ),
        ] {
            let styled_base = styled(&doc, 2, &base_style);
            assert_regions_match(&styled_base, &format!("{base_label} base {label}"));
            let mut package = styled_base.clone();
            package.layers[2].opacity = 0.6;
            assert_regions_match(&package, &format!("{base_label} base 0.6 {label}"));
        }
        let masked_base = masked(&doc, 2, gradient(16, 12), (5, 5), 0.9, 6.0);
        assert_regions_match(&masked_base, &format!("masked base {label}"));
    }
}

#[test]
fn a_second_knockout_member_ignores_what_the_first_member_drew() {
    // Base (2), a Multiply member (3) that alters the buffer, then a Deep
    // hole member (4): the hole shows the BASE alone.
    let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    let doc = with_raster(&doc, shape_pixels(19, 13), (4, 4), BlendMode::Normal, 1.0);
    let mut doc = with_raster(&doc, top_pixels(20, 14), (8, 6), BlendMode::Multiply, 1.0);
    doc.layers[3].clipped = true;
    let mut doc = with_raster(&doc, shape_pixels(17, 12), (10, 5), BlendMode::Normal, 1.0);
    doc.layers[4].clipped = true;
    for (label, json) in KNOCKOUTS {
        let doc = styled(&doc, 4, json);
        assert_regions_match(&doc, &format!("second member {label}"));
    }
}

#[test]
fn a_knockout_group_member_and_an_adjustment_base_match() {
    // A group member (always isolated) carrying a knockout, effect-less and
    // with effects (materialized).
    let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    let doc = with_raster(&doc, shape_pixels(19, 13), (4, 4), BlendMode::Normal, 1.0);
    let doc = with_raster(&doc, shape_pixels(17, 12), (12, 9), BlendMode::Normal, 1.0);
    let doc = with_raster(&doc, top_pixels(9, 9), (14, 6), BlendMode::Screen, 0.8);
    let (mut doc, m) = group(&doc, &[3, 4], "Member");
    doc.layers[m].clipped = true;
    for (label, json) in KNOCKOUTS {
        assert_regions_match(&styled(&doc, m, json), &format!("group member {label}"));
    }
    let effects = "{\"fill_opacity\":0.5,\"knockout\":\"deep\",\"effects\":[\
                   {\"type\":\"stroke\",\"size\":2},{\"type\":\"color_overlay\",\"opacity\":0.5}]}";
    assert_regions_match(&styled(&doc, m, effects), "materialized group member");
    // An ADJUSTMENT base contributes nothing with a non-empty run, knockout
    // member or not.
    let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    let doc = with_adjustment(&doc, "hue_saturation", "{\"hue\":-60,\"saturation\":0.2}");
    let mut doc = with_raster(&doc, shape_pixels(17, 12), (12, 9), BlendMode::Normal, 1.0);
    doc.layers[3].clipped = true;
    for (label, json) in KNOCKOUTS {
        assert_regions_match(&styled(&doc, 3, json), &format!("adjustment base {label}"));
    }
}

#[test]
fn a_knockout_clip_base_with_visible_and_clamp_only_members_matches() {
    for (label, json) in KNOCKOUTS {
        let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
        let doc = with_raster(&doc, shape_pixels(17, 12), TOP_AT, BlendMode::Normal, 1.0);
        let doc = styled(&doc, 2, json);
        let mut doc = with_raster(&doc, top_pixels(15, 15), (14, 8), BlendMode::Multiply, 0.6);
        doc.layers[3].clipped = true;
        let mut doc = with_raster(&doc, top_pixels(9, 9), (2, 2), BlendMode::Normal, 0.0);
        doc.layers[4].clipped = true;
        assert_regions_match_with(
            &doc,
            &format!("knockout base {label}"),
            &around((14, 8), 15, 15, 1),
        );
        let mut clamp_only = doc.clone();
        clamp_only.layers[3].visible = false;
        assert_regions_match(&clamp_only, &format!("knockout base, clamp-only {label}"));
        // With effects, the package.
        let mut package = styled(
            &doc,
            2,
            &json.replace('}', ",\"effects\":[{\"type\":\"stroke\",\"size\":2}]}"),
        );
        package.layers[2].opacity = 0.7;
        assert_regions_match(&package, &format!("knockout base package {label}"));
    }
}

// ----------------------------------------------------------------- D6 --

const GROUPED: &str = "\"blend_interior_as_group\":true,\"fill_opacity\":0.5,\"effects\":[\
     {\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#3080c0\",\"opacity\":0.8},\
     {\"type\":\"gradient_overlay\",\"opacity\":0.6},\
     {\"type\":\"satin\",\"distance\":4,\"size\":3,\"color\":\"#602000\"},\
     {\"type\":\"inner_glow\",\"size\":3},\
     {\"type\":\"inner_shadow\",\"distance\":2,\"size\":2},\
     {\"type\":\"stroke\",\"size\":2,\"position\":\"inside\",\"color\":\"#20ff00\"},\
     {\"type\":\"bevel_emboss\",\"size\":3,\"depth\":2}]";

#[test]
fn grouped_interiors_on_a_lone_raster_match() {
    for (mode, opacity) in [
        (BlendMode::Multiply, 1.0),
        (BlendMode::Normal, 1.0),
        (BlendMode::Multiply, 0.6),
    ] {
        let doc = over_background(&format!("{{{GROUPED}}}"), mode, opacity);
        assert_regions_match_with(
            &doc,
            &format!("D6 {mode:?} {opacity}"),
            &around(TOP_AT, 17, 12, 3),
        );
        // D6 beside a knockout.
        let doc = over_background(
            &format!("{{\"knockout\":\"deep\",{GROUPED}}}"),
            mode,
            opacity,
        );
        assert_regions_match(&doc, &format!("D6 + deep {mode:?} {opacity}"));
    }
}

#[test]
fn grouped_interiors_on_bases_members_and_groups_match() {
    let doc = with_raster(&base(), top_pixels(30, 14), (4, 9), BlendMode::Screen, 0.8);
    let doc = with_raster(&doc, shape_pixels(17, 12), TOP_AT, BlendMode::Multiply, 1.0);
    let doc = styled(&doc, 2, &format!("{{{GROUPED}}}"));
    let mut doc = with_raster(&doc, top_pixels(15, 15), (14, 8), BlendMode::Multiply, 0.6);
    doc.layers[3].clipped = true;
    // A base WITH a visible member: D6 evaluates ON (P7-39 lifted the narrowing).
    assert_regions_match(&doc, "D6 base with a visible member");
    // Hidden-only members: ON.
    let mut hidden = doc.clone();
    hidden.layers[3].visible = false;
    assert_regions_match(&hidden, "D6 base with hidden-only members");
    // On a MEMBER: ON (`member_env` resets the flag).
    let member = styled(&doc, 3, &format!("{{{GROUPED}}}"));
    assert_regions_match(&member, "D6 on a member");
    // On a MATERIALIZED group.
    let doc = with_raster(
        &base(),
        shape_pixels(17, 12),
        TOP_AT,
        BlendMode::Normal,
        1.0,
    );
    let doc = with_raster(&doc, top_pixels(13, 10), (-4, 15), BlendMode::Screen, 0.8);
    let (mut doc, g) = group(&doc, &[1, 2], "Group");
    doc.layers[g].blend = BlendMode::Multiply;
    assert_regions_match(&styled(&doc, g, &format!("{{{GROUPED}}}")), "D6 group");
    assert_regions_match(
        &styled(&doc, g, &format!("{{\"knockout\":\"shallow\",{GROUPED}}}")),
        "D6 + shallow group",
    );
}

// --------------------------------------------------------------- policy --

#[test]
fn a_knockout_document_refuses_linear_light() {
    let doc = over_background(KNOCKOUTS[0].1, BlendMode::Normal, 1.0);
    assert!(doc.with_evaluation(Evaluation::LinearLight).is_err());
    let doc = over_background(&format!("{{{GROUPED}}}"), BlendMode::Normal, 1.0);
    assert!(doc.with_evaluation(Evaluation::LinearLight).is_err());
}

#[test]
fn a_knockout_plan_keeps_its_row_shape() {
    let doc = over_background(KNOCKOUTS[0].1, BlendMode::Normal, 1.0);
    let p = plan(&doc, (4, 3, 9, 7), 1).unwrap();
    assert_eq!(p["contract_version"], 1);
    let row = &p["layers"][2];
    assert_eq!(row["style"], "fill");
    assert_eq!(row["neighborhood"]["kind"], "pointwise");
    assert_eq!(p["global_analysis"], false);
    let mut keys: Vec<&String> = row.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "blend_if",
            "clip_base",
            "global_analysis",
            "group",
            "index",
            "kind",
            "mask",
            "name",
            "neighborhood",
            "operator",
            "style"
        ]
    );
    // No knockout term: the accumulator and the output only.
    let pixels = 9u64 * 7;
    assert_eq!(p["scratch_heap_bytes"], pixels * 16 + pixels * 4);
}

// --------------------------------------------------------------- budget --

#[test]
fn a_visible_shallow_child_in_a_materialized_nests_pass_through_subgroup_is_charged() {
    // A plain pass-through subgroup holding a VISIBLE opacity-1 Shallow
    // child snapshots the 300-pixel buffer (`level_wants_shallow`): peak
    // 600. The background is below opacity 1, so D5's root snapshot is off.
    let mut doc = with_raster(&base(), top_pixels(20, 15), (10, 8), BlendMode::Normal, 1.0);
    doc.layers[0].opacity = 0.95;
    let doc = with_raster(&doc, opaque(6, 6), (12, 9), BlendMode::Normal, 1.0);
    let doc = styled(&doc, 2, "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}");
    let (doc, p) = group(&doc, &[1, 2], "Pass");
    let (doc, g) = group(&doc, &[p], "Styled");
    let doc = styled(
        &doc,
        g,
        "{\"effects\":[{\"type\":\"color_overlay\",\"color\":\"#30c080\"}]}",
    );
    assert_budget_refuses(&doc, 599, "shallow snapshot 599");
    assert_budget_admits(&doc, 600, 600, "shallow snapshot 600");
}

/// The pinned DIVERGENCE (`regional`'s stated differences): a 100×100
/// canvas whose entry 0 is an opaque canvas-sized Background, entry 1
/// `second`, then an isolated group with a drop shadow over a 100×60 child
/// (a 6000-pixel materialization). The reference holds D5's 10000-pixel
/// Background snapshot while the group renders; the regional path allocates
/// no snapshot.
fn divergence(second: impl FnOnce(DrDocument) -> DrDocument) -> DrDocument {
    let doc = DrDocument::from_pixels(RgbaImage::from_fn(100, 100, |x, y| {
        Rgba([(x * 2) as u8, (y * 2) as u8, ((x + y) % 256) as u8, 255])
    }));
    let doc = second(doc);
    let top = doc.layers.len() - 1;
    let mut doc = doc
        .adding_image_layer(
            top,
            RgbaImage::from_fn(100, 60, |x, y| {
                Rgba([(x * 3) as u8, 90, (y * 4) as u8, 200])
            }),
            "Child",
        )
        .expect("add layer");
    let child = doc.layers.len() - 1;
    doc.layers[child].offset = (0, 20);
    let (doc, g, ..) = doc.group_layers(&[child], "Shadowed").expect("group");
    styled(
        &doc,
        g,
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}",
    )
}

fn deep_raster(doc: DrDocument) -> DrDocument {
    let mut out = doc
        .adding_image_layer(0, shape_pixels(30, 30), "Deep")
        .expect("add layer");
    out.layers[1].offset = (10, 10);
    styled(&out, 1, "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}")
}

fn shallow_in_pass_through(doc: DrDocument) -> DrDocument {
    let doc = deep_raster(doc);
    let doc = styled(&doc, 1, "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}");
    let (doc, ..) = doc.group_layers(&[1], "Pass").expect("group");
    doc
}

#[test]
fn the_background_and_pass_through_snapshot_charges_are_a_stated_difference() {
    for (label, doc) in [
        ("deep raster", divergence(deep_raster)),
        ("pass-through shallow", divergence(shallow_in_pass_through)),
    ] {
        assert_eq!(
            doc.flattened_with_group_budget(12000).unwrap_err(),
            budget_sentence(12000),
            "{label}: the reference refuses"
        );
        assert_budget_admits(&doc, 12000, MAX_GROUP_BUFFER_PIXELS, label);
    }
}

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_knockout_render_returns_no_output() {
    let doc = over_background(KNOCKOUTS[0].1, BlendMode::Normal, 0.7);
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

/// 13A-3b-i's probe: runs `render(control)` on a worker, polls until the
/// progress exceeds `loop_start` (the fraction done before the sample loop)
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
fn cancel_knockout_render(bench: bool) {
    // A Shallow lone raster over a Background: every root sample runs the
    // D5 split and the unit pass.
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
            0.9,
        );
        styled(&doc, 1, "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}")
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
            "13A-3c 8-bit cancel: {:.3} ms cancel-to-return; {:.2} ns per root sample \
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
fn a_concurrently_cancelled_knockout_render_returns_operation_cancelled() {
    cancel_knockout_render(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_knockout_render_is_measured() {
    cancel_knockout_render(true);
}

/// The u16 concurrent cancel: with `bench` the measured
/// wall-clock record (`make cancel-bench`); without it the
/// load-independent default, each stage cancelled with the worker PARKED
/// past its opening report (`cancelled_past_each`, the count from a fresh
/// twin), no poller.
fn cancel_knockout_render_at_u16(bench: bool) {
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
        let style =
            LayerStyle::from_json("{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}").unwrap();
        let top = PrecisionDocumentLayer::raster(
            "Top",
            image(w - 40, h - 40, 3, true),
            (20, 20),
            0.9,
            true,
        )
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
            profile,
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap()
    };
    let d = build();
    // The loop begins once the fill is advanced: (work − pixels·rows) / work.
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
            "13A-3c u16 cancel: {:.3} ms cancel-to-return; {:.2} ns per root sample \
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
fn a_concurrently_cancelled_knockout_render_returns_operation_cancelled_at_u16() {
    cancel_knockout_render_at_u16(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_knockout_render_is_measured_at_u16() {
    cancel_knockout_render_at_u16(true);
}
