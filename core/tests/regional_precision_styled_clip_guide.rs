//! Phase 13A-4b-ii-b-2: the 16-bit regional render over a guide-reading
//! Shadows/Highlights as a CLIP MEMBER of a STYLED base — a fill-only style,
//! effects at opacity 1 and below it (the typed PACKAGE is one canvas-sized
//! copy, so it is admitted here), a knockout base without and with planes
//! (routed), a Deep knockout base under D5's Background, a materialized
//! group base — at the root, inside isolated levels and plain and gated
//! pass-through groups, beside raster and knockout members, staged through
//! the enclosing level with the unit RE-RUN from its held "began" sample, the
//! staged cursor's member index a STOP inside `render_styled`'s own member
//! loop. The oracle is INDEPENDENT of the regional code:
//! `dr_precision_doc_render`'s projection read back as u16 codes, cropped and
//! top-left-cell sampled here in plain index arithmetic (tolerance: EXACT,
//! declared before measuring). Every typed accumulator (and so every styled
//! unit's private buffer) is canvas-sized, so a member stage is anchored at
//! the canvas; the plan figures are restated independently. The u16
//! concurrent-cancel measurement lives in `regional_byte_styled_clip_guide.rs`
//! beside the 8-bit one.

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
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

/// Two effects that render planes: a drop shadow (Below) and a stroke. The
/// stroke is mid-toned: its band lands wholly under the holed raster's
/// partial pixels (#2's disjoint union), and an Overlay layer over black
/// would show nothing of the guide's change.
const FX: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                  {\"type\":\"stroke\",\"size\":3,\"color\":\"#4080c0\"}]}";

const FILL: &str = "{\"fill_opacity\":0.5}";

const SHALLOW: &str = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}";

const SHALLOW_FX: &str = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\",\"effects\":\
                          [{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                          {\"type\":\"stroke\",\"size\":3}]}";

const DEEP: &str = "{\"fill_opacity\":0.6,\"knockout\":\"deep\"}";

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

