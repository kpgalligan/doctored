//! The Healing Brush's two exports — `dr_doc_heal_layer` and
//! `dr_doc_heal_to_new_layer` — under a job control. A controlled call is the
//! NULL-control call byte for byte; a cancel at any stage returns NULL and
//! "operation cancelled" with no partial document and no scratch left
//! reserved; the token is single-use and the NULL/shape refusals leave it
//! unclaimed; the working-memory limit refuses by name; and each component's
//! stage-4 reports land in a share of the post-plan range weighted by its
//! padded window area.
//!
//! Building a component's window planes and writing the healed rect
//! back both poll — the checkpoint counts between the reports are exactly
//! the declared ones, and a cancel parked inside either returns nothing. The
//! byte inpaint's final write polls the same way. One `make cancel-bench`
//! probe times both gaps on a large canvas.
//!
//! The union read (heal and byte inpaint) polls once per
//! visited tile, the new-layer deposit's checkpoints are exactly
//! `common::deposit_checkpoints`, and the spot heal's reservations carry
//! component records. Those are the pre-walk
//! `heal_components::component_count_bound` before its walk and one per
//! component found after it, not one per covered pixel; and the deposit reads
//! only the healed rect, since the new layer is transparent.
//!
//! The Healing Brush's reserve before its component walk carries one
//! component record per start `heal_components::component_count_bound`
//! counts — a proven upper bound on the components — and its post-plan
//! reserve one per component found, so a dense Patch pays one record, not
//! one per covered pixel.
//!
//! Three more `make cancel-bench` probes time a cancel inside the
//! heal's and the spot heal's union reads and inside each step of the
//! new-layer deposit, on a canvas whose union spans 192 tiles.
//!
//! The spot heal's post-plan reserve charges the windows' union rect
//! on the layer, not the whole layer, and one more `make cancel-bench` probe
//! times each step of the byte Content-Aware Fill's new-layer deposit.
//!
//! Black-box through the FFI. Every cancel PARKS the worker at a counted
//! checkpoint (`common::probe::cancelled_parked_at`) taken from the worker's
//! own report log — no poller races it and nothing here reads a clock,
//! except the ignored bench probes.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_content_aware::dr_doc_content_aware_fill_edit;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_heal::{dr_doc_heal_layer, dr_doc_heal_to_new_layer, dr_doc_retouch_source};
use doctored_core::ffi_jobs::*;
use doctored_core::job_control::{DrJobControl, Report};
use doctored_core::layer_raster::LayerRaster;
use image::{Rgba, RgbaImage};

use crate::common;
use common::probe::*;
use common::*;

const W: u32 = 160;
const H: u32 = 120;
const CURRENT_AND_BELOW: u32 = 1;
/// `precision_inpaint::COMPONENT_BYTES`, restated (the module is private).
const COMPONENT_BYTES: u64 = 128;
/// `doc_heal`'s `HEAL_CANVAS_BYTES_PER_PIXEL`, restated (it is private).
const HEAL_CANVAS_BYTES: u64 = 10;
/// `doc_heal`'s `HEAL_WINDOW_BYTES_PER_PIXEL`, restated (it is private).
const HEAL_WINDOW_BYTES: u64 = 48;

/// A single opaque layer: a smooth gradient the heal's membrane can match.
fn fixture() -> DrDocument {
    gradient(W, H)
}

/// [`fixture`]'s gradient at any size, each channel folded back into range.
fn gradient(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x % 200 + 40) as u8,
            (y % 180 + 60) as u8,
            ((x + y) % 400 / 2 + 20) as u8,
            255,
        ])
    }))
}

/// A canvas-frame PREMULTIPLIED overlay at full coverage inside each rect,
/// whose straight RGB is a coarse checker — texture the destination does not
/// have, so the healed interior always moves.
fn overlay(rects: &[(u32, u32, u32, u32)]) -> Vec<u8> {
    overlay_in(W, H, rects)
}

/// [`overlay`] on a `w × h` canvas.
fn overlay_in(w: u32, h: u32, rects: &[(u32, u32, u32, u32)]) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    for &(x0, y0, x1, y1) in rects {
        for y in y0..y1 {
            for x in x0..x1 {
                let v = if (x / 3 + y / 3) % 2 == 0 { 70 } else { 150 };
                let i = ((y * w + x) * 4) as usize;
                out[i..i + 4].copy_from_slice(&[v, v + 20, v + 40, 255]);
            }
        }
    }
    out
}

/// One 40 x 40 footprint in the middle of the canvas.
fn stroke() -> Vec<u8> {
    overlay(&[(60, 40, 100, 80)])
}

/// The current-and-below plate for layer 0 of `doc`.
fn plate(doc: &DrDocument) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let img =
        unsafe { dr_doc_retouch_source(doc, 0, CURRENT_AND_BELOW, false, ptr::null(), &mut err) };
    assert!(err.is_null() && !img.is_null(), "plate refused");
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    out
}

/// Which export a call drives.
#[derive(Clone, Copy)]
enum Export {
    InPlace,
    NewLayer,
}

/// Runs `export` on `doc` with `control` (NULL allowed): the result handle
/// (NULL = refused) and the message, if any. `W × H` unless `doc` is a live
/// document, whose own size is passed.
fn call(
    export: Export,
    doc: *const DrDocument,
    overlay: *const u8,
    plate: *const u8,
    control: *const DrJobControl,
) -> (*mut DrDocument, Option<String>) {
    let (w, h) = unsafe { doc.as_ref() }.map_or((W, H), |d| (d.width, d.height));
    let name = CString::new("Retouch").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        match export {
            Export::InPlace => dr_doc_heal_layer(doc, 0, overlay, w, h, 1.0, control, &mut err),
            Export::NewLayer => dr_doc_heal_to_new_layer(
                doc,
                0,
                overlay,
                w,
                h,
                1.0,
                plate,
                name.as_ptr(),
                control,
                &mut err,
            ),
        }
    };
    let message = (!err.is_null()).then(|| take_err_string(err));
    (out, message)
}

