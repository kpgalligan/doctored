//! The f64 INVERSE-MAPPING REFERENCE the filtered-geometry tests measure
//! against, written once and generic over the maximum code.
//!
//! It was `precision_straighten.rs`'s own oracle — pixel-centre convention,
//! transparent out-of-bounds taps, PREMULTIPLIED accumulation, normalized
//! weights — and the only reason it moved is that a SECOND filtered-geometry
//! op (`canvas_rotation.rs`, Phase 12F-1b3's arbitrary canvas rotation) needs
//! exactly the same yardstick at a different maximum code. Two copies of a
//! resampling reference is two chances to agree with the implementation by
//! accident; one copy compared at 255 and at 65535 is the same independent
//! answer measured twice.
//!
//! Samples cross as `u32` at every depth and each call names its own `max`
//! (255 for a byte document, 65535 for a typed one), so nothing here knows
//! which depth it is serving. `precision_straighten.rs` keeps thin `u16`
//! wrappers over these functions, which is what lets that file's twelve test
//! BODIES stay exactly as they were written.
//!
//! This is a REFERENCE, not a second implementation of the shipped kernel: it
//! is deliberately the straightforward, slow, allocation-happy form, and it
//! shares no line of code with `core/src/sampling.rs`.
//!
//! # The comparisons
//!
//! [`assert_rgba_within_one`] and [`assert_plane_within_one`] are the fixed
//! tolerances both callers' status docs state and neither may widen: alpha
//! within 1 code EVERYWHERE, straight RGB within 1 code wherever the
//! reference's own alpha is at least 1, and all four components exactly zero
//! wherever the result's alpha rounds to 0 — a transparent store carries no
//! colour to compare, which is the clause that proves LOUD hidden RGB at
//! alpha 0 never bleeds.
#![allow(dead_code)]

/// The inverse of a 2x3 affine in `CGAffineTransform` element order
/// (`[a, b, c, d, tx, ty]`). The caller only ever hands over a matrix it has
/// already established is invertible.
pub fn invert(m: [f64; 6]) -> [f64; 6] {
    let det = m[0] * m[3] - m[1] * m[2];
    [
        m[3] / det,
        -m[1] / det,
        -m[2] / det,
        m[0] / det,
        (m[2] * m[5] - m[3] * m[4]) / det,
        (m[1] * m[4] - m[0] * m[5]) / det,
    ]
}

