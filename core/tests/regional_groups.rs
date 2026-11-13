//! Phase 13A-3a: the regional document render over GROUPS at 8 bits —
//! isolated and pass-through, plain and gated, nested to the depth cap, as
//! lone entries, clip bases, clip members and baseless clipped entries,
//! hidden and empty — against the full-frame projection. The oracle is
//! INDEPENDENT of the regional code, exactly as in `regional_byte_styles.rs`:
//! `dr_doc_flattened`'s projection cropped and top-left-cell sampled here in
//! plain index arithmetic, byte for byte. The two concurrent-cancel
//! measurements (8 bits and u16, one nested group each) live here too, as
//! `#[ignore]`d `…_is_measured` twins, so `make cancel-bench`
//! records both depths; their default twins cancel with the worker parked.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument, LayerKind, LayerRaster};
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_precision::dr_precision_image_free;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::Value;

use crate::common;
use common::{adjust_meta, img_dims, img_pixels, plasma, styled, take_err_string};

use crate::region_plan;

const W: u32 = 29;
const H: u32 = 19;

const BLEND_IF_GRAY: &str =
    "\"blend_if\":{\"channel\":\"gray\",\"this_layer\":[10,60,180,240],\"underlying\":[0,40,200,255]}";
const DROP_SHADOW: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}";

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

/// The canvas windows every case is checked at — edges, corners, interior,
/// 1×1, the whole canvas — plus `extra` (windows straddling a mask edge).
fn assert_regions_match_with(doc: &DrDocument, what: &str, extra: &[(u32, u32, u32, u32)]) {
    let reference = full(doc);
    let (cw, ch) = (doc.width, doc.height);
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
        (13, 9, 1, 1),
    ];
    windows.extend_from_slice(extra);
    for window in windows {
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

fn assert_regions_match(doc: &DrDocument, what: &str) {
    assert_regions_match_with(doc, what, &[]);
}

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

fn opaque() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        let c = plasma(x, y);
        Rgba([c[0], c[1], c[2], 255])
    }))
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

/// A raster added at the top of the stack (top level).
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

/// `group_layers` over `idx` (a new Pass Through group), and the group's
/// index.
fn group(doc: &DrDocument, idx: &[usize], name: &str) -> (DrDocument, usize) {
    let (out, g, ..) = doc.group_layers(idx, name).expect("group");
    (out, g)
}

/// An EMPTY top-level group entry appended at the top.
fn with_empty_group(doc: &DrDocument, name: &str) -> DrDocument {
    let mut out = doc.clone();
    let mut entry = doc.layers[0].clone();
    entry.kind = LayerKind::Group;
    entry.name = name.into();
    entry.pixels = LayerRaster::new(RgbaImage::new(1, 1));
    entry.offset = (0, 0);
    entry.blend = BlendMode::PassThrough;
    entry.opacity = 1.0;
    entry.visible = true;
    entry.mask = None;
    entry.meta = None;
    entry.style = None;
    entry.clipped = false;
    entry.depth = 0;
    out.layers.push(entry);
    out
}

/// A gradient coverage plane with a hard hole, so every coverage level and
/// both endpoints occur.
fn gradient(w: u32, h: u32) -> GrayImage {
    GrayImage::from_fn(w, h, |x, y| {
        if x == w / 2 && y == h / 2 {
            Luma([0])
        } else {
            Luma([((x * 255) / (w - 1).max(1)) as u8 ^ (y * 9) as u8])
        }
    })
}

/// Sets layer `idx`'s mask to `plane` at `offset` with the given properties.
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

/// Windows straddling the edges of a rect at `offset` of size `w`×`h`,
/// clamped into the canvas.
fn around(offset: (i32, i32), w: u32, h: u32) -> Vec<(u32, u32, u32, u32)> {
    let clamp = |v: i64, hi: u32| v.clamp(0, i64::from(hi) - 1) as u32;
    let mut out = Vec::new();
    for (cx, cy) in [
        (i64::from(offset.0), i64::from(offset.1)),
        (i64::from(offset.0) + i64::from(w), i64::from(offset.1)),
        (i64::from(offset.0), i64::from(offset.1) + i64::from(h)),
        (
            i64::from(offset.0) + i64::from(w),
            i64::from(offset.1) + i64::from(h),
        ),
    ] {
        let x = clamp(cx - 3, W);
        let y = clamp(cy - 2, H);
        out.push((x, y, (W - x).min(6), (H - y).min(5)));
    }
    out
}

