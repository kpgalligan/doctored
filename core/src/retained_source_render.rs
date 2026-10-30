//! Source evaluation adapters reuse the existing ICC transforms and sampling
//! kernels. Alpha/hidden RGB obey the native depth's existing conversion rules.
//!
//! `color8` walks its runs and answers `Colored8` — `Raster` (a plane that already lives somewhere:
//! the asset's) or `Owned` (a transient converted `RgbaImage`, tiled
//! `unseeded` only by the caller that stores it, never seeded). A side with no matrix/TRC model
//! converts through `lut_pipeline` (relative colorimetric, no BPC; `color8`, `color8_image`,
//! `color16` and `profile_cost`), charged `LUT_COLOR_WORK_FACTOR` times the matrix colour visit.
use crate::doc::LayerRaster;
use crate::doc_transform::{exact_form, resample_rgba_with_context, Affine, Exact, SourceMap};
use crate::icc::IccProfile;
use crate::icc_intent::{ColorPipeline, RenderingIntent};
use crate::icc_transform::{PrecisionTransform, Transform};
use crate::job_control::JobContext;
use crate::precision_image::{
    reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_sampling::{self, GeometryFilter};
use crate::retained_source::{RetainedSource16, RetainedSource8, SourceAsset, SourcePlan};
use crate::source_asset::SourcePixels;
use crate::source_transform::{self, SourceTransform};
use image::RgbaImage;
use std::sync::Arc;

/// One decision shared by source execution and its work estimate. Exact source
/// placement reuses the colored allocation; permutations use nearest sampling.
/// The requested interpolation kernel applies only to other accumulated maps.
#[derive(Clone, Copy)]
pub(crate) enum AffineSampling {
    Shared,
    Sample(GeometryFilter),
}
impl AffineSampling {
    pub(crate) fn new(
        source: (u32, u32),
        dest: (u32, u32),
        matrix: Affine,
        filter: GeometryFilter,
    ) -> Self {
        match exact_form(&matrix) {
            Some(Exact::Translate) if source == dest => Self::Shared,
            Some(_) => Self::Sample(GeometryFilter::Nearest),
            None => Self::Sample(filter),
        }
    }
    fn rgba_work(self, pixels: u64) -> u64 {
        match self {
            Self::Shared => 0,
            // Four channels, kernel taps, initialization and final writeback.
            Self::Sample(filter) => pixels * 4 * (filter.taps().pow(2) + 2),
        }
    }
}

pub(crate) struct SourceSampling {
    rgba: AffineSampling,
    mask: GeometryFilter,
}
impl SourceSampling {
    pub(crate) fn new(
        source: (u32, u32),
        dest: (u32, u32),
        matrix: SourceTransform,
        filter: GeometryFilter,
    ) -> Self {
        Self {
            rgba: match matrix {
                SourceTransform::Affine(m) => {
                    AffineSampling::new(source, dest, Affine::from_array(m), filter)
                }
                SourceTransform::Projective(_) => AffineSampling::Sample(filter),
            },
            mask: filter,
        }
    }
}

/// Immutable source content.
pub(crate) struct SourceStage<'a> {
    pub asset: &'a SourceAsset,
}

/// [`work_plan_within`] with no work refusal (`CONTROLLED_JOB_WORK`):
/// a plan, or a render that runs under its caller's job control.
pub(crate) fn work_plan(
    retained: u64,
    stage: SourceStage<'_>,
    dest: (u32, u32),
    mask_pixels: u64,
    sampling: SourceSampling,
    fallback_depth: u32,
    color: SourceColorCost,
) -> Result<SourcePlan, String> {
    work_plan_within(
        crate::sampling_gaussian::CONTROLLED_JOB_WORK,
        retained,
        stage,
        dest,
        mask_pixels,
        sampling,
        fallback_depth,
        color,
    )
}

