//! LANDMARK REGIONS (Select ▸ Face Parts…) — smooth u16 coverage of
//! regions described by landmark POINT LOOPS (a face detector's eye, lip or
//! brow outline), the KERNEL under `ffi_landmark`. It names no document type.
//!
//! **Request.** A region is three lists of loops — `include`, `exclude` and
//! `clip` — and its coverage is `include × clip × (1 − exclude)`, each list
//! the union of its loops. The request's coverage is the union (per-sample
//! max) of its regions. Every coordinate is canvas continuous space, as in
//! `path_geometry` (the top-left corner of pixel `(x, y)` is `(x, y)`). A
//! loop is CLOSED (at least three points), or an open LINE with a
//! `half_width`, whose region is the band of that half-width around it
//! ([`band_outline`]).
//!
//! **Curve.** Each closed loop becomes one closed subpath: the centripetal
//! Catmull–Rom spline (α = 0.5) through every point ([`spline_subpath`]),
//! written as cubic Bézier anchors. Landmark points come unevenly spaced —
//! the two ends of a band outline sit a band's width apart beside long runs,
//! and a blinking eye's upper and lower lids nearly touch — and the uniform
//! spline overshoots on such spacing into cusps and small self-loops, which
//! fill as spurs or holes. The centripetal parameterization provably has
//! neither (Yuksel, Schaefer & Keyser, "Parameterization and applications of
//! Catmull–Rom curves", 2011) and still passes through every landmark.
//!
//! **Coverage** is `PathShape::coverage` (`artwork_coverage::rasterize`, the
//! crate's one analytic area rasterizer) under the nonzero rule, each loop
//! first turned to positive signed area so a list's coverage is the union of
//! its loops whichever way the caller wound them. A region is rasterized over
//! the integer box of its include loops' control points, clipped to the
//! canvas: the curve lies inside its control hull, so nothing outside the box
//! can be covered. Planes combine by [`product`], the crate's rounding
//! convention for a coverage product.
//!
//! **Degenerate input.** Points closer than [`MERGE_DISTANCE`] to the one
//! before them are merged (cyclically for a closed loop), so a repeated
//! closing point adds no zero-length segment. A closed loop left with fewer
//! than three distinct points, or a band with fewer than two, covers
//! nothing: a region with no such include loop, or whose box misses the
//! canvas, is skipped; an exclude loop of that kind excludes nothing; a clip
//! list made only of them covers nothing, so its region is skipped. Fewer RAW
//! points than that is a malformed request and is refused.

use serde_json::Value;

use crate::job_control::JobContext;
use crate::path_geometry::{
    coverage_scratch as path_coverage_scratch, Anchor, FillRule, PathShape, Subpath, MAX_COORDINATE,
};
use crate::precision_image::{pixel_count, reserved_vec};

/// The most regions one request may hold: seven parts of a few dozen faces.
pub const MAX_LANDMARK_REGIONS: usize = 1_024;
/// The most loops in one region's include, exclude or clip list.
pub const MAX_REGION_LOOPS: usize = 16;
/// The most points one loop may hold. A face detector's densest outline is
/// under a hundred points.
pub const MAX_LOOP_POINTS: usize = 1_024;
/// The most points over the whole request. A band doubles its points and a
/// spline anchor is three rasterizer points, so one region's include list at
/// its caps (16 × 2,048 × 3) stays inside `artwork_coverage::MAX_PATH_POINTS`.
pub const MAX_LANDMARK_POINTS: usize = 262_144;

/// Points closer than this, in pixels, are one point: far below anything a
/// detector resolves, and far enough above zero that every knot interval of
/// the spline (its square root, 1e-3) is a safe divisor.
const MERGE_DISTANCE: f64 = 1e-6;

/// One loop of a region: a closed outline (`half_width` `None`), or an open
/// line whose region is the band of `half_width` around it.
#[derive(Clone, Debug, PartialEq)]
pub struct LandmarkLoop {
    pub points: Vec<[f64; 2]>,
    pub half_width: Option<f64>,
}

/// One region: `include × clip × (1 − exclude)`, each list a union.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LandmarkRegion {
    pub include: Vec<LandmarkLoop>,
    pub exclude: Vec<LandmarkLoop>,
    pub clip: Vec<LandmarkLoop>,
}

