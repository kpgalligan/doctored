//! Experimental trimap-based foreground/background sampling; no host exposure.
//!
//! The alpha projection and color/spatial cost follow equations 2–5 of He,
//! Rhemann, Rother, Tang and Sun, *A Global Sampling Method for Alpha Matting*,
//! CVPR 2011: https://www.microsoft.com/en-us/research/wp-content/uploads/2011/06/HeEtAl.pdf
//! Our search is a bounded nearest-boundary plus global-color candidate set,
//! not that paper's SampleMatch algorithm or a claim to reproduce its results.
//! Wang and Cohen's *Optimized Color Sampling for Robust Matting* (CVPR 2007)
//! motivates exposing sample reliability instead of treating every fit as fact.
//!
//! Colors are finite normalized RGB in an explicitly caller-selected domain.
//! No transfer curve or implicit sRGB assumption exists here. Encoded fixtures
//! must use encoded colors; linear compositing requires colors decoded through
//! the document profile before this call. Samples and results stay in that
//! domain. Unknown pixels need plausible known F AND B colors. A fully
//! translucent object without opaque F evidence is underdetermined and cannot
//! be recovered from a trimap automatically. Incorrect known labels are pinned,
//! not secretly repaired using a segmentation guess or ground truth.
//!
//! experimental categorical-trimap alpha estimation with bounded foreground/background sampling and
//! explicit unresolved confidence. Known labels are hard constraints; absent sample classes refuse.
//! Spatial tree searches also poll cancellation. Keep the original 24-case gate and separate alpha,
//! reference-alpha color and combined alpha/color results.

use crate::{
    doc::MAX_PIXELS,
    job_control::{DrJobControl, JobContext},
    matting_samples::{Nearby, Sample, SampleIndex, CANDIDATES},
};
use image::GrayImage;

/// Work limits are refusals, never silent downsampling of unknown coverage.
pub const MAX_UNKNOWN_PIXELS: usize = 1_000_000;
pub const MAX_BOUNDARY_SAMPLES: usize = 250_000;

/// Fixed defaults precede quality measurement. Distances use normalized RGB.
#[derive(Clone, Copy, Debug)]
pub struct MattingParams {
    pub minimum_separation: f32,
    pub maximum_residual: f32,
    pub spatial_weight: f32,
}

impl Default for MattingParams {
    fn default() -> Self {
        Self {
            // Twelve encoded levels of separation still gives substantial
            // quantization uncertainty. Below this threshold no new alpha is
            // claimed. The units change only when the caller selects a domain.
            minimum_separation: 12.0 / 255.0,
            maximum_residual: 8.0 / 255.0,
            // Paper eq.5 uses RGB 0..255 with weight1 for the spatial term.
            spatial_weight: 1.0 / 255.0,
        }
    }
}

/// Sample colors are meaningful for solved UNKNOWN pixels. At pinned pixels,
/// the corresponding known color is exact and the unobserved color is zero.
/// Confidence is an explicit fit/separation diagnostic, NOT a calibrated
/// probability. Zero-confidence unknown pixels retain the prior byte exactly.
pub struct SampledMatte {
    pub alpha: GrayImage,
    pub foreground: Vec<[f32; 3]>,
    pub background: Vec<[f32; 3]>,
    pub confidence: GrayImage,
    pub unknown_pixels: usize,
    pub unresolved_pixels: usize,
    pub foreground_samples: usize,
    pub background_samples: usize,
}

/// Estimate a new matte without mutating any input. Trimap labels must be
/// exactly0/background,128/unknown,255/foreground. Known constraints override
/// prior alpha; an explicit unknown label relaxes either known constraint.
/// Missing F/B samples, excess work, malformed buffers and cancellation return
/// an error. Optional controls are single-use and reserve scratch before any
/// large allocation. A completed result is published only after every check.
pub fn estimate(
    colors: &[[f32; 3]],
    width: u32,
    height: u32,
    trimap: &[u8],
    prior: &[u8],
    params: &MattingParams,
    control: Option<&DrJobControl>,
) -> Result<SampledMatte, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let result = estimate_inner(colors, width, height, trimap, prior, params, job)?;
    job.report(1.0, 5)?;
    Ok(result)
}

