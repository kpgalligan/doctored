//! Brightness/Contrast's modern (`legacy: false`) form beside the
//! legacy one. The oracle is the modern formula restated here in binary64
//! from the schema row in `adjust`'s module doc — brightness `x^g`, `g =
//! 2^(-1.5·b)`; contrast `x < 0.5 ? 0.5·(2x)^k : 1 − 0.5·(2(1−x))^k`, `k =
//! 2^(1.5·c)`; then the luma-anchored saturation scale `1 + s` — never read
//! off the implementation. Legacy is pinned against itself: an absent
//! `legacy` and `legacy: true` give the same bytes, on the filter and the
//! layer, and both stay within one code of the frozen `dr_image_adjust`.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::ffi::*;
use doctored_core::ffi_adjust::*;
use doctored_core::ffi_doc::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::json;
use tempfile::TempDir;

use crate::common;
use common::*;

/// The modern form in binary64 for one channel triple in [0, 1].
fn modern(b: f64, c: f64, s: f64, rgb: [f64; 3]) -> [f64; 3] {
    let g = (-1.5 * b).exp2();
    let k = (1.5_f64 * c).exp2();
    let o = rgb.map(|x| {
        let y = if b == 0.0 { x } else { x.powf(g) };
        if c == 0.0 {
            y
        } else if y < 0.5 {
            0.5 * (2.0 * y).powf(k)
        } else {
            1.0 - 0.5 * (2.0 * (1.0 - y)).powf(k)
        }
    });
    let a = 0.2126 * o[0] + 0.7152 * o[1] + 0.0722 * o[2];
    o.map(|v| a + (v - a) * (1.0 + s))
}

/// The legacy affine in binary64 (`ops::adjust`'s statements).
fn legacy(b: f64, c: f64, s: f64, rgb: [f64; 3]) -> [f64; 3] {
    let o = rgb.map(|v| (v + b - 0.5) * (1.0 + c) + 0.5);
    let a = 0.2126 * o[0] + 0.7152 * o[1] + 0.0722 * o[2];
    o.map(|v| a + (v - a) * (1.0 + s))
}

