//! Move and Free Transform of ONE colour channel's selected pixels
//! (`"target":"red"|"green"|"blue"|"alpha"`), through both FFI seams
//! (`dr_doc_selected_pixels_edit` and `dr_precision_doc_edit`) at 8 and 16
//! bits and in a 32-bit float document.
//!
//! The oracle is written independently in f64 from take2's rule: component k
//! floats as premultiplied `(v·c, c)` with c the selection coverage times the
//! stored alpha for Red, Green and Blue (the coverage alone for Alpha, which
//! reads 0 outside the layer), resamples (nearest or bilinear), unpremultiplies
//! and clamps (to 0…1, or to the float's own visible range at f32); the hole
//! is `lerp(v, h, c)` with h the background swatch's 30/59/11 luma rounded to
//! a code, /255 (`plane_gray.py`'s `3059r`, at 8, 16 and 32 bits; Alpha: 0; a
//! duplicate: none); the float lands source-over an
//! opaque gray, `lerp(holed, v_f, a_f)`. Writes clip to the layer; every other
//! component stays bit for bit.

use crate::common;
use crate::float_fixture::float_doc;

use std::ffi::CString;
use std::ptr;

use common::selected_pixels::*;
use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::{LOCK_ALL, LOCK_PIXELS, LOCK_TRANSPARENCY};
use doctored_core::ffi_retained_source::{dr_doc_mixed_source_edit, dr_precision_doc_source_edit};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use image::RgbaImage;
use serde_json::{json, Value};

const EMPTY: &str = "The selection holds no pixels in this colour channel.";
const CANVAS: (u32, u32) = (16, 12);
/// take2's P1 background swatch: its hole code is 102 = 0.4 × 255.
const P1: [u8; 3] = [220, 30, 170];
const TARGETS: [&str; 4] = ["red", "green", "blue", "alpha"];

// --------------------------------------------------------------- requests --

fn moved(target: &str, dx: i32, dy: i32, swatch: Option<[u8; 3]>) -> Value {
    let mut v = json!({"op":"move_selected","layer":0,"dx":dx,"dy":dy,"target":target});
    if let Some(s) = swatch {
        v["swatch"] = json!(s);
    }
    v
}

fn transformed(target: &str, m: [f64; 6], filter: &str, swatch: Option<[u8; 3]>) -> Value {
    let mut v = json!({"op":"transform_selected","layer":0,"matrix":m,"filter":filter,
                       "target":target});
    if let Some(s) = swatch {
        v["swatch"] = json!(s);
    }
    v
}

fn rotate(degrees: f64, cx: f64, cy: f64) -> [f64; 6] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [
        cos,
        sin,
        -sin,
        cos,
        cx - cos * cx + sin * cy,
        cy - sin * cx - cos * cy,
    ]
}

/// `plane_gray.py`'s swatches Q1..Q3 with the code Photoshop left in the
/// vacated area at 8, 16 and 32 bits (`plane-gray-431-633/take1`). Rec. 601
/// floored, the rule before that take, reads 29, 80 and 148.
const TAKE: [([u8; 3], u8); 3] = [([0, 0, 255], 28), ([250, 10, 0], 81), ([90, 200, 40], 149)];

/// The independent hole: 0.30 r + 0.59 g + 0.11 b rounded to a code, over
/// 255, in f64 (the nudge keeps an exact half from landing below it).
fn hole3059(s: [u8; 3]) -> f64 {
    let l = 0.30 * f64::from(s[0]) + 0.59 * f64::from(s[1]) + 0.11 * f64::from(s[2]);
    (l + 0.5 + 1e-9).floor() / 255.0
}

fn component(target: &str) -> usize {
    TARGETS.iter().position(|t| *t == target).unwrap()
}

// --------------------------------------------------------------- fixtures --

/// Opaque on the left, transparent from x = 9 on (take2's shape), at an
/// offset so a selection can reach past the layer.
fn layer_px(x: u32, y: u32) -> [u8; 4] {
    let a = if x >= 9 { 0 } else { 255 };
    [
        (x * 29 + y * 3 + 13) as u8,
        (y * 41 + 7) as u8,
        ((x + y) * 17 + 101) as u8,
        a,
    ]
}

const OFFSET: (i32, i32) = (1, 1);
const SIZE: (u32, u32) = (13, 10);

