//! The tile pool's skeleton file (13C-1): a native file with its pixel
//! payloads replaced by pool keys, so a state written to the pool is the
//! skeleton plus the payloads the pool did not hold yet. The rebuild
//! (`doc_tile_pool`) walks it back into the native file byte for byte.
//!
//! Layout, little-endian: `DRSK`, version u32 (1), kind u8 (0 a byte
//! document, 1 a typed one), then segments, each a tag byte:
//!
//! - `1` literal: u32 length (at most [`LITERAL_MAX`]), then that many bytes
//!   of the native file, verbatim;
//! - `2` tiles: u8 site ([`Site`]), u32 width, u32 height, u32 tile count
//!   (`⌈w/256⌉·⌈h/256⌉`), then the keys in `TilePlane` index order;
//! - `3` blob: u64 length, u32 chunk count (`⌈len/1 MiB⌉`), then the keys of
//!   its `BLOB_CHUNK_BYTES` chunks from the payload's first byte;
//! - `0xFF` end, then a SHA-256 over every byte before the digest.
//!
//! The site fixes both the sample type and the encoder that writes the
//! payload back. The skeleton is checked whole before any of it is used: a
//! digest that differs, or a segment that does not parse, is `damaged tile
//! pool skeleton <path>`. It is an internal format of the pool, not a
//! `.doctored` word; a later layout raises the version.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};

use crate::sha256::Sha256;
use crate::tile_pool::{Key, BLOB_CHUNK_BYTES};
use crate::tile_store::TILE_EDGE;

const MAGIC: &[u8; 4] = b"DRSK";
const VERSION: u32 = 1;
/// A literal record's cap: the skeleton sink holds at most this much
/// literal before it writes a record.
pub(crate) const LITERAL_MAX: usize = 1 << 20;

const LITERAL: u8 = 1;
const TILES: u8 = 2;
const BLOB: u8 = 3;
const END: u8 = 0xFF;

pub(crate) const KIND_BYTE: u8 = 0;
pub(crate) const KIND_TYPED: u8 = 1;

/// A raster payload site: the sample type and the encoder that writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Site {
    /// A byte layer record's u32 length and RGBA8 PNG (`drdc::put_layer_png`).
    Rgba8Png = 1,
    /// A stored RGBA16 PNG (`precision_png::put_native_png`).
    Rgba16Png = 2,
    /// f32 samples (`float_native::put_samples`).
    Float = 3,
    /// CMYKA16 rows (`cmyk_native::put_payload`).
    Cmyk = 4,
}

impl Site {
    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            1 => Self::Rgba8Png,
            2 => Self::Rgba16Png,
            3 => Self::Float,
            4 => Self::Cmyk,
            _ => return None,
        })
    }
}

/// The tile count of a `width × height` plane.
pub(crate) fn tile_count(width: u32, height: u32) -> u64 {
    u64::from(width.div_ceil(TILE_EDGE)) * u64::from(height.div_ceil(TILE_EDGE))
}

/// The chunk count of a `len`-byte blob.
pub(crate) fn chunk_count(len: u64) -> u64 {
    len.div_ceil(BLOB_CHUNK_BYTES as u64)
}

/// Streams a skeleton to the temporary `save_atomically` renames into place.
pub(crate) struct SkeletonWriter {
    out: BufWriter<File>,
    hasher: Sha256,
    path: String,
}

impl SkeletonWriter {
    pub(crate) fn create(temporary: &str, path: &str, kind: u8) -> Result<Self, String> {
        let file = File::create(temporary).map_err(|e| format!("failed to write {path}: {e}"))?;
        let mut writer = Self {
            out: BufWriter::with_capacity(256 * 1024, file),
            hasher: Sha256::new(),
            path: path.to_string(),
        };
        writer.put(MAGIC)?;
        writer.put(&VERSION.to_le_bytes())?;
        writer.put(&[kind])?;
        Ok(writer)
    }

    fn put(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.hasher.update(bytes);
        self.out
            .write_all(bytes)
            .map_err(|e| format!("failed to write {}: {e}", self.path))
    }

    /// Native bytes, verbatim, in records of at most [`LITERAL_MAX`].
    pub(crate) fn literal(&mut self, bytes: &[u8]) -> Result<(), String> {
        for chunk in bytes.chunks(LITERAL_MAX) {
            self.put(&[LITERAL])?;
            self.put(&(chunk.len() as u32).to_le_bytes())?;
            self.put(chunk)?;
        }
        Ok(())
    }

    pub(crate) fn tiles(
        &mut self,
        site: Site,
        (w, h): (u32, u32),
        keys: &[Key],
    ) -> Result<(), String> {
        self.put(&[TILES, site as u8])?;
        for value in [w, h, keys.len() as u32] {
            self.put(&value.to_le_bytes())?;
        }
        keys.iter().try_for_each(|key| self.put(key))
    }

    pub(crate) fn blob(&mut self, len: u64, keys: &[Key]) -> Result<(), String> {
        self.put(&[BLOB])?;
        self.put(&len.to_le_bytes())?;
        self.put(&(keys.len() as u32).to_le_bytes())?;
        keys.iter().try_for_each(|key| self.put(key))
    }

