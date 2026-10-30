//! Independent u16 coverage/native tests. Float comparisons use direct f64
//! coverage/source-over references; byte data never manufactures precision.
use crate::common;
use crate::group_roots;
use crate::precision_native_fixture as fixture;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_adjustment::PrecisionAdjustmentState;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::precision_render::{PrecisionLayer, PrecisionScene, MIN_RENDER_ALPHA};
use group_roots::GroupRoots;
use serde_json::json;
use std::sync::Arc;

use crate::identity;

/// Every native encode below goes through here. Phase 12E-1 made the
/// always-written identity wrapper the OUTERMOST `.rz` word and its `LIDS`
/// table the file's last block; this file's subject is an EARLIER wrapper,
/// so the outer one is asserted and then peeled, leaving byte-for-byte what
/// the writer these tests were written against produced.
fn encoded(doc: &PrecisionDocument) -> Vec<u8> {
    identity::typed_prior_body(&doc.encode_native(LIMIT, 0, None).unwrap())
}

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
fn plane(w: u32, h: u32, values: Vec<u16>) -> PrecisionPlane {
    PrecisionPlane::from_u16(w, h, values, None).unwrap()
}
fn mask(
    w: u32,
    h: u32,
    values: Vec<u16>,
    offset: (i32, i32),
    density: f32,
    feather: f32,
    enabled: bool,
) -> PrecisionMask {
    let mut p = MaskProperties::at(offset);
    p.density = density;
    p.feather = feather;
    PrecisionMask::new(plane(w, h, values), p, enabled).unwrap()
}
fn image(values: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(values.len() as u32, 1, values, IccProfile::srgb(), None).unwrap()
}
fn document(values: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(image(values), Metadata::default(), Resolution::default())
        .unwrap()
}
fn op() -> String {
    json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string()
}
fn execute(
    doc: &PrecisionDocument,
    value: serde_json::Value,
    samples: &[u16],
) -> Option<PrecisionDocument> {
    doc.mask_edit(
        &MaskEdit::parse(&value.to_string()).unwrap(),
        samples,
        LIMIT,
        0,
        None,
    )
    .unwrap()
}

#[test]
fn independent_raw_extent_density_and_disabled_mask_use_float_coverage() {
    let pixels = vec![[12001, 22002, 32003, 12345]; 5];
    for (density, enabled) in [(1.0, true), (0.625, true), (0.625, false)] {
        let raw = vec![1, 32769, 65535];
        let layer = PrecisionLayer::raster(image(pixels.clone()), (0, 0), 0.375)
            .unwrap()
            .with_mask(Some(mask(3, 1, raw.clone(), (1, 0), density, 0.0, enabled)));
        let scene =
            PrecisionScene::new(5, 1, IccProfile::srgb(), vec![layer], None, false).unwrap();
        let out = scene
            .render(PrecisionStorage::Rgba32Float, LIMIT, 0, None)
            .unwrap();
        for (x, pixel) in out.to_rgba32().ok().unwrap().iter().enumerate() {
            let coverage = if (1..=3).contains(&x) {
                f64::from(raw[x - 1]) / 65535.0
            } else {
                0.0
            };
            let coverage = if enabled {
                1.0 - f64::from(density) * (1.0 - coverage)
            } else {
                1.0
            };
            let alpha = f64::from(pixels[x][3]) / 65535.0 * 0.375 * coverage;
            assert!((f64::from(pixel[3]) - alpha).abs() < 2e-6);
            if alpha == 0.0 {
                assert_eq!(*pixel, [0.0; 4]);
            } else {
                for c in 0..3 {
                    assert!((f64::from(pixel[c]) - f64::from(pixels[x][c]) / 65535.0).abs() < 2e-6);
                }
            }
        }
    }
}

