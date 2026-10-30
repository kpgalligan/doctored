//! The typed `.rz` reader's and writer's I/O: a bounds-checked
//! [`Reader`] over either a caller's byte slice or an open file, and the
//! [`NativeSink`] a writer streams into (a `Vec<u8>` or a temporary file).
//!
//! **Reading a file.** Nothing maps or buffers the whole file. Every read is
//! a positional `pread` (`FileExt::read_at`) at an offset the reader already
//! checked against the length it saw at open, so a file truncated or grown
//! by another process can only make a read come back short, which is an
//! `Err` ("high-depth native file shrank during read"), never a panic and
//! never a SIGBUS (the reason this is a stream and not an `mmap`). Bytes
//! appended after the open are never read: the length is fixed at open.
//!
//! Two kinds of field come out of a file. A SPAN ([`Reader::take_payload`])
//! is a pixel payload — a 16-bit raster PNG, a float raster, a mask or
//! channel plane, an RSRC asset's PNG or f32 body, a skipped SOCT entry —
//! recorded as an offset and a length and read again only when it decodes
//! ([`Payload`]). Everything else is a SMALL TAKE ([`Reader::take`]): a
//! slot, a table, a WARP original, a RAW or RBIN blob. In file mode a take
//! is copied into an arena that lives as long as the [`NativeFile`], so it
//! can be borrowed exactly as a slice of a whole buffer was; each arena
//! allocation is admitted against the caller's working limit first. The
//! numeric getters read through a 1 MiB read-ahead and are not counted.
//!
//! **What the reader holds** ([`Reader::held_bytes`]): the bytes `take`
//! returned plus [`NATIVE_STREAM_WINDOW_BYTES`], counted the same way over a
//! slice and over a file, so `parse_native` and `open_native` admit the same
//! figure. The typed plan's held term is `encoded_bound − streamed +
//! NATIVE_STREAM_WINDOW_BYTES` (`precision_native_plan`), which dominates it.
//!
//! **Writing.** A writer appends through [`NativeSink`]: a `Vec<u8>` (the
//! in-memory twin, and the byte writer, which still assembles its file) or a
//! [`FileSink`], a 1 MiB `BufWriter` over the temporary file `save_atomically`
//! renames into place.

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;

use crate::precision_image::{admit, BOOKKEEPING_BYTES};

/// The fixed transient the streamed reader and writer hold beside the
/// document, charged on both sides of the typed plan. Reader: the 1 MiB
/// numeric read-ahead plus one payload's 1 MiB section buffer (a PNG
/// decoder's `BufReader`, a float row block, a plane block). Writer: the
/// 1 MiB `BufWriter` plus the IDAT window (under 256 KiB). Both sum to at
/// most 2.25 MiB; 4 MiB leaves the rest for the allocator's rounding of
/// those buffers and the decoder's own small state, which the reader's
/// measurement (`precision_native_plan`'s `# Streamed reader and writer
///`) covers.
pub(crate) const NATIVE_STREAM_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

/// The numeric read-ahead and a payload reader's section buffer: one MiB
/// each, so a header of many small fields costs one `pread` per MiB rather
/// than one per field.
const READ_AHEAD_BYTES: usize = 1024 * 1024;
const SECTION_BUFFER_BYTES: usize = 1024 * 1024;
/// The writer's `BufWriter`: large enough that a 64 KiB `append` chunk never
/// reaches the file as a write of its own.
const SINK_BUFFER_BYTES: usize = 1024 * 1024;

const SHRANK: &str = "high-depth native file shrank during read";
const END: &str = "unexpected end of file";

/// Where a typed writer's bytes go.
pub(crate) trait NativeSink {
    /// The bytes written so far.
    fn position(&self) -> u64;
    /// Appends `bytes`.
    fn put(&mut self, bytes: &[u8]) -> Result<(), String>;
    /// Makes room for `additional` more bytes; `false` when it cannot. A
    /// `Vec` reserves exactly, as the writers did before the sink (each
    /// caller keeps its own allocation-failure sentence); a file needs no
    /// room.
    fn reserve(&mut self, _additional: usize) -> bool {
        true
    }
}

impl NativeSink for Vec<u8> {
    fn position(&self) -> u64 {
        self.len() as u64
    }
    fn put(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.extend_from_slice(bytes);
        Ok(())
    }
    fn reserve(&mut self, additional: usize) -> bool {
        self.try_reserve_exact(additional).is_ok()
    }
}

