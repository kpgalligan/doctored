//! Moving a SET of entries: the link groups that decide who comes along, the
//! translate and the affine every Move-tool drag and Free Transform commit
//! folds into one call, and align / distribute over a selection.
//!
//! `doc_structure` holds the ops that change the SHAPE of the stack; this file
//! is its sibling for the ops that change where entries SIT, which is where
//! that module's size budget sent them. `doc_group_query` owns the two queries
//! both rest on — `layer_bounds` (the CONTENT box) and `expanded_set`.
//!
//! # One call, one document
//!
//! Every op here takes the whole set and is ALL-OR-NOTHING: a host that
//! looped would pay a document clone per entry and, worse, would leave the
//! document half-moved when one member refused. The set is expanded by
//! SUBTREE (moving a group moves its children) and then by LINK GROUP, and a
//! POSITION lock anywhere in the expanded set refuses the whole call, because
//! a partial move of a linked set is exactly what linking exists to prevent.
//!
//! # Links
//!
//! `Layer::link` is a group id, 0 for unlinked. Ids are the SMALLEST unused
//! non-zero value rather than a counter, so a save round trip is
//! byte-identical; `Channel::id`'s atomic counter is right there for the
//! opposite reason — it is never persisted.
//!
//! # A group's mask travels as bookkeeping
//!
//! A group has no pixel buffer. Its offset mirrors its independent mask's
//! canvas offset. Linked masks translate as bookkeeping; unlinked masks stay
//! fixed while raster descendants move. Raw coverage is retained off canvas.
//!
//! Link fan-out is a property of the two ops that MEAN "move this"
//! ([`DrDocument::move_layers`] and [`DrDocument::transform_layers_with_job`]), never a
//! hidden behaviour of a setter: `with_layer_offset` writes one entry's
//! offset and is what paste and `set_layer_properties` use.
//!
//! Align and distribute expand each entry's delta the same way, so links
//! follow there too — through [`DrDocument::moved_once`], which applies ONE
//! delta per entry rather than looping `move_layers`. The consequence:
//! aligning entries that are LINKED TO EACH OTHER moves each one's whole
//! link group, so the members travel as a rigid body and the last delta
//! applied is the one that sticks. Link a set or align it, not both.
//! ("Sticks" is now literally true: chaining `move_layers` calls ADDED the
//! deltas instead, so an entry two members reached travelled twice as far.)
//!
//! # The gate
//!
//! Every op here ends in `doc_group::validated`, exactly as the ops that DO
//! change the shape of the stack do. None of these can break the depth
//! sequence today — they write offsets, pixels and link ids — and that is
//! precisely why the gate is uniform rather than reasoned about per op: one
//! O(n) scan on a call that already cloned the document is far cheaper than
//! the day someone teaches one of them to move an entry and the check is not
//! there. See `doc_group`'s module doc for why it is a runtime check.

use image::imageops::FilterType;

use crate::align_math::{
    align_deltas, claim_deltas, content_boxes, distribute_deltas, AlignEdge, ContentBox,
};
use crate::doc::{saturating_i32, DrDocument, Layer, LayerKind};
use crate::doc_group::validated;
use crate::doc_group_query::{expanded_set, next_link_id};
use crate::doc_lock::EditKind;
use crate::doc_structure::independent_roots;
use crate::doc_transform::Affine;
use crate::job_control::JobContext;

impl DrDocument {
    /// Translates every given entry by `(dx, dy)` — the ONE move-a-set op, so
    /// the Move tool, the arrow-key nudge and align / distribute all go
    /// through it and all follow subtrees and link groups the same way.
    ///
    /// `None` for an empty or out-of-range set, a zero delta (the core's no-op
    /// rule), a POSITION lock anywhere in the expanded set, and a set that
    /// resolves to groups alone — a group has no pixel rect of its own, so
    /// moving one means moving its raster descendants, and a group with none
    /// has nothing to move.
    pub fn move_layers(&self, idx: &[usize], dx: i32, dy: i32) -> Option<Self> {
        if dx == 0 && dy == 0 {
            return None;
        }
        let set = self.movable_set(idx)?;
        let mut doc = self.clone();
        let mut moved = false;
        for &i in &set {
            moved |= translate_entry(&mut doc.layers, i, dx, dy);
        }
        moved.then_some(doc).and_then(validated)
    }

