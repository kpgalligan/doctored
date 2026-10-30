//! Numerical qualification of shared adjustment preparation. Achromatic
//! special cases give independent scalar references for all 21 operations;
//! severe curves and spatial guides additionally exercise true u16 samples.

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

const FLOAT_BOUND: f64 = 2e-6;
/// Photo Filter multiplies in the profile's D50 PCS, and the builtin
/// sRGB tag matrix maps (1, 1, 1) to D50 only to s15Fixed16 tag precision,
/// so a grey filter is not exactly neutral there: its own bound, declared
/// before measuring.
const PHOTO_FILTER_BOUND: f64 = 2e-4;
fn srgb_decode(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb_encode(linear: f64) -> f64 {
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}
fn description(name: &str, params: Value) -> String {
    json!({"type":"adjust", "evaluation":"precision_v1", "op":name, "params":params}).to_string()
}
fn prepared(name: &str, params: Value) -> PrecisionAdjustment {
    PrecisionAdjustment::parse(&description(name, params), &IccProfile::srgb()).unwrap()
}
fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for digit in 0..4 {
            encoded.push(if digit > chunk.len() {
                '='
            } else {
                DIGITS[((value >> (18 - 6 * digit)) & 63) as usize] as char
            });
        }
    }
    encoded
}
fn lut(values: &[f32]) -> Value {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    json!({"kind":"1d", "size":2, "table":base64(&bytes)})
}

#[test]
fn every_shared_operator_has_a_continuous_scalar_reference_case() {
    let cases = [
        ("bcs", json!({"brightness":0.125})),
        ("levels", json!({"gamma":0.1})),
        ("hue_rotate", json!({"degrees":120})),
        ("threshold", json!({"level":0.4})),
        ("posterize", json!({"levels":5})),
        ("curves", json!({"rgb":[[0,0],[255,127.5]]})),
        ("invert", json!({})),
        ("grayscale", json!({})),
        ("sepia", json!({})),
        ("exposure", json!({"exposure":1,"evaluation":"legacy_srgb"})),
        ("vibrance", json!({"vibrance":0.5})),
        ("hue_saturation", json!({"lightness":0.25})),
        ("color_balance", json!({})),
        ("black_and_white", json!({"reds":0.9})),
        (
            "photo_filter",
            json!({"color":"#808080","density":0.5,"preserve_luminosity":false}),
        ),
        (
            "channel_mixer",
            json!({"monochrome":true,"gray":{"r":0.25,"g":0.25,"b":0.25,"constant":0.125}}),
        ),
        ("selective_color", json!({})),
        (
            "shadows_highlights",
            json!({"shadows":{"amount":1,"tone":0.5},"highlights":{"amount":0},"color":0}),
        ),
        ("white_balance", json!({})),
        (
            "gradient_map",
            json!({"dither":false,"gradient":{"stops":[{"position":0,"color":"#000000"},{"position":1,"color":"#ffffff"}]}}),
        ),
        ("color_lookup", lut(&[0.0, 0.125, 0.25, 1.0, 0.875, 0.75])),
    ];
    assert_eq!(cases.len(), 21);
    for (name, params) in cases {
        let op = prepared(name, params);
        assert_eq!(op.operation(), name);
        let canonical = PrecisionAdjustment::parse(op.description(), &IccProfile::srgb()).unwrap();
        assert_eq!(canonical.description(), op.description());
        let mut maximum = 0.0_f64;
        for i in 0..513 {
            // Odd, non-byte-aligned samples avoid trivial 256-code agreement.
            let v = (i * 127 + 3).min(65535) as f32 / 65535.0;
            let x = f64::from(v);
            let expected = match name {
                "bcs" => [x + 0.125; 3],
                "levels" => [x.powi(10); 3],
                "threshold" => [if x >= 0.4 { 1.0 } else { 0.0 }; 3],
                "posterize" => [(x * 5.0).floor().min(4.0) / 4.0; 3],
                "curves" => [x * 0.5; 3],
                "invert" => [1.0 - x; 3],
                "sepia" => [1.351 * x, 1.203 * x, 0.937 * x],
                "exposure" => {
                    let linear = if x <= 0.04045 {
                        x / 12.92
                    } else {
                        ((x + 0.055) / 1.055).powf(2.4)
                    } * 2.0;
                    [if linear <= 0.0031308 {
                        linear * 12.92
                    } else {
                        1.055 * linear.powf(1.0 / 2.4) - 0.055
                    }; 3]
                }
                "hue_saturation" => [x + (1.0 - x) * 0.25; 3],
                "photo_filter" => {
                    let factor = 0.5 + 0.5 * srgb_decode(128.0 / 255.0);
                    [srgb_encode(srgb_decode(x) * factor); 3]
                }
                "channel_mixer" => [x * 0.75 + 0.125; 3],
                "shadows_highlights" => [x.powf(1.0 / 1.16); 3],
                "color_lookup" => [x, 0.125 + 0.75 * x, 0.25 + 0.5 * x],
                _ => [x; 3],
            }
            .map(|v| v.clamp(0.0, 1.0));
            let actual = op.apply_at([v; 3], 0.3, (i, 17));
            for (actual, expected) in actual.into_iter().zip(expected) {
                maximum = maximum.max((f64::from(actual) - expected).abs());
            }
            // Chromatic inputs also cover finite, normalized output and
            // canonical reconstruction, independent of the grey reference.
            let rgb = [v, 1.0 - v, (v * 0.731 + 0.017).fract()];
            let actual = op.apply_at(rgb, 0.3, (i, 17));
            assert!(
                actual
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "{name}"
            );
            assert_eq!(actual, canonical.apply_at(rgb, 0.3, (i, 17)));
        }
        eprintln!("{name}: maximum scalar reference error {maximum:.10}");
        let bound = if name == "photo_filter" {
            PHOTO_FILTER_BOUND
        } else {
            FLOAT_BOUND
        };
        assert!(maximum <= bound, "{name}: {maximum}");
    }
}

