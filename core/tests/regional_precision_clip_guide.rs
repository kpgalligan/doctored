//! Phase 13A-4b-ii-b-1: the 16-bit regional render over a guide-reading
//! Shadows/Highlights as a CLIP MEMBER of an UNSTYLED base (at the root,
//! inside isolated levels, inside plain and gated pass-through groups, beside
//! knockout members, under a Blend If base, under D5's Background), staged
//! through the enclosing level with the unit re-run from its held "began"
//! sample, against the full-frame typed projection. The oracle is INDEPENDENT
//! of the regional code: `dr_precision_doc_render`'s projection read back as
//! u16 codes, cropped and top-left-cell sampled here in plain index
//! arithmetic (tolerance: EXACT, declared before measuring). Every typed
//! accumulator (and so every clip unit's private buffer) is canvas-sized, so
//! a member stage is anchored at the canvas; one plane over `R_0` (and its
//! `zero_plane` fill) per distinct clip unit holding a guided member, and per
//! pass-through group on the path (the typed arm always copies), and every
//! member stage's rows counted from its unit's FIRST row — restated here
//! independently. The u16 concurrent-cancel measurement lives in
//! `regional_byte_clip_guide.rs` beside the 8-bit one.

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
use doctored_core::style::{BlendIf, BlendIfChannel, LayerStyle};
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

fn member(size: (u32, u32), offset: (i32, i32), seed: u32, mode: BlendMode, opacity: f32) -> Layer {
    raster(size, offset, seed, mode, opacity).with_clipped(true)
}

fn guide_at(radius: f32, opacity: f32) -> Layer {
    let params = json!({"shadows":{"amount":0.7,"tone":0.9},
        "highlights":{"amount":0.35,"tone":0.8},"radius":radius});
    let op = json!({"type":"adjust","op":"shadows_highlights","params":params,
        "evaluation":"precision_v1"});
    Layer::adjustment(
        "shadows_highlights",
        &op.to_string(),
        opacity,
        true,
        &profile(),
    )
    .unwrap()
}

fn guide(radius: f32) -> Layer {
    guide_at(radius, 0.8)
}

