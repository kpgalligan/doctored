//! DISTANCE ALONG ONE SUBPATH — [`Along`], one subpath's arc length and the
//! FRAME (a point and the unit tangent) at a distance along it: what text on a
//! path places each glyph by, and what `outline_map` bends an outline along.
//! A kernel over `path_flatten` (the master copy of the flattening); it names
//! no document type.
//!
//! **Measure.** The subpath is flattened by [`PathShape::flattened`] at
//! [`ALONG_TOLERANCE`] (1/64 px), its closing segment included when it is
//! closed, and the polyline is measured: lengths are cumulative chord lengths,
//! a frame is the point a linear interpolation reaches on the chord holding
//! that distance and the chord's own direction. Zero-length chords are
//! dropped, so every chord has a direction.
//!
//! **Error.** Every flattened piece lies within τ = [`ALONG_TOLERANCE`] of its
//! chord (the flattener's rule). For a curve whose radius of curvature is at
//! least `r` everywhere, a piece of sagitta `s ≤ τ` subtends a half-angle
//! `θ` with `cos θ = 1 − s/r`, so: a frame's POINT lies within τ of the curve;
//! the measured LENGTH is short of the true one (a chord is never longer than
//! its arc) by at most a factor τ/(3r) to leading order (`1 − sin θ/θ ≈ θ²/6`
//! and `s/r ≈ θ²/2`); and a frame's TANGENT is within `acos(1 − τ/r)` of the
//! curve's tangent at the nearest point. A distance measured along the
//! polyline therefore drifts from the true arc length by at most that factor
//! of the distance itself. A straight segment is exact. For a 100 px radius
//! the length is short by at most 5·10⁻⁵ of itself and the tangent is within
//! 0.8°.
//!
//! **Ends.** A CLOSED subpath wraps: any finite distance is taken modulo the
//! length. An OPEN one has a frame only at distances in `0..=length`; past
//! either end [`Along::frame`] answers none and [`Along::frame_extended`]
//! continues straight along the end chord. `reversed` measures from the other
//! end (a closed subpath still starts at its first anchor and runs the other
//! way round).

use crate::path_geometry::PathShape;

/// The flatness, in pixels, a subpath is measured at.
pub const ALONG_TOLERANCE: f64 = 1.0 / 64.0;

/// The most distances one frames request may ask (`dr_path_frames`): a line
/// of text asks one per glyph, and 65,536 glyphs is far past the longest line
/// the coverage rasterizer accepts.
pub const MAX_FRAMES: usize = 65_536;

/// A point on the subpath and the unit tangent there, in the direction of
/// travel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub point: [f64; 2],
    pub tangent: [f64; 2],
}

/// One subpath as a measured polyline.
#[derive(Clone, Debug)]
pub struct Along {
    /// The polyline, at least two points, no two consecutive ones equal; a
    /// closed subpath's last point repeats its first.
    points: Vec<[f64; 2]>,
    /// `lengths[k]` is the distance from the first point to `points[k]`.
    lengths: Vec<f64>,
    closed: bool,
}

const NO_LENGTH: &str = "the subpath has no length";

impl Along {
    /// Subpath `subpath` of `shape` measured from its first anchor, or from
    /// its other end when `reversed`. `Err` for no such subpath, the
    /// flattener's refusal, or a subpath with no length (one anchor, or every
    /// anchor on one point).
    pub fn new(shape: &PathShape, subpath: usize, reversed: bool) -> Result<Along, String> {
        let one = shape
            .subpaths
            .get(subpath)
            .ok_or_else(|| format!("the path has no subpath {subpath}"))?;
        let single = PathShape {
            subpaths: vec![one.clone()],
            fill_rule: shape.fill_rule,
        };
        let (mut flat, closed) = single
            .flattened(ALONG_TOLERANCE)?
            .pop()
            .ok_or_else(|| NO_LENGTH.to_string())?;
        if closed {
            if let Some(&first) = flat.first() {
                flat.push(first);
            }
        }
        if reversed {
            flat.reverse();
        }
        let mut points: Vec<[f64; 2]> = Vec::with_capacity(flat.len());
        let mut lengths = Vec::with_capacity(flat.len());
        for p in flat {
            match points.last() {
                None => {
                    points.push(p);
                    lengths.push(0.0);
                }
                Some(&last) => {
                    let step = (p[0] - last[0]).hypot(p[1] - last[1]);
                    if step > 0.0 {
                        let total = lengths.last().copied().unwrap_or(0.0) + step;
                        points.push(p);
                        lengths.push(total);
                    }
                }
            }
        }
        let length = lengths.last().copied().unwrap_or(0.0);
        if points.len() < 2 || !(length > 0.0 && length.is_finite()) {
            return Err(NO_LENGTH.into());
        }
        Ok(Along {
            points,
            lengths,
            closed,
        })
    }

    /// The subpath's measured length, closing segment included.
    pub fn length(&self) -> f64 {
        self.lengths.last().copied().unwrap_or(0.0)
    }

    /// The frame `distance` along the subpath. Closed: any finite distance,
    /// taken modulo the length. Open: `None` outside `0..=length`. `None` for
    /// a distance that is not finite.
    pub fn frame(&self, distance: f64) -> Option<Frame> {
        if !distance.is_finite() {
            return None;
        }
        let length = self.length();
        let d = if self.closed {
            // `rem_euclid` may round up to the divisor itself.
            distance.rem_euclid(length).clamp(0.0, length)
        } else if (0.0..=length).contains(&distance) {
            distance
        } else {
            return None;
        };
        Some(self.at(d))
    }

    /// [`Self::frame`], except that an OPEN subpath continues straight past
    /// either end along its end chord's direction, so every finite distance
    /// has a frame. A distance that is not finite answers a frame of NaNs.
    pub fn frame_extended(&self, distance: f64) -> Frame {
        if !distance.is_finite() {
            return Frame {
                point: [f64::NAN; 2],
                tangent: [f64::NAN; 2],
            };
        }
        if self.closed {
            return self.frame(distance).unwrap_or_else(|| self.at(0.0));
        }
        let length = self.length();
        let (end, past) = if distance < 0.0 {
            (self.at(0.0), distance)
        } else if distance > length {
            (self.at(length), distance - length)
        } else {
            return self.at(distance);
        };
        Frame {
            point: [
                end.point[0] + end.tangent[0] * past,
                end.point[1] + end.tangent[1] * past,
            ],
            tangent: end.tangent,
        }
    }

    /// The frame at `d` in `0..=length`: the chord whose span holds it (the
    /// chord STARTING at a vertex the distance lands on, the last chord at the
    /// very end), a linear interpolation along it, and its unit direction.
    fn at(&self, d: f64) -> Frame {
        let chords = self.points.len() - 1;
        let index = self
            .lengths
            .partition_point(|&l| l <= d)
            .saturating_sub(1)
            .min(chords - 1);
        let (a, b) = (self.points[index], self.points[index + 1]);
        let span = self.lengths[index + 1] - self.lengths[index];
        let t = ((d - self.lengths[index]) / span).clamp(0.0, 1.0);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let norm = dx.hypot(dy);
        Frame {
            point: [a[0] + dx * t, a[1] + dy * t],
            tangent: [dx / norm, dy / norm],
        }
    }
}
