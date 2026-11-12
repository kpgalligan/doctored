//! Phase 13A-4b-ii-a: the 16-bit regional render over a guide-reading
//! Shadows/Highlights directly inside PASS-THROUGH groups (plain or gated by
//! opacity / mask, nested, at the root or inside an isolated group), staged
//! through the ENCLOSING level, against the full-frame typed projection. The
//! oracle is INDEPENDENT of the regional code: `dr_precision_doc_render`'s
//! projection read back as u16 codes, cropped and top-left-cell sampled here
//! in plain index arithmetic (tolerance: EXACT, declared before measuring).
//! Every typed accumulator is canvas-sized and the typed pass-through arm
//! always composites onto a copy gated back, so every pass-through group on a
//! stage's path is HELD: one more plane over `R_0` (and its `zero_plane`
//! fill) per distinct group (restated here independently). The u16
//! concurrent-cancel measurement lives in
//! `regional_byte_pass_through_guide.rs` beside the 8-bit one.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::BlendMode;
use doctored_core::doc_evaluation::Evaluation;
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
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

const GROUP_STYLE: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                           {\"type\":\"stroke\",\"size\":3}]}";

const SHALLOW: &str = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}";

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

/// 13A-4b-i's windows plus, for the content `rect`, windows straddling each
/// of its corners and windows wholly outside it.
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

/// A canvas-sized backdrop with transparent holes and translucent pixels at
/// `opacity` (1: a knockout above takes it for the Background, D5).
fn backdrop_at(cw: u32, ch: u32, opacity: f32) -> Layer {
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
    Layer::raster("Backdrop", image, (0, 0), opacity, true).unwrap()
}

