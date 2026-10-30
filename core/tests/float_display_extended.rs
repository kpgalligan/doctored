//! v1.x EDR: the extended-range view of a float projection,
//! `dr_precision_doc_display_extended` — `display_encoding`'s rule X.
//!
//! The oracle is the rule written out here: `clamp(v · 2^stops, ±65504) · α`
//! per RGB sample and α itself, each the NEAREST binary16 (ties to even),
//! with a binary16 decoder of the test's own. The SDR view is not touched
//! (`float_view.rs` stays as it was).

use std::ffi::c_char;
use std::ptr;

use crate::common::take_err_string;
use crate::float_fixture::float_doc;
use doctored_core::ffi_float::dr_precision_doc_display_extended;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// IEEE binary16 bits as a number (normal, subnormal, ±0, ±∞, NaN).
fn half(bits: u16) -> f64 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let fraction = f64::from(bits & 0x3ff);
    sign * match exponent {
        0 => fraction * 2f64.powi(-24),
        31 if fraction == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        e => (1.0 + fraction / 1024.0) * 2f64.powi(e - 15),
    }
}

/// True when `bits` is the binary16 nearest `value` (ties to an even
/// significand): every other finite half is farther, or as far and odd.
fn is_nearest(bits: u16, value: f64) -> bool {
    let got = half(bits);
    let distance = (got - value).abs();
    for neighbour in [bits.wrapping_sub(1), bits.wrapping_add(1)] {
        let other = half(neighbour);
        if !other.is_finite() || (neighbour & 0x7c00) == 0x7c00 {
            continue;
        }
        if (neighbour ^ bits) & 0x8000 != 0 && value != 0.0 {
            continue;
        }
        let d = (other - value).abs();
        if d < distance || (d == distance && bits & 1 == 1) {
            return false;
        }
    }
    true
}

