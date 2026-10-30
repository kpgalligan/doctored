//! The SHAPE operations of a path — coverage, flattening, hit testing,
//! inserting and converting an anchor — through the `dr_path_*` exports that
//! take one shape as JSON and no document.
//!
//! Each is checked against an oracle written here: a rectangle's exact
//! per-pixel overlap, the Green's-theorem area of a four-arc Bézier circle,
//! the winding of nested squares, the Bernstein form of a cubic for the
//! flatten error and for an inserted anchor, and the handle rule of a
//! converted one. The `pub` helpers are shared with `path_trace.rs`.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_path::*;
use doctored_core::job_control::DrJobControl;
use doctored_core::path_geometry::MAX_PATH_ANCHORS;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::path_geometry::{take, LIMIT};

/// What a refused coverage call must leave in its output.
const SENTINEL: u16 = 0x4a4a;

/// One anchor as the tests author it: the point and its two handles.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub p: [f64; 2],
    pub i: [f64; 2],
    pub o: [f64; 2],
}

pub fn corner(x: f64, y: f64) -> Node {
    Node {
        p: [x, y],
        i: [x, y],
        o: [x, y],
    }
}

/// The shape JSON of `subpaths` (each a node list and its closed flag).
pub fn shape_of(subpaths: &[(&[Node], bool)], rule: &str) -> Value {
    let subpaths: Vec<Value> = subpaths
        .iter()
        .map(|(nodes, closed)| {
            let anchors: Vec<Value> = nodes
                .iter()
                .map(|n| json!({"x": n.p[0], "y": n.p[1], "in": n.i, "out": n.o}))
                .collect();
            json!({"closed": closed, "anchors": anchors})
        })
        .collect();
    json!({"fill_rule": rule, "subpaths": subpaths})
}

/// The nodes of one subpath of a shape the core wrote.
fn nodes(shape: &Value, subpath: usize) -> Vec<Node> {
    let pair = |v: &Value| [v[0].as_f64().unwrap(), v[1].as_f64().unwrap()];
    shape["subpaths"][subpath]["anchors"]
        .as_array()
        .expect("anchors")
        .iter()
        .map(|a| Node {
            p: [a["x"].as_f64().unwrap(), a["y"].as_f64().unwrap()],
            i: pair(&a["in"]),
            o: pair(&a["out"]),
        })
        .collect()
}

/// The segments of a subpath of two or more nodes as cubic control points:
/// node k to node k + 1, and the last back to the first when closed.
fn cubics(nodes: &[Node], closed: bool) -> Vec<[[f64; 2]; 4]> {
    let count = if closed { nodes.len() } else { nodes.len() - 1 };
    (0..count)
        .map(|k| {
            let (a, b) = (nodes[k], nodes[(k + 1) % nodes.len()]);
            [a.p, a.o, b.i, b.p]
        })
        .collect()
}

/// A cubic in Bernstein form, and its derivative.
fn bernstein(p: &[[f64; 2]; 4], t: f64) -> [f64; 2] {
    let s = 1.0 - t;
    let b = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
    [0, 1].map(|axis| (0..4).map(|k| b[k] * p[k][axis]).sum())
}

fn bernstein_derivative(p: &[[f64; 2]; 4], t: f64) -> [f64; 2] {
    let s = 1.0 - t;
    let b = [3.0 * s * s, 6.0 * s * t, 3.0 * t * t];
    [0, 1].map(|axis| (0..3).map(|k| b[k] * (p[k + 1][axis] - p[k][axis])).sum())
}

fn gap(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    };
    gap(p, [a[0] + t * dx, a[1] + t * dy])
}

/// The distance from `p` to a polyline, its closing edge included when
/// `closed`.
pub fn to_polyline(p: [f64; 2], points: &[[f64; 2]], closed: bool) -> f64 {
    let mut best = points
        .windows(2)
        .map(|pair| to_segment(p, pair[0], pair[1]))
        .fold(gap(p, points[0]), f64::min);
    if closed {
        best = best.min(to_segment(p, points[points.len() - 1], points[0]));
    }
    best
}

/// `dr_path_coverage` over the `size` window at `origin`: the plane, or the
/// refusal's sentence once the output is seen untouched.
pub fn coverage(
    shape: &Value,
    origin: (i32, i32),
    size: (u32, u32),
    anti_alias: bool,
    clip: Option<&[u16]>,
) -> Result<Vec<u16>, String> {
    coverage_under(shape, origin, size, anti_alias, clip, LIMIT, ptr::null())
}

