//! Merge to HDR's pixel math: the exposure-weighted scene-linear merge
//! of 2…16 frames of one scene, the estimate of their relative exposures when
//! no metadata gives them, and the ghost test. Free functions over
//! [`FrameRows`], which hands out one canvas row of one frame at a time as
//! linear light; this module names no document type and `doc_merge_hdr` is the
//! adapter that reads the rows from layers.
//!
//! # The merge
//!
//! A frame's sample at a pixel carries its linear RGB, its `level` (the
//! largest STORED channel over full scale, so one clipped channel is seen
//! before the transfer curve hides it) and its alpha. With `e_i = ev_i −
//! ev_reference` (stops; larger = more light), over the frames with alpha > 0:
//!
//! ```text
//! w_i = alpha_i · weight(level_i) · 2^e_i
//! rgb = Σ w_i · linear_i / 2^e_i  ÷  Σ w_i
//! ```
//!
//! accumulated in f64 and rounded once to f32, so the result is the scene's
//! radiance on the reference frame's scale. `linear_i / 2^e_i` is frame `i`'s
//! own estimate of that radiance. The `2^e_i` in the weight is the
//! photon-noise weight: a frame that collected `2^e` times the light has
//! `2^e` times less variance in that estimate, so the longest exposure that is
//! not clipped decides a shadow and the shortest decides a highlight. Output
//! alpha is the largest frame alpha, and a pixel no frame covers is
//! `[0, 0, 0, 0]`.
//!
//! When `Σ w_i` is 0 no frame is trusted there, and the least bad one is
//! taken alone: where every covering frame is at or above [`LEVEL_CLIP`], the
//! SHORTEST exposure (the one closest to not clipping), counted in
//! `all_clipped`; otherwise the LONGEST exposure among the covering frames
//! below `LEVEL_CLIP` (the one with the most signal in a shadow every frame
//! lost), counted in `all_dark`. Ties go to the first listed frame. A sample
//! with a non-finite component is treated as alpha 0, so no NaN or infinity
//! can enter a sum.
//!
//! # The lattice
//!
//! Exposure estimation and the ghost test need statistics of the frames
//! against each other, not every pixel: one pre-pass reads a lattice of rows
//! and columns `stride = ceil(sqrt(w·h / LATTICE_SAMPLES))` apart, starting at
//! 0. At a lattice sample a frame is WELL EXPOSED when its weight is exactly 1
//! and its alpha exactly 1; a sample is JOINT for two frames when both are.
//! `Y` is the mean of the three linear channels.
//!
//! [`estimate_evs`] orders the frames by their median `level` (darkest first:
//! neighbours in that order overlap the most), takes for each neighbouring
//! pair the median of `log2(Y_b / Y_a)` over their joint samples, accumulates
//! along the order and answers relative to frame 0. A pair with fewer than
//! [`MIN_PAIR_SAMPLES`] joint samples has no estimate, and the answer names it
//! ([`Estimate::Unpaired`]) rather than guessing.
//!
//! # Ghosts
//!
//! With `deghost`, each non-reference frame `i` is measured against the
//! reference over their joint lattice samples: `r = ln Y_i − ln Y_ref − e_i ·
//! ln 2` is 0 for a static pixel and exact EVs, `m_i` is the median of `r`
//! (the frame's constant offset: an EV that is a little wrong, a flicker),
//! `sigma_i = 1.4826 · median |r − m_i|` is the robust noise of a static
//! pixel, and `tau_i = max(DEGHOST_SIGMAS · sigma_i, ln(1 + DEGHOST_FLOOR))`.
//! A frame with fewer than `MIN_PAIR_SAMPLES` joint samples is not judged. In
//! the merge, frame `i` is judged at a pixel only where it and the reference
//! are JOINT there, the population `sigma_i` was measured on; a pixel where
//! any judged frame has `|r − m_i| > tau_i` takes the reference's linear value
//! alone. The residual is centred on `m_i`, so a constant error in a frame's
//! EV moves `m_i` and not the verdict. Outside the joint region nothing is
//! judged: the noise estimate does not hold there.

use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;

/// The fewest frames a merge takes: one frame has nothing to merge with.
pub const MIN_FRAMES: usize = 2;

/// The most frames a merge takes. A bracket of the whole admitted span
/// ([`EV_SPAN_MAX`]) at one-stop steps is 16 frames, and the merge holds one
/// row of every frame at once, so the count is what bounds that buffer.
pub const MAX_FRAMES: usize = 16;

