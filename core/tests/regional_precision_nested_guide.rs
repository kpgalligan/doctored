//! Phase 13A-4b-i: the 16-bit regional render over a guide-reading
//! Shadows/Highlights inside ISOLATED groups (nested, at any unit position)
//! and inside MATERIALIZED subtrees, against the full-frame typed projection.
//! The oracle is INDEPENDENT of the regional code: `dr_precision_doc_render`'s
//! projection read back as u16 codes, cropped and top-left-cell sampled here
//! in plain index arithmetic (tolerance: EXACT, declared before measuring).
//! Every typed accumulator is canvas-sized, so a nested level's buffer — and
//! the lattice its planes are aligned to — is the CANVAS (restated here
//! independently). The u16 concurrent-cancel measurement over a guided group
//! lives in `regional_byte_nested_guide.rs` beside the 8-bit one.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use crate::region_plan;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const BOOKKEEPING: u64 = 1024 * 1024;

const BLEND_IF_GRAY: &str =
    "\"blend_if\":{\"channel\":\"gray\",\"this_layer\":[10,60,180,240],\"underlying\":[0,40,200,255]}";

const GROUP_STYLE: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                           {\"type\":\"stroke\",\"size\":3}]}";

type Window = (u32, u32, u32, u32);
type Samples = (u32, u32, Vec<u32>);

// ------------------------------------------------------------- plumbing --

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(p) };
    text
}

fn samples(image: *mut PrecisionImage) -> Samples {
    let (w, h) = unsafe {
        (
            dr_precision_image_width(image),
            dr_precision_image_height(image),
        )
    };
    let n = (w * h * 4) as usize;
    let p16 = unsafe { dr_precision_image_pixels_rgba16(image) };
    assert!(!p16.is_null(), "a u16 image");
    let out = unsafe { std::slice::from_raw_parts(p16, n) }
        .iter()
        .map(|&v| u32::from(v))
        .collect();
    unsafe { dr_precision_image_free(image) };
    (w, h, out)
}

fn full(doc: &PrecisionDocument) -> Samples {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "render: {}", take(err));
    samples(image)
}

fn region(doc: &PrecisionDocument, (x, y, w, h): Window, d: u32) -> Result<Samples, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_render_region(doc, x, y, w, h, d, LIMIT, 0, ptr::null(), &mut err)
    };
    if image.is_null() {
        return Err(take(err));
    }
    Ok(samples(image))
}

fn plan(doc: &PrecisionDocument, (x, y, w, h): Window, d: u32) -> Result<Value, String> {
    region_plan::typed(doc, (x, y, w, h), d)
}

/// The independent oracle: sample (x + i·d, y + j·d) of the full render.
fn expected((cw, _, reference): &Samples, (x, y, w, h): Window, d: u32) -> Samples {
    let ow = w.div_ceil(d);
    let oh = h.div_ceil(d);
    let mut out = Vec::new();
    for j in 0..oh {
        for i in 0..ow {
            let cx = u64::from(x) + u64::from(i) * u64::from(d);
            let cy = u64::from(y) + u64::from(j) * u64::from(d);
            let at = ((cy * u64::from(*cw) + cx) * 4) as usize;
            out.extend_from_slice(&reference[at..at + 4]);
        }
    }
    (ow, oh, out)
}

/// 13A-4a's windows plus, for the group's content `rect`, windows
/// straddling each of its corners and windows wholly outside it.
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

fn assert_regions_match(doc: &PrecisionDocument, rect: Window, what: &str) {
    let reference = full(doc);
    let (cw, ch) = doc.dimensions();
    for window in windows(cw, ch, rect) {
        for d in [1, 2, 3, 7, u32::MAX] {
            let got =
                region(doc, window, d).unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, window, d),
                "{what}: window {window:?} divisor {d}"
            );
        }
    }
}

// ------------------------------------------------------------ fixtures --