#[test]
fn severe_levels_and_continuous_channel_then_master_curves_keep_u16_detail() {
    let levels = prepared("levels", json!({"gamma":0.1}));
    let curves = prepared(
        "curves",
        json!({"r":[[0,0],[255,127.5]],"rgb":[[0,0],[255,64]]}),
    );
    let mut errors = [0.0_f64; 2];
    let mut previous = None;
    for code in 0..=65535 {
        let source = code as f32 / 65535.0;
        errors[0] = errors[0].max(
            (f64::from(levels.apply_at([source; 3], 0.0, (0, 0))[0]) - f64::from(source).powi(10))
                .abs(),
        );
        let output = curves.apply_at([source; 3], 0.0, (0, 0));
        for (actual, scale) in output
            .into_iter()
            .zip([32.0 / 255.0, 64.0 / 255.0, 64.0 / 255.0])
        {
            errors[1] = errors[1].max((f64::from(actual) - f64::from(source) * scale).abs());
        }
        if let Some(previous) = previous {
            assert!(
                output[0] > previous,
                "adjacent u16 inputs collapsed at {code}"
            );
        }
        previous = Some(output[0]);
    }
    eprintln!(
        "full u16 ramp: levels error {}, curves error {}",
        errors[0], errors[1]
    );
    assert!(errors.into_iter().all(|v| v <= FLOAT_BOUND));
}

#[test]
fn identity_preparations_preserve_float_bits_and_dither_is_one_u16_code_wide() {
    for name in [
        "bcs",
        "levels",
        "hue_rotate",
        "curves",
        "exposure",
        "vibrance",
        "hue_saturation",
        "color_balance",
        "channel_mixer",
        "selective_color",
        "white_balance",
    ] {
        let op = prepared(name, json!({}));
        assert!(op.is_identity(), "{name}");
        let input = [1e-12, 0.12345678, 0.99999994];
        assert_eq!(op.apply_at(input, 0.731, (-7, 19)), input, "{name}");
    }
    let gradient =
        json!({"stops":[{"position":0,"color":"#000000"},{"position":1,"color":"#ffffff"}]});
    let plain = prepared("gradient_map", json!({"gradient":gradient,"dither":false}));
    let dithered = prepared("gradient_map", json!({"gradient":gradient,"dither":true}));
    let rgb = [0.413571; 3];
    let anchor = plain.apply_at(rgb, 0.0, (0, 0))[0];
    let mut changed = 0;
    for x in -256..256 {
        let value = dithered.apply_at(rgb, 0.0, (x, 19))[0];
        assert!((value - anchor).abs() <= 0.5 / 65535.0 + 2e-7);
        changed += usize::from(value != anchor);
        assert_eq!(value, dithered.apply_at(rgb, 0.0, (x, 19))[0]);
    }
    assert!(changed > 400);
}

