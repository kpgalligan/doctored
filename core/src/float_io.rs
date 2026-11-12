//! Float interchange in and out for the 32-bit float document (Phase 11F):
//! float TIFF through the 11D `tiff_float` seam, OpenEXR (`exr_float`) and
//! Radiance RGBE (`radiance_hdr`). This module owns
//! the POLICY the seams deliberately leave open: what a value above 1.0 or
//! below 0.0 means, what happens to a NaN, which alpha convention the samples
//! are stored in, and which profile a file's primaries become.
//!
//! Import: the samples are sanitised ONCE ([`crate::float_document::sanitize`])
//! and every repair is disclosed as an import warning. The
//! converted pixels go row by row straight into tiles at every size
//! (`PrecisionImage::from_rgba32_tiles_decoded`), never into a contiguous copy. `ExtraSamples = 1`
//! (associated alpha) is unpremultiplied on the way in, because a float
//! document stores STRAIGHT alpha; `2` is taken as is and an absent alpha
//! becomes 1.0. One or two samples are grayscale and fill R = G = B — there
//! is no curve to undo, since the samples are already linear. An embedded ICC
//! must be an RGB matrix/TRC profile this build can model, because the SDR
//! encode runs its forward curve; anything else refuses by sentence rather
//! than being ignored. The one exception (#1 phase 3c): a one- or
//! two-sample TIFF whose ICC is a usable Gray profile (`GrayPolicy::new`)
//! opens as a 32-bit Gray document under that policy's twin; an untagged
//! one still opens as RGB float under sRGB.
//!
//! OpenEXR and Radiance carry no ICC profile; they state xy primaries
//! (`chromaticities`, `PRIMARIES=`). Absent → the built-in sRGB with a
//! warning (OpenEXR's own default is Rec.709); within
//! `float_primaries::MATCH_TOLERANCE` of Rec.709 → sRGB; of Display P3 → the
//! built-in Display P3; anything else → the samples are converted once, in
//! linear light, to sRGB primaries (`float_primaries::conversion` to
//! `REC709`) with a warning naming the conversion. Negatives stay. OpenEXR alpha is ASSOCIATED
//! and takes the TIFF `ExtraSamples = 1` path; Radiance has none and opens
//! opaque. An OpenEXR canvas is the DISPLAY window and the one layer is the
//! DATA window at its offset; a difference, other parts and a pixel aspect
//! other than 1 are warnings, and a Radiance `EXPOSURE` / `COLORCORR` is
//! disclosed and not applied.
//!
//! The open's plan (file + the codec's scratch + 16 B/px of tiles +
//! bookkeeping) was measured with `precision_render`'s
//! `# Admission` method on a Deflate 4×f32 `ExtraSamples = 2`
//! TIFF (an uncompressed 100 MP file exceeds the input cap, 512 MiB then and
//! 1 GiB now; the blue
//! steps every 8 px so Deflate fits it), A confirmed by a "capacity" refusal
//! at A − 1: 24 MP max E 854,786,696, A 1,238,664,243 (1.4491×); 100 MP max E
//! 3,553,692,192, A 5,147,289,595 (1.4484×). It covers 1.25×, so unchanged.
//! The OpenEXR and Radiance opens charge the same shape with their seams'
//! scratch (`count·32 + 1 MiB`, `count·24 + 1 MiB`), measured
//! on a 6000×4000 picture (`tests/float_interchange_24mp.rs::measure`:
//! Release, one case per process, `MallocLargeCache=0`, two sets of three
//! runs, the file written by a child process, a baseline twin reading 0):
//! OpenEXR (f32, ZIP, a 31.6 MB file) max E 771,588,696 against a plan of
//! 1,185,700,751 (1.5367×; E spreads about 1 % between runs, the crate's
//! parallel block decode); Radiance (a 12.8 MB file) max E 687,047,208
//! against 974,883,403 (1.4189×). Both cover 1.25×, so the per-pixel figures
//! stand; that test pins the floors.
//!
//! An HDR still decoded by the host is the fourth source,
//! [`FloatContainer::HostStill`]: `float_host_intake` turns the host's f32 or
//! PQ/HLG pixels into linear light and hands them to the same
//! [`document_of`], its stated primaries through the same [`profile_of`]
//! rule. It is never a write container.
//!
//! A float TIFF stored rotated (Orientation 2 to 8, or its EXIF packet's)
//! opens upright, as every other flat TIFF open does: [`document_of`] reads
//! each upright pixel's stored sample through `pixel_orientation`, and a
//! quarter turn swaps the canvas and the resolution's axes. Its paths are
//! read from its 8BIM run (`tiff_paths`), which reads none from a rotated
//! file.
//!
//! Export: TIFF writes four samples, `ExtraSamples = 2` (unassociated),
//! Deflate, f32 or half, and the profile, packets and paths from the shared
//! sidecar (a 32-bit Gray document's kept-gray TIFF writes its R alone, plus
//! its alpha when any pixel is not opaque, [`encode_float_gray`]) — the same
//! `EncodeSidecar` every other writer in the crate uses, written by
//! `tiff_encode`'s one sidecar writer. OpenEXR and
//! Radiance convert NO pixel: the file declares the document profile's
//! primaries ([`primaries_of`]). OpenEXR writes associated alpha (each RGB
//! sample times alpha), f32 or half; Radiance writes `max(rgb, 0) · alpha`.
//!
//! # The OpenEXR round trip
//!
//! Open then export at f32 under the same primaries: bit-exact where alpha is
//! 1 or absent; where 0 < alpha < 1, alpha is bit-exact and each RGB sample
//! is within 2 ulp of the stored f32 (one division and one multiplication,
//! each correctly rounded), for normal samples whose straight value stays
//! inside ±65504; where alpha is 0 the RGB comes back 0, the open's canonical
//! zero.

