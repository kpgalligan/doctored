//! Motion Blur, Lens Blur and Tilt-Shift: three `SpatialKernel` arms
//! behind `dr_image_filter_op` at 8 bits and `selected_edit` at 16. The
//! oracles are naive per-pixel loops over f64 written here from the documented
//! formulas: every sample of the line, every lattice point of the disc. The
//! cancel case parks the worker at a checkpoint taken from a fresh twin's own
//! report log, so nothing depends on timing.
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::{dr_image_filter_op, dr_image_filter_op_scratch_bytes};
use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common;
use common::probe::{cancelled_parked_at, reports};

const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}

/// `(pixels, message)`: `pixels` is None for a NULL result, `message` empty
/// when no error was written.
fn run(
    img: *const DrImage,
    op: &str,
    params: &str,
    control: *const DrJobControl,
    count: usize,
) -> (Option<Vec<u8>>, String) {
    let op = CString::new(op).unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), control, &mut err) };
    let pixels = (!out.is_null()).then(|| {
        let v =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), count * 4) }.to_vec();
        unsafe { dr_image_free(out) };
        v
    });
    let message = if err.is_null() {
        String::new()
    } else {
        let m = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        m
    };
    (pixels, message)
}

fn filter(w: u32, h: u32, pixels: &[u8], op: &str, params: Value) -> Vec<u8> {
    let img = image(w, h, pixels);
    let (out, message) = run(img, op, &params.to_string(), ptr::null(), (w * h) as usize);
    unsafe { dr_image_free(img) };
    assert!(message.is_empty(), "{op}: {message}");
    out.unwrap_or_else(|| panic!("{op} answered the bypass"))
}

fn refusal(op: &str, params: Value) -> String {
    let img = image(4, 3, &[128; 48]);
    let (out, message) = run(img, op, &params.to_string(), ptr::null(), 12);
    unsafe { dr_image_free(img) };
    assert!(out.is_none(), "{op} {params} was accepted");
    message
}

/// Structure and mixed alpha (opaque, partial, a transparent pixel with RGB 0).
fn plate(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let value = |offset: u32| ((x * 23 + y * 41 + offset + (i % 3) * 17) % 256) as u8;
            if i % 13 == 5 {
                return [0, 0, 0, 0];
            }
            [
                value(0),
                value(80),
                value(160),
                if i % 4 == 1 {
                    128 + (i % 64) as u8
                } else {
                    255
                },
            ]
        })
        .collect()
}

/// The premultiplied unit sample of pixel `(x, y)`, clamped to the image.
fn premultiplied(src: &[u8], w: u32, h: u32, x: i64, y: i64) -> [f64; 4] {
    let cx = x.clamp(0, i64::from(w) - 1) as usize;
    let cy = y.clamp(0, i64::from(h) - 1) as usize;
    let i = (cy * w as usize + cx) * 4;
    let a = f64::from(src[i + 3]) / 255.0;
    [
        f64::from(src[i]) / 255.0 * a,
        f64::from(src[i + 1]) / 255.0 * a,
        f64::from(src[i + 2]) / 255.0 * a,
        a,
    ]
}

/// The premultiplied sample at continuous `(qx, qy)` (pixel centres at
/// half-integers), 2×2 tent weights, clamped edges.
fn bilinear(src: &[u8], w: u32, h: u32, qx: f64, qy: f64) -> [f64; 4] {
    let (sx, sy) = (qx - 0.5, qy - 0.5);
    let (x0, y0) = (sx.floor() as i64, sy.floor() as i64);
    let mut out = [0.0; 4];
    for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let (px, py) = (x0 + dx, y0 + dy);
        let weight = (1.0 - (sx - px as f64).abs()) * (1.0 - (sy - py as f64).abs());
        let p = premultiplied(src, w, h, px, py);
        for c in 0..4 {
            out[c] += weight * p[c];
        }
    }
    out
}

/// A premultiplied mean as straight 8-bit levels: Box Blur's combine, with
/// its transparent floor.
fn levels(mean: [f64; 4]) -> [f64; 4] {
    if mean[3] < 0.5 / 65535.0 {
        return [0.0; 4];
    }
    [
        (mean[0] / mean[3]).clamp(0.0, 1.0) * 255.0,
        (mean[1] / mean[3]).clamp(0.0, 1.0) * 255.0,
        (mean[2] / mean[3]).clamp(0.0, 1.0) * 255.0,
        mean[3] * 255.0,
    ]
}

