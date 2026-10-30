//! Retouching onto a NEW layer charges the temporary document's copy of
//! the sampling plate to the job. Parked at the same point of its own run, a
//! new-layer export reserves exactly its in-place twin's figure plus the
//! plate's `W·H·4` tiles; before the solve starts it holds exactly `W·H·8`
//! (the owned copy and the tiles it is cut into). And
//! `dr_doc_spot_heal_to_new_layer` closes its control like its sibling: a
//! last checkpoint after the kernel, then 1.0 / 5, so a cancel landing after
//! the solve still returns nothing.
//!
//! The deposit that lifts the healed plate onto the new layer polls —
//! one canvas pass for its domain, a reservation, the healed rect read and
//! the write once per tile, the lift every 4096 pixels
//! (`common::deposit_checkpoints`) — and a cancel parked inside it returns
//! nothing. Its reservation charges the rewritten tiles by the tiles the
//! domain touches, so a scattered stroke the solve admitted is not refused
//! at the deposit. The retouch layer (and `dr_doc_adding_layer_at`'s)
//! is a `LayerRaster::filled` plane — one shared tile, not a canvas copy.
//! The deposit reads only the healed rect, since the new layer is
//! transparent, so it loses one read per tile and `4·w·h` of scratch.
//!
//! Content-Aware Fill's new-layer arm lifts its result through the same
//! polled rect deposit instead of a canvas-sized lifted image — its tail is
//! the in-place arm's plus exactly `common::deposit_checkpoints` — and a
//! cancel parked inside that deposit returns nothing, with the fill's
//! admission still the live reservation — the edit's own
//! re-admission by its coverage's component bound (`FILL_BOX` is one
//! rectangle, one start), `admission − 719·COMPONENT_BYTES`.
//!
//! Black-box through the FFI. Every figure is read while the worker is PARKED
//! at a counted checkpoint taken from its own report log
//! (`common::probe::cancelled_parked_at`), so nothing here reads a clock.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_content_aware::{
    dr_doc_content_aware_fill_edit, dr_doc_content_aware_fill_scratch,
};
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_heal::{
    dr_doc_heal_layer, dr_doc_heal_to_new_layer, dr_doc_retouch_source,
    dr_doc_spot_heal_to_new_layer,
};
use doctored_core::ffi_insert::dr_doc_adding_layer_at;
use doctored_core::ffi_jobs::*;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_raster::LayerRaster;
use image::{Rgba, RgbaImage};

use crate::common;
use common::probe::*;
use common::*;

use crate::resource_report;

const W: u32 = 160;
const H: u32 = 120;
/// `precision_inpaint::COMPONENT_BYTES`, restated (the module is private).
const COMPONENT_BYTES: u64 = 128;
const CURRENT: u32 = 0;
const CURRENT_AND_BELOW: u32 = 1;
/// The temporary document's plate: `W·H` RGBA8 pixels.
const PLATE: u64 = W as u64 * H as u64 * 4;

/// One opaque layer of structured, non-periodic texture both the heal and
/// the inpaint can work against.
fn fixture() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        let c = plasma(x, y);
        Rgba([c[0], c[1], c[2], 255])
    }))
}