use std::io::Cursor;
use std::sync::Arc;

use crate::adjust_white_balance::{mat_vec, native_adaptation};
use crate::exr_float::{self, Chromaticities};
use crate::float_document::{sanitize, ToneMap, FLOAT_RANGE_MAX};
use crate::float_primaries;
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::icc::{IccBytes, IccProfile};
use crate::job_control::JobContext;
use crate::metadata::{Metadata, Resolution};
use crate::metadata_write::EncodeSidecar;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::radiance_hdr;
use crate::tiff_float::{self, FloatSampleType, FloatTiff};
use crate::tile_store::TilePlane;

/// The three float interchange containers, and the host-decoded HDR still
/// (open only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FloatContainer {
    Tiff,
    Exr,
    Radiance,
    HostStill,
}

impl FloatContainer {
    /// The name a refusal or warning calls the container by.
    fn name(self) -> &'static str {
        match self {
            Self::Tiff => "float TIFF",
            Self::Exr => "OpenEXR",
            Self::Radiance => "Radiance HDR",
            Self::HostStill => "HDR still",
        }
    }
}

fn problem(
    container: FloatContainer,
    category: &str,
    label: &str,
    error: impl std::fmt::Display,
) -> String {
    format!("{} {category} for {label}: {error}", container.name())
}

