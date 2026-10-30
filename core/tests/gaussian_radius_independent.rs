//! The legacy Gaussian family runs a radius-independent recursive
//! Gaussian from σ4 up. Gaussian Blur itself (8-bit
//! `dr_image_filter_op` `blur`, the 16-bit `blur` selected edit) runs the
//! extended-box kernel instead (`gaussian_blur_photoshop.rs` holds it to its
//! own oracle), so the recursive filter's bound is checked here through the
//! legacy Sharpen, which keeps it (8-bit `dr_image_sharpen`, the 16-bit
//! `sharpen` edit, σ = min(amount, 5)): on mid-range opaque input nothing
//! clamps, so the blur is recovered exactly as 2·source − sharpened. The
//! tests on `blur` that do not depend on the kernel (premultiplied alpha,
//! depth agreement, cancel, refusals, plans) stay. Every expectation is
//! black-box through the C boundary. The oracle is an independent binary64
//! separable Gaussian with normalized `2·ceil(3σ)+1` taps and clamped edges,
//! compared on premultiplied samples. No Photoshop or other golden image is read.
use doctored_core::ffi::{
    dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_image_sharpen, dr_string_free,
};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

use crate::gaussian_reference;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The core's per-allocation bookkeeping allowance (1 MiB).
const BK: u64 = 1 << 20;
/// `sampling_gaussian_fast::LEGACY_FAST_SIGMA`, the switch.
const SWITCH: f32 = 4.0;
/// The largest error the step-1 measurement found between the fast filter and
/// the exact kernel at σ ≥ the switch: 0.6409/255 in premultiplied unit scale,
/// on the quadrant step at σ64, the same at 8 and 16 bits
/// (`designs/phase-9f-selected-tone-status.md`, "a radius-independent legacy
/// Gaussian"). Each bound is ceil(that × (2^d − 1)) + 1 codes, the +1
/// covering the output's own quantization. Never widened.
const BOUND_8: f64 = 2.0; // ceil(0.6409) + 1
const BOUND_16: f64 = 166.0; // ceil(0.6409 / 255 · 65535) + 1

fn take_error(error: *mut std::ffi::c_char) -> String {
    assert!(!error.is_null());
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    message
}

fn doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn request(op: &str, params: Value) -> CString {
    CString::new(json!({"layer": 0, "op": op, "params": params}).to_string()).unwrap()
}

/// The 16-bit selected edit through the C boundary; `Err` carries its message.
fn edit16(
    d: &PrecisionDocument,
    params: Value,
    control: Option<&DrJobControl>,
) -> Result<Vec<[u16; 4]>, String> {
    let request = request("blur", params);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            d,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c as *const DrJobControl),
            &mut error,
        )
    };
    if !error.is_null() {
        assert!(out.is_null());
        return Err(take_error(error));
    }
    assert!(!out.is_null(), "blur changed nothing");
    let out = unsafe { Box::from_raw(out) };
    Ok(out.layer_source(0).unwrap().to_rgba16().unwrap().to_vec())
}

/// The 16-bit `sharpen` selected edit at `amount` (σ = min(amount, 5)).
fn sharpen16(d: &PrecisionDocument, amount: f32) -> Vec<[u16; 4]> {
    let request = request("sharpen", json!({ "amount": amount }));
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            d,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null() && !out.is_null(), "sharpen σ{amount}");
    let out = unsafe { Box::from_raw(out) };
    out.layer_source(0).unwrap().to_rgba16().unwrap().to_vec()
}

/// `dr_image_sharpen`'s bytes at `amount`.
fn sharpen8(w: u32, h: u32, pixels: &[u8], amount: f32) -> Vec<u8> {
    let img = image(w, h, pixels);
    let out = unsafe { dr_image_sharpen(img, amount) };
    unsafe { dr_image_free(img) };
    assert!(!out.is_null());
    let p = unsafe { dr_image_pixels_rgba(out) };
    let bytes = unsafe { std::slice::from_raw_parts(p, pixels.len()) }.to_vec();
    unsafe { dr_image_free(out) };
    bytes
}

