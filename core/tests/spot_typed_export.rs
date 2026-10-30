//! #1 spot channels 1c-1 (`spot_typed`, `spot_cmyk`, `spot_view`): the
//! flattened typed exports composite the overprint, against hand oracles
//! written here (per plane on unit values over an opaque composite,
//! `(1 − w)·c + w·((1 − s)·c·ink + s·ink)`, the ink from the core's encoding
//! seams; CMYK on `L = 1 − ink` per plate): an RGB16 PNG16 read back, a
//! Gray16 kept PNG read back as one channel, a CMYK kept-JPEG intermediate
//! read back as CMYK; a layer export carries no overprint; and a `region` /
//! `preview_divisor` overprint equals the full overprint's samples exactly
//! at divisors 1 and 2, with its refusals.

use std::io::Cursor;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportRequest;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_spot_channel::{dr_doc_spot_overprint, dr_image_spot_overprint};
use doctored_core::ffi_spot_merge::{dr_doc_spot_overprint_region, dr_image_spot_overprint_region};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::spot_channel::SpotInk;
use doctored_core::spot_cmyk::spot_ink_cmyk;
use doctored_core::spot_view::ink_encoded;
use doctored_core::DrImage;
use image::{DynamicImage, Rgba, RgbaImage};
use tiff::decoder::{Decoder, DecodingResult};
use tiff::ColorType;

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 4;
const H: u32 = 3;

fn coverage8() -> Vec<u8> {
    vec![0, 255, 17, 51, 0, 128, 200, 254, 1, 77, 160, 0]
}

fn coverage16() -> Vec<u16> {
    coverage8().iter().map(|&c| u16::from(c) * 257).collect()
}

fn oracle(c: f64, w: f64, s: f64, ink: f64) -> f64 {
    (1.0 - w) * c + w * ((1.0 - s) * c * ink + s * ink)
}

