//! OpenEXR in and out of the 32-bit float document.
//!
//! The READ oracle is `common/exr_builder.rs`, a hand-written OpenEXR byte
//! writer that shares nothing with the `exr` crate: every authored sample is
//! compared with what the open holds. The WRITE side is read back through
//! the seam that oracle has already checked. Half samples are compared with
//! the builder's own widening and its search for the nearest half, and the
//! primaries conversions with the published matrices (ITU-R BT.2087 for
//! Rec.2020, the ACEScg-to-sRGB Bradford matrix), written out here. The
//! primaries an export declares for a profile that is neither built-in are
//! compared with Adobe RGB (1998)'s published xy pairs.

use std::sync::Arc;

use doctored_core::doc_export::ExportRequest;
use doctored_core::exr_float::{self, Chromaticities};
use doctored_core::ffi::{dr_image_height, dr_image_pixels_rgba, dr_image_width};
use doctored_core::float_primaries::{DISPLAY_P3, REC709};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::FloatSampleType;

use crate::exr_builder::{
    file, float_channels, half_channels, half_value, nearest_half_bits, Part,
};
use crate::float_fixture::float_doc;
use crate::icc_spelling::adobe_rgb_1998;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn open(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("image.exr");
    std::fs::write(&path, bytes).unwrap();
    PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
}

fn warnings(document: &PrecisionDocument) -> String {
    document.info().unwrap()["import_warnings"].to_string()
}

fn layer_pixels(document: &PrecisionDocument) -> Vec<[f32; 4]> {
    document.layer_source(0).unwrap().to_rgba32().ok().unwrap()
}

fn export(document: &PrecisionDocument, request: &str) -> Vec<u8> {
    let request = ExportRequest::parse(request).unwrap_or_else(|e| panic!("{request}: {e}"));
    document
        .export_encoded(None, &request, LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("export: {e}"))
        .bytes()
        .to_vec()
}

/// A position on the number line of f32 bit patterns, so the distance
/// between two floats is a count of representable values (ulps).
fn ordered(value: f32) -> i64 {
    let bits = value.to_bits();
    if bits & 0x8000_0000 != 0 {
        -i64::from(bits & 0x7fff_ffff)
    } else {
        i64::from(bits)
    }
}

fn ulps(a: f32, b: f32) -> i64 {
    (ordered(a) - ordered(b)).abs()
}

/// 5×3 RGB samples: values above 1, a negative, a tiny one and zero.
fn rgb_samples() -> Vec<f32> {
    (0..15)
        .flat_map(|i| {
            let v = i as f32;
            [v * 0.75 - 1.5, 0.001 * v, 4.0 + v * 100.0]
        })
        .collect()
}

#[test]
fn an_f32_file_without_alpha_opens_with_the_authored_samples() {
    let samples = rgb_samples();
    let bytes = file(&[Part::new(5, 3, float_channels(&["R", "G", "B"], &samples))]);
    let plan = exr_float::inspect(&bytes, "t").unwrap();
    assert_eq!((plan.width, plan.height), (5, 3));
    assert_eq!(plan.canvas, (5, 3));
    assert_eq!(plan.offset, (0, 0));
    assert!(!plan.alpha);
    assert_eq!((plan.part, plan.parts), (0, 1));
    assert_eq!(plan.chromaticities, None);
    assert_eq!(plan.scratch_bytes, 15 * 32 + 1024 * 1024);

    let document = open(&bytes).unwrap();
    assert!(document.is_float());
    assert_eq!(document.dimensions(), (5, 3));
    assert!(document.profile().is_builtin_srgb());
    for (i, pixel) in layer_pixels(&document).iter().enumerate() {
        for c in 0..3 {
            assert_eq!(pixel[c].to_bits(), samples[i * 3 + c].to_bits(), "{i}:{c}");
        }
        assert_eq!(pixel[3], 1.0, "an absent alpha is opaque");
    }
    let text = warnings(&document);
    assert!(text.contains("without chromaticities"), "{text}");
    assert!(!text.contains("unpremultiplied"), "{text}");
}