/// The source render's plan, refused above `work_limit`: `MAX_WORK_UNITS`
/// for a preview (`interactive`), its caller's cap for every other edit of a
/// placed image at either depth (`precision_geometry_budget`:
/// `destructive_work_cap(job)` for a run, `CONTROLLED_JOB_WORK`
/// for a plan), and `MAX_DESTRUCTIVE_WORK` for the float layers paste's
/// re-render, which has no job control.
/// Its scratch is the input, output and tiling planes of the affine sampling.
#[allow(clippy::too_many_arguments)]
pub(crate) fn work_plan_within(
    work_limit: u64,
    retained: u64,
    stage: SourceStage<'_>,
    dest: (u32, u32),
    mask_pixels: u64,
    sampling: SourceSampling,
    fallback_depth: u32,
    color: SourceColorCost,
) -> Result<SourcePlan, String> {
    let SourceStage { asset } = stage;
    let source = asset.dimensions();
    // Below 32 bits an f32 master renders through its u16 codes
    // ([`float_master_codes`]), so it is
    // charged as a 16-bit master plus the encode's own planes and visit.
    let float_low = asset.depth() == 32 && fallback_depth < 32;
    let master_depth = if float_low { 16 } else { asset.depth() };
    let depth = u64::from(master_depth.max(fallback_depth) / 8);
    let promote = master_depth == 8 && fallback_depth >= 16;
    let quantize = master_depth >= 16 && fallback_depth == 8;
    let n = u64::from(dest.0) * u64::from(dest.1);
    let s = u64::from(source.0) * u64::from(source.1);
    // A linked mask follows the incremental edit, independently of the master's
    // accumulated map. Undoing a filtered source scale can make RGBA exact while
    // the mask still needs Lanczos taps. Keep its requested-kernel charge even
    // when its own exact fast path could cost less. The full source color visit
    // remains conservative when matching profiles allow sharing too.
    let work = sampling.rgba.rgba_work(n)
        + s * color.work_per_pixel
        + mask_pixels * (sampling.mask.taps().pow(2) + 2)
        + if promote { s * 4 } else { 0 }
        + if quantize { n * 4 } else { 0 }
        + if float_low { s * 4 } else { 0 };
    if work > work_limit {
        return Err(
            "source transform exceeds the bounded sampling work limit; rasterize the layer or use a smaller size"
                .into(),
        );
    }
    // The SAMPLING: `(s + n)` the input and output planes, the second `n`
    // the TILING TRANSIENT (13B-3b-1) — the rendered plane's tiles beside
    // its contiguous render (`LayerRaster::new`/`unseeded` at 8 bits, the
    // typed constructors' `tile()` at 16), charged whether or not the arm
    // tiles, like the conservative source colour visit above.
    let sampling = (s + n) * 4 * depth + n * 4 * depth;
    Ok(SourcePlan {
        retained_bytes: retained,
        scratch_bytes: 4 * BOOKKEEPING_BYTES
            + sampling
            + mask_pixels * depth
            + if promote { s * 8 } else { 0 }
            + if quantize { n * 4 } else { 0 }
            + if float_low {
                s * FLOAT_ENCODE_BYTES_PER_PIXEL
            } else {
                0
            }
            + color.scratch_bytes,
        work_units: work,
    })
}
/// The scratch an f32 master's encode charges per SOURCE pixel below 32 bits:
/// take1's PP3 read `matrix_clip`, so
/// `float_master_codes` holds the master converted into the document profile
/// (an f32 plane, 16 bytes) beside its encoded u16 codes (8 bytes). The
/// `clip_matrix` row would have held the codes alone (8). Raised only on an
/// admission miss; `admission-600p3a` (`float_native`'s `# Measurement
/// record (phase 3a)`) passed at 24.
pub(crate) const FLOAT_ENCODE_BYTES_PER_PIXEL: u64 = 24;

