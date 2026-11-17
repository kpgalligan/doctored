//! KERNEL: brush dynamics' DUAL BRUSH (`designs/brush-dynamics.md`, slice 4):
//! a second tip that walks the first tip's path with its own size, spacing,
//! scatter and stream, and whose accumulated coverage multiplies the first's
//! before the stroke's one quantization. The handle and the rasterizing
//! exports are `ffi_brush_dual`; the coverage is `brush_coverage` (THE DUAL
//! STROKE).
//!
//! # The rule
//!
//! - THE VALUE. The second tip has a size (1…5000 px, absolute), its own
//!   hardness, roundness and angle (a native brush request), a spacing
//!   (1…200 % of its size) and a scatter (amount 0…10, both axes, count
//!   1…16, count jitter 0…1). It takes no pressure, tilt, jitter, shape or
//!   texture: every second dab has diameter `size` and deposit 1.
//! - THE PATH. The second tip walks the polyline through the first tip's
//!   SPACING stamps' nominal centres, in order. An airbrush tick is a deposit
//!   at rest, not a point of the path, so it never feeds the walk (the caller
//!   withholds it). The first fed point lays one stamp there and becomes the
//!   anchor. At each later point `p`, while `|p − anchor| ≥ step`, `step =
//!   max(1, size · spacing / 100)`, the anchor moves `step` toward `p` and a
//!   stamp is laid there with the direction `p − anchor` before the move.
//!   The walk is positional: one long move lays the stamps many short
//!   collinear moves lay.
//! - THE STREAM. Second stamp `j` (the walk's 0-based count of second stamps)
//!   resolves through [`Dynamics::dabs`], the scatter's one implementation,
//!   with the seed `2³² | seed`, `seed` the first tip's u32 seed: every
//!   first-tip seed lies below 2³², so the two streams never share a seed.
//!   Its sample is the stamp's position, the size, no pen and the direction.
//! - THE COVERAGE. `F1` is the first dab list's field exactly as
//!   `rasterize_textured` accumulates it (shapes, texture, deposits); `F2` the
//!   second list's under the second tip's request, with no shapes and no
//!   texture. The output pixel is `first.quantize(F1 · F2)`, 0 outside either
//!   footprint. Multiplying before the one rounding keeps a soft second tip's
//!   ramp continuous; quantizing each field first would round twice.
//! - THE CAPS. The second list holds at most 50,000 dabs on its own; the work
//!   is the two plans' sum, at most `MAX_BRUSH_WORK` (the first tip's
//!   sentence), and the scratch the two plans' sum. A pixel or aliased tip on
//!   either list refuses ([`DUAL_TIP_REFUSAL`]): a two-valued dab times a
//!   second field is a third value.
use crate::brush_coverage::BrushRequest;
use crate::brush_dynamics::{Dynamics, DAB_LEN, SAMPLE_LEN};
use crate::brush_texture::BrushTexture;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use serde::Deserialize;

/// A dual request's byte bound, checked before parsing.
pub const MAX_REQUEST_BYTES: usize = 1024;
/// The smallest and largest second-tip size, in pixels.
pub const MIN_SIZE: f64 = 1.;
pub const MAX_SIZE: f64 = 5000.;
/// The smallest and largest second-tip spacing, in percent of its size.
pub const MIN_SPACING: f64 = 1.;
pub const MAX_SPACING: f64 = 200.;
/// A size or spacing out of range (not finite included).
pub const DUAL_REFUSAL: &str =
    "native brush dual holds a size from 1 to 5000 and a spacing from 1 to 200";
