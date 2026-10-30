//! Categorical trimap painting for an explicitly supplied 0/128/255 plane.
//! A round-cap stroke labels pixel centers whose distance from any polyline
//! segment is <= radius. There is no antialias coverage or compositing: the last
//! stroke wins, and Unknown deliberately removes a prior known constraint.
//!
//! This constructor does not infer a trimap from alpha or enable a matting
//! candidate. Geometry is preflighted before output allocation: the sum of all
//! clipped segment-box areas is capped, including repeated/overlapping visits.
//! That is the actual loop work rather than only the final affected area.
//!
//! Explicit base labels only; no alpha initializer is inferred. Typed strict JSON, point/stroke
//! caps, aggregate segment-box work admission and cooperative cancellation precede atomic
//! caller-buffer publication.

use crate::{doc::MAX_PIXELS, job_control::JobContext};
use serde::Deserialize;

pub(crate) const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_STROKES: usize = 256;
pub(crate) const MAX_STROKE_POINTS: usize = 8192;
pub(crate) const MAX_POINTS: usize = 65_536;
pub(crate) const MAX_VISITS: u64 = 64_000_000;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Label {
    Background,
    Unknown,
    Foreground,
}
impl Label {
    fn byte(self) -> u8 {
        match self {
            Self::Background => 0,
            Self::Unknown => 128,
            Self::Foreground => 255,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stroke {
    label: Label,
    radius: f64,
    points: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    version: u32,
    width: u32,
    height: u32,
    strokes: Vec<Stroke>,
}

pub(crate) fn pixel_count(width: u32, height: u32) -> Result<usize, String> {
    let n = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || n > MAX_PIXELS {
        return Err("trimap dimensions must be nonzero and fit the canvas pixel limit".into());
    }
    Ok(n as usize)
}

pub(crate) fn scratch_bytes(n: usize, json_bytes: usize) -> u64 {
    // The JSON multiplier covers the owned text and typed deserialization,
    // including vector growth before malformed/oversized point lists refuse.
    // The fixed allowance covers the bounded segment plan and parser overhead.
    n as u64 + json_bytes as u64 * 8 + 16 * 1024 * 1024
}

pub(crate) struct Segment {
    origin: [f64; 2],
    vector: [f64; 2],
    inverse_length_squared: f64,
    radius_squared: f64,
    pub(crate) bounds: [usize; 4],
    pub(crate) label: u8,
}
impl Segment {
    pub(crate) fn new(
        a: [f64; 2],
        b: [f64; 2],
        radius: f64,
        label: u8,
        width: u32,
        height: u32,
    ) -> Self {
        let vector = [b[0] - a[0], b[1] - a[1]];
        let length_squared = vector[0] * vector[0] + vector[1] * vector[1];
        let lower = |value: f64, limit: u32| value.floor().clamp(0.0, f64::from(limit)) as usize;
        let upper = |value: f64, limit: u32| value.ceil().clamp(0.0, f64::from(limit)) as usize;
        Self {
            origin: a,
            vector,
            inverse_length_squared: if length_squared == 0.0 {
                0.0
            } else {
                1.0 / length_squared
            },
            radius_squared: radius * radius,
            bounds: [
                lower(a[0].min(b[0]) - radius, width),
                lower(a[1].min(b[1]) - radius, height),
                upper(a[0].max(b[0]) + radius, width),
                upper(a[1].max(b[1]) + radius, height),
            ],
            label,
        }
    }

    pub(crate) fn visits(&self) -> u64 {
        ((self.bounds[2] - self.bounds[0]) * (self.bounds[3] - self.bounds[1])) as u64
    }

    pub(crate) fn contains(&self, x: f64, y: f64) -> bool {
        let delta = [x - self.origin[0], y - self.origin[1]];
        let t = ((delta[0] * self.vector[0] + delta[1] * self.vector[1])
            * self.inverse_length_squared)
            .clamp(0.0, 1.0);
        let dx = delta[0] - t * self.vector[0];
        let dy = delta[1] - t * self.vector[1];
        dx * dx + dy * dy <= self.radius_squared
    }
}

pub(crate) fn apply_strokes(
    input: &[u8],
    width: u32,
    height: u32,
    json: &str,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let n = pixel_count(width, height)?;
    if input.len() != n || json.len() > MAX_JSON_BYTES {
        return Err("trimap input length or 4 MiB stroke payload limit exceeded".into());
    }
    job.reserve(scratch_bytes(n, json.len()))?;
    for (i, byte) in input.iter().enumerate() {
        job.every(i)?;
        if !matches!(byte, 0 | 128 | 255) {
            return Err("trimap contains a noncategorical label; use only 0, 128 or 255".into());
        }
    }
    job.report(0.1, 2)?;
    let payload: Payload =
        serde_json::from_str(json).map_err(|e| format!("invalid trimap strokes: {e}"))?;
    if payload.version != 1 || payload.width != width || payload.height != height {
        return Err("trimap stroke version or canvas dimensions do not match".into());
    }
    if payload.strokes.len() > MAX_STROKES {
        return Err("trimap has more than 256 strokes".into());
    }
    let mut points = 0usize;
    let mut visits = 0u64;
    let mut plan = Vec::new();
    for stroke in &payload.strokes {
        job.check()?;
        points += stroke.points.len();
        if stroke.points.is_empty()
            || stroke.points.len() > MAX_STROKE_POINTS
            || points > MAX_POINTS
        {
            return Err(
                "trimap permits 1–8192 points per stroke and 65536 points per request".into(),
            );
        }
        if !stroke.radius.is_finite()
            || !(0.5..=512.0).contains(&stroke.radius)
            || stroke
                .points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 100_000.0)
        {
            return Err(
                "trimap radius must be 0.5–512 and finite coordinates within ±100000".into(),
            );
        }
        for index in 0..stroke.points.len().saturating_sub(1).max(1) {
            job.every(index)?;
            let a = stroke.points[index];
            let b = stroke.points[(index + 1).min(stroke.points.len() - 1)];
            let segment = Segment::new(a, b, stroke.radius, stroke.label.byte(), width, height);
            visits = visits
                .checked_add(segment.visits())
                .ok_or("trimap work estimate overflow")?;
            if visits > MAX_VISITS {
                return Err("trimap strokes exceed 64 million segment-box pixel visits; shorten strokes or reduce the radius".into());
            }
            if segment.visits() > 0 {
                plan.push(segment);
            }
        }
    }
    job.report(0.2, 2)?;
    let mut output = Vec::with_capacity(n);
    for chunk in input.chunks(65_536) {
        job.check()?;
        output.extend_from_slice(chunk);
    }
    let mut completed = 0usize;
    for segment in plan {
        let [left, top, right, bottom] = segment.bounds;
        for y in top..bottom {
            job.check()?;
            for x in left..right {
                job.every(completed)?;
                if segment.contains(x as f64 + 0.5, y as f64 + 0.5) {
                    output[y * width as usize + x] = segment.label;
                }
                completed += 1;
                if completed & 65535 == 0 {
                    job.report(0.2 + 0.8 * completed as f64 / visits.max(1) as f64, 6)?;
                }
            }
        }
    }
    job.check()?;
    Ok(output)
}
