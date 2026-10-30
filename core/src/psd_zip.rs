//! ZIP (zlib) PSD plane decoding for layer channels, mask channels and the
//! stored composite, at 8, 16 and 32 bits.
//!
//! Adobe's compression word table (Photoshop File Formats, "Channel image
//! data") is `0 = Raw`, `1 = RLE (PackBits)`, `2 = ZIP without prediction`,
//! `3 = ZIP with prediction`. Words 2 and 3 are a raw zlib stream of the
//! plane's `rows * row_bytes` bytes. Word 3 additionally DELTA-encodes each
//! row: every sample after the first holds its difference from the previous
//! one, so decoding adds the running sum back, wrapping, and a row never
//! predicts across into the next one. At 8 bits the sample is a byte; at 16 it
//! is a big-endian u16. Photoshop 26.10.0 writes word 3 for every layer and
//! mask channel of a 16-bit file. At 32 bits word 3 is Photoshop's
//! byte-plane form instead: each row of `w` big-endian f32 is stored as four
//! planes of `w` bytes (every sample's first byte, then every second, third
//! and fourth), and the delta runs over every byte of that `w · 4`-byte row;
//! decoding adds the bytes back across the whole row, wrapping, then
//! re-interleaves the four planes into big-endian f32.
//!
//! This is the ONE inflate seam in the crate (`miniz_oxide`'s core
//! `decompress`), and `undo_prediction` is the ONE place the per-row
//! prediction is undone. Its encode twin, [`zip_predicted32`], is the one
//! place a 32-bit plane is predicted and deflated (`miniz_oxide`'s
//! `compress_to_vec_zlib`): the little-endian layered TIFF's channels
//! (`tiff_layers_order`), which Photoshop writes as word 3. Its users:
//!
//! - `unpack_zip32` / `unpack_zip16` / `unpack_zip8`: one layer or mask plane. The admitted
//!   plane size IS the output buffer, a stream that inflates to any other
//!   length refuses, and so do bytes after the zlib stream.
//! - `unpack_zip_rows`: the stored composite, which is ONE stream of
//!   `channels * height` rows. Only the first `kept_rows` are inflated, into a
//!   buffer of exactly that size, so a file's saved channels are never
//!   inflated and the header's channel count sizes no allocation.
//! - `inflate_zlib_bounded`: a compressed PNG XMP `iTXt` (`metadata::scan_png`),
//!   bounded by the DRDC blob cap.
//!
//! Corrupt input is an error, never a panic, and each caller's limit IS the
//! inflate limit, so a zip bomb cannot allocate past it. `validate_channel`
//! frames the zlib header; `decode`/`decode16`/`attach`/`attach_typed` decode.

use miniz_oxide::inflate::core::inflate_flags::{
    TINFL_FLAG_PARSE_ZLIB_HEADER, TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
};
use miniz_oxide::inflate::core::{decompress, DecompressorOxide};
use miniz_oxide::inflate::TINFLStatus;

/// The two-byte zlib header the compression word is followed by: RFC 1950's
/// deflate method (CM 8) and the check that the header word is a multiple of
/// 31. `bytes` are the channel bytes AFTER the compression word.
pub(crate) fn validate_zip_header(bytes: &[u8]) -> Result<(), String> {
    let invalid = || "PSD ZIP channel has an invalid zlib header".to_string();
    let (Some(cmf), Some(flg)) = (bytes.first(), bytes.get(1)) else {
        return Err(invalid());
    };
    if cmf & 0x0f != 8 || (u16::from(*cmf) << 8 | u16::from(*flg)) % 31 != 0 {
        return Err(invalid());
    }
    Ok(())
}

