//! Strict typed PNG framing and native pixel-only payloads. Native framing is
//! preflighted before a document decoder allocates any layer. The shared codec
//! owns sample encoding; these wrappers add captured-profile and job contracts.
//!
//! # Reopen decode scratch
//! Since the banded reader the reopen is charged by
//! `precision_native_plan::reopen_scratch`; its measurement record is
//! `precision_png_rows`' module doc. The record below is HEAD's whole-plane
//! decode.
//! `NATIVE_DECODE_BYTES_PER_PIXEL` is 27 (was 32), the smallest integer K for
//! which the reader's own admission figure, `A(K) = A − (32 − K) · P`, covers
//! 1.25 × the measured reopen peak in every case. Method: a
//! 5700×3800 RGBA16 sRGB photo bound by `retain`, edited at bicubic and saved
//! with `save_native`; the measured call is `open_native(path,
//! REFERENCE_WORKING_BYTES, 0, None)`, 3 runs each, one case per process, E =
//! `/usr/bin/time -l` peak − `ri_phys_footprint` read just before the call
//! (bytes). L is the file length, P the pixels of the largest serialized
//! plane, A the reader's figure (`retained + L + decode + bookkeeping`, read
//! from its refusal at a probing `other_retained`), H the edit-time envelope
//! (`native_plan()` `reopen_retained_bytes + decode_scratch_bytes`), both at
//! 32. A17 source `transform` 17°: peaks 1,465,009,544 / 1,464,632,712 /
//! 1,464,632,712 over pre 1,245,472 / 1,261,856 / 1,261,856, max E
//! 1,463,764,072, L 451,662,843, P 34,791,724, A 2,022,923,821 (1.3820×), H
//! 2,137,880,936; S17 `affine_set`: peaks 1,465,009,544 / 1,464,665,480 /
//! 1,464,665,480 over pre 1,261,856 ×3, max E 1,463,747,688, the same L, P,
//! A (1.3820×) and H; P `perspective` (7410×4370 result): peaks 1,387,595,072
//! / 1,387,595,072 / 1,387,529,536 over pre 1,261,856 / 1,229,088 /
//! 1,245,472, max E 1,386,365,984, L 432,380,336, P 32,381,700, A
//! 1,907,240,354 (1.3757×), H 2,017,379,920; R17 Rotate Canvas: peaks
//! 1,464,616,328 / 1,465,173,408 / 1,464,993,160 over pre 1,245,472 /
//! 1,229,088 / 1,261,856, max E 1,463,944,320, L 451,662,875, P 34,791,724,
//! A 2,022,923,853 (1.3818×), H 2,137,881,000; N3-R17, R17 of xorshift noise
//! in RGB (alpha opaque): peaks 1,464,649,096 / 1,464,681,888 /
//! 1,464,632,712, max E 1,463,436,416, L, P, A and H as R17's; N4-R17, noise
//! in all four channels: peaks 1,464,649,096 / 1,464,632,712 /
//! 1,465,058,720, max E 1,463,796,864, the same figures. The native PNG is
//! stored, not compressed, so L does not depend on content and the noise
//! cases decide nothing apart (both give K = 27). The baseline twin's peaks
//! (1,294,624 / 1,294,624 / 1,311,008) sit at pre (1,261,856 / 1,245,472 /
//! 1,278,240). The peak is the file, the decoded planes and two 8 B/px
//! copies of the largest plane (the PNG rows and the sample vector), about
//! 16 B/px of transient. At K = 27, A(27) / max E is 1.2632× (A17), 1.2632×
//! (S17), 1.2589× (P), 1.2630× (R17), 1.2634× (N3), 1.2631× (N4); at 26
//! every case falls below 1.25× (1.2356–1.2397×).
//!
//! The edit-time envelope of a 17° turn of a 6000×4000 placed image (a
//! 6908×5580 raster, 38,546,640 pixels, beside its 24 MP original) was
//! 2,175,013,712 bytes at 27 (2,367,746,912 at 32); the perspective's was
//! 2,055,014,912 (2,234,414,912). A 5961×3974 turn's (a 6863×5544 raster)
//! was 2,146,998,404 bytes. Since the exact PNG bound (below) the
//! 6000×4000 turn's envelope is 2,047,871,054 (`reopen_retained`
//! 501,442,498 + decode scratch 1,546,428,556). Header bytes count
//! twice in the envelope, once in `reopen_retained` and once through
//! `encoded_bound` (`precision_native_plan::finish_plan`), because the reader
//! holds them twice (measured in `precision_native_plan`'s record). O1
//! (report only): since the banded writer (below) a single-layer
//! 6000×4000 raster turned 17° has `retained_bytes() + write_scratch_bytes`
//! 700,520,107 (308,387,755 + 392,132,352; was 2,163,982,605) and the
//! 6000×4000 placed result 1,132,553,784 (500,391,371 + 632,162,413; was
//! 2,355,988,889), so both save at 16 bits under the working limit.
//!
//! The reopen of that placed result (M2; the method above, with
//! `MallocLargeCache=0` exported for every process, see `# Banded native
//! write`), 6000×4000 placed images edited at bicubic and saved with
//! `save_native`: A17: peaks 1,429,030,184 / 1,429,046,568 / 1,429,013,800
//! over pre 1,261,856 / 1,245,472 / 1,229,088, max E 1,427,801,096, L
//! 500,426,364, P 38,546,640 (6908×5580), A 2,047,870,782 (1.4343×), H
//! 2,047,870,990; S17: peaks 1,429,046,568 / 1,429,046,568 / 1,429,046,592
//! over pre 1,245,472 / 1,261,856 / 1,245,472, max E 1,427,801,120, the same
//! L, P, A (1.4343×) and H; P (a 7800×4600 result): peaks 1,345,438,944 /
//! 1,345,455,328 / 1,345,422,560 over pre 1,261,856 / 1,261,856 / 1,245,472,
//! max E 1,344,193,472, L 479,090,731, P 35,880,000, A 1,933,202,749
//! (1.4382×), H 1,933,203,049; R17: peaks 1,429,013,800 / 1,429,030,184 /
//! 1,429,013,800 over pre 1,229,088 / 1,245,472 / 1,229,088, max E
//! 1,427,784,712, L 500,426,396, P 38,546,640, A 2,047,870,814 (1.4343×), H
//! 2,047,871,054; N4-R17: peaks 1,429,030,184 / 1,429,013,800 /
//! 1,429,030,184 over pre 1,229,088 / 1,245,472 / 1,212,704, max E
//! 1,427,817,480, L, P, A and H as R17's (1.4343×). The baseline twin's
//! peaks (1,294,624 / 1,278,240 / 1,278,240 and 1,278,240 / 1,294,624 /
//! 1,278,240) sit at pre. Every case has A ≥ 1.25 × max E (R-ok), so K = 27
//! holds at 24 MP, and H ≥ A in every case. The 5700×3800 record above was
//! taken with the allocator's large-allocation cache on, so its peaks are
//! not comparable figure for figure with these.
//!
//! # Encoded bound
//! `native_png_encoded_bound` is the exact stored length of a native RGBA16
//! PNG, not a fitted 10 B/px + 1 MiB. The writer of every 16-bit native
//! plane, raster layers and 16-bit source assets alike, is
//! [`put_native_png`] (below), which writes the bytes the former path
//! wrote (pinned by SHA-256 in `tests/native_save_bytes.rs`). That path was
//! `encode_native_png` → `precision_codec::encode(…, PrecisionFormat::Png,
//! &EncodeSidecar::NONE, …)`: image 0.25.10's
//! `PngEncoder::new_with_quality(CompressionType::Uncompressed,
//! FilterType::NoFilter)` (`codecs/png.rs` 671–714), which maps to png
//! 0.18.1 `Compression::NoCompression` → `DeflateCompression::NoCompression`
//! (`common.rs` 411–419). png's `write_image_data`
//! (`encoder.rs` 742–783) takes the `NoCompression` arm: it ignores
//! `options.filter` and writes, per row, filter byte 0 then the raw row into
//! fdeflate 0.3.7's `StoredOnlyCompressor` (`compress.rs` 188–250), so the
//! length never depends on pixel values. The stream is a 2-byte zlib header,
//! then stored blocks; a new 5-byte block header opens only when the current
//! block holds 65,535 bytes and more data follows, and `finish` adds a
//! 4-byte Adler-32. The zlib bytes go into IDAT chunks of at most 2^31−1
//! bytes each (`write_zlib_encoded_idat`); IEND is written on `Writer` drop.
//! `encode_header` writes the signature and IHDR (13 bytes); every other
//! chunk comes from `Info` fields that are `None` under `Info::with_size`,
//! image sets ICC and EXIF only when they are non-empty, and
//! `EncodeSidecar::NONE` leaves both empty. The reader also refuses any chunk
//! other than IHDR, IDAT and IEND (`decode_native_png`). So, for `w×h`:
//! R = h·(8w+1); Z = R + 5·⌈R/65,535⌉ + 6; L = 8 + 25 + Z +
//! 12·⌈Z/(2^31−1)⌉ + 12. Under `MAX_PIXELS` = 10^8, R ≤ 9·10^8, so there is
//! exactly one IDAT; the function still computes the count.
//!
//! The bound equals L, and `put_native_png` writes exactly L bytes: it
//! writes L as the length prefix before the PNG and refuses ("encoded PNG
//! length differs from its planned bound") if the PNG it wrote is any other
//! length. `tests/native_png_bound.rs` fails on any drift: it walks each
//! saved PNG with its own oracle and checks the length against its own formula, on
//! flat and noise pixels. Cross-check against batch 91's files (the
//! record above): R17's L 451,662,875 = 278,360,397 (6562×5302) +
//! 173,297,088 (5700×3800) + a 5,390-byte header; P's L 432,380,336 =
//! 259,077,798 (7410×4370) + 173,297,088 + a 5,450-byte header, 60 bytes
//! more for its projective source-transform entry. At 6000×4000 turned 17°
//! the two plane bounds go from 627,563,552 to 500,421,006 (308,402,293 +
//! 192,018,713), 127,142,546 less; the plan's `encoded_bound` is
//! 500,426,396, the same 5,390-byte header.
//!
//! Older files still read: no reader compares a record to this bound. The
//! reader checks only `MAX_DRDC_PNG_LEN` (`precision_native_read.rs`,
//! `native_sources.rs`) and `MAX_NATIVE_BYTES`, and charges the actual file
//! length; re-saves re-encode from pixels and never copy stored PNG bytes.
//! The bytes are unchanged, so there is no `DRDC_VERSION` bump. With no
//! 1 MiB per-plane slack left, every plan header term must count exactly the
//! bytes its writer writes (`evaluation_native::header_bytes` once counted
//! 12 of its 16).
//!
//! H ≥ A holds by construction, since the bound is ≥ L and `reopen_retained`
//! is ≥ the reader's retained figure; the 6000×4000 reopen it covers is
//! measured above (M2).
//!
//! # Banded native write
//! [`put_native_png`] streams each 16-bit plane from its tile rows
//! (`row_segments`) through one 8·w-byte row buffer into fdeflate's
//! `StoredOnlyCompressor`, writing straight into the `.rz` body buffer, which
//! `encode_with_context` reserves once at `encoded_bound`. Before it, the
//! largest plane was copied at the save's peak, beside the retained tiles:
//! (1) `precision_codec::encode`'s native-endian sample Vec, 8 B/px; (2)
//! image 0.25.10's `reordered` big-endian Vec (`codecs/png.rs` 763), 8 B/px;
//! (3) png 0.18.1's zlib `Cursor<Vec>` (`encoder.rs` 780), grown by
//! doubling, about 8 B/px and up to 16 at a reallocation; (4) the output
//! Cursor, 8 B/px, filled at `encoder.rs` 887 — (1)–(4) live together at the
//! IDAT write, about 32 B/px; then (5) the returned PNG beside `out` grown by
//! `append`, a reallocation that held the old and the new `out` (for a
//! source PNG, written second, the plan's `2·encoded_bound + largest_png`).
//! The banded writer removes (1)–(5); beside the retained tiles there
//! remained `out` (the file, ≤ `encoded_bound`), one row and the pre-built
//! header tables. The streamed writer does not assemble the file either:
//! `put_native_png` writes to a `NativeSink` (the temporary file
//! `save_atomically` renames, through a 1 MiB `BufWriter`) through an
//! [`IdatWindow`] that commits the zlib stream behind the compressor's
//! seek-back and takes the IDAT CRC as it commits, and every table encoder
//! reaches the sink through `precision_native_write`. An 8-bit original
//! keeps `PngEncoder`'s default encode and its 48 B/px charge
//! (`native_sources::byte_source_png_encode_scratch`).
//!
//! The plan's write term (`precision_native_plan::finish_plan`), declared
//! before measuring: base = `encoded_bound` + header + the largest writer
//! transient (`native_png_encode_scratch`, 8·w + 4 MiB, for a 16-bit plane;
//! the 8-bit original's encode) + bookkeeping, and `write_scratch_bytes` =
//! base + ⌈base/4⌉. A float document kept `2·encoded_bound` + bookkeeping
//! (unmeasured). Both were replaced by the streamed term (no file;
//! `precision_native_plan`'s `# Streamed reader and writer`); the
//! record below is the in-memory writer's.
//!
//! Method: one case per process, 3 runs, a baseline twin per size,
//! with `MallocLargeCache=0` exported for every process: on macOS 26.5.1
//! libmalloc's large-allocation cache keeps freed large blocks in
//! `phys_footprint` and hands them to a later large allocation, so a peak
//! read after setup can sit near 0. The document (a 16-bit plasma photo,
//! bound by `retain` when placed, edited at bicubic) is built in-process,
//! then `proc_reset_footprint_interval` and pre = `ri_phys_footprint`; the
//! measured call is `save_native(path, REFERENCE_WORKING_BYTES, 0, None)` and E =
//! `ri_interval_max_phys_footprint` − pre (`RUSAGE_INFO_V4`), bytes. The
//! calibration, run in each session, discriminates: touch and drop 1 GiB,
//! and the footprint returns to within 16,384 bytes of its start; reset,
//! touch 256 MiB, and E is 268,566,720 (256 MiB + 131,264) while the
//! lifetime peak − pre is 1,074,283,264 (without `MallocLargeCache=0` the
//! dropped GiB stays resident and the lifetime peak − pre is only
//! 268,583,104). Every baseline twin reads E 0. `/usr/bin/time -l` peak −
//! pre is an upper bound only, since the setup (the turn) can peak above a
//! banded save.
//!
//! M0, the former writer, 5700×3800: S-R17 (placed, Rotate Canvas 17°, a
//! 6562×5302 raster) E 1,114,030,872 / 1,114,047,256 / 1,114,030,872 over
//! pre 629,113,600 / 629,064,448 / 629,080,832 (32.02 B/px of the largest
//! plane; time peak − pre agrees), plan 1,675,356,014; S-RAS (an unplaced
//! raster, R17) E 1,114,030,872 / 1,113,998,104 / 1,113,998,104, plan
//! 1,675,353,346.
//!
//! M1, the banded writer, max E against base and `write_scratch_bytes`
//! (write / max E). 5700×3800: S-R17 451,936,568 ×3, base 456,963,641,
//! write 571,204,552 (1.2639×); S-P (`perspective` to the quad [0, 0, w, 0,
//! 1.15w, 1.15h, −0.15w, 1.15h]) 432,619,832 / 432,603,448 / 432,603,448,
//! base 437,687,946, write 547,109,933 (1.2646×); S-RAS 278,560,960 ×3,
//! base 283,661,217, write 354,576,522 (1.2729×); S-N4 (xorshift noise in
//! all four channels, placed, R17) 451,936,568 ×3, base and write as
//! S-R17's (1.2639×). 6000×4000: S-R17 500,679,016 ×3, base 505,729,930,
//! write 632,162,413 (1.2626×); S-P 479,347,048 / 479,379,816 /
//! 479,347,048, base 484,401,461, write 605,501,827 (1.2631×); S-RAS
//! 308,625,624 ×3, base 313,705,881, write 392,132,352 (1.2706×); S-N4
//! 500,679,016 / 500,679,016 / 500,711,784, base and write as S-R17's
//! (1.2625×). Every max E is below its base (by 5.0–5.1 MB, the fixed
//! allowance and bookkeeping): the peak is the file (6000×4000 S-R17's E is
//! `encoded_bound` + 252,620), so `write_scratch_bytes` ≥ 1.25 × max E by
//! construction (S1). At 6000×4000 the placed result's save admits at
//! 1,132,553,784 (retained 500,391,371 + 632,162,413); the former writer's
//! plan was 2,355,988,889.
//!
//! Native v13 raster payloads are lossless RGBA16 PNG; mask/channel payloads are raw u16. The
//! native PNG is stored (uncompressed), and `native_png_encoded_bound` is its exact stored length,
//! so every plan header term must count the writer's bytes exactly. `put_native_png` writes it
//! row by row from the tiles straight into the save's sink, the former image/png path's bytes
//! exactly (`tests/native_save_bytes.rs`); the plan's write term is twice the header, one row, the
//! sink's window and bookkeeping plus a quarter. `precision_png_rows.rs` reads it back the same way:
//! `decode_native_png` takes png's rows (`Transformations::IDENTITY`) straight into tiles
//! (`TilePlane::from_rows_with_job`, adopted by `PrecisionImage::from_rgba16_tiles_decoded`), and
//! refuses an interlaced native plane.

