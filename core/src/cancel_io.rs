//! Cancel polling inside a codec call: [`Polled`] wraps the sink an encoder
//! writes or the source a decoder reads, and polls the job as the bytes move.
//!
//! A dependency codec (`image`'s, `png`, `tiff`, the crate's own TIFF writer)
//! is one synchronous call that takes no job, so before this adapter Cancel was
//! seen only before and after it. `Polled` passes at most [`POLL_BYTES`] to each
//! inner `write`, `read` or `fill_buf` (short writes and reads are legal, and
//! `write_all` / `read_exact` loop over them), and polls the job BEFORE the
//! next transfer once that many bytes have moved since its last poll. One
//! checkpoint per 64 KiB moved, so a file smaller than one interval polls
//! nothing and its checkpoint count is unchanged; a refusal becomes an
//! `io::Error` the codec propagates, and [`settle`] turns whatever sentence
//! the codec wraps it in back into exactly `operation cancelled`. Under
//! `JobContext::NONE` it is a plain pass-through: no cap and no count.
//!
//! What it buys depends on when a codec moves its bytes. A streaming encoder
//! is polled while it computes: JPEG writes each entropy-coded byte as it is
//! produced and BMP each pixel, and the crate's uncompressed TIFF writers are
//! polled through their strip writes, which are most of their work. A decoder
//! pulls its input as it inflates or unpacks (PNG through `fill_buf`, TIFF
//! strip by strip), so the 16-bit PNG and TIFF opens are polled throughout the
//! decode, at a rate that follows the ENCODED bytes (a highly compressible PNG
//! polls less per decoded pixel). An encoder that builds its whole output
//! before its first write is polled only while that output is copied out: PNG
//! (the `png` crate deflates, or stores, the whole frame in memory before its
//! IDAT write), WebP (the frame is encoded into a buffer first), GIF (quantized
//! and LZW-coded first) and JPEG XL (`zune-jpegxl` encodes into a buffer).
//! Their compute before the first byte stays one unpolled stretch.
//!
//! Seeks pass through uncounted: the TIFF writer seeks back to patch offsets,
//! and the readers seek to strips and chunks.

use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};

use crate::job_control::{JobContext, CANCELLED};

/// The bytes moved between polls: the block `precision_codec::read_regular`
/// already reads and polls at.
pub(crate) const POLL_BYTES: usize = 64 * 1024;

/// A sink or source that polls `job` every [`POLL_BYTES`] moved (module doc).
pub(crate) struct Polled<'a, T> {
    inner: T,
    job: JobContext<'a>,
    moved: usize,
}

impl<'a, T> Polled<'a, T> {
    pub(crate) fn new(inner: T, job: JobContext<'a>) -> Self {
        Polled {
            inner,
            job,
            moved: 0,
        }
    }

    /// The most one transfer may move: [`POLL_BYTES`] under a control,
    /// anything without one.
    fn cap(&self, len: usize) -> usize {
        if self.job.is_controlled() {
            len.min(POLL_BYTES)
        } else {
            len
        }
    }

    /// One checkpoint once an interval has moved since the last.
    fn poll(&mut self) -> io::Result<()> {
        if self.moved >= POLL_BYTES {
            self.moved = 0;
            self.job.check().map_err(io::Error::other)?;
        }
        Ok(())
    }

    fn count(&mut self, bytes: usize) {
        if self.job.is_controlled() {
            self.moved = self.moved.saturating_add(bytes);
        }
    }
}

impl<T: Write> Write for Polled<'_, T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.poll()?;
        let written = self.inner.write(&buf[..self.cap(buf.len())])?;
        self.count(written);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<T: Read> Read for Polled<'_, T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.poll()?;
        let len = self.cap(buf.len());
        let read = self.inner.read(&mut buf[..len])?;
        self.count(read);
        Ok(read)
    }
}

impl<T: BufRead> BufRead for Polled<'_, T> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.poll()?;
        let cap = self.cap(usize::MAX);
        let buffered = self.inner.fill_buf()?;
        Ok(&buffered[..buffered.len().min(cap)])
    }

    fn consume(&mut self, amount: usize) {
        self.count(amount);
        self.inner.consume(amount);
    }
}

impl<T: Seek> Seek for Polled<'_, T> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

/// A codec call's result under `job`: a failure while a cancel is pending is
/// the cancel, whatever sentence the codec wrapped the poll's error in.
pub(crate) fn settle<T>(job: JobContext<'_>, result: Result<T, String>) -> Result<T, String> {
    match result {
        Err(_) if job.cancelled() => Err(CANCELLED.into()),
        other => other,
    }
}