/// A guide CLIPPED to the unit below it.
fn member_guide(radius: f32) -> Layer {
    guide(radius).with_clipped(true)
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

fn blend_if() -> BlendIf {
    BlendIf {
        channel: BlendIfChannel::Gray,
        this_layer: [10, 60, 180, 240],
        underlying: [0, 40, 200, 255],
        ..BlendIf::default()
    }
}

/// The content rect of a raster at `offset` of `size`, within the canvas.
fn rect_of(offset: (i32, i32), size: (u32, u32), canvas: (u32, u32)) -> Window {
    let x0 = offset.0.max(0) as u32;
    let y0 = offset.1.max(0) as u32;
    let x1 = ((offset.0 + size.0 as i32).max(0) as u32).min(canvas.0);
    let y1 = ((offset.1 + size.1 as i32).max(0) as u32).min(canvas.1);
    (x0, y0, x1 - x0, y1 - y0)
}

/// A holed Overlay raster B at `offset` of `size` and a guide of `radius`
/// clipped to it: one clip unit.
fn unit(radius: f32, offset: (i32, i32), size: (u32, u32)) -> Vec<Layer> {
    vec![
        raster(size, offset, 1, BlendMode::Overlay, 0.85),
        member_guide(radius),
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

/// The ROOT staged plan restated: `guides` are (layer, radius, rows,
/// through) from the first guide to the last — `rows` counted from the
/// guided row, or for a clip MEMBER from its unit's FIRST row, to the next
/// guided row — over `rows_0` rows below the first guide; `holds` planes over
/// `R_0` (one per distinct clip unit holding a guided member and per
/// pass-through group on a path). Scratch: the accumulator, the output, the
/// allowance, every stage plane and guide, the holds; work: `P_0`'s fill,
/// each stage's rows over the next plane (the last over the output), every
/// guide, the output fill and the holds' fills.
fn assert_root_plan(
    d: &PrecisionDocument,
    guides: &[(usize, f32, u64, &[usize])],
    rows_0: u64,
    holds: u64,
    window: Window,
    div: u32,
) {
    let canvas = d.dimensions();
    let p = plan(d, window, div).unwrap();
    let pixels = u64::from(window.2.div_ceil(div)) * u64::from(window.3.div_ceil(div));
    let mut planes = vec![window; guides.len()];
    let mut inner = window;
    for (k, &(_, radius, _, _)) in guides.iter().enumerate().rev() {
        inner = plane_region(radius, inner, canvas);
        planes[k] = inner;
    }
    let stages: Vec<Value> = guides
        .iter()
        .zip(&planes)
        .map(|(&(layer, radius, _, through), &plane)| {
            stage(Value::Null, canvas, layer, radius, plane, through)
        })
        .collect();
    assert_eq!(p["stages"], json!(stages), "{window:?}/{div}");
    let halo: u32 = guides
        .iter()
        .map(|&(_, radius, _, _)| geometry(radius).2)
        .sum();
    assert_eq!(p["neighborhood"]["halo_pixels"], halo, "{window:?}/{div}");
    assert_eq!(p["required_input_region"], grown(window, halo, canvas));
    assert!(within(planes[0], window_of(&p["required_input_region"])));
    let r0 = planes[0];
    let mut expected_scratch = pixels * 16 + pixels * 8 + BOOKKEEPING + holds * area(r0) * 16;
    let mut expected_work = area(r0) + area(r0) * rows_0 + pixels + holds * area(r0);
    for (k, &(_, radius, rows, _)) in guides.iter().enumerate() {
        let (gs, gw) = guide_figures(radius, planes[k]);
        let evaluated = planes.get(k + 1).map_or(pixels, |&next| area(next));
        expected_scratch += area(planes[k]) * 16 + gs;
        expected_work += evaluated * rows + gw;
    }
    assert_eq!(scratch(&p), expected_scratch, "{window:?}/{div}");
    assert_eq!(work(&p), expected_work, "{window:?}/{div}");
}

fn assert_unit_radius(cw: u32, ch: u32, radius: f32) {
    let offset = (37, 29);
    let size = (cw - 80, ch - 70);
    let rect = rect_of(offset, size, (cw, ch));
    let mut layers = vec![backdrop(cw, ch)];
    layers.extend(unit(radius, offset, size));
    let d = doc(cw, ch, layers);
    let unguided = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
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
        ((cw - 3, ch - 3, 3, 3), 3),
    ] {
        // Backdrop and B over R_0; B RE-RUN with the guide over the output;
        // the unit's began sample held.
        assert_root_plan(&d, &[(2, radius, 2, &[])], 2, 1, window, div);
        let p = plan(&d, window, div).unwrap();
        assert_eq!(p["layers"][2]["clip_base"], 1);
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_guide_clipped_to_a_holey_raster_at_the_root_stages_radius_4_at_u16() {
    assert_unit_radius(113, 101, 4.0);
}

#[test]
fn a_guide_clipped_to_a_holey_raster_at_the_root_stages_radius_30_at_u16() {
    assert_unit_radius(389, 387, 30.0);
}

#[test]
fn a_guide_clipped_to_a_holey_raster_at_the_root_stages_radius_120_at_u16() {
    let (_, _, halo) = geometry(120.0);
    assert_unit_radius(3 * halo + 5, 3 * halo + 3, 120.0);
}

#[test]
fn the_spec_example_counts_the_member_stage_from_the_units_first_row_at_u16() {
    // Raster base B (row 0) + guided member M (row 1): rows_0 = 1,
    // stages[0].rows = 2.
    let (cw, ch) = (113, 101);
    let d = doc(
        cw,
        ch,
        vec![
            Layer::raster("B", holes(cw, ch, 3), (0, 0), 1.0, true).unwrap(),
            member_guide(8.0),
        ],
    );
    assert_regions_match(&d, (0, 0, cw, ch), "B + M");
    for (window, div) in [((30, 20, 40, 30), 1), ((0, 0, cw, ch), 3)] {
        assert_root_plan(&d, &[(1, 8.0, 2, &[])], 1, 1, window, div);
    }
}

#[test]
fn raster_members_below_and_above_the_guide_are_re_run_with_the_unit_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            member((70, 60), (60, 50), 2, BlendMode::Multiply, 0.9),
            member_guide(8.0),
            member((60, 70), (80, 70), 3, BlendMode::Screen, 0.8),
        ],
    );
    assert_regions_match(&d, rect, "members below and above the guide");
    for (window, div) in [((60, 50, 30, 20), 1), ((0, 0, cw, ch), 2)] {
        assert_root_plan(&d, &[(3, 8.0, 4, &[])], 3, 1, window, div);
    }
}

#[test]
fn two_guides_in_one_unit_resume_one_hold_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((23, 19), (160, 150));
    let rect = rect_of(offset, size, (cw, ch));
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            raster(size, offset, 5, BlendMode::Normal, 0.95),
            member_guide(8.0),
            member((70, 60), (60, 70), 6, BlendMode::Overlay, 0.9),
            member_guide(16.0),
        ],
    );
    assert_regions_match(&d, rect, "two guides in one unit");
    for (window, div) in [((80, 70, 30, 20), 1), ((0, 0, cw, ch), 3)] {
        assert_root_plan(
            &d,
            &[(2, 8.0, 3, &[]), (4, 16.0, 4, &[])],
            2,
            1,
            window,
            div,
        );
    }
}

