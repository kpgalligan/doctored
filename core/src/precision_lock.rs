//! Layer LOCKS and LINKS at high depth: the typed twins of `doc_lock`'s four
//! bits and of `doc_align`'s two link ops.
//!
//! **Every rule here is the BYTE one, verbatim.** `doc_lock.rs`'s module doc
//! is the specification of what each bit forbids — the frozen alpha channel,
//! the two pixels that get their colour back with it, why a transform is a
//! POSITION edit and never a Pixels one, why Lock All is the one spelling
//! that also freezes the mask, why a MASK APPLY and a MERGE answer to Lock
//! Transparency, and why locks never block a rename, a reorder, an opacity
//! change or a colour label. `doc_align.rs`'s `link_layers` / `unlink_layers`
//! doc comments are the specification of the link ops (the smallest unused
//! id, computed after the set's own ids are cleared, and the lone-member
//! prune). Nothing about what a lock MEANS is decided here; only how it is
//! expressed over typed records.
//!
//! # The two shapes that differ
//!
//! A byte document stores its stack flat, so `lock_block`'s POSITION walk over
//! a group's descendants is a slice range and the link ops are one pass over
//! one vector. A typed document stores a TREE. Two different expressions
//! follow, and each is the cheapest that is still exact:
//!
//! * An entry's effective locks include every enclosing group's
//!   (`doc_lock`'s "A group's locks cover its contents"). The byte stack finds
//!   a parent by depth; here `ancestors` is one descent shaped like
//!   `PrecisionDocument::layer`'s that collects the postorder group indices
//!   it steps into, and the per-entry rule is `doc_lock::entry_block` itself.
//! * `lock_block` reads the POSTORDER numbering directly — a group record at
//!   index `i` holding `records` records owns exactly `i + 1 - records ..= i`,
//!   because postorder puts a node's whole subtree immediately before it — so
//!   the descendant walk is `PrecisionDocument::layer` calls and nothing is
//!   cloned.
//! * The two LINK ops rewrite a field on many records at once, so they go
//!   through `precision_structure_ops`' flat list (`flatten` / `rebuild`),
//!   which is the same splice model the five structure ops use.
//!
//! # The ONE enforcement point
//!
//! Every typed op that writes a layer wraps its body in [`PrecisionDocument::
//! under_locks`], exactly as the byte ops wrap theirs. The difference is the
//! REPORTING shape: a typed edit reaches the host through
//! `dr_precision_doc_edit`, whose refusals are named strings, so
//! `under_locks` answers `Err(lock_sentence(..))` where the byte twin answers
//! `None`. The host's own pre-checks (`LockFlags.refusal` through
//! `lockBlocking`) still phrase the sentence the user sees; this one is the
//! backstop for a caller that did not ask.
//!
//! The two MERGES are the same exception they are at byte depth, for the same
//! reason: they drain their operands' subtrees, so the entry the lock belongs
//! to does not survive at the index it was asked about. `precision_merge`
//! therefore asks [`PrecisionDocument::lock_block`] directly with
//! `EditKind::Merge` before touching anything.
//!
//! One gap is DISCLOSED rather than papered over: the retained-source and RAW
//! edit families (`precision_retained_source.rs`, `raw_source_edit.rs`) are
//! not gated by locks at either depth. Owner 12E-2.
//!
//! **Every rule is `doc_lock.rs`'s and `doc_align.rs`'s verbatim** — the four bits, the frozen
//! alpha channel and the two pixels that get their colour back with it, "a transform is a POSITION
//! edit and never a Pixels one", Lock All as the one spelling that also freezes the mask, the
//! MaskApply and Merge rules, the smallest-unused link id and the lone-member prune. A
//! GROUP's locks cover its contents at both depths: `lock_block` ORs `doc_lock::entry_block` (the
//! one per-entry table) over the entry and every enclosing group — byte ancestors from
//! `doc_group::parent`, typed ones from `ancestors`, one `layer()`-shaped descent collecting
//! postorder group indices — Lock All still means one entry holding all three bits,
//! `freezing_alpha` freezes when a transparency owner exists, and `lock_owner` (`dr_doc_lock_owner`
//! / `dr_precision_doc_lock_owner`) names the nearest entry whose own bits hold the named bit, so
//! `lock_sentence(index, owner, …)` says "inside group G". Two expressions differ and each is the
//! cheapest exact one: `lock_block`'s POSITION walk reads the POSTORDER numbering directly (a group
//! at index `i` holding `records` records owns `i + 1 - records ..= i`, so nothing is cloned), and
//! the two LINK ops go through `precision_structure_ops`' flat list because they rewrite one field
//! on many records. Every typed writer is wrapped (`precision_paint`, `precision_paint_tools`,
//! `precision_retouch`, `precision_scalar_edit` — RGBA → Pixels, mask → Mask, channel ungated —
//! `precision_adjustment`, `selected_adjustment`, `artwork_document`, `precision_geometry` —
//! per-layer affine/perspective → Position or Mask, canvas ops ungated — `precision_mask_edit` and
//! `precision_mask_apply`); three sites CONSULT the lock with `lock_refusal` rather than wrapping
//! it, because they do not write the target's own buffer and `under_locks`' no-op latch would
//! otherwise throw the edit away, and one (a mask APPLY) must be NAMED rather than un-applied.
//! **Since 12E-2b the retained-source and ATTACHMENT families ARE lock-gated at both depths**
//! (`core/tests/source_locks.rs`): CaptureRaster/Replace answer to `Merge`, Capture
//! and Replace ALSO to `Position` when the destination extent or offset would move, Transform to
//! `Position`, Rasterize to `Pixels` after its no-source no-op, and every RAW/host-media/local-mask
//! attachment writer to `Pixels` through the new `RawDocument::attachment_lock_refusal`;
//! Place/Retain are ungated, the last because insertion is STRUCTURE. Each is a
//! CONSULT rather than a wrap, since `under_locks`' no-op latch would throw a binding-only change
//! away. The 12E-1b disclosure is corrected with it: the BYTE `doc_retained_source.rs` had
//! consulted `lock_block` since 8A and only lacked tests.

