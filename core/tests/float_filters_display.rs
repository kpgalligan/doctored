//! The eleven selected-edit filters with a
//! display-referred parameter, or none, on a 32-bit float layer: Surface
//! Blur, Dust & Scratches, Reduce Noise, Add Grain, Clouds, Difference
//! Clouds, Emboss, Edge Detect and Sharpen on the layer's display-encoded
//! copy; Add Noise and Pixelate on its linear samples. Presence, the Camera
//! Raw Filter and Red Eye are `float_filters_develop.rs`'s.
//!
//! Every oracle is written here: the document profile's transfer curve and
//! the logarithm above white restated, a 16-bit twin built from the same
//! codes, or a direct binary64 loop. The built-in sRGB profile's curve is the
//! IEC 61966-2.1 transfer sampled into Apple's 1024-entry u16 `curv` table
//! (`icc_builtin`'s module doc) and read by linear interpolation, so the
//! oracle restates that table: `tab[i] = round(srgb(i/1023)·65535)`. Its
//! slope at white, the logarithm's scale, is the last segment's. The bounds are declared before
//! the first run and never widened:
//!
//! - TWIN. A float layer built from 16-bit codes `c` (`v = EOTF(c/65535)`),
//!   the filter at full coverage: every component whose 16-bit result is not
//!   0 or 65535 has `|E(out) − c16/65535| ≤ 4/65535`.
//! - SURFACE. With headroom, against a direct tent on the encoded copy:
//!   `≤ (hi − lo)/255 + 2/65535` in encoded units.
//! - SHARPEN. Against `decode(2E − G_σ(E))` in binary64: `≤ 1e-4·max(1,
//!   |ref|)`; and Sharpen differs from `unsharp_mask {radius σ, amount 1,
//!   threshold 0}` on the same layer.
//! - PIXELATE. The premultiplied cell mean in binary64, `≤ 1e-6` relative.
//! - NOISE. On mid-grey against the 16-bit draw: `|out/v − 1 − δ16| ≤
//!   2/65535`; zero stays zero.
//! - ABOVE 1. Every filter but Clouds (which replaces the pixel) keeps a
//!   value above 1.0 above 1.0 and finite.
//! - RELIEF. On an SDR 0→1 step Emboss and Edge Detect lie in 0…1 linear at
//!   every pixel; on a layer holding 8.0 every encoded output channel lies in
//!   `[min(0, lo_c), max(1, hi_c)]` of the encoded copy.
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use serde_json::{json, Value};

use crate::float_filters::{
    bits, document, filter, filtered, gaussian_reference, plan, samples, solid_doc, Picture,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// TWIN's bound, in encoded units.
const TWIN: f64 = 4.0 / 65535.0;

// ---- the encoding, restated --------------------------------------------

/// The IEC 61966-2.1 electro-optical transfer function.
fn srgb(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// The built-in sRGB profile's 1024 u16 `curv` samples, over 65535.
fn table() -> &'static [f64] {
    static TABLE: std::sync::OnceLock<Vec<f64>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        (0..1024)
            .map(|i| (srgb(f64::from(i) / 1023.0) * 65535.0).round() / 65535.0)
            .collect()
    })
}

/// The profile's decode on 0…1: the table, linearly interpolated.
pub(crate) fn eotf(x: f64) -> f64 {
    let t = table();
    let pos = x.clamp(0.0, 1.0) * 1023.0;
    let i = (pos.floor() as usize).min(1022);
    let f = pos - i as f64;
    t[i] * (1.0 - f) + t[i + 1] * f
}

/// The inverse of [`eotf`] on 0…1: the segment holding `v`, inverted.
fn oetf(v: f64) -> f64 {
    let t = table();
    if v <= 0.0 {
        return 0.0;
    }
    let i = t[1..].partition_point(|x| *x < v).min(1022);
    (i as f64 + (v - t[i]) / (t[i + 1] - t[i])).min(1023.0) / 1023.0
}

/// The logarithm's scale above white: the curve's slope at 1, its last
/// segment's `Δcode / Δlight`.
fn shoulder() -> f64 {
    let t = table();
    (1.0 / 1023.0) / (t[1023] - t[1022])
}

