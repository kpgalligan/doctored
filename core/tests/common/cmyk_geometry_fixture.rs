//! The CMYK geometry files' shared fixtures (#547 phase 4b:
//! `cmyk_geometry.rs`, `cmyk_warp.rs`, `cmyk_selected_pixels.rs`).
//!
//! THE TWINS. Every CMYK geometry op resamples the five planes premultiplied
//! by alpha exactly as RGB resamples its four, so its result is checked
//! against two RGB16 documents built from the CMYK document's own stored
//! records ([`twins`]): one of `[C, M, Y, A]` and one of `[K, K, K, A]`, at
//! the same offsets, opacity and masks. The same request on all three must
//! give C, M, Y bit-identical to the first twin's R, G, B, K to the second's
//! R, and alpha to both ([`assert_twins`]).
//!
//! THE ORACLE. An f64 premultiplied reference resampler written here from
//! the kernels' closed forms (triangle, Catmull-Rom, Lanczos3), never read
//! off `sampling.rs`: a transform takes the full support around each
//! destination pixel centre mapped back to the source, normalizes it, drops
//! the out-of-bounds taps (transparent), and unpremultiplies by the raw
//! alpha sum; a resize normalizes its source-clipped support, widened by the
//! reduction ratio. Each finishes with one rounding.

#![allow(dead_code, clippy::needless_range_loop)]

use std::ffi::CString;
use std::ptr;

use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;

use crate::cmyk_structure_fixture::{free, try_edit};

/// One five-plane stored sample.
pub type Cmyk = [u16; 5];

/// A record's stored samples, offset and size.
pub type Record<S> = (Vec<S>, (i32, i32), (u32, u32));

/// A CMYK document and its two RGB16 twins (module doc).
pub struct Twins {
    pub cmyk: *mut PrecisionDocument,
    pub cmy: *mut PrecisionDocument,
    pub k: *mut PrecisionDocument,
}

impl Twins {
    pub fn free(self) {
        free(self.cmyk);
        free(self.cmy);
        free(self.k);
    }
}

/// The stored samples of record `index`, its offset and size.
pub fn record(doc: *const PrecisionDocument, index: usize) -> Record<Cmyk> {
    let doc = unsafe { &*doc };
    let layer = doc.layer(index).expect("a record");
    let image = layer.cmyk_source().expect("a CMYK raster");
    (
        doc.cmyk_layer_samples(index).unwrap(),
        layer.offset(),
        image.dimensions(),
    )
}

/// An RGB16 record's samples, offset and size.
pub fn rgb_record(doc: *const PrecisionDocument, index: usize) -> Record<[u16; 4]> {
    let doc = unsafe { &*doc };
    let layer = doc.layer(index).expect("a record");
    let image = layer.source().expect("an RGB raster");
    (
        image.to_rgba16().unwrap(),
        layer.offset(),
        image.dimensions(),
    )
}

/// The two RGB16 twins of a CMYK document of root raster records (module
/// doc): every record's view, offset, opacity, visibility and mask.
pub fn twins(cmyk: *mut PrecisionDocument) -> Twins {
    let doc = unsafe { &*cmyk };
    let build = |black: bool| {
        let layers = (0..doc.layer_count())
            .map(|i| {
                let layer = doc.layer(i).unwrap();
                let (samples, offset, (w, h)) = record(cmyk, i);
                let view = samples
                    .iter()
                    .map(|s| {
                        if black {
                            [s[3], s[3], s[3], s[4]]
                        } else {
                            [s[0], s[1], s[2], s[4]]
                        }
                    })
                    .collect();
                let image =
                    PrecisionImage::from_rgba16(w, h, view, IccProfile::srgb(), None).unwrap();
                PrecisionDocumentLayer::raster(
                    layer.name(),
                    image,
                    offset,
                    layer.opacity(),
                    layer.visible(),
                )
                .unwrap()
                .with_mask(layer.mask().cloned())
            })
            .collect();
        let (w, h) = doc.dimensions();
        Box::into_raw(Box::new(
            PrecisionDocument::from_parts(
                w,
                h,
                IccProfile::srgb(),
                Metadata::default(),
                Resolution::default(),
                layers,
            )
            .unwrap(),
        ))
    };
    Twins {
        cmyk,
        cmy: build(false),
        k: build(true),
    }
}

