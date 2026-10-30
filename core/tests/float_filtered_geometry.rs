//! FILTERED geometry at 32-bit float — Image Size, straighten, arbitrary
//! canvas rotation, per-layer affine and perspective (pixels and mask) and
//! the affine/perspective SETS over a group.
//!
//! The yardstick is an independent f64 reference written out here and
//! sharing no line with `core/src/sampling.rs`: the kernel formulas restated
//! (transform taps normalized over their full support; resize taps
//! scale-aware and source-clipped), PREMULTIPLIED accumulation, the
//! unpremultiply by the RAW accumulated alpha, D2 (an accumulated alpha below
//! 2^-24 is transparent) and D3 (each RGB component clamped to the source
//! layer's own visible range, per channel over its alpha > 0 pixels).
//!
//! # The tolerance
//!
//! Each output component is compared with
//! `|Δ| ≤ K·ε₃₂·Σ|wᵢaᵢvᵢ| / a_out + ε₃₂·|ref|` and alpha with
//! `|Δ| ≤ K·ε₃₂·Σ|wᵢaᵢ|`, where `a_out` is the reference's raw accumulated
//! alpha and ε₃₂ is `f32::EPSILON`. `K` is fixed per test:
//!
//! * `K = 64` for every TRANSFORM (affine, perspective, straighten, canvas
//!   rotation, the sets): a 6×6 Lanczos tap sum is 36 f32 additions, each tap
//!   weight is an f32 product of two weights normalized in f64 and rounded
//!   once, and the premultiply is two more roundings, so 64 covers the
//!   worst-case f32 accumulation with margin and nothing else (the largest
//!   multiplier these fixtures actually need is 3.7);
//! * `K = 4096` for RESIZE: the shipped separable resize evaluates its kernel
//!   in f32 (`resize_weights`: the centre, the scale and the kernel in f32),
//!   so each tap weight carries an ABSOLUTE error of a few ε₃₂ whatever its
//!   size, and a tiny tail weight beside a ±65504 sample is off by far more
//!   than ε₃₂ relative to its own term. The largest multiplier these fixtures
//!   need is 1250 (a 23→9 Catmull-Rom reduction next to ±65504); 4096 keeps
//!   that margin and still pins every component to about 5·10⁻⁴ of the
//!   magnitude summed into it.
//!
//! The in-memory fixtures (written as 32-bit float TIFFs and opened through
//! the shipped reader) carry negatives, values to ±65504, partial alpha, an
//! alpha far below 2^-96 and alpha-0 pixels whose HIDDEN RGB lies outside the
//! visible range — so "hidden RGB never leaks" is the visible-range assertion
//! itself. The committed 11F scene is used too and skipped when absent, as
//! `float_exact_geometry.rs` does.
#![allow(clippy::needless_range_loop)]

use std::ffi::CString;
use std::ptr;

use doctored_core::doc_mask::MaskProperties;
use doctored_core::doc_rotate::rotated_canvas;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::tiff_float::{encode_to_vec, FloatTiff};
use serde_json::{json, Value};

use crate::inverse_mapping;
use inverse_mapping::{bounds, invert};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";
/// The fixed tolerance multipliers; see the module doc.
const K: f64 = 64.0;
const K_RESIZE: f64 = 4096.0;
const EPS: f64 = f32::EPSILON as f64;
/// D2's output floor, 2^-24, `sampling::EXTENDED_ALPHA_FLOOR` restated.
const FLOOR: f64 = 1.0 / 16_777_216.0;
const W: u32 = 23;
const H: u32 = 17;

// ---------------------------------------------------------------- fixtures --

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

/// Straight RGBA. Columns 0..3 are alpha 0 with LOUD hidden RGB (outside the
/// visible range unless `extremes` puts ±65504 in the visible set too), one
/// of them carries an alpha far below 2^-96, column 3 is partial alpha and
/// the rest is opaque or partial with values in [-3, 30].
fn scene(w: u32, h: u32, extremes: bool) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let r = hash(i);
            let v = |shift: u32| ((r >> shift) & 0xff) as f32 / 255.0 * 33.0 - 3.0;
            if x < 3 {
                if x == 1 && y == h / 2 {
                    return [5.0, 5.0, 5.0, 1e-31];
                }
                let hidden = if r & 1 == 0 { 60000.0 } else { -60000.0 };
                return [hidden, -hidden, hidden, 0.0];
            }
            let alpha = if x == 3 || r.is_multiple_of(7) {
                ((r >> 4) % 1000) as f32 / 1000.0 * 0.9 + 0.05
            } else {
                1.0
            };
            let mut p = [v(8), v(16), v(24), alpha];
            if extremes && (x, y) == (w / 2, h / 2) {
                p = [65504.0, -65504.0, 65504.0, 1.0];
            }
            if extremes && (x, y) == (w / 2 + 3, h / 2 - 2) {
                p = [-65504.0, 65504.0, -1.0, 1.0];
            }
            p
        })
        .collect()
}

fn float_doc(w: u32, h: u32, pixels: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width: w,
        height: h,
        channels: 4,
        alpha: Some(false),
        samples: pixels.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("filtered.tiff");
    std::fs::write(&path, encode_to_vec(&tiff, false).unwrap()).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(doc.is_float());
    assert_eq!(
        source(&doc, 0)
            .iter()
            .flatten()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        pixels
            .iter()
            .flatten()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        "the fixture opens bit-exact"
    );
    doc
}

fn scene_fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FIXTURE).exists() {
        return None;
    }
    Some(PrecisionDocument::open_preserving(FIXTURE, LIMIT, 0, None).expect("the fixture"))
}

