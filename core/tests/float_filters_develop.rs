//! Filter ▸ Presence, the Develop Filter and
//! Red Eye (the tool and Filter ▸ Remove Red Eye) on a 32-bit float layer,
//! every control of the first two included.
//!
//! Oracles and bounds, declared before the first run and never widened:
//!
//! - TWIN (`float_filters_display.rs`'s): a float layer built from 16-bit
//!   codes against the 16-bit result, `|E(out) − c16/65535| ≤ 4/65535` on
//!   every component whose 16-bit result is not 0 or 65535. Presence's
//!   Texture and Clarity, the Develop Filter's display controls (Contrast,
//!   Whites, Blacks, Texture, Clarity) on a plate whose results stay inside
//!   0…1, and Red Eye. The linear stages have their own oracles below: the
//!   16-bit ones decode through the analytic sRGB curve, not the profile's
//!   table, so in deep shadow the two depths' "linear" differ by more than
//!   the bound and the twin would test that difference, not the filter.
//! - EXPOSURE. A Develop Filter of Exposure `e` alone is `v·2^e`, the f32
//!   product, bit for bit, on every visible pixel; a transparent pixel keeps
//!   its bits.
//! - WHITE BALANCE. Temperature and Tint alone are
//!   `common::white_balance_oracle::linear_white_balance` on the stored
//!   values, within `1e-4·max(1, |ref|)`.
//! - ABOVE 1. A value above 1.0 stays above 1.0 and finite; Red Eye leaves
//!   an above-white catchlight's bits alone.
//! - D, DEHAZE (`sampling_dehaze`'s rule D), on 96×96 flat bands (a 0.85
//!   grey haze, (0.3, 0.2, 0.1), (8, 0, 0), 2.0 grey and 4.0 grey) at amounts
//!   +50, +100 and −50, against the rule restated in f64 through
//!   `float_filters_display`'s `encode`, at probes at least 6 px from a band
//!   edge: the airlight is the top 0.1% of the copy with RGB clamped to 0…1,
//!   the veil the dark value, `t = max(1 − 0.95·(a/100)·W, 0.1)`. An
//!   in-range component is within `TWIN·(1 + 1/t)` in encoded units of the
//!   rule and of a 16-bit twin of the clipped copy (a code's error is divided
//!   by `t`); at a positive amount every component above 1 is bit-exact (the
//!   2.0 and 4.0 bands, the 8.0 red); at every amount every output component
//!   is at most `max(input, 1)`.
//! - H, HIGHLIGHTS AND SHADOWS (`ShadowsHighlights::apply_float`), on flat
//!   bands (2, 2, 2), (0.5, 1.5, 0.2), 0.18 grey and (1.5, 0.3, 0.3) against
//!   the f64 restatement of `apply` with no clamp: a pixel whose encoded luma
//!   is at or above 1 is bit-exact, any other within `2·TWIN` in encoded
//!   units, at probes at least 8 px from a band edge.
//! - V, VIBRANCE AND SATURATION (the Scaled display form): against a 16-bit
//!   twin holding the `E(c)` codes, decoded and times `m`
//!   (`float_display_adjustments.rs`' method), within `5e-4·max(1, |want|)`,
//!   and within `1e-6·max(1, |want|)` of the `vibrance` adjustment applied at
//!   float.
//! - TABLE. Presence and the Develop Filter have no refused row; Select
//!   and Mask's Decontaminate Colors does.
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_retouch::RetouchEdit;
use serde_json::{json, Value};
use std::ffi::CStr;

use crate::common::white_balance_oracle::linear_white_balance;
use crate::float_adjustments::{add, float_row, render};
use crate::float_filters::Picture;
use crate::float_filters::{document, filter, filtered, hdr_picture, plan, samples, solid_doc};
use crate::float_filters_display::{against_twin, decode, encode, float_of, plate_codes, wide};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const TWIN: f64 = 4.0 / 65535.0;

/// [`plate_codes`] narrowed to 2500…52500, so a white-balance shift, an
/// exposure or a Whites move keeps every result inside 0…1 at both depths.
fn mid_codes(w: u32, h: u32) -> Vec<[u16; 4]> {
    plate_codes(w, h)
        .into_iter()
        .map(|p| {
            let narrow = |v: u16| (2500 + (u32::from(v) - 2500) * 5 / 6) as u16;
            [narrow(p[0]), narrow(p[1]), narrow(p[2]), p[3]]
        })
        .collect()
}