use crate::doc_lock::{
    entry_block, EditKind, LOCK_ALL, LOCK_PIXELS, LOCK_POSITION, LOCK_TRANSPARENCY,
};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::PrecisionStorage;
use crate::precision_structure_ops::{flatten, rebuild, FlatRecord};

/// The bit a refusal NAMES out of `blocking`, and its word: `LOCK_ALL` names
/// itself; otherwise the first set bit in the order the byte documentation
/// lists them.
pub(crate) fn named(blocking: u32) -> (u32, &'static str) {
    if blocking & LOCK_ALL == LOCK_ALL {
        (LOCK_ALL, "all")
    } else if blocking & LOCK_TRANSPARENCY != 0 {
        (LOCK_TRANSPARENCY, "transparency")
    } else if blocking & LOCK_PIXELS != 0 {
        (LOCK_PIXELS, "pixels")
    } else {
        (LOCK_POSITION, "position")
    }
}

/// The refusal a typed lock phrases, naming the bit that stopped the edit.
/// `owner` is the record whose own bits hold that bit
/// ([`PrecisionDocument::blocking_owner`]); when it is an enclosing group
/// rather than `index` itself, the sentence names the group.
pub(crate) fn lock_sentence(index: usize, owner: usize, blocking: u32) -> String {
    let (_, which) = named(blocking);
    if owner != index {
        return format!(
            "high-depth layer {index} is inside group {owner}, which is locked ({which})"
        );
    }
    format!("high-depth layer {index} is locked ({which})")
}

