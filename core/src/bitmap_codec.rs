//! The 1-bit files of a Bitmap document (#547 Bitmap phase 1c, KERNEL):
//! which PNG, TIFF and BMP files hold one packed plane ([`facts`], header
//! work only), their decode to `bitmap_method`'s plane ([`decode`]: MSB
//! first, 1 white, pad bits 0, every format), the PNG and BMP encoders and
//! the polarity flip PSD and the inverted file sources need
//! (`designs/bitmap-mode.md`). `bitmap_io` owns the open,
//! `bitmap_export` the kept export, `bitmap_psd` the PSD/PSB colour mode 0
//! files and `tiff_encode::encode_bilevel` the TIFF writer.
//!
//! # Which files
//!
//! * **PNG** — IHDR colour type 0 at bit depth 1, no `acTL` and no `tRNS`
//!   (a Bitmap document holds no alpha, so such a file keeps today's open,
//!   with its transparency). An `iCCP` must be a profile a Gray document
//!   takes (`gray_document::GrayPolicy::new` at 8 bits); an `sRGB` chunk is
//!   sGray; `gAMA` and `cHRM` are ignored. A row is `ceil(w/8)` bytes, 1
//!   white (PNG §7.2), so the rows are the plane as they stand.
//! * **TIFF** — a classic TIFF whose first directory holds Photometric 0
//!   (WhiteIsZero) or 1 (BlackIsZero) as a SHORT, one sample of 1 bit,
//!   SampleFormat absent or 1, PlanarConfiguration 1, FillOrder (266)
//!   absent or 1, no ExtraSamples, a Compression in
//!   `tiff_directory::COMPRESSIONS` and an ICC that is absent or Gray as
//!   above. The `tiff` crate (0.11.3) inverts WhiteIsZero samples itself and
//!   ignores FillOrder, so a WhiteIsZero file is decoded through the shared
//!   `Overlay` with Photometric patched to 1 (`indexed_tiff`'s patch) and
//!   inverted ONCE here by [`flip_polarity`].
//! * **BMP** — `BM`, a DIB header of 40, 108 or 124 bytes, one plane, 1 bpp,
//!   BI_RGB, 0 or 2 colours used and two palette entries that are exactly
//!   black and white, in either order; a white-first palette makes a bit 0
//!   white, so its rows are inverted by [`flip_polarity`]. Rows are padded
//!   to four bytes and stored bottom-up unless the height is negative. The
//!   header's pixels per metre are the resolution.
//!
//! Every other file answers `None` and keeps today's open: a palette PNG
//! stays Indexed, a 1-bit BMP of other colours stays RGB.
//!
//! # Pad bits
//!
//! A row's bits past the width are unspecified in every source (PNG §7.2,
//! BMP rows), and the plane holds them 0 (`BitmapPolicy::new` refuses a set
//! one), so [`decode`] ends every format with [`clear_pad_bits`], the ONE
//! pad-bit clear, which [`flip_polarity`] calls after it inverts.

use std::borrow::Cow;
use std::io::Cursor;

use crate::bitmap_method::stride;
use crate::dr_image::Format;
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::metadata::{Resolution, PNG_SIGNATURE};
use crate::metadata_write::EncodeSidecar;
use crate::tiff_directory::{bytes_reader, Directory, Overlay, COMPRESSIONS};

/// `dr_path_bitmap_source`'s answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BitmapSource {
    Png = 1,
    Tiff = 2,
    Bmp = 3,
}

/// What the header walk learnt of a 1-bit file (module doc).
#[derive(Clone, Debug)]
pub(crate) struct BitmapFacts {
    pub(crate) source: BitmapSource,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The PNG `iCCP` or TIFF ICC, a Gray profile; never for a BMP.
    pub(crate) icc: Option<Vec<u8>>,
    /// A PNG `sRGB` chunk (sGray).
    pub(crate) srgb: bool,
    /// The stored bits are the plane's inverse: a WhiteIsZero TIFF or a
    /// white-first BMP palette.
    pub(crate) white_is_zero: bool,
    /// A BMP's pixels per metre (x, y), both positive.
    pub(crate) ppm: Option<(u32, u32)>,
}

