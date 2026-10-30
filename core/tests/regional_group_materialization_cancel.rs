//! Phase 13A-5b-ii-a: cancellation checkpoints inside the byte reference
//! compositor's chain the regional render runs to MATERIALIZE a styled group
//! — `doc_group::rendered_group` → `composite_level_into` →
//! `composite_unit_into` → `composite_clip_group_into` /
//! `composite_layer_into` → `composite_styled_into`, polling the job carried
//! in `CompositeEnv::job` (since 13A-5b-iii-b-2 the materialization's range
//! `[m0, m1]`, `m0 = pixels / work` the fill's close, with every inner call
//! site narrowed `checkpoints_only`, so the chain reports nothing and an
//! inner blur reports stage 6 at `m0`) — measured under the 13A-5a
//! CANCELLATION MODEL (`regional`'s module doc): a cancel issued in stage `s`
//! returns within `T_f + W_s · c_s`, `T_f` = 1 ms the return floor.
//!
//! Intervals (DECLARED before measuring): ONE ROW of the layer rect within
//! the buffer in `composite_layer_into`, `composite_adjustment_into`, the
//! knockout unit pass's mix loop and the gated lerp — and, since 13A-5b-ii-b,
//! ONE ROW of the buffer (or of the strip, plane, layer rect or crop) inside
//! `composite_pixels_into`, `composite_contribution`,
//! `clamp_alpha_to_shape`, `composite_buffer_into`, `BackdropSnapshot::of`,
//! a package strip's copy and mix, `encode_projection_with_job` /
//! `quantize_with_job`, `Shape::of_layer` and the mask crop — bounded by
//! `T_f + c_row`, `c_row = t_mat / ((N−1)·H)`; a nested guide build, a mask
//! feather (the group's own and every child layer's enabled mask) and a
//! style plane render each run their own 13A-5a / 13A-5b-i interval — a
//! child mask blur at factor 1 `T_f + 4096·c`, `c = t_blur / (2·pw·ph)`; a
//! child style plane render `T_f + max(4096·c, c_map)`, `c = t_style /
//! (2·pw·ph)`; still ONE unit: the buffer allocs, the `acc.to_vec()` /
//! `buf.clone()` snapshots, the alpha scans and resets, the coverage clone
//! and `drop(buf)` — each one memory-bound pass, bounded by `T_f + c_copy`,
//! `c_copy` the worker's own `acc.to_vec()`, the longest inter-checkpoint
//! gap of its own uncancelled chain (13A-5b-iii-a: `common::probe`'s
//! `Timeline`, read at the count the cancel was issued at).
//!
//! Calibration: `T(n)` is the first report above its own loop start
//! (`loop_start`, 13A-5b-iii-b-2) of an uncancelled run of the fixture with
//! `n` children (fresh fixtures); `t_mat = T(6) − T(1)`, the
//! cost of N − 1 = 5 child composites. `t_blur` / `t_style` are the same
//! differences against a feather-0 / size-0 twin. The TRIGGER is the job's
//! checkpoint count (`DrJobControl::checkpoints`): the test thread waits for
//! an entry, then for `k` more checkpoints or for the count to freeze
//! (`min(5 ms, longest gap / 2)`, the model's unpolled unit), and cancels;
//! the verdict — stage 1 at `m0` inside the chain (`inside_the_chain`),
//! stage 6 at `m0` inside an inner blur (`inside_the_inner_blur`) — is read
//! from the worker's own state at its return. Misses are
//! recorded, never widened. LOAD ROBUSTNESS (13A-5b-iii-b-1): the default `make
//! test` keeps the exactness, the pre-cancelled control (its return time
//! printed, not bounded), the INTERVAL guard (its PIN twin `#[ignore]`d)
//! and the two cache-integrity tests — since 13A-5b-iii-b-3 each
//! cancelled with the worker PARKED at `n0 + 1 + k` (`n0` the inner blur's
//! entry count in a fresh twin's own report log, `entry_count`; `k` 0 for (s),
//! half the child's blur for (c); `cancelled_parked_at` under a `Released`
//! guard), no poller; the measured PASS/MISS test (calibrations, freeze probes)
//! is `#[ignore]`d and runs through `make cancel-bench`.
//!
//! The oracle for exactness is `dr_doc_flattened` cropped here in plain index
//! arithmetic. Style and mask caches are `Arc`s every clone shares, so every
//! run that must render builds its fixture anew.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_regional::*;
use doctored_core::job_control::DrJobControl;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::Value;

