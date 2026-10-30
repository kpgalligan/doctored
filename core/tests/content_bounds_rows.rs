//! The content-bounds scan reads ROWS, polls an optional job once per
//! row, and Stroke…'s alpha scan is memoized, so the host's scratch query
//! and the stroke share one scan. Black-box through the FFI. The
//! trimmed byte layer export's scan polls the export's own job once per row.
//! The trimmed typed layer export's tile-band scan polls the export's
//! job once per 256-row band.
//!
//! The pins are hand-computed boxes over a 600×530 layer whose only opaque
//! pixels are at buffer (300, 10), (5, 400) and (599, 529), so every edge
//! crosses a 256-px tile run: the box is x 5..600, y 10..530; a mask that
//! keeps x < 512 drops (599, 529), giving x 5..301, y 10..401. The stroke
//! scratch is `24·|F| + 4·|layer ∪ F| + 4096` for a 4 px stroke: the pad is
//! 4 + 2 = 6, so F is 607×532 at (−1, 4), and the union with the layer's
//! 600×530 at (0, 0) is 607×536.

use crate::common;

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportResult;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_adding_layer_mask;
use doctored_core::ffi_export::*;
use doctored_core::ffi_fill_stroke::{dr_doc_stroke_layer_edge, dr_doc_stroke_layer_edge_scratch};
use doctored_core::ffi_group::dr_doc_layer_bounds;
use doctored_core::ffi_precision_inspect::dr_precision_doc_layer_bounds;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{Rgba, RgbaImage};

use common::probe::{cancelled_parked_at, entry_count};
use common::{opaque_pattern, take_doc, take_err_string, BLEND_NORMAL, MASK_FROM_SELECTION};

const W: u32 = 600;
const H: u32 = 530;
const DOTS: [(u32, u32); 3] = [(300, 10), (5, 400), (599, 529)];
const OUTSIDE: c_int = 2;
const BLUE: [u8; 4] = [20, 40, 230, 255];

type Rect = (i32, i32, i32, i32);