/// Undo the per-row delta of compression word 3 in place: within each row of
/// `row_bytes`, every sample after the first is its stored difference plus the
/// previous sample, wrapping. `sample_bytes` is 1 or 2 (big-endian), or 4 for
/// Photoshop's 32-bit byte-plane form (module doc): the delta is undone per
/// BYTE across the whole row, then the row's four byte planes are
/// re-interleaved into big-endian f32 samples.
fn undo_prediction(out: &mut [u8], row_bytes: usize, sample_bytes: usize) -> Result<(), String> {
    if row_bytes == 0 {
        return Ok(());
    }
    if sample_bytes == 4 {
        if !row_bytes.is_multiple_of(4) {
            return Err(
                "PSD ZIP predicted plane has a row length that is not whole samples".into(),
            );
        }
        let width = row_bytes / 4;
        let mut planes = vec![0u8; row_bytes];
        for row in out.chunks_exact_mut(row_bytes) {
            let mut previous = 0u8;
            for (index, (stored, byte)) in row.iter().zip(planes.iter_mut()).enumerate() {
                *byte = if index == 0 {
                    *stored
                } else {
                    previous.wrapping_add(*stored)
                };
                previous = *byte;
            }
            for (x, sample) in row.chunks_exact_mut(4).enumerate() {
                for (k, byte) in sample.iter_mut().enumerate() {
                    *byte = planes[k * width + x];
                }
            }
        }
        return Ok(());
    }
    if sample_bytes == 2 {
        if !row_bytes.is_multiple_of(2) {
            return Err("PSD ZIP predicted plane has an odd row length".into());
        }
        for row in out.chunks_exact_mut(row_bytes) {
            let mut previous = 0u16;
            for (index, sample) in row.chunks_exact_mut(2).enumerate() {
                let value = u16::from_be_bytes([sample[0], sample[1]]);
                let value = if index == 0 {
                    value
                } else {
                    previous.wrapping_add(value)
                };
                sample.copy_from_slice(&value.to_be_bytes());
                previous = value;
            }
        }
    } else {
        for row in out.chunks_exact_mut(row_bytes) {
            let mut previous = 0u8;
            for (index, sample) in row.iter_mut().enumerate() {
                if index != 0 {
                    *sample = previous.wrapping_add(*sample);
                }
                previous = *sample;
            }
        }
    }
    Ok(())
}

/// The encode twin of [`undo_prediction`]'s 32-bit arm: rows of `width`
/// big-endian f32 (`samples_be`, a whole number of rows) as Photoshop's
/// byte-plane form — each row's four byte planes (every sample's first byte,
/// then every second, third and fourth), every byte after the row's first
/// replaced by its wrapping difference from the byte before it — deflated
/// into one zlib stream. The result is a word-3 channel's payload, the bytes
/// after its compression word; the byte planes are the same in either file
/// byte order. Level 6 is `miniz_oxide`'s default; the reader does not
/// depend on it.
pub(crate) fn zip_predicted32(samples_be: &[u8], width: usize) -> Vec<u8> {
    let row_bytes = width * 4;
    let mut predicted = Vec::with_capacity(samples_be.len());
    if row_bytes > 0 {
        let mut planes = vec![0u8; row_bytes];
        for row in samples_be.chunks(row_bytes) {
            let (columns, used) = (row.len() / 4, row.len() / 4 * 4);
            for (x, sample) in row.chunks_exact(4).enumerate() {
                for (k, byte) in sample.iter().enumerate() {
                    planes[k * columns + x] = *byte;
                }
            }
            let mut previous = 0u8;
            for (index, byte) in planes[..used].iter().enumerate() {
                predicted.push(if index == 0 {
                    *byte
                } else {
                    byte.wrapping_sub(previous)
                });
                previous = *byte;
            }
        }
    }
    miniz_oxide::deflate::compress_to_vec_zlib(&predicted, 6)
}

/// One whole plane: inflate to EXACTLY `row_bytes * rows` bytes and undo the
/// prediction when `predicted`.
fn unpack_plane(
    encoded: &[u8],
    row_bytes: usize,
    rows: usize,
    sample_bytes: usize,
    predicted: bool,
) -> Result<Vec<u8>, String> {
    validate_zip_header(encoded)?;
    let expected = row_bytes
        .checked_mul(rows)
        .ok_or("PSD ZIP plane size overflow")?;
    let mut out = vec![0u8; expected];
    let (status, consumed, produced) = decompress(
        &mut DecompressorOxide::new(),
        encoded,
        &mut out,
        0,
        TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    );
    if status != TINFLStatus::Done {
        return Err("PSD ZIP channel could not be decompressed".into());
    }
    if produced != expected {
        return Err("PSD ZIP channel decompressed to the wrong plane length".into());
    }
    if consumed != encoded.len() {
        return Err("PSD ZIP channel has trailing data".into());
    }
    if predicted {
        undo_prediction(&mut out, row_bytes, sample_bytes)?;
    }
    Ok(out)
}

