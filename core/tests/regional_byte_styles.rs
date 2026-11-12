//! Phase 13A-2b: the 8-bit regional document render over layer styles with
//! NO enabled effect and NO knockout — fill opacity, Blend If, disabled
//! effects, `blend_interior_as_group` — on a lone raster, a clip base and a
//! clip member, including a styled base's shape clamp after every VISIBLE
//! member (a zero-opacity one too), against the full-frame projection. The
//! oracle is INDEPENDENT of the regional code, exactly as in
//! `regional_masks_clips.rs`: `dr_doc_flattened`'s projection cropped and
//! top-left-cell sampled here in plain index arithmetic, byte for byte.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind};
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_regional::*;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::Value;

use crate::common;
use common::{img_dims, img_pixels, plasma, styled, take_err_string};

use crate::region_plan;

const W: u32 = 29;
const H: u32 = 19;

const BLEND_IF_GRAY: &str =
    "\"blend_if\":{\"channel\":\"gray\",\"this_layer\":[10,60,180,240],\"underlying\":[0,40,200,255]}";
const BLEND_IF_RED: &str = "\"blend_if\":{\"channel\":\"red\",\"underlying\":[30,90,160,220]}";

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

/// Background, a Multiply base at (3, 2), and above it a raster Multiply 0.6
/// member 15×15 at (10, 5) — which exceeds the base rect — and a levels
/// adjustment member.
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
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, RgbaImage::new(1, 1), "levels")
        .expect("add layer");
    out.layers[3].meta = Some(common::adjust_meta(
        "levels",
        "{\"black\":0.1,\"gamma\":1.6}",
    ));
    out.layers[3].opacity = 0.8;
    out.layers[3].clipped = true;
    out
}

fn rows(p: &Value) -> Vec<u64> {
    p["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["index"].as_u64().unwrap())
        .collect()
}

/// The five admitted lone styles: fill 0.5, fill 0, a gray Blend If on both
/// ramps, a red-channel `underlying`-only ramp, and fill with Blend If.
fn lone_styles() -> Vec<(&'static str, String)> {
    vec![
        ("fill 0.5", "{\"fill_opacity\":0.5}".to_string()),
        ("fill 0", "{\"fill_opacity\":0}".to_string()),
        ("gray blend if", format!("{{{BLEND_IF_GRAY}}}")),
        ("red underlying", format!("{{{BLEND_IF_RED}}}")),
        (
            "fill + blend if",
            format!("{{\"fill_opacity\":0.5,{BLEND_IF_GRAY}}}"),
        ),
    ]
}

// ------------------------------------------------------------- lone --

#[test]
fn lone_styled_rasters_match_the_projection() {
    for (label, style) in lone_styles() {
        for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Dissolve] {
            for opacity in [0.9, 1.0] {
                let doc = with_raster(&base(), top_pixels(17, 12), (5, 3), mode, opacity);
                let doc = styled(&doc, 1, &style);
                let what = format!("{label} {mode:?} {opacity}");
                assert_regions_match_with(&doc, &what, &around((5, 3), 17, 12));
            }
        }
    }
}

#[test]
fn a_masked_lone_styled_raster_matches_with_feather_6() {
    for (label, style) in lone_styles() {
        let doc = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Normal, 0.9);
        let doc = styled(&doc, 1, &style);
        let doc = masked(&doc, 1, gradient(12, 9), (7, 4), 0.7, 6.0, 0);
        assert_regions_match_with(&doc, label, &around((7, 4), 12, 9));
    }
}

#[test]
fn a_disabled_effect_is_admitted() {
    let style = "{\"fill_opacity\":0.5,\"effects\":[{\"type\":\"drop_shadow\",\
                 \"enabled\":false,\"distance\":3,\"size\":2}]}";
    let doc = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Screen, 0.9);
    let doc = styled(&doc, 1, style);
    assert_regions_match(&doc, "disabled effect, lone");
    let doc = styled(&clip_stack(), 1, style);
    assert_regions_match(&doc, "disabled effect, clip base");
}

// ------------------------------------------------------------ clip units --

