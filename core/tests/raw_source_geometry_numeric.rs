use crate::raw_geometry_fixture as fixture;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use fixture::{codes, image, mask, LIMIT};

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
fn exact_exif_all_eight_non_square_and_hidden_rgb() {
    let mut count = 0;
    for (w, h) in [(7, 5), (5, 7), (1, 7), (7, 1)] {
        let old = codes(w, h);
        let mask = mask(w, h);
        let image = image(w, h);
        let pixels = image.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        for orientation in 1..=8 {
            let request =
                format!("{{\"version\":1,\"kind\":\"exif\",\"orientation\":{orientation}}}");
            let transformed = mask.remap(&request, 1, LIMIT, 0, None).unwrap();
            let transformed_image = image.raw_source_geometry(&request, LIMIT, 0, None).unwrap();
            if orientation == 1 {
                assert!(transformed.is_none());
                assert!(transformed_image.is_none());
                continue;
            }
            let transformed = transformed.unwrap();
            let transformed_image = transformed_image.unwrap();
            let (dw, dh) = transformed.dimensions();
            assert_eq!((dw, dh), if orientation >= 5 { (h, w) } else { (w, h) });
            assert_eq!(transformed_image.profile().bytes(), image.profile().bytes());
            let output = transformed_image
                .layer_source(0)
                .unwrap()
                .to_rgba16()
                .ok()
                .unwrap();
            for y in 0..dh {
                for x in 0..dw {
                    let (sx, sy) = expected_exif(orientation, w, h, x, y);
                    let before = (sy * w + sx) as usize;
                    let after = (y * dw + x) as usize;
                    assert_eq!(transformed.sample(after), Some(old[before]));
                    assert_eq!(output[after], pixels[before]);
                    count += 1;
                }
            }
        }
    }
    println!("exact-exif codes_compared={count} max_mask_error=0 max_rgba_error=0");
}

#[test]
fn integer_crop_pad_and_noop() {
    let mask = mask(7, 5);
    let old = codes(7, 5);
    for (dw, dh, tx, ty) in [(5, 3, 1, 1), (9, 7, -1, -1), (7, 5, 0, 0), (8, 4, -2, 2)] {
        let request = format!("{{\"version\":1,\"kind\":\"affine\",\"width\":{dw},\"height\":{dh},\"inverse\":[1,0,0,1,{tx},{ty}],\"filter\":\"bicubic\"}}");
        let changed = mask.remap(&request, 1, LIMIT, 0, None).unwrap();
        if tx == 0 && ty == 0 && dw == 7 && dh == 5 {
            assert!(changed.is_none());
            continue;
        }
        let changed = changed.unwrap();
        for y in 0..dh {
            for x in 0..dw {
                let sx = i64::from(x) + tx;
                let sy = i64::from(y) + ty;
                let expected = if (0..7).contains(&sx) && (0..5).contains(&sy) {
                    old[sy as usize * 7 + sx as usize]
                } else {
                    0
                };
                assert_eq!(changed.sample((y * dw + x) as usize), Some(expected));
            }
        }
    }
}