/// Whether two typed records carry the same mask — the other half of
/// [`PrecisionDocument::freezing_alpha`]'s no-op latch, so an edit that
/// touched only the mask is not thrown away with the pixels
/// (`doc_lock::same_mask`).
fn same_mask(a: &PrecisionDocumentLayer, b: &PrecisionDocumentLayer) -> bool {
    match (a.mask(), b.mask()) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.enabled() == y.enabled()
                && x.properties() == y.properties()
                && x.plane().dimensions() == y.plane().dimensions()
                && x.plane().samples() == y.plane().samples()
        }
        _ => false,
    }
}

impl PrecisionDocument {
    /// Which of record `index`'s EFFECTIVE lock bits would block an edit of
    /// `kind` (0 = allowed) — `doc_lock::lock_block`'s rules over typed
    /// records: `entry_block` of its own bits OR'd with `entry_block` of every
    /// enclosing group's. 0 for an out-of-range index, as at byte
    /// depth.
    ///
    /// A POSITION edit on a GROUP also answers with its descendants' position
    /// bits, because moving a group moves them; postorder is what makes that
    /// walk a range.
    pub fn lock_block(&self, index: usize, kind: EditKind) -> u32 {
        let Some(entry) = self.layer(index) else {
            return 0;
        };
        let mut blocked = entry_block(entry.locks(), kind);
        if kind == EditKind::Position && entry.children().is_some() {
            // Postorder: a node's whole subtree sits immediately before it,
            // so the descendants are exactly the records
            // `index + 1 - records .. index`.
            let start = index + 1 - entry.records.min(index + 1);
            for i in start..index {
                blocked |= self.layer(i).map_or(0, |l| l.locks()) & LOCK_POSITION;
            }
        }
        for group in self.ancestors(index) {
            blocked |= entry_block(self.layer(group).map_or(0, |l| l.locks()), kind);
        }
        blocked
    }

