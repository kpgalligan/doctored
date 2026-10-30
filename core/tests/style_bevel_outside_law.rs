//! The Smooth Outer Bevel's outside cut (#2 part 2 phase 2,
//! `scripts/photoshop-probe/effect_kernels_578_p2_take.py`, its candidate
//! SO1) against an independent brute-force oracle, through the FFI
//! (`common::styled` sets the style with `dr_doc_set_layer_style`; the
//! 16-bit rows go through `dr_precision_doc_edit`), on an anti-aliased disc
//! and a hard rect.
//!
//! The oracle, written here from the runner's CANDIDATES and the law it
//! cuts, never from the crate's code: `a = coverage / 255`; `W` the size
//! rounded half up (1 acts as 2); the height `H` the separable triangle `(W
//! − |j|) / W²` on `a` (reads clamped); `hx = depth · W · (H[x+1] − H[x−1])
//! / 2` and the same in y; the NORMALIZED law `d = (nl − lz) / (1 − lz)`
//! where `nl = n·L >= lz`, `(nl − lz) / lz` below; Soften the same triangle
//! on `d` at the Soften's width. THE CUT, at altitude >= 45 only: `n = W −
//! 1`, and `d = 0` at every pixel of coverage < 128 whose `(2n + 1)²` box
//! holds no pixel of coverage >= 128, every box pixel visited. The
//! highlight `round(255 · clamp(d))` restricted to the outside band
//! `round(hl · (255 − c) / 255)` (the shadow the same on `−d`) is a white
//! Normal pass over black, then the black layer at `a`: red reads `255 ·
//! band / 255 · (1 − a)`.
//!
//! Bound: two codes per pixel at 8 bits and two 8-bit codes at 16 bits
//! (the kernel's f32 and the oracle's f64 round `d` independently), and
//! exactly 0 where the cut zeroes a pixel the uncut law lights.

use serde_json::{json, Value};

use crate::identify_578::{render16_alpha, render8_alpha, supersampled};

const W: u32 = 64;
const H: u32 = 64;
const TOL: f64 = 2.0;

