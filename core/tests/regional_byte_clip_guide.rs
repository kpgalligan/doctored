//! Phase 13A-4b-ii-b-1: the 8-bit regional document render over a
//! guide-reading Shadows/Highlights as a CLIP MEMBER of an UNSTYLED base (at
//! the root, inside isolated levels, inside plain and gated pass-through
//! groups, beside knockout members, under D5's Background), staged through the
//! enclosing level with the unit re-run from its held "began" sample, against
//! the full-frame projection. The oracle is INDEPENDENT of the regional code:
//! `dr_doc_flattened`'s projection cropped and top-left-cell sampled here in
//! plain index arithmetic, byte for byte (tolerance: EXACT, declared before
//! measuring). The plan figures are restated from the guide's own sizing rule
//! anchored at the enclosing level's buffer, written out independently: one
//! held plane over `R_0` per distinct clip unit holding a guided member (and
//! per held pass-through group), and every member stage's rows counted from
//! its unit's FIRST row (the fill re-runs the unit from its base). The two
//! concurrent-cancel measurements (8 bits and u16) live here, as
//! `#[ignore]`d `…_is_measured` twins, so `make cancel-bench`
//! records both depths; their default twins cancel with the worker parked.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::{BlendMode, DrDocument, LayerKind, LayerRaster};
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
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::{adjust_meta, img_dims, img_pixels, plasma, styled, take_err_string};

use crate::region_plan;

type Window = (u32, u32, u32, u32);

const GROUP_STYLE: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                           {\"type\":\"stroke\",\"size\":3}]}";

const SHALLOW: &str = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}";

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

