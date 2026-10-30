//! What a filter does to the dark surround of a highlight far
//! above white in a 32-bit float layer.
//!
//! A kernel that approximates its definition has an error relative to the
//! BRIGHTEST sample it reads. A 0…1 clamp and a 16-bit code hide that; a float
//! layer does not: a ripple of one part in ten thousand of a 5000 highlight is
//! five times the 0.1 background it lands on. So every kernel the float mode
//! runs is held here to one bound, declared before anything was measured:
//!
//! on a 0.1 background holding one 5000 pixel, and separately a 5000 disc of
//! radius 6, at EVERY pixel `|out − ref| ≤ 2 % of ref` (`ref` is never below
//! 0.1), `ref` being the filter's own definition evaluated in f64 here.
//!
//! The Gaussian's reference is the sampled Gaussian out to ±ceil(8σ), where a
//! weight is below e^-32. Measured against it on this fixture (2026-10-12),
//! the recursive Gaussian that 8 and 16 bits run from σ4 up missed the bound
//! on the disc at σ 8 and σ 32, so a float layer runs the FIR at every σ; the
//! figures are on `sampling_gaussian::runs_fast`. Gaussian Blur
//! from radius 7 up is the extended-box kernel at every depth, held here to its
//! own definition (Unsharp Mask and High Pass keep the FIR). Each of the other means is
//! its own definition summed directly (no cascade, no downsampled pass), so
//! what this file measures on them is f32 storage and, for Motion and Radial
//! Blur, where each bilinear sample is placed: f32 positions missed the bound
//! thousands of pixels from the origin, so a float layer places them in
//! binary64 (`sampling_radial::sample_exact` has those figures).
use serde_json::{json, Value};

use crate::extended_box_reference::FITTED_MIN_RADIUS;
use crate::float_filters::{
    box_reference, disc_reference, filter, fitted_reference, gaussian_reference, motion_reference,
    radial_reference, samples, solid_doc, straight_of, tilt_reference, Picture,
};

const BACKGROUND: f32 = 0.1;
const HIGHLIGHT: f32 = 5000.0;
/// The bound, as a fraction of the reference.
const BOUND: f64 = 0.02;
const W: u32 = 200;
const H: u32 = 168;
/// The highlight's centre: off the centre of the plane, in the second
/// `BAND_ROWS` (128) band, so a banded kernel's seams are inside the fixture.
const CENTRE: (i64, i64) = (93, 131);

/// A `w × h` background holding the highlight `dx² + dy² ≤ radius²` about
/// `centre`: one pixel at radius 0.
fn fixture(w: u32, h: u32, centre: (i64, i64), radius: i64) -> Picture {
    let pixels = (0..w * h)
        .map(|i| {
            let (dx, dy) = (i64::from(i % w) - centre.0, i64::from(i / w) - centre.1);
            let v = if dx * dx + dy * dy <= radius * radius {
                HIGHLIGHT
            } else {
                BACKGROUND
            };
            [v, v, v, 1.0]
        })
        .collect();
    Picture { w, h, pixels }
}

