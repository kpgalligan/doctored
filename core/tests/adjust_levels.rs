//! The `levels` adjustment's output range and per-channel
//! stages (`core/src/adjust_levels.rs`), black-box through
//! `dr_image_adjust_op`, the compositor and the typed `PrecisionAdjustment`.
//!
//! The oracles are written out here in f64 from the formula — `u =
//! clamp((v - black) / (white - black), 0, 1)`, `t = u ^ (1 / gamma)` except
//! below `x0 = 2·2^(gamma/(p − 1))` (p = 1/gamma) at gamma > 1, where #578's
//! toe (H-CB, Color Balance's Hermite) runs the cubic Hermite from (0, 0) at
//! slope `2^gamma` to `(x0, x0^p)` at slope `p·x0^(p − 1)`, then
//! `out = output_black + (output_white - output_black) * t`, the channel's
//! stage first and the master's second — or are the independent per-channel
//! export `dr_image_levels_channels`, which the Auto commands run, or
//! `color_balance`'s own curve at the same gamma. Photoshop's own take1
//! samples are pinned too. The last test is the destructive-levels case set
//! that used to live in `filter_tests` against the deleted `dr_image_levels`,
//! now through the ONE twin.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::DrDocument;
use doctored_core::ffi_adjust::{dr_image_adjust_op, dr_image_levels_channels};
use doctored_core::icc::IccProfile;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::*;

/// `dr_image_adjust_op("levels", params)`, or the core's refusal.
fn levels(img: *const DrImage, params: &Value) -> Result<*mut DrImage, String> {
    adjust_op(img, "levels", params)
}

/// `dr_image_adjust_op(op, params)`, or the core's refusal.
fn adjust_op(img: *const DrImage, op: &str, params: &Value) -> Result<*mut DrImage, String> {
    let op = CString::new(op).unwrap();
    let params = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(img, op.as_ptr(), params.as_ptr(), &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        assert!(err.is_null(), "success must not set err_out");
        Ok(out)
    }
}

fn levels_channels(img: *const DrImage, b: [f32; 3], w: [f32; 3], g: [f32; 3]) -> *mut DrImage {
    let out = unsafe { dr_image_levels_channels(img, b.as_ptr(), w.as_ptr(), g.as_ptr()) };
    assert!(!out.is_null(), "levels_channels refused");
    out
}

/// One stage in f64: (black, white, gamma, output_black, output_white).
type Stage = (f64, f64, f64, f64, f64);

const IDENTITY: Stage = (0.0, 1.0, 1.0, 0.0, 1.0);

/// The H-CB curve at `u` in [0, 1] (module doc): `u^p`, except below x0 at
/// gamma > 1, where the cubic Hermite on the standard basis over [0, x0]
/// joins (0, 0) at slope `2^gamma` to `(x0, x0^p)` at the power's slope.
fn hermite(gamma: f64, u: f64) -> f64 {
    let p = 1.0 / gamma;
    if gamma <= 1.0 {
        return u.powf(p);
    }
    let x0 = 2.0 * 2f64.powf(gamma / (p - 1.0));
    if u >= x0 {
        return u.powf(p);
    }
    let (start_slope, end_value, end_slope) = (2f64.powf(gamma), x0.powf(p), p * x0.powf(p - 1.0));
    let s = u / x0;
    let h10 = s.powi(3) - 2.0 * s.powi(2) + s;
    let h01 = -2.0 * s.powi(3) + 3.0 * s.powi(2);
    let h11 = s.powi(3) - s.powi(2);
    h10 * x0 * start_slope + h01 * end_value + h11 * x0 * end_slope
}

fn stage(s: Stage, v: f64) -> f64 {
    let (black, white, gamma, ob, ow) = s;
    let u = ((v - black) / (white - black)).clamp(0.0, 1.0);
    ob + (ow - ob) * hermite(gamma, u)
}

/// The documented order: channel first, then master.
fn oracle(master: Stage, channels: [Stage; 3], rgb: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|c| stage(master, stage(channels[c], rgb[c])))
}

/// The OTHER order, which A2 measured Photoshop does not use.
fn reversed(master: Stage, channels: [Stage; 3], rgb: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|c| stage(channels[c], stage(master, rgb[c])))
}