/// A premultiplied overlay at full coverage over one 30 × 24 rect, its
/// straight RGB a coarse checker the destination does not have.
fn overlay() -> Vec<u8> {
    let mut out = vec![0u8; (W * H * 4) as usize];
    for y in 48..72 {
        for x in 60..90 {
            let v = if (x / 3 + y / 3) % 2 == 0 { 70 } else { 150 };
            let i = ((y * W + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&[v, v + 20, v + 40, 255]);
        }
    }
    out
}

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
#[derive(Clone, Copy, Debug)]
enum Export {
    Heal,
    HealToNewLayer,
    SpotHeal,
    SpotHealToNewLayer,
}

/// `export` on the document at `doc` (an address, so the closure is `Send`)
/// under the control at `token`, freeing any result.
fn render(export: Export, doc: usize) -> impl FnOnce(usize) -> (bool, String) + Send {
    move |token: usize| {
        let doc = doc as *const DrDocument;
        let control = token as *const DrJobControl;
        let (ov, pl) = (overlay(), plate(unsafe { &*doc }));
        let name = CString::new("Retouch").unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            match export {
                Export::Heal => {
                    dr_doc_heal_layer(doc, 0, ov.as_ptr(), W, H, 1.0, control, &mut err)
                }
                Export::HealToNewLayer => dr_doc_heal_to_new_layer(
                    doc,
                    0,
                    ov.as_ptr(),
                    W,
                    H,
                    1.0,
                    pl.as_ptr(),
                    name.as_ptr(),
                    control,
                    &mut err,
                ),
                Export::SpotHeal => dr_doc_spot_heal_layer_job(
                    doc,
                    0,
                    ov.as_ptr(),
                    W,
                    H,
                    1.0,
                    0,
                    7,
                    0,
                    CURRENT,
                    false,
                    control,
                    &mut err,
                ),
                Export::SpotHealToNewLayer => dr_doc_spot_heal_to_new_layer(
                    doc,
                    0,
                    ov.as_ptr(),
                    W,
                    H,
                    1.0,
                    0,
                    7,
                    0,
                    pl.as_ptr(),
                    name.as_ptr(),
                    control,
                    &mut err,
                ),
            }
        };
        let message = (!err.is_null()).then(|| take_err_string(err));
        let ok = !out.is_null();
        if ok {
            unsafe { dr_doc_free(out) };
        }
        (ok, message.unwrap_or_default())
    }
}

/// The live reservation while `export` is parked `k` checkpoints after the
/// first report of `stage` in its own uncancelled log.
fn scratch_after(export: Export, stage: u32, k: u64) -> u64 {
    let doc = fixture();
    let at = entry_count(
        &|_, s, _| s == stage,
        render(export, &doc as *const _ as usize),
    );
    let parked = cancelled_parked_at(
        at + k,
        &|_, s| s == stage,
        render(export, &doc as *const _ as usize),
    );
    println!(
        "{export:?}: {}; scratch {}",
        parked_line(&parked),
        parked.scratch
    );
    parked.scratch
}

#[test]
fn a_new_layer_heal_reserves_the_in_place_figure_plus_the_plate() {
    let in_place = scratch_after(Export::Heal, 4, 1);
    let new_layer = scratch_after(Export::HealToNewLayer, 4, 1);
    assert!(in_place > 0);
    assert_eq!(new_layer, in_place + PLATE);
}

#[test]
fn a_new_layer_spot_heal_reserves_the_in_place_figure_plus_the_plate() {
    let in_place = scratch_after(Export::SpotHeal, 3, 1);
    let new_layer = scratch_after(Export::SpotHealToNewLayer, 3, 1);
    assert!(in_place > 0);
    assert_eq!(new_layer, in_place + PLATE);
}

#[test]
fn the_plate_copy_is_reserved_before_it_is_made() {
    // Checkpoint 1 is `begin`, 2 the reservation's own, 3 the solve's first.
    for export in [Export::HealToNewLayer, Export::SpotHealToNewLayer] {
        let doc = fixture();
        let parked = cancelled_parked_at(
            3,
            &|progress, stage| stage == 1 && progress == 0.0,
            render(export, &doc as *const _ as usize),
        );
        assert_eq!(parked.scratch, 2 * PLATE, "{export:?}");
    }
}

#[test]
fn a_new_layer_spot_heal_closes_its_control() {
    let doc = fixture();
    let control = unsafe { dr_job_control_create() };
    let (ok, message) =
        render(Export::SpotHealToNewLayer, &doc as *const _ as usize)(control as usize);
    assert!(ok, "{message}");
    unsafe {
        assert_eq!(dr_job_control_progress(control), 1.0);
        assert_eq!(dr_job_control_stage(control), 5);
        assert_eq!(dr_job_control_scratch_bytes(control), 0);
        dr_job_control_free(control);
    }
    // A cancel landing after the solve — at the closing checkpoint, the count
    // the stage-5 report records — returns nothing.
    let close = entry_count(
        &|_, s, _| s == 5,
        render(Export::SpotHealToNewLayer, &doc as *const _ as usize),
    );
    let parked = cancelled_parked_at(
        close,
        &|_, stage| stage == 4,
        render(Export::SpotHealToNewLayer, &doc as *const _ as usize),
    );
    println!("{}", parked_line(&parked));
}

