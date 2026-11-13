//! Phase 13A-5c-2a: the retouch SAMPLING PLATE (`inpaint_source::
//! sampling_source`, reached through `dr_doc_retouch_source`). Since 13A-5c-2a
//! the tiled path admits every mask form, pointwise adjustments and opaque
//! layer metadata; since 13A-5c-2b groups and styles route through the byte
//! regional tree (`regional_sampling_tree_cancel.rs`), and only a guided
//! Shadows/Highlights keeps the full-frame fallback, which runs
//! `flattened_cut_with_job` under the sampling job, `checkpoints_only` — the
//! plate still reports no progress.
//!
//! Default tests (load-independent, 13A-5b-iii-b-1): the tiled plate equals
//! `dr_doc_flattened` byte for byte for every newly admitted form AND succeeds
//! under a scratch limit of `plate + 1 MiB + T + 256 KiB` (`T` this file's own
//! mirror of `tile_scratch_bytes`) — the ROUTE witness, since the fallback
//! would need `plate + 1 MiB + ≥ 16·W·H`; grouped, styled (both now the tree)
//! and guided (the fallback) plates equal the projection under a live job,
//! the guided fixture refuses under that same limit with
//! `DrJobControl::reserve`'s sentence, and a cut (Current and Below, ignore
//! adjustments) equals the projection of the stack without the cut entry;
//! cancels PARKED inside the tree's mask blur, inside the fallback's chain
//! (a guided fixture) and inside the tile loop return nothing and hold
//! nothing. The canvas is 2100×150: three tile columns (1024, 1024, 52) by
//! three tile rows (64, 64, 22), partial edges on both axes.
//!
//! DECLARED before measuring (the 13A-5a model, `regional`'s module doc;
//! `inpaint_source`'s module doc): a cancel issued inside the TILE LOOP returns
//! within `T_f + c_tile_row` — one row of one layer's tile composite (≤ 1024
//! samples; the Tiles arm's unit, unchanged); a cancel issued inside the
//! FALLBACK returns within `T_f + c_row` — one row of the running chain
//! primitive. `T_f` = 1 ms the return floor. Calibration (the `#[ignore]`d
//! bench, `make cancel-bench`): `T(n)` is the minimum of three uncancelled
//! plate renders of a fresh `n`-layer fixture on a 3000×2000 canvas — `n`
//! canvas-sized rasters inside an isolated group under a guided
//! Shadows/Highlights (fallback; 13A-5c-2b's fixture, whose T(n) carry the
//! guided adjustment's whole-canvas cost and so are not comparable with
//! 13A-5c-2a's bare group, now a tree plate) or `n` canvas-sized rasters
//! under aligned, unmodified masks (tiled, no derivation, so the difference
//! is compositing only); `c_row = (T(6) − T(1)) / (5·H)`, `c_tile_row =
//! (T(6) − T(1)) / (5·H·⌈W/1024⌉)`. The probe enters at half the uncancelled twin's
//! checkpoint count (past the preparation) and cancels one checkpoint later.
//! Misses are recorded, never widened.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_heal::dr_doc_retouch_source;
use doctored_core::ffi_jobs::*;
use doctored_core::job_control::DrJobControl;
use image::{GrayImage, Luma, Rgba, RgbaImage};

use crate::common;
use common::probe::*;
use common::{adjust_meta, img_pixels, opaque_pattern, styled, take_err_string};

use crate::resource_report;

const W: u32 = 2100;
const H: u32 = 150;
/// The bench canvas.
const BW: u32 = 3000;
const BH: u32 = 2000;
/// Layers of the measured bench fixtures.
const N: usize = 6;

const CURRENT_AND_BELOW: u32 = 1;
const ALL: u32 = 2;
const MIB: u64 = 1024 * 1024;
const LEVELS_PARAMS: &str = "{\"black\":0.1,\"white\":0.9,\"gamma\":1.8}";

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