fn fixture(depth: Depth) -> (Plane, Doc) {
    let plane = Plane::of(depth, OFFSET, SIZE, layer_px);
    let d = doc(depth, CANVAS, &plane);
    (plane, d)
}

fn edit(doc: &Doc, request: &Value, cov: &[u16]) -> Doc {
    doc.edit(&request.to_string(), cov)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

// ----------------------------------------------------------------- oracle --

#[derive(Clone, Copy, PartialEq)]
enum Kernel {
    Nearest,
    Bilinear,
}

struct Oracle<'a> {
    canvas: (u32, u32),
    /// The stored, normalized pixel at a canvas point; None off the layer.
    pixel: &'a dyn Fn(i64, i64) -> Option<[f64; 4]>,
    k: usize,
    coverage: &'a [u16],
    /// The unit hole; None is the duplicate.
    hole: Option<f64>,
    m: [f64; 6],
    kernel: Kernel,
    /// f32: clamp to the float's own visible range instead of 0…1.
    extended: bool,
}

impl Oracle<'_> {
    fn on(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < i64::from(self.canvas.0) && y < i64::from(self.canvas.1)
    }
    fn v(&self, x: i64, y: i64) -> f64 {
        (self.pixel)(x, y).map_or(0.0, |p| p[self.k])
    }
    /// The coverage the walk sees: the selection × the stored alpha for
    /// R/G/B (0 off the layer), the selection alone for Alpha.
    fn cov(&self, x: i64, y: i64) -> f64 {
        if !self.on(x, y) {
            return 0.0;
        }
        let c =
            f64::from(self.coverage[y as usize * self.canvas.0 as usize + x as usize]) / 65535.0;
        if self.k == 3 {
            c
        } else {
            // The scaled coverage is u16, as the selection is.
            let a = (self.pixel)(x, y).map_or(0.0, |p| p[3]);
            (c * 65535.0 * a).round() / 65535.0
        }
    }
    fn range(&self) -> (f64, f64) {
        let mut range = (f64::INFINITY, f64::NEG_INFINITY);
        for y in 0..i64::from(self.canvas.1) {
            for x in 0..i64::from(self.canvas.0) {
                if self.cov(x, y) > 0.0 {
                    range = (range.0.min(self.v(x, y)), range.1.max(self.v(x, y)));
                }
            }
        }
        range
    }
    fn float(&self, x: i64, y: i64) -> [f64; 2] {
        let c = self.cov(x, y);
        [self.v(x, y) * c, c]
    }
    fn landed(&self, x: i64, y: i64) -> Option<(f64, f64)> {
        let m = self.m;
        let det = m[0] * m[3] - m[1] * m[2];
        let (dx, dy) = (x as f64 + 0.5 - m[4], y as f64 + 0.5 - m[5]);
        let u = (m[3] * dx - m[2] * dy) / det;
        let v = (-m[1] * dx + m[0] * dy) / det;
        let acc = match self.kernel {
            Kernel::Nearest => self.float(u.floor() as i64, v.floor() as i64),
            Kernel::Bilinear => {
                let (kx, ky) = ((u - 0.5).floor(), (v - 0.5).floor());
                let (fx, fy) = (u - 0.5 - kx, v - 0.5 - ky);
                let mut acc = [0.0; 2];
                for (oy, wy) in [(0, 1.0 - fy), (1, fy)] {
                    for (ox, wx) in [(0, 1.0 - fx), (1, fx)] {
                        let s = self.float(kx as i64 + ox, ky as i64 + oy);
                        acc[0] += s[0] * wx * wy;
                        acc[1] += s[1] * wx * wy;
                    }
                }
                acc
            }
        };
        if acc[1] <= 0.0 {
            return None;
        }
        let (lo, hi) = if self.extended {
            self.range()
        } else {
            (0.0, 1.0)
        };
        Some(((acc[0] / acc[1]).clamp(lo, hi), acc[1].clamp(0.0, 1.0)))
    }
    /// The expected unit value at a layer pixel; None keeps it.
    fn expect(&self, x: i64, y: i64) -> Option<f64> {
        if !self.on(x, y) {
            return None;
        }
        let c = self.cov(x, y);
        let landing = self.landed(x, y);
        let holed = match self.hole {
            Some(h) if c > 0.0 => {
                let v = self.v(x, y);
                v + (h - v) * c
            }
            _ if landing.is_none() => return None,
            _ => self.v(x, y),
        };
        Some(match landing {
            Some((vf, af)) => holed + (vf - holed) * af,
            None => holed,
        })
    }
}

