//! The ONE 8-bit PNG encoder (13C-2a): an RGBA8 or L8 plane written to a
//! [`NativeSink`] row by row, byte for byte the PNG `image`'s `PngEncoder`
//! writes at its defaults (image 0.25.10 over png 0.18.1: `Compression::Fast`,
//! `Filter::Adaptive`). The byte native writer's layer, channel and embedded
//! 8-bit source planes, the tile pool's rebuild of them and the 8-bit flat PNG
//! all go through it, so each keeps every saved byte while holding a few rows
//! instead of the zlib stream.
//!
//! **The master copies.** A private copy of png 0.18.1, kept byte-identical:
//! `Writer::write_image_data`'s `FdeflateUltraFast` arm (`encoder.rs` 787–817:
//! per row the adaptive filter's choice, the filter byte then the filtered row
//! in ONE `write_data` call to fdeflate 0.3.7's `Compressor`; when the
//! finished stream is longer than the stored size of the filtered rows
//! (`StoredOnlyCompressor::compressed_size`), the stored-only stream instead,
//! filter byte 0 and the raw rows);
//! `adaptive_filter` and `filter_internal` (`filter/mod.rs` 361–528: Up, Sub,
//! Avg, Paeth in that order, a cost `<=` the best so far wins so a later tie
//! wins, a zero cost returns at once, the cost the sum of each byte's |i8|);
//! the Paeth predictor `filter_paeth_fpnge` (`filter/paeth.rs` 45–84);
//! `encode_header` (signature; IHDR: 8 bits, colour type 6 or 0, deflate,
//! adaptive, no interlace), one IDAT (`write_zlib_encoded_idat`, whose 2^31 − 1
//! byte chunk `MAX_PIXELS` never reaches) and IEND. `tests/native_rows8_bytes.rs`
//! holds this module to `PngEncoder` byte for byte, so a bump of either crate
//! that changes `PngEncoder`'s bytes fails there, which is the intent: png's
//! own stream writer does not reproduce them, which is why the filter is
//! copied rather than called.
//!
//! **Two passes, never a whole stream.** The IDAT length precedes a zlib
//! stream whose length is known only after the last row. On a sink that
//! rewrites (a file, a `Vec`) one pass writes a placeholder and patches it;
//! when fdeflate grew the data the sink is cut back to the IDAT type and the
//! stored stream written in a second pass over the rows. On a sink that does
//! not (the tile pool's skeleton, which keys bytes as they arrive) a counting
//! pass measures the stream first, and the second pass writes it with its
//! length known. A row source is therefore read once or twice and must
//! answer the same rows each time.
//!
//! **What it holds** ([`scratch_bytes`]): three rows (the previous, the
//! current, and the filtered one with its filter byte), one 64 KiB block of
//! compressed output plus up to one row's worth more, the stored pass's IDAT
//! window (`precision_png::IdatWindow`, under 256 KiB) and the compressor's
//! few words.
//!
//! **Cancel.** The job is polled once per 64 KiB of compressed output
//! (`cancel_io::POLL_BYTES`) and once per 64 KiB of rows the stored pass
//! feeds, so a plane's compute is never one unpolled stretch.

use std::cell::RefCell;
use std::io::Write;

use crate::cancel_io::POLL_BYTES;
use crate::job_control::JobContext;
use crate::native_stream::NativeSink;
use crate::precision_png::IdatWindow;

const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
/// IEND: zero length, the type and its fixed CRC.
const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];
/// The signature and the IHDR chunk (its length, type, 13 data bytes, CRC).
const HEAD_BYTES: u64 = 8 + 12 + 13;
/// A chunk's length, type and CRC around its data.
const CHUNK_FRAMING_BYTES: u64 = 12;
/// png 0.18.1's `MAX_IDAT_CHUNK_LEN`: a longer stream would be split, which
/// this encoder does not do (no 8-bit plane within `MAX_PIXELS` reaches it).
const MAX_IDAT_BYTES: u64 = (u32::MAX >> 1) as u64;
/// What [`scratch_bytes`] charges beside the rows: the 64 KiB output block,
/// the stored pass's IDAT window and the compressor, rounded up.
const FIXED_BYTES: u64 = 1024 * 1024;
/// The rows [`scratch_bytes`] charges: three held, two for the output block's
/// overshoot by one row's compressed bytes (at most ⌈3/2⌉ of it, fdeflate's
/// longest literal code being 12 bits).
const ROWS_HELD: u64 = 5;

