//! Indexed PNG, GIF and palette TIFF files as Indexed Color documents (#1
//! Indexed phases 1a and 1b): which files open as one
//! ([`indexed_source_at`], header work only) and the open itself
//! ([`PrecisionDocument::open_indexed`]).
//!
//! # Which files
//!
//! Exactly the files `open_indexed` accepts, decided before any pixel
//! decodes (as `gray_io::gray_source`):
//!
//! * PNG colour type 3 at bit depth 1, 2, 4 or 8, a `PLTE` of 2–256
//!   entries, a `tRNS` (if any) whose every alpha is 0 or 255 with at most
//!   one 0, no `acTL`, and the RGB profile contract the typed open applies
//!   (`precision_png::png_profile_contract`): an `iCCP` that
//!   `IccProfile::parse`s as an RGB profile, else `sRGB` or nothing.
//! * GIF with exactly one image descriptor, at (0, 0), covering the logical
//!   screen, a colour table (the local one, else the global one), and no
//!   ICC application extension. A Graphic Control Extension's transparent
//!   index, when set, must lie inside that table. The walk skips every data
//!   sub-block.
//! * A palette TIFF as `indexed_tiff` states (phase 1b): Photometric 3, one
//!   1/2/4/8-bit sample or an 8-bit sample with an unassociated alpha, a
//!   ColorMap, an RGB ICC or none; its transparent index comes from the
//!   decode (the one index whose pixels are exactly the clear ones).
//!
//! Every other file answers `None` and keeps today's open (as RGB).
//!
//! # The open
//!
//! The palette and the transparent index are kept exactly, duplicate
//! entries included, and every index is kept as it is stored. The `iCCP`
//! profile is kept verbatim; `sRGB` and no profile are the built-in sRGB,
//! the latter disclosed by the typed open's untagged sentence. The EXIF
//! orientation is applied to the plane as `gray_io::open_gray` applies it,
//! and a TIFF's paths are read from its 8BIM run (`tiff_paths`).

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;

use crate::icc::IccProfile;
use crate::indexed_document::{IndexedPolicy, LAYER_NAME};
use crate::indexed_palette::Rgb;
use crate::indexed_tiff::PaletteTiff;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};

/// `dr_path_indexed_source`'s answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IndexedSource {
    Png = 1,
    Gif = 2,
    Tiff = 3,
}

/// Bytes a canvas pixel the open charges beside the encoded file: the
/// decoded and the oriented index planes (2 B/px), the RGBA16 tiles of the
/// "Index" layer (8 B/px) and the opened document's display preparation the
/// host runs right after. Held by `scripts/indexed-admission/measure.py`
/// (`open-indexed-png`) to ⌈1.25 × its measured peak⌉: it started at Gray's
/// open shape (`gray_io::OPEN_BYTES_PER_PIXEL`, 41), which missed at 24 MP
/// (792.8 MB measured, 33.0 B/px) by 0.5 %, so it was raised to 42; a miss
/// raises this term.
pub(crate) const OPEN_BYTES_PER_PIXEL: u64 = 42;

/// The typed open's sentence for a file with no profile (`precision_codec`).
pub(crate) const UNTAGGED: &str =
    "RGB input without an ICC profile is interpreted as sRGB; no pixel conversion was performed";

const PNG_SIGNATURE: &[u8; 8] = &crate::metadata::PNG_SIGNATURE;

/// What the header walk learnt of an indexed file.
struct Facts {
    width: u32,
    height: u32,
    /// PNG bit depth (1, 2, 4 or 8); 8 for a GIF.
    depth: u8,
    palette: Vec<Rgb>,
    /// The PNG `tRNS` or GIF transparent index; a palette TIFF's comes from
    /// its decode (`indexed_tiff::decode`).
    transparent: Option<u8>,
    /// The PNG `iCCP` or TIFF ICC profile; never for a GIF.
    icc: Option<Vec<u8>>,
    /// A palette TIFF's own facts, for its decode.
    tiff: Option<PaletteTiff>,
}

/// Whether `magic` starts a classic TIFF.
fn is_tiff(magic: &[u8]) -> bool {
    magic.starts_with(b"II*\0") || magic.starts_with(b"MM\0*")
}

/// A palette TIFF's facts as the open's.
fn tiff_facts(tiff: PaletteTiff) -> Facts {
    Facts {
        width: tiff.width,
        height: tiff.height,
        depth: tiff.bits as u8,
        palette: tiff.palette.clone(),
        transparent: None,
        icc: tiff.icc.clone(),
        tiff: Some(tiff),
    }
}

