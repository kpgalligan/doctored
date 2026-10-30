//! Counterexamples for coupled alpha/foreground confidence. These retain the
//! strict reference-opaque color gate: a consistent compositing fit is not
//! evidence that a fractional alpha or an RGB change is physically correct.
use crate::matting_quality as quality;
use doctored_core::{
    ffi::*,
    ffi_doc::*,
    ffi_foreground::dr_image_estimate_foreground,
    sampled_matting::{estimate, MattingParams},
};
use image::{GrayImage, Luma, RgbaImage};
use std::{ffi::CString, ptr};

fn ramp() -> (RgbaImage, GrayImage, Vec<u8>) {
    let alpha = GrayImage::from_fn(97, 9, |x, _| {
        Luma([((f64::from(x) / 96.0 - 0.25) * 510.0)
            .round()
            .clamp(0.0, 255.0) as u8])
    });
    // Exact byte-domain composition of white F and black B: RGB equals alpha.
    let source = RgbaImage::from_fn(97, 9, |x, y| {
        let a = alpha.get_pixel(x, y)[0];
        image::Rgba([a, a, a, 255])
    });
    let trimap = alpha
        .as_raw()
        .iter()
        .map(|&a| if a == 0 || a == 255 { a } else { 128 })
        .collect();
    (source, alpha, trimap)
}

#[test]
fn identical_composites_and_valid_constraints_admit_conflicting_foreground_truths() {
    let (source, prior, trimap) = ramp();
    let colors = quality::colors(&source);
    let sampled = estimate(
        &colors,
        97,
        9,
        &trimap,
        prior.as_raw(),
        &MattingParams::default(),
        None,
    )
    .unwrap();
    let i = 4 * 97 + 48;
    let observed = f32::from(source.as_raw()[4 * i]) / 255.0;
    let a = f32::from(prior.as_raw()[i]) / 255.0;
    assert!((observed - a).abs() < f32::EPSILON);
    assert_eq!(trimap[i], 128);
    // World A: an opaque gray mark with F=C. World B: translucent white,
    // alpha=C and B=0. Both have the same observed RGB. At every *known*
    // trimap pixel the two worlds agree, so neither violates user constraints.
    let mut opaque_world_alpha = prior.clone();
    opaque_world_alpha.as_mut()[i] = 255;
    let mut opaque_world_f = vec![[1.0; 3]; colors.len()];
    opaque_world_f[i] = [observed; 3];
    for j in 0..colors.len() {
        let a1 = f32::from(opaque_world_alpha.as_raw()[j]) / 255.0;
        let a2 = f32::from(prior.as_raw()[j]) / 255.0;
        for c in 0..3 {
            assert!((a1 * opaque_world_f[j][c] - colors[j][c]).abs() < 1e-6);
            assert!((a2 - colors[j][c]).abs() < 1e-6);
        }
        if trimap[j] != 128 {
            assert_eq!(opaque_world_alpha.as_raw()[j], prior.as_raw()[j]);
        }
    }
    assert_eq!(sampled.alpha.as_raw()[i], prior.as_raw()[i]);
    assert_eq!(sampled.confidence.as_raw()[i], 255);
    assert_eq!(sampled.foreground[i], [1.0; 3]);
    // A perfect sample fit, even with correct known constraints, cannot choose
    // between these worlds. An opaque-preserving RGB equals C exactly; the
    // other world's foreground differs by ~0.5, well beyond the fixed0.05 gate.
    assert!(1.0 - observed > 0.49);
}

#[test]
fn foreground_color_stage_with_estimated_alpha_does_not_certify_opaque_preservation() {
    let (source, alpha, _trimap) = ramp();
    unsafe {
        let image = dr_image_from_rgba(source.as_raw().as_ptr(), 97, 9);
        let doc = dr_doc_from_image(image);
        let params = CString::new(r#"{"color_space":"encoded"}"#).unwrap();
        let mut error = ptr::null_mut();
        let result = dr_image_estimate_foreground(
            doc,
            image,
            alpha.as_ptr(),
            params.as_ptr(),
            ptr::null(),
            &mut error,
        );
        assert!(error.is_null());
        assert!(!result.is_null());
        let pixels =
            std::slice::from_raw_parts(dr_image_pixels_rgba(result), source.as_raw().len())
                .to_vec();
        assert_eq!(
            std::slice::from_raw_parts(dr_image_pixels_rgba(image), source.as_raw().len()),
            source.as_raw()
        );
        let output = RgbaImage::from_raw(97, 9, pixels).unwrap();
        let estimated = quality::colors(&output);
        let correct_f = vec![[1.0; 3]; alpha.as_raw().len()];
        let translucent =
            quality::color_metrics(&alpha, &alpha, &correct_f, &estimated, &source).unwrap();
        assert!(translucent["fractional_foreground_mad"].as_f64().unwrap() < 0.05);
        let i = 4 * 97 + 48;
        let mut opaque_world = alpha.clone();
        opaque_world.as_mut()[i] = 255;
        let mut opaque_f = correct_f;
        opaque_f[i] = quality::colors(&source)[i];
        let before = quality::color_metrics(
            &opaque_world,
            &alpha,
            &opaque_f,
            &quality::colors(&source),
            &source,
        )
        .unwrap();
        let after =
            quality::color_metrics(&opaque_world, &alpha, &opaque_f, &estimated, &source).unwrap();
        assert_eq!(after["opaque_changed_pixels"], 1);
        assert!(!quality::color_gate(&before, &after));
        // The same function is useful with correct alpha and still fails the
        // coupled requirement with a wrong fractional alpha at an opaque pixel.
        dr_image_free(result);
        dr_doc_free(doc);
        dr_image_free(image);
    }
}

#[test]
fn exact_reconstruction_and_gamut_feasibility_do_not_identify_foreground_color() {
    let (observed, alpha, true_f, true_b) = (0.5_f64, 0.5, 0.8_f64, 0.2_f64);
    assert_eq!(observed, alpha * true_f + (1.0 - alpha) * true_b);
    // Two equally feasible pairs explain exactly the same observed pixel and
    // even the same correct alpha. A least-change projection of (.8,.8) onto
    // the compositing line chooses (.5,.5), damaging an initially correct F.
    let initial = [0.8_f64, 0.8_f64];
    let residual = observed - alpha * initial[0] - (1.0 - alpha) * initial[1];
    let denominator = alpha * alpha + (1.0 - alpha) * (1.0 - alpha);
    let projected = [
        initial[0] + alpha * residual / denominator,
        initial[1] + (1.0 - alpha) * residual / denominator,
    ];
    assert!((observed - alpha * projected[0] - (1.0 - alpha) * projected[1]).abs() < 1e-12);
    assert!(projected.iter().all(|v| (0.0..=1.0).contains(v)));
    assert!((projected[0] - true_f).abs() > 0.29);
    assert_eq!(initial[0], true_f);
}
