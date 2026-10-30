//! The Smudge brush: the stroke drags the picture along its path.
//!
//! The walk runs over a premultiplied f32 FIELD covering the stroke's
//! footprint (`brush_coverage::footprint`). Dab 0 only anchors. The move from
//! dab `k − 1` to dab `k`, `δ = p_k − p_{k−1}`, is walked in `m = max(1,
//! ⌈|δ|⌉)` SUB-STEPS of `s = δ/m`, at most one pixel each, and sub-step `j` of
//! `1..=m` uses the dab `d_{k−1} + ((d_k − d_{k−1})·j)/m` (centre, diameter and
//! opacity; the product is taken before the division, so an integer move lands
//! on exact steps). Every pixel `q` of that dab's clipped box becomes
//!
//! ```text
//! old + a·(S(q − s) − old),   a = min(1, strength · coverage · opacity · weight(q))
//! ```
//!
//! with `S` a bilinear read of the field as the sub-step found it (positions
//! clamped to the field), `coverage` the dab's own (`BrushRequest::
//! dab_coverage`, classified for a hard tip exactly as the stroke rasterizer
//! classifies it) and `weight` the caller's selection. Each sub-step is staged
//! and then written, so a pixel never reads a neighbour the same sub-step has
//! already moved.
//!
//! WHY SUB-STEPS, AND WHY THE SAME `a` AT EACH. Moving the picture by a whole
//! Spacing step per dab leaves discrete copies with gaps between them. Walked a
//! pixel at a time with the same `a`, a pixel carries a fraction `a` of what
//! arrives at every sub-step, so its mean carry over the move is `a·δ`: the
//! smear is continuous, and the result depends on the path, not on how many
//! dabs the host laid along it (a straight move as 2 dabs 12 px apart and as
//! 13 dabs 1 px apart is the same 12 sub-steps). The per-sub-step amount
//! `a^(1/m)` would carry far more than the per-dab amount asks for (at
//! strength 0.5 over 12 sub-steps, a mean 11.3 of 12 px against 6).
//!
//! WORK. [`smudge_work`] first takes `BrushRequest::plan`'s refusals, then
//! bounds `Σ m` by `MAX_SMUDGE_WORK` before any sub-step is visited (a far move
//! is refused without being walked), then charges every sub-step
//! `dab_work + 1` — `plan`'s own per-dab charge, so a hard tip pays for its
//! classifier and band, not 64 samples a pixel — and refuses in its own
//! sentence above `MAX_SMUDGE_WORK` (256 million: the 8-bit stroke runs
//! synchronously on the canvas's final tick, so Smudge does not share the
//! brush job's 2 billion). The adapters call it before allocating.
use crate::brush_coverage::{footprint, BrushRequest, MAX_SMUDGE_WORK};
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;

const WORK_REFUSAL: &str =
    "smudge stroke exceeds 256 million work units; use a shorter stroke or a smaller brush";

/// `m` for the move between two dabs.
fn sub_step_count(from: &[f64], to: &[f64]) -> u64 {
    let length = (to[0] - from[0]).hypot(to[1] - from[1]);
    (length.ceil() as u64).max(1)
}

/// Sub-step `j` of `m`: the interpolated dab.
fn sub_step_dab(from: &[f64], to: &[f64], j: u64, m: u64) -> [f64; 4] {
    std::array::from_fn(|c| from[c] + ((to[c] - from[c]) * j as f64) / m as f64)
}

/// The work units of [`smudge`] over a `dimensions` plane, refused before
/// anything is walked or allocated (module doc, WORK).
pub(crate) fn smudge_work(
    request: &BrushRequest,
    dimensions: (u32, u32),
    dabs: &[f64],
) -> Result<u64, String> {
    request.plan(dimensions, dabs)?;
    let mut steps = 0_u64;
    for pair in dabs.chunks_exact(4).collect::<Vec<_>>().windows(2) {
        steps = steps.saturating_add(sub_step_count(pair[0], pair[1]));
        if steps > MAX_SMUDGE_WORK {
            return Err(WORK_REFUSAL.into());
        }
    }
    let mut work = 0_u64;
    let mut previous: Option<&[f64]> = None;
    for dab in dabs.chunks_exact(4) {
        if let Some(from) = previous {
            let m = sub_step_count(from, dab);
            for j in 1..=m {
                work += request.dab_work(dimensions, &sub_step_dab(from, dab, j, m)) + 1;
                if work > MAX_SMUDGE_WORK {
                    return Err(WORK_REFUSAL.into());
                }
            }
        }
        previous = Some(dab);
    }
    Ok(work)
}

