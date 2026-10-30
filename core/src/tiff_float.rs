//! Floating-point TIFF: the pure decode/encode SEAM (Phase 11D).
//!
//! # The boundary, stated first
//!
//! This module has no FFI export of its own. The app reaches it through
//! Phase 11F's `float_io`, which `precision_io::open_preserving` routes a
//! genuine float TIFF to. A float TIFF is an HDR/scene-referred interchange
//! file, and deciding what a value above 1.0 or below 0.0 MEANS — tone
//! mapping, clipping, an HDR readout, a delivery transform — is the policy
//! `float_io` owns. This module answers only the question "can this crate
//! read and write the container exactly", which is answerable on its own. NaN and infinity
//! are therefore COUNTED ([`FloatTiff::non_finite`]) and passed through
//! unchanged rather than repaired: repairing them would be the policy this
//! slice does not have.
//!
//! # What is supported
//!
//! Decode: classic TIFF or BigTIFF, `SampleFormat = 3` (IEEE floating
//! point) at 16 or 32 bits, 1 to 4 samples per pixel, chunky, strips or
//! tiles, predictor 1 (none) or 3 (the
//! floating-point predictor), and whatever compression the `tiff` crate
//! decodes (None, Deflate, LZW, PackBits). Half samples are widened to f32 on
//! the way in, which is exact — every f16 is an f32.
//!
//! Encode: f32, or half through this module's own 16-bit colour types
//! (`half::f16::from_f32`: the nearest half, ties to even), predictor NONE,
//! 1 to 4 samples (gray + alpha, #547 phase 3c, through this module's own
//! `GrayA32Float` / `GrayA16Float`), optionally Deflate.
//! The `tiff` crate has no float-predictor ENCODER, so writing predictor 3
//! would mean writing the byte shuffle here; the acceptance criterion for
//! that (an owned writer checked against `tiffinfo`) is recorded in the 11D
//! status doc rather than guessed at now. F64 is refused in both directions:
//! a double-precision image is a scientific product, not a photograph.
//!
//! # Why the plan is separate from the decode
//!
//! [`inspect`] reads the header only and reports what a decode would cost, so
//! a caller admits the allocation before it happens — the same split
//! `precision_codec::inspect` / `decode` uses, for the same reason.
//!
//! **Phase 11F's `float_io.rs` is now its only in-crate caller and owns what a value above 1.0
//! MEANS** (the finite rule, the alpha convention and the profile requirement). Decodes
//! `SampleFormat = 3` at 16 or 32 bits (f16 widened to f32, which is exact), 1–4 samples, chunky,
//! strips or tiles, predictor 1 or 3, any compression the `tiff` crate reads; encodes f32 or half at 1 to
//! 4 samples with optional Deflate and predictor NONE (the crate has no float-predictor
//! ENCODER). NaN and infinity are COUNTED in `non_finite` and passed through unchanged — a
//! finite-value policy is 11F's. Every refusal names its element: SampleFormat ≠ 3, bits ∉ {16,
//! 32}, planar, > 4 samples, mixed widths, multipage, a bad `ExtraSamples`.

use std::io::{Seek, Write};

use tiff::decoder::{Decoder, DecodingResult};
use tiff::encoder::{colortype, Compression, TiffEncoder};
use tiff::tags::{PhotometricInterpretation, SampleFormat, Tag};

use crate::metadata::{Metadata, Resolution};
use crate::metadata_write::EncodeSidecar;
use crate::precision_image::{pixel_count, reserved_vec};

/// The two float widths this seam admits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatSampleType {
    F16,
    F32,
}

impl FloatSampleType {
    pub fn bits(self) -> u8 {
        match self {
            Self::F16 => 16,
            Self::F32 => 32,
        }
    }
}

/// What a float TIFF's header says, before a sample is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatTiffPlan {
    pub width: u32,
    pub height: u32,
    /// 1 (gray), 2 (gray + alpha), 3 (RGB) or 4 (RGBA).
    pub channels: u8,
    pub sample_type: FloatSampleType,
    /// `Some(premultiplied)` when the file declares an alpha sample through
    /// `ExtraSamples`, `None` when it declares none. 1 is associated
    /// (premultiplied) alpha and 2 is unassociated (straight), which is the
    /// same mapping the integer path reads.
    pub alpha: Option<bool>,
    pub compression: u16,
    pub predictor: u16,
    /// The decoded plane plus the codec's own working copy.
    pub scratch_bytes: u64,
}

/// One decoded float image: the samples as f32, interleaved, row 0 = top,
/// plus everything the container carried beside them.
#[derive(Clone, Debug)]
pub struct FloatTiff {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub alpha: Option<bool>,
    pub samples: Vec<f32>,
    pub icc: Option<Vec<u8>>,
    pub resolution: Option<Resolution>,
    pub metadata: Metadata,
    /// How many samples were NaN or infinite. They are in `samples`
    /// unchanged; this is the disclosure, not a repair.
    pub non_finite: u64,
}

