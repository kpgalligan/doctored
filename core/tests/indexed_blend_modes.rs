//! #1 Indexed Color phase 3b: the Brush, Fill, Stroke and the Paint Bucket
//! in blend modes other than Normal (`indexed_paint`, `indexed_edge`'s
//! `EdgeInk::Blend`).
//!
//! The oracle is independent of the kernel: Image ▸ Mode ▸ RGB Color, the
//! same request on that RGB document, then the shared re-index
//! (`indexed_fixture::oracle_with`, `Ink::Blended`) with this file's own
//! full-strength colour of each mode, written from the W3C compositing
//! formulas in f64. The fixtures are chosen so that T's last code cannot
//! change the entry nearest it (asserted), so the f64 formula and the
//! kernel's f32 one cannot disagree on E.

use serde_json::{json, Value};

use crate::indexed_fixture::{
    edit_plan, free, indices, nearest, oracle_with, paint, palette_of, rgb_doc, set_table,
    to_indexed, to_rgb, window, Ink,
};

const W: u32 = 12;
const H: u32 = 8;

const INK: [u8; 3] = [220, 30, 30];

/// The 12 × 8 scene: black, white, (190, 50, 50) and (40, 60, 200) bands,
/// its last row clear (the transparent index) when `clear_row`.
fn scene(clear_row: bool) -> *mut doctored_core::precision_document::PrecisionDocument {
    let mut pixels = Vec::new();
    for y in 0..H {
        for x in 0..W {
            pixels.push(if clear_row && y == H - 1 {
                [0, 0, 0, 0]
            } else if x < 3 {
                [0, 0, 0, 255]
            } else if x < 6 {
                [255, 255, 255, 255]
            } else if x < 9 {
                [190, 50, 50, 255]
            } else {
                [40, 60, 200, 255]
            });
        }
    }
    let rgb = rgb_doc(W, H, &pixels);
    let out = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    out
}

/// The scene with no transparent index and an extra unused grey entry, so
/// E has five candidates.
fn opaque_scene() -> *mut doctored_core::precision_document::PrecisionDocument {
    let doc = scene(false);
    let (mut palette, t) = palette_of(doc);
    assert_eq!(t, None);
    palette.push([128, 128, 128]);
    let out = set_table(doc, &json!({"colors":palette,"transparent_index":null}))
        .unwrap()
        .unwrap();
    free(doc);
    out
}

/// A graded disc of coverage around (6, 4), radius 5.5, ramping over 2 px:
/// every band takes full ink and an edge.
fn disc() -> Vec<u16> {
    let mut out = Vec::new();
    for y in 0..H {
        for x in 0..W {
            let d = ((f64::from(x) - 6.0).powi(2) + (f64::from(y) - 4.0).powi(2)).sqrt();
            out.push((((5.5 - d) / 2.0).clamp(0.0, 1.0) * 65535.0).round() as u16);
        }
    }
    out
}

fn ink_json() -> Value {
    json!([
        f64::from(INK[0]) / 255.0,
        f64::from(INK[1]) / 255.0,
        f64::from(INK[2]) / 255.0,
        1.0
    ])
}

fn unit(c: u8) -> f64 {
    f64::from(c) / 255.0
}