use crate::common;
use common::probe::*;
use common::{adjust_meta, img_pixels, styled, take_err_string};

use crate::region_plan;
use crate::resource_report;

type Window = (u32, u32, u32, u32);

const W: u32 = 3000;
const H: u32 = 2000;
/// The mixed fixture's canvas.
const SW: u32 = 300;
const SH: u32 = 200;
/// Children of the measured group.
const N: usize = 6;

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

/// The independent oracle at divisor 1: the window's rows of the projection.
fn crop(full: &[u8], canvas_w: u32, (x, y, w, h): Window) -> Vec<u8> {
    let mut out = Vec::new();
    for j in y..y + h {
        let at = ((j * canvas_w + x) * 4) as usize;
        out.extend_from_slice(&full[at..at + w as usize * 4]);
    }
    out
}

/// The regional render at divisor 1 under a LIVE job control (the polled
/// path), never cancelled.
fn polled_region(doc: &DrDocument, (x, y, w, h): Window) -> Vec<u8> {
    let hd = handle(doc);
    let control = unsafe { dr_job_control_create() };
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, 1, u64::MAX, control, &mut err) };
    unsafe {
        dr_job_control_free(control);
        dr_doc_free(hd);
    }
    assert!(!img.is_null(), "region: {}", take_err_string(err));
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    out
}

/// The whole-canvas render of a clone of `doc` on `token`, run once.
fn whole(doc: &DrDocument) -> impl FnOnce(usize) -> (bool, String) + Send {
    let (w, h) = (doc.width, doc.height);
    let address = handle(doc) as usize;
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
                token as *const DrJobControl,
                &mut err,
            )
        };
        unsafe { dr_doc_free(address as *mut DrDocument) };
        let ok = !out.is_null();
        if ok {
            unsafe { dr_image_free(out) };
        }
        (ok, take_err_string(err))
    }
}

fn progress(control: *mut DrJobControl) -> f64 {
    unsafe { dr_job_control_progress(control) }
}

/// Bytes of derived / aligned mask planes the document's caches hold,
/// through the resource report (`MaskProperties::cached_planes`).
fn mask_cache_bytes(doc: &DrDocument) -> u64 {
    let hd = handle(doc);
    let ptrs = [hd as *const DrDocument];
    let report = resource_report::report(&ptrs, &[], &[], &[], 0, 0);
    unsafe { dr_doc_free(hd) };
    let report = report.expect("a resource report");
    report["categories"]["mask_cache_planes"]["unique_bytes"]
        .as_u64()
        .unwrap_or(0)
}

/// The byte plan of `doc` over its whole canvas (13A-5b-iii-b-2).
fn plan(doc: &DrDocument) -> Value {
    region_plan::byte(doc, (0, 0, doc.width, doc.height), 1, u64::MAX)
        .unwrap_or_else(|e| panic!("plan: {e}"))
}

/// The fraction done at the materialization's start, the fill's close
/// `m0 = pixels / work` (13A-5b-iii-b-2).
fn m0(doc: &DrDocument) -> f64 {
    let work = plan(doc)["work_units"].as_u64().unwrap() as f64;
    f64::from(doc.width) * f64::from(doc.height) / work
}

/// The first report above `doc`'s own loop start (`loop_start`).
fn after_loop_start(doc: &DrDocument) -> f64 {
    loop_start(&plan(doc)) + 1e-6
}

/// Runs `render` uncancelled while a SPINNING poller waits for progress
/// strictly above `above`; returns the time from the render's start.
fn first_report<F>(above: f64, render: F) -> Duration
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let control = unsafe { dr_job_control_create() };
    let token = control as usize;
    let out = thread::scope(|scope| {
        let start = Instant::now();
        let worker = scope.spawn(move || render(token));
        let deadline = start + Duration::from_secs(240);
        let value = loop {
            let value = progress(control);
            if value > above || worker.is_finished() {
                break value;
            }
            assert!(Instant::now() < deadline, "the stage never started");
            std::hint::spin_loop();
        };
        assert!(value > above, "render completed before {above}");
        let seen = start.elapsed();
        let (ok, message) = worker.join().unwrap();
        assert!(ok, "{message}");
        seen
    });
    unsafe { dr_job_control_free(control) };
    out
}

// ------------------------------------------------------------ fixtures --