fn backdrop(cw: u32, ch: u32) -> Layer {
    backdrop_at(cw, ch, 1.0)
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

fn pass(children: Vec<Layer>, opacity: f32) -> Layer {
    Layer::pass_through("Pass", children, opacity, true).unwrap()
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

/// A holed Overlay raster A at `offset` of `size` and a guide of `radius`.
fn children(radius: f32, offset: (i32, i32), size: (u32, u32)) -> Vec<Layer> {
    vec![
        raster(size, offset, 1, BlendMode::Overlay, 0.85),
        guide(radius),
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
/// CANVAS origin (every typed buffer is the canvas), clamped.
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

fn stage(
    level: Value,
    canvas: (u32, u32),
    layer: usize,
    radius: f32,
    plane: Window,
    through: &[usize],
) -> Value {
    let (f, cells, _) = geometry(radius);
    json!({"level": level, "buffer": region_json((0, 0, canvas.0, canvas.1)), "layer": layer,
        "plane": region_json(plane), "cell_pixels": f, "reach_cells": cells,
        "through": through})
}

fn scratch(p: &Value) -> u64 {
    p["scratch_heap_bytes"].as_u64().unwrap()
}

fn work(p: &Value) -> u64 {
    p["work_units"].as_u64().unwrap()
}

/// The root Pass Through document's plan (backdrop 0, P 3 = { A 1, guide
/// 2 }), restated for `window` at `div`: the root's level stages the guide
/// through the group, which the typed arm always holds.
fn assert_pass_plan(d: &PrecisionDocument, radius: f32, window: Window, div: u32) {
    let canvas = d.dimensions();
    let p = plan(d, window, div).unwrap();
    let (_, _, halo) = geometry(radius);
    let pixels = u64::from(window.2.div_ceil(div)) * u64::from(window.3.div_ceil(div));
    let r0 = plane_region(radius, window, canvas);
    assert_eq!(
        p["stages"],
        json!([stage(Value::Null, canvas, 2, radius, r0, &[3])]),
        "{window:?}"
    );
    assert_eq!(p["neighborhood"]["halo_pixels"], halo);
    assert_eq!(p["layers"][2]["group"], 3);
    assert_eq!(p["required_input_region"], grown(window, halo, canvas));
    assert!(within(r0, window_of(&p["required_input_region"])));
    let (gs, gw) = guide_figures(radius, r0);
    // The accumulator, the output, the allowance, P_0 and the guide, and the
    // hold's plane.
    assert_eq!(
        scratch(&p),
        pixels * 16 + pixels * 8 + BOOKKEEPING + area(r0) * 16 + gs + area(r0) * 16,
        "{window:?}"
    );
    // `P_0`'s fill, backdrop and A over R_0, the guide, the guide and the
    // group over the output, the output fill, and the hold's fill.
    assert_eq!(
        work(&p),
        area(r0) + area(r0) * 2 + gw + pixels * 2 + pixels + area(r0),
        "{window:?}"
    );
}

fn assert_pass_radius(cw: u32, ch: u32, radius: f32) {
    let offset = (37, 29);
    let size = (cw - 80, ch - 70);
    let rect = rect_of(offset, size, (cw, ch));
    let layers = |opacity: f32| {
        vec![
            backdrop(cw, ch),
            pass(children(radius, offset, size), opacity),
        ]
    };
    let d = doc(cw, ch, layers(1.0));
    let unguided = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            pass(vec![raster(size, offset, 1, BlendMode::Overlay, 0.85)], 1.0),
        ],
    );
    assert_ne!(
        full(&d),
        full(&unguided),
        "radius {radius}: the guide changes the projection"
    );
    assert_regions_match(&d, rect, &format!("radius {radius}, plain"));
    let gated = doc(cw, ch, layers(0.6));
    assert_regions_match(&gated, rect, &format!("radius {radius}, opacity 0.6"));
    for (window, div) in [
        ((cw / 3 + 1, ch / 3 + 1, cw / 3 - 2, ch / 3 - 2), 1),
        ((0, 0, cw, ch), 2),
        ((rect.0 - 4, rect.1 - 3, 9, 7), 1),
        ((cw - 3, ch - 3, 3, 3), 3),
    ] {
        assert_pass_plan(&d, radius, window, div);
        assert_pass_plan(&gated, radius, window, div);
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_plain_and_a_gated_root_pass_through_stage_a_radius_4_guide_at_u16() {
    assert_pass_radius(113, 101, 4.0);
}

#[test]
fn a_plain_and_a_gated_root_pass_through_stage_a_radius_30_guide_at_u16() {
    assert_pass_radius(389, 387, 30.0);
}

#[test]
fn a_plain_and_a_gated_root_pass_through_stage_a_radius_120_guide_at_u16() {
    let (_, _, halo) = geometry(120.0);
    assert_pass_radius(3 * halo + 5, 3 * halo + 3, 120.0);
}

#[test]
fn a_pass_through_gated_by_a_feathered_mask_and_by_both_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    for opacity in [1.0, 0.6] {
        let p = pass(children(8.0, offset, size), opacity).with_mask(Some(mask(rect, 6.0)));
        let d = doc(cw, ch, vec![backdrop(cw, ch), p]);
        assert_regions_match(&d, rect, &format!("a feather-6 mask at {opacity}"));
        let q = plan(&d, (60, 50, 30, 20), 1).unwrap();
        assert_eq!(q["stages"][0]["through"], json!([3]));
    }
}

#[test]
fn a_shallow_knockout_child_beside_the_guide_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    let mut kids = children(8.0, offset, size);
    let a = kids.remove(0);
    kids.insert(0, styled(a, SHALLOW, (cw, ch)));
    // Not a Background (D5 is its own case below).
    let d = doc(cw, ch, vec![backdrop_at(cw, ch, 0.97), pass(kids, 1.0)]);
    let plain = doc(
        cw,
        ch,
        vec![
            backdrop_at(cw, ch, 0.97),
            pass(children(8.0, offset, size), 1.0),
        ],
    );
    assert_ne!(full(&d), full(&plain), "the knockout punches");
    assert_regions_match(&d, rect, "a Shallow child beside the guide");
}

#[test]
fn nested_pass_throughs_hold_every_level_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    let window = (60, 50, 30, 20);
    let r0 = plane_region(8.0, window, (cw, ch));
    // Backdrop (0); Outer (5) = { Inner (3) = { A (1), guide (2) }, B (4) }.
    let nested = |inner: f32, outer: f32| {
        doc(
            cw,
            ch,
            vec![
                backdrop(cw, ch),
                pass(
                    vec![
                        pass(children(8.0, offset, size), inner),
                        raster((70, 60), (60, 70), 2, BlendMode::Multiply, 0.85),
                    ],
                    outer,
                ),
            ],
        )
    };
    // One level for comparison: A, the guide, B and the group at the root.
    let single = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            pass(
                vec![
                    raster(size, offset, 1, BlendMode::Overlay, 0.85),
                    guide(8.0),
                    raster((70, 60), (60, 70), 2, BlendMode::Multiply, 0.85),
                ],
                1.0,
            ),
        ],
    );
    for (inner, outer) in [(1.0, 1.0), (1.0, 0.6), (0.6, 1.0), (0.7, 0.6)] {
        let d = nested(inner, outer);
        let what = format!("inner {inner} in outer {outer}");
        assert_regions_match(&d, rect, &what);
        let p = plan(&d, window, 1).unwrap();
        assert_eq!(p["stages"][0]["through"], json!([5, 3]), "{what}");
        // Two holds (the typed arm always copies), both entered in P_0; one
        // more row (the inner group) over the output than the single level.
        let q = plan(&single, window, 1).unwrap();
        let pixels = area(window);
        assert_eq!(scratch(&p) - scratch(&q), area(r0) * 16, "{what}");
        assert_eq!(work(&p) - work(&q), area(r0) + pixels, "{what}");
    }
}

