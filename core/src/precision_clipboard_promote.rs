//! The clipboard's PROMOTION into a 32-bit float document — the f32
//! arm of `ClipboardPayload::converted_with` (`precision_clipboard_convert`
//! holds the resolution order; this module is only the f32 kernel). The
//! kernel is [`promoted_image`], over one image, which the layers paste into
//! a float document calls per record too (`layer_clipboard_float`).
//! Pasted pixels are converted into the destination's depth, 32-bit
//! included, so a 16-bit (or byte-origin) payload pasted into a float
//! document lands as linear light in the document's profile.
//!
//! # The rule
//!
//! A float document stores LINEAR light in its profile's primaries
//! (`float_document`). For a u16 payload, each sample is decoded through
//! the ONE typed transfer, `TypedTransfer::for_profile(P).decode(code /
//! 65535)` (`precision_evaluation`), with P the PAYLOAD's profile under
//! Convert and the DESTINATION's under Preserve (Don't Convert keeps the
//! numbers: they are read as the destination's own encoding). Under Convert
//! with differing profiles the linear triple then goes through
//! `MatrixTrc::combined_with` (source → destination, `icc_transform`) in
//! f64, rounded once to f32. Alpha is `code as f32 / 65535`. An f32
//! payload in another profile under Convert takes the same matrix on its
//! linear RGB, alpha kept. Coverage, offset, resolution and the editable
//! section are carried; the byte-origin flag is cleared; the result carries
//! the f32 payload's tone map (an f32 source's own, `Clip` for a promoted
//! one — a paste reads only the samples).
//!
//! Every output component must be finite with `|v| ≤ FLOAT_RANGE_MAX`;
//! anything else refuses by name rather than being repaired (the float
//! storage rule). The conversion admits bookkeeping, `2 · payload_bytes` of
//! the f32 target and `TypedTransfer::table_bytes` before allocating
//! (`ClipboardPayload::conversion_bytes`).
//!
//! # The LUT arm
//!
//! [`lut_promoted_image`] is the same u16 → linear f32 conversion for a
//! source whose profile has no matrix/TRC model (A2B/B2A tables): each code
//! goes through `ColorPipeline::linear` (the profile's table into PCS XYZ,
//! then the destination's matrix, in f64), is rounded once to f32, unclamped,
//! and passes the same range check. Only a placed image's master reaches it
//! (`retained_source_float::source_float`, whose module doc holds the
//! Photoshop take). [`models`] still refuses a LUT profile, so a clipboard
//! payload or a pasted raster record in one refuses as before.
//!
//! # Admission margin
//!
//! [`PASTE_PROMOTE_MARGIN_PER_PIXEL`] × the destination canvas is added to
//! the paste's insertion admission whenever a conversion into float ran
//! (`PrecisionDocument::clipboard_paste`, `clipboard_paste_plan`,
//! `clipboard_paste_allocation`).
//!
//! # Measurement record (batch 174, 2026-10-05)
//!
//! Fixture and method as `precision_clipboard_host`'s record: a Release app
//! with `DR_WORKING_LIMIT_BYTES = 8589934592`, `MallocLargeCache=0`,
//! `DR_BACKGROUND=1`, private `DR_*_DIR`s and a private pasteboard, one
//! fresh instance per run; E = `ri_interval_max_phys_footprint` − the
//! footprint read right after `proc_reset_footprint_interval`, around ONE
//! MCP `paste` (centred). The float target is the float_11f `scene.tiff`
//! resized to 6000×4000 (`image_size`), the source a 6000×4000 16-bit
//! white document (`new_document`) copied whole (a 192 MB payload, inline):
//! case 1 sRGB (the paste's `profile` reply "same"), case 2 Display P3
//! ("converted"). A = [`PrecisionDocument::clipboard_paste_allocation`] of
//! the same paste (a one-layer 6000×4000 float document, request name
//! "Pasted Layer"): `max(conversion, insertion + margin + 2·converted)`,
//! the insertion side deciding (2·384,000,000 + 4,198,444 + margin).
//!
//! - Without a margin (K = 0): sRGB E = 1,155,515,232 / 1,155,924,832 /
//!   1,155,482,464; Display P3 E = 1,155,777,376 / 1,155,924,832 /
//!   1,156,891,464; the `list_documents` twin E = 0 every run. A =
//!   772,198,444 for both (A / max E = 0.667). Nearly all of E is the
//!   publication's display preparation of the new float projection, inside
//!   the same paste call (`doc_paste_layers`' record saw the same).
//! - Fitted: `⌈1.25 · max E⌉ − A` over the 24,000,000-pixel destination
//!   canvas is 28.08 B/px, so K = 29 (28 gives 1,444,198,444 <
//!   1,446,114,330).
//! - With K = 29, re-measured on a rebuilt app: sRGB E = 1,156,137,800 /
//!   1,155,711,816 / 1,155,334,960; Display P3 E = 1,155,892,016 /
//!   1,155,629,872 / 1,155,875,704; twin E = 0 / 0 / 0. A = 1,468,198,444
//!   (P = `clipboard_paste_plan` 1,472,283,027), A / max E = 1.270 (sRGB)
//!   and 1.270 (P3): ⌈1.25 × 1,156,137,800⌉ = 1,445,172,250 ≤ A. Both gates
//!   hold.
//!
//! an f32 target is the PROMOTE module (its kernel is `promoted_image`, over one image, shared with
//! the layers paste into float): a u16 code decoded by `TypedTransfer::decode` (the payload's TRC
//! under Convert, the destination's under Preserve), then under Convert `MatrixTrc::combined_with`
//! in f64 rounded once to f32, alpha `code/65535`; any output outside ±`FLOAT_RANGE_MAX` refuses by
//! name.