/// The temporary file a typed save streams into.
pub(crate) struct FileSink {
    out: BufWriter<File>,
    written: u64,
    path: String,
}

impl FileSink {
    /// Creates `temporary`; errors name the destination `path`.
    pub(crate) fn create(temporary: &str, path: &str) -> Result<Self, String> {
        let file = File::create(temporary).map_err(|e| format!("failed to write {path}: {e}"))?;
        Ok(Self {
            out: BufWriter::with_capacity(SINK_BUFFER_BYTES, file),
            written: 0,
            path: path.to_string(),
        })
    }

    /// Flushes the buffer to the file.
    pub(crate) fn finish(self) -> Result<(), String> {
        let path = self.path;
        self.out
            .into_inner()
            .map_err(|e| format!("failed to write {path}: {}", e.error()))?;
        Ok(())
    }
}

impl NativeSink for FileSink {
    fn position(&self) -> u64 {
        self.written
    }
    fn put(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.out
            .write_all(bytes)
            .map_err(|e| format!("failed to write {}: {e}", self.path))?;
        self.written += bytes.len() as u64;
        Ok(())
    }
}

/// An open typed `.rz` file: its handle, the length seen at open, and the
/// arena that holds every small take until the file is dropped.
pub(crate) struct NativeFile {
    file: File,
    length: u64,
    path: String,
    arena: RefCell<Vec<Box<[u8]>>>,
    arena_bytes: Cell<u64>,
    limit: u64,
    other_retained: u64,
}

impl NativeFile {
    /// Opens `path` for streamed reading: a regular file, at most
    /// `precision_native::MAX_NATIVE_BYTES` long, and the reader's fixed
    /// window admitted against `limit` beside `other_retained`.
    pub(crate) fn open(path: &str, limit: u64, other_retained: u64) -> Result<Self, String> {
        crate::precision_codec::require_regular_input(path)?;
        let file = File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
        let length = file
            .metadata()
            .map_err(|e| format!("failed to inspect {path}: {e}"))?
            .len();
        if length > crate::precision_native::MAX_NATIVE_BYTES {
            return Err("high-depth native file exceeds the encoded size limit".into());
        }
        admit(
            NATIVE_STREAM_WINDOW_BYTES,
            BOOKKEEPING_BYTES,
            other_retained,
            limit,
        )?;
        Ok(Self {
            file,
            length,
            path: path.to_string(),
            arena: RefCell::new(Vec::new()),
            arena_bytes: Cell::new(0),
            limit,
            other_retained,
        })
    }

    /// The length seen at open.
    pub(crate) fn len(&self) -> u64 {
        self.length
    }

    /// Fills `buf` from `offset`; a short read means the file shrank.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let mut done = 0;
        while done < buf.len() {
            match self.file.read_at(&mut buf[done..], offset + done as u64) {
                Ok(0) => return Err(SHRANK.into()),
                Ok(n) => done += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(format!("failed to read {}: {e}", self.path)),
            }
        }
        Ok(())
    }

    /// Admits `n` more arena bytes, then reads `[offset, offset + n)` into a
    /// new arena box and lends it for the file's lifetime.
    fn take(&self, offset: u64, n: usize, ahead: &mut ReadAhead) -> Result<&[u8], String> {
        let total = self
            .arena_bytes
            .get()
            .checked_add(n as u64)
            .ok_or("high-depth native byte overflow")?;
        admit(
            total,
            NATIVE_STREAM_WINDOW_BYTES + BOOKKEEPING_BYTES,
            self.other_retained,
            self.limit,
        )?;
        let mut data = Vec::new();
        data.try_reserve_exact(n)
            .map_err(|_| "could not allocate high-depth native input")?;
        data.resize(n, 0);
        if n <= READ_AHEAD_BYTES / 2 {
            data.copy_from_slice(ahead.bytes(self, offset, n)?);
        } else {
            self.read_exact_at(offset, &mut data)?;
        }
        // The pointer is taken from the box only after it is stored: moving a
        // `Box` by value retags it as unique, which would invalidate a pointer
        // taken before the move (Stacked Borrows; Miri flags it).
        let (pointer, length) = {
            let mut arena = self.arena.borrow_mut();
            arena.push(data.into_boxed_slice());
            let slot = arena.last().ok_or("high-depth native arena is empty")?;
            (slot.as_ptr(), slot.len())
        };
        self.arena_bytes.set(total);
        // SAFETY: the pointer comes from the box as stored in the arena, after
        // its last by-value move. The box's heap allocation never moves (a
        // later push that reallocates the `Vec` copies only the box pointers
        // and never retags them), and the arena only grows: no box is removed
        // or freed before `self` drops. The returned slice borrows `self`, so
        // it cannot outlive the arena, and nothing ever writes to an arena box
        // after this point.
        Ok(unsafe { std::slice::from_raw_parts(pointer, length) })
    }
}

