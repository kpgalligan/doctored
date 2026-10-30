//! Black-box color-estimation contracts and independent compositing oracles.
//! These isolate color estimation with supplied alpha; general cutout quality
//! remains a separate gate, including real photographs and estimated alpha.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::{ffi::*, ffi_color::*, ffi_doc::*, ffi_foreground::*, ffi_jobs::*};
use std::{
    ffi::{CStr, CString},
    ptr,
};

fn fixture(w: u32, h: u32, linear: bool) -> (Vec<u8>, Vec<u8>, [f64; 3]) {
    let foreground = [0.18, 0.36, 0.62];
    let background = [0.91, 0.06, 0.12];
    let mut bytes = Vec::new();
    let mut alpha = Vec::new();
    for _y in 0..h {
        for x in 0..w {
            let a = (((f64::from(x) / f64::from(w - 1) - 0.3) / 0.4).clamp(0.0, 1.0) * 255.0)
                .round() as u8;
            alpha.push(a);
            for c in 0..3 {
                let v = f64::from(a) / 255.0 * foreground[c]
                    + (1.0 - f64::from(a) / 255.0) * background[c];
                let encoded = if linear { srgb_encode(v) } else { v };
                bytes.push((encoded * 255.0).round() as u8);
            }
            bytes.push(255);
        }
    }
    (bytes, alpha, foreground)
}