/// The selected edit plan's `scratch_bytes`.
fn plan16(d: &PrecisionDocument, params: Value) -> Result<u64, String> {
    let request = request("blur", params);
    let mut error = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_selected_edit_plan(d, request.as_ptr(), LIMIT, 0, &mut error) };
    if !error.is_null() {
        assert!(out.is_null());
        return Err(take_error(error));
    }
    let info: Value = serde_json::from_slice(unsafe { CStr::from_ptr(out) }.to_bytes()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(info["scratch_bytes"].as_u64().unwrap())
}

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}

/// Uncontrolled `dr_image_filter_op` `blur` at `sigma`: NULL on a refusal
/// (a σ that is not JSON, such as an infinity, is one too).
fn byte_blur(img: *const DrImage, sigma: f32) -> *mut DrImage {
    let op = CString::new("blur").unwrap();
    let params = CString::new(format!(r#"{{"sigma":{}}}"#, f64::from(sigma))).unwrap();
    unsafe {
        dr_image_filter_op(
            img,
            op.as_ptr(),
            params.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
}

/// `dr_image_filter_op` `blur`'s bytes, or None for NULL.
fn blur8(w: u32, h: u32, pixels: &[u8], sigma: f32) -> Option<Vec<u8>> {
    let img = image(w, h, pixels);
    let out = byte_blur(img, sigma);
    unsafe { dr_image_free(img) };
    if out.is_null() {
        return None;
    }
    let p = unsafe { dr_image_pixels_rgba(out) };
    assert!(!p.is_null());
    let bytes = unsafe { std::slice::from_raw_parts(p, pixels.len()) }.to_vec();
    unsafe { dr_image_free(out) };
    Some(bytes)
}

/// SplitMix64, written independently of the core's.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Straight samples in [0, 1] of the three fixture kinds: a quadrant step,
/// splitmix noise (both opaque), and a 50%-alpha disc on transparent whose
/// outside carries saturated hidden RGB.
fn fixture(kind: &str, w: u32, h: u32) -> Vec<[f64; 4]> {
    let mut state = 0x5EED_u64;
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            match kind {
                "quadrant" => {
                    let (right, low) = (x >= w / 2, y >= h / 2);
                    [
                        f64::from(u8::from(right)),
                        f64::from(u8::from(low)),
                        f64::from(u8::from(right != low)),
                        1.0,
                    ]
                }
                "noise" => {
                    let v = splitmix(&mut state);
                    [
                        (v & 0xFFFF) as f64 / 65535.0,
                        ((v >> 16) & 0xFFFF) as f64 / 65535.0,
                        ((v >> 32) & 0xFFFF) as f64 / 65535.0,
                        1.0,
                    ]
                }
                _ => {
                    let dx = f64::from(x) + 0.5 - f64::from(w) / 2.0;
                    let dy = f64::from(y) + 0.5 - f64::from(h) / 2.0;
                    if dx * dx + dy * dy <= 50.0 * 50.0 {
                        [
                            f64::from(x) / f64::from(w),
                            f64::from(y) / f64::from(h),
                            0.3,
                            0.5,
                        ]
                    } else {
                        [1.0, 0.0, 1.0, 0.0]
                    }
                }
            }
        })
        .collect()
}

/// The independent oracle: binary64 throughout, normalized taps over
/// `±ceil(3σ)`, clamped edges, separable, on premultiplied samples.
fn oracle(premultiplied: &[[f64; 4]], w: usize, h: usize, sigma: f64) -> Vec<[f64; 4]> {
    let half = (3.0 * sigma).ceil() as i64;
    let mut taps: Vec<f64> = (-half..=half)
        .map(|k| (-0.5 * (k as f64 / sigma).powi(2)).exp())
        .collect();
    let total: f64 = taps.iter().sum();
    taps.iter_mut().for_each(|t| *t /= total);
    let pass = |src: &[[f64; 4]], horizontal: bool| -> Vec<[f64; 4]> {
        let mut out = vec![[0.0; 4]; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0; 4];
                for (j, t) in taps.iter().enumerate() {
                    let o = j as i64 - half;
                    let (sx, sy) = if horizontal {
                        ((x as i64 + o).clamp(0, w as i64 - 1) as usize, y)
                    } else {
                        (x, (y as i64 + o).clamp(0, h as i64 - 1) as usize)
                    };
                    let p = src[sy * w + sx];
                    for c in 0..4 {
                        acc[c] += t * p[c];
                    }
                }
                out[y * w + x] = acc;
            }
        }
        out
    };
    pass(&pass(premultiplied, true), false)
}

