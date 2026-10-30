//! The structural OPERATIONS over a set of entries: the sibling reorder
//! (`arrange_layer`), Merge Layers, Merge Visible, Stamp Visible and Layer Via
//! Copy / Via Cut. The set duplicate and delete live in `doc_structure_sets`.
//!
//! `doc_group` owns the layout, the invariant and the compositor; `doc_group_ops`
//! the two ops that create and dissolve the shape (`group_layers` /
//! `ungroup_layer`) and the structural move every one of these builds on;
//! `doc_align` the ops that move a set around the canvas. Every op here ends in
//! [`doc_group::validated`], so a malformed result is a NO-OP rather than a
//! corrupt document — see `doc_group`'s module doc for why that gate is a
//! runtime check and not an assertion.
//!
//! # Why a set op is ONE call and never a host loop
//!
//! Every structural op RENUMBERS: deleting a subtree, duplicating one or
//! merging two shifts every index above it. A host that looped over its
//! selection would address the wrong entries after the first step, so each op
//! here takes the whole set and makes one document. Where an internal loop is
//! unavoidable it runs TOP-DOWN, because an edit above an index never moves
//! the entries below it.
//!
//! # The set's shape
//!
//! Two different rules, because two different things are being asked:
//!
//! * `duplicate_layers` and `remove_layers` (`doc_structure_sets`) take any
//!   set and drop the entries SUBSUMED by another given entry's subtree; that
//!   module's doc states the rule.
//! * `merge_layers` requires one shared parent, exactly as `group_layers`
//!   does: merging across levels has no defined slot for the result.

use crate::adjust::Adjustment;
use crate::comps_geometry::merged_comps;
use crate::doc::{BlendMode, DrDocument, Layer, LayerKind};
use crate::doc_group::{
    composite_level_into, level_range, materialized, parent, relocated, subtree, top_level, units,
    validated, Unit,
};
use crate::doc_lock::EditKind;
use crate::style_composite::{merge_extent, CompositeEnv};

/// Where a sibling reorder puts an entry within its own level. Mirrors
/// `DrArrange` in the C header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum Arrange {
    /// Above every sibling.
    Front = 0,
    /// One place up.
    Forward = 1,
    /// One place down.
    Backward = 2,
    /// Below every sibling.
    Back = 3,
}

impl Arrange {
    /// Maps a raw `DrArrange` value coming across the FFI.
    pub fn from_c(value: i32) -> Option<Self> {
        match value {
            0 => Some(Arrange::Front),
            1 => Some(Arrange::Forward),
            2 => Some(Arrange::Backward),
            3 => Some(Arrange::Back),
            _ => None,
        }
    }
}

impl DrDocument {
    /// Moves entry `idx` among its SIBLINGS — never into or out of a group,
    /// which is [`DrDocument::move_layer_to`]'s job. A group carries its whole
    /// subtree with it.
    ///
    /// `None` for an out-of-range index and when the entry is already where
    /// `how` would put it (the core's no-op rule), so Bring Forward on the
    /// topmost sibling is a refusal rather than an identical copy.
    pub fn arrange_layer(&self, idx: usize, how: Arrange) -> Option<Self> {
        let depth = self.layers.get(idx)?.depth;
        let level = units(&self.layers, level_range(&self.layers, idx), depth);
        let pos = level.iter().position(|u| u.idx == idx)?;
        let top = level.len().checked_sub(1)?;
        let target = match how {
            Arrange::Front | Arrange::Forward if pos == top => return None,
            Arrange::Front => top,
            Arrange::Forward => pos + 1,
            Arrange::Backward | Arrange::Back if pos == 0 => return None,
            Arrange::Backward => pos - 1,
            Arrange::Back => 0,
        };
        let sub = subtree(&self.layers, idx);
        // `relocated` inserts into the stack with `sub` ALREADY TAKEN OUT, so
        // the two directions index differently: moving UP lands above the
        // target sibling's whole subtree, and everything at or above `sub.end`
        // has shifted down by the subtree's length; moving DOWN lands at the
        // start of the target's subtree, which sits below `sub` and therefore
        // has not moved at all.
        let at = if target > pos {
            level[target].idx + 1 - sub.len()
        } else {
            level[target].start
        };
        relocated(self, sub, at, 0)
    }