/// The product of two u16 coverages, rounded to nearest — the convention of
/// every coverage product in the crate (`scoped_auto::mul` is the same
/// expression; `ffi_path::dr_path_coverage`'s clip uses this one).
pub(crate) fn product(a: u16, b: u16) -> u16 {
    ((u32::from(a) * u32::from(b) + 32767) / 65535) as u16
}

/// The working memory [`landmark_coverage`] may hold over a `width` x
/// `height` canvas beyond the request: the rasterizer's scratch and the plane
/// it answers (`path_geometry::coverage_scratch`), plus the other two of a
/// region's three planes and the canvas output.
pub fn coverage_scratch(width: u32, height: u32) -> u64 {
    path_coverage_scratch(width, height) + 3 * u64::from(width) * u64::from(height) * 2
}

fn coordinate(v: &Value) -> Result<f64, String> {
    let v = v.as_f64().ok_or("a landmark coordinate must be a number")?;
    if !v.is_finite() || v.abs() > MAX_COORDINATE {
        return Err(format!(
            "a landmark coordinate is not finite or is outside ±{MAX_COORDINATE}"
        ));
    }
    Ok(v)
}

fn point(v: &Value) -> Result<[f64; 2], String> {
    match v.as_array().map(Vec::as_slice) {
        Some([x, y]) => Ok([coordinate(x)?, coordinate(y)?]),
        _ => Err("a landmark point must be an [x, y] pair".into()),
    }
}

fn landmark_loop(v: &Value, total: &mut usize) -> Result<LandmarkLoop, String> {
    let object = v.as_object().ok_or("a landmark loop must be an object")?;
    let half_width = match object.get("half_width") {
        None => None,
        Some(h) => {
            let h = h
                .as_f64()
                .filter(|h| h.is_finite() && *h > 0.0 && *h <= MAX_COORDINATE)
                .ok_or("a landmark loop's half_width must be a positive number")?;
            Some(h)
        }
    };
    let list = object
        .get("points")
        .and_then(Value::as_array)
        .ok_or("a landmark loop needs a points array")?;
    if list.len() > MAX_LOOP_POINTS {
        return Err(format!(
            "too many points in a landmark loop (max {MAX_LOOP_POINTS})"
        ));
    }
    let least = if half_width.is_some() { 2 } else { 3 };
    if list.len() < least {
        return Err(if half_width.is_some() {
            "a landmark line needs at least two points".into()
        } else {
            "a landmark loop needs at least three points".into()
        });
    }
    *total += list.len();
    if *total > MAX_LANDMARK_POINTS {
        return Err(format!(
            "too many landmark points (max {MAX_LANDMARK_POINTS})"
        ));
    }
    Ok(LandmarkLoop {
        points: list.iter().map(point).collect::<Result<_, _>>()?,
        half_width,
    })
}

fn loops(
    region: &serde_json::Map<String, Value>,
    key: &str,
    total: &mut usize,
) -> Result<Vec<LandmarkLoop>, String> {
    let Some(value) = region.get(key) else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| format!("a landmark region's {key} must be an array"))?;
    if list.len() > MAX_REGION_LOOPS {
        return Err(format!(
            "too many loops in a landmark region's {key} (max {MAX_REGION_LOOPS})"
        ));
    }
    list.iter().map(|v| landmark_loop(v, total)).collect()
}

/// Reads the request JSON
/// `{"regions":[{"include":[LOOP…],"exclude":[LOOP…],"clip":[LOOP…]}]}`,
/// where `LOOP` is `{"points":[[x,y],…]}` or
/// `{"points":[[x,y],…],"half_width":h}`. Refuses malformed JSON, a missing
/// or empty `include`, a loop with fewer than three points (two for a line),
/// a coordinate that is not finite or is past `MAX_COORDINATE`, a
/// `half_width` that is not a positive number, and any cap; every count is
/// checked against its array's length before the array is read.
pub fn regions_from_json(text: &str) -> Result<Vec<LandmarkRegion>, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| format!("landmark regions json is malformed: {e}"))?;
    let list = value
        .as_object()
        .and_then(|o| o.get("regions"))
        .and_then(Value::as_array)
        .ok_or("landmark regions json needs a regions array")?;
    if list.len() > MAX_LANDMARK_REGIONS {
        return Err(format!(
            "too many landmark regions (max {MAX_LANDMARK_REGIONS})"
        ));
    }
    let mut total = 0usize;
    list.iter()
        .map(|entry| {
            let region = entry
                .as_object()
                .ok_or("a landmark region must be an object")?;
            if !region.get("include").is_some_and(Value::is_array) {
                return Err("a landmark region needs an include array".to_string());
            }
            let include = loops(region, "include", &mut total)?;
            if include.is_empty() {
                return Err("a landmark region's include must not be empty".into());
            }
            Ok(LandmarkRegion {
                include,
                exclude: loops(region, "exclude", &mut total)?,
                clip: loops(region, "clip", &mut total)?,
            })
        })
        .collect()
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The points with each one closer than [`MERGE_DISTANCE`] to the last kept
/// one dropped; `cyclic` also drops trailing points that close onto the first.
fn merged(points: &[[f64; 2]], cyclic: bool) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last().is_none_or(|&q| distance(p, q) >= MERGE_DISTANCE) {
            out.push(p);
        }
    }
    if cyclic {
        while out.len() > 1 && distance(out[0], out[out.len() - 1]) < MERGE_DISTANCE {
            out.pop();
        }
    }
    out
}

