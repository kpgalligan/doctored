//! `dr_path_subpath_at`, the subpath whose fill lies under a point (Path
//! Selection's press), checked on a grid of points against an oracle written
//! here: each subpath's outline flattened by `dr_path_flatten` at 1/4096 px,
//! closed (a fill closes an open subpath), its winding around the point by
//! SUMMED SIGNED ANGLES — not the kernel's ray-crossing count — and among the
//! enclosing subpaths the one whose control box, read from the shape JSON, is
//! smallest, a tie going to the later one.

use std::f64::consts::TAU;
use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_path::dr_path_subpath_at;
use serde_json::{json, Value};

use crate::path_shape::{corner, flatten, shape_of, to_polyline, Node};

/// How far from every outline a grid point must lie to be judged: well past
/// the kernel's own 1/64 px flatness and the oracle's 1/4096.
const MARGIN: f64 = 0.05;

fn subpath_at(shape: &Value, at: [f64; 2]) -> i64 {
    let text = CString::new(shape.to_string()).unwrap();
    unsafe { dr_path_subpath_at(text.as_ptr(), at[0], at[1]) }
}

/// The winding of the closed polygon `points` around `p` by summed signed
/// angles: each edge turns the bearing from `p` by the angle between its two
/// ends, and a whole number of turns is left once the polygon closes.
fn angle_winding(points: &[[f64; 2]], p: [f64; 2]) -> i32 {
    let n = points.len();
    let mut total = 0.0;
    for k in 0..n {
        let (a, b) = (points[k], points[(k + 1) % n]);
        let (ax, ay, bx, by) = (a[0] - p[0], a[1] - p[1], b[0] - p[0], b[1] - p[1]);
        total += (ax * by - ay * bx).atan2(ax * bx + ay * by);
    }
    (total / TAU).round() as i32
}

/// The control box area of subpath `index` of `shape`, from its JSON.
fn box_area(shape: &Value, index: usize) -> f64 {
    let pair = |v: &Value| [v[0].as_f64().unwrap(), v[1].as_f64().unwrap()];
    let mut low = [f64::MAX; 2];
    let mut high = [f64::MIN; 2];
    for anchor in shape["subpaths"][index]["anchors"].as_array().unwrap() {
        let point = [anchor["x"].as_f64().unwrap(), anchor["y"].as_f64().unwrap()];
        for q in [point, pair(&anchor["in"]), pair(&anchor["out"])] {
            for axis in 0..2 {
                low[axis] = low[axis].min(q[axis]);
                high[axis] = high[axis].max(q[axis]);
            }
        }
    }
    (high[0] - low[0]) * (high[1] - low[1])
}

/// Every grid point over `window` farther than `MARGIN` from each outline
/// gets the oracle's answer; returns how many points were judged inside a
/// subpath and how many outside, so a case can require both.
fn check_grid(name: &str, shape: &Value, window: [f64; 4]) -> (usize, usize) {
    let polylines = flatten(shape, 1.0 / 4096.0).expect("the shape flattens");
    let count = polylines.len();
    let areas: Vec<f64> = (0..count).map(|i| box_area(shape, i)).collect();
    let (mut inside, mut outside) = (0, 0);
    // An irrational-looking offset keeps the grid off every anchor and edge.
    let mut y = window[1] + 0.137;
    while y < window[3] {
        let mut x = window[0] + 0.291;
        while x < window[2] {
            let p = [x, y];
            x += 0.75;
            if polylines
                .iter()
                .any(|(points, _)| to_polyline(p, points, true) <= MARGIN)
            {
                continue;
            }
            let mut expected: Option<(f64, usize)> = None;
            for (index, (points, _)) in polylines.iter().enumerate() {
                if angle_winding(points, p) != 0
                    && expected.is_none_or(|(smallest, _)| areas[index] <= smallest)
                {
                    expected = Some((areas[index], index));
                }
            }
            let want = expected.map_or(-1, |(_, index)| index as i64);
            assert_eq!(
                subpath_at(shape, p),
                want,
                "{name}: the subpath under {p:?}"
            );
            if want >= 0 {
                inside += 1;
            } else {
                outside += 1;
            }
        }
        y += 0.75;
    }
    (inside, outside)
}

/// A curved closed blob and a square inside it.
fn blob_and_hole() -> (Vec<Node>, Vec<Node>) {
    let blob = vec![
        Node {
            p: [60.25, 30.5],
            i: [30.25, 30.5],
            o: [90.25, 30.5],
        },
        Node {
            p: [130.75, 80.25],
            i: [130.75, 50.25],
            o: [130.75, 110.25],
        },
        Node {
            p: [60.5, 130.125],
            i: [90.5, 130.125],
            o: [30.5, 130.125],
        },
        corner(20.5, 80.75),
    ];
    let hole = vec![
        corner(50.0, 60.0),
        corner(90.0, 60.0),
        corner(90.0, 100.0),
        corner(50.0, 100.0),
    ];
    (blob, hole)
}

