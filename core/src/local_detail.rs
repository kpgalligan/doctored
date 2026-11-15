//! A local's Sharpness and Noise Reduction: the detail stage
//! of a Develop Filter mask (`develop_filter_masks`) and of a RAW Develop
//! local's tone (`raw_local_tone_apply`), the one home of the mapping from
//! the two sliders onto the global filters. It adds no pixel math: each
//! stage is an existing kernel, called, never copied.
//!
//! The mapping, in order (a control at 0 adds no stage):
//! 1. **Noise Reduction** `n` > 0 is Filter ▸ Noise ▸ Reduce Noise at radius
//!    [`NOISE_RADIUS`], Luminance `n/100 ·` Smart Sharpen's
//!    `NOISE_LEVEL_AT_FULL`, Preserve Details 0 and Colour Noise 0: the
//!    guided-denoise correction plane (`filter_kernels::neighborhood_plane_in`,
//!    `Neighborhood::Denoise` with the halves `sampling_denoise::halves`
//!    picks), then Reduce Noise's combine (`filter_kernels::evaluated_in`,
//!    `FilterKind::ReduceNoise`) written over it in place, the master copy
//!    both `reduce_noise` routes run.
//! 2. **Sharpness**, over that result. `s` > 0 is Smart Sharpen
//!    (`sampling_smart_sharpen::SmartSharpen::plane_in`, the master copy) at
//!    amount `s/100 ·` [`SHARPEN_AMOUNT_AT_FULL`] %, radius
//!    [`SHARPEN_RADIUS`], Reduce Noise 0, Remove Gaussian Blur and both fades
//!    0, which is Unsharp Mask at threshold 0 bit for bit. `s` < 0 softens:
//!    Gaussian Blur (`Neighborhood::Gaussian`, then `FilterKind::GaussianBlur`)
//!    at radius `−s/100 ·` [`SOFTEN_RADIUS_AT_FULL`].
//!
//! Every radius is fixed in target pixels: detail is a property of the pixel
//! grid, not a fraction of the frame. Samples are straight, in the caller's
//! `SampleRange`: `Unit` at 8 and 16 bits (the Develop Filter masks refuse a
//! 32-bit float layer before reaching it); `Extended` for a RAW local on a
//! float development (`raw_local_tone_float`), which runs Noise Reduction
//! and positive Sharpness on the display-encoded copy and negative
//! Sharpness on the linear values (rule L), each a stage of its own.
use crate::filter_kernels::{
    evaluated_in, neighborhood_plan_in, neighborhood_plane_in, FilterKind, Neighborhood,
    SampleRange,
};
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::precision_sampling::visible_range;
use crate::sampling_denoise::halves;
use crate::sampling_smart_sharpen::{SharpenRemove, SmartSharpen, TonalFade, NOISE_LEVEL_AT_FULL};

/// Sharpening's Smart Sharpen radius in pixels (Gaussian removal): one pixel,
/// so positive Sharpness crisps pixel-level detail without the wide halos a
/// larger radius draws round every edge.
pub(crate) const SHARPEN_RADIUS: f64 = 1.0;
/// Smart Sharpen's amount in percent at Sharpness +100: 150 is a
/// strong-but-usable end, so the slider's top is a firm sharpen that a
/// mask's coverage then fades out.
pub(crate) const SHARPEN_AMOUNT_AT_FULL: f64 = 150.0;
/// Gaussian Blur's radius in pixels at Sharpness −100: three pixels softens
/// skin pore and fabric texture clearly while leaving an edge wider than a
/// few pixels readable, the way a negative Sharpness local is used.
pub(crate) const SOFTEN_RADIUS_AT_FULL: f64 = 3.0;
/// Reduce Noise's radius: Smart Sharpen's own Reduce Noise window (σ 0.5,
/// the 5×5 box that holds pixel-scale grain and no edge structure wider
/// than it), so the two routes to luminance noise agree.
pub(crate) const NOISE_RADIUS: f64 = 0.5;

/// One local's detail controls: Sharpness −100…100 and Noise Reduction
/// 0…100, both 0 by default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LocalDetail {
    pub sharpness: f64,
    pub noise_reduction: f64,
}

impl LocalDetail {
    /// Sharpness −100…100 and Noise Reduction 0…100, both finite.
    pub(crate) fn validate(&self) -> Result<(), String> {
        let within = |v: f64, low: f64, high: f64| v.is_finite() && (low..=high).contains(&v);
        if !within(self.sharpness, -100.0, 100.0) || !within(self.noise_reduction, 0.0, 100.0) {
            return Err(
                "invalid local detail: sharpness is -100 to 100, noise reduction 0 to 100".into(),
            );
        }
        Ok(())
    }

    /// Both controls at 0: the stage changes nothing.
    pub(crate) fn identity(&self) -> bool {
        self.sharpness == 0.0 && self.noise_reduction == 0.0
    }

    /// Reduce Noise's neighbourhood and combine (module doc, 1), or None at 0.
    fn denoise(&self) -> Option<(Neighborhood, FilterKind)> {
        (self.noise_reduction > 0.0).then(|| {
            let luminance = self.noise_reduction / 100.0 * NOISE_LEVEL_AT_FULL;
            let (luma, chroma) = halves(luminance, 0.0, 0.0);
            (
                Neighborhood::Denoise {
                    sigma: NOISE_RADIUS,
                    luminance,
                    luma,
                    chroma,
                },
                FilterKind::ReduceNoise {
                    luminance,
                    detail: 0.0,
                    color: 0.0,
                },
            )
        })
    }