/// A byte source after ICC conversion. `Raster`: a plane that already lives
/// somewhere (the asset's) — a caller storing it
/// mints a fresh identity. `Owned`: a transient converted plane; tiled
/// (`unseeded`) only by the caller that stores it, never seeded, so a
/// conversion nobody keeps leaves nothing in the view cache.
pub(crate) enum Colored8 {
    Raster(LayerRaster),
    Owned(RgbaImage),
}
impl Colored8 {
    pub(crate) fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Raster(raster) => raster.dimensions(),
            Self::Owned(image) => image.dimensions(),
        }
    }
    /// The plane as a stored raster: always a fresh identity, never a seed.
    pub(crate) fn into_raster(self) -> LayerRaster {
        match self {
            Self::Raster(raster) => raster.reidentified(),
            Self::Owned(image) => LayerRaster::unseeded(image),
        }
    }
}

/// The source's bytes in `to`: `Raster(source.clone())` when no conversion is
/// needed, else the converted plane `Owned`. Reads the tiles in place.
pub(crate) fn color8(
    source: &LayerRaster,
    from: &Arc<IccProfile>,
    to: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<Colored8, String> {
    if from.bytes() == to.bytes() {
        return Ok(Colored8::Raster(source.clone()));
    }
    if let Some(pipeline) = lut_pipeline(from, to)? {
        let (width, height) = source.dimensions();
        let bytes = convert8_runs(
            |rgb| pipeline.color8(rgb),
            source.runs(),
            width as usize * height as usize,
            job,
        )?;
        return RgbaImage::from_raw(width, height, bytes)
            .map(Colored8::Owned)
            .ok_or_else(|| "invalid converted source dimensions".into());
    }
    let src = from
        .model()
        .ok_or("source conversion requires an RGB matrix/TRC source profile")?;
    let dst = to
        .model()
        .ok_or("source conversion requires an RGB matrix/TRC destination profile")?;
    let Some(transform) = Transform::between(src, dst) else {
        return Ok(Colored8::Raster(source.clone()));
    };
    let (width, height) = source.dimensions();
    let bytes = convert8_runs(
        |rgb| transform.color(rgb),
        source.runs(),
        width as usize * height as usize,
        job,
    )?;
    RgbaImage::from_raw(width, height, bytes)
        .map(Colored8::Owned)
        .ok_or_else(|| "invalid converted source dimensions".into())
}

/// [`color8`]'s conversion over an owned image (the contact sheet).
pub(crate) fn color8_image(
    source: &Arc<RgbaImage>,
    from: &Arc<IccProfile>,
    to: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<Arc<RgbaImage>, String> {
    if from.bytes() == to.bytes() {
        return Ok(source.clone());
    }
    if let Some(pipeline) = lut_pipeline(from, to)? {
        let len = source.width() as usize * source.height() as usize;
        let bytes = convert8_runs(
            |rgb| pipeline.color8(rgb),
            std::iter::once(&source.as_raw()[..len * 4]).map(|raw| raw.as_chunks::<4>().0),
            len,
            job,
        )?;
        return RgbaImage::from_raw(source.width(), source.height(), bytes)
            .map(Arc::new)
            .ok_or_else(|| "invalid converted source dimensions".into());
    }
    let src = from
        .model()
        .ok_or("source conversion requires an RGB matrix/TRC source profile")?;
    let dst = to
        .model()
        .ok_or("source conversion requires an RGB matrix/TRC destination profile")?;
    let Some(transform) = Transform::between(src, dst) else {
        return Ok(source.clone());
    };
    let len = source.width() as usize * source.height() as usize;
    let bytes = convert8_runs(
        |rgb| transform.color(rgb),
        std::iter::once(&source.as_raw()[..len * 4]).map(|raw| raw.as_chunks::<4>().0),
        len,
        job,
    )?;
    RgbaImage::from_raw(source.width(), source.height(), bytes)
        .map(Arc::new)
        .ok_or_else(|| "invalid converted source dimensions".into())
}

/// The per-pixel conversion over runs that concatenate in plane order; `i`
/// is the RUNNING sample index, so every `job.every` lands on the sample the
/// single-image walk polled at.
fn convert8_runs<'a>(
    color: impl Fn([u8; 3]) -> [u8; 3],
    runs: impl Iterator<Item = &'a [[u8; 4]]>,
    len: usize,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let mut bytes = reserved_vec(len * 4)?;
    let mut i = 0usize;
    for run in runs {
        for pixel in run {
            job.every(i)?;
            let rgb = color([pixel[0], pixel[1], pixel[2]]);
            bytes.extend_from_slice(&[rgb[0], rgb[1], rgb[2], pixel[3]]);
            i += 1;
        }
    }
    Ok(bytes)
}
pub(crate) fn color16(
    source: &PrecisionImage,
    to: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    if source.profile().bytes() == to.bytes() {
        return Ok(source.with_profile(to.clone()));
    }
    if let Some(pipeline) = lut_pipeline(source.profile(), to)? {
        if source.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("retained source requires RGBA16 UNORM storage".into());
        }
        // `ColorPipeline::color8`'s rule at 16 bits: a LUT side answers
        // continuous values, clamped and rounded once.
        return convert16_rows(source, to, job, |rgb| {
            let out = pipeline.color(rgb);
            if out.iter().any(|v| !v.is_finite()) {
                return Err("source color transform produced an invalid component".into());
            }
            Ok(out.map(|v| v.clamp(0.0, 1.0)))
        });
    }
    let src = source
        .profile()
        .model()
        .ok_or("source conversion requires an RGB matrix/TRC source profile")?;
    let dst = to
        .model()
        .ok_or("source conversion requires an RGB matrix/TRC destination profile")?;
    let transform = PrecisionTransform::new(src, dst)
        .map_err(|e| crate::icc_encode_refusal::own_profile_error(to, e))?;
    if source.storage() != PrecisionStorage::Rgba16Unorm {
        return Err("retained source requires RGBA16 UNORM storage".into());
    }
    convert16_rows(source, to, job, |rgb| {
        let rgb = transform.color(rgb);
        if rgb
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("source color transform produced an invalid component".into());
        }
        Ok(rgb)
    })
}

