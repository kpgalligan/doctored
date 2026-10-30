//! Sky Replacement's edge decontamination through the FFI:
//! `"decontaminate": true` on `dr_doc_sky_replace` /
//! `dr_precision_doc_sky_replace` (both outputs) and the preview
//! `dr_image_sky_replace`, plus the plan's `decontaminate` argument, at 8
//! and 16 bits.
//!
//! # The scene and its oracles
//!
//! An opaque 64×48 picture: rows 0–19 the old sky `B`, rows 20–27 a ramp
//! of foreground fraction `α = (y − 19)/9` with colour `C = α·F + (1 − α)·B`
//! and model coverage `m0 = round((1 − α)·65535)`, rows 28 on the
//! foreground `F` at coverage 0, except row 40, x 0–7: `F` at coverage 500
//! (a low-coverage foreground sample). The new sky is the uniform dark `K`;
//! lighting, colour and edge lighting are 0, so with Shift and Fade at 0 the
//! correct Duplicate pixel is the closed form `m0·K + (1 − m0)·F`: the
//! foreground the ramp was mixed from, recovered.
//!
//! # Bounds, declared before the first run
//!
//! * Duplicate pixel: ±2 codes at 8 bits, ±8 at 16 (the source's
//!   quantization divided by `α` in the solve, scaled back by `1 − m0`).
//! * Foreground Edge pixel against the stored `F`: ±3 codes at 8 bits, ±12
//!   at 16 (the same error without the `1 − m0` scale, at `α ≥ 1/9`).
//! * No halo: no output luma (Rec.709, `blend::LUMA_*`) more than 2 8-bit
//!   codes above the brighter of `K` and `F`; without decontamination some
//!   pixel is more than 20 codes above (the halo the scene is built to show).
//! * Group against Duplicate: M2b's bounds (`sky_replacement_group.rs`),
//!   ±3 codes at 8 bits and ±6 at 16.
//! * Old Actions, repeat runs, the preview against the byte doc op and the
//!   round trips: exact.
#![allow(clippy::needless_range_loop)]

use doctored_core::doc::BlendMode;
use doctored_core::ffi_sky::dr_sky_replace_scratch_bytes;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::layer_bytes;
use crate::sky_replacement::{
    build, byte_image, layer_count, preview, replace, uniform_sky, Depth, Doc, H, W,
};
use crate::sky_replacement_group::{
    duplicate, entries, gradient_sky, new_layers, projection, save_psd, save_rz, sky_samples,
};

const B: [f64; 3] = [0.95, 0.93, 0.90];
const F: [f64; 3] = [0.10, 0.15, 0.08];
const K: [u8; 4] = [10, 12, 30, 255];
const EDGE: &str = "Foreground Edge";
const CHILDREN: [&str; 3] = ["Foreground Color", "Foreground Lighting", "Sky"];
/// The ramp's rows: the edge band at Fade and Shift 0.
const RAMP: std::ops::RangeInclusive<u32> = 20..=27;

/// The scene's pixels and coverage (module doc).
fn scene() -> (Vec<[f64; 4]>, Vec<u16>) {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let (c, m0) = if y < 20 {
                (B, 65535)
            } else if RAMP.contains(&y) {
                let a = f64::from(y - 19) / 9.0;
                let c = std::array::from_fn(|k| a * F[k] + (1.0 - a) * B[k]);
                (c, ((1.0 - a) * 65535.0).round() as u16)
            } else if y == 40 && x < 8 {
                (F, 500)
            } else {
                (F, 0)
            };
            ([c[0], c[1], c[2], 1.0], m0)
        })
        .unzip()
}

/// Shift and Fade 0, lighting, colour and edge lighting 0, then `extra`.
fn base(extra: Value) -> Value {
    let mut v = json!({"fade_edge": 0, "shift_edge": 0, "lighting": 0,
                       "color_adjustment": 0, "edge_lighting": 0});
    for (k, value) in extra.as_object().unwrap() {
        v[k] = value.clone();
    }
    v
}

fn on(p: Value) -> Value {
    let mut v = p;
    v["decontaminate"] = json!(true);
    v
}

fn stored(depth: Depth, v: f64) -> f64 {
    (v.clamp(0.0, 1.0) * depth.max()).round() / depth.max()
}

/// Layer `at`'s samples (canvas-sized at (0, 0)) as unit values.
fn unit(doc: &Doc, depth: Depth, at: usize) -> Vec<f64> {
    sky_samples(doc, at)
        .iter()
        .map(|&v| f64::from(v) / depth.max())
        .collect()
}

