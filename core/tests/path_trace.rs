//! Make Work Path — `dr_path_trace`, a u16 coverage plane to closed
//! Bézier subpaths under the even-odd rule.
//!
//! The oracle is the SHAPE that was sampled, never the tracer: a disc, a
//! box, an annulus and a half disc, each given here as the distance from a
//! point to its true boundary. A trace at tolerance `t` must (1) keep every
//! point of its outline within `t + 1` pixels of that boundary — `t` for the
//! fit, one pixel for the grid the contour is sampled on — and (2) fill, at
//! a hard edge, exactly the pixels of the plane wherever a pixel's centre is
//! further than `t + 1` from the boundary. Planes are sampled 16 x 16 a
//! pixel, here.

use std::ptr;

use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_path::dr_path_trace;
use doctored_core::job_control::DrJobControl;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::path_geometry::{take, LIMIT};
use crate::path_shape::{coverage, flatten};

type Size = (u32, u32);

fn trace_under(
    plane: &[u16],
    size: Size,
    tolerance: f64,
    limit: u64,
    control: *const DrJobControl,
) -> Result<Value, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_path_trace(
            plane.as_ptr(),
            plane.len(),
            size.0,
            size.1,
            tolerance,
            limit,
            0,
            control,
            &mut error,
        )
    };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    assert!(error.is_null());
    Ok(serde_json::from_str(&take(out)).expect("shape JSON"))
}

fn trace(plane: &[u16], size: Size, tolerance: f64) -> Result<Value, String> {
    trace_under(plane, size, tolerance, LIMIT, ptr::null())
}

/// The coverage of `inside` over a canvas, 16 x 16 samples a pixel.
fn sampled(size: Size, inside: impl Fn(f64, f64) -> bool) -> Vec<u16> {
    let mut plane = Vec::with_capacity((size.0 * size.1) as usize);
    for y in 0..size.1 {
        for x in 0..size.0 {
            let mut hits = 0u32;
            for j in 0..16 {
                for i in 0..16 {
                    let px = f64::from(x) + (f64::from(i) + 0.5) / 16.0;
                    let py = f64::from(y) + (f64::from(j) + 0.5) / 16.0;
                    hits += u32::from(inside(px, py));
                }
            }
            plane.push((f64::from(hits) / 256.0 * 65535.0).round() as u16);
        }
    }
    plane
}

fn subpaths(shape: &Value) -> &Vec<Value> {
    shape["subpaths"].as_array().expect("subpaths")
}

fn anchors(shape: &Value, subpath: usize) -> &Vec<Value> {
    shape["subpaths"][subpath]["anchors"]
        .as_array()
        .expect("anchors")
}

fn point(anchor: &Value) -> [f64; 2] {
    [anchor["x"].as_f64().unwrap(), anchor["y"].as_f64().unwrap()]
}

fn pair(value: &Value) -> [f64; 2] {
    [value[0].as_f64().unwrap(), value[1].as_f64().unwrap()]
}

/// Whether an anchor is a plain corner: both handles on the point.
fn is_corner(anchor: &Value) -> bool {
    let p = point(anchor);
    pair(&anchor["in"]) == p && pair(&anchor["out"]) == p && anchor["smooth"] == false
}