use std::sync::Arc;

use crate::float_document::{ToneMap, FLOAT_RANGE_MAX};
use crate::icc::IccProfile;
use crate::icc_intent::ColorPipeline;
use crate::icc_transform::MatrixTrc;
use crate::job_control::JobContext;
use crate::precision_clipboard::PasteProfile;
use crate::precision_clipboard_codec::ClipboardPayload;
use crate::precision_evaluation::TypedTransfer;
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage, PrecisionStorage};

/// The fitted margin a paste into a float document charges per CANVAS
/// pixel on top of the conversion and the insertion, whenever a conversion
/// into float ran. Why: the measured peak of a 24 MP 16-bit paste into a
/// 24 MP float document (the module doc's record) is above what the core's
/// own terms charge — most of it is the publication's display preparation
/// of the new float projection, inside the same paste call — and the house
/// rule asks the allocation to be at least 1.25× that peak; 29 is the
/// smallest integer that passes (28 does not).
pub const PASTE_PROMOTE_MARGIN_PER_PIXEL: u64 = 29;

/// `m · rgb` in f64, rounded once to f32.
fn apply(m: &[[f64; 3]; 3], rgb: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| (0..3).map(|k| m[i][k] * f64::from(rgb[k])).sum::<f64>() as f32)
}

/// The start of [`checked`]'s range refusal, which names the clipboard. The
/// callers that convert something else (`retained_source_float`'s placed
/// master, `float_color`'s Convert to Profile) match on it and answer their
/// own sentence.
pub(crate) const RANGE_REFUSAL_PREFIX: &str = "the clipboard's pixels convert to a value outside";

/// The one refusal of an out-of-range float output.
fn checked(pixel: [f32; 4]) -> Result<[f32; 4], String> {
    if pixel[..3]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= FLOAT_RANGE_MAX)
    {
        Ok(pixel)
    } else {
        Err(format!(
            "{RANGE_REFUSAL_PREFIX} the 32-bit float range (±{FLOAT_RANGE_MAX}); the paste is refused rather than clipped"
        ))
    }
}

/// The source and destination matrix/TRC models when the bytes differ,
/// `None` when they agree; refuses a profile with no model. The one
/// statement of the rule: `ClipboardPayload::profile_models` delegates here.
pub(crate) fn models<'a>(
    source: &'a Arc<IccProfile>,
    profile: &'a Arc<IccProfile>,
) -> Result<Option<(&'a MatrixTrc, &'a MatrixTrc)>, String> {
    if source.bytes() == profile.bytes() {
        return Ok(None);
    }
    let src = source
        .model()
        .ok_or("clipboard conversion requires an RGB matrix/TRC source profile")?;
    let dst = profile
        .model()
        .ok_or("clipboard conversion requires an RGB matrix/TRC destination profile")?;
    Ok(Some((src, dst)))
}

