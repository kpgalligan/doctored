//! The byte Content-Aware Fill admission is stated in the byte kernel's
//! own terms. `dr_doc_content_aware_fill_scratch` equals an independent
//! restatement of `doc_content_aware`'s module-doc formula on the reported
//! 12 MP geometry, the documented 1 MP cap, a new-layer request, a
//! feathered-mask target and a grouped document, with and without a source
//! mask; and a fill whose job control is limited to exactly that admission
//! completes, so no nested reservation on the byte path exceeds it.
//!
//! In place the kernel's layer term is its union-rect clone, bounded by
//! the estimate window (`min(layer bytes, 4·window)`), and the edit re-admits
//! by its coverage's component bound instead of the box's pixel count: a
//! fill of three separate squares in a 780-pixel box runs under the
//! metadata figure less 777 records and refuses one byte below that.
//!
//! Black-box through the FFI. The plate term of a document with a mask or a
//! group is read from `dr_mixed_resources_report_with_raw_masks`'s
//! `projection_scratch_estimate_bytes`, never from the crate's own helpers.

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_content_aware::*;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_jobs::*;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::*;

use crate::resource_report;

/// The byte kernel's figures, restated: `doc_inpaint::inpaint_scratch_bound`'s
/// 10 B per canvas pixel, 136 B per window pixel (capped at the 32 MP solve
/// window) and 8 MiB; `precision_inpaint`'s 128 B component record per
/// request-box pixel (capped at the 1 MP hole); the new layer's measured
/// canvas term.
const MIB8: u64 = 8 * 1024 * 1024;
/// `precision_inpaint::COMPONENT_BYTES`, restated (the module is private).
const COMPONENT_BYTES: u64 = 128;
/// `doc_content_aware`'s measured `NEW_LAYER_CANVAS_BYTES`, restated (private).
const NEW_LAYER_CANVAS_BYTES: u64 = 9;
const MAX_SOLVE_WINDOW: u64 = 32_000_000;
const MAX_HOLE: u64 = 1_000_000;
/// `doc_inpaint::LIFTED_WINDOW_BYTES`, restated: a lifted call's window
/// term, where lift 0 keeps 136.
const LIFTED_WINDOW_BYTES: u64 = 40;

/// `doc_inpaint::inpaint_lift`, restated: the smallest `L` with
/// `covered ≤ 1 MP · 4^L` (at most 3 is ever reached here).
fn lift(covered: u64) -> u32 {
    (0..8).find(|l| covered <= MAX_HOLE << (2 * l)).unwrap_or(7)
}
/// `inpaint_plan`'s ring bounds: 3 · the 7-pixel patch, and 512.
const RING_MIN: f64 = 21.0;
const RING_MAX: f64 = 512.0;

/// The request box grown by the requested ring and clamped to the canvas.
fn window(canvas: (u32, u32), bx: (u32, u32, u32, u32), ring: u32) -> u64 {
    let (x, y, w, h) = bx;
    let area = f64::from(w) * f64::from(h);
    let radius = (area / std::f64::consts::PI).sqrt().ceil();
    let asked = if ring == 0 { radius } else { f64::from(ring) };
    let pad = asked
        .max(RING_MIN)
        .max(radius * 0.5)
        .clamp(RING_MIN, RING_MAX)
        .ceil() as u32;
    let ww = (x + w + pad).min(canvas.0) - x.saturating_sub(pad);
    let wh = (y + h + pad).min(canvas.1) - y.saturating_sub(pad);
    u64::from(ww) * u64::from(wh)
}

/// The admission, restated term by term.
struct Terms {
    canvas: (u32, u32),
    bx: (u32, u32, u32, u32),
    ring: u32,
    layer: u64,
    plate: u64,
    source_mask: bool,
    new_layer: bool,
    /// The request's `covered` count, which sets the lift.
    covered: Option<u64>,
}