#[test]
fn a_shallow_knockout_member_beside_the_guide_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    let knockout = |seed: u32, at: (i32, i32)| {
        styled(
            member((70, 60), at, seed, BlendMode::Normal, 0.9),
            SHALLOW,
            (cw, ch),
        )
    };
    // Not a Background (D5 is its own case below): a Shallow member below
    // the guide, and one above it.
    let below = doc(
        cw,
        ch,
        vec![
            backdrop_at(cw, ch, 0.97),
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            knockout(2, (60, 50)),
            member_guide(8.0),
        ],
    );
    let plain = doc(
        cw,
        ch,
        vec![
            backdrop_at(cw, ch, 0.97),
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            member((70, 60), (60, 50), 2, BlendMode::Normal, 0.9),
            member_guide(8.0),
        ],
    );
    assert_ne!(full(&below), full(&plain), "the knockout punches");
    assert_regions_match(&below, rect, "a Shallow member below the guide");
    let above = doc(
        cw,
        ch,
        vec![
            backdrop_at(cw, ch, 0.97),
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            member_guide(8.0),
            knockout(4, (70, 60)),
        ],
    );
    assert_regions_match(&above, rect, "a Shallow member above the guide");
}

#[test]
fn a_blend_if_base_clamps_the_members_and_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Typed Blend If on the base: its alpha weighted against the sample
    // beneath the unit (the held began sample on a re-run), members clamped
    // with `min`.
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            raster(size, offset, 1, BlendMode::Overlay, 0.85)
                .with_blend_if(Some(blend_if()))
                .unwrap(),
            member((70, 60), (60, 50), 2, BlendMode::Multiply, 0.9),
            member_guide(8.0),
        ],
    );
    assert_regions_match(&d, rect, "a Blend If base");
    let p = plan(&d, (60, 50, 30, 20), 1).unwrap();
    assert_eq!(p["layers"][1]["blend_if"], true);
    assert_root_plan(&d, &[(3, 8.0, 3, &[])], 3, 1, (60, 50, 30, 20), 1);
}

#[test]
fn a_masked_multiply_member_guide_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    let masked = guide_at(8.0, 0.7)
        .with_blend_mode(BlendMode::Multiply)
        .unwrap()
        .with_mask(Some(mask(rect, 6.0)))
        .with_clipped(true);
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            masked,
        ],
    );
    assert_regions_match(&d, rect, "a feather-6 masked Multiply 0.7 member guide");
    let p = plan(&d, (60, 50, 30, 20), 1).unwrap();
    assert_eq!(p["stages"][0]["layer"], 2);
    assert_eq!(p["layers"][2]["mask"], true);
}

#[test]
fn hidden_members_are_dropped_and_zero_opacity_members_staged_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    let layers = |raster_member: Layer, guided: Layer| {
        doc(
            cw,
            ch,
            vec![
                backdrop(cw, ch),
                raster(size, offset, 1, BlendMode::Overlay, 0.85),
                raster_member,
                guided,
            ],
        )
    };
    let kid = |visible: bool, opacity: f32| {
        Layer::raster("Member", holes(70, 60, 2), (60, 50), opacity, visible)
            .unwrap()
            .with_blend_mode(BlendMode::Multiply)
            .unwrap()
            .with_clipped(true)
    };
    // A hidden raster member is dropped (the scene keeps only visible
    // members); a zero-opacity one stays in the loop.
    for (visible, opacity) in [(false, 0.9), (true, 0.0)] {
        let d = layers(kid(visible, opacity), member_guide(8.0));
        let what = format!("raster member visible {visible} opacity {opacity}");
        assert_regions_match(&d, rect, &what);
        assert_eq!(
            plan(&d, (60, 50, 30, 20), 1).unwrap()["stages"][0]["layer"],
            3,
            "{what}"
        );
    }
    // The guided member itself hidden: dropped, no stage.
    let hidden = Layer::adjustment(
        "shadows_highlights",
        &json!({"type":"adjust","op":"shadows_highlights","evaluation":"precision_v1",
            "params":{"shadows":{"amount":0.7,"tone":0.9},"radius":8}})
        .to_string(),
        0.8,
        false,
        &profile(),
    )
    .unwrap()
    .with_clipped(true);
    let d = layers(kid(true, 0.9), hidden);
    assert_regions_match(&d, rect, "a hidden guide");
    assert_eq!(plan(&d, (60, 50, 30, 20), 1).unwrap()["stages"], json!([]));
    // At opacity 0 the scene keeps it (visible): staged, then skipped by the
    // whole-layer skip.
    let d = layers(kid(true, 0.9), guide_at(8.0, 0.0).with_clipped(true));
    assert_regions_match(&d, rect, "a zero-opacity guide");
    assert_eq!(
        plan(&d, (60, 50, 30, 20), 1).unwrap()["stages"][0]["layer"],
        3
    );
}

