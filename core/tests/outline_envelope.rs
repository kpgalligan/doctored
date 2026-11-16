//! Warp Text's envelope through the FFI (`dr_outline_map`'s
//! `"kind":"envelope"` map and `dr_outline_map_bounds`), against an oracle
//! written here from designs/warp-text.md WT-2 and its outside-the-box rule:
//! every output vertex is the oracle's image of a point of its source
//! segment, dense oracle samples lie within the tolerance of the output, the
//! bounds hold the image of every interior point and (with no distortion)
//! meet it on every side, and `arc` keeps every radius positive and the
//! text's order. Nothing is imported from the kernels.

use std::f64::consts::PI;
use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_path_along::dr_outline_map_bounds;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::outline_map::{at, gap, glyphish, map_raw, polylines, segments, to_polylines, Seg};
use crate::path_shape::{corner, shape_of};

type Point = [f64; 2];

const STYLES: [&str; 9] = [
    "arch", "upper", "lower", "bulge", "squeeze", "inflate", "flag", "rise", "arc",
];

/// The bend rows: (bend, horizontal, vertical); the last one distorts.
const ROWS: [(f64, f64, f64); 5] = [
    (-1.0, 0.0, 0.0),
    (-0.5, 0.0, 0.0),
    (0.5, 0.0, 0.0),
    (1.0, 0.0, 0.0),
    (0.6, 0.5, -0.7),
];

#[derive(Clone, Copy)]
struct Warp {
    style: &'static str,
    rect: [f64; 4],
    bend: f64,
    horizontal: f64,
    vertical: f64,
}

impl Warp {
    fn json(&self) -> Value {
        json!({"kind": "envelope", "style": self.style, "rect": self.rect, "bend": self.bend,
               "horizontal": self.horizontal, "vertical": self.vertical})
    }

    /// WT-2 written out: the style, then the two distortions, the fractions
    /// clamped inside every weight while x and y are not; `arc` past the box
    /// by the stated rule.
    fn oracle(&self, q: Point) -> Point {
        let [x0, y0, w, h] = self.rect;
        let (xc, yc) = (x0 + w / 2.0, y0 + h / 2.0);
        let [x, y] = q;
        let u = (x - x0) / w;
        let v = (y - y0) / h;
        let uc = u.clamp(0.0, 1.0);
        let vc = v.clamp(0.0, 1.0);
        let c = 2.0 * uc - 1.0;
        let d = 2.0 * vc - 1.0;
        let bow = |t: f64| 1.0 - t * t;
        let b = self.bend;
        let (xs, ys) = match self.style {
            "arch" => (x, y - b * (h / 2.0) * bow(c)),
            "upper" => (x, y - b * (h / 2.0) * bow(c) * (1.0 - vc)),
            "lower" => (x, y + b * (h / 2.0) * bow(c) * vc),
            "bulge" => (x, yc + (y - yc) * (1.0 + 0.75 * b * bow(c))),
            "squeeze" => (xc + (x - xc) * (1.0 - 0.75 * b * bow(d)), y),
            "inflate" => (
                xc + (x - xc) * (1.0 + 0.5 * b * bow(d)),
                yc + (y - yc) * (1.0 + 0.5 * b * bow(c)),
            ),
            "flag" => (x, y + b * (h / 2.0) * (2.0 * PI * uc).sin()),
            "rise" => (x, y - b * (h / 2.0) * (PI * (uc - 0.5)).sin()),
            "arc" => arc_oracle(self.rect, b, q),
            other => panic!("no style {other}"),
        };
        [
            xc + (xs - xc) * (1.0 + 0.75 * self.vertical * d),
            yc + (ys - yc) * (1.0 + 0.75 * self.horizontal * c),
        ]
    }
}

