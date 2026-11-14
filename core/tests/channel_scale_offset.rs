//! Apply Image's Scale and Offset for Add and Subtract, through
//! `dr_blend_planes`, `dr_blend_planes_rgb` and the typed plane op
//! (`dr_selection_u16`, `"kind":"blend"`), against an oracle computed in f64
//! from the codes: `v = clamp((b ± s) / scale + offset)`, faded by opacity,
//! rounded. A result may differ from the oracle by one code only where the
//! exact value lies within a hair of a half code (the blenders run in f32).
use std::ffi::c_int;
use std::ptr;

use crate::common::{BLEND_ADDITION, BLEND_MULTIPLY, BLEND_SUBTRACT};
use crate::native_scalar::{error, text, LIMIT};
use doctored_core::ffi_channel::{dr_blend_planes, dr_blend_planes_rgb};
use doctored_core::ffi_selection_u16::dr_selection_u16;
use serde_json::json;

const SCALES: [f32; 3] = [1.0, 1.5, 2.0];
/// Offsets in 8-bit levels; the exports take them over 255.
const OFFSETS: [f32; 4] = [-128.0, 0.0, 64.0, 128.0];

/// The exact result in codes of `max`, before rounding.
fn oracle(b: f64, s: f64, subtract: bool, scale: f64, offset: f64, opacity: f64, max: f64) -> f64 {
    let (b, s) = (b / max, s / max);
    let sum = if subtract { b - s } else { b + s };
    let v = (sum / scale + offset).clamp(0.0, 1.0);
    (b + (v - b) * opacity) * max
}

/// `got` is the oracle's rounding, or one code off where the exact value is
/// within `hair` codes of a half.
fn agrees(got: u32, exact: f64, hair: f64) -> bool {
    let rounded = exact.round();
    if f64::from(got) == rounded {
        return true;
    }
    let half_distance = (exact - exact.floor() - 0.5).abs();
    half_distance < hair && (f64::from(got) - exact).abs() < 1.0
}

/// Every (base, source) byte pair: base down the rows, source across.
fn byte_pairs() -> (Vec<u8>, Vec<u8>) {
    let mut base = Vec::with_capacity(65536);
    let mut source = Vec::with_capacity(65536);
    for b in 0..=255u8 {
        for s in 0..=255u8 {
            base.push(b);
            source.push(s);
        }
    }
    (base, source)
}

fn blend(
    base: &mut [u8],
    source: &[u8],
    mode: c_int,
    opacity: f32,
    scale: f32,
    offset: f32,
) -> bool {
    unsafe {
        dr_blend_planes(
            base.as_mut_ptr(),
            source.as_ptr(),
            256,
            256,
            mode,
            opacity,
            false,
            false,
            scale,
            offset,
        )
    }
}

#[test]
fn byte_add_and_subtract_follow_the_scaled_sum_for_every_pair() {
    let (base, source) = byte_pairs();
    for (mode, subtract) in [(BLEND_ADDITION, false), (BLEND_SUBTRACT, true)] {
        for scale in SCALES {
            for offset in OFFSETS {
                let mut out = base.clone();
                assert!(blend(&mut out, &source, mode, 1.0, scale, offset / 255.0));
                for i in 0..out.len() {
                    let exact = oracle(
                        f64::from(base[i]),
                        f64::from(source[i]),
                        subtract,
                        f64::from(scale),
                        f64::from(offset) / 255.0,
                        1.0,
                        255.0,
                    );
                    assert!(
                        agrees(u32::from(out[i]), exact, 1e-3),
                        "mode {mode} scale {scale} offset {offset}: b {} s {} gave {} for {exact}",
                        base[i],
                        source[i],
                        out[i]
                    );
                }
            }
        }
    }
}

#[test]
fn byte_scaled_blend_fades_by_opacity_and_inverts_first() {
    let (base, source) = byte_pairs();
    let mut out = base.clone();
    let ok = unsafe {
        dr_blend_planes(
            out.as_mut_ptr(),
            source.as_ptr(),
            256,
            256,
            BLEND_SUBTRACT,
            0.5,
            true,
            true,
            2.0,
            128.0 / 255.0,
        )
    };
    assert!(ok);
    for i in 0..out.len() {
        // Inversion first: the complement is both blended and faded from.
        let (b, s) = (255.0 - f64::from(base[i]), 255.0 - f64::from(source[i]));
        let exact = oracle(b, s, true, 2.0, 128.0 / 255.0, 0.5, 255.0);
        assert!(
            agrees(u32::from(out[i]), exact, 1e-3),
            "pair {i}: {} for {exact}",
            out[i]
        );
    }
}

#[test]
fn rgb_scaled_blend_matches_the_oracle_per_component() {
    let (base, source) = byte_pairs();
    // Three different planes: each component its own pairing.
    let base_g: Vec<u8> = base.iter().map(|v| v.wrapping_mul(7)).collect();
    let base_b: Vec<u8> = source.iter().map(|v| 255 - v).collect();
    let source_g: Vec<u8> = source.iter().map(|v| v.wrapping_mul(13)).collect();
    let source_b: Vec<u8> = base.clone();
    let bases = [base.clone(), base_g, base_b];
    let sources = [source.clone(), source_g, source_b];
    for (mode, subtract) in [(BLEND_ADDITION, false), (BLEND_SUBTRACT, true)] {
        let mut out = bases.clone();
        let [r, g, b] = &mut out;
        let ok = unsafe {
            dr_blend_planes_rgb(
                r.as_mut_ptr(),
                g.as_mut_ptr(),
                b.as_mut_ptr(),
                sources[0].as_ptr(),
                sources[1].as_ptr(),
                sources[2].as_ptr(),
                256,
                256,
                mode,
                0.75,
                false,
                false,
                1.5,
                -64.0 / 255.0,
            )
        };
        assert!(ok);
        for c in 0..3 {
            for i in 0..65536 {
                let exact = oracle(
                    f64::from(bases[c][i]),
                    f64::from(sources[c][i]),
                    subtract,
                    1.5,
                    -64.0 / 255.0,
                    0.75,
                    255.0,
                );
                assert!(
                    agrees(u32::from(out[c][i]), exact, 1e-3),
                    "component {c} pair {i}: {} for {exact}",
                    out[c][i]
                );
            }
        }
    }
}