fn expected(t: &Terms) -> u64 {
    let n = u64::from(t.canvas.0) * u64::from(t.canvas.1);
    let win = window(t.canvas, t.bx, t.ring).min(MAX_SOLVE_WINDOW);
    let area = u64::from(t.bx.2) * u64::from(t.bx.3);
    // In place the kernel clones only its windows' union rect, which lies
    // inside the window; a new layer charges its whole plate.
    let layer = if t.new_layer {
        t.layer
    } else {
        t.layer.min(4 * win)
    };
    let per_window = if t.covered.map_or(0, lift) == 0 {
        136
    } else {
        LIFTED_WINDOW_BYTES
    };
    let mut total =
        n * 10 + layer + win * per_window + MIB8 + area.min(MAX_HOLE) * COMPONENT_BYTES + t.plate;
    if t.source_mask {
        total += n + 2 * win;
    }
    if t.new_layer {
        total += NEW_LAYER_CANVAS_BYTES * n;
    }
    total
}

fn request(
    bx: (u32, u32, u32, u32),
    layer: usize,
    sample: &str,
    output: &str,
    source_mask: bool,
) -> Value {
    let mut r = json!({"op":"content_aware_fill","layer":layer,"x":bx.0,"y":bx.1,
        "width":bx.2,"height":bx.3,"ring":0,"seed":7,"sample":sample,"output":output});
    if source_mask {
        r["source_mask"] = json!({"version":1,"initial":"all","operations":[]});
    }
    r
}

fn scratch(doc: &DrDocument, r: &Value) -> u64 {
    let text = CString::new(r.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_content_aware_fill_scratch(doc, text.as_ptr(), &mut err) };
    assert!(err.is_null(), "{}", take_err_string(err));
    assert!(out > 0);
    out
}

/// `projection_scratch_estimate_bytes` for `doc` alone.
fn projection(doc: &DrDocument) -> u64 {
    let ptrs = [doc as *const DrDocument];
    resource_report::report(&ptrs, &[], &[], &[], 0, 0).expect("a resource report")
        ["projection_scratch_estimate_bytes"]
        .as_u64()
        .unwrap()
}

fn flat(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([128, 128, 128, 255])))
}

#[test]
fn the_reported_geometry_is_admitted_well_under_a_gibibyte() {
    // 4000×3000, a 900×900 box at (1550, 1050), ring 0: window 1916².
    let (canvas, bx) = ((4000, 3000), (1550, 1050, 900, 900));
    assert_eq!(window(canvas, bx, 0), 1916 * 1916);
    let doc = flat(canvas.0, canvas.1);
    let got = scratch(&doc, &request(bx, 0, "current", "layer", false));
    let terms = Terms {
        canvas,
        bx,
        ring: 0,
        layer: 4000 * 3000 * 4,
        plate: 0,
        source_mask: false,
        new_layer: false,
        covered: None,
    };
    assert_eq!(got, expected(&terms));
    assert_eq!(got, 746_016_448);
    assert!(got < 1 << 30);
}

#[test]
fn the_documented_cap_is_admitted_in_place_and_on_a_new_layer() {
    let (canvas, bx) = ((4000, 3000), (1500, 1000, 1000, 1000));
    let doc = flat(canvas.0, canvas.1);
    let got = scratch(&doc, &request(bx, 0, "current", "layer", false));
    let mut terms = Terms {
        canvas,
        bx,
        ring: 0,
        layer: 4000 * 3000 * 4,
        plate: 0,
        source_mask: false,
        new_layer: false,
        covered: None,
    };
    assert_eq!(got, expected(&terms));
    assert_eq!(got, 829_909_248);
    // A plain one-layer document tiles its plate with no mask derivation and
    // no description context: the plate term is zero.
    let got = scratch(
        &doc,
        &request(bx, 0, "current_and_below", "new_layer", false),
    );
    terms.new_layer = true;
    assert_eq!(got, expected(&terms));
    assert_eq!(got, 969_522_944);
    assert!(got < 3 << 29, "{got} is not under 1.5 GiB");
}

