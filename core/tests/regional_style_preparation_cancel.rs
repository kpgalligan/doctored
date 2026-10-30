//! Phase 13A-5b-i: cancellation checkpoints inside the LAYER-STYLE PLANE
//! RENDER the regional render runs before its loops — byte
//! `LayerStyle::rendered_with_job` (through `Styled::of`) and the typed
//! `prepare_style`'s `render_effects_with_job`, both under
//! `JobContext::checkpoints_only` until they reported inside their ranges (8
//! bits since 13A-5b-iii-b-2, u16 since 13A-5b-iii-c-1) — measured under the
//! 13A-5a CANCELLATION MODEL (`regional`'s module doc): a cancel issued in
//! stage `s` returns within `T_f + W_s · c_s`, `T_f` = 1 ms the return
//! floor.
//!
//! Intervals (DECLARED before measuring): inside the satin's matte, the
//! box-pair law at every size since #2 part 3 (`style_fx_satin`; the
//! Gaussian it replaced ran a Triangle resample at factor 8 for Size 40),
//! 4096 cells of each pass's horizontal sums and ONE ROW (`pw` cells, `pw ≤
//! 4096`) of its column emit, the band at `n = 0` polling every 4096 cells
//! and inside its distance transform, and its whole-plane passes (the
//! division by `width²`, the byte rounding) one unit each, bounded by
//! `c_map`; inside a dilate/erode, the distance
//! transform's own polling and every 4096 cells of the mapping; ONE ROW (`pw`
//! cells) inside each bespoke per-pixel loop (the bevel's height-field adds,
//! `shade` and coverage maps, the gradient overlay's loop, the stroke's
//! gradient fill, satin's soft XOR); since 13A-5b-ii-b, ONE ROW of the
//! padded plane (`pw` cells) inside `Shape::of_layer`, bounded by `c_map`
//! (one row costs less than the one-pass `c_map`); every other primitive is
//! ONE unit polled at its boundaries — `Shape::inverted`, the coverage
//! `clone` / `to_vec`, the all-zero scans, `shift_plane`, `multiply_planes`,
//! `subtract_planes`, `invert_plane`, `push_banded`'s multiply — each one
//! memory-bound pass over the plane, bounded by `c_map`, this test's OWN
//! timing of one `multiply_planes`-shaped pass over two `pw·ph` buffers.
//!
//! Calibration: `T_0` / `T_S` are the first reports above the pre-stage value
//! in the size-0 / size-S uncancelled runs (fresh fixtures), `t_style = T_S −
//! T_0`. Bounds: every satin size, `c = t_style / (2·pw·ph)` and `T_f +
//! max(4096·c, c_map)`; the bevel (every row of
//! every polled sub-stage costs at most that sub-stage's total over `ph`
//! rows), `T_f + max(t_style / ph, c_map)`. Since 13A-5b-iii-b-2 the byte
//! plan counts the planes, which report INSIDE their range `[pixels / work,
//! s1]` (`s1 = 1 − 2·pixels / work`, the loop's start) at stage 6 while every
//! driver report during preparation carries stage 1, and `T_0` / `T_S` are
//! each run's first report above its own loop start (`loop_start`). The
//! TRIGGER (13A-5b-ii-b, the shared `common::probe`) is the job's checkpoint
//! count, never a time: at 8 bits the entry is the blur's flip (`at_the_blur`,
//! the first stage-6 report); every satin size and the bevel cancel `k`
//! checkpoints later, `k` half the checkpoints a fresh uncancelled run polls
//! from that entry to its first report above `s1` (no size has an unpolled
//! resample to freeze in since #2 part 3); the verdict is
//! `inside_the_blur(s1)`.
//! At u16, since 13A-5b-iii-c-1 the planes report INSIDE their range
//! `[pre, target]` (`pre = pixels / work`, the zero fill's close; `target =
//! 1 − 2·pixels / work`, the style's advance and the loop's start; per-effect
//! sub-ranges, the blur's last row reporting the range's end): the probe's
//! entry is the first value above `pre` (the blur's first row), its window's
//! exit the blur's last row (`p ≥ target − 1e-6`), `k` half that window at
//! every size, and the verdict `pre < p ≤ target`
//! (inclusive of the advance's own value: typed reports carry no stage flip).
//! The CALIBRATION exit is a different one: `T_0` / `T_S` are each run's
//! first report above its own `target + 1e-6`, the loop's first chunk
//! (`target + 2·CHUNK / work`), mirroring the byte `loop_start`, so `T_S −
//! T_0` covers the whole plane render (blur, post-blur passes, advance) at
//! both sizes; `target − 1e-6` is the probe's window exit only. Misses are
//! recorded, never widened. LOAD ROBUSTNESS (13A-5b-iii-b-1): those probes
//! serve only the measured PASS/MISS test, `#[ignore]`d and run through
//! `make cancel-bench`; the default `make test` keeps the exactness, the
//! pre-cancelled control (its return time printed, not bounded), the
//! INTERVAL guards (their PIN twins `#[ignore]`d) and the byte
//! cache-integrity test, which cancels at
//! the blur's flip (`k` = 0) at every size — since 13A-5b-iii-b-3 with the
//! worker PARKED at `n0 + 1`, the first checkpoint after the flip, `n0` read
//! from a fresh twin's own report log (`entry_count`, `cancelled_parked_at`
//! under a `Released` guard), so no poller can stall past the stage or
//! return before the entry; the byte planes' RAMP guard (13A-5b-iii-b-2) and
//! the typed one (13A-5b-iii-c-1) are bench-only, the typed INTERVAL runs
//! from `target` to the loop's first chunk.
//!
//! The oracles for exactness are `dr_doc_flattened` (8 bits) and
//! `dr_precision_doc_render` (u16), cropped here in plain index arithmetic.
//! The byte plane cache is an `Arc<LayerStyle>` every clone shares, so every
//! byte run that must render planes builds its fixture anew.

