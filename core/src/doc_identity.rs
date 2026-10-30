//! Stable LAYER identity: the id a host hangs per-layer state on, and the
//! two lookups that turn one into an index and back.
//!
//! It is the third member of a family this crate already has twice —
//! `Channel::id` (`doc_channel`) and `Guide::id` (`doc_guide`) — and it
//! exists for the same reason both of those do, only more sharply: a layer's
//! INDEX is not a name. Every insert, delete, group, ungroup, merge, reorder
//! and duplicate renumbers the stack, so an index recorded at one moment
//! points at a different entry at the next. A NAME is not one either: two
//! layers may share one, and a rename breaks it.
//!
//! **PERSISTED in `.rz` since Phase 12E-1** (`identity_native`), unlike the
//! guide and channel ids beside it: the additive outermost wrapper — DRDC 45
//! at byte depth, 46 at typed depth — always writes a `LIDS` table, so a
//! reopen, a recovery and a snapshot all hand back the ids the document had.
//! Uniqueness is per DOCUMENT: two files may carry the same ids, and the
//! reader raises the process counter past every id it reads
//! ([`reserve_layer_ids_through`]) so a fresh mint can never collide with one.
//! Only a file with no 45/46 word — every file an earlier build wrote, and
//! every PSD import — MINTS fresh ids, exactly as every reader did before.
//!
//! **The minting rule, in one line: a NEW picture gets a new id; the SAME
//! picture carried through an op keeps its own.** So `Layer::new` mints (and
//! with it every insert, Via Copy, Stamp Visible and flatten that goes
//! through it), `duplicating_layer` mints per copied entry across the whole
//! subtree (a copy is a new object — the same rule `Channel::duplicated`
//! follows), a group created by `group_layers` mints for the container while
//! its members keep theirs, and a MERGE keeps the LOWEST merged entry's id
//! (the merged entry takes that member's slot, name, locks and link, so it is
//! that layer with new pixels). That merge rule has NO exception: Merge
//! Layers, Merge DOWN and Merge Visible all keep the lowest entry's id at
//! BOTH depths — `merging_down` and `precision_merge::merge_down` assign it
//! over the one `Layer::new` / `merged_leaf` mints, exactly as the other two
//! do. Everything else — a rename, an opacity
//! change, a reorder, a geometry op, a render-time materialization, an undo
//! handle — carries the id through untouched.
//!
//! 0 is never a live id, so it is free to mean "no layer" across the FFI.
//!
//! **A second, CONTENT-level identity sits beside it:**
//! [`DrDocument::layer_storage_identity`] answers the identity of an entry's
//! pixel STORAGE (`LayerRaster::storage_identity`), not of the layer. The
//! layer id survives a pixel edit; the storage identity does not — every
//! pixel write answers a new value, even one that leaves every byte equal —
//! while a clone, an undo handle and a name/opacity/visibility/offset write
//! keep it. It is what a host keys a per-layer render cache (the Layers
//! panel's thumbnails) on, compared only while both rasters are owned. A
//! GROUP answers `None`: its thumbnail is its projection, which this identity
//! does not cover.
//!
//! **And a third, for the MASK's storage:**
//! [`DrDocument::layer_mask_storage_identity`] /
//! [`PrecisionDocument::layer_mask_storage_identity`] answer the address of
//! the raw mask plane's shared allocation — the byte `Arc<GrayImage>`, the
//! typed `PrecisionPlane`'s `Arc<Vec<u16>>`. A rename, a clone, an undo
//! handle and a mask PROPERTY write (density, feather, link, enabled) carry
//! the same `Arc`; painting or replacing the mask allocates a new one. It
//! covers the RAW plane only — exactly what the panel's mask thumbnail
//! shows — and, like the pixel identity, is
//! comparable only while both planes are owned. `None` for an out-of-range
//! index, an entry with no mask and a GROUP (whose panel mask is a canvas
//! view the plane does not determine alone).
//!
//! `NEXT_LAYER_ID` (one counter for byte and typed documents alike), `mint_layer_id`, `Layer::id` /
//! `PrecisionDocumentLayer::id`, and the `layer_id` / `layer_index_by_id` pair on `DrDocument` and
//! `PrecisionDocument`. The third member of the `Channel::id` / `Guide::id` family, and since 12E-1
//! the one member that IS PERSISTED (`identity_native.rs`): a reader mints only for a file with no
//! 45/46 word, and `reserve_layer_ids_through` raises the counter past every id a file brings so a
//! later mint cannot collide. Two exports: `dr_doc_layer_id`, and `dr_doc_layer_storage_identity`
//! — a second, CONTENT-level identity (`DrDocument::layer_storage_identity` over
//! `LayerRaster::storage_identity`): equal nonzero values hold the same pixel storage (a clone, an
//! undo handle, a name/opacity/visibility/offset write keep it; every pixel write answers a new
//! value), 0 for NULL/out-of-range/GROUP, comparable only while both rasters are owned; the Layers
//! panel's thumbnail cache keys on it (typed twin: `dr_precision_image_storage_identity` on
//! `dr_precision_doc_layer_source`). A third, the raw MASK plane's storage identity
//! (`layer_mask_storage_identity` at both depths: the byte `Arc<GrayImage>` / typed
//! `PrecisionPlane` `Arc` address; a rename, clone, undo handle or density/feather/link/enabled
//! write keeps it, a mask paint or replacement answers a new value, 0 for no
//! mask/GROUP/out-of-range/NULL), exported at EACH depth (`dr_doc_layer_mask_storage_identity`,
//! `dr_precision_doc_layer_mask_storage_identity`) because a typed mask has no handle of its own;
//! the panel's mask thumbnail cache keys on it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::doc::{DrDocument, LayerKind};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};