#[test]
fn a_pass_through_inside_an_isolated_group_stages_the_groups_level_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((101, 97), (80, 70));
    let rect = rect_of(offset, size, (cw, ch));
    let canvas = (cw, ch);
    let layers = || {
        vec![
            backdrop(cw, ch),
            isolated(vec![pass(children(8.0, offset, size), 0.6)]),
        ]
    };
    let d = doc(cw, ch, layers());
    assert_regions_match(&d, rect, "a gated pass-through in an isolated group");
    let window = (120, 110, 30, 20);
    let p = plan(&d, window, 1).unwrap();
    let r0 = plane_region(8.0, window, canvas);
    assert_eq!(
        p["stages"],
        json!([stage(json!(4), canvas, 2, 8.0, r0, &[3])])
    );
    // Every typed buffer is the canvas: a window missing the content still
    // stages the group's level.
    let far = plan(&d, (0, 0, 20, 20), 1).unwrap();
    assert_eq!(far["stages"][0]["level"], 4);
    // Under a ROOT guide (5): the root's level, then the group's over the
    // root's R_0.
    let mut rooted = layers();
    rooted.push(guide(4.0));
    let rooted = doc(cw, ch, rooted);
    assert_regions_match(&rooted, rect, "under a root guide");
    let p = plan(&rooted, window, 1).unwrap();
    let root_r0 = plane_region(4.0, window, canvas);
    assert_eq!(
        p["stages"],
        json!([
            stage(Value::Null, canvas, 5, 4.0, root_r0, &[]),
            stage(
                json!(4),
                canvas,
                2,
                8.0,
                plane_region(8.0, root_r0, canvas),
                &[3]
            ),
        ])
    );
}

#[test]
fn an_isolated_group_whose_only_guide_is_inside_a_pass_through_child_is_staged_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((33, 27), (140, 130));
    let rect = rect_of(offset, size, (cw, ch));
    let canvas = (cw, ch);
    for opacity in [1.0, 0.6] {
        // Backdrop (0); G (5) = { P (3) = { A (1), guide (2) }, B (4) }.
        let d = doc(
            cw,
            ch,
            vec![
                backdrop(cw, ch),
                isolated(vec![
                    pass(children(16.0, offset, size), opacity),
                    raster((60, 50), (70, 70), 3, BlendMode::Screen, 0.8),
                ]),
            ],
        );
        let what = format!("a group whose guide is in its pass-through child, {opacity}");
        assert_regions_match(&d, rect, &what);
        let window = (80, 70, 30, 20);
        let p = plan(&d, window, 1).unwrap();
        assert_eq!(
            p["stages"],
            json!([stage(
                json!(5),
                canvas,
                2,
                16.0,
                plane_region(16.0, window, canvas),
                &[3]
            )]),
            "{what}"
        );
    }
}

#[test]
fn an_isolated_guided_group_inside_a_pass_through_stages_two_levels_in_pre_order_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((33, 27), (140, 130));
    let rect = rect_of(offset, size, (cw, ch));
    let canvas = (cw, ch);
    for opacity in [1.0, 0.6] {
        // Backdrop (0); P (5) = { G (3) = { A (1), guide8 (2) }, guide16 (4) }.
        let d = doc(
            cw,
            ch,
            vec![
                backdrop(cw, ch),
                pass(
                    vec![isolated(children(8.0, offset, size)), guide(16.0)],
                    opacity,
                ),
            ],
        );
        let what = format!("a guided group in a pass-through, {opacity}");
        assert_regions_match(&d, rect, &what);
        let window = (80, 70, 30, 20);
        let p = plan(&d, window, 1).unwrap();
        let root_r0 = plane_region(16.0, window, canvas);
        assert_eq!(
            p["stages"],
            json!([
                stage(Value::Null, canvas, 4, 16.0, root_r0, &[5]),
                stage(
                    json!(3),
                    canvas,
                    2,
                    8.0,
                    plane_region(8.0, root_r0, canvas),
                    &[]
                ),
            ]),
            "{what}"
        );
    }
}

