//! The display-defined paint tools in a 32-bit float document — the
//! Paint Bucket (R2, R3), Dodge and Burn on a layer (R4) and on a mask or
//! alpha channel at 16 and 32 bits (R4m), and the Sponge (R5) — through
//! `dr_precision_doc_edit` (`bucket_fill`, `dodge_burn`, `scalar_edit`,
//! `retouch_brush`).
//!
//! The oracles are written here in binary64 from the stated rules
//! (`display_encoding`'s module doc), never read off the implementation:
//! the sRGB curve written out (E the encode, D the decode), the Dodge/Burn
//! shaping written out, and the luma weights of Rec. 709. Every R4/R5 case
//! includes 1.0, 2.0, 8.0, 0.999, −0.25 and 0.18, declared before running
//! ([`VALUES`]). Tolerances: R4 `1e-5·max(1, |want|)`; R4m the u16 rounding
//! of the f64 value (`|got − want| ≤ 0.5`, plus 1e-3 for the f32 shaping);
//! R5 `1e-5·max(1, |want|)`.

use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::float_fixture::float_doc;
use crate::native_scalar as native;
use native::*;

/// The values every R4/R5 case includes.
const VALUES: [f32; 6] = [1.0, 2.0, 8.0, 0.999, -0.25, 0.18];
const W: u32 = 6;
const H: u32 = 3;

// --------------------------------------------------------------- oracle --

