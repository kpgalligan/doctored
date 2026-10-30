//! The 32-bit float arm of the RAW local development: the local blend, the
//! manual vignette and the range sample over a scene-linear f32 base.
//!
//! `raw_local_develop::develop` is the master copy of the order (validate,
//! admit, copy the base, blend each enabled local through
//! `blend::gate_source_into`, vignette, compare with the base) and of every
//! refusal sentence; it delegates here when the base is `Rgba32Float`. What
//! differs is only what the storage implies:
//!
//! - Nothing is clamped and nothing is quantized. A float development keeps
//!   the highlights the sensor recorded above white as values above 1.0, and
//!   a wide-gamut colour as a component below 0; the blend and the vignette
//!   carry both through. A pixel no local weighs keeps its base sample bit
//!   for bit, and a result bitwise equal to the base is `None`.
//! - A range reads the base through `raw_local_range::lab_linear`: the
//!   samples are already linear light in the profile's primaries, so no tone
//!   curve is evaluated.
//! - The vignette is `adjust_tone::Exposure::gain_only(stops).apply_linear`
//!   on the samples themselves. The u16 arm decodes through the profile's
//!   curve, applies the same gain and encodes again; here there is no curve
//!   on either side.
//! - The base needs f32 variants. A u16 variant beside a float base refuses:
//!   mixing them would need a decode rule, and the host develops every
//!   variant at the base's own encoding.
//! - A local's tone controls (`raw_local_tone`) refuse by name
//!   (`raw_local_develop::FLOAT_TONE_REFUSAL`): they evaluate through
//!   display-referred kernels with no rule for samples above 1.0. A local
//!   with no camera variant (NULL) therefore has nothing to apply and is
//!   skipped.
//!
//! Every result goes through `PrecisionImage::from_rgba32_extended`, which
//! refuses a non-finite sample or one past `FLOAT_RANGE_MAX` and never
//! repairs it, and lands in a one-layer float document under the Clip tone
//! map ([`float_document`]), the document the host's float intake builds.
use crate::float_document::ToneMap;
use crate::icc_transform::MatrixTrc;
use crate::job_control::{JobContext, EVERY_INTERVAL};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::raw_local_develop::{plain_image, Vignette, FLOAT_TONE_REFUSAL};
use crate::raw_local_effective::{EffectiveMask, LocalSettings};
use crate::raw_local_mask::{dimensions, RawLocalMask, PARSE_SCRATCH};
use crate::resources::Inventory;

const BASE: &str = "local base requires genuine RGBA32 float samples";
const VARIANT: &str = "local variant requires genuine RGBA32 float samples beside a float base";

/// One plain f32 raster as a one-layer float document named `name`, tagged
/// with the image's own profile, under the Clip tone map: what a float
/// development, its local blend and its geometry all answer with. Refuses a
/// u16 image (`raster_float`) and a profile with no matrix/TRC model
/// (`FloatPolicy::for_profile`).
pub(crate) fn float_document(
    image: PrecisionImage,
    name: &str,
) -> Result<PrecisionDocument, String> {
    let (width, height) = image.dimensions();
    let profile = image.profile().clone();
    let layer = PrecisionDocumentLayer::raster_float(name, image, (0, 0), 1.0, true)?;
    PrecisionDocument::from_parts_float(
        width,
        height,
        profile,
        crate::metadata::Metadata::default(),
        crate::metadata::Resolution::default(),
        vec![layer],
        Vec::new(),
        ToneMap::Clip,
    )
}

