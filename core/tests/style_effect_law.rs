//! The box-pair law of the Drop Shadow, the Inner Shadow and the Outer Glow
//! (#578 part 2, `scripts/photoshop-probe/effect_kernels_578_take.py`, its
//! candidates D1, I1 and O1) against an independent brute-force oracle,
//! through the FFI (`common::styled` sets the style with
//! `dr_doc_set_layer_style`; the 16-bit rows go through
//! `dr_precision_doc_edit`), on a hard rect and on the same rect with an
//! anti-aliased edge column.
//!
//! The oracle, written here from the runner's THE LAW and never from the
//! crate's code: `S = round(size)`, `n = round(S · p)`, `w = S − n` (2 where
//! it is 1); the band of a coverage `u` grown by `n` px is, per pixel, the
//! larger of `round(255 · clamp(n + 1 − d_full))` (`d_full` the Euclidean
//! distance between pixel centres to the nearest fully covered pixel) and,
//! where `u` has a partial pixel, `round(255 · clamp(n − min(|p − q| −
//! u_q)))` over the partial pixels `q` closer than `n + 1`, every candidate
//! visited (a brute force); `B` is the separable triangle `(w − |j|) / w²`
//! (`|j| < w`) over the band's bytes as unit values, or the band at `w =
//! 0`, and every plane is rounded once as `floor(255 v + 1/2)`. Then:
//! - Drop Shadow: `m = B`, landing at the light offset;
//! - Inner Shadow: `B` of `1 − a`, shifted in the plane (255 beyond it),
//!   times the coverage;
//! - Outer Glow: `g = min(1, B / r)`, knocked out in the plane by `1 − a`,
//!   and the visible glow `g (1 − a)` under a covering layer (the
//!   measured composite), a hollow halo at Fill 0 %.
//!
//! Scenes: white backdrop; a 20 × 16 rect at (24, 20) whose column x = 44
//! is at alpha 102 (0.4) in the anti-aliased variant. The shadows render at
//! Fill 0 % with the layer knock-out off, so each pixel reads `255 − m`
//! (black, Normal, opacity 1); the glow is black, Normal, opacity 1, under
//! a white layer at Fill 100 % or 0 %, so each pixel reads `255 (1 −
//! visible)`. Bound: one code at 8 bits, one 8-bit code at 16 bits.

use serde_json::{json, Value};

use crate::identify_578::render_rgb_alpha;

const W: u32 = 72;
const H: u32 = 56;
const RECT: (u32, u32, u32, u32) = (24, 20, 20, 16);
const EDGE: u8 = 102;

/// The picture's coverage bytes: the hard rect, plus the column x = 44 at
/// [`EDGE`] when `aa`.
fn coverage(aa: bool) -> Vec<u8> {
    let (x0, y0, rw, rh) = RECT;
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            let rows = (y0..y0 + rh).contains(&y);
            if rows && (x0..x0 + rw).contains(&x) {
                255
            } else if aa && rows && x == x0 + rw {
                EDGE
            } else {
                0
            }
        })
        .collect()
}

/// `(n, w)` of the law (module doc).
fn split(size: f64, p: f64) -> (i64, i64) {
    let s = (size + 0.5).floor().max(1.0);
    let n = (s * p + 0.5).floor().min(s);
    let w = s - n;
    (n as i64, if w == 1.0 { 2 } else { w as i64 })
}

fn byte(v: f64) -> u8 {
    (255.0 * v + 0.5).floor().clamp(0.0, 255.0) as u8
}

fn round_byte(v: f64) -> u8 {
    (255.0 * v.clamp(0.0, 1.0)).round() as u8
}

/// The band of `u` grown by `n` px, brute force (module doc).
fn band(u: &[u8], n: i64) -> Vec<u8> {
    let at = |i: usize| {
        (
            (i as i64 % i64::from(W)) as f64,
            (i as i64 / i64::from(W)) as f64,
        )
    };
    let full: Vec<usize> = (0..u.len()).filter(|&i| u[i] == 255).collect();
    let partial: Vec<usize> = (0..u.len()).filter(|&i| u[i] > 0 && u[i] < 255).collect();
    let nf = n as f64;
    (0..u.len())
        .map(|i| {
            let (px, py) = at(i);
            let d_full = full
                .iter()
                .map(|&q| {
                    let (qx, qy) = at(q);
                    (px - qx).hypot(py - qy)
                })
                .fold(f64::INFINITY, f64::min);
            let mut b = round_byte(nf + 1.0 - d_full);
            if n == 0 {
                // A grow of 0 is the coverage itself.
                return u[i];
            }
            let best = partial
                .iter()
                .filter_map(|&q| {
                    let (qx, qy) = at(q);
                    let d = (px - qx).hypot(py - qy);
                    (d < nf + 1.0).then(|| d - f64::from(u[q]) / 255.0)
                })
                .fold(f64::INFINITY, f64::min);
            if best.is_finite() {
                b = b.max(round_byte(nf - best));
            }
            b
        })
        .collect()
}