/// Backdrop (0), a holed Overlay raster B (1) at `offset` of `size`, and a
/// guide of `radius` (2) CLIPPED to B, inside B.
fn unit(cw: u32, ch: u32, radius: f32, offset: (i32, i32), size: (u32, u32)) -> DrDocument {
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    with_member_guide(&doc, radius, (offset.0.max(0) + 2, offset.1.max(0) + 3))
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

fn meet(a: Window, b: Window) -> Option<Window> {
    let x0 = a.0.max(b.0);
    let y0 = a.1.max(b.1);
    let x1 = (a.0 + a.2).min(b.0 + b.2);
    let y1 = (a.1 + a.3).min(b.1 + b.3);
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
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

/// 13A-5c-3: the description parse the byte plan reserves under every
/// profile — the largest non-group `meta` length × 256.
fn description_parse(doc: &DrDocument) -> u64 {
    doc.layers
        .iter()
        .filter(|l| l.kind != LayerKind::Group)
        .filter_map(|l| l.meta.as_ref().map(|m| m.len() as u64))
        .max()
        .unwrap_or(0)
        * 256
}

/// The ROOT staged plan restated: `guides` are (layer, radius, rows) from
/// the first guide to the last — `rows` counted from the guided row, or for
/// a clip MEMBER from its unit's FIRST row, to the next guided row — over
/// `rows_0` rows below the first guide; `holds` planes over `R_0` (one per
/// distinct clip unit holding a guided member and per held pass-through
/// group). Scratch: the accumulator and the output, every stage plane and
/// guide, the holds; work: each stage's rows over the next plane (the last
/// over the output), every guide, the accumulator fill, the hold fills and
/// stage 0's `prev` fill over `R_0` (13A-5b-iii-a; no caller here has D5).
fn assert_root_plan(
    doc: &DrDocument,
    guides: &[(usize, f32, u64, &[usize])],
    rows_0: u64,
    holds: u64,
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
    assert_eq!(p["required_input_region"], grown(window, halo, (cw, ch)));
    assert!(within(planes[0], window_of(&p["required_input_region"])));
    let r0 = planes[0];
    let mut expected_scratch =
        pixels * 16 + pixels * 4 + holds * area(r0) * 16 + description_parse(doc);
    let mut expected_work = area(r0) * rows_0 + pixels + holds * area(r0) + area(r0);
    for (k, &(_, radius, rows, _)) in guides.iter().enumerate() {
        let (gs, gw) = guide_figures(radius, planes[k]);
        let evaluated = planes.get(k + 1).map_or(pixels, |&next| area(next));
        expected_scratch += area(planes[k]) * 16 + gs;
        expected_work += evaluated * rows + gw;
    }
    assert_eq!(scratch(&p), expected_scratch, "{window:?}/{d}");
    assert_eq!(work(&p), expected_work, "{window:?}/{d}");
}

fn assert_unit_radius(cw: u32, ch: u32, radius: f32) {
    let offset = (37, 29);
    let size = (cw - 80, ch - 70);
    let rect = rect_of(offset, size, (cw, ch));
    let doc = unit(cw, ch, radius, offset, size);
    let mut unguided = doc.clone();
    unguided.layers[2].visible = false;
    assert_ne!(
        full(&doc),
        full(&unguided),
        "radius {radius}: the guide changes the projection"
    );
    assert_regions_match(&doc, rect, &format!("radius {radius}"));
    for (window, d) in [
        ((cw / 3 + 1, ch / 3 + 1, cw / 3 - 2, ch / 3 - 2), 1),
        ((0, 0, cw, ch), 2),
        ((rect.0 - 4, rect.1 - 3, 9, 7), 1),
        ((cw - 3, ch - 3, 3, 3), 3),
    ] {
        // Backdrop and B over R_0; B RE-RUN with the guide over the output
        // (two rows from the unit's first); the unit's began sample held.
        assert_root_plan(&doc, &[(2, radius, 2, &[])], 2, 1, window, d);
        let p = plan(&doc, window, d).unwrap();
        let (_, _, halo) = geometry(radius);
        assert_eq!(p["layers"][2]["neighborhood"]["halo_pixels"], halo);
        assert_eq!(p["layers"][2]["clip_base"], 1);
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_guide_clipped_to_a_holey_raster_at_the_root_stages_radius_4() {
    assert_unit_radius(113, 101, 4.0);
}

#[test]
fn a_guide_clipped_to_a_holey_raster_at_the_root_stages_radius_30() {
    assert_unit_radius(389, 387, 30.0);
}

#[test]
fn a_guide_clipped_to_a_holey_raster_at_the_root_stages_radius_120() {
    let (_, _, halo) = geometry(120.0);
    assert_unit_radius(3 * halo + 5, 3 * halo + 3, 120.0);
}

#[test]
fn the_spec_example_counts_the_member_stage_from_the_units_first_row() {
    // Raster base B (row 0) + guided member M (row 1) at the root: rows_0 =
    // 1, stages[0].rows = 2, work area(r0)·1 + pixels·2 + gw + pixels +
    // area(r0).
    let (cw, ch) = (113, 101);
    let doc = DrDocument::from_pixels(holes(cw, ch, 3));
    let doc = with_member_guide(&doc, 8.0, (30, 30));
    assert_regions_match(&doc, (0, 0, cw, ch), "B + M");
    for (window, d) in [((30, 20, 40, 30), 1), ((0, 0, cw, ch), 3)] {
        assert_root_plan(&doc, &[(1, 8.0, 2, &[])], 1, 1, window, d);
    }
}

#[test]
fn raster_members_below_and_above_the_guide_are_re_run_with_the_unit() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0), B (1), a Multiply member (2), the guide (3), a Screen
    // member (4) — every member clipped to B.
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    let doc = with_member(&doc, holes(70, 60, 2), (60, 50), BlendMode::Multiply, 0.9);
    let doc = with_member_guide(&doc, 8.0, (40, 40));
    let doc = with_member(&doc, holes(60, 70, 3), (80, 70), BlendMode::Screen, 0.8);
    assert_regions_match(&doc, rect, "members below and above the guide");
    // rows_0: backdrop, B, the Multiply member; the stage: B, both members
    // and the guide.
    for (window, d) in [((60, 50, 30, 20), 1), ((0, 0, cw, ch), 2)] {
        assert_root_plan(&doc, &[(3, 8.0, 4, &[])], 3, 1, window, d);
    }
}

#[test]
fn two_guides_in_one_unit_resume_one_hold() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((23, 19), (160, 150));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0), B (1), guide8 (2), a raster member (3), guide16 (4).
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 5),
        offset,
        BlendMode::Normal,
        0.95,
    );
    let doc = with_member_guide(&doc, 8.0, (30, 30));
    let doc = with_member(&doc, holes(70, 60, 6), (60, 70), BlendMode::Overlay, 0.9);
    let doc = with_member_guide(&doc, 16.0, (40, 40));
    assert_regions_match(&doc, rect, "two guides in one unit");
    // Stage 1 counts B and the guide8 member (from the unit's first row);
    // stage 2 counts B, both members and both guides; ONE hold.
    for (window, d) in [((80, 70, 30, 20), 1), ((0, 0, cw, ch), 3)] {
        assert_root_plan(
            &doc,
            &[(2, 8.0, 3, &[]), (4, 16.0, 4, &[])],
            2,
            1,
            window,
            d,
        );
    }
}