#[test]
fn strict_descriptions_finite_preparation_and_guide_plans_are_bounded() {
    let profile = IccProfile::srgb();
    for invalid in [
        json!({"type":"adjust","op":"invert"}),
        json!({"type":"adjust","op":"invert","evaluation":"future"}),
        json!({"type":"adjust","op":"invert","evaluation":"precision_v1","params":{"typo":0}}),
    ] {
        assert!(PrecisionAdjustment::parse(&invalid.to_string(), &profile).is_err());
    }
    let tiny_intervals = description("curves", json!({"r":[[0,0],[1e-320,1],[255,255]]}));
    assert!(PrecisionAdjustment::parse(&tiny_intervals, &profile).is_err());
    let extreme = description("color_lookup", lut(&[-f32::MAX; 6]));
    assert!(PrecisionAdjustment::parse(&extreme, &profile).is_err());
    let mut unbounded_domain = lut(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    unbounded_domain["domain_min"] = json!([-3e38, -3e38, -3e38]);
    unbounded_domain["domain_max"] = json!([3e38, 3e38, 3e38]);
    assert!(
        PrecisionAdjustment::parse(&description("color_lookup", unbounded_domain), &profile)
            .is_err()
    );
    assert!(PrecisionAdjustment::parse(
        &description("color_lookup", lut(&[-1.0, -1.0, -1.0, 2.0, 2.0, 2.0])),
        &profile
    )
    .is_ok());
    for radius in [0.0, 1.0, 6.0, 30.0, 1000.0] {
        let meta = description("shadows_highlights", json!({"radius":radius}));
        let op = PrecisionAdjustment::parse(&meta, &profile).unwrap();
        let plan = PrecisionAdjustment::guide_plan_for_description(&meta, 6073, 4027).unwrap();
        assert_eq!(
            plan,
            (
                op.guide_scratch_bytes(6073, 4027).unwrap(),
                op.guide_work_units(6073, 4027).unwrap()
            )
        );
        assert!(plan.0 > 0 && plan.1 > 0);
    }
    let idle = description(
        "shadows_highlights",
        json!({"shadows":{"amount":0},"highlights":{"amount":0}}),
    );
    assert_eq!(
        PrecisionAdjustment::guide_plan_for_description(&idle, 6073, 4027).unwrap(),
        (0, 0)
    );
    // Plan admission includes old and new Value vector storage during growth,
    // even for a large description that strict parsing will subsequently reject.
    let elements = (1usize << 20) + 1;
    let invalid = format!("[{}0]", "0,".repeat(elements - 1));
    let growth_peak = (3 * (1usize << 20) * std::mem::size_of::<Value>()) as u64;
    assert!(PrecisionAdjustment::prepare_scratch_bytes(&invalid, &profile).unwrap() >= growth_peak);
    let oversized = " ".repeat(16 * 1024 * 1024 + 1);
    assert!(PrecisionAdjustment::prepare_scratch_bytes(&oversized, &profile).is_err());
}

#[test]
fn float_spatial_guide_retains_sub_byte_alpha_against_direct_2d_gaussian() {
    let (w, h) = (19usize, 7usize);
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let value = 1001 + ((i * 7919) % 24000) as u16;
            [value, value, value, (i % 4) as u16]
        })
        .collect();
    let source =
        PrecisionImage::from_rgba16(w as u32, h as u32, pixels.clone(), IccProfile::srgb(), None)
            .unwrap();
    let document =
        PrecisionDocument::from_image(source, Metadata::default(), Resolution::default()).unwrap();
    // Radius 4 => sigma 2, with the shared finite 11-tap support. This oracle
    // uses direct 2-D f64 sums rather than the implementation's separable f32
    // passes. Every nonzero source alpha is below one byte coverage step.
    let meta = description(
        "shadows_highlights",
        json!({"radius":4,"shadows":{"amount":1,"tone":0.5},"highlights":{"amount":0},"color":0}),
    );
    let changed = document
        .apply_adjustment(0, &meta, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let output = changed.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    let mut maximum = 0.0_f64;
    let mut different_from_unblurred = 0;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            assert_eq!(output[i][3], pixels[i][3]);
            if pixels[i][3] == 0 {
                assert_eq!(output[i], pixels[i]);
                continue;
            }
            let (mut weighted, mut coverage) = (0.0, 0.0);
            for dy in -5_i64..=5 {
                for dx in -5_i64..=5 {
                    let sx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                    let sy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                    let pixel = pixels[sy * w + sx];
                    let weight = (-((dx * dx + dy * dy) as f64) / 8.0).exp() * f64::from(pixel[3]);
                    weighted += weight * f64::from(pixel[0]) / 65535.0;
                    coverage += weight;
                }
            }
            let guide = weighted / coverage;
            let input = f64::from(pixels[i][0]) / 65535.0;
            let reference = input.powf(1.0 / (1.0 + (1.0 - guide / 0.5).clamp(0.0, 1.0).powi(2)));
            let unblurred = input.powf(1.0 / (1.0 + (1.0 - input / 0.5).clamp(0.0, 1.0).powi(2)));
            different_from_unblurred += usize::from((reference - unblurred).abs() > 0.001);
            for &channel in &output[i][..3] {
                maximum = maximum.max((f64::from(channel) / 65535.0 - reference).abs());
            }
        }
    }
    eprintln!("sub-byte-alpha spatial guide: maximum stored error {maximum:.10}");
    assert!(different_from_unblurred > 50);
    assert!(maximum <= FLOAT_BOUND + 0.5 / 65535.0, "{maximum}");
    assert_eq!(
        document.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        pixels
    );
}