fn problem(category: &str, label: &str, error: impl std::fmt::Display) -> String {
    format!("float TIFF {category} for {label}: {error}")
}

/// IFD0's PlanarConfiguration through the shared reader, `None` when the
/// tag is absent or the structure is unreadable (the `tiff` decoder then
/// reports whatever is actually wrong).
fn planar_configuration(bytes: &[u8]) -> Option<u32> {
    let tiff = crate::tiff_view::Tiff::file(bytes)?;
    let (entries, _) = tiff.ifd(tiff.ifd0)?;
    entries
        .iter()
        .find(|entry| entry.tag == 284)
        .and_then(|entry| tiff.scalar(entry))
}

/// Header-only inspection. Allocates no pixels.
pub fn inspect(bytes: &[u8], label: &str) -> Result<FloatTiffPlan, String> {
    Ok(plan_of(bytes, label)?.0)
}

/// The plan plus the decoder that produced it, so `decode` reads the header
/// once.
type Planned<'a> = (FloatTiffPlan, Decoder<std::io::Cursor<&'a [u8]>>);

fn plan_of<'a>(bytes: &'a [u8], label: &str) -> Result<Planned<'a>, String> {
    if !crate::metadata_tiff::is_tiff(bytes) {
        return Err(problem("format", label, "input is not a TIFF"));
    }
    // PlanarConfiguration is read through the crate's OWN shared TIFF reader,
    // before the `tiff` decoder is built: a planar file fails that
    // construction with a size complaint that names nothing.
    if planar_configuration(bytes) == Some(2) {
        return Err(problem("format", label, "planar TIFF is unavailable"));
    }
    let mut decoder =
        Decoder::new(std::io::Cursor::new(bytes)).map_err(|e| problem("decode", label, e))?;
    let (width, height) = decoder
        .dimensions()
        .map_err(|e| problem("decode", label, e))?;
    let count = pixel_count(width, height).map_err(|e| problem("capacity", label, e))? as u64;

    let formats = decoder
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .map_err(|e| problem("depth", label, e))?
        .unwrap_or_default();
    if formats.is_empty() || formats.iter().any(|&f| f != 3) {
        return Err(problem(
            "depth",
            label,
            "SampleFormat must be 3 (IEEE floating point) for every sample",
        ));
    }
    let bits = decoder
        .find_tag_unsigned_vec::<u16>(Tag::BitsPerSample)
        .map_err(|e| problem("depth", label, e))?
        .unwrap_or_default();
    if bits.is_empty() || bits.iter().any(|&b| b != bits[0]) {
        return Err(problem(
            "depth",
            label,
            "BitsPerSample must state one width for every sample",
        ));
    }
    let sample_type = match bits[0] {
        16 => FloatSampleType::F16,
        32 => FloatSampleType::F32,
        other => {
            return Err(problem(
                "depth",
                label,
                format!("{other}-bit float samples are unavailable; 16 and 32 are supported"),
            ))
        }
    };
    let samples = decoder
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
        .map_err(|e| problem("format", label, e))?
        .unwrap_or(1);
    if samples == 0 || samples > 4 {
        return Err(problem(
            "format",
            label,
            "SamplesPerPixel must be 1, 2, 3 or 4",
        ));
    }
    if formats.len() != 1 && formats.len() != usize::from(samples) {
        return Err(problem(
            "depth",
            label,
            "SampleFormat must state one value per sample",
        ));
    }
    if decoder.more_images() {
        return Err(problem("format", label, "multipage TIFF is unavailable"));
    }
    let extras = decoder
        .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
        .map_err(|e| problem("alpha", label, e))?
        .unwrap_or_default();
    let alpha = match extras.as_slice() {
        [] => None,
        [1] => Some(true),
        [2] => Some(false),
        _ => {
            return Err(problem(
                "alpha",
                label,
                "ExtraSamples must be absent or declare one associated (1) or unassociated (2) alpha sample",
            ))
        }
    };
    let compression = decoder
        .find_tag_unsigned::<u16>(Tag::Compression)
        .map_err(|e| problem("format", label, e))?
        .unwrap_or(1);
    let predictor = decoder
        .find_tag_unsigned::<u16>(Tag::Predictor)
        .map_err(|e| problem("format", label, e))?
        .unwrap_or(1);
    let plan = FloatTiffPlan {
        width,
        height,
        channels: samples as u8,
        sample_type,
        alpha,
        compression,
        predictor,
        // The decoder's own buffer at the stored width plus the f32 plane
        // this returns.
        scratch_bytes: count * u64::from(samples) * (u64::from(sample_type.bits()) / 8 + 4)
            + 1024 * 1024,
    };
    Ok((plan, decoder))
}

