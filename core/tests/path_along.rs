//! `dr_path_length` and `dr_path_frames`, distance along one subpath (text
//! on a path), against oracles written here: a rectangle's exact perimeter
//! and frames, a circle of 64 cubic arcs against 2πr and the analytic point
//! and tangent within the bounds `path_along`'s module doc states, and an
//! asymmetric cubic's length against Gauss–Legendre quadrature of |B′(t)|.
//! Nothing is imported from the kernels.

use std::f64::consts::TAU;
use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_path_along::{dr_path_frames, dr_path_length};
use serde_json::Value;

use crate::common::take_err_string;
use crate::path_shape::{corner, shape_of, Node};

/// The flatness the kernel measures at (its documented 1/64 px).
pub const TAU_PX: f64 = 1.0 / 64.0;

pub type Frame = ([f64; 2], [f64; 2]);

fn text(shape: &Value) -> CString {
    CString::new(shape.to_string()).unwrap()
}

pub fn length(shape: &Value, subpath: u32) -> Result<f64, String> {
    let json = text(shape);
    let mut err: *mut c_char = ptr::null_mut();
    let value = unsafe { dr_path_length(json.as_ptr(), subpath, &mut err) };
    if value < 0.0 {
        return Err(take_err_string(err));
    }
    assert!(err.is_null());
    Ok(value)
}

/// The frames at `distances`, `None` where the kernel wrote four NaN.
pub fn frames(
    shape: &Value,
    subpath: u32,
    reversed: bool,
    distances: &[f64],
) -> Result<Vec<Option<Frame>>, String> {
    let json = text(shape);
    let mut out = vec![0.0; distances.len() * 4];
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_path_frames(
            json.as_ptr(),
            subpath,
            reversed,
            distances.as_ptr(),
            distances.len(),
            out.as_mut_ptr(),
            out.len(),
            &mut err,
        )
    };
    if !ok {
        return Err(take_err_string(err));
    }
    Ok(out
        .chunks(4)
        .map(|f| {
            if f.iter().all(|v| v.is_nan()) {
                None
            } else {
                assert!(f.iter().all(|v| v.is_finite()), "a partial NaN frame {f:?}");
                Some(([f[0], f[1]], [f[2], f[3]]))
            }
        })
        .collect())
}

pub fn frame(shape: &Value, reversed: bool, d: f64) -> Option<Frame> {
    frames(shape, 0, reversed, &[d]).expect("frames")[0]
}