/// [`color16`]'s per-pixel walk; `color` answers components in [0, 1].
fn convert16_rows(
    source: &PrecisionImage,
    to: &Arc<IccProfile>,
    job: JobContext<'_>,
    color: impl Fn([f32; 3]) -> Result<[f32; 3], String>,
) -> Result<PrecisionImage, String> {
    let (w, h) = source.dimensions();
    let mut pixels = reserved_vec(w as usize * h as usize)?;
    // Each row's tile runs are copied into ONE row buffer before the
    // per-pixel transform walks it in HEAD's order: transforming straight off
    // the runs measured ~5% slower at 3 MP (13B-3e-2 fix pass), the row copy
    // matches the contiguous walk.
    let mut row: Vec<[u16; 4]> = reserved_vec(w as usize)?;
    let mut i = 0usize;
    for y in 0..h {
        row.clear();
        for (_, run) in source.row_segments(y, 0, w) {
            let PrecisionRun::Rgba16(run) = run else {
                return Err("retained source requires RGBA16 UNORM storage".into());
            };
            row.extend_from_slice(run);
        }
        for pixel in &row {
            job.every(i)?;
            let rgb = color(std::array::from_fn(|c| f32::from(pixel[c]) / 65535.))?;
            pixels.push([
                (rgb[0] * 65535.).round() as u16,
                (rgb[1] * 65535.).round() as u16,
                (rgb[2] * 65535.).round() as u16,
                pixel[3],
            ]);
            i += 1;
        }
    }
    PrecisionImage::from_rgba16_unseeded(w, h, pixels, to.clone(), None)
}
pub(crate) fn render8(
    mut source: RetainedSource8,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<RetainedSource8, String> {
    if source.asset.depth() >= 16 {
        let (rendered, offset) =
            render_high_depth(&source.asset, source.transform, source.filter, profile, job)?;
        source.rendered = LayerRaster::unseeded(
            rendered
                .quantize_with_context(crate::precision_quantize::PrecisionDither::None, job)?,
        );
        source.rendered_offset = offset;
        source.rendered_profile = profile.clone();
        return Ok(source);
    }
    let colored = source8(&source.asset, profile, job)?;
    let bounds = source_transform::bounds(source.asset.dimensions(), source.transform)?;
    let sampling = match source.transform {
        SourceTransform::Affine(m) => AffineSampling::new(
            colored.dimensions(),
            (bounds.2, bounds.3),
            Affine::from_array(m),
            source.filter,
        ),
        SourceTransform::Projective(_) => AffineSampling::Sample(source.filter),
    };
    source.rendered = match sampling {
        AffineSampling::Shared => colored.into_raster(),
        AffineSampling::Sample(filter) => {
            let held;
            let image: &RgbaImage = match &colored {
                Colored8::Raster(r) => {
                    held = r.to_contiguous_with_job(job)?;
                    &held
                }
                Colored8::Owned(i) => i,
            };
            LayerRaster::unseeded(resample_rgba_with_context(
                image,
                bounds,
                &source_map(source.transform, (bounds.0, bounds.1))?,
                filter.sampler(),
                false,
                job,
            )?)
        }
    };
    source.rendered_offset = (bounds.0, bounds.1);
    source.rendered_profile = profile.clone();
    Ok(source)
}
pub(crate) fn render16(
    mut source: RetainedSource16,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<RetainedSource16, String> {
    let (image, offset) =
        render_high_depth(&source.asset, source.transform, source.filter, profile, job)?;
    source.rendered = image;
    source.rendered_offset = offset;
    Ok(source)
}
/// The byte master in `profile`: the asset's own plane when no conversion
/// is needed, else [`color8`]'s converted plane.
pub(crate) fn source8(
    asset: &Arc<SourceAsset>,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<Colored8, String> {
    color8(
        asset.rgba8().ok_or("invalid byte retained source")?,
        asset.profile(),
        profile,
        job,
    )
}

/// An f32 master as u16 codes in `profile` by take1's build rows (receipt
/// `float-low-depth-600/take1`): converted into `profile`
/// in linear light first (PP3 read `matrix_clip`, so the unclamped values
/// cross the matrix), then clipped and encoded by `profile`'s own forward
/// curve (P8 / P16 read RENDER `clip`, no tone map) through the one encode
/// loop, `float_document::encoded_u16` under a Clip policy. A converted
/// sample beyond ±65504 refuses as the float place does. A document profile
/// with no matrix/TRC model refuses with its own sentence, checked here
/// because `FloatPolicy::for_profile`'s names a float document.
fn float_master_codes(
    master: &PrecisionImage,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    use crate::float_document::{encoded_u16, FloatPolicy, ToneMap};
    if profile.model().is_none() {
        return Err(format!(
            "“{}” has no matrix/TRC model this build can decode, so it cannot render a 32-bit float master",
            profile.name()
        ));
    }
    let policy = FloatPolicy::for_profile(profile, ToneMap::Clip)?;
    let linear = crate::retained_source_float::master_in_profile(master, profile, job)?;
    encoded_u16(&policy, &linear, job)
}

/// The master as u16 codes in `profile`: a byte master promoted, an f32
/// master through [`float_master_codes`], then [`color16`].
pub(crate) fn source16(
    asset: &Arc<SourceAsset>,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let original = match &asset.pixels {
        SourcePixels::Rgba16(image) => image.clone(),
        SourcePixels::Rgba8 { pixels, profile } => PrecisionImage::from_rgba16_unseeded(
            pixels.width(),
            pixels.height(),
            crate::precision_conversion::promote_raster(pixels, job)?,
            profile.clone(),
            None,
        )?,
        // Below 32 bits an f32 master renders as codes measured against
        // Photoshop; in a float document it never reaches here
        // (`retained_source_float::source_float`).
        SourcePixels::Rgba32F(master) => float_master_codes(master, profile, job)?,
    };
    color16(&original, profile, job)
}

fn render_high_depth(
    asset: &Arc<SourceAsset>,
    transform: SourceTransform,
    filter: GeometryFilter,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<(PrecisionImage, (i32, i32)), String> {
    let colored = source16(asset, profile, job)?;
    let bounds = source_transform::bounds(asset.dimensions(), transform)?;
    let sampling = match transform {
        SourceTransform::Affine(m) => AffineSampling::new(
            colored.dimensions(),
            (bounds.2, bounds.3),
            Affine::from_array(m),
            filter,
        ),
        SourceTransform::Projective(_) => AffineSampling::Sample(filter),
    };
    let image = match sampling {
        AffineSampling::Shared => colored.reidentified(),
        AffineSampling::Sample(filter) => precision_sampling::transform_image(
            &colored,
            (bounds.2, bounds.3),
            &source_map(transform, (bounds.0, bounds.1))?,
            filter,
            job,
        )?,
    };
    Ok((image, (bounds.0, bounds.1)))
}
pub(crate) fn source_map(
    transform: SourceTransform,
    origin: (i32, i32),
) -> Result<SourceMap, String> {
    match transform {
        SourceTransform::Affine(matrix) => Ok(SourceMap::new(
            &Affine::from_array(matrix)
                .inverse()
                .ok_or("singular source transform")?,
            (0, 0),
            origin,
        )),
        SourceTransform::Projective(_) => {
            Ok(SourceMap::projective(&transform.inverse()?, (0, 0), origin))
        }
    }
}
/// The colour visit's admission figures for one source conversion:
/// `scratch_bytes` the prepared tables, `work_per_pixel` the per-source-pixel
/// work units [`work_plan_within`] charges for the visit.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceColorCost {
    pub scratch_bytes: u64,
    pub work_per_pixel: u64,
}

/// `ColorPipeline::color8` through a LUT side over the matrix fast path, per
/// pixel, rounded up: the `lut_color_cost_ratio` probe
/// (`tests/source_conversion_lut.rs`, release) over 1,048,576 triples with
/// Adobe's `VideoHD16-235.icc`, 2026-10-03, three runs: sRGB→LUT 7.82–7.97×,
/// LUT→sRGB 9.91–10.17× the sRGB→Display P3 fast path; the larger, 10.17,
/// rounds up to 11.
pub(crate) const LUT_COLOR_WORK_FACTOR: u64 = 11;

/// `Ok(None)` when both profiles carry a matrix/TRC model (callers keep the
/// shipped `Transform` / `PrecisionTransform` path verbatim); otherwise the
/// relative-colorimetric, BPC-off general pipeline. `Err` is
/// `ColorPipeline::new`'s sentence. Every source render is relative/no-BPC,
/// and this is the one place that choice is written.
pub(crate) fn lut_pipeline(
    from: &IccProfile,
    to: &IccProfile,
) -> Result<Option<ColorPipeline>, String> {
    if from.model().is_some() && to.model().is_some() {
        return Ok(None);
    }
    ColorPipeline::new(from, to, RenderingIntent::RelativeColorimetric, false).map(Some)
}

/// The source colour visit's cost; same refusals and `typed` meaning as the
/// scratch figure it replaced.
pub(crate) fn profile_cost(
    from: &Arc<IccProfile>,
    to: &Arc<IccProfile>,
    typed: bool,
) -> Result<SourceColorCost, String> {
    if from.bytes() == to.bytes() {
        return Ok(SourceColorCost {
            scratch_bytes: 0,
            work_per_pixel: 4,
        });
    }
    if let Some(pipeline) = lut_pipeline(from, to)? {
        return Ok(SourceColorCost {
            scratch_bytes: pipeline.scratch_bytes(),
            work_per_pixel: 4 * LUT_COLOR_WORK_FACTOR,
        });
    }
    let src = from
        .model()
        .ok_or("source conversion requires an RGB matrix/TRC source profile")?;
    let dst = to
        .model()
        .ok_or("source conversion requires an RGB matrix/TRC destination profile")?;
    Ok(SourceColorCost {
        scratch_bytes: if typed {
            PrecisionTransform::prepare_scratch_bytes(src, dst)
        } else {
            BOOKKEEPING_BYTES
        },
        work_per_pixel: 4,
    })
}
