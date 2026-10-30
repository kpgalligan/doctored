//! Face Parts: `dr_landmark_coverage`, landmark point loops to u16
//! coverage through closed centripetal Catmull–Rom splines.
//!
//! The oracle is the CURVE, evaluated here independently of the core: the
//! Barry–Goldman pyramid of the centripetal (α = ½) Catmull–Rom spline
//! through the same points, sampled 2,000 times a segment, whose shoelace
//! area a covered loop's summed coverage must match within
//! `0.5 + 1e-4·A` pixels (flattening at 1/4096 px and u16 rounding of each
//! edge pixel, both far below the half pixel). Region algebra (clip, exclude,
//! union, max) is checked against the same calls made one plane at a time.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_landmark::dr_landmark_coverage;
use doctored_core::job_control::DrJobControl;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::path_geometry::LIMIT;

const W: u32 = 280;
const H: u32 = 200;
const SENTINEL: u16 = 0x1234;

type P = [f64; 2];

fn coverage_under(
    request: &str,
    size: (u32, u32),
    limit: u64,
    control: *const DrJobControl,
) -> Result<Vec<u16>, String> {
    let text = CString::new(request).unwrap();
    let mut output = vec![SENTINEL; (size.0 * size.1) as usize];
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_landmark_coverage(
            text.as_ptr(),
            size.0,
            size.1,
            output.as_mut_ptr(),
            output.len(),
            limit,
            0,
            control,
            &mut error,
        )
    };
    if !ok {
        assert!(
            output.iter().all(|&v| v == SENTINEL),
            "a refused call wrote its output"
        );
        return Err(take_err_string(error));
    }
    assert!(error.is_null());
    Ok(output)
}

fn coverage(request: &Value) -> Vec<u16> {
    coverage_under(&request.to_string(), (W, H), LIMIT, ptr::null()).expect("coverage")
}

fn refused(request: &str) -> String {
    coverage_under(request, (W, H), LIMIT, ptr::null()).unwrap_err()
}

fn closed(points: &[P]) -> Value {
    json!({ "points": points })
}

fn region(include: Vec<Value>) -> Value {
    json!({ "include": include })
}

fn request(regions: Vec<Value>) -> Value {
    json!({ "regions": regions })
}

/// Summed coverage in pixels.
fn area(plane: &[u16]) -> f64 {
    plane.iter().map(|&v| f64::from(v)).sum::<f64>() / 65535.0
}

fn shoelace(points: &[P]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        * 0.5
}

/// The closed centripetal Catmull–Rom curve through `points`, by the
/// Barry–Goldman pyramid, 2,000 samples a segment.
fn barry_goldman(points: &[P]) -> Vec<P> {
    let n = points.len();
    let at = |i: usize| points[i % n];
    let lerp = |a: P, b: P, ta: f64, tb: f64, t: f64| {
        let u = (t - ta) / (tb - ta);
        [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]
    };
    let knot = |a: P, b: P| ((a[0] - b[0]).hypot(a[1] - b[1])).sqrt();
    let mut samples = Vec::with_capacity(n * 2000);
    for i in 0..n {
        let (p0, p1, p2, p3) = (at(i + n - 1), at(i), at(i + 1), at(i + 2));
        let t0 = 0.0;
        let t1 = t0 + knot(p0, p1);
        let t2 = t1 + knot(p1, p2);
        let t3 = t2 + knot(p2, p3);
        for s in 0..2000 {
            let t = t1 + (t2 - t1) * f64::from(s) / 2000.0;
            let a1 = lerp(p0, p1, t0, t1, t);
            let a2 = lerp(p1, p2, t1, t2, t);
            let a3 = lerp(p2, p3, t2, t3, t);
            let b1 = lerp(a1, a2, t0, t2, t);
            let b2 = lerp(a2, a3, t1, t3, t);
            samples.push(lerp(b1, b2, t1, t2, t));
        }
    }
    samples
}

fn curve_area(points: &[P]) -> f64 {
    shoelace(&barry_goldman(points)).abs()
}

fn assert_area(measured: f64, expected: f64, what: &str) {
    let bound = 0.5 + 1e-4 * expected;
    assert!(
        (measured - expected).abs() <= bound,
        "{what}: coverage {measured} against the curve's {expected} (bound {bound})"
    );
}

fn circle(cx: f64, cy: f64, r: f64, n: usize) -> Vec<P> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect()
}

/// An eye-like almond with deliberately uneven spacing.
fn almond() -> Vec<P> {
    vec![
        [40.0, 100.0],
        [52.0, 88.0],
        [75.0, 81.0],
        [118.0, 84.0],
        [150.0, 100.0],
        [128.0, 111.0],
        [80.0, 117.0],
        [55.0, 112.0],
    ]
}