/// Alpha 0 inside a grid of discs, an anti-aliased 4 px ramp, 255 between.
fn hole_alpha(x: u32, y: u32) -> f32 {
    let (dx, dy) = ((x % 97) as f32 - 48.0, (y % 83) as f32 - 41.0);
    ((dx * dx + dy * dy).sqrt() - 18.0) / 4.0
}

fn background(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
    }))
}

/// A canvas-sized raster with anti-aliased holes, colour varied by `seed`.
fn holed(w: u32, h: u32, seed: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let a = (hole_alpha(x + 13 * seed, y + 7 * seed) * 255.0).clamp(0.0, 255.0) as u8;
        Rgba([
            (x * 7 + seed * 40) as u8,
            (y * 3 + seed * 70) as u8,
            (160 + seed * 17) as u8,
            a,
        ])
    })
}

/// A layer with a real SHAPE: transparent outside an ellipse, graded alpha
/// near the rim.
fn shape_pixels(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let dx = (x as f32 + 0.5 - cx) / cx;
        let dy = (y as f32 + 0.5 - cy) / cy;
        let r = dx * dx + dy * dy;
        let alpha = if r > 1.0 {
            0
        } else if r > 0.7 {
            (60 + (x * 13 + y * 29) % 150) as u8
        } else {
            255
        };
        Rgba([
            (x * 37 + y * 11) as u8,
            (200 - (x * 5) % 200) as u8,
            (y * 23 + 7) as u8,
            alpha,
        ])
    })
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

/// A gradient coverage plane with a hard hole.
fn gradient(w: u32, h: u32) -> GrayImage {
    GrayImage::from_fn(w, h, |x, y| {
        if (x / 17 + y / 13) % 5 == 0 {
            Luma([0])
        } else {
            Luma([((x * 255) / (w - 1).max(1)) as u8 ^ (y * 9) as u8])
        }
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

/// An adjustment layer over a `size` pixel extent at `offset`, at the top.
fn with_adjustment(
    doc: &DrDocument,
    op: &str,
    params: &str,
    size: (u32, u32),
    offset: (i32, i32),
) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, RgbaImage::new(size.0, size.1), op)
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].meta = Some(adjust_meta(op, params));
    out.layers[top].offset = offset;
    out
}

/// An ENABLED mask on `idx`.
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

/// `group_layers` over `idx` (a new Pass Through group), and its index.
fn group(doc: &DrDocument, idx: &[usize], name: &str) -> (DrDocument, usize) {
    let (out, g, ..) = doc.group_layers(idx, name).expect("group");
    (out, g)
}

fn top(doc: &DrDocument) -> usize {
    doc.layers.len() - 1
}

fn drop_shadow(size: f32) -> String {
    format!("{{\"effects\":[{{\"type\":\"drop_shadow\",\"distance\":0,\"size\":{size}}}]}}")
}

/// (g) / (g1): a Background and an isolated group carrying a drop shadow
/// (distance 0, size 4) over `n` canvas-sized Normal 0.9 rasters with
/// anti-aliased holes, built anew per call (cold caches). Children are
/// `1..=n`, the group `n + 1`.
fn group_doc(n: usize) -> DrDocument {
    let mut doc = background(W, H);
    for i in 0..n {
        doc = with_raster(&doc, holed(W, H, i as u32), (0, 0), BlendMode::Normal, 0.9);
    }
    let children: Vec<usize> = (1..=n).collect();
    let (doc, g) = group(&doc, &children, "Group");
    styled(&doc, g, &drop_shadow(4.0))
}

/// (g′): (g) whose `n` children sit inside a GATED (opacity 0.6)
/// pass-through group inside the styled group — `composite_gated_pass`'s
/// unpolled `acc.to_vec()`. Children are `1..=n`, the gated group `n + 1`,
/// the styled group `n + 2`.
fn gated_group_doc(n: usize) -> DrDocument {
    let mut doc = background(W, H);
    for i in 0..n {
        doc = with_raster(&doc, holed(W, H, i as u32), (0, 0), BlendMode::Normal, 0.9);
    }
    let children: Vec<usize> = (1..=n).collect();
    let (mut doc, gated) = group(&doc, &children, "Gated");
    doc.layers[gated].opacity = 0.6;
    let (doc, g) = group(&doc, &[gated], "Group");
    styled(&doc, g, &drop_shadow(4.0))
}

/// The canvas-sized coverage plane of (c)'s child mask.
fn child_plane() -> GrayImage {
    GrayImage::from_fn(W, H, |x, y| {
        Luma([if (x / 40 + y / 30) % 3 == 0 {
            0
        } else {
            ((x * 5 + y * 11) % 256) as u8
        }])
    })
}