/// The two fixtures: one pixel, and the disc of radius 6.
fn fixtures() -> [(&'static str, Picture); 2] {
    [
        ("pixel", fixture(W, H, CENTRE, 0)),
        ("disc", fixture(W, H, CENTRE, 6)),
    ]
}

/// One measured case: its name, the worst `|out − ref| / ref` over every
/// pixel and colour channel, where that is, the lowest and highest output
/// colour, and the lowest output alpha.
type Case = (String, f64, (u32, u32), f32, f32, f32);

/// `op` on `picture` against `reference`.
fn worst(
    name: String,
    picture: &Picture,
    op: &str,
    params: &Value,
    reference: &[[f64; 4]],
) -> Case {
    let doc = solid_doc(picture);
    let out = samples(
        &filter(&doc, op, params, &[])
            .unwrap_or_else(|e| panic!("{op} {params}: {e}"))
            .unwrap_or_else(|| panic!("{op} {params}: no change")),
    );
    let mut worst = (0.0_f64, 0_usize);
    let (mut lowest, mut highest) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut alpha = f32::INFINITY;
    for (i, (got, want)) in out.iter().zip(reference).enumerate() {
        assert!(got[3] <= 1.0, "{op} {params} pixel {i}: alpha {}", got[3]);
        alpha = alpha.min(got[3]);
        let want = straight_of(*want);
        for c in 0..3 {
            assert!(
                want[c] >= f64::from(BACKGROUND) * 0.999,
                "{op}: ref {want:?}"
            );
            lowest = lowest.min(got[c]);
            highest = highest.max(got[c]);
            let error = (f64::from(got[c]) - want[c]).abs() / want[c];
            if error > worst.0 {
                worst = (error, i);
            }
        }
    }
    let at = (worst.1 as u32 % picture.w, worst.1 as u32 / picture.w);
    (name, worst.0, at, lowest, highest, alpha)
}

/// Prints each case (the measurement record), then fails on any above the
/// bound, on any output outside the picture's own values (a mean never leaves
/// them), and on any alpha that is not exactly 1: the picture is opaque, and
/// a float layer stores an alpha one ulp short of it.
fn held(cases: Vec<Case>) {
    let mut missed = Vec::new();
    for (name, error, (x, y), lowest, highest, alpha) in cases {
        eprintln!(
            "HIGHLIGHT {name}: worst |out-ref|/ref = {:.6} % at ({x}, {y}); output {lowest} … {highest}; lowest alpha {alpha}",
            error * 100.0
        );
        if error > BOUND || lowest < BACKGROUND * 0.9999 || highest > HIGHLIGHT || alpha != 1.0 {
            missed.push(name);
        }
    }
    assert!(
        missed.is_empty(),
        "above 2 % of the reference, outside the picture's range, or not opaque: {missed:?}"
    );
}

#[test]
fn a_gaussian_keeps_the_dark_surround_of_a_highlight_within_two_percent() {
    let mut cases = Vec::new();
    for (name, picture) in fixtures() {
        // σ 2 is below the other depths' switch to the recursive kernel, 8 and
        // 32 above it, and 64 is the largest σ the filter takes. A
        // radius from 7 up runs Gaussian Blur's four extended boxes
        // (`gaussian_blur_filter`), so its reference is that kernel's own
        // definition (`fitted_reference`), held to the same bound.
        for sigma in [2.0, 8.0, 32.0, 64.0] {
            let reference = if sigma >= FITTED_MIN_RADIUS {
                fitted_reference(&picture, sigma)
            } else {
                gaussian_reference(&picture, sigma)
            };
            let case = format!("blur σ{sigma} {name}");
            cases.push(worst(
                case,
                &picture,
                "blur",
                &json!({"sigma": sigma}),
                &reference,
            ));
        }
    }
    held(cases);
    // σ 128 is outside the filter at every depth: 64 is the largest it takes.
    let (_, picture) = &fixtures()[0];
    let refusal = filter(&solid_doc(picture), "blur", &json!({"sigma": 128}), &[])
        .err()
        .unwrap();
    assert!(
        refusal.starts_with("sigma must be finite in [") && refusal.ends_with(",64]"),
        "{refusal}"
    );
}

#[test]
fn the_other_means_keep_the_dark_surround_of_a_highlight_within_two_percent() {
    let mut cases = Vec::new();
    for (name, picture) in fixtures() {
        for radius in [3_u32, 40] {
            let reference = box_reference(&picture, radius);
            let case = format!("box_blur r{radius} {name}");
            let params = json!({"radius": radius});
            cases.push(worst(case, &picture, "box_blur", &params, &reference));
        }
        for (angle, distance) in [(0.0, 10_u32), (30.0, 10), (-71.0, 37), (90.0, 120)] {
            let reference = motion_reference(&picture, angle, distance);
            let params = json!({"angle": angle, "distance": distance});
            let case = format!("motion_blur {angle}° d{distance} {name}");
            cases.push(worst(case, &picture, "motion_blur", &params, &reference));
        }
        for (method, quality, amount) in [
            ("spin", "good", 20_u32),
            ("spin", "best", 20),
            ("zoom", "good", 40),
            ("zoom", "best", 100),
        ] {
            let reference = radial_reference(&picture, method, quality, amount, (0.5, 0.5), None);
            let params = json!({"method": method, "quality": quality, "amount": amount});
            let case = format!("radial_blur {method} {quality} a{amount} {name}");
            cases.push(worst(case, &picture, "radial_blur", &params, &reference));
        }
        for (radius, brightness, threshold) in
            [(4_u32, 0.0, 255.0), (15, 0.0, 255.0), (9, 60.0, 128.0)]
        {
            let highlight = (brightness > 0.0).then_some((brightness, threshold));
            let reference = disc_reference(&picture, radius, highlight);
            let params =
                json!({"radius": radius, "brightness": brightness, "threshold": threshold});
            let case = format!("lens_blur r{radius} b{brightness} {name}");
            cases.push(worst(case, &picture, "lens_blur", &params, &reference));
        }
        let tilt = (9_u32, (0.45, 0.6), 20.0, 0.1, 0.3);
        let reference = tilt_reference(&picture, tilt);
        let params = json!({"radius": tilt.0, "center_x": tilt.1 .0, "center_y": tilt.1 .1,
            "angle": tilt.2, "focus": tilt.3, "transition": tilt.4});
        let case = format!("tilt_shift {name}");
        cases.push(worst(case, &picture, "tilt_shift", &params, &reference));
    }
    held(cases);
}

#[test]
fn a_sampled_path_holds_the_bound_thousands_of_pixels_from_the_origin() {
    // Motion and Radial Blur place each bilinear sample at a position whose
    // resolution falls with its distance from the origin, and the tent turns
    // a position error into a weight on the highlight. So the same bound is
    // held where a 24 MP picture's far edge is, not only beside the origin.
    let (w, h, centre) = (4200_u32, 48_u32, (4150_i64, 30_i64));
    let mut cases = Vec::new();
    for (name, radius) in [("pixel", 0), ("disc", 6)] {
        let picture = fixture(w, h, centre, radius);
        for (angle, distance) in [(30.0, 10_u32), (-71.0, 37)] {
            let reference = motion_reference(&picture, angle, distance);
            let params = json!({"angle": angle, "distance": distance});
            let case = format!("far motion_blur {angle}° d{distance} {name}");
            cases.push(worst(case, &picture, "motion_blur", &params, &reference));
        }
        // 2050 px from the centre: a 36 px arc at Amount 1, a 41 px ray at 4.
        for (method, quality, amount) in [("spin", "good", 1_u32), ("zoom", "best", 4)] {
            let reference = radial_reference(&picture, method, quality, amount, (0.5, 0.5), None);
            let params = json!({"method": method, "quality": quality, "amount": amount});
            let case = format!("far radial_blur {method} {quality} a{amount} {name}");
            cases.push(worst(case, &picture, "radial_blur", &params, &reference));
        }
    }
    held(cases);
}