/// `raw_local_develop::develop` over an f32 base (module doc). The caller has
/// claimed the job and checked the input counts and text lengths.
#[allow(clippy::too_many_arguments)]
pub(crate) fn develop(
    base: &PrecisionDocument,
    variants: &[Option<&PrecisionDocument>],
    masks: &[&RawLocalMask],
    settings_text: &str,
    vignette_text: Option<&str>,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    let original = plain_image(base)?;
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
    if settings
        .iter()
        .any(|s| s.tone.is_some_and(|t| !t.is_identity()))
    {
        return Err(FLOAT_TONE_REFUSAL.into());
    }
    let vignette: Option<Vignette> = vignette_text
        .map(|s| serde_json::from_str(s).map_err(|e| format!("invalid manual vignette: {e}")))
        .transpose()?;
    if let Some(v) = &vignette {
        v.validate(w, h)?;
    }
    let model = original
        .profile()
        .model()
        .ok_or(crate::adjust_calibrated::PROFILE_REFUSAL)?;
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
        feather = feather.max(plan.0);
        if image.is_some_and(|image| image.storage() != PrecisionStorage::Rgba32Float) {
            return Err(VARIANT.into());
        }
        images.push(image);
    }
    let vignette = vignette.filter(|v| v.stops != 0.0);
    if vignette.is_none() && settings.iter().all(|s| !s.enabled || s.density == 0.0) {
        job.report(1.0, 5)?;
        return Ok(None);
    }
    // 32 bytes a pixel: the accumulated f32 plane, which is also the result
    // (nothing is quantized into a second buffer), and the tiles
    // `from_rgba32_extended` copies it into before the plane is dropped. The
    // u16 arm's 24 is its f32 plane beside an 8-byte result.
    let scratch = PARSE_SCRATCH
        + 32 * n as u64
        + feather
        + 4 * BOOKKEEPING_BYTES
        + original.profile().bytes().len() as u64 * 4;
    admit(inventory.unique_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    let mut accumulated = reserved_vec(n)?;
    let mut i = 0;
    for run in original.runs_rgba32().ok_or(BASE)? {
        let end = i + run.len();
        let mut at = i.next_multiple_of(EVERY_INTERVAL);
        while at < end {
            job.check()?;
            at += EVERY_INTERVAL;
        }
        i = end;
        accumulated.extend_from_slice(run);
    }
    for (li, ((image, mask), settings)) in images.iter().zip(masks).zip(&settings).enumerate() {
        // A local with no variant has only tone controls, refused above, so
        // it changes nothing here.
        let Some(image) = image else {
            continue;
        };
        if !settings.enabled || settings.density == 0.0 {
            continue;
        }
        let effective = EffectiveMask::prepare(mask, *settings, job)?;
        let mut range = match &settings.range {
            Some(range) => Some((range, original.runs_rgba32().ok_or(BASE)?.flatten())),
            None => None,
        };
        let mut next = 0;
        for run in image.runs_rgba32().ok_or(VARIANT)? {
            for value in run {
                let i = next;
                next += 1;
                job.every(i)?;
                let base_sample = range.as_mut().and_then(|r| r.1.next());
                let mut weight = effective.at(i)?;
                if weight == 0.0 {
                    continue;
                }
                if let Some((range, _)) = &range {
                    let s = base_sample.ok_or("local base differs from its variant")?;
                    weight *= range.coverage(crate::raw_local_range::lab_linear(
                        model,
                        [s[0], s[1], s[2]],
                    ));
                    if weight == 0.0 {
                        continue;
                    }
                }
                if *value != accumulated[i] {
                    crate::blend::gate_source_into(&mut accumulated[i], *value, weight);
                }
            }
        }
        job.report((li + 1) as f64 / (images.len().max(1) + 1) as f64 * 0.8, 3)?;
    }
    if let Some(v) = vignette {
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
            let distance = (x - v.center[0]).hypot(y - v.center[1]);
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
            // The samples are linear light already, so the gain applies to
            // them directly: the u16 arm's decode and encode have no
            // counterpart.
            let exposure = crate::adjust_tone::Exposure::gain_only(stops);
            let rgb = exposure.apply_linear([p[0], p[1], p[2]].map(f64::from));
            for (slot, value) in p.iter_mut().zip(rgb) {
                *slot = value as f32;
            }
        }
    }
    let mut changed = false;
    let mut i = 0;
    for run in original.runs_rgba32().ok_or(BASE)? {
        let end = i + run.len();
        let mut at = i.next_multiple_of(EVERY_INTERVAL);
        while at < end {
            job.check()?;
            at += EVERY_INTERVAL;
        }
        changed |= run
            .iter()
            .zip(&accumulated[i..end])
            .any(|(s, p)| s.map(f32::to_bits) != p.map(f32::to_bits));
        i = end;
    }
    job.report(1.0, 5)?;
    if !changed {
        return Ok(None);
    }
    let image =
        PrecisionImage::from_rgba32_extended(w, h, accumulated, original.profile().clone(), None)?;
    float_document(image, "Background").map(Some)
}

/// `raw_local_develop::raw_range_sample`'s window mean over an f32 base: the
/// same `SAMPLE_WINDOW` square about pixel `centre`, clipped to the frame,
/// each sample through `lab_linear`. The caller has validated the point and
/// found the model.
pub(crate) fn range_sample(
    image: &PrecisionImage,
    model: &MatrixTrc,
    centre: (u32, u32),
) -> Result<[f64; 3], String> {
    let (w, h) = image.dimensions();
    let (cx, cy) = centre;
    let reach = crate::raw_local_range::SAMPLE_WINDOW / 2;
    let mut sum = [0.0_f64; 3];
    let mut count = 0.0;
    for py in cy.saturating_sub(reach)..=(cy + reach).min(h - 1) {
        for px in cx.saturating_sub(reach)..=(cx + reach).min(w - 1) {
            let s = image.sample_at(px, py).ok_or(BASE)?;
            let lab = crate::raw_local_range::lab_linear(model, [s[0], s[1], s[2]]);
            for (slot, v) in sum.iter_mut().zip(lab) {
                *slot += f64::from(v);
            }
            count += 1.0;
        }
    }
    Ok(sum.map(|v| v / count))
}
