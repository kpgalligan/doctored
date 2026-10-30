//! Phase 13A-5c-3: the byte regional plan's two resource terms — the
//! description parse under EVERY profile (`adjust_calibrated::parse_scratch`:
//! the largest non-group `meta` length × 256, because the admission walk
//! parses `Adjustment::from_meta` per adjustment node whatever the profile)
//! and, inside a MATERIALIZED nest, the reference's UNCHARGED per-unit
//! buffers (`doc_regional_budget::materialization_charges`' third figure:
//! unit buffers, package strips and their knockout windows, the run base's
//! guide, a pass-through's mask crop, each nested synthetic). Every fixture
//! is pinned exactly — the expected figures restated here by hand from the
//! fixture, never read back from the code — renders equal to the crop of
//! `dr_doc_flattened`, and completes under a scratch limit of its own plan.
//! Load-independent assertions only (13A-5b-iii-b-1's load policy).

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::{adjust_meta, img_dims, img_pixels, plasma, styled, take_err_string};

use crate::region_plan;

type Window = (u32, u32, u32, u32);

/// The canvas.
const CW: u32 = 128;
const CH: u32 = 112;
/// Every root plan here is the whole canvas at divisor 1: the f32
/// accumulator and the RGBA8 output, 20 bytes a pixel.
const N: u64 = CW as u64 * CH as u64;

/// The nest's FRAME: an opaque 100×100 raster at (14, 6). Every other child
/// of the nest lies well inside it (styles' pads and shadow shifts
/// included), so the nest's rect — its accumulator — is the frame's.
const FRAME: (i32, i32) = (14, 6);
const FW: u64 = 100;
const FH: u64 = 100;
/// `A`: the nest's accumulator pixels.
const A: u64 = FW * FH;
/// `S = min(A, W·256)`: one package strip (256 rows of the nest's width).
const S: u64 = if A < FW * 256 { A } else { FW * 256 };

/// A single colour overlay: reach 0, so pad 1.
const OVERLAY: &str =
    "{\"effects\":[{\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#80c0ff\"}]}";
const DROP_SHADOW: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}";
/// "Blend Interior Effects as Group" over the drop shadow and a colour
/// overlay at opacity 0: the only grouped effect is skipped by its settings.
const FLAG_SKIPPED_OVERLAY: &str = "{\"blend_interior_as_group\":true,\"effects\":\
                                    [{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2},\
                                    {\"type\":\"color_overlay\",\"opacity\":0}]}";
/// The same two effects without the flag.
const SKIPPED_OVERLAY: &str = "{\"effects\":\
                               [{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2},\
                               {\"type\":\"color_overlay\",\"opacity\":0}]}";
/// The flag, the same drop shadow and a LIVE colour overlay.
const FLAG_LIVE_OVERLAY: &str = "{\"blend_interior_as_group\":true,\"effects\":\
                                 [{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2},\
                                 {\"type\":\"color_overlay\",\"blend\":\"multiply\",\
                                 \"color\":\"#80c0ff\"}]}";
const SHALLOW: &str = "{\"fill_opacity\":0.6,\"knockout\":\"shallow\"}";
const DEEP: &str = "{\"fill_opacity\":0.6,\"knockout\":\"deep\"}";
/// A Shallow knockout that renders planes (a drop shadow at opacity 1).
const SHALLOW_SHADOWED: &str = "{\"fill_opacity\":0.6,\"knockout\":\"shallow\",\"effects\":\
                                [{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}";
const BLEND_IF: &str = "{\"blend_if\":{\"channel\":\"gray\",\"this_layer\":[10,60,180,240],\
                        \"underlying\":[0,40,200,255]}}";

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

fn expected(full: &[u8], (x, y, w, h): Window, d: u32) -> (u32, u32, Vec<u8>) {
    let (ow, oh) = (w.div_ceil(d), h.div_ceil(d));
    let mut out = Vec::new();
    for j in 0..oh {
        for i in 0..ow {
            let (cx, cy) = (x + i * d, y + j * d);
            let at = ((cy * CW + cx) * 4) as usize;
            out.extend_from_slice(&full[at..at + 4]);
        }
    }
    (ow, oh, out)
}

