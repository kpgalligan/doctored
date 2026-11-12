//! 1-bit PNG, TIFF and BMP files as Bitmap documents (#1 Bitmap phase
//! 1c): which files open as one ([`bitmap_source_at`], header work only,
//! `bitmap_codec`'s claim) and the open itself
//! ([`PrecisionDocument::open_bitmap`]), with the plane-to-document step the
//! PSD open shares ([`PrecisionDocument::from_bitmap_plane`]).
//!
//! # The open
//!
//! The file is read whole (`precision_codec::read_regular`) and admitted as
//! [`open_scratch`] before its rows decode (`bitmap_codec::decode`). The
//! EXIF or TIFF orientation is applied to the bits as `indexed_io` applies
//! it to the indices. The resolution is the file's (`metadata::scan_report`),
//! or a BMP's pixels per metre. A TIFF's paths are read from its 8BIM run
//! (`tiff_paths`). The profile is the file's Gray ICC, else
//! sGray for a PNG `sRGB` chunk, else the working Gray, with NO untagged
//! warning: Photoshop's Bitmap mode carries no profile, so a 1-bit file
//! without one is the ordinary case. The document is what
//! `bitmap_convert::convert_to_bitmap` builds: the plane's policy, its
//! render in the Gray twin as ONE "Bitmap" layer, the Gray policy, then the
//! Bitmap policy.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::sync::Arc;

use crate::bitmap_codec::BitmapSource;
use crate::bitmap_document::{BitmapPolicy, LAYER_NAME};
use crate::bitmap_method::{is_white, pack_row, stride};
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::gray_working::GrayWorkingSpace;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::{Metadata, Resolution, PNG_SIGNATURE};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};

/// Bytes a canvas pixel the open charges beside the encoded file: the
/// decoded and the oriented packed planes (an eighth of a byte each), the
/// RGBA16 tiles of the "Bitmap" layer (8 B/px) and the opened document's
/// display preparation the host runs right after. It starts at Indexed's
/// measured open shape (`indexed_io::OPEN_BYTES_PER_PIXEL`, 42), the same
/// layer and display under a smaller plane. Held by
/// `scripts/bitmap-admission/measure.py` (`open-bitmap-png`,
/// `open-bitmap-tiff`, `open-bitmap-psd`) to ⌈1.25 × its measured peak⌉; a
/// miss raises this term.
pub(crate) const OPEN_BYTES_PER_PIXEL: u64 = 42;

/// Which 1-bit container the file at `path` is (`bitmap_codec`'s claim),
/// header work only. `Err` only when the file cannot be opened.
pub(crate) fn bitmap_source_at(path: &str) -> Result<Option<BitmapSource>, String> {
    let mut file = File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let mut magic = [0u8; 8];
    if file.read_exact(&mut magic).is_err() {
        return Ok(None);
    }
    if magic.starts_with(b"II*\0") || magic.starts_with(b"MM\0*") {
        drop(file);
        let mut read = crate::tiff_directory::file_reader(path)?;
        return Ok(crate::bitmap_codec::tiff_facts(&mut read).map(|facts| facts.source));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("failed to read {path}: {e}"))?;
    let mut reader = BufReader::new(file);
    if magic == PNG_SIGNATURE {
        let head = crate::gray_io::png_head_from(&mut reader);
        return Ok(head
            .and_then(|head| crate::bitmap_codec::png_facts(&head))
            .map(|facts| facts.source));
    }
    if magic.starts_with(b"BM") {
        let mut head = Vec::new();
        reader
            .take(crate::bitmap_codec::BMP_HEAD_BYTES as u64)
            .read_to_end(&mut head)
            .map_err(|e| format!("failed to read {path}: {e}"))?;
        return Ok(crate::bitmap_codec::bmp_facts(&head).map(|facts| facts.source));
    }
    Ok(None)
}

/// The open's working memory: [`OPEN_BYTES_PER_PIXEL`] a pixel, the encoded
/// file and bookkeeping. The PSD open charges the same (`bitmap_psd`).
pub(crate) fn open_scratch(width: u32, height: u32, encoded: u64) -> Result<u64, String> {
    Ok(pixel_count(width, height)? as u64 * OPEN_BYTES_PER_PIXEL + encoded + BOOKKEEPING_BYTES)
}

