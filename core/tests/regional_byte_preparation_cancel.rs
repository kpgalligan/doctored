//! Phase 13A-5a: cancellation checkpoints inside the two byte PREPARATION
//! blurs the regional render runs before its loops — the mask feather
//! (`MaskProperties::derived_with_job`) and the byte guide's blur
//! (`adjust_guide`'s u8 `GuideSample::blur`) — measured under the 13A-5a
//! CANCELLATION MODEL (`regional`'s module doc): a cancel issued in stage `s`
//! returns within `T_f + W_s · c_s`, `T_f` = 1 ms the return floor, `W_s` the
//! stage's checkpoint interval and `c_s` its per-unit cost calibrated on its
//! OWN uncancelled run here. Intervals (declared before measuring): the byte
//! mask blur at factor 1, 4096 cells per `feather` pass; at factor > 1, ONE
//! Triangle resample of the extended plane — since 13A-5b-iii-a its cost is
//! the worker's own: the gap of the checkpoint TIMELINE (`common::probe`'s
//! `timeline`, an uncancelled run from the blur's entry to the first root
//! report) at the count the cancel was issued at, the freeze
//! `min(5 ms, longest gap / 2)`; the byte guide build, 4096 cells (its grid
//! pass, `c` from stage 0's last report to the guide blur's first). Since
//! 13A-5b-iii-b-2 the unstaged fill reports and the plan counts the mask
//! blur, which reports INSIDE its range `[b0, b1]` (`b0 = pixels / work`,
//! the fill's close; `b1 = 1 − 2·pixels / work`, the loop's start) at stage
//! 6 while every driver report during preparation carries stage 1, so the
//! blur's entry is the first stage-6 report (`at_the_blur`) and its verdict
//! stage 6 at or below `b1` (`inside_the_blur(b1)`); the guide's grid pass
//! reports nothing (since 13A-5b-iii-a the guide's blur reports INSIDE its
//! range), so every cancel is TRIGGERED by the job's CHECKPOINT COUNT
//! (`DrJobControl::checkpoints`,
//! 13A-5b-ii-a — load-independent, where the 13A-5a time probe misfired past
//! the blur under load): the test thread waits for the stage's entry, then
//! for `k` more checkpoints (or for the count to FREEZE — the model's
//! unpolled unit), and cancels within the poll gap after a checkpoint, so the
//! cancel is observed at the NEXT one — the model's worst case. The verdict
//! is read post hoc from the worker's own state at its return; a probe the
//! render outruns fails naming the poller stall. The probe helpers are
//! `common::probe`'s (folded in 13A-5b-iii-a). The oracle for
//! exactness is `dr_doc_flattened` cropped here in plain index arithmetic.
//! The derived-mask cache is an `Arc` every clone shares, so every run that
//! must blur builds its fixture anew. The reports-once guards (13A-5b-i) read
//! a REPORT LOG of every distinct progress value (`logged`) for two
//! load-independent leak signatures — a value strictly between the pre-stage
//! value and the first legitimate report, and progress pinned at 1.0 while
//! the render still runs — so under load they can only miss a leak, never
//! report one falsely (the section comment); since 13A-5b-iii-a the guide's
//! guard reads a RAMP instead (`assert_ramp`, the section comment), and since
//! 13A-5b-iii-b-2 so does the mask blur's bench test, while its default guard
//! reads the INTERVAL from the blur's advance `b1` to the first row.
//! LOAD ROBUSTNESS (13A-5b-iii-b-1): the default `make test` keeps the
//! exactness, the pre-cancelled control (its return time printed, not bounded),
//! the cache-integrity test and the mask blur's INTERVAL guard (its PIN moved
//! to the bench test); the guide's RAMP guard and the measured
//! PASS/MISS test (freeze probes, calibrations) are `#[ignore]`d and run
//! through `make cancel-bench`. Since 13A-5b-iii-b-3 the cache-integrity
//! test races no poller: it takes the blur's ENTRY COUNT `n0` from a fresh
//! twin's own report log (`entry_count`) and cancels with the worker PARKED at
//! `n0 + 1`, the first checkpoint after the entry report (`cancelled_parked_at`,
//! under a `Released` guard); the STALL wording above is the bench probes'.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_regional::*;
use doctored_core::job_control::DrJobControl;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::probe::*;
use common::{adjust_meta, img_pixels, take_err_string};