/// A render for the probe helpers: `export` on the document at `doc`
/// (an address, so the closure is `Send`), freeing any result.
fn render(
    export: Export,
    doc: usize,
    overlay: Vec<u8>,
    plate: Vec<u8>,
) -> impl FnOnce(usize) -> (bool, String) + Send {
    move |token: usize| {
        let (out, message) = call(
            export,
            doc as *const DrDocument,
            overlay.as_ptr(),
            plate.as_ptr(),
            token as *const DrJobControl,
        );
        let ok = !out.is_null();
        if ok {
            unsafe { dr_doc_free(out) };
        }
        (ok, message.unwrap_or_default())
    }
}

/// Every layer's bytes, bottom first.
fn all_layers(doc: *const DrDocument) -> Vec<Vec<u8>> {
    let count = unsafe { dr_doc_layer_count(doc) };
    (0..count).map(|i| layer_pixels(doc, i)).collect()
}

#[test]
fn a_fresh_control_matches_the_null_control_and_closes_at_complete() {
    let doc = fixture();
    let (ov, pl) = (stroke(), plate(&doc));
    for export in [Export::InPlace, Export::NewLayer] {
        let (plain, message) = call(export, &doc, ov.as_ptr(), pl.as_ptr(), ptr::null());
        assert!(message.is_none() && !plain.is_null(), "{message:?}");
        let control = unsafe { dr_job_control_create() };
        let (controlled, message) = call(export, &doc, ov.as_ptr(), pl.as_ptr(), control);
        assert!(message.is_none() && !controlled.is_null(), "{message:?}");
        assert_eq!(all_layers(controlled), all_layers(plain));
        unsafe {
            assert_eq!(dr_job_control_progress(control), 1.0);
            assert_eq!(dr_job_control_stage(control), 5);
            assert_eq!(dr_job_control_scratch_bytes(control), 0);
            dr_doc_free(plain);
            dr_doc_free(controlled);
            dr_job_control_free(control);
        }
    }
}

#[test]
fn a_precancelled_control_returns_nothing_and_leaves_the_input_alone() {
    let doc = fixture();
    let before = doc.layers[0].pixels.clone();
    let (ov, pl) = (stroke(), plate(&doc));
    for export in [Export::InPlace, Export::NewLayer] {
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let (out, message) = call(export, &doc, ov.as_ptr(), pl.as_ptr(), control);
        assert!(out.is_null());
        assert_eq!(message.as_deref(), Some("operation cancelled"));
        assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &before));
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        unsafe { dr_job_control_free(control) };
    }
}

/// A parked cancel of `export` at `count`, asserting the verdict, a live
/// reservation while parked, and an untouched input.
fn parked_cancel(export: Export, count: u64, verdict: &dyn Fn(f64, u32) -> bool) {
    let doc = fixture();
    let before = doc.layers[0].pixels.clone();
    let parked = cancelled_parked_at(
        count,
        verdict,
        render(export, &doc as *const _ as usize, stroke(), plate(&doc)),
    );
    println!("{}", parked_line(&parked));
    assert!(parked.scratch > 0, "nothing was reserved while parked");
    assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &before));
}

/// The checkpoint count of the first report of `stage` in an uncancelled run.
fn stage_count(export: Export, stage: u32) -> u64 {
    let doc = fixture();
    entry_count(
        &|_, s, _| s == stage,
        render(export, &doc as *const _ as usize, stroke(), plate(&doc)),
    )
}

#[test]
fn a_cancel_while_planning_returns_nothing() {
    let count = stage_count(Export::InPlace, 2) - 1;
    parked_cancel(Export::InPlace, count, &|_, stage| stage == 1);
}

#[test]
fn a_cancel_once_the_plan_is_measured_returns_nothing() {
    let count = stage_count(Export::InPlace, 2) + 1;
    parked_cancel(Export::InPlace, count, &|progress, stage| {
        stage == 2 && (progress - 0.05).abs() <= 1e-6
    });
}

#[test]
fn a_cancel_inside_a_solve_returns_nothing() {
    let count = stage_count(Export::InPlace, 4) + 1;
    parked_cancel(Export::InPlace, count, &|progress, stage| {
        stage == 4 && progress < 1.0
    });
}

#[test]
fn a_cancel_inside_a_new_layer_solve_returns_nothing() {
    let count = stage_count(Export::NewLayer, 4) + 1;
    parked_cancel(Export::NewLayer, count, &|progress, stage| {
        stage == 4 && progress < 1.0
    });
}

#[test]
fn a_consumed_token_is_refused() {
    let doc = fixture();
    let (ov, pl) = (stroke(), plate(&doc));
    for export in [Export::InPlace, Export::NewLayer] {
        let control = unsafe { dr_job_control_create() };
        let (first, message) = call(export, &doc, ov.as_ptr(), pl.as_ptr(), control);
        assert!(message.is_none() && !first.is_null(), "{message:?}");
        let (second, message) = call(export, &doc, ov.as_ptr(), pl.as_ptr(), control);
        assert!(second.is_null());
        assert_eq!(
            message.as_deref(),
            Some("job control has already been used")
        );
        unsafe {
            dr_doc_free(first);
            dr_job_control_free(control);
        }
    }
}

#[test]
fn the_working_memory_limit_refuses_by_name() {
    let doc = fixture();
    let (ov, pl) = (stroke(), plate(&doc));
    for export in [Export::InPlace, Export::NewLayer] {
        let control = unsafe { dr_job_control_create() };
        assert!(unsafe { dr_job_control_set_scratch_limit(control, 1) });
        let (out, message) = call(export, &doc, ov.as_ptr(), pl.as_ptr(), control);
        assert!(out.is_null());
        let message = message.expect("a limit refusal speaks");
        assert!(message.contains("working-memory limit"), "{message}");
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        unsafe { dr_job_control_free(control) };
    }
}

