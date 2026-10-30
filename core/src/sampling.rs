//! Shared encoded-color, premultiplied-f32 sampling for transforms, resize,
//! and Gaussian blur. Byte, typed UNORM and extended f32 storage share the
//! walks. Alpha is coverage, not a fourth independent color channel; hidden
//! RGB must never enter a neighboring visible sample. No transfer-curve
//! conversion occurs.
//!
//! Resize keeps its established edge normalization and widens support for
//! reduction; transforms keep transparent out-of-bounds taps. Both use the
//! same interpolation kernels and unpremultiply before clamping, including
//! negative-lobe overshoot. Intermediate passes never quantize or clamp.
//!
//! The extended f32 domain (32-bit float documents) has no storage
//! boundary to clamp at, so its adapter supplies the two bounds UNORM gets
//! for free: an accumulated alpha below [`EXTENDED_ALPHA_FLOOR`] (2^-24) has
//! no surviving color, and RGB is clamped to the layer's own visible range (`ColorRange`) so a
//! negative lobe's ringing never leaves it. That clamp is a REPAIR, the one
//! exception to 11F's "edit output refuses, never repairs" rule; it is named
//! in `float_document`'s module doc.

use crate::job_control::JobContext;
use image::imageops::{self, FilterType};
use image::{GenericImageView, Rgba, RgbaImage};

#[path = "sampling_walk.rs"]
mod walk;
pub(crate) use walk::{map_into, resample_into, resample_rows_into, resize_into, warp_into};

/// Storage specialization only: all weighted sampling keeps the same f32
/// arithmetic. Byte callers keep their historical 0..255 accumulator units.
pub(crate) trait Unorm: Copy + Default + PartialEq {
    const MAX: f32;
    fn raw(self) -> f32;
    fn quantize(value: f32) -> Self;
}
impl Unorm for u8 {
    const MAX: f32 = 255.0;
    fn raw(self) -> f32 {
        f32::from(self)
    }
    fn quantize(value: f32) -> Self {
        quantize_coverage(value)
    }
}
impl Unorm for u16 {
    const MAX: f32 = 65535.0;
    fn raw(self) -> f32 {
        f32::from(self)
    }
    fn quantize(value: f32) -> Self {
        value.clamp(0.0, 65535.0).round() as u16
    }
}

/// Widest fixed transform support: Lanczos3 takes six taps per axis.
pub(crate) const MAX_TAPS: usize = 6;

/// Quantizes alpha/coverage identically for RGBA and single-channel masks.
pub(crate) fn quantize_coverage(acc: f32) -> u8 {
    acc.clamp(0.0, 255.0).round() as u8
}

/// Adds a weighted straight pixel in premultiplied space. The multiplication
/// order preserves the existing transform arithmetic exactly.
pub(crate) fn accumulate(acc: &mut [f32; 4], src: &[u8], weight: f32) {
    accumulate_unorm(acc, src, weight);
}

pub(crate) fn accumulate_unorm<T: Unorm>(acc: &mut [f32; 4], src: &[T], weight: f32) {
    accumulate_planes::<T, 4>(acc, src, weight);
}

/// [`accumulate_unorm`] over any number of planes, alpha LAST (`C − 1`):
/// every colour plane is premultiplied by that alpha with the same
/// operations in the same order, so C = 4 is RGBA's arithmetic bit for bit
/// and C = 5 is a CMYK document's straight CMYKA16 ink (#547 phase 4b).
pub(crate) fn accumulate_planes<T: Unorm, const C: usize>(
    acc: &mut [f32; C],
    src: &[T],
    weight: f32,
) {
    let a = src[C - 1].raw();
    let pm = weight * a * (1.0 / T::MAX);
    for c in 0..C - 1 {
        acc[c] += pm * src[c].raw();
    }
    acc[C - 1] += weight * a;
}

/// Returns straight RGBA8, dividing by RAW alpha before clamping. Dividing
/// by the alpha byte would shift color wherever a negative-lobe kernel
/// overshoots. Sub-byte alpha that rounds to zero has no surviving color.
pub(crate) fn straight(acc: [f32; 4]) -> [u8; 4] {
    straight_unorm(acc)
}

pub(crate) fn straight_unorm<T: Unorm>(acc: [f32; 4]) -> [T; 4] {
    straight_planes::<T, 4>(acc)
}