    /// Merge Layers over a multi-selection: the given entries —
    /// which must all share ONE parent — become ONE raster entry at the lowest
    /// member's slot and depth, keeping that member's name, visibility,
    /// clipped flag, locks and link.
    ///
    /// The pixels come from the projection's OWN walk
    /// ([`doc_group::composite_level_into`]) over the members alone, so the
    /// clip runs, blend modes, opacities, masks and styles among them bake
    /// exactly as they composite; a member that is a GROUP is materialized
    /// first, so it merges as the single layer it renders to.
    /// [`DrDocument::merging_down`] is the two-adjacent-entries form that
    /// predates this and keeps its own extra rules (an invisible upper entry
    /// is simply removed, a hidden lower one refuses the merge); both drive
    /// the same compositing primitives, neither reimplements blending.
    ///
    /// `None` for fewer than two members, members that do not share one
    /// parent, an out-of-range or repeating index, a hidden LOWEST member
    /// (which would silently discard everything above it, the rule
    /// `merging_down` already applies), a LOWEST member whose locks refuse a
    /// merge (`doc_lock::EditKind::Merge` — the merged entry takes its slot
    /// and replaces its picture), or a union extent that is empty or over the
    /// pixel cap.
    ///
    /// A LAYER COMP bound to the lowest member follows the merge: its
    /// recorded offsets shift by the merged origin − that member's offset and
    /// its recorded appearance becomes the merged layer's own, through
    /// `comps_geometry::merged_comps` (the rule `merging_down` states).
    pub fn merge_layers(&self, idx: &[usize]) -> Option<Self> {
        let mut members = idx.to_vec();
        members.sort_unstable();
        members.dedup();
        if members.len() < 2 || *members.last()? >= self.layers.len() {
            return None;
        }
        let depth = self.layers[members[0]].depth;
        let group = parent(&self.layers, members[0]);
        if members
            .iter()
            .any(|&i| self.layers[i].depth != depth || parent(&self.layers, i) != group)
        {
            return None;
        }
        let lowest = &self.layers[members[0]];
        if !lowest.visible {
            return None;
        }
        // The DESTINATION's locks, asked before anything is built: the merge
        // drains every member's subtree, so this is the last point at which
        // the index still names the entry the lock belongs to (see
        // `doc_lock`).
        if self.lock_block(members[0], EditKind::Merge) != 0 {
            return None;
        }
        let name = lowest.name.clone();
        let clipped = lowest.clipped;
        let locks = lowest.locks;
        let link = lowest.link;
        // …and its colour LABEL, for the same reason as the identity below
        // (`doc_label`).
        let label = lowest.label;
        // …and its IDENTITY, for the reason `merge_visible` states: the
        // merged entry takes this member's slot, name, locks and link, so it
        // is that layer with new pixels (`doc_identity`).
        let identity = lowest.id;

        // The merged entry is visible by construction: the lowest member's
        // visibility is what it keeps, and a hidden lowest member was refused
        // two lines up.
        let mut merged = self.merged_layer(&members, &name)?;
        merged.id = identity;
        merged.clipped = clipped;
        merged.depth = depth;
        merged.locks = locks;
        merged.link = link;
        merged.label = label;

        let subs: Vec<_> = members.iter().map(|&i| subtree(&self.layers, i)).collect();
        let at = subs[0].start;
        let mut doc = self.clone();
        if let Some(comps) = merged_comps(
            &self.comps,
            self.comps_after_growth(members[0], merged.offset),
            identity,
        ) {
            doc.comps = comps;
        }
        // TOP-DOWN again: every removal is above the ones still to come, so
        // `at` — the lowest member's own slot — never moves.
        for sub in subs.iter().rev() {
            doc.layers.drain(sub.clone());
        }
        doc.layers.splice(at..at, std::iter::once(merged));
        validated(doc)
    }