/// The region under a control whose scratch is limited to `limit`
/// (`u64::MAX`: unlimited); the control must release its scratch on return.
fn region(doc: &DrDocument, (x, y, w, h): Window, d: u32, limit: u64) -> (u32, u32, Vec<u8>) {
    let hd = handle(doc);
    let control = unsafe { dr_job_control_create() };
    if limit != u64::MAX {
        assert!(unsafe { dr_job_control_set_scratch_limit(control, limit) });
    }
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, d, u64::MAX, control, &mut err) };
    let scratch = unsafe { dr_job_control_scratch_bytes(control) };
    unsafe {
        dr_job_control_free(control);
        dr_doc_free(hd);
    }
    assert_eq!(scratch, 0, "the render left scratch reserved");
    assert!(!img.is_null(), "region: {}", take_err_string(err));
    let (ow, oh) = img_dims(img);
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    (ow, oh, out)
}

/// The whole-canvas plan at divisor 1.
fn plan(doc: &DrDocument) -> Value {
    region_plan::byte(doc, (0, 0, CW, CH), 1, u64::MAX).unwrap_or_else(|e| panic!("plan: {e}"))
}

fn scratch(doc: &DrDocument) -> u64 {
    plan(doc)["scratch_heap_bytes"].as_u64().unwrap()
}

/// Equal to the crop of the full projection at a few windows and divisors,
/// and (iii) the whole canvas completes under a scratch limit of exactly the
/// plan's figure.
fn assert_renders(doc: &DrDocument, what: &str) {
    let reference = full(doc);
    for (window, d) in [
        ((0, 0, CW, CH), 1),
        ((10, 5, 50, 40), 1),
        ((20, 20, 33, 17), 3),
        ((60, 50, 68, 62), 2),
        ((0, 0, CW, CH), 7),
    ] {
        assert!(
            region(doc, window, d, u64::MAX) == expected(&reference, window, d),
            "{what}: window {window:?} divisor {d} differs from the projection"
        );
    }
    let limit = scratch(doc);
    let window = (0, 0, CW, CH);
    assert!(
        region(doc, window, 1, limit) == expected(&reference, window, 1),
        "{what}: under the plan's own scratch limit {limit}"
    );
}

// ------------------------------------------------------------- fixtures --

fn background() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(CW, CH, |x, y| {
        let c = plasma(x, y);
        Rgba([c[0], c[1], c[2], 255])
    }))
}

/// Translucent, varied pixels with a transparent corner.
fn pattern(w: u32, h: u32, seed: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let alpha = if x < 2 && y < 2 {
            0
        } else {
            (90 + (x * 13 + y * 29 + seed * 7) % 166) as u8
        };
        Rgba([
            (x * 37 + y * 11 + seed) as u8,
            (200 - x * 5) as u8,
            (y * 23 + 7 * seed) as u8,
            alpha,
        ])
    })
}

fn opaque(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([(x * 23) as u8, (y * 41) as u8, ((x ^ y) * 9) as u8, 255])
    })
}

/// `pixels` added on top of the stack at `offset`, Normal, at `opacity`.
fn raster(doc: &DrDocument, pixels: RgbaImage, offset: (i32, i32), opacity: f32) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, pixels, "Raster")
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].offset = offset;
    out.layers[top].opacity = opacity;
    out
}

/// The top entry, styled with `json`.
fn style_top(doc: &DrDocument, json: &str) -> DrDocument {
    styled(doc, doc.layers.len() - 1, json)
}

/// The top entry, clipped.
fn clip_top(mut doc: DrDocument) -> DrDocument {
    let top = doc.layers.len() - 1;
    doc.layers[top].clipped = true;
    doc
}

/// A 20×20 pattern raster at `offset` with `json` as its style.
fn styled_raster(doc: &DrDocument, offset: (i32, i32), json: &str, opacity: f32) -> DrDocument {
    style_top(&raster(doc, pattern(20, 20, 3), offset, opacity), json)
}

/// `group_layers` over `idx` (a new Pass Through group), and its index.
fn group(doc: &DrDocument, idx: &[usize], name: &str) -> (DrDocument, usize) {
    let (out, g, ..) = doc.group_layers(idx, name).expect("group");
    (out, g)
}

/// The top-level entries above index `from`.
fn top_level_from(doc: &DrDocument, from: usize) -> Vec<usize> {
    (from..doc.layers.len())
        .filter(|&i| doc.layers[i].depth == 0)
        .collect()
}

/// The top raster grouped alone (Pass Through), and the group's index.
fn group_top(doc: &DrDocument, name: &str) -> (DrDocument, usize) {
    group(doc, &[doc.layers.len() - 1], name)
}