/// [`straight_unorm`] over any number of planes, alpha LAST: each colour
/// plane divides by the RAW accumulated alpha before clamping, as RGBA does
/// (C = 4 is that arithmetic bit for bit; C = 5 is CMYKA16, #547 phase 4b).
pub(crate) fn straight_planes<T: Unorm, const C: usize>(acc: [f32; C]) -> [T; C] {
    let alpha = T::quantize(acc[C - 1]);
    if alpha == T::default() {
        return [T::default(); C];
    }
    let scale = T::MAX / acc[C - 1];
    let mut out = [alpha; C];
    for c in 0..C - 1 {
        out[c] = T::quantize(acc[c] * scale);
    }
    out
}

/// Per-channel RGB bounds of a float layer's visible pixels (alpha > 0).
/// Filtered float output is clamped into it so ringing never leaves the
/// source's own range; see the module doc.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ColorRange {
    pub(crate) min: [f32; 3],
    pub(crate) max: [f32; 3],
}

/// The extended f32 twin of [`accumulate_unorm`]: the same premultiplied
/// accumulation over straight coverage in [0, 1], with no storage scale.
pub(crate) fn accumulate_extended(acc: &mut [f32; 4], src: &[f32; 4], weight: f32) {
    let pm = weight * src[3];
    for c in 0..3 {
        acc[c] += pm * src[c];
    }
    acc[3] += weight * src[3];
}

/// D2: an extended f32 geometry output alpha below 2^-24 becomes fully
/// transparent (alpha 0, RGB 0). 2^-24 is below one f32 ulp at 1.0 and far
/// below a 16-bit code (2^-16): the light dropped per pixel is at most
/// 2^-24 × the layer's largest RGB, invisible for ordinary values and a few
/// display codes at an extreme HDR edge (≈3.9e-3 linear at 65504); and the
/// surviving alpha × any mask coverage (≥ 2^-16) × any UI opacity stays far
/// above the renderer's 2^-96 `MIN_RENDER_ALPHA` refusal floor, which a
/// lower output floor let Lanczos tails fall under (the renderer refuses an
/// effective alpha below it, so the whole document stopped rendering).
pub(crate) const EXTENDED_ALPHA_FLOOR: f32 = 1.0 / 16_777_216.0;

/// The extended f32 twin of [`straight_unorm`], the master copy: RGB divides
/// by the RAW accumulated alpha before clamping. Where UNORM rounds sub-code
/// alpha to zero, f32 drops alpha below [`EXTENDED_ALPHA_FLOOR`] (and a NaN)
/// to transparent; RGB is clamped to `range` rather than to a code range,
/// and overshooting alpha is clamped to full coverage.
pub(crate) fn straight_extended(acc: [f32; 4], range: &ColorRange) -> [f32; 4] {
    if acc[3].is_nan() || acc[3] < EXTENDED_ALPHA_FLOOR {
        return [0.0; 4];
    }
    [
        (acc[0] / acc[3]).clamp(range.min[0], range.max[0]),
        (acc[1] / acc[3]).clamp(range.min[1], range.max[1]),
        (acc[2] / acc[3]).clamp(range.min[2], range.max[2]),
        acc[3].min(1.0),
    ]
}

/// The shared separable interpolation kernels. Transforms normalize their
/// full support; resize normalizes the source-clipped, scale-aware support.
#[derive(Clone, Copy)]
pub(crate) enum Kernel {
    /// Triangle filter, 2x2 taps.
    Bilinear,
    /// Catmull-Rom bicubic, 4x4 taps.
    Bicubic,
    /// Lanczos, 6x6 taps.
    Lanczos3,
    /// Gaussian resampling (not exposed by the transform FFI).
    Gaussian,
}

// Both arithmetic specializations expand the same polynomial/window formula.
macro_rules! kernel_weight {
    ($kind:expr, $t:expr, $pi:expr) => {{
        let x = ($t).abs();
        match $kind {
            Kernel::Bilinear => (1.0 - x).max(0.0),
            Kernel::Bicubic => {
                // Catmull-Rom (Mitchell with B = 0, C = 0.5): interpolating,
                // so an exact hit reproduces the source sample.
                if x < 1.0 {
                    ((1.5 * x - 2.5) * x) * x + 1.0
                } else if x < 2.0 {
                    ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
                } else {
                    0.0
                }
            }
            Kernel::Gaussian => (-2.0 * x * x).exp(),
            Kernel::Lanczos3 => {
                // sinc(x) * sinc(x/3), written to avoid 0/0 at the centre.
                if x < 1e-6 {
                    1.0
                } else if x < 3.0 {
                    let px = $pi * x;
                    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
                } else {
                    0.0
                }
            }
        }
    }};
}

