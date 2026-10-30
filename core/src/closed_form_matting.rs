//! Experimental local color-line alpha solve; no FFI or editor exposure.
//!
//! Levin, Lischinski and Weiss, A Closed Form Solution to Natural Image Matting,
//! equations10–12: https://doi.org/10.1109/TPAMI.2007.1177 . Known trimap
//! constraints are exact. The prior initializes PCG only; it is not hidden
//! ground truth or an extra data term. Numerical convergence does NOT establish
//! that the image's alpha is identifiable. Missing foreground/background labels,
//! exceeded work budgets and nonconvergence refuse instead of publishing a matte.
//!
//! experimental spatial color-line alpha solver with exact known labels, a compact local stencil
//! and preconditioned conjugate gradients. The original entry point retains hard categorical labels
//! and initializer-only prior semantics; nonconvergence and absent constraint classes refuse.
//! Neither numerical convergence nor a soft prior is physical confidence.

use crate::{
    doc::MAX_PIXELS,
    job_control::{DrJobControl, JobContext},
    matting_input::{ConstraintInput, SoftPriorInput},
    matting_laplacian::Laplacian,
};
use image::GrayImage;

pub const MAX_UNKNOWN_PIXELS: usize = 500_000;
pub const MAX_ACTIVE_WINDOWS: usize = 1_000_000;
pub(crate) const MAX_STENCIL_TERMS: u64 = 20_000_000_000;

#[derive(Clone, Copy, Debug)]
pub struct ClosedFormParams {
    pub epsilon: f64,
    pub max_iterations: usize,
    pub residual_rms_tolerance: f64,
}

impl Default for ClosedFormParams {
    fn default() -> Self {
        Self {
            // Nine independent uniform8-bit quantization errors have summed
            // variance9/(12*255²). This regularizes a3x3 encoded-RGB window
            // at its measurement noise floor. Other domains select epsilon.
            epsilon: 9.0 / (12.0 * 255.0 * 255.0),
            max_iterations: 500,
            residual_rms_tolerance: 1e-6,
        }
    }
}

impl ClosedFormParams {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.epsilon.is_finite()
            || !(1e-9..=1.0).contains(&self.epsilon)
            || !(1..=1000).contains(&self.max_iterations)
            || !self.residual_rms_tolerance.is_finite()
            || !(1e-9..=1e-3).contains(&self.residual_rms_tolerance)
        {
            return Err(
                "closed-form matting parameters are outside finite supported ranges".into(),
            );
        }
        Ok(())
    }
}

pub struct ClosedFormMatte {
    pub alpha: GrayImage,
    pub unknown_pixels: usize,
    pub active_windows: usize,
    pub iterations: usize,
    /// True reduced-system residual before final0..1 clipping/byte encoding.
    pub residual_rms: f64,
    pub relative_residual: f64,
    pub clipped_pixels: usize,
}

/// Conservative scratch including output, excluding borrowed colors/trimap/prior.
/// 5N covers a full-canvas u32 index and alpha byte;260U covers the25-double
/// stencil, RHS, pixel indices and five PCG vectors with alignment headroom.
pub fn scratch_estimate(width: u32, height: u32, unknown: usize) -> Option<u64> {
    (u64::from(width) * u64::from(height))
        .checked_mul(5)?
        .checked_add((unknown as u64).checked_mul(260)?)?
        .checked_add(64 * 1024)
}

/// Immutable safe experimental API. RGB must be finite0..1 in a declared
/// caller-selected compositing domain; there is no implicit profile conversion.
/// Trimap labels are exactly0/128/255. A NULL-equivalent optional token runs
/// the same bounded algorithm without cancellation. Tokens remain single-use.
pub fn estimate(
    colors: &[[f32; 3]],
    width: u32,
    height: u32,
    trimap: &[u8],
    prior: &[u8],
    params: &ClosedFormParams,
    control: Option<&DrJobControl>,
) -> Result<ClosedFormMatte, String> {
    estimate_input(
        colors,
        (width, height),
        ConstraintInput::Legacy(trimap),
        prior,
        params,
        control,
    )
}

/// Version-1 uncertain automatic prior plus optional explicit user constraints.
/// This shares the legacy stencil, PCG, admission and residual confirmation.
/// No known sample class is required: the positive prior term removes the
/// nullspace, but cannot establish physical identifiability or qualify quality.
pub fn estimate_soft_prior(
    colors: &[[f32; 3]],
    size: (u32, u32),
    prior: &[u8],
    input: SoftPriorInput<'_>,
    params: &ClosedFormParams,
    control: Option<&DrJobControl>,
) -> Result<ClosedFormMatte, String> {
    estimate_input(
        colors,
        size,
        ConstraintInput::Soft(input),
        prior,
        params,
        control,
    )
}