fn mask(doc: &DrDocument, idx: usize, (w, h): (u32, u32), offset: (i32, i32)) -> DrDocument {
    let mut out = doc.clone();
    let layer = &mut out.layers[idx];
    layer.mask = Some(Arc::new(GrayImage::from_fn(w, h, |x, y| {
        Luma([((x * 255) / (w - 1).max(1)) as u8 ^ (y * 9) as u8])
    })));
    layer.mask_enabled = true;
    layer.mask_properties = MaskProperties::at(offset);
    out
}

/// The NEST: the Background (0), then — grouped into one Normal group
/// styled with a colour overlay, which the reference MATERIALIZES — the
/// frame and whatever `content` adds above it.
fn nest(content: impl FnOnce(DrDocument) -> DrDocument) -> DrDocument {
    let doc = raster(&background(), opaque(FW as u32, FH as u32), FRAME, 1.0);
    let doc = content(doc);
    let children = top_level_from(&doc, 1);
    let (mut doc, g) = group(&doc, &children, "Nest");
    doc.layers[g].blend = BlendMode::Normal;
    styled(&doc, g, OVERLAY)
}

/// `style_scratch` of the colour overlay over a `w`×`h` synthetic, restated:
/// pad 1, so `P = (w + 2)·(h + 2)`; one enabled effect, so `P·(2 + 2)` u8
/// planes; `plane_blur::float_plan` at sigma 0 (factor 1, 3 taps) =
/// `P·32 + 3·4 + 4096`; and the strip window `16·min(h + 2, 256)·(w + 2)`.
fn overlay_planes(w: u64, h: u64) -> u64 {
    let (wp, hp) = (w + 2, h + 2);
    let p = wp * hp;
    p * 4 + p * 32 + 12 + 4096 + 16 * hp.min(256) * wp
}

/// `style_scratch` of `DROP_SHADOW` (distance 3, size 2, spread 0) over a
/// `w`×`h` raster, restated: the drop shadow's box-pair law (#2,
/// `style_matte`: `n = 0`, `w = 2`), reach `n + 1 + (w − 1)` = 2 and pad 3,
/// `P = (w + 6)·(h + 6)`; one enabled effect, so `P·(2 + 2)` u8 planes;
/// `plane_blur::float_plan` at sigma 0 (the law runs no Gaussian; factor 1,
/// 3 taps) = `P·32 + 3·4 + 4096`; and the strip window
/// `16·min(h + 6, 256)·(w + 6)`.
fn drop_shadow_planes(w: u64, h: u64) -> u64 {
    let (wp, hp) = (w + 6, h + 6);
    let p = wp * hp;
    p * 4 + p * 32 + 12 + 4096 + 16 * hp.min(256) * wp
}

/// The whole-canvas plan of a nest document, restated: the accumulator and
/// output, the description parse, and the nest's term — its charged `peak`
/// pixels × 16, the frame-sized synthetic's RGBA8, its planes and the
/// `uncharged` bytes (13A-5c-3).
fn nest_scratch(parse: u64, peak: u64, uncharged: u64) -> u64 {
    N * 20 + parse + peak * 16 + A * 4 + overlay_planes(FW, FH) + uncharged
}

fn assert_nest(doc: &DrDocument, peak: u64, uncharged: u64, what: &str) {
    assert_eq!(scratch(doc), nest_scratch(0, peak, uncharged), "{what}");
    assert_renders(doc, what);
}

// ------------------------------------------------ (i) the description parse --

const LEVELS_SHORT: &str = "{\"black\":0.1,\"white\":0.9,\"gamma\":1.8}";
const LEVELS_LONG: &str = "{\"black\":0.12345,\"white\":0.9,\"gamma\":1.8}";
const LEVELS_MID: &str = "{\"black\":0.1,\"white\":0.9,\"gamma\":1.85}";

fn with_levels(doc: &DrDocument, params: &str) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, RgbaImage::new(1, 1), "Levels")
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].meta = Some(adjust_meta("levels", params));
    out
}

fn unmodelled() -> Arc<IccProfile> {
    let mut bytes = IccProfile::srgb().bytes()[..128].to_vec();
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes[..4].copy_from_slice(&132u32.to_be_bytes());
    let profile = IccProfile::parse(&bytes).unwrap();
    assert!(profile.model().is_none(), "the profile has no model");
    Arc::new(profile)
}

