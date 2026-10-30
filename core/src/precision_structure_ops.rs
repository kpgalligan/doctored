//! The typed twins of the four pure STRUCTURE operations: the structural move
//! (`move_layer_to`), the sibling reorder (`arrange_layer`), subset
//! `group_layers` / `ungroup_layer` and the set `remove_layers`.
//!
//! Every contract here is the BYTE one, verbatim: `doc_group_ops` owns the
//! move, the grouping and the ungrouping, `doc_structure` the sibling reorder
//! and `doc_structure_sets::remove_layers` the set delete, and each of those
//! doc comments is the specification this file implements at high depth.
//! Nothing about what an op MEANS is decided here; only how it is expressed
//! over typed records.
//!
//! # Why a FLAT postorder list
//!
//! A byte document already stores its stack flat, with a `depth` field per
//! entry, so every structural op is a splice over one vector. A typed
//! document stores a TREE — each group owns an `Arc<Vec<_>>` of children —
//! and the same ops written over that tree would be five recursive rewrites
//! that agree with the byte five only by inspection.
//!
//! So the tree is flattened into exactly the postorder numbering
//! `PrecisionDocument::layer` already indexes (children precede their group,
//! which is the byte stack's own order), the byte splice runs over the flat
//! list, and [`rebuild`] rebuilds the tree. The record's pixels, mask, style,
//! Blend If, description and retained source travel through as the very
//! allocations they were — [`flatten`] and [`rebuild`] only move nodes — and
//! each group's `records`/`depth` bookkeeping is RECOMPUTED by
//! `PrecisionDocumentLayer::group`, which is the one place that arithmetic
//! lives.
//!
//! Identity follows `doc_identity`'s rule: a move, a delete and an ungroup
//! CARRY every id through, grouping mints for the container alone and its
//! members keep theirs.
//!
//! Purity follows `core/CLAUDE.md`: an op whose result would be the document
//! it was given answers `None`, never an identical copy.
//!
//! `move_layer_to`, `arrange_layer`, subset `group_layers`, `ungroup_layer` and `remove_layers` on
//! `PrecisionDocument`, reached through `precision_structure.rs`'s `StructureEdit` and so through
//! the one `dr_precision_doc_edit` request surface. The model is a FLAT POSTORDER record list,
//! because a byte document already stores its stack flat with a per-entry level and writing the
//! five ops over the typed TREE would be five recursive rewrites agreeing with the byte five only
//! by inspection: `flatten` produces exactly the numbering `PrecisionDocument::layer` indexes, the
//! byte splice runs over it, and `rebuild` rebuilds through `PrecisionDocumentLayer::group` — the
//! one place the `records`/`depth` arithmetic lives. Every op answers `None` for an exact no-op;
//! the two grouping ops answer a REPORT (`{group, cleared_clip, reordered}` / `{cleared_clip,
//! landed}`) that rides in the PLAN reply under `"report"`. Phase 12E-1b adds six more
//! `StructureEdit` variants through the same surface — `merge_layers` / `merge_down` /
//! `merge_visible` (`precision_merge.rs`) and `layer_lock` / `link_layers` / `unlink_layers`
//! (`precision_lock.rs`) — and makes `flatten`, `rebuild`, `subtree`, `parent`, `level_range`,
//! `units` and `FlatRecord` `pub(crate)` so those two modules share this ONE model rather than
//! writing a second.

use std::ops::Range;
use std::sync::Arc;

use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_render::MAX_GROUP_DEPTH;
use crate::precision_structure::ArrangeHow;

/// One record of the flattened document: the node itself with any children
/// removed, and the NESTING LEVEL it sits at (0 at the top level).
///
/// `depth` here is the byte stack's `Layer::depth` — a level — and
/// deliberately NOT `PrecisionDocumentLayer::depth`, which is a subtree
/// HEIGHT. The height is recomputed by [`rebuild`], so a stale one in the
/// carried node is never read.
pub(crate) struct FlatRecord {
    pub layer: PrecisionDocumentLayer,
    pub depth: usize,
}

