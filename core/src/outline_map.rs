//! OUTLINE MAPS — an outline in `artwork_coverage`'s command form (verbs 0
//! move, 1 line, 2 quadratic, 3 cubic, 4 close) sent through a NON-LINEAR
//! point map and returned as moves, lines and closes, ready for
//! `dr_artwork_path_coverage`. Text on a path bends its underline and
//! strikethrough this way. A kernel over `path_along`; it names no document
//! type.
//!
//! **Subdivision.** A non-linear map bends straight lines, so every segment,
//! lines included, is walked by its own parameter: a piece `[t0, t1]` is
//! emitted as the chord between its two mapped ends once the mapped midpoint
//! and both mapped quarter points lie within `tolerance` of that chord
//! (`artwork_coverage::distance`, to the chord SEGMENT), and is split in half
//! otherwise. A line is always split at least once, since three of its points
//! can be collinear under a map that still bends it between them. The depth
//! is capped at [`MAP_DEPTH`], where a piece is emitted whatever its
//! flatness. Every emitted vertex is the map of a point on the source
//! segment.
//!
//! **Closure.** A subpath left open (a move or the end with no close) is
//! closed here by the mapped straight line back to its start and a close
//! verb, which is the closure a fill adds, so the output never relies on a
//! straight implicit close of a bent outline.

use crate::artwork_coverage::{distance, MAX_PATH_POINTS};
use crate::path_along::Along;
use crate::path_geometry::MIN_FLATTEN_TOLERANCE;

/// The subdivision depth at which a piece is emitted as its chord whatever
/// its flatness (2²⁰ pieces of one segment, far past the output cap).
pub const MAP_DEPTH: u8 = 20;

type Point = [f64; 2];

/// A non-linear point map.
pub enum OutlineMap<'a> {
    /// Text on a path: source x is a distance along `along` from `start`,
    /// source y is measured down from `baseline` (layout space, y down), and
    /// `shift` raises the result along the normal. A point maps to
    /// `F.point + up·(baseline − y + shift)` with `F` the extended frame at
    /// `start + x` and `up = (t.y, −t.x)`, the tangent turned a quarter
    /// toward screen-up for a path running left to right.
    Along {
        along: &'a Along,
        start: f64,
        baseline: f64,
        shift: f64,
    },
}

impl OutlineMap<'_> {
    /// The image of one point.
    pub fn point(&self, p: [f64; 2]) -> [f64; 2] {
        match self {
            OutlineMap::Along {
                along,
                start,
                baseline,
                shift,
            } => {
                let frame = along.frame_extended(start + p[0]);
                let t = frame.tangent;
                let rise = baseline - p[1] + shift;
                [frame.point[0] + t[1] * rise, frame.point[1] - t[0] * rise]
            }
        }
    }
}

/// One source segment as a function of its parameter.
#[derive(Clone, Copy)]
enum Segment {
    Line(Point, Point),
    Cubic([Point; 4]),
}

impl Segment {
    fn at(self, t: f64) -> Point {
        match self {
            Segment::Line(a, b) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
            Segment::Cubic(p) => {
                let s = 1.0 - t;
                let w = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
                [0, 1].map(|axis| (0..4).map(|k| w[k] * p[k][axis]).sum())
            }
        }
    }
}

/// The mapped outline under construction.
struct Mapped<'m, 'a> {
    map: &'m OutlineMap<'a>,
    tolerance: f64,
    verbs: Vec<u8>,
    points: Vec<Point>,
}

impl Mapped<'_, '_> {
    fn emit(&mut self, verb: u8, point: Option<Point>) -> Result<(), String> {
        if self.verbs.len() >= MAX_PATH_POINTS
            || (point.is_some() && self.points.len() >= MAX_PATH_POINTS)
        {
            return Err(format!(
                "the mapped outline exceeds {MAX_PATH_POINTS} points; use a larger tolerance"
            ));
        }
        self.verbs.push(verb);
        if let Some(p) = point {
            self.points.push(p);
        }
        Ok(())
    }

    /// Appends the piece `[t0, t1]` of `segment` WITHOUT its first point,
    /// `m0` and `m1` being the maps of its ends.
    fn piece(
        &mut self,
        segment: Segment,
        (t0, t1): (f64, f64),
        (m0, m1): (Point, Point),
        depth: u8,
    ) -> Result<(), String> {
        let mid = (t0 + t1) * 0.5;
        let m_mid = self.map.point(segment.at(mid));
        let must_split = depth == 0 && matches!(segment, Segment::Line(..));
        let flat = !must_split
            && [0.25, 0.75]
                .into_iter()
                .map(|q| self.map.point(segment.at(t0 + (t1 - t0) * q)))
                .chain([m_mid])
                .all(|m| distance(m, m0, m1) <= self.tolerance);
        if flat || depth >= MAP_DEPTH {
            return self.emit(1, Some(m1));
        }
        self.piece(segment, (t0, mid), (m0, m_mid), depth + 1)?;
        self.piece(segment, (mid, t1), (m_mid, m1), depth + 1)
    }

