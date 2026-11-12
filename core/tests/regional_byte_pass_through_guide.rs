//! Phase 13A-4b-ii-a: the 8-bit regional document render over a guide-reading
//! Shadows/Highlights directly inside PASS-THROUGH groups (plain or gated by
//! opacity / mask, nested, at the root or inside an isolated group), staged
//! through the ENCLOSING level, against the full-frame projection. The oracle
//! is INDEPENDENT of the regional code: `dr_doc_flattened`'s projection
//! cropped and top-left-cell sampled here in plain index arithmetic, byte for
//! byte (tolerance: EXACT, declared before measuring). The plan figures are
//! restated from the guide's own sizing rule anchored at the enclosing
//! level's buffer (the canvas at the root, an isolated group's rect below
//! it), written out independently, with one held plane over `R_0` per
//! distinct HELD pass-through group (gated, or a Shallow child reads the
//! sample as its children began). The two concurrent-cancel measurements (8
//! bits and u16) live here, as `#[ignore]`d `…_is_measured` twins,
//! so `make cancel-bench` records both depths; their default twins
//! cancel with the worker parked.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::{BlendMode, DrDocument, LayerKind};
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

/// The feather-6 blur work [`masked`] adds to the byte plan (13A-5b-iii-b-2),
/// restated: sigma 2.3 → factor 1 → `feather_radius` 6.0 → 13 taps and a
/// pad of 7, so the padded plane `(mw + 14)·(mh + 14)` at 32 + 2·13 per
/// pixel (outside coverage 0).
fn feather6_work(rect: Window) -> u64 {
    let (mw, mh) = (u64::from(rect.2 * 2 / 3), u64::from(rect.3 * 2 / 3));
    (mw + 14) * (mh + 14) * (32 + 2 * 13)
}