/// How near (in codes) a value a rounding sees must be to a .5 tie for the
/// R8 test to allow the core one code either way (its curve is f32, about
/// 3e-5 code); declared before measuring, never widened.
const TIE: f64 = 1e-3;

/// #578 phase 4's 8-bit member R8(h,h,x), from the runner's docstring
/// (`scripts/photoshop-probe/eight_bit_578_take.py`): one stage on the code
/// `c = 255·v` (and its integer input and output codes), each snapped to an
/// integer within 1e-3; `s = clamp((c − in_lo)·255/(in_hi − in_lo), 0, 255)`
/// rounded half up, the H-CB curve at that code rounded half up, then the
/// output line. `tie` records a rounding that saw a value within [`TIE`] of
/// a .5 tie.
fn r8_stage(s: Stage, v: f64, tie: &mut bool) -> f64 {
    let snap = |c: f64| {
        if (c - c.round()).abs() <= 1e-3 {
            c.round()
        } else {
            c
        }
    };
    let mut half_up = |x: f64| {
        *tie |= (x - x.floor() - 0.5).abs() < TIE;
        (x + 0.5).floor()
    };
    let (black, white, gamma, ob, ow) = s;
    let (c, in_lo, in_hi) = (snap(255.0 * v), snap(255.0 * black), snap(255.0 * white));
    let (lo, hi) = (snap(255.0 * ob), snap(255.0 * ow));
    let code = half_up(((c - in_lo) * 255.0 / (in_hi - in_lo)).clamp(0.0, 255.0));
    let f = half_up(255.0 * hermite(gamma, code / 255.0)) / 255.0;
    lo / 255.0 + (hi - lo) / 255.0 * f
}

/// R8(h,h,x) on a triple, the channel's stage first (its output passed on
/// unrounded) and identity stages skipped: the final codes, and whether any
/// rounding on the way (the final one included) saw a near tie.
fn r8_codes(master: Stage, channels: [Stage; 3], rgb: [f64; 3]) -> [(u8, bool); 3] {
    std::array::from_fn(|c| {
        let mut tie = false;
        let mut v = rgb[c];
        if channels[c] != IDENTITY {
            v = r8_stage(channels[c], v, &mut tie);
        }
        if master != IDENTITY {
            v = r8_stage(master, v, &mut tie);
        }
        let x = 255.0 * v;
        tie |= (x - x.floor() - 0.5).abs() < TIE;
        ((x + 0.5).floor().clamp(0.0, 255.0) as u8, tie)
    })
}

fn stage_json(s: Stage) -> Value {
    json!({"black": s.0, "white": s.1, "gamma": s.2, "output_black": s.3, "output_white": s.4})
}

/// The worst 8-bit difference between `out` and the f64 oracle over `src`;
/// alpha must be untouched.
fn worst_against(out: &[u8], src: &RgbaImage, f: impl Fn([f64; 3]) -> [f64; 3]) -> u32 {
    let mut worst = 0;
    for (i, px) in src.pixels().enumerate() {
        let rgb = [0, 1, 2].map(|c| f64::from(px[c]) / 255.0);
        let want = f(rgb);
        for c in 0..3 {
            let want = (want[c].clamp(0.0, 1.0) * 255.0).round() as i64;
            worst = worst.max((i64::from(out[i * 4 + c]) - want).unsigned_abs() as u32);
        }
        assert_eq!(out[i * 4 + 3], px[3], "alpha changed at {i}");
    }
    worst
}

#[test]
fn master_only_levels_equal_the_per_channel_export_with_repeated_triples() {
    let dir = tempfile::tempdir().unwrap();
    let src = test_pattern(48, 32);
    let img = open_pattern(&dir, "pattern.png", &src);
    let reference = levels_channels(img, [0.1; 3], [0.9; 3], [1.8; 3]);
    for params in [
        json!({"black": 0.1, "white": 0.9, "gamma": 1.8}),
        // The output keys at their defaults change nothing.
        json!({"black": 0.1, "white": 0.9, "gamma": 1.8,
               "output_black": 0.0, "output_white": 1.0}),
        // And so do identity channel objects.
        json!({"black": 0.1, "white": 0.9, "gamma": 1.8, "r": {}, "g": stage_json(IDENTITY)}),
    ] {
        let out = levels(img, &params).expect("levels");
        assert_eq!(pixels(out), pixels(reference), "{params}");
        free(out);
    }
    free(reference);
    free(img);
}