    /// Positive Sharpness as Smart Sharpen (module doc, 2), or None.
    fn sharpen(&self) -> Option<SmartSharpen> {
        let off = TonalFade {
            fade: 0.0,
            tonal_width: 0.0,
            radius: 0,
        };
        (self.sharpness > 0.0).then_some(SmartSharpen {
            amount: self.sharpness / 100.0 * SHARPEN_AMOUNT_AT_FULL,
            radius: SHARPEN_RADIUS,
            noise: 0.0,
            remove: SharpenRemove::Gaussian,
            angle: 0.0,
            shadows: off,
            highlights: off,
        })
    }

    /// Negative Sharpness as Gaussian Blur's neighbourhood, or None.
    fn soften(&self) -> Option<Neighborhood> {
        (self.sharpness < 0.0).then(|| Neighborhood::Gaussian {
            sigma: -self.sharpness / 100.0 * SOFTEN_RADIUS_AT_FULL,
        })
    }

    /// `(scratch, work)`, refused before any allocation when the summed work
    /// is above `cap`: the Reduce Noise plan, then the sharpen or soften plan
    /// beside the n×16 denoised plane it reads, the larger of the two, plus
    /// two bookkeeping allowances. Each kernel's plan holds its own output
    /// plane and margin, in `range` (only the Gaussian reads it: a float
    /// plane's is the FIR at every σ). `(0, 0)` for the identity.
    pub(crate) fn plan(
        &self,
        dimensions: (u32, u32),
        cap: u64,
        range: &SampleRange,
    ) -> Result<(u64, u64), String> {
        if self.identity() {
            return Ok((0, 0));
        }
        let n = pixel_count(dimensions.0, dimensions.1)? as u64;
        let (mut peak, mut work, mut kept) = (0_u64, 0_u64, 0_u64);
        if let Some((neighborhood, _)) = self.denoise() {
            let (scratch, pass) = neighborhood_plan_in(neighborhood, dimensions, range, u64::MAX)?;
            peak = scratch;
            work = work.saturating_add(pass);
            kept = n.saturating_mul(16);
        }
        let second = match (self.sharpen(), self.soften()) {
            (Some(sharpen), _) => Some(sharpen.plan_in(dimensions, u64::MAX, range)?),
            (None, Some(soften)) => {
                Some(neighborhood_plan_in(soften, dimensions, range, u64::MAX)?)
            }
            (None, None) => None,
        };
        if let Some((scratch, pass)) = second {
            peak = peak.max(kept.saturating_add(scratch));
            work = work.saturating_add(pass);
        }
        if work > cap {
            return Err("local detail exceeds the bounded pixel-work limit".into());
        }
        Ok((peak.saturating_add(2 * BOOKKEEPING_BYTES), work))
    }

    /// The detailed straight samples of `read` in `range` (module doc):
    /// Reduce Noise, then the sharpen or soften over its result. The identity
    /// returns the samples as read.
    pub(crate) fn plane_in(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        range: &SampleRange,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plan(dimensions, cap, range)?;
        let n = pixel_count(dimensions.0, dimensions.1)?;
        let denoise = self.denoise();
        let split = if denoise.is_some() && self.sharpness != 0.0 {
            0.5
        } else if denoise.is_some() {
            1.0
        } else {
            0.0
        };
        let denoised = match denoise {
            Some((neighborhood, kind)) => {
                let part = job.range(0.0, split);
                let mut plane = neighborhood_plane_in(
                    neighborhood,
                    dimensions,
                    cap,
                    range,
                    &read,
                    part.range(0.0, 0.9),
                )?;
                let combine = part.range(0.9, 1.0);
                for (i, out) in plane.iter_mut().enumerate() {
                    combine.every(i)?;
                    *out = evaluated_in(kind, read(i), *out, range);
                }
                Some(plane)
            }
            None => None,
        };
        let source = |i: usize| match &denoised {
            Some(plane) => plane[i],
            None => read(i),
        };
        let job = job.range(split, 1.0);
        // At `Extended` the sharpen or soften reads the denoised plane, so
        // its visible range is that plane's (the range a float filter on the
        // denoised layer would measure), not the caller's: Reduce Noise can
        // move a component past what `read` held, and the Unsharp combine
        // clamps its low-pass to the range it is given.
        let range = match (range, &denoised) {
            (SampleRange::Extended { .. }, Some(plane)) => SampleRange::Extended {
                visible: visible_range(plane.iter().copied(), job.checkpoints_only())?,
            },
            _ => *range,
        };
        let range = &range;
        if let Some(sharpen) = self.sharpen() {
            return sharpen.plane_in(source, dimensions, cap, range, job);
        }
        if let Some(soften) = self.soften() {
            let mut plane =
                neighborhood_plane_in(soften, dimensions, cap, range, source, job.range(0.0, 0.9))?;
            let combine = job.range(0.9, 1.0);
            for (i, out) in plane.iter_mut().enumerate() {
                combine.every(i)?;
                *out = evaluated_in(FilterKind::GaussianBlur, source(i), *out, range);
            }
            return Ok(plane);
        }
        match denoised {
            Some(plane) => Ok(plane),
            None => {
                let mut out = reserved_vec(n)?;
                for i in 0..n {
                    job.every(i)?;
                    out.push(read(i));
                }
                Ok(out)
            }
        }
    }
}