/// The separable triangle `(w − |j|) / w²` over the band's bytes as unit
/// values, edge-clamped (the scenes never reach the border), or the band at
/// `w = 0`.
fn box_pair(band: &[u8], w: i64) -> Vec<f64> {
    let unit: Vec<f64> = band.iter().map(|&c| f64::from(c) / 255.0).collect();
    if w == 0 {
        return unit;
    }
    let (cw, ch) = (i64::from(W), i64::from(H));
    let tap = |j: i64| (w - j.abs()) as f64 / (w * w) as f64;
    let at = |v: &[f64], x: i64, y: i64| v[(y.clamp(0, ch - 1) * cw + x.clamp(0, cw - 1)) as usize];
    let mut rows = vec![0.0; unit.len()];
    for y in 0..ch {
        for x in 0..cw {
            rows[(y * cw + x) as usize] = (1 - w..w).map(|j| tap(j) * at(&unit, x + j, y)).sum();
        }
    }
    let mut out = vec![0.0; unit.len()];
    for y in 0..ch {
        for x in 0..cw {
            out[(y * cw + x) as usize] = (1 - w..w).map(|j| tap(j) * at(&rows, x, y + j)).sum();
        }
    }
    out
}

/// `B` of `u` at `size` and `p` (module doc).
fn law(u: &[u8], size: f64, p: f64) -> Vec<f64> {
    let (n, w) = split(size, p);
    box_pair(&band(u, n), w)
}

/// `plane` shifted by `(dx, dy)`: `out(x, y) = plane(x − dx, y − dy)`, `fill`
/// beyond it.
fn shifted(plane: &[u8], (dx, dy): (i64, i64), fill: u8) -> Vec<u8> {
    let (cw, ch) = (i64::from(W), i64::from(H));
    (0..ch)
        .flat_map(|y| (0..cw).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (sx, sy) = (x - dx, y - dy);
            if sx < 0 || sy < 0 || sx >= cw || sy >= ch {
                fill
            } else {
                plane[(sy * cw + sx) as usize]
            }
        })
        .collect()
}

/// `multiply_planes`' byte product, `(a·b + 127) / 255`.
fn product(a: u8, b: u8) -> u8 {
    ((u32::from(a) * u32::from(b) + 127) / 255) as u8
}

/// The red channel of the scene at `depth` (8 or 16) with `style`, a
/// `source`-coloured layer of the picture's coverage over white.
fn render(depth: u32, aa: bool, source: f64, style: &Value) -> Vec<f64> {
    let alpha: Vec<f64> = coverage(aa).iter().map(|&c| f64::from(c) / 255.0).collect();
    render_rgb_alpha(depth, W, H, &alpha, [source; 3], [1.0; 3], style)
        .into_iter()
        .map(|p| p[0])
        .collect()
}

/// Every pixel of `got` within one code of `want` (both 8-bit codes).
fn assert_codes(got: &[f64], want: &[f64], what: &str) {
    let mut lit = 0;
    for (i, (&g, &w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g - w).abs() <= 1.0 + 1e-9,
            "{what}: pixel ({}, {}) {g:.2} against {w:.2}",
            i as u32 % W,
            i as u32 / W
        );
        lit += usize::from(w < 254.5);
    }
    assert!(lit > 100, "{what}: only {lit} pixels lit");
}

#[test]
fn the_drop_shadow_is_the_box_pair_of_its_band_at_the_light_offset() {
    // Size 6, spread 50 %: n 3, w 3; angle 90°, distance 2 -> offset (0, 2).
    for aa in [false, true] {
        let u = coverage(aa);
        let m: Vec<u8> = law(&u, 6.0, 0.5).into_iter().map(byte).collect();
        let want: Vec<f64> = shifted(&m, (0, 2), 0)
            .iter()
            .map(|&c| 255.0 - f64::from(c))
            .collect();
        let style = json!({"fill_opacity":0,"effects":[{"type":"drop_shadow","blend":"normal",
            "color":"#000000","opacity":1,"size":6,"spread":0.5,"distance":2,"angle":90,
            "use_global_light":false,"layer_knocks_out":false}]});
        for depth in [8, 16] {
            let what = format!("drop shadow, aa {aa}, {depth} bits");
            assert_codes(&render(depth, aa, 0.0, &style), &want, &what);
        }
    }
}

#[test]
fn the_inner_shadow_is_the_box_pair_of_the_inverse_clipped_to_the_shape() {
    // Size 5, choke 40 %: n 2, w 3; angle 180°, distance 3 -> offset (3, 0).
    for aa in [false, true] {
        let u = coverage(aa);
        let inverse: Vec<u8> = u.iter().map(|&c| 255 - c).collect();
        let m: Vec<u8> = law(&inverse, 5.0, 0.4).into_iter().map(byte).collect();
        let want: Vec<f64> = shifted(&m, (3, 0), 255)
            .iter()
            .zip(&u)
            .map(|(&c, &a)| 255.0 - f64::from(product(c, a)))
            .collect();
        let style = json!({"fill_opacity":0,"effects":[{"type":"inner_shadow","blend":"normal",
            "color":"#000000","opacity":1,"size":5,"choke":0.4,"distance":3,"angle":180,
            "use_global_light":false}]});
        for depth in [8, 16] {
            let what = format!("inner shadow, aa {aa}, {depth} bits");
            assert_codes(&render(depth, aa, 0.0, &style), &want, &what);
        }
    }
}