#[test]
fn component_reports_are_weighted_by_padded_window_area() {
    // Two components, the first in scan order the smaller: 10 x 10 (padded
    // 12 x 12) above 30 x 20 (padded 32 x 22).
    let (a1, a2) = (12.0 * 12.0, 32.0 * 22.0);
    let doc = fixture();
    let log = reports(render(
        Export::InPlace,
        &doc as *const _ as usize,
        overlay(&[(10, 10, 20, 20), (60, 50, 90, 70)]),
        Vec::new(),
    ));
    let boundary = 0.05 + 0.95 * a1 / (a1 + a2);
    assert!(
        log.iter()
            .any(|r| r.stage == 4 && (r.progress - boundary).abs() <= 1e-6),
        "no stage-4 report at {boundary}: {log:?}"
    );
}

#[test]
fn null_arguments_refuse_silently_and_leave_the_token_unclaimed() {
    let doc = fixture();
    let (ov, pl) = (stroke(), plate(&doc));
    let cases: [(Export, *const DrDocument, *const u8, *const u8); 4] = [
        (Export::InPlace, ptr::null(), ov.as_ptr(), pl.as_ptr()),
        (Export::InPlace, &doc, ptr::null(), pl.as_ptr()),
        (Export::NewLayer, &doc, ov.as_ptr(), ptr::null()),
        (Export::NewLayer, ptr::null(), ov.as_ptr(), pl.as_ptr()),
    ];
    for (export, d, o, p) in cases {
        let control = unsafe { dr_job_control_create() };
        let (out, message) = call(export, d, o, p, control);
        assert!(out.is_null() && message.is_none(), "{message:?}");
        let (claimed, message) = call(export, &doc, ov.as_ptr(), pl.as_ptr(), control);
        assert!(!claimed.is_null() && message.is_none(), "{message:?}");
        unsafe {
            dr_doc_free(claimed);
            dr_job_control_free(control);
        }
    }
}

#[test]
fn empty_coverage_refuses_silently_and_still_completes() {
    let doc = fixture();
    let (ov, pl) = (overlay(&[]), plate(&doc));
    for export in [Export::InPlace, Export::NewLayer] {
        let control = unsafe { dr_job_control_create() };
        let (out, message) = call(export, &doc, ov.as_ptr(), pl.as_ptr(), control);
        assert!(out.is_null() && message.is_none(), "{message:?}");
        assert_eq!(unsafe { dr_job_control_progress(control) }, 1.0);
        unsafe { dr_job_control_free(control) };
    }
}

// ------------------------------------------------ the window planes and --
// -------------------------------------------------- the final write poll --

/// A canvas whose one footprint's padded window spans two tile rows and two
/// tile columns.
const TW: u32 = 600;
const TH: u32 = 400;
/// The footprint, `(x0, y0, x1, y1)` exclusive: 400 × 80 covered pixels.
const WIDE: (u32, u32, u32, u32) = (100, 200, 500, 280);

/// `WIDE` padded by the heal's one-pixel ring (inside the canvas here).
fn padded() -> (u32, u32, u32, u32) {
    (WIDE.0 - 1, WIDE.1 - 1, WIDE.2 + 1, WIDE.3 + 1)
}

/// `export` over `WIDE` on a `TW × TH` gradient at `doc`.
fn wide_render(export: Export, doc: &DrDocument) -> impl FnOnce(usize) -> (bool, String) + Send {
    render(
        export,
        doc as *const _ as usize,
        overlay_in(TW, TH, &[WIDE]),
        plate(doc),
    )
}

/// The checkpoint counts of an uncancelled run's first stage-2, first and
/// last stage-4 and stage-5 reports.
fn marks(log: &[Report]) -> (u64, u64, u64, u64) {
    let first = |stage: u32| log.iter().find(|r| r.stage == stage).unwrap().checkpoints;
    let last4 = log.iter().rev().find(|r| r.stage == 4).unwrap().checkpoints;
    (first(2), first(4), last4, first(5))
}

#[test]
fn the_window_planes_and_the_final_write_poll_at_their_declared_interval() {
    let covered = u64::from((WIDE.2 - WIDE.0) * (WIDE.3 - WIDE.1));
    let p = padded();
    let window = u64::from((p.2 - p.0) * (p.3 - p.1));
    assert!(
        rect_tiles(p) >= 2,
        "the fixture must span more than one tile"
    );
    for export in [Export::InPlace, Export::NewLayer] {
        let doc = gradient(TW, TH);
        let (stage2, first4, last4, stage5) = marks(&reports(wide_render(export, &doc)));
        // The reservation, one check per visited tile of the union read,
        // the component loop's check, the component-pixel pass every
        // 4096, the window pass every 4096, then the solve's first report's
        // own check.
        assert_eq!(
            first4 - stage2,
            3 + rect_tiles(p) + covered.div_ceil(4096) + window.div_ceil(4096),
            "window planes"
        );
        // One check per visited tile of the write, the new layer's deposit,
        // then the closing one.
        let deposit = match export {
            Export::InPlace => 0,
            Export::NewLayer => deposit_checkpoints(u64::from(TW * TH), WIDE),
        };
        assert_eq!(stage5 - last4, 1 + rect_tiles(p) + deposit, "final write");
    }
}

