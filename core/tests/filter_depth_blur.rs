//! Depth Blur: the `depth_blur` op of `dr_image_filter_op` at 8 bits
//! and the selected edit at 16 and 32 (`dr_precision_doc_selected_edit`),
//! with SYNTHETIC depth maps; the model never runs here. Every expectation is
//! stated from `depth_blur`'s module doc, not read back from the kernel: a
//! request that changes nothing is NULL with no error, a pixel at the focal
//! depth keeps its sample exactly, the blur grows with the distance from the
//! focus, and one pixel at a known radius is the disc mean written below in
//! binary64 (the disc `dx² + dy² ≤ (r + ½)²`, clamped edges).
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::float_fixture::float_doc;

/// 200 × 40: `max_radius` is round(200 / 40) = 5.
const W: u32 = 200;
const H: u32 = 40;

fn message(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let m = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    m
}

/// Opaque splitmix noise, so every disc mean differs from its centre.
fn noise() -> Vec<u8> {
    let mut state = 0x1234_5678_9ABC_DEF0_u64;
    let mut out = Vec::with_capacity((W * H * 4) as usize);
    for _ in 0..W * H {
        for c in 0..4 {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            out.push(if c == 3 { 255 } else { (z >> 56) as u8 });
        }
    }
    out
}

/// A depth map the plane's size over the whole plane, sample `f(x, y)`.
fn depth(f: impl Fn(u32, u32) -> f64) -> Value {
    let samples: Vec<f64> = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| f(x, y))
        .collect();
    json!({"width": W, "height": H, "samples": samples, "frame": [0, 0, W, H]})
}

/// Near (1) left of column 60, far (0) from it on.
fn two_planes() -> Value {
    depth(|x, _| if x < 60 { 1.0 } else { 0.0 })
}

fn with(mut params: Value, depth: Value) -> Value {
    params["depth"] = depth;
    params
}

/// `dr_image_filter_op` `depth_blur`: `(pixels, message)`.
fn byte_run(pixels: &[u8], params: &str) -> (Option<Vec<u8>>, String) {
    let img: *mut DrImage = unsafe { dr_image_from_rgba(pixels.as_ptr(), W, H) };
    assert!(!img.is_null());
    let op = CString::new("depth_blur").unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), ptr::null(), &mut err) };
    let result = (!out.is_null()).then(|| {
        let v =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), pixels.len()) }.to_vec();
        unsafe { dr_image_free(out) };
        v
    });
    unsafe { dr_image_free(img) };
    (result, message(err))
}

/// The selected edit on layer 0: `Ok(None)` is NULL with no error.
fn selected(doc: &PrecisionDocument, params: &Value) -> Result<Option<PrecisionDocument>, String> {
    let request =
        CString::new(json!({"layer": 0, "op": "depth_blur", "params": params}).to_string())
            .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let error = message(err);
    if out.is_null() {
        return if error.is_empty() {
            Ok(None)
        } else {
            Err(error)
        };
    }
    assert!(error.is_empty(), "{error}");
    Ok(Some(*unsafe { Box::from_raw(out) }))
}

fn doc16(pixels: &[u8]) -> PrecisionDocument {
    let wide: Vec<[u16; 4]> = pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(W, H, wide, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// The float twin: each byte as `v / 255` in linear light.
fn float_pixels(pixels: &[u8]) -> Vec<[f32; 4]> {
    pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| f32::from(p[c]) / 255.0))
        .collect()
}

fn doc32(pixels: &[u8]) -> PrecisionDocument {
    float_doc(W, H, &IccProfile::srgb(), &[float_pixels(pixels)])
}

fn codes16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.layer_source(0).unwrap().to_rgba16().unwrap()
}

fn samples32(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.layer_source(0).unwrap().to_rgba32().unwrap()
}