fn premultiply(straight: [f64; 4]) -> [f64; 4] {
    let a = straight[3];
    [straight[0] * a, straight[1] * a, straight[2] * a, a]
}

/// The legacy Sharpen's Gaussian (the recursive filter at σ4 and σ5) is
/// within the measured bound of the exact kernel at both depths. The input is
/// the quadrant step and noise squeezed into 0.4…0.6 and opaque, so neither
/// the Gaussian nor 2·source − Gaussian leaves the code range and the
/// Gaussian is recovered exactly from the sharpened codes.
#[test]
fn fast_gaussian_is_within_the_measured_bound_of_an_exact_gaussian_at_both_depths() {
    let (w, h) = (131_u32, 300_u32);
    for kind in ["quadrant", "noise"] {
        let straight: Vec<[f64; 4]> = fixture(kind, w, h)
            .into_iter()
            .map(|p| {
                [
                    (2.0 + p[0]) / 5.0,
                    (2.0 + p[1]) / 5.0,
                    (2.0 + p[2]) / 5.0,
                    1.0,
                ]
            })
            .collect();
        let codes16: Vec<[u16; 4]> = straight
            .iter()
            .map(|p| p.map(|v| (v * 65535.0).round() as u16))
            .collect();
        let bytes: Vec<u8> = straight
            .iter()
            .flat_map(|p| p.map(|v| (v * 255.0).round() as u8))
            .collect();
        let d = doc(w, h, codes16.clone());
        let in16: Vec<[f64; 4]> = codes16
            .iter()
            .map(|p| premultiply(p.map(|v| f64::from(v) / 65535.0)))
            .collect();
        let in8: Vec<[f64; 4]> = bytes
            .chunks_exact(4)
            .map(|p| premultiply([0, 1, 2, 3].map(|c| f64::from(p[c]) / 255.0)))
            .collect();
        for sigma in [SWITCH, 5.0] {
            let want16 = oracle(&in16, w as usize, h as usize, f64::from(sigma));
            let got16 = sharpen16(&d, sigma);
            let mut worst16 = 0.0_f64;
            for ((got, source), want) in got16.iter().zip(&codes16).zip(&want16) {
                for c in 0..3 {
                    let low = 2.0 * f64::from(source[c]) - f64::from(got[c]);
                    assert!(got[c] > 0 && got[c] < 65535, "16-bit {kind}: clamped");
                    worst16 = worst16.max((low - want[c] * 65535.0).abs());
                }
            }
            assert!(
                worst16 <= BOUND_16,
                "16-bit {kind} σ{sigma}: {worst16} codes > {BOUND_16}"
            );
            let want8 = oracle(&in8, w as usize, h as usize, f64::from(sigma));
            let got8 = sharpen8(w, h, &bytes, sigma);
            let mut worst8 = 0.0_f64;
            for ((got, source), want) in got8.chunks_exact(4).zip(bytes.chunks_exact(4)).zip(&want8)
            {
                for c in 0..3 {
                    let low = 2.0 * f64::from(source[c]) - f64::from(got[c]);
                    assert!(got[c] > 0 && got[c] < 255, "8-bit {kind}: clamped");
                    worst8 = worst8.max((low - want[c] * 255.0).abs());
                }
            }
            assert!(
                worst8 <= BOUND_8,
                "8-bit {kind} σ{sigma}: {worst8} codes > {BOUND_8}"
            );
        }
    }
}