#[test]
fn an_srgb_plan_charges_one_parse_of_the_description() {
    let doc = with_levels(&background(), LEVELS_SHORT);
    assert!(doc.profile.model().is_some(), "sRGB is modelled");
    let p = plan(&doc);
    assert_eq!(p["layers"][1]["operator"], "levels");
    // `{"type":"adjust","op":"levels","params":{"black":0.1,"white":0.9,"gamma":1.8}}`.
    let len = "{\"type\":\"adjust\",\"op\":\"levels\",\"params\":\
               {\"black\":0.1,\"white\":0.9,\"gamma\":1.8}}"
        .len() as u64;
    assert_eq!(len, 78);
    assert_eq!(
        p["scratch_heap_bytes"].as_u64().unwrap(),
        N * 20 + len * 256
    );
    assert_renders(&doc, "one levels row");
}

#[test]
fn three_descriptions_charge_the_largest_not_the_sum() {
    let doc = with_levels(&background(), LEVELS_SHORT);
    let doc = with_levels(&doc, LEVELS_LONG);
    let doc = with_levels(&doc, LEVELS_MID);
    let p = plan(&doc);
    for i in 1..=3 {
        assert_eq!(p["layers"][i]["operator"], "levels", "row {i}");
    }
    // The long row's `0.12345` is four characters over the short one's 78,
    // the mid row's `1.85` one over.
    let largest = 78 + 4;
    assert_eq!(
        p["scratch_heap_bytes"].as_u64().unwrap(),
        N * 20 + largest * 256
    );
    assert_renders(&doc, "three levels rows");
}

#[test]
fn a_groups_meta_is_never_parsed_or_charged() {
    let doc = with_levels(&background(), LEVELS_SHORT);
    let (mut grouped, g) = group_top(&doc, "Group");
    // The host's own blob: the core never interprets a group's meta.
    grouped.layers[g].meta = Some("x".repeat(10_000));
    assert_eq!(scratch(&grouped), N * 20 + 78 * 256);
    assert_renders(&grouped, "a group meta");
}

#[test]
fn the_unmodelled_twin_charges_the_same_parse() {
    let doc = with_levels(&background(), LEVELS_SHORT);
    let doc = with_levels(&doc, LEVELS_LONG);
    let mut twin = doc.clone();
    twin.profile = unmodelled();
    // 13A-5c-1's unmodelled figure, unchanged — and now the modelled one's.
    assert_eq!(scratch(&twin), N * 20 + 82 * 256);
    assert_eq!(scratch(&twin), scratch(&doc));
    assert_renders(&twin, "the unmodelled twin");
}

// ------------------------------------------- (ii) the uncharged nest term --

#[test]
fn a_nest_of_plain_rasters_holds_nothing_uncharged() {
    let doc = nest(|doc| raster(&doc, pattern(20, 20, 1), (40, 30), 0.8));
    assert_nest(&doc, A, 0, "the control");
}

#[test]
fn a_clip_unit_holds_its_buffer_and_a_group_members_synthetic() {
    // A raster base and one visible raster member: the unit buffer and
    // `base_alpha`, 20·A.
    let doc = nest(|doc| {
        let doc = raster(&doc, pattern(20, 20, 2), (30, 30), 1.0);
        clip_top(raster(&doc, pattern(10, 10, 4), (35, 35), 0.9))
    });
    assert_nest(&doc, A, 20 * A, "a raster member");
    // The member a GROUP over a 10×10 raster: `resolved_member` renders it
    // (charged: A + 100) and its 10×10 synthetic lives through the unit.
    let doc = nest(|doc| {
        let doc = raster(&doc, pattern(20, 20, 2), (30, 30), 1.0);
        let doc = raster(&doc, pattern(10, 10, 4), (35, 35), 0.9);
        clip_top(group_top(&doc, "Member").0)
    });
    assert_nest(&doc, A + 100, 20 * A + 4 * 100, "a group member");
}

#[test]
fn a_knockout_member_adds_the_base_only_snapshot() {
    // The base-only clone (16·A) beside the unit buffer (20·A), and the
    // member's own routed unit pass (20·A plus its pad-0 shape, 100 bytes:
    // no effect renders planes).
    let doc = nest(|doc| {
        let doc = raster(&doc, pattern(20, 20, 2), (30, 30), 1.0);
        let doc = raster(&doc, pattern(10, 10, 4), (35, 35), 1.0);
        clip_top(style_top(&doc, SHALLOW))
    });
    assert_nest(
        &doc,
        A,
        20 * A + 16 * A + (20 * A + 100),
        "a knockout member",
    );
}

#[test]
fn a_blend_if_base_with_a_run_adds_its_snapshot_and_shape() {
    // The unit buffer, the Blend If snapshot over the 20×20 layer rect
    // (16·min(400, A)) and the pad-0 shape (400 bytes: no planes).
    let doc = nest(|doc| {
        let doc = styled_raster(&doc, (30, 30), BLEND_IF, 1.0);
        clip_top(raster(&doc, pattern(10, 10, 4), (35, 35), 1.0))
    });
    assert_nest(&doc, A, 20 * A + 16 * 400 + 400, "a Blend If base");
}

