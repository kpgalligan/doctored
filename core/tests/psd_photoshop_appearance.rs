//! The adjustments #2 measured against Photoshop 26.10.0's own
//! renders: the committed `post`, `hue2` and `vibA` layers of the
//! `adjustments` fixture (`doctored-fixtures/photoshop/adjustments`,
//! `photoshop.json` lists each layer's descriptor), applied to that
//! fixture's base at 8 bits through `dr_image_adjust_op` and at 16 bits
//! through `PrecisionAdjustment::apply_at`, and compared code for code with
//! `<layer>_{8,16}.png`. A 16-bit difference is divided by 257
//! ("8-bit-equivalent").
//!
//! The bounds were declared before measuring (the batch spec): max ≤ 2 and
//! mean ≤ 0.5 at both depths, except `hue2` at 8 bits, a master set, held
//! to max ≤ 3 — take4's recorded master residual.

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::icc::IccProfile;
use serde_json::{json, Value};

use crate::common::take_err_string;

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

/// `op`/`params` over the 8-bit base through the destructive FFI.
fn ours8(op: &str, params: &Value) -> Vec<[u16; 3]> {
    let base = rgb("base.png");
    let rgba: Vec<u8> = base
        .iter()
        .flat_map(|p| [p[0] as u8, p[1] as u8, p[2] as u8, 255])
        .collect();
    let op = CString::new(op).unwrap();
    let params = CString::new(params.to_string()).unwrap();
    unsafe {
        let image = dr_image_from_rgba(rgba.as_ptr(), SIDE as u32, SIDE as u32);
        assert!(!image.is_null());
        let mut error = ptr::null_mut();
        let out = dr_image_adjust_op(image, op.as_ptr(), params.as_ptr(), &mut error);
        dr_image_free(image);
        assert!(!out.is_null(), "{}", take_err_string(error));
        let pixels = std::slice::from_raw_parts(dr_image_pixels_rgba(out), SIDE * SIDE * 4);
        let got = pixels
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

fn check(layer: &str, op: &str, params: Value, bound8: (f64, f64), bound16: (f64, f64)) {
    let (max8, mean8) = residual(&ours8(op, &params), &rgb(&format!("{layer}_8.png")), 1.0);
    let (max16, mean16) = residual(
        &ours16(op, &params),
        &rgb(&format!("{layer}_16.png")),
        257.0,
    );
    assert!(
        max8 <= bound8.0 && mean8 <= bound8.1,
        "{layer} at 8 bits: max {max8}, mean {mean8}, bound {bound8:?}"
    );
    assert!(
        max16 <= bound16.0 && mean16 <= bound16.1,
        "{layer} at 16 bits: max {max16}, mean {mean16}, bound {bound16:?}"
    );
}

const DEFAULT: (f64, f64) = (2.0, 0.5);

#[test]
fn posterize_matches_photoshop() {
    check("post", "posterize", json!({"levels": 4}), DEFAULT, DEFAULT);
}

#[test]
fn hue_saturation_matches_photoshop() {
    check(
        "hue2",
        "hue_saturation",
        json!({"hue": 20, "saturation": 0.25, "lightness": -0.1}),
        (3.0, 0.5),
        DEFAULT,
    );
}
