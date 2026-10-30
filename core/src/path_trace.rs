//! PATH TRACING — a u16 coverage plane to a Bézier path: Make Work
//! Path from a selection. A kernel: it names no document type, and the one
//! caller is `ffi_path::dr_path_trace`.
//!
//! Three steps, each bounded by the caller's `tolerance` in pixels.
//!
//! **1. The half-coverage contour.** A sample sits at its pixel's centre
//! (`x + 0.5`, `y + 0.5`) and the plane is ringed with zeros, so every
//! contour closes. Marching squares over the cells between four neighbouring
//! samples finds where the coverage crosses one half; a crossing is placed by
//! LINEAR interpolation between its two samples, so an anti-aliased edge is
//! traced where the half level actually falls and a hard edge on the pixel
//! boundary. A sample is inside at 32768 and above — the cut
//! `path_geometry`'s hard-edged coverage makes — and the level interpolated
//! to is 32767.5, which no sample equals, so a crossing never lands on a
//! sample and no contour is degenerate. A cell whose two diagonals disagree
//! (a saddle) joins the pair whose mean of four samples is inside. Contours
//! are followed with the inside on one fixed side, so each crossing is left
//! by exactly one cell and entered by its neighbour: every contour is a
//! closed loop and is visited once.
//!
//! **2. Douglas–Peucker** at `tolerance` reduces a loop to the vertices a
//! polygon needs; they are where a corner can be. The contour turns a CORNER
//! where the chord over the last [`window`] pixels of arc and the chord over
//! the next differ by more than 45°: a circle of radius above 1.3 windows
//! turns less than that and stays smooth, a right angle does not. Each
//! vertex is moved to the sharpest turn within a window of it before it is
//! judged, because the reduction only lands near a corner.
//!
//! **3. Schneider's cubic fit** ("An Algorithm for Automatically Fitting
//! Digitized Curves", Graphics Gems 1990) runs between consecutive corners
//! over the contour's own points, not the reduced polygon, so the error it
//! bounds is the distance to what was traced: every contour point lies within
//! `tolerance` of the fitted curve at its parameter. A run whose points all
//! lie within `tolerance` of its chord becomes a straight segment. A loop
//! with no corner is one smooth closed run from its first vertex. The fit
//! and the point helpers both steps use live in `path_fit`.
//!
//! The result is closed subpaths under the even-odd rule, so a hole is the
//! loop inside its outline. A loop whose box is no larger than `tolerance` on
//! either side is dropped: it is smaller than the error the caller allowed.
//! Rasterizing the result at the same size reproduces the half-coverage
//! region to within `tolerance` plus the pixel the contour is sampled at.

use crate::artwork_coverage::distance;
use crate::job_control::JobContext;
use crate::path_fit::{dot, fit, length, sub, unit, Cubic, Point};
use crate::path_geometry::{Anchor, FillRule, PathShape, Subpath, MAX_PATH_ANCHORS, MAX_SUBPATHS};
use crate::precision_image::{pixel_count, reserved_vec};

/// The tightest tolerance a trace accepts, in pixels. Below a quarter pixel
/// the fit follows the sampling grid's own stair steps rather than the shape.
pub const MIN_TRACE_TOLERANCE: f64 = 0.25;
/// The loosest tolerance a trace accepts, in pixels.
pub const MAX_TRACE_TOLERANCE: f64 = 64.0;
/// The most contour points one trace follows, over every loop. A selection's
/// outline is a few points per pixel of its length, so this is an outline
/// about a million pixels long — far past any drawn or grown selection, and
/// reached only by a plane of scattered single pixels (a noisy colour
/// range), which has no useful path. It bounds the working memory
/// `ffi_path` charges and the time of the fit.
pub const MAX_CONTOUR_POINTS: usize = 2_097_152;

/// A sample at or above this is inside (one half, rounded up).
const INSIDE: u16 = 32768;
/// The level a crossing is interpolated to: between 32767 and [`INSIDE`].
const LEVEL: f64 = 32767.5;
/// The cosine of the largest turn a smooth vertex may make (45°).
const CORNER_COSINE: f64 = std::f64::consts::FRAC_1_SQRT_2;

const TOP: u8 = 0;
const RIGHT: u8 = 1;
const BOTTOM: u8 = 2;
const LEFT: u8 = 3;

/// The arc length a tangent or a corner is judged over: long enough to
/// average the grid's half-pixel bevels and stair steps, and never shorter
/// than the error allowed.
pub(crate) fn window(tolerance: f64) -> f64 {
    tolerance.max(2.0)
}

