//! The banded native RGBA16 PNG reader: each native layer / source
//! plane is decoded row by row through png's own row reader
//! (`png::Reader::next_row`, `Transformations::IDENTITY`, big-endian
//! samples) straight into 256×256 tiles (`TilePlane::from_rows_with_job`),
//! so a reopen holds one PNG row, png's small unfiltering window and one
//! tile band beside the decoded tiles, instead of HEAD's two whole-plane
//! 8 B/px copies (the decoder's output buffer and the sample vector).
//!
//! An interlaced native plane is refused (`precision_png::png_header`): no
//! Doctored writer has emitted one, and decoding Adam7 needs the whole
//! plane.
//!
//! # Banded reopen record
//! Declared before measuring: the band figure [`native_png_band_scratch`]
//! (`NATIVE_BAND_ROWS · (8·w + 1) + NATIVE_FIXED`) and the reopen figure
//! `precision_native_plan::reopen_scratch`, base = retained + file + band +
//! bookkeeping, plus a quarter, capped by HEAD's whole-plane figure (27 B/px,
//! `precision_png::NATIVE_DECODE_BYTES_PER_PIXEL`). Stop rule: any max E above
//! base stops the change (the band and the quarter are not tuned to fit).
//! The declared cap was later removed; it never won in these cases.
//!
//! Method: `precision_png`'s `# Banded native write`, one case
//! per process, 3 runs, `MallocLargeCache=0` exported for every process. Each
//! document is built in-process and saved with `save_native`; in the measured
//! process `proc_reset_footprint_interval`, pre = `ri_phys_footprint`, the
//! measured call is `open_native(path, REFERENCE_WORKING_BYTES, 0, None)` and E =
//! `ri_interval_max_phys_footprint` − pre (`RUSAGE_INFO_V4`), bytes. The
//! calibration passed in the session: 1 GiB touched and dropped came back to
//! within 16,384 bytes; 256 MiB touched gave E 268,583,104 / 268,566,720 /
//! 268,566,720 with lifetime peak − pre 1,074,283,264. The baseline twin read
//! E 0 in all six runs. L is the file length, P the pixels of the largest
//! plane, A the reader's figure (from its refusal at a probing
//! `other_retained`), base the reader's banded base (A = base + ⌈base/4⌉ in
//! every case: the whole-plane cap never won) and H the plan's
//! `reopen_retained_bytes + decode_scratch_bytes`.
//!
//! - (a) R17, a 6000×4000 placed plasma photo turned by Rotate Canvas 17°
//!   bicubic (a 6908×5580 raster beside the 6000×4000 source): E
//!   1,003,766,480 / 1,003,750,096 / 1,003,782,864, L 500,426,396, P
//!   38,546,640, base 1,007,995,774, A 1,259,994,718 (1.2552× max E), H
//!   1,259,995,018. HEAD (M2, `precision_png`'s record): max E 1,427,784,712,
//!   A 2,047,870,814, H 2,047,871,054. N4-R17 (xorshift noise in all four
//!   channels): E 1,003,766,480 ×3, the same L, P, base, A (1.2553×) and H.
//! - (b) a white 6000×4000 16-bit background plus a placed 6000×4000 plasma
//!   photo, source `transform` 17° bicubic (planes 6000×4000, 6908×5580 and
//!   the 6000×4000 source): E 1,005,175,648 / 1,005,159,264 / 1,005,159,264,
//!   L 692,445,138, P 38,546,640, base 1,392,014,540, A 1,740,018,175,
//!   H 1,740,018,492. After the open its footprint is 311,984,488 above pre
//!   while `retained_bytes()` is 501,628,972, so about one 6000×4000 plane
//!   is not counted in `phys_footprint` here and E may undercount by up to
//!   192,000,000. Against E + 192,000,000 = 1,197,175,648 the margin below
//!   base is 194,838,892 and A ≥ 1.4534× (1.7311× against the measured E).
//! - (c) case (b)'s two-layer document turned by Rotate Canvas 17° bicubic:
//!   not built. The turn's own admission refuses first ("precision operation
//!   needs 2416422387 bytes including retained inputs"), before any reopen
//!   plan.
//! - (d) a placed 8064×6048 16-bit photo at identity (raster and source):
//!   E 1,564,705,896 / 1,564,673,128 / 1,564,673,128, L 780,414,272, P
//!   48,771,072, base 1,568,095,650, A 1,960,119,563 (1.2527×), H
//!   1,960,119,823.
//! - (e) RS0, a 4000×3000 placed photo under source `transform`
//!   [0.5, 0, 0, 0.5, 0, 0] bicubic (a 2000×1500 raster, so the source PNG is
//!   the largest plane): E 241,057,984 / 241,074,368 / 241,074,368, L
//!   120,019,144, P 12,000,000, base 246,843,178, A 308,553,973 (1.2799×), H
//!   308,554,233.
//! - (f) RS1, (e) with a 25,000,000-byte RAW attached: E 291,127,536 /
//!   291,127,536 / 291,111,152, L 145,019,423, P 12,000,000, base
//!   296,905,153, A 371,131,442 (1.2748×), H 371,131,744.
//!
//! Every max E is below its base (by 3.4–5.8 MB; (b) by at least
//! 194,838,892 once its undercount is allowed for), so A ≥
//! 1.25 × max E by construction, and H ≥ A in every case (by 260–317
//! bytes). The peak is the file, the decoded tiles and one band: (a)'s E − L
//! is 13.06 B per pixel of P, the raster's 8 plus the source's 8 per its own
//! pixel, where HEAD held about 16 B/px more (the decoder's output buffer and
//! the sample vector). The edit-time envelope follows, and the two-layer
//! 24 MP document of (b) saves and reopens.
//!
//! The reopen no longer holds the file: the PNG is a span of the
//! streamed reader (`native_stream`), decoded through a 1 MiB `BufReader`
//! over its section of the file, and the figure's `file` term is the
//! reader's held bytes (its small takes and window). The record above is the
//! whole-file reader's; the streamed one is `precision_native_plan`'s
//! `# Streamed reader and writer`.

