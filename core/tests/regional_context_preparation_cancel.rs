//! Phase 13A-5c-1: the byte regional render's DESCRIPTION PREPARATION —
//! `adjust_calibrated::validate_context`'s parse of every non-group layer
//! `meta` under a profile with no matrix/TRC model. Since 13A-5c-1 the plan
//! charges one parse's scratch (`adjust_calibrated::context_scratch`: the
//! largest non-group `meta` length × 256, zero under a modelled profile),
//! the render reserves it and runs `validate_context_with_job` under its own
//! job (a check before and after each layer), the walk polls once per node,
//! and `composite_level_into`'s re-validation inside a materialization runs
//! under the materialization's live job.
//!
//! The fixture (`fixture`): a 256×192 canvas under an UNMODELLED RGB profile
//! (sRGB's 128-byte header with a zero tag count — kept, but with no model),
//! an opaque Background, then three canvas-sized transparent rasters carrying
//! a Levels description padded to `META_BYTES` = 4 MiB by a top-level
//! numeric `pad` array (`parse_meta` reads only `type` / `op` / `params`, so
//! the array is parsed and ignored). `from_meta` memoizes a meta above 64 KiB
//! (LRU of 16), so a run on a reused string parses NOTHING: every fixture
//! carries a fresh NONCE as the array's first element and every render that
//! must parse builds a fresh fixture. The materialized twin
//! (`materialized_fixture`) adds an isolated Normal group with a Multiply
//! colour overlay (MATERIALIZED) around ONE more padded Levels layer, so
//! `composite_level_into` re-validates once under the live job.
//!
//! The render's CHECKPOINT MODEL, from the code: `begin` 1, the render's
//! `check` 2, the context `reserve` 3, `validate_context_with_job`'s leading
//! check 4, then two per layer of ANY kind — layer `i` polled at `4 + 2i + 1`
//! and `4 + 2i + 2`, its parse between them (layer 1's between 7 and 8).
//!
//! DECLARED before measuring (the 13A-5a model, `regional`'s module doc):
//! stage "description preparation", interval `W_s` = ONE meta parse (serde
//! cannot be polled inside a parse); a cancel issued inside a parse returns
//! within `T_f + c_parse`, `T_f` = 1 ms the return floor and `c_parse` the
//! worker's own uncancelled checkpoint-timeline gap at the count the cancel
//! was issued at (`Timeline::gap_at`, 13A-5b-iii-a's `c_copy` form).
//!
//! LOAD ROBUSTNESS (13A-5b-iii-b-1): the default tests assert only
//! load-independent facts — exactness against `dr_doc_flattened`, the plan's
//! scratch, the checkpoint count delta against the sRGB twin, and a cancel
//! PARKED right after the first parse (`cancelled_parked_at`, its
//! release-to-return time printed only). The PASS/MISS verdict is
//! `#[ignore]`d and runs through `make cancel-bench`.

use std::ffi::c_char;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use image::RgbaImage;
use serde_json::Value;

use crate::common;
use common::probe::*;
use common::{img_dims, img_pixels, opaque_pattern, styled, take_err_string};

use crate::region_plan;

type Window = (u32, u32, u32, u32);

const W: u32 = 256;
const H: u32 = 192;
/// Every padded description's exact length (under `set_meta`'s 16 MiB cap).
const META_BYTES: usize = 4 * 1024 * 1024;
const LEVELS_PARAMS: &str = "{\"black\":0.1,\"white\":0.9,\"gamma\":1.8}";
const OVERLAY: &str =
    "{\"effects\":[{\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#80c0ff\"}]}";

/// `begin`, the render's `check`, the context `reserve` and
/// `validate_context_with_job`'s leading check (module doc).
const BEFORE_LAYERS: u64 = 4;

/// The checkpoint just before layer `i`'s parse.
fn parse_opens(i: u64) -> u64 {
    BEFORE_LAYERS + 2 * i + 1
}

/// The checkpoint just after layer `i`'s parse.
fn parse_closes(i: u64) -> u64 {
    parse_opens(i) + 1
}

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

/// The independent oracle: pixel (x + i·d, y + j·d) of the full projection
/// for every output (i, j), row-major.
fn expected(full: &[u8], (x, y, w, h): Window, d: u32) -> (u32, u32, Vec<u8>) {
    let (ow, oh) = (w.div_ceil(d), h.div_ceil(d));
    let mut out = Vec::new();
    for j in 0..oh {
        for i in 0..ow {
            let (cx, cy) = (x + i * d, y + j * d);
            let at = ((cy * W + cx) * 4) as usize;
            out.extend_from_slice(&full[at..at + 4]);
        }
    }
    (ow, oh, out)
}

