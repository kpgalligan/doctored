//! The Satin's box-pair matte and per-copy contour (#2 part 3,
//! `scripts/photoshop-probe/effect_kernels_578_p3_take.py`, its candidates
//! SA1 and SA2l, built as the satin build rule says) against an independent
//! brute-force oracle, through the FFI (`common::styled` sets the style with
//! `dr_doc_set_layer_style`; the 16-bit rows go through
//! `dr_precision_doc_edit`), on a hard rect with an anti-aliased edge column
//! and on an anti-aliased disc.
//!
//! The oracle, written here from the runner's CANDIDATES and never from the
//! crate's code: `u` the coverage bytes as unit values; `w` the size rounded
//! half up (1 acts as 2); the matte `p` the separable triangle `(w − |j|) /
//! w²` (`|j| < w`) on `u` (the law's band at `n = 0` is `u` itself), every
//! tap visited, rounded once as `floor(255 p + 1/2)`; `(dx, dy) =
//! (round(−d cos a), round(d sin a))`; `a = p(x − dx, y − dy)`, `b = p(x +
//! dx, y + dy)` (0 off the plane); each copy mapped through the preset's
//! measured table (`fixtures/psd_photoshop/contour/tables.json`, the
//! identity for Linear); `s = |C(a) − C(b)|`, `255 − s` under Invert; the
//! satin's coverage `s · u`. A white Normal 100 % satin on a black layer
//! over opaque black reads `s · u` on red.
//!
//! Bound: two codes per pixel at 8 bits and two 8-bit codes at 16 bits (the
//! kernel's byte product `(s u + 127) / 255` against the oracle's f64, and
//! the 16-bit composite's own rounding).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::identify_578::{render_rgb_alpha, supersampled};

const W: u32 = 80;
const H: u32 = 64;
const TOL: f64 = 2.0;
/// The rect x [24, 56), y [20, 44); the column x = 56 at alpha 102 (0.4)
/// in the anti-aliased variant.
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

/// The matte `p` as bytes (module doc), brute force: every tap of the
/// separable triangle, 0 off the plane.
fn matte(u: &[u8], size: f64) -> Vec<u8> {
    let w = width(size);
    let (cw, ch) = (i64::from(W), i64::from(H));
    let tap = |j: i64| (w - j.abs()) as f64 / (w * w) as f64;
    let at = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= cw || y >= ch {
            0.0
        } else {
            f64::from(u[(y * cw + x) as usize]) / 255.0
        }
    };
    (0..ch)
        .flat_map(|y| (0..cw).map(move |x| (x, y)))
        .map(|(x, y)| {
            let mut sum = 0.0;
            for j in 1 - w..w {
                for k in 1 - w..w {
                    sum += tap(j) * tap(k) * at(x + k, y + j);
                }
            }
            (255.0 * sum + 0.5).floor().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// `(round(−d cos a), round(d sin a))`.
fn offset(angle: f64, distance: f64) -> (i64, i64) {
    let a = angle.to_radians();
    (
        (-distance * a.cos()).round() as i64,
        (distance * a.sin()).round() as i64,
    )
}

/// The satin's coverage `s · u` in codes (module doc); `per_copy` false
/// maps the combined plane instead (the site the build replaced).
fn oracle(
    u: &[u8],
    size: f64,
    distance: f64,
    angle: f64,
    invert: bool,
    table: Option<&[f64]>,
    per_copy: bool,
) -> Vec<f64> {
    let p = matte(u, size);
    let (dx, dy) = offset(angle, distance);
    let (cw, ch) = (i64::from(W), i64::from(H));
    let read = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= cw || y >= ch {
            0.0
        } else {
            f64::from(p[(y * cw + x) as usize])
        }
    };
    let map = |v: f64| table.map_or(v, |t| t[v as usize]);
    (0..ch)
        .flat_map(|y| (0..cw).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (a, b) = (read(x - dx, y - dy), read(x + dx, y + dy));
            let s = if per_copy {
                (map(a) - map(b)).abs()
            } else {
                map((a - b).abs())
            };
            let s = if invert { 255.0 - s } else { s };
            s * f64::from(u[(y * cw + x) as usize]) / 255.0
        })
        .collect()
}