#[test]
fn a_styled_clip_base_matches_the_projection() {
    let fill_blend_if = format!("{{\"fill_opacity\":0.5,{BLEND_IF_GRAY}}}");
    let doc = styled(&clip_stack(), 1, &fill_blend_if);
    assert_regions_match_with(&doc, "fill + blend if base", &around((3, 2), 19, 13));
    // Fill 0: the base's pixels vanish but its SHAPE still clips the members,
    // which therefore show inside it.
    let zero = styled(&clip_stack(), 1, "{\"fill_opacity\":0}");
    let mut hidden = zero.clone();
    hidden.layers[1].visible = false;
    assert_ne!(
        full(&zero),
        full(&hidden),
        "the members show inside the shape"
    );
    assert_regions_match_with(&zero, "fill 0 base", &around((3, 2), 19, 13));
    // A Dissolve base.
    let mut dissolve = doc.clone();
    dissolve.layers[1].blend = BlendMode::Dissolve;
    dissolve.layers[1].opacity = 0.55;
    assert_regions_match(&dissolve, "dissolve styled base");
    // A masked styled base, feather 0 and 6.
    for feather in [0.0, 6.0] {
        let masked_base = masked(&doc, 1, gradient(19, 13), (3, 2), 0.8, feather, 0);
        assert_regions_match_with(
            &masked_base,
            &format!("masked styled base feather {feather}"),
            &around((3, 2), 19, 13),
        );
    }
    // `blend_interior_as_group` changes nothing without an enabled effect.
    let grouped = styled(
        &clip_stack(),
        1,
        "{\"fill_opacity\":0.5,\"blend_interior_as_group\":true}",
    );
    assert_regions_match(&grouped, "blend interior as group");
}

#[test]
fn a_styled_member_matches_under_an_unstyled_and_a_styled_base() {
    let member = format!("{{\"fill_opacity\":0.4,{BLEND_IF_GRAY}}}");
    let doc = styled(&clip_stack(), 2, &member);
    assert_regions_match_with(&doc, "styled member", &around((10, 5), 15, 15));
    let doc = styled(&doc, 1, &format!("{{\"fill_opacity\":0.5,{BLEND_IF_RED}}}"));
    assert_regions_match_with(&doc, "styled member, styled base", &around((10, 5), 15, 15));
}

// ------------------------------------------------------- the clamp step --

/// A styled base (gray 204, alpha 143, mask 115) over opaque black, and a
/// VISIBLE clipped member at opacity 0: the reference still clamps the base
/// buffer to the base's shape after it.
fn clamp_fixture() -> DrDocument {
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
        0,
    );
    // Weight 1 over black, non-identity so the style is kept.
    let doc = styled(&doc, 1, "{\"blend_if\":{\"underlying\":[0,0,200,255]}}");
    let mut doc = with_raster(&doc, top_pixels(15, 15), (10, 5), BlendMode::Multiply, 0.6);
    doc.layers[2].clipped = true;
    doc.layers[2].opacity = 0.0;
    doc
}

#[test]
fn a_visible_zero_opacity_member_still_clamps_a_styled_base() {
    let doc = clamp_fixture();
    let at = ((4 * W + 5) * 4) as usize;
    // Pass alpha 143·115/255² = 0.2529, clamped to round(64.49)/255 =
    // 0.2510; ×0.8 over black → 51.2.
    assert_eq!(full(&doc)[at..at + 3], [51, 51, 51], "the reference clamps");
    let mut hidden = doc.clone();
    hidden.layers[2].visible = false;
    // No visible member, no clamp: 0.8 × 0.2529 → 51.59.
    assert_eq!(full(&hidden)[at..at + 3], [52, 52, 52], "no clamp");
    assert_regions_match(&doc, "clamp-only step");
    assert_regions_match(&hidden, "hidden member");
    let pixels = u64::from(W) * u64::from(H);
    let window = (0, 0, W, H);
    for (d, listed) in [(&doc, vec![0, 1]), (&hidden, vec![0, 1])] {
        let p = plan(d, window, 1).unwrap();
        assert_eq!(rows(&p), listed);
        assert_eq!(p["work_units"], pixels * (listed.len() as u64 + 1));
    }
    // A contributing member AFTER the clamp-only step, then BEFORE it.
    let before = with_raster(&doc, top_pixels(15, 15), (10, 5), BlendMode::Multiply, 0.6);
    let mut before = before;
    before.layers[3].clipped = true;
    assert_regions_match(&before, "zero-opacity step before the member");
    let p = plan(&before, window, 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 3]);
    assert_eq!(p["work_units"], pixels * 4);
    let mut after = before.clone();
    after.layers.swap(2, 3);
    assert_regions_match(&after, "zero-opacity step after the member");
    let p = plan(&after, window, 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2]);
    assert_eq!(p["work_units"], pixels * 4);
}

