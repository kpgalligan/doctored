//! Bevel & Emboss Technique (#2): Smooth, Chisel Hard and Chisel Soft —
//! black-box through the style FFI, the typed edit and the projection.
//!
//! The Chisel oracle restates the module doc's height from an independent
//! brute-force distance (every pixel's Euclidean distance between pixel
//! centres to the nearest pixel of the other side, membership at coverage
//! ≥ 128): inside `di = clamp(d / size)`, outside `do = clamp(d / size)`;
//! inner `di`, outer `1 − do`, emboss `(1 + di − do) / 2`, pillow
//! `(1 − di − do) / 2`, stored as `round(255 h)`. Chisel Soft blurs that
//! byte plane with the header's feather contract (`common::feathered_plane`)
//! at `sigma = size · r / 2`, r the shipped `CHISEL_SOFT_RATIO`. The shading,
//! the bands and the W3C composite are `style_bevel_emboss.rs`'s, restated;
//! tolerances are that file's ±2 per colour channel, alpha exact; the
//! highlight's and shadow's 75 % opacity is its byte
//! (`common::byte_unit`).
//!
//! The Chisel law's shipped arms (#2 bevel phase 2, `style_bevel_chisel`:
//! Pillow Emboss, Hard and Soft) have their own oracle, restated from the
//! law and sharing nothing with the core: the same brute-force distance over
//! the facet `S_f = size / 2`, `m = (1 + di − do) / 2` in f64; Chisel Soft
//! feathers `m` with the header's contract (`common::feather_kernel`, clamped
//! reads) at `sigma = size · r / 2`; the reflection `m·a + (1 − m)(1 − a)`;
//! Soften as the separable triangle `(W − |j|) / W²` of `W = pair_width(soften)`
//! on that height; `hx = gain · (h[x+1] − h[x−1]) / 2` with `gain = 2 k ·
//! depth · W_f`, `k = (W_f − delta) / (2 W_f)`, `W_f = pair_width(S_f)`, delta
//! the runner's 0.86 (Hard) and 0.48 (Soft); the NORMALIZED law; no Soften on
//! the shading.
//!
//! The depths: every depth renders the bevel through the one u8 plane
//! renderer, so a Chisel bevel at 16 or 32 bits is the byte document's
//! within one 8-bit code (white highlight Normal over black, shadow off: the
//! red IS the code — `R / 257` at 16 bits, `255 R` at 32).

use std::ffi::{c_char, c_int, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_painting_layer};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::{BevelTechnique, Effect, LayerStyle};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_structure_fixture::free;
use crate::cmyk_style_fixture::{backdrop, flat, hard_rect, layer, styled as cmyk_styled};
use crate::common::*;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// `style_fx_bevel_emboss::CHISEL_SOFT_RATIO`, restated: the r the #2 take
/// judged (`bevel_chisel_578_take.py`; 0.5 was the declared value before it).
const CHISEL_SOFT_RATIO: f32 = 0.1;

/// `style_bevel_chisel::CHISEL_HARD_DELTA` / `CHISEL_SOFT_DELTA`, restated: the
/// runner's FIT (`bevel_578_law_fit.py --judge-chisel-take3`).
const CHISEL_HARD_DELTA: f64 = 0.86;
const CHISEL_SOFT_DELTA: f64 = 0.48;

// --------------------------------------------------------------- fixture --

const GRAY: [u8; 4] = [128, 128, 128, 255];
const RECT_COLOR: [u8; 4] = [120, 160, 200, 255];
const CANVAS: (u32, u32) = (60, 50);
/// 24×20 at (18, 15): 15 px or more of margin on every side, more than a
/// size-6 Chisel Soft reaches (6 px of ramp plus a sigma-1.5 blur), so the
/// oracle's clamp-to-edge feather equals the core's zero-padded plane.
const RECT: (i32, i32, u32, u32) = (18, 15, 24, 20);
const STYLES: [&str; 4] = ["inner_bevel", "outer_bevel", "emboss", "pillow_emboss"];