/// `E(v)`: the profile's curve inverted to 1, `1 + s·ln v` above, odd.
pub(crate) fn encode(v: f64) -> f64 {
    let u = v.abs();
    let e = if u <= 1.0 {
        oetf(u)
    } else {
        1.0 + shoulder() * u.ln()
    };
    e.copysign(v)
}

/// The inverse of [`encode`], inside ±65504.
pub(crate) fn decode(f: f64) -> f64 {
    let u = f.abs();
    let l = if u <= 1.0 {
        eotf(u)
    } else {
        ((u - 1.0) / shoulder()).exp()
    };
    l.min(65504.0).copysign(f)
}

// ---- twins ----------------------------------------------------------------

const W: u32 = 32;
const H: u32 = 24;

/// An opaque SDR plate of 16-bit codes inside 2500…62500: ramps, a hard
/// edge, texture and specks.
pub(crate) fn plate_codes(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let ramp = 3000 + x * 1500 + y * 900;
            let edge = if x > w / 2 { 14000 } else { 0 };
            let texture = ((x * 7 + y * 13) % 17) * 600;
            let speck = if i % 37 == 0 { 9000 } else { 0 };
            let v = |o: u32| ((ramp + edge + texture + speck + o) % 60000 + 2500) as u16;
            [v(0), v(11000), v(29000), 65535]
        })
        .collect()
}

/// The float twin of `codes`: each code's linear light, `EOTF(c/65535)`.
pub(crate) fn float_of(w: u32, h: u32, codes: &[[u16; 4]]) -> PrecisionDocument {
    let pixels = codes
        .iter()
        .map(|p| {
            let c = |k: usize| eotf(f64::from(p[k]) / 65535.0) as f32;
            [c(0), c(1), c(2), f32::from(p[3]) / 65535.0]
        })
        .collect();
    solid_doc(&Picture { w, h, pixels })
}

/// `op` at 16 bits on `codes`, full coverage.
pub(crate) fn wide(w: u32, h: u32, codes: &[[u16; 4]], op: &str, params: &Value) -> Vec<[u16; 4]> {
    let doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, codes.to_vec(), IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let edit =
        SelectedEdit::parse(&json!({"layer": 0, "op": op, "params": params}).to_string()).unwrap();
    doc.selected_edit(&edit, &[], LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("{op} at 16 bits: {e}"))
        .unwrap_or_else(|| panic!("{op} at 16 bits: no change"))
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// TWIN for one request: the float result encoded against the 16-bit codes.
pub(crate) fn against_twin(w: u32, h: u32, codes: &[[u16; 4]], op: &str, params: &Value) {
    let twin = wide(w, h, codes, op, params);
    let got = filtered(&float_of(w, h, codes), op, params);
    let mut compared = 0;
    for (i, (g, c16)) in got.iter().zip(&twin).enumerate() {
        assert!(g.iter().all(|v| v.is_finite()), "{op} pixel {i}: {g:?}");
        for c in 0..3 {
            if c16[c] == 0 || c16[c] == 65535 {
                continue;
            }
            let (e, want) = (encode(f64::from(g[c])), f64::from(c16[c]) / 65535.0);
            assert!(
                (e - want).abs() <= TWIN,
                "{op} {params} pixel {i} channel {c}: E(out) {e}, 16-bit {want}"
            );
            compared += 1;
        }
    }
    assert!(compared > got.len() / 2, "{op}: only {compared} compared");
}

#[test]
fn the_display_filters_match_their_16_bit_twin_on_sdr_values() {
    let codes = plate_codes(W, H);
    for (op, params) in [
        ("surface_blur", json!({"radius": 3, "threshold": 40})),
        ("surface_blur", json!({"radius": 1, "threshold": 6})),
        // Threshold 0 is the plain median: no gate to sit on a boundary.
        ("dust_and_scratches", json!({"radius": 2, "threshold": 0})),
        (
            "reduce_noise",
            json!({"radius": 2, "luminance": 0.05, "detail": 0.4, "color": 0.6}),
        ),
        (
            "add_grain",
            json!({"amount": 60, "size": 1.5, "roughness": 30, "seed": 7}),
        ),
        ("clouds", json!({"seed": 5})),
        (
            "clouds",
            json!({"seed": 9, "foreground_color": "#204080", "background_color": "#f0e0a0",
                "high_contrast": true}),
        ),
        ("difference_clouds", json!({"seed": 5})),
        ("emboss", json!({})),
        ("edge_detect", json!({})),
    ] {
        against_twin(W, H, &codes, op, &params);
    }
}

// ---- above white ----------------------------------------------------------

/// 40×36: the SDR plate's light with a 12×12 block of 8.0 at (10, 8).
fn bright_block() -> Picture {
    let (w, h) = (40_u32, 36_u32);
    let codes = plate_codes(w, h);
    let pixels = codes
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (x, y) = (i as u32 % w, i as u32 / w);
            if (10..22).contains(&x) && (8..20).contains(&y) {
                return [8.0, 8.0, 8.0, 1.0];
            }
            let c = |k: usize| eotf(f64::from(p[k]) / 65535.0) as f32;
            [c(0), c(1), c(2), 1.0]
        })
        .collect();
    Picture { w, h, pixels }
}