/// Every pixel of the edited layer against the oracle at 8 or 16 bits:
/// component k within `slack` codes, every other component bit for bit, the
/// layer's rect unchanged. Returns how many pixels component k changed.
fn check_codes(depth: Depth, before: &Plane, after: &Plane, o: &Oracle, slack: f64) -> usize {
    assert_eq!((after.offset, after.size), (before.offset, before.size));
    let max = depth.max();
    let mut changed = 0;
    for (i, (got, was)) in after.px.iter().zip(&before.px).enumerate() {
        let x = i64::from(before.offset.0) + (i as i64 % i64::from(before.size.0));
        let y = i64::from(before.offset.1) + (i as i64 / i64::from(before.size.0));
        for c in 0..4 {
            if c != o.k {
                assert_eq!(got[c], was[c], "{depth:?} ({x}, {y})[{c}] untouched");
            }
        }
        match o.expect(x, y) {
            None => assert_eq!(got[o.k], was[o.k], "{depth:?} ({x}, {y}) kept"),
            Some(want) => {
                let want = want.clamp(0.0, 1.0) * max;
                assert!(
                    (f64::from(got[o.k]) - want).abs() <= slack + 1e-9,
                    "{depth:?} ({x}, {y})[{}] got {} want {want}",
                    o.k,
                    got[o.k]
                );
            }
        }
        changed += usize::from(got[o.k] != was[o.k]);
    }
    changed
}

fn normalized(depth: Depth, plane: &Plane) -> impl Fn(i64, i64) -> Option<[f64; 4]> + '_ {
    let max = depth.max();
    move |x, y| plane.at(x, y).map(|p| p.map(|v| f64::from(v) / max))
}

// ------------------------------------------------------- 8 and 16 bits --

#[test]
fn a_hard_move_of_each_colour_channel_matches_the_oracle() {
    // The selection reaches past the layer's right edge and over its
    // transparent columns; the landing is on opaque pixels.
    let cov = coverage(CANVAS, rect(2, 2, 13, 4));
    for depth in DEPTHS {
        let (plane, base) = fixture(depth);
        let pixel = normalized(depth, &plane);
        for target in TARGETS {
            let k = component(target);
            let swatch = (k < 3).then_some(P1);
            let got = edit(&base, &moved(target, -1, 4, swatch), &cov).plane(0);
            let o = Oracle {
                canvas: CANVAS,
                pixel: &pixel,
                k,
                coverage: &cov,
                hole: Some(if k < 3 { hole3059(P1) } else { 0.0 }),
                m: [1.0, 0.0, 0.0, 1.0, -1.0, 4.0],
                kernel: Kernel::Nearest,
                extended: false,
            };
            assert!(
                check_codes(depth, &plane, &got, &o, 0.0) > 0,
                "{depth:?} {target}"
            );
        }
        // The vacated opaque pixel reads the swatch's code exactly.
        let got = edit(&base, &moved("red", -1, 4, Some(P1)), &cov).plane(0);
        assert_eq!(got.at(3, 2).unwrap()[0], depth.code(102, 0), "{depth:?}");
        // A transparent source pixel lands nothing: (10, 3) is transparent,
        // so (9, 7) keeps its own red.
        assert_eq!(
            got.at(9, 7).unwrap()[0],
            plane.at(9, 7).unwrap()[0],
            "{depth:?}"
        );
    }
}

#[test]
fn an_absent_swatch_is_black_and_each_swatch_leaves_its_own_code() {
    let cov = coverage(CANVAS, rect(2, 2, 4, 4));
    for depth in DEPTHS {
        let (_, base) = fixture(depth);
        let take = TAKE.map(|(swatch, code)| (Some(swatch), code));
        for (swatch, code) in [
            (None, 0u8),
            (Some([60, 120, 230]), 114),
            (Some([10, 90, 250]), 84),
            (Some([255, 255, 255]), 255),
        ]
        .into_iter()
        .chain(take)
        {
            assert_eq!(
                swatch.map_or(0.0, hole3059),
                f64::from(code) / 255.0,
                "{swatch:?}"
            );
            let got = edit(&base, &moved("green", 6, 0, swatch), &cov).plane(0);
            assert_eq!(
                got.at(3, 3).unwrap()[1],
                depth.code(code, 0),
                "{depth:?} {swatch:?}"
            );
        }
    }
}

