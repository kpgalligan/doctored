//! Flat Lab TIFFs as Lab Color documents (#1 Lab phase 3a): which TIFFs
//! open as one ([`lab_source_at`], header work only) and the open itself
//! ([`PrecisionDocument::open_lab`]). `gray_io` is the template.
//!
//! # Which files
//!
//! A classic TIFF (`II` or `MM`, never BigTIFF) whose first directory is
//! PhotometricInterpretation 8 (CIELab, TIFF 6.0 §23) or 9 (ICCLab, TIFF
//! Technote 4) stored as a SHORT, chunky (PlanarConfiguration absent or 1), SamplesPerPixel 3,
//! or 4 with ExtraSamples 2 (an unassociated alpha), BitsPerSample 8 or 16
//! on every sample, SampleFormat absent, all 1, or (CIELab, whose a\*/b\*
//! are signed) [1, 2, 2] or [1, 2, 2, 1], and Compression none, LZW,
//! Deflate or PackBits. Every other file — an associated-alpha or planar
//! Lab TIFF among them — answers `None` and keeps today's path.
//!
//! # The open
//!
//! The `tiff` crate decodes Photometric 2 (RGB) only among these, so the
//! file is decoded through an [`Overlay`] reader that presents
//! Photometric 2 and SampleFormat 1 in the directory's own value bytes —
//! no copy of the file — and the raw samples come back unsigned. Each one
//! becomes the document's code through `lab_encode::storage_code`: CIELab
//! (measured against Photoshop by phase 3a's take) or ICCLab (declared from
//! the published spec, not measured against Photoshop); alpha is the 8-bit
//! code ×257 or the 16-bit value. Resolution, metadata and the EXIF
//! orientation are applied as `gray_io::open_gray` applies them, and the
//! paths are read from the 8BIM run (`tiff_paths`). The plan
//! is [`open_scratch`]: the Lab open's own measured charge
//! (`lab_limits::OPEN_BYTES_PER_PIXEL`, phase 3b) plus the wider decoded
//! samples. A multipage TIFF whose first image is page 1 opens each page
//! that matches page 1 as a hidden layer "Page n", as `open_gray` does.

use std::sync::Arc;

use tiff::decoder::{Decoder as TiffDecoder, DecodingResult};

use crate::job_control::{DrJobControl, JobContext};
use crate::lab_document::{LabDepth, LabPolicy};
use crate::lab_encode::{storage_code, LabFileEncoding};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage};
use crate::tiff_directory::{bytes_reader, file_reader, Directory, Overlay, COMPRESSIONS};
use crate::tile_store::TilePlane;

/// `dr_path_lab_source`'s answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabSource {
    Tiff = 1,
}

/// A flat Lab TIFF's facts ([`lab_source_at`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LabTiff {
    pub width: u32,
    pub height: u32,
    pub bits: u16,
    pub encoding: LabFileEncoding,
    pub alpha: bool,
}

/// What the header walk found: the facts and the byte patches the
/// [`Overlay`] applies (offset, replacement).
struct Header {
    tiff: LabTiff,
    patches: Vec<(u64, Vec<u8>)>,
}

