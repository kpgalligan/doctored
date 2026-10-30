//! Phase 13A-2a: the 8-bit regional document render over LAYER MASKS (raw
//! placement, density, feather, outside coverage, enabled or not) and CLIP
//! UNITS (members, hidden and zero-opacity members, hidden bases, adjustment
//! bases, baseless clipped entries), against the full-frame projection. The
//! oracle is INDEPENDENT of the regional code, exactly as in
//! `regional_document.rs`: `dr_doc_flattened`'s projection cropped and
//! top-left-cell sampled here in plain index arithmetic, byte for byte.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument, LayerKind};
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_regional::*;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::{adjust_meta, img_dims, img_pixels, plasma, styled, take_err_string};

use crate::region_plan;

const W: u32 = 29;
const H: u32 = 19;

// ------------------------------------------------------------- plumbing --

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

fn full(doc: &DrDocument) -> Vec<u8> {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_flattened(h, 0, std::ptr::null(), &mut err) };
    assert!(!img.is_null(), "flattened: {}", take_err_string(err));
    let out = img_pixels(img);
    unsafe {
        dr_image_free(img);
        dr_doc_free(h);
    }
    out
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

/// An adjustment layer on top whose (unused) pixels span `pixels` at
/// `offset`: a masked adjustment's coverage is confined to that extent.
fn with_adjustment_at(
    doc: &DrDocument,
    op: &str,
    params: &str,
    blend: BlendMode,
    pixels: (u32, u32),
    offset: (i32, i32),
) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, RgbaImage::new(pixels.0, pixels.1), op)
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].meta = Some(adjust_meta(op, params));
    out.layers[top].blend = blend;
    out.layers[top].opacity = 0.8;
    out.layers[top].offset = offset;
    out
}

fn with_adjustment(doc: &DrDocument, op: &str, params: &str, blend: BlendMode) -> DrDocument {
    with_adjustment_at(doc, op, params, blend, (1, 1), (0, 0))
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
    outside: u8,
) -> DrDocument {
    let mut out = doc.clone();
    let mut properties = MaskProperties::at(offset);
    properties.density = density;
    properties.feather = feather;
    properties.outside_coverage = outside;
    let layer = &mut out.layers[idx];
    layer.mask = Some(Arc::new(plane));
    layer.mask_enabled = true;
    layer.mask_properties = properties;
    out
}

/// Windows straddling the edges of a mask at `offset` of size `w`×`h`,
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

fn two() -> DrDocument {
    with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Normal, 0.9)
}

// ------------------------------------------------------------- masks --

#[test]
fn reveal_hide_and_gradient_masks_match_the_projection() {
    for (label, plane) in [
        ("reveal", GrayImage::from_pixel(17, 12, Luma([255]))),
        ("hide", GrayImage::from_pixel(17, 12, Luma([0]))),
        ("gradient", gradient(17, 12)),
    ] {
        let doc = masked(&two(), 1, plane, (5, 3), 1.0, 0.0, 0);
        assert_regions_match_with(&doc, label, &around((5, 3), 17, 12));
    }
}

#[test]
fn a_mask_placed_apart_from_its_layer_matches_for_both_outside_coverages() {
    for outside in [0u8, 255] {
        for offset in [(-4, -3), (12, 8), (20, -2), (-10, 14)] {
            let doc = masked(&two(), 1, gradient(11, 9), offset, 1.0, 0.0, outside);
            assert_regions_match_with(
                &doc,
                &format!("offset {offset:?} outside {outside}"),
                &around(offset, 11, 9),
            );
        }
    }
}

#[test]
fn density_one_half_matches_the_projection() {
    for outside in [0u8, 255] {
        let doc = masked(&two(), 1, gradient(13, 10), (8, 2), 0.5, 0.0, outside);
        assert_regions_match_with(&doc, "density 0.5", &around((8, 2), 13, 10));
    }
}