#[test]
fn every_finite_half_widens_exactly() {
    // 63,488 finite half patterns: both signs, exponents 0 to 30.
    let patterns: Vec<u16> = (0..=u16::MAX).filter(|b| (b >> 10) & 0x1f != 31).collect();
    assert_eq!(patterns.len(), 63_488);
    let (width, height) = (248u32, 256u32);
    let bits: Vec<u16> = patterns
        .iter()
        .enumerate()
        .flat_map(|(i, &p)| [p, patterns[patterns.len() - 1 - i], 0x3c00])
        .collect();
    let bytes = file(&[Part::new(
        width,
        height,
        half_channels(&["R", "G", "B"], &bits),
    )]);
    let decoded = exr_float::decode(&bytes, "t", u64::MAX).unwrap();
    assert_eq!(decoded.non_finite, 0);
    for (i, pixel) in decoded.samples.as_chunks::<4>().0.iter().enumerate() {
        for c in 0..3 {
            assert_eq!(
                pixel[c].to_bits(),
                half_value(bits[i * 3 + c]).to_bits(),
                "half {:#06x}",
                bits[i * 3 + c]
            );
        }
        assert_eq!(pixel[3], 1.0);
    }
}

/// The round-trip fixture: an opaque block, fractional alphas and an alpha-0
/// block whose stored RGB is not zero. The file stores ASSOCIATED samples.
fn alpha_fixture() -> (u32, u32, Vec<f32>) {
    let (width, height) = (12u32, 6u32);
    let samples = (0..width * height)
        .flat_map(|i| {
            let (x, y) = (i % width, i / width);
            let alpha = match x / 4 {
                0 => 1.0,
                1 => [0.3f32, 0.5, 0.123_456_7, 0.999, 0.01, 0.75][y as usize],
                _ => 0.0,
            };
            let v = (i as f32 * 0.618_034).fract();
            // Associated: at most alpha, plus highlights above it and a
            // negative, all with a straight value far inside ±65504.
            let scale = if alpha > 0.0 { alpha } else { 1.0 };
            [
                v * scale,
                (3.5 + v) * scale,
                (-0.25 * v - 0.01) * scale,
                alpha,
            ]
        })
        .collect();
    (width, height, samples)
}

#[test]
fn the_f32_round_trip_meets_its_criterion_in_each_alpha_class() {
    let (width, height, samples) = alpha_fixture();
    let bytes = file(&[Part::new(
        width,
        height,
        float_channels(&["R", "G", "B", "A"], &samples),
    )]);
    let document = open(&bytes).unwrap();
    assert!(warnings(&document).contains("associated alpha unpremultiplied"));

    // The open: straight samples, alpha exact, alpha 0 the canonical zero.
    for (i, pixel) in layer_pixels(&document).iter().enumerate() {
        let stored = &samples[i * 4..i * 4 + 4];
        assert_eq!(pixel[3].to_bits(), stored[3].to_bits(), "alpha {i}");
        for c in 0..3 {
            let want = if stored[3] > 0.0 {
                stored[c] / stored[3]
            } else {
                0.0
            };
            assert_eq!(pixel[c].to_bits(), want.to_bits(), "straight {i}:{c}");
        }
    }

    let exported = export(&document, "{\"format\":\"exr\",\"bit_depth\":32}");
    let back = exr_float::decode(&exported, "export", u64::MAX).unwrap();
    assert!(back.plan.alpha);
    assert_eq!((back.plan.width, back.plan.height), (width, height));
    let mut classes = [0usize; 3];
    for (i, pixel) in back.samples.as_chunks::<4>().0.iter().enumerate() {
        let stored = &samples[i * 4..i * 4 + 4];
        assert_eq!(pixel[3].to_bits(), stored[3].to_bits(), "alpha {i}");
        if stored[3] == 1.0 {
            classes[0] += 1;
            for c in 0..3 {
                assert_eq!(pixel[c].to_bits(), stored[c].to_bits(), "opaque {i}:{c}");
            }
        } else if stored[3] > 0.0 {
            classes[1] += 1;
            for c in 0..3 {
                assert!(
                    ulps(pixel[c], stored[c]) <= 2,
                    "fractional {i}:{c}: {} vs {} is {} ulp",
                    pixel[c],
                    stored[c],
                    ulps(pixel[c], stored[c])
                );
            }
        } else {
            classes[2] += 1;
            assert_eq!(pixel[..3], [0.0; 3], "transparent {i}");
        }
    }
    assert!(classes.iter().all(|count| *count > 0), "{classes:?}");
}