fn close(out: &[u8], expected: [f64; 4], i: usize, what: &str) {
    for c in 0..4 {
        let got = f64::from(out[i * 4 + c]);
        if expected[3] < 0.5 && c < 3 {
            continue;
        }
        assert!(
            (got - expected[c]).abs() <= 1.0,
            "{what} pixel{i} component{c}: {got} vs {}",
            expected[c]
        );
    }
}

/// `distance + 1` samples one pixel apart along the line at `angle`, centred
/// on the pixel; a positive angle is counterclockwise on the y-down image.
fn motion_oracle(
    src: &[u8],
    w: u32,
    h: u32,
    angle: f64,
    distance: u32,
    x: u32,
    y: u32,
) -> [f64; 4] {
    let (sin, cos) = angle.to_radians().sin_cos();
    let mut acc = [0.0; 4];
    for k in 0..=distance {
        let t = f64::from(k) - f64::from(distance) / 2.0;
        let p = bilinear(
            src,
            w,
            h,
            f64::from(x) + 0.5 + t * cos,
            f64::from(y) + 0.5 - t * sin,
        );
        for c in 0..4 {
            acc[c] += p[c];
        }
    }
    levels(acc.map(|v| v / f64::from(distance + 1)))
}

/// The weighted premultiplied mean over the lattice disc `dx² + dy² ≤
/// (r + ½)²`, clamped edges. `highlight` is `(brightness, threshold)`.
fn disc_mean(
    src: &[u8],
    w: u32,
    h: u32,
    r: i64,
    highlight: Option<(f64, f64)>,
    x: u32,
    y: u32,
) -> [f64; 4] {
    let mut acc = [0.0; 4];
    let mut total = 0.0;
    for dy in -r..=r {
        for dx in -r..=r {
            if 4 * (dx * dx + dy * dy) > (2 * r + 1) * (2 * r + 1) {
                continue;
            }
            let p = premultiplied(src, w, h, i64::from(x) + dx, i64::from(y) + dy);
            let weight = match highlight {
                None => 1.0,
                Some((brightness, threshold)) => {
                    let luma = LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2];
                    let t = threshold / 255.0;
                    1.0 + brightness / 100.0
                        * 8.0
                        * ((luma - t) / (1.0 - t).max(1.0 / 255.0)).clamp(0.0, 1.0)
                }
            };
            for c in 0..4 {
                acc[c] += weight * p[c];
            }
            total += weight;
        }
    }
    acc.map(|v| v / total)
}

/// Tilt-Shift's radius at a pixel, from the documented formula.
#[allow(clippy::too_many_arguments)]
fn tilt_radius(
    w: u32,
    h: u32,
    radius: f64,
    center: (f64, f64),
    angle: f64,
    focus: f64,
    transition: f64,
    x: u32,
    y: u32,
) -> f64 {
    let (height, width) = (f64::from(h), f64::from(w));
    let (sin, cos) = angle.to_radians().sin_cos();
    // The line's direction is (cos, −sin); the distance is along its normal.
    let d = ((f64::from(x) + 0.5 - center.0 * width) * sin
        + (f64::from(y) + 0.5 - center.1 * height) * cos)
        .abs();
    let beyond = d - focus * height;
    let t = if transition > 0.0 {
        (beyond / (transition * height)).clamp(0.0, 1.0)
    } else if beyond > 0.0 {
        1.0
    } else {
        0.0
    };
    radius * t
}

#[test]
fn motion_blur_is_the_mean_of_the_line_samples() {
    let (w, h) = (23_u32, 17_u32);
    let src = plate(w, h);
    for (angle, distance) in [(0.0, 1_u32), (0.0, 6), (90.0, 5), (30.0, 9), (-135.0, 12)] {
        let out = filter(
            w,
            h,
            &src,
            "motion_blur",
            json!({"angle":angle,"distance":distance}),
        );
        for y in 0..h {
            for x in 0..w {
                close(
                    &out,
                    motion_oracle(&src, w, h, angle, distance, x, y),
                    (y * w + x) as usize,
                    &format!("motion {angle} {distance}"),
                );
            }
        }
    }
}

#[test]
fn motion_blur_smears_along_its_angle_only() {
    // One white column on black: a horizontal blur widens it, a vertical one
    // leaves every pixel as it was.
    let (w, h) = (21_u32, 9_u32);
    let src: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = if i % w == 10 { 255 } else { 0 };
            [v, v, v, 255]
        })
        .collect();
    let across = filter(w, h, &src, "motion_blur", json!({"angle":0,"distance":8}));
    let row = 4 * w as usize * 4;
    assert!(across[row + 7 * 4] > 0 && across[row + 13 * 4] > 0);
    assert_eq!(across[row + 3 * 4], 0);
    let along = filter(w, h, &src, "motion_blur", json!({"angle":90,"distance":8}));
    assert_eq!(along, src);
}

