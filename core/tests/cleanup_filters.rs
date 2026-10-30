//! The destructive noise-cleanup filters at the C boundary, through
//! `dr_image_filter_op`'s `dust_and_scratches` and `reduce_noise`.
//! Bypasses, alpha preservation, membership and refusal wording are exact.
use crate::live_filter_support::text;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::sampling_rank::disc_offsets;
use doctored_core::DrImage;
use serde_json::json;
use std::ffi::CString;
use std::ptr;

fn plate(w: u32, h: u32) -> Vec<u8> {
    // Speckle and grain over structure, with adversarial alpha: real work for
    // a rank filter and a denoise, and transparency to leave alone.
    (0..w * h)
        .flat_map(|i| {
            let base = 40 + (i % 7) * 9 + (i / w) * 5;
            let speck = if i % 11 == 0 { 200 } else { 0 };
            let value = |offset: u32| ((base + speck + offset) % 256) as u8;
            [
                value(0),
                value(37),
                value(91),
                match i % 5 {
                    0 => 0,
                    1 => 1,
                    2 => 255,
                    _ => ((i * 53 + 17) % 256) as u8,
                },
            ]
        })
        .collect()
}
fn image(w: u32, h: u32, pixels: &[u8]) -> *mut doctored_core::DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}
fn samples(image: *const doctored_core::DrImage, count: usize) -> Vec<u8> {
    let p = unsafe { dr_image_pixels_rgba(image) };
    assert!(!p.is_null());
    unsafe { std::slice::from_raw_parts(p, count * 4) }.to_vec()
}
/// `dr_image_filter_op` with a NULL control, given its raw `params` JSON text:
/// the handle (NULL on a bypass or a refusal) and the error (empty if none).
unsafe fn filter_op(img: *const DrImage, op: &str, params: &str) -> (*mut DrImage, String) {
    let op = CString::new(op).unwrap();
    let params = CString::new(params).unwrap();
    let mut e = ptr::null_mut();
    let p = unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), ptr::null(), &mut e) };
    (p, unsafe { text(e) })
}
unsafe fn dust_op(img: *const DrImage, radius: f64, threshold: f64) -> (*mut DrImage, String) {
    let params = json!({"radius": radius, "threshold": threshold});
    unsafe { filter_op(img, "dust_and_scratches", &params.to_string()) }
}
unsafe fn denoise_op(
    img: *const DrImage,
    radius: f64,
    l: f64,
    d: f64,
    c: f64,
) -> (*mut DrImage, String) {
    let params = json!({"radius": radius, "luminance": l, "detail": d, "color": c});
    unsafe { filter_op(img, "reduce_noise", &params.to_string()) }
}
fn dust(w: u32, h: u32, pixels: &[u8], radius: f64, threshold: f64) -> Vec<u8> {
    let produced = unsafe {
        let source = image(w, h, pixels);
        let (out, e) = dust_op(source, radius, threshold);
        assert!(e.is_empty(), "{e}");
        dr_image_free(source);
        out
    };
    assert!(!produced.is_null());
    let out = samples(produced, (w * h) as usize);
    unsafe { dr_image_free(produced) };
    out
}
fn denoise(w: u32, h: u32, pixels: &[u8], radius: f64, l: f64, d: f64, c: f64) -> Vec<u8> {
    let produced = unsafe {
        let source = image(w, h, pixels);
        let (out, e) = denoise_op(source, radius, l, d, c);
        assert!(e.is_empty(), "{e}");
        dr_image_free(source);
        out
    };
    assert!(!produced.is_null());
    let out = samples(produced, (w * h) as usize);
    unsafe { dr_image_free(produced) };
    out
}
#[test]
fn destructive_dust_output_is_always_a_member_of_its_own_window() {
    let (w, h) = (9u32, 7u32);
    let pixels = plate(w, h);
    let out = dust(w, h, &pixels, 2.0, 0.0);
    for y in 0..h {
        for x in 0..w {
            let index = (y * w + x) as usize;
            if pixels[index * 4 + 3] == 0 {
                continue;
            }
            let window: Vec<[u8; 4]> = disc_offsets(2.0)
                .into_iter()
                .map(|(dx, dy)| {
                    let sx = (i64::from(x) + i64::from(dx)).clamp(0, i64::from(w) - 1) as usize;
                    let sy = (i64::from(y) + i64::from(dy)).clamp(0, i64::from(h) - 1) as usize;
                    let at = (sy * w as usize + sx) * 4;
                    [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
                })
                .filter(|p| p[3] > 0)
                .collect();
            for c in 0..3 {
                assert!(
                    window.iter().any(|p| p[c] == out[index * 4 + c]),
                    "sample{index} channel{c} is not a member of its own window"
                );
            }
        }
    }
}

#[test]
fn destructive_cleanup_preserves_alpha_and_transparent_samples_exactly() {
    let (w, h) = (8, 6);
    let pixels = plate(w, h);
    for out in [
        dust(w, h, &pixels, 2.0, 0.0),
        denoise(w, h, &pixels, 1.0, 0.05, 0.0, 1.0),
    ] {
        for index in 0..(w * h) as usize {
            assert_eq!(
                out[index * 4 + 3],
                pixels[index * 4 + 3],
                "sample{index}: alpha must survive cleanup unchanged"
            );
            if pixels[index * 4 + 3] == 0 {
                assert_eq!(
                    &out[index * 4..index * 4 + 4],
                    &pixels[index * 4..index * 4 + 4],
                    "sample{index}: a transparent sample keeps its stored colour"
                );
            }
        }
    }
}

#[test]
fn exact_cleanup_bypasses_return_null_without_an_error() {
    let (w, h) = (8, 6);
    let pixels = plate(w, h);
    let source = image(w, h, &pixels);
    for (label, (produced, e)) in unsafe {
        [
            ("dust radius zero", dust_op(source, 0.0, 0.0)),
            ("dust radius below one pixel", dust_op(source, 0.75, 0.0)),
            ("dust threshold one", dust_op(source, 4.0, 1.0)),
            (
                "denoise radius zero",
                denoise_op(source, 0.0, 0.05, 0.0, 1.0),
            ),
            (
                "denoise both halves off",
                denoise_op(source, 2.0, 0.0, 0.0, 0.0),
            ),
            (
                "denoise full detail retention and no colour",
                denoise_op(source, 2.0, 0.05, 1.0, 0.0),
            ),
            // A NULL handle is the same silent refusal, checked before the
            // parameters, like every other export here.
            ("null handle", dust_op(ptr::null(), 400.0, 9.0)),
            (
                "null handle denoise",
                denoise_op(ptr::null(), -1.0, 9.0, 9.0, 9.0),
            ),
        ]
    } {
        assert!(produced.is_null(), "{label} must be an exact bypass");
        assert!(e.is_empty(), "{label} is not an error: {e}");
    }
    // A NULL err_out is tolerated on every path, bypass and failure alike.
    unsafe {
        for (op, params) in [
            ("dust_and_scratches", r#"{"radius":0,"threshold":0}"#),
            ("dust_and_scratches", r#"{"radius":4,"threshold":9}"#),
            (
                "reduce_noise",
                r#"{"radius":1,"luminance":9,"detail":0,"color":0}"#,
            ),
        ] {
            let op = CString::new(op).unwrap();
            let params = CString::new(params).unwrap();
            assert!(dr_image_filter_op(
                source,
                op.as_ptr(),
                params.as_ptr(),
                ptr::null(),
                ptr::null_mut()
            )
            .is_null());
        }
        dr_image_free(source);
    }
}

#[test]
fn bad_cleanup_parameters_explain_themselves() {
    let (w, h) = (8, 6);
    let pixels = plate(w, h);
    for (arguments, fragment) in [
        ((100.0, 0.0, 0.0, 0.0, true), "radius"),
        ((2.0, 1.5, 0.0, 0.0, true), "threshold"),
        ((-1.0, 0.0, 0.0, 0.0, false), "radius"),
        ((1.0, 0.5, 0.0, 0.0, false), "luminance"),
        ((1.0, 0.01, 2.0, 0.0, false), "detail"),
        ((1.0, 0.01, 0.5, -1.0, false), "color"),
    ] {
        let (a, b, c, d, is_dust) = arguments;
        let message = unsafe {
            let source = image(w, h, &pixels);
            let (out, e) = if is_dust {
                dust_op(source, a, b)
            } else {
                denoise_op(source, a, b, c, d)
            };
            dr_image_free(source);
            assert!(out.is_null());
            e
        };
        assert!(message.contains(fragment), "{message}");
    }
    // JSON has no literal for a non-finite number, so NaN reaches the core only
    // as text the parser refuses.
    for (op, params) in [
        ("dust_and_scratches", r#"{"radius":NaN,"threshold":0}"#),
        (
            "reduce_noise",
            r#"{"radius":1,"luminance":Infinity,"detail":0,"color":0}"#,
        ),
    ] {
        let message = unsafe {
            let source = image(w, h, &pixels);
            let (out, e) = filter_op(source, op, params);
            dr_image_free(source);
            assert!(out.is_null());
            e
        };
        assert!(message.contains("invalid filter params"), "{message}");
    }
}

#[test]
fn an_oversized_destructive_rank_window_refuses_before_allocating_pixels() {
    // The plan is consulted against the destructive ceiling BEFORE the float
    // plane is filled: at the image ceiling that would be gigabytes written and
    // thrown away to learn the radius was never admissible.
    let (w, h) = (2400, 2000);
    let pixels = vec![128u8; (w * h * 4) as usize];
    let message = unsafe {
        let source = image(w, h, &pixels);
        let (out, e) = dust_op(source, 24.0, 0.0);
        dr_image_free(source);
        assert!(out.is_null());
        e
    };
    assert!(message.contains("work"), "{message}");
}