// ------------------------------------------------------------ refusals --

#[test]
fn effects_and_knockouts_are_admitted() {
    let drop_shadow = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}";
    let knockout = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}";
    let lone = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Normal, 0.9);
    // An enabled effect is admitted since 13A-3b-i, a knockout since 13A-3c.
    assert_regions_match(&styled(&lone, 1, drop_shadow), "drop shadow");
    assert_regions_match(&styled(&lone, 1, knockout), "a Shallow knockout");
    let both = "{\"knockout\":\"deep\",\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\
                \"size\":2}]}";
    assert_regions_match(&styled(&lone, 1, both), "Deep beside an effect");
    // A member with a knockout punches to the base-only sample.
    let member = styled(&clip_stack(), 2, knockout);
    assert_regions_match(&member, "a knockout member");
    // Since 13A-4b-ii-b-1 the unit's guided Shadows/Highlights member beside
    // the knockout member (an unstyled base) is staged too.
    let mut guided = member.clone();
    guided.layers[3].meta = Some(common::adjust_meta(
        "shadows_highlights",
        "{\"shadows\":{\"amount\":0.6,\"tone\":0.4},\"radius\":12}",
    ));
    assert_regions_match(&guided, "a guided member beside a knockout member");
    // Hidden it is dropped, and at opacity 0 it is a clamp-only step.
    let mut hidden = member.clone();
    hidden.layers[2].visible = false;
    assert_regions_match(&hidden, "hidden knockout member");
    let mut zero = member.clone();
    zero.layers[2].opacity = 0.0;
    assert_regions_match(&zero, "zero-opacity knockout member");
    assert_eq!(rows(&plan(&zero, (0, 0, W, H), 1).unwrap()), [0, 1, 3]);
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
fn the_plan_marks_styled_blend_if_rows_and_keeps_its_formulas() {
    let doc = styled(
        &clip_stack(),
        1,
        &format!("{{\"fill_opacity\":0.5,{BLEND_IF_GRAY}}}"),
    );
    let doc = styled(&doc, 2, "{\"fill_opacity\":0.4}");
    let window = (4, 3, 9, 7);
    let p = plan(&doc, window, 1).unwrap();
    let listed = p["layers"].as_array().unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3]);
    for (i, row) in listed.iter().enumerate() {
        assert_eq!(row["blend_if"], i == 1, "row {i}");
        assert_eq!(row["mask"], false);
        assert_eq!(row["neighborhood"]["kind"], "pointwise");
    }
    assert_eq!(p["contract_version"], 1);
    let pixels = 9u64 * 7;
    assert_eq!(
        p["scratch_heap_bytes"],
        pixels * 16 + pixels * 4 + description_parse(&doc)
    );
    assert_eq!(p["work_units"], pixels * (4 + 1));
    // A styled member's Blend If is marked on its own row.
    let member = styled(&clip_stack(), 2, &format!("{{{BLEND_IF_RED}}}"));
    let p = plan(&member, window, 1).unwrap();
    let flags: Vec<bool> = p["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["blend_if"].as_bool().unwrap())
        .collect();
    assert_eq!(flags, [false, false, true, false]);
}

#[test]
fn a_styled_document_evaluates_linear_light_regionally() {
    let doc = styled(&clip_stack(), 1, "{\"fill_opacity\":0.5}");
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

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_styled_clip_render_returns_no_output() {
    let doc = styled(
        &clip_stack(),
        1,
        &format!("{{\"fill_opacity\":0.5,{BLEND_IF_GRAY}}}"),
    );
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
