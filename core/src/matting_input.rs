//! Versioned experimental matting evidence, independent of a solver or host UI.
//!
//! An automatic alpha prior is uncertain evidence, including its endpoints.
//! Only explicit caller-supplied categorical constraints pin alpha exactly.
//! Version 1 adds lambda * sum((alpha - prior)^2) to the color-line energy;
//! it does not convert automatic threshold labels into user instructions.
//!
//! `matting_input.rs` separately declares version-1 soft automatic priors and explicit user-only
//! hard constraints. Its positive quadratic prior shares the same Laplacian, PCG, true-residual
//! confirmation and cancellation.

#[derive(Clone, Copy, Debug)]
pub struct SoftPriorInput<'a> {
    pub version: u32,
    /// Exactly 0/128/255, where 128 releases a user constraint. None means all
    /// pixels are unconstrained; it does not ask the solver to infer labels.
    pub user_constraints: Option<&'a [u8]>,
    /// A positive weight makes the unconstrained system positive definite.
    pub prior_weight: f64,
}

impl SoftPriorInput<'_> {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported matting input policy version".into());
        }
        if !self.prior_weight.is_finite() || !(1e-6..=100.0).contains(&self.prior_weight) {
            return Err("soft matting prior weight must be finite and in 0.000001..100".into());
        }
        Ok(())
    }
}

pub(crate) enum ConstraintInput<'a> {
    Legacy(&'a [u8]),
    Soft(SoftPriorInput<'a>),
}