fn gray_doc() -> DrDocument {
    let doc = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, GRAY));
    let doc = doc
        .adding_image_layer(0, solid(RECT.2, RECT.3, RECT_COLOR), "Rect")
        .expect("add rect layer");
    doc.with_layer_offset(1, RECT.0, RECT.1).expect("offset")
}

fn in_rect(x: i64, y: i64) -> bool {
    x >= i64::from(RECT.0)
        && y >= i64::from(RECT.1)
        && x < i64::from(RECT.0) + i64::from(RECT.2)
        && y < i64::from(RECT.1) + i64::from(RECT.3)
}

/// The Doctored bevel's knobs, light explicit (angle 0 = from the right).
#[derive(Clone, Copy)]
struct Bevel {
    style: &'static str,
    technique: &'static str,
    size: f32,
    soften: f32,
    depth: f32,
    down: bool,
}

impl Bevel {
    fn new(style: &'static str, technique: &'static str) -> Bevel {
        Bevel {
            style,
            technique,
            size: 6.0,
            soften: 0.0,
            depth: 1.0,
            down: false,
        }
    }

    fn effect(&self) -> Value {
        json!({"type":"bevel_emboss","style":self.style,"technique":self.technique,
            "depth":self.depth,"direction":if self.down {"down"} else {"up"},"size":self.size,
            "soften":self.soften,"angle":0,"use_global_light":false,"altitude":30,
            "highlight_blend":"screen","highlight_color":"#ffffff","highlight_opacity":0.75,
            "shadow_blend":"multiply","shadow_color":"#000000","shadow_opacity":0.75})
    }

    fn json(&self) -> String {
        json!({"effects":[self.effect()]}).to_string()
    }
}

// --------------------------------------------------------------- oracle --

/// The feather radius for a Photoshop size (sigma = size / 2, the header's
/// `sigma = 0.3 (r − 1) + 0.8` inverted, floored at 0.5).
fn radius_for(size: f32) -> f32 {
    let sigma = size / 2.0;
    if sigma <= 0.0 {
        0.0
    } else {
        ((sigma - 0.8) / 0.3 + 1.0).max(0.5)
    }
}

/// Brute force: each pixel's distance to the nearest pixel of the other
/// side (`f32::INFINITY` when there is none).
fn opposite_distance(sel: &[u8], w: u32, h: u32) -> Vec<f32> {
    let (w, h) = (w as i64, h as i64);
    let inside = |i: i64| sel[i as usize] >= 128;
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let mut best = f32::INFINITY;
            for j in 0..w * h {
                if inside(j) != inside(i) {
                    let (dx, dy) = ((j % w - x) as f32, (j / w - y) as f32);
                    best = best.min((dx * dx + dy * dy).sqrt());
                }
            }
            best
        })
        .collect()
}

/// The Chisel Hard height bytes (module doc).
fn chisel_hard(sel: &[u8], w: u32, h: u32, b: &Bevel) -> Vec<u8> {
    let d = opposite_distance(sel, w, h);
    sel.iter()
        .zip(&d)
        .map(|(&c, &d)| {
            let t = (d / b.size).clamp(0.0, 1.0);
            let (di, dout) = if c >= 128 { (t, 0.0) } else { (0.0, t) };
            let unit = match b.style {
                "inner_bevel" => di,
                "outer_bevel" => 1.0 - dout,
                "emboss" => (1.0 + di - dout) / 2.0,
                "pillow_emboss" => (1.0 - di - dout) / 2.0,
                other => panic!("unknown style {other}"),
            };
            (255.0 * unit).round() as u8
        })
        .collect()
}

/// The technique's height, then the soften blur.
fn height(sel: &[u8], w: u32, h: u32, b: &Bevel) -> Vec<u8> {
    let hard = chisel_hard(sel, w, h, b);
    let shaped = match b.technique {
        "chisel_hard" => hard,
        "chisel_soft" => feathered_plane(&hard, w, h, radius_for(b.size * CHISEL_SOFT_RATIO)),
        other => panic!("no Chisel oracle for {other}"),
    };
    feathered_plane(&shaped, w, h, radius_for(b.soften))
}