#[test]
fn per_channel_stages_alone_equal_the_per_channel_export() {
    let dir = tempfile::tempdir().unwrap();
    let src = test_pattern(48, 32);
    let img = open_pattern(&dir, "pattern.png", &src);
    let params = json!({
        "r": {"black": 0.2, "white": 0.8, "gamma": 1.0},
        "g": {"black": 0.0, "white": 0.6, "gamma": 0.5},
        "b": {"black": 0.3, "white": 1.0, "gamma": 2.5},
    });
    let out = levels(img, &params).expect("levels");
    let reference = levels_channels(img, [0.2, 0.0, 0.3], [0.8, 0.6, 1.0], [1.0, 0.5, 2.5]);
    assert_eq!(pixels(out), pixels(reference));
    free(out);
    free(reference);
    free(img);
}

#[test]
fn the_output_range_maps_and_an_inverted_one_inverts() {
    let dir = tempfile::tempdir().unwrap();
    let src = test_pattern(48, 32);
    let img = open_pattern(&dir, "pattern.png", &src);
    for master in [
        (0.0, 1.0, 1.0, 20.0 / 255.0, 235.0 / 255.0),
        (0.1, 0.9, 1.4, 0.25, 0.75),
        // Inverted: output black above output white.
        (0.0, 1.0, 1.0, 1.0, 0.0),
        (0.05, 0.95, 0.7, 0.9, 0.2),
    ] {
        let out = levels(img, &stage_json(master)).expect("levels");
        let got = pixels(out);
        let worst = worst_against(&got, &src, |rgb| oracle(master, [IDENTITY; 3], rgb));
        assert!(worst <= 1, "{master:?}: worst {worst}");
        free(out);
    }
    // A pure inversion through the output range is exactly 255 - v.
    let out = levels(img, &json!({"output_black": 1.0, "output_white": 0.0})).expect("invert");
    for (got, want) in pixels(out).chunks_exact(4).zip(src.pixels()) {
        for c in 0..3 {
            assert_eq!(got[c], 255 - want[c]);
        }
    }
    free(out);
    free(img);
}

/// `levl_both`'s Photoshop record codes: the master is record 0,
/// green record 2. The kernel follows the channel-first oracle; the other
/// order is a visibly different picture.
#[test]
fn the_channel_stage_applies_before_the_master() {
    let dir = tempfile::tempdir().unwrap();
    let src = test_pattern(64, 48);
    let img = open_pattern(&dir, "pattern.png", &src);
    let master = (30.0 / 255.0, 220.0 / 255.0, 1.3, 0.0, 1.0);
    let green = (40.0 / 255.0, 1.0, 0.7, 0.0, 1.0);
    let channels = [IDENTITY, green, IDENTITY];
    let params = json!({"black": master.0, "white": master.1, "gamma": master.2,
                        "g": {"black": green.0, "white": green.1, "gamma": green.2}});
    let out = levels(img, &params).expect("levels");
    let got = pixels(out);
    let worst = worst_against(&got, &src, |rgb| oracle(master, channels, rgb));
    assert!(worst <= 1, "channel-first: worst {worst}");
    let other = worst_against(&got, &src, |rgb| reversed(master, channels, rgb));
    assert!(other > 10, "the reverse order must differ visibly: {other}");
    free(out);
    free(img);
}