/// A pixel or aliased request on either list (module doc, THE CAPS).
pub const DUAL_TIP_REFUSAL: &str = "native brush dual requires stamped tips that are not aliased";
/// The bit that lifts the second stream's seed above every first-tip seed.
const STREAM_BIT: u64 = 1 << 32;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScatterText {
    #[serde(default)]
    amount: f64,
    #[serde(default)]
    both_axes: bool,
    #[serde(default = "one")]
    count: u32,
    #[serde(default)]
    count_jitter: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestText {
    version: u32,
    #[serde(default)]
    seed: u32,
    size: f64,
    spacing: f64,
    #[serde(default)]
    scatter: Option<ScatterText>,
}

fn one() -> u32 {
    1
}

/// A parsed dual request: the second tip's size, its spacing and its scatter
/// (module doc), immutable once made.
pub struct DualBrush {
    size: f64,
    spacing: f64,
    scatter: Dynamics,
}

/// Where a walk stopped: the anchor after its last stamp and the stamps laid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DualStep {
    pub anchor: [f64; 2],
    pub stamps: u64,
}

impl DualBrush {
    /// Strict: `{"version":1,"seed":u32,"size":f64,"spacing":f64,
    /// "scatter":{"amount","both_axes","count","count_jitter"}}`, known keys
    /// only; `seed` and `scatter` may be absent. A size or spacing out of range
    /// refuses with [`DUAL_REFUSAL`], a scatter value with the scatter's own
    /// sentences (`Dynamics::scatter_only`).
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_REQUEST_BYTES {
            return Err(format!(
                "native brush dual request exceeds {MAX_REQUEST_BYTES} bytes"
            ));
        }
        let request: RequestText = serde_json::from_str(text)
            .map_err(|e| format!("invalid native brush dual request: {e}"))?;
        if request.version != 1 {
            return Err("native brush dual requires version 1".into());
        }
        // A NaN lies in neither closed range.
        if !(MIN_SIZE..=MAX_SIZE).contains(&request.size)
            || !(MIN_SPACING..=MAX_SPACING).contains(&request.spacing)
        {
            return Err(DUAL_REFUSAL.into());
        }
        let scatter = request.scatter.unwrap_or(ScatterText {
            amount: 0.,
            both_axes: false,
            count: 1,
            count_jitter: 0.,
        });
        let scatter = Dynamics::scatter_only(
            STREAM_BIT | u64::from(request.seed),
            scatter.amount,
            scatter.both_axes,
            scatter.count,
            scatter.count_jitter,
        )?;
        Ok(Self {
            size: request.size,
            spacing: request.spacing,
            scatter,
        })
    }

    /// The most dabs one second stamp lays: the scatter count.
    pub fn max_dabs(&self) -> usize {
        self.scatter.max_dabs()
    }

    /// The walk's step: `max(1, size · spacing / 100)` (module doc).
    fn step(&self) -> f64 {
        (self.size * self.spacing / 100.).max(1.)
    }

    /// Feeds the path point `to` to a walk whose anchor is `anchor` (None: `to`
    /// is the path's first point) and whose next stamp is `index`, laying at
    /// most `max_stamps` stamps (module doc, THE PATH). Each stamp's dabs are
    /// appended to `out`, four values a dab (x, y, size, 1.0). A walk cut short
    /// by `max_stamps` continues from the returned anchor with the same `to`
    /// and the index advanced by the stamps laid, and lays what one call would
    /// have. None (nothing appended) when a point is not finite, or when the
    /// first point finds no room for its stamp.
    pub fn walk(
        &self,
        index: u64,
        anchor: Option<[f64; 2]>,
        to: [f64; 2],
        max_stamps: usize,
        out: &mut Vec<f64>,
    ) -> Option<DualStep> {
        if !to.iter().all(|v| v.is_finite()) {
            return None;
        }
        let Some(mut at) = anchor else {
            if max_stamps == 0 {
                return None;
            }
            self.lay(index, to, [0., 0.], out)?;
            return Some(DualStep {
                anchor: to,
                stamps: 1,
            });
        };
        if !at.iter().all(|v| v.is_finite()) {
            return None;
        }
        let step = self.step();
        let start = out.len();
        let mut stamps = 0_u64;
        while (stamps as usize) < max_stamps {
            let (dx, dy) = (to[0] - at[0], to[1] - at[1]);
            let distance = dx.hypot(dy);
            if distance < step {
                break;
            }
            at = [at[0] + dx / distance * step, at[1] + dy / distance * step];
            // The index is a stream coordinate, never a count, so it wraps.
            if self
                .lay(index.wrapping_add(stamps), at, [dx, dy], out)
                .is_none()
            {
                out.truncate(start);
                return None;
            }
            stamps += 1;
        }
        Some(DualStep { anchor: at, stamps })
    }

    /// Second stamp `index` at `at` with the direction `direction` (zero for
    /// none), its dabs' first four values appended to `out` (module doc, THE
    /// STREAM): with no size, opacity or shape response a dab's diameter is
    /// `size` and its deposit 1.
    fn lay(&self, index: u64, at: [f64; 2], direction: [f64; 2], out: &mut Vec<f64>) -> Option<()> {
        let nan = f64::NAN;
        let sample: [f64; SAMPLE_LEN] = [
            at[0],
            at[1],
            self.size,
            nan,
            nan,
            nan,
            direction[0],
            direction[1],
            nan,
            0.,
            0.,
        ];
        let mut dabs = Vec::with_capacity(DAB_LEN * self.max_dabs());
        self.scatter.dabs(index, &sample, &mut dabs)?;
        for d in dabs.chunks_exact(DAB_LEN) {
            out.extend_from_slice(&d[..4]);
        }
        Some(())
    }
}