/// Backdrop (0), then a Pass Through group (3) holding a holed Overlay raster
/// A (1) at `offset` of `size` and a guide of `radius` (2) inside A.
fn passed(cw: u32, ch: u32, radius: f32, offset: (i32, i32), size: (u32, u32)) -> DrDocument {
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    let doc = with_guide(&doc, radius, (offset.0.max(0) + 2, offset.1.max(0) + 3));
    let (doc, p) = pass(&doc, &[1, 2]);
    assert_eq!(p, 3);
    doc
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

/// The root Pass Through document's plan (`passed`), restated for `window`
/// at `d`: the root's level (null, the canvas) stages the guide (2) through
/// the group (3), its halo, the input region containing the plane, and the
/// scratch and work — one more plane over `R_0` and its fill iff `held`.
fn assert_pass_plan(doc: &DrDocument, radius: f32, held: bool, window: Window, d: u32) {
    let (cw, ch) = (doc.width, doc.height);
    let canvas = (0, 0, cw, ch);
    let p = plan(doc, window, d).unwrap();
    let (_, _, halo) = geometry(radius);
    let pixels = u64::from(window.2.div_ceil(d)) * u64::from(window.3.div_ceil(d));
    let r0 = plane_in(radius, window, canvas);
    assert_eq!(
        p["stages"],
        json!([stage(Value::Null, canvas, 2, radius, r0, &[3])]),
        "{window:?}"
    );
    assert_eq!(p["neighborhood"]["halo_pixels"], halo, "{window:?}");
    assert_eq!(p["layers"][2]["neighborhood"]["halo_pixels"], halo);
    assert_eq!(p["layers"][2]["group"], 3);
    assert_eq!(p["required_input_region"], grown(window, halo, (cw, ch)));
    assert!(within(r0, window_of(&p["required_input_region"])));
    let (gs, gw) = guide_figures(radius, r0);
    let hold = u64::from(held) * area(r0);
    // The accumulator and the output, P_0 and the guide, and the hold.
    assert_eq!(
        scratch(&p),
        pixels * 16 + pixels * 4 + area(r0) * 16 + gs + hold * 16 + description_parse(doc),
        "{window:?}"
    );
    // Backdrop and A over R_0, the guide and the group over the output, the
    // guide, the accumulator fill, the hold's fill, and stage 0's `prev`
    // fill over R_0 (13A-5b-iii-a; no D5 here).
    assert_eq!(
        work(&p),
        area(r0) * 2 + pixels * 2 + gw + pixels + hold + area(r0),
        "{window:?}"
    );
}

fn assert_pass_radius(cw: u32, ch: u32, radius: f32) {
    let offset = (37, 29);
    let size = (cw - 80, ch - 70);
    let rect = rect_of(offset, size, (cw, ch));
    let doc = passed(cw, ch, radius, offset, size);
    let mut unguided = doc.clone();
    unguided.layers[2].visible = false;
    assert_ne!(
        full(&doc),
        full(&unguided),
        "radius {radius}: the guide changes the projection"
    );
    assert_regions_match(&doc, rect, &format!("radius {radius}, plain"));
    let plans = [
        ((cw / 3 + 1, ch / 3 + 1, cw / 3 - 2, ch / 3 - 2), 1),
        ((0, 0, cw, ch), 2),
        ((rect.0 - 4, rect.1 - 3, 9, 7), 1),
        ((cw - 3, ch - 3, 3, 3), 3),
    ];
    for (window, d) in plans {
        assert_pass_plan(&doc, radius, false, window, d);
    }
    // Gated by opacity: the one guide's cursor enters the group in P_0 and
    // the hold is read by the FINISH in the last loop, over the output.
    let mut gated = doc.clone();
    gated.layers[3].opacity = 0.6;
    assert_regions_match(&gated, rect, &format!("radius {radius}, opacity 0.6"));
    for (window, d) in plans {
        assert_pass_plan(&gated, radius, true, window, d);
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_plain_and_a_gated_root_pass_through_stage_a_radius_4_guide() {
    assert_pass_radius(113, 101, 4.0);
}

#[test]
fn a_plain_and_a_gated_root_pass_through_stage_a_radius_30_guide() {
    assert_pass_radius(389, 387, 30.0);
}

#[test]
fn a_plain_and_a_gated_root_pass_through_stage_a_radius_120_guide() {
    let (_, _, halo) = geometry(120.0);
    assert_pass_radius(3 * halo + 5, 3 * halo + 3, 120.0);
}

#[test]
fn a_pass_through_gated_by_a_feathered_mask_and_by_both_matches() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    let doc = passed(cw, ch, 8.0, offset, size);
    let m = masked(&doc, 3, rect, 6.0);
    assert_regions_match(&m, rect, "a feather-6 mask");
    let mut both = m.clone();
    both.layers[3].opacity = 0.6;
    assert_regions_match(&both, rect, "a feather-6 mask at opacity 0.6");
    // A mask disabled is no gate: nothing held.
    let mut off = m.clone();
    off.layers[3].mask_enabled = false;
    assert_regions_match(&off, rect, "a disabled mask");
    assert_pass_plan(&off, 8.0, false, (60, 50, 30, 20), 1);
    // The same mask charged on a layer off the path (the backdrop): the
    // mask's derived plane is then the only other scratch difference.
    let mut elsewhere = off.clone();
    elsewhere.layers[0].mask = m.layers[3].mask.clone();
    elsewhere.layers[0].mask_enabled = true;
    elsewhere.layers[0].mask_properties = m.layers[3].mask_properties.clone();
    let canvas = (0, 0, cw, ch);
    for (window, div) in [((60, 50, 30, 20), 1), ((0, 0, cw, ch), 3)] {
        let r0 = plane_in(8.0, window, canvas);
        for d in [&m, &both] {
            let p = plan(d, window, div).unwrap();
            assert_eq!(
                p["stages"],
                json!([stage(Value::Null, canvas, 2, 8.0, r0, &[3])])
            );
            // One hold over R_0 and its fill, and — 13A-5b-iii-b-2 — the
            // enabled mask's feather blur.
            let q = plan(&elsewhere, window, div).unwrap();
            assert_eq!(scratch(&p) - scratch(&q), area(r0) * 16, "{window:?}");
            let q = plan(&off, window, div).unwrap();
            assert_eq!(
                work(&p) - work(&q),
                area(r0) + feather6_work(rect),
                "{window:?}"
            );
        }
    }
}

#[test]
fn a_shallow_knockout_child_beside_the_guide_holds_the_plain_group_sample() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    let mut doc = passed(cw, ch, 8.0, offset, size);
    // Not a Background (D5 is its own case below).
    doc.layers[0].opacity = 0.97;
    let knockout = styled(&doc, 1, SHALLOW);
    assert_ne!(full(&knockout), full(&doc), "the knockout punches");
    assert_regions_match(&knockout, rect, "a Shallow child beside the guide");
    let window = (60, 50, 30, 20);
    let (a, b) = (
        plan(&knockout, window, 1).unwrap(),
        plan(&doc, window, 1).unwrap(),
    );
    let r0 = plane_in(8.0, window, (0, 0, cw, ch));
    assert_eq!(scratch(&a) - scratch(&b), area(r0) * 16);
    assert_eq!(work(&a) - work(&b), area(r0));
    assert_eq!(a["stages"][0]["through"], json!([3]));
    // Another pass-through above holding a Shallow raster (unstaged: no
    // guide on its path).
    let above = with_raster(&doc, holes(60, 50, 4), (60, 60), BlendMode::Normal, 0.9);
    let (above, _) = pass(&above, &[4]);
    let above = styled(&above, 4, SHALLOW);
    assert_regions_match(&above, rect, "a Shallow raster in a pass-through above");
}

#[test]
fn nested_pass_throughs_hold_each_held_level() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); Outer (5) = { Inner (3) = { A (1), guide (2) }, B (4) }.
    let doc = passed(cw, ch, 8.0, offset, size);
    let doc = with_raster(&doc, holes(70, 60, 2), (60, 70), BlendMode::Multiply, 0.85);
    let (doc, outer) = pass(&doc, &[3, 4]);
    assert_eq!(outer, 5);
    let window = (60, 50, 30, 20);
    let r0 = plane_in(8.0, window, (0, 0, cw, ch));
    let base = plan(&doc, window, 1).unwrap();
    assert_eq!(base["stages"][0]["through"], json!([5, 3]));
    assert_regions_match(&doc, rect, "plain in plain");
    for (inner_opacity, outer_opacity, holds) in [(1.0, 0.6, 1), (0.6, 1.0, 1), (0.7, 0.6, 2)] {
        let mut d = doc.clone();
        d.layers[3].opacity = inner_opacity;
        d.layers[5].opacity = outer_opacity;
        let what = format!("inner {inner_opacity} in outer {outer_opacity}");
        assert_regions_match(&d, rect, &what);
        let p = plan(&d, window, 1).unwrap();
        assert_eq!(p["stages"][0]["through"], json!([5, 3]), "{what}");
        // One plane over R_0 per HELD group; both entered in P_0.
        assert_eq!(
            scratch(&p) - scratch(&base),
            holds * area(r0) * 16,
            "{what}"
        );
        assert_eq!(work(&p) - work(&base), holds * area(r0), "{what}");
    }
}

