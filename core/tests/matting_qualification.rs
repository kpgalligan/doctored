//! Independent hand-computed checks for qualification metrics, not kernel tests.
use crate::matting_quality as quality;
use image::{GrayImage, RgbaImage};

#[test]
fn foreground_and_recomposition_oracles_distinguish_alpha_from_color() {
    let truth = GrayImage::from_raw(2, 1, vec![128, 255]).unwrap();
    let foreground = vec![[0.2_f32; 3]; 2];
    let source = RgbaImage::from_raw(2, 1, vec![153, 153, 153, 255, 51, 51, 51, 255]).unwrap();
    let exact = quality::color_metrics(&truth, &truth, &foreground, &foreground, &source).unwrap();
    assert_eq!(exact["fractional_foreground_mad"], 0.0);
    assert_eq!(exact["recomposite"]["white"]["mad"], 0.0);
    assert_eq!(exact["opaque_changed_max_code_delta"], 0);
    assert_eq!(exact["opaque_changed_outside_boundary"], 0);
    let contaminated = vec![[0.6_f32; 3], [0.2; 3]];
    let before =
        quality::color_metrics(&truth, &truth, &foreground, &contaminated, &source).unwrap();
    assert!((before["fractional_foreground_mad"].as_f64().unwrap() - 0.4).abs() < 1e-6);
    assert!(
        (before["recomposite"]["dark"]["mad"].as_f64().unwrap() - 0.4 * 128.0 / 255.0 / 2.0).abs()
            < 1e-6
    );
    assert!(
        !quality::color_gate(&before, &before),
        "identity must not qualify visible fringes"
    );
    assert!(quality::color_gate(&before, &exact));
    let wrong_alpha = GrayImage::from_raw(2, 1, vec![0, 255]).unwrap();
    let wrong =
        quality::color_metrics(&truth, &wrong_alpha, &foreground, &foreground, &source).unwrap();
    assert_eq!(wrong["fractional_foreground_mad"], 0.0);
    assert!(
        wrong["recomposite"]["white"]["boundary_mad"]
            .as_f64()
            .unwrap()
            > 0.1
    );
    assert!(!quality::color_gate(&before, &wrong));
    let changed =
        quality::color_metrics(&truth, &truth, &foreground, &[[0.2; 3], [0.3; 3]], &source)
            .unwrap();
    assert_eq!(changed["opaque_changed_pixels"], 1);
    assert_eq!(changed["opaque_changed_max_code_delta"], 26);
    assert_eq!(changed["opaque_changed_outside_boundary"], 0);
}

#[test]
fn metrics_reject_invalid_dimensions_and_colors() {
    let a = GrayImage::from_pixel(1, 1, image::Luma([128]));
    let source = RgbaImage::new(1, 1);
    for values in [vec![], vec![[f32::NAN; 3]], vec![[1.1; 3]]] {
        assert!(quality::color_metrics(&a, &a, &[[0.0; 3]], &values, &source).is_err());
    }
    assert!(
        quality::color_metrics(&a, &GrayImage::new(2, 1), &[[0.0; 3]], &[[0.0; 3]], &source)
            .is_err()
    );
    assert!(quality::color_metrics(
        &GrayImage::new(0, 0),
        &GrayImage::new(0, 0),
        &[],
        &[],
        &RgbaImage::new(0, 0)
    )
    .is_err());
}

#[test]
fn trimap_is_prior_only_and_does_not_invent_opaque_glass() {
    let prior = GrayImage::from_fn(25, 1, |x, _| {
        image::Luma([if x < 10 {
            0
        } else if x < 15 {
            128
        } else {
            255
        }])
    });
    let trimap = quality::trimap_from_prior(&prior);
    assert_eq!(
        trimap.into_raw(),
        [vec![0; 7], vec![128; 11], vec![255; 7]].concat()
    );
    let glass = GrayImage::from_pixel(15, 15, image::Luma([170]));
    assert!(quality::trimap_from_prior(&glass)
        .pixels()
        .all(|p| p[0] == 128));
}