/// (c): (g1) whose one child raster carries an ENABLED canvas-sized mask at
/// `feather` (6: factor 1; 0: the calibration twin).
fn child_masked(feather: f32) -> DrDocument {
    masked(&group_doc(1), 1, child_plane(), (0, 0), 1.0, feather)
}

/// (s): (g) with 3 children whose TOP child raster also carries a satin of
/// `size` (4; 0: the calibration twin) at distance 1: since #2 part 3 the
/// box-pair law's style plane, whose rows report at stage 6 as the
/// Gaussian's did (the drop shadow it carried before runs the law since
/// #2).
fn child_styled(size: f32) -> DrDocument {
    styled(
        &group_doc(3),
        3,
        &format!("{{\"effects\":[{{\"type\":\"satin\",\"distance\":1,\"size\":{size}}}]}}"),
    )
}

const SHADOWS_HIGHLIGHTS: &str = "{\"shadows\":{\"amount\":0.7,\"tone\":0.9},\"highlights\":\
     {\"amount\":0.35,\"tone\":0.8},\"radius\":30}";

/// (h) at 300×200: a MATERIALIZED group (drop shadow, under a feathered (6)
/// canvas-sized group mask) holding a shaped raster under an enabled
/// feather-4 mask, a Multiply raster, a Levels adjustment under an enabled
/// feather-3 mask, a guided Shadows/Highlights (radius 30), a clip unit (a
/// raster base, an adjustment member and a raster member under an enabled
/// feather-3 mask), a gated (0.6) pass-through group, a nested styled
/// isolated group and a Shallow-knockout styled raster under an enabled
/// feather-3 mask. `member`: the group is a CLIP MEMBER of a raster base.
fn mixed(member: bool) -> DrDocument {
    let mut doc = background(SW, SH);
    if member {
        doc = with_raster(
            &doc,
            shape_pixels(260, 170),
            (15, 10),
            BlendMode::Normal,
            1.0,
        );
    }
    let mut children = Vec::new();
    // A shaped raster under an enabled feather-4 mask.
    doc = with_raster(
        &doc,
        shape_pixels(180, 120),
        (20, 15),
        BlendMode::Normal,
        1.0,
    );
    let at = top(&doc);
    doc = masked(&doc, at, gradient(170, 110), (25, 20), 0.9, 4.0);
    children.push(at);
    // A Multiply raster.
    doc = with_raster(
        &doc,
        top_pixels(150, 100),
        (120, 80),
        BlendMode::Multiply,
        0.8,
    );
    children.push(top(&doc));
    // A Levels adjustment under an enabled feather-3 mask.
    doc = with_adjustment(
        &doc,
        "levels",
        "{\"black\":0.1,\"white\":0.9,\"gamma\":1.8}",
        (200, 150),
        (40, 30),
    );
    let at = top(&doc);
    doc = masked(&doc, at, gradient(200, 150), (40, 30), 1.0, 3.0);
    children.push(at);
    // A guided Shadows/Highlights.
    doc = with_adjustment(
        &doc,
        "shadows_highlights",
        SHADOWS_HIGHLIGHTS,
        (SW, SH),
        (0, 0),
    );
    children.push(top(&doc));
    // A clip unit: a raster base, an adjustment member, a masked raster
    // member.
    doc = with_raster(
        &doc,
        shape_pixels(140, 110),
        (60, 50),
        BlendMode::Normal,
        0.9,
    );
    children.push(top(&doc));
    doc = with_adjustment(
        &doc,
        "hue_saturation",
        "{\"hue\":-60,\"saturation\":0.2}",
        (1, 1),
        (0, 0),
    );
    let at = top(&doc);
    doc.layers[at].clipped = true;
    children.push(at);
    doc = with_raster(&doc, top_pixels(120, 90), (90, 70), BlendMode::Screen, 0.8);
    let at = top(&doc);
    doc.layers[at].clipped = true;
    doc = masked(&doc, at, gradient(110, 80), (95, 75), 1.0, 3.0);
    children.push(at);
    // A gated (0.6) pass-through group.
    doc = with_raster(
        &doc,
        shape_pixels(110, 80),
        (150, 20),
        BlendMode::Normal,
        1.0,
    );
    let (next, gated) = group(&doc, &[top(&doc)], "Gated");
    doc = next;
    doc.layers[gated].opacity = 0.6;
    children.push(gated);
    // A nested styled isolated group.
    doc = with_raster(&doc, top_pixels(90, 70), (30, 110), BlendMode::Normal, 1.0);
    let (next, nested) = group(&doc, &[top(&doc)], "Nested");
    doc = styled(
        &next,
        nested,
        "{\"effects\":[{\"type\":\"outer_glow\",\"size\":3}]}",
    );
    children.push(nested);
    // A Shallow-knockout styled raster under an enabled feather-3 mask.
    doc = with_raster(
        &doc,
        shape_pixels(100, 90),
        (170, 90),
        BlendMode::Normal,
        1.0,
    );
    let at = top(&doc);
    doc = styled(
        &doc,
        at,
        "{\"knockout\":\"shallow\",\"fill_opacity\":0.5,\"effects\":[{\"type\":\"drop_shadow\",\
         \"distance\":3,\"size\":2}]}",
    );
    doc = masked(&doc, at, gradient(100, 90), (170, 90), 1.0, 3.0);
    children.push(at);
    let (doc, g) = group(&doc, &children, "Materialized");
    let doc = styled(&doc, g, &drop_shadow(4.0));
    let mut doc = masked(&doc, g, gradient(SW, SH), (0, 0), 0.9, 6.0);
    if member {
        doc.layers[g].clipped = true;
    }
    doc
}