fn weight(delta: f64, bicubic: bool) -> f64 {
    let x = delta.abs();
    if !bicubic {
        return (1.0 - x).max(0.0);
    }
    if x < 1.0 {
        ((1.5 * x - 2.5) * x) * x + 1.0
    } else if x < 2.0 {
        ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
    } else {
        0.0
    }
}
fn taps(p: f64, bicubic: bool) -> Vec<(i64, f64)> {
    let base = (p - 0.5).floor() as i64;
    let lead = if bicubic { 1 } else { 0 };
    let mut result: Vec<_> = (0..if bicubic { 4 } else { 2 })
        .map(|i| {
            let at = base + i - lead;
            (at, weight(p - 0.5 - at as f64, bicubic))
        })
        .collect();
    let total: f64 = result.iter().map(|(_, w)| *w).sum();
    if total != 0.0 && total != 1.0 {
        for (_, w) in &mut result {
            *w /= total;
        }
    }
    result
}
fn scalar_reference(source: &[u16], width: u32, height: u32, p: [f64; 2], bicubic: bool) -> u16 {
    let mut result = 0.0;
    for (y, wy) in taps(p[1], bicubic) {
        for (x, wx) in taps(p[0], bicubic) {
            if x >= 0 && y >= 0 && x < i64::from(width) && y < i64::from(height) {
                result += f64::from(source[y as usize * width as usize + x as usize]) * wx * wy;
            }
        }
    }
    result.clamp(0.0, 65535.0).round() as u16
}
fn image_reference(
    source: &[[u16; 4]],
    width: u32,
    height: u32,
    positions: [[f64; 2]; 3],
    bicubic: bool,
) -> [u16; 4] {
    let mut channels = [[0.0; 2]; 3];
    for channel in 0..3 {
        let p = positions[channel];
        for (y, wy) in taps(p[1], bicubic) {
            for (x, wx) in taps(p[0], bicubic) {
                if x >= 0 && y >= 0 && x < i64::from(width) && y < i64::from(height) {
                    let sample = source[y as usize * width as usize + x as usize];
                    let a = f64::from(sample[3]) / 65535.0;
                    channels[channel][0] += f64::from(sample[channel]) / 65535.0 * a * wx * wy;
                    channels[channel][1] += a * wx * wy;
                }
            }
        }
    }
    let alpha = (channels[1][1].clamp(0.0, 1.0) * 65535.0).round() as u16;
    let mut result = [0; 4];
    result[3] = alpha;
    if alpha > 0 {
        for c in 0..3 {
            if channels[c][1] > 0.0 {
                result[c] =
                    ((channels[c][0] / channels[c][1]).clamp(0.0, 1.0) * 65535.0).round() as u16;
            }
        }
    }
    result
}
fn affine(m: [f64; 6], x: f64, y: f64) -> [f64; 2] {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}
fn lens(
    x: f64,
    y: f64,
    channel: usize,
    radial: [f64; 3],
    scales: [f64; 3],
    center: [f64; 2],
    dimensions: (u32, u32),
) -> [f64; 2] {
    let r = f64::from(dimensions.0.max(dimensions.1)) / 2.0;
    let c = [
        center[0] * f64::from(dimensions.0),
        center[1] * f64::from(dimensions.1),
    ];
    let q = [(x - c[0]) / r, (y - c[1]) / r];
    let r2 = q[0] * q[0] + q[1] * q[1];
    let d = 1.0 + radial[0] * r2 + radial[1] * r2 * r2 + radial[2] * r2 * r2 * r2;
    [
        c[0] + r * q[0] * d * scales[channel],
        c[1] + r * q[1] * d * scales[channel],
    ]
}
fn compare(
    request: &str,
    map: impl Fn(f64, f64, usize) -> [f64; 2],
    bicubic: bool,
    width: u32,
    height: u32,
    channel: u32,
) -> (usize, u16) {
    let mask = mask(width, height);
    let source = codes(width, height);
    let image = image(width, height);
    let pixels = image.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    let output_mask = mask
        .remap(request, channel, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let output_image = image
        .raw_source_geometry(request, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let output_pixels = output_image
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap();
    let (dw, dh) = output_mask.dimensions();
    let mut max_error = 0;
    let mut nonvacuity = 0;
    for y in 0..dh {
        for x in 0..dw {
            let index = (y * dw + x) as usize;
            let p = map(f64::from(x) + 0.5, f64::from(y) + 0.5, channel as usize);
            let expected = scalar_reference(&source, width, height, p, bicubic);
            let actual = output_mask.sample(index).unwrap();
            max_error = max_error.max(expected.abs_diff(actual));
            if x < width && y < height {
                nonvacuity = nonvacuity.max(source[(y * width + x) as usize].abs_diff(actual));
            }
            let expected_rgba = image_reference(
                &pixels,
                width,
                height,
                [
                    map(f64::from(x) + 0.5, f64::from(y) + 0.5, 0),
                    map(f64::from(x) + 0.5, f64::from(y) + 0.5, 1),
                    map(f64::from(x) + 0.5, f64::from(y) + 0.5, 2),
                ],
                bicubic,
            );
            for c in 0..4 {
                assert!(
                    expected_rgba[c].abs_diff(output_pixels[index][c]) <= 1,
                    "RGBA c={c} x={x} y={y}: expected={expected_rgba:?} actual={:?}",
                    output_pixels[index]
                );
            }
        }
    }
    assert!(max_error <= 1, "mask max error {max_error}");
    (dw as usize * dh as usize, nonvacuity)
}

#[test]
fn fractional_affine_and_lens_independent_f64() {
    let (w, h) = (23, 17);
    let mut count = 0;
    let mut difference = 0;
    for (matrix, bicubic) in [
        ([1.0, 0.0, 0.0, 1.0, 0.27, -0.38], false),
        ([0.96, 0.12, -0.13, 1.03, 1.1, -0.7], true),
        ([1.2, -0.08, 0.23, 0.82, -0.4, 0.5], false),
    ] {
        let request = serde_json::json!({"version":1,"kind":"affine","width":w,"height":h,
            "inverse":matrix,"filter":if bicubic {"bicubic"} else {"bilinear"}})
        .to_string();
        let (n, d) = compare(&request, |x, y, _| affine(matrix, x, y), bicubic, w, h, 1);
        count += n;
        difference = difference.max(d);
    }
    for (radial, scales, bicubic) in [
        ([0.04, 0.0, 0.0], [1., 1., 1.], false),
        ([-0.03, 0.002, 0.0], [1., 1., 1.], true),
        ([0.0; 3], [0.93, 1., 1.07], false),
        ([0.01, -0.002, 0.0002], [0.96, 1., 1.04], true),
    ] {
        let center = [0.31, 0.67];
        let request = serde_json::json!({"version":1,"kind":"lens","profile_id":"independent-f64",
            "center":center,"radial":radial,"red_scale":scales[0],"blue_scale":scales[2],
            "filter":if bicubic {"bicubic"} else {"bilinear"}})
        .to_string();
        let (n, d) = compare(
            &request,
            |x, y, c| lens(x, y, c, radial, scales, center, (w, h)),
            bicubic,
            w,
            h,
            1,
        );
        count += n;
        difference = difference.max(d);
    }
    assert!(
        difference >= 128,
        "filtered cases were vacuous: {difference}"
    );
    println!(
        "filtered-f64 compared={count} max_mask_error=1 max_rgba_error=1 nonvacuity={difference}"
    );
}

#[test]
fn tca_alone_keeps_opaque_green_and_alpha_exact() {
    let (w, h) = (23, 17);
    let center = [0.31, 0.67];
    let scales = [0.93, 1.0, 1.07];
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let x = i % w;
            let y = i / w;
            [
                ((x * 2729 + y * 131) % 65536) as u16,
                ((x * 97 + y * 3149) % 65536) as u16,
                (65535 - (x * 881 + y * 1979) % 65536) as u16,
                65535,
            ]
        })
        .collect();
    let source = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels.clone(), IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let canonical = mask(w, h);
    let samples = codes(w, h);
    let mut compared = 0;
    let mut displaced = [0u16; 2];
    for bicubic in [false, true] {
        let request = serde_json::json!({"version":1,"kind":"lens","profile_id":"opaque-tca-only",
            "center":center,"radial":[0,0,0],"red_scale":scales[0],
            "blue_scale":scales[2],"filter":if bicubic {"bicubic"} else {"bilinear"}})
        .to_string();
        if let Some(green) = canonical.remap(&request, 1, LIMIT, 0, None).unwrap() {
            for index in 0..samples.len() {
                assert_eq!(green.sample(index), canonical.sample(index));
            }
        }
        let red = canonical
            .remap(&request, 0, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let blue = canonical
            .remap(&request, 2, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let developed = source
            .raw_source_geometry(&request, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(developed.profile().bytes(), source.profile().bytes());
        let actual = developed.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        for y in 0..h {
            for x in 0..w {
                let index = (y * w + x) as usize;
                let at = [f64::from(x) + 0.5, f64::from(y) + 0.5];
                let positions =
                    [0, 1, 2].map(|c| lens(at[0], at[1], c, [0.0; 3], scales, center, (w, h)));
                let expected = image_reference(&pixels, w, h, positions, bicubic);
                assert_eq!(actual[index][1], pixels[index][1]);
                assert_eq!(actual[index][3], 65535);
                for channel in [0, 2] {
                    assert!(actual[index][channel].abs_diff(expected[channel]) <= 1);
                }
                for (slot, (channel, plane)) in [(0, &red), (2, &blue)].into_iter().enumerate() {
                    let expected_mask =
                        scalar_reference(&samples, w, h, positions[channel], bicubic);
                    let value = plane.sample(index).unwrap();
                    assert!(value.abs_diff(expected_mask) <= 1);
                    displaced[slot] =
                        displaced[slot].max(value.abs_diff(canonical.sample(index).unwrap()));
                }
                compared += 1;
            }
        }
    }
    assert!(
        displaced[0] >= 128 && displaced[1] >= 128,
        "red/blue displacement {displaced:?}"
    );
    println!("opaque-tca compared={compared} green_alpha_exact=true red_blue_displacement={displaced:?} max_error=1");
}
