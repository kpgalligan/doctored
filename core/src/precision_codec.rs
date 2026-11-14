//! Preserving RGB/RGBA16 PNG and TIFF (classic or BigTIFF) interchange for the neutral path.
//! No implicit byte promotion, profile conversion, associated-alpha conversion,
//! planar TIFF, animation, multipage or HDR interpretation. Orientation only
//! permutes samples. Packet policy is shared with doc_color's byte writer.
//! `PrecisionDocument::open_preserving` hands a multipage TIFF to this codec
//! one page at a time (`tiff_pages::with_page`); every other caller still
//! meets the multipage refusal.
//!
//! Codec dependencies decode/compress a frame in one synchronous call that takes
//! no job; the decoders read their input and the encoders write their output
//! through `cancel_io::Polled`, so the job is polled every 64 KiB of encoded
//! bytes moved inside that call (what each codec computes before its first
//! write is `cancel_io`'s module doc), and a cancel there answers exactly
//! `operation cancelled` (`cancel_io::settle`). Admission precedes pixel
//! allocation; checkpoints also surround those calls and poll our sample
//! conversion loops every 4096 pixels. Cancellation drops partial
//! buffers/files and never publishes a partial image.
//!
//! Byte preview conversion is explicit and cannot enter typed saves. The 7E host selects preserving
//! import explicitly; a typed refusal never authorizes a byte fallback.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Cursor, Read, Seek, Write};
use std::sync::Arc;

use image::codecs::png::{CompressionType, FilterType, PngDecoder, PngEncoder};
use image::{ColorType, ExtendedColorType, ImageDecoder, ImageEncoder};
use tiff::decoder::{Decoder as TiffDecoder, DecodingResult};
use tiff::tags::Tag;

use crate::cancel_io::{settle, Polled};
use crate::gray16_import::GrayInterpretation;
use crate::icc::IccProfile;
use crate::icc_lut::{DeviceSpace, ProofProfile};
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::{self, Metadata, Resolution};
use crate::metadata_write::{EncodeSidecar, MetadataInjector};
use crate::precision_image::{admit, pixel_count, reserved_vec, PrecisionImage};
pub(crate) use crate::precision_png::{
    inspect_native_payload, native_png_decode_scratch, native_png_encode_scratch,
    native_png_encoded_bound, put_native_png,
};
use crate::precision_png::{png_header, png_profile_contract};
pub(crate) use crate::precision_png_rows::{decode_native_png, native_png_band_scratch};
use crate::Format;

/// A bounded input container, independently of its decoded sample size.
pub const MAX_ENCODED_BYTES: u64 = 1024 * 1024 * 1024;
const DECODE_FIXED: u64 = 256 * 1024 * 1024;
const ENCODE_FIXED: u64 = 128 * 1024 * 1024;
pub(crate) const NATIVE_FIXED: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecisionFormat {
    Png,
    Tiff,
}