fn coverage_under(
    shape: &Value,
    origin: (i32, i32),
    size: (u32, u32),
    anti_alias: bool,
    clip: Option<&[u16]>,
    limit: u64,
    control: *const DrJobControl,
) -> Result<Vec<u16>, String> {
    let text = CString::new(shape.to_string()).unwrap();
    let mut out = vec![SENTINEL; (size.0 * size.1) as usize];
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_path_coverage(
            text.as_ptr(),
            origin.0,
            origin.1,
            size.0,
            size.1,
            anti_alias,
            clip.map_or(ptr::null(), <[u16]>::as_ptr),
            clip.map_or(0, <[u16]>::len),
            out.as_mut_ptr(),
            out.len(),
            limit,
            0,
            control,
            &mut error,
        )
    };
    if ok {
        assert!(error.is_null());
        return Ok(out);
    }
    assert!(
        out.iter().all(|v| *v == SENTINEL),
        "a refused coverage wrote to its output"
    );
    Err(take_err_string(error))
}

/// One flattened subpath: its points and its closed flag.
pub type Polyline = (Vec<[f64; 2]>, bool);

/// `dr_path_flatten`: each subpath's polyline.
pub fn flatten(shape: &Value, tolerance: f64) -> Result<Vec<Polyline>, String> {
    let text = CString::new(shape.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_path_flatten(text.as_ptr(), tolerance, &mut error) };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    let parsed: Value = serde_json::from_str(&take(out)).expect("polyline JSON");
    Ok(parsed
        .as_array()
        .expect("an array of polylines")
        .iter()
        .map(|entry| {
            let points = entry["points"]
                .as_array()
                .expect("points")
                .iter()
                .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
                .collect();
            (points, entry["closed"].as_bool().expect("closed"))
        })
        .collect())
}

fn hit(shape: &Value, at: [f64; 2], tolerance: f64) -> Value {
    hit_offering(shape, at, tolerance, true)
}

/// `dr_path_hit` with handles offered or not.
fn hit_offering(shape: &Value, at: [f64; 2], tolerance: f64, handles: bool) -> Value {
    let text = CString::new(shape.to_string()).unwrap();
    let out = unsafe { dr_path_hit(text.as_ptr(), at[0], at[1], tolerance, handles) };
    serde_json::from_str(&take(out)).expect("hit JSON")
}

fn insert(shape: &Value, subpath: usize, segment: usize, t: f64) -> Result<Value, String> {
    let text = CString::new(shape.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_path_insert_anchor(text.as_ptr(), subpath, segment, t, &mut error) };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    Ok(serde_json::from_str(&take(out)).expect("shape JSON"))
}

/// The converted shape, `Ok(None)` for NULL with no message (unchanged).
fn convert(
    shape: &Value,
    subpath: usize,
    anchor: usize,
    smooth: bool,
) -> Result<Option<Value>, String> {
    let text = CString::new(shape.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_path_convert_anchor(text.as_ptr(), subpath, anchor, smooth, &mut error) };
    if !out.is_null() {
        return Ok(Some(serde_json::from_str(&take(out)).expect("shape JSON")));
    }
    if error.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(error))
    }
}

/// How much of pixel `p` the interval `[low, high]` covers along one axis.
fn overlap(low: f64, high: f64, p: u32) -> f64 {
    (high.min(f64::from(p) + 1.0) - low.max(f64::from(p))).max(0.0)
}

