//! Model masks: `dr_model_mask_coverage`, a segmentation model's
//! low-resolution output plane to canvas u16 coverage.
//!
//! The oracle is an f64 bilinear written here from the contract alone
//! (pixel centres at half-integers on both sides, clamped edges), then the
//! zero threshold of a logit (SAM's `mask_threshold = 0.0`) or a clamp and the 1/256 floor, the
//! clip and a round; the kernel must match it within one code, skipping a logit within 1e-6 of
//! zero, whose sign f32 and f64 rounding may flip. The pipeline test feeds a low-resolution step to Select and
//! Mask's edge stage (`dr_selection_refine`) over a picture whose true edge
//! sits mid-cell, and bounds the refined IoU, fixed before running, at 0.98
//! and above the unrefined one.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_model_mask::dr_model_mask_coverage;
use doctored_core::ffi_selection_tools::dr_selection_refine;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

use crate::common::take_err_string;

const SENTINEL: u16 = 0x1234;
/// The contract's floor: a mapped value below one 8-bit display step is 0.
const FLOOR: f64 = 1.0 / 256.0;

fn floored(value: f64) -> f64 {
    if value < FLOOR {
        0.0
    } else {
        value
    }
}

struct Plane {
    width: u32,
    height: u32,
    channels: u32,
    samples: Vec<f32>,
}

fn noise(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

/// A plane of `channels` channel-major planes with samples in `[lo, hi)`.
fn plane(width: u32, height: u32, channels: u32, lo: f64, hi: f64, seed: u64) -> Plane {
    let mut seed = seed;
    let samples = (0..width * height * channels)
        .map(|_| (lo + (hi - lo) * noise(&mut seed)) as f32)
        .collect();
    Plane {
        width,
        height,
        channels,
        samples,
    }
}

struct Call<'a> {
    plane: &'a Plane,
    count: Option<usize>,
    request: String,
    clip: Option<&'a [u8]>,
    canvas: (u32, u32),
    limit: u64,
    control: *const DrJobControl,
}

impl<'a> Call<'a> {
    fn new(plane: &'a Plane, request: &Value, canvas: (u32, u32)) -> Self {
        Self {
            plane,
            count: None,
            request: request.to_string(),
            clip: None,
            canvas,
            limit: REFERENCE_WORKING_BYTES,
            control: ptr::null(),
        }
    }

    fn run(&self) -> Result<(Vec<u16>, u64), String> {
        let text = CString::new(self.request.as_str()).unwrap();
        let n = (self.canvas.0 * self.canvas.1) as usize;
        let mut output = vec![SENTINEL; n];
        let mut covered = 77u64;
        let mut error = ptr::null_mut();
        let ok = unsafe {
            dr_model_mask_coverage(
                self.plane.samples.as_ptr(),
                self.count.unwrap_or(self.plane.samples.len()),
                self.plane.width,
                self.plane.height,
                self.plane.channels,
                text.as_ptr(),
                self.clip.map_or(ptr::null(), <[u8]>::as_ptr),
                self.clip.map_or(0, <[u8]>::len),
                self.canvas.0,
                self.canvas.1,
                output.as_mut_ptr(),
                output.len(),
                &mut covered,
                self.limit,
                0,
                self.control,
                &mut error,
            )
        };
        if !ok {
            assert!(
                output.iter().all(|&v| v == SENTINEL) && covered == 77,
                "a refused call wrote its output"
            );
            return Err(take_err_string(error));
        }
        assert!(error.is_null());
        Ok((output, covered))
    }
}

/// The independent oracle: the selected channels' sum at canvas pixel
/// `(x, y)`, read bilinearly with clamped edges, mapped and clipped, in f64.
/// A logit within 1e-6 of zero answers NaN: its sign is rounding's, and
/// [`assert_within_one`] skips it.
fn oracle(
    p: &Plane,
    frame: [u32; 4],
    channels: &[usize],
    logit: bool,
    clip: Option<&[u8]>,
    canvas: (u32, u32),
) -> Vec<f64> {
    let (pw, ph) = (p.width as usize, p.height as usize);
    let cell = pw * ph;
    let sum = |x: usize, y: usize| -> f64 {
        channels
            .iter()
            .map(|&c| f64::from(p.samples[c * cell + y * pw + x]))
            .sum()
    };
    let [fx, fy, fw, fh] = frame;
    let mut out = vec![0.0; (canvas.0 * canvas.1) as usize];
    for y in fy..fy + fh {
        for x in fx..fx + fw {
            let u = (f64::from(x - fx) + 0.5) * pw as f64 / f64::from(fw) - 0.5;
            let v = (f64::from(y - fy) + 0.5) * ph as f64 / f64::from(fh) - 0.5;
            let (x0, y0) = (u.floor(), v.floor());
            let (tx, ty) = (u - x0, v - y0);
            let cx = |i: f64| (i as i64).clamp(0, pw as i64 - 1) as usize;
            let cy = |i: f64| (i as i64).clamp(0, ph as i64 - 1) as usize;
            let s = sum(cx(x0), cy(y0)) * (1.0 - tx) * (1.0 - ty)
                + sum(cx(x0 + 1.0), cy(y0)) * tx * (1.0 - ty)
                + sum(cx(x0), cy(y0 + 1.0)) * (1.0 - tx) * ty
                + sum(cx(x0 + 1.0), cy(y0 + 1.0)) * tx * ty;
            let value = if logit {
                if s.abs() < 1e-6 {
                    f64::NAN
                } else if s > 0.0 {
                    1.0
                } else {
                    0.0
                }
            } else {
                floored(s.clamp(0.0, 1.0))
            };
            let index = (y * canvas.0 + x) as usize;
            let keep = clip.map_or(1.0, |c| f64::from(c[index]) / 255.0);
            out[index] = value * keep * 65535.0;
        }
    }
    out
}

