//! Filter ▸ Sharpen ▸ Smart Sharpen: the `smart_sharpen`
//! `SpatialKernel` arm behind `dr_image_filter_op` at 8 bits and
//! `selected_edit` at 16. Every expectation is a property or the reference
//! written in `common/smart_sharpen_reference.rs`; there are no golden images
//! and no Photoshop numbers.
//!
//! The bounds, declared before the first run and never widened (a miss
//! changes a kernel constant):
//!
//! - ORACLE. Reduce Noise 0, both fades 60, tonal widths 40, radii 3, on a
//!   plate with partial alpha and transparent pixels, every radius below 4 so
//!   the 8- and 16-bit Gaussian stays on its FIR: Gaussian σ 1.5 and 3, Lens
//!   1.5 and 2.6, Motion 1.5 (distance 3) at 30°. Against the reference, ≤ 1
//!   code at 8 bits (`dr_image_filter_op`) and ≤ 3 codes at 16 (the selected
//!   edit), every channel of every pixel; alpha exact, an alpha-0 pixel as
//!   read.
//! - USM. Gaussian, Reduce Noise 0, fades 0, amount 150, radius 1.5 equals
//!   `unsharp_mask {radius 1.5, amount 1.5, threshold 0}` bit for bit at 8
//!   and 16 bits.
//! - NOISE. At 16 bits, Reduce Noise 40 on a plate equals Reduce Noise 0 on
//!   `reduce_noise {radius 0.5, luminance 0.04, detail 0, color 0}` of the
//!   plate within 4 codes (the second run reads the first's 16-bit codes).
//! - HALO. An 8-bit 160×16 strip, a step from 64 to 191 blurred by σ 1.5;
//!   Lens radius 2 amount 200 (noise 0) against `unsharp_mask` 2/2/0. H is
//!   the sum over row 8 of the overshoot past 191 and the undershoot below
//!   64: H_lens ≤ 0.25·H_usm, the largest single overshoot or undershoot of
//!   Lens ≤ 3 codes, H_usm ≥ 20, and Lens leaves fewer pixels strictly
//!   between 77 and 178 than the source has.
//! - REDUCE NOISE. A 16-bit 64×64 plate of 0.5 + N(0, 4/255), seeded; Lens
//!   radius 1 amount 200: the standard deviation over the interior (8 px in
//!   from each edge) at noise 50 is ≤ 0.75 × the one at noise 0.
//! - TONES. A 16-bit 90×12 plate of three column thirds at luma 0.12, 0.5
//!   and 0.88, each ± 0.04 of a sine down the rows; Gaussian radius 1 amount
//!   100, width 30, radius 2, fade 100. Shadows only leaves the middle and
//!   right thirds bit-identical to fades 0 and cuts the left third's mean
//!   |out − src| to ≤ 0.6× fades 0's; Highlights only mirrored.
//! - Parameters refuse by name; a pre-cancelled control returns "operation
//!   cancelled"; a coverage confines the 16-bit result; the 8-bit plan at
//!   6000×4000 is pinned to its literal, derived beside it.
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::{dr_image_filter_op, dr_image_filter_op_scratch_bytes};
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
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

use crate::smart_sharpen_reference::{smart_sharpen, Remove, Settings};

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}

/// `(pixels, message)` of `op` at 8 bits: `pixels` None for a NULL result.
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

fn filter8(w: u32, h: u32, pixels: &[u8], op: &str, params: &Value) -> Vec<u8> {
    let img = image(w, h, pixels);
    let (out, message) = run(img, op, &params.to_string(), ptr::null(), (w * h) as usize);
    unsafe { dr_image_free(img) };
    assert!(message.is_empty(), "{op} {params}: {message}");
    out.unwrap_or_else(|| panic!("{op} {params}: no result"))
}

fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn filter16(
    w: u32,
    h: u32,
    pixels: &[[u16; 4]],
    op: &str,
    params: &Value,
    coverage: &[u16],
) -> Vec<[u16; 4]> {
    let edit = SelectedEdit::parse(&json!({"layer":0,"op":op,"params":params}).to_string())
        .unwrap_or_else(|e| panic!("{op} {params}: {e}"));
    doc16(w, h, pixels.to_vec())
        .selected_edit(&edit, coverage, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap_or_else(|e| panic!("{op} {params}: {e}"))
        .unwrap_or_else(|| panic!("{op} {params}: no edit"))
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

/// Smooth ramps, a hard edge, texture, partial alpha and transparent pixels.
fn plate(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            if i % 29 == 7 {
                return [0, 0, 0, 0];
            }
            let ramp = 40 + x * 4 + y * 3;
            let edge = if x > w / 2 { 70 } else { 0 };
            let texture = (x * 7 + y * 13) % 11 * 3;
            let value = |offset: u32| ((ramp + edge + texture + offset) % 200 + 25) as u8;
            [
                value(0),
                value(60),
                value(130),
                if i % 5 == 2 { 90 + (i % 97) as u8 } else { 255 },
            ]
        })
        .collect()
}

fn unit8(pixels: &[u8]) -> Vec<[f64; 4]> {
    pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| f64::from(p[c]) / 255.0))
        .collect()
}

fn remove_name(remove: Remove) -> &'static str {
    match remove {
        Remove::Gaussian => "gaussian",
        Remove::Lens => "lens",
        Remove::Motion => "motion",
    }
}

fn params_of(s: &Settings) -> Value {
    json!({
        "amount": s.amount, "radius": s.radius, "noise": 0, "remove": remove_name(s.remove),
        "angle": s.angle,
        "shadow_fade": s.shadows.0, "shadow_tonal_width": s.shadows.1,
        "shadow_radius": s.shadows.2,
        "highlight_fade": s.highlights.0, "highlight_tonal_width": s.highlights.1,
        "highlight_radius": s.highlights.2,
    })
}

// ---- ORACLE -----------------------------------------------------------------

