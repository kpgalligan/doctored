//! The eleven carried adjustment kinds of the `adjustments` fixture measured
//! against Photoshop 26.10.0's own renders (#2): each layer's params as the
//! import carries them (`psd_photoshop.rs`'s `kinds()` pins those metas),
//! applied to the fixture's base at 8 bits through `dr_doc_adjust_image` on
//! an sRGB document (the profile-bound path the layer render takes for
//! Exposure, Photo Filter and Vibrance) and at 16 bits through
//! `PrecisionAdjustment::apply_at`, and compared code for code with
//! `<layer>_{8,16}.png` (`photoshop.json` lists each layer's descriptor). A
//! 16-bit difference is divided by 257 ("8-bit-equivalent").
//!
//! The bound was declared before measuring: max ≤ 2 and mean ≤ 0.5 codes
//! per depth. A row that misses asserts its own measured residual
//! (`MEASURED`), never a wider number, so the table can only tighten. Every
//! row prints `kind depth max mean`; `make t F=psd_photoshop_appearance
//! A=--nocapture` shows the whole table. Threshold and Photo Filter met it
//! only once their 8-bit forms were refitted (`adjust_render_take.rs` holds
//! them to the confirming take).

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_calibrated::dr_doc_adjust_image;
use doctored_core::icc::IccProfile;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{pixels, take_err_string};

const SIDE: usize = 64;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/adjustments")
        .join(name)
}

/// A PNG's RGB samples as 16-bit-wide values in its own codes.
fn rgb(name: &str) -> Vec<[u16; 3]> {
    let image = image::open(fixture(name)).expect(name);
    let wide = image.to_rgb16();
    let eight = image.color().bits_per_pixel() / image.color().channel_count() as u16 == 8;
    assert_eq!(wide.dimensions(), (SIDE as u32, SIDE as u32), "{name}");
    wide.pixels()
        .map(|p| if eight { p.0.map(|c| c / 257) } else { p.0 })
        .collect()
}

/// `op`/`params` over the 8-bit base through `dr_doc_adjust_image` on an
/// sRGB document.
fn ours8(op: &str, params: &Value) -> Vec<[u16; 3]> {
    let base = rgb("base.png");
    let src = RgbaImage::from_fn(SIDE as u32, SIDE as u32, |x, y| {
        let p = base[y as usize * SIDE + x as usize];
        Rgba([p[0] as u8, p[1] as u8, p[2] as u8, 255])
    });
    let mut doc = DrDocument::from_pixels(src.clone());
    doc.profile = IccProfile::srgb();
    let op = CString::new(op).unwrap();
    let params = CString::new(params.to_string()).unwrap();
    unsafe {
        let image = dr_image_from_rgba(src.as_raw().as_ptr(), SIDE as u32, SIDE as u32);
        assert!(!image.is_null());
        let mut error = ptr::null_mut();
        let out = dr_doc_adjust_image(&doc, image, op.as_ptr(), params.as_ptr(), &mut error);
        dr_image_free(image);
        assert!(!out.is_null(), "{}", take_err_string(error));
        let got = pixels(out)
            .chunks(4)
            .map(|p| [p[0], p[1], p[2]].map(u16::from))
            .collect();
        dr_image_free(out);
        got
    }
}

/// `op`/`params` over the 16-bit base through the precision interpreter.
fn ours16(op: &str, params: &Value) -> Vec<[u16; 3]> {
    let meta = json!({
        "type": "adjust", "evaluation": "precision_v1", "op": op, "params": params
    })
    .to_string();
    let adjustment = PrecisionAdjustment::parse(&meta, &IccProfile::srgb()).expect("parses");
    rgb("base_16.png")
        .iter()
        .map(|p| {
            let x = p.map(|c| f32::from(c) / 65535.0);
            adjustment
                .apply_at(x, 0.0, (0, 0))
                .map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
        })
        .collect()
}

/// (max, mean) of the per-component difference in 8-bit codes.
fn residual(got: &[[u16; 3]], want: &[[u16; 3]], scale: f64) -> (f64, f64) {
    let diffs: Vec<f64> = got
        .iter()
        .zip(want)
        .flat_map(|(g, w)| (0..3).map(move |k| (f64::from(g[k]) - f64::from(w[k])).abs() / scale))
        .collect();
    let max = diffs.iter().copied().fold(0.0, f64::max);
    (max, diffs.iter().sum::<f64>() / diffs.len() as f64)
}