#[test]
fn a_fractional_rectangle_covers_each_pixel_by_its_exact_overlap() {
    // No pixel's overlap is one half, so the hard edge has no knife edge.
    let (x0, y0, x1, y1) = (2.25, 1.4, 5.75, 4.125);
    let ring = [
        corner(x0, y0),
        corner(x1, y0),
        corner(x1, y1),
        corner(x0, y1),
    ];
    let shape = shape_of(&[(&ring, true)], "nonzero");
    let (w, h) = (8u32, 6u32);
    let exact = |x: u32, y: u32| overlap(x0, x1, x) * overlap(y0, y1, y);
    let soft = coverage(&shape, (0, 0), (w, h), true, None).expect("coverage");
    for y in 0..h {
        for x in 0..w {
            let got = f64::from(soft[(y * w + x) as usize]);
            let want = (exact(x, y) * 65535.0).round();
            assert!((got - want).abs() <= 1.0, "({x}, {y}): {got} for {want}");
        }
    }
    // An open subpath is closed by a straight line for fill: the same plane.
    let open = shape_of(&[(&ring, false)], "nonzero");
    assert_eq!(coverage(&open, (0, 0), (w, h), true, None).unwrap(), soft);

    // A hard edge is that coverage cut at one half.
    let hard = coverage(&shape, (0, 0), (w, h), false, None).expect("hard coverage");
    for y in 0..h {
        for x in 0..w {
            let want = if exact(x, y) >= 0.5 { 65535 } else { 0 };
            assert_eq!(hard[(y * w + x) as usize], want, "({x}, {y})");
        }
    }
    assert!(hard.contains(&65535) && hard.contains(&0));

    // A window is the same plane read from its own origin, off-canvas rows
    // and columns included.
    let (ox, oy, ww, wh) = (2i32, -1i32, 5u32, 4u32);
    let window = coverage(&shape, (ox, oy), (ww, wh), true, None).expect("a window");
    for y in 0..wh {
        for x in 0..ww {
            let (cx, cy) = (x as i32 + ox, y as i32 + oy);
            let want = if cy < 0 {
                0.0
            } else {
                (exact(cx as u32, cy as u32) * 65535.0).round()
            };
            let got = f64::from(window[(y * ww + x) as usize]);
            assert!((got - want).abs() <= 1.0, "window ({x}, {y})");
        }
    }

    // A clip multiplies in, rounded to nearest.
    let clip: Vec<u16> = (0..w * h).map(|i| (i * 2731 % 65536) as u16).collect();
    let clipped = coverage(&shape, (0, 0), (w, h), true, Some(&clip)).expect("clipped");
    for (i, got) in clipped.iter().enumerate() {
        let want = (f64::from(soft[i]) * f64::from(clip[i]) / 65535.0).round();
        assert_eq!(f64::from(*got), want, "sample {i}");
    }
    // An empty shape covers nothing.
    let empty = json!({"subpaths": []});
    assert!(coverage(&empty, (0, 0), (w, h), true, None)
        .unwrap()
        .iter()
        .all(|v| *v == 0));
}

#[test]
fn a_four_arc_bezier_circle_covers_its_greens_theorem_area() {
    // Four cubic arcs, every control point at four decimals so the shape
    // the core stores is the one integrated here.
    let q = |v: f64| (v * 1e4).round() / 1e4;
    let (cx, cy, r) = (16.3, 15.6, 10.25);
    let k = q(r * 0.552_284_749_8);
    let ring = [
        Node {
            p: [cx, cy - r],
            i: [cx - k, cy - r],
            o: [cx + k, cy - r],
        },
        Node {
            p: [cx + r, cy],
            i: [cx + r, cy - k],
            o: [cx + r, cy + k],
        },
        Node {
            p: [cx, cy + r],
            i: [cx + k, cy + r],
            o: [cx - k, cy + r],
        },
        Node {
            p: [cx - r, cy],
            i: [cx - r, cy + k],
            o: [cx - r, cy - k],
        },
    ];
    // Green's theorem: the area is the loop integral of x dy. Each arc's
    // integrand is a quintic in t, which three Gauss–Legendre nodes
    // integrate exactly.
    let half = 0.5 * (3.0f64 / 5.0).sqrt();
    let gauss = [
        (0.5 - half, 5.0 / 18.0),
        (0.5, 8.0 / 18.0),
        (0.5 + half, 5.0 / 18.0),
    ];
    let area: f64 = cubics(&ring, true)
        .iter()
        .map(|arc| {
            gauss
                .iter()
                .map(|(t, weight)| {
                    weight * bernstein(arc, *t)[0] * bernstein_derivative(arc, *t)[1]
                })
                .sum::<f64>()
        })
        .sum::<f64>()
        .abs();
    assert!(
        (area - std::f64::consts::PI * r * r).abs() < 0.2,
        "the oracle is a circle's area: {area}"
    );

    let (w, h) = (32u32, 32u32);
    let shape = shape_of(&[(&ring, true)], "nonzero");
    let plane = coverage(&shape, (0, 0), (w, h), true, None).expect("coverage");
    let covered: f64 = plane.iter().map(|v| f64::from(*v)).sum::<f64>() / 65535.0;
    // Each sample rounds by at most half a code (1024 samples: 0.008 px²),
    // and the rasterizer's chords sit within 1/4096 px of the arcs over a
    // 65 px perimeter (0.016 px²).
    assert!(
        (covered - area).abs() < 0.03,
        "covered {covered}, area {area}"
    );
    // Pixels wholly inside or outside the circle are whole or empty.
    for y in 0..h {
        for x in 0..w {
            let far = |px: u32, py: u32| gap([f64::from(px), f64::from(py)], [cx, cy]);
            let reach = [far(x, y), far(x + 1, y), far(x, y + 1), far(x + 1, y + 1)];
            let got = plane[(y * w + x) as usize];
            if reach.iter().all(|d| *d < r - 0.05) {
                assert_eq!(got, 65535, "({x}, {y}) is inside");
            }
            if reach.iter().all(|d| *d > r + 0.05) && far(x, y) > r + 1.5 {
                assert_eq!(got, 0, "({x}, {y}) is outside");
            }
        }
    }
    // Reversing the winding changes nothing about the fill.
    let reversed: Vec<Node> = ring
        .iter()
        .rev()
        .map(|n| Node {
            p: n.p,
            i: n.o,
            o: n.i,
        })
        .collect();
    let again = coverage(
        &shape_of(&[(&reversed, true)], "nonzero"),
        (0, 0),
        (w, h),
        true,
        None,
    )
    .unwrap();
    for (a, b) in plane.iter().zip(&again) {
        assert!(a.abs_diff(*b) <= 1);
    }
}