/// A bound on the pixels of the stage [`smudge`] holds beside the field: the
/// largest sub-step box. A sub-step's diameter lies between its two dabs', so
/// its box is at most `(⌈d_max⌉ + 2)²`, and never more than the field.
pub(crate) fn smudge_stage_pixels(field_pixels: u64, dabs: &[f64]) -> u64 {
    let widest = dabs
        .chunks_exact(4)
        .map(|d| d[2])
        .fold(0.0_f64, f64::max)
        .ceil() as u64
        + 2;
    widest.saturating_mul(widest).min(field_pixels)
}

/// Bilinear read of the field at `(x, y)` in plane pixel-index coordinates
/// (integers are pixel positions), clamped to `rect`.
fn bilinear(field: &[[f32; 4]], rect: [usize; 4], x: f64, y: f64) -> [f32; 4] {
    let width = rect[2] - rect[0];
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
    let column = |v: f64| (v as i64).clamp(rect[0] as i64, rect[2] as i64 - 1) as usize - rect[0];
    let row = |v: f64| (v as i64).clamp(rect[1] as i64, rect[3] as i64 - 1) as usize - rect[1];
    let (xa, xb) = (column(x0), column(x0 + 1.0));
    let (ya, yb) = (row(y0) * width, row(y0 + 1.0) * width);
    let (p00, p01, p10, p11) = (
        field[ya + xa],
        field[ya + xb],
        field[yb + xa],
        field[yb + xb],
    );
    std::array::from_fn(|c| {
        (p00[c] * (1.0 - fx) + p01[c] * fx) * (1.0 - fy) + (p10[c] * (1.0 - fx) + p11[c] * fx) * fy
    })
}

/// Smudges `field` — premultiplied samples over `rect = [x0, y0, x1, y1)` of a
/// plane, row-major — along `dabs` (packed x / y / diameter / opacity, in that
/// plane's coordinates) at `strength` in [0, 1]. `weight(x, y)` is the
/// selection's coverage of plane pixel `(x, y)` in [0, 1]. Sub-step boxes are
/// clipped to `rect`; nothing outside it is read or written. The caller has
/// admitted the stroke through [`smudge_work`] and quantizes the field once
/// when this returns.
pub(crate) fn smudge(
    field: &mut [[f32; 4]],
    rect: [usize; 4],
    request: &BrushRequest,
    dabs: &[f64],
    strength: f64,
    weight: impl Fn(usize, usize) -> f64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let width = rect[2] - rect[0];
    let plane = (
        u32::try_from(rect[2]).map_err(|_| "smudge field exceeds the supported range")?,
        u32::try_from(rect[3]).map_err(|_| "smudge field exceeds the supported range")?,
    );
    let rotation = request.rotation();
    let moves = (dabs.len() / 4).saturating_sub(1).max(1);
    let mut stage: Vec<[f32; 4]> = reserved_vec(0)?;
    let mut previous: Option<&[f64]> = None;
    for (k, dab) in dabs.chunks_exact(4).enumerate() {
        job.check()?;
        let Some(from) = previous.replace(dab) else {
            continue;
        };
        let m = sub_step_count(from, dab);
        let step = ((dab[0] - from[0]) / m as f64, (dab[1] - from[1]) / m as f64);
        for j in 1..=m {
            job.check()?;
            let tip = sub_step_dab(from, dab, j, m);
            // The dab's box, clipped to the plane's corner at `rect`'s far
            // edge by `footprint` and to `rect`'s near edge here.
            let Some(b) = footprint(plane, &tip) else {
                continue;
            };
            let (x0, y0) = (b[0].max(rect[0]), b[1].max(rect[1]));
            let (x1, y1) = (b[2], b[3]);
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            stage.clear();
            stage
                .try_reserve((x1 - x0) * (y1 - y0))
                .map_err(|_| "could not allocate precision output".to_string())?;
            for y in y0..y1 {
                for x in x0..x1 {
                    let old = field[(y - rect[1]) * width + (x - rect[0])];
                    let amount = (strength
                        * request.dab_coverage(&tip, rotation, x, y)
                        * tip[3]
                        * weight(x, y))
                    .min(1.0);
                    stage.push(if amount > 0.0 {
                        let carried = bilinear(field, rect, x as f64 - step.0, y as f64 - step.1);
                        let amount = amount as f32;
                        std::array::from_fn(|c| old[c] + amount * (carried[c] - old[c]))
                    } else {
                        old
                    });
                }
            }
            for (row, line) in stage.chunks_exact(x1 - x0).enumerate() {
                let at = (y0 + row - rect[1]) * width + (x0 - rect[0]);
                field[at..at + line.len()].copy_from_slice(line);
            }
        }
        job.report(k as f64 / moves as f64 * 0.9, 3)?;
    }
    Ok(())
}