/// Rec.709 luma, the weights of `blend::LUMA_*`.
fn luma(c: &[f64]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn k_unit() -> [f64; 3] {
    [K[0], K[1], K[2]].map(|v| f64::from(v) / 255.0)
}

/// Every fact of two documents: entries, projection and each raster's
/// samples.
fn assert_same(a: &Doc, b: &Doc, what: &str) {
    let (ea, eb) = (entries(a), entries(b));
    assert_eq!(ea, eb, "{what}: entries");
    assert_eq!(projection(a), projection(b), "{what}: projection");
    for (at, entry) in ea.iter().enumerate() {
        if entry.kind == "raster" {
            assert_eq!(
                sky_samples(a, at),
                sky_samples(b, at),
                "{what}: {}",
                entry.name
            );
        }
    }
}

// ------------------------------------------------------------- recovery --

#[test]
fn decontamination_recovers_the_foreground_of_the_edge() {
    let (pixels, m0) = scene();
    let sky = uniform_sky(K);
    let k = k_unit();
    for depth in [Depth::Byte, Depth::Sixteen] {
        let (flat_bound, edge_bound) = match depth {
            Depth::Byte => (2.0, 3.0),
            Depth::Sixteen => (8.0, 12.0),
        };
        let doc = build(depth, W, H, std::slice::from_ref(&pixels));
        let p = on(base(json!({})));
        let flat = replace(&doc, 0, &sky, &m0, &duplicate(p.clone())).unwrap();
        let out = unit(&flat, depth, 1);
        for i in 0..m0.len() {
            let m = f64::from(m0[i]) / 65535.0;
            for c in 0..3 {
                let want = m * k[c] + (1.0 - m) * F[c];
                let diff = (out[i * 4 + c] - want).abs() * depth.max();
                assert!(
                    diff <= flat_bound + 1e-6,
                    "{depth:?} pixel {i} channel {c}: {diff} codes"
                );
            }
        }
        let grouped = replace(&doc, 0, &sky, &m0, &new_layers(p)).unwrap();
        let all = entries(&grouped);
        assert_eq!(all[1].name, EDGE);
        let mask = all[1].mask.as_ref().unwrap();
        let edge = unit(&grouped, depth, 1);
        let f = F.map(|v| stored(depth, v));
        let mut inside = 0;
        for i in 0..mask.len() {
            if mask[i] == 0 {
                continue;
            }
            inside += 1;
            for c in 0..3 {
                let diff = (edge[i * 4 + c] - f[c]).abs() * depth.max();
                assert!(
                    diff <= edge_bound + 1e-6,
                    "{depth:?} edge pixel {i} channel {c}: {diff} codes"
                );
            }
            assert_eq!(edge[i * 4 + 3], 1.0, "the source's alpha");
        }
        assert_eq!(inside, 8 * W as usize, "{depth:?}: the ramp's rows");
    }
}

#[test]
fn foreground_samples_are_never_solved() {
    let (pixels, m0) = scene();
    let sky = uniform_sky(K);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(depth, W, H, std::slice::from_ref(&pixels));
        let p = base(json!({}));
        let off = replace(&doc, 0, &sky, &m0, &duplicate(p.clone())).unwrap();
        let with = replace(&doc, 0, &sky, &m0, &duplicate(on(p.clone()))).unwrap();
        let grouped = replace(&doc, 0, &sky, &m0, &new_layers(on(p))).unwrap();
        let (a, b) = (sky_samples(&off, 1), sky_samples(&with, 1));
        let mask = entries(&grouped)[1].mask.clone().unwrap();
        for x in 0..8usize {
            let i = 40 * W as usize + x;
            assert_eq!(m0[i], 500);
            assert_eq!(a[i * 4..i * 4 + 4], b[i * 4..i * 4 + 4], "{depth:?} x {x}");
            assert_eq!(mask[i], 0, "{depth:?} x {x}: outside the edge layer");
        }
    }
}

#[test]
fn a_negative_shift_edge_keeps_the_sources_own_colour() {
    let (pixels, m0) = scene();
    let sky = uniform_sky(K);
    let depth = Depth::Sixteen;
    let doc = build(depth, W, H, &[pixels]);
    let p = base(json!({"shift_edge": -100}));
    let off = replace(&doc, 0, &sky, &m0, &duplicate(p.clone())).unwrap();
    let with = replace(&doc, 0, &sky, &m0, &duplicate(on(p.clone()))).unwrap();
    let grouped = replace(&doc, 0, &sky, &m0, &new_layers(on(p))).unwrap();
    let all = entries(&grouped);
    assert_eq!(all[4].name, "Sky");
    // With Edge Lighting 0 the Sky's mask is the refined mask `m`.
    let (sky_mask, edge_mask) = (all[4].mask.clone().unwrap(), all[1].mask.clone().unwrap());
    let (a, b) = (sky_samples(&off, 1), sky_samples(&with, 1));
    let mut strip = 0;
    for i in 0..m0.len() {
        if sky_mask[i] == 0 && m0[i] > 0 {
            strip += 1;
            assert_eq!(a[i * 4..i * 4 + 4], b[i * 4..i * 4 + 4], "pixel {i}");
            assert_eq!(edge_mask[i], 0, "pixel {i}: outside the edge layer");
        }
    }
    assert!(strip >= 64, "the shifted strip holds {strip} pixels");
}