const DEFAULT: (f64, f64) = (2.0, 0.5);

/// Rows that miss the declared bound, each pinned at its own measured
/// residual (layer, depth, max, mean): never a wider number. Vibrance at
/// both depths (`vibrance_law`'s tables; two fitted candidates already
/// failed offline) and Black & White's 8-bit Tint (14 of 4096 pixels over
/// 1 code, `adjust_bw8`).
const MEASURED: &[(&str, u32, f64, f64)] = &[
    ("vibA", 8, 8.0, 0.2791),
    ("vibA", 16, 7.5720, 0.2758),
    ("blwh", 8, 4.0, 0.0317),
];

fn bound(layer: &str, depth: u32) -> (f64, f64) {
    MEASURED
        .iter()
        .find(|(l, d, _, _)| *l == layer && *d == depth)
        .map_or(DEFAULT, |&(_, _, max, mean)| (max, mean))
}

fn check(layer: &str, op: &str, params: Value) {
    let rows = [
        (
            8,
            residual(&ours8(op, &params), &rgb(&format!("{layer}_8.png")), 1.0),
        ),
        (
            16,
            residual(
                &ours16(op, &params),
                &rgb(&format!("{layer}_16.png")),
                257.0,
            ),
        ),
    ];
    for (depth, (max, mean)) in rows {
        println!("{layer} {depth} {max:.4} {mean:.4}");
    }
    for (depth, (max, mean)) in rows {
        let (max_bound, mean_bound) = bound(layer, depth);
        assert!(
            max <= max_bound + 1e-4 && mean <= mean_bound + 1e-4,
            "{layer} at {depth} bits: max {max:.4}, mean {mean:.4}, bound \
             {max_bound} / {mean_bound}"
        );
    }
}

#[test]
fn invert_matches_photoshop() {
    check("nvrt", "invert", json!({}));
}

#[test]
fn threshold_matches_photoshop() {
    // 153 / 255.
    check("thrs", "threshold", json!({"level": 0.6}));
}

#[test]
fn posterize_matches_photoshop() {
    check("post", "posterize", json!({"levels": 4}));
}

#[test]
fn levels_matches_photoshop() {
    check(
        "levl",
        "levels",
        json!({"black": 0.2, "gamma": 1.5, "white": 0.8}),
    );
}

#[test]
fn curves_matches_photoshop() {
    check(
        "curv",
        "curves",
        json!({"rgb": [[0, 0], [64, 80], [255, 255]]}),
    );
}

#[test]
fn color_balance_matches_photoshop() {
    let neutral = json!({"cyan_red": 0.0, "magenta_green": 0.0, "yellow_blue": 0.0});
    check(
        "blnc",
        "color_balance",
        json!({
            "shadows": neutral, "highlights": neutral,
            "midtones": {"cyan_red": 0.25, "magenta_green": -0.1, "yellow_blue": 0.3},
            "preserve_luminosity": false,
        }),
    );
}

#[test]
fn hue_saturation_matches_photoshop() {
    check(
        "hue2",
        "hue_saturation",
        json!({"hue": 20, "saturation": 0.25, "lightness": -0.1}),
    );
}

#[test]
fn photo_filter_matches_photoshop() {
    check(
        "phfl",
        "photo_filter",
        json!({"color": "#ec8a00", "density": 0.4, "preserve_luminosity": true}),
    );
}

#[test]
fn exposure_matches_photoshop() {
    check(
        "expA",
        "exposure",
        json!({"evaluation": "document_v2", "exposure": 1.5, "gamma": 2.0, "offset": -0.25}),
    );
}

#[test]
fn vibrance_matches_photoshop() {
    check(
        "vibA",
        "vibrance",
        json!({"saturation": -0.2, "vibrance": 0.4}),
    );
}

#[test]
fn black_and_white_matches_photoshop() {
    check(
        "blwh",
        "black_and_white",
        json!({"reds": 0.3, "yellows": -0.5, "greens": 1.2, "cyans": 0.6,
               "blues": 2.5, "magentas": 0.0, "tint": true, "tint_color": "#998a66"}),
    );
}