use crate::region_plan;

type Window = (u32, u32, u32, u32);

const W: u32 = 3000;
const H: u32 = 2000;

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

fn plan(doc: &DrDocument) -> Value {
    region_plan::byte(doc, (0, 0, W, H), 1, u64::MAX).unwrap_or_else(|e| panic!("plan: {e}"))
}

/// The whole-canvas render of a clone of `doc` on `token`, run once.
fn whole(doc: &DrDocument) -> impl FnOnce(usize) -> (bool, String) + Send {
    let address = handle(doc) as usize;
    move |token: usize| {
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_doc_render_region(
                address as *const DrDocument,
                0,
                0,
                W,
                H,
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

/// Runs `render` uncancelled while a SPINNING poller waits for progress
/// strictly above each of `above` in turn; returns, per threshold, the time
/// from the render's start and the first value observed above it.
fn observed<F>(above: &[f64], render: F) -> Vec<(Duration, f64)>
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let control = unsafe { dr_job_control_create() };
    let token = control as usize;
    let out = thread::scope(|scope| {
        let start = Instant::now();
        let worker = scope.spawn(move || render(token));
        let deadline = start + Duration::from_secs(240);
        let mut seen = Vec::new();
        for &threshold in above {
            let value = loop {
                let value = progress(control);
                if value > threshold || worker.is_finished() {
                    break value;
                }
                assert!(Instant::now() < deadline, "the stage never started");
                std::hint::spin_loop();
            };
            assert!(value > threshold, "render completed before {threshold}");
            seen.push((start.elapsed(), value));
        }
        let (ok, message) = worker.join().unwrap();
        assert!(ok, "{message}");
        seen
    });
    unsafe { dr_job_control_free(control) };
    out
}

// ------------------------------------------------------------ fixtures --

fn base() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
    }))
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

/// (a) A Background and a SoftLight raster with an ENABLED canvas-sized mask
/// at `feather` (0: unfeathered, the calibration twin), built anew per call.
fn masked(feather: f32) -> DrDocument {
    let doc = with_raster(
        &base(),
        RgbaImage::from_fn(W, H, |x, y| Rgba([(x * 7) as u8, (y * 3) as u8, 160, 220])),
        (0, 0),
        BlendMode::SoftLight,
        0.9,
    );
    let plane = GrayImage::from_fn(W, H, |x, y| {
        Luma([if (x / 40 + y / 30) % 3 == 0 {
            0
        } else {
            ((x * 5 + y * 11) % 256) as u8
        }])
    });
    let mut out = doc;
    let mut properties = MaskProperties::at((0, 0));
    properties.feather = feather;
    let layer = &mut out.layers[1];
    layer.mask = Some(Arc::new(plane));
    layer.mask_enabled = true;
    layer.mask_properties = properties;
    out
}

/// (b) The 13A-4a guided root fixture: a Background and a Multiply raster
/// (stage 0), a radius-30 guide and a SoftLight raster (the final stage).
fn guided() -> DrDocument {
    let doc = with_raster(
        &base(),
        RgbaImage::from_fn(W - 40, H - 40, |x, y| {
            Rgba([
                (x * 3) as u8,
                90,
                (y * 5) as u8,
                if (x + y) % 7 == 0 { 0 } else { 200 },
            ])
        }),
        (20, 20),
        BlendMode::Multiply,
        0.9,
    );
    let mut doc = doc
        .adding_image_layer(
            doc.layers.len() - 1,
            RgbaImage::new(W, H),
            "shadows_highlights",
        )
        .expect("add layer");
    let top = doc.layers.len() - 1;
    doc.layers[top].meta = Some(adjust_meta(
        "shadows_highlights",
        "{\"shadows\":{\"amount\":0.7,\"tone\":0.9},\"highlights\":{\"amount\":0.35,\
         \"tone\":0.8},\"radius\":30}",
    ));
    doc.layers[top].opacity = 0.8;
    with_raster(
        &doc,
        RgbaImage::from_fn(W / 2, H / 2, |x, y| {
            Rgba([
                (x * 37 + y * 11) as u8,
                (200 - (x * 5) % 200) as u8,
                (y * 23 + 7) as u8,
                (40 + (x * 13 + y * 29) % 216) as u8,
            ])
        }),
        (100, 100),
        BlendMode::SoftLight,
        0.8,
    )
}

