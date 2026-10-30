//! Display-only local-mask overlay, sampled in the same source region/divisor
//! coordinates as Develop. Native coverage remains in its immutable RZLM blob.
//!
//! Previews are explicitly byte display overlays; `dr_raw_local_mask_preview_scratch` uses the
//! actual worker planner so large feathered previews do not rely on a fixed UI budget.
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, reserved_vec, BOOKKEEPING_BYTES};
use crate::raw_local_effective::{EffectiveMask, LocalSettings};
use crate::raw_local_mask::RawLocalMask;
use crate::raw_local_range::LocalRange;

pub struct LocalPreview {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub divisor: u32,
    pub density: f64,
    pub feather: f64,
    pub effective: bool,
}
impl RawLocalMask {
    fn preview_plan(&self, view: &LocalPreview) -> Result<(u32, u32, LocalSettings, u64), String> {
        if view.width == 0
            || view.height == 0
            || view.divisor == 0
            || view
                .x
                .checked_add(view.width)
                .is_none_or(|v| v > self.width)
            || view
                .y
                .checked_add(view.height)
                .is_none_or(|v| v > self.height)
        {
            return Err("invalid local-mask preview region".into());
        }
        let (w, h) = (
            view.width.div_ceil(view.divisor),
            view.height.div_ceil(view.divisor),
        );
        if w > 1600 || h > 1600 {
            return Err("local-mask preview exceeds 1600 pixels per side".into());
        }
        let settings = LocalSettings {
            enabled: true,
            density: view.density,
            feather: view.feather,
            range: None,
            tone: None,
        };
        settings.validate()?;
        let actual = if view.effective {
            settings
        } else {
            LocalSettings {
                enabled: true,
                density: 1.0,
                feather: 0.0,
                range: None,
                tone: None,
            }
        };
        let plan = actual.plan(self)?;
        let scratch = plan.0 + u64::from(w) * u64::from(h) * 4 + BOOKKEEPING_BYTES;
        Ok((w, h, actual, scratch))
    }
    /// Same ROI/coverage validation and conservative plan used by preview,
    /// without constructing a coverage or display buffer.
    pub fn preview_scratch(&self, view: LocalPreview) -> Result<u64, String> {
        self.preview_plan(&view).map(|p| p.3)
    }
    /// `range` is the selected local's range with the base development it
    /// reads (`raw_local_range.rs`): the overlay's alpha is then the shape
    /// coverage times the range's coverage of the base sample under each
    /// display pixel, the weight `raw_local_develop::develop` blends by. The
    /// range is applied whether or not `view.effective` is set. The base is
    /// the caller's own handle and is read in place: no plane is built, so
    /// the plan is `preview_scratch`'s, and the admission counts the base
    /// beside the coverage as retained, not as scratch.
    pub fn preview(
        &self,
        view: LocalPreview,
        range: Option<(&PrecisionDocument, &LocalRange)>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<image::RgbaImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let (w, h, actual, scratch) = self.preview_plan(&view)?;
        let mut retained = self.retained_bytes();
        let range = range
            .map(|(base, range)| {
                range.validate()?;
                let mut inventory = crate::resources::Inventory::default();
                self.record_resources(&mut inventory);
                base.record_resources(&mut inventory);
                retained = inventory.unique_bytes();
                let image = crate::raw_local_develop::plain_image(base)?;
                if image.dimensions() != self.dimensions() {
                    return Err(String::from(
                        "local range base differs from the coverage dimensions",
                    ));
                }
                let model = image
                    .profile()
                    .model()
                    .ok_or(crate::adjust_calibrated::PROFILE_REFUSAL)?;
                Ok((image, model, range))
            })
            .transpose()?;
        admit(retained, scratch, other, limit)?;
        job.reserve(scratch)?;
        let coverage = EffectiveMask::prepare(self, actual, job)?;
        let mut pixels = reserved_vec(w as usize * h as usize * 4)?;
        for y in 0..h {
            job.check()?;
            for x in 0..w {
                let sx = view.x + (x * view.divisor + view.divisor / 2).min(view.width - 1);
                let sy = view.y + (y * view.divisor + view.divisor / 2).min(view.height - 1);
                let mut weight = coverage.at(sy as usize * self.width as usize + sx as usize)?;
                if weight != 0.0 {
                    // An f32 base (a 32-bit float development) is linear
                    // light: the same coverage of `lab_linear`.
                    if let Some((image, model, range)) = range.filter(|(image, _, _)| {
                        image.storage() == crate::precision_image::PrecisionStorage::Rgba32Float
                    }) {
                        let s = image
                            .sample_at(sx, sy)
                            .ok_or("local base requires genuine RGBA32 float samples")?;
                        weight *= range.coverage(crate::raw_local_range::lab_linear(
                            model,
                            [s[0], s[1], s[2]],
                        ));
                    } else if let Some((image, model, range)) = range {
                        let s = image
                            .rgba16_at(sx, sy)
                            .ok_or("local base requires genuine RGBA16 samples")?;
                        weight *=
                            range.coverage(crate::raw_local_range::lab(model, [s[0], s[1], s[2]]));
                    }
                }
                let alpha = (weight * 255.0).round() as u8;
                pixels.extend_from_slice(&[255, 0, 0, alpha]);
            }
        }
        job.report(1.0, 5)?;
        image::RgbaImage::from_raw(w, h, pixels)
            .ok_or_else(|| "invalid local-mask preview output".into())
    }
}