/// Decodes a float TIFF whose plan costs at most `limit` scratch bytes.
pub fn decode(bytes: &[u8], label: &str, limit: u64) -> Result<FloatTiff, String> {
    let (plan, mut decoder) = plan_of(bytes, label)?;
    if plan.scratch_bytes > limit {
        return Err(problem(
            "capacity",
            label,
            "decoding this float TIFF needs more working memory than the caller allowed",
        ));
    }
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = plan.scratch_bytes as usize;
    limits.ifd_value_size = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
    decoder = decoder.with_limits(limits);
    let samples = match decoder
        .read_image()
        .map_err(|e| problem("decode", label, e))?
    {
        DecodingResult::F32(values) => values,
        // Widening a half to a float is exact, so this is the ONE
        // representation change the seam makes and it loses nothing.
        DecodingResult::F16(values) => {
            let mut out = reserved_vec(values.len())?;
            for value in &values {
                out.push(value.to_f32());
            }
            out
        }
        _ => {
            return Err(problem(
                "depth",
                label,
                "decoder changed sample representation",
            ))
        }
    };
    let expected = u64::from(plan.width) * u64::from(plan.height) * u64::from(plan.channels);
    if samples.len() as u64 != expected {
        return Err(problem(
            "decode",
            label,
            "sample count differs from the declared dimensions",
        ));
    }
    let non_finite = samples.iter().filter(|v| !v.is_finite()).count() as u64;

    let mut ifd = decoder.image_ifd();
    let icc = if let Some(entry) = ifd.find_entry(Tag::IccProfile) {
        if !matches!(
            entry.field_type(),
            tiff::tags::Type::BYTE | tiff::tags::Type::UNDEFINED
        ) || entry.count() > u64::from(crate::drdc::MAX_DRDC_BLOB_LEN)
        {
            return Err(problem("profile", label, "invalid or oversized ICC tag"));
        }
        let mut icc = reserved_vec(entry.count() as usize)?;
        icc.resize(entry.count() as usize, 0);
        let read = ifd
            .find_tag_bytes(Tag::IccProfile, &mut icc, 0)
            .map_err(|e| problem("profile", label, e))?;
        if read != Some(icc.len()) {
            return Err(problem("profile", label, "incomplete ICC tag"));
        }
        Some(icc)
    } else {
        None
    };
    // The packets and the density come from the SHARED container scanner, so
    // a float TIFF's metadata is read by exactly the code every other TIFF's
    // is.
    let (scan, _) = crate::metadata::scan_report(bytes);
    Ok(FloatTiff {
        width: plan.width,
        height: plan.height,
        channels: plan.channels,
        alpha: plan.alpha,
        samples,
        icc,
        resolution: scan.resolution,
        metadata: scan.metadata,
        non_finite,
    })
}

/// [`encode`] into a fresh buffer with no sidecar — the form a caller that
/// only wants the bytes (a test, and 11F's first wiring) asks for. The
/// sidecar-carrying entry point stays crate-internal because
/// `EncodeSidecar` is.
pub fn encode_to_vec(image: &FloatTiff, deflate: bool) -> Result<Vec<u8>, String> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    encode(
        image,
        &mut bytes,
        deflate,
        FloatSampleType::F32,
        &EncodeSidecar::NONE,
    )?;
    Ok(bytes.into_inner())
}

/// [`encode_to_vec`] at HALF width: each sample is the nearest IEEE half to
/// its f32 (ties to even, a magnitude past 65504 becoming infinity), which is
/// what `half::f16::from_f32` computes.
pub fn encode_half_to_vec(image: &FloatTiff, deflate: bool) -> Result<Vec<u8>, String> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    encode(
        image,
        &mut bytes,
        deflate,
        FloatSampleType::F16,
        &EncodeSidecar::NONE,
    )?;
    Ok(bytes.into_inner())
}

/// The half-float colour types the `tiff` crate does not ship: `Inner` is the
/// half's 16 BITS, declared `SampleFormat = 3` at 16 bits a sample, so the
/// encoder writes the words as they are and a reader sees IEEE halves.
/// Predictor NONE, as at 32 bits (module doc), so `horizontal_predict` is
/// never called.
macro_rules! half_color_type {
    ($name:ident, $photometric:ident, $count:expr) => {
        struct $name;
        impl colortype::ColorType for $name {
            type Inner = u16;
            const TIFF_VALUE: PhotometricInterpretation = PhotometricInterpretation::$photometric;
            const BITS_PER_SAMPLE: &'static [u16] = &[16; $count];
            const SAMPLE_FORMAT: &'static [SampleFormat] = &[SampleFormat::IEEEFP; $count];
            fn horizontal_predict(_: &[u16], _: &mut Vec<u16>) {
                unreachable!()
            }
        }
    };
}
half_color_type!(Gray16Float, BlackIsZero, 1);
half_color_type!(GrayA16Float, BlackIsZero, 2);
half_color_type!(Rgb16Float, RGB, 3);
half_color_type!(Rgba16Float, RGB, 4);