/// The document's records in POSTORDER — children before their group, the
/// order `PrecisionDocument::layer` indexes and the order the byte stack is
/// stored in.
pub(crate) fn flatten(layers: &[PrecisionDocumentLayer]) -> Vec<FlatRecord> {
    fn walk(layers: &[PrecisionDocumentLayer], depth: usize, out: &mut Vec<FlatRecord>) {
        for layer in layers {
            if let Some(children) = layer.children() {
                walk(children, depth + 1, out);
            }
            let mut node = layer.clone();
            node.content = match &layer.content {
                Content::Group(_) => Content::Group(Arc::new(Vec::new())),
                Content::PassThrough(_) => Content::PassThrough(Arc::new(Vec::new())),
                other => other.clone(),
            };
            out.push(FlatRecord { layer: node, depth });
        }
    }
    let mut out = Vec::new();
    walk(layers, 0, &mut out);
    out
}

/// The inverse: the flat list back into the tree, refusing a malformed depth
/// sequence exactly as `doc_group::validate_structure` does — a record
/// shallower than the level it sits in, and a LEAF closing a non-empty run
/// (which would be children with no enclosing group).
///
/// Each group is rebuilt through `PrecisionDocumentLayer::group`, so the
/// record count and the group-depth ceiling are checked by the one
/// constructor that owns them; every other field is carried across
/// explicitly, the `precision_document_edit::edit` pattern.
pub(crate) fn rebuild(flat: Vec<FlatRecord>) -> Result<Vec<PrecisionDocumentLayer>, String> {
    fn level(
        flat: &[FlatRecord],
        range: Range<usize>,
        depth: usize,
    ) -> Result<Vec<PrecisionDocumentLayer>, String> {
        let end = range.end;
        let mut out = Vec::new();
        let mut start = range.start;
        for i in range {
            let record_depth = flat[i].depth;
            if record_depth < depth {
                return Err("high-depth record structure is malformed".into());
            }
            if record_depth != depth {
                continue;
            }
            let record = &flat[i];
            let node = match &record.layer.content {
                Content::Group(_) | Content::PassThrough(_) => {
                    let children = level(flat, start..i, depth + 1)?;
                    let mut group = PrecisionDocumentLayer::group(
                        record.layer.name(),
                        children,
                        record.layer.opacity,
                        record.layer.visible,
                        record.layer.is_pass_through(),
                    )?;
                    // The SAME group with changed children keeps its own
                    // identity and every property `group` does not take.
                    group.id = record.layer.id;
                    group.description = record.layer.description.clone();
                    group.retained_source = record.layer.retained_source.clone();
                    group.warp = record.layer.warp.clone();
                    group.mask = record.layer.mask.clone();
                    group.blend_mode = record.layer.blend_mode;
                    group.clipped = record.layer.clipped;
                    group.blend_if = record.layer.blend_if;
                    group.style = record.layer.style.clone();
                    group.label = record.layer.label;
                    group.reference = record.layer.reference;
                    group.locks = record.layer.locks;
                    group.link = record.layer.link;
                    group.open = record.layer.open;
                    group
                }
                _ => {
                    if start != i {
                        return Err("high-depth record structure is malformed".into());
                    }
                    let mut leaf = record.layer.clone();
                    leaf.records = 1;
                    leaf.depth = 0;
                    leaf
                }
            };
            out.push(node);
            start = i + 1;
        }
        if start != end {
            return Err("high-depth record structure is malformed".into());
        }
        Ok(out)
    }
    level(&flat, 0..flat.len(), 0)
}

/// What `group_layers` changed beyond the nesting, in NEW indices.
pub struct GroupReport {
    /// The new container's record index.
    pub group: usize,
    /// Entries whose `clipped` flag was released (at most one: the
    /// bottom-most member, which has nothing below it inside the group).
    pub cleared_clip: Vec<usize>,
    /// Surviving entries a NON-CONTIGUOUS set pushed past the gathered block.
    pub reordered: Vec<usize>,
}

/// What `ungroup_layer` changed beyond the nesting, in NEW indices.
pub struct UngroupReport {
    /// The freed child whose baseless `clipped` flag was released.
    pub cleared_clip: Vec<usize>,
    /// The freed children, bottom-first.
    pub landed: Vec<usize>,
}

// ---------------------------------------------------- flat-list geometry --

/// `flat[idx]`'s subtree as a half-open range, `start..idx + 1` — the byte
/// `doc_group::subtree` over the same numbering.
pub(crate) fn subtree(flat: &[FlatRecord], idx: usize) -> Range<usize> {
    let Some(record) = flat.get(idx) else {
        return idx..idx;
    };
    let depth = record.depth;
    let mut start = idx;
    while start > 0 && flat[start - 1].depth > depth {
        start -= 1;
    }
    start..idx + 1
}