/// A parked cancel of `export` over `WIDE` at `count`: a live reservation
/// while parked, an untouched input.
fn wide_parked_cancel(export: Export, count: u64, verdict: &dyn Fn(f64, u32) -> bool) {
    let doc = gradient(TW, TH);
    let before = doc.layers[0].pixels.clone();
    let parked = cancelled_parked_at(count, verdict, wide_render(export, &doc));
    println!("{}", parked_line(&parked));
    assert!(parked.scratch > 0, "nothing was reserved while parked");
    assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &before));
}

#[test]
fn a_cancel_inside_the_window_planes_or_the_final_write_returns_nothing() {
    for export in [Export::InPlace, Export::NewLayer] {
        let (stage2, first4, last4, _) = {
            let doc = gradient(TW, TH);
            marks(&reports(wide_render(export, &doc)))
        };
        // The union read's first tile, the component-pixel pass's
        // first poll, the window pass's last.
        let read = rect_tiles(padded());
        for count in [stage2 + 2, stage2 + 3 + read, first4 - 1] {
            wide_parked_cancel(export, count, &|progress, stage| {
                stage == 2 && (progress - 0.05).abs() <= 1e-6
            });
        }
        // The write's first tile.
        wide_parked_cancel(export, last4 + 1, &|progress, stage| {
            stage == 4 && progress >= 1.0 - 1e-6
        });
    }
}

#[test]
fn a_cancel_inside_the_inpaint_final_write_returns_nothing() {
    // A spot-heal dab straddling a tile column: its window, and so the
    // written rect, spans two tile columns.
    let (tw, th) = (TW, TH);
    let doc = gradient(tw, th);
    let before = doc.layers[0].pixels.clone();
    let dab = overlay_in(tw, th, &[(240, 100, 270, 130)]);
    let spot = move |doc: usize| {
        let dab = dab.clone();
        move |token: usize| {
            let mut err: *mut c_char = ptr::null_mut();
            let out = unsafe {
                dr_doc_spot_heal_layer_job(
                    doc as *const DrDocument,
                    0,
                    dab.as_ptr(),
                    tw,
                    th,
                    1.0,
                    0,
                    7,
                    0,
                    0,
                    false,
                    token as *const DrJobControl,
                    &mut err,
                )
            };
            let message = (!err.is_null()).then(|| take_err_string(err));
            let ok = !out.is_null();
            if ok {
                unsafe { dr_doc_free(out) };
            }
            (ok, message.unwrap_or_default())
        }
    };
    let at = &doc as *const _ as usize;
    let log = reports(spot(at));
    let last4 = log.iter().rev().find(|r| r.stage == 4).unwrap().checkpoints;
    let stage5 = log.iter().find(|r| r.stage == 5).unwrap().checkpoints;
    // At least two tiles' checks and the closing one.
    assert!(
        stage5 - last4 >= 3,
        "the write polled {} times",
        stage5 - last4 - 1
    );
    let parked = cancelled_parked_at(last4 + 1, &|_, stage| stage == 4, spot(at));
    println!("{}", parked_line(&parked));
    assert!(parked.scratch > 0);
    assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &before));
}

/// `dr_doc_spot_heal_layer_job` (ring 0, seed 7, sample Current) with
/// `overlay` on the `w × h` document at `doc`, freeing any result.
fn spot_render(
    doc: usize,
    overlay: Vec<u8>,
    (w, h): (u32, u32),
) -> impl FnOnce(usize) -> (bool, String) + Send {
    move |token: usize| {
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_doc_spot_heal_layer_job(
                doc as *const DrDocument,
                0,
                overlay.as_ptr(),
                w,
                h,
                1.0,
                0,
                7,
                0,
                0,
                false,
                token as *const DrJobControl,
                &mut err,
            )
        };
        let message = (!err.is_null()).then(|| take_err_string(err));
        let ok = !out.is_null();
        if ok {
            unsafe { dr_doc_free(out) };
        }
        (ok, message.unwrap_or_default())
    }
}

#[test]
fn the_inpaint_union_read_polls_once_per_tile() {
    // The dab's window is its box grown by the ring-21 minimum,
    // (219, 79)–(291, 151), which straddles a tile column.
    let doc = gradient(TW, TH);
    let before = doc.layers[0].pixels.clone();
    let at = &doc as *const _ as usize;
    let dab = || overlay_in(TW, TH, &[(240, 100, 270, 130)]);
    let window = (219, 79, 291, 151);
    assert_eq!(rect_tiles(window), 2);
    let log = reports(spot_render(at, dab(), (TW, TH)));
    let opening: Vec<u64> = log
        .iter()
        .filter(|r| r.stage == 3 && (r.progress - 0.18).abs() <= 1e-6)
        .map(|r| r.checkpoints)
        .take(2)
        .collect();
    // The read's tiles, the check after it, the first component's report.
    assert_eq!(opening[1] - opening[0], 2 + rect_tiles(window), "{log:?}");
    let parked = cancelled_parked_at(
        opening[0] + 1,
        &|progress, stage| stage == 3 && (progress - 0.18).abs() <= 1e-6,
        spot_render(at, dab(), (TW, TH)),
    );
    println!("{}", parked_line(&parked));
    assert!(parked.scratch > 0);
    assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &before));
}

/// The spot heal's windows' union on the canvas, in pixels: each dab rect
/// (exclusive) grown by the ring-21 minimum on every side, the union's box
/// clamped to the `w × h` canvas.
fn padded_union(rects: &[(u32, u32, u32, u32)], (w, h): (u32, u32)) -> u64 {
    const RING: u32 = 21;
    let x0 = rects
        .iter()
        .map(|r| r.0.saturating_sub(RING))
        .min()
        .unwrap();
    let y0 = rects
        .iter()
        .map(|r| r.1.saturating_sub(RING))
        .min()
        .unwrap();
    let x1 = rects.iter().map(|r| (r.2 + RING).min(w)).max().unwrap();
    let y1 = rects.iter().map(|r| (r.3 + RING).min(h)).max().unwrap();
    u64::from(x1 - x0) * u64::from(y1 - y0)
}