#[test]
fn an_isolated_group_base_is_charged_in_the_member_stage_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); G (3) = { A (1), C (2) }; the guide (4) clipped to G.
    let group = |opacity: f32| {
        Layer::isolated_normal(
            "Group",
            vec![
                raster(size, offset, 1, BlendMode::Overlay, 0.85),
                raster((70, 60), (60, 50), 2, BlendMode::Multiply, 0.9),
            ],
            opacity,
            true,
        )
        .unwrap()
    };
    let d = doc(
        cw,
        ch,
        vec![backdrop(cw, ch), group(1.0), member_guide(8.0)],
    );
    assert_regions_match(&d, rect, "a guide clipped to an isolated group");
    for (window, div) in [((60, 50, 30, 20), 1), ((0, 0, cw, ch), 7)] {
        assert_root_plan(&d, &[(4, 8.0, 4, &[])], 4, 1, window, div);
    }
    let gated = doc(
        cw,
        ch,
        vec![backdrop(cw, ch), group(0.6), member_guide(8.0)],
    );
    assert_regions_match(&gated, rect, "the group base at 0.6");
}

#[test]
fn a_unit_inside_an_isolated_level_stages_the_groups_level_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((101, 97), (80, 70));
    let rect = rect_of(offset, size, (cw, ch));
    let canvas = (cw, ch);
    let layers = || vec![backdrop(cw, ch), isolated(unit(8.0, offset, size))];
    let d = doc(cw, ch, layers());
    assert_regions_match(&d, rect, "a unit in an isolated group");
    let window = (120, 110, 30, 20);
    assert_eq!(
        plan(&d, window, 1).unwrap()["stages"],
        json!([stage(
            json!(3),
            canvas,
            2,
            8.0,
            plane_region(8.0, window, canvas),
            &[]
        )])
    );
    // Under a ROOT guide (4) above it: the root's level, then the group's.
    let mut rooted = layers();
    rooted.push(guide(4.0));
    let rooted = doc(cw, ch, rooted);
    assert_regions_match(&rooted, rect, "under a root guide");
    let root_r0 = plane_region(4.0, window, canvas);
    assert_eq!(
        plan(&rooted, window, 1).unwrap()["stages"],
        json!([
            stage(Value::Null, canvas, 4, 4.0, root_r0, &[]),
            stage(
                json!(3),
                canvas,
                2,
                8.0,
                plane_region(8.0, root_r0, canvas),
                &[]
            ),
        ])
    );
}

#[test]
fn a_unit_inside_plain_and_gated_pass_throughs_holds_the_unit_and_the_group_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // Backdrop (0); P (3) = { B (1), guide (2) clipped to B }. Holds: the
    // unit and the group (the typed arm always copies).
    for (opacity, masked) in [(1.0, false), (0.6, false), (0.6, true)] {
        let p = pass(unit(8.0, offset, size), opacity).with_mask(masked.then(|| mask(rect, 6.0)));
        let d = doc(cw, ch, vec![backdrop(cw, ch), p]);
        let what = format!("a unit in a pass-through, {opacity}, mask {masked}");
        assert_regions_match(&d, rect, &what);
        if !masked {
            for (window, div) in [((60, 50, 30, 20), 1), ((0, 0, cw, ch), 2)] {
                assert_root_plan(&d, &[(2, 8.0, 3, &[3])], 2, 2, window, div);
            }
        }
    }
    // Nested: Outer (4) = { P (3) }: three holds.
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            pass(vec![pass(unit(8.0, offset, size), 1.0)], 0.7),
        ],
    );
    assert_regions_match(&d, rect, "a unit in plain in gated");
    assert_root_plan(&d, &[(2, 8.0, 4, &[4, 3])], 2, 3, (60, 50, 30, 20), 1);
}

