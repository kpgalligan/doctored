//! Native storage adapters for Apply Image/Calculations and luminosity masks.
//! The blend table and Dissolve coordinates are doc_plane's existing operator;
//! grayscale HSL refusal and operand inversion retain the byte contract.
use crate::blend::BlendMode;
use crate::job_control::JobContext;
use crate::plane_scale_offset::PlaneBlend;
use crate::precision_image::reserved_vec;
use serde::Deserialize;
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LuminosityBand {
    Lights,
    Darks,
    Midtones,
}
impl LuminosityBand {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Lights => 0,
            Self::Darks => 1,
            Self::Midtones => 2,
        }
    }
}
pub(crate) fn validate(channels: usize, mode: i32, opacity: f64) -> Result<BlendMode, String> {
    let mode = BlendMode::from_c(mode).ok_or("unknown plane blend mode")?;
    if !matches!(channels, 1 | 3)
        || mode.is_group_only()
        || channels == 1 && crate::doc_plane::degenerates_on_gray(mode)
    {
        return Err("plane blend needs one scalar or three RGB planes; HSL requires RGB and Pass Through is unavailable".into());
    }
    if !opacity.is_finite() || !(0. ..=1.).contains(&opacity) {
        return Err("plane blend opacity must be finite in [0,1]".into());
    }
    Ok(mode)
}
/// [`validate`] plus Apply Image's Scale and Offset (`plane_scale_offset`):
/// the blend one native plane pixel runs.
pub(crate) fn validate_scaled(
    channels: usize,
    mode: i32,
    opacity: f64,
    scale: f64,
    offset: f64,
) -> Result<PlaneBlend, String> {
    let mode = validate(channels, mode, opacity)?;
    Ok(PlaneBlend::new(mode, scale as f32, offset as f32)?)
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn blend(
    input: &[u16],
    dimensions: (u32, u32),
    channels: usize,
    mode: i32,
    opacity: f64,
    (scale, offset): (f64, f64),
    invert_base: bool,
    invert_source: bool,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let blend = validate_scaled(channels, mode, opacity, scale, offset)?;
    let (w, h) = dimensions;
    let n = w as usize * h as usize;
    if input.len() != n * channels * 2 {
        return Err("plane blend input length differs from plan".into());
    }
    let mut out = reserved_vec(n * channels)?;
    out.resize(n * channels, 0);
    for i in 0..n {
        job.every(i)?;
        let base: [u16; 3] = std::array::from_fn(|c| {
            let v = input[if channels == 1 { i } else { c * n + i }];
            if invert_base {
                65535 - v
            } else {
                v
            }
        });
        let source: [u16; 3] = std::array::from_fn(|c| {
            let v = input[n * channels + if channels == 1 { i } else { c * n + i }];
            if invert_source {
                65535 - v
            } else {
                v
            }
        });
        let b = base.map(|v| f32::from(v) / 65535.);
        let s = source.map(|v| f32::from(v) / 65535.);
        let result = blend.triple(
            b,
            s,
            opacity as f32,
            ((i % w as usize) as i64, (i / w as usize) as i64),
        );
        for c in 0..channels {
            out[c * n + i] = if opacity == 0. || result[c] == b[c] {
                base[c]
            } else {
                (result[c].clamp(0., 1.) * 65535.).round() as u16
            };
        }
        if i & 65535 == 0 {
            job.report(i as f64 / n as f64, 6)?;
        }
    }
    job.check()?;
    Ok(out)
}