/// The regional render under a LIVE job control, never cancelled; its
/// output and the control's checkpoint count.
fn polled_region(doc: &DrDocument, (x, y, w, h): Window, d: u32) -> (u32, u32, Vec<u8>, u64) {
    let hd = handle(doc);
    let control = unsafe { dr_job_control_create() };
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, d, u64::MAX, control, &mut err) };
    let checkpoints = unsafe { &*control }.checkpoints();
    unsafe {
        dr_job_control_free(control);
        dr_doc_free(hd);
    }
    assert!(!img.is_null(), "region: {}", take_err_string(err));
    let (ow, oh) = img_dims(img);
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    (ow, oh, out, checkpoints)
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

// ------------------------------------------------------------ fixtures --

/// A fresh nonce per fixture: distinct strings defeat `from_meta`'s memo.
fn nonce() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// `calibrated_adjustment_tests`' unmodelled RGB profile: sRGB's header
/// with a zero tag count, header size patched to 132.
fn unmodelled() -> Arc<IccProfile> {
    let mut bytes = IccProfile::srgb().bytes()[..128].to_vec();
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes[..4].copy_from_slice(&132u32.to_be_bytes());
    let profile = IccProfile::parse(&bytes).unwrap();
    assert!(profile.model().is_none(), "the profile has no model");
    Arc::new(profile)
}

/// A Levels description padded to exactly `META_BYTES` by a top-level
/// numeric `pad` array whose first element is `nonce`.
fn padded_meta(nonce: u64) -> String {
    let tail = "]}";
    let mut meta = format!(
        "{{\"type\":\"adjust\",\"op\":\"levels\",\"params\":{LEVELS_PARAMS},\"pad\":[{nonce}"
    );
    meta.reserve(META_BYTES - meta.len());
    while meta.len() + 2 + tail.len() <= META_BYTES {
        meta.push_str(",0");
    }
    while meta.len() + tail.len() < META_BYTES {
        meta.push(' ');
    }
    meta.push_str(tail);
    assert_eq!(meta.len(), META_BYTES);
    meta
}

/// A canvas-sized transparent raster carrying `padded_meta(nonce)` on top.
fn with_levels(doc: &DrDocument, nonce: u64) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, RgbaImage::new(W, H), "Levels")
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].meta = Some(padded_meta(nonce));
    out
}

/// The module doc's stack, under the unmodelled profile or (its twin) sRGB.
fn stack(nonce: u64, profile: Option<Arc<IccProfile>>) -> DrDocument {
    let mut doc = DrDocument::from_pixels(opaque_pattern(W, H));
    if let Some(profile) = profile {
        doc.profile = profile;
    }
    for _ in 0..3 {
        doc = with_levels(&doc, nonce);
    }
    doc
}

fn fixture(nonce: u64) -> DrDocument {
    stack(nonce, Some(unmodelled()))
}

/// The stack plus an isolated Normal group with a Multiply colour overlay
/// (materialized) around one more padded Levels layer.
fn materialize(doc: &DrDocument, nonce: u64) -> DrDocument {
    let doc = with_levels(doc, nonce);
    let child = doc.layers.len() - 1;
    let (mut doc, g, ..) = doc.group_layers(&[child], "Group").expect("group");
    doc.layers[g].blend = BlendMode::Normal;
    styled(&doc, g, OVERLAY)
}

fn materialized_fixture(nonce: u64) -> DrDocument {
    materialize(&fixture(nonce), nonce)
}

// ---------------------------------------------------------- exactness --

#[test]
fn the_regional_render_equals_the_reference_under_an_unmodelled_profile() {
    let doc = fixture(nonce());
    let reference = full(&doc);
    for (window, d) in [
        ((0, 0, W, H), 1),
        ((37, 29, 101, 77), 1),
        ((W - 17, 0, 17, 9), 1),
        ((0, H - 1, 1, 1), 1),
        ((0, 0, W, H), 2),
    ] {
        let (ow, oh, got, _) = polled_region(&doc, window, d);
        let (ew, eh, want) = expected(&reference, window, d);
        assert_eq!((ow, oh), (ew, eh), "window {window:?} at divisor {d}");
        assert!(
            got == want,
            "window {window:?} at divisor {d} differs from the projection"
        );
    }
}