/// `style_bevel_emboss.rs`'s shading restated: central differences over
/// `s = max(1, round(size / 8))`, `n = normalize(−depth·size·hx, …, 1)`,
/// `d = n.x L.x + n.y L.y`, negated for direction down.
fn shading(height: &[u8], w: u32, h: u32, b: &Bevel) -> Vec<f32> {
    let (wi, hi) = (i64::from(w), i64::from(h));
    let at =
        |x: i64, y: i64| f32::from(height[(y.clamp(0, hi - 1) * wi + x.clamp(0, wi - 1)) as usize]);
    let s = (b.size / 8.0).round().max(1.0) as i64;
    let k = b.depth * b.size;
    let alt = 30f32.to_radians();
    let l = (alt.cos(), 0.0f32);
    let mut out = Vec::with_capacity(height.len());
    for y in 0..hi {
        for x in 0..wi {
            let hx = (at(x + s, y) - at(x - s, y)) / (2.0 * s as f32 * 255.0);
            let hy = (at(x, y + s) - at(x, y - s)) / (2.0 * s as f32 * 255.0);
            let (nx, ny) = (-k * hx, -k * hy);
            let len = (nx * nx + ny * ny + 1.0).sqrt();
            let d = (nx / len) * l.0 + (ny / len) * l.1;
            out.push(if b.down { -d } else { d });
        }
    }
    out
}

fn coverage(d: f32) -> u8 {
    (255.0 * d.clamp(0.0, 1.0)).round() as u8
}

/// Whether the Chisel law renders `b` (module doc): its shipped arms only.
fn law_arm(b: &Bevel) -> bool {
    b.style == "pillow_emboss" && matches!(b.technique, "chisel_hard" | "chisel_soft")
}

/// The box pair's width for a size or soften of `v` px: 0 for none, else `v`
/// rounded half up, at least 1, and 2 where that is 1.
fn pair_width(v: f32) -> usize {
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    match (f64::from(v) + 0.5).floor().max(1.0) as usize {
        1 => 2,
        s => s,
    }
}

/// The separable triangle `(width − |j|) / width²` over `plane`, x then y,
/// reads clamped to the plane; the plane itself at width 0.
fn triangle(plane: &[f64], w: u32, h: u32, width: usize) -> Vec<f64> {
    if width == 0 {
        return plane.to_vec();
    }
    let (wi, hi, n) = (i64::from(w), i64::from(h), width as i64);
    let taps: Vec<(i64, f64)> = (1 - n..n)
        .map(|j| (j, (n - j.abs()) as f64 / (n * n) as f64))
        .collect();
    let mut across = vec![0.0; plane.len()];
    for y in 0..hi {
        for x in 0..wi {
            across[(y * wi + x) as usize] = taps
                .iter()
                .map(|&(j, k)| k * plane[(y * wi + (x + j).clamp(0, wi - 1)) as usize])
                .sum();
        }
    }
    let mut out = vec![0.0; plane.len()];
    for y in 0..hi {
        for x in 0..wi {
            out[(y * wi + x) as usize] = taps
                .iter()
                .map(|&(j, k)| k * across[((y + j).clamp(0, hi - 1) * wi + x) as usize])
                .sum();
        }
    }
    out
}

/// The header's feather (`common::feather_kernel`) of an f64 plane, x then
/// y, reads clamped to the plane.
fn feathered_unit(plane: &[f64], w: u32, h: u32, radius: f32) -> Vec<f64> {
    if radius <= 0.0 {
        return plane.to_vec();
    }
    let (k, half) = feather_kernel(radius);
    let (wi, hi) = (i64::from(w), i64::from(h));
    let pass = |src: &[f64], along_x: bool| -> Vec<f64> {
        let mut out = vec![0.0; src.len()];
        for y in 0..hi {
            for x in 0..wi {
                out[(y * wi + x) as usize] = k
                    .iter()
                    .enumerate()
                    .map(|(i, &kv)| {
                        let j = i as i64 - half;
                        let at = if along_x {
                            y * wi + (x + j).clamp(0, wi - 1)
                        } else {
                            (y + j).clamp(0, hi - 1) * wi + x
                        };
                        f64::from(kv) * src[at as usize]
                    })
                    .sum();
            }
        }
        out
    };
    pass(&pass(plane, true), false)
}