/// The sRGB encode (E), written out from IEC 61966-2.1.
fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// The sRGB decode (D), written out from IEC 61966-2.1.
fn decode(e: f64) -> f64 {
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

/// Dodge/Burn's shaping at exposure `e` over `range`, written out from the
/// tool's stated curves: shadows move the black point by `e/3`, midtones
/// are the gamma `v^(1/(1+e))` / `v^(1+e)`, highlights move the white point.
fn shaped(v: f64, e: f64, range: u8, burn: bool) -> f64 {
    let k = e / 3.0;
    match (range, burn) {
        (0, false) => k + v * (1.0 - k),
        (0, true) => ((v - k) / (1.0 - k)).max(0.0),
        (1, false) => v.powf(1.0 / (1.0 + e)),
        (1, true) => v.powf(1.0 + e),
        (2, false) => (v / (1.0 - k)).min(1.0),
        (2, true) => v * (1.0 - k),
        _ => unreachable!(),
    }
}

/// R4 for one component at coverage `c`.
fn dodge_burn(v: f64, e: f64, range: u8, burn: bool, c: f64) -> f64 {
    let t = if v > 1.0 {
        v * decode(shaped(1.0, e, range, burn))
    } else if v < 0.0 {
        v + decode(shaped(0.0, e, range, burn))
    } else {
        decode(shaped(encode(v), e, range, burn))
    };
    (v + c * (t - v)).clamp(-65504.0, 65504.0)
}

/// R1's code of one linear component.
fn code(v: f64) -> i64 {
    (encode(v) * 65535.0).round() as i64
}

fn close(got: f32, want: f64, tol: f64) -> bool {
    (f64::from(got) - want).abs() <= tol * want.abs().max(1.0)
}

// ------------------------------------------------------------- fixtures --

fn doc(pixels: Vec<[f32; 4]>) -> PrecisionDocument {
    float_doc(W, H, &IccProfile::srgb(), &[pixels])
}

fn layer32(d: &PrecisionDocument) -> Vec<[f32; 4]> {
    d.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn bits(p: [f32; 4]) -> [u32; 4] {
    p.map(f32::to_bits)
}

/// A gray float pixel, opaque.
fn gray(v: f32) -> [f32; 4] {
    [v, v, v, 1.0]
}

/// Every pixel of the canvas holds `VALUES` across its channels, shifted per
/// pixel so each value meets each channel; the last pixel is alpha 0 with
/// hidden colour.
fn tone_pixels() -> Vec<[f32; 4]> {
    let n = (W * H) as usize;
    (0..n)
        .map(|i| {
            if i == n - 1 {
                return [0.5, 4.0, -0.25, 0.0];
            }
            let at = |c: usize| VALUES[(i + 2 * c) % VALUES.len()];
            [at(0), at(1), at(2), 1.0]
        })
        .collect()
}

fn bucket(x: u32, y: u32, tolerance: u32, contiguous: bool, color: Value, space: &str) -> Value {
    json!({"op":"bucket_fill","layer":0,"color":color,"color_space":space,"opacity":1.0,
        "x":x,"y":y,"tolerance":tolerance,"contiguous":contiguous})
}

fn full() -> Vec<u16> {
    vec![65535; (W * H) as usize]
}

// ----------------------------------------------------------- the bucket --

#[test]
fn the_bucket_compares_r1_codes_at_the_tolerance_boundary() {
    // The seed and two neighbours exactly 100 and 101 codes above it on the
    // display copy (D of a code, so R1 lands on the code), the rest black.
    let at = |k: i64| decode(k as f64 / 65535.0) as f32;
    let mut pixels = vec![gray(0.0); (W * H) as usize];
    pixels[0] = gray(at(30000));
    pixels[1] = gray(at(30100));
    pixels[2] = gray(at(30101));
    assert_eq!(
        code(f64::from(pixels[1][0])) - code(f64::from(pixels[0][0])),
        100
    );
    assert_eq!(
        code(f64::from(pixels[2][0])) - code(f64::from(pixels[0][0])),
        101
    );
    let d = doc(pixels.clone());
    let ink = decode(0.5);
    for (tolerance, filled) in [(99, 1), (100, 2), (101, 3)] {
        let out = edit(
            &d,
            bucket(
                0,
                0,
                tolerance,
                false,
                json!([0.5, 0.5, 0.5, 1.0]),
                "document",
            ),
            &full(),
        )
        .expect("the bucket fills");
        for (i, (got, was)) in layer32(&out).iter().zip(&pixels).enumerate() {
            if i < filled {
                // The profile's curve is the IEC 1024-entry table, within
                // 2e-5 of the formula (`float_fill_stroke.rs`'s bound).
                assert!(
                    (0..3).all(|c| close(got[c], ink, 2e-5)) && got[3] == 1.0,
                    "tolerance {tolerance} pixel {i}: {got:?}"
                );
            } else {
                assert_eq!(bits(*got), bits(*was), "tolerance {tolerance} pixel {i}");
            }
        }
    }
}

#[test]
fn a_seed_at_8_with_tolerance_0_fills_1_2_and_8_but_not_0_999() {
    // Above white compares as white; 0.999 is a few codes below it. Row 0
    // holds the six values, row 1 is 0.999 throughout and row 2 is 8.0, so
    // a contiguous fill from (2, 0) cannot reach row 2.
    assert!(code(0.999) < 65535);
    let mut pixels: Vec<[f32; 4]> = VALUES.iter().map(|v| gray(*v)).collect();
    pixels.extend([gray(0.999); W as usize]);
    pixels.extend([gray(8.0); W as usize]);
    let d = doc(pixels.clone());
    let four = bits([4.0, 4.0, 4.0, 1.0]);
    for contiguous in [false, true] {
        // Linear 4.0 is the ink as given, bit for bit.
        let out = edit(
            &d,
            bucket(2, 0, 0, contiguous, json!([4.0, 4.0, 4.0, 1.0]), "linear"),
            &full(),
        )
        .expect("the bucket fills");
        for (i, (got, was)) in layer32(&out).iter().zip(&pixels).enumerate() {
            let reached = was[0] >= 1.0 && (!contiguous || i < W as usize);
            if reached {
                assert_eq!(bits(*got), four, "contiguous {contiguous} pixel {i}");
            } else {
                assert_eq!(bits(*got), bits(*was), "contiguous {contiguous} pixel {i}");
            }
        }
    }
}

#[test]
fn a_selection_gates_the_bucket_and_a_mask_fills_through_scalar_edit() {
    let n = (W * H) as usize;
    let d = doc(vec![gray(2.0); n]);
    let mut request = bucket(0, 0, 0, false, json!([0.25, 0.25, 0.25, 1.0]), "linear");
    request["selection"] = json!(true);
    // Selected on the even pixels only.
    let selection: Vec<u16> = (0..n).map(|i| if i % 2 == 0 { 65535 } else { 0 }).collect();
    let mut input = full();
    input.extend(&selection);
    let out = edit(&d, request.clone(), &input).expect("the bucket fills");
    for (i, got) in layer32(&out).iter().enumerate() {
        let want = if i % 2 == 0 { 0.25 } else { 2.0 };
        assert_eq!(bits(*got), bits(gray(want)), "pixel {i}");
    }
    // A seed outside the selection fills nothing.
    request["x"] = json!(1);
    assert!(edit(&d, request, &input).is_none());
    // A layer mask is u16 coverage at every depth: the scalar bucket, its
    // region over the stored codes.
    let ramp: Vec<u16> = (0..n).map(|i| if i < 9 { 1000 } else { 5000 }).collect();
    let masked = edit(
        &d,
        json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true}),
        &ramp,
    )
    .expect("a mask");
    let out = scalar(
        &masked,
        "mask",
        json!({"kind":"bucket_fill","x":0,"y":0,"tolerance":0,"contiguous":true,"value":40000}),
        false,
        &[],
    )
    .expect("the mask fills");
    let plane = plane(&out, "mask", false);
    for (i, got) in plane.iter().enumerate() {
        assert_eq!(*got, if i < 9 { 40000 } else { 5000 }, "mask sample {i}");
    }
}

// ------------------------------------------------------ dodge and burn --

fn dodge(exposure: f64, range: u8, burn: bool) -> Value {
    json!({"op":"dodge_burn","layer":0,"exposure":exposure,"range":range,"burn":burn})
}

