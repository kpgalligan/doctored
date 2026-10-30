//! Stamp Selected and Merge Group at 8 bits (#547 phase 4c), the byte half
//! of `precision_merge_selected`, whose module doc is the contract. Both take
//! their pixels from `doc_structure::merged_layer` — the members alone
//! through the projection's own level walk over their union extent — so
//! nothing here composites.
//!
//! * [`DrDocument::stamp_selected`]: ONE group, or two or more entries
//!   sharing one parent, become ONE new raster entry (Normal, opacity 1,
//!   visible, unclipped, a new id) directly above the topmost member's
//!   subtree at its depth; every original is untouched.
//! * [`DrDocument::merge_group`]: a visible group, its locks admitting a
//!   merge, is replaced by ONE raster entry holding its RAW projection (the
//!   group at opacity 1, Normal, visible, no mask or style, unclipped, a Pass
//!   Through group isolated), which keeps the group's slot, depth, name, id,
//!   label, opacity, blend mode (Pass Through becomes Normal), style, mask,
//!   clipping, locks and link — so an isolated group's composite is
//!   unchanged. The comps are carried as they are, as at high depth.

use crate::doc::{BlendMode, DrDocument, LayerKind};
use crate::doc_group::{parent, subtree, validated};
use crate::doc_lock::EditKind;

impl DrDocument {
    /// Stamp Selected (module doc). `None` for an empty or out-of-range set,
    /// one entry that is not a group, two or more that do not share one
    /// parent, a union with nothing visible, and a malformed result.
    pub fn stamp_selected(&self, idx: &[usize], name: &str) -> Option<Self> {
        let mut members = idx.to_vec();
        members.sort_unstable();
        members.dedup();
        let top = *members.last()?;
        if top >= self.layers.len() {
            return None;
        }
        if members.len() == 1 && self.layers[top].kind != LayerKind::Group {
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
        let mut stamp = self.merged_layer(&members, name)?;
        stamp.depth = depth;
        stamp.clipped = false;
        let at = subtree(&self.layers, top).end;
        let mut doc = self.clone();
        doc.layers.insert(at, stamp);
        validated(doc)
    }

    /// Merge Group (module doc). `None` for an out-of-range index, an entry
    /// that is not a visible group, locks that refuse a merge, a group with
    /// nothing visible, and a malformed result.
    pub fn merge_group(&self, idx: usize) -> Option<Self> {
        let entry = self.layers.get(idx)?;
        if entry.kind != LayerKind::Group || !entry.visible {
            return None;
        }
        if self.lock_block(idx, EditKind::Merge) != 0 {
            return None;
        }
        // The RAW group: its own compositing properties set aside, so the
        // walk renders exactly its children (`group_projection`'s rule).
        let mut raw = self.clone();
        {
            let group = &mut raw.layers[idx];
            group.opacity = 1.0;
            group.blend = BlendMode::Normal;
            group.mask = None;
            group.style = None;
            group.clipped = false;
            group.visible = true;
        }
        let mut leaf = raw.merged_layer(&[idx], &entry.name)?;
        leaf.id = entry.id;
        leaf.label = entry.label;
        leaf.visible = entry.visible;
        leaf.opacity = entry.opacity;
        leaf.blend = if entry.blend == BlendMode::PassThrough {
            BlendMode::Normal
        } else {
            entry.blend
        };
        leaf.style = entry.style.clone();
        leaf.mask = entry.mask.clone();
        leaf.mask_enabled = entry.mask_enabled;
        leaf.mask_properties = entry.mask_properties.clone();
        leaf.clipped = entry.clipped;
        leaf.locks = entry.locks;
        leaf.link = entry.link;
        leaf.depth = entry.depth;
        let range = subtree(&self.layers, idx);
        let mut doc = self.clone();
        doc.layers.splice(range, std::iter::once(leaf));
        validated(doc)
    }
}
