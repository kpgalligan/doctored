//! Gray files as Grayscale documents (#1 phase 1a): which PNG, TIFF and
//! JPEG files open as one ([`gray_source`], header work only) and the open
//! itself ([`PrecisionDocument::open_gray`]).
//!
//! # Which files
//!
//! Exactly the files `open_gray` accepts, decided from the bytes before the
//! first IDAT, strip or SOS (as `cmyk_source::cmyk_source_at`):
//!
//! * PNG colour type 0 or 4 at bit depth 8 or 16, no `cICP`/`mDCV`/`cLLI`,
//!   and the gray profile contract the typed open already applies — no
//!   `iCCP`, or one `ProofProfile` reads as Gray with a tone curve
//!   (`precision_codec::gray_interpretation`); with neither an `iCCP` nor
//!   `sRGB`, a usable bare `gAMA` is the Gray profile it describes
//!   (`png_declared`) and an unusable one refuses
//!   (`precision_png::png_profile_contract`).
//! * TIFF IFD0 Photometric BlackIsZero, SamplesPerPixel 1 or 2 (the second
//!   one declared an unassociated alpha, chunky), BitsPerSample 8 or 16,
//!   SampleFormat unsigned (absent counts as unsigned), its ICC absent or
//!   Gray as above.
//! * JPEG with one component at 8-bit precision, its APP2 ICC absent or Gray.
//!
//! Every other file answers `None` and keeps today's path, so a gray PNG
//! with an RGB `iCCP` or an unusable `gAMA` refuses (16-bit) or opens flat
//! (8-bit).
//!
//! # The open
//!
//! Codes are copied into R = G = B exactly (8-bit ×257). A usable embedded
//! Gray ICC, or the one a bare `gAMA` describes, is kept verbatim
//! (`GrayPolicy`); a PNG `sRGB` chunk is sGray; no profile at all is the
//! working Gray, disclosed by [`untagged_sentence`].
//! The EXIF orientation is applied as the flat open applies it. A TIFF's
//! paths are read and a turned JPEG's path records dropped (`tiff_paths`).
//! A multipage
//! TIFF whose first image is page 1 (`tiff_pages`) opens each page that
//! matches page 1 as a hidden layer "Page n" (`doc_tiff_pages`), each page
//! read by this same open.

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;

use image::{ColorType, ImageDecoder};
use tiff::decoder::{Decoder as TiffDecoder, DecodingResult};
use tiff::tags::Tag;

use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::gray_working::GrayWorkingSpace;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use crate::tile_store::TilePlane;

/// `dr_path_gray_source`'s answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GraySource {
    Png = 1,
    Tiff = 2,
    Jpeg = 3,
}

/// Bytes a canvas pixel the open charges beside the encoded file: the
/// decoded samples (up to 4 B/px), the RGBA16 tiles (8 B/px) and the opened
/// document's display preparation the host runs right after.
/// `scripts/gray-admission/measure.py` holds every open plan to ⌈1.25 × its
/// measured peak⌉ (40 missed `open-png16` and `open-png8` at 24 MP, 770 MB
/// measured, by 0.1 %, so it was raised to 41); a miss raises this term.
pub(crate) const OPEN_BYTES_PER_PIXEL: u64 = 41;

/// The sentence an untagged gray file opens under.
pub(crate) fn untagged_sentence(name: &str) -> String {
    format!(
        "The image is grayscale with no usable Gray profile, so the working Gray “{name}” was assigned as it was read."
    )
}

const PNG_SIGNATURE: &[u8; 8] = &crate::metadata::PNG_SIGNATURE;

/// The most PNG header bytes [`png_head_from`] collects: an `iCCP` holds a
/// profile of at most `MAX_DRDC_BLOB_LEN`, and a real header stays far below.
const PNG_HEADER_CAP: usize = 32 << 20;

/// Which gray container `bytes` is (module doc); `None` for every other file.
pub(crate) fn gray_source(bytes: &[u8]) -> Option<GraySource> {
    if bytes.starts_with(PNG_SIGNATURE) {
        return png_source(png_head(bytes)?).then_some(GraySource::Png);
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg_source(bytes).then_some(GraySource::Jpeg);
    }
    if crate::metadata_tiff::is_tiff(bytes) {
        return tiff_source(Cursor::new(bytes)).then_some(GraySource::Tiff);
    }
    None
}