/// `arc`: the box on a circle of radius R = w/(π|b|); outside it the radius
/// grows on the outer side, is R²/(R + e) on the inner side, and an end
/// overhang runs along the end's tangent.
fn arc_oracle(rect: [f64; 4], b: f64, q: Point) -> (f64, f64) {
    let [x0, y0, w, h] = rect;
    if b.abs() < 1e-6 {
        return (q[0], q[1]);
    }
    let big_r = w / (PI * b.abs());
    let u = (q[0] - x0) / w;
    let v = (q[1] - y0) / h;
    let uc = u.clamp(0.0, 1.0);
    let theta = PI * b.abs() * (uc - 0.5);
    let t = q[0] - (x0 + uc * w);
    if b > 0.0 {
        let centre = (x0 + w / 2.0, y0 + h + big_r);
        let r = if v > 1.0 {
            big_r * big_r / (big_r + h * (v - 1.0))
        } else {
            big_r + h * (1.0 - v)
        };
        (
            centre.0 + r * theta.sin() + t * theta.cos(),
            centre.1 - r * theta.cos() + t * theta.sin(),
        )
    } else {
        let centre = (x0 + w / 2.0, y0 - big_r);
        let r = if v < 0.0 {
            big_r * big_r / (big_r - h * v)
        } else {
            big_r + h * v
        };
        (
            centre.0 + r * theta.sin() + t * theta.cos(),
            centre.1 + r * theta.cos() - t * theta.sin(),
        )
    }
}

/// The arc's centre and inner radius.
fn arc_centre(rect: [f64; 4], b: f64) -> (Point, f64) {
    let [x0, y0, w, h] = rect;
    let big_r = w / (PI * b.abs());
    let cy = if b > 0.0 { y0 + h + big_r } else { y0 - big_r };
    ([x0 + w / 2.0, cy], big_r)
}

