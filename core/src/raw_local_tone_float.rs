//! The 32-bit float half of the RAW local tone path: one local's
//! [`LocalTone`] on a float (linear) development under rule L
//! (`designs/v1-float-editing.md`). Each control runs in the domain the float
//! route already runs its filter or adjustment in, through the same kernels
//! and the 16-bit stage's descriptions (`raw_local_tone`), never a copy of
//! their math; `raw_float_develop` blends the answered sample through the
//! local's coverage in linear light. In the 16-bit stage's order
//! (`raw_local_tone_apply`):
//!
//! 1. **Exposure** on the stored linear value: the float `exposure` form
//!    (`PrecisionAdjustment::apply_linear`, `v·2^e`), nothing clamped.
//! 2. **Contrast, Highlights and Shadows, Whites and Blacks** on the
//!    display-encoded value (`display_encoding`, E continued above white):
//!    `PrecisionAdjustment::apply_encoded` (`bcs` modern, rule H, Levels
//!    unclamped), the Highlights/Shadows guide built over the Contrast output
//!    as at 16 bits; the result is `decode_changed` back, so a component the
//!    three leave bitwise alone keeps its stored bits.
//! 3. **Saturation** on the decoded linear value: the float `vibrance` form
//!    (the Scaled display form, rule A).
//! 4. **Clarity and Texture** (`Presence::plane_in`), then **Noise Reduction
//!    and positive Sharpness** (`LocalDetail::plane_in`: Reduce Noise, Smart
//!    Sharpen), on the encoded copy of the result so far, at
//!    `SampleRange::Extended` over that copy's visible range, decoded back
//!    with `decode_changed`.
//! 5. **Negative Sharpness** (Gaussian Blur) last, on the linear values, at
//!    `Extended` over their visible range.
//!
//! The encoded values are computed per read, never held as a copy, as the
//! 16-bit stage computes its pointwise steps per read: the stage holds what
//! that one holds (the guide, then one plane at a time), so its plan is that
//! plan with the float range's Gaussian. A tone with no step-2 control
//! encodes nothing pointwise. A sample with alpha 0 is returned as read.
use std::cell::RefCell;

use crate::adjust_precision::{PrecisionAdjustment, PrecisionGuide};
use crate::display_encoding::DisplayEncoding;
use crate::filter_kernels::SampleRange;
use crate::float_filter_apply::PLAN_RANGE;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::local_detail::LocalDetail;
use crate::precision_image::{pixel_count, BOOKKEEPING_BYTES};
use crate::precision_sampling::visible_range;
use crate::raw_local_tone::LocalTone;
use crate::sampling_presence::Presence;

/// The plane a built stage answers from, in the domain its last kernel ran.
enum Plane {
    /// Step 4's result, on the encoded copy.
    Encoded(Vec<[f32; 4]>),
    /// Step 5's result, linear.
    Linear(Vec<[f32; 4]>),
}

pub(crate) struct FloatToneStage {
    /// Step 1: Exposure.
    front: Vec<PrecisionAdjustment>,
    /// Step 2, ahead of the guide: Contrast.
    encoded_before: Vec<PrecisionAdjustment>,
    /// Step 2: Highlights and Shadows, reading the guide.
    shadows_highlights: Option<PrecisionAdjustment>,
    /// Step 2, after the guide: Whites and Blacks.
    encoded_after: Vec<PrecisionAdjustment>,
    /// Step 3: Saturation.
    back: Vec<PrecisionAdjustment>,
    presence: Option<Presence>,
    /// Step 4's detail: Noise Reduction and positive Sharpness.
    detail_encoded: Option<LocalDetail>,
    /// Step 5: negative Sharpness.
    detail_linear: Option<LocalDetail>,
    dimensions: (u32, u32),
    /// The largest preparation allowance of the descriptions, as the 16-bit
    /// stage holds it.
    preparation: u64,
    guide: Option<PrecisionGuide>,
    plane: Option<Plane>,
}

/// The first refusal a kernel's read met. A kernel's `read` answers a sample
/// for every index, so a step that refuses there answers the sample as read
/// and the refusal is raised once the kernel returns. (`prepare` has
/// evaluated every operation once, so none is expected.)
#[derive(Default)]
struct Refusal(RefCell<Option<String>>);

impl Refusal {
    fn answer(&self, value: Result<[f32; 4], String>, read: [f32; 4]) -> [f32; 4] {
        value.unwrap_or_else(|e| {
            self.0.borrow_mut().get_or_insert(e);
            read
        })
    }

