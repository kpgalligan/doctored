//! The shared Gaussian neighbourhood (the destructive
//! Unsharp Mask and High Pass) keeps its ceil(3σ) FIR below σ4, bit-identical
//! to before (now a row ring instead of a whole horizontal plane). The
//! radius-independent recursive Gaussian from σ4 up is held to its clause in
//! `gaussian_radius_independent.rs`. Every expectation is black-box through
//! the C boundary.
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_filters::dr_image_filter_op;
use serde_json::{json, Value};
use std::ffi::CString;
use std::ptr;

use crate::common::sha256;

const W: u32 = 131;
const H: u32 = 300;

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Splitmix noise colour; alpha 50% inside a centred disc of radius 50 and
/// opaque outside, so no output alpha is ever zero.
fn noise_disc() -> Vec<[u16; 4]> {
    let mut state = 0x5EED_u64;
    (0..W * H)
        .map(|i| {
            let v = splitmix(&mut state);
            let (x, y) = (f64::from(i % W) + 0.5, f64::from(i / W) + 0.5);
            let (dx, dy) = (x - f64::from(W) / 2.0, y - f64::from(H) / 2.0);
            let alpha = if dx * dx + dy * dy <= 50.0 * 50.0 {
                32768
            } else {
                65535
            };
            [v as u16, (v >> 16) as u16, (v >> 32) as u16, alpha]
        })
        .collect()
}

/// Byte `dr_image_filter_op` on the fixture's byte codes.
fn byte_op(pixels: &[[u16; 4]], op: &str, params: Value) -> Vec<u8> {
    let bytes: Vec<u8> = pixels
        .iter()
        .flat_map(|p| p.map(|v| ((u32::from(v) + 128) / 257) as u8))
        .collect();
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), W, H) };
    assert!(!img.is_null());
    let op = CString::new(op).unwrap();
    let params = CString::new(params.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), ptr::null(), &mut err) };
    assert!(err.is_null() && !out.is_null());
    let len = (W * H * 4) as usize;
    let result = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), len) }.to_vec();
    unsafe {
        dr_image_free(out);
        dr_image_free(img);
    }
    result
}

/// Below σ4 the FIR's output is unchanged: SHA-256 pins captured on the tree
/// before the row ring and the switch (HEAD 10c8007).
#[test]
fn the_fir_arm_is_bit_identical_below_the_switch() {
    let pixels = noise_disc();
    let answers = [
        (
            "byte unsharp σ2",
            sha256::sha256_hex(&byte_op(
                &pixels,
                "unsharp_mask",
                json!({"radius":2,"amount":1,"threshold":0}),
            )),
        ),
        (
            "byte high_pass σ3.9",
            sha256::sha256_hex(&byte_op(&pixels, "high_pass", json!({"radius":3.9}))),
        ),
    ];
    // Captured at HEAD 10c8007, before the ring and the switch.
    let pins: [&str; 2] = [
        "9159189574cc9701d27e10d514eacb20c806f3f5741e1f613797865917e3ee4a",
        "3801cb8bad5a8cd3f0312eff8d230eca0a4651b49d7ef822bbe070ad43ad7c7b",
    ];
    for ((name, digest), pin) in answers.iter().zip(pins) {
        assert_eq!(digest, pin, "{name}");
    }
}
