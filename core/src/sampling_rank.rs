//! Versioned, cancellable source-filter RANK sampling: the per-channel lower
//! median over a lattice disc. `sampling_gaussian` stays the master copy of the
//! clamped-edge law and this module follows it rather than re-deriving it, but
//! the two neighborhoods differ in one way that matters: a rank statistic
//! cannot be weighted without becoming a different operator, so this plane
//! samples STRAIGHT colour and alpha decides MEMBERSHIP — a transparent
//! neighbour is excluded from the vote rather than contributing a zero-weighted
//! sample, and a neighbour at one code of alpha votes as loudly as an opaque
//! one.
//!
//! [`median_band`] is the one implementation: [`median_bounded`] runs it over
//! the whole plane.
//!
//! `dust_and_scratches` (threshold) reads `sampling_rank.rs` — a straight per-channel LOWER median
//! over the lattice disc `dx²+dy² ≤ fl(r²)`, where alpha decides MEMBERSHIP rather than weight and
//! a transparent neighbour never votes; threshold0 is a plain median, not a bypass, and the disc is
//! exact, so its agreement with the oracle is an equality check rather than a tolerance.
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use std::ops::Range;

const RADIUS_RANGE: &str = "rank filter radius must be finite and in [0,64] source pixels";

/// The lattice disc `D(r) = { (dx,dy) in Z^2 : dx^2 + dy^2 <= fl(r*r) }`, in
/// ascending (dy, dx) order.
///
/// `fl(r*r)` is ONE binary64 round-to-nearest product, computed here and
/// nowhere else. `dx^2 + dy^2 <= 8192` for r <= 64 is exactly representable in
/// binary64, so that product is the only rounding in the predicate; and
/// round-to-nearest is monotone, so `r1 <= r2` implies `fl(r1*r1) <= fl(r2*r2)`
/// and therefore `D(r1)` is a subset of `D(r2)` at EVERY value, including the
/// ulp neighbourhood of an irrational square root. The support can never shrink
/// as the radius grows.
///
/// A disc rather than a square: a square median at radius r reaches
/// `sqrt(2)*r` diagonally, so it erases features about 41% larger along the
/// diagonals — a directional bias on exactly the fine structure this operator
/// exists to preserve. The Gaussian family here is isotropic and a stack should
/// not switch geometry families mid-chain.
pub fn disc_offsets(radius: f64) -> Vec<(i32, i32)> {
    let limit = radius * radius;
    let half = radius.floor() as i32;
    let mut out = Vec::new();
    for dy in -half..=half {
        for dx in -half..=half {
            if f64::from(dx * dx + dy * dy) <= limit {
                out.push((dx, dy));
            }
        }
    }
    out
}

/// Work units per disc neighbour: the gather (two clamps, an index, an alpha
/// load, a test and three stores) plus three introselects at their expected
/// comparison count.
///
/// Both rank constants are MEASURED (the ignored probe in
/// `tests/banded_reduce_noise.rs`): byte `dr_image_filter_op` and the 16-bit
/// selected edit at threshold 0, r ∈ {1,2,3,4,6,8} on 3000×2000 noise (r8 on
/// its first 1500 rows), timed in the radius-16 unit. Fitting each path's
/// units per pixel to `a·k + b` gave byte 8.04·k + 263 and 16-bit
/// 12.29·k + 286: a disc measured less per neighbour than the former 24 and
/// far more per pixel, so the former model under-charged r1–r2.
/// Taking the larger a and b, scaling by the worst point's ratio (1.108) and
/// the 1.15 safety factor gives these two figures, which cover every measured
/// point; the 24 MP ceiling stays r3.5.
pub(crate) const RANK_NEIGHBOUR_WORK: u64 = 16;
/// Work units per pixel: the scratch clear, the centre load and the store
/// (calibrated with [`RANK_NEIGHBOUR_WORK`]).
pub(crate) const RANK_PIXEL_WORK: u64 = 364;

