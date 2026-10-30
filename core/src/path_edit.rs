//! PATH EDITING GEOMETRY — what the Pen asks of a shape: the hit test
//! ([`PathShape::hit`]), inserting an anchor on a segment
//! ([`PathShape::inserting_anchor`]) and converting an anchor between corner
//! and smooth ([`PathShape::converting_anchor`]). A kernel beside
//! `path_geometry`, which holds the model, [`PathHit`], the caps and the
//! segment numbering these use. It names no document type.

use crate::path_geometry::{
    is_line, lerp, stored, Anchor, PathHit, PathShape, Point, MAX_PATH_ANCHORS,
};

/// How many evenly spaced parameters [`PathShape::hit`] tries on a curved
/// segment before refining around the nearest. A cubic's distance to a point
/// has at most five local minima along the curve, so 32 samples leave each
/// basin several samples wide.
const HIT_SAMPLES: usize = 32;

/// The cubic `p` at `t`, by de Casteljau.
fn cubic_at(p: &[Point; 4], t: f64) -> Point {
    let (a, b, c) = (
        lerp(p[0], p[1], t),
        lerp(p[1], p[2], t),
        lerp(p[2], p[3], t),
    );
    lerp(lerp(a, b, t), lerp(b, c, t), t)
}

/// The nearest candidate of one kind within [`PathShape::hit`]'s tolerance;
/// a tie keeps the earlier one.
struct Nearest {
    tolerance: f64,
    best: Option<(f64, PathHit)>,
}

impl Nearest {
    fn offer(&mut self, distance: f64, hit: PathHit) {
        if distance <= self.tolerance && self.best.is_none_or(|(d, _)| distance < d) {
            self.best = Some((distance, hit));
        }
    }
}

/// The parameter of the point of the cubic `p` nearest `at`, and its
/// distance: the nearest of [`HIT_SAMPLES`] even samples, refined by a
/// ternary search over the two sample intervals around it.
fn nearest_on_cubic(p: &[Point; 4], at: Point) -> (f64, f64) {
    let gap = |t: f64| {
        let q = cubic_at(p, t);
        (q[0] - at[0]).hypot(q[1] - at[1])
    };
    let step = 1.0 / HIT_SAMPLES as f64;
    let mut best = (0.0, gap(0.0));
    for i in 1..=HIT_SAMPLES {
        let t = i as f64 * step;
        let d = gap(t);
        if d < best.1 {
            best = (t, d);
        }
    }
    let (mut low, mut high) = ((best.0 - step).max(0.0), (best.0 + step).min(1.0));
    // Each round keeps two thirds of the interval: 40 rounds take 1/16 to
    // below 1e-8 of the parameter range.
    for _ in 0..40 {
        let (a, b) = (low + (high - low) / 3.0, high - (high - low) / 3.0);
        if gap(a) <= gap(b) {
            high = b;
        } else {
            low = a;
        }
    }
    let t = (low + high) * 0.5;
    let d = gap(t);
    if d < best.1 {
        (t, d)
    } else {
        best
    }
}

impl PathShape {
    /// What lies within `tolerance` pixels of `at`, nearest first within
    /// each kind and the kinds in this order: an ANCHOR's point; with
    /// `handles`, a HANDLE that is off its point (one sitting on its point
    /// cannot be picked apart from the anchor); a SEGMENT, with the
    /// parameter of its nearest point. So an anchor wins over the segments
    /// that end on it. `handles` false never answers a handle: a press that
    /// cannot grab one (the Pen's plain click) must reach the segment a
    /// handle tip lies near. `None` for a miss, a negative or NaN tolerance,
    /// or a non-finite point.
    pub fn hit(&self, at: Point, tolerance: f64, handles: bool) -> PathHit {
        if tolerance.is_nan() || tolerance < 0.0 || !at[0].is_finite() || !at[1].is_finite() {
            return PathHit::None;
        }
        let gap = |p: Point| (p[0] - at[0]).hypot(p[1] - at[1]);
        let mut nearest = Nearest {
            tolerance,
            best: None,
        };
        for (subpath, path) in self.subpaths.iter().enumerate() {
            for (index, a) in path.anchors.iter().enumerate() {
                nearest.offer(gap(a.point), PathHit::Anchor { subpath, index });
            }
        }
        if let Some((_, hit)) = nearest.best {
            return hit;
        }
        let offered = if handles {
            self.subpaths.as_slice()
        } else {
            &[]
        };
        for (subpath, path) in offered.iter().enumerate() {
            for (index, a) in path.anchors.iter().enumerate() {
                if a.handle_in != a.point {
                    nearest.offer(gap(a.handle_in), PathHit::HandleIn { subpath, index });
                }
                if a.handle_out != a.point {
                    nearest.offer(gap(a.handle_out), PathHit::HandleOut { subpath, index });
                }
            }
        }
        if let Some((_, hit)) = nearest.best {
            return hit;
        }
        for (subpath, path) in self.subpaths.iter().enumerate() {
            for index in 0..path.segment_count() {
                let Some((a, b)) = path.segment(index) else {
                    break;
                };
                let curve = [a.point, a.handle_out, b.handle_in, b.point];
                // The curve lies in its control box: one further than
                // `tolerance` from the box cannot be hit.
                let outside = |axis: usize| {
                    let low = curve.iter().map(|p| p[axis]).fold(f64::MAX, f64::min);
                    let high = curve.iter().map(|p| p[axis]).fold(f64::MIN, f64::max);
                    at[axis] < low - tolerance || at[axis] > high + tolerance
                };
                if outside(0) || outside(1) {
                    continue;
                }
                let (t, distance) = if is_line(a, b) {
                    let (dx, dy) = (b.point[0] - a.point[0], b.point[1] - a.point[1]);
                    let length = dx * dx + dy * dy;
                    let t = if length == 0.0 {
                        0.0
                    } else {
                        (((at[0] - a.point[0]) * dx + (at[1] - a.point[1]) * dy) / length)
                            .clamp(0.0, 1.0)
                    };
                    (t, gap(lerp(a.point, b.point, t)))
                } else {
                    nearest_on_cubic(&curve, at)
                };
                nearest.offer(distance, PathHit::Segment { subpath, index, t });
            }
        }
        nearest.best.map_or(PathHit::None, |(_, hit)| hit)
    }

