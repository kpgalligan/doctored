//! #2 part 4's two built rules against independent brute-force oracles,
//! through the FFI (`common::styled` sets the style with
//! `dr_doc_set_layer_style`; the 16-bit renders go through
//! `dr_precision_doc_edit`), on a hard rect with an anti-aliased edge column
//! and on an anti-aliased disc, at 8 and 16 bits. The runner is
//! `scripts/photoshop-probe/effect_kernels_578_p4_take.py`; each rule was
//! CONFIRMED on its take (`effect-kernels-578-p4/take1`).
//!
//! The oracles, written here from the runner's CANDIDATES and never from the
//! crate's code (`u` the coverage bytes as unit values, `a` the layer's
//! alpha, the same bytes; `w` the size rounded half up, 1 acting as 2; `T(j)
//! = (w − |j|) / w²` for `|j| < w`, every tap visited; `(dx, dy) = (round(−d
//! cos θ), round(d sin θ))`, a pixel `q` reading `p = q − (dx, dy)`):
//!
//! - IC1, the contoured Inner Shadow at Choke 0: `m` the separable `T` over
//!   `1 − u` (1 off the canvas), the preset's measured table `C`
//!   (`fixtures/psd_photoshop/contour/tables.json`) read piecewise linearly
//!   at `255 m` and rounded once, `X(q) = X(p)` (`C(255)` off the canvas),
//!   the shadow's coverage `X u`. A white Normal 100 % shadow on a black
//!   layer over opaque black reads `X u` on red.
//! - DK1, the Drop Shadow's landing at Spread 0: `m` the separable `T` over
//!   `u` (0 off the canvas), rounded once; `M(q) = m(p)`; with Layer Knocks
//!   Out on, `M (1 − u_q)` and `sa = M (1 − u_q) o / (1 − a_q f)` where `0 <
//!   a_q f < 1` (capped at 1), else `M o` (Knocks Out off); the backdrop `B`
//!   takes the shadow `C` at `sa` with its blend (Normal `C`, Multiply `B
//!   C`), then the layer's colour `S` lands at `a f` (f the Fill).
//!
//! Bound: two codes per sample at 8 bits and two 8-bit codes at 16 bits
//! (the kernel's byte roundings, its byte opacity and Fill at 8 bits, and
//! the 16-bit composite's own rounding against the oracle's f64).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::identify_578::{render_rgb_alpha, supersampled};

const W: u32 = 80;
const H: u32 = 64;
const TOL: f64 = 2.0;
/// The rect x [24, 56), y [20, 44); its column x = 56 at alpha 102 (0.4).
const RECT: (u32, u32, u32, u32) = (24, 20, 32, 24);
const EDGE: u8 = 102;

/// The picture's coverage bytes (module doc).
fn coverage(shape: &str) -> Vec<u8> {
    match shape {
        "rect" => {
            let (x0, y0, rw, rh) = RECT;
            (0..H)
                .flat_map(|y| (0..W).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let rows = (y0..y0 + rh).contains(&y);
                    if rows && (x0..x0 + rw).contains(&x) {
                        255
                    } else if rows && x == x0 + rw {
                        EDGE
                    } else {
                        0
                    }
                })
                .collect()
        }
        "disc" => supersampled(W, H, |x, y| (x - 40.0).hypot(y - 32.0) <= 15.0)
            .iter()
            .map(|&a| (255.0 * a + 0.5).floor() as u8)
            .collect(),
        other => panic!("unknown shape {other}"),
    }
}

fn tables() -> BTreeMap<String, Vec<f64>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/contour/tables.json");
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("tables.json")).expect("json");
    v["tables"]
        .as_object()
        .expect("tables")
        .iter()
        .map(|(k, t)| {
            let t: Vec<f64> = t
                .as_array()
                .expect("table")
                .iter()
                .map(|v| v.as_f64().expect("entry"))
                .collect();
            (k.clone(), t)
        })
        .collect()
}

/// The box pair's width for a size of `v` px (module doc).
fn width(v: f64) -> i64 {
    let s = (v + 0.5).floor().max(1.0);
    if s == 1.0 {
        2
    } else {
        s as i64
    }
}