#[test]
fn tiny_u16_alpha_times_mask_and_opacity_is_supported_without_relaxing_error() {
    let source = image(vec![[30001, 30002, 30003, 1]]);
    let layer = PrecisionLayer::raster(source, (0, 0), 0.5)
        .unwrap()
        .with_mask(Some(mask(1, 1, vec![1], (0, 0), 1.0, 0.0, true)));
    let out = PrecisionScene::new(1, 1, IccProfile::srgb(), vec![layer], None, false)
        .unwrap()
        .render(PrecisionStorage::Rgba32Float, LIMIT, 0, None)
        .unwrap();
    let pixel = out.to_rgba32().ok().unwrap()[0];
    assert!(pixel[3] > 0.0);
    assert!((f64::from(pixel[3]) - 0.5 / 65535.0_f64.powi(2)).abs() < 1e-16);
    for (c, value) in pixel[..3].iter().enumerate() {
        assert!((f64::from(*value) - f64::from(30001 + c as u16) / 65535.0).abs() < 2e-6);
    }
    // Newly qualified floor keeps all RGB products normal; even a tiny RGB
    // component has an absolute error far below the existing float tolerance.
    for alpha in [MIN_RENDER_ALPHA, MIN_RENDER_ALPHA * 2.0] {
        let source = PrecisionImage::from_rgba32(
            1,
            1,
            vec![[1e-12, 0.375, 0.875, alpha]],
            IccProfile::srgb(),
            None,
        )
        .unwrap();
        let layer = PrecisionLayer::raster(source, (0, 0), 1.0).unwrap();
        let output = PrecisionScene::new(1, 1, IccProfile::srgb(), vec![layer], None, false)
            .unwrap()
            .render(PrecisionStorage::Rgba32Float, LIMIT, 0, None)
            .unwrap();
        let p = output.to_rgba32().ok().unwrap()[0];
        assert_eq!(p[3], alpha);
        for (a, b) in p[..3].iter().zip([1e-12, 0.375, 0.875]) {
            assert!((*a - b).abs() < 2e-6);
        }
    }
}

#[test]
fn adjustment_and_both_group_modes_use_the_same_mask_gate() {
    let source = image(vec![[10001, 20002, 30003, 32123]; 3]);
    let profile = source.profile().clone();
    let coverage = [1_u16, 32769, 65535];
    let adjustment = PrecisionLayer::adjustment(
        PrecisionAdjustmentState::new(&op(), &profile).unwrap(),
        0.625,
    )
    .unwrap();
    let mask = mask(3, 1, coverage.to_vec(), (0, 0), 1.0, 0.0, true);
    for mode in 0..3 {
        let base = PrecisionLayer::raster(source.clone(), (0, 0), 1.0).unwrap();
        let layers = match mode {
            0 => vec![base, adjustment.clone().with_mask(Some(mask.clone()))],
            1 => vec![
                PrecisionLayer::isolated_normal(vec![base, adjustment.clone()], 0.375)
                    .unwrap()
                    .with_mask(Some(mask.clone())),
            ],
            _ => vec![
                base,
                PrecisionLayer::pass_through(vec![adjustment.clone()], 1.0)
                    .unwrap()
                    .with_mask(Some(mask.clone())),
            ],
        };
        let output = PrecisionScene::new(3, 1, profile.clone(), layers, None, false)
            .unwrap()
            .render(PrecisionStorage::Rgba32Float, LIMIT, 0, None)
            .unwrap();
        for (x, pixel) in output.to_rgba32().ok().unwrap().iter().enumerate() {
            let m = f64::from(coverage[x]) / 65535.0;
            for (c, input) in [10001_u16, 20002, 30003].iter().enumerate() {
                let input = f64::from(*input) / 65535.0;
                let k = 0.625 * if mode == 1 { 1.0 } else { m };
                assert!((f64::from(pixel[c]) - (input + (1.0 - 2.0 * input) * k)).abs() < 2e-6);
            }
            let alpha = 32123.0 / 65535.0 * if mode == 1 { 0.375 * m } else { 1.0 };
            assert!((f64::from(pixel[3]) - alpha).abs() < 2e-6);
        }
    }
}

