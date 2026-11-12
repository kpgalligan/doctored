//! Phase 13A-4a: the 8-bit regional document render over a guide-reading
//! Shadows/Highlights at the ROOT — staged over the planes the plan declares —
//! against the full-frame projection. The oracle is INDEPENDENT of the
//! regional code: `dr_doc_flattened`'s projection cropped and top-left-cell
//! sampled here in plain index arithmetic, byte for byte (tolerance: EXACT,
//! declared before measuring). The plan figures are restated here from the
//! guide's own sizing rule, written out independently. The two
//! concurrent-cancel measurements (8 bits and u16) live here too, as
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

/// The 13A-2 windows (edges, corners, 1×1, the whole canvas) plus windows
/// whose guide plane is a STRICT sub-window of a canvas three halos wide.
fn windows(cw: u32, ch: u32) -> Vec<Window> {
    vec![
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
    ]
}

fn assert_regions_match(doc: &DrDocument, what: &str) {
    let reference = full(doc);
    let (cw, ch) = (doc.width, doc.height);
    for window in windows(cw, ch) {
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

/// A plasma backdrop with transparent HOLES — small ones on a lattice and
/// one large block (the guide's coverage-0 fallback) — and a band of
/// translucent pixels.
fn backdrop(cw: u32, ch: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(cw, ch, |x, y| {
        let c = plasma(x, y);
        let block = x > cw / 5 && x < cw / 5 + cw / 3 && y > ch / 2 && y < ch / 2 + ch / 3;
        let hole = block || (x / 7 + y / 5) % 9 == 0;
        let alpha = if hole {
            0
        } else if (x + 2 * y) % 5 == 0 || y < ch / 8 {
            (50 + (x * 7 + y * 3) % 170) as u8
        } else {
            255
        };
        Rgba([c[0], c[1], c[2], alpha])
    }))
}

fn top_pixels(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 37 + y * 11) as u8,
            (200 - (x * 5) % 200) as u8,
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

fn sh_params(radius: f32) -> String {
    format!(
        "{{\"shadows\":{{\"amount\":0.7,\"tone\":0.9}},\"highlights\":{{\"amount\":0.35,\
         \"tone\":0.8}},\"radius\":{radius}}}"
    )
}

/// A guided Shadows/Highlights on top, over canvas-sized transparent pixels
/// (so a mask can cover the canvas), opacity 0.8.
fn with_guide(doc: &DrDocument, radius: f32) -> DrDocument {
    let pixels = RgbaImage::new(doc.width, doc.height);
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, pixels, "shadows_highlights")
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].meta = Some(adjust_meta("shadows_highlights", &sh_params(radius)));
    out.layers[top].opacity = 0.8;
    out
}

/// Backdrop, a translucent raster, then a guide of `radius`.
fn stack(cw: u32, ch: u32, radius: f32) -> DrDocument {
    let doc = with_raster(
        &backdrop(cw, ch),
        top_pixels(cw / 2, ch / 2),
        (cw as i32 / 4, ch as i32 / 5),
        BlendMode::Overlay,
        0.85,
    );
    with_guide(&doc, radius)
}