impl PrecisionFormat {
    fn legacy(self) -> Format {
        match self {
            Self::Png => Format::Png,
            Self::Tiff => Format::Tiff,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PrecisionDecodePlan {
    pub width: u32,
    pub height: u32,
    pub format: PrecisionFormat,
    pub source_channels: u8,
    /// True when the SOURCE is grayscale (1 or 2 samples per pixel). The
    /// decoded document is RGB either way — `gray16_import` interprets the
    /// samples — but a host that wants to say what the file was reads this.
    pub gray: bool,
    pub orientation: u8,
    /// Codec scratch plus returned typed pixels; encoded input is separate.
    pub scratch_bytes: u64,
}

pub struct DecodedPrecision {
    pub image: PrecisionImage,
    pub metadata: Metadata,
    pub resolution: Resolution,
    pub warnings: Vec<String>,
}

/// The encoded input as a decoder reads it: polled as it moves (`cancel_io`).
type Source<'a> = Polled<'a, Cursor<&'a [u8]>>;

// Exactly one decoder lives per call. Its sub-KiB stack state is included in
// the fixed header reservation; boxing either variant adds no useful bound.
#[allow(clippy::large_enum_variant)]
enum Decoder<'a> {
    Png(PngDecoder<Source<'a>>),
    Tiff(TiffDecoder<Source<'a>>),
}

struct Header<'a> {
    decoder: Decoder<'a>,
    stored: (u32, u32),
    plan: PrecisionDecodePlan,
    /// Present exactly when `plan.gray`: how the samples are to be read, and
    /// the sentence that discloses it.
    gray: Option<(GrayInterpretation, String)>,
    profile: Arc<IccProfile>,
    metadata: Metadata,
    resolution: Resolution,
    warnings: Vec<String>,
}

fn problem(category: &str, label: &str, error: impl std::fmt::Display) -> String {
    format!("high-depth {category} for {label}: {error}")
}

fn bounded_input(bytes: &[u8], label: &str) -> Result<(), String> {
    if bytes.len() as u64 > MAX_ENCODED_BYTES {
        Err(problem("capacity", label, "encoded input exceeds 1 GiB"))
    } else {
        Ok(())
    }
}

pub(crate) fn png_limits(bytes: u64) -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(bytes);
    limits.max_image_width = Some(crate::doc::MAX_PIXELS as u32);
    limits.max_image_height = Some(crate::doc::MAX_PIXELS as u32);
    limits
}

/// Header-only inspection. It never allocates decoded pixels. Bounded codec
/// headers/metadata can allocate; callers must retain encoded bytes separately.
pub fn inspect(bytes: &[u8], label: &str) -> Result<PrecisionDecodePlan, String> {
    Ok(header(bytes, label, JobContext::NONE)?.plan)
}

/// Parses the container's header with a decoder that reads `bytes` polled
/// under `job`.
fn header<'a>(bytes: &'a [u8], label: &str, job: JobContext<'a>) -> Result<Header<'a>, String> {
    bounded_input(bytes, label)?;
    let source = || Polled::new(Cursor::new(bytes), job);
    let (decoder, stored, channels, format, profile_bytes, fallback_orientation, interpretation) =
        if bytes.starts_with(&crate::metadata::PNG_SIGNATURE) {
            let ((w, h), source_channels) = png_header(bytes, false)?;
            pixel_count(w, h).map_err(|e| problem("capacity", label, e))?;
            let mut decoder = PngDecoder::with_limits(source(), png_limits(32 * 1024 * 1024))
                .map_err(|e| problem("decode", label, e))?;
            if decoder.is_apng().map_err(|e| problem("format", label, e))? {
                return Err(problem("format", label, "animated PNG is unavailable"));
            }
            let gray = source_channels == 1 || source_channels == 2;
            // The `iCCP`, else the profile a bare `gAMA`/`cHRM` describes
            // (`png_declared`), read exactly as an `iCCP` holding it.
            let icc = match decoder
                .icc_profile()
                .map_err(|e| problem("profile", label, e))?
            {
                Some(icc) => Some(icc),
                None => crate::png_declared::profile(bytes, gray),
            };
            // The GRAY branch is taken BEFORE the shared `IccProfile::parse`
            // step below, which gray never reaches: a Gray ICC has no primaries
            // and `IccProfile::parse` would refuse it, so the profile is read
            // through `ProofProfile` here and the document's own profile is
            // sRGB (`gray16_import`'s module doc says why).
            let interpretation = if gray {
                Some(gray_interpretation(icc.as_deref(), label)?)
            } else {
                None
            };
            let parsed = match &interpretation {
                Some((interpretation, _)) => {
                    matches!(interpretation, GrayInterpretation::GrayProfile(_))
                }
                None => icc.is_some(),
            };
            png_profile_contract(bytes, parsed, gray)?;
            let orientation = decoder
                .orientation()
                .map_err(|e| problem("orientation", label, e))?
                .to_exif();
            let channels = match decoder.color_type() {
                ColorType::L16 => 1,
                ColorType::La16 => 2,
                ColorType::Rgb16 => 3,
                ColorType::Rgba16 => 4,
                _ => {
                    return Err(problem(
                        "depth",
                        label,
                        "genuine unsigned Gray/GrayA/RGB/RGBA16 input is required",
                    ))
                }
            };
            // `>=` for RGB: the decoder may widen a tRNS-bearing Rgb16 IHDR to
            // Rgba16, reporting one channel more than the header declared.
            // Gray is never widened, so there the two must agree.
            debug_assert!(channels >= source_channels);
            debug_assert!(!gray || channels == source_channels);
            (
                Decoder::Png(decoder),
                (w, h),
                channels,
                PrecisionFormat::Png,
                icc,
                orientation,
                interpretation,
            )
        } else if crate::metadata_tiff::is_tiff(bytes) {
            let mut decoder =
                TiffDecoder::new(source()).map_err(|e| problem("decode", label, e))?;
            let stored = decoder
                .dimensions()
                .map_err(|e| problem("decode", label, e))?;
            pixel_count(stored.0, stored.1).map_err(|e| problem("capacity", label, e))?;
            let mut limits = tiff::decoder::Limits::default();
            // TIFF also charges tag Value arrays against this limit. Tiny
            // images still need room for SampleFormat and capture metadata.
            limits.decoding_buffer_size = ((u64::from(stored.0) * u64::from(stored.1) * 8)
                as usize)
                .max(crate::drdc::MAX_DRDC_BLOB_LEN as usize);
            limits.ifd_value_size = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
            decoder = decoder.with_limits(limits);
            let channels = match decoder
                .colortype()
                .map_err(|e| problem("depth", label, e))?
            {
                tiff::ColorType::Gray(16) => 1,
                // The `tiff` crate reports a two-sample BlackIsZero image as
                // Multiband rather than GrayA (its own comment says so); the
                // ExtraSamples rule below is what makes the second sample an
                // alpha rather than an unnamed band.
                tiff::ColorType::Multiband {
                    bit_depth: 16,
                    num_samples: 2,
                } => 2,
                tiff::ColorType::RGB(16) => 3,
                tiff::ColorType::RGBA(16) => 4,
                _ => {
                    return Err(problem(
                        "depth",
                        label,
                        "genuine unsigned Gray/GrayA/RGB/RGBA16 input is required",
                    ))
                }
            };
            let formats = decoder
                .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
                .map_err(|e| problem("depth", label, e))?;
            if formats.is_some_and(|v| v.iter().any(|&f| f != 1)) {
                return Err(problem(
                    "depth",
                    label,
                    "only unsigned integer samples are supported",
                ));
            }
            let planar = decoder
                .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
                .map_err(|e| problem("format", label, e))?
                .unwrap_or(1);
            if planar != 1 || decoder.more_images() {
                return Err(problem(
                    "format",
                    label,
                    "planar or multipage TIFF is unavailable",
                ));
            }
            let extras = decoder
                .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
                .map_err(|e| problem("alpha", label, e))?;
            // Spot channels (#1 1c-2a): trailing extra samples of 0 after
            // the colour and its straight alpha, admitted only when every
            // one is a kept spot channel (`tiff_spot`).
            let spot_extras = || {
                let tail = |v: &[u16]| !v.is_empty() && v.iter().all(|&e| e == 0);
                let shape = match (channels, extras.as_deref()) {
                    (3, Some(v)) => tail(v),
                    (4, Some([2, rest @ ..])) => tail(rest),
                    _ => false,
                };
                shape && crate::tiff_spot::tiff_spot_extras_accounted(bytes)
            };
            if ((channels == 4 && extras.as_deref() != Some(&[2][..]))
                || (channels == 3 && extras.as_ref().is_some_and(|v| !v.is_empty())))
                && !spot_extras()
            {
                return Err(problem(
                    "alpha",
                    label,
                    "RGBA TIFF must declare one unassociated alpha sample",
                ));
            }
            // The same rule one channel down (Phase 11D).
            if (channels == 2 && extras.as_deref() != Some(&[2][..]))
                || (channels == 1 && extras.is_some_and(|v| !v.is_empty()))
            {
                return Err(problem(
                    "alpha",
                    label,
                    "GrayA TIFF must declare one unassociated alpha sample",
                ));
            }
            // Read profile bytes directly: find_tag builds a Value enum per
            // byte and would multiply a valid 16 MiB profile into hundreds of MB.
            let mut ifd = decoder.image_ifd();
            let icc = if let Some(entry) = ifd.find_entry(Tag::IccProfile) {
                if !matches!(
                    entry.field_type(),
                    tiff::tags::Type::BYTE | tiff::tags::Type::UNDEFINED
                ) || entry.count() > u64::from(crate::drdc::MAX_DRDC_BLOB_LEN)
                {
                    return Err(problem("profile", label, "invalid or oversized ICC tag"));
                }
                let mut bytes = reserved_vec(entry.count() as usize)?;
                bytes.resize(entry.count() as usize, 0);
                let read = ifd
                    .find_tag_bytes(Tag::IccProfile, &mut bytes, 0)
                    .map_err(|e| problem("profile", label, e))?;
                if read != Some(bytes.len()) {
                    return Err(problem("profile", label, "incomplete ICC tag"));
                }
                Some(bytes)
            } else {
                None
            };
            let interpretation = if channels == 1 || channels == 2 {
                // Same order as the PNG branch: gray reads its profile through
                // `ProofProfile` and never reaches `IccProfile::parse`.
                Some(gray_interpretation(icc.as_deref(), label)?)
            } else {
                None
            };
            (
                Decoder::Tiff(decoder),
                stored,
                channels,
                PrecisionFormat::Tiff,
                icc,
                1,
                interpretation,
            )
        } else {
            return Err(problem(
                "format",
                label,
                "only 16-bit PNG and TIFF are supported; no byte fallback is permitted",
            ));
        };
    let (scan, scan_warnings) = metadata::scan_report(bytes);
    let mut warnings: Vec<String> = scan_warnings.into_iter().map(str::to_owned).collect();
    let profile = if let Some((_, warning)) = &interpretation {
        // Gray never reaches the RGB profile step: the document is sRGB and
        // the file's own curve is what `gray16_import` reads.
        warnings.push(warning.clone());
        IccProfile::srgb()
    } else if let Some(icc) = scan.icc.or(profile_bytes) {
        Arc::new(
            IccProfile::parse(&icc).ok_or_else(|| {
                problem("profile", label, "invalid or unsupported RGB image profile")
            })?,
        )
    } else {
        warnings.push(
            "RGB input without an ICC profile is interpreted as sRGB; no pixel conversion was performed".into(),
        );
        IccProfile::srgb()
    };
    let orientation = crate::metadata_tiff::orientation_of_file(bytes)
        .or_else(|| {
            scan.metadata
                .exif
                .as_deref()
                .and_then(crate::metadata_tiff::orientation)
        })
        .map_or(fallback_orientation, |o| o.to_exif());
    let swap = (5..=8).contains(&orientation);
    let (width, height) = if swap { (stored.1, stored.0) } else { stored };
    let resolution = scan.resolution.unwrap_or_default().transposed_if(swap);
    let count = u64::from(width) * u64::from(height);
    let plan = PrecisionDecodePlan {
        width,
        height,
        format,
        source_channels: channels,
        gray: interpretation.is_some(),
        orientation,
        // 34 B/px (formerly 32): the least per-pixel figure with
        // which `encoded + this` — the preserving open's one up-front
        // admission — is at least 1.25× the measured whole-open peak of a
        // 16-bit PNG at 12, 48, 67, 96 and 100 MP (binding at 96 and 100 MP;
        // the rows sit beside `HostStillDecoder.scratchFixedWide` in the
        // app). PNG and TIFF share it.
        scratch_bytes: 34 * count + DECODE_FIXED,
    };
    Ok(Header {
        decoder,
        stored,
        plan,
        gray: interpretation,
        profile,
        metadata: scan.metadata,
        resolution,
        warnings,
    })
}

/// The Gray profile contract, shared by both containers (Phase 11D): a
/// grayscale image carries a GRAY ICC profile or none at all. The bytes are
/// read with `icc_lut::ProofProfile` — never `IccProfile::parse`, which
/// models RGB matrix/TRC profiles and would refuse a Gray one — and must
/// present a device space of Gray with a tone curve; an sRGB or any other
/// RGB profile on a gray image is refused here rather than silently ignored,
/// because it is a claim about primaries the samples do not have.
pub(crate) fn gray_interpretation(
    icc: Option<&[u8]>,
    label: &str,
) -> Result<(GrayInterpretation, String), String> {
    let Some(icc) = icc else {
        return Ok((
            GrayInterpretation::SrgbCodes,
            "grayscale input without a Gray ICC profile is interpreted as sRGB-encoded gray; no pixel conversion was performed".into(),
        ));
    };
    let wrong = || {
        problem(
            "profile",
            label,
            "a grayscale image must carry a Gray ICC profile or none",
        )
    };
    let profile = ProofProfile::parse(icc).map_err(|_| wrong())?;
    if profile.device() != DeviceSpace::Gray {
        return Err(wrong());
    }
    let curve = profile.gray_curve().ok_or_else(wrong)?;
    // The warning NAMES the profile, because "converted through a curve" is
    // not a statement anyone can check and "converted through Gray Gamma
    // 2.2" is. It is the flat open's own sentence (`open_profile`'s S1), so
    // both opens of one file say the same thing and the host can show it
    // verbatim as the open's profile refusal.
    let warning = crate::open_profile::converted_sentence(profile.name(), "Gray");
    Ok((GrayInterpretation::GrayProfile(curve.clone()), warning))
}

/// Decodes genuine16-bit source samples. Input length is counted as retained;
/// include any spare input capacity in other_retained_bytes. The dependency
/// decoder reads its input polled under `control` (the module doc).
pub fn decode(
    bytes: &[u8],
    label: &str,
    working_limit_bytes: u64,
    other_retained_bytes: u64,
    control: Option<&DrJobControl>,
) -> Result<DecodedPrecision, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    decode_with_context(
        bytes,
        label,
        working_limit_bytes,
        other_retained_bytes,
        0,
        job,
    )
}

pub(crate) fn decode_with_context(
    bytes: &[u8],
    label: &str,
    limit: u64,
    other: u64,
    owned_input_bytes: u64,
    job: JobContext<'_>,
) -> Result<DecodedPrecision, String> {
    job.check()?;
    admit(bytes.len() as u64, DECODE_FIXED, other, limit)
        .map_err(|e| problem("capacity", label, e))?;
    job.reserve(DECODE_FIXED + owned_input_bytes)?;
    let mut header = settle(job, header(bytes, label, job))?;
    // One image too large to keep at 16 bits refuses in its own sentence
    // before the scratch admission and the decode.
    crate::native_sources::single_image_capacity((header.plan.width, header.plan.height), 16)?;
    admit(bytes.len() as u64, header.plan.scratch_bytes, other, limit)
        .map_err(|e| problem("capacity", label, e))?;
    job.reserve(header.plan.scratch_bytes + owned_input_bytes)?;
    job.report(0.1, 1)?;
    let pixels = match header.decoder {
        Decoder::Png(mut decoder) => {
            decoder
                .set_limits(png_limits(header.plan.scratch_bytes))
                .map_err(|e| problem("capacity", label, e))?;
            let mut raw = reserved_vec(decoder.total_bytes() as usize)?;
            raw.resize(decoder.total_bytes() as usize, 0);
            job.check()?;
            let read = decoder
                .read_image(&mut raw)
                .map_err(|e| problem("decode", label, e));
            settle(job, read)?;
            job.report(0.75, 1)?;
            let mut samples = reserved_vec(raw.len() / 2)?;
            for (i, b) in raw.chunks_exact(2).enumerate() {
                job.every(i)?;
                samples.push(u16::from_ne_bytes([b[0], b[1]]));
            }
            drop(raw);
            let (samples, channels) = degrayed(samples, &header.gray, &header.plan, job)?;
            upright(
                &samples,
                header.stored,
                channels,
                header.plan.orientation,
                job,
            )?
        }
        Decoder::Tiff(ref mut decoder) => {
            let read = decoder
                .read_image()
                .map_err(|e| problem("decode", label, e));
            let raw = settle(job, read)?;
            job.report(0.75, 1)?;
            let DecodingResult::U16(samples) = raw else {
                return Err(problem(
                    "depth",
                    label,
                    "decoder changed sample representation",
                ));
            };
            let (samples, channels) = degrayed(samples, &header.gray, &header.plan, job)?;
            upright(
                &samples,
                header.stored,
                channels,
                header.plan.orientation,
                job,
            )?
        }
    };
    job.check()?;
    let image = PrecisionImage::from_rgba16_decoded(
        header.plan.width,
        header.plan.height,
        pixels,
        header.profile,
        None,
    )?;
    job.report(1.0, 5)?;
    Ok(DecodedPrecision {
        image,
        metadata: header.metadata,
        resolution: header.resolution,
        warnings: header.warnings,
    })
}

pub fn open(
    path: &str,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<DecodedPrecision, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let bytes = read_regular(path, limit, other, job)?;
    let len = bytes.len() as u64;
    decode_with_context(&bytes, path, limit, other, len, job)
}

/// Reads a bounded regular file into an owned buffer, admitting its length
/// plus the fixed decode reservation first. Split out of [`open`] so
/// `PrecisionDocument::open_preserving` can look at the SAME bytes to decide
/// between the u16 codec and the float TIFF path without reading the file
/// twice (Phase 11F).
pub(crate) fn read_regular(
    path: &str,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    job.check()?;
    require_regular_input(path)?;
    let mut file = File::open(path).map_err(|e| problem("read", path, e))?;
    let len = file.metadata().map_err(|e| problem("read", path, e))?.len();
    if len > MAX_ENCODED_BYTES {
        return Err(problem("capacity", path, "encoded input exceeds 1 GiB"));
    }
    admit(len, DECODE_FIXED, other, limit).map_err(|e| problem("capacity", path, e))?;
    job.reserve(len + DECODE_FIXED)?;
    let mut bytes = reserved_vec(len as usize)?;
    let mut block = [0_u8; 65536];
    loop {
        job.check()?;
        let count = file
            .read(&mut block)
            .map_err(|e| problem("read", path, e))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > len as usize {
            return Err(problem("read", path, "file changed size while being read"));
        }
        bytes.extend_from_slice(&block[..count]);
    }
    if bytes.len() != len as usize {
        return Err(problem("read", path, "file changed size while being read"));
    }
    Ok(bytes)
}

/// Follow ordinary symlinks, but refuse pipes/devices/directories before an
/// open could block waiting for a producer. Filesystem latency is not bounded.
pub(crate) fn require_regular_input(path: &str) -> Result<(), String> {
    let metadata = std::fs::metadata(path).map_err(|e| problem("read", path, e))?;
    if !metadata.is_file() {
        return Err(problem("read", path, "input must be a regular image file"));
    }
    Ok(())
}

/// Gray in, RGB out — the ONE place `gray16_import` is reached from the
/// codec. An RGB source passes straight through, so the RGB path's samples
/// and its allocations are exactly what they were. The conversion runs
/// BEFORE `upright` because the interpretation is per sample and the
/// orientation permutation is per pixel: doing it the other way round would
/// mean writing a second permutation that knows about gray strides.
fn degrayed(
    samples: Vec<u16>,
    interpretation: &Option<(GrayInterpretation, String)>,
    plan: &PrecisionDecodePlan,
    job: JobContext<'_>,
) -> Result<(Vec<u16>, u8), String> {
    let Some((interpretation, _)) = interpretation else {
        return Ok((samples, plan.source_channels));
    };
    let encoder = crate::gray16_import::srgb_encoder()?;
    let pixels = crate::gray16_import::gray_to_rgba16(
        &samples,
        plan.source_channels,
        interpretation,
        &encoder,
        job,
    )?;
    drop(samples);
    let mut flat = reserved_vec(pixels.len() * 4)?;
    for (i, pixel) in pixels.iter().enumerate() {
        job.every(i)?;
        flat.extend_from_slice(pixel);
    }
    Ok((flat, 4))
}

fn upright(
    samples: &[u16],
    stored: (u32, u32),
    channels: u8,
    orientation: u8,
    job: JobContext<'_>,
) -> Result<Vec<[u16; 4]>, String> {
    let (w, h) = (stored.0 as usize, stored.1 as usize);
    if samples.len() != w * h * usize::from(channels) {
        return Err("high-depth decode: sample count differs from dimensions".into());
    }
    let (ow, oh) = crate::pixel_orientation::dimensions(w, h, orientation);
    let mut output = reserved_vec(w * h)?;
    // Exact EXIF permutation, equivalent to DynamicImage::apply_orientation;
    // directly assembles typed pixels instead of adding an intermediate image.
    for y in 0..oh {
        for x in 0..ow {
            job.every(y * ow + x)?;
            let (sx, sy) = crate::pixel_orientation::source(x, y, w, h, orientation);
            let at = (sy * w + sx) * usize::from(channels);
            output.push([
                samples[at],
                samples[at + 1],
                samples[at + 2],
                if channels == 4 {
                    samples[at + 3]
                } else {
                    65535
                },
            ]);
        }
    }
    job.check()?;
    Ok(output)
}

/// Conservative typed export scratch; retained document/image state is separate.
pub fn encode_scratch_bytes(image: &PrecisionImage) -> Result<u64, String> {
    if image.storage() != crate::precision_image::PrecisionStorage::Rgba16Unorm {
        return Err("high-depth depth: export requires explicit RGBA16 storage; float conversion is not implicit".into());
    }
    encode_dimensions_scratch(image.dimensions())
}

/// Metadata-only export admission, before rendering its authoritative image.
pub fn encode_dimensions_scratch(dimensions: (u32, u32)) -> Result<u64, String> {
    let count = pixel_count(dimensions.0, dimensions.1)? as u64;
    Ok(48 * count + 16 * u64::from(dimensions.0) + ENCODE_FIXED)
}

#[allow(clippy::too_many_arguments)]
pub fn save(
    image: &PrecisionImage,
    metadata: &Metadata,
    resolution: Resolution,
    path: &str,
    format: PrecisionFormat,
    embed_profile: bool,
    strip_metadata: bool,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<u32, String> {
    save_with_paths(
        image,
        metadata,
        resolution,
        path,
        format,
        embed_profile,
        strip_metadata,
        (limit, other),
        control,
        None,
    )
}

/// [`save`] for a document's TIFF: `paths`, when `Some`, are the path
/// resources it writes in place of the inherited run's (`tiff_paths`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_with_paths(
    image: &PrecisionImage,
    metadata: &Metadata,
    resolution: Resolution,
    path: &str,
    format: PrecisionFormat,
    embed_profile: bool,
    strip_metadata: bool,
    (limit, other): (u64, u64),
    control: Option<&DrJobControl>,
    paths: Option<Vec<u8>>,
) -> Result<u32, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    for packet in [&metadata.exif, &metadata.xmp, &metadata.iptc]
        .into_iter()
        .flatten()
    {
        if packet.len() > crate::drdc::MAX_DRDC_BLOB_LEN as usize {
            return Err(problem("capacity", path, "metadata packet exceeds 16 MiB"));
        }
    }
    let metadata_bytes: u64 = [&metadata.exif, &metadata.xmp, &metadata.iptc]
        .into_iter()
        .flatten()
        .map(|packet| packet.len() as u64)
        .sum();
    let scratch = encode_scratch_bytes(image)?;
    admit(
        image.retained_bytes() + metadata_bytes,
        scratch,
        other,
        limit,
    )
    .map_err(|e| problem("capacity", path, e))?;
    job.reserve(scratch)?;
    let (w, h) = image.dimensions();
    let (mut side, carried) = crate::doc_color::image_sidecar(
        image.profile(),
        metadata,
        resolution,
        format.legacy(),
        embed_profile,
        strip_metadata,
        (w, h),
    );
    if let Some(paths) = paths.filter(|_| format == PrecisionFormat::Tiff) {
        crate::tiff_paths::attach(&mut side, paths);
    }
    crate::dr_image::save_atomically(path, |temporary| {
        job.check()?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary)
            .map_err(|e| problem("write", path, e))?;
        let writer = BufWriter::new(file);
        let mut injector = if format == PrecisionFormat::Png {
            MetadataInjector::png(writer, side.png_chunks())
        } else {
            MetadataInjector::passthrough(writer)
        };
        let encoded = encode(image, &mut injector, format, &side, job)
            .map_err(|e| problem("encode", path, e));
        settle(job, encoded)?;
        let mut writer = injector.finish()?;
        writer.flush().map_err(|e| problem("write", path, e))?;
        job.check()
    })?;
    // The atomic rename committed the complete file. A cancellation arriving
    // after that commit cannot truthfully turn the successful save into an error.
    if let Some(control) = control {
        control.report(1.0, 5);
    }
    Ok(carried)
}

