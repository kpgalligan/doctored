//! The destructive photographic operators. They share `filter_kernels`'s
//! neighborhood dispatcher and combine step with the selected and export
//! paths.
//!
//! This module has no byte exports of its own: `dr_image_filter_op`
//! parses Unsharp Mask, High Pass, Dust & Scratches and Reduce Noise with
//! `SelectedFilter::parse` and runs them through the one shared [`apply`]
//! (`ops_spatial::filter_op`). [`unsharp_mask_samples`] /
//! [`unsharp_mask_runs`] stay for the export workspace's output sharpening.
//! [`apply`] reads the caller's bytes in place, scaled as it reads them, rather
//! than holding an n×16 straight copy beside the n×16 plane.
//! `ops_spatial::filter_op_scratch_bytes` is its plan, which the host admits.
//!
//! The legacy `ops::sharpen` keeps its own historical formula and the legacy
//! Gaussian for existing recorded Actions: the image crate's kernel below σ4
//! and `sampling_gaussian_fast` from σ4 up. It is deliberately
//! NOT reimplemented here. These operators run against the destructive work ceiling
//! rather than the tighter interactive one, because a destructive pass runs
//! once instead of inside a coalescing preview.
//!
//! `ops_photographic.rs` holds the destructive photographic filters — Unsharp Mask, High Pass,
//! Reduce Noise and Dust & Scratches — as ONE shared `apply` (taking the caller's job, reading the
//! caller's bytes in place through a static 256-entry `f32::from(v) / 255.0` table, no straight
//! copy) that routes through `filter_kernels`'s neighborhood dispatcher and combine step.
use crate::filter_kernels::{evaluated, neighborhood_plan, neighborhood_plane};
use crate::filter_kernels::{FilterKind, Neighborhood};
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::sampling::Unorm;
use crate::sampling_gaussian::destructive_work_cap;
use image::RgbaImage;

/// `f32::from(v) / 255.0` for every byte code `v`: the value the former n×16
/// straight copy stored, so reading through it is bit-identical. The plane
/// passes below σ4 and the denoise windows read a pixel once per tap, and
/// converting on each read made them up to 1.4× slower than the copy did; a
/// table lookup costs about what loading the stored float did. Static, so it
/// is no per-call scratch.
static UNIT: [f32; 256] = {
    let mut table = [0.0; 256];
    let mut v = 0;
    while v < 256 {
        table[v] = v as f32 / 255.0;
        v += 1;
    }
    table
};

/// Shared destructive byte body behind `ops_spatial::filter_op`, under the
/// caller's `job` (`JobContext::NONE` uncontrolled). The caller has already
/// answered every exact bypass, so this always returns a filtered image;
/// `Option` keeps `filter_op`'s "nothing changed" shape.
pub(crate) fn apply(
    img: &RgbaImage,
    neighborhood: Neighborhood,
    kind: FilterKind,
    job: JobContext<'_>,
) -> Result<Option<RgbaImage>, String> {
    let (width, height) = img.dimensions();
    // Refuse an over-budget kernel BEFORE any pixel allocation. The plane pass
    // charges the same ceiling, but only after this function has already filled
    // a 16 bytes/px float plane — at the 100 MP image ceiling that is 1.6 GB
    // written and thrown away to learn the radius was never admissible. The
    // ceiling is `destructive_work_cap(job)`: none under a job control.
    let cap = destructive_work_cap(job);
    neighborhood_plan(neighborhood, (width, height), cap)?;
    let source = img.as_raw().as_chunks::<4>().0;
    let unit = |pixel: &[u8; 4]| pixel.map(|v| UNIT[usize::from(v)]);
    let plane = neighborhood_plane(
        neighborhood,
        (width, height),
        cap,
        |index| unit(&source[index]),
        job,
    )?;
    let mut out: Vec<u8> = reserved_vec(source.len() * 4)?;
    for (index, pixel) in source.iter().enumerate() {
        let value = evaluated(kind, unit(pixel), plane[index]);
        if value.iter().any(|v| !v.is_finite()) {
            return Err("filter produced an invalid component".into());
        }
        out.extend_from_slice(&value.map(|v| u8::quantize(v * 255.0)));
    }
    RgbaImage::from_raw(width, height, out)
        .map(Some)
        .ok_or_else(|| "invalid filtered image dimensions".into())
}

