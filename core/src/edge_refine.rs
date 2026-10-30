//! Edge refinement of selection coverage (Select and Mask's Edge
//! Detection): inside a band around the coverage's 50% contour, and under
//! Refine Edge brush strokes, coverage is re-estimated from the picture's own
//! colours. Free functions over f32 planes; `selection_refine` (byte) and
//! `precision_selection`'s `refine_edge` kind (u16) are the adapters.
//!
//! WHERE. The write area `W` is every AUTOMATIC pixel whose signed distance to
//! the contour (`doc_select::signed_distance_from_membership` at coverage
//! ≥ 0.5) is at most `radius` in magnitude, plus every INCLUDE pixel, minus
//! every EXCLUDE pixel (the area plane: [`AREA_EXCLUDE`] / [`AREA_AUTO`] /
//! [`AREA_INCLUDE`]; no plane = all automatic). Outside `W` every sample is
//! returned bit-identical; an empty `W` returns the coverage unchanged.
//!
//! WHAT. For each of R, G and B the He/Sun/Tang guided filter (ECCV 2010,
//! equations 5, 6 and 8) of the coverage `p` with that channel `I` (encoded
//! RGB times source alpha, so latent colour under transparency is ignored) as
//! the guide, over the clipped box window of half-width `r = max(4, ⌈radius⌉)`:
//!
//! ```text
//! a = cov(I, p) / (var(I) + ε),   b = mean(p) − a·mean(I),   q = mean(a)·I + mean(b)
//! ```
//!
//! and a CONFIDENCE `k = cov(I, p)² / ((var(I) + ε)·(var(p) + δ))` in 0…1: the
//! share of the coverage's local variance the channel's linear fit explains
//! (R², regularized like `a`). Three scalar filters, not one on luma, because
//! two colours of equal luminance carry no luma edge; a channel that does not
//! separate them has a small `k` and is weighted out.
//!
//! - Smart Radius off: `q = Σ(k_c² + 1e-4)·q_c / Σ(k_c² + 1e-4)` — the
//!   confidence-weighted mean of the three estimates, and their plain mean (a
//!   box-blurred edge of half-width `r`) where no channel sees an edge.
//! - Smart Radius on: `out = coverage + max_c(k_c)·(q − coverage)` — the
//!   refinement fades with the best channel's confidence, so a hard mask edge
//!   across flat colour stays hard and a hair edge takes the estimate.
//!
//! CONSTANTS. `ε = 0.0004` is a 2% intensity variation squared: below it a
//! window is treated as flat, which limits how far noise is amplified into
//! coverage. `δ = 1e-6` only keeps `k` finite where the window holds no
//! coverage edge (`cov` is 0 there, so `k` is 0). The `1e-4` weight floor
//! keeps the smart-off mean defined where every `k` is 0.
//!
//! THE WORK RECTANGLE. Everything is computed over `W`'s bounding box grown by
//! `2r` and clipped to the canvas: `q` at a pixel reads `a` and `b` within
//! `r`, each of which reads statistics within `r`, so the result inside `W`
//! is the full-canvas one.
//!
//! MEMORY. [`BYTES_PER_PIXEL`] bounds the adapter's whole edge stage. The
//! kernel holds the write flags (1 B) and, over the work rectangle, six
//! shared planes (mean and variance of `p`, the weighted sum and weight, the
//! best confidence, the box filter's scratch) and four per channel — 41 B a
//! pixel at most, the channels run one after another; the distance field's
//! two planes (8 B) are freed before any of them. The adapter's f32 coverage
//! and result (8 B) and its byte or u16 planes sit beside that.
use crate::doc_select::signed_distance_from_membership;
use crate::job_control::JobContext;

/// The area plane's three categories (the Refine Edge brush): never refined,
/// refined inside the radius band, always refined.
pub(crate) const AREA_EXCLUDE: u8 = 0;
pub(crate) const AREA_AUTO: u8 = 128;
pub(crate) const AREA_INCLUDE: u8 = 255;

/// The largest Edge Detection radius, in canvas pixels.
pub(crate) const MAX_EDGE_RADIUS: f32 = 64.0;

