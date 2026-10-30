//! Explicit flattened conversion, independent integer quantization, metadata
//! retention, native reopening and immutable cancellation/resource refusals.
use crate::common;
use crate::group_roots;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::doc::{DrDocument, LayerKind, LayerRaster};
use doctored_core::ffi::{dr_image_free, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_jobs::dr_job_control_set_scratch_limit;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_conversion::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use group_roots::GroupRoots;
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::Value;
use std::ffi::{c_char, CStr};
use std::ptr;
use std::sync::Arc;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

unsafe fn text(pointer: *mut c_char) -> String {
    assert!(!pointer.is_null());
    let result = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(pointer) };
    result
}
fn typed(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
fn legacy_fixture(grouped: bool) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(37, 19, |x, y| {
        Rgba([(x * 7) as u8, (y * 13) as u8, (x + y) as u8, 255])
    }));
    let mut foreground = doc.layers[0].clone();
    foreground.pixels = LayerRaster::new(RgbaImage::from_fn(37, 19, |x, y| {
        Rgba([233, 47, 169, ((x * 9 + y * 11) % 256) as u8])
    }));
    foreground.opacity = 0.713;
    foreground.mask = Some(Arc::new(GrayImage::from_fn(37, 19, |x, y| {
        image::Luma([((x + y) * 7) as u8])
    })));
    doc.layers.push(foreground);
    if grouped {
        for layer in &mut doc.layers {
            layer.depth = 1;
        }
        let mut group = doc.layers[0].clone();
        group.kind = LayerKind::Group;
        group.depth = 0;
        group.pixels = LayerRaster::new(RgbaImage::new(1, 1));
        group.opacity = 0.67;
        doc.layers.push(group);
        let mut adjustment = doc.layers[2].clone();
        adjustment.kind = LayerKind::Raster;
        adjustment.opacity = 0.37;
        adjustment.meta = Some(r#"{"type":"adjust","op":"levels","params":{"gamma":0.72}}"#.into());
        doc.layers.push(adjustment);
    }
    doc.profile = IccProfile::display_p3();
    doc.resolution = Resolution {
        x: 123.5,
        y: 287.25,
    };
    doc.metadata.xmp = Some(Arc::from(b"opaque captured XMP".as_slice()));
    doc.add_channel(
        "Saved selection",
        &vec![143; 37 * 19],
        37,
        19,
        [255, 0, 0],
        0.5,
    )
    .unwrap()
}

#[test]
fn promotion_renders_legacy_evaluation_before_exact_codes_and_drops_editable_tree() {
    for grouped in [false, true] {
        let doc = legacy_fixture(grouped);
        let expected = doc.flattened().unwrap();
        let original = doc.layers[0].pixels.clone();
        let mut err = ptr::null_mut();
        unsafe {
            let plan: Value =
                serde_json::from_str(&text(dr_doc_promote_flattened_plan(&doc, &mut err))).unwrap();
            assert_eq!(plan["flattened"], true);
            assert_eq!(plan["output_bytes"], 37 * 19 * (8 + 2));
            let out = dr_doc_promote_flattened(&doc, LIMIT, 0, ptr::null(), &mut err);
            assert!(
                !out.is_null(),
                "{}",
                if err.is_null() {
                    String::new()
                } else {
                    text(err)
                }
            );
            assert_eq!((*out).layer_count(), 1);
            assert_eq!((*out).channels().len(), 1);
            assert_eq!((*out).channels()[0].name(), "Saved selection");
            assert_eq!((*out).profile().bytes(), doc.profile.bytes());
            assert_eq!((*out).metadata(), &doc.metadata);
            assert_eq!((*out).resolution().x, 123.5);
            assert_eq!((*out).resolution().y, 287.25);
            for (actual, pixel) in (*out)
                .layer_source(0)
                .unwrap()
                .to_rgba16()
                .ok()
                .unwrap()
                .iter()
                .zip(expected.pixels())
            {
                assert_eq!(*actual, pixel.0.map(|v| u16::from(v) * 257));
            }
            let native = (*out).encode_native(LIMIT, 0, None).unwrap();
            let reopened = PrecisionDocument::parse_native(&native, LIMIT, 0, None).unwrap();
            assert_eq!(
                reopened.layer_source(0).unwrap().to_rgba16().ok(),
                (*out).layer_source(0).unwrap().to_rgba16().ok()
            );
            assert_eq!(reopened.metadata(), &doc.metadata);
            dr_precision_doc_free(out);
        }
        assert!(LayerRaster::ptr_eq(&original, &doc.layers[0].pixels));
        assert_eq!(doc.flattened().unwrap(), expected);
    }
}