#[test]
fn a_transform_matches_the_oracle() {
    let cov = coverage(CANVAS, rect(2, 2, 6, 5));
    for depth in DEPTHS {
        let (plane, base) = fixture(depth);
        let pixel = normalized(depth, &plane);
        for target in ["red", "blue", "alpha"] {
            let k = component(target);
            let swatch = (k < 3).then_some([60, 120, 230]);
            for (m, kernel, filter, slack) in [
                (rotate(90.0, 5.0, 5.0), Kernel::Nearest, "nearest", 0.0),
                (rotate(20.0, 5.5, 5.0), Kernel::Bilinear, "bilinear", 1.0),
            ] {
                let got = edit(&base, &transformed(target, m, filter, swatch), &cov).plane(0);
                let o = Oracle {
                    canvas: CANVAS,
                    pixel: &pixel,
                    k,
                    coverage: &cov,
                    hole: Some(if k < 3 { hole3059([60, 120, 230]) } else { 0.0 }),
                    m,
                    kernel,
                    extended: false,
                };
                check_codes(depth, &plane, &got, &o, slack);
            }
        }
    }
}

#[test]
fn a_move_equals_the_transform_translation_and_a_duplicate_leaves_no_hole() {
    let cov = coverage(CANVAS, rect(2, 3, 5, 4));
    for depth in DEPTHS {
        let (plane, base) = fixture(depth);
        let pixel = normalized(depth, &plane);
        for target in TARGETS {
            let swatch = (target != "alpha").then_some(P1);
            let a = edit(&base, &moved(target, 3, 2, swatch), &cov).plane(0);
            let m = [1.0, 0.0, 0.0, 1.0, 3.0, 2.0];
            let b = edit(&base, &transformed(target, m, "bilinear", swatch), &cov).plane(0);
            assert_eq!(a, b, "{depth:?} {target}");
            let mut copy = moved(target, 3, 2, swatch);
            copy["duplicate"] = json!(true);
            // An opaque Alpha copied onto opaque pixels is no edit at all.
            let got = base
                .edit(&copy.to_string(), &cov)
                .unwrap()
                .map_or_else(|| plane.clone(), |d| d.plane(0));
            let o = Oracle {
                canvas: CANVAS,
                pixel: &pixel,
                k: component(target),
                coverage: &cov,
                hole: None,
                m,
                kernel: Kernel::Nearest,
                extended: false,
            };
            check_codes(depth, &plane, &got, &o, 0.0);
        }
    }
}

#[test]
fn a_feathered_selection_over_partial_alpha_follows_the_built_row() {
    let canvas = CANVAS;
    let cov = coverage(canvas, |x, y| ((x * 4111 + y * 977) % 65536) as u16);
    for depth in DEPTHS {
        let plane = Plane::of(depth, OFFSET, SIZE, |x, y| {
            let p = layer_px(x, y);
            [p[0], p[1], p[2], (x * 19 + y * 23) as u8]
        });
        let base = doc(depth, canvas, &plane);
        let pixel = normalized(depth, &plane);
        for target in ["green", "alpha"] {
            let k = component(target);
            let swatch = (k < 3).then_some(P1);
            let got = edit(&base, &moved(target, 2, -1, swatch), &cov).plane(0);
            let o = Oracle {
                canvas,
                pixel: &pixel,
                k,
                coverage: &cov,
                hole: Some(if k < 3 { hole3059(P1) } else { 0.0 }),
                m: [1.0, 0.0, 0.0, 1.0, 2.0, -1.0],
                kernel: Kernel::Nearest,
                extended: false,
            };
            check_codes(depth, &plane, &got, &o, 1.0);
        }
    }
}