impl Kernel {
    /// Half-width of the support, in source pixels.
    pub(crate) fn radius(self) -> i64 {
        match self {
            Kernel::Bilinear => 1,
            Kernel::Bicubic => 2,
            Kernel::Lanczos3 | Kernel::Gaussian => 3,
        }
    }

    pub(crate) fn taps(self) -> usize {
        2 * self.radius() as usize
    }

    pub(crate) fn weight(self, t: f32) -> f32 {
        kernel_weight!(self, t, std::f32::consts::PI)
    }
    fn weight_precise(self, t: f64) -> f64 {
        kernel_weight!(self, t, std::f64::consts::PI)
    }

    /// Fills `out[..taps]` with the normalized tap weights for the continuous
    /// source coordinate `s` (pixel `k` covers `[k, k+1)`), returning the
    /// index of the first tap.
    pub(crate) fn weights(self, s: f64, out: &mut [f32; MAX_TAPS]) -> i64 {
        self.normalized_weights(s, out, |distance| self.weight(distance as f32))
    }
    pub(crate) fn weights_precise(self, s: f64, out: &mut [f32; MAX_TAPS]) -> i64 {
        let mut precise = [0.0_f64; MAX_TAPS];
        let first =
            self.normalized_weights(s, &mut precise, |distance| self.weight_precise(distance));
        for (output, value) in out.iter_mut().zip(precise) {
            *output = value as f32;
        }
        first
    }
    /// Binary64 tap normalization for a retained warp's final resample.
    pub(crate) fn weights_f64(self, s: f64, out: &mut [f64; MAX_TAPS]) -> i64 {
        self.normalized_weights(s, out, |distance| self.weight_precise(distance))
    }
    fn normalized_weights<N>(
        self,
        s: f64,
        out: &mut [N; MAX_TAPS],
        evaluate: impl Fn(f64) -> N,
    ) -> i64
    where
        N: Copy + Default + PartialEq + From<u8> + std::ops::AddAssign + std::ops::DivAssign,
    {
        // Index space: integers are pixel CENTRES.
        let p = s - 0.5;
        let base = p.floor();
        let lead = self.radius() - 1;
        let mut sum = N::default();
        for (t, w) in out.iter_mut().enumerate().take(self.taps()) {
            *w = evaluate(p - (base + t as f64 - lead as f64));
            sum += *w;
        }
        if sum != N::default() && sum != N::from(1) {
            for w in out.iter_mut().take(self.taps()) {
                *w /= sum;
            }
        }
        // `as` saturates, so an absurd coordinate simply lands out of bounds.
        (base as i64).saturating_sub(lead)
    }
}

/// Source-clipped, normalized resize weights. Unlike transform taps the
/// support widens on reduction to anti-alias details smaller than a pixel.
/// Clipping before normalization preserves resize's established border rule.
pub(super) fn resize_weights(
    kernel: Kernel,
    source: u32,
    target: u32,
    index: u32,
    weights: &mut Vec<f32>,
    job: crate::job_control::JobContext<'_>,
) -> Result<u32, String> {
    let ratio = source as f32 / target as f32;
    let scale = ratio.max(1.0);
    let center = (index as f32 + 0.5) * ratio;
    let support = kernel.radius() as f32 * scale;
    let first = ((center - support).floor() as i64).clamp(0, i64::from(source) - 1) as u32;
    let end =
        ((center + support).ceil() as i64).clamp(i64::from(first) + 1, i64::from(source)) as u32;
    weights.clear();
    let mut sum = 0.0;
    for i in first..end {
        job.every(i as usize)?;
        let weight = kernel.weight((i as f32 - (center - 0.5)) / scale);
        weights.push(weight);
        sum += weight;
    }
    for weight in weights {
        *weight /= sum;
    }
    Ok(first)
}