    /// One delta per ROOT, applied so that every entry moves EXACTLY ONCE —
    /// what align and distribute need and what a chain of
    /// [`Self::move_layers`] calls does not give them.
    ///
    /// Each root is expanded the way `move_layers` expands a set (by subtree,
    /// then by link group) and stakes a claim on every entry it reaches; the
    /// LAST root to claim an entry is the delta that entry takes, which is
    /// what this module's doc has always promised. Looping `move_layers`
    /// instead ADDS the deltas: a selection holding a group and one of its own
    /// children moved that child by its own delta and again by the group's, so
    /// a left-align pushed it off the canvas rather than onto the edge.
    ///
    /// The offsets are written directly, through the same `translate_entry`
    /// the plain move uses, so there is still one definition of what moving an
    /// entry does. `None` when nothing moved.
    fn moved_once(&self, deltas: &[(usize, i32, i32)]) -> Option<Self> {
        let claimed = claim_deltas(self.layers.len(), deltas, |root| {
            expanded_set(&self.layers, &[root])
        });
        let mut doc = self.clone();
        let mut moved = false;
        for (i, delta) in claimed.iter().enumerate() {
            let Some((dx, dy)) = *delta else { continue };
            moved |= translate_entry(&mut doc.layers, i, dx, dy);
        }
        moved.then_some(doc).and_then(validated)
    }

    /// The same affine applied to every given entry, each deriving its own
    /// destination extent through [`DrDocument::transform_layer`] — one
    /// matrix, N layers, and all-or-nothing: the first entry that refuses (an
    /// empty destination extent, an offset outside the i32 range, a
    /// destination past the pixel cap) refuses the whole call, so a Free
    /// Transform over a selection never lands on half of it.
    ///
    /// A GROUP has no pixels to resample: its linked independent mask follows
    /// the same raw-plane affine path, while its raster descendants transform
    /// individually. Unlinked masks stay in their existing canvas locations.
    ///
    /// Same refusals as [`DrDocument::move_layers`] for the set itself.
    /// Polls `job` through every member (see [`Self::transformed_entries`]);
    /// `Err` is only the cancel, and `floor` is the caller's already-live
    /// reservation a retained member's nested render keeps.
    pub(crate) fn transform_layers_with_job(
        &self,
        idx: &[usize],
        m: Affine,
        filter: FilterType,
        job: JobContext<'_>,
        floor: u64,
    ) -> Result<Option<Self>, String> {
        let Some(set) = self.movable_set(idx) else {
            return Ok(None);
        };
        // The vector-mask follow hook (`doc_transform::following_transform`).
        Ok(self
            .transformed_entries(&set, m, filter, false, job, floor)?
            .map(|out| self.following_transform(out, &m)))
    }

    /// The WHOLE stack resampled by one matrix: the Crop tool's straighten,
    /// and the agent `crop` tool's.
    ///
    /// Deliberately NOT gated by per-entry POSITION locks. Straightening is a
    /// document GEOMETRY op, in the same family as [`DrDocument::crop`],
    /// `geometry`, `canvas_resize` and `resize`, not one of which consults a
    /// layer lock: a position lock says "do not move this layer WITHIN the
    /// picture", and re-framing the picture moves nothing within it. Routing
    /// the straighten through [`Self::transform_layers_with_job`] instead made ONE
    /// locked layer refuse an entire document-wide crop, with no way for
    /// the host to explain it and no matching refusal from the four ops beside
    /// it.
    ///
    /// `None` covers both a refusal — the same per-entry refusals
    /// [`Self::transform_layers_with_job`] has (an empty destination extent,
    /// an offset outside the i32 range, a destination past the pixel cap) —
    /// and a stack with no member to write, which
    /// [`Self::straighten_writes_nothing`] tells apart.
    pub fn straighten_layers(&self, m: Affine, filter: FilterType) -> Option<Self> {
        self.straighten_layers_with_job(m, filter, JobContext::NONE, 0)
            .ok()
            .flatten()
    }