/// The Outer Glow's visible share per pixel (module doc): `g (1 − a)`, `g`
/// the law at `range`, the plane knocked out in proportion first.
fn glow_want(u: &[u8], size: f64, spread: f64, range: f64) -> Vec<f64> {
    law(u, size, spread)
        .iter()
        .zip(u)
        .map(|(&b, &a)| {
            let g = byte((b / range).min(1.0));
            let plane = product(g, 255 - a);
            if a == 0 || a == 255 {
                255.0 - f64::from(plane)
            } else {
                // Under the partial pixel the plane lands at `sa / (1 − a)`
                // and the layer's own pass leaves `(1 − a)` of it: `g (1 −
                // a)` up to the plane's byte.
                let cover = 1.0 - f64::from(a) / 255.0;
                let landed = (f64::from(plane) / 255.0 / cover).min(1.0);
                255.0 * (1.0 - landed * cover)
            }
        })
        .collect()
}

#[test]
fn the_outer_glow_reads_g_times_one_minus_alpha_under_a_covering_layer() {
    // Size 8, spread 25 %: n 2, w 6; Range 50 % and 100 %.
    for aa in [false, true] {
        let u = coverage(aa);
        for range in [0.5, 1.0] {
            let want = glow_want(&u, 8.0, 0.25, range);
            let style = json!({"effects":[{"type":"outer_glow","blend":"normal",
                "color":"#000000","opacity":1,"size":8,"spread":0.25,"range":range}]});
            for depth in [8, 16] {
                let what = format!("outer glow, aa {aa}, range {range}, {depth} bits");
                assert_codes(&render(depth, aa, 1.0, &style), &want, &what);
            }
        }
    }
}

#[test]
fn the_outer_glow_is_a_hollow_halo_at_fill_zero() {
    // Fill 0 %: the layer draws nothing, the plane alone shows — `g (1 −
    // a)` again, nothing under the solid rect.
    for aa in [false, true] {
        let u = coverage(aa);
        let want: Vec<f64> = law(&u, 8.0, 0.25)
            .iter()
            .zip(&u)
            .map(|(&b, &a)| 255.0 - f64::from(product(byte((b / 0.5).min(1.0)), 255 - a)))
            .collect();
        let style = json!({"fill_opacity":0,"effects":[{"type":"outer_glow","blend":"normal",
            "color":"#000000","opacity":1,"size":8,"spread":0.25}]});
        for depth in [8, 16] {
            let got = render(depth, aa, 1.0, &style);
            assert_codes(&got, &want, &format!("hollow halo, aa {aa}, {depth} bits"));
            assert!(
                (got[(28 * W + 30) as usize] - 255.0).abs() <= 0.5,
                "nothing under the solid rect"
            );
        }
    }
}

#[test]
fn the_anti_aliased_band_moves_with_the_partial_pixel() {
    // The law's band is measured from the anti-aliased contour: past the
    // partial column (alpha 0.4) the band of n = 3 reads `n − (d − 0.4)`, so
    // where the hard rect's band has ended (x = 47, 4 px from its last full
    // column) the anti-aliased one still reads 0.4 (the binarized band, at
    // 0.4 < 1/2, would ignore the column and read the hard rect's 0).
    let hard = band(&coverage(false), 3);
    let soft = band(&coverage(true), 3);
    let at = |b: &[u8], x: u32| b[(28 * W + x) as usize];
    assert_eq!(
        (at(&hard, 46), at(&soft, 46)),
        (255, 255),
        "3 px past: in both bands"
    );
    assert_eq!(
        at(&hard, 47),
        0,
        "4 px past the hard rect: its band has ended"
    );
    assert_eq!(
        at(&soft, 47),
        round_byte(3.0 - (3.0 - 0.4)),
        "3 px past the column"
    );
    assert_eq!(at(&soft, 48), 0, "4 px past the column");
    // And the drop shadow renders that difference (Fill 0 %, distance 0;
    // size 4, spread 75 %: n 3, w 1 acting as 2).
    let style = json!({"fill_opacity":0,"effects":[{"type":"drop_shadow","blend":"normal",
        "color":"#000000","opacity":1,"size":4,"spread":0.75,"distance":0,
        "layer_knocks_out":false}]});
    let (h, s) = (render(8, false, 0.0, &style), render(8, true, 0.0, &style));
    assert_ne!(h[(28 * W + 47) as usize], s[(28 * W + 47) as usize]);
}