#[test]
fn all_u16_codes_quantize_against_integer_oracle_and_alpha_is_never_dithered() {
    let pixels: Vec<_> = (0..=u16::MAX).map(|v| [v, 65535 - v, v, v]).collect();
    let image =
        PrecisionImage::from_rgba16(256, 256, pixels.clone(), IccProfile::srgb(), None).unwrap();
    for dither in [0, 1] {
        let mut err = ptr::null_mut();
        let output =
            unsafe { dr_precision_image_quantize(&image, dither, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!output.is_null());
        assert!(err.is_null());
        let raw =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(output), pixels.len() * 4) };
        if dither == 0 {
            assert_eq!(
                image.to_rgba8(LIMIT, 0, None).unwrap().as_raw().as_slice(),
                raw
            );
        }
        for (i, (actual, p)) in raw.chunks_exact(4).zip(&pixels).enumerate() {
            for c in 0..4 {
                let rank = bayer_rank(i % 256, i / 256);
                let expected = if dither == 0 || c == 3 {
                    (f64::from(p[c]) / 257.0).round() as u8
                } else {
                    (f64::from(p[c]) / 257.0 + (rank as f64 + 0.5) / 64.0)
                        .floor()
                        .min(255.0) as u8
                };
                assert_eq!(actual[c], expected, "dither={dither} pixel={i} channel={c}");
            }
        }
        unsafe { dr_image_free(output) };
    }
    assert_eq!(image.to_rgba16().ok().unwrap(), pixels);
}

/// Recursive 2x2 construction, independent from production's fixed table.
fn bayer_rank(x: usize, y: usize) -> usize {
    let mut rank = 0;
    for bit in 0..3 {
        let a = (x >> bit) & 1;
        let b = (y >> bit) & 1;
        rank = rank * 4 + [[0, 2], [3, 1]][b][a];
    }
    rank
}