use std::sync::Arc;

use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::native_stream::Payload;
use crate::precision_codec::NATIVE_FIXED;
use crate::precision_image::{pixel_count, PrecisionImage};
use crate::precision_png::{inspect_native_payload, payload_chunks};
use crate::tile_store::TilePlane;

/// The rows of PNG data the banded reader's figure allows for (png's
/// unfiltering window keeps a few rows; declared before measuring).
pub(crate) const NATIVE_BAND_ROWS: u64 = 16;

/// The banded reader's transient for one plane: `NATIVE_BAND_ROWS` PNG rows
/// (`8·w + 1` bytes each, the filter byte included) plus the fixed codec
/// allowance. Also png's own allocation limit for the decode.
pub(crate) fn native_png_band_scratch(dimensions: (u32, u32)) -> Result<u64, String> {
    pixel_count(dimensions.0, dimensions.1)?;
    Ok(NATIVE_BAND_ROWS * (8 * u64::from(dimensions.0) + 1) + NATIVE_FIXED)
}

/// png's decoding error, worded as a PNG decode failure (HEAD's image-crate
/// errors named the format; callers and tests read that).
fn decode_error(e: png::DecodingError) -> String {
    format!("high-depth native: layer PNG decode failed: {e}")
}

/// Decodes a native RGBA16 PNG of `dimensions` into a tiled image, row by
/// row (module doc). Pixels only (IHDR, IDAT, IEND), not interlaced. The
/// PNG is a span (`native_stream`): a slice decodes through a `Cursor` as it
/// always did, a file span through a 1 MiB `BufReader` over its section.
pub(crate) fn decode_native_png(
    png: &Payload<'_>,
    dimensions: (u32, u32),
    profile: Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    job.check()?;
    if inspect_native_payload(png)? != dimensions {
        return Err("high-depth native: layer PNG dimensions changed".into());
    }
    payload_chunks(png, |kind| {
        job.check()?;
        if matches!(kind, b"IHDR" | b"IDAT" | b"IEND") {
            Ok(())
        } else {
            Err("high-depth native: layer PNG must contain pixels only".into())
        }
    })?;
    let limit = usize::try_from(native_png_band_scratch(dimensions)?)
        .map_err(|_| "high-depth native: layer PNG exceeds the decoder limit")?;
    let mut decoder = png::Decoder::new_with_limits(png.reader(), png::Limits { bytes: limit });
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(decode_error)?;
    let info = reader.info();
    if info.interlaced {
        return Err("high-depth native: layer PNG must not be interlaced".into());
    }
    if (info.width, info.height) != dimensions
        || info.bit_depth != png::BitDepth::Sixteen
        || info.color_type != png::ColorType::Rgba
    {
        return Err("high-depth native: layer PNG dimensions changed".into());
    }
    job.check()?;
    let row_bytes = 8 * dimensions.0 as usize;
    let mut i = 0_usize;
    let plane = TilePlane::from_rows_with_job(
        dimensions.0,
        dimensions.1,
        |_, row| {
            let data = reader
                .next_row()
                .map_err(decode_error)?
                .ok_or("high-depth native: layer PNG is truncated")?
                .data();
            if data.len() != row_bytes {
                return Err("high-depth native: layer PNG row length differs".into());
            }
            for (pixel, bytes) in row.iter_mut().zip(data.chunks_exact(8)) {
                job.every(i)?;
                i += 1;
                *pixel =
                    std::array::from_fn(|c| u16::from_be_bytes([bytes[2 * c], bytes[2 * c + 1]]));
            }
            Ok(())
        },
        job,
    )?;
    // The end of the frame: png advances over the rest of the image data,
    // as its whole-frame read does.
    if reader.next_row().map_err(decode_error)?.is_some() {
        return Err("high-depth native: layer PNG has extra rows".into());
    }
    job.check()?;
    PrecisionImage::from_rgba16_tiles_decoded(plane, profile)
}