/// Background (0), raster A Multiply 0.85 at (3, 2) 19×13 (1), raster B
/// Screen 0.8 at (10, 5) 15×12 (2), a hue/saturation adjustment (3) and a
/// levels adjustment (4).
fn stack() -> DrDocument {
    let doc = with_raster(
        &base(),
        top_pixels(19, 13),
        (3, 2),
        BlendMode::Multiply,
        0.85,
    );
    let doc = with_raster(&doc, top_pixels(15, 12), (10, 5), BlendMode::Screen, 0.8);
    let doc = with_adjustment(&doc, "hue_saturation", "{\"hue\":-60,\"saturation\":0.2}");
    with_adjustment(&doc, "levels", "{\"black\":0.1,\"gamma\":1.6}")
}

fn column<'a>(p: &'a Value, key: &str) -> Vec<&'a Value> {
    p["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| &l[key])
        .collect()
}

fn rows(p: &Value) -> Vec<u64> {
    column(p, "index")
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect()
}

fn at(x: u32, y: u32) -> usize {
    ((y * W + x) * 4) as usize
}

// ----------------------------------------------------- pass through --

#[test]
fn pass_through_groups_plain_and_gated_match_the_projection() {
    let (doc, g) = group(&stack(), &[1, 2, 3, 4], "Pass");
    assert_eq!(g, 5);
    assert_eq!(doc.layers[g].blend, BlendMode::PassThrough);
    // The adjustment children reach the backdrop BELOW the group: a pixel
    // outside both rasters moves, which isolating the group would stop.
    let outside = at(1, 17);
    assert_ne!(
        full(&doc)[outside..outside + 3],
        full(&base())[outside..outside + 3]
    );
    assert_regions_match(&doc, "plain pass-through");
    let mut half = doc.clone();
    half.layers[g].opacity = 0.5;
    assert_regions_match(&half, "pass-through at opacity 0.5");
    let gated = masked(&doc, g, gradient(24, 15), (2, 1), 0.5, 6.0);
    assert_regions_match_with(&gated, "pass-through masked", &around((2, 1), 24, 15));
    let mut both = gated.clone();
    both.layers[g].opacity = 0.5;
    assert_regions_match_with(&both, "pass-through masked at 0.5", &around((2, 1), 24, 15));
    // The plan: the group row after its children's, a feathered group mask
    // declares its halo.
    let p = plan(&both, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3, 4, 5]);
    let row = &p["layers"][5];
    assert_eq!(row["kind"], "group");
    assert_eq!(row["operator"], "pass_through");
    assert_eq!(row["mask"], true);
    assert_eq!(row["group"], Value::Null);
    assert_eq!(row["neighborhood"]["kind"], "spatial");
    let halo = row["neighborhood"]["halo_pixels"].as_u64().unwrap();
    assert!(halo > 0);
    assert_eq!(p["neighborhood"]["halo_pixels"], halo);
    for child in 1..5 {
        assert_eq!(p["layers"][child]["group"], 5, "child {child}");
    }
    assert_eq!(p["group_depth"], 1);
}

// ---------------------------------------------------------- isolated --

#[test]
fn isolated_groups_in_three_modes_match_the_projection() {
    let (doc, g) = group(&stack(), &[1, 2, 3, 4], "Iso");
    for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Dissolve] {
        for opacity in [1.0, 0.7] {
            let mut d = doc.clone();
            d.layers[g].blend = mode;
            d.layers[g].opacity = opacity;
            assert_regions_match(&d, &format!("isolated {mode:?} {opacity}"));
            let p = plan(&d, (0, 0, 4, 4), 1).unwrap();
            assert_eq!(p["layers"][5]["operator"], "isolated");
        }
    }
    // The hue and levels children reach only the group's OWN pixels: outside
    // both rasters the projection is the background's.
    let mut iso = doc.clone();
    iso.layers[g].blend = BlendMode::Normal;
    let outside = at(1, 17);
    assert_eq!(
        full(&iso)[outside..outside + 4],
        full(&base())[outside..outside + 4]
    );
    for feather in [0.0, 6.0] {
        let m = masked(&iso, g, gradient(20, 14), (4, 3), 0.8, feather);
        let mut m = m;
        m.layers[g].opacity = 0.7;
        assert_regions_match_with(
            &m,
            &format!("masked isolated feather {feather}"),
            &around((4, 3), 20, 14),
        );
    }
    // A masked child inside the group.
    let child = masked(&iso, 2, gradient(15, 12), (10, 5), 0.6, 6.0);
    assert_regions_match_with(&child, "masked child", &around((10, 5), 15, 12));
}

