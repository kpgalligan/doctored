//! One separable Gaussian scalar kernel, extracted from doc_select's feather
//! path without changing its byte arithmetic/order. Typed tone guides use the
//! same kernel with f32 storage, retaining fractional luma and coverage.
use crate::job_control::JobContext;

pub(crate) trait Scalar: Copy {
    fn raw(self) -> f32;
    fn from_filtered(value: f32) -> Self;
}
impl Scalar for u8 {
    fn raw(self) -> f32 {
        f32::from(self)
    }
    fn from_filtered(value: f32) -> Self {
        value.clamp(0.0, 255.0).round() as u8
    }
}
impl Scalar for f32 {
    fn raw(self) -> f32 {
        self
    }
    fn from_filtered(value: f32) -> Self {
        value.clamp(0.0, 1.0)
    }
}
pub(crate) fn feather<T: Scalar>(
    mask: &mut [T],
    width: u32,
    height: u32,
    radius: f32,
    control: JobContext<'_>,
) -> Result<(), String> {
    let (w, h) = (width as usize, height as usize);
    if !radius.is_finite() || radius <= 0.0 || w == 0 || h == 0 || mask.len() != w * h {
        return Ok(());
    }
    let taps = nearest_odd(radius * 2.0 + 1.0).max(3);
    let half = (taps / 2) as i64;
    let sigma = 0.3 * (radius - 1.0) + 0.8;
    // Normalized 1-D Gaussian; the 1/(sqrt(2*pi)*sigma) scale cancels.
    let mut kernel: Vec<f32> = (0..taps)
        .map(|i| (-0.5 * ((i as i64 - half) as f32 / sigma).powi(2)).exp())
        .collect();
    let sum: f32 = kernel.iter().sum();
    for k in &mut kernel {
        *k /= sum;
    }

    // Horizontal pass into a float plane, then vertical back into the mask,
    // both clamping sample coordinates to the edges.
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        control.check()?;
        let row = &mask[y * w..(y + 1) * w];
        for x in 0..w {
            control.every(x)?;
            let mut acc = 0.0;
            for (i, k) in kernel.iter().enumerate() {
                let sx = (x as i64 + i as i64 - half).clamp(0, w as i64 - 1) as usize;
                acc += k * row[sx].raw();
            }
            tmp[y * w + x] = acc;
        }
        control.report(0.5 * (y + 1) as f64 / h as f64, 6)?;
    }
    for y in 0..h {
        control.check()?;
        for x in 0..w {
            control.every(x)?;
            let mut acc = 0.0;
            for (i, k) in kernel.iter().enumerate() {
                let sy = (y as i64 + i as i64 - half).clamp(0, h as i64 - 1) as usize;
                acc += k * tmp[sy * w + x];
            }
            mask[y * w + x] = T::from_filtered(acc);
        }
        control.report(0.5 + 0.5 * (y + 1) as f64 / h as f64, 6)?;
    }
    Ok(())
}

/// Nearest odd integer to `x` (ties resolve to the lower odd value),
/// mirroring the image crate's kernel-size rounding. `x` must be >= 1.
pub(crate) fn nearest_odd(x: f32) -> u32 {
    let n = x.round().max(1.0) as u32;
    if n % 2 == 1 {
        n
    } else if x - (n - 1) as f32 <= (n + 1) as f32 - x {
        n - 1
    } else {
        n + 1
    }
}