/// `edit` applied to all three documents: `None` when none changed (and
/// `Err` naming the one that disagrees).
pub fn each(
    twins: &Twins,
    edit: impl Fn(*const PrecisionDocument) -> Result<*mut PrecisionDocument, String>,
) -> Option<Twins> {
    let cmyk = edit(twins.cmyk).unwrap_or_else(|e| panic!("CMYK: {e}"));
    let cmy = edit(twins.cmy).unwrap_or_else(|e| panic!("CMY twin: {e}"));
    let k = edit(twins.k).unwrap_or_else(|e| panic!("K twin: {e}"));
    let changed = [cmyk, cmy, k].map(|d| !d.is_null());
    assert!(
        changed == [true; 3] || changed == [false; 3],
        "the twins disagree on a change: {changed:?}"
    );
    (!cmyk.is_null()).then_some(Twins { cmyk, cmy, k })
}

/// `request` through `dr_precision_doc_edit` on all three, with `samples`.
pub fn each_edit(twins: &Twins, request: &str, samples: &[u16]) -> Option<Twins> {
    each(twins, |doc| try_edit(doc, request, samples))
}

/// The module doc's twin identity: every record's offset, size, planes and
/// mask; every C, M, Y plane bit-identical to the CMY twin's R, G, B, K to the
/// K twin's R, alpha to both.
pub fn assert_twins(twins: &Twins, what: &str) {
    let doc = unsafe { &*twins.cmyk };
    let (cmy, k) = unsafe { (&*twins.cmy, &*twins.k) };
    assert_eq!(doc.dimensions(), cmy.dimensions(), "{what}: canvas");
    assert_eq!(doc.dimensions(), k.dimensions(), "{what}: canvas");
    assert_eq!(doc.layer_count(), cmy.layer_count(), "{what}: records");
    for i in 0..doc.layer_count() {
        let (samples, offset, size) = record(twins.cmyk, i);
        let (a, ao, asz) = rgb_record(twins.cmy, i);
        let (b, bo, bsz) = rgb_record(twins.k, i);
        assert_eq!((offset, size), (ao, asz), "{what}: record {i} extent");
        assert_eq!((offset, size), (bo, bsz), "{what}: record {i} extent");
        for (p, ((s, x), y)) in samples.iter().zip(&a).zip(&b).enumerate() {
            assert_eq!(
                [s[0], s[1], s[2], s[4]],
                *x,
                "{what}: record {i} pixel {p}: C, M, Y, A against the CMY twin"
            );
            assert_eq!(
                [s[3], s[4]],
                [y[0], y[3]],
                "{what}: record {i} pixel {p}: K, A against the K twin"
            );
        }
        let masks = [doc, cmy, k].map(|d| {
            d.layer(i).unwrap().mask().map(|m| {
                (
                    m.properties().offset,
                    m.plane().dimensions(),
                    m.plane().samples().to_vec(),
                )
            })
        });
        assert_eq!(masks[0], masks[1], "{what}: record {i} mask");
        assert_eq!(masks[0], masks[2], "{what}: record {i} mask");
    }
}

/// `request` must change all three (module doc), and the results are twins.
pub fn assert_twin_edit(twins: &Twins, request: &str, samples: &[u16]) -> Twins {
    let out = each_edit(twins, request, samples).unwrap_or_else(|| panic!("{request}: no change"));
    assert!(
        unsafe { &*out.cmyk }.is_cmyk(),
        "{request}: the document is still CMYK"
    );
    assert_twins(&out, request);
    out
}

/// A C string of `text`.
pub fn c(text: &str) -> CString {
    CString::new(text).unwrap()
}

/// A null error slot.
pub fn no_err() -> *mut std::ffi::c_char {
    ptr::null_mut()
}

// ------------------------------------------------------------ the oracle --

/// The three filtered kernels of the oracle.
#[derive(Clone, Copy, Debug)]
pub enum Kernel {
    Bilinear,
    Bicubic,
    Lanczos3,
}