fn mask_samples(w: u32, h: u32) -> Vec<u16> {
    (0..w * h).map(|i| (hash(i + 7) % 65_536) as u16).collect()
}

// ----------------------------------------------------------------- helpers --

fn parse(value: &Value) -> GeometryEdit {
    GeometryEdit::parse(&value.to_string()).expect("a geometry request")
}

fn apply(doc: &PrecisionDocument, value: &Value) -> PrecisionDocument {
    doc.geometry(&parse(value), LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("{value} runs: {e}"))
        .unwrap_or_else(|| panic!("{value} changes something"))
}

fn ffi_edit(doc: &PrecisionDocument, value: &Value) -> PrecisionDocument {
    let text = CString::new(value.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !error.is_null() {
        let text = unsafe { std::ffi::CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { doctored_core::ffi::dr_string_free(error) };
        panic!("{value} reported: {text}");
    }
    assert!(!out.is_null(), "{value} returned nothing");
    *unsafe { Box::from_raw(out) }
}

fn source(doc: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    let image = doc.layer_source(index).expect("a raster record");
    assert_eq!(image.storage(), PrecisionStorage::Rgba32Float);
    image.to_rgba32().expect("float storage")
}

fn extent(doc: &PrecisionDocument, index: usize) -> (i32, i32, u32, u32) {
    let (x, y) = doc.layer(index).unwrap().offset();
    let (w, h) = doc.layer_source(index).unwrap().dimensions();
    (x, y, w, h)
}

/// D3's range, restated: per-channel min/max over alpha > 0.
fn visible_range(pixels: &[[f32; 4]]) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in pixels.iter().filter(|p| p[3] > 0.0) {
        for c in 0..3 {
            min[c] = min[c].min(f64::from(p[c]));
            max[c] = max[c].max(f64::from(p[c]));
        }
    }
    if min[0].is_infinite() {
        return ([0.0; 3], [0.0; 3]);
    }
    (min, max)
}

// --------------------------------------------------------------- reference --

fn radius(kind: &str) -> i64 {
    match kind {
        "bilinear" => 1,
        "bicubic" => 2,
        "lanczos3" => 3,
        other => panic!("no kernel {other}"),
    }
}

fn weight(kind: &str, t: f64) -> f64 {
    let x = t.abs();
    match kind {
        "bilinear" => (1.0 - x).max(0.0),
        // Catmull-Rom (B = 0, C = 0.5).
        "bicubic" if x < 1.0 => (1.5 * x - 2.5) * x * x + 1.0,
        "bicubic" if x < 2.0 => ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0,
        "bicubic" => 0.0,
        // sinc(x)·sinc(x/3).
        "lanczos3" if x < 1e-6 => 1.0,
        "lanczos3" if x < 3.0 => {
            let px = std::f64::consts::PI * x;
            3.0 * px.sin() * (px / 3.0).sin() / (px * px)
        }
        "lanczos3" => 0.0,
        other => panic!("no kernel {other}"),
    }
}

/// Transform taps: the full support around `s` (integers + 0.5 are pixel
/// centres), normalized; the first tap's index.
fn transform_taps(kind: &str, s: f64) -> (i64, Vec<f64>) {
    let r = radius(kind);
    let p = s - 0.5;
    let base = p.floor();
    let mut w: Vec<f64> = (0..2 * r)
        .map(|t| weight(kind, p - (base + (t - (r - 1)) as f64)))
        .collect();
    let sum: f64 = w.iter().sum();
    if sum != 0.0 {
        w.iter_mut().for_each(|v| *v /= sum);
    }
    (base as i64 - (r - 1), w)
}

/// Resize taps: scale-aware support, clipped to the source, then normalized.
fn resize_taps(kind: &str, source: u32, target: u32, index: u32) -> Vec<(usize, f64)> {
    let ratio = f64::from(source) / f64::from(target);
    let scale = ratio.max(1.0);
    let center = (f64::from(index) + 0.5) * ratio;
    let support = radius(kind) as f64 * scale;
    let first = ((center - support).floor() as i64).clamp(0, i64::from(source) - 1);
    let end = ((center + support).ceil() as i64).clamp(first + 1, i64::from(source));
    let mut taps: Vec<(usize, f64)> = (first..end)
        .map(|i| (i as usize, weight(kind, (i as f64 + 0.5 - center) / scale)))
        .collect();
    let sum: f64 = taps.iter().map(|t| t.1).sum();
    taps.iter_mut().for_each(|t| t.1 /= sum);
    taps
}

/// One output pixel's raw premultiplied accumulator and the absolute sums
/// the tolerance is built from.
#[derive(Default)]
struct Acc {
    sum: [f64; 4],
    abs: [f64; 4],
}

impl Acc {
    fn add(&mut self, w: f64, p: [f32; 4]) {
        let wa = w * f64::from(p[3]);
        for c in 0..3 {
            self.sum[c] += wa * f64::from(p[c]);
            self.abs[c] += (wa * f64::from(p[c])).abs();
        }
        self.sum[3] += wa;
        self.abs[3] += wa.abs();
    }
}

/// The reference transform: every destination pixel centre of `dest`
/// (canvas coordinates) mapped back through `map` into canvas space, then
/// into the source plane at `offset`; transparent outside the source.
fn reference_transform(
    pixels: &[[f32; 4]],
    dims: (u32, u32),
    offset: (i32, i32),
    dest: (i32, i32, u32, u32),
    map: impl Fn(f64, f64) -> (f64, f64),
    kind: &str,
) -> Vec<Acc> {
    let (sw, sh) = (i64::from(dims.0), i64::from(dims.1));
    let mut out = Vec::new();
    for j in 0..dest.3 {
        for i in 0..dest.2 {
            let (x, y) = map(
                f64::from(dest.0) + f64::from(i) + 0.5,
                f64::from(dest.1) + f64::from(j) + 0.5,
            );
            let (u, v) = (x - f64::from(offset.0), y - f64::from(offset.1));
            let (x0, wx) = transform_taps(kind, u);
            let (y0, wy) = transform_taps(kind, v);
            let mut acc = Acc::default();
            for (ty, &wyt) in wy.iter().enumerate() {
                let sy = y0 + ty as i64;
                if sy < 0 || sy >= sh {
                    continue;
                }
                for (tx, &wxt) in wx.iter().enumerate() {
                    let sx = x0 + tx as i64;
                    if sx < 0 || sx >= sw {
                        continue;
                    }
                    acc.add(wyt * wxt, pixels[(sy * sw + sx) as usize]);
                }
            }
            out.push(acc);
        }
    }
    out
}

fn reference_resize(
    pixels: &[[f32; 4]],
    dims: (u32, u32),
    target: (u32, u32),
    kind: &str,
) -> Vec<Acc> {
    let mut out = Vec::new();
    for y in 0..target.1 {
        let wy = resize_taps(kind, dims.1, target.1, y);
        for x in 0..target.0 {
            let wx = resize_taps(kind, dims.0, target.0, x);
            let mut acc = Acc::default();
            for &(sy, wyt) in &wy {
                for &(sx, wxt) in &wx {
                    acc.add(wyt * wxt, pixels[sy * dims.0 as usize + sx]);
                }
            }
            out.push(acc);
        }
    }
    out
}

/// Compares a kernel result with the reference under the module doc's
/// tolerance, and asserts D2 (no alpha in (0, 2^-24)) and D3 (RGB within the
/// source's visible range — which is also "hidden RGB never leaks", since
/// every fixture's hidden RGB lies outside that range).
fn assert_matches(got: &[[f32; 4]], want: &[Acc], source: &[[f32; 4]], k: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    let (min, max) = visible_range(source);
    for (index, (g, r)) in got.iter().zip(want).enumerate() {
        let a = f64::from(g[3]);
        assert!(
            a == 0.0 || a >= FLOOR,
            "{what}: alpha {a:e} at {index} is inside (0, 2^-24)"
        );
        let alpha_tolerance = k * EPS * r.abs[3];
        let transparent = r.sum[3].is_nan() || r.sum[3] < FLOOR;
        if a == 0.0 {
            assert_eq!(
                g.map(f32::to_bits),
                [0u32; 4],
                "{what}: a transparent result at {index} stores zeros"
            );
            assert!(
                transparent || r.sum[3] <= FLOOR + alpha_tolerance,
                "{what}: {index} is transparent against the reference's alpha {:e}",
                r.sum[3]
            );
            continue;
        }
        for c in 0..3 {
            let v = f64::from(g[c]);
            assert!(
                v >= min[c] && v <= max[c],
                "{what}: channel {c} = {v} at {index} leaves the visible range [{}, {}]",
                min[c],
                max[c]
            );
        }
        if transparent {
            assert!(
                a <= alpha_tolerance,
                "{what}: {index} has alpha {a:e} where the reference is transparent"
            );
            continue;
        }
        let want_alpha = r.sum[3].min(1.0);
        assert!(
            (a - want_alpha).abs() <= alpha_tolerance,
            "{what}: alpha at {index} is {a} against {want_alpha}"
        );
        for c in 0..3 {
            let want_c = (r.sum[c] / r.sum[3]).clamp(min[c], max[c]);
            let tolerance = k * EPS * r.abs[c] / r.sum[3] + EPS * want_c.abs();
            let v = f64::from(g[c]);
            assert!(
                (v - want_c).abs() <= tolerance,
                "{what}: channel {c} at {index} is {v} against {want_c} (tolerance {tolerance:e})"
            );
        }
    }
}

/// The 3×3 homography taking `rect`'s corners (TL, TR, BR, BL) onto `quad`,
/// solved here by Gaussian elimination on the eight point equations, then
/// inverted by its adjugate.
fn inverse_homography(rect: [f64; 4], quad: [f64; 8]) -> [f64; 9] {
    let [x, y, w, h] = rect;
    let from = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    let mut m = [[0.0f64; 9]; 8];
    for (k, &(sx, sy)) in from.iter().enumerate() {
        let (dx, dy) = (quad[2 * k], quad[2 * k + 1]);
        m[2 * k] = [sx, sy, 1.0, 0.0, 0.0, 0.0, -dx * sx, -dx * sy, dx];
        m[2 * k + 1] = [0.0, 0.0, 0.0, sx, sy, 1.0, -dy * sx, -dy * sy, dy];
    }
    for col in 0..8 {
        let pivot = (col..8)
            .max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))
            .unwrap();
        m.swap(col, pivot);
        for row in 0..8 {
            if row != col {
                let f = m[row][col] / m[col][col];
                for k in col..9 {
                    m[row][k] -= f * m[col][k];
                }
            }
        }
    }
    let p: Vec<f64> = (0..8).map(|i| m[i][8] / m[i][i]).collect();
    let f = [p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], 1.0];
    [
        f[4] * f[8] - f[5] * f[7],
        f[2] * f[7] - f[1] * f[8],
        f[1] * f[5] - f[2] * f[4],
        f[5] * f[6] - f[3] * f[8],
        f[0] * f[8] - f[2] * f[6],
        f[2] * f[3] - f[0] * f[5],
        f[3] * f[7] - f[4] * f[6],
        f[1] * f[6] - f[0] * f[7],
        f[0] * f[4] - f[1] * f[3],
    ]
}

