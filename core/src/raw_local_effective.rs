//! Effective local coverage shares the existing padded floating-point feather.
//! Local density is an effect multiplier (zero disables its contribution), not
//! a layer-mask black-point lift; pixels outside unfeathered coverage stay exact.
//!
//! Local density multiplies contribution, while feather reuses the existing padded floating-point
//! mask blur.
use crate::doc_mask::MaskProperties;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec};
use crate::raw_local_mask::RawLocalMask;
use crate::raw_local_range::LocalRange;
use crate::raw_local_tone::LocalTone;
use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalSettings {
    pub enabled: bool,
    pub density: f64,
    pub feather: f64,
    /// The local's luminance/colour range (`raw_local_range.rs`); it
    /// multiplies the coverage [`EffectiveMask::at`] answers and is not part
    /// of it, so feather and density act on the shape alone.
    #[serde(default)]
    pub range: Option<LocalRange>,
    /// The local's tone and colour controls (`raw_local_tone.rs`), applied to
    /// its source before the blend; they never change the coverage.
    #[serde(default)]
    pub tone: Option<LocalTone>,
}
impl LocalSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !self.density.is_finite()
            || !(0.0..=1.0).contains(&self.density)
            || !self.feather.is_finite()
            || !(0.0..=256.0).contains(&self.feather)
            || (self.density > 0.0
                && (self.density as f32) < crate::precision_render::MIN_RENDER_ALPHA)
            || (self.feather > 0.0 && self.feather as f32 == 0.0)
        {
            return Err("invalid local density, feather or positive arithmetic range".into());
        }
        if let Some(range) = &self.range {
            range.validate()?;
        }
        if let Some(tone) = &self.tone {
            tone.validate()?;
        }
        Ok(())
    }
    fn properties(&self) -> MaskProperties {
        let mut p = MaskProperties::at((0, 0));
        p.feather = self.feather as f32;
        p
    }
    pub(crate) fn plan(&self, mask: &RawLocalMask) -> Result<(u64, u64), String> {
        self.plan_at(mask.dimensions())
    }
    /// [`Self::plan`] for a mask of `dimensions` that is not built yet (the
    /// Develop Filter's masks are planned before any coverage exists).
    pub(crate) fn plan_at(&self, dimensions: (u32, u32)) -> Result<(u64, u64), String> {
        self.validate()?;
        if !self.enabled || self.density == 0.0 || self.feather == 0.0 {
            return Ok((0, 0));
        }
        let p = self.properties();
        let (w, h, _) = p
            .padded_dimensions(dimensions)
            .ok_or("local feather exceeds supported dimensions")?;
        crate::plane_blur::float_plan(w, h, p.sigma())
    }
}
pub(crate) struct EffectiveMask<'a> {
    mask: &'a RawLocalMask,
    settings: LocalSettings,
    blurred: Option<(Vec<f32>, u32, u32)>,
}
impl<'a> EffectiveMask<'a> {
    pub fn prepare(
        mask: &'a RawLocalMask,
        settings: LocalSettings,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        settings.plan(mask)?;
        let mut out = Self {
            mask,
            settings,
            blurred: None,
        };
        if !settings.enabled || settings.density == 0.0 || settings.feather == 0.0 {
            return Ok(out);
        }
        let p = settings.properties();
        let (w, h, pad) = p
            .padded_dimensions(mask.dimensions())
            .ok_or("local feather exceeds supported dimensions")?;
        let mut values = reserved_vec(pixel_count(w, h)?)?;
        for _ in 0..h {
            job.check()?;
            values.extend(std::iter::repeat_n(0.0, w as usize));
        }
        for y in 0..mask.height {
            for x in 0..mask.width {
                let i = y as usize * mask.width as usize + x as usize;
                job.every(i)?;
                values[(y + pad) as usize * w as usize + (x + pad) as usize] =
                    f32::from(mask.at(i)) / 65535.0;
            }
        }
        crate::plane_blur::floats(&mut values, w, h, p.sigma(), job)?;
        out.blurred = Some((values, w, pad));
        Ok(out)
    }
    pub fn at(&self, index: usize) -> Result<f32, String> {
        if !self.settings.enabled || self.settings.density == 0.0 {
            return Ok(0.0);
        }
        let raw = if let Some((values, w, pad)) = &self.blurred {
            let x = index % self.mask.width as usize;
            let y = index / self.mask.width as usize;
            values[(y + *pad as usize) * *w as usize + x + *pad as usize].clamp(0.0, 1.0)
        } else {
            f32::from(self.mask.at(index)) / 65535.0
        };
        crate::precision_render::checked_alpha(1.0, self.settings.density as f32, raw)
    }
}