fn adjust(img: *const DrImage, params: &serde_json::Value) -> Vec<u8> {
    let op = CString::new("bcs").unwrap();
    let text = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(img, op.as_ptr(), text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{params}: {}", take_err_string(err));
    let bytes = img_pixels(out);
    unsafe { dr_image_free(out) };
    bytes
}

/// Every grey level, then a spread of colours.
fn pattern() -> RgbaImage {
    RgbaImage::from_fn(256, 2, |x, y| {
        let v = x as u8;
        if y == 0 {
            Rgba([v, v, v, 255])
        } else {
            Rgba([v, v.wrapping_mul(7), 255 - v, 255])
        }
    })
}

fn within_one(got: &[u8], src: &RgbaImage, oracle: impl Fn([f64; 3]) -> [f64; 3], what: &str) {
    for (px, (g, s)) in got.chunks_exact(4).zip(src.pixels()).enumerate() {
        let want = oracle([s[0], s[1], s[2]].map(|v| f64::from(v) / 255.0));
        for c in 0..3 {
            let w = (want[c].clamp(0.0, 1.0) * 255.0).round();
            assert!(
                (f64::from(g[c]) - w).abs() <= 1.0,
                "{what}: pixel {px} channel {c}: {} vs {w}",
                g[c]
            );
        }
        assert_eq!(g[3], s[3], "{what}: alpha");
    }
}

const SETTINGS: [[f64; 3]; 6] = [
    [0.3, 0.0, 0.0],
    [-0.6, 0.0, 0.0],
    [0.0, 0.5, 0.0],
    [0.0, -0.8, 0.0],
    [0.25, 0.4, -0.3],
    [-1.0, 1.0, 0.5],
];

#[test]
fn an_absent_legacy_and_legacy_true_are_the_frozen_bytes() {
    let dir = TempDir::new().unwrap();
    let src = pattern();
    let img = open_image(&dir, "legacy.png", &src);
    for [b, c, s] in SETTINGS {
        let absent = adjust(
            img,
            &json!({"brightness": b, "contrast": c, "saturation": s}),
        );
        let explicit = adjust(
            img,
            &json!({"brightness": b, "contrast": c, "saturation": s, "legacy": true}),
        );
        assert_eq!(absent, explicit, "{b} {c} {s}");
        within_one(&absent, &src, |v| legacy(b, c, s, v), "legacy oracle");
        let frozen = unsafe { dr_image_adjust(img, b as f32, c as f32, s as f32) };
        assert!(!frozen.is_null());
        let frozen_bytes = img_pixels(frozen);
        unsafe { dr_image_free(frozen) };
        assert!(
            absent
                .iter()
                .zip(&frozen_bytes)
                .all(|(a, f)| a.abs_diff(*f) <= 1),
            "{b} {c} {s}: the frozen export"
        );
    }
    unsafe { dr_image_free(img) };
}

#[test]
fn the_modern_form_matches_its_oracle_and_differs_from_legacy() {
    let dir = TempDir::new().unwrap();
    let src = pattern();
    let img = open_image(&dir, "modern.png", &src);
    for [b, c, s] in SETTINGS {
        let params = json!({"brightness": b, "contrast": c, "saturation": s, "legacy": false});
        let got = adjust(img, &params);
        within_one(&got, &src, |v| modern(b, c, s, v), &params.to_string());
        let old = adjust(
            img,
            &json!({"brightness": b, "contrast": c, "saturation": s}),
        );
        assert_ne!(got, old, "{params}: the modern form is its own curve");
    }
    unsafe { dr_image_free(img) };
}

#[test]
fn modern_endpoints_stay_fixed_and_the_ramp_stays_monotonic() {
    let dir = TempDir::new().unwrap();
    let src = pattern();
    let img = open_image(&dir, "ramp.png", &src);
    for b in [-1.0, -0.4, 0.0, 0.4, 1.0] {
        for c in [-1.0, -0.4, 0.0, 0.4, 1.0] {
            let got = adjust(
                img,
                &json!({"brightness": b, "contrast": c, "legacy": false}),
            );
            let greys: Vec<u8> = got[..256 * 4].chunks_exact(4).map(|p| p[0]).collect();
            assert_eq!(greys[0], 0, "{b} {c}: black stays black");
            assert_eq!(greys[255], 255, "{b} {c}: white stays white");
            assert!(
                greys.windows(2).all(|w| w[0] <= w[1]),
                "{b} {c}: the ramp never reverses"
            );
            // The oracle's slope at 0.5 is k: a small step about mid-grey.
            if b == 0.0 {
                let k = (1.5_f64 * c).exp2();
                let lo = modern(0.0, c, 0.0, [0.49; 3])[0];
                let hi = modern(0.0, c, 0.0, [0.51; 3])[0];
                assert!(((hi - lo) / 0.02 - k).abs() < 0.02 * k, "{c}: slope k");
            }
        }
    }
    unsafe { dr_image_free(img) };
}

#[test]
fn a_zero_modern_edit_is_the_identity() {
    let dir = TempDir::new().unwrap();
    let src = pattern();
    let img = open_image(&dir, "identity.png", &src);
    assert_eq!(adjust(img, &json!({"legacy": false})), *src.as_raw());
    unsafe { dr_image_free(img) };
}

#[test]
fn a_non_boolean_legacy_is_malformed() {
    let dir = TempDir::new().unwrap();
    let img = open_image(&dir, "bad.png", &pattern());
    for bad in [json!("false"), json!(0), json!(null)] {
        let op = CString::new("bcs").unwrap();
        let text = CString::new(json!({"brightness": 0.2, "legacy": bad}).to_string()).unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe { dr_image_adjust_op(img, op.as_ptr(), text.as_ptr(), &mut err) };
        assert!(out.is_null(), "legacy {bad} must refuse");
        assert!(!take_err_string(err).is_empty());
    }
    unsafe { dr_image_free(img) };
}

#[test]
fn the_8_bit_layer_is_byte_identical_to_the_filter_in_both_forms() {
    let dir = TempDir::new().unwrap();
    let src = pattern();
    for (tag, params) in [
        (
            "modern",
            r#"{"brightness":0.25,"contrast":0.4,"saturation":-0.3,"legacy":false}"#,
        ),
        (
            "legacy",
            r#"{"brightness":0.25,"contrast":0.4,"saturation":-0.3,"legacy":true}"#,
        ),
    ] {
        let doc = doc_from(&dir, &format!("{tag}-bg.png"), &src);
        let doc = add_layer(
            &dir,
            &format!("{tag}-top.png"),
            doc,
            0,
            &solid(2, 2, MAGENTA),
            "Adjust",
        );
        let doc = set_meta(doc, 1, &adjust_meta("bcs", params));
        let img = open_image(&dir, &format!("{tag}-ref.png"), &src);
        let filtered = adjust(img, &serde_json::from_str(params).unwrap());
        assert_eq!(flat_pixels(doc), filtered, "{tag}");
        unsafe { dr_image_free(img) };
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn the_16_bit_path_runs_the_same_formula() {
    let srgb = IccProfile::srgb();
    let (b, c, s) = (0.3, -0.45, 0.2);
    let meta = json!({"type": "adjust", "evaluation": "precision_v1", "op": "bcs",
        "params": {"brightness": b, "contrast": c, "saturation": s, "legacy": false}})
    .to_string();
    let op = PrecisionAdjustment::parse(&meta, &srgb).unwrap();
    assert_eq!(op.operation(), "bcs");
    let mut maximum = 0.0_f64;
    for i in 0..513 {
        let v = (i * 127 + 3).min(65535) as f32 / 65535.0;
        let rgb = [v, 1.0 - v, (v * 0.731 + 0.017).fract()];
        let want = modern(b, c, s, rgb.map(f64::from)).map(|v| v.clamp(0.0, 1.0));
        let got = op.apply_at(rgb, 0.0, (i, 0));
        for k in 0..3 {
            maximum = maximum.max((f64::from(got[k]) - want[k]).abs());
        }
    }
    assert!(maximum <= 2e-6, "maximum error {maximum}");
    // And through a 16-bit document render, within one u16 code.
    let samples: Vec<[u16; 4]> = (0..64u16)
        .map(|i| [i * 1040, 65535 - i * 1000, i * 517, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(64, 1, samples.clone(), srgb.clone(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        64,
        1,
        srgb,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
    .add_adjustment("Modern", &meta, 1.0, REFERENCE_WORKING_BYTES, 0, None)
    .unwrap()
    .unwrap();
    let out = doc
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    for (px, src) in out.iter().zip(&samples) {
        let want = modern(
            b,
            c,
            s,
            [src[0], src[1], src[2]].map(|v| f64::from(v) / 65535.0),
        );
        for k in 0..3 {
            let w = want[k].clamp(0.0, 1.0) * 65535.0;
            assert!(
                (f64::from(px[k]) - w).abs() <= 1.5,
                "{src:?} channel {k}: {} vs {w}",
                px[k]
            );
        }
    }
}