    /// The postorder indices of record `index`'s enclosing groups, NEAREST
    /// first; empty at the top level and for an out-of-range index. One
    /// descent shaped like [`PrecisionDocument::layer`]'s, collecting each
    /// group it steps into: a group spanning `records` records whose subtree
    /// starts at `base` is itself record `base + records - 1`.
    fn ancestors(&self, index: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut layers: &[PrecisionDocumentLayer] = &self.layers;
        let mut base = 0usize;
        let mut rest = index;
        'descend: loop {
            for layer in layers {
                if rest >= layer.records {
                    rest -= layer.records;
                    base += layer.records;
                    continue;
                }
                if let Content::Group(children) | Content::PassThrough(children) = &layer.content {
                    if rest < layer.records - 1 {
                        out.push(base + layer.records - 1);
                        layers = &children[..];
                        continue 'descend;
                    }
                }
                break 'descend;
            }
            return Vec::new();
        }
        out.reverse();
        out
    }

    /// The record whose OWN bits hold every bit of `locks` (masked to
    /// `LOCK_ALL`): `index` itself first, then its enclosing groups outward,
    /// nearest first — `doc_lock::lock_owner` over typed records. `None` when
    /// the mask is empty, `index` is out of range, or nothing matches.
    pub fn lock_owner(&self, index: usize, locks: u32) -> Option<usize> {
        let wanted = locks & LOCK_ALL;
        if wanted == 0 {
            return None;
        }
        std::iter::once(index)
            .chain(self.ancestors(index))
            .find(|&i| self.layer(i).is_some_and(|l| l.locks() & wanted == wanted))
    }

    /// The record a refusal of `blocking` names: the owner of the bit
    /// [`lock_sentence`] words, or `index` itself when no record in its chain
    /// holds it (a group's move blocked by a descendant's position bit).
    pub(crate) fn blocking_owner(&self, index: usize, blocking: u32) -> usize {
        self.lock_owner(index, named(blocking).0).unwrap_or(index)
    }

    /// Pure setter: replaces record `index`'s lock flags, masking off the
    /// reserved bits so a caller cannot store a value this build would not
    /// understand (`doc_lock::with_layer_locks`). `Ok(None)` when the record
    /// already carries exactly those flags — the core's purity rule.
    pub fn with_layer_locks(&self, index: usize, locks: u32) -> Result<Option<Self>, String> {
        let locks = locks & LOCK_ALL;
        let old = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        if old.locks() == locks {
            return Ok(None);
        }
        let mut layer = old.clone();
        layer.locks = locks;
        self.replacing_layer(index, layer)
    }

    /// Links the given records: they take the SMALLEST unused non-zero link
    /// id, computed after their own old ids are cleared, so re-linking the
    /// same set is `Ok(None)` rather than a document that differs only in a
    /// number. An id left with a single member afterwards is cleared, since a
    /// link group of one is not a link (`doc_align::link_layers`).
    pub fn link_layers(&self, indices: &[usize]) -> Result<Option<Self>, String> {
        let mut flat = flatten(&self.layers);
        let set = index_set(&flat, indices)?;
        if set.len() < 2 {
            return Err("high-depth linking requires at least two layers".into());
        }
        let before: Vec<u32> = flat.iter().map(|r| r.layer.link).collect();
        for &i in &set {
            flat[i].layer.link = 0;
        }
        let id = next_link_id(&flat);
        for &i in &set {
            flat[i].layer.link = id;
        }
        prune_lone_links(&mut flat);
        self.changed_links(&before, flat)
    }

    /// Unlinks the given records. An id left with a single member is cleared
    /// too, for the same reason [`Self::link_layers`] clears one
    /// (`doc_align::unlink_layers`).
    pub fn unlink_layers(&self, indices: &[usize]) -> Result<Option<Self>, String> {
        let mut flat = flatten(&self.layers);
        let set = index_set(&flat, indices)?;
        let before: Vec<u32> = flat.iter().map(|r| r.layer.link).collect();
        for &i in &set {
            flat[i].layer.link = 0;
        }
        prune_lone_links(&mut flat);
        self.changed_links(&before, flat)
    }

    /// The rebuilt document when its link ids differ from `before`, `Ok(None)`
    /// when they do not — the core's no-op rule applied to the one field the
    /// two link ops write (`doc_align::changed_links`).
    fn changed_links(&self, before: &[u32], flat: Vec<FlatRecord>) -> Result<Option<Self>, String> {
        if before
            .iter()
            .zip(flat.iter())
            .all(|(&a, r)| a == r.layer.link)
        {
            return Ok(None);
        }
        let layers = rebuild(flat)?;
        self.rebuilt(
            self.width,
            self.height,
            layers,
            self.channels.as_ref().clone(),
        )
        .map(Some)
    }

    /// The named refusal a lock phrases for record `index` and `kind`, or
    /// `Ok(())` when nothing blocks. The typed edits report through the
    /// request path, so a lock refusal is an `Err` with a sentence rather
    /// than the byte `None`.
    pub(crate) fn lock_refusal(&self, index: usize, kind: EditKind) -> Result<(), String> {
        let blocking = self.lock_block(index, kind);
        if blocking != 0 {
            let owner = self.blocking_owner(index, blocking);
            return Err(lock_sentence(index, owner, blocking));
        }
        Ok(())
    }

    /// Runs `op` under record `index`'s locks — the ONE place typed locks are
    /// enforced (`doc_lock::under_locks`). Refuses by name when a lock forbids
    /// the edit outright, restores the record's original alpha afterwards when
    /// transparency is locked, and answers `Ok(None)` when the restore left
    /// the record byte-identical (the purity latch the byte twin needs for
    /// exactly the same reasons: an eraser stroke, a cleared selection or a
    /// brush landing entirely where alpha is 0 all come back unchanged).
    pub(crate) fn under_locks(
        &self,
        index: usize,
        kind: EditKind,
        op: impl FnOnce(&Self) -> Result<Option<Self>, String>,
    ) -> Result<Option<Self>, String> {
        self.lock_refusal(index, kind)?;
        // A MASK edit of a record with a live vector mask edits the RASTER
        // part: it runs with the raster part alone in the slot, and the
        // product is baked back (`precision_vector_mask`, the byte
        // `doc_lock::under_locks` wrap). A mask APPLY is not wrapped.
        if kind == EditKind::Mask {
            if let Some((exposed, record)) = self.exposing_vector_mask(index)? {
                let Some(edited) = op(&exposed)? else {
                    return Ok(None);
                };
                return Self::rebaking_vector_mask(
                    edited,
                    record,
                    crate::job_control::JobContext::NONE,
                )
                .map(Some);
            }
        }
        let Some(out) = op(self)? else {
            return Ok(None);
        };
        self.freezing_alpha(index, kind, out)
    }

    /// The transparency half of [`Self::under_locks`]: `out` with record
    /// `index`'s original alpha channel restored, a NAMED refusal when the
    /// edit could not be reconciled with a frozen alpha (a resized or moved
    /// buffer), and `Ok(None)` when the restore left the record unchanged.
    ///
    /// A no-op for any record that is not transparency-locked (by its own bit
    /// or an enclosing group's) and for every
    /// edit kind but `Pixels`: a Position edit resamples the alpha with
    /// everything else, a Mask edit never touches the record's alpha at all,
    /// and a Mask APPLY and a Merge — the two that do — are refused outright
    /// by [`Self::lock_block`] rather than silently un-applied.
    fn freezing_alpha(
        &self,
        index: usize,
        kind: EditKind,
        out: Self,
    ) -> Result<Option<Self>, String> {
        let before = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let Some(owner) = self.lock_owner(index, LOCK_TRANSPARENCY) else {
            return Ok(Some(out));
        };
        if kind != EditKind::Pixels {
            return Ok(Some(out));
        }
        let after = out
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let (Some(src), Some(dst)) = (before.source(), after.source()) else {
            // A group or an adjustment holds no alpha channel of its own;
            // there is nothing to freeze.
            return Ok(Some(out));
        };
        if src.dimensions() != dst.dimensions() || before.offset() != after.offset() {
            return Err(lock_sentence(index, owner, LOCK_TRANSPARENCY));
        }
        self.restoring_alpha(index, out)
    }

    /// `out` with record `index`'s original alpha channel restored — the
    /// restore [`Self::freezing_alpha`] runs under a transparency lock, and
    /// the one Preserve Transparency runs (`precision_paint_tools`'
    /// `preserve_alpha`, `doc_lock::restoring_alpha`'s typed twin).
    /// `Ok(None)` when the restore left the record unchanged; `Ok(Some(out))`
    /// untouched for a record with no pixels of its own; `Err` when the edit
    /// resized or moved the buffer.
    pub(crate) fn restoring_alpha(&self, index: usize, out: Self) -> Result<Option<Self>, String> {
        let before = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let after = out
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let (Some(src), Some(dst)) = (before.source(), after.source()) else {
            return Ok(Some(out));
        };
        if src.dimensions() != dst.dimensions() || before.offset() != after.offset() {
            return Err("the alpha restore needs the layer's own buffer, unmoved".into());
        }
        // Two pixels get their COLOUR back as well as their alpha, exactly as
        // `doc_lock::freezing_alpha` explains: one the record already holds at
        // zero alpha (nothing there is part of the picture, and putting the
        // bytes back is what lets the latch below see a byte-exact no-op) and
        // one the OP drove to zero (an erase or a clear, which zero the colour
        // as they go, so restoring only the alpha would resurrect the pixel in
        // black).
        //
        // 13B-3d-2: the restore reads and writes only the bbox of the tiles
        // `dst` does not share with `src` (`unshared_rect`, one rect read of
        // each, written back into `src` through `with_rect`). Exact: in a
        // shared tile `d == s` bitwise, so the pixel below is `s` and `same`
        // holds there; a `dst` sharing no tile restores over the whole plane.
        if src.storage() != dst.storage() {
            return Err(match dst.storage() {
                PrecisionStorage::Rgba32Float => "high-depth lock source is not float".into(),
                PrecisionStorage::Rgba16Unorm => "high-depth lock source is not u16".into(),
            });
        }
        let alloc = || "could not allocate precision output".to_string();
        let (restored, unchanged) = match (src.unshared_rect(dst), dst.storage()) {
            (None, _) => (src.reidentified(), true),
            (Some((x, y, w, h)), PrecisionStorage::Rgba32Float) => {
                let (source, destination) = (
                    src.read_rect_rgba32(x, y, w, h).ok_or_else(alloc)?,
                    dst.read_rect_rgba32(x, y, w, h).ok_or_else(alloc)?,
                );
                let mut pixels = crate::precision_image::reserved_vec(destination.len())?;
                let mut same = true;
                for (s, d) in source.iter().zip(destination.iter()) {
                    let pixel = if s[3] == 0.0 || d[3] == 0.0 {
                        *s
                    } else {
                        [d[0], d[1], d[2], s[3]]
                    };
                    same &= pixel == *s;
                    pixels.push(pixel);
                }
                let image = src.with_rect_rgba32(x, y, w, h, &pixels)?;
                (image.unwrap_or_else(|| src.reidentified()), same)
            }
            (Some((x, y, w, h)), PrecisionStorage::Rgba16Unorm) => {
                let (source, destination) = (
                    src.read_rect_rgba16(x, y, w, h).ok_or_else(alloc)?,
                    dst.read_rect_rgba16(x, y, w, h).ok_or_else(alloc)?,
                );
                let mut pixels = crate::precision_image::reserved_vec(destination.len())?;
                let mut same = true;
                for (s, d) in source.iter().zip(destination.iter()) {
                    let pixel = if s[3] == 0 || d[3] == 0 {
                        *s
                    } else {
                        [d[0], d[1], d[2], s[3]]
                    };
                    same &= pixel == *s;
                    pixels.push(pixel);
                }
                let image = src.with_rect_rgba16(x, y, w, h, &pixels)?;
                (image.unwrap_or_else(|| src.reidentified()), same)
            }
        };
        if unchanged && same_mask(after, before) {
            return Ok(None);
        }
        let mut layer = after.clone();
        layer.content = Content::Raster {
            image: restored,
            offset: after.offset(),
        };
        out.replacing_layer(index, layer)
    }
}