#[test]
fn refused_scale_and_offset_leave_every_buffer_untouched() {
    let (base, source) = byte_pairs();
    // A non-identity pair is Add and Subtract's only; out-of-range and
    // non-finite values are refused for every mode.
    let refused: [(c_int, f32, f32); 7] = [
        (BLEND_MULTIPLY, 2.0, 0.5),
        (BLEND_MULTIPLY, 1.0, 0.25),
        (BLEND_ADDITION, 0.5, 0.0),
        (BLEND_ADDITION, 2.5, 0.0),
        (BLEND_SUBTRACT, 2.0, 1.5),
        (BLEND_SUBTRACT, f32::NAN, 0.0),
        (BLEND_ADDITION, 1.0, f32::INFINITY),
    ];
    for (mode, scale, offset) in refused {
        let mut out = base.clone();
        assert!(!blend(&mut out, &source, mode, 1.0, scale, offset));
        assert_eq!(out, base, "mode {mode} scale {scale} offset {offset}");
        let mut planes = [base.clone(), base.clone(), base.clone()];
        let [r, g, b] = &mut planes;
        let ok = unsafe {
            dr_blend_planes_rgb(
                r.as_mut_ptr(),
                g.as_mut_ptr(),
                b.as_mut_ptr(),
                source.as_ptr(),
                source.as_ptr(),
                source.as_ptr(),
                256,
                256,
                mode,
                1.0,
                false,
                false,
                scale,
                offset,
            )
        };
        assert!(!ok);
        assert!(planes.iter().all(|p| *p == base));
    }
    // The identity pair is every mode's table, Multiply included.
    let mut out = base.clone();
    assert!(blend(&mut out, &source, BLEND_MULTIPLY, 1.0, 1.0, 0.0));
}

/// The typed plane op: `channels` planes of base then source.
fn typed(w: u32, operation: serde_json::Value, input: &[u16], outputs: usize) -> Option<Vec<u16>> {
    let request = text(operation);
    let mut out = vec![7u16; outputs];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_selection_u16(
            w,
            1,
            request.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let message = unsafe { error(err) };
    if ok {
        Some(out)
    } else {
        assert!(!message.is_empty());
        None
    }
}

#[test]
fn typed_scaled_blend_matches_the_oracle_at_sixteen_bits() {
    // A spread of 16-bit codes, every pairing of them.
    let codes: Vec<u16> = (0..64u32)
        .map(|i| ((i * 65535) / 63) as u16 ^ ((i as u16 * 37) & 0xff))
        .collect();
    let mut base = Vec::new();
    let mut source = Vec::new();
    for &b in &codes {
        for &s in &codes {
            base.push(b);
            source.push(s);
        }
    }
    let n = base.len();
    let w = n as u32;
    for (mode, subtract) in [(BLEND_ADDITION, false), (BLEND_SUBTRACT, true)] {
        for (scale, offset) in [(2.0, 0.5), (1.5, -0.25), (1.25, 0.0)] {
            let request = json!({"kind":"blend","channels":1,"mode":mode,"opacity":0.8,
                "scale":scale,"offset":offset});
            let input = [base.as_slice(), source.as_slice()].concat();
            let out = typed(w, request, &input, n).expect("a valid scaled blend");
            for i in 0..n {
                let exact = oracle(
                    f64::from(base[i]),
                    f64::from(source[i]),
                    subtract,
                    scale,
                    offset,
                    0.8,
                    65535.0,
                );
                assert!(
                    agrees(u32::from(out[i]), exact, 0.05),
                    "mode {mode} scale {scale} offset {offset}: b {} s {} gave {} for {exact}",
                    base[i],
                    source[i],
                    out[i]
                );
            }
        }
    }
    // Three planes as one colour take the same arithmetic per component.
    let rgb_input = [
        base.as_slice(),
        source.as_slice(),
        base.as_slice(),
        source.as_slice(),
        base.as_slice(),
        source.as_slice(),
    ]
    .concat();
    let request = json!({"kind":"blend","channels":3,"mode":BLEND_SUBTRACT,"opacity":1,
        "scale":2,"offset":0.5});
    let out = typed(w, request, &rgb_input, 3 * n).expect("a valid RGB scaled blend");
    for c in 0..3 {
        for i in 0..n {
            let (b, s) = (rgb_input[c * n + i], rgb_input[3 * n + c * n + i]);
            let exact = oracle(f64::from(b), f64::from(s), true, 2.0, 0.5, 1.0, 65535.0);
            assert!(agrees(u32::from(out[c * n + i]), exact, 0.05));
        }
    }
    // Refusals: another mode, a value out of range.
    for (mode, scale, offset) in [
        (BLEND_MULTIPLY, 2.0, 0.5),
        (BLEND_ADDITION, 3.0, 0.0),
        (BLEND_SUBTRACT, 1.0, -2.0),
    ] {
        let request = json!({"kind":"blend","channels":1,"mode":mode,"opacity":1,
            "scale":scale,"offset":offset});
        assert!(typed(
            w,
            request,
            &[base.as_slice(), source.as_slice()].concat(),
            n
        )
        .is_none());
    }
}