fn gap(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn close(a: Frame, b: Frame, eps: f64) -> bool {
    gap(a.0, b.0) <= eps && gap(a.1, b.1) <= eps
}

/// A circle of `n` cubic arcs about `centre`, starting at angle 0 and running
/// toward increasing angle (clockwise on a y-down canvas): each arc's handles
/// lie along the tangent at the classic `(4/3)·tan(Δ/4)·r`.
pub fn circle_nodes(centre: [f64; 2], r: f64, n: usize) -> Vec<Node> {
    let step = TAU / n as f64;
    let h = 4.0 / 3.0 * (step / 4.0).tan() * r;
    (0..n)
        .map(|k| {
            let a = step * k as f64;
            let p = [centre[0] + r * a.cos(), centre[1] + r * a.sin()];
            let t = [-a.sin(), a.cos()];
            Node {
                p,
                i: [p[0] - h * t[0], p[1] - h * t[1]],
                o: [p[0] + h * t[0], p[1] + h * t[1]],
            }
        })
        .collect()
}

fn rectangle() -> Vec<Node> {
    vec![
        corner(10.0, 20.0),
        corner(110.0, 20.0),
        corner(110.0, 70.0),
        corner(10.0, 70.0),
    ]
}

#[test]
fn a_rectangle_measures_and_frames_exactly_and_a_closed_one_wraps() {
    let closed = shape_of(&[(&rectangle(), true)], "nonzero");
    assert_eq!(length(&closed, 0).unwrap(), 300.0);
    let expected: [(f64, Frame); 7] = [
        (0.0, ([10.0, 20.0], [1.0, 0.0])),
        (50.0, ([60.0, 20.0], [1.0, 0.0])),
        // A distance landing on a vertex takes the chord leaving it.
        (100.0, ([110.0, 20.0], [0.0, 1.0])),
        (125.0, ([110.0, 45.0], [0.0, 1.0])),
        (200.0, ([60.0, 70.0], [-1.0, 0.0])),
        (275.0, ([10.0, 45.0], [0.0, -1.0])),
        (299.0, ([10.0, 21.0], [0.0, -1.0])),
    ];
    for (d, want) in expected {
        let got = frame(&closed, false, d).expect("a closed frame");
        assert!(close(got, want, 1e-12), "closed {d}: {got:?} != {want:?}");
        // Wrapping: a lap on, a lap back, and the same distance negative.
        for wrapped in [d + 300.0, d - 300.0, d + 900.0] {
            let other = frame(&closed, false, wrapped).expect("a wrapped frame");
            assert!(close(other, want, 1e-9), "closed {wrapped}: {other:?}");
        }
    }
    let back = frame(&closed, false, -25.0).expect("a negative distance wraps");
    assert!(close(back, ([10.0, 45.0], [0.0, -1.0]), 1e-9), "{back:?}");

    // Open, the closing side is not part of it: three sides, 250 px.
    let open = shape_of(&[(&rectangle(), false)], "nonzero");
    assert_eq!(length(&open, 0).unwrap(), 250.0);
    let ends = frames(&open, 0, false, &[-0.001, 0.0, 250.0, 250.001, f64::NAN]).unwrap();
    assert_eq!(ends[0], None);
    assert!(close(ends[1].unwrap(), ([10.0, 20.0], [1.0, 0.0]), 1e-12));
    assert!(close(ends[2].unwrap(), ([10.0, 70.0], [-1.0, 0.0]), 1e-9));
    assert_eq!(ends[3], None);
    assert_eq!(ends[4], None);
    assert_eq!(frame(&closed, false, f64::INFINITY), None);
}

#[test]
fn reversed_runs_the_other_way_from_the_other_end() {
    let open = shape_of(&[(&rectangle(), false)], "nonzero");
    let closed = shape_of(&[(&rectangle(), true)], "nonzero");
    for (shape, total) in [(&open, 250.0), (&closed, 300.0)] {
        for d in [3.25, 47.5, 140.0, 199.75, 241.0] {
            let forward = frame(shape, false, total - d).expect("forward");
            let reversed = frame(shape, true, d).expect("reversed");
            assert!(
                gap(forward.0, reversed.0) <= 1e-9,
                "{d}: {forward:?} {reversed:?}"
            );
            let negated = [-forward.1[0], -forward.1[1]];
            assert!(
                gap(negated, reversed.1) <= 1e-12,
                "{d}: {forward:?} {reversed:?}"
            );
        }
    }
    // An open subpath reversed starts on its last anchor; a closed one still
    // starts on its first and runs the other way round.
    let start = frame(&open, true, 0.0).unwrap();
    assert!(close(start, ([10.0, 70.0], [1.0, 0.0]), 1e-12), "{start:?}");
    let start = frame(&closed, true, 0.0).unwrap();
    assert!(close(start, ([10.0, 20.0], [0.0, 1.0]), 1e-12), "{start:?}");
}

#[test]
fn a_circle_of_sixty_four_arcs_measures_two_pi_r_within_the_stated_bounds() {
    let (centre, r) = ([200.0, 150.0], 100.0);
    let circle = shape_of(&[(&circle_nodes(centre, r, 64), true)], "nonzero");
    let measured = length(&circle, 0).unwrap();
    let truth = TAU * r;
    // A chord is never longer than its arc; the shortfall is at most the
    // factor τ/(3r) (to leading order, so one percent of slack).
    let shortfall = truth * TAU_PX / (3.0 * r) * 1.01 + 1e-6;
    assert!(
        measured <= truth + 1e-6 && truth - measured <= shortfall,
        "{measured} against {truth} (shortfall bound {shortfall})"
    );
    let turn = (1.0 - TAU_PX / r).acos();
    let distances: Vec<f64> = (0..97).map(|k| k as f64 * measured / 96.0 + 0.37).collect();
    let got = frames(&circle, 0, false, &distances).unwrap();
    for (d, f) in distances.iter().zip(got) {
        let (point, tangent) = f.expect("a closed frame");
        // The distance along the polyline drifts from the true arc length
        // by at most the length factor of itself.
        let drift = d * TAU_PX / (3.0 * r) * 1.01;
        let phi = (d % measured) / r;
        let want = [centre[0] + r * phi.cos(), centre[1] + r * phi.sin()];
        assert!(
            gap(point, want) <= TAU_PX + drift + 1e-6,
            "point at {d}: {point:?} against {want:?}"
        );
        assert!(
            (tangent[0].hypot(tangent[1]) - 1.0).abs() <= 1e-12,
            "a unit tangent at {d}: {tangent:?}"
        );
        let analytic = [-phi.sin(), phi.cos()];
        let angle = (analytic[0] * tangent[1] - analytic[1] * tangent[0])
            .atan2(analytic[0] * tangent[0] + analytic[1] * tangent[1])
            .abs();
        assert!(
            angle <= turn + drift / r + 1e-9,
            "tangent at {d}: {tangent:?} against {analytic:?} ({angle} rad)"
        );
    }
    // Reversed, the circle runs the other way from the same start.
    for d in [5.0, 100.5, 333.3] {
        let forward = frame(&circle, false, measured - d).unwrap();
        let reversed = frame(&circle, true, d).unwrap();
        assert!(gap(forward.0, reversed.0) <= 1e-9, "{d}");
        assert!(
            gap([-forward.1[0], -forward.1[1]], reversed.1) <= 1e-9,
            "{d}"
        );
    }
}

/// One cubic, its derivative and second derivative.
fn cubic_d1(p: &[[f64; 2]; 4], t: f64) -> [f64; 2] {
    let s = 1.0 - t;
    [0, 1].map(|a| {
        3.0 * s * s * (p[1][a] - p[0][a])
            + 6.0 * s * t * (p[2][a] - p[1][a])
            + 3.0 * t * t * (p[3][a] - p[2][a])
    })
}

fn cubic_d2(p: &[[f64; 2]; 4], t: f64) -> [f64; 2] {
    [0, 1].map(|a| {
        6.0 * (1.0 - t) * (p[2][a] - 2.0 * p[1][a] + p[0][a])
            + 6.0 * t * (p[3][a] - 2.0 * p[2][a] + p[1][a])
    })
}

/// ∫₀¹ |B′(t)| dt by five-point Gauss–Legendre on 512 panels.
fn quadrature_length(p: &[[f64; 2]; 4]) -> f64 {
    let a = (5.0 - 2.0 * (10.0f64 / 7.0).sqrt()).sqrt() / 3.0;
    let b = (5.0 + 2.0 * (10.0f64 / 7.0).sqrt()).sqrt() / 3.0;
    let wa = (322.0 + 13.0 * 70.0f64.sqrt()) / 900.0;
    let wb = (322.0 - 13.0 * 70.0f64.sqrt()) / 900.0;
    let nodes = [(0.0, 128.0 / 225.0), (a, wa), (-a, wa), (b, wb), (-b, wb)];
    let panels = 512;
    let h = 1.0 / panels as f64;
    (0..panels)
        .map(|k| {
            let mid = (k as f64 + 0.5) * h;
            nodes
                .iter()
                .map(|&(x, w)| {
                    let d = cubic_d1(p, mid + x * h / 2.0);
                    w * d[0].hypot(d[1])
                })
                .sum::<f64>()
                * h
                / 2.0
        })
        .sum()
}

#[test]
fn an_asymmetric_cubic_measures_its_quadrature_length_within_the_bound() {
    let p = [[20.0, 30.0], [80.0, 150.0], [170.0, -40.0], [260.0, 60.0]];
    let nodes = [
        Node {
            p: p[0],
            i: p[0],
            o: p[1],
        },
        Node {
            p: p[3],
            i: p[2],
            o: p[3],
        },
    ];
    let curve = shape_of(&[(&nodes, false)], "nonzero");
    let measured = length(&curve, 0).unwrap();
    let truth = quadrature_length(&p);
    // The smallest radius of curvature, |B′|³ / |B′ × B″|, over a fine grid.
    let r_min = (0..=20_000)
        .map(|k| {
            let t = k as f64 / 20_000.0;
            let (d1, d2) = (cubic_d1(&p, t), cubic_d2(&p, t));
            let cross = (d1[0] * d2[1] - d1[1] * d2[0]).abs();
            d1[0].hypot(d1[1]).powi(3) / cross.max(1e-300)
        })
        .fold(f64::MAX, f64::min);
    let shortfall = truth * TAU_PX / (3.0 * r_min) * 1.01 + 1e-9;
    assert!(
        measured <= truth + 1e-9 && truth - measured <= shortfall,
        "{measured} against {truth} (r_min {r_min}, bound {shortfall})"
    );
    // The ends are the anchors; past them there is no frame.
    let ends = frames(&curve, 0, false, &[0.0, measured, -1e-6, measured + 1e-6]).unwrap();
    assert!(gap(ends[0].unwrap().0, p[0]) <= 1e-12);
    assert!(gap(ends[1].unwrap().0, p[3]) <= 1e-9);
    assert_eq!((ends[2], ends[3]), (None, None));
}

#[test]
fn refusals_name_the_reason_and_leave_the_buffer_alone() {
    let square = shape_of(&[(&rectangle(), true)], "nonzero");
    assert!(length(&square, 1).unwrap_err().contains("no subpath 1"));
    let dot = shape_of(&[(&[corner(5.0, 5.0)], false)], "nonzero");
    assert!(length(&dot, 0).unwrap_err().contains("no length"));
    let stacked = shape_of(&[(&[corner(5.0, 5.0), corner(5.0, 5.0)], true)], "nonzero");
    assert!(length(&stacked, 0).unwrap_err().contains("no length"));
    assert!(frames(&dot, 0, false, &[1.0])
        .unwrap_err()
        .contains("no length"));

    let json = text(&square);
    let distances = [1.0, 2.0];
    let sentinel = 4242.0;
    for (count, buffer_len) in [(2usize, 7usize), (2, 9), (65_537, 65_537 * 4)] {
        let mut out = vec![sentinel; buffer_len];
        let wanted: Vec<f64> = (0..count).map(|k| distances[k % 2]).collect();
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            dr_path_frames(
                json.as_ptr(),
                0,
                false,
                wanted.as_ptr(),
                count,
                out.as_mut_ptr(),
                buffer_len,
                &mut err,
            )
        };
        assert!(!ok);
        assert!(take_err_string(err).contains("invalid path frames buffers"));
        assert!(out.iter().all(|&v| v == sentinel));
    }
    // No such subpath: nothing is written either.
    let mut out = [sentinel; 8];
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_path_frames(
            json.as_ptr(),
            3,
            false,
            distances.as_ptr(),
            2,
            out.as_mut_ptr(),
            8,
            &mut err,
        )
    };
    assert!(!ok && take_err_string(err).contains("no subpath 3"));
    assert!(out.iter().all(|&v| v == sentinel));
    // NULL distances with a count is refused; an empty request is fine.
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_path_frames(
            json.as_ptr(),
            0,
            false,
            ptr::null(),
            1,
            out.as_mut_ptr(),
            4,
            &mut err,
        )
    };
    assert!(!ok && !take_err_string(err).is_empty());
    let ok = unsafe {
        dr_path_frames(
            json.as_ptr(),
            0,
            false,
            ptr::null(),
            0,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
        )
    };
    assert!(ok);
    let malformed = CString::new("{\"subpaths\": [").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    assert_eq!(
        unsafe { dr_path_length(malformed.as_ptr(), 0, &mut err) },
        -1.0
    );
    assert!(take_err_string(err).contains("malformed"));
}