/// One decoded container, whichever it was, as the ONE document builder
/// takes it.
pub(crate) struct FloatSource {
    pub(crate) container: FloatContainer,
    /// The layer's own size AS STORED (an OpenEXR data window); `orientation`
    /// turns it.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The EXIF orientation (1–8) the samples are stored in: the layer is
    /// turned upright through `pixel_orientation`, so 5 to 8 swap its size.
    /// Only a TIFF states one; every other source passes 1.
    pub(crate) orientation: u8,
    /// The canvas (upright), and the layer's offset on it.
    pub(crate) canvas: (u32, u32),
    pub(crate) offset: (i32, i32),
    /// Samples per pixel in `samples`: 1, 2, 3 or 4.
    pub(crate) channels: usize,
    /// `Some(true)` for associated alpha, `Some(false)` for straight, `None`
    /// when the file declares none.
    pub(crate) alpha: Option<bool>,
    pub(crate) samples: Vec<f32>,
    pub(crate) profile: Arc<IccProfile>,
    /// A linear conversion into the profile's primaries, row-major, applied
    /// to each straight RGB triple; `None` when the samples are already there.
    pub(crate) matrix: Option<[[f64; 3]; 3]>,
    pub(crate) resolution: Resolution,
    pub(crate) metadata: Metadata,
    /// What the container's own reading has to disclose, in order, after the
    /// non-finite count.
    pub(crate) warnings: Vec<String>,
}

/// The working memory of one float open: the seam's scratch, 16 B/px of
/// tiles and bookkeeping, admitted beside the file's own bytes. A float
/// export's preview decodes the written bytes the same way and charges the
/// same plan (`doc_export_float::FloatDeliverable::preview`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn admit_open(
    container: FloatContainer,
    bytes: &[u8],
    label: &str,
    dimensions: (u32, u32),
    seam_scratch: u64,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let count = pixel_count(dimensions.0, dimensions.1)?;
    let scratch = seam_scratch
        .checked_add(count as u64 * 16)
        .and_then(|v| v.checked_add(BOOKKEEPING_BYTES))
        .ok_or_else(|| format!("{} working-memory calculation overflow", container.name()))?;
    admit(bytes.len() as u64, scratch, other, limit)
        .map_err(|e| problem(container, "capacity", label, e))?;
    job.reserve(scratch)
}

/// Opens a genuine floating-point TIFF as a 32-bit float document at the
/// default `clip` map; one or two samples under a usable Gray ICC open as a
/// 32-bit Gray document (module doc).
pub(crate) fn from_float_tiff(
    bytes: &[u8],
    label: &str,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
    job.check()?;
    let plan = tiff_float::inspect(bytes, label)?;
    admit_open(
        FloatContainer::Tiff,
        bytes,
        label,
        (plan.width, plan.height),
        plan.scratch_bytes,
        limit,
        other,
        job,
    )?;
    let decoded = tiff_float::decode(bytes, label, plan.scratch_bytes)?;
    job.report(0.6, 1)?;
    let FloatTiff {
        width,
        height,
        channels,
        alpha,
        samples,
        icc,
        resolution,
        metadata,
        ..
    } = decoded;
    let mut warnings = Vec::new();
    // A one- or two-sample file under a Gray profile is a Gray document
    // (#1 phase 3c); its samples are linear `Y` under the policy's twin.
    let gray = match &icc {
        Some(bytes) if channels <= 2 => GrayPolicy::new(bytes, GrayDepth::ThirtyTwo).ok(),
        _ => None,
    };
    let profile = match (&icc, &gray) {
        (_, Some(policy)) => policy.twin().clone(),
        (Some(bytes), None) => {
            let parsed = IccProfile::parse(bytes).ok_or_else(|| {
                problem(
                    FloatContainer::Tiff,
                    "profile",
                    label,
                    "invalid or unsupported RGB image profile",
                )
            })?;
            let model = parsed.model().ok_or_else(|| {
                problem(
                    FloatContainer::Tiff,
                    "profile",
                    label,
                    "a 32-bit float document requires an RGB matrix/TRC profile",
                )
            })?;
            model
                .validate_precision()
                .map_err(|e| problem(FloatContainer::Tiff, "profile", label, e))?;
            Arc::new(parsed)
        }
        (None, None) => {
            warnings.push(
                "float TIFF without an ICC profile is interpreted in sRGB primaries as linear light; no pixel conversion was performed".into(),
            );
            IccProfile::srgb()
        }
    };
    // Stored rotated, the raster opens upright as every other flat TIFF
    // open turns it (`gray_io`, `lab_tiff_read`): tag 274, else the EXIF
    // packet's, and the resolution's axes swap with a quarter turn.
    let orientation = crate::metadata_tiff::orientation_of_file(bytes)
        .or_else(|| {
            metadata
                .exif
                .as_deref()
                .and_then(crate::metadata_tiff::orientation)
        })
        .map_or(1, |o| o.to_exif());
    let quarter = (5..=8).contains(&orientation);
    let (upright_w, upright_h) =
        crate::pixel_orientation::dimensions(width as usize, height as usize, orientation);
    let document = document_of(
        FloatSource {
            container: FloatContainer::Tiff,
            width,
            height,
            orientation,
            canvas: (upright_w as u32, upright_h as u32),
            offset: (0, 0),
            channels: usize::from(channels),
            alpha,
            samples,
            profile,
            matrix: None,
            resolution: resolution.unwrap_or_default().transposed_if(quarter),
            metadata,
            warnings,
        },
        label,
        job,
    )?;
    let document = match gray {
        Some(policy) => document.with_gray(Arc::new(policy))?,
        None => document,
    };
    // Its paths (`tiff_paths`): a file stored rotated reads none, as at
    // every other flat TIFF open, since its records name the stored raster.
    document.with_tiff_paths(bytes)
}