#[test]
fn a_spot_heal_reserves_one_component_record_per_component() {
    // 112 isolated 2 × 2 dabs, each its own component with a
    // 44 × 44 window (the box plus the ring-21 minimum on every side). Each
    // dab has one start and is one component, so both reserves carry 112.
    // The post-plan reserve charges the windows' union rect, not the
    // layer.
    let doc = doc_of(TW, TH, plasma);
    let at = &doc as *const _ as usize;
    let rects: Vec<(u32, u32, u32, u32)> = (0..14)
        .flat_map(|i| (0..8).map(move |j| (30 + 40 * i, 30 + 40 * j)))
        .map(|(x, y)| (x, y, x + 2, y + 2))
        .collect();
    let union = padded_union(&rects, (TW, TH));
    assert_eq!(union, 564 * 324);
    let dabs = move || overlay_in(TW, TH, &rects);
    let n = u64::from(TW * TH);
    let records = 112 * COMPONENT_BYTES;
    let base = n * 10 + 8 * 1024 * 1024 + records;
    type Verdict<'a> = &'a dyn Fn(f64, u32) -> bool;
    let cases: [(u32, f64, u64, Verdict); 2] = [
        (1, 0.03, base, &|_, stage| stage == 1),
        (3, 0.18, base + 4 * union + 136 * 44 * 44, &|_, stage| {
            stage == 2
        }),
    ];
    for (stage, progress, expected, verdict) in cases {
        let count = entry_count(
            &|p, s, _| s == stage && (p - progress).abs() <= 1e-6,
            spot_render(at, dabs(), (TW, TH)),
        );
        let parked = cancelled_parked_at(count, verdict, spot_render(at, dabs(), (TW, TH)));
        println!("{}", parked_line(&parked));
        assert_eq!(parked.scratch, expected, "stage {stage} at {progress}");
    }
}

#[test]
fn a_dense_spot_heal_reserves_its_bound_then_its_count() {
    // A U — two 4 × 12 bars joined by a 4 × 4 foot, 112 covered
    // pixels. Two starts (each bar's top-left pixel; the foot's left
    // neighbour is covered), one component, a 12 × 12 box and so a 54 × 54
    // window (the ring-21 minimum on every side).
    // The post-plan reserve charges the one window's rect.
    let doc = doc_of(TW, TH, plasma);
    let at = &doc as *const _ as usize;
    let rects = [
        (100, 100, 104, 112),
        (108, 100, 112, 112),
        (104, 108, 108, 112),
    ];
    let union = padded_union(&rects, (TW, TH));
    assert_eq!(union, 54 * 54);
    let u = || overlay_in(TW, TH, &rects);
    let n = u64::from(TW * TH);
    let base = n * 10 + 8 * 1024 * 1024;
    type Verdict<'a> = &'a dyn Fn(f64, u32) -> bool;
    let cases: [(u32, f64, u64, Verdict); 2] = [
        (1, 0.03, base + 2 * COMPONENT_BYTES, &|_, stage| stage == 1),
        (
            3,
            0.18,
            base + COMPONENT_BYTES + 4 * union + 136 * 54 * 54,
            &|_, stage| stage == 2,
        ),
    ];
    for (stage, progress, expected, verdict) in cases {
        let count = entry_count(
            &|p, s, _| s == stage && (p - progress).abs() <= 1e-6,
            spot_render(at, u(), (TW, TH)),
        );
        let parked = cancelled_parked_at(count, verdict, spot_render(at, u(), (TW, TH)));
        println!("{}", parked_line(&parked));
        assert_eq!(parked.scratch, expected, "stage {stage} at {progress}");
    }
}

#[test]
fn a_sparse_heal_reserves_one_component_record_per_component() {
    // The Healing Brush's `components·COMPONENT_BYTES` term and the reserve
    // before its component walk: 112 isolated 4 × 4 dabs, each its own
    // component with a 6 × 6 padded window, and each one start of the
    // pre-walk bound.
    let doc = gradient(TW, TH);
    let before = doc.layers[0].pixels.clone();
    let at = &doc as *const _ as usize;
    let dabs: Vec<(u32, u32, u32, u32)> = (0..14)
        .flat_map(|i| (0..8).map(move |j| (30 + 40 * i, 30 + 40 * j)))
        .map(|(x, y)| (x, y, x + 4, y + 4))
        .collect();
    let dabs = move || overlay_in(TW, TH, &dabs);
    let heal = move || render(Export::InPlace, at, dabs(), Vec::new());
    let n = u64::from(TW * TH);
    let records = 112;
    // The padded windows' union: (29, 29) to (30 + 40·13 + 5, 30 + 40·7 + 5).
    let union: u64 = (555 - 29) * (315 - 29);
    let walk = n * HEAL_CANVAS_BYTES + records * COMPONENT_BYTES + 8 * 1024 * 1024;
    let stage2 = entry_count(&|_, stage, _| stage == 2, heal());
    // Parked at the post-plan reserve's own checkpoint, the pre-walk
    // reservation is still the live one; one checkpoint later (the union
    // read's first tile) the post-plan one has replaced it.
    let cases = [
        (stage2 + 1, walk),
        (stage2 + 2, walk + 4 * union + HEAL_WINDOW_BYTES * 36),
    ];
    for (count, expected) in cases {
        let parked = cancelled_parked_at(count, &|_, stage| stage == 2, heal());
        println!("{}", parked_line(&parked));
        assert_eq!(parked.scratch, expected, "parked at {count}");
        assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &before));
    }
}

// ------------------------------------------------ the pre-walk bound --