const ENCODING: &str = "PNG encoding failed";

/// One 8-bit plane's PNG: its size, its channels (4: RGBA, 1: gray) and the
/// whole chunks written after IHDR and before IEND (empty for a native plane;
/// the flat PNG's profile, metadata and resolution chunks).
pub(crate) struct Png8<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) channels: u8,
    pub(crate) after_ihdr: &'a [u8],
    pub(crate) before_iend: &'a [u8],
}

/// A plane's rows: fills row `y` (`channels · width` bytes, top row 0). It
/// may be asked for every row twice (module doc).
pub(crate) type Rows<'r> = dyn FnMut(u32, &mut [u8]) -> Result<(), String> + 'r;

/// A length-prefixed PNG record in a native file.
pub(crate) struct Record<'a> {
    /// The little-endian length's width: 4 (a byte layer or channel record)
    /// or 8 (an embedded source).
    pub(crate) prefix: usize,
    /// The longest PNG the record may hold (at most `u32::MAX` for a 4-byte
    /// prefix), and the refusal past it.
    pub(crate) cap: u64,
    pub(crate) too_large: &'a str,
    /// Announce the PNG to the sink as one payload (`NativeSink::begin_blob`)
    /// once its length is known.
    pub(crate) blob: bool,
}

impl Png8<'_> {
    fn row_bytes(&self) -> usize {
        usize::from(self.channels) * self.width as usize
    }

    /// The stored-only stream's exact length (fdeflate's own formula).
    fn stored_len(&self) -> u64 {
        fdeflate::StoredOnlyCompressor::<()>::compressed_size(
            (self.row_bytes() + 1) * self.height as usize,
        ) as u64
    }

    /// The PNG's length around a `zlib`-byte stream.
    pub(crate) fn length(&self, zlib: u64) -> u64 {
        HEAD_BYTES
            + self.after_ihdr.len() as u64
            + CHUNK_FRAMING_BYTES
            + zlib
            + self.before_iend.len() as u64
            + IEND.len() as u64
    }

    fn check(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err(format!("{ENCODING}: zero width or height"));
        }
        if !matches!(self.channels, 1 | 4) {
            return Err(format!("{ENCODING}: unsupported channel count"));
        }
        Ok(())
    }
}

/// The encoder's transient beside the sink for a `width`-wide plane of
/// `channels` (module doc): `5·(c·w + 1) + 1 MiB`.
pub(crate) fn scratch_bytes(width: u32, channels: u8) -> u64 {
    ROWS_HELD * (u64::from(channels) * u64::from(width) + 1) + FIXED_BYTES
}

/// Writes a native PNG record: the length in `record.prefix` little-endian
/// bytes, then the PNG. On a sink that rewrites the length is patched once
/// the PNG is written; on one that does not the PNG is measured first, its
/// length written and, with `record.blob`, announced as one payload. Answers
/// the PNG's length.
pub(crate) fn put_png8_record(
    out: &mut dyn NativeSink,
    record: &Record<'_>,
    png: &Png8<'_>,
    rows: &mut Rows<'_>,
    job: JobContext<'_>,
) -> Result<u64, String> {
    png.check()?;
    let prefix = &[0_u8; 8][..record.prefix];
    if out.rewrites() {
        let at = out.position();
        out.put(prefix)?;
        let len = put_rewriting(out, png, rows, job)?;
        if len > record.cap {
            return Err(record.too_large.into());
        }
        out.patch(at, &len.to_le_bytes()[..record.prefix])
            .map(|()| len)
    } else {
        let zlib = measure(png, rows, job)?;
        let len = png.length(zlib.len);
        if len > record.cap {
            return Err(record.too_large.into());
        }
        out.put(&len.to_le_bytes()[..record.prefix])?;
        if record.blob {
            out.begin_blob(len)?;
        }
        put_measured(out, png, zlib, rows, job)
    }
}

