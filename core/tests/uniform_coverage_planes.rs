//! 13B-5: a bitwise-uniform coverage plane (a Reveal All or Hide All mask, a
//! uniform channel, a typed mask set from a uniform buffer) is stored as ONE
//! filled plane: one tile allocation per tile shape, at most four, instead
//! of a whole plane of identical tiles in every history state that adds one.
//!
//! The counts read the app's own resource report
//! (`dr_mixed_resources_report_with_raw_masks`). 1500×1000 is 6×4 tiles with
//! a clipped right column and bottom row, so a filled plane holds exactly the
//! four shapes this file sums itself; a plane with one differing sample is
//! tiled as before, whole.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_channel::{dr_doc_add_channel, dr_doc_channel_plane};
use doctored_core::ffi_doc::{dr_doc_adding_layer_mask, dr_doc_free};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_scope::dr_doc_layer_mask_storage_image;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{img_dims, img_pixels, take_err_string};
use crate::resource_report;

const W: u32 = 1500;
const H: u32 = 1000;

/// The four clipped tile shapes of a `W × H` plane, in samples: the interior
/// 256×256, the right column 220×256, the bottom row 256×232 and the corner
/// 220×232.
fn four_shapes() -> u64 {
    let (right, bottom) = (u64::from(W % 256), u64::from(H % 256));
    256 * 256 + right * 256 + 256 * bottom + right * bottom
}

fn unique_bytes(report: &Value, category: &str) -> u64 {
    report["categories"][category]["unique_bytes"]
        .as_u64()
        .unwrap_or_else(|| panic!("no {category} in {report}"))
}

fn byte_report(documents: &[*mut DrDocument]) -> Value {
    let documents: Vec<*const DrDocument> = documents.iter().map(|d| *d as *const _).collect();
    resource_report::report(&documents, &[], &[], &[], 0, 0).expect("a report")
}

fn base() -> *mut DrDocument {
    Box::into_raw(Box::new(DrDocument::from_pixels(RgbaImage::from_pixel(
        W,
        H,
        Rgba([90, 120, 30, 255]),
    ))))
}

/// Layer `idx`'s stored mask samples, through the FFI.
fn stored_mask(doc: *const DrDocument, idx: usize) -> Vec<u8> {
    let (mut ox, mut oy) = (0, 0);
    let image = unsafe { dr_doc_layer_mask_storage_image(doc, idx, &mut ox, &mut oy) };
    assert!(!image.is_null(), "a stored mask");
    assert_eq!(img_dims(image), (W, H));
    let gray = img_pixels(image).chunks_exact(4).map(|p| p[0]).collect();
    unsafe { dr_image_free(image) };
    gray
}

#[test]
fn reveal_all_and_hide_all_masks_are_filled_planes() {
    let base = base();
    for (kind, value) in [(0, 255_u8), (1, 0)] {
        let masked = unsafe { dr_doc_adding_layer_mask(base, 0, kind, ptr::null(), 0, 0) };
        assert!(!masked.is_null(), "a masked document");
        assert_eq!(
            unique_bytes(&byte_report(&[masked]), "layer_masks"),
            four_shapes(),
            "kind {kind}: one allocation per tile shape"
        );
        assert!(
            stored_mask(masked, 0).iter().all(|&s| s == value),
            "kind {kind}: every sample is {value}"
        );
        unsafe { dr_doc_free(masked) };
    }
    unsafe { dr_doc_free(base) };
}

#[test]
fn a_mask_from_a_varied_selection_is_tiled_whole() {
    let base = base();
    let mut selection = vec![255_u8; W as usize * H as usize];
    selection[W as usize * 600 + 1400] = 7;
    let masked = unsafe { dr_doc_adding_layer_mask(base, 0, 2, selection.as_ptr(), W, H) };
    assert!(!masked.is_null(), "a masked document");
    assert_eq!(
        unique_bytes(&byte_report(&[masked]), "layer_masks"),
        u64::from(W) * u64::from(H),
        "one differing sample keeps every tile its own"
    );
    assert_eq!(stored_mask(masked, 0), selection);
    unsafe { dr_doc_free(masked) };
    unsafe { dr_doc_free(base) };
}

#[test]
fn a_uniform_channel_is_a_filled_plane() {
    let base = base();
    let name = CString::new("Alpha 1").unwrap();
    let plane = vec![255_u8; W as usize * H as usize];
    let with =
        unsafe { dr_doc_add_channel(base, name.as_ptr(), plane.as_ptr(), W, H, 255, 0, 0, 0.5) };
    assert!(!with.is_null(), "a document with a channel");
    assert_eq!(
        unique_bytes(&byte_report(&[with]), "saved_channels"),
        four_shapes()
    );
    let mut out = vec![0_u8; plane.len()];
    assert!(unsafe { dr_doc_channel_plane(with, 0, out.as_mut_ptr(), W, H) });
    assert_eq!(out, plane);
    unsafe { dr_doc_free(with) };
    unsafe { dr_doc_free(base) };
}

/// The typed `set_mask` edit with `samples`, through `dr_precision_doc_edit`.
fn typed_set_mask(doc: *const PrecisionDocument, samples: &[u16]) -> *mut PrecisionDocument {
    let request = CString::new(
        json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0}).to_string(),
    )
    .unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let next = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!next.is_null(), "a typed mask: {}", take_err_string(error));
    next
}

#[test]
fn a_typed_mask_set_from_a_uniform_buffer_is_a_filled_plane() {
    let n = W as usize * H as usize;
    let image = PrecisionImage::from_rgba16(
        W,
        H,
        vec![[9000, 20000, 31000, 65535]; n],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let base = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Default::default(), Default::default()).unwrap(),
    ));
    let uniform = vec![65535_u16; n];
    let mut varied = uniform.clone();
    varied[n - 1] = 3;
    for (samples, want) in [(&uniform, four_shapes() * 2), (&varied, n as u64 * 2)] {
        let masked = typed_set_mask(base, samples);
        let report =
            resource_report::report(&[], &[], &[masked as *const _], &[], 0, 0).expect("a report");
        assert_eq!(unique_bytes(&report, "precision_plane"), want);
        let document = unsafe { &*masked };
        let mask = document
            .layer(0)
            .and_then(|layer| layer.mask())
            .expect("a typed mask");
        assert_eq!(&mask.plane().samples(), samples);
        drop(unsafe { Box::from_raw(masked) });
    }
    drop(unsafe { Box::from_raw(base) });
}