/// The block's centre, (16, 14): every window below of radius 4 or less
/// lies inside the block.
const CENTRE: usize = 14 * 40 + 16;

#[test]
fn a_value_above_white_stays_above_white() {
    let picture = bright_block();
    let doc = solid_doc(&picture);
    for (op, params, kept) in [
        ("surface_blur", json!({"radius": 3, "threshold": 40}), true),
        (
            "dust_and_scratches",
            json!({"radius": 3, "threshold": 0}),
            true,
        ),
        ("sharpen", json!({"amount": 1.5}), true),
        (
            "reduce_noise",
            json!({"radius": 1, "luminance": 0.05, "detail": 0.4, "color": 0.6}),
            false,
        ),
        ("add_noise", json!({"amount": 0.2, "seed": 3}), false),
        ("pixelate", json!({"block": 4}), true),
    ] {
        let out = filtered(&doc, op, &params);
        assert!(out.iter().flatten().all(|v| v.is_finite()), "{op}");
        let p = out[CENTRE];
        for c in 0..3 {
            assert!(p[c] > 1.0, "{op}: {p:?}");
            if kept {
                assert!((f64::from(p[c]) - 8.0).abs() <= 1e-3 * 8.0, "{op}: {p:?}");
            }
        }
    }
    // Grain lives where 4Y(1 − Y) of the encoded luma does: none at or above
    // white, so the block keeps its bits.
    let grained = filtered(
        &doc,
        "add_grain",
        &json!({"amount": 100, "size": 1, "roughness": 50, "seed": 4}),
    );
    // Difference Clouds from black to black is |E − 0|: the block's code, so
    // its bits.
    let differenced = filter(
        &doc,
        "difference_clouds",
        &json!({"foreground_color": "#000000", "background_color": "#000000"}),
        &[],
    )
    .unwrap()
    .map_or_else(|| picture.pixels.clone(), |out| samples(&out));
    for out in [grained, differenced] {
        for (i, (got, was)) in out.iter().zip(&picture.pixels).enumerate() {
            if was[0] == 8.0 {
                assert_eq!(got.map(f32::to_bits), was.map(f32::to_bits), "{i}");
            }
        }
    }
    // Clouds replaces the pixel with the mix of two sRGB colours: inside 0…1.
    let clouded = filtered(&doc, "clouds", &json!({"seed": 2}));
    assert!(clouded
        .iter()
        .all(|p| p[..3].iter().all(|v| (0.0..=1.0).contains(v)) && p[3] == 1.0));
}

// ---- Surface Blur with headroom -------------------------------------------