// ------------------------------------------------------- the deposit poll --

/// The stroke's domain bounding box, `(x0, y0, x1, y1)` exclusive.
const STROKE: (u32, u32, u32, u32) = (60, 48, 90, 72);
/// One full RGBA8 tile.
const TILE: u64 = 256 * 256 * 4;

/// The first stage-4 report's, the last stage-4 report's and the stage-5
/// report's checkpoint counts in an uncancelled run.
fn deposit_marks(export: Export) -> (u64, u64, u64) {
    let doc = fixture();
    let log = reports(render(export, &doc as *const _ as usize));
    let first4 = log.iter().find(|r| r.stage == 4).unwrap().checkpoints;
    let last4 = log.iter().rev().find(|r| r.stage == 4).unwrap().checkpoints;
    let stage5 = log.iter().find(|r| r.stage == 5).unwrap().checkpoints;
    (first4, last4, stage5)
}

#[test]
fn the_deposit_polls_its_declared_checkpoints_and_a_cancel_inside_it_returns_nothing() {
    for export in [Export::HealToNewLayer, Export::SpotHealToNewLayer] {
        let (_, last4, stage5) = deposit_marks(export);
        // The final write's one tile, the deposit, the closing check.
        assert_eq!(
            stage5 - last4,
            1 + 1 + deposit_checkpoints(u64::from(W * H), STROKE),
            "{export:?}"
        );
        // The deposit's first poll: nothing returned, the input untouched.
        let doc = fixture();
        let before = doc.layers[0].pixels.clone();
        let parked = cancelled_parked_at(
            last4 + 2,
            &|_, stage| stage == 4,
            render(export, &doc as *const _ as usize),
        );
        println!("{export:?}: {}", parked_line(&parked));
        assert!(parked.scratch > 0, "{export:?}");
        assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &before));
        // The write's last tile holds the deposit's reservation: the held
        // temp plate, the mask, the one healed rect read and two
        // rewritten tiles per touched tile (one here), plus the retouch
        // layer's 1 MiB.
        let parked = cancelled_parked_at(
            stage5 - 1,
            &|_, stage| stage == 4,
            render(export, &doc as *const _ as usize),
        );
        assert_eq!(
            parked.scratch,
            PLATE + u64::from(W * H) + 4 * 720 + 2 * TILE + 1_048_576,
            "{export:?}"
        );
    }
}

/// `export` (a new-layer one) on the document at `doc`, with `overlay` and
/// the document's own current-and-below plate, freeing any result.
fn render_on(
    export: Export,
    doc: usize,
    overlay: Vec<u8>,
) -> impl FnOnce(usize) -> (bool, String) + Send {
    move |token: usize| {
        let doc = doc as *const DrDocument;
        let (w, h) = unsafe { ((*doc).width, (*doc).height) };
        let pl = plate(unsafe { &*doc });
        let name = CString::new("Retouch").unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let control = token as *const DrJobControl;
        let out = unsafe {
            match export {
                Export::HealToNewLayer => dr_doc_heal_to_new_layer(
                    doc,
                    0,
                    overlay.as_ptr(),
                    w,
                    h,
                    1.0,
                    pl.as_ptr(),
                    name.as_ptr(),
                    control,
                    &mut err,
                ),
                Export::SpotHealToNewLayer => dr_doc_spot_heal_to_new_layer(
                    doc,
                    0,
                    overlay.as_ptr(),
                    w,
                    h,
                    1.0,
                    0,
                    7,
                    0,
                    pl.as_ptr(),
                    name.as_ptr(),
                    control,
                    &mut err,
                ),
                _ => unreachable!("a new-layer export"),
            }
        };
        let message = (!err.is_null()).then(|| take_err_string(err));
        let ok = !out.is_null();
        if ok {
            unsafe { dr_doc_free(out) };
        }
        (ok, message.unwrap_or_default())
    }
}