fn masked(doc: &DrDocument, idx: usize, feather: f32) -> DrDocument {
    let (cw, ch) = (doc.width, doc.height);
    let (mw, mh) = (cw * 2 / 3, ch * 2 / 3);
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
    let mut properties = MaskProperties::at((cw as i32 / 6, ch as i32 / 5));
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
// Written out independently of the core: sigma = radius / 2; the cell factor
// 1 up to sigma 3, then 2, 4, 8 at each doubling; the grid sigma is sigma at
// factor 1, else sqrt(sigma² − f²/3) / f; the blur's reach is the kernel's
// half width plus one (and, above sigma 3, two more resampling pixels, all
// scaled by the blur's own factor); two more cells for the bilinear read.

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

/// The plane over `inner`: grown by f·reach, aligned outward to f·8, clamped.
fn plane_region(radius: f32, inner: Window, canvas: (u32, u32)) -> Window {
    let (f, cells, _) = geometry(radius);
    let grow = u64::from(f * cells);
    let lattice = u64::from(f * 8);
    let x0 = u64::from(inner.0).saturating_sub(grow) / lattice * lattice;
    let y0 = u64::from(inner.1).saturating_sub(grow) / lattice * lattice;
    let x1 = ((u64::from(inner.0 + inner.2) + grow).div_ceil(lattice) * lattice)
        .min(u64::from(canvas.0));
    let y1 = ((u64::from(inner.1 + inner.3) + grow).div_ceil(lattice) * lattice)
        .min(u64::from(canvas.1));
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

fn area((_, _, w, h): Window) -> u64 {
    u64::from(w) * u64::from(h)
}

/// The guide's (scratch, work) over `plane`, from the shared typed planner
/// (`guide_plan_for_description`, the reference's own sizing).
fn guide_figures(radius: f32, plane: Window) -> (u64, u64) {
    let meta = json!({"type":"adjust","op":"shadows_highlights","evaluation":"precision_v1",
        "params":{"shadows":{"amount":0.7},"radius":radius}})
    .to_string();
    PrecisionAdjustment::guide_plan_for_description(&meta, plane.2, plane.3).unwrap()
}

fn within(inner: &Value, outer: &Value) -> bool {
    let n = |v: &Value, k: &str| v[k].as_u64().unwrap();
    n(inner, "x") >= n(outer, "x")
        && n(inner, "y") >= n(outer, "y")
        && n(inner, "x") + n(inner, "width") <= n(outer, "x") + n(outer, "width")
        && n(inner, "y") + n(inner, "height") <= n(outer, "y") + n(outer, "height")
}

fn assert_radius(cw: u32, ch: u32, radius: f32) {
    let (_, _, halo) = geometry(radius);
    let doc = stack(cw, ch, radius);
    let mut unguided = doc.clone();
    unguided.layers.truncate(2);
    assert_ne!(
        full(&doc),
        full(&unguided),
        "radius {radius}: the guide changes the projection"
    );
    assert_regions_match(&doc, &format!("radius {radius}"));
    // The interior window's plane is a STRICT sub-window unless the halo
    // exceeds the canvas (radius 1000: the plane is the canvas).
    let window = (cw / 3 + 1, ch / 3 + 1, cw / 3 - 2, ch / 3 - 2);
    let p = plan(&doc, window, 1).unwrap();
    let stage = &p["stages"][0];
    let plane = plane_region(radius, window, (cw, ch));
    assert_eq!(stage["plane"], region_json(plane), "radius {radius}");
    let strict = plane.0 > 0 && plane.1 > 0 && plane.0 + plane.2 < cw && plane.1 + plane.3 < ch;
    assert_eq!(strict, 3 * halo <= cw.min(ch), "radius {radius}: {plane:?}");
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_radius_4_guide_at_factor_1_matches_the_projection() {
    assert_eq!(geometry(4.0).0, 1);
    assert_radius(53, 50, 4.0);
}

#[test]
fn a_radius_8_guide_at_factor_2_matches_the_projection() {
    assert_eq!(geometry(8.0).0, 2);
    assert_radius(101, 98, 8.0);
}

#[test]
fn a_radius_16_guide_at_factor_4_matches_the_projection() {
    assert_eq!(geometry(16.0).0, 4);
    assert_radius(197, 195, 16.0);
}

#[test]
fn a_radius_30_guide_at_factor_8_matches_the_projection() {
    assert_eq!(geometry(30.0).0, 8);
    assert_radius(389, 387, 30.0);
}

#[test]
fn a_radius_120_guide_whose_grid_blur_reduces_again_matches_the_projection() {
    // The grid sigma exceeds 3, so the byte blur reduces the grid itself.
    let sigma = 60.0f32;
    assert!(reduced(sigma, 8) > 3.0);
    let (_, _, halo) = geometry(120.0);
    assert_radius(3 * halo + 5, 3 * halo + 3, 120.0);
}

#[test]
fn a_radius_1000_guide_whose_plane_is_the_canvas_matches_the_projection() {
    assert_radius(203, 157, 1000.0);
    let doc = stack(203, 157, 1000.0);
    let p = plan(&doc, (80, 60, 5, 5), 1).unwrap();
    assert_eq!(p["stages"][0]["plane"], region_json((0, 0, 203, 157)));
}

#[test]
fn a_masked_translucent_multiply_and_dissolve_guide_matches() {
    let doc = stack(101, 98, 8.0);
    for feather in [0.0, 6.0] {
        let m = masked(&doc, 2, feather);
        assert_regions_match(&m, &format!("masked feather {feather}"));
        let p = plan(&m, (40, 40, 10, 10), 1).unwrap();
        assert_eq!(p["layers"][2]["mask"], true);
        assert_eq!(p["layers"][2]["neighborhood"]["kind"], "spatial");
    }
    for (mode, opacity) in [
        (BlendMode::Normal, 0.7),
        (BlendMode::Multiply, 0.8),
        (BlendMode::Dissolve, 0.6),
    ] {
        let mut d = masked(&doc, 2, 6.0);
        d.layers[2].blend = mode;
        d.layers[2].opacity = opacity;
        assert_regions_match(&d, &format!("{mode:?} {opacity}"));
    }
}

#[test]
fn two_guides_with_a_raster_between_nest_their_planes() {
    let doc = stack(197, 195, 8.0);
    let doc = with_raster(
        &doc,
        top_pixels(70, 60),
        (40, 50),
        BlendMode::SoftLight,
        0.9,
    );
    let doc = with_guide(&doc, 16.0);
    assert_regions_match(&doc, "two guides");
    let window = (70, 70, 30, 20);
    let p = plan(&doc, window, 1).unwrap();
    let stages = p["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0]["layer"], 2);
    assert_eq!(stages[1]["layer"], 4);
    let inner = plane_region(16.0, window, (197, 195));
    let outer = plane_region(8.0, inner, (197, 195));
    assert_eq!(stages[1]["plane"], region_json(inner));
    assert_eq!(stages[0]["plane"], region_json(outer));
    assert!(within(&stages[1]["plane"], &stages[0]["plane"]));
    assert!(within(&stages[0]["plane"], &p["required_input_region"]));
}

#[test]
fn a_guide_above_a_knockout_background_matches() {
    // D5: entry 0 is a plain canvas-sized raster and a visible style asks
    // for a knockout, so the staged loop splits the Background in stage 0.
    let doc = with_raster(
        &backdrop(101, 98),
        top_pixels(60, 50),
        (20, 20),
        BlendMode::Normal,
        0.9,
    );
    let doc = styled(&doc, 1, "{\"fill_opacity\":0.4,\"knockout\":\"shallow\"}");
    let doc = with_guide(&doc, 8.0);
    assert_regions_match(&doc, "knockout Background + guide");
    let window = (30, 30, 20, 20);
    let without = {
        let mut d = doc.clone();
        d.layers[1].style = None;
        plan(&d, window, 1).unwrap()
    };
    let with = plan(&doc, window, 1).unwrap();
    // The Background snapshot `S` is one more plane over R_0.
    let r0 = plane_region(8.0, window, (101, 98));
    assert_eq!(
        with["scratch_heap_bytes"].as_u64().unwrap()
            - without["scratch_heap_bytes"].as_u64().unwrap(),
        area(r0) * 16
    );
}

#[test]
fn a_baseless_clipped_guide_at_the_root_matches() {
    let mut doc = stack(101, 98, 8.0);
    doc.layers[0].clipped = true;
    doc.layers[1].clipped = true;
    doc.layers[2].clipped = true;
    assert_regions_match(&doc, "baseless clipped guide");
    let p = plan(&doc, (10, 10, 20, 20), 1).unwrap();
    assert_eq!(p["stages"][0]["layer"], 2);
    assert_eq!(p["layers"][2]["clip_base"], Value::Null);
}

#[test]
fn a_guided_base_over_a_non_empty_run_contributes_nothing() {
    let doc = stack(101, 98, 8.0);
    for hidden in [false, true] {
        let mut d = with_raster(&doc, top_pixels(50, 40), (10, 10), BlendMode::Normal, 1.0);
        d.layers[3].clipped = true;
        d.layers[3].visible = !hidden;
        // The private buffer stays transparent: the whole unit contributes
        // nothing (a hidden member still makes the run non-empty at 8 bits).
        let mut without = d.clone();
        without.layers.truncate(2);
        assert_eq!(full(&d), full(&without), "hidden {hidden}");
        assert_regions_match(&d, &format!("guided base over a run, hidden {hidden}"));
        let p = plan(&d, (10, 10, 20, 20), 1).unwrap();
        assert_eq!(p["stages"], json!([]));
        assert_eq!(p["layers"][2]["neighborhood"]["kind"], "pointwise");
    }
}

#[test]
fn an_empty_isolated_group_below_a_guide_keeps_the_guides_layer_index() {
    // An EMPTY isolated (Normal) group has no extent, so `resolve` drops its
    // unit and the guide's resolved position is one less than its walk
    // position: the stage must name the LAYER index.
    let base = with_raster(
        &backdrop(101, 98),
        top_pixels(50, 40),
        (20, 20),
        BlendMode::Multiply,
        0.8,
    );
    // No children.
    let mut empty = base.clone();
    let mut entry = base.layers[0].clone();
    entry.kind = LayerKind::Group;
    entry.name = "Empty".into();
    entry.pixels = LayerRaster::new(RgbaImage::new(1, 1));
    entry.blend = BlendMode::Normal;
    entry.opacity = 1.0;
    entry.visible = true;
    entry.mask = None;
    entry.meta = None;
    entry.style = None;
    entry.clipped = false;
    entry.depth = 0;
    empty.layers.push(entry);
    let empty = with_guide(&empty, 8.0);
    assert_regions_match(&empty, "empty group, no children");
    let p = plan(&empty, (30, 30, 10, 10), 1).unwrap();
    assert_eq!(p["stages"][0]["layer"], 3);
    // Hidden-only children.
    let hidden = with_raster(&base, top_pixels(30, 30), (5, 5), BlendMode::Normal, 1.0);
    let (mut hidden, g, ..) = hidden.group_layers(&[2], "Hidden kids").expect("group");
    hidden.layers[2].visible = false;
    hidden.layers[g].blend = BlendMode::Normal;
    let hidden = with_guide(&hidden, 8.0);
    assert_regions_match(&hidden, "empty group, hidden children");
    let p = plan(&hidden, (30, 30, 10, 10), 1).unwrap();
    assert_eq!(p["stages"][0]["layer"], 4);
}

#[test]
fn guided_entries_inside_a_group_and_as_a_member_match() {
    let doc = stack(53, 50, 4.0);
    // A default `group_layers` group is Pass Through: since 13A-4b-ii-a the
    // guide directly inside it is staged by the root level.
    let (grouped, ..) = doc.group_layers(&[2], "Group").expect("group");
    assert_regions_match(&grouped, "a guide inside a Pass Through group");
    // Since 13A-4b-ii-b-1 a guided clip member of an unstyled base is staged
    // too (the unit re-run from its held began sample).
    let mut member = doc.clone();
    member.layers[2].clipped = true;
    assert_regions_match(&member, "a guide clipped to a plain raster");
}

#[test]
fn a_zero_amount_shadows_highlights_is_pointwise() {
    let mut doc = stack(53, 50, 4.0);
    doc.layers[2].meta = Some(adjust_meta(
        "shadows_highlights",
        "{\"shadows\":{\"amount\":0},\"highlights\":{\"amount\":0},\"midtone_contrast\":0.3,\"radius\":12}",
    ));
    assert_regions_match(&doc, "zero amounts");
    let p = plan(&doc, (5, 5, 10, 10), 1).unwrap();
    assert_eq!(p["stages"], json!([]));
    assert_eq!(p["layers"][2]["neighborhood"]["kind"], "pointwise");
    assert_eq!(p["neighborhood"]["halo_pixels"], 0);
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

#[test]
fn the_plan_restates_stages_border_policy_halo_scratch_and_work() {
    let (cw, ch) = (101, 98);
    let doc = stack(cw, ch, 8.0);
    let (f, cells, halo) = geometry(8.0);
    for (window, d) in [
        ((40, 40, 13, 9), 2),
        ((0, 0, cw, ch), 1),
        ((90, 3, 11, 20), 3),
    ] {
        let p = plan(&doc, window, d).unwrap();
        let r0 = plane_region(8.0, window, (cw, ch));
        assert_eq!(
            p["stages"],
            json!([{"level": null, "buffer": region_json((0, 0, cw, ch)), "layer": 2,
                "plane": region_json(r0), "cell_pixels": f, "reach_cells": cells,
                "through": []}])
        );
        assert_eq!(p["border_policy"], "canvas_clamped");
        assert_eq!(
            p["neighborhood"],
            json!({"kind":"spatial","halo_pixels":halo})
        );
        assert_eq!(
            p["layers"][2]["neighborhood"],
            json!({"kind":"spatial","halo_pixels":halo})
        );
        assert_eq!(p["required_input_region"], grown(window, halo, (cw, ch)));
        assert!(within(
            &p["stages"][0]["plane"],
            &p["required_input_region"]
        ));
        let pixels = u64::from(window.2.div_ceil(d)) * u64::from(window.3.div_ceil(d));
        let (gs, gw) = guide_figures(8.0, r0);
        assert_eq!(
            p["scratch_heap_bytes"].as_u64().unwrap(),
            pixels * 16 + pixels * 4 + area(r0) * 16 + gs + description_parse(&doc),
            "{window:?}"
        );
        // Stage 0 (two rows) over R_0, stage 1 (one row) over the output,
        // the guide, the output fill and stage 0's `prev` fill over R_0
        // (13A-5b-iii-a; no D5 here).
        assert_eq!(
            p["work_units"].as_u64().unwrap(),
            area(r0) * 2 + pixels + gw + pixels + area(r0),
            "{window:?}"
        );
    }
    // Without a guide the figures are 13A-3c's.
    let mut plain = doc.clone();
    plain.layers.truncate(2);
    let p = plan(&plain, (40, 40, 13, 9), 2).unwrap();
    assert_eq!(p["stages"], json!([]));
    assert_eq!(p["border_policy"], "canvas_clamped");
    assert_eq!(p["work_units"], 7 * 5 * 3);
}

#[test]
fn the_input_halo_composes_each_rows_halo_with_the_guides_above_it() {
    let (cw, ch) = (197, 195);
    let window = (80, 80, 20, 20);
    let feathered = |doc: &DrDocument, idx: usize| {
        let mut out = doc.clone();
        let plane = GrayImage::from_fn(90, 80, |x, y| Luma([((x * 3 + y * 5) % 256) as u8]));
        let mut properties = MaskProperties::at((30, 40));
        properties.feather = 5.0;
        let layer = &mut out.layers[idx];
        layer.mask = Some(Arc::new(plane));
        layer.mask_enabled = true;
        layer.mask_properties = properties;
        out
    };
    let pad_of = |p: &Value, row: usize| {
        p["layers"][row]["neighborhood"]["halo_pixels"]
            .as_u64()
            .unwrap() as u32
    };
    let (_, _, h4) = geometry(4.0);
    let (_, _, h8) = geometry(8.0);
    // A feather-P raster below a radius-4 guide: P + H.
    let doc = feathered(&stack(cw, ch, 4.0), 1);
    let p = plan(&doc, window, 1).unwrap();
    let pad = pad_of(&p, 1);
    assert!(pad > 0);
    assert_eq!(p["neighborhood"]["halo_pixels"], pad + h4);
    assert_eq!(
        p["required_input_region"],
        grown(window, pad + h4, (cw, ch))
    );
    assert_regions_match(&doc, "feathered raster below a guide");
    // A guided row with its own feather-P mask: max(P, H).
    let doc = feathered(&stack(cw, ch, 4.0), 2);
    let p = plan(&doc, window, 1).unwrap();
    let own = pad_of(&p, 2);
    assert_eq!(own, h4.max(pad));
    assert_eq!(p["neighborhood"]["halo_pixels"], own);
    // Two guides with a feather-P raster between: max(P + H_2, H_1 + H_2).
    let two = with_raster(
        &stack(cw, ch, 4.0),
        top_pixels(70, 60),
        (40, 50),
        BlendMode::Screen,
        0.9,
    );
    let two = with_guide(&feathered(&two, 3), 8.0);
    let p = plan(&two, window, 1).unwrap();
    assert_eq!(p["neighborhood"]["halo_pixels"], (pad + h8).max(h4 + h8));
    assert_regions_match(&two, "two guides, a feathered raster between");
    // A pure guide pair: H_1 + H_2.
    let pair = with_guide(&stack(cw, ch, 4.0), 8.0);
    let p = plan(&pair, window, 1).unwrap();
    assert_eq!(p["neighborhood"]["halo_pixels"], h4 + h8);
    assert_eq!(p["required_input_region"], grown(window, h4 + h8, (cw, ch)));
}

#[test]
fn linear_light_evaluates_a_guided_document_regionally() {
    let doc = stack(53, 50, 4.0);
    let linear = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(
        full(&linear),
        full(&doc),
        "the policy changes the projection"
    );
    assert_regions_match(&linear, "linear light");
}

#[test]
fn a_pre_cancelled_guided_render_holds_no_scratch() {
    let doc = stack(101, 98, 8.0);
    let hd = handle(&doc);
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_render_region(hd, 0, 0, 101, 98, 1, u64::MAX, control, &mut err) };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), "operation cancelled");
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe {
        dr_job_control_free(control);
        dr_doc_free(hd);
    }
}

// ------------------------------------------------ measured cancellation --
//
// DECLARED (before measuring): a cancel returns within 4096 samples of the
// running stage loop, or after the running byte `blur_plane` (uncancellable,
// 13A-5) completes.

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
            assert!(Instant::now() < deadline, "the stage never started");
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

/// Where stage 0 and the final stage begin, as progress fractions (the u16
/// twin's shape since 13A-5b-iii-a): stage 0 once `P_0`'s fill is advanced
/// (`|R_0| / work`); the final stage once that fill, stage 0's rows over
/// R_0 and the guide are done, plus half a row of the final stage.
fn stage_starts(p: &Value, w: u32) -> (f64, f64) {
    let work = p["work_units"].as_u64().unwrap() as f64;
    let plane = &p["stages"][0]["plane"];
    let (pw, ph) = (
        plane["width"].as_u64().unwrap() as u32,
        plane["height"].as_u64().unwrap() as u32,
    );
    let (_, gw) = guide_figures(30.0, (0, 0, pw, ph));
    let a = area((0, 0, pw, ph)) as f64;
    (a / work, (a * 3.0 + gw as f64 + f64::from(w)) / work)
}

/// The 8-bit concurrent cancel: with `bench` the measured
/// wall-clock record (`make cancel-bench`); without it the
/// load-independent default, each stage cancelled with the worker PARKED
/// past its opening report (`cancelled_past_each`, the count from a fresh
/// twin), no poller.
fn cancel_guided_render(bench: bool) {
    // Background + raster (stage 0), a radius-30 guide + a SoftLight raster
    // (the final stage), over the whole 3000×2000 canvas.
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
            BlendMode::Multiply,
            0.9,
        );
        let doc = with_guide(&doc, 30.0);
        with_raster(
            &doc,
            top_pixels(w / 2, h / 2),
            (100, 100),
            BlendMode::SoftLight,
            0.8,
        )
    };
    let doc = build();
    let p = plan(&doc, (0, 0, w, h), 1).unwrap();
    assert_eq!(p["stages"].as_array().unwrap().len(), 1);
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
    let (stage_0, final_stage) = stage_starts(&p, w);
    if bench {
        let control = unsafe { dr_job_control_create() };
        let looped = timed(control, stage_0, false, render);
        unsafe { dr_job_control_free(control) };
        // Stage-0 plane samples plus output samples.
        let pixels = f64::from(w) * f64::from(h);
        // The final stage's samples are dearer than the render's average, so its
        // estimate is calibrated on its OWN uncancelled loop: from the same probe
        // point to the return, over the output (the u16 twin's calibration).
        let control = unsafe { dr_job_control_create() };
        let final_looped = timed(control, final_stage, false, render);
        unsafe { dr_job_control_free(control) };
        for (label, start, looped, per_sample) in [
            (
                "stage 0",
                stage_0,
                looped,
                looped.as_secs_f64() / (2.0 * pixels),
            ),
            (
                "final stage",
                final_stage,
                final_looped,
                final_looped.as_secs_f64() / pixels,
            ),
        ] {
            let control = unsafe { dr_job_control_create() };
            let latency = timed(control, start, true, render);
            eprintln!(
                "13A-4a 8-bit cancel during {label}: {:.3} ms cancel-to-return; {:.2} ns per \
                 staged sample (loop {:.1} ms); ≈{:.0} samples after the cancel (declared \
                 bound 4096 samples of the running stage loop)",
                latency.as_secs_f64() * 1e3,
                per_sample * 1e9,
                looped.as_secs_f64() * 1e3,
                latency.as_secs_f64() / per_sample,
            );
            assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
            unsafe { dr_job_control_free(control) };
        }
    } else {
        let twin = |token: usize| {
            let twin = handle(&build());
            let out = render_at(twin as usize)(token);
            unsafe { dr_doc_free(twin) };
            out
        };
        let parked = common::probe::cancelled_past_each(&[stage_0, final_stage], twin, twin);
        eprintln!(
            "8-bit cancel past stage 0's open: {}",
            common::probe::parked_line(&parked[0])
        );
        eprintln!(
            "8-bit cancel past the final stage's open: {}",
            common::probe::parked_line(&parked[1])
        );
    }
    unsafe { dr_doc_free(hd) };
}

