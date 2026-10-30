//! The fitted Gaussian Blur: the kernel of Filter ▸ Blur ▸ Gaussian
//! Blur at 8, 16 and 32 bits (`dr_image_filter_op` `blur` through
//! `sampling::fitted_blur`, the 16-bit `blur` selected edit through
//! `selected_filter`, the float route's `Route::GaussianBlur`). Every other
//! Gaussian in the core keeps its own kernel (`sampling_gaussian`,
//! `sampling_gaussian_fast`, `gaussian_plane`).
//!
//! THE MASTER of the Gaussian Blur filter's box filter: an extended box
//! (Gwosdek, Grewenig, Bruhn and Weickert, "Theoretical foundations of
//! Gaussian convolution by extended box filtering", SSVM 2011) run `K` times
//! per axis. One pass of radius `l` and end weight `α` is
//! `y[i] = (Σ_{|j|≤l} x[i+j] + α(x[i−l−1] + x[i+l+1])) / (2l + 1 + 2α)`, with
//! `v = σ²/K`, `l = ⌊√(12v + 1)/2 − 1/2⌋` and
//! `α = (2l + 1)(l(l + 1) − 3v) / (6(v − (l + 1)²))`, so `K` passes have
//! variance exactly σ². Edges are exact clamp-to-edge of the K-fold kernel:
//! a line is extended once by `K(l + 1)` copies of its end samples and every
//! pass keeps only the samples its window covers ("pad-once"; per-pass
//! clamping was measured and lost). Horizontal pass, then vertical; line math
//! binary64, storage f32 between the passes. It is not Box Blur's integer
//! box (`sampling_box`, the ONE clamped-edge sliding box sum): its fractional
//! end taps and pad-once edges are what make K passes a Gaussian of any σ.
//!
//! THE LAW was fitted on one Photoshop 26.10.0 take and confirmed on a second
//! (batch v1x-blur-wand, `scripts/photoshop-probe/blur_wand_take.py`; receipts
//! `doctored-fixtures/photoshop/blur-wand-v1x/take1c` and `take2`). From
//! [`FITTED_MIN_RADIUS`] (7) up, Radius `r` (the request's `sigma`) runs
//! K = 4 extended boxes at σ = 0.99·r: take1c's one member over every fitted
//! radius from 3 to 100, and within 1 code at 8 bits of every take2 render
//! from r = 9 to 250. Photoshop's Radius is NOT a σ: four boxes of σ 0.99·r
//! are what its blur is, which an exact Gaussian matches only at a σ about
//! 2% LARGER (batch 146's best fit, σ 12.29 at r = 12). Below 7 the filter
//! keeps today's kernel: take2 at r = 6.5 and 1.3 was neither within the
//! bound nor measurably closer (`FITTED_MIN_RADIUS` says how). The fit
//! table and both takes' verdicts are in
//! `designs/phase-9f-selected-tone-status.md`.
//!
//! [`band`] computes each `BAND_ROWS` tile from its own window of ± the line
//! kernel's full support (`K(l + 1)` rows), so a tile's kept rows never read a
//! replicated window edge and any aligned range equals the whole plane bit
//! for bit.
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::{reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling_gaussian_fast::{window_rows, FAST_WORK_PER_PIXEL, MAX_SIGMA};
use crate::selected_filter::BAND_ROWS;
use std::ops::Range;

/// The law's member: `PASSES` extended boxes per axis at σ = `SCALE`·r,
/// take1c's one member over every fitted radius from 3 to 100 (8-bit max
/// 1 code against Photoshop there), confirmed by take2 at r = 9, 14, 20, 40,
/// 75 and 250 (8-bit max 1 code at each).
const PASSES: u32 = 4;
const SCALE: f64 = 0.99;

/// The smallest radius the shipped law covers; below it the filter keeps
/// today's kernel (`sampling::blur`, `selected_filter`'s legacy tiles, the
/// float FIR). take2 refused r = 6.5 and 1.3 by the declared ship rule (8-bit
/// max 3 and 7 codes, neither within the bound nor measurably closer than
/// today), so the law starts at the smallest fitted radius above both, 7.
pub(crate) const FITTED_MIN_RADIUS: f64 = 7.0;

/// Whether `radius` runs this filter.
pub(crate) fn runs(radius: f64) -> bool {
    radius >= FITTED_MIN_RADIUS
}

/// The σ the law runs at `radius`.
pub(crate) fn kernel_sigma(radius: f64) -> f64 {
    SCALE * radius
}

/// One pass's geometry: (l, α) for variance `v` (module doc).
fn geometry(v: f64) -> (usize, f64) {
    let l = (0.5 * (12.0 * v + 1.0).sqrt() - 0.5).floor().max(0.0);
    let next = (l + 1.0) * (l + 1.0);
    let alpha = (2.0 * l + 1.0) * (l * (l + 1.0) - 3.0 * v) / (6.0 * (v - next));
    (l as usize, alpha)
}

/// The line kernel at a radius: `passes` extended boxes of radius `l` and
/// end weight `alpha`.
struct Line {
    passes: usize,
    l: usize,
    alpha: f64,
}

impl Line {
    fn new(radius: f64) -> Self {
        let sigma = kernel_sigma(radius);
        let (l, alpha) = geometry(sigma * sigma / f64::from(PASSES));
        Line {
            passes: PASSES as usize,
            l,
            alpha,
        }
    }

    /// The samples a line is extended by at each end: the K-fold support.
    fn pad(&self) -> usize {
        self.passes * (self.l + 1)
    }

    /// One valid pass over `x` into `y`: `y.len() == x.len() − 2(l + 1)`,
    /// `y[i]` centred on `x[i + l + 1]`. `stride` interleaves independent
    /// lines (a block of columns), each sample `[f64; 4]`.
    fn pass(&self, x: &[[f64; 4]], y: &mut [[f64; 4]], stride: usize) {
        let (l, alpha) = (self.l, self.alpha);
        let reach = 2 * l + 2;
        let rows = x.len() / stride - reach;
        let norm = 1.0 / (2.0 * l as f64 + 1.0 + 2.0 * alpha);
        for c in 0..stride {
            let at = |r: usize| x[r * stride + c];
            // The inner sum over x[i + 1 ..= i + 2l + 1].
            let mut sum = [0.0_f64; 4];
            for r in 1..=2 * l + 1 {
                let v = at(r);
                for k in 0..4 {
                    sum[k] += v[k];
                }
            }
            for i in 0..rows {
                let (a, b) = (at(i), at(i + reach));
                let mut out = [0.0_f64; 4];
                for k in 0..4 {
                    out[k] = (sum[k] + alpha * (a[k] + b[k])) * norm;
                }
                y[i * stride + c] = out;
                if i + 1 < rows {
                    let (enter, leave) = (at(i + reach), at(i + 1));
                    for k in 0..4 {
                        sum[k] += enter[k] - leave[k];
                    }
                }
            }
        }
    }

    /// All passes over a line already extended by [`Line::pad`] at each end:
    /// `buf` holds it on entry and `scratch` is the same size. Returns the
    /// `len`-sample result, at the front of one of the two.
    fn run<'a>(
        &self,
        buf: &'a mut [[f64; 4]],
        scratch: &'a mut [[f64; 4]],
        len: usize,
        stride: usize,
    ) -> &'a [[f64; 4]] {
        let mut extended = len + 2 * self.pad();
        let (mut from, mut to) = (buf, scratch);
        for _ in 0..self.passes {
            let next = extended - 2 * (self.l + 1);
            self.pass(&from[..extended * stride], &mut to[..next * stride], stride);
            std::mem::swap(&mut from, &mut to);
            extended = next;
        }
        let from: &'a [[f64; 4]] = from;
        &from[..len * stride]
    }
}