fn profile() -> Arc<IccProfile> {
    IccProfile::srgb()
}

/// A canvas-sized backdrop with transparent holes and translucent pixels.
fn backdrop(cw: u32, ch: u32) -> Layer {
    let pixels = (0..cw * ch)
        .map(|i| {
            let (x, y) = (i % cw, i / cw);
            let a = if (x / 7 + y / 5).is_multiple_of(9) {
                0
            } else if (x + 2 * y).is_multiple_of(5) || y < ch / 8 {
                (9000 + (x * 3001 + y * 1777) % 50000) as u16
            } else {
                65535
            };
            [
                ((x * 2711 + y * 911 + 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + 77) % 65536) as u16,
                (((x ^ y) * 1999) % 65536) as u16,
                a,
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(cw, ch, pixels, profile(), None).unwrap();
    Layer::raster("Backdrop", image, (0, 0), 1.0, true).unwrap()
}

/// A translucent image with transparent HOLES (a lattice and one block).
fn holes(w: u32, h: u32, seed: u32) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let block = x > w / 5 && x < w / 5 + w / 3 && y > h / 2 && y < h / 2 + h / 3;
            let hole = block || (x / 5 + y / 7 + seed).is_multiple_of(8);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                ((x ^ y) * 1999 % 65536 + seed) as u16,
                if hole {
                    0
                } else {
                    (9000 + (x * 3001 + y * 1777 + seed * 97) % 56000) as u16
                },
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, profile(), None).unwrap()
}

fn raster(size: (u32, u32), offset: (i32, i32), seed: u32, mode: BlendMode, opacity: f32) -> Layer {
    Layer::raster("Raster", holes(size.0, size.1, seed), offset, opacity, true)
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
}

fn guide(radius: f32) -> Layer {
    let params = json!({"shadows":{"amount":0.7,"tone":0.9},
        "highlights":{"amount":0.35,"tone":0.8},"radius":radius});
    let op = json!({"type":"adjust","op":"shadows_highlights","params":params,
        "evaluation":"precision_v1"});
    Layer::adjustment("shadows_highlights", &op.to_string(), 0.8, true, &profile()).unwrap()
}

fn isolated(children: Vec<Layer>) -> Layer {
    Layer::isolated_normal("Group", children, 1.0, true).unwrap()
}

fn styled(layer: Layer, json: &str, canvas: (u32, u32)) -> Layer {
    let style = LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"));
    layer.with_layer_style(Some(&style), canvas).unwrap()
}

fn doc(cw: u32, ch: u32, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        cw,
        ch,
        profile(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn mask(rect: Window, feather: f32) -> PrecisionMask {
    let (mw, mh) = (rect.2 * 2 / 3, rect.3 * 2 / 3);
    let plane = (0..mw * mh)
        .map(|i| {
            let (x, y) = (i % mw, i / mw);
            let dx = x as f32 / mw as f32 - 0.5;
            let dy = y as f32 / mh as f32 - 0.5;
            if dx * dx + dy * dy < 0.2 {
                (30000 + (x * 3 + y) * 97 % 35535) as u16
            } else {
                0
            }
        })
        .collect();
    let mut properties = MaskProperties::at((rect.0 as i32 + 5, rect.1 as i32 + 3));
    properties.density = 0.9;
    properties.feather = feather;
    PrecisionMask::new(
        PrecisionPlane::from_u16(mw, mh, plane, None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}

/// The content rect of a raster at `offset` of `size`, within the canvas.
fn rect_of(offset: (i32, i32), size: (u32, u32), canvas: (u32, u32)) -> Window {
    let x0 = offset.0.max(0) as u32;
    let y0 = offset.1.max(0) as u32;
    let x1 = ((offset.0 + size.0 as i32).max(0) as u32).min(canvas.0);
    let y1 = ((offset.1 + size.1 as i32).max(0) as u32).min(canvas.1);
    (x0, y0, x1 - x0, y1 - y0)
}

/// Backdrop (0), then an isolated group (3) = { a holed Overlay raster (1)
/// at `offset` of `size`, a guide of `radius` (2) }.
fn grouped(cw: u32, ch: u32, radius: f32, offset: (i32, i32), size: (u32, u32)) -> Vec<Layer> {
    vec![
        backdrop(cw, ch),
        isolated(vec![
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            guide(radius),
        ]),
    ]
}

// ------------------------------------------- the guide's own sizing rule --

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

/// The plane over `inner`: grown by f·reach, aligned outward to f·8 from the
/// CANVAS origin (a typed group's buffer is the canvas), clamped.
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

fn window_of(v: &Value) -> Window {
    let n = |k: &str| v[k].as_u64().unwrap() as u32;
    (n("x"), n("y"), n("width"), n("height"))
}

fn area((_, _, w, h): Window) -> u64 {
    u64::from(w) * u64::from(h)
}

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

fn stage(level: Value, canvas: (u32, u32), layer: usize, radius: f32, plane: Window) -> Value {
    let (f, cells, _) = geometry(radius);
    json!({"level": level, "buffer": region_json((0, 0, canvas.0, canvas.1)), "layer": layer,
        "plane": region_json(plane), "cell_pixels": f, "reach_cells": cells, "through": []})
}

/// The lone-guided-group document's plan restated for `window` at `d`: the
/// group's level is staged over the window whatever the content rect (its
/// buffer is the canvas), with its figures.
fn assert_group_plan(d: &PrecisionDocument, radius: f32, window: Window, div: u32) {
    let canvas = d.dimensions();
    let p = plan(d, window, div).unwrap();
    let (_, _, halo) = geometry(radius);
    let pixels = u64::from(window.2.div_ceil(div)) * u64::from(window.3.div_ceil(div));
    let r1 = window;
    let r0 = plane_region(radius, r1, canvas);
    assert_eq!(
        p["stages"],
        json!([stage(json!(3), canvas, 2, radius, r0)]),
        "{window:?}"
    );
    assert_eq!(p["neighborhood"]["halo_pixels"], halo);
    assert_eq!(p["required_input_region"], grown(window, halo, canvas));
    assert!(within(r0, window_of(&p["required_input_region"])));
    let (gs, gw) = guide_figures(radius, r0);
    assert_eq!(
        p["scratch_heap_bytes"].as_u64().unwrap(),
        pixels * 16 + pixels * 8 + BOOKKEEPING + (area(r0) + area(r1)) * 16 + gs,
        "{window:?}"
    );
    // Four rows over the output, its fill, `P_0`'s fill, A over R_0, the
    // guide over the held R_1, the guide.
    assert_eq!(
        p["work_units"].as_u64().unwrap(),
        pixels * 4 + pixels + area(r0) + area(r0) + area(r1) + gw,
        "{window:?}"
    );
}

fn assert_group_radius(cw: u32, ch: u32, radius: f32) {
    let offset = (37, 29);
    let size = (cw - 80, ch - 70);
    let rect = rect_of(offset, size, (cw, ch));
    assert!(rect.0 + rect.2 < cw && rect.1 + rect.3 < ch);
    let layers = grouped(cw, ch, radius, offset, size);
    let d = doc(cw, ch, layers.clone());
    let unguided = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            isolated(vec![raster(size, offset, 1, BlendMode::Overlay, 0.85)]),
        ],
    );
    assert_ne!(
        full(&d),
        full(&unguided),
        "radius {radius}: the guide changes the projection"
    );
    assert_regions_match(&d, rect, &format!("radius {radius}"));
    for (window, div) in [
        ((cw / 3 + 1, ch / 3 + 1, cw / 3 - 2, ch / 3 - 2), 1),
        ((0, 0, cw, ch), 2),
        ((rect.0 - 4, rect.1 - 3, 9, 7), 1),
        ((0, 0, rect.0, 20), 1),
        ((cw - 3, ch - 3, 3, 3), 3),
    ] {
        assert_group_plan(&d, radius, window, div);
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn an_isolated_group_inside_the_canvas_stages_a_radius_4_guide_at_u16() {
    assert_group_radius(113, 101, 4.0);
}

#[test]
fn an_isolated_group_inside_the_canvas_stages_a_radius_30_guide_at_u16() {
    assert_group_radius(389, 387, 30.0);
}

#[test]
fn an_isolated_group_inside_the_canvas_stages_a_radius_120_guide_at_u16() {
    let (_, _, halo) = geometry(120.0);
    assert_group_radius(3 * halo + 5, 3 * halo + 3, 120.0);
}

#[test]
fn a_group_touching_the_canvas_edge_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((-9, 61), (150, 160));
    let rect = rect_of(offset, size, (cw, ch));
    let d = doc(cw, ch, grouped(cw, ch, 8.0, offset, size));
    assert_regions_match(&d, rect, "a group at the canvas edge");
    for window in [(0, 150, 40, 45), (100, 60, 97, 20), (5, 5, 30, 30)] {
        assert_group_plan(&d, 8.0, window, 1);
    }
}

/// Backdrop (0); G1 (6) = { A (1), G2 (4) = { B (2), guide2 (3) },
/// guide1 (5) } — G2 BELOW G1's guide, so G2 is evaluated over G1's R_0.
fn nested(cw: u32, ch: u32, guide1_visible: bool) -> Vec<Layer> {
    let mut guide1 = guide(16.0);
    if !guide1_visible {
        guide1 = Layer::adjustment(
            "shadows_highlights",
            &json!({"type":"adjust","op":"shadows_highlights","evaluation":"precision_v1",
                "params":{"shadows":{"amount":0.7,"tone":0.9},"radius":16}})
            .to_string(),
            0.8,
            false,
            &profile(),
        )
        .unwrap();
    }
    vec![
        backdrop(cw, ch),
        isolated(vec![
            raster((150, 140), (21, 17), 1, BlendMode::Normal, 0.9),
            isolated(vec![
                raster((90, 70), (51, 43), 2, BlendMode::Multiply, 0.85),
                guide(8.0),
            ]),
            guide1,
        ]),
    ]
}

#[test]
fn nested_isolated_groups_each_with_a_guide_match_with_and_without_a_root_guide_at_u16() {
    let (cw, ch) = (197, 195);
    let canvas = (cw, ch);
    let rect2 = rect_of((51, 43), (90, 70), canvas);
    let d = doc(cw, ch, nested(cw, ch, true));
    assert_regions_match(&d, rect2, "nested guides, no root guide");
    let window = (70, 60, 30, 25);
    let p = plan(&d, window, 1).unwrap();
    let g1_r0 = plane_region(16.0, window, canvas);
    let g2_r0 = plane_region(8.0, g1_r0, canvas);
    assert_eq!(
        p["stages"],
        json!([
            stage(json!(6), canvas, 5, 16.0, g1_r0),
            stage(json!(4), canvas, 3, 8.0, g2_r0),
        ])
    );
    let (_, _, h8) = geometry(8.0);
    let (_, _, h16) = geometry(16.0);
    assert_eq!(p["neighborhood"]["halo_pixels"], h8 + h16);
    for plane in [g1_r0, g2_r0] {
        assert!(within(plane, window_of(&p["required_input_region"])));
    }
    let pixels = area(window);
    let (gs1, gw1) = guide_figures(16.0, g1_r0);
    let (gs2, gw2) = guide_figures(8.0, g2_r0);
    // Seven rows over the output and its fill; one `P_0` fill per staged
    // level; G1: rows [1, 5) over its R_0 and [5, 6) over its R_1 (the
    // window); G2: rows [2, 3) over its R_0 and [3, 4) over its R_1 (G1's
    // R_0).
    assert_eq!(
        p["work_units"].as_u64().unwrap(),
        pixels * 7
            + pixels
            + area(g1_r0)
            + area(g2_r0)
            + area(g1_r0) * 4
            + area(window)
            + gw1
            + area(g2_r0)
            + area(g1_r0)
            + gw2
    );
    assert_eq!(
        p["scratch_heap_bytes"].as_u64().unwrap(),
        pixels * 24
            + BOOKKEEPING
            + (area(g1_r0) + area(window)) * 16
            + gs1
            + (area(g2_r0) + area(g1_r0)) * 16
            + gs2
    );
    // With a ROOT guide on top: the root's level first, then G1 and G2.
    let mut rooted = nested(cw, ch, true);
    rooted.push(guide(4.0));
    let rooted = doc(cw, ch, rooted);
    assert_regions_match(&rooted, rect2, "nested guides under a root guide");
    let p = plan(&rooted, window, 1).unwrap();
    let levels: Vec<&Value> = p["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| &s["level"])
        .collect();
    assert_eq!(levels, [&Value::Null, &json!(6), &json!(4)]);
    assert_eq!(p["stages"][0]["layer"], 7);
    let root_r0 = plane_region(4.0, window, canvas);
    assert_eq!(
        p["stages"][1]["plane"],
        region_json(plane_region(16.0, root_r0, canvas))
    );
}

#[test]
fn an_unguided_isolated_group_holding_a_guided_one_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let rect2 = rect_of((51, 43), (90, 70), (cw, ch));
    let d = doc(cw, ch, nested(cw, ch, false));
    assert_regions_match(&d, rect2, "an unguided group over a guided one");
    let window = (60, 50, 40, 30);
    let p = plan(&d, window, 1).unwrap();
    assert_eq!(
        p["stages"],
        json!([stage(
            json!(4),
            (cw, ch),
            3,
            8.0,
            plane_region(8.0, window, (cw, ch))
        )])
    );
}

#[test]
fn a_guide_in_a_group_with_root_guides_below_and_above_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let canvas = (cw, ch);
    let (offset, size) = ((33, 27), (140, 130));
    let rect = rect_of(offset, size, canvas);
    // Backdrop (0), a root guide (1), G (4) = { A (2), guide (3) }, a raster
    // (5), a root guide (6).
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            guide(4.0),
            isolated(vec![
                raster(size, offset, 3, BlendMode::Screen, 0.9),
                guide(8.0),
            ]),
            raster((60, 50), (70, 70), 4, BlendMode::SoftLight, 0.8),
            guide(16.0),
        ],
    );
    assert_regions_match(&d, rect, "a group guide between root guides");
    let window = (90, 80, 20, 15);
    let p = plan(&d, window, 1).unwrap();
    let stages = p["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 3);
    let root_r1 = plane_region(16.0, window, canvas);
    let root_r0 = plane_region(4.0, root_r1, canvas);
    assert_eq!(stages[0]["plane"], region_json(root_r0));
    assert_eq!(stages[1]["plane"], region_json(root_r1));
    assert_eq!(
        stages[2],
        stage(json!(4), canvas, 3, 8.0, plane_region(8.0, root_r1, canvas))
    );
}

#[test]
fn two_guides_in_one_group_with_a_raster_between_nest_their_planes_at_u16() {
    let (cw, ch) = (197, 195);
    let canvas = (cw, ch);
    let (offset, size) = ((23, 19), (160, 150));
    let rect = rect_of(offset, size, canvas);
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            isolated(vec![
                raster(size, offset, 5, BlendMode::Normal, 0.95),
                guide(8.0),
                raster((70, 60), (60, 70), 6, BlendMode::Overlay, 0.9),
                guide(16.0),
            ]),
        ],
    );
    assert_regions_match(&d, rect, "two guides in one group");
    let window = (80, 70, 30, 20);
    let p = plan(&d, window, 1).unwrap();
    let r1 = plane_region(16.0, window, canvas);
    let r0 = plane_region(8.0, r1, canvas);
    assert_eq!(
        p["stages"],
        json!([
            stage(json!(5), canvas, 2, 8.0, r0),
            stage(json!(5), canvas, 4, 16.0, r1)
        ])
    );
}