/// The disc mean of radius `r` at `(x, y)` over `value(x, y, c)`, opaque.
fn disc_mean(value: impl Fn(u32, u32, usize) -> f64, r: i64, x: i64, y: i64) -> [f64; 3] {
    let limit = (r as f64 + 0.5) * (r as f64 + 0.5);
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for dy in -r..=r {
        for dx in -r..=r {
            if (dx * dx + dy * dy) as f64 > limit {
                continue;
            }
            let sx = (x + dx).clamp(0, i64::from(W) - 1) as u32;
            let sy = (y + dy).clamp(0, i64::from(H) - 1) as u32;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += value(sx, sy, c);
            }
            count += 1.0;
        }
    }
    sum.map(|s| s / count)
}

#[test]
fn zero_strength_and_an_infinite_focal_range_change_nothing_at_every_depth() {
    let pixels = noise();
    for params in [
        json!({"blur_strength": 0, "focal_range": 10}),
        json!({"blur_strength": 80, "focal_range": 100}),
    ] {
        let params = with(params, two_planes());
        let (out, error) = byte_run(&pixels, &params.to_string());
        assert!(out.is_none() && error.is_empty(), "8-bit {params}: {error}");
        assert!(matches!(selected(&doc16(&pixels), &params), Ok(None)));
        assert!(matches!(selected(&doc32(&pixels), &params), Ok(None)));
    }
}

#[test]
fn a_pixel_at_the_focal_depth_keeps_its_sample_at_8_16_and_32_bits() {
    let pixels = noise();
    // Focus at the near plane (D = 0); (20, 20) is 40 px inside it.
    let params = with(
        json!({"focal_distance": 0, "focal_range": 10, "blur_strength": 100}),
        two_planes(),
    );
    let i = (20 * W + 20) as usize;
    let far = (20 * W + 150) as usize;
    let (out, error) = byte_run(&pixels, &params.to_string());
    let out = out.unwrap_or_else(|| panic!("8-bit: {error}"));
    assert_eq!(out[i * 4..i * 4 + 4], pixels[i * 4..i * 4 + 4]);
    assert_ne!(out[far * 4..far * 4 + 3], pixels[far * 4..far * 4 + 3]);

    let source = doc16(&pixels);
    let out = codes16(&selected(&source, &params).unwrap().unwrap());
    assert_eq!(out[i], codes16(&source)[i]);
    assert_ne!(out[far], codes16(&source)[far]);

    let source = doc32(&pixels);
    let out = samples32(&selected(&source, &params).unwrap().unwrap());
    let before = samples32(&source);
    assert_eq!(out[i].map(f32::to_bits), before[i].map(f32::to_bits));
    assert_ne!(out[far], before[far]);
}

#[test]
fn the_blur_grows_as_the_depth_leaves_the_focus() {
    let pixels = noise();
    // Nearness rises left to right, so D = 1 − x / 199; the focus is D = 1
    // (the left edge) with no range, so ρ = 5 · x / 199 grows with x.
    let params = with(
        json!({"focal_distance": 100, "focal_range": 0, "blur_strength": 100}),
        depth(|x, _| f64::from(x)),
    );
    let (out, error) = byte_run(&pixels, &params.to_string());
    let out = out.unwrap_or_else(|| panic!("8-bit: {error}"));
    // Mean absolute change per band of 25 columns.
    let band = |b: u32| -> f64 {
        let mut total = 0.0;
        for y in 0..H {
            for x in b * 25..(b + 1) * 25 {
                let i = ((y * W + x) * 4) as usize;
                for c in 0..3 {
                    total += (f64::from(out[i + c]) - f64::from(pixels[i + c])).abs();
                }
            }
        }
        total / f64::from(25 * H * 3)
    };
    // The last band (x 175…199) is left out: its discs reach past the right
    // edge, whose clamped pixel repeats and pulls the mean toward the edge
    // column, so its change says nothing about the radius.
    let bands: Vec<f64> = (0..7).map(band).collect();
    for pair in bands.windows(2) {
        assert!(pair[0] < pair[1], "not monotone: {bands:?}");
    }
}

