//! `dr_raw_local_mask_resize`: a RAW local's u16 coverage carried to another frame size in
//! normalised coordinates (the RAW-settings sync's locals). Every mask is built here as
//! RZLM1 bytes and every resize goes through the FFI; the oracles are the closed-form
//! functions the masks were sampled from, evaluated at the target's own pixel centres.
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_raw_geometry::dr_raw_local_mask_resize;
use doctored_core::ffi_raw_local::{
    dr_raw_local_mask_bytes, dr_raw_local_mask_free, dr_raw_local_mask_from_bytes,
};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::raw_local_mask::RawLocalMask;
use std::ffi::{c_char, CStr};
use std::ptr;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// RZLM1: `RZLM`, version 1, width, height (u32 LE), then `width * height` u16 LE samples.
fn rzlm(width: u32, height: u32, sample: impl Fn(u32, u32) -> f64) -> Vec<u8> {
    let mut bytes = b"RZLM".to_vec();
    for word in [1, width, height] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for y in 0..height {
        for x in 0..width {
            let value = sample(x, y).clamp(0.0, 65535.0).round() as u16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

fn mask(bytes: &[u8]) -> *mut RawLocalMask {
    let mut error: *mut c_char = ptr::null_mut();
    let mask = unsafe {
        dr_raw_local_mask_from_bytes(
            bytes.as_ptr(),
            bytes.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert_eq!(take_error(error), None);
    assert!(!mask.is_null());
    mask
}

/// The resized samples, `Ok(None)` for the NULL-without-error answer, `Err` for an error.
fn resize(
    bytes: &[u8],
    width: u32,
    height: u32,
    control: *const DrJobControl,
) -> Result<Option<Vec<u16>>, String> {
    let source = mask(bytes);
    let mut error: *mut c_char = ptr::null_mut();
    let result =
        unsafe { dr_raw_local_mask_resize(source, width, height, LIMIT, 0, control, &mut error) };
    unsafe { dr_raw_local_mask_free(source) };
    if let Some(error) = take_error(error) {
        assert!(result.is_null());
        return Err(error);
    }
    if result.is_null() {
        return Ok(None);
    }
    let mut length = 0usize;
    let mut error: *mut c_char = ptr::null_mut();
    let data = unsafe { dr_raw_local_mask_bytes(result, &mut length, &mut error) };
    assert_eq!(take_error(error), None);
    let encoded = unsafe { std::slice::from_raw_parts(data, length) };
    assert_eq!(&encoded[..4], b"RZLM");
    assert_eq!(&encoded[8..12], &width.to_le_bytes());
    assert_eq!(&encoded[12..16], &height.to_le_bytes());
    assert_eq!(length, 16 + 2 * width as usize * height as usize);
    let samples = encoded[16..]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    unsafe { dr_raw_local_mask_free(result) };
    Ok(Some(samples))
}

fn resized(bytes: &[u8], width: u32, height: u32) -> Vec<u16> {
    resize(bytes, width, height, ptr::null())
        .expect("resize")
        .expect("a changed size answers a mask")
}

#[test]
fn full_coverage_stays_full_at_every_pixel() {
    let full = rzlm(301, 200, |_, _| 65535.0);
    for (w, h) in [(640, 427), (97, 64)] {
        let samples = resized(&full, w, h);
        assert!(
            samples.iter().all(|&s| s == 65535),
            "{w}x{h}: a full plane lost coverage"
        );
    }
}

/// The largest |resized − oracle| over target pixels at least `margin` from every border.
fn worst(samples: &[u16], w: u32, h: u32, margin: u32, oracle: impl Fn(u32, u32) -> f64) -> f64 {
    let mut worst = 0.0f64;
    for y in margin..h - margin {
        for x in margin..w - margin {
            let got = f64::from(samples[(y * w + x) as usize]);
            worst = worst.max((got - oracle(x, y)).abs());
        }
    }
    worst
}

#[test]
fn ramps_keep_their_normalised_position() {
    let (sw, sh, w, h) = (400u32, 300u32, 250u32, 350u32);
    let x_ramp = rzlm(sw, sh, |x, _| {
        65535.0 * (f64::from(x) + 0.5) / f64::from(sw)
    });
    let y_ramp = rzlm(sw, sh, |_, y| {
        65535.0 * (f64::from(y) + 0.5) / f64::from(sh)
    });
    let margin = (f64::from(sw) / f64::from(w))
        .max(f64::from(sh) / f64::from(h))
        .ceil() as u32
        + 1;
    // The y axis enlarges (300 → 350): two-tap linear interpolation reproduces a ramp, so the
    // only error is the source's and the output's rounding, at most 1 code.
    let y_worst = worst(&resized(&y_ramp, w, h), w, h, margin, |_, y| {
        65535.0 * (f64::from(y) + 0.5) / f64::from(h)
    });
    assert!(y_worst <= 1.0, "y-ramp off by {y_worst} codes");
    // The x axis reduces by 1.6. The widened triangle's taps at a non-integer support do not
    // have their centroid exactly at the target centre: computing the kernel's exact
    // edge-normalised weights for 400 → 250 gives a periodic shift of at most ~0.06 source
    // pixel, 10.57 codes of this 164-code-per-pixel ramp, so the 1-code bound of the enlarging
    // axis is unattainable with this kernel. The bound is that computed ripple plus the source's
    // and the output's rounding, rounded up to 16 codes (about 0.1 source pixel): a half-pixel
    // misregistration on the reducing axis (82 codes) still fails it.
    let x_worst = worst(&resized(&x_ramp, w, h), w, h, margin, |x, _| {
        65535.0 * (f64::from(x) + 0.5) / f64::from(w)
    });
    assert!(x_worst <= 16.0, "x-ramp off by {x_worst} codes");
}

/// An elliptical smoothstep in normalised coordinates: centre (0.3, 0.6), radii 0.2 of the
/// width and 0.25 of the height, full inside half the radius (hardness 0.5), smoothstep to 0
/// at the ellipse.
fn ellipse(u: f64, v: f64) -> f64 {
    let d = (((u - 0.3) / 0.2).powi(2) + ((v - 0.6) / 0.25).powi(2)).sqrt();
    let t = ((1.0 - d) / (1.0 - 0.5)).clamp(0.0, 1.0);
    65535.0 * t * t * (3.0 - 2.0 * t)
}

#[test]
fn a_smooth_mask_matches_its_formula_at_the_target_centres() {
    let (sw, sh) = (600u32, 400u32);
    let source = rzlm(sw, sh, |x, y| {
        ellipse(
            (f64::from(x) + 0.5) / f64::from(sw),
            (f64::from(y) + 0.5) / f64::from(sh),
        )
    });
    for (w, h) in [(300u32, 450u32), (900, 1000)] {
        let samples = resized(&source, w, h);
        let off = worst(&samples, w, h, 0, |x, y| {
            ellipse(
                (f64::from(x) + 0.5) / f64::from(w),
                (f64::from(y) + 0.5) / f64::from(h),
            )
        });
        assert!(off <= 328.0, "{w}x{h}: off by {off} codes");
    }
}

#[test]
fn the_same_size_answers_null_without_error() {
    let bytes = rzlm(31, 17, |x, y| f64::from(x * 1000 + y));
    assert_eq!(resize(&bytes, 31, 17, ptr::null()), Ok(None));
}

#[test]
fn bad_sizes_and_a_cancel_are_errors() {
    let bytes = rzlm(31, 17, |x, _| f64::from(x * 2000));
    for (w, h) in [(0, 17), (31, 0), (8001, 8000)] {
        let error = resize(&bytes, w, h, ptr::null()).expect_err("a bad size must be refused");
        assert!(!error.is_empty());
    }
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let answer = resize(&bytes, 62, 34, control);
    unsafe { dr_job_control_free(control) };
    assert_eq!(answer, Err("operation cancelled".to_string()));
}
