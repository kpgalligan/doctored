//! Phase 13A-4b-ii-b-2: the 8-bit regional document render over a
//! guide-reading Shadows/Highlights as a CLIP MEMBER of a STYLED base — a
//! fill-only style (with and without Blend If), effects at opacity 1, effects
//! below opacity 1 in Dissolve, a knockout base without and with planes
//! (routed), a Deep knockout base under D5's Background, a materialized group
//! base — at the root, inside isolated levels and plain and gated
//! pass-through groups, beside raster and knockout members, staged through
//! the enclosing level with the unit RE-RUN from its held "began" sample, the
//! staged cursor's member index a STOP inside the style's own member loop;
//! and the PACKAGED base (effects below opacity 1, not Dissolve) refused by
//! name. The oracle is INDEPENDENT of the regional code: `dr_doc_flattened`'s
//! projection cropped and top-left-cell sampled here in plain index
//! arithmetic, byte for byte (tolerance: EXACT, declared before measuring).
//! The plan figures are restated from the guide's own sizing rule anchored at
//! the canvas: one held plane over `R_0` per distinct clip unit holding a
//! guided member (and per held pass-through group, and D5's Background
//! snapshot), every member stage's rows counted from its unit's FIRST row.
//! The two concurrent-cancel measurements (8 bits and u16) live here, as
//! `#[ignore]`d `…_is_measured` twins, so `make cancel-bench`
//! records both depths; their default twins cancel with the worker parked.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::{BlendMode, DrDocument, LayerRaster};
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
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::{adjust_meta, img_dims, img_pixels, plasma, styled, take_err_string};

use crate::region_plan;

type Window = (u32, u32, u32, u32);

/// Two effects that render planes: a drop shadow (Below) and a stroke.
const FX: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                  {\"type\":\"stroke\",\"size\":3}]}";

const FILL: &str = "{\"fill_opacity\":0.5}";

const FILL_BLEND_IF: &str = "{\"fill_opacity\":0.7,\"blend_if\":{\"channel\":\"gray\",\
                             \"this_layer\":[10,60,180,240],\"underlying\":[0,40,200,255]}}";

const SHALLOW: &str = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}";

const SHALLOW_FX: &str = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\",\"effects\":\
                          [{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                          {\"type\":\"stroke\",\"size\":3}]}";

const DEEP: &str = "{\"fill_opacity\":0.6,\"knockout\":\"deep\"}";

const PACKAGED_BASE: &str =
    "is Shadows/Highlights with a guide under a packaged styled base (8-bit effects below opacity 1)";

// ------------------------------------------------------------- plumbing --

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

fn full(doc: &DrDocument) -> Vec<u8> {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_flattened(h, 0, std::ptr::null(), &mut err) };
    unsafe { dr_doc_free(h) };
    assert!(!img.is_null(), "flattened: {}", take_err_string(err));
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    out
}

/// The independent oracle: pixel (x + i·d, y + j·d) of the full projection.
fn expected(full: &[u8], canvas_w: u32, (x, y, w, h): Window, d: u32) -> (u32, u32, Vec<u8>) {
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

fn region(doc: &DrDocument, (x, y, w, h): Window, d: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let hd = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, d, u64::MAX, ptr::null(), &mut err) };
    unsafe { dr_doc_free(hd) };
    if img.is_null() {
        return Err(take_err_string(err));
    }
    let (ow, oh) = img_dims(img);
    let px = img_pixels(img);
    unsafe { dr_image_free(img) };
    Ok((ow, oh, px))
}

fn plan(doc: &DrDocument, (x, y, w, h): Window, d: u32) -> Result<Value, String> {
    region_plan::byte(doc, (x, y, w, h), d, u64::MAX)
}

/// 13A-4b-i's windows (edges, corners, 1×1, the whole canvas, strict
/// sub-windows, windows straddling each corner of `rect`) plus windows wholly
/// outside `rect` (left of it and above it, where there is room).
fn windows(cw: u32, ch: u32, rect: Window) -> Vec<Window> {
    let mut out = vec![
        (0, 0, cw, ch),
        (3, 2, 11, 7),
        (0, 0, 5, 4),
        (cw - 6, 0, 6, 5),
        (0, ch - 3, 7, 3),
        (cw - 4, ch - 5, 4, 5),
        (cw - 1, ch - 1, 1, 1),
        (0, 0, 1, 1),
        (cw / 3 + 1, ch / 3 + 1, cw / 3 - 2, ch / 3 - 2),
        (cw / 2, ch / 2, 1, 1),
        (0, ch / 2 - 3, 9, 6),
    ];
    let (rx, ry, rw, rh) = rect;
    for (cx, cy) in [(rx, ry), (rx + rw, ry), (rx, ry + rh), (rx + rw, ry + rh)] {
        let x = cx.saturating_sub(4).min(cw - 1);
        let y = cy.saturating_sub(3).min(ch - 1);
        out.push((x, y, (cw - x).min(9), (ch - y).min(7)));
    }
    if rx >= 3 {
        out.push((0, ry, rx, rh.min(ch - ry)));
    }
    if ry >= 3 {
        out.push((rx, 0, rw.min(cw - rx), ry));
    }
    out
}