fn code(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A separable W3C blend `b(backdrop, source)` per channel, in codes.
fn separable(s: [u8; 3], ink: [u8; 3], b: fn(f64, f64) -> f64) -> [u8; 3] {
    [0, 1, 2].map(|c| code(b(unit(s[c]), unit(ink[c]))))
}

fn multiply(s: [u8; 3], ink: [u8; 3]) -> [u8; 3] {
    separable(s, ink, |b, s| b * s)
}

fn screen(s: [u8; 3], ink: [u8; 3]) -> [u8; 3] {
    separable(s, ink, |b, s| b + s - b * s)
}

fn darken(s: [u8; 3], ink: [u8; 3]) -> [u8; 3] {
    separable(s, ink, f64::min)
}

fn difference(s: [u8; 3], ink: [u8; 3]) -> [u8; 3] {
    separable(s, ink, |b, s| (b - s).abs())
}

/// W3C Lum (0.3, 0.59, 0.11).
fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// W3C ClipColor.
fn clip_color(mut c: [f64; 3]) -> [f64; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if n < 0.0 {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 && x > l {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    c
}

/// Luminosity: SetLum(backdrop, Lum(source)).
fn luminosity(s: [u8; 3], ink: [u8; 3]) -> [u8; 3] {
    let b = s.map(unit);
    let d = lum(ink.map(unit)) - lum(b);
    clip_color(b.map(|v| v + d)).map(code)
}

/// Dissolve's full strength is the ink itself.
fn dissolve(_s: [u8; 3], ink: [u8; 3]) -> [u8; 3] {
    ink
}

type Blended = fn([u8; 3], [u8; 3]) -> [u8; 3];

/// (name, `blend_mode`, the test's own full-strength colour).
const MODES: [(&str, u32, Blended); 6] = [
    ("multiply", 1, multiply),
    ("screen", 2, screen),
    ("darken", 6, darken),
    ("difference", 8, difference),
    ("luminosity", 26, luminosity),
    ("dissolve", 14, dissolve),
];

/// T's last code cannot change E: moving any one channel of T by one code
/// keeps the nearest entry, for every opaque backdrop of `palette`.
fn assert_target_is_robust(palette: &[[u8; 3]], t: Option<u8>, blended: Blended, name: &str) {
    for (i, &s) in palette.iter().enumerate() {
        if Some(i as u8) == t {
            continue;
        }
        let target = blended(s, INK);
        let e = nearest(palette, t, target);
        for c in 0..3 {
            for delta in [-1i16, 1] {
                let mut moved = target;
                moved[c] = (i16::from(target[c]) + delta).clamp(0, 255) as u8;
                assert_eq!(
                    nearest(palette, t, moved),
                    e,
                    "{name}: T {target:?} over {s:?} is on an edge of E"
                );
            }
        }
    }
}

fn brush(mode: u32) -> Value {
    json!({"op":"paint_blend","layer":0,"color":ink_json(),"color_space":"document",
           "opacity":1.0,"blend_mode":mode})
}

/// `request` on `doc` against the same request on its RGB twin re-indexed
/// by the oracle; returns the number of indices it moved.
fn check(
    doc: *const doctored_core::precision_document::PrecisionDocument,
    request: &Value,
    input: &[u16],
    blended: Blended,
    name: &str,
) -> usize {
    let (palette, t) = palette_of(doc);
    let before = indices(doc);
    let painted = paint(doc, request, input)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .unwrap_or_else(|| panic!("{name}: no index moved"));
    let rgb = to_rgb(doc);
    let twin = paint(rgb, request, input)
        .unwrap_or_else(|e| panic!("{name} on RGB: {e}"))
        .unwrap_or_else(|| panic!("{name} on RGB: nothing changed"));
    let expected = oracle_with(
        &palette,
        t,
        Some(&before),
        &window(twin),
        Ink::Blended(INK, blended),
        [255, 255, 255],
    )
    .unwrap();
    let got = indices(painted);
    assert_eq!(got, expected, "{name}");
    let moved = got.iter().zip(&before).filter(|(a, b)| a != b).count();
    free(twin);
    free(rgb);
    free(painted);
    moved
}

#[test]
fn the_brush_in_each_mode_is_the_rgb_stroke_reindexed() {
    for doc in [scene(true), opaque_scene()] {
        let (palette, t) = palette_of(doc);
        for (name, mode, blended) in MODES {
            assert_target_is_robust(&palette, t, blended, name);
            let moved = check(doc, &brush(mode), &disc(), blended, name);
            assert!(moved > 0, "{name}: no index moved");
        }
        free(doc);
    }
}

#[test]
fn the_bucket_and_the_stroke_in_a_mode_are_the_rgb_op_reindexed() {
    let n = (W * H) as usize;
    for doc in [scene(true), opaque_scene()] {
        let (palette, t) = palette_of(doc);
        assert_target_is_robust(&palette, t, screen, "screen");
        assert_target_is_robust(&palette, t, multiply, "multiply");
        let bucket = json!({"op":"bucket_fill","layer":0,"color":ink_json(),
            "color_space":"document","opacity":0.7,"blend_mode":2,"x":1,"y":2,
            "tolerance":0,"contiguous":true});
        check(doc, &bucket, &vec![65535; n], screen, "bucket screen");
        let stroke = json!({"op":"stroke_layer_edge","layer":0,"width":2.0,"location":"inside",
            "color":ink_json(),"color_space":"document","opacity":1.0,"blend_mode":1});
        check(doc, &stroke, &[], multiply, "stroke multiply");
        free(doc);
    }
}

#[test]
fn a_mode_plans_as_normal_does() {
    let doc = scene(true);
    let normal = edit_plan(doc, &brush(0)).unwrap();
    for (name, mode, _) in MODES {
        let plan = edit_plan(doc, &brush(mode)).unwrap();
        let keys = |v: &Value| {
            let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(keys(&plan), keys(&normal), "{name}");
        assert_eq!(plan["input_samples"], normal["input_samples"], "{name}");
        assert_eq!(plan["scratch_bytes"], normal["scratch_bytes"], "{name}");
    }
    free(doc);
}