#[test]
fn a_unit_between_root_guides_below_and_above_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((33, 27), (140, 130));
    let rect = rect_of(offset, size, (cw, ch));
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            guide(4.0),
            raster(size, offset, 3, BlendMode::Screen, 0.9),
            member_guide(8.0),
            raster((60, 50), (70, 70), 4, BlendMode::SoftLight, 0.8),
            guide(16.0),
        ],
    );
    assert_regions_match(&d, rect, "a unit between root guides");
    for (window, div) in [((90, 80, 20, 15), 1), ((0, 0, cw, ch), 3)] {
        assert_root_plan(
            &d,
            &[(1, 4.0, 2, &[]), (3, 8.0, 3, &[]), (5, 16.0, 1, &[])],
            1,
            1,
            window,
            div,
        );
    }
}

#[test]
fn a_unit_above_d5s_background_matches_at_u16() {
    let (cw, ch) = (197, 195);
    let (offset, size) = ((31, 25), (130, 120));
    let rect = rect_of(offset, size, (cw, ch));
    // D5: a Shallow member of the unit above the Background.
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            raster(size, offset, 1, BlendMode::Overlay, 0.85),
            styled(
                member((70, 60), (60, 50), 2, BlendMode::Normal, 0.9),
                SHALLOW,
                (cw, ch),
            ),
            member_guide(8.0),
        ],
    );
    assert_regions_match(&d, rect, "D5 under a unit with a Shallow member");
    // A Deep knockout raster at the root between the Background and the
    // unit.
    let deep = styled(
        raster((60, 50), (20, 20), 7, BlendMode::Normal, 0.9),
        "{\"fill_opacity\":0.4,\"knockout\":\"deep\"}",
        (cw, ch),
    );
    let mut layers = vec![backdrop(cw, ch), deep];
    layers.extend(unit(8.0, offset, size));
    assert_regions_match(&doc(cw, ch, layers), rect, "D5, a Deep knockout below");
}

#[test]
fn a_unit_inside_a_materialized_group_is_rendered_unstaged_at_u16() {
    let (cw, ch) = (113, 101);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, (cw, ch));
    let m = styled(isolated(unit(8.0, offset, size)), GROUP_STYLE, (cw, ch));
    let d = doc(cw, ch, vec![backdrop(cw, ch), m]);
    assert_regions_match(&d, rect, "a member guide inside M");
    let p = plan(&d, (30, 30, 20, 20), 1).unwrap();
    assert_eq!(p["stages"], json!([]));
    let (_, _, halo) = geometry(8.0);
    assert_eq!(p["layers"][2]["neighborhood"]["halo_pixels"], halo);
}

#[test]
fn a_guide_clipped_to_an_adjustment_base_contributes_nothing_and_is_staged_at_u16() {
    let (cw, ch) = (113, 101);
    let levels = json!({"type":"adjust","op":"levels","evaluation":"precision_v1",
        "params":{"black":0.1,"gamma":1.6}});
    let d = doc(
        cw,
        ch,
        vec![
            backdrop(cw, ch),
            Layer::adjustment("levels", &levels.to_string(), 1.0, true, &profile()).unwrap(),
            member_guide(8.0),
        ],
    );
    assert_regions_match(&d, (0, 0, cw, ch), "a guide clipped to Levels");
    assert_root_plan(&d, &[(2, 8.0, 2, &[])], 2, 1, (30, 20, 30, 20), 1);
}

#[test]
fn styled_bases_equal_the_reference_at_u16() {
    let (cw, ch) = (113, 101);
    let (offset, size) = ((17, 13), (70, 60));
    let rect = rect_of(offset, size, (cw, ch));
    // Since 13A-4b-ii-b-2 every styled base stages the member at 16 bits —
    // effects at 0.85 included (the typed package is one canvas-sized copy).
    for style in [
        "{\"fill_opacity\":0.5}",
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}",
        SHALLOW,
    ] {
        let d = doc(
            cw,
            ch,
            vec![
                backdrop(cw, ch),
                styled(
                    raster(size, offset, 1, BlendMode::Overlay, 0.85),
                    style,
                    (cw, ch),
                ),
                member_guide(8.0),
            ],
        );
        assert_regions_match(&d, rect, style);
    }
    // A materialized group base: G (2) = { A (1) } with an effect; the guide
    // (3) clipped to G.
    let g = styled(
        isolated(vec![raster(size, offset, 1, BlendMode::Overlay, 0.85)]),
        GROUP_STYLE,
        (cw, ch),
    );
    let d = doc(cw, ch, vec![backdrop(cw, ch), g, member_guide(8.0)]);
    assert_regions_match(&d, rect, "a materialized group base");
}

#[test]
fn linear_light_evaluates_a_document_with_a_member_guide_at_u16_regionally() {
    let mut layers = vec![backdrop(53, 50)];
    layers.extend(unit(4.0, (7, 5), (30, 25)));
    let d = doc(53, 50, layers);
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