#[test]
fn an_all_transparent_lift_refuses_with_the_empty_sentence() {
    // Columns 11…13 of the canvas are the layer's transparent x ≥ 10; past
    // the layer (x = 14, 15) there is nothing either.
    let cov = coverage(CANVAS, rect(11, 2, 5, 5));
    for depth in DEPTHS {
        let (_, base) = fixture(depth);
        for target in ["red", "green", "blue"] {
            let err = base
                .edit(&moved(target, -6, 1, Some(P1)).to_string(), &cov)
                .err();
            assert_eq!(err.as_deref(), Some(EMPTY), "{depth:?} {target} move");
            let m = rotate(30.0, 12.0, 4.0);
            let text = transformed(target, m, "bilinear", Some(P1)).to_string();
            assert_eq!(
                base.edit(&text, &cov).err().as_deref(),
                Some(EMPTY),
                "{depth:?} {target}"
            );
        }
        // Alpha floats the coverage itself: its lift moves zeros.
        assert!(base
            .edit(&moved("alpha", -6, 1, None).to_string(), &cov)
            .unwrap()
            .is_some());
    }
}

#[test]
fn the_identity_and_an_unchanged_channel_are_no_edit() {
    let cov = coverage(CANVAS, rect(2, 2, 4, 4));
    for depth in DEPTHS {
        let (_, base) = fixture(depth);
        assert!(base
            .edit(&moved("red", 0, 0, Some(P1)).to_string(), &cov)
            .unwrap()
            .is_none());
        let m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let text = transformed("alpha", m, "bilinear", None).to_string();
        assert!(base.edit(&text, &cov).unwrap().is_none(), "{depth:?}");
        // A red already at the hole everywhere: the move changes nothing.
        let flat = Plane {
            offset: (0, 0),
            size: CANVAS,
            px: vec![
                [depth.code(102, 0), 9, 9, depth.code(255, 0)];
                (CANVAS.0 * CANVAS.1) as usize
            ],
        };
        let flat = doc(depth, CANVAS, &flat);
        let text = moved("red", 2, 1, Some(P1)).to_string();
        assert!(flat.edit(&text, &cov).unwrap().is_none(), "{depth:?}");
    }
}

// --------------------------------------------------------- 32-bit float --

/// Extended straight RGBA: above white, negative, half alpha, and a
/// transparent block on the right.
fn extended(x: u32, y: u32) -> [f32; 4] {
    let alpha = if x >= 12 {
        0.0
    } else if (x + 2 * y) % 7 == 3 {
        0.5
    } else {
        1.0
    };
    [
        -0.75 + 0.625 * x as f32,
        3.5 - 0.375 * y as f32,
        ((x * 7 + y * 3) % 11) as f32 * 0.9 - 1.25,
        alpha,
    ]
}

fn float_of(profile: &std::sync::Arc<IccProfile>) -> PrecisionDocument {
    let records = (0..CANVAS.0 * CANVAS.1)
        .map(|i| extended(i % CANVAS.0, i / CANVAS.0))
        .collect();
    float_doc(CANVAS.0, CANVAS.1, profile, &[records])
}

/// Record 0's stored samples at canvas points; None off the layer.
fn float_pixels(doc: &PrecisionDocument) -> impl Fn(i64, i64) -> Option<[f32; 4]> + '_ {
    let layer = doc.layer(0).unwrap();
    let (ox, oy) = layer.offset();
    let image = layer.source().unwrap();
    move |x, y| {
        let (lx, ly) = (x - i64::from(ox), y - i64::from(oy));
        if lx < 0 || ly < 0 {
            return None;
        }
        image.sample_at(lx as u32, ly as u32)
    }
}