/// Inflate one 16-bit plane to EXACTLY `row_bytes * rows` bytes and undo the
/// per-row prediction when `predicted`. A stream that inflates to a different
/// length, that is malformed, or that is FOLLOWED by bytes the zlib stream
/// does not own refuses — the RLE path refuses trailing data the same way.
/// The output buffer handed to the inflater is the admitted plane size, so
/// nothing larger is ever allocated.
pub fn unpack_zip16(
    encoded: &[u8],
    row_bytes: usize,
    rows: usize,
    predicted: bool,
) -> Result<Vec<u8>, String> {
    unpack_plane(encoded, row_bytes, rows, 2, predicted)
}

/// The 32-bit twin of [`unpack_zip16`]: the same exact-length
/// inflate and refusals, with compression word 3's byte-plane prediction
/// undone (module doc) so the result is `rows` rows of big-endian f32.
pub fn unpack_zip32(
    encoded: &[u8],
    row_bytes: usize,
    rows: usize,
    predicted: bool,
) -> Result<Vec<u8>, String> {
    unpack_plane(encoded, row_bytes, rows, 4, predicted)
}

/// The 8-bit twin of [`unpack_zip16`]: the same exact-length inflate and the
/// same refusals, with compression word 3's delta taken per BYTE.
pub fn unpack_zip8(
    encoded: &[u8],
    row_bytes: usize,
    rows: usize,
    predicted: bool,
) -> Result<Vec<u8>, String> {
    unpack_plane(encoded, row_bytes, rows, 1, predicted)
}

/// The first `kept_rows` rows of a stream that may hold more — the stored
/// composite, whose one stream runs through every channel of the file. The
/// output buffer is exactly `row_bytes * kept_rows` bytes and inflation stops
/// when it is full, so rows past the kept ones are never inflated and the
/// bytes that encode them are NOT read or checked. A stream that ends, or
/// fails, before the kept rows are complete is `Err`.
pub(crate) fn unpack_zip_rows(
    encoded: &[u8],
    row_bytes: usize,
    kept_rows: usize,
    sample_bytes: usize,
    predicted: bool,
) -> Result<Vec<u8>, String> {
    validate_zip_header(encoded)?;
    let expected = row_bytes
        .checked_mul(kept_rows)
        .ok_or("PSD ZIP plane size overflow")?;
    let mut out = vec![0u8; expected];
    let (status, _, produced) = decompress(
        &mut DecompressorOxide::new(),
        encoded,
        &mut out,
        0,
        TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    );
    // `HasMoreOutput` is the full buffer with stream left over: the rows this
    // caller does not keep.
    if !matches!(status, TINFLStatus::Done | TINFLStatus::HasMoreOutput) {
        return Err("PSD ZIP composite could not be decompressed".into());
    }
    if produced != expected {
        return Err("PSD ZIP composite ends before its color planes".into());
    }
    if predicted {
        undo_prediction(&mut out, row_bytes, sample_bytes)?;
    }
    Ok(out)
}

/// Inflate ONE complete zlib stream (RFC 1950 header parsed, Adler-32 verified) whose
/// output may not exceed `limit` bytes; never allocates more than `limit` output bytes.
/// Err on a malformed/truncated stream, a checksum mismatch, output past `limit`, or any
/// byte after the stream ends (the same trailing-data rule as `unpack_zip16`).
pub(crate) fn inflate_zlib_bounded(encoded: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let flags = TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    // The output buffer starts at twice the input and doubles, never past
    // `limit`; the non-wrapping flag makes the whole buffer the back-reference
    // window, so each call resumes at `produced` in the same (grown) buffer.
    let mut out = vec![0u8; encoded.len().saturating_mul(2).min(limit)];
    let mut state = DecompressorOxide::new();
    let mut consumed = 0usize;
    let mut produced = 0usize;
    loop {
        let input = encoded
            .get(consumed..)
            .ok_or("zlib stream consumed past its end")?;
        let (status, used, wrote) = decompress(&mut state, input, &mut out, produced, flags);
        consumed = consumed.saturating_add(used);
        produced = produced.saturating_add(wrote);
        match status {
            TINFLStatus::Done => {
                if consumed != encoded.len() {
                    return Err("zlib stream has trailing data".into());
                }
                out.truncate(produced);
                return Ok(out);
            }
            TINFLStatus::HasMoreOutput => {
                if out.len() >= limit {
                    return Err("zlib stream inflates past its limit".into());
                }
                let grown = out.len().saturating_mul(2).max(1).min(limit);
                out.resize(grown, 0);
            }
            _ => return Err("zlib stream could not be decompressed".into()),
        }
    }
}