/// Which indexed container `bytes` is and what its header says (module
/// doc); `None` for every other file.
fn facts(bytes: &[u8]) -> Option<(IndexedSource, Facts)> {
    if bytes.starts_with(PNG_SIGNATURE) {
        return png_facts(crate::gray_io::png_head(bytes)?).map(|f| (IndexedSource::Png, f));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return gif_facts(&mut Cursor::new(bytes)).map(|f| (IndexedSource::Gif, f));
    }
    if is_tiff(bytes) {
        return crate::indexed_tiff::palette_tiff_of(bytes)
            .map(|t| (IndexedSource::Tiff, tiff_facts(t)));
    }
    None
}

/// Which indexed container the file at `path` is (module doc), header work only. `Err` only
/// when the file cannot be opened.
pub(crate) fn indexed_source_at(path: &str) -> Result<Option<IndexedSource>, String> {
    let mut file = File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let mut magic = [0u8; 8];
    if file.read_exact(&mut magic).is_err() {
        return Ok(None);
    }
    if is_tiff(&magic) {
        drop(file);
        let mut read = crate::tiff_directory::file_reader(path)?;
        return Ok(crate::indexed_tiff::palette_tiff(&mut read).map(|_| IndexedSource::Tiff));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("failed to read {path}: {e}"))?;
    let mut reader = BufReader::new(file);
    if &magic == PNG_SIGNATURE {
        let head = crate::gray_io::png_head_from(&mut reader);
        return Ok(head
            .and_then(|head| png_facts(&head))
            .map(|_| IndexedSource::Png));
    }
    if magic.starts_with(b"GIF87a") || magic.starts_with(b"GIF89a") {
        return Ok(gif_facts(&mut reader).map(|_| IndexedSource::Gif));
    }
    Ok(None)
}