#[test]
fn promoted_byte_codes_and_constant_ramp_mean_survive_ordered_grid() {
    for v in 0..=255_u16 {
        let image =
            PrecisionImage::from_rgba16(8, 8, vec![[v * 257; 4]; 64], IccProfile::srgb(), None)
                .unwrap();
        let out = image
            .quantize(PrecisionDither::Ordered, LIMIT, 0, None)
            .unwrap();
        assert!(out.as_raw().iter().all(|q| u16::from(*q) == v));
    }
    let value = 12345_u16;
    let image = PrecisionImage::from_rgba16(
        8,
        8,
        vec![[value, value, value, 1]; 64],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let out = image
        .quantize(PrecisionDither::Ordered, LIMIT, 0, None)
        .unwrap();
    let mean = out.pixels().map(|p| f64::from(p[0])).sum::<f64>() / 64.0;
    assert!((mean - f64::from(value) / 257.0).abs() <= 1.0 / 128.0);
    assert!(out.pixels().all(|p| p[3] == 0 && p[0] > 0));
}

#[test]
fn downconversion_uses_authoritative_projection_and_retains_native_sidecar() {
    let source = PrecisionImage::from_rgba16(
        16,
        8,
        (0..128)
            .map(|i| [10001 + i, 33003 - i, 60007, 65535])
            .collect(),
        IccProfile::display_p3(),
        None,
    )
    .unwrap();
    let metadata = Metadata {
        xmp: Some(Arc::from(b"captured typed XMP".as_slice())),
        ..Metadata::default()
    };
    let mut doc = PrecisionDocument::from_image(
        source,
        metadata,
        Resolution {
            x: 301.5,
            y: 157.25,
        },
    )
    .unwrap();
    doc = doc.group_all_pass_through(0.73).unwrap().unwrap();
    let projection = doc.render(LIMIT, 0, None).unwrap();
    for dither in [0, 1] {
        let mut err = ptr::null_mut();
        unsafe {
            let plan: Value =
                serde_json::from_str(&text(dr_precision_doc_downconvert_plan(&doc, &mut err)))
                    .unwrap();
            assert!(plan["scratch_bytes"].as_u64().unwrap() >= 16 * 8 * 12);
            let out = dr_precision_doc_downconvert(&doc, dither, LIMIT, 0, ptr::null(), &mut err);
            assert!(!out.is_null());
            assert!(err.is_null());
            assert_eq!((*out).layers.len(), 1);
            let expected = projection
                .quantize(PrecisionDither::from_code(dither).unwrap(), LIMIT, 0, None)
                .unwrap();
            assert_eq!(&*(&(*out).layers)[0].pixels.contiguous_image(), &expected);
            assert_eq!((*out).profile.bytes(), doc.profile().bytes());
            assert_eq!(&(*out).metadata, doc.metadata());
            assert_eq!((*out).resolution, doc.resolution());
            dr_doc_free(out);
        }
    }
    assert_eq!(doc.layer_count(), 2);
}

#[test]
fn conversion_refuses_capacity_cancellation_and_invalid_dither_without_mutation() {
    let legacy = legacy_fixture(false);
    let typed = typed(32, 32, vec![[12345, 22222, 34567, 65535]; 1024]);
    for limit in [1, 4 * 1024 * 1024] {
        assert!(legacy.promote_flattened(limit, LIMIT, None).is_err());
        assert!(typed
            .downconvert(PrecisionDither::None, limit, LIMIT, None)
            .is_err());
    }
    let token = DrJobControl::default();
    token.cancel();
    assert!(legacy.promote_flattened(LIMIT, 0, Some(&token)).is_err());
    assert_eq!(token.scratch_bytes(), 0);
    let token = DrJobControl::default();
    unsafe {
        assert!(dr_job_control_set_scratch_limit(&token, 1));
    }
    assert!(typed
        .downconvert(PrecisionDither::None, LIMIT, 0, Some(&token))
        .is_err());
    assert_eq!(token.scratch_bytes(), 0);
    let mut err = ptr::null_mut();
    unsafe {
        assert!(
            dr_precision_doc_downconvert(&typed, 99, LIMIT, 0, ptr::null(), &mut err).is_null()
        );
        assert!(text(err).contains("dither"));
    }
    assert_eq!(
        typed.layer_source(0).unwrap().to_rgba16().ok().unwrap()[0],
        [12345, 22222, 34567, 65535]
    );
}

#[test]
fn conversion_exports_are_null_safe() {
    unsafe {
        assert!(dr_doc_promote_flattened_plan(ptr::null(), ptr::null_mut()).is_null());
        assert!(
            dr_doc_promote_flattened(ptr::null(), LIMIT, 0, ptr::null(), ptr::null_mut()).is_null()
        );
        assert!(dr_precision_doc_downconvert_plan(ptr::null(), ptr::null_mut()).is_null());
        assert!(dr_precision_doc_downconvert(
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_image_quantize(
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
}

#[test]
fn native_output_and_token_capacity_refuse_before_a_large_destination_allocation() {
    let mut doc = DrDocument::from_pixels(RgbaImage::new(1, 1));
    // A legal 100 MP canvas. Its one-raster native output reopens
    // (a 1 GiB stored-plane record limit, no decoder cap on a 16-bit plane),
    // so the plan is admitted with 800 MB of output, and a working limit that
    // cannot hold it refuses before any 100 MP pixels are created.
    doc.width = 10_000;
    doc.height = 10_000;
    let plan = doc.promote_flattened_plan().unwrap();
    assert_eq!(plan.output_bytes, 800_000_000);
    let token = DrJobControl::default();
    assert!(doc
        .promote_flattened(1024 * 1024 * 1024, 0, Some(&token))
        .is_err());
    assert_eq!(token.scratch_bytes(), 0);
    assert_eq!(doc.layers[0].pixels.dimensions(), (1, 1));
    // Past the pixel limit the plan itself refuses, at any working limit.
    doc.width = 10_001;
    assert!(doc.promote_flattened_plan().is_err());
    assert!(doc.promote_flattened(LIMIT, 0, Some(&token)).is_err());
    assert_eq!(token.scratch_bytes(), 0);
    assert_eq!(doc.layers[0].pixels.dimensions(), (1, 1));
}

#[test]
fn tiny_canvas_large_metadata_is_admitted_by_description_size_not_pixel_count() {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(1, 1, Rgba([12, 34, 56, 255])));
    // Valid JSON with an irrelevant array still allocates its complete Value
    // before legacy operation recognition. One pixel does not bound that work.
    let description = format!(
        r#"{{"type":"adjust","op":"invert","params":{{"unused":[0{}]}}}}"#,
        ",0".repeat(512 * 1024)
    );
    let minimum = 64 * description.len() as u64;
    doc.layers[0].meta = Some(description);
    let plan = doc.promote_flattened_plan().unwrap();
    assert!(plan.scratch_bytes >= minimum + 4 * 1024 * 1024);
    let control = DrJobControl::default();
    let failure = doc
        .promote_flattened(32 * 1024 * 1024, 0, Some(&control))
        .err()
        .unwrap();
    assert!(failure.contains("working limit"));
    assert_eq!(control.scratch_bytes(), 0);
    assert_eq!(
        doc.layers[0].pixels.contiguous_image().as_raw(),
        &[12, 34, 56, 255]
    );
}

/// `calibrated_adjustment_tests`' unmodelled RGB profile: sRGB's header
/// with a zero tag count, header size patched to 132.
fn unmodelled() -> Arc<IccProfile> {
    let mut bytes = IccProfile::srgb().bytes()[..128].to_vec();
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes[..4].copy_from_slice(&132u32.to_be_bytes());
    let profile = IccProfile::parse(&bytes).unwrap();
    assert!(profile.model().is_none(), "the profile has no model");
    Arc::new(profile)
}

#[test]
fn tiny_canvas_large_metadata_under_an_unmodelled_profile_renders_within_its_own_plan() {
    // The `:343` fixture under a profile with no model: the render's
    // description-context parse (`context_scratch`, 256 bytes per byte of the
    // largest description) must be inside the published plan.
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(1, 1, Rgba([12, 34, 56, 255])));
    let description = format!(
        r#"{{"type":"adjust","op":"invert","params":{{"unused":[0{}]}}}}"#,
        ",0".repeat(512 * 1024)
    );
    let length = description.len() as u64;
    doc.layers[0].meta = Some(description);
    doc.profile = unmodelled();
    let plan = doc.promote_flattened_plan().unwrap();
    assert!(plan.scratch_bytes >= 64 * length + 256 * length + 4 * 1024 * 1024);
    assert!(!plan.cooperative_render);
    let control = DrJobControl::default();
    // Every `reserve` is checked against this limit, so success is the
    // witness that no render-time reservation exceeds the plan.
    assert!(unsafe { dr_job_control_set_scratch_limit(&control, plan.scratch_bytes) });
    let promoted = doc.promote_flattened(LIMIT, 0, Some(&control));
    assert!(promoted.is_ok(), "{:?}", promoted.err());
    assert_eq!(control.scratch_bytes(), 0);
}

#[test]
fn cancellation_during_actual_downconversion_discards_output_and_releases_token() {
    let doc = typed(2048, 2048, vec![[12345, 23456, 34567, 65535]; 2048 * 2048]);
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match doc.downconvert(PrecisionDither::Ordered, LIMIT, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    // k = 0: the quantization polls only at its reports.
    let (entry, _) = entry_then_cancel(&|progress, _, _| progress >= 0.85, render);
    let parked = cancelled_parked_at(
        entry.checkpoints + 1,
        &|_, stage| stage == entry.stage,
        render,
    );
    eprintln!(
        "live document downconversion cancellation: {}",
        parked_line(&parked)
    );
    assert_eq!(doc.layer_count(), 1);
    assert_eq!(
        doc.layer_source(0).unwrap().to_rgba16().ok().unwrap()[0],
        [12345, 23456, 34567, 65535]
    );
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancellation_during_actual_downconversion_discards_output_and_releases_token() {
        let doc = Arc::new(typed(
            2048,
            2048,
            vec![[12345, 23456, 34567, 65535]; 2048 * 2048],
        ));
        let token = Arc::new(DrJobControl::default());
        let source = doc.clone();
        let control = token.clone();
        let worker = std::thread::spawn(move || {
            source.downconvert(PrecisionDither::Ordered, LIMIT, 0, Some(&control))
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        while token.progress() < 0.85 && !worker.is_finished() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            token.progress() >= 0.85 && !worker.is_finished(),
            "must cancel during final quantization"
        );
        let began = Instant::now();
        token.cancel();
        assert!(worker.join().unwrap().err().unwrap().contains("cancelled"));
        assert!(began.elapsed() < Duration::from_secs(2));
        eprintln!(
            "live document downconversion cancellation: {:?}",
            began.elapsed()
        );
        assert_eq!(token.scratch_bytes(), 0);
        assert_eq!(doc.layer_count(), 1);
        assert_eq!(
            doc.layer_source(0).unwrap().to_rgba16().ok().unwrap()[0],
            [12345, 23456, 34567, 65535]
        );
    }
}