    fn raise(&self) -> Result<(), String> {
        match self.0.borrow_mut().take() {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }
}

fn rgb(p: [f32; 4]) -> [f32; 3] {
    [p[0], p[1], p[2]]
}

fn with_rgb(p: [f32; 4], rgb: [f32; 3]) -> [f32; 4] {
    [rgb[0], rgb[1], rgb[2], p[3]]
}

impl FloatToneStage {
    /// The stage for `tone` under `profile`, or None when every control is 0.
    /// Refuses an invalid tone, and an operation whose float form refuses
    /// (each is evaluated once here, so the refusal names it before any
    /// pixel is read).
    pub(crate) fn prepare(
        tone: &LocalTone,
        profile: &IccProfile,
        dimensions: (u32, u32),
    ) -> Result<Option<Self>, String> {
        tone.validate()?;
        if tone.is_identity() {
            return Ok(None);
        }
        let detail = tone.detail();
        let mut stage = Self {
            front: Vec::new(),
            encoded_before: Vec::new(),
            shadows_highlights: None,
            encoded_after: Vec::new(),
            back: Vec::new(),
            presence: tone.presence(),
            detail_encoded: detail
                .map(|d| LocalDetail {
                    sharpness: d.sharpness.max(0.0),
                    noise_reduction: d.noise_reduction,
                })
                .filter(|d| !d.identity()),
            detail_linear: detail
                .map(|d| LocalDetail {
                    sharpness: d.sharpness.min(0.0),
                    noise_reduction: 0.0,
                })
                .filter(|d| !d.identity()),
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
                "exposure" | "vibrance" => {
                    operation.apply_linear([0.5; 3], (0, 0))?;
                }
                _ => {
                    operation.apply_encoded([0.5; 3], 0.5)?;
                }
            }
            match operation.operation() {
                "exposure" => stage.front.push(operation),
                "bcs" => stage.encoded_before.push(operation),
                "shadows_highlights" => stage.shadows_highlights = Some(operation),
                "vibrance" => stage.back.push(operation),
                _ => stage.encoded_after.push(operation),
            }
        }
        Ok(Some(stage))
    }

    /// Whether the stage builds a plane before it answers: the guide, step 4
    /// or step 5.
    pub(crate) fn reads_plane(&self) -> bool {
        self.shadows_highlights.is_some()
            || self.presence.is_some()
            || self.detail_encoded.is_some()
            || self.detail_linear.is_some()
    }

    /// The bytes this stage holds once built: its prepared operations, the
    /// preparation allowance, the guide's plan and the largest of the
    /// Presence plan (its result plane included), the encoded detail plan
    /// beside the n×16 Presence plane it reads, and the linear detail plan
    /// beside the n×16 encoded plane it reads, each kernel planned in the
    /// float range. Refuses work above `cap` before any allocation.
    pub(crate) fn plan(&self, cap: u64) -> Result<u64, String> {
        let (w, h) = self.dimensions;
        let plane = pixel_count(w, h)? as u64 * 16;
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
        let encoded = match &self.detail_encoded {
            Some(detail) => {
                let read = if self.presence.is_some() { plane } else { 0 };
                read.saturating_add(detail.plan(self.dimensions, cap, &PLAN_RANGE)?.0)
            }
            None => 0,
        };
        let linear = match &self.detail_linear {
            Some(detail) => {
                let read = if self.presence.is_some() || self.detail_encoded.is_some() {
                    plane
                } else {
                    0
                };
                read.saturating_add(detail.plan(self.dimensions, cap, &PLAN_RANGE)?.0)
            }
            None => 0,
        };
        Ok(total.saturating_add(presence.max(encoded).max(linear)))
    }

    /// Builds the guide over the Contrast output on the encoded values, then
    /// step 4's plane over the encoded result of steps 1–3, then step 5's
    /// over the decoded result, each from `read` (the source's linear
    /// samples) and each dropped once the next has read it.
    pub(crate) fn build(
        &mut self,
        read: &dyn Fn(usize) -> [f32; 4],
        encoding: &DisplayEncoding<'_>,
        cap: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        self.plan(cap)?;
        let (w, h) = self.dimensions;
        let n = pixel_count(w, h)?;
        let refusal = Refusal::default();
        if let Some(operation) = &self.shadows_highlights {
            let guide = operation.guide(
                w,
                h,
                &|i| {
                    let sample = read(i);
                    refusal.answer(self.contrasted(i, sample, encoding), sample)
                },
                job.range(0.0, 0.3),
            )?;
            refusal.raise()?;
            self.guide = guide;
        }
        job.report(0.3, 3)?;
        let stages = [
            self.presence.is_some(),
            self.detail_encoded.is_some(),
            self.detail_linear.is_some(),
        ];
        let count = stages.iter().filter(|s| **s).count().max(1) as f64;
        let mut next = 0.0;
        let mut share = || {
            let part = job.range(0.3 + 0.7 * next / count, 0.3 + 0.7 * (next + 1.0) / count);
            next += 1.0;
            part
        };
        let this = &*self;
        let encoded_read = |i: usize| {
            let sample = read(i);
            refusal.answer(
                this.pointwise(i, sample, encoding)
                    .map(|v| encoding.encode(v)),
                sample,
            )
        };
        let presence = match this.presence {
            Some(presence) => {
                let part = share();
                let visible = visible_range((0..n).map(encoded_read), part.range(0.0, 0.05))?;
                refusal.raise()?;
                let plane = presence.plane_in(
                    encoded_read,
                    this.dimensions,
                    cap,
                    &SampleRange::Extended { visible },
                    part.range(0.05, 1.0),
                )?;
                refusal.raise()?;
                Some(plane)
            }
            None => None,
        };
        let encoded = match (this.detail_encoded, presence) {
            (Some(detail), presence) => {
                let part = share();
                let source = |i: usize| match &presence {
                    Some(plane) => plane[i],
                    None => encoded_read(i),
                };
                let visible = visible_range((0..n).map(source), part.range(0.0, 0.05))?;
                refusal.raise()?;
                let plane = detail.plane_in(
                    source,
                    this.dimensions,
                    cap,
                    &SampleRange::Extended { visible },
                    part.range(0.05, 1.0),
                )?;
                refusal.raise()?;
                Some(plane)
            }
            (None, presence) => presence,
        };
        let plane = match (this.detail_linear, encoded) {
            (Some(detail), encoded) => {
                let part = share();
                let source = |i: usize| {
                    let sample = read(i);
                    let linear = refusal.answer(this.pointwise(i, sample, encoding), sample);
                    match &encoded {
                        Some(plane) if linear[3] > 0.0 => {
                            encoding.decode_changed(linear, encoding.encode(linear), plane[i])
                        }
                        _ => linear,
                    }
                };
                let visible = visible_range((0..n).map(source), part.range(0.0, 0.05))?;
                refusal.raise()?;
                let plane = detail.plane_in(
                    source,
                    this.dimensions,
                    cap,
                    &SampleRange::Extended { visible },
                    part.range(0.05, 1.0),
                )?;
                refusal.raise()?;
                Some(Plane::Linear(plane))
            }
            (None, encoded) => encoded.map(Plane::Encoded),
        };
        self.plane = plane;
        job.report(1.0, 3)
    }