// ------------------------------------------------------------ nesting --

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

/// Inner (Multiply, isolated) ⊂ Middle (Pass Through, 0.6, gated) ⊂ Outer
/// (Normal, isolated, 0.9, masked).
fn nested() -> DrDocument {
    let (doc, inner) = group(&stack(), &[1, 2], "Inner");
    assert_eq!(inner, 3);
    let mut doc = doc;
    doc.layers[inner].blend = BlendMode::Multiply;
    let (doc, middle) = group(&doc, &[3, 4], "Middle");
    assert_eq!(middle, 5);
    let mut doc = doc;
    doc.layers[middle].opacity = 0.6;
    let (doc, outer) = group(&doc, &[5, 6], "Outer");
    assert_eq!(outer, 7);
    let mut doc = masked(&doc, outer, gradient(20, 14), (4, 3), 0.8, 0.0);
    doc.layers[outer].blend = BlendMode::Normal;
    doc.layers[outer].opacity = 0.9;
    doc
}

#[test]
fn groups_nested_three_deep_mixing_kinds_match_and_plan_their_rows() {
    let doc = nested();
    assert_regions_match_with(&doc, "nested three deep", &around((4, 3), 20, 14));
    let window = (4, 3, 9, 7);
    let p = plan(&doc, window, 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3, 4, 5, 6, 7]);
    let groups: Vec<Option<u64>> = column(&p, "group").iter().map(|v| v.as_u64()).collect();
    assert_eq!(
        groups,
        [
            None,
            Some(3),
            Some(3),
            Some(5),
            Some(5),
            Some(7),
            Some(7),
            None
        ]
    );
    let kinds: Vec<&str> = column(&p, "kind")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "raster",
            "raster",
            "raster",
            "group",
            "adjustment",
            "group",
            "adjustment",
            "group"
        ]
    );
    let operators: Vec<&str> = column(&p, "operator")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        operators,
        [
            "composite",
            "composite",
            "composite",
            "isolated",
            "hue_saturation",
            "pass_through",
            "levels",
            "isolated"
        ]
    );
    assert!(column(&p, "clip_base").iter().all(|v| v.is_null()));
    assert_eq!(p["group_depth"], 3);
    assert_eq!(p["contract_version"], 1);
    let pixels = 9u64 * 7;
    assert_eq!(
        p["scratch_heap_bytes"],
        pixels * 16 + pixels * 4 + description_parse(&doc)
    );
    assert_eq!(p["work_units"], pixels * (8 + 1));
    // A hidden subtree drops its rows and its nesting.
    let mut hidden = doc.clone();
    hidden.layers[5].visible = false;
    let p = plan(&hidden, window, 1).unwrap();
    assert_eq!(rows(&p), [0, 6, 7]);
    assert_eq!(p["group_depth"], 1);
    assert_regions_match(&hidden, "a hidden middle group");
}

#[test]
fn groups_nested_to_the_depth_cap_match_the_projection() {
    let doc = with_raster(
        &base(),
        top_pixels(17, 12),
        (5, 3),
        BlendMode::Multiply,
        0.9,
    );
    let mut doc = with_adjustment(&doc, "hue_saturation", "{\"hue\":40,\"saturation\":0.3}");
    for level in 0..10 {
        let top = doc.layers.len() - 1;
        let lo = if level == 0 { 1 } else { top };
        let idx: Vec<usize> = (lo..=top).collect();
        let (next, g) = group(&doc, &idx, &format!("G{level}"));
        doc = next;
        match level % 3 {
            0 => {}
            1 => doc.layers[g].blend = BlendMode::Multiply,
            _ => doc.layers[g].opacity = 0.8,
        }
    }
    let top = doc.layers.len() - 1;
    assert!(
        doc.group_layers(&[top], "Eleven").is_none(),
        "the nesting cap"
    );
    assert_eq!(doc.layers.iter().map(|l| l.depth).max(), Some(10));
    assert_regions_match(&doc, "ten nested groups");
    assert_eq!(plan(&doc, (0, 0, W, H), 1).unwrap()["group_depth"], 10);
}

// --------------------------------------------------------- clip units --