/// The dual stroke's u16 coverage (module doc, THE COVERAGE): `F1` from
/// `first` over `dabs` with its optional shapes and texture (output pixel (0,
/// 0) is the texture's canvas `origin`), `F2` from `second` over `dual_dabs`,
/// and `first.quantize(F1 · F2)` per pixel. Refuses a pixel or aliased tip on
/// either request. The caller has admitted both lists, their work sum (THE
/// CAPS) and the shapes (`ffi_brush_dual`'s `dual_request`); `output_samples`
/// is the first plan's.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_dual(
    first: &BrushRequest,
    second: &BrushRequest,
    dimensions: (u32, u32),
    dabs: &[f64],
    shapes: Option<&[f64]>,
    texture: Option<(&BrushTexture, (i64, i64))>,
    dual_dabs: &[f64],
    output_samples: usize,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    if !first.stamped() || !second.stamped() {
        return Err(DUAL_TIP_REFUSAL.into());
    }
    let f1 = first.field(dimensions, dabs, shapes, texture, (0., 0.45), job)?;
    let f2 = second.field(dimensions, dual_dabs, None, None, (0.45, 0.9), job)?;
    product(
        first,
        dimensions,
        output_samples,
        (&f1.0, f1.1),
        (&f2.0, f2.1),
        job,
    )
}

/// `first.quantize(F1 · F2)` per canvas pixel, 0 outside either footprint
/// (module doc, THE COVERAGE): each field with its footprint `[x0, y0, x1,
/// y1)`, `samples` the first plan's output count. The coloured stroke
/// (`brush_colour`) quantizes its dual coverage here too.
pub(crate) fn product(
    first: &BrushRequest,
    dimensions: (u32, u32),
    samples: usize,
    f1: (&[f64], [usize; 4]),
    f2: (&[f64], [usize; 4]),
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let (f1, [ax0, ay0, ax1, ay1]) = f1;
    let (f2, [bx0, by0, bx1, by1]) = f2;
    let (aw, bw) = (ax1 - ax0, bx1 - bx0);
    let (width, height) = (dimensions.0 as usize, dimensions.1 as usize);
    let mut output = reserved_vec(samples)?;
    let mut i = 0;
    for y in 0..height {
        for x in 0..width {
            job.every(i)?;
            i += 1;
            let inside = (ax0..ax1).contains(&x)
                && (ay0..ay1).contains(&y)
                && (bx0..bx1).contains(&x)
                && (by0..by1).contains(&y);
            let value = if inside {
                f1[(y - ay0) * aw + (x - ax0)] * f2[(y - by0) * bw + (x - bx0)]
            } else {
                0.
            };
            output.push(first.quantize(value));
        }
    }
    job.check()?;
    Ok(output)
}
