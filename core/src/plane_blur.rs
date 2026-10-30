//! One multiresolution scalar blur schedule, shared by legacy byte styles and
//! typed masks. Byte resizes retain image's established rounding; float fields
//! use the existing cancellable triangle resampler with no quantization.
use crate::gaussian_plane::{self, Scalar};
use crate::job_control::JobContext;
use crate::style_render::{downsample_factor, feather_radius, reduced_sigma};

trait Sample: Scalar {
    fn resize(
        source: &[Self],
        from: (u32, u32),
        to: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<Vec<Self>, String>;
}
impl Sample for u8 {
    fn resize(
        source: &[Self],
        from: (u32, u32),
        to: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<Vec<Self>, String> {
        job.check()?;
        let image = image::ImageBuffer::<image::Luma<u8>, _>::from_raw(from.0, from.1, source)
            .ok_or("invalid scalar blur source")?;
        let result =
            image::imageops::resize(&image, to.0, to.1, image::imageops::FilterType::Triangle)
                .into_raw();
        job.check()?;
        Ok(result)
    }
}
impl Sample for f32 {
    fn resize(
        source: &[Self],
        from: (u32, u32),
        to: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<Vec<Self>, String> {
        Ok(
            crate::sampling::resize_fields::<1>(source.as_chunks::<1>().0, from, to, job)?
                .into_flattened(),
        )
    }
}
fn blur<T: Sample>(
    plane: &mut [T],
    w: u32,
    h: u32,
    sigma: f32,
    job: JobContext<'_>,
) -> Result<(), String> {
    job.check()?;
    if sigma.is_nan() || sigma <= 0.0 || w == 0 || h == 0 || plane.len() != w as usize * h as usize
    {
        return Ok(());
    }
    let factor = downsample_factor(sigma);
    if factor == 1 {
        return gaussian_plane::feather(plane, w, h, feather_radius(sigma), job);
    }
    let we = w.div_ceil(factor) * factor;
    let he = h.div_ceil(factor) * factor;
    let mut extended = vec![T::from_filtered(0.0); we as usize * he as usize];
    for y in 0..h as usize {
        job.check()?;
        for start in (0..w as usize).step_by(4096) {
            job.check()?;
            let end = (start + 4096).min(w as usize);
            extended[y * we as usize + start..y * we as usize + end]
                .copy_from_slice(&plane[y * w as usize + start..y * w as usize + end]);
        }
    }
    let (rw, rh) = (we / factor, he / factor);
    let mut reduced = T::resize(&extended, (we, he), (rw, rh), job)?;
    gaussian_plane::feather(
        &mut reduced,
        rw,
        rh,
        feather_radius(reduced_sigma(sigma, factor)),
        job,
    )?;
    let back = T::resize(&reduced, (rw, rh), (we, he), job)?;
    for y in 0..h as usize {
        job.check()?;
        for start in (0..w as usize).step_by(4096) {
            job.check()?;
            let end = (start + 4096).min(w as usize);
            plane[y * w as usize + start..y * w as usize + end]
                .copy_from_slice(&back[y * we as usize + start..y * we as usize + end]);
        }
    }
    job.check()
}
/// The byte blur, polling `job` — the byte twin of [`floats`] (Phase
/// 13A-5a).
pub(crate) fn bytes_with_job(
    plane: &mut [u8],
    w: u32,
    h: u32,
    sigma: f32,
    job: JobContext<'_>,
) -> Result<(), String> {
    blur(plane, w, h, sigma, job)
}
pub(crate) fn floats(
    plane: &mut [f32],
    w: u32,
    h: u32,
    sigma: f32,
    job: JobContext<'_>,
) -> Result<(), String> {
    blur(plane, w, h, sigma, job)
}

/// Includes caller's full plane, extension, separable sampling intermediates,
/// returned upsample and Gaussian scratch. Every extended image is bounded.
pub(crate) fn float_plan(w: u32, h: u32, sigma: f32) -> Result<(u64, u64), String> {
    let factor = downsample_factor(sigma);
    let we = w
        .div_ceil(factor)
        .checked_mul(factor)
        .ok_or("scalar blur extent overflow")?;
    let he = h
        .div_ceil(factor)
        .checked_mul(factor)
        .ok_or("scalar blur extent overflow")?;
    let pixels = crate::precision_image::pixel_count(we, he)? as u64;
    let reduced = pixels / u64::from(factor).pow(2);
    let actual_sigma = if factor == 1 {
        sigma
    } else {
        reduced_sigma(sigma, factor)
    };
    let taps =
        u64::from(gaussian_plane::nearest_odd(feather_radius(actual_sigma) * 2.0 + 1.0).max(3));
    let work = pixels * 32 + reduced * 2 * taps;
    Ok((pixels * 32 + taps * 4 + 4096, work))
}

/// [`float_plan`]'s WORK figure alone — `float_plan` is the master formula
/// (the factor, the extent rounded up to it, the reduced plane, the actual
/// sigma, the taps, `pixels·32 + reduced·2·taps`) — INFALLIBLE: u64
/// saturating arithmetic throughout and no `pixel_count` refusal, so a work
/// term built on it can never add a refusal to a plan that admits the blur
/// (Phase 13A-5b-iii-b-2, the byte regional plan's mask term). Agrees with
/// `float_plan` on every extent `float_plan` admits.
pub(crate) fn float_work(w: u32, h: u32, sigma: f32) -> u64 {
    let factor = u64::from(downsample_factor(sigma));
    let we = u64::from(w).div_ceil(factor).saturating_mul(factor);
    let he = u64::from(h).div_ceil(factor).saturating_mul(factor);
    let pixels = we.saturating_mul(he);
    let reduced = pixels / factor.pow(2);
    let actual_sigma = if factor == 1 {
        sigma
    } else {
        reduced_sigma(sigma, factor as u32)
    };
    let taps =
        u64::from(gaussian_plane::nearest_odd(feather_radius(actual_sigma) * 2.0 + 1.0).max(3));
    pixels
        .saturating_mul(32)
        .saturating_add(reduced.saturating_mul(2).saturating_mul(taps))
}