#[test]
fn a_dark_sky_shows_no_halo() {
    let (pixels, m0) = scene();
    let sky = uniform_sky(K);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(depth, W, H, std::slice::from_ref(&pixels));
        let p = base(json!({"fade_edge": 100}));
        let f = F.map(|v| stored(depth, v));
        let ceiling = luma(&k_unit()).max(luma(&f));
        let excess = |q: &Value| {
            let out = replace(&doc, 0, &sky, &m0, &duplicate(q.clone())).unwrap();
            unit(&out, depth, 1)
                .chunks(4)
                .map(|px| (luma(px) - ceiling) * 255.0)
                .fold(f64::MIN, f64::max)
        };
        let with = excess(&on(p.clone()));
        assert!(with <= 2.0 + 1e-6, "{depth:?}: {with} codes above");
        let without = excess(&p);
        assert!(without > 20.0, "{depth:?}: the halo is {without} codes");
    }
}

#[test]
fn the_group_with_its_edge_composites_to_the_duplicate_layer_output() {
    let (pixels, m0) = scene();
    let skies = [
        ("warm", gradient_sky([250, 150, 60], [255, 200, 120])),
        ("cool", gradient_sky([30, 70, 200], [90, 140, 230])),
    ];
    for depth in [Depth::Byte, Depth::Sixteen] {
        let bound = match depth {
            Depth::Byte => 3.0,
            Depth::Sixteen => 6.0,
        };
        let doc = build(depth, W, H, std::slice::from_ref(&pixels));
        for (what, sky) in &skies {
            for mode in ["multiply", "screen"] {
                let p = on(
                    json!({"fade_edge": 3, "lighting": 50, "color_adjustment": 100,
                                  "edge_lighting": 50, "lighting_mode": mode}),
                );
                let flat = projection(&replace(&doc, 0, sky, &m0, &duplicate(p.clone())).unwrap());
                let grouped = replace(&doc, 0, sky, &m0, &new_layers(p)).unwrap();
                assert_eq!(layer_count(&grouped), 6);
                let grouped = projection(&grouped);
                let worst = flat
                    .iter()
                    .zip(&grouped)
                    .map(|(a, b)| (a - b).abs() * depth.max())
                    .fold(0.0, f64::max);
                assert!(
                    worst <= bound + 1e-6,
                    "{depth:?} {what} {mode}: {worst} codes past {bound}"
                );
            }
        }
    }
}

// ---------------------------------------------- structure and round trips --

fn edged(depth: Depth) -> Doc {
    let (pixels, m0) = scene();
    let doc = build(depth, W, H, &[pixels]);
    let p = on(
        json!({"fade_edge": 0, "lighting_mode": "screen", "lighting": 60,
                      "color_adjustment": 80, "edge_lighting": 50}),
    );
    replace(
        &doc,
        0,
        &gradient_sky([30, 70, 200], [90, 140, 230]),
        &m0,
        &new_layers(p),
    )
    .unwrap()
}

#[test]
fn the_edge_layer_is_the_groups_first_child_masked_to_the_band() {
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = edged(depth);
        assert_eq!(layer_count(&doc), 6, "{depth:?}");
        let all = entries(&doc);
        let edge = &all[1];
        assert_eq!(
            (
                edge.name.as_str(),
                edge.kind,
                edge.blend,
                edge.opacity,
                edge.clipped
            ),
            (EDGE, "raster", BlendMode::Normal, 1.0, false),
            "{depth:?}"
        );
        assert!(edge.curves.is_none());
        let mask = edge.mask.as_ref().unwrap();
        for (i, &v) in mask.iter().enumerate() {
            let y = i as u32 / W;
            let want = if RAMP.contains(&y) { 65535 } else { 0 };
            assert_eq!(v, want, "{depth:?} mask at {i}");
        }
        for (k, name) in CHILDREN.iter().enumerate() {
            assert_eq!(all[2 + k].name, *name);
        }
        assert_eq!(all[5].kind, "group");
        match &doc {
            Doc::Byte(d) => {
                assert_eq!(d.layers[1].offset, (0, 0));
                assert_eq!(d.layers[1].pixels.dimensions(), (W, H));
                for at in 1..=4 {
                    assert_eq!(d.layers[at].depth, d.layers[5].depth + 1);
                }
            }
            Doc::Typed(d) => {
                assert_eq!(d.layer(5).unwrap().children().unwrap().len(), 4);
                let l = d.layer(1).unwrap();
                assert_eq!(l.offset(), (0, 0));
                assert_eq!(l.source().unwrap().dimensions(), (W, H));
            }
        }
        // Outside the band the layer is transparent.
        let samples = sky_samples(&doc, 1);
        for (i, &v) in mask.iter().enumerate() {
            if v == 0 {
                assert_eq!(samples[i * 4 + 3], 0, "{depth:?} alpha at {i}");
            }
        }
    }
}