/// Background (0), A (1, d1) and a hue adjustment (2, d1) in group G (3),
/// and a raster C clipped to G (4).
fn clip_base_group() -> DrDocument {
    let doc = with_raster(&base(), top_pixels(19, 13), (3, 2), BlendMode::Normal, 0.9);
    let doc = with_adjustment(&doc, "hue_saturation", "{\"hue\":-60,\"saturation\":0.2}");
    let (doc, g) = group(&doc, &[1, 2], "G");
    assert_eq!(g, 3);
    let mut doc = with_raster(&doc, top_pixels(15, 12), (8, 6), BlendMode::Multiply, 0.8);
    doc.layers[4].clipped = true;
    doc
}

#[test]
fn a_group_clip_base_isolates_for_a_visible_member() {
    let doc = clip_base_group();
    assert_regions_match_with(&doc, "group clip base", &around((8, 6), 15, 12));
    let p = plan(&doc, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3, 4]);
    assert_eq!(p["layers"][3]["operator"], "isolated");
    assert_eq!(p["layers"][4]["clip_base"], 3);
    assert_eq!(p["layers"][4]["group"], Value::Null);
    // A Multiply base at 0.7 with a masked member.
    let mut moded = masked(&doc, 4, gradient(15, 12), (8, 6), 0.7, 6.0);
    moded.layers[3].blend = BlendMode::Multiply;
    moded.layers[3].opacity = 0.7;
    assert_regions_match_with(&moded, "multiply group base", &around((8, 6), 15, 12));
    // A GROUP member, Pass Through, holding a raster and a levels child: a
    // member always composites isolated (Normal).
    let doc = with_adjustment(&doc, "levels", "{\"gamma\":1.8}");
    let (mut doc, m) = group(&doc, &[4, 5], "M");
    assert_eq!(m, 6);
    doc.layers[m].clipped = true;
    assert_regions_match(&doc, "group member");
    let p = plan(&doc, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(p["layers"][6]["operator"], "isolated");
    assert_eq!(p["layers"][6]["clip_base"], 3);
    assert_eq!(p["layers"][4]["group"], 6);
    assert_eq!(p["layers"][4]["clip_base"], Value::Null);
    // The member hidden: the base returns to Pass Through and still equals
    // the reference (drive gate G3).
    let mut hidden = doc.clone();
    hidden.layers[m].visible = false;
    assert_regions_match(&hidden, "hidden group member");
    let p = plan(&hidden, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3]);
    assert_eq!(p["layers"][3]["operator"], "pass_through");
}

/// The objection's fixture: an opaque backdrop, a Pass Through group P
/// holding an Invert adjustment, and a HIDDEN raster clipped to P. The
/// positional run is non-empty, but the reference keeps P pass-through and
/// never takes the private-buffer path — so the backdrop IS inverted.
#[test]
fn a_pass_through_base_whose_only_member_is_hidden_keeps_pass_through() {
    let doc = with_adjustment(&opaque(), "invert", "{}");
    let (doc, p_idx) = group(&doc, &[1], "P");
    assert_eq!(p_idx, 2);
    let mut doc = with_raster(&doc, top_pixels(15, 12), (8, 6), BlendMode::Normal, 1.0);
    doc.layers[3].clipped = true;
    doc.layers[3].visible = false;
    let backdrop = full(&opaque());
    let projection = full(&doc);
    for (got, was) in projection.chunks(4).zip(backdrop.chunks(4)) {
        for c in 0..3 {
            assert_eq!(got[c], 255 - was[c], "inverted");
        }
        assert_eq!(got[3], 255);
    }
    assert_regions_match(&doc, "hidden-only member, opacity 1");
    let p = plan(&doc, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2]);
    assert_eq!(p["layers"][2]["operator"], "pass_through");
    let mut half = doc.clone();
    half.layers[p_idx].opacity = 0.5;
    let projection = full(&half);
    for got in projection.chunks(4) {
        for &c in &got[..3] {
            assert!((127..=128).contains(&c), "half inverted: {c}");
        }
    }
    assert_regions_match(&half, "hidden-only member, opacity 0.5");
}

