//! Radial Blur: Spin and Zoom, as the equal-weight
//! premultiplied mean of samples along each pixel's arc (Spin) or ray (Zoom)
//! about a centre placed in the FRAME — the selection's bounding box in plane
//! pixels, or the whole plane (`ops_spatial::Frame`). Pixels outside the frame
//! are not computed: the plane holds their premultiplied source, so with the
//! selection's zero coverage around them they are never written.
//!
//! With `c` the centre and `p` the pixel centre `(x + ½, y + ½)`, `r = |p − c|`:
//! - Spin samples at `c + R(θ_k)(p − c)`, `θ_k = Θ(t_k − ½)`, `Θ = amount ·
//!   SPIN_RADIANS_PER_AMOUNT`, over an arc of length `rΘ`.
//! - Zoom samples at `c + s_k(p − c)`, `s_k = 1 + Z(t_k − ½)`, `Z = amount ·
//!   ZOOM_SPAN_PER_AMOUNT`, over a ray of length `Zr`.
//!
//! The path is symmetric about the pixel, so the blur does not drift. The
//! sample count is `N = 1 + min(cap, ⌈length · density⌉)`: Good is bilinear at
//! one sample per pixel of path (cap 64), Best bilinear at two (cap 128), and
//! Draft nearest at a quarter (cap 16) with stratified jitter `t_k = (k +
//! u_k)/N`, `u_k` drawn positionally from `SplitMix64::at(RADIAL_DRAFT_SEED,
//! x, y, k)`. The jitter is Draft's visible grain, made deterministic, so a
//! replay is identical. Alpha is filtered, as in Box Blur, whose combine and
//! transparent floor this shares.
//!
//! Radial Blur, Clouds / Difference Clouds and Offset, three more `SpatialKernel`
//! arms on the same route (`SelectedFilter::Spatial` at 16 bits, `filter_op` at 8; no new export).
//! Radial Blur and Offset act within an `ops_spatial::Frame` — the selection's bounding box in
//! plane pixels, or the whole plane — derived at run time
//! and never recorded: `coverage_frame` over the u16 coverage at 16 bits (`selected_edit_unlocked`,
//! scalar `applying_unlocked` → `precision_scalar_filter::evaluate`'s `frame`, applied by
//! `SelectedFilter::with_frame`, which always replaces a parsed region; the 16-bit plans pass
//! `None`, a bound for any frame), the host's `region_x/y/width/height` at 8 bits (all four or
//! none; `plan` refuses one empty or outside the plane). Radial: the equal-weight premultiplied
//! mean along an arc (Spin, `SPIN_RADIANS_PER_AMOUNT`) or ray (Zoom, `ZOOM_SPAN_PER_AMOUNT`) about
//! a centre given as frame fractions; Good/Best bilinear at 1/2 samples per path pixel (caps
//! 64/128, steps tabled per N), Draft nearest at ¼ with `SplitMix64::at(RADIAL_DRAFT_SEED, x, y,
//! k)` stratified jitter; Box Blur's combine and alpha floor; pixels outside the frame hold their
//! premultiplied source; the plan bounds work per 64×64 tile at the farthest
//! tile-corner-to-frame-corner distance. Work constants (`RADIAL_*_WORK_PER_SAMPLE`,
//! `CLOUDS_WORK_PER_PIXEL`, `OFFSET_WORK_PER_PIXEL`) come from `tests/radial_clouds_offset.rs`'s
//! ignored probe ×1.15; the record is in `designs/phase-9f-selected-tone-status.md`.
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::ops_spatial::{frame_or_plane, Frame};
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::rng::SplitMix64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RadialMethod {
    Spin,
    Zoom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RadialQuality {
    Draft,
    Good,
    Best,
}

/// Spin's total arc per Amount unit: one degree, so Amount is the arc in
/// degrees (Amount 10 is a gentle turn, 100 a heavy one).
pub(crate) const SPIN_RADIANS_PER_AMOUNT: f64 = std::f64::consts::PI / 180.0;
/// Zoom's radial span per Amount unit, as a fraction of the distance from the
/// centre: Amount 100, the top of the 1…100 range, smears each pixel over
/// half its radius, ±25% about it; Amount 10 is a 5% streak.
pub(crate) const ZOOM_SPAN_PER_AMOUNT: f64 = 0.005;
/// The fixed stream Draft's jitter is drawn from. Fixed rather than a
/// parameter because Draft's grain is a look, not a choice: the same picture
/// and settings always give the same grain, so an Action replays exactly.
pub(crate) const RADIAL_DRAFT_SEED: u64 = 0x5241_4449_414C_4446;
/// Work units per Draft sample (nearest read, one positional hash), and per
/// Good/Best sample (2×2 bilinear read). Frozen from the `#[ignore]`d
/// `radial_clouds_offset_calibration_probe` (`tests/radial_clouds_offset.rs`):
/// units per PLANNED sample on opaque noise at the Gaussian σ3.9 reference's
/// r16-unit rate, min of 3, the larger of the 8-bit `dr_image_filter_op` and
/// the whole 16-bit `selected_edit`, the largest over two runs, times 1.15.
/// Bilinear (run 1, 6000×4000, 1.1507e9 units/s): Spin Good a10 21.178 s /
/// 42.737 s (15.62 / 31.52), Zoom Good a10 18.789 s / 33.007 s (13.86 /
/// 24.35), Spin Best a10 40.063 s / 75.838 s (14.89 / 28.19); run 2 (refused
/// at 24 MP, so 2400×1600): Good 3.137 s / 5.620 s (14.49 / 25.96), Best
/// 6.006 s / 10.727 s (13.98 / 24.97) → 31.52 × 1.15 = 37. Nearest (6000×4000):
/// Spin Draft a10 run 1 6.076 s / 9.441 s (17.14 / 26.63), run 2 6.083 s /
/// 10.122 s (17.19 / 28.60); Zoom Draft a100 3.555 s / 8.202 s, 3.631 s /
/// 8.602 s (up to 24.31) → 28.60 × 1.15 = 33. The 16-bit tiled read sets both.
/// At 24 MP Good and Best at Amount 10 are refused (5.77e10 and 1.15e11 of
/// 2.4e10; Good fits about 10 MP, Best about 5 MP) and Draft is admitted even
/// at Amount 100 (1.35e10). The record is in
/// `designs/phase-9f-selected-tone-status.md`.
pub(crate) const RADIAL_NEAREST_WORK_PER_SAMPLE: u64 = 33;
pub(crate) const RADIAL_BILINEAR_WORK_PER_SAMPLE: u64 = 37;

/// Tile edge of the plan's work bound: small enough that a tile's farthest
/// corner distance is close to its pixels' own, large enough that the sum is
/// cheap at any size.
const PLAN_TILE: u32 = 64;

/// `(interpolate, density, cap)` for a quality.
fn quality_terms(quality: RadialQuality) -> (bool, f64, u64) {
    match quality {
        RadialQuality::Draft => (false, 0.25, 16),
        RadialQuality::Good => (true, 1.0, 64),
        RadialQuality::Best => (true, 2.0, 128),
    }
}

/// The path length per unit of radius.
fn length_per_radius(amount: u32, method: RadialMethod) -> f64 {
    match method {
        RadialMethod::Spin => f64::from(amount) * SPIN_RADIANS_PER_AMOUNT,
        RadialMethod::Zoom => f64::from(amount) * ZOOM_SPAN_PER_AMOUNT,
    }
}

/// `N = 1 + min(cap, ⌈length · density⌉)`.
fn sample_count(length: f64, density: f64, cap: u64) -> u64 {
    1 + ((length * density).ceil().max(0.0) as u64).min(cap)
}

/// `(scratch, work)` refused before any allocation. The work is a bound for
/// ANY centre inside the frame: per 64×64 tile, its pixels times the sample
/// count at the largest tile-corner-to-frame-corner distance (no pixel of the
/// tile is farther than that from any point of the frame), so the 16-bit plans
/// that pass no frame still bound the run.
pub(crate) fn radial_plan(
    dimensions: (u32, u32),
    amount: u32,
    method: RadialMethod,
    quality: RadialQuality,
    frame: Option<Frame>,
    cap: u64,
) -> Result<(u64, u64), String> {
    let n = pixel_count(dimensions.0, dimensions.1)? as u64;
    if !(1..=100).contains(&amount) {
        return Err("radial blur amount must be an integer in [1,100]".into());
    }
    let f = frame_or_plane(frame, dimensions)?;
    let (_, density, sample_cap) = quality_terms(quality);
    let per_sample = match quality {
        RadialQuality::Draft => RADIAL_NEAREST_WORK_PER_SAMPLE,
        _ => RADIAL_BILINEAR_WORK_PER_SAMPLE,
    };
    let per_radius = length_per_radius(amount, method);
    let corners = [
        (f64::from(f.x), f64::from(f.y)),
        (f64::from(f.x + f.width), f64::from(f.y)),
        (f64::from(f.x), f64::from(f.y + f.height)),
        (f64::from(f.x + f.width), f64::from(f.y + f.height)),
    ];
    let mut work = 0_u64;
    for ty in (f.y..f.y + f.height).step_by(PLAN_TILE as usize) {
        let th = PLAN_TILE.min(f.y + f.height - ty);
        for tx in (f.x..f.x + f.width).step_by(PLAN_TILE as usize) {
            let tw = PLAN_TILE.min(f.x + f.width - tx);
            let mut far = 0.0_f64;
            for (px, py) in [(tx, ty), (tx + tw, ty), (tx, ty + th), (tx + tw, ty + th)] {
                for (cx, cy) in corners {
                    far = far.max((f64::from(px) - cx).hypot(f64::from(py) - cy));
                }
            }
            let samples = sample_count(far * per_radius, density, sample_cap);
            work = work.saturating_add(
                (u64::from(tw) * u64::from(th))
                    .saturating_mul(samples)
                    .saturating_mul(per_sample),
            );
        }
        if work > cap {
            return Err("radial blur exceeds the bounded pixel-work limit".into());
        }
    }
    Ok((n * 16 + 2 * BOOKKEEPING_BYTES, work))
}

/// The premultiplied `[r·a, g·a, b·a, a]` of a straight sample.
#[inline(always)]
pub(crate) fn premultiplied(p: [f32; 4]) -> [f32; 4] {
    [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
}

/// The premultiplied sample at continuous position `(qx, qy)` (pixel centres
/// at half-integers), nearest or 2×2 bilinear with clamped plane edges. The
/// bilinear weights are `sampling::Kernel::Bilinear`'s tent (the master copy);
/// it is inlined here because the path takes up to 129 samples a pixel and the
/// general resampler's per-call setup would dominate. Positions and weights
/// are f32 — a resolution of 2^-9 px at 16384 px from the origin, below what
/// a mean of many bilinear samples can show — because the four-lane f32 body
/// is what keeps a sample cheap (measured 25.4 s → 21.6 s for 24 MP Spin Good
/// at Amount 10 against binary64); the path mean is summed in binary64.
#[inline(always)]
pub(crate) fn sample(
    read: &impl Fn(usize) -> [f32; 4],
    (w, h): (usize, usize),
    (qx, qy): (f32, f32),
    interpolate: bool,
) -> [f32; 4] {
    let (wmax, hmax) = (w as i32 - 1, h as i32 - 1);
    if !interpolate {
        // The pixel whose square holds the point.
        let (x, y) = (
            (qx.floor() as i32).clamp(0, wmax),
            (qy.floor() as i32).clamp(0, hmax),
        );
        return premultiplied(read(y as usize * w + x as usize));
    }
    let (sx, sy) = (qx - 0.5, qy - 0.5);
    let (x0, y0) = (sx.floor(), sy.floor());
    let (fx, fy) = (sx - x0, sy - y0);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let (xa, xb) = (x0.clamp(0, wmax) as usize, (x0 + 1).clamp(0, wmax) as usize);
    let (ya, yb) = (
        y0.clamp(0, hmax) as usize * w,
        (y0 + 1).clamp(0, hmax) as usize * w,
    );
    let (p00, p01) = (premultiplied(read(ya + xa)), premultiplied(read(ya + xb)));
    let (p10, p11) = (premultiplied(read(yb + xa)), premultiplied(read(yb + xb)));
    let (w00, w01) = ((1.0 - fx) * (1.0 - fy), fx * (1.0 - fy));
    let (w10, w11) = ((1.0 - fx) * fy, fx * fy);
    std::array::from_fn(|c| p00[c] * w00 + p01[c] * w01 + p10[c] * w10 + p11[c] * w11)
}

/// [`sample`] with binary64 positions and weights: what a 32-bit float layer
/// samples with (`SampleRange::Extended`). The tent is the same; only the
/// arithmetic is wider, for two reasons a 0…1 picture never shows.
///
/// - An f32 position 4096 px from the origin is rounded by up to 2^-12 px,
///   and the tent turns a position error straight into a weight on the
///   neighbouring pixel. Beside a highlight of 5000 that weight is worth 1.2,
///   twelve times a 0.1 surround. Measured 4150 px out
///   (`tests/float_filters_highlight.rs`, 2026-10-12, the bound being 2 % of
///   the f64 reference at every pixel): Spin Good missed at 21.3 % beside one
///   5000 pixel and 8.8 % beside a 5000 disc with f32 positions, and Motion
///   Blur stood at 0.30 %.
/// - Four f32 weights do not sum to exactly 1, so the mean of an OPAQUE
///   picture came out at alpha 0.99999994 in places. A 16-bit code rounds
///   that to opaque; a float layer would store it, and an opaque layer would
///   stop being one. In binary64 the sum is 1 to within 1e-15, which the f32
///   the plane stores rounds to exactly 1.
///
/// It costs about a sixth more time than the f32 body ([`sample`]'s
/// measurement), which the work constants' 1.15 margin does not cover and
/// does not need to: a float filter runs under a job control, with no work
/// ceiling.
#[inline(always)]
pub(crate) fn sample_exact(
    read: &impl Fn(usize) -> [f32; 4],
    (w, h): (usize, usize),
    (qx, qy): (f64, f64),
    interpolate: bool,
) -> [f64; 4] {
    let (wmax, hmax) = (w as i64 - 1, h as i64 - 1);
    let at = |i: usize| premultiplied(read(i)).map(f64::from);
    if !interpolate {
        let (x, y) = (
            (qx.floor() as i64).clamp(0, wmax),
            (qy.floor() as i64).clamp(0, hmax),
        );
        return at(y as usize * w + x as usize);
    }
    let (sx, sy) = (qx - 0.5, qy - 0.5);
    let (x0, y0) = (sx.floor(), sy.floor());
    let (fx, fy) = (sx - x0, sy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (xa, xb) = (x0.clamp(0, wmax) as usize, (x0 + 1).clamp(0, wmax) as usize);
    let (ya, yb) = (
        y0.clamp(0, hmax) as usize * w,
        (y0 + 1).clamp(0, hmax) as usize * w,
    );
    let (p00, p01, p10, p11) = (at(ya + xa), at(ya + xb), at(yb + xa), at(yb + xb));
    let (w00, w01) = ((1.0 - fx) * (1.0 - fy), fx * (1.0 - fy));
    let (w10, w11) = ((1.0 - fx) * fy, fx * fy);
    std::array::from_fn(|c| p00[c] * w00 + p01[c] * w01 + p10[c] * w10 + p11[c] * w11)
}

/// The path's linear map for parameter `t`: `(a, b)` with the sample at
/// `c + (a·dx − b·dy, b·dx + a·dy)` — a rotation by `Θ(t − ½)` for Spin, a
/// scale by `1 + Z(t − ½)` for Zoom.
fn path_step(method: RadialMethod, per_radius: f64, t: f64) -> (f64, f64) {
    match method {
        RadialMethod::Spin => {
            let (sin, cos) = (per_radius * (t - 0.5)).sin_cos();
            (cos, sin)
        }
        RadialMethod::Zoom => (1.0 + per_radius * (t - 0.5), 0.0),
    }
}

/// The plane: inside the frame, the premultiplied mean along the path (below
/// `range.alpha_floor()` of alpha it is transparent black, `box_plane`'s
/// floor: half a 16-bit code at 8 and 16 bits, 2^-24 in a float layer);
/// outside it, the premultiplied source. `ops_spatial`'s Box Blur combine
/// turns either back to straight samples. A float layer (`Extended`) places
/// and weighs each sample in binary64 ([`sample_exact`]); 8 and 16 bits keep
/// [`sample`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn radial_plane(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    amount: u32,
    method: RadialMethod,
    quality: RadialQuality,
    center: (f64, f64),
    frame: Option<Frame>,
    range: &SampleRange,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let f = frame_or_plane(frame, dimensions)?;
    let alpha_floor = range.alpha_floor();
    let exact = matches!(range, SampleRange::Extended { .. });
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let (interpolate, density, cap) = quality_terms(quality);
    let per_radius = length_per_radius(amount, method);
    let cx = f64::from(f.x) + center.0 * f64::from(f.width);
    let cy = f64::from(f.y) + center.1 * f64::from(f.height);
    let unit = (1_u64 << 53) as f64;
    // Good and Best place sample k of N at t = k/(N−1) for every pixel, so
    // each N's steps are computed once: row N holds N steps from N(N−1)/2.
    let steps: Vec<(f64, f64)> = if interpolate {
        (1..=cap + 1)
            .flat_map(|n| {
                (0..n).map(move |k| {
                    let t = if n == 1 {
                        0.5
                    } else {
                        k as f64 / (n - 1) as f64
                    };
                    path_step(method, per_radius, t)
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut out = reserved_vec(w * h)?;
    for y in 0..h {
        job.check()?;
        let inside_row = y >= f.y as usize && y < (f.y + f.height) as usize;
        for x in 0..w {
            let i = y * w + x;
            job.every(i)?;
            if !inside_row || x < f.x as usize || x >= (f.x + f.width) as usize {
                out.push(premultiplied(read(i)));
                continue;
            }
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let count = sample_count(dx.hypot(dy) * per_radius, density, cap);
            let mut acc = [0.0_f64; 4];
            for k in 0..count {
                let (a, b) = if interpolate {
                    steps[(count * (count - 1) / 2 + k) as usize]
                } else {
                    let mut jitter = SplitMix64::at(RADIAL_DRAFT_SEED, x as u64, y as u64, k);
                    let t = (k as f64 + (jitter.next_u64() >> 11) as f64 / unit) / count as f64;
                    path_step(method, per_radius, t)
                };
                let q = (cx + a * dx - b * dy, cy + b * dx + a * dy);
                let p = if exact {
                    sample_exact(&read, (w, h), q, interpolate)
                } else {
                    sample(&read, (w, h), (q.0 as f32, q.1 as f32), interpolate).map(f64::from)
                };
                for c in 0..4 {
                    acc[c] += p[c];
                }
            }
            let mean = acc.map(|v| v / count as f64);
            out.push(if mean[3] < alpha_floor {
                [0.0; 4]
            } else {
                mean.map(|v| v as f32)
            });
        }
        job.report((y + 1) as f64 / h as f64, 6)?;
    }
    Ok(out)
}