fn projective(inverse: [f64; 9]) -> impl Fn(f64, f64) -> (f64, f64) + Copy {
    move |x, y| {
        let q = inverse[6] * x + inverse[7] * y + inverse[8];
        (
            (inverse[0] * x + inverse[1] * y + inverse[2]) / q,
            (inverse[3] * x + inverse[4] * y + inverse[5]) / q,
        )
    }
}

fn affine_map(matrix: [f64; 6]) -> impl Fn(f64, f64) -> (f64, f64) + Copy {
    let m = invert(matrix);
    move |x, y| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// Every raster of `out` against the reference resample of the matching
/// raster of `base` through `map`, on `out`'s own destination grid.
fn assert_layers(
    base: &PrecisionDocument,
    out: &PrecisionDocument,
    layers: &[usize],
    map: impl Fn(f64, f64) -> (f64, f64) + Copy,
    kind: &str,
    what: &str,
) {
    for &index in layers {
        let pixels = source(base, index);
        let (x, y, w, h) = extent(base, index);
        let dest = extent(out, index);
        let want = reference_transform(&pixels, (w, h), (x, y), dest, map, kind);
        assert_matches(
            &source(out, index),
            &want,
            &pixels,
            K,
            &format!("{what} layer {index}"),
        );
    }
}

// ------------------------------------------------------------------- tests --

#[test]
fn fractional_affine_matches_the_reference_for_every_kernel() {
    for extremes in [false, true] {
        let base = float_doc(W, H, &scene(W, H, extremes));
        let matrix = [
            1.37 * 0.96,
            1.37 * 0.28,
            -1.37 * 0.28,
            1.37 * 0.96,
            4.25,
            -1.5,
        ];
        for kind in ["bilinear", "bicubic", "lanczos3"] {
            let out = apply(
                &base,
                &json!({"op": "affine", "layer": 0, "target": "pixels",
                        "matrix": matrix, "filter": kind}),
            );
            assert_eq!(
                extent(&out, 0),
                bounds((0, 0), (W, H), matrix),
                "{kind}: the extent"
            );
            assert_layers(
                &base,
                &out,
                &[0],
                affine_map(matrix),
                kind,
                &format!("affine {kind} extremes={extremes}"),
            );
        }
    }
}

#[test]
fn perspective_matches_the_reference() {
    let base = float_doc(W, H, &scene(W, H, true));
    let quad = [1.0, 2.0, 21.5, 0.5, 24.0, 18.0, -1.5, 15.0];
    for kind in ["bilinear", "lanczos3"] {
        let out = apply(
            &base,
            &json!({"op": "perspective", "layer": 0, "target": "pixels",
                    "quad": quad, "filter": kind}),
        );
        let inverse = inverse_homography([0.0, 0.0, f64::from(W), f64::from(H)], quad);
        assert_layers(
            &base,
            &out,
            &[0],
            projective(inverse),
            kind,
            &format!("perspective {kind}"),
        );
    }
}

#[test]
fn resize_up_and_down_matches_the_reference() {
    let pixels = scene(W, H, true);
    let base = float_doc(W, H, &pixels);
    for (target, kind) in [
        ((37, 29), "bilinear"),
        ((41, 30), "lanczos3"),
        ((9, 7), "bicubic"),
        ((7, 5), "lanczos3"),
    ] {
        let out = apply(
            &base,
            &json!({"op": "resize", "width": target.0, "height": target.1, "filter": kind}),
        );
        assert_eq!(out.dimensions(), target);
        assert_eq!(extent(&out, 0), (0, 0, target.0, target.1));
        let want = reference_resize(&pixels, (W, H), target, kind);
        assert_matches(
            &source(&out, 0),
            &want,
            &pixels,
            K_RESIZE,
            &format!("resize {target:?} {kind}"),
        );
    }
}

#[test]
fn straighten_and_rotate_canvas_match_the_reference() {
    let base = float_doc(W, H, &scene(W, H, false));
    // Straighten: the per-layer matrix is `m` shifted by the window.
    let (s, c) = (0.12f64.sin(), 0.12f64.cos());
    let (cx, cy) = (f64::from(W) / 2.0, f64::from(H) / 2.0);
    let m = [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy];
    let window = (2u32, 1u32, W - 4, H - 3);
    let out = apply(
        &base,
        &json!({"op": "straighten", "matrix": m, "filter": "bicubic",
                "x": window.0, "y": window.1, "width": window.2, "height": window.3}),
    );
    assert_eq!(out.dimensions(), (window.2, window.3));
    let shifted = [
        m[0],
        m[1],
        m[2],
        m[3],
        m[4] - f64::from(window.0),
        m[5] - f64::from(window.1),
    ];
    assert_eq!(extent(&out, 0), bounds((0, 0), (W, H), shifted));
    assert_layers(
        &base,
        &out,
        &[0],
        affine_map(shifted),
        "bicubic",
        "straighten",
    );

    // Arbitrary canvas rotation at 7.5 degrees, through the shared plan's
    // matrix (the matrix is the plan's input, not the oracle's subject).
    let plan = rotated_canvas(W, H, 7.5).unwrap();
    for kind in ["bilinear", "lanczos3"] {
        let out = apply(
            &base,
            &json!({"op": "rotate_canvas", "degrees": 7.5, "filter": kind}),
        );
        assert_eq!(out.dimensions(), (plan.width, plan.height));
        assert_layers(
            &base,
            &out,
            &[0],
            affine_map(plan.matrix),
            kind,
            &format!("rotate_canvas {kind}"),
        );
    }
}

/// Two rasters in a group (one moved by an integer translation, which is a
/// `Keep`), transformed as a SET through the group record.
#[test]
fn affine_and_perspective_sets_over_a_group_match_the_reference() {
    let base = float_doc(W, H, &scene(W, H, true));
    let base = base.duplicate_layer(0).unwrap().unwrap();
    assert_eq!(base.layer_count(), 2);
    let moved = apply(
        &base,
        &json!({"op": "affine", "layer": 1, "target": "pixels",
                "matrix": [1.0, 0.0, 0.0, 1.0, 3.0, -2.0], "filter": "lanczos3"}),
    );
    assert!(
        PrecisionImage::ptr_eq(
            base.layer_source(1).unwrap(),
            moved.layer_source(1).unwrap()
        ),
        "an integer translation keeps the f32 allocation"
    );
    let (grouped, _) = moved.group_layers(&[0, 1], "Set", false).unwrap().unwrap();
    let rasters: Vec<usize> = (0..grouped.layer_count())
        .filter(|&i| grouped.layer_source(i).is_some())
        .collect();
    let group = (0..grouped.layer_count())
        .find(|&i| grouped.layer_source(i).is_none())
        .expect("the group record");
    assert_eq!(rasters.len(), 2);

    let matrix = [0.9, -0.2, 0.25, 1.1, 2.5, 3.25];
    let out = apply(
        &grouped,
        &json!({"op": "affine_set", "layers": [group], "matrix": matrix,
                "filter": "bicubic"}),
    );
    assert_layers(
        &grouped,
        &out,
        &rasters,
        affine_map(matrix),
        "bicubic",
        "affine_set",
    );

    let rect = [0.0, 0.0, f64::from(W), f64::from(H)];
    let quad = [2.0, 1.0, 22.0, -1.0, 25.5, 19.0, -0.5, 16.0];
    let out = apply(
        &grouped,
        &json!({"op": "perspective_set", "layers": [group], "source_rect": rect,
                "quad": quad, "filter": "lanczos3"}),
    );
    assert_layers(
        &grouped,
        &out,
        &rasters,
        projective(inverse_homography(rect, quad)),
        "lanczos3",
        "perspective_set",
    );
}

/// Auto-Align's warp: `perspective_set` over ONE raster at a non-zero
/// offset inside a larger canvas, the quad being where a KNOWN 3×3 matrix
/// sends the layer's corners. The oracle's map is that matrix's inverse by
/// cofactors, never the quad solved back, so the core's own solve is what is
/// measured. The integer translation is a `Keep`.
#[test]
fn a_known_homography_perspective_set_matches_the_reference() {
    let (lw, lh) = (40u32, 30u32);
    let (ox, oy) = (21i32, 17i32);
    let single = float_doc(lw, lh, &scene(lw, lh, true));
    let placed = ffi_edit(
        &single,
        &json!({"op": "canvas_size", "width": 96, "height": 72, "x": ox, "y": oy}),
    );
    assert_eq!(placed.dimensions(), (96, 72));
    assert_eq!(extent(&placed, 0), (ox, oy, lw, lh));
    let base = placed.duplicate_layer(0).unwrap().unwrap();
    assert_eq!(base.layer_count(), 2);
    assert_eq!(extent(&base, 1), (ox, oy, lw, lh));

    let (x, y) = (f64::from(ox), f64::from(oy));
    let (w, h) = (f64::from(lw), f64::from(lh));
    let rect = [x, y, w, h];
    let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    // Row-major 3×3: (x, y) -> (m0·x + m1·y + m2, m3·x + m4·y + m5) / (m6·x + m7·y + m8).
    let forward = |m: &[f64; 9], px: f64, py: f64| {
        let q = m[6] * px + m[7] * py + m[8];
        (
            (m[0] * px + m[1] * py + m[2]) / q,
            (m[3] * px + m[4] * py + m[5]) / q,
        )
    };
    let quad_of = |m: &[f64; 9]| {
        let mut quad = [0.0f64; 8];
        for (k, &(cx, cy)) in corners.iter().enumerate() {
            let (qx, qy) = forward(m, cx, cy);
            quad[2 * k] = qx;
            quad[2 * k + 1] = qy;
        }
        quad
    };
    let cofactor_inverse = |m: &[f64; 9]| {
        let adjugate = [
            m[4] * m[8] - m[5] * m[7],
            m[2] * m[7] - m[1] * m[8],
            m[1] * m[5] - m[2] * m[4],
            m[5] * m[6] - m[3] * m[8],
            m[0] * m[8] - m[2] * m[6],
            m[2] * m[3] - m[0] * m[5],
            m[3] * m[7] - m[4] * m[6],
            m[1] * m[6] - m[0] * m[7],
            m[0] * m[4] - m[1] * m[3],
        ];
        let det = m[0] * adjugate[0] + m[1] * adjugate[3] + m[2] * adjugate[6];
        adjugate.map(|v| v / det)
    };
    let outward = |quad: &[f64; 8]| {
        let snap = |v: f64| {
            if (v - v.round()).abs() <= 1e-9 {
                v.round()
            } else {
                v
            }
        };
        let xs: Vec<f64> = (0..4).map(|k| snap(quad[2 * k])).collect();
        let ys: Vec<f64> = (0..4).map(|k| snap(quad[2 * k + 1])).collect();
        let left = xs.iter().cloned().fold(f64::INFINITY, f64::min).floor();
        let top = ys.iter().cloned().fold(f64::INFINITY, f64::min).floor();
        let right = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max).ceil();
        let bottom = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max).ceil();
        (
            left as i32,
            top as i32,
            (right - left) as u32,
            (bottom - top) as u32,
        )
    };

    // 2° about the layer's centre, scale 1.02, then a shift of (3.5, −2.25).
    let angle = 2.0f64.to_radians();
    let (a, b) = (1.02 * angle.cos(), 1.02 * angle.sin());
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let rotation = [
        a,
        -b,
        cx - a * cx + b * cy + 3.5,
        b,
        a,
        cy - b * cx - a * cy - 2.25,
        0.0,
        0.0,
        1.0,
    ];
    let perspective = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 4e-3, -3e-3, 1.0];
    for (name, matrix) in [("rotation", rotation), ("perspective", perspective)] {
        let quad = quad_of(&matrix);
        for kind in ["bilinear", "bicubic"] {
            let out = ffi_edit(
                &base,
                &json!({"op": "perspective_set", "layers": [1], "source_rect": rect,
                        "quad": quad, "filter": kind}),
            );
            assert_eq!(out.dimensions(), (96, 72), "{name} {kind}: the canvas");
            assert_eq!(extent(&out, 1), outward(&quad), "{name} {kind}: the extent");
            assert_eq!(extent(&out, 0), extent(&base, 0), "{name} {kind}: layer 0");
            assert!(
                PrecisionImage::ptr_eq(base.layer_source(0).unwrap(), out.layer_source(0).unwrap()),
                "{name} {kind}: the other layer keeps its allocation"
            );
            assert_layers(
                &base,
                &out,
                &[1],
                projective(cofactor_inverse(&matrix)),
                kind,
                &format!("known homography {name} {kind}"),
            );
        }
    }

    let translation = [1.0, 0.0, 7.0, 0.0, 1.0, -4.0, 0.0, 0.0, 1.0];
    let moved = ffi_edit(
        &base,
        &json!({"op": "perspective_set", "layers": [1], "source_rect": rect,
                "quad": quad_of(&translation), "filter": "bicubic"}),
    );
    assert!(
        PrecisionImage::ptr_eq(
            base.layer_source(1).unwrap(),
            moved.layer_source(1).unwrap()
        ),
        "an integer translation quad is a Keep: the same allocation"
    );
    assert_eq!(extent(&moved, 1), (ox + 7, oy - 4, lw, lh));
    assert_eq!(extent(&moved, 0), extent(&base, 0));
}