/// The ONE header walk. `read(offset, length)` returns exactly `length`
/// bytes of the file or `None`; it is asked only for the header, the first
/// directory and its out-of-line SHORT arrays (`tiff_directory`).
fn walk(read: &mut dyn FnMut(u64, usize) -> Option<Vec<u8>>) -> Option<Header> {
    let found = Directory::first(read)?;
    let width = found.scalar(256)?;
    let height = found.scalar(257)?;
    let photometric = found.scalar(262)?;
    let encoding = match photometric {
        8 => LabFileEncoding::CieLab,
        9 => LabFileEncoding::IccLab,
        _ => return None,
    };
    let samples = found.scalar(277).unwrap_or(1);
    if !COMPRESSIONS.contains(&found.scalar(259).unwrap_or(1)) {
        return None;
    }
    if found.scalar(284).unwrap_or(1) != 1 {
        return None;
    }
    let bits: Vec<u16> = found
        .shorts(258, 64, read)??
        .into_iter()
        .map(|(v, _)| v)
        .collect();
    let extras: Vec<u16> = found
        .shorts(338, 64, read)?
        .unwrap_or_default()
        .into_iter()
        .map(|(v, _)| v)
        .collect();
    let alpha = match (samples, extras.as_slice()) {
        (3, []) => false,
        (4, [2]) => true,
        _ => return None,
    };
    let depth = *bits.first()?;
    if !matches!(depth, 8 | 16)
        || bits.len() != samples as usize
        || bits.iter().any(|&b| b != depth)
    {
        return None;
    }
    let mut patches = Vec::new();
    if let Some(formats) = found.shorts(339, 64, read)? {
        let values: Vec<u16> = formats.iter().map(|(v, _)| *v).collect();
        let signed = encoding == LabFileEncoding::CieLab
            && matches!(values.as_slice(), [1, 2, 2] | [1, 2, 2, 1]);
        if !signed && values.iter().any(|&v| v != 1) {
            return None;
        }
        for (value, at) in formats {
            if value != 1 {
                patches.push((at, found.short_bytes(1)));
            }
        }
    }
    // The patch is a SHORT, so a LONG Photometric (which `scalar` reads) is
    // not claimed and keeps today's open.
    let entry = found.entry(262)?;
    if entry.kind != 3 {
        return None;
    }
    patches.push((entry.at, found.short_bytes(2)));
    Some(Header {
        tiff: LabTiff {
            width,
            height,
            bits: depth,
            encoding,
            alpha,
        },
        patches,
    })
}

/// The header walk over bytes already read.
fn header(bytes: &[u8]) -> Option<Header> {
    walk(&mut bytes_reader(bytes))
}

/// Which flat Lab TIFF the file at `path` is (module doc), header work
/// only; `None` for every other file. `Err` only when the file cannot be
/// opened.
pub(crate) fn lab_source_at(path: &str) -> Result<Option<LabTiff>, String> {
    let mut read = file_reader(path)?;
    Ok(walk(&mut read).map(|h| h.tiff))
}

/// The open's working memory: the Lab open's own charge
/// (`lab_limits::OPEN_BYTES_PER_PIXEL` a pixel, measured; phase 3a charged
/// Gray's 41), the file and the bookkeeping, plus the decoded u16 samples
/// past 4 B a pixel — pixels × (samples × 2 − 4) — and, at 8 bits, the raw
/// bytes held while they are widened (pixels × samples).
fn open_scratch(
    width: u32,
    height: u32,
    encoded: u64,
    samples: u16,
    bits: u16,
) -> Result<u64, String> {
    let pixels = pixel_count(width, height)? as u64;
    let samples = u64::from(samples);
    let extra =
        pixels * (samples * 2).saturating_sub(4) + if bits == 8 { pixels * samples } else { 0 };
    Ok(pixels * crate::lab_limits::OPEN_BYTES_PER_PIXEL
        + encoded
        + crate::precision_image::BOOKKEEPING_BYTES
        + extra)
}

/// The admitted plan of opening a Lab TIFF of these facts from a file of
/// `encoded` bytes (the open's own [`open_scratch`]).
pub(crate) fn open_plan(tiff: &LabTiff, encoded: u64) -> Result<u64, String> {
    open_scratch(
        tiff.width,
        tiff.height,
        encoded,
        3 + u16::from(tiff.alpha),
        tiff.bits,
    )
}

impl PrecisionDocument {
    /// A flat Lab TIFF as a Lab Color document (module doc). `Err` for every
    /// file [`lab_source_at`] does not answer for.
    pub fn open_lab(
        path: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let mut bytes = crate::precision_codec::read_regular(path, limit, other, job)?;
        // A multipage TIFF whose first image is page 1 opens every page that
        // matches it (`doc_tiff_pages`).
        let pages = crate::tiff_pages::read(&bytes)
            .filter(crate::tiff_pages::Pages::first_image_is_page_one);
        let document = match pages {
            Some(pages) => {
                Self::stack_tiff_pages(&mut bytes, &pages, path, other, |page, other, _| {
                    Self::open_lab_bytes(page, path, limit, other, job)
                })?
            }
            None => Self::open_lab_bytes(&bytes, path, limit, other, job)?,
        };
        job.report(1.0, 5)?;
        Ok(document)
    }