#[test]
fn the_innermost_enclosing_subpath_answers_in_either_drawing_order() {
    let (blob, hole) = blob_and_hole();
    for rule in ["nonzero", "evenodd"] {
        let blob_first = shape_of(&[(&blob, true), (&hole, true)], rule);
        let hole_first = shape_of(&[(&hole, true), (&blob, true)], rule);
        for (name, shape) in [("blob first", blob_first), ("hole first", hole_first)] {
            let (inside, outside) = check_grid(name, &shape, [0.0, 0.0, 150.0, 150.0]);
            assert!(
                inside > 1000 && outside > 1000,
                "{name} {rule}: {inside} in, {outside} out"
            );
        }
    }
    // The hole answers inside it in both orders, the blob around it.
    let blob_first = shape_of(&[(&blob, true), (&hole, true)], "evenodd");
    let hole_first = shape_of(&[(&hole, true), (&blob, true)], "evenodd");
    assert_eq!(subpath_at(&blob_first, [70.0, 80.0]), 1);
    assert_eq!(subpath_at(&hole_first, [70.0, 80.0]), 0);
    assert_eq!(subpath_at(&blob_first, [40.0, 80.0]), 0);
    assert_eq!(subpath_at(&hole_first, [40.0, 80.0]), 1);
}

#[test]
fn a_figure_of_eight_an_open_v_and_two_rectangles_answer_by_their_own_outlines() {
    // Both lobes of a figure-of-eight wind once, opposite ways: both are its fill.
    let eight = [
        corner(10.0, 10.0),
        corner(50.0, 50.0),
        corner(50.0, 10.0),
        corner(10.0, 50.0),
    ];
    let eight = shape_of(&[(&eight, true)], "nonzero");
    let (inside, outside) = check_grid("figure of eight", &eight, [0.0, 0.0, 60.0, 60.0]);
    assert!(
        inside > 500 && outside > 500,
        "figure of eight: {inside} in, {outside} out"
    );
    assert_eq!(subpath_at(&eight, [20.0, 30.0]), 0);
    assert_eq!(subpath_at(&eight, [40.0, 30.0]), 0);
    assert_eq!(subpath_at(&eight, [30.0, 20.0]), -1);

    // An open V is closed by the straight line its fill adds: a triangle.
    let v = [corner(10.0, 10.0), corner(30.0, 50.0), corner(50.0, 10.0)];
    let v = shape_of(&[(&v, false)], "nonzero");
    let (inside, outside) = check_grid("open v", &v, [0.0, 0.0, 60.0, 60.0]);
    assert!(
        inside > 500 && outside > 500,
        "open v: {inside} in, {outside} out"
    );
    assert_eq!(subpath_at(&v, [30.0, 20.0]), 0);

    let left = [
        corner(5.0, 5.0),
        corner(25.0, 5.0),
        corner(25.0, 30.0),
        corner(5.0, 30.0),
    ];
    let right = [
        corner(40.0, 8.0),
        corner(70.0, 8.0),
        corner(70.0, 28.0),
        corner(40.0, 28.0),
    ];
    let two = shape_of(&[(&left, true), (&right, false)], "nonzero");
    let (inside, outside) = check_grid("two rectangles", &two, [0.0, 0.0, 80.0, 40.0]);
    assert!(
        inside > 500 && outside > 500,
        "two rectangles: {inside} in, {outside} out"
    );
    assert_eq!(subpath_at(&two, [10.0, 10.0]), 0);
    assert_eq!(subpath_at(&two, [50.0, 20.0]), 1);
    assert_eq!(subpath_at(&two, [32.0, 20.0]), -1);
}

#[test]
fn a_miss_a_non_finite_point_and_malformed_json_answer_minus_one() {
    let square = [
        corner(0.0, 0.0),
        corner(10.0, 0.0),
        corner(10.0, 10.0),
        corner(0.0, 10.0),
    ];
    let square = shape_of(&[(&square, true)], "nonzero");
    assert_eq!(subpath_at(&square, [5.0, 5.0]), 0);
    assert_eq!(subpath_at(&square, [15.0, 5.0]), -1);
    assert_eq!(subpath_at(&square, [f64::NAN, 5.0]), -1);
    assert_eq!(subpath_at(&square, [5.0, f64::INFINITY]), -1);
    assert_eq!(subpath_at(&json!({"subpaths": []}), [5.0, 5.0]), -1);
    let malformed = CString::new("{\"subpaths\": [").unwrap();
    assert_eq!(
        unsafe { dr_path_subpath_at(malformed.as_ptr(), 5.0, 5.0) },
        -1
    );
    let empty_subpath = json!({"subpaths": [{"closed": true, "anchors": []}]});
    assert_eq!(subpath_at(&empty_subpath, [5.0, 5.0]), -1);
    assert_eq!(unsafe { dr_path_subpath_at(ptr::null(), 5.0, 5.0) }, -1);
}