/// The stored `sw × sh` plane turned by EXIF `orientation` (1 keeps it):
/// each output bit read from its source pixel (`pixel_orientation`), each
/// row packed by `bitmap_method::pack_row`.
fn oriented(
    stored: Vec<u8>,
    (sw, sh): (u32, u32),
    orientation: u8,
    job: JobContext<'_>,
) -> Result<(Vec<u8>, u32, u32), String> {
    if orientation == 1 {
        return Ok((stored, sw, sh));
    }
    let (sw, sh) = (sw as usize, sh as usize);
    let (w, h) = crate::pixel_orientation::dimensions(sw, sh, orientation);
    let (from, to) = (stride(sw as u32), stride(w as u32));
    let mut out = crate::precision_image::reserved_vec(to * h)?;
    out.resize(to * h, 0u8);
    let mut bits = vec![0u8; w];
    for (y, row) in out.chunks_exact_mut(to.max(1)).enumerate().take(h) {
        for (x, bit) in bits.iter_mut().enumerate() {
            let (sx, sy) = crate::pixel_orientation::source(x, y, sw, sh, orientation);
            *bit = u8::from(is_white(&stored[sy * from..(sy + 1) * from], sx));
        }
        pack_row(&bits, row);
        job.every(y * w)?;
    }
    Ok((out, w as u32, h as u32))
}

impl PrecisionDocument {
    /// A 1-bit PNG, TIFF or BMP as a Bitmap document (module doc).
    /// `working` is the working Gray's wire name, the profile of a file with
    /// none. `Err` for every file [`bitmap_source_at`] does not answer for.
    pub fn open_bitmap(
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
        let bytes = crate::precision_codec::read_regular(path, limit, other, job)?;
        let facts = crate::bitmap_codec::facts(&bytes).ok_or_else(|| {
            format!("failed to decode {path}: the file is not a 1-bit PNG, TIFF or BMP")
        })?;
        let scratch = open_scratch(facts.width, facts.height, bytes.len() as u64)?;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let stored = crate::bitmap_codec::decode(&bytes, &facts, path)?;
        job.check()?;
        let (scan, scan_warnings) = crate::metadata::scan_report(&bytes);
        let warnings: Vec<String> = scan_warnings.into_iter().map(str::to_owned).collect();
        let policy = match (&facts.icc, facts.srgb) {
            (Some(icc), _) => GrayPolicy::new(icc, GrayDepth::Eight)?,
            (None, true) => GrayPolicy::for_space(GrayWorkingSpace::SGray, GrayDepth::Eight)?,
            (None, false) => GrayPolicy::for_space(working, GrayDepth::Eight)?,
        };
        let orientation = crate::metadata_tiff::orientation_of_file(&bytes)
            .or_else(|| {
                scan.metadata
                    .exif
                    .as_deref()
                    .and_then(crate::metadata_tiff::orientation)
            })
            .map_or(1, |o| o.to_exif());
        let (packed, width, height) =
            oriented(stored, (facts.width, facts.height), orientation, job)?;
        let quarter = (5..=8).contains(&orientation);
        let resolution = match facts.ppm {
            Some(ppm) => crate::bitmap_codec::ppm_resolution(ppm),
            None => scan
                .resolution
                .map(|r| r.transposed_if(quarter))
                .unwrap_or_default(),
        };
        let document = Self::from_bitmap_plane(
            packed,
            width,
            height,
            policy,
            scan.metadata,
            resolution,
            warnings,
            job,
        )?;
        // A TIFF's paths (`tiff_paths`).
        let document = match facts.source {
            BitmapSource::Tiff => document.with_tiff_paths(&bytes)?,
            BitmapSource::Png | BitmapSource::Bmp => document,
        };
        job.report(1.0, 5)?;
        Ok(document)
    }

    /// The Bitmap document of a packed `width × height` plane under the
    /// Gray `policy` (module doc): the open's and the PSD open's one
    /// constructor, `convert_to_bitmap`'s shape.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_bitmap_plane(
        packed: Vec<u8>,
        width: u32,
        height: u32,
        policy: GrayPolicy,
        metadata: Metadata,
        resolution: Resolution,
        warnings: Vec<String>,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let bitmap = BitmapPolicy::new(packed, width, height)?;
        let image = bitmap.render(policy.twin(), job)?;
        PrecisionDocument::from_image_named(image, metadata, resolution, LAYER_NAME)?
            .with_import_warnings(warnings)?
            .with_gray(Arc::new(policy))?
            .with_bitmap(Arc::new(bitmap))
    }
}