#[test]
fn presence_texture_and_clarity_match_their_16_bit_twin() {
    let (w, h) = (48, 40);
    let codes = mid_codes(w, h);
    for params in [
        json!({"texture": 60}),
        json!({"clarity": -40}),
        json!({"texture": -30, "clarity": 70}),
    ] {
        against_twin(w, h, &codes, "presence", &params);
    }
}

#[test]
fn the_develop_filter_filter_matches_its_16_bit_twin() {
    let (w, h) = (48, 40);
    let codes = mid_codes(w, h);
    for params in [
        json!({"contrast": 25}),
        json!({"whites": -15, "blacks": 10}),
        json!({"texture": 30, "clarity": 20}),
        json!({"contrast": -20, "whites": -10, "blacks": -5, "texture": 20, "clarity": 15}),
    ] {
        against_twin(w, h, &codes, "develop_filter", &params);
    }
}

#[test]
fn develop_filter_exposure_is_the_linear_gain_bit_for_bit() {
    let (doc, picture) = document(&hdr_picture(true));
    for e in [1.0_f64, 0.5, -1.3] {
        let gain = (e as f32).exp2();
        let out = filtered(&doc, "develop_filter", &json!({"exposure": e}));
        for (i, (got, was)) in out.iter().zip(&picture.pixels).enumerate() {
            let want = if was[3] > 0.0 {
                [was[0] * gain, was[1] * gain, was[2] * gain, was[3]]
            } else {
                *was
            };
            assert_eq!(
                got.map(f32::to_bits),
                want.map(f32::to_bits),
                "exposure {e} pixel {i}: {got:?} vs {want:?}"
            );
        }
    }
}

#[test]
fn develop_filter_white_balance_is_the_linear_matrix_on_stored_values() {
    let (doc, picture) = document(&hdr_picture(true));
    for (t, tint) in [(30.0_f64, 0.0_f64), (-45.0, 12.0), (0.0, -20.0)] {
        let kelvin = 1.0e6 / (1.0e6 / 6504.0 - t);
        let out = filtered(
            &doc,
            "develop_filter",
            &json!({"temperature": t, "tint": tint}),
        );
        for (i, (got, was)) in out.iter().zip(&picture.pixels).enumerate() {
            if was[3] <= 0.0 {
                assert_eq!(got.map(f32::to_bits), was.map(f32::to_bits), "{i}");
                continue;
            }
            let want = linear_white_balance([was[0], was[1], was[2]].map(f64::from), kelvin, tint);
            for c in 0..3 {
                assert!(
                    (f64::from(got[c]) - want[c]).abs() <= 1e-4 * want[c].abs().max(1.0),
                    "t {t} tint {tint} pixel {i} channel {c}: {} vs {}",
                    got[c],
                    want[c]
                );
            }
        }
    }
}

/// 64×64: the mid plate's light with a 32×32 block of 8.0 at (16, 16).
fn bright() -> Picture {
    let (w, h) = (64_u32, 64_u32);
    let doc = float_of(w, h, &mid_codes(w, h));
    let mut pixels = samples(&doc);
    for y in 16..48 {
        for x in 16..48 {
            pixels[(y * w + x) as usize] = [8.0, 8.0, 8.0, 1.0];
        }
    }
    Picture { w, h, pixels }
}

#[test]
fn presence_and_develop_filter_keep_a_value_above_white_above_it() {
    let doc = solid_doc(&bright());
    let centre = 32 * 64 + 32;
    for (op, params) in [
        ("presence", json!({"texture": 80, "clarity": 80})),
        ("presence", json!({"texture": -80, "clarity": -80})),
        (
            "develop_filter",
            json!({"exposure": 1, "contrast": 40, "whites": 30, "blacks": -20,
                "texture": 50, "clarity": 50}),
        ),
        ("develop_filter", json!({"contrast": -60, "whites": -100})),
    ] {
        let out = filtered(&doc, op, &params);
        assert!(out.iter().flatten().all(|v| v.is_finite()), "{op} {params}");
        let p = out[centre];
        assert!(p[..3].iter().all(|v| *v > 1.0), "{op} {params}: {p:?}");
    }
}

// ---- D: Dehaze --------------------------------------------------------------

