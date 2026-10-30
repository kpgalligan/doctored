//! The ONE process-wide bounded cache of disposable CONTIGUOUS VIEWS of tile
//! planes (13B-2b's cache, moved out of `layer_raster` and shared with the
//! typed `PrecisionImage` since 13B-3b-1).
//!
//! A view is a whole-plane row-major copy of a `tile_store::TilePlane`: an
//! `RgbaImage` for a byte layer raster, an `Arc<Vec<[u16; 4]>>` for a u16
//! typed image (no f32 view since 13B-3e-4c-2a: no f32 constructor seeds and
//! nothing reads one). Every entry is keyed on the tile plane's INDEX
//! allocation (`TilePlane::weak_index`) — never on an identity token, so
//! holding a view does not move `LayerRaster::weak_count` — with at most one
//! live entry per plane; a `PlaneKey`'s variant is part of the key, so a byte
//! and a typed plane never alias. The bounds are shared by both
//! view types: `CONTIGUOUS_CACHE_BYTES` (one projection's working set, a
//! 3-layer 48 MP or 10-layer 24 MP document) and `CONTIGUOUS_CACHE_ENTRIES`,
//! least recently used evicted first; a view above the byte bound on its own
//! is never retained.
//!
//! Writers SEED the cache with the contiguous buffer they already hold
//! (`LayerRaster::new` / `rebuilt`, the seeded u16 constructor's `tile()`),
//! so the first read after a whole-image write is free. The lock never covers a
//! materialization or a drop of a view: a miss builds the view outside it and
//! re-checks before inserting, so concurrent readers of one plane end with one
//! shared view, and removed entries are dropped after the lock is released.
//!
//! The views are DISPOSABLE: a resource report lists the views of the
//! supplied handles' planes as `contiguous_views` cache bytes (top-level
//! `contiguous_view_bytes`) and `dr_mixed_resources_evict_caches` drops every view,
//! byte and typed alike.
//!
//! the ONE process-wide bounded cache of disposable whole-plane contiguous views (13B-2b's cache,
//! moved out of `layer_raster` and generalized in 13B-3b-1): `PlaneKey::{Rgba8, Rgba16}` (a `Weak`
//! of the tile plane's index — never an identity token; the variant is part of the key),
//! `View::{Rgba8(Arc<RgbaImage>), Rgba16(Arc<Vec<[u16;4]>>)}` (bytes capacity-based; the f32
//! variants removed in 13B-3e-4c-2a, when no f32 constructor seeds), `CONTIGUOUS_CACHE_BYTES` (1
//! GiB) / `CONTIGUOUS_CACHE_ENTRIES` (256) shared by both depths, LRU, one live entry per plane;
//! `seed` (writers hand over the buffer they hold), `view` (hit, else materialize OUTSIDE the lock,
//! re-check, insert; removed entries dropped unlocked), `contiguous_view_bytes` (test oracle), and
//! the report / eviction trio `contiguous_views_of` / `record_contiguous_views` /
//! `evict_contiguous_views` that `resources.rs` calls. Since 13B-3e-4a a typed FFI pin is a private
//! copy, never a cache view, so no pinned view merges here any more (`Inventory::record`'s merge
//! rule still applies to any view an authoritative owner also holds). Byte callers go through
//! `layer_raster`'s `seed` / `view` wrappers, typed ones through `precision_image`.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use image::RgbaImage;

use crate::resources::Inventory;
use crate::tile_store::Tile;

/// Bytes of disposable contiguous views retained process-wide (see the
/// module doc for why one projection's working set).
pub const CONTIGUOUS_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

/// Views retained process-wide, whatever their size.
pub const CONTIGUOUS_CACHE_ENTRIES: usize = 256;

/// A non-owning handle onto a tile plane's index allocation, tagged with the
/// plane's sample type.
#[derive(Clone)]
pub(crate) enum PlaneKey {
    Rgba8(Weak<[Arc<Tile<[u8; 4]>>]>),
    Rgba16(Weak<[Arc<Tile<[u16; 4]>>]>),
}

impl PlaneKey {
    /// The index allocation's address: what `TilePlane::record_resources`
    /// records as `tile_structure`.
    fn addr(&self) -> usize {
        match self {
            Self::Rgba8(w) => w.as_ptr().cast::<()>() as usize,
            Self::Rgba16(w) => w.as_ptr().cast::<()>() as usize,
        }
    }

    /// The plane is still alive.
    fn alive(&self) -> bool {
        match self {
            Self::Rgba8(w) => w.strong_count() > 0,
            Self::Rgba16(w) => w.strong_count() > 0,
        }
    }

    /// The same plane: the same variant and the same index allocation.
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Rgba8(a), Self::Rgba8(b)) => Weak::ptr_eq(a, b),
            (Self::Rgba16(a), Self::Rgba16(b)) => Weak::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// One cached whole-plane view, shared by value.
#[derive(Clone)]
pub(crate) enum View {
    Rgba8(Arc<RgbaImage>),
    Rgba16(Arc<Vec<[u16; 4]>>),
}

impl View {
    /// The view's allocation capacity in bytes (what the byte bound counts).
    fn bytes(&self) -> u64 {
        match self {
            Self::Rgba8(image) => image.as_raw().capacity() as u64,
            Self::Rgba16(samples) => samples.capacity() as u64 * 8,
        }
    }

    /// Records the view as one `contiguous_views` cache allocation.
    fn record(&self, inventory: &mut Inventory) {
        match self {
            Self::Rgba8(image) => inventory.vec(image.as_raw(), "contiguous_views", true),
            Self::Rgba16(samples) => inventory.vec(samples, "contiguous_views", true),
        }
    }