/// Masks stay u16 at float: a mask-target affine is the SAME u16 walk a
/// 16-bit document runs, sample for sample.
#[test]
fn a_mask_target_affine_equals_the_same_mask_on_a_u16_document() {
    let samples = mask_samples(W, H);
    let mask = || {
        PrecisionMask::new(
            PrecisionPlane::from_u16(W, H, samples.clone(), None).unwrap(),
            MaskProperties::at((0, 0)),
            true,
        )
        .unwrap()
    };
    let float = float_doc(W, H, &scene(W, H, false))
        .set_layer_mask(0, Some(mask()), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let u16_doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            W,
            H,
            vec![[1000, 2000, 3000, 65535]; (W * H) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
    .set_layer_mask(0, Some(mask()), LIMIT, 0, None)
    .unwrap()
    .unwrap();
    for kind in ["bilinear", "bicubic", "lanczos3"] {
        let request = json!({"op": "affine", "layer": 0, "target": "mask",
                             "matrix": [1.1, 0.2, -0.15, 0.95, 1.5, -0.75], "filter": kind});
        let a = apply(&float, &request);
        let b = apply(&u16_doc, &request);
        let (ma, mb) = (
            a.layer(0).unwrap().mask().unwrap(),
            b.layer(0).unwrap().mask().unwrap(),
        );
        assert_eq!(ma.properties().offset, mb.properties().offset, "{kind}");
        assert_eq!(ma.plane().dimensions(), mb.plane().dimensions(), "{kind}");
        assert_eq!(ma.plane().samples(), mb.plane().samples(), "{kind}");
        assert!(
            PrecisionImage::ptr_eq(a.layer_source(0).unwrap(), float.layer_source(0).unwrap()),
            "{kind}: the f32 pixels are untouched by a mask transform"
        );
    }
}

/// Nearest is a bit copy: every destination pixel is the covering source
/// pixel's exact bits — hidden RGB included — or transparent zero.
#[test]
fn nearest_is_a_bit_copy_in_transform_and_resize() {
    let pixels = scene(W, H, true);
    let base = float_doc(W, H, &pixels);
    let matrix = [1.37, 0.31, -0.29, 1.21, 0.37, 0.61];
    let out = apply(
        &base,
        &json!({"op": "affine", "layer": 0, "target": "pixels",
                "matrix": matrix, "filter": "nearest"}),
    );
    let dest = extent(&out, 0);
    let got = source(&out, 0);
    let map = affine_map(matrix);
    let mut compared = 0;
    for j in 0..dest.3 {
        for i in 0..dest.2 {
            let (u, v) = map(
                f64::from(dest.0) + f64::from(i) + 0.5,
                f64::from(dest.1) + f64::from(j) + 0.5,
            );
            // A centre within 1e-6 of a pixel edge may round either way in
            // the shipped incremental map; it is not a bit-copy question.
            if (u - u.round()).abs() < 1e-6 || (v - v.round()).abs() < 1e-6 {
                continue;
            }
            let (x, y) = (u.floor(), v.floor());
            let want = if x >= 0.0 && y >= 0.0 && x < f64::from(W) && y < f64::from(H) {
                pixels[y as usize * W as usize + x as usize]
            } else {
                [0.0; 4]
            };
            let g = got[(j * dest.2 + i) as usize];
            assert_eq!(g.map(f32::to_bits), want.map(f32::to_bits), "({i},{j})");
            compared += 1;
        }
    }
    assert!(compared > (dest.2 * dest.3) as usize / 2);

    for target in [(37u32, 29u32), (9, 7)] {
        let out = apply(
            &base,
            &json!({"op": "resize", "width": target.0, "height": target.1, "filter": "nearest"}),
        );
        let got = source(&out, 0);
        for y in 0..target.1 {
            let sy = ((2 * u64::from(y) + 1) * u64::from(H) / (2 * u64::from(target.1))) as usize;
            for x in 0..target.0 {
                let sx =
                    ((2 * u64::from(x) + 1) * u64::from(W) / (2 * u64::from(target.0))) as usize;
                assert_eq!(
                    got[(y * target.0 + x) as usize].map(f32::to_bits),
                    pixels[sy * W as usize + sx].map(f32::to_bits),
                    "resize {target:?} ({x},{y})"
                );
            }
        }
    }
}

#[test]
fn a_filtered_float_result_survives_the_v40_round_trip_bit_exact() {
    let base = float_doc(W, H, &scene(W, H, true));
    for request in [
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": [1.2, 0.3, -0.3, 1.2, 0.5, 0.25], "filter": "lanczos3"}),
        json!({"op": "resize", "width": 31, "height": 12, "filter": "bicubic"}),
        json!({"op": "rotate_canvas", "degrees": 7.5, "filter": "bilinear"}),
    ] {
        let out = apply(&base, &request);
        let bytes = out.encode_native(LIMIT, 0, None).expect("v40 bytes");
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
        assert!(reopened.is_float(), "{request}");
        assert_eq!(extent(&reopened, 0), extent(&out, 0), "{request}");
        assert_eq!(
            source(&reopened, 0)
                .iter()
                .flatten()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            source(&out, 0)
                .iter()
                .flatten()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            "{request}: every f32 bit survives"
        );
        assert_eq!(
            reopened.encode_native(LIMIT, 0, None).expect("resave"),
            bytes,
            "{request}: reopen then resave is identical"
        );
    }
}

#[test]
fn identity_and_integer_translation_change_nothing_they_need_not() {
    let base = float_doc(W, H, &scene(W, H, true));
    for request in [
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], "filter": "lanczos3"}),
        json!({"op": "resize", "width": W, "height": H, "filter": "lanczos3"}),
        json!({"op": "rotate_canvas", "degrees": 0.0, "filter": "bicubic"}),
    ] {
        assert!(
            base.geometry(&parse(&request), LIMIT, 0, None)
                .expect("an identity is not an error")
                .is_none(),
            "{request} is an exact no-op"
        );
    }
    let moved = apply(
        &base,
        &json!({"op": "affine", "layer": 0, "target": "pixels",
                "matrix": [1.0, 0.0, 0.0, 1.0, -4.0, 6.0], "filter": "bicubic"}),
    );
    assert!(
        PrecisionImage::ptr_eq(
            base.layer_source(0).unwrap(),
            moved.layer_source(0).unwrap()
        ),
        "an integer translation is a Keep: the same allocation"
    );
    assert_eq!(moved.layer(0).unwrap().offset(), (-4, 6));
}

