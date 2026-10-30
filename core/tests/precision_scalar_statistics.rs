//! Scalar histograms use independent integer sums/bin/endpoint oracles. Hidden
//! RGB, zero alpha and off-canvas raw mask storage remain real scalar samples.
use crate::native_scalar as common;
use common::*;
use doctored_core::{
    ffi::dr_string_free,
    ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_scalar_statistics},
    job_control::DrJobControl,
    precision_document::PrecisionDocument,
};
use serde_json::{json, Value};
use std::{ffi::CStr, ptr};

fn histogram(doc: &PrecisionDocument, target: &str, coverage: &[u16], stride: u32) -> Value {
    let request = json!({"op":"scalar_statistics","target":target,"index":0,"selection":!coverage.is_empty(),"stride":stride});
    let planned = plan(doc, request.clone());
    assert_eq!(planned["input_samples"], coverage.len());
    let text = text(request);
    let mut err = ptr::null_mut();
    unsafe {
        let raw = dr_precision_doc_scalar_statistics(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert!(!raw.is_null(), "{}", error(err));
        assert!(err.is_null());
        let result = serde_json::from_str(CStr::from_ptr(raw).to_str().unwrap()).unwrap();
        dr_string_free(raw);
        result
    }
}
fn verify(report: &Value, values: &[u16], weights: &[u16], stride: usize) {
    let weights: Vec<u64> = if weights.is_empty() {
        vec![65535; values.len()]
    } else {
        weights.iter().map(|v| u64::from(*v)).collect()
    };
    let sum: u64 = values
        .iter()
        .zip(&weights)
        .map(|(v, w)| u64::from(*v) * w)
        .sum();
    let mass: u64 = weights.iter().sum();
    let zero: u64 = values
        .iter()
        .zip(&weights)
        .filter(|(v, w)| **v == 0 && **w > 0)
        .count() as u64;
    let full: u64 = values
        .iter()
        .zip(&weights)
        .filter(|(v, w)| **v == 65535 && **w > 0)
        .count() as u64;
    let mut bins = vec![0u64; 256];
    for (i, (&v, &weight)) in values.iter().zip(&weights).enumerate() {
        if i % stride == 0 {
            bins[((u64::from(v) * 255 + 32767) / 65535) as usize] += weight;
        }
    }
    assert_eq!(report["source"], "scalar");
    assert_eq!(report["maximum_code"], 65535);
    assert_eq!(report["coverage_denominator"], 65535);
    assert_eq!(report["full_rgb_weight_units"], mass);
    for component in 0..3 {
        assert_eq!(report["weighted_code_sums_rgba"][component], sum);
        assert_eq!(report["exact_zero_rgba"][component], zero);
        assert_eq!(report["exact_max_rgba"][component], full);
        assert_eq!(report["bin_weight_units"][component], json!(bins));
        let zero_weight: u64 = values
            .iter()
            .zip(&weights)
            .filter(|(v, _)| **v == 0)
            .map(|(_, w)| w)
            .sum();
        let full_weight: u64 = values
            .iter()
            .zip(&weights)
            .filter(|(v, _)| **v == 65535)
            .map(|(_, w)| w)
            .sum();
        assert_eq!(
            report["exact_zero_weight_units_rgba"][component],
            zero_weight
        );
        assert_eq!(
            report["exact_max_weight_units_rgba"][component],
            full_weight
        );
    }
}

#[test]
fn native_channel_exact_bins_weights_and_endpoints_ignore_display_stride() {
    let values = [0, 1, 2, 127, 128, 257, 32768, 65534, 65535];
    let document = doc(9, 1, vec![[10001, 20002, 30003, 1]; 9]);
    let document = edit(
        &document,
        json!({"op":"set_channel","name":"Fine"}),
        &values,
    )
    .unwrap();
    for weights in [
        &[][..],
        &[1, 65535, 32769, 127, 0, 255, 65534, 2, 12345][..],
    ] {
        for stride in [1, 3] {
            verify(
                &histogram(&document, "channel", weights, stride),
                &values,
                weights,
                stride as usize,
            );
        }
    }
    assert_eq!(document.channels()[0].plane().samples(), values);
}

#[test]
fn raw_mask_statistics_map_canvas_weights_without_effective_mask_properties() {
    let document = doc(3, 1, vec![[10001, 20002, 30003, 1]; 3]);
    let values = [0, 1, 65535, 65534, 65535];
    let document = edit(
        &document,
        json!({"op":"set_mask","layer":0,"width":5,"height":1,
        "x":-1,"density":0.3,"feather":1.7,"enabled":false,"linked":false}),
        &values,
    )
    .unwrap();
    verify(&histogram(&document, "mask", &[], 1), &values, &[], 1);
    let result = histogram(&document, "mask", &[1, 32768, 0], 2);
    verify(&result, &values, &[0, 1, 32768, 0, 0], 2);
    assert_eq!(result["scope"]["x"], -1);
    assert_eq!(result["scope"]["width"], 5);
    assert_eq!(
        document.layer(0).unwrap().mask().unwrap().plane().samples(),
        values
    );
}

#[test]
fn color_and_alpha_planes_count_hidden_rgb_and_zero_alpha_as_scalar_values() {
    let pixels = vec![
        [0, 1, 2, 0],
        [65535, 32769, 1, 1],
        [257, 65535, 0, 65535],
        [1001, 2002, 3003, 257],
    ];
    let document = doc(4, 1, pixels.clone());
    for (index, target) in ["red", "green", "blue", "alpha"].iter().enumerate() {
        let values: Vec<_> = pixels.iter().map(|p| p[index]).collect();
        verify(&histogram(&document, target, &[], 1), &values, &[], 1);
    }
    assert_eq!(
        document.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        pixels
    );
}

#[test]
fn scalar_statistics_ffi_null_length_cancel_and_resource_refusals_are_atomic() {
    let document = doc(3, 1, vec![[1001, 2002, 3003, 1]; 3]);
    let request =
        text(json!({"op":"scalar_statistics","target":"alpha","index":0,"selection":true}));
    let coverage = [1, 32769, 65535];
    let control = DrJobControl::default();
    control.cancel();
    unsafe {
        let cases = [
            (
                ptr::null(),
                request.as_ptr(),
                coverage.as_ptr(),
                3,
                LIMIT,
                ptr::null(),
            ),
            (
                &document as *const _,
                ptr::null(),
                coverage.as_ptr(),
                3,
                LIMIT,
                ptr::null(),
            ),
            (
                &document as *const _,
                request.as_ptr(),
                ptr::null(),
                3,
                LIMIT,
                ptr::null(),
            ),
            (
                &document as *const _,
                request.as_ptr(),
                coverage.as_ptr(),
                2,
                LIMIT,
                ptr::null(),
            ),
            (
                &document as *const _,
                request.as_ptr(),
                coverage.as_ptr(),
                3,
                1,
                ptr::null(),
            ),
            (
                &document as *const _,
                request.as_ptr(),
                coverage.as_ptr(),
                3,
                LIMIT,
                &control as *const _,
            ),
        ];
        for (doc, req, input, len, limit, control) in cases {
            let mut err = ptr::null_mut();
            assert!(dr_precision_doc_scalar_statistics(
                doc, req, input, len, limit, 0, control, &mut err
            )
            .is_null());
            assert!(!err.is_null());
            dr_string_free(err);
        }
        assert!(dr_precision_doc_scalar_statistics(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
    assert_eq!(coverage, [1, 32769, 65535]);
    for operation in [
        json!({"op":"scalar_statistics","target":"effective_mask","index":0}),
        json!({"op":"scalar_statistics","target":"alpha","index":0,"stride":0}),
        json!({"op":"scalar_statistics","target":"alpha","index":0,"unknown":1}),
    ] {
        assert!(
            doctored_core::precision_scalar_statistics::ScalarStatistics::parse(
                &operation.to_string()
            )
            .is_err()
        );
    }
}

/// The committed float scene: skipped gracefully when absent.
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";
/// The core's sentence, restated here as the oracle.
const FLOAT_COMPONENT_REFUSAL: &str = "scalar statistics of red, green, blue or alpha are unavailable in a 32-bit float document: its raster components are float samples, not u16 codes; its masks and channels are u16 coverage and are measured as at 16 bits";

/// Calls `dr_precision_doc_edit_plan` and `dr_precision_doc_scalar_statistics`
/// (selection false, length 0) expecting NULL, and returns both errors.
fn refusals(doc: &PrecisionDocument, request: Value) -> (String, String) {
    let text = text(request);
    unsafe {
        let mut err = ptr::null_mut();
        let planned = dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err);
        assert!(planned.is_null());
        let plan_error = error(err);
        let mut err = ptr::null_mut();
        let report = dr_precision_doc_scalar_statistics(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert!(report.is_null());
        (plan_error, error(err))
    }
}

#[test]
fn float_masks_and_channels_measure_as_u16_while_components_refuse_at_plan() {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let document =
        PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).expect("the fixture");
    assert!(document.is_float());
    let (w, h) = document.dimensions();
    let n = w as usize * h as usize;
    let values: Vec<u16> = (0..n)
        .map(|i| match i % 6 {
            0 => 0,
            1 => 1,
            2 => 65534,
            3 => 65535,
            _ => ((i as u64 * 7919) % 65536) as u16,
        })
        .collect();
    let channel: Vec<u16> = values.iter().rev().copied().collect();
    let weights: Vec<u16> = (0..n).map(|i| [0, 1, 32768, 65535, 12345][i % 5]).collect();
    let document = edit(
        &document,
        json!({"op":"set_mask","layer":0,"width":w,"height":h}),
        &values,
    )
    .expect("a float mask is a change");
    let document = edit(
        &document,
        json!({"op":"set_channel","name":"Probe"}),
        &channel,
    )
    .expect("a float channel is a change");
    for (target, samples) in [("mask", &values), ("channel", &channel)] {
        for coverage in [&[][..], &weights[..]] {
            for stride in [1, 3] {
                let report = histogram(&document, target, coverage, stride);
                assert_eq!(report["storage"], "u16_scalar");
                verify(&report, samples, coverage, stride as usize);
            }
        }
    }
    assert_eq!(
        document.layer(0).unwrap().mask().unwrap().plane().samples(),
        values
    );
    assert_eq!(document.channels()[0].plane().samples(), channel);
    for target in ["red", "green", "blue", "alpha"] {
        let (plan_error, read_error) = refusals(
            &document,
            json!({"op":"scalar_statistics","target":target,"index":0,"selection":false}),
        );
        assert_eq!(plan_error, FLOAT_COMPONENT_REFUSAL);
        assert_eq!(read_error, FLOAT_COMPONENT_REFUSAL);
        let text = text(json!({"op":"read_plane","target":target,"index":0}));
        let mut err = ptr::null_mut();
        let planned =
            unsafe { dr_precision_doc_edit_plan(&document, text.as_ptr(), LIMIT, 0, &mut err) };
        assert!(planned.is_null());
        assert_eq!(
            unsafe { error(err) },
            "high-depth plane read requires u16 storage"
        );
    }
}
