//! #1 spot channels 1c-1 (`spot_merge`, `ffi_spot_merge`): Merge Spot
//! Channel and the alpha ↔ spot kind change against hand oracles written
//! here. The overprint oracle is, per plane on unit values over an opaque
//! composite, `(1 − w)·c + w·((1 − s)·c·ink + s·ink)` with the ink from the
//! core's own encoding seams (`spot_view::ink_encoded` in RGB,
//! `spot_cmyk::spot_ink_cmyk` in CMYK); Gray's ink is L* alone through the
//! Gray Gamma 2.2 curve, computed here; CMYK's plates run on `L = 1 − ink`.
//! The merge at 8 bits (two layers become one, the channel goes, every
//! other channel stays), 16 bits, CMYK, Gray 16 and Lab 16 (coverage-0
//! pixels bit-exact, one of them outside ProPhoto; covered pixels equal the
//! RGB oracle taken through the same two public conversions), the Duotone
//! refusal, and the kind change's `max − v` round trip at both depths.

use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_spot_merge::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::spot_channel::{SpotInk, DUOTONE_MERGE_REFUSAL, NOT_SPOT_REFUSAL};
use doctored_core::spot_cmyk::spot_ink_cmyk;
use doctored_core::spot_view::ink_encoded;
use image::{Rgba, RgbaImage};
use serde_json::Value;

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::duotone_document::{duotone, monotone_black};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 4;
const H: u32 = 3;

fn red(solidity: u8) -> SpotInk {
    SpotInk::new([54.29, 80.81, 69.89], solidity).unwrap()
}

/// Twelve coverage values, 0 and 255 among them.
fn coverage8() -> Vec<u8> {
    vec![0, 255, 17, 51, 0, 128, 200, 254, 1, 77, 160, 0]
}

fn coverage16() -> Vec<u16> {
    coverage8().iter().map(|&c| u16::from(c) * 257).collect()
}

/// The overprint oracle on one unit plane value over an opaque pixel.
fn oracle(c: f64, w: f64, s: f64, ink: f64) -> f64 {
    (1.0 - w) * c + w * ((1.0 - s) * c * ink + s * ink)
}

fn base8() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 60 + 13) as u8, (y * 80 + 7) as u8, 201, 255])
    })
}