/// The zlib stream a measuring pass chose: its length, and whether it is
/// the stored-only form.
#[derive(Clone, Copy)]
struct Zlib {
    len: u64,
    stored: bool,
}

/// One counting pass over the rows (module doc).
fn measure(png: &Png8<'_>, rows: &mut Rows<'_>, job: JobContext<'_>) -> Result<Zlib, String> {
    let (len, _) = deflate_pass(&mut Counter(0), png, rows, job)?;
    let stored = png.stored_len();
    Ok(if len > stored {
        Zlib {
            len: stored,
            stored: true,
        }
    } else {
        Zlib { len, stored: false }
    })
}

/// The single pass on a rewriting sink, with the stored rewrite when
/// fdeflate grew the data.
fn put_rewriting(
    out: &mut dyn NativeSink,
    png: &Png8<'_>,
    rows: &mut Rows<'_>,
    job: JobContext<'_>,
) -> Result<u64, String> {
    let start = out.position();
    let length_at = put_head(out, png, 0)?;
    let (mut len, mut crc) = deflate_pass(out, png, rows, job)?;
    if len > png.stored_len() {
        // Back to just after the IDAT type, which the stored pass's CRC
        // starts over as the first pass's did.
        out.truncate(length_at + 8)?;
        (len, crc) = stored_pass(out, png, rows, job)?;
    }
    idat_length(len)?;
    out.patch(length_at, &(len as u32).to_be_bytes())?;
    put_tail(out, png, crc)?;
    Ok(out.position() - start)
}

/// The second pass on a sink that does not rewrite, its stream measured.
fn put_measured(
    out: &mut dyn NativeSink,
    png: &Png8<'_>,
    zlib: Zlib,
    rows: &mut Rows<'_>,
    job: JobContext<'_>,
) -> Result<u64, String> {
    let start = out.position();
    put_head(out, png, idat_length(zlib.len)?)?;
    let (len, crc) = if zlib.stored {
        stored_pass(out, png, rows, job)?
    } else {
        deflate_pass(out, png, rows, job)?
    };
    if len != zlib.len {
        return Err(format!("{ENCODING}: the rows changed between two passes"));
    }
    put_tail(out, png, crc)?;
    Ok(out.position() - start)
}

fn idat_length(len: u64) -> Result<u32, String> {
    if len > MAX_IDAT_BYTES {
        return Err(format!("{ENCODING}: the image data exceeds one IDAT chunk"));
    }
    Ok(len as u32)
}

/// The signature, IHDR, `after_ihdr`, then the IDAT's length and type;
/// answers the offset of the length.
fn put_head(out: &mut dyn NativeSink, png: &Png8<'_>, idat_len: u32) -> Result<u64, String> {
    out.put(&SIGNATURE)?;
    let mut ihdr = [0_u8; 25];
    ihdr[..4].copy_from_slice(&13_u32.to_be_bytes());
    ihdr[4..8].copy_from_slice(b"IHDR");
    ihdr[8..12].copy_from_slice(&png.width.to_be_bytes());
    ihdr[12..16].copy_from_slice(&png.height.to_be_bytes());
    // 8 bits, colour type 6 (RGBA) or 0 (gray), deflate, adaptive filtering,
    // no interlace.
    let colour = if png.channels == 4 { 6 } else { 0 };
    ihdr[16..21].copy_from_slice(&[8, colour, 0, 0, 0]);
    let crc = crc32fast::hash(&ihdr[4..21]);
    ihdr[21..].copy_from_slice(&crc.to_be_bytes());
    out.put(&ihdr)?;
    out.put(png.after_ihdr)?;
    let at = out.position();
    out.put(&idat_len.to_be_bytes())?;
    out.put(b"IDAT")?;
    Ok(at)
}

fn put_tail(out: &mut dyn NativeSink, png: &Png8<'_>, crc: u32) -> Result<(), String> {
    out.put(&crc.to_be_bytes())?;
    out.put(png.before_iend)?;
    out.put(&IEND)
}