/// The numeric getters' read-ahead over a file: one MiB from the last miss.
#[derive(Default)]
struct ReadAhead {
    bytes: Vec<u8>,
    at: u64,
}

impl ReadAhead {
    /// `[offset, offset + n)`, which the caller has checked lies inside the
    /// file, from the buffer, refilled from `offset` on a miss.
    fn bytes(&mut self, file: &NativeFile, offset: u64, n: usize) -> Result<&[u8], String> {
        let start = offset.wrapping_sub(self.at);
        if offset < self.at || start + n as u64 > self.bytes.len() as u64 {
            let want = (file.len() - offset).min(READ_AHEAD_BYTES as u64) as usize;
            if self.bytes.capacity() < READ_AHEAD_BYTES {
                self.bytes
                    .try_reserve_exact(READ_AHEAD_BYTES)
                    .map_err(|_| "could not allocate high-depth native input")?;
            }
            self.bytes.resize(want, 0);
            // A failed fill leaves nothing valid behind.
            self.at = offset;
            if let Err(e) = file.read_exact_at(offset, &mut self.bytes) {
                self.bytes.clear();
                return Err(e);
            }
            return Ok(&self.bytes[..n]);
        }
        let start = start as usize;
        Ok(&self.bytes[start..start + n])
    }
}

enum Source<'a> {
    Slice(&'a [u8]),
    File {
        file: &'a NativeFile,
        ahead: ReadAhead,
    },
}

/// Bounds-checked little-endian reader over a typed or byte DRDC body.
pub(crate) struct Reader<'a> {
    source: Source<'a>,
    pos: u64,
    taken: u64,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            source: Source::Slice(bytes),
            pos: 0,
            taken: 0,
        }
    }

    pub(crate) fn over_file(file: &'a NativeFile) -> Self {
        Self {
            source: Source::File {
                file,
                ahead: ReadAhead::default(),
            },
            pos: 0,
            taken: 0,
        }
    }

    fn len(&self) -> u64 {
        match &self.source {
            Source::Slice(bytes) => bytes.len() as u64,
            Source::File { file, .. } => file.len(),
        }
    }

    pub(crate) fn remaining(&self) -> usize {
        (self.len() - self.pos) as usize
    }

    /// The end of a field of `n` bytes at the cursor, if the input holds it.
    fn end(&self, n: u64) -> Result<u64, String> {
        self.pos
            .checked_add(n)
            .filter(|&e| e <= self.len())
            .ok_or_else(|| END.to_string())
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.end(n as u64)?;
        let slice = match &mut self.source {
            Source::Slice(bytes) => &bytes[self.pos as usize..end as usize],
            Source::File { .. } if n == 0 => &[],
            Source::File { file, ahead } => {
                let file: &'a NativeFile = file;
                file.take(self.pos, n, ahead)?
            }
        };
        self.pos = end;
        self.taken += n as u64;
        Ok(slice)
    }

    /// Records the next `n` bytes as a span without reading them.
    pub(crate) fn take_payload(&mut self, n: u64) -> Result<Payload<'a>, String> {
        let end = self.end(n)?;
        let payload = match &self.source {
            Source::Slice(bytes) => Payload::Bytes(&bytes[self.pos as usize..end as usize]),
            Source::File { file, .. } => Payload::File {
                file,
                offset: self.pos,
                len: n,
            },
        };
        self.pos = end;
        Ok(payload)
    }

    /// The bytes `take` returned plus the fixed window (module doc).
    pub(crate) fn held_bytes(&self) -> u64 {
        self.taken + NATIVE_STREAM_WINDOW_BYTES
    }

    /// A fixed-width field, not counted in [`Self::held_bytes`].
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let end = self.end(N as u64)?;
        let mut out = [0_u8; N];
        match &mut self.source {
            Source::Slice(bytes) => out.copy_from_slice(&bytes[self.pos as usize..end as usize]),
            Source::File { file, ahead } => out.copy_from_slice(ahead.bytes(file, self.pos, N)?),
        }
        self.pos = end;
        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, String> {
        Ok(self.fixed::<1>()?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.fixed()?))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.fixed()?))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.fixed()?))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.fixed()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.fixed()?))
    }

    /// The version-8 guide block's currency: a guide position and each ruler
    /// origin component are geometric quantities that must survive an Image
    /// Size round trip, so they are stored at f64 precision.
    pub(crate) fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.fixed()?))
    }
}