/// The adapter's reservation for the edge stage, per canvas pixel (module
/// doc, MEMORY).
///
/// Record (the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, a fresh
/// instance per case, `MallocLargeCache=0`, E =
/// `ri_interval_max_phys_footprint` − pre around the one MCP call, 3 runs + a
/// baseline twin at E ≤ 98,304; a 6000×4000 16-bit layer of two-colour Clouds
/// with a hard elliptical selection at (50, 50), 5900×3900, whose edge band's
/// bounding box is nearly the canvas):
/// `refine_selection edge_radius 8` (the typed `refine_edge` kind): E
/// 1,588,102,272 / 1,588,216,984 / 1,588,036,760 against a plan of at least
/// 2,745,437,184 (1.729×; 9 MiB + 114 B a pixel — this constant, the kind's
/// 40 B and twice its input, and the composite's 8 B — leaving out the
/// analysis term's render and statistics scratch).
pub(crate) const BYTES_PER_PIXEL: u64 = 64;

/// Guided-filter regularization (module doc, CONSTANTS).
const EPSILON: f32 = 0.0004;
/// Keeps the confidence finite in a window with no coverage edge.
const DELTA: f32 = 1e-6;
/// The smart-off weight floor.
const WEIGHT_FLOOR: f32 = 1e-4;

/// Edge refinement's two controls: the band's half-width in canvas pixels
/// (0…[`MAX_EDGE_RADIUS`]; 0 refines only INCLUDE pixels) and Smart Radius.
#[derive(Clone, Copy)]
pub(crate) struct EdgeRefine {
    pub radius: f32,
    pub smart: bool,
}

/// Validates an area plane and answers whether it holds an INCLUDE pixel.
/// `Err` for any value other than the three categories.
pub(crate) fn area_includes(area: &[u8]) -> Result<bool, String> {
    let mut includes = false;
    for &value in area {
        match value {
            AREA_INCLUDE => includes = true,
            AREA_EXCLUDE | AREA_AUTO => {}
            _ => {
                return Err(
                    "refinement area values must be 0 (exclude), 128 (automatic) or 255 (include)"
                        .into(),
                )
            }
        }
    }
    Ok(includes)
}

/// The clipped box mean of `plane` (`w`×`h`, half-width `r`) in place, through
/// `scratch` (same length). Two running sums make the time linear in pixels
/// whatever the radius; f64 accumulators keep the drift along a wide row far
/// below one f32 step.
fn box_mean(
    plane: &mut [f32],
    scratch: &mut [f32],
    (w, h): (usize, usize),
    r: usize,
    job: JobContext<'_>,
) -> Result<(), String> {
    for y in 0..h {
        job.check()?;
        let row = &plane[y * w..(y + 1) * w];
        let mut sum: f64 = row[..=r.min(w - 1)].iter().map(|v| f64::from(*v)).sum();
        for x in 0..w {
            let (left, right) = (x.saturating_sub(r), (x + r).min(w - 1));
            scratch[y * w + x] = (sum / (right - left + 1) as f64) as f32;
            if x >= r {
                sum -= f64::from(row[x - r]);
            }
            if x + r + 1 < w {
                sum += f64::from(row[x + r + 1]);
            }
        }
    }
    let mut sums = vec![0.0_f64; w];
    for y in 0..=r.min(h - 1) {
        for (sum, value) in sums.iter_mut().zip(&scratch[y * w..(y + 1) * w]) {
            *sum += f64::from(*value);
        }
    }
    for y in 0..h {
        job.check()?;
        let (top, bottom) = (y.saturating_sub(r), (y + r).min(h - 1));
        let count = (bottom - top + 1) as f64;
        for (out, sum) in plane[y * w..(y + 1) * w].iter_mut().zip(&sums) {
            *out = (sum / count) as f32;
        }
        if y >= r {
            for (sum, value) in sums.iter_mut().zip(&scratch[(y - r) * w..(y - r + 1) * w]) {
                *sum -= f64::from(*value);
            }
        }
        if y + r + 1 < h {
            for (sum, value) in sums
                .iter_mut()
                .zip(&scratch[(y + r + 1) * w..(y + r + 2) * w])
            {
                *sum += f64::from(*value);
            }
        }
    }
    Ok(())
}

