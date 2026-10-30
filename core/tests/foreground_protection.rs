//! Independent explicit footprint and observed-color support checks.
use doctored_core::{foreground_protection::protect, job_control::DrJobControl};
use image::{GrayImage, Luma, Rgba, RgbaImage};

#[test]
fn only_unknown_low_alpha_and_low_prior_pixels_can_change() {
    let source = RgbaImage::from_pixel(6, 1, Rgba([20, 30, 40, 255]));
    let estimated = RgbaImage::from_pixel(6, 1, Rgba([200, 210, 220, 255]));
    let alpha = GrayImage::from_raw(6, 1, vec![64, 128, 0, 64, 64, 64]).unwrap();
    let prior = GrayImage::from_raw(6, 1, vec![0, 0, 0, 128, 0, 0]).unwrap();
    let result = protect(
        &source,
        &estimated,
        &alpha,
        &prior,
        &[128, 128, 128, 128, 0, 255],
        None,
    )
    .unwrap();
    // The explicit foreground at x5 provides identical observed-color support.
    assert!(result.image.is_none());
    assert_eq!(result.eligible_pixels, 1);
    assert_eq!(result.protected_by_source_color, 1);
    let user = [128, 128, 128, 128, 0, 0];
    let output = protect(&source, &estimated, &alpha, &prior, &user, None)
        .unwrap()
        .image
        .unwrap();
    assert_eq!(output.get_pixel(0, 0), estimated.get_pixel(0, 0));
    for x in 1..6 {
        assert_eq!(output.get_pixel(x, 0), source.get_pixel(x, 0));
    }
}

#[test]
fn observed_foreground_support_uses_inclusive_code_tolerance_and_user_override() {
    for (difference, protected) in [(8, true), (9, false)] {
        let mut source = RgbaImage::from_pixel(2, 1, Rgba([50, 60, 70, 255]));
        source.put_pixel(1, 0, Rgba([50 + difference, 60, 70, 255]));
        let estimated = RgbaImage::from_pixel(2, 1, Rgba([200, 210, 220, 255]));
        let alpha = GrayImage::from_raw(2, 1, vec![64, 255]).unwrap();
        let prior = GrayImage::from_raw(2, 1, vec![0, 247]).unwrap();
        let result = protect(&source, &estimated, &alpha, &prior, &[128, 128], None).unwrap();
        assert_eq!(result.image.is_none(), protected);
        let result = protect(&source, &estimated, &alpha, &prior, &[128, 0], None).unwrap();
        assert!(result.image.is_some()); // Explicit BG cannot support foreground.
        let prior = GrayImage::from_pixel(2, 1, Luma([0]));
        let result = protect(&source, &estimated, &alpha, &prior, &[128, 255], None).unwrap();
        assert_eq!(result.image.is_none(), protected);
    }
}

#[test]
fn malformed_work_and_cancelled_inputs_refuse_without_mutation() {
    let source = RgbaImage::from_pixel(500, 500, Rgba([20, 30, 40, 255]));
    let estimated = RgbaImage::from_pixel(500, 500, Rgba([200, 210, 220, 255]));
    let alpha = GrayImage::from_pixel(500, 500, Luma([64]));
    let prior = GrayImage::new(500, 500);
    let labels = vec![128; 250000];
    let error = protect(&source, &estimated, &alpha, &prior, &labels, None)
        .err()
        .unwrap();
    assert!(error.contains("local-sample budget"));
    assert!(protect(&source, &estimated, &alpha, &prior, &[], None).is_err());
    let control = DrJobControl::default();
    control.cancel();
    assert!(protect(&source, &estimated, &alpha, &prior, &labels, Some(&control)).is_err());
    assert_eq!(control.scratch_bytes(), 0);
    assert_eq!(source.get_pixel(0, 0), &Rgba([20, 30, 40, 255]));
}