use crate::drdc::MAX_DRDC_PNG_LEN;
use crate::job_control::JobContext;
use crate::metadata;
use crate::native_stream::{NativeSink, Payload};
use crate::precision_codec::NATIVE_FIXED;
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage, PrecisionRun};
use std::io::{Seek, SeekFrom, Write};

pub(crate) fn png_header(bytes: &[u8], native: bool) -> Result<((u32, u32), u8), String> {
    if bytes.len() < 33
        || bytes[..8] != metadata::PNG_SIGNATURE
        || &bytes[8..16] != b"\0\0\0\rIHDR"
        || metadata::crc32(&bytes[12..29]) != u32::from_be_bytes(bytes[29..33].try_into().unwrap())
    {
        return Err("high-depth decode: invalid PNG header".into());
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    // Colour types 0 and 4 are 16-bit GRAY and GRAY+ALPHA (Phase 11D); they
    // are an INPUT form only, interpreted by `gray16_import`, and a NATIVE
    // payload is still exactly type 6.
    if bytes[24] != 16 || !matches!(bytes[25], 0 | 2 | 4 | 6) || (native && bytes[25] != 6) {
        return Err("high-depth depth: genuine Gray/GrayA/RGB/RGBA16 PNG input is required".into());
    }
    if bytes[26] != 0 || bytes[27] != 0 || bytes[28] > 1 {
        return Err("high-depth format: unsupported PNG coding".into());
    }
    // The banded native reader (`precision_png_rows`) decodes rows
    // straight into tiles, which Adam7 cannot do; no Doctored writer emits
    // an interlaced plane (`put_native_png` writes interlace 0).
    if native && bytes[28] != 0 {
        return Err("high-depth native: layer PNG must not be interlaced".into());
    }
    pixel_count(w, h)?;
    let channels = match bytes[25] {
        0 => 1,
        4 => 2,
        2 => 3,
        _ => 4,
    };
    Ok(((w, h), channels))
}

pub(crate) fn png_chunks(
    bytes: &[u8],
    mut visit: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let mut at = 8_usize;
    while at < bytes.len() {
        let head = bytes
            .get(at..at + 8)
            .ok_or("high-depth decode: truncated PNG chunk")?;
        let size = u32::from_be_bytes(head[..4].try_into().unwrap()) as usize;
        let end = at
            .checked_add(size)
            .and_then(|v| v.checked_add(12))
            .filter(|&v| v <= bytes.len())
            .ok_or("high-depth decode: truncated PNG chunk")?;
        visit(&head[4..8])?;
        at = end;
    }
    Ok(())
}

/// Which PNG colour chunks may sit beside which profile declaration.
///
/// `has_icc` means "an embedded profile that PARSED" — for a gray image that
/// is a GRAY ICC read through `icc_lut::ProofProfile` (`gray` true), for an
/// RGB one the `IccProfile` the decoder handed back. The only thing `gray`
/// changes is the sentence the gamma/chromaticity refusal names: a grayscale
/// file has to carry a Gray ICC, and telling its author to embed an RGB one
/// would be advice that refuses again.
pub(crate) fn png_profile_contract(bytes: &[u8], has_icc: bool, gray: bool) -> Result<(), String> {
    let (mut srgb, mut transfer, mut icc) = (false, false, false);
    png_chunks(bytes, |kind| {
        match kind {
            b"sRGB" => srgb = true,
            b"gAMA" | b"cHRM" => transfer = true,
            b"iCCP" => icc = true,
            b"cICP" | b"mDCV" | b"cLLI" => {
                return Err("high-depth profile: PNG HDR/CICP interpretation is unavailable".into())
            }
            _ => {}
        }
        Ok(())
    })?;
    if icc && !has_icc {
        return Err("high-depth profile: embedded PNG profile could not be decoded".into());
    }
    if transfer && !has_icc && !srgb {
        return Err(if gray {
            "high-depth profile: PNG gamma/chromaticities require an explicit Gray ICC or sRGB declaration".into()
        } else {
            "high-depth profile: PNG gamma/chromaticities require an explicit RGB ICC or sRGB declaration".to_string()
        });
    }
    Ok(())
}

pub(crate) fn inspect_native_png(bytes: &[u8]) -> Result<(u32, u32), String> {
    let (dimensions, _) = png_header(bytes, true)?;
    Ok(dimensions)
}

/// [`inspect_native_png`] of a span: its first 33 bytes (signature and IHDR).
pub(crate) fn inspect_native_payload(png: &Payload<'_>) -> Result<(u32, u32), String> {
    inspect_native_png(&png.head(33)?)
}

/// [`png_chunks`] over a span, with its sentences: a slice walks as before,
/// a file span reads each 8-byte chunk head where it lies.
pub(crate) fn payload_chunks(
    png: &Payload<'_>,
    mut visit: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if let Payload::Bytes(bytes) = png {
        return png_chunks(bytes, visit);
    }
    let length = png.len();
    let mut at = 8_u64;
    while at < length {
        if at + 8 > length {
            return Err("high-depth decode: truncated PNG chunk".into());
        }
        let mut head = [0_u8; 8];
        png.read_at(at, &mut head)?;
        let size = u64::from(u32::from_be_bytes(head[..4].try_into().unwrap()));
        let end = at + size + 12;
        if end > length {
            return Err("high-depth decode: truncated PNG chunk".into());
        }
        visit(&head[4..8])?;
        at = end;
    }
    Ok(())
}

/// The PNG signature's length (`metadata::PNG_SIGNATURE`).
const PNG_SIGNATURE_BYTES: u64 = metadata::PNG_SIGNATURE.len() as u64;
/// A chunk's length, type and CRC around its data.
const PNG_CHUNK_FRAMING_BYTES: u64 = 12;
/// IHDR's data.
const PNG_IHDR_DATA_BYTES: u64 = 13;
/// The zlib stream's 2-byte header plus its Adler-32.
const ZLIB_FRAMING_BYTES: u64 = 6;
/// A stored deflate block's BFINAL/BTYPE byte plus LEN and NLEN.
const STORED_BLOCK_HEADER_BYTES: u64 = 5;
/// The most data one stored deflate block carries.
const STORED_BLOCK_MAX_BYTES: u64 = 65_535;
/// png's `MAX_IDAT_CHUNK_LEN` (`u32::MAX >> 1`).
const IDAT_CHUNK_MAX_BYTES: u64 = (1 << 31) - 1;

/// The exact stored length of a native RGBA16 PNG of `dimensions` (derived
/// in the module doc's `# Encoded bound`).
pub(crate) fn native_png_encoded_bound(dimensions: (u32, u32)) -> Result<u64, String> {
    const OVERFLOW: &str = "high-depth native PNG bound overflow";
    pixel_count(dimensions.0, dimensions.1)?;
    let (w, h) = (u64::from(dimensions.0), u64::from(dimensions.1));
    // R: per row, filter byte 0 then 8 bytes per RGBA16 pixel.
    let rows = w
        .checked_mul(8)
        .and_then(|v| v.checked_add(1))
        .and_then(|v| v.checked_mul(h))
        .ok_or(OVERFLOW)?;
    // Z: stored blocks of at most 65,535 bytes inside the zlib framing.
    let zlib = rows
        .div_ceil(STORED_BLOCK_MAX_BYTES)
        .checked_mul(STORED_BLOCK_HEADER_BYTES)
        .and_then(|v| v.checked_add(rows))
        .and_then(|v| v.checked_add(ZLIB_FRAMING_BYTES))
        .ok_or(OVERFLOW)?;
    // L: signature, IHDR, the IDAT chunks, IEND.
    zlib.div_ceil(IDAT_CHUNK_MAX_BYTES)
        .checked_mul(PNG_CHUNK_FRAMING_BYTES)
        .and_then(|v| v.checked_add(zlib))
        .and_then(|v| v.checked_add(PNG_SIGNATURE_BYTES))
        .and_then(|v| v.checked_add(PNG_CHUNK_FRAMING_BYTES + PNG_IHDR_DATA_BYTES))
        .and_then(|v| v.checked_add(PNG_CHUNK_FRAMING_BYTES))
        .ok_or_else(|| OVERFLOW.into())
}
/// Decode scratch per pixel of the largest native raster PNG decoded WHOLE
/// (measured in the module doc; was 32): HEAD's whole-plane figure
/// before the banded reader (`precision_png_rows`). It is now the float reopen
/// figure and an 8-bit original's banded term (`image::load_from_memory`
/// decodes it unbanded) in `precision_native_plan::reopen_scratch`, and no
/// longer caps the 16-bit reopen figure; the reader, the
/// asset inspector and the edit-time reopen envelope all read it through
/// [`native_png_decode_scratch`], so the writer never produces a file its
/// reader refuses.
pub(crate) const NATIVE_DECODE_BYTES_PER_PIXEL: u64 = 27;

pub(crate) fn native_png_decode_scratch(dimensions: (u32, u32)) -> Result<u64, String> {
    Ok(
        pixel_count(dimensions.0, dimensions.1)? as u64 * NATIVE_DECODE_BYTES_PER_PIXEL
            + NATIVE_FIXED,
    )
}
/// The writer's transient beside `out`: one row plus the fixed allowance
/// ([`put_native_png`] streams the rows, measured in the module doc).
pub(crate) fn native_png_encode_scratch(dimensions: (u32, u32)) -> Result<u64, String> {
    pixel_count(dimensions.0, dimensions.1)?;
    Ok(8 * u64::from(dimensions.0) + NATIVE_FIXED)
}

/// The bytes of a native PNG around its zlib stream: the signature, IHDR,
/// the one IDAT's framing and IEND.
const PNG_FRAMING_BYTES: u64 = PNG_SIGNATURE_BYTES
    + PNG_CHUNK_FRAMING_BYTES
    + PNG_IHDR_DATA_BYTES
    + 2 * PNG_CHUNK_FRAMING_BYTES;
/// IEND: zero length, the type and its fixed CRC.
const PNG_IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];