#[test]
fn nested_squares_fill_by_the_shapes_rule() {
    let outer = [
        corner(2.0, 2.0),
        corner(14.0, 2.0),
        corner(14.0, 14.0),
        corner(2.0, 14.0),
    ];
    let inner = [
        corner(5.0, 5.0),
        corner(11.0, 5.0),
        corner(11.0, 11.0),
        corner(5.0, 11.0),
    ];
    let mut reversed = inner;
    reversed.reverse();
    let size = (16u32, 16u32);
    let total = |shape: &Value| -> u64 {
        coverage(shape, (0, 0), size, true, None)
            .expect("coverage")
            .iter()
            .map(|v| u64::from(*v))
            .sum()
    };
    // Same direction: the inner square winds twice. Nonzero fills it, even
    // odd makes it a hole (144 − 36 pixels).
    let same = [(&outer[..], true), (&inner[..], true)];
    assert_eq!(total(&shape_of(&same, "nonzero")), 144 * 65535);
    assert_eq!(total(&shape_of(&same, "evenodd")), 108 * 65535);
    // Opposite direction: the windings cancel, a hole under both rules.
    let opposite = [(&outer[..], true), (&reversed[..], true)];
    assert_eq!(total(&shape_of(&opposite, "nonzero")), 108 * 65535);
    assert_eq!(total(&shape_of(&opposite, "evenodd")), 108 * 65535);
    let hole = coverage(&shape_of(&same, "evenodd"), (0, 0), size, true, None).unwrap();
    assert_eq!(hole[8 * 16 + 8], 0, "the centre is the hole");
    assert_eq!(hole[3 * 16 + 3], 65535, "the ring is filled");
}

#[test]
fn coverage_refuses_bad_buffers_admission_and_a_cancelled_job() {
    let ring = [corner(1.0, 1.0), corner(6.0, 1.0), corner(3.0, 6.0)];
    let shape = shape_of(&[(&ring, true)], "nonzero");
    let size = (8u32, 8u32);
    // A clip of another length, and a zero window.
    let short = vec![65535u16; 63];
    let message = coverage(&shape, (0, 0), size, true, Some(&short)).unwrap_err();
    assert!(message.contains("buffers"), "{message}");
    assert!(coverage(&shape, (0, 0), (0, 8), true, None).is_err());
    // A refused shape and malformed JSON name themselves.
    let hollow = json!({"subpaths": [{"anchors": []}]});
    let message = coverage(&hollow, (0, 0), size, true, None).unwrap_err();
    assert!(message.contains("at least one anchor"), "{message}");
    let mut out = [SENTINEL; 64];
    let mut error = ptr::null_mut();
    let garbage = CString::new("{not json").unwrap();
    let call = |text: *const c_char, out: *mut u16, count: usize, error: &mut *mut c_char| unsafe {
        dr_path_coverage(
            text,
            0,
            0,
            8,
            8,
            true,
            ptr::null(),
            0,
            out,
            count,
            LIMIT,
            0,
            ptr::null(),
            error,
        )
    };
    assert!(!call(garbage.as_ptr(), out.as_mut_ptr(), 64, &mut error));
    assert!(take_err_string(error).contains("malformed"));
    let mut error = ptr::null_mut();
    assert!(!call(ptr::null(), out.as_mut_ptr(), 64, &mut error));
    assert!(take_err_string(error).contains("NULL"));
    // An output of the wrong length, or none.
    let text = CString::new(shape.to_string()).unwrap();
    let mut error = ptr::null_mut();
    assert!(!call(text.as_ptr(), out.as_mut_ptr(), 63, &mut error));
    assert!(take_err_string(error).contains("buffers"));
    let mut error = ptr::null_mut();
    assert!(!call(text.as_ptr(), ptr::null_mut(), 64, &mut error));
    assert!(take_err_string(error).contains("buffers"));
    assert!(out.iter().all(|v| *v == SENTINEL));

    // The working limit is asked before anything is rasterized.
    let message = coverage_under(&shape, (0, 0), size, true, None, 1024, ptr::null()).unwrap_err();
    assert!(message.contains("working limit"), "{message}");
    // A cancelled job returns nothing.
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let result = coverage_under(&shape, (0, 0), size, true, None, LIMIT, control);
    unsafe { dr_job_control_free(control) };
    assert!(result.unwrap_err().contains("cancel"));
}

