//! A Camera RAW local over a Lensfun-corrected development (v1-lensfun), through
//! the FFI: the app corrects the base and each camera variant before the
//! locals mix (`RawLocalEvaluation`), so a local's coverage is in the corrected
//! frame, as with a decoder lens profile.
//!
//! Oracles, written here and not read off the implementation:
//!
//! - coverage: `radial_reference`'s pixel-centre law (normalised elliptical
//!   distance, smoothstep falloff from `hardness` to 1). A pixel whose centre
//!   is beyond 1.02 has no coverage, so the mix leaves the corrected base
//!   there bit for bit (16 bits: code for code), transparent edge pixels of
//!   the pincushion correction included; one nearer than 0.78 has full
//!   coverage, so it is the corrected variant (2e-6 in float, one code at 16
//!   bits);
//! - linearity: the correction is a weighted sum of samples, so correcting a
//!   variant that is exactly half the base gives half the corrected base
//!   (1e-6 relative, float only);
//! - the defect: correcting after the mix (the old order) samples coverage
//!   from the uncorrected frame, where this pincushion profile reads inward
//!   at radii below the calibration unit, so some pixel beyond 1.02 moves.
//!
//! The profile is the one camera.NEF (NIKON D70s, Tamron AF 18-250mm A18 at
//! 35 mm) resolves to: ptlens {0, 0.03755, 0}, the 35 mm TCA poly3 row, crop
//! 1.525 on a 1.53 calibration, aspect 1.5. Bounds were fixed before the first
//! run.
use crate::raw_local_numeric::{develop, image, mask_edit, radial_reference, rgba, success, Mask};
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba32;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};
use std::ffi::CString;
use std::ptr;

const W: u32 = 192;
const H: u32 = 128;
const CENTER: [f64; 2] = [96.0, 64.0];
const RADIUS: [f64; 2] = [38.0, 24.0];
const HARDNESS: f64 = 0.8;
const OUTSIDE: f64 = 1.02;
const INSIDE: f64 = 0.78;

fn camera_profile() -> Value {
    json!({
        "id": "Tamron AF 18-250mm f/3.5-6.3 Di II LD Aspherical (IF) Macro",
        "crop_factor": 1.525, "calibration_crop_factor": 1.53,
        "aspect_ratio": 1.5, "center": [0.0, 0.0], "focal_length": 35.0,
        "f_number": 8.0, "subject_distance": null,
        "distortion": {"model": "ptlens", "a": 0.0, "b": 0.03755, "c": 0.0},
        "tca": {"model": "poly3", "vr": 1.000062, "cr": 0.0002671, "br": -0.0001337,
            "vb": 1.0001761, "cb": 0.0004179, "bb": -0.0000737},
        "vignetting": null,
    })
}