/// Appends the u64 LE length, then the native RGBA16 PNG of `image`, to `out`.
/// Rows are streamed from `row_segments` through one 8·w-byte row buffer,
/// so no copy of the plane is made beside the file, and the zlib
/// stream passes through an [`IdatWindow`], so the IDAT is never held
/// whole either.
///
/// A duplicate, byte for byte, of the former writer (image 0.25.10's
/// `PngEncoder` at `CompressionType::Uncompressed` over png 0.18.1; pinned by
/// `tests/native_save_bytes.rs`). Its masters: png 0.18.1's `encode_header`
/// (signature and IHDR), the `NoCompression` arm of `write_image_data`
/// (`encoder.rs` 742–783: per row, filter byte 0 then the raw big-endian
/// row, into the compressor), `write_zlib_encoded_idat` (one IDAT, since the zlib
/// stream is under 2^31 − 1 bytes below `MAX_PIXELS`) and `write_iend`; and
/// fdeflate 0.3.7's `StoredOnlyCompressor`, which this drives directly. The
/// compressor patches each stored block's header in place, at most 65,540
/// bytes behind its end, so the IDAT CRC is taken over the bytes the window
/// commits, which are final. The written length must equal
/// [`native_png_encoded_bound`], so the length prefix is written first.
pub(crate) fn put_native_png(
    out: &mut dyn NativeSink,
    image: &PrecisionImage,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    const ENCODING: &str = "high-depth PNG encoding failed";
    if image.runs_rgba16().is_none() {
        return Err("high-depth depth: RGBA16 storage is required".into());
    }
    let (width, height) = image.dimensions();
    let length = native_png_encoded_bound((width, height))?;
    let zlib = length - PNG_FRAMING_BYTES;
    if length > u64::from(MAX_DRDC_PNG_LEN) || zlib > IDAT_CHUNK_MAX_BYTES {
        return Err("encoded high-depth PNG exceeds its admitted bound".into());
    }
    if out
        .position()
        .checked_add(8 + length)
        .is_none_or(|size| size > bound)
    {
        return Err("high-depth native encoding exceeds its admitted size".into());
    }
    if !out.reserve(8 + length as usize) {
        return Err("could not allocate high-depth native output".into());
    }
    out.put(&length.to_le_bytes())?;
    let start = out.position();
    out.put(&metadata::PNG_SIGNATURE)?;
    let mut ihdr = [0_u8; 17];
    ihdr[..4].copy_from_slice(b"IHDR");
    ihdr[4..8].copy_from_slice(&width.to_be_bytes());
    ihdr[8..12].copy_from_slice(&height.to_be_bytes());
    // 16 bits, colour type 6 (RGBA), deflate, adaptive filtering, no interlace.
    ihdr[12..].copy_from_slice(&[16, 6, 0, 0, 0]);
    out.put(&(PNG_IHDR_DATA_BYTES as u32).to_be_bytes())?;
    out.put(&ihdr)?;
    out.put(&crc32fast::hash(&ihdr).to_be_bytes())?;
    out.put(&(zlib as u32).to_be_bytes())?;
    out.put(b"IDAT")?;
    job.report(0.1, 1)?;
    let mut row = reserved_vec(8 * width as usize)?;
    row.resize(8 * width as usize, 0_u8);
    let mut compressor = fdeflate::StoredOnlyCompressor::new(IdatWindow::new(&mut *out)?)
        .map_err(|e| format!("{ENCODING}: {e}"))?;
    let mut i = 0;
    for y in 0..height {
        let mut at = 0;
        for (_, run) in image.row_segments(y, 0, width) {
            let PrecisionRun::Rgba16(run) = run else {
                return Err("high-depth depth: RGBA16 storage is required".into());
            };
            for pixel in run {
                job.every(i)?;
                i += 1;
                for value in pixel {
                    row[at..at + 2].copy_from_slice(&value.to_be_bytes());
                    at += 2;
                }
            }
        }
        if at != row.len() {
            return Err(format!("{ENCODING}: row {y} is short"));
        }
        compressor
            .write_data(&[0])
            .and_then(|()| compressor.write_data(&row))
            .map_err(|e| format!("{ENCODING}: {e}"))?;
    }
    let window = compressor
        .finish()
        .map_err(|e| format!("{ENCODING}: {e}"))?;
    let crc = window.close()?;
    job.report(0.3, 1)?;
    out.put(&crc.to_be_bytes())?;
    out.put(&PNG_IEND)?;
    job.check()?;
    if out.position() - start != length {
        return Err("high-depth native: encoded PNG length differs from its planned bound".into());
    }
    Ok(())
}