/// `blur` itself still runs the recursive filter from σ4 to below the
/// extended-box kernel (radius 7): at σ6.5 its premultiplied output,
/// partial alpha (the disc) included, is within the same measured bound of
/// the exact kernel at both depths.
#[test]
fn blur_below_the_fitted_radius_is_within_the_measured_bound_at_both_depths() {
    let (w, h) = (131_u32, 300_u32);
    let sigma = 6.5_f32;
    for kind in ["quadrant", "noise", "disc"] {
        let straight = fixture(kind, w, h);
        let codes16: Vec<[u16; 4]> = straight
            .iter()
            .map(|p| p.map(|v| (v * 65535.0).round() as u16))
            .collect();
        let bytes: Vec<u8> = straight
            .iter()
            .flat_map(|p| p.map(|v| (v * 255.0).round() as u8))
            .collect();
        let d = doc(w, h, codes16.clone());
        let in16: Vec<[f64; 4]> = codes16
            .iter()
            .map(|p| premultiply(p.map(|v| f64::from(v) / 65535.0)))
            .collect();
        let in8: Vec<[f64; 4]> = bytes
            .chunks_exact(4)
            .map(|p| premultiply([0, 1, 2, 3].map(|c| f64::from(p[c]) / 255.0)))
            .collect();
        let want16 = oracle(&in16, w as usize, h as usize, f64::from(sigma));
        let got16 = edit16(&d, json!({ "sigma": sigma }), None).unwrap();
        let mut worst16 = 0.0_f64;
        for (got, want) in got16.iter().zip(&want16) {
            let a = f64::from(got[3]);
            for c in 0..4 {
                let value = if c == 3 {
                    a
                } else {
                    f64::from(got[c]) * a / 65535.0
                };
                worst16 = worst16.max((value - want[c] * 65535.0).abs());
            }
        }
        assert!(
            worst16 <= BOUND_16,
            "16-bit {kind} σ{sigma}: {worst16} codes > {BOUND_16}"
        );
        let want8 = oracle(&in8, w as usize, h as usize, f64::from(sigma));
        let got8 = blur8(w, h, &bytes, sigma).unwrap();
        let mut worst8 = 0.0_f64;
        for (got, want) in got8.chunks_exact(4).zip(&want8) {
            let a = f64::from(got[3]);
            for c in 0..4 {
                let value = if c == 3 {
                    a
                } else {
                    f64::from(got[c]) * a / 255.0
                };
                worst8 = worst8.max((value - want[c] * 255.0).abs());
            }
        }
        assert!(
            worst8 <= BOUND_8,
            "8-bit {kind} σ{sigma}: {worst8} codes > {BOUND_8}"
        );
    }
}

#[test]
fn opaque_byte_input_agrees_across_depths_at_sigma_12() {
    let (w, h) = (131_u32, 300_u32);
    let mut state = 0x5EED_u64;
    let bytes: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = splitmix(&mut state);
            let edge = if (i % w) < w / 2 { 0 } else { 200 };
            [v as u8, edge, (v >> 8) as u8 / 2 + edge / 2, 255]
        })
        .collect();
    let codes: Vec<[u16; 4]> = bytes
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect();
    let got8 = blur8(w, h, &bytes, 12.0).unwrap();
    let got16 = edit16(&doc(w, h, codes), json!({"sigma": 12}), None).unwrap();
    for (i, (b, p)) in got8.chunks_exact(4).zip(&got16).enumerate() {
        for c in 0..4 {
            let wide = f64::from(p[c]) / 257.0;
            assert!(
                (f64::from(b[c]) - wide).abs() <= 1.0,
                "sample {i} channel {c}: {} vs {wide}",
                b[c]
            );
        }
    }
}