#[test]
fn a_shallow_knockout_member_beside_the_guide_matches() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0, not a Background at 0.97), B (1), a Shallow knockout
    // member (2) — it punches to the base-only sample — then the guide (3).
    let mut doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    doc.layers[0].opacity = 0.97;
    let doc = with_member(&doc, holes(70, 60, 2), (60, 50), BlendMode::Normal, 0.9);
    let doc = with_member_guide(&doc, 8.0, (40, 40));
    let knockout = styled(&doc, 2, SHALLOW);
    assert_ne!(full(&knockout), full(&doc), "the knockout punches");
    assert_regions_match(&knockout, rect, "a Shallow member below the guide");
    // Above the guide.
    let above = with_member(&doc, holes(60, 50, 4), (70, 60), BlendMode::Normal, 0.9);
    let above = styled(&above, 4, SHALLOW);
    assert_regions_match(&above, rect, "a Shallow member above the guide");
    assert_root_plan(&knockout, &[(3, 8.0, 3, &[])], 3, 1, (60, 50, 30, 20), 1);
}

#[test]
fn a_masked_multiply_member_guide_matches() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // The guide's pixels span the canvas, so its mask reaches B (an
    // adjustment's mask covers only its pixel extent).
    let mut doc = unit(cw, ch, 8.0, offset, size);
    doc.layers[2].pixels = LayerRaster::new(RgbaImage::new(cw, ch));
    doc.layers[2].offset = (0, 0);
    doc.layers[2].opacity = 0.7;
    doc.layers[2].blend = BlendMode::Multiply;
    let doc = masked(&doc, 2, rect, 6.0);
    assert_regions_match(&doc, rect, "a feather-6 masked Multiply 0.7 member guide");
    let p = plan(&doc, (60, 50, 30, 20), 1).unwrap();
    assert_eq!(p["stages"][0]["layer"], 2);
    assert_eq!(p["layers"][2]["mask"], true);
}

#[test]
fn hidden_and_zero_opacity_members_are_dropped_or_clamp_only() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0), B (1), a raster member (2), the guide (3).
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    let doc = with_member(&doc, holes(70, 60, 2), (60, 50), BlendMode::Multiply, 0.9);
    let doc = with_member_guide(&doc, 8.0, (40, 40));
    // A hidden raster member is dropped; a zero-opacity one is a clamp-only
    // step before the guide's.
    for (visible, opacity) in [(false, 0.9), (true, 0.0)] {
        let mut d = doc.clone();
        d.layers[2].visible = visible;
        d.layers[2].opacity = opacity;
        let what = format!("raster member visible {visible} opacity {opacity}");
        assert_regions_match(&d, rect, &what);
        assert_eq!(
            plan(&d, (60, 50, 30, 20), 1).unwrap()["stages"][0]["layer"],
            3,
            "{what}"
        );
    }
    // The guided member itself hidden is dropped, at opacity 0 it is a
    // clamp-only step: no stage either way.
    for (visible, opacity) in [(false, 0.8), (true, 0.0)] {
        let mut d = doc.clone();
        d.layers[3].visible = visible;
        d.layers[3].opacity = opacity;
        let what = format!("guide visible {visible} opacity {opacity}");
        assert_regions_match(&d, rect, &what);
        assert_eq!(
            plan(&d, (60, 50, 30, 20), 1).unwrap()["stages"],
            json!([]),
            "{what}"
        );
    }
}