/// Err unless `radius` is in (0, `MAX_SIGMA`].
fn checked(radius: f64) -> Result<(), String> {
    if !radius.is_finite() || radius <= 0.0 || radius > f64::from(MAX_SIGMA) {
        return Err("Gaussian sigma must be finite, positive and at most 1000".into());
    }
    Ok(())
}

/// Columns the vertical pass gathers at once.
const BLOCK: usize = 32;

/// (scratch bytes for `rows` output rows of any band, work units for the
/// WHOLE w×h plane). Err for a radius outside (0, `MAX_SIGMA`]. No cap: each
/// caller applies its own.
///
/// Scratch is the f32 output rows and one f32 window, the two binary64
/// extended rows of the horizontal pass, the two binary64 extended column
/// blocks of the vertical pass, plus five bookkeeping allowances. Work is
/// `sampling_gaussian_fast::FAST_WORK_PER_PIXEL` per window pixel over every
/// window of the plane (no work constant of its own).
///
/// Record (2026-10-13; Release, one case per process, `MallocLargeCache=0`,
/// E = `ri_interval_max_phys_footprint` − pre, 3 runs and a base twin at E 0,
/// opaque 6000×4000; `tests/gaussian_blur_photoshop.rs`' `measure_*` probes
/// and `float_adjust_apply_24mp.rs`' `measure`):
/// - 8-bit `dr_image_filter_op` `blur`: r25 E 136,773,704 ×3 against
///   184,357,760 (1.348×); r64 E 162,906,208 ×3 against 218,412,160 (1.341×).
/// - 16-bit `blur` edit: r25 E ≤ 386,793,760 against 485,583,360 (1.255×);
///   r64 E ≤ 385,696,056 against 512,826,880 (1.330×).
/// - float `blur` edit: r8 / r25 / r64 E ≤ 769,737,256 / 769,016,360 /
///   768,426,536 against 1,026,291,456 (its second stage binds; ≥ 1.333×).
///
/// The Release app drive (`scripts/gaussian-blur-admission/measure.py`) holds
/// the whole MCP call against what the host admits for it: 8 bits within the
/// commit stage (1.40×), 32 bits (1.336×), and 16 bits within max(this plan,
/// the 16-bit display preparation `PreparedDocumentEdit` admits) plus the
/// retained growth. Its 16-bit E, 576.7 MB, is the same at r6 on the old
/// kernel: the call's peak is the edit's output plus that display
/// preparation, not this plan (M16 was re-declared so after its first run,
/// which held the call against this plan alone).
pub(crate) fn plan(dimensions: (u32, u32), radius: f64, rows: u32) -> Result<(u64, u64), String> {
    checked(radius)?;
    let (w, h) = dimensions;
    let w = u64::from(w);
    let line = Line::new(radius);
    let pad = line.pad() as u64;
    let halo = line.pad() as u32;
    let window = u64::from(h.min(BAND_ROWS.saturating_add(halo.saturating_mul(2))));
    let work = window_rows(h, halo)
        .saturating_mul(w)
        .saturating_mul(FAST_WORK_PER_PIXEL);
    let block = (BLOCK as u64).min(w.max(1));
    let scratch = u64::from(rows) * w * 16
        + window * w * 16
        + 2 * (w + 2 * pad) * 32
        + 2 * (window + 2 * pad) * block * 32
        + 5 * BOOKKEEPING_BYTES;
    Ok((scratch, work))
}