fn assert_regions_match(doc: &DrDocument, rect: Window, what: &str) {
    let reference = full(doc);
    let (cw, ch) = (doc.width, doc.height);
    for window in windows(cw, ch, rect) {
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

fn assert_refuses(doc: &DrDocument, index: usize, name: &str, construct: &str) {
    let window = (2, 1, 9, 6);
    let sentence = format!("regional render refuses layer {index} ({name}): {construct}");
    assert_eq!(plan(doc, window, 1).unwrap_err(), sentence);
    assert_eq!(region(doc, window, 1).unwrap_err(), sentence);
}

// ------------------------------------------------------------ fixtures --

/// A plasma backdrop with transparent holes and translucent pixels — a
/// canvas-sized plain raster at (0, 0), so a knockout above it takes it for
/// the Background (D5).
fn backdrop(cw: u32, ch: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(cw, ch, |x, y| {
        let c = plasma(x, y);
        let hole = (x / 7 + y / 5).is_multiple_of(9);
        let alpha = if hole {
            0
        } else if (x + 2 * y).is_multiple_of(5) || y < ch / 8 {
            (50 + (x * 7 + y * 3) % 170) as u8
        } else {
            255
        };
        Rgba([c[0], c[1], c[2], alpha])
    }))
}

/// A translucent raster with transparent HOLES — small ones on a lattice and
/// one large block (the guide's coverage-0 fallback).
fn holes(w: u32, h: u32, seed: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let block = x > w / 5 && x < w / 5 + w / 3 && y > h / 2 && y < h / 2 + h / 3;
        let hole = block || (x / 5 + y / 7 + seed).is_multiple_of(8);
        Rgba([
            (x * 37 + y * 11 + seed * 50) as u8,
            (200 - (x * 5 + seed) % 200) as u8,
            (y * 23 + 7) as u8,
            if hole {
                0
            } else {
                (60 + (x * 13 + y * 29 + seed) % 196) as u8
            },
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

fn sh_params(radius: f32) -> String {
    format!(
        "{{\"shadows\":{{\"amount\":0.7,\"tone\":0.9}},\"highlights\":{{\"amount\":0.35,\
         \"tone\":0.8}},\"radius\":{radius}}}"
    )
}

/// A guided Shadows/Highlights on top over ONE transparent pixel at `at`, so
/// it widens no enclosing group's extent past `at`; opacity 0.8.
fn with_guide(doc: &DrDocument, radius: f32, at: (i32, i32)) -> DrDocument {
    let mut out = doc
        .adding_image_layer(
            doc.layers.len() - 1,
            RgbaImage::new(1, 1),
            "shadows_highlights",
        )
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].meta = Some(adjust_meta("shadows_highlights", &sh_params(radius)));
    out.layers[top].offset = at;
    out.layers[top].opacity = 0.8;
    out
}

/// [`with_guide`] CLIPPED to the unit below it.
fn with_member_guide(doc: &DrDocument, radius: f32, at: (i32, i32)) -> DrDocument {
    let mut out = with_guide(doc, radius, at);
    let top = out.layers.len() - 1;
    out.layers[top].clipped = true;
    out
}

/// A raster clipped to the unit below it.
fn with_member(
    doc: &DrDocument,
    pixels: RgbaImage,
    offset: (i32, i32),
    blend: BlendMode,
    opacity: f32,
) -> DrDocument {
    let mut out = with_raster(doc, pixels, offset, blend, opacity);
    let top = out.layers.len() - 1;
    out.layers[top].clipped = true;
    out
}

/// `group_layers` over `idx` made ISOLATED (blend Normal), and its index.
fn isolate(doc: &DrDocument, idx: &[usize]) -> (DrDocument, usize) {
    let (mut out, g, ..) = doc.group_layers(idx, "Group").expect("group");
    out.layers[g].blend = BlendMode::Normal;
    (out, g)
}

/// `group_layers` over `idx`: a (default) Pass Through group, and its index.
fn pass(doc: &DrDocument, idx: &[usize]) -> (DrDocument, usize) {
    let (out, g, ..) = doc.group_layers(idx, "Pass").expect("group");
    assert_eq!(out.layers[g].blend, BlendMode::PassThrough);
    (out, g)
}

/// A raster's rect: at `offset` of `size`, within the canvas.
fn rect_of(offset: (i32, i32), size: (u32, u32), canvas: (u32, u32)) -> Window {
    let x0 = offset.0.max(0) as u32;
    let y0 = offset.1.max(0) as u32;
    let x1 = ((offset.0 + size.0 as i32).max(0) as u32).min(canvas.0);
    let y1 = ((offset.1 + size.1 as i32).max(0) as u32).min(canvas.1);
    (x0, y0, x1 - x0, y1 - y0)
}

/// A feathered elliptical mask over part of `rect` on layer `idx`.
fn masked(doc: &DrDocument, idx: usize, rect: Window, feather: f32) -> DrDocument {
    let (mw, mh) = (rect.2 * 2 / 3, rect.3 * 2 / 3);
    let plane = GrayImage::from_fn(mw, mh, |x, y| {
        let dx = x as f32 / mw as f32 - 0.5;
        let dy = y as f32 / mh as f32 - 0.5;
        Luma([if dx * dx + dy * dy < 0.2 {
            (120 + (x * 3 + y) % 136) as u8
        } else {
            0
        }])
    });
    let mut out = doc.clone();
    let mut properties = MaskProperties::at((rect.0 as i32 + 5, rect.1 as i32 + 3));
    properties.density = 0.9;
    properties.feather = feather;
    let layer = &mut out.layers[idx];
    layer.mask = Some(Arc::new(plane));
    layer.mask_enabled = true;
    layer.mask_properties = properties;
    out
}

// ------------------------------------------- the guide's own sizing rule --
//
// Written out independently of the core (13A-4a's restatement): sigma =
// radius / 2; the cell factor 1 up to sigma 3, then 2, 4, 8 at each
// doubling; the grid sigma is sigma at factor 1, else sqrt(sigma² − f²/3) /
// f; the blur's reach; two more cells for the bilinear read. The plane is
// aligned relative to the enclosing level's BUFFER origin and clamped to it.

fn factor(sigma: f32) -> u32 {
    if sigma <= 3.0 {
        1
    } else if sigma <= 6.0 {
        2
    } else if sigma <= 12.0 {
        4
    } else {
        8
    }
}

fn kernel_radius(sigma: f32) -> f32 {
    ((sigma - 0.8) / 0.3 + 1.0).max(0.5)
}

fn reduced(sigma: f32, f: u32) -> f32 {
    let f = f as f32;
    (sigma * sigma - f * f / 3.0).max(0.0).sqrt() / f
}

fn reach(sigma: f32) -> u32 {
    if sigma <= 0.0 {
        return 0;
    }
    let f = factor(sigma);
    if f == 1 {
        kernel_radius(sigma).ceil() as u32 + 1
    } else {
        f * (kernel_radius(reduced(sigma, f)).ceil() as u32 + 3)
    }
}

/// `(cell_pixels, reach_cells, halo)` for a guide of `radius`.
fn geometry(radius: f32) -> (u32, u32, u32) {
    let sigma = radius / 2.0;
    let f = factor(sigma);
    let grid = if f == 1 { sigma } else { reduced(sigma, f) };
    let cells = reach(grid) + 2;
    (f, cells, f * (cells + 8))
}

/// The plane over `inner` inside `buffer`: grown by f·reach, aligned outward
/// to f·8 RELATIVE TO THE BUFFER'S ORIGIN, clamped to the buffer.
fn plane_in(radius: f32, inner: Window, buffer: Window) -> Window {
    let (f, cells, _) = geometry(radius);
    let grow = u64::from(f * cells);
    let lattice = u64::from(f * 8);
    let (bx, by) = (u64::from(buffer.0), u64::from(buffer.1));
    let x0 = bx + (u64::from(inner.0) - bx).saturating_sub(grow) / lattice * lattice;
    let y0 = by + (u64::from(inner.1) - by).saturating_sub(grow) / lattice * lattice;
    let x1 = (bx + (u64::from(inner.0 + inner.2) + grow - bx).div_ceil(lattice) * lattice)
        .min(bx + u64::from(buffer.2));
    let y1 = (by + (u64::from(inner.1 + inner.3) + grow - by).div_ceil(lattice) * lattice)
        .min(by + u64::from(buffer.3));
    (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
}

fn grown(window: Window, halo: u32, canvas: (u32, u32)) -> Value {
    let x0 = window.0.saturating_sub(halo);
    let y0 = window.1.saturating_sub(halo);
    let x1 = (window.0 + window.2 + halo).min(canvas.0);
    let y1 = (window.1 + window.3 + halo).min(canvas.1);
    json!({"x": x0, "y": y0, "width": x1 - x0, "height": y1 - y0})
}

fn region_json((x, y, w, h): Window) -> Value {
    json!({"x": x, "y": y, "width": w, "height": h})
}

fn window_of(v: &Value) -> Window {
    let n = |k: &str| v[k].as_u64().unwrap() as u32;
    (n("x"), n("y"), n("width"), n("height"))
}

fn area((_, _, w, h): Window) -> u64 {
    u64::from(w) * u64::from(h)
}

/// The guide's (scratch, work) over `plane` (the shared typed planner).
fn guide_figures(radius: f32, plane: Window) -> (u64, u64) {
    let meta = json!({"type":"adjust","op":"shadows_highlights","evaluation":"precision_v1",
        "params":{"shadows":{"amount":0.7},"radius":radius}})
    .to_string();
    PrecisionAdjustment::guide_plan_for_description(&meta, plane.2, plane.3).unwrap()
}

fn within(inner: Window, outer: Window) -> bool {
    inner.0 >= outer.0
        && inner.1 >= outer.1
        && inner.0 + inner.2 <= outer.0 + outer.2
        && inner.1 + inner.3 <= outer.1 + outer.3
}

fn stage(
    level: Value,
    buffer: Window,
    layer: usize,
    radius: f32,
    plane: Window,
    through: &[usize],
) -> Value {
    let (f, cells, _) = geometry(radius);
    json!({"level": level, "buffer": region_json(buffer), "layer": layer,
        "plane": region_json(plane), "cell_pixels": f, "reach_cells": cells,
        "through": through})
}

fn scratch(p: &Value) -> u64 {
    p["scratch_heap_bytes"].as_u64().unwrap()
}

fn work(p: &Value) -> u64 {
    p["work_units"].as_u64().unwrap()
}

/// The ROOT staged plan restated for clip units under STYLED bases:
/// `guides` are (layer, radius, rows, through) from the first guide to the
/// last — `rows` counted from the guided row, or for a clip MEMBER from its
/// unit's FIRST row, to the next guided row — over `rows_0` rows below the
/// first guide; `holds` planes over `R_0` (one per distinct clip unit holding
/// a guided member and per held pass-through group) and, with `background`,
/// D5's Background snapshot one more. A style's own scratch (its planes) is
/// the same with the guides hidden, so the staged scratch is restated as the
/// difference from that plan; the work is restated whole: every stage's rows
/// over the next plane (the last over the output), every guide, the
/// accumulator fill, the hold fills and stage 0's `prev` fill over `R_0` —
/// with `background` D5's `snapshot` fill one more (13A-5b-iii-a) — plus the
/// preparation the 8-bit plan counts since 13A-5b-iii-b-2 (`prep`: the
/// unguided plan's work less its fill and one pass per row).
/// The input is the window grown by the halos, or — a style with planes —
/// the whole canvas.
fn assert_root_plan(
    doc: &DrDocument,
    guides: &[(usize, f32, u64, &[usize])],
    rows_0: u64,
    holds: u64,
    background: bool,
    window: Window,
    d: u32,
) {
    let (cw, ch) = (doc.width, doc.height);
    let canvas = (0, 0, cw, ch);
    let p = plan(doc, window, d).unwrap();
    let pixels = u64::from(window.2.div_ceil(d)) * u64::from(window.3.div_ceil(d));
    // Planes from the LAST guide backwards.
    let mut planes = vec![window; guides.len()];
    let mut inner = window;
    for (k, &(_, radius, _, _)) in guides.iter().enumerate().rev() {
        inner = plane_in(radius, inner, canvas);
        planes[k] = inner;
    }
    let stages: Vec<Value> = guides
        .iter()
        .zip(&planes)
        .map(|(&(layer, radius, _, through), &plane)| {
            stage(Value::Null, canvas, layer, radius, plane, through)
        })
        .collect();
    assert_eq!(p["stages"], json!(stages), "{window:?}/{d}");
    let halo: u32 = guides
        .iter()
        .map(|&(_, radius, _, _)| geometry(radius).2)
        .sum();
    assert_eq!(p["neighborhood"]["halo_pixels"], halo, "{window:?}/{d}");
    if p["global_analysis"] == json!(false) {
        assert_eq!(p["required_input_region"], grown(window, halo, (cw, ch)));
    } else {
        assert_eq!(p["required_input_region"], region_json(canvas));
    }
    assert!(within(planes[0], window_of(&p["required_input_region"])));
    let mut unguided = doc.clone();
    for &(layer, ..) in guides {
        unguided.layers[layer].visible = false;
    }
    let bare = plan(&unguided, window, d).unwrap();
    assert_eq!(bare["stages"], json!([]));
    let r0 = planes[0];
    let bare_rows = bare["layers"].as_array().unwrap().len() as u64;
    let prep = work(&bare) - pixels * (bare_rows + 1);
    let mut expected_scratch = scratch(&bare) + (holds + u64::from(background)) * area(r0) * 16;
    let mut expected_work = prep
        + area(r0) * rows_0
        + pixels
        + holds * area(r0)
        + (1 + u64::from(background)) * area(r0);
    for (k, &(_, radius, rows, _)) in guides.iter().enumerate() {
        let (gs, gw) = guide_figures(radius, planes[k]);
        let evaluated = planes.get(k + 1).map_or(pixels, |&next| area(next));
        expected_scratch += area(planes[k]) * 16 + gs;
        expected_work += evaluated * rows + gw;
    }
    assert_eq!(scratch(&p), expected_scratch, "{window:?}/{d}");
    assert_eq!(work(&p), expected_work, "{window:?}/{d}");
}

const CW: u32 = 197;
const CH: u32 = 195;
const OFFSET: (i32, i32) = (31, 25);
const SIZE: (u32, u32) = (130, 120);
const PLANS: [(Window, u32); 2] = [((60, 50, 30, 20), 1), ((0, 0, CW, CH), 3)];

fn content() -> Window {
    rect_of(OFFSET, SIZE, (CW, CH))
}

/// Backdrop (0) at `backdrop_opacity` (1: a Background for D5), a holed
/// raster B (1) at `OFFSET` of `SIZE` with `blend` / `opacity` styled
/// `style`, and a radius-8 guide (2) CLIPPED to B.
fn styled_unit(style: &str, blend: BlendMode, opacity: f32, backdrop_opacity: f32) -> DrDocument {
    let mut doc = with_raster(
        &backdrop(CW, CH),
        holes(SIZE.0, SIZE.1, 1),
        OFFSET,
        blend,
        opacity,
    );
    doc.layers[0].opacity = backdrop_opacity;
    let doc = styled(&doc, 1, style);
    with_member_guide(&doc, 8.0, (40, 40))
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_guide_under_a_fill_only_base_matches_with_and_without_blend_if() {
    for (what, style) in [("fill-only", FILL), ("fill + Blend If", FILL_BLEND_IF)] {
        let doc = styled_unit(style, BlendMode::Overlay, 0.85, 1.0);
        assert_eq!(
            plan(&doc, (0, 0, 9, 9), 1).unwrap()["layers"][1]["style"],
            "fill"
        );
        assert_regions_match(&doc, content(), what);
        // Backdrop and B over R_0; B RE-RUN with the guide over the output;
        // the unit's began sample held.
        for (window, d) in PLANS {
            assert_root_plan(&doc, &[(2, 8.0, 2, &[])], 2, 1, false, window, d);
        }
    }
}

#[test]
fn a_guide_under_an_effects_base_at_opacity_1_matches() {
    let doc = styled_unit(FX, BlendMode::Overlay, 1.0, 1.0);
    let mut unguided = doc.clone();
    unguided.layers[2].visible = false;
    assert_ne!(
        full(&doc),
        full(&unguided),
        "the guide changes the projection"
    );
    assert_regions_match(&doc, content(), "effects at opacity 1");
    for (window, d) in PLANS {
        assert_root_plan(&doc, &[(2, 8.0, 2, &[])], 2, 1, false, window, d);
    }
    let p = plan(&doc, (60, 50, 30, 20), 1).unwrap();
    assert_eq!(p["layers"][1]["style"], "effects");
    assert_eq!(p["global_analysis"], json!(["layer_style_planes"]));
    assert_eq!(p["layers"][2]["clip_base"], 1);
}

#[test]
fn a_packaged_base_refuses_by_name_and_dissolve_matches() {
    // Effects below opacity 1: the PACKAGE (`composite_package_into` runs
    // the members per strip), refused at plan and render naming the guide.
    let packaged = styled_unit(FX, BlendMode::Overlay, 0.85, 1.0);
    assert_refuses(&packaged, 2, "shadows_highlights", PACKAGED_BASE);
    // With the guide hidden the packaged base itself is admitted.
    let mut unguided = packaged.clone();
    unguided.layers[2].visible = false;
    assert_regions_match(&unguided, content(), "the packaged base alone");
    // Dissolve is never packaged.
    let dissolve = styled_unit(FX, BlendMode::Dissolve, 0.85, 1.0);
    assert_regions_match(&dissolve, content(), "effects at 0.85 in Dissolve");
    for (window, d) in PLANS {
        assert_root_plan(&dissolve, &[(2, 8.0, 2, &[])], 2, 1, false, window, d);
    }
}

#[test]
fn knockout_bases_without_and_with_planes_match() {
    // Backdrop at 0.97 (no Background): a Shallow knockout base without
    // planes (routed, fill 0.5, at 0.85) and with effects at opacity 1.
    for (what, style, opacity) in [
        ("Shallow, no planes", SHALLOW, 0.85),
        ("Shallow with effects", SHALLOW_FX, 1.0),
    ] {
        let doc = styled_unit(style, BlendMode::Normal, opacity, 0.97);
        assert_regions_match(&doc, content(), what);
        for (window, d) in PLANS {
            assert_root_plan(&doc, &[(2, 8.0, 2, &[])], 2, 1, false, window, d);
        }
    }
}

#[test]
fn a_deep_knockout_base_under_d5s_background_matches() {
    // The backdrop (0) is a Background and the base's knockout asks for it:
    // the Background snapshot is one more plane over R_0.
    for (what, style, opacity) in [
        ("Deep", DEEP, 0.9),
        ("Shallow at the root", SHALLOW, 0.9),
        ("Shallow with effects", SHALLOW_FX, 1.0),
    ] {
        let doc = styled_unit(style, BlendMode::Normal, opacity, 1.0);
        assert_regions_match(&doc, content(), what);
        for (window, d) in PLANS {
            assert_root_plan(&doc, &[(2, 8.0, 2, &[])], 2, 1, true, window, d);
        }
    }
}

#[test]
fn a_materialized_group_base_matches() {
    // G (2) = { A (1) } with effects, at opacity 1; the guide (3) clipped
    // to G.
    let base = with_raster(
        &backdrop(CW, CH),
        holes(SIZE.0, SIZE.1, 1),
        OFFSET,
        BlendMode::Overlay,
        0.85,
    );
    let (base, g) = isolate(&base, &[1]);
    assert_eq!(g, 2);
    let base = styled(&base, g, FX);
    let doc = with_member_guide(&base, 8.0, (40, 40));
    assert_regions_match(&doc, content(), "a materialized group base");
    // rows_0: backdrop, A, G; the member stage: A, G and the guide.
    for (window, d) in PLANS {
        assert_root_plan(&doc, &[(3, 8.0, 3, &[])], 3, 1, false, window, d);
    }
    let p = plan(&doc, (60, 50, 30, 20), 1).unwrap();
    assert_eq!(p["global_analysis"], json!(["group_materialization"]));
}

#[test]
fn raster_members_below_and_above_the_guide_are_re_run_under_an_effects_base() {
    // Backdrop (0), B (1) with effects, a Multiply member (2), the guide
    // (3), a Screen member (4) — every member clipped to B.
    let doc = with_raster(
        &backdrop(CW, CH),
        holes(SIZE.0, SIZE.1, 1),
        OFFSET,
        BlendMode::Overlay,
        1.0,
    );
    let doc = styled(&doc, 1, FX);
    let doc = with_member(&doc, holes(70, 60, 2), (60, 50), BlendMode::Multiply, 0.9);
    let doc = with_member_guide(&doc, 8.0, (40, 40));
    let doc = with_member(&doc, holes(60, 70, 3), (80, 70), BlendMode::Screen, 0.8);
    assert_regions_match(&doc, content(), "members below and above the guide");
    // rows_0: backdrop, B, the Multiply member; the stage: B, both members
    // and the guide.
    for (window, d) in PLANS {
        assert_root_plan(&doc, &[(3, 8.0, 4, &[])], 3, 1, false, window, d);
    }
}

#[test]
fn two_guides_under_an_effects_base_resume_one_hold() {
    // Backdrop (0), B (1) with effects, guide8 (2), a raster member (3),
    // guide16 (4).
    let doc = with_raster(
        &backdrop(CW, CH),
        holes(SIZE.0, SIZE.1, 5),
        OFFSET,
        BlendMode::Normal,
        1.0,
    );
    let doc = styled(&doc, 1, FX);
    let doc = with_member_guide(&doc, 8.0, (30, 30));
    let doc = with_member(&doc, holes(70, 60, 6), (60, 70), BlendMode::Overlay, 0.9);
    let doc = with_member_guide(&doc, 16.0, (40, 40));
    assert_regions_match(&doc, content(), "two guides in one unit");
    for (window, d) in PLANS {
        assert_root_plan(
            &doc,
            &[(2, 8.0, 3, &[]), (4, 16.0, 4, &[])],
            2,
            1,
            false,
            window,
            d,
        );
    }
}

#[test]
fn a_shallow_knockout_member_beside_the_guide_under_an_effects_base_matches() {
    // Backdrop (0, not a Background at 0.97), B (1) with effects, a Shallow
    // knockout member (2) — it punches to the base-only sample — then the
    // guide (3); and a Shallow member above the guide.
    let mut doc = with_raster(
        &backdrop(CW, CH),
        holes(SIZE.0, SIZE.1, 1),
        OFFSET,
        BlendMode::Overlay,
        1.0,
    );
    doc.layers[0].opacity = 0.97;
    let doc = styled(&doc, 1, FX);
    let doc = with_member(&doc, holes(70, 60, 2), (60, 50), BlendMode::Normal, 0.9);
    let doc = with_member_guide(&doc, 8.0, (40, 40));
    let below = styled(&doc, 2, SHALLOW);
    assert_ne!(full(&below), full(&doc), "the knockout punches");
    assert_regions_match(&below, content(), "a Shallow member below the guide");
    for (window, d) in PLANS {
        assert_root_plan(&below, &[(3, 8.0, 3, &[])], 3, 1, false, window, d);
    }
    let above = with_member(&doc, holes(60, 50, 4), (70, 60), BlendMode::Normal, 0.9);
    let above = styled(&above, 4, SHALLOW);
    assert_regions_match(&above, content(), "a Shallow member above the guide");
}

#[test]
fn a_masked_multiply_member_guide_under_an_effects_base_matches() {
    // The guide's pixels span the canvas, so its mask reaches B (an
    // adjustment's mask covers only its pixel extent).
    let mut doc = styled_unit(FX, BlendMode::Overlay, 1.0, 1.0);
    doc.layers[2].pixels = LayerRaster::new(RgbaImage::new(CW, CH));
    doc.layers[2].offset = (0, 0);
    doc.layers[2].opacity = 0.7;
    doc.layers[2].blend = BlendMode::Multiply;
    let doc = masked(&doc, 2, content(), 6.0);
    assert_regions_match(
        &doc,
        content(),
        "a feather-6 masked Multiply 0.7 member guide",
    );
    let p = plan(&doc, (60, 50, 30, 20), 1).unwrap();
    assert_eq!(p["stages"][0]["layer"], 2);
    assert_eq!(p["layers"][2]["mask"], true);
}

#[test]
fn a_small_knockout_patch_feeds_the_guide_its_buffer_outside_the_rect() {
    // Backdrop (0, 0.97), a SMALL Shallow knockout raster K (1) — 14×12 at
    // (90, 80) — a Multiply member (2) clipped to K spilling far past it,
    // and a radius-8 guide (3) clipped to K. Outside K's rect the unit
    // buffer is the members over transparency clamped to the footprint's 0,
    // and the guide reads it there: the stage plane must hold it, not the
    // accumulator.
    let patch = rect_of((90, 80), (14, 12), (CW, CH));
    let mut doc = with_raster(
        &backdrop(CW, CH),
        holes(14, 12, 3),
        (90, 80),
        BlendMode::Normal,
        0.9,
    );
    doc.layers[0].opacity = 0.97;
    let doc = styled(&doc, 1, SHALLOW);
    let doc = with_member(&doc, holes(80, 70, 4), (60, 50), BlendMode::Multiply, 0.9);
    let doc = with_member_guide(&doc, 8.0, (92, 82));
    let mut unguided = doc.clone();
    unguided.layers[3].visible = false;
    assert_ne!(full(&doc), full(&unguided), "the guide changes the patch");
    assert_regions_match(&doc, patch, "a small knockout patch");
    for (window, d) in [((0, 0, CW, CH), 1), ((70, 60, 60, 50), 1)] {
        assert_root_plan(&doc, &[(3, 8.0, 3, &[])], 3, 1, false, window, d);
    }
}

#[test]
fn a_styled_unit_inside_isolated_and_pass_through_levels_matches() {
    // Backdrop (0); B (1) with effects at opacity 1, the guide (2) clipped
    // to B; the unit inside an isolated group, a plain and a gated
    // pass-through (3).
    let doc = styled_unit(FX, BlendMode::Overlay, 1.0, 1.0);
    let (iso, g) = isolate(&doc, &[1, 2]);
    assert_eq!(g, 3);
    assert_regions_match(&iso, content(), "a styled unit in an isolated group");
    let p = plan(&iso, (60, 50, 30, 20), 1).unwrap();
    assert_eq!(p["stages"][0]["level"], 3);
    assert_eq!(p["stages"][0]["layer"], 2);
    let (plain, p_idx) = pass(&doc, &[1, 2]);
    assert_eq!(p_idx, 3);
    assert_regions_match(&plain, content(), "a styled unit in a plain pass-through");
    let mut gated = plain.clone();
    gated.layers[3].opacity = 0.6;
    assert_regions_match(&gated, content(), "a styled unit in a gated pass-through");
    // Rows: backdrop and B over R_0; B, the guide and the group over the
    // output. Holds: the unit, plus the gated group.
    for (window, d) in PLANS {
        assert_root_plan(&plain, &[(2, 8.0, 3, &[3])], 2, 1, false, window, d);
        assert_root_plan(&gated, &[(2, 8.0, 3, &[3])], 2, 2, false, window, d);
    }
}

#[test]
fn linear_light_evaluates_a_document_with_a_styled_member_guide_regionally() {
    let doc = styled_unit(FILL, BlendMode::Normal, 1.0, 1.0);
    let linear = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(
        full(&linear),
        full(&doc),
        "the policy changes the projection"
    );
    assert_regions_match(&linear, content(), "linear light");
}

// ------------------------------------------------ measured cancellation --
//
// DECLARED (before measuring): a cancel returns within 4096 samples of the
// running stage loop — clip-unit re-runs and hold fills included — or after
// the running byte `blur_plane` / `rendered_group` / `LayerStyle::rendered`
// (uncancellable, 13A-5) completes. Each stage's per-sample estimate is calibrated on its OWN
// uncancelled loop: the time between the progress point that opens it and
// the one that opens the next, over its samples.

/// The 13A-5a model's RETURN FLOOR `T_f`: unwinding from a checkpoint to the
/// FFI return (dropping the live planes, the join, the poller's sleep).
const RETURN_FLOOR_S: f64 = 1e-3;

/// Runs `render`, waits for progress past each of `probes` in turn and
/// returns the instants it passed them; with `cancel`, cancels at the last
/// probe and returns the cancel-to-return latency as the last duration.
fn probed<F>(control: *mut DrJobControl, probes: &[f64], cancel: bool, render: F) -> Vec<Duration>
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let token = control as usize;
    thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let (ok, message) = render(token);
            (ok, message, Instant::now())
        });
        let deadline = Instant::now() + Duration::from_secs(240);
        let mut at = Vec::new();
        for &probe in probes {
            while unsafe { dr_job_control_progress(control) } <= probe && !worker.is_finished() {
                assert!(Instant::now() < deadline, "the stage never started");
                thread::sleep(Duration::from_micros(100));
            }
            assert!(
                !worker.is_finished(),
                "render completed before probe {probe}"
            );
            at.push(Instant::now());
        }
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
        let mut out: Vec<Duration> = at.windows(2).map(|w| w[1] - w[0]).collect();
        out.push(returned.saturating_duration_since(*at.last().unwrap()));
        out
    })
}