/// Twice the signed (shoelace) area of the closed polygon through `points`.
fn signed_area2(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % n];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// The closed outline of the band of `half_width` around the open line
/// through `points`: each point offset by `half_width` along its normal —
/// the normal of the central difference `P[i+1] − P[i−1]`, one-sided at the
/// two ends — the `+` side in order, then the `−` side reversed. Points must
/// be pairwise distinct from their neighbours (two at least); a central
/// difference of zero (a line doubling straight back) falls back to the
/// forward one.
pub fn band_outline(points: &[[f64; 2]], half_width: f64) -> Vec<[f64; 2]> {
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    let normal = |i: usize| {
        let before = points[i.saturating_sub(1)];
        let after = points[(i + 1).min(n - 1)];
        let mut t = [after[0] - before[0], after[1] - before[1]];
        let mut length = t[0].hypot(t[1]);
        if length < MERGE_DISTANCE {
            let (a, b) = if i + 1 < n {
                (points[i], points[i + 1])
            } else {
                (points[i - 1], points[i])
            };
            t = [b[0] - a[0], b[1] - a[1]];
            length = t[0].hypot(t[1]).max(MERGE_DISTANCE);
        }
        [-t[1] / length, t[0] / length]
    };
    let mut outline = Vec::with_capacity(2 * n);
    let normals: Vec<[f64; 2]> = (0..n).map(normal).collect();
    for (p, m) in points.iter().zip(&normals) {
        outline.push([p[0] + half_width * m[0], p[1] + half_width * m[1]]);
    }
    for (p, m) in points.iter().zip(&normals).rev() {
        outline.push([p[0] - half_width * m[0], p[1] - half_width * m[1]]);
    }
    outline
}

/// The closed centripetal Catmull–Rom spline through `points` (in order,
/// wrapping) as a closed subpath of cubic anchors, one per point. Segment
/// `P1 → P2`, with neighbours `P0` and `P3`, has knot intervals
/// `dᵢ = |Pᵢ₊₁ − Pᵢ|^½` and end tangents (per unit knot)
/// `M1 = (P1−P0)/d0 − (P2−P0)/(d0+d1) + (P2−P1)/d1` and
/// `M2 = (P2−P1)/d1 − (P3−P1)/(d1+d2) + (P3−P2)/d2`, which are the ends of
/// the Barry–Goldman pyramid's derivative; over an interval of length `d1`
/// the Bézier handles are `P1 + M1·d1/3` and `P2 − M2·d1/3`. Points must be
/// distinct from their cyclic neighbours (every `dᵢ` positive); fewer than
/// three answers an empty subpath.
pub fn spline_subpath(points: &[[f64; 2]]) -> Subpath {
    let n = points.len();
    if n < 3 {
        return Subpath {
            anchors: Vec::new(),
            closed: true,
        };
    }
    let at = |i: usize| points[i % n];
    // d[i] is the knot interval of segment i → i + 1.
    let d: Vec<f64> = (0..n).map(|i| distance(at(i), at(i + 1)).sqrt()).collect();
    // The tangent at point i, between segments i − 1 and i.
    let tangent = |i: usize| {
        let (p0, p1, p2) = (at(i + n - 1), at(i), at(i + 1));
        let (d0, d1) = (d[(i + n - 1) % n], d[i]);
        [0, 1].map(|k| (p1[k] - p0[k]) / d0 - (p2[k] - p0[k]) / (d0 + d1) + (p2[k] - p1[k]) / d1)
    };
    let tangents: Vec<[f64; 2]> = (0..n).map(tangent).collect();
    let anchors = (0..n)
        .map(|i| {
            let p = at(i);
            let m = tangents[i];
            let before = d[(i + n - 1) % n] / 3.0;
            let after = d[i] / 3.0;
            Anchor {
                point: p,
                handle_in: [p[0] - m[0] * before, p[1] - m[1] * before],
                handle_out: [p[0] + m[0] * after, p[1] + m[1] * after],
                smooth: true,
            }
        })
        .collect();
    Subpath {
        anchors,
        closed: true,
    }
}

