//! KERNEL: Auto-Blend Layers ▸ Panorama's Seamless Tones and Colors:
//! one RGB gain per frame, so overlapping frames agree before the seams are
//! cut and the multi-band blend runs.
//!
//! # The samples
//!
//! Every `panorama_seam::analysis_step`-th pixel in x and y, read through the
//! adapters' row reader (`focus_stack::ReadRow`). A sample counts where at
//! least two frames have alpha 1 and no channel of them is at or above 0.98
//! or at or below 0.02 (a clipped channel says nothing about exposure). Each
//! such pair `i < j` adds one to `N_ij` and its two colours to the pair's
//! sums, so `Ī_ij` is frame `i`'s mean over its overlap with `j`.
//!
//! # The solve
//!
//! Per channel, the gains `g` minimise Brown and Lowe's error (Automatic
//! Panoramic Image Stitching, IJCV 2007, §6):
//!
//! `Σ_{i<j} N_ij [ (g_i Ī_ij − g_j Ī_ji)² / σ_N² + ((1 − g_i)² + (1 − g_j)²) / σ_g² ]`
//!
//! with σ_N = 10/255 and σ_g = 1: a linear system, solved in f64 with partial
//! pivoting. A frame in no pair keeps gain 1.
//!
//! Why σ_g = 1 and not the paper's 0.1. The prior pulls each gain toward 1,
//! and at 0.1 it pulls hard enough to leave the step it is meant to remove:
//! two frames of mean 0.5, one a 0.8 exposure of the other, come out with
//! gain ratio g_A/g_B = 0.851, a visible 6% step left at the seam. At σ_g = 1
//! the ratio is 0.8006, so the overlap agrees to a code, and the prior still
//! fixes the overall level (the gains straddle 1) so the panorama does not
//! drift brighter or darker than its frames.

use crate::focus_stack::{ReadRow, MAX_FRAMES};
use crate::job_control::JobContext;
use crate::panorama_seam::analysis_step;
use crate::precision_image::reserved_vec;

/// σ_N (module doc), in unit codes.
const SIGMA_N: f64 = 10.0 / 255.0;

/// σ_g (module doc).
const SIGMA_G: f64 = 1.0;

/// A sample's channel at or past these says nothing about exposure.
const CLIP_LOW: f32 = 0.02;
const CLIP_HIGH: f32 = 0.98;