#[test]
fn feather_uses_independent_gaussian_and_never_quantizes_tiny_coverage() {
    let (w, h) = (9_u32, 7_u32);
    let profile = IccProfile::srgb();
    let pixels = vec![[18001, 28002, 38003, 65535]; (w * h) as usize];
    let source = PrecisionImage::from_rgba16(w, h, pixels, profile.clone(), None).unwrap();
    let raw = vec![1, 7, 10001, 32769, 65535, 13];
    // Feather radius3 => sigma1.4 and7 taps. Direct2D f64 convolution over
    // zero outside the independent3×2 raw rectangle, unlike clamped canvas.
    let layer = PrecisionLayer::raster(source, (0, 0), 1.0)
        .unwrap()
        .with_mask(Some(mask(3, 2, raw.clone(), (3, 2), 1.0, 3.0, true)));
    let scene = PrecisionScene::new(w, h, profile, vec![layer], None, false).unwrap();
    let out = scene
        .render(PrecisionStorage::Rgba32Float, LIMIT, 0, None)
        .unwrap();
    let weights: Vec<_> = (-3_i32..=3)
        .map(|v| (-0.5 * (f64::from(v) / 1.4).powi(2)).exp())
        .collect();
    let norm = weights.iter().sum::<f64>().powi(2);
    let mut positive_sub_byte = 0;
    for y in 0..h {
        for x in 0..w {
            let mut reference = 0.0;
            for dy in -3_i32..=3 {
                for dx in -3_i32..=3 {
                    let sx = x as i32 + dx - 3;
                    let sy = y as i32 + dy - 2;
                    if (0..3).contains(&sx) && (0..2).contains(&sy) {
                        reference += f64::from(raw[sy as usize * 3 + sx as usize]) / 65535.0
                            * weights[(dx + 3) as usize]
                            * weights[(dy + 3) as usize]
                            / norm;
                    }
                }
            }
            let actual = f64::from(out.sample_at(x, y).unwrap()[3]);
            assert!(
                (actual - reference).abs() < 2e-6,
                "{x},{y}: {actual} vs {reference}"
            );
            if reference > 0.0 && reference < 1.0 / 255.0 {
                positive_sub_byte += 1;
                assert!(actual > 0.0);
            }
        }
    }
    assert!(positive_sub_byte > 5);
}

#[test]
fn masks_channels_and_context_survive_edits_and_native13_with_old_reads() {
    let original = document(vec![[30001, 30002, 30003, 1], [40001, 40002, 40003, 0]]);
    let attached=execute(&original,json!({"op":"set_mask","layer":0,"width":3,"height":1,"x":-1,"linked":false,"density":0.625,"feather":2.0,"enabled":false}),&[1,32769,65535]).unwrap();
    let channelled = execute(
        &attached,
        json!({"op":"set_channel","index":null,"name":"Coverage"}),
        &[1, 65534],
    )
    .unwrap();
    let id = channelled.channels()[0].id();
    let adjusted = channelled
        .add_adjustment("Invert", &op(), 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let grouped = adjusted
        .group_all_pass_through(0.75)
        .unwrap()
        .unwrap()
        .set_layer_opacity(0, 0.5)
        .unwrap()
        .unwrap();
    let duplicated = grouped.duplicate_layer(0).unwrap().unwrap();
    assert_eq!(duplicated.channels()[0].id(), id);
    assert_eq!(duplicated.channels()[0].plane().samples(), &[1, 65534]);
    for index in [0, 1] {
        let mask = duplicated.layer(index).unwrap().mask().unwrap();
        assert_eq!(mask.plane().samples(), &[1, 32769, 65535]);
        assert_eq!(mask.properties().offset, (-1, 0));
        assert!(!mask.properties().linked);
        assert!(!mask.enabled());
    }
    let bytes = encoded(&duplicated);
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 13);
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(reopened.channels()[0].plane().samples(), &[1, 65534]);
    assert_ne!(reopened.channels()[0].id(), id);
    assert_eq!(
        reopened.render(LIMIT, 0, None).unwrap().to_rgba16().ok(),
        duplicated.render(LIMIT, 0, None).unwrap().to_rgba16().ok()
    );
    for version in [11, 12] {
        let bytes = fixture::image_file(
            version,
            &[[30001, 30002, 30003, 1]],
            if version == 12 { Some(op()) } else { None }.as_deref(),
        );
        let old = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
        assert!(old.channels().is_empty());
        assert!(old.layer(0).unwrap().mask().is_none());
        assert_eq!(
            old.layer_source(0).unwrap().to_rgba16().ok().unwrap()[0],
            [30001, 30002, 30003, 1]
        );
    }
    assert!(original.layer(0).unwrap().mask().is_none());
    assert!(original.channels().is_empty());
}