/// One dense 400 × 200 Patch on the `TW × TH` gradient: one component whose
/// padded window, 402 × 202, is also the windows' union.
const DENSE: (u32, u32, u32, u32) = (100, 100, 500, 300);
/// `DENSE`'s padded window, in pixels.
const DENSE_WINDOW: u64 = 402 * 202;

/// The heal's figure on the `TW × TH` canvas with `records` component records
/// and no union or window term: the reserve before the component walk.
fn pre_walk(records: u64) -> u64 {
    u64::from(TW * TH) * HEAL_CANVAS_BYTES + records * COMPONENT_BYTES + 8 * 1024 * 1024
}

/// The live reservation of an in-place heal of `rects` on the `TW × TH`
/// gradient, parked `offset` checkpoints after its stage-2 report: 1 is the
/// post-plan reserve's own checkpoint (the pre-walk reservation still live),
/// 2 the union read's first tile (the post-plan one live).
fn parked_heal_scratch(rects: &[(u32, u32, u32, u32)], offset: u64) -> u64 {
    let doc = gradient(TW, TH);
    let before = doc.layers[0].pixels.clone();
    let at = &doc as *const _ as usize;
    let rects = rects.to_vec();
    let heal = || render(Export::InPlace, at, overlay_in(TW, TH, &rects), Vec::new());
    let stage2 = entry_count(&|_, stage, _| stage == 2, heal());
    let parked = cancelled_parked_at(stage2 + offset, &|_, stage| stage == 2, heal());
    println!("{}; scratch {}", parked_line(&parked), parked.scratch);
    assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &before));
    parked.scratch
}

#[test]
fn a_dense_heal_reserves_one_component_record() {
    let walk = pre_walk(1);
    assert_eq!(parked_heal_scratch(&[DENSE], 1), walk, "pre-walk");
    assert_eq!(
        parked_heal_scratch(&[DENSE], 2),
        walk + (4 + HEAL_WINDOW_BYTES) * DENSE_WINDOW,
        "post-plan"
    );
}

#[test]
fn a_dense_heal_is_admitted_under_its_own_post_plan_figure() {
    let limit = pre_walk(1) + (4 + HEAL_WINDOW_BYTES) * DENSE_WINDOW;
    // One record per covered pixel before the walk would have refused it.
    assert!(pre_walk(400 * 200) > limit);
    let doc = gradient(TW, TH);
    let ov = overlay_in(TW, TH, &[DENSE]);
    let control = unsafe { dr_job_control_create() };
    assert!(unsafe { dr_job_control_set_scratch_limit(control, limit) });
    let (out, message) = call(Export::InPlace, &doc, ov.as_ptr(), ptr::null(), control);
    assert!(message.is_none() && !out.is_null(), "{message:?}");
    unsafe {
        assert_eq!(dr_job_control_scratch_bytes(control), 0);
        dr_doc_free(out);
        dr_job_control_free(control);
    }
}

/// The `TW × TH` covered plane of `rects` (exclusive).
fn covered_plane(rects: &[(u32, u32, u32, u32)]) -> Vec<bool> {
    let mut out = vec![false; (TW * TH) as usize];
    for &(x0, y0, x1, y1) in rects {
        for y in y0..y1 {
            for x in x0..x1 {
                out[(y * TW + x) as usize] = true;
            }
        }
    }
    out
}

/// `S`, restated: covered pixels whose left neighbour (when `x > 0`) and
/// upper neighbour (when `y > 0`) are both uncovered.
fn starts(covered: &[bool]) -> u64 {
    let w = TW as usize;
    (0..covered.len())
        .filter(|&i| {
            let (x, y) = (i % w, i / w);
            covered[i] && !(x > 0 && covered[i - 1]) && !(y > 0 && covered[i - w])
        })
        .count() as u64
}

/// The 4-connected component count, by this test's own flood fill.
fn component_count(covered: &[bool]) -> u64 {
    let (w, h) = (TW as usize, TH as usize);
    let mut seen = vec![false; covered.len()];
    let mut count = 0;
    for start in 0..covered.len() {
        if !covered[start] || seen[start] {
            continue;
        }
        count += 1;
        seen[start] = true;
        let mut stack = vec![start];
        while let Some(p) = stack.pop() {
            let (x, y) = (p % w, p / w);
            let mut visit = |q: usize| {
                if covered[q] && !seen[q] {
                    seen[q] = true;
                    stack.push(q);
                }
            };
            if x > 0 {
                visit(p - 1);
            }
            if x + 1 < w {
                visit(p + 1);
            }
            if y > 0 {
                visit(p - w);
            }
            if y + 1 < h {
                visit(p + w);
            }
        }
    }
    count
}