    /// [`Self::straighten_layers`] under a `job`: the same body, member k of
    /// n polled and reported by `transformed_entries`. Refusals stay
    /// `Ok(None)`; `Err` is only the cancel.
    pub(crate) fn straighten_layers_with_job(
        &self,
        m: Affine,
        filter: FilterType,
        job: JobContext<'_>,
        floor: u64,
    ) -> Result<Option<Self>, String> {
        let set: Vec<usize> = (0..self.layers.len()).collect();
        let Some(mut out) = self.transformed_entries(&set, m, filter, true, job, floor)? else {
            return Ok(None);
        };
        out.comps = self.straightened_comps(&m);
        Ok(Some(out))
    }

    /// True when [`Self::straighten_layers`] has no member to write: every
    /// entry is a mask-less group or a mask-less adjustment layer. The master
    /// rule is `transformed_entries` under `all_masks = true` (every raster
    /// that is not an adjustment layer is written, and so is every mask);
    /// this restates only which entries it would skip.
    pub(crate) fn straighten_writes_nothing(&self) -> bool {
        self.layers.iter().all(|layer| {
            layer.mask.is_none()
                && (layer.kind == LayerKind::Group
                    || crate::doc_geometry_set::is_adjustment(layer.meta.as_deref()))
        })
    }