#[test]
fn a_routed_lone_base_without_planes_adds_its_unit_pass() {
    let doc = nest(|doc| styled_raster(&doc, (30, 30), DEEP, 1.0));
    assert_nest(&doc, A, 20 * A + 400, "a routed lone base");
}

#[test]
fn a_flagged_lone_base_whose_grouped_effects_are_all_skipped_adds_no_unit_pass() {
    let skipped = nest(|doc| styled_raster(&doc, (30, 30), SKIPPED_OVERLAY, 1.0));
    let flag_skipped = nest(|doc| styled_raster(&doc, (30, 30), FLAG_SKIPPED_OVERLAY, 1.0));
    let flag_live = nest(|doc| styled_raster(&doc, (30, 30), FLAG_LIVE_OVERLAY, 1.0));
    // Two enabled effects at the drop shadow's pad 3, so `P = 26·26`, and
    // `style_scratch` adds one more `2·P` u8 plane pair over the drop
    // shadow's figure; the overlay's sigma 0 and reach 0 change neither the
    // float plan nor the pad.
    assert_nest(
        &skipped,
        A,
        drop_shadow_planes(20, 20) + 2 * 26 * 26,
        "a skipped overlay beside a drop shadow",
    );
    // The flag moves nothing the renderer emits, so it plans nothing more.
    assert_eq!(
        scratch(&flag_skipped),
        scratch(&skipped),
        "a flag whose grouped effects are all skipped routes no unit pass"
    );
    // A live overlay renders: the routed unit pass's buffer, and no pad-0
    // shape because planes render.
    assert_eq!(
        scratch(&flag_live) - scratch(&skipped),
        20 * A,
        "a live grouped overlay adds the routed unit pass"
    );
    assert_renders(&flag_skipped, "a flagged skipped overlay");
    assert_renders(&flag_live, "a flagged live overlay");
}

#[test]
fn a_flagged_pattern_only_style_plans_its_unit_pass() {
    // Pattern Overlay is the fifth GROUPED interior effect, so a
    // pattern-only style with the flag routes the unit pass (`moves_under_d6`
    // must say so, or the dry run under-plans the real pass by 20·A).
    let plain = nest(|doc| {
        styled_raster(
            &doc,
            (30, 30),
            "{\"effects\":[{\"type\":\"pattern_overlay\",\"blend\":\"multiply\"}]}",
            1.0,
        )
    });
    let flagged = nest(|doc| {
        styled_raster(
            &doc,
            (30, 30),
            "{\"blend_interior_as_group\":true,\"effects\":\
             [{\"type\":\"pattern_overlay\",\"blend\":\"multiply\"}]}",
            1.0,
        )
    });
    assert_eq!(
        scratch(&flagged) - scratch(&plain),
        20 * A,
        "a live grouped pattern overlay adds the routed unit pass"
    );
    assert_renders(&plain, "a pattern overlay");
    assert_renders(&flagged, "a flagged pattern overlay");
}

#[test]
fn a_shape_burst_stroke_renders_regionally() {
    // A Shape Burst stroke's field spans its whole plane, built once
    // per stroke (`style_stroke_fill`, its `2·P` charged in `style_scratch`),
    // so every window equals the projection's crop at every position.
    for position in ["outside", "inside", "center"] {
        let json = format!(
            "{{\"effects\":[{{\"type\":\"stroke\",\"size\":4,\"position\":\"{position}\",\
             \"fill_type\":\"gradient\",\"gradient\":{{\"style\":\"shape_burst\",\
             \"dither\":true}}}}]}}"
        );
        let doc = nest(|doc| styled_raster(&doc, (30, 30), &json, 1.0));
        assert_renders(&doc, &format!("a {position} Shape Burst stroke"));
    }
}

#[test]
fn a_pattern_stroke_renders_regionally() {
    // A Pattern stroke's level is built once per stroke and
    // charged once (`style_pattern::level_bytes`); a linked tile is
    // layer-local and an unlinked one canvas-aligned, so every window
    // equals the projection's crop at every position, under the plan's own
    // scratch limit, both ways.
    for position in ["outside", "inside", "center"] {
        for linked in [true, false] {
            let json = format!(
                "{{\"effects\":[{{\"type\":\"stroke\",\"size\":4,\"position\":\"{position}\",\
                 \"fill_type\":\"pattern\",\"pattern_fill\":{{\"pattern\":{{\"preset\":\"dots\"}},\
                 \"scale\":0.7,\"angle\":20,\"phase\":[3,1],\"link_with_layer\":{linked}}}}}]}}"
            );
            let doc = nest(|doc| styled_raster(&doc, (30, 30), &json, 1.0));
            assert_renders(
                &doc,
                &format!("a {position} Pattern stroke, linked {linked}"),
            );
        }
    }
}

