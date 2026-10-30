//! Phase 13A-5b-ii-b: per-ROW cancellation checkpoints inside the byte
//! style/knockout primitives the regional render's MATERIALIZATION runs
//! (`doc_group::rendered_group` under `CompositeEnv::job`, since
//! 13A-5b-iii-b-2 the materialization's range with every inner call site
//! narrowed `checkpoints_only`, so it reports nothing and progress holds at
//! the range's start, the fill's close `pixels / work`) —
//! `composite_pixels_into`, `composite_contribution`,
//! `clamp_alpha_to_shape`, `composite_buffer_into`, `BackdropSnapshot::of`,
//! the package strip's copy and mix, `encode_projection_with_job` /
//! `quantize_with_job`, `Shape::of_layer` and the group mask's crop —
//! measured under the 13A-5a CANCELLATION MODEL (`regional`'s module doc): a
//! cancel issued in stage `s` returns within `T_f + W_s · c_s`, `T_f` = 1 ms
//! the return floor.
//!
//! Intervals (DECLARED before measuring): ONE ROW of the buffer (or of the
//! strip, plane, layer rect or crop) inside each of those primitives, bounded
//! by `T_f + c_row`, `c_row = t_chain / n_chain` — the chain's mean cost per
//! checkpoint over its own uncancelled run (`t_chain` / `n_chain` the time
//! and checkpoint count from past the root fill to the group's own blur
//! flip, the first stage-6 report). Still ONE unit: the buffer allocs, the
//! `acc.to_vec()` / `buf.clone()` snapshots, the alpha scans and resets, the
//! coverage clone and `drop(buf)` (`regional_group_materialization_cancel`'s
//! (m2) measures that bound, `T_f + c_copy`).
//!
//! Fixture (q), 3000×2000, whole canvas, divisor 1: a Background; child A, a
//! styled CLIP BASE (a colour overlay — an enabled effect with no blur, so no
//! stage flip inside the chain — a non-identity Blend If, fill 0.8, opacity
//! 0.5: the PACKAGE path — strip copy, the full-rect `BackdropSnapshot::of`,
//! pixels into the private buffer, the raster member, `clamp_alpha_to_shape`,
//! `composite_buffer_into`, the Interior contribution, the mix) with one
//! canvas-sized raster member; child B, a styled clip base (fill 0.9, NO
//! effects → `Shape::of_layer(.., 0)`) with one raster member; A and B in an
//! isolated group carrying a drop shadow (size 4) and an ENABLED
//! canvas-sized mask at feather 0, density 0.9 (the crop loop, not the
//! Borrowed fast path).
//!
//! The TRIGGER is the job's checkpoint count (`common::probe`): past the
//! root fill, then `⌊n_chain·j/8⌋` more checkpoints, j ∈ {1, 3, 5, 7}; the
//! verdict (stage 1 at the fill's close `pixels / work`,
//! `inside_the_chain`) is read from the worker's own state at its return.
//! Cost calibration stays time-based. Misses are recorded, never
//! widened. LOAD ROBUSTNESS (13A-5b-iii-b-1): the default `make test` keeps
//! the exactness, the pre-cancelled control (its return time printed, not
//! bounded), the INTERVAL guard and the zero-width pass; the measured
//! PASS/MISS test and the PIN guard are `#[ignore]`d and run
//! through `make cancel-bench`.
//!
//! The oracle for exactness is `dr_doc_flattened` cropped here in plain index
//! arithmetic. Style and mask caches are `Arc`s every clone shares, so every
//! run that must render builds its fixture anew.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument, LayerRaster};
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
use common::{img_pixels, styled, take_err_string};

use crate::region_plan;

type Window = (u32, u32, u32, u32);

const W: u32 = 3000;
const H: u32 = 2000;
const WINDOWS: [Window; 3] = [(0, 0, W, H), (1234, 777, 301, 203), (W - 17, 0, 17, 9)];

// ------------------------------------------------------------- plumbing --
//
// Copied from `regional_group_materialization_cancel.rs`.

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

/// The byte plan of `doc` over its whole canvas (13A-5b-iii-b-2).
fn plan(doc: &DrDocument) -> Value {
    region_plan::byte(doc, (0, 0, doc.width, doc.height), 1, u64::MAX)
        .unwrap_or_else(|e| panic!("plan: {e}"))
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

/// `pixels` at `offset`, Normal at `opacity`, pushed on top.
fn with_raster(
    doc: &DrDocument,
    pixels: RgbaImage,
    offset: (i32, i32),
    opacity: f32,
    clipped: bool,
) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, pixels, "Raster")
        .expect("add layer");
    let top = out.layers.len() - 1;
    let layer = &mut out.layers[top];
    layer.offset = offset;
    layer.blend = BlendMode::Normal;
    layer.opacity = opacity;
    layer.clipped = clipped;
    out
}

const BLEND_IF: &str = "\"blend_if\":{\"channel\":\"gray\",\"this_layer\":[10,60,180,240],\
     \"underlying\":[0,40,200,255]}";