/// [`gray_source`] for the file at `path`, header work only. `Err` only when
/// the file cannot be opened.
pub(crate) fn gray_source_at(path: &str) -> Result<Option<GraySource>, String> {
    let mut file = File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let mut magic = [0u8; 8];
    if file.read_exact(&mut magic).is_err() {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("failed to read {path}: {e}"))?;
    if &magic == PNG_SIGNATURE {
        let head = png_head_from(&mut BufReader::new(file));
        return Ok(head
            .filter(|head| png_source(head))
            .map(|_| GraySource::Png));
    }
    if magic.starts_with(&[0xFF, 0xD8]) {
        let header = crate::cmyk_source::jpeg_header(&mut BufReader::new(file));
        return Ok(header
            .filter(|header| jpeg_source(header))
            .map(|_| GraySource::Jpeg));
    }
    if !crate::metadata_tiff::is_tiff(&magic) {
        return Ok(None);
    }
    Ok(tiff_source(BufReader::new(file)).then_some(GraySource::Tiff))
}

/// A PNG's signature and every chunk before its first IDAT, from the whole
/// file; `None` for a truncated chunk run.
pub(crate) fn png_head(bytes: &[u8]) -> Option<&[u8]> {
    let mut at = 8usize;
    loop {
        let head = bytes.get(at..at + 8)?;
        if &head[4..8] == b"IDAT" || &head[4..8] == b"IEND" {
            return Some(&bytes[..at]);
        }
        let size = u32::from_be_bytes(head[..4].try_into().ok()?) as usize;
        at = at.checked_add(size)?.checked_add(12)?;
        if at > bytes.len() {
            return None;
        }
    }
}

/// [`png_head`] read from a file, chunk by chunk, never past the first IDAT.
pub(crate) fn png_head_from(reader: &mut impl Read) -> Option<Vec<u8>> {
    let mut out = vec![0u8; 8];
    reader.read_exact(&mut out).ok()?;
    loop {
        let mut head = [0u8; 8];
        reader.read_exact(&mut head).ok()?;
        if &head[4..8] == b"IDAT" || &head[4..8] == b"IEND" {
            return Some(out);
        }
        let size = u32::from_be_bytes(head[..4].try_into().ok()?) as usize;
        let total = size.checked_add(12)?;
        if out.len() + total > PNG_HEADER_CAP {
            return None;
        }
        out.extend_from_slice(&head);
        let at = out.len();
        out.resize(at + size + 4, 0);
        reader.read_exact(&mut out[at..]).ok()?;
    }
}

/// A PNG chunk's type and data, in order (the head's chunks only).
pub(crate) fn png_chunk_data(head: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    let mut at = 8usize;
    while let Some(chunk) = head.get(at..at + 8) {
        let size = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize;
        let Some(data) = head.get(at + 8..at + 8 + size) else {
            break;
        };
        out.push((&chunk[4..8], data));
        at += size + 12;
    }
    out
}

/// The facts of a gray PNG head: (bit depth, has `sRGB`, the `iCCP` profile
/// or the one a bare `gAMA` describes).
type PngFacts = (u8, bool, Option<Vec<u8>>);

/// The head's facts when it is a gray PNG `open_gray` accepts (module doc).
fn png_facts(head: &[u8]) -> Option<PngFacts> {
    if head.len() < 33 || &head[12..16] != b"IHDR" {
        return None;
    }
    let (depth, colour) = (head[24], head[25]);
    if !matches!(depth, 8 | 16) || !matches!(colour, 0 | 4) {
        return None;
    }
    let chunks = png_chunk_data(head);
    let srgb = chunks.iter().any(|(kind, _)| *kind == b"sRGB");
    let icc = match chunks.iter().find(|(kind, _)| *kind == b"iCCP") {
        Some((_, data)) => Some(iccp_profile(data)?),
        None => crate::png_declared::profile(head, true),
    };
    crate::precision_codec::gray_interpretation(icc.as_deref(), "the file").ok()?;
    crate::precision_png::png_profile_contract(head, icc.is_some(), true).ok()?;
    Some((depth, srgb, icc))
}

fn png_source(head: &[u8]) -> bool {
    png_facts(head).is_some()
}

