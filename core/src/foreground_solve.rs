//! The foreground-colour solver shared by the byte (`foreground_estimate`) and
//! 16-bit (`precision_foreground`) estimates: the local two-variable
//! compositing/alpha-gradient energy of Germer et al., Fast Multi-Level
//! Foreground Estimation (2020), equations 3/9:
//! https://arxiv.org/abs/2006.14970 . This is a new implementation, not a port.
//! Coarse-to-fine triangle resampling uses the shared sampling implementation
//! (`sampling::resize_fields`). Ten low-resolution and two higher-resolution
//! sweeps are approximation bounds, not convergence claims.
//!
//! Free functions over f32 fields: the adapters decode their own storage into
//! the input field and quantize the estimate back.

use crate::{job_control::JobContext, sampling::resize_fields};

struct Level {
    size: (u32, u32),
    // RGB in the explicitly selected compositing domain, then alpha coverage.
    input: Vec<[f32; 4]>,
}

/// Solves foreground and background colours for `input` — one `[r, g, b,
/// alpha]` per pixel of a `size` plane, row-major, RGB in the caller's
/// compositing domain and alpha the matte in 0…1 — and returns one
/// `[Fr, Fg, Fb, Br, Bg, Bb]` per pixel, each in 0…1. The input pyramid is at
/// most 32 bytes a pixel (even for a 1×N plane), the live old and new
/// estimates and the separable resize intermediate at most 60; the caller
/// reserves for them. Reports progress from 0.15 (pyramid built) to 0.95.
/// `Err` only for a cancelled job.
pub(crate) fn solve(
    input: Vec<[f32; 4]>,
    size: (u32, u32),
    job: JobContext<'_>,
) -> Result<Vec<[f32; 6]>, String> {
    let mut levels = vec![Level { size, input }];
    while levels.last().is_some_and(|v| v.size != (1, 1)) {
        let previous = levels.last().expect("nonempty pyramid");
        let size = (previous.size.0.div_ceil(2), previous.size.1.div_ceil(2));
        let input = resize_fields(&previous.input, previous.size, size, job)?;
        levels.push(Level { size, input });
    }
    job.report(0.15, 6)?;
    let mut estimate = vec![[0.0; 6]];
    let mut old_size = (1, 1);
    let total_work: u64 = levels
        .iter()
        .map(|v| u64::from(v.size.0) * u64::from(v.size.1) * sweeps(v.size) as u64)
        .sum();
    let mut done = 0_u64;
    while let Some(level) = levels.pop() {
        if old_size != level.size {
            estimate = resize_fields(&estimate, old_size, level.size, job)?;
        }
        for _ in 0..sweeps(level.size) {
            relax(&level, &mut estimate, job)?;
            done += u64::from(level.size.0) * u64::from(level.size.1);
            job.report(0.15 + 0.8 * done as f64 / total_work as f64, 6)?;
        }
        old_size = level.size;
    }
    Ok(estimate)
}

fn sweeps(size: (u32, u32)) -> usize {
    if size.0.max(size.1) <= 32 {
        10
    } else {
        2
    }
}

fn relax(level: &Level, colors: &mut [[f32; 6]], job: JobContext<'_>) -> Result<(), String> {
    let (w, h) = (level.size.0 as usize, level.size.1 as usize);
    for y in 0..h {
        job.check()?;
        for x in 0..w {
            let i = y * w + x;
            job.every(i)?;
            let input = level.input[i];
            let a = f64::from(input[3]);
            let b = 1.0 - a;
            let neighbors = [
                y * w + x.saturating_sub(1),
                y * w + (x + 1).min(w - 1),
                y.saturating_sub(1) * w + x,
                (y + 1).min(h - 1) * w + x,
            ];
            let mut weight_sum = 0.0;
            let mut rhs = [0.0; 6];
            for c in 0..3 {
                rhs[c] = a * f64::from(input[c]);
                rhs[c + 3] = b * f64::from(input[c]);
            }
            for j in neighbors {
                // Paper defaults stabilize flat alpha while propagating colors
                // more strongly across mixed boundaries. Fixed before testing.
                let weight = 0.005 + 0.1 * (a - f64::from(level.input[j][3])).abs();
                weight_sum += weight;
                for (c, r) in rhs.iter_mut().enumerate() {
                    *r += weight * f64::from(colors[j][c]);
                }
            }
            let aa = a * a + weight_sum;
            let bb = b * b + weight_sum;
            let ab = a * b;
            let determinant = aa * bb - ab * ab;
            for c in 0..3 {
                colors[i][c] =
                    ((bb * rhs[c] - ab * rhs[c + 3]) / determinant).clamp(0.0, 1.0) as f32;
                colors[i][c + 3] =
                    ((aa * rhs[c + 3] - ab * rhs[c]) / determinant).clamp(0.0, 1.0) as f32;
            }
        }
    }
    Ok(())
}
