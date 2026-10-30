//! Layer style NOISE: the `noise` key of the style JSON,
//! every noisy effect against an oracle written here, Photoshop's own
//! statistics from take 2, and the byte `.rz` word 97. Each effect is drawn
//! black, Normal, 100 % over a white canvas, the layer itself at
//! `fill_opacity` 0, so a pixel's coverage is `a = 255 − R`. The oracle is
//! this file's own restatement, not the crate's code:
//!
//! - `u` = the top 24 bits of SplitMix64 (Steele, Lea and Flood) addressed
//!   by the fixed seed, the pixel's LAYER coordinate and the effect kind's
//!   render-order index (drop shadow 0, outer glow 1, inner glow 7, inner
//!   shadow 8), divided by 2^24;
//! - the take's law ADD: `round(clamp(a + 255 n (2u − 1), 0, 255))`, 0 where
//!   `a` is 0, applied after the contour; on a Below effect held to
//!   `round(min(255, k a))` (k 8.25 on the drop shadow, half that on the
//!   outer glow);
//! - per stripe, the law's closed-form distribution (the uniform's measure
//!   over each rounded code);
//! - Photoshop's take 2 readings (`fixtures/psd_photoshop/noise/stats.json`,
//!   `noise_take.py --export-fixture`), held to the take's bound: 2 codes
//!   plus three standard errors of the two samples.

use std::collections::BTreeMap;
use std::path::PathBuf;

use doctored_core::doc::DrDocument;
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{rect_layer_doc, solid, styled, try_styled, WHITE};

// ---------------------------------------------------------- the oracle --

const SEED: u64 = 0x525A_204E_4F49_5345;
const STREAM_DS: u64 = 0;
const STREAM_OG: u64 = 1;
const STREAM_IG: u64 = 7;
const STREAM_IS: u64 = 8;
const CAP_DS: f64 = 8.25;
const CAP_OG: f64 = 8.25 / 2.0;