fn extended(
    document: &PrecisionDocument,
    image: &PrecisionImage,
    stops: f64,
    length: usize,
) -> Result<(Vec<u16>, f32), String> {
    let mut out = vec![0u8; length];
    let mut peak = f32::NAN;
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_display_extended(
            document,
            image,
            stops,
            out.as_mut_ptr(),
            out.len(),
            &mut peak,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !ok {
        return Err(take_err_string(err));
    }
    let halves = out
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    Ok((halves, peak))
}

fn scene() -> Vec<[f32; 4]> {
    vec![
        [0.25, 1.0, 4.0, 1.0],
        [8.0, 0.1, -0.02, 0.5],
        [65504.0, 1000.0, 0.0, 1.0],
        [3.0, 3.0, 3.0, 0.0],
        [1.0 / 3.0, 2.0 / 3.0, 0.7, 0.25],
        [0.000_1, 12.5, 2.0, 1.0],
    ]
}

#[test]
fn rule_x_is_the_nearest_half_of_the_gained_premultiplied_sample() {
    let document = float_doc(3, 2, &IccProfile::srgb(), &[scene()]);
    let rendered = document.render(LIMIT, 0, None).unwrap();
    let source = rendered.to_rgba32().unwrap();
    for stops in [0.0, 1.0, -2.5, 4.0, 20.0, -20.0] {
        let (halves, peak) = extended(&document, &rendered, stops, 6 * 8).unwrap();
        // The rule's f32 products: the gain as the f32 `2^stops`, the
        // gained sample held to ±65504, then times alpha.
        let gain = 2f64.powf(stops) as f32;
        let mut expected_peak = f64::NEG_INFINITY;
        for (i, pixel) in source.iter().enumerate() {
            for c in 0..3 {
                let value = f64::from((pixel[c] * gain).clamp(-65504.0, 65504.0) * pixel[3]);
                let bits = halves[i * 4 + c];
                assert!(
                    is_nearest(bits, value),
                    "{stops} stops, pixel {i}:{c}: {} for {value}",
                    half(bits)
                );
                expected_peak = expected_peak.max(half(bits));
            }
            assert!(
                is_nearest(halves[i * 4 + 3], f64::from(pixel[3])),
                "alpha {i}"
            );
        }
        assert_eq!(f64::from(peak), expected_peak, "{stops} stops");
    }
}

#[test]
fn values_above_white_survive_and_one_stop_doubles_the_peak() {
    let document = float_doc(1, 1, &IccProfile::srgb(), &[vec![[4.0, 4.0, 4.0, 1.0]]]);
    let rendered = document.render(LIMIT, 0, None).unwrap();
    let (halves, peak) = extended(&document, &rendered, 0.0, 8).unwrap();
    assert_eq!(half(halves[0]), 4.0);
    assert_eq!(peak, 4.0);
    let (_, doubled) = extended(&document, &rendered, 1.0, 8).unwrap();
    assert_eq!(doubled, 8.0);
    // Ties go to even: 2049 lies halfway between 2048 and 2050.
    let tie = float_doc(
        1,
        1,
        &IccProfile::srgb(),
        &[vec![[2049.0, 2051.0, 0.0, 1.0]]],
    );
    let rendered = tie.render(LIMIT, 0, None).unwrap();
    let (halves, _) = extended(&tie, &rendered, 0.0, 8).unwrap();
    assert_eq!(half(halves[0]), 2048.0);
    assert_eq!(half(halves[1]), 2052.0);
}

#[test]
fn refusals_name_their_reason() {
    let document = float_doc(2, 1, &IccProfile::srgb(), &[vec![[1.0; 4]; 2]]);
    let rendered = document.render(LIMIT, 0, None).unwrap();
    assert_eq!(
        extended(&document, &rendered, 0.0, 15).unwrap_err(),
        "the extended-range display buffer must hold width × height × 8 bytes"
    );
    let error = extended(&document, &rendered, 21.0, 16).unwrap_err();
    assert!(error.starts_with("view exposure must be"), "{error}");

    let image =
        PrecisionImage::from_rgba16(2, 1, vec![[1, 2, 3, 65535]; 2], IccProfile::srgb(), None)
            .unwrap();
    let plain =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    assert_eq!(
        extended(&plain, &rendered, 0.0, 16).unwrap_err(),
        "an extended-range display requires a 32-bit float document"
    );
    let codes = plain.render(LIMIT, 0, None).unwrap();
    assert_eq!(
        extended(&document, &codes, 0.0, 16).unwrap_err(),
        "an extended-range display requires rgba32_float storage"
    );

    // A limit below the plan refuses before anything is written.
    let mut out = vec![7u8; 16];
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_display_extended(
            &document,
            &rendered,
            0.0,
            out.as_mut_ptr(),
            16,
            ptr::null_mut(),
            1024,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!ok);
    assert!(!take_err_string(err).is_empty());
    assert!(out.iter().all(|&b| b == 7));
}

#[test]
fn null_arguments_return_false() {
    let document = float_doc(1, 1, &IccProfile::srgb(), &[vec![[1.0; 4]]]);
    let rendered = document.render(LIMIT, 0, None).unwrap();
    let mut out = [0u8; 8];
    let mut err: *mut c_char = ptr::null_mut();
    let calls = [
        (ptr::null(), &rendered as *const _, out.as_mut_ptr()),
        (&document as *const _, ptr::null(), out.as_mut_ptr()),
        (
            &document as *const _,
            &rendered as *const _,
            ptr::null_mut(),
        ),
    ];
    for (doc, image, buffer) in calls {
        let ok = unsafe {
            dr_precision_doc_display_extended(
                doc,
                image,
                0.0,
                buffer,
                8,
                ptr::null_mut(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!ok);
        assert!(!take_err_string(err).is_empty());
        err = ptr::null_mut();
    }
    let ok = unsafe {
        dr_precision_doc_display_extended(
            ptr::null(),
            ptr::null(),
            0.0,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    };
    assert!(!ok);
}