/// A half-transparent top layer over the middle row.
fn top8() -> RgbaImage {
    RgbaImage::from_fn(W, H, |_, y| {
        if y == 1 {
            Rgba([250, 240, 10, 128])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

#[test]
fn a_byte_merge_flattens_and_bakes_one_ink() {
    let ink = red(60);
    let alpha_plane: Vec<u8> = (0..W * H).map(|i| (i * 20) as u8).collect();
    let doc = DrDocument::from_pixels(base8())
        .adding_image_layer(0, top8(), "Top")
        .unwrap()
        .add_channel("Alpha 1", &alpha_plane, W, H, [255, 0, 0], 0.5)
        .unwrap()
        .add_spot_channel("Red", ink, &coverage8())
        .unwrap();
    assert_eq!(doc.layers.len(), 2);
    let flat = doc.flattened().unwrap();
    // Out of range and an alpha channel change nothing.
    assert!(doc.merge_spot_channel(2).unwrap().is_none());
    assert!(doc.merge_spot_channel(0).unwrap().is_none());
    let merged = doc.merge_spot_channel(1).unwrap().unwrap();
    assert_eq!(merged.layers.len(), 1, "flattened");
    assert_eq!(merged.channels.len(), 1, "the spot channel is gone");
    assert_eq!(merged.channels[0].name, "Alpha 1");
    assert_eq!(merged.channel_plane(0).unwrap(), alpha_plane.as_slice());
    let encoded = ink_encoded(&ink, Some(&IccProfile::srgb())).unwrap();
    let out = merged.flattened().unwrap().into_raw();
    let before = flat.into_raw();
    for (i, &cov) in coverage8().iter().enumerate() {
        let w = f64::from(cov) / 255.0;
        for c in 0..3 {
            let unit = f64::from(before[i * 4 + c]) / 255.0;
            let expected = oracle(unit, w, 0.6, f64::from(encoded[c])) * 255.0;
            let got = f64::from(out[i * 4 + c]);
            if cov == 0 {
                assert_eq!(out[i * 4 + c], before[i * 4 + c], "px {i} c {c} untouched");
            } else {
                assert!(
                    (got - expected).abs() <= 1.0,
                    "px {i} c {c}: {got} vs {expected}"
                );
            }
        }
        assert_eq!(out[i * 4 + 3], 255);
    }
    // The FFI: one export, NULL for an alpha channel, the input untouched.
    let mut err = ptr::null_mut();
    let via = unsafe { dr_doc_merge_spot_channel(&doc, 1, &mut err) };
    assert!(!via.is_null(), "{}", take_err_string(err));
    assert_eq!(unsafe { &*via }.flattened().unwrap().into_raw(), out);
    assert!(unsafe { dr_doc_merge_spot_channel(&doc, 0, &mut err) }.is_null());
    assert!(err.is_null());
    assert_eq!(doc.layers.len(), 2);
    unsafe { dr_doc_free(via) };
}

fn rgb16_image(seed: u16, alpha: u16) -> PrecisionImage {
    let pixels = (0..W * H)
        .map(|i| {
            let i = i as u16;
            [i * 3000 + seed, 40000 - seed, 60000 - i * 2000, alpha]
        })
        .collect();
    PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap()
}

fn rgb16_layered() -> PrecisionDocument {
    let bottom =
        PrecisionDocumentLayer::raster("Background", rgb16_image(0, 65535), (0, 0), 1.0, true)
            .unwrap();
    let top =
        PrecisionDocumentLayer::raster("Top", rgb16_image(900, 30000), (0, 0), 0.8, true).unwrap();
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![bottom, top],
    )
    .unwrap()
}

fn render16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn plan(doc: &PrecisionDocument, index: usize) -> Value {
    doc.merge_spot_channel_plan(index).unwrap()
}

/// Every pixel at most `bound` codes from the 16-bit oracle; coverage 0
/// bit-exact.
fn assert_rgb_oracle(
    before: &[[u16; 4]],
    after: &[[u16; 4]],
    ink: [f32; 3],
    solidity: f64,
    bound: f64,
) {
    for (i, &cov) in coverage16().iter().enumerate() {
        let w = f64::from(cov) / 65535.0;
        if cov == 0 {
            assert_eq!(after[i], before[i], "px {i} untouched");
            continue;
        }
        for c in 0..3 {
            let unit = f64::from(before[i][c]) / 65535.0;
            let expected = oracle(unit, w, solidity, f64::from(ink[c])) * 65535.0;
            let got = f64::from(after[i][c]);
            assert!(
                (got - expected).abs() <= bound,
                "px {i} c {c}: {got} vs {expected}"
            );
        }
        assert_eq!(after[i][3], 65535, "px {i}");
    }
}

#[test]
fn a_typed_rgb16_merge_flattens_and_bakes_one_ink() {
    let ink = red(25);
    let doc = rgb16_layered()
        .add_spot_channel("Red", ink, &coverage16(), LIMIT, 0, None)
        .unwrap();
    let value = plan(&doc, 0);
    assert!(value["refusal"].is_null(), "{value}");
    assert!(value["working_memory_plan_bytes"].as_u64().unwrap() > 0);
    let before = render16(&doc);
    let merged = doc.merge_spot_channel(0, LIMIT, 0, None).unwrap();
    assert_eq!(merged.layer_count(), 1, "flattened");
    assert!(merged.channels().is_empty(), "the spot channel is gone");
    let encoded = ink_encoded(&ink, Some(&IccProfile::srgb())).unwrap();
    assert_rgb_oracle(&before, &render16(&merged), encoded, 0.25, 1.0);
    // An admission refusal allocates nothing and says so.
    assert!(doc.merge_spot_channel(0, 1024, 0, None).is_err());
}

#[test]
fn a_typed_merge_refuses_an_alpha_channel_and_out_of_range_by_name() {
    let alpha = MaskEdit::SetChannel {
        index: None,
        name: "Alpha 1".into(),
    };
    let doc = rgb16_layered()
        .mask_edit(&alpha, &coverage16(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(plan(&doc, 0)["refusal"], NOT_SPOT_REFUSAL);
    assert_eq!(
        doc.merge_spot_channel(0, LIMIT, 0, None).err().unwrap(),
        NOT_SPOT_REFUSAL
    );
    assert_eq!(
        plan(&doc, 4)["refusal"],
        "typed channel index is out of range"
    );
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_merge_spot_channel(&doc, 0, LIMIT, 0, ptr::null(), &mut err) };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), NOT_SPOT_REFUSAL);
}

#[test]
fn a_cmyk_merge_overprints_in_ink_on_every_plate() {
    let rgb = PrecisionDocument::from_image(
        rgb16_image(0, 65535),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let profile = press_profile("Press Test");
    let cmyk = rgb
        .convert_to_cmyk(Some(&profile), "{}", LIMIT, 0, None)
        .unwrap();
    let ink = SpotInk::new([62.0, 30.0, -45.0], 40).unwrap();
    let doc = cmyk
        .add_spot_channel("Violet", ink, &coverage16(), LIMIT, 0, None)
        .unwrap();
    let plates = spot_ink_cmyk(&doc, 0).unwrap();
    assert!(plates.iter().any(|&v| v > 0.05), "{plates:?}");
    let before = doc.cmyk_layer_samples(0).unwrap();
    let merged = doc.merge_spot_channel(0, LIMIT, 0, None).unwrap();
    assert!(merged.is_cmyk() && merged.channels().is_empty());
    assert_eq!(merged.layer_count(), 1);
    let after = merged.cmyk_layer_samples(0).unwrap();
    let s = 0.4;
    for (i, &cov) in coverage16().iter().enumerate() {
        assert_eq!(before[i][4], 65535, "an opaque composite");
        if cov == 0 {
            assert_eq!(after[i], before[i], "px {i} untouched");
            continue;
        }
        let w = f64::from(cov) / 65535.0;
        for plate in 0..4 {
            let paper = 1.0 - f64::from(before[i][plate]) / 65535.0;
            let light = 1.0 - f64::from(plates[plate]);
            let expected = 65535.0 * (1.0 - oracle(paper, w, s, light));
            let got = f64::from(after[i][plate]);
            assert!(
                (got - expected).abs() <= 1.0,
                "px {i} plate {plate}: {got} vs {expected}"
            );
        }
        assert_eq!(after[i][4], 65535);
    }
}

#[test]
fn a_read_only_cmyk_profile_refuses_the_ink_instead_of_printing_nothing() {
    // A CMYK document may hold a profile with no B2A table; a spot ink has no
    // CMYK there, so the merge and a kept export refuse by sentence and the
    // channel stays.
    let dir = tempfile::TempDir::new().unwrap();
    let data: Vec<u8> = (0..W * H)
        .flat_map(|i| [(i * 4000) as u16, 20000, 9000, (i * 1000) as u16])
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let profile = crate::cmyk_fixture::press_profile_without_b2a("Read Only");
    let path = dir.path().join("read-only.tif");
    std::fs::write(
        &path,
        crate::cmyk_tiff_builder::cmyk_tiff(W, H, 16, 4, 0, &data, Some(&profile)),
    )
    .unwrap();
    let c = crate::common::cpath(&path);
    let mut err = ptr::null_mut();
    let opened = unsafe {
        doctored_core::ffi_precision::dr_precision_doc_open(
            c.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!opened.is_null(), "{}", take_err_string(err));
    let cmyk = unsafe { Box::from_raw(opened) };
    assert!(cmyk.is_cmyk());
    let doc = cmyk
        .add_spot_channel("Violet", red(40), &coverage16(), LIMIT, 0, None)
        .unwrap();
    let sentence = spot_ink_cmyk(&doc, 0).unwrap_err();
    assert!(
        sentence.contains("Read Only") && sentence.contains("B2A"),
        "{sentence}"
    );
    assert_eq!(
        doc.merge_spot_channel(0, LIMIT, 0, None).err().unwrap(),
        sentence
    );
    let jpeg = doctored_core::doc_export::ExportRequest::parse(r#"{"format":"jpeg"}"#).unwrap();
    assert_eq!(
        doc.export_encoded(None, &jpeg, LIMIT, 0, None)
            .err()
            .unwrap(),
        sentence
    );
    assert_eq!(doc.channels().len(), 1, "the channel stays");
}

/// Gray Gamma 2.2's code of L* alone: Y from the CIE inverse, then Y^(1/2.2).
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
fn a_gray16_merge_bakes_the_inks_lightness_as_gray() {
    let rgb = PrecisionDocument::from_image(
        rgb16_image(0, 65535),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let gray = rgb
        .convert_to_gray(
            None,
            r#"{"space":"gray_gamma_22","depth":16}"#,
            LIMIT,
            0,
            None,
        )
        .unwrap();
    // A saturated ink: only its L* (50) is read.
    let ink = SpotInk::new([50.0, 60.0, -40.0], 30).unwrap();
    let doc = gray
        .add_spot_channel("Spot", ink, &coverage16(), LIMIT, 0, None)
        .unwrap();
    let before = render16(&doc);
    let merged = doc.merge_spot_channel(0, LIMIT, 0, None).unwrap();
    assert!(merged.is_gray() && merged.channels().is_empty());
    let after = render16(&merged);
    let g = gray_ink(50.0) as f32;
    // Declared before measuring: 4 codes at 16 bits, the Gray Gamma 2.2
    // table's inversion against the exact power.
    assert_rgb_oracle(&before, &after, [g; 3], 0.3, 4.0);
    for pixel in &after {
        assert!(pixel[0] == pixel[1] && pixel[1] == pixel[2], "{pixel:?}");
    }
}

/// The Lab twin's 16-bit codes of (L*, a*, b*).
fn lab_codes(lab: [f64; 3]) -> [u16; 4] {
    let unit = |v: f64| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    [
        unit(lab[0] / 100.0),
        unit((lab[1] + 128.0) / 255.0),
        unit((lab[2] + 128.0) / 255.0),
        65535,
    ]
}

#[test]
fn a_lab16_merge_keeps_uncovered_codes_exact_and_overprints_through_prophoto() {
    let twin = PrecisionDocument::from_image(
        rgb16_image(0, 65535),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
    .convert_to_lab(r#"{"depth":16}"#, LIMIT, 0, None)
    .unwrap()
    .profile()
    .clone();
    // Pixel 0 (coverage 0) is far outside ProPhoto: L* 98 with a* −128 and
    // b* 127 is no colour any RGB space holds.
    let outside = lab_codes([98.0, -128.0, 127.0]);
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| match i {
            0 => outside,
            _ => lab_codes([20.0 + f64::from(i) * 6.0, f64::from(i) * 4.0 - 20.0, 15.0]),
        })
        .collect();
    let lab = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(W, H, pixels.clone(), twin, None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
    .with_lab_depth(16)
    .unwrap();
    let prophoto = IccProfile::prophoto_rgb();
    // The round trip alone moves pixel 0: it is outside ProPhoto.
    let rgb = lab
        .convert_lab_to_rgb(prophoto.bytes(), "", LIMIT, 0, None)
        .unwrap();
    let rgb_samples = render16(&rgb);
    let back = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(W, H, rgb_samples.clone(), prophoto.clone(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
    .convert_to_lab(r#"{"depth":16}"#, LIMIT, 0, None)
    .unwrap();
    assert_ne!(render16(&back)[0], outside, "the round trip clips pixel 0");

    let ink = SpotInk::new([45.0, 50.0, 20.0], 50).unwrap();
    let doc = lab
        .add_spot_channel("Spot", ink, &coverage16(), LIMIT, 0, None)
        .unwrap();
    let merged = doc.merge_spot_channel(0, LIMIT, 0, None).unwrap();
    assert!(merged.is_lab() && merged.channels().is_empty());
    let after = render16(&merged);

    // The oracle: the RGB rule on the ProPhoto samples, then back to Lab.
    let encoded = ink_encoded(&ink, Some(&prophoto)).unwrap();
    let printed: Vec<[u16; 4]> = rgb_samples
        .iter()
        .zip(coverage16())
        .map(|(px, cov)| {
            let w = f64::from(cov) / 65535.0;
            let mut out = *px;
            for c in 0..3 {
                let unit = f64::from(px[c]) / 65535.0;
                out[c] = (oracle(unit, w, 0.5, f64::from(encoded[c])) * 65535.0).round() as u16;
            }
            out
        })
        .collect();
    let expected = render16(
        &PrecisionDocument::from_image(
            PrecisionImage::from_rgba16(W, H, printed, prophoto, None).unwrap(),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap()
        .convert_to_lab(r#"{"depth":16}"#, LIMIT, 0, None)
        .unwrap(),
    );
    for (i, &cov) in coverage16().iter().enumerate() {
        if cov == 0 {
            assert_eq!(after[i], pixels[i], "px {i} bit-exact");
            continue;
        }
        assert_ne!(after[i], pixels[i], "px {i} overprinted");
        for c in 0..4 {
            let d = (i32::from(after[i][c]) - i32::from(expected[i][c])).abs();
            assert!(d <= 2, "px {i} c {c}: {:?} vs {:?}", after[i], expected[i]);
        }
    }
}

#[test]
fn a_duotone_merge_refuses_by_name() {
    let gray = crate::duotone_document::gray_doc(&[0, 9000, 30000, 65535], 16, None);
    let duo = duotone(gray, &monotone_black());
    let doc = unsafe { &*duo }
        .add_spot_channel("Spot", red(0), &[65535, 0, 30000, 1], LIMIT, 0, None)
        .unwrap();
    assert_eq!(plan(&doc, 0)["refusal"], DUOTONE_MERGE_REFUSAL);
    assert_eq!(
        doc.merge_spot_channel(0, LIMIT, 0, None).err().unwrap(),
        DUOTONE_MERGE_REFUSAL
    );
    crate::duotone_document::free(duo);
    crate::duotone_document::free(gray);
}

#[test]
fn a_byte_kind_change_inverts_the_plane_both_ways() {
    let plane: Vec<u8> = (0..W * H).map(|i| (i * 21) as u8).collect();
    let doc = DrDocument::from_pixels(base8())
        .add_channel("Alpha 1", &plane, W, H, [255, 0, 0], 0.5)
        .unwrap();
    let ink = red(35);
    assert!(doc.set_channel_kind(0, None).is_none(), "already alpha");
    assert!(doc.set_channel_kind(1, Some(ink)).is_none(), "out of range");
    let spot = doc.set_channel_kind(0, Some(ink)).unwrap();
    assert_eq!(spot.channel_spot(0), Some(ink));
    let inverted: Vec<u8> = plane.iter().map(|v| 255 - v).collect();
    assert_eq!(spot.channel_plane(0).unwrap(), inverted.as_slice());
    assert!(
        spot.set_channel_kind(0, Some(red(90))).is_none(),
        "already spot"
    );
    let back = spot.set_channel_kind(0, None).unwrap();
    assert_eq!(back.channel_spot(0), None);
    assert_eq!(back.channel_plane(0).unwrap(), plane.as_slice());
    assert_eq!(back.channels[0].overlay_color, [255, 0, 0], "rubylith kept");
    // The FFI: spot false ignores the ink; a bad ink names itself.
    let mut err = ptr::null_mut();
    let via = unsafe { dr_doc_set_channel_kind(&spot, 0, false, 999.0, 0.0, 0.0, 200, &mut err) };
    assert!(!via.is_null(), "{}", take_err_string(err));
    assert_eq!(unsafe { &*via }.channel_plane(0).unwrap(), plane.as_slice());
    unsafe { dr_doc_free(via) };
    assert!(
        unsafe { dr_doc_set_channel_kind(&doc, 0, true, 999.0, 0.0, 0.0, 0, &mut err) }.is_null()
    );
    assert!(!take_err_string(err).is_empty());
}

#[test]
fn a_typed_kind_change_inverts_the_plane_both_ways() {
    let alpha = MaskEdit::SetChannel {
        index: None,
        name: "Alpha 1".into(),
    };
    let plane = coverage16();
    let doc = rgb16_layered()
        .mask_edit(&alpha, &plane, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let ink = red(35);
    assert!(doc.set_channel_kind(0, None, LIMIT, 0).unwrap().is_none());
    assert_eq!(
        doc.set_channel_kind(1, Some(ink), LIMIT, 0).err().unwrap(),
        "typed channel index is out of range"
    );
    let spot = doc
        .set_channel_kind(0, Some(ink), LIMIT, 0)
        .unwrap()
        .unwrap();
    assert_eq!(spot.channels()[0].metadata().spot, Some(ink));
    let inverted: Vec<u16> = plane.iter().map(|v| 65535 - v).collect();
    assert_eq!(spot.channels()[0].plane().samples(), inverted.as_slice());
    assert_eq!(spot.channels()[0].name(), "Alpha 1");
    let back = spot.set_channel_kind(0, None, LIMIT, 0).unwrap().unwrap();
    assert_eq!(back.channels()[0].metadata().spot, None);
    assert_eq!(back.channels()[0].plane().samples(), plane.as_slice());
    let mut err = ptr::null_mut();
    let via = unsafe {
        dr_precision_doc_set_channel_kind(&spot, 0, true, 50.0, 0.0, 0.0, 0, LIMIT, 0, &mut err)
    };
    assert!(
        via.is_null() && err.is_null(),
        "already spot: NULL, no error"
    );
}