/// Refines `coverage` (0…1, canvas-sized, row-major) against the guide
/// `guide_at(i)` — pixel `i`'s encoded RGB in 0…1 times its alpha — as the
/// module doc describes. `area` is the optional category plane (validated
/// here: [`area_includes`]). Returns a new plane; samples outside the write
/// area are bit-identical to the input. `Err`: a plane whose length is not
/// the canvas's, an invalid area value, or a cancelled job.
pub(crate) fn refine(
    guide_at: impl Fn(usize) -> [f32; 3],
    coverage: &[f32],
    area: Option<&[u8]>,
    dimensions: (u32, u32),
    parameters: EdgeRefine,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let n = w * h;
    if coverage.len() != n || area.is_some_and(|a| a.len() != n) {
        return Err("edge refinement plane length differs from the canvas".into());
    }
    if let Some(area) = area {
        area_includes(area)?;
    }
    let radius = parameters.radius;
    let mut out = coverage.to_vec();
    if n == 0 {
        return Ok(out);
    }
    // The write area and its bounding box.
    let mut write = vec![0_u8; n];
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    {
        let sdf = if radius > 0.0 {
            Some(signed_distance_from_membership(
                w,
                h,
                |i| coverage[i] >= 0.5,
                job,
            )?)
        } else {
            None
        };
        for (i, flag) in write.iter_mut().enumerate() {
            job.every(i)?;
            let chosen = match area.map_or(AREA_AUTO, |a| a[i]) {
                AREA_EXCLUDE => false,
                AREA_INCLUDE => true,
                _ => sdf.as_ref().is_some_and(|s| s[i].abs() <= radius),
            };
            if chosen {
                *flag = 1;
                let (x, y) = (i % w, i / w);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x0 >= x1 {
        return Ok(out);
    }
    job.report(0.1, 6)?;
    let r = (radius.ceil() as usize).max(4);
    let (x0, y0) = (x0.saturating_sub(2 * r), y0.saturating_sub(2 * r));
    let (x1, y1) = ((x1 + 2 * r).min(w), (y1 + 2 * r).min(h));
    let size = (x1 - x0, y1 - y0);
    let m = size.0 * size.1;
    let canvas = |i: usize| (i / size.0 + y0) * w + (i % size.0 + x0);
    let mut scratch = vec![0.0_f32; m];
    // The coverage's own window statistics, shared by the three channels.
    let mut mean_p: Vec<f32> = (0..m).map(|i| coverage[canvas(i)]).collect();
    let mut var_p: Vec<f32> = mean_p.iter().map(|p| p * p).collect();
    box_mean(&mut mean_p, &mut scratch, size, r, job)?;
    box_mean(&mut var_p, &mut scratch, size, r, job)?;
    for (variance, mean) in var_p.iter_mut().zip(&mean_p) {
        *variance = (*variance - mean * mean).max(0.0);
    }
    let mut weighted = vec![0.0_f32; m];
    let mut weight = vec![0.0_f32; m];
    let mut best = vec![0.0_f32; m];
    for channel in 0..3 {
        let stage = job.range(0.1 + 0.3 * channel as f64, 0.1 + 0.3 * (channel + 1) as f64);
        let guide: Vec<f32> = (0..m).map(|i| guide_at(canvas(i))[channel]).collect();
        // `first` is mean(I), then the confidence; `second` is mean(I²), then
        // b, then mean(b); `third` is mean(I·p), then a, then mean(a).
        let mut first = guide.clone();
        let mut second: Vec<f32> = guide.iter().map(|g| g * g).collect();
        let mut third: Vec<f32> = (0..m).map(|i| guide[i] * coverage[canvas(i)]).collect();
        box_mean(&mut first, &mut scratch, size, r, stage)?;
        box_mean(&mut second, &mut scratch, size, r, stage)?;
        box_mean(&mut third, &mut scratch, size, r, stage)?;
        stage.report(0.5, 6)?;
        for i in 0..m {
            stage.every(i)?;
            let covariance = third[i] - first[i] * mean_p[i];
            let variance = (second[i] - first[i] * first[i]).max(0.0);
            let a = covariance / (variance + EPSILON);
            third[i] = a;
            second[i] = mean_p[i] - a * first[i];
            first[i] = (covariance * covariance / ((variance + EPSILON) * (var_p[i] + DELTA)))
                .clamp(0.0, 1.0);
        }
        box_mean(&mut second, &mut scratch, size, r, stage)?;
        box_mean(&mut third, &mut scratch, size, r, stage)?;
        for i in 0..m {
            stage.every(i)?;
            let q = (third[i] * guide[i] + second[i]).clamp(0.0, 1.0);
            let k = first[i];
            let share = k * k + WEIGHT_FLOOR;
            weighted[i] += share * q;
            weight[i] += share;
            best[i] = best[i].max(k);
        }
        stage.report(1.0, 6)?;
    }
    for i in 0..m {
        job.every(i)?;
        let at = canvas(i);
        if write[at] == 0 {
            continue;
        }
        let q = weighted[i] / weight[i];
        out[at] = if parameters.smart {
            (coverage[at] + best[i] * (q - coverage[at])).clamp(0.0, 1.0)
        } else {
            q
        };
    }
    Ok(out)
}