use std::ffi::c_char;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use serde_json::Value;

use crate::common;
use common::probe::*;
use common::{img_pixels, styled, take_err_string};

use crate::region_plan;

type Window = (u32, u32, u32, u32);

const W: u32 = 3000;
const H: u32 = 2000;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The two satin sizes: the law's box pair at width 4 and 40 (#2 part 3;
/// the Gaussian before ran factor 1 and factor 8 at these sizes).
const SIZES: [f32; 2] = [4.0, 40.0];
const WINDOWS: [Window; 3] = [(0, 0, W, H), (1234, 777, 301, 203), (W - 17, 0, 17, 9)];

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

/// The independent oracle at divisor 1: the window's rows of the projection
/// (`channels` samples per pixel).
fn crop<T: Copy>(full: &[T], channels: u32, (x, y, w, h): Window) -> Vec<T> {
    let mut out = Vec::new();
    for j in y..y + h {
        let at = ((j * W + x) * channels) as usize;
        out.extend_from_slice(&full[at..at + (w * channels) as usize]);
    }
    out
}

/// The byte regional render at divisor 1 under a LIVE job control (the
/// polled path), never cancelled.
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

/// The whole-canvas byte render of a clone of `doc` on `token`, run once.
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

/// A u16 image's samples, freeing it.
fn samples16(image: *mut PrecisionImage) -> Vec<u16> {
    assert!(!image.is_null());
    let (w, h) = unsafe {
        (
            dr_precision_image_width(image),
            dr_precision_image_height(image),
        )
    };
    let p = unsafe { dr_precision_image_pixels_rgba16(image) };
    assert!(!p.is_null(), "u16 samples");
    let out = unsafe { std::slice::from_raw_parts(p, (w * h * 4) as usize) }.to_vec();
    unsafe { dr_precision_image_free(image) };
    out
}

fn full16(doc: &PrecisionDocument) -> Vec<u16> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "render: {}", take_err_string(err));
    samples16(image)
}

/// The u16 regional render at divisor 1 under a LIVE job control.
fn polled_region16(doc: &PrecisionDocument, (x, y, w, h): Window) -> Vec<u16> {
    let control = unsafe { dr_job_control_create() };
    let mut err: *mut c_char = ptr::null_mut();
    let image =
        unsafe { dr_precision_doc_render_region(doc, x, y, w, h, 1, LIMIT, 0, control, &mut err) };
    unsafe { dr_job_control_free(control) };
    assert!(!image.is_null(), "region: {}", take_err_string(err));
    samples16(image)
}