#[test]
fn the_pre_walk_bound_counts_every_component() {
    type Case = (&'static str, Vec<(u32, u32, u32, u32)>, u64, u64);
    let cases: [Case; 3] = [
        // The second block's first pixel sits at x = 0 right below the
        // first block's last row: an `i > 0` left test would drop it.
        ("row wrap", vec![(596, 0, 600, 4), (0, 4, 4, 8)], 2, 2),
        // Two starts along the top, one component through the bottom bar.
        (
            "U",
            vec![
                (100, 100, 104, 112),
                (108, 100, 112, 112),
                (104, 108, 108, 112),
            ],
            2,
            1,
        ),
        // Corner-touching blocks are not 4-connected.
        (
            "staircase",
            (0..5)
                .map(|k| (100 + 4 * k, 100 + 4 * k, 104 + 4 * k, 104 + 4 * k))
                .collect(),
            5,
            5,
        ),
    ];
    for (what, rects, s, k) in cases {
        let covered = covered_plane(&rects);
        assert_eq!(
            (starts(&covered), component_count(&covered)),
            (s, k),
            "{what}"
        );
        let term = parked_heal_scratch(&rects, 1) - pre_walk(0);
        assert_eq!(term % COMPONENT_BYTES, 0, "{what}");
        assert_eq!(term / COMPONENT_BYTES, s, "{what}");
        assert!(s >= k, "{what}");
    }
}

// ------------------------------------------------ bench (cancel-bench) --

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn a_concurrently_cancelled_heal_returns_within_one_poll_of_its_planes_and_its_write() {
    // A 2000 × 1500 footprint on a 4000 × 3000 canvas. Each step's `c` is the
    // longest gap between checkpoint-count changes in the worker's own
    // uncancelled timeline over that step; the bound is `T_f + c`.
    const BW: u32 = 4000;
    const BH: u32 = 3000;
    let region = (1000, 750, 3000, 2250);
    let doc = gradient(BW, BH);
    let run = || {
        render(
            Export::InPlace,
            &doc as *const _ as usize,
            overlay_in(BW, BH, &[region]),
            Vec::new(),
        )
    };
    type Signal<'a> = &'a dyn Fn(f64, u32, u64) -> bool;
    let steps: [(&str, Signal, Signal, u64); 2] = [
        (
            "window planes",
            &|_, stage, _| stage == 2,
            &|_, stage, _| stage == 4,
            // Past the planes' first few checks, inside the component-pixel pass.
            3 + 200,
        ),
        (
            "final write",
            &|progress, stage, _| stage == 4 && progress >= 1.0 - 1e-6,
            &|_, stage, _| stage == 5,
            2,
        ),
    ];
    for (what, entry, exit, k) in steps {
        let t = timeline(entry, exit, run());
        let (longest_at, longest) = t.longest().expect("a gap");
        let c = longest.as_secs_f64();
        let p = probe(
            entry,
            Trigger::After(k),
            &|_, stage| stage == if what == "final write" { 4 } else { 2 },
            run(),
        );
        let bound = RETURN_FLOOR_S + c;
        eprintln!(
            "heal ({BW}×{BH}, footprint 2000×1500) cancel inside the {what}: {:.3} ms \
             cancel-to-return (issued at {:.1} ms; {}); c {:.3} ms (longest gap, after \
             checkpoint {longest_at}); bound T_f + c = {:.3} ms (T_f 1 ms); ≈{:.2} gaps past the \
             floor; {}",
            p.latency.as_secs_f64() * 1e3,
            p.at.as_secs_f64() * 1e3,
            checkpoints_line(&p),
            c * 1e3,
            bound * 1e3,
            past(p.latency, c),
            verdict(p.latency, bound),
        );
    }
}

// ------------------------------------------------- the union reads and --
// ------------------------------------------------- the new-layer deposit --

/// The bench canvas: two 40 × 40 dabs at opposite corners, so the
/// windows' union spans 192 tiles while the solves stay cheap.
const BENCH_W: u32 = 4000;
const BENCH_H: u32 = 3000;
const BENCH_DABS: [(u32, u32, u32, u32); 2] = [(100, 100, 140, 140), (3860, 2860, 3900, 2900)];
/// The heal's padded windows' union, exclusive.
const BENCH_UNION: (u32, u32, u32, u32) = (99, 99, 3901, 2901);

/// Times a cancel `k` checkpoints into the checkpoint range `[start, end)`:
/// `c` is the longest gap in the worker's own uncancelled timeline over the
/// range, the probe cancels once the count reaches `start + k`, and the line
/// says whether the worker returned inside the range. Prints PASS/MISS
/// against `T_f + c` after `label`; asserts nothing about time.
fn bench_range<F, R>(
    label: &str,
    what: &str,
    range: (u64, u64),
    k: u64,
    verdict: &dyn Fn(f64, u32) -> bool,
    run: F,
) where
    F: Fn() -> R,
    R: FnOnce(usize) -> (bool, String) + Send,
{
    let (start, end) = range;
    let t = timeline(&|_, _, n| n >= start, &|_, _, n| n >= end, run());
    let (longest_at, longest) = t.longest().expect("a gap");
    let c = longest.as_secs_f64();
    let p = probe(&|_, _, n| n >= start, Trigger::After(k), verdict, run());
    let bound = RETURN_FLOOR_S + c;
    let inside = if (start..end).contains(&p.checkpoints) {
        "inside"
    } else {
        "outside"
    };
    eprintln!(
        "{label} cancel inside the {what} \
         [{start}, {end}): {:.3} ms cancel-to-return (issued at {:.1} ms; {}); returned {inside} \
         the range; c {:.3} ms (longest gap, after checkpoint {longest_at}); bound T_f + c = \
         {:.3} ms (T_f 1 ms); ≈{:.2} gaps past the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        c * 1e3,
        bound * 1e3,
        past(p.latency, c),
        common::probe::verdict(p.latency, bound),
    );
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn a_concurrently_cancelled_heal_returns_within_one_tile_of_its_union_read() {
    let doc = gradient(BENCH_W, BENCH_H);
    let at = &doc as *const _ as usize;
    let run = || {
        render(
            Export::InPlace,
            at,
            overlay_in(BENCH_W, BENCH_H, &BENCH_DABS),
            Vec::new(),
        )
    };
    let tiles = rect_tiles(BENCH_UNION);
    assert_eq!(tiles, 192);
    // The post-plan reserve, then one check per tile of the union read.
    let stage2 = entry_count(&|_, stage, _| stage == 2, run());
    let start = stage2 + 2;
    bench_range(
        &heal_label(),
        "heal union read",
        (start, start + tiles),
        tiles / 2,
        &|_, stage| stage == 2,
        run,
    );
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn a_concurrently_cancelled_spot_heal_returns_within_one_tile_of_its_union_read() {
    let doc = gradient(BENCH_W, BENCH_H);
    let at = &doc as *const _ as usize;
    let run = || {
        spot_render(
            at,
            overlay_in(BENCH_W, BENCH_H, &BENCH_DABS),
            (BENCH_W, BENCH_H),
        )
    };
    // Between the first two stage-3 0.18 reports: the read's tiles, then
    // the check after it.
    let opening: Vec<u64> = reports(run())
        .iter()
        .filter(|r| r.stage == 3 && (r.progress - 0.18).abs() <= 1e-6)
        .map(|r| r.checkpoints)
        .take(2)
        .collect();
    let (start, end) = (opening[0] + 1, opening[1] - 1);
    bench_range(
        &heal_label(),
        "spot-heal union read",
        (start, end),
        (end - start) / 2,
        &|_, stage| stage == 3,
        run,
    );
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn a_concurrently_cancelled_new_layer_heal_returns_within_one_poll_of_its_deposit() {
    let doc = gradient(BENCH_W, BENCH_H);
    let at = &doc as *const _ as usize;
    let pl = plate(&doc);
    let run = || {
        render(
            Export::NewLayer,
            at,
            overlay_in(BENCH_W, BENCH_H, &BENCH_DABS),
            pl.clone(),
        )
    };
    let (_, _, last4, stage5) = marks(&reports(run()));
    let n = u64::from(BENCH_W * BENCH_H);
    let rect = (100, 100, 3900, 2900);
    let write = rect_tiles(BENCH_UNION);
    // The temporary heal's final write, the deposit, the closing check.
    assert_eq!(stage5 - last4, 1 + write + deposit_checkpoints(n, rect));
    let tiles = rect_tiles(rect);
    let domain = n.div_ceil(4096);
    let lift = (u64::from(rect.2 - rect.0) * u64::from(rect.3 - rect.1)).div_ceil(4096);
    // The domain pass, the reservation, the healed read, the lift and the
    // write, in order; the transparent new layer is never read.
    let pass = last4 + 1 + write;
    let read = pass + domain + 1;
    let lifted = read + tiles;
    let written = lifted + lift;
    let steps = [
        ("deposit's domain pass", pass, pass + domain),
        ("deposit's rect read", read, read + tiles),
        ("deposit's lift", lifted, lifted + lift),
        ("deposit's write", written, written + tiles),
    ];
    for (what, start, end) in steps {
        bench_range(
            &heal_label(),
            what,
            (start, end),
            (end - start) / 2,
            &|_, stage| stage == 4,
            run,
        );
    }
}

/// The union-read and deposit probes' line prefix.
fn heal_label() -> String {
    format!("heal ({BENCH_W}×{BENCH_H}, two 40×40 dabs)")
}

/// `dr_doc_content_aware_fill_edit` (ring 0, seed 7, sample Current) over
/// the request box `(100, 100, 3800, 2800)` with `coverage`, placed by
/// `output`, on the document at `doc`; frees any result.
fn fill_render(
    doc: usize,
    output: &'static str,
    coverage: Vec<u8>,
) -> impl FnOnce(usize) -> (bool, String) + Send {
    move |token: usize| {
        let request = CString::new(format!(
            r#"{{"op":"content_aware_fill","layer":0,"x":100,"y":100,"width":3800,"height":2800,"ring":0,"seed":7,"sample":"current","output":"{output}"}}"#
        ))
        .unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_doc_content_aware_fill_edit(
                doc as *const DrDocument,
                request.as_ptr(),
                coverage.as_ptr(),
                token as *const DrJobControl,
                &mut err,
            )
        };
        let message = (!err.is_null()).then(|| take_err_string(err));
        let ok = !out.is_null();
        if ok {
            unsafe { dr_doc_free(out) };
        }
        (ok, message.unwrap_or_default())
    }
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn a_concurrently_cancelled_new_layer_fill_returns_within_one_poll_of_its_deposit() {
    // The byte Content-Aware Fill's new-layer arm lifts through the
    // same polled rect deposit; its four steps, timed as the heal
    // deposit is, on the bench canvas with the two dabs punched flat.
    let mask = mask_of(BENCH_W, BENCH_H, |x, y| {
        BENCH_DABS
            .iter()
            .any(|&(x0, y0, x1, y1)| (x0..x1).contains(&x) && (y0..y1).contains(&y))
    });
    let doc = punch(&gradient(BENCH_W, BENCH_H), &mask, [240, 20, 200]);
    let at = &doc as *const _ as usize;
    let run = || fill_render(at, "new_layer", mask.clone());
    let (_, _, last4, stage5) = marks(&reports(fill_render(at, "layer", mask.clone())));
    let kernel_tail = stage5 - last4 - 1;
    let (_, _, last4, stage5) = marks(&reports(run()));
    let n = u64::from(BENCH_W * BENCH_H);
    let rect = (100, 100, 3900, 2900);
    // The kernel's tail, the deposit, the closing check.
    assert_eq!(
        stage5 - last4 - kernel_tail - 1,
        deposit_checkpoints(n, rect)
    );
    let tiles = rect_tiles(rect);
    let domain = n.div_ceil(4096);
    let lift = (u64::from(rect.2 - rect.0) * u64::from(rect.3 - rect.1)).div_ceil(4096);
    // The domain pass, the reservation, the healed read, the lift and the
    // write, in order; the transparent new layer is never read.
    let pass = last4 + kernel_tail + 1;
    let read = pass + domain + 1;
    let lifted = read + tiles;
    let written = lifted + lift;
    let steps = [
        ("deposit's domain pass", pass, pass + domain),
        ("deposit's rect read", read, read + tiles),
        ("deposit's lift", lifted, lifted + lift),
        ("deposit's write", written, written + tiles),
    ];
    let label = format!("new-layer fill ({BENCH_W}×{BENCH_H}, two 40×40 dabs)");
    for (what, start, end) in steps {
        bench_range(
            &label,
            what,
            (start, end),
            (end - start) / 2,
            &|_, stage| stage == 4,
            run,
        );
    }
}