/// A zeroed buffer of `len` bytes, or the allocation refusal.
fn zeroed(len: usize) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(len)
        .map_err(|_| format!("{ENCODING}: could not allocate a row"))?;
    buffer.resize(len, 0);
    Ok(buffer)
}

/// The compressor's output, shared with the row loop, which drains it.
struct Pending<'a>(&'a RefCell<Vec<u8>>);

impl Write for Pending<'_> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The IDAT data on its way out: its CRC (over the chunk type first), its
/// length, and the bytes moved since the last poll.
struct Stream {
    crc: crc32fast::Hasher,
    len: u64,
    moved: usize,
}

impl Stream {
    /// Puts what the compressor wrote once it holds `at_least` bytes, and
    /// polls once per 64 KiB put.
    fn drain(
        &mut self,
        pending: &RefCell<Vec<u8>>,
        out: &mut dyn NativeSink,
        at_least: usize,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let mut bytes = pending.borrow_mut();
        if bytes.is_empty() || bytes.len() < at_least {
            return Ok(());
        }
        self.crc.update(&bytes);
        out.put(&bytes)?;
        self.len += bytes.len() as u64;
        self.moved += bytes.len();
        bytes.clear();
        while self.moved >= POLL_BYTES {
            self.moved -= POLL_BYTES;
            job.check()?;
        }
        Ok(())
    }
}

/// The adaptive-filtered fdeflate stream of every row into `out`: answers
/// its length and the IDAT CRC.
fn deflate_pass(
    out: &mut dyn NativeSink,
    png: &Png8<'_>,
    rows: &mut Rows<'_>,
    job: JobContext<'_>,
) -> Result<(u64, u32), String> {
    let row = png.row_bytes();
    let bpp = usize::from(png.channels);
    let mut previous = zeroed(row)?;
    let mut current = zeroed(row)?;
    let mut filtered = zeroed(row + 1)?;
    let mut block = Vec::new();
    block
        .try_reserve_exact(POLL_BYTES + 2 * (row + 1) + 64)
        .map_err(|_| format!("{ENCODING}: could not allocate its output"))?;
    let pending = RefCell::new(block);
    let mut crc = crc32fast::Hasher::new();
    crc.update(b"IDAT");
    let mut stream = Stream {
        crc,
        len: 0,
        moved: 0,
    };
    let mut compressor =
        fdeflate::Compressor::new(Pending(&pending)).map_err(|e| format!("{ENCODING}: {e}"))?;
    for y in 0..png.height {
        rows(y, &mut current)?;
        filtered[0] = adaptive_filter(bpp, &previous, &current, &mut filtered[1..]);
        compressor
            .write_data(&filtered)
            .map_err(|e| format!("{ENCODING}: {e}"))?;
        stream.drain(&pending, out, POLL_BYTES, job)?;
        std::mem::swap(&mut previous, &mut current);
    }
    compressor
        .finish()
        .map_err(|e| format!("{ENCODING}: {e}"))?;
    stream.drain(&pending, out, 0, job)?;
    Ok((stream.len, stream.crc.finalize()))
}

/// The stored-only stream (filter byte 0, the raw rows) into `out` through
/// the IDAT window: answers its length and the IDAT CRC.
fn stored_pass(
    out: &mut dyn NativeSink,
    png: &Png8<'_>,
    rows: &mut Rows<'_>,
    job: JobContext<'_>,
) -> Result<(u64, u32), String> {
    let row = png.row_bytes();
    let mut current = zeroed(row)?;
    let mut compressor = fdeflate::StoredOnlyCompressor::new(IdatWindow::new(&mut *out)?)
        .map_err(|e| format!("{ENCODING}: {e}"))?;
    let mut moved = 0;
    for y in 0..png.height {
        rows(y, &mut current)?;
        compressor
            .write_data(&[0])
            .and_then(|()| compressor.write_data(&current))
            .map_err(|e| format!("{ENCODING}: {e}"))?;
        moved += row + 1;
        while moved >= POLL_BYTES {
            moved -= POLL_BYTES;
            job.check()?;
        }
    }
    let crc = compressor
        .finish()
        .map_err(|e| format!("{ENCODING}: {e}"))?
        .close()?;
    Ok((png.stored_len(), crc))
}

