//! Phase 13A-4a: the 16-bit regional render over a guide-reading
//! Shadows/Highlights at the ROOT — staged over the planes the plan declares —
//! against the full-frame typed projection. The oracle is INDEPENDENT of the
//! regional code: `dr_precision_doc_render`'s projection read back as u16
//! codes (f32 bit patterns for float), cropped and top-left-cell sampled here
//! in plain index arithmetic (tolerance: EXACT, declared before measuring).
//! The plan figures are restated from the guide's own sizing rule, written
//! out independently. The u16 concurrent-cancel measurement lives in
//! `regional_byte_guide.rs` beside the 8-bit one.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::BlendMode;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::*;
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
    let out = if !p16.is_null() {
        unsafe { std::slice::from_raw_parts(p16, n) }
            .iter()
            .map(|&v| u32::from(v))
            .collect()
    } else {
        unsafe { &*image }
            .to_rgba32()
            .expect("an image with samples")
            .as_flattened()
            .iter()
            .map(|v| v.to_bits())
            .collect()
    };
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

/// The 13A-2 windows plus windows whose plane is a STRICT sub-window.
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

fn assert_regions_match(doc: &PrecisionDocument, what: &str) {
    let reference = full(doc);
    let (cw, ch) = doc.dimensions();
    for window in windows(cw, ch) {
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

/// A canvas-sized backdrop with transparent HOLES (small ones on a lattice
/// and one large block) and translucent pixels.
fn backdrop(cw: u32, ch: u32) -> Layer {
    let pixels = (0..cw * ch)
        .map(|i| {
            let (x, y) = (i % cw, i / cw);
            let block = x > cw / 5 && x < cw / 5 + cw / 3 && y > ch / 2 && y < ch / 2 + ch / 3;
            let a = if block || (x / 7 + y / 5) % 9 == 0 {
                0
            } else if (x + 2 * y) % 5 == 0 || y < ch / 8 {
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

fn image16(w: u32, h: u32, seed: u32) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                ((x ^ y) * 1999 % 65536 + seed) as u16,
                (9000 + (x * 3001 + y * 1777 + seed * 97) % 56000) as u16,
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, profile(), None).unwrap()
}

fn raster(
    name: &str,
    size: (u32, u32),
    offset: (i32, i32),
    mode: BlendMode,
    opacity: f32,
) -> Layer {
    Layer::raster(name, image16(size.0, size.1, 5), offset, opacity, true)
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
}

fn op(params: Value) -> String {
    json!({"type":"adjust","op":"shadows_highlights","params":params,"evaluation":"precision_v1"})
        .to_string()
}

fn guide(radius: f32) -> Layer {
    let params = json!({"shadows":{"amount":0.7,"tone":0.9},
        "highlights":{"amount":0.35,"tone":0.8},"radius":radius});
    Layer::adjustment("shadows_highlights", &op(params), 0.8, true, &profile()).unwrap()
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

/// Backdrop, a translucent Overlay raster, then a guide of `radius`.
fn layers(cw: u32, ch: u32, radius: f32) -> Vec<Layer> {
    vec![
        backdrop(cw, ch),
        raster(
            "Raster",
            (cw / 2, ch / 2),
            (cw as i32 / 4, ch as i32 / 5),
            BlendMode::Overlay,
            0.85,
        ),
        guide(radius),
    ]
}

fn mask(cw: u32, ch: u32, feather: f32) -> PrecisionMask {
    let (mw, mh) = (cw * 2 / 3, ch * 2 / 3);
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
    let mut properties = MaskProperties::at((cw as i32 / 6, ch as i32 / 5));
    properties.density = 0.9;
    properties.feather = feather;
    PrecisionMask::new(
        PrecisionPlane::from_u16(mw, mh, plane, None).unwrap(),
        properties,
        true,
    )
    .unwrap()
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

fn guide_figures(radius: f32, plane: Window) -> (u64, u64) {
    let meta = op(json!({"shadows":{"amount":0.7},"radius":radius}));
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
    let d = doc(cw, ch, layers(cw, ch, radius));
    let mut unguided = layers(cw, ch, radius);
    unguided.truncate(2);
    assert_ne!(full(&d), full(&doc(cw, ch, unguided)), "radius {radius}");
    assert_regions_match(&d, &format!("radius {radius}"));
    let window = (cw / 3 + 1, ch / 3 + 1, cw / 3 - 2, ch / 3 - 2);
    let p = plan(&d, window, 1).unwrap();
    let plane = plane_region(radius, window, (cw, ch));
    assert_eq!(
        p["stages"][0]["plane"],
        region_json(plane),
        "radius {radius}"
    );
    let strict = plane.0 > 0 && plane.1 > 0 && plane.0 + plane.2 < cw && plane.1 + plane.3 < ch;
    assert_eq!(strict, 3 * halo <= cw.min(ch), "radius {radius}: {plane:?}");
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_radius_4_guide_at_factor_1_matches_the_u16_projection() {
    assert_radius(53, 50, 4.0);
}

#[test]
fn a_radius_8_guide_at_factor_2_matches_the_u16_projection() {
    assert_radius(101, 98, 8.0);
}

#[test]
fn a_radius_16_guide_at_factor_4_matches_the_u16_projection() {
    assert_radius(197, 195, 16.0);
}

#[test]
fn a_radius_30_guide_at_factor_8_matches_the_u16_projection() {
    assert_radius(389, 387, 30.0);
}

#[test]
fn a_radius_120_guide_with_a_wide_grid_blur_matches_the_u16_projection() {
    let (_, _, halo) = geometry(120.0);
    assert_radius(3 * halo + 5, 3 * halo + 3, 120.0);
}

#[test]
fn a_radius_1000_guide_whose_plane_is_the_canvas_matches_the_u16_projection() {
    assert_radius(203, 157, 1000.0);
}

#[test]
fn a_masked_translucent_multiply_and_dissolve_guide_matches_at_u16() {
    let (cw, ch) = (101, 98);
    for feather in [0.0, 6.0] {
        let mut l = layers(cw, ch, 8.0);
        l[2] = guide(8.0).with_mask(Some(mask(cw, ch, feather)));
        let d = doc(cw, ch, l);
        assert_regions_match(&d, &format!("masked feather {feather}"));
        assert_eq!(
            plan(&d, (40, 40, 10, 10), 1).unwrap()["layers"][2]["mask"],
            true
        );
    }
    for (mode, opacity) in [
        (BlendMode::Normal, 0.7),
        (BlendMode::Multiply, 0.8),
        (BlendMode::Dissolve, 0.6),
    ] {
        let params = json!({"shadows":{"amount":0.7,"tone":0.9},"radius":8});
        let g = Layer::adjustment("shadows_highlights", &op(params), opacity, true, &profile())
            .unwrap()
            .with_blend_mode(mode)
            .unwrap()
            .with_mask(Some(mask(cw, ch, 6.0)));
        let mut l = layers(cw, ch, 8.0);
        l[2] = g;
        assert_regions_match(&doc(cw, ch, l), &format!("{mode:?} {opacity}"));
    }
}

#[test]
fn two_guides_with_a_raster_between_nest_their_planes_at_u16() {
    let (cw, ch) = (197, 195);
    let mut l = layers(cw, ch, 8.0);
    l.push(raster(
        "Between",
        (70, 60),
        (40, 50),
        BlendMode::SoftLight,
        0.9,
    ));
    l.push(guide(16.0));
    let d = doc(cw, ch, l);
    assert_regions_match(&d, "two guides");
    let window = (70, 70, 30, 20);
    let p = plan(&d, window, 1).unwrap();
    let stages = p["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0]["layer"], 2);
    assert_eq!(stages[1]["layer"], 4);
    let inner = plane_region(16.0, window, (cw, ch));
    assert_eq!(stages[1]["plane"], region_json(inner));
    assert_eq!(
        stages[0]["plane"],
        region_json(plane_region(8.0, inner, (cw, ch)))
    );
    assert!(within(&stages[1]["plane"], &stages[0]["plane"]));
    assert!(within(&stages[0]["plane"], &p["required_input_region"]));
}

#[test]
fn a_guide_above_a_knockout_background_matches_at_u16() {
    let (cw, ch) = (101, 98);
    let style = LayerStyle::from_json("{\"fill_opacity\":0.4,\"knockout\":\"shallow\"}").unwrap();
    let knocked = raster("Knockout", (60, 50), (20, 20), BlendMode::Normal, 0.9)
        .with_layer_style(Some(&style), (cw, ch))
        .unwrap();
    let d = doc(cw, ch, vec![backdrop(cw, ch), knocked, guide(8.0)]);
    assert_regions_match(&d, "knockout Background + guide");
    let window = (30, 30, 20, 20);
    let plain = doc(cw, ch, layers(cw, ch, 8.0));
    let r0 = plane_region(8.0, window, (cw, ch));
    let with = plan(&d, window, 1).unwrap()["scratch_heap_bytes"]
        .as_u64()
        .unwrap();
    let without = plan(&plain, window, 1).unwrap()["scratch_heap_bytes"]
        .as_u64()
        .unwrap();
    // The Background snapshot `S` is one more plane over R_0.
    assert_eq!(with - without, area(r0) * 16);
}

#[test]
fn a_baseless_clipped_guide_at_the_root_matches_at_u16() {
    let (cw, ch) = (101, 98);
    let l: Vec<Layer> = layers(cw, ch, 8.0)
        .into_iter()
        .map(|layer| layer.with_clipped(true))
        .collect();
    let d = doc(cw, ch, l);
    assert_regions_match(&d, "baseless clipped guide");
    assert_eq!(
        plan(&d, (10, 10, 20, 20), 1).unwrap()["stages"][0]["layer"],
        2
    );
}

#[test]
fn a_guided_base_over_a_visible_run_contributes_nothing_and_over_a_hidden_one_applies() {
    let (cw, ch) = (101, 98);
    let member = |visible: bool| {
        Layer::raster("Member", image16(50, 40, 7), (10, 10), 1.0, visible)
            .unwrap()
            .with_clipped(true)
    };
    let mut plain = layers(cw, ch, 8.0);
    plain.truncate(2);
    let plain = full(&doc(cw, ch, plain));
    // A visible member: the clip buffer stays transparent.
    let mut l = layers(cw, ch, 8.0);
    l.push(member(true));
    let d = doc(cw, ch, l);
    assert_eq!(full(&d), plain);
    assert_regions_match(&d, "guided base over a visible run");
    let p = plan(&d, (10, 10, 20, 20), 1).unwrap();
    assert_eq!(p["stages"], json!([]));
    assert_eq!(p["layers"][2]["neighborhood"]["kind"], "pointwise");
    // A hidden-only run is empty at typed depth: the lone arm, staged.
    let mut l = layers(cw, ch, 8.0);
    l.push(member(false));
    let d = doc(cw, ch, l);
    assert_ne!(full(&d), plain);
    assert_regions_match(&d, "guided base over a hidden-only run");
    assert_eq!(
        plan(&d, (10, 10, 20, 20), 1).unwrap()["stages"][0]["layer"],
        2
    );
}

#[test]
fn an_empty_isolated_group_below_a_guide_keeps_the_guides_record_index() {
    let (cw, ch) = (101, 98);
    let base = || {
        vec![
            backdrop(cw, ch),
            raster("Raster", (50, 40), (20, 20), BlendMode::Multiply, 0.8),
        ]
    };
    let mut l = base();
    l.push(Layer::isolated_normal("Empty", vec![], 1.0, true).unwrap());
    l.push(guide(8.0));
    let d = doc(cw, ch, l);
    assert_regions_match(&d, "empty group, no children");
    assert_eq!(
        plan(&d, (30, 30, 10, 10), 1).unwrap()["stages"][0]["layer"],
        3
    );
    let hidden = Layer::raster("Hidden", image16(30, 30, 9), (5, 5), 1.0, false).unwrap();
    let mut l = base();
    l.push(Layer::isolated_normal("Hidden kids", vec![hidden], 1.0, true).unwrap());
    l.push(guide(8.0));
    let d = doc(cw, ch, l);
    assert_regions_match(&d, "empty group, hidden children");
    assert_eq!(
        plan(&d, (30, 30, 10, 10), 1).unwrap()["stages"][0]["layer"],
        4
    );
}

#[test]
fn a_guided_entry_inside_an_isolated_group_and_as_a_member_matches_at_u16() {
    let (cw, ch) = (53, 50);
    // Since 13A-4b-i a guide in an ISOLATED group's level is staged.
    let grouped = vec![
        backdrop(cw, ch),
        Layer::isolated_normal("Group", vec![guide(4.0)], 1.0, true).unwrap(),
    ];
    assert_regions_match(&doc(cw, ch, grouped), "a guide in an isolated group");
    // Since 13A-4b-ii-b-1 a guided clip member of an unstyled base is staged
    // too (the unit re-run from its held began sample).
    let mut member = layers(cw, ch, 4.0);
    member[2] = guide(4.0).with_clipped(true);
    assert_regions_match(&doc(cw, ch, member), "a guide clipped to a plain raster");
}

#[test]
fn a_zero_amount_shadows_highlights_is_pointwise_at_u16() {
    let (cw, ch) = (53, 50);
    let mut l = layers(cw, ch, 4.0);
    let params = json!({"shadows":{"amount":0},"highlights":{"amount":0},"midtone_contrast":0.3});
    l[2] = Layer::adjustment("shadows_highlights", &op(params), 0.8, true, &profile()).unwrap();
    let d = doc(cw, ch, l);
    assert_regions_match(&d, "zero amounts");
    let p = plan(&d, (5, 5, 10, 10), 1).unwrap();
    assert_eq!(p["stages"], json!([]));
    assert_eq!(p["layers"][2]["neighborhood"]["kind"], "pointwise");
}

#[test]
fn the_typed_plan_restates_stages_border_policy_halo_scratch_and_work() {
    let (cw, ch) = (101, 98);
    let d = doc(cw, ch, layers(cw, ch, 8.0));
    let (f, cells, halo) = geometry(8.0);
    for (window, div) in [
        ((40, 40, 13, 9), 2),
        ((0, 0, cw, ch), 1),
        ((90, 3, 11, 20), 3),
    ] {
        let p = plan(&d, window, div).unwrap();
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
        assert_eq!(p["required_input_region"], grown(window, halo, (cw, ch)));
        let pixels = u64::from(window.2.div_ceil(div)) * u64::from(window.3.div_ceil(div));
        let (gs, gw) = guide_figures(8.0, r0);
        assert_eq!(
            p["scratch_heap_bytes"].as_u64().unwrap(),
            pixels * 16 + pixels * 8 + BOOKKEEPING + area(r0) * 16 + gs,
            "{window:?}"
        );
        // `P_0`'s fill, stage 0 (two rows) over R_0, the guide, stage 1 (one
        // row) over the output and the output fill.
        assert_eq!(
            p["work_units"].as_u64().unwrap(),
            area(r0) + area(r0) * 2 + gw + pixels + pixels,
            "{window:?}"
        );
    }
}

#[test]
fn the_typed_input_halo_composes_each_rows_halo_with_the_guides_above_it() {
    let (cw, ch) = (197, 195);
    let window = (80, 80, 20, 20);
    let (_, _, h4) = geometry(4.0);
    let (_, _, h8) = geometry(8.0);
    let feathered = || mask(cw, ch, 5.0);
    let pad_of = |p: &Value, row: usize| {
        p["layers"][row]["neighborhood"]["halo_pixels"]
            .as_u64()
            .unwrap() as u32
    };
    // A feather-P raster below a radius-4 guide: P + H.
    let mut l = layers(cw, ch, 4.0);
    l[1] = raster(
        "Raster",
        (cw / 2, ch / 2),
        (40, 30),
        BlendMode::Overlay,
        0.85,
    )
    .with_mask(Some(feathered()));
    let d = doc(cw, ch, l.clone());
    let p = plan(&d, window, 1).unwrap();
    let pad = pad_of(&p, 1);
    assert!(pad > 0);
    assert_eq!(p["neighborhood"]["halo_pixels"], pad + h4);
    assert_eq!(
        p["required_input_region"],
        grown(window, pad + h4, (cw, ch))
    );
    assert_regions_match(&d, "feathered raster below a guide");
    // A guided row with its own feather-P mask: max(P, H).
    let mut own = layers(cw, ch, 4.0);
    own[2] = guide(4.0).with_mask(Some(feathered()));
    let p = plan(&doc(cw, ch, own), window, 1).unwrap();
    assert_eq!(p["neighborhood"]["halo_pixels"], pad.max(h4));
    // Two guides with a feather-P raster between: max(P + H_2, H_1 + H_2).
    let mut two = layers(cw, ch, 4.0);
    two.push(
        raster("Between", (70, 60), (40, 50), BlendMode::Screen, 0.9).with_mask(Some(feathered())),
    );
    two.push(guide(8.0));
    let two = doc(cw, ch, two);
    let p = plan(&two, window, 1).unwrap();
    assert_eq!(p["neighborhood"]["halo_pixels"], (pad + h8).max(h4 + h8));
    assert_regions_match(&two, "two guides, a feathered raster between");
    // A pure guide pair: H_1 + H_2.
    let mut pair = layers(cw, ch, 4.0);
    pair.push(guide(8.0));
    let p = plan(&doc(cw, ch, pair), window, 1).unwrap();
    assert_eq!(p["neighborhood"]["halo_pixels"], h4 + h8);
    assert_eq!(p["required_input_region"], grown(window, h4 + h8, (cw, ch)));
}

#[test]
fn linear_light_evaluates_a_guided_u16_document_regionally() {
    let d = doc(53, 50, layers(53, 50, 4.0));
    let linear = d
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(full(&linear), full(&d), "the policy changes the projection");
    assert_regions_match(&linear, "linear light");
}

#[test]
fn a_pre_cancelled_guided_u16_render_holds_no_scratch() {
    let d = doc(101, 98, layers(101, 98, 8.0));
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_render_region(&d, 0, 0, 101, 98, 1, LIMIT, 0, control, &mut err)
    };
    assert!(out.is_null());
    assert_eq!(take(err), "operation cancelled");
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe { dr_job_control_free(control) };
}

#[test]
fn the_float_policy_refuses_the_guide_and_plan_render_and_full_render_agree() {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let fixture = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    // The layer is added, but the float policy refuses the guided operator
    // with its own sentence at the full render, the plan and the render.
    let params = json!({"shadows":{"amount":0.7},"radius":30});
    let guided = fixture
        .add_adjustment("S/H", &op(params), 0.8, LIMIT, 0, None)
        .unwrap()
        .expect("an adjustment");
    let sentence = "the shadows_highlights adjustment is unavailable in a 32-bit float document";
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(&guided, LIMIT, 0, ptr::null(), &mut err) };
    assert!(image.is_null(), "the full render must refuse");
    assert_eq!(take(err), sentence);
    assert_eq!(plan(&guided, (0, 0, 4, 4), 1).unwrap_err(), sentence);
    assert_eq!(region(&guided, (0, 0, 4, 4), 1).unwrap_err(), sentence);
    assert_eq!(plan(&fixture, (0, 0, 4, 4), 1).unwrap()["depth"], "rgba32f");
    assert_regions_match(&fixture, "float fixture");
}