#[test]
fn baseless_clipped_groups_composite_as_if_unclipped() {
    // At the root: [bg, hue](Bottom, clipped, baseless) then a raster.
    let doc = with_adjustment(
        &base(),
        "hue_saturation",
        "{\"hue\":-60,\"saturation\":0.2}",
    );
    let (mut doc, bottom) = group(&doc, &[0, 1], "Bottom");
    assert_eq!(bottom, 2);
    doc.layers[bottom].clipped = true;
    let doc = with_raster(&doc, top_pixels(15, 12), (8, 6), BlendMode::Screen, 0.8);
    assert_regions_match(&doc, "baseless root group");
    let p = plan(&doc, (0, 0, W, H), 1).unwrap();
    assert_eq!(p["layers"][2]["operator"], "pass_through");
    assert_eq!(p["layers"][2]["clip_base"], Value::Null);
    // Inside a group's level: the clipped group at the bottom of Outer
    // (`group_layers` clears the bottom entry's flag, so it is set again).
    let (mut doc, outer) = group(&doc, &[2, 3], "Outer");
    doc.layers[2].clipped = true;
    doc.layers[outer].blend = BlendMode::Normal;
    assert_regions_match(&doc, "baseless group inside a group");
    let p = plan(&doc, (0, 0, W, H), 1).unwrap();
    assert_eq!(p["layers"][2]["operator"], "pass_through");
    assert_eq!(p["layers"][2]["group"], outer);
}

// ---------------------------------------------------- hidden, empty --

#[test]
fn a_hidden_group_skips_its_subtree_unchecked() {
    let (doc, g) = group(&stack(), &[1, 2], "G");
    let styled_child = styled(&doc, 1, DROP_SHADOW);
    // Visible, the styled child is admitted since 13A-3b-i…
    assert_regions_match(&styled_child, "visible group, styled child");
    // …hidden, the group's whole subtree is skipped unchecked.
    let mut hidden = styled_child.clone();
    hidden.layers[g].visible = false;
    assert_regions_match(&hidden, "hidden group, styled child");
    let p = plan(&hidden, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 4, 5]);
    assert_eq!(p["group_depth"], 0);
    // At opacity 0 too.
    let mut zero = styled_child.clone();
    zero.layers[g].opacity = 0.0;
    assert_regions_match(&zero, "zero-opacity group, styled child");
}

#[test]
fn empty_groups_match_the_projection() {
    let doc = with_empty_group(&stack(), "Empty");
    let e = doc.layers.len() - 1;
    assert_regions_match(&doc, "empty pass-through");
    let mut iso = doc.clone();
    iso.layers[e].blend = BlendMode::Multiply;
    assert_regions_match(&iso, "empty isolated");
    let p = plan(&iso, (0, 0, W, H), 1).unwrap();
    assert_eq!(p["layers"][5]["kind"], "group");
    // An empty clip base with a visible member: the unit contributes nothing.
    let mut based = with_raster(&doc, top_pixels(15, 12), (8, 6), BlendMode::Normal, 1.0);
    based.layers[e + 1].clipped = true;
    assert_eq!(full(&based), full(&doc));
    assert_regions_match(&based, "empty clip base");
    // An empty group member.
    let mut member = with_empty_group(&stack(), "EmptyMember");
    let m = member.layers.len() - 1;
    member.layers[m].clipped = true;
    assert_regions_match(&member, "empty clip member");
}

/// A styled base (gray 204, alpha 143, mask 115) over opaque black — the
/// 13A-2b clamp fixture — with a GROUP clipped above it. A group member
/// renders to NOTHING when its extent is empty (`rendered_group`'s `None`)
/// and is then dropped from the loop, so no clamp runs (52); one with an
/// extent elsewhere on the canvas is a real step and the clamp runs (51).
#[test]
fn an_empty_extent_group_member_is_dropped_under_a_styled_base() {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 255])));
    let doc = with_raster(
        &doc,
        RgbaImage::from_pixel(19, 13, Rgba([204, 204, 204, 143])),
        (3, 2),
        BlendMode::Normal,
        1.0,
    );
    let doc = masked(
        &doc,
        1,
        GrayImage::from_pixel(19, 13, Luma([115])),
        (3, 2),
        1.0,
        0.0,
    );
    let doc = styled(&doc, 1, "{\"blend_if\":{\"underlying\":[0,0,200,255]}}");
    let probe = at(5, 4);
    // (label, the member group's raster offset — none for an EMPTY group —,
    // the expected projection at the probe).
    let cases = [
        ("empty", None, 52u8),
        ("off-canvas", Some((40, 30)), 52),
        ("elsewhere", Some((24, 15)), 51),
    ];
    for (label, offset, value) in cases {
        let mut d = match offset {
            None => with_empty_group(&doc, "M"),
            Some(offset) => {
                let d = with_raster(&doc, top_pixels(3, 3), offset, BlendMode::Normal, 1.0);
                group(&d, &[2], "M").0
            }
        };
        let m = d.layers.len() - 1;
        d.layers[m].clipped = true;
        assert_eq!(full(&d)[probe..probe + 3], [value; 3], "{label}");
        assert_regions_match(&d, label);
    }
}

