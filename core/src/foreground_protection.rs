//! Experimental source-only restriction of foreground color correction.
//!
//! This hypothesis protects pixels outside a low-coverage footprint and pixels
//! matching nearby high-prior observed foreground colors. It does not infer
//! physical opacity, change alpha, or qualify decontamination by construction.
//! Source and prior may themselves be ambiguous; the original opaque/color
//! gates must score the complete output without exemptions.
//!
//! failed source-only color-protection hypothesis over the existing foreground estimate, with a
//! declared low-alpha footprint, nearby observed foreground support, exact user-known RGB, bounded
//! samples and cancellation. It never changes alpha or uses reference colors; all original combined
//! gates remain mandatory.
use crate::{
    doc::MAX_PIXELS,
    job_control::{DrJobControl, JobContext},
};
use image::{GrayImage, RgbaImage};

const RADIUS: u32 = 16;
const MATCH_CODES: u8 = 8;
const MAX_SAMPLE_CHECKS: u64 = 250_000_000;

pub struct ProtectedForeground {
    /// None means unchanged source RGB, not unchanged estimated RGB.
    pub image: Option<RgbaImage>,
    pub eligible_pixels: usize,
    pub protected_by_source_color: usize,
    pub corrected_pixels: usize,
}

/// Immutable encoded-RGBA8 research API. User labels must be exact 0/128/255;
/// all user-known pixels retain source RGB. Inputs must share opaque extent.
pub fn protect(
    source: &RgbaImage,
    estimated: &RgbaImage,
    alpha: &GrayImage,
    prior: &GrayImage,
    user_constraints: &[u8],
    control: Option<&DrJobControl>,
) -> Result<ProtectedForeground, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let (w, h) = source.dimensions();
    let n = u64::from(w) * u64::from(h);
    if n == 0
        || n > MAX_PIXELS
        || estimated.dimensions() != (w, h)
        || alpha.dimensions() != (w, h)
        || prior.dimensions() != (w, h)
        || user_constraints.len() != n as usize
    {
        return Err("foreground protection dimensions or buffer lengths are invalid".into());
    }
    let eligible = |i: usize| {
        alpha.as_raw()[i] > 0
            && alpha.as_raw()[i] < 128
            && prior.as_raw()[i] < 128
            && user_constraints[i] == 128
    };
    let mut eligible_pixels = 0;
    for (i, (p, q)) in source.pixels().zip(estimated.pixels()).enumerate() {
        job.every(i)?;
        if p[3] != 255 || q[3] != 255 || ![0, 128, 255].contains(&user_constraints[i]) {
            return Err(
                "foreground protection requires opaque pixels and categorical user labels".into(),
            );
        }
        eligible_pixels += usize::from(eligible(i) && p != q);
    }
    let checks = eligible_pixels as u64 * u64::from(2 * RADIUS + 1).pow(2);
    if checks > MAX_SAMPLE_CHECKS {
        return Err(
            "foreground protection exceeds the bounded local-sample budget; narrow the unknown band or refine a smaller area"
                .into(),
        );
    }
    if eligible_pixels == 0 {
        return Ok(ProtectedForeground {
            image: None,
            eligible_pixels,
            protected_by_source_color: 0,
            corrected_pixels: 0,
        });
    }
    job.reserve(n * 4 + 64 * 1024)?;
    let mut bytes = Vec::with_capacity(source.as_raw().len());
    for chunk in source.as_raw().chunks(16384) {
        job.check()?;
        bytes.extend_from_slice(chunk);
    }
    let mut output =
        RgbaImage::from_raw(w, h, bytes).ok_or("invalid foreground protection output")?;
    let mut protected = 0;
    let mut corrected = 0;
    for y in 0..h {
        job.report(y as f64 / h as f64, 4)?;
        for x in 0..w {
            let i = y as usize * w as usize + x as usize;
            job.every(i)?;
            let observed = source.get_pixel(x, y);
            let candidate = estimated.get_pixel(x, y);
            if !eligible(i) || observed == candidate {
                continue;
            }
            let mut supported = false;
            'neighbors: for sy in y.saturating_sub(RADIUS)..=(y + RADIUS).min(h - 1) {
                job.check()?;
                for sx in x.saturating_sub(RADIUS)..=(x + RADIUS).min(w - 1) {
                    let j = sy as usize * w as usize + sx as usize;
                    // A user's explicit foreground can provide observed color
                    // support even when the uncertain automatic prior disagrees.
                    if user_constraints[j] == 0
                        || (user_constraints[j] != 255 && prior.as_raw()[j] < 247)
                    {
                        continue;
                    }
                    let p = source.get_pixel(sx, sy);
                    if (0..3).all(|c| p[c].abs_diff(observed[c]) <= MATCH_CODES) {
                        supported = true;
                        break 'neighbors;
                    }
                }
            }
            if supported {
                protected += 1;
            } else {
                output.put_pixel(x, y, *candidate);
                corrected += 1;
            }
        }
    }
    job.report(1.0, 5)?;
    Ok(ProtectedForeground {
        image: (corrected > 0).then_some(output),
        eligible_pixels,
        protected_by_source_color: protected,
        corrected_pixels: corrected,
    })
}