/// Gray + alpha at 32 bits a sample (#547 phase 3c), which the `tiff` crate
/// does not ship: BlackIsZero, two IEEE floats, predictor NONE as at every
/// width here, so `horizontal_predict` is never called.
struct GrayA32Float;
impl colortype::ColorType for GrayA32Float {
    type Inner = f32;
    const TIFF_VALUE: PhotometricInterpretation = PhotometricInterpretation::BlackIsZero;
    const BITS_PER_SAMPLE: &'static [u16] = &[32; 2];
    const SAMPLE_FORMAT: &'static [SampleFormat] = &[SampleFormat::IEEEFP; 2];
    fn horizontal_predict(_: &[f32], _: &mut Vec<f32>) {
        unreachable!()
    }
}

/// Writes `image` as a float TIFF at `samples` width, optionally
/// Deflate-compressed.
/// Predictor NONE: there is no float-predictor encoder in the `tiff` crate
/// and this module does not grow one (see the module doc).
pub(crate) fn encode<W: Write + Seek>(
    image: &FloatTiff,
    writer: &mut W,
    deflate: bool,
    samples: FloatSampleType,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    let expected = u64::from(image.width) * u64::from(image.height) * u64::from(image.channels);
    if image.width == 0 || image.height == 0 || image.samples.len() as u64 != expected {
        return Err("float TIFF encode: sample count differs from the declared dimensions".into());
    }
    let halves = match samples {
        FloatSampleType::F32 => None,
        FloatSampleType::F16 => {
            let mut words: Vec<u16> = reserved_vec(image.samples.len())?;
            for value in &image.samples {
                words.push(half::f16::from_f32(*value).to_bits());
            }
            Some(words)
        }
    };
    let run = || -> tiff::TiffResult<()> {
        let mut encoder = TiffEncoder::new(writer)?;
        if deflate {
            encoder = encoder.with_compression(Compression::Deflate(Default::default()));
        }
        match (&halves, image.channels) {
            (None, 1) => {
                write_image::<W, colortype::Gray32Float>(encoder, image, side, &image.samples)
            }
            (None, 2) => write_image::<W, GrayA32Float>(encoder, image, side, &image.samples),
            (None, 3) => {
                write_image::<W, colortype::RGB32Float>(encoder, image, side, &image.samples)
            }
            (None, _) => {
                write_image::<W, colortype::RGBA32Float>(encoder, image, side, &image.samples)
            }
            (Some(words), 1) => write_image::<W, Gray16Float>(encoder, image, side, words),
            (Some(words), 2) => write_image::<W, GrayA16Float>(encoder, image, side, words),
            (Some(words), 3) => write_image::<W, Rgb16Float>(encoder, image, side, words),
            (Some(words), _) => write_image::<W, Rgba16Float>(encoder, image, side, words),
        }
    };
    run().map_err(|e| format!("float TIFF encode failed: {e}"))
}

fn write_image<W: Write + Seek, C: colortype::ColorType>(
    mut encoder: TiffEncoder<&mut W>,
    image: &FloatTiff,
    side: &EncodeSidecar<'_>,
    data: &[C::Inner],
) -> tiff::TiffResult<()>
where
    [C::Inner]: tiff::encoder::TiffValue,
{
    let mut out = encoder.new_image::<C>(image.width, image.height)?;
    let tags = out.encoder();
    if let Some(icc) = side.icc {
        tags.write_tag(Tag::Unknown(34675), icc)?;
    }
    if let Some(xmp) = &side.xmp {
        tags.write_tag(Tag::Unknown(700), xmp.as_slice())?;
    }
    let dpi = side.dpi.unwrap_or_default().sane();
    let rational = |ppi: f32| tiff::encoder::Rational {
        n: (f64::from(ppi) * 10_000.0).round() as u32,
        d: 10_000,
    };
    tags.write_tag(Tag::XResolution, rational(dpi.x))?;
    tags.write_tag(Tag::YResolution, rational(dpi.y))?;
    tags.write_tag(Tag::ResolutionUnit, 2u16)?;
    tags.write_tag(Tag::Orientation, 1u16)?;
    if matches!(image.channels, 2 | 4) {
        // The alpha sample is already counted by the colour type; this
        // states what it MEANS, exactly as the integer encoder does.
        let kind: u16 = if image.alpha == Some(true) { 1 } else { 2 };
        tags.write_tag(Tag::ExtraSamples, &[kind][..])?;
    }
    out.write_data(data)
}