/// The NORMALIZED law for slopes (`hx`, `hy`) under `l` (module doc).
fn normalized(hx: f64, hy: f64, l: (f64, f64, f64)) -> f64 {
    let nl = (-hx * l.0 - hy * l.1 + l.2) / (1.0 + hx * hx + hy * hy).sqrt();
    if nl >= l.2 {
        if l.2 < 1.0 {
            (nl - l.2) / (1.0 - l.2)
        } else {
            0.0
        }
    } else if l.2 > 0.0 {
        (nl - l.2) / l.2
    } else {
        0.0
    }
}

/// The Chisel law's shading `d` per pixel for a shipped arm (module doc),
/// under the fixture's light (angle 0, altitude 30).
fn law_shading(sel: &[u8], w: u32, h: u32, b: &Bevel) -> Vec<f32> {
    let s_f = b.size / 2.0;
    let d = opposite_distance(sel, w, h);
    let mut m: Vec<f64> = sel
        .iter()
        .zip(&d)
        .map(|(&c, &d)| {
            let t = (f64::from(d) / f64::from(s_f)).clamp(0.0, 1.0);
            let (di, dout) = if c >= 128 { (t, 0.0) } else { (0.0, t) };
            (1.0 + di - dout) / 2.0
        })
        .collect();
    let delta = if b.technique == "chisel_soft" {
        m = feathered_unit(&m, w, h, radius_for(b.size * CHISEL_SOFT_RATIO));
        CHISEL_SOFT_DELTA
    } else {
        CHISEL_HARD_DELTA
    };
    let reflected: Vec<f64> = m
        .iter()
        .zip(sel)
        .map(|(&m, &c)| {
            let a = f64::from(c) / 255.0;
            m * a + (1.0 - m) * (1.0 - a)
        })
        .collect();
    let height = triangle(&reflected, w, h, pair_width(b.soften));
    let w_f = pair_width(s_f) as f64;
    let k = (w_f - delta) / (2.0 * w_f);
    let gain = 2.0 * k * f64::from(b.depth) * w_f;
    let (wi, hi) = (i64::from(w), i64::from(h));
    let at = |x: i64, y: i64| height[(y.clamp(0, hi - 1) * wi + x.clamp(0, wi - 1)) as usize];
    let sign = if b.down { -1.0 } else { 1.0 };
    let alt = 30f64.to_radians();
    let l = (alt.cos(), 0.0, alt.sin());
    let mut out = Vec::with_capacity(sel.len());
    for y in 0..hi {
        for x in 0..wi {
            let hx = (at(x + 1, y) - at(x - 1, y)) / 2.0 * gain * sign;
            let hy = (at(x, y + 1) - at(x, y - 1)) / 2.0 * gain * sign;
            out.push(normalized(hx, hy, l) as f32);
        }
    }
    out
}

/// `round(a · b / 255)` per pixel.
fn mul(a: &[u8], b: &[u8]) -> Vec<u8> {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| ((u32::from(x) * u32::from(y) + 127) / 255) as u8)
        .collect()
}

