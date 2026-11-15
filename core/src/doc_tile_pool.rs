//! The tile pool's document adapters (13C-1): [`DrTilePool`], the handle
//! (`tile_pool::Pool` plus the identity memo), the byte and typed writes of a
//! document into a pool and a skeleton, the rebuild of a skeleton into the
//! native file, and collection. The pool, the codec and the key are
//! `tile_pool`'s; the skeleton format is `pool_skeleton`'s.
//!
//! **Write.** The native writers run unchanged into a [`SkeletonSink`]: every
//! byte they put goes to the skeleton as a literal, except a raster they offer
//! through `NativeSink::raster` (a tiled layer or master: its tiles are keyed
//! and appended, the skeleton records the keys and nothing is encoded) and a
//! payload they announce through `NativeSink::begin_blob` (a mask or channel
//! plane, a RAW payload, an 8-bit source PNG: keyed in 1 MiB chunks). A blob
//! under [`BLOB_MIN_BYTES`] stays literal: its key and index entry would cost
//! more than rewriting it. The sink cannot rewrite a byte it took, so the
//! 8-bit PNG encoder (`png_rows8`) measures a PNG it writes here before
//! writing it, and never stages it whole. The pool is flushed before the
//! skeleton's publishing rename, so a published skeleton never names a
//! payload that is not durable.
//!
//! **The identity memo.** Per handle and sample type, a tile's address maps
//! to `(Weak<Tile>, key)`, so a tile already keyed is not hashed again. An
//! entry answers only while its tile is alive: the `Weak` pins the tile's
//! allocation (its header, not its samples, which are a separate box), so
//! the address cannot be reused while the entry exists. Dead entries are
//! pruned at the start of each write. A memo hit still asks the pool for the
//! key, and appends the tile again when a collection removed it.
//!
//! **Rebuild.** The skeleton is verified whole first, then streamed into the
//! native file through `save_atomically`: literals copied, blob chunks
//! fetched (each hash-checked), and each tile segment fetched one raster at
//! a time and written by the writer's own function for its site, so the
//! rebuilt file is the saved file byte for byte. Its transient is one raster
//! beside the window and that site's encoder rows.
//!
//! **Purity.** A document is never changed; the handle's pool files and its
//! memo are, so the handle is a `Mutex` and every call takes it.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use crate::cancel_io::settle;
use crate::cmyk_image::CmykImage;
use crate::doc::DrDocument;
use crate::dr_image::save_atomically;
use crate::job_control::{DrJobControl, JobContext};
use crate::native_stream::{FileSink, NativeSink, RasterRef};
use crate::pool_skeleton::{
    Segment, Site, SkeletonReader, SkeletonWriter, KIND_BYTE, KIND_TYPED, LITERAL_MAX,
};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::tile_pool::{AppendStats, Key, Pool, PoolSample, BLOB_CHUNK_BYTES, TAG_BLOB};
use crate::tile_store::{Tile, TilePlane, TILE_EDGE};

/// The fixed transient a pool write or rebuild holds beside the document's
/// own write plan: one tile's little-endian bytes (at most 1 MiB, a 256²
/// RGBA32F tile), its shuffled copy (1 MiB), the two deflate outputs of a
/// payload that does not compress (about 1 MiB each), the deflate
/// compressor's tables (under 0.5 MiB), the skeleton sink's literal buffer
/// and blob chunk (1 MiB each) and the skeleton's 256 KiB write buffer: about
/// 7 MiB. A rebuild holds less beside its one raster: a stored payload and
/// its decoded bytes (1 MiB each) and the file sink's 1 MiB buffer. 8 MiB
/// rounds the larger up. A measured peak is 13C-6's.
pub const TILE_POOL_WINDOW_BYTES: u64 = 8 * 1024 * 1024;

/// A blob shorter than this stays literal (module doc).
const BLOB_MIN_BYTES: u64 = 64 * 1024;

type Memo<S> = HashMap<usize, (Weak<Tile<S>>, Key)>;