// ------------------------------------------------ the planes' own sizing --
//
// Written out independently of the core (13A-5a / 13A-5b-i): a mask's sigma
// is 0.3·(feather − 1) + 0.8; the blur works at factor 1 up to sigma 3,
// then 2, 4, 8 at each doubling; its reach is the kernel's half width plus
// one, and above sigma 3 two more resampling pixels, scaled by the factor.
// A mask's pad is that reach. The (s) satin runs the box-pair law at spread
// 0 since #2 part 3 (`style_fx_satin`), no Gaussian: its matte's reach is
// the box width `w`, the size rounded half up, and its plane pad (at
// distance 1) `w + 1 + 1`.

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

fn blur_reach(sigma: f32) -> u32 {
    if sigma <= 0.0 {
        return 0;
    }
    let f = factor(sigma);
    if f == 1 {
        kernel_radius(sigma).ceil() as u32 + 1
    } else {
        let ff = f as f32;
        let reduced = (sigma * sigma - ff * ff / 3.0).max(0.0).sqrt() / ff;
        f * (kernel_radius(reduced).ceil() as u32 + 3)
    }
}

/// The child mask's padded plane `(pw, ph)` at `feather`.
fn padded(feather: f32) -> (u32, u32) {
    let pad = blur_reach(0.3 * (feather - 1.0) + 0.8);
    (W + 2 * pad, H + 2 * pad)
}

/// The padded plane `(pw, ph)` of a canvas-sized raster's satin at
/// distance 1: the law's reach `w` (the size rounded half up, #2 part 3)
/// plus `ceil(distance)`, pad the reach plus one.
fn plane(size: f32) -> (u32, u32) {
    let pad = (size + 0.5).floor() as u32 + 1 + 1;
    (W + 2 * pad, H + 2 * pad)
}

// ---------------------------------------------------------- exactness --

const WINDOWS: [Window; 3] = [(0, 0, W, H), (1234, 777, 301, 203), (W - 17, 0, 17, 9)];
const SMALL_WINDOWS: [Window; 3] = [(0, 0, SW, SH), (123, 77, 31, 23), (SW - 17, 0, 17, 9)];

fn assert_polled_matches(build: &dyn Fn() -> DrDocument, windows: &[Window], what: &str) {
    let reference = full(&build());
    let canvas_w = build().width;
    for &window in windows {
        // A fresh document per render: cold caches run the polled chain.
        let got = polled_region(&build(), window);
        assert!(
            got == crop(&reference, canvas_w, window),
            "{what}: window {window:?}"
        );
    }
}

#[test]
fn the_polled_materialization_matches_the_full_frame() {
    assert_polled_matches(&|| group_doc(N), &WINDOWS, "(g) six children");
    assert_polled_matches(
        &|| child_masked(6.0),
        &WINDOWS,
        "(c) a feathered child mask",
    );
    assert_polled_matches(&|| child_styled(4.0), &WINDOWS, "(s) a styled child");
}

