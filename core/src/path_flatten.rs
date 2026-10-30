//! PATH FLATTENING — [`PathShape::flattened`], each subpath as a
//! polyline within the caller's tolerance, for what FOLLOWS the outline (a
//! stroke, the canvas overlay). A kernel beside `path_geometry`, which holds
//! the model, the caps ([`MIN_FLATTEN_TOLERANCE`], [`MAX_FLATTENED_POINTS`])
//! and the segment numbering this walks. The master copy of the subdivision
//! is `artwork_coverage::cubic`, which flattens for coverage at a fixed
//! 1/4096 px and emits edges, not points. It names no document type.

use crate::artwork_coverage;
use crate::path_geometry::{
    is_line, lerp, PathShape, Point, MAX_FLATTENED_POINTS, MIN_FLATTEN_TOLERANCE,
};

/// The subdivision depth at which a curve piece is emitted as its chord
/// whatever its flatness. Each level quarters the control distance, so 20
/// levels take the widest curve the wall allows (4·10⁶ px) to 4·10⁻⁶ px,
/// below [`MIN_FLATTEN_TOLERANCE`]: the cap is never what ends a
/// subdivision, it only bounds the recursion.
const FLATTEN_DEPTH: u8 = 20;

/// Appends the cubic `p` WITHOUT its first point: split in half until both
/// control points lie within `tolerance` of the chord, measured to the chord
/// SEGMENT so a control that doubles back cannot pass as flat. The curve lies
/// in its control hull and distance to a segment is convex, so every point of
/// an emitted piece is within `tolerance` of its chord.
fn flatten_cubic(
    p: [Point; 4],
    tolerance: f64,
    depth: u8,
    out: &mut Vec<Point>,
    room: &mut usize,
) -> Result<(), String> {
    let flat = artwork_coverage::distance(p[1], p[0], p[3])
        .max(artwork_coverage::distance(p[2], p[0], p[3]))
        <= tolerance;
    if flat || depth >= FLATTEN_DEPTH {
        return push_flat(out, p[3], room);
    }
    let (a, b, c) = (
        lerp(p[0], p[1], 0.5),
        lerp(p[1], p[2], 0.5),
        lerp(p[2], p[3], 0.5),
    );
    let (d, e) = (lerp(a, b, 0.5), lerp(b, c, 0.5));
    let f = lerp(d, e, 0.5);
    flatten_cubic([p[0], a, d, f], tolerance, depth + 1, out, room)?;
    flatten_cubic([f, e, c, p[3]], tolerance, depth + 1, out, room)
}

fn push_flat(out: &mut Vec<Point>, point: Point, room: &mut usize) -> Result<(), String> {
    if *room == 0 {
        return Err(format!(
            "the path flattens to more than {MAX_FLATTENED_POINTS} points; use a larger tolerance"
        ));
    }
    *room -= 1;
    out.push(point);
    Ok(())
}

impl PathShape {
    /// Each subpath as a polyline and its `closed` flag, every point within
    /// `tolerance` pixels of the curve and the curve within `tolerance` of
    /// the polyline (clamped up to [`MIN_FLATTEN_TOLERANCE`]; a NaN is that
    /// floor). The polyline starts at the first anchor and passes through
    /// every anchor in order. A CLOSED subpath's polyline covers its closing
    /// segment and does NOT repeat the first point at its end — the flag
    /// says so; an open one ends on its last anchor (the straight closure a
    /// fill adds is not followed). `Err` when the whole shape needs more
    /// than [`MAX_FLATTENED_POINTS`] points.
    pub fn flattened(&self, tolerance: f64) -> Result<Vec<(Vec<Point>, bool)>, String> {
        let tolerance = if tolerance.is_nan() {
            MIN_FLATTEN_TOLERANCE
        } else {
            tolerance.max(MIN_FLATTEN_TOLERANCE)
        };
        let mut room = MAX_FLATTENED_POINTS;
        let mut out = Vec::with_capacity(self.subpaths.len());
        for subpath in &self.subpaths {
            let Some(first) = subpath.anchors.first() else {
                continue;
            };
            let mut points = Vec::new();
            push_flat(&mut points, first.point, &mut room)?;
            let segments = subpath.segment_count();
            for index in 0..segments {
                let Some((a, b)) = subpath.segment(index) else {
                    break;
                };
                if is_line(a, b) {
                    push_flat(&mut points, b.point, &mut room)?;
                } else {
                    let curve = [a.point, a.handle_out, b.handle_in, b.point];
                    flatten_cubic(curve, tolerance, 0, &mut points, &mut room)?;
                }
            }
            // The closing segment ended on the first point again.
            if subpath.closed && segments > 0 {
                points.pop();
            }
            out.push((points, subpath.closed));
        }
        Ok(out)
    }
}