#[test]
fn a_pass_through_inside_an_isolated_group_stages_the_groups_level() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((101, 97), (80, 70));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); G (4) = { P (3) = { A (1), guide (2) } }.
    let doc = passed(cw, ch, 8.0, offset, size);
    let (mut doc, g) = isolate(&doc, &[3]);
    assert_eq!(g, 4);
    doc.layers[3].opacity = 0.6;
    assert_regions_match(&doc, rect, "a gated pass-through in an isolated group");
    let window = (120, 110, 30, 20);
    let p = plan(&doc, window, 1).unwrap();
    let r1 = meet(window, rect).unwrap();
    let r0 = plane_in(8.0, r1, rect);
    assert_eq!(
        p["stages"],
        json!([stage(json!(4), rect, 2, 8.0, r0, &[3])])
    );
    // A window missing the rect stages nothing.
    assert_eq!(plan(&doc, (0, 0, 20, 20), 1).unwrap()["stages"], json!([]));
    // Under a ROOT guide (5): the root's level, then the group's, whose
    // target is the root's R_0 within the rect; a window whose root plane
    // misses the rect stages the root only.
    let rooted = with_guide(&doc, 4.0, (0, 0));
    assert_regions_match(&rooted, rect, "under a root guide");
    let p = plan(&rooted, window, 1).unwrap();
    let canvas = (0, 0, cw, ch);
    let root_r0 = plane_in(4.0, window, canvas);
    let g_r1 = meet(root_r0, rect).unwrap();
    assert_eq!(
        p["stages"],
        json!([
            stage(Value::Null, canvas, 5, 4.0, root_r0, &[]),
            stage(json!(4), rect, 2, 8.0, plane_in(8.0, g_r1, rect), &[3]),
        ])
    );
    let p = plan(&rooted, (0, 0, 20, 20), 1).unwrap();
    assert_eq!(p["stages"].as_array().unwrap().len(), 1);
    assert_eq!(p["stages"][0]["level"], Value::Null);
}