#[test]
fn hidden_saturated_colour_cannot_tint_at_sigma_12() {
    let (w, h) = (40_u32, 30_u32);
    let inside = |x: u32, y: u32| (12..28).contains(&x) && (9..21).contains(&y);
    let bytes: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            if inside(i % w, i / w) {
                [91, 173, 227, 255]
            } else {
                [255, 0, 13, 0]
            }
        })
        .collect();
    let got8 = blur8(w, h, &bytes, 12.0).unwrap();
    let mut visible = 0;
    for p in got8.chunks_exact(4) {
        if p[3] == 0 {
            assert_eq!(p, [0, 0, 0, 0]);
        } else {
            visible += 1;
            assert_eq!(&p[..3], [91, 173, 227]);
        }
    }
    assert!(visible > 0);
    let codes: Vec<[u16; 4]> = bytes
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect();
    let got16 = edit16(&doc(w, h, codes), json!({"sigma": 12}), None).unwrap();
    let mut visible = 0;
    for p in &got16 {
        if p[3] == 0 {
            assert_eq!(*p, [0, 0, 0, 0]);
        } else {
            visible += 1;
            assert_eq!(p[..3], [91 * 257, 173 * 257, 227 * 257]);
        }
    }
    assert!(visible > 0);
}

#[test]
fn precancelled_fast_edit_answers_only_the_cancel() {
    let (w, h) = (97_u32, 300_u32);
    let d = doc(w, h, vec![[1000, 30000, 60000, 40000]; (w * h) as usize]);
    let control = DrJobControl::default();
    control.cancel();
    let error = edit16(&d, json!({"sigma": 12}), Some(&control)).unwrap_err();
    assert_eq!(error, "operation cancelled");
    assert_eq!(control.scratch_bytes(), 0);
}

/// σ1000 is the largest σ `dr_image_filter_op` `blur` runs (on the
/// extended-box kernel; `gaussian_blur_photoshop.rs` holds σ1000 to that
/// kernel's oracle). Anything above it is NULL rather than a smaller σ.
#[test]
fn sigma_1000_is_admitted_and_larger_sigmas_are_refused() {
    let small: Vec<u8> = (0..37 * 29 * 4).map(|i| (i * 7 % 251) as u8).collect();
    assert!(blur8(37, 29, &small, 1000.0).is_some(), "σ1000 is admitted");
    for sigma in [1000.5_f32, 2000.0, f32::MAX] {
        assert!(
            blur8(37, 29, &small, sigma).is_none(),
            "σ{sigma} must be refused, not run as σ1000"
        );
    }
}

#[test]
fn camera_size_16_bit_blur_plans_admit_sigma_12_to_64() {
    // `selected_banded_filters`' one-band bound: the output codes and their
    // tiled copy, the canvas coverage lease, bookkeeping and at most 64 MiB
    // for one band of the filter's working buffers.
    let one_band = |w: u64, h: u64| {
        let n = w * h;
        6 * BK + n * 8 + n * 8 + n * 2 + 64 * 1024 * 1024
    };
    let d = doc(6000, 4000, vec![[20001, 30002, 40003, 65535]; 6000 * 4000]);
    for sigma in [12, 25] {
        let scratch = plan16(&d, json!({ "sigma": sigma })).unwrap();
        assert!(
            scratch <= one_band(6000, 4000),
            "σ{sigma}: {scratch} > {}",
            one_band(6000, 4000)
        );
    }
    // FAST_WORK_PER_PIXEL ≤ F_max(64) = 203, so the slider's top is admitted.
    plan16(&d, json!({"sigma": 64})).unwrap();
    drop(d);
    let d = doc(4000, 3000, vec![[20001, 30002, 40003, 65535]; 4000 * 3000]);
    let scratch = plan16(&d, json!({"sigma": 12})).unwrap();
    assert!(scratch <= one_band(4000, 3000), "12 MP σ12: {scratch}");
}

// ------------------------------------------------------------ calibration --

/// The explicit margin `FAST_WORK_PER_PIXEL` carries over the measured
/// figure: the Surface Blur probe's factor, for the same reasons.
const SAFETY_FACTOR: f64 = 1.15;

fn min_of_3(mut run: impl FnMut()) -> f64 {
    (0..3)
        .map(|_| {
            let start = std::time::Instant::now();
            run();
            start.elapsed().as_secs_f64()
        })
        .fold(f64::INFINITY, f64::min)
}