    /// The body both transforms share: `set` is already the entries to write,
    /// and the lock policy has already been decided by whoever built it.
    ///
    /// A raster entry goes through `transform_layer_unlocked_with_job` rather than
    /// `transform_layer` because the gate belongs to the SET, not to the
    /// entry: `transform_layers_with_job` asked `movable_set` all-or-nothing before the
    /// first pixel moved, and `straighten_layers` is exempt by design. Asking
    /// again per entry would be a second lock policy in a file whose whole
    /// point is that there is one.
    ///
    /// `None` when nothing was written — an empty group (which PSD import and
    /// a crafted `.rz` both produce) has no pixels to resample and no mask to
    /// warp, and returning an identical copy for it would mint an undo step
    /// and dirty the document for an edit that moved nothing.
    ///
    /// Under a `job`, member k of n reports stage 6 at k/n and runs in the
    /// `k/n … (k+1)/n` slice of the range, so a retained member's nested
    /// completion report lands at its own end. Refusals stay `Ok(None)`;
    /// `Err` is only the cancel.
    fn transformed_entries(
        &self,
        set: &[usize],
        m: Affine,
        filter: FilterType,
        all_masks: bool,
        job: JobContext<'_>,
        floor: u64,
    ) -> Result<Option<Self>, String> {
        // Plan the complete set before allocating its first transformed
        // raster or independent mask. A later aggregate refusal must not pay
        // for all earlier resamples first.
        let mut pixel_dims = Vec::with_capacity(self.layers.len());
        let mut mask_dims = Vec::new();
        for (i, layer) in self.layers.iter().enumerate() {
            let transforms = set.contains(&i);
            let dims = if transforms
                && layer.kind == LayerKind::Raster
                && !crate::doc_geometry_set::is_adjustment(layer.meta.as_deref())
            {
                let Some(b) = crate::doc_transform::transformed_plane_bounds(
                    layer.pixels.dimensions(),
                    layer.offset,
                    &m,
                ) else {
                    return Ok(None);
                };
                (b.2, b.3)
            } else {
                layer.pixels.dimensions()
            };
            pixel_dims.push(dims);
            if let Some(raw) = &layer.mask {
                let dims = if transforms && (all_masks || layer.mask_properties.linked) {
                    let Some(b) = crate::doc_transform::transformed_plane_bounds(
                        raw.dimensions(),
                        layer.mask_properties.offset,
                        &m,
                    ) else {
                        return Ok(None);
                    };
                    if !layer.mask_properties.valid((b.2, b.3)) {
                        return Ok(None);
                    }
                    (b.2, b.3)
                } else {
                    raw.dimensions()
                };
                mask_dims.push(dims);
            }
        }
        if !self.native_layers_fit_dimensions(pixel_dims)
            || !self.native_masks_fit_dimensions(mask_dims)
        {
            return Ok(None);
        }
        let kinds: Vec<LayerKind> = set.iter().map(|&i| self.layers[i].kind).collect();
        let n = set.len() as f64;
        let mut doc = self.clone();
        let mut changed = false;
        for (k, (&i, &kind)) in set.iter().zip(kinds.iter()).enumerate() {
            let k = k as f64;
            job.report(k / n, 6)?;
            let member = job.range(k / n, (k + 1.) / n);
            match kind {
                LayerKind::Raster => {
                    if crate::doc_geometry_set::is_adjustment(doc.layers[i].meta.as_deref()) {
                        if doc.layers[i].mask.is_some()
                            && (all_masks || doc.layers[i].mask_properties.linked)
                        {
                            if doc.layers[i]
                                .affine_mask_with_job(m, filter, member)?
                                .is_none()
                            {
                                return Ok(None);
                            }
                            changed = true;
                        }
                        continue;
                    }
                    let linked = doc.layers[i].mask_properties.linked;
                    if all_masks {
                        doc.layers[i].mask_properties.linked = true;
                    }
                    let Some(next) =
                        doc.transform_layer_unlocked_with_job(i, m, filter, member, floor)?
                    else {
                        return Ok(None);
                    };
                    doc = next;
                    doc.layers[i].mask_properties.linked = linked;
                    changed = true;
                }
                LayerKind::Group => {
                    // The group record's reference point and linked anchors
                    // follow M (`doc_reference`); alone it is not an
                    // edit (`changed`).
                    let before = doc.layers[i].clone();
                    crate::doc_reference::follow_layer(
                        &before,
                        &mut doc.layers[i],
                        crate::reference_point::ReferenceOp::Transform,
                        [m.a, m.b, m.c, m.d, m.tx, m.ty],
                        &crate::doc_reference::TurnedTiles::default(),
                    );
                    if doc.layers[i].mask.is_some()
                        && (all_masks || doc.layers[i].mask_properties.linked)
                    {
                        if doc.layers[i]
                            .affine_mask_with_job(m, filter, member)?
                            .is_none()
                        {
                            return Ok(None);
                        }
                        changed = true;
                    }
                }
            }
        }
        Ok(changed.then_some(doc).and_then(validated))
    }

    /// Aligns every given entry's CONTENT bounds ([`DrDocument::layer_bounds`])
    /// to `edge` of the reference rect: the UNION of the given entries' bounds
    /// (`to_canvas` false) or the canvas.
    ///
    /// Content bounds, not the pixel rect: a photo-shaped layer on a
    /// canvas-sized buffer — what every paste, text layer and shape layer
    /// produces — has two different rectangles, and the one a person means is
    /// the opaque one. An entry with nothing opaque is skipped rather than
    /// aligned by a rect it does not really occupy.
    ///
    /// Moves through [`DrDocument::move_layers`], so subtrees and link groups
    /// follow and a POSITION lock refuses the whole call — asked UP FRONT
    /// here, not left to the per-entry moves below (see [`Self::movable_set`]).
    /// `None` when nothing would move, and when fewer than two entries have
    /// content while `to_canvas` is false — aligning one layer to itself is a
    /// no-op by definition.
    pub fn align_layers(&self, idx: &[usize], edge: AlignEdge, to_canvas: bool) -> Option<Self> {
        self.movable_set(idx)?;
        let boxes = self.content_boxes(idx)?;
        let canvas = to_canvas.then_some((self.width, self.height));
        self.moved_once(&align_deltas(&boxes, edge, canvas)?)
    }