#[test]
fn two_guides_in_one_pass_through_resume_one_hold_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((23, 19), (160, 150));
    let rect = rect_of(offset, size, (cw, ch));
    let canvas = (cw, ch);
    let kids = || {
        vec![
            raster(size, offset, 5, BlendMode::Normal, 0.95),
            guide(8.0),
            raster((70, 60), (60, 70), 6, BlendMode::Overlay, 0.9),
            guide(16.0),
        ]
    };
    let window = (80, 70, 30, 20);
    let r1 = plane_region(16.0, window, canvas);
    let r0 = plane_region(8.0, r1, canvas);
    for (opacity, masked) in [(1.0, false), (0.6, false), (0.6, true)] {
        let p = pass(kids(), opacity).with_mask(masked.then(|| mask(rect, 6.0)));
        let d = doc(cw, ch, vec![backdrop(cw, ch), p]);
        let what = format!("two guides in a pass-through, {opacity}, mask {masked}");
        assert_regions_match(&d, rect, &what);
        let p = plan(&d, window, 1).unwrap();
        assert_eq!(
            p["stages"],
            json!([
                stage(Value::Null, canvas, 2, 8.0, r0, &[5]),
                stage(Value::Null, canvas, 4, 16.0, r1, &[5]),
            ]),
            "{what}"
        );
    }
}

#[test]
fn a_pass_through_guide_between_root_guides_below_and_above_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((33, 27), (140, 130));
    let rect = rect_of(offset, size, (cw, ch));
    let canvas = (cw, ch);
    for opacity in [1.0, 0.6] {
        // Backdrop (0), a root guide (1), P (4) = { A (2), guide (3) }, a
        // raster (5), a root guide (6).
        let d = doc(
            cw,
            ch,
            vec![
                backdrop(cw, ch),
                guide(4.0),
                pass(
                    vec![raster(size, offset, 3, BlendMode::Screen, 0.9), guide(8.0)],
                    opacity,
                ),
                raster((60, 50), (70, 70), 4, BlendMode::SoftLight, 0.8),
                guide(16.0),
            ],
        );
        let what = format!("between root guides, {opacity}");
        assert_regions_match(&d, rect, &what);
        let window = (90, 80, 20, 15);
        let r2 = plane_region(16.0, window, canvas);
        let r1 = plane_region(8.0, r2, canvas);
        let r0 = plane_region(4.0, r1, canvas);
        assert_eq!(
            plan(&d, window, 1).unwrap()["stages"],
            json!([
                stage(Value::Null, canvas, 1, 4.0, r0, &[]),
                stage(Value::Null, canvas, 3, 8.0, r1, &[4]),
                stage(Value::Null, canvas, 6, 16.0, r2, &[]),
            ]),
            "{what}"
        );
    }
}

#[test]
fn a_knockout_background_under_a_guided_pass_through_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    for opacity in [1.0, 0.6] {
        // D5: a Shallow child of the pass-through asks for a knockout.
        let mut kids = children(8.0, offset, size);
        let a = kids.remove(0);
        kids.insert(0, styled(a, SHALLOW, (cw, ch)));
        let d = doc(cw, ch, vec![backdrop(cw, ch), pass(kids, opacity)]);
        assert_regions_match(&d, rect, &format!("D5, a Shallow child, {opacity}"));
        // A Deep knockout at the root below the pass-through.
        let deep = styled(
            raster((60, 50), (20, 20), 7, BlendMode::Normal, 0.9),
            "{\"fill_opacity\":0.4,\"knockout\":\"deep\"}",
            (cw, ch),
        );
        let d = doc(
            cw,
            ch,
            vec![
                backdrop(cw, ch),
                deep,
                pass(children(8.0, offset, size), opacity),
            ],
        );
        assert_regions_match(&d, rect, &format!("D5, a Deep knockout below, {opacity}"));
    }
}

