//! Additional pure filters: hue rotate, per-channel levels (the Auto
//! commands'), threshold, posterize, pixelate, noise, edge detect, emboss.
//! See include/doctored_core.h. The Levels dialog, `apply_filter` "levels"
//! and the levels adjustment layer run `adjust_levels` instead.
//!
//! Every function returns a new image and never mutates its input; invalid
//! arguments yield `None` (surfaced as NULL through the FFI). Color math is
//! f32 per channel, rounded and clamped back to u8; alpha passes through
//! untouched unless a filter's contract says otherwise.
//!
//! The selected-edit planes of Emboss / Edge Detect ([`relief_plane`]) and
//! Pixelate ([`pixelate_plane_f32`]), and Add Noise's float sample
//! ([`noise_relative`]), live here too; each states its 32-bit rule.

use image::{Rgba, RgbaImage};

use crate::adjust_levels::LevelsStage;
use crate::adjust_math;
use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::rng::SplitMix64;
use crate::sampling::EXTENDED_ALPHA_FLOOR;

/// The hueRotate matrix from the SVG filter effects specification, built
/// from cos/sin of `degrees` (no HSL round-trip). Shared with the
/// `hue_rotate` adjustment layer (`adjust`) so both produce the same colors.
pub(crate) fn hue_rotate_matrix(degrees: f32) -> [[f32; 3]; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    [
        [
            0.213 + c * 0.787 - s * 0.213,
            0.715 - c * 0.715 - s * 0.715,
            0.072 - c * 0.072 + s * 0.928,
        ],
        [
            0.213 - c * 0.213 + s * 0.143,
            0.715 + c * 0.285 + s * 0.140,
            0.072 - c * 0.072 - s * 0.283,
        ],
        [
            0.213 - c * 0.213 - s * 0.787,
            0.715 - c * 0.715 + s * 0.715,
            0.072 + c * 0.928 + s * 0.072,
        ],
    ]
}