    /// Pins the view's allocation owner for the life of `inventory`.
    fn pin(self, inventory: &mut Inventory) {
        match self {
            Self::Rgba8(image) => inventory.pin(image),
            Self::Rgba16(samples) => inventory.pin(samples),
        }
    }

    /// The byte view; `None` for a typed one.
    pub(crate) fn into_rgba8(self) -> Option<Arc<RgbaImage>> {
        match self {
            Self::Rgba8(image) => Some(image),
            _ => None,
        }
    }

    /// The u16 view; `None` for any other.
    pub(crate) fn into_rgba16(self) -> Option<Arc<Vec<[u16; 4]>>> {
        match self {
            Self::Rgba16(samples) => Some(samples),
            _ => None,
        }
    }
}

struct ViewEntry {
    key: PlaneKey,
    view: View,
    bytes: u64,
    used: u64,
}

struct ContiguousCache {
    entries: Vec<ViewEntry>,
    clock: u64,
}

static VIEWS: Mutex<ContiguousCache> = Mutex::new(ContiguousCache {
    entries: Vec::new(),
    clock: 0,
});

fn views() -> MutexGuard<'static, ContiguousCache> {
    VIEWS.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ContiguousCache {
    /// The live entry for `key`, touched (most recently used).
    fn hit(&mut self, key: &PlaneKey) -> Option<View> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.key.same(key) && e.key.alive())?;
        self.clock += 1;
        entry.used = self.clock;
        Some(entry.view.clone())
    }

    /// Entries whose plane is gone. Returned so the caller drops the views
    /// after releasing the lock.
    fn prune(&mut self) -> Vec<ViewEntry> {
        self.entries.extract_if(.., |e| !e.key.alive()).collect()
    }

    /// Makes `view` the ONE entry for `key`, evicting least recently used
    /// entries until both bounds hold; a view above the byte bound on its own
    /// is not retained. Returns the removed entries (dropped unlocked).
    fn insert(&mut self, key: PlaneKey, view: View) -> Vec<ViewEntry> {
        let mut removed: Vec<ViewEntry> =
            self.entries.extract_if(.., |e| e.key.same(&key)).collect();
        removed.extend(self.prune());
        let bytes = view.bytes();
        if bytes > CONTIGUOUS_CACHE_BYTES {
            return removed;
        }
        let mut total: u64 = self.entries.iter().map(|e| e.bytes).sum();
        while !self.entries.is_empty()
            && (self.entries.len() >= CONTIGUOUS_CACHE_ENTRIES
                || total + bytes > CONTIGUOUS_CACHE_BYTES)
        {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.used)
                .map(|(i, _)| i)
                .expect("a non-empty cache");
            let entry = self.entries.swap_remove(oldest);
            total -= entry.bytes;
            removed.push(entry);
        }
        self.clock += 1;
        self.entries.push(ViewEntry {
            key,
            view,
            bytes,
            used: self.clock,
        });
        removed
    }
}

/// Caches `view` as the plane's view (the writer already holds it).
pub(crate) fn seed(key: PlaneKey, view: View) {
    let removed = views().insert(key, view);
    drop(removed);
}

/// The plane's cached view; on a miss `materialize` builds it OUTSIDE the
/// lock, and the cache is re-checked before inserting.
pub(crate) fn view(key: PlaneKey, materialize: impl FnOnce() -> View) -> View {
    if let Some(hit) = views().hit(&key) {
        return hit;
    }
    let view = materialize();
    let mut cache = views();
    // Another reader may have materialized the same plane meanwhile: keep
    // one view per plane and hand out the one already cached.
    if let Some(hit) = cache.hit(&key) {
        drop(cache);
        return hit;
    }
    let removed = cache.insert(key, view.clone());
    drop(cache);
    drop(removed);
    view
}

/// Disposable view bytes currently cached for live planes. Test oracle.
pub fn contiguous_view_bytes() -> u64 {
    let mut cache = views();
    let removed = cache.prune();
    let bytes = cache.entries.iter().map(|e| e.bytes).sum();
    drop(cache);
    drop(removed);
    bytes
}

/// The cached views of every tile plane `walked` recorded (a plane's index
/// allocation is what `TilePlane::record_resources` records as
/// `tile_structure` and what the cache keys on), cloned under the lock so
/// each is pinned for as long as the caller holds it. Views of planes outside
/// the walked set belong to other handles' reports, as every other document
/// cache does.
pub(crate) fn contiguous_views_of(walked: &Inventory) -> Vec<View> {
    let mut cache = views();
    let removed = cache.prune();
    let found = cache
        .entries
        .iter()
        .filter(|entry| walked.has(entry.key.addr()))
        .map(|entry| entry.view.clone())
        .collect();
    drop(cache);
    drop(removed);
    found
}

/// Records `views` as `contiguous_views` cache allocations, pinning each view
/// so no eviction can free and reuse its address during the report. A view an
/// authoritative owner also holds stays retained under that owner's category
/// (`Inventory::record`'s merge rule).
pub(crate) fn record_contiguous_views(views: Vec<View>, inventory: &mut Inventory) {
    for view in views {
        view.record(inventory);
        view.pin(inventory);
    }
}

/// Drops every cached view, byte and typed (readers keep the Arcs they hold).
pub(crate) fn evict_contiguous_views() {
    let removed = std::mem::take(&mut views().entries);
    drop(removed);
}