/// Premultiplied rows `rows` (`rows.start % BAND_ROWS == 0`) of the filter
/// over the plane `read` yields (premultiplied samples, row-major index).
/// Each `BAND_ROWS` tile is computed from its own window of ± the kernel's support,
/// so any aligned range is bit-identical to the same rows of `0..h`. `Unit`
/// clamps each output to a valid premultiplied sample (alpha in [0, 1],
/// colour in [0, alpha]); `Extended` clamps nothing (the weights are
/// non-negative, so no ringing is made). Progress is reported 0..1 over its
/// own rows. With `JobContext::NONE` the only `Err` is an allocation refused.
pub(crate) fn band(
    dimensions: (u32, u32),
    radius: f64,
    rows: Range<u32>,
    range: &SampleRange,
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    checked(radius)?;
    let (w, h) = dimensions;
    if w == 0 || !rows.start.is_multiple_of(BAND_ROWS) || rows.start >= rows.end || rows.end > h {
        return Err("Gaussian Blur band rows lie outside the plane".into());
    }
    let unit = matches!(range, SampleRange::Unit);
    let line = Line::new(radius);
    let pad = line.pad();
    let halo = pad as u32;
    let width = w as usize;
    let len = (rows.end - rows.start) as usize;
    let window = h.min(BAND_ROWS.saturating_add(halo.saturating_mul(2))) as usize;
    let block = BLOCK.min(width);
    let mut out = reserved_vec(len * width)?;
    out.resize(len * width, [0.0_f32; 4]);
    let mut win: Vec<[f32; 4]> = reserved_vec(window * width)?;
    let mut row: Vec<[f64; 4]> = reserved_vec(width + 2 * pad)?;
    row.resize(width + 2 * pad, [0.0; 4]);
    let mut row_scratch: Vec<[f64; 4]> = reserved_vec(width + 2 * pad)?;
    row_scratch.resize(width + 2 * pad, [0.0; 4]);
    let mut col: Vec<[f64; 4]> = reserved_vec((window + 2 * pad) * block)?;
    col.resize((window + 2 * pad) * block, [0.0; 4]);
    let mut col_scratch: Vec<[f64; 4]> = reserved_vec((window + 2 * pad) * block)?;
    col_scratch.resize((window + 2 * pad) * block, [0.0; 4]);
    for t0 in rows.clone().step_by(BAND_ROWS as usize) {
        // The window comes from the whole tile, whatever `rows.end` is; only
        // the rows written are clipped to it.
        let tile_end = (t0 + BAND_ROWS).min(h);
        let t1 = tile_end.min(rows.end);
        let top = t0.saturating_sub(halo);
        let bottom = tile_end.saturating_add(halo).min(h);
        let n = (bottom - top) as usize;
        // Horizontal, every window row: extend once, run every pass.
        win.clear();
        for r in top..bottom {
            job.check()?;
            let base = r as usize * width;
            let (first, last) = (read(base), read(base + width - 1));
            for (i, v) in row.iter_mut().enumerate() {
                let sample = if i < pad {
                    first
                } else if i >= pad + width {
                    last
                } else {
                    read(base + i - pad)
                };
                *v = sample.map(f64::from);
            }
            let done = line.run(&mut row, &mut row_scratch, width, 1);
            win.extend(done.iter().map(|v| v.map(|c| c as f32)));
        }
        // Vertical, a block of columns at a time over the window extended by
        // `pad` copies of its end rows; only the tile's rows are kept, and
        // those never read a copy of an inner window edge.
        let (lo, hi) = ((t0 - top) as usize, (t1 - top) as usize);
        for c0 in (0..width).step_by(block) {
            job.check()?;
            let cols = block.min(width - c0);
            for e in 0..n + 2 * pad {
                let r = e.saturating_sub(pad).min(n - 1);
                for c in 0..cols {
                    col[e * cols + c] = win[r * width + c0 + c].map(f64::from);
                }
            }
            let done = line.run(&mut col, &mut col_scratch, n, cols);
            for r in lo..hi {
                let at = (t0 as usize - rows.start as usize + r - lo) * width + c0;
                for c in 0..cols {
                    let v = done[r * cols + c];
                    out[at + c] = if unit {
                        // A NaN fails `> 0` and `max` drops it, so it lands on 0.
                        let a = v[3] as f32;
                        let alpha = if a > 0.0 { a.min(1.0) } else { 0.0 };
                        [
                            (v[0] as f32).max(0.0).min(alpha),
                            (v[1] as f32).max(0.0).min(alpha),
                            (v[2] as f32).max(0.0).min(alpha),
                            alpha,
                        ]
                    } else {
                        v.map(|x| x as f32)
                    };
                }
            }
        }
        job.report(f64::from(t1 - rows.start) / len as f64, 6)?;
    }
    Ok(out)
}