#[test]
fn the_polled_mixed_materialization_matches_the_full_frame() {
    assert_polled_matches(&|| mixed(false), &SMALL_WINDOWS, "(h)");
    assert_polled_matches(&|| mixed(true), &SMALL_WINDOWS, "(h) as a clip member");
}

#[test]
fn a_pre_cancelled_control_returns_at_once_with_no_output() {
    for doc in [
        group_doc(N),
        child_masked(6.0),
        child_styled(4.0),
        mixed(false),
        mixed(true),
    ] {
        let hd = handle(&doc);
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let mut err: *mut c_char = ptr::null_mut();
        let started = Instant::now();
        let out = unsafe {
            dr_doc_render_region(
                hd,
                0,
                0,
                doc.width,
                doc.height,
                1,
                u64::MAX,
                control,
                &mut err,
            )
        };
        let took = started.elapsed();
        assert!(out.is_null());
        assert_eq!(take_err_string(err), "operation cancelled");
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        eprintln!(
            "13A-5b-iii-b-1 pre-cancelled control returned in {:.3} ms (no bound asserted)",
            took.as_secs_f64() * 1e3
        );
        unsafe {
            dr_job_control_free(control);
            dr_doc_free(hd);
        }
    }
}

// --------------------------------------------------- reports-once guards --
//
// `rendered_group`'s chain reports nothing inside its range (13A-5b-iii-b-2:
// `[m0, m1]`, `m0 = pixels / work` the fill's close, `m1 = pixels·(2 + N) /
// work` its advance — the buffer and one pass per child row): a REPORT LOG
// of every distinct progress value shows no value strictly between `m0` and
// `m1`, nor between the planes' advance `s1 = 1 − (N + 2)·pixels / work` (the
// loop's start) and the first row's report `s1 + (N + 2)·W / work`
// (INTERVAL), and 1.0 is never pinned while the render still runs (PIN).
// The log and both signatures are `common::probe`'s (13A-5b-iii-a). A stall
// can only make them MISS a leak, but the PIN is a ratio of two wall-clock
// readings: the INTERVAL runs in default `make test` (13A-5b-iii-b-1), the
// PIN in its `#[ignore]`d twin, through `make cancel-bench`.