#[test]
fn feather_6_and_40_match_the_projection_across_mask_and_canvas_edges() {
    for feather in [6.0, 40.0] {
        for outside in [0u8, 255] {
            for density in [1.0, 0.5] {
                let doc = masked(
                    &two(),
                    1,
                    gradient(12, 9),
                    (7, 4),
                    density,
                    feather,
                    outside,
                );
                let what = format!("feather {feather} outside {outside} density {density}");
                assert_regions_match_with(&doc, &what, &around((7, 4), 12, 9));
                let p = plan(&doc, (0, 0, W, H), 1).unwrap();
                let row = &p["layers"][1];
                assert_eq!(row["mask"], true);
                assert_eq!(row["neighborhood"]["kind"], "spatial", "{what}");
                assert!(row["neighborhood"]["halo_pixels"].as_u64().unwrap() > 0);
            }
        }
    }
}

#[test]
fn a_disabled_mask_is_admitted_and_ignored() {
    let mut doc = masked(&two(), 1, gradient(12, 9), (7, 4), 0.5, 6.0, 0);
    doc.layers[1].mask_enabled = false;
    assert_regions_match(&doc, "disabled");
    let p = plan(&doc, (0, 0, W, H), 1).unwrap();
    assert_eq!(p["layers"][1]["mask"], false);
    assert_eq!(p["layers"][1]["neighborhood"]["kind"], "pointwise");
    assert_eq!(p["neighborhood"]["kind"], "pointwise");
}

#[test]
fn a_masked_adjustment_is_confined_to_its_extent() {
    let under = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Overlay, 0.8);
    for (pixels, offset) in [((W, H), (0, 0)), ((14, 10), (6, 5)), ((9, 30), (-3, -4))] {
        for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Dissolve] {
            let doc = with_adjustment_at(
                &under,
                "hue_saturation",
                "{\"hue\":50,\"saturation\":0.4}",
                mode,
                pixels,
                offset,
            );
            // A mask smaller than the canvas, offset from the layer, feathered
            // on one pass.
            for (feather, density) in [(0.0, 1.0), (6.0, 0.7)] {
                let doc = masked(&doc, 2, gradient(11, 8), (4, 6), density, feather, 0);
                let what = format!("{pixels:?}@{offset:?} {mode:?} feather {feather}");
                assert_regions_match_with(&doc, &what, &around((4, 6), 11, 8));
            }
        }
    }
}

// ------------------------------------------------------------ clip units --

/// Background, a Multiply base at (3, 2), and above it a raster Multiply 0.6
/// member and a levels adjustment member.
fn clip_stack() -> DrDocument {
    let doc = with_raster(
        &base(),
        top_pixels(19, 13),
        (3, 2),
        BlendMode::Multiply,
        0.85,
    );
    let mut doc = with_raster(&doc, top_pixels(15, 15), (10, 5), BlendMode::Multiply, 0.6);
    doc.layers[2].clipped = true;
    let mut doc = with_adjustment(
        &doc,
        "levels",
        "{\"black\":0.1,\"gamma\":1.6}",
        BlendMode::Normal,
    );
    doc.layers[3].clipped = true;
    doc
}