fn dotted() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        if DOTS.contains(&(x, y)) {
            Rgba([200, 100, 50, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

/// An opaque background with the dotted layer as entry 1, at (0, 0).
fn byte_doc() -> DrDocument {
    DrDocument::from_pixels(opaque_pattern(W, H))
        .adding_image_layer(0, dotted(), "Dots")
        .expect("a layer")
}

/// The selection the masked rows keep: x < 512.
fn kept(x: u32) -> bool {
    x < 512
}

fn byte_masked() -> DrDocument {
    let doc = byte_doc();
    let selection: Vec<u8> = (0..W * H)
        .map(|i| if kept(i % W) { 255 } else { 0 })
        .collect();
    let out =
        unsafe { dr_doc_adding_layer_mask(&doc, 1, MASK_FROM_SELECTION, selection.as_ptr(), W, H) };
    assert!(!out.is_null(), "the mask was added");
    *unsafe { Box::from_raw(out) }
}

fn byte_bounds(doc: &DrDocument) -> Option<Rect> {
    let mut xywh = [0i32; 4];
    unsafe { dr_doc_layer_bounds(doc, 1, xywh.as_mut_ptr()) }
        .then_some((xywh[0], xywh[1], xywh[2], xywh[3]))
}

/// The trimmed PNG export's `info.layer` rectangle.
fn trimmed_rect(doc: &DrDocument) -> Rect {
    let request = CString::new(r#"{"format":"png"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result: *mut ExportResult = unsafe {
        dr_doc_export_layer_encode(doc, 1, true, request.as_ptr(), ptr::null(), &mut err)
    };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_export_result_info(result, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    unsafe { dr_export_result_free(result) };
    let info: serde_json::Value = serde_json::from_str(&text).expect("info json");
    let n = |v: &serde_json::Value| v.as_i64().expect("integer") as i32;
    let layer = &info["layer"];
    (
        n(&layer["origin"][0]),
        n(&layer["origin"][1]),
        n(&layer["size"][0]),
        n(&layer["size"][1]),
    )
}

fn typed_doc(masked: bool) -> PrecisionDocument {
    let samples = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if DOTS.contains(&(x, y)) {
                [50000, 20000, 9000, 65535]
            } else {
                [(x * 99) as u16, (y * 77) as u16, 4321, 0]
            }
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, samples, IccProfile::srgb(), None).unwrap();
    let mut layer = PrecisionDocumentLayer::raster("Dots", image, (0, 0), 1.0, true).unwrap();
    if masked {
        let plane = (0..W * H)
            .map(|i| if kept(i % W) { 65535 } else { 0 })
            .collect();
        let mask = PrecisionMask::new(
            PrecisionPlane::from_u16(W, H, plane, None).unwrap(),
            MaskProperties::at((0, 0)),
            true,
        )
        .unwrap();
        layer = layer.with_mask(Some(mask));
    }
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

fn typed_bounds(doc: &PrecisionDocument) -> Option<Rect> {
    let mut xywh = [0i32; 4];
    unsafe { dr_precision_doc_layer_bounds(doc, 0, xywh.as_mut_ptr()) }
        .then_some((xywh[0], xywh[1], xywh[2], xywh[3]))
}

// ----------------------------------------------------------------- pins --

#[test]
fn bounds_across_tile_runs_are_pinned() {
    let unmasked = (5, 10, 595, 520);
    let masked = (5, 10, 296, 391);
    assert_eq!(byte_bounds(&byte_doc()), Some(unmasked), "byte content box");
    assert_eq!(byte_bounds(&byte_masked()), Some(masked), "byte masked box");
    assert_eq!(trimmed_rect(&byte_doc()), unmasked, "trimmed export");
    assert_eq!(typed_bounds(&typed_doc(false)), Some(unmasked), "typed box");
    assert_eq!(
        typed_bounds(&typed_doc(true)),
        Some(masked),
        "typed masked box"
    );
    assert_eq!(
        unsafe { dr_doc_stroke_layer_edge_scratch(&byte_doc(), 1, 4.0) },
        24 * 607 * 532 + 4 * 607 * 536 + 4096,
        "stroke scratch"
    );
}

// ----------------------------------------------------- cancel and share --

/// A FRESH 300×2000 layer (its own allocation, so nothing about it is
/// memoized) whose only opaque pixel is (150, 1990).
fn tall_layer() -> RgbaImage {
    RgbaImage::from_fn(300, 2000, |x, y| {
        if (x, y) == (150, 1990) {
            Rgba([10, 200, 30, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

/// A 64×64 document with the tall layer as entry 1.
fn tall_doc() -> DrDocument {
    DrDocument::from_pixels(opaque_pattern(64, 64))
        .adding_image_layer(0, tall_layer(), "Tall")
        .expect("a layer")
}

/// The tall layer on a 300×2000 canvas, so its isolated render — the
/// buffer the trim scans — is 2000 rows tall.
fn tall_canvas_doc() -> DrDocument {
    DrDocument::from_pixels(opaque_pattern(300, 2000))
        .adding_image_layer(0, tall_layer(), "Tall")
        .expect("a layer")
}

/// An Outside 4 px stroke of entry 1 under `control`: the change (if any)
/// and the error (if any).
fn stroke(doc: &DrDocument, control: *const DrJobControl) -> (Option<DrDocument>, Option<String>) {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_stroke_layer_edge(
            doc,
            1,
            4.0,
            OUTSIDE,
            BLUE.as_ptr(),
            BLEND_NORMAL,
            1.0,
            false,
            control,
            &mut err,
        )
    };
    take_doc(out, err)
}

/// Cancels a stroke of `doc` with the worker parked at checkpoint 1002 —
/// inside the alpha scan of a 2000-row layer — and asserts it returned
/// "operation cancelled" before reserving anything.
fn cancel_in_the_scan(doc: &DrDocument) {
    let parked = cancelled_parked_at(1002, &|_, _| true, |token| {
        let (out, err) = stroke(doc, token as *const DrJobControl);
        (out.is_some(), err.unwrap_or_default())
    });
    assert_eq!(parked.scratch, 0, "cancelled before the reservation");
}

/// The checkpoint count of one completed stroke of `doc`.
fn stroke_checkpoints(doc: &DrDocument) -> u64 {
    let control = DrJobControl::default();
    let (out, err) = stroke(doc, &control);
    assert!(err.is_none(), "{err:?}");
    assert!(out.is_some(), "the stroke changed the layer");
    control.checkpoints()
}

#[test]
fn the_alpha_scan_is_cancellable() {
    cancel_in_the_scan(&tall_doc());
}

#[test]
fn the_scratch_query_and_the_stroke_share_one_scan() {
    let (a, b, c) = (tall_doc(), tall_doc(), tall_doc());
    cancel_in_the_scan(&c);
    assert_ne!(unsafe { dr_doc_stroke_layer_edge_scratch(&b, 1, 4.0) }, 0);
    let (on_a, on_b) = (stroke_checkpoints(&a), stroke_checkpoints(&b));
    assert_eq!(
        on_a - on_b,
        2000,
        "B's stroke reused the scan its scratch query made"
    );
    assert_eq!(
        stroke_checkpoints(&c),
        on_a,
        "the cancelled scan cached nothing"
    );
}

/// A PNG export of entry 1 under `control`, trimmed or canvas-framed: the
/// render's `(completed, error)`.
fn layer_export(doc: &DrDocument, trimmed: bool, control: *const DrJobControl) -> (bool, String) {
    let request = CString::new(r#"{"format":"png"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result: *mut ExportResult =
        unsafe { dr_doc_export_layer_encode(doc, 1, trimmed, request.as_ptr(), control, &mut err) };
    if result.is_null() {
        return (false, take_err_string(err));
    }
    unsafe { dr_export_result_free(result) };
    (true, String::new())
}

/// The checkpoint count of one completed export of entry 1.
fn export_checkpoints(doc: &DrDocument, trimmed: bool) -> u64 {
    let control = DrJobControl::default();
    let (ok, err) = layer_export(doc, trimmed, &control);
    assert!(ok, "{err}");
    control.checkpoints()
}

#[test]
fn the_trim_scan_polls_the_export_job_once_per_row() {
    let doc = tall_canvas_doc();
    assert_eq!(
        export_checkpoints(&doc, true) - export_checkpoints(&doc, false),
        2000,
        "the trim scan polls once per row of the 2000-row render"
    );
}

#[test]
fn the_trim_scan_is_cancellable() {
    let n0 = entry_count(&|_, stage, _| stage == 1, |token| {
        layer_export(&tall_canvas_doc(), true, token as *const DrJobControl)
    });
    let doc = tall_canvas_doc();
    // After `begin` come two checks (after `begin`, after the isolated
    // render), then one before each row is read: this parks before row
    // index 999 of 2000.
    let parked = cancelled_parked_at(n0 + 2 + 1000, &|p, s| s == 1 && p == 0.0, |token| {
        layer_export(&doc, true, token as *const DrJobControl)
    });
    assert_eq!(parked.scratch, 0, "cancelled inside the scan");
}

/// A 300×2000 sRGB document: an opaque u16 "Base" under a fully
/// transparent u16 "Clear" (postorder index 1, the exported item), so the
/// trimmed export of "Clear" scans a 2000-row render — 8 tile bands — and
/// refuses after the scan.
fn typed_tall_doc() -> PrecisionDocument {
    let (w, h) = (300u32, 2000u32);
    let layer = |name: &str, alpha: u16| {
        let samples = vec![[30000, 20000, 10000, alpha]; (w * h) as usize];
        let image = PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap();
        PrecisionDocumentLayer::raster(name, image, (0, 0), 1.0, true).unwrap()
    };
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer("Base", 65535), layer("Clear", 0)],
    )
    .unwrap()
}

/// A 16-bit PNG export of "Clear" under `control`, trimmed or
/// canvas-framed: `(completed, error)`.
fn typed_layer_export(
    doc: &PrecisionDocument,
    trimmed: bool,
    control: *const DrJobControl,
) -> (bool, String) {
    let request = CString::new(r#"{"format":"png","bit_depth":16}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result: *mut ExportResult = unsafe {
        dr_precision_doc_export_layer_encode(
            doc,
            1,
            trimmed,
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            control,
            &mut err,
        )
    };
    if result.is_null() {
        return (false, take_err_string(err));
    }
    unsafe { dr_export_result_free(result) };
    (true, String::new())
}

/// The checkpoint count at the isolated render's closing `report(1.0, 5)`,
/// read off a completed canvas-framed export; everything before it is
/// independent of `trimmed`.
fn typed_render_count() -> u64 {
    entry_count(&|_, stage, _| stage == 5, |token| {
        typed_layer_export(&typed_tall_doc(), false, token as *const DrJobControl)
    })
}

#[test]
fn the_typed_trim_scan_polls_once_per_tile_band() {
    let r5 = typed_render_count();
    let control = DrJobControl::default();
    let (ok, err) = typed_layer_export(&typed_tall_doc(), true, &control);
    assert!(!ok, "a fully transparent item refuses trimmed");
    assert_eq!(
        err,
        r#"layer "Clear" renders no visible pixels; nothing to trim"#
    );
    // One check after the render, then one per 256-row band of 2000 rows;
    // the refusal follows the scan with no further checkpoint.
    assert_eq!(
        control.checkpoints(),
        r5 + 1 + 8,
        "the trim scan polls once per tile band"
    );
}

#[test]
fn the_typed_trim_scan_is_cancellable() {
    let r5 = typed_render_count();
    let doc = typed_tall_doc();
    // Parks on band 4 of 8.
    cancelled_parked_at(r5 + 1 + 4, &|p, s| s == 5 && p == 1.0, |token| {
        typed_layer_export(&doc, true, token as *const DrJobControl)
    });
}
