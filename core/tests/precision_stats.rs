//! Independent exact-code readout/histogram, family guards and cancellation.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_develop_inspect::dr_precision_image_histogram_checked;
use doctored_core::ffi_precision_conversion::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

unsafe fn owned_json(value: *mut c_char) -> Value {
    assert!(!value.is_null());
    let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(value) }.to_bytes()).unwrap();
    unsafe { dr_string_free(value) };
    parsed
}
fn image(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::display_p3(), None).unwrap()
}

#[test]
fn sample_keeps_sub_byte_codes_fractional_means_latent_rgb_and_profile() {
    let pixels = vec![
        [1, 2, 3, 0],
        [2, 3, 4, 1],
        [65535, 257, 256, 65535],
        [259, 258, 257, 2],
    ];
    let source = image(2, 2, pixels.clone());
    let mut err = ptr::null_mut();
    let point = unsafe { owned_json(dr_precision_image_sample(&source, 0, 0, 0, &mut err)) };
    assert!(err.is_null());
    assert_eq!(point["center_rgba16"], json!([1, 2, 3, 0]));
    assert_eq!(point["normalized_rgba"][0].as_f64().unwrap(), 1.0 / 65535.0);
    assert!(point["profile_name"].as_str().unwrap().contains("P3"));
    let block = unsafe { owned_json(dr_precision_image_sample(&source, 0, 0, 1, &mut err)) };
    for c in 0..4 {
        let sum: u64 = pixels.iter().map(|p| u64::from(p[c])).sum();
        assert_eq!(block["sum_rgba16"][c], sum);
        assert_eq!(block["mean_rgba16"][c].as_f64().unwrap(), sum as f64 / 4.0);
        assert_eq!(
            block["normalized_rgba"][c].as_f64().unwrap(),
            sum as f64 / 4.0 / 65535.0
        );
    }
    let outside = source.sample(-1, -1, 1).unwrap();
    assert!(outside["center_rgba16"].is_null());
    assert_eq!(outside["sample_count"], 1);
    assert_eq!(outside["sum_rgba16"], json!([1, 2, 3, 0]));
    assert!(source.sample(i32::MAX, i32::MIN, 2).is_err());
    assert!(source.sample(0, 0, 3).is_err());
    assert_eq!(source.to_rgba16().ok().unwrap(), pixels);
}

#[test]
fn all_codes_histogram_distinguishes_exact_clipping_from_display_end_buckets() {
    let source = image(
        256,
        256,
        (0..=u16::MAX).map(|v| [v, 65535 - v, v, 65535]).collect(),
    );
    let mut err = ptr::null_mut();
    let token = DrJobControl::default();
    let result = unsafe {
        owned_json(dr_precision_image_histogram_checked(
            &source,
            0,
            REFERENCE_WORKING_BYTES,
            0,
            &token,
            &mut err,
        ))
    };
    assert!(err.is_null());
    assert_eq!(token.scratch_bytes(), 0);
    assert_eq!(token.progress(), 1.0);
    assert_eq!(result["total"], 65536);
    assert_eq!(result["exact_zero_rgba"], json!([1, 1, 1, 0]));
    assert_eq!(result["exact_max_rgba"], json!([1, 1, 1, 65536]));
    let mut reference = [0_u64; 256];
    for v in 0..=65535 {
        reference[(v as f64 / 257.0).round() as usize] += 1;
    }
    for c in 0..3 {
        assert_eq!(result["bins"][c], json!(reference.as_slice()));
        assert_eq!(result["bins"][c][0], 129);
        assert_eq!(result["bins"][c][255], 129);
    }
    assert_eq!(result["alpha_bins"][255], 65536);
}

