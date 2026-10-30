//! The 32-bit float (linear) Camera RAW development, through the FFI:
//! the float intake, the local blend, a range, the vignette, the source
//! geometry, the develop inspection and the master read.
//!
//! Oracles, all f64 and written here, none read off the implementation:
//!
//! - blend: `b + w · (v − b)` per channel on opaque samples, `w` the mask's
//!   own u16 code over 65535 times the density, within 2e-6;
//! - range: CIE L*a*b* (D50 = 0.96422, 1, 0.82521) from the profile's
//!   `rXYZ`/`gXYZ`/`bXYZ` tags applied to the LINEAR sample (no tone curve),
//!   L* clamped to 0…100. A sample at least 0.0025 l from the hard bound is
//!   exactly the base or exactly the variant;
//! - vignette: the sample times `2^stops`, `stops` the strength times
//!   `(distance / farthest corner centre)^falloff`, within 1e-5 relative;
//! - geometry: the eight EXIF orientations as index maps (bit for bit), and
//!   a constant plane, which any normalized kernel leaves constant (1e-5);
//! - inspection: the code the profile's own TRC tag encodes `clamp(v, 0, 1)`
//!   to (the tag's decode inverted by bisection), within one code.
//!
//! Bounds were fixed before the first run.
use crate::raw_local_numeric::{icc_be32, mask_edit, success, transfer_decode, transfer_tag, Mask};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_develop_inspect::{
    dr_precision_doc_source_master, dr_precision_image_develop_inspection,
};
use doctored_core::ffi_precision_input::{
    dr_precision_doc_from_rgba16, dr_precision_doc_from_rgba32,
};
use doctored_core::ffi_raw_geometry::dr_raw_source_geometry;
use doctored_core::ffi_raw_local::*;
use doctored_core::ffi_retained_source::dr_precision_doc_mixed_source_edit;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::raw_local_mask::RawLocalMask;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const D50: [f64; 3] = [0.96422, 1.0, 0.82521];
const HARD_LUMINANCE_MARGIN: f64 = 0.0025;

// ---- plumbing ---------------------------------------------------------------