#[test]
fn a_half_export_writes_the_nearest_half() {
    let samples = rgb_samples();
    let document = open(&file(&[Part::new(
        5,
        3,
        float_channels(&["R", "G", "B"], &samples),
    )]))
    .unwrap();
    let request =
        ExportRequest::parse("{\"format\":\"exr\",\"bit_depth\":32,\"float_samples\":\"f16\"}")
            .unwrap();
    let result = document
        .export_encoded(None, &request, LIMIT, 0, None)
        .unwrap();
    assert_eq!(result.info_json()["float_samples"], "f16");
    assert_eq!(result.info_json()["bit_depth"], 32);
    assert_eq!(result.info_json()["format"], "exr");
    let back = exr_float::decode(result.bytes(), "export", u64::MAX).unwrap();
    for (i, pixel) in back.samples.as_chunks::<4>().0.iter().enumerate() {
        for c in 0..3 {
            let want = half_value(nearest_half_bits(samples[i * 3 + c]));
            assert_eq!(pixel[c].to_bits(), want.to_bits(), "{i}:{c}");
        }
        assert_eq!(pixel[3], 1.0);
    }
}

#[test]
fn the_seam_writes_what_it_is_given_at_both_widths() {
    let rgba: Vec<f32> = (0..9 * 7 * 4).map(|i| i as f32 * 0.37 - 20.0).collect();
    let f32_bytes = exr_float::encode(9, 7, &rgba, FloatSampleType::F32, REC709).unwrap();
    let back = exr_float::decode(&f32_bytes, "t", u64::MAX).unwrap();
    assert_eq!(back.plan.chromaticities, Some(REC709));
    assert_eq!(
        back.samples.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        rgba.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
    let half_bytes = exr_float::encode(9, 7, &rgba, FloatSampleType::F16, DISPLAY_P3).unwrap();
    let back = exr_float::decode(&half_bytes, "t", u64::MAX).unwrap();
    assert_eq!(back.plan.chromaticities, Some(DISPLAY_P3));
    for (got, want) in back.samples.iter().zip(&rgba) {
        assert_eq!(
            got.to_bits(),
            half_value(nearest_half_bits(*want)).to_bits()
        );
    }
    // The bytes are a function of the samples alone.
    assert_eq!(
        f32_bytes,
        exr_float::encode(9, 7, &rgba, FloatSampleType::F32, REC709).unwrap()
    );
    assert!(exr_float::encode(9, 7, &rgba[1..], FloatSampleType::F32, REC709).is_err());
    assert!(exr_float::encode(0, 7, &[], FloatSampleType::F32, REC709).is_err());
}

fn with_primaries(primaries: Chromaticities, samples: &[f32]) -> Vec<u8> {
    let mut part = Part::new(5, 3, float_channels(&["R", "G", "B"], samples));
    part.chromaticities = Some(primaries);
    file(&[part])
}

#[test]
fn stated_primaries_choose_the_profile_and_no_pixel_moves_for_a_built_in() {
    let samples = rgb_samples();
    let srgb = open(&with_primaries(REC709, &samples)).unwrap();
    assert!(srgb.profile().is_builtin_srgb());
    assert!(
        !warnings(&srgb).contains("chromaticities"),
        "{}",
        warnings(&srgb)
    );

    // A renderer's own rounding of the same primaries is still the space.
    let mut near = DISPLAY_P3;
    near[0][0] += 0.0015;
    near[3][1] -= 0.0015;
    let p3 = open(&with_primaries(near, &samples)).unwrap();
    assert_eq!(
        p3.profile().bytes()[..],
        IccProfile::display_p3().bytes()[..]
    );
    for (i, pixel) in layer_pixels(&p3).iter().enumerate() {
        for c in 0..3 {
            assert_eq!(pixel[c].to_bits(), samples[i * 3 + c].to_bits(), "{i}:{c}");
        }
    }

    // The export states the document profile's primaries and converts nothing.
    let exported = export(&p3, "{\"format\":\"exr\",\"bit_depth\":32}");
    let back = exr_float::decode(&exported, "export", u64::MAX).unwrap();
    assert_eq!(back.plan.chromaticities, Some(DISPLAY_P3));
    for (i, pixel) in back.samples.as_chunks::<4>().0.iter().enumerate() {
        for c in 0..3 {
            assert_eq!(pixel[c].to_bits(), samples[i * 3 + c].to_bits(), "{i}:{c}");
        }
    }
    let exported = export(&srgb, "{\"format\":\"exr\",\"bit_depth\":32}");
    assert_eq!(
        exr_float::inspect(&exported, "export")
            .unwrap()
            .chromaticities,
        Some(REC709)
    );
}

/// Adobe RGB (1998), from its specification: red, green, blue, D65 white.
const ADOBE_RGB: Chromaticities = [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06], [0.3127, 0.3290]];