    /// Spaces the given entries evenly along one axis: the two OUTERMOST keep
    /// their positions and the gaps between adjacent CONTENT bounds are made
    /// equal — "distribute spacing", not "distribute centers".
    ///
    /// Needs at least three entries with content. The gap comes out negative
    /// when the content is wider than the span it has to fit into; that is
    /// arithmetic, not an error, and the entries simply overlap evenly.
    ///
    /// `None` for fewer than three entries with content, for the same set
    /// refusals as [`DrDocument::move_layers`] — the POSITION lock among them,
    /// asked UP FRONT (see [`Self::movable_set`]) — and when nothing would
    /// move (already evenly spaced).
    pub fn distribute_layers(&self, idx: &[usize], vertical: bool) -> Option<Self> {
        self.movable_set(idx)?;
        let boxes = self.content_boxes(idx)?;
        self.moved_once(&distribute_deltas(&boxes, vertical)?)
    }

    /// Links the given entries: they take the SMALLEST unused non-zero link
    /// id, computed after their own old ids are cleared, so re-linking the
    /// same set is a no-op rather than a document that differs only in a
    /// number. An id left with a single member afterwards is cleared, since a
    /// link group of one is not a link.
    ///
    /// `None` for fewer than two entries, an out-of-range index, and when
    /// nothing would change.
    pub fn link_layers(&self, idx: &[usize]) -> Option<Self> {
        let set = self.index_set(idx)?;
        if set.len() < 2 {
            return None;
        }
        let mut doc = self.clone();
        for &i in &set {
            doc.layers[i].link = 0;
        }
        let id = next_link_id(&doc.layers);
        for &i in &set {
            doc.layers[i].link = id;
        }
        prune_lone_links(&mut doc.layers);
        changed_links(self, doc).and_then(validated)
    }

    /// Unlinks the given entries. An id left with a single member is cleared
    /// too, for the same reason [`DrDocument::link_layers`] clears one.
    /// `None` for an empty or out-of-range set and when nothing would change.
    pub fn unlink_layers(&self, idx: &[usize]) -> Option<Self> {
        let set = self.index_set(idx)?;
        let mut doc = self.clone();
        for &i in &set {
            doc.layers[i].link = 0;
        }
        prune_lone_links(&mut doc.layers);
        changed_links(self, doc).and_then(validated)
    }

    /// The given indices, deduplicated and ascending. `None` for an empty set
    /// and for any index past the end — a caller that named an entry that is
    /// not there has miscounted, and quietly doing less than it asked is worse
    /// than refusing.
    fn index_set(&self, idx: &[usize]) -> Option<Vec<usize>> {
        let mut set = idx.to_vec();
        set.sort_unstable();
        set.dedup();
        if set.is_empty() || *set.last()? >= self.layers.len() {
            return None;
        }
        Some(set)
    }

    /// [`Self::index_set`] expanded by subtree and link group, with the
    /// all-or-nothing POSITION lock check every move-a-set op makes.
    ///
    /// `align_layers` and `distribute_layers` call it for the CHECK alone,
    /// before computing a single delta, and discard the set they get back.
    /// They could not leave the check to the [`Self::move_layers`] calls they
    /// make per entry: an entry whose delta is zero — and a left-align always
    /// has at least one, the entry that DEFINES the reference edge — never
    /// reaches `move_layers` at all, and an entry with nothing opaque in it is
    /// skipped before that. Whether a position lock was honoured would then
    /// depend on invisible geometry rather than on the lock, and the promise
    /// both ops' doc comments make (and that the host repeats when it names
    /// the lock in a refusal) would be false exactly when a locked entry
    /// happened to already sit where the alignment wanted it.
    fn movable_set(&self, idx: &[usize]) -> Option<Vec<usize>> {
        let set = expanded_set(&self.layers, &self.index_set(idx)?);
        if set.is_empty() {
            return None;
        }
        if set
            .iter()
            .any(|&i| self.lock_block(i, EditKind::Position) != 0)
        {
            return None;
        }
        Some(set)
    }