#[test]
fn the_edge_layer_survives_rz_and_psd() {
    let dir = TempDir::new().unwrap();
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = edged(depth);
        let back = save_rz(&doc, &dir.path().join(format!("{depth:?}.rz")));
        assert_eq!(entries(&back), entries(&doc), "{depth:?} .doctored");
        assert_eq!(
            sky_samples(&back, 1),
            sky_samples(&doc, 1),
            "{depth:?} .doctored"
        );
        let back = save_psd(&doc, &dir.path().join(format!("{depth:?}.psd")));
        let (before, after) = (entries(&doc), entries(&back));
        assert_eq!(after.len(), before.len(), "{depth:?} PSD");
        for (a, b) in after.iter().zip(&before) {
            assert_eq!(
                (&a.name, a.kind, a.blend, a.clipped, &a.mask),
                (&b.name, b.kind, b.blend, b.clipped, &b.mask),
                "{depth:?} PSD {}",
                b.name
            );
            assert!((a.opacity - b.opacity).abs() <= 0.5 / 255.0);
        }
        assert_eq!(sky_samples(&back, 1), sky_samples(&doc, 1), "{depth:?} PSD");
    }
}

// ------------------------------------------------------------ old Actions --

#[test]
fn an_absent_key_is_off_and_every_run_is_deterministic() {
    let (pixels, m0) = scene();
    let sky = gradient_sky([250, 150, 60], [255, 200, 120]);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(depth, W, H, std::slice::from_ref(&pixels));
        for output in [duplicate as fn(Value) -> Value, new_layers] {
            let absent = replace(&doc, 0, &sky, &m0, &output(json!({}))).unwrap();
            let off = output(json!({"decontaminate": false}));
            let off = replace(&doc, 0, &sky, &m0, &off).unwrap();
            assert_same(&absent, &off, &format!("{depth:?} absent vs off"));
            let first = replace(&doc, 0, &sky, &m0, &output(on(json!({})))).unwrap();
            let second = replace(&doc, 0, &sky, &m0, &output(on(json!({})))).unwrap();
            assert_same(&first, &second, &format!("{depth:?} two runs"));
        }
        let grouped = replace(&doc, 0, &sky, &m0, &new_layers(json!({}))).unwrap();
        assert_eq!(layer_count(&grouped), 5, "{depth:?}: three children, off");
        assert_eq!(
            replace(&doc, 0, &sky, &m0, &json!({"decontaminate": 1}))
                .err()
                .unwrap(),
            "sky replacement decontaminate must be a boolean"
        );
    }
    // The preview with it on is the byte doc op's layer, byte for byte.
    let p = on(json!({}));
    let doc = build(Depth::Byte, W, H, std::slice::from_ref(&pixels));
    let Doc::Byte(out) = replace(&doc, 0, &sky, &m0, &duplicate(p.clone())).unwrap() else {
        unreachable!()
    };
    let previewed = preview(&byte_image(W, H, &pixels), &sky, &m0, (W, H), &p).unwrap();
    assert_eq!(layer_bytes(&out, 1), previewed);
}

// ------------------------------------------------------------------ plan --

#[test]
fn the_edge_layer_is_planned() {
    let mib4 = 4 * 1024 * 1024;
    let plan = |bits, output, decontaminate| unsafe {
        dr_sky_replace_scratch_bytes(100, 50, 20, 10, bits, output, decontaminate)
    };
    assert_eq!(plan(8, 1, true), 5000 * (57 + 12) + 200 * 36 + mib4);
    assert_eq!(plan(16, 1, true), 5000 * (57 + 24) + 200 * 36 + mib4);
    assert_eq!(plan(8, 1, false), 5000 * (57 + 7) + 200 * 36 + mib4);
    for bits in [8, 16, 32] {
        assert_eq!(plan(bits, 0, true), plan(bits, 0, false), "{bits}");
    }
    assert_eq!(plan(32, 1, true), 0);
}