// ------------------------------------------------------------- styles --

#[test]
fn effect_less_styled_groups_match_lone_and_as_a_clip_base() {
    let fill_blend_if = format!("{{\"fill_opacity\":0.5,{BLEND_IF_GRAY}}}");
    let (doc, g) = group(&stack(), &[1, 2, 3, 4], "Styled");
    let lone = styled(&doc, g, &fill_blend_if);
    assert_regions_match(&lone, "styled group");
    let p = plan(&lone, (0, 0, W, H), 1).unwrap();
    assert_eq!(p["layers"][5]["operator"], "isolated", "a style isolates");
    assert_eq!(p["layers"][5]["blend_if"], true);
    let zero = styled(&doc, g, "{\"fill_opacity\":0}");
    assert_regions_match(&zero, "fill 0 group");
    let masked_lone = masked(&lone, g, gradient(20, 14), (4, 3), 0.8, 6.0);
    assert_regions_match_with(&masked_lone, "masked styled group", &around((4, 3), 20, 14));
    // As a clip base: the member is clamped to the group's SHAPE.
    let mut based = with_raster(&lone, top_pixels(15, 12), (8, 6), BlendMode::Multiply, 0.8);
    based.layers[6].clipped = true;
    assert_regions_match_with(&based, "styled group base", &around((8, 6), 15, 12));
    let mut zero_member = based.clone();
    zero_member.layers[6].opacity = 0.0;
    assert_regions_match(&zero_member, "styled group base, clamp-only step");
}

// ------------------------------------------------------- linear light --

#[test]
fn a_linear_light_document_with_a_nested_multiply_group_matches() {
    let (doc, inner) = group(&stack(), &[1, 2], "Inner");
    let mut doc = doc;
    doc.layers[inner].blend = BlendMode::Multiply;
    let (doc, _) = group(&doc, &[3, 4], "Outer");
    let linear = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(
        full(&linear),
        full(&doc),
        "the policy changes the projection"
    );
    assert_regions_match(&linear, "linear light groups");
}

// ------------------------------------------------------------ refusals --

#[test]
fn guided_children_knockout_children_and_styled_groups_are_admitted() {
    let (doc, g) = group(&stack(), &[1, 2, 3, 4], "Group");
    assert_eq!(g, 5);
    // A group with an enabled effect is admitted since 13A-3b-ii (the
    // materialized group).
    assert_regions_match(&styled(&doc, g, DROP_SHADOW), "a group with effects");
    // A knockout child is admitted since 13A-3c (Shallow inside a
    // pass-through group punches to the level's own starting sample).
    let knockout = styled(&doc, 2, "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}");
    assert_regions_match(&knockout, "a knockout child");
    // A guided Shadows/Highlights inside a (Pass Through) group: since
    // 13A-4b-ii-a staged by the root level.
    let guided = with_adjustment(
        &base(),
        "shadows_highlights",
        "{\"shadows\":{\"amount\":0.6,\"tone\":0.4},\"radius\":12}",
    );
    let (guided, _) = group(&guided, &[1], "Group");
    assert_regions_match(&guided, "a guided child of a Pass Through group");
    // ORDER, re-targeted: a knockout MEMBER of the group is admitted; its
    // visible member ISOLATES the group, so since 13A-4b-i the guided child
    // is staged in the group's level and equals the reference.
    let mut ordered = with_raster(&knockout, top_pixels(9, 9), (4, 4), BlendMode::Normal, 1.0);
    ordered.layers[6].clipped = true;
    let ordered = styled(&ordered, 6, "{\"knockout\":\"deep\"}");
    assert_regions_match(&ordered, "a knockout member of the group");
    let mut guided_child = ordered.clone();
    guided_child.layers[3].meta = Some(adjust_meta(
        "shadows_highlights",
        "{\"shadows\":{\"amount\":0.6,\"tone\":0.4},\"radius\":12}",
    ));
    assert_regions_match(&guided_child, "a guided child of an isolated clip base");
}