/// The coverage plane ringed with zeros. Ringed sample `(sx, sy)` is plane
/// sample `(sx − 1, sy − 1)` and sits at canvas point `(sx − 0.5, sy − 0.5)`.
struct Grid<'a> {
    coverage: &'a [u16],
    width: usize,
    height: usize,
}

impl Grid<'_> {
    fn value(&self, sx: usize, sy: usize) -> u16 {
        if sx == 0 || sy == 0 || sx > self.width || sy > self.height {
            0
        } else {
            self.coverage[(sy - 1) * self.width + sx - 1]
        }
    }

    /// The four corners of cell `(cx, cy)` — top-left, top-right,
    /// bottom-right, bottom-left — as inside bits 1, 2, 4, 8, and whether
    /// their mean is inside (the saddle rule).
    fn cell(&self, cx: usize, cy: usize) -> (u8, bool) {
        let corners = [
            self.value(cx, cy),
            self.value(cx + 1, cy),
            self.value(cx + 1, cy + 1),
            self.value(cx, cy + 1),
        ];
        let mut mask = 0;
        let mut sum = 0u32;
        for (bit, value) in corners.into_iter().enumerate() {
            mask |= u8::from(value >= INSIDE) << bit;
            sum += u32::from(value);
        }
        (mask, sum >= 4 * u32::from(INSIDE))
    }

    /// The two ringed samples a side of cell `(cx, cy)` joins.
    fn ends(cx: usize, cy: usize, side: u8) -> [(usize, usize); 2] {
        match side {
            TOP => [(cx, cy), (cx + 1, cy)],
            RIGHT => [(cx + 1, cy), (cx + 1, cy + 1)],
            BOTTOM => [(cx, cy + 1), (cx + 1, cy + 1)],
            _ => [(cx, cy), (cx, cy + 1)],
        }
    }

    /// Where the half level falls on a crossed side.
    fn crossing(&self, cx: usize, cy: usize, side: u8) -> Point {
        let [a, b] = Self::ends(cx, cy, side);
        let (va, vb) = (
            f64::from(self.value(a.0, a.1)),
            f64::from(self.value(b.0, b.1)),
        );
        // One sample is at most 32767 and the other at least 32768.
        let t = (LEVEL - va) / (vb - va);
        let at = |s: (usize, usize)| [s.0 as f64 - 0.5, s.1 as f64 - 0.5];
        let (pa, pb) = (at(a), at(b));
        [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t]
    }

    /// One bit per grid edge, so a loop is followed once: the edge leaving
    /// ringed sample `(x, y)` rightwards is bit `2·(y·W + x)`, downwards the
    /// bit after it.
    fn edge_bit(&self, cx: usize, cy: usize, side: u8) -> usize {
        let [a, _] = Self::ends(cx, cy, side);
        2 * (a.1 * (self.width + 2) + a.0) + usize::from(side == RIGHT || side == LEFT)
    }
}

/// The bytes of the trace's visited-edge bit set: two bits for each sample of
/// the ringed plane. `ffi_path` charges it.
pub(crate) fn seen_bytes(width: u32, height: u32) -> u64 {
    (2 * (u64::from(width) + 2) * (u64::from(height) + 2)).div_ceil(8)
}

/// The working memory a trace of a `width` x `height` plane may hold, beyond
/// the caller's plane: the visited-edge bits; 64 bytes for each contour
/// point it can follow (16 for the loop, 16 for its copy as a run, 8 for its
/// parameter, 9 for the reduction's flag and kept index, the rest growth
/// slack) — a cell holds at most two crossings and a trace follows at most
/// [`MAX_CONTOUR_POINTS`]; and 48 MiB for the fitted cubics, the result at
/// `MAX_PATH_ANCHORS` and its JSON (about 1 KiB an anchor while the JSON
/// tree is alive, as `path_native::transient_bound` counts it).
pub(crate) fn scratch_bytes(width: u32, height: u32) -> u64 {
    let cells = (u64::from(width) + 1) * (u64::from(height) + 1);
    let points = (2 * cells).min(MAX_CONTOUR_POINTS as u64);
    seen_bytes(width, height) + points * 64 + 48 * 1024 * 1024
}

/// Whether a contour ENTERS a cell through `side`, under the one orientation
/// every loop is followed in: across a side whose first corner (reading
/// left-to-right or top-to-bottom) is inside and second outside, travel is
/// downwards or leftwards; with the corners the other way round it is
/// upwards or rightwards.
fn enters(mask: u8, side: u8) -> bool {
    let (tl, tr, br, bl) = (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0, mask & 8 != 0);
    match side {
        TOP => tl && !tr,
        RIGHT => tr && !br,
        BOTTOM => !bl && br,
        _ => !tl && bl,
    }
}

