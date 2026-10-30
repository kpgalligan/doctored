//! The float Smudge plan's write term: what `with_rect_rgba32` can allocate
//! writing the stroke's footprint back, as `PrecisionImage::rect_write_pixels`
//! states it and `dr_precision_doc_smudge_plan` charges it.
//!
//! Every expected figure is counted by hand from the tile grid (256-pixel
//! tiles, the last column and row clipped to the plane); none is a measured
//! footprint. The measured record is `float_retouch_brush`'s module doc and
//! its floors are `selected_color_planes_24mp.rs`'s.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_retouch_brush::dr_precision_doc_smudge_plan;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::Value;

use crate::float_fixture::float_doc;

const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

fn opaque(w: u32, h: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| [(i % w) as f32 / 64.0, (i / w) as f32 / 64.0, 0.5, 1.0])
        .collect()
}

fn document(w: u32, h: u32) -> PrecisionDocument {
    let d = float_doc(w, h, &IccProfile::srgb(), &[opaque(w, h)]);
    let layer = d.layer(0).unwrap();
    assert_eq!(layer.source().unwrap().dimensions(), (w, h));
    assert_eq!(layer.offset(), (0, 0));
    d
}

/// `scratch_bytes` of the smudge plan for a two-dab stroke.
fn planned(d: &PrecisionDocument, dabs: [[f64; 4]; 2]) -> u64 {
    let brush = CString::new(HARD).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut e: *mut c_char = ptr::null_mut();
    let p = unsafe {
        dr_precision_doc_smudge_plan(
            d,
            0,
            brush.as_ptr(),
            flat.as_ptr(),
            2,
            false,
            false,
            REFERENCE_WORKING_BYTES,
            0,
            &mut e,
        )
    };
    assert!(e.is_null() && !p.is_null(), "the plan was expected");
    let plan: Value = serde_json::from_slice(unsafe { CStr::from_ptr(p) }.to_bytes()).unwrap();
    unsafe { dr_string_free(p) };
    plan["scratch_bytes"].as_u64().unwrap()
}

#[test]
fn a_rect_write_is_the_tiles_it_touches_clipped_to_the_plane() {
    // 768 × 768: a 3 × 3 grid of whole tiles.
    let d = document(768, 768);
    let image = d.layer_source(0).unwrap();
    // Inside tile (0, 0).
    assert_eq!(image.rect_write_pixels(40, 40, 80, 40), 256 * 256);
    // Across the (256, 256) corner: four tiles.
    assert_eq!(image.rect_write_pixels(216, 236, 80, 40), 512 * 512);
    // One pixel either side of a tile column's edge: two tiles.
    assert_eq!(image.rect_write_pixels(255, 10, 2, 1), 512 * 256);
    // Ending exactly on a tile edge stays inside the tile.
    assert_eq!(image.rect_write_pixels(200, 200, 56, 56), 256 * 256);
    // The right and bottom edge: the last tile.
    assert_eq!(image.rect_write_pixels(700, 700, 68, 68), 256 * 256);
    assert_eq!(image.rect_write_pixels(0, 0, 768, 768), 768 * 768);
    // A zero area and a rect leaving the plane write nothing.
    assert_eq!(image.rect_write_pixels(10, 10, 0, 5), 0);
    assert_eq!(image.rect_write_pixels(10, 10, 5, 0), 0);
    assert_eq!(image.rect_write_pixels(700, 700, 69, 10), 0);
    assert_eq!(image.rect_write_pixels(700, 700, 10, 69), 0);
    assert_eq!(image.rect_write_pixels(u32::MAX, 0, 2, 2), 0);

    // 700 × 600: the last column is 188 wide and the last row 88 high.
    let d = document(700, 600);
    let image = d.layer_source(0).unwrap();
    assert_eq!(image.rect_write_pixels(600, 520, 100, 80), 188 * 88);
    assert_eq!(image.rect_write_pixels(500, 100, 20, 20), (700 - 256) * 256);
    assert_eq!(image.rect_write_pixels(100, 500, 20, 20), 256 * (600 - 256));
    assert_eq!(image.rect_write_pixels(0, 0, 1, 1), 256 * 256);
    assert_eq!(image.rect_write_pixels(0, 0, 700, 600), 700 * 600);
}

#[test]
fn a_contiguous_plane_is_rewritten_whole() {
    let (w, h) = (300_u32, 200_u32);
    let image =
        PrecisionImage::contiguous_rgba32_extended(w, h, opaque(w, h), IccProfile::srgb()).unwrap();
    assert_eq!(image.rect_write_pixels(10, 10, 4, 4), 300 * 200);
    assert_eq!(image.rect_write_pixels(0, 0, 300, 200), 300 * 200);
    assert_eq!(image.rect_write_pixels(10, 10, 0, 4), 0);
    assert_eq!(image.rect_write_pixels(298, 10, 4, 4), 0);
}

#[test]
fn the_smudge_plan_charges_every_tile_the_footprint_touches() {
    let d = document(768, 768);
    // A hard size-40 tip moved 40 px: an 80 × 40 footprint either way.
    let inside = [[60.0, 60.0, 40.0, 1.0], [100.0, 60.0, 40.0, 1.0]];
    let across = [[236.0, 256.0, 40.0, 1.0], [276.0, 256.0, 40.0, 1.0]];
    let image = d.layer_source(0).unwrap();
    let one = image.rect_write_pixels(40, 40, 80, 40);
    let four = image.rect_write_pixels(216, 236, 80, 40);
    assert_eq!((one, four), (256 * 256, 512 * 512));
    let (small, large) = (planned(&d, inside), planned(&d, across));
    // The same footprint area, stage and dab count: the plans differ by the
    // write term alone, at least an f32 pixel (16 B) for each extra tile pixel.
    assert!(
        large > small && large - small >= 16 * (four - one),
        "{large} against {small}: {} extra tile pixels",
        four - one
    );
    // The same stroke moved inside its tile plans the same.
    let moved = [[70.0, 90.0, 40.0, 1.0], [110.0, 90.0, 40.0, 1.0]];
    assert_eq!(planned(&d, moved), small);
}