/// (q) (module doc), built anew per call (cold caches). A is layer 1 (its
/// member 2), B layer 3 (its member 4), the group 5.
fn q() -> DrDocument {
    let mut doc = background(W, H);
    doc = with_raster(&doc, holed(W, H, 1), (0, 0), 0.5, false);
    let a = doc.layers.len() - 1;
    doc = styled(
        &doc,
        a,
        &format!(
            "{{\"fill_opacity\":0.8,{BLEND_IF},\"effects\":[{{\"type\":\"color_overlay\",\
             \"color\":\"#3080c0\",\"opacity\":0.7}}]}}"
        ),
    );
    doc = with_raster(&doc, top_pixels(W, H), (0, 0), 0.8, true);
    doc = with_raster(&doc, shape_pixels(1800, 1300), (400, 300), 1.0, false);
    let b = doc.layers.len() - 1;
    doc = styled(&doc, b, "{\"fill_opacity\":0.9}");
    doc = with_raster(&doc, holed(1500, 1100, 3), (700, 500), 0.9, true);
    let (mut doc, g, ..) = doc.group_layers(&[1, 2, 3, 4], "Group").expect("group");
    let mut properties = MaskProperties::at((0, 0));
    properties.density = 0.9;
    properties.feather = 0.0;
    let layer = &mut doc.layers[g];
    layer.mask = Some(Arc::new(gradient(W, H)));
    layer.mask_enabled = true;
    layer.mask_properties = properties;
    styled(
        &doc,
        g,
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":0,\"size\":4}]}",
    )
}

// ---------------------------------------------------------- exactness --

#[test]
fn the_polled_primitives_match_the_full_frame() {
    let reference = full(&q());
    for window in WINDOWS {
        // A fresh document per render: cold caches run the polled chain.
        let got = polled_region(&q(), window);
        assert!(got == crop(&reference, W, window), "(q): window {window:?}");
    }
}

#[test]
fn a_pre_cancelled_control_returns_at_once_with_no_output() {
    let doc = q();
    let hd = handle(&doc);
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let mut err: *mut c_char = ptr::null_mut();
    let started = Instant::now();
    let out = unsafe { dr_doc_render_region(hd, 0, 0, W, H, 1, u64::MAX, control, &mut err) };
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

// --------------------------------------------------- reports-once guards --
//
// Nothing reports between the loop's start `L` (`loop_start`,
// 13A-5b-iii-b-2: the materialization and the group's planes advanced) and
// the first row's report `L + 6·W / work` (six rows): a REPORT LOG of every
// distinct progress value shows no value strictly between them (INTERVAL),
// and 1.0 is never pinned while the render still runs (PIN).
// The log helpers are `common::probe`'s (the local copies folded in
// 13A-5b-iii-b-1). A stall can only make them MISS a leak, but the PIN is a
// ratio of two wall-clock readings: the INTERVAL runs in default
// `make test` and the PIN in its `#[ignore]`d twin, through `make cancel-bench`.

#[test]
fn the_polled_primitives_leak_no_report_into_the_root_range() {
    let p = plan(&q());
    let work = p["work_units"].as_u64().unwrap() as f64;
    let l = loop_start(&p);
    let (log, _) = logged(whole(&q()));
    assert_interval(&log, l, l + 6.0 * f64::from(W) / work, "(q)");
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn the_polled_primitives_are_not_pinned_at_their_end() {
    let (log, tf) = logged(whole(&q()));
    assert_pin(&log, tf, "(q)");
}

// ------------------------------------------------ measured cancellation --
//
// DECLARED (before measuring), the module doc's row interval. Printed: the
// latency, the calibrated costs, the bound, the intervals past the floor and
// PASS/MISS. Misses are recorded, never widened. A wall-clock verdict:
// `#[ignore]`d, run through `make cancel-bench` (13A-5b-iii-b-1).

/// One probe's line, on 13A-5b-ii-a's `report` shape.
fn report(what: &str, p: &Probe, window: (Duration, Duration), costs: &str, unit: f64, name: &str) {
    let bound = RETURN_FLOOR_S + unit;
    eprintln!(
        "13A-5b-ii-b {what}: {:.3} ms cancel-to-return (issued at {:.1} ms, calibration window \
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
fn a_cancel_inside_the_primitive_rows_returns_within_the_model() {
    // Calibration: from past the root fill to the group's own blur flip.
    let ((t_entry, n_entry), (t_exit, n_exit)) =
        window(&past_the_fill(W, H), &|_, stage, _| stage == 6, whole(&q()));
    let t_chain = t_exit.saturating_sub(t_entry).as_secs_f64();
    let n_chain = n_exit.saturating_sub(n_entry);
    assert!(n_chain > 0, "the chain polled nothing");
    let c_row = t_chain / n_chain as f64;
    let work = plan(&q())["work_units"].as_u64().unwrap() as f64;
    let start = f64::from(W) * f64::from(H) / work;
    for j in [1u64, 3, 5, 7] {
        let k = n_chain * j / 8;
        let p = cancelled_after_checkpoints(
            &past_the_fill(W, H),
            k,
            &inside_the_chain(start),
            whole(&q()),
        );
        report(
            &format!("8-bit cancel {j}/8 of the way through rendered_group's primitive rows ((q))"),
            &p,
            (t_entry, t_exit),
            &format!(
                "c_row = t_chain / n_chain = {:.3} µs per checkpoint (t_chain {:.1} ms, n_chain \
                 {n_chain})",
                c_row * 1e6,
                t_chain * 1e3
            ),
            c_row,
            "c_row",
        );
    }
}

// ------------------------------------------- the zero-width gated pass --

#[test]
fn a_gated_pass_over_a_zero_width_canvas_does_not_panic() {
    let mut doc = DrDocument::from_pixels(RgbaImage::new(0, 3));
    // `adding_image_layer` refuses the zero-width canvas, so the 1×1 child
    // is pushed directly: a copy of the Background with its own pixels and id.
    let mut child = doc.layers[0].clone();
    child.pixels = LayerRaster::new(RgbaImage::from_pixel(1, 1, Rgba([10, 20, 30, 255])));
    child.name = "Child".into();
    child.id = doc.layers[0].id + 1;
    doc.layers.push(child);
    let (mut doc, g, ..) = doc.group_layers(&[1], "Gated").expect("group");
    doc.layers[g].opacity = 0.6;
    let flat = doc.flattened().expect("a zero-width projection");
    assert_eq!(flat.dimensions(), (0, 3));
}