    /// Merge Visible: every entry that CONTRIBUTES to the projection — its own
    /// `visible` true AND every ancestor's true — is replaced by ONE
    /// canvas-sized raster entry holding [`DrDocument::flattened`].
    ///
    /// "Contributes to the projection" is the single predicate for BOTH halves
    /// of the op, which is what makes it safe: a visible layer inside a hidden
    /// group contributes nothing — the walk skips the hidden group's whole
    /// subtree — so it is never merged away and survives untouched. Using
    /// "visible entry" for one half and its complement for the other would
    /// destroy that layer on a single keystroke. A GROUP is removed only when
    /// its whole subtree contributed; otherwise it survives to hold whatever
    /// did not.
    ///
    /// The merged entry lands at depth 0, at the slot where the bottom-most
    /// contributing entry's top-level ancestor began, and takes that
    /// bottom-most entry's NAME — the bottom visible layer is the layer the
    /// user is thinking of, rather than a container that may be about to
    /// disappear.
    ///
    /// `None` when fewer than two LEAF entries contribute: counting *entries*
    /// would count group rows, so a single layer inside one visible group
    /// would wrongly satisfy a "two entries" floor and merge itself into a
    /// copy of itself.
    ///
    /// A LAYER COMP bound to that bottom-most contributing leaf follows the
    /// merge: its recorded offsets shift by (0, 0) − the leaf's offset
    /// and its recorded appearance becomes the merged layer's own, through
    /// `comps_geometry::merged_comps`.
    pub fn merge_visible(&self) -> Option<Self> {
        let contributes = contributing(&self.layers);
        let leaves = self
            .layers
            .iter()
            .enumerate()
            .filter(|(i, l)| contributes[*i] && l.kind == LayerKind::Raster)
            .count();
        if leaves < 2 {
            return None;
        }
        // Removed = contributes AND its whole subtree contributes. A group
        // whose children are hidden contributes itself but must survive to
        // hold them.
        let removed: Vec<bool> = (0..self.layers.len())
            .map(|i| contributes[i] && subtree(&self.layers, i).all(|d| contributes[d]))
            .collect();
        // The SLOT comes from the bottom-most contributing entry, which may be
        // a group whose own children are hidden; the NAME comes from the
        // bottom-most contributing LEAF, which is the layer a person means by
        // "the bottom visible layer" and is guaranteed to exist by the floor
        // above.
        let bottom = contributes.iter().position(|&c| c)?;
        // The same entry gives the NAME and the IDENTITY: a merge keeps the
        // lowest merged entry's id, because the merged entry IS that layer
        // with new pixels (`doc_identity`).
        // Its colour LABEL travels with them (`doc_label`).
        let (kept, name, identity, label) = self
            .layers
            .iter()
            .enumerate()
            .find(|(i, l)| contributes[*i] && l.kind == LayerKind::Raster)
            .map(|(i, l)| (i, l.name.clone(), l.id, l.label))?;
        let mut ancestor = bottom;
        while let Some(above) = parent(&self.layers, ancestor) {
            ancestor = above;
        }
        let slot = subtree(&self.layers, ancestor).start;
        let at = (0..slot).filter(|&i| !removed[i]).count();

        let mut merged = Layer::new(self.flattened().ok()?, &name);
        merged.id = identity;
        merged.label = label;
        let mut doc = self.clone();
        if let Some(comps) = merged_comps(
            &self.comps,
            self.comps_after_growth(kept, merged.offset),
            identity,
        ) {
            doc.comps = comps;
        }
        doc.layers = self
            .layers
            .iter()
            .enumerate()
            .filter(|(i, _)| !removed[*i])
            .map(|(_, l)| l.clone())
            .collect();
        doc.layers.insert(at, merged);
        validated(doc)
    }