const DW: u32 = 96;
const DH: u32 = 96;
/// The Dehaze bands, top to bottom.
const HAZE_BANDS: [[f32; 3]; 5] = [
    [0.85, 0.85, 0.85],
    [0.3, 0.2, 0.1],
    [8.0, 0.0, 0.0],
    [2.0, 2.0, 2.0],
    [4.0, 4.0, 4.0],
];

/// The band of row `y` among `bands` equal bands of a `DH`-row plate, and
/// whether the row is at least `margin` rows from the band's edges.
fn band_row(y: u32, bands: u32, margin: u32) -> (usize, bool) {
    let b = y * bands / DH;
    let first = (0..DH).find(|r| r * bands / DH == b).unwrap();
    let last = (0..DH).rev().find(|r| r * bands / DH == b).unwrap();
    (b as usize, y >= first + margin && y + margin <= last)
}

fn banded(bands: &[[f32; 3]]) -> Picture {
    let pixels = (0..DW * DH)
        .map(|i| {
            let (b, _) = band_row(i / DW, bands.len() as u32, 0);
            let p = bands[b];
            [p[0], p[1], p[2], 1.0]
        })
        .collect();
    Picture {
        w: DW,
        h: DH,
        pixels,
    }
}

/// Whether pixel `i` of a `bands`-band plate is a probe: `margin` px from
/// every band edge and the plate's sides.
fn probe(i: usize, bands: usize, margin: u32) -> bool {
    let (x, y) = (i as u32 % DW, i as u32 / DW);
    band_row(y, bands as u32, margin).1 && x >= margin && x + margin < DW
}

/// The encoded copy of one linear triple (`float_filters_display`'s E).
fn encoded(p: [f32; 4]) -> [f64; 3] {
    [p[0], p[1], p[2]].map(|v| encode(f64::from(v)))
}

/// Rule D's airlight, restated: the mean colour of the clipped copy's pixels
/// whose dark value `min(R, G, B)` is in the top 0.1% (a 65536-bin
/// histogram, at least one pixel).
fn airlight(pixels: &[[f32; 4]]) -> [f64; 3] {
    let clipped: Vec<[f64; 3]> = pixels
        .iter()
        .map(|p| encoded(*p).map(|v| v.clamp(0.0, 1.0)))
        .collect();
    let bin = |c: &[f64; 3]| (c[0].min(c[1]).min(c[2]) * 65535.0).round() as usize;
    let mut bins = vec![0_u64; 65536];
    clipped.iter().for_each(|c| bins[bin(c)] += 1);
    let wanted = ((clipped.len() as f64 * 0.001) as u64).max(1);
    let (mut seen, mut threshold) = (0, 0);
    for (index, count) in bins.iter().enumerate().rev() {
        seen += count;
        if seen >= wanted {
            threshold = index;
            break;
        }
    }
    let chosen: Vec<&[f64; 3]> = clipped.iter().filter(|c| bin(c) >= threshold).collect();
    std::array::from_fn(|k| chosen.iter().map(|c| c[k]).sum::<f64>() / chosen.len() as f64)
}

/// Rule D at one flat-region pixel: `(the output code per component, t)`.
fn dehazed(p: [f32; 4], a: [f64; 3], amount: f64) -> ([f64; 3], f64) {
    let code = encoded(p);
    if amount < 0.0 {
        let mix = 0.6 * -amount / 100.0;
        return (
            std::array::from_fn(|c| code[c] + (a[c] - code[c]) * mix),
            1.0,
        );
    }
    let w = (0..3)
        .map(|c| (code[c].clamp(0.0, 1.0) / a[c].max(0.05)).min(1.0))
        .fold(1.0_f64, f64::min)
        .max(0.0);
    let t = (1.0 - 0.95 * amount / 100.0 * w).max(0.1);
    let out = std::array::from_fn(|c| {
        if code[c] > 1.0 {
            code[c]
        } else {
            ((code[c] - a[c]) / t + a[c]).clamp(code[c].min(0.0), 1.0)
        }
    });
    (out, t)
}