#[test]
fn a_textured_bevel_renders_regionally_and_charges_its_level_once() {
    // An enabled bevel texture places one tile, whose PYRAMID
    // level (16 bytes a texel; the checkerboard preset at scale 1 reads the
    // tile itself) is charged once (`style_pattern::level_bytes` over
    // `placed_fill`); a switched-off texture places none. A linked texture
    // is layer-local and an unlinked one canvas-aligned, so every window
    // equals the projection's crop under the plan's own scratch limit.
    let presets: Value =
        serde_json::from_str(&doctored_core::style::pattern_presets_json()).unwrap();
    let checker = presets
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["preset"] == "checkerboard")
        .unwrap();
    let level = 16 * checker["width"].as_u64().unwrap() * checker["height"].as_u64().unwrap();
    for linked in [true, false] {
        let json = |enabled: bool| {
            format!(
                "{{\"effects\":[{{\"type\":\"bevel_emboss\",\"size\":6,\"texture_enabled\":{enabled},\
                 \"texture\":{{\"pattern\":{{\"preset\":\"checkerboard\"}},\"phase\":[3,1],\
                 \"depth\":2,\"link_with_layer\":{linked}}}}}]}}"
            )
        };
        let on = nest(|doc| styled_raster(&doc, (30, 30), &json(true), 1.0));
        let off = nest(|doc| styled_raster(&doc, (30, 30), &json(false), 1.0));
        assert_eq!(
            scratch(&on),
            scratch(&off) + level,
            "linked {linked}: the level is charged once"
        );
        assert_renders(&on, &format!("a textured bevel, linked {linked}"));
    }
}

#[test]
fn a_nested_styled_groups_synthetic_and_planes_are_charged() {
    // A Normal group over a 20×20 raster at (30, 30), styled with the
    // overlay: its rect is the raster's (a group's own style grows only its
    // parent's extent), charged A + 400; its 20×20 synthetic and its planes
    // were uncharged before 13A-5c-3.
    let inner = |masked: bool| {
        nest(move |doc| {
            let doc = raster(&doc, pattern(20, 20, 5), (30, 30), 1.0);
            let (mut doc, g) = group_top(&doc, "Inner");
            doc.layers[g].blend = BlendMode::Normal;
            let doc = styled(&doc, g, OVERLAY);
            if masked {
                mask(&doc, g, (20, 20), (30, 30))
            } else {
                doc
            }
        })
    };
    let synthetic = 4 * 400 + overlay_planes(20, 20);
    assert_nest(&inner(false), A + 400, synthetic, "a nested styled group");
    // Its cropped mask too.
    assert_nest(
        &inner(true),
        A + 400,
        synthetic + 400,
        "a masked nested styled group",
    );
}

#[test]
fn a_lone_package_adds_one_strip() {
    // A drop-shadowed raster at opacity 0.5 (Normal, no Blend If, no
    // knockout) in the 100×100 nest: `composite_package_into`'s strip copy,
    // 16·S with S = A (100 rows ≤ 256). The opacity-1 twin takes the direct
    // path; every other term (its planes included) is the same.
    let lone = |opacity: f32| nest(|doc| styled_raster(&doc, (40, 40), DROP_SHADOW, opacity));
    let (package, direct) = (lone(0.5), lone(1.0));
    assert_eq!(S, A);
    // The direct twin's absolute figure: its planes only.
    assert_eq!(drop_shadow_planes(20, 20), 39_260);
    assert_nest(&direct, A, drop_shadow_planes(20, 20), "the direct twin");
    assert_eq!(scratch(&package) - scratch(&direct), 16 * S);
    assert_renders(&package, "a lone package");
    assert_renders(&direct, "its direct twin");
    // The same raster as a clip BASE with one visible member: the unit
    // buffer beside the base's strip.
    let base = nest(|doc| {
        let doc = styled_raster(&doc, (40, 40), DROP_SHADOW, 0.5);
        clip_top(raster(&doc, pattern(10, 10, 4), (45, 45), 1.0))
    });
    assert_eq!(scratch(&base) - scratch(&direct), 20 * A + 16 * S);
    assert_renders(&base, "a package base");
}