impl Kernel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Bilinear => "bilinear",
            Self::Bicubic => "bicubic",
            Self::Lanczos3 => "lanczos3",
        }
    }

    /// Half-width of the support in source pixels.
    pub fn radius(self) -> f64 {
        match self {
            Self::Bilinear => 1.0,
            Self::Bicubic => 2.0,
            Self::Lanczos3 => 3.0,
        }
    }

    /// The closed forms: the triangle; Catmull-Rom (Keys, a = −1/2);
    /// sinc(x)·sinc(x/3) on |x| < 3.
    pub fn weight(self, x: f64) -> f64 {
        let x = x.abs();
        match self {
            Self::Bilinear => (1.0 - x).max(0.0),
            Self::Bicubic => {
                if x < 1.0 {
                    1.5 * x * x * x - 2.5 * x * x + 1.0
                } else if x < 2.0 {
                    -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
                } else {
                    0.0
                }
            }
            Self::Lanczos3 => {
                if x == 0.0 {
                    1.0
                } else if x < 3.0 {
                    let p = std::f64::consts::PI * x;
                    (p.sin() / p) * ((p / 3.0).sin() / (p / 3.0))
                } else {
                    0.0
                }
            }
        }
    }
}

/// The premultiplied sum's finish: alpha rounded once; zero alpha is a
/// transparent zero sample; each ink divided by the RAW alpha sum.
fn finish(acc: [f64; 5]) -> Cmyk {
    let alpha = acc[4].clamp(0.0, 65535.0).round();
    if alpha == 0.0 {
        return [0; 5];
    }
    let mut out = [alpha as u16; 5];
    for c in 0..4 {
        out[c] = (acc[c] / acc[4]).clamp(0.0, 65535.0).round() as u16;
    }
    out
}

/// Adds `weight` of a straight sample to a premultiplied f64 sum (ink codes
/// times alpha codes; alpha codes).
fn add(acc: &mut [f64; 5], s: Cmyk, weight: f64) {
    let a = f64::from(s[4]);
    for c in 0..4 {
        acc[c] += weight * a * f64::from(s[c]);
    }
    acc[4] += weight * a;
}

/// The transform oracle: `inverse` maps a canvas point to a canvas source
/// point (projective, row-major 3×3); the source record stands at
/// `origin`; the result covers `dest` (canvas x, y, w, h).
pub fn transform_oracle(
    source: &[Cmyk],
    size: (u32, u32),
    origin: (i32, i32),
    inverse: [f64; 9],
    dest: ((i32, i32), (u32, u32)),
    kernel: Kernel,
) -> Vec<Cmyk> {
    let ((dx, dy), (dw, dh)) = dest;
    let r = kernel.radius();
    let mut out = Vec::with_capacity(dw as usize * dh as usize);
    for j in 0..dh {
        for i in 0..dw {
            let (x, y) = (
                f64::from(dx) + f64::from(i) + 0.5,
                f64::from(dy) + f64::from(j) + 0.5,
            );
            let w = inverse[6] * x + inverse[7] * y + inverse[8];
            let u = (inverse[0] * x + inverse[1] * y + inverse[2]) / w - f64::from(origin.0);
            let v = (inverse[3] * x + inverse[4] * y + inverse[5]) / w - f64::from(origin.1);
            // Pixel k's centre is k + 1/2; the support is the 2r centres
            // nearest the point, normalized over all of them.
            let taps = |s: f64| -> Vec<(i64, f64)> {
                let p = s - 0.5;
                let first = p.floor() as i64 - (r as i64 - 1);
                let taps: Vec<(i64, f64)> = (0..2 * r as i64)
                    .map(|t| (first + t, kernel.weight(p - (first + t) as f64)))
                    .collect();
                let sum: f64 = taps.iter().map(|t| t.1).sum();
                taps.into_iter().map(|(k, w)| (k, w / sum)).collect()
            };
            let mut acc = [0.0; 5];
            for (sy, wy) in taps(v) {
                for (sx, wx) in taps(u) {
                    if sx < 0 || sy < 0 || sx >= i64::from(size.0) || sy >= i64::from(size.1) {
                        continue;
                    }
                    add(
                        &mut acc,
                        source[sy as usize * size.0 as usize + sx as usize],
                        wx * wy,
                    );
                }
            }
            out.push(finish(acc));
        }
    }
    out
}

/// One axis of the resize oracle: the normalized taps of output `index`.
fn resize_taps(kernel: Kernel, source: u32, target: u32, index: u32) -> Vec<(usize, f64)> {
    let ratio = f64::from(source) / f64::from(target);
    let scale = ratio.max(1.0);
    let centre = (f64::from(index) + 0.5) * ratio;
    let support = kernel.radius() * scale;
    let first = ((centre - support).floor() as i64).clamp(0, i64::from(source) - 1);
    let end = ((centre + support).ceil() as i64).clamp(first + 1, i64::from(source));
    let taps: Vec<(usize, f64)> = (first..end)
        .map(|k| {
            (
                k as usize,
                kernel.weight((k as f64 - (centre - 0.5)) / scale),
            )
        })
        .collect();
    let sum: f64 = taps.iter().map(|t| t.1).sum();
    taps.into_iter().map(|(k, w)| (k, w / sum)).collect()
}