    /// Stamp Visible: the projection as a NEW canvas-sized raster entry
    /// immediately above `above_idx`'s subtree, at that entry's depth.
    /// Nothing else in the stack changes.
    ///
    /// One line on purpose: [`DrDocument::adding_image_layer`] already is
    /// "insert these pixels above that entry's subtree", so there is no second
    /// definition of where a new entry lands. `None` on an out-of-range index,
    /// and `None` when NOTHING contributes to the projection: the stamp would
    /// be a fully transparent layer, which is an undo step and a dirty
    /// document in exchange for nothing. The floor is one contributing LEAF,
    /// not [`DrDocument::merge_visible`]'s two — stamping a single visible
    /// layer is a copy of it, which is a thing a person means.
    pub fn stamp_visible(&self, above_idx: usize, name: &str) -> Option<Self> {
        let contributes = contributing(&self.layers);
        if !self
            .layers
            .iter()
            .enumerate()
            .any(|(i, l)| contributes[i] && l.kind == LayerKind::Raster)
        {
            return None;
        }
        let pixels = self.flattened().ok()?;
        self.adding_image_layer(above_idx, pixels, name)
    }

    /// Layer Via Copy (`cut` false) and Layer Via Cut (`cut` true): the source
    /// entry's pixels weighted by a canvas-sized coverage mask (or the whole
    /// layer when `mask` is `None`) become a NEW raster entry immediately
    /// above the source, at the same depth and the same canvas position. With
    /// `cut`, the same coverage is then cleared from the source through
    /// [`DrDocument::clear_selection`], under the source's locks.
    ///
    /// This op RASTERIZES: the new entry keeps neither the source's meta nor
    /// its style, because neither would describe its pixels any more — a host
    /// whose target is a described layer (text, shape) wants
    /// [`DrDocument::duplicating_layer`] instead, and the UI's Layer Via Copy
    /// routes those cases there. It keeps no MASK either, for the same reason
    /// and one more: the copy is the source's raw pixels weighted by the
    /// coverage, so a source whose mask hides part of it copies that part too
    /// — the selection is what was asked for, and a mask is a compositing
    /// property rather than a description of the pixels.
    ///
    /// `None` on an out-of-range index, a GROUP, an ADJUSTMENT layer (it has
    /// no pixels of its own to copy), a mask that is not canvas-sized, and a
    /// coverage that selects nothing inside the layer — copying an empty
    /// layer is never what was meant.
    pub fn layer_via(
        &self,
        idx: usize,
        mask: Option<&[u8]>,
        cut: bool,
        name: &str,
    ) -> Option<Self> {
        let layer = self.raster_layer(idx)?;
        if layer
            .meta
            .as_deref()
            .and_then(Adjustment::from_meta)
            .is_some()
        {
            return None;
        }
        let canvas_px = (self.width as usize).checked_mul(self.height as usize)?;
        if mask.is_some_and(|m| m.len() != canvas_px) {
            return None;
        }
        let (lw, lh) = layer.pixels.dimensions();
        let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        let mut pixels = layer.pixels.to_contiguous().ok()?;
        let raw: &mut [u8] = &mut pixels;
        let mut any = false;
        for ly in 0..i64::from(lh) {
            for lx in 0..i64::from(lw) {
                let di = ((ly * i64::from(lw) + lx) * 4) as usize;
                let coverage = match mask {
                    None => 255u32,
                    Some(mask) => {
                        let cx = lx + off_x;
                        let cy = ly + off_y;
                        if cx < 0
                            || cy < 0
                            || cx >= i64::from(self.width)
                            || cy >= i64::from(self.height)
                        {
                            // A selection never reaches past the canvas, so a
                            // layer pixel hanging off it is never selected.
                            0
                        } else {
                            u32::from(mask[(cy * i64::from(self.width) + cx) as usize])
                        }
                    }
                };
                // round(alpha * coverage / 255) in integers; the + 127 is the
                // rounding term `clear_selection` uses for its mirror of this.
                let alpha = (u32::from(raw[di + 3]) * coverage + 127) / 255;
                raw[di + 3] = alpha as u8;
                any |= alpha > 0;
            }
        }
        if !any {
            return None;
        }
        // The cut runs on the SOURCE first, so the copied pixels are read
        // before anything is taken away, and the source's locks gate it.
        let base = match (cut, mask) {
            (false, _) => self.clone(),
            (true, Some(mask)) => self.clear_selection(idx, mask)?,
            // "The whole layer" with a cut means exactly that: full coverage.
            (true, None) => self.clear_selection(idx, &vec![255u8; canvas_px])?,
        };
        let at = subtree(&self.layers, idx).end;
        let mut doc = base.adding_image_layer(idx, pixels, name)?;
        // `adding_image_layer` places a new entry at the canvas origin; Via
        // Copy keeps the source's own rect, so the copy sits exactly on top of
        // what it came from.
        doc.layers[at].offset = layer.offset;
        validated(doc)
    }

