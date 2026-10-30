//! Phase 13A-5b-iii-c-1: the TYPED staged guide's progress inside the
//! regional render — `precision_regional_guide::render_staged` builds each
//! guide under `progress.job.range(start, end)` (its plan work `g`), and
//! `adjust_guide::build` gives its two grid blurs their OWN sub-ranges of it
//! (`GuideWork::blur_ranges`): the grid pass and per-cell means flat at the
//! range's start, the first blur ramping `(p + c)/t → (p + c + 2·c·taps)/t`,
//! the second on to `(t − c)/t`, the combine flat, then the driver's advance
//! at the range's end — the byte guide's shape
//! (`regional_byte_preparation_cancel.rs`), at u16.
//!
//! The fixture is the byte `guided()`'s twin at u16 (builders copied from
//! `regional_precision_guide.rs`): a 3000×2000 sRGB canvas with an opaque
//! seeded backdrop and a translucent Multiply raster (stage 0), a radius-30
//! `precision_v1` Shadows/Highlights at the ROOT and a SoftLight raster (the
//! final stage). Its progress figures (`guide_points16`): stage 0's close
//! `close = 3A / work` (`A = |R_0|`: the stage-0 `prev` zero fill, then its
//! two rows), the guide's advance `guide = (3A + g) / work`, and `next`, the
//! final stage's first report after the advance — the output zero fill's
//! first `CHECKPOINT_PIXELS` chunk, `guide + 4096 / work` (`Last::Output` →
//! `zero_plane`).
//!
//! LOAD ROBUSTNESS (13A-5b-iii-b-1): the default test holds only
//! load-independent facts — the worker's OWN report log (`reports`, under
//! `record_reports`: the first report above the advance IS `next`) and the
//! one-sided INTERVAL guard over the spinning `logged` trace (a stall can only
//! make it miss a leak); the RAMP guards, whose strictly-inside clause a
//! logger stall can fail, and the PIN, a ratio of two wall-clock
//! readings, are `#[ignore]`d and run through `make cancel-bench`.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::BlendMode;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use crate::common;
use common::probe::*;
use common::take_err_string;

use crate::region_plan;

type Window = (u32, u32, u32, u32);

const W: u32 = 3000;
const H: u32 = 2000;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const RADIUS: f32 = 30.0;

// ------------------------------------------------------------- plumbing --

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

// ------------------------------------------------------------ fixtures --

fn profile() -> Arc<IccProfile> {
    IccProfile::srgb()
}

fn image16(w: u32, h: u32, seed: u32, opaque: bool) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                ((x ^ y) * 1999 % 65536 + seed) as u16,
                if opaque {
                    65535
                } else {
                    (9000 + (x * 3001 + y * 1777 + seed * 97) % 56000) as u16
                },
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
    Layer::raster(
        name,
        image16(size.0, size.1, 5, false),
        offset,
        opacity,
        true,
    )
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

/// The byte `guided()`'s twin at u16: an opaque seeded backdrop and a
/// Multiply raster (stage 0), the radius-30 guide, a SoftLight raster.
fn guided16() -> PrecisionDocument {
    let layers = vec![
        Layer::raster("Background", image16(W, H, 1, true), (0, 0), 1.0, true).unwrap(),
        raster(
            "Multiply",
            (W - 40, H - 40),
            (20, 20),
            BlendMode::Multiply,
            0.9,
        ),
        guide(RADIUS),
        raster(
            "SoftLight",
            (W / 2, H / 2),
            (100, 100),
            BlendMode::SoftLight,
            0.8,
        ),
    ];
    PrecisionDocument::from_parts(
        W,
        H,
        profile(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

// ---------------------------------------------- the guide's own sizing --
//
// Written out independently of the core: the guide's sigma is radius / 2;
// the grid works at factor 1 up to sigma 3, then 2, 4, 8 at each doubling;
// above factor 1 the grid sigma is reduced by the resample's own softening;
// the blur's kernel is the odd width nearest `2·r + 1`, `r` the feather
// radius of the grid sigma.

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

fn window_of(v: &Value) -> Window {
    let n = |k: &str| v[k].as_u64().unwrap() as u32;
    (n("x"), n("y"), n("width"), n("height"))
}

fn area((_, _, w, h): Window) -> f64 {
    f64::from(w) * f64::from(h)
}

/// The guide's work over `plane` (the shared typed planner).
fn guide_work(plane: Window) -> f64 {
    let meta = op(json!({"shadows":{"amount":0.7},"radius":RADIUS}));
    PrecisionAdjustment::guide_plan_for_description(&meta, plane.2, plane.3)
        .unwrap()
        .1 as f64
}

/// The one stage's `R_0` and the plan's work.
fn stage_plane() -> (Window, f64) {
    let p = plan16(&guided16());
    assert_eq!(p["stages"].as_array().unwrap().len(), 1);
    let work = p["work_units"].as_u64().unwrap() as f64;
    (window_of(&p["stages"][0]["plane"]), work)
}

/// `(close, guide, next)` (module doc): stage 0's close `3A / work`, the
/// guide's advance `(3A + g) / work` and the output zero fill's first chunk
/// `guide + 4096 / work`.
fn guide_points16() -> (f64, f64, f64) {
    let (r0, work) = stage_plane();
    let a = area(r0);
    let g = guide_work(r0);
    assert!(g / work > 1e-5, "the guide's report is resolvable");
    let guide = (3.0 * a + g) / work;
    (3.0 * a / work, guide, guide + 4096.0 / work)
}

/// The two grid-blur sub-ranges over `(close, guide)`, as the byte suite's
/// `guide_sub_ranges`: `close + (guide − close)·{(p + c)/t,
/// (p + c + 2·c·taps)/t, (t − c)/t}`, `t` asserted equal to the planner's.
fn guide_sub_ranges16() -> (f64, f64, f64) {
    let (close, guide, _) = guide_points16();
    let (r0, _) = stage_plane();
    let sigma = RADIUS / 2.0;
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

// --------------------------------------------------- reports-once guards --

#[test]
fn the_typed_guide_leaks_no_report_after_its_advance() {
    let (_, guide, next) = guide_points16();
    // (i) The worker's own log: the first report above the guide's advance
    // is the output zero fill's first chunk — nothing from the guide's build
    // lands past its range.
    let log = reports(whole16(guided16()));
    let first = log
        .iter()
        .find(|r| r.progress > guide + 1e-6)
        .expect("a report after the guide's advance");
    assert!(
        (first.progress - next).abs() <= 1e-6,
        "the first report above the guide's advance {guide} is {} (stage {}, checkpoint {}), \
         not the output fill's first chunk {next}",
        first.progress,
        first.stage,
        first.checkpoints
    );
    // (ii) The spinning trace: one-sided only.
    let (log, _) = logged(whole16(guided16()));
    assert_interval(&log, guide, next, "the typed guide");
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-c-1)"]
fn the_typed_guide_blurs_each_report_inside_their_sub_ranges() {
    // 13A-5b-iii-c-1: the first blur ramps `b1 → b2`, the second `b2 → b3`.
    let (b1, b2, b3) = guide_sub_ranges16();
    let (log, tf) = logged(whole16(guided16()));
    assert_ramp(&log, b1, b2, "the typed guide's first blur");
    assert_ramp(&log, b2, b3, "the typed guide's second blur");
    assert_pin(&log, tf, "the typed guide");
}