/// The numbers of a Radiance header's `PRIMARIES=` line, read from the text.
fn radiance_primaries(bytes: &[u8]) -> Vec<f32> {
    let end = bytes
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .expect("a header");
    let header = String::from_utf8_lossy(&bytes[..end]).to_string();
    let line = header
        .lines()
        .find_map(|line| line.strip_prefix("PRIMARIES="))
        .unwrap_or_else(|| panic!("no PRIMARIES line in {header}"));
    line.split_whitespace()
        .map(|number| number.parse().unwrap())
        .collect()
}

/// A document profile that is neither built-in states its NATIVE primaries:
/// the D50-adapted colorants taken back through the profile's own adaptation,
/// whether that is a `wtpt` holding D65 or a `chad` tag. The oracle is the
/// published xy pairs; nothing here computes an adaptation.
#[test]
fn a_profile_that_is_neither_built_in_declares_its_native_primaries() {
    let pixels: Vec<[f32; 4]> = rgb_samples()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| [p[0], p[1], p[2], 1.0])
        .collect();
    for with_chad in [false, true] {
        let profile = Arc::new(IccProfile::parse(&adobe_rgb_1998(with_chad)).expect("a profile"));
        assert!(!profile.describes_srgb());
        let document = float_doc(5, 3, &profile, std::slice::from_ref(&pixels));

        let exported = export(&document, "{\"format\":\"exr\",\"bit_depth\":32}");
        let back = exr_float::decode(&exported, "export", u64::MAX).unwrap();
        let declared = back.plan.chromaticities.expect("chromaticities");
        for (k, (got, want)) in declared
            .iter()
            .flatten()
            .zip(ADOBE_RGB.iter().flatten())
            .enumerate()
        {
            assert!(
                (got - want).abs() <= 1e-3,
                "chad {with_chad}, exr coordinate {k}: {got} vs {want} in {declared:?}"
            );
        }
        // The samples are the document's own: the file names the space.
        for (i, pixel) in back.samples.as_chunks::<4>().0.iter().enumerate() {
            for c in 0..3 {
                assert_eq!(pixel[c].to_bits(), pixels[i][c].to_bits(), "{i}:{c}");
            }
        }

        let exported = export(&document, "{\"format\":\"hdr\",\"bit_depth\":32}");
        let stated = radiance_primaries(&exported);
        assert_eq!(stated.len(), 8, "{stated:?}");
        for (k, (got, want)) in stated.iter().zip(ADOBE_RGB.iter().flatten()).enumerate() {
            assert!(
                (got - want).abs() <= 1e-3,
                "chad {with_chad}, hdr coordinate {k}: {got} vs {want} in {stated:?}"
            );
        }
    }
}

/// ITU-R BT.2087: linear Rec.2020 to linear Rec.709, both D65.
const REC2020_TO_709: [[f64; 3]; 3] = [
    [1.6605, -0.5876, -0.0728],
    [-0.1246, 1.1329, -0.0083],
    [-0.0182, -0.1006, 1.1187],
];
const REC2020: Chromaticities = [
    [0.708, 0.292],
    [0.170, 0.797],
    [0.131, 0.046],
    [0.3127, 0.3290],
];

/// ACEScg (AP1, white 0.32168 0.33767) to linear sRGB through a Bradford
/// adaptation, as published with the ACES colour-space conversions.
const ACESCG_TO_SRGB: [[f64; 3]; 3] = [
    [1.705_05, -0.621_79, -0.083_26],
    [-0.130_26, 1.140_80, -0.010_55],
    [-0.024_00, -0.128_97, 1.152_97],
];
const AP1: Chromaticities = [
    [0.713, 0.293],
    [0.165, 0.830],
    [0.128, 0.044],
    [0.32168, 0.33767],
];