#[test]
fn an_isolated_group_whose_only_guide_is_inside_a_pass_through_child_is_staged() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((33, 27), (140, 130));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); G (5) = { P (3) = { A (1), guide (2) }, B (4) }.
    let doc = passed(cw, ch, 16.0, offset, size);
    let doc = with_raster(&doc, holes(60, 50, 3), (70, 70), BlendMode::Screen, 0.8);
    let (doc, g) = isolate(&doc, &[3, 4]);
    assert_eq!(g, 5);
    assert_regions_match(
        &doc,
        rect,
        "a group whose guide is in its pass-through child",
    );
    let window = (80, 70, 30, 20);
    let p = plan(&doc, window, 1).unwrap();
    let r1 = meet(window, rect).unwrap();
    let r0 = plane_in(16.0, r1, rect);
    assert_eq!(
        p["stages"],
        json!([stage(json!(5), rect, 2, 16.0, r0, &[3])])
    );
    let mut gated = doc.clone();
    gated.layers[3].opacity = 0.6;
    assert_regions_match(&gated, rect, "the same, gated");
}

#[test]
fn an_isolated_guided_group_inside_a_pass_through_stages_two_levels_in_pre_order() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((33, 27), (140, 130));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); P (5) = { G (3) = { A (1), guide (2) }, guide2 (4) }.
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 5),
        offset,
        BlendMode::Overlay,
        0.9,
    );
    let doc = with_guide(&doc, 8.0, (40, 40));
    let (doc, g) = isolate(&doc, &[1, 2]);
    assert_eq!(g, 3);
    let doc = with_guide(&doc, 16.0, (0, 0));
    let (mut doc, p_idx) = pass(&doc, &[3, 4]);
    assert_eq!(p_idx, 5);
    assert_regions_match(&doc, rect, "a guided group in a plain pass-through");
    doc.layers[5].opacity = 0.6;
    assert_regions_match(&doc, rect, "a guided group in a gated pass-through");
    let window = (80, 70, 30, 20);
    let p = plan(&doc, window, 1).unwrap();
    let canvas = (0, 0, cw, ch);
    let root_r0 = plane_in(16.0, window, canvas);
    let g_r1 = meet(root_r0, rect).unwrap();
    assert_eq!(
        p["stages"],
        json!([
            stage(Value::Null, canvas, 4, 16.0, root_r0, &[5]),
            stage(json!(3), rect, 2, 8.0, plane_in(8.0, g_r1, rect), &[]),
        ])
    );
}

#[test]
fn two_guides_in_one_gated_pass_through_resume_one_hold() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((23, 19), (160, 150));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); P (5) = { A (1), guide8 (2), B (3), guide16 (4) }.
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(size.0, size.1, 5),
        offset,
        BlendMode::Normal,
        0.95,
    );
    let doc = with_guide(&doc, 8.0, (30, 30));
    let doc = with_raster(&doc, holes(70, 60, 6), (60, 70), BlendMode::Overlay, 0.9);
    let doc = with_guide(&doc, 16.0, (40, 40));
    let (plain, p_idx) = pass(&doc, &[1, 2, 3, 4]);
    assert_eq!(p_idx, 5);
    assert_regions_match(&plain, rect, "two guides in a plain pass-through");
    let mut gated = plain.clone();
    gated.layers[5].opacity = 0.6;
    assert_regions_match(&gated, rect, "two guides in a gated pass-through");
    let gated = masked(&gated, 5, rect, 6.0);
    assert_regions_match(&gated, rect, "two guides, gated and masked");
    let window = (80, 70, 30, 20);
    let canvas = (0, 0, cw, ch);
    let r1 = plane_in(16.0, window, canvas);
    let r0 = plane_in(8.0, r1, canvas);
    let (p, q) = (
        plan(&gated, window, 1).unwrap(),
        plan(&plain, window, 1).unwrap(),
    );
    assert_eq!(
        p["stages"],
        json!([
            stage(Value::Null, canvas, 2, 8.0, r0, &[5]),
            stage(Value::Null, canvas, 4, 16.0, r1, &[5]),
        ])
    );
    // ONE hold: entered before P_0, resumed by both later loops. (The mask's
    // derived plane is the rest of the difference.)
    let mask = {
        let unmasked = {
            let mut d = plain.clone();
            d.layers[5].opacity = 0.6;
            d
        };
        scratch(&p) - scratch(&plan(&unmasked, window, 1).unwrap())
    };
    assert_eq!(scratch(&p) - scratch(&q), area(r0) * 16 + mask);
    // The hold's fill and — 13A-5b-iii-b-2 — the mask's feather blur.
    assert_eq!(work(&p) - work(&q), area(r0) + feather6_work(rect));
}