#[test]
fn surface_blur_with_headroom_is_the_tent_on_the_encoded_copy() {
    let picture = bright_block();
    let doc = solid_doc(&picture);
    let (w, h) = (picture.w as i64, picture.h as i64);
    let encoded: Vec<[f64; 3]> = picture
        .pixels
        .iter()
        .map(|p| [0, 1, 2].map(|c| encode(f64::from(p[c]))))
        .collect();
    let lo = encoded.iter().flatten().fold(0.0_f64, |m, v| m.min(*v));
    let hi = encoded.iter().flatten().fold(1.0_f64, |m, v| m.max(*v));
    assert!(hi > 1.9, "the block is above white: {hi}");
    for (radius, threshold) in [(2_i64, 30.0), (4, 100.0)] {
        let got = filtered(
            &doc,
            "surface_blur",
            &json!({"radius": radius, "threshold": threshold}),
        );
        // The tent reaches 2.5·T levels, a level being 1/255 of the 0…1 code.
        let reach = 2.5 * threshold / 255.0;
        let bound = (hi - lo) / 255.0 + 2.0 / 65535.0;
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                for c in 0..3 {
                    let centre = encoded[i][c];
                    let (mut num, mut den) = (0.0, 0.0);
                    for dy in -radius..=radius {
                        for dx in -radius..=radius {
                            let sx = (x + dx).clamp(0, w - 1);
                            let sy = (y + dy).clamp(0, h - 1);
                            let v = encoded[(sy * w + sx) as usize][c];
                            let weight = (1.0 - (v - centre).abs() / reach).max(0.0);
                            num += weight * v;
                            den += weight;
                        }
                    }
                    let want = num / den;
                    let e = encode(f64::from(got[i][c]));
                    assert!(
                        (e - want).abs() <= bound,
                        "r{radius} T{threshold} pixel {i} channel {c}: {e} vs {want}"
                    );
                }
            }
        }
    }
}

// ---- Sharpen ------------------------------------------------------------

#[test]
fn sharpen_is_unsharp_mask_amount_one_on_the_encoded_copy() {
    let picture = bright_block();
    let doc = solid_doc(&picture);
    let encoded = Picture {
        w: picture.w,
        h: picture.h,
        pixels: picture
            .pixels
            .iter()
            .map(|p| {
                let e = |c: usize| encode(f64::from(p[c])) as f32;
                [e(0), e(1), e(2), p[3]]
            })
            .collect(),
    };
    for amount in [0.6_f64, 1.5, 9.0] {
        let sigma = f64::from(amount.min(5.0) as f32);
        let got = filtered(&doc, "sharpen", &json!({"amount": amount}));
        let low = gaussian_reference(&encoded, sigma);
        for (i, (g, l)) in got.iter().zip(&low).enumerate() {
            for c in 0..3 {
                let e = f64::from(encoded.pixels[i][c]);
                let want = decode(2.0 * e - l[c] / l[3]);
                let value = f64::from(g[c]);
                assert!(
                    (value - want).abs() <= 1e-4 * want.abs().max(1.0),
                    "amount {amount} pixel {i} channel {c}: {value} vs {want}"
                );
            }
            assert_eq!(g[3], 1.0);
        }
        // The user's Unsharp Mask with the same numbers stays linear.
        let unsharp = filtered(
            &doc,
            "unsharp_mask",
            &json!({"radius": sigma, "amount": 1, "threshold": 0}),
        );
        assert_ne!(bits(&unsharp), bits(&got), "amount {amount}");
    }
}

// ---- Pixelate and Add Noise (linear) --------------------------------------