/// The destination extent of a plane at `offset` with `dims` under `m`: the
/// four transformed corners, each snapped onto an integer it sits within 1e-9
/// of, then floored/ceiled outward. `bounds_of_corners`' rule, restated.
pub fn bounds(offset: (i32, i32), dims: (u32, u32), m: [f64; 6]) -> (i32, i32, u32, u32) {
    let at = |x: f64, y: f64| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
    let (x, y) = (f64::from(offset.0), f64::from(offset.1));
    let (w, h) = (f64::from(dims.0), f64::from(dims.1));
    let snap = |v: f64| {
        let r = v.round();
        if (v - r).abs() <= 1e-9 {
            r
        } else {
            v
        }
    };
    let corners = [at(x, y), at(x + w, y), at(x, y + h), at(x + w, y + h)];
    let xs: Vec<f64> = corners.iter().map(|c| snap(c.0)).collect();
    let ys: Vec<f64> = corners.iter().map(|c| snap(c.1)).collect();
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

/// The Keys cubic's parameter `a` of a `"keys:<a>"` kind (the R5 fit:
/// `"keys:-0.75"`, `"keys:-1"`); nil for every other kind. `"bicubic"` is the
/// Keys cubic at a = -0.5 too, kept as its own spelled-out branch so its
/// answers stay bit-identical to every test written against it.
pub fn keys_parameter(kind: &str) -> Option<f64> {
    kind.strip_prefix("keys:").map(|a| {
        a.parse::<f64>()
            .unwrap_or_else(|_| panic!("a Keys kind names its parameter: {kind}"))
    })
}

fn kernel_radius(kind: &str) -> i64 {
    if keys_parameter(kind).is_some() {
        return 2;
    }
    match kind {
        "bilinear" => 1,
        "bicubic" => 2,
        other => panic!("this reference covers bilinear, bicubic and keys:<a>, not {other}"),
    }
}

fn kernel_weight(kind: &str, t: f64) -> f64 {
    let x = t.abs();
    if let Some(a) = keys_parameter(kind) {
        // Keys (1981): (a + 2)|x|^3 - (a + 3)|x|^2 + 1 inside one pixel,
        // a|x|^3 - 5a|x|^2 + 8a|x| - 4a out to two.
        return if x < 1.0 {
            ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
        } else if x < 2.0 {
            ((a * x - 5.0 * a) * x + 8.0 * a) * x - 4.0 * a
        } else {
            0.0
        };
    }
    match kind {
        "bilinear" => (1.0 - x).max(0.0),
        // Catmull-Rom (Mitchell with B = 0, C = 0.5).
        _ if x < 1.0 => ((1.5 * x - 2.5) * x) * x + 1.0,
        _ if x < 2.0 => ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0,
        _ => 0.0,
    }
}

/// Normalized tap weights for the continuous source coordinate `s`, where an
/// integer is a pixel CENTRE at `s = k + 0.5`; returns the first tap index.
fn taps(kind: &str, s: f64) -> (i64, Vec<f64>) {
    let radius = kernel_radius(kind);
    let count = 2 * radius as usize;
    let p = s - 0.5;
    let base = p.floor();
    let lead = radius - 1;
    let mut weights: Vec<f64> = (0..count)
        .map(|t| kernel_weight(kind, p - (base + t as f64 - lead as f64)))
        .collect();
    let sum: f64 = weights.iter().sum();
    if sum != 0.0 && sum != 1.0 {
        for w in weights.iter_mut() {
            *w /= sum;
        }
    }
    ((base as i64) - lead, weights)
}

fn quantize(value: f64, max: u32) -> u32 {
    value.clamp(0.0, f64::from(max)).round() as u32
}

/// The source coordinate a destination index maps to, in the source plane's
/// own index space.
fn source_at(
    inverse: [f64; 6],
    source_offset: (i32, i32),
    dest: (i32, i32, u32, u32),
    i: u32,
    j: u32,
) -> (f64, f64) {
    let cx = f64::from(dest.0) + f64::from(i) + 0.5;
    let cy = f64::from(dest.1) + f64::from(j) + 0.5;
    (
        inverse[0] * cx + inverse[2] * cy + inverse[4] - f64::from(source_offset.0),
        inverse[1] * cx + inverse[3] * cy + inverse[5] - f64::from(source_offset.1),
    )
}

/// The RGBA resampler: premultiplied f64 accumulation over normalized taps,
/// transparent outside the source, unpremultiplied by the RAW alpha sum on
/// the way out. `max` is the maximum code of the storage being modelled.
pub fn reference_rgba(
    source: &[[u32; 4]],
    dims: (u32, u32),
    source_offset: (i32, i32),
    dest: (i32, i32, u32, u32),
    inverse: [f64; 6],
    kind: &str,
    max: u32,
) -> Vec<[u32; 4]> {
    let (sw, sh) = dims;
    let full = f64::from(max);
    let mut out = vec![[0u32; 4]; (dest.2 as usize) * (dest.3 as usize)];
    for j in 0..dest.3 {
        for i in 0..dest.2 {
            let (u, v) = source_at(inverse, source_offset, dest, i, j);
            let di = (j as usize) * (dest.2 as usize) + i as usize;
            if kind == "nearest" {
                let (x, y) = (u.floor(), v.floor());
                if x >= 0.0 && y >= 0.0 && x < f64::from(sw) && y < f64::from(sh) {
                    out[di] = source[y as usize * sw as usize + x as usize];
                }
                continue;
            }
            let (x0, wx) = taps(kind, u);
            let (y0, wy) = taps(kind, v);
            let mut acc = [0.0f64; 4];
            for (ty, &wyt) in wy.iter().enumerate() {
                let sy = y0 + ty as i64;
                if wyt == 0.0 || sy < 0 || sy >= i64::from(sh) {
                    continue;
                }
                for (tx, &wxt) in wx.iter().enumerate() {
                    let sx = x0 + tx as i64;
                    if wxt == 0.0 || sx < 0 || sx >= i64::from(sw) {
                        continue;
                    }
                    let pixel = source[sy as usize * sw as usize + sx as usize];
                    let weight = wyt * wxt;
                    let alpha = f64::from(pixel[3]);
                    // PREMULTIPLIED: hidden RGB at alpha 0 is weighted by 0
                    // and cannot bleed.
                    let pm = weight * alpha / full;
                    for c in 0..3 {
                        acc[c] += pm * f64::from(pixel[c]);
                    }
                    acc[3] += weight * alpha;
                }
            }
            let alpha = quantize(acc[3], max);
            if alpha == 0 {
                out[di] = [0; 4];
                continue;
            }
            let scale = full / acc[3];
            out[di] = [
                quantize(acc[0] * scale, max),
                quantize(acc[1] * scale, max),
                quantize(acc[2] * scale, max),
                alpha,
            ];
        }
    }
    out
}

/// The scalar twin, with the plane's own `outside` coverage: the destination
/// starts at `outside`, and every tap is taken RELATIVE to it.
#[allow(clippy::too_many_arguments)]
pub fn reference_plane(
    source: &[u32],
    dims: (u32, u32),
    source_offset: (i32, i32),
    dest: (i32, i32, u32, u32),
    inverse: [f64; 6],
    kind: &str,
    outside: u32,
    max: u32,
) -> Vec<u32> {
    let (sw, sh) = dims;
    let mut out = vec![outside; (dest.2 as usize) * (dest.3 as usize)];
    for j in 0..dest.3 {
        for i in 0..dest.2 {
            let (u, v) = source_at(inverse, source_offset, dest, i, j);
            let di = (j as usize) * (dest.2 as usize) + i as usize;
            if kind == "nearest" {
                let (x, y) = (u.floor(), v.floor());
                if x >= 0.0 && y >= 0.0 && x < f64::from(sw) && y < f64::from(sh) {
                    out[di] = source[y as usize * sw as usize + x as usize];
                }
                continue;
            }
            let (x0, wx) = taps(kind, u);
            let (y0, wy) = taps(kind, v);
            let mut acc = 0.0f64;
            for (ty, &wyt) in wy.iter().enumerate() {
                let sy = y0 + ty as i64;
                if wyt == 0.0 || sy < 0 || sy >= i64::from(sh) {
                    continue;
                }
                for (tx, &wxt) in wx.iter().enumerate() {
                    let sx = x0 + tx as i64;
                    if wxt == 0.0 || sx < 0 || sx >= i64::from(sw) {
                        continue;
                    }
                    acc += wyt
                        * wxt
                        * (f64::from(source[sy as usize * sw as usize + sx as usize])
                            - f64::from(outside));
                }
            }
            out[di] = quantize(acc + f64::from(outside), max);
        }
    }
    out
}

/// The RGBA comparison, with the three clauses spelled out in the module doc.
pub fn assert_rgba_within_one(got: &[[u32; 4]], want: &[[u32; 4]], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    for (index, (g, w)) in got.iter().zip(want).enumerate() {
        let delta = i64::from(g[3]) - i64::from(w[3]);
        assert!(
            delta.abs() <= 1,
            "{what}: alpha at {index} is {} against the reference's {}",
            g[3],
            w[3]
        );
        if g[3] == 0 {
            // A result whose own alpha rounds to zero stores all four
            // components zero and has no colour to compare.
            assert_eq!(
                *g, [0u32; 4],
                "{what}: pixel {index} rounds to transparent and must store zeros"
            );
            continue;
        }
        if w[3] >= 1 {
            for c in 0..3 {
                let delta = i64::from(g[c]) - i64::from(w[c]);
                assert!(
                    delta.abs() <= 1,
                    "{what}: channel {c} at {index} is {} against the reference's {}",
                    g[c],
                    w[c]
                );
            }
        }
    }
}

/// The scalar comparison: within 1 code everywhere.
pub fn assert_plane_within_one(got: &[u32], want: &[u32], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    for (index, (&g, &w)) in got.iter().zip(want).enumerate() {
        let delta = i64::from(g) - i64::from(w);
        assert!(
            delta.abs() <= 1,
            "{what}: sample {index} is {g} against the reference's {w}"
        );
    }
}