/// The id counter behind `Layer::id` and `PrecisionDocumentLayer::id`.
/// Starts at 1, so 0 is free to mean "no layer". `Relaxed` is enough:
/// nothing is published through this counter, only made distinct, and a
/// `fetch_add` is atomic whatever the ordering (`doc_guide`'s own note).
///
/// ONE counter for BOTH depths, so an id can never mean two different layers
/// in one process — a `convert_precision` round trip mints from the same
/// sequence the byte document did.
static NEXT_LAYER_ID: AtomicU64 = AtomicU64::new(1);

/// A fresh layer identity, unique among every layer this process has minted.
pub(crate) fn mint_layer_id() -> u64 {
    NEXT_LAYER_ID.fetch_add(1, Ordering::Relaxed)
}

/// Raises the counter past `max`, so the next mint is above every id a file
/// just handed this process. Called by the `LIDS` readers at both depths
/// (`identity_native`): ids are unique per DOCUMENT, and this is what keeps a
/// layer created AFTER an open from colliding with one the file carried.
/// A lower `max` changes nothing, which is what `fetch_max` means.
pub(crate) fn reserve_layer_ids_through(max: u64) {
    NEXT_LAYER_ID.fetch_max(max.saturating_add(1), Ordering::Relaxed);
}

impl DrDocument {
    /// Entry `idx`'s stable identity; `None` for an out-of-range index.
    pub fn layer_id(&self, idx: usize) -> Option<u64> {
        Some(self.layers.get(idx)?.id)
    }

    /// The index the layer with this identity currently sits at; `None` when
    /// no entry carries it (it was deleted, merged away, or the id came from
    /// another session). 0 is never a live id and answers `None`.
    pub fn layer_index_by_id(&self, id: u64) -> Option<usize> {
        if id == 0 {
            return None;
        }
        self.layers.iter().position(|l| l.id == id)
    }

    /// Entry `idx`'s pixel-storage identity (the module doc's second
    /// identity); `None` for an out-of-range index or a GROUP entry.
    pub fn layer_storage_identity(&self, idx: usize) -> Option<u64> {
        let layer = self.layers.get(idx)?;
        if layer.kind == LayerKind::Group {
            return None;
        }
        Some(layer.pixels.storage_identity())
    }

    /// Entry `idx`'s raw-mask storage identity (the module doc's third
    /// identity); `None` for an out-of-range index, an entry with no mask or
    /// a GROUP.
    pub fn layer_mask_storage_identity(&self, idx: usize) -> Option<u64> {
        let layer = self.layers.get(idx)?;
        if layer.kind == LayerKind::Group {
            return None;
        }
        Some(Arc::as_ptr(layer.mask.as_ref()?) as usize as u64)
    }
}

impl PrecisionDocumentLayer {
    /// This record's stable identity — `Layer::id`'s twin at high depth.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// This record and, for a group, its whole subtree copied with FRESH
    /// identities — what a typed duplicate hands back, so a host keying view
    /// state on an id never sees the original's state on the copy.
    pub(crate) fn freshly_identified(&self) -> Self {
        let mut copy = self.clone();
        copy.id = mint_layer_id();
        copy.content = match &copy.content {
            Content::Group(children) => Content::Group(std::sync::Arc::new(
                children.iter().map(Self::freshly_identified).collect(),
            )),
            Content::PassThrough(children) => Content::PassThrough(std::sync::Arc::new(
                children.iter().map(Self::freshly_identified).collect(),
            )),
            other => other.clone(),
        };
        copy
    }
}

impl PrecisionDocument {
    /// Record `idx`'s stable identity, in the same flattened record numbering
    /// `PrecisionDocument::layer` indexes; `None` for an out-of-range index.
    pub fn layer_id(&self, idx: usize) -> Option<u64> {
        Some(self.layer(idx)?.id())
    }

    /// The record index the layer with this identity currently sits at.
    pub fn layer_index_by_id(&self, id: u64) -> Option<usize> {
        if id == 0 {
            return None;
        }
        (0..self.layer_count()).find(|&i| self.layer_id(i) == Some(id))
    }

    /// Record `idx`'s raw-mask storage identity — the byte
    /// [`DrDocument::layer_mask_storage_identity`]'s twin over the mask
    /// plane's samples; `None` for an out-of-range index, a record with no
    /// mask or a group.
    pub fn layer_mask_storage_identity(&self, idx: usize) -> Option<u64> {
        let layer = self.layer(idx)?;
        if matches!(layer.content, Content::Group(_) | Content::PassThrough(_)) {
            return None;
        }
        Some(Arc::as_ptr(&layer.mask.as_ref()?.plane.pixels) as usize as u64)
    }
}