/// A plasma background under a canvas-sized plasma layer whose centre is
/// punched flat, the layer carrying an enabled mask feathered by 6.
fn feathered(w: u32, h: u32) -> DrDocument {
    let hole = mask_of(w, h, |x, y| (40..70).contains(&x) && (30..56).contains(&y));
    let base = punch(&doc_of(w, h, plasma), &hole, [240, 20, 200]);
    let top = base.layer_canvas_image(0).unwrap();
    let mut doc = doc_of(w, h, |x, y| plasma(y, x))
        .adding_image_layer(0, top, "Top")
        .expect("add layer");
    let mut properties = MaskProperties::at((0, 0));
    properties.feather = 6.0;
    let plane = GrayImage::from_fn(w, h, |x, _| Luma([(x * 255 / w) as u8 | 128]));
    let layer = &mut doc.layers[1];
    layer.mask = Some(Arc::new(plane));
    layer.mask_enabled = true;
    layer.mask_properties = properties;
    doc
}

/// A plasma background and a punched plasma raster inside a pass-through
/// group; the raster's index.
fn grouped(w: u32, h: u32) -> (DrDocument, usize) {
    let mut doc = feathered(w, h);
    doc.layers[1].mask = None;
    doc.layers[1].mask_enabled = false;
    let (doc, ..) = doc.group_layers(&[1], "Group").expect("group");
    let target = doc
        .layers
        .iter()
        .position(|l| l.depth == 1)
        .expect("grouped raster");
    (doc, target)
}

#[test]
fn a_feathered_target_and_a_grouped_document_add_their_plate_terms() {
    let (w, h) = (1200u32, 60u32);
    let n = u64::from(w) * u64::from(h);
    let bx = (40, 30, 30, 26);
    // Pointwise stack: the tiled plate reserves the mask derivations, which
    // is the projection's own mask term — the projection less its 20n base.
    let doc = feathered(w, h);
    for source_mask in [false, true] {
        for (sample, plate) in [
            ("current", 0),
            ("current_and_below", projection(&doc) - 20 * n),
        ] {
            let got = scratch(&doc, &request(bx, 1, sample, "layer", source_mask));
            let terms = Terms {
                canvas: (w, h),
                bx,
                ring: 0,
                layer: n * 4,
                plate,
                source_mask,
                new_layer: false,
                covered: None,
            };
            assert_eq!(got, expected(&terms), "{sample}, source mask {source_mask}");
        }
    }
    // A group: the projection plus what a (≤ 1024)×(≤ 64) tile's 20 B
    // accumulator exceeds the projection's 16n by on this short, wide canvas.
    let (doc, target) = grouped(w, h);
    let tile = u64::from(w.min(1024)) * u64::from(h.min(64));
    let plate = projection(&doc) + (20 * tile).saturating_sub(16 * n);
    assert!(
        20 * tile > 16 * n,
        "the fixture must exercise the tile term"
    );
    for source_mask in [false, true] {
        let got = scratch(
            &doc,
            &request(bx, target, "current_and_below", "layer", source_mask),
        );
        let terms = Terms {
            canvas: (w, h),
            bx,
            ring: 0,
            layer: n * 4,
            plate,
            source_mask,
            new_layer: false,
            covered: None,
        };
        assert_eq!(got, expected(&terms), "grouped, source mask {source_mask}");
    }
}

/// The fill under a job control limited to exactly its admission.
fn fill_within_admission(doc: &DrDocument, r: &Value) {
    let admission = scratch(doc, r);
    let coverage = mask_of(doc.width, doc.height, |x, y| {
        let (bx, by) = (
            r["x"].as_u64().unwrap() as u32,
            r["y"].as_u64().unwrap() as u32,
        );
        let (bw, bh) = (
            r["width"].as_u64().unwrap() as u32,
            r["height"].as_u64().unwrap() as u32,
        );
        (bx..bx + bw).contains(&x) && (by..by + bh).contains(&y)
    });
    let text = CString::new(r.to_string()).unwrap();
    let control = unsafe { dr_job_control_create() };
    assert!(unsafe { dr_job_control_set_scratch_limit(control, admission) });
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_content_aware_fill_edit(doc, text.as_ptr(), coverage.as_ptr(), control, &mut err)
    };
    assert!(
        err.is_null(),
        "{r} refused under its own admission {admission}: {}",
        take_err_string(err)
    );
    assert!(!out.is_null(), "{r} changed nothing");
    unsafe {
        assert_eq!(dr_job_control_scratch_bytes(control), 0);
        dr_doc_free(out);
        dr_job_control_free(control);
    }
}