/// The 16-bit path: the typed description parses with the new keys and its
/// `apply_at` follows the same f64 oracle on continuous values.
#[test]
fn the_typed_adjustment_parses_and_follows_the_oracle() {
    let master = (0.05, 0.9, 1.2, 0.1, 0.95);
    let channels = [
        (0.1, 1.0, 0.8, 0.0, 1.0),
        IDENTITY,
        (0.0, 0.7, 1.0, 0.2, 0.9),
    ];
    let description = json!({"type": "adjust", "evaluation": "precision_v1", "op": "levels",
        "params": {"black": master.0, "white": master.1, "gamma": master.2,
                   "output_black": master.3, "output_white": master.4,
                   "r": stage_json(channels[0]), "b": stage_json(channels[2])}})
    .to_string();
    let typed = PrecisionAdjustment::parse(&description, &IccProfile::srgb()).expect("parse");
    assert!(!typed.is_identity());
    for i in 0..=64 {
        let v = f64::from(i) / 64.0;
        let rgb = [v, 1.0 - v, (v * 3.0) % 1.0];
        let got = typed.apply_at(rgb.map(|x| x as f32), 0.0, (0, 0));
        let want = oracle(master, channels, rgb);
        for c in 0..3 {
            assert!(
                (f64::from(got[c]) - want[c]).abs() < 1e-5,
                "{rgb:?} channel {c}: {} vs {}",
                got[c],
                want[c]
            );
        }
    }
    // Every stage at its defaults is the identity at this depth too.
    let identity = json!({"type": "adjust", "evaluation": "precision_v1", "op": "levels",
        "params": {"r": {}, "output_black": 0.0}})
    .to_string();
    assert!(PrecisionAdjustment::parse(&identity, &IccProfile::srgb())
        .expect("parse")
        .is_identity());
}

#[test]
fn malformed_levels_refuse_and_unknown_typed_keys_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let img = open_pattern(&dir, "p.png", &test_pattern(8, 8));
    for params in [
        json!({"r": 3}),
        json!({"g": [0.1, 0.9]}),
        json!({"b": {"black": 0.6, "white": 0.4}}),
        json!({"r": {"gamma": 11.0}}),
        json!({"output_black": 1.5}),
        json!({"output_white": -0.1}),
        json!({"g": {"output_white": 2.0}}),
        json!({"black": 0.5, "white": 0.5}),
        json!({"output_black": "0.2"}),
    ] {
        assert!(levels(img, &params).is_err(), "{params} must refuse");
    }
    // The byte schema ignores unknown keys, inside a channel too.
    let out = levels(img, &json!({"r": {"black": 0.1, "note": 1}, "extra": true}))
        .expect("unknown keys are ignored");
    free(out);
    free(img);

    let typed = |params: Value| {
        let text = json!({"type": "adjust", "evaluation": "precision_v1", "op": "levels",
                          "params": params})
        .to_string();
        PrecisionAdjustment::parse(&text, &IccProfile::srgb()).map(|_| ())
    };
    assert!(typed(json!({"black": 0.1, "output_white": 0.9, "g": {"gamma": 2}})).is_ok());
    let top = typed(json!({"extra": 1})).unwrap_err();
    assert!(
        top.contains("unknown high-depth adjustment parameter extra"),
        "{top}"
    );
    let nested = typed(json!({"r": {"black": 0.1, "note": 1}})).unwrap_err();
    assert!(
        nested.contains("unknown high-depth adjustment parameter note"),
        "{nested}"
    );
    let shape = typed(json!({"b": 0.5})).unwrap_err();
    assert!(shape.contains("must be an object"), "{shape}");
    assert!(typed(json!({"r": {"black": 0.9, "white": 0.1}})).is_err());
}

/// The layer and the filter run one kernel: a per-channel, output-ranged
/// levels LAYER over an image flattens to exactly the destructive op's bytes.
#[test]
fn a_per_channel_levels_layer_matches_the_destructive_op() {
    let dir = tempfile::tempdir().unwrap();
    let src = opaque_pattern(24, 16);
    let img = open_pattern(&dir, "pattern.png", &src);
    let params = json!({"black": 0.05, "white": 0.95, "gamma": 1.1,
        "output_black": 0.1, "output_white": 0.9,
        "r": {"black": 0.2, "white": 1.0, "gamma": 0.8},
        "b": {"black": 0.0, "white": 0.8, "gamma": 1.0, "output_black": 1.0, "output_white": 0.0}});
    let out = levels(img, &params).expect("levels");
    let mut doc = DrDocument::from_pixels(src.clone());
    doc = doc
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(24, 16, Rgba([255, 0, 255, 255])),
            "Levels",
        )
        .expect("add layer");
    doc.layers[1].meta =
        Some(json!({"type": "adjust", "op": "levels", "params": params}).to_string());
    assert_eq!(doc.flattened().expect("flatten").as_raw(), &pixels(out));
    free(out);
    free(img);
}