    fn segment(&mut self, segment: Segment, from: Point, to: Point) -> Result<(), String> {
        let ends = (self.map.point(from), self.map.point(to));
        self.piece(segment, (0.0, 1.0), ends, 0)
    }

    /// Closes the current subpath: the mapped line back to `start` when the
    /// pen is elsewhere, then a close verb.
    fn close(&mut self, current: Point, start: Point) -> Result<(), String> {
        if current != start {
            self.segment(Segment::Line(current, start), current, start)?;
        }
        self.emit(4, None)
    }
}

/// `commands` and `points` (the verbs and point counts of
/// `artwork_coverage::outline`) through `map`, as verbs 0 move, 1 line and 4
/// close and their points, each piece within `tolerance` pixels of the mapped
/// curve at its midpoint and quarter points (the module doc). `Err` for an
/// unknown verb, a point count that does not match the verbs, a coordinate
/// that is not finite or past ±10⁹, more than `MAX_PATH_POINTS` verbs or
/// points in or out, and a tolerance that is not positive and finite (a
/// smaller one is raised to `MIN_FLATTEN_TOLERANCE`).
pub fn map_outline(
    commands: &[u8],
    points: &[[f64; 2]],
    map: &OutlineMap<'_>,
    tolerance: f64,
) -> Result<(Vec<u8>, Vec<[f64; 2]>), String> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err("an outline map tolerance must be a positive number".into());
    }
    if commands.len() > MAX_PATH_POINTS
        || points.len() > MAX_PATH_POINTS
        || points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
    {
        return Err("artwork path exceeds its finite geometry bounds".into());
    }
    let mut out = Mapped {
        map,
        tolerance: tolerance.max(MIN_FLATTEN_TOLERANCE),
        verbs: Vec::new(),
        points: Vec::new(),
    };
    let mut index = 0;
    let mut start: Option<Point> = None;
    let mut current: Option<Point> = None;
    // Whether the subpath since the last move has drawn since its last close.
    let mut open = false;
    for &command in commands {
        let n = match command {
            0 | 1 => 1,
            2 => 2,
            3 => 3,
            4 => 0,
            _ => return Err("unknown artwork path command".into()),
        };
        let end = index + n;
        let p = points
            .get(index..end)
            .ok_or("artwork path point count mismatch")?;
        let pen = current.ok_or("artwork path needs a move command");
        match command {
            0 => {
                if let (Some(a), Some(b), true) = (current, start, open) {
                    out.close(a, b)?;
                }
                out.emit(0, Some(map.point(p[0])))?;
                start = Some(p[0]);
                current = start;
                open = false;
            }
            1 => {
                let a = pen?;
                out.segment(Segment::Line(a, p[0]), a, p[0])?;
                current = Some(p[0]);
                open = true;
            }
            2 => {
                // Raised to a cubic exactly as `artwork_coverage::outline`
                // does (the master copy of this elevation).
                let a = pen?;
                let b = [
                    a[0] + (p[0][0] - a[0]) * 2.0 / 3.0,
                    a[1] + (p[0][1] - a[1]) * 2.0 / 3.0,
                ];
                let c = [
                    p[1][0] + (p[0][0] - p[1][0]) * 2.0 / 3.0,
                    p[1][1] + (p[0][1] - p[1][1]) * 2.0 / 3.0,
                ];
                out.segment(Segment::Cubic([a, b, c, p[1]]), a, p[1])?;
                current = Some(p[1]);
                open = true;
            }
            3 => {
                let a = pen?;
                out.segment(Segment::Cubic([a, p[0], p[1], p[2]]), a, p[2])?;
                current = Some(p[2]);
                open = true;
            }
            4 => {
                let a = pen?;
                let b = start.ok_or("artwork path needs an origin")?;
                out.close(a, b)?;
                current = start;
                open = false;
            }
            _ => unreachable!(),
        }
        index = end;
    }
    if index != points.len() {
        return Err("artwork path has unused points".into());
    }
    if let (Some(a), Some(b), true) = (current, start, open) {
        out.close(a, b)?;
    }
    Ok((out.verbs, out.points))
}