#[test]
fn a_group_past_the_reference_nesting_limit_refuses_in_its_words() {
    // Ten nested groups, and an EMPTY group at depth 10 inside the
    // innermost: the reference refuses it, and so do the plan and render.
    let mut doc = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Normal, 1.0);
    for level in 0..10 {
        let top = doc.layers.len() - 1;
        doc = group(&doc, &[top], &format!("G{level}")).0;
    }
    let innermost = doc
        .layers
        .iter()
        .position(|l| l.kind == LayerKind::Group)
        .unwrap();
    let mut entry = doc.layers[innermost].clone();
    entry.depth = 10;
    entry.name = "Deep".into();
    doc.layers.insert(innermost, entry);
    let reference = full_result(&doc).unwrap_err();
    assert_eq!(reference, "rendering exceeds the group nesting limit");
    assert_eq!(plan(&doc, (0, 0, 4, 4), 1).unwrap_err(), reference);
    assert_eq!(region(&doc, (0, 0, 4, 4), 1).unwrap_err(), reference);
}

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_grouped_render_returns_no_output() {
    let doc = nested();
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

/// The 8-bit concurrent cancel. With `bench`, a wall-clock record:
/// DECLARED BOUND (before measuring): a cancel returns within 4096 ROOT
/// samples — one checkpoint of the window loop (run-major within a row since
/// 13A-latency, a run never crossing a checkpoint), each root sample
/// evaluating every row. Measured: the cancel-to-return latency and, from an
/// uncancelled render of the same document, the time per root sample; their
/// quotient estimates the root samples processed after the cancel, printed
/// for the record (`--nocapture`). Without it, the load-independent
/// default: the loop's first report's checkpoint count comes from a fresh
/// twin's own report log and the worker is cancelled PARKED at the next
/// checkpoint (`cancelled_past_each`), no poller.
fn cancel_grouped_render(bench: bool) {
    let (w, h) = (3000u32, 2000u32);
    let build = || {
        let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
            Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
        }));
        for i in 0..3u8 {
            doc = with_raster(
                &doc,
                RgbaImage::from_pixel(w, h, Rgba([i * 60, 90, 200 - i * 40, 200])),
                (i32::from(i), 0),
                BlendMode::SoftLight,
                0.9,
            );
        }
        // One nested group: Inner (Multiply, isolated) inside Outer (Pass
        // Through at 0.9, gated).
        let (d, inner) = group(&doc, &[2, 3], "Inner");
        let mut doc = d;
        doc.layers[inner].blend = BlendMode::Multiply;
        let (d, outer) = group(&doc, &[1, inner], "Outer");
        let mut doc = d;
        doc.layers[outer].opacity = 0.9;
        doc
    };
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
    let doc = build();
    // 13A-5b-iii-b-2: the fill reports, so the loop has started once progress
    // passes the plan's loop start.
    let start = common::probe::loop_start(&plan(&doc, (0, 0, w, h), 1).unwrap());
    let hd = handle(&doc);
    let render = render_at(hd as usize);
    if bench {
        let started = Instant::now();
        let img = unsafe {
            dr_doc_render_region(hd, 0, 0, w, h, 1, u64::MAX, ptr::null(), ptr::null_mut())
        };
        let uncancelled = started.elapsed();
        assert!(!img.is_null());
        unsafe { dr_image_free(img) };
        let per_sample = uncancelled.as_secs_f64() / (f64::from(w) * f64::from(h));
        let control = unsafe { dr_job_control_create() };
        let token = control as usize;
        let latency = thread::scope(|scope| {
            let worker = scope.spawn(move || {
                let (ok, message) = render(token);
                (ok, message, Instant::now())
            });
            let deadline = Instant::now() + Duration::from_secs(60);
            while unsafe { dr_job_control_progress(control) } <= start + 1e-6
                && !worker.is_finished()
            {
                assert!(Instant::now() < deadline, "the render never started");
                thread::sleep(Duration::from_millis(1));
            }
            assert!(
                !worker.is_finished(),
                "render completed before cancellation"
            );
            let cancelled_at = Instant::now();
            unsafe { dr_job_control_cancel(control) };
            let (ok, message, returned_at) = worker.join().unwrap();
            assert!(!ok, "a cancelled render returns no image");
            assert_eq!(message, "operation cancelled");
            returned_at.saturating_duration_since(cancelled_at)
        });
        eprintln!(
            "13A-3a 8-bit cancel: {:.3} ms cancel-to-return; {:.2} ns per root sample \
             (uncancelled {:.1} ms); ≈{:.0} root samples after the cancel (declared bound 4096)",
            latency.as_secs_f64() * 1e3,
            per_sample * 1e9,
            uncancelled.as_secs_f64() * 1e3,
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
        let parked = common::probe::cancelled_past_each(&[start + 1e-6], twin, render);
        eprintln!(
            "8-bit cancel inside the loop: {}",
            common::probe::parked_line(&parked[0])
        );
    }
    unsafe { dr_doc_free(hd) };
}