/// The red codes of a white Normal 100 % satin on a black layer of `u` over
/// opaque black at `depth` (module doc).
fn render(depth: u32, u: &[u8], style: &Value) -> Vec<f64> {
    let alpha: Vec<f64> = u.iter().map(|&c| f64::from(c) / 255.0).collect();
    render_rgb_alpha(depth, W, H, &alpha, [0.0; 3], [0.0; 3], style)
        .into_iter()
        .map(|p| p[0])
        .collect()
}

fn satin_style(size: f64, distance: f64, angle: f64, invert: bool, contour: Option<&str>) -> Value {
    let mut fx = json!({"type":"satin","blend":"normal","color":"#ffffff","opacity":1,
        "size":size,"distance":distance,"angle":angle,"invert":invert});
    if let Some(preset) = contour {
        fx["contour"] = json!({ "preset": preset });
    }
    json!({ "effects": [fx] })
}

/// Every pixel of `got` within [`TOL`] of `want` (both 8-bit codes), with
/// enough of the sheen lit to matter.
fn assert_codes(got: &[f64], want: &[f64], what: &str) {
    let mut lit = 0;
    for (i, (&g, &w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g - w).abs() <= TOL + 1e-9,
            "{what}: pixel ({}, {}) {g:.2} against {w:.2}",
            i as u32 % W,
            i as u32 / W
        );
        lit += usize::from(w > 0.5);
    }
    assert!(lit > 100, "{what}: only {lit} pixels lit");
}

#[test]
fn a_linear_satin_is_the_box_pair_matte_of_the_coverage() {
    for shape in ["rect", "disc"] {
        let u = coverage(shape);
        // No offset lands on a half (`3 cos 120°` would: a rounding tie).
        for (size, distance, angle) in [(6.0, 4.0, 30.0), (9.0, 3.0, 110.0), (1.0, 2.0, 90.0)] {
            for invert in [false, true] {
                let want = oracle(&u, size, distance, angle, invert, None, true);
                let style = satin_style(size, distance, angle, invert, None);
                for depth in [8, 16] {
                    let what =
                        format!("{shape} size {size} d {distance} a {angle} i {invert}, {depth}");
                    assert_codes(&render(depth, &u, &style), &want, &what);
                }
            }
        }
    }
}

#[test]
fn a_contoured_satin_maps_each_shifted_copy_before_the_combine() {
    let tables = tables();
    for shape in ["rect", "disc"] {
        let u = coverage(shape);
        for preset in ["cone", "half_round", "rolling_slope_descending"] {
            let table = tables[preset].as_slice();
            for invert in [false, true] {
                let want = oracle(&u, 8.0, 4.0, 30.0, invert, Some(table), true);
                let style = satin_style(8.0, 4.0, 30.0, invert, Some(preset));
                for depth in [8, 16] {
                    let what = format!("{shape} {preset} i {invert}, {depth} bits");
                    assert_codes(&render(depth, &u, &style), &want, &what);
                }
            }
        }
    }
}

#[test]
fn the_per_copy_site_is_not_the_combined_plane_site() {
    // The oracle discriminates the built site: on a Cone satin the combined
    // plane's map, the site the build replaced, misses the render by more
    // than the bound somewhere.
    let tables = tables();
    let u = coverage("disc");
    let table = tables["cone"].as_slice();
    let got = render(8, &u, &satin_style(8.0, 4.0, 30.0, false, Some("cone")));
    let old = oracle(&u, 8.0, 4.0, 30.0, false, Some(table), false);
    let worst = got
        .iter()
        .zip(&old)
        .map(|(g, o)| (g - o).abs())
        .fold(0.0, f64::max);
    assert!(
        worst > 20.0,
        "the combined-plane site misses by {worst:.1} codes"
    );
}