/// The side a contour that entered through `entry` leaves by. A cell with
/// one inside diagonal (masks 5 and 10) holds two contours and joins them
/// through its centre when the centre is inside.
fn exit_of(mask: u8, centre: bool, entry: u8) -> u8 {
    match (mask, centre, entry) {
        (5, true, TOP) | (5, false, BOTTOM) => RIGHT,
        (5, _, _) => LEFT,
        (10, true, LEFT) | (10, false, RIGHT) => TOP,
        (10, _, _) => BOTTOM,
        _ => {
            let (tl, tr, br, bl) = (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0, mask & 8 != 0);
            if !tl && tr {
                TOP
            } else if !tr && br {
                RIGHT
            } else if bl && !br {
                BOTTOM
            } else {
                LEFT
            }
        }
    }
}

/// Follows the loop that enters cell `(cx, cy)` through `side`, marking each
/// crossing in `seen`, and returns its points in order (the first is not
/// repeated). `room` is what is left of [`MAX_CONTOUR_POINTS`].
fn follow(
    grid: &Grid<'_>,
    start: (usize, usize, u8),
    seen: &mut [u8],
    room: &mut usize,
    job: JobContext<'_>,
) -> Result<Vec<Point>, String> {
    let mut points = Vec::new();
    let (mut cx, mut cy, mut entry) = start;
    loop {
        job.every(points.len())?;
        if *room == 0 {
            return Err(format!(
                "the outline is too intricate to trace (more than {MAX_CONTOUR_POINTS} contour points)"
            ));
        }
        *room -= 1;
        let bit = grid.edge_bit(cx, cy, entry);
        seen[bit / 8] |= 1 << (bit % 8);
        points.push(grid.crossing(cx, cy, entry));
        let (mask, centre) = grid.cell(cx, cy);
        // A crossed side is never on the ring's outer edge (both of its
        // samples would be the ring's zeros), so the neighbour exists.
        (cx, cy, entry) = match exit_of(mask, centre, entry) {
            TOP => (cx, cy - 1, BOTTOM),
            RIGHT => (cx + 1, cy, LEFT),
            BOTTOM => (cx, cy + 1, TOP),
            _ => (cx - 1, cy, RIGHT),
        };
        if (cx, cy, entry) == start {
            return Ok(points);
        }
    }
}

/// How many steps from `from` along the ring `d` (forwards or backwards) it
/// takes to walk `span` pixels, at most `limit`. A run that is not a ring is
/// walked with a `limit` that stops at its end.
pub(crate) fn steps(d: &[Point], from: usize, forward: bool, span: f64, limit: usize) -> usize {
    let n = d.len();
    let (mut walked, mut at, mut taken) = (0.0, from, 0);
    while taken < limit && walked < span {
        let next = if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        };
        walked += length(sub(d[next], d[at]));
        at = next;
        taken += 1;
    }
    taken
}

/// Douglas–Peucker over the closed loop `ring`: the indices kept, ascending.
/// The loop is opened at its first point and at the point farthest from it,
/// and each half is reduced on its own.
fn reduced(ring: &[Point], tolerance: f64, job: JobContext<'_>) -> Result<Vec<usize>, String> {
    let n = ring.len();
    let far = (1..n)
        .max_by(|&a, &b| length(sub(ring[a], ring[0])).total_cmp(&length(sub(ring[b], ring[0]))))
        .unwrap_or(0);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    // A span's end may be `n`, the first point again.
    let mut spans = vec![(0, far), (far, n)];
    let mut polled = 0usize;
    while let Some((first, last)) = spans.pop() {
        let (a, b) = (ring[first], ring[last % n]);
        let mut worst = (tolerance, None);
        for (i, p) in ring.iter().enumerate().take(last).skip(first + 1) {
            polled += 1;
            job.every(polled)?;
            let d = distance(*p, a, b);
            if d > worst.0 {
                worst = (d, Some(i));
            }
        }
        if let (_, Some(i)) = worst {
            keep[i] = true;
            spans.push((first, i));
            spans.push((i, last));
        }
    }
    Ok((0..n).filter(|&i| keep[i]).collect())
}

