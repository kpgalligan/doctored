//! Local RGB color-line matting Laplacian, stored as a bounded 5x5 stencil.
//!
//! Eliminating the affine color coefficients in a window gives
//! Lw = I - 11'/m - X (X'X + epsilon I)^-1 X', where X contains centered RGB.
//! Only windows touching unknown pixels contribute to the reduced system.
//! Dirichlet constraints move directly into the right-hand side. No sparse
//! library, image-sized dense matrix, or repeated per-iteration window fitting.

use crate::job_control::JobContext;

pub(crate) const STENCIL: usize = 25;
pub(crate) const KNOWN: u32 = u32::MAX;

pub(crate) struct Laplacian {
    pub(crate) rows: Vec<[f64; STENCIL]>,
    pub(crate) rhs: Vec<f64>,
    pub(crate) pixels: Vec<u32>,
    mapping: Vec<u32>,
    offsets: [i64; STENCIL],
}

impl Laplacian {
    /// Adds the declared quadratic automatic-prior energy after elimination of
    /// explicit hard constraints. Shared by all soft-prior candidate policies.
    pub(crate) fn add_soft_prior(
        &mut self,
        prior: &[u8],
        weight: f64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        for (k, &pixel) in self.pixels.iter().enumerate() {
            job.every(k)?;
            self.rows[k][12] += weight;
            self.rhs[k] += weight * f64::from(prior[pixel as usize]) / 255.0;
        }
        Ok(())
    }

    pub(crate) fn build(
        colors: &[[f32; 3]],
        size: (u32, u32),
        trimap: &[u8],
        unknown: usize,
        epsilon: f64,
        soft_prior: Option<(&[u8], f64)>,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let (w, h) = (size.0 as usize, size.1 as usize);
        let mut mapping = vec![KNOWN; colors.len()];
        let mut pixels = Vec::with_capacity(unknown);
        for (i, &label) in trimap.iter().enumerate() {
            job.every(i)?;
            if label == 128 {
                mapping[i] = pixels.len() as u32;
                pixels.push(i as u32);
            }
        }
        let offsets =
            std::array::from_fn(|k| (k as i64 / 5 - 2) * i64::from(size.0) + (k as i64 % 5 - 2));
        let mut result = Self {
            rows: vec![[0.0; STENCIL]; unknown],
            rhs: vec![0.0; unknown],
            pixels,
            mapping,
            offsets,
        };
        let (ww, wh) = (w.min(3), h.min(3));
        let count = ww * wh;
        for y in 0..=h - wh {
            job.check()?;
            for x in 0..=w - ww {
                if x & 63 == 0 {
                    job.check()?;
                }
                let indices: [usize; 9] = std::array::from_fn(|i| (y + i / ww) * w + x + i % ww);
                let indices = &indices[..count];
                if !indices.iter().any(|&i| trimap[i] == 128) {
                    continue;
                }
                let mut mean = [0.0; 3];
                for &i in indices {
                    for c in 0..3 {
                        mean[c] += f64::from(colors[i][c]) / count as f64;
                    }
                }
                let mut centered = [[0.0; 3]; 9];
                let mut covariance = [[0.0; 3]; 3];
                for (k, &i) in indices.iter().enumerate() {
                    for c in 0..3 {
                        centered[k][c] = f64::from(colors[i][c]) - mean[c];
                    }
                    for c in 0..3 {
                        for d in 0..3 {
                            covariance[c][d] += centered[k][c] * centered[k][d];
                        }
                    }
                }
                for (i, row) in covariance.iter_mut().enumerate() {
                    row[i] += epsilon;
                }
                let factor = factor_spd(covariance)?;
                let projected: [[f64; 3]; 9] =
                    std::array::from_fn(|i| solve_spd(factor, centered[i]));
                // Symmetric insertion uses precisely the same floating-point
                // value for both rows, including after many overlapping windows.
                for i in 0..count {
                    for j in 0..=i {
                        let dot = centered[i]
                            .iter()
                            .zip(projected[j])
                            .map(|(a, b)| a * b)
                            .sum::<f64>();
                        let value = (if i == j { 1.0 } else { 0.0 }) - 1.0 / count as f64 - dot;
                        result.insert(indices[i], indices[j], value, trimap, w);
                        if i != j {
                            result.insert(indices[j], indices[i], value, trimap, w);
                        }
                    }
                }
            }
        }
        if let Some((prior, weight)) = soft_prior {
            result.add_soft_prior(prior, weight, job)?;
        }
        for (i, row) in result.rows.iter().enumerate() {
            job.every(i)?;
            if !row[12].is_finite() || row[12] <= 0.0 || row.iter().any(|v| !v.is_finite()) {
                return Err(
                    "closed-form matting produced an invalid positive-definite system".into(),
                );
            }
        }
        Ok(result)
    }