/// A closed subpath of two curves and a straight side, and an open curve.
fn curved() -> (Vec<Node>, Vec<Node>) {
    let closed = vec![
        Node {
            p: [10.0, 40.0],
            i: [10.0, 40.0],
            o: [15.0, 5.0],
        },
        Node {
            p: [50.0, 20.0],
            i: [35.0, -10.0],
            o: [70.0, 60.0],
        },
        Node {
            p: [30.0, 55.0],
            i: [45.0, 70.0],
            o: [30.0, 55.0],
        },
    ];
    let open = vec![
        Node {
            p: [60.0, 5.0],
            i: [60.0, 5.0],
            o: [90.0, 5.0],
        },
        Node {
            p: [80.0, 45.0],
            i: [60.0, 30.0],
            o: [80.0, 45.0],
        },
        corner(95.0, 50.0),
    ];
    (closed, open)
}

#[test]
fn a_flattened_path_stays_within_its_tolerance_of_the_bernstein_curve() {
    let (closed, open) = curved();
    let shape = shape_of(&[(&closed, true), (&open, false)], "nonzero");
    let mut counts = Vec::new();
    for tolerance in [2.0, 0.25, 0.01] {
        let polylines = flatten(&shape, tolerance).expect("polylines");
        assert_eq!(polylines.len(), 2);
        for ((points, is_closed), (nodes, want_closed)) in
            polylines.iter().zip([(&closed, true), (&open, false)])
        {
            assert_eq!(*is_closed, want_closed);
            assert_eq!(
                points[0], nodes[0].p,
                "a polyline starts at its first anchor"
            );
            let last = points[points.len() - 1];
            if want_closed {
                assert_ne!(last, nodes[0].p, "a closed polyline does not repeat it");
            } else {
                assert_eq!(
                    last,
                    nodes[nodes.len() - 1].p,
                    "an open one ends on its last"
                );
            }
            // Every anchor is a vertex, in order.
            let mut from = 0;
            for node in nodes {
                let at = points[from..]
                    .iter()
                    .position(|p| *p == node.p)
                    .unwrap_or_else(|| panic!("anchor {:?} is not a vertex", node.p));
                from += at;
            }
            // The curve, by Bernstein evaluation, never leaves the polyline
            // by more than the tolerance (plus the four-decimal rounding of
            // a vertex), and every vertex lies on the curve.
            let arcs = cubics(nodes, want_closed);
            let mut samples = Vec::new();
            for arc in &arcs {
                for step in 0..=400 {
                    samples.push(bernstein(arc, f64::from(step) / 400.0));
                }
            }
            for sample in &samples {
                let off = to_polyline(*sample, points, want_closed);
                assert!(off <= tolerance + 1e-4, "{off} at tolerance {tolerance}");
            }
            for vertex in points {
                // Samples are at most 0.4 px apart along these curves.
                let off = samples
                    .iter()
                    .map(|s| gap(*s, *vertex))
                    .fold(f64::MAX, f64::min);
                assert!(off <= 0.25, "vertex {vertex:?} is {off} from the curve");
            }
        }
        counts.push(polylines.iter().map(|(p, _)| p.len()).sum::<usize>());
    }
    assert!(counts[0] < counts[1] && counts[1] < counts[2], "{counts:?}");
    // The straight side of the closed subpath adds no vertex: at a loose
    // tolerance it is three anchors and the vertices its two curves need.
    let loose = flatten(&shape, 1e6).unwrap();
    assert_eq!(loose[0].0.len(), 3);
    assert_eq!(loose[1].0.len(), 3);

    // A lone anchor is one point; an empty shape no polyline.
    let dot = shape_of(&[(&[corner(4.0, 5.0)], true)], "nonzero");
    assert_eq!(flatten(&dot, 0.25).unwrap(), vec![(vec![[4.0, 5.0]], true)]);
    assert!(flatten(&json!({"subpaths": []}), 0.25).unwrap().is_empty());

    // Refusals: a tolerance that is not a positive number, and a shape that
    // would need more points than the cap.
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let message = flatten(&shape, bad).unwrap_err();
        assert!(message.contains("tolerance"), "{bad}: {message}");
    }
    let wall = 999_999.0;
    let wild: Vec<Node> = (0..40)
        .map(|k| {
            let s = if k % 2 == 0 { 1.0 } else { -1.0 };
            Node {
                p: [s * wall, s * wall],
                i: [s * wall, -s * wall],
                o: [s * wall, -s * wall],
            }
        })
        .collect();
    let message = flatten(&shape_of(&[(&wild, false)], "nonzero"), 1e-4).unwrap_err();
    assert!(message.contains("more than 1048576 points"), "{message}");
}