/// The cosine of the loop's turn at `at`: the angle between the chord
/// arriving over the last `span` pixels of arc and the one leaving over the
/// next. 1 is straight on.
fn turn_cosine(ring: &[Point], at: usize, span: f64) -> f64 {
    let n = ring.len();
    // Under half the loop each way, so the two chords never share a far end.
    let back = steps(ring, at, false, span, (n - 1) / 2);
    let ahead = steps(ring, at, true, span, (n - 1) / 2);
    let arriving = sub(ring[at], ring[(at + n - back) % n]);
    let leaving = sub(ring[(at + ahead) % n], ring[at]);
    let scale = length(arriving) * length(leaving);
    if scale > 0.0 {
        dot(arriving, leaving) / scale
    } else {
        1.0
    }
}

/// The corners of the loop, ascending and distinct. A reduced vertex only
/// lands NEAR a corner (anywhere within `tolerance` of the polygon will do
/// for Douglas–Peucker), so each vertex walks to the sharpest turn within
/// one `span` of arc either side of it, and again from there until no
/// sharper turn is in reach, and is a corner when that turn is past 45°. Two
/// vertices by one corner therefore land on the same point, and a point a
/// pixel short of a corner (whose own windowed turn can pass 45°) is never
/// taken for a second one.
fn corners(
    ring: &[Point],
    vertices: &[usize],
    span: f64,
    job: JobContext<'_>,
) -> Result<Vec<usize>, String> {
    let n = ring.len();
    let mut found = Vec::new();
    for (index, &vertex) in vertices.iter().enumerate() {
        job.every(index * 64)?;
        let mut at = (turn_cosine(ring, vertex, span), vertex);
        // Each move is to a strictly sharper turn, so the walk ends; the
        // bound only keeps a pathological loop from costing more.
        for _ in 0..16 {
            let back = steps(ring, at.1, false, span, (n - 1) / 2);
            let ahead = steps(ring, at.1, true, span, (n - 1) / 2);
            let sharpest = (0..=back + ahead)
                .map(|k| (at.1 + n - back + k) % n)
                .map(|i| (turn_cosine(ring, i, span), i))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match sharpest {
                Some(next) if next.0 < at.0 => at = next,
                _ => break,
            }
        }
        if at.0 < CORNER_COSINE {
            found.push(at.1);
        }
    }
    found.sort_unstable();
    found.dedup();
    Ok(found)
}

/// One traced loop as a closed subpath, or `None` when it is smaller than
/// `tolerance` on both sides.
fn subpath(ring: &[Point], tolerance: f64, job: JobContext<'_>) -> Result<Option<Subpath>, String> {
    let n = ring.len();
    let extent = |axis: usize| {
        let low = ring.iter().map(|p| p[axis]).fold(f64::MAX, f64::min);
        let high = ring.iter().map(|p| p[axis]).fold(f64::MIN, f64::max);
        high - low
    };
    if n < 3 || (extent(0) <= tolerance && extent(1) <= tolerance) {
        return Ok(None);
    }
    let span = window(tolerance);
    let vertices = reduced(ring, tolerance, job)?;
    let corners = corners(ring, &vertices, span, job)?;

    // The cubics around the loop, and whether each STARTS at a corner.
    let mut cubics: Vec<Cubic> = Vec::new();
    let mut starts_at_corner: Vec<bool> = Vec::new();
    let mut run: Vec<Point> = Vec::new();
    if corners.is_empty() {
        // One smooth closed run from the first vertex round to itself, its
        // two end tangents one line so the join is smooth too.
        let start = vertices[0];
        run.extend((0..=n).map(|i| ring[(start + i) % n]));
        let back = steps(ring, start, false, span / 2.0, n / 2).max(1);
        let ahead = steps(ring, start, true, span / 2.0, n / 2).max(1);
        let tangent = unit(sub(ring[(start + ahead) % n], ring[(start + n - back) % n]))
            .or_else(|| unit(sub(ring[(start + 1) % n], ring[start])))
            .unwrap_or([1.0, 0.0]);
        fit(
            &run,
            tangent,
            [-tangent[0], -tangent[1]],
            tolerance,
            &mut cubics,
            job,
        )?;
        starts_at_corner.resize(cubics.len(), false);
    } else {
        for (k, &from) in corners.iter().enumerate() {
            job.check()?;
            let to = corners[(k + 1) % corners.len()];
            // The run's length in steps; a lone corner runs the whole loop.
            let count = match (to + n - from) % n {
                0 => n,
                c => c,
            };
            let before = cubics.len();
            run.clear();
            run.extend((0..=count).map(|i| ring[(from + i) % n]));
            let last = run.len() - 1;
            let (a, b) = (run[0], run[last]);
            // A lone corner's run returns to it: a chord of no length.
            let straight = count < n && run.iter().all(|p| distance(*p, a, b) <= tolerance);
            if !straight {
                let ahead = steps(&run, 0, true, span, last).max(1);
                let back = steps(&run, last, false, span, last).max(1);
                let t1 = unit(sub(run[ahead], run[0])).unwrap_or([1.0, 0.0]);
                let t2 = unit(sub(run[last - back], run[last])).unwrap_or([-1.0, 0.0]);
                fit(&run, t1, t2, tolerance, &mut cubics, job)?;
            } else {
                cubics.push([a, a, b, b]);
            }
            starts_at_corner.resize(cubics.len(), false);
            if let Some(first) = starts_at_corner.get_mut(before) {
                *first = true;
            }
        }
    }
    let count = cubics.len();
    if count == 0 {
        return Ok(None);
    }
    let anchors = (0..count)
        .map(|k| {
            let (leaving, arriving) = (&cubics[k], &cubics[(k + count - 1) % count]);
            let point = leaving[0];
            let (handle_in, handle_out) = (arriving[2], leaving[1]);
            Anchor {
                point,
                handle_in,
                handle_out,
                smooth: !starts_at_corner[k] && handle_in != point && handle_out != point,
            }
        })
        .collect();
    Ok(Some(Subpath {
        anchors,
        closed: true,
    }))
}