#[test]
fn a_cancelled_filtered_job_errs_with_no_result() {
    let base = float_doc(W, H, &scene(W, H, true));
    for request in [
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": [1.2, 0.3, -0.3, 1.2, 0.5, 0.25], "filter": "lanczos3"}),
        json!({"op": "resize", "width": 41, "height": 30, "filter": "lanczos3"}),
    ] {
        let control = DrJobControl::default();
        control.cancel();
        let message = match base.geometry(&parse(&request), LIMIT, 0, Some(&control)) {
            Err(message) => message,
            Ok(_) => panic!("{request}: a cancelled job must not produce a result"),
        };
        assert!(message.contains("cancel"), "{request}: {message}");
        assert_eq!(
            control.scratch_bytes(),
            0,
            "{request}: nothing stays reserved"
        );
    }
}

/// A float raster past 67,108,864 px (a 1 GiB f32 plane) whose native
/// reopen the 2 GiB reference limit cannot hold is refused by the native plan
/// inside `geometry_plan`, before any destination exists.
#[test]
fn a_float_upscale_past_the_contiguous_limit_refuses_at_plan_time() {
    let base = float_doc(W, H, &scene(W, H, false));
    let request = json!({"op": "resize", "width": 10000, "height": 9000, "filter": "bilinear"});
    // 10000 × 9000 = 90,000,000 px, past 67,108,864. The streamed reopen
    // charges 16 B/px decoded and the 9 B/px float margin, about
    // 2.25 GB, past the 2 GiB limit (a 72 MP result, about 1.81 GB, now fits
    // since the file is no longer charged).
    let message = match base.geometry_plan(&parse(&request)) {
        Err(message) => message,
        Ok(_) => panic!("a 90 MP float result must refuse at plan time"),
    };
    assert!(!message.is_empty());
    assert!(base.geometry(&parse(&request), LIMIT, 0, None).is_err());
}