fn rgb16() -> PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| [(i * 3000) as u16, 40000, (60000 - i * 2000) as u16, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

fn render16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn request(text: &str) -> ExportRequest {
    ExportRequest::parse(text).unwrap()
}

fn png16() -> ExportRequest {
    request(r#"{"format":"png","bit_depth":16}"#)
}

/// Each plane of `got` within `bound` codes of the oracle over `before`
/// (the first `planes` components), coverage 0 bit-exact.
fn assert_planes(before: &[Vec<u16>], got: &[Vec<u16>], ink: &[f64], s: f64, bound: f64) {
    for (i, &cov) in coverage16().iter().enumerate() {
        let w = f64::from(cov) / 65535.0;
        for (c, &ink) in ink.iter().enumerate() {
            if cov == 0 {
                assert_eq!(got[i][c], before[i][c], "px {i} c {c} untouched");
                continue;
            }
            let unit = f64::from(before[i][c]) / 65535.0;
            let expected = oracle(unit, w, s, ink) * 65535.0;
            let value = f64::from(got[i][c]);
            assert!(
                (value - expected).abs() <= bound,
                "px {i} c {c}: {value} vs {expected}"
            );
        }
    }
}

#[test]
fn an_rgb16_png16_export_composites_the_overprint() {
    let ink = SpotInk::new([54.29, 80.81, 69.89], 60).unwrap();
    let doc = rgb16()
        .add_spot_channel("Red", ink, &coverage16(), LIMIT, 0, None)
        .unwrap();
    let before: Vec<Vec<u16>> = render16(&doc).iter().map(|p| p.to_vec()).collect();
    let plan = doc.export_plan(None, &png16()).unwrap();
    let bare = rgb16().export_plan(None, &png16()).unwrap();
    assert!(
        plan["scratch_bytes"].as_u64() > bare["scratch_bytes"].as_u64(),
        "the plan charges the overprint"
    );
    let result = doc.export_encoded(None, &png16(), LIMIT, 0, None).unwrap();
    let read = image::load_from_memory(result.bytes()).unwrap();
    let DynamicImage::ImageRgba16(read) = read else {
        panic!("a 16-bit RGBA PNG")
    };
    let got: Vec<Vec<u16>> = read.pixels().map(|p| p.0.to_vec()).collect();
    let encoded = ink_encoded(&ink, Some(&IccProfile::srgb())).unwrap();
    let ink: Vec<f64> = encoded.iter().map(|&v| f64::from(v)).collect();
    assert_planes(&before, &got, &ink, 0.6, 1.0);
    // A layer export renders the layer alone and never overprints.
    let layer = doc
        .export_layer_encoded(0, false, &png16(), LIMIT, 0, None)
        .unwrap();
    let DynamicImage::ImageRgba16(layer) = image::load_from_memory(layer.bytes()).unwrap() else {
        panic!("a 16-bit RGBA PNG")
    };
    let plain: Vec<Vec<u16>> = layer.pixels().map(|p| p.0.to_vec()).collect();
    assert_eq!(plain, before, "no overprint in a layer export");
}

/// Gray Gamma 2.2's code of L* alone.
fn gray_ink(l_star: f64) -> f64 {
    let fy = (l_star + 16.0) / 116.0;
    let y = if fy.powi(3) > 216.0 / 24389.0 {
        fy.powi(3)
    } else {
        l_star * 27.0 / 24389.0
    };
    y.powf(1.0 / 2.2)
}

#[test]
fn a_gray16_kept_png_composites_the_inks_gray() {
    let gray = rgb16()
        .convert_to_gray(
            None,
            r#"{"space":"gray_gamma_22","depth":16}"#,
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let ink = SpotInk::new([35.0, -40.0, 60.0], 0).unwrap();
    let doc = gray
        .add_spot_channel("Spot", ink, &coverage16(), LIMIT, 0, None)
        .unwrap();
    let before: Vec<Vec<u16>> = render16(&doc).iter().map(|p| vec![p[0]]).collect();
    let result = doc.export_encoded(None, &png16(), LIMIT, 0, None).unwrap();
    let DynamicImage::ImageLuma16(read) = image::load_from_memory(result.bytes()).unwrap() else {
        panic!("a kept 16-bit Gray PNG")
    };
    let got: Vec<Vec<u16>> = read.pixels().map(|p| vec![p.0[0]]).collect();
    // Declared before measuring: 4 codes, the Gray Gamma 2.2 table's
    // inversion against the exact power.
    assert_planes(&before, &got, &[gray_ink(35.0)], 0.0, 4.0);
}

#[test]
fn a_cmyk_kept_jpeg_intermediate_carries_the_ink_on_paper() {
    let profile = press_profile("Press Test");
    let cmyk = rgb16()
        .convert_to_cmyk(Some(&profile), "{}", LIMIT, 0, None)
        .unwrap();
    let ink = SpotInk::new([62.0, 30.0, -45.0], 70).unwrap();
    let doc = cmyk
        .add_spot_channel("Violet", ink, &coverage16(), LIMIT, 0, None)
        .unwrap();
    let plates = spot_ink_cmyk(&doc, 0).unwrap();
    let before = doc.cmyk_layer_samples(0).unwrap();
    let jpeg = request(r#"{"format":"jpeg"}"#);
    let result = doc.export_encoded(None, &jpeg, LIMIT, 0, None).unwrap();
    assert!(result.is_host_pending());
    let mut decoder = Decoder::new(Cursor::new(result.bytes())).unwrap();
    assert_eq!(decoder.colortype().unwrap(), ColorType::CMYK(8));
    let DecodingResult::U8(samples) = decoder.read_image().unwrap() else {
        panic!("8-bit CMYK samples")
    };
    for (i, &cov) in coverage16().iter().enumerate() {
        let w = f64::from(cov) / 65535.0;
        for plate in 0..4 {
            let paper = 1.0 - f64::from(before[i][plate]) / 65535.0;
            let light = 1.0 - f64::from(plates[plate]);
            let wide = (65535.0 * (1.0 - oracle(paper, w, 0.7, light))).round();
            let expected = (wide as u32 + 128) / 257;
            let got = u32::from(samples[i * 4 + plate]);
            assert!(
                got.abs_diff(expected) <= 1,
                "px {i} plate {plate}: {got} vs {expected}"
            );
        }
    }
    // With a spot channel the kept export takes the staged branch, whose
    // plan charges the on-paper image the overprint fills
    // (`cmyk_export_ink`).
    let spotted = doc.export_plan(None, &jpeg).unwrap();
    assert!(spotted["scratch_bytes"].as_u64().unwrap() >= u64::from(W * H) * 10);
}

fn base8() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 60 + 13) as u8, (y * 80 + 7) as u8, 201, 255])
    })
}