/// A row-major linear conversion between two sets of primaries.
type PrimariesMatrix = [[f64; 3]; 3];

/// The profile a file's stated primaries become, the conversion its samples
/// need to get there, and the warning either choice owes the user (module
/// doc). `what` names the attribute: `chromaticities`, `PRIMARIES`, or an
/// HDR still's `primaries`.
pub(crate) fn profile_of(
    container: FloatContainer,
    primaries: Option<Chromaticities>,
    what: &str,
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<(Arc<IccProfile>, Option<PrimariesMatrix>), String> {
    let Some(stated) = primaries else {
        warnings.push(format!(
            "{} without {what} is interpreted in sRGB primaries as linear light; no pixel conversion was performed",
            container.name()
        ));
        return Ok((IccProfile::srgb(), None));
    };
    if float_primaries::matches(&stated, &float_primaries::REC709) {
        return Ok((IccProfile::srgb(), None));
    }
    if float_primaries::matches(&stated, &float_primaries::DISPLAY_P3) {
        return Ok((IccProfile::display_p3(), None));
    }
    let matrix =
        float_primaries::conversion(&stated, &float_primaries::REC709).ok_or_else(|| {
            problem(
                container,
                "profile",
                label,
                format!("{what} do not describe a colour basis"),
            )
        })?;
    let numbers: Vec<String> = stated.iter().flatten().map(|v| v.to_string()).collect();
    warnings.push(format!(
        "{} {what} ({}) are neither sRGB nor Display P3; the samples were converted in linear light to sRGB primaries",
        container.name(),
        numbers.join(" ")
    ));
    Ok((IccProfile::srgb(), Some(matrix)))
}

/// Opens an OpenEXR file as a 32-bit float document at the default `clip`
/// map: the first flat RGB part, its display window as the canvas (module
/// doc).
pub(crate) fn from_exr(
    bytes: &[u8],
    label: &str,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
    job.check()?;
    let plan = exr_float::inspect(bytes, label)?;
    admit_open(
        FloatContainer::Exr,
        bytes,
        label,
        (plan.width, plan.height),
        plan.scratch_bytes,
        limit,
        other,
        job,
    )?;
    let decoded = exr_float::decode(bytes, label, plan.scratch_bytes)?;
    job.report(0.6, 1)?;
    let plan = decoded.plan;
    let mut warnings = Vec::new();
    let (profile, matrix) = profile_of(
        FloatContainer::Exr,
        plan.chromaticities,
        "chromaticities",
        label,
        &mut warnings,
    )?;
    if plan.parts > 1 {
        let named = plan
            .part_name
            .as_ref()
            .map(|name| format!(" (\"{name}\")"))
            .unwrap_or_default();
        warnings.push(format!(
            "OpenEXR part {} of {}{named} was opened; the other parts were not read",
            plan.part + 1,
            plan.parts
        ));
    }
    if plan.canvas != (plan.width, plan.height) || plan.offset != (0, 0) {
        warnings.push(format!(
            "the OpenEXR data window ({}×{} at {}, {}) differs from the display window ({}×{}); the canvas is the display window",
            plan.width, plan.height, plan.offset.0, plan.offset.1, plan.canvas.0, plan.canvas.1
        ));
    }
    if plan.pixel_aspect != 1.0 {
        warnings.push(format!(
            "the OpenEXR pixel aspect ratio {} is not applied; pixels are square here",
            plan.pixel_aspect
        ));
    }
    document_of(
        FloatSource {
            container: FloatContainer::Exr,
            width: plan.width,
            height: plan.height,
            orientation: 1,
            canvas: plan.canvas,
            offset: plan.offset,
            channels: 4,
            alpha: plan.alpha.then_some(true),
            samples: decoded.samples,
            profile,
            matrix,
            resolution: Resolution::default(),
            metadata: Metadata::default(),
            warnings,
        },
        label,
        job,
    )
}

/// Opens a Radiance RGBE file as an opaque 32-bit float document at the
/// default `clip` map.
pub(crate) fn from_radiance(
    bytes: &[u8],
    label: &str,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
    job.check()?;
    let plan = radiance_hdr::inspect(bytes, label)?;
    admit_open(
        FloatContainer::Radiance,
        bytes,
        label,
        (plan.width, plan.height),
        plan.scratch_bytes,
        limit,
        other,
        job,
    )?;
    let (plan, samples) = radiance_hdr::decode(bytes, label, plan.scratch_bytes)?;
    job.report(0.6, 1)?;
    let mut warnings = Vec::new();
    let (profile, matrix) = profile_of(
        FloatContainer::Radiance,
        plan.primaries,
        "PRIMARIES",
        label,
        &mut warnings,
    )?;
    if let Some(exposure) = plan.exposure {
        warnings.push(format!(
            "the Radiance HDR EXPOSURE {exposure} is recorded in the file and was not applied"
        ));
    }
    if let Some([r, g, b]) = plan.color_correction {
        warnings.push(format!(
            "the Radiance HDR COLORCORR {r} {g} {b} is recorded in the file and was not applied"
        ));
    }
    document_of(
        FloatSource {
            container: FloatContainer::Radiance,
            width: plan.width,
            height: plan.height,
            orientation: 1,
            canvas: (plan.width, plan.height),
            offset: (0, 0),
            channels: 3,
            alpha: None,
            samples,
            profile,
            matrix,
            resolution: Resolution::default(),
            metadata: Metadata::default(),
            warnings,
        },
        label,
        job,
    )
}

/// The decoded container as a document: sanitize, the alpha convention, the
/// primaries conversion, the range clamp, the tiles and the warnings, once
/// for the three containers and the host-decoded HDR still
/// (`float_host_intake`).
pub(crate) fn document_of(
    source: FloatSource,
    label: &str,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
    let FloatSource {
        container,
        width,
        height,
        orientation,
        canvas,
        offset,
        channels,
        alpha,
        mut samples,
        profile,
        matrix,
        resolution,
        metadata,
        warnings: container_warnings,
    } = source;
    let count = pixel_count(width, height)?;
    if samples.len() != count * channels {
        return Err(problem(
            container,
            "decode",
            label,
            "sample count differs from the declared dimensions",
        ));
    }
    let (non_finite, mut clamped) = sanitize(&mut samples, channels);
    let mut warnings = Vec::new();
    if non_finite > 0 {
        warnings.push(format!("{non_finite} non-finite samples replaced"));
    }
    warnings.extend(container_warnings);
    if alpha == Some(true) {
        warnings.push("associated alpha unpremultiplied".into());
    }
    // The tiles are filled upright: each output pixel reads its stored
    // sample through the shared EXIF mapping, so a turned file needs no
    // second sample buffer and every stored pixel is still read once.
    let (sw, sh) = (width as usize, height as usize);
    let (upright_w, upright_h) = crate::pixel_orientation::dimensions(sw, sh, orientation);
    let fill_row = |y: u32, row: &mut [[f32; 4]]| {
        let first = y as usize * upright_w;
        for (x, pixel) in row.iter_mut().enumerate() {
            job.every(first + x)?;
            let (sx, sy) = crate::pixel_orientation::source(x, y as usize, sw, sh, orientation);
            let at = (sy * sw + sx) * channels;
            let (rgb, a) = match channels {
                1 => ([samples[at]; 3], 1.0),
                2 => ([samples[at]; 3], samples[at + 1]),
                3 => ([samples[at], samples[at + 1], samples[at + 2]], 1.0),
                _ => (
                    [samples[at], samples[at + 1], samples[at + 2]],
                    samples[at + 3],
                ),
            };
            // Straight alpha is the document's storage: an associated-alpha
            // file divides here, and `α = 0` has no colour to recover, so its
            // RGB is the canonical zero rather than a division by zero.
            let mut rgb = if alpha == Some(true) {
                if a > 0.0 {
                    rgb.map(|v| v / a)
                } else {
                    [0.0; 3]
                }
            } else {
                rgb
            };
            if let Some(matrix) = matrix {
                rgb = mat_vec(matrix, rgb.map(f64::from)).map(|v| v as f32);
            }
            for value in &mut rgb {
                let bounded = value.clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX);
                if bounded != *value {
                    clamped += 1;
                    *value = bounded;
                }
            }
            *pixel = [rgb[0], rgb[1], rgb[2], a];
        }
        Ok(())
    };
    let plane = TilePlane::from_rows_with_job(
        upright_w as u32,
        upright_h as u32,
        fill_row,
        JobContext::NONE,
    )?;
    drop(samples);
    if clamped > 0 {
        warnings.push(format!("{clamped} out-of-range samples clamped"));
    }
    job.report(0.9, 1)?;
    let image = PrecisionImage::from_rgba32_tiles_decoded(plane, profile.clone())?;
    let layer = PrecisionDocumentLayer::raster_float("Background", image, offset, 1.0, true)?;
    PrecisionDocument::from_parts_float(
        canvas.0,
        canvas.1,
        profile,
        metadata,
        resolution,
        vec![layer],
        Vec::new(),
        ToneMap::Clip,
    )?
    .with_import_warnings(warnings)
}

