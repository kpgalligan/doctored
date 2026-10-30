//! Phase 13A-5b-iii-c-2: the TYPED styled composite inside a typed
//! MATERIALIZATION — `precision_render::render_styled` →
//! `precision_style_render::composite_styled_into`, reached from the
//! regional `prepare_style` through `render_layers` over a styled group's
//! children. Since 13A-5b-iii-c-2 (a) the composite's planes render under
//! `render_styled`'s `progress.job.range(completed/total, (completed +
//! work)/total)` (`work` the child's `style_scratch` charge, no
//! `checkpoints_only`), and (b) the seven typed call sites of the shared
//! primitives (`composite_contribution`, `clamp_alpha_to_shape`,
//! `BackdropSnapshot::of`, `composite_buffer_into`) poll the live job, one
//! ROW each, instead of `JobContext::NONE`.
//!
//! The fixture (`materialized16`): a 3000×2000 sRGB u16 canvas, an opaque
//! seeded Background, then an isolated Normal group carrying a Multiply
//! colour overlay (an enabled effect, so the group is MATERIALIZED) over ONE
//! canvas-sized Normal child with anti-aliased holes carrying a drop shadow
//! of size 4 at distance 0 and Spread 100 % (a dilation, no blur; reach 5),
//! then a satin of size 4 at distance 1 (the box-pair law since #578 part
//! 3, whose rows report at stage 6 as the Gaussian's did; reach 5, the
//! shadow's too, so the pad is 6). The soft drop shadow it carried before
//! runs the box-pair law since #578, and the Spread 100 % shadow keeps a
//! Below contribution in the composite. With `A = W·H`
//! and `work` the plan's `work_units`, the progress figures (`points`): the
//! acc fill closes at `A/work`, the group buffer's zero fill at `s =
//! 2A/work` — the child composite's range start; its planes ramp to `e =
//! (2A + S)/work` (`S = P·(2 + 8·2)`, `P = pw·ph` the child's padded plane,
//! written out below; the satin, rendered last, ramps the second half), the
//! Below contribution, the pixel pass and the Interior contribution report
//! nothing, the child's base advance re-reports `e` (`3A < 2A + S`, held
//! under `fetch_max`) and its style advance lands at `adv = (3A + S)/work`.
//!
//! DECLARED before measuring (the 13A-5a model, `regional`'s module doc): a
//! cancel issued inside the composite after its planes — the Below
//! contribution's H rows, then the pixel pass's H rows, then the Interior
//! contribution's H rows, each primitive
//! polling once per row — returns within `T_f + c_row`, `T_f` = 1 ms the
//! return floor and `c_row = t_comp / n_comp` the composite's mean
//! per-checkpoint cost over its own uncancelled run (time and checkpoints
//! from the planes' last report, `p ≥ e − 1e-6`, to the style advance,
//! `p > e + 1e-6`) — the "inside the style/knockout primitives" row form.
//! The probe's entry is that last planes report, `k` a quarter of `n_comp`
//! (inside the Below contribution's rows), the verdict progress within 1e-6
//! of `e`.
//!
//! LOAD ROBUSTNESS (13A-5b-iii-b-1): the default tests assert only
//! load-independent facts — exactness against `dr_precision_doc_render`, and
//! on the worker's OWN report log (`reports`, under `record_reports`) the
//! planes' ramp inside `(s, e)`, the style advance as the first report past
//! `e` and the composite's per-row checkpoints (at least 2·H between the
//! planes' last report and the base advance); the parked cancel
//! (`cancelled_parked_at`, `H/2` checkpoints past that last report) prints
//! its release-to-return time only. The RAMP guard over the spinning trace
//! and the PASS/MISS verdict are `#[ignore]`d and run through
//! `make cancel-bench`.

use std::ffi::c_char;
use std::ptr;

use doctored_core::ffi_jobs::*;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::{DrJobControl, Report};
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use serde_json::Value;

use crate::common;
use common::probe::*;
use common::take_err_string;

use crate::region_plan;

type Window = (u32, u32, u32, u32);

const W: u32 = 3000;
const H: u32 = 2000;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIZE: f32 = 4.0;
const WINDOWS: [Window; 4] = [
    (0, 0, W, H),
    (1234, 777, 301, 203),
    (W - 17, 0, 17, 9),
    (0, H - 1, 1, 1),
];

// ------------------------------------------------------------- plumbing --

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

/// The full-frame reference, with or without a live job control.
fn full16(doc: &PrecisionDocument, live: bool) -> Vec<u16> {
    let control = if live {
        unsafe { dr_job_control_create() }
    } else {
        ptr::null_mut()
    };
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, control, &mut err) };
    if live {
        unsafe { dr_job_control_free(control) };
    }
    assert!(!image.is_null(), "render: {}", take_err_string(err));
    samples16(image)
}

