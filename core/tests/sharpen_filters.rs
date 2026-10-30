//! The destructive Phase 9B sharpening filters at the C boundary, through
//! `dr_image_filter_op`'s `unsharp_mask` and `high_pass`. Bypasses,
//! alpha preservation and refusal wording are exact checks.
use crate::live_filter_support::text;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::DrImage;
use serde_json::json;
use std::ffi::CString;
use std::ptr;

fn plate(w: u32, h: u32) -> Vec<u8> {
    // Coprime ramps with hard edges and adversarial alpha, so sharpening has
    // real detail to amplify and transparency to leave alone.
    (0..w * h)
        .flat_map(|i| {
            let edge = if (i / w).is_multiple_of(3) { 255 } else { 8 };
            [
                ((i * 37 + 11) % 256) as u8,
                edge,
                ((i * 91 + 3) % 256) as u8,
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
unsafe fn unsharp(
    img: *const DrImage,
    radius: f64,
    amount: f64,
    threshold: f64,
) -> (*mut DrImage, String) {
    let params = json!({"radius": radius, "amount": amount, "threshold": threshold});
    unsafe { filter_op(img, "unsharp_mask", &params.to_string()) }
}
unsafe fn high_pass(img: *const DrImage, radius: f64) -> (*mut DrImage, String) {
    unsafe { filter_op(img, "high_pass", &json!({ "radius": radius }).to_string()) }
}

#[test]
fn destructive_sharpening_preserves_alpha_and_transparent_samples_exactly() {
    let (w, h) = (8, 6);
    let pixels = plate(w, h);
    for produced in [
        unsafe {
            let source = image(w, h, &pixels);
            let (out, e) = unsharp(source, 2.0, 3.0, 0.0);
            dr_image_free(source);
            assert!(e.is_empty());
            out
        },
        unsafe {
            let source = image(w, h, &pixels);
            let (out, e) = high_pass(source, 2.0);
            dr_image_free(source);
            assert!(e.is_empty());
            out
        },
    ] {
        let out = samples(produced, (w * h) as usize);
        unsafe { dr_image_free(produced) };
        for index in 0..(w * h) as usize {
            assert_eq!(
                out[index * 4 + 3],
                pixels[index * 4 + 3],
                "pixel{index}: alpha must survive sharpening unchanged"
            );
            if pixels[index * 4 + 3] == 0 {
                assert_eq!(
                    &out[index * 4..index * 4 + 4],
                    &pixels[index * 4..index * 4 + 4],
                    "pixel{index}: a transparent sample stays exact"
                );
            }
        }
    }
}

#[test]
fn high_pass_of_a_constant_plate_is_the_neutral_pivot() {
    let (w, h) = (6, 6);
    for level in [0u8, 77, 255] {
        let pixels: Vec<u8> = (0..w * h)
            .flat_map(|_| [level, level, level, 255])
            .collect();
        let source = image(w, h, &pixels);
        let (out, e) = unsafe { high_pass(source, 3.0) };
        unsafe { dr_image_free(source) };
        assert!(e.is_empty() && !out.is_null());
        let values = samples(out, (w * h) as usize);
        unsafe { dr_image_free(out) };
        for index in 0..(w * h) as usize {
            for c in 0..3 {
                assert!(values[index * 4 + c].abs_diff(128) <= 1, "level {level}");
            }
            assert_eq!(values[index * 4 + 3], 255);
        }
    }
}

#[test]
fn exact_bypasses_return_null_without_an_error_and_bad_parameters_explain_themselves() {
    let (w, h) = (4, 4);
    let pixels = plate(w, h);
    let source = image(w, h, &pixels);
    for result in unsafe {
        [
            unsharp(source, 0.0, 2.0, 0.0),
            unsharp(source, 2.0, 0.0, 0.0),
            high_pass(source, 0.0),
        ]
    } {
        assert!(result.0.is_null(), "an exact bypass allocates nothing");
        assert_eq!(result.1, "", "an exact bypass is not an error");
    }
    for (radius, amount, threshold) in [
        (-1.0, 1.0, 0.0),
        (64.5, 1.0, 0.0),
        (2.0, 5.5, 0.0),
        (2.0, 1.0, 1.5),
        (2.0, 1.0, -0.5),
    ] {
        let (p, message) = unsafe { unsharp(source, radius, amount, threshold) };
        assert!(p.is_null());
        assert!(!message.is_empty(), "{radius}/{amount}/{threshold}");
    }
    for radius in [-1.0, 64.5] {
        let (p, message) = unsafe { high_pass(source, radius) };
        assert!(p.is_null());
        assert!(message.contains("radius"), "{message}");
    }
    // JSON has no literal for a non-finite number, so NaN and Infinity reach
    // the core only as text the parser refuses.
    for (op, params) in [
        ("unsharp_mask", r#"{"radius":NaN,"amount":1,"threshold":0}"#),
        (
            "unsharp_mask",
            r#"{"radius":2,"amount":Infinity,"threshold":0}"#,
        ),
        ("high_pass", r#"{"radius":NaN}"#),
    ] {
        let (p, message) = unsafe { filter_op(source, op, params) };
        assert!(p.is_null());
        assert!(message.contains("invalid filter params"), "{message}");
    }
    // A NULL handle is a REFUSAL, not an error — the same shape
    // `dr_image_adjust_op` takes and what "NULL tolerance first" means. It is
    // checked before the parameter range, so a NULL handle with a bad radius
    // still refuses silently rather than reporting the radius.
    for produced in unsafe {
        [
            unsharp(ptr::null(), 1.0, 1.0, 0.0),
            high_pass(ptr::null(), 1.0),
            high_pass(ptr::null(), -1.0),
        ]
    } {
        assert!(produced.0.is_null());
        assert!(
            produced.1.is_empty(),
            "a NULL image is a refusal, not an error"
        );
    }
    // A NULL err_out is tolerated on every path.
    let op = CString::new("unsharp_mask").unwrap();
    let bad = CString::new(r#"{"radius":-1}"#).unwrap();
    assert!(unsafe {
        dr_image_filter_op(
            ptr::null(),
            op.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        dr_image_filter_op(
            source,
            op.as_ptr(),
            bad.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    unsafe { dr_image_free(source) };
}

#[test]
fn a_large_destructive_radius_runs_at_twelve_megapixels() {
    // 4000x3000 at radius 48 was 289 taps of the ceil(3σ) kernel, past the
    // destructive ceiling. From σ4 up the Gaussian's cost does not grow with
    // the radius, so it now runs; a flat plate stays
    // flat under Unsharp Mask.
    let (w, h) = (4000, 3000);
    let pixels = vec![128u8; (w * h * 4) as usize];
    let source = image(w, h, &pixels);
    let (p, message) = unsafe { unsharp(source, 48.0, 1.0, 0.0) };
    assert!(!p.is_null(), "{message}");
    assert_eq!(samples(p, (w * h) as usize), pixels);
    unsafe {
        dr_image_free(p);
        dr_image_free(source);
    }
    // An ordinary photographic radius on an ordinary plate is admitted.
    let small = image(64, 64, &vec![128u8; 64 * 64 * 4]);
    let (ok, e) = unsafe { unsharp(small, 1.2, 1.0, 0.0) };
    assert!(!ok.is_null(), "{e}");
    unsafe {
        dr_image_free(ok);
        dr_image_free(small);
    }
}
