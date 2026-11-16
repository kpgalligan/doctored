//! The process-wide RESIDENT-CACHE REGISTRY (13B-6): one byte bound over every
//! disposable cache payload that holds planes or tables, shared by every
//! document and job in the process: a style's rendered planes (the byte
//! entries of `style_cache` and the typed ones of `precision_style_cache`), a
//! byte mask's derived (feathered) and aligned planes (`doc_mask`), and the
//! LUT memo's parsed tables (`adjust_lut`).
//!
//! **Ownership.** The registry holds the one strong reference to each payload;
//! the cache that made it holds a [`Ticket`] (a `Weak` of the payload and a
//! shared token). A hit upgrades the `Weak` ([`Ticket::get`], which also marks
//! the entry most recently used); a payload the registry evicted no longer
//! upgrades, and its cache treats the entry as absent and builds it again.
//! A reader keeps the `Arc` it upgraded for as long as it uses it, so an
//! eviction never pulls planes from under a render. When the last clone of a
//! ticket drops (its cache truncated, cleared or freed with its document),
//! the entry leaves the registry and its bytes are free at once.
//!
//! **Bound and order.** `cache_limit::registry_bound` of the process working
//! limit (the working limit / 8), read at every insert. An insert that takes
//! the total past the bound evicts the least recently used entries, of any
//! class, until the total fits; the clock is one counter, so no two entries
//! tie. A payload larger than the bound on its own is not retained (its
//! ticket upgrades only while its maker still holds the `Arc`) and evicts
//! nothing. A style keeps its own `CACHE_CAPACITY` entries as its count bound.
//!
//! **Bytes.** An entry's bytes are what the resource report counts for its
//! payload: the owner's own accounting walk over the one payload into a
//! scratch `Inventory`, so the bound and the report never use two formulas.
//! An allocation two payloads share (a pattern level, a Shape Burst field)
//! counts in both entries, so the bound may over-count, never under-count.
//!
//! **Locks.** One mutex, and it is a LEAF: the registry never calls owner
//! code and no payload holds a ticket. An owner may upgrade or drop a ticket
//! under its own lock (owner → registry, one direction); `retain` is called
//! outside owner locks, and every payload the registry lets go of is dropped
//! after its mutex is released.
//!
//! Not members: the content-box memos (entry-bounded and holding no pixels)
//! and the contiguous views (their own byte bound, `contiguous_views`); the
//! report section (`resources::mixed_report_with_masks`, `cache_registry`)
//! lists both beside the registry's classes.

use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use serde_json::{json, Map, Value};

/// What a registered payload is, for the report's per-class figures.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CacheClass {
    /// A byte style entry's `RenderedEffects` (`style_cache`).
    StylePlanes,
    /// A typed style entry's `RenderedEffects` (`precision_style_cache`).
    TypedStylePlanes,
    /// A feathered byte mask's padded plane (`MaskProperties::derived_with_job`).
    MaskDerived,
    /// A byte mask's plane aligned to its layer (`Layer::aligned_mask_with_job`).
    MaskAligned,
    /// A LUT memo slot's meta and parsed adjustment (`adjust_lut::memoized`).
    Lut,
}

impl CacheClass {
    const ALL: [Self; 5] = [
        Self::StylePlanes,
        Self::TypedStylePlanes,
        Self::MaskDerived,
        Self::MaskAligned,
        Self::Lut,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::StylePlanes => "style_planes",
            Self::TypedStylePlanes => "typed_style_planes",
            Self::MaskDerived => "mask_derived",
            Self::MaskAligned => "mask_aligned",
            Self::Lut => "lut",
        }
    }
}

type Payload = Arc<dyn Any + Send + Sync>;

struct Entry {
    id: u64,
    class: CacheClass,
    bytes: u64,
    used: Arc<AtomicU64>,
    payload: Payload,
}

struct Registry {
    entries: Vec<Entry>,
    bytes: u64,
    evictions: u64,
    next_id: u64,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    entries: Vec::new(),
    bytes: 0,
    evictions: 0,
    next_id: 0,
});

/// The one use clock: every insert and every hit takes the next tick.
static CLOCK: AtomicU64 = AtomicU64::new(1);

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

fn tick() -> u64 {
    CLOCK.fetch_add(1, Ordering::Relaxed)
}