/// The oracle projection: below halves, the rect, the inner halves — every
/// step the W3C reference composite, quantized once.
fn expected(b: &Bevel) -> RgbaImage {
    let (w, h) = CANVAS;
    let sel = selection(w, h, |x, y| {
        if in_rect(i64::from(x), i64::from(y)) {
            255
        } else {
            0
        }
    });
    let d = if law_arm(b) {
        law_shading(&sel, w, h, b)
    } else {
        shading(&height(&sel, w, h, b), w, h, b)
    };
    let hl: Vec<u8> = d.iter().map(|&v| coverage(v)).collect();
    let sh: Vec<u8> = d.iter().map(|&v| coverage(-v)).collect();
    let outside: Vec<u8> = sel.iter().map(|&v| 255 - v).collect();
    let zero = vec![0u8; sel.len()];
    let (inner, outer) = match b.style {
        "inner_bevel" => (true, false),
        "outer_bevel" => (false, true),
        _ => (true, true),
    };
    let pick = |on: bool, plane: &[u8], band: &[u8]| {
        if on {
            mul(plane, band)
        } else {
            zero.clone()
        }
    };
    let (o_hl, o_sh) = (pick(outer, &hl, &outside), pick(outer, &sh, &outside));
    let (i_hl, i_sh) = (pick(inner, &hl, &sel), pick(inner, &sh, &sel));
    let (white, black) = ([1.0f32, 1.0, 1.0, 1.0], [0.0f32, 0.0, 0.0, 1.0]);
    RgbaImage::from_fn(w, h, |x, y| {
        let i = (y * w + x) as usize;
        let op = |plane: &[u8]| byte_unit(0.75) as f32 * f32::from(plane[i]) / 255.0;
        let mut acc = to_unit(GRAY);
        acc = ref_composite(acc, white, op(&o_hl), BLEND_SCREEN);
        acc = ref_composite(acc, black, op(&o_sh), BLEND_MULTIPLY);
        if in_rect(i64::from(x), i64::from(y)) {
            acc = ref_composite(acc, to_unit(RECT_COLOR), 1.0, BLEND_NORMAL);
        }
        acc = ref_composite(acc, white, op(&i_hl), BLEND_SCREEN);
        acc = ref_composite(acc, black, op(&i_sh), BLEND_MULTIPLY);
        Rgba(acc.map(|v| (v * 255.0).round() as u8))
    })
}

fn assert_image(got: &RgbaImage, want: &RgbaImage, tol: i32, what: &str) {
    assert_eq!(got.dimensions(), want.dimensions(), "{what}: dimensions");
    for (x, y, g) in got.enumerate_pixels() {
        let w = want.get_pixel(x, y);
        for c in 0..4 {
            let t = if c == 3 { 0 } else { tol };
            assert!(
                (i32::from(g[c]) - i32::from(w[c])).abs() <= t,
                "{what} ({x},{y}) channel {c}: got {:?} want {:?}",
                g.0,
                w.0
            );
        }
    }
}

fn flat8(json: &str) -> RgbaImage {
    styled(&gray_doc(), 1, json).flattened().unwrap()
}

// ------------------------------------------------------------------ JSON --

fn bevel_of(style: &LayerStyle) -> &doctored_core::style::BevelEmboss {
    match &style.effects[0] {
        Effect::BevelEmboss(b) => b,
        other => panic!("not a bevel: {other:?}"),
    }
}