/// An `iCCP` chunk's profile: a 1–79 byte name, NUL, method 0, then zlib.
pub(crate) fn iccp_profile(data: &[u8]) -> Option<Vec<u8>> {
    let nul = data.iter().take(80).position(|&b| b == 0)?;
    if nul == 0 || data.get(nul + 1) != Some(&0) {
        return None;
    }
    crate::psd_zip::inflate_zlib_bounded(&data[nul + 2..], crate::drdc::MAX_DRDC_BLOB_LEN as usize)
        .ok()
}

/// A JPEG header's frame: one component at 8-bit precision, and its APP2 ICC
/// absent or Gray.
fn jpeg_source(bytes: &[u8]) -> bool {
    let Some((precision, components)) = jpeg_frame(bytes) else {
        return false;
    };
    if precision != 8 || components != 1 {
        return false;
    }
    let icc = crate::dr_image::embedded_icc(bytes);
    crate::precision_codec::gray_interpretation(icc.as_deref(), "the file").is_ok()
}

/// The first SOF segment's (precision, component count).
fn jpeg_frame(bytes: &[u8]) -> Option<(u8, u8)> {
    let mut at = 2usize;
    loop {
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let mut marker = *bytes.get(at + 1)?;
        while marker == 0xFF {
            at += 1;
            marker = *bytes.get(at + 1)?;
        }
        at += 2;
        match marker {
            0x01 | 0xD0..=0xD7 => continue,
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return Some((*bytes.get(at + 2)?, *bytes.get(at + 7)?));
        }
        at += length;
    }
}

/// IFD0 of a gray TIFF `open_gray` accepts (module doc).
fn tiff_source<R: Read + Seek>(reader: R) -> bool {
    tiff_icc(reader).is_some()
}

/// IFD0's ICC (`Some(None)` when absent) when it is a gray TIFF `open_gray`
/// accepts; `None` otherwise.
pub(crate) fn tiff_icc<R: Read + Seek>(reader: R) -> Option<Option<Vec<u8>>> {
    let decoder = TiffDecoder::new(reader).ok()?;
    let mut limits = tiff::decoder::Limits::default();
    limits.ifd_value_size = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
    let mut decoder = decoder.with_limits(limits);
    let photometric = decoder
        .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
        .ok()??;
    let samples = decoder
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
        .ok()?
        .unwrap_or(1);
    let bits = decoder
        .find_tag_unsigned_vec::<u16>(Tag::BitsPerSample)
        .ok()?
        .unwrap_or_else(|| vec![1]);
    let formats = decoder
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .ok()?;
    let extras = decoder
        .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
        .ok()?;
    let planar = decoder
        .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
        .ok()?
        .unwrap_or(1);
    if photometric != 1 || !matches!(samples, 1 | 2) {
        return None;
    }
    let depth = *bits.first()?;
    if !matches!(depth, 8 | 16) || bits.iter().any(|&b| b != depth) {
        return None;
    }
    if formats.is_some_and(|v| v.iter().any(|&f| f != 1)) {
        return None;
    }
    if samples == 2 && (extras.as_deref() != Some(&[2][..]) || planar != 1) {
        return None;
    }
    if samples == 1 && extras.is_some_and(|v| !v.is_empty()) {
        return None;
    }
    let icc = decoder.get_tag_u8_vec(Tag::IccProfile).ok();
    crate::precision_codec::gray_interpretation(icc.as_deref(), "the file").ok()?;
    Some(icc)
}

/// The decoded gray samples: `channels` (1 or 2) u16 samples a pixel,
/// 8-bit codes already ×257.
pub(crate) struct GraySamples {
    pub(crate) samples: Vec<u16>,
    pub(crate) channels: usize,
    pub(crate) stored: (u32, u32),
    pub(crate) depth: GrayDepth,
}

/// The open's working memory: [`OPEN_BYTES_PER_PIXEL`] a pixel, the encoded
/// file and bookkeeping. New Grayscale charges it with no file
/// (`PrecisionDocument::new_gray`).
pub(crate) fn open_scratch(width: u32, height: u32, encoded: u64) -> Result<u64, String> {
    Ok(pixel_count(width, height)? as u64 * OPEN_BYTES_PER_PIXEL + encoded + BOOKKEEPING_BYTES)
}