#[test]
fn dehaze_holds_above_white_and_follows_rule_d_and_its_16_bit_twin() {
    let picture = banded(&HAZE_BANDS);
    let doc = solid_doc(&picture);
    let a = airlight(&picture.pixels);
    // The 2.0 and 4.0 bands clip to white, so the airlight is white's, as
    // the 16-bit twin of the clipped copy measures it.
    assert!(a.iter().all(|v| (v - 1.0).abs() < 1e-12), "{a:?}");
    let clipped: Vec<[u16; 4]> = picture
        .pixels
        .iter()
        .map(|p| {
            let c = |k: usize| (encode(f64::from(p[k]).clamp(0.0, 1.0)) * 65535.0).round() as u16;
            [c(0), c(1), c(2), 65535]
        })
        .collect();
    for amount in [50.0_f64, 100.0, -50.0] {
        let params = json!({ "dehaze": amount });
        let out = filtered(&doc, "presence", &params);
        let twin = wide(DW, DH, &clipped, "presence", &params);
        let mut compared = 0;
        for (i, (got, was)) in out.iter().zip(&picture.pixels).enumerate() {
            assert!(got.iter().all(|v| v.is_finite()), "{amount} {i}");
            for c in 0..3 {
                let bound = f64::from(was[c]).max(1.0) * (1.0 + 1e-6);
                assert!(
                    f64::from(got[c]) <= bound,
                    "{amount} pixel {i} channel {c}: {} from {}",
                    got[c],
                    was[c]
                );
                if amount > 0.0 && was[c] > 1.0 {
                    assert_eq!(got[c].to_bits(), was[c].to_bits(), "{amount} {i} {c}");
                }
            }
            if !probe(i, HAZE_BANDS.len(), 6) {
                continue;
            }
            let (want, t) = dehazed(*was, a, amount);
            let tolerance = TWIN * (1.0 + 1.0 / t);
            for c in 0..3 {
                if amount > 0.0 && was[c] > 1.0 {
                    continue;
                }
                let e = encode(f64::from(got[c]));
                assert!(
                    (e - want[c]).abs() <= tolerance,
                    "{amount} pixel {i} channel {c}: E(out) {e}, rule {}",
                    want[c]
                );
                let c16 = twin[i][c];
                if was[c] <= 1.0 && c16 != 0 && c16 != 65535 {
                    let w16 = f64::from(c16) / 65535.0;
                    assert!(
                        (e - w16).abs() <= tolerance,
                        "{amount} pixel {i} channel {c}: E(out) {e}, 16-bit {w16}"
                    );
                    compared += 1;
                }
            }
        }
        assert!(compared > 1000, "{amount}: only {compared} compared");
    }
}

// ---- H: Highlights and Shadows ----------------------------------------------

const TONE_BANDS: [[f32; 3]; 4] = [
    [2.0, 2.0, 2.0],
    [0.5, 1.5, 0.2],
    [0.18, 0.18, 0.18],
    [1.5, 0.3, 0.3],
];

/// The Rec. 709 luma of an encoded triple.
fn luma(e: [f64; 3]) -> f64 {
    0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2]
}

/// Rule H at a flat-region pixel, restated in f64: `None` where the encoded
/// luma is at or above 1 (held), else the output codes of `apply` with no
/// clamp, the Develop Filter's mapping (tone 0.5, no Color, no midtone
/// contrast; the negative side at half).
fn toned(p: [f32; 4], highlights: f64, shadows: f64) -> Option<[f64; 3]> {
    let code = encoded(p);
    let l = luma(code);
    if l >= 1.0 {
        return None;
    }
    let s = shadows / 100.0;
    let h = -highlights / 100.0;
    let a_s = if s < 0.0 { s / 2.0 } else { s };
    let a_h = if h < 0.0 { h / 2.0 } else { h };
    let b = l.clamp(0.0, 1.0);
    let w_s = (1.0 - b / 0.5).clamp(0.0, 1.0).powi(2);
    let w_h = (1.0 - (1.0 - b) / 0.5).clamp(0.0, 1.0).powi(2);
    let e = (1.0 + a_h * w_h) / (1.0 + a_s * w_s);
    let k = l.powf(e) / l;
    Some(code.map(|c| c * k))
}

#[test]
fn highlights_and_shadows_hold_white_and_follow_rule_h() {
    let picture = banded(&TONE_BANDS);
    let doc = solid_doc(&picture);
    for (highlights, shadows) in [(-60.0_f64, 60.0_f64), (50.0, -40.0)] {
        let params = json!({"highlights": highlights, "shadows": shadows});
        let out = filtered(&doc, "develop_filter", &params);
        let mut moved = false;
        for (i, (got, was)) in out.iter().zip(&picture.pixels).enumerate() {
            assert!(got.iter().all(|v| v.is_finite()), "{params} {i}");
            if !probe(i, TONE_BANDS.len(), 8) {
                continue;
            }
            match toned(*was, highlights, shadows) {
                None => assert_eq!(got.map(f32::to_bits), was.map(f32::to_bits), "{params} {i}"),
                Some(want) => {
                    for c in 0..3 {
                        let e = encode(f64::from(got[c]));
                        assert!(
                            (e - want[c]).abs() <= 2.0 * TWIN,
                            "{params} pixel {i} channel {c}: E(out) {e}, rule {}",
                            want[c]
                        );
                    }
                    moved |= got[0].to_bits() != was[0].to_bits();
                }
            }
        }
        assert!(moved, "{params}: nothing moved");
    }
}