#[test]
fn a_pass_through_guide_between_root_guides_below_and_above_matches() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((33, 27), (140, 130));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0), a root guide (1), P (4) = { A (2), guide (3) }, a raster
    // (5), a root guide (6).
    let doc = with_guide(&backdrop(cw, ch), 4.0, (0, 0));
    let doc = with_raster(
        &doc,
        holes(size.0, size.1, 3),
        offset,
        BlendMode::Screen,
        0.9,
    );
    let doc = with_guide(&doc, 8.0, (40, 30));
    let (doc, p_idx) = pass(&doc, &[2, 3]);
    assert_eq!(p_idx, 4);
    let doc = with_raster(&doc, holes(60, 50, 4), (70, 70), BlendMode::SoftLight, 0.8);
    let doc = with_guide(&doc, 16.0, (0, 0));
    assert_regions_match(&doc, rect, "plain, between root guides");
    let mut gated = doc.clone();
    gated.layers[4].opacity = 0.6;
    // The hold is entered by loop 0 (over R_1), not by P_0.
    assert_regions_match(&gated, rect, "gated, between root guides");
    let window = (90, 80, 20, 15);
    let canvas = (0, 0, cw, ch);
    let r2 = plane_in(16.0, window, canvas);
    let r1 = plane_in(8.0, r2, canvas);
    let r0 = plane_in(4.0, r1, canvas);
    let p = plan(&gated, window, 1).unwrap();
    assert_eq!(
        p["stages"],
        json!([
            stage(Value::Null, canvas, 1, 4.0, r0, &[]),
            stage(Value::Null, canvas, 3, 8.0, r1, &[4]),
            stage(Value::Null, canvas, 6, 16.0, r2, &[]),
        ])
    );
    // Conservatively charged over R_0.
    let q = plan(&doc, window, 1).unwrap();
    assert_eq!(scratch(&p) - scratch(&q), area(r0) * 16);
}

#[test]
fn a_knockout_background_under_a_guided_pass_through_matches() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // D5: the backdrop is a Background and a Shallow child of the gated
    // pass-through asks for a knockout.
    let doc = passed(cw, ch, 8.0, offset, size);
    let inside = styled(&doc, 1, SHALLOW);
    assert_regions_match(&inside, rect, "D5, a Shallow child beside the guide");
    let mut gated = inside.clone();
    gated.layers[3].opacity = 0.6;
    assert_regions_match(&gated, rect, "D5, gated");
    // A knockout at the root below the pass-through (1), the group (4).
    let root = with_raster(
        &backdrop(cw, ch),
        holes(60, 50, 7),
        (20, 20),
        BlendMode::Normal,
        0.9,
    );
    let root = styled(&root, 1, "{\"fill_opacity\":0.4,\"knockout\":\"deep\"}");
    let root = with_raster(
        &root,
        holes(size.0, size.1, 1),
        offset,
        BlendMode::Overlay,
        0.85,
    );
    let root = with_guide(&root, 8.0, (40, 40));
    let (mut root, p_idx) = pass(&root, &[2, 3]);
    assert_eq!(p_idx, 4);
    assert_regions_match(&root, rect, "D5, a Deep knockout below");
    root.layers[4].opacity = 0.6;
    assert_regions_match(&root, rect, "D5, a Deep knockout below, gated");
    // The Background snapshot is one more plane over R_0.
    let window = (60, 50, 30, 20);
    let r0 = plane_in(8.0, window, (0, 0, cw, ch));
    let mut plain = root.clone();
    plain.layers[1].style = None;
    assert_eq!(
        scratch(&plan(&root, window, 1).unwrap()) - scratch(&plan(&plain, window, 1).unwrap()),
        area(r0) * 16
    );
}