fn estimate_input(
    colors: &[[f32; 3]],
    (width, height): (u32, u32),
    input: ConstraintInput<'_>,
    prior: &[u8],
    params: &ClosedFormParams,
    control: Option<&DrJobControl>,
) -> Result<ClosedFormMatte, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    estimate_input_job(colors, (width, height), input, prior, params, job)
}

pub(crate) fn estimate_input_job(
    colors: &[[f32; 3]],
    (width, height): (u32, u32),
    input: ConstraintInput<'_>,
    prior: &[u8],
    params: &ClosedFormParams,
    job: JobContext<'_>,
) -> Result<ClosedFormMatte, String> {
    let n = u64::from(width) * u64::from(height);
    let (constraints, prior_weight) = match input {
        ConstraintInput::Legacy(trimap) => (Some(trimap), 0.0),
        ConstraintInput::Soft(input) => {
            input.validate()?;
            (input.user_constraints, input.prior_weight)
        }
    };
    if width == 0
        || height == 0
        || n > MAX_PIXELS
        || colors.len() != n as usize
        || constraints.is_some_and(|labels| labels.len() != colors.len())
        || prior.len() != colors.len()
    {
        return Err("closed-form matting dimensions or buffer lengths are invalid".into());
    }
    // Missing explicit constraints makes every pixel unknown. Refuse before
    // allocating the label plane; the existing full-resolution caps stay fixed.
    if constraints.is_none() && n > MAX_UNKNOWN_PIXELS as u64 {
        return Err(format!(
            "closed-form matting has {n} unknown pixels; limit is {MAX_UNKNOWN_PIXELS}; narrow the unknown band or refine a smaller area"
        ));
    }
    let owned_labels;
    let label_scratch = if constraints.is_none() { n } else { 0 };
    let trimap = match constraints {
        Some(labels) => labels,
        None => {
            job.reserve(
                scratch_estimate(width, height, n as usize)
                    .and_then(|bytes| bytes.checked_add(n))
                    .ok_or("closed-form scratch overflow")?,
            )?;
            owned_labels = vec![128; n as usize];
            &owned_labels
        }
    };
    params.validate()?;
    let mut counts = [0_usize; 3];
    for (i, (&label, color)) in trimap.iter().zip(colors).enumerate() {
        job.every(i)?;
        if color
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("closed-form colors must be finite normalized RGB".into());
        }
        counts[match label {
            0 => 0,
            128 => 1,
            255 => 2,
            _ => return Err("closed-form trimap labels must be exactly0,128 or255".into()),
        }] += 1;
    }
    let unknown = counts[1];
    if unknown > MAX_UNKNOWN_PIXELS {
        return Err(format!(
            "closed-form matting has {unknown} unknown pixels; limit is {MAX_UNKNOWN_PIXELS}; narrow the unknown band or refine a smaller area"
        ));
    }
    if prior_weight == 0.0 && unknown > 0 && (counts[0] == 0 || counts[2] == 0) {
        return Err("closed-form matting needs known foreground and background constraints; missing translucent-foreground evidence cannot be inferred".into());
    }
    // Two preconditioner triangular sweeps plus a matrix product per iteration,
    // with true-residual confirmation and restart: five passes per iteration
    // conservatively include an extra residual product and dense vector work.
    let terms = (unknown as u64) * 25 * (5 * params.max_iterations as u64 + 3);
    if terms > MAX_STENCIL_TERMS {
        return Err(
            "closed-form matting exceeds the bounded solver work budget; narrow the unknown band or refine a smaller area"
                .into(),
        );
    }
    let (w, h) = (width as usize, height as usize);
    let (ww, wh) = (w.min(3), h.min(3));
    let mut windows = 0;
    if unknown > 0 {
        for y in 0..=h - wh {
            job.check()?;
            for x in 0..=w - ww {
                job.every(x)?;
                if (0..wh).any(|dy| (0..ww).any(|dx| trimap[(y + dy) * w + x + dx] == 128)) {
                    windows += 1;
                    if windows > MAX_ACTIVE_WINDOWS {
                        return Err("closed-form matting exceeds the bounded window budget; narrow the unknown band or refine a smaller area".into());
                    }
                }
            }
        }
    }
    job.reserve(
        scratch_estimate(width, height, unknown)
            .and_then(|bytes| bytes.checked_add(label_scratch))
            .ok_or("closed-form scratch overflow")?,
    )?;
    if unknown == 0 {
        let output = copy_coverage(trimap, job)?;
        job.report(1.0, 5)?;
        return Ok(ClosedFormMatte {
            alpha: GrayImage::from_raw(width, height, output)
                .ok_or("invalid closed-form output")?,
            unknown_pixels: 0,
            active_windows: 0,
            iterations: 0,
            residual_rms: 0.0,
            relative_residual: 0.0,
            clipped_pixels: 0,
        });
    }
    let system = Laplacian::build(
        colors,
        (width, height),
        trimap,
        unknown,
        params.epsilon,
        (prior_weight > 0.0).then_some((prior, prior_weight)),
        job,
    )?;
    job.report(0.2, 6)?;
    let mut initial = Vec::with_capacity(unknown);
    for (k, &i) in system.pixels.iter().enumerate() {
        job.every(k)?;
        initial.push(f64::from(prior[i as usize]) / 255.0);
    }
    let (values, iterations, residual_rms, relative_residual) =
        solve(&system, initial, params, job.range(0.2, 0.98))?;
    let mut output = copy_coverage(trimap, job)?;
    let mut clipped = 0;
    for (k, (&pixel, &value)) in system.pixels.iter().zip(&values).enumerate() {
        job.every(k)?;
        clipped += usize::from(!(0.0..=1.0).contains(&value));
        output[pixel as usize] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    job.report(1.0, 5)?;
    Ok(ClosedFormMatte {
        alpha: GrayImage::from_raw(width, height, output).ok_or("invalid closed-form output")?,
        unknown_pixels: unknown,
        active_windows: windows,
        iterations,
        residual_rms,
        relative_residual,
        clipped_pixels: clipped,
    })
}