#[test]
fn the_committed_scene_matches_the_reference() {
    let Some(base) = scene_fixture() else {
        return;
    };
    assert!(base.is_float());
    let matrix = [
        1.37 * 0.96,
        1.37 * 0.28,
        -1.37 * 0.28,
        1.37 * 0.96,
        2.5,
        -3.5,
    ];
    for kind in ["bicubic", "lanczos3"] {
        let out = apply(
            &base,
            &json!({"op": "affine", "layer": 0, "target": "pixels",
                    "matrix": matrix, "filter": kind}),
        );
        assert_layers(
            &base,
            &out,
            &[0],
            affine_map(matrix),
            kind,
            &format!("scene affine {kind}"),
        );
    }
    let (w, h) = base.dimensions();
    let target = (w * 3 / 2, h * 2 / 3);
    let out = apply(
        &base,
        &json!({"op": "resize", "width": target.0, "height": target.1, "filter": "lanczos3"}),
    );
    let pixels = source(&base, 0);
    let (x, y, sw, sh) = extent(&base, 0);
    if (x, y, sw, sh) == (0, 0, w, h) {
        assert_matches(
            &source(&out, 0),
            &reference_resize(&pixels, (w, h), target, "lanczos3"),
            &pixels,
            K_RESIZE,
            "scene resize lanczos3",
        );
    }
    assert!(
        source(&out, 0)
            .iter()
            .any(|p| p[3] > 0.0 && p[..3].iter().any(|v| *v > 1.0)),
        "an above-white value survives the resample"
    );
}