#[test]
fn other_primaries_are_converted_in_linear_light_with_a_warning() {
    let samples = rgb_samples();
    for (primaries, matrix, tolerance) in
        [(REC2020, REC2020_TO_709, 2e-3), (AP1, ACESCG_TO_SRGB, 2e-3)]
    {
        let document = open(&with_primaries(primaries, &samples)).unwrap();
        assert!(document.profile().is_builtin_srgb());
        let text = warnings(&document);
        assert!(
            text.contains("converted in linear light to sRGB primaries"),
            "{text}"
        );
        for (i, pixel) in layer_pixels(&document).iter().enumerate() {
            let source = &samples[i * 3..i * 3 + 3];
            let size = source.iter().fold(0f64, |m, v| m.max(f64::from(v.abs())));
            for c in 0..3 {
                let want: f64 = (0..3).map(|k| matrix[c][k] * f64::from(source[k])).sum();
                assert!(
                    (f64::from(pixel[c]) - want).abs() <= tolerance * size.max(1e-6) * 3.0,
                    "{i}:{c}: {} vs {want}",
                    pixel[c]
                );
            }
        }
        // A negative component stays one: the conversion clamps nothing.
        assert!(layer_pixels(&document).iter().any(|p| p[0] < 0.0));
    }
    // Collinear primaries are no colour basis.
    let flat = [[0.3, 0.3], [0.4, 0.4], [0.5, 0.5], [0.3127, 0.3290]];
    let error = open(&with_primaries(flat, &samples)).err().unwrap();
    assert!(error.contains("colour basis"), "{error}");
}

#[test]
fn the_first_rgb_part_opens_and_the_others_are_disclosed() {
    let depth: Vec<f32> = (0..15).map(|i| i as f32).collect();
    let samples = rgb_samples();
    let mut first = Part::new(5, 3, float_channels(&["Z"], &depth));
    first.name = Some("depth");
    let mut second = Part::new(5, 3, float_channels(&["R", "G", "B"], &samples));
    second.name = Some("beauty");
    let mut third = Part::new(5, 3, float_channels(&["R", "G", "B"], &depth.repeat(3)));
    third.name = Some("later");
    let bytes = file(&[first, second, third]);
    let plan = exr_float::inspect(&bytes, "t").unwrap();
    assert_eq!((plan.part, plan.parts), (1, 3));
    assert_eq!(plan.part_name.as_deref(), Some("beauty"));
    let document = open(&bytes).unwrap();
    for (i, pixel) in layer_pixels(&document).iter().enumerate() {
        for c in 0..3 {
            assert_eq!(pixel[c].to_bits(), samples[i * 3 + c].to_bits(), "{i}:{c}");
        }
    }
    let text = warnings(&document);
    assert!(text.contains("part 2 of 3"), "{text}");
    assert!(text.contains("beauty"), "{text}");
}

#[test]
fn deep_and_colourless_files_refuse_by_name() {
    let samples = rgb_samples();
    let mut deep = Part::new(5, 3, float_channels(&["R", "G", "B"], &samples));
    deep.deep = true;
    let error = open(&file(&[deep.clone()])).err().unwrap();
    assert!(error.contains("only deep data"), "{error}");

    let mut flat = Part::new(5, 3, float_channels(&["R", "G", "B"], &samples));
    flat.name = Some("flat");
    deep.name = Some("deep");
    let error = open(&file(&[deep, flat])).err().unwrap();
    assert!(error.contains("deep parts"), "{error}");

    let luminance: Vec<f32> = (0..15).map(|i| i as f32 * 0.1).collect();
    let error = open(&file(&[Part::new(
        5,
        3,
        float_channels(&["Y"], &luminance),
    )]))
    .err()
    .unwrap();
    assert!(error.contains("R, G and B"), "{error}");
}

#[test]
fn the_display_window_is_the_canvas_and_the_data_window_the_layer() {
    let samples = rgb_samples();
    let mut part = Part::new(5, 3, float_channels(&["R", "G", "B"], &samples));
    part.data_window = (12, 7, 5, 3);
    part.display_window = (10, 5, 20, 12);
    part.pixel_aspect = 2.0;
    let bytes = file(&[part]);
    let plan = exr_float::inspect(&bytes, "t").unwrap();
    assert_eq!(plan.canvas, (20, 12));
    assert_eq!(plan.offset, (2, 2));
    assert_eq!(plan.pixel_aspect, 2.0);

    let document = open(&bytes).unwrap();
    assert_eq!(document.dimensions(), (20, 12));
    assert_eq!(document.layer(0).unwrap().offset(), (2, 2));
    let rendered = document
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .ok()
        .unwrap();
    for (i, pixel) in rendered.iter().enumerate() {
        let (x, y) = (i as i32 % 20 - 2, i as i32 / 20 - 2);
        if (0..5).contains(&x) && (0..3).contains(&y) {
            let at = (y * 5 + x) as usize * 3;
            assert_eq!(pixel[0].to_bits(), samples[at].to_bits(), "{i}");
            assert_eq!(pixel[3], 1.0);
        } else {
            assert_eq!(pixel[3], 0.0, "outside the data window {i}");
        }
    }
    let text = warnings(&document);
    assert!(text.contains("data window"), "{text}");
    assert!(text.contains("pixel aspect ratio 2"), "{text}");
}