/// The primaries an OpenEXR or Radiance export declares for `profile`: the
/// published Rec.709 constants when it describes sRGB, the Display P3
/// constants when its model matches the built-in, otherwise its own native
/// primaries (`float_primaries::of_model`). No pixel is converted.
pub(crate) fn primaries_of(profile: &IccProfile) -> Result<Chromaticities, String> {
    if profile.describes_srgb() {
        return Ok(float_primaries::REC709);
    }
    let model = profile
        .model()
        .ok_or("a 32-bit float export requires an RGB matrix/TRC document profile")?;
    if IccProfile::display_p3()
        .model()
        .is_some_and(|p3| model.approx_eq(p3))
    {
        return Ok(float_primaries::DISPLAY_P3);
    }
    let media_white = IccBytes::new(profile.bytes()).and_then(|icc| icc.media_white());
    Ok(float_primaries::of_model(
        model,
        native_adaptation(profile.chromatic_adaptation(), media_white),
    ))
}

/// Encodes the F32 projection of a 32-bit Gray document as a kept-gray float
/// TIFF (#1 phase 3c): its R (R = G = B) as one sample, and its alpha as a
/// second (`ExtraSamples = 2`) only when some pixel's alpha is not 1.0;
/// Deflate, at `samples` width, with `side`'s profile and packets (the
/// caller's Gray ICC).
pub(crate) fn encode_float_gray(
    image: &PrecisionImage,
    samples: FloatSampleType,
    side: &EncodeSidecar<'_>,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let alpha = image
        .runs_rgba32()
        .ok_or("a 32-bit float export requires rgba32_float storage")?
        .flatten()
        .any(|pixel| pixel[3] != 1.0);
    let runs = image
        .runs_rgba32()
        .ok_or("a 32-bit float export requires rgba32_float storage")?;
    let (width, height) = image.dimensions();
    let channels: u8 = if alpha { 2 } else { 1 };
    let mut flat = reserved_vec(width as usize * height as usize * usize::from(channels))?;
    for (i, pixel) in runs.flatten().enumerate() {
        job.every(i)?;
        flat.push(pixel[0]);
        if alpha {
            flat.push(pixel[3]);
        }
    }
    job.check()?;
    let out = FloatTiff {
        width,
        height,
        channels,
        alpha: Some(false),
        samples: flat,
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let mut bytes = Cursor::new(Vec::new());
    tiff_float::encode(&out, &mut bytes, true, samples, side)?;
    job.check()?;
    Ok(bytes.into_inner())
}

/// Encodes an F32 projection in `container` (module doc, "Export"). `samples`
/// chooses f32 or half for TIFF and OpenEXR; Radiance has one sample form and
/// ignores it. `side` carries the profile and packets a TIFF embeds; the
/// other two carry neither.
pub(crate) fn encode_float(
    image: &PrecisionImage,
    container: FloatContainer,
    samples: FloatSampleType,
    side: &EncodeSidecar<'_>,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    if container == FloatContainer::HostStill {
        return Err("an HDR still is a float source, not a float export container".into());
    }
    let runs = image
        .runs_rgba32()
        .ok_or("a 32-bit float export requires rgba32_float storage")?;
    let (width, height) = image.dimensions();
    let count = width as usize * height as usize;
    let per_pixel = if container == FloatContainer::Radiance {
        3
    } else {
        4
    };
    let mut flat = reserved_vec(count * per_pixel)?;
    let mut i = 0;
    for run in runs {
        for pixel in run {
            job.every(i)?;
            i += 1;
            let [r, g, b, a] = *pixel;
            match container {
                FloatContainer::Tiff | FloatContainer::HostStill => flat.extend_from_slice(pixel),
                // OpenEXR alpha is associated: the writer multiplies.
                FloatContainer::Exr => flat.extend_from_slice(&[r * a, g * a, b * a, a]),
                // RGBE has no alpha and no negative value.
                FloatContainer::Radiance => {
                    flat.extend_from_slice(&[r.max(0.0) * a, g.max(0.0) * a, b.max(0.0) * a]);
                }
            }
        }
    }
    job.check()?;
    let bytes = match container {
        FloatContainer::Tiff => {
            let out = FloatTiff {
                width,
                height,
                channels: 4,
                alpha: Some(false),
                samples: flat,
                icc: None,
                resolution: None,
                metadata: Metadata::default(),
                non_finite: 0,
            };
            let mut bytes = Cursor::new(Vec::new());
            tiff_float::encode(&out, &mut bytes, true, samples, side)?;
            bytes.into_inner()
        }
        FloatContainer::Exr => exr_float::encode(
            width,
            height,
            &flat,
            samples,
            primaries_of(image.profile())?,
        )?,
        FloatContainer::Radiance => {
            radiance_hdr::encode(width, height, &flat, primaries_of(image.profile())?)?
        }
        FloatContainer::HostStill => {
            return Err("an HDR still is a float source, not a float export container".into())
        }
    };
    job.check()?;
    Ok(bytes)
}