/// Resize independent floating-point fields using the same normalized triangle
/// weights as RGBA resize. Unlike `resize`, fields have no premultiplication or
/// byte boundary; matting pyramids contain RGB estimates and scalar coverage.
/// Caller validates dimensions and reserves source/intermediate/output storage.
pub(crate) fn resize_fields<const C: usize>(
    source: &[[f32; C]],
    source_size: (u32, u32),
    target_size: (u32, u32),
    job: crate::job_control::JobContext<'_>,
) -> Result<Vec<[f32; C]>, String> {
    let mut out = vec![[0.0; C]; target_size.0 as usize * target_size.1 as usize];
    resize_into(
        source_size,
        target_size,
        Kernel::Bilinear,
        |i| source[i],
        |acc, sample, weight| {
            for c in 0..C {
                acc[c] += sample[c] * weight;
            }
        },
        |sample| sample,
        |i, sample| out[i] = sample,
        false,
        job,
    )?;
    Ok(out)
}

/// Resizes straight RGBA8. Caller validates output dimensions/budgets.
/// Nearest and identity resize copy bytes, including latent transparent RGB.
/// Opaque images retain their established image-crate output byte-for-byte.
pub(crate) fn resize(src: &RgbaImage, w: u32, h: u32, filter: FilterType) -> RgbaImage {
    if matches!(filter, FilterType::Nearest)
        || src.dimensions() == (w, h)
        || src.pixels().all(|p| p[3] == 255)
    {
        return imageops::resize(src, w, h, filter);
    }
    let kernel = match filter {
        FilterType::Nearest => unreachable!(),
        FilterType::Triangle => Kernel::Bilinear,
        FilterType::CatmullRom => Kernel::Bicubic,
        FilterType::Gaussian => Kernel::Gaussian,
        FilterType::Lanczos3 => Kernel::Lanczos3,
    };
    let mut out = RgbaImage::new(w, h);
    resize_into(
        src.dimensions(),
        (w, h),
        kernel,
        |i| {
            src.get_pixel(i as u32 % src.width(), i as u32 / src.width())
                .0
        },
        |acc, sample, weight| accumulate(acc, &sample, weight),
        straight,
        |i, sample| out.put_pixel(i as u32 % w, i as u32 / w, Rgba(sample)),
        false,
        crate::job_control::JobContext::NONE,
    )
    .expect("uncancellable resize");
    out
}

/// Reads premultiplied float pixels of rows `top..top + rows` without
/// allocating a source copy. The Gaussian implementation materializes its own
/// intermediate buffers.
struct PremultipliedView<'a> {
    image: &'a RgbaImage,
    top: u32,
    rows: u32,
}

impl GenericImageView for PremultipliedView<'_> {
    type Pixel = Rgba<f32>;

    fn dimensions(&self) -> (u32, u32) {
        (self.image.width(), self.rows)
    }

    fn get_pixel(&self, x: u32, y: u32) -> Self::Pixel {
        let mut acc = [0.0; 4];
        accumulate(&mut acc, &self.image.get_pixel(x, self.top + y).0, 1.0);
        Rgba(acc)
    }
}