pub(crate) fn plan_bounded(
    width: u32,
    height: u32,
    radius: f64,
    cap: u64,
) -> Result<(u64, u64), String> {
    if !radius.is_finite() || !(0.0..=64.0).contains(&radius) {
        return Err(RADIUS_RANGE.into());
    }
    let n = pixel_count(width, height)? as u64;
    if radius < 1.0 {
        // Below one pixel the disc holds the centre alone, so the median IS the
        // sample: the same exact-bypass shape the Gaussian declares at sigma 0.
        return Ok((n * 16 + BOOKKEEPING_BYTES, n));
    }
    // The offset list is the one definition of the support, so the forecast
    // reads it rather than re-deriving the count. It is bounded by 129x129
    // pairs at the maximum radius and holds no pixels.
    let k = disc_offsets(radius).len() as u64;
    let work = n.saturating_mul(RANK_NEIGHBOUR_WORK * k + RANK_PIXEL_WORK);
    if work > cap {
        return Err("rank source filter exceeds the bounded pixel-work limit".into());
    }
    // ONE straight float plane — the rank pass reads the caller's plane in
    // place and has no separable intermediate, unlike the Gaussian's two — plus
    // the bounded offset list and three per-pixel scratch vectors of k samples.
    Ok((n * 16 + k * 8 + k * 12 + 2 * BOOKKEEPING_BYTES, work))
}

/// The whole-plane median: [`plan_bounded`]'s refusal, then [`median_band`]
/// over every row — the one implementation.
pub(crate) fn median_bounded(
    width: u32,
    height: u32,
    radius: f64,
    cap: u64,
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    plan_bounded(width, height, radius, cap)?;
    median_band(width, height, radius, cap, 0..height, read, job)
}

/// Rows `rows` (start < end ≤ h; any start, no alignment rule) of exactly
/// [`median_bounded`]'s plane, bit for bit: every output pixel gathers its
/// own clamped disc, so a band is the whole-plane loop over a row range.
/// `read` takes the ABSOLUTE row-major index and is only called for rows
/// within floor(radius) of `rows`, clamped to [0, h). Below radius 1 each
/// pixel polls `job.every` at its absolute index; from radius 1 up each row
/// checks, each pixel polls `job.every(x)` and each row reports
/// (y − start + 1)/len at stage 6 — over 0..h exactly the whole-plane calls.
pub(crate) fn median_band(
    width: u32,
    height: u32,
    radius: f64,
    cap: u64,
    rows: Range<u32>,
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    plan_bounded(width, height, radius, cap)?;
    if rows.start >= rows.end || rows.end > height {
        return Err("rank band rows lie outside the plane".into());
    }
    let (w, h) = (width as usize, height as usize);
    let (r0, r1) = (rows.start as usize, rows.end as usize);
    let len = r1 - r0;
    let mut out = reserved_vec(len * w)?;
    if radius < 1.0 {
        for i in r0 * w..r1 * w {
            job.every(i)?;
            out.push(read(i));
        }
        return Ok(out);
    }
    let offsets = disc_offsets(radius);
    let mut channels: [Vec<f32>; 3] = [
        reserved_vec(offsets.len())?,
        reserved_vec(offsets.len())?,
        reserved_vec(offsets.len())?,
    ];
    for y in r0..r1 {
        job.check()?;
        for x in 0..w {
            job.every(x)?;
            for values in channels.iter_mut() {
                values.clear();
            }
            for &(dx, dy) in &offsets {
                // Clamped edges, exactly as the Gaussian extends them. A
                // clamped duplicate votes once per offset and is deliberately
                // NOT deduplicated: a position-dependent support count would
                // break both the rank and the O(1)-per-neighbour work model.
                let sx = (x as i64 + i64::from(dx)).clamp(0, w as i64 - 1) as usize;
                let sy = (y as i64 + i64::from(dy)).clamp(0, h as i64 - 1) as usize;
                let sample = read(sy * w + sx);
                if sample[3] > 0.0 {
                    for (c, values) in channels.iter_mut().enumerate() {
                        values.push(sample[c]);
                    }
                }
            }
            let centre = read(y * w + x);
            let members = channels[0].len();
            if members == 0 {
                // Only reachable at a transparent centre, whose combine step
                // returns the source before it ever reads this plane.
                out.push(centre);
                continue;
            }
            // The LOWER order statistic. `|D(r)|` is always odd, so an even
            // count means the alpha exclusion removed an odd number of
            // neighbours; averaging the two middle elements there would invent
            // a value no neighbour had and destroy the membership property the
            // numerical gate rests on. `total_cmp` rather than a partial
            // comparison: an upstream stage could in principle produce a NaN,
            // and a rank filter must order it rather than panic.
            let mut value = centre;
            for (c, values) in channels.iter_mut().enumerate() {
                let (_, median, _) =
                    values.select_nth_unstable_by((members - 1) / 2, f32::total_cmp);
                value[c] = *median;
            }
            out.push(value);
        }
        job.report((y - r0 + 1) as f64 / len as f64, 6)?;
    }
    Ok(out)
}
