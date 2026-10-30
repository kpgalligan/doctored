//! Auto-Align's warp path: `perspective_set` over ONE raster that sits
//! at a non-zero offset inside a larger canvas, with the destination quad a
//! KNOWN 3×3 matrix `K` maps the layer's corners to, at 8 and 16 bits through
//! the FFI (`dr_doc_geometry_edit` / `dr_precision_doc_edit`).
//!
//! The host registers a layer with Vision, projects the layer's four corners
//! through the homography it measured and hands the core that quad; the core
//! solves its own homography from `source_rect` and `quad`. So the fact to pin
//! is that the pixels the core produces are the ones `K` describes.
//!
//! # The oracle
//!
//! `K`'s inverse is written here by cofactors (the adjugate over the
//! determinant), never solved back from the quad, so a solver that fitted the
//! four corners with a different map between them would not agree with it.
//! Every destination pixel centre (integers + 0.5) goes through that inverse
//! into canvas space and then into the source plane; the taps are the
//! normalized bilinear or Catmull-Rom weights over their full support,
//! accumulated PREMULTIPLIED in f64, transparent outside the source. It shares
//! no line with `core/src/sampling.rs`. The comparison is
//! `inverse_mapping::assert_rgba_within_one` at 255 and at 65535.
//!
//! Three matrices: an integer translation (which must be a lossless copy), a
//! 2° rotation about the layer's centre with a 1.02 scale and a fractional
//! shift (a parallelogram), and a true perspective (the bottom row's `m[6]`
//! and `m[7]` non-zero).
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_geometry::dr_doc_geometry_edit;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::inverse_mapping::assert_rgba_within_one;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The canvas, and the floating layer's rectangle inside it.
const CANVAS: (u32, u32) = (96, 72);
const LAYER: (u32, u32) = (40, 30);
const OFFSET: (i32, i32) = (21, 17);

// ---------------------------------------------------------------- fixtures --

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

/// Straight RGBA at `max`. Column 0 is alpha 0 with loud hidden RGB, every
/// seventh pixel is partial alpha and the rest is opaque noise.
fn layer_samples(max: u32) -> Vec<[u32; 4]> {
    (0..LAYER.0 * LAYER.1)
        .map(|i| {
            let r = hash(i + 11);
            let v = |shift: u32| ((r >> shift) & 0xffff) * max / 0xffff;
            if i % LAYER.0 == 0 {
                return [max, 0, max, 0];
            }
            let alpha = if r.is_multiple_of(7) {
                max / 8 + ((r >> 5) % 1000) * (max - max / 8) / 1000
            } else {
                max
            };
            [v(0), v(8), v(16), alpha]
        })
        .collect()
}

fn base_samples(max: u32) -> Vec<[u32; 4]> {
    (0..CANVAS.0 * CANVAS.1)
        .map(|i| {
            let r = hash(i + 100_003);
            [
                (r & 0xff) * max / 0xff,
                ((r >> 8) & 0xff) * max / 0xff,
                max / 3,
                max,
            ]
        })
        .collect()
}

fn byte_image(dims: (u32, u32), samples: &[[u32; 4]]) -> RgbaImage {
    RgbaImage::from_fn(dims.0, dims.1, |x, y| {
        Rgba(samples[(y * dims.0 + x) as usize].map(|v| v as u8))
    })
}

/// Layer 0 "Base" fills the canvas; layer 1 "Floating" is `LAYER` at `OFFSET`.
fn byte_document() -> DrDocument {
    let base = DrDocument::from_pixels(byte_image(CANVAS, &base_samples(255)));
    let mut doc = base
        .adding_image_layer(0, byte_image(LAYER, &layer_samples(255)), "Floating")
        .expect("the floating layer");
    doc.layers[1].offset = OFFSET;
    assert_eq!((doc.width, doc.height), CANVAS);
    doc
}

fn typed_document() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let image = |dims: (u32, u32), samples: Vec<[u32; 4]>| {
        PrecisionImage::from_rgba16(
            dims.0,
            dims.1,
            samples.iter().map(|p| p.map(|v| v as u16)).collect(),
            profile.clone(),
            None,
        )
        .unwrap()
    };
    let base = PrecisionDocumentLayer::raster(
        "Base",
        image(CANVAS, base_samples(65535)),
        (0, 0),
        1.,
        true,
    )
    .unwrap();
    let floating = PrecisionDocumentLayer::raster(
        "Floating",
        image(LAYER, layer_samples(65535)),
        OFFSET,
        1.,
        true,
    )
    .unwrap();
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![base, floating],
    )
    .unwrap()
}