/// Rotates hue by `degrees` using [`hue_rotate_matrix`]. Any finite angle
/// is accepted; returns `None` for NaN or infinite values. Alpha untouched.
pub(crate) fn hue_rotate(img: &RgbaImage, degrees: f32) -> Option<RgbaImage> {
    if !degrees.is_finite() {
        return None;
    }
    let m = hue_rotate_matrix(degrees);
    let mut out = img.clone();
    for px in out.pixels_mut() {
        let r = f32::from(px[0]) / 255.0;
        let g = f32::from(px[1]) / 255.0;
        let b = f32::from(px[2]) / 255.0;
        for (dst, row) in px.0.iter_mut().zip(&m) {
            let v = row[0] * r + row[1] * g + row[2] * b;
            *dst = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    Some(out)
}

/// Levels with a black point, white point and gamma PER channel (R, G, B):
/// each channel runs the input half of one `adjust_levels` stage
/// (`LevelsStage::input`) in its 8-bit form (`LevelsStage::apply8`: the
/// stage curve, `t^(1/gamma)` and its toe, on codes), so the Auto commands
/// that run it and a master-only levels adjustment with the triple repeated
/// produce the same bytes. `None` unless EVERY channel has
/// `0 <= black < white <= 1` and gamma in [0.1, 10] (NaN fails every
/// comparison). Alpha untouched.
pub(crate) fn levels_channels(
    img: &RgbaImage,
    black: [f32; 3],
    white: [f32; 3],
    gamma: [f32; 3],
) -> Option<RgbaImage> {
    let stages = [
        LevelsStage::input(black[0], white[0], gamma[0])?,
        LevelsStage::input(black[1], white[1], gamma[1])?,
        LevelsStage::input(black[2], white[2], gamma[2])?,
    ];
    let mut out = img.clone();
    for px in out.pixels_mut() {
        for (ch, stage) in px.0.iter_mut().zip(&stages) {
            *ch = (stage.apply8(f32::from(*ch) / 255.0) * 255.0).round() as u8;
        }
    }
    Some(out)
}

/// Luma threshold: every color channel becomes 0 or 255 depending on whether
/// the pixel's 8-bit luma reaches `level` — the ONE kernel the adjustment
/// layer also runs ([`adjust_math::threshold`]). Requires `level` in
/// [0, 1]; alpha untouched.
pub(crate) fn threshold(img: &RgbaImage, level: f32) -> Option<RgbaImage> {
    if !(0.0..=1.0).contains(&level) {
        return None;
    }
    let mut out = img.clone();
    for px in out.pixels_mut() {
        let rgb = [px[0], px[1], px[2]].map(|c| f32::from(c) / 255.0);
        let v = if adjust_math::threshold(rgb, level, 255) > 0.5 {
            255
        } else {
            0
        };
        px[0] = v;
        px[1] = v;
        px[2] = v;
    }
    Some(out)
}

/// Reduces each color channel to `levels` values through the ONE
/// [`adjust_math::posterize`] (the 8-bit bins measured against Photoshop).
/// Requires `levels` in 2..=64; alpha untouched.
pub(crate) fn posterize(img: &RgbaImage, levels: u32) -> Option<RgbaImage> {
    if !(2..=64).contains(&levels) {
        return None;
    }
    let mut out = img.clone();
    for px in out.pixels_mut() {
        for ch in px.0.iter_mut().take(3) {
            let t = adjust_math::posterize(f32::from(*ch) / 255.0, levels, 255);
            *ch = (t * 255.0).round() as u8;
        }
    }
    Some(out)
}

/// Mosaic of `block` x `block` cells anchored at (0, 0); partial edge cells
/// use their actual extent. Each cell becomes its ALPHA-WEIGHTED mean color
/// (`sum c*a / sum a`; a cell whose alpha sums to zero becomes fully
/// transparent black) with the plain mean alpha. Requires `block` in
/// 1..=1024; `block == 1` is the identity (still a fresh image).
pub(crate) fn pixelate(img: &RgbaImage, block: u32) -> Option<RgbaImage> {
    if !(1..=1024).contains(&block) {
        return None;
    }
    let mut out = img.clone();
    if block == 1 {
        return Some(out);
    }
    let (w, h) = img.dimensions();
    for cy in (0..h).step_by(block as usize) {
        let cell_h = block.min(h - cy);
        for cx in (0..w).step_by(block as usize) {
            let cell_w = block.min(w - cx);
            // Integer accumulation is exact: c*a <= 65025 summed over at most
            // 1024*1024 pixels fits comfortably in u64.
            let mut sum_ca = [0u64; 3];
            let mut sum_a = 0u64;
            for y in cy..cy + cell_h {
                for x in cx..cx + cell_w {
                    let p = img.get_pixel(x, y);
                    let a = u64::from(p[3]);
                    sum_a += a;
                    for (s, ch) in sum_ca.iter_mut().zip(p.0) {
                        *s += u64::from(ch) * a;
                    }
                }
            }
            let count = u64::from(cell_w) * u64::from(cell_h);
            let mean = Rgba(pixelate_mean(sum_ca, sum_a, count).map(|v| v.round() as u8));
            for y in cy..cy + cell_h {
                for x in cx..cx + cell_w {
                    out.put_pixel(x, y, mean);
                }
            }
        }
    }
    Some(out)
}

/// Additive uniform noise in [-amount, +amount] (normalized scale) per COLOR
/// channel, independent per channel, deterministic from `seed` (SplitMix64,
/// pixels visited in row-major order, channels r, g, b). Requires `amount`
/// in (0, 1]; alpha untouched.
pub(crate) fn noise(img: &RgbaImage, amount: f32, seed: u64) -> Option<RgbaImage> {
    if !(amount > 0.0 && amount <= 1.0) {
        return None;
    }
    let mut rng = SplitMix64::new(seed);
    let mut out = img.clone();
    for px in out.pixels_mut() {
        for ch in px.0.iter_mut().take(3) {
            let delta = noise_delta(&mut rng, amount);
            let v = f32::from(*ch) / 255.0 + delta;
            *ch = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    Some(out)
}

/// Normalized ([0, 1]) Rec. 709 luma plane with border-replicating sampling,
/// shared by the convolution filters.
struct LumaPlane {
    w: usize,
    h: usize,
    data: Vec<f32>,
}

impl LumaPlane {
    fn new(img: &RgbaImage) -> Self {
        let (w, h) = img.dimensions();
        let data = img
            .pixels()
            .map(|p| {
                (LUMA_R * f32::from(p[0]) + LUMA_G * f32::from(p[1]) + LUMA_B * f32::from(p[2]))
                    / 255.0
            })
            .collect();
        LumaPlane {
            w: w as usize,
            h: h as usize,
            data,
        }
    }

    /// Samples with replicated borders (coordinates clamp to the edges).
    fn at(&self, x: i64, y: i64) -> f32 {
        let x = x.clamp(0, self.w as i64 - 1) as usize;
        let y = y.clamp(0, self.h as i64 - 1) as usize;
        self.data[y * self.w + x]
    }
}

const SOBEL_X: [[f32; 3]; 3] = [[-1.0, 0.0, 1.0], [-2.0, 0.0, 2.0], [-1.0, 0.0, 1.0]];
const SOBEL_Y: [[f32; 3]; 3] = [[-1.0, -2.0, -1.0], [0.0, 0.0, 0.0], [1.0, 2.0, 1.0]];
const EMBOSS: [[f32; 3]; 3] = [[-2.0, -1.0, 0.0], [-1.0, 1.0, 1.0], [0.0, 1.0, 2.0]];

/// Sobel edge magnitude on the Rec. 709 luma plane, `sqrt(gx^2 + gy^2)`
/// clamped to [0, 1], rendered as opaque grayscale (r = g = b, alpha 255).
pub(crate) fn edge_detect(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let luma = LumaPlane::new(img);
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mag = relief_value(false, |dx, dy| {
                luma.at(i64::from(x) + dx, i64::from(y) + dy)
            });
            let v = (mag * 255.0).round() as u8;
            out.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
    out
}

/// Emboss: the 3x3 relief kernel `[[-2,-1,0],[-1,1,1],[0,1,2]]` applied to
/// the Rec. 709 luma plane plus a 0.5 bias, clamped to [0, 1], rendered as
/// opaque grayscale (r = g = b, alpha 255). Borders replicate.
pub(crate) fn emboss(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let luma = LumaPlane::new(img);
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = (relief_value(true, |dx, dy| luma.at(i64::from(x) + dx, i64::from(y) + dy))
                * 255.0)
                .round() as u8;
            out.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
    out
}

/// Shared deterministic delta; every legacy and native RGB sample consumes one.
pub(crate) fn noise_delta(rng: &mut SplitMix64, amount: f32) -> f32 {
    (2.0 * rng.next_unit_f32() - 1.0) * amount
}
/// One 3x3 kernel over caller-owned sampling, summed in row-major order.
fn convolve3(kernel: &[[f32; 3]; 3], at: &impl Fn(i64, i64) -> f32) -> f32 {
    let mut sum = 0.0;
    for (y, row) in kernel.iter().enumerate() {
        for (x, k) in row.iter().enumerate() {
            sum += k * at(x as i64 - 1, y as i64 - 1);
        }
    }
    sum
}

/// The unclamped Sobel gradient `[gx, gy]` at the sample `at(0, 0)`, with
/// caller-owned sampling and edge policy. The one Sobel implementation: Find
/// Edges (`relief_value`) clamps its magnitude, the Magnetic Lasso's edge
/// strength (`edge_trace`) scales it per channel.
pub(crate) fn sobel_gradient(at: impl Fn(i64, i64) -> f32) -> [f32; 2] {
    [convolve3(&SOBEL_X, &at), convolve3(&SOBEL_Y, &at)]
}

/// Shared relief kernels with caller-owned luma sampling and edge policy.
pub(crate) fn relief_value(emboss: bool, at: impl Fn(i64, i64) -> f32) -> f32 {
    relief_value_in(emboss, at, &SampleRange::Unit)
}

/// [`relief_value`] in `range`: clamped to 0…1 at `Unit`, unclamped at
/// `Extended`, where [`relief_plane`] finishes each channel with
/// `SampleRange::display_bounded` instead.
pub(crate) fn relief_value_in(
    emboss: bool,
    at: impl Fn(i64, i64) -> f32,
    range: &SampleRange,
) -> f32 {
    let value = if emboss {
        convolve3(&EMBOSS, &at) + 0.5
    } else {
        let [gx, gy] = sobel_gradient(at);
        (gx * gx + gy * gy).sqrt()
    };
    match range {
        SampleRange::Unit => value.clamp(0.0, 1.0),
        SampleRange::Extended { .. } => value,
    }
}

/// The relief plane (Emboss, or Edge Detect's Sobel magnitude) of the
/// straight samples `read` yields: the Rec. 709 luma of each 3×3
/// neighbourhood (clamped edges), the kernel's value as an opaque grey
/// `[v, v, v, 1]`, each channel finished with `range.display_bounded`. At
/// `Unit` that is the 16-bit plane `selected_filter` has always built.
///
/// Emboss and Edge Detect at 32 bits: the float route builds the plane on
/// the layer's DISPLAY-ENCODED copy (`display_encoding`), so the 0.5 grey
/// Emboss adds and the edge magnitude are display values, as at 16 bits:
/// the kernel's weights sum to 1, so a flat region embosses to its encoded
/// luma plus 0.5, and a flat black one to the display's mid-grey (linear
/// 0.2140 in sRGB), not to linear 0.5. The finish keeps an SDR layer inside 0…1 exactly as at 16 bits; only
/// content already above white or below zero can reach past it.
pub(crate) fn relief_plane(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    emboss: bool,
    range: &SampleRange,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let (w, h) = dimensions;
    let n = w as usize * h as usize;
    let mut out = reserved_vec(n)?;
    for i in 0..n {
        job.every(i)?;
        let x = (i % w as usize) as i64;
        let y = (i / w as usize) as i64;
        let at = |dx: i64, dy: i64| {
            let px = (x + dx).clamp(0, i64::from(w) - 1);
            let py = (y + dy).clamp(0, i64::from(h) - 1);
            let p = read(py as usize * w as usize + px as usize);
            LUMA_R * p[0] + LUMA_G * p[1] + LUMA_B * p[2]
        };
        let value = f64::from(relief_value_in(emboss, at, range));
        let channel = |c: usize| range.display_bounded(c, value) as f32;
        out.push([channel(0), channel(1), channel(2), 1.]);
    }
    Ok(out)
}

/// Exact integer cell sums become straight mean RGB and mean alpha in caller
/// code units. Byte/native outputs choose their own final storage boundary.
pub(crate) fn pixelate_mean(sum_ca: [u64; 3], sum_a: u64, count: u64) -> [f64; 4] {
    pixelate_mean_f64(sum_ca.map(|v| v as f64), sum_a as f64, count as f64)
}

/// The master of [`pixelate_mean`]: binary64 cell sums of `c·a` and `a` over
/// `count` pixels as the straight mean colour and the mean alpha; a cell
/// whose alpha sums to zero is transparent black.
pub(crate) fn pixelate_mean_f64(sum_ca: [f64; 3], sum_a: f64, count: f64) -> [f64; 4] {
    if sum_a == 0.0 {
        return [0.0; 4];
    }
    [
        sum_ca[0] / sum_a,
        sum_ca[1] / sum_a,
        sum_ca[2] / sum_a,
        sum_a / count,
    ]
}

/// Pixelate over straight f32 samples: each `block`-square cell anchored at
/// (0, 0) (partial edge cells their actual extent) becomes its premultiplied
/// mean in binary64 ([`pixelate_mean_f64`]), alpha the plain mean; a mean
/// alpha below 2^-24 (`sampling::EXTENDED_ALPHA_FLOOR`) is transparent black,
/// as every float mean's is.
///
/// Pixelate at 32 bits runs on the STORED linear samples: a cell is the
/// light a coarser sensor would have recorded over it, so a highlight in a
/// cell lifts the whole cell by its share of energy. No colour is clamped.
pub(crate) fn pixelate_plane_f32(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    block: u32,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let (w, h) = dimensions;
    let n = w as usize * h as usize;
    let mut out = reserved_vec(n)?;
    out.resize(n, [0.0_f32; 4]);
    for cy in (0..h).step_by(block as usize) {
        for cx in (0..w).step_by(block as usize) {
            job.check()?;
            let ch = block.min(h - cy);
            let cw = block.min(w - cx);
            let mut sum_ca = [0.0_f64; 3];
            let mut sum_a = 0.0_f64;
            for y in cy..cy + ch {
                for x in cx..cx + cw {
                    let p = read(y as usize * w as usize + x as usize);
                    let a = f64::from(p[3]);
                    sum_a += a;
                    for c in 0..3 {
                        sum_ca[c] += f64::from(p[c]) * a;
                    }
                }
            }
            let mean = pixelate_mean_f64(sum_ca, sum_a, f64::from(cw) * f64::from(ch));
            let sample = if mean[3] < f64::from(EXTENDED_ALPHA_FLOOR) {
                [0.0; 4]
            } else {
                mean.map(|v| v as f32)
            };
            for y in cy..cy + ch {
                for x in cx..cx + cw {
                    out[y as usize * w as usize + x as usize] = sample;
                }
            }
        }
    }
    Ok(out)
}

/// One Add Noise sample at 32 bits: `v·(1 + δ)` in binary64, with `δ` the
/// draw [`noise_delta`] makes at every depth (the same stream, so a seed is
/// the same grain). Add Noise at 32 bits is RELATIVE in linear light: black
/// stays black and a highlight gets grain in proportion to its light, where
/// the 16-bit additive `v + δ` on display codes would vanish into a
/// highlight's headroom and swamp the shadows of a linear layer.
pub(crate) fn noise_relative(value: f32, delta: f32) -> f32 {
    (f64::from(value) * (1.0 + f64::from(delta))) as f32
}