/// The loop's subpath, or `None` when it is degenerate (the module doc).
fn loop_subpath(l: &LandmarkLoop) -> Option<Subpath> {
    let mut points = match l.half_width {
        Some(h) => {
            let line = merged(&l.points, false);
            if line.len() < 2 {
                return None;
            }
            merged(&band_outline(&line, h), true)
        }
        None => merged(&l.points, true),
    };
    if points.len() < 3 {
        return None;
    }
    if signed_area2(&points) < 0.0 {
        points.reverse();
    }
    Some(spline_subpath(&points))
}

/// The nonzero union of a list's non-degenerate loops; `None` when every
/// loop is degenerate (or the list is empty).
fn list_shape(list: &[LandmarkLoop]) -> Option<PathShape> {
    let subpaths: Vec<Subpath> = list.iter().filter_map(loop_subpath).collect();
    (!subpaths.is_empty()).then_some(PathShape {
        subpaths,
        fill_rule: FillRule::NonZero,
    })
}

/// The integer box `(x0, y0, x1, y1)` of the shape's control points clipped
/// to the canvas, or `None` when it covers no pixel of it.
fn window(shape: &PathShape, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let [min_x, min_y, max_x, max_y] = shape.bounds()?;
    let clamp = |v: f64, side: u32| v.clamp(0.0, f64::from(side)) as u32;
    let (x0, x1) = (clamp(min_x.floor(), width), clamp(max_x.ceil(), width));
    let (y0, y1) = (clamp(min_y.floor(), height), clamp(max_y.ceil(), height));
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

/// The u16 coverage (65535 = covered) of `regions` over the `width` x
/// `height` canvas, row-major: the per-sample max of each region's
/// `include × clip × (65535 − exclude)` (the module doc). An empty list
/// covers nothing. `Err` is a zero or oversized canvas, the rasterizer's own
/// refusals (its edge and work caps), an allocation failure or a
/// cancellation; the caller admits [`coverage_scratch`] first.
pub(crate) fn landmark_coverage(
    regions: &[LandmarkRegion],
    width: u32,
    height: u32,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let n = pixel_count(width, height)?;
    let mut output = reserved_vec(n)?;
    output.resize(n, 0u16);
    let count = regions.len().max(1) as f64;
    for (index, region) in regions.iter().enumerate() {
        job.check()?;
        let job = job.range(index as f64 / count, (index + 1) as f64 / count);
        let Some(include) = list_shape(&region.include) else {
            continue;
        };
        let Some((x0, y0, x1, y1)) = window(&include, width, height) else {
            continue;
        };
        let clip = if region.clip.is_empty() {
            None
        } else {
            match list_shape(&region.clip) {
                Some(shape) => Some(shape),
                None => continue,
            }
        };
        let exclude = list_shape(&region.exclude);
        let (w, h) = (x1 - x0, y1 - y0);
        let origin = (x0 as i32, y0 as i32);
        let mut plane = include.coverage(origin, w, h, true, job.range(0.0, 1.0 / 3.0))?;
        if let Some(clip) = clip {
            let keep = clip.coverage(origin, w, h, true, job.range(1.0 / 3.0, 2.0 / 3.0))?;
            for (sample, &k) in plane.iter_mut().zip(&keep) {
                *sample = product(*sample, k);
            }
        }
        if let Some(exclude) = exclude {
            let cut = exclude.coverage(origin, w, h, true, job.range(2.0 / 3.0, 1.0))?;
            for (sample, &c) in plane.iter_mut().zip(&cut) {
                *sample = product(*sample, u16::MAX - c);
            }
        }
        let (w, x0, y0) = (w as usize, x0 as usize, y0 as usize);
        for (row, line) in plane.chunks_exact(w).enumerate() {
            let start = (y0 + row) * width as usize + x0;
            for (out, &v) in output[start..start + w].iter_mut().zip(line) {
                *out = (*out).max(v);
            }
        }
    }
    job.check()?;
    Ok(output)
}