#[test]
fn lens_blur_is_the_weighted_disc_mean() {
    let (w, h) = (19_u32, 15_u32);
    let src = plate(w, h);
    for (radius, brightness, threshold) in [
        (1_i64, 0.0, 255.0),
        (4, 0.0, 255.0),
        (3, 60.0, 128.0),
        (5, 100.0, 200.0),
        (2, 100.0, 255.0),
    ] {
        let out = filter(
            w,
            h,
            &src,
            "lens_blur",
            json!({"radius":radius,"brightness":brightness,"threshold":threshold}),
        );
        let highlight = (brightness > 0.0).then_some((brightness, threshold));
        for y in 0..h {
            for x in 0..w {
                close(
                    &out,
                    levels(disc_mean(&src, w, h, radius, highlight, x, y)),
                    (y * w + x) as usize,
                    &format!("lens {radius} {brightness} {threshold}"),
                );
            }
        }
    }
}

#[test]
fn lens_blur_brightness_spreads_a_highlight_further_than_the_plain_mean() {
    // One white pixel on mid grey: with highlights on, the disc around it is
    // brighter than the unweighted mean makes it.
    let (w, h) = (21_u32, 21_u32);
    let src: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = if i == 10 * w + 10 { 255 } else { 90 };
            [v, v, v, 255]
        })
        .collect();
    let plain = filter(w, h, &src, "lens_blur", json!({"radius":4}));
    let bright = filter(
        w,
        h,
        &src,
        "lens_blur",
        json!({"radius":4,"brightness":100,"threshold":200}),
    );
    let at = ((10 * w + 12) * 4) as usize;
    assert!(
        bright[at] > plain[at] + 5,
        "{} vs {}",
        bright[at],
        plain[at]
    );
    // Far from the highlight both are the flat grey.
    assert_eq!(plain[0], 90);
    assert_eq!(bright[0], 90);
}

#[test]
fn tilt_shift_is_the_disc_mean_at_the_radius_of_each_pixel() {
    let (w, h) = (20_u32, 24_u32);
    let src = plate(w, h);
    for (radius, center, angle, focus, transition) in [
        (4.0, (0.5, 0.5), 0.0, 0.15, 0.25),
        (5.0, (0.3, 0.6), 35.0, 0.1, 0.4),
        (3.0, (0.5, 0.25), -90.0, 0.2, 0.0),
        (6.0, (0.5, 0.5), 0.0, 0.0, 1.0),
    ] {
        let out = filter(
            w,
            h,
            &src,
            "tilt_shift",
            json!({"radius":radius,"center_x":center.0,"center_y":center.1,
                "angle":angle,"focus":focus,"transition":transition}),
        );
        for y in 0..h {
            for x in 0..w {
                let rho = tilt_radius(w, h, radius, center, angle, focus, transition, x, y);
                let lower = rho.floor();
                let mut mean = disc_mean(&src, w, h, lower as i64, None, x, y);
                if rho > lower {
                    let upper = disc_mean(&src, w, h, lower as i64 + 1, None, x, y);
                    for c in 0..4 {
                        mean[c] += (rho - lower) * (upper[c] - mean[c]);
                    }
                }
                close(
                    &out,
                    levels(mean),
                    (y * w + x) as usize,
                    &format!("tilt {radius} {angle} {focus} {transition}"),
                );
            }
        }
    }
}

#[test]
fn tilt_shift_leaves_the_focus_band_exact() {
    let (w, h) = (16_u32, 40_u32);
    let src = plate(w, h);
    let out = filter(
        w,
        h,
        &src,
        "tilt_shift",
        json!({"radius":6,"focus":0.2,"transition":0.2}),
    );
    // Rows within 0.2·40 = 8 px of the centre line y = 20 keep their bytes,
    // but for a transparent pixel, which the combine stores as transparent
    // black.
    for y in 13..27 {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if src[i + 3] == 0 {
                assert_eq!(out[i + 3], 0);
            } else {
                assert_eq!(out[i..i + 4], src[i..i + 4], "{x},{y}");
            }
        }
    }
    assert_ne!(out, src);
}