/// The largest distance, in stops, between any frame and the reference. A
/// stored sample is at most 1.0 and the merge divides it by `2^e`, so at 15
/// stops the largest merged value is 2^15 = 32768, inside the float
/// document's ±65504 (`float_document::FLOAT_RANGE_MAX`); one more stop would
/// not be.
pub const EV_SPAN_MAX: f64 = 15.0;

/// At or below this level a sample has no weight. 2% of full scale is five
/// codes of an 8-bit file: quantization and read noise are a large share of
/// the value, and a longer frame, when there is one, holds the same radiance
/// far above them.
pub const LEVEL_DARK: f32 = 0.02;

/// The bottom of the fully trusted band; the weight rises linearly from
/// [`LEVEL_DARK`] to here so no seam appears where a frame starts to count.
/// At 10% of full scale an 8-bit sample is 25 codes, where one code is under
/// 4% of the value.
pub const LEVEL_LOW: f32 = 0.10;

/// The top of the fully trusted band; the weight falls linearly from here to
/// [`LEVEL_CLIP`]. Above 90% a camera's response starts to shoulder and a
/// compressed file's ringing can reach full scale, so the trust is withdrawn
/// gradually rather than at the clip point.
pub const LEVEL_HIGH: f32 = 0.90;

/// At or above this level a sample counts as clipped and has no weight. Not
/// 1.0: a channel that clipped on the sensor is often stored a code or two
/// under full scale after white balance and compression, so 2% of headroom is
/// treated as clipped too.
pub const LEVEL_CLIP: f32 = 0.98;

/// The lattice's target sample count per frame, 512². A median over a quarter
/// of a million samples is steady to a few thousandths of the spread, the
/// pre-pass reads at most this many pixels of each frame whatever the canvas
/// size, and at 12 bytes a sample 16 frames keep about 50 MB.
pub const LATTICE_SAMPLES: u64 = 262_144;

/// The fewest joint samples a pair of frames needs before a median over them
/// is believed. The median of 64 samples is within about a sixth of their
/// spread; with fewer, the two frames share almost no well-exposed region and
/// a number would be a guess.
pub const MIN_PAIR_SAMPLES: usize = 64;

/// How many robust standard deviations a pixel's residual must exceed to be a
/// ghost. At 4 sigma a static pixel under Gaussian noise is misjudged about
/// 6 times in 100,000, and a misjudged pixel only falls back to the
/// reference, while a subject that moved differs by many times the noise.
pub const DEGHOST_SIGMAS: f64 = 4.0;

/// The smallest relative radiance difference that can be a ghost, whatever
/// the measured noise. Noiseless frames (synthetic, or heavily denoised)
/// measure a sigma of 0, and then quantization alone would read as movement:
/// at the bottom of the trusted band an 8-bit code is about 4% of the value
/// in linear light, so two frames can differ by nearly 9% with nothing
/// moving. 10% is above that.
pub const DEGHOST_FLOOR: f64 = 0.10;

/// The factor that turns a median absolute deviation into the standard
/// deviation of a normal distribution (1 / Φ⁻¹(3/4)).
const MAD_TO_SIGMA: f64 = 1.4826;

/// One frame's sample at one canvas pixel.
#[derive(Clone, Copy, Default)]
pub struct FrameSample {
    /// Linear-light RGB, straight (not multiplied by alpha).
    pub linear: [f32; 3],
    /// The largest stored channel over full scale, in [0, 1].
    pub level: f32,
    /// Coverage in [0, 1]; 0 where the frame has no pixel.
    pub alpha: f32,
}

/// The frames of one merge on one canvas grid.
pub trait FrameRows {
    /// How many frames there are.
    fn frames(&self) -> usize;
    /// The canvas, `(width, height)`.
    fn size(&self) -> (u32, u32);
    /// Canvas row `y` of `frame` into `out`, whose length is the width. Every
    /// element is written; alpha is 0 where the frame has no pixel.
    fn row(&self, frame: usize, y: u32, out: &mut [FrameSample]);
}

/// How far a sample at `level` is trusted, in [0, 1]: 0 at or below
/// [`LEVEL_DARK`] and at or above [`LEVEL_CLIP`], 1 on [[`LEVEL_LOW`],
/// [`LEVEL_HIGH`]], linear between. A NaN level has no weight.
pub fn weight(level: f32) -> f32 {
    if level.is_nan() || level <= LEVEL_DARK || level >= LEVEL_CLIP {
        0.0
    } else if level < LEVEL_LOW {
        (level - LEVEL_DARK) / (LEVEL_LOW - LEVEL_DARK)
    } else if level <= LEVEL_HIGH {
        1.0
    } else {
        (LEVEL_CLIP - level) / (LEVEL_CLIP - LEVEL_HIGH)
    }
}