#[derive(Default)]
struct Memos {
    rgba8: Memo<[u8; 4]>,
    rgba16: Memo<[u16; 4]>,
    rgba32: Memo<[f32; 4]>,
    cmyk: Memo<[u16; 5]>,
}

impl Memos {
    fn prune(&mut self) {
        self.rgba8.retain(|_, (tile, _)| tile.strong_count() > 0);
        self.rgba16.retain(|_, (tile, _)| tile.strong_count() > 0);
        self.rgba32.retain(|_, (tile, _)| tile.strong_count() > 0);
        self.cmyk.retain(|_, (tile, _)| tile.strong_count() > 0);
    }
}

/// A raster sample type with a memo of its own.
trait Memoized: PoolSample {
    fn memo(memos: &mut Memos) -> &mut Memo<Self>;
}

impl Memoized for [u8; 4] {
    fn memo(memos: &mut Memos) -> &mut Memo<Self> {
        &mut memos.rgba8
    }
}
impl Memoized for [u16; 4] {
    fn memo(memos: &mut Memos) -> &mut Memo<Self> {
        &mut memos.rgba16
    }
}
impl Memoized for [f32; 4] {
    fn memo(memos: &mut Memos) -> &mut Memo<Self> {
        &mut memos.rgba32
    }
}
impl Memoized for [u16; 5] {
    fn memo(memos: &mut Memos) -> &mut Memo<Self> {
        &mut memos.cmyk
    }
}

/// What the last write did.
#[derive(Clone, Copy, Default)]
struct WriteReport {
    hashed_tiles: u64,
    hashed_chunks: u64,
    stats: AppendStats,
}

struct State {
    pool: Pool,
    memos: Memos,
    last: WriteReport,
    current: WriteReport,
}

impl State {
    fn begin_write(&mut self) {
        self.memos.prune();
        self.pool.reset_stats();
        self.current = WriteReport::default();
    }

    fn end_write(&mut self) {
        self.current.stats = self.pool.stats();
        self.last = self.current;
    }
}

/// An open tile pool and its memo (module doc).
pub struct DrTilePool {
    state: Mutex<State>,
}