#[test]
fn the_three_filters_keep_transparent_transparent_and_opaque_opaque() {
    let (w, h) = (14_u32, 11_u32);
    for (op, params) in [
        ("motion_blur", json!({"angle":20,"distance":7})),
        (
            "lens_blur",
            json!({"radius":3,"brightness":50,"threshold":100}),
        ),
        ("tilt_shift", json!({"radius":4,"focus":0.0})),
    ] {
        let clear = vec![0_u8; (w * h * 4) as usize];
        let img = image(w, h, &clear);
        let (out, message) = run(img, op, &params.to_string(), ptr::null(), (w * h) as usize);
        unsafe { dr_image_free(img) };
        assert!(message.is_empty(), "{op}: {message}");
        assert_eq!(out.as_deref(), Some(&clear[..]), "{op}: transparent");
        let opaque: Vec<u8> = plate(w, h)
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        let out = filter(w, h, &opaque, op, params);
        assert!(out.chunks_exact(4).all(|p| p[3] == 255), "{op}: opaque");
    }
}

fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

#[test]
fn byte_and_16_bit_agree_within_a_level_and_coverage_confines_the_result() {
    let (w, h) = (26_u32, 20_u32);
    let src = plate(w, h);
    let wide: Vec<[u16; 4]> = src
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect();
    let d = doc16(w, h, wide.clone());
    let inside = |x: u32, y: u32| (5..19).contains(&x) && (4..15).contains(&y);
    let coverage: Vec<u16> = (0..w * h)
        .map(|i| if inside(i % w, i / w) { 65535 } else { 0 })
        .collect();
    for (op, params) in [
        ("motion_blur", json!({"angle":45,"distance":6})),
        (
            "lens_blur",
            json!({"radius":3,"brightness":40,"threshold":150}),
        ),
        (
            "tilt_shift",
            json!({"radius":4,"focus":0.05,"transition":0.3}),
        ),
    ] {
        let edit =
            SelectedEdit::parse(&json!({"layer":0,"op":op,"params":params.clone()}).to_string())
                .unwrap();
        let out16 = d
            .selected_edit(&edit, &coverage, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .unwrap_or_else(|| panic!("{op}: no change at 16 bits"))
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .unwrap();
        // None of the three takes a frame: the 8-bit filter of the whole image
        // is what the selection's coverage keeps.
        let out8 = filter(w, h, &src, op, params);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if !inside(x, y) {
                    assert_eq!(out16[i], wide[i], "{op} outside {x},{y}");
                    continue;
                }
                for c in 0..4 {
                    if c < 3 && out16[i][3] == 0 {
                        continue;
                    }
                    let got = f64::from(out16[i][c]) / 257.0;
                    assert!(
                        (got - f64::from(out8[i * 4 + c])).abs() <= 1.0,
                        "{op} {x},{y} {c}: {got} vs {}",
                        out8[i * 4 + c]
                    );
                }
            }
        }
    }
}

#[test]
fn parameters_refuse_by_name() {
    for (op, params, wanted) in [
        (
            "motion_blur",
            json!({"distance":0}),
            "distance must be finite in [1,2000]",
        ),
        (
            "motion_blur",
            json!({"distance":2001}),
            "distance must be finite in [1,2000]",
        ),
        (
            "motion_blur",
            json!({"distance":2.5}),
            "distance must be an integer",
        ),
        (
            "motion_blur",
            json!({"angle":361}),
            "angle must be finite in [-360,360]",
        ),
        (
            "motion_blur",
            json!({"radius":3}),
            "unexpected motion_blur parameter radius",
        ),
        (
            "lens_blur",
            json!({"radius":0}),
            "radius must be finite in [1,100]",
        ),
        (
            "lens_blur",
            json!({"radius":101}),
            "radius must be finite in [1,100]",
        ),
        (
            "lens_blur",
            json!({"brightness":101}),
            "brightness must be finite in [0,100]",
        ),
        (
            "lens_blur",
            json!({"threshold":256}),
            "threshold must be finite in [0,255]",
        ),
        (
            "lens_blur",
            json!({"angle":3}),
            "unexpected lens_blur parameter angle",
        ),
        (
            "tilt_shift",
            json!({"radius":1.5}),
            "radius must be an integer",
        ),
        (
            "tilt_shift",
            json!({"angle":91}),
            "angle must be finite in [-90,90]",
        ),
        (
            "tilt_shift",
            json!({"focus":1.5}),
            "focus must be finite in [0,1]",
        ),
        (
            "tilt_shift",
            json!({"transition":-0.1}),
            "transition must be finite in [0,1]",
        ),
        (
            "tilt_shift",
            json!({"center_x":2}),
            "center_x must be finite in [0,1]",
        ),
        (
            "tilt_shift",
            json!({"region_x":0}),
            "unexpected tilt_shift parameter region_x",
        ),
    ] {
        assert_eq!(refusal(op, params.clone()), wanted, "{op} {params}");
    }
}

