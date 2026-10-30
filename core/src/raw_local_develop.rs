//! Mix genuine alternate developments before layer geometry, then apply the
//! profile-calibrated manual vignette. Sampling's shared pass-through gate owns
//! premultiplied interpolation; exposure and ICC forward encoders own color math.
//!
//! A local may have no camera variant: its tone controls
//! (`raw_local_tone`, evaluated by `raw_local_tone_apply`) adjust the
//! development as the earlier locals left it, and a local with a variant and
//! a tone adjusts the variant's samples. The legacy contract (every local
//! with a variant, no tone) runs exactly the code it always did.
//!
//! Exact zero/identity branches preserve samples. Plain source extraction shares u16
//! storage/profile. Independent numerical and native/refusal coverage lives in
//! `raw_local_numeric.rs` and `raw_local_native.rs`. Since 13B-3e-3b `develop` reads its variants
//! and base as `runs_rgba16` row-major behind `storage()` gates at HEAD's positions (passes 1 and 3
//! hoist `job.every` per run through `EVERY_INTERVAL`), binding no view; its output stays Seeded
//! while its whole reader `raw_source_geometry.rs:392` remains (13B-3e-4;
//! `tests/raw_develop_reads.rs`). Since 13B-3e-4a the output (`develop`'s
//! `PrecisionImage::from_rgba16_unseeded` call) is
//! UNSEEDED: its geometry reader copies ONE transient, and the develop preview's first call
//! materializes a view (`preview_view_rgba16`, kept exception (13B-3e-4c-1, owner 13A-latency);
//! `tests/host_intake_unseeded.rs`).
use crate::icc_transform::PrecisionToneEncoder;
use crate::job_control::{DrJobControl, JobContext, EVERY_INTERVAL};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{
    admit, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::raw_local_effective::{EffectiveMask, LocalSettings};
use crate::raw_local_limits::{MAX_RAW_LOCALS, MAX_RAW_LOCAL_VARIANTS};
use crate::raw_local_mask::{dimensions, RawLocalMask, PARSE_SCRATCH};
use crate::raw_local_tone_apply::ToneStage;
use crate::resources::Inventory;
use serde::Deserialize;

/// The settings array's byte bound. One local's object with a range and a
/// tone, every number written at full f64 precision, is under 1 KiB, so
/// sixteen are under 16 KiB; the bound is twice that (it was 8 KiB for four).
pub const MAX_SETTINGS_BYTES: usize = 32 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vignette {
    pub stops: f64,
    pub center: [f64; 2],
    pub falloff: f64,
    pub reference_size: Option<[u32; 2]>,
    pub reference_from_source: Option<[f64; 6]>,
}
impl Vignette {
    pub(crate) fn validate(&self, w: u32, h: u32) -> Result<(), String> {
        if self.reference_size.is_some() != self.reference_from_source.is_some() {
            return Err("manual vignette reference size and matrix must be paired".into());
        }
        let (reference_w, reference_h) = if let Some([rw, rh]) = self.reference_size {
            crate::raw_local_mask::dimensions(rw, rh)?;
            let matrix = self
                .reference_from_source
                .ok_or("missing manual vignette matrix")?;
            if !matrix.iter().all(|v| v.is_finite())
                || matrix[..4].iter().any(|v| v.abs() > 1e6)
                || matrix[4..].iter().any(|v| v.abs() > 1e9)
                || (matrix[0] * matrix[3] - matrix[1] * matrix[2]).abs() < 1e-12
            {
                return Err("invalid manual vignette reference matrix".into());
            }
            for x in [0.0, f64::from(w)] {
                for y in [0.0, f64::from(h)] {
                    let p = [
                        matrix[0] * x + matrix[2] * y + matrix[4],
                        matrix[1] * x + matrix[3] * y + matrix[5],
                    ];
                    if p.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
                        return Err("manual vignette reference exceeds coordinate bounds".into());
                    }
                }
            }
            (rw, rh)
        } else {
            (w, h)
        };
        if !self.stops.is_finite()
            || !(-4.0..=4.0).contains(&self.stops)
            || !self.falloff.is_finite()
            || !(0.25..=4.0).contains(&self.falloff)
            || !self.center.iter().all(|v| v.is_finite())
            || !(0.0..=reference_w as f64).contains(&self.center[0])
            || !(0.0..=reference_h as f64).contains(&self.center[1])
        {
            return Err("invalid manual vignette strength, center or falloff".into());
        }
        Ok(())
    }
    pub(crate) fn radius(&self, w: u32, h: u32) -> f64 {
        crate::lens_model::vignette_radius(self.center, w, h)
    }
}
pub(crate) fn plain_image(document: &PrecisionDocument) -> Result<&PrecisionImage, String> {
    if document.layer_count() != 1 {
        return Err("local development requires one plain full-source raster".into());
    }
    let layer = document
        .layer(0)
        .ok_or("local development source is empty")?;
    if !layer.visible
        || layer.opacity != 1.0
        || layer.mask.is_some()
        || layer.description.is_some()
        || layer.retained_source.is_some()
    {
        return Err(
            "local development source must not contain layer edits or retained bindings".into(),
        );
    }
    match &layer.content {
        Content::Raster { image, offset }
            if *offset == (0, 0) && image.dimensions() == document.dimensions() =>
        {
            Ok(image)
        }
        _ => Err("local development requires one full-source raster at the origin".into()),
    }
}
/// The tone refusal at the float depth (`raw_float_develop`): the tone
/// controls evaluate through the display-referred adjustment and Presence
/// kernels, which have no rule for scene-linear samples above 1.0, so a tone
/// local on a float base refuses by name rather than reading it through the
/// 16-bit path. The host disables the controls there and the MCP raw tools
/// refuse with this same sentence.
pub(crate) const FLOAT_TONE_REFUSAL: &str =
    "RAW local tone controls are not available for a 32-bit float development.";

