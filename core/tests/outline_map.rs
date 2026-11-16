//! `dr_outline_map`, an outline bent along one subpath (text on a path's
//! underline and strikethrough), against oracles written here. On a STRAIGHT
//! path the map is a rigid motion, so a line-only outline's output is known
//! exactly (each line split once, at its midpoint) and a curved one's every
//! vertex lies on the moved curve. On a circle of 64 cubic arcs the map is
//! the polar oracle `centre + (r + baseline − y + shift)·(cos, sin)(θ₀ +
//! (start + x)/r)`: every output vertex lies within the stated bounds of the
//! oracle's image of the outline, and dense oracle samples lie within the
//! tolerance plus those bounds of the output polyline. Nothing is imported
//! from the kernels.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_path_along::dr_outline_map;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::path_along::{circle_nodes, length, TAU_PX};
use crate::path_shape::{corner, shape_of};

type Point = [f64; 2];

struct Mapped {
    verbs: Vec<u8>,
    points: Vec<Point>,
}

fn map_raw(verbs: &[u8], points: &[Point], map: &Value, tolerance: f64) -> Result<Mapped, String> {
    let flat: Vec<f64> = points.iter().flatten().copied().collect();
    let json = CString::new(map.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_outline_map(
            verbs.as_ptr(),
            verbs.len(),
            flat.as_ptr(),
            points.len(),
            json.as_ptr(),
            tolerance,
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let parsed: Value = serde_json::from_str(&crate::path_geometry::take(out)).unwrap();
    let verbs = parsed["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    let points = parsed["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
        .collect();
    Ok(Mapped { verbs, points })
}

fn along(shape: &Value, start: f64, baseline: f64, shift: f64) -> Value {
    json!({"kind": "along", "shape": shape, "subpath": 0, "reversed": false,
           "start": start, "baseline": baseline, "shift": shift})
}

fn gap(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn to_segment(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    };
    gap(p, [a[0] + t * dx, a[1] + t * dy])
}

/// The output's closed polylines, one per move.
fn polylines(m: &Mapped) -> Vec<Vec<Point>> {
    let mut out: Vec<Vec<Point>> = Vec::new();
    let mut at = 0;
    for &verb in &m.verbs {
        match verb {
            0 => {
                out.push(vec![m.points[at]]);
                at += 1;
            }
            1 => {
                out.last_mut().unwrap().push(m.points[at]);
                at += 1;
            }
            4 => {
                let first = out.last().unwrap()[0];
                out.last_mut().unwrap().push(first);
            }
            other => panic!("the output holds verb {other}"),
        }
    }
    assert_eq!(at, m.points.len());
    out
}

fn to_polylines(p: Point, lines: &[Vec<Point>]) -> f64 {
    lines
        .iter()
        .flat_map(|l| l.windows(2).map(|w| to_segment(p, w[0], w[1])))
        .fold(f64::MAX, f64::min)
}

/// A source outline as segments in parameter form, for dense sampling.
#[derive(Clone, Copy)]
enum Seg {
    Line(Point, Point),
    Cubic([Point; 4]),
}

fn at(seg: Seg, t: f64) -> Point {
    match seg {
        Seg::Line(a, b) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
        Seg::Cubic(p) => {
            let s = 1.0 - t;
            let w = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
            [0, 1].map(|a| (0..4).map(|k| w[k] * p[k][a]).sum())
        }
    }
}

/// The source segments of a move/line/quad/cubic/close outline, the implicit
/// close of an open subpath included.
fn segments(verbs: &[u8], points: &[Point]) -> Vec<Seg> {
    let (mut out, mut i, mut start, mut pen) = (Vec::new(), 0, [0.0; 2], [0.0; 2]);
    let mut open = false;
    for &v in verbs {
        match v {
            0 => {
                if open && pen != start {
                    out.push(Seg::Line(pen, start));
                }
                start = points[i];
                pen = start;
                i += 1;
                open = false;
            }
            1 => {
                out.push(Seg::Line(pen, points[i]));
                pen = points[i];
                i += 1;
                open = true;
            }
            2 => {
                let (c, e) = (points[i], points[i + 1]);
                let c1 = [
                    pen[0] + 2.0 / 3.0 * (c[0] - pen[0]),
                    pen[1] + 2.0 / 3.0 * (c[1] - pen[1]),
                ];
                let c2 = [
                    e[0] + 2.0 / 3.0 * (c[0] - e[0]),
                    e[1] + 2.0 / 3.0 * (c[1] - e[1]),
                ];
                out.push(Seg::Cubic([pen, c1, c2, e]));
                pen = e;
                i += 2;
                open = true;
            }
            3 => {
                out.push(Seg::Cubic([pen, points[i], points[i + 1], points[i + 2]]));
                pen = points[i + 2];
                i += 3;
                open = true;
            }
            _ => {
                if pen != start {
                    out.push(Seg::Line(pen, start));
                }
                pen = start;
                open = false;
            }
        }
    }
    if open && pen != start {
        out.push(Seg::Line(pen, start));
    }
    out
}

/// A glyph-like outline in layout space (y down, baseline at 40): an
/// underline bar, a quadratic bowl and a cubic stem, the last left open.
fn glyphish() -> (Vec<u8>, Vec<Point>) {
    let verbs = vec![0, 1, 1, 1, 4, 0, 2, 2, 1, 4, 0, 3, 1];
    let points = vec![
        // The bar: 0..120 wide, 3 px tall, just under the baseline.
        [0.0, 42.0],
        [120.0, 42.0],
        [120.0, 45.0],
        [0.0, 45.0],
        // The bowl.
        [10.0, 30.0],
        [25.0, 10.0],
        [40.0, 30.0],
        [25.0, 50.0],
        [10.0, 30.0],
        [10.0, 30.0],
        // The stem, left open.
        [60.0, 40.0],
        [65.0, 10.0],
        [90.0, 5.0],
        [95.0, 40.0],
        [60.0, 40.0],
    ];
    (verbs, points)
}

#[test]
fn on_a_straight_path_lines_split_once_and_every_vertex_is_the_rigid_image() {
    // A straight open subpath from (50, 60) toward (3, 4)/5.
    let path = shape_of(
        &[(&[corner(50.0, 60.0), corner(350.0, 460.0)], false)],
        "nonzero",
    );
    let (start, baseline, shift) = (12.5, 40.0, 3.0);
    let rigid = |p: Point| {
        let s = start + p[0];
        let rise = baseline - p[1] + shift;
        [50.0 + 0.6 * s + 0.8 * rise, 60.0 + 0.8 * s - 0.6 * rise]
    };
    // A line-only outline: each line becomes exactly two, through the image
    // of its midpoint; the close is a line back first.
    let bar_verbs = [0u8, 1, 1, 1, 4];
    let bar = [[0.0, 42.0], [120.0, 42.0], [120.0, 45.0], [0.0, 45.0]];
    let got = map_raw(
        &bar_verbs,
        &bar,
        &along(&path, start, baseline, shift),
        0.05,
    )
    .unwrap();
    assert_eq!(got.verbs, vec![0, 1, 1, 1, 1, 1, 1, 1, 1, 4]);
    let mid = |a: Point, b: Point| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let mut want = vec![rigid(bar[0])];
    for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
        want.push(rigid(mid(bar[a], bar[b])));
        want.push(rigid(bar[b]));
    }
    assert_eq!(got.points.len(), want.len());
    for (g, w) in got.points.iter().zip(&want) {
        assert!(gap(*g, *w) <= 1e-9, "{g:?} against {w:?}");
    }

    // Curves: every vertex lies on the moved curve, and the moved curve lies
    // within the tolerance of the output.
    let (verbs, points) = glyphish();
    let tolerance = 0.05;
    let got = map_raw(
        &verbs,
        &points,
        &along(&path, start, baseline, shift),
        tolerance,
    )
    .unwrap();
    let source = segments(&verbs, &points);
    let dense: Vec<Vec<Point>> = source
        .iter()
        .map(|&s| {
            (0..=4096)
                .map(|k| rigid(at(s, k as f64 / 4096.0)))
                .collect()
        })
        .collect();
    for v in &got.points {
        let d = to_polylines(*v, &dense);
        assert!(d <= 1e-6, "vertex {v:?} is {d} px off the moved outline");
    }
    let lines = polylines(&got);
    for sample in dense.iter().flatten() {
        let d = to_polylines(*sample, &lines);
        assert!(
            d <= tolerance + 1e-9,
            "sample {sample:?} is {d} px off the output"
        );
    }
}

#[test]
fn on_a_circle_the_output_follows_the_polar_oracle_within_the_bounds() {
    let (centre, r) = ([300.0, 300.0], 100.0);
    let circle = shape_of(&[(&circle_nodes(centre, r, 64), true)], "nonzero");
    let measured = length(&circle, 0).unwrap();
    let (verbs, points) = glyphish();
    let source = segments(&verbs, &points);
    let tolerance = 0.05;
    // The last start runs the outline past the end of the lap: it wraps.
    for (start, baseline, shift) in [(0.0, 40.0, 0.0), (75.25, 40.0, 4.0), (560.0, 30.0, -2.5)] {
        let polar = |p: Point| {
            let rho = r + baseline - p[1] + shift;
            let phi = (start + p[0]) / r;
            [centre[0] + rho * phi.cos(), centre[1] + rho * phi.sin()]
        };
        let got = map_raw(
            &verbs,
            &points,
            &along(&circle, start, baseline, shift),
            tolerance,
        )
        .unwrap();
        // The bounds of `path_along`'s module doc at this outline's reach:
        // the frame's point within τ; the distance drifting by τ/(3r) of
        // itself (the farthest x plus a lap, since a closed path wraps) and
        // scaled to the outline's largest radius; and the tangent's error
        // turning a point `h` off the path by at most h·acos(1 − τ/r).
        let reach = start % measured + 120.0 + measured;
        // The control points bound the outline, so they bound its height.
        let h_max = points
            .iter()
            .map(|p| (baseline - p[1] + shift).abs())
            .fold(0.0, f64::max);
        let rho_max = r + h_max;
        let drift = reach * TAU_PX / (3.0 * r) * 1.01 * rho_max / r;
        let turn = (1.0 - TAU_PX / r).acos() + drift / r;
        let bound = TAU_PX + drift + h_max * turn + 1e-6;
        let dense: Vec<Vec<Point>> = source
            .iter()
            .map(|&s| {
                (0..=4096)
                    .map(|k| polar(at(s, k as f64 / 4096.0)))
                    .collect()
            })
            .collect();
        for v in &got.points {
            let d = to_polylines(*v, &dense);
            assert!(
                d <= bound,
                "start {start}: vertex {v:?} is {d} px off (bound {bound})"
            );
        }
        let lines = polylines(&got);
        for sample in dense.iter().flatten() {
            let d = to_polylines(*sample, &lines);
            assert!(
                d <= tolerance + bound,
                "start {start}: sample {sample:?} is {d} px off the output"
            );
        }
    }
}

#[test]
fn refusals_name_the_reason() {
    let path = shape_of(
        &[(&[corner(0.0, 0.0), corner(100.0, 0.0)], false)],
        "nonzero",
    );
    let map = along(&path, 0.0, 0.0, 0.0);
    let line = [[0.0, 0.0], [10.0, 0.0]];
    let refused = |verbs: &[u8], points: &[Point], map: &Value, tol: f64| match map_raw(
        verbs, points, map, tol,
    ) {
        Ok(_) => panic!("{verbs:?} {map} {tol} was accepted"),
        Err(e) => e,
    };
    assert!(refused(&[0, 7], &line, &map, 0.1).contains("unknown artwork path command"));
    assert!(refused(&[0, 1, 1], &line, &map, 0.1).contains("point count mismatch"));
    assert!(refused(&[0], &line, &map, 0.1).contains("unused points"));
    let warp = json!({"kind": "warp", "shape": path, "subpath": 0});
    assert!(refused(&[0, 1], &line, &warp, 0.1).contains("kind"));
    for tol in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(refused(&[0, 1], &line, &map, tol).contains("tolerance"));
    }
    let past = json!({"kind": "along", "shape": path, "subpath": 1});
    assert!(refused(&[0, 1], &line, &past, 0.1).contains("no subpath 1"));
    let no_shape = json!({"kind": "along", "subpath": 0});
    assert!(refused(&[0, 1], &line, &no_shape, 0.1).contains("shape"));
    let bad_number = json!({"kind": "along", "shape": path, "subpath": 0, "start": "x"});
    assert!(refused(&[0, 1], &line, &bad_number, 0.1).contains("start"));
    assert!(
        refused(&[0, 1], &[[0.0, 0.0], [f64::INFINITY, 0.0]], &map, 0.1)
            .contains("finite geometry bounds")
    );

    // A tiny tolerance is raised to the floor, not refused; an empty outline
    // maps to nothing.
    assert!(map_raw(&[0, 1], &line, &map, 1e-12).is_ok());
    let empty = map_raw(&[], &[], &map, 0.1).unwrap();
    assert!(empty.verbs.is_empty() && empty.points.is_empty());
    // NULL map JSON.
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_outline_map(ptr::null(), 0, ptr::null(), 0, ptr::null(), 0.1, &mut err) };
    assert!(out.is_null() && take_err_string(err).contains("NULL"));
}