/// Traces the region of `coverage` (row-major, `width` x `height`, one u16
/// per pixel) at or above one half into closed Bézier subpaths under the
/// even-odd rule, each within `tolerance` pixels of the half-coverage
/// contour (the module doc's three steps). A plane with nothing at or above
/// one half answers an empty shape.
///
/// `Err`: a `tolerance` that is not finite or lies outside
/// [`MIN_TRACE_TOLERANCE`]..=[`MAX_TRACE_TOLERANCE`]; a zero, oversized or
/// mis-sized plane; an outline of more than [`MAX_CONTOUR_POINTS`] points;
/// a result of more than `MAX_SUBPATHS` loops or `MAX_PATH_ANCHORS` anchors
/// (the sentence says to raise the tolerance); a cancellation.
pub(crate) fn trace(
    coverage: &[u16],
    width: u32,
    height: u32,
    tolerance: f64,
    job: JobContext<'_>,
) -> Result<PathShape, String> {
    if !tolerance.is_finite() || !(MIN_TRACE_TOLERANCE..=MAX_TRACE_TOLERANCE).contains(&tolerance) {
        return Err(format!(
            "trace tolerance must be between {MIN_TRACE_TOLERANCE} and {MAX_TRACE_TOLERANCE} pixels"
        ));
    }
    if pixel_count(width, height)? != coverage.len() {
        return Err("trace coverage does not match its dimensions".into());
    }
    let grid = Grid {
        coverage,
        width: width as usize,
        height: height as usize,
    };
    let (columns, rows) = (grid.width + 1, grid.height + 1);
    let bits = seen_bytes(width, height) as usize;
    let mut seen: Vec<u8> = reserved_vec(bits)?;
    seen.resize(bits, 0);
    let mut room = MAX_CONTOUR_POINTS;
    let mut subpaths = Vec::new();
    let mut anchors = 0usize;
    for cy in 0..rows {
        job.report(cy as f64 / rows as f64, 6)?;
        for cx in 0..columns {
            let (mask, _) = grid.cell(cx, cy);
            if mask == 0 || mask == 15 {
                continue;
            }
            for side in [TOP, RIGHT, BOTTOM, LEFT] {
                let bit = grid.edge_bit(cx, cy, side);
                if !enters(mask, side) || seen[bit / 8] & (1 << (bit % 8)) != 0 {
                    continue;
                }
                let ring = follow(&grid, (cx, cy, side), &mut seen, &mut room, job)?;
                let Some(traced) = subpath(&ring, tolerance, job)? else {
                    continue;
                };
                anchors += traced.anchors.len();
                if subpaths.len() >= MAX_SUBPATHS || anchors > MAX_PATH_ANCHORS {
                    return Err(format!(
                        "the outline traces to more than {MAX_SUBPATHS} subpaths or \
                         {MAX_PATH_ANCHORS} anchors; raise the tolerance"
                    ));
                }
                subpaths.push(traced);
            }
        }
    }
    job.report(1.0, 6)?;
    PathShape {
        subpaths,
        fill_rule: FillRule::EvenOdd,
    }
    .validated()
}