/// The IDAT's zlib bytes on their way to the sink: a `Write + Seek`
/// window that keeps at least the last [`IDAT_WINDOW_KEEP_BYTES`] bytes
/// uncommitted, because `StoredOnlyCompressor` seeks back over the stored
/// block it just filled (65,535 bytes and its 5-byte header) to patch the
/// header. Older bytes are final: they go to the sink and into the IDAT CRC,
/// which starts over the chunk type. A seek outside the window is an error
/// (the compressor never makes one). `write` never returns `Err`, because
/// `finish` unwraps its last write: the first sink error is kept and
/// returned by [`IdatWindow::close`].
struct IdatWindow<'s> {
    sink: &'s mut dyn NativeSink,
    crc: crc32fast::Hasher,
    buffer: Vec<u8>,
    /// The stream offset of `buffer[0]`.
    base: u64,
    /// The stream offset the next write lands at.
    pos: u64,
    error: Option<String>,
}

/// Two stored blocks with their headers: more than one seek-back can span.
const IDAT_WINDOW_KEEP_BYTES: usize = 2 * 65_540;
/// The window commits once it holds this much, so with one ≤ 65,535-byte
/// write on top it stays under 256 KiB (`NATIVE_STREAM_WINDOW_BYTES`' writer
/// share).
const IDAT_WINDOW_COMMIT_BYTES: usize = 192 * 1024;

