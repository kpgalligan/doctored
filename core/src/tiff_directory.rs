//! A classic TIFF's first directory, read header-only, and the byte
//! [`Overlay`] that presents patched directory values to the `tiff` decoder
//! (#1: moved verbatim out of `lab_tiff_read.rs` so the Lab and the
//! palette TIFF opens share one walk; leaf, std only).
//!
//! [`Directory::first`] reads the header (`II` or `MM`, 42, never BigTIFF)
//! and the first directory's entries through a `read(offset, length)`
//! callback that returns exactly `length` bytes or `None`
//! ([`bytes_reader`] over bytes already read, [`file_reader`] over a file,
//! bounded by its size so a forged count cannot size an allocation). Each
//! caller applies its own checks over the entries: `scalar` reads a SHORT
//! or LONG of count 1, `shorts` a SHORT array and where each value lives
//! in the file, so a caller can patch it. The [`Overlay`] lays those
//! patches over the file's bytes as the decoder reads them, the file itself
//! untouched and never copied.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};

/// A directory this large is not a photograph's; refuse rather than walk it.
pub(crate) const MAX_ENTRIES: usize = 4096;

/// The compressions the `tiff` crate decodes losslessly on raw samples:
/// none, LZW, Deflate (8 and the old 32946) and PackBits.
pub(crate) const COMPRESSIONS: &[u32] = &[1, 5, 8, 32946, 32773];

/// One directory entry: its field type, its count, the value field's
/// absolute offset and the value field's four bytes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    pub kind: u16,
    pub count: u32,
    pub at: u64,
    pub value: [u8; 4],
}

/// The first directory of a classic TIFF (module doc).
pub(crate) struct Directory {
    big: bool,
    entries: HashMap<u16, Entry>,
}

impl Directory {
    /// The header and the first directory's entries; `None` for anything
    /// but a classic TIFF whose first directory reads whole.
    pub(crate) fn first(read: &mut dyn FnMut(u64, usize) -> Option<Vec<u8>>) -> Option<Self> {
        let head = read(0, 8)?;
        let big = match &head[..2] {
            b"MM" => true,
            b"II" => false,
            _ => return None,
        };
        let mut directory = Directory {
            big,
            entries: HashMap::new(),
        };
        if directory.u16_of(&head[2..4]) != 42 {
            return None;
        }
        let start = u64::from(directory.u32_of(&head[4..8]));
        let count = usize::from(directory.u16_of(&read(start, 2)?));
        if count > MAX_ENTRIES {
            return None;
        }
        let entries = read(start + 2, count * 12)?;
        for (index, entry) in entries.chunks_exact(12).enumerate() {
            let tag = directory.u16_of(entry);
            let at = start + 2 + index as u64 * 12 + 8;
            let parsed = Entry {
                kind: directory.u16_of(&entry[2..]),
                count: directory.u32_of(&entry[4..8]),
                at,
                value: [entry[8], entry[9], entry[10], entry[11]],
            };
            directory.entries.insert(tag, parsed);
        }
        Some(directory)
    }

    fn u16_of(&self, b: &[u8]) -> u16 {
        if self.big {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        }
    }

    fn u32_of(&self, b: &[u8]) -> u32 {
        if self.big {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        }
    }

    /// The entry of `tag`, when the directory holds one.
    pub(crate) fn entry(&self, tag: u16) -> Option<&Entry> {
        self.entries.get(&tag)
    }

    /// A SHORT or LONG of count 1.
    pub(crate) fn scalar(&self, tag: u16) -> Option<u32> {
        let entry = self.entries.get(&tag)?;
        if entry.count != 1 {
            return None;
        }
        match entry.kind {
            3 => Some(u32::from(self.u16_of(&entry.value))),
            4 => Some(self.u32_of(&entry.value)),
            _ => None,
        }
    }

    /// A SHORT array's values and where each lives in the file:
    /// `Some(None)` when the tag is absent, `None` when it is not a SHORT
    /// array of 1..=`max` values or its values cannot be read.
    pub(crate) fn shorts(
        &self,
        tag: u16,
        max: u32,
        read: &mut dyn FnMut(u64, usize) -> Option<Vec<u8>>,
    ) -> Option<Option<Vec<(u16, u64)>>> {
        let Some(entry) = self.entries.get(&tag).copied() else {
            return Some(None);
        };
        let n = entry.count;
        if entry.kind != 3 || n == 0 || n > max {
            return None;
        }
        let (bytes, start) = if n <= 2 {
            (entry.value[..n as usize * 2].to_vec(), entry.at)
        } else {
            let offset = u64::from(self.u32_of(&entry.value));
            (read(offset, n as usize * 2)?, offset)
        };
        Some(Some(
            bytes
                .chunks_exact(2)
                .enumerate()
                .map(|(i, b)| (self.u16_of(b), start + i as u64 * 2))
                .collect(),
        ))
    }

    /// `value` as a SHORT in the file's byte order (a patch's bytes).
    pub(crate) fn short_bytes(&self, value: u16) -> Vec<u8> {
        if self.big {
            value.to_be_bytes().to_vec()
        } else {
            value.to_le_bytes().to_vec()
        }
    }

    /// A LONG's value field read as an offset (an out-of-line array's).
    pub(crate) fn offset_of(&self, entry: &Entry) -> u64 {
        u64::from(self.u32_of(&entry.value))
    }
}

/// The reader over bytes already read.
pub(crate) fn bytes_reader(bytes: &[u8]) -> impl FnMut(u64, usize) -> Option<Vec<u8>> + '_ {
    move |offset, length| {
        let start = usize::try_from(offset).ok()?;
        bytes
            .get(start..start.checked_add(length)?)
            .map(<[u8]>::to_vec)
    }
}

/// The reader over the file at `path`, bounded by its size. `Err` only when
/// the file cannot be opened.
pub(crate) fn file_reader(path: &str) -> Result<impl FnMut(u64, usize) -> Option<Vec<u8>>, String> {
    let mut file = File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let size = file
        .metadata()
        .map_err(|e| format!("failed to read {path}: {e}"))?
        .len();
    Ok(move |offset: u64, length: usize| -> Option<Vec<u8>> {
        // Bounded by the file, so a forged count cannot size an allocation.
        if offset.checked_add(length as u64)? > size {
            return None;
        }
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut out = vec![0u8; length];
        file.read_exact(&mut out).ok()?;
        Some(out)
    })
}

/// The file's bytes with a walk's patches laid over them as they are read
/// (module doc): the file itself untouched.
pub(crate) struct Overlay<'a> {
    inner: Cursor<&'a [u8]>,
    patches: &'a [(u64, Vec<u8>)],
}

impl<'a> Overlay<'a> {
    /// `bytes` read with `patches` (offset, replacement) laid over them.
    pub(crate) fn new(bytes: &'a [u8], patches: &'a [(u64, Vec<u8>)]) -> Self {
        Overlay {
            inner: Cursor::new(bytes),
            patches,
        }
    }
}

impl Read for Overlay<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let start = self.inner.position();
        let n = self.inner.read(buf)?;
        let end = start + n as u64;
        for (at, bytes) in self.patches {
            for (i, byte) in bytes.iter().enumerate() {
                let offset = at + i as u64;
                if (start..end).contains(&offset) {
                    buf[(offset - start) as usize] = *byte;
                }
            }
        }
        Ok(n)
    }
}

impl Seek for Overlay<'_> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(to)
    }
}
