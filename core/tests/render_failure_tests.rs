//! Capacity refusals are errors, never transparent group content. Tiny explicit
//! budgets exercise the production render path through the FFI.
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::{dr_image_free, dr_string_free};
use doctored_core::ffi_doc::{dr_doc_flattened, dr_doc_free, dr_doc_open, dr_doc_save_native};
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::{cpath, img_pixels, take_err_string};

fn red() -> RgbaImage {
    RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]))
}

fn wrapped(doc: DrDocument, idx: usize, isolated: bool) -> (DrDocument, usize) {
    let (doc, group, _, _) = doc.group_layers(&[idx], "Group").unwrap();
    let doc = if isolated {
        doc.with_layer_blend_mode(group, BlendMode::Normal).unwrap()
    } else {
        doc.with_layer_opacity(group, 0.5).unwrap()
    };
    (doc, group)
}

fn render(doc: &DrDocument, budget: u64) -> Result<Vec<u8>, String> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_doc_flattened(doc, budget, std::ptr::null(), &mut err) };
    if image.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null());
    let pixels = img_pixels(image);
    unsafe { dr_image_free(image) };
    Ok(pixels)
}

#[test]
fn isolated_group_refusal_is_distinct_from_empty_content() {
    let (doc, _) = wrapped(DrDocument::from_pixels(red()), 0, true);
    let error = render(&doc, 3).unwrap_err();
    assert!(error.contains("group buffer budget (3 pixels)"), "{error}");
    assert!(error.contains("save as Doctored (.doctored)"));
    assert_eq!(render(&doc, 4).unwrap(), red().into_raw());
    // Failure does not poison the immutable document or later renders.
    assert_eq!(render(&doc, 0).unwrap(), red().into_raw());
}

#[test]
fn nested_isolated_and_gated_groups_propagate_the_inner_failure() {
    for inner_isolated in [false, true] {
        for outer_isolated in [false, true] {
            let (doc, group) = wrapped(DrDocument::from_pixels(red()), 0, inner_isolated);
            let (doc, _) = wrapped(doc, group, outer_isolated);
            assert!(render(&doc, 7).unwrap_err().contains("group buffer budget"));
            let alpha = match (inner_isolated, outer_isolated) {
                (true, true) => 255,
                (false, false) => 64,
                _ => 128,
            };
            assert_eq!(render(&doc, 8).unwrap(), [255, 0, 0, alpha].repeat(4));
        }
    }
}

#[test]
fn sibling_groups_release_their_peak_budget() {
    let mut doc = DrDocument::from_pixels(red());
    let (first, group) = wrapped(doc, 0, true);
    doc = first;
    let mut last = group;
    for _ in 0..8 {
        doc = doc.adding_image_layer(last, red(), "Red").unwrap();
        let (next, group) = wrapped(doc, last + 1, true);
        doc = next;
        last = group;
    }
    assert_eq!(render(&doc, 4).unwrap(), red().into_raw());
}

#[test]
fn clipped_group_member_propagates_failure_and_hidden_group_spends_nothing() {
    let base = DrDocument::from_pixels(red());
    let doc = base.adding_image_layer(0, red(), "Clipped").unwrap();
    let (doc, group) = wrapped(doc, 1, true);
    let doc = doc.with_layer_clipped(group, true).unwrap();
    assert!(render(&doc, 3).unwrap_err().contains("group buffer budget"));
    let hidden = doc.with_layer_visible(group, false).unwrap();
    assert_eq!(render(&hidden, 1).unwrap(), red().into_raw());
}

#[test]
fn failed_projection_keeps_native_save_and_reopen_available() {
    let (doc, _) = wrapped(DrDocument::from_pixels(red()), 0, true);
    assert!(render(&doc, 1).is_err());
    let dir = TempDir::new().unwrap();
    let path = cpath(&dir.path().join("preserved.rz"));
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_doc_save_native(&doc, path.as_ptr(), std::ptr::null(), &mut err) });
    assert!(err.is_null());
    let reopened = unsafe { dr_doc_open(path.as_ptr(), std::ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    assert_eq!(render(unsafe { &*reopened }, 4).unwrap(), red().into_raw());
    assert!(render(unsafe { &*reopened }, 1).is_err());
    unsafe { dr_doc_free(reopened) };
}

#[test]
fn reduced_budget_null_inputs_are_reported_and_null_error_output_is_safe() {
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_doc_flattened(ptr::null(), 1, std::ptr::null(), &mut err) }.is_null());
    assert_eq!(take_err_string(err), "document is NULL");
    let (doc, _) = wrapped(DrDocument::from_pixels(red()), 0, true);
    assert!(unsafe { dr_doc_flattened(&doc, 1, std::ptr::null(), ptr::null_mut()) }.is_null());
    unsafe { dr_string_free(ptr::null_mut()) };
}