/// The plate under a LIVE control, optionally scratch-limited; the control's
/// scratch must be released on return either way.
fn plate_with(
    doc: &DrDocument,
    idx: usize,
    sample: u32,
    ignore_adjustments: bool,
    limit: Option<u64>,
) -> Result<Vec<u8>, String> {
    let hd = handle(doc);
    let control = unsafe { dr_job_control_create() };
    if let Some(limit) = limit {
        assert!(unsafe { dr_job_control_set_scratch_limit(control, limit) });
    }
    let mut err: *mut c_char = ptr::null_mut();
    let img =
        unsafe { dr_doc_retouch_source(hd, idx, sample, ignore_adjustments, control, &mut err) };
    let scratch = unsafe { dr_job_control_scratch_bytes(control) };
    unsafe {
        dr_job_control_free(control);
        dr_doc_free(hd);
    }
    assert_eq!(scratch, 0, "the plate left scratch reserved");
    if img.is_null() {
        return Err(take_err_string(err));
    }
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    Ok(out)
}

/// The Sample All plate of a clone of `doc` on `token`, run once.
fn plate(doc: &DrDocument) -> impl FnOnce(usize) -> (bool, String) + Send {
    let address = handle(doc) as usize;
    move |token: usize| {
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_doc_retouch_source(
                address as *const DrDocument,
                0,
                ALL,
                false,
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

/// The checkpoint count of one uncancelled Sample All plate of `doc`.
fn total_checkpoints(doc: &DrDocument) -> u64 {
    let control = unsafe { dr_job_control_create() };
    let (ok, message) = plate(doc)(control as usize);
    assert!(ok, "{message}");
    let count = unsafe { &*control }.checkpoints();
    unsafe { dr_job_control_free(control) };
    count
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

// ------------------------------------------- the tile_scratch mirror --
//
// `MaskProperties::padded_dimensions`' reach, restated (the blur's
// decimation factor and kernel radius), copied from
// `regional_group_materialization_cancel.rs`.

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

/// A `(w, h)` mask's padded plane at `feather`.
fn padded(feather: f32, (w, h): (u32, u32)) -> (u32, u32) {
    let pad = blur_reach(0.3 * (feather - 1.0) + 0.8);
    (w + 2 * pad, h + 2 * pad)
}

/// This file's own `tile_scratch_bytes` (uncut): per visible, mask-enabled
/// layer whose mask needs derivation, the feathered padded plane at 12 bytes
/// per pixel plus the layer-sized aligned plane.
fn tile_scratch(doc: &DrDocument) -> u64 {
    let mut bytes = 0u64;
    for layer in &doc.layers {
        let Some(mask) = layer.mask.as_ref() else {
            continue;
        };
        if !layer.visible || !layer.mask_enabled {
            continue;
        }
        let p = &layer.mask_properties;
        if p.feather > 0.0
            || p.density != 1.0
            || p.offset != layer.offset
            || mask.dimensions() != layer.pixels.dimensions()
        {
            if p.feather > 0.0 {
                let (pw, ph) = padded(p.feather, mask.dimensions());
                bytes += u64::from(pw) * u64::from(ph) * 12;
            }
            bytes += u64::from(layer.pixels.width()) * u64::from(layer.pixels.height());
        }
    }
    bytes
}

/// Test 1's scratch limit: the plate, 1 MiB of bookkeeping, the mirror's `T`
/// and 256 KiB.
fn tiled_limit(doc: &DrDocument) -> u64 {
    u64::from(doc.width) * u64::from(doc.height) * 4 + MIB + tile_scratch(doc) + 256 * 1024
}

// ------------------------------------------------------------ fixtures --

/// Alpha 0 inside a grid of discs, an anti-aliased 4 px ramp, 255 between.
fn hole_alpha(x: u32, y: u32) -> f32 {
    let (dx, dy) = ((x % 97) as f32 - 48.0, (y % 83) as f32 - 41.0);
    ((dx * dx + dy * dy).sqrt() - 18.0) / 4.0
}

/// A raster with anti-aliased holes, colour varied by `seed`.
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

fn background(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(opaque_pattern(w, h))
}

fn top(doc: &DrDocument) -> usize {
    doc.layers.len() - 1
}

fn with_raster(
    doc: &DrDocument,
    pixels: RgbaImage,
    offset: (i32, i32),
    blend: BlendMode,
    opacity: f32,
) -> DrDocument {
    let mut out = doc
        .adding_image_layer(top(doc), pixels, "Raster")
        .expect("add layer");
    let at = top(&out);
    let layer = &mut out.layers[at];
    layer.offset = offset;
    layer.blend = blend;
    layer.opacity = opacity;
    out
}

/// An ENABLED mask on `idx` (fresh caches).
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

/// A canvas-sized transparent adjustment layer carrying `op` on top.
fn with_adjustment(doc: &DrDocument, op: &str, params: &str, opacity: f32) -> DrDocument {
    let mut out = doc
        .adding_image_layer(top(doc), RgbaImage::new(doc.width, doc.height), op)
        .expect("add layer");
    let at = top(&out);
    out.layers[at].meta = Some(adjust_meta(op, params));
    out.layers[at].opacity = opacity;
    out
}

fn drop_shadow(size: f32) -> String {
    format!("{{\"effects\":[{{\"type\":\"drop_shadow\",\"distance\":0,\"size\":{size}}}]}}")
}

fn sh_params(radius: f32) -> String {
    format!(
        "{{\"shadows\":{{\"amount\":0.7,\"tone\":0.9}},\"highlights\":{{\"amount\":0.35,\
         \"tone\":0.8}},\"radius\":{radius}}}"
    )
}

/// Test 1's nine tiled forms, each over an opaque Background and built anew
/// per call (cold caches).
fn tiled_case(case: char) -> DrDocument {
    let doc = background(W, H);
    match case {
        // (a) a feathered (6) mask on an OFFSET layer.
        'a' => {
            let doc = with_raster(
                &doc,
                holed(W - 80, H - 30, 1),
                (37, 11),
                BlendMode::Normal,
                1.0,
            );
            let at = top(&doc);
            masked(&doc, at, gradient(W - 80, H - 30), (37, 11), 1.0, 6.0)
        }
        // (b) density 0.4.
        'b' => {
            let doc = with_raster(&doc, holed(W, H, 2), (0, 0), BlendMode::Normal, 1.0);
            let at = top(&doc);
            masked(&doc, at, gradient(W, H), (0, 0), 0.4, 0.0)
        }
        // (c) the mask's offset differs from its layer's.
        'c' => {
            let doc = with_raster(&doc, holed(W, H, 3), (0, 0), BlendMode::Normal, 1.0);
            let at = top(&doc);
            masked(&doc, at, gradient(W, H), (13, -7), 1.0, 0.0)
        }
        // (d) the mask's dimensions differ from its layer's.
        'd' => {
            let doc = with_raster(&doc, holed(W, H, 4), (0, 0), BlendMode::Normal, 1.0);
            let at = top(&doc);
            masked(&doc, at, gradient(W / 2, H / 2), (0, 0), 1.0, 0.0)
        }
        // (e) outside coverage 255 plus a feather (4).
        'e' => {
            let doc = with_raster(&doc, holed(W, H, 5), (0, 0), BlendMode::Normal, 1.0);
            let at = top(&doc);
            let mut doc = masked(&doc, at, gradient(W / 3, H / 2), (100, 20), 1.0, 4.0);
            doc.layers[at].mask_properties.outside_coverage = 255;
            doc
        }
        // (f) a pointwise Levels at opacity 0.7 under a feathered (3) mask.
        'f' => {
            let doc = with_raster(&doc, holed(W, H, 6), (0, 0), BlendMode::Normal, 0.9);
            let doc = with_adjustment(&doc, "levels", LEVELS_PARAMS, 0.7);
            let at = top(&doc);
            masked(&doc, at, gradient(W, H), (0, 0), 1.0, 3.0)
        }
        // (g) metadata `from_meta` rejects, on a raster.
        'g' => {
            let mut doc = with_raster(&doc, holed(W, H, 7), (0, 0), BlendMode::Multiply, 0.8);
            let at = top(&doc);
            doc.layers[at].meta = Some("{\"type\":\"note\"}".into());
            doc
        }
        // (h) a clip run over a masked (feather 3) base.
        'h' => {
            let doc = with_raster(&doc, holed(W - 40, H, 8), (20, 0), BlendMode::Normal, 1.0);
            let base = top(&doc);
            let doc = masked(&doc, base, gradient(W - 40, H), (20, 0), 1.0, 3.0);
            let mut doc = with_raster(&doc, holed(W, H, 9), (0, 0), BlendMode::Screen, 0.8);
            let member = top(&doc);
            doc.layers[member].clipped = true;
            let mut doc = with_adjustment(&doc, "levels", LEVELS_PARAMS, 1.0);
            let member = top(&doc);
            doc.layers[member].clipped = true;
            doc
        }
        // (i) Dissolve at opacity 0.5.
        'i' => with_raster(&doc, holed(W, H, 10), (0, 0), BlendMode::Dissolve, 0.5),
        _ => unreachable!(),
    }
}

/// (2a): an isolated (Normal) group holding a raster under an enabled
/// canvas-sized mask at `feather` and a second raster. Children 1 and 2, the
/// group 3.
fn group_masked(feather: f32) -> DrDocument {
    let doc = background(W, H);
    let doc = with_raster(&doc, holed(W, H, 1), (0, 0), BlendMode::Normal, 0.9);
    let doc = masked(&doc, 1, gradient(W, H), (0, 0), 1.0, feather);
    let doc = with_raster(&doc, holed(W, H, 2), (0, 0), BlendMode::Multiply, 0.8);
    let (mut doc, g, ..) = doc.group_layers(&[1, 2], "Group").expect("group");
    doc.layers[g].blend = BlendMode::Normal;
    doc
}

/// (2b): a drop shadow (size 4) on a raster.
fn shadowed() -> DrDocument {
    let doc = with_raster(
        &background(W, H),
        holed(W - 200, H - 40, 3),
        (100, 20),
        BlendMode::Normal,
        1.0,
    );
    styled(&doc, 1, &drop_shadow(4.0))
}

/// (2c): a guided Shadows/Highlights (radius 8) at opacity 0.8.
fn guided() -> DrDocument {
    let doc = with_raster(
        &background(W, H),
        holed(W, H, 4),
        (0, 0),
        BlendMode::Normal,
        0.9,
    );
    with_adjustment(&doc, "shadows_highlights", &sh_params(8.0), 0.8)
}

/// `n` canvas-sized rasters inside an isolated (Normal) group over a
/// Background (a tree plate alone; the fallback bench wraps it in
/// [`guided_over`]).
fn bench_group(n: usize) -> DrDocument {
    let mut doc = background(BW, BH);
    for i in 0..n {
        doc = with_raster(
            &doc,
            holed(BW, BH, i as u32),
            (0, 0),
            BlendMode::Normal,
            0.9,
        );
    }
    let children: Vec<usize> = (1..=n).collect();
    let (mut doc, g, ..) = doc.group_layers(&children, "Group").expect("group");
    doc.layers[g].blend = BlendMode::Normal;
    doc
}

/// `doc` under a guided Shadows/Highlights (radius 8) at depth 0 on top: the
/// full-frame fallback's route (the regional tree's one staged operator).
fn guided_over(doc: &DrDocument) -> DrDocument {
    with_adjustment(doc, "shadows_highlights", &sh_params(8.0), 0.8)
}

/// The bench's tiled fixture: `n` canvas-sized rasters, each under an
/// aligned, unmodified mask (no derivation), over a Background.
fn bench_masked(n: usize) -> DrDocument {
    let mut doc = background(BW, BH);
    for i in 0..n {
        doc = with_raster(
            &doc,
            holed(BW, BH, i as u32),
            (0, 0),
            BlendMode::Normal,
            0.9,
        );
        let at = top(&doc);
        doc = masked(&doc, at, gradient(BW, BH), (0, 0), 1.0, 0.0);
    }
    doc
}

// ------------------------------------------- 1. tiled exactness --

#[test]
fn every_newly_tiled_form_equals_the_projection_within_the_tiled_reservation() {
    for case in ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i'] {
        let reference = full(&tiled_case(case));
        let live = plate_with(&tiled_case(case), 0, ALL, false, None)
            .unwrap_or_else(|e| panic!("({case}): {e}"));
        assert!(live == reference, "({case}) differs from the projection");
        let doc = tiled_case(case);
        let limit = tiled_limit(&doc);
        let limited = plate_with(&doc, 0, ALL, false, Some(limit)).unwrap_or_else(|e| {
            panic!(
                "({case}) refused under the tiled limit {limit} (T {}): {e}",
                tile_scratch(&doc)
            )
        });
        assert!(limited == reference, "({case}) under the limit differs");
    }
}

// ------------------------------------ 2. tree / fallback exactness --

#[test]
fn grouped_and_styled_plates_equal_the_projection_under_a_live_job() {
    for (what, build) in [
        (
            "an isolated group with a feathered child mask",
            &(|| group_masked(6.0)) as &dyn Fn() -> DrDocument,
        ),
        ("a drop shadow", &shadowed),
        ("a guided Shadows/Highlights", &guided),
    ] {
        let reference = full(&build());
        let live =
            plate_with(&build(), 0, ALL, false, None).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(live == reference, "{what} differs from the projection");
    }
}

#[test]
fn the_guided_fallback_refuses_under_the_tiled_limit() {
    let doc = guided();
    let limit = tiled_limit(&doc);
    let Err(failure) = plate_with(&doc, 0, ALL, false, Some(limit)) else {
        panic!("the full-frame fallback fits under the tiled limit");
    };
    assert!(
        failure.contains("working-memory limit"),
        "not DrJobControl::reserve's sentence: {failure}"
    );
}

#[test]
fn a_cut_levels_entry_equals_the_projection_without_it() {
    // A drop-shadowed raster routes the cut stack through the regional tree
    // (13A-5c-2b); the Levels on top is unclipped, depth 0.
    let doc = with_adjustment(&shadowed(), "levels", LEVELS_PARAMS, 1.0);
    let levels = top(&doc);
    let reference = full(&doc.remove_layers(&[levels]).expect("remove"));
    let below = plate_with(&doc, 1, CURRENT_AND_BELOW, false, None).expect("current and below");
    assert!(below == reference, "Current and Below differs");
    let ignored = plate_with(&doc, 1, ALL, true, None).expect("ignore adjustments");
    assert!(ignored == reference, "ignore adjustments differs");
}

// --------------------------------------------- 3. parked cancels --

#[test]
fn a_cancel_parked_inside_the_tree_mask_blur_returns_nothing_and_caches_nothing() {
    let n0 = entry_count(&at_the_blur, plate(&group_masked(6.0)));
    let doc = group_masked(6.0);
    let p = cancelled_parked_at(
        n0 + 1,
        &|progress, stage| progress == 0.0 && stage == 6,
        plate(&doc),
    );
    eprintln!(
        "13A-5c-2b parked cancel inside the tree's child mask blur: {}",
        parked_line(&p)
    );
    assert_eq!(mask_cache_bytes(&doc), 0, "a cancelled blur cached a plane");
    let reference = full(&group_masked(6.0));
    let again = plate_with(&doc, 0, ALL, false, None).expect("re-run");
    assert!(again == reference, "the re-run differs from the projection");
    // The cache is `Arc`-shared across clones, so the completed re-run's
    // plane is visible here: the zero above was not vacuous.
    assert!(
        mask_cache_bytes(&doc) > 0,
        "the completed re-run cached no plane"
    );
}

#[test]
fn a_cancel_parked_inside_the_fallback_chain_returns_nothing() {
    let total = total_checkpoints(&guided_over(&group_masked(0.0)));
    // The chain polls at least once per canvas row of each of its two child
    // composites, so half the total lies well past the handful of
    // checkpoints before the render (an unpolled chain would total a few).
    assert!(
        total >= 2 * u64::from(H),
        "the fallback polled only {total} times: the chain is not under the job"
    );
    let p = cancelled_parked_at(
        total / 2,
        &|progress, _| progress == 0.0,
        plate(&guided_over(&group_masked(0.0))),
    );
    eprintln!(
        "13A-5c-2a parked cancel inside the fallback's chain (of {total}): {}",
        parked_line(&p)
    );
}

#[test]
fn a_cancel_parked_inside_the_tile_loop_returns_nothing() {
    let total = total_checkpoints(&tiled_case('i'));
    // Each tile's layer composites poll per tile row under the live job, so
    // the total is at least one per canvas row of each of the two layers
    // (a loop polling only per tile would total about a dozen).
    assert!(
        total >= 2 * u64::from(H),
        "the tile loop polled only {total} times: its composites are not under the job"
    );
    let p = cancelled_parked_at(
        total / 2,
        &|progress, _| progress == 0.0,
        plate(&tiled_case('i')),
    );
    eprintln!(
        "13A-5c-2a parked cancel inside the tile loop (of {total}): {}",
        parked_line(&p)
    );
}

// ------------------------------------------------ bench (cancel-bench) --

/// `T(n)`: the minimum of three uncancelled plate renders of a fresh fixture.
fn calibrated(build: &dyn Fn() -> DrDocument) -> Duration {
    (0..3)
        .map(|_| {
            let doc = build();
            let control = unsafe { dr_job_control_create() };
            let render = plate(&doc);
            let start = Instant::now();
            let (ok, message) = render(control as usize);
            let elapsed = start.elapsed();
            unsafe { dr_job_control_free(control) };
            assert!(ok, "{message}");
            elapsed
        })
        .min()
        .unwrap()
}

/// One measured probe line. `run` names the run whose fixture `build` makes,
/// so the line is read against that run's record only: the fallback's
/// fixture is 13A-5c-2b's (`n` rasters in an isolated group under a guided
/// Shadows/Highlights), not 13A-5c-2a's bare group.
fn bench(
    run: &str,
    what: &str,
    fixture: &str,
    bound_name: &str,
    rows: f64,
    build: &dyn Fn(usize) -> DrDocument,
) {
    let t1 = calibrated(&|| build(1));
    let t6 = calibrated(&|| build(N));
    let c = (t6.saturating_sub(t1)).as_secs_f64() / ((N - 1) as f64 * rows);
    let half = total_checkpoints(&build(N)) / 2;
    let p = probe(
        &|_, _, n| n >= half,
        Trigger::After(1),
        &|progress, _| progress == 0.0,
        plate(&build(N)),
    );
    let bound = RETURN_FLOOR_S + c;
    eprintln!(
        "{run} byte cancel inside the {what} ({BW}×{BH}, {N} layers {fixture}): {:.3} ms \
         cancel-to-return (issued at {:.1} ms; {}); T(1) {:.1} ms, T({N}) {:.1} ms, {bound_name} \
         {:.4} ms; bound T_f + {bound_name} = {:.3} ms (T_f 1 ms); ≈{:.2} units past the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        t1.as_secs_f64() * 1e3,
        t6.as_secs_f64() * 1e3,
        c * 1e3,
        bound * 1e3,
        past(p.latency, c),
        verdict(p.latency, bound),
    );
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5c-2a)"]
fn a_concurrently_cancelled_fallback_returns_within_one_chain_row() {
    bench(
        "13A-5c-2b",
        "full-frame fallback",
        "in an isolated group under a guided Shadows/Highlights",
        "c_row",
        f64::from(BH),
        &|n| guided_over(&bench_group(n)),
    );
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5c-2a)"]
fn a_concurrently_cancelled_tile_loop_returns_within_one_tile_row() {
    let columns = f64::from(BW.div_ceil(1024));
    bench(
        "13A-5c-2a",
        "tile loop",
        "under aligned masks",
        "c_tile_row",
        f64::from(BH) * columns,
        &bench_masked,
    );
}