impl DrTilePool {
    /// Opens the pool at `directory`, making it (one level) when absent.
    pub fn open(directory: &str) -> Result<Self, String> {
        Ok(Self {
            state: Mutex::new(State {
                pool: Pool::open(std::path::Path::new(directory))?,
                memos: Memos::default(),
                last: WriteReport::default(),
                current: WriteReport::default(),
            }),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, String> {
        self.state
            .lock()
            .map_err(|_| "tile pool is unusable after an internal error".to_string())
    }

    /// Writes the native file `skeleton` names back from the pool to
    /// `native`, atomically (module doc).
    pub fn rebuild(
        &self,
        skeleton: &str,
        native: &str,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let mut reader = SkeletonReader::open(skeleton)?;
        let mut state = self.lock()?;
        save_atomically(native, |temporary| {
            let mut out = FileSink::create(temporary, native)?;
            while let Some(segment) = reader.next_segment()? {
                job.check()?;
                match segment {
                    Segment::Literal(bytes) => out.put(&bytes)?,
                    Segment::Blob { len, keys } => {
                        let mut left = len;
                        for key in &keys {
                            let size = left.min(BLOB_CHUNK_BYTES as u64) as usize;
                            let bytes =
                                state.pool.fetch(key, TAG_BLOB, (size as u32, 0), 1, size)?;
                            out.put(&bytes)?;
                            left -= size as u64;
                        }
                    }
                    Segment::Tiles {
                        site,
                        width,
                        height,
                        keys,
                    } => rebuild_raster(
                        &mut state.pool,
                        &mut out,
                        site,
                        (width, height),
                        &keys,
                        job,
                    )?,
                }
            }
            out.finish()?;
            job.check()
        })?;
        if let Some(control) = control {
            control.report(1.0, 5);
        }
        Ok(())
    }

    /// Keeps only the payloads the listed skeletons name. Every skeleton is
    /// read first; one that cannot be read refuses and removes nothing.
    pub fn collect(&self, skeletons: &[String]) -> Result<(), String> {
        let mut live = HashSet::new();
        for path in skeletons {
            SkeletonReader::open(path)?.all_keys(&mut live)?;
        }
        self.lock()?.pool.collect(&live)
    }

    /// The pool's counts and the last write's, as JSON.
    pub fn info_json(&self) -> Result<String, String> {
        let state = self.lock()?;
        let info = state.pool.info();
        let last = state.last;
        Ok(serde_json::json!({
            "tiles": info.tiles,
            "packs": info.packs,
            "stored_bytes": info.stored_bytes,
            "live_bytes": info.live_bytes,
            "appended_tiles": last.stats.tiles,
            "appended_chunks": last.stats.chunks,
            "appended_bytes": last.stats.bytes,
            "hashed_tiles": last.hashed_tiles,
            "hashed_chunks": last.hashed_chunks,
            "raw_payloads": last.stats.raw,
            "deflate_payloads": last.stats.deflate,
            "shuffle_payloads": last.stats.shuffle,
        })
        .to_string())
    }
}

impl DrDocument {
    /// Writes the document into `pool` and its skeleton to `skeleton`
    /// (module doc): the byte writer (`drdc::write_native`) into the
    /// skeleton sink, as the typed twin does. The document is unchanged; the
    /// pool gains the payloads it did not hold. A cancel answers `operation
    /// cancelled` and publishes no skeleton.
    pub fn tile_pool_write(
        &self,
        pool: &DrTilePool,
        skeleton: &str,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.native_save_preflight()?;
        let mut state = pool.lock()?;
        state.begin_write();
        let result = save_atomically(skeleton, |temporary| {
            let mut sink = SkeletonSink::create(temporary, skeleton, KIND_BYTE, &mut state, job)?;
            self.write_native(&mut sink, job)?;
            sink.finish()?;
            state.pool.flush()?;
            job.check()
        });
        state.end_write();
        settle(job, result)?;
        if let Some(control) = control {
            control.report(1.0, 5);
        }
        Ok(())
    }
}

impl PrecisionDocument {
    /// The typed twin of [`DrDocument::tile_pool_write`]: the native save's
    /// admission (its write scratch plus [`TILE_POOL_WINDOW_BYTES`] against
    /// `limit` beside `other_retained`), then the typed writer into the pool.
    pub fn tile_pool_write(
        &self,
        pool: &DrTilePool,
        skeleton: &str,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let plan = self.admit_native_write(
            limit,
            other_retained.saturating_add(TILE_POOL_WINDOW_BYTES),
            job,
        )?;
        let mut state = pool.lock()?;
        state.begin_write();
        let result = save_atomically(skeleton, |temporary| {
            let mut sink = SkeletonSink::create(temporary, skeleton, KIND_TYPED, &mut state, job)?;
            self.write_native(&plan, &mut sink, job)?;
            sink.finish()?;
            state.pool.flush()?;
            job.check()
        });
        state.end_write();
        result?;
        if let Some(control) = control {
            control.report(1.0, 5);
        }
        Ok(())
    }
}

/// An open blob payload in the sink.
struct BlobScope {
    len: u64,
    left: u64,
    chunk: Vec<u8>,
    keys: Vec<Key>,
}

/// The native writers' sink during a pool write (module doc).
struct SkeletonSink<'a> {
    writer: SkeletonWriter,
    state: &'a mut State,
    literal: Vec<u8>,
    blob: Option<BlobScope>,
    position: u64,
    scratch: Vec<u8>,
    job: JobContext<'a>,
}

impl<'a> SkeletonSink<'a> {
    fn create(
        temporary: &str,
        path: &str,
        kind: u8,
        state: &'a mut State,
        job: JobContext<'a>,
    ) -> Result<Self, String> {
        Ok(Self {
            writer: SkeletonWriter::create(temporary, path, kind)?,
            state,
            literal: Vec::new(),
            blob: None,
            position: 0,
            scratch: Vec::new(),
            job,
        })
    }

    fn flush_literal(&mut self) -> Result<(), String> {
        if !self.literal.is_empty() {
            self.writer.literal(&self.literal)?;
            self.literal.clear();
        }
        Ok(())
    }

    /// Keys and stores the open blob's current chunk; writes the blob
    /// segment once its last byte arrived.
    fn close_chunk(&mut self) -> Result<(), String> {
        let Some(blob) = self.blob.as_mut() else {
            return Ok(());
        };
        let key = crate::tile_pool::key(TAG_BLOB, blob.chunk.len() as u32, 0, &blob.chunk);
        self.state.current.hashed_chunks += 1;
        self.state.pool.append(&key, TAG_BLOB, &blob.chunk, 1)?;
        blob.keys.push(key);
        blob.chunk.clear();
        if blob.left == 0 {
            if let Some(blob) = self.blob.take() {
                self.writer.blob(blob.len, &blob.keys)?;
            }
        }
        Ok(())
    }

    /// The keys of `plane`'s tiles in index order, appending the tiles the
    /// pool does not hold (module doc).
    fn keys<S: Memoized>(&mut self, plane: &TilePlane<S>) -> Result<Vec<Key>, String> {
        let mut keys = Vec::with_capacity((plane.columns() * plane.rows()) as usize);
        for ty in 0..plane.rows() {
            for tx in 0..plane.columns() {
                self.job.check()?;
                let tile = plane.tile(tx, ty).ok_or("tile pool write lost a tile")?;
                let address = Arc::as_ptr(tile) as usize;
                let known = S::memo(&mut self.state.memos)
                    .get(&address)
                    .filter(|(weak, _)| Weak::as_ptr(weak) == Arc::as_ptr(tile))
                    .map(|(_, key)| *key);
                let key = match known {
                    Some(key) => {
                        if !self.state.pool.contains(&key) {
                            self.scratch.clear();
                            S::put_le(tile.samples(), &mut self.scratch);
                            self.state
                                .pool
                                .append(&key, S::TAG, &self.scratch, S::ELEMENT)?;
                        }
                        key
                    }
                    None => {
                        self.scratch.clear();
                        S::put_le(tile.samples(), &mut self.scratch);
                        let key = crate::tile_pool::key(
                            S::TAG,
                            tile.width(),
                            tile.height(),
                            &self.scratch,
                        );
                        self.state.current.hashed_tiles += 1;
                        self.state
                            .pool
                            .append(&key, S::TAG, &self.scratch, S::ELEMENT)?;
                        S::memo(&mut self.state.memos).insert(address, (Arc::downgrade(tile), key));
                        key
                    }
                };
                keys.push(key);
            }
        }
        Ok(keys)
    }

    /// The literal tail and the end record; a blob left open is an error.
    fn finish(mut self) -> Result<(), String> {
        if self.blob.is_some() {
            return Err("tile pool write ended inside a payload".into());
        }
        self.flush_literal()?;
        self.writer.finish()
    }
}

impl NativeSink for SkeletonSink<'_> {
    fn position(&self) -> u64 {
        self.position
    }

    /// A literal or a chunk already keyed cannot be rewritten (module doc).
    fn rewrites(&self) -> bool {
        false
    }

    fn put(&mut self, mut bytes: &[u8]) -> Result<(), String> {
        self.position += bytes.len() as u64;
        while !bytes.is_empty() {
            if let Some(blob) = self.blob.as_mut() {
                let room = BLOB_CHUNK_BYTES - blob.chunk.len();
                let take = (blob.left.min(room as u64) as usize).min(bytes.len());
                blob.chunk.extend_from_slice(&bytes[..take]);
                blob.left -= take as u64;
                bytes = &bytes[take..];
                if blob.chunk.len() == BLOB_CHUNK_BYTES || blob.left == 0 {
                    self.close_chunk()?;
                }
            } else {
                let take = (LITERAL_MAX - self.literal.len()).min(bytes.len());
                self.literal.extend_from_slice(&bytes[..take]);
                bytes = &bytes[take..];
                if self.literal.len() == LITERAL_MAX {
                    self.flush_literal()?;
                }
            }
        }
        Ok(())
    }

    fn raster(&mut self, raster: RasterRef<'_>) -> Result<bool, String> {
        if self.blob.is_some() {
            return Ok(false);
        }
        self.flush_literal()?;
        let (site, dimensions, keys) = match raster {
            RasterRef::Rgba8Png(plane) => (Site::Rgba8Png, plane.dimensions(), self.keys(plane)?),
            RasterRef::Rgba16Png(plane) => (Site::Rgba16Png, plane.dimensions(), self.keys(plane)?),
            RasterRef::Float(plane) => (Site::Float, plane.dimensions(), self.keys(plane)?),
            RasterRef::Cmyk(plane) => (Site::Cmyk, plane.dimensions(), self.keys(plane)?),
        };
        self.writer.tiles(site, dimensions, &keys)?;
        Ok(true)
    }

    fn begin_blob(&mut self, len: u64) -> Result<(), String> {
        if len < BLOB_MIN_BYTES || self.blob.is_some() {
            return Ok(());
        }
        self.flush_literal()?;
        let mut chunk = Vec::new();
        chunk
            .try_reserve_exact(BLOB_CHUNK_BYTES)
            .map_err(|_| "could not allocate the tile pool's chunk")?;
        self.blob = Some(BlobScope {
            len,
            left: len,
            chunk,
            keys: Vec::new(),
        });
        Ok(())
    }
}

/// A tile's `(w, h)` at index `index` of a `width`-wide plane.
fn tile_size((width, height): (u32, u32), index: usize) -> (u32, u32, u32, u32) {
    let columns = width.div_ceil(TILE_EDGE) as usize;
    let (x, y) = (
        (index % columns) as u32 * TILE_EDGE,
        (index / columns) as u32 * TILE_EDGE,
    );
    (
        x,
        y,
        (width - x).min(TILE_EDGE),
        (height - y).min(TILE_EDGE),
    )
}

fn fetch_tile<S: PoolSample>(
    pool: &mut Pool,
    key: &Key,
    (w, h): (u32, u32),
) -> Result<Vec<u8>, String> {
    pool.fetch(key, S::TAG, (w, h), S::ELEMENT, (w * h) as usize * S::BYTES)
}

/// The plane a tile segment names, fetched tile by tile.
fn plane<S: PoolSample>(
    pool: &mut Pool,
    dimensions: (u32, u32),
    keys: &[Key],
    job: JobContext<'_>,
) -> Result<TilePlane<S>, String> {
    let mut tiles = Vec::with_capacity(keys.len());
    for (index, key) in keys.iter().enumerate() {
        job.check()?;
        let (_, _, w, h) = tile_size(dimensions, index);
        let bytes = fetch_tile::<S>(pool, key, (w, h))?;
        let samples: Box<[S]> = bytes.chunks_exact(S::BYTES).map(S::from_le).collect();
        tiles.push(Arc::new(Tile::new(w, h, samples)));
    }
    TilePlane::from_tiles(dimensions.0, dimensions.1, tiles)
}

/// One tile segment written back by its site's own encoder.
fn rebuild_raster(
    pool: &mut Pool,
    out: &mut FileSink,
    site: Site,
    dimensions: (u32, u32),
    keys: &[Key],
    job: JobContext<'_>,
) -> Result<(), String> {
    match site {
        Site::Rgba8Png => {
            let plane = plane::<[u8; 4]>(pool, dimensions, keys, job)?;
            let mut rows = crate::drdc::rgba_rows(|y| plane.row_segments(y, 0, u32::MAX));
            crate::drdc::put_layer_png(out, dimensions, &mut rows, job)
        }
        Site::Rgba16Png => {
            let image = PrecisionImage::from_tile_plane16(plane(pool, dimensions, keys, job)?);
            crate::precision_codec::put_native_png(out, &image, u64::MAX, job)
        }
        Site::Float => {
            let image = PrecisionImage::from_tile_plane32(plane(pool, dimensions, keys, job)?);
            crate::float_native::put_samples(out, &image, u64::MAX, job)
        }
        Site::Cmyk => {
            let image = CmykImage::from_tile_plane(plane(pool, dimensions, keys, job)?);
            crate::cmyk_native::put_payload(out, &image, u64::MAX, job)
        }
    }
}