/// Gaussian blur under the caller's `job`, one `BAND_ROWS` band at a time at
/// every σ, so a cancel is answered within a band and the scratch is one
/// band's ([`blur_scratch_bytes`]). `Err` is a cancel or a refused allocation.
/// Its callers ([`fitted_blur`], which delegates below
/// `gaussian_blur_filter::FITTED_MIN_RADIUS`, and `ops::blur`) refuse σ above
/// `sampling_gaussian_fast::MAX_SIGMA` (1000), so no σ is run as a smaller one.
///
/// From `LEGACY_FAST_SIGMA` up, opaque or not, it is the radius-independent
/// `sampling_gaussian_fast` over unit-scale premultiplied
/// samples, each band converted back through [`straight`]. `band` reports
/// 0..1 over its own rows, so each tile is given its own share of the job's
/// range and this loop reports nothing itself.
///
/// Below the switch it reuses the image crate's kernel, edge extension and
/// float convolution; only the input/output alpha boundary changes. That float
/// blur does not clamp accumulated channels. Opaque input keeps the prior
/// byte output and needs no alpha conversion; which of the two runs is
/// decided once, for the whole image. Each band is the kernel run over a row
/// view of the band plus `selected_filter::legacy_halo` rows either side
/// (clamped to the image), which exceeds the kernel's half-width, so the kept
/// rows are the whole-image kernel's rows byte for byte
/// (`tests/byte_gaussian_blur_op.rs`).
pub(crate) fn blur(src: &RgbaImage, sigma: f32, job: JobContext<'_>) -> Result<RgbaImage, String> {
    use crate::selected_filter::{legacy_halo, BAND_ROWS};
    let (w, h) = src.dimensions();
    let width = w as usize;
    let mut out: Vec<u8> = crate::precision_image::reserved_vec(src.as_raw().len())?;
    out.resize(src.as_raw().len(), 0);
    let total = f64::from(h.max(1));
    if w == 0 || h == 0 {
        // Nothing to filter; the image crate answers the same empty image.
    } else if sigma >= crate::sampling_gaussian_fast::LEGACY_FAST_SIGMA {
        let raw = src.as_raw();
        let read = |i: usize| {
            let mut acc = [0.0; 4];
            accumulate(&mut acc, &raw[i * 4..i * 4 + 4], 1.0);
            acc.map(|v| v / 255.0)
        };
        for start in (0..h).step_by(BAND_ROWS as usize) {
            let end = (start + BAND_ROWS).min(h);
            let low = crate::sampling_gaussian_fast::band(
                (w, h),
                f64::from(sigma),
                start..end,
                read,
                job.range(f64::from(start) / total, f64::from(end) / total),
            )?;
            let base = start as usize * width * 4;
            for (i, p) in low.iter().enumerate() {
                out[base + i * 4..base + i * 4 + 4]
                    .copy_from_slice(&straight(p.map(|v| v * 255.0)));
            }
        }
    } else {
        let opaque = src.pixels().all(|p| p[3] == 255);
        let halo = legacy_halo(sigma);
        for start in (0..h).step_by(BAND_ROWS as usize) {
            job.check()?;
            let end = (start + BAND_ROWS).min(h);
            let top = start.saturating_sub(halo);
            let bottom = end.saturating_add(halo).min(h);
            let skip = (start - top) as usize * width * 4;
            let kept = &mut out[start as usize * width * 4..end as usize * width * 4];
            if opaque {
                let low = imageops::blur(&*imageops::crop_imm(src, 0, top, w, bottom - top), sigma);
                kept.copy_from_slice(&low.as_raw()[skip..skip + kept.len()]);
            } else {
                let view = PremultipliedView {
                    image: src,
                    top,
                    rows: bottom - top,
                };
                let low = imageops::blur(&view, sigma);
                let rows = &low.as_raw()[skip..skip + kept.len()];
                for (o, p) in kept.chunks_exact_mut(4).zip(rows.chunks_exact(4)) {
                    o.copy_from_slice(&straight([p[0], p[1], p[2], p[3]]));
                }
            }
            job.report(f64::from(end) / total, 6)?;
        }
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| "invalid blurred image dimensions".into())
}

/// [`blur`]'s planned peak scratch over an image of `dimensions`: the n×4
/// output plus one band. From `LEGACY_FAST_SIGMA` up the band is
/// `sampling_gaussian_fast::plan` for min(`BAND_ROWS`, h) rows (its own
/// formula's one home). Below it the image crate's kernel holds two f32
/// planes of the band's window (n×16 each) beside its own output (n×4 opaque,
/// n×16 premultiplied), 48 B per window pixel at most, over
/// min(h, `BAND_ROWS` + 2·`legacy_halo`) rows, plus two bookkeeping
/// allowances. Err for a σ outside (0, `MAX_SIGMA`] or an overflow.
///
/// Record (Release core, one case per process, `MallocLargeCache=0`, E =
/// `ri_interval_max_phys_footprint` − pre, 3 runs + a baseline twin at E 0):
/// uncontrolled `dr_image_filter_op` `blur` on 6000×4000 splitmix noise. The
/// plan is `ops_spatial::filter_op_scratch_bytes`: this figure + 4 BK, plus a
/// quarter.
/// - σ25 opaque (the former recursive filter; σ25 is now the fitted
///   kernel, whose record is on `gaussian_blur_filter::plan`): E 140,738,656
///   ×3; plan 188,916,480 (1.342×).
/// - σ2 opaque: E 128,974,920 / 128,974,920 / 128,991,304; plan 182,584,320
///   (1.415×).
/// - σ2 with partial alpha (a quarter of the pixels alpha 0, a quarter
///   partial; the image crate's float path): E 139,935,840 / 139,952,224 /
///   139,935,840; plan 182,584,320 (1.305×).
pub(crate) fn blur_scratch_bytes(dimensions: (u32, u32), sigma: f32) -> Result<u64, String> {
    use crate::precision_image::BOOKKEEPING_BYTES;
    use crate::selected_filter::{legacy_halo, BAND_ROWS};
    let overflow = || "Gaussian blur plan overflow".to_string();
    let (w, h) = dimensions;
    let output = u64::from(w)
        .checked_mul(u64::from(h))
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(overflow)?;
    let band = if sigma >= crate::sampling_gaussian_fast::LEGACY_FAST_SIGMA {
        crate::sampling_gaussian_fast::plan(dimensions, f64::from(sigma), BAND_ROWS.min(h))?.0
    } else {
        if !sigma.is_finite() || sigma <= 0.0 {
            return Err("Gaussian sigma must be finite, positive and at most 1000".into());
        }
        let window = h.min(BAND_ROWS.saturating_add(legacy_halo(sigma).saturating_mul(2)));
        u64::from(w)
            .checked_mul(u64::from(window))
            .and_then(|pixels| pixels.checked_mul(48))
            .and_then(|bytes| bytes.checked_add(2 * BOOKKEEPING_BYTES))
            .ok_or_else(overflow)?
    };
    output.checked_add(band).ok_or_else(overflow)
}