fn image_of(pixels: &RgbaImage) -> *mut DrImage {
    unsafe { dr_image_from_rgba(pixels.as_raw().as_ptr(), pixels.width(), pixels.height()) }
}

fn read(image: *mut DrImage, w: u32, h: u32) -> Vec<u8> {
    let out =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), (w * h * 4) as usize) }
            .to_vec();
    unsafe { dr_image_free(image) };
    out
}

/// The `(x, y, divisor)` region of `full` (`w`×`h` output), sampled.
fn sampled(full: &[u8], x: u32, y: u32, d: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for j in 0..h {
        for i in 0..w {
            let at = (((y + j * d) * W + x + i * d) * 4) as usize;
            out.extend_from_slice(&full[at..at + 4]);
        }
    }
    out
}

#[test]
fn a_region_overprint_is_the_full_overprints_samples() {
    let ink = SpotInk::new([54.29, 80.81, 69.89], 40).unwrap();
    let blue = SpotInk::new([30.0, 20.0, -60.0], 0).unwrap();
    let doc = DrDocument::from_pixels(base8())
        .add_spot_channel("Red", ink, &coverage8())
        .unwrap()
        .add_spot_channel(
            "Blue",
            blue,
            &coverage8().iter().rev().copied().collect::<Vec<_>>(),
        )
        .unwrap();
    let mut err = ptr::null_mut();
    let source = image_of(&base8());
    let full = read(
        unsafe { dr_doc_spot_overprint(&doc, source, &mut err) },
        W,
        H,
    );
    unsafe { dr_image_free(source) };
    let icc: &[u8] = &[];
    let one = read(
        unsafe {
            let source = image_of(&base8());
            let out = dr_image_spot_overprint(
                source,
                coverage8().as_ptr(),
                12,
                54.29,
                80.81,
                69.89,
                40,
                icc.as_ptr(),
                0,
                &mut err,
            );
            dr_image_free(source);
            out
        },
        W,
        H,
    );
    for (x, y, d, w, h) in [
        (0, 0, 1, W, H),
        (1, 1, 1, 2, 2),
        (0, 0, 2, 2, 2),
        (1, 0, 2, 2, 2),
    ] {
        let crop = RgbaImage::from_fn(w, h, |i, j| *base8().get_pixel(x + i * d, y + j * d));
        let region = image_of(&crop);
        let out = unsafe { dr_doc_spot_overprint_region(&doc, region, x, y, d, &mut err) };
        assert!(!out.is_null(), "{}", take_err_string(err));
        assert_eq!(
            read(out, w, h),
            sampled(&full, x, y, d, w, h),
            "doc ({x},{y}) /{d}"
        );
        let out = unsafe {
            dr_image_spot_overprint_region(
                region,
                coverage8().as_ptr(),
                12,
                W,
                H,
                x,
                y,
                d,
                54.29,
                80.81,
                69.89,
                40,
                icc.as_ptr(),
                0,
                &mut err,
            )
        };
        assert!(!out.is_null(), "{}", take_err_string(err));
        assert_eq!(
            read(out, w, h),
            sampled(&one, x, y, d, w, h),
            "image ({x},{y}) /{d}"
        );
        unsafe { dr_image_free(region) };
    }
    // A region past the canvas and a zero divisor refuse by sentence.
    let crop = RgbaImage::from_pixel(2, 2, Rgba([9, 9, 9, 255]));
    let region = image_of(&crop);
    for (x, d) in [(3, 1), (0, 0), (2, 2)] {
        let out = unsafe { dr_doc_spot_overprint_region(&doc, region, x, 0, d, &mut err) };
        assert!(out.is_null(), "({x}) /{d}");
        assert!(!take_err_string(err).is_empty(), "({x}) /{d}");
    }
    unsafe { dr_image_free(region) };
}