#[test]
fn a_package_member_beside_a_knockout_sibling_copies_both_planes() {
    // Members of an unstyled base: a 10×10 raster with (or without) a
    // Shallow knockout, and a drop-shadowed raster at opacity 0.5. With the
    // knockout the unit keeps the base-only snapshot (16·A) and `member_env`
    // hands BOTH planes to the package member, whose strip then copies two
    // windows too: 16·S·3, inside the max over the members.
    let run = |knockout: bool| {
        nest(move |doc| {
            let doc = raster(&doc, pattern(20, 20, 2), (30, 30), 1.0);
            let doc = raster(&doc, pattern(10, 10, 4), (32, 32), 1.0);
            let doc = clip_top(if knockout {
                style_top(&doc, SHALLOW)
            } else {
                doc
            });
            clip_top(styled_raster(&doc, (40, 40), DROP_SHADOW, 0.5))
        })
    };
    let with = 20 * A + 16 * A + (20 * A + 100).max(16 * S * 3);
    let without = 20 * A + 16 * S;
    let (k, plain) = (run(true), run(false));
    // The knockout-free twin's absolute figure: the member's planes beside
    // the unit buffer and its one strip.
    assert_nest(
        &plain,
        A,
        drop_shadow_planes(20, 20) + without,
        "the knockout-free twin",
    );
    assert_eq!(scratch(&k) - scratch(&plain), with - without);
    assert_renders(&k, "a package member beside a knockout");
    assert_renders(&plain, "its knockout-free twin");
}

#[test]
fn a_package_under_a_shallow_pass_through_level_copies_its_window() {
    // A plain (opacity 1, unmasked) pass-through group whose children hold a
    // visible Shallow knockout raster (10×10) and a drop-shadowed raster: the
    // level snapshots (charged), and the package's strip copies the Shallow
    // window beside itself: 16·S·2 — against the knockout's own unit pass,
    // 20·A + 100, in the level's max. The twin's raster is at opacity 1.
    let level = |opacity: f32| {
        nest(move |doc| {
            let doc = style_top(&raster(&doc, pattern(10, 10, 4), (30, 30), 1.0), SHALLOW);
            let doc = styled_raster(&doc, (50, 50), DROP_SHADOW, opacity);
            let children = top_level_from(&doc, doc.layers.len() - 2);
            group(&doc, &children, "Pass").0
        })
    };
    let (package, direct) = (level(0.5), level(1.0));
    let knockout = 20 * A + 100;
    // The direct twin's absolute figure: the level's snapshot is CHARGED (a
    // second `A` in the peak); uncharged, the raster's planes beside the
    // knockout's unit pass (the direct raster holds no buffer of its own).
    assert_nest(
        &direct,
        2 * A,
        drop_shadow_planes(20, 20) + knockout,
        "the direct twin",
    );
    assert_eq!(
        scratch(&package) - scratch(&direct),
        knockout.max(16 * S * 2) - knockout
    );
    assert_renders(&package, "a package under a Shallow level");
    assert_renders(&direct, "its direct twin");
}

#[test]
fn a_package_member_under_a_shallow_level_copies_no_knockout_window() {
    // 13A-6a carried item: a drop-shadowed MEMBER at opacity 0.5 (package)
    // under an unstyled base, inside a plain pass-through level that holds a
    // Shallow plane (a visible Shallow sibling UNIT beside the clip unit),
    // with no knockout in the clip run. `member_env` sets BOTH of a member's
    // knockout planes to the unit's base-only snapshot and never passes the
    // level's plane through; with no knockout member there is no snapshot,
    // both are `None`, and `windowed` copies no window — so the member's
    // strip is 16·S alone, and `2·[members_want_base_snapshot]` is exact.
    // The Shallow sibling renders planes (a drop shadow at opacity 1), so its
    // routed unit pass is exactly 20·A, never above the clip unit's.
    let level = |opacity: f32| {
        nest(move |doc| {
            let doc = style_top(
                &raster(&doc, pattern(10, 10, 4), (30, 30), 1.0),
                SHALLOW_SHADOWED,
            );
            let doc = raster(&doc, pattern(20, 20, 2), (50, 50), 1.0);
            let doc = clip_top(styled_raster(&doc, (52, 52), DROP_SHADOW, opacity));
            let children = top_level_from(&doc, doc.layers.len() - 3);
            group(&doc, &children, "Pass").0
        })
    };
    let (package, direct) = (level(0.5), level(1.0));
    assert_eq!(scratch(&package) - scratch(&direct), 16 * S);
    assert_renders(&package, "a package member under a Shallow level");
    assert_renders(&direct, "its direct twin");
}