    /// The shape with one more anchor ON segment `segment` of `subpath`, at
    /// parameter `t` ([`PathHit::Segment`]'s), the outline unchanged: a
    /// curved segment is split by de Casteljau — the new anchor is smooth,
    /// and the two anchors it sits between have the handle facing it
    /// shortened — and a straight one gets a corner at that fraction of its
    /// length. `None` (nothing to insert): no such subpath or segment, a `t`
    /// that is NaN or not strictly between 0 and 1, or a shape already at
    /// [`MAX_PATH_ANCHORS`]. The outline is kept to the model's four
    /// decimals.
    pub fn inserting_anchor(&self, subpath: usize, segment: usize, t: f64) -> Option<Self> {
        if t.is_nan() || t <= 0.0 || t >= 1.0 || self.anchor_count() >= MAX_PATH_ANCHORS {
            return None;
        }
        let (a, b) = self.subpaths.get(subpath)?.segment(segment)?;
        let (a, b) = (*a, *b);
        let mut out = self.clone();
        let anchors = &mut out.subpaths[subpath].anchors;
        let next = (segment + 1) % anchors.len();
        let stored_point = |p: Point| [stored(p[0]), stored(p[1])];
        let added = if is_line(&a, &b) {
            Anchor::corner(stored_point(lerp(a.point, b.point, t)))
        } else {
            let (q0, q1, q2) = (
                lerp(a.point, a.handle_out, t),
                lerp(a.handle_out, b.handle_in, t),
                lerp(b.handle_in, b.point, t),
            );
            let (r0, r1) = (lerp(q0, q1, t), lerp(q1, q2, t));
            anchors[segment].handle_out = stored_point(q0);
            anchors[next].handle_in = stored_point(q2);
            Anchor {
                point: stored_point(lerp(r0, r1, t)),
                handle_in: stored_point(r0),
                handle_out: stored_point(r1),
                smooth: true,
            }
        };
        anchors.insert(segment + 1, added);
        Some(out)
    }

    /// The shape with anchor `anchor` of `subpath` converted. To a CORNER
    /// (`smooth` false): both handles go onto the point. To a SMOOTH anchor:
    /// the handles are laid along the line from the previous anchor to the
    /// next one (the neighbours a closed subpath wraps to), each a third of
    /// the way to its neighbour — the usual automatic handle; an end of an
    /// open subpath has one neighbour and gets the one handle facing it, and
    /// when both neighbours are the same point the handles lie across the
    /// line to it. `None`: no such subpath or anchor, an anchor that is
    /// already exactly that (an identical shape would be a phantom edit), or
    /// a smooth request with no neighbour at a different point to aim at.
    pub fn converting_anchor(&self, subpath: usize, anchor: usize, smooth: bool) -> Option<Self> {
        let path = self.subpaths.get(subpath)?;
        let current = *path.anchors.get(anchor)?;
        let p = current.point;
        let converted = if smooth {
            let n = path.anchors.len();
            let wraps = path.closed && n > 1;
            let before = match anchor {
                0 => wraps.then(|| n - 1),
                i => Some(i - 1),
            };
            let after = if anchor + 1 < n {
                Some(anchor + 1)
            } else {
                wraps.then_some(0)
            };
            let previous = before.map(|i| path.anchors[i].point);
            let next = after.map(|i| path.anchors[i].point);
            let (from, to) = (previous.unwrap_or(p), next.unwrap_or(p));
            let mut along = [to[0] - from[0], to[1] - from[1]];
            if along == [0.0, 0.0] {
                // Both neighbours are one point: lie across the line to it.
                let q = previous.or(next)?;
                along = [-(q[1] - p[1]), q[0] - p[0]];
            }
            let length = along[0].hypot(along[1]);
            if length == 0.0 {
                return None;
            }
            let unit = [along[0] / length, along[1] / length];
            let reach =
                |q: Option<Point>| q.map_or(0.0, |q| (q[0] - p[0]).hypot(q[1] - p[1]) / 3.0);
            let (back, ahead) = (reach(previous), reach(next));
            Anchor {
                point: p,
                handle_in: [stored(p[0] - unit[0] * back), stored(p[1] - unit[1] * back)],
                handle_out: [
                    stored(p[0] + unit[0] * ahead),
                    stored(p[1] + unit[1] * ahead),
                ],
                smooth: true,
            }
        } else {
            Anchor::corner(p)
        };
        if converted == current {
            return None;
        }
        let mut out = self.clone();
        out.subpaths[subpath].anchors[anchor] = converted;
        Some(out)
    }
}