/// The head's facts when it is an indexed PNG `open_indexed` accepts.
fn png_facts(head: &[u8]) -> Option<Facts> {
    if head.len() < 33 || &head[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(head[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(head[20..24].try_into().ok()?);
    let (depth, colour) = (head[24], head[25]);
    if colour != 3 || !matches!(depth, 1 | 2 | 4 | 8) {
        return None;
    }
    let chunks = crate::gray_io::png_chunk_data(head);
    if chunks.iter().any(|(kind, _)| *kind == b"acTL") {
        return None;
    }
    let plte = chunks.iter().find(|(kind, _)| *kind == b"PLTE")?.1;
    if plte.len() % 3 != 0 || !(2..=256).contains(&(plte.len() / 3)) {
        return None;
    }
    let palette: Vec<Rgb> = plte.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    let transparent = match chunks.iter().find(|(kind, _)| *kind == b"tRNS") {
        None => None,
        Some((_, alphas)) => {
            if alphas.len() > palette.len() || alphas.iter().any(|&a| a != 0 && a != 255) {
                return None;
            }
            let mut clear = alphas.iter().enumerate().filter(|(_, &a)| a == 0);
            let first = clear.next().map(|(i, _)| i as u8);
            if clear.next().is_some() {
                return None;
            }
            first
        }
    };
    let icc = match chunks.iter().find(|(kind, _)| *kind == b"iCCP") {
        Some((_, data)) => {
            let icc = crate::gray_io::iccp_profile(data)?;
            let profile = IccProfile::parse(&icc)?;
            if !crate::indexed_document::is_rgb_profile(&profile) {
                return None;
            }
            Some(icc)
        }
        None => None,
    };
    crate::precision_png::png_profile_contract(head, icc.is_some(), false).ok()?;
    Some(Facts {
        width,
        height,
        depth,
        palette,
        transparent,
        icc,
        tiff: None,
    })
}

fn read_n(reader: &mut impl Read, n: usize) -> Option<Vec<u8>> {
    let mut out = vec![0u8; n];
    reader.read_exact(&mut out).ok()?;
    Some(out)
}

fn read_u8(reader: &mut impl Read) -> Option<u8> {
    let mut byte = [0u8; 1];
    reader.read_exact(&mut byte).ok()?;
    Some(byte[0])
}

/// Skips a run of data sub-blocks through its zero terminator, returning the
/// first sub-block's bytes.
fn sub_blocks(reader: &mut impl Read) -> Option<Vec<u8>> {
    let mut first = None;
    loop {
        let size = usize::from(read_u8(reader)?);
        if size == 0 {
            return Some(first.unwrap_or_default());
        }
        let block = read_n(reader, size)?;
        first.get_or_insert(block);
    }
}

fn color_table(reader: &mut impl Read, packed: u8) -> Option<Vec<Rgb>> {
    let count = 2usize << (packed & 7);
    let bytes = read_n(reader, count * 3)?;
    Some(bytes.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect())
}

/// The GIF header walk (module doc).
fn gif_facts(reader: &mut impl Read) -> Option<Facts> {
    let header = read_n(reader, 13)?;
    let width = u32::from(u16::from_le_bytes([header[6], header[7]]));
    let height = u32::from(u16::from_le_bytes([header[8], header[9]]));
    let global = if header[10] & 0x80 != 0 {
        Some(color_table(reader, header[10])?)
    } else {
        None
    };
    let mut pending: Option<u8> = None;
    let mut image: Option<(Vec<Rgb>, Option<u8>)> = None;
    loop {
        let Some(block) = read_u8(reader) else {
            // A file that ends after its image, with no trailer, still
            // holds the one image.
            break;
        };
        match block {
            0x21 => {
                let label = read_u8(reader)?;
                let first = sub_blocks(reader)?;
                match label {
                    0xF9 if first.len() >= 4 => {
                        pending = (first[0] & 1 != 0).then_some(first[3]);
                    }
                    0xFF if first.starts_with(b"ICCRGBG1") => return None,
                    _ => {}
                }
            }
            0x2C => {
                if image.is_some() {
                    return None;
                }
                let descriptor = read_n(reader, 9)?;
                let field =
                    |at: usize| u32::from(u16::from_le_bytes([descriptor[at], descriptor[at + 1]]));
                if field(0) != 0 || field(2) != 0 || field(4) != width || field(6) != height {
                    return None;
                }
                let local = if descriptor[8] & 0x80 != 0 {
                    Some(color_table(reader, descriptor[8])?)
                } else {
                    None
                };
                let palette = local.or_else(|| global.clone())?;
                read_u8(reader)?;
                sub_blocks(reader)?;
                image = Some((palette, pending.take()));
            }
            0x3B => break,
            _ => return None,
        }
    }
    let (palette, transparent) = image?;
    if transparent.is_some_and(|t| usize::from(t) >= palette.len()) {
        return None;
    }
    Some(Facts {
        width,
        height,
        depth: 8,
        palette,
        transparent,
        icc: None,
        tiff: None,
    })
}

/// The open's working memory: [`OPEN_BYTES_PER_PIXEL`] a pixel, the encoded
/// file and bookkeeping.
pub(crate) fn open_scratch(width: u32, height: u32, encoded: u64) -> Result<u64, String> {
    Ok(pixel_count(width, height)? as u64 * OPEN_BYTES_PER_PIXEL + encoded + BOOKKEEPING_BYTES)
}

/// A PNG's stored indices, one byte a pixel, rows of 1/2/4-bit samples
/// unpacked (most significant bits first).
fn png_indices(bytes: &[u8], facts: &Facts, path: &str) -> Result<Vec<u8>, String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: usize::MAX / 2,
        },
    );
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(|e| fail(e.to_string()))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| fail("the image is too large".into()))?;
    let mut packed = crate::precision_image::reserved_vec(size)?;
    packed.resize(size, 0u8);
    let info = reader
        .next_frame(&mut packed)
        .map_err(|e| fail(e.to_string()))?;
    let (w, h) = (facts.width as usize, facts.height as usize);
    if (info.width, info.height) != (facts.width, facts.height) {
        return Err(fail("the decoded size differs from the header".into()));
    }
    unpack_rows(packed, w, h, facts.depth, info.line_size)
        .ok_or_else(|| fail("the decoded size differs from the header".into()))
}

/// Rows of `line` bytes holding `depth`-bit samples (1, 2, 4 or 8, most
/// significant bits first) as one byte a pixel; `None` when `packed` is
/// short. The PNG and palette TIFF decodes' one unpacking.
pub(crate) fn unpack_rows(
    mut packed: Vec<u8>,
    w: usize,
    h: usize,
    depth: u8,
    line: usize,
) -> Option<Vec<u8>> {
    if line.checked_mul(h)? > packed.len() || line * 8 < w * usize::from(depth) {
        return None;
    }
    if depth == 8 && line == w {
        packed.truncate(w * h);
        return Some(packed);
    }
    let depth = usize::from(depth);
    let mask = ((1u16 << depth) - 1) as u8;
    let mut out = crate::precision_image::reserved_vec(w * h).ok()?;
    for y in 0..h {
        let row = &packed[y * line..(y + 1) * line];
        for x in 0..w {
            let bit = x * depth;
            let shift = 8 - depth - bit % 8;
            out.push((row[bit / 8] >> shift) & mask);
        }
    }
    Some(out)
}