// ---- V: Vibrance and Saturation ---------------------------------------------

/// `float_display_adjustments.rs`' patches: 1.0, 2.0, 8.0, 0.999, −0.25 and
/// 0.18 on each channel at least once.
const PATCHES: [[f32; 3]; 8] = [
    [1.0, 1.0, 1.0],
    [2.0, 0.5, 0.25],
    [8.0, 1.0, -0.25],
    [0.999, 0.18, 0.5],
    [0.18, -0.25, 1.0],
    [-0.25, 2.0, 0.18],
    [0.5, 8.0, 0.999],
    [0.18, 0.18, 0.18],
];

#[test]
fn vibrance_and_saturation_take_the_scaled_display_form() {
    let n = PATCHES.len() as u32;
    let picture = Picture {
        w: n,
        h: 1,
        pixels: PATCHES.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
    };
    let doc = solid_doc(&picture);
    for (vibrance, saturation) in [(40.0_f64, 0.0_f64), (0.0, -35.0), (-30.0, 25.0)] {
        let params = json!({"vibrance": vibrance, "saturation": saturation});
        let out = filtered(&doc, "develop_filter", &params);
        let parts: Vec<(f64, [f64; 3])> = PATCHES
            .iter()
            .map(|p| {
                let m = p.iter().fold(1.0_f64, |m, v| m.max(f64::from(*v)));
                (m, p.map(|v| f64::from(v) / m))
            })
            .collect();
        let codes: Vec<[u16; 4]> = parts
            .iter()
            .map(|(_, c)| {
                let e = c.map(|v| (encode(v.max(0.0)) * 65535.0).round() as u16);
                [e[0], e[1], e[2], 65535]
            })
            .collect();
        let twin = wide(n, 1, &codes, "develop_filter", &params);
        // Image ▸ Adjustments ▸ Vibrance at float, as a layer over the same
        // patches.
        let adjusted = render(&add(
            &float_row(IccProfile::srgb().bytes(), &PATCHES),
            "vibrance",
            &json!({"vibrance": vibrance / 100.0, "saturation": saturation / 100.0}),
            1.0,
        ));
        for (i, ((m, c), q)) in parts.iter().zip(&twin).enumerate() {
            let got = out[i];
            assert!(got.iter().all(|v| v.is_finite()), "{params} {i}");
            assert_eq!(got[3], 1.0);
            for k in 0..3 {
                let want = m * (decode(f64::from(q[k]) / 65535.0) + c[k].min(0.0));
                assert!(
                    (f64::from(got[k]) - want).abs() <= 5e-4 * want.abs().max(1.0),
                    "{params} patch {i} channel {k}: {} vs twin {want}",
                    got[k]
                );
                let same = f64::from(adjusted[i][k]);
                assert!(
                    (f64::from(got[k]) - same).abs() <= 1e-6 * same.abs().max(1.0),
                    "{params} patch {i} channel {k}: {} vs the adjustment {same}",
                    got[k]
                );
            }
        }
    }
}

// ---- the operator table -----------------------------------------------------