#[test]
fn an_inserted_anchor_keeps_the_curve() {
    let (closed, open) = curved();
    let shape = shape_of(&[(&closed, true), (&open, false)], "evenodd");
    let before = cubics(&closed, true);

    // A curved segment splits by de Casteljau: the left piece at s is the
    // old curve at s·t, the right piece at s is it at t + s·(1 − t).
    let t = 0.3;
    let split = insert(&shape, 0, 0, t).expect("an insert");
    assert_eq!(split["fill_rule"], "evenodd");
    let after = nodes(&split, 0);
    assert_eq!(after.len(), 4);
    assert!(split["subpaths"][0]["anchors"][1]["smooth"]
        .as_bool()
        .unwrap());
    let pieces = cubics(&after, true);
    for step in 0..=50 {
        let s = f64::from(step) / 50.0;
        // Control points are stored at four decimals.
        assert!(gap(bernstein(&pieces[0], s), bernstein(&before[0], s * t)) < 3e-4);
        assert!(
            gap(
                bernstein(&pieces[1], s),
                bernstein(&before[0], t + s * (1.0 - t))
            ) < 3e-4
        );
        // The other segments are untouched.
        assert!(gap(bernstein(&pieces[2], s), bernstein(&before[1], s)) < 1e-9);
        assert!(gap(bernstein(&pieces[3], s), bernstein(&before[2], s)) < 1e-9);
    }
    assert_eq!(nodes(&split, 1).len(), 3, "the other subpath is untouched");

    // The closing segment (index n − 1) is a straight side here: the new
    // anchor is a corner on it, appended after the last anchor.
    let closing = insert(&shape, 0, 2, 0.25).expect("an insert on the closing side");
    let after = nodes(&closing, 0);
    assert_eq!(after.len(), 4);
    let (a, b) = (closed[2].p, closed[0].p);
    let want = [a[0] + 0.25 * (b[0] - a[0]), a[1] + 0.25 * (b[1] - a[1])];
    assert!(gap(after[3].p, want) < 1e-4);
    assert_eq!(after[3].i, after[3].p);
    assert_eq!(after[3].o, after[3].p);
    assert!(!closing["subpaths"][0]["anchors"][3]["smooth"]
        .as_bool()
        .unwrap());
    for (now, was) in after.iter().zip(&closed) {
        assert_eq!(now.p, was.p);
    }

    // What cannot be inserted says so: an open subpath has no closing
    // segment, t must lie strictly inside the segment.
    assert!(insert(&shape, 1, 1, 0.5).is_ok());
    for (subpath, segment, t) in [
        (1, 2, 0.5),
        (2, 0, 0.5),
        (0, 3, 0.5),
        (0, 0, 0.0),
        (0, 0, 1.0),
        (0, 0, f64::NAN),
    ] {
        let message = insert(&shape, subpath, segment, t).unwrap_err();
        assert!(message.contains("no anchor can be inserted"), "{message}");
    }
    // A full shape has no room for one more.
    let corner_pair = json!([1.5, 2.5]);
    let full =
        json!({"subpaths": [{"closed": true, "anchors": vec![corner_pair; MAX_PATH_ANCHORS]}]});
    assert!(insert(&full, 0, 0, 0.5).is_err());
}