#[test]
fn the_materialization_leaks_no_report_into_the_root_range() {
    let doc = group_doc(N);
    let work = plan(&doc)["work_units"].as_u64().unwrap() as f64;
    let pixels = f64::from(W) * f64::from(H);
    let rows = (N + 2) as f64;
    let (m0, m1) = (pixels / work, pixels * rows / work);
    let s1 = 1.0 - rows * pixels / work;
    let (log, _) = logged(whole(&doc));
    assert_interval(&log, m0, m1, "(g) the materialization");
    assert_interval(&log, s1, s1 + rows * f64::from(W) / work, "(g) the loop");
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn the_materialization_is_not_pinned_at_its_end() {
    let (log, tf) = logged(whole(&group_doc(N)));
    assert_pin(&log, tf, "(g)");
}

// ------------------------------------------------------ cache integrity --

#[test]
fn a_cancel_inside_the_inner_style_render_caches_no_partial_plane() {
    // Document A: cancelled PARKED at the first checkpoint after the INNER
    // raster's first blur report (inside `rendered_group`; the entry count
    // `n0` from a fresh twin's own report log, 13A-5b-iii-b-3), then
    // rendered again uncancelled — equal to an independently built B's full
    // render.
    let a = child_styled(4.0);
    let start = m0(&a);
    let n0 = entry_count(&at_the_blur, whole(&child_styled(4.0)));
    let p = cancelled_parked_at(n0 + 1, &inside_the_inner_blur(start), whole(&a));
    let got = polled_region(&a, (0, 0, W, H));
    assert!(
        got == full(&child_styled(4.0)),
        "a cancelled inner render left a partial plane in the cache"
    );
    eprintln!(
        "13A-5b-iii-b-3 cache integrity, (s) the inner satin: at its first blur report, \
         {}; the re-render equals the reference",
        parked_line(&p),
    );
}

#[test]
fn a_cancel_inside_a_child_mask_blur_caches_no_partial_plane() {
    // Document A: cancelled PARKED half a pass into the CHILD's mask blur
    // (the (m4) trigger: `n0 + 1 + k`, the entry count `n0` from a fresh
    // twin's own report log, 13A-5b-iii-b-3), so nothing may be cached for
    // that mask; then rendered again uncancelled — equal to B, and now the
    // cache holds the plane.
    let a = child_masked(6.0);
    let (pw, ph) = padded(6.0);
    let k = (u64::from(pw) * u64::from(ph)).div_ceil(4096) / 2;
    let start = m0(&a);
    let n0 = entry_count(&at_the_blur, whole(&child_masked(6.0)));
    let p = cancelled_parked_at(n0 + 1 + k, &inside_the_inner_blur(start), whole(&a));
    assert_eq!(
        mask_cache_bytes(&a),
        0,
        "a cancelled child mask blur cached a plane"
    );
    let got = polled_region(&a, (0, 0, W, H));
    assert!(
        got == full(&child_masked(6.0)),
        "a cancelled child mask blur left a partial plane in the cache"
    );
    assert!(
        mask_cache_bytes(&a) > 0,
        "the completed render caches its plane"
    );
    eprintln!(
        "13A-5b-iii-b-3 cache integrity, (c) the child's feather-6 mask: inside its blur, {}; no \
         plane cached; the re-render equals the reference",
        parked_line(&p),
    );
}

// ------------------------------------------------ measured cancellation --
//
// DECLARED (before measuring), the module doc's intervals. Printed: the
// latency, the calibrated costs, the bound, the intervals past the floor and
// PASS/MISS. Misses are recorded, never widened. A wall-clock verdict:
// `#[ignore]`d, run through `make cancel-bench` (13A-5b-iii-b-1).

/// `c_map`: one `multiply_planes`-shaped pass, `a = (a·b + 127) / 255`, over
/// two `pw·ph` buffers, timed here (13A-5b-i's one-unit bound).
fn c_map((pw, ph): (u32, u32)) -> f64 {
    let n = pw as usize * ph as usize;
    let mut a: Vec<u8> = (0..n).map(|i| (i * 7) as u8).collect();
    let b: Vec<u8> = (0..n).map(|i| (i * 13 + 5) as u8).collect();
    let started = Instant::now();
    for (x, &y) in a.iter_mut().zip(&b) {
        *x = ((u32::from(*x) * u32::from(y) + 127) / 255) as u8;
    }
    let took = started.elapsed().as_secs_f64();
    std::hint::black_box(&a);
    took
}

/// One probe's line, on 13A-5b-i's `report` shape.
fn report(what: &str, p: &Probe, window: (Duration, Duration), costs: &str, unit: f64, name: &str) {
    let bound = RETURN_FLOOR_S + unit;
    eprintln!(
        "13A-5b-ii-a {what}: {:.3} ms cancel-to-return (issued at {:.1} ms, calibration window \
         ≈ {:.1}–{:.1} ms, difference {:.1} ms; {}); {costs}; bound T_f + {name} = {:.3} ms (T_f \
         1 ms); ≈{:.2} intervals past the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        window.0.as_secs_f64() * 1e3,
        window.1.as_secs_f64() * 1e3,
        window.1.saturating_sub(window.0).as_secs_f64() * 1e3,
        checkpoints_line(p),
        bound * 1e3,
        past(p.latency, unit),
        verdict(p.latency, bound),
    );
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-b-1)"]
fn a_concurrently_cancelled_materialization_returns_within_the_model() {
    // Calibration: T(1), T(N) — t_mat, the cost of N − 1 child composites.
    let t1 = first_report(after_loop_start(&group_doc(1)), whole(&group_doc(1)));
    let tn = first_report(after_loop_start(&group_doc(N)), whole(&group_doc(N)));
    let t_mat = tn.saturating_sub(t1).as_secs_f64();
    let n_1 = (N - 1) as f64;
    let c_row = t_mat / (n_1 * f64::from(H));

    // (m1) (g): past the fill, then N·H/2 checkpoints — inside a child's row
    // loop, one row per checkpoint.
    let k = (N as u64) * u64::from(H) / 2;
    let p = cancelled_after_checkpoints(
        &past_the_fill(W, H),
        k,
        &inside_the_chain(m0(&group_doc(N))),
        whole(&group_doc(N)),
    );
    report(
        "8-bit cancel inside rendered_group's child row loop ((g), N = 6)",
        &p,
        (t1, tn),
        &format!(
            "c_row = t_mat / ((N−1)·H) = {:.3} µs per row (t_mat {:.1} ms)",
            c_row * 1e6,
            t_mat * 1e3
        ),
        c_row,
        "c_row",
    );

    // (m2) (g′): past the fill, then a freeze of the count for
    // `min(5 ms, longest gap / 2)` — an unpolled unit (the gated pass's
    // `acc.to_vec()`). Calibrated BEFORE the probe on the worker's own
    // uncancelled chain: the checkpoint timeline from past the fill to the
    // styled group's own drop-shadow box pair (the first stage-6 report,
    // after the materialization); `c_copy` is its gap at the count the
    // cancel was issued at, and its longest gap's count is printed beside
    // `issued` as identification (expected equal), never asserted.
    // The timeline is recorded TWICE and the second kept: the first run pays
    // the gated pass's `acc.to_vec()` on cold pages (measured 7–9 ms when
    // this test runs alone, against ≈2 ms in the probe's run, so its
    // `longest / 2` freeze never fired inside the chain); the second runs
    // under the probe's own warm conditions.
    let _cold = timeline(
        &past_the_fill(W, H),
        &at_the_blur,
        whole(&gated_group_doc(N)),
    );
    let t = timeline(
        &past_the_fill(W, H),
        &at_the_blur,
        whole(&gated_group_doc(N)),
    );
    let (longest_at, longest) = t.longest().expect("a gap");
    let freeze = FREEZE.min(longest / 2);
    let p = cancelled_in_freeze(
        &past_the_fill(W, H),
        freeze,
        &inside_the_chain(m0(&gated_group_doc(N))),
        whole(&gated_group_doc(N)),
    );
    let copy = t
        .gap_at(p.issued)
        .expect("issued inside the timeline")
        .as_secs_f64();
    report(
        "8-bit cancel in rendered_group's unpolled unit ((g′), the freeze probe)",
        &p,
        (t1, tn),
        &format!(
            "c_copy = the timeline's gap at the issued count {} = {:.3} ms (longest gap {:.3} ms \
             at count {longest_at}; freeze {:.3} ms)",
            p.issued,
            copy * 1e3,
            longest.as_secs_f64() * 1e3,
            freeze.as_secs_f64() * 1e3
        ),
        copy,
        "c_copy",
    );

    // (m3) (s): the flip into the INNER raster's satin blur, reached
    // through the chain's `rendered_with_job`; k = 0.
    let (pw, ph) = plane(4.0);
    let map = c_map((pw, ph));
    let t0 = first_report(
        after_loop_start(&child_styled(0.0)),
        whole(&child_styled(0.0)),
    );
    let ts = first_report(
        after_loop_start(&child_styled(4.0)),
        whole(&child_styled(4.0)),
    );
    let t_style = ts.saturating_sub(t0).as_secs_f64();
    let c = t_style / (2.0 * f64::from(pw) * f64::from(ph));
    let p = cancelled_after_checkpoints(
        &at_the_blur,
        0,
        &inside_the_inner_blur(m0(&child_styled(4.0))),
        whole(&child_styled(4.0)),
    );
    report(
        "8-bit cancel inside a child's style plane render in rendered_group ((s), size 4, \
         box pair)",
        &p,
        (t0, ts),
        &format!(
            "c = t_style / (2·pw·ph) = {:.3} ns per cell (2·{pw}·{ph} cells), c_map {:.3} ms",
            c * 1e9,
            map * 1e3
        ),
        (4096.0 * c).max(map),
        "max(4096·c, c_map)",
    );

    // (m4) (c): the flip into the CHILD's mask blur (the first stage-6
    // report: `rendered_group` completes before the group's own planes),
    // reached through `active_mask_with_job`; k = half a pass.
    let (pw, ph) = padded(6.0);
    let tu = first_report(
        after_loop_start(&child_masked(0.0)),
        whole(&child_masked(0.0)),
    );
    let tb = first_report(
        after_loop_start(&child_masked(6.0)),
        whole(&child_masked(6.0)),
    );
    let t_blur = tb.saturating_sub(tu).as_secs_f64();
    let c = t_blur / (2.0 * f64::from(pw) * f64::from(ph));
    let k = (u64::from(pw) * u64::from(ph)).div_ceil(4096) / 2;
    let p = cancelled_after_checkpoints(
        &at_the_blur,
        k,
        &inside_the_inner_blur(m0(&child_masked(6.0))),
        whole(&child_masked(6.0)),
    );
    report(
        "8-bit cancel inside a child's mask feather blur in rendered_group ((c), feather 6, \
         factor 1)",
        &p,
        (tu, tb),
        &format!(
            "c = t_blur / (2·pw·ph) = {:.3} ns per cell (2·{pw}·{ph} cells)",
            c * 1e9
        ),
        4096.0 * c,
        "4096·c",
    );
}