fn bounds_raw(map: &Value, region: [f64; 4]) -> Result<[f64; 4], String> {
    let json = CString::new(map.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_outline_map_bounds(
            json.as_ptr(),
            region[0],
            region[1],
            region[2],
            region[3],
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let parsed: Vec<f64> = serde_json::from_str(&crate::path_geometry::take(out)).unwrap();
    Ok([parsed[0], parsed[1], parsed[2], parsed[3]])
}

/// A deterministic stream of fractions in [0, 1).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The parameter of `seg` whose oracle image lies nearest `vertex`: the
/// nearest of 1024 samples, refined by a ternary search around it.
fn nearest_on(warp: &Warp, seg: Seg, vertex: Point) -> f64 {
    let n = 1024;
    let mut best = (f64::MAX, 0usize);
    for k in 0..=n {
        let d = gap(warp.oracle(at(seg, k as f64 / n as f64)), vertex);
        if d < best.0 {
            best = (d, k);
        }
    }
    let (mut lo, mut hi) = (
        (best.1 as f64 - 2.0).max(0.0) / n as f64,
        (best.1 as f64 + 2.0).min(n as f64) / n as f64,
    );
    let off = |t: f64| gap(warp.oracle(at(seg, t)), vertex);
    for _ in 0..120 {
        let a = lo + (hi - lo) / 3.0;
        let b = hi - (hi - lo) / 3.0;
        if off(a) <= off(b) {
            hi = b;
        } else {
            lo = a;
        }
    }
    off(0.5 * (lo + hi)).min(best.0)
}

/// Every output vertex is the oracle image of a point of a source segment
/// (to 1e-9), and dense oracle samples lie within `reach` of the output.
fn assert_follows(
    warp: &Warp,
    (verbs, points): (&[u8], &[Point]),
    tolerance: f64,
    reach: f64,
    what: &str,
) {
    let got = map_raw(verbs, points, &warp.json(), tolerance)
        .unwrap_or_else(|e| panic!("{what}: refused: {e}"));
    let source = segments(verbs, points);
    let dense: Vec<Vec<Point>> = source
        .iter()
        .map(|&s| {
            (0..=1024)
                .map(|k| warp.oracle(at(s, k as f64 / 1024.0)))
                .collect()
        })
        .collect();
    let boxes: Vec<[f64; 4]> = dense
        .iter()
        .map(|line| {
            line.iter()
                .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| {
                    [
                        b[0].min(p[0]),
                        b[1].min(p[1]),
                        b[2].max(p[0]),
                        b[3].max(p[1]),
                    ]
                })
        })
        .collect();
    for v in &got.points {
        let off = source
            .iter()
            .zip(&boxes)
            .filter(|(_, b)| {
                v[0] >= b[0] - 1.0 && v[0] <= b[2] + 1.0 && v[1] >= b[1] - 1.0 && v[1] <= b[3] + 1.0
            })
            .map(|(&s, _)| nearest_on(warp, s, *v))
            .fold(f64::MAX, f64::min);
        assert!(
            off <= 1e-9,
            "{what}: vertex {v:?} is {off} px off the oracle's outline"
        );
    }
    let lines = polylines(&got);
    for sample in dense.iter().flatten() {
        let d = to_polylines(*sample, &lines);
        assert!(
            d <= reach + 1e-9,
            "{what}: sample {sample:?} is {d} px off the output"
        );
    }
}

#[test]
fn every_style_follows_its_formula_inside_and_overhanging_the_box() {
    let (verbs, points) = glyphish();
    // glyphish spans x 0..120 and y 5..50: one box holds it, the other is
    // overhung on every side (above, below and past both ends).
    let inside = [-10.0, 0.0, 140.0, 60.0];
    let overhung = [20.0, 12.0, 60.0, 28.0];
    let tolerance = 0.05;
    // Inside the box the map is smooth, so the output is within the
    // tolerance everywhere. An overhang crosses the clamp at the box's edge,
    // where the map has a corner: a piece is accepted on its midpoint and
    // quarter points, and a corner lying between two of them pokes past the
    // chord further than they do (a straight-sided one at the piece's
    // fraction s by min(2s, 4(1 − s)) times the tolerance, up to 4/3; bowed
    // sides add to that). The overhang is held to twice the tolerance.
    for style in STYLES {
        for (bend, horizontal, vertical) in ROWS {
            for (rect, reach) in [(inside, tolerance), (overhung, tolerance * 2.0)] {
                let warp = Warp {
                    style,
                    rect,
                    bend,
                    horizontal,
                    vertical,
                };
                let what = format!("{style} b {bend} h {horizontal} v {vertical} rect {rect:?}");
                assert_follows(&warp, (&verbs, &points), tolerance, reach, &what);
            }
        }
    }
}

#[test]
fn the_bounds_hold_every_interior_point_and_meet_each_side() {
    let rect = [10.0, 20.0, 100.0, 40.0];
    let larger = [0.0, 16.0, 120.0, 48.0];
    let mut random = Lcg(0x5eed);
    for style in STYLES {
        for (bend, horizontal, vertical) in ROWS {
            for region in [rect, larger] {
                let warp = Warp {
                    style,
                    rect,
                    bend,
                    horizontal,
                    vertical,
                };
                let what =
                    format!("{style} b {bend} h {horizontal} v {vertical} region {region:?}");
                let bounds = bounds_raw(&warp.json(), region).unwrap();
                let slack = 1e-9 * 200.0;
                for _ in 0..100_000 {
                    let q = [
                        region[0] + random.next() * region[2],
                        region[1] + random.next() * region[3],
                    ];
                    let m = warp.oracle(q);
                    assert!(
                        m[0] >= bounds[0] - slack
                            && m[0] <= bounds[2] + slack
                            && m[1] >= bounds[1] - slack
                            && m[1] <= bounds[3] + slack,
                        "{what}: {q:?} maps to {m:?}, outside {bounds:?}"
                    );
                }
                if horizontal != 0.0 || vertical != 0.0 {
                    continue;
                }
                // Tightness: the region's boundary reaches every side.
                let mut reach = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                let [rx, ry, rw, rh] = region;
                for k in 0..=4096 {
                    let f = k as f64 / 4096.0;
                    for q in [
                        [rx + f * rw, ry],
                        [rx + f * rw, ry + rh],
                        [rx, ry + f * rh],
                        [rx + rw, ry + f * rh],
                    ] {
                        let m = warp.oracle(q);
                        reach = [
                            reach[0].min(m[0]),
                            reach[1].min(m[1]),
                            reach[2].max(m[0]),
                            reach[3].max(m[1]),
                        ];
                    }
                }
                let near = 0.01 * rw.max(rh);
                for side in 0..4 {
                    assert!(
                        (reach[side] - bounds[side]).abs() <= near,
                        "{what}: side {side} is {} but the boundary reaches {}",
                        bounds[side],
                        reach[side]
                    );
                }
            }
        }
    }
}

#[test]
fn the_widest_row_of_squeeze_and_inflate_is_the_middle_one() {
    let rect = [0.0, 0.0, 100.0, 40.0];
    for (style, bend, low, high) in [
        ("squeeze", -1.0, -37.5, 137.5),
        ("inflate", 1.0, -25.0, 125.0),
    ] {
        let warp = Warp {
            style,
            rect,
            bend,
            horizontal: 0.0,
            vertical: 0.0,
        };
        let bounds = bounds_raw(&warp.json(), rect).unwrap();
        assert!(
            (bounds[0] - low).abs() <= 1e-9 && (bounds[2] - high).abs() <= 1e-9,
            "{style} b {bend}: x range {} to {}, want {low} to {high}",
            bounds[0],
            bounds[2]
        );
    }
}

#[test]
fn arc_keeps_the_order_and_every_radius_in_its_band() {
    let rect = [0.0, 0.0, 200.0, 30.0];
    for bend in [-1.0, 1.0] {
        let warp = Warp {
            style: "arc",
            rect,
            bend,
            horizontal: 0.0,
            vertical: 0.0,
        };
        let (centre, big_r) = arc_centre(rect, bend);
        let mut random = Lcg(7);
        for _ in 0..10_000 {
            let q = [random.next() * 200.0, random.next() * 30.0];
            let m = map_point(&warp, q);
            let r = gap(m, centre);
            assert!(
                r >= big_r - 1e-9 && r <= big_r + 30.0 + 1e-9,
                "b {bend}: {q:?} lands at radius {r}, outside [{big_r}, {}]",
                big_r + 30.0
            );
            // Two points of one row keep their left-to-right order.
            let left = map_point(&warp, [q[0] * 0.5, q[1]]);
            assert!(
                left[0] < m[0] || q[0] == 0.0,
                "b {bend}: {q:?} passes its left twin"
            );
        }
    }
}

#[test]
fn arc_inner_overhangs_stay_off_the_centre() {
    // A deep inner overhang (2R below a b = 1 box) and a one-glyph box under
    // a padding wider than its radius.
    let deep = [0.0, 0.0, 120.0, 20.0];
    let r_deep = 120.0 / PI;
    let glyph = [0.0, 0.0, 14.0, 28.0];
    for (rect, region, overhang) in [
        (deep, [0.0, 0.0, 120.0, 20.0 + 2.0 * r_deep], 0.0),
        (glyph, [-5.0, -5.0, 24.0, 38.0], 5.0),
    ] {
        let warp = Warp {
            style: "arc",
            rect,
            bend: 1.0,
            horizontal: 0.0,
            vertical: 0.0,
        };
        let (centre, big_r) = arc_centre(rect, 1.0);
        let bounds = bounds_raw(&warp.json(), region).unwrap();
        let ceiling = big_r + rect[3] + 2.0 * overhang + 1e-9;
        let mut random = Lcg(11);
        for _ in 0..100_000 {
            let q = [
                region[0] + random.next() * region[2],
                region[1] + random.next() * region[3],
            ];
            let m = map_point(&warp, q);
            let r = gap(m, centre);
            assert!(
                r > 0.0 && r <= ceiling,
                "{rect:?}: {q:?} lands at radius {r}"
            );
            assert!(
                m[0] >= bounds[0] - 1e-9
                    && m[0] <= bounds[2] + 1e-9
                    && m[1] >= bounds[1] - 1e-9
                    && m[1] <= bounds[3] + 1e-9,
                "{rect:?}: {q:?} maps to {m:?}, outside {bounds:?}"
            );
        }
        // The radius falls as the inner overhang grows, and two points on
        // either side of the centre column keep their order.
        let bottom = rect[1] + rect[3];
        let depth = region[1] + region[3] - bottom;
        let mut last = f64::MAX;
        for k in 0..=64 {
            let y = bottom + depth * k as f64 / 64.0;
            let r = gap(map_point(&warp, [rect[0] + rect[2] * 0.3, y]), centre);
            assert!(
                r < last || k == 0,
                "{rect:?}: radius {r} at y {y} did not fall"
            );
            last = r;
            let left = map_point(&warp, [rect[0] + rect[2] * 0.2, y]);
            let right = map_point(&warp, [rect[0] + rect[2] * 0.8, y]);
            assert!(
                left[0] < centre[0] && right[0] > centre[0],
                "{rect:?}: order lost at y {y}"
            );
        }
    }
}

/// One point through `dr_outline_map`: a tiny move-line outline whose first
/// vertex is the point's image.
fn map_point(warp: &Warp, q: Point) -> Point {
    let got = map_raw(&[0, 1], &[q, [q[0] + 1e-3, q[1]]], &warp.json(), 0.05).unwrap();
    let image = got.points[0];
    let oracle = warp.oracle(q);
    assert!(
        gap(image, oracle) <= 1e-9,
        "{q:?}: {image:?} against the oracle's {oracle:?}"
    );
    image
}

#[test]
fn envelope_refusals_name_the_reason() {
    let line = [[0.0, 0.0], [10.0, 0.0]];
    let refused = |map: Value| match map_raw(&[0, 1], &line, &map, 0.1) {
        Ok(_) => panic!("{map} was accepted"),
        Err(e) => e,
    };
    let good = json!({"kind": "envelope", "style": "arch", "rect": [0, 0, 10, 10], "bend": 0.5});
    assert!(map_raw(&[0, 1], &line, &good, 0.1).is_ok());
    let mut bad = good.clone();
    bad["style"] = json!("twist");
    assert!(refused(bad).contains("style"));
    for key in ["bend", "horizontal", "vertical"] {
        let mut bad = good.clone();
        bad[key] = json!(1.5);
        assert!(refused(bad).contains(key), "{key} 1.5");
    }
    let mut bad = good.clone();
    bad["rect"] = json!([0, 0, 0, 10]);
    assert!(refused(bad).contains("rect"));
    let mut bad = good.clone();
    bad["rect"] = json!([0, 0, 2e9, 10]);
    assert!(refused(bad).contains("rect"));
    // Absent numbers are 0: no bend is the identity.
    let flat = json!({"kind": "envelope", "style": "arc", "rect": [0, 0, 10, 10]});
    let got = map_raw(&[0, 1], &line, &flat, 0.1).unwrap();
    for p in &got.points {
        assert!(p[1].abs() <= 1e-12 && (0.0..=10.0).contains(&p[0]), "{p:?}");
    }

    // The bounds export.
    assert!(bounds_raw(&good, [0.0, 0.0, f64::NAN, 1.0])
        .unwrap_err()
        .contains("region"));
    assert!(bounds_raw(&good, [0.0, 0.0, -1.0, 1.0])
        .unwrap_err()
        .contains("region"));
    assert!(bounds_raw(&good, [0.0, 0.0, 2e9, 1.0])
        .unwrap_err()
        .contains("region"));
    let path = shape_of(
        &[(&[corner(0.0, 0.0), corner(100.0, 0.0)], false)],
        "nonzero",
    );
    let along = json!({"kind": "along", "shape": path, "subpath": 0});
    assert!(bounds_raw(&along, [0.0, 0.0, 1.0, 1.0])
        .unwrap_err()
        .contains("envelope"));
    let mut bad = good.clone();
    bad["bend"] = json!(-2);
    assert!(bounds_raw(&bad, [0.0, 0.0, 1.0, 1.0])
        .unwrap_err()
        .contains("bend"));
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_outline_map_bounds(ptr::null(), 0.0, 0.0, 1.0, 1.0, &mut err) };
    assert!(out.is_null() && take_err_string(err).contains("NULL"));
}