/// The largest embedded TIFF profile read (`indexed_tiff`'s cap).
const MAX_ICC_BYTES: u32 = 16 * 1024 * 1024;

/// The BMP headers read: the file header, the largest DIB header and the
/// two-entry palette.
pub(crate) const BMP_HEAD_BYTES: usize = 14 + 124 + 8;

/// Metres per inch, BMP's pixels-per-metre unit.
const METRES_PER_INCH: f64 = 0.0254;

/// Whether `icc` is a profile a Gray document takes (module doc).
fn gray_icc(icc: &[u8]) -> bool {
    GrayPolicy::new(icc, GrayDepth::Eight).is_ok()
}

/// Which 1-bit file `bytes` is and what its header says (module doc);
/// `None` for every other file.
pub(crate) fn facts(bytes: &[u8]) -> Option<BitmapFacts> {
    if bytes.starts_with(&PNG_SIGNATURE) {
        return png_facts(crate::gray_io::png_head(bytes)?);
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return tiff_facts(&mut bytes_reader(bytes));
    }
    if bytes.starts_with(b"BM") {
        return bmp_head(bytes).map(|(_, facts)| facts);
    }
    None
}

/// The facts of a PNG head (its signature and every chunk before the first
/// IDAT) when it is a 1-bit gray PNG (module doc).
pub(crate) fn png_facts(head: &[u8]) -> Option<BitmapFacts> {
    if head.len() < 33 || &head[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(head[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(head[20..24].try_into().ok()?);
    if head[24] != 1 || head[25] != 0 {
        return None;
    }
    let chunks = crate::gray_io::png_chunk_data(head);
    if chunks
        .iter()
        .any(|(kind, _)| *kind == b"acTL" || *kind == b"tRNS")
    {
        return None;
    }
    let icc = match chunks.iter().find(|(kind, _)| *kind == b"iCCP") {
        Some((_, data)) => {
            let icc = crate::gray_io::iccp_profile(data)?;
            if !gray_icc(&icc) {
                return None;
            }
            Some(icc)
        }
        None => None,
    };
    Some(BitmapFacts {
        source: BitmapSource::Png,
        width,
        height,
        icc,
        srgb: chunks.iter().any(|(kind, _)| *kind == b"sRGB"),
        white_is_zero: false,
        ppm: None,
    })
}

/// The facts of the TIFF the bytes `read` serves when it is a 1-bit gray
/// TIFF (module doc), header work only.
pub(crate) fn tiff_facts(
    read: &mut dyn FnMut(u64, usize) -> Option<Vec<u8>>,
) -> Option<BitmapFacts> {
    let found = Directory::first(read)?;
    let photometric = found.entry(262)?;
    let white_is_zero = match (photometric.kind, found.scalar(262)?) {
        (3, 0) => true,
        (3, 1) => false,
        _ => return None,
    };
    let width = found.scalar(256)?;
    let height = found.scalar(257)?;
    if found.scalar(277).unwrap_or(1) != 1
        || !COMPRESSIONS.contains(&found.scalar(259).unwrap_or(1))
        || found.scalar(284).unwrap_or(1) != 1
        || found.scalar(266).unwrap_or(1) != 1
        || found.entry(338).is_some()
    {
        return None;
    }
    if let Some(bits) = found.shorts(258, 1, read)? {
        if bits.iter().any(|(v, _)| *v != 1) {
            return None;
        }
    }
    if let Some(formats) = found.shorts(339, 1, read)? {
        if formats.iter().any(|(v, _)| *v != 1) {
            return None;
        }
    }
    let icc = match found.entry(34675).copied() {
        None => None,
        Some(entry) => {
            if !matches!(entry.kind, 1 | 7) || entry.count <= 4 || entry.count > MAX_ICC_BYTES {
                return None;
            }
            let bytes = read(found.offset_of(&entry), entry.count as usize)?;
            if !gray_icc(&bytes) {
                return None;
            }
            Some(bytes)
        }
    };
    Some(BitmapFacts {
        source: BitmapSource::Tiff,
        width,
        height,
        icc,
        srgb: false,
        white_is_zero,
        ppm: None,
    })
}

/// Where a BMP's rows are: the pixel data's offset, a stored row's bytes
/// (padded to four) and whether the rows run top-down.
struct BmpRows {
    offset: usize,
    line: usize,
    top_down: bool,
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// A BMP head's rows and facts when it is a 1-bit black-and-white BMP
/// (module doc); `head` is at least the file and DIB headers and the
/// palette.
fn bmp_head(head: &[u8]) -> Option<(BmpRows, BitmapFacts)> {
    if !head.starts_with(b"BM") {
        return None;
    }
    let offset = le32(head, 10)? as usize;
    let dib = le32(head, 14)? as usize;
    if !matches!(dib, 40 | 108 | 124) {
        return None;
    }
    let width = le32(head, 18)? as i32;
    let height = le32(head, 22)? as i32;
    if width <= 0 || height == 0 || height == i32::MIN {
        return None;
    }
    if le16(head, 26)? != 1 || le16(head, 28)? != 1 || le32(head, 30)? != 0 {
        return None;
    }
    if !matches!(le32(head, 46)?, 0 | 2) {
        return None;
    }
    let palette = head.get(14 + dib..14 + dib + 8)?;
    let entry = |i: usize| [palette[4 * i], palette[4 * i + 1], palette[4 * i + 2]];
    let white_first = match (entry(0), entry(1)) {
        ([0, 0, 0], [255, 255, 255]) => false,
        ([255, 255, 255], [0, 0, 0]) => true,
        _ => return None,
    };
    if offset < 14 + dib + 8 {
        return None;
    }
    let (xppm, yppm) = (le32(head, 38)? as i32, le32(head, 42)? as i32);
    let width = width as u32;
    let rows = BmpRows {
        offset,
        line: (width as usize).div_ceil(32) * 4,
        top_down: height < 0,
    };
    let facts = BitmapFacts {
        source: BitmapSource::Bmp,
        width,
        height: height.unsigned_abs(),
        icc: None,
        srgb: false,
        white_is_zero: white_first,
        ppm: (xppm > 0 && yppm > 0).then_some((xppm as u32, yppm as u32)),
    };
    Some((rows, facts))
}

/// The facts of a BMP head read from a file (`bitmap_io::bitmap_source_at`).
pub(crate) fn bmp_facts(head: &[u8]) -> Option<BitmapFacts> {
    bmp_head(head).map(|(_, facts)| facts)
}

/// A BMP's pixels per metre as the resolution in pixels per inch.
pub(crate) fn ppm_resolution((x, y): (u32, u32)) -> Resolution {
    let ppi = |ppm: u32| (f64::from(ppm) * METRES_PER_INCH) as f32;
    Resolution {
        x: ppi(x),
        y: ppi(y),
    }
    .sane()
}

/// Zeroes each `stride(width)`-byte row's bits past `width`, IN PLACE on
/// the caller-owned plane: the one pad-bit clear (module doc).
pub(crate) fn clear_pad_bits(packed: &mut [u8], width: u32) {
    let line = stride(width);
    let used = width % 8;
    if used == 0 || line == 0 {
        return;
    }
    let keep = !(0xFFu8 >> used);
    for row in packed.chunks_exact_mut(line) {
        row[line - 1] &= keep;
    }
}

/// Every bit of a `width`-wide plane inverted, then its pad bits cleared:
/// PSD's polarity (1 black) both ways, and an inverted file source's rows.
pub(crate) fn flip_polarity(packed: &[u8], width: u32) -> Vec<u8> {
    let mut out: Vec<u8> = packed.iter().map(|byte| !byte).collect();
    clear_pad_bits(&mut out, width);
    out
}

/// The packed plane of a file [`facts`] answered for (module doc): MSB
/// first, 1 white, pad bits 0.
pub(crate) fn decode(bytes: &[u8], facts: &BitmapFacts, path: &str) -> Result<Vec<u8>, String> {
    let fail = |what: &str| format!("failed to decode {path}: {what}");
    let line = stride(facts.width);
    let size = line
        .checked_mul(facts.height as usize)
        .ok_or_else(|| fail("the image is too large"))?;
    let mut packed = match facts.source {
        BitmapSource::Png => png_rows(bytes, facts, line, path)?,
        BitmapSource::Tiff => tiff_rows(bytes, facts, size, path)?,
        BitmapSource::Bmp => bmp_rows(bytes, line, size, path)?,
    };
    if packed.len() != size {
        return Err(fail("the sample count differs from the dimensions"));
    }
    if facts.white_is_zero {
        return Ok(flip_polarity(&packed, facts.width));
    }
    clear_pad_bits(&mut packed, facts.width);
    Ok(packed)
}

/// A 1-bit gray PNG's rows, which are the plane's rows.
fn png_rows(bytes: &[u8], facts: &BitmapFacts, line: usize, path: &str) -> Result<Vec<u8>, String> {
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
    let mut raw = crate::precision_image::reserved_vec(size)?;
    raw.resize(size, 0u8);
    let info = reader
        .next_frame(&mut raw)
        .map_err(|e| fail(e.to_string()))?;
    if (info.width, info.height) != (facts.width, facts.height) || info.line_size < line {
        return Err(fail("the decoded size differs from the header".into()));
    }
    if info.line_size == line {
        raw.truncate(line * facts.height as usize);
        return Ok(raw);
    }
    let mut out = crate::precision_image::reserved_vec(line * facts.height as usize)?;
    for row in raw.chunks_exact(info.line_size).take(facts.height as usize) {
        out.extend_from_slice(&row[..line]);
    }
    Ok(out)
}

/// A 1-bit gray TIFF's rows, Photometric patched to 1 when it is 0 (module
/// doc).
fn tiff_rows(
    bytes: &[u8],
    facts: &BitmapFacts,
    size: usize,
    path: &str,
) -> Result<Vec<u8>, String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    let found = Directory::first(&mut bytes_reader(bytes))
        .ok_or_else(|| fail("the TIFF directory cannot be read".into()))?;
    let photometric = found
        .entry(262)
        .ok_or_else(|| fail("the TIFF has no photometric interpretation".into()))?;
    let patches = if facts.white_is_zero {
        vec![(photometric.at, found.short_bytes(1))]
    } else {
        Vec::new()
    };
    let decoder = tiff::decoder::Decoder::new(Overlay::new(bytes, &patches))
        .map_err(|e| fail(e.to_string()))?;
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = size.max(crate::drdc::MAX_DRDC_BLOB_LEN as usize);
    limits.ifd_value_size = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
    let mut decoder = decoder.with_limits(limits);
    let stored = decoder.dimensions().map_err(|e| fail(e.to_string()))?;
    if stored != (facts.width, facts.height) {
        return Err(fail("the directory's dimensions disagree".into()));
    }
    let mut raw = match decoder.read_image().map_err(|e| fail(e.to_string()))? {
        tiff::decoder::DecodingResult::U8(raw) => raw,
        _ => return Err(fail("the TIFF's samples are not 1-bit".into())),
    };
    if raw.len() < size {
        return Err(fail("the sample count differs from the dimensions".into()));
    }
    raw.truncate(size);
    Ok(raw)
}

/// A 1-bit BMP's rows, top row first, each cut to the plane's stride.
fn bmp_rows(bytes: &[u8], line: usize, size: usize, path: &str) -> Result<Vec<u8>, String> {
    let fail = |what: &str| format!("failed to decode {path}: {what}");
    let (rows, facts) = bmp_head(bytes).ok_or_else(|| fail("the BMP header cannot be read"))?;
    let height = facts.height as usize;
    let end = rows
        .line
        .checked_mul(height)
        .and_then(|n| n.checked_add(rows.offset))
        .ok_or_else(|| fail("the image is too large"))?;
    if end > bytes.len() {
        return Err(fail("the pixel data is truncated"));
    }
    let mut out = crate::precision_image::reserved_vec(size)?;
    for y in 0..height {
        let stored = if rows.top_down { y } else { height - 1 - y };
        let at = rows.offset + stored * rows.line;
        out.extend_from_slice(&bytes[at..at + line]);
    }
    Ok(out)
}

/// The plane as a 1-bit gray PNG (colour type 0, depth 1, 1 white: the
/// plane's own rows). The ICC (`iCCP`) and EXIF (`eXIf`) go through the
/// `png` encoder's own chunks; XMP, `pHYs` and IPTC are spliced by the
/// injector a Gray PNG uses (`gray_encode::injector`), as
/// `indexed_encode::encode_png` does.
pub(crate) fn encode_png(
    packed: &[u8],
    dims: (u32, u32),
    side: &EncodeSidecar<'_>,
) -> Result<Vec<u8>, String> {
    if packed.len() as u64 != stride(dims.0) as u64 * u64::from(dims.1) {
        return Err("encoding failed: the Bitmap plane does not match the dimensions".into());
    }
    let mut info = png::Info::with_size(dims.0, dims.1);
    info.color_type = png::ColorType::Grayscale;
    info.bit_depth = png::BitDepth::One;
    info.icc_profile = side.icc.map(|icc| Cow::Owned(icc.to_vec()));
    info.exif_metadata = side.exif.clone().map(Cow::Owned);
    let mut sink = crate::gray_encode::injector(Format::Png, side);
    let fail = |e: png::EncodingError| format!("encoding failed: {e}");
    {
        let encoder = png::Encoder::with_info(&mut sink, info).map_err(fail)?;
        let mut writer = encoder.write_header().map_err(fail)?;
        writer.write_image_data(packed).map_err(fail)?;
        writer.finish().map_err(fail)?;
    }
    Ok(sink.finish()?.into_inner())
}

/// The plane as a 1 bpp BMP: a 14-byte file header, a 40-byte
/// BITMAPINFOHEADER (BI_RGB, two colours, the resolution in pixels per
/// metre), the palette black then white (so a bit 1 is white, the plane's
/// own polarity) and the rows bottom-up, each padded to four bytes. A BMP
/// carries no profile and no metadata.
pub(crate) fn encode_bmp(
    packed: &[u8],
    dims: (u32, u32),
    resolution: Resolution,
) -> Result<Vec<u8>, String> {
    let line = stride(dims.0);
    let height = dims.1 as usize;
    if packed.len() as u64 != line as u64 * u64::from(dims.1) {
        return Err("encoding failed: the Bitmap plane does not match the dimensions".into());
    }
    let (Ok(width), Ok(rows)) = (i32::try_from(dims.0), i32::try_from(dims.1)) else {
        return Err("encoding failed: a BMP is at most 2147483647 pixels a side".into());
    };
    let padded = (dims.0 as usize).div_ceil(32) * 4;
    let offset = 14 + 40 + 8;
    let image = padded as u64 * height as u64;
    let Ok(total) = u32::try_from(offset as u64 + image) else {
        return Err("encoding failed: a BMP file is at most 4 GiB".into());
    };
    let ppm = |ppi: f32| (f64::from(ppi) / METRES_PER_INCH).round() as i32;
    let resolution = resolution.sane();
    let mut out = crate::precision_image::reserved_vec(total as usize)?;
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&rows.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(image as u32).to_le_bytes());
    out.extend_from_slice(&ppm(resolution.x).to_le_bytes());
    out.extend_from_slice(&ppm(resolution.y).to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 0]);
    for y in (0..height).rev() {
        out.extend_from_slice(&packed[y * line..(y + 1) * line]);
        out.resize(out.len() + padded - line, 0);
    }
    Ok(out)
}