impl<'s> IdatWindow<'s> {
    fn new(sink: &'s mut dyn NativeSink) -> Result<Self, String> {
        let mut crc = crc32fast::Hasher::new();
        crc.update(b"IDAT");
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(IDAT_WINDOW_COMMIT_BYTES + 64 * 1024)
            .map_err(|_| "could not allocate high-depth native output")?;
        Ok(Self {
            sink,
            crc,
            buffer,
            base: 0,
            pos: 0,
            error: None,
        })
    }

    /// Commits the first `count` buffered bytes.
    fn commit(&mut self, count: usize) {
        let bytes = &self.buffer[..count];
        self.crc.update(bytes);
        if self.error.is_none() {
            if let Err(e) = self.sink.put(bytes) {
                self.error = Some(e);
            }
        }
        self.buffer.drain(..count);
        self.base += count as u64;
    }

    /// Commits the rest and answers the IDAT CRC, or the first sink error.
    fn close(mut self) -> Result<u32, String> {
        let rest = self.buffer.len();
        self.commit(rest);
        match self.error {
            Some(e) => Err(e),
            None => Ok(self.crc.finalize()),
        }
    }
}

impl Write for IdatWindow<'_> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let at = (self.pos - self.base) as usize;
        let overlap = (self.buffer.len() - at).min(data.len());
        self.buffer[at..at + overlap].copy_from_slice(&data[..overlap]);
        self.buffer.extend_from_slice(&data[overlap..]);
        self.pos += data.len() as u64;
        let end = self.base + self.buffer.len() as u64;
        if self.pos == end && self.buffer.len() >= IDAT_WINDOW_COMMIT_BYTES {
            self.commit(self.buffer.len() - IDAT_WINDOW_KEEP_BYTES);
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for IdatWindow<'_> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let end = self.base + self.buffer.len() as u64;
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => end.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
        };
        match target {
            Some(at) if (self.base..=end).contains(&at) => {
                self.pos = at;
                Ok(at)
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek outside the native IDAT window",
            )),
        }
    }
}