/// The given indices over the flat list, deduplicated and ascending. An empty
/// set and an index past the end are NAMED refusals — a caller that named a
/// record that is not there has miscounted, and quietly doing less than it
/// asked is worse than refusing (`doc_align::index_set`).
fn index_set(flat: &[FlatRecord], indices: &[usize]) -> Result<Vec<usize>, String> {
    let mut set = indices.to_vec();
    set.sort_unstable();
    set.dedup();
    match set.last() {
        None => Err("high-depth linking requires at least one layer".into()),
        Some(&last) if last >= flat.len() => Err("high-depth layer index is out of range".into()),
        Some(_) => Ok(set),
    }
}

/// The SMALLEST unused non-zero link id (`doc_group_query::next_link_id`).
pub(crate) fn next_link_id(flat: &[FlatRecord]) -> u32 {
    let mut used: Vec<u32> = flat
        .iter()
        .map(|r| r.layer.link)
        .filter(|&v| v != 0)
        .collect();
    used.sort_unstable();
    used.dedup();
    let mut candidate = 1u32;
    for value in used {
        if value == candidate {
            candidate = candidate.saturating_add(1);
        } else if value > candidate {
            break;
        }
    }
    candidate
}

/// Clears every link id carried by exactly one record: a link group of one is
/// not a link, and leaving it would make the next [`next_link_id`] skip a
/// value for no reason (`doc_align::prune_lone_links`).
fn prune_lone_links(flat: &mut [FlatRecord]) {
    let ids: Vec<u32> = flat.iter().map(|r| r.layer.link).collect();
    for record in flat.iter_mut() {
        if record.layer.link != 0 && ids.iter().filter(|&&id| id == record.layer.link).count() < 2 {
            record.layer.link = 0;
        }
    }
}