/// A sink that only counts: the measuring pass's.
struct Counter(u64);

impl NativeSink for Counter {
    fn position(&self) -> u64 {
        self.0
    }
    fn put(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.0 += bytes.len() as u64;
        Ok(())
    }
}

/// png's row filters, by their filter-type byte.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowFilter {
    Sub = 1,
    Up = 2,
    Avg = 3,
    Paeth = 4,
}

/// png 0.18.1's `adaptive_filter` with its `sum_buffer` cost (module doc):
/// leaves the chosen filter's output in `output` and answers its byte.
fn adaptive_filter(bpp: usize, previous: &[u8], current: &[u8], output: &mut [u8]) -> u8 {
    let mut min_cost = u64::MAX;
    let mut choice = RowFilter::Up;
    for filter in [
        RowFilter::Up,
        RowFilter::Sub,
        RowFilter::Avg,
        RowFilter::Paeth,
    ] {
        filter_row(filter, bpp, previous, current, output);
        let cost = output
            .iter()
            .map(|&b| u64::from((b as i8).unsigned_abs()))
            .sum::<u64>();
        if cost <= min_cost {
            min_cost = cost;
            choice = filter;
            if cost == 0 {
                return choice as u8;
            }
        }
    }
    if choice != RowFilter::Paeth {
        filter_row(choice, bpp, previous, current, output);
    }
    choice as u8
}

/// png 0.18.1's `filter_internal` for one filter: `output[i]` is
/// `current[i]` less the filter's prediction, wrapping. Where a prediction
/// reads `current[i - bpp]` or `previous[i - bpp]`, the whole slice is zipped
/// against the `[bpp..]` ones; the first `bpp` bytes predict from zero.
fn filter_row(filter: RowFilter, bpp: usize, previous: &[u8], current: &[u8], output: &mut [u8]) {
    match filter {
        RowFilter::Up => {
            for ((o, &x), &b) in output.iter_mut().zip(current).zip(previous) {
                *o = x.wrapping_sub(b);
            }
        }
        RowFilter::Sub => {
            let (head, tail) = output.split_at_mut(bpp);
            head.copy_from_slice(&current[..bpp]);
            for ((o, &x), &a) in tail.iter_mut().zip(&current[bpp..]).zip(current) {
                *o = x.wrapping_sub(a);
            }
        }
        RowFilter::Avg => {
            let (head, tail) = output.split_at_mut(bpp);
            for ((o, &x), &b) in head.iter_mut().zip(current).zip(previous) {
                *o = x.wrapping_sub(b / 2);
            }
            let rest = tail
                .iter_mut()
                .zip(&current[bpp..])
                .zip(current)
                .zip(&previous[bpp..]);
            for (((o, &x), &a), &b) in rest {
                // The bitwise average png uses: ⌊(a + b) / 2⌋ without widening.
                *o = x.wrapping_sub((a & b) + ((a ^ b) >> 1));
            }
        }
        RowFilter::Paeth => {
            let (head, tail) = output.split_at_mut(bpp);
            for ((o, &x), &b) in head.iter_mut().zip(current).zip(previous) {
                *o = x.wrapping_sub(paeth(0, b, 0));
            }
            let rest = tail
                .iter_mut()
                .zip(&current[bpp..])
                .zip(current)
                .zip(&previous[bpp..])
                .zip(previous);
            for ((((o, &x), &a), &b), &c) in rest {
                *o = x.wrapping_sub(paeth(a, b, c));
            }
        }
    }
}

/// png 0.18.1's `filter_paeth_fpnge`: the PNG Paeth predictor on unsigned
/// bytes (`pc` is only exact when `c` lies between `a` and `b`; otherwise it
/// exceeds both `pa` and `pb`, so 255 decides the same).
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let pa = b.max(c) - c.min(b);
    let pb = a.max(c) - c.min(a);
    let pc = if (a < c) == (c < b) {
        pa.max(pb) - pa.min(pb)
    } else {
        255
    };
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}