/// The independent oracle at divisor 1: the window's rows of the projection.
fn crop(full: &[u16], (x, y, w, h): Window) -> Vec<u16> {
    let mut out = Vec::new();
    for j in y..y + h {
        let at = ((j * W + x) * 4) as usize;
        out.extend_from_slice(&full[at..at + (w * 4) as usize]);
    }
    out
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

/// The whole-canvas u16 regional render of `doc` on `token`, run once.
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

// ------------------------------------------------------------ fixtures --

/// Alpha 0 inside a grid of discs, an anti-aliased 4 px ramp, 1 between.
fn hole_alpha(x: u32, y: u32) -> f32 {
    let (dx, dy) = ((x % 97) as f32 - 48.0, (y % 83) as f32 - 41.0);
    ((dx * dx + dy * dy).sqrt() - 18.0) / 4.0
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

fn style(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap()
}

/// The module doc's fixture: Background, then a colour-overlaid isolated
/// group over one child with a Spread 100 % drop shadow and a satin.
fn materialized16() -> PrecisionDocument {
    let child = Layer::raster("Styled", image16(false), (0, 0), 1.0, true)
        .unwrap()
        .with_layer_style(
            Some(&style(&format!(
                "{{\"effects\":[{{\"type\":\"drop_shadow\",\"distance\":0,\"size\":{SIZE},\
                 \"spread\":1}},{{\"type\":\"satin\",\"distance\":1,\"size\":{SIZE}}}]}}"
            ))),
            (W, H),
        )
        .unwrap();
    let group = Layer::isolated_normal("Group", vec![child], 1.0, true)
        .unwrap()
        .with_layer_style(
            Some(&style(
                "{\"effects\":[{\"type\":\"color_overlay\",\"blend\":\"multiply\",\
                 \"color\":\"#80c0ff\"}]}",
            )),
            (W, H),
        )
        .unwrap();
    let layers = vec![
        Layer::raster("Background", image16(true), (0, 0), 1.0, true).unwrap(),
        group,
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

// ---------------------------------------- the child plane's own sizing --
//
// Written out independently of the core (the style suite's rule): a satin
// runs the box-pair law at spread 0 since #578 part 3 (`style_fx_satin`),
// whose reach is its width `w`, the size rounded half up (4 here), plus
// `ceil(distance)` (1 here); the plane pad is the reach plus one. The
// Spread 100 % drop shadow's reach, `ceil(size · spread) + 1` = 5, equals
// the satin's 5, so the pad is 6.

/// The child's padded plane `(pw, ph)`.
fn plane() -> (u32, u32) {
    let pad = (SIZE + 0.5).floor() as u32 + 1 + 1;
    (W + 2 * pad, H + 2 * pad)
}

/// `(s, e, adv)` (module doc) over the plan's `work`.
fn points(doc: &PrecisionDocument) -> (f64, f64, f64) {
    let plan = plan16(doc);
    assert_eq!(
        plan["global_analysis"],
        serde_json::json!(["layer_style_planes", "group_materialization"]),
        "the child's planes and the group's materialization, in row order"
    );
    let work = plan["work_units"].as_u64().unwrap() as f64;
    let a = f64::from(W) * f64::from(H);
    let (pw, ph) = plane();
    let s = f64::from(pw) * f64::from(ph) * 18.0;
    assert!(3.0 * a < 2.0 * a + s, "the base advance holds at `e`");
    assert!(3.0 * a + s < work, "the plan's work covers the structure");
    (2.0 * a / work, (2.0 * a + s) / work, (3.0 * a + s) / work)
}

/// The index of the planes' last report (the first at or above `e − 1e-6`)
/// and of the style advance (the first above `e + 1e-6`) in `log`.
fn composite_span(log: &[Report], e: f64) -> (usize, usize) {
    let last = log
        .iter()
        .position(|r| r.progress >= e - 1e-6)
        .expect("the planes reach their range's end");
    let adv = log
        .iter()
        .position(|r| r.progress > e + 1e-6)
        .expect("a report after the child's composite");
    assert!(last < adv);
    (last, adv)
}

// ---------------------------------------------------------- exactness --

#[test]
fn the_polled_materialized_composite_matches_the_full_frame_at_u16() {
    let doc = materialized16();
    let reference = full16(&doc, false);
    // The full frame's own styled composite now reports under a live job:
    // pixels unchanged.
    assert!(
        full16(&doc, true) == reference,
        "the full-frame render under a live control differs"
    );
    for window in WINDOWS {
        assert!(
            polled_region16(&doc, window) == crop(&reference, window),
            "window {window:?} differs from the projection's crop"
        );
    }
}

// --------------------------------------------- the worker's own report log --

#[test]
fn the_materialized_composite_reports_inside_its_range_and_polls_per_row() {
    let doc = materialized16();
    let (s, e, adv) = points(&doc);
    let log = reports(whole16(materialized16()));
    // (i) The planes RAMP inside `(s, e)`: the blur's two passes report once
    // per plane row each (2·ph ≥ 2·H values); under `checkpoints_only` the
    // only value there was the base advance's `3A/work`.
    let mut inside: Vec<f64> = log
        .iter()
        .map(|r| r.progress)
        .filter(|&p| s + 1e-6 < p && p < e - 1e-6)
        .collect();
    inside.dedup();
    assert!(
        inside.len() >= H as usize,
        "{} distinct reports strictly inside the child's composite range ({s}, {e}); the \
         planes report per row",
        inside.len()
    );
    // (ii) Nothing after the planes leaks past `e`: the first report above
    // it is the child's style advance, and the one before it (the base
    // advance) holds at `e`.
    let (last, next) = composite_span(&log, e);
    assert!(
        (log[next].progress - adv).abs() <= 1e-6,
        "the first report above e = {e} is {} (checkpoint {}), not the style advance {adv}",
        log[next].progress,
        log[next].checkpoints
    );
    assert!(
        (log[next - 1].progress - e).abs() <= 1e-6,
        "the base advance reports {}, not the range's end {e}",
        log[next - 1].progress
    );
    // (iii) The composite after the planes polls per ROW: the Below
    // contribution's H rows (13A-5b-iii-c-2), the pixel pass's H rows and
    // the Interior contribution's H rows.
    let polled = log[next - 1].checkpoints - log[last].checkpoints;
    assert!(
        polled >= 3 * u64::from(H),
        "{polled} checkpoints between the planes' last report and the base advance; the \
         two contributions and the pixel pass poll once per row each (≥ {})",
        3 * H
    );
    eprintln!(
        "13A-5b-iii-c-2 report log: {} distinct values inside ({s:.6}, {e:.6}); {polled} \
         checkpoints from the planes' last report (checkpoint {}) to the base advance",
        inside.len(),
        log[last].checkpoints
    );
}

#[test]
fn a_cancel_parked_inside_the_materialized_composite_returns_cancelled() {
    let (_, e, _) = points(&materialized16());
    let log = reports(whole16(materialized16()));
    let (last, _) = composite_span(&log, e);
    // H/2 checkpoints past the planes' last report: inside the Below
    // contribution's H rows (module doc).
    let count = log[last].checkpoints + u64::from(H) / 2;
    let p = cancelled_parked_at(
        count,
        &|progress, _| (progress - e).abs() <= 1e-6,
        whole16(materialized16()),
    );
    eprintln!(
        "13A-5b-iii-c-2 parked cancel inside the materialized composite (e = {e:.6}): {}",
        parked_line(&p)
    );
}

// ------------------------------------------------ bench (cancel-bench) --

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-c-2)"]
fn the_materialized_composite_planes_report_inside_their_range() {
    let (s, e, _) = points(&materialized16());
    let (log, tf) = logged(whole16(materialized16()));
    assert_ramp(&log, s, e, "the materialized child's composite");
    assert_pin(&log, tf, "the materialized child's composite");
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-c-2)"]
fn a_concurrently_cancelled_materialized_composite_returns_within_the_model() {
    let (_, e, _) = points(&materialized16());
    let entry = move |p: f64, _: u32, _: u64| p >= e - 1e-6;
    let ((t0, n0), (t1, n1)) = window(&entry, &|p, _, _| p > e + 1e-6, whole16(materialized16()));
    let n_comp = n1.saturating_sub(n0).max(1);
    let t_comp = t1.saturating_sub(t0).as_secs_f64();
    let c_row = t_comp / n_comp as f64;
    let k = n_comp / 4;
    let p = cancelled_after_checkpoints(
        &entry,
        k,
        &|progress, _| (progress - e).abs() <= 1e-6,
        whole16(materialized16()),
    );
    let bound = RETURN_FLOOR_S + c_row;
    eprintln!(
        "13A-5b-iii-c-2 u16 cancel inside the materialized styled composite (satin \
         {SIZE}): {:.3} ms cancel-to-return (issued at {:.1} ms; {}); window {:.1}–{:.1} ms, \
         t_comp {:.3} ms over n_comp {n_comp} checkpoints, c_row {:.3} µs; bound T_f + c_row = \
         {:.3} ms (T_f 1 ms); ≈{:.2} intervals past the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        t0.as_secs_f64() * 1e3,
        t1.as_secs_f64() * 1e3,
        t_comp * 1e3,
        c_row * 1e6,
        bound * 1e3,
        past(p.latency, c_row),
        verdict(p.latency, bound),
    );
}