/// The destructive Levels cases `filter_tests` pinned on `dr_image_levels`,
/// now through `dr_image_adjust_op` (NaN arguments cannot be spelled in
/// JSON, so those three rows have no twin here).
#[test]
fn levels_identity_black_point_gamma_and_guards() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let img = open_pattern(&dir, "pattern.png", &pattern);
    let src = pattern.as_raw();

    // (0, 1, 1) is the exact identity.
    let same = levels(img, &json!({"black": 0.0, "white": 1.0, "gamma": 1.0})).unwrap();
    assert_eq!(pixels(same), *src, "levels(0,1,1) must be exact identity");
    free(same);

    // black = 0.5 crushes every channel value <= 127 (c/255 <= 0.5) to 0.
    let dark = levels(img, &json!({"black": 0.5, "white": 1.0, "gamma": 1.0})).unwrap();
    for (i, (&got, &want)) in pixels(dark).iter().zip(src.iter()).enumerate() {
        if i % 4 == 3 {
            assert_eq!(got, want, "alpha changed at byte {i}");
        } else if want <= 127 {
            assert_eq!(got, 0, "byte {i}: input {want} not crushed to 0");
        }
    }
    free(dark);

    // gamma branch: input 0.25 with gamma 2 -> sqrt(0.25) = 0.5 (+-1).
    let gray = RgbaImage::from_pixel(4, 4, Rgba([64, 64, 64, 255]));
    let gimg = open_pattern(&dir, "gray.png", &gray);
    let out = levels(gimg, &json!({"black": 0.0, "white": 1.0, "gamma": 2.0})).unwrap();
    let [r, g, b, a] = pixel_at(out, 2, 2);
    for v in [r, g, b] {
        assert!(
            (127..=129).contains(&v),
            "gamma 2 on 64 gave {v}, want ~128"
        );
    }
    assert_eq!(a, 255);
    free(out);
    free(gimg);

    // Invalid arguments refuse.
    for (black, white, gamma) in [
        (0.5, 0.5, 1.0),  // black == white
        (0.7, 0.5, 1.0),  // black > white
        (-0.1, 1.0, 1.0), // black below range
        (0.0, 1.1, 1.0),  // white above range
        (0.0, 1.0, 0.0),  // gamma zero
        (0.0, 1.0, 0.05), // gamma below range
        (0.0, 1.0, 10.5), // gamma above range
    ] {
        assert!(
            levels(
                img,
                &json!({"black": black, "white": white, "gamma": gamma})
            )
            .is_err(),
            "levels({black},{white},{gamma}) should refuse"
        );
    }
    free(img);
}

