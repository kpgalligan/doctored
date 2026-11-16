//! The banded 8-bit native reader (13C-2b): a byte layer plane, channel
//! plane or embedded 8-bit source stored in the writer's form is decoded row
//! by row through png's own row reader (`png::Reader::next_row`,
//! `Transformations::IDENTITY`) straight into 256×256 tiles
//! (`TilePlane::from_rows_with_job`), and a raw mask record is read one row
//! at a time from its span. So an open holds one PNG row, png's small
//! unfiltering window and one tile band beside the decoded tiles, instead of
//! the decoder's whole-plane buffer and the file it was read from.
//!
//! **The master copy** of the shape is `precision_png_rows::decode_native_png`
//! (the RGBA16 reader: limits, identity transformations, the header re-check,
//! `next_row` into the plane, the truncated, row-length and extra-row
//! refusals). This is its twin because the samples and colour types differ:
//! an 8-bit row is copied straight into `[u8; 4]` or `u8` samples, where a
//! 16-bit one is big-endian pairs, and a gray plane is colour type 0.
//!
//! **The writer's form** ([`writer_form`]) is what `png_rows8` writes, and
//! what `image`'s `PngEncoder` wrote before it: 8 bits, colour type 6 (RGBA)
//! or 0 (gray), not interlaced, and no chunk but IHDR, IDAT and IEND. Any
//! other stored PNG (an RGB, 16-bit, gray-alpha, interlaced or annotated one
//! from an older or another writer) is the caller's to decode whole, as the
//! reader always did.
//!
//! Errors are bare sentences; each caller adds its plane's prefix.
//! [`band_scratch8`] is what one plane holds beside its tiles, the 8-bit
//! reopen plan's transient (`drdc_reopen_plan`).

use crate::cancel_io::Polled;
use crate::job_control::JobContext;
use crate::native_stream::Payload;
use crate::precision_codec::NATIVE_FIXED;
use crate::precision_png_rows::NATIVE_BAND_ROWS;
use crate::tile_store::{Sample, TilePlane};

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// IHDR's length and type as the first chunk spells them.
const IHDR_HEAD: [u8; 8] = [0, 0, 0, 13, b'I', b'H', b'D', b'R'];
/// The signature, IHDR's length and type, and its 13 data bytes.
const HEAD_BYTES: usize = 8 + 8 + 13;
/// A chunk's length, type and CRC around its data.
const CHUNK_FRAMING_BYTES: u64 = 12;

/// The banded reader's transient for one plane `width` pixels wide with
/// `channels` samples a pixel: `NATIVE_BAND_ROWS` PNG rows (`channels·w + 1`
/// bytes each, the filter byte included) plus the fixed codec allowance, the
/// 16-bit reader's figure at one byte a sample. Also png's own allocation
/// limit for the decode.
pub(crate) fn band_scratch8(width: u32, channels: u8) -> u64 {
    NATIVE_BAND_ROWS * (u64::from(channels) * u64::from(width) + 1) + NATIVE_FIXED
}