// ------------------------------------------------------------ the matrices --

/// Row-major 3×3, canvas pixels: `(x, y) -> (m0·x + m1·y + m2, m3·x + m4·y +
/// m5) / (m6·x + m7·y + m8)`.
type Matrix = [f64; 9];

fn translation() -> Matrix {
    [1.0, 0.0, 7.0, 0.0, 1.0, -4.0, 0.0, 0.0, 1.0]
}

/// 2° about the layer's centre, scale 1.02, then a shift of (3.5, −2.25).
fn rotation() -> Matrix {
    let angle = 2.0f64.to_radians();
    let (a, b) = (1.02 * angle.cos(), 1.02 * angle.sin());
    let cx = f64::from(OFFSET.0) + f64::from(LAYER.0) / 2.0;
    let cy = f64::from(OFFSET.1) + f64::from(LAYER.1) / 2.0;
    [
        a,
        -b,
        cx - a * cx + b * cy + 3.5,
        b,
        a,
        cy - b * cx - a * cy - 2.25,
        0.0,
        0.0,
        1.0,
    ]
}

/// The identity with a projective bottom row.
fn perspective() -> Matrix {
    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 4e-3, -3e-3, 1.0]
}

fn apply(m: &Matrix, x: f64, y: f64) -> (f64, f64) {
    let w = m[6] * x + m[7] * y + m[8];
    (
        (m[0] * x + m[1] * y + m[2]) / w,
        (m[3] * x + m[4] * y + m[5]) / w,
    )
}

/// The inverse by cofactors: the adjugate over the determinant.
fn cofactor_inverse(m: &Matrix) -> Matrix {
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
    assert!(det.abs() > 1e-9, "the fixture matrix is invertible");
    adjugate.map(|v| v / det)
}

/// The layer rectangle's corners under `m`: TL, TR, BR, BL.
fn quad(m: &Matrix) -> [f64; 8] {
    let (x, y) = (f64::from(OFFSET.0), f64::from(OFFSET.1));
    let (w, h) = (f64::from(LAYER.0), f64::from(LAYER.1));
    let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    let mut out = [0.0; 8];
    for (k, &(cx, cy)) in corners.iter().enumerate() {
        let (qx, qy) = apply(m, cx, cy);
        out[2 * k] = qx;
        out[2 * k + 1] = qy;
    }
    out
}

/// The quad's axis-aligned box rounded OUTWARD, a corner within 1e-9 of an
/// integer snapped onto it first.
fn outward_bounds(quad: &[f64; 8]) -> (i32, i32, u32, u32) {
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
}

fn request(m: &Matrix, filter: &str) -> Value {
    json!({
        "op": "perspective_set", "layers": [1],
        "source_rect": [OFFSET.0, OFFSET.1, LAYER.0, LAYER.1],
        "quad": quad(m), "filter": filter,
    })
}

// --------------------------------------------------------------- reference --

fn radius(kind: &str) -> i64 {
    match kind {
        "bilinear" => 1,
        "bicubic" => 2,
        other => panic!("this reference covers bilinear and bicubic, not {other}"),
    }
}

fn weight(kind: &str, t: f64) -> f64 {
    let x = t.abs();
    match kind {
        "bilinear" => (1.0 - x).max(0.0),
        // Catmull-Rom (B = 0, C = 0.5).
        _ if x < 1.0 => (1.5 * x - 2.5) * x * x + 1.0,
        _ if x < 2.0 => ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0,
        _ => 0.0,
    }
}

/// Normalized taps around the continuous source coordinate `s` (integers +
/// 0.5 are pixel centres), with the first tap's index.
fn taps(kind: &str, s: f64) -> (i64, Vec<f64>) {
    let r = radius(kind);
    let p = s - 0.5;
    let base = p.floor();
    let mut weights: Vec<f64> = (0..2 * r)
        .map(|t| weight(kind, p - (base + (t - (r - 1)) as f64)))
        .collect();
    let sum: f64 = weights.iter().sum();
    if sum != 0.0 {
        weights.iter_mut().for_each(|w| *w /= sum);
    }
    (base as i64 - (r - 1), weights)
}

