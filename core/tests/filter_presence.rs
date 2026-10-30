//! Filters ▸ Presence: the `presence` `SpatialKernel` arm (Texture,
//! Clarity, Dehaze) behind `dr_image_filter_op` at 8 bits and `selected_edit`
//! at 16. Every expectation is a property or a reference written here; there
//! are no golden images and no Photoshop numbers.
//!
//! The bounds, declared before the first run and never widened (a miss
//! changes a kernel constant, and a Flat miss is a formula defect):
//!
//! - FLAT. A picture of one colour (mid grey, dark grey 5/255, pure red
//!   255/0/0, black, white) is the bypass or comes back byte for byte, at
//!   amounts ±100 and ±37 of each slider alone and of all three, both depths.
//! - ZERO. `{}` and all three at 0 answer no result and no error, plan 0
//!   bytes, and make no 16-bit edit.
//! - BANDS. A 2048×16 opaque grey strip `Y = 0.5 + 0.02·sin(2π(x + ½)/P)` at
//!   16 bits (the sinusoid is taken at pixel centres, so the P = 2 strip is a
//!   real alternation and not a flat row). Contrast is the least-squares
//!   amplitude of that same waveform over the middle 1024 columns (the fit is
//!   `c + A·sin(2π(x + ½)/P)`: the bases are symmetric windows, so the output
//!   keeps the input's phase), and the ratio is output over input.
//!   Texture's band is P ∈ {8, 12, 16} and outside it P ∈ {2, 4, 128, 512};
//!   Clarity's band is P ∈ {64, 96, 128} and outside it P ∈ {2, 4, 8, 512}.
//!   At +100 every in-band ratio is ≥ 1.5 and every outside |ratio − 1| ≤
//!   0.15; at −100 every in-band ratio is ≤ 0.6 and every outside |ratio − 1|
//!   ≤ 0.15. One 8-bit strip at amplitude 0.04 checks direction only.
//! - STEP. A 2048×16 strip, 0.25 grey | 0.75 grey, at 16 bits: the overshoot
//!   above the high level and the undershoot below the low one are each ≤ 5%
//!   of the step for Clarity +100 alone and for Texture +100 alone, and ≤ 8%
//!   with both.
//! - DEHAZE. A 256×256 scene of 8-pixel tiles whose colours each have one
//!   channel at 0 (the dark-channel prior, by construction), hazed here by
//!   `I = J·t + A·(1 − t)` with `A = (0.90, 0.92, 0.95)`: a sky band of
//!   `t = 0` on the top 48 rows, `t = 0.8` in the left half below it and
//!   `t = 0.5` in the right. At +100, over the non-sky pixels more than 16
//!   pixels from a zone boundary, mean |out − J| ≤ 0.05 and ≤ ¼ of mean
//!   |I − J|. At −50 and −100 on the clean scene (sky kept), every such
//!   pixel's |out_c − A_c| ≤ |J_c − A_c|, strictly smaller where the two
//!   differ by more than 2 levels, and −100 moves farther than −50.
//! - The depths agree within one 8-bit level; a coverage confines the 16-bit
//!   result exactly; parameters refuse by name; NULL is tolerated; the run
//!   reports progress and a parked cancel returns "operation cancelled".
//! - The 8-bit plan at 6000×4000 with all three sliders is pinned to its
//!   literal, derived beside it, and so is the 16-bit plan of the same edit.
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

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}