fn checked(size: (u32, u32), frames: usize) -> Result<(usize, usize), String> {
    if size.0 == 0 || size.1 == 0 {
        return Err("panorama gains need a non-empty canvas".into());
    }
    if frames == 0 || frames > MAX_FRAMES {
        return Err(format!(
            "panorama gains take between 1 and {MAX_FRAMES} frames"
        ));
    }
    Ok((size.0 as usize, size.1 as usize))
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, String> {
    let mut out = reserved_vec(count)?;
    out.resize(count, value);
    Ok(out)
}

/// `(scratch bytes, work units)` of [`gains`]: the pair counts and sums
/// (32 bytes a pair of frames), one sampled row of every frame (16 bytes a
/// sample), one canvas row, and the solve's matrix; the work is every frame's
/// sampled rows.
pub(crate) fn gains_plan(size: (u32, u32), frames: usize) -> Result<(u64, u64), String> {
    let (w, h) = checked(size, frames)?;
    let step = u64::from(analysis_step(size));
    let (w, h, f) = (w as u64, h as u64, frames as u64);
    let columns = w.div_ceil(step);
    let rows = h.div_ceil(step);
    Ok((
        (f * f * 40)
            .saturating_add(f.saturating_mul(columns).saturating_mul(16))
            .saturating_add(w.saturating_mul(16))
            .saturating_add(f * 8),
        rows.saturating_mul(w).saturating_mul(f),
    ))
}

/// Frame `frame`'s colour at a sample when it counts (module doc).
fn usable(p: [f32; 4]) -> Option<[f32; 3]> {
    let rgb = [p[0], p[1], p[2]];
    (p[3] >= 1.0 && rgb.iter().all(|&v| v > CLIP_LOW && v < CLIP_HIGH)).then_some(rgb)
}

/// The per-frame RGB gains (module doc), one per frame in reader order.
/// `Err` for a refused size, a refused allocation or a cancel.
pub(crate) fn gains(
    size: (u32, u32),
    frames: usize,
    read: ReadRow<'_>,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 3]>, String> {
    let (w, h) = checked(size, frames)?;
    let step = analysis_step(size) as usize;
    let columns = w.div_ceil(step);
    // `count[i * frames + j]` and `sums[i * frames + j]` (frame i's colour
    // summed over its samples shared with j), for i ≠ j.
    let mut count = filled(frames * frames, 0.0f64)?;
    let mut sums = filled(frames * frames, [0.0f64; 3])?;
    let mut samples = filled(frames * columns, None::<[f32; 3]>)?;
    let mut row = filled(w, [0.0f32; 4])?;
    let mut present: Vec<usize> = reserved_vec(frames)?;
    let rows = h.div_ceil(step);
    for (n, y) in (0..h).step_by(step).enumerate() {
        for frame in 0..frames {
            read(frame, y as u32, &mut row);
            let out = &mut samples[frame * columns..(frame + 1) * columns];
            for (slot, x) in out.iter_mut().zip((0..w).step_by(step)) {
                *slot = usable(row[x]);
            }
        }
        for column in 0..columns {
            present.clear();
            present.extend((0..frames).filter(|&k| samples[k * columns + column].is_some()));
            for (a, &i) in present.iter().enumerate() {
                for &j in &present[a + 1..] {
                    let (Some(ci), Some(cj)) =
                        (samples[i * columns + column], samples[j * columns + column])
                    else {
                        continue;
                    };
                    count[i * frames + j] += 1.0;
                    count[j * frames + i] += 1.0;
                    for c in 0..3 {
                        sums[i * frames + j][c] += f64::from(ci[c]);
                        sums[j * frames + i][c] += f64::from(cj[c]);
                    }
                }
            }
        }
        job.report((n + 1) as f64 / rows as f64, 6)?;
    }
    let mut out = vec![[1.0f32; 3]; frames];
    let noise = 1.0 / (SIGMA_N * SIGMA_N);
    let prior = 1.0 / (SIGMA_G * SIGMA_G);
    let mut matrix = filled(frames * (frames + 1), 0.0f64)?;
    for c in 0..3 {
        job.check()?;
        matrix.fill(0.0);
        let width = frames + 1;
        for i in 0..frames {
            for j in 0..frames {
                let n = count[i * frames + j];
                if i == j || n == 0.0 {
                    continue;
                }
                let mean_i = sums[i * frames + j][c] / n;
                let mean_j = sums[j * frames + i][c] / n;
                matrix[i * width + i] += n * (mean_i * mean_i * noise + prior);
                matrix[i * width + j] -= n * mean_i * mean_j * noise;
                matrix[i * width + frames] += n * prior;
            }
            if matrix[i * width + i] == 0.0 {
                // A frame in no pair: the row reads g_i = 1.
                matrix[i * width + i] = 1.0;
                matrix[i * width + frames] = 1.0;
            }
        }
        if let Some(solution) = solve(&mut matrix, frames) {
            for (gain, value) in out.iter_mut().zip(solution) {
                gain[c] = value as f32;
            }
        }
    }
    Ok(out)
}

/// Gaussian elimination with partial pivoting on the `n × (n + 1)` augmented
/// matrix; `None` for a singular one (every gain then stays 1). The system is
/// symmetric positive definite whenever σ_g is finite, so `None` is a guard.
fn solve(m: &mut [f64], n: usize) -> Option<Vec<f64>> {
    let width = n + 1;
    for col in 0..n {
        let pivot = (col..n).max_by(|&a, &b| {
            m[a * width + col]
                .abs()
                .total_cmp(&m[b * width + col].abs())
        })?;
        if m[pivot * width + col].abs() < 1e-300 {
            return None;
        }
        if pivot != col {
            for k in 0..width {
                m.swap(pivot * width + k, col * width + k);
            }
        }
        for row in col + 1..n {
            let factor = m[row * width + col] / m[col * width + col];
            if factor != 0.0 {
                for k in col..width {
                    m[row * width + k] -= factor * m[col * width + k];
                }
            }
        }
    }
    let mut x = vec![0.0f64; n];
    for row in (0..n).rev() {
        let mut value = m[row * width + n];
        for k in row + 1..n {
            value -= m[row * width + k] * x[k];
        }
        x[row] = value / m[row * width + row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// One row through a frame's gain: RGB times `gain`, alpha untouched,
/// unclamped (the caller rounds into its storage).
pub(crate) fn gained(gain: [f32; 3], row: &mut [[f32; 4]]) {
    for p in row {
        for c in 0..3 {
            p[c] *= gain[c];
        }
    }
}
