//! PATH FITTING — step 3 of `path_trace`: Schneider's cubic fit ("An
//! Algorithm for Automatically Fitting Digitized Curves", Graphics Gems 1990)
//! of a run of traced contour points between two corners, every point within
//! `tolerance` of the fitted curve at its parameter. It also holds the point
//! helpers both halves use. A kernel: it names no document type; the one
//! caller is `path_trace`, whose module doc describes all three steps and
//! whose `window` and `steps` measure the arc a split tangent is taken over.

use crate::job_control::JobContext;
use crate::path_trace::{steps, window};

/// Newton–Raphson rounds per fit before a run is split (Schneider's 4).
const REPARAMETERIZE_ROUNDS: usize = 4;

pub(crate) type Point = [f64; 2];
pub(crate) type Cubic = [Point; 4];

pub(crate) fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

pub(crate) fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

pub(crate) fn length(a: Point) -> f64 {
    a[0].hypot(a[1])
}

pub(crate) fn along(a: Point, direction: Point, by: f64) -> Point {
    [a[0] + direction[0] * by, a[1] + direction[1] * by]
}

/// `a` at unit length, or `None` for a zero vector.
pub(crate) fn unit(a: Point) -> Option<Point> {
    let l = length(a);
    (l > 0.0).then(|| [a[0] / l, a[1] / l])
}