/// Mix the locals over the base development, then the manual vignette.
///
/// `variants[i]` is local `i`'s camera variant, or None for a local whose
/// only controls are its `tone` (`raw_local_tone`): such a local reads the
/// development as the earlier locals left it, so overlapping tone locals
/// compound, and with an identity tone it is skipped. A local with a variant
/// and a tone applies the tone to the variant's samples. Every local blends
/// through its coverage times its range (`blend::gate_source_into`). At most
/// [`MAX_RAW_LOCALS`] locals, [`MAX_RAW_LOCAL_VARIANTS`] of them with a
/// variant, and 32 KiB of settings.
#[allow(clippy::too_many_arguments)]
pub fn develop(
    base: &PrecisionDocument,
    variants: &[Option<&PrecisionDocument>],
    masks: &[&RawLocalMask],
    settings_text: &str,
    vignette_text: Option<&str>,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    job.check()?;
    if variants.len() > MAX_RAW_LOCALS
        || variants.iter().flatten().count() > MAX_RAW_LOCAL_VARIANTS
        || masks.len() != variants.len()
        || settings_text.len() > MAX_SETTINGS_BYTES
        || vignette_text.is_some_and(|s| s.len() > 4096)
    {
        return Err("local development input count or settings exceed the supported bounds".into());
    }
    let original = plain_image(base)?;
    // A 32-bit float development blends in f32 with no clamp and no
    // quantization (`raw_float_develop`, whose order this function masters).
    if original.storage() == PrecisionStorage::Rgba32Float {
        return crate::raw_float_develop::develop(
            base,
            variants,
            masks,
            settings_text,
            vignette_text,
            limit,
            other,
            job,
        );
    }
    let (w, h) = original.dimensions();
    let n = dimensions(w, h)?;
    let mut inventory = Inventory::default();
    base.record_resources(&mut inventory);
    for v in variants.iter().flatten() {
        v.record_resources(&mut inventory);
    }
    for mask in masks {
        mask.record_resources(&mut inventory);
    }
    admit(inventory.unique_bytes(), PARSE_SCRATCH, other, limit)?;
    job.reserve(PARSE_SCRATCH)?;
    let settings: Vec<LocalSettings> = serde_json::from_str(settings_text)
        .map_err(|e| format!("invalid local blend settings: {e}"))?;
    if settings.len() != variants.len() {
        return Err("local blend settings differ from variant count".into());
    }
    let vignette: Option<Vignette> = vignette_text
        .map(|s| serde_json::from_str(s).map_err(|e| format!("invalid manual vignette: {e}")))
        .transpose()?;
    if let Some(v) = &vignette {
        v.validate(w, h)?;
    }
    let mut images = Vec::with_capacity(variants.len());
    let mut feather = 0;
    for ((variant, mask), settings) in variants.iter().zip(masks).zip(&settings) {
        let image = variant.map(plain_image).transpose()?;
        if image.is_some_and(|image| {
            image.dimensions() != (w, h) || image.profile().bytes() != original.profile().bytes()
        }) || mask.dimensions() != (w, h)
        {
            return Err(
                "local variants, coverage and profile must match the full global source".into(),
            );
        }
        let plan = settings.plan(mask)?;
        if settings.enabled && settings.range.is_some() && original.profile().model().is_none() {
            return Err(crate::adjust_calibrated::PROFILE_REFUSAL.into());
        }
        feather = feather.max(plan.0);
        if image.is_some_and(|image| image.storage() != PrecisionStorage::Rgba16Unorm) {
            return Err("local variant requires genuine RGBA16 samples".into());
        }
        images.push(image);
    }
    let vignette = vignette.filter(|v| v.stops != 0.0);
    if vignette.is_none() && settings.iter().all(|s| !s.enabled || s.density == 0.0) {
        job.report(1.0, 5)?;
        return Ok(None);
    }
    // One tone stage is live at a time (prepared, built, blended, dropped),
    // so the plan holds the largest. Each is prepared here once to validate
    // and plan it, inside PARSE_SCRATCH, and again in its turn below.
    let cap = crate::sampling_gaussian::destructive_work_cap(job);
    let mut tone_scratch = 0;
    for settings in settings.iter().filter(|s| s.enabled && s.density != 0.0) {
        if let Some(tone) = &settings.tone {
            if let Some(stage) = ToneStage::prepare(tone, original.profile(), (w, h))? {
                tone_scratch = tone_scratch.max(stage.plan(cap)?);
            }
        }
    }
    let scratch = PARSE_SCRATCH
        + 24 * n as u64
        + feather
        + tone_scratch
        + 4 * BOOKKEEPING_BYTES
        + original.profile().bytes().len() as u64 * 4;
    admit(inventory.unique_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    if original.storage() != PrecisionStorage::Rgba16Unorm {
        return Err("local base requires genuine RGBA16 samples".into());
    }
    let mut accumulated = reserved_vec(n)?;
    // Row-major runs. `job.every(i)` is hoisted to each run: one check per
    // multiple of `EVERY_INTERVAL` in the run's index range (nothing
    // fallible sits between two checks), so the checkpoints are the flat
    // walk's.
    let mut i = 0;
    for run in original
        .runs_rgba16()
        .ok_or("local base requires genuine RGBA16 samples")?
    {
        let end = i + run.len();
        let mut at = i.next_multiple_of(EVERY_INTERVAL);
        while at < end {
            job.check()?;
            at += EVERY_INTERVAL;
        }
        i = end;
        accumulated.extend(run.iter().map(|p| p.map(|c| f32::from(c) / 65535.0)));
    }
    let locals = images.len();
    for (li, ((image, mask), settings)) in images.iter().zip(masks).zip(&settings).enumerate() {
        if !settings.enabled || settings.density == 0.0 {
            continue;
        }
        let mut stage = match &settings.tone {
            Some(tone) => ToneStage::prepare(tone, original.profile(), (w, h))?,
            None => None,
        };
        // A tone-only local whose tone is the identity changes nothing.
        if image.is_none() && stage.is_none() {
            continue;
        }
        let share = |at: f64| (li as f64 + at) / (locals.max(1) + 1) as f64 * 0.8;
        // Only a stage that reads a neighbourhood has planes to build; a
        // pointwise one is evaluated per sample below.
        if let (Some(stage), Some(true)) = (&mut stage, settings.tone.map(|t| t.reads_plane())) {
            let build = job.range(share(0.0), share(0.5));
            match image {
                Some(image) => stage.build(&|i| image.pixel(i), cap, build)?,
                None => stage.build(&|i| accumulated[i], cap, build)?,
            }
        }
        let effective = EffectiveMask::prepare(mask, *settings, job)?;
        // A range reads the BASE's samples (`raw_local_range.rs`), walked
        // row-major beside the variant's; no plane is built for it.
        let mut range = match &settings.range {
            Some(range) => Some((
                range,
                original
                    .profile()
                    .model()
                    .ok_or(crate::adjust_calibrated::PROFILE_REFUSAL)?,
                original
                    .runs_rgba16()
                    .ok_or("local base requires genuine RGBA16 samples")?
                    .flatten(),
            )),
            None => None,
        };
        // The variant's samples row-major beside the index, as the base's
        // for a range; a tone-only local reads `accumulated` itself.
        let mut variant = match image {
            Some(image) => Some(
                image
                    .runs_rgba16()
                    .ok_or("local variant requires genuine RGBA16 samples")?
                    .flatten(),
            ),
            None => None,
        };
        for (i, out) in accumulated.iter_mut().enumerate() {
            job.every(i)?;
            let variant_sample = match variant.as_mut() {
                Some(samples) => Some(*samples.next().ok_or("local variant is short")?),
                None => None,
            };
            let base_sample = range.as_mut().and_then(|r| r.2.next());
            let mut weight = effective.at(i)?;
            if weight == 0.0 {
                continue;
            }
            if let Some((range, model, _)) = &range {
                let s = base_sample.ok_or("local base differs from its variant")?;
                // The product is taken after `EffectiveMask::at`: a small
                // smoothstep value is a legitimate weight, not an alpha
                // below `checked_alpha`'s arithmetic floor.
                weight *= range.coverage(crate::raw_local_range::lab(model, [s[0], s[1], s[2]]));
                if weight == 0.0 {
                    continue;
                }
            }
            let source = match variant_sample {
                Some(p) => p.map(|c| f32::from(c) / 65535.0),
                None => *out,
            };
            let value = match &stage {
                Some(stage) => stage.value(i, source),
                None => source,
            };
            if value != *out {
                crate::blend::gate_source_into(out, value, weight);
            }
        }
        drop(stage);
        job.report(share(1.0), 3)?;
    }
    if let Some(v) = vignette {
        let model = original
            .profile()
            .model()
            .ok_or(crate::adjust_calibrated::PROFILE_REFUSAL)?;
        let encoder = PrecisionToneEncoder::new(model)?;
        let radius = if let Some([rw, rh]) = v.reference_size {
            v.radius(rw, rh)
        } else {
            v.radius(w, h)
        };
        for (i, p) in accumulated.iter_mut().enumerate() {
            job.every(i)?;
            let (source_x, source_y) = ((i as u32 % w) as f64 + 0.5, (i as u32 / w) as f64 + 0.5);
            let (x, y) = if let Some(b) = v.reference_from_source {
                (
                    b[0] * source_x + b[2] * source_y + b[4],
                    b[1] * source_x + b[3] * source_y + b[5],
                )
            } else {
                (source_x, source_y)
            };
            let distance = x - v.center[0];
            let distance = distance.hypot(y - v.center[1]);
            if radius == 0.0 || distance == 0.0 {
                continue;
            }
            let fraction = if v.reference_size.is_some() {
                (distance / radius).min(1.0)
            } else {
                distance / radius
            };
            let stops = crate::lens_model::vignette_stops(v.stops, fraction, v.falloff) as f32;
            if stops == 0.0 {
                continue;
            }
            // Uniform exposure commutes with the calibrated reference-basis
            // matrices. Reuse its gain/linear kernel in the source basis,
            // avoiding a redundant round trip through an intermediate gamut.
            let exposure = crate::adjust_tone::Exposure::gain_only(stops);
            let rgb = encoder.rgb(exposure.apply_linear(model.decode_rgb([p[0], p[1], p[2]])));
            p[..3].copy_from_slice(&rgb);
        }
    }
    let mut result = reserved_vec(n)?;
    let mut changed = false;
    // The base's runs row-major beside `accumulated[i]`, `job.every(i)`
    // hoisted to each run as in the first pass.
    let mut i = 0;
    for run in original
        .runs_rgba16()
        .ok_or("local base requires genuine RGBA16 samples")?
    {
        let end = i + run.len();
        let mut at = i.next_multiple_of(EVERY_INTERVAL);
        while at < end {
            job.check()?;
            at += EVERY_INTERVAL;
        }
        for (s, p) in run.iter().zip(&accumulated[i..end]) {
            let value = p.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16);
            changed |= value != *s;
            result.push(value);
        }
        i = end;
    }
    drop(accumulated);
    job.report(1.0, 5)?;
    if !changed {
        return Ok(None);
    }
    let image =
        PrecisionImage::from_rgba16_unseeded(w, h, result, original.profile().clone(), None)?;
    PrecisionDocument::from_image(
        image,
        crate::metadata::Metadata::default(),
        crate::metadata::Resolution::default(),
    )
    .map(Some)
}