#[test]
fn an_isolated_group_base_is_charged_in_the_member_stage() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); G (3) = { A (1), C (2) } isolated; the guide (4)
    // clipped to G.
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    let doc = with_raster(&doc, holes(70, 60, 2), (60, 50), BlendMode::Multiply, 0.9);
    let (doc, g) = isolate(&doc, &[1, 2]);
    assert_eq!(g, 3);
    let doc = with_member_guide(&doc, 8.0, (40, 40));
    assert_regions_match(&doc, rect, "a guide clipped to an isolated group");
    // rows_0: backdrop, A, C, G; the member stage: A, C, G and the guide.
    for (window, d) in [((60, 50, 30, 20), 1), ((0, 0, cw, ch), 7)] {
        assert_root_plan(&doc, &[(4, 8.0, 4, &[])], 4, 1, window, d);
    }
    let mut gated = doc.clone();
    gated.layers[3].opacity = 0.6;
    assert_regions_match(&gated, rect, "the group base at 0.6");
}

#[test]
fn a_unit_inside_an_isolated_level_stages_the_groups_level() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((101, 97), (80, 70));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); G (3) = { B (1), guide (2) clipped to B }.
    let doc = unit(cw, ch, 8.0, offset, size);
    let (doc, g) = isolate(&doc, &[1, 2]);
    assert_eq!(g, 3);
    assert_regions_match(&doc, rect, "a unit in an isolated group");
    let window = (120, 110, 30, 20);
    let p = plan(&doc, window, 1).unwrap();
    let r1 = meet(window, rect).unwrap();
    let r0 = plane_in(8.0, r1, rect);
    assert_eq!(p["stages"], json!([stage(json!(3), rect, 2, 8.0, r0, &[])]));
    assert_eq!(plan(&doc, (0, 0, 20, 20), 1).unwrap()["stages"], json!([]));
    // Under a ROOT guide (4) above it: the root's level, then the group's.
    let rooted = with_guide(&doc, 4.0, (0, 0));
    assert_regions_match(&rooted, rect, "under a root guide");
    let canvas = (0, 0, cw, ch);
    let root_r0 = plane_in(4.0, window, canvas);
    let g_r1 = meet(root_r0, rect).unwrap();
    assert_eq!(
        plan(&rooted, window, 1).unwrap()["stages"],
        json!([
            stage(Value::Null, canvas, 4, 4.0, root_r0, &[]),
            stage(json!(3), rect, 2, 8.0, plane_in(8.0, g_r1, rect), &[]),
        ])
    );
}

#[test]
fn a_unit_inside_plain_and_gated_pass_throughs_holds_the_unit_and_the_gate() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); P (3) = { B (1), guide (2) clipped to B }.
    let doc = unit(cw, ch, 8.0, offset, size);
    let (plain, p_idx) = pass(&doc, &[1, 2]);
    assert_eq!(p_idx, 3);
    assert_regions_match(&plain, rect, "a unit in a plain pass-through");
    let mut gated = plain.clone();
    gated.layers[3].opacity = 0.6;
    assert_regions_match(&gated, rect, "a unit in a gated pass-through");
    let gated_masked = masked(&gated, 3, rect, 6.0);
    assert_regions_match(
        &gated_masked,
        rect,
        "a unit in a gated, masked pass-through",
    );
    // Rows: backdrop and B over R_0; B, the guide and the group over the
    // output. Holds: the unit, plus the gated group.
    for (window, d) in [((60, 50, 30, 20), 1), ((0, 0, cw, ch), 2)] {
        assert_root_plan(&plain, &[(2, 8.0, 3, &[3])], 2, 1, window, d);
        assert_root_plan(&gated, &[(2, 8.0, 3, &[3])], 2, 2, window, d);
    }
    // Nested: Outer (4) = { P (3) } gated, P plain.
    let (mut nested, outer) = pass(&plain, &[3]);
    assert_eq!(outer, 4);
    nested.layers[4].opacity = 0.7;
    assert_regions_match(&nested, rect, "a unit in plain in gated");
    assert_root_plan(&nested, &[(2, 8.0, 4, &[4, 3])], 2, 2, (60, 50, 30, 20), 1);
}

