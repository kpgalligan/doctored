//! The adapter half of the RAW local tone path: binds one local's
//! [`LocalTone`] descriptions to the base development's profile through the
//! precision_v1 adjustment interpreter (`PrecisionAdjustment`, the one
//! `dr_precision_doc_apply_adjustment` uses), builds the Shadows/Highlights
//! guide and the Presence plane a local's evaluation reads, runs the
//! Sharpness and Noise Reduction stage (`local_detail`) last over the
//! result, and answers the adjusted sample per pixel. `raw_local_develop`
//! blends that sample through the local's coverage; no colour or tone math
//! lives here.
//!
//! A stage is built over one source (the local's camera variant, or the
//! development as the earlier locals left it) and read at the same indices,
//! so the guide and the Presence plane describe exactly the samples
//! [`ToneStage::value`] is asked about. Pixels with alpha 0 are returned as
//! read, as the destructive adjustment leaves them.
use crate::adjust_precision::{PrecisionAdjustment, PrecisionGuide};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::local_detail::LocalDetail;
use crate::precision_image::{pixel_count, BOOKKEEPING_BYTES};
use crate::raw_local_tone::LocalTone;
use crate::sampling_presence::Presence;

pub(crate) struct ToneStage {
    /// Exposure and Contrast: pointwise, ahead of the guide.
    before: Vec<PrecisionAdjustment>,
    /// Highlights and Shadows: reads the guide built over `before`'s output.
    shadows_highlights: Option<PrecisionAdjustment>,
    /// Whites, Blacks and Saturation: pointwise, after the guide.
    after: Vec<PrecisionAdjustment>,
    presence: Option<Presence>,
    /// Sharpness and Noise Reduction: last, over the Presence plane or the
    /// pointwise steps.
    detail: Option<LocalDetail>,
    dimensions: (u32, u32),
    /// The largest preparation allowance of the descriptions
    /// (`PrecisionAdjustment::prepare_scratch_bytes`), held in the plan for
    /// the re-preparation of a stage.
    preparation: u64,
    guide: Option<PrecisionGuide>,
    plane: Option<Vec<[f32; 4]>>,
}

impl ToneStage {
    /// The stage for `tone` under `profile`, or None when every control is 0.
    /// Refuses an invalid tone and a profile an operation cannot use (the
    /// calibrated exposure needs a matrix/TRC model).
    pub(crate) fn prepare(
        tone: &LocalTone,
        profile: &IccProfile,
        dimensions: (u32, u32),
    ) -> Result<Option<Self>, String> {
        tone.validate()?;
        if tone.is_identity() {
            return Ok(None);
        }
        let mut stage = Self {
            before: Vec::new(),
            shadows_highlights: None,
            after: Vec::new(),
            presence: tone.presence(),
            detail: tone.detail(),
            dimensions,
            preparation: 0,
            guide: None,
            plane: None,
        };
        for description in tone.descriptions(dimensions) {
            stage.preparation = stage
                .preparation
                .max(PrecisionAdjustment::prepare_scratch_bytes(
                    &description,
                    profile,
                )?);
            let operation = PrecisionAdjustment::parse(&description, profile)?;
            if operation.is_identity() {
                continue;
            }
            match operation.operation() {
                "exposure" | "bcs" => stage.before.push(operation),
                "shadows_highlights" => stage.shadows_highlights = Some(operation),
                _ => stage.after.push(operation),
            }
        }
        Ok(Some(stage))
    }

    /// The bytes this stage holds once built: its prepared operations, the
    /// preparation allowance, the guide's plan and the larger of the Presence
    /// plan (its result plane included) and the detail plan beside the n×16
    /// Presence plane it reads. Refuses work above `cap` before any
    /// allocation.
    pub(crate) fn plan(&self, cap: u64) -> Result<u64, String> {
        let (w, h) = self.dimensions;
        let mut total = self.preparation + 2 * BOOKKEEPING_BYTES;
        for operation in self.operations() {
            total = total.saturating_add(operation.retained_bytes());
        }
        if let Some(operation) = &self.shadows_highlights {
            if operation.guide_work_units(w, h)? > cap {
                return Err("local tone exceeds the bounded pixel-work limit".into());
            }
            total = total.saturating_add(operation.guide_scratch_bytes(w, h)?);
        }
        let presence = match &self.presence {
            Some(presence) => presence.plan(self.dimensions, cap)?.0,
            None => 0,
        };
        let detail = match &self.detail {
            Some(detail) => {
                let read = if self.presence.is_some() {
                    pixel_count(w, h)? as u64 * 16
                } else {
                    0
                };
                read.saturating_add(detail.plan(self.dimensions, cap)?.0)
            }
            None => 0,
        };
        Ok(total.saturating_add(presence.max(detail)))
    }