/// A premultiplied full-coverage checker overlay over each exclusive rect.
fn dabs(w: u32, h: u32, rects: &[(u32, u32, u32, u32)]) -> Vec<u8> {
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

#[test]
fn a_scattered_stroke_the_solve_admitted_is_not_refused_at_the_deposit() {
    // Two dabs at opposite corners: the domain's box is nearly the canvas,
    // but it touches only two tiles.
    let (w, h) = (2000, 1500);
    let doc = doc_of(w, h, plasma);
    let at = &doc as *const _ as usize;
    let overlay = || dabs(w, h, &[(20, 20, 40, 40), (1960, 1460, 1980, 1480)]);
    for export in [Export::HealToNewLayer, Export::SpotHealToNewLayer] {
        let log = reports(render_on(export, at, overlay()));
        // The solve's largest reservation is the one in force at its first
        // stage-4 report (the held plate included).
        let solve = log.iter().find(|r| r.stage == 4).unwrap().checkpoints;
        let solve =
            cancelled_parked_at(solve + 1, &|_, s| s == 4, render_on(export, at, overlay()));
        let close = log.iter().find(|r| r.stage == 5).unwrap().checkpoints;
        let deposit =
            cancelled_parked_at(close - 1, &|_, s| s == 4, render_on(export, at, overlay()));
        println!(
            "{export:?}: solve {} deposit {}",
            solve.scratch, deposit.scratch
        );
        assert!(deposit.scratch > 0);
        assert!(
            deposit.scratch <= solve.scratch,
            "{export:?}: the deposit reserves {} over the solve's {}",
            deposit.scratch,
            solve.scratch
        );
    }
}

// ------------------------------------------------- the filled retouch layer --

/// `category.field` of the resource report over `doc` alone.
fn inventory(doc: *const DrDocument, category: &str, field: &str) -> u64 {
    let value = resource_report::report(&[doc], &[], &[], &[], 0, 0).expect("a report");
    value["categories"][category][field].as_u64().unwrap_or(0)
}

#[test]
fn the_retouch_layer_and_a_new_layer_share_one_filled_tile() {
    // 1024 × 768 is 4 × 3 full tiles: `filled` shares ONE tile for all 12.
    let (w, h) = (1024, 768);
    let doc = doc_of(w, h, plasma);
    let count = |d: *const DrDocument| {
        (
            inventory(d, "layer_pixels", "allocation_count"),
            inventory(d, "layer_pixels", "unique_bytes"),
        )
    };
    let source = count(&doc);
    // A heal stroke inside tile (0, 0): the filled tile plus the one tile
    // the deposit rewrites.
    let control = unsafe { dr_job_control_create() };
    let overlay = dabs(w, h, &[(60, 48, 90, 72)]);
    let pl = plate(&doc);
    let name = CString::new("Retouch").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let healed = unsafe {
        dr_doc_heal_to_new_layer(
            &doc,
            0,
            overlay.as_ptr(),
            w,
            h,
            1.0,
            pl.as_ptr(),
            name.as_ptr(),
            control,
            &mut err,
        )
    };
    assert!(err.is_null() && !healed.is_null(), "a new-layer heal");
    let after = count(healed);
    assert_eq!(after.0, source.0 + 2);
    assert_eq!(after.1, source.1 + 2 * TILE);
    // New Layer: the filled tile alone.
    let added = unsafe { dr_doc_adding_layer_at(&doc, 0, 0, name.as_ptr()) };
    assert!(!added.is_null(), "a new layer");
    let after = count(added);
    assert_eq!(after.0, source.0 + 1);
    assert_eq!(after.1, source.1 + TILE);
    unsafe {
        dr_doc_free(healed);
        dr_doc_free(added);
        dr_job_control_free(control);
    }
}

// ------------------------------------------ the fill's new-layer deposit --

/// The fill's damage and selection, `(x0, y0, x1, y1)` exclusive.
const FILL_BOX: (u32, u32, u32, u32) = (60, 40, 90, 64);

/// A plasma document with `FILL_BOX` punched flat, and the box's coverage
/// (255 inside).
fn fill_fixture() -> (DrDocument, Vec<u8>) {
    let (x0, y0, x1, y1) = FILL_BOX;
    let coverage = mask_of(W, H, |x, y| (x0..x1).contains(&x) && (y0..y1).contains(&y));
    let doc = punch(&doc_of(W, H, plasma), &coverage, [240, 20, 200]);
    (doc, coverage)
}

/// The fill request over `FILL_BOX`, placed by `output`.
fn fill_request(output: &str) -> CString {
    CString::new(format!(
        r#"{{"op":"content_aware_fill","layer":0,"x":60,"y":40,"width":30,"height":24,"ring":0,"seed":7,"sample":"current","output":"{output}"}}"#
    ))
    .unwrap()
}

/// `dr_doc_content_aware_fill_edit` with `output` on the document at `doc`
/// (NULL control allowed); the caller frees a non-NULL result.
fn fill(
    doc: *const DrDocument,
    output: &str,
    coverage: &[u8],
    token: usize,
) -> (*mut DrDocument, Option<String>) {
    let request = fill_request(output);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_content_aware_fill_edit(
            doc,
            request.as_ptr(),
            coverage.as_ptr(),
            token as *const DrJobControl,
            &mut err,
        )
    };
    (out, (!err.is_null()).then(|| take_err_string(err)))
}