#[test]
fn a_raster_base_with_raster_and_adjustment_members_matches() {
    let doc = clip_stack();
    assert_regions_match(&doc, "clip unit");
    let p = plan(&doc, (0, 0, W, H), 1).unwrap();
    let bases: Vec<_> = p["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| (l["index"].clone(), l["clip_base"].clone()))
        .collect();
    assert_eq!(
        bases,
        [
            (json!(0), Value::Null),
            (json!(1), Value::Null),
            (json!(2), json!(1)),
            (json!(3), json!(1)),
        ]
    );
}

#[test]
fn hidden_and_zero_opacity_members_and_hidden_bases_match() {
    let mut hidden = clip_stack();
    hidden.layers[2].visible = false;
    assert_regions_match(&hidden, "hidden member");
    let mut zero = clip_stack();
    zero.layers[2].opacity = 0.0;
    assert_regions_match(&zero, "zero-opacity member");
    let listed = |doc: &DrDocument| -> Vec<u64> {
        plan(doc, (0, 0, W, H), 1).unwrap()["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["index"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(listed(&hidden), [0, 1, 3]);
    assert_eq!(listed(&zero), [0, 1, 3]);
    // A hidden base skips its whole run unchecked — a styled member included.
    let mut hidden_base = styled(
        &clip_stack(),
        2,
        "{\"effects\":[{\"type\":\"stroke\",\"size\":2,\"color\":\"#FF0000\"}]}",
    );
    hidden_base.layers[1].visible = false;
    assert_regions_match(&hidden_base, "hidden base");
    assert_eq!(listed(&hidden_base), [0]);
    let mut zero_base = clip_stack();
    zero_base.layers[1].opacity = 0.0;
    assert_regions_match(&zero_base, "zero-opacity base");
}

#[test]
fn an_adjustment_base_with_visible_members_contributes_nothing() {
    let doc = with_adjustment(&base(), "invert", "{}", BlendMode::Normal);
    let mut doc = with_raster(&doc, top_pixels(17, 12), (5, 3), BlendMode::Normal, 1.0);
    doc.layers[2].clipped = true;
    assert_eq!(full(&doc), full(&base()), "the unit contributes nothing");
    assert_regions_match(&doc, "adjustment base");
}

#[test]
fn an_adjustment_base_whose_only_member_is_hidden_contributes_nothing_at_8_bits() {
    // Invert over the raster at 0, its ONLY member hidden: the positional run
    // is non-empty, so the reference takes the buffer path and the Invert
    // contributes nothing (the typed twin applies it — see
    // `regional_precision_masks_clips.rs`).
    let doc = with_adjustment(&base(), "invert", "{}", BlendMode::Normal);
    let mut doc = with_raster(&doc, top_pixels(17, 12), (5, 3), BlendMode::Normal, 1.0);
    doc.layers[2].clipped = true;
    doc.layers[2].visible = false;
    assert_eq!(full(&doc), full(&base()));
    assert_regions_match(&doc, "hidden-only run");
    let p = plan(&doc, (0, 0, W, H), 1).unwrap();
    let rows = p["layers"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "the base and no member rows");
    assert_eq!(rows[1]["index"], 1);
    assert_eq!(rows[1]["kind"], "adjustment");
    assert_eq!(rows[1]["clip_base"], Value::Null);
}

#[test]
fn baseless_clipped_entries_are_each_their_own_unit() {
    // Indices 0 and 1 both clipped: both baseless. Entry 1 extends beyond
    // entry 0's footprint and must NOT be clamped to it.
    let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(9, 7, |x, y| {
        Rgba([(x * 20) as u8, (y * 30) as u8, 90, 255])
    }));
    doc.width = W;
    doc.height = H;
    let mut doc = with_raster(&doc, top_pixels(20, 14), (4, 3), BlendMode::Screen, 0.9);
    doc.layers[0].clipped = true;
    doc.layers[1].clipped = true;
    let mut doc = with_raster(&doc, top_pixels(9, 9), (2, 2), BlendMode::Normal, 0.7);
    doc.layers[2].clipped = true;
    assert_regions_match(&doc, "baseless");
    let p = plan(&doc, (0, 0, W, H), 1).unwrap();
    for row in p["layers"].as_array().unwrap() {
        assert_eq!(row["clip_base"], Value::Null, "{row}");
    }
    // Entry 1 shows beyond entry 0's 9×7 footprint.
    let (_, _, px) = region(&doc, (15, 10, 1, 1), 1).unwrap();
    assert!(px[3] > 0, "entry 1 is visible outside entry 0's footprint");
}

#[test]
fn dissolve_bases_masked_bases_and_masked_members_match() {
    let mut doc = clip_stack();
    doc.layers[1].blend = BlendMode::Dissolve;
    doc.layers[1].opacity = 0.55;
    assert_regions_match(&doc, "dissolve base");
    let doc = masked(&clip_stack(), 1, gradient(19, 13), (3, 2), 1.0, 0.0, 0);
    assert_regions_match_with(&doc, "masked base", &around((3, 2), 19, 13));
    let doc = masked(&doc, 2, gradient(10, 12), (12, 4), 0.6, 6.0, 255);
    assert_regions_match_with(&doc, "masked base + member", &around((12, 4), 10, 12));
    let doc = masked(&doc, 3, gradient(8, 8), (1, 1), 1.0, 0.0, 0);
    assert_regions_match(&doc, "masked adjustment member");
    let mut dissolved = doc.clone();
    dissolved.layers[2].blend = BlendMode::Dissolve;
    assert_regions_match(&dissolved, "dissolve member");
}

#[test]
fn a_linear_light_document_with_masks_and_clips_matches() {
    let doc = masked(&clip_stack(), 2, gradient(10, 12), (12, 4), 0.6, 6.0, 0);
    let linear = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(full(&linear), full(&doc));
    assert_regions_match(&linear, "linear light");
}

// ----------------------------------------------------------------- plan --

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
fn the_plan_declares_masks_halos_and_formulas() {
    let (mw, mh) = (12, 9);
    let doc = masked(&clip_stack(), 2, gradient(mw, mh), (7, 4), 1.0, 6.0, 0);
    // A window touching the canvas's right edge, so the grown input region
    // is clamped there.
    let window = (W - 5, 3, 5, 6);
    let p = plan(&doc, window, 1).unwrap();
    let rows = p["layers"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let halo = rows[2]["neighborhood"]["halo_pixels"].as_u64().unwrap() as u32;
    assert!(halo > 0);
    assert_eq!(rows[2]["neighborhood"]["kind"], "spatial");
    assert_eq!(rows[2]["mask"], true);
    assert_eq!(rows[2]["clip_base"], 1);
    for i in [0, 1, 3] {
        assert_eq!(rows[i]["mask"], false);
        assert_eq!(
            rows[i]["neighborhood"],
            json!({"kind":"pointwise","halo_pixels":0})
        );
    }
    for row in rows {
        assert_eq!(row["blend_if"], false);
    }
    assert_eq!(
        p["neighborhood"],
        json!({"kind":"spatial","halo_pixels":halo})
    );
    let x0 = (W - 5).saturating_sub(halo);
    let y0 = 3u32.saturating_sub(halo);
    let x1 = W.min(W + halo);
    let y1 = H.min(3 + 6 + halo);
    assert_eq!(
        p["required_input_region"],
        json!({"x":x0,"y":y0,"width":x1 - x0,"height":y1 - y0})
    );
    assert_eq!(
        p["requested_region"],
        json!({"x":W - 5,"y":3,"width":5,"height":6})
    );
    // Scratch: accumulator + output + the feathered plane (the mask's padded
    // extent, 12 bytes a pixel).
    let pixels = 5u64 * 6;
    let plane = u64::from(mw + 2 * halo) * u64::from(mh + 2 * halo) * 12;
    assert_eq!(
        p["scratch_heap_bytes"],
        pixels * 16 + pixels * 4 + plane + description_parse(&doc)
    );
    // Work: the fill, one pass per row and — 13A-5b-iii-b-2 — the feather
    // blur over the padded plane (feather 6 → sigma 2.3 → factor 1 →
    // `feather_radius` 6.0 → 13 taps, outside coverage 0: the
    // `regional_precision_masks_clips` restatement).
    let padded = u64::from(mw + 2 * halo) * u64::from(mh + 2 * halo);
    assert_eq!(
        p["work_units"],
        pixels * (4 + 1) + padded * 32 + padded * 2 * 13
    );
}

// ------------------------------------------------------------ refusals --

#[test]
fn styled_and_group_members_and_knockout_bases_are_admitted() {
    let doc = styled(
        &clip_stack(),
        2,
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}",
    );
    // Since 13A-3b-i a styled member with an enabled effect is admitted.
    assert_regions_match(&doc, "a drop-shadow member");
    // Byte Blend If and fill opacity are admitted since 13A-2b
    // (`regional_byte_styles.rs`), a knockout base since 13A-3c.
    let knockout = styled(
        &clip_stack(),
        1,
        "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}",
    );
    assert_regions_match(&knockout, "a knockout base");
    // The unit's guided Shadows/Highlights member under the knockout
    // (routed) base is staged since 13A-4b-ii-b-2 and equals the reference.
    let mut guided = knockout.clone();
    guided.layers[3].meta = Some(adjust_meta(
        "shadows_highlights",
        "{\"shadows\":{\"amount\":0.6,\"tone\":0.4},\"radius\":12}",
    ));
    assert_regions_match(&guided, "a guided member under a knockout base");
    // Since 13A-3a a member group is admitted (always isolated) and equals
    // the reference, and so does a hidden empty group member.
    let (grouped, ..) = clip_stack().group_layers(&[2], "Group").expect("group");
    assert!(grouped.layers.iter().any(|l| l.depth > 0), "a child");
    assert_regions_match(&grouped, "a member group");
    let mut empty = clip_stack();
    let mut entry = empty.layers[2].clone();
    entry.kind = LayerKind::Group;
    entry.name = "Empty".into();
    entry.visible = false;
    entry.clipped = true;
    empty.layers.insert(3, entry);
    assert_regions_match(&empty, "a hidden empty group member");
}

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_masked_clip_render_returns_no_output() {
    let doc = masked(&clip_stack(), 2, gradient(10, 12), (12, 4), 0.6, 6.0, 0);
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

/// The concurrent cancel. With `bench`, a wall-clock record: DECLARED BOUND:
/// a cancel returns within 4096 samples (one checkpoint of the unit loop).
/// Measured: the cancel-to-return latency and, from an uncancelled render of
/// the same document, the time per sample; their quotient estimates the
/// samples processed after the cancel, printed for the record
/// (`--nocapture`). Without it, the load-independent default: the
/// loop's first report's checkpoint count comes from a fresh twin's own
/// report log and the worker is cancelled PARKED at the next checkpoint
/// (`cancelled_past_each`), no poller.
fn cancel_masked_clip_render(bench: bool) {
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
        doc.layers[2].clipped = true;
        doc.layers[3].clipped = true;
        // Unfeathered masks at density 0.5: `derived` is the raw plane, so no
        // (uncancellable, 13A-5) blur precedes the loop.
        let plane = GrayImage::from_fn(w, h, |x, y| Luma([((x + y) % 256) as u8]));
        for idx in [1, 2] {
            doc = masked(&doc, idx, plane.clone(), (0, 0), 0.5, 0.0, 0);
        }
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
        // Unit-samples: one per canvas pixel per unit (background + clip unit).
        let per_sample = uncancelled.as_secs_f64() / (2.0 * f64::from(w) * f64::from(h));
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
            "13A-2a cancel: {:.3} ms cancel-to-return; {:.2} ns per unit-sample \
             (uncancelled {:.1} ms); ≈{:.0} samples after the cancel (declared bound 4096)",
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
            "cancel inside the unit loop: {}",
            common::probe::parked_line(&parked[0])
        );
    }
    unsafe { dr_doc_free(hd) };
}

#[test]
fn a_concurrently_cancelled_masked_clip_render_returns_operation_cancelled() {
    cancel_masked_clip_render(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_masked_clip_render_is_measured() {
    cancel_masked_clip_render(true);
}