fn quantize(value: f64, max: u32) -> u32 {
    value.clamp(0.0, f64::from(max)).round() as u32
}

/// The source-plane coordinate destination pixel `(i, j)` of `dest` reads.
fn source_point(inverse: &Matrix, dest: (i32, i32, u32, u32), i: u32, j: u32) -> (f64, f64) {
    let (x, y) = apply(
        inverse,
        f64::from(dest.0) + f64::from(i) + 0.5,
        f64::from(dest.1) + f64::from(j) + 0.5,
    );
    (x - f64::from(OFFSET.0), y - f64::from(OFFSET.1))
}

/// The floating layer resampled onto `dest` through `inverse`.
fn reference(
    source: &[[u32; 4]],
    dest: (i32, i32, u32, u32),
    inverse: &Matrix,
    kind: &str,
    max: u32,
) -> Vec<[u32; 4]> {
    let (sw, sh) = (i64::from(LAYER.0), i64::from(LAYER.1));
    let full = f64::from(max);
    let mut out = Vec::with_capacity((dest.2 * dest.3) as usize);
    for j in 0..dest.3 {
        for i in 0..dest.2 {
            let (u, v) = source_point(inverse, dest, i, j);
            let (x0, wx) = taps(kind, u);
            let (y0, wy) = taps(kind, v);
            let mut acc = [0.0f64; 4];
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
                    let pixel = source[(sy * sw + sx) as usize];
                    let alpha = f64::from(pixel[3]);
                    let premultiplied = wyt * wxt * alpha / full;
                    for c in 0..3 {
                        acc[c] += premultiplied * f64::from(pixel[c]);
                    }
                    acc[3] += wyt * wxt * alpha;
                }
            }
            let alpha = quantize(acc[3], max);
            if alpha == 0 {
                out.push([0; 4]);
                continue;
            }
            let scale = full / acc[3];
            out.push([
                quantize(acc[0] * scale, max),
                quantize(acc[1] * scale, max),
                quantize(acc[2] * scale, max),
                alpha,
            ]);
        }
    }
    out
}

/// Every destination pixel whose source coordinate lies farther outside the
/// source rectangle than the kernel reaches reads no source pixel at all, so
/// it must store four zeros. Returns how many such pixels there were.
fn assert_outside_is_zero(
    got: &[[u32; 4]],
    dest: (i32, i32, u32, u32),
    inverse: &Matrix,
    kind: &str,
    what: &str,
) -> usize {
    let reach = radius(kind) as f64;
    let (w, h) = (f64::from(LAYER.0), f64::from(LAYER.1));
    let mut outside = 0;
    for j in 0..dest.3 {
        for i in 0..dest.2 {
            let (u, v) = source_point(inverse, dest, i, j);
            if u < -reach || v < -reach || u > w + reach || v > h + reach {
                outside += 1;
                assert_eq!(
                    got[(j * dest.2 + i) as usize],
                    [0u32; 4],
                    "{what}: pixel ({i}, {j}) lies outside the quad and must be transparent"
                );
            }
        }
    }
    outside
}

// --------------------------------------------------------------------- ffi --

fn error_text(error: *mut c_char) -> String {
    if error.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    text
}

fn byte_edit(doc: &DrDocument, value: &Value) -> DrDocument {
    let text = CString::new(value.to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_doc_geometry_edit(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut error) };
    assert!(!out.is_null(), "{value} reported: {}", error_text(error));
    *unsafe { Box::from_raw(out) }
}