/// Conservative retained output plus temporary boundary index, excluding
/// caller-owned RGB/trimap/prior. All output planes total26 bytes per pixel.
pub fn scratch_estimate(width: u32, height: u32) -> Option<u64> {
    let pixels = u64::from(width).checked_mul(u64::from(height))?;
    pixels
        .checked_mul(26)?
        .checked_add((MAX_BOUNDARY_SAMPLES * std::mem::size_of::<Sample>()) as u64)?
        .checked_add(64 * 1024)
}

fn estimate_inner(
    colors: &[[f32; 3]],
    width: u32,
    height: u32,
    trimap: &[u8],
    prior: &[u8],
    params: &MattingParams,
    job: JobContext<'_>,
) -> Result<SampledMatte, String> {
    let n = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || n > MAX_PIXELS
        || n as usize != colors.len()
        || trimap.len() != colors.len()
        || prior.len() != colors.len()
    {
        return Err("matting dimensions or buffer lengths are invalid".into());
    }
    if !params.minimum_separation.is_finite()
        || params.minimum_separation <= 0.0
        || !params.maximum_residual.is_finite()
        || params.maximum_residual <= 0.0
        || !params.spatial_weight.is_finite()
        || params.spatial_weight < 0.0
    {
        return Err("matting sample parameters must be finite and positive".into());
    }
    let (w, h) = (width as usize, height as usize);
    let mut unknown = 0;
    let mut samples = [0_usize; 2];
    for i in 0..colors.len() {
        job.every(i)?;
        if colors[i]
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("matting colors must be finite normalized RGB".into());
        }
        match trimap[i] {
            128 => unknown += 1,
            label @ (0 | 255) => {
                if boundary(trimap, w, h, i) {
                    samples[usize::from(label == 255)] += 1;
                }
            }
            _ => return Err("matting trimap labels must be exactly 0, 128 or 255".into()),
        }
    }
    if unknown > MAX_UNKNOWN_PIXELS {
        return Err(format!(
            "matting has {unknown} unknown pixels; limit is {MAX_UNKNOWN_PIXELS}; narrow the unknown band or refine a smaller area"
        ));
    }
    if samples.iter().sum::<usize>() > MAX_BOUNDARY_SAMPLES {
        return Err(format!(
            "matting boundary exceeds {MAX_BOUNDARY_SAMPLES} samples; narrow the unknown band or refine a smaller area"
        ));
    }
    if unknown > 0 && samples.contains(&0) {
        return Err("matting unknown region needs known foreground and background samples; a translucent object may require additional color evidence".into());
    }
    let scratch = scratch_estimate(width, height).ok_or("matting scratch size overflow")?;
    job.reserve(scratch)?;
    let mut foreground_samples = Vec::with_capacity(samples[1]);
    let mut background_samples = Vec::with_capacity(samples[0]);
    for i in 0..colors.len() {
        job.every(i)?;
        if trimap[i] != 128 && boundary(trimap, w, h, i) {
            let sample = Sample {
                x: (i % w) as u32,
                y: (i / w) as u32,
                rgb: colors[i],
            };
            if trimap[i] == 255 {
                foreground_samples.push(sample);
            } else {
                background_samples.push(sample);
            }
        }
    }
    let foreground_index = SampleIndex::new(foreground_samples, job)?;
    let background_index = SampleIndex::new(background_samples, job)?;
    job.report(0.15, 6)?;
    let mut alpha = vec![0; colors.len()];
    let mut confidence = vec![0; colors.len()];
    let mut foreground = vec![[0.0; 3]; colors.len()];
    let mut background = vec![[0.0; 3]; colors.len()];
    let mut unresolved = 0;
    let mut nearby_f = [Nearby::default(); CANDIDATES];
    let mut nearby_b = [Nearby::default(); CANDIDATES];
    for i in 0..colors.len() {
        // A pixel tests at most256 sample pairs. Poll every64 targets rather
        // than the dense-cell interval to keep cancellation bounded.
        if i & 63 == 0 {
            job.check()?;
        }
        match trimap[i] {
            255 => {
                alpha[i] = 255;
                foreground[i] = colors[i];
                confidence[i] = 255;
            }
            0 => {
                background[i] = colors[i];
                confidence[i] = 255;
            }
            _ => {
                alpha[i] = prior[i];
                let x = (i % w) as u32;
                let y = (i / w) as u32;
                let f_count = foreground_index.nearby(x, y, &mut nearby_f, job)?;
                let b_count = background_index.nearby(x, y, &mut nearby_b, job)?;
                let found = choose_pair(
                    colors[i],
                    &nearby_f[..f_count],
                    &nearby_b[..b_count],
                    params,
                );
                if let Some(fit) = found {
                    alpha[i] = (fit.alpha * 255.0).round() as u8;
                    foreground[i] = fit.foreground;
                    background[i] = fit.background;
                    confidence[i] = (fit.confidence * 255.0).round().max(1.0) as u8;
                } else {
                    unresolved += 1;
                }
            }
        }
        if i % w == w - 1 {
            job.report(0.15 + 0.85 * (i + 1) as f64 / colors.len() as f64, 6)?;
        }
    }
    Ok(SampledMatte {
        alpha: GrayImage::from_raw(width, height, alpha).ok_or("invalid matting output")?,
        foreground,
        background,
        confidence: GrayImage::from_raw(width, height, confidence)
            .ok_or("invalid confidence output")?,
        unknown_pixels: unknown,
        unresolved_pixels: unresolved,
        foreground_samples: samples[1],
        background_samples: samples[0],
    })
}