fn widened(raw: Vec<u8>, sixteen: bool) -> Vec<u16> {
    if sixteen {
        raw.chunks_exact(2)
            .map(|b| u16::from_ne_bytes([b[0], b[1]]))
            .collect()
    } else {
        raw.into_iter().map(|v| u16::from(v) * 257).collect()
    }
}

/// The decoded samples of a gray file `gray_source` answered `source` for,
/// admitted as [`open_scratch`] first (`open_gray`, and the clipboard's gray
/// TIFF, `gray_clipboard`).
pub(crate) fn decode_samples(
    bytes: &[u8],
    source: GraySource,
    path: &str,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<GraySamples, String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    let admit_open = |(w, h): (u32, u32)| -> Result<(), String> {
        let scratch = open_scratch(w, h, bytes.len() as u64)?;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)
    };
    match source {
        GraySource::Png | GraySource::Jpeg => {
            let decoder: Box<dyn image::ImageDecoder> = if source == GraySource::Png {
                Box::new(
                    image::codecs::png::PngDecoder::with_limits(
                        Cursor::new(bytes),
                        crate::precision_codec::png_limits(u64::MAX / 4),
                    )
                    .map_err(|e| fail(e.to_string()))?,
                )
            } else {
                Box::new(
                    image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))
                        .map_err(|e| fail(e.to_string()))?,
                )
            };
            let stored = decoder.dimensions();
            admit_open(stored)?;
            let (channels, sixteen) = match decoder.color_type() {
                ColorType::L8 => (1, false),
                ColorType::La8 => (2, false),
                ColorType::L16 => (1, true),
                ColorType::La16 => (2, true),
                _ => return Err(fail("the image is not grayscale".into())),
            };
            let mut raw = crate::precision_image::reserved_vec(decoder.total_bytes() as usize)?;
            raw.resize(decoder.total_bytes() as usize, 0);
            job.check()?;
            decoder
                .read_image(&mut raw)
                .map_err(|e| fail(e.to_string()))?;
            job.check()?;
            Ok(GraySamples {
                samples: widened(raw, sixteen),
                channels,
                stored,
                depth: if sixteen {
                    GrayDepth::Sixteen
                } else {
                    GrayDepth::Eight
                },
            })
        }
        GraySource::Tiff => {
            let mut decoder =
                TiffDecoder::new(Cursor::new(bytes)).map_err(|e| fail(e.to_string()))?;
            let stored = decoder.dimensions().map_err(|e| fail(e.to_string()))?;
            admit_open(stored)?;
            let mut limits = tiff::decoder::Limits::default();
            limits.decoding_buffer_size = ((u64::from(stored.0) * u64::from(stored.1) * 4)
                as usize)
                .max(crate::drdc::MAX_DRDC_BLOB_LEN as usize);
            limits.ifd_value_size = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
            let mut decoder = decoder.with_limits(limits);
            let channels = decoder
                .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
                .map_err(|e| fail(e.to_string()))?
                .unwrap_or(1) as usize;
            job.check()?;
            let (samples, depth) = match decoder.read_image().map_err(|e| fail(e.to_string()))? {
                DecodingResult::U8(raw) => (widened(raw, false), GrayDepth::Eight),
                DecodingResult::U16(raw) => (raw, GrayDepth::Sixteen),
                _ => return Err(fail("the TIFF's samples are not 8 or 16-bit".into())),
            };
            job.check()?;
            Ok(GraySamples {
                samples,
                channels,
                stored,
                depth,
            })
        }
    }
}

impl PrecisionDocument {
    /// A gray PNG, TIFF or JPEG as a Grayscale document (module doc).
    /// `working` is the working Gray's wire name (Color Settings), assigned
    /// to a file with no profile. `Err` for every file [`gray_source`] does
    /// not answer for.
    pub fn open_gray(
        path: &str,
        working: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let working = GrayWorkingSpace::from_name(working)
            .ok_or_else(|| format!("\"{working}\" is not a Gray working space"))?;
        let mut bytes = crate::precision_codec::read_regular(path, limit, other, job)?;
        // A multipage TIFF whose first image is page 1 opens every page that
        // matches it (`doc_tiff_pages`).
        let pages = crate::metadata_tiff::is_tiff(&bytes)
            .then(|| crate::tiff_pages::read(&bytes))
            .flatten()
            .filter(crate::tiff_pages::Pages::first_image_is_page_one);
        let document = match pages {
            Some(pages) => {
                Self::stack_tiff_pages(&mut bytes, &pages, path, other, |page, other, _| {
                    Self::open_gray_bytes(page, path, working, limit, other, job)
                })?
            }
            None => Self::open_gray_bytes(&bytes, path, working, limit, other, job)?,
        };
        job.report(1.0, 5)?;
        Ok(document)
    }