/// The index of `idx`'s enclosing group, or `None` at the top level.
pub(crate) fn parent(flat: &[FlatRecord], idx: usize) -> Option<usize> {
    let depth = flat.get(idx)?.depth;
    if depth == 0 {
        return None;
    }
    flat[idx + 1..]
        .iter()
        .position(|r| r.depth < depth)
        .map(|offset| idx + 1 + offset)
}

/// The half-open range of the LEVEL `idx` sits in.
pub(crate) fn level_range(flat: &[FlatRecord], idx: usize) -> Range<usize> {
    match parent(flat, idx) {
        Some(group) => subtree(flat, group).start..group,
        None => 0..flat.len(),
    }
}

/// `(subtree start, index)` for every entry of the level spanning `range` at
/// `depth`, bottom-first — the byte `doc_group::units`.
pub(crate) fn units(flat: &[FlatRecord], range: Range<usize>, depth: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = range.start;
    for i in range {
        if flat[i].depth == depth {
            out.push((start, i));
            start = i + 1;
        }
    }
    out
}

/// The given set with every entry SUBSUMED by another given entry's subtree
/// dropped — `doc_structure::independent_roots`.
pub(crate) fn independent_roots(flat: &[FlatRecord], idx: &[usize]) -> Result<Vec<usize>, String> {
    let mut sorted = idx.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.is_empty() || *sorted.last().expect("non-empty") >= flat.len() {
        return Err("high-depth layer index is out of range".into());
    }
    let mut roots: Vec<usize> = Vec::new();
    for &i in sorted.iter().rev() {
        if roots.iter().any(|&kept| subtree(flat, kept).contains(&i)) {
            continue;
        }
        roots.push(i);
    }
    roots.reverse();
    Ok(roots)
}

/// The ONE splice: move `sub` so it sits at `at` in the list with that
/// subtree ALREADY taken out, re-basing every moved record's level by
/// `shift` — `doc_group::relocated`.
fn relocated(
    mut flat: Vec<FlatRecord>,
    sub: Range<usize>,
    at: usize,
    shift: i64,
) -> Result<Vec<FlatRecord>, String> {
    let mut block: Vec<FlatRecord> = flat.drain(sub).collect();
    for record in &mut block {
        let depth = record.depth as i64 + shift;
        if !(0..=MAX_GROUP_DEPTH as i64).contains(&depth) {
            return Err("high-depth group nesting exceeds its limit".into());
        }
        record.depth = depth as usize;
    }
    let at = at.min(flat.len());
    flat.splice(at..at, block);
    Ok(flat)
}

impl PrecisionDocument {
    fn rebuilt_from_flat(&self, flat: Vec<FlatRecord>) -> Result<Self, String> {
        let layers = rebuild(flat)?;
        self.rebuilt(
            self.width,
            self.height,
            layers,
            self.channels.as_ref().clone(),
        )
    }

    /// THE structural move: record `from` and its whole subtree land at index
    /// `to` — an index into the list with that subtree already taken out — at
    /// nesting level `depth`. `Ok(None)` when the block lands back where it
    /// came from at the level it already had (`doc_group_ops::move_layer_to`'s
    /// no-op rule, decidable without building the result).
    pub fn move_layer_to(
        &self,
        from: usize,
        to: usize,
        depth: usize,
    ) -> Result<Option<Self>, String> {
        let flat = flatten(&self.layers);
        if from >= flat.len() || to >= flat.len() {
            return Err("high-depth layer index is out of range".into());
        }
        if depth > MAX_GROUP_DEPTH {
            return Err("high-depth group nesting exceeds its limit".into());
        }
        let from_depth = flat[from].depth;
        let sub = subtree(&flat, from);
        if to.min(flat.len() - sub.len()) == sub.start && depth == from_depth {
            return Ok(None);
        }
        let moved = relocated(flat, sub, to, depth as i64 - from_depth as i64)?;
        self.rebuilt_from_flat(moved).map(Some)
    }

