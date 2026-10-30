//! Independent f64 references and resource/refusal checks for the isolated
//! Phase 7A experiment. No test promotes it into the legacy editor mode.
use crate::common;
use common::probe::{entry_then_cancel, parked_line, Parked, Released};
use std::sync::Arc;
use std::time::{Duration, Instant};

use doctored_core::ffi_jobs::dr_job_control_set_scratch_limit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_render::{
    require_supported, PrecisionLayer, PrecisionScene, PrototypeOperation, MAX_GROUP_DEPTH,
    MAX_RECORDS, MIN_RENDER_ALPHA,
};

const U16: PrecisionStorage = PrecisionStorage::Rgba16Unorm;
const F32: PrecisionStorage = PrecisionStorage::Rgba32Float;

fn image(pixels: Vec<[f32; 4]>, width: u32) -> PrecisionImage {
    PrecisionImage::from_rgba32(
        width,
        pixels.len() as u32 / width,
        pixels,
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}
fn layer(image: PrecisionImage, opacity: f32) -> PrecisionLayer {
    PrecisionLayer::raster(image, (0, 0), opacity).unwrap()
}
fn scene(width: u32, height: u32, layers: Vec<PrecisionLayer>) -> PrecisionScene {
    PrecisionScene::new(width, height, IccProfile::srgb(), layers, None, false).unwrap()
}

// Premultiplied f64 source-over, independently expressed rather than repeating
// the production W3C straight-color three-term expansion.
fn over(background: [f64; 4], source: [f64; 4], opacity: f32) -> [f64; 4] {
    let alpha = source[3] * f64::from(opacity);
    let old_alpha = background[3] * (1.0 - alpha);
    let combined = alpha + old_alpha;
    if combined == 0.0 {
        return [0.0; 4];
    }
    [
        (alpha * source[0] + old_alpha * background[0]) / combined,
        (alpha * source[1] + old_alpha * background[1]) / combined,
        (alpha * source[2] + old_alpha * background[2]) / combined,
        combined,
    ]
}
fn decoded(image: &PrecisionImage, index: usize) -> [f64; 4] {
    let width = image.dimensions().0 as usize;
    let (x, y) = ((index % width) as u32, (index / width) as u32);
    if let Some(pixel) = image.rgba16_at(x, y) {
        pixel.map(|v| f64::from(v) / 65535.0)
    } else {
        image.sample_at(x, y).unwrap().map(f64::from)
    }
}

#[test]
fn all_u16_codes_round_trip_and_owned_allocations_are_adopted() {
    let pixels: Vec<_> = (0..=u16::MAX)
        .map(|v| [v, 65535 - v, v / 2, if v % 3 == 0 { 0 } else { v }])
        .collect();
    let source =
        PrecisionImage::from_rgba16(65536, 1, pixels, IccProfile::display_p3(), None).unwrap();
    assert_eq!(source.storage_bytes(), 65536 * 8);
    let converted = source
        .convert(F32, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    let restored = converted
        .convert(U16, REFERENCE_WORKING_BYTES, source.retained_bytes(), None)
        .unwrap();
    assert_eq!(restored.to_rgba16().ok(), source.to_rgba16().ok());
    assert!(Arc::ptr_eq(restored.profile(), source.profile()));
    let same = source
        .convert(U16, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    assert!(PrecisionImage::ptr_eq(&same, &source));
    let preview = source.to_rgba8(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    for (actual, original) in preview
        .pixels()
        .zip(source.to_rgba16().ok().unwrap().iter())
    {
        assert_eq!(
            actual.0,
            original.map(|v| ((u32::from(v) + 128) / 257) as u8)
        );
    }
    // Float constructor adopts its owned allocation and preserves signed zero
    // and even subnormal alpha; rendering has a separate arithmetic contract.
    let data = vec![[0.75, -0.0, 0.25, f32::from_bits(1)]];
    let float = image(data, 1);
    assert_eq!(
        float.to_rgba32().ok().unwrap()[0][1].to_bits(),
        (-0.0_f32).to_bits()
    );
    assert_eq!(float.to_rgba32().ok().unwrap()[0][3].to_bits(), 1);
}

#[test]
fn actual_sub_byte_distinctions_survive_two_isolation_boundaries() {
    for storage in [U16, F32] {
        let source = if storage == U16 {
            PrecisionImage::from_rgba16(
                2,
                1,
                vec![[30000, 12345, 40000, 65535], [30001, 12346, 40001, 65535]],
                IccProfile::srgb(),
                None,
            )
            .unwrap()
        } else {
            image(
                vec![
                    [0.500001, 0.123456, 0.765432, 1.0],
                    [0.500002, 0.123457, 0.765433, 1.0],
                ],
                2,
            )
        };
        let original = source.clone();
        let inner = PrecisionLayer::isolated_normal(vec![layer(source, 1.0)], 1.0).unwrap();
        let outer = PrecisionLayer::isolated_normal(vec![inner], 1.0).unwrap();
        let output = scene(2, 1, vec![outer])
            .render(storage, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap();
        assert_eq!(output.to_rgba16().ok(), original.to_rgba16().ok());
        assert_eq!(output.to_rgba32().ok(), original.to_rgba32().ok());
        assert_ne!(decoded(&output, 0), decoded(&output, 1));
        let preview = output.to_rgba8(REFERENCE_WORKING_BYTES, 0, None);
        if storage == U16 {
            let preview = preview.unwrap();
            assert_eq!(preview.get_pixel(0, 0), preview.get_pixel(1, 0));
        } else {
            // Phase 11F: F32 storage is extended range, so direct RGBA8
            // conversion refuses and display goes through the document's
            // tone map instead.
            assert!(preview.unwrap_err().contains("tone map"));
        }
    }
}

#[test]
fn nested_mixed_storage_composites_match_independent_f64_reference() {
    let lower = image(
        (0..12)
            .map(|i| {
                [
                    0.40001 + i as f32 * 0.00001,
                    0.18,
                    0.61,
                    [0.0, 0.2, 0.7, 1.0][i % 4],
                ]
            })
            .collect(),
        4,
    );
    let upper = PrecisionImage::from_rgba16(
        4,
        3,
        (0..12)
            .map(|i| {
                [
                    31001 + i * 3,
                    20001 + i,
                    49002 - i,
                    [0, 1, 17001, 65535][i as usize % 4],
                ]
            })
            .collect(),
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let foreground = image(
        (0..12)
            .map(|i| [0.91, 0.200003, 0.13, [0.0, 1e-8, 0.5, 0.95][i % 4]])
            .collect(),
        4,
    );
    let group = PrecisionLayer::isolated_normal(
        vec![
            layer(upper.clone(), 0.375),
            layer(foreground.clone(), 0.8125),
        ],
        0.625,
    )
    .unwrap();
    let group = PrecisionLayer::isolated_normal(vec![group], 0.75).unwrap();
    let composite = scene(4, 3, vec![layer(lower.clone(), 0.875), group]);
    for storage in [U16, F32] {
        let output = composite
            .render(storage, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap();
        for i in 0..12 {
            let back = over([0.0; 4], decoded(&lower, i), 0.875);
            let group = over(
                over([0.0; 4], decoded(&upper, i), 0.375),
                decoded(&foreground, i),
                0.8125,
            );
            let group = over([0.0; 4], group, 0.625);
            let expected = over(back, group, 0.75);
            for (actual, expected) in decoded(&output, i).into_iter().zip(expected) {
                let tolerance = 2e-6 + if storage == U16 { 0.5 / 65535.0 } else { 0.0 };
                assert!(
                    (actual - expected).abs() <= tolerance,
                    "pixel {i}: {actual} != {expected}"
                );
            }
        }
    }
    // Rendering never writes the captured source, including latent RGB.
    assert_eq!(
        lower.to_rgba32().ok().unwrap()[0],
        [0.40001, 0.18, 0.61, 0.0]
    );
}

#[test]
fn offsets_clip_at_canvas_and_hidden_rgb_never_enters_visible_color() {
    let source = image(vec![[1.0, 0.0, 0.0, 0.0], [0.2, 0.6, 0.8, 1.0]], 2);
    let output = scene(
        2,
        1,
        vec![PrecisionLayer::raster(source.clone(), (-1, 0), 1.0).unwrap()],
    )
    .render(F32, REFERENCE_WORKING_BYTES, 0, None)
    .unwrap();
    assert_eq!(
        output.to_rgba32().ok().unwrap(),
        &[[0.2, 0.6, 0.8, 1.0], [0.0; 4]]
    );
    for offset in [
        (i32::MIN, i32::MIN),
        (i32::MAX, i32::MAX),
        (i32::MAX, 0),
        (0, i32::MAX),
    ] {
        let out = scene(
            2,
            1,
            vec![PrecisionLayer::raster(source.clone(), offset, 1.0).unwrap()],
        )
        .render(F32, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
        assert!(out.to_rgba32().ok().unwrap().iter().all(|p| *p == [0.0; 4]));
    }
    assert_eq!(source.to_rgba32().ok().unwrap()[0], [1.0, 0.0, 0.0, 0.0]);
}

#[test]
fn invalid_float_geometry_profiles_and_unsupported_operations_refuse() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.001, 1.001] {
        for channel in 0..4 {
            let mut pixel = [0.5; 4];
            pixel[channel] = bad;
            assert!(
                PrecisionImage::from_rgba32(1, 1, vec![pixel], IccProfile::srgb(), None).is_err()
            );
        }
    }
    for (w, h) in [(0, 1), (1, 0), (u32::MAX, u32::MAX), (10001, 10000)] {
        assert!(PrecisionImage::from_rgba16(w, h, Vec::new(), IccProfile::srgb(), None).is_err());
    }
    assert!(PrecisionImage::from_rgba16(2, 1, vec![[0; 4]], IccProfile::srgb(), None).is_err());
    let source = image(vec![[0.5; 4]], 1);
    for opacity in [f32::NAN, -0.1, 1.1] {
        assert!(PrecisionLayer::raster(source.clone(), (0, 0), opacity).is_err());
    }
    for profile in [
        IccProfile::display_p3(),
        Arc::new(IccProfile::parse(IccProfile::srgb().bytes()).unwrap()),
    ] {
        assert!(
            PrecisionScene::new(1, 1, profile, vec![layer(source.clone(), 1.0)], None, false)
                .is_err()
        );
    }
    for operation in [
        PrototypeOperation::OtherBlendMode,
        PrototypeOperation::Geometry,
        PrototypeOperation::Paint,
        PrototypeOperation::Retouch,
        PrototypeOperation::NativePersistence,
        PrototypeOperation::ProfileConversion,
        PrototypeOperation::Hdr,
    ] {
        assert!(require_supported(operation)
            .unwrap_err()
            .contains("no implicit RGBA8"));
    }
}

#[test]
fn near_zero_arithmetic_has_an_explicit_floor_without_mutating_storage() {
    for alpha in [MIN_RENDER_ALPHA, MIN_RENDER_ALPHA * 2.0, 1e-8] {
        let source = image(vec![[0.25, 0.5, 0.75, alpha]], 1);
        let output = scene(1, 1, vec![layer(source, 1.0)])
            .render(F32, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap();
        let pixel = output.to_rgba32().ok().unwrap()[0];
        assert_eq!(pixel[3], alpha);
        for (actual, expected) in pixel[..3].iter().zip([0.25, 0.5, 0.75]) {
            assert!((*actual - expected).abs() < 2e-6);
        }
    }
    for (alpha, opacity) in [
        (f32::from_bits(1), 1.0),
        (MIN_RENDER_ALPHA * 0.5, 1.0),
        (MIN_RENDER_ALPHA, 0.5),
        (f32::MIN_POSITIVE, f32::MIN_POSITIVE),
    ] {
        let source = image(vec![[0.25, 0.5, 0.75, alpha]], 1);
        let control = DrJobControl::default();
        let result = scene(1, 1, vec![layer(source.clone(), opacity)]).render(
            F32,
            REFERENCE_WORKING_BYTES,
            0,
            Some(&control),
        );
        assert!(result.err().unwrap().contains("2^-96"));
        assert_eq!(control.scratch_bytes(), 0);
        assert_eq!(
            source.to_rgba32().ok().unwrap()[0][3].to_bits(),
            alpha.to_bits()
        );
    }
}

#[test]
fn memory_plan_counts_capacity_sharing_and_final_output_before_allocation() {
    let mut pixels = Vec::with_capacity(1024);
    pixels.push([12345, 30001, 40001, 65535]);
    let source = PrecisionImage::from_rgba16(1, 1, pixels, IccProfile::srgb(), None).unwrap();
    assert_eq!(source.storage_bytes(), 8);
    let one = scene(1, 1, vec![layer(source.clone(), 1.0)])
        .plan(U16)
        .unwrap();
    let many = scene(1, 1, (0..20).map(|_| layer(source.clone(), 1.0)).collect());
    let plan = many.plan(U16).unwrap();
    assert_eq!(
        plan.unique_retained_bytes - one.unique_retained_bytes,
        19 * std::mem::size_of::<PrecisionLayer>() as u64
    );
    assert_eq!(plan.output_bytes, 8);
    let group = PrecisionLayer::isolated_normal(vec![layer(source.clone(), 1.0)], 0.75).unwrap();
    let grouped_once = scene(1, 1, vec![group.clone()]).plan(U16).unwrap();
    let grouped_twice = scene(1, 1, vec![group.clone(), group]).plan(U16).unwrap();
    assert_eq!(
        grouped_twice.unique_retained_bytes - grouped_once.unique_retained_bytes,
        std::mem::size_of::<PrecisionLayer>() as u64
    );
    let total = plan.total_working_bytes(0).unwrap();
    assert!(many.render(U16, total - 1, 0, None).is_err());
    let completed = DrJobControl::default();
    assert!(many.render(U16, total, 0, Some(&completed)).is_ok());
    assert_eq!(completed.progress(), 1.0);
    assert_eq!(completed.scratch_bytes(), 0);
    assert!(many
        .render(U16, total, 0, Some(&completed))
        .err()
        .unwrap()
        .contains("already been used"));
    assert!(many.render(U16, total, 1, None).is_err());
    for limit in [0, 1] {
        assert!(many.render(U16, limit, 0, None).is_err());
    }
    assert!(many
        .render(U16, REFERENCE_WORKING_BYTES + 1, 0, None)
        .is_ok());
    assert!(many.render(U16, u64::MAX, 0, None).is_ok());
    assert!(source
        .to_rgba8(source.retained_bytes() + 4, 0, None)
        .is_err());
    assert!(source
        .convert(F32, REFERENCE_WORKING_BYTES, u64::MAX, None)
        .is_err());
    let control = DrJobControl::default();
    assert!(unsafe { dr_job_control_set_scratch_limit(&control, 1) });
    assert!(many
        .render(U16, REFERENCE_WORKING_BYTES, 0, Some(&control))
        .is_err());
    assert_eq!(control.scratch_bytes(), 0);
}

#[test]
fn record_and_depth_limits_refuse_and_work_is_planned_without_large_pixel_allocations() {
    let source = image(vec![[0.5; 4]], 1);
    assert!(PrecisionScene::new(
        1,
        1,
        IccProfile::srgb(),
        (0..=MAX_RECORDS)
            .map(|_| layer(source.clone(), 1.0))
            .collect(),
        None,
        false
    )
    .is_err());
    let mut nested = layer(source, 1.0);
    for _ in 0..MAX_GROUP_DEPTH {
        nested = PrecisionLayer::isolated_normal(vec![nested], 1.0).unwrap();
    }
    assert!(PrecisionLayer::isolated_normal(vec![nested.clone()], 1.0).is_err());
    // Sixteen nested groups over 100 MP: 2 visits a pixel each, 2 more for
    // initialization and finalization, and the 1×1 raster's own pixel. Past
    // the 2·10⁹ the plan once refused at; a render runs under a job
    // control, so the plan answers and reports the work without allocating.
    let large = scene(10000, 10000, vec![nested]);
    assert_eq!(large.plan(F32).unwrap().work_units, 3_400_000_001);
    // Nine empty nested groups plus initialization/finalization account for
    // exactly 20 visits over 100 MP. The query allocates none of those pixel
    // buffers; actual rendering still refuses the independent memory bound.
    let mut empty = PrecisionLayer::isolated_normal(Vec::new(), 1.0).unwrap();
    for _ in 1..9 {
        empty = PrecisionLayer::isolated_normal(vec![empty], 1.0).unwrap();
    }
    let exact_work = scene(10000, 10000, vec![empty]);
    assert_eq!(exact_work.plan(F32).unwrap().work_units, 2_000_000_000);
    assert!(exact_work
        .render(F32, REFERENCE_WORKING_BYTES, 0, None)
        .is_err());
}

#[test]
fn cancellation_during_compositing_returns_no_result_and_releases_scratch() {
    let source = image(vec![[0.5, 0.20001, 0.75, 0.125]; 1024 * 1024], 1024);
    let work = scene(
        1024,
        1024,
        (0..64).map(|_| layer(source.clone(), 1.0)).collect(),
    );
    // k = 0: the compositing polls only at its reports.
    let (entry, _) = entry_then_cancel(&|progress, _, _| progress >= 0.1, |token| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match work.render(F32, REFERENCE_WORKING_BYTES, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    });
    let park = entry.checkpoints + 1;
    // The seam directly, not `cancelled_parked_at`: the cancelled control is
    // reused below, which must refuse as already used.
    let control = DrJobControl::default();
    control.park_at(park);
    let parked = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let out = work.render(F32, REFERENCE_WORKING_BYTES, 0, Some(&control));
            let returned = Instant::now();
            control.release();
            (out.err(), returned)
        });
        let _guard = Released(&control);
        assert!(
            control.wait_parked(Duration::from_secs(240)),
            "the worker did not park at {park}"
        );
        let (count, progress, stage, scratch) = (
            control.checkpoints(),
            control.progress(),
            control.stage(),
            control.scratch_bytes(),
        );
        let at = Instant::now();
        control.cancel();
        control.release();
        let (error, returned) = worker.join().unwrap();
        assert!(error
            .expect("must cancel during active compositing")
            .contains("cancelled"));
        assert_eq!(count, park, "the worker parked at the wrong checkpoint");
        assert_eq!(
            stage, entry.stage,
            "parked at checkpoint {count}: stage {stage}"
        );
        assert!(
            progress >= 0.1,
            "must exercise pixel compositing, not just allocation"
        );
        Parked {
            count,
            progress,
            stage,
            scratch,
            latency: returned.saturating_duration_since(at),
        }
    });
    eprintln!("compositing cancellation: {}", parked_line(&parked));
    assert_eq!(control.scratch_bytes(), 0);
    assert!(control.progress() < 1.0);
    let pre_cancelled = DrJobControl::default();
    pre_cancelled.cancel();
    assert!(source
        .to_rgba8(REFERENCE_WORKING_BYTES, 0, Some(&pre_cancelled))
        .is_err());
    assert!(work
        .render(F32, REFERENCE_WORKING_BYTES, 0, Some(&control))
        .err()
        .unwrap()
        .contains("already been used"));
    assert_eq!(
        source.to_rgba32().ok().unwrap()[0],
        [0.5, 0.20001, 0.75, 0.125]
    );
}

mod bench {
    use super::*;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancellation_during_compositing_returns_no_result_and_releases_scratch() {
        let source = image(vec![[0.5, 0.20001, 0.75, 0.125]; 1024 * 1024], 1024);
        let work = scene(
            1024,
            1024,
            (0..64).map(|_| layer(source.clone(), 1.0)).collect(),
        );
        let control = DrJobControl::default();
        std::thread::scope(|scope| {
            let worker =
                scope.spawn(|| work.render(F32, REFERENCE_WORKING_BYTES, 0, Some(&control)));
            let started = Instant::now();
            while control.progress() < 0.1
                && !worker.is_finished()
                && started.elapsed() < Duration::from_secs(5)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(
                !worker.is_finished(),
                "must cancel during active compositing"
            );
            assert!(
                control.progress() >= 0.1,
                "must exercise pixel compositing, not just allocation"
            );
            let cancelled = Instant::now();
            control.cancel();
            assert!(worker.join().unwrap().err().unwrap().contains("cancelled"));
            assert!(cancelled.elapsed() < Duration::from_millis(500));
        });
        assert_eq!(control.scratch_bytes(), 0);
        assert!(control.progress() < 1.0);
        let pre_cancelled = DrJobControl::default();
        pre_cancelled.cancel();
        assert!(source
            .to_rgba8(REFERENCE_WORKING_BYTES, 0, Some(&pre_cancelled))
            .is_err());
        assert!(work
            .render(F32, REFERENCE_WORKING_BYTES, 0, Some(&control))
            .err()
            .unwrap()
            .contains("already been used"));
        assert_eq!(
            source.to_rgba32().ok().unwrap()[0],
            [0.5, 0.20001, 0.75, 0.125]
        );
    }
}