#[test]
fn every_removal_matches_the_reference_at_both_depths() {
    let (w, h) = (40_u32, 30_u32);
    let src8 = plate(w, h);
    let src16 = widen(&src8);
    let mut failures = Vec::new();
    for (remove, radius, angle) in [
        (Remove::Gaussian, 1.5, 0.0),
        (Remove::Gaussian, 3.0, 0.0),
        (Remove::Lens, 1.5, 0.0),
        (Remove::Lens, 2.6, 0.0),
        (Remove::Motion, 1.5, 30.0),
    ] {
        let settings = Settings {
            amount: 120.0,
            radius,
            remove,
            angle,
            shadows: (60.0, 40.0, 3),
            highlights: (60.0, 40.0, 3),
        };
        let params = params_of(&settings);
        let want = smart_sharpen(&unit8(&src8), w as usize, h as usize, &settings);
        let out8 = filter8(w, h, &src8, "smart_sharpen", &params);
        let out16 = filter16(w, h, &src16, "smart_sharpen", &params, &[]);
        for (i, reference) in want.iter().enumerate() {
            if src8[i * 4 + 3] == 0 {
                assert_eq!(
                    out8[i * 4..i * 4 + 4],
                    src8[i * 4..i * 4 + 4],
                    "{params} {i}"
                );
                assert_eq!(out16[i], src16[i], "{params} {i}");
                continue;
            }
            assert_eq!(out8[i * 4 + 3], src8[i * 4 + 3], "{params} alpha {i}");
            assert_eq!(out16[i][3], src16[i][3], "{params} alpha {i}");
            for c in 0..3 {
                let e8 = (f64::from(out8[i * 4 + c]) - reference[c] * 255.0).abs();
                let e16 = (f64::from(out16[i][c]) - reference[c] * 65535.0).abs();
                if e8 > 1.0 || e16 > 3.0 {
                    failures.push(format!(
                        "{remove:?} r{radius} pixel {i} c{c}: 8-bit off {e8:.2}, 16-bit off {e16:.2}"
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures[..failures.len().min(20)].join("\n")
    );
}

// ---- USM ----------------------------------------------------------------------

#[test]
fn gaussian_without_noise_or_fades_is_unsharp_mask_bit_for_bit() {
    let (w, h) = (37_u32, 23_u32);
    let src8 = plate(w, h);
    let src16 = widen(&src8);
    let smart = json!({"amount": 150, "radius": 1.5, "noise": 0, "remove": "gaussian"});
    let usm = json!({"radius": 1.5, "amount": 1.5, "threshold": 0});
    assert_eq!(
        filter8(w, h, &src8, "smart_sharpen", &smart),
        filter8(w, h, &src8, "unsharp_mask", &usm)
    );
    assert_eq!(
        filter16(w, h, &src16, "smart_sharpen", &smart, &[]),
        filter16(w, h, &src16, "unsharp_mask", &usm, &[])
    );
}

// ---- NOISE --------------------------------------------------------------------

#[test]
fn reduce_noise_is_reduce_noise_before_the_sharpen() {
    let (w, h) = (40_u32, 30_u32);
    let src16 = widen(&plate(w, h));
    for remove in ["gaussian", "lens", "motion"] {
        let params = |noise: u32| {
            json!({"amount": 120, "radius": 1.5, "noise": noise, "remove": remove,
                "angle": 20, "shadow_fade": 30, "highlight_fade": 30})
        };
        let together = filter16(w, h, &src16, "smart_sharpen", &params(40), &[]);
        let denoised = filter16(
            w,
            h,
            &src16,
            "reduce_noise",
            &json!({"radius": 0.5, "luminance": 0.04, "detail": 0, "color": 0}),
            &[],
        );
        let apart = filter16(w, h, &denoised, "smart_sharpen", &params(0), &[]);
        for (i, (a, b)) in together.iter().zip(&apart).enumerate() {
            for c in 0..4 {
                assert!(
                    (i32::from(a[c]) - i32::from(b[c])).abs() <= 4,
                    "{remove} pixel {i} c{c}: {a:?} vs {b:?}"
                );
            }
        }
    }
}

// ---- HALO -----------------------------------------------------------------------

const HALO_W: u32 = 160;
const HALO_H: u32 = 16;

/// Φ, by the Abramowitz–Stegun 7.1.26 erf (|error| ≤ 1.5e-7).
fn normal_cdf(z: f64) -> f64 {
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    if z >= 0.0 {
        0.5 * (1.0 + erf)
    } else {
        0.5 * (1.0 - erf)
    }
}

fn step_strip() -> Vec<u8> {
    (0..HALO_W * HALO_H)
        .flat_map(|i| {
            let x = f64::from(i % HALO_W) + 0.5;
            let v = (64.0 + 127.0 * normal_cdf((x - 80.0) / 1.5)).round() as u8;
            [v, v, v, 255]
        })
        .collect()
}

/// `(H, the largest single excursion)` of row 8, in 8-bit levels.
fn halo(pixels: &[u8]) -> (f64, f64) {
    let (mut total, mut worst) = (0.0, 0.0_f64);
    for x in 0..HALO_W {
        let v = f64::from(pixels[((8 * HALO_W + x) * 4) as usize]);
        let excursion = (v - 191.0).max(0.0) + (64.0 - v).max(0.0);
        total += excursion;
        worst = worst.max(excursion);
    }
    (total, worst)
}

fn mid_band(pixels: &[u8]) -> usize {
    (0..HALO_W)
        .filter(|x| {
            let v = pixels[((8 * HALO_W + x) * 4) as usize];
            v > 77 && v < 178
        })
        .count()
}

#[test]
fn lens_removal_draws_a_quarter_of_the_unsharp_mask_halo() {
    let src = step_strip();
    let lens = filter8(
        HALO_W,
        HALO_H,
        &src,
        "smart_sharpen",
        &json!({"amount": 200, "radius": 2, "noise": 0, "remove": "lens"}),
    );
    let usm = filter8(
        HALO_W,
        HALO_H,
        &src,
        "unsharp_mask",
        &json!({"radius": 2, "amount": 2, "threshold": 0}),
    );
    let ((h_lens, worst), (h_usm, _)) = (halo(&lens), halo(&usm));
    println!(
        "HALO H_lens {h_lens} (worst {worst}) H_usm {h_usm}; mid band {} -> {}",
        mid_band(&src),
        mid_band(&lens)
    );
    assert!(h_usm >= 20.0, "the fixture draws no USM halo: {h_usm}");
    assert!(h_lens <= 0.25 * h_usm, "H_lens {h_lens} vs H_usm {h_usm}");
    assert!(worst <= 3.0, "a lens excursion of {worst} levels");
    assert!(
        mid_band(&lens) < mid_band(&src),
        "the edge is no steeper: {} vs {}",
        mid_band(&lens),
        mid_band(&src)
    );
}

// ---- REDUCE NOISE ---------------------------------------------------------------

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn noisy_plate(w: u32, h: u32) -> Vec<[u16; 4]> {
    let mut state = 0x5eed_u64;
    let mut uniform = || ((splitmix(&mut state) >> 11) + 1) as f64 / (1_u64 << 53) as f64;
    (0..w * h)
        .map(|_| {
            let (u1, u2) = (uniform(), uniform());
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            let v = ((0.5 + z * 4.0 / 255.0).clamp(0.0, 1.0) * 65535.0).round() as u16;
            [v, v, v, 65535]
        })
        .collect()
}

fn interior_std(pixels: &[[u16; 4]], w: u32, h: u32) -> f64 {
    let values: Vec<f64> = (8..h - 8)
        .flat_map(|y| (8..w - 8).map(move |x| (y * w + x) as usize))
        .map(|i| f64::from(pixels[i][1]) / 65535.0)
        .collect();
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / values.len() as f64).sqrt()
}

#[test]
fn reduce_noise_lowers_the_noise_in_a_flat_region() {
    let (w, h) = (64_u32, 64_u32);
    let src = noisy_plate(w, h);
    let std_at = |noise: u32| {
        let params = json!({"amount": 200, "radius": 1, "noise": noise, "remove": "lens"});
        interior_std(&filter16(w, h, &src, "smart_sharpen", &params, &[]), w, h)
    };
    let (quiet, loud) = (std_at(50), std_at(0));
    println!("REDUCE NOISE std at noise 50 {quiet}, at noise 0 {loud}");
    assert!(quiet <= 0.75 * loud, "{quiet} vs {loud}");
}

// ---- TONES ----------------------------------------------------------------------

const TONE_W: u32 = 90;
const TONE_H: u32 = 12;

fn thirds() -> Vec<[u16; 4]> {
    (0..TONE_W * TONE_H)
        .map(|i| {
            let (x, y) = (i % TONE_W, i / TONE_W);
            let base = [0.12, 0.5, 0.88][(x / (TONE_W / 3)) as usize];
            let v = base + 0.04 * (2.0 * std::f64::consts::PI * f64::from(y) / 6.0).sin();
            let code = (v * 65535.0).round() as u16;
            [code, code, code, 65535]
        })
        .collect()
}

fn mean_change(out: &[[u16; 4]], src: &[[u16; 4]], third: u32) -> f64 {
    let columns = third * TONE_W / 3..(third + 1) * TONE_W / 3;
    let (mut sum, mut count) = (0.0, 0.0);
    for y in 0..TONE_H {
        for x in columns.clone() {
            let i = (y * TONE_W + x) as usize;
            sum += (f64::from(out[i][0]) - f64::from(src[i][0])).abs();
            count += 1.0;
        }
    }
    sum / count
}

#[test]
fn the_fades_act_only_in_their_tonal_ranges() {
    let src = thirds();
    let base = json!({"amount": 100, "radius": 1, "noise": 0, "remove": "gaussian"});
    let with = |extra: Value| {
        let mut params = base.clone();
        for (k, v) in extra.as_object().unwrap() {
            params[k] = v.clone();
        }
        filter16(TONE_W, TONE_H, &src, "smart_sharpen", &params, &[])
    };
    let plain = with(json!({}));
    let shadows = with(json!({"shadow_fade": 100, "shadow_tonal_width": 30, "shadow_radius": 2}));
    let highlights =
        with(json!({"highlight_fade": 100, "highlight_tonal_width": 30, "highlight_radius": 2}));
    let third_of = |i: usize| (i as u32 % TONE_W) / (TONE_W / 3);
    for (i, ((p, s), hl)) in plain.iter().zip(&shadows).zip(&highlights).enumerate() {
        if third_of(i) != 0 {
            assert_eq!(p, s, "shadows moved pixel {i} outside the shadows");
        }
        if third_of(i) != 2 {
            assert_eq!(p, hl, "highlights moved pixel {i} outside the highlights");
        }
    }
    let (left_plain, left_faded) = (mean_change(&plain, &src, 0), mean_change(&shadows, &src, 0));
    let (right_plain, right_faded) = (
        mean_change(&plain, &src, 2),
        mean_change(&highlights, &src, 2),
    );
    println!(
        "TONES shadows {left_faded} of {left_plain}; highlights {right_faded} of {right_plain}"
    );
    assert!(left_plain > 0.0 && right_plain > 0.0);
    assert!(
        left_faded <= 0.6 * left_plain,
        "{left_faded} vs {left_plain}"
    );
    assert!(
        right_faded <= 0.6 * right_plain,
        "{right_faded} vs {right_plain}"
    );
}

// ---- refusals, cancel, coverage -------------------------------------------------

#[test]
fn parameters_refuse_by_name() {
    let (w, h) = (4_u32, 3_u32);
    let img = image(w, h, &[128; 48]);
    for (params, wanted) in [
        (json!({"amount": 0}), "amount must be finite in [1,500]"),
        (json!({"amount": 501}), "amount must be finite in [1,500]"),
        (json!({"radius": 0.05}), "radius must be finite in [0.1,64]"),
        (json!({"noise": 101}), "noise must be finite in [0,100]"),
        (
            json!({"remove": "box"}),
            "remove must be one of gaussian, lens, motion",
        ),
        (json!({"angle": 181}), "angle must be finite in [-180,180]"),
        (
            json!({"shadow_fade": -1}),
            "shadow_fade must be finite in [0,100]",
        ),
        (
            json!({"highlight_tonal_width": 100.5}),
            "highlight_tonal_width must be finite in [0,100]",
        ),
        (
            json!({"shadow_radius": 1.5}),
            "shadow_radius must be an integer",
        ),
        (
            json!({"highlight_radius": 0}),
            "highlight_radius must be finite in [1,100]",
        ),
        (
            json!({"threshold": 3}),
            "unexpected smart_sharpen parameter threshold",
        ),
    ] {
        let (out, message) = run(img, "smart_sharpen", &params.to_string(), ptr::null(), 12);
        assert!(out.is_none(), "{params} was accepted");
        assert_eq!(message, wanted, "{params}");
    }
    // The defaults are never a bypass.
    let (out, message) = run(img, "smart_sharpen", "{}", ptr::null(), 12);
    assert!(out.is_some() && message.is_empty(), "{message}");
    unsafe { dr_image_free(img) };
}

#[test]
fn a_cancelled_control_stops_the_run() {
    let (w, h) = (24_u32, 18_u32);
    let img = image(w, h, &plate(w, h));
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let (out, message) = run(
        img,
        "smart_sharpen",
        r#"{"noise":30,"shadow_fade":40}"#,
        control,
        (w * h) as usize,
    );
    unsafe {
        dr_job_control_free(control);
        dr_image_free(img);
    }
    assert!(out.is_none());
    assert_eq!(message, "operation cancelled");
}

#[test]
fn a_coverage_confines_the_16_bit_result() {
    let (w, h) = (36_u32, 28_u32);
    let src = widen(&plate(w, h));
    let inside = |x: u32, y: u32| (5..30).contains(&x) && (4..22).contains(&y);
    let coverage: Vec<u16> = (0..w * h)
        .map(|i| if inside(i % w, i / w) { 65535 } else { 0 })
        .collect();
    let params = json!({"amount": 180, "radius": 2, "noise": 20, "remove": "motion", "angle": 45});
    let whole = filter16(w, h, &src, "smart_sharpen", &params, &[]);
    let confined = filter16(w, h, &src, "smart_sharpen", &params, &coverage);
    for i in 0..(w * h) as usize {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let wanted = if inside(x, y) { whole[i] } else { src[i] };
        assert_eq!(confined[i], wanted, "{x},{y}");
    }
}

// ---- the plan -------------------------------------------------------------------

/// The 8-bit plan of a 6000×4000 image at the admission's settings (Lens
/// radius 4, amount 200, Reduce Noise 50, both fades 50 at width 50 and
/// radius 8), from the stages `SmartSharpen::plan_in` documents. n =
/// 24,000,000, and every stage adds the 4n margin.
/// - Reduce Noise: `base_plan` at half-width 2, the n×16 correction plane,
///   five box rings of min(2·2 + 2, 4000) + 1 = 7 rows and a delay ring of 3
///   rows plus 2 regression rows, each 6000 binary64 (1,920,000), four
///   bookkeeping MiB and the n×4 delta: 486,114,304, plus 4n.
/// - Each classifier: two rings of 2·8 + 2 + 1 = 19 rows of 6000 binary64
///   (1,824,000) and its n×4 plane, beside the planes kept before it.
/// - The disc: the n×16 plane, a ring of 9 rows of 6009 five-binary64 sums
///   and 5·9 span entries (2,163,420), two bookkeeping MiB, beside the three
///   kept planes (12n): 772,260,572, the widest stage.
///
/// The arm adds two bookkeeping MiB; `filter_op_scratch_bytes` four more and
/// a quarter.
const PLAN_24MP: u64 = 973_190_035;

#[test]
fn the_24_megapixel_byte_plan_is_pinned() {
    let (w, h) = (6000_u32, 4000_u32);
    let bytes = vec![0_u8; w as usize * h as usize * 4];
    let img = image(w, h, &bytes);
    drop(bytes);
    let params = json!({"amount": 200, "radius": 4, "noise": 50, "remove": "lens",
        "shadow_fade": 50, "shadow_tonal_width": 50, "shadow_radius": 8,
        "highlight_fade": 50, "highlight_tonal_width": 50, "highlight_radius": 8});
    let (name, text) = (
        CString::new("smart_sharpen").unwrap(),
        CString::new(params.to_string()).unwrap(),
    );
    let mut err: *mut c_char = ptr::null_mut();
    let planned =
        unsafe { dr_image_filter_op_scratch_bytes(img, name.as_ptr(), text.as_ptr(), &mut err) };
    assert!(err.is_null());
    unsafe { dr_image_free(img) };
    let n = 24_000_000_u64;
    let mib = 1024 * 1024_u64;
    let noise = 16 * n + (5 * 7 + 3 + 2) * 6000 * 8 + 4 * mib + 4 * n + 4 * n;
    let rings = 2 * 19 * 6000 * 8;
    let shadows = 4 * n + 4 * n + rings + 4 * n;
    let highlights = 8 * n + 4 * n + rings + 4 * n;
    let disc = 16 * n + (9 * 6009 * 40 + 5 * 9 * 4) + 2 * mib;
    let lens = 12 * n + disc + 4 * n;
    let kernel = noise.max(shadows).max(highlights).max(lens) + 2 * mib;
    let base = kernel.max(20 * n) + 4 * mib;
    assert_eq!(base + base.div_ceil(4), PLAN_24MP, "the derivation");
    assert_eq!(planned, PLAN_24MP);
}