#[test]
fn technique_parses_writes_only_when_chisel_and_refuses_an_unknown_value() {
    let absent = parse(r#"{"effects":[{"type":"bevel_emboss"}]}"#);
    assert_eq!(bevel_of(&absent).technique, BevelTechnique::Smooth);
    let smooth = parse(r#"{"effects":[{"type":"bevel_emboss","technique":"smooth"}]}"#);
    assert_eq!(smooth, absent, "an explicit smooth is the default");
    assert_eq!(smooth.to_json(), absent.to_json());
    assert!(
        !absent.to_json().contains("technique"),
        "smooth writes no key"
    );
    for (name, technique) in [
        ("chisel_hard", BevelTechnique::ChiselHard),
        ("chisel_soft", BevelTechnique::ChiselSoft),
    ] {
        let style = parse(&format!(
            r#"{{"effects":[{{"type":"bevel_emboss","technique":"{name}"}}]}}"#
        ));
        assert_eq!(bevel_of(&style).technique, technique);
        let text = style.to_json();
        assert!(
            text.contains(&format!("\"technique\":\"{name}\"")),
            "{text}"
        );
        assert_eq!(parse(&text), style, "{name} round-trips");
    }
    // Strict (the FFI setter) refuses by name; lenient (the `.rz` reader)
    // reads an unknown value as Smooth.
    let bad =
        r#"{"effects":[{"type":"drop_shadow"},{"type":"bevel_emboss","technique":"chisel"}]}"#;
    let doc = Box::into_raw(Box::new(gray_doc()));
    assert_eq!(
        style_error(doc, 1, bad).as_deref(),
        Some("style: effects[1].technique must be one of smooth, chisel_hard, chisel_soft")
    );
    unsafe { dr_doc_free(doc) };
    let lenient = LayerStyle::from_json_lenient(bad).unwrap();
    match &lenient.effects[1] {
        Effect::BevelEmboss(b) => assert_eq!(b.technique, BevelTechnique::Smooth),
        other => panic!("not a bevel: {other:?}"),
    }
}

// ---------------------------------------------------------------- kernel --

#[test]
fn chisel_hard_is_the_clamped_distance_ramp_for_every_style() {
    for style in STYLES {
        for down in [false, true] {
            let b = Bevel {
                down,
                ..Bevel::new(style, "chisel_hard")
            };
            assert_image(
                &flat8(&b.json()),
                &expected(&b),
                2,
                &format!("{style} down {down}"),
            );
        }
    }
}

#[test]
fn chisel_soft_is_the_hard_height_blurred_at_the_declared_ratio() {
    for style in STYLES {
        let b = Bevel::new(style, "chisel_soft");
        assert_image(&flat8(&b.json()), &expected(&b), 2, style);
    }
    // With soften and depth on top, the shared tail.
    let b = Bevel {
        soften: 3.0,
        depth: 2.0,
        ..Bevel::new("inner_bevel", "chisel_soft")
    };
    assert_image(&flat8(&b.json()), &expected(&b), 2, "soften 3 depth 2");
    // The Chisel law softens the height, for both shipped arms.
    for technique in ["chisel_hard", "chisel_soft"] {
        let b = Bevel {
            soften: 3.0,
            depth: 2.0,
            ..Bevel::new("pillow_emboss", technique)
        };
        assert_image(
            &flat8(&b.json()),
            &expected(&b),
            2,
            &format!("pillow_emboss {technique} soften 3 depth 2"),
        );
    }
}

#[test]
fn a_chisel_inner_bevel_shades_its_facet_flat_unlike_smooth() {
    // On the right band (light from the right) Chisel Hard's facet is one
    // plane: the pixels between its foot and its crest all light the same.
    let hard = flat8(&Bevel::new("inner_bevel", "chisel_hard").json());
    let smooth = flat8(&Bevel::new("inner_bevel", "smooth").json());
    let y = (RECT.1 + 10) as u32;
    let right = (RECT.0 + RECT.2 as i32 - 1) as u32;
    let facet: Vec<[u8; 4]> = (right - 4..right).map(|x| hard.get_pixel(x, y).0).collect();
    assert!(
        facet.windows(2).all(|p| p[0] == p[1]),
        "a planar facet: {facet:?}"
    );
    assert_ne!(facet[0], RECT_COLOR, "and it is lit");
    assert_ne!(hard, smooth, "Chisel is not Smooth");
}

#[test]
fn smooth_is_byte_identical_with_the_key_absent_or_explicit() {
    for style in STYLES {
        let explicit = Bevel::new(style, "smooth");
        let mut absent = explicit.effect();
        absent.as_object_mut().unwrap().remove("technique");
        let doc = styled(&gray_doc(), 1, &explicit.json());
        let bare = styled(&gray_doc(), 1, &json!({"effects":[absent]}).to_string());
        assert_eq!(
            doc.flattened().unwrap().into_raw(),
            bare.flattened().unwrap().into_raw(),
            "{style}"
        );
        let text = doc.layers[1].style.as_deref().unwrap().to_json();
        assert!(!text.contains("technique"), "{text}");
    }
}

// ---------------------------------------------------------------- depths --

const D_CANVAS: (u32, u32) = (64, 48);
const D_RECT: (i32, i32, u32, u32) = (12, 10, 40, 28);

/// White highlight Normal 100 %, shadow off: over black the red is the
/// highlight coverage.
fn lit(style: &str, technique: &str) -> Value {
    json!({"type":"bevel_emboss","style":style,"technique":technique,"size":7,"angle":0,
        "use_global_light":false,"altitude":30,"highlight_blend":"normal",
        "highlight_color":"#ffffff","highlight_opacity":1,"shadow_opacity":0})
}

fn byte_red(effect: &Value) -> Vec<f64> {
    let doc = DrDocument::from_pixels(solid(D_CANVAS.0, D_CANVAS.1, [0, 0, 0, 255]))
        .adding_image_layer(0, solid(D_RECT.2, D_RECT.3, [0, 0, 0, 255]), "Rect")
        .unwrap();
    let doc = doc.with_layer_offset(1, D_RECT.0, D_RECT.1).unwrap();
    styled(&doc, 1, &json!({"effects":[effect]}).to_string())
        .flattened()
        .unwrap()
        .pixels()
        .map(|p| f64::from(p.0[0]))
        .collect()
}

fn image16(w: u32, h: u32) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        w,
        h,
        vec![[0, 0, 0, 65535]; (w * h) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

fn typed_doc() -> PrecisionDocument {
    PrecisionDocument::from_parts(
        D_CANVAS.0,
        D_CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Base", image16(D_CANVAS.0, D_CANVAS.1), (0, 0), 1.0, true).unwrap(),
            Layer::raster(
                "Rect",
                image16(D_RECT.2, D_RECT.3),
                (D_RECT.0, D_RECT.1),
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn typed_styled(doc: &PrecisionDocument, style: &Value) -> PrecisionDocument {
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

fn assert_within(got: &[f64], want: &[f64], what: &str) {
    let mut on = 0;
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((g - w).abs() <= 1.0, "{what}: pixel {i}: {g} against {w}");
        on += usize::from(*w > 0.0);
    }
    assert!(on > 100, "{what}: only {on} pixels lit");
}

#[test]
fn chisel_at_16_and_32_bits_is_the_byte_render() {
    let float = typed_doc().promote_to_float(LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    for technique in ["chisel_hard", "chisel_soft"] {
        for style in STYLES {
            let effect = lit(style, technique);
            let want = byte_red(&effect);
            let what = format!("{style} {technique}");
            let s = json!({"effects":[effect]});
            let px16 = typed_styled(&typed_doc(), &s)
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba16()
                .unwrap();
            let got: Vec<f64> = px16.iter().map(|p| f64::from(p[0]) / 257.0).collect();
            assert_within(&got, &want, &format!("16-bit {what}"));
            let px32 = typed_styled(&float, &s)
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba32()
                .unwrap();
            let got: Vec<f64> = px32.iter().map(|p| 255.0 * f64::from(p[0])).collect();
            assert_within(&got, &want, &format!("32-bit {what}"));
        }
    }
}

#[test]
fn a_cmyk_chisel_renders_and_differs_from_its_smooth_twin() {
    let (x0, y0) = (D_RECT.0 as u32, D_RECT.1 as u32);
    let base = |depth: u32| {
        layer(
            backdrop(D_CANVAS.0, D_CANVAS.1, depth, &[[0.0; 4]]),
            "Shape",
            [10.0, 60.0, 30.0, 0.0],
            &hard_rect(
                D_CANVAS.0,
                D_CANVAS.1,
                (x0, y0, x0 + D_RECT.2, y0 + D_RECT.3),
            ),
        )
    };
    for depth in [8, 16] {
        let unstyled = base(depth);
        let plain = flat(unstyled);
        free(unstyled);
        let mut planes = Vec::new();
        for technique in ["smooth", "chisel_hard", "chisel_soft"] {
            let mut effect = lit("emboss", technique);
            effect["shadow_opacity"] = json!(0.75);
            let doc = cmyk_styled(base(depth), 1, &json!({"effects":[effect]}));
            planes.push(flat(doc));
            free(doc);
        }
        for (i, technique) in ["smooth", "chisel_hard", "chisel_soft"].iter().enumerate() {
            assert_ne!(planes[i], plain, "{depth}-bit {technique} renders");
        }
        assert_ne!(
            planes[1], planes[0],
            "{depth}-bit Chisel Hard is not Smooth"
        );
        assert_ne!(
            planes[2], planes[0],
            "{depth}-bit Chisel Soft is not Smooth"
        );
        assert_ne!(planes[2], planes[1], "{depth}-bit the two Chisels differ");
    }
}

// ----------------------------------------------------------- persistence --

#[test]
fn technique_survives_the_byte_and_typed_rz() {
    let dir = TempDir::new().unwrap();
    for technique in ["chisel_hard", "chisel_soft"] {
        let json = Bevel::new("emboss", technique).json();
        let doc = styled(&gray_doc(), 1, &json);
        let path = dir.path().join(format!("{technique}.rz"));
        doc.save_native(path.to_str().unwrap()).unwrap();
        let back = DrDocument::open(path.to_str().unwrap()).unwrap();
        let text = back.layers[1].style.as_deref().unwrap().to_json();
        assert!(text.contains(technique), "{text}");
        assert_eq!(text, doc.layers[1].style.as_deref().unwrap().to_json());
        assert_eq!(back.flattened().unwrap(), doc.flattened().unwrap());

        let typed = typed_styled(
            &typed_doc(),
            &json!({"effects":[lit("inner_bevel", technique)]}),
        );
        let path = dir.path().join(format!("{technique}-16.rz"));
        let path = path.to_str().unwrap();
        typed.save_native(path, LIMIT, 0, None).unwrap();
        let reopened = PrecisionDocument::open_native(path, LIMIT, 0, None).unwrap();
        let reported = reopened.layer(1).unwrap().reported_style().unwrap();
        assert!(reported.contains(technique), "{reported}");
        assert_eq!(
            Some(reported),
            typed.layer(1).unwrap().reported_style(),
            "{technique}"
        );
        assert_eq!(
            reopened
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba16()
                .unwrap(),
            typed.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
        );
    }
}

// -------------------------------------------------------------- locality --

/// Paints a canvas-frame premultiplied overlay onto layer `idx` through the
/// FFI (`dr_doc_painting_layer`).
fn painted(doc: &DrDocument, idx: usize, overlay: &[u8], mode: c_int) -> DrDocument {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let out = unsafe {
        dr_doc_painting_layer(
            handle,
            idx,
            overlay.as_ptr(),
            doc.width,
            doc.height,
            mode,
            1.0,
        )
    };
    unsafe { dr_doc_free(handle) };
    assert!(!out.is_null(), "painting_layer refused");
    *unsafe { Box::from_raw(out) }
}

fn overlay_rect(w: u32, h: u32, rect: (u32, u32, u32, u32), px: [u8; 4]) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in rect.1..rect.1 + rect.3 {
        for x in rect.0..rect.0 + rect.2 {
            let i = ((y * w + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&px);
        }
    }
    out
}

#[test]
fn a_local_edit_patches_a_chisel_bevel_exactly() {
    // `height_reach` (and the Chisel law's `chisel_law_reach` for Pillow
    // Emboss) is the Chisel's locality: a brush tick's patched
    // projection must equal a cold render (a fresh style, an empty cache).
    const W: u32 = 480;
    const H: u32 = 360;
    let mut content = RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 0]));
    for y in 100..250 {
        for x in 140..340 {
            content.put_pixel(x, y, Rgba([200, 40, 40, 255]));
        }
    }
    let base = DrDocument::from_pixels(solid(W, H, [255, 255, 255, 255]))
        .adding_image_layer(0, content, "Content")
        .unwrap();
    let hole = overlay_rect(W, H, (230, 170, 8, 6), [0, 0, 0, 255]);
    let dab = overlay_rect(W, H, (100, 60, 12, 12), [0, 0, 255, 255]);
    for json in [
        Bevel {
            size: 24.0,
            soften: 3.0,
            ..Bevel::new("emboss", "chisel_hard")
        }
        .json(),
        Bevel {
            size: 30.0,
            ..Bevel::new("outer_bevel", "chisel_soft")
        }
        .json(),
        Bevel::new("pillow_emboss", "chisel_soft").json(),
        Bevel {
            size: 10.0,
            soften: 2.0,
            ..Bevel::new("pillow_emboss", "chisel_hard")
        }
        .json(),
    ] {
        let cached = styled(&base, 1, &json);
        let _ = cached.flattened().unwrap();
        for (name, overlay, mode) in [
            ("hole", &hole, COMPOSITE_ERASE),
            ("dab", &dab, COMPOSITE_OVER),
        ] {
            let edited = painted(&cached, 1, overlay, mode);
            let got = edited.flattened().unwrap().into_raw();
            let mut fresh = edited.clone();
            fresh.layers[1].style = Some(Arc::new(parse(&json)));
            assert_eq!(got, fresh.flattened().unwrap().into_raw(), "{json}: {name}");
        }
    }
}