    /// [`Self::open_gray`] of one file's bytes already read: a one-image
    /// file, or one page of a multipage TIFF made the file's only page
    /// (`tiff_pages::with_page`).
    fn open_gray_bytes(
        bytes: &[u8],
        path: &str,
        working: GrayWorkingSpace,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let source = gray_source(bytes).ok_or_else(|| {
            format!("failed to decode {path}: the file is not a grayscale PNG, TIFF or JPEG")
        })?;
        let (icc, srgb) = match source {
            GraySource::Png => {
                let head = png_head(bytes).ok_or_else(|| format!("failed to decode {path}"))?;
                let (_, srgb, icc) =
                    png_facts(head).ok_or_else(|| format!("failed to decode {path}"))?;
                (icc, srgb)
            }
            GraySource::Tiff => (tiff_icc(Cursor::new(bytes)).flatten(), false),
            GraySource::Jpeg => (crate::dr_image::embedded_icc(bytes), false),
        };
        let decoded = decode_samples(bytes, source, path, limit, other, job)?;
        let mut warnings = Vec::new();
        let policy = match (icc, srgb) {
            (Some(icc), _) => GrayPolicy::new(&icc, decoded.depth)?,
            (None, true) => GrayPolicy::for_space(GrayWorkingSpace::SGray, decoded.depth)?,
            (None, false) => {
                let policy = GrayPolicy::for_space(working, decoded.depth)?;
                warnings.push(untagged_sentence(policy.name()));
                policy
            }
        };
        let (scan, scan_warnings) = crate::metadata::scan_report(bytes);
        warnings.extend(scan_warnings.into_iter().map(str::to_owned));
        let orientation = crate::metadata_tiff::orientation_of_file(bytes)
            .or_else(|| {
                scan.metadata
                    .exif
                    .as_deref()
                    .and_then(crate::metadata_tiff::orientation)
            })
            .map_or(1, |o| o.to_exif());
        // A JPEG's 8BIM run keeps no path record through an open that
        // turned it (`tiff_paths::jpeg_opened`); a TIFF's are
        // `with_tiff_paths`' below.
        let metadata = match source {
            GraySource::Jpeg => {
                let (metadata, turned) = crate::tiff_paths::jpeg_opened(scan.metadata, orientation);
                warnings.extend(turned);
                metadata
            }
            GraySource::Png | GraySource::Tiff => scan.metadata,
        };
        let (sw, sh) = (decoded.stored.0 as usize, decoded.stored.1 as usize);
        if decoded.samples.len() != sw * sh * decoded.channels {
            return Err(format!(
                "failed to decode {path}: the sample count differs from the dimensions"
            ));
        }
        let (w, h) = crate::pixel_orientation::dimensions(sw, sh, orientation);
        let channels = decoded.channels;
        let samples = &decoded.samples;
        let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
            w as u32,
            h as u32,
            |y, row| {
                for (x, pixel) in row.iter_mut().enumerate() {
                    let (sx, sy) =
                        crate::pixel_orientation::source(x, y as usize, sw, sh, orientation);
                    let at = (sy * sw + sx) * channels;
                    let gray = samples[at];
                    let alpha = if channels == 2 {
                        samples[at + 1]
                    } else {
                        65535
                    };
                    *pixel = [gray, gray, gray, alpha];
                }
                job.every(y as usize * w)
            },
            job,
        )?;
        drop(decoded);
        let image = PrecisionImage::from_rgba16_tiles_unseeded(plane, policy.twin().clone())?;
        let quarter = (5..=8).contains(&orientation);
        let resolution = scan
            .resolution
            .map(|r| r.transposed_if(quarter))
            .unwrap_or_default();
        let document = PrecisionDocument::from_image(image, metadata, resolution)?
            .with_import_warnings(warnings)?
            .with_gray(Arc::new(policy))?;
        match source {
            GraySource::Tiff => document.with_tiff_paths(bytes),
            GraySource::Png | GraySource::Jpeg => Ok(document),
        }
    }
}