    /// Moves record `index` among its SIBLINGS only — never into or out of a
    /// group, which is [`Self::move_layer_to`]'s job. `Ok(None)` when the
    /// record is already where `how` would put it.
    pub fn arrange_layer(&self, index: usize, how: ArrangeHow) -> Result<Option<Self>, String> {
        let flat = flatten(&self.layers);
        if index >= flat.len() {
            return Err("high-depth layer index is out of range".into());
        }
        let depth = flat[index].depth;
        let level = units(&flat, level_range(&flat, index), depth);
        let position = level
            .iter()
            .position(|&(_, i)| i == index)
            .ok_or("high-depth layer index is out of range")?;
        let top = level.len() - 1;
        let target = match how {
            ArrangeHow::Front | ArrangeHow::Forward if position == top => return Ok(None),
            ArrangeHow::Front => top,
            ArrangeHow::Forward => position + 1,
            ArrangeHow::Backward | ArrangeHow::Back if position == 0 => return Ok(None),
            ArrangeHow::Backward => position - 1,
            ArrangeHow::Back => 0,
        };
        let sub = subtree(&flat, index);
        // Moving UP lands above the target sibling's whole subtree, in the
        // list with `sub` already drained; moving DOWN lands at the start of
        // the target's subtree, which has not moved (`doc_structure`).
        let at = if target > position {
            level[target].1 + 1 - sub.len()
        } else {
            level[target].0
        };
        let moved = relocated(flat, sub, at, 0)?;
        self.rebuilt_from_flat(moved).map(Some)
    }

    /// Wraps the given records in a NEW group. Every record must share ONE
    /// parent; the subtrees are gathered in their existing relative order and
    /// the group is inserted so its subtree occupies the TOPMOST given
    /// record's slot, with the gathered levels shifted by +1
    /// (`doc_group_ops::group_layers`).
    ///
    /// The container MINTS an identity; its members keep theirs
    /// (`doc_identity`).
    ///
    /// A 32-bit FLOAT document honours `pass_through` as every other depth
    /// does, and the float render composites it (`float_document`; measured
    /// against Photoshop 26.10.0, which makes a new group Pass Through at 32
    /// bits).
    pub fn group_layers(
        &self,
        indices: &[usize],
        name: &str,
        pass_through: bool,
    ) -> Result<Option<(Self, GroupReport)>, String> {
        let flat = flatten(&self.layers);
        let mut sorted = indices.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        let (&first, &last) = match (sorted.first(), sorted.last()) {
            (Some(first), Some(last)) => (first, last),
            _ => return Err("high-depth grouping requires at least one layer".into()),
        };
        if last >= flat.len() {
            return Err("high-depth layer index is out of range".into());
        }
        let depth = flat[first].depth;
        let parent_of_set = parent(&flat, first);
        if sorted
            .iter()
            .any(|&i| flat[i].depth != depth || parent(&flat, i) != parent_of_set)
        {
            return Err("high-depth grouping requires one shared parent group".into());
        }
        let subs: Vec<Range<usize>> = sorted.iter().map(|&i| subtree(&flat, i)).collect();
        let deepest = subs
            .iter()
            .flat_map(|r| flat[r.clone()].iter())
            .map(|r| r.depth)
            .max()
            .expect("a non-empty set has a deepest record");
        if deepest.saturating_add(1) > MAX_GROUP_DEPTH {
            return Err("high-depth group nesting exceeds its limit".into());
        }
        let mut gathered_flag = vec![false; flat.len()];
        for range in &subs {
            for i in range.clone() {
                gathered_flag[i] = true;
            }
        }
        let mut gathered: Vec<FlatRecord> = Vec::new();
        let mut gathered_from: Vec<usize> = Vec::new();
        let mut kept: Vec<FlatRecord> = Vec::new();
        let mut kept_from: Vec<usize> = Vec::new();
        for (i, record) in flat.into_iter().enumerate() {
            if gathered_flag[i] {
                gathered.push(FlatRecord {
                    depth: record.depth + 1,
                    layer: record.layer,
                });
                gathered_from.push(i);
            } else {
                kept.push(record);
                kept_from.push(i);
            }
        }
        // The bottom-most grouped record has nothing below it INSIDE the
        // group to clip to, so a set flag is released, as the byte op
        // releases it.
        let mut cleared_clip = Vec::new();
        let bottom = gathered_from
            .iter()
            .position(|&o| o == first)
            .expect("the bottom-most given record is gathered");
        if gathered[bottom].layer.clipped {
            gathered[bottom].layer.clipped = false;
            cleared_clip.push(first);
        }
        let at = kept_from
            .iter()
            .position(|&o| o > last)
            .unwrap_or(kept.len());
        let group_index = at + gathered.len();
        let mut container =
            PrecisionDocumentLayer::group(name, Vec::new(), 1.0, true, pass_through)?;
        container.records = 1;
        container.depth = 0;
        let mut next: Vec<FlatRecord> = Vec::with_capacity(kept.len() + gathered.len() + 1);
        let tail = kept.split_off(at);
        next.extend(kept);
        next.extend(gathered);
        next.push(FlatRecord {
            layer: container,
            depth,
        });
        next.extend(tail);
        // Original index -> new index, so both reports name records the
        // caller can address afterwards.
        let new_index = |original: usize| -> usize {
            match gathered_from.iter().position(|&o| o == original) {
                Some(g) => at + g,
                None => {
                    let k = kept_from
                        .iter()
                        .position(|&o| o == original)
                        .expect("every record is either gathered or kept");
                    if k < at {
                        k
                    } else {
                        k + gathered_from.len() + 1
                    }
                }
            }
        };
        let cleared_clip: Vec<usize> = cleared_clip.into_iter().map(new_index).collect();
        let reordered: Vec<usize> = kept_from
            .iter()
            .copied()
            .filter(|&o| o > subs[0].start && o < last)
            .map(new_index)
            .collect();
        let document = self.rebuilt_from_flat(next)?;
        Ok(Some((
            document,
            GroupReport {
                group: group_index,
                cleared_clip,
                reordered,
            },
        )))
    }