#[test]
fn noops_resource_refusals_and_bad_requests_preserve_every_handle() {
    let d = document(vec![[10001, 20002, 30003, 65535]; 4]);
    let edit = json!({"op":"set_mask","layer":0,"width":4,"height":1});
    let masked = execute(&d, edit.clone(), &[1, 2, 3, 4]).unwrap();
    assert!(execute(&masked, edit, &[1, 2, 3, 4]).is_none());
    assert!(execute(&d, json!({"op":"remove_mask","layer":0}), &[]).is_none());
    let with_channel = execute(
        &masked,
        json!({"op":"set_channel","name":"A"}),
        &[1, 2, 3, 4],
    )
    .unwrap();
    assert!(execute(
        &with_channel,
        json!({"op":"set_channel","index":0,"name":"A"}),
        &[1, 2, 3, 4]
    )
    .is_none());
    for invalid in [
        json!({"op":"set_mask","layer":0,"width":0,"height":1}),
        json!({"op":"set_mask","layer":0,"width":1,"height":1,"density":1.1}),
        json!({"op":"set_channel","index":1,"name":"Bad"}),
        json!({"op":"mask_properties","layer":0,"x":0}),
    ] {
        if let Ok(edit) = MaskEdit::parse(&invalid.to_string()) {
            assert!(edit.scratch_bytes(&d).is_err());
        }
    }
    assert!(MaskEdit::parse(r#"{"op":"remove_mask","layer":0,"unknown":true}"#).is_err());
    let edit = MaskEdit::parse(r#"{"op":"set_mask","layer":0,"width":4,"height":1}"#).unwrap();
    let scratch = edit.scratch_bytes(&d).unwrap();
    let token = DrJobControl::default();
    assert!(d
        .mask_edit(
            &edit,
            &[1, 2, 3, 4],
            d.retained_bytes() + scratch - 1,
            0,
            Some(&token)
        )
        .is_err());
    assert_eq!(token.scratch_bytes(), 0);
    let token = DrJobControl::default();
    token.cancel();
    assert!(d
        .mask_edit(&edit, &[1, 2, 3, 4], LIMIT, 0, Some(&token))
        .is_err());
    assert!(d.layer(0).unwrap().mask().is_none());
}

#[test]
fn raw_planes_preserve_every_u16_code_and_channel_edits_do_not_render() {
    let values: Vec<_> = (0..=u16::MAX).collect();
    let raw = plane(65536, 1, values.clone());
    assert_eq!(raw.samples(), values);
    let d = document(vec![[12345, 23456, 34567, 65535]; 65536]);
    let before = d.render(LIMIT, 0, None).unwrap();
    let c = execute(&d, json!({"op":"set_channel","name":"All codes"}), &values).unwrap();
    assert_eq!(
        c.render(LIMIT, 0, None).unwrap().to_rgba16().ok(),
        before.to_rgba16().ok()
    );
    let bytes = encoded(&c);
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(reopened.channels()[0].plane().samples(), values);
    let deleted = execute(&c, json!({"op":"remove_channel","index":0}), &[]).unwrap();
    assert!(deleted.channels().is_empty());
}

#[test]
fn positive_mask_opacity_underflow_refuses_in_raster_and_group_paths() {
    let opacity = f32::from_bits(1);
    for mode in 0..3 {
        let source = PrecisionLayer::raster(
            image(vec![[30001, 30002, 30003, 65535]]),
            (0, 0),
            if mode == 0 { opacity } else { 1.0 },
        )
        .unwrap();
        let layer = match mode {
            0 => source,
            1 => PrecisionLayer::isolated_normal(vec![source], opacity).unwrap(),
            _ => PrecisionLayer::pass_through(vec![source], opacity).unwrap(),
        }
        .with_mask(Some(mask(1, 1, vec![1], (0, 0), 1.0, 0.0, true)));
        let scene =
            PrecisionScene::new(1, 1, IccProfile::srgb(), vec![layer], None, false).unwrap();
        let token = DrJobControl::default();
        let error = scene
            .render(PrecisionStorage::Rgba32Float, LIMIT, 0, Some(&token))
            .err()
            .unwrap();
        assert!(error.contains("2^-96"), "{error}");
        assert_eq!(token.scratch_bytes(), 0);
    }
}

#[test]
fn native_scalar_framing_is_validated_before_any_raster_decode() {
    let d = document(vec![[10001, 20002, 30003, 65535]; 2]);
    let d = execute(
        &d,
        json!({"op":"set_mask","layer":0,"width":2,"height":1,"feather":2}),
        &[1, 65535],
    )
    .unwrap();
    let d = execute(
        &d,
        json!({"op":"set_channel","name":"Last channel"}),
        &[1, 65534],
    )
    .unwrap();
    let bytes = encoded(&d);
    let mut bad = bytes.clone();
    let png = bad
        .windows(8)
        .position(|v| v == b"\x89PNG\r\n\x1a\n")
        .unwrap();
    let png_length = u64::from_le_bytes(bad[png - 8..png].try_into().unwrap()) as usize;
    bad.drain(png + 33..png + png_length);
    bad[png - 8..png].copy_from_slice(&33_u64.to_le_bytes());
    // Final channel has two u16 samples. Framing must reject its wrong byte
    // length before the preceding header-only PNG is handed to a decoder.
    let length_offset = bad.len() - 12;
    bad[length_offset..length_offset + 8].copy_from_slice(&2_u64.to_le_bytes());
    let error = PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
        .err()
        .unwrap();
    assert!(error.contains("scalar byte length"), "{error}");
    let mut invalid_properties = bytes;
    let record = 32 + 4 + d.profile().bytes().len() + 8 + 3;
    let common = 20 + d.layer(0).unwrap().name().len();
    let density = record + common + 1 + 4 + 8 + 8;
    invalid_properties[density..density + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    let token = DrJobControl::default();
    let error = PrecisionDocument::parse_native(&invalid_properties, LIMIT, 0, Some(&token))
        .err()
        .unwrap();
    assert!(error.contains("mask properties"), "{error}");
    assert_eq!(token.scratch_bytes(), 0);
}

#[test]
fn active_float_feather_cancellation_releases_scratch_and_preserves_raw() {
    let (w, h) = (1024, 1024);
    let profile = IccProfile::srgb();
    let source = PrecisionImage::from_rgba16(
        w,
        h,
        vec![[12345, 23456, 34567, 65535]; (w * h) as usize],
        profile.clone(),
        None,
    )
    .unwrap();
    let mask = mask(w, h, vec![12345; (w * h) as usize], (0, 0), 1.0, 30.0, true);
    let captured = mask.clone();
    let layer = PrecisionLayer::raster(source, (0, 0), 1.0)
        .unwrap()
        .with_mask(Some(mask));
    let scene = PrecisionScene::new(w, h, profile, vec![layer], None, false).unwrap();
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match scene.render(PrecisionStorage::Rgba16Unorm, LIMIT, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= 0.1, render);
    let n0 = entry.checkpoints;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 3,
        "the entry's kernel polls fewer than three checkpoints before its next report"
    );
    let parked = cancelled_parked_at(
        n0 + 3,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    eprintln!("float feather cancellation: {}", parked_line(&parked));
    assert!(captured.plane().samples().iter().all(|v| *v == 12345));
}

mod bench {
    use super::*;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn active_float_feather_cancellation_releases_scratch_and_preserves_raw() {
        let (w, h) = (1024, 1024);
        let profile = IccProfile::srgb();
        let source = PrecisionImage::from_rgba16(
            w,
            h,
            vec![[12345, 23456, 34567, 65535]; (w * h) as usize],
            profile.clone(),
            None,
        )
        .unwrap();
        let mask = mask(w, h, vec![12345; (w * h) as usize], (0, 0), 1.0, 30.0, true);
        let captured = mask.clone();
        let layer = PrecisionLayer::raster(source, (0, 0), 1.0)
            .unwrap()
            .with_mask(Some(mask));
        let scene = PrecisionScene::new(w, h, profile, vec![layer], None, false).unwrap();
        let token = Arc::new(DrJobControl::default());
        let worker_token = token.clone();
        let worker = std::thread::spawn(move || {
            scene.render(PrecisionStorage::Rgba16Unorm, LIMIT, 0, Some(&worker_token))
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while token.progress() < 0.1 && !worker.is_finished() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!worker.is_finished(), "must reach active feather work");
        let start = std::time::Instant::now();
        token.cancel();
        assert!(worker.join().unwrap().err().unwrap().contains("cancelled"));
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(token.scratch_bytes(), 0);
        assert!(captured.plane().samples().iter().all(|v| *v == 12345));
    }
}