/// #578: the toe against Photoshop 26.10.0's own take1 samples
/// (`psd-appearance-578/take1`, `lev_srgb8`, sets M1–M4 and M6): the input
/// triple and Photoshop's output, with the H-CB oracle's output computed here
/// from each set's stages. Doctored lands within the oracle's own
/// residual plus 1 code (f32 against f64), where the pure power (L0) was up
/// to 41 codes off.
#[test]
fn the_toe_follows_photoshops_take1_samples() {
    type Sample = ([u8; 3], [u8; 3]);
    let set = |black: f64, white: f64, gamma: f64, ob: f64, ow: f64| -> Stage {
        (black / 255.0, white / 255.0, gamma, ob / 255.0, ow / 255.0)
    };
    let sets: [(Value, Stage, [Stage; 3], [Sample; 2]); 5] = [
        (
            json!({"gamma": 1.35}),
            set(0.0, 255.0, 1.35, 0.0, 255.0),
            [IDENTITY; 3],
            [
                ([2, 87, 172], [5, 115, 190]),
                ([6, 91, 176], [14, 119, 194]),
            ],
        ),
        (
            json!({"black": 20.0 / 255.0, "white": 235.0 / 255.0, "gamma": 2.2}),
            set(20.0, 235.0, 2.2, 0.0, 255.0),
            [IDENTITY; 3],
            [
                ([22, 107, 192], [9, 169, 230]),
                ([24, 109, 194], [22, 171, 231]),
            ],
        ),
        (
            json!({"gamma": 3.0}),
            set(0.0, 255.0, 3.0, 0.0, 255.0),
            [IDENTITY; 3],
            [
                ([2, 87, 172], [16, 178, 224]),
                ([3, 88, 173], [23, 179, 224]),
            ],
        ),
        (
            json!({"black": 40.0 / 255.0, "white": 200.0 / 255.0, "gamma": 3.5,
                   "output_black": 10.0 / 255.0, "output_white": 245.0 / 255.0}),
            set(40.0, 200.0, 3.5, 10.0, 245.0),
            [IDENTITY; 3],
            [
                ([42, 127, 212], [39, 207, 245]),
                ([41, 126, 211], [30, 207, 245]),
            ],
        ),
        (
            json!({"g": {"black": 15.0 / 255.0, "white": 240.0 / 255.0, "gamma": 2.8}}),
            IDENTITY,
            [IDENTITY, set(15.0, 240.0, 2.8, 0.0, 255.0), IDENTITY],
            [
                ([189, 18, 103], [189, 20, 103]),
                ([190, 19, 104], [190, 33, 104]),
            ],
        ),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (n, (params, master, channels, samples)) in sets.iter().enumerate() {
        let src = RgbaImage::from_fn(2, 1, |x, _| {
            let rgb = samples[x as usize].0;
            Rgba([rgb[0], rgb[1], rgb[2], 255])
        });
        let img = open_pattern(&dir, &format!("toe{n}.png"), &src);
        let out = levels(img, params).unwrap();
        for (x, (input, photoshop)) in samples.iter().enumerate() {
            let got = pixel_at(out, x as u32, 0);
            let want = oracle(*master, *channels, input.map(|v| f64::from(v) / 255.0));
            for c in 0..3 {
                let model = (want[c].clamp(0.0, 1.0) * 255.0).round() as u8;
                let tolerance = u32::from(photoshop[c].abs_diff(model)) + 1;
                assert!(
                    u32::from(got[c].abs_diff(photoshop[c])) <= tolerance,
                    "set {n} {params} input {input:?}: channel {c} {} vs Photoshop {} \
                     (tolerance {tolerance})",
                    got[c],
                    photoshop[c]
                );
            }
        }
        free(out);
        free(img);
    }
}

/// #578: Levels' toe IS Color Balance's. A red-only levels stage at gamma G
/// and a `color_balance` midtones cyan-red of M/100 (Preserve Luminosity
/// off, so red's gamma is `r2(2^(M/100))` = G, and its black and white points
/// stay at the rails) agree within a code on every 8-bit code, and neither
/// touches green or blue. At G = 1.41 and 2.0 the toe moves shadow codes off
/// the pure power (codes 1–14 and 1–27); at 1.19 it spans only codes 0–2 and
/// moves none, so that pair asserts the agreement alone.
#[test]
fn levels_toe_is_color_balances() {
    let dir = tempfile::tempdir().unwrap();
    let src = RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, x as u8, x as u8, 255]));
    let img = open_pattern(&dir, "ramp.png", &src);
    for (m, gamma, moves) in [(25, 1.19, false), (50, 1.41, true), (100, 2.0, true)] {
        let lev = levels(img, &json!({"r": {"gamma": gamma}})).expect("levels");
        let cb = adjust_op(
            img,
            "color_balance",
            &json!({"midtones": {"cyan_red": f64::from(m) / 100.0},
                    "preserve_luminosity": false}),
        )
        .expect("color_balance");
        let (lev_px, cb_px) = (pixels(lev), pixels(cb));
        let mut moved = 0;
        for x in 0..256usize {
            let (l, c) = (&lev_px[x * 4..x * 4 + 4], &cb_px[x * 4..x * 4 + 4]);
            assert!(
                l[0].abs_diff(c[0]) <= 1,
                "gamma {gamma} code {x}: levels {} vs color balance {}",
                l[0],
                c[0]
            );
            for ch in [1, 2] {
                assert_eq!(l[ch], x as u8, "levels touched channel {ch} at {x}");
                assert_eq!(c[ch], x as u8, "color balance touched channel {ch} at {x}");
            }
            let power = (255.0 * (x as f64 / 255.0).powf(1.0 / gamma)).round() as u8;
            if l[0] != power {
                moved += 1;
            }
        }
        if moves {
            assert!(
                moved > 0,
                "gamma {gamma}: the toe moved no code off the pure power"
            );
        }
        free(lev);
        free(cb);
    }
    free(img);
}