/// The content rect of a raster at `offset` of `size`, within the canvas.
fn rect_of(offset: (i32, i32), size: (u32, u32), canvas: (u32, u32)) -> Window {
    let x0 = offset.0.max(0) as u32;
    let y0 = offset.1.max(0) as u32;
    let x1 = ((offset.0 + size.0 as i32).max(0) as u32).min(canvas.0);
    let y1 = ((offset.1 + size.1 as i32).max(0) as u32).min(canvas.1);
    (x0, y0, x1 - x0, y1 - y0)
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

/// The ROOT staged plan restated for clip units under STYLED bases over
/// `build(true)` (the guides visible) and `build(false)` (the guides
/// hidden): `guides` are (layer, radius, rows, through) from the first guide
/// to the last — `rows` counted from the guided row, or for a clip MEMBER
/// from its unit's FIRST row, to the next guided row — over `rows_0` rows
/// below the first guide; `holds` planes over `R_0` (one per distinct clip
/// unit holding a guided member and per pass-through group on a path) and,
/// with `background`, D5's Background snapshot one more. A style's own
/// scratch and preparation work (its planes, a materialization) are the same
/// with the guides hidden, so they are read from that plan — its work less
/// the accumulator fill and one pass per row over the output — and the
/// staged figures restated on top: `P_0`'s fill, each stage's rows over the
/// next plane (the last over the output), every guide, the output fill and
/// the holds' fills.
#[allow(clippy::too_many_arguments)]
fn assert_root_plan(
    build: &dyn Fn(bool) -> PrecisionDocument,
    guides: &[(usize, f32, u64, &[usize])],
    rows_0: u64,
    holds: u64,
    background: bool,
    window: Window,
    div: u32,
) {
    let d = build(true);
    let canvas = d.dimensions();
    let p = plan(&d, window, div).unwrap();
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
    if p["global_analysis"] == json!(false) {
        assert_eq!(p["required_input_region"], grown(window, halo, canvas));
    } else {
        assert_eq!(
            p["required_input_region"],
            region_json((0, 0, canvas.0, canvas.1))
        );
    }
    assert!(within(planes[0], window_of(&p["required_input_region"])));
    let bare = plan(&build(false), window, div).unwrap();
    assert_eq!(bare["stages"], json!([]));
    let rows = bare["layers"].as_array().unwrap().len() as u64;
    let extra = work(&bare) - pixels - pixels * rows;
    let r0 = planes[0];
    let mut expected_scratch = scratch(&bare) + (holds + u64::from(background)) * area(r0) * 16;
    let mut expected_work = extra + area(r0) + area(r0) * rows_0 + pixels + holds * area(r0);
    for (k, &(_, radius, rows, _)) in guides.iter().enumerate() {
        let (gs, gw) = guide_figures(radius, planes[k]);
        let evaluated = planes.get(k + 1).map_or(pixels, |&next| area(next));
        expected_scratch += area(planes[k]) * 16 + gs;
        expected_work += evaluated * rows + gw;
    }
    assert_eq!(scratch(&p), expected_scratch, "{window:?}/{div}");
    assert_eq!(work(&p), expected_work, "{window:?}/{div}");
}

const CW: u32 = 197;
const CH: u32 = 195;
const OFFSET: (i32, i32) = (31, 25);
const SIZE: (u32, u32) = (130, 120);
const PLANS: [(Window, u32); 2] = [((60, 50, 30, 20), 1), ((0, 0, CW, CH), 3)];

fn content() -> Window {
    rect_of(OFFSET, SIZE, (CW, CH))
}

/// A radius-`radius` guide CLIPPED to the unit below it, visible or hidden.
fn member_guide_shown(radius: f32, visible: bool) -> Layer {
    let params = json!({"shadows":{"amount":0.7,"tone":0.9},
        "highlights":{"amount":0.35,"tone":0.8},"radius":radius});
    let op = json!({"type":"adjust","op":"shadows_highlights","params":params,
        "evaluation":"precision_v1"});
    Layer::adjustment(
        "shadows_highlights",
        &op.to_string(),
        0.8,
        visible,
        &profile(),
    )
    .unwrap()
    .with_clipped(true)
}

/// Backdrop (0) at `backdrop_opacity` (1: a Background for D5), a holed
/// raster B (1) at `OFFSET` of `SIZE` with `blend` / `opacity` styled
/// `style`, and a radius-8 guide (2) CLIPPED to B, shown or hidden.
fn styled_unit(
    style: &str,
    blend: BlendMode,
    opacity: f32,
    backdrop_opacity: f32,
    shown: bool,
) -> PrecisionDocument {
    doc(
        CW,
        CH,
        vec![
            backdrop_at(CW, CH, backdrop_opacity),
            styled(raster(SIZE, OFFSET, 1, blend, opacity), style, (CW, CH)),
            member_guide_shown(8.0, shown),
        ],
    )
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_guide_under_a_fill_only_base_matches_at_u16() {
    let build = |shown| styled_unit(FILL, BlendMode::Overlay, 0.85, 1.0, shown);
    assert_regions_match(&build(true), content(), "fill-only");
    for (window, div) in PLANS {
        assert_root_plan(&build, &[(2, 8.0, 2, &[])], 2, 1, false, window, div);
    }
}

#[test]
fn a_guide_under_an_effects_base_matches_at_opacity_1_and_below_at_u16() {
    // Below opacity 1 the typed PACKAGE (one canvas-sized copy) is admitted:
    // under a stop the copy holds the unit buffer. Dissolve is never
    // packaged.
    for (what, blend, opacity) in [
        ("effects at 1", BlendMode::Overlay, 1.0),
        ("effects at 0.85 (the package)", BlendMode::Overlay, 0.85),
        ("effects at 0.85 in Dissolve", BlendMode::Dissolve, 0.85),
    ] {
        let build = |shown| styled_unit(FX, blend, opacity, 1.0, shown);
        assert_ne!(
            full(&build(true)),
            full(&build(false)),
            "{what}: the guide changes the projection"
        );
        assert_regions_match(&build(true), content(), what);
        for (window, div) in PLANS {
            assert_root_plan(&build, &[(2, 8.0, 2, &[])], 2, 1, false, window, div);
        }
    }
    let p = plan(
        &styled_unit(FX, BlendMode::Overlay, 0.85, 1.0, true),
        (60, 50, 30, 20),
        1,
    )
    .unwrap();
    assert_eq!(p["layers"][1]["style"], "effects");
    assert_eq!(p["layers"][2]["clip_base"], 1);
}

#[test]
fn knockout_bases_without_and_with_planes_match_at_u16() {
    // Backdrop at 0.97 (no Background).
    for (what, style, opacity) in [
        ("Shallow, no planes", SHALLOW, 0.85),
        ("Shallow with effects", SHALLOW_FX, 1.0),
        ("Shallow with effects, packaged", SHALLOW_FX, 0.85),
    ] {
        let build = |shown| styled_unit(style, BlendMode::Normal, opacity, 0.97, shown);
        assert_regions_match(&build(true), content(), what);
        for (window, div) in PLANS {
            assert_root_plan(&build, &[(2, 8.0, 2, &[])], 2, 1, false, window, div);
        }
    }
}

#[test]
fn a_deep_knockout_base_under_d5s_background_matches_at_u16() {
    for (what, style, opacity) in [
        ("Deep", DEEP, 0.9),
        ("Shallow at the root", SHALLOW, 0.9),
        ("Shallow with effects", SHALLOW_FX, 1.0),
    ] {
        let build = |shown| styled_unit(style, BlendMode::Normal, opacity, 1.0, shown);
        assert_regions_match(&build(true), content(), what);
        for (window, div) in PLANS {
            assert_root_plan(&build, &[(2, 8.0, 2, &[])], 2, 1, true, window, div);
        }
    }
}

#[test]
fn a_materialized_group_base_matches_at_u16() {
    // G (2) = { A (1) } with effects, at 1 and at 0.85 (packaged); the
    // guide (3) clipped to G.
    for opacity in [1.0, 0.85] {
        let build = |shown| {
            let g = Layer::isolated_normal(
                "Group",
                vec![raster(SIZE, OFFSET, 1, BlendMode::Overlay, 0.85)],
                opacity,
                true,
            )
            .unwrap();
            doc(
                CW,
                CH,
                vec![
                    backdrop(CW, CH),
                    styled(g, FX, (CW, CH)),
                    member_guide_shown(8.0, shown),
                ],
            )
        };
        assert_regions_match(&build(true), content(), &format!("group at {opacity}"));
        // rows_0: backdrop, A, G; the member stage: A, G and the guide.
        for (window, div) in PLANS {
            assert_root_plan(&build, &[(3, 8.0, 3, &[])], 3, 1, false, window, div);
        }
    }
}

#[test]
fn raster_members_below_and_above_the_guide_are_re_run_under_an_effects_base_at_u16() {
    // B (1) with effects at 0.85 (the package), a Multiply member (2), the
    // guide (3), a Screen member (4).
    let build = |shown| {
        doc(
            CW,
            CH,
            vec![
                backdrop(CW, CH),
                styled(
                    raster(SIZE, OFFSET, 1, BlendMode::Overlay, 0.85),
                    FX,
                    (CW, CH),
                ),
                member((70, 60), (60, 50), 2, BlendMode::Multiply, 0.9),
                member_guide_shown(8.0, shown),
                member((60, 70), (80, 70), 3, BlendMode::Screen, 0.8),
            ],
        )
    };
    assert_regions_match(&build(true), content(), "members below and above the guide");
    for (window, div) in PLANS {
        assert_root_plan(&build, &[(3, 8.0, 4, &[])], 3, 1, false, window, div);
    }
}

#[test]
fn two_guides_under_an_effects_base_resume_one_hold_at_u16() {
    let build = |shown| {
        doc(
            CW,
            CH,
            vec![
                backdrop(CW, CH),
                styled(
                    raster(SIZE, OFFSET, 5, BlendMode::Normal, 0.95),
                    FX,
                    (CW, CH),
                ),
                member_guide_shown(8.0, shown),
                member((70, 60), (60, 70), 6, BlendMode::Overlay, 0.9),
                member_guide_shown(16.0, shown),
            ],
        )
    };
    assert_regions_match(&build(true), content(), "two guides in one unit");
    for (window, div) in PLANS {
        assert_root_plan(
            &build,
            &[(2, 8.0, 3, &[]), (4, 16.0, 4, &[])],
            2,
            1,
            false,
            window,
            div,
        );
    }
}

#[test]
fn a_shallow_knockout_member_beside_the_guide_under_an_effects_base_matches_at_u16() {
    // Backdrop (0, 0.97), B (1) with effects, a Shallow member (2), the
    // guide (3); and a Shallow member above the guide.
    let build = |shown| {
        doc(
            CW,
            CH,
            vec![
                backdrop_at(CW, CH, 0.97),
                styled(
                    raster(SIZE, OFFSET, 1, BlendMode::Overlay, 1.0),
                    FX,
                    (CW, CH),
                ),
                styled(
                    member((70, 60), (60, 50), 2, BlendMode::Normal, 0.9),
                    SHALLOW,
                    (CW, CH),
                ),
                member_guide_shown(8.0, shown),
            ],
        )
    };
    assert_regions_match(&build(true), content(), "a Shallow member below the guide");
    for (window, div) in PLANS {
        assert_root_plan(&build, &[(3, 8.0, 3, &[])], 3, 1, false, window, div);
    }
    let above = doc(
        CW,
        CH,
        vec![
            backdrop_at(CW, CH, 0.97),
            styled(
                raster(SIZE, OFFSET, 1, BlendMode::Overlay, 1.0),
                FX,
                (CW, CH),
            ),
            member_guide(8.0),
            styled(
                member((60, 50), (70, 60), 4, BlendMode::Normal, 0.9),
                SHALLOW,
                (CW, CH),
            ),
        ],
    );
    assert_regions_match(&above, content(), "a Shallow member above the guide");
}

#[test]
fn a_masked_multiply_member_guide_under_an_effects_base_matches_at_u16() {
    let masked = guide_at(8.0, 0.7)
        .with_blend_mode(BlendMode::Multiply)
        .unwrap()
        .with_mask(Some(mask(content(), 6.0)))
        .with_clipped(true);
    let d = doc(
        CW,
        CH,
        vec![
            backdrop(CW, CH),
            styled(
                raster(SIZE, OFFSET, 1, BlendMode::Overlay, 0.85),
                FX,
                (CW, CH),
            ),
            masked,
        ],
    );
    assert_regions_match(
        &d,
        content(),
        "a feather-6 masked Multiply 0.7 member guide",
    );
    let p = plan(&d, (60, 50, 30, 20), 1).unwrap();
    assert_eq!(p["stages"][0]["layer"], 2);
    assert_eq!(p["layers"][2]["mask"], true);
}

#[test]
fn a_small_knockout_patch_feeds_the_guide_its_buffer_outside_the_rect_at_u16() {
    // Backdrop (0, 0.97), a SMALL Shallow knockout raster K (1) — 14×12 at
    // (90, 80) — a Multiply member (2) spilling far past it and a radius-8
    // guide (3), both clipped to K: outside K's rect the guide reads the
    // unit buffer (the members over transparency, clamped to 0).
    let patch = rect_of((90, 80), (14, 12), (CW, CH));
    let build = |shown| {
        doc(
            CW,
            CH,
            vec![
                backdrop_at(CW, CH, 0.97),
                styled(
                    raster((14, 12), (90, 80), 3, BlendMode::Normal, 0.9),
                    SHALLOW,
                    (CW, CH),
                ),
                member((80, 70), (60, 50), 4, BlendMode::Multiply, 0.9),
                member_guide_shown(8.0, shown),
            ],
        )
    };
    assert_ne!(
        full(&build(true)),
        full(&build(false)),
        "the guide changes the patch"
    );
    assert_regions_match(&build(true), patch, "a small knockout patch");
    for (window, div) in [((0, 0, CW, CH), 1), ((70, 60, 60, 50), 1)] {
        assert_root_plan(&build, &[(3, 8.0, 3, &[])], 3, 1, false, window, div);
    }
}

#[test]
fn a_styled_unit_inside_isolated_and_pass_through_levels_matches_at_u16() {
    let unit = |shown| {
        vec![
            styled(
                raster(SIZE, OFFSET, 1, BlendMode::Overlay, 0.85),
                FX,
                (CW, CH),
            ),
            member_guide_shown(8.0, shown),
        ]
    };
    let iso = doc(CW, CH, vec![backdrop(CW, CH), isolated(unit(true))]);
    assert_regions_match(&iso, content(), "a styled unit in an isolated group");
    let p = plan(&iso, (60, 50, 30, 20), 1).unwrap();
    assert_eq!(p["stages"][0]["level"], 3);
    assert_eq!(p["stages"][0]["layer"], 2);
    // Holds: the unit and the group (the typed arm always copies).
    for opacity in [1.0, 0.6] {
        let build = |shown| doc(CW, CH, vec![backdrop(CW, CH), pass(unit(shown), opacity)]);
        let what = format!("a styled unit in a pass-through at {opacity}");
        assert_regions_match(&build(true), content(), &what);
        for (window, div) in PLANS {
            assert_root_plan(&build, &[(2, 8.0, 3, &[3])], 2, 2, false, window, div);
        }
    }
}

#[test]
fn linear_light_evaluates_a_document_with_a_styled_member_guide_at_u16_regionally() {
    let d = styled_unit(FILL, BlendMode::Normal, 1.0, 1.0, true);
    let linear = d
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(full(&linear), full(&d), "the policy changes the projection");
    assert_regions_match(&linear, content(), "linear light");
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