/// Rows of every ±(ceil(4σ)+4) window over 128-row tiles of an `h`-row plane.
fn window_rows(h: u64, sigma: f64) -> u64 {
    let halo = (4.0 * sigma).ceil() as u64 + 4;
    (0..h)
        .step_by(128)
        .map(|t0| (t0 + 128).min(h).saturating_add(halo).min(h) - t0.saturating_sub(halo))
        .sum()
}

/// The `FAST_WORK_PER_PIXEL` calibration record. Times the Surface Blur
/// probe's reference (byte High Pass σ3.9 on 3000×2000 noise, charged
/// n·(8·25+12) and converted to the radius-16 unit by
/// `gaussian_reference::R39_PER_R16`, since r16 itself runs the fast
/// Gaussian), then byte `dr_image_filter_op` `blur`, the 16-bit `blur`
/// selected edit and byte High Pass (the live family's arm) at
/// σ ∈ {4, 12, 25, 64} on the same noise. Every timing is the
/// minimum of three. Each converts to work units at the reference's rate and
/// is divided by its window pixels (`window_rows · w`). The probe prints each
/// implied figure, the largest of them, and that figure × `SAFETY_FACTOR`,
/// which `FAST_WORK_PER_PIXEL` must be at least. It also prints F_max(σ) =
/// floor(24e9 / (W(σ)·6000)) for 6000×4000.
#[test]
#[ignore = "calibration probe: make t F=gaussian_radius_independent A='--ignored --nocapture --test-threads=1'"]
fn fast_gaussian_calibration_probe() {
    let (w, h) = (3000_u32, 2000_u32);
    let n = u64::from(w) * u64::from(h);
    let mut state = 0x5EED_u64;
    let bytes: Vec<u8> = (0..n)
        .flat_map(|_| {
            let v = splitmix(&mut state);
            [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]
        })
        .collect();
    let img = image(w, h, &bytes);
    let op = CString::new("high_pass").unwrap();
    let high_pass = |radius: f32| {
        let params = CString::new(json!({ "radius": radius }).to_string()).unwrap();
        min_of_3(|| {
            let mut err = ptr::null_mut();
            let out = unsafe {
                dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), ptr::null(), &mut err)
            };
            assert!(!out.is_null());
            unsafe { dr_image_free(out) };
        })
    };
    let t_reference = high_pass(3.9);
    let rate = (n * (8 * 25 + 12)) as f64 / t_reference / gaussian_reference::R39_PER_R16;
    println!(
        "reference: high_pass σ3.9 on {w}x{h}: {t_reference:.3} s, r16-unit rate {rate:.4e} units/s"
    );
    let codes: Vec<[u16; 4]> = bytes
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect();
    let d = doc(w, h, codes);
    let mut largest = 0.0_f64;
    for sigma in [SWITCH, 12.0, 25.0, 64.0] {
        let window = (window_rows(u64::from(h), f64::from(sigma)) * u64::from(w)) as f64;
        let t8 = min_of_3(|| {
            let out = byte_blur(img, sigma);
            assert!(!out.is_null());
            unsafe { dr_image_free(out) };
        });
        let t16 = min_of_3(|| {
            edit16(&d, json!({ "sigma": sigma }), None).unwrap();
        });
        let t_hp = high_pass(sigma);
        let (u8_, u16_, hp) = (
            t8 * rate / window,
            t16 * rate / window,
            t_hp * rate / window,
        );
        largest = largest.max(u8_).max(u16_).max(hp);
        println!(
            "σ{sigma}: window pixels {window:.4e}; byte {t8:.3} s → {u8_:.1} units/px; 16-bit edit {t16:.3} s → {u16_:.1} units/px; byte high_pass {t_hp:.3} s → {hp:.1} units/px"
        );
    }
    unsafe { dr_image_free(img) };
    println!(
        "largest implied {largest:.1}; × {SAFETY_FACTOR} = {:.1} (FAST_WORK_PER_PIXEL must be at least this)",
        largest * SAFETY_FACTOR
    );
    for sigma in [12.0, 25.0, 64.0] {
        let rows = window_rows(4000, sigma);
        println!(
            "6000x4000 σ{sigma}: W = {rows}, F_max = {}",
            24_000_000_000 / (rows * 6000)
        );
    }
}