/// Whether `png` is in the writer's form for `channels` (4: RGBA, colour
/// type 6; 1: gray, colour type 0): the PNG signature, an IHDR of bit depth
/// 8, that colour type, compression 0, filter 0 and interlace 0, and a chunk
/// walk that meets nothing but IHDR, IDAT and IEND and ends at the span's
/// end. Anything else, a malformed head or chunk walk included, is
/// `Ok(false)`, and the caller decodes the PNG whole. `Err` only when the
/// span cannot be read.
pub(crate) fn writer_form(png: &Payload<'_>, channels: u8) -> Result<bool, String> {
    let colour = match channels {
        4 => 6,
        1 => 0,
        _ => return Ok(false),
    };
    let length = png.len();
    if length < HEAD_BYTES as u64 {
        return Ok(false);
    }
    let mut head = [0_u8; HEAD_BYTES];
    png.read_at(0, &mut head)?;
    let ihdr = &head[16..];
    if head[..8] != SIGNATURE
        || head[8..16] != IHDR_HEAD
        || ihdr[8] != 8
        || ihdr[9] != colour
        || ihdr[10..13] != [0, 0, 0]
    {
        return Ok(false);
    }
    let mut at = SIGNATURE.len() as u64;
    while at < length {
        if length - at < CHUNK_FRAMING_BYTES {
            return Ok(false);
        }
        let mut chunk = [0_u8; 8];
        png.read_at(at, &mut chunk)?;
        if !matches!(&chunk[4..], b"IHDR" | b"IDAT" | b"IEND") {
            return Ok(false);
        }
        let size = u64::from(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        at += size + CHUNK_FRAMING_BYTES;
    }
    Ok(at == length)
}

/// Decodes a writer's-form RGBA8 PNG of `dimensions` into a tiled plane, row
/// by row (module doc). Nothing is shared between tiles, as a decoded layer
/// plane never was.
pub(crate) fn decode_rgba8(
    png: &Payload<'_>,
    dimensions: (u32, u32),
    job: JobContext<'_>,
) -> Result<TilePlane<[u8; 4]>, String> {
    decode(png, dimensions, 4, false, job, |data, row| {
        row.copy_from_slice(data.as_chunks::<4>().0);
    })
}

/// Decodes a writer's-form gray PNG of `dimensions` into a tiled plane, row
/// by row (module doc). A uniform tile is shared, so a uniform channel stays
/// a few tiles, as `GrayRaster::new` keeps one.
pub(crate) fn decode_gray8(
    png: &Payload<'_>,
    dimensions: (u32, u32),
    job: JobContext<'_>,
) -> Result<TilePlane<u8>, String> {
    decode(png, dimensions, 1, true, job, |data, row| {
        row.copy_from_slice(data);
    })
}

/// A raw mask span (`w·h` bytes, row 0 first) as a tiled plane, one
/// `read_at` per row, a uniform tile shared as [`decode_gray8`] shares one.
/// The caller guarantees `w·h ≤ MAX_PIXELS`: the tile store refuses a larger
/// plane.
pub(crate) fn raw_gray8(
    raw: &Payload<'_>,
    (width, height): (u32, u32),
    job: JobContext<'_>,
) -> Result<TilePlane<u8>, String> {
    if raw.len() != u64::from(width) * u64::from(height) {
        return Err("raw mask length differs from its dimensions".into());
    }
    TilePlane::from_rows_shared_with_job(
        width,
        height,
        |y, row| raw.read_at(u64::from(y) * u64::from(width), row),
        job,
    )
}

/// The row decode both PNG planes share: png's reader over the span through
/// `cancel_io::Polled`, limited to [`band_scratch8`], the header checked
/// against the form and `dimensions` the caller preflighted, each row handed
/// to `unpack`, then the end of the frame.
fn decode<S: Sample>(
    png: &Payload<'_>,
    (width, height): (u32, u32),
    channels: u8,
    share: bool,
    job: JobContext<'_>,
    unpack: impl Fn(&[u8], &mut [S]),
) -> Result<TilePlane<S>, String> {
    job.check()?;
    let limit = usize::try_from(band_scratch8(width, channels))
        .map_err(|_| "PNG exceeds the decoder limit")?;
    let mut decoder =
        png::Decoder::new_with_limits(Polled::new(png.reader(), job), png::Limits { bytes: limit });
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    let colour = if channels == 4 {
        png::ColorType::Rgba
    } else {
        png::ColorType::Grayscale
    };
    if info.interlaced
        || (info.width, info.height) != (width, height)
        || info.bit_depth != png::BitDepth::Eight
        || info.color_type != colour
    {
        return Err("PNG header changed after preflight".into());
    }
    job.check()?;
    let row_bytes = usize::from(channels) * width as usize;
    let fill = |_: u32, row: &mut [S]| -> Result<(), String> {
        let data = reader
            .next_row()
            .map_err(|e| e.to_string())?
            .ok_or("PNG is truncated")?
            .data();
        if data.len() != row_bytes {
            return Err("PNG row length differs".into());
        }
        unpack(data, row);
        Ok(())
    };
    let plane = if share {
        TilePlane::from_rows_shared_with_job(width, height, fill, job)?
    } else {
        TilePlane::from_rows_with_job(width, height, fill, job)?
    };
    // The end of the frame: png advances over the rest of the image data,
    // checking its CRCs, as its whole-frame read does.
    if reader.next_row().map_err(|e| e.to_string())?.is_some() {
        return Err("PNG has extra rows".into());
    }
    job.check()?;
    Ok(plane)
}