/// Whether the frame has a usable pixel here: alpha above 0 and every
/// component finite.
fn covering(sample: &FrameSample) -> bool {
    sample.alpha > 0.0
        && sample.alpha.is_finite()
        && sample.level.is_finite()
        && sample.linear.iter().all(|v| v.is_finite())
}

/// Whether the sample is WELL EXPOSED: covering, weight exactly 1, alpha
/// exactly 1.
fn well_exposed(sample: &FrameSample) -> bool {
    covering(sample) && sample.alpha == 1.0 && weight(sample.level) == 1.0
}

/// `Y`: the mean of the three linear channels.
fn mean_linear(sample: &FrameSample) -> f64 {
    sample.linear.iter().map(|v| f64::from(*v)).sum::<f64>() / 3.0
}

/// The median of `values`, which it sorts: the middle element, or the mean of
/// the two middle ones for an even count. `None` for no values.
fn median(values: &mut [f64]) -> Option<f64> {
    values.sort_unstable_by(f64::total_cmp);
    let n = values.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(values[n / 2]),
        _ => Some((values[n / 2 - 1] + values[n / 2]) / 2.0),
    }
}

/// The lattice's step along both axes.
fn lattice_stride(width: u32, height: u32) -> u32 {
    let ratio = f64::from(width) * f64::from(height) / LATTICE_SAMPLES as f64;
    (ratio.sqrt().ceil() as u32).max(1)
}

/// The lattice's sample count per frame: positions 0, stride, 2·stride, …
/// along each axis.
fn lattice_count(width: u32, height: u32) -> u64 {
    let stride = lattice_stride(width, height);
    u64::from(width.div_ceil(stride)) * u64::from(height.div_ceil(stride))
}

/// The lattice statistics of every frame: per sample, `Y` where the frame is
/// well exposed (NaN elsewhere) and the level where it covers (NaN elsewhere).
struct Lattice {
    count: usize,
    y: Vec<Vec<f64>>,
    level: Vec<Vec<f32>>,
}

impl Lattice {
    fn read(rows: &dyn FrameRows, job: JobContext<'_>) -> Result<Self, String> {
        let frames = rows.frames();
        let (width, height) = rows.size();
        let stride = lattice_stride(width, height) as usize;
        let count = usize::try_from(lattice_count(width, height))
            .map_err(|_| "merge to hdr lattice exceeds addressable memory".to_string())?;
        let mut y = Vec::with_capacity(frames);
        let mut level = Vec::with_capacity(frames);
        for _ in 0..frames {
            y.push(reserved_vec::<f64>(count)?);
            level.push(reserved_vec::<f32>(count)?);
        }
        let mut row: Vec<FrameSample> = reserved_vec(width as usize)?;
        row.resize(width as usize, FrameSample::default());
        for line in (0..height).step_by(stride) {
            job.report(f64::from(line) / f64::from(height.max(1)), 1)?;
            for frame in 0..frames {
                rows.row(frame, line, &mut row);
                for sample in row.iter().step_by(stride) {
                    y[frame].push(if well_exposed(sample) {
                        mean_linear(sample)
                    } else {
                        f64::NAN
                    });
                    level[frame].push(if covering(sample) {
                        sample.level
                    } else {
                        f32::NAN
                    });
                }
            }
        }
        Ok(Self { count, y, level })
    }

    /// `f(Y_a, Y_b)` over the joint samples of frames `a` and `b`, into
    /// `out`.
    fn joint(&self, a: usize, b: usize, out: &mut Vec<f64>, f: impl Fn(f64, f64) -> f64) {
        out.clear();
        for (ya, yb) in self.y[a].iter().zip(&self.y[b]) {
            if !ya.is_nan() && !yb.is_nan() {
                out.push(f(*ya, *yb));
            }
        }
    }
}

/// What [`estimate_evs`] found.
pub enum Estimate {
    /// One EV per frame, relative to frame 0 (`evs[0]` is 0).
    Evs(Vec<f64>),
    /// Frames `a` and `b`, neighbours in median-level order, have fewer than
    /// [`MIN_PAIR_SAMPLES`] joint samples, so there is no estimate.
    Unpaired { a: usize, b: usize },
}