    /// The given entries composited bottom-first into ONE raster layer over
    /// their union extent, through the projection's own level walk. Each
    /// member is materialized first (a raster entry cloned, a group rendered),
    /// so the walk sees a flat level and re-derives the clip runs among them
    /// exactly as it would in the document. Each member keeps its document
    /// `visible` flag, so D6's "visible clipped members" answer matches the
    /// screen (12E-3b decision (ix)).
    ///
    /// `None` when NOTHING VISIBLE materialized (so there is no union extent
    /// at all — a set whose only visible member is an empty group), and when
    /// the union extent is empty, does not fit the i32 offset range, or
    /// exceeds the pixel cap — the same refusals
    /// [`DrDocument::merging_down`] makes.
    pub(crate) fn merged_layer(&self, members: &[usize], name: &str) -> Option<Layer> {
        let colors = self.style_colors();
        // The evaluation policy's transfer and its refusal walk
        // (`doc_evaluation`): a merge is a projection over the named entries.
        let transfer = self.transfer().ok()?;
        let env = self.composite_env(colors.as_ref(), transfer);
        let budget = crate::doc_group::RenderBudget::default();
        let level: Vec<Layer> = members
            .iter()
            .map(|&i| {
                let Some(mut layer) = materialized(&self.layers, i, env, &budget)? else {
                    // A group that renders nothing on the canvas stays an
                    // empty GROUP entry at depth 0 with the document's own
                    // `visible` flag (12E-3b decision (ix): D6 reads the
                    // DOCUMENT's flags). As a clipped member the flat
                    // walk then resolves it to exactly the invisible Group
                    // placeholder the projection's own walk does, and still
                    // counts it as a visible member; as a base or baseless
                    // entry it has no extent and contributes nothing. The
                    // group-only blend maps to Normal, as `rendered_group`
                    // does, so the entry takes that no-extent early exit
                    // rather than the gated pass-through copy — a cheapness
                    // and consistency choice, not a correctness one.
                    let entry = &self.layers[i];
                    return Ok(Layer {
                        depth: 0,
                        blend: if entry.blend.is_group_only() {
                            BlendMode::Normal
                        } else {
                            entry.blend
                        },
                        ..entry.clone()
                    });
                };
                layer.depth = 0;
                layer.kind = LayerKind::Raster;
                // `rendered_group` hands back a layer marked visible, because
                // the projection filters BEFORE it calls in. Here the filtering
                // happens after, so the entry's own flag has to be put back —
                // otherwise merging a set containing a hidden group would bake
                // that group in.
                layer.visible &= self.layers[i].visible;
                Ok(layer)
            })
            .collect::<Result<_, String>>()
            .ok()?;
        // Seeded from None rather than from MAX/MIN sentinels — the shape
        // `doc_group::extent_of_level` uses — because "no member
        // materialized" is REACHABLE: `merge_layers` guarantees only that the
        // lowest member is VISIBLE, and a visible EMPTY group renders to
        // nothing and stays in the level as an empty group entry. With
        // sentinels the union then underflowed (`x1 - x0`), which panics in a
        // debug build and in release yields a 1x1 layer at (-1, -1) that
        // REPLACES the members — silent data loss. `None` here is the same
        // refusal `merging_down` makes when its upper entry renders to
        // nothing.
        let mut union: Option<(i64, i64, i64, i64)> = None;
        // Only MATERIALIZED members count: an empty group entry keeps its
        // document `visible` flag for D6 but has no extent.
        for layer in level
            .iter()
            .filter(|l| l.visible && l.kind == LayerKind::Raster)
        {
            let rect = merge_extent(layer, env);
            union = Some(match union {
                None => rect,
                Some(u) => (
                    u.0.min(rect.0),
                    u.1.min(rect.1),
                    u.2.max(rect.2),
                    u.3.max(rect.3),
                ),
            });
        }
        let (x0, y0, x1, y1) = union?;
        let (uw, uh) = ((x1 - x0).max(0) as u64, (y1 - y0).max(0) as u64);
        if uw == 0 || uh == 0 || uw.checked_mul(uh)? > crate::doc::MAX_PIXELS {
            return None;
        }
        if x0 < i64::from(i32::MIN) || y0 < i64::from(i32::MIN) {
            return None;
        }
        let origin = (x0 as i32, y0 as i32);
        // The REAL backdrop a knockout member would have read from the
        // document's own projection, windowed onto this private accumulator
        // (`style_knockout_merge`). Both planes absent — today's statements
        // exactly — for a set with no knockout member, and for the one excluded
        // set (a lowest member clipped to a base outside the set).
        let backdrop = self
            .merge_backdrop(members, origin, (uw as u32, uh as u32), &budget)
            .ok()?;
        let env = CompositeEnv {
            knockout: backdrop.backdrop(),
            ..env
        };
        let mut acc = vec![[0.0f32; 4]; (uw * uh) as usize];
        composite_level_into(
            &mut acc,
            uw as u32,
            uh as u32,
            origin,
            &level,
            &top_level(&level),
            env,
            0,
            &budget,
        )
        .ok()?;
        let mut merged = Layer::new(
            crate::doc_evaluation::encode_projection(&acc, uw as u32, uh as u32, transfer),
            name,
        );
        merged.offset = origin;
        Some(merged)
    }
}