#[test]
fn a_pixel_at_a_known_radius_is_the_disc_mean_at_every_depth() {
    let pixels = noise();
    // (150, 20) lies far inside the far plane (n = 0, D = 1, unguided by any
    // edge); the focus is D = 0 with no range and strength 60, so
    // ρ = 5 · 0.6 · 1 = 3.
    let params = with(
        json!({"focal_distance": 0, "focal_range": 0, "blur_strength": 60}),
        two_planes(),
    );
    let (x, y) = (150_u32, 20_u32);
    let i = (y * W + x) as usize;
    let byte = |sx: u32, sy: u32, c: usize| f64::from(pixels[((sy * W + sx) * 4) as usize + c]);

    let (out, error) = byte_run(&pixels, &params.to_string());
    let out = out.unwrap_or_else(|| panic!("8-bit: {error}"));
    let expected = disc_mean(|sx, sy, c| byte(sx, sy, c) / 255.0, 3, x.into(), y.into());
    for c in 0..3 {
        let got = f64::from(out[i * 4 + c]);
        assert!(
            (got - expected[c] * 255.0).abs() <= 1.0,
            "8-bit c{c}: {got} vs {}",
            expected[c] * 255.0
        );
    }

    let out = codes16(&selected(&doc16(&pixels), &params).unwrap().unwrap());
    let expected = disc_mean(
        |sx, sy, c| byte(sx, sy, c) * 257.0 / 65535.0,
        3,
        x.into(),
        y.into(),
    );
    for c in 0..3 {
        let got = f64::from(out[i][c]);
        assert!(
            (got - expected[c] * 65535.0).abs() <= 1.0,
            "16-bit c{c}: {got} vs {}",
            expected[c] * 65535.0
        );
    }

    let out = samples32(&selected(&doc32(&pixels), &params).unwrap().unwrap());
    let expected = disc_mean(
        |sx, sy, c| f64::from((byte(sx, sy, c) as f32) / 255.0),
        3,
        x.into(),
        y.into(),
    );
    for c in 0..3 {
        let got = f64::from(out[i][c]);
        assert!(
            (got - expected[c]).abs() <= 1e-5,
            "32-bit c{c}: {got} vs {}",
            expected[c]
        );
    }
}

fn refusal(params: &str) -> String {
    let (out, error) = byte_run(&noise(), params);
    assert!(out.is_none(), "{params} was accepted");
    error
}

#[test]
fn refusals_name_what_they_refuse() {
    let base = json!({"blur_strength": 50});
    assert_eq!(refusal(&base.to_string()), "depth blur needs a depth map");
    assert_eq!(
        refusal(&with(base.clone(), depth(|_, _| 0.5)).to_string()),
        "depth map has no range"
    );
    assert_eq!(
        refusal(&with(base.clone(), depth(|x, _| if x == 7 { 1e39 } else { 0.0 })).to_string()),
        "depth map samples must be finite"
    );
    let mut no_subject = two_planes();
    no_subject["subject"] = json!(vec![0; (W * H) as usize]);
    assert_eq!(
        refusal(&with(json!({"focus_subject": true}), no_subject).to_string()),
        "focus subject found no subject in the depth input"
    );
    let mut outside = two_planes();
    outside["frame"] = json!([10, 0, W, H]);
    assert_eq!(
        refusal(&with(base.clone(), outside).to_string()),
        "depth blur frame lies outside the image"
    );
    let mut small = two_planes();
    small["frame"] = json!([0, 0, 100, H]);
    assert_eq!(
        refusal(&with(json!({"focal_point": [150, 10]}), small).to_string()),
        "depth blur focal point lies outside the depth frame"
    );
    assert_eq!(
        refusal(&with(json!({"haze": 101}), two_planes()).to_string()),
        "haze must be finite in [0,100]"
    );
    assert_eq!(
        refusal(&with(json!({"sharpness": 1}), two_planes()).to_string()),
        "unexpected depth_blur parameter sharpness"
    );
    let oversize = format!(
        "{{\"depth\":{},\"pad\":\"{}\"}}",
        two_planes(),
        "x".repeat(9 << 20)
    );
    assert_eq!(
        refusal(&oversize),
        "filter params exceeds the supported byte limit"
    );
}