#[test]
fn non_finite_samples_are_repaired_and_counted() {
    let mut samples = rgb_samples();
    samples[0] = f32::NAN;
    samples[4] = f32::INFINITY;
    samples[8] = f32::NEG_INFINITY;
    let bytes = file(&[Part::new(5, 3, float_channels(&["R", "G", "B"], &samples))]);
    assert_eq!(
        exr_float::decode(&bytes, "t", u64::MAX).unwrap().non_finite,
        3
    );
    let document = open(&bytes).unwrap();
    let pixels = layer_pixels(&document);
    assert_eq!(pixels[0][0], 0.0);
    assert_eq!(pixels[1][1], 65504.0);
    assert_eq!(pixels[2][2], -65504.0);
    assert!(warnings(&document).contains("3 non-finite samples replaced"));
}

#[test]
fn corrupt_and_oversized_files_return_an_error() {
    let samples = rgb_samples();
    let bytes = file(&[Part::new(5, 3, float_channels(&["R", "G", "B"], &samples))]);
    assert!(exr_float::is_exr(&bytes));
    assert!(!exr_float::is_exr(b"II*\0"));
    assert!(exr_float::inspect(b"not an exr", "t").is_err());
    for cut in [5, 9, 40, bytes.len() / 2, bytes.len() - 7] {
        assert!(open(&bytes[..cut]).is_err(), "truncated at {cut}");
    }
    let mut garbage = bytes.clone();
    for byte in garbage.iter_mut().skip(8).take(200) {
        *byte = byte.wrapping_mul(31).wrapping_add(7);
    }
    assert!(open(&garbage).is_err());
    // The plan is refused before a sample is allocated.
    let error = exr_float::decode(&bytes, "t", 100).err().unwrap();
    assert!(error.contains("working memory"), "{error}");
}

#[test]
fn exr_exists_only_at_thirty_two_bits_from_a_float_document() {
    for (request, named) in [
        ("{\"format\":\"exr\"}", "32-bit float only"),
        ("{\"format\":\"exr\",\"bit_depth\":16}", "32-bit float only"),
        (
            "{\"format\":\"png\",\"bit_depth\":16,\"float_samples\":\"f16\"}",
            "requires bit_depth 32 and tiff or exr",
        ),
        (
            "{\"format\":\"exr\",\"bit_depth\":32,\"float_samples\":\"f64\"}",
            "f32 or f16",
        ),
        (
            "{\"format\":\"exr\",\"bit_depth\":32,\"profile\":\"srgb\"}",
            "profile document",
        ),
    ] {
        let error = ExportRequest::parse(request).err().unwrap_or_default();
        assert!(error.contains(named), "{request} -> {error}");
    }
    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[100, 200, 300, 65535]; 2],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let document =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let request = ExportRequest::parse("{\"format\":\"exr\",\"bit_depth\":32}").unwrap();
    let error = document
        .export_encoded(None, &request, LIMIT, 0, None)
        .err()
        .unwrap();
    assert!(error.contains("32-bit float document"), "{error}");
}

#[test]
fn the_preview_is_the_written_file_under_the_documents_tone_map() {
    let samples = rgb_samples();
    let document = open(&file(&[Part::new(
        5,
        3,
        float_channels(&["R", "G", "B"], &samples),
    )]))
    .unwrap();
    let rendered = document.render(LIMIT, 0, None).unwrap();
    let display = document.display_rgba8(&rendered, LIMIT, 0, None).unwrap();
    for request in [
        "{\"format\":\"exr\",\"bit_depth\":32}",
        "{\"format\":\"tiff\",\"bit_depth\":32}",
    ] {
        let request = ExportRequest::parse(request).unwrap();
        let result = document
            .export_encoded(Some(&rendered), &request, LIMIT, 0, None)
            .unwrap();
        assert_eq!(result.info_json()["float_samples"], "f32");
        let preview = result.decode_preview().unwrap();
        let preview = unsafe {
            let length = dr_image_width(&preview) * dr_image_height(&preview) * 4;
            std::slice::from_raw_parts(dr_image_pixels_rgba(&preview), length as usize).to_vec()
        };
        assert_eq!(
            &preview,
            display.as_raw(),
            "an exact export previews as the document displays"
        );
    }
}