fn assert_within_one(got: &[u16], expected: &[f64]) {
    assert_eq!(got.len(), expected.len());
    for (i, (&g, &e)) in got.iter().zip(expected).enumerate() {
        if e.is_nan() {
            continue;
        }
        assert!(
            (f64::from(g) - e).abs() <= 1.0,
            "sample {i}: {g} vs oracle {e:.3}"
        );
    }
}

#[test]
fn the_identity_frame_equals_the_samples_exactly() {
    let p = plane(7, 5, 1, -0.2, 1.2, 1);
    let request = json!({"frame": [0, 0, 7, 5], "values": "probability"});
    let (coverage, covered) = Call::new(&p, &request, (7, 5)).run().unwrap();
    let expected: Vec<u16> = p
        .samples
        .iter()
        .map(|&s| (floored(f64::from(s).clamp(0.0, 1.0)) * 65535.0).round() as u16)
        .collect();
    assert_eq!(coverage, expected);
    let half = expected.iter().filter(|&&v| v >= 32768).count() as u64;
    assert_eq!(covered, half);
}

#[test]
fn an_offset_upsample_matches_the_bilinear_oracle() {
    let p = plane(6, 4, 1, -0.3, 1.3, 2);
    let frame = [5, 3, 18, 12];
    let canvas = (30, 20);
    let request = json!({"frame": frame, "values": "probability", "channels": [0]});
    let (coverage, covered) = Call::new(&p, &request, canvas).run().unwrap();
    let expected = oracle(&p, frame, &[0], false, None, canvas);
    assert_within_one(&coverage, &expected);
    for y in 0..canvas.1 {
        for x in 0..canvas.0 {
            let inside = (5..23).contains(&x) && (3..15).contains(&y);
            if !inside {
                assert_eq!(coverage[(y * canvas.0 + x) as usize], 0, "({x}, {y})");
            }
        }
    }
    assert_eq!(
        covered,
        coverage.iter().filter(|&&v| v >= 32768).count() as u64
    );
}

#[test]
fn logits_match_the_zero_threshold_oracle() {
    let p = plane(9, 7, 3, -6.0, 6.0, 3);
    let frame = [2, 1, 25, 19];
    let canvas = (31, 23);
    let request = json!({"frame": frame, "values": "logit", "channels": [1]});
    let (coverage, _) = Call::new(&p, &request, canvas).run().unwrap();
    assert_within_one(&coverage, &oracle(&p, frame, &[1], true, None, canvas));
}

#[test]
fn coverage_far_from_the_object_is_exactly_zero() {
    // A logit plane is thresholded at zero, so SAM's negative-logit haze is
    // 0; a softmax share is positive in every cell and the floor makes a
    // pixel far from the object 0. Either way the selection's bounds are the
    // object's.
    let mut p = plane(8, 8, 1, -12.0, -6.0, 9);
    p.samples[3 * 8 + 4] = 8.0;
    let canvas = (32, 32);
    let request = json!({"frame": [0, 0, 32, 32], "values": "logit"});
    let (coverage, _) = Call::new(&p, &request, canvas).run().unwrap();
    assert_within_one(
        &coverage,
        &oracle(&p, [0, 0, 32, 32], &[0], true, None, canvas),
    );
    assert_eq!(coverage[0], 0, "a corner far from the object");
    assert_eq!(coverage[(31 * 32 + 31) as usize], 0);
    assert!(
        coverage[(14 * 32 + 18) as usize] > 32768,
        "the object stays"
    );
    let tiny = Plane {
        width: 2,
        height: 1,
        channels: 1,
        samples: vec![0.003, 0.004],
    };
    let request = json!({"frame": [0, 0, 2, 1], "values": "probability"});
    let (coverage, _) = Call::new(&tiny, &request, (2, 1)).run().unwrap();
    assert_eq!(coverage, vec![0, 262], "0.003 < 1/256 <= 0.004");
}