fn typed_edit(doc: &PrecisionDocument, value: &Value) -> PrecisionDocument {
    let text = CString::new(value.to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
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
    assert!(!out.is_null(), "{value} reported: {}", error_text(error));
    *unsafe { Box::from_raw(out) }
}

fn byte_layer(doc: &DrDocument, index: usize) -> ((i32, i32, u32, u32), Vec<[u32; 4]>) {
    let layer = &doc.layers[index];
    let (w, h) = layer.pixels.dimensions();
    let image = layer.pixels.contiguous_image();
    let pixels = image.pixels().map(|p| p.0.map(u32::from)).collect();
    ((layer.offset.0, layer.offset.1, w, h), pixels)
}

fn typed_layer(doc: &PrecisionDocument, index: usize) -> ((i32, i32, u32, u32), Vec<[u32; 4]>) {
    let (x, y) = doc.layer(index).unwrap().offset();
    let source = doc.layer_source(index).expect("a raster record");
    let (w, h) = source.dimensions();
    let pixels = source
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .map(|p| p.map(u32::from))
        .collect();
    ((x, y, w, h), pixels)
}

/// One depth's document behind the two readings the tests need.
enum Subject {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

impl Subject {
    fn both() -> [(&'static str, u32, Subject); 2] {
        [
            ("8-bit", 255, Subject::Byte(byte_document())),
            ("16-bit", 65535, Subject::Typed(typed_document())),
        ]
    }

    fn edit(&self, value: &Value) -> Subject {
        match self {
            Subject::Byte(doc) => Subject::Byte(byte_edit(doc, value)),
            Subject::Typed(doc) => Subject::Typed(typed_edit(doc, value)),
        }
    }

    fn layer(&self, index: usize) -> ((i32, i32, u32, u32), Vec<[u32; 4]>) {
        match self {
            Subject::Byte(doc) => byte_layer(doc, index),
            Subject::Typed(doc) => typed_layer(doc, index),
        }
    }

    fn canvas(&self) -> (u32, u32) {
        match self {
            Subject::Byte(doc) => (doc.width, doc.height),
            Subject::Typed(doc) => doc.dimensions(),
        }
    }

    fn layer_count(&self) -> usize {
        match self {
            Subject::Byte(doc) => doc.layers.len(),
            Subject::Typed(doc) => doc.layer_count(),
        }
    }
}

/// What every warp must leave alone: the canvas, the layer count and layer 0.
fn assert_rest_unchanged(before: &Subject, after: &Subject, what: &str) {
    assert_eq!(after.canvas(), CANVAS, "{what}: the canvas");
    assert_eq!(after.layer_count(), before.layer_count(), "{what}: layers");
    assert_eq!(after.layer(0), before.layer(0), "{what}: the other layer");
}

// ------------------------------------------------------------------- tests --

#[test]
fn a_known_homography_warps_the_layer_onto_the_reference_at_both_depths() {
    for (name, matrix) in [("rotation", rotation()), ("perspective", perspective())] {
        let inverse = cofactor_inverse(&matrix);
        // The cofactor inverse really is the inverse: K⁻¹(K(p)) = p.
        let (px, py) = apply(&matrix, 33.25, 40.5);
        let (bx, by) = apply(&inverse, px, py);
        assert!((bx - 33.25).abs() < 1e-9 && (by - 40.5).abs() < 1e-9);
        for (depth, max, subject) in Subject::both() {
            let (_, source) = subject.layer(1);
            for kind in ["bilinear", "bicubic"] {
                let what = format!("{depth} {name} {kind}");
                let out = subject.edit(&request(&matrix, kind));
                assert_rest_unchanged(&subject, &out, &what);
                let (extent, got) = out.layer(1);
                assert_eq!(
                    extent,
                    outward_bounds(&quad(&matrix)),
                    "{what}: the extent is the quad's outward-rounded box"
                );
                let want = reference(&source, extent, &inverse, kind, max);
                assert_rgba_within_one(&got, &want, &what);
                let outside = assert_outside_is_zero(&got, extent, &inverse, kind, &what);
                if name == "perspective" && kind == "bilinear" {
                    assert!(
                        outside > 0,
                        "{what}: the fixture has pixels outside the quad"
                    );
                }
                assert!(
                    got.iter().any(|p| p[3] == max),
                    "{what}: the warp kept opaque pixels"
                );
            }
        }
    }
}

#[test]
fn an_integer_translation_quad_is_a_lossless_copy_at_both_depths() {
    let matrix = translation();
    for (depth, _, subject) in Subject::both() {
        let (before, source) = subject.layer(1);
        for kind in ["bilinear", "bicubic"] {
            let what = format!("{depth} translation {kind}");
            let out = subject.edit(&request(&matrix, kind));
            assert_rest_unchanged(&subject, &out, &what);
            let (extent, got) = out.layer(1);
            assert_eq!(
                extent,
                (before.0 + 7, before.1 - 4, before.2, before.3),
                "{what}: the rectangle moved by (7, -4)"
            );
            assert_eq!(extent, outward_bounds(&quad(&matrix)), "{what}: the extent");
            assert_eq!(got, source, "{what}: every sample is copied exactly");
        }
    }
}