#[test]
fn a_closed_loop_covers_the_centripetal_curve_through_its_points() {
    let ring = circle(140.0, 100.0, 60.0, 12);
    let measured = area(&coverage(&request(vec![region(vec![closed(&ring)])])));
    let expected = curve_area(&ring);
    assert_area(measured, expected, "12-point circle");
    let disc = std::f64::consts::PI * 60.0 * 60.0;
    assert!(
        (measured - disc).abs() <= 0.01 * disc,
        "{measured} against πR² {disc}"
    );
    assert!(
        measured >= shoelace(&ring).abs(),
        "a convex set's spline holds its polygon"
    );

    let eye = almond();
    let measured = area(&coverage(&request(vec![region(vec![closed(&eye)])])));
    assert_area(measured, curve_area(&eye), "uneven almond");
}

#[test]
fn exclude_cuts_and_clip_confines() {
    let outer = circle(140.0, 100.0, 60.0, 12);
    let inner = circle(140.0, 100.0, 30.0, 10);
    let ring = coverage(&request(vec![json!({
        "include": [closed(&outer)],
        "exclude": [closed(&inner)],
    })]));
    assert_area(area(&ring), curve_area(&outer) - curve_area(&inner), "ring");

    let eye = almond();
    let alone = coverage(&request(vec![region(vec![closed(&eye)])]));
    // A clip that holds the include's whole control box leaves it as is.
    let big = circle(95.0, 100.0, 140.0, 16);
    let held = coverage(&request(vec![json!({
        "include": [closed(&eye)],
        "clip": [closed(&big)],
    })]));
    assert_eq!(held, alone, "a containing clip changes nothing");
    // A disjoint clip leaves nothing.
    let far = circle(230.0, 100.0, 20.0, 8);
    let none = coverage(&request(vec![json!({
        "include": [closed(&eye)],
        "clip": [closed(&far)],
    })]));
    assert!(
        none.iter().all(|&v| v == 0),
        "a disjoint clip covers nothing"
    );
    // An overlapping clip is never more than either alone.
    let half = circle(140.0, 100.0, 40.0, 10);
    let half_alone = coverage(&request(vec![region(vec![closed(&half)])]));
    let both = coverage(&request(vec![json!({
        "include": [closed(&eye)],
        "clip": [closed(&half)],
    })]));
    assert!(both.iter().any(|&v| v > 0), "the overlap is covered");
    for ((&b, &e), &h) in both.iter().zip(&alone).zip(&half_alone) {
        assert!(u32::from(b) <= u32::from(e.min(h)) + 1);
    }
}

#[test]
fn a_band_covers_the_spline_of_its_offset_outline() {
    let line: Vec<P> = (0..11)
        .map(|i| [40.0 + 20.0 * f64::from(i), 100.0])
        .collect();
    let h = 8.0;
    let band = coverage(&request(vec![region(vec![
        json!({ "points": line, "half_width": h }),
    ])]));
    // The offset outline of a straight line: one side, then the other back.
    let mut outline: Vec<P> = line.iter().map(|p| [p[0], p[1] + h]).collect();
    outline.extend(line.iter().rev().map(|p| [p[0], p[1] - h]));
    let measured = area(&band);
    assert_area(measured, curve_area(&outline), "straight band");
    let rectangle = 2.0 * h * 200.0;
    assert!(
        (measured - rectangle).abs() <= 0.05 * rectangle,
        "{measured} against 2hL {rectangle}"
    );
}

#[test]
fn regions_and_loops_are_unions() {
    let a = circle(100.0, 100.0, 40.0, 10);
    let b = circle(170.0, 100.0, 40.0, 10);
    let first = coverage(&request(vec![region(vec![closed(&a)])]));
    let second = coverage(&request(vec![region(vec![closed(&b)])]));
    let two = coverage(&request(vec![
        region(vec![closed(&a)]),
        region(vec![closed(&b)]),
    ]));
    for ((&t, &x), &y) in two.iter().zip(&first).zip(&second) {
        assert_eq!(t, x.max(y), "two regions are the per-sample max");
    }
    // A clockwise loop beside a counter-clockwise one in one list is still a
    // union: the overlap is wholly covered, not cancelled.
    let mut reversed = b.clone();
    reversed.reverse();
    let union = coverage(&request(vec![region(vec![closed(&a), closed(&reversed)])]));
    let overlap = (100 * W + 135) as usize;
    assert_eq!(union[overlap], 65535, "the overlap is covered");
    assert_eq!(first[overlap], 65535);
    assert_eq!(second[overlap], 65535);
}

