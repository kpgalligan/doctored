//! The BYTE Modify Selection feather and smooth (`dr_selection_feather`,
//! `dr_selection_smooth`, and Refine's smooth and feather stages through
//! `dr_selection_refine`) are bounded as the typed ones are
//! (`bounded_selection_support.rs`): the Gaussian runs over the selection's
//! SUPPORT grown by its half width, and the radius limit is the typed
//! selection's 250.
//!
//! The oracle is `common::feathered_plane`, the header's separable kernel
//! rebuilt from the contract over the WHOLE plane: the bounded result equals
//! it byte for byte (the same tap order and one rounding), for selections in
//! the interior, against an edge, in a corner and over the whole canvas. The
//! bound itself is read from Refine's stage-6 report log: the Gaussian reports
//! twice per row it filters, so the count names the rows, load-independently.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::*;
use doctored_core::ffi_doc::{dr_selection_feather, dr_selection_smooth};
use doctored_core::ffi_selection_tools::dr_selection_refine;

use crate::common;
use common::feathered_plane;
use common::probe::reports;

/// The header's smoothstep remap, applied to the feathered bytes.
fn smoothed_plane(plane: &[u8], w: u32, h: u32, radius: f32) -> Vec<u8> {
    feathered_plane(plane, w, h, radius)
        .into_iter()
        .map(|v| {
            let t = f32::from(v) / 255.0;
            (255.0 * t * t * (3.0 - 2.0 * t)).round() as u8
        })
        .collect()
}

/// A `w`×`h` mask holding a soft-edged block at `(x, y, bw, bh)`: 255 inside
/// with a ramp on its first column and row, so the kernel sees more than one
/// value.
fn block(w: u32, h: u32, (x, y, bw, bh): (u32, u32, u32, u32)) -> Vec<u8> {
    let mut mask = vec![0u8; (w * h) as usize];
    for row in y..y + bh {
        for col in x..x + bw {
            let soft = col == x || row == y;
            mask[(row * w + col) as usize] = if soft {
                96 + (col + row) as u8 % 64
            } else {
                255
            };
        }
    }
    mask
}

#[test]
fn the_bounded_byte_feather_and_smooth_equal_the_whole_plane_kernel() {
    const W: u32 = 96;
    const H: u32 = 80;
    let mut cases: Vec<(&str, Vec<u8>)> = vec![
        ("interior", block(W, H, (40, 30, 12, 14))),
        ("against the left edge", block(W, H, (0, 20, 7, 9))),
        ("in the top-left corner", block(W, H, (0, 0, 5, 6))),
        ("in the bottom-right corner", block(W, H, (88, 71, 8, 9))),
        ("one pixel", block(W, H, (61, 17, 1, 1))),
        (
            "over the whole canvas",
            (0..W * H)
                .map(|i| ((i * 37 + i / W * 11) % 256) as u8)
                .collect(),
        ),
        ("empty", vec![0u8; (W * H) as usize]),
    ];
    let mut two = block(W, H, (6, 8, 4, 4));
    for (i, v) in block(W, H, (70, 60, 9, 5)).into_iter().enumerate() {
        two[i] = two[i].max(v);
    }
    cases.push(("two blocks far apart", two));
    for radius in [0.25f32, 1.0, 2.5, 8.0, 40.0] {
        for (what, mask) in &cases {
            let mut feathered = mask.clone();
            assert!(unsafe { dr_selection_feather(feathered.as_mut_ptr(), W, H, radius) });
            assert!(
                feathered == feathered_plane(mask, W, H, radius),
                "feather {radius}, {what}: differs from the whole-plane kernel"
            );
            let mut smoothed = mask.clone();
            assert!(unsafe { dr_selection_smooth(smoothed.as_mut_ptr(), W, H, radius) });
            assert!(
                smoothed == smoothed_plane(mask, W, H, radius),
                "smooth {radius}, {what}: differs from the whole-plane kernel"
            );
        }
    }
}

#[test]
fn a_radius_above_250_is_refused_with_the_mask_untouched() {
    const W: u32 = 9;
    const H: u32 = 7;
    let mask = block(W, H, (3, 2, 3, 3));
    let above = f32::from_bits(250f32.to_bits() + 1);
    for radius in [250.0f32, above, 250.5, 1e6] {
        for (name, op) in [
            (
                "feather",
                dr_selection_feather as unsafe extern "C" fn(*mut u8, u32, u32, f32) -> bool,
            ),
            ("smooth", dr_selection_smooth),
        ] {
            let mut out = mask.clone();
            let ok = unsafe { op(out.as_mut_ptr(), W, H, radius) };
            if radius <= 250.0 {
                assert!(ok, "{name} {radius} refused");
                assert_ne!(out, mask, "{name} {radius} changed nothing");
            } else {
                assert!(!ok, "{name} {radius} was accepted");
                assert_eq!(out, mask, "{name} {radius} wrote the mask");
            }
        }
    }
}

/// Refine with only a feather is the feather of the mask, and its Gaussian
/// reports twice per row it filters (the horizontal and vertical passes) at
/// stage 6, besides Refine's own three stage-6 reports: a 16×16 selection on
/// a 2048×2048 canvas at feather 8 (half width 8) filters 32 rows, not 2048.
#[test]
fn refines_feather_filters_the_window_not_the_canvas() {
    const W: u32 = 2048;
    const H: u32 = 2048;
    let mask = block(W, H, (1000, 1200, 16, 16));
    let out = std::sync::Mutex::new(Vec::new());
    let log = reports(|token| unsafe {
        let source = dr_image_from_rgba(vec![255u8; (W * H * 4) as usize].as_ptr(), W, H);
        let params = CString::new(r#"{"feather":8}"#).unwrap();
        let mut refined = vec![77u8; mask.len()];
        let ok = dr_selection_refine(
            source,
            mask.as_ptr(),
            ptr::null(),
            params.as_ptr(),
            refined.as_mut_ptr(),
            token as *const _,
            ptr::null_mut(),
        );
        dr_image_free(source);
        *out.lock().unwrap() = refined;
        (ok, String::from("refine failed"))
    });
    let rows = 16 + 2 * 8;
    let stage6 = log.iter().filter(|r| r.stage == 6).count();
    assert_eq!(
        stage6,
        2 * rows + 3,
        "stage-6 reports: the Gaussian filtered {} rows",
        (stage6 - 3) / 2
    );
    assert!(
        *out.lock().unwrap() == feathered_plane(&mask, W, H, 8.0),
        "Refine's feather differs from the whole-plane kernel"
    );
}
