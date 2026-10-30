//! Bounded per-pixel research joint alpha/foreground/background estimation.
//! Gaussian conditional colors follow Chuang et al., Bayesian Matting (2001),
//! with a finite Gaussian alpha prior added to the original joint MAP energy.
//! The 3x3 Schur complement shares the color-line solver's Cholesky routines.
//! Grid and golden-section samples are an approximation, not a convergence or
//! opacity guarantee. Inputs and outputs use one caller-selected RGB domain.

use crate::matting_laplacian::{factor_spd, solve_spd};

/// Variance of one uniform encoded-byte quantization error, not camera-noise
/// calibration. Caller-supplied color covariances include this same floor.
pub const OBSERVATION_VARIANCE: f64 = 1.0 / (12.0 * 255.0 * 255.0);
/// Nine grid points, prior, color projection, and ten golden-section samples.
pub const MAX_EVALUATIONS: usize = 21;
const ALPHA_VARIANCE: f64 = 0.5 * 0.5;

#[derive(Clone, Copy, Debug)]
pub struct Gaussian {
    pub mean: [f64; 3],
    /// Finite, exactly symmetric positive-definite covariance.
    pub covariance: [[f64; 3]; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct Estimate {
    pub alpha: f64,
    pub foreground: [f64; 3],
    pub background: [f64; 3],
    /// Full joint objective of the returned, gamut-clipped colors. Includes
    /// color covariance log determinants; omits shared Gaussian constants.
    pub objective: f64,
    pub evaluations: usize,
}

struct Model<'a> {
    color: [f64; 3],
    prior: f64,
    foreground: &'a Gaussian,
    background: &'a Gaussian,
    foreground_factor: [[f64; 3]; 3],
    background_factor: [[f64; 3]; 3],
    log_normalization: f64,
}

/// Return an immutable bounded approximation. Automatic prior endpoints are
/// uncertain; `fixed_alpha` alone restricts alpha exactly. At a fixed endpoint
/// the observed corresponding F/B color is retained exactly. Malformed input,
/// non-SPD covariance, or nonfinite intermediate arithmetic refuses.
pub fn estimate(
    color: [f64; 3],
    prior: f64,
    foreground: &Gaussian,
    background: &Gaussian,
    fixed_alpha: Option<f64>,
) -> Result<Estimate, String> {
    if !color.into_iter().all(unit) || !unit(prior) || fixed_alpha.is_some_and(|a| !unit(a)) {
        return Err("bayesian color requires finite normalized colors and alpha".into());
    }
    let foreground_factor = validate(foreground)?;
    let background_factor = validate(background)?;
    let log_normalization = (0..3)
        .map(|c| foreground_factor[c][c].ln() + background_factor[c][c].ln())
        .sum();
    let model = Model {
        color,
        prior,
        foreground,
        background,
        foreground_factor,
        background_factor,
        log_normalization,
    };
    if let Some(alpha) = fixed_alpha {
        return model.evaluate(alpha, true);
    }
    let mut best = model.evaluate(0.0, false)?;
    let mut evaluations = 1;
    let mut best_grid = 0_usize;
    for grid in 1..=8 {
        let candidate = model.evaluate(grid as f64 / 8.0, false)?;
        evaluations += 1;
        if candidate.objective < best.objective {
            best_grid = grid;
            best = candidate;
        }
    }
    let difference = subtract(foreground.mean, background.mean);
    let separation = dot(difference, difference);
    let projection = if separation > 0.0 {
        (dot(subtract(color, background.mean), difference) / separation).clamp(0.0, 1.0)
    } else {
        prior
    };
    for alpha in [prior, projection] {
        choose(&mut best, model.evaluate(alpha, false)?);
        evaluations += 1;
    }
    // Refine the best *grid* basin; the independently evaluated prior and
    // projection remain eligible even if they lie outside this interval.
    let mut left = best_grid.saturating_sub(1) as f64 / 8.0;
    let mut right = (best_grid + 1).min(8) as f64 / 8.0;
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut lower = model.evaluate(right - ratio * (right - left), false)?;
    let mut upper = model.evaluate(left + ratio * (right - left), false)?;
    choose(&mut best, lower);
    choose(&mut best, upper);
    evaluations += 2;
    for _ in 0..8 {
        if lower.objective <= upper.objective {
            right = upper.alpha;
            upper = lower;
            lower = model.evaluate(right - ratio * (right - left), false)?;
            choose(&mut best, lower);
        } else {
            left = lower.alpha;
            lower = upper;
            upper = model.evaluate(left + ratio * (right - left), false)?;
            choose(&mut best, upper);
        }
        evaluations += 1;
    }
    best.evaluations = evaluations;
    Ok(best)
}

impl Model<'_> {
    fn evaluate(&self, alpha: f64, fixed: bool) -> Result<Estimate, String> {
        let beta = 1.0 - alpha;
        let covariance = std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                alpha * alpha * self.foreground.covariance[row][column]
                    + beta * beta * self.background.covariance[row][column]
                    + if row == column {
                        OBSERVATION_VARIANCE
                    } else {
                        0.0
                    }
            })
        });
        let residual = std::array::from_fn(|c| {
            self.color[c] - alpha * self.foreground.mean[c] - beta * self.background.mean[c]
        });
        let correction = solve_spd(factor_spd(covariance)?, residual);
        let mut foreground = std::array::from_fn(|c| {
            self.foreground.mean[c] + alpha * dot(self.foreground.covariance[c], correction)
        });
        let mut background = std::array::from_fn(|c| {
            self.background.mean[c] + beta * dot(self.background.covariance[c], correction)
        });
        if !foreground.into_iter().chain(background).all(f64::is_finite) {
            return Err("bayesian color arithmetic is not finite".into());
        }
        foreground = foreground.map(|v| v.clamp(0.0, 1.0));
        background = background.map(|v| v.clamp(0.0, 1.0));
        if fixed && alpha == 1.0 {
            foreground = self.color;
        } else if fixed && alpha == 0.0 {
            background = self.color;
        }
        // Clipping changes the unconstrained conditional minimum. Evaluate the
        // original joint energy again; do not use an eliminated or unclipped
        // score, and do not add logdet(S), which would marginalize F/B instead.
        let residual =
            std::array::from_fn(|c| self.color[c] - alpha * foreground[c] - beta * background[c]);
        let f_delta = subtract(foreground, self.foreground.mean);
        let b_delta = subtract(background, self.background.mean);
        let objective = 0.5
            * (dot(residual, residual) / OBSERVATION_VARIANCE
                + dot(f_delta, solve_spd(self.foreground_factor, f_delta))
                + dot(b_delta, solve_spd(self.background_factor, b_delta))
                + (alpha - self.prior).powi(2) / ALPHA_VARIANCE)
            + self.log_normalization;
        if !objective.is_finite() {
            return Err("bayesian color objective is not finite".into());
        }
        Ok(Estimate {
            alpha,
            foreground,
            background,
            objective,
            evaluations: 1,
        })
    }
}

fn validate(gaussian: &Gaussian) -> Result<[[f64; 3]; 3], String> {
    if !gaussian.mean.into_iter().all(unit)
        || !gaussian
            .covariance
            .into_iter()
            .flatten()
            .all(f64::is_finite)
        || (0..3).any(|r| (0..3).any(|c| gaussian.covariance[r][c] != gaussian.covariance[c][r]))
    {
        return Err(
            "bayesian color requires normalized means and finite symmetric covariance".into(),
        );
    }
    factor_spd(gaussian.covariance)
        .map_err(|_| "bayesian color covariance is not positive definite".into())
}

fn unit(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left.into_iter().zip(right).map(|(a, b)| a * b).sum()
}

fn subtract(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|c| left[c] - right[c])
}

fn choose(best: &mut Estimate, candidate: Estimate) {
    if candidate.objective < best.objective {
        *best = candidate;
    }
}