/// Measures and prints each stage: `stages` are (label, the progress that
/// opens it, the progress that opens the next — or `None` for the last —
/// its samples). Without `bench` it measures nothing: each stage is
/// cancelled with the worker PARKED past the report that opens it, the count
/// from `twin`'s own report log (`cancelled_past_each`), and the parked
/// figures are printed. `twin` renders a FRESH fixture on every call, and the
/// parked runs use it too: a run parked in a later stage has completed the
/// earlier stages' cached planes, which a later run on the same document
/// would skip, moving its count past the render's end.
fn measure<T, F>(
    bench: bool,
    depth: &str,
    stages: &[(&str, f64, Option<f64>, f64)],
    twin: T,
    render: F,
) where
    T: Fn(usize) -> (bool, String) + Send + Copy,
    F: Fn(usize) -> (bool, String) + Send + Copy,
{
    if !bench {
        let opens: Vec<f64> = stages.iter().map(|&(_, open, ..)| open).collect();
        let parked = common::probe::cancelled_past_each(&opens, twin, twin);
        for (&(label, ..), p) in stages.iter().zip(&parked) {
            eprintln!(
                "{depth} cancel during {label}: {}",
                common::probe::parked_line(p)
            );
        }
        return;
    }
    for &(label, open, next, samples) in stages {
        let probes: Vec<f64> = std::iter::once(open).chain(next).collect();
        let control = unsafe { dr_job_control_create() };
        let looped = probed(control, &probes, false, render);
        unsafe { dr_job_control_free(control) };
        let per_sample = looped[0].as_secs_f64() / samples;
        let control = unsafe { dr_job_control_create() };
        let latency = *probed(control, &[open], true, render).last().unwrap();
        // The 13A-5a model: a stage of checkpoint interval W (4096 samples of
        // the running stage loop or hold fill) returns within T_f + W·c, T_f
        // the 1 ms return floor and c its per-sample cost on its OWN
        // uncancelled loop; "past the floor" is max(0, latency − T_f) / c.
        let latency_s = latency.as_secs_f64();
        let bound = RETURN_FLOOR_S + 4096.0 * per_sample;
        eprintln!(
            "13A-4b-ii-b-2 {depth} cancel during {label} (13A-5a model): {:.3} ms \
             cancel-to-return; c = {:.2} ns per sample (loop {:.1} ms over {samples} \
             samples); bound T_f + W·c = {:.3} ms (T_f 1 ms, W 4096 samples); ≈{:.0} \
             samples past the floor; {}",
            latency_s * 1e3,
            per_sample * 1e9,
            looped[0].as_secs_f64() * 1e3,
            bound * 1e3,
            (latency_s - RETURN_FLOOR_S).max(0.0) / per_sample,
            if latency_s <= bound { "PASS" } else { "MISS" },
        );
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        unsafe { dr_job_control_free(control) };
    }
}