#[test]
fn a_unit_between_root_guides_below_and_above_matches() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((33, 27), (140, 130));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0), a root guide (1), B (2), the guide (3) clipped to B, a
    // raster (4), a root guide (5).
    let doc = with_guide(&backdrop(cw, ch), 4.0, (0, 0));
    let doc = with_raster(
        &doc,
        holes(size.0, size.1, 3),
        offset,
        BlendMode::Screen,
        0.9,
    );
    let doc = with_member_guide(&doc, 8.0, (40, 30));
    let doc = with_raster(&doc, holes(60, 50, 4), (70, 70), BlendMode::SoftLight, 0.8);
    let doc = with_guide(&doc, 16.0, (0, 0));
    assert_regions_match(&doc, rect, "a unit between root guides");
    // The unit's hold is entered by loop 0 (over R_1), charged over R_0.
    for (window, d) in [((90, 80, 20, 15), 1), ((0, 0, cw, ch), 3)] {
        assert_root_plan(
            &doc,
            &[(1, 4.0, 2, &[]), (3, 8.0, 3, &[]), (5, 16.0, 1, &[])],
            1,
            1,
            window,
            d,
        );
    }
}

#[test]
fn a_unit_above_d5s_background_matches() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // D5: the backdrop (0) is a Background and a Shallow member (2) of the
    // unit above it asks for a knockout; the guide (3).
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    let doc = with_member(&doc, holes(70, 60, 2), (60, 50), BlendMode::Normal, 0.9);
    let doc = with_member_guide(&doc, 8.0, (40, 40));
    let d5 = styled(&doc, 2, SHALLOW);
    assert_regions_match(&d5, rect, "D5 under a unit with a Shallow member");
    // A Deep knockout raster at the root between the Background and the
    // unit.
    let deep = with_raster(
        &backdrop(cw, ch),
        holes(60, 50, 7),
        (20, 20),
        BlendMode::Normal,
        0.9,
    );
    let deep = styled(&deep, 1, "{\"fill_opacity\":0.4,\"knockout\":\"deep\"}");
    let deep = with_raster(
        &deep,
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    let deep = with_member_guide(&deep, 8.0, (40, 40));
    assert_regions_match(&deep, rect, "D5, a Deep knockout below the unit");
    // The Background snapshot is one more plane over R_0.
    let window = (60, 50, 30, 20);
    let r0 = plane_in(8.0, window, (0, 0, cw, ch));
    let mut plain = deep.clone();
    plain.layers[1].style = None;
    assert_eq!(
        scratch(&plan(&deep, window, 1).unwrap()) - scratch(&plan(&plain, window, 1).unwrap()),
        area(r0) * 16
    );
}

#[test]
fn a_unit_inside_a_materialized_group_is_rendered_unstaged() {
    let (cw, ch) = (113, 101);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, (cw, ch));
    // M (3) = { B (1), guide (2) clipped } with an effect.
    let doc = unit(cw, ch, 8.0, offset, size);
    let (doc, m) = isolate(&doc, &[1, 2]);
    let m_doc = styled(&doc, m, GROUP_STYLE);
    assert_regions_match(&m_doc, rect, "a member guide inside M");
    let p = plan(&m_doc, (30, 30, 20, 20), 1).unwrap();
    assert_eq!(p["stages"], json!([]));
    let (_, _, halo) = geometry(8.0);
    assert_eq!(p["layers"][2]["neighborhood"]["halo_pixels"], halo);
}