/// The frames' relative exposures from their own pixels (module doc, "The
/// lattice"). `Err` only for a cancel or a failed allocation.
pub fn estimate_evs(rows: &dyn FrameRows, job: JobContext<'_>) -> Result<Estimate, String> {
    let frames = rows.frames();
    let lattice = Lattice::read(rows, job.range(0.0, 0.9))?;
    let mut scratch: Vec<f64> = reserved_vec(lattice.count)?;
    let mut medians = Vec::with_capacity(frames);
    for levels in &lattice.level {
        job.check()?;
        scratch.clear();
        scratch.extend(levels.iter().filter(|v| !v.is_nan()).map(|v| f64::from(*v)));
        medians.push(median(&mut scratch).unwrap_or(0.0));
    }
    let mut order: Vec<usize> = (0..frames).collect();
    order.sort_by(|a, b| medians[*a].total_cmp(&medians[*b]).then(a.cmp(b)));
    let mut evs = vec![0.0f64; frames];
    for pair in order.windows(2) {
        job.check()?;
        let (a, b) = (pair[0], pair[1]);
        lattice.joint(a, b, &mut scratch, |ya, yb| (yb / ya).log2());
        if scratch.len() < MIN_PAIR_SAMPLES {
            return Ok(Estimate::Unpaired { a, b });
        }
        let Some(step) = median(&mut scratch) else {
            return Ok(Estimate::Unpaired { a, b });
        };
        evs[b] = evs[a] + step;
    }
    let base = evs.first().copied().unwrap_or(0.0);
    job.report(1.0, 1)?;
    Ok(Estimate::Evs(evs.iter().map(|ev| ev - base).collect()))
}

/// One judged frame's ghost test: the median residual, the threshold around
/// it and the robust noise sigma the threshold came from.
#[derive(Clone, Copy)]
struct Ghost {
    median: f64,
    tau: f64,
    sigma: f64,
}

/// Each frame's ghost test against the reference; `None` for the reference
/// and for a frame with too few joint samples.
fn ghost_tests(
    lattice: &Lattice,
    exposure: &[f64],
    reference: usize,
    job: JobContext<'_>,
) -> Result<Vec<Option<Ghost>>, String> {
    let frames = exposure.len();
    let mut residuals: Vec<f64> = reserved_vec(lattice.count)?;
    let mut deviations: Vec<f64> = reserved_vec(lattice.count)?;
    let mut tests = vec![None; frames];
    for frame in (0..frames).filter(|frame| *frame != reference) {
        job.check()?;
        let shift = exposure[frame] * std::f64::consts::LN_2;
        lattice.joint(frame, reference, &mut residuals, |y, y_reference| {
            y.ln() - y_reference.ln() - shift
        });
        if residuals.len() < MIN_PAIR_SAMPLES {
            continue;
        }
        let Some(middle) = median(&mut residuals) else {
            continue;
        };
        deviations.clear();
        deviations.extend(residuals.iter().map(|r| (r - middle).abs()));
        let Some(spread) = median(&mut deviations) else {
            continue;
        };
        let sigma = MAD_TO_SIGMA * spread;
        let tau = (DEGHOST_SIGMAS * sigma).max((1.0 + DEGHOST_FLOOR).ln());
        tests[frame] = Some(Ghost {
            median: middle,
            tau,
            sigma,
        });
    }
    Ok(tests)
}

/// A finished merge.
pub struct Merged {
    /// Straight linear RGBA, row-major, canvas-sized.
    pub pixels: Vec<[f32; 4]>,
    /// Pixels where every covering frame was clipped (the shortest exposure
    /// was taken alone).
    pub all_clipped: u64,
    /// Pixels where no frame had weight and at least one was not clipped (the
    /// longest such exposure was taken alone).
    pub all_dark: u64,
    /// Pixels the ghost test gave to the reference alone.
    pub deghosted: u64,
    /// Per frame, the robust noise sigma the ghost test measured (natural log
    /// units); `None` for the reference, an unjudged frame, and every frame
    /// when `deghost` was off.
    pub noise_sigma: Vec<Option<f64>>,
}