#[test]
fn degenerate_input_covers_nothing_and_refuses_nothing() {
    let empty = coverage(&request(vec![]));
    assert!(empty.iter().all(|&v| v == 0), "no region covers nothing");

    let eye = almond();
    let alone = coverage(&request(vec![region(vec![closed(&eye)])]));
    let collapsed = vec![[10.0, 10.0], [10.0, 10.0], [10.0, 10.0]];
    let beside = coverage(&request(vec![
        region(vec![closed(&collapsed)]),
        region(vec![closed(&eye)]),
    ]));
    assert_eq!(beside, alone, "a collapsed include contributes nothing");
    let off = vec![[-50.0, -50.0], [-40.0, -50.0], [-40.0, -40.0]];
    let beside = coverage(&request(vec![
        region(vec![closed(&off)]),
        region(vec![closed(&eye)]),
    ]));
    assert_eq!(beside, alone, "an off-canvas include contributes nothing");
    // A degenerate exclude excludes nothing.
    let cut = coverage(&request(vec![json!({
        "include": [closed(&eye)],
        "exclude": [closed(&collapsed)],
    })]));
    assert_eq!(cut, alone, "a collapsed exclude excludes nothing");
    // A clip list of only degenerate loops covers nothing.
    let clipped = coverage(&request(vec![json!({
        "include": [closed(&eye)],
        "clip": [closed(&collapsed)],
    })]));
    assert!(
        clipped.iter().all(|&v| v == 0),
        "a collapsed clip covers nothing"
    );

    // A loop repeating its first point is the loop without it.
    let repeated = coverage(&request(vec![region(vec![closed(&[
        [100.0, 60.0],
        [160.0, 60.0],
        [160.0, 120.0],
        [100.0, 60.0],
    ])])]));
    let triangle = coverage(&request(vec![region(vec![closed(&[
        [100.0, 60.0],
        [160.0, 60.0],
        [160.0, 120.0],
    ])])]));
    assert_eq!(repeated, triangle);
    assert!(area(&triangle) > 0.0);
}

#[test]
fn malformed_requests_and_refused_jobs_leave_the_output_untouched() {
    let eye = almond();
    let good = request(vec![region(vec![closed(&eye)])]).to_string();
    for (text, expected) in [
        ("not json".to_string(), "malformed"),
        ("{}".to_string(), "regions"),
        (r#"{"regions":[{}]}"#.to_string(), "include"),
        (r#"{"regions":[{"include":[]}]}"#.to_string(), "empty"),
        (
            r#"{"regions":[{"include":[{"points":[[0,0],[1,1]]}]}]}"#.to_string(),
            "three",
        ),
        (
            r#"{"regions":[{"include":[{"points":[[0,0]],"half_width":2}]}]}"#.to_string(),
            "two",
        ),
        (
            r#"{"regions":[{"include":[{"points":[[0,0],[9,0]],"half_width":0}]}]}"#.to_string(),
            "half_width",
        ),
        (
            r#"{"regions":[{"include":[{"points":[[0,0],[1,1],[2000000,0]]}]}]}"#.to_string(),
            "outside",
        ),
        (
            r#"{"regions":[{"include":[{"points":[[0,0],[1,1],[1]]}]}]}"#.to_string(),
            "pair",
        ),
        (
            request(vec![region(vec![closed(&eye)]); 1025]).to_string(),
            "too many landmark regions",
        ),
        (
            request(vec![region(vec![closed(&eye); 17])]).to_string(),
            "too many loops",
        ),
        (
            request(vec![region(vec![closed(&vec![[1.0, 2.0]; 1025])])]).to_string(),
            "too many points in a landmark loop",
        ),
    ] {
        let message = refused(&text);
        assert!(message.contains(expected), "{expected}: {message}");
    }
    // The buffers are the canvas's.
    let text = CString::new(good.clone()).unwrap();
    let mut output = vec![SENTINEL; 4];
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_landmark_coverage(
            text.as_ptr(),
            W,
            H,
            output.as_mut_ptr(),
            4,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!ok);
    assert!(take_err_string(error).contains("buffers"));
    assert_eq!(output, vec![SENTINEL; 4]);
    // The working limit is asked first; a cancelled job returns nothing.
    let message = coverage_under(&good, (W, H), 1024, ptr::null()).unwrap_err();
    assert!(message.contains("working limit"), "{message}");
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let result = coverage_under(&good, (W, H), LIMIT, control);
    unsafe { dr_job_control_free(control) };
    assert!(result.unwrap_err().contains("cancel"));
}