fn plan16(doc: &PrecisionDocument) -> Value {
    region_plan::typed(doc, (0, 0, W, H), 1).unwrap_or_else(|e| panic!("plan: {e}"))
}

/// The whole-canvas u16 render of `doc` on `token`, run once.
fn whole16(doc: PrecisionDocument) -> impl FnOnce(usize) -> (bool, String) + Send {
    move |token: usize| {
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_render_region(
                &doc,
                0,
                0,
                W,
                H,
                1,
                LIMIT,
                0,
                token as *const DrJobControl,
                &mut err,
            )
        };
        let ok = !out.is_null();
        if ok {
            unsafe { dr_precision_image_free(out) };
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

/// A satin of `size` at distance 1: since #2 part 3 the box-pair law's
/// style plane, whose rows report at stage 6 as the Gaussian's did. The
/// fixture carried a drop shadow at distance 0 until the shadows' law
/// (#2).
fn satin(size: f32) -> String {
    format!("{{\"effects\":[{{\"type\":\"satin\",\"distance\":1,\"size\":{size}}}]}}")
}

fn bevel(size: f32) -> String {
    format!(
        "{{\"effects\":[{{\"type\":\"bevel_emboss\",\"style\":\"inner_bevel\",\
         \"size\":{size}}}]}}"
    )
}

/// Alpha 0 inside a grid of discs, an anti-aliased 4 px ramp, 255 between.
fn hole_alpha(x: u32, y: u32) -> f32 {
    let (dx, dy) = ((x % 97) as f32 - 48.0, (y % 83) as f32 - 41.0);
    ((dx * dx + dy * dy).sqrt() - 18.0) / 4.0
}

/// (c) / (e) at 8 bits: a Background and a canvas-sized Normal raster with
/// anti-aliased holes carrying `style`, built anew per call (a cold cache).
fn byte_doc(style: &str) -> DrDocument {
    let doc = DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
    }));
    let mut doc = doc
        .adding_image_layer(
            0,
            RgbaImage::from_fn(W, H, |x, y| {
                let a = (hole_alpha(x, y) * 255.0).clamp(0.0, 255.0) as u8;
                Rgba([(x * 7) as u8, (y * 3) as u8, 160, a])
            }),
            "Styled",
        )
        .expect("add layer");
    doc.layers[1].blend = BlendMode::Normal;
    styled(&doc, 1, style)
}

