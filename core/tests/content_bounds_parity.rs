//! The byte content box (`dr_doc_layer_bounds`) and the trimmed
//! per-layer export (`dr_doc_export_layer_encode`) run ONE scan, so for a
//! depth-0 raster inside the canvas they report the same rectangle — sparse,
//! single-pixel, full and masked alike — and both agree that an empty layer
//! has none. Black-box through the FFI.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_export::*;
use doctored_core::ffi_group::dr_doc_layer_bounds;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

const W: u32 = 16;
const H: u32 = 12;

/// An opaque background with `layer` (canvas-sized, at the origin) above it
/// as entry 1.
fn doc_with(dir: &TempDir, layer: &RgbaImage) -> *mut DrDocument {
    let doc = doc_from(dir, "bg.png", &solid(W, H, [10, 20, 30, 255]));
    add_layer(dir, "layer.png", doc, 0, layer, "Layer")
}

fn content_bounds(doc: *const DrDocument, idx: usize) -> Option<(i32, i32, i32, i32)> {
    let mut xywh = [0i32; 4];
    unsafe { dr_doc_layer_bounds(doc, idx, xywh.as_mut_ptr()) }
        .then_some((xywh[0], xywh[1], xywh[2], xywh[3]))
}

/// The trimmed PNG export's `info.layer` rectangle, or the refusal.
fn trimmed_rect(doc: *const DrDocument, idx: usize) -> Result<(i32, i32, i32, i32), String> {
    let request = CString::new(r#"{"format":"png"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result: *mut ExportResult = unsafe {
        dr_doc_export_layer_encode(doc, idx, true, request.as_ptr(), ptr::null(), &mut err)
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
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
    Ok((
        n(&layer["origin"][0]),
        n(&layer["origin"][1]),
        n(&layer["size"][0]),
        n(&layer["size"][1]),
    ))
}

fn assert_parity(doc: *const DrDocument, what: &str) -> (i32, i32, i32, i32) {
    let bounds = content_bounds(doc, 1).unwrap_or_else(|| panic!("{what}: no content box"));
    let trimmed = trimmed_rect(doc, 1).unwrap_or_else(|e| panic!("{what}: trim refused: {e}"));
    assert_eq!(
        bounds, trimmed,
        "{what}: content box and export trim disagree"
    );
    bounds
}

fn layer_of(f: impl Fn(u32, u32) -> bool) -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        if f(x, y) {
            Rgba([200, 100, 50, 180])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

#[test]
fn a_sparse_rect_trims_to_the_content_box() {
    let dir = TempDir::new().unwrap();
    let doc = doc_with(
        &dir,
        &layer_of(|x, y| (3..9).contains(&x) && (2..7).contains(&y)),
    );
    assert_eq!(assert_parity(doc, "sparse rect"), (3, 2, 6, 5));
    unsafe { dr_doc_free(doc) };
}

#[test]
fn single_corner_pixels_trim_to_the_content_box() {
    let dir = TempDir::new().unwrap();
    for (cx, cy) in [(0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1)] {
        let doc = doc_with(&dir, &layer_of(|x, y| x == cx && y == cy));
        assert_eq!(
            assert_parity(doc, &format!("corner ({cx},{cy})")),
            (cx as i32, cy as i32, 1, 1)
        );
        unsafe { dr_doc_free(doc) };
    }
    let doc = doc_with(
        &dir,
        &layer_of(|x, y| (x, y) == (1, H - 2) || (x, y) == (W - 3, 1)),
    );
    assert_eq!(
        assert_parity(doc, "two pixels"),
        (1, 1, W as i32 - 3, H as i32 - 2)
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn full_coverage_trims_to_the_canvas() {
    let dir = TempDir::new().unwrap();
    let doc = doc_with(&dir, &layer_of(|_, _| true));
    assert_eq!(assert_parity(doc, "full"), (0, 0, W as i32, H as i32));
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_binary_mask_limits_both_answers_alike() {
    let dir = TempDir::new().unwrap();
    let doc = doc_with(&dir, &layer_of(|_, _| true));
    let selection = selection(W, H, |x, y| {
        if (5..11).contains(&x) && (4..9).contains(&y) {
            255
        } else {
            0
        }
    });
    let doc = apply(doc, |d| unsafe {
        dr_doc_adding_layer_mask(d, 1, MASK_FROM_SELECTION, selection.as_ptr(), W, H)
    });
    assert_eq!(assert_parity(doc, "binary mask"), (5, 4, 6, 5));
    unsafe { dr_doc_free(doc) };
}

#[test]
fn an_empty_layer_has_no_box_and_refuses_the_trim() {
    let dir = TempDir::new().unwrap();
    let doc = doc_with(&dir, &layer_of(|_, _| false));
    assert_eq!(
        content_bounds(doc, 1),
        None,
        "an empty layer has no content box"
    );
    assert!(
        trimmed_rect(doc, 1).is_err(),
        "a trimmed export of an empty layer refuses"
    );
    unsafe { dr_doc_free(doc) };
}