fn gap(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The two checks of the module doc, for a trace of `plane` whose true
/// boundary is `boundary` away from a point.
fn assert_follows(
    shape: &Value,
    plane: &[u16],
    size: Size,
    tolerance: f64,
    boundary: impl Fn([f64; 2]) -> f64,
    what: &str,
) {
    assert_eq!(shape["fill_rule"], "evenodd", "{what}");
    let reach = tolerance + 1.0;
    for (points, closed) in flatten(shape, 0.05).expect("the traced outline") {
        assert!(closed, "{what}: a traced subpath is closed");
        for p in points {
            let off = boundary(p);
            assert!(off <= reach, "{what}: {p:?} is {off} from the boundary");
        }
    }
    let again = coverage(shape, (0, 0), size, false, None).expect("the traced fill");
    for y in 0..size.1 {
        for x in 0..size.0 {
            let i = (y * size.0 + x) as usize;
            if boundary([f64::from(x) + 0.5, f64::from(y) + 0.5]) > reach {
                assert_eq!(
                    again[i] >= 32768,
                    plane[i] >= 32768,
                    "{what}: pixel ({x}, {y}) changed sides"
                );
            }
        }
    }
}

#[test]
fn a_disc_traces_to_one_smooth_loop_within_its_tolerance() {
    let size = (64u32, 64u32);
    let (centre, r) = ([32.3, 30.7], 20.5);
    let plane = sampled(size, |x, y| gap([x, y], centre) <= r);
    let boundary = |p: [f64; 2]| (gap(p, centre) - r).abs();
    let mut counts = Vec::new();
    for tolerance in [0.5, 1.0, 2.0, 4.0] {
        let shape = trace(&plane, size, tolerance).expect("a trace");
        let what = format!("disc at {tolerance}");
        assert_eq!(subpaths(&shape).len(), 1, "{what}");
        assert_eq!(shape["subpaths"][0]["closed"], true);
        assert_follows(&shape, &plane, size, tolerance, boundary, &what);
        let ring = anchors(&shape, 0);
        assert!(
            (2..=24).contains(&ring.len()),
            "{what}: {} anchors",
            ring.len()
        );
        for anchor in ring {
            // No corner on a circle: every anchor is smooth, its handles off
            // the point and in one line through it.
            assert_eq!(anchor["smooth"], true, "{what}: {anchor}");
            let p = point(anchor);
            let (arriving, leaving) = (pair(&anchor["in"]), pair(&anchor["out"]));
            let (a, b) = (
                [p[0] - arriving[0], p[1] - arriving[1]],
                [leaving[0] - p[0], leaving[1] - p[1]],
            );
            let (la, lb) = (a[0].hypot(a[1]), b[0].hypot(b[1]));
            assert!(la > 0.01 && lb > 0.01, "{what}: a handle sits on its point");
            let sine = (a[0] * b[1] - a[1] * b[0]) / (la * lb);
            let cosine = (a[0] * b[0] + a[1] * b[1]) / (la * lb);
            // Handles are stored at four decimals.
            assert!(sine.abs() < 1e-2 && cosine > 0.0, "{what}: a kinked anchor");
        }
        counts.push(ring.len());
    }
    assert!(
        counts[0] >= counts[3],
        "a looser fit needs no more anchors: {counts:?}"
    );
}

/// The distance from `p` to the outline of the box `[x0, x1] x [y0, y1]`.
fn to_box(p: [f64; 2], x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
    let dx = (x0 - p[0]).max(p[0] - x1);
    let dy = (y0 - p[1]).max(p[1] - y1);
    if dx <= 0.0 && dy <= 0.0 {
        -dx.max(dy)
    } else {
        dx.max(0.0).hypot(dy.max(0.0))
    }
}

#[test]
fn a_hard_box_traces_to_four_corners_joined_by_straight_sides() {
    let size = (64u32, 64u32);
    let (x0, y0, x1, y1) = (10.0, 12.0, 50.0, 40.0);
    let plane = sampled(size, |x, y| x >= x0 && x < x1 && y >= y0 && y < y1);
    assert!(plane.iter().all(|v| *v == 0 || *v == 65535), "a hard edge");
    let boundary = |p: [f64; 2]| to_box(p, x0, y0, x1, y1);
    let shape = trace(&plane, size, 1.0).expect("a trace");
    assert_follows(&shape, &plane, size, 1.0, boundary, "box at 1");
    assert_eq!(subpaths(&shape).len(), 1);
    let ring = anchors(&shape, 0);
    assert_eq!(ring.len(), 4, "{shape}");
    assert!(ring.iter().all(is_corner), "{shape}");
    // Each true corner has its own anchor within a pixel.
    for corner in [[x0, y0], [x1, y0], [x1, y1], [x0, y1]] {
        let near = ring.iter().filter(|a| gap(point(a), corner) <= 1.0).count();
        assert_eq!(near, 1, "corner {corner:?}: {shape}");
    }

    // Tighter and looser tolerances follow the same box.
    for tolerance in [0.25, 0.5, 3.0] {
        let shape = trace(&plane, size, tolerance).expect("a trace");
        let what = format!("box at {tolerance}");
        assert_follows(&shape, &plane, size, tolerance, boundary, &what);
        assert_eq!(subpaths(&shape).len(), 1, "{what}");
    }

    // A whole plane traces to the canvas's own outline.
    let whole = (16u32, 12u32);
    let full = vec![65535u16; 16 * 12];
    let shape = trace(&full, whole, 1.0).expect("a trace");
    let edge = |p: [f64; 2]| to_box(p, 0.0, 0.0, 16.0, 12.0);
    assert_follows(&shape, &full, whole, 1.0, edge, "the whole canvas");
    assert_eq!(anchors(&shape, 0).len(), 4);
}

#[test]
fn an_annulus_traces_to_an_outline_and_its_hole() {
    let size = (64u32, 64u32);
    let (centre, outer, inner) = ([32.0, 32.0], 24.0, 10.0);
    let plane = sampled(size, |x, y| {
        let d = gap([x, y], centre);
        d <= outer && d >= inner
    });
    let boundary = |p: [f64; 2]| {
        let d = gap(p, centre);
        (d - outer).abs().min((d - inner).abs())
    };
    for tolerance in [0.5, 1.5] {
        let shape = trace(&plane, size, tolerance).expect("a trace");
        let what = format!("annulus at {tolerance}");
        assert_eq!(subpaths(&shape).len(), 2, "{what}");
        assert_follows(&shape, &plane, size, tolerance, boundary, &what);
        // Even-odd makes the inner loop a hole: the centre stays empty.
        let again = coverage(&shape, (0, 0), size, true, None).unwrap();
        assert_eq!(again[32 * 64 + 32], 0, "{what}: the centre");
        assert_eq!(again[32 * 64 + 49], 65535, "{what}: the ring");
    }
}

#[test]
fn a_half_disc_keeps_its_two_corners_and_its_straight_side() {
    let size = (64u32, 64u32);
    let (centre, r) = ([32.0, 32.0], 20.0);
    let plane = sampled(size, |x, y| x >= centre[0] && gap([x, y], centre) <= r);
    let ends = [[32.0, 12.0], [32.0, 52.0]];
    let boundary = |p: [f64; 2]| {
        let arc = if p[0] >= centre[0] {
            (gap(p, centre) - r).abs()
        } else {
            gap(p, ends[0]).min(gap(p, ends[1]))
        };
        let side = if (ends[0][1]..=ends[1][1]).contains(&p[1]) {
            (p[0] - centre[0]).abs()
        } else {
            gap(p, ends[0]).min(gap(p, ends[1]))
        };
        arc.min(side)
    };
    let shape = trace(&plane, size, 1.0).expect("a trace");
    assert_follows(&shape, &plane, size, 1.0, boundary, "half disc at 1");
    assert_eq!(subpaths(&shape).len(), 1);
    let ring = anchors(&shape, 0);
    let sharp: Vec<usize> = (0..ring.len())
        .filter(|&k| ring[k]["smooth"] == false)
        .collect();
    assert_eq!(sharp.len(), 2, "two corners: {shape}");
    let (a, b) = (sharp[0], sharp[1]);
    assert!(
        gap(point(&ring[a]), ends[0]) <= 1.5 && gap(point(&ring[b]), ends[1]) <= 1.5
            || gap(point(&ring[a]), ends[1]) <= 1.5 && gap(point(&ring[b]), ends[0]) <= 1.5,
        "one corner anchor at each end of the side: {shape}"
    );
    // The straight side joins the two corners directly: one of them leaves
    // on a line (its out handle on its point) that the other receives, and
    // the arc leaves the other on a curve.
    let straight = |from: usize, to: usize| {
        (from + 1) % ring.len() == to
            && pair(&ring[from]["out"]) == point(&ring[from])
            && pair(&ring[to]["in"]) == point(&ring[to])
    };
    assert!(straight(a, b) != straight(b, a), "{shape}");

    // A tighter fit follows the same shape.
    for tolerance in [0.25, 0.5] {
        let shape = trace(&plane, size, tolerance).expect("a trace");
        let what = format!("half disc at {tolerance}");
        assert_follows(&shape, &plane, size, tolerance, boundary, &what);
        assert_eq!(subpaths(&shape).len(), 1, "{what}");
    }
}

#[test]
fn nothing_to_trace_is_an_empty_shape_and_a_speck_under_the_tolerance_is_dropped() {
    let size = (16u32, 16u32);
    let mut plane = vec![0u16; 256];
    assert_eq!(
        trace(&plane, size, 2.0),
        Ok(json!({"fill_rule": "evenodd", "subpaths": []}))
    );
    // Just under one half is outside.
    plane.fill(32767);
    assert_eq!(subpaths(&trace(&plane, size, 2.0).unwrap()).len(), 0);
    // One pixel: a loop one pixel across, kept at a quarter-pixel tolerance
    // and dropped at two.
    plane.fill(0);
    plane[5 * 16 + 7] = 65535;
    assert_eq!(subpaths(&trace(&plane, size, 2.0).unwrap()).len(), 0);
    let speck = trace(&plane, size, 0.25).unwrap();
    assert_eq!(subpaths(&speck).len(), 1);
    for anchor in anchors(&speck, 0) {
        assert!(
            to_box(point(anchor), 7.0, 5.0, 8.0, 6.0) <= 0.75,
            "{anchor}"
        );
    }
}

#[test]
fn a_trace_refuses_bad_input_admission_a_cancel_and_what_it_cannot_hold() {
    let size = (16u32, 16u32);
    let mut plane = vec![0u16; 256];
    plane[5 * 16 + 7] = 65535;
    for bad in [0.0, 0.2, 64.5, -1.0, f64::NAN, f64::INFINITY] {
        let message = trace(&plane, size, bad).unwrap_err();
        assert!(message.contains("tolerance"), "{bad}: {message}");
    }
    // A plane that is not its dimensions, a zero plane, and no plane.
    let message = trace(&plane[..255], size, 2.0).unwrap_err();
    assert!(message.contains("buffer"), "{message}");
    assert!(trace(&[], (0, 16), 2.0).is_err());
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_path_trace(
            ptr::null(),
            256,
            16,
            16,
            2.0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(out.is_null());
    assert!(take_err_string(error).contains("buffer"));
    // The working limit is asked first; a cancelled job returns nothing.
    let message = trace_under(&plane, size, 2.0, 1024, ptr::null()).unwrap_err();
    assert!(message.contains("working limit"), "{message}");
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let result = trace_under(&plane, size, 2.0, LIMIT, control);
    unsafe { dr_job_control_free(control) };
    assert!(result.unwrap_err().contains("cancel"));

    // A checkerboard of single pixels: at a quarter pixel every one is its
    // own loop, more than a shape holds; the sentence says what to do.
    let board = |size: Size| -> Vec<u16> {
        (0..size.0 * size.1)
            .map(|i| {
                if (i % size.0 + i / size.0).is_multiple_of(2) {
                    65535
                } else {
                    0
                }
            })
            .collect()
    };
    let message = trace(&board((256, 256)), (256, 256), 0.25).unwrap_err();
    assert!(message.contains("raise the tolerance"), "{message}");
    // At two pixels each loop is dropped as it is found, so a board of a
    // million of them runs into the contour bound instead.
    let message = trace(&board((2048, 1024)), (2048, 1024), 2.0).unwrap_err();
    assert!(message.contains("too intricate"), "{message}");
}