fn image16(opaque: bool) -> PrecisionImage {
    let pixels = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            [
                ((x * 2711 + y * 911) % 65536) as u16,
                ((x * 523 + y * 3301) % 65536) as u16,
                ((x ^ y) * 1999 % 65536) as u16,
                if opaque {
                    65535
                } else {
                    (hole_alpha(x, y) * 65535.0).clamp(0.0, 65535.0) as u16
                },
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap()
}

/// (d) the same at u16.
fn typed_doc(style: &str) -> PrecisionDocument {
    let parsed = LayerStyle::from_json(style).unwrap();
    let layers = vec![
        PrecisionDocumentLayer::raster("Background", image16(true), (0, 0), 1.0, true).unwrap(),
        PrecisionDocumentLayer::raster("Styled", image16(false), (0, 0), 1.0, true)
            .unwrap()
            .with_layer_style(Some(&parsed), (W, H))
            .unwrap(),
    ];
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

// ---------------------------------------- the style plane's own sizing --
//
// Written out independently of the core: a satin runs the box-pair law at
// spread 0 since #2 part 3 (`style_fx_satin`), no Gaussian and no
// resample at any size; its matte's reach is the box width `w`, the size
// rounded half up (4 and 40 here), a satin's reach `w + ceil(distance)` (1
// here) and the plane pad the reach plus one, `w + 2`. The Smooth Inner
// Bevel's is its own (#2, `style_bevel_smooth`): no blur, the box pair and
// the central difference reach `W` (the size rounded half up) plus 1 for the
// coverage's edge, so its pad is `W + 2`, 6 px at size 4 (bench (e)).

/// The padded plane `(pw, ph)` of a satin of `size` at distance 1 (the inner
/// bevel's own is bench (e)'s).
fn plane(size: f32) -> (u32, u32) {
    let pad = (size + 0.5).floor() as u32 + 1 + 1;
    let (pw, ph) = (W + 2 * pad, H + 2 * pad);
    assert!(
        pw <= 4096,
        "a box-pair row never exceeds one 4096-cell chunk"
    );
    (pw, ph)
}

// ---------------------------------------------------------- exactness --

#[test]
fn the_polled_style_planes_match_the_full_frame_at_8_bits() {
    for size in SIZES {
        let reference = full(&byte_doc(&satin(size)));
        for window in WINDOWS {
            // A fresh document per render: a cold cache runs the polled render.
            let got = polled_region(&byte_doc(&satin(size)), window);
            assert!(
                got == crop(&reference, 4, window),
                "satin {size}: window {window:?}"
            );
        }
    }
    let reference = full(&byte_doc(&bevel(4.0)));
    for window in &WINDOWS[..2] {
        let got = polled_region(&byte_doc(&bevel(4.0)), *window);
        assert!(
            got == crop(&reference, 4, *window),
            "inner bevel 4: window {window:?}"
        );
    }
}

#[test]
fn the_polled_style_planes_match_the_full_frame_at_u16() {
    for size in SIZES {
        let doc = typed_doc(&satin(size));
        let reference = full16(&doc);
        for window in WINDOWS {
            let got = polled_region16(&doc, window);
            assert!(
                got == crop(&reference, 4, window),
                "u16 satin {size}: window {window:?}"
            );
        }
    }
}

#[test]
fn a_pre_cancelled_control_returns_at_once_with_no_output() {
    let check = |render: Box<dyn FnOnce(usize) -> (bool, String) + Send>, what: &str| {
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let started = Instant::now();
        let (ok, message) = render(control as usize);
        let took = started.elapsed();
        assert!(!ok, "{what}: no image");
        assert_eq!(message, "operation cancelled", "{what}");
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        eprintln!(
            "{what}: 13A-5b-iii-b-1 pre-cancelled control returned in {:.3} ms (no bound asserted)",
            took.as_secs_f64() * 1e3
        );
        unsafe { dr_job_control_free(control) };
    };
    for size in SIZES {
        check(
            Box::new(whole(&byte_doc(&satin(size)))),
            &format!("8-bit satin {size}"),
        );
        check(
            Box::new(whole16(typed_doc(&satin(size)))),
            &format!("u16 satin {size}"),
        );
    }
    check(
        Box::new(whole(&byte_doc(&bevel(4.0)))),
        "8-bit inner bevel 4",
    );
}

// --------------------------------------------------- the stage's window --

/// The byte plan of `doc` over the whole canvas.
fn plan(doc: &DrDocument) -> Value {
    region_plan::byte(doc, (0, 0, W, H), 1, u64::MAX).unwrap_or_else(|e| panic!("plan: {e}"))
}

/// The byte progress figures of `style` (13A-5b-iii-b-2): the fill's close
/// `pixels / work`, the planes' advance `s1 = 1 − 2·pixels / work` (the
/// loop's start: two rows over the canvas) and the plan's `work`.
fn byte_points(style: &str) -> (f64, f64, f64) {
    let work = plan(&byte_doc(style))["work_units"].as_u64().unwrap() as f64;
    let pixels = f64::from(W) * f64::from(H);
    (pixels / work, 1.0 - 2.0 * pixels / work, work)
}

/// The byte plane window of `style`'s render beside its `twin` (size 0):
/// `(T_0, T_S)`, the first reports above each run's own loop start
/// (`loop_start`, 13A-5b-iii-b-2) of the uncancelled runs (fresh fixtures).
fn byte_window(twin: &str, style: &str) -> (Duration, Duration) {
    let l0 = loop_start(&plan(&byte_doc(twin)));
    let t0 = observed(&[l0 + 1e-6], whole(&byte_doc(twin)))[0].0;
    let ls = loop_start(&plan(&byte_doc(style)));
    let ts = observed(&[ls + 1e-6], whole(&byte_doc(style)))[0].0;
    (t0, ts)
}

/// The typed sample loop's chunk: `regional_plan::CHECKPOINT_PIXELS`, the
/// plan's `cancellation_checkpoint_pixels` (asserted in [`typed_points`]).
const CHUNK: f64 = 4096.0;

/// The u16 progress figures of `doc`'s plan: the pre-stage value `pre` (the
/// zero fill's close, `pixels / work`), the style's `advance` report `target`
/// (`1 − 2·pixels / work`, the loop's start: two rows over the canvas) and
/// the plan's `work`. Since 13A-5b-iii-c-1 the planes report INSIDE
/// `[pre, target]`.
fn typed_points(doc: &PrecisionDocument) -> (f64, f64, f64) {
    let plan = plan16(doc);
    assert_eq!(
        plan["cancellation_checkpoint_pixels"].as_f64(),
        Some(CHUNK),
        "the loop's chunk"
    );
    let work = plan["work_units"].as_u64().unwrap() as f64;
    let pixels = f64::from(W) * f64::from(H);
    assert!(work >= 3.0 * pixels, "the plan's work covers the structure");
    (pixels / work, 1.0 - 2.0 * pixels / work, work)
}

/// The u16 plane window: `(T_0, T_S)`, the first reports above each run's
/// own `target + 1e-6` — the loop's first chunk (`target + 2·CHUNK / work`),
/// as the byte `byte_window` times to `loop_start`, so `T_S − T_0` covers the
/// whole plane render (blur, post-blur passes, advance) at both sizes
/// (13A-5b-iii-c-1).
fn typed_window(size: f32) -> (Duration, Duration) {
    let twin = typed_doc(&satin(0.0));
    let (_, target0, _) = typed_points(&twin);
    let t0 = observed(&[target0 + 1e-6], whole16(twin))[0].0;
    let doc = typed_doc(&satin(size));
    let (_, target, _) = typed_points(&doc);
    let ts = observed(&[target + 1e-6], whole16(doc))[0].0;
    (t0, ts)
}

/// The byte checkpoint-trigger probe on a clone of `doc` (module doc): the
/// blur's flip, then `k` checkpoints — half a fresh uncancelled run's window
/// from that flip to its first report above the planes' advance `s1`.
fn byte_probe(doc: &DrDocument, style: &str) -> Probe {
    let (_, s1, _) = byte_points(style);
    let ((_, n_entry), (_, n_exit)) = window(
        &at_the_blur,
        &|p, _, _| p > s1 + 1e-6,
        whole(&byte_doc(style)),
    );
    let k = n_exit.saturating_sub(n_entry) / 2;
    cancelled_after_checkpoints(&at_the_blur, k, &inside_the_blur(s1), whole(doc))
}

/// The u16 checkpoint-trigger probe (module doc, 13A-5b-iii-c-1): the first
/// value above the pre-stage value (the blur's first row), then half the
/// window to the blur's last row (`target − 1e-6`); the verdict is progress
/// inside the range, `pre < p ≤ target` (inclusive of the advance's own
/// value: typed reports carry no stage flip).
fn typed_probe(size: f32) -> Probe {
    let doc = typed_doc(&satin(size));
    let (pre, target, _) = typed_points(&doc);
    let entry = move |p: f64, _: u32, _: u64| p > pre + 1e-6;
    let inside = move |p: f64, _: u32| pre + 1e-6 < p && p <= target + 1e-6;
    let ((_, n_entry), (_, n_exit)) = window(
        &entry,
        &|p, _, _| p >= target - 1e-6,
        whole16(typed_doc(&satin(size))),
    );
    let k = n_exit.saturating_sub(n_entry) / 2;
    cancelled_after_checkpoints(&entry, k, &inside, whole16(doc))
}

#[test]
fn a_cancel_inside_the_plane_render_caches_no_partial_plane() {
    for size in SIZES {
        // Document A: cancelled PARKED at the first checkpoint after the
        // blur's flip (`k` = 0 at every size — no freeze; the entry count
        // `n0` from a fresh twin's own report log, 13A-5b-iii-b-3), then
        // rendered again uncancelled — equal to an independently built B's
        // full render.
        let a = byte_doc(&satin(size));
        let (_, s1, _) = byte_points(&satin(size));
        let n0 = entry_count(&at_the_blur, whole(&byte_doc(&satin(size))));
        let p = cancelled_parked_at(n0 + 1, &inside_the_blur(s1), whole(&a));
        let got = polled_region(&a, (0, 0, W, H));
        assert!(
            got == full(&byte_doc(&satin(size))),
            "satin {size}: a cancelled render left a partial plane in the cache"
        );
        eprintln!(
            "13A-5b-iii-b-3 cache integrity, satin {size}: at the blur's flip, {}; the \
             re-render equals the reference",
            parked_line(&p),
        );
    }
}

// --------------------------------------------------- reports-once guards --
//
// INTERVAL / PIN over `common::probe`'s REPORT LOG (the local copies folded in
// 13A-5b-iii-b-1): a stall can only make them MISS a leak, so the INTERVAL
// stays in the default `make test`; the PIN is a ratio of two wall-clock
// readings, so it runs in the `#[ignore]`d twins, through
// `make cancel-bench`.

/// The byte guards' styles: every satin size and an inner bevel.
fn byte_guard_styles() -> Vec<(String, String)> {
    SIZES
        .iter()
        .map(|&s| (format!("8-bit satin {s}"), satin(s)))
        .chain(std::iter::once(("8-bit inner bevel 4".into(), bevel(4.0))))
        .collect()
}

#[test]
fn the_byte_plane_render_leaks_no_report_into_the_root_range() {
    // 13A-5b-iii-b-2: the pre-stage value is the planes' advance `s1` (the
    // loop's start); the first legitimate report is the first row's, `s1 +
    // 2·W / work`.
    for (what, style) in byte_guard_styles() {
        let (_, s1, work) = byte_points(&style);
        let (log, _) = logged(whole(&byte_doc(&style)));
        assert_interval(&log, s1, s1 + 2.0 * f64::from(W) / work, &what);
    }
}

#[test]
fn a_soft_shadow_or_glow_plane_reports_its_box_pair_rows() {
    // #2: the soft drop shadow, inner shadow and outer glow run the
    // box-pair law, whose two passes report per row at stage 6 as the
    // Gaussian they replaced did; read on the worker's own report log.
    for kind in ["drop_shadow", "inner_shadow", "outer_glow"] {
        let style = format!("{{\"effects\":[{{\"type\":\"{kind}\",\"size\":4}}]}}");
        let (start, s1, _) = byte_points(&style);
        let log = reports(whole(&byte_doc(&style)));
        let rows = log
            .iter()
            .filter(|r| r.stage == 6 && r.progress > start + 1e-6 && r.progress < s1 - 1e-6)
            .count();
        assert!(
            rows >= 2 * H as usize,
            "{kind}: {rows} stage-6 reports inside ({start}, {s1}); the box pair reports \
             each pass's rows"
        );
    }
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn the_byte_plane_render_is_not_pinned_at_its_end() {
    for (what, style) in byte_guard_styles() {
        let (log, tf) = logged(whole(&byte_doc(&style)));
        assert_pin(&log, tf, &what);
    }
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-b-2)"]
fn the_byte_plane_render_reports_inside_its_range() {
    // 13A-5b-iii-b-2: from the fill's close to the planes' advance `s1`.
    let (start, s1, _) = byte_points(&satin(4.0));
    let (log, tf) = logged(whole(&byte_doc(&satin(4.0))));
    assert_ramp(&log, start, s1, "8-bit satin 4");
    assert_pin(&log, tf, "8-bit satin 4");
}

#[test]
fn the_typed_plane_render_leaks_no_report_into_the_root_range() {
    // 13A-5b-iii-c-1: the pre-stage value is the style's advance `target`
    // (the loop's start); the first legitimate report is the loop's first
    // chunk, `target + 2·CHUNK / work` (`CHUNK·rows`, two layers).
    for size in SIZES {
        let doc = typed_doc(&satin(size));
        let (_, target, work) = typed_points(&doc);
        let (log, _) = logged(whole16(doc));
        let what = format!("u16 satin {size}");
        assert_interval(&log, target, target + 2.0 * CHUNK / work, &what);
    }
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn the_typed_plane_render_is_not_pinned_at_its_end() {
    for size in SIZES {
        let (log, tf) = logged(whole16(typed_doc(&satin(size))));
        assert_pin(&log, tf, &format!("u16 satin {size}"));
    }
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-c-1)"]
fn the_typed_plane_render_reports_inside_its_range() {
    // 13A-5b-iii-c-1: from the zero fill's close `pre` to the style's
    // advance `target`.
    let doc = typed_doc(&satin(4.0));
    let (pre, target, _) = typed_points(&doc);
    let (log, tf) = logged(whole16(doc));
    assert_ramp(&log, pre, target, "u16 satin 4");
    assert_pin(&log, tf, "u16 satin 4");
}

// ------------------------------------------------ measured cancellation --
//
// DECLARED (before measuring), the 13A-5a model with the module doc's
// intervals. Printed: the latency, the calibrated costs, the bound, the
// intervals past the floor and PASS/MISS. Misses are recorded, never
// widened. A wall-clock verdict: `#[ignore]`d, run through
// `make cancel-bench` (13A-5b-iii-b-1).

/// `c_map`: one `multiply_planes`-shaped pass, `a = (a·b + 127) / 255`, over
/// two `pw·ph` buffers, timed here.
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

fn report(
    what: &str,
    p: &Probe,
    (t0, ts): (Duration, Duration),
    costs: &str,
    unit: f64,
    unit_name: &str,
) {
    let bound = RETURN_FLOOR_S + unit;
    eprintln!(
        "13A-5b-i {what}: {:.3} ms cancel-to-return (issued at {:.1} ms, plane window ≈ \
         {:.1}–{:.1} ms, t_style {:.1} ms; {}); {costs}; bound T_f + {unit_name} = {:.3} ms (T_f \
         1 ms); ≈{:.2} intervals past the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        t0.as_secs_f64() * 1e3,
        ts.as_secs_f64() * 1e3,
        ts.saturating_sub(t0).as_secs_f64() * 1e3,
        checkpoints_line(p),
        bound * 1e3,
        past(p.latency, unit),
        verdict(p.latency, bound),
    );
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-b-1)"]
fn a_concurrently_cancelled_style_plane_render_returns_within_the_model() {
    for size in SIZES {
        let (pw, ph) = plane(size);
        let map = c_map((pw, ph));
        // #2 part 3: the box-pair law at every size, no resample.
        let interval = |t_style: f64| -> (f64, String, &'static str) {
            let c = t_style / (2.0 * f64::from(pw) * f64::from(ph));
            (
                (4096.0 * c).max(map),
                format!(
                    "c = {:.3} ns per cell (2·{pw}·{ph} cells), c_map {:.3} ms",
                    c * 1e9,
                    map * 1e3
                ),
                "max(4096·c, c_map)",
            )
        };

        // (c) 8 bits.
        let window = byte_window(&satin(0.0), &satin(size));
        let t_style = window.1.saturating_sub(window.0);
        let (unit, costs, name) = interval(t_style.as_secs_f64());
        let p = byte_probe(&byte_doc(&satin(size)), &satin(size));
        report(
            &format!("8-bit cancel during the satin plane render (size {size}, box pair)"),
            &p,
            window,
            &costs,
            unit,
            name,
        );

        // (d) u16.
        let window = typed_window(size);
        let t_style = window.1.saturating_sub(window.0);
        let (unit, costs, name) = interval(t_style.as_secs_f64());
        let p = typed_probe(size);
        report(
            &format!("u16 cancel during the satin plane render (size {size}, box pair)"),
            &p,
            window,
            &costs,
            unit,
            name,
        );
    }

    // (e) 8 bits, the inner bevel of size 4: every row of every polled
    // sub-stage costs at most that sub-stage's total over `ph` rows. Its
    // plane is padded by the bevel's own pad, `W + 2` = 6 at size 4 (the
    // sizing comment above).
    let pad = 4 + 2;
    let (pw, ph) = (W + 2 * pad, H + 2 * pad);
    let map = c_map((pw, ph));
    let window = byte_window(&bevel(0.0), &bevel(4.0));
    let t_style = window.1.saturating_sub(window.0);
    let per_row = t_style.as_secs_f64() / f64::from(ph);
    let p = byte_probe(&byte_doc(&bevel(4.0)), &bevel(4.0));
    report(
        "8-bit cancel during the inner-bevel plane render (size 4, factor 1)",
        &p,
        window,
        &format!(
            "t_style / ph = {:.3} ms per row ({ph} rows of {pw}), c_map {:.3} ms",
            per_row * 1e3,
            map * 1e3
        ),
        per_row.max(map),
        "max(t_style / ph, c_map)",
    );
}