    /// Dissolves group `index`: its children take its level and its slot, in
    /// order, and the group record is removed. The group's own mask, style,
    /// opacity, blend mode, Blend If and clipped flag are DISCARDED — they
    /// cannot be expressed on the children (`doc_group_ops::ungroup_layer`).
    pub fn ungroup_layer(&self, index: usize) -> Result<Option<(Self, UngroupReport)>, String> {
        let flat = flatten(&self.layers);
        let record = flat
            .get(index)
            .ok_or("high-depth layer index is out of range")?;
        if !matches!(
            record.layer.content,
            Content::Group(_) | Content::PassThrough(_)
        ) {
            return Err("high-depth ungroup requires a group record".into());
        }
        if flat.len() == 1 {
            return Err("a high-depth document must keep at least one record".into());
        }
        let group_depth = record.depth;
        let start = subtree(&flat, index).start;
        // Read off the ORIGINAL levels, before the children are re-based:
        // the freed children, and the bottom-most CHILD — which is not
        // `start` whenever that child is itself a group, in which case
        // `start` is the deepest grandchild and its clipping never changes.
        let landed: Vec<usize> = units(&flat, start..index, group_depth + 1)
            .iter()
            .map(|&(_, idx)| idx)
            .collect();
        // A child that was BASELESS inside the group stays baseless:
        // releasing the flag is what stops it silently clipping to whatever
        // sits below the group.
        let releases = landed
            .first()
            .copied()
            .filter(|&b| flat[b].layer.clipped)
            .filter(|_| {
                let level = level_range(&flat, index);
                units(&flat, level.start..start, group_depth)
                    .iter()
                    .any(|&(_, idx)| !flat[idx].layer.clipped)
            });
        let mut flat = flat;
        for record in &mut flat[start..index] {
            record.depth = record.depth.saturating_sub(1);
        }
        let mut cleared_clip = Vec::new();
        if let Some(bottom) = releases {
            flat[bottom].layer.clipped = false;
            cleared_clip.push(bottom);
        }
        flat.remove(index);
        // Removing the group shifts nothing below `index`, so both reports
        // already name the records' new addresses.
        let document = self.rebuilt_from_flat(flat)?;
        Ok(Some((
            document,
            UngroupReport {
                cleared_clip,
                landed,
            },
        )))
    }

    /// Removes every given record (a group with its whole subtree). Records
    /// SUBSUMED by another given record's subtree are dropped, and a call
    /// that would leave the document with nothing in it is refused
    /// (`doc_structure_sets::remove_layers`).
    pub fn remove_layers(&self, indices: &[usize]) -> Result<Option<Self>, String> {
        let flat = flatten(&self.layers);
        let roots = independent_roots(&flat, indices)?;
        let total: usize = roots.iter().map(|&i| subtree(&flat, i).len()).sum();
        if total >= flat.len() {
            return Err("a high-depth document must keep at least one record".into());
        }
        let mut flat = flat;
        // TOP-DOWN, so a removal above never renumbers the records still to
        // go.
        for &i in roots.iter().rev() {
            let range = subtree(&flat, i);
            flat.drain(range);
        }
        self.rebuilt_from_flat(flat).map(Some)
    }
}