#[test]
fn a_guide_clipped_to_an_adjustment_base_contributes_nothing_and_is_staged() {
    let (cw, ch) = (113, 101);
    // Backdrop (0), a Levels adjustment (1), the guide (2) clipped to it.
    let mut doc = backdrop(cw, ch)
        .adding_image_layer(0, RgbaImage::new(1, 1), "levels")
        .expect("add layer");
    doc.layers[1].meta = Some(adjust_meta("levels", "{\"black\":0.1,\"gamma\":1.6}"));
    let doc = with_member_guide(&doc, 8.0, (20, 20));
    assert_regions_match(&doc, (0, 0, cw, ch), "a guide clipped to Levels");
    assert_root_plan(&doc, &[(2, 8.0, 2, &[])], 2, 1, (30, 20, 30, 20), 1);
}

#[test]
fn styled_bases_equal_the_reference_and_a_packaged_base_refuses() {
    let (cw, ch) = (113, 101);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, (cw, ch));
    let doc = unit(cw, ch, 8.0, offset, size);
    // Since 13A-4b-ii-b-2 a fill-only and a knockout base (Overlay at 0.85)
    // stage the member; effects at 0.85 are the PACKAGE, refused by name.
    for (what, style) in [
        ("fill-only", "{\"fill_opacity\":0.5}"),
        ("knockout", SHALLOW),
    ] {
        let d = styled(&doc, 1, style);
        assert!(d.layers[1].style.is_some(), "{what}");
        assert_regions_match(&d, rect, what);
    }
    let effects = styled(
        &doc,
        1,
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}",
    );
    assert_refuses(&effects, 2, "shadows_highlights", PACKAGED_BASE);
    // A materialized group base (opacity 1): G (2) = { A (1) } with an
    // effect; the guide (3) clipped to G.
    let base = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    let (base, g) = isolate(&base, &[1]);
    let base = styled(&base, g, GROUP_STYLE);
    let materialized = with_member_guide(&base, 8.0, (20, 20));
    assert_regions_match(&materialized, rect, "a materialized group base");
}

#[test]
fn linear_light_evaluates_a_document_with_a_member_guide_regionally() {
    let doc = unit(53, 50, 4.0, (7, 5), (30, 25));
    let linear = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(
        full(&linear),
        full(&doc),
        "the policy changes the projection"
    );
    assert_regions_match(&linear, (0, 0, 53, 50), "linear light");
}

// ------------------------------------------------ measured cancellation --
//
// DECLARED (before measuring): a cancel returns within 4096 samples of the
// running stage loop — clip-unit re-runs and hold fills included — or after
// the running byte `blur_plane` / `rendered_group` (uncancellable, 13A-5)
// completes. Each stage's per-sample estimate is calibrated on its OWN
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
            "13A-4b-ii-b-1 {depth} cancel during {label} (13A-5a model): {:.3} ms \
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