/// The anti-aliased disc (radius 16 at (32, 32)) or the hard 20 × 16 rect
/// at (22, 24), as coverage bytes `floor(255 a + 1/2)`.
fn coverage(shape: &str) -> Vec<u8> {
    match shape {
        "disc" => supersampled(W, H, |x, y| (x - 32.0).hypot(y - 32.0) <= 16.0)
            .iter()
            .map(|&a| (255.0 * a + 0.5).floor() as u8)
            .collect(),
        "rect" => (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .map(|(x, y)| {
                if (22..42).contains(&x) && (24..40).contains(&y) {
                    255
                } else {
                    0
                }
            })
            .collect(),
        other => panic!("unknown shape {other}"),
    }
}

/// The law's width for a size or soften of `v` px (module doc).
fn width(v: f64) -> i64 {
    if v <= 0.0 {
        return 0;
    }
    match (v + 0.5).floor().max(1.0) as i64 {
        1 => 2,
        s => s,
    }
}

/// The separable triangle `(n − |j|) / n²`, x then y, reads clamped; the
/// plane itself at `n = 0`.
fn triangle(plane: &[f64], n: i64) -> Vec<f64> {
    if n == 0 {
        return plane.to_vec();
    }
    let (wi, hi) = (i64::from(W), i64::from(H));
    let tap = |j: i64| (n - j.abs()) as f64 / (n * n) as f64;
    let mut across = vec![0.0; plane.len()];
    for y in 0..hi {
        for x in 0..wi {
            across[(y * wi + x) as usize] = (1 - n..n)
                .map(|j| tap(j) * plane[(y * wi + (x + j).clamp(0, wi - 1)) as usize])
                .sum();
        }
    }
    let mut out = vec![0.0; plane.len()];
    for y in 0..hi {
        for x in 0..wi {
            out[(y * wi + x) as usize] = (1 - n..n)
                .map(|j| tap(j) * across[((y + j).clamp(0, hi - 1) * wi + x) as usize])
                .sum();
        }
    }
    out
}

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

/// One case: the bevel's knobs and the lit side.
#[derive(Clone, Copy)]
struct Case {
    size: f64,
    soften: f64,
    altitude: f64,
    angle: f64,
    depth: f64,
    highlight: bool,
}

fn style(c: &Case) -> Value {
    json!({"effects":[{"type":"bevel_emboss","style":"outer_bevel","technique":"smooth",
        "depth":c.depth,"direction":"up","size":c.size,"soften":c.soften,"angle":c.angle,
        "altitude":c.altitude,"use_global_light":false,
        "highlight_blend":"normal","highlight_color":"#ffffff",
        "highlight_opacity":if c.highlight {1} else {0},
        "shadow_blend":"normal","shadow_color":"#ffffff",
        "shadow_opacity":if c.highlight {0} else {1}}]})
}

/// True where the brute-force cut (module doc) zeroes the pixel at `n`.
fn far(cov: &[u8], n: i64) -> Vec<bool> {
    let (wi, hi) = (i64::from(W), i64::from(H));
    (0..hi)
        .flat_map(|y| (0..wi).map(move |x| (x, y)))
        .map(|(x, y)| {
            if cov[(y * wi + x) as usize] >= 128 {
                return false;
            }
            let near = (-n..=n).any(|dy| {
                (-n..=n).any(|dx| {
                    let (qx, qy) = ((x + dx).clamp(0, wi - 1), (y + dy).clamp(0, hi - 1));
                    cov[(qy * wi + qx) as usize] >= 128
                })
            });
            !near
        })
        .collect()
}

/// The oracle's red codes (module doc), with or without the cut.
fn oracle(cov: &[u8], c: &Case, cut: bool) -> Vec<f64> {
    let a: Vec<f64> = cov.iter().map(|&v| f64::from(v) / 255.0).collect();
    let w = width(c.size);
    let height = triangle(&a, w);
    let (wi, hi) = (i64::from(W), i64::from(H));
    let at = |x: i64, y: i64| height[(y.clamp(0, hi - 1) * wi + x.clamp(0, wi - 1)) as usize];
    let gain = c.depth * w as f64 / 2.0;
    let (ang, alt) = (c.angle.to_radians(), c.altitude.to_radians());
    let l = (alt.cos() * ang.cos(), -alt.cos() * ang.sin(), alt.sin());
    let mut d = Vec::with_capacity(cov.len());
    for y in 0..hi {
        for x in 0..wi {
            let hx = (at(x + 1, y) - at(x - 1, y)) * gain;
            let hy = (at(x, y + 1) - at(x, y - 1)) * gain;
            d.push(normalized(hx, hy, l));
        }
    }
    let mut d = triangle(&d, width(c.soften));
    if cut && c.altitude >= 45.0 {
        for (v, gone) in d.iter_mut().zip(far(cov, w - 1)) {
            if gone {
                *v = 0.0;
            }
        }
    }
    d.iter()
        .zip(cov)
        .map(|(&v, &k)| {
            let side = if c.highlight { v } else { -v };
            let lit = (255.0 * side.clamp(0.0, 1.0)).round() as u32;
            let band = ((lit * u32::from(255 - k) + 127) / 255) as f64;
            band * (1.0 - f64::from(k) / 255.0)
        })
        .collect()
}

fn render(cov: &[u8], c: &Case, depth: u32) -> Vec<f64> {
    if depth == 8 {
        render8_alpha(W, H, cov, &style(c))
    } else {
        let a: Vec<u16> = cov.iter().map(|&v| u16::from(v) * 257).collect();
        render16_alpha(W, H, &a, &style(c))
    }
}

const CASES: [Case; 5] = [
    Case {
        size: 4.0,
        soften: 0.0,
        altitude: 60.0,
        angle: 30.0,
        depth: 1.0,
        highlight: true,
    },
    Case {
        size: 7.0,
        soften: 3.0,
        altitude: 45.0,
        angle: 120.0,
        depth: 1.5,
        highlight: true,
    },
    Case {
        size: 6.0,
        soften: 0.0,
        altitude: 70.0,
        angle: 30.0,
        depth: 1.2,
        highlight: false,
    },
    Case {
        size: 5.0,
        soften: 2.0,
        altitude: 90.0,
        angle: 0.0,
        depth: 1.0,
        highlight: false,
    },
    Case {
        size: 6.0,
        soften: 0.0,
        altitude: 44.0,
        angle: 30.0,
        depth: 1.2,
        highlight: false,
    },
];

#[test]
fn the_outer_bevel_matches_the_cut_law_oracle() {
    let mut zeroed = 0usize;
    let mut beyond_euclid = 0usize;
    for shape in ["disc", "rect"] {
        let cov = coverage(shape);
        for c in CASES {
            let want = oracle(&cov, &c, true);
            let uncut = oracle(&cov, &c, false);
            let n = width(c.size) - 1;
            let gone = far(&cov, n);
            for depth in [8, 16] {
                let got = render(&cov, &c, depth);
                let what = format!(
                    "{shape} size {} soften {} altitude {} {} at {depth} bits",
                    c.size,
                    c.soften,
                    c.altitude,
                    if c.highlight { "highlight" } else { "shadow" }
                );
                for (i, (&g, &o)) in got.iter().zip(&want).enumerate() {
                    let (x, y) = (i as u32 % W, i as u32 / W);
                    assert!(
                        (g - o).abs() <= TOL,
                        "{what} ({x}, {y}): {g} against the oracle's {o}"
                    );
                    if c.altitude >= 45.0 && gone[i] && uncut[i] > TOL {
                        assert_eq!(g, 0.0, "{what} ({x}, {y}): the cut leaves {g}");
                        zeroed += 1;
                    }
                    // A pixel the square support keeps that a Euclidean
                    // count of n + 1/2 would cut: the chessboard metric.
                    if c.altitude >= 45.0 && cov[i] < 128 && !gone[i] && g > TOL {
                        let d = (0..W * H)
                            .filter(|&j| cov[j as usize] >= 128)
                            .map(|j| {
                                let (qx, qy) = (f64::from(j % W), f64::from(j / W));
                                (qx - f64::from(x)).hypot(qy - f64::from(y))
                            })
                            .fold(f64::INFINITY, f64::min);
                        if d > n as f64 + 0.5 {
                            beyond_euclid += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(zeroed > 0, "no case lit a pixel the cut zeroes");
    assert!(
        beyond_euclid > 0,
        "no lit pixel lies past a Euclidean n + 1/2: the metric is unchecked"
    );
}

#[test]
fn below_45_degrees_nothing_is_cut() {
    // Altitude 44: the oracle without the cut, and a pixel the cut would
    // have zeroed stays lit.
    let cov = coverage("disc");
    let c = CASES[4];
    let want = oracle(&cov, &c, false);
    let gone = far(&cov, width(c.size) - 1);
    let got = render(&cov, &c, 8);
    let mut kept = 0;
    for (i, (&g, &o)) in got.iter().zip(&want).enumerate() {
        assert!((g - o).abs() <= TOL, "pixel {i}: {g} against {o}");
        if gone[i] && g > TOL {
            kept += 1;
        }
    }
    assert!(kept > 0, "no pixel past W - 1 is lit at altitude 44");
}

#[test]
fn emboss_and_the_chisels_are_not_cut() {
    // Smooth Emboss lights `W' = round(size / 2)` px outside at altitude 60
    // (its cut was refuted on the confirm take), and a Chisel Hard Outer
    // Bevel is the Chisel's own kernel: each draws past `n = W − 1` px.
    let cov = coverage("rect");
    for (bevel_style, technique, size, n) in [
        ("emboss", "smooth", 8.0, 3),
        ("outer_bevel", "chisel_hard", 6.0, 5),
    ] {
        let s = json!({"effects":[{"type":"bevel_emboss","style":bevel_style,
            "technique":technique,"depth":1.0,"direction":"up","size":size,"soften":0,
            "angle":0,"altitude":60,"use_global_light":false,
            "highlight_blend":"normal","highlight_color":"#ffffff","highlight_opacity":1,
            "shadow_blend":"normal","shadow_color":"#ffffff","shadow_opacity":0}]});
        let got = render8_alpha(W, H, &cov, &s);
        let gone = far(&cov, n);
        let lit = got
            .iter()
            .zip(&gone)
            .filter(|(&g, &past)| past && g > TOL)
            .count();
        assert!(
            lit > 0,
            "{bevel_style} {technique}: nothing lit past {n} px outside"
        );
    }
}