#[test]
fn a_pass_through_clip_base_absorbs_hidden_members_and_isolates_under_visible_ones_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    let canvas = (cw, ch);
    let window = (60, 50, 30, 20);
    for visible in [false, true] {
        // Backdrop (0), P (3) = { A (1), guide (2) }, a member (4) clipped.
        let member = Layer::raster("Member", holes(80, 60, 7), (50, 60), 0.9, visible)
            .unwrap()
            .with_clipped(true);
        let d = doc(
            cw,
            ch,
            vec![
                backdrop(cw, ch),
                pass(children(8.0, offset, size), 1.0),
                member,
            ],
        );
        let what = format!("a pass-through clip base, member visible {visible}");
        assert_regions_match(&d, rect, &what);
        let p = plan(&d, window, 1).unwrap();
        let r0 = plane_region(8.0, window, canvas);
        let expected = if visible {
            // Forced isolated (`isolated_normal`): the group's own level.
            stage(json!(3), canvas, 2, 8.0, r0, &[])
        } else {
            stage(Value::Null, canvas, 2, 8.0, r0, &[3])
        };
        assert_eq!(p["stages"], json!([expected]), "{what}");
    }
}

#[test]
fn a_hidden_or_zero_opacity_pass_through_matches_at_u16() {
    let (cw, ch) = (113, 101);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, (cw, ch));
    let hidden = Layer::pass_through("Pass", children(8.0, offset, size), 1.0, false).unwrap();
    let d = doc(cw, ch, vec![backdrop(cw, ch), hidden]);
    assert_regions_match(&d, rect, "a hidden pass-through");
    assert_eq!(plan(&d, (20, 20, 20, 20), 1).unwrap()["stages"], json!([]));
    // The typed skip is `visible` alone (the scene builder's rule): an
    // opacity-0 pass-through is staged and gated all the way back.
    let zero = doc(
        cw,
        ch,
        vec![backdrop(cw, ch), pass(children(8.0, offset, size), 0.0)],
    );
    assert_regions_match(&zero, rect, "a zero-opacity pass-through");
    let p = plan(&zero, (20, 20, 20, 20), 1).unwrap();
    assert_eq!(p["stages"][0]["through"], json!([3]));
}

#[test]
fn a_guided_clip_member_inside_a_pass_through_matches_at_u16() {
    let (cw, ch) = (53, 50);
    let rect = rect_of((7, 5), (30, 25), (cw, ch));
    for opacity in [1.0, 0.6] {
        let p = pass(
            vec![
                raster((30, 25), (7, 5), 1, BlendMode::Normal, 1.0),
                guide(4.0).with_clipped(true),
            ],
            opacity,
        );
        // Since 13A-4b-ii-b-1 staged by the root level through the group.
        assert_regions_match(
            &doc(cw, ch, vec![backdrop(cw, ch), p]),
            rect,
            &format!("a guided clip member in a pass-through at {opacity}"),
        );
    }
}

#[test]
fn a_pass_through_guide_inside_a_materialized_group_is_rendered_unstaged_at_u16() {
    let (cw, ch) = (113, 101);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, (cw, ch));
    let m = styled(
        isolated(vec![pass(children(8.0, offset, size), 0.6)]),
        GROUP_STYLE,
        (cw, ch),
    );
    let d = doc(cw, ch, vec![backdrop(cw, ch), m]);
    assert_regions_match(&d, rect, "a pass-through guide inside M");
    let p = plan(&d, (30, 30, 20, 20), 1).unwrap();
    assert_eq!(p["stages"], json!([]));
    let (_, _, halo) = geometry(8.0);
    assert_eq!(p["layers"][2]["neighborhood"]["halo_pixels"], halo);
}

#[test]
fn linear_light_evaluates_a_document_with_a_pass_through_guide_at_u16_regionally() {
    let d = doc(
        53,
        50,
        vec![backdrop(53, 50), pass(children(4.0, (7, 5), (30, 25)), 0.6)],
    );
    let linear = d
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(full(&linear), full(&d), "the policy changes the projection");
    assert_regions_match(&linear, (0, 0, 53, 50), "linear light");
}

#[test]
fn the_float_policy_refuses_the_guide_at_plan_render_and_full_render() {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let fixture = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    let op = json!({"type":"adjust","op":"shadows_highlights","evaluation":"precision_v1",
        "params":{"shadows":{"amount":0.7},"radius":30}})
    .to_string();
    let guided = fixture
        .add_adjustment("S/H", &op, 0.8, LIMIT, 0, None)
        .unwrap()
        .expect("an adjustment");
    let sentence = "the shadows_highlights adjustment is unavailable in a 32-bit float document";
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(&guided, LIMIT, 0, ptr::null(), &mut err) };
    assert!(image.is_null(), "the full render must refuse");
    assert_eq!(take(err), sentence);
    assert_eq!(plan(&guided, (0, 0, 4, 4), 1).unwrap_err(), sentence);
    assert_eq!(region(&guided, (0, 0, 4, 4), 1).unwrap_err(), sentence);
}