#[test]
fn a_hit_finds_anchors_first_then_handles_then_segments() {
    let curve = [
        corner(10.0, 10.0),
        Node {
            p: [30.0, 10.0],
            i: [25.0, 5.0],
            o: [35.0, 15.0],
        },
        corner(50.0, 30.0),
    ];
    let triangle = [corner(60.0, 10.0), corner(80.0, 10.0), corner(70.0, 30.0)];
    let shape = shape_of(&[(&curve, false), (&triangle, true)], "nonzero");
    assert_eq!(
        hit(&shape, [10.4, 10.3], 1.0),
        json!({"kind": "anchor", "subpath": 0, "index": 0})
    );
    assert_eq!(
        hit(&shape, [25.2, 5.1], 1.0),
        json!({"kind": "handle_in", "subpath": 0, "index": 1})
    );
    assert_eq!(
        hit(&shape, [35.0, 15.0], 1.0),
        json!({"kind": "handle_out", "subpath": 0, "index": 1})
    );
    // The nearest of two anchors in reach.
    assert_eq!(
        hit(&shape, [69.0, 29.0], 30.0),
        json!({"kind": "anchor", "subpath": 1, "index": 2})
    );
    // A curved segment answers its own parameter.
    let arc = cubics(&curve, false)[0];
    for t in [0.2, 0.5, 0.85] {
        let on = bernstein(&arc, t);
        let found = hit(&shape, [on[0] + 0.1, on[1] - 0.1], 0.5);
        assert_eq!(found["kind"], "segment", "{found}");
        assert_eq!(
            (found["subpath"].as_u64(), found["index"].as_u64()),
            (Some(0), Some(0))
        );
        let at = bernstein(&arc, found["t"].as_f64().unwrap());
        assert!(gap(at, on) < 0.2, "t {t}: {found}");
    }
    // A straight segment answers the fraction of its length; the closing
    // side of the triangle is segment 2.
    let found = hit(&shape, [65.0, 10.4], 1.0);
    assert_eq!(found["kind"], "segment");
    assert_eq!(
        (found["subpath"].as_u64(), found["index"].as_u64()),
        (Some(1), Some(0))
    );
    assert!((found["t"].as_f64().unwrap() - 0.25).abs() < 1e-9);
    let found = hit(&shape, [65.0, 20.0], 1.0);
    assert_eq!(
        (found["subpath"].as_u64(), found["index"].as_u64()),
        (Some(1), Some(2))
    );
    assert!((found["t"].as_f64().unwrap() - 0.5).abs() < 1e-9);
    // An anchor wins over the segments that end on it; an open subpath's
    // fill closure (50, 30) → (10, 10) is not a segment.
    assert_eq!(hit(&shape, [60.0, 10.0], 2.0)["kind"], "anchor");
    assert_eq!(hit(&shape, [30.0, 20.0], 1.0), json!({"kind": "none"}));
    // Misses.
    assert_eq!(hit(&shape, [200.0, 200.0], 5.0), json!({"kind": "none"}));
    assert_eq!(hit(&shape, [10.0, 10.0], -1.0), json!({"kind": "none"}));
    assert_eq!(hit(&shape, [10.0, 10.0], f64::NAN), json!({"kind": "none"}));
    assert_eq!(
        hit(&json!({"subpaths": []}), [0.0, 0.0], 5.0),
        json!({"kind": "none"})
    );
    let garbage = CString::new("[]").unwrap();
    assert!(unsafe { dr_path_hit(garbage.as_ptr(), 0.0, 0.0, 1.0, true) }.is_null());
}

