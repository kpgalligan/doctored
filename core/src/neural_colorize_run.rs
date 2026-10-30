//! The adapter of Filter ▸ Neural Filters ▸ Colorize (M4 phase 2) over
//! `neural_colorize`'s kernel, at 8 bits (`RgbaImage`) and 16
//! (`PrecisionImage`, RGBA16 only): the document profile's model and
//! encoder, the depth checks, the plan, admission and job control.
//!
//! The host runs two calls around its model: `input_*` writes the model's
//! `3·side²` gray input from the layer, and `apply_*` adjusts the model's
//! `2·side²` chroma and reconstructs the layer at full resolution. The
//! profile is the DOCUMENT's (its bytes): at 8 bits `IccProfile::model` with
//! the byte `ToneEncoder`, at 16 the validated `precision_model` with the
//! `PrecisionToneEncoder`; anything else refuses with `PROFILE_REFUSAL`. A
//! float image refuses by name. `apply_*` answers `None` when the output
//! equals the input (a neutral prediction at saturation 0, say).
//!
//! **The plan** is the larger of the two phases plus `BOOKKEEPING_BYTES`.
//! Input: `resize_into`'s `width × side` f32 intermediate, the `side²` L*
//! cells, and at 16 bits the n×8 transient copy of the source codes. Apply:
//! the `side²` binary64 a*b* plane and its blur scratch (2 × side² × 16), the
//! output codes (n × pixel), and at 16 bits the transient source codes, the
//! tiled copy the output image makes of its codes (n×8 each) and the
//! precision encoder's tables. The caller's buffers and the input image are
//! the caller's own.
use image::RgbaImage;

use crate::adjust_calibrated::{precision_model, Encoder};
use crate::icc::IccProfile;
use crate::icc_transform::{MatrixTrc, PrecisionToneEncoder, ToneEncoder};
use crate::job_control::{DrJobControl, JobContext, JobRun};
use crate::neural_colorize::{adjusted_plane, check_side, input, reconstruct, Adjust};
use crate::precision_image::{
    admit, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};

const NOT_FLOAT: &str = "neural filters are not available in a 32-bit document";
const PROFILE_REFUSAL: &str =
    "colorize needs an RGB matrix/TRC document profile; convert or assign a supported profile first";

fn begin<'a>(
    control: Option<&'a DrJobControl>,
) -> Result<(Option<JobRun<'a>>, JobContext<'a>), String> {
    let run = control.map(DrJobControl::begin).transpose()?;
    Ok((run, control.map_or(JobContext::NONE, JobContext::new)))
}

/// Colorize's working set over a `width × height` layer at `bytes_per_sample`
/// 1 or 2 (module doc).
pub(crate) fn plan(
    width: u32,
    height: u32,
    side: u32,
    bytes_per_sample: u32,
) -> Result<u64, String> {
    check_side(side)?;
    if !matches!(bytes_per_sample, 1 | 2) {
        return Err("bytes_per_sample must be 1 or 2".into());
    }
    if width == 0 || height == 0 {
        return Err("colorize needs a non-empty image".into());
    }
    let wide = bytes_per_sample == 2;
    let n = u64::from(width) * u64::from(height);
    let cells = u64::from(side) * u64::from(side);
    let codes = if wide { n * 8 } else { 0 };
    let input = u64::from(width) * u64::from(side) * 4 + cells * 4 + codes;
    let encoder = if wide {
        PrecisionToneEncoder::scratch_bytes()
    } else {
        0
    };
    let apply = 2 * cells * 16 + n * 4 * u64::from(bytes_per_sample) + 2 * codes + encoder;
    input
        .max(apply)
        .checked_add(BOOKKEEPING_BYTES)
        .ok_or_else(|| "colorize plan overflow".to_string())
}

/// The document profile's model and encoder (module doc).
fn profile_model(bytes: &[u8], wide: bool) -> Result<(MatrixTrc, Encoder), String> {
    let profile = IccProfile::parse(bytes).ok_or(PROFILE_REFUSAL)?;
    if wide {
        let model = precision_model(&profile).map_err(|_| PROFILE_REFUSAL.to_string())?;
        let encoder = PrecisionToneEncoder::new(model).map_err(|_| PROFILE_REFUSAL.to_string())?;
        Ok((model.clone(), Encoder::Precision(encoder)))
    } else {
        let model = profile.model().ok_or(PROFILE_REFUSAL)?;
        Ok((model.clone(), Encoder::Byte(ToneEncoder::new(model))))
    }
}

fn input_len(side: u32, out: &[f32]) -> Result<(), String> {
    check_side(side)?;
    if out.len() != 3 * side as usize * side as usize {
        return Err("the colorize input must hold 3 × side² samples".into());
    }
    Ok(())
}