#[test]
fn the_group_as_clip_base_member_inside_pass_through_masked_and_styled_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let canvas = (cw, ch);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, canvas);
    let group = || {
        isolated(vec![
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            guide(8.0),
        ])
    };
    let level_of =
        |d: &PrecisionDocument| plan(d, (60, 60, 20, 20), 1).unwrap()["stages"][0]["level"].clone();
    // A CLIP BASE with a visible member.
    let base = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            group(),
            raster((80, 60), (50, 60), 7, BlendMode::Normal, 0.9).with_clipped(true),
        ],
    );
    assert_regions_match(&base, rect, "the group as a clip base");
    assert_eq!(level_of(&base), 3);
    // A CLIP MEMBER over a raster base.
    let member = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            raster((120, 110), (40, 35), 8, BlendMode::Normal, 1.0),
            group().with_clipped(true),
        ],
    );
    assert_regions_match(&member, rect, "the group as a clip member");
    assert_eq!(level_of(&member), 4);
    // INSIDE a plain and a gated Pass Through group (a raster beside it).
    for (opacity, gated_mask) in [(1.0, false), (0.6, false), (1.0, true)] {
        let mut outer = Layer::pass_through(
            "Pass",
            vec![
                group(),
                raster((50, 40), (100, 90), 9, BlendMode::Screen, 0.8),
            ],
            opacity,
            true,
        )
        .unwrap();
        if gated_mask {
            outer = outer.with_mask(Some(mask(rect, 0.0)));
        }
        let d = doc(cw, ch, vec![backdrop(cw, ch), outer]);
        let what = format!("inside pass-through {opacity} mask {gated_mask}");
        assert_regions_match(&d, rect, &what);
        assert_eq!(level_of(&d), 3, "{what}");
    }
    // MASKED (feather 6) at opacity 0.7 Multiply.
    let masked = Layer::isolated_normal(
        "Group",
        vec![
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            guide(8.0),
        ],
        0.7,
        true,
    )
    .unwrap()
    .with_blend_mode(BlendMode::Multiply)
    .unwrap()
    .with_mask(Some(mask(rect, 6.0)));
    let d = doc(cw, ch, vec![backdrop(cw, ch), masked]);
    assert_regions_match(&d, rect, "masked feather 6, 0.7 Multiply");
    // A fill / Blend If style (no effect: not materialized).
    let s = styled(
        group(),
        &format!("{{\"fill_opacity\":0.6,{BLEND_IF_GRAY}}}"),
        canvas,
    );
    let d = doc(cw, ch, vec![backdrop(cw, ch), s]);
    assert_regions_match(&d, rect, "fill + Blend If style");
    assert_eq!(level_of(&d), 3);
}

