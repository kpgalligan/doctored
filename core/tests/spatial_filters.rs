//! Box Blur, Surface Blur and Add Grain at the C boundary and through the
//! 16-bit selected-edit path. Every expectation is an independent oracle: a
//! direct binary64 2-D premultiplied box mean, a direct 2-D tent sum over the
//! window, and the grain's statistical contract (RMS, determinism, position).
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

use crate::gaussian_reference;

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}
fn samples(image: *const DrImage, count: usize) -> Vec<u8> {
    let p = unsafe { dr_image_pixels_rgba(image) };
    assert!(!p.is_null());
    unsafe { std::slice::from_raw_parts(p, count * 4) }.to_vec()
}
/// `Ok(None)` is a NULL result with no error.
fn raw_filter(img: *const DrImage, op: &str, params: Option<&str>) -> Result<*mut DrImage, String> {
    let op = CString::new(op).unwrap();
    let params = params.map(|p| CString::new(p).unwrap());
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_image_filter_op(
            img,
            op.as_ptr(),
            params.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
            ptr::null(),
            &mut err,
        )
    };
    if err.is_null() {
        Ok(out)
    } else {
        assert!(out.is_null());
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        Err(message)
    }
}
fn filter(w: u32, h: u32, pixels: &[u8], op: &str, params: Value) -> Vec<u8> {
    let img = image(w, h, pixels);
    let out = raw_filter(img, op, Some(&params.to_string())).unwrap();
    assert!(!out.is_null(), "{op} answered the bypass");
    let result = samples(out, (w * h) as usize);
    unsafe {
        dr_image_free(out);
        dr_image_free(img);
    }
    result
}
fn filter16(w: u32, h: u32, pixels: Vec<[u16; 4]>, op: &str, params: Value) -> Vec<[u16; 4]> {
    let d = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let e = SelectedEdit::parse(&json!({"layer":0,"op":op,"params":params}).to_string()).unwrap();
    let out = d
        .selected_edit(&e, &[], REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    out.as_ref()
        .unwrap_or(&d)
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}
fn widen(pixels: &[u8]) -> Vec<[u16; 4]> {
    pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect()
}
/// Structure, speckle and adversarial alpha (0, 1, 255 and partials).
fn plate(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let base = 40 + (i % 7) * 9 + (i / w) * 5;
            let speck = if i % 11 == 0 { 120 } else { 0 };
            let value = |offset: u32| ((base + speck + offset) % 256) as u8;
            [
                value(0),
                value(37),
                value(91),
                match i % 5 {
                    0 => 0,
                    1 => 1,
                    2 => 255,
                    _ => ((i * 53 + 17) % 256) as u8,
                },
            ]
        })
        .collect()
}
fn at(pixels: &[u8], w: u32, h: u32, x: i64, y: i64) -> [f64; 4] {
    let x = x.clamp(0, i64::from(w) - 1) as usize;
    let y = y.clamp(0, i64::from(h) - 1) as usize;
    let i = (y * w as usize + x) * 4;
    [0, 1, 2, 3].map(|c| f64::from(pixels[i + c]))
}

#[test]
fn box_blur_matches_an_independent_premultiplied_oracle() {
    let (w, h) = (13, 9);
    let input = plate(w, h);
    for radius in [1_i64, 2, 3, 20] {
        let out = filter(w, h, &input, "box_blur", json!({ "radius": radius }));
        for y in 0..i64::from(h) {
            for x in 0..i64::from(w) {
                let mut sum = [0.0_f64; 4];
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let p = at(&input, w, h, x + dx, y + dy);
                        let a = p[3] / 255.0;
                        for c in 0..3 {
                            sum[c] += p[c] / 255.0 * a;
                        }
                        sum[3] += a;
                    }
                }
                let alpha = sum[3] / ((2 * radius + 1) * (2 * radius + 1)) as f64;
                let i = (y * i64::from(w) + x) as usize * 4;
                let expected_alpha = (alpha * 255.0).round();
                assert!(
                    (f64::from(out[i + 3]) - expected_alpha).abs() <= 1.0,
                    "r{radius} ({x},{y}) alpha"
                );
                if expected_alpha == 0.0 {
                    continue;
                }
                for c in 0..3 {
                    let expected = (sum[c] / sum[3] * 255.0).round();
                    assert!(
                        (f64::from(out[i + c]) - expected).abs() <= 1.0,
                        "r{radius} ({x},{y}) c{c}: {} vs {expected}",
                        out[i + c]
                    );
                }
            }
        }
    }
}