/// `dr_precision_doc_from_rgba32` over `pixels`.
fn float_image(pixels: &[[f32; 4]], profile: &IccProfile) -> Box<PrecisionDocument> {
    assert_eq!(pixels.len(), (W * H) as usize);
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_from_rgba32(
            pixels.as_ptr().cast(),
            pixels.len() * 4,
            W,
            H,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    assert!(!output.is_null());
    unsafe { Box::from_raw(output) }
}

fn floats(document: &PrecisionDocument) -> Vec<[f32; 4]> {
    document.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// The Lensfun stage: `lens_correction` with the camera profile through
/// `dr_precision_doc_selected_edit`, as `applyingRawLensfun` runs it.
fn corrected(document: &PrecisionDocument) -> Box<PrecisionDocument> {
    let request = CString::new(
        json!({"layer": 0, "op": "lens_correction", "params": {"profile": camera_profile()}})
            .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_selected_edit(
            document,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    assert!(!output.is_null(), "the profile corrects the frame");
    unsafe { Box::from_raw(output) }
}

fn radial_mask() -> Mask {
    mask_edit(
        None,
        W,
        H,
        json!({"op":"radial","center":CENTER,"radius":RADIUS,"hardness":HARDNESS}),
    )
    .unwrap()
}

/// The normalised elliptical distance of pixel `i`'s centre.
fn distance(i: usize) -> f64 {
    let p = [(i % W as usize) as f64 + 0.5, (i / W as usize) as f64 + 0.5];
    let x = (p[0] - CENTER[0]) / RADIUS[0];
    let y = (p[1] - CENTER[1]) / RADIUS[1];
    let d = (x * x + y * y).sqrt();
    // The oracle's coverage law agrees on both sides of the bands used here.
    if d > OUTSIDE {
        assert_eq!(radial_reference(p, CENTER, RADIUS, HARDNESS), 0.0);
    }
    if d < INSIDE {
        assert_eq!(radial_reference(p, CENTER, RADIUS, HARDNESS), 1.0);
    }
    d
}

/// An opaque ramp in [0.05, 0.95], different in each channel.
fn ramp_value(i: usize) -> [f64; 3] {
    let x = (i % W as usize) as f64 / f64::from(W - 1);
    let y = (i / W as usize) as f64 / f64::from(H - 1);
    [0.05 + 0.9 * x, 0.05 + 0.9 * y, 0.05 + 0.45 * (x + y)]
}

fn settings() -> Value {
    json!([{"enabled": true, "density": 1, "feather": 0}])
}

#[test]
fn a_local_over_lens_corrected_float_developments_moves_nothing_outside_its_coverage() {
    let profile = IccProfile::srgb();
    let base: Vec<[f32; 4]> = (0..(W * H) as usize)
        .map(|i| {
            let v = ramp_value(i);
            [v[0] as f32, v[1] as f32, v[2] as f32, 1.0]
        })
        .collect();
    let variant: Vec<[f32; 4]> = base
        .iter()
        .map(|p| [p[0] * 0.5, p[1] * 0.5, p[2] * 0.5, 1.0])
        .collect();
    let b = float_image(&base, &profile);
    let v = float_image(&variant, &profile);
    let lb = corrected(&b);
    let lv = corrected(&v);
    let mask = radial_mask();
    let out =
        develop(&lb, &[&lv], &[&mask], settings(), None).expect("the local changes the frame");
    assert!(out.is_float());
    let (actual, lb_px, lv_px) = (floats(&out), floats(&lb), floats(&lv));
    let (actual_bits, lb_bits) = (bits(&actual), bits(&lb_px));
    let (mut outside, mut transparent, mut inside) = (0, 0, 0);
    for i in 0..(W * H) as usize {
        let d = distance(i);
        if d > OUTSIDE {
            assert_eq!(
                actual_bits[i], lb_bits[i],
                "pixel {i} at {d}: outside moved"
            );
            outside += 1;
            transparent += usize::from(lb_px[i][3] == 0.0);
        } else if d < INSIDE && lb_px[i][3] == 1.0 {
            for c in 0..3 {
                let (got, want) = (f64::from(actual[i][c]), f64::from(lv_px[i][c]));
                assert!(
                    (got - want).abs() <= 2e-6,
                    "pixel {i} channel {c}: {got} expected {want}"
                );
                let half = 0.5 * f64::from(lb_px[i][c]);
                assert!(
                    (want - half).abs() <= 1e-6 * half.abs(),
                    "pixel {i} channel {c}: L(V) {want} expected {half}"
                );
            }
            inside += 1;
        }
    }
    assert!(
        outside > 10_000 && transparent > 100 && inside > 1_000,
        "{outside} outside, {transparent} transparent, {inside} inside"
    );

    // The old order, the local mixed over the uncorrected base and the mix
    // corrected after, moves pixels the corrected frame leaves outside.
    let mixed =
        develop(&b, &[&v], &[&mask], settings(), None).expect("the local changes the frame");
    let old = bits(&floats(&corrected(&mixed)));
    let moved = (0..(W * H) as usize)
        .filter(|&i| distance(i) > OUTSIDE && old[i] != lb_bits[i])
        .count();
    assert!(moved >= 1, "the old order moved no outside pixel");
}

/// IEC 61966-2-1, independent of the profile's own tag.
fn srgb_decode(e: f64) -> f64 {
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_encode(l: f64) -> f64 {
    if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

#[test]
fn a_local_over_lens_corrected_16_bit_developments_moves_nothing_outside_its_coverage() {
    let profile = IccProfile::srgb();
    let base: Vec<[u16; 4]> = (0..(W * H) as usize)
        .map(|i| {
            let v = ramp_value(i);
            [code(v[0]), code(v[1]), code(v[2]), 65535]
        })
        .collect();
    let variant: Vec<[u16; 4]> = base
        .iter()
        .map(|p| {
            let half = |c: u16| code(srgb_encode(srgb_decode(f64::from(c) / 65535.0) * 0.5));
            [half(p[0]), half(p[1]), half(p[2]), 65535]
        })
        .collect();
    let b = image(W, H, &base, &profile);
    let v = image(W, H, &variant, &profile);
    let lb = corrected(&b);
    let lv = corrected(&v);
    let mask = radial_mask();
    let out =
        develop(&lb, &[&lv], &[&mask], settings(), None).expect("the local changes the frame");
    let (actual, lb_px, lv_px) = (rgba(&out), rgba(&lb), rgba(&lv));
    let (mut outside, mut transparent, mut inside) = (0, 0, 0);
    for i in 0..(W * H) as usize {
        let d = distance(i);
        if d > OUTSIDE {
            assert_eq!(actual[i], lb_px[i], "pixel {i} at {d}: outside moved");
            outside += 1;
            transparent += usize::from(lb_px[i][3] == 0);
        } else if d < INSIDE && lb_px[i][3] == 65535 {
            for c in 0..3 {
                assert!(
                    actual[i][c].abs_diff(lv_px[i][c]) <= 1,
                    "pixel {i} channel {c}: {} expected {}",
                    actual[i][c],
                    lv_px[i][c]
                );
            }
            inside += 1;
        }
    }
    assert!(
        outside > 10_000 && transparent > 100 && inside > 1_000,
        "{outside} outside, {transparent} transparent, {inside} inside"
    );

    let mixed =
        develop(&b, &[&v], &[&mask], settings(), None).expect("the local changes the frame");
    let old = rgba(&corrected(&mixed));
    let moved = (0..(W * H) as usize)
        .filter(|&i| distance(i) > OUTSIDE && old[i] != lb_px[i])
        .count();
    assert!(moved >= 1, "the old order moved no outside pixel");
}
