//! The content-addressed tile pool (13C-1): append-only pack files of
//! compressed payloads and one index, in a directory of their own. Each
//! payload is a tile's samples or a fixed chunk of a larger payload, keyed by
//! its SHA-256 ([`key`]), so a key already in the pool appends nothing and two
//! states that share a tile store it once. A skeleton (`pool_skeleton`) names
//! a native file's payloads by key; this module names no document type.
//!
//! The key and the codec are `tile_pool_codec`'s (re-exported here): the
//! key is a SHA-256 over the sample type, the size and the samples; a
//! payload is stored raw, deflated or shuffle-deflated, whichever is smallest.
//!
//! **Files.** `pack-<n>` (an 8-byte header, then payloads; at most
//! [`PACK_LIMIT`] bytes, a new pack when the next payload would pass it) and
//! `pool.index` (an 8-byte header, then 49-byte entries: key 32, pack u32,
//! offset u64, stored length u32, codec u8). A later entry for a key wins.
//!
//! **Order and crashes.** An append writes the payload to its pack and keeps
//! its entry pending; [`Pool::flush`] barrier-flushes the packs written, only
//! then appends the pending entries to the index and barrier-flushes it, and
//! flushes the directory when a file was created (`durable_file`, the core's
//! one flush policy). So a durable index entry always names durable payload
//! bytes, and a caller that publishes a skeleton after `flush` never names a
//! key whose entry can be lost. [`Pool::open`] makes the directory with ONE
//! non-recursive `mkdir` (a deleted parent stays deleted), truncates a torn
//! index tail to its last whole entry, and drops an entry whose pack is
//! missing or ends before it (rewriting the index without it, so a later
//! append cannot grow the pack back under it); an append always goes at its
//! pack's file length, so a torn payload is only unreferenced bytes.
//!
//! **Damage.** [`Pool::fetch`] re-derives the key from the decoded bytes. A
//! payload that is missing, truncated, undecodable or whose key differs is an
//! `Err` naming it, and its entry is dropped from this handle, so the next
//! append of the same content writes it again (its new entry then wins).
//!
//! **Collection.** [`Pool::collect`] keeps only the live keys: it rewrites
//! the index (through `save_atomically`), copies the live payloads of every
//! pack whose live bytes are under half its payload bytes into another pack,
//! flushes, rewrites the index again, and only then removes those packs. A
//! crash at any point leaves unreferenced bytes, never a dangling key.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::tile_pool_codec::{decode, encode, DEFLATE, RAW, SHUFFLE};
pub use crate::tile_pool_codec::{
    key, key_text, Key, PoolSample, BLOB_CHUNK_BYTES, TAG_BLOB, TAG_CMYKA16, TAG_GRAY16, TAG_GRAY8,
    TAG_RGBA16, TAG_RGBA32F, TAG_RGBA8,
};

/// A pack's cap: large enough that a pool of a few documents is a handful of
/// files, small enough that a compaction copy stays bounded.
pub const PACK_LIMIT: u64 = 256 << 20;

const INDEX: &str = "pool.index";
const INDEX_MAGIC: &[u8; 4] = b"DRTI";
const PACK_MAGIC: &[u8; 4] = b"DRTK";
const VERSION: u32 = 1;
const HEADER: u64 = 8;
const ENTRY: usize = 49;

#[derive(Clone, Copy)]
struct Entry {
    pack: u32,
    offset: u64,
    stored: u32,
    codec: u8,
}

/// What the appends since [`Pool::reset_stats`] stored.
#[derive(Clone, Copy, Debug, Default)]
pub struct AppendStats {
    pub tiles: u64,
    pub chunks: u64,
    pub bytes: u64,
    pub raw: u64,
    pub deflate: u64,
    pub shuffle: u64,
}

/// The pool's size on disk and in its index.
#[derive(Clone, Copy, Debug, Default)]
pub struct PoolInfo {
    pub tiles: u64,
    pub packs: u64,
    pub stored_bytes: u64,
    pub live_bytes: u64,
}

/// An open pool directory (module doc).
pub struct Pool {
    dir: PathBuf,
    entries: HashMap<Key, Entry>,
    /// Each pack's length as this handle wrote or read it.
    packs: BTreeMap<u32, u64>,
    writer: Option<(u32, File)>,
    /// Packs never appended to again: a bad header, or a compaction source.
    sealed: BTreeSet<u32>,
    pending: Vec<(Key, Entry)>,
    written: BTreeSet<u32>,
    created: bool,
    made_dir: bool,
    readers: HashMap<u32, File>,
    stats: AppendStats,
}

fn io(what: &str, path: &Path, e: std::io::Error) -> String {
    format!("failed to {what} {}: {e}", path.display())
}