fn cubic_at(c: &Cubic, t: f64) -> Point {
    let s = 1.0 - t;
    let (b0, b1, b2, b3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
    [
        b0 * c[0][0] + b1 * c[1][0] + b2 * c[2][0] + b3 * c[3][0],
        b0 * c[0][1] + b1 * c[1][1] + b2 * c[2][1] + b3 * c[3][1],
    ]
}

/// Schneider's `GenerateBezier`: the least-squares handle lengths along the
/// two end tangents for the points `d` at parameters `u`. When the system is
/// singular or a length comes out non-positive or longer than the whole
/// chord several times over (an ill-conditioned run), both fall back to a
/// third of the chord, the Wu–Barsky heuristic the paper uses.
fn generate(d: &[Point], u: &[f64], t1: Point, t2: Point) -> Cubic {
    let (first, last) = (d[0], d[d.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, &t) in d.iter().zip(u) {
        let s = 1.0 - t;
        let (b0, b1, b2, b3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
        let (a1, a2) = ([t1[0] * b1, t1[1] * b1], [t2[0] * b2, t2[1] * b2]);
        c00 += dot(a1, a1);
        c01 += dot(a1, a2);
        c11 += dot(a2, a2);
        let rest = [
            p[0] - (first[0] * (b0 + b1) + last[0] * (b2 + b3)),
            p[1] - (first[1] * (b0 + b1) + last[1] * (b2 + b3)),
        ];
        x0 += dot(a1, rest);
        x1 += dot(a2, rest);
    }
    let determinant = c00 * c11 - c01 * c01;
    let chord = length(sub(last, first));
    let (mut left, mut right) = if determinant.abs() > 1e-12 {
        (
            (x0 * c11 - x1 * c01) / determinant,
            (c00 * x1 - c01 * x0) / determinant,
        )
    } else {
        (0.0, 0.0)
    };
    let sound = |v: f64| v.is_finite() && v > 1e-6 * chord && v <= 3.0 * chord.max(1.0);
    if !sound(left) || !sound(right) {
        left = chord / 3.0;
        right = chord / 3.0;
    }
    [first, along(first, t1, left), along(last, t2, right), last]
}

/// The largest squared distance from a point of `d` to the curve at its
/// parameter, and that point's index (never an end).
fn worst(d: &[Point], u: &[f64], c: &Cubic) -> (f64, usize) {
    let mut worst = (0.0, d.len() / 2);
    for i in 1..d.len() - 1 {
        let gap = sub(cubic_at(c, u[i]), d[i]);
        let squared = dot(gap, gap);
        if squared >= worst.0 {
            worst = (squared, i);
        }
    }
    worst
}

/// Schneider's `Reparameterize`: one Newton–Raphson step of each parameter
/// towards the foot of its point on the curve, kept inside `[0, 1]`.
fn reparameterize(d: &[Point], u: &mut [f64], c: &Cubic) {
    let first: [Point; 3] =
        std::array::from_fn(|i| [3.0 * (c[i + 1][0] - c[i][0]), 3.0 * (c[i + 1][1] - c[i][1])]);
    let second: [Point; 2] = std::array::from_fn(|i| {
        [
            2.0 * (first[i + 1][0] - first[i][0]),
            2.0 * (first[i + 1][1] - first[i][1]),
        ]
    });
    for (p, t) in d.iter().zip(u.iter_mut()) {
        let s = 1.0 - *t;
        let q = cubic_at(c, *t);
        let q1 = [
            s * s * first[0][0] + 2.0 * s * *t * first[1][0] + *t * *t * first[2][0],
            s * s * first[0][1] + 2.0 * s * *t * first[1][1] + *t * *t * first[2][1],
        ];
        let q2 = [
            s * second[0][0] + *t * second[1][0],
            s * second[0][1] + *t * second[1][1],
        ];
        let gap = sub(q, *p);
        let denominator = dot(q1, q1) + dot(gap, q2);
        if denominator != 0.0 {
            let next = *t - dot(gap, q1) / denominator;
            if next.is_finite() {
                *t = next.clamp(0.0, 1.0);
            }
        }
    }
}

/// Schneider's `FitCubic` over the run `d`, start tangent `t1` and end
/// tangent `t2` (each pointing INTO the run): the cubics in order, every
/// point of `d` within `tolerance` of its piece. Splits are kept on an
/// explicit stack — a run can be millions of points long.
pub(crate) fn fit(
    d: &[Point],
    t1: Point,
    t2: Point,
    tolerance: f64,
    out: &mut Vec<Cubic>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let allowed = tolerance * tolerance;
    let span = window(tolerance) / 2.0;
    let mut pending = vec![(0usize, d.len() - 1, t1, t2)];
    let mut u = Vec::new();
    while let Some((first, last, t1, t2)) = pending.pop() {
        job.check()?;
        let run = &d[first..=last];
        if run.len() == 2 {
            let third = length(sub(run[1], run[0])) / 3.0;
            out.push([
                run[0],
                along(run[0], t1, third),
                along(run[1], t2, third),
                run[1],
            ]);
            continue;
        }
        // Chord-length parameters.
        u.clear();
        u.push(0.0);
        for pair in run.windows(2) {
            u.push(u[u.len() - 1] + length(sub(pair[1], pair[0])));
        }
        let total = u[u.len() - 1];
        if total == 0.0 {
            out.push([run[0], run[0], run[0], run[0]]);
            continue;
        }
        for t in &mut u {
            *t /= total;
        }
        let mut curve = generate(run, &u, t1, t2);
        let (mut error, mut split) = worst(run, &u, &curve);
        // Within four times the error: better parameters may close the gap.
        if error >= allowed && error < 16.0 * allowed {
            for _ in 0..REPARAMETERIZE_ROUNDS {
                reparameterize(run, &mut u, &curve);
                curve = generate(run, &u, t1, t2);
                (error, split) = worst(run, &u, &curve);
                if error < allowed {
                    break;
                }
            }
        }
        if error < allowed {
            out.push(curve);
            continue;
        }
        // Split at the worst point; the tangent there is taken across it,
        // over half a window each way, so the two pieces join smoothly.
        let at = first + split;
        let back = steps(d, at, false, span, at).max(1);
        let ahead = steps(d, at, true, span, d.len() - 1 - at).max(1);
        let centre = unit(sub(d[at - back], d[at + ahead]))
            .or_else(|| unit(sub(d[at - 1], d[at])))
            .unwrap_or([1.0, 0.0]);
        pending.push((at, last, [-centre[0], -centre[1]], t2));
        pending.push((first, at, t1, centre));
    }
    Ok(())
}