// ---------------------------------------------- the blur's own sizing --
//
// Written out independently of the core: a mask's sigma is 0.3·(feather −
// 1) + 0.8; the blur works at factor 1 up to sigma 3, then 2, 4, 8 at each
// doubling; its reach (the mask's pad) is the kernel's half width plus one,
// and above sigma 3 two more resampling pixels, scaled by the factor.

fn mask_sigma(feather: f32) -> f32 {
    0.3 * (feather - 1.0) + 0.8
}

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

fn pad(sigma: f32) -> u32 {
    let f = factor(sigma);
    if f == 1 {
        kernel_radius(sigma).ceil() as u32 + 1
    } else {
        let f32_f = f as f32;
        let reduced = (sigma * sigma - f32_f * f32_f / 3.0).max(0.0).sqrt() / f32_f;
        f * (kernel_radius(reduced).ceil() as u32 + 3)
    }
}

/// The mask's padded plane `(pw, ph)` at `feather`.
fn padded(feather: f32) -> (u32, u32) {
    let p = pad(mask_sigma(feather));
    (W + 2 * p, H + 2 * p)
}

fn window_of(v: &Value) -> Window {
    let n = |k: &str| v[k].as_u64().unwrap() as u32;
    (n("x"), n("y"), n("width"), n("height"))
}

fn area((_, _, w, h): Window) -> f64 {
    f64::from(w) * f64::from(h)
}

/// The guide's work over `plane` (the shared typed planner).
fn guide_work(plane: Window) -> f64 {
    let meta = json!({"type":"adjust","op":"shadows_highlights","evaluation":"precision_v1",
        "params":{"shadows":{"amount":0.7},"radius":30.0}})
    .to_string();
    PrecisionAdjustment::guide_plan_for_description(&meta, plane.2, plane.3)
        .unwrap()
        .1 as f64
}

/// (a)'s progress figures (13A-5b-iii-b-2): the fill's close `b0 =
/// pixels / work`, the mask blur's advance `b1 = 1 − 2·pixels / work` (the
/// loop's start: two rows over the canvas), the plan's `work`.
fn mask_points(feather: f32) -> (f64, f64, f64) {
    let work = plan(&masked(feather))["work_units"].as_u64().unwrap() as f64;
    let pixels = f64::from(W) * f64::from(H);
    (pixels / work, 1.0 - 2.0 * pixels / work, work)
}

/// (b)'s progress figures (13A-5b-iii-a): stage 0's close
/// `(fill + a0) / work` = `3·|R_0| / work` — the stage-0 `prev` fill
/// (`fill = |R_0|`, ONE plane: `guided()` has no style, so no D5) then
/// stage 0's two rows over `R_0` (`a0`) — its previous row report's
/// midpoint, the guide's advance `(fill + a0 + g0) / work` (its range's
/// end), and `|R_0|`.
fn guide_points() -> (f64, f64, f64, f64) {
    let p = plan(&guided());
    assert_eq!(p["stages"].as_array().unwrap().len(), 1);
    let work = p["work_units"].as_u64().unwrap() as f64;
    let r0 = window_of(&p["stages"][0]["plane"]);
    let fill = area(r0);
    let a0 = 2.0 * area(r0);
    let g0 = guide_work(r0);
    assert!(g0 / work > 1e-5, "the guide's report is resolvable");
    let w0 = f64::from(r0.2);
    (
        (fill + a0) / work,
        (fill + a0 - w0) / work,
        (fill + a0 + g0) / work,
        area(r0),
    )
}