fn boundary(trimap: &[u8], w: usize, h: usize, i: usize) -> bool {
    let (x, y) = (i % w, i / w);
    (x > 0 && trimap[i - 1] == 128)
        || (x + 1 < w && trimap[i + 1] == 128)
        || (y > 0 && trimap[i - w] == 128)
        || (y + 1 < h && trimap[i + w] == 128)
}

struct Fit {
    alpha: f32,
    foreground: [f32; 3],
    background: [f32; 3],
    confidence: f32,
}

fn choose_pair(color: [f32; 3], f: &[Nearby], b: &[Nearby], params: &MattingParams) -> Option<Fit> {
    let nearest_f = f.first()?.distance.max(1.0);
    let nearest_b = b.first()?.distance.max(1.0);
    let mut best_cost = f32::INFINITY;
    let mut best = None;
    for foreground in f {
        for background in b {
            let fc = foreground.sample.rgb;
            let bc = background.sample.rgb;
            let mut numerator = 0.0;
            let mut denominator = 0.0;
            for channel in 0..3 {
                let difference = fc[channel] - bc[channel];
                numerator += (color[channel] - bc[channel]) * difference;
                denominator += difference * difference;
            }
            let separation = denominator.sqrt();
            if separation <= params.minimum_separation {
                continue;
            }
            let alpha = (numerator / denominator).clamp(0.0, 1.0);
            let residual = (0..3)
                .map(|c| (color[c] - (alpha * fc[c] + (1.0 - alpha) * bc[c])).powi(2))
                .sum::<f32>()
                .sqrt();
            if residual >= params.maximum_residual {
                continue;
            }
            let spatial = foreground.distance / nearest_f + background.distance / nearest_b;
            let cost = residual + params.spatial_weight * spatial;
            if cost < best_cost {
                best_cost = cost;
                let separation_confidence =
                    ((separation - params.minimum_separation) / params.minimum_separation).min(1.0);
                best = Some(Fit {
                    alpha,
                    foreground: fc,
                    background: bc,
                    confidence: separation_confidence * (1.0 - residual / params.maximum_residual),
                });
            }
        }
    }
    best
}
