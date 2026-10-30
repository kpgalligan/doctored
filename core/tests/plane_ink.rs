//! At 8 and 16 bits, the gray Fill and Stroke write into ONE colour
//! channel of a layer (`dr_plane_ink_u8`, `dr_plane_ink_u16`), against
//! Photoshop's own read values (`scripts/photoshop-probe/plane_gray.py`,
//! receipt `plane-gray-431-633/take1`) and an oracle written here in f64:
//! `0.30 r + 0.59 g + 0.11 b`, rounded to a code at 8 bits and unrounded
//! until the one 16-bit quantization at 16.
//!
//! The take's `soft8` and `soft16` verdicts are `written`: neither write is
//! weighted by the pixel's own transparency. The two paths the app fills a
//! colour plane through (the scalar `paint` kind and `dr_doc_apply_layer_plane`)
//! are pinned unweighted over half-alpha and transparent pixels.

use crate::common;
use crate::native_scalar as native;

use std::ptr;

use common::layer_pixels;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_scope::dr_doc_apply_layer_plane;
use doctored_core::ffi_selection_u16::{dr_plane_ink_u16, dr_plane_ink_u8};
use image::RgbaImage;
use native::{doc, error, scalar};
use serde_json::json;

/// The take's swatches Q1..Q3 with the code read in the filled channel at
/// 8 bits and the 16-bit sample read there at 16 (`read` × 65535).
const TAKE: [([u8; 3], u8, u16); 3] = [
    ([0, 0, 255], 28, 7208),
    ([250, 10, 0], 81, 20792),
    ([90, 200, 40], 149, 38395),
];
/// The take's T16, 2/32768, in 16-bit codes.
const T16_CODES: u16 = 4;

fn ink8(s: [u8; 3]) -> u8 {
    let mut out = 0;
    assert!(unsafe { dr_plane_ink_u8(s[0], s[1], s[2], &mut out, ptr::null_mut()) });
    out
}

fn ink16(rgb: [f64; 3]) -> u16 {
    let mut out = 0;
    let mut e = ptr::null_mut();
    let ok = unsafe { dr_plane_ink_u16(rgb[0], rgb[1], rgb[2], &mut out, &mut e) };
    assert!(ok, "{}", unsafe { error(e) });
    out
}

/// The independent luma over 8-bit codes, in code units.
fn luma(s: [u8; 3]) -> f64 {
    0.30 * f64::from(s[0]) + 0.59 * f64::from(s[1]) + 0.11 * f64::from(s[2])
}

#[test]
fn the_exports_read_what_photoshop_wrote() {
    for (swatch, code, sample) in TAKE {
        assert_eq!(ink8(swatch), code, "{swatch:?}");
        let got = ink16(swatch.map(|c| f64::from(c) / 255.0));
        assert!(got.abs_diff(sample) <= T16_CODES, "{swatch:?}: {got}");
    }
}

#[test]
fn a_swatch_grid_matches_the_f64_luma() {
    let steps: Vec<u8> = (0..=15).map(|i| i * 17).collect();
    let mut differs_from_floor = 0;
    for &r in &steps {
        for &g in &steps {
            for &b in &steps {
                let l = luma([r, g, b]);
                // Half up; the nudge keeps an exact .5 from landing below it
                // in f64 (a tie is unmeasured: the kernel's choice is up).
                let want = (l + 0.5 + 1e-9).floor() as u8;
                assert_eq!(ink8([r, g, b]), want, "{r} {g} {b}");
                differs_from_floor += usize::from(want != l.floor() as u8);
                let unit = [r, g, b].map(|c| f64::from(c) / 255.0);
                let exact = (l / 255.0 * 65535.0).round();
                let got = f64::from(ink16(unit));
                assert!((got - exact).abs() <= 1.0, "{r} {g} {b}: {got} vs {exact}");
            }
        }
    }
    assert!(
        differs_from_floor > 1000,
        "the grid must tell rounding from flooring"
    );
    assert_eq!(ink8([255, 255, 255]), 255);
    assert_eq!(ink16([1.0, 1.0, 1.0]), 65535);
    assert_eq!(ink16([0.0, 0.0, 0.0]), 0);
    // Sub-byte ink survives: the 16-bit rule does not round to an 8-bit code.
    let fine = 12345.0 / 65535.0;
    assert_eq!(ink16([fine, fine, fine]), 12345);
}