/// The bound every insert holds: the working limit / 8 (`cache_limit`).
pub(crate) fn bound() -> u64 {
    crate::cache_limit::registry_bound(crate::precision_image::process_working_limit())
}

impl Registry {
    /// Removes entry `id` if it is still registered (it may have been
    /// evicted); the payload is handed back to be dropped outside the lock.
    fn remove(&mut self, id: u64) -> Option<Payload> {
        let at = self.entries.iter().position(|e| e.id == id)?;
        let entry = self.entries.swap_remove(at);
        self.bytes -= entry.bytes;
        Some(entry.payload)
    }
}

/// The shared part of a ticket: dropping the last clone unregisters it.
struct Token {
    id: u64,
    used: Arc<AtomicU64>,
}

impl Drop for Token {
    fn drop(&mut self) {
        let released = registry().remove(self.id);
        drop(released);
    }
}

/// An owner's handle on a registered payload (module doc, "Ownership").
pub(crate) struct Ticket<T> {
    token: Arc<Token>,
    payload: Weak<T>,
}

impl<T> Clone for Ticket<T> {
    fn clone(&self) -> Self {
        Self {
            token: Arc::clone(&self.token),
            payload: Weak::clone(&self.payload),
        }
    }
}

impl<T> Ticket<T> {
    /// The payload, marked most recently used; `None` once evicted.
    pub(crate) fn get(&self) -> Option<Arc<T>> {
        let payload = self.payload.upgrade()?;
        self.token.used.store(tick(), Ordering::Relaxed);
        Some(payload)
    }

    /// The payload without touching its use (accounting, candidate scans).
    pub(crate) fn peek(&self) -> Option<Arc<T>> {
        self.payload.upgrade()
    }

    /// Whether the payload is still held (by the registry or a reader).
    pub(crate) fn is_live(&self) -> bool {
        self.payload.strong_count() > 0
    }

    /// Whether both tickets name one registry entry.
    pub(crate) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.token, &other.token)
    }
}

/// Registers `payload` as `bytes` of class `class` and answers the owner's
/// ticket, evicting least recently used entries until the total fits the
/// bound (module doc). Call it outside the owner's lock.
pub(crate) fn retain<T: Any + Send + Sync>(
    class: CacheClass,
    payload: &Arc<T>,
    bytes: u64,
) -> Ticket<T> {
    let bound = bound();
    let used = Arc::new(AtomicU64::new(tick()));
    let mut evicted = Vec::new();
    let id = {
        let mut registry = registry();
        registry.next_id += 1;
        let id = registry.next_id;
        if bytes <= bound {
            let shared: Payload = Arc::clone(payload) as Payload;
            registry.entries.push(Entry {
                id,
                class,
                bytes,
                used: Arc::clone(&used),
                payload: shared,
            });
            registry.bytes += bytes;
            // The new entry holds the newest tick, so it goes last, and it
            // fits on its own: the loop ends.
            while registry.bytes > bound {
                let Some(oldest) = (0..registry.entries.len())
                    .min_by_key(|&i| registry.entries[i].used.load(Ordering::Relaxed))
                else {
                    break;
                };
                let entry = registry.entries.swap_remove(oldest);
                registry.bytes -= entry.bytes;
                registry.evictions += 1;
                evicted.push(entry.payload);
            }
        }
        id
    };
    drop(evicted);
    Ticket {
        token: Arc::new(Token { id, used }),
        payload: Arc::downgrade(payload),
    }
}

/// The registry's figures: the bound, the total bytes and entries, the
/// evictions since the process started, and bytes and entries per class.
pub(crate) fn report() -> Value {
    let bound = bound();
    let registry = registry();
    let mut classes = Map::new();
    for class in CacheClass::ALL {
        let (bytes, entries) = registry
            .entries
            .iter()
            .filter(|e| e.class == class)
            .fold((0_u64, 0_u64), |(b, n), e| (b + e.bytes, n + 1));
        classes.insert(
            class.name().into(),
            json!({"bytes": bytes, "entries": entries}),
        );
    }
    json!({
        "bound_bytes": bound,
        "bytes": registry.bytes,
        "entries": registry.entries.len(),
        "evictions": registry.evictions,
        "classes": classes,
    })
}