/// The given indices with every entry SUBSUMED by another given entry's
/// subtree dropped, ascending. `None` for an empty set and for any index past
/// the end — a caller that named a layer that is not there has miscounted, and
/// silently doing less than it asked is worse than refusing.
pub(crate) fn independent_roots(layers: &[Layer], idx: &[usize]) -> Option<Vec<usize>> {
    let mut sorted = idx.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.is_empty() || *sorted.last()? >= layers.len() {
        return None;
    }
    // A subtree ENDS with its own entry, so an ancestor always has the HIGHER
    // index: walking down from the top, an entry is subsumed exactly when it
    // falls inside a root already kept.
    let mut roots: Vec<usize> = Vec::new();
    for &i in sorted.iter().rev() {
        if roots.iter().any(|&kept| subtree(layers, kept).contains(&i)) {
            continue;
        }
        roots.push(i);
    }
    roots.reverse();
    Some(roots)
}

/// Which entries CONTRIBUTE to the projection: their own `visible` true and
/// every ancestor's true. The single predicate `merge_visible` uses for both
/// halves of its work.
fn contributing(layers: &[Layer]) -> Vec<bool> {
    let mut out = vec![false; layers.len()];
    mark_level(layers, &top_level(layers), 0, &mut out);
    out
}

fn mark_level(layers: &[Layer], level: &[Unit], depth: u16, out: &mut [bool]) {
    for unit in level {
        let entry = &layers[unit.idx];
        if !entry.visible {
            continue; // an invisible entry hides its whole subtree
        }
        out[unit.idx] = true;
        if entry.kind == LayerKind::Group {
            let children = units(layers, unit.start..unit.idx, depth + 1);
            mark_level(layers, &children, depth + 1, out);
        }
    }
}
