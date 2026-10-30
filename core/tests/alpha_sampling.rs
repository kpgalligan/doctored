//! Independent transparent-edge references for the public resize/blur paths.
//! Oracles integrate 2-D kernels in f64 directly and quantize only at the
//! end, independently of the core's separable f32 implementation.

use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};

use crate::common;
use common::*;

fn image_handle(src: &RgbaImage) -> *mut DrImage {
    let out = unsafe { dr_image_from_rgba(src.as_ptr(), src.width(), src.height()) };
    assert!(!out.is_null());
    out
}

/// Uncontrolled Gaussian Blur, `dr_image_filter_op` `blur`: NULL on a refusal.
fn byte_blur(img: *const DrImage, sigma: f32) -> *mut DrImage {
    let op = std::ffi::CString::new("blur").unwrap();
    let params = std::ffi::CString::new(format!(r#"{{"sigma":{}}}"#, f64::from(sigma))).unwrap();
    unsafe {
        doctored_core::ffi_filters::dr_image_filter_op(
            img,
            op.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    }
}

fn fixture(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let alpha = [0, 1, 37, 119, 201, 255][((x * 7 + y * 3) % 6) as usize];
        Rgba([
            ((x * 73 + y * 41) % 256) as u8,
            ((x * 23 + y * 79) % 256) as u8,
            ((x * 61 + y * 13) % 256) as u8,
            alpha,
        ])
    })
}

fn finish(sum: [f64; 4], weight: f64) -> [u8; 4] {
    let alpha = (sum[3] / weight).clamp(0.0, 255.0).round() as u8;
    if alpha == 0 {
        return [0; 4];
    }
    [
        (sum[0] / sum[3]).clamp(0.0, 255.0).round() as u8,
        (sum[1] / sum[3]).clamp(0.0, 255.0).round() as u8,
        (sum[2] / sum[3]).clamp(0.0, 255.0).round() as u8,
        alpha,
    ]
}

fn add(sum: &mut [f64; 4], pixel: Rgba<u8>, weight: f64) {
    let alpha_weight = f64::from(pixel[3]) * weight;
    for c in 0..3 {
        sum[c] += f64::from(pixel[c]) * alpha_weight;
    }
    sum[3] += alpha_weight;
}

fn weight(t: f64, filter: i32) -> f64 {
    let x = t.abs();
    match filter {
        FILTER_BILINEAR => (1.0 - x).max(0.0),
        FILTER_CATMULL_ROM if x < 1.0 => 1.5 * x.powi(3) - 2.5 * x.powi(2) + 1.0,
        FILTER_CATMULL_ROM if x < 2.0 => -0.5 * x.powi(3) + 2.5 * x.powi(2) - 4.0 * x + 2.0,
        FILTER_CATMULL_ROM => 0.0,
        FILTER_LANCZOS3 if x == 0.0 => 1.0,
        FILTER_LANCZOS3 if x < 3.0 => {
            let angle = std::f64::consts::PI * x;
            (angle.sin() / angle) * ((angle / 3.0).sin() / (angle / 3.0))
        }
        FILTER_LANCZOS3 => 0.0,
        _ => unreachable!(),
    }
}

fn resize_reference(src: &RgbaImage, w: u32, h: u32, filter: i32) -> RgbaImage {
    let rx = f64::from(src.width()) / f64::from(w);
    let ry = f64::from(src.height()) / f64::from(h);
    RgbaImage::from_fn(w, h, |x, y| {
        let cx = (f64::from(x) + 0.5) * rx - 0.5;
        let cy = (f64::from(y) + 0.5) * ry - 0.5;
        let mut sum = [0.0; 4];
        let mut total = 0.0;
        for (sx, sy, pixel) in src.enumerate_pixels() {
            let k = weight((f64::from(sx) - cx) / rx.max(1.0), filter)
                * weight((f64::from(sy) - cy) / ry.max(1.0), filter);
            add(&mut sum, *pixel, k);
            total += k;
        }
        Rgba(finish(sum, total))
    })
}

/// A direct 2-D Gaussian with repeated border pixels. Explicit support
/// fixtures: sigma .8 uses 3 taps, 1.5 uses 7, matching the public blur's
/// established neighborhood. Normalization is calculated independently.
fn blur_reference(src: &RgbaImage, sigma: f64, radius: i32) -> RgbaImage {
    RgbaImage::from_fn(src.width(), src.height(), |x, y| {
        let mut sum = [0.0; 4];
        let mut total = 0.0;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let k = (-f64::from(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp();
                let sx = (x as i32 + dx).clamp(0, src.width() as i32 - 1) as u32;
                let sy = (y as i32 + dy).clamp(0, src.height() as i32 - 1) as u32;
                add(&mut sum, *src.get_pixel(sx, sy), k);
                total += k;
            }
        }
        Rgba(finish(sum, total))
    })
}

fn assert_reference(got: *const DrImage, want: &RgbaImage, context: &str) {
    assert!(!got.is_null(), "{context}");
    assert_eq!(img_dims(got), want.dimensions());
    for (i, (g, w)) in img_pixels(got).iter().zip(want.as_raw()).enumerate() {
        assert!(
            (i16::from(*g) - i16::from(*w)).abs() <= 1,
            "{context}, byte {i}: got {g}, expected {w}"
        );
    }
}

#[test]
fn resize_matches_independent_premultiplied_reference_at_transparent_edges() {
    let src = fixture(9, 7);
    let handle = image_handle(&src);
    for filter in [FILTER_BILINEAR, FILTER_CATMULL_ROM, FILTER_LANCZOS3] {
        for (w, h) in [(17, 13), (4, 3), (13, 4), (1, 1)] {
            let got = unsafe { dr_image_resize(handle, w, h, filter) };
            assert_reference(got, &resize_reference(&src, w, h, filter), "resize");
            unsafe { dr_image_free(got) };
        }
    }
    assert_eq!(img_pixels(handle), *src.as_raw(), "source mutated");
    unsafe { dr_image_free(handle) };
}

#[test]
fn gaussian_blur_matches_independent_reference_and_keeps_source_immutable() {
    let src = fixture(9, 7);
    let handle = image_handle(&src);
    for (sigma, radius) in [(0.8, 1), (1.5, 3)] {
        let got = byte_blur(handle, sigma as f32);
        assert_reference(got, &blur_reference(&src, sigma, radius), "blur");
        unsafe { dr_image_free(got) };
    }
    assert_eq!(img_pixels(handle), *src.as_raw(), "source mutated");
    unsafe { dr_image_free(handle) };
}

#[test]
fn saturated_hidden_colors_cannot_tint_resize_blur_or_sharpen() {
    let src = RgbaImage::from_fn(16, 9, |x, y| {
        if (4..12).contains(&x) && (2..7).contains(&y) {
            Rgba([91, 173, 227, 255])
        } else {
            Rgba([255, 0, 13, 0])
        }
    });
    let handle = image_handle(&src);
    let results = unsafe {
        [
            dr_image_resize(handle, 31, 19, FILTER_BILINEAR),
            dr_image_resize(handle, 31, 19, FILTER_CATMULL_ROM),
            dr_image_resize(handle, 31, 19, FILTER_LANCZOS3),
            byte_blur(handle, 1.5),
            dr_image_sharpen(handle, 1.5),
        ]
    };
    for got in results {
        assert!(!got.is_null());
        let mut visible = 0;
        for pixel in img_pixels(got).chunks_exact(4) {
            if pixel[3] == 0 {
                assert_eq!(pixel, [0, 0, 0, 0]);
            } else {
                visible += 1;
                assert_eq!(&pixel[..3], [91, 173, 227]);
            }
        }
        assert!(visible > 0);
        unsafe { dr_image_free(got) };
    }
    unsafe { dr_image_free(handle) };
}

#[test]
fn document_resize_and_layer_thumbnail_use_the_same_alpha_boundary() {
    let src = fixture(9, 7);
    let image = image_handle(&src);
    let doc = unsafe { dr_doc_from_image(image) };
    let resized = unsafe { dr_doc_resize(doc, 4, 3, FILTER_BILINEAR) };
    let layer = unsafe { dr_doc_layer_image(resized, 0) };
    assert_reference(
        layer,
        &resize_reference(&src, 4, 3, FILTER_BILINEAR),
        "document resize",
    );
    let thumbnail = unsafe { dr_doc_layer_thumbnail(doc, 0, 4) };
    assert_reference(
        thumbnail,
        &resize_reference(&src, 4, 3, FILTER_BILINEAR),
        "thumbnail",
    );
    unsafe {
        dr_image_free(layer);
        dr_image_free(thumbnail);
        dr_doc_free(resized);
        dr_doc_free(doc);
        dr_image_free(image);
    }
}

#[test]
fn nearest_identity_and_encoded_color_policy_are_preserved() {
    let src = RgbaImage::from_raw(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]).unwrap();
    let handle = image_handle(&src);
    let got = unsafe { dr_image_resize(handle, 3, 1, FILTER_BILINEAR) };
    assert_eq!(&img_pixels(got)[4..8], [128, 128, 128, 255]);
    unsafe {
        dr_image_free(got);
        dr_image_free(handle);
    }

    let transparent = RgbaImage::from_raw(2, 1, vec![255, 13, 177, 0, 40, 50, 60, 1]).unwrap();
    let handle = image_handle(&transparent);
    let nearest = unsafe { dr_image_resize(handle, 4, 1, FILTER_NEAREST) };
    assert_eq!(
        img_pixels(nearest),
        vec![255, 13, 177, 0, 255, 13, 177, 0, 40, 50, 60, 1, 40, 50, 60, 1]
    );
    for filter in [FILTER_BILINEAR, FILTER_CATMULL_ROM, FILTER_LANCZOS3] {
        let got = unsafe { dr_image_resize(handle, 2, 1, filter) };
        assert_eq!(img_pixels(got), *transparent.as_raw());
        unsafe { dr_image_free(got) };
    }
    unsafe {
        dr_image_free(nearest);
        dr_image_free(handle);
    }
}