    /// The adjusted linear sample at `index` whose source sample is
    /// `sample`: the built plane's value (decoded from the encoded copy where
    /// step 4 ran last), else steps 1–3.
    pub(crate) fn value(
        &self,
        index: usize,
        sample: [f32; 4],
        encoding: &DisplayEncoding<'_>,
    ) -> Result<[f32; 4], String> {
        match &self.plane {
            Some(Plane::Linear(plane)) => Ok(plane[index]),
            Some(Plane::Encoded(plane)) => {
                let linear = self.pointwise(index, sample, encoding)?;
                if linear[3] <= 0.0 {
                    return Ok(linear);
                }
                Ok(encoding.decode_changed(linear, encoding.encode(linear), plane[index]))
            }
            None => self.pointwise(index, sample, encoding),
        }
    }

    fn operations(&self) -> impl Iterator<Item = &PrecisionAdjustment> {
        self.front
            .iter()
            .chain(&self.encoded_before)
            .chain(&self.shadows_highlights)
            .chain(&self.encoded_after)
            .chain(&self.back)
    }

    fn position(&self, index: usize) -> (i64, i64) {
        let w = self.dimensions.0 as usize;
        ((index % w) as i64, (index / w) as i64)
    }

    /// Step 1 of `sample`, encoded, through the Contrast ahead of the guide:
    /// what the Highlights/Shadows guide is built from, as at 16 bits.
    fn contrasted(
        &self,
        index: usize,
        sample: [f32; 4],
        encoding: &DisplayEncoding<'_>,
    ) -> Result<[f32; 4], String> {
        if sample[3] <= 0.0 {
            return Ok(encoding.encode(sample));
        }
        let xy = self.position(index);
        let mut linear = rgb(sample);
        for operation in &self.front {
            linear = operation.apply_linear(linear, xy)?;
        }
        let mut code = encoding.encode(with_rgb(sample, linear));
        for operation in &self.encoded_before {
            code = with_rgb(code, operation.apply_encoded(rgb(code), 0.0)?);
        }
        Ok(code)
    }

    /// Steps 1–3 of one linear sample (module doc).
    fn pointwise(
        &self,
        index: usize,
        sample: [f32; 4],
        encoding: &DisplayEncoding<'_>,
    ) -> Result<[f32; 4], String> {
        if sample[3] <= 0.0 {
            return Ok(sample);
        }
        let xy = self.position(index);
        let mut linear = sample;
        for operation in &self.front {
            linear = with_rgb(linear, operation.apply_linear(rgb(linear), xy)?);
        }
        if !self.encoded_before.is_empty()
            || self.shadows_highlights.is_some()
            || !self.encoded_after.is_empty()
        {
            let code = encoding.encode(linear);
            let mut value = code;
            for operation in &self.encoded_before {
                value = with_rgb(value, operation.apply_encoded(rgb(value), 0.0)?);
            }
            if let Some(operation) = &self.shadows_highlights {
                let guide = self
                    .guide
                    .as_ref()
                    .map_or(0.0, |g| g.at(xy.0 as u32, xy.1 as u32));
                value = with_rgb(value, operation.apply_encoded(rgb(value), guide)?);
            }
            for operation in &self.encoded_after {
                value = with_rgb(value, operation.apply_encoded(rgb(value), 0.0)?);
            }
            linear = encoding.decode_changed(linear, code, value);
        }
        for operation in &self.back {
            linear = with_rgb(linear, operation.apply_linear(rgb(linear), xy)?);
        }
        Ok(linear)
    }
}