/// The Pen's plain click cannot grab a handle, so it asks with `handles`
/// false: a handle tip lying on a segment must not hide the segment.
#[test]
fn a_hit_without_handles_reaches_the_segment_under_a_handle_tip() {
    // The smooth anchor's out handle ends at (30, 40), which is the middle
    // of the straight segment (10, 40) → (50, 40) of the second subpath.
    let curve = [
        corner(10.0, 20.0),
        Node {
            p: [30.0, 20.0],
            i: [30.0, 0.0],
            o: [30.0, 40.0],
        },
        corner(60.0, 20.0),
    ];
    let line = [corner(10.0, 40.0), corner(50.0, 40.0)];
    let shape = shape_of(&[(&curve, false), (&line, false)], "nonzero");
    let tip = [30.2, 40.1];
    assert_eq!(
        hit_offering(&shape, tip, 1.0, true),
        json!({"kind": "handle_out", "subpath": 0, "index": 1})
    );
    let found = hit_offering(&shape, tip, 1.0, false);
    assert_eq!(found["kind"], "segment", "{found}");
    assert_eq!(
        (found["subpath"].as_u64(), found["index"].as_u64()),
        (Some(1), Some(0))
    );
    // The oracle: the foot of the perpendicular from the point onto the
    // segment, as a fraction of its 40-pixel length.
    assert!((found["t"].as_f64().unwrap() - (30.2 - 10.0) / 40.0).abs() < 1e-9);
    // A handle tip with no segment under it is nothing at all…
    assert_eq!(
        hit_offering(&shape, [30.0, 0.0], 1.0, true),
        json!({"kind": "handle_in", "subpath": 0, "index": 1})
    );
    assert_eq!(
        hit_offering(&shape, [30.0, 0.0], 1.0, false),
        json!({"kind": "none"})
    );
    // …and an anchor still wins over the segments that end on it.
    assert_eq!(
        hit_offering(&shape, [30.3, 20.2], 1.0, false),
        json!({"kind": "anchor", "subpath": 0, "index": 1})
    );
}

#[test]
fn converting_an_anchor_sets_its_handles_by_rule_and_refuses_the_same_anchor() {
    let curve = [
        corner(10.0, 10.0),
        Node {
            p: [30.0, 10.0],
            i: [25.0, 5.0],
            o: [35.0, 15.0],
        },
        corner(50.0, 30.0),
    ];
    let triangle = [corner(60.0, 10.0), corner(80.0, 10.0), corner(70.0, 30.0)];
    let shape = shape_of(
        &[
            (&curve, false),
            (&triangle, true),
            (&[corner(5.0, 5.0)], true),
        ],
        "nonzero",
    );

    // To a corner: both handles onto the point; a second time is no change.
    let cornered = convert(&shape, 0, 1, false).unwrap().expect("a corner");
    let after = nodes(&cornered, 0);
    assert_eq!((after[1].i, after[1].o), ([30.0, 10.0], [30.0, 10.0]));
    assert!(!cornered["subpaths"][0]["anchors"][1]["smooth"]
        .as_bool()
        .unwrap());
    assert_eq!(convert(&cornered, 0, 1, false), Ok(None));
    assert_eq!(convert(&shape, 0, 0, false), Ok(None), "already a corner");

    // To smooth, in a closed subpath: along previous → next, a third of
    // the way to each neighbour.
    let smoothed = convert(&shape, 1, 0, true)
        .unwrap()
        .expect("a smooth anchor");
    let after = nodes(&smoothed, 1);
    let (p, previous, next) = ([60.0f64, 10.0], [70.0f64, 30.0], [80.0f64, 10.0]);
    let along = [next[0] - previous[0], next[1] - previous[1]];
    let length = along[0].hypot(along[1]);
    let unit = [along[0] / length, along[1] / length];
    let (back, ahead) = (gap(previous, p) / 3.0, gap(next, p) / 3.0);
    assert!(gap(after[0].o, [p[0] + unit[0] * ahead, p[1] + unit[1] * ahead]) < 1e-4);
    assert!(gap(after[0].i, [p[0] - unit[0] * back, p[1] - unit[1] * back]) < 1e-4);
    assert!(smoothed["subpaths"][1]["anchors"][0]["smooth"]
        .as_bool()
        .unwrap());
    assert_eq!(after[0].p, p);
    assert_eq!(after[1].p, [80.0, 10.0], "the neighbours are untouched");
    assert_eq!(
        convert(&smoothed, 1, 0, true),
        Ok(None),
        "already that anchor"
    );

    // The end of an open subpath has one neighbour: one handle, facing it.
    let ended = convert(&shape, 0, 0, true).unwrap().expect("a smooth end");
    let after = nodes(&ended, 0);
    assert!(gap(after[0].o, [10.0 + 20.0 / 3.0, 10.0]) < 1e-4);
    assert_eq!(after[0].i, [10.0, 10.0]);

    // A lone anchor has nothing to aim at; a missing one is an error.
    assert_eq!(convert(&shape, 2, 0, true), Ok(None));
    for (subpath, anchor) in [(0, 3), (3, 0)] {
        let message = convert(&shape, subpath, anchor, true).unwrap_err();
        assert!(message.contains("no such anchor"), "{message}");
    }
}