/// Backdrop (0), a root radius-30 guide (1), a Multiply raster over most of
/// the canvas (2) styled `style` at `opacity` with a radius-30 guide (3)
/// CLIPPED to it, and a SoftLight raster (4), over the whole 3000×2000
/// canvas. Cursors: [1], then [2, 0] — the unit is entered (its began sample
/// held over R_1) by the loop after the root guide and re-run over the
/// output. Every probe sits where 13A-4b-ii-b-1's did — re-based in 13A-5a
/// for the reporting hold fill, in 13A-5b-iii-a for the reporting stage-0
/// fills (`d5`: the backdrop is D5's Background, whose `snapshot` fill is one
/// more `|R_0|`) and in 13A-5b-iii-b-2 for the style's planes, which the
/// 8-bit plan now counts and resolves before the stages: every probe is
/// shifted by `prep / work` (the u16 `extra` pattern), `prep` the plan's
/// work with layers 1 and 3 hidden less its fill and one pass per row.
/// `bench` is `measure`'s.
fn measure_byte(bench: bool, variant: &str, style: &str, opacity: f32, d5: bool) {
    let (w, h) = (3000u32, 2000u32);
    let build = || {
        let doc = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
            Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
        }));
        let doc = with_guide(&doc, 30.0, (0, 0));
        let doc = with_raster(
            &doc,
            holes(w - 43, h - 37, 1),
            (21, 19),
            BlendMode::Multiply,
            opacity,
        );
        let doc = styled(&doc, 2, style);
        let doc = with_member_guide(&doc, 30.0, (30, 30));
        with_raster(
            &doc,
            holes(w / 2, h / 2, 2),
            (100, 100),
            BlendMode::SoftLight,
            0.8,
        )
    };
    let doc = build();
    let p = plan(&doc, (0, 0, w, h), 1).unwrap();
    assert_eq!(p["stages"].as_array().unwrap().len(), 2);
    assert_eq!(p["stages"][1]["layer"], 3);
    let work = work(&p) as f64;
    let r0 = window_of(&p["stages"][0]["plane"]);
    let r1 = window_of(&p["stages"][1]["plane"]);
    let (_, g0) = guide_figures(30.0, r0);
    let pixels = f64::from(w) * f64::from(h);
    let mut bare = doc.clone();
    bare.layers[1].visible = false;
    bare.layers[3].visible = false;
    let bare = plan(&bare, (0, 0, w, h), 1).unwrap();
    let bare_rows = bare["layers"].as_array().unwrap().len() as f64;
    let prep = self::work(&bare) as f64 - pixels * (bare_rows + 1.0);
    // Progress (reported per row): stage 0's `prev` fill and, under D5, its
    // `snapshot` fill (`f0 = |R_0|·(1 + [D5])`, per 4096-sample chunk,
    // 13A-5b-iii-a), stage 0 (the backdrop over R_0), guide 1, the hold's
    // fill (per 4096-sample chunk, 13A-5a), loop 0 (guide 1 and the styled
    // base over R_1, entering the unit), guide 2, the final loop (the unit
    // re-run — the styled base and guide — and the SoftLight raster over the
    // output). Each probe passes one row (or chunk) after its value, and
    // every value is shifted by the preparation (`at`, 13A-5b-iii-b-2).
    let at = |v: f64| (prep + v) / work;
    let (a0, a1) = (area(r0) as f64, area(r1) as f64);
    let f0 = a0 * (1.0 + f64::from(u8::from(d5)));
    let c = 4096.0;
    let (w0, w1) = (f64::from(r0.2), f64::from(r1.2));
    let g0 = g0 as f64;
    let (_, g1) = guide_figures(30.0, r1);
    let g1 = g1 as f64;
    let ww = f64::from(w);
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
    let twin = |token: usize| {
        let twin = handle(&build());
        let out = render_at(twin as usize)(token);
        unsafe { dr_doc_free(twin) };
        out
    };
    measure(
        bench,
        &format!("8-bit {variant}"),
        &[
            ("the stage-0 fill", at(c), Some(at(f0 + w0)), f0 - c),
            (
                "stage 0",
                at(f0 + w0),
                Some(at(f0 + a0 - w0)),
                a0 - 2.0 * w0,
            ),
            (
                "the unit's hold fill",
                at(f0 + a0 + g0),
                Some(at(f0 + a0 + g0 + a1)),
                a1,
            ),
            (
                "the entering stage (loop 0)",
                at(f0 + a0 + g0 + a1 + 2.0 * w1),
                Some(at(f0 + a0 + g0 + a1 + 2.0 * (a1 - w1))),
                a1 - 2.0 * w1,
            ),
            (
                "the root loop (the resumed unit)",
                at(f0 + a0 + g0 + 3.0 * a1 + g1 + 3.0 * ww),
                None,
                pixels - 2.0 * ww,
            ),
        ],
        twin,
        render,
    );
    unsafe { dr_doc_free(hd) };
}

