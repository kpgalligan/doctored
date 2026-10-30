//! Clouds and Difference Clouds: the Render filters, as a
//! positional fractal value noise mixed between the foreground and background
//! colours. A value depends only on the seed and the plane position, so the
//! same seed always renders the same clouds, at either depth and in any order.
//!
//! Octave `o` (0…`CLOUD_OCTAVES`−1) is value noise on a lattice of period
//! `CLOUD_BASE_PERIOD / 2^o` pixels: lattice values are uniform in [0, 1) from
//! `SplitMix64::at(seed, ix, iy, o)`, interpolated with smoothstep weights at the
//! pixel centre. The octaves are summed at weights `0.5^o` and normalized, then
//! stretched about ½ by a gain and clamped: a sum of independent octaves
//! crowds towards the middle, and the gain restores the full light-to-dark
//! sweep. High contrast is the larger gain, which clips more of both ends.
//!
//! The mix is `fg + (bg − fg)·t` on unit bytes with no gamma conversion.
//! Clouds writes `[mix, 1]`, replacing alpha as a render does. Difference
//! Clouds writes `|source − mix|` on the colour and keeps the source alpha;
//! an alpha-0 pixel is left untouched.
//!
//! Clouds and Difference Clouds at 32 bits: the float route runs this combine
//! on the layer's DISPLAY-ENCODED copy (`display_encoding`), so the
//! foreground and background hex colours are mixed in document-encoded units
//! exactly as at 16 bits, and Difference Clouds' `|source − mix|` reads the
//! encoded source; the result is decoded back to linear light. The cloud
//! field is generated, not measured, so it has no parameter to restate.
//!
//! Clouds: positional 8-octave smoothstep value noise (`cloud_value`), gain 2 (4 high contrast),
//! mixed fg→bg on unit bytes; Clouds writes `[mix, 1]`, Difference Clouds `|src − mix|` with alpha
//! kept and alpha-0 untouched.
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::rng::SplitMix64;

/// Eight octaves take the finest lattice to a two-pixel period, below which
/// value noise only adds aliasing.
pub(crate) const CLOUD_OCTAVES: u32 = 8;
/// The coarsest lattice period in pixels: about the size of the largest
/// cloud forms, independent of the image size.
pub(crate) const CLOUD_BASE_PERIOD: f64 = 256.0;
/// The stretch about ½: the 0.5^o-weighted mean of eight independent uniform
/// octaves has a standard deviation of about 0.17, so ×2 spreads the bulk of
/// it over most of [0, 1] with only a little clipped at each end.
pub(crate) const CLOUD_GAIN: f64 = 2.0;
/// High contrast doubles the stretch, which clips the outer tails to pure
/// foreground and background.
pub(crate) const CLOUD_HIGH_CONTRAST_GAIN: f64 = 4.0;
/// Work units per pixel: eight octaves of four positional hashes and a
/// smoothstep bilinear blend, plus the combine. Frozen from the `#[ignore]`d
/// `radial_clouds_offset_calibration_probe` (`tests/radial_clouds_offset.rs`):
/// at 6000×4000, min of 3, at the reference's r16-unit rate, the larger of the
/// 8-bit run and the whole 16-bit edit, the largest over two runs: Clouds
/// 1.178 s / 1.499 s and 1.173 s / 1.506 s (up to 72.33 units), Difference
/// Clouds high contrast 1.197 s / 1.530 s and 1.207 s / 1.543 s (73.35 and
/// 74.12); 74.12 × 1.15 = 86. 24 MP charges 2.06e9 of 2.4e10, so every image
/// up to the 100 MP pixel cap is admitted. The record is in
/// `designs/phase-9f-selected-tone-status.md`.
pub(crate) const CLOUDS_WORK_PER_PIXEL: u64 = 86;

/// The lattice value at `(ix, iy)` of octave `o`, uniform in [0, 1).
fn lattice(seed: u64, ix: u64, iy: u64, octave: u32) -> f64 {
    (SplitMix64::at(seed, ix, iy, u64::from(octave)).next_u64() >> 11) as f64 / (1_u64 << 53) as f64
}

/// Smoothstep: zero slope at both lattice points, so the octave has no creases.
fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// The cloud value in [0, 1] at plane pixel `(x, y)`.
pub(crate) fn cloud_value(seed: u64, x: u32, y: u32, high_contrast: bool) -> f64 {
    let (mut sum, mut weights) = (0.0, 0.0);
    let mut weight = 1.0;
    for octave in 0..CLOUD_OCTAVES {
        let period = CLOUD_BASE_PERIOD / f64::from(1_u32 << octave);
        let (u, v) = ((f64::from(x) + 0.5) / period, (f64::from(y) + 0.5) / period);
        let (fu, fv) = (u.floor(), v.floor());
        let (ix, iy) = (fu as u64, fv as u64);
        let (sx, sy) = (smooth(u - fu), smooth(v - fv));
        let top = lattice(seed, ix, iy, octave)
            + sx * (lattice(seed, ix + 1, iy, octave) - lattice(seed, ix, iy, octave));
        let bottom = lattice(seed, ix, iy + 1, octave)
            + sx * (lattice(seed, ix + 1, iy + 1, octave) - lattice(seed, ix, iy + 1, octave));
        sum += weight * (top + sy * (bottom - top));
        weights += weight;
        weight *= 0.5;
    }
    let gain = if high_contrast {
        CLOUD_HIGH_CONTRAST_GAIN
    } else {
        CLOUD_GAIN
    };
    (0.5 + gain * (sum / weights - 0.5)).clamp(0.0, 1.0)
}

/// `(scratch, work)` refused before any allocation. No frame: clouds fill the
/// whole plane and the selection's coverage scopes the write.
pub(crate) fn clouds_plan(dimensions: (u32, u32), cap: u64) -> Result<(u64, u64), String> {
    let n = pixel_count(dimensions.0, dimensions.1)? as u64;
    let work = n.saturating_mul(CLOUDS_WORK_PER_PIXEL);
    if work > cap {
        return Err("clouds exceeds the bounded pixel-work limit".into());
    }
    Ok((n * 16 + 2 * BOOKKEEPING_BYTES, work))
}

/// The plane: `[t, 0, 0, 0]` per pixel, `t` the cloud value.
pub(crate) fn clouds_plane(
    dimensions: (u32, u32),
    seed: u64,
    high_contrast: bool,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let (w, h) = dimensions;
    let mut out = reserved_vec(w as usize * h as usize)?;
    for y in 0..h {
        job.check()?;
        for x in 0..w {
            out.push([cloud_value(seed, x, y, high_contrast) as f32, 0.0, 0.0, 0.0]);
        }
        if y % 64 == 63 {
            job.report(f64::from(y + 1) / f64::from(h), 6)?;
        }
    }
    Ok(out)
}

/// One output pixel from its straight source and cloud value `t`.
pub(crate) fn clouds_combine(
    source: [f32; 4],
    t: f32,
    foreground: [u8; 3],
    background: [u8; 3],
    difference: bool,
) -> [f32; 4] {
    if difference && source[3] <= 0.0 {
        return source;
    }
    let mix = |c: usize| {
        let (fg, bg) = (f32::from(foreground[c]), f32::from(background[c]));
        (fg + (bg - fg) * t) / 255.0
    };
    let mut out = [mix(0), mix(1), mix(2), 1.0];
    if difference {
        for (c, value) in out.iter_mut().take(3).enumerate() {
            *value = (source[c] - *value).abs();
        }
        out[3] = source[3];
    }
    out
}