    /// The end record and the digest; flushes the buffer.
    pub(crate) fn finish(mut self) -> Result<(), String> {
        self.put(&[END])?;
        let digest = self.hasher.clone().finish();
        let path = self.path;
        let mut out = self.out;
        out.write_all(&digest)
            .and_then(|()| out.flush())
            .map_err(|e| format!("failed to write {path}: {e}"))
    }
}

/// One segment, in file order.
pub(crate) enum Segment {
    Literal(Vec<u8>),
    Tiles {
        site: Site,
        width: u32,
        height: u32,
        keys: Vec<Key>,
    },
    Blob {
        len: u64,
        keys: Vec<Key>,
    },
}

/// A verified skeleton, read segment by segment.
pub(crate) struct SkeletonReader {
    input: BufReader<File>,
    /// Bytes left before the digest.
    left: u64,
    path: String,
    pub(crate) kind: u8,
    ended: bool,
}

impl SkeletonReader {
    /// Opens `path`, checks its digest over the whole file, then its header.
    pub(crate) fn open(path: &str) -> Result<Self, String> {
        let damaged = || format!("damaged tile pool skeleton {path}");
        let file = File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
        let length = file
            .metadata()
            .map_err(|e| format!("failed to read {path}: {e}"))?
            .len();
        // Header (9) + end tag (1) + digest (32).
        if length < 42 {
            return Err(damaged());
        }
        let body = length - 32;
        let mut input = BufReader::with_capacity(256 * 1024, file);
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 256 * 1024];
        let mut done = 0_u64;
        while done < body {
            let n = ((body - done) as usize).min(buffer.len());
            input.read_exact(&mut buffer[..n]).map_err(|_| damaged())?;
            hasher.update(&buffer[..n]);
            done += n as u64;
        }
        let mut stored = [0_u8; 32];
        input.read_exact(&mut stored).map_err(|_| damaged())?;
        if hasher.finish() != stored {
            return Err(damaged());
        }
        let file = File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
        let mut reader = Self {
            input: BufReader::with_capacity(256 * 1024, file),
            left: body,
            path: path.to_string(),
            kind: 0,
            ended: false,
        };
        let mut head = [0_u8; 9];
        reader.read(&mut head)?;
        if &head[..4] != MAGIC
            || u32::from_le_bytes([head[4], head[5], head[6], head[7]]) != VERSION
            || head[8] > KIND_TYPED
        {
            return Err(damaged());
        }
        reader.kind = head[8];
        Ok(reader)
    }

    fn damaged(&self) -> String {
        format!("damaged tile pool skeleton {}", self.path)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<(), String> {
        if buf.len() as u64 > self.left {
            return Err(self.damaged());
        }
        self.input.read_exact(buf).map_err(|_| self.damaged())?;
        self.left -= buf.len() as u64;
        Ok(())
    }

    fn u32(&mut self) -> Result<u32, String> {
        let mut bytes = [0_u8; 4];
        self.read(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn keys(&mut self, count: u64) -> Result<Vec<Key>, String> {
        if count.saturating_mul(32) > self.left {
            return Err(self.damaged());
        }
        let mut keys = vec![[0_u8; 32]; count as usize];
        for key in &mut keys {
            self.read(key)?;
        }
        Ok(keys)
    }

    /// The next segment; `None` after the end record, which must be the last
    /// byte before the digest.
    pub(crate) fn next_segment(&mut self) -> Result<Option<Segment>, String> {
        if self.ended {
            return Ok(None);
        }
        let mut tag = [0_u8; 1];
        self.read(&mut tag)?;
        match tag[0] {
            LITERAL => {
                let len = self.u32()? as usize;
                if len == 0 || len > LITERAL_MAX {
                    return Err(self.damaged());
                }
                let mut bytes = vec![0_u8; len];
                self.read(&mut bytes)?;
                Ok(Some(Segment::Literal(bytes)))
            }
            TILES => {
                let mut site = [0_u8; 1];
                self.read(&mut site)?;
                let site = Site::from_byte(site[0]).ok_or_else(|| self.damaged())?;
                let (width, height, count) = (self.u32()?, self.u32()?, self.u32()?);
                let pixels = u64::from(width) * u64::from(height);
                if pixels == 0
                    || pixels > crate::doc::MAX_PIXELS
                    || u64::from(count) != tile_count(width, height)
                {
                    return Err(self.damaged());
                }
                let keys = self.keys(u64::from(count))?;
                Ok(Some(Segment::Tiles {
                    site,
                    width,
                    height,
                    keys,
                }))
            }
            BLOB => {
                let mut len = [0_u8; 8];
                self.read(&mut len)?;
                let len = u64::from_le_bytes(len);
                let count = self.u32()?;
                if len == 0 || u64::from(count) != chunk_count(len) {
                    return Err(self.damaged());
                }
                let keys = self.keys(u64::from(count))?;
                Ok(Some(Segment::Blob { len, keys }))
            }
            END if self.left == 0 => {
                self.ended = true;
                Ok(None)
            }
            _ => Err(self.damaged()),
        }
    }

    /// Every key the skeleton names (the collection's live set).
    pub(crate) fn all_keys(
        mut self,
        into: &mut std::collections::HashSet<Key>,
    ) -> Result<(), String> {
        while let Some(segment) = self.next_segment()? {
            match segment {
                Segment::Literal(_) => {}
                Segment::Tiles { keys, .. } | Segment::Blob { keys, .. } => into.extend(keys),
            }
        }
        Ok(())
    }
}