/// Filter ▸ Blur ▸ Gaussian Blur at 8 bits: the fitted kernel
/// (`gaussian_blur_filter`, the master) at Radius `radius` over unit-scale
/// premultiplied samples, one `BAND_ROWS` band at a time under `job`, each
/// band converted back through [`straight`]. A radius the law does not cover
/// (`!gaussian_blur_filter::runs`) is [`blur`]. `Err` is a cancel or a
/// refused allocation; the caller refuses a radius above
/// `sampling_gaussian_fast::MAX_SIGMA`.
pub(crate) fn fitted_blur(
    src: &RgbaImage,
    radius: f32,
    job: JobContext<'_>,
) -> Result<RgbaImage, String> {
    use crate::gaussian_blur_filter as filter;
    use crate::selected_filter::BAND_ROWS;
    if !filter::runs(f64::from(radius)) {
        return blur(src, radius, job);
    }
    let (w, h) = src.dimensions();
    let width = w as usize;
    let mut out: Vec<u8> = crate::precision_image::reserved_vec(src.as_raw().len())?;
    out.resize(src.as_raw().len(), 0);
    let total = f64::from(h.max(1));
    let raw = src.as_raw();
    let read = |i: usize| {
        let mut acc = [0.0; 4];
        accumulate(&mut acc, &raw[i * 4..i * 4 + 4], 1.0);
        acc.map(|v| v / 255.0)
    };
    if w > 0 && h > 0 {
        for start in (0..h).step_by(BAND_ROWS as usize) {
            let end = (start + BAND_ROWS).min(h);
            let low = filter::band(
                (w, h),
                f64::from(radius),
                start..end,
                &crate::filter_kernels::SampleRange::Unit,
                read,
                job.range(f64::from(start) / total, f64::from(end) / total),
            )?;
            let base = start as usize * width * 4;
            for (i, p) in low.iter().enumerate() {
                out[base + i * 4..base + i * 4 + 4]
                    .copy_from_slice(&straight(p.map(|v| v * 255.0)));
            }
        }
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| "invalid blurred image dimensions".into())
}

/// [`fitted_blur`]'s planned peak scratch: the n×4 output plus
/// `gaussian_blur_filter::plan` for one band of min(`BAND_ROWS`, h) rows, or
/// [`blur_scratch_bytes`] where [`fitted_blur`] runs [`blur`]. Err for a
/// radius outside (0, `MAX_SIGMA`] or an overflow.
pub(crate) fn fitted_blur_scratch_bytes(
    dimensions: (u32, u32),
    radius: f32,
) -> Result<u64, String> {
    use crate::gaussian_blur_filter as filter;
    use crate::selected_filter::BAND_ROWS;
    if !filter::runs(f64::from(radius)) {
        return blur_scratch_bytes(dimensions, radius);
    }
    let (w, h) = dimensions;
    let output = u64::from(w)
        .checked_mul(u64::from(h))
        .and_then(|n| n.checked_mul(4))
        .ok_or("Gaussian blur plan overflow")?;
    let band = filter::plan(dimensions, f64::from(radius), BAND_ROWS.min(h))?.0;
    output
        .checked_add(band)
        .ok_or_else(|| "Gaussian blur plan overflow".into())
}