/// The resize oracle: `source` (`size`) to `target`.
pub fn resize_oracle(
    source: &[Cmyk],
    size: (u32, u32),
    target: (u32, u32),
    kernel: Kernel,
) -> Vec<Cmyk> {
    let rows: Vec<_> = (0..target.1)
        .map(|y| resize_taps(kernel, size.1, target.1, y))
        .collect();
    let columns: Vec<_> = (0..target.0)
        .map(|x| resize_taps(kernel, size.0, target.0, x))
        .collect();
    let mut out = Vec::with_capacity(target.0 as usize * target.1 as usize);
    for row in &rows {
        for column in &columns {
            let mut acc = [0.0; 5];
            for &(sy, wy) in row {
                for &(sx, wx) in column {
                    add(&mut acc, source[sy * size.0 as usize + sx], wx * wy);
                }
            }
            out.push(finish(acc));
        }
    }
    out
}

/// Every plate of `got` within `bound` codes of `want` as it shows ON PAPER
/// — the straight ink difference times the result's own alpha, |Δink|·a —
/// and alpha within `bound` codes. At full alpha that is the straight
/// difference; under an alpha of a few hundred codes the straight ink
/// divides the shared kernels' f32 weight rounding by that alpha (a 2-code
/// straight step at alpha 369 is 0.01 code on paper), and the RGB kernels
/// are the same arithmetic, so the coverage-weighted reading is the one that
/// says whether a plate is right.
pub fn assert_within(got: &[Cmyk], want: &[Cmyk], bound: u16, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..4 {
            let seen = f64::from(g[c].abs_diff(w[c])) * f64::from(g[4]) / 65535.0;
            assert!(
                seen <= f64::from(bound),
                "{what}: pixel {i} plate {c} ({seen:.3} on paper): {g:?} vs the oracle's {w:?}"
            );
        }
        assert!(
            g[4].abs_diff(w[4]) <= bound,
            "{what}: pixel {i} alpha: {g:?} vs the oracle's {w:?}"
        );
    }
}

/// `[a, b, c, d, tx, ty]` (`x' = a·x + c·y + tx`, `y' = b·x + d·y + ty`)
/// turning `degrees` about `centre` and scaling by `scale`.
pub fn turn(degrees: f64, scale: f64, centre: (f64, f64)) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    let (a, b) = (c * scale, s * scale);
    [
        a,
        b,
        -b,
        a,
        centre.0 - a * centre.0 + b * centre.1,
        centre.1 - b * centre.0 - a * centre.1,
    ]
}

/// The canvas inverse of an affine `m` as a projective 3×3.
pub fn affine_inverse(m: [f64; 6]) -> [f64; 9] {
    let [a, b, c, d, tx, ty] = m;
    let det = a * d - b * c;
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    [
        ia,
        ic,
        -(ia * tx + ic * ty),
        ib,
        id,
        -(ib * tx + id * ty),
        0.0,
        0.0,
        1.0,
    ]
}

/// The homography taking the four `from` points to the four `to` points
/// (the classic eight-unknown linear solve, Gaussian elimination with
/// partial pivoting), row-major with h33 = 1.
pub fn homography(from: [(f64, f64); 4], to: [(f64, f64); 4]) -> [f64; 9] {
    let mut m = [[0.0f64; 9]; 8];
    for k in 0..4 {
        let ((x, y), (u, v)) = (from[k], to[k]);
        m[2 * k] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * k + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
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
    let mut h = [1.0; 9];
    for k in 0..8 {
        h[k] = m[k][8] / m[k][k];
    }
    h
}

/// The inverse of a row-major 3×3 (adjugate over determinant).
pub fn invert3(m: [f64; 9]) -> [f64; 9] {
    let [a, b, c, d, e, f, g, h, i] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        (e * i - f * h) / det,
        (c * h - b * i) / det,
        (b * f - c * e) / det,
        (f * g - d * i) / det,
        (a * i - c * g) / det,
        (c * d - a * f) / det,
        (d * h - e * g) / det,
        (b * g - a * h) / det,
        (a * e - b * d) / det,
    ]
}