#[test]
fn histogram_visibility_stride_and_luma_counting_are_explicit() {
    let pixels = vec![
        [0, 0, 0, 0],
        [0, 0, 0, 1],
        [1, 1, 1, 2],
        [65535, 65535, 65535, 65535],
        [65534, 65534, 65534, 3],
        [1234, 5678, 9012, 0],
    ];
    let source = image(6, 1, pixels.clone());
    let full = source
        .histogram_checked(1, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    assert_eq!(full["total"], 4);
    assert_eq!(full["sampled_pixels"], 6);
    assert_eq!(full["exact_zero_rgba"], json!([1, 1, 1, 2]));
    assert_eq!(full["exact_max_rgba"], json!([1, 1, 1, 1]));
    assert_eq!(full["exact_zero_luma"], 1);
    assert_eq!(full["exact_max_luma"], 1);
    assert_eq!(full["bins"][3][0], 2);
    assert_eq!(full["bins"][3][255], 2);
    let sparse = source
        .histogram_checked(2, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    assert_eq!(sparse["total"], 2);
    assert_eq!(sparse["sampled_pixels"], 3);
    assert_eq!(sparse["exact_zero_rgba"], json!([0, 0, 0, 1]));
    assert_eq!(sparse["exact_max_rgba"], json!([0, 0, 0, 0]));
    assert_eq!(sparse["clipping_scope"], "sampled positions only");
    assert_eq!(
        source
            .histogram_checked(u32::MAX, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()["sampled_pixels"],
        1
    );
    assert_eq!(source.to_rgba16().ok().unwrap(), pixels);
}

#[test]
fn capability_is_core_owned_and_unknown_depth_operation_target_fail_closed() {
    for (op, target, supported) in [
        ("paint", "raster", true),
        ("paint", "mask", false),
        ("adjustment_apply", "red", false),
        ("geometry_affine", "mask", true),
        ("geometry_affine", "group", false),
        ("read_plane", "alpha", true),
        ("channel_read", "channel", true),
        ("histogram", "document", true),
        ("layer_name", "group", true),
        ("layer_visible", "adjustment", true),
        ("content_aware_fill", "raster", false),
        ("unknown", "document", false),
        ("clone", "pixels", false),
    ] {
        let op = CString::new(op).unwrap();
        let target = CString::new(target).unwrap();
        let mut err = ptr::null_mut();
        let result = unsafe {
            owned_json(dr_precision_capability(
                op.as_ptr(),
                target.as_ptr(),
                16,
                &mut err,
            ))
        };
        assert!(err.is_null());
        assert_eq!(result["supported"], supported);
        if !supported {
            assert!(result["reason"].is_string());
        }
        let result = unsafe {
            owned_json(dr_precision_capability(
                op.as_ptr(),
                target.as_ptr(),
                8,
                &mut err,
            ))
        };
        assert_eq!(result["supported"], false);
    }
}

#[test]
fn live_histogram_cancellation_releases_reservation_and_preserves_source() {
    let source = image(2048, 2048, vec![[12345, 34567, 45678, 65535]; 2048 * 2048]);
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match source.histogram_checked(1, REFERENCE_WORKING_BYTES, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    // k = 0: the histogram polls only at its reports.
    let (entry, _) = entry_then_cancel(&|progress, _, _| progress >= 0.01, render);
    let parked = cancelled_parked_at(
        entry.checkpoints + 1,
        &|_, stage| stage == entry.stage,
        render,
    );
    eprintln!("live u16 histogram cancellation: {}", parked_line(&parked));
    assert_eq!(
        source.to_rgba16().ok().unwrap()[0],
        [12345, 34567, 45678, 65535]
    );
}

#[test]
fn statistics_and_capability_exports_are_null_safe() {
    unsafe {
        assert!(dr_precision_image_sample(ptr::null(), 0, 0, 0, ptr::null_mut()).is_null());
        assert!(dr_precision_image_histogram_checked(
            ptr::null(),
            1,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_capability(ptr::null(), ptr::null(), 16, ptr::null_mut()).is_null());
        let long = CString::new("x".repeat(65)).unwrap();
        let valid = CString::new("raster").unwrap();
        assert!(
            dr_precision_capability(long.as_ptr(), valid.as_ptr(), 16, ptr::null_mut()).is_null()
        );
        let bad = [255_u8, 0];
        assert!(
            dr_precision_capability(bad.as_ptr().cast(), valid.as_ptr(), 16, ptr::null_mut())
                .is_null()
        );
    }
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn live_histogram_cancellation_releases_reservation_and_preserves_source() {
        let source = Arc::new(image(
            2048,
            2048,
            vec![[12345, 34567, 45678, 65535]; 2048 * 2048],
        ));
        let token = Arc::new(DrJobControl::default());
        let worker_source = source.clone();
        let worker_token = token.clone();
        let worker = std::thread::spawn(move || {
            worker_source.histogram_checked(1, REFERENCE_WORKING_BYTES, 0, Some(&worker_token))
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while token.progress() < 0.01 && !worker.is_finished() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            !worker.is_finished(),
            "fixture must still be counting at cancellation"
        );
        let began = Instant::now();
        token.cancel();
        assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
        assert!(began.elapsed() < Duration::from_secs(2));
        eprintln!("live u16 histogram cancellation: {:?}", began.elapsed());
        assert_eq!(token.scratch_bytes(), 0);
        assert_eq!(
            source.to_rgba16().ok().unwrap()[0],
            [12345, 34567, 45678, 65535]
        );
    }
}