#[test]
fn a_fill_limited_to_its_admission_completes() {
    let doc = feathered(160, 100);
    let bx = (40, 30, 30, 26);
    fill_within_admission(&doc, &request(bx, 1, "current", "layer", false));
    fill_within_admission(&doc, &request(bx, 1, "all", "layer", false));
    fill_within_admission(&doc, &request(bx, 1, "current", "new_layer", false));
    fill_within_admission(&doc, &request(bx, 1, "all", "layer", true));
    fill_within_admission(&doc, &request(bx, 1, "current", "new_layer", true));
    let (doc, target) = grouped(160, 100);
    fill_within_admission(
        &doc,
        &request(bx, target, "current_and_below", "layer", false),
    );
}

/// Three separate 4 × 4 squares in the 30 × 26 box are three starts,
/// so the edit re-admits at the metadata figure less `(780 − 3)` records: a
/// control limited to exactly that returns without error and releases its
/// scratch, and one byte less refuses by memory.
#[test]
fn a_fill_admits_its_component_bound_not_its_box() {
    let doc = feathered(160, 100);
    let bx = (40, 30, 30, 26);
    let r = request(bx, 1, "current", "layer", false);
    let metadata = scratch(&doc, &r);
    let squares = [(42, 32), (52, 40), (62, 48)];
    let coverage = mask_of(doc.width, doc.height, |x, y| {
        squares
            .iter()
            .any(|&(sx, sy)| (sx..sx + 4).contains(&x) && (sy..sy + 4).contains(&y))
    });
    let limit = metadata - (780 - 3) * COMPONENT_BYTES;
    let text = CString::new(r.to_string()).unwrap();
    for (limit, admitted) in [(limit, true), (limit - 1, false)] {
        let control = unsafe { dr_job_control_create() };
        assert!(unsafe { dr_job_control_set_scratch_limit(control, limit) });
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_doc_content_aware_fill_edit(
                &doc,
                text.as_ptr(),
                coverage.as_ptr(),
                control,
                &mut err,
            )
        };
        if admitted {
            assert!(
                err.is_null(),
                "refused at {limit}: {}",
                take_err_string(err)
            );
        } else {
            assert!(out.is_null());
            let message = take_err_string(err);
            assert!(message.contains("memory"), "{message}");
        }
        unsafe {
            assert_eq!(dr_job_control_scratch_bytes(control), 0);
            if !out.is_null() {
                dr_doc_free(out);
            }
            dr_job_control_free(control);
        }
    }
}