    /// The given entries paired with their CONTENT boxes as
    /// `(x0, y0, x1, y1)` (exclusive), skipping every entry with nothing
    /// opaque in it. `None` when the set is empty or out of range, or when no
    /// entry in it has content at all.
    ///
    /// Reduced to `doc_structure::independent_roots` first — the same rule
    /// `duplicate_layers` and `remove_layers` apply — so an entry that sits
    /// inside another given entry's subtree is DROPPED rather than being given
    /// a delta of its own. Aligning a group already aligns its children, and
    /// giving a child a second delta on top of that is not a refinement, it is
    /// a double move.
    fn content_boxes(&self, idx: &[usize]) -> Option<Vec<(usize, ContentBox)>> {
        let roots = independent_roots(&self.layers, idx)?;
        let boxes = content_boxes(&roots, |i| self.layer_bounds(i));
        (!boxes.is_empty()).then_some(boxes)
    }
}

/// Adds `(dx, dy)` to entry `i`'s offset; whether anything was written. The
/// ONE place a translate touches an offset, shared by
/// [`DrDocument::move_layers`], [`DrDocument::moved_once`] and
/// [`DrDocument::with_layer_offset`]'s group branch.
///
/// A group has no pixel rect: only its linked mask moves here, while raster
/// descendants move independently. Every mask retains its complete raw plane;
/// a move out and back is lossless. False if this entry has nothing to move.
pub(crate) fn translate_entry(layers: &mut [Layer], i: usize, dx: i32, dy: i32) -> bool {
    let Some(layer) = layers.get_mut(i) else {
        return false;
    };
    if dx == 0 && dy == 0 {
        return false;
    }
    // Every entry the translate reaches accumulates it, group or raster
    // (`Layer::reference`); the return value is unchanged, so a reference
    // alone never makes a move an edit.
    layer.reference = crate::doc::translated_offset(layer.reference, i64::from(dx), i64::from(dy));
    let mask_moves = layer.mask.is_some() && layer.mask_properties.linked;
    if layer.kind == LayerKind::Group && !mask_moves {
        return false;
    }
    if layer.kind == LayerKind::Raster {
        layer.offset = (
            saturating_i32(i64::from(layer.offset.0) + i64::from(dx)),
            saturating_i32(i64::from(layer.offset.1) + i64::from(dy)),
        );
    }
    if mask_moves {
        layer.translate_mask(i64::from(dx), i64::from(dy));
    }
    true
}

/// Clears every link id carried by exactly one entry: a link group of one is
/// not a link, and leaving it would make the next `next_link_id` skip a value
/// for no reason.
fn prune_lone_links(layers: &mut [Layer]) {
    let ids: Vec<u32> = layers.iter().map(|l| l.link).collect();
    for layer in layers.iter_mut() {
        if layer.link != 0 && ids.iter().filter(|&&id| id == layer.link).count() < 2 {
            layer.link = 0;
        }
    }
}

/// `doc` when its link ids differ from `before`'s, `None` when they do not —
/// the core's no-op rule applied to the one field these two ops write.
fn changed_links(before: &DrDocument, doc: DrDocument) -> Option<DrDocument> {
    let same = before
        .layers
        .iter()
        .zip(doc.layers.iter())
        .all(|(a, b)| a.link == b.link);
    (!same).then_some(doc)
}
