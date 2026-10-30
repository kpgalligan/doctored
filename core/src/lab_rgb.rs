//! A Lab Color document's pixels in RGB (#1 phase 1b): the conversion an
//! export runs before its RGB stages, and the Layers-panel thumbnail fitted
//! before it converts (the fit-first rule).
//!
//! Both are Mode ▸ RGB Color's loop (`lab_convert::converted_image` under a
//! `ColorPipeline` from the Lab twin): every u16 pixel converted, clamped to
//! 0…1 and kept at full u16 range, alpha kept — an out-of-gamut colour
//! clips, never refuses (which is why neither runs `doc_export`'s
//! `convert16`, whose out-of-range check refuses). ColorSync never decodes
//! the twin (`lab_convert::lab_display`'s reasons).
//!
//! * **Export** ([`lab_to_rgb_image`]): the resampled Lab composite into the
//!   request's destination (`ExportRequest::destination_for`: sRGB for the
//!   document's own profile, which an RGB file cannot carry), under the
//!   request's intent and black point compensation.
//! * **Thumbnail** ([`lab_thumbnail_rgba8`]): `layer_thumbnail::fit_u16` at
//!   u16 first, then the view's conversion (the built-in Display P3,
//!   relative colorimetric, no BPC) over the fitted pixels only, quantized
//!   once without dither.

use std::sync::Arc;

use image::RgbaImage;

use crate::icc::IccProfile;
use crate::icc_intent::{ColorPipeline, RenderingIntent};
use crate::job_control::{DrJobControl, JobContext};
use crate::lab_convert::converted_image;
use crate::lab_document::lab_twin;
use crate::layer_thumbnail::{fit_u16, fit_u16_scratch, fitted_dims};
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES};
use crate::precision_quantize::PrecisionDither;

/// Bytes an exported pixel's Lab → RGB stage charges: its converted u16
/// output (8 B/px; the pipeline is prepared once). Held by
/// `scripts/lab-admission` `export-lab-png16` and `export-lab-jpeg8` to
/// ⌈1.25 × the measured peak⌉; a miss raises this term.
pub(crate) const CONVERT_BYTES_PER_PIXEL: u64 = 8;

/// Bytes a fitted thumbnail pixel charges after the fit: its Display P3
/// u16 view (8) and its quantized RGBA8 (4).
const THUMB_BYTES_PER_PIXEL: u64 = 12;

/// The full-range u16 code of a unit sample (Mode ▸ RGB Color's `wide`).
fn wide(unit: f64) -> u16 {
    (unit.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// `image`, its samples Lab twin codes, in `destination` under `intent` and
/// `bpc` (module doc). Admission belongs to the caller.
pub(crate) fn lab_to_rgb_image(
    image: &PrecisionImage,
    destination: &Arc<IccProfile>,
    intent: RenderingIntent,
    bpc: bool,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let pipeline = ColorPipeline::new(&lab_twin(), destination, intent, bpc)?;
    converted_image(image, &pipeline, destination, &wide, job)
}

/// The scratch of [`lab_to_rgb_image`]'s pipeline into `destination`.
pub(crate) fn lab_to_rgb_scratch(
    destination: &IccProfile,
    intent: RenderingIntent,
    bpc: bool,
) -> Result<u64, String> {
    Ok(ColorPipeline::new(&lab_twin(), destination, intent, bpc)?.scratch_bytes())
}

/// A Lab layer's Layers-panel thumbnail as Display P3 RGBA8 (module doc):
/// fitted by `layer_thumbnail::fitted_dims` (never upscaled) at u16, then
/// converted and quantized. Refuses an image that is not Lab or not u16
/// with `lab_display`'s sentences; admits bookkeeping, the fit's scratch
/// (`fit_u16_scratch`), 12 B a fitted pixel and the pipeline's tables
/// against `limit` beside the image's retained bytes and `other`, and
/// reserves them before allocating.
pub fn lab_thumbnail_rgba8(
    image: &PrecisionImage,
    max_side: u32,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<RgbaImage, String> {
    if image.profile().bytes() != lab_twin().bytes() {
        return Err("the Lab Color view takes a Lab Color image".into());
    }
    if image.storage() != PrecisionStorage::Rgba16Unorm {
        return Err("the Lab Color view takes a 16-bit image".into());
    }
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let view = IccProfile::display_p3();
    let pipeline = ColorPipeline::new(
        &lab_twin(),
        &view,
        RenderingIntent::RelativeColorimetric,
        false,
    )?;
    let (w, h) = image.dimensions();
    let (tw, th) = fitted_dims(w, h, max_side);
    let scratch = [
        BOOKKEEPING_BYTES,
        fit_u16_scratch((w, h), max_side)?,
        (u64::from(tw) * u64::from(th)).saturating_mul(THUMB_BYTES_PER_PIXEL),
        pipeline.scratch_bytes(),
    ]
    .iter()
    .try_fold(0u64, |sum, &term| sum.checked_add(term))
    .ok_or("thumbnail working-memory calculation overflow")?;
    admit(image.retained_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    let fitted = fit_u16(image, max_side, job.range(0.0, 0.8))?;
    let shown = converted_image(&fitted, &pipeline, &view, &wide, job.range(0.8, 0.95))?;
    shown.quantize_with_context(PrecisionDither::None, job.range(0.95, 1.0))
}