#[test]
fn a_shadows_highlights_base_builds_its_guide_beside_the_unit_buffer() {
    let params = "{\"shadows\":{\"amount\":0.7,\"tone\":0.9},\"highlights\":{\"amount\":0.35,\
                  \"tone\":0.8},\"radius\":4}";
    let meta = adjust_meta("shadows_highlights", params);
    let doc = nest(|doc| {
        let mut doc = doc
            .adding_image_layer(doc.layers.len() - 1, RgbaImage::new(1, 1), "S/H")
            .expect("add layer");
        let top = doc.layers.len() - 1;
        doc.layers[top].meta = Some(meta.clone());
        // Its 1×1 pixel rect inside the frame, so the nest's extent stays
        // the frame's (an unmasked adjustment reaches the whole accumulator
        // wherever its pixels sit).
        doc.layers[top].offset = (50, 50);
        clip_top(raster(&doc, pattern(20, 20, 6), (40, 40), 1.0))
    });
    // The guide over the nest's 100×100 accumulator, sized by the shared
    // typed planner (the reference's own sizing).
    let precision = json!({"type":"adjust","op":"shadows_highlights","evaluation":"precision_v1",
        "params":{"shadows":{"amount":0.7},"radius":4.0}})
    .to_string();
    let (guide, _) =
        PrecisionAdjustment::guide_plan_for_description(&precision, FW as u32, FH as u32).unwrap();
    assert!(guide > 0);
    assert_eq!(
        scratch(&doc),
        nest_scratch(meta.len() as u64 * 256, A, 20 * A + guide)
    );
    assert_renders(&doc, "a Shadows/Highlights base");
}

#[test]
fn a_masked_pass_through_holds_its_crop() {
    // A pass-through group masked over 30×30 around a routed raster (Deep,
    // no planes: 20·A + 400 at its level). At opacity 1 the mask gates the
    // pass (its copy charged: A + A) and the crop sits beside the children;
    // at opacity 0 the crop alone (taken before the opacity check).
    let masked = |opacity: f32| {
        nest(move |doc| {
            let doc = styled_raster(&doc, (35, 35), DEEP, 1.0);
            let (doc, g) = group_top(&doc, "Masked");
            let mut doc = mask(&doc, g, (30, 30), (30, 30));
            doc.layers[g].opacity = opacity;
            doc
        })
    };
    assert_nest(&masked(1.0), A + A, A + (20 * A + 400), "masked at 1");
    assert_nest(&masked(0.0), A, A, "masked at 0");
}

#[test]
fn an_unmasked_pass_through_holds_only_its_childrens_figure() {
    let pass = |opacity: f32| {
        nest(move |doc| {
            let doc = styled_raster(&doc, (35, 35), DEEP, 1.0);
            let (mut doc, g) = group_top(&doc, "Pass");
            doc.layers[g].opacity = opacity;
            doc
        })
    };
    assert_nest(&pass(1.0), A, 20 * A + 400, "plain pass");
    // Gated by opacity: the copy is charged, nothing more uncharged.
    assert_nest(&pass(0.5), A + A, 20 * A + 400, "gated pass");
}

#[test]
fn sibling_units_take_the_max_and_a_nest_two_deep_the_sum() {
    // Two routed rasters side by side: 20·A + 400 and 20·A + 900 — the
    // larger, never both.
    let doc = nest(|doc| {
        let doc = styled_raster(&doc, (20, 20), DEEP, 1.0);
        style_top(&raster(&doc, pattern(30, 30, 7), (60, 50), 1.0), DEEP)
    });
    assert_nest(&doc, A, 20 * A + 900, "two siblings");
    // A styled group (the overlay) over a routed 20×20 raster inside the
    // nest: its synthetic and planes PLUS its own nest's figure — the routed
    // raster's unit pass over the group's 20×20 accumulator (20·400 + 400).
    let doc = nest(|doc| {
        let doc = styled_raster(&doc, (30, 30), DEEP, 1.0);
        let (mut doc, g) = group_top(&doc, "Inner");
        doc.layers[g].blend = BlendMode::Normal;
        styled(&doc, g, OVERLAY)
    });
    assert_nest(
        &doc,
        A + 400,
        4 * 400 + overlay_planes(20, 20) + (20 * 400 + 400),
        "two deep",
    );
}