// ---------------------------------------------------------- exactness --

#[test]
fn the_polled_mask_blur_matches_the_full_frame_at_both_feathers() {
    for feather in [6.0, 40.0] {
        let reference = full(&masked(feather));
        for window in [(0, 0, W, H), (1234, 777, 301, 203), (W - 17, 0, 17, 9)] {
            // A fresh document per render: a cold cache runs the polled blur.
            let got = polled_region(&masked(feather), window);
            assert!(
                got == crop(&reference, W, window),
                "feather {feather}: window {window:?}"
            );
        }
    }
}

#[test]
fn a_pre_cancelled_control_returns_at_once_with_no_output() {
    for doc in [masked(6.0), masked(40.0), guided()] {
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
}

/// The blur window of the feathered render: `(T_u, T_b)`, the times of the
/// unfeathered and feathered uncancelled runs' first reports above their own
/// loop start (`loop_start`, 13A-5b-iii-b-2; fresh fixtures).
fn blur_window(feather: f32) -> (Duration, Duration) {
    let lu = loop_start(&plan(&masked(0.0)));
    let unfeathered = observed(&[lu + 1e-6], whole(&masked(0.0)))[0].0;
    let lf = loop_start(&plan(&masked(feather)));
    let feathered = observed(&[lf + 1e-6], whole(&masked(feather)))[0].0;
    (unfeathered, feathered)
}

#[test]
fn a_cancel_inside_the_mask_blur_caches_no_partial_plane() {
    for feather in [6.0, 40.0] {
        // Document A: cancelled PARKED at the first checkpoint after the
        // blur's first report (the flip into stage 6, `k` = 0; the entry
        // count `n0` from a fresh twin's own report log, 13A-5b-iii-b-3),
        // then rendered again uncancelled — equal to an independently built
        // B's full render.
        let a = masked(feather);
        let (_, b1, _) = mask_points(feather);
        let n0 = entry_count(&at_the_blur, whole(&masked(feather)));
        let p = cancelled_parked_at(n0 + 1, &inside_the_blur(b1), whole(&a));
        let got = polled_region(&a, (0, 0, W, H));
        assert!(
            got == full(&masked(feather)),
            "feather {feather}: a cancelled blur left a partial plane in the cache"
        );
        eprintln!(
            "13A-5b-iii-b-3 cache integrity, feather {feather}: at the blur's first report, {}; \
             the re-render equals the reference",
            parked_line(&p),
        );
    }
}

// --------------------------------------------------- reports-once guards --
//
// A leaked blur report is caught by a REPORT LOG, never by one short-lived
// value: `logged` records every DISTINCT progress value with the time it was
// seen, and two signatures are read off the whole log.
//
// - INTERVAL: no logged value strictly between the pre-stage value and the
//   stage's first legitimate report (a leaked `feather` report lands under
//   the root range between them, and `fetch_max` keeps it until overtaken).
// - PIN: a leaked blur's last report is `1.0`, which pins progress for the
//   rest of the render — at least the loop's duration; a legitimate render
//   reaches 1.0 only at its final `report(1.0, 5)` and returns within a
//   scheduling gap. `t1` (when 1.0 was first logged) against `tf` (the
//   WORKER's own return stamp): `tf − t1 ≤ 0.5·t1`.
//
// Neither depends on the logger catching a value before it is overwritten:
// under load the logger can only MISS a leak, never report one falsely. A
// logger stall can only DELAY `t1`, shrinking `tf − t1`; `tf` is stamped by
// the worker itself, so a stall can never inflate it.
//
// Since 13A-5b-iii-a the byte guide reports INSIDE its range, so its guard is
// a RAMP (`assert_ramp`): from stage 0's close to the guide's advance the
// logged values are non-decreasing, stay inside the range and at least one
// lies strictly inside it — the check a guide polling under
// `checkpoints_only` (whose only value in the range is the advance) fails.
// `assert_ramp`'s strictly-inside clause is the one signature a logger stall
// can fail falsely (it must catch one of the first blur's ~2·gh row values),
// so since 13A-5b-iii-b-1 the RAMP test is `#[ignore]`d and runs only through
// `make cancel-bench`. The PIN is a ratio of two wall-clock readings, so
// it too runs only in the `#[ignore]`d tests (the mask blur's in
// `the_mask_blur_reports_inside_its_range`); the mask blur's INTERVAL test
// stays in the default `make test`. The log helpers are `common::probe`'s.

#[test]
fn the_mask_blur_leaks_no_report_into_the_root_range() {
    // 13A-5b-iii-b-2: the pre-stage value is the blur's advance `b1` (the
    // loop's start); the first legitimate report is the first row's, `b1 +
    // 2·W / work` (`done += rows·ow`).
    for feather in [6.0, 40.0] {
        let (_, b1, work) = mask_points(feather);
        let (log, _) = logged(whole(&masked(feather)));
        let what = format!("feather {feather}");
        assert_interval(&log, b1, b1 + 2.0 * f64::from(W) / work, &what);
    }
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-b-2)"]
fn the_mask_blur_reports_inside_its_range() {
    // 13A-5b-iii-b-2: from the fill's close `b0` to the blur's advance `b1`.
    for feather in [6.0, 40.0] {
        let (b0, b1, _) = mask_points(feather);
        let (log, tf) = logged(whole(&masked(feather)));
        let what = format!("the mask blur, feather {feather}");
        assert_ramp(&log, b0, b1, &what);
        assert_pin(&log, tf, &what);
    }
}

/// `nearest_odd`, written out independently: the odd integer nearest `x`
/// (at least 1), a tie going down.
fn nearest_odd(x: f32) -> u64 {
    let n = x.round().max(1.0) as u64;
    if n % 2 == 1 {
        n
    } else if x - (n - 1) as f32 <= (n + 1) as f32 - x {
        n - 1
    } else {
        n + 1
    }
}

#[test]
fn the_plan_counts_the_mask_blur() {
    // 13A-5b-iii-b-2: the plan's work less the fill and the two rows is the
    // mask blur's, restated here — the padded plane rounded up to the factor
    // `we·he`, 32 per pixel, plus two passes of `taps` over the reduced plane
    // (outside coverage 0: no complement passes).
    for feather in [6.0, 40.0] {
        let work = plan(&masked(feather))["work_units"].as_u64().unwrap();
        let pixels = u64::from(W) * u64::from(H);
        let sigma = mask_sigma(feather);
        let f = factor(sigma);
        let (pw, ph) = padded(feather);
        let (we, he) = (pw.div_ceil(f) * f, ph.div_ceil(f) * f);
        let actual = if f == 1 {
            sigma
        } else {
            let ff = f as f32;
            (sigma * sigma - ff * ff / 3.0).max(0.0).sqrt() / ff
        };
        let taps = nearest_odd(kernel_radius(actual) * 2.0 + 1.0).max(3);
        let extent = u64::from(we) * u64::from(he);
        let reduced = extent / u64::from(f * f);
        assert_eq!(
            work - 3 * pixels,
            extent * 32 + reduced * 2 * taps,
            "feather {feather}: factor {f}, plane {pw}×{ph}, taps {taps}"
        );
    }
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-b-1)"]
fn the_byte_guide_blur_reports_inside_its_range() {
    // The range: stage 0's close to the guide's advance (13A-5b-iii-a).
    let (close, _, guide, _) = guide_points();
    let (log, tf) = logged(whole(&guided()));
    assert_ramp(&log, close, guide, "the byte guide");
    assert_pin(&log, tf, "the byte guide");
}

/// (b)'s two grid-blur sub-ranges (13A-5b-iii-c-1): `(b1, b2, b3)` =
/// `close + (guide − close)·{(p + c)/t, (p + c + 2·c·taps)/t, (t − c)/t}`,
/// the plan's terms restated here from `R_0`'s dimensions at radius 30 — `p`
/// its pixels, `c` its ⌈w/f⌉·⌈h/f⌉ cells, `taps` the grid blur's kernel —
/// and `t = p + c·(2 + 4·taps)` asserted equal to the planner's guide work.
fn guide_sub_ranges() -> (f64, f64, f64) {
    let (close, _, guide, _) = guide_points();
    let r0 = window_of(&plan(&guided())["stages"][0]["plane"]);
    let sigma = 30.0f32 / 2.0;
    let f = factor(sigma);
    let pixels = u64::from(r0.2) * u64::from(r0.3);
    let cells = u64::from(r0.2.div_ceil(f)) * u64::from(r0.3.div_ceil(f));
    let grid = if f == 1 {
        sigma
    } else {
        let ff = f as f32;
        (sigma * sigma - ff * ff / 3.0).max(0.0).sqrt() / ff
    };
    let taps = if grid <= 0.0 {
        0
    } else {
        nearest_odd(kernel_radius(grid) * 2.0 + 1.0).max(3)
    };
    let t = pixels + cells * (2 + 4 * taps);
    assert_eq!(
        t as f64,
        guide_work(r0),
        "the guide's work is p + c·(2 + 4·taps)"
    );
    let at = |n: u64| close + (guide - close) * n as f64 / t as f64;
    (
        at(pixels + cells),
        at(pixels + cells + 2 * cells * taps),
        at(t - cells),
    )
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-c-1)"]
fn the_byte_guide_blurs_each_report_inside_their_sub_ranges() {
    // 13A-5b-iii-c-1: the first blur ramps `b1 → b2`, the second `b2 → b3`.
    let (b1, b2, b3) = guide_sub_ranges();
    let (log, tf) = logged(whole(&guided()));
    assert_ramp(&log, b1, b2, "the byte guide's first blur");
    assert_ramp(&log, b2, b3, "the byte guide's second blur");
    assert_pin(&log, tf, "the byte guide");
}

// ------------------------------------------------ measured cancellation --
//
// DECLARED (before measuring), the 13A-5a model: a cancel issued in a stage
// returns within T_f + W·c (T_f = 1 ms; W and c per stage above). Printed:
// the latency, c, the bound, max(0, latency − T_f)/c ("units past the
// floor") and PASS/MISS. Misses are recorded, never widened. A wall-clock
// verdict: `#[ignore]`d, run through `make cancel-bench` (13A-5b-iii-b-1).

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-b-1)"]
fn a_concurrently_cancelled_preparation_blur_returns_within_the_model() {
    // The mask blur at factor 1 (feather 6): W = 4096 cells of a `feather`
    // pass; c = t_blur over its two passes' cells. The cancel fires half a
    // pass's 4096-cell checkpoints after the blur's first report.
    let (unfeathered, feathered) = blur_window(6.0);
    let t_blur = feathered.saturating_sub(unfeathered);
    let (pw, ph) = padded(6.0);
    let cells = 2.0 * f64::from(pw) * f64::from(ph);
    let c = t_blur.as_secs_f64() / cells;
    let k = (u64::from(pw) * u64::from(ph)).div_ceil(4096) / 2;
    let (_, b1, _) = mask_points(6.0);
    let p = cancelled_after_checkpoints(&at_the_blur, k, &inside_the_blur(b1), whole(&masked(6.0)));
    let bound = RETURN_FLOOR_S + 4096.0 * c;
    eprintln!(
        "13A-5a 8-bit cancel during the mask feather blur (factor 1, feather 6): {:.3} ms \
         cancel-to-return (issued at {:.1} ms, blur ≈ {:.1}–{:.1} ms; {}); c = {:.3} ns per \
         cell (t_blur {:.1} ms over {cells} cells, 2·{pw}·{ph}); bound T_f + W·c = {:.3} ms \
         (T_f 1 ms, W 4096 cells); ≈{:.0} cells past the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        unfeathered.as_secs_f64() * 1e3,
        feathered.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        c * 1e9,
        t_blur.as_secs_f64() * 1e3,
        bound * 1e3,
        past(p.latency, c),
        verdict(p.latency, bound),
    );

    // The mask blur at factor 8 (feather 40): W = ONE Triangle resample of
    // the extended plane, its cost the worker's OWN (13A-5b-iii-a): recorded
    // BEFORE the probe, the checkpoint timeline of an uncancelled run from the
    // blur's first report to the first root report; the freeze is
    // `min(5 ms, longest gap / 2)` and `c` the timeline's gap at the count
    // the cancel was issued at (the unit it froze in, under the worker's
    // conditions). The longest gap's count is printed beside `issued` as
    // identification (expected equal: the up resample), never asserted.
    let (unfeathered, feathered) = blur_window(40.0);
    let t_blur = feathered.saturating_sub(unfeathered);
    let (_, b1, _) = mask_points(40.0);
    let t = timeline(&at_the_blur, &|p, _, _| p > b1 + 1e-6, whole(&masked(40.0)));
    let (longest_at, longest) = t.longest().expect("a gap");
    let freeze = FREEZE.min(longest / 2);
    let p = cancelled_in_freeze(
        &at_the_blur,
        freeze,
        &inside_the_blur(b1),
        whole(&masked(40.0)),
    );
    let c = t
        .gap_at(p.issued)
        .expect("issued inside the timeline")
        .as_secs_f64();
    let bound = RETURN_FLOOR_S + c;
    eprintln!(
        "13A-5a 8-bit cancel during the mask feather blur (factor 8, feather 40): {:.3} ms \
         cancel-to-return (issued at {:.1} ms, blur ≈ {:.1}–{:.1} ms, t_blur {:.1} ms; {}); c = \
         the timeline's gap at the issued count {} = {:.3} ms (longest gap {:.3} ms at count \
         {longest_at}; freeze {:.3} ms); bound T_f + W·c = {:.3} ms (T_f 1 ms, W 1 resample); \
         ≈{:.2} resamples past the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        unfeathered.as_secs_f64() * 1e3,
        feathered.as_secs_f64() * 1e3,
        t_blur.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        p.issued,
        c * 1e3,
        longest.as_secs_f64() * 1e3,
        freeze.as_secs_f64() * 1e3,
        bound * 1e3,
        past(p.latency, c),
        verdict(p.latency, bound),
    );

    // The byte guide build (b), its grid pass: W = 4096 cells (`every(base +
    // x)`, 4096 R_0 samples); c = the window from stage 0's last report to the
    // guide blur's first report (13A-5b-iii-a: the grid pass reports
    // nothing, the blur ramps inside the range) over |R_0|. The entry is
    // stage 0's close; the cancel fires half the grid pass's 4096-cell
    // checkpoints on, so it lands in the grid pass.
    let (close, before_close, guide, r0) = guide_points();
    let seen = observed(&[before_close, close], whole(&guided()));
    let window = seen[1].0.saturating_sub(seen[0].0);
    let c = window.as_secs_f64() / r0;
    // In the grid pass, progress sits at stage 0's close (its last row
    // report), strictly below the guide's advance.
    assert!(close + 1e-6 < guide);
    let k = (r0 as u64).div_ceil(4096) / 2;
    let p = cancelled_after_checkpoints(
        &|progress, _, _| progress >= close - 1e-6,
        k,
        &|progress, _| (progress - close).abs() <= 1e-6,
        whole(&guided()),
    );
    let bound = RETURN_FLOOR_S + 4096.0 * c;
    eprintln!(
        "13A-5a 8-bit cancel during the byte guide build's grid pass (radius 30): {:.3} ms \
         cancel-to-return (issued at {:.1} ms after stage 0's close {close}, the guide's \
         advance {guide}; {}); c = {:.3} ns per R_0 sample (the grid pass: window {:.1} ms \
         over {r0} samples); bound T_f + W·c = {:.3} ms (T_f 1 ms, W 4096 cells); ≈{:.0} cells \
         past the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        c * 1e9,
        window.as_secs_f64() * 1e3,
        bound * 1e3,
        past(p.latency, c),
        verdict(p.latency, bound),
    );
}
