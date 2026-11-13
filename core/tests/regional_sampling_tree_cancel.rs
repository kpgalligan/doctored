//! Phase 13A-5c-2b: the retouch SAMPLING PLATE (`inpaint_source::
//! sampling_source`, reached through `dr_doc_retouch_source`) for stacks
//! holding groups and layer styles routes through the byte REGIONAL tree
//! (`doc_regional_plate::regional_cut_plate`): admitted once over the canvas
//! with the cut, resolved once over the canvas clip and sampled in ≤ 1024×64
//! tiles, so no canvas-sized f32 accumulator is ever allocated.
//!
//! Default tests (load-independent, 13A-5b-iii-b-1): every grouped / styled
//! Sample All plate equals `dr_doc_flattened` byte for byte AND succeeds under
//! the ROUTE WITNESS limit `L = plan.scratch_bytes − 16·n + 20·tile + 1 MiB +
//! 256 KiB` (`plan` the full-canvas regional plan at divisor 1, `n = W·H`,
//! `tile = 1024·64`, 1 MiB `ffi_heal`'s bookkeeping), which the full-frame
//! fallback cannot meet (it needs ≥ `projection_scratch_bytes + 1 MiB`, ≥
//! 76·n for a group); cut plates (Current and Below, ignore adjustments) equal
//! `DrDocument::retouch_plate_full_frame`; cancels PARKED inside the tree's
//! preparation and inside its tile loop, both under `L`, return nothing and
//! hold nothing; the 16-bit promotion stays inside its own plan, including on
//! canvases where `20·tile > 16·n`. The canvas is 2100×150: three tile columns
//! (1024, 1024, 52) by three tile rows (64, 64, 22).
//!
//! DECLARED before measuring (`inpaint_source`'s module doc): a cancel issued
//! inside the tree's TILE LOOP returns within `T_f + c_tree_row` — one full
//! tile row of ROOT samples through the WHOLE tree (one run-major run since
//! 13A-latency, ≤ 1024 samples). `T_f` = 1 ms the return floor. Calibration (the `#[ignore]`d
//! bench, `make cancel-bench`): `T(n)` is the minimum of three uncancelled
//! plate renders of a fresh fixture — `n` canvas-sized rasters inside an
//! isolated group on a 3000×2000 canvas — and `c_tree_row = 1024 · T(6) /
//! (BW·BH)`, `T(6)`'s per-root-sample cost times one full tile row (its
//! preparation, encode and copy only add slack). The probe enters at half the
//! uncancelled twin's checkpoint count and cancels one checkpoint later.
//! Misses are recorded, never widened.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use doctored_core::doc::{BlendMode, DrDocument, LayerKind, LayerRaster};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_heal::dr_doc_retouch_source;
use doctored_core::ffi_jobs::*;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retouch_sampling::RetouchSampling;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::Value;

use crate::common;
use common::probe::*;
use common::{adjust_meta, img_pixels, opaque_pattern, styled, take_err_string};

use crate::region_plan;
use crate::resource_report;

const W: u32 = 2100;
const H: u32 = 150;
/// The bench canvas.
const BW: u32 = 3000;
const BH: u32 = 2000;
/// Layers of the measured bench fixture.
const N: usize = 6;
/// The plate's tile (`doc_regional_plate::tile_pixels` on this canvas).
const TILE: u64 = 1024 * 64;

const CURRENT_AND_BELOW: u32 = 1;
const ALL: u32 = 2;
const MIB: u64 = 1024 * 1024;
const LEVELS_PARAMS: &str = "{\"black\":0.1,\"white\":0.9,\"gamma\":1.8}";
const BLEND_IF: &str =
    "\"blend_if\":{\"channel\":\"gray\",\"this_layer\":[10,60,180,240],\"underlying\":[0,40,200,255]}";

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

