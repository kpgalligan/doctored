//! Replace Color through `dr_image_adjust_op` and the layer. The
//! oracle restates the schema row in binary64: `d` is the Euclidean
//! distance in 0…255 code units to the nearest colour, the weight is 1 up
//! to `f/2`, `2(f − d)/f` to `f`, 0 beyond; the pixel moves that fraction
//! of the way to `hue_saturation` with the same sliders (whose own oracle
//! is `adjust_color_tests`'s, so here it is read off the destructive
//! `hue_saturation` op as the full-weight reference).

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi::*;
use doctored_core::ffi_adjust::*;
use doctored_core::ffi_doc::*;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::*;

fn try_adjust(img: *const DrImage, op: &str, params: &Value) -> Result<Vec<u8>, String> {
    let op = CString::new(op).unwrap();
    let text = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(img, op.as_ptr(), text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let bytes = img_pixels(out);
    unsafe { dr_image_free(out) };
    Ok(bytes)
}

fn weight(d: f64, f: f64) -> f64 {
    if d <= f / 2.0 {
        1.0
    } else if d < f {
        2.0 * (f - d) / f
    } else {
        0.0
    }
}

/// Greys stepping away from the sampled grey 100 along the diagonal: each
/// step of `t` in all three channels is a distance of `t·√3`.
fn ladder(steps: &[f64]) -> (RgbaImage, Vec<f64>) {
    let levels: Vec<u8> = steps
        .iter()
        .map(|d| (100.0 + d / 3f64.sqrt()).round() as u8)
        .collect();
    let img = RgbaImage::from_fn(levels.len() as u32, 1, |x, _| {
        let v = levels[x as usize];
        Rgba([v, v, v, 255])
    });
    let distances = levels
        .iter()
        .map(|&v| (f64::from(v) - 100.0) * 3f64.sqrt())
        .collect();
    (img, distances)
}

#[test]
fn the_weight_is_full_to_half_fuzziness_then_falls_linearly_to_zero() {
    let dir = TempDir::new().unwrap();
    let f = 80.0;
    // d = 0, f/2, ¾f, f and beyond, plus points inside each segment.
    let (src, distances) = ladder(&[0.0, 20.0, 40.0, 50.0, 60.0, 70.0, 80.0, 120.0]);
    let img = open_image(&dir, "ladder.png", &src);
    let sliders = json!({"lightness": 0.6});
    let full = try_adjust(img, "hue_saturation", &sliders).unwrap();
    let got = try_adjust(
        img,
        "replace_color",
        &json!({"colors": ["#646464"], "fuzziness": f, "lightness": 0.6}),
    )
    .unwrap();
    for (i, d) in distances.iter().enumerate() {
        let w = weight(*d, f);
        let before = f64::from(src.as_raw()[i * 4]);
        let after = f64::from(full[i * 4]);
        let want = before + w * (after - before);
        let have = f64::from(got[i * 4]);
        assert!(
            (have - want).abs() <= 1.0,
            "d {d:.2}: w {w:.3}, got {have}, want {want:.2}"
        );
        assert_eq!(got[i * 4 + 3], 255, "alpha");
    }
    // The ends exactly: d = 0 is the full edit, d ≥ f untouched.
    assert_eq!(got[..4], full[..4]);
    assert_eq!(got[7 * 4..], src.as_raw()[7 * 4..]);
    unsafe { dr_image_free(img) };
}

#[test]
fn zero_fuzziness_matches_only_the_exact_colour_and_the_nearest_colour_wins() {
    let dir = TempDir::new().unwrap();
    let src = RgbaImage::from_fn(4, 1, |x, _| match x {
        0 => Rgba([200, 30, 30, 255]),
        1 => Rgba([201, 30, 30, 255]),
        2 => Rgba([30, 30, 200, 255]),
        _ => Rgba([30, 200, 30, 255]),
    });
    let img = open_image(&dir, "exact.png", &src);
    let full = try_adjust(img, "hue_saturation", &json!({"hue": 120})).unwrap();
    let got = try_adjust(
        img,
        "replace_color",
        &json!({"colors": ["#c81e1e", "#1e1ec8"], "fuzziness": 0, "hue": 120}),
    )
    .unwrap();
    assert_eq!(got[..4], full[..4], "the exact red takes the edit");
    assert_eq!(
        got[4..8],
        src.as_raw()[4..8],
        "one code off is outside f = 0"
    );
    assert_eq!(got[8..12], full[8..12], "the second colour matches too");
    assert_eq!(got[12..], src.as_raw()[12..], "green matches neither");
    unsafe { dr_image_free(img) };
}

#[test]
fn colors_are_required_and_malformed_params_refuse() {
    let dir = TempDir::new().unwrap();
    let img = open_image(&dir, "bad.png", &solid(1, 1, [128, 128, 128, 255]));
    let nine: Vec<&str> = vec!["#000000"; 9];
    for bad in [
        json!({}),
        json!({"colors": []}),
        json!({"colors": nine}),
        json!({"colors": "#808080"}),
        json!({"colors": ["808080"]}),
        json!({"colors": ["#80808"]}),
        json!({"colors": [128]}),
        json!({"colors": ["#808080"], "fuzziness": 201}),
        json!({"colors": ["#808080"], "fuzziness": -1}),
        json!({"colors": ["#808080"], "hue": 181}),
        json!({"colors": ["#808080"], "saturation": 1.5}),
        json!({"colors": ["#808080"], "lightness": "up"}),
    ] {
        let err = try_adjust(img, "replace_color", &bad).unwrap_err();
        assert!(err.contains("replace_color"), "{bad}: {err}");
    }
    // Eight colours and the edge values parse.
    let eight: Vec<&str> = vec!["#808080"; 8];
    let params = json!({"colors": eight, "fuzziness": 200, "hue": -180, "saturation": -1});
    assert!(try_adjust(img, "replace_color", &params).is_ok());
    unsafe { dr_image_free(img) };
}

#[test]
fn the_8_bit_layer_is_byte_identical_to_the_filter() {
    let dir = TempDir::new().unwrap();
    let src = opaque_pattern(64, 64);
    let params = r##"{"colors":["#204060","#c08040"],"fuzziness":90,"hue":40,"saturation":0.3,"lightness":-0.1}"##;
    let doc = doc_from(&dir, "rc-bg.png", &src);
    let doc = add_layer(&dir, "rc-top.png", doc, 0, &solid(2, 2, MAGENTA), "Adjust");
    let doc = set_meta(doc, 1, &adjust_meta("replace_color", params));
    let img = open_image(&dir, "rc-ref.png", &src);
    let filtered =
        try_adjust(img, "replace_color", &serde_json::from_str(params).unwrap()).unwrap();
    assert_ne!(filtered, *src.as_raw(), "the case must change something");
    assert_eq!(flat_pixels(doc), filtered);
    unsafe { dr_image_free(img) };
    unsafe { dr_doc_free(doc) };
}