/// The colour a range's Sample Colour click reads from a base development, as
/// the L*a*b* a colour range stores: the mean of `raw_local_range::lab` (each
/// pixel's L* already clamped to 0…100, so the reported L always validates)
/// over the `SAMPLE_WINDOW` square about the pixel holding `(x, y)`, clipped
/// to the frame. `x` is in 0…width and `y` in 0…height, in source pixels; a
/// point on the right or bottom edge reads the last column or row. Refuses a
/// point off the frame, a base that is not one plain RGBA16 raster, and a
/// profile with no matrix/TRC model. An f32 base (a 32-bit float development)
/// is read as linear light through `raw_local_range::lab_linear`.
pub fn raw_range_sample(base: &PrecisionDocument, x: f64, y: f64) -> Result<[f64; 3], String> {
    let image = plain_image(base)?;
    let (w, h) = image.dimensions();
    if !x.is_finite()
        || !y.is_finite()
        || !(0.0..=f64::from(w)).contains(&x)
        || !(0.0..=f64::from(h)).contains(&y)
        || w == 0
        || h == 0
    {
        return Err("local range sample point is outside the source".into());
    }
    let model = image
        .profile()
        .model()
        .ok_or(crate::adjust_calibrated::PROFILE_REFUSAL)?;
    let (cx, cy) = ((x as u32).min(w - 1), (y as u32).min(h - 1));
    // An f32 base is linear light: the same window through `lab_linear`.
    if image.storage() == PrecisionStorage::Rgba32Float {
        return crate::raw_float_develop::range_sample(image, model, (cx, cy));
    }
    // Every in-frame `rgba16_at` of a u16 image answers, so the window's
    // reads cannot fail once the storage is known.
    if image.rgba16_at(cx, cy).is_none() {
        return Err("local base requires genuine RGBA16 samples".into());
    }
    Ok(crate::raw_local_range::window_mean(
        model,
        (w, h),
        (cx, cy),
        |px, py| {
            image
                .rgba16_at(px, py)
                .map_or([0; 3], |s| [s[0], s[1], s[2]])
        },
    ))
}
