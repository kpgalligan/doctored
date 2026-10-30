//! Pixel-center local fields, separate from artwork's area-coverage policy.
//! Soft radial and linear fields retain continuous values until native storage.
//! A brush evaluates only bounded dab rectangles, composes ordered f32 coverage,
//! and quantizes once per command; it never passes through byte coverage.
//!
//! Pixel-center radial smoothstep/linear fields and bounded ordered brush dabs quantize once at
//! native depth; hard radial coverage is a point-center cut, not artwork area integration.
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::raw_local_mask::{dimensions, RawLocalMask, RAW_LOCAL_HEADER};
use serde::Deserialize;

pub const MAX_RAW_LOCAL_DABS: usize = 4096;
pub const MAX_RAW_LOCAL_WORK: u64 = 256_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dab {
    pub center: [f64; 2],
    pub radius: f64,
    pub hardness: f64,
    pub opacity: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushMode {
    Add,
    Subtract,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    // Empty struct variants preserve deny_unknown_fields. Serde's internally
    // tagged unit variants otherwise discard extra object fields silently.
    Empty {},
    Full {},
    Radial {
        center: [f64; 2],
        radius: [f64; 2],
        hardness: f64,
    },
    Linear {
        from: [f64; 2],
        to: [f64; 2],
    },
    Invert {},
    Brush {
        mode: BrushMode,
        dabs: Vec<Dab>,
    },
}
fn unit(v: f64) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}
fn point(p: [f64; 2], w: u32, h: u32) -> bool {
    p[0].is_finite()
        && p[1].is_finite()
        && (0.0..=w as f64).contains(&p[0])
        && (0.0..=h as f64).contains(&p[1])
}
fn radius(r: f64, w: u32, h: u32) -> bool {
    r.is_finite() && r > 0.0 && r <= 2.0 * f64::from(w.max(h))
}
fn bounds(d: &Dab, w: u32, h: u32) -> [usize; 4] {
    [
        (d.center[0] - d.radius - 0.5).ceil().clamp(0.0, w as f64) as usize,
        (d.center[1] - d.radius - 0.5).ceil().clamp(0.0, h as f64) as usize,
        (d.center[0] + d.radius - 0.5)
            .floor()
            .mul_add(1.0, 1.0)
            .clamp(0.0, w as f64) as usize,
        (d.center[1] + d.radius - 0.5)
            .floor()
            .mul_add(1.0, 1.0)
            .clamp(0.0, h as f64) as usize,
    ]
}
pub(crate) fn radial(p: [f64; 2], center: [f64; 2], radius: [f64; 2], hardness: f64) -> f64 {
    let d = ((p[0] - center[0]) / radius[0]).hypot((p[1] - center[1]) / radius[1]);
    if d <= hardness {
        1.0
    } else if d >= 1.0 {
        0.0
    } else {
        let t = (d - hardness) / (1.0 - hardness);
        1.0 - t * t * (3.0 - 2.0 * t)
    }
}
impl Command {
    pub fn parse(text: &str, w: u32, h: u32) -> Result<Self, String> {
        let command: Self =
            serde_json::from_str(text).map_err(|e| format!("invalid local mask command: {e}"))?;
        command.validate(w, h)?;
        Ok(command)
    }
    /// Every check [`Self::parse`] makes after deserializing, in its order and
    /// with its sentences, on a `w`×`h` plane: `Ok` is the brush's evaluated
    /// pixel count (the sum of its clipped dab rectangles), 0 for any other
    /// command. The Develop Filter's masks (`develop_filter_masks`) build their
    /// pixel-unit strokes and pass them through here.
    pub(crate) fn validate(&self, w: u32, h: u32) -> Result<u64, String> {
        let mut work = 0;
        let valid = match self {
            Self::Empty {} | Self::Full {} | Self::Invert {} => true,
            Self::Radial {
                center,
                radius: r,
                hardness,
            } => point(*center, w, h) && r.iter().all(|v| radius(*v, w, h)) && unit(*hardness),
            Self::Linear { from, to } => point(*from, w, h) && point(*to, w, h) && from != to,
            Self::Brush { dabs, .. } => {
                if dabs.len() > MAX_RAW_LOCAL_DABS {
                    return Err("local brush exceeds 4096 dabs; use a shorter stroke".into());
                }
                for d in dabs {
                    if !point(d.center, w, h)
                        || !radius(d.radius, w, h)
                        || !unit(d.hardness)
                        || !unit(d.opacity)
                    {
                        return Err("invalid local brush dab".into());
                    }
                    let [x0, y0, x1, y1] = bounds(d, w, h);
                    work += (x1 - x0) as u64 * (y1 - y0) as u64;
                    if work > MAX_RAW_LOCAL_WORK {
                        return Err(
                            "local brush exceeds 256 million evaluated pixels; use a shorter stroke or a smaller brush"
                                .into(),
                        );
                    }
                }
                true
            }
        };
        if !valid {
            return Err("local mask coordinates, radius or hardness are invalid".into());
        }
        Ok(work)
    }
    pub(crate) fn brush(&self) -> bool {
        matches!(self, Self::Brush { .. })
    }
    pub(crate) fn apply(
        &self,
        prior: Option<&RawLocalMask>,
        w: u32,
        h: u32,
        job: JobContext<'_>,
    ) -> Result<Vec<u8>, String> {
        let n = dimensions(w, h)?;
        let mut bytes = RawLocalMask::empty_bytes(w, h)?;
        if matches!(self, Self::Invert {}) && prior.is_none() {
            return Err("inverting requires a captured local mask".into());
        }
        if let Self::Brush { mode, dabs } = self {
            let mut field = reserved_vec(n)?;
            for i in 0..n {
                job.every(i)?;
                field.push(prior.map_or(0.0, |p| f32::from(p.at(i)) / 65535.0));
            }
            for (di, d) in dabs.iter().enumerate() {
                let [x0, y0, x1, y1] = bounds(d, w, h);
                for y in y0..y1 {
                    job.check()?;
                    for x in x0..x1 {
                        job.every(x)?;
                        let v = (radial(
                            [x as f64 + 0.5, y as f64 + 0.5],
                            d.center,
                            [d.radius; 2],
                            d.hardness,
                        ) * d.opacity) as f32;
                        if v == 0.0 {
                            continue;
                        }
                        let old = &mut field[y * w as usize + x];
                        *old = match mode {
                            BrushMode::Add => *old + v * (1.0 - *old),
                            BrushMode::Subtract => crate::blend::erase_alpha(*old, v),
                        };
                    }
                }
                job.report((di + 1) as f64 / (dabs.len().max(1) as f64) * 0.9, 3)?;
            }
            for (i, v) in field.into_iter().enumerate() {
                job.every(i)?;
                bytes[RAW_LOCAL_HEADER + i * 2..RAW_LOCAL_HEADER + i * 2 + 2]
                    .copy_from_slice(&((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes());
            }
        } else {
            for y in 0..h {
                job.check()?;
                for x in 0..w {
                    job.every(x as usize)?;
                    let i = y as usize * w as usize + x as usize;
                    let p = [x as f64 + 0.5, y as f64 + 0.5];
                    let code = match self {
                        Self::Empty {} => 0,
                        Self::Full {} => 65535,
                        Self::Invert {} => 65535 - prior.unwrap().at(i),
                        Self::Radial {
                            center,
                            radius,
                            hardness,
                        } => (radial(p, *center, *radius, *hardness) * 65535.0).round() as u16,
                        Self::Linear { from, to } => {
                            let d = [to[0] - from[0], to[1] - from[1]];
                            let t = ((p[0] - from[0]) * d[0] + (p[1] - from[1]) * d[1])
                                / (d[0] * d[0] + d[1] * d[1]);
                            (t.clamp(0.0, 1.0) * 65535.0).round() as u16
                        }
                        Self::Brush { .. } => unreachable!(),
                    };
                    bytes[RAW_LOCAL_HEADER + i * 2..RAW_LOCAL_HEADER + i * 2 + 2]
                        .copy_from_slice(&code.to_le_bytes());
                }
                job.report((y + 1) as f64 / h as f64 * 0.9, 3)?;
            }
        }
        Ok(bytes)
    }
}