#[test]
fn a_concurrently_cancelled_guided_render_returns_operation_cancelled() {
    cancel_guided_render(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_guided_render_is_measured() {
    cancel_guided_render(true);
}

/// The u16 concurrent cancel: with `bench` the measured
/// wall-clock record (`make cancel-bench`); without it the
/// load-independent default, each stage cancelled with the worker PARKED
/// past its opening report (`cancelled_past_each`, the count from a fresh
/// twin), no poller.
fn cancel_guided_render_at_u16(bench: bool) {
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
        let op = json!({"type":"adjust","op":"shadows_highlights","evaluation":"precision_v1",
        "params":{"shadows":{"amount":0.7,"tone":0.45},"highlights":{"amount":0.35},"radius":30}})
        .to_string();
        let layers = vec![
            PrecisionDocumentLayer::raster("Base", image(w, h, 1, false), (0, 0), 1.0, true)
                .unwrap(),
            PrecisionDocumentLayer::raster(
                "Mid",
                image(w - 40, h - 40, 3, true),
                (20, 20),
                0.9,
                true,
            )
            .unwrap()
            .with_blend_mode(BlendMode::Multiply)
            .unwrap(),
            PrecisionDocumentLayer::adjustment("Guide", &op, 0.8, true, &profile).unwrap(),
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
    assert_eq!(p["stages"].as_array().unwrap().len(), 1);
    // Stage 0 begins once `P_0`'s fill is advanced; the final stage once
    // stage 0 (two rows), the guide and the output fill are.
    let work = p["work_units"].as_u64().unwrap() as f64;
    let pixels = f64::from(w) * f64::from(h);
    let (_, gw) = guide_figures(30.0, (0, 0, w, h));
    let stage_0 = pixels / work;
    let final_stage = (pixels + pixels * 2.0 + gw as f64 + pixels + 4096.0) / work;
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
        let looped = timed(control, stage_0, false, render);
        unsafe { dr_job_control_free(control) };
        // The final stage's samples are dearer than the render's average, so its
        // estimate is calibrated on its OWN uncancelled loop: from the same probe
        // point to the return, over the canvas.
        let control = unsafe { dr_job_control_create() };
        let final_looped = timed(control, final_stage, false, render);
        unsafe { dr_job_control_free(control) };
        for (label, start, looped, per_sample) in [
            (
                "stage 0",
                stage_0,
                looped,
                looped.as_secs_f64() / (2.0 * pixels),
            ),
            (
                "final stage",
                final_stage,
                final_looped,
                final_looped.as_secs_f64() / pixels,
            ),
        ] {
            let control = unsafe { dr_job_control_create() };
            let latency = timed(control, start, true, render);
            eprintln!(
                "13A-4a u16 cancel during {label}: {:.3} ms cancel-to-return; {:.2} ns per \
                 staged sample (loop {:.1} ms); ≈{:.0} samples after the cancel (declared \
                 bound 4096 samples of the running stage loop)",
                latency.as_secs_f64() * 1e3,
                per_sample * 1e9,
                looped.as_secs_f64() * 1e3,
                latency.as_secs_f64() / per_sample,
            );
            assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
            unsafe { dr_job_control_free(control) };
        }
    } else {
        let twin = |token: usize| {
            let twin = build();
            render_at(&twin as *const PrecisionDocument as usize)(token)
        };
        let parked = common::probe::cancelled_past_each(&[stage_0, final_stage], twin, twin);
        eprintln!(
            "u16 cancel past stage 0's open: {}",
            common::probe::parked_line(&parked[0])
        );
        eprintln!(
            "u16 cancel past the final stage's open: {}",
            common::probe::parked_line(&parked[1])
        );
    }
}

#[test]
fn a_concurrently_cancelled_guided_render_returns_operation_cancelled_at_u16() {
    cancel_guided_render_at_u16(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_guided_render_is_measured_at_u16() {
    cancel_guided_render_at_u16(true);
}