/// The shared Gaussian radius range, checked by name so every destructive
/// caller gets the same refusal.
fn gaussian_radius(sigma: f64) -> Result<Neighborhood, String> {
    if !sigma.is_finite() || !(0.0..=64.0).contains(&sigma) {
        return Err("filter radius must be a finite number of pixels in [0,64]".into());
    }
    Ok(Neighborhood::Gaussian { sigma })
}

/// Unsharp Mask over a caller's own STRAIGHT samples, at either depth — the
/// export workspace's output-sharpening stage, which runs it on u16 codes so
/// a 16-bit deliverable is never sharpened through a byte round trip.
///
/// The arithmetic is [`apply`]'s, generalized: the input is scaled by
/// `1 / T::MAX` into the same [0,1] plane the shared kernel reads, and the
/// result is quantized with `T::quantize(v * T::MAX)` — which at `T = u8` is
/// literally `apply`'s `u8::quantize(v * 255.0)`, so the byte codes agree with
/// `dr_image_filter_op`'s `unsharp_mask`.
///
/// `None` is the exact bypass (a zero radius or a zero amount), reported to
/// the caller as "nothing changed" rather than an identical copy.
pub(crate) fn unsharp_mask_samples<T: Unorm>(
    source: &[[T; 4]],
    dimensions: (u32, u32),
    sigma: f64,
    amount: f64,
    threshold: f64,
    job: JobContext<'_>,
) -> Result<Option<Vec<[T; 4]>>, String> {
    unsharp_mask_runs(
        std::iter::once(source),
        source.len(),
        dimensions,
        sigma,
        amount,
        threshold,
        job,
    )
}

/// [`unsharp_mask_samples`] over the caller's samples as consecutive runs
/// in plane order (a typed image's `runs_rgba16`), `count` samples in all:
/// the same refusals in the same order, then the same fill.
pub(crate) fn unsharp_mask_runs<'a, T: Unorm + 'a>(
    runs: impl Iterator<Item = &'a [[T; 4]]>,
    count: usize,
    dimensions: (u32, u32),
    sigma: f64,
    amount: f64,
    threshold: f64,
    job: JobContext<'_>,
) -> Result<Option<Vec<[T; 4]>>, String> {
    let neighborhood = gaussian_radius(sigma)?;
    if !amount.is_finite()
        || !(0.0..=5.0).contains(&amount)
        || !threshold.is_finite()
        || !(0.0..=1.0).contains(&threshold)
    {
        return Err(
            "unsharp mask amount must be in [0,5] and threshold in [0,1] normalized units".into(),
        );
    }
    if sigma == 0.0 || amount == 0.0 {
        return Ok(None);
    }
    let (width, height) = dimensions;
    if count != (width as usize).saturating_mul(height as usize) {
        return Err("unsharp mask sample count differs from the dimensions".into());
    }
    // Refuse an over-budget kernel BEFORE any pixel allocation, exactly as
    // `apply` does: the plane pass charges the same ceiling, but only after a
    // 16 bytes/px float plane has already been filled and thrown away. The
    // ceiling is `destructive_work_cap(job)`: none under a job control.
    let cap = destructive_work_cap(job);
    neighborhood_plan(neighborhood, dimensions, cap)?;
    let kind = FilterKind::UnsharpMask { amount, threshold };
    let mut straight: Vec<[f32; 4]> = reserved_vec(count)?;
    for run in runs {
        for pixel in run {
            straight.push(pixel.map(|v| v.raw() / T::MAX));
        }
    }
    let plane = {
        let input: &[[f32; 4]] = &straight;
        neighborhood_plane(neighborhood, dimensions, cap, |index| input[index], job)?
    };
    let mut out: Vec<[T; 4]> = reserved_vec(count)?;
    for (index, sample) in straight.iter().enumerate() {
        let value = evaluated(kind, *sample, plane[index]);
        if value.iter().any(|v| !v.is_finite()) {
            return Err("filter produced an invalid component".into());
        }
        out.push(value.map(|v| T::quantize(v * T::MAX)));
    }
    Ok(Some(out))
}