fn taken(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

/// `dr_precision_doc_from_rgba32` over `pixels`.
fn intake(
    w: u32,
    h: u32,
    pixels: &[[f32; 4]],
    profile: &IccProfile,
) -> Result<Box<PrecisionDocument>, String> {
    assert_eq!(pixels.len(), w as usize * h as usize);
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_from_rgba32(
            pixels.as_ptr().cast(),
            pixels.len() * 4,
            w,
            h,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    match taken(error) {
        Some(message) => {
            assert!(output.is_null());
            Err(message)
        }
        None => {
            assert!(!output.is_null());
            Ok(unsafe { Box::from_raw(output) })
        }
    }
}

fn float_image(
    w: u32,
    h: u32,
    pixels: &[[f32; 4]],
    profile: &IccProfile,
) -> Box<PrecisionDocument> {
    intake(w, h, pixels, profile).unwrap()
}

fn floats(document: &PrecisionDocument) -> Vec<[f32; 4]> {
    document.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// `dr_precision_doc_develop_locals`: `Ok(None)` when nothing changed.
fn develop(
    base: &PrecisionDocument,
    variants: &[&PrecisionDocument],
    masks: &[&Mask],
    settings: Value,
    vignette: Option<Value>,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    assert_eq!(variants.len(), masks.len());
    let variants: Vec<_> = variants
        .iter()
        .map(|d| *d as *const PrecisionDocument)
        .collect();
    let masks: Vec<_> = masks.iter().map(|m| m.0 as *const RawLocalMask).collect();
    let settings = CString::new(settings.to_string()).unwrap();
    let vignette = vignette.map(|v| CString::new(v.to_string()).unwrap());
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_develop_locals(
            base,
            variants.as_ptr(),
            masks.as_ptr(),
            variants.len(),
            settings.as_ptr(),
            vignette.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    match taken(error) {
        Some(message) => Err(message),
        None => Ok((!output.is_null()).then(|| unsafe { Box::from_raw(output) })),
    }
}

/// An RZLM1 mask holding `values` exactly.
fn mask_from(w: u32, h: u32, values: &[u16]) -> Mask {
    assert_eq!(values.len(), w as usize * h as usize);
    let mut bytes = b"RZLM".to_vec();
    for value in [1, w, h] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut error = ptr::null_mut();
    let pointer = unsafe {
        dr_raw_local_mask_from_bytes(
            bytes.as_ptr(),
            bytes.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    assert!(!pointer.is_null());
    Mask(pointer)
}

/// `dr_raw_source_geometry`: `None` for an identity.
fn geometry(source: &PrecisionDocument, request: &Value) -> Option<Box<PrecisionDocument>> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let output =
        unsafe { dr_raw_source_geometry(source, text.as_ptr(), LIMIT, 0, ptr::null(), &mut error) };
    success(error);
    (!output.is_null()).then(|| unsafe { Box::from_raw(output) })
}

/// Samples with highlights above white, a component below zero and exact
/// zeros, all opaque: a scene-linear development's shape.
fn scene(n: usize) -> Vec<[f32; 4]> {
    (0..n)
        .map(|i| {
            let t = i as f32;
            match i % 4 {
                0 => [3.75 - t * 0.03125, 1.5 + t * 0.0625, 0.25, 1.0],
                1 => [-0.25, 0.5 + t * 0.015625, 2.0, 1.0],
                2 => [0.0, 0.0, 0.0, 1.0],
                _ => [0.125 * t, 0.75, 1.0 + t * 0.25, 1.0],
            }
        })
        .collect()
}

// ---- intake -----------------------------------------------------------------

#[test]
fn the_float_intake_keeps_extended_samples_exactly_and_refuses_what_it_cannot_hold() {
    let profile = IccProfile::display_p3();
    let pixels = vec![
        [4.0, -0.25, 0.0, 1.0],
        [65504.0, -65504.0, 1.0, 0.5],
        [0.5, 0.25, 0.125, 0.0],
        [1.0e-6, 3.5, 0.75, 1.0],
    ];
    let document = float_image(2, 2, &pixels, &profile);
    assert!(document.is_float());
    assert_eq!(document.float_refusal(), None);
    assert_eq!(document.dimensions(), (2, 2));
    assert_eq!(document.profile().bytes(), profile.bytes());
    let image = document.layer_source(0).unwrap();
    assert_eq!(image.storage(), PrecisionStorage::Rgba32Float);
    assert_eq!(bits(&floats(&document)), bits(&pixels));

    for (label, bad) in [
        ("NaN", [f32::NAN, 0.0, 0.0, 1.0]),
        ("infinity", [0.0, f32::INFINITY, 0.0, 1.0]),
        ("negative infinity", [0.0, 0.0, f32::NEG_INFINITY, 1.0]),
        ("70000", [70000.0, 0.0, 0.0, 1.0]),
        ("-70000", [0.0, -70000.0, 0.0, 1.0]),
        ("alpha 1.5", [0.5, 0.5, 0.5, 1.5]),
        ("alpha -0.5", [0.5, 0.5, 0.5, -0.5]),
        ("NaN alpha", [0.5, 0.5, 0.5, f32::NAN]),
    ] {
        let mut samples = pixels.clone();
        samples[3] = bad;
        let refusal = intake(2, 2, &samples, &profile).err();
        assert!(refusal.is_some_and(|m| !m.is_empty()), "{label} is refused");
    }
    // A count that is not width × height × 4, and a profile that is not ICC.
    let mut error = ptr::null_mut();
    let short = unsafe {
        dr_precision_doc_from_rgba32(
            pixels.as_ptr().cast(),
            pixels.len() * 4 - 1,
            2,
            2,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(short.is_null() && taken(error).is_some());
    let junk = [7_u8; 64];
    let mut error = ptr::null_mut();
    let untagged = unsafe {
        dr_precision_doc_from_rgba32(
            pixels.as_ptr().cast(),
            pixels.len() * 4,
            2,
            2,
            junk.as_ptr(),
            junk.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(untagged.is_null() && taken(error).is_some());
    // The working limit is consulted before anything is copied.
    let mut error = ptr::null_mut();
    let denied = unsafe {
        dr_precision_doc_from_rgba32(
            pixels.as_ptr().cast(),
            pixels.len() * 4,
            2,
            2,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            1,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(denied.is_null() && taken(error).is_some());
}

#[test]
fn an_f32_image_wraps_as_a_float_document() {
    let profile = IccProfile::srgb();
    let pixels = scene(12);
    let source = float_image(4, 3, &pixels, &profile);
    let mut error = ptr::null_mut();
    let wrapped = unsafe {
        dr_precision_doc_from_image(
            source.layer_source(0).unwrap(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    assert!(!wrapped.is_null());
    let wrapped = unsafe { Box::from_raw(wrapped) };
    assert!(wrapped.is_float());
    assert_eq!(wrapped.float_refusal(), None);
    assert_eq!(bits(&floats(&wrapped)), bits(&pixels));
    assert_eq!(wrapped.profile().bytes(), profile.bytes());
}

// ---- the local blend --------------------------------------------------------

#[test]
fn the_float_blend_matches_the_f64_mixture_and_keeps_extended_values() {
    let profile = IccProfile::srgb();
    let (w, h) = (4, 3);
    let base = scene(12);
    let variant: Vec<[f32; 4]> = base
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let t = i as f32;
            [
                p[0] * 0.5 - 0.125,
                p[1] + 1.75 - t * 0.0625,
                p[2] * 2.0 + 0.0625,
                1.0,
            ]
        })
        .collect();
    let codes = [
        0, 1, 327, 16000, 32768, 65534, 65535, 1701, 56789, 128, 4096, 60000,
    ];
    let density = 0.73125;
    let mask = mask_from(w, h, &codes);
    let base_doc = float_image(w, h, &base, &profile);
    let variant_doc = float_image(w, h, &variant, &profile);
    let out = develop(
        &base_doc,
        &[&variant_doc],
        &[&mask],
        json!([{"enabled":true,"density":density,"feather":0}]),
        None,
    )
    .unwrap()
    .expect("the blend changes the base");
    assert!(out.is_float());
    assert_eq!(out.float_refusal(), None);
    assert_eq!(out.profile().bytes(), profile.bytes());
    let actual = floats(&out);
    let (mut above, mut below) = (0, 0);
    for i in 0..base.len() {
        let weight = f64::from(codes[i]) / 65535.0 * density;
        for c in 0..3 {
            let (b, v) = (f64::from(base[i][c]), f64::from(variant[i][c]));
            let expected = b + weight * (v - b);
            let error = (f64::from(actual[i][c]) - expected).abs();
            assert!(
                error <= 2e-6,
                "pixel {i} channel {c}: {} expected {expected}",
                actual[i][c]
            );
            above += usize::from(actual[i][c] > 1.0);
            below += usize::from(actual[i][c] < 0.0);
        }
        assert_eq!(actual[i][3], 1.0, "pixel {i}: alpha");
    }
    // Nothing is clamped: the result still holds values on both sides of 0…1.
    assert!(above >= 6 && below >= 1, "{above} above, {below} below");
    // A pixel the mask excludes keeps its base sample bit for bit.
    assert_eq!(actual[0].map(f32::to_bits), base[0].map(f32::to_bits));
    // The inputs are untouched.
    assert_eq!(bits(&floats(&base_doc)), bits(&base));
    assert_eq!(bits(&floats(&variant_doc)), bits(&variant));
}

#[test]
fn a_float_blend_that_changes_nothing_is_absent_and_a_u16_variant_is_refused() {
    let profile = IccProfile::srgb();
    let (w, h) = (4, 3);
    let base = scene(12);
    let base_doc = float_image(w, h, &base, &profile);
    let twin = float_image(w, h, &base, &profile);
    let full = mask_edit(None, w, h, json!({"op":"full"})).unwrap();
    let on = json!([{"enabled":true,"density":1,"feather":0}]);
    // Equal variants, a disabled local and zero density change nothing.
    assert!(develop(&base_doc, &[&twin], &[&full], on.clone(), None)
        .unwrap()
        .is_none());
    let other: Vec<[f32; 4]> = base.iter().map(|p| [p[0] + 1.0, p[1], p[2], 1.0]).collect();
    let other_doc = float_image(w, h, &other, &profile);
    for settings in [
        json!([{"enabled":false,"density":1,"feather":0}]),
        json!([{"enabled":true,"density":0,"feather":0}]),
    ] {
        assert!(develop(&base_doc, &[&other_doc], &[&full], settings, None)
            .unwrap()
            .is_none());
    }
    // A full-weight local answers the variant exactly.
    let replaced = develop(&base_doc, &[&other_doc], &[&full], on.clone(), None)
        .unwrap()
        .unwrap();
    assert_eq!(bits(&floats(&replaced)), bits(&other));
    // A u16 variant beside a float base is refused, as is the reverse.
    let codes = [[1000_u16, 2000, 3000, 65535]; 12];
    let mut error = ptr::null_mut();
    let sixteen = unsafe {
        dr_precision_doc_from_rgba16(
            codes.as_ptr().cast(),
            codes.len() * 4,
            w,
            h,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    let sixteen = unsafe { Box::from_raw(sixteen) };
    assert_eq!(
        develop(&base_doc, &[&sixteen], &[&full], on.clone(), None).err(),
        Some("local variant requires genuine RGBA32 float samples beside a float base".into())
    );
    assert_eq!(
        develop(&sixteen, &[&base_doc], &[&full], on, None).err(),
        Some("local variant requires genuine RGBA16 samples".into())
    );
}

// ---- the range ----------------------------------------------------------------

fn xyz_tag(bytes: &[u8], signature: &[u8; 4]) -> [f64; 3] {
    for n in 0..icc_be32(bytes, 128) as usize {
        let record = 132 + n * 12;
        if &bytes[record..record + 4] == signature {
            let start = icc_be32(bytes, record + 4) as usize;
            assert_eq!(&bytes[start..start + 4], b"XYZ ");
            return std::array::from_fn(|k| {
                f64::from(icc_be32(bytes, start + 8 + k * 4) as i32) / 65536.0
            });
        }
    }
    panic!("the profile lacks a colorant");
}

/// Lab of a LINEAR sample in the profile's primaries; `[0]` is L* before the
/// clamp, so a caller can tell a sample above white from one near it.
fn oracle_lab(profile: &IccProfile, rgb: [f32; 3]) -> [f64; 3] {
    let bytes = profile.bytes();
    let mut xyz = [0.0; 3];
    for (c, signature) in [b"rXYZ", b"gXYZ", b"bXYZ"].into_iter().enumerate() {
        let column = xyz_tag(bytes, signature);
        for k in 0..3 {
            xyz[k] += column[k] * f64::from(rgb[c]);
        }
    }
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(xyz[0] / D50[0]), f(xyz[1] / D50[1]), f(xyz[2] / D50[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// A neutral ramp of linear values from 0 to 4 (quadratic, so the darks hold
/// samples too), then a coloured one whose blue is below zero at the dark
/// end.
fn linear_ramp(w: u32) -> Vec<[f32; 4]> {
    let v = |i: u32| {
        let t = i as f32 / (w - 1) as f32;
        4.0 * t * t
    };
    (0..w)
        .map(|i| [v(i), v(i), v(i), 1.0])
        .chain((0..w).map(|i| [v(i) * 0.5, 0.05 + v(i) * 0.125, v(i) * 0.25 - 0.01, 1.0]))
        .collect()
}

#[test]
fn a_hard_luminance_range_reads_lab_of_the_linear_base() {
    let w = 256;
    let base = linear_ramp(w);
    let variant: Vec<[f32; 4]> = base.iter().map(|p| [p[0] + 1.5, p[1], p[2], 1.0]).collect();
    let full = mask_edit(None, w, 2, json!({"op":"full"})).unwrap();
    let (low, high) = (0.3, 1.0);
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let base_doc = float_image(w, 2, &base, &profile);
        let variant_doc = float_image(w, 2, &variant, &profile);
        let out = develop(
            &base_doc,
            &[&variant_doc],
            &[&full],
            json!([{"enabled":true,"density":1,"feather":0,"range":{"luminance":
                {"low":low,"high":high,"low_falloff":0,"high_falloff":0,"invert":false}}}]),
            None,
        )
        .unwrap()
        .expect("the range admits some of the frame");
        let actual = floats(&out);
        let (mut inside, mut outside, mut over) = (0, 0, 0);
        for (i, sample) in base.iter().enumerate() {
            let lightness = oracle_lab(&profile, [sample[0], sample[1], sample[2]])[0] / 100.0;
            if (lightness - low).abs() < HARD_LUMINANCE_MARGIN {
                continue;
            }
            // L* is clamped to 100, so a sample above white has l = 1 and is
            // inside `high: 1`.
            let expected = if lightness < low { base[i] } else { variant[i] };
            assert_eq!(
                actual[i].map(f32::to_bits),
                expected.map(f32::to_bits),
                "pixel {i}: l {lightness}"
            );
            if lightness < low {
                outside += 1;
            } else {
                inside += 1;
                over += usize::from(lightness > 1.0 + HARD_LUMINANCE_MARGIN);
            }
        }
        assert!(inside >= 100 && outside >= 20 && over >= 100);
    }
}

#[test]
fn a_range_sample_of_a_float_base_is_the_mean_lab_of_its_linear_window() {
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let (w, h) = (9, 7);
        let pixels: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let t = i as f32;
                [
                    0.02 + t * 0.011,
                    0.9 - t * 0.012,
                    0.05 + (t * 0.37) % 0.6,
                    1.0,
                ]
            })
            .collect();
        let base = float_image(w, h, &pixels, &profile);
        for (x, y) in [(4.5, 3.5), (0.0, 0.0), (9.0, 7.0), (1.2, 5.9)] {
            let mut lab = [0.0_f64; 3];
            let mut error = ptr::null_mut();
            let ok = unsafe {
                dr_precision_doc_raw_range_sample(&*base, x, y, lab.as_mut_ptr(), &mut error)
            };
            success(error);
            assert!(ok);
            let (cx, cy) = ((x as u32).min(w - 1), (y as u32).min(h - 1));
            let mut sum = [0.0; 3];
            let mut count = 0.0;
            for py in cy.saturating_sub(2)..=(cy + 2).min(h - 1) {
                for px in cx.saturating_sub(2)..=(cx + 2).min(w - 1) {
                    let p = pixels[(py * w + px) as usize];
                    let mut one = oracle_lab(&profile, [p[0], p[1], p[2]]);
                    one[0] = one[0].clamp(0.0, 100.0);
                    for k in 0..3 {
                        sum[k] += one[k];
                    }
                    count += 1.0;
                }
            }
            for k in 0..3 {
                let expected = sum[k] / count;
                assert!(
                    (lab[k] - expected).abs() <= 2e-3,
                    "({x}, {y}) component {k}: {} expected {expected}",
                    lab[k]
                );
            }
        }
    }
    // A window of samples above white reads L* 100.
    let bright = vec![[3.0_f32, 3.0, 3.0, 1.0]; 25];
    let base = float_image(5, 5, &bright, &IccProfile::srgb());
    let mut lab = [0.0_f64; 3];
    let mut error = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_raw_range_sample(&*base, 2.5, 2.5, lab.as_mut_ptr(), &mut error)
    });
    success(error);
    assert_eq!(lab[0], 100.0);
}

// ---- the vignette -------------------------------------------------------------

fn vignette_stops(p: [f64; 2], size: [u32; 2], center: [f64; 2], stops: f64, falloff: f64) -> f64 {
    let distance = |q: [f64; 2]| ((q[0] - center[0]).powi(2) + (q[1] - center[1]).powi(2)).sqrt();
    let mut farthest: f64 = 0.0;
    for x in [0.5, f64::from(size[0]) - 0.5] {
        for y in [0.5, f64::from(size[1]) - 0.5] {
            farthest = farthest.max(distance([x, y]));
        }
    }
    if stops == 0.0 || distance(p) == 0.0 {
        0.0
    } else {
        stops * (distance(p) / farthest).powf(falloff)
    }
}

#[test]
fn the_float_vignette_is_a_linear_gain() {
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        for (w, h, center) in [(9_u32, 5_u32, [4.5, 2.5]), (4, 11, [0.0, 11.0])] {
            let pixels = scene((w * h) as usize);
            let base = float_image(w, h, &pixels, &profile);
            for stops in [-4.0, -0.3125, 0.73125, 4.0] {
                for falloff in [0.25, 1.0, 4.0] {
                    let out = develop(
                        &base,
                        &[],
                        &[],
                        json!([]),
                        Some(json!({"stops":stops,"center":center,"falloff":falloff})),
                    )
                    .unwrap()
                    .expect("the vignette changes the frame");
                    assert!(out.is_float());
                    let actual = floats(&out);
                    for (i, pixel) in pixels.iter().enumerate() {
                        let gain = vignette_stops(
                            [f64::from(i as u32 % w) + 0.5, f64::from(i as u32 / w) + 0.5],
                            [w, h],
                            center,
                            stops,
                            falloff,
                        )
                        .exp2();
                        for c in 0..3 {
                            let expected = f64::from(pixel[c]) * gain;
                            let error = (f64::from(actual[i][c]) - expected).abs();
                            assert!(
                                error <= 1e-5 * expected.abs(),
                                "pixel {i} channel {c}: {} expected {expected}",
                                actual[i][c]
                            );
                        }
                        assert_eq!(actual[i][3], pixel[3]);
                    }
                    // The gain is not clamped at white.
                    if stops > 0.0 {
                        assert!(actual.iter().flatten().any(|v| *v > 4.0));
                    }
                }
            }
        }
    }
    // A neutral vignette changes nothing.
    let base = float_image(3, 4, &scene(12), &IccProfile::srgb());
    assert!(develop(
        &base,
        &[],
        &[],
        json!([]),
        Some(json!({"stops":0.0,"center":[1.5,2.0],"falloff":1.0}))
    )
    .unwrap()
    .is_none());
}

// ---- geometry -----------------------------------------------------------------

fn expected_exif(o: u32, w: u32, h: u32, x: u32, y: u32) -> (u32, u32) {
    match o {
        1 => (x, y),
        2 => (w - 1 - x, y),
        3 => (w - 1 - x, h - 1 - y),
        4 => (x, h - 1 - y),
        5 => (y, x),
        6 => (y, h - 1 - x),
        7 => (w - 1 - y, h - 1 - x),
        8 => (w - 1 - y, x),
        _ => unreachable!(),
    }
}

#[test]
fn an_exif_map_of_a_float_source_is_an_exact_permutation() {
    for (w, h) in [(7_u32, 5_u32), (5, 7), (1, 7)] {
        // Distinct samples, some above white, some below zero, one hidden
        // under zero alpha.
        let pixels: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let t = i as f32;
                [
                    t * 0.173 - 0.5,
                    4.0 - t * 0.091,
                    (t * 1.37) % 2.5,
                    if i == 3 { 0.0 } else { 1.0 - (t % 4.0) * 0.125 },
                ]
            })
            .collect();
        let source = float_image(w, h, &pixels, &IccProfile::display_p3());
        for orientation in 1..=8 {
            let request = json!({"version":1,"kind":"exif","orientation":orientation});
            let Some(mapped) = geometry(&source, &request) else {
                assert_eq!(orientation, 1, "only the identity is absent");
                continue;
            };
            assert_ne!(orientation, 1);
            assert!(mapped.is_float());
            assert_eq!(mapped.float_refusal(), None);
            assert_eq!(mapped.profile().bytes(), source.profile().bytes());
            let (dw, dh) = mapped.dimensions();
            assert_eq!((dw, dh), if orientation >= 5 { (h, w) } else { (w, h) });
            let output = floats(&mapped);
            for y in 0..dh {
                for x in 0..dw {
                    let (sx, sy) = expected_exif(orientation, w, h, x, y);
                    assert_eq!(
                        output[(y * dw + x) as usize].map(f32::to_bits),
                        pixels[(sy * w + sx) as usize].map(f32::to_bits),
                        "orientation {orientation} at ({x}, {y})"
                    );
                }
            }
        }
    }
}

#[test]
fn a_resampled_constant_float_plane_stays_constant_above_white() {
    let (w, h) = (24_u32, 18_u32);
    let pixels = vec![[3.5_f32, 3.5, 3.5, 1.0]; (w * h) as usize];
    let source = float_image(w, h, &pixels, &IccProfile::srgb());
    for filter in ["bilinear", "bicubic"] {
        let request = json!({"version":1,"kind":"affine","width":20,"height":16,
            "inverse":[0.94,0.21,-0.21,0.94,2.3,1.7],"filter":filter});
        let mapped = geometry(&source, &request).expect("the rotation changes the frame");
        assert!(mapped.is_float());
        assert_eq!(mapped.dimensions(), (20, 16));
        let output = floats(&mapped);
        let mut opaque = 0;
        for (i, p) in output.iter().enumerate() {
            assert!((0.0..=1.0).contains(&p[3]), "pixel {i}: alpha {}", p[3]);
            if p[3] == 0.0 {
                continue;
            }
            for (c, value) in p[..3].iter().enumerate() {
                assert!(
                    (f64::from(*value) - 3.5).abs() <= 1e-5,
                    "{filter} pixel {i} channel {c}: {value}"
                );
            }
            opaque += usize::from(p[3] >= 0.999);
        }
        assert!(opaque >= 100, "{filter}: {opaque} interior pixels");
    }
}

// ---- inspection and the master --------------------------------------------------

/// The code the profile's own TRC tag encodes `linear` to: the tag's decode
/// (`transfer_decode`, table or parametric) inverted by bisection, clamped
/// to 0…1 first. The built-in sRGB profile's tag is a sampled table, so in
/// the toe it differs from the analytic sRGB curve by about one percent.
fn encoded_code(tag: &[u8], linear: f64) -> f64 {
    let target = linear.clamp(0.0, 1.0);
    if target <= transfer_decode(tag, 0.0) {
        return 0.0;
    }
    if target >= transfer_decode(tag, 1.0) {
        return 65535.0;
    }
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..64 {
        let middle = (low + high) * 0.5;
        if transfer_decode(tag, middle) < target {
            low = middle;
        } else {
            high = middle;
        }
    }
    ((low + high) * 0.5 * 65535.0).round()
}

fn inspection(image: &PrecisionImage) -> Box<PrecisionImage> {
    let mut error = ptr::null_mut();
    let output =
        unsafe { dr_precision_image_develop_inspection(image, LIMIT, 0, ptr::null(), &mut error) };
    success(error);
    assert!(!output.is_null());
    unsafe { Box::from_raw(output) }
}

#[test]
fn the_develop_inspection_of_a_float_image_is_its_sdr_clipped_encoding() {
    let profile = IccProfile::srgb();
    let (w, h) = (16_u32, 8_u32);
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let t = i as f32 / (w * h - 1) as f32;
            [
                t * 2.5 - 0.25,
                t * t,
                1.0 - t * 0.999,
                if i % 5 == 0 { 0.5 } else { 1.0 },
            ]
        })
        .collect();
    let document = float_image(w, h, &pixels, &profile);
    let view = inspection(document.layer_source(0).unwrap());
    assert_eq!(view.storage(), PrecisionStorage::Rgba16Unorm);
    assert_eq!(view.dimensions(), (w, h));
    assert_eq!(view.profile().bytes(), profile.bytes());
    let codes = view.to_rgba16().unwrap();
    let (mut white, mut black) = (0, 0);
    for (i, (code, pixel)) in codes.iter().zip(&pixels).enumerate() {
        for c in 0..3 {
            let expected = encoded_code(transfer_tag(profile.bytes(), c), f64::from(pixel[c]));
            assert!(
                (f64::from(code[c]) - expected).abs() <= 1.0,
                "pixel {i} channel {c}: {} expected {expected}",
                code[c]
            );
            // At or above white reads 65535, at or below zero reads 0.
            if pixel[c] >= 1.0 {
                assert_eq!(code[c], 65535);
                white += 1;
            }
            if pixel[c] <= 0.0 {
                assert_eq!(code[c], 0);
                black += 1;
            }
        }
        let alpha = (65535.0 * f64::from(pixel[3])).round();
        assert!(
            (f64::from(code[3]) - alpha).abs() <= 1.0,
            "pixel {i}: alpha"
        );
    }
    assert!(white >= 20 && black >= 5);
    // The working limit applies to the f32 case.
    let mut error = ptr::null_mut();
    let denied = unsafe {
        dr_precision_image_develop_inspection(
            document.layer_source(0).unwrap(),
            1,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(denied.is_null() && taken(error).is_some());

    // A u16 image is answered as it is.
    let sixteen: Vec<[u16; 4]> = (0..w * h)
        .map(|i| [(i * 511) as u16, (65535 - i * 300) as u16, 32768, 65535])
        .collect();
    let mut error = ptr::null_mut();
    let typed = unsafe {
        dr_precision_doc_from_rgba16(
            sixteen.as_ptr().cast(),
            sixteen.len() * 4,
            w,
            h,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    let typed = unsafe { Box::from_raw(typed) };
    let same = inspection(typed.layer_source(0).unwrap());
    assert_eq!(same.storage(), PrecisionStorage::Rgba16Unorm);
    assert_eq!(same.to_rgba16().unwrap(), sixteen);
}

/// `dr_precision_doc_mixed_source_edit` placing `master` as a retained
/// record above `document`'s one layer.
fn placed(document: &PrecisionDocument, master: &PrecisionDocument) -> Box<PrecisionDocument> {
    let request = CString::new(
        json!({"op":"place","source_id":"camera","name":"camera","x":1,"y":2,
            "depth_conversion":"reject"})
        .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_mixed_source_edit(
            document,
            ptr::null(),
            master,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    assert!(!output.is_null());
    unsafe { Box::from_raw(output) }
}

#[test]
fn the_source_master_of_a_float_development_is_its_f32_master() {
    let profile = IccProfile::srgb();
    let canvas = float_image(12, 10, &vec![[0.25, 0.5, 0.75, 1.0]; 120], &profile);
    let pixels = scene(20);
    let master = float_image(5, 4, &pixels, &profile);
    let document = placed(&canvas, &master);
    let mut error = ptr::null_mut();
    let image = unsafe { dr_precision_doc_source_master(&*document, 1, &mut error) };
    success(error);
    assert!(!image.is_null());
    let image = unsafe { Box::from_raw(image) };
    assert_eq!(image.storage(), PrecisionStorage::Rgba32Float);
    assert_eq!(image.dimensions(), (5, 4));
    assert_eq!(bits(&image.to_rgba32().unwrap()), bits(&pixels));

    // A u16 master in the same document is answered as u16.
    let codes: Vec<[u16; 4]> = (0..20)
        .map(|i| [i * 3001, 40000, 65535 - i * 7, 65535])
        .collect();
    let mut error = ptr::null_mut();
    let typed = unsafe {
        dr_precision_doc_from_rgba16(
            codes.as_ptr().cast(),
            codes.len() * 4,
            5,
            4,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    let typed = unsafe { Box::from_raw(typed) };
    let document = placed(&canvas, &typed);
    let mut error = ptr::null_mut();
    let image = unsafe { dr_precision_doc_source_master(&*document, 1, &mut error) };
    success(error);
    let image = unsafe { Box::from_raw(image) };
    assert_eq!(image.storage(), PrecisionStorage::Rgba16Unorm);
    assert_eq!(image.to_rgba16().unwrap(), codes);

    // A plain layer and an index past the stack refuse.
    for layer in [0, 9] {
        let mut error = ptr::null_mut();
        let none = unsafe { dr_precision_doc_source_master(&*document, layer, &mut error) };
        assert!(none.is_null());
        assert_eq!(
            taken(error).as_deref(),
            Some("layer has no valid retained u16 or f32 master")
        );
    }
}
