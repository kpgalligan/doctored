//! Smart Sharpen on a 32-bit float layer: the whole filter runs on the
//! layer's display-encoded copy (`float_filter_route`, `Domain::Display`;
//! `sampling_smart_sharpen`'s module doc states the rule). The oracles are
//! `float_filters_display`'s: the 16-bit twin through the restated encoding.
//! The bounds, declared before the first run and never widened:
//!
//! - TWIN. A float layer built from 16-bit codes, Lens and Motion removal
//!   with Reduce Noise and the fades on, at full coverage: every component
//!   whose 16-bit result is not 0 or 65535 has `|E(out) − c16/65535| ≤
//!   4/65535`.
//! - GAUSSIAN. A float layer's Gaussian is the FIR at every σ, cut where its
//!   tails are under 1e-7 (`sampling_gaussian::runs_fast`), wider than the
//!   16-bit `ceil(3σ)` kernel, so the Gaussian removal has no 16-bit twin
//!   (it measured 8 codes off one). It is held instead to legacy Sharpen,
//!   which is Unsharp Mask amount 1 on the same encoded copy: Smart Sharpen
//!   Gaussian at amount 100, Reduce Noise 0 and no fade equals `sharpen
//!   {amount σ}` bit for bit, values above white included.
//! - ABOVE 1. A 32×32 block of 8.0 in an SDR plate: at the block's centre
//!   every removal leaves every channel finite and above 1.0, and no sample
//!   anywhere is non-finite.
//! - The operator table admits `smart_sharpen_filter`, and the float plan
//!   holds the encoded copy beside the kernel's own plan.
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use serde_json::{json, Value};
use std::ffi::CStr;

use crate::float_filters::{filtered, plan, samples, solid_doc, Picture};
use crate::float_filters_display::{against_twin, float_of, plate_codes};

fn requests() -> Vec<Value> {
    vec![
        json!({"amount": 150, "radius": 1.5, "noise": 30, "remove": "gaussian",
            "shadow_fade": 50, "shadow_tonal_width": 40, "shadow_radius": 2,
            "highlight_fade": 40, "highlight_tonal_width": 60, "highlight_radius": 3}),
        json!({"amount": 200, "radius": 2.6, "noise": 20, "remove": "lens",
            "shadow_fade": 60, "highlight_fade": 60}),
        json!({"amount": 120, "radius": 1.5, "noise": 10, "remove": "motion", "angle": 30,
            "highlight_fade": 30, "highlight_radius": 2}),
    ]
}

#[test]
fn lens_and_motion_match_their_16_bit_twin_on_sdr_values() {
    let (w, h) = (32_u32, 24_u32);
    let codes = plate_codes(w, h);
    for params in requests().into_iter().skip(1) {
        against_twin(w, h, &codes, "smart_sharpen", &params);
    }
}

#[test]
fn gaussian_without_noise_or_fades_is_legacy_sharpen_bit_for_bit() {
    let doc = solid_doc(&bright());
    // Radii an f32 holds exactly: Sharpen keeps its sigma as an f32.
    for radius in [0.75, 1.5, 3.0] {
        let smart = filtered(
            &doc,
            "smart_sharpen",
            &json!({"amount": 100, "radius": radius, "noise": 0, "remove": "gaussian"}),
        );
        let legacy = filtered(&doc, "sharpen", &json!({"amount": radius}));
        assert!(
            smart
                .iter()
                .zip(&legacy)
                .all(|(a, b)| a.map(f32::to_bits) == b.map(f32::to_bits)),
            "radius {radius}"
        );
    }
}

/// 64×64: the SDR plate's light with a 32×32 block of 8.0 at (16, 16).
fn bright() -> Picture {
    let (w, h) = (64_u32, 64_u32);
    let doc = float_of(w, h, &plate_codes(w, h));
    let mut pixels = samples(&doc);
    for y in 16..48 {
        for x in 16..48 {
            pixels[(y * w + x) as usize] = [8.0, 8.0, 8.0, 1.0];
        }
    }
    Picture { w, h, pixels }
}

#[test]
fn a_value_above_white_stays_above_white() {
    let doc = solid_doc(&bright());
    let centre = 32 * 64 + 32;
    for params in requests() {
        let out = filtered(&doc, "smart_sharpen", &params);
        assert!(out.iter().flatten().all(|v| v.is_finite()), "{params}");
        let p = out[centre];
        assert!(p[..3].iter().all(|v| *v > 1.0), "{params}: {p:?}");
    }
}

#[test]
fn the_operator_table_admits_it_and_the_plan_holds_the_encoded_copy() {
    let table = unsafe { dr_float_operators() };
    let text = unsafe { CStr::from_ptr(table) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(table) };
    let table: Value = serde_json::from_str(&text).unwrap();
    assert!(table["admitted"]
        .as_array()
        .unwrap()
        .contains(&json!("smart_sharpen_filter")));
    let doc = solid_doc(&bright());
    let n = 64 * 64_u64;
    for params in requests() {
        let planned = plan(&doc, "smart_sharpen", &params).unwrap();
        // The apply stage (40 bytes a pixel), the encoded copy at 5/4 and
        // the borrowed coverage, at least.
        assert!(
            planned["scratch_bytes"].as_u64().unwrap() >= n * 40 + n * 20,
            "{params}: {planned}"
        );
    }
}