#[test]
fn dodge_and_burn_follow_r4_in_every_range_and_coverage() {
    let pixels = tone_pixels();
    let d = doc(pixels.clone());
    let n = (W * H) as usize;
    for range in 0..3u8 {
        for burn in [false, true] {
            for stroke in [32768u16, 65535] {
                let c = f64::from(stroke) / 65535.0;
                let out = edit(&d, dodge(0.6, range, burn), &vec![stroke; n])
                    .expect("dodge/burn changes the layer");
                for (i, (got, was)) in layer32(&out).iter().zip(&pixels).enumerate() {
                    if was[3] == 0.0 {
                        assert_eq!(bits(*got), bits(*was), "alpha 0 is skipped");
                        continue;
                    }
                    assert_eq!(got[3].to_bits(), was[3].to_bits(), "alpha is kept");
                    for ch in 0..3 {
                        let want = dodge_burn(f64::from(was[ch]), 0.6, range, burn, c);
                        assert!(
                            close(got[ch], want, 1e-5),
                            "range {range} burn {burn} coverage {c} pixel {i} channel {ch}: \
                             {} from {}, want {want}",
                            got[ch],
                            was[ch]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dodge_and_burn_on_a_mask_and_a_channel_follow_r4m_at_16_and_32_bits() {
    let n = (W * H) as usize;
    let ramp: Vec<u16> = (0..n).map(|i| (i as u32 * 3851 % 65536) as u16).collect();
    let stroke: Vec<u16> = (0..n).map(|i| [65535, 32768, 0, 12000][i % 4]).collect();
    let float = doc(vec![gray(2.0); n]);
    let sixteen = native::doc(W, H, vec![[20000, 30000, 40000, 65535]; n]);
    for (depth, base) in [("32", float), ("16", sixteen)] {
        let masked = edit(
            &base,
            json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true}),
            &ramp,
        )
        .expect("a mask");
        let with_channel = edit(
            &masked,
            json!({"op":"set_channel","index":null,"name":"Coverage"}),
            &ramp,
        )
        .expect("a channel");
        for target in ["mask", "channel"] {
            for (range, burn) in [(0u8, false), (1, true), (2, false), (1, false)] {
                let operation =
                    json!({"kind":"dodge_burn","exposure":0.5,"range":range,"burn":burn});
                let out = scalar(&with_channel, target, operation, false, &stroke)
                    .expect("the plane changes");
                let got = plane(&out, target, false);
                for i in 0..n {
                    let o = f64::from(ramp[i]);
                    let t = f64::from(stroke[i]) / 65535.0;
                    let value = 65535.0 * shaped(o / 65535.0, 0.5, range, burn);
                    let want = (o * (1.0 - t) + value * t).clamp(0.0, 65535.0);
                    assert!(
                        (f64::from(got[i]) - want).abs() <= 0.5 + 1e-3,
                        "{depth}-bit {target} range {range} burn {burn} sample {i}: {} want {want}",
                        got[i]
                    );
                }
            }
        }
    }
}

// ------------------------------------------------------------ the sponge --

fn sponge(kind: &str, strength: f64) -> Value {
    json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":strength,"selection":false})
}

#[test]
fn the_sponge_follows_r5_unclamped_and_holds_the_stored_bound() {
    let mut pixels = tone_pixels();
    // A saturated highlight whose doubled chroma passes the stored bound.
    pixels[0] = [60000.0, 0.0, 0.0, 1.0];
    let d = doc(pixels.clone());
    let n = (W * H) as usize;
    let luma = |p: [f32; 4]| {
        0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
    };
    for kind in ["saturate", "desaturate"] {
        for strength in [1.0, 0.5] {
            let out = edit(&d, sponge(kind, strength), &vec![65535; n]).expect("the sponge");
            for (i, (got, was)) in layer32(&out).iter().zip(&pixels).enumerate() {
                if was[3] == 0.0 {
                    assert_eq!(bits(*got), bits(*was), "alpha 0 is skipped");
                    continue;
                }
                let y = luma(*was);
                for c in 0..3 {
                    let v = f64::from(was[c]);
                    let target = if kind == "desaturate" {
                        y
                    } else {
                        y + 2.0 * (v - y)
                    };
                    let want = (v + strength * (target - v)).clamp(-65504.0, 65504.0);
                    assert!(
                        close(got[c], want, 1e-5),
                        "{kind} {strength} pixel {i} channel {c}: {} want {want}",
                        got[c]
                    );
                }
                assert_eq!(got[3].to_bits(), was[3].to_bits());
            }
            if kind == "saturate" && strength == 1.0 {
                assert_eq!(layer32(&out)[0][0], 65504.0, "held to the stored bound");
            }
        }
    }
}

#[test]
fn exposure_0_and_strength_0_change_nothing() {
    let d = doc(tone_pixels());
    let n = (W * H) as usize;
    assert!(edit(&d, dodge(0.0, 1, false), &vec![65535; n]).is_none());
    assert!(edit(&d, sponge("saturate", 0.0), &vec![65535; n]).is_none());
    let masked = edit(
        &d,
        json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true}),
        &vec![30000; n],
    )
    .expect("a mask");
    let operation = json!({"kind":"dodge_burn","exposure":0.0,"range":1,"burn":false});
    assert!(scalar(&masked, "mask", operation, false, &vec![65535; n]).is_none());
}