#[test]
fn a_window_missing_the_content_still_stages_the_canvas_buffered_level_at_u16() {
    // Typed accumulators are canvas-sized: the group's buffer is the canvas,
    // so its level is staged over any window (the 8-bit rect skip has no
    // typed counterpart).
    let (cw, ch) = (197, 195);
    let (offset, size) = ((101, 97), (80, 70));
    let rect = rect_of(offset, size, (cw, ch));
    let mut layers = grouped(cw, ch, 8.0, offset, size);
    layers.push(guide(4.0));
    let d = doc(cw, ch, layers);
    assert_regions_match(&d, rect, "a root guide over a far group");
    let p = plan(&d, (0, 0, 20, 20), 1).unwrap();
    let levels: Vec<&Value> = p["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| &s["level"])
        .collect();
    assert_eq!(levels, [&Value::Null, &json!(3)]);
}

#[test]
fn materialized_subtrees_admit_guides_and_stage_nothing_at_u16() {
    let (cw, ch) = (113, 101);
    let canvas = (cw, ch);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, canvas);
    let children = || {
        vec![
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            guide(8.0),
        ]
    };
    let m = styled(isolated(children()), GROUP_STYLE, canvas);
    let d = doc(cw, ch, vec![backdrop(cw, ch), m]);
    assert_regions_match(&d, rect, "a guided child of a materialized group");
    let p = plan(&d, (30, 30, 20, 20), 1).unwrap();
    assert_eq!(p["stages"], json!([]));
    let (_, _, halo) = geometry(8.0);
    assert_eq!(p["layers"][2]["neighborhood"]["halo_pixels"], halo);
    for isolated_inner in [true, false] {
        let inner = if isolated_inner {
            isolated(children())
        } else {
            Layer::pass_through("Pass", children(), 1.0, true).unwrap()
        };
        let m = styled(isolated(vec![inner]), GROUP_STYLE, canvas);
        let d = doc(cw, ch, vec![backdrop(cw, ch), m]);
        let what = format!("a guide in an isolated={isolated_inner} group inside M");
        assert_regions_match(&d, rect, &what);
        assert_eq!(
            plan(&d, (30, 30, 20, 20), 1).unwrap()["stages"],
            json!([]),
            "{what}"
        );
    }
}