#[test]
fn the_byte_plan_answers_for_each_and_refuses_in_the_filter_words() {
    let (w, h) = (64_u32, 48_u32);
    let img = image(w, h, &plate(w, h));
    for (op, params) in [
        ("motion_blur", "{}"),
        ("lens_blur", "{}"),
        ("tilt_shift", "{}"),
    ] {
        let (name, text) = (CString::new(op).unwrap(), CString::new(params).unwrap());
        let mut err: *mut c_char = ptr::null_mut();
        let bytes = unsafe {
            dr_image_filter_op_scratch_bytes(img, name.as_ptr(), text.as_ptr(), &mut err)
        };
        assert!(err.is_null(), "{op}");
        // At least the n×16 plane the kernel returns.
        assert!(bytes >= u64::from(w * h) * 16, "{op}: {bytes}");
    }
    let (name, text) = (
        CString::new("lens_blur").unwrap(),
        CString::new(r#"{"radius":0}"#).unwrap(),
    );
    let mut err: *mut c_char = ptr::null_mut();
    let bytes =
        unsafe { dr_image_filter_op_scratch_bytes(img, name.as_ptr(), text.as_ptr(), &mut err) };
    assert_eq!(bytes, 0);
    assert!(!err.is_null());
    let message = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    assert_eq!(message, "radius must be finite in [1,100]");
    unsafe { dr_image_free(img) };
}

#[test]
fn null_arguments_are_tolerated() {
    for op in ["motion_blur", "lens_blur", "tilt_shift"] {
        // A NULL image is NULL with no error; NULL params mean the defaults.
        let (out, message) = run(ptr::null(), op, "{}", ptr::null(), 0);
        assert!(out.is_none() && message.is_empty(), "{op}: {message}");
        let (w, h) = (40_u32, 40_u32);
        let img = image(w, h, &plate(w, h));
        let name = CString::new(op).unwrap();
        let out = unsafe {
            dr_image_filter_op(
                img,
                name.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
            )
        };
        assert!(!out.is_null(), "{op}: defaults refused");
        unsafe {
            dr_image_free(out);
            dr_image_free(img);
        }
    }
}

#[test]
fn each_filter_reports_progress_and_cancels_mid_run() {
    let (w, h) = (120_u32, 80_u32);
    let n = (w * h) as usize;
    let img = image(w, h, &plate(w, h)) as usize;
    for (op, params) in [
        ("motion_blur", r#"{"angle":30,"distance":12}"#),
        (
            "lens_blur",
            r#"{"radius":6,"brightness":40,"threshold":128}"#,
        ),
        ("tilt_shift", r#"{"radius":6,"focus":0.1,"transition":0.3}"#),
    ] {
        let (plain, message) = run(img as *const DrImage, op, params, ptr::null(), n);
        assert!(message.is_empty(), "{op}: {message}");
        assert!(plain.is_some(), "{op}: a bypass");
        let control = unsafe { dr_job_control_create() };
        let (controlled, message) = run(img as *const DrImage, op, params, control, n);
        unsafe { dr_job_control_free(control) };
        assert!(message.is_empty(), "{op}: {message}");
        assert!(plain == controlled, "{op}: controlled bytes differ");

        let render = |token: usize| {
            let (out, message) = run(
                img as *const DrImage,
                op,
                params,
                token as *const DrJobControl,
                n,
            );
            (out.is_some(), message)
        };
        let log = reports(render);
        assert!(
            log.iter().all(|r| (0.0..=1.0).contains(&r.progress)),
            "{op}: a progress outside [0, 1]"
        );
        let last = log.last().expect("a report");
        assert!(
            last.progress == 1.0 && last.stage == 5,
            "{op}: the log ends at ({}, stage {})",
            last.progress,
            last.stage
        );
        let inside: Vec<u64> = log
            .iter()
            .filter(|r| r.stage == 6 && r.progress > 0.0 && r.progress < 1.0)
            .map(|r| r.checkpoints)
            .collect();
        assert!(
            inside.len() >= 2,
            "{op}: fewer than two stage-6 reports inside (0, 1)"
        );
        // `checkpoint` parks before `report` stores its stage, so the second
        // stage-6 report parks with stage 6 already stored by the first.
        let count = inside[1];
        let parked = cancelled_parked_at(count, &|_, stage| stage == 6, render);
        assert_eq!(parked.count, count, "{op}");
    }
    unsafe { dr_image_free(img as *mut DrImage) };
}