#[test]
fn a_pass_through_clip_base_absorbs_hidden_members_and_isolates_under_visible_ones() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0), P (3) = { A (1), guide (2) }, a member (4) clipped to P.
    let doc = passed(cw, ch, 8.0, offset, size);
    let mut doc = with_raster(&doc, holes(80, 60, 7), (50, 60), BlendMode::Normal, 0.9);
    doc.layers[4].clipped = true;
    let mut hidden = doc.clone();
    hidden.layers[4].visible = false;
    assert_regions_match(&hidden, rect, "hidden-only members: pass-through");
    let window = (60, 50, 30, 20);
    let p = plan(&hidden, window, 1).unwrap();
    assert_eq!(p["stages"][0]["level"], Value::Null);
    assert_eq!(p["stages"][0]["through"], json!([3]));
    // A VISIBLE member isolates the base: the group's own level stages it.
    assert_regions_match(&doc, rect, "a visible member: isolated");
    let p = plan(&doc, window, 1).unwrap();
    let r1 = meet(window, rect).unwrap();
    assert_eq!(
        p["stages"],
        json!([stage(json!(3), rect, 2, 8.0, plane_in(8.0, r1, rect), &[])])
    );
}

#[test]
fn a_hidden_or_zero_opacity_pass_through_is_skipped() {
    let (cw, ch) = (113, 101);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, (cw, ch));
    let doc = passed(cw, ch, 8.0, offset, size);
    for (visible, opacity) in [(false, 1.0), (true, 0.0)] {
        let mut d = doc.clone();
        d.layers[3].visible = visible;
        d.layers[3].opacity = opacity;
        let what = format!("visible {visible} opacity {opacity}");
        assert_regions_match(&d, rect, &what);
        assert_eq!(
            plan(&d, (20, 20, 20, 20), 1).unwrap()["stages"],
            json!([]),
            "{what}"
        );
    }
}

#[test]
fn a_guided_clip_member_inside_a_pass_through_matches() {
    let (cw, ch) = (53, 50);
    let rect = rect_of((7, 5), (30, 25), (cw, ch));
    let doc = with_raster(
        &backdrop(cw, ch),
        holes(30, 25, 1),
        (7, 5),
        BlendMode::Normal,
        1.0,
    );
    let mut member = with_guide(&doc, 4.0, (8, 6));
    member.layers[2].clipped = true;
    // Since 13A-4b-ii-b-1 staged by the root level through the group.
    let (plain, p_idx) = pass(&member, &[1, 2]);
    assert_regions_match(&plain, rect, "a guided clip member in a plain pass-through");
    let mut gated = plain.clone();
    gated.layers[p_idx].opacity = 0.6;
    assert_regions_match(&gated, rect, "a guided clip member in a gated pass-through");
}

#[test]
fn a_pass_through_guide_inside_a_materialized_group_is_rendered_unstaged() {
    let (cw, ch) = (113, 101);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, (cw, ch));
    // M (4) = { P (3) = { A (1), guide (2) } } with an effect.
    let doc = passed(cw, ch, 8.0, offset, size);
    let (doc, m) = isolate(&doc, &[3]);
    let m_doc = styled(&doc, m, GROUP_STYLE);
    assert_regions_match(&m_doc, rect, "a pass-through guide inside M");
    let p = plan(&m_doc, (30, 30, 20, 20), 1).unwrap();
    assert_eq!(p["stages"], json!([]));
    let (_, _, halo) = geometry(8.0);
    assert_eq!(p["layers"][2]["neighborhood"]["halo_pixels"], halo);
}