#[test]
fn pixelate_is_the_premultiplied_linear_cell_mean() {
    let (doc, picture) = document(&crate::float_filters::hdr_picture(true));
    for block in [2_u32, 5, 8] {
        let got = filtered(&doc, "pixelate", &json!({"block": block}));
        let (w, h) = (picture.w, picture.h);
        for cy in (0..h).step_by(block as usize) {
            for cx in (0..w).step_by(block as usize) {
                let (mut ca, mut a, mut n) = ([0.0_f64; 3], 0.0_f64, 0.0_f64);
                for y in cy..(cy + block).min(h) {
                    for x in cx..(cx + block).min(w) {
                        let p = picture.pixels[(y * w + x) as usize];
                        a += f64::from(p[3]);
                        n += 1.0;
                        for c in 0..3 {
                            ca[c] += f64::from(p[c]) * f64::from(p[3]);
                        }
                    }
                }
                for y in cy..(cy + block).min(h) {
                    for x in cx..(cx + block).min(w) {
                        let g = got[(y * w + x) as usize];
                        if a == 0.0 {
                            assert_eq!(g.map(f32::to_bits), [0; 4]);
                            continue;
                        }
                        let want = [ca[0] / a, ca[1] / a, ca[2] / a, a / n];
                        for c in 0..4 {
                            assert!(
                                (f64::from(g[c]) - want[c]).abs() <= 1e-6 * want[c].abs().max(1.0),
                                "block {block} cell ({cx},{cy}) channel {c}: {g:?} vs {want:?}"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(filter(&doc, "pixelate", &json!({"block": 1}), &[])
        .unwrap()
        .is_none());
}

#[test]
fn add_noise_is_relative_in_linear_light_with_the_16_bit_draw() {
    let (w, h) = (24_u32, 16_u32);
    let amount = 0.2;
    let params = json!({"amount": amount, "seed": 11});
    // Mid-grey at 16 bits, where the additive draw is never clamped.
    let codes = vec![[32768_u16, 32768, 32768, 65535]; (w * h) as usize];
    let twin = wide(w, h, &codes, "add_noise", &params);
    let v = eotf(32768.0 / 65535.0);
    let got = filtered(&float_of(w, h, &codes), "add_noise", &params);
    for (i, (g, c16)) in got.iter().zip(&twin).enumerate() {
        for c in 0..3 {
            let delta16 = (f64::from(c16[c]) - 32768.0) / 65535.0;
            let ratio = f64::from(g[c]) / f64::from(v as f32) - 1.0;
            assert!(
                (ratio - delta16).abs() <= 2.0 / 65535.0,
                "pixel {i} channel {c}: {ratio} vs {delta16}"
            );
            assert!(ratio.abs() <= amount + 1e-6);
        }
    }
    // Zero stays zero; a highlight gets grain in proportion; the coverage
    // never changes a pixel's draw.
    let mut pixels = vec![[0.0_f32, 0.5, 6.0, 1.0]; (w * h) as usize];
    pixels[5] = [1.0, 1.0, 1.0, 1.0];
    let doc = solid_doc(&Picture { w, h, pixels });
    let full = filtered(&doc, "add_noise", &params);
    for p in full
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 5)
        .map(|(_, p)| p)
    {
        assert_eq!(p[0].to_bits(), 0.0_f32.to_bits());
        assert!(p[2] >= 6.0 * (1.0 - amount as f32) - 1e-5 && p[2] > 1.0);
    }
    let coverage: Vec<u16> = (0..w * h)
        .map(|i| if i % 3 == 0 { 0 } else { u16::MAX })
        .collect();
    let covered = samples(
        &filter(&doc, "add_noise", &params, &coverage)
            .unwrap()
            .unwrap(),
    );
    for (i, (c, f)) in covered.iter().zip(&full).enumerate() {
        if coverage[i] == u16::MAX {
            assert_eq!(c.map(f32::to_bits), f.map(f32::to_bits), "{i}");
        }
    }
}

// ---- Emboss and Edge Detect bounds ---------------------------------------------

#[test]
fn emboss_and_edge_detect_stay_inside_the_encoded_range() {
    // An SDR step 0 → 1 (vertical edge): at 16 bits the output is in 0…1, and
    // so it is here, edges included.
    let (w, h) = (16_u32, 12_u32);
    let step: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let v = if i % w < w / 2 { 0.0 } else { 1.0 };
            [v, v, v, 1.0]
        })
        .collect();
    let doc = solid_doc(&Picture { w, h, pixels: step });
    for op in ["emboss", "edge_detect"] {
        for p in filtered(&doc, op, &json!({})) {
            assert!(
                p[..3].iter().all(|v| (0.0..=1.0).contains(v)),
                "{op}: {p:?}"
            );
            assert_eq!(p[3], 1.0);
        }
    }
    // A flat region embosses to its encoded luma + 0.5 (the kernel's weights
    // sum to 1): a flat black one to the display's mid-grey, EOTF(0.5).
    let flat = filtered(&doc, "emboss", &json!({}));
    assert!(
        (f64::from(flat[0][0]) - eotf(0.5)).abs() <= 2e-6,
        "{:?}",
        flat[0]
    );
    // A layer holding 8.0: each encoded channel within [min(0, lo), max(1, hi)].
    let picture = bright_block();
    let doc = solid_doc(&picture);
    let mut lo = [0.0_f64; 3];
    let mut hi = [1.0_f64; 3];
    for p in &picture.pixels {
        for c in 0..3 {
            let e = encode(f64::from(p[c]));
            lo[c] = lo[c].min(e);
            hi[c] = hi[c].max(e);
        }
    }
    for op in ["emboss", "edge_detect"] {
        let out = filtered(&doc, op, &json!({}));
        assert!(
            out.iter().any(|p| p[0] > 1.0),
            "{op}: an edge of the block passes white"
        );
        for p in out {
            for c in 0..3 {
                let e = encode(f64::from(p[c]));
                assert!(e >= lo[c] - 1e-5 && e <= hi[c] + 1e-5, "{op}: {p:?}");
            }
        }
    }
}

// ---- shared cases -----------------------------------------------------------

/// One request per filter this file covers, each changing the SDR plate.
fn requests() -> Vec<(&'static str, Value)> {
    vec![
        ("surface_blur", json!({"radius": 2, "threshold": 30})),
        (
            "dust_and_scratches",
            json!({"radius": 2, "threshold": 0.02}),
        ),
        ("reduce_noise", json!({"radius": 2, "luminance": 0.05})),
        ("add_noise", json!({"amount": 0.1, "seed": 2})),
        ("add_grain", json!({"amount": 40, "seed": 2})),
        ("clouds", json!({"seed": 3})),
        ("difference_clouds", json!({"seed": 3})),
        ("emboss", json!({})),
        ("edge_detect", json!({})),
        ("pixelate", json!({"block": 3})),
        ("sharpen", json!({"amount": 1})),
    ]
}

#[test]
fn coverage_gates_each_display_filter_and_mixes_in_linear_light() {
    let (w, h) = (W, H);
    let doc = float_of(w, h, &plate_codes(w, h));
    let before = samples(&doc);
    let count = (w * h) as usize;
    let weights: Vec<u16> = (0..count)
        .map(|i| [0, 32768, u16::MAX][(i % 32 + i / 32) % 3])
        .collect();
    for (op, params) in requests() {
        let full = filtered(&doc, op, &params);
        let out = samples(&filter(&doc, op, &params, &weights).unwrap().unwrap());
        for i in 0..count {
            let (b, a, g) = (before[i], full[i], out[i]);
            match weights[i] {
                0 => assert_eq!(g.map(f32::to_bits), b.map(f32::to_bits), "{op} {i}"),
                u16::MAX => assert_eq!(g.map(f32::to_bits), a.map(f32::to_bits), "{op} {i}"),
                weight => {
                    // Opaque before and after: the straight lerp of linear values.
                    let t = f64::from(weight) / 65535.0;
                    for c in 0..3 {
                        let want = f64::from(b[c]) + t * (f64::from(a[c]) - f64::from(b[c]));
                        assert!(
                            (f64::from(g[c]) - want).abs() <= 2e-6 * want.abs().max(1.0),
                            "{op} {i} channel {c}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn the_plan_charges_the_encoded_copy_and_a_cancel_stops_each_filter() {
    let (w, h) = (W, H);
    let doc = float_of(w, h, &plate_codes(w, h));
    let count = u64::from(w * h);
    for (op, params) in requests() {
        let plan = plan(&doc, op, &params).unwrap();
        assert_eq!(plan["output_bytes"], count * 16, "{op}");
        let display = !matches!(op, "add_noise" | "pixelate");
        // The apply stage, plus the encoded copy at 5/4 in the display domain.
        let least = count * 40 + if display { count * 20 } else { 0 };
        assert!(
            plan["scratch_bytes"].as_u64().unwrap() >= least,
            "{op}: {plan}"
        );
        let edit =
            SelectedEdit::parse(&json!({"layer": 0, "op": op, "params": params}).to_string())
                .unwrap();
        let control = DrJobControl::default();
        control.cancel();
        assert_eq!(
            doc.selected_edit(&edit, &[], LIMIT, 0, Some(&control))
                .err()
                .as_deref(),
            Some("operation cancelled"),
            "{op}"
        );
        // A live control changes nothing.
        let live = DrJobControl::default();
        let controlled = doc
            .selected_edit(&edit, &[], LIMIT, 0, Some(&live))
            .unwrap()
            .unwrap();
        assert_eq!(
            bits(&samples(&controlled)),
            bits(&filtered(&doc, op, &params)),
            "{op}"
        );
    }
}