    /// [`Self::open_lab`] of one file's bytes already read: a one-image
    /// file, or one page of a multipage TIFF made the file's only page
    /// (`tiff_pages::with_page`).
    fn open_lab_bytes(
        bytes: &[u8],
        path: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let fail = |what: String| format!("failed to decode {path}: {what}");
        let header =
            header(bytes).ok_or_else(|| fail("the file is not a Lab Color TIFF".into()))?;
        let facts = header.tiff;
        let samples = 3 + usize::from(facts.alpha);
        let scratch = open_plan(&facts, bytes.len() as u64)?;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        let overlay = Overlay::new(bytes, &header.patches);
        let decoder = TiffDecoder::new(overlay).map_err(|e| fail(e.to_string()))?;
        let mut limits = tiff::decoder::Limits::default();
        limits.decoding_buffer_size = ((u64::from(facts.width)
            * u64::from(facts.height)
            * samples as u64
            * u64::from(facts.bits / 8)) as usize)
            .max(crate::drdc::MAX_DRDC_BLOB_LEN as usize);
        limits.ifd_value_size = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
        let mut decoder = decoder.with_limits(limits);
        let stored = decoder.dimensions().map_err(|e| fail(e.to_string()))?;
        if stored != (facts.width, facts.height) {
            return Err(fail("the directory's dimensions disagree".into()));
        }
        job.check()?;
        let code = |channel: usize, sample: u16| -> u16 {
            if channel == 3 {
                if facts.bits == 8 {
                    sample * 257
                } else {
                    sample
                }
            } else {
                storage_code(facts.encoding, facts.bits, channel, sample)
            }
        };
        let codes: Vec<u16> = match decoder.read_image().map_err(|e| fail(e.to_string()))? {
            DecodingResult::U8(raw) if facts.bits == 8 => raw
                .iter()
                .enumerate()
                .map(|(i, &v)| code(i % samples, u16::from(v)))
                .collect(),
            DecodingResult::U16(mut raw) if facts.bits == 16 => {
                for (i, v) in raw.iter_mut().enumerate() {
                    *v = code(i % samples, *v);
                }
                raw
            }
            _ => return Err(fail("the TIFF's samples are not 8 or 16-bit".into())),
        };
        job.check()?;
        let (sw, sh) = (stored.0 as usize, stored.1 as usize);
        if codes.len() != sw * sh * samples {
            return Err(fail("the sample count differs from the dimensions".into()));
        }
        let (scan, scan_warnings) = crate::metadata::scan_report(bytes);
        let warnings: Vec<String> = scan_warnings.into_iter().map(str::to_owned).collect();
        let orientation = crate::metadata_tiff::orientation_of_file(bytes)
            .or_else(|| {
                scan.metadata
                    .exif
                    .as_deref()
                    .and_then(crate::metadata_tiff::orientation)
            })
            .map_or(1, |o| o.to_exif());
        let (w, h) = crate::pixel_orientation::dimensions(sw, sh, orientation);
        let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
            w as u32,
            h as u32,
            |y, row| {
                for (x, pixel) in row.iter_mut().enumerate() {
                    let (sx, sy) =
                        crate::pixel_orientation::source(x, y as usize, sw, sh, orientation);
                    let at = (sy * sw + sx) * samples;
                    let alpha = if samples == 4 { codes[at + 3] } else { 65535 };
                    *pixel = [codes[at], codes[at + 1], codes[at + 2], alpha];
                }
                job.every(y as usize * w)
            },
            job,
        )?;
        drop(codes);
        let depth = if facts.bits == 16 {
            LabDepth::Sixteen
        } else {
            LabDepth::Eight
        };
        let policy = LabPolicy::for_depth(depth);
        let image = PrecisionImage::from_rgba16_tiles_unseeded(plane, policy.twin().clone())?;
        let quarter = (5..=8).contains(&orientation);
        let resolution = scan
            .resolution
            .map(|r| r.transposed_if(quarter))
            .unwrap_or_default();
        PrecisionDocument::from_image(image, scan.metadata, resolution)?
            .with_import_warnings(warnings)?
            .with_lab(Arc::new(policy))?
            .with_tiff_paths(bytes)
    }
}