/// Encodes `image` into `writer`; the dependency encoder writes through
/// `cancel_io::Polled`, so `job` is polled as its bytes move, and a cancel
/// anywhere in here answers exactly `operation cancelled`.
pub(crate) fn encode<W: Write + Seek>(
    image: &PrecisionImage,
    writer: &mut W,
    format: PrecisionFormat,
    side: &EncodeSidecar<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let mut sink = Polled::new(writer, job);
    settle(job, encode_polled(image, &mut sink, format, side, job))
}

fn encode_polled<W: Write + Seek>(
    image: &PrecisionImage,
    writer: &mut W,
    format: PrecisionFormat,
    side: &EncodeSidecar<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let runs = image
        .runs_rgba16()
        .ok_or("high-depth depth: RGBA16 storage is required")?;
    job.report(0.1, 1)?;
    let (width, height) = image.dimensions();
    let count = width as usize * height as usize;
    let mut i = 0;
    match format {
        PrecisionFormat::Png => {
            let mut bytes = reserved_vec(count * 8)?;
            for run in runs {
                for pixel in run {
                    job.every(i)?;
                    i += 1;
                    for value in pixel {
                        bytes.extend_from_slice(&value.to_ne_bytes());
                    }
                }
            }
            let mut encoder = PngEncoder::new_with_quality(
                writer,
                CompressionType::Uncompressed,
                FilterType::NoFilter,
            );
            if let Some(icc) = side.icc {
                encoder
                    .set_icc_profile(icc.to_vec())
                    .map_err(|e| e.to_string())?;
            }
            if let Some(exif) = &side.exif {
                encoder
                    .set_exif_metadata(exif.clone())
                    .map_err(|e| e.to_string())?;
            }
            job.report(0.3, 1)?;
            encoder
                .write_image(
                    &bytes,
                    image.dimensions().0,
                    image.dimensions().1,
                    ExtendedColorType::Rgba16,
                )
                .map_err(|e| format!("high-depth PNG encoding failed: {e}"))?;
        }
        PrecisionFormat::Tiff => {
            let mut samples = reserved_vec(count * 4)?;
            for run in runs {
                for pixel in run {
                    job.every(i)?;
                    i += 1;
                    samples.extend_from_slice(pixel);
                }
            }
            job.report(0.3, 1)?;
            crate::tiff_encode::encode_rgba16(&samples, image.dimensions(), writer, side)?;
        }
    }
    job.check()
}