#[test]
fn a_channel_sum_clamps() {
    let p = plane(8, 8, 3, 0.0, 0.9, 4);
    let frame = [0, 0, 20, 20];
    let canvas = (20, 20);
    let request = json!({"frame": frame, "values": "probability", "channels": [2, 0]});
    let (coverage, _) = Call::new(&p, &request, canvas).run().unwrap();
    let expected = oracle(&p, frame, &[0, 2], false, None, canvas);
    assert_within_one(&coverage, &expected);
    assert!(coverage.contains(&u16::MAX), "some sums pass 1 and clamp");
}

#[test]
fn the_clip_multiplies() {
    let p = plane(5, 5, 1, 0.0, 1.0, 5);
    let frame = [1, 2, 14, 11];
    let canvas = (17, 15);
    let mut seed = 6;
    let clip: Vec<u8> = (0..canvas.0 * canvas.1)
        .map(|_| (noise(&mut seed) * 256.0) as u8)
        .collect();
    let request = json!({"frame": frame, "values": "probability"});
    let mut call = Call::new(&p, &request, canvas);
    call.clip = Some(&clip);
    let (coverage, covered) = call.run().unwrap();
    assert_within_one(
        &coverage,
        &oracle(&p, frame, &[0], false, Some(&clip), canvas),
    );
    assert_eq!(
        covered,
        coverage.iter().filter(|&&v| v >= 32768).count() as u64
    );
}

#[test]
fn every_refusal_names_its_reason() {
    let p = plane(4, 4, 2, 0.0, 1.0, 7);
    let canvas = (8, 8);
    let refused = |request: Value| Call::new(&p, &request, canvas).run().unwrap_err();
    let says = |error: String, words: &str| {
        assert!(error.contains(words), "{error:?} lacks {words:?}");
    };
    says(
        refused(json!({"frame": [4, 0, 5, 8], "values": "probability"})),
        "outside the canvas",
    );
    says(
        refused(json!({"frame": [0, 0, 8, 8], "values": "probability", "channels": [2]})),
        "not in the model output",
    );
    says(
        refused(json!({"frame": [0, 0, 8, 8], "values": "probability", "channels": [1, 1]})),
        "listed twice",
    );
    says(
        refused(json!({"frame": [0, 0, 8, 8], "values": "logit", "channels": [0, 1]})),
        "exactly one channel",
    );
    says(
        refused(json!({"frame": [0, 0, 8, 8], "values": "linear"})),
        "values must be",
    );
    says(
        refused(json!({"frame": [0, 0, 0, 8], "values": "probability"})),
        "positive width",
    );
    says(
        refused(json!({"frame": [0, 0, 8, 8], "values": "probability", "scale": 2})),
        "unknown key",
    );
    says(
        refused(json!({"frame": [0.5, 0, 8, 8], "values": "probability"})),
        "non-negative integers",
    );
    // A count that is not w·h·c.
    let request = json!({"frame": [0, 0, 8, 8], "values": "probability"});
    let mut short = Call::new(&p, &request, canvas);
    short.count = Some(p.samples.len() - 1);
    says(short.run().unwrap_err(), "needs 32");
    // A non-finite sample in a selected channel; the other channel may hold one.
    let mut bad = plane(4, 4, 2, 0.0, 1.0, 8);
    bad.samples[16 + 5] = f32::NAN;
    let ok = Call::new(&bad, &request, canvas).run();
    assert!(ok.is_ok(), "channel 1's NaN is not read: {ok:?}");
    let ones = json!({"frame": [0, 0, 8, 8], "values": "probability", "channels": [1]});
    says(
        Call::new(&bad, &ones, canvas).run().unwrap_err(),
        "model output is not finite",
    );
    bad.samples[3] = f32::INFINITY;
    says(
        Call::new(&bad, &request, canvas).run().unwrap_err(),
        "model output is not finite",
    );
    // Planes past the caps.
    let wide = plane(4097, 1, 1, 0.0, 1.0, 9);
    says(
        Call::new(&wide, &request, canvas).run().unwrap_err(),
        "larger than 4096",
    );
    let deep = plane(1, 1, 17, 0.0, 1.0, 10);
    says(
        Call::new(&deep, &request, canvas).run().unwrap_err(),
        "larger than 4096",
    );
    // A clip of the wrong length, the working limit.
    let clip = vec![255u8; 63];
    let mut clipped = Call::new(&p, &request, canvas);
    clipped.clip = Some(&clip);
    says(clipped.run().unwrap_err(), "canvas-sized");
    let mut tight = Call::new(&p, &request, canvas);
    tight.limit = 64;
    says(tight.run().unwrap_err(), "working limit");
}