/// The separable triangle of `w` over `at`, every tap (module doc).
fn triangle(at: impl Fn(i64, i64) -> f64, w: i64) -> Vec<f64> {
    let tap = |j: i64| (w - j.abs()) as f64 / (w * w) as f64;
    let (cw, ch) = (i64::from(W), i64::from(H));
    (0..ch)
        .flat_map(|y| (0..cw).map(move |x| (x, y)))
        .map(|(x, y)| {
            let mut sum = 0.0;
            for j in 1 - w..w {
                for k in 1 - w..w {
                    sum += tap(j) * tap(k) * at(x + k, y + j);
                }
            }
            sum
        })
        .collect()
}

/// `(dx, dy)` of a light at `angle` degrees and `distance` px (module doc).
fn offset(angle: f64, distance: f64) -> (i64, i64) {
    let a = angle.to_radians();
    (
        (-distance * a.cos()).round() as i64,
        (distance * a.sin()).round() as i64,
    )
}

/// The table read piecewise linearly at the fractional code `at`.
fn read(table: &[f64], at: f64) -> f64 {
    let at = at.clamp(0.0, 255.0);
    let i = (at.floor() as usize).min(254);
    table[i] + (at - i as f64) * (table[i + 1] - table[i])
}

fn inside(x: i64, y: i64) -> bool {
    (0..i64::from(W)).contains(&x) && (0..i64::from(H)).contains(&y)
}

fn index(x: i64, y: i64) -> usize {
    (y * i64::from(W) + x) as usize
}

/// IC1's red codes (module doc).
fn ic1_oracle(u: &[u8], table: &[f64], size: f64, angle: f64, distance: f64) -> Vec<f64> {
    let unit = |x: i64, y: i64| f64::from(u[index(x, y)]) / 255.0;
    let m = triangle(
        |x, y| if inside(x, y) { 1.0 - unit(x, y) } else { 1.0 },
        width(size),
    );
    let x_codes: Vec<f64> = m
        .iter()
        .map(|&v| (read(table, 255.0 * v) + 0.5).floor())
        .collect();
    let (dx, dy) = offset(angle, distance);
    (0..i64::from(H))
        .flat_map(|y| (0..i64::from(W)).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (px, py) = (x - dx, y - dy);
            let xs = if inside(px, py) {
                x_codes[index(px, py)]
            } else {
                table[255]
            };
            xs * unit(x, y)
        })
        .collect()
}

#[test]
fn a_contoured_inner_shadow_reads_its_contour_on_the_unrounded_matte() {
    let tables = tables();
    let identity: Vec<f64> = (0..256).map(f64::from).collect();
    let mut worst = 0.0f64;
    for shape in ["rect", "disc"] {
        let u = coverage(shape);
        let alpha: Vec<f64> = u.iter().map(|&b| f64::from(b) / 255.0).collect();
        for contour in ["linear", "cone", "half_round", "ring"] {
            let table = if contour == "linear" {
                &identity
            } else {
                &tables[contour]
            };
            for (size, angle, distance) in [(6.0, 90.0, 4.0), (9.0, 0.0, 3.0)] {
                let mut fx = json!({"type":"inner_shadow","blend":"normal","color":"#ffffff",
                    "opacity":1,"use_global_light":false,"angle":angle,"distance":distance,
                    "size":size,"choke":0,"noise":0});
                if contour != "linear" {
                    fx["contour"] = json!({ "preset": contour });
                }
                let style = json!({ "effects": [fx] });
                let want = ic1_oracle(&u, table, size, angle, distance);
                for depth in [8, 16] {
                    let got = render_rgb_alpha(depth, W, H, &alpha, [0.0; 3], [0.0; 3], &style);
                    let e = got
                        .iter()
                        .zip(&want)
                        .map(|(g, w)| (g[0] - w).abs())
                        .fold(0.0, f64::max);
                    worst = worst.max(e);
                    assert!(
                        e <= TOL,
                        "{shape} {contour} size {size} at {depth} bits: {e:.3} codes"
                    );
                }
            }
        }
    }
    eprintln!("IC1 oracle: worst {worst:.3} codes");
}