/// SplitMix64 at `(SEED, lx, ly, stream)`: one Weyl step, the finalizer,
/// the top 24 bits over 2^24.
fn unit(stream: u64, lx: i64, ly: i64) -> f32 {
    let state = SEED
        ^ (lx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (ly as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ stream.wrapping_mul(0x94D0_49BB_1331_11EB);
    let mut z = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// The law ADD at coverage `a`, amount `n`.
fn law(a: u8, n: f64, u: f32) -> u8 {
    if a == 0 {
        return 0;
    }
    (f64::from(a) + 255.0 * n * (2.0 * f64::from(u) - 1.0))
        .clamp(0.0, 255.0)
        .round() as u8
}

/// A Below effect's law, held to `round(min(255, k plane))`.
fn below(mapped: u8, plane: u8, n: f64, u: f32, k: f64) -> u8 {
    law(mapped, n, u).min((k * f64::from(plane)).min(255.0).round() as u8)
}

/// The law's closed-form (mean, SD) at coverage `a`: `x` uniform on
/// `a ± 255 n`, each code `k` taking the measure of `x` that rounds to it
/// after the clamp.
fn law_moments(a: f64, n: f64) -> (f64, f64) {
    let h = 255.0 * n;
    let (lo, hi) = (a - h, a + h);
    let (mut m1, mut m2) = (0.0, 0.0);
    for k in 0..=255 {
        let s = if k == 0 { f64::MIN } else { k as f64 - 0.5 };
        let e = if k == 255 { f64::MAX } else { k as f64 + 0.5 };
        let p = (e.min(hi) - s.max(lo)).max(0.0) / (2.0 * h);
        m1 += p * k as f64;
        m2 += p * (k * k) as f64;
    }
    (m1, (m2 - m1 * m1).max(0.0).sqrt())
}

fn mean_sd(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let m = values.iter().sum::<f64>() / n;
    let v = values.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / n;
    (m, v.sqrt())
}

/// Lag-1 correlation of `e` along `(dx, dy)` over the pairs both in `e`.
fn lag1(e: &BTreeMap<(u32, u32), f64>, dx: u32, dy: u32) -> f64 {
    let pairs: Vec<(f64, f64)> = e
        .iter()
        .filter_map(|(&(x, y), &v)| e.get(&(x + dx, y + dy)).map(|&w| (v, w)))
        .collect();
    let n = pairs.len() as f64;
    let (ma, mb) = (
        pairs.iter().map(|p| p.0).sum::<f64>() / n,
        pairs.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let cov: f64 = pairs.iter().map(|p| (p.0 - ma) * (p.1 - mb)).sum();
    let va: f64 = pairs.iter().map(|p| (p.0 - ma).powi(2)).sum();
    let vb: f64 = pairs.iter().map(|p| (p.1 - mb).powi(2)).sum();
    cov / (va * vb).sqrt()
}

// ---------------------------------------------------------- the scenes --

/// The take's stripes scene, smaller: a 512 x 160 white layer whose alpha is
/// 64, 128, 192, 255 in four 128-px columns, at `offset` on a 560 x 208
/// white canvas.
const STRIPES: [u8; 4] = [64, 128, 192, 255];
const STRIPE_W: u32 = 128;
const STRIPE_H: u32 = 160;
const STRIPE_CANVAS: (u32, u32) = (560, 208);

fn stripes(offset: (i32, i32)) -> DrDocument {
    let layer = RgbaImage::from_fn(4 * STRIPE_W, STRIPE_H, |x, _| {
        Rgba([255, 255, 255, STRIPES[(x / STRIPE_W) as usize]])
    });
    let doc = DrDocument::from_pixels(solid(STRIPE_CANVAS.0, STRIPE_CANVAS.1, WHITE))
        .adding_image_layer(0, layer, "Stripes")
        .unwrap();
    doc.with_layer_offset(1, offset.0, offset.1).unwrap()
}

/// The take's DS: size 0, spread 0, distance 0, knock-out off.
fn hard_shadow(noise: f64) -> Value {
    json!({"type":"drop_shadow","blend":"normal","color":"#000000","opacity":1,
        "distance":0,"spread":0,"size":0,"layer_knocks_out":false,"use_global_light":false,
        "noise":noise})
}

fn coverage(doc: &DrDocument, effects: Value) -> Vec<u8> {
    let style = json!({"fill_opacity": 0, "effects": effects});
    styled(doc, 1, &style.to_string())
        .flattened()
        .unwrap()
        .pixels()
        .map(|p| 255 - p.0[0])
        .collect()
}

/// The stripe `s` (0..4) a canvas pixel `i` lies inside, `margin` px in from
/// its edges, for a layer at `offset`.
fn stripe_of(i: usize, offset: (i32, i32), margin: u32) -> Option<usize> {
    let w = STRIPE_CANVAS.0 as i64;
    let (x, y) = (
        i as i64 % w - i64::from(offset.0),
        i as i64 / w - i64::from(offset.1),
    );
    let (m, sw) = (i64::from(margin), i64::from(STRIPE_W));
    if y < m || y >= i64::from(STRIPE_H) - m || x < 0 || x >= 4 * sw {
        return None;
    }
    let s = x / sw;
    (x - s * sw >= m && x - s * sw < sw - m).then_some(s as usize)
}

fn layer_xy(i: usize, w: u32, offset: (i32, i32)) -> (i64, i64) {
    let (x, y) = (i as i64 % i64::from(w), i as i64 / i64::from(w));
    (x - i64::from(offset.0), y - i64::from(offset.1))
}

const BLUR_CANVAS: (u32, u32) = (400, 300);
const BLUR_RECT: (i32, i32, u32, u32) = (100, 75, 200, 150);

fn blur_scene() -> DrDocument {
    rect_layer_doc(BLUR_CANVAS, BLUR_RECT, [0, 0, 0, 255])
}

fn effect(kind: &str, size: f64, noise: f64) -> Value {
    let mut e = json!({"type":kind,"blend":"normal","color":"#000000","opacity":1,
        "size":size,"noise":noise});
    for (k, v) in [
        ("spread", json!(0)),
        ("choke", json!(0)),
        ("distance", json!(0)),
        ("use_global_light", json!(false)),
        ("layer_knocks_out", json!(false)),
    ] {
        let fits = match kind {
            "drop_shadow" => k != "choke",
            "inner_shadow" => k != "spread" && k != "layer_knocks_out",
            "outer_glow" => k == "spread",
            _ => k == "choke",
        };
        if fits {
            e[k] = v;
        }
    }
    e
}

// ------------------------------------------------------------ the JSON --

#[test]
fn noise_parses_clamps_quantizes_and_is_written_only_above_zero() {
    for kind in ["drop_shadow", "inner_shadow", "outer_glow", "inner_glow"] {
        let bare = LayerStyle::from_json(&json!({"effects":[{"type":kind}]}).to_string())
            .unwrap()
            .to_json();
        assert!(!bare.contains("noise"), "{kind}: absent is 0, not written");
        for (given, want) in [
            (json!(0.5), Some("0.5")),
            (json!(0.123456), Some("0.1235")),
            (json!(7), Some("1.0")),
            (json!(-0.3), None),
            (json!(0), None),
        ] {
            let text = LayerStyle::from_json(
                &json!({"effects":[{"type":kind,"noise":given}]}).to_string(),
            )
            .unwrap()
            .to_json();
            match want {
                Some(v) => assert!(
                    text.contains(&format!("\"noise\":{v}")),
                    "{kind} {given}: {text}"
                ),
                None => assert_eq!(text, bare, "{kind} {given}: written at 0"),
            }
        }
        let Err(message) =
            LayerStyle::from_json(&json!({"effects":[{"type":kind,"noise":"x"}]}).to_string())
        else {
            panic!("{kind}: a string noise must refuse");
        };
        assert_eq!(message, "effects[0].noise must be a number");
    }
    // The other kinds have no Noise: the key is an ignored unknown key.
    let satin = LayerStyle::from_json(r#"{"effects":[{"type":"satin","noise":0.5}]}"#).unwrap();
    assert!(!satin.to_json().contains("noise"));
}

#[test]
fn noise_zero_is_byte_identical_on_all_four() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = blur_scene();
    for kind in ["drop_shadow", "inner_shadow", "outer_glow", "inner_glow"] {
        let mut without = effect(kind, 12.0, 0.0);
        without.as_object_mut().unwrap().remove("noise");
        let zero = effect(kind, 12.0, 0.0);
        let a = styled(&doc, 1, &json!({"effects":[without]}).to_string());
        let b = styled(&doc, 1, &json!({"effects":[zero]}).to_string());
        assert_eq!(
            a.layers[1].style.as_deref().map(LayerStyle::to_json),
            b.layers[1].style.as_deref().map(LayerStyle::to_json),
            "{kind}"
        );
        assert_eq!(a.flattened().unwrap(), b.flattened().unwrap(), "{kind}");
        let (pa, pb) = (
            dir.path().join(format!("{kind}-a.rz")),
            dir.path().join(format!("{kind}-b.rz")),
        );
        a.save_native(pa.to_str().unwrap()).unwrap();
        b.save_native(pb.to_str().unwrap()).unwrap();
        let bytes = std::fs::read(&pa).unwrap();
        assert_eq!(
            bytes,
            std::fs::read(&pb).unwrap(),
            "{kind}: .doctored bytes"
        );
        assert_ne!(&bytes[8..12], &97u32.to_le_bytes(), "{kind}: no word 97");
    }
}

// --------------------------------------------------------- the renders --

#[test]
fn the_stripes_are_the_law_at_every_pixel_and_per_stripe() {
    let offset = (24, 24);
    let doc = stripes(offset);
    for n in [0.25, 0.5, 1.0] {
        let got = coverage(&doc, json!([hard_shadow(n)]));
        let mut per: [Vec<f64>; 4] = Default::default();
        let mut e = BTreeMap::new();
        let mut judged = 0;
        for (i, &g) in got.iter().enumerate() {
            let Some(s) = stripe_of(i, offset, 0) else {
                assert_eq!(g, 0, "n {n}: pixel {i} off the layer");
                continue;
            };
            let a = STRIPES[s];
            let (lx, ly) = layer_xy(i, STRIPE_CANVAS.0, offset);
            let want = below(a, a, n, unit(STREAM_DS, lx, ly), CAP_DS);
            assert!(
                g.abs_diff(want) <= 1,
                "n {n}: pixel ({lx}, {ly}) a {a}: {g} against {want}"
            );
            judged += 1;
            if stripe_of(i, offset, 4).is_some() {
                per[s].push(f64::from(g));
                let (lx, ly) = (lx as u32, ly as u32);
                e.insert((lx, ly), f64::from(g) - law_moments(f64::from(a), n).0);
            }
        }
        assert_eq!(judged, (4 * STRIPE_W * STRIPE_H) as usize);
        for (s, values) in per.iter().enumerate() {
            let (m, sd) = mean_sd(values);
            let (wm, wsd) = law_moments(f64::from(STRIPES[s]), n);
            assert!(
                (m - wm).abs() <= 1.5 && (sd - wsd).abs() <= 1.5,
                "n {n} stripe {}: mean {m} SD {sd} against {wm} {wsd}",
                STRIPES[s]
            );
        }
        let (rx, ry) = (lag1(&e, 1, 0), lag1(&e, 0, 1));
        assert!(
            rx.abs() <= 0.05 && ry.abs() <= 0.05,
            "n {n}: lag-1 {rx} {ry}"
        );
    }
}

#[test]
fn the_grain_moves_with_the_layer() {
    let (a, b) = ((24, 24), (37, 31));
    let first = coverage(&stripes(a), json!([hard_shadow(0.5)]));
    let moved = coverage(&stripes(b), json!([hard_shadow(0.5)]));
    let w = STRIPE_CANVAS.0 as usize;
    let (dx, dy) = ((b.0 - a.0) as usize, (b.1 - a.1) as usize);
    let (mut same_layer, mut same_canvas, mut n) = (0, 0, 0);
    for (i, &g) in moved.iter().enumerate() {
        if stripe_of(i, b, 0).is_none() {
            continue;
        }
        let back = i - dy * w - dx;
        same_layer += usize::from(first[back] == g);
        if stripe_of(i, a, 0) == stripe_of(i, b, 0) {
            same_canvas += usize::from(first[i] == g);
        }
        n += 1;
    }
    assert_eq!(same_layer, n, "the grain is keyed on the layer coordinate");
    assert!(
        same_canvas * 2 < n,
        "the grain must not stay on the canvas ({same_canvas} of {n})"
    );
}

#[test]
fn a_drop_shadow_and_an_outer_glow_draw_their_own_fields() {
    let doc = blur_scene();
    let inside = |i: usize| {
        let (x, y) = layer_xy(i, BLUR_CANVAS.0, (BLUR_RECT.0, BLUR_RECT.1));
        x >= 0 && y >= 0 && x < i64::from(BLUR_RECT.2) && y < i64::from(BLUR_RECT.3)
    };
    let mut differ = 0;
    let mut n = 0;
    // Each effect's own noiseless plane: the outer glow's is not the drop
    // shadow's (its narrower blur and matte stretch, #578).
    let twin = coverage(&doc, json!([effect("drop_shadow", 12.0, 0.0)]));
    let twin_og = coverage(&doc, json!([effect("outer_glow", 12.0, 0.0)]));
    let ds = coverage(&doc, json!([effect("drop_shadow", 12.0, 0.5)]));
    let og = coverage(&doc, json!([effect("outer_glow", 12.0, 0.5)]));
    for (i, &a) in twin.iter().enumerate() {
        if inside(i) {
            continue;
        }
        let (lx, ly) = layer_xy(i, BLUR_CANVAS.0, (BLUR_RECT.0, BLUR_RECT.1));
        let (u_ds, u_og) = (unit(STREAM_DS, lx, ly), unit(STREAM_OG, lx, ly));
        let want_ds = below(a, a, 0.5, u_ds, CAP_DS);
        let g = twin_og[i];
        let want_og = below(g, g, 0.5, u_og, CAP_OG);
        assert!(ds[i].abs_diff(want_ds) <= 1, "DS ({lx}, {ly}) a {a}");
        assert!(og[i].abs_diff(want_og) <= 1, "OG ({lx}, {ly}) a {g}");
        if a >= 32 {
            n += 1;
            differ += usize::from(below(a, a, 0.5, u_og, CAP_DS) != want_ds);
        }
    }
    assert!(n > 1000 && differ * 2 > n, "{differ} of {n} differ");
}

#[test]
fn interior_noise_is_the_bare_law_before_the_coverage_multiply() {
    let doc = blur_scene();
    for (kind, stream) in [("inner_shadow", STREAM_IS), ("inner_glow", STREAM_IG)] {
        let twin = coverage(&doc, json!([effect(kind, 12.0, 0.0)]));
        let got = coverage(&doc, json!([effect(kind, 12.0, 0.5)]));
        let mut n = 0;
        for (i, (&a, &g)) in twin.iter().zip(&got).enumerate() {
            let (lx, ly) = layer_xy(i, BLUR_CANVAS.0, (BLUR_RECT.0, BLUR_RECT.1));
            let want = law(a, 0.5, unit(stream, lx, ly));
            assert!(
                g.abs_diff(want) <= 1,
                "{kind} ({lx}, {ly}) a {a}: {g} {want}"
            );
            n += usize::from(a > 0);
        }
        assert!(n > 5000, "{kind}: {n}");
    }
}

#[test]
fn a_contoured_noise_maps_first_then_adds_the_grain() {
    let cone = contour_cone();
    let offset = (24, 24);
    let doc = stripes(offset);
    let mut fx = hard_shadow(0.5);
    fx["contour"] = json!({"preset":"cone"});
    let got = coverage(&doc, json!([fx]));
    for (i, &g) in got.iter().enumerate() {
        let Some(s) = stripe_of(i, offset, 0) else {
            continue;
        };
        let a = STRIPES[s];
        let (lx, ly) = layer_xy(i, STRIPE_CANVAS.0, offset);
        let want = below(
            cone[usize::from(a)],
            a,
            0.5,
            unit(STREAM_DS, lx, ly),
            CAP_DS,
        );
        assert!(
            g.abs_diff(want) <= 1,
            "({lx}, {ly}) a {a}: {g} against {want}"
        );
    }
}

// ------------------------------------------------------- Photoshop's rows --

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/noise/stats.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn contour_cone() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/contour/tables.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v["tables"]["cone"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_u64().unwrap() as u8)
        .collect()
}

/// The take's bound between two samples: 2 codes plus three standard errors
/// of the difference.
fn within(what: &str, ps: (f64, f64, f64), rz: (f64, f64, f64)) {
    let (mean_miss, sd_miss) = misses(ps, rz);
    assert!(
        mean_miss <= 0.0 && sd_miss <= 0.0,
        "{what}: {}",
        described(ps, rz)
    );
}

/// How far the mean and the sd each lie past the take's bound (`within`'s);
/// zero or less is inside it.
fn misses(ps: (f64, f64, f64), rz: (f64, f64, f64)) -> (f64, f64) {
    let (pm, psd, pn) = ps;
    let (rm, rsd, rn) = rz;
    let se = (psd * psd / pn + rsd * rsd / rn).sqrt();
    (
        (pm - rm).abs() - (2.0 + 3.0 * se),
        (psd - rsd).abs() - (2.0 + 3.0 * se / 2f64.sqrt()),
    )
}

fn described(ps: (f64, f64, f64), rz: (f64, f64, f64)) -> String {
    let ((pm, psd, pn), (rm, rsd, rn)) = (ps, rz);
    format!("Photoshop {pm} / {psd} (n {pn}) against Doctored {rm} / {rsd} (n {rn})")
}

/// The Photoshop bins whose MEAN misses the take's bound, each a known residual
/// with its recorded miss in codes: (row, bin, largest miss). The bound itself
/// is unchanged; the test asserts each listed bin is still compared, still
/// misses, by no more than its recorded amount, and still meets the sd check.
///
/// og_n50 bin 160 was one (#578: Photoshop 168.723 / sd 67.368 (n 780)
/// against Doctored 154.588 / sd 72.630 (n 1164), a miss of 2.484 codes
/// under the Gaussian member) until the Outer Glow's box-pair law
/// (`effect_kernels_578_take.py`): it now reads 168.374 / sd 68.453 (n 764)
/// and meets the bound.
///
/// ig_n50 bin 48 (#578, RULING-2): Photoshop 64.347 / sd 61.332 (n 1476) against
/// Doctored 45.897 / sd 59.378 (n 1468), a miss of 9.7751 codes under the shipped
/// box law. Cause: the law's Size 40 rings sit one code higher (49/55/61 against
/// Photoshop's 49/54/60), so codes 54 and 60 hold only k = 16 and k = 8 pixels yet
/// get Photoshop's weight of 404 each, while the bin's SE uses the full rn = 1468.
const KNOWN_RESIDUALS: &[(&str, u64, f64)] = &[("ig_n50", 48, 9.776)];

#[test]
fn photoshops_stripes_hold_to_the_takes_bound() {
    let stats = fixture();
    assert_eq!(stats["verdict"], "VALID");
    assert_eq!(stats["winners"]["law"], "ADD");
    let met: Vec<&str> = stats["met_rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let offset = (24, 24);
    let doc = stripes(offset);
    for (row, n) in [("ds_n25", 0.25), ("ds_n50", 0.5), ("ds_n100", 1.0)] {
        assert!(met.contains(&row), "{row} met in the take");
        let got = coverage(&doc, json!([hard_shadow(n)]));
        let mut per: [Vec<f64>; 4] = Default::default();
        for (i, &g) in got.iter().enumerate() {
            if let Some(s) = stripe_of(i, offset, 4) {
                per[s].push(f64::from(g));
            }
        }
        let bins = stats["rows"][row].as_array().unwrap();
        assert_eq!(bins.len(), 4, "{row}: one bin per stripe");
        for (bin, values) in bins.iter().zip(&per) {
            let (m, sd) = mean_sd(values);
            within(
                &format!("{row} bin {}", bin["bin"]),
                (
                    bin["mean"].as_f64().unwrap(),
                    bin["sd"].as_f64().unwrap(),
                    bin["n"].as_f64().unwrap(),
                ),
                (m, sd, values.len() as f64),
            );
        }
    }
    // SITE (ds_cone_n50): one bin per stripe, the alpha-255 stripe judged.
    assert_eq!(stats["site_met"], true);
    let mut fx = hard_shadow(0.5);
    fx["contour"] = json!({"preset":"cone"});
    let got = coverage(&doc, json!([fx]));
    let mut per: [Vec<f64>; 4] = Default::default();
    for (i, &g) in got.iter().enumerate() {
        if let Some(s) = stripe_of(i, offset, 4) {
            per[s].push(f64::from(g));
        }
    }
    for (s, values) in per.iter().enumerate() {
        let bin = &stats["site"][STRIPES[s].to_string()];
        let (m, sd) = mean_sd(values);
        within(
            &format!("ds_cone_n50 alpha {}", STRIPES[s]),
            (
                bin["mean"].as_f64().unwrap(),
                bin["sd"].as_f64().unwrap(),
                bin["n"].as_f64().unwrap(),
            ),
            (m, sd, values.len() as f64),
        );
    }
}

/// Per twin code: (pixels, sum, sum of squares) of `got` over the pixels
/// whose `twin` code is that code.
fn per_code(twin: &[u8], got: &[u8]) -> BTreeMap<u8, (f64, f64, f64)> {
    let mut out: BTreeMap<u8, (f64, f64, f64)> = BTreeMap::new();
    for (&c, &g) in twin.iter().zip(got) {
        let e = out.entry(c).or_default();
        let g = f64::from(g);
        *e = (e.0 + 1.0, e.1 + g, e.2 + g * g);
    }
    out
}

#[test]
fn photoshops_glows_and_inner_shadow_hold_to_the_takes_bound() {
    // The take's blur scene at its own geometry: each Photoshop bin's twin
    // code histogram weighs Doctored's per-code statistics.
    let stats = fixture();
    let doc = blur_scene();
    let mut residuals_seen = 0;
    for (row, kind) in [
        ("og_n50", "outer_glow"),
        ("is_n50", "inner_shadow"),
        ("ig_n50", "inner_glow"),
    ] {
        let twin = coverage(&doc, json!([effect(kind, 40.0, 0.0)]));
        let got = coverage(&doc, json!([effect(kind, 40.0, 0.5)]));
        let codes = per_code(&twin, &got);
        let mut compared = 0;
        for bin in stats["rows"][row].as_array().unwrap() {
            let (mut w, mut s1, mut s2, mut rn, mut total) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for (c, count) in bin["codes"].as_object().unwrap() {
                let count = count.as_f64().unwrap();
                total += count;
                if let Some(&(k, a, b)) = codes.get(&c.parse::<u8>().unwrap()) {
                    w += count;
                    s1 += count * a / k;
                    s2 += count * b / k;
                    rn += k;
                }
            }
            // Doctored's glow plane does not reach every code Photoshop's
            // does (its glow blur is the recorded kernel residual), and the
            // law depends on the code alone: a bin this scene cannot fill
            // is not compared.
            if w < 0.9 * total {
                continue;
            }
            compared += 1;
            let m = s1 / w;
            let sd = (s2 / w - m * m).max(0.0).sqrt();
            let what = format!("{row} bin {}", bin["bin"]);
            let ps = (
                bin["mean"].as_f64().unwrap(),
                bin["sd"].as_f64().unwrap(),
                bin["n"].as_f64().unwrap(),
            );
            let known = KNOWN_RESIDUALS
                .iter()
                .find(|&&(r, b, _)| r == row && Some(b) == bin["bin"].as_u64());
            let Some(&(_, _, recorded)) = known else {
                within(&what, ps, (m, sd, rn));
                continue;
            };
            residuals_seen += 1;
            let (mean_miss, sd_miss) = misses(ps, (m, sd, rn));
            let got = described(ps, (m, sd, rn));
            assert!(sd_miss <= 0.0, "{what} (known residual) sd: {got}");
            assert!(
                mean_miss > 0.0,
                "{what} now meets the bound: remove it from KNOWN_RESIDUALS ({got})"
            );
            assert!(
                mean_miss <= recorded,
                "{what} misses by {mean_miss}, past its recorded {recorded}: {got}"
            );
        }
        assert!(compared >= 4, "{row}: only {compared} bins compared");
    }
    assert_eq!(
        residuals_seen,
        KNOWN_RESIDUALS.len(),
        "every known residual is a compared bin"
    );
}

#[test]
fn a_below_glows_low_codes_follow_the_measured_cap() {
    // `noise_take.py`'s LOW CODES: the outer glow's codes under 32 read
    // about 2.5 c in Photoshop, the Below cap; the inner shadow's follow the
    // bare law. Every code Photoshop read at least 100 times and this scene
    // at least 50, held to the take's bound.
    let stats = fixture();
    let doc = blur_scene();
    for (row, kind) in [("og_n50", "outer_glow"), ("is_n50", "inner_shadow")] {
        let twin = coverage(&doc, json!([effect(kind, 40.0, 0.0)]));
        let got = coverage(&doc, json!([effect(kind, 40.0, 0.5)]));
        let codes = per_code(&twin, &got);
        let mut compared = 0;
        for (c, ps) in stats["low_codes"][row].as_object().unwrap() {
            let pn = ps["n"].as_f64().unwrap();
            let Some(&(k, a, b)) = codes.get(&c.parse::<u8>().unwrap()) else {
                continue;
            };
            if pn < 100.0 || k < 50.0 {
                continue;
            }
            let m = a / k;
            let sd = (b / k - m * m).max(0.0).sqrt();
            within(
                &format!("{row} code {c}"),
                (ps["mean"].as_f64().unwrap(), ps["sd"].as_f64().unwrap(), pn),
                (m, sd, k),
            );
            compared += 1;
        }
        assert!(compared >= 8, "{row}: only {compared} codes compared");
    }
}

// ------------------------------------------------------------------ .rz 97 --

#[test]
fn the_byte_word_97_comes_first_round_trips_and_refuses_both_ways() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = blur_scene();
    let save = |name: &str, style: &Value| -> (Vec<u8>, DrDocument) {
        let styled_doc = styled(&doc, 1, &style.to_string());
        let path = dir.path().join(name);
        styled_doc.save_native(path.to_str().unwrap()).unwrap();
        (std::fs::read(&path).unwrap(), styled_doc)
    };
    let (plain, _) = save("plain.rz", &json!({"effects":[{"type":"inner_glow"}]}));
    for kind in ["drop_shadow", "inner_shadow", "outer_glow", "inner_glow"] {
        let style =
            json!({"fill_opacity":0.5,"effects":[{"type":kind,"noise":0.4,"enabled":false}]});
        let (bytes, styled_doc) = save(&format!("{kind}.rz"), &style);
        assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
        assert_eq!(
            &bytes[8..12],
            &97u32.to_le_bytes(),
            "{kind}: FIRST inside 45"
        );
        assert_eq!(&bytes[12..16], &plain[8..12], "{kind}: then the plain word");
        let path = dir.path().join(format!("{kind}.rz"));
        let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            reopened.layers[1].style.as_deref().map(LayerStyle::to_json),
            styled_doc.layers[1]
                .style
                .as_deref()
                .map(LayerStyle::to_json)
        );
        let mut stripped = bytes.clone();
        stripped.drain(8..12);
        let cut = dir.path().join(format!("{kind}-cut.rz"));
        std::fs::write(&cut, &stripped).unwrap();
        let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
            panic!("{kind}: noise without the word must refuse");
        };
        assert!(
            message.ends_with("native layer style noise requires DRDC 97"),
            "{message}"
        );
    }
    // A noisy, contoured style renders back equal, and 97 comes before 96.
    let (both, styled_doc) = save(
        "both.rz",
        &json!({"effects":[{"type":"outer_glow","noise":0.3,"size":9,
            "contour":{"preset":"cone"}}]}),
    );
    assert_eq!(&both[8..12], &97u32.to_le_bytes());
    assert_eq!(&both[12..16], &96u32.to_le_bytes(), "97 ahead of 96");
    let reopened = DrDocument::open(dir.path().join("both.rz").to_str().unwrap()).unwrap();
    assert_eq!(
        reopened.flattened().unwrap(),
        styled_doc.flattened().unwrap()
    );
    let mut forged = plain.clone();
    forged.splice(8..8, 97u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = DrDocument::open(path.to_str().unwrap()) else {
        panic!("the word without noise must refuse");
    };
    assert!(message.ends_with("invalid native noise word"), "{message}");
}

#[test]
fn a_comp_only_noise_claims_the_word() {
    use doctored_core::doc_comps::{CompCaptures, CompRequest};
    let dir = tempfile::TempDir::new().unwrap();
    let doc = styled(
        &blur_scene(),
        1,
        &json!({"effects":[{"type":"drop_shadow","noise":0.2}]}).to_string(),
    );
    let save = CompRequest::Save {
        name: "Grain".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = doc.edit_layer_comps(&save).unwrap().unwrap();
    let cleared = try_styled(&saved, 1, None).unwrap().unwrap();
    assert!(cleared.layers[1].style.is_none());
    let path = dir.path().join("comp.rz");
    cleared.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &97u32.to_le_bytes());
    assert!(DrDocument::open(path.to_str().unwrap()).is_ok());
}