#[test]
fn invalid_ink_refuses_without_output() {
    unsafe {
        for bad in [f64::NAN, f64::INFINITY, -0.001, 1.001] {
            for slot in 0..3 {
                let mut rgb = [0.5; 3];
                rgb[slot] = bad;
                let mut out = 999;
                let mut e = ptr::null_mut();
                assert!(!dr_plane_ink_u16(rgb[0], rgb[1], rgb[2], &mut out, &mut e));
                assert_eq!(out, 999);
                assert_eq!(
                    error(e),
                    "colour plane ink RGB must be finite and within [0,1]"
                );
                assert!(!dr_plane_ink_u16(
                    rgb[0],
                    rgb[1],
                    rgb[2],
                    &mut out,
                    ptr::null_mut()
                ));
            }
        }
        let mut e = ptr::null_mut();
        assert!(!dr_plane_ink_u16(0., 0., 0., ptr::null_mut(), &mut e));
        assert_eq!(error(e), "colour plane ink output is null");
        assert!(!dr_plane_ink_u16(
            0.,
            0.,
            0.,
            ptr::null_mut(),
            ptr::null_mut()
        ));
        let mut e = ptr::null_mut();
        assert!(!dr_plane_ink_u8(1, 2, 3, ptr::null_mut(), &mut e));
        assert_eq!(error(e), "colour plane ink output is null");
        assert!(!dr_plane_ink_u8(1, 2, 3, ptr::null_mut(), ptr::null_mut()));
    }
}

/// Row S16 (`soft16` `written`): the scalar `paint` on Red writes
/// `old + (v − old)·t` whatever the pixel's stored alpha, alpha 0 included.
#[test]
fn a_16_bit_plane_paint_is_not_weighted_by_alpha() {
    let pixels = vec![
        [40000, 11, 12, 65535],
        [40000, 21, 22, 32768],
        [40000, 31, 32, 0],
        [40000, 41, 42, 32768],
    ];
    let d = doc(4, 1, pixels.clone());
    let value = ink16(TAKE[0].0.map(|c| f64::from(c) / 255.0));
    let coverage = [65535, 65535, 65535, 32768];
    let next = scalar(
        &d,
        "red",
        json!({"kind":"paint","value":value,"opacity":1.0}),
        false,
        &coverage,
    )
    .unwrap();
    let out = next.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    for (i, before) in pixels.iter().enumerate() {
        let t = f64::from(coverage[i]) / 65535.0;
        let want = f64::from(before[0]) + (f64::from(value) - f64::from(before[0])) * t;
        assert!(
            (f64::from(out[i][0]) - want).abs() <= 1.0,
            "pixel {i}: {} vs {want}",
            out[i][0]
        );
        assert_eq!(out[i][1..], before[1..], "pixel {i}");
    }
    assert_eq!(out[0][0], value);
    assert_eq!(out[1][0], value);
    assert_eq!(out[2][0], value);
}

/// Row S8 (`soft8` `written`): the 8-bit plane writer takes the filled gray
/// whatever the pixel's stored alpha, alpha 0 included.
#[test]
fn an_8_bit_plane_write_is_not_weighted_by_alpha() {
    let before: [[u8; 4]; 4] = [
        [200, 11, 12, 255],
        [200, 21, 22, 128],
        [200, 31, 32, 0],
        [200, 41, 42, 128],
    ];
    let src = RgbaImage::from_raw(4, 1, before.concat()).unwrap();
    let doc = DrDocument::from_pixels(src);
    let gray = ink8(TAKE[0].0);
    let filled = [gray, gray, gray, 255].repeat(4);
    let result = unsafe { dr_image_from_rgba(filled.as_ptr(), 4, 1) };
    assert!(!result.is_null());
    let selection = [255u8, 255, 255, 128];
    let out = unsafe { dr_doc_apply_layer_plane(&doc, 0, 0, result, selection.as_ptr(), 4, 1) };
    assert!(!out.is_null());
    for (i, actual) in layer_pixels(out, 0).chunks_exact(4).enumerate() {
        let t = f64::from(selection[i]) / 255.0;
        let want = f64::from(before[i][0]) + (f64::from(gray) - f64::from(before[i][0])) * t;
        assert!(
            (f64::from(actual[0]) - want).abs() <= 1.0,
            "pixel {i}: {} vs {want}",
            actual[0]
        );
        assert_eq!(actual[1..], before[i][1..], "pixel {i}");
    }
    assert_eq!(layer_pixels(out, 0)[0], gray);
    assert_eq!(layer_pixels(out, 0)[4], gray);
    assert_eq!(layer_pixels(out, 0)[8], gray);
    unsafe {
        dr_doc_free(out);
        dr_image_free(result);
    }
}