/// The direct tent filter at one 8-bit pixel and channel, in levels.
#[allow(clippy::too_many_arguments)]
fn tent(input: &[u8], w: u32, h: u32, x: i64, y: i64, c: usize, r: i64, t: f64) -> Option<f64> {
    let centre = at(input, w, h, x, y)[c];
    let (mut num, mut den) = (0.0, 0.0);
    for dy in -r..=r {
        for dx in -r..=r {
            let p = at(input, w, h, x + dx, y + dy);
            let weight = (1.0 - (p[c] - centre).abs() / (2.5 * t)).max(0.0);
            num += p[3] * weight * p[c];
            den += p[3] * weight;
        }
    }
    (den > 0.0).then(|| num / den)
}

#[test]
fn surface_blur_is_the_direct_tent_filter_at_8_bits() {
    let (w, h) = (11, 9);
    let input = plate(w, h);
    for (radius, threshold) in [(1, 2.0), (2, 15.0), (3, 60.0), (4, 255.0)] {
        let out = filter(
            w,
            h,
            &input,
            "surface_blur",
            json!({"radius": radius, "threshold": threshold}),
        );
        for y in 0..i64::from(h) {
            for x in 0..i64::from(w) {
                let i = (y * i64::from(w) + x) as usize * 4;
                assert_eq!(out[i + 3], input[i + 3], "alpha is exact");
                if input[i + 3] == 0 {
                    assert_eq!(out[i..i + 4], input[i..i + 4], "alpha-0 untouched");
                    continue;
                }
                for c in 0..3 {
                    let expected = tent(&input, w, h, x, y, c, radius, threshold).unwrap();
                    // Exact, except that a value sitting on a half level is
                    // the one place two summation orders may round apart.
                    let tie = ((expected - expected.floor()) - 0.5).abs() < 1e-3;
                    let got = f64::from(out[i + c]);
                    if tie {
                        assert!((got - expected.round()).abs() <= 1.0);
                    } else {
                        assert_eq!(
                            got,
                            expected.round(),
                            "r{radius} T{threshold} ({x},{y}) c{c}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn surface_blur_keeps_horizontal_and_diagonal_steps_below_the_threshold() {
    let (w, h) = (16, 12);
    for diagonal in [false, true] {
        let input: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                let high = if diagonal { x > y } else { x >= w / 2 };
                let v = if high { 170 } else { 60 };
                [v, 255 - v, v, 255]
            })
            .collect();
        // The step is 110 levels; 2.5 × 40 = 100 leaves it alone exactly.
        let out = filter(
            w,
            h,
            &input,
            "surface_blur",
            json!({"radius": 4, "threshold": 40}),
        );
        assert_eq!(out, input, "diagonal {diagonal}");
    }
}

#[test]
fn surface_blur_byte_origin_16_bit_matches_8_bit() {
    let (w, h) = (12, 10);
    let input = plate(w, h);
    for (radius, threshold) in [(2, 15.0), (5, 120.0)] {
        let params = json!({"radius": radius, "threshold": threshold});
        let byte = filter(w, h, &input, "surface_blur", params.clone());
        let wide = filter16(w, h, widen(&input), "surface_blur", params);
        for (i, p) in wide.iter().enumerate() {
            for c in 0..4 {
                let narrowed = (f64::from(p[c]) / 257.0).round();
                assert!((narrowed - f64::from(byte[i * 4 + c])).abs() <= 1.0);
            }
            if input[i * 4 + 3] == 0 {
                assert_eq!(*p, widen(&input)[i], "alpha-0 untouched at 16 bits");
            }
            assert_eq!(p[3], u16::from(input[i * 4 + 3]) * 257);
        }
    }
}

#[test]
fn grain_is_deterministic_positional_and_agrees_across_depths() {
    let (w, h) = (24, 16);
    let input = plate(w, h);
    let params = |seed: i64| json!({"amount": 60, "size": 1.5, "roughness": 30, "seed": seed});
    let a = filter(w, h, &input, "add_grain", params(7));
    assert_eq!(a, filter(w, h, &input, "add_grain", params(7)));
    assert_ne!(a, filter(w, h, &input, "add_grain", params(8)));
    assert_ne!(a, filter(w, h, &input, "add_grain", params(-7)));
    for (i, chunk) in input.chunks_exact(4).enumerate() {
        assert_eq!(a[i * 4 + 3], chunk[3], "alpha exact");
        if chunk[3] == 0 {
            assert_eq!(a[i * 4..i * 4 + 4], *chunk, "alpha-0 untouched");
        }
    }
    let wide = filter16(w, h, widen(&input), "add_grain", params(7));
    for (i, p) in wide.iter().enumerate() {
        for c in 0..4 {
            let narrowed = (f64::from(p[c]) / 257.0).round();
            assert!((narrowed - f64::from(a[i * 4 + c])).abs() <= 1.0);
        }
    }
    // At roughness 100 the grain is the positional white field itself, so a
    // larger canvas holds the smaller one's grain on the overlap.
    let rough = json!({"amount": 80, "roughness": 100, "seed": 11});
    let grey = |w: u32, h: u32| vec![128_u8; (w * h * 4) as usize];
    let small = filter(16, 16, &grey(16, 16), "add_grain", rough.clone());
    let large = filter(32, 32, &grey(32, 32), "add_grain", rough);
    for y in 0..16 {
        for x in 0..16 {
            let s = (y * 16 + x) * 4;
            let l = (y * 32 + x) * 4;
            assert_eq!(small[s..s + 4], large[l..l + 4]);
        }
    }
}

fn rms_levels(out: &[u8], value: u8) -> f64 {
    let n = out.len() / 4;
    let sum: f64 = out
        .chunks_exact(4)
        .map(|p| (f64::from(p[0]) - f64::from(value)).powi(2))
        .sum();
    (sum / n as f64).sqrt()
}

#[test]
fn grain_rms_at_mid_grey_is_the_frozen_constant() {
    let (w, h) = (256, 256);
    let grey = vec![128_u8; (w * h * 4) as usize];
    let y = 128.0 / 255.0;
    let target = |amount: f64| amount * 0.001 * 4.0 * y * (1.0 - y) * 255.0;
    let white = filter(
        w,
        h,
        &grey,
        "add_grain",
        json!({"amount": 50, "roughness": 100}),
    );
    let rms = rms_levels(&white, 128);
    assert!((rms / target(50.0) - 1.0).abs() < 0.05, "white rms {rms}");
    let smooth = filter(
        w,
        h,
        &grey,
        "add_grain",
        json!({"amount": 100, "size": 2, "roughness": 0, "seed": 3}),
    );
    let rms = rms_levels(&smooth, 128);
    assert!((rms / target(100.0) - 1.0).abs() < 0.10, "smooth rms {rms}");
    for value in [0_u8, 255] {
        let flat: Vec<u8> = (0..w * h)
            .flat_map(|_| [value, value, value, 255])
            .collect();
        let out = filter(
            w,
            h,
            &flat,
            "add_grain",
            json!({"amount": 100, "roughness": 100}),
        );
        assert_eq!(out, flat, "black and white are unchanged");
    }
}

#[test]
fn filter_op_refuses_by_name_and_bypasses_exactly() {
    let input = plate(6, 5);
    let img = image(6, 5, &input);
    for (op, params, expected) in [
        (
            "sharpen_more",
            "{}",
            "supports blur, box_blur, surface_blur, add_grain, radial_blur, clouds, \
             difference_clouds, offset, motion_blur, lens_blur, tilt_shift, lens_correction, \
             presence, develop_filter, smart_sharpen, liquify, depth_blur, unsharp_mask, high_pass, \
             dust_and_scratches and reduce_noise only",
        ),
        (
            "gaussian_blur",
            "{}",
            "supports blur, box_blur, surface_blur, add_grain, radial_blur, clouds, \
             difference_clouds, offset, motion_blur, lens_blur, tilt_shift, lens_correction, \
             presence, develop_filter, smart_sharpen, liquify, depth_blur, unsharp_mask, high_pass, \
             dust_and_scratches and reduce_noise only",
        ),
        ("box_blur", "{", "invalid filter params"),
        ("box_blur", "[1]", "must be a JSON object"),
        (
            "box_blur",
            r#"{"radius":0}"#,
            "radius must be finite in [1,1000]",
        ),
        (
            "box_blur",
            r#"{"radius":1001}"#,
            "radius must be finite in [1,1000]",
        ),
        ("box_blur", r#"{"radius":2.5}"#, "radius must be an integer"),
        (
            "box_blur",
            r#"{"sigma":2}"#,
            "unexpected box_blur parameter sigma",
        ),
        (
            "surface_blur",
            r#"{"threshold":1}"#,
            "threshold must be finite in [2,255]",
        ),
        (
            "surface_blur",
            r#"{"radius":101}"#,
            "radius must be finite in [1,100]",
        ),
        (
            "surface_blur",
            r#"{"radius":"3"}"#,
            "radius must be numeric",
        ),
        (
            "add_grain",
            r#"{"amount":101}"#,
            "amount must be finite in [0,100]",
        ),
        (
            "add_grain",
            r#"{"size":5}"#,
            "size must be finite in [0.5,4]",
        ),
        (
            "add_grain",
            r#"{"roughness":-1}"#,
            "roughness must be finite in [0,100]",
        ),
        (
            "add_grain",
            r#"{"seed":1.5}"#,
            "seed must be a 64-bit integer",
        ),
    ] {
        let err = raw_filter(img, op, Some(params)).unwrap_err();
        assert!(err.contains(expected), "{op} {params}: {err}");
    }
    // Amount 0 is the exact bypass: NULL with no error.
    assert!(raw_filter(img, "add_grain", Some(r#"{"amount":0}"#))
        .unwrap()
        .is_null());
    // NULL params means every default.
    for op in ["box_blur", "surface_blur", "add_grain"] {
        let out = raw_filter(img, op, None).unwrap();
        assert!(!out.is_null());
        unsafe { dr_image_free(out) };
    }
    // A NULL image is a refusal, not an error.
    assert!(raw_filter(ptr::null(), "box_blur", None).unwrap().is_null());
    let op = CString::new("box_blur").unwrap();
    assert!(unsafe {
        dr_image_filter_op(
            ptr::null(),
            op.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    unsafe { dr_image_free(img) };
    // A plan over the work limit refuses before any plane is allocated: one
    // column a million rows tall re-seeds a 201-column window every row.
    let tall = vec![128_u8; 4_000_000];
    let column = image(1, 1_000_000, &tall);
    let err = raw_filter(
        column,
        "surface_blur",
        Some(r#"{"radius":100,"threshold":255}"#),
    )
    .unwrap_err();
    assert!(
        err.contains("surface blur exceeds the bounded pixel-work limit"),
        "{err}"
    );
    unsafe { dr_image_free(column) };
}

/// SplitMix64, written independently of the core's for the probe's inputs.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The explicit margin the shipped `SURFACE_*_WORK_PER_1024` carry over the
/// measured pair: the measured pair is only 2–6% above 24 MP wall-clock times,
/// the reference varies about 2% between runs, and the 16-bit read path is not
/// calibrated.
const SURFACE_SAFETY_FACTOR: f64 = 1.15;

/// The fastest of three timings of `run`, in seconds, so an interference spike
/// on one of them is rejected by the probe rather than by hand.
fn min_of_3<T>(mut run: impl FnMut() -> T, mut done: impl FnMut(T)) -> f64 {
    (0..3)
        .map(|_| {
            let start = std::time::Instant::now();
            let out = run();
            let t = start.elapsed().as_secs_f64();
            done(out);
            t
        })
        .fold(f64::INFINITY, f64::min)
}

/// The four-corner Surface Blur calibration record. Times the Gaussian
/// reference (byte High Pass at σ3.9, converted to the radius-16 unit by
/// `R39_PER_R16`) and Surface Blur at
/// r ∈ {1,100} × T ∈ {2,255}, all on 3000×2000 opaque images. Fits
/// time = a·slide + b·evaluate by least squares over the four corners (the
/// counts are `sampling_surface::surface_plan`'s), scales both coefficients by
/// the one factor that makes the charge at least the time at every corner and
/// at four interior checks (r5 T15, r5 T255, r10 T60, r50 T255), and prints the
/// implied `SURFACE_SLIDE_WORK_PER_1024` / `SURFACE_EVALUATE_WORK_PER_1024`,
/// each point's charge against its time, that pair times
/// `SURFACE_SAFETY_FACTOR` and what the scaled pair admits at 24 MP. Every
/// timing, the reference included, is the minimum of three. Inputs above
/// T=2 and the reference use full-range noise; the T=2 corners mid-grey ±5
/// levels. The kernel evaluates every bin in reach, empty
/// or not, so the content does not change the cost.
#[test]
#[ignore = "calibration probe: make t F=spatial_filters A='--ignored --nocapture --test-threads=1'"]
fn surface_blur_calibration_probe() {
    let (w, h) = (3000_u32, 2000_u32);
    let n = u64::from(w) * u64::from(h);
    let mut state = 0x5EED_u64;
    let full: Vec<u8> = (0..n)
        .flat_map(|_| {
            let v = splitmix(&mut state);
            [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]
        })
        .collect();
    let narrow: Vec<u8> = (0..n)
        .flat_map(|_| {
            let v = splitmix(&mut state);
            let level = |shift: u32| (123 + ((v >> shift) % 11)) as u8;
            [level(0), level(8), level(16), 255]
        })
        .collect();
    let full_img = image(w, h, &full);
    let narrow_img = image(w, h, &narrow);
    let t_gauss = min_of_3(
        || {
            let mut err = ptr::null_mut();
            let op = CString::new("high_pass").unwrap();
            let params = CString::new(r#"{"radius":3.9}"#).unwrap();
            unsafe {
                dr_image_filter_op(
                    full_img,
                    op.as_ptr(),
                    params.as_ptr(),
                    ptr::null(),
                    &mut err,
                )
            }
        },
        |reference| {
            assert!(!reference.is_null());
            unsafe { dr_image_free(reference) };
        },
    );
    let work_gauss = (n * (8 * 25 + 12)) as f64;
    // Work units per second, so a time converts to a charge: the σ3.9 FIR's
    // rate converted to the r16 unit every constant here was derived in
    // (`gaussian_reference::R39_PER_R16`).
    let rate = work_gauss / t_gauss / gaussian_reference::R39_PER_R16;
    println!(
        "reference: high_pass σ3.9 on {w}x{h}: {t_gauss:.3} s, work {work_gauss:.3e}; r16-unit rate {rate:.4e} units/s"
    );
    // (slide, evaluate) bin operations: the plan's counts.
    let counts = |w: u64, h: u64, radius: u64, threshold: f64| {
        let e = (2 * (2.5 * threshold).ceil() as u64 + 2).min(256);
        let span = 2 * radius + 1;
        let n = w * h;
        (
            (3 * (n * 258 + span * (256 * h + w))) as f64,
            (3 * n * e) as f64,
        )
    };
    let mut corners = Vec::new();
    // The four corners, then interior checks that only feed the factor: they
    // hold the charge to the time where linearity is the claim, not a fit.
    let points = [
        (1_u64, 2.0_f64),
        (1, 255.0),
        (100, 2.0),
        (100, 255.0),
        (5, 15.0),
        (5, 255.0),
        (10, 60.0),
        (50, 255.0),
    ];
    for (radius, threshold) in points {
        let input = if threshold > 2.0 {
            full_img
        } else {
            narrow_img
        };
        let params = json!({"radius": radius, "threshold": threshold}).to_string();
        let mut refusal = None;
        let t = min_of_3(
            || raw_filter(input, "surface_blur", Some(&params)),
            |out| match out {
                Ok(out) => unsafe { dr_image_free(out) },
                Err(e) => refusal = Some(e),
            },
        );
        let (slide, evaluate) = counts(u64::from(w), u64::from(h), radius, threshold);
        match refusal {
            None => {
                println!(
                    "corner r{radius} T{threshold}: {t:.3} s, slide {slide:.3e}, evaluate {evaluate:.3e}"
                );
                corners.push((radius, threshold, t, slide, evaluate));
            }
            Some(e) => println!("corner r{radius} T{threshold}: refused ({e})"),
        }
    }
    unsafe {
        dr_image_free(full_img);
        dr_image_free(narrow_img);
    }
    assert_eq!(
        corners.len(),
        points.len(),
        "every point must run to calibrate"
    );
    // Least squares over the four corners, no intercept: the 2×2 normal
    // equations.
    let (mut ss, mut se, mut ee, mut ts, mut te) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(_, _, t, s, e) in &corners[..4] {
        ss += s * s;
        se += s * e;
        ee += e * e;
        ts += t * s;
        te += t * e;
    }
    let det = ss * ee - se * se;
    let a = ((ts * ee - te * se) / det).max(0.0);
    let b = ((te * ss - ts * se) / det).max(0.0);
    println!(
        "fit: {:.4} ns per slide bin, {:.4} ns per evaluated bin",
        a * 1e9,
        b * 1e9
    );
    let factor = corners
        .iter()
        .map(|&(_, _, t, s, e)| t / (a * s + b * e))
        .fold(1.0_f64, f64::max);
    let slide_k = (1024.0 * a * rate * factor).ceil();
    let evaluate_k = (1024.0 * b * rate * factor).ceil();
    println!(
        "factor {factor:.4}: implied SURFACE_SLIDE_WORK_PER_1024 {slide_k}, SURFACE_EVALUATE_WORK_PER_1024 {evaluate_k}"
    );
    for &(radius, threshold, t, s, e) in &corners {
        let charge = (slide_k * s + evaluate_k * e) / 1024.0;
        println!(
            "corner r{radius} T{threshold}: charge {charge:.3e} ({:.3} s) vs measured {:.3e} ({t:.3} s)",
            charge / rate,
            t * rate
        );
    }
    let shipped_slide = (slide_k * SURFACE_SAFETY_FACTOR).ceil();
    let shipped_evaluate = (evaluate_k * SURFACE_SAFETY_FACTOR).ceil();
    println!(
        "x{SURFACE_SAFETY_FACTOR}: SURFACE_SLIDE_WORK_PER_1024 {shipped_slide}, SURFACE_EVALUATE_WORK_PER_1024 {shipped_evaluate}"
    );
    // `sampling_gaussian::MAX_DESTRUCTIVE_WORK`, which the plan is checked against.
    let cap = 24_000_000_000_f64;
    for (radius, threshold) in [(5_u64, 15.0_f64), (5, 255.0), (100, 15.0), (100, 255.0)] {
        let (s, e) = counts(6000, 4000, radius, threshold);
        let charge = (shipped_slide * s + shipped_evaluate * e) / 1024.0 + 4.0 * 24e6;
        let verdict = if charge <= cap { "admitted" } else { "refused" };
        println!("24 MP r{radius} T{threshold}: charge {charge:.3e} of {cap:.3e}: {verdict}");
    }
}