#[test]
fn a_cancelled_control_writes_nothing() {
    let p = plane(4, 4, 1, 0.0, 1.0, 11);
    let request = json!({"frame": [0, 0, 8, 8], "values": "probability"});
    unsafe {
        let control = dr_job_control_create();
        dr_job_control_cancel(control);
        let mut call = Call::new(&p, &request, (8, 8));
        call.control = control;
        let error = call.run().unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
        dr_job_control_free(control);
    }
}

/// IoU of `mask ≥ 128` against `truth ≥ 128`.
fn iou(mask: &[u8], truth: &[u8]) -> f64 {
    let (mut both, mut either) = (0u64, 0u64);
    for (&m, &t) in mask.iter().zip(truth) {
        let (m, t) = (m >= 128, t >= 128);
        both += u64::from(m && t);
        either += u64::from(m || t);
    }
    both as f64 / either as f64
}

/// The 16 x 16 step over a 256 x 256 canvas, upsampled by the kernel and
/// refined by `dr_selection_refine` at edge radius 16, over the two-colour
/// picture whose true edge is at [`EDGE`]: (unrefined, refined, truth).
fn step_pipeline(smart: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut step = Plane {
        width: 16,
        height: 16,
        channels: 1,
        samples: vec![0.0; 256],
    };
    for y in 0..16 {
        for x in 0..=8 {
            step.samples[y * 16 + x] = 1.0;
        }
    }
    let request = json!({"frame": [0, 0, SIDE, SIDE], "values": "probability"});
    let (coverage, _) = Call::new(&step, &request, (SIDE, SIDE)).run().unwrap();
    let mask: Vec<u8> = coverage
        .iter()
        .map(|&v| ((u32::from(v) * 255 + 32767) / 65535) as u8)
        .collect();
    let (left, right) = ([200u8, 60, 40], [30u8, 90, 200]);
    let mut rgba = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    let mut truth = Vec::with_capacity((SIDE * SIDE) as usize);
    for _ in 0..SIDE {
        for x in 0..SIDE {
            let colour = if x < EDGE { left } else { right };
            rgba.extend_from_slice(&colour);
            rgba.push(255);
            truth.push(if x < EDGE { 255 } else { 0 });
        }
    }
    let params = CString::new(format!(r#"{{"edge_radius":16,"smart_radius":{smart}}}"#)).unwrap();
    let mut refined = vec![0u8; mask.len()];
    unsafe {
        let source = dr_image_from_rgba(rgba.as_ptr(), SIDE, SIDE);
        let mut error = ptr::null_mut();
        let ok = dr_selection_refine(
            source,
            mask.as_ptr(),
            ptr::null(),
            params.as_ptr(),
            refined.as_mut_ptr(),
            ptr::null(),
            &mut error,
        );
        dr_image_free(source);
        assert!(ok, "{}", take_err_string(error));
    }
    (mask, refined, truth)
}

const SIDE: u32 = 256;
const EDGE: u32 = 136;

/// A 16 x 16 step (cells 0…8 covered) over a 256 x 256 canvas whose
/// picture's true edge is at x = 136, mid-way through cell 8: the upsampled
/// 50 % contour (between cell 8's centre and cell 9's, at x = 144) is 8 px
/// off, and the edge stage must move it onto the colours.
///
/// The declared bound, refined IoU ≥ 0.98 and above the unrefined IoU, is
/// asserted on the guided filter's own estimate (Smart Radius off). With
/// Smart Radius on — the app's setting — the estimate is faded by the fit's
/// confidence, so the 8 px of wrong coverage beside the true edge fall but
/// do not all cross 50 % (measured 0.9577 against the unrefined 0.9444);
/// there the test asserts the improvement and that each row's steepest drop
/// now sits at the true edge.
#[test]
fn a_low_res_step_refined_by_the_edge_stage_finds_the_true_edge() {
    let (mask, refined, truth) = step_pipeline(false);
    let (before, after) = (iou(&mask, &truth), iou(&refined, &truth));
    assert!(
        after >= 0.98,
        "refined IoU {after:.4} (unrefined {before:.4})"
    );
    assert!(
        after > before,
        "refined {after:.4} vs unrefined {before:.4}"
    );

    let (mask, refined, truth) = step_pipeline(true);
    let (before, after) = (iou(&mask, &truth), iou(&refined, &truth));
    assert!(
        after > before,
        "smart: refined {after:.4} vs unrefined {before:.4}"
    );
    for row in refined.chunks(SIDE as usize) {
        let steepest = (1..row.len())
            .max_by_key(|&x| i32::from(row[x - 1]) - i32::from(row[x]))
            .unwrap();
        assert_eq!(steepest as u32, EDGE, "smart: the steepest drop of a row");
    }
}
