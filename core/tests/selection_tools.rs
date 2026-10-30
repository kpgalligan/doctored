//! Black-box selection tool contracts: independently calculated coverage,
//! profile-dependent distances, fixed-canvas transforms, and atomic failures.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use common::*;
use doctored_core::{ffi::*, ffi_doc::*, ffi_jobs::*, ffi_selection_tools::*};
use image::{Rgba, RgbaImage};
use std::{ffi::CString, ptr};

fn json(s: &str) -> CString {
    CString::new(s).unwrap()
}

#[test]
fn sampled_range_unions_samples_and_respects_source_alpha() {
    let pixels = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 0, 0, 0];
    unsafe {
        let source = dr_image_from_rgba(pixels.as_ptr(), 4, 1);
        let doc = dr_doc_from_image(source);
        let mut out = vec![73; 4];
        assert!(dr_selection_color_range(
            doc,
            source,
            json(
                r#"{"mode":"sampled","samples":[[255,0,0],[0,255,0]],"tolerance":0,"softness":0}"#
            )
            .as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [255, 128, 0, 0]);
        assert_eq!(
            std::slice::from_raw_parts(dr_image_pixels_rgba(source), 16),
            pixels
        );
        dr_doc_free(doc);
        dr_image_free(source);
    }
}

#[test]
fn tonal_range_uses_lab_lightness_and_soft_linear_shoulders() {
    unsafe {
        let bytes = [0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255];
        let source = dr_image_from_rgba(bytes.as_ptr(), 3, 1);
        let doc = dr_doc_from_image(source);
        let mut out = [99; 3];
        assert!(dr_selection_color_range(
            doc,
            source,
            json(r#"{"mode":"tonal","lower":0,"upper":40,"softness":20}"#).as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        // sRGB128 linear=0.21586, CIELab L*=116*cbrt(Y)-16=53.585.
        assert_eq!(out[0], 255);
        assert!((i16::from(out[1]) - 82).abs() <= 1);
        assert_eq!(out[2], 0);
        dr_doc_free(doc);
        dr_image_free(source);
    }
}

#[test]
fn quick_selection_stops_edges_and_gradient_drift_and_accepts_multiple_seeds() {
    let pixels = RgbaImage::from_fn(9, 2, |x, _| {
        Rgba([if x < 6 { 20 + x as u8 * 8 } else { 150 }; 4])
    });
    let mut pixels = pixels;
    for pixel in pixels.pixels_mut() {
        pixel[3] = 255;
    }
    unsafe {
        let source = dr_image_from_rgba(pixels.as_raw().as_ptr(), 9, 2);
        let doc = dr_doc_from_image(source);
        let mut out = [0; 18];
        assert!(dr_selection_quick(
            doc,
            source,
            json(r#"{"seeds":[[0,0]],"tolerance":12,"edge":8}"#).as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out[0], 255);
        assert_eq!(out[1], 255);
        assert_eq!(out[5], 0);
        assert_eq!(out[6], 0);
        assert!(dr_selection_quick(
            doc,
            source,
            json(r#"{"seeds":[[0,0],[8,1]],"tolerance":200,"edge":8}"#).as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(out.iter().all(|v| *v == 255));
        assert!(dr_selection_quick(
            doc,
            source,
            json(r#"{"seeds":[[0,0]],"tolerance":200,"edge":8}"#).as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out[5], 255);
        assert_eq!(out[6], 0);
        dr_doc_free(doc);
        dr_image_free(source);
    }
}

#[test]
fn coverage_transform_matches_independent_half_pixel_and_flip_oracles() {
    unsafe {
        let input = [0, 64, 128, 255];
        let mut out = [77; 4];
        let matrix = [1., 0., 0., 1., 0.5, 0.];
        assert!(dr_selection_transform(
            input.as_ptr(),
            4,
            1,
            matrix.as_ptr(),
            1,
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [0, 32, 96, 192]);
        let matrix = [-1., 0., 0., 1., 4., 0.];
        assert!(dr_selection_transform(
            input.as_ptr(),
            4,
            1,
            matrix.as_ptr(),
            1,
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [255, 128, 64, 0]);
        let matrix = [1., 0., 0., 1., 0., 0.];
        assert!(dr_selection_transform(
            input.as_ptr(),
            4,
            1,
            matrix.as_ptr(),
            1,
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, input);
    }
}

#[test]
fn refinement_identity_contrast_and_brush_extent() {
    unsafe {
        let source = dr_image_from_rgba([64; 16].as_ptr(), 4, 1);
        let mask = [0, 85, 170, 255];
        let mut out = [77; 4];
        assert!(dr_selection_refine(
            source,
            mask.as_ptr(),
            ptr::null(),
            json("{}").as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, mask);
        assert!(dr_selection_refine(
            source,
            mask.as_ptr(),
            ptr::null(),
            json(r#"{"contrast":0.5}"#).as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [0, 43, 213, 255]);
        // The area plane: EXCLUDE at both ends, INCLUDE in the middle. Across
        // flat colour the estimate is the window mean of the coverage (half),
        // written only under INCLUDE.
        let area = [0, 255, 255, 0];
        assert!(dr_selection_refine(
            source,
            mask.as_ptr(),
            area.as_ptr(),
            json("{}").as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out[0], mask[0]);
        assert_eq!(out[3], mask[3]);
        assert!(out[1] > mask[1]);
        assert!(out[2] < mask[2]);
        dr_image_free(source);
    }
}

#[test]
fn refinement_reuses_existing_morphology_exactly() {
    unsafe {
        let source = dr_image_from_rgba(vec![255; 11 * 9 * 4].as_ptr(), 11, 9);
        let mask: Vec<u8> = (0..99)
            .map(|i| {
                if i % 11 > 3 && i % 11 < 8 && i / 11 > 2 {
                    255
                } else {
                    0
                }
            })
            .collect();
        let mut expected = mask.clone();
        assert!(dr_selection_smooth(expected.as_mut_ptr(), 11, 9, 2.));
        assert!(dr_selection_grow(expected.as_mut_ptr(), 11, 9, 1.5));
        assert!(dr_selection_feather(expected.as_mut_ptr(), 11, 9, 2.));
        let mut out = vec![77; 99];
        assert!(dr_selection_refine(
            source,
            mask.as_ptr(),
            ptr::null(),
            json(r#"{"smooth":2,"shift":1.5,"feather":2}"#).as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, expected);
        dr_image_free(source);
    }
}

#[test]
fn invalid_inputs_and_all_nulls_refuse_without_publication() {
    unsafe {
        assert!(!dr_selection_color_range(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(!dr_selection_quick(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(!dr_selection_refine(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(!dr_selection_transform(
            ptr::null(),
            0,
            0,
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut()
        ));
        let source = dr_image_from_rgba([255; 8].as_ptr(), 2, 1);
        let doc = dr_doc_from_image(source);
        let mask = [0, 255];
        let mut out = [77; 2];
        for request in [
            r#"{"mode":"wat"}"#,
            r#"{"mode":"sampled","samples":[]}"#,
            r#"{"mode":"tonal","lower":75,"upper":25}"#,
            r#"{"mode":"tonal","typo":1}"#,
        ] {
            let mut err = ptr::null_mut();
            assert!(!dr_selection_color_range(
                doc,
                source,
                json(request).as_ptr(),
                out.as_mut_ptr(),
                ptr::null(),
                &mut err
            ));
            assert!(!take_err_string(err).is_empty());
            assert_eq!(out, [77; 2]);
        }
        for request in [
            r#"{"seeds":[]}"#,
            r#"{"seeds":[[2,0]]}"#,
            r#"{"seeds":[[0.5,0]]}"#,
            r#"{"seeds":[[0,0]],"edge":201}"#,
        ] {
            assert!(!dr_selection_quick(
                doc,
                source,
                json(request).as_ptr(),
                out.as_mut_ptr(),
                ptr::null(),
                ptr::null_mut()
            ));
            assert_eq!(out, [77; 2]);
        }
        for request in [
            r#"{"smooth":65}"#,
            r#"{"edge_radius":65}"#,
            r#"{"edge_radius":-1}"#,
            r#"{"smart_radius":1}"#,
            r#"{"edge_strength":1}"#,
            r#"{"contrast":-1}"#,
            r#"{"unknown":0}"#,
        ] {
            assert!(!dr_selection_refine(
                source,
                mask.as_ptr(),
                ptr::null(),
                json(request).as_ptr(),
                out.as_mut_ptr(),
                ptr::null(),
                ptr::null_mut()
            ));
            assert_eq!(out, [77; 2]);
        }
        assert!(!dr_selection_transform(
            mask.as_ptr(),
            2,
            1,
            [0.; 6].as_ptr(),
            1,
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [77; 2]);
        dr_doc_free(doc);
        dr_image_free(source);
    }
}

#[test]
fn all_tools_honor_cancelled_tokens_and_scratch_refusal() {
    unsafe {
        let source = dr_image_from_rgba([255; 8].as_ptr(), 2, 1);
        let doc = dr_doc_from_image(source);
        let mask = [0, 255];
        let mut out = [77; 2];
        for operation in 0..4 {
            for cancelled in [true, false] {
                let token = dr_job_control_create();
                if cancelled {
                    dr_job_control_cancel(token);
                } else {
                    assert!(dr_job_control_set_scratch_limit(token, 1));
                }
                let ok = match operation {
                    0 => dr_selection_color_range(
                        doc,
                        source,
                        json(r#"{"mode":"tonal"}"#).as_ptr(),
                        out.as_mut_ptr(),
                        token,
                        ptr::null_mut(),
                    ),
                    1 => dr_selection_quick(
                        doc,
                        source,
                        json(r#"{"seeds":[[0,0]]}"#).as_ptr(),
                        out.as_mut_ptr(),
                        token,
                        ptr::null_mut(),
                    ),
                    2 => dr_selection_refine(
                        source,
                        mask.as_ptr(),
                        ptr::null(),
                        json("{}").as_ptr(),
                        out.as_mut_ptr(),
                        token,
                        ptr::null_mut(),
                    ),
                    _ => dr_selection_transform(
                        mask.as_ptr(),
                        2,
                        1,
                        [1., 0., 0., 1., 0., 0.].as_ptr(),
                        1,
                        out.as_mut_ptr(),
                        token,
                        ptr::null_mut(),
                    ),
                };
                assert!(!ok);
                assert_eq!(out, [77; 2]);
                assert_eq!(dr_job_control_scratch_bytes(token), 0);
                dr_job_control_free(token);
            }
        }
        dr_doc_free(doc);
        dr_image_free(source);
    }
}

#[test]
fn cancelling_running_refinement_discards_partial_buffers() {
    let written = std::sync::Mutex::new(Vec::new());
    let render = |token: usize| unsafe {
        let source = dr_image_from_rgba(vec![255; 1536 * 1024 * 4].as_ptr(), 1536, 1024);
        let mask: Vec<u8> = (0..1536 * 1024).map(|i| (i % 251) as u8).collect();
        let mut out = vec![77; mask.len()];
        let mut err = ptr::null_mut();
        let ok = dr_selection_refine(
            source,
            mask.as_ptr(),
            ptr::null(),
            json(r#"{"smooth":64,"feather":64}"#).as_ptr(),
            out.as_mut_ptr(),
            token as *const _,
            &mut err,
        );
        dr_image_free(source);
        *written.lock().unwrap() = out;
        (
            ok,
            if ok {
                String::new()
            } else {
                take_err_string(err)
            },
        )
    };
    // k = 0: with no shift, the 0.2 report is followed directly by the 0.4
    // report's own check (no checkpoint between them).
    let (entry, _) = entry_then_cancel(&|progress, _, _| progress >= 0.2, render);
    let parked = cancelled_parked_at(
        entry.checkpoints + 1,
        &|_, stage| stage == entry.stage,
        render,
    );
    eprintln!(
        "selection refinement cancellation: {}",
        parked_line(&parked)
    );
    // Smooth has completed and changed the private working mask. The cancel
    // lands at the first checkpoint after smooth completes (the 0.4 report's
    // own check), not merely before allocation.
    assert!(parked.progress >= 0.2);
    assert!(parked.progress < 1.0);
    assert!(parked.scratch > 0);
    assert!(written.lock().unwrap().iter().all(|v| *v == 77));
}

#[test]
fn color_range_obeys_the_document_profile_instead_of_assuming_srgb() {
    use doctored_core::ffi_color::*;
    unsafe {
        let source = dr_image_from_rgba([200, 150, 120, 255].as_ptr(), 1, 1);
        let srgb = dr_doc_from_image(source);
        let mut profile = vec![0; dr_builtin_profile_len(1)];
        assert!(dr_builtin_profile(1, profile.as_mut_ptr(), profile.len()));
        let p3 = dr_doc_assign_profile(srgb, profile.as_ptr(), profile.len(), std::ptr::null_mut());
        assert!(!p3.is_null());
        let request = json(r#"{"mode":"tonal","lower":66.5,"upper":100,"softness":0}"#);
        let mut out = [77];
        assert!(dr_selection_color_range(
            srgb,
            source,
            request.as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [0]);
        assert!(dr_selection_color_range(
            p3,
            source,
            request.as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [255]);
        dr_doc_free(srgb);
        dr_doc_free(p3);
        dr_image_free(source);
    }
}

/// The edge stage runs FIRST: a refinement with an edge radius and a feather
/// is the feather (`dr_selection_feather`) of the edge-only result. The
/// estimate's own arithmetic is checked in `edge_refine.rs`.
#[test]
fn the_edge_stage_runs_before_the_other_four() {
    let (w, h) = (48_u32, 32_u32);
    let bytes: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let t = ((f64::from(i % w) - 20.5) / 6.0).clamp(0.0, 1.0);
            [
                (200.0 - 160.0 * t).round() as u8,
                (60.0 + 30.0 * t).round() as u8,
                (40.0 + 160.0 * t).round() as u8,
                255,
            ]
        })
        .collect();
    let mask: Vec<u8> = (0..w * h)
        .map(|i| if i % w < 24 { 255 } else { 0 })
        .collect();
    unsafe {
        let source = dr_image_from_rgba(bytes.as_ptr(), w, h);
        let mut edge_only = vec![77; mask.len()];
        assert!(dr_selection_refine(
            source,
            mask.as_ptr(),
            ptr::null(),
            json(r#"{"edge_radius":6}"#).as_ptr(),
            edge_only.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_ne!(edge_only, mask);
        let mut expected = edge_only.clone();
        assert!(dr_selection_feather(expected.as_mut_ptr(), w, h, 2.));
        let mut out = vec![77; mask.len()];
        assert!(dr_selection_refine(
            source,
            mask.as_ptr(),
            ptr::null(),
            json(r#"{"edge_radius":6,"feather":2}"#).as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, expected);
        dr_image_free(source);
    }
}

mod bench {
    use super::*;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancelling_running_refinement_discards_partial_buffers() {
        unsafe {
            let token = dr_job_control_create();
            let token_address = token as usize;
            let worker = std::thread::spawn(move || {
                let source = dr_image_from_rgba(vec![255; 1536 * 1024 * 4].as_ptr(), 1536, 1024);
                let mask: Vec<u8> = (0..1536 * 1024).map(|i| (i % 251) as u8).collect();
                let mut out = vec![77; mask.len()];
                let mut err = ptr::null_mut();
                let ok = dr_selection_refine(
                    source,
                    mask.as_ptr(),
                    ptr::null(),
                    json(r#"{"smooth":64,"feather":64}"#).as_ptr(),
                    out.as_mut_ptr(),
                    token_address as *const _,
                    &mut err,
                );
                dr_image_free(source);
                (ok, out, take_err_string(err))
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while dr_job_control_progress(token) < 0.2 && std::time::Instant::now() < deadline {
                std::thread::yield_now();
            }
            // Smooth has completed and changed the private working mask. Cancel
            // during the subsequent large feather pass, not merely before allocation.
            assert!(dr_job_control_progress(token) >= 0.2);
            assert!(dr_job_control_progress(token) < 1.0);
            assert!(dr_job_control_scratch_bytes(token) > 0);
            let start = std::time::Instant::now();
            dr_job_control_cancel(token);
            let (ok, out, error) = worker.join().unwrap();
            assert!(!ok);
            assert!(error.contains("cancelled"));
            assert!(out.iter().all(|v| *v == 77));
            assert!(start.elapsed() < std::time::Duration::from_millis(500));
            assert_eq!(dr_job_control_scratch_bytes(token), 0);
            dr_job_control_free(token);
        }
    }
}