/// #578 phase 4: a byte document's Levels round twice (R8(h,h,x)). On a
/// 256-code ramp, for L4 (30–220, gamma 4), L6 (10–245, gamma 2, output
/// 20–235) and N8 (master 10–250 gamma 1.9 over red 0–240 gamma 3.1), every
/// code equals the f64 R8 oracle, but where a rounding saw a near tie (one
/// code either way); at least one code per set differs from H-CB on its own;
/// and the typed 16-bit stage of L4 still follows the continuous H-CB oracle.
#[test]
fn eight_bit_levels_follow_r8() {
    let code = |k: f64| k / 255.0;
    let sets: [(Value, Stage, [Stage; 3]); 3] = [
        (
            json!({"black": code(30.0), "white": code(220.0), "gamma": 4.0}),
            (code(30.0), code(220.0), 4.0, 0.0, 1.0),
            [IDENTITY; 3],
        ),
        (
            json!({"black": code(10.0), "white": code(245.0), "gamma": 2.0,
                   "output_black": code(20.0), "output_white": code(235.0)}),
            (code(10.0), code(245.0), 2.0, code(20.0), code(235.0)),
            [IDENTITY; 3],
        ),
        (
            json!({"black": code(10.0), "white": code(250.0), "gamma": 1.9,
                   "r": {"black": 0.0, "white": code(240.0), "gamma": 3.1}}),
            (code(10.0), code(250.0), 1.9, 0.0, 1.0),
            [(0.0, code(240.0), 3.1, 0.0, 1.0), IDENTITY, IDENTITY],
        ),
    ];
    let dir = tempfile::tempdir().unwrap();
    let src = RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, x as u8, x as u8, 255]));
    let img = open_pattern(&dir, "r8-ramp.png", &src);
    for (n, (params, master, channels)) in sets.iter().enumerate() {
        let out = levels(img, params).expect("levels");
        let got = pixels(out);
        let mut off_hcb = 0;
        for x in 0..256usize {
            let rgb = [f64::from(x as u8) / 255.0; 3];
            let r8 = r8_codes(*master, *channels, rgb);
            let hcb = oracle(*master, *channels, rgb);
            for c in 0..3 {
                let (want, tie) = r8[c];
                let have = got[x * 4 + c];
                assert!(
                    have == want || (tie && have.abs_diff(want) == 1),
                    "set {n} code {x} channel {c}: {have} vs R8 {want} (near tie {tie})"
                );
                if have != (hcb[c].clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8 {
                    off_hcb += 1;
                }
            }
        }
        assert!(off_hcb > 0, "set {n}: R8 moved no code off H-CB");
        free(out);
    }
    // Input points 1e-6 apart snap to one code; the 8-bit form keeps them
    // unsnapped, so the ramp is a clean step from the output black, with no
    // NaN (which would write 0) at the shared code.
    let narrow = json!({"black": code(100.0), "white": code(100.0) + 1e-6, "gamma": 1.0,
                        "output_black": code(20.0)});
    let out = levels(img, &narrow).expect("narrow levels");
    let got = pixels(out);
    for x in 0..256usize {
        let have = got[x * 4];
        match x {
            0..=99 => assert_eq!(have, 20, "narrow span code {x}"),
            100 => assert!(have >= 20, "narrow span code 100: {have}"),
            _ => assert_eq!(have, 255, "narrow span code {x}"),
        }
    }
    free(out);
    free(img);
    let (master, _, _) = sets[0].clone();
    let description = json!({"type": "adjust", "evaluation": "precision_v1", "op": "levels",
                             "params": master})
    .to_string();
    let typed = PrecisionAdjustment::parse(&description, &IccProfile::srgb()).expect("parse");
    for x in 0..=255 {
        let v = f64::from(x) / 255.0;
        let got = typed.apply_at([v as f32; 3], 0.0, (0, 0));
        let want = oracle(sets[0].1, [IDENTITY; 3], [v; 3]);
        for c in 0..3 {
            assert!(
                (f64::from(got[c]) - want[c]).abs() < 1e-5,
                "typed L4 at {x}: {} vs H-CB {}",
                got[c],
                want[c]
            );
        }
    }
}