/// A GIF's one frame as indices.
fn gif_indices(bytes: &[u8], facts: &Facts, path: &str) -> Result<Vec<u8>, String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    let pixels = pixel_count(facts.width, facts.height)? as u64;
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    let limit = std::num::NonZeroU64::new(pixels.max(1)).ok_or("empty GIF")?;
    options.set_memory_limit(gif::MemoryLimit::Bytes(limit));
    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(|e| fail(e.to_string()))?;
    let frame = decoder
        .read_next_frame()
        .map_err(|e| fail(e.to_string()))?
        .ok_or_else(|| fail("the GIF holds no image".into()))?;
    if u32::from(frame.width) != facts.width
        || u32::from(frame.height) != facts.height
        || frame.buffer.len() as u64 != pixels
    {
        return Err(fail("the frame differs from the logical screen".into()));
    }
    Ok(frame.buffer.to_vec())
}

impl PrecisionDocument {
    /// An indexed PNG, GIF or palette TIFF as an Indexed Color document (module doc).
    /// `Err` for every file [`indexed_source_at`] does not answer for.
    pub fn open_indexed(
        path: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let bytes = crate::precision_codec::read_regular(path, limit, other, job)?;
        let (source, facts) = facts(&bytes).ok_or_else(|| {
            format!("failed to decode {path}: the file is not an indexed PNG, GIF or TIFF")
        })?;
        let scratch = match &facts.tiff {
            Some(tiff) => crate::indexed_tiff::open_plan(tiff, bytes.len() as u64)?,
            None => open_scratch(facts.width, facts.height, bytes.len() as u64)?,
        };
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let (stored, transparent) = match (source, &facts.tiff) {
            (IndexedSource::Png, _) => (png_indices(&bytes, &facts, path)?, facts.transparent),
            (IndexedSource::Gif, _) => (gif_indices(&bytes, &facts, path)?, facts.transparent),
            (IndexedSource::Tiff, Some(tiff)) => crate::indexed_tiff::decode(&bytes, tiff, path)?,
            (IndexedSource::Tiff, None) => {
                return Err(format!(
                    "failed to decode {path}: the file is not a palette TIFF"
                ))
            }
        };
        job.check()?;
        if stored
            .iter()
            .any(|&i| usize::from(i) >= facts.palette.len())
        {
            return Err(format!(
                "failed to decode {path}: an index is outside the palette"
            ));
        }
        let (scan, scan_warnings) = crate::metadata::scan_report(&bytes);
        let mut warnings: Vec<String> = scan_warnings.into_iter().map(str::to_owned).collect();
        let profile = match &facts.icc {
            Some(icc) => Arc::new(
                IccProfile::parse(icc)
                    .ok_or_else(|| format!("failed to decode {path}: the profile is unusable"))?,
            ),
            None => {
                let srgb = source == IndexedSource::Png
                    && crate::gray_io::png_head(&bytes).is_some_and(|head| {
                        crate::gray_io::png_chunk_data(head)
                            .iter()
                            .any(|(kind, _)| *kind == b"sRGB")
                    });
                if !srgb {
                    warnings.push(UNTAGGED.into());
                }
                IccProfile::srgb()
            }
        };
        let orientation = crate::metadata_tiff::orientation_of_file(&bytes)
            .or_else(|| {
                scan.metadata
                    .exif
                    .as_deref()
                    .and_then(crate::metadata_tiff::orientation)
            })
            .map_or(1, |o| o.to_exif());
        let (sw, sh) = (facts.width as usize, facts.height as usize);
        let indices = if orientation == 1 {
            stored
        } else {
            let (w, h) = crate::pixel_orientation::dimensions(sw, sh, orientation);
            let mut out = crate::precision_image::reserved_vec(w * h)?;
            for y in 0..h {
                for x in 0..w {
                    let (sx, sy) = crate::pixel_orientation::source(x, y, sw, sh, orientation);
                    out.push(stored[sy * sw + sx]);
                }
                job.every(y * w)?;
            }
            out
        };
        let (w, h) = crate::pixel_orientation::dimensions(sw, sh, orientation);
        let policy = IndexedPolicy::new(facts.palette, transparent, indices, w as u32, h as u32)?;
        let image = policy.render(&profile, job)?;
        let quarter = (5..=8).contains(&orientation);
        let resolution = scan
            .resolution
            .map(|r| r.transposed_if(quarter))
            .unwrap_or_default();
        let document =
            PrecisionDocument::from_image_named(image, scan.metadata, resolution, LAYER_NAME)?
                .with_import_warnings(warnings)?
                .with_indexed(Arc::new(policy))?;
        // A TIFF's paths (`tiff_paths`).
        let document = match source {
            IndexedSource::Tiff => document.with_tiff_paths(&bytes)?,
            IndexedSource::Png | IndexedSource::Gif => document,
        };
        job.report(1.0, 5)?;
        Ok(document)
    }
}