/// The byte admission refuses a box or ring out of range in the fill's
/// own words, through the scratch export and the edit alike.
#[test]
fn the_admission_refuses_in_the_fills_own_words() {
    let range = "content-aware fill bounds or ring are out of range";
    let doc = flat(64, 64);
    let mut ring = request((24, 24, 16, 16), 0, "current", "layer", false);
    ring["ring"] = json!(513);
    for r in [
        ring,
        request((24, 24, 50, 16), 0, "current", "layer", false),
        request((24, 24, 0, 16), 0, "current", "layer", false),
    ] {
        let text = CString::new(r.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe { dr_doc_content_aware_fill_scratch(&doc, text.as_ptr(), &mut err) };
        assert_eq!(out, 0, "{r}");
        assert_eq!(take_err_string(err), range, "{r}");
    }
    let mut mask = vec![0u8; 64 * 64];
    mask[30 * 64 + 30] = 255;
    let (out, message) = fill(&doc, 0, &mask, (64, 64), (513, 7), false);
    assert!(out.is_none());
    assert_eq!(message.as_deref(), Some(range));
}

/// A request carrying its hole's `covered` count is priced at that
/// count's lift: a 4 MP hole (lift 1) charges `LIFTED_WINDOW_BYTES` per
/// window pixel, and without the count the figure is the lift-0 one,
/// unchanged.
#[test]
fn a_lifted_request_is_priced_at_its_lift() {
    let (canvas, bx) = ((2600, 2600), (300, 300, 2000, 2000));
    let doc = flat(canvas.0, canvas.1);
    let mut terms = Terms {
        canvas,
        bx,
        ring: 0,
        layer: 2600 * 2600 * 4,
        plate: 0,
        source_mask: false,
        new_layer: false,
        covered: None,
    };
    let plain = scratch(&doc, &request(bx, 0, "current", "layer", false));
    assert_eq!(plain, expected(&terms));
    let mut lifted = request(bx, 0, "current", "layer", false);
    lifted["covered"] = json!(4_000_000);
    terms.covered = Some(4_000_000);
    assert_eq!(lift(4_000_000), 1);
    let got = scratch(&doc, &lifted);
    assert_eq!(got, expected(&terms));
    assert_eq!(
        plain - got,
        window(canvas, bx, 0) * (136 - LIFTED_WINDOW_BYTES)
    );
    // A megapixel or less stays at lift 0 with or without the count.
    let small = (1000, 1000, 1000, 1000);
    let mut counted = request(small, 0, "current", "layer", false);
    counted["covered"] = json!(1_000_000);
    assert_eq!(
        scratch(&doc, &counted),
        scratch(&doc, &request(small, 0, "current", "layer", false))
    );
}

/// The edit with `coverage` and the request `r`: the result and the message.
fn edit(doc: &DrDocument, r: &Value, coverage: &[u8]) -> (bool, Option<String>) {
    let text = CString::new(r.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_content_aware_fill_edit(doc, text.as_ptr(), coverage.as_ptr(), ptr::null(), &mut err)
    };
    let changed = !out.is_null();
    if changed {
        unsafe { dr_doc_free(out) };
    }
    (changed, (!err.is_null()).then(|| take_err_string(err)))
}

/// `covered` is the count of coverage bytes at or above the fill's
/// hole threshold (1), so a feathered ramp counts and is admitted; any other
/// count is refused by name before the fill runs.
#[test]
fn the_covered_count_is_the_holes_own() {
    let doc = feathered(160, 100);
    let bx = (40, 30, 30, 26);
    // A soft hole: a ramp from 1 to 255 across the box, 0 in its first row.
    let coverage: Vec<u8> = (0..160 * 100)
        .map(|i| {
            let (x, y) = (i % 160, i / 160);
            if (40..70).contains(&x) && (31..56).contains(&y) {
                (1 + (x - 40) * 254 / 29) as u8
            } else {
                0
            }
        })
        .collect();
    let count = coverage.iter().filter(|c| **c >= 1).count() as u64;
    assert_eq!(count, 30 * 25);
    assert!(coverage.iter().filter(|c| **c > 0 && **c < 128).count() > 300);
    let mut r = request(bx, 1, "current", "layer", false);
    r["covered"] = json!(count);
    let (changed, message) = edit(&doc, &r, &coverage);
    assert!(message.is_none(), "{message:?}");
    assert!(changed, "the feathered hole was filled");
    // The count of bytes at or above 128 is not the hole's.
    let hard = coverage.iter().filter(|c| **c >= 128).count() as u64;
    r["covered"] = json!(hard);
    let (changed, message) = edit(&doc, &r, &coverage);
    assert!(!changed);
    assert_eq!(
        message.as_deref(),
        Some(
            format!(
                "content-aware request covered {hard} does not match the hole's {count} \
                 covered pixels"
            )
            .as_str()
        )
    );
    // Another op refuses the key.
    let red_eye = json!({"op":"red_eye","layer":0,"x":0,"y":0,"width":4,"height":4,"covered":16});
    let (changed, message) = edit(&doc, &red_eye, &coverage);
    assert!(!changed);
    assert_eq!(
        message.as_deref(),
        Some("covered is not an argument of this retouch operation")
    );
}