// ---------------------------------------------------------------- plan --

#[test]
fn the_plan_charges_the_description_parse() {
    let pixels = u64::from(W) * u64::from(H);
    let output_bytes = pixels * 4;
    let unmodelled_plan = plan(&fixture(nonce()));
    let srgb_plan = plan(&stack(nonce(), None));
    for plan in [&unmodelled_plan, &srgb_plan] {
        assert_eq!(plan["output"]["bytes"].as_u64(), Some(output_bytes));
        let rows = plan["layers"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["operator"], "composite");
        for row in &rows[1..] {
            assert_eq!(row["operator"], "levels");
        }
        // The accumulator fill plus one pass per row: unchanged.
        assert_eq!(plan["work_units"].as_u64(), Some(pixels * 5));
    }
    assert_eq!(
        unmodelled_plan["scratch_heap_bytes"].as_u64(),
        Some(pixels * 16 + output_bytes + META_BYTES as u64 * 256),
        "the unmodelled plan charges one parse of the largest description"
    );
    // 13A-5c-3: the walk parses every adjustment node's description under
    // every profile, so the modelled twin carries the same one parse.
    assert_eq!(
        srgb_plan["scratch_heap_bytes"].as_u64(),
        Some(pixels * 16 + output_bytes + META_BYTES as u64 * 256),
        "a modelled profile's walk still parses the largest description"
    );
}

// ------------------------------------------------------ the checkpoints --

#[test]
fn the_description_stage_polls_once_per_parse() {
    let window = (0, 0, W, H);
    let flat = fixture(nonce());
    let layers = flat.layers.len() as u64;
    let (.., unmodelled_count) = polled_region(&flat, window, 1);
    let (.., srgb_count) = polled_region(&stack(nonce(), None), window, 1);
    assert_eq!(
        unmodelled_count
            .checked_sub(srgb_count)
            .expect("the unmodelled twin polls at least as often"),
        2 * layers,
        "the admission's validate_context_with_job checks twice per layer"
    );
    let n = nonce();
    let nested = materialized_fixture(n);
    let layers = nested.layers.len() as u64;
    let (.., unmodelled_count) = polled_region(&nested, window, 1);
    let n = nonce();
    let (.., srgb_count) = polled_region(&materialize(&stack(n, None), n), window, 1);
    assert_eq!(
        unmodelled_count
            .checked_sub(srgb_count)
            .expect("the unmodelled twin polls at least as often"),
        4 * layers,
        "the admission and the materialization's composite_level_into each check twice per \
         layer"
    );
}

#[test]
fn a_cancel_parked_after_the_first_parse_returns_nothing() {
    let p = cancelled_parked_at(
        parse_closes(1),
        &|progress, stage| progress == 0.0 && stage == 1,
        whole(&fixture(nonce())),
    );
    eprintln!(
        "13A-5c-1 parked cancel after layer 1's description parse: {}",
        parked_line(&p)
    );
}

// ------------------------------------------------ bench (cancel-bench) --

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-5c-1)"]
fn a_concurrently_cancelled_description_parse_returns_within_the_model() {
    let (open, close) = (parse_opens(1), parse_closes(1));
    let line = timeline(
        &|_, _, n| n >= open,
        &|_, _, n| n >= close,
        whole(&fixture(nonce())),
    );
    let c_parse = line
        .gap_at(open)
        .expect("the timeline recorded the parse's opening checkpoint");
    let p = probe(
        &|_, _, n| n >= open,
        Trigger::Freeze(FREEZE.min(c_parse / 2)),
        &|progress, stage| progress == 0.0 && stage == 1,
        whole(&fixture(nonce())),
    );
    assert_eq!(
        p.checkpoints, close,
        "the cancel returned at checkpoint {}, not at the parse's close {close}",
        p.checkpoints
    );
    let c = c_parse.as_secs_f64();
    let bound = RETURN_FLOOR_S + c;
    eprintln!(
        "13A-5c-1 byte cancel inside a description parse ({} MiB meta, unmodelled profile): \
         {:.3} ms cancel-to-return (issued at {:.1} ms; {}); c_parse {:.3} ms (timeline gap at \
         {open}); bound T_f + c_parse = {:.3} ms (T_f 1 ms); ≈{:.2} parses past the floor; {}",
        META_BYTES >> 20,
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        c * 1e3,
        bound * 1e3,
        past(p.latency, c),
        verdict(p.latency, bound),
    );
}