/// DK1's RGB codes (module doc); `knocked` false is Knocks Out off.
#[allow(clippy::too_many_arguments)]
fn dk1_oracle(
    u: &[u8],
    size: f64,
    angle: f64,
    distance: f64,
    knocked: bool,
    fill: f64,
    multiply: bool,
    opacity: f64,
    under_layer: bool,
) -> Vec<[f64; 3]> {
    let (b, s, c) = (BACKDROP.map(unit), SOURCE.map(unit), SHADOW.map(unit));
    let at = |x: i64, y: i64| f64::from(u[index(x, y)]) / 255.0;
    let m: Vec<f64> = triangle(
        |x, y| if inside(x, y) { at(x, y) } else { 0.0 },
        width(size),
    )
    .iter()
    .map(|&v| (255.0 * v + 0.5).floor() / 255.0)
    .collect();
    let (dx, dy) = offset(angle, distance);
    (0..i64::from(H))
        .flat_map(|y| (0..i64::from(W)).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (px, py) = (x - dx, y - dy);
            let big_m = if inside(px, py) {
                m[index(px, py)]
            } else {
                0.0
            };
            let a = at(x, y);
            let mut sa = big_m * opacity;
            if knocked {
                sa *= 1.0 - a;
                let la = a * fill;
                if under_layer && la > 0.0 && la < 1.0 {
                    sa = (sa / (1.0 - la)).min(1.0);
                }
            }
            [0, 1, 2].map(|k| {
                let mixed = if multiply { b[k] * c[k] } else { c[k] };
                let acc = b[k] + sa * (mixed - b[k]);
                255.0 * (acc + a * fill * (s[k] - acc))
            })
        })
        .collect()
}

const BACKDROP: [u8; 3] = [196, 204, 92];
const SOURCE: [u8; 3] = [24, 64, 148];
const SHADOW: [u8; 3] = [48, 24, 112];

fn unit(v: u8) -> f64 {
    f64::from(v) / 255.0
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[test]
fn a_knocked_out_drop_shadow_lands_under_the_layer_at_every_fill() {
    let (size, angle, distance) = (6.0, 90.0, 4.0);
    let mut worst = 0.0f64;
    let mut old_miss = 0.0f64;
    for shape in ["rect", "disc"] {
        let u = coverage(shape);
        let alpha: Vec<f64> = u.iter().map(|&b| f64::from(b) / 255.0).collect();
        for knocked in [true, false] {
            for fill in [1.0, 0.5, 0.0] {
                for (multiply, opacity) in [(false, 0.8), (true, 0.75)] {
                    let fx = json!({"type":"drop_shadow",
                        "blend": if multiply { "multiply" } else { "normal" },
                        "color": hex(SHADOW), "opacity": opacity, "use_global_light": false,
                        "angle": angle, "distance": distance, "size": size, "spread": 0,
                        "noise": 0, "layer_knocks_out": knocked});
                    let style = json!({"fill_opacity": fill, "effects": [fx]});
                    let want = dk1_oracle(
                        &u, size, angle, distance, knocked, fill, multiply, opacity, true,
                    );
                    let old = dk1_oracle(
                        &u, size, angle, distance, knocked, fill, multiply, opacity, false,
                    );
                    for depth in [8, 16] {
                        let got = render_rgb_alpha(
                            depth,
                            W,
                            H,
                            &alpha,
                            SOURCE.map(unit),
                            BACKDROP.map(unit),
                            &style,
                        );
                        let mut e = 0.0f64;
                        let mut o = 0.0f64;
                        for ((g, w), t) in got.iter().zip(&want).zip(&old) {
                            for k in 0..3 {
                                e = e.max((g[k] - w[k]).abs());
                                o = o.max((g[k] - t[k]).abs());
                            }
                        }
                        worst = worst.max(e);
                        if knocked && fill > 0.0 {
                            old_miss = old_miss.max(o);
                        }
                        assert!(
                            e <= TOL,
                            "{shape} knocked {knocked} fill {fill} multiply {multiply} at \
                             {depth} bits: {e:.3} codes"
                        );
                    }
                }
            }
        }
    }
    eprintln!("DK1 oracle: worst {worst:.3} codes; the replaced landing misses by {old_miss:.3}");
    assert!(
        old_miss > 10.0,
        "the replaced landing (the shadow at sa under the layer) must miss the render \
         on the partial pixels: {old_miss:.3} codes"
    );
}