/// A span: a pixel payload borrowed from a slice, or its offset and length
/// in an open file, read only when it decodes.
#[derive(Clone, Copy)]
pub(crate) enum Payload<'a> {
    Bytes(&'a [u8]),
    File {
        file: &'a NativeFile,
        offset: u64,
        len: u64,
    },
}

impl<'a> Payload<'a> {
    pub(crate) fn len(&self) -> u64 {
        match self {
            Payload::Bytes(bytes) => bytes.len() as u64,
            Payload::File { len, .. } => *len,
        }
    }

    /// The first `min(n, len)` bytes.
    pub(crate) fn head(&self, n: usize) -> Result<Vec<u8>, String> {
        let n = (n as u64).min(self.len()) as usize;
        let mut out = vec![0_u8; n];
        self.read_at(0, &mut out)?;
        Ok(out)
    }

    /// Fills `buf` from `offset` within the payload, exactly.
    pub(crate) fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let end = offset
            .checked_add(buf.len() as u64)
            .filter(|&e| e <= self.len())
            .ok_or(END)?;
        match self {
            Payload::Bytes(bytes) => {
                buf.copy_from_slice(&bytes[offset as usize..end as usize]);
                Ok(())
            }
            Payload::File {
                file,
                offset: start,
                ..
            } => file.read_exact_at(start + offset, buf),
        }
    }

    /// A buffered reader over the payload: a `Cursor` over a slice (the
    /// slice path's codec calls are unchanged), else a 1 MiB `BufReader`
    /// over the file section.
    pub(crate) fn reader(&self) -> PayloadReader<'a> {
        match *self {
            Payload::Bytes(bytes) => PayloadReader::Bytes(Cursor::new(bytes)),
            Payload::File { file, offset, len } => PayloadReader::File(BufReader::with_capacity(
                SECTION_BUFFER_BYTES,
                Section {
                    file,
                    start: offset,
                    len,
                    pos: 0,
                },
            )),
        }
    }
}

/// One span of a [`NativeFile`] as a `Read + Seek` stream. A short read
/// inside the span is an I/O error naming the shrink.
pub(crate) struct Section<'a> {
    file: &'a NativeFile,
    start: u64,
    len: u64,
    pos: u64,
}

impl Read for Section<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let n = (self.len - self.pos).min(buf.len() as u64) as usize;
        self.file
            .read_exact_at(self.start + self.pos, &mut buf[..n])
            .map_err(std::io::Error::other)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Section<'_> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.len.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
        };
        self.pos = target.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek before the start of a native payload",
            )
        })?;
        Ok(self.pos)
    }
}

/// A [`Payload`]'s reader: `Read + BufRead + Seek` either way.
pub(crate) enum PayloadReader<'a> {
    Bytes(Cursor<&'a [u8]>),
    File(BufReader<Section<'a>>),
}

impl Read for PayloadReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            PayloadReader::Bytes(r) => r.read(buf),
            PayloadReader::File(r) => r.read(buf),
        }
    }
}

impl BufRead for PayloadReader<'_> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        match self {
            PayloadReader::Bytes(r) => r.fill_buf(),
            PayloadReader::File(r) => r.fill_buf(),
        }
    }
    fn consume(&mut self, amount: usize) {
        match self {
            PayloadReader::Bytes(r) => r.consume(amount),
            PayloadReader::File(r) => r.consume(amount),
        }
    }
}

impl Seek for PayloadReader<'_> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        match self {
            PayloadReader::Bytes(r) => r.seek(to),
            PayloadReader::File(r) => r.seek(to),
        }
    }
}