impl Pool {
    /// Opens (or makes, with one non-recursive `mkdir`) the pool at `dir`.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let made_dir = match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => true,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => false,
            Err(e) => return Err(io("create tile pool", dir, e)),
        };
        let meta = std::fs::symlink_metadata(dir).map_err(|e| io("read tile pool", dir, e))?;
        if !meta.is_dir() {
            return Err(format!("tile pool {} is not a directory", dir.display()));
        }
        let mut pool = Self {
            dir: dir.to_path_buf(),
            entries: HashMap::new(),
            packs: BTreeMap::new(),
            writer: None,
            sealed: BTreeSet::new(),
            pending: Vec::new(),
            written: BTreeSet::new(),
            created: false,
            made_dir,
            readers: HashMap::new(),
            stats: AppendStats::default(),
        };
        for item in std::fs::read_dir(dir).map_err(|e| io("read tile pool", dir, e))? {
            let item = item.map_err(|e| io("read tile pool", dir, e))?;
            let Some(number) = item
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix("pack-"))
                .and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };
            let path = item.path();
            let meta = std::fs::symlink_metadata(&path).map_err(|e| io("read", &path, e))?;
            if !meta.is_file() {
                continue;
            }
            let mut head = [0_u8; 8];
            let good = File::open(&path).and_then(|f| f.read_exact_at(&mut head, 0));
            if good.is_err() || head != header(PACK_MAGIC) {
                pool.sealed.insert(number);
            }
            pool.packs.insert(number, meta.len());
        }
        pool.read_index()?;
        Ok(pool)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join(INDEX)
    }

    fn pack_path(&self, number: u32) -> PathBuf {
        self.dir.join(format!("pack-{number:08}"))
    }

    fn read_index(&mut self) -> Result<(), String> {
        let path = self.index_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io("read", &path, e)),
        };
        if bytes.len() >= HEADER as usize && bytes[..8] != header(INDEX_MAGIC) {
            return Err(format!("tile pool index {} is damaged", path.display()));
        }
        // A torn header or entry: cut back to the last whole entry, so the
        // next flush appends on an entry boundary.
        let whole = if bytes.len() < HEADER as usize {
            0
        } else {
            HEADER as usize + (bytes.len() - HEADER as usize) / ENTRY * ENTRY
        };
        if whole < bytes.len() {
            let file = OpenOptions::new()
                .write(true)
                .open(&path)
                .map_err(|e| io("repair", &path, e))?;
            file.set_len(whole as u64)
                .and_then(|()| crate::durable_file::order_file(&file))
                .map_err(|e| io("repair", &path, e))?;
        }
        let mut dropped = false;
        for raw in bytes[HEADER.min(whole as u64) as usize..whole].chunks_exact(ENTRY) {
            let mut key = [0_u8; 32];
            key.copy_from_slice(&raw[..32]);
            let entry = Entry {
                pack: u32::from_le_bytes([raw[32], raw[33], raw[34], raw[35]]),
                offset: u64::from_le_bytes(std::array::from_fn(|i| raw[36 + i])),
                stored: u32::from_le_bytes([raw[44], raw[45], raw[46], raw[47]]),
                codec: raw[48],
            };
            let fits = self.packs.get(&entry.pack).is_some_and(|&length| {
                entry.offset >= HEADER && entry.offset + u64::from(entry.stored) <= length
            });
            if fits && entry.codec <= SHUFFLE {
                self.entries.insert(key, entry);
            } else {
                dropped = true;
            }
        }
        // A dropped entry is rewritten away now: a later append could
        // otherwise grow its pack under it and bring it back.
        if dropped {
            self.rewrite_index()?;
        }
        Ok(())
    }

    /// Whether `key` has a payload in this handle's index.
    pub fn contains(&self, key: &Key) -> bool {
        self.entries.contains_key(key)
    }

    /// Stores `bytes` under `key` unless it is already there; `true` when a
    /// payload was appended. `element` is the shuffle's component size.
    pub fn append(
        &mut self,
        key: &Key,
        tag: u8,
        bytes: &[u8],
        element: usize,
    ) -> Result<bool, String> {
        if self.entries.contains_key(key) {
            return Ok(false);
        }
        let (codec, stored) = encode(bytes, element);
        self.put_stored(key, codec, &stored)?;
        let stats = &mut self.stats;
        if tag == TAG_BLOB {
            stats.chunks += 1;
        } else {
            stats.tiles += 1;
        }
        stats.bytes += stored.len() as u64;
        match codec {
            RAW => stats.raw += 1,
            DEFLATE => stats.deflate += 1,
            _ => stats.shuffle += 1,
        }
        Ok(true)
    }

    fn put_stored(&mut self, key: &Key, codec: u8, stored: &[u8]) -> Result<(), String> {
        let length = stored.len() as u64;
        let number = self.writer_for(length)?;
        let offset = self.packs.get(&number).copied().unwrap_or(HEADER);
        let path = self.pack_path(number);
        let Some((_, file)) = self.writer.as_mut() else {
            return Err(format!("tile pool pack {} is not open", path.display()));
        };
        if let Err(e) = file.write_all(stored) {
            // The pack's length is now unknown: the next append reopens it
            // and reads it again.
            self.writer = None;
            return Err(io("write", &path, e));
        }
        self.packs.insert(number, offset + length);
        let entry = Entry {
            pack: number,
            offset,
            stored: length as u32,
            codec,
        };
        self.entries.insert(*key, entry);
        self.pending.push((*key, entry));
        self.written.insert(number);
        Ok(())
    }

    /// The pack the next `length` bytes go to: the open one while it has
    /// room, else the highest pack with room (reopened at its file length),
    /// else a new one.
    fn writer_for(&mut self, length: u64) -> Result<u32, String> {
        let room = |pool: &Self, number: u32| {
            !pool.sealed.contains(&number)
                && pool
                    .packs
                    .get(&number)
                    .is_some_and(|&l| l + length <= PACK_LIMIT)
        };
        if let Some((number, _)) = &self.writer {
            let number = *number;
            if room(self, number) {
                return Ok(number);
            }
            self.writer = None;
        }
        if let Some(&number) = self.packs.keys().next_back() {
            if room(self, number) {
                let path = self.pack_path(number);
                let file = OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .map_err(|e| io("open", &path, e))?;
                let length = file.metadata().map_err(|e| io("read", &path, e))?.len();
                self.packs.insert(number, length);
                if room(self, number) {
                    self.writer = Some((number, file));
                    return Ok(number);
                }
            }
        }
        let number = self.packs.keys().next_back().map_or(1, |n| n + 1);
        let path = self.pack_path(number);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| io("create", &path, e))?;
        file.write_all(&header(PACK_MAGIC))
            .map_err(|e| io("write", &path, e))?;
        self.packs.insert(number, HEADER);
        self.written.insert(number);
        self.created = true;
        self.writer = Some((number, file));
        Ok(number)
    }

    /// Makes every append so far durable, payloads before their entries
    /// (module doc).
    pub fn flush(&mut self) -> Result<(), String> {
        self.flush_packs()?;
        if !self.pending.is_empty() {
            let path = self.index_path();
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| io("open", &path, e))?;
            let length = file.metadata().map_err(|e| io("read", &path, e))?.len();
            let mut bytes = Vec::with_capacity(HEADER as usize + self.pending.len() * ENTRY);
            if length < HEADER {
                // A new index, or one whose header a crash tore.
                file.set_len(0).map_err(|e| io("write", &path, e))?;
                bytes.extend_from_slice(&header(INDEX_MAGIC));
                self.created = true;
            } else if !(length - HEADER).is_multiple_of(ENTRY as u64) {
                // A write this handle failed part way: back to an entry boundary.
                let whole = HEADER + (length - HEADER) / ENTRY as u64 * ENTRY as u64;
                file.set_len(whole).map_err(|e| io("write", &path, e))?;
            }
            for (key, entry) in &self.pending {
                put_entry(&mut bytes, key, entry);
            }
            file.write_all(&bytes)
                .and_then(|()| crate::durable_file::order_file(&file))
                .map_err(|e| io("write", &path, e))?;
            self.pending.clear();
        }
        self.persist_dir();
        Ok(())
    }

    fn flush_packs(&mut self) -> Result<(), String> {
        for &number in &self.written {
            let path = self.pack_path(number);
            File::open(&path)
                .and_then(|file| crate::durable_file::order_file(&file))
                .map_err(|e| io("flush", &path, e))?;
        }
        self.written.clear();
        Ok(())
    }

    fn persist_dir(&mut self) {
        if self.created {
            crate::durable_file::persist_parent_dir(&self.index_path().to_string_lossy());
            self.created = false;
        }
        if self.made_dir {
            crate::durable_file::persist_parent_dir(&self.dir.to_string_lossy());
            self.made_dir = false;
        }
    }

    /// The decoded payload of `key`, checked against the key (module doc).
    pub fn fetch(
        &mut self,
        key: &Key,
        tag: u8,
        (width, height): (u32, u32),
        element: usize,
        expected: usize,
    ) -> Result<Vec<u8>, String> {
        let name = key_text(key);
        let Some(entry) = self.entries.get(key).copied() else {
            return Err(format!("tile pool payload {name} is missing"));
        };
        let result = self.read_stored(entry, &name).and_then(|stored| {
            decode(entry.codec, &stored, element, expected)
                .ok()
                .filter(|bytes| crate::tile_pool::key(tag, width, height, bytes) == *key)
                .ok_or_else(|| format!("tile pool payload {name} failed its hash check"))
        });
        if result.is_err() {
            self.entries.remove(key);
        }
        result
    }

    fn read_stored(&mut self, entry: Entry, name: &str) -> Result<Vec<u8>, String> {
        let path = self.pack_path(entry.pack);
        let file = match self.readers.entry(entry.pack) {
            std::collections::hash_map::Entry::Occupied(slot) => slot.into_mut(),
            std::collections::hash_map::Entry::Vacant(slot) => slot.insert(
                File::open(&path).map_err(|_| format!("tile pool payload {name} is missing"))?,
            ),
        };
        let mut stored = vec![0_u8; entry.stored as usize];
        match file.read_exact_at(&mut stored, entry.offset) {
            Ok(()) => Ok(stored),
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                Err(format!("tile pool payload {name} is truncated"))
            }
            Err(e) => Err(io("read", &path, e)),
        }
    }

    /// Keeps only `live` keys and compacts sparse packs (module doc).
    pub fn collect(&mut self, live: &HashSet<Key>) -> Result<(), String> {
        self.flush()?;
        self.entries.retain(|key, _| live.contains(key));
        self.rewrite_index()?;
        let mut used: BTreeMap<u32, u64> = BTreeMap::new();
        for entry in self.entries.values() {
            *used.entry(entry.pack).or_default() += u64::from(entry.stored);
        }
        let doomed: Vec<u32> = self
            .packs
            .iter()
            .filter(|(number, &length)| {
                used.get(number).copied().unwrap_or(0) * 2 < length.saturating_sub(HEADER)
                    || (self.sealed.contains(number) && !used.contains_key(number))
            })
            .map(|(number, _)| *number)
            .collect();
        if doomed.is_empty() {
            return Ok(());
        }
        self.sealed.extend(doomed.iter().copied());
        let moving: Vec<(Key, Entry)> = self
            .entries
            .iter()
            .filter(|(_, entry)| doomed.contains(&entry.pack))
            .map(|(key, entry)| (*key, *entry))
            .collect();
        for (key, entry) in moving {
            let stored = self.read_stored(entry, &key_text(&key))?;
            self.put_stored(&key, entry.codec, &stored)?;
        }
        self.flush_packs()?;
        self.pending.clear();
        self.rewrite_index()?;
        for number in doomed {
            let path = self.pack_path(number);
            self.readers.remove(&number);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(io("remove", &path, e)),
            }
            self.packs.remove(&number);
            self.sealed.remove(&number);
        }
        self.created = true;
        self.persist_dir();
        Ok(())
    }

    /// The whole index, written beside and renamed over the old one.
    fn rewrite_index(&mut self) -> Result<(), String> {
        let mut bytes = Vec::with_capacity(HEADER as usize + self.entries.len() * ENTRY);
        bytes.extend_from_slice(&header(INDEX_MAGIC));
        for (key, entry) in &self.entries {
            put_entry(&mut bytes, key, entry);
        }
        let path = self.index_path();
        let shown = path.to_string_lossy().into_owned();
        crate::dr_image::save_atomically(&shown, |temporary| {
            std::fs::write(temporary, &bytes).map_err(|e| format!("failed to write {shown}: {e}"))
        })
    }

    pub fn info(&self) -> PoolInfo {
        PoolInfo {
            tiles: self.entries.len() as u64,
            packs: self.packs.len() as u64,
            stored_bytes: self.packs.values().sum(),
            live_bytes: self.entries.values().map(|e| u64::from(e.stored)).sum(),
        }
    }

    pub fn reset_stats(&mut self) {
        self.stats = AppendStats::default();
    }

    pub fn stats(&self) -> AppendStats {
        self.stats
    }
}

fn header(magic: &[u8; 4]) -> [u8; 8] {
    let mut out = [0_u8; 8];
    out[..4].copy_from_slice(magic);
    out[4..].copy_from_slice(&VERSION.to_le_bytes());
    out
}

fn put_entry(bytes: &mut Vec<u8>, key: &Key, entry: &Entry) {
    bytes.extend_from_slice(key);
    bytes.extend_from_slice(&entry.pack.to_le_bytes());
    bytes.extend_from_slice(&entry.offset.to_le_bytes());
    bytes.extend_from_slice(&entry.stored.to_le_bytes());
    bytes.push(entry.codec);
}