#[test]
fn a_concurrently_cancelled_styled_member_guided_render_returns_operation_cancelled() {
    // An effects base at opacity 1 (the styled-direct member loop), and a
    // Deep knockout base (routed) with the backdrop a D5 Background.
    measure_byte(false, "effects base", FX, 1.0, false);
    measure_byte(false, "Deep knockout base", DEEP, 0.9, true);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_styled_member_guided_render_is_measured() {
    measure_byte(true, "effects base", FX, 1.0, false);
    measure_byte(true, "Deep knockout base", DEEP, 0.9, true);
}

/// The u16 twin of [`measure_byte`]: the same five records. The typed plan
/// counts the style's preparation (`prepare` advances its planes before the
/// loop), so every probe is shifted by it: `extra` is the plan's work less
/// 13A-4b-ii-b-1's figure for the same structure. `bench` is `measure`'s.
fn measure_u16(bench: bool, variant: &str, style: &str, opacity: f32) {
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
                        } else if (x + y).is_multiple_of(7) {
                            0
                        } else {
                            51000
                        },
                    ]
                })
                .collect();
            PrecisionImage::from_rgba16(iw, ih, pixels, profile.clone(), None).unwrap()
        };
        let op = json!({"type":"adjust","op":"shadows_highlights","evaluation":"precision_v1",
        "params":{"shadows":{"amount":0.7,"tone":0.45},"highlights":{"amount":0.35},"radius":30}})
        .to_string();
        let parsed = LayerStyle::from_json(style).unwrap();
        let layers = vec![
            PrecisionDocumentLayer::raster("Base", image(w, h, 1, false), (0, 0), 1.0, true)
                .unwrap(),
            PrecisionDocumentLayer::adjustment("Root guide", &op, 0.8, true, &profile).unwrap(),
            PrecisionDocumentLayer::raster(
                "Mid",
                image(w - 43, h - 37, 3, true),
                (21, 19),
                opacity,
                true,
            )
            .unwrap()
            .with_blend_mode(BlendMode::Multiply)
            .unwrap()
            .with_layer_style(Some(&parsed), (w, h))
            .unwrap(),
            PrecisionDocumentLayer::adjustment("Guide", &op, 0.8, true, &profile)
                .unwrap()
                .with_clipped(true),
            PrecisionDocumentLayer::raster(
                "Top",
                image(w / 2, h / 2, 5, true),
                (100, 100),
                0.8,
                true,
            )
            .unwrap()
            .with_blend_mode(BlendMode::SoftLight)
            .unwrap(),
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
    let p = region_plan::typed(&d, (0, 0, w, h), 1).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p["stages"].as_array().unwrap().len(), 2);
    assert_eq!(p["stages"][1]["layer"], 3);
    let work = work(&p) as f64;
    let pixels = f64::from(w) * f64::from(h);
    let (_, gw) = guide_figures(30.0, (0, 0, w, h));
    let g = gw as f64;
    // Progress (reported per 4096-sample chunk): the style's preparation,
    // `P_0`'s fill, stage 0 (the base over the canvas), guide 1, the hold's
    // fill, loop 0 (guide 1 and Mid over the canvas, entering the unit),
    // guide 2, the output's fill, the final loop (the unit re-run — Mid and
    // the guide — and Top over the output). Each probe passes one chunk
    // after its value.
    let extra = work - (9.0 * pixels + 2.0 * g);
    assert!(extra >= 0.0, "the plan's work covers the structure");
    let c = 4096.0;
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
    let twin = |token: usize| {
        let twin = build();
        render_at(&twin as *const PrecisionDocument as usize)(token)
    };
    measure(
        bench,
        &format!("u16 {variant}"),
        &[
            (
                "stage 0",
                (extra + pixels) / work,
                Some((extra + 2.0 * pixels - c) / work),
                pixels - c,
            ),
            (
                "the unit's hold fill",
                (extra + 2.0 * pixels + g) / work,
                Some((extra + 3.0 * pixels + g - c) / work),
                pixels - c,
            ),
            (
                "the entering stage (loop 0)",
                (extra + 3.0 * pixels + g) / work,
                Some((extra + 5.0 * pixels + g - 2.0 * c) / work),
                pixels - c,
            ),
            (
                "the root loop (the resumed unit)",
                (extra + 6.0 * pixels + 2.0 * g) / work,
                None,
                pixels - c,
            ),
        ],
        twin,
        render,
    );
}

#[test]
fn a_concurrently_cancelled_styled_member_guided_render_returns_operation_cancelled_at_u16() {
    measure_u16(false, "effects base", FX, 1.0);
    measure_u16(false, "Deep knockout base", DEEP, 0.9);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_styled_member_guided_render_is_measured_at_u16() {
    measure_u16(true, "effects base", FX, 1.0);
    measure_u16(true, "Deep knockout base", DEEP, 0.9);
}