#[test]
fn linear_light_evaluates_a_document_with_a_pass_through_guide_regionally() {
    let doc = passed(53, 50, 4.0, (7, 5), (30, 25));
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
// running stage loop — nested and pass-through paths and hold fills
// included — or after the running byte `blur_plane` / `rendered_group`
// (uncancellable, 13A-5) completes. Each stage's per-sample estimate is
// calibrated on its OWN uncancelled loop: the time between the progress
// point that opens it and the one that opens the next, over its samples.

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
            "13A-4b-ii-a {depth} cancel during {label} (13A-5a model): {:.3} ms \
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
fn cancel_pass_through_guided_render(bench: bool) {
    // Backdrop (0), a root radius-30 guide (1), a GATED Pass Through group
    // P (4) = { a Multiply raster over most of the canvas (2), a radius-30
    // guide (3) } and a SoftLight raster (5), over the whole 3000×2000
    // canvas. Cursors: [1], then [2, 1] — P is entered (its hold filled over
    // R_1) by the loop after the root guide and finished over the output.
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
        let doc = with_guide(&doc, 30.0, (30, 30));
        let (mut doc, p_idx) = pass(&doc, &[2, 3]);
        assert_eq!(p_idx, 4);
        doc.layers[4].opacity = 0.6;
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
    assert_eq!(p["stages"][1]["through"], json!([4]));
    let work = work(&p) as f64;
    let r0 = window_of(&p["stages"][0]["plane"]);
    let r1 = window_of(&p["stages"][1]["plane"]);
    let (_, g0) = guide_figures(30.0, r0);
    let (_, g1) = guide_figures(30.0, r1);
    let pixels = f64::from(w) * f64::from(h);
    // Progress (reported per row): stage 0's `prev` fill (`f0 = |R_0|`, per
    // 4096-sample chunk, 13A-5b-iii-a; no D5), stage 0 (the backdrop over
    // R_0), guide 1, the hold's fill (per 4096-sample chunk, 13A-5a), loop 0
    // (guide 1 and A over R_1, entering P), guide 2, the final loop (guide 2,
    // P, the SoftLight raster over the output). Each probe passes one row (or
    // chunk) after its value.
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
                "the hold fill",
                (f0 + a0 + g0) / work,
                Some((f0 + a0 + g0 + a1) / work),
                a1,
            ),
            (
                "the held gated stage (loop 0)",
                (f0 + a0 + g0 + a1 + 2.0 * w1) / work,
                Some((f0 + a0 + g0 + a1 + 2.0 * (a1 - w1)) / work),
                a1 - 2.0 * w1,
            ),
            (
                "the root loop (the final stage)",
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
fn a_concurrently_cancelled_pass_through_guided_render_returns_operation_cancelled() {
    cancel_pass_through_guided_render(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_pass_through_guided_render_is_measured() {
    cancel_pass_through_guided_render(true);
}

/// The u16 concurrent cancel: with `bench` the measured
/// wall-clock record (`make cancel-bench`); without it the
/// load-independent default, each stage cancelled with the worker PARKED
/// past its opening report (`cancelled_past_each`, the count from a fresh
/// twin), no poller.
fn cancel_pass_through_guided_render_at_u16(bench: bool) {
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
        let children = vec![
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
            PrecisionDocumentLayer::adjustment("Guide", &op, 0.8, true, &profile).unwrap(),
        ];
        let layers = vec![
            PrecisionDocumentLayer::raster("Base", image(w, h, 1, false), (0, 0), 1.0, true)
                .unwrap(),
            PrecisionDocumentLayer::adjustment("Root guide", &op, 0.8, true, &profile).unwrap(),
            PrecisionDocumentLayer::pass_through("Pass", children, 0.6, true).unwrap(),
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
    assert_eq!(p["stages"][1]["through"], json!([4]));
    let work = work(&p) as f64;
    let pixels = f64::from(w) * f64::from(h);
    let (_, gw) = guide_figures(30.0, (0, 0, w, h));
    // Progress (reported per 4096-sample chunk): `P_0`'s fill, stage 0 (the
    // base over the canvas), guide 1, the hold's fill, loop 0 (guide 1 and
    // Mid over the canvas, entering the group), guide 2, the output's fill,
    // the final loop (guide 2, the group and Top over the output). Each probe
    // passes one chunk after its value.
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
                "the hold fill",
                (2.0 * pixels + g) / work,
                Some((3.0 * pixels + g - c) / work),
                pixels - c,
            ),
            (
                "the held gated stage (loop 0)",
                (3.0 * pixels + g) / work,
                Some((5.0 * pixels + g - 2.0 * c) / work),
                pixels - c,
            ),
            (
                "the root loop (the final stage)",
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
fn a_concurrently_cancelled_pass_through_guided_render_returns_operation_cancelled_at_u16() {
    cancel_pass_through_guided_render_at_u16(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_pass_through_guided_render_is_measured_at_u16() {
    cancel_pass_through_guided_render_at_u16(true);
}