#[test]
fn a_concurrently_cancelled_grouped_render_returns_operation_cancelled() {
    cancel_grouped_render(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_grouped_render_is_measured() {
    cancel_grouped_render(true);
}

/// The u16 twin: the same shape, typed, `cancel_grouped_render`'s two paths;
/// the same declared 4096-root-sample bound.
fn cancel_grouped_render_at_u16(bench: bool) {
    let (w, h) = (3000u32, 2000u32);
    let build = || {
        let profile = IccProfile::srgb();
        let image = |seed: u32, translucent: bool| {
            let pixels = (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    [
                        ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                        ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                        ((x ^ y) * 1999 % 65536 + seed) as u16,
                        if translucent { 51000 } else { 65535 },
                    ]
                })
                .collect();
            PrecisionImage::from_rgba16(w, h, pixels, profile.clone(), None).unwrap()
        };
        let raster = |seed: u32, offset: i32| {
            PrecisionDocumentLayer::raster("Raster", image(seed, true), (offset, 0), 0.9, true)
                .unwrap()
                .with_blend_mode(BlendMode::SoftLight)
                .unwrap()
        };
        let inner = PrecisionDocumentLayer::isolated_normal(
            "Inner",
            vec![raster(3, 1), raster(4, 2)],
            1.0,
            true,
        )
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
        let outer =
            PrecisionDocumentLayer::pass_through("Outer", vec![raster(2, 0), inner], 0.9, true)
                .unwrap();
        let layers = vec![
            PrecisionDocumentLayer::raster("Base", image(1, false), (0, 0), 1.0, true).unwrap(),
            outer,
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
    let d = build();
    let render = render_at(&d as *const PrecisionDocument as usize);
    if bench {
        let started = Instant::now();
        let img = unsafe {
            dr_precision_doc_render_region(
                &d,
                0,
                0,
                w,
                h,
                1,
                limit,
                0,
                ptr::null(),
                ptr::null_mut(),
            )
        };
        let uncancelled = started.elapsed();
        assert!(!img.is_null());
        unsafe { dr_precision_image_free(img) };
        let per_sample = uncancelled.as_secs_f64() / (f64::from(w) * f64::from(h));
        let control = unsafe { dr_job_control_create() };
        let token = control as usize;
        let latency = thread::scope(|scope| {
            let worker = scope.spawn(move || {
                let (ok, message) = render(token);
                (ok, message, Instant::now())
            });
            let deadline = Instant::now() + Duration::from_secs(60);
            while unsafe { dr_job_control_progress(control) } <= 0.0 && !worker.is_finished() {
                assert!(Instant::now() < deadline, "the render never started");
                thread::sleep(Duration::from_millis(1));
            }
            assert!(
                !worker.is_finished(),
                "render completed before cancellation"
            );
            let cancelled_at = Instant::now();
            unsafe { dr_job_control_cancel(control) };
            let (ok, message, returned_at) = worker.join().unwrap();
            assert!(!ok, "a cancelled render returns no image");
            assert_eq!(message, "operation cancelled");
            returned_at.saturating_duration_since(cancelled_at)
        });
        eprintln!(
            "13A-3a u16 cancel: {:.3} ms cancel-to-return; {:.2} ns per root sample \
             (uncancelled {:.1} ms); ≈{:.0} root samples after the cancel (declared bound 4096)",
            latency.as_secs_f64() * 1e3,
            per_sample * 1e9,
            uncancelled.as_secs_f64() * 1e3,
            latency.as_secs_f64() / per_sample,
        );
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        unsafe { dr_job_control_free(control) };
    } else {
        let twin = |token: usize| {
            let twin = build();
            render_at(&twin as *const PrecisionDocument as usize)(token)
        };
        let parked = common::probe::cancelled_past_each(&[0.0], twin, render);
        eprintln!(
            "u16 cancel once the render reports: {}",
            common::probe::parked_line(&parked[0])
        );
    }
}

#[test]
fn a_concurrently_cancelled_grouped_render_returns_operation_cancelled_at_u16() {
    cancel_grouped_render_at_u16(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_grouped_render_is_measured_at_u16() {
    cancel_grouped_render_at_u16(true);
}