/// [`plate`] with the token's scratch limited to `limit` first, so the route
/// is witnessed: under the route-witness limit only the tree fits.
fn plate_limited(doc: &DrDocument, limit: u64) -> impl FnOnce(usize) -> (bool, String) + Send {
    let render = plate(doc);
    move |token: usize| {
        assert!(unsafe { dr_job_control_set_scratch_limit(token as *const DrJobControl, limit) });
        render(token)
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

/// The full-canvas regional plan at divisor 1.
fn plan(doc: &DrDocument) -> Value {
    region_plan::byte(doc, (0, 0, doc.width, doc.height), 1, u64::MAX)
        .unwrap_or_else(|e| panic!("plan: {e}"))
}

/// The ROUTE WITNESS limit `L = plan.scratch_bytes − 16·n + 20·tile + 1 MiB +
/// 256 KiB`.
fn route_limit(doc: &DrDocument) -> u64 {
    let n = u64::from(doc.width) * u64::from(doc.height);
    plan(doc)["scratch_heap_bytes"].as_u64().unwrap() - 16 * n + 20 * TILE + MIB + 256 * 1024
}

/// The resource report's `projection_scratch_estimate_bytes` — the public
/// face of `render_scratch::projection_scratch_bytes`.
fn projection_scratch(doc: &DrDocument) -> u64 {
    let hd = handle(doc);
    let ptrs = [hd as *const DrDocument];
    let report = resource_report::report(&ptrs, &[], &[], &[], 0, 0);
    unsafe { dr_doc_free(hd) };
    let report = report.expect("a resource report");
    report["projection_scratch_estimate_bytes"]
        .as_u64()
        .unwrap()
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

/// A Levels entry directly above `idx` at `idx`'s own level (inside `idx`'s
/// enclosing group).
fn levels_above(doc: &DrDocument, idx: usize) -> DrDocument {
    let mut out = doc
        .adding_image_layer(idx, RgbaImage::new(doc.width, doc.height), "levels")
        .expect("add layer");
    let at = (idx + 1..out.layers.len())
        .find(|&i| out.layers[i].depth == doc.layers[idx].depth)
        .expect("the inserted entry");
    out.layers[at].meta = Some(adjust_meta("levels", LEVELS_PARAMS));
    out
}

/// `group_layers` with the group's blend set.
fn grouped(doc: &DrDocument, idx: &[usize], blend: BlendMode) -> (DrDocument, usize) {
    let (mut doc, g, ..) = doc.group_layers(idx, "Group").expect("group");
    doc.layers[g].blend = blend;
    (doc, g)
}

/// An empty group entry on top (the `regional_groups` fixture).
fn with_empty_group(doc: &DrDocument, blend: BlendMode) -> DrDocument {
    let mut out = doc.clone();
    let mut entry = doc.layers[0].clone();
    entry.kind = LayerKind::Group;
    entry.name = "Empty".into();
    entry.pixels = LayerRaster::new(RgbaImage::new(1, 1));
    entry.offset = (0, 0);
    entry.blend = blend;
    entry.opacity = 1.0;
    entry.visible = true;
    entry.mask = None;
    entry.meta = None;
    entry.style = None;
    entry.clipped = false;
    entry.depth = 0;
    out.layers.push(entry);
    out
}

/// A satin of `size` at distance 1: since #2 part 3 the box-pair law's
/// style plane, whose rows report at stage 6 as the Gaussian's did (the drop
/// shadow these cases carried runs the law since #2).
fn satin(size: f32) -> String {
    format!("{{\"effects\":[{{\"type\":\"satin\",\"distance\":1,\"size\":{size}}}]}}")
}

/// (a) / (a0): an isolated (Normal) group holding a raster under an enabled
/// canvas-sized mask at `feather` and a second raster. Children 1 and 2, the
/// group 3.
fn group_masked(feather: f32) -> DrDocument {
    let doc = background(W, H);
    let doc = with_raster(&doc, holed(W, H, 1), (0, 0), BlendMode::Normal, 0.9);
    let doc = masked(&doc, 1, gradient(W, H), (0, 0), 1.0, feather);
    let doc = with_raster(&doc, holed(W, H, 2), (0, 0), BlendMode::Multiply, 0.8);
    grouped(&doc, &[1, 2], BlendMode::Normal).0
}

/// Test 1's tree forms, each built anew per call (cold caches), with the
/// index of the group whose first child test 2 targets.
fn tree_case(case: &str) -> (DrDocument, usize) {
    let doc = background(W, H);
    match case {
        "a" => (group_masked(6.0), 3),
        "a0" => (group_masked(0.0), 3),
        // (b) a pass-through group at opacity 0.7 under an enabled feathered
        // (3) mask, holding a Multiply raster and a pointwise Levels.
        "b" => {
            let doc = with_raster(&doc, holed(W, H, 2), (0, 0), BlendMode::Multiply, 1.0);
            let doc = with_adjustment(&doc, "levels", LEVELS_PARAMS, 1.0);
            let (mut doc, g) = grouped(&doc, &[1, 2], BlendMode::PassThrough);
            doc.layers[g].opacity = 0.7;
            (masked(&doc, g, gradient(W, H), (0, 0), 1.0, 3.0), g)
        }
        // (c) isolated ⊂ pass-through ⊂ isolated (depth 3), a clip run
        // (raster base, Screen member, Levels member) inside.
        "c" => {
            let doc = with_raster(&doc, holed(W - 40, H, 8), (20, 0), BlendMode::Normal, 1.0);
            let mut doc = with_raster(&doc, holed(W, H, 9), (0, 0), BlendMode::Screen, 0.8);
            doc.layers[2].clipped = true;
            let mut doc = with_adjustment(&doc, "levels", LEVELS_PARAMS, 1.0);
            doc.layers[3].clipped = true;
            let (doc, inner) = grouped(&doc, &[1, 2, 3], BlendMode::Normal);
            let (doc, middle) = grouped(&doc, &[inner], BlendMode::PassThrough);
            grouped(&doc, &[middle], BlendMode::Normal)
        }
        // (d) a raster with drop shadow + outer glow, fill opacity 0.6 and a
        // Blend If.
        "d" => {
            let doc = with_raster(
                &doc,
                holed(W - 200, H - 40, 3),
                (100, 20),
                BlendMode::Normal,
                1.0,
            );
            let style = format!(
                "{{\"fill_opacity\":0.6,{BLEND_IF},\"effects\":[{{\"type\":\"drop_shadow\",\
                 \"distance\":0,\"size\":4}},{{\"type\":\"outer_glow\",\"size\":3}}]}}"
            );
            (styled(&doc, 1, &style), 1)
        }
        // (e) a styled GROUP (satin size 4) whose extent straddles
        // x = 1024 and y = 64.
        "e" => {
            let doc = with_raster(&doc, holed(400, 60, 11), (900, 40), BlendMode::Normal, 1.0);
            let doc = with_raster(
                &doc,
                holed(300, 50, 12),
                (1000, 70),
                BlendMode::Multiply,
                0.9,
            );
            let (doc, g) = grouped(&doc, &[1, 2], BlendMode::Normal);
            (styled(&doc, g, &satin(4.0)), g)
        }
        // (f) knockout: entry 0 Background-qualifying, a Deep-knockout styled
        // raster at depth 0, a Shallow one inside a pass-through group.
        "f" => {
            let doc = with_raster(&doc, holed(W, H, 13), (0, 0), BlendMode::Normal, 1.0);
            let doc = styled(&doc, 1, "{\"fill_opacity\":0.4,\"knockout\":\"deep\"}");
            let doc = with_raster(&doc, holed(W, H, 14), (0, 0), BlendMode::Normal, 0.9);
            let doc = with_raster(
                &doc,
                holed(W - 100, H - 20, 15),
                (50, 10),
                BlendMode::Normal,
                1.0,
            );
            let doc = styled(&doc, 3, "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}");
            grouped(&doc, &[2, 3], BlendMode::PassThrough)
        }
        // (g) a styled group nested inside a styled group.
        "g" => {
            let doc = with_raster(&doc, holed(600, 90, 16), (700, 30), BlendMode::Normal, 1.0);
            let doc = with_raster(&doc, holed(500, 80, 17), (1100, 50), BlendMode::Screen, 0.8);
            let (doc, inner) = grouped(&doc, &[1, 2], BlendMode::Normal);
            let doc = with_raster(&doc, holed(300, 60, 18), (400, 10), BlendMode::Normal, 0.9);
            let (doc, outer) = grouped(&doc, &[inner, inner + 1], BlendMode::Normal);
            let doc = styled(&doc, inner, &satin(3.0));
            let doc = styled(
                &doc,
                outer,
                "{\"effects\":[{\"type\":\"outer_glow\",\"size\":4,\"color\":\"#40ff80\"}]}",
            );
            (doc, outer)
        }
        // (h) a group as clip base with two members, a group as a clip
        // member.
        "h" => {
            let doc = with_raster(
                &doc,
                holed(W - 60, H - 20, 19),
                (30, 10),
                BlendMode::Normal,
                1.0,
            );
            let doc = with_raster(&doc, holed(W, H, 20), (0, 0), BlendMode::Multiply, 0.8);
            let (doc, base) = grouped(&doc, &[1, 2], BlendMode::PassThrough);
            let mut doc = with_raster(&doc, holed(W, H, 21), (0, 0), BlendMode::Screen, 0.8);
            doc.layers[base + 1].clipped = true;
            let doc = with_raster(&doc, holed(W / 2, H, 22), (300, 0), BlendMode::Normal, 1.0);
            let doc = with_raster(&doc, holed(W / 2, H, 23), (900, 0), BlendMode::Overlay, 0.7);
            let (mut doc, member) = grouped(&doc, &[base + 2, base + 3], BlendMode::Normal);
            doc.layers[member].clipped = true;
            (doc, base)
        }
        // (i) an empty group and a hidden group beside (a).
        "i" => {
            let doc = with_empty_group(&group_masked(6.0), BlendMode::Multiply);
            let doc = with_raster(&doc, holed(W, H, 24), (0, 0), BlendMode::Normal, 1.0);
            let doc = with_raster(&doc, holed(W, H, 25), (0, 0), BlendMode::Screen, 1.0);
            let (mut doc, hidden) = grouped(&doc, &[5, 6], BlendMode::Normal);
            doc.layers[hidden].visible = false;
            (doc, 3)
        }
        // (j) Dissolve at 0.5 inside a group.
        "j" => {
            let doc = with_raster(&doc, holed(W, H, 10), (0, 0), BlendMode::Dissolve, 0.5);
            let doc = with_raster(&doc, holed(W, H, 26), (0, 0), BlendMode::Normal, 0.6);
            grouped(&doc, &[1, 2], BlendMode::Normal)
        }
        _ => unreachable!(),
    }
}

/// The bench's fixture: `n` canvas-sized rasters inside an isolated (Normal)
/// group over a Background (the fallback suite's `bench_group`).
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
    grouped(&doc, &children, BlendMode::Normal).0
}

// ------------------------------------------------ 1. tree exactness --

#[test]
fn every_tree_form_equals_the_projection_within_the_route_witness_limit() {
    for case in ["a", "a0", "b", "c", "d", "e", "f", "g", "h", "i", "j"] {
        let reference = full(&tree_case(case).0);
        let live = plate_with(&tree_case(case).0, 0, ALL, false, None)
            .unwrap_or_else(|e| panic!("({case}): {e}"));
        assert!(live == reference, "({case}) differs from the projection");
        let doc = tree_case(case).0;
        let limit = route_limit(&doc);
        let limited = plate_with(&doc, 0, ALL, false, Some(limit))
            .unwrap_or_else(|e| panic!("({case}) refused under the route limit {limit}: {e}"));
        assert!(limited == reference, "({case}) under the limit differs");
    }
}

// ---------------------------------------------------------- 2. cuts --

#[test]
fn cut_tree_plates_equal_the_full_frame_cut() {
    for case in ["b", "c", "e", "h"] {
        let (doc, g) = tree_case(case);
        // The group's first child: the lowest index of its subtree.
        let depth = doc.layers[g].depth;
        let first = (0..g)
            .rev()
            .take_while(|&i| doc.layers[i].depth > depth)
            .last()
            .expect("a child");
        // A Levels inside the group (above its top child) and one on top.
        let doc = levels_above(&doc, g - 1);
        let doc = with_adjustment(&doc, "levels", LEVELS_PARAMS, 1.0);
        for (mode, code, ignore) in [
            (RetouchSampling::CurrentAndBelow, CURRENT_AND_BELOW, false),
            (RetouchSampling::All, ALL, true),
        ] {
            let reference = doc
                .retouch_plate_full_frame(first, mode, ignore)
                .unwrap_or_else(|e| panic!("({case}) reference: {e}"))
                .expect("a composite plate")
                .into_raw();
            // Under the route-witness limit, so the comparison witnesses the
            // Tree route (the fallback needs more).
            let live = plate_with(&doc, first, code, ignore, Some(route_limit(&doc)))
                .unwrap_or_else(|e| panic!("({case}) {mode:?}: {e}"));
            assert!(
                live == reference,
                "({case}) {mode:?} ignore {ignore} differs from the full-frame cut"
            );
        }
    }
}

// --------------------------------------------- 3. parked cancels --

#[test]
fn a_cancel_parked_inside_the_tree_preparation_returns_nothing() {
    let doc = tree_case("e").0;
    let limit = route_limit(&doc);
    let n0 = entry_count(&at_the_blur, plate_limited(&tree_case("e").0, limit));
    let p = cancelled_parked_at(
        n0 + 1,
        &|progress, stage| progress == 0.0 && stage == 6,
        plate_limited(&doc, limit),
    );
    eprintln!(
        "13A-5c-2b parked cancel inside the tree's preparation (styled group): {}",
        parked_line(&p)
    );
    let reference = full(&tree_case("e").0);
    let again = plate_with(&doc, 0, ALL, false, Some(limit)).expect("re-run");
    assert!(again == reference, "the re-run differs from the projection");
}

#[test]
fn a_cancel_parked_inside_the_tree_tile_loop_returns_nothing() {
    let doc = tree_case("a0").0;
    let total = total_checkpoints(&doc);
    // The tile loop alone polls once per tile row, `⌈W/1024⌉·H = 3·H`, before
    // any encode-row poll; with no preparation blur, half the total lies in
    // the loop.
    assert!(
        total >= 3 * u64::from(H),
        "the tree plate polled only {total} times: the tile loop is not under the job"
    );
    let limit = route_limit(&doc);
    let p = cancelled_parked_at(
        total / 2,
        &|progress, _| progress == 0.0,
        plate_limited(&doc, limit),
    );
    eprintln!(
        "13A-5c-2b parked cancel inside the tree's tile loop (of {total}): {}",
        parked_line(&p)
    );
}

// ------------------------------------------------ 5. promotion pin --

fn promotes_within_its_plan(what: &str, doc: &DrDocument) {
    let plan = doc.promote_flattened_plan().unwrap();
    let control = DrJobControl::default();
    // Every `reserve` is checked against this limit, so success is the
    // witness that no render-time reservation exceeds the plan.
    assert!(unsafe { dr_job_control_set_scratch_limit(&control, plan.scratch_bytes) });
    let promoted = doc.promote_flattened(REFERENCE_WORKING_BYTES, 0, Some(&control));
    assert!(promoted.is_ok(), "{what}: {:?}", promoted.err());
    assert_eq!(control.scratch_bytes(), 0, "{what}: scratch left reserved");
}

/// (t1): a 10×10 raster with an effect-less style (fill opacity 0.5) at
/// (300, 15) over a 600×40 Background — `20·tile = 20·n > 16·n`.
fn small_styled() -> DrDocument {
    let doc = with_raster(
        &background(600, 40),
        holed(10, 10, 27),
        (300, 15),
        BlendMode::Normal,
        1.0,
    );
    styled(&doc, 1, "{\"fill_opacity\":0.5}")
}

/// (t2): an isolated Normal group of two 10×10 rasters over a 600×40
/// Background — a covering pin only; (t1) witnesses the `(20·tile − 16·n)⁺`
/// term.
fn small_group() -> DrDocument {
    let doc = with_raster(
        &background(600, 40),
        holed(10, 10, 28),
        (300, 15),
        BlendMode::Normal,
        1.0,
    );
    let doc = with_raster(&doc, holed(10, 10, 29), (305, 20), BlendMode::Multiply, 0.8);
    grouped(&doc, &[1, 2], BlendMode::Normal).0
}

#[test]
fn the_promotion_stays_inside_its_plan_through_the_tree() {
    promotes_within_its_plan("(a)", &tree_case("a").0);
    promotes_within_its_plan("(e)", &tree_case("e").0);
    promotes_within_its_plan("(t1)", &small_styled());
    promotes_within_its_plan("(t2)", &small_group());
    // The plan arm's new term is visible on (t1): `(20·tile − 16·n)⁺ = 4·n`
    // above the projection estimate plus the plan's four bookkeeping units
    // (4 MiB; (t1) carries no metadata, so context and metadata parse are 0).
    let doc = small_styled();
    let n = 600 * 40;
    let plan = doc.promote_flattened_plan().unwrap();
    assert!(
        plan.scratch_bytes >= projection_scratch(&doc) + 4 * n + 4 * MIB,
        "the promotion plan {} misses the tile term over the projection {}",
        plan.scratch_bytes,
        projection_scratch(&doc)
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

fn uptime() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("uptime unavailable: {e}"))
}

/// The Tree calibration: `c_tree_row = 1024 · T(6) / (BW·BH)`.
fn bench_tree(build: &dyn Fn(usize) -> DrDocument) {
    let t1 = calibrated(&|| build(1));
    let t6 = calibrated(&|| build(N));
    let c = 1024.0 * t6.as_secs_f64() / (f64::from(BW) * f64::from(BH));
    let half = total_checkpoints(&build(N)) / 2;
    let p = probe(
        &|_, _, n| n >= half,
        Trigger::After(1),
        &|progress, _| progress == 0.0,
        plate(&build(N)),
    );
    let bound = RETURN_FLOOR_S + c;
    eprintln!(
        "13A-5c-2b byte cancel inside the tree's tile loop ({BW}×{BH}, {N} layers): {:.3} ms \
         cancel-to-return (issued at {:.1} ms; {}); T(1) {:.1} ms, T({N}) {:.1} ms, c_tree_row \
         {:.4} ms; bound T_f + c_tree_row = {:.3} ms (T_f 1 ms); ≈{:.2} units past the floor; \
         {}; load: {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        t1.as_secs_f64() * 1e3,
        t6.as_secs_f64() * 1e3,
        c * 1e3,
        bound * 1e3,
        past(p.latency, c),
        verdict(p.latency, bound),
        uptime(),
    );
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5c-2b)"]
fn a_concurrently_cancelled_tree_plate_returns_within_one_tree_row() {
    bench_tree(&bench_group);
}