#[test]
fn a_guided_base_over_a_run_inside_a_group_contributes_nothing_at_u16() {
    let (cw, ch) = (113, 101);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, (cw, ch));
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            isolated(vec![
                raster(size, offset, 1, BlendMode::Overlay, 0.85),
                guide(8.0),
                raster((30, 30), (30, 30), 2, BlendMode::Normal, 1.0).with_clipped(true),
            ]),
        ],
    );
    assert_regions_match(&d, rect, "a guided base over a run in a group");
    let p = plan(&d, (30, 30, 20, 20), 1).unwrap();
    assert_eq!(p["stages"], json!([]));
    assert_eq!(p["layers"][2]["neighborhood"]["kind"], "pointwise");
}

#[test]
fn guided_clip_members_and_pass_through_children_match_at_u16() {
    let (cw, ch) = (53, 50);
    let children = || {
        vec![
            raster((30, 25), (7, 5), 1, BlendMode::Normal, 1.0),
            guide(4.0).with_clipped(true),
        ]
    };
    let rect = rect_of((7, 5), (30, 25), (cw, ch));
    // Since 13A-4b-ii-b-1 a guided clip member inside an isolated group is
    // staged by the group's level; inside a materialized group the reference
    // renders it (no stage).
    let d = doc(cw, ch, vec![backdrop(cw, ch), isolated(children())]);
    assert_regions_match(&d, rect, "a guided clip member in an isolated group");
    let m = styled(isolated(children()), GROUP_STYLE, (cw, ch));
    let m = doc(cw, ch, vec![backdrop(cw, ch), m]);
    assert_regions_match(&m, rect, "a guided clip member in a materialized group");
    assert_eq!(plan(&m, (10, 8, 20, 15), 1).unwrap()["stages"], json!([]));
    // A guided child directly in a plain and a gated Pass Through group:
    // since 13A-4b-ii-a staged by the enclosing (root) level.
    for opacity in [1.0, 0.6] {
        let pass = Layer::pass_through(
            "Pass",
            vec![
                raster((30, 25), (7, 5), 1, BlendMode::Normal, 1.0),
                guide(4.0),
            ],
            opacity,
            true,
        )
        .unwrap();
        assert_regions_match(
            &doc(cw, ch, vec![backdrop(cw, ch), pass]),
            rect,
            &format!("a guide in a Pass Through group at opacity {opacity}"),
        );
    }
}