fn copy_coverage(source: &[u8], job: JobContext<'_>) -> Result<Vec<u8>, String> {
    let mut output = Vec::with_capacity(source.len());
    for chunk in source.chunks(4096) {
        job.check()?;
        output.extend_from_slice(chunk);
    }
    job.check()?;
    Ok(output)
}

fn norm_squared(values: &[f64], job: JobContext<'_>) -> Result<f64, String> {
    dot(values, values, job)
}
fn dot(a: &[f64], b: &[f64], job: JobContext<'_>) -> Result<f64, String> {
    let mut sum = 0.0;
    for (i, (&x, &y)) in a.iter().zip(b).enumerate() {
        job.every(i)?;
        sum += x * y;
    }
    Ok(sum)
}

fn solve(
    system: &Laplacian,
    mut x: Vec<f64>,
    params: &ClosedFormParams,
    job: JobContext<'_>,
) -> Result<(Vec<f64>, usize, f64, f64), String> {
    let n = x.len();
    let mut ax = vec![0.0; n];
    system.apply(&x, &mut ax, job)?;
    let mut r = Vec::with_capacity(n);
    for (i, (&b, &v)) in system.rhs.iter().zip(&ax).enumerate() {
        job.every(i)?;
        r.push(b - v);
    }
    let mut z = vec![0.0; n];
    system.precondition(&r, &mut z, job)?;
    let mut p = z.clone();
    let rhs_norm = norm_squared(&system.rhs, job)?.sqrt();
    let mut rz = dot(&r, &z, job)?;
    let threshold = params.residual_rms_tolerance * (n as f64).sqrt();
    for iteration in 0..=params.max_iterations {
        let mut residual = norm_squared(&r, job)?.sqrt();
        // Recurrence residual can drift on ill-conditioned systems. Confirm
        // with b-Ax before declaring success. A false recurrence convergence
        // restarts from that measured residual, never accepts an optimistic one.
        let recompute = residual <= threshold || iteration == params.max_iterations;
        if recompute {
            system.apply(&x, &mut ax, job)?;
            for i in 0..n {
                job.every(i)?;
                r[i] = system.rhs[i] - ax[i];
            }
            residual = norm_squared(&r, job)?.sqrt();
            if residual <= threshold {
                return Ok((
                    x,
                    iteration,
                    residual / (n as f64).sqrt(),
                    residual / rhs_norm.max(f64::EPSILON),
                ));
            }
            system.precondition(&r, &mut z, job)?;
            p.copy_from_slice(&z);
            rz = dot(&r, &z, job)?;
        }
        if iteration == params.max_iterations {
            return Err(format!("closed-form matting did not converge after {iteration} iterations (RMS residual {:.6e}, relative {:.6e}); add constraints or reduce the unknown region",residual/(n as f64).sqrt(),residual/rhs_norm.max(f64::EPSILON)));
        }
        system.apply(&p, &mut ax, job)?;
        let denominator = dot(&p, &ax, job)?;
        if !denominator.is_finite() || denominator <= 0.0 || !rz.is_finite() || rz <= 0.0 {
            return Err("closed-form matting solve lost positive definiteness".into());
        }
        let step = rz / denominator;
        for i in 0..n {
            job.every(i)?;
            x[i] += step * p[i];
            r[i] -= step * ax[i];
        }
        system.precondition(&r, &mut z, job)?;
        let next = dot(&r, &z, job)?;
        let beta = next / rz;
        for i in 0..n {
            job.every(i)?;
            p[i] = z[i] + beta * p[i];
        }
        rz = next;
        job.report((iteration + 1) as f64 / params.max_iterations as f64, 6)?;
    }
    unreachable!("iteration limit returns above")
}
