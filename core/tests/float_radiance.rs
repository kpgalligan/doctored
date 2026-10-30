//! Radiance RGBE (`.hdr`) in and out of the 32-bit float document.
//!
//! The READ oracle is the format itself, written out here by hand: a text
//! header, a resolution line, then four bytes a pixel — three mantissas and a
//! shared exponent — worth `m · 2^(e − 136)` per channel and zero when `e` is
//! 0. The WRITE side is read back through the seam those hand-built bytes
//! have checked and compared with the format's own quantisation step.

use std::ptr;

use doctored_core::doc_export::ExportRequest;
use doctored_core::exr_float::Chromaticities;
use doctored_core::ffi_doc::dr_doc_open;
use doctored_core::float_primaries::{DISPLAY_P3, REC709};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::radiance_hdr;

use crate::common::{cpath, take_err_string};
use crate::exr_builder::{file, float_channels, Part};
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// One RGBE pixel's value, from the format's definition.
fn rgbe_value(pixel: [u8; 4]) -> [f32; 3] {
    if pixel[3] == 0 {
        return [0.0; 3];
    }
    let scale = 2f64.powi(i32::from(pixel[3]) - 136);
    [0, 1, 2].map(|c| (f64::from(pixel[c]) * scale) as f32)
}

/// A flat (not run-length encoded) file: `header` lines, then the pixels.
fn flat_file(signature: &str, header: &[&str], resolution: &str, pixels: &[[u8; 4]]) -> Vec<u8> {
    let mut out = format!("{signature}\n").into_bytes();
    for line in header {
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out.push(b'\n');
    out.extend_from_slice(resolution.as_bytes());
    out.push(b'\n');
    for pixel in pixels {
        out.extend_from_slice(pixel);
    }
    out
}

/// 3×2 pixels: mid-grey, a highlight far above white, a dim value, black
/// with a zero exponent, and two saturated ones.
const PIXELS: [[u8; 4]; 6] = [
    [128, 128, 128, 128],
    [200, 100, 50, 140],
    [255, 1, 0, 110],
    [9, 9, 9, 0],
    [255, 255, 255, 129],
    [1, 2, 3, 136],
];

fn standard() -> Vec<u8> {
    flat_file(
        "#?RADIANCE",
        &["FORMAT=32-bit_rle_rgbe"],
        "-Y 2 +X 3",
        &PIXELS,
    )
}

fn open(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("image.hdr");
    std::fs::write(&path, bytes).unwrap();
    PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
}

fn warnings(document: &PrecisionDocument) -> String {
    document.info().unwrap()["import_warnings"].to_string()
}

fn layer_pixels(document: &PrecisionDocument) -> Vec<[f32; 4]> {
    document.layer_source(0).unwrap().to_rgba32().ok().unwrap()
}

fn assert_authored(document: &PrecisionDocument) {
    for (i, pixel) in layer_pixels(document).iter().enumerate() {
        let want = rgbe_value(PIXELS[i]);
        for c in 0..3 {
            assert_eq!(pixel[c].to_bits(), want[c].to_bits(), "{i}:{c}");
        }
        assert_eq!(pixel[3], 1.0, "RGBE has no alpha");
    }
}

#[test]
fn a_flat_file_opens_opaque_with_the_authored_values() {
    let bytes = standard();
    assert!(radiance_hdr::is_radiance(&bytes));
    let plan = radiance_hdr::inspect(&bytes, "t").unwrap();
    assert_eq!((plan.width, plan.height), (3, 2));
    assert_eq!(plan.primaries, None);
    assert_eq!(plan.exposure, None);
    assert_eq!(plan.scratch_bytes, 6 * 24 + 1024 * 1024);
    let document = open(&bytes).unwrap();
    assert!(document.is_float());
    assert_eq!(document.dimensions(), (3, 2));
    assert!(document.profile().is_builtin_srgb());
    assert_authored(&document);
    assert!(layer_pixels(&document)[1][0] > 1.0, "a value above white");
    assert!(warnings(&document).contains("without PRIMARIES"));
}

#[test]
fn the_rgbe_signature_and_a_missing_format_line_open_too() {
    let document = open(&flat_file("#?RGBE", &[], "-Y 2 +X 3", &PIXELS)).unwrap();
    assert_authored(&document);
}

#[test]
fn a_run_length_encoded_scanline_decodes() {
    // Width 8, one scanline in the new scheme: the 2 2 hi lo marker, then
    // each component as a run of eight (128 + 8, value) or eight literals.
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 8\n".to_vec();
    bytes.extend_from_slice(&[2, 2, 0, 8]);
    bytes.extend_from_slice(&[136, 200]);
    bytes.push(8);
    bytes.extend_from_slice(&[10, 20, 30, 40, 50, 60, 70, 80]);
    bytes.extend_from_slice(&[136, 7]);
    bytes.extend_from_slice(&[136, 130]);
    let (plan, samples) = radiance_hdr::decode(&bytes, "t", u64::MAX).unwrap();
    assert_eq!((plan.width, plan.height), (8, 1));
    for x in 0..8usize {
        let want = rgbe_value([200, (x as u8 + 1) * 10, 7, 130]);
        for c in 0..3 {
            assert_eq!(samples[x * 3 + c].to_bits(), want[c].to_bits(), "{x}:{c}");
        }
    }
}

fn primaries_line(primaries: Chromaticities) -> String {
    let numbers: Vec<String> = primaries.iter().flatten().map(|v| v.to_string()).collect();
    format!("PRIMARIES= {}", numbers.join(" "))
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

#[test]
fn a_primaries_line_chooses_the_profile_or_converts() {
    let with = |primaries: Chromaticities| {
        let line = primaries_line(primaries);
        flat_file(
            "#?RADIANCE",
            &["FORMAT=32-bit_rle_rgbe", &line],
            "-Y 2 +X 3",
            &PIXELS,
        )
    };
    let srgb = open(&with(REC709)).unwrap();
    assert!(srgb.profile().is_builtin_srgb());
    assert!(
        !warnings(&srgb).contains("PRIMARIES"),
        "{}",
        warnings(&srgb)
    );
    assert_authored(&srgb);

    let p3 = open(&with(DISPLAY_P3)).unwrap();
    assert_eq!(
        p3.profile().bytes()[..],
        IccProfile::display_p3().bytes()[..]
    );
    assert_authored(&p3);
    assert_eq!(
        radiance_hdr::inspect(&with(DISPLAY_P3), "t")
            .unwrap()
            .primaries,
        Some(DISPLAY_P3)
    );

    let wide = open(&with(REC2020)).unwrap();
    assert!(wide.profile().is_builtin_srgb());
    assert!(warnings(&wide).contains("converted in linear light to sRGB primaries"));
    for (i, pixel) in layer_pixels(&wide).iter().enumerate() {
        let source = rgbe_value(PIXELS[i]);
        let size = source.iter().fold(0f64, |m, v| m.max(f64::from(*v)));
        for c in 0..3 {
            let want: f64 = (0..3)
                .map(|k| REC2020_TO_709[c][k] * f64::from(source[k]))
                .sum();
            assert!(
                (f64::from(pixel[c]) - want).abs() <= 6e-3 * size.max(1e-9),
                "{i}:{c}: {} vs {want}",
                pixel[c]
            );
        }
    }

    let short = flat_file(
        "#?RADIANCE",
        &["PRIMARIES= 0.64 0.33 0.3"],
        "-Y 2 +X 3",
        &PIXELS,
    );
    let error = open(&short).err().unwrap();
    assert!(
        error.contains("PRIMARIES must state eight numbers"),
        "{error}"
    );
}

#[test]
fn exposure_and_colour_correction_are_disclosed_and_not_applied() {
    let bytes = flat_file(
        "#?RADIANCE",
        &[
            "FORMAT=32-bit_rle_rgbe",
            "EXPOSURE=2.5",
            "COLORCORR=1.1 0.9 1.0",
        ],
        "-Y 2 +X 3",
        &PIXELS,
    );
    let plan = radiance_hdr::inspect(&bytes, "t").unwrap();
    assert_eq!(plan.exposure, Some(2.5));
    assert_eq!(plan.color_correction, Some([1.1, 0.9, 1.0]));
    let document = open(&bytes).unwrap();
    assert_authored(&document);
    let text = warnings(&document);
    assert!(text.contains("EXPOSURE 2.5"), "{text}");
    assert!(text.contains("COLORCORR 1.1 0.9 1"), "{text}");
    assert!(text.contains("not applied"), "{text}");
}

#[test]
fn another_format_or_orientation_refuses_by_name() {
    let xyze = flat_file(
        "#?RADIANCE",
        &["FORMAT=32-bit_rle_xyze"],
        "-Y 2 +X 3",
        &PIXELS,
    );
    let error = open(&xyze).err().unwrap();
    assert!(error.contains("32-bit_rle_xyze"), "{error}");
    assert!(error.contains("unavailable"), "{error}");
    for resolution in ["+Y 2 +X 3", "-Y 2 -X 3", "+X 3 -Y 2"] {
        let turned = flat_file(
            "#?RADIANCE",
            &["FORMAT=32-bit_rle_rgbe"],
            resolution,
            &PIXELS,
        );
        let error = open(&turned).err().unwrap();
        assert!(error.contains("orientation"), "{resolution}: {error}");
        assert!(error.contains(resolution), "{resolution}: {error}");
    }
}

#[test]
fn corrupt_files_return_an_error() {
    let bytes = standard();
    assert!(!radiance_hdr::is_radiance(b"#?RADIUS"));
    assert!(radiance_hdr::inspect(b"not a radiance file", "t").is_err());
    for cut in [8, 20, bytes.len() - 13, bytes.len() - 1] {
        assert!(open(&bytes[..cut]).is_err(), "truncated at {cut}");
    }
    let unreadable = flat_file("#?RADIANCE", &[], "-Y two +X 3", &PIXELS);
    assert!(open(&unreadable).is_err());
    let empty = flat_file("#?RADIANCE", &[], "-Y 0 +X 3", &[]);
    assert!(open(&empty).is_err());
    let error = radiance_hdr::decode(&bytes, "t", 10).err().unwrap();
    assert!(error.contains("working memory"), "{error}");
}

#[test]
fn an_export_is_within_the_formats_own_step_of_the_premultiplied_samples() {
    let (width, height) = (17u32, 5u32);
    let pixels: Vec<[f32; 4]> = (0..width * height)
        .map(|i| {
            let v = (i as f32 * 0.618_034).fract();
            let alpha = [1.0f32, 0.5, 0.25, 1.0, 0.8][(i % 5) as usize];
            [v * 6.0, 0.02 + v * 0.001, -0.5 + v * 900.0, alpha]
        })
        .collect();
    let document = float_doc(
        width,
        height,
        &IccProfile::srgb(),
        std::slice::from_ref(&pixels),
    );
    let request = ExportRequest::parse("{\"format\":\"hdr\",\"bit_depth\":32}").unwrap();
    let result = document
        .export_encoded(None, &request, LIMIT, 0, None)
        .unwrap();
    assert_eq!(result.info_json()["format"], "hdr");
    assert_eq!(result.info_json()["bit_depth"], 32);
    assert_eq!(result.info_json()["float_samples"], "rgbe");
    assert_eq!(result.info_json()["carried"], 0);
    let head = result.bytes().len().min(160);
    let header = String::from_utf8_lossy(&result.bytes()[..head]).to_string();
    assert!(
        header.contains("PRIMARIES= 0.64 0.33 0.3 0.6 0.15 0.06 0.3127 0.329\n"),
        "{header}"
    );
    let (plan, samples) = radiance_hdr::decode(result.bytes(), "export", u64::MAX).unwrap();
    assert_eq!((plan.width, plan.height), (width, height));
    assert_eq!(plan.primaries, Some(REC709));
    for (i, pixel) in pixels.iter().enumerate() {
        let want = [0, 1, 2].map(|c| pixel[c].max(0.0) * pixel[3]);
        let largest = want.iter().fold(0f32, |m, v| m.max(*v));
        // One mantissa step: 1/256 of the power of two above the largest
        // channel.
        let step = 2f32.powf(largest.log2().floor() + 1.0) / 256.0;
        for c in 0..3 {
            let got = samples[i * 3 + c];
            assert!(
                (got - want[c]).abs() <= step * 1.001,
                "{i}:{c}: {got} vs {} (step {step})",
                want[c]
            );
        }
    }
    // The reopened file is an opaque float document.
    let reopened = open(result.bytes()).unwrap();
    assert!(reopened.is_float());
    assert!(!warnings(&reopened).contains("PRIMARIES"));
    assert!(layer_pixels(&reopened).iter().all(|p| p[3] == 1.0));
}

#[test]
fn hdr_exists_only_at_thirty_two_bits_without_half_samples() {
    for (request, named) in [
        ("{\"format\":\"hdr\"}", "32-bit float only"),
        ("{\"format\":\"hdr\",\"bit_depth\":16}", "32-bit float only"),
        (
            "{\"format\":\"hdr\",\"bit_depth\":32,\"float_samples\":\"f16\"}",
            "requires bit_depth 32 and tiff or exr",
        ),
    ] {
        let error = ExportRequest::parse(request).err().unwrap_or_default();
        assert!(error.contains(named), "{request} -> {error}");
    }
}

/// The 8-bit open would clamp a float container, so it refuses both by name.
#[test]
fn the_flat_open_refuses_both_float_containers() {
    let directory = tempfile::TempDir::new().unwrap();
    let samples: Vec<f32> = (0..12).map(|i| i as f32 * 0.5).collect();
    let exr = file(&[Part::new(2, 2, float_channels(&["R", "G", "B"], &samples))]);
    for (name, bytes, named) in [
        ("image.hdr", standard(), "Radiance HDR"),
        ("image.exr", exr, "OpenEXR"),
    ] {
        let path = directory.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let mut err = ptr::null_mut();
        let doc = unsafe { dr_doc_open(cpath(&path).as_ptr(), &mut err) };
        assert!(doc.is_null(), "{name} must not open at 8 bits");
        let error = take_err_string(err);
        assert!(error.contains(named), "{error}");
        assert!(error.contains("32-bit float document"), "{error}");
    }
}