#[test]
fn presence_and_the_develop_filter_filter_have_no_refused_row() {
    let (doc, _) = document(&hdr_picture(false));
    for (op, params) in [
        ("presence", json!({"dehaze": 20})),
        ("presence", json!({"texture": 10, "dehaze": -5})),
        (
            "develop_filter",
            json!({"exposure": 1, "highlights": 20, "shadows": -20, "dehaze": 10,
                "vibrance": 10, "saturation": -10}),
        ),
    ] {
        assert!(plan(&doc, op, &params).is_ok(), "{op} {params} plan");
        let out = filter(&doc, op, &params, &[]);
        assert!(matches!(out, Ok(Some(_))), "{op} {params}: {:?}", out.err());
    }
    let table = unsafe { dr_float_operators() };
    let text = unsafe { CStr::from_ptr(table) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(table) };
    let table: Value = serde_json::from_str(&text).unwrap();
    let refused: Vec<&str> = table["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["operator"].as_str().unwrap())
        .collect();
    for gone in ["Dehaze", "Develop Filter", "edge stage", "the Sponge"] {
        assert!(!refused.iter().any(|row| row.contains(gone)), "{gone}");
    }
    assert!(refused.contains(&"Select and Mask's Decontaminate Colors"));
    assert!(!refused.iter().any(|row| row.contains("display-encoded")));
    let admitted = table["admitted"].as_array().unwrap();
    assert!(admitted.contains(&json!("red_eye")));
}

// ---- Red Eye --------------------------------------------------------------

const RW: u32 = 24;
const RH: u32 = 28;

/// A #8c8c8c field with a #d01818 disc of radius 3 at (6, 16).
fn eye_codes() -> Vec<[u16; 4]> {
    (0..RW * RH)
        .map(|i| {
            let (x, y) = (i64::from(i % RW), i64::from(i / RW));
            if (x - 6).pow(2) + (y - 16).pow(2) <= 9 {
                [0xd0 * 257, 0x18 * 257, 0x18 * 257, 65535]
            } else {
                [0x8c * 257, 0x8c * 257, 0x8c * 257, 65535]
            }
        })
        .collect()
}

fn red_eye() -> RetouchEdit {
    RetouchEdit::parse(
        &json!({"op": "red_eye", "layer": 0, "x": 1, "y": 11, "width": 11, "height": 11,
            "pupil_size": 1.0, "darken": 0.5})
        .to_string(),
    )
    .unwrap()
}

fn inside(i: usize) -> bool {
    let (x, y) = (i as u32 % RW, i as u32 / RW);
    (1..12).contains(&x) && (11..22).contains(&y)
}

#[test]
fn red_eye_matches_its_16_bit_twin_and_keeps_everything_else() {
    let codes = eye_codes();
    let coverage = vec![u16::MAX; (RW * RH) as usize];
    let wide = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(RW, RH, codes.clone(), IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
    .retouch(&red_eye(), &coverage, LIMIT, 0, None)
    .unwrap()
    .expect("the 16-bit pupil is corrected")
    .layer_source(0)
    .unwrap()
    .to_rgba16()
    .unwrap();
    // The float twin, with an above-white catchlight inside the rect.
    let mut pixels = samples(&float_of(RW, RH, &codes));
    let catchlight = (12 * RW + 2) as usize;
    pixels[catchlight] = [6.0, 6.0, 6.0, 1.0];
    let doc = solid_doc(&Picture {
        w: RW,
        h: RH,
        pixels: pixels.clone(),
    });
    let plan_ok = doc.retouch_plan(&red_eye());
    assert!(plan_ok.is_ok(), "{:?}", plan_ok.err());
    let out = doc
        .retouch(&red_eye(), &coverage, LIMIT, 0, None)
        .unwrap()
        .expect("the float pupil is corrected");
    let got = samples(&out);
    let mut corrected = 0;
    for (i, (g, was)) in got.iter().zip(&pixels).enumerate() {
        assert!(g.iter().all(|v| v.is_finite()), "{i}");
        if !inside(i) || i == catchlight {
            assert_eq!(g.map(f32::to_bits), was.map(f32::to_bits), "pixel {i}");
            continue;
        }
        assert_eq!(g[3], 1.0);
        for c in 0..3 {
            let c16 = wide[i][c];
            if c16 == 0 || c16 == 65535 {
                continue;
            }
            let (e, want) = (encode(f64::from(g[c])), f64::from(c16) / 65535.0);
            assert!(
                (e - want).abs() <= TWIN,
                "pixel {i} channel {c}: E(out) {e}, 16-bit {want}"
            );
        }
        if g[0] < was[0] {
            corrected += 1;
        }
    }
    assert!(corrected >= 20, "{corrected} pupil pixels corrected");
    // An all-red field has no pupil: nothing changes, as at 16 bits.
    let field = solid_doc(&Picture {
        w: RW,
        h: RH,
        pixels: vec![[0.6, 0.01, 0.01, 1.0]; (RW * RH) as usize],
    });
    assert!(field
        .retouch(&red_eye(), &coverage, LIMIT, 0, None)
        .unwrap()
        .is_none());
}