/// Regression: a Lanczos affine of a layer with HDR values at its alpha
/// edge, then Exposure +2 above it, then render and merge all succeed — D2
/// and D3 keep every output inside what the float renderer admits.
#[test]
fn a_lanczos_hdr_edge_then_exposure_renders_and_merges() {
    let mut pixels = scene(W, H, false);
    for y in 0..H {
        // ×4 under Exposure +2 is 64000 — inside ±65504 only if the Lanczos
        // overshoot is clamped to the layer's range (D3).
        pixels[(y * W + 3) as usize] = [16000.0, 0.25, -2.0, 1.0];
        pixels[(y * W + 4) as usize] = [-16000.0, 15000.0, 3.0, 0.5];
    }
    let base = float_doc(W, H, &pixels);
    let out = apply(
        &base,
        &json!({"op": "affine", "layer": 0, "target": "pixels",
                "matrix": [1.1, 0.35, -0.35, 1.1, 0.4, -0.3], "filter": "lanczos3"}),
    );
    let exposed = out
        .add_adjustment(
            "Exposure",
            "{\"type\":\"adjust\",\"evaluation\":\"precision_v1\",\"op\":\"exposure\",\"params\":{\"exposure\":2.0}}",
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(exposed.float_refusal(), None);
    let rendered = exposed.render(LIMIT, 0, None).expect("the render succeeds");
    assert_eq!(rendered.storage(), PrecisionStorage::Rgba32Float);
    let merged = ffi_edit(&exposed, &json!({"op": "merge_down", "layer": 1}));
    assert!(merged.is_float());
    merged
        .render(LIMIT, 0, None)
        .expect("the merged render succeeds");
}

/// Regression (review finding 1): Lanczos tails used to leave float alphas
/// just above the renderer's 2^-96 floor (2e-29 was measured), and a mask's
/// low coverage then pushed alpha × coverage under it, so the whole document
/// refused to render. D2's floor is 2^-24: every reference alpha in
/// (0, 2^-24) comes out as exact zeros — including some the old 2^-96 floor
/// kept — and after a second move (which with the old floor left alphas near
/// 5e-28 on the canvas) the layer under a mask whose coverage is 1/65535
/// everywhere renders, at full and at 1/65535 opacity.
#[test]
fn lanczos_tails_below_the_floor_are_exact_zero_and_a_masked_layer_renders() {
    let (w, h) = (21u32, 17u32);
    // An opaque block in a wide transparent margin, so the Lanczos tails
    // stay on the canvas.
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (9..12).contains(&x) && (7..10).contains(&y) {
                [2.5, 0.5, -0.25, 1.0]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let base = float_doc(w, h, &pixels);
    // A tiny fractional move: every tail tap sits just past a kernel zero,
    // so a diagonal margin pixel's alpha is the product of two tail sums of
    // order 1e-4 — positive, above 2^-96 and below 2^-24.
    let there = [1.0, 0.0, 0.0, 1.0, 1e-4, 1e-4];
    let request = |matrix: [f64; 6]| {
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": matrix, "filter": "lanczos3"})
    };
    let out = apply(&base, &request(there));
    let got = source(&out, 0);
    let want = reference_transform(
        &pixels,
        (w, h),
        (0, 0),
        extent(&out, 0),
        affine_map(there),
        "lanczos3",
    );
    let old_floor = 1.0 / 79_228_162_514_264_337_593_543_950_336.0_f64;
    let mut dropped = 0;
    for (index, (g, r)) in got.iter().zip(&want).enumerate() {
        let tolerance = K * EPS * r.abs[3];
        if r.sum[3] > old_floor + tolerance && r.sum[3] < FLOOR - tolerance {
            assert_eq!(
                g.map(f32::to_bits),
                [0u32; 4],
                "alpha {:e} at {index} is below 2^-24 and stores exact zeros",
                r.sum[3]
            );
            dropped += 1;
        }
    }
    assert!(
        dropped > 0,
        "the fixture leaves some Lanczos tail alpha in (2^-96, 2^-24)"
    );
    let back = apply(&out, &request([1.0, 0.0, 0.0, 1.0, -1e-4, -1e-4]));
    for (index, p) in source(&back, 0).iter().enumerate() {
        assert!(
            p[3] == 0.0 || f64::from(p[3]) >= FLOOR,
            "alpha {:e} at {index} is inside (0, 2^-24)",
            p[3]
        );
    }
    let (lw, lh) = back.layer_source(0).unwrap().dimensions();
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(lw, lh, vec![1; (lw * lh) as usize], None).unwrap(),
        MaskProperties::at(back.layer(0).unwrap().offset()),
        true,
    )
    .unwrap();
    let masked = back
        .set_layer_mask(0, Some(mask), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    masked
        .render(LIMIT, 0, None)
        .expect("a masked layer after a filtered transform renders");
    masked
        .set_layer_opacity(0, 1.0 / 65535.0)
        .unwrap()
        .unwrap()
        .render(LIMIT, 0, None)
        .expect("a faint masked layer after a filtered transform renders");
}