/// `(pixels, message)`: `pixels` is None for a NULL result, `message` empty
/// when no error was written.
fn run(
    img: *const DrImage,
    params: &str,
    control: *const DrJobControl,
    count: usize,
) -> (Option<Vec<u8>>, String) {
    let op = CString::new("presence").unwrap();
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

/// The 8-bit result, or None for the bypass.
fn filter8(w: u32, h: u32, pixels: &[u8], params: &Value) -> Option<Vec<u8>> {
    let img = image(w, h, pixels);
    let (out, message) = run(img, &params.to_string(), ptr::null(), (w * h) as usize);
    unsafe { dr_image_free(img) };
    assert!(message.is_empty(), "{params}: {message}");
    out
}

fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// The 16-bit result through the selected-edit path, or None for no edit.
fn filter16(
    w: u32,
    h: u32,
    pixels: &[[u16; 4]],
    params: &Value,
    coverage: &[u16],
) -> Option<Vec<[u16; 4]>> {
    let edit = SelectedEdit::parse(&json!({"layer":0,"op":"presence","params":params}).to_string())
        .unwrap_or_else(|e| panic!("{params}: {e}"));
    doc16(w, h, pixels.to_vec())
        .selected_edit(&edit, coverage, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap_or_else(|e| panic!("{params}: {e}"))
        .map(|d| d.layer_source(0).unwrap().to_rgba16().unwrap())
}

fn widen(pixels: &[u8]) -> Vec<[u16; 4]> {
    pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect()
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

// ---- Flat and zero ----------------------------------------------------------

#[test]
fn a_flat_picture_is_unchanged_at_every_amount() {
    let (w, h) = (64_u32, 48_u32);
    let mut settings = Vec::new();
    for amount in [100, -100, 37, -37] {
        for key in ["texture", "clarity", "dehaze"] {
            settings.push(json!({ key: amount }));
        }
        settings.push(json!({"texture":amount,"clarity":amount,"dehaze":amount}));
    }
    for colour in [
        [128_u8, 128, 128],
        [5, 5, 5],
        [255, 0, 0],
        [0, 0, 0],
        [255, 255, 255],
    ] {
        let flat8: Vec<u8> = (0..w * h)
            .flat_map(|_| [colour[0], colour[1], colour[2], 255])
            .collect();
        let flat16 = widen(&flat8);
        for params in &settings {
            if let Some(out) = filter8(w, h, &flat8, params) {
                assert!(out == flat8, "{colour:?} {params}: 8-bit bytes moved");
            }
            if let Some(out) = filter16(w, h, &flat16, params, &[]) {
                assert!(out == flat16, "{colour:?} {params}: 16-bit codes moved");
            }
        }
    }
}

fn plan(img: *const DrImage, params: &str) -> u64 {
    let (name, text) = (
        CString::new("presence").unwrap(),
        CString::new(params).unwrap(),
    );
    let mut err: *mut c_char = ptr::null_mut();
    let bytes =
        unsafe { dr_image_filter_op_scratch_bytes(img, name.as_ptr(), text.as_ptr(), &mut err) };
    assert!(err.is_null(), "{params}");
    bytes
}

#[test]
fn all_zero_is_the_exact_bypass_and_plans_nothing() {
    let (w, h) = (24_u32, 18_u32);
    let src = plate(w, h);
    let img = image(w, h, &src);
    for params in ["{}", r#"{"texture":0,"clarity":0,"dehaze":0}"#] {
        let (out, message) = run(img, params, ptr::null(), (w * h) as usize);
        assert!(out.is_none() && message.is_empty(), "{params}: {message}");
        assert_eq!(plan(img, params), 0, "{params}");
        let value: Value = serde_json::from_str(params).unwrap();
        assert!(
            filter16(w, h, &widen(&src), &value, &[]).is_none(),
            "{params}: a 16-bit edit"
        );
    }
    for params in [
        r#"{"texture":10}"#,
        r#"{"clarity":-10}"#,
        r#"{"dehaze":10}"#,
        r#"{"dehaze":-10}"#,
    ] {
        // At least the n×16 plane the kernel returns.
        assert!(plan(img, params) >= u64::from(w * h) * 16, "{params}");
    }
    unsafe { dr_image_free(img) };
}

// ---- Bands and the step -------------------------------------------------------

const STRIP_W: u32 = 2048;
const STRIP_H: u32 = 16;

/// The test waveform at column `x`: a unit sinusoid of period `period`, taken
/// at the pixel's centre.
fn wave(x: u32, period: f64) -> f64 {
    (2.0 * std::f64::consts::PI * (f64::from(x) + 0.5) / period).sin()
}

/// The strip's grey value at column `x`, in 0…1.
fn strip_value(x: u32, period: f64, amplitude: f64) -> f64 {
    0.5 + amplitude * wave(x, period)
}

fn strip16(value: impl Fn(u32) -> f64) -> Vec<[u16; 4]> {
    (0..STRIP_W * STRIP_H)
        .map(|i| {
            let v = (value(i % STRIP_W) * 65535.0).round() as u16;
            [v, v, v, 65535]
        })
        .collect()
}

fn strip8(value: impl Fn(u32) -> f64) -> Vec<u8> {
    (0..STRIP_W * STRIP_H)
        .flat_map(|i| {
            let v = (value(i % STRIP_W) * 255.0).round() as u8;
            [v, v, v, 255]
        })
        .collect()
}

/// The least-squares amplitude `A` of `c + A·wave` over the middle 1024
/// columns of `row` (two unknowns, the normal equations solved directly).
fn amplitude(row: impl Fn(u32) -> f64, period: f64) -> f64 {
    let (mut n, mut s, mut ss, mut v, mut sv) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for x in STRIP_W / 4..STRIP_W * 3 / 4 {
        let (basis, value) = (wave(x, period), row(x));
        n += 1.0;
        s += basis;
        ss += basis * basis;
        v += value;
        sv += basis * value;
    }
    (n * sv - s * v) / (n * ss - s * s)
}

/// Output contrast over input contrast at `period`, on the 16-bit strip's
/// middle row.
fn ratio16(period: f64, params: &Value) -> f64 {
    let src = strip16(|x| strip_value(x, period, 0.02));
    let out = filter16(STRIP_W, STRIP_H, &src, params, &[])
        .unwrap_or_else(|| panic!("{params} at P {period}: no edit"));
    let at = |pixels: &[[u16; 4]], x: u32| {
        f64::from(pixels[((STRIP_H / 2) * STRIP_W + x) as usize][0]) / 65535.0
    };
    amplitude(|x| at(&out, x), period) / amplitude(|x| at(&src, x), period)
}

#[test]
fn texture_and_clarity_move_their_own_band_and_leave_the_others() {
    let mut failures = Vec::new();
    for (key, inside, outside) in [
        ("texture", [8.0, 12.0, 16.0], [2.0, 4.0, 128.0, 512.0]),
        ("clarity", [64.0, 96.0, 128.0], [2.0, 4.0, 8.0, 512.0]),
    ] {
        for amount in [100, -100] {
            let params = json!({ key: amount });
            for period in inside {
                let ratio = ratio16(period, &params);
                let ok = if amount > 0 {
                    ratio >= 1.5
                } else {
                    ratio <= 0.6
                };
                if !ok {
                    failures.push(format!("{params} in band at P {period}: ratio {ratio:.4}"));
                }
            }
            for period in outside {
                let ratio = ratio16(period, &params);
                if (ratio - 1.0).abs() > 0.15 {
                    failures.push(format!("{params} outside at P {period}: ratio {ratio:.4}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_byte_path_moves_each_band_the_same_way() {
    for (key, period) in [("texture", 12.0), ("clarity", 96.0)] {
        let src = strip8(|x| strip_value(x, period, 0.04));
        let at = |pixels: &[u8], x: u32| {
            f64::from(pixels[(((STRIP_H / 2) * STRIP_W + x) * 4) as usize]) / 255.0
        };
        let before = amplitude(|x| at(&src, x), period);
        for amount in [100, -100] {
            let params = json!({ key: amount });
            let out = filter8(STRIP_W, STRIP_H, &src, &params)
                .unwrap_or_else(|| panic!("{params}: a bypass"));
            let after = amplitude(|x| at(&out, x), period);
            if amount > 0 {
                assert!(after > before, "{params}: {after} is not above {before}");
            } else {
                assert!(after < before, "{params}: {after} is not below {before}");
            }
        }
    }
}

#[test]
fn a_hard_step_keeps_its_overshoot_under_the_bound() {
    let level = |x: u32| if x < STRIP_W / 2 { 0.25 } else { 0.75 };
    let src = strip16(level);
    let (low, high) = (
        f64::from(src[0][0]),
        f64::from(src[(STRIP_W - 1) as usize][0]),
    );
    for (params, bound) in [
        (json!({"clarity":100}), 0.05),
        (json!({"texture":100}), 0.05),
        (json!({"texture":100,"clarity":100}), 0.08),
    ] {
        let out = filter16(STRIP_W, STRIP_H, &src, &params, &[])
            .unwrap_or_else(|| panic!("{params}: no edit"));
        let row = &out[((STRIP_H / 2) * STRIP_W) as usize..((STRIP_H / 2 + 1) * STRIP_W) as usize];
        let (mut least, mut most) = (f64::MAX, f64::MIN);
        for p in row {
            least = least.min(f64::from(p[0]));
            most = most.max(f64::from(p[0]));
        }
        let overshoot = (most - high) / (high - low);
        let undershoot = (low - least) / (high - low);
        assert!(
            overshoot <= bound && undershoot <= bound,
            "{params}: overshoot {overshoot:.4}, undershoot {undershoot:.4}, bound {bound}"
        );
    }
}

// ---- Dehaze -------------------------------------------------------------------

const SCENE: u32 = 256;
const TILE: u32 = 8;
const SKY_ROWS: u32 = 48;
const AIRLIGHT: [f64; 3] = [0.90, 0.92, 0.95];

/// The clean scene below the sky: 8-pixel tiles, each one colour with one
/// channel at 0 and the other two between 0.25 and 1.
fn scene(x: u32, y: u32) -> [f64; 3] {
    let (tx, ty) = (x / TILE, y / TILE);
    let level = |k: u32| 0.25 + 0.75 * f64::from((tx * 37 + ty * 59 + k * 101) % 97) / 96.0;
    let mut colour = [level(0), level(1), level(2)];
    colour[((tx + 2 * ty) % 3) as usize] = 0.0;
    colour
}

fn transmission(x: u32, y: u32) -> f64 {
    if y < SKY_ROWS {
        0.0
    } else if x < SCENE / 2 {
        0.8
    } else {
        0.5
    }
}

/// The picture: the sky band is the airlight; below it the scene, hazed by
/// the model when `hazed`.
fn picture(x: u32, y: u32, hazed: bool) -> [f64; 3] {
    if y < SKY_ROWS {
        return AIRLIGHT;
    }
    let (j, t) = (scene(x, y), transmission(x, y));
    if !hazed {
        return j;
    }
    [0, 1, 2].map(|c| j[c] * t + AIRLIGHT[c] * (1.0 - t))
}

/// Below the sky and more than 16 pixels from the sky's edge and from the
/// line between the two transmission zones.
fn measured(x: u32, y: u32) -> bool {
    y > SKY_ROWS + 16 && (x + 17 < SCENE / 2 || x > SCENE / 2 + 16)
}

fn picture8(hazed: bool) -> Vec<u8> {
    (0..SCENE * SCENE)
        .flat_map(|i| {
            let p = picture(i % SCENE, i / SCENE, hazed).map(|v| (v * 255.0).round() as u8);
            [p[0], p[1], p[2], 255]
        })
        .collect()
}

/// Mean |a − b| over the measured pixels' colour channels; `a` and `b` answer
/// a pixel's colour in 0…1.
fn mean_error(a: impl Fn(u32, u32) -> [f64; 3], b: impl Fn(u32, u32) -> [f64; 3]) -> f64 {
    let (mut sum, mut count) = (0.0, 0.0);
    for y in 0..SCENE {
        for x in 0..SCENE {
            if !measured(x, y) {
                continue;
            }
            let (p, q) = (a(x, y), b(x, y));
            for c in 0..3 {
                sum += (p[c] - q[c]).abs();
                count += 1.0;
            }
        }
    }
    sum / count
}

#[test]
fn dehaze_recovers_a_scene_hazed_by_the_model() {
    let params = json!({"dehaze":100});
    let hazed8 = picture8(true);
    let hazed16 = widen(&hazed8);
    let out8 = filter8(SCENE, SCENE, &hazed8, &params).expect("an 8-bit result");
    let out16 = filter16(SCENE, SCENE, &hazed16, &params, &[]).expect("a 16-bit edit");
    let index = |x: u32, y: u32| (y * SCENE + x) as usize;
    let byte = |pixels: &[u8], x: u32, y: u32| {
        [0, 1, 2].map(|c| f64::from(pixels[index(x, y) * 4 + c]) / 255.0)
    };
    let wide = |pixels: &[[u16; 4]], x: u32, y: u32| {
        [0, 1, 2].map(|c| f64::from(pixels[index(x, y)][c]) / 65535.0)
    };
    let before = mean_error(|x, y| byte(&hazed8, x, y), scene);
    for (depth, after) in [
        (8, mean_error(|x, y| byte(&out8, x, y), scene)),
        (16, mean_error(|x, y| wide(&out16, x, y), scene)),
    ] {
        assert!(
            after <= 0.05 && after <= before / 4.0,
            "{depth}-bit: mean error {after:.4} from {before:.4}"
        );
    }
    // The sky is the airlight and stays it.
    for x in [0, SCENE / 2, SCENE - 1] {
        for c in 0..3 {
            let (got, was) = (out8[index(x, 8) * 4 + c], hazed8[index(x, 8) * 4 + c]);
            assert!(
                got.abs_diff(was) <= 1,
                "sky {x} channel {c}: {got} from {was}"
            );
        }
    }
}

#[test]
fn negative_dehaze_moves_the_clean_scene_toward_the_airlight() {
    let clean8 = picture8(false);
    let clean16 = widen(&clean8);
    // Distance from the stored airlight (the sky's own samples), in the
    // depth's codes, of every measured pixel's channels.
    let distances8 = |pixels: &[u8]| -> Vec<i64> {
        let mut out = Vec::new();
        for y in 0..SCENE {
            for x in 0..SCENE {
                if measured(x, y) {
                    let i = ((y * SCENE + x) * 4) as usize;
                    for c in 0..3 {
                        out.push((i64::from(pixels[i + c]) - i64::from(clean8[c])).abs());
                    }
                }
            }
        }
        out
    };
    let distances16 = |pixels: &[[u16; 4]]| -> Vec<i64> {
        let mut out = Vec::new();
        for y in 0..SCENE {
            for x in 0..SCENE {
                if measured(x, y) {
                    let i = (y * SCENE + x) as usize;
                    for c in 0..3 {
                        out.push((i64::from(pixels[i][c]) - i64::from(clean16[0][c])).abs());
                    }
                }
            }
        }
        out
    };
    let run8 = |amount: i32| {
        distances8(&filter8(SCENE, SCENE, &clean8, &json!({"dehaze":amount})).expect("a result"))
    };
    let run16 = |amount: i32| {
        distances16(
            &filter16(SCENE, SCENE, &clean16, &json!({"dehaze":amount}), &[]).expect("an edit"),
        )
    };
    for (depth, level, before, half, full) in [
        (8, 1_i64, distances8(&clean8), run8(-50), run8(-100)),
        (16, 257, distances16(&clean16), run16(-50), run16(-100)),
    ] {
        assert!(!before.is_empty());
        for (i, was) in before.iter().enumerate() {
            for (amount, now) in [(-50, half[i]), (-100, full[i])] {
                assert!(
                    now <= *was,
                    "{depth}-bit {amount} sample {i}: {now} from {was}"
                );
                if *was > 2 * level {
                    assert!(
                        now < *was,
                        "{depth}-bit {amount} sample {i}: {now} from {was}"
                    );
                }
            }
            assert!(
                full[i] <= half[i],
                "{depth}-bit sample {i}: -100 at {} is behind -50 at {}",
                full[i],
                half[i]
            );
        }
        let (half, full): (i64, i64) = (half.iter().sum(), full.iter().sum());
        assert!(
            full < half,
            "{depth}-bit: -100 at {full} is not past -50 at {half}"
        );
    }
}

// ---- Depths, coverage, refusals, NULL, progress and cancel ---------------------

#[test]
fn byte_and_16_bit_agree_within_a_level_and_coverage_confines_the_result() {
    let (w, h) = (36_u32, 28_u32);
    let src = plate(w, h);
    let wide = widen(&src);
    let inside = |x: u32, y: u32| (5..30).contains(&x) && (4..22).contains(&y);
    let coverage: Vec<u16> = (0..w * h)
        .map(|i| if inside(i % w, i / w) { 65535 } else { 0 })
        .collect();
    for params in [
        json!({"texture":60,"clarity":-40,"dehaze":30}),
        json!({"texture":-80,"dehaze":-50}),
        json!({"clarity":100}),
    ] {
        let out16 = filter16(w, h, &wide, &params, &coverage)
            .unwrap_or_else(|| panic!("{params}: no change at 16 bits"));
        let out8 = filter8(w, h, &src, &params).unwrap_or_else(|| panic!("{params}: a bypass"));
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                // Colour only: alpha is exact and an alpha-0 pixel untouched.
                assert_eq!(out8[i * 4 + 3], src[i * 4 + 3], "{params} alpha {x},{y}");
                if src[i * 4 + 3] == 0 {
                    assert_eq!(
                        out8[i * 4..i * 4 + 4],
                        src[i * 4..i * 4 + 4],
                        "{params} {x},{y}"
                    );
                }
                if !inside(x, y) {
                    assert_eq!(out16[i], wide[i], "{params} outside {x},{y}");
                    continue;
                }
                for c in 0..4 {
                    let got = f64::from(out16[i][c]) / 257.0;
                    assert!(
                        (got - f64::from(out8[i * 4 + c])).abs() <= 1.0,
                        "{params} {x},{y} {c}: {got} vs {}",
                        out8[i * 4 + c]
                    );
                }
            }
        }
    }
}

#[test]
fn parameters_refuse_by_name() {
    let (w, h) = (4_u32, 3_u32);
    let img = image(w, h, &[128; 48]);
    for (params, wanted) in [
        (
            json!({"texture":101}),
            "texture must be finite in [-100,100]",
        ),
        (
            json!({"clarity":-101}),
            "clarity must be finite in [-100,100]",
        ),
        (
            json!({"dehaze":100.5}),
            "dehaze must be finite in [-100,100]",
        ),
        (json!({"texture":"a"}), "texture must be numeric"),
        (json!({"dehaze":true}), "dehaze must be numeric"),
        (json!({"radius":3}), "unexpected presence parameter radius"),
    ] {
        let (out, message) = run(img, &params.to_string(), ptr::null(), 12);
        assert!(out.is_none(), "{params} was accepted");
        assert_eq!(message, wanted, "{params}");
    }
    unsafe { dr_image_free(img) };
}

#[test]
fn null_arguments_are_tolerated() {
    // A NULL image is NULL with no error; NULL params are the defaults, the
    // exact bypass, so NULL with no error too.
    let (out, message) = run(ptr::null(), r#"{"clarity":20}"#, ptr::null(), 0);
    assert!(out.is_none() && message.is_empty(), "{message}");
    let (w, h) = (20_u32, 20_u32);
    let img = image(w, h, &plate(w, h));
    let name = CString::new("presence").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, name.as_ptr(), ptr::null(), ptr::null(), &mut err) };
    assert!(out.is_null() && err.is_null());
    let bytes = unsafe {
        dr_image_filter_op_scratch_bytes(ptr::null(), name.as_ptr(), ptr::null(), ptr::null_mut())
    };
    assert_eq!(bytes, 0);
    unsafe { dr_image_free(img) };
}

#[test]
fn it_reports_progress_and_cancels_mid_run() {
    let (w, h) = (120_u32, 80_u32);
    let n = (w * h) as usize;
    let img = image(w, h, &plate(w, h)) as usize;
    for params in [
        r#"{"texture":50,"clarity":-30,"dehaze":40}"#,
        r#"{"clarity":60}"#,
        r#"{"dehaze":-50}"#,
    ] {
        let (plain, message) = run(img as *const DrImage, params, ptr::null(), n);
        assert!(message.is_empty(), "{params}: {message}");
        assert!(plain.is_some(), "{params}: a bypass");
        let control = unsafe { dr_job_control_create() };
        let (controlled, message) = run(img as *const DrImage, params, control, n);
        unsafe { dr_job_control_free(control) };
        assert!(message.is_empty(), "{params}: {message}");
        assert!(plain == controlled, "{params}: controlled bytes differ");

        let render = |token: usize| {
            let (out, message) = run(
                img as *const DrImage,
                params,
                token as *const DrJobControl,
                n,
            );
            (out.is_some(), message)
        };
        let log = reports(render);
        assert!(
            log.iter().all(|r| (0.0..=1.0).contains(&r.progress)),
            "{params}: a progress outside [0, 1]"
        );
        assert!(
            log.windows(2)
                .all(|pair| pair[0].progress <= pair[1].progress),
            "{params}: progress went backwards"
        );
        let last = log.last().expect("a report");
        assert!(
            last.progress == 1.0 && last.stage == 5,
            "{params}: the log ends at ({}, stage {})",
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
            "{params}: fewer than two stage-6 reports inside (0, 1)"
        );
        // `checkpoint` parks before `report` stores its stage, so the second
        // stage-6 report parks with stage 6 already stored by the first.
        let count = inside[1];
        let parked = cancelled_parked_at(count, &|_, stage| stage == 6, render);
        assert_eq!(parked.count, count, "{params}");
    }
    unsafe { dr_image_free(img as *mut DrImage) };
}

// ---- The plan -------------------------------------------------------------------

/// The 8-bit plan of a 6000×4000 image with all three sliders on, derived
/// from the stages the header documents. n = 24,000,000; the radii are fine 3,
/// mid 18, coarse 117 and haze 70 (6000/2048 times 1, 6, 40 and 24, rounded).
/// The widest guided pass is the coarse base: the n×16 correction plane, five
/// box rings of min(2·117 + 2, 4000) + 1 = 237 rows and a delay ring of 118
/// rows plus 2 regression rows, each row 6000 binary64 (62,640,000), four
/// bookkeeping MiB, and the n×4 delta it returns: 546,834,304. With it are the
/// three planes kept from the earlier passes (veil, fine, mid; 288,000,000)
/// and the arm's two bookkeeping MiB: 836,931,456, above the 20n combine
/// stage. `filter_op_scratch_bytes` adds four bookkeeping MiB and a quarter.
const PLAN_24MP: u64 = 1_051_407_200;
/// The 16-bit plan of the same edit, `selected_edit_plan`'s `scratch_bytes`:
/// six bookkeeping MiB, the n×8 output codes, the kernel's 836,931,456 (above
/// the n×16 plane held beside the n×8 copy) and the canvas's n×2 coverage.
/// `scripts/presence-admission/measure.py` holds both literals to the
/// measured footprint.
const PLAN_24MP_WIDE: u64 = 1_083_222_912;

#[test]
fn the_24_megapixel_plans_are_pinned() {
    let (w, h) = (6000_u32, 4000_u32);
    let bytes = vec![0_u8; w as usize * h as usize * 4];
    let img = image(w, h, &bytes);
    drop(bytes);
    let planned = plan(img, r#"{"texture":50,"clarity":50,"dehaze":50}"#);
    unsafe { dr_image_free(img) };
    let n = 24_000_000_u64;
    let mib = 1024 * 1024_u64;
    let coarse = 16 * n + (5 * 237 + 118 + 2) * 6000 * 8 + 4 * mib + 4 * n;
    let kernel = coarse + 3 * 4 * n + 2 * mib;
    let base = kernel + 4 * mib;
    assert_eq!(base + base.div_ceil(4), PLAN_24MP, "the derivation");
    assert_eq!(planned, PLAN_24MP);

    let edit = SelectedEdit::parse(
        &json!({"layer":0,"op":"presence","params":{"texture":50,"clarity":50,"dehaze":50}})
            .to_string(),
    )
    .unwrap();
    let wide = doc16(w, h, vec![[0_u16; 4]; (w * h) as usize])
        .selected_edit_plan(&edit)
        .unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap();
    assert_eq!(
        6 * mib + 8 * n + kernel + 2 * n,
        PLAN_24MP_WIDE,
        "the derivation"
    );
    assert_eq!(wide, PLAN_24MP_WIDE);
}