/// The module doc's rule over ONE image: `image` as f32 samples in `profile`
/// under `mode`. An f32 image under Preserve, or whose profile has
/// `profile`'s exact bytes, is `with_profile` (samples shared, nothing
/// allocated); every other image is a new f32 plane. The caller admits and
/// reserves the conversion.
pub(crate) fn promoted_image(
    image: &PrecisionImage,
    profile: &Arc<IccProfile>,
    mode: PasteProfile,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let source_profile = image.profile();
    let (w, h) = image.dimensions();
    let count = pixel_count(w, h)?;
    let matrix = match mode {
        PasteProfile::Convert => {
            models(source_profile, profile)?.map(|(src, dst)| src.combined_with(dst))
        }
        PasteProfile::Preserve => None,
    };
    let mut i = 0;
    if let Some(runs) = image.runs_rgba16() {
        let mut pixels: Vec<[f32; 4]> = reserved_vec(count)?;
        let decode_profile = match mode {
            PasteProfile::Convert => source_profile,
            PasteProfile::Preserve => profile,
        };
        let transfer = TypedTransfer::for_profile(decode_profile)?;
        for run in runs {
            for code in run {
                job.every(i)?;
                i += 1;
                let encoded = [code[0], code[1], code[2]].map(|v| f32::from(v) / 65535.0);
                let linear = transfer.decode(encoded);
                let rgb = match &matrix {
                    Some(m) => apply(m, linear),
                    None => linear,
                };
                pixels.push(checked([
                    rgb[0],
                    rgb[1],
                    rgb[2],
                    f32::from(code[3]) / 65535.0,
                ])?);
            }
        }
        PrecisionImage::from_rgba32_extended(w, h, pixels, profile.clone(), None)
    } else if let Some(runs) = image.runs_rgba32() {
        let Some(m) = matrix else {
            return Ok(image.with_profile(profile.clone()));
        };
        let mut pixels: Vec<[f32; 4]> = reserved_vec(count)?;
        for run in runs {
            for pixel in run {
                job.every(i)?;
                i += 1;
                let rgb = apply(&m, [pixel[0], pixel[1], pixel[2]]);
                pixels.push(checked([rgb[0], rgb[1], rgb[2], pixel[3]])?);
            }
        }
        PrecisionImage::from_rgba32_extended(w, h, pixels, profile.clone(), None)
    } else {
        Err("clipboard payload has no typed samples".into())
    }
}

/// An RGBA16 image in a non-matrix profile as f32 linear light in `profile`
/// through `pipeline` (`retained_source_render::lut_pipeline(image.profile(), profile)`),
/// each RGB component rounded once to f32 and checked by `checked`; alpha `code / 65535`.
/// Err for non-RGBA16 storage or a pipeline whose `linear` answers None.
pub(crate) fn lut_promoted_image(
    image: &PrecisionImage,
    pipeline: &ColorPipeline,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = image.dimensions();
    let count = pixel_count(w, h)?;
    let runs = image
        .runs_rgba16()
        .ok_or("a LUT-profile source converts into float from RGBA16 UNORM storage only")?;
    let mut pixels: Vec<[f32; 4]> = reserved_vec(count)?;
    let mut i = 0;
    for run in runs {
        for code in run {
            job.every(i)?;
            i += 1;
            let encoded = [code[0], code[1], code[2]].map(|v| f32::from(v) / 65535.0);
            let linear = pipeline.linear(encoded).ok_or(
                "a LUT-profile source converts into float only through a matrix/TRC destination profile",
            )?;
            pixels.push(checked([
                linear[0] as f32,
                linear[1] as f32,
                linear[2] as f32,
                f32::from(code[3]) / 65535.0,
            ])?);
        }
    }
    PrecisionImage::from_rgba32_extended(w, h, pixels, profile.clone(), None)
}

impl ClipboardPayload {
    /// The payload as f32 samples in `profile` under `mode` (module doc),
    /// through [`promoted_image`]. The caller (`converted_with_context`) has
    /// already admitted and reserved the conversion and ruled out a no-op.
    pub(crate) fn promoted(
        &self,
        profile: &Arc<IccProfile>,
        mode: PasteProfile,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let tone_map = match self.image.storage() {
            PrecisionStorage::Rgba32Float => self.tone_map.unwrap_or(ToneMap::Clip),
            PrecisionStorage::Rgba16Unorm => ToneMap::Clip,
        };
        let image = promoted_image(&self.image, profile, mode, job)?;
        job.report(1.0, 5)?;
        Ok(Self {
            image,
            offset: self.offset,
            resolution: self.resolution,
            coverage: self.coverage.clone(),
            byte_origin: false,
            tone_map: Some(tone_map),
            editable: self.editable.clone(),
        })
    }
}