fn chroma_len(side: u32, chroma: &[f32]) -> Result<(), String> {
    check_side(side)?;
    if chroma.len() != 2 * side as usize * side as usize {
        return Err("the colorize chroma must hold 2 × side² samples".into());
    }
    Ok(())
}

/// The model's input from an 8-bit layer.
pub(crate) fn input_bytes(
    image: &RgbaImage,
    profile: &[u8],
    side: u32,
    out: &mut [f32],
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<(), String> {
    input_len(side, out)?;
    let (width, height) = image.dimensions();
    let plan = plan(width, height, side, 1)?;
    let (model, _) = profile_model(profile, false)?;
    let (_run, job) = begin(control)?;
    admit(image.as_raw().len() as u64, plan, other, limit)?;
    job.reserve(plan)?;
    let job = job.with_reservation_floor(plan);
    input(
        image.as_raw().as_chunks::<4>().0,
        (width, height),
        &model,
        side,
        out,
        job,
    )
}

/// The model's input from a 16-bit layer.
pub(crate) fn input_wide(
    image: &PrecisionImage,
    profile: &[u8],
    side: u32,
    out: &mut [f32],
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<(), String> {
    if image.storage() == PrecisionStorage::Rgba32Float {
        return Err(NOT_FLOAT.into());
    }
    input_len(side, out)?;
    let (width, height) = image.dimensions();
    let plan = plan(width, height, side, 2)?;
    let (model, _) = profile_model(profile, true)?;
    let (_run, job) = begin(control)?;
    admit(image.retained_bytes(), plan, other, limit)?;
    job.reserve(plan)?;
    let job = job.with_reservation_floor(plan);
    let codes = image.transient_rgba16()?;
    input(&codes, (width, height), &model, side, out, job)
}

/// Colorize applied to an 8-bit layer, or `None` when nothing changes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_bytes(
    image: &RgbaImage,
    profile: &[u8],
    side: u32,
    chroma: &[f32],
    adjust: &str,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<RgbaImage>, String> {
    chroma_len(side, chroma)?;
    let adjust = Adjust::parse(adjust)?;
    let (width, height) = image.dimensions();
    let plan = plan(width, height, side, 1)?;
    let (model, encoder) = profile_model(profile, false)?;
    let (_run, job) = begin(control)?;
    admit(image.as_raw().len() as u64, plan, other, limit)?;
    job.reserve(plan)?;
    let job = job.with_reservation_floor(plan);
    let plane = adjusted_plane(chroma, side, &adjust)?;
    job.check()?;
    let n = image.as_raw().len();
    let mut bytes: Vec<u8> = reserved_vec(n)?;
    bytes.resize(n, 0);
    reconstruct(
        image.as_raw().as_chunks::<4>().0,
        (width, height),
        &model,
        &encoder,
        &plane,
        side,
        bytes.as_chunks_mut::<4>().0,
        job.range(0.0, 0.95),
    )?;
    drop(plane);
    if bytes.as_slice() == image.as_raw().as_slice() {
        return Ok(None);
    }
    let out =
        RgbaImage::from_raw(width, height, bytes).ok_or("colorize produced invalid dimensions")?;
    job.report(1.0, 6)?;
    Ok(Some(out))
}

/// Colorize applied to a 16-bit layer, or `None` when nothing changes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_wide(
    image: &PrecisionImage,
    profile: &[u8],
    side: u32,
    chroma: &[f32],
    adjust: &str,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionImage>, String> {
    if image.storage() == PrecisionStorage::Rgba32Float {
        return Err(NOT_FLOAT.into());
    }
    chroma_len(side, chroma)?;
    let adjust = Adjust::parse(adjust)?;
    let (width, height) = image.dimensions();
    let plan = plan(width, height, side, 2)?;
    let (model, encoder) = profile_model(profile, true)?;
    let (_run, job) = begin(control)?;
    admit(image.retained_bytes(), plan, other, limit)?;
    job.reserve(plan)?;
    let job = job.with_reservation_floor(plan);
    let plane = adjusted_plane(chroma, side, &adjust)?;
    job.check()?;
    let codes = image.transient_rgba16()?;
    let mut out: Vec<[u16; 4]> = reserved_vec(codes.len())?;
    out.resize(codes.len(), [0; 4]);
    reconstruct(
        &codes,
        (width, height),
        &model,
        &encoder,
        &plane,
        side,
        &mut out,
        job.range(0.0, 0.9),
    )?;
    drop(plane);
    if out.as_slice() == &*codes {
        return Ok(None);
    }
    drop(codes);
    let profile = image.profile().clone();
    let out = PrecisionImage::from_rgba16_unseeded(width, height, out, profile, None)?;
    job.report(1.0, 6)?;
    Ok(Some(out))
}