/// [`fill`] as a probe render, freeing any result.
fn fill_render(
    doc: usize,
    output: &'static str,
    coverage: Vec<u8>,
) -> impl FnOnce(usize) -> (bool, String) + Send {
    move |token: usize| {
        let (out, message) = fill(doc as *const DrDocument, output, &coverage, token);
        let ok = !out.is_null();
        if ok {
            unsafe { dr_doc_free(out) };
        }
        (ok, message.unwrap_or_default())
    }
}

/// The last stage-4 report's and the stage-5 report's checkpoint counts in
/// an uncancelled fill with `output`.
fn fill_marks(doc: usize, output: &'static str, coverage: &[u8]) -> (u64, u64) {
    let log = reports(fill_render(doc, output, coverage.to_vec()));
    let last4 = log.iter().rev().find(|r| r.stage == 4).unwrap().checkpoints;
    let stage5 = log.iter().find(|r| r.stage == 5).unwrap().checkpoints;
    (last4, stage5)
}

#[test]
fn a_new_layer_fill_deposits_through_the_polled_rect_deposit() {
    let (doc, coverage) = fill_fixture();
    // Guard: the fill moves every box pixel, so the deposit's domain is the
    // whole box.
    let (out, message) = fill(&doc, "layer", &coverage, 0);
    assert!(message.is_none() && !out.is_null(), "{message:?}");
    let (before, after) = (layer_pixels(&doc, 0), layer_pixels(out, 0));
    unsafe { dr_doc_free(out) };
    let (x0, y0, x1, y1) = FILL_BOX;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * W + x) * 4) as usize;
            assert_ne!(before[i..i + 4], after[i..i + 4], "({x}, {y}) unchanged");
        }
    }
    let at = &doc as *const _ as usize;
    let tail = |output| {
        let (last4, stage5) = fill_marks(at, output, &coverage);
        stage5 - last4
    };
    assert_eq!(
        tail("new_layer") - tail("layer"),
        deposit_checkpoints(u64::from(W * H), FILL_BOX)
    );
}

#[test]
fn a_cancel_inside_the_fill_deposit_returns_nothing() {
    let (doc, coverage) = fill_fixture();
    let at = &doc as *const _ as usize;
    let before = doc.layers[0].pixels.clone();
    let (last4, stage5) = fill_marks(at, "layer", &coverage);
    let kernel_tail = stage5 - last4 - 1;
    let (last4, stage5) = fill_marks(at, "new_layer", &coverage);
    let request = fill_request("new_layer");
    let mut err: *mut c_char = ptr::null_mut();
    let admission = unsafe { dr_doc_content_aware_fill_scratch(&doc, request.as_ptr(), &mut err) };
    assert!(err.is_null() && admission > 0);
    // The metadata figure charges the box's 720 records; the edit re-admits
    // with the one start its single rectangle has.
    let admission = admission - 719 * COMPONENT_BYTES;
    // The deposit's first domain poll, and the write's last tile.
    for count in [last4 + kernel_tail + 1, stage5 - 1] {
        let parked = cancelled_parked_at(
            count,
            &|_, stage| stage == 4,
            fill_render(at, "new_layer", coverage.clone()),
        );
        println!("{}; scratch {}", parked_line(&parked), parked.scratch);
        assert_eq!(parked.scratch, admission, "parked at {count}");
        assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &before));
    }
}