    /// Builds the guide over the pointwise steps ahead of it, then the
    /// Presence plane over every pointwise step, both from `read`, then the
    /// detail plane over the Presence plane (dropped once read) or the
    /// pointwise steps.
    pub(crate) fn build(
        &mut self,
        read: &dyn Fn(usize) -> [f32; 4],
        cap: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        self.plan(cap)?;
        let (w, h) = self.dimensions;
        if let Some(operation) = &self.shadows_highlights {
            let before = &self.before;
            let guide = operation.guide(
                w,
                h,
                &|i| stepped(before, i, w, read(i)),
                job.range(0.0, 0.3),
            )?;
            self.guide = guide;
        }
        job.report(0.3, 3)?;
        let split = if self.detail.is_some() { 0.65 } else { 1.0 };
        let presence = match self.presence {
            Some(presence) => Some(presence.plane(
                |i| self.pointwise(i, read(i)),
                self.dimensions,
                cap,
                job.range(0.3, split),
            )?),
            None => None,
        };
        self.plane = match self.detail {
            Some(detail) => Some(detail.plane_in(
                |i| match &presence {
                    Some(plane) => plane[i],
                    None => self.pointwise(i, read(i)),
                },
                self.dimensions,
                cap,
                job.range(split, 1.0),
            )?),
            None => presence,
        };
        job.report(1.0, 3)
    }

    /// The adjusted sample at `index` whose source sample is `sample`: the
    /// built plane's value when Clarity, Texture, Sharpness or Noise
    /// Reduction is set (built over the same source), else the pointwise
    /// steps.
    pub(crate) fn value(&self, index: usize, sample: [f32; 4]) -> [f32; 4] {
        match &self.plane {
            Some(plane) => plane[index],
            None => self.pointwise(index, sample),
        }
    }

    fn operations(&self) -> impl Iterator<Item = &PrecisionAdjustment> {
        self.before
            .iter()
            .chain(&self.shadows_highlights)
            .chain(&self.after)
    }

    fn pointwise(&self, index: usize, sample: [f32; 4]) -> [f32; 4] {
        if sample[3] <= 0.0 {
            return sample;
        }
        let w = self.dimensions.0 as usize;
        let xy = ((index % w) as i64, (index / w) as i64);
        let mut rgb = [sample[0], sample[1], sample[2]];
        for operation in &self.before {
            rgb = operation.apply_at(rgb, 0.0, xy);
        }
        if let Some(operation) = &self.shadows_highlights {
            let guide = self
                .guide
                .as_ref()
                .map_or(0.0, |g| g.at(xy.0 as u32, xy.1 as u32));
            rgb = operation.apply_at(rgb, guide, xy);
        }
        for operation in &self.after {
            rgb = operation.apply_at(rgb, 0.0, xy);
        }
        [rgb[0], rgb[1], rgb[2], sample[3]]
    }
}

/// `before`'s steps of one source sample: what the Shadows/Highlights guide
/// is built from.
fn stepped(before: &[PrecisionAdjustment], index: usize, w: u32, sample: [f32; 4]) -> [f32; 4] {
    if sample[3] <= 0.0 || before.is_empty() {
        return sample;
    }
    let w = w as usize;
    let xy = ((index % w) as i64, (index / w) as i64);
    let mut rgb = [sample[0], sample[1], sample[2]];
    for operation in before {
        rgb = operation.apply_at(rgb, 0.0, xy);
    }
    [rgb[0], rgb[1], rgb[2], sample[3]]
}