/// The 8-bit concurrent cancel: with `bench` the measured
/// wall-clock record (`make cancel-bench`); without it the
/// load-independent default, each stage cancelled with the worker PARKED
/// past its opening report (`cancelled_past_each`, the count from a fresh
/// twin), no poller.
fn cancel_clip_member_guided_render(bench: bool) {
    // Backdrop (0), a root radius-30 guide (1), a Multiply raster over most
    // of the canvas (2) and a radius-30 guide (3) CLIPPED to it, and a
    // SoftLight raster (4), over the whole 3000×2000 canvas. Cursors: [1],
    // then [2, 0] — the unit is entered (its began sample held over R_1) by
    // the loop after the root guide and re-run over the output.
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
            0.9,
        );
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
    let (_, g1) = guide_figures(30.0, r1);
    let pixels = f64::from(w) * f64::from(h);
    // Progress (reported per row): stage 0's `prev` fill (`f0 = |R_0|`, per
    // 4096-sample chunk, 13A-5b-iii-a; no D5), stage 0 (the backdrop over
    // R_0), guide 1, the hold's fill (per 4096-sample chunk, 13A-5a), loop 0
    // (guide 1 and the base over R_1, entering the unit), guide 2, the final
    // loop (the unit re-run — base and guide — and the SoftLight raster over
    // the output). Each probe passes one row (or chunk) after its value.
    let (a0, a1) = (area(r0) as f64, area(r1) as f64);
    let f0 = a0;
    let c = 4096.0;
    let (w0, w1) = (f64::from(r0.2), f64::from(r1.2));
    let (g0, g1) = (g0 as f64, g1 as f64);
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
        "8-bit",
        &[
            // 13A-5b-iii-a: stage 0's `prev` fill reports per 4096-sample
            // chunk from 0 to f0; opens at its first chunk, closes at stage
            // 0's open (the hold-fill row's shape).
            ("the stage-0 fill", c / work, Some((f0 + w0) / work), f0 - c),
            (
                "stage 0",
                (f0 + w0) / work,
                Some((f0 + a0 - w0) / work),
                a0 - 2.0 * w0,
            ),
            // 13A-5a: the fill reports per 4096-sample chunk from guide 1's
            // report (f0 + a0 + g0) to (f0 + a0 + g0 + a1); opens at its first
            // chunk, closes at loop 0's first row: the fill's samples (its
            // first chunk short, one row of loop 0 beside).
            (
                "the unit's hold fill",
                (f0 + a0 + g0) / work,
                Some((f0 + a0 + g0 + a1) / work),
                a1,
            ),
            (
                "the entering stage (loop 0)",
                (f0 + a0 + g0 + a1 + 2.0 * w1) / work,
                Some((f0 + a0 + g0 + a1 + 2.0 * (a1 - w1)) / work),
                a1 - 2.0 * w1,
            ),
            (
                "the root loop (the resumed unit)",
                (f0 + a0 + g0 + 3.0 * a1 + g1 + 3.0 * ww) / work,
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
fn a_concurrently_cancelled_clip_member_guided_render_returns_operation_cancelled() {
    cancel_clip_member_guided_render(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_clip_member_guided_render_is_measured() {
    cancel_clip_member_guided_render(true);
}

/// The u16 concurrent cancel: with `bench` the measured
/// wall-clock record (`make cancel-bench`); without it the
/// load-independent default, each stage cancelled with the worker PARKED
/// past its opening report (`cancelled_past_each`, the count from a fresh
/// twin), no poller.
fn cancel_clip_member_guided_render_at_u16(bench: bool) {
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
        let layers = vec![
            PrecisionDocumentLayer::raster("Base", image(w, h, 1, false), (0, 0), 1.0, true)
                .unwrap(),
            PrecisionDocumentLayer::adjustment("Root guide", &op, 0.8, true, &profile).unwrap(),
            PrecisionDocumentLayer::raster(
                "Mid",
                image(w - 43, h - 37, 3, true),
                (21, 19),
                0.9,
                true,
            )
            .unwrap()
            .with_blend_mode(BlendMode::Multiply)
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
    // Progress (reported per 4096-sample chunk): `P_0`'s fill, stage 0 (the
    // base over the canvas), guide 1, the hold's fill, loop 0 (guide 1 and
    // Mid over the canvas, entering the unit), guide 2, the output's fill,
    // the final loop (the unit re-run — Mid and the guide — and Top over the
    // output). Each probe passes one chunk after its value.
    let g = gw as f64;
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
        "u16",
        &[
            (
                "stage 0",
                pixels / work,
                Some((2.0 * pixels - c) / work),
                pixels - c,
            ),
            (
                "the unit's hold fill",
                (2.0 * pixels + g) / work,
                Some((3.0 * pixels + g - c) / work),
                pixels - c,
            ),
            (
                "the entering stage (loop 0)",
                (3.0 * pixels + g) / work,
                Some((5.0 * pixels + g - 2.0 * c) / work),
                pixels - c,
            ),
            (
                "the root loop (the resumed unit)",
                (6.0 * pixels + 2.0 * g) / work,
                None,
                pixels - c,
            ),
        ],
        twin,
        render,
    );
}

#[test]
fn a_concurrently_cancelled_clip_member_guided_render_returns_operation_cancelled_at_u16() {
    cancel_clip_member_guided_render_at_u16(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_clip_member_guided_render_is_measured_at_u16() {
    cancel_clip_member_guided_render_at_u16(true);
}