    fn insert(&mut self, i: usize, j: usize, value: f64, trimap: &[u8], width: usize) {
        let row = self.mapping[i];
        if row == KNOWN {
            return;
        }
        if self.mapping[j] == KNOWN {
            if trimap[j] == 255 {
                self.rhs[row as usize] -= value;
            }
        } else {
            let dx = j as i64 % width as i64 - i as i64 % width as i64;
            let dy = j as i64 / width as i64 - i as i64 / width as i64;
            self.rows[row as usize][((dy + 2) * 5 + dx + 2) as usize] += value;
        }
    }

    fn neighbor(&self, row: usize, slot: usize) -> usize {
        // Only a nonzero coefficient inserted from a validated local window
        // reaches this function, so its pixel and unknown index are valid.
        let pixel = i64::from(self.pixels[row]) + self.offsets[slot];
        self.mapping[pixel as usize] as usize
    }

    pub(crate) fn apply(
        &self,
        input: &[f64],
        out: &mut [f64],
        job: JobContext<'_>,
    ) -> Result<(), String> {
        for (i, (row, output)) in self.rows.iter().zip(out).enumerate() {
            job.every(i)?;
            *output = 0.0;
            for (slot, &value) in row.iter().enumerate() {
                if value != 0.0 {
                    *output += value * input[self.neighbor(i, slot)];
                }
            }
        }
        Ok(())
    }

    /// Symmetric Gauss-Seidel: M=(D+L)D^-1(D+L)'. Because D is positive,
    /// M is SPD even when the matting Laplacian has positive off-diagonals.
    /// This changes convergence speed, never the original objective/constraints.
    pub(crate) fn precondition(
        &self,
        input: &[f64],
        out: &mut [f64],
        job: JobContext<'_>,
    ) -> Result<(), String> {
        for i in 0..self.rows.len() {
            job.every(i)?;
            let row = &self.rows[i];
            let mut value = input[i];
            for (slot, &coefficient) in row[..12].iter().enumerate() {
                if coefficient != 0.0 {
                    value -= coefficient * out[self.neighbor(i, slot)];
                }
            }
            out[i] = value / row[12];
        }
        for i in (0..self.rows.len()).rev() {
            job.every(i)?;
            let row = &self.rows[i];
            let mut value = out[i] * row[12];
            for (slot, &coefficient) in row.iter().enumerate().skip(13) {
                if coefficient != 0.0 {
                    value -= coefficient * out[self.neighbor(i, slot)];
                }
            }
            out[i] = value / row[12];
        }
        Ok(())
    }
}

/// Specialized Cholesky factorization for regularized covariance matrices.
/// The ICC matrix inverse's determinant threshold is intentionally unsuitable
/// for these nearly rank-one color covariances; no generic inverse is copied.
pub(crate) fn factor_spd(a: [[f64; 3]; 3]) -> Result<[[f64; 3]; 3], String> {
    let mut l = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..=i {
            let value = a[i][j] - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if i == j {
                if !value.is_finite() || value <= 0.0 {
                    return Err("closed-form covariance is not positive definite".into());
                }
                l[i][j] = value.sqrt();
            } else {
                l[i][j] = value / l[j][j];
            }
        }
    }
    Ok(l)
}

pub(crate) fn solve_spd(l: [[f64; 3]; 3], b: [f64; 3]) -> [f64; 3] {
    let mut x = [0.0; 3];
    for i in 0..3 {
        x[i] = (b[i] - (0..i).map(|j| l[i][j] * x[j]).sum::<f64>()) / l[i][i];
    }
    for i in (0..3).rev() {
        x[i] = (x[i] - (i + 1..3).map(|j| l[j][i] * x[j]).sum::<f64>()) / l[i][i];
    }
    x
}