fn srgb_encode(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

unsafe fn error_text(error: *mut std::ffi::c_char) -> String {
    assert!(!error.is_null());
    let value = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    value
}

#[test]
fn removes_known_background_in_both_declared_color_domains_and_preserves_inputs() {
    for linear in [false, true] {
        let (bytes, alpha, foreground) = fixture(97, 65, linear);
        unsafe {
            let source = dr_image_from_rgba(bytes.as_ptr(), 97, 65);
            let doc = dr_doc_from_image(source);
            let params = CString::new(if linear {
                r#"{"color_space":"linear"}"#
            } else {
                r#"{"color_space":"encoded"}"#
            })
            .unwrap();
            let mut error = ptr::null_mut();
            let result = dr_image_estimate_foreground(
                doc,
                source,
                alpha.as_ptr(),
                params.as_ptr(),
                ptr::null(),
                &mut error,
            );
            assert!(error.is_null(), "{}", error_text(error));
            assert!(!result.is_null());
            let out = std::slice::from_raw_parts(dr_image_pixels_rgba(result), bytes.len());
            let mut initial_error = 0.0;
            let mut final_error = 0.0;
            let mut samples = 0;
            for (i, &a) in alpha.iter().enumerate() {
                assert_eq!(out[4 * i + 3], 255);
                if a == 0 || a == 255 {
                    assert_eq!(&out[4 * i..4 * i + 4], &bytes[4 * i..4 * i + 4]);
                    continue;
                }
                for c in 0..3 {
                    let decode = |b: u8| {
                        let v = f64::from(b) / 255.0;
                        if linear {
                            srgb_decode(v)
                        } else {
                            v
                        }
                    };
                    let weight = f64::from(a) / 255.0;
                    initial_error += weight * (decode(bytes[4 * i + c]) - foreground[c]).abs();
                    final_error += weight * (decode(out[4 * i + c]) - foreground[c]).abs();
                    samples += 1;
                }
            }
            // Independent constant-color oracle with a broad partial-alpha
            // ramp. Predeclared useful behavior: <=0.02 weighted mean color
            // error and >=75% reduction of the known background contamination.
            assert!(
                final_error / samples as f64 <= 0.02,
                "linear={linear}, MAD={}",
                final_error / samples as f64
            );
            assert!(
                final_error <= initial_error * 0.25,
                "linear={linear}, before={initial_error}, after={final_error}"
            );
            assert_eq!(
                std::slice::from_raw_parts(dr_image_pixels_rgba(source), bytes.len()),
                bytes
            );
            dr_image_free(result);
            dr_doc_free(doc);
            dr_image_free(source);
        }
    }
}

#[test]
fn linear_estimation_uses_the_assigned_profile_trc_instead_of_assuming_srgb() {
    let (bytes, alpha, foreground) = fixture(97, 65, false);
    unsafe {
        let mut profile = vec![0; dr_builtin_profile_len(0)];
        assert!(dr_builtin_profile(0, profile.as_mut_ptr(), profile.len()));
        // Retain the built-in matrix/header; replace only each TRC's payload
        // with ICC curv count0 (identity). This is not another ICC builder.
        let count = u32::from_be_bytes(profile[128..132].try_into().unwrap()) as usize;
        let mut replaced = 0;
        for tag in 0..count {
            let entry = 132 + 12 * tag;
            if [b"rTRC", b"gTRC", b"bTRC"]
                .iter()
                .any(|signature| profile[entry..entry + 4] == signature[..])
            {
                let offset =
                    u32::from_be_bytes(profile[entry + 4..entry + 8].try_into().unwrap()) as usize;
                let size = u32::from_be_bytes(profile[entry + 8..entry + 12].try_into().unwrap());
                assert!(size >= 12);
                profile[offset..offset + 12].copy_from_slice(b"curv\0\0\0\0\0\0\0\0");
                profile[entry + 8..entry + 12].copy_from_slice(&12_u32.to_be_bytes());
                replaced += 1;
            }
        }
        assert_eq!(replaced, 3);
        let source = dr_image_from_rgba(bytes.as_ptr(), 97, 65);
        let original_doc = dr_doc_from_image(source);
        let doc = dr_doc_assign_profile(
            original_doc,
            profile.as_ptr(),
            profile.len(),
            std::ptr::null_mut(),
        );
        assert!(!doc.is_null());
        assert!(dr_doc_profile_is_convertible(doc));
        let mut outputs = Vec::new();
        for space in ["encoded", "linear"] {
            let params = CString::new(format!(r#"{{"color_space":"{space}"}}"#)).unwrap();
            let mut error = ptr::null_mut();
            let result = dr_image_estimate_foreground(
                doc,
                source,
                alpha.as_ptr(),
                params.as_ptr(),
                ptr::null(),
                &mut error,
            );
            assert!(error.is_null(), "{}", error_text(error));
            assert!(!result.is_null());
            outputs.push(
                std::slice::from_raw_parts(dr_image_pixels_rgba(result), bytes.len()).to_vec(),
            );
            dr_image_free(result);
        }
        // Encoded and decoded values are the same in an identity-TRC profile.
        // Only byte rounding may differ. A hard-coded sRGB decode/encode would
        // make these two calls solve different compositing equations.
        for (encoded, linear) in outputs[0].iter().zip(&outputs[1]) {
            assert!(encoded.abs_diff(*linear) <= 1);
        }
        let mut weighted_error = 0.0;
        let mut samples = 0;
        for (i, &a) in alpha.iter().enumerate() {
            if a == 0 || a == 255 {
                assert_eq!(&outputs[1][4 * i..4 * i + 4], &bytes[4 * i..4 * i + 4]);
            } else {
                for (c, expected) in foreground.iter().enumerate() {
                    weighted_error += f64::from(a) / 255.0
                        * (f64::from(outputs[1][4 * i + c]) / 255.0 - expected).abs();
                    samples += 1;
                }
            }
        }
        assert!(weighted_error / samples as f64 <= 0.02);
        assert_eq!(
            std::slice::from_raw_parts(dr_image_pixels_rgba(source), bytes.len()),
            bytes
        );
        dr_doc_free(doc);
        dr_doc_free(original_doc);
        dr_image_free(source);
    }
}

#[test]
fn zero_strength_and_binary_mattes_are_true_noops() {
    let (bytes, alpha, _) = fixture(17, 3, false);
    unsafe {
        let source = dr_image_from_rgba(bytes.as_ptr(), 17, 3);
        let doc = dr_doc_from_image(source);
        for (mask, params) in [
            (alpha.clone(), r#"{"color_space":"encoded","strength":0}"#),
            (
                alpha
                    .iter()
                    .map(|a| if *a < 128 { 0 } else { 255 })
                    .collect(),
                r#"{"color_space":"linear"}"#,
            ),
            (vec![0; alpha.len()], r#"{"color_space":"encoded"}"#),
            (vec![255; alpha.len()], r#"{"color_space":"encoded"}"#),
        ] {
            let mut error = ptr::null_mut();
            assert!(dr_image_estimate_foreground(
                doc,
                source,
                mask.as_ptr(),
                CString::new(params).unwrap().as_ptr(),
                ptr::null(),
                &mut error
            )
            .is_null());
            assert!(error.is_null());
        }
        dr_doc_free(doc);
        dr_image_free(source);
    }
}

#[test]
fn strict_parameters_ambiguous_alpha_and_nonopaque_source_refuse() {
    let (mut bytes, alpha, _) = fixture(17, 3, false);
    unsafe {
        let source = dr_image_from_rgba(bytes.as_ptr(), 17, 3);
        let doc = dr_doc_from_image(source);
        for params in [
            "{}",
            "[]",
            r#"{"color_space":"srgb"}"#,
            r#"{"color_space":"linear","strength":2}"#,
            r#"{"color_space":"encoded","strength":true}"#,
            r#"{"color_space":"encoded","alpha":1}"#,
        ] {
            let mut error = ptr::null_mut();
            assert!(dr_image_estimate_foreground(
                doc,
                source,
                alpha.as_ptr(),
                CString::new(params).unwrap().as_ptr(),
                ptr::null(),
                &mut error
            )
            .is_null());
            assert!(!error_text(error).is_empty());
        }
        let params = CString::new(r#"{"color_space":"encoded"}"#).unwrap();
        let mut error = ptr::null_mut();
        assert!(dr_image_estimate_foreground(
            doc,
            source,
            vec![128; alpha.len()].as_ptr(),
            params.as_ptr(),
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(error_text(error).contains("constant fractional"));
        bytes[3] = 254;
        let transparent = dr_image_from_rgba(bytes.as_ptr(), 17, 3);
        let mut error = ptr::null_mut();
        assert!(dr_image_estimate_foreground(
            doc,
            transparent,
            alpha.as_ptr(),
            params.as_ptr(),
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(error_text(error).contains("opaque source"));
        let small = dr_image_from_rgba([0, 0, 0, 255].as_ptr(), 1, 1);
        let mut error = ptr::null_mut();
        assert!(dr_image_estimate_foreground(
            doc,
            small,
            [128].as_ptr(),
            params.as_ptr(),
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(error_text(error).contains("dimensions"));
        dr_image_free(small);
        dr_image_free(transparent);
        dr_doc_free(doc);
        dr_image_free(source);
    }
}

#[test]
fn required_pointer_nulls_and_resource_refusal_are_safe() {
    let (bytes, alpha, _) = fixture(17, 3, false);
    unsafe {
        let source = dr_image_from_rgba(bytes.as_ptr(), 17, 3);
        let doc = dr_doc_from_image(source);
        let params = CString::new(r#"{"color_space":"encoded"}"#).unwrap();
        for (d, s, a, p) in [
            (
                ptr::null(),
                source.cast_const(),
                alpha.as_ptr(),
                params.as_ptr(),
            ),
            (
                doc.cast_const(),
                ptr::null(),
                alpha.as_ptr(),
                params.as_ptr(),
            ),
            (
                doc.cast_const(),
                source.cast_const(),
                ptr::null(),
                params.as_ptr(),
            ),
            (
                doc.cast_const(),
                source.cast_const(),
                alpha.as_ptr(),
                ptr::null(),
            ),
        ] {
            assert!(
                dr_image_estimate_foreground(d, s, a, p, ptr::null(), ptr::null_mut()).is_null()
            );
        }
        for cancel in [false, true] {
            let token = dr_job_control_create();
            assert!(dr_job_control_set_scratch_limit(token, 1));
            if cancel {
                dr_job_control_cancel(token);
            }
            let mut error = ptr::null_mut();
            assert!(dr_image_estimate_foreground(
                doc,
                source,
                alpha.as_ptr(),
                params.as_ptr(),
                token,
                &mut error
            )
            .is_null());
            let message = error_text(error);
            assert!(message.contains(if cancel {
                "cancelled"
            } else {
                "working memory"
            }));
            assert_eq!(dr_job_control_scratch_bytes(token), 0);
            let mut error = ptr::null_mut();
            assert!(dr_image_estimate_foreground(
                doc,
                source,
                alpha.as_ptr(),
                params.as_ptr(),
                token,
                &mut error
            )
            .is_null());
            assert!(error_text(error).contains("already been used"));
            dr_job_control_free(token);
        }
        dr_doc_free(doc);
        dr_image_free(source);
    }
}

#[test]
fn live_cancellation_during_pyramid_or_solve_never_publishes() {
    let (bytes, alpha, _) = fixture(1537, 1025, false);
    unsafe {
        let source = dr_image_from_rgba(bytes.as_ptr(), 1537, 1025);
        let doc = dr_doc_from_image(source);
        // Integer addresses cross the test thread boundary; handles stay owned
        // here until join. The production contract likewise requires retention.
        let addresses = (source as usize, doc as usize);
        let alpha = &alpha;
        let render = move |token: usize| {
            let params = CString::new(r#"{"color_space":"encoded"}"#).unwrap();
            let mut error = ptr::null_mut();
            let result = dr_image_estimate_foreground(
                addresses.1 as _,
                addresses.0 as _,
                alpha.as_ptr(),
                params.as_ptr(),
                token as _,
                &mut error,
            );
            let null = result.is_null();
            dr_image_free(result);
            (
                !null,
                if error.is_null() {
                    String::new()
                } else {
                    error_text(error)
                },
            )
        };
        // k = 1: the coarsest pyramid level polls only twice before its
        // sweep's report (the entry's next report is at n0 + 3).
        let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= 0.15, render);
        let n0 = entry.checkpoints;
        assert!(
            next.expect("the entry's kernel never reported again")
                .checkpoints
                > n0 + 2,
            "the entry's kernel polls fewer than two checkpoints before its next report"
        );
        let parked = cancelled_parked_at(
            n0 + 2,
            &|progress, stage| stage == entry.stage && progress == entry.progress,
            render,
        );
        assert!(parked.scratch > 0);
        eprintln!(
            "foreground estimation cancellation: {}",
            parked_line(&parked)
        );
        dr_doc_free(doc);
        dr_image_free(source);
    }
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn live_cancellation_during_pyramid_or_solve_never_publishes() {
        let (bytes, alpha, _) = fixture(1537, 1025, false);
        unsafe {
            let source = dr_image_from_rgba(bytes.as_ptr(), 1537, 1025);
            let doc = dr_doc_from_image(source);
            let token = dr_job_control_create();
            // Integer addresses cross the test thread boundary; handles stay owned
            // here until join. The production contract likewise requires retention.
            let addresses = (source as usize, doc as usize, token as usize);
            let worker = std::thread::spawn(move || {
                let params = CString::new(r#"{"color_space":"encoded"}"#).unwrap();
                let mut error = ptr::null_mut();
                let result = dr_image_estimate_foreground(
                    addresses.1 as _,
                    addresses.0 as _,
                    alpha.as_ptr(),
                    params.as_ptr(),
                    addresses.2 as _,
                    &mut error,
                );
                let null = result.is_null();
                dr_image_free(result);
                (
                    null,
                    if error.is_null() {
                        String::new()
                    } else {
                        error_text(error)
                    },
                )
            });
            let limit = Instant::now() + Duration::from_secs(5);
            while dr_job_control_progress(token) < 0.15 && Instant::now() < limit {
                std::thread::yield_now();
            }
            assert!(dr_job_control_progress(token) >= 0.15);
            assert!(dr_job_control_scratch_bytes(token) > 0);
            let start = Instant::now();
            dr_job_control_cancel(token);
            let (null, message) = worker.join().unwrap();
            assert!(null && message.contains("cancelled"), "{message}");
            assert!(start.elapsed() < Duration::from_millis(500));
            assert_eq!(dr_job_control_scratch_bytes(token), 0);
            dr_job_control_free(token);
            dr_doc_free(doc);
            dr_image_free(source);
        }
    }
}