/// The merge of `rows` (module doc). `evs` holds one exposure value per frame
/// in stops and `reference` is the frame whose scale the result is on. `Err`
/// for a count that does not match, a cancel or a failed allocation.
pub fn merge(
    rows: &dyn FrameRows,
    evs: &[f64],
    reference: usize,
    deghost: bool,
    job: JobContext<'_>,
) -> Result<Merged, String> {
    let frames = rows.frames();
    if frames == 0 || evs.len() != frames || reference >= frames {
        return Err(
            "merge to hdr needs one exposure value per frame and a reference among them".into(),
        );
    }
    let (width, height) = rows.size();
    let exposure: Vec<f64> = evs.iter().map(|ev| ev - evs[reference]).collect();
    let gain: Vec<f64> = exposure.iter().map(|e| e.exp2()).collect();
    let tests = if deghost {
        let lattice = Lattice::read(rows, job.range(0.0, 0.08))?;
        ghost_tests(&lattice, &exposure, reference, job.range(0.08, 0.1))?
    } else {
        vec![None; frames]
    };
    let noise_sigma = tests.iter().map(|test| test.map(|t| t.sigma)).collect();
    let rows_job = job.range(0.1, 1.0);
    let w = width as usize;
    let count = w
        .checked_mul(height as usize)
        .ok_or("merge to hdr canvas exceeds addressable memory")?;
    let mut pixels: Vec<[f32; 4]> = reserved_vec(count)?;
    let held = frames
        .checked_mul(w)
        .ok_or("merge to hdr row buffer exceeds addressable memory")?;
    let mut buffer: Vec<FrameSample> = reserved_vec(held)?;
    buffer.resize(held, FrameSample::default());
    let (mut all_clipped, mut all_dark, mut deghosted) = (0u64, 0u64, 0u64);
    for y in 0..height {
        rows_job.report(f64::from(y) / f64::from(height), 6)?;
        for (frame, row) in buffer.chunks_exact_mut(w.max(1)).enumerate() {
            rows.row(frame, y, row);
        }
        for x in 0..w {
            let anchor = buffer[reference * w + x];
            let judged = deghost && well_exposed(&anchor);
            let mut sum = [0.0f64; 3];
            let mut total = 0.0f64;
            let mut alpha = 0.0f32;
            let mut ghost = false;
            // The shortest covering exposure, and the longest one below the
            // clip level; the first listed frame wins a tie.
            let mut shortest: Option<usize> = None;
            let mut longest: Option<usize> = None;
            for frame in 0..frames {
                let sample = buffer[frame * w + x];
                if !covering(&sample) {
                    continue;
                }
                alpha = alpha.max(sample.alpha);
                let own = f64::from(sample.alpha) * f64::from(weight(sample.level)) * gain[frame];
                for (channel, value) in sum.iter_mut().zip(sample.linear) {
                    *channel += own * (f64::from(value) / gain[frame]);
                }
                total += own;
                if shortest.is_none_or(|k| exposure[frame] < exposure[k]) {
                    shortest = Some(frame);
                }
                if sample.level < LEVEL_CLIP
                    && longest.is_none_or(|k| exposure[frame] > exposure[k])
                {
                    longest = Some(frame);
                }
                if judged && frame != reference && well_exposed(&sample) {
                    if let Some(test) = tests[frame] {
                        let residual = mean_linear(&sample).ln()
                            - mean_linear(&anchor).ln()
                            - exposure[frame] * std::f64::consts::LN_2;
                        ghost |= (residual - test.median).abs() > test.tau;
                    }
                }
            }
            let alone = |frame: usize| {
                let sample = buffer[frame * w + x];
                sample.linear.map(|v| (f64::from(v) / gain[frame]) as f32)
            };
            let rgb = match (shortest, longest) {
                (None, _) => {
                    pixels.push([0.0; 4]);
                    continue;
                }
                _ if ghost => {
                    deghosted += 1;
                    anchor.linear
                }
                _ if total > 0.0 => sum.map(|v| (v / total) as f32),
                (_, Some(frame)) => {
                    all_dark += 1;
                    alone(frame)
                }
                (Some(frame), None) => {
                    all_clipped += 1;
                    alone(frame)
                }
            };
            pixels.push([rgb[0], rgb[1], rgb[2], alpha]);
        }
    }
    rows_job.report(1.0, 6)?;
    Ok(Merged {
        pixels,
        all_clipped,
        all_dark,
        deghosted,
        noise_sigma,
    })
}

/// The working memory [`merge`] holds beside its output for a `width` ×
/// `height` canvas and `frames` frames: one row of every frame, the lattice's
/// 12 bytes per sample per frame with its one-row read buffer, and the two
/// median work lists. `None` on overflow. [`estimate_evs`] holds less.
pub fn scratch_bytes(width: u32, height: u32, frames: usize) -> Option<u64> {
    let frames = u64::try_from(frames).ok()?;
    let sample = std::mem::size_of::<FrameSample>() as u64;
    let row = u64::from(width).checked_mul(sample)?;
    let lattice = lattice_count(width, height);
    row.checked_mul(frames)?
        .checked_add(row)?
        .checked_add(lattice.checked_mul(12)?.checked_mul(frames)?)?
        .checked_add(lattice.checked_mul(16)?)
}