fn run32(doc: &PrecisionDocument, request: &Value, cov: &[u16]) -> PrecisionDocument {
    edit16(doc, &request.to_string(), cov, None)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

/// Every pixel of the edited float layer: component k within 1e-6 of the
/// magnitudes mixed, the others bit for bit.
fn check_float(before: &PrecisionDocument, after: &PrecisionDocument, o: &Oracle) {
    let (was, got) = (float_pixels(before), float_pixels(after));
    let a = after.layer(0).unwrap();
    assert_eq!(a.offset(), before.layer(0).unwrap().offset());
    for y in 0..i64::from(CANVAS.1) {
        for x in 0..i64::from(CANVAS.0) {
            let (Some(w), Some(g)) = (was(x, y), got(x, y)) else {
                assert_eq!(
                    was(x, y).is_some(),
                    got(x, y).is_some(),
                    "({x}, {y}) extent"
                );
                continue;
            };
            for c in 0..4 {
                if c != o.k {
                    assert_eq!(g[c].to_bits(), w[c].to_bits(), "({x}, {y})[{c}] untouched");
                }
            }
            match o.expect(x, y) {
                None => assert_eq!(g[o.k].to_bits(), w[o.k].to_bits(), "({x}, {y}) kept"),
                Some(want) => {
                    let bound = 1e-6 * (1.0 + want.abs() + f64::from(w[o.k]).abs()) * 4.0;
                    assert!(
                        (f64::from(g[o.k]) - want).abs() <= bound,
                        "({x}, {y})[{}]: {} vs {want}",
                        o.k,
                        g[o.k]
                    );
                }
            }
        }
    }
}

#[test]
fn a_float_colour_channel_moves_and_transforms_with_extended_values_kept() {
    let base = float_of(&IccProfile::srgb());
    let stored = float_pixels(&base);
    let pixel = |x, y| stored(x, y).map(|p: [f32; 4]| p.map(f64::from));
    let cov = coverage(CANVAS, rect(2, 2, 13, 5));
    for target in TARGETS {
        let k = component(target);
        let swatch = (k < 3).then_some(P1);
        for (request, m, kernel, hole) in [
            (
                moved(target, -1, 4, swatch),
                [1.0, 0.0, 0.0, 1.0, -1.0, 4.0],
                Kernel::Nearest,
                true,
            ),
            (
                transformed(target, rotate(25.0, 7.0, 5.0), "bilinear", swatch),
                rotate(25.0, 7.0, 5.0),
                Kernel::Bilinear,
                true,
            ),
            (
                json!({"op":"move_selected","layer":0,"dx":3,"dy":2,"target":target,
                       "duplicate":true}),
                [1.0, 0.0, 0.0, 1.0, 3.0, 2.0],
                Kernel::Nearest,
                false,
            ),
        ] {
            let got = run32(&base, &request, &cov);
            assert!(got.is_float());
            let o = Oracle {
                canvas: CANVAS,
                pixel: &pixel,
                k,
                coverage: &cov,
                hole: hole.then_some(if k < 3 { hole3059(P1) } else { 0.0 }),
                m,
                kernel,
                extended: true,
            };
            check_float(&base, &got, &o);
        }
    }
    // The vacated opaque red is take2's raw 0.4 (102/255, no decode), and an
    // extended value lands unclamped: (5, 3)'s red 2.375 lands at (4, 7).
    let got = run32(&base, &moved("red", -1, 4, Some(P1)), &cov);
    let after = float_pixels(&got);
    assert!((after(5, 3).unwrap()[0] - 102.0 / 255.0).abs() <= 1e-6);
    assert!((after(4, 7).unwrap()[0] - stored(5, 3).unwrap()[0]).abs() <= 1e-6);
    assert!(stored(5, 3).unwrap()[0] > 1.0);
    // The take's swatches read their 8-bit code over 255, unconverted.
    let cov = coverage(CANVAS, rect(2, 2, 4, 4));
    for (swatch, code) in TAKE {
        let got = run32(&base, &moved("green", 6, 0, Some(swatch)), &cov);
        let after = float_pixels(&got);
        assert!(
            (after(3, 3).unwrap()[1] - f32::from(code) / 255.0).abs() <= 1e-6,
            "{swatch:?}"
        );
    }
}

#[test]
fn a_display_p3_float_document_writes_the_raw_hole() {
    let base = float_of(&IccProfile::display_p3());
    let cov = coverage(CANVAS, rect(2, 2, 4, 4));
    let got = run32(&base, &moved("blue", 6, 0, Some([60, 120, 230])), &cov);
    let after = float_pixels(&got);
    // No profile decode: the code over 255, as in sRGB.
    assert!((after(3, 3).unwrap()[2] - 114.0 / 255.0).abs() <= 1e-6);
}

#[test]
fn a_float_all_transparent_lift_refuses_and_the_identity_is_no_edit() {
    let base = float_of(&IccProfile::srgb());
    let cov = coverage(CANVAS, rect(12, 2, 4, 4));
    let err = edit16(
        &base,
        &moved("red", -6, 1, Some(P1)).to_string(),
        &cov,
        None,
    )
    .err();
    assert_eq!(err.as_deref(), Some(EMPTY));
    let none = edit16(&base, &moved("green", 0, 0, None).to_string(), &cov, None).unwrap();
    assert!(none.is_none());
}

// ---------------------------------------------------------- refusals --

#[test]
fn locks_refuse_by_the_core_kinds() {
    let cov = coverage(CANVAS, rect(2, 2, 4, 4));
    for (locks, red, alpha) in [
        (LOCK_PIXELS, Some("pixels"), Some("pixels")),
        (LOCK_TRANSPARENCY, None, Some("transparency")),
        // Lock All names the bit each kind asks first.
        (LOCK_ALL, Some("pixels"), Some("transparency")),
    ] {
        let plane = Plane::of(Depth::D8, OFFSET, SIZE, layer_px);
        let mut d8 = byte_doc(CANVAS, &plane);
        d8.layers[0].locks = locks;
        let plane = Plane::of(Depth::D16, OFFSET, SIZE, layer_px);
        let d16 = typed_doc(CANVAS, vec![typed_layer(&plane)])
            .with_layer_locks(0, locks)
            .unwrap()
            .unwrap();
        for (target, want) in [("red", red), ("alpha", alpha)] {
            let text = moved(target, 2, 1, None).to_string();
            let got8 = edit8(&d8, &text, &cov, None);
            let got16 = edit16(&d16, &text, &cov, None);
            match want {
                None => {
                    assert!(got8.unwrap().is_some(), "{target} {locks}");
                    assert!(got16.unwrap().is_some(), "{target} {locks}");
                }
                Some(which) => {
                    let lock = |name: &str| format!("layer 0 is locked ({name})");
                    assert_eq!(got8.err(), Some(lock(which)), "{target} {locks}");
                    assert_eq!(
                        got16.err(),
                        Some(format!("high-depth {}", lock(which))),
                        "{target} {locks}"
                    );
                }
            }
        }
    }
}

fn placed8() -> DrDocument {
    let base = DrDocument::from_pixels(RgbaImage::from_fn(CANVAS.0, CANVAS.1, |x, y| {
        image::Rgba(pattern(x, y))
    }));
    let source =
        DrDocument::from_pixels(RgbaImage::from_pixel(6, 6, image::Rgba([9, 99, 199, 255])));
    let text = CString::new(
        json!({"op":"place","source_id":"asset","name":"Placed","x":2,"y":2}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_mixed_source_edit(
            &base,
            &source,
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(take_err(err).is_none());
    *unsafe { Box::from_raw(out) }
}

fn placed16() -> PrecisionDocument {
    let plane = Plane::of(Depth::D16, (0, 0), CANVAS, pattern);
    let base = typed_doc(CANVAS, vec![typed_layer(&plane)]);
    let six = Plane::of(Depth::D16, (0, 0), (6, 6), |_, _| [9, 99, 199, 255]);
    let input = typed_doc((6, 6), vec![typed_layer(&six)]);
    let text = CString::new(
        json!({"op":"place","source_id":"asset","name":"Placed","x":2,"y":2}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_source_edit(
            &base,
            &input,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(take_err(err).is_none());
    *unsafe { Box::from_raw(out) }
}

#[test]
fn a_retained_source_a_group_and_an_adjustment_refuse_as_the_layer_path() {
    let cov = coverage(CANVAS, rect(2, 2, 4, 4));
    let mut text = moved("red", 1, 1, Some(P1));
    text["layer"] = json!(1);
    let retained = "Rasterize the retained source before moving its selected pixels";
    let err = edit8(&placed8(), &text.to_string(), &cov, None).err();
    assert_eq!(err.as_deref(), Some(retained));
    let err = edit16(&placed16(), &text.to_string(), &cov, None).err();
    assert_eq!(err.as_deref(), Some(retained));

    let plane = Plane::of(Depth::D8, (0, 0), CANVAS, pattern);
    let (grouped, group, _, _) = byte_doc(CANVAS, &plane).group_layers(&[0], "G").unwrap();
    let mut grouped_text = moved("blue", 1, 0, None);
    grouped_text["layer"] = json!(group);
    let err = edit8(&grouped, &grouped_text.to_string(), &cov, None)
        .err()
        .unwrap();
    assert!(err.contains("is a group"), "{err}");
    let mut adjusted = byte_doc(CANVAS, &plane)
        .adding_image_layer(0, RgbaImage::new(CANVAS.0, CANVAS.1), "Fx")
        .unwrap();
    adjusted.layers[1].meta = Some(adjustment_meta());
    let mut adjusted_text = moved("alpha", 1, 0, None);
    adjusted_text["layer"] = json!(1);
    let err = edit8(&adjusted, &adjusted_text.to_string(), &cov, None)
        .err()
        .unwrap();
    assert!(err.contains("adjustment layer"), "{err}");
    let plane16 = Plane::of(Depth::D16, (0, 0), CANVAS, pattern);
    let group =
        PrecisionDocumentLayer::isolated_normal("G", vec![typed_layer(&plane16)], 1.0, true)
            .unwrap();
    grouped_text["layer"] = json!(1);
    let err = edit16(
        &typed_doc(CANVAS, vec![group]),
        &grouped_text.to_string(),
        &cov,
        None,
    )
    .err()
    .unwrap();
    assert!(err.contains("is a group"), "{err}");
}

#[test]
fn malformed_colour_requests_refuse_at_parse() {
    let cov = coverage(CANVAS, rect(2, 2, 4, 4));
    let (_, base) = fixture(Depth::D8);
    let swatch_only =
        "swatch applies to a red, green, blue, cyan, magenta, yellow or black target only";
    for target in [
        json!("layer"),
        json!("mask"),
        json!({"channel": 0}),
        json!("alpha"),
    ] {
        let mut text = moved("red", 1, 0, Some(P1));
        text["target"] = target;
        assert_eq!(
            base.edit(&text.to_string(), &cov).err().as_deref(),
            Some(swatch_only)
        );
    }
    let mut text = moved("green", 1, 0, None);
    text["background"] = json!(9);
    assert_eq!(
        base.edit(&text.to_string(), &cov).err().as_deref(),
        Some("background applies to a mask or channel target only")
    );
    // An ink plate is a CMYK document's target (#1), refused by name here.
    let mut plate = moved("red", 1, 0, None);
    plate["target"] = json!("cyan");
    assert_eq!(
        base.edit(&plate.to_string(), &cov).err().as_deref(),
        Some("The cyan channel requires a CMYK document")
    );
    for bad in [json!("teal"), json!([256, 0, 0])] {
        let mut text = moved("red", 1, 0, None);
        if bad.is_string() {
            text["target"] = bad;
        } else {
            text["swatch"] = bad;
        }
        let err = base.edit(&text.to_string(), &cov).err().unwrap();
        assert!(err.starts_with("invalid selected pixels request"), "{err}");
    }
    // A coverage of the wrong length.
    let err = base
        .edit(&moved("red", 1, 0, None).to_string(), &cov[1..])
        .err();
    assert_eq!(
        err.as_deref(),
        Some("selected pixels coverage must be canvas-sized u16")
    );
}

#[test]
fn the_plan_charges_the_coverage_copy_for_red_green_and_blue_only() {
    for depth in DEPTHS {
        let (_, base) = fixture(depth);
        let red = base.plan(&moved("red", 3, 1, None).to_string()).unwrap();
        let alpha = base.plan(&moved("alpha", 3, 1, None).to_string()).unwrap();
        let keys: Vec<&String> = red.as_object().unwrap().keys().collect();
        assert!(keys.contains(&&"scratch_bytes".to_string()), "{depth:?}");
        let copy = 2 * u64::from(CANVAS.0 * CANVAS.1);
        assert_eq!(
            red["scratch_bytes"].as_u64().unwrap(),
            alpha["scratch_bytes"].as_u64().unwrap() + copy,
            "{depth:?}"
        );
    }
}

#[test]
fn a_cancelled_control_returns_operation_cancelled() {
    let cov = coverage(CANVAS, rect(2, 2, 4, 4));
    let text = moved("red", 2, 1, Some(P1)).to_string();
    for depth in DEPTHS {
        let control = DrJobControl::default();
        control.cancel();
        let err = match fixture(depth).1 {
            Doc::Byte(d) => edit8(&d, &text, &cov, Some(&control)).err(),
            Doc::Typed(d) => edit16(&d, &text, &cov, Some(&control)).err(),
        };
        assert_eq!(err.as_deref(), Some("operation cancelled"), "{depth:?}");
    }
}
