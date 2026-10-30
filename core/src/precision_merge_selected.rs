//! Stamp Selected and Merge Group at 16 bits, 32-bit float and on a CMYK
//! document (#547 phase 4c): the typed twins of `doc_merge_selected`, whose
//! rules they share, and both built on `precision_merge`'s pixel half
//! (`merged_record`: the members' union extent through the ordinary typed
//! compositor, or `cmyk_merge`'s frame render on a CMYK document).
//!
//! # Stamp Selected
//!
//! The members are the given records deduplicated: ONE group, or two or
//! more sharing one parent at one level (`merge_layers`' rule). Their pixels
//! — the members alone, composited as they project, over their union extent
//! — become ONE new raster record named by the caller: Normal, opacity 1,
//! visible, unclipped, with a new id, directly above the TOPMOST member's
//! subtree at its level (`insert_slot(top, Above)` with `clipped` forced off,
//! Stamp Visible's sequence). Every original is untouched, so it is
//! lock-exempt, like every insertion. A union with nothing visible refuses
//! with `precision_merge_extent`'s sentence.
//!
//! # Merge Group (⌘E with a group selected)
//!
//! The record must be a VISIBLE group ("Merge Group requires a visible group;
//! layer N is not one"), and its locks are asked with `EditKind::Merge`. Its
//! pixels are the RAW group (`raw_group_document`: opacity 1, Normal, no
//! mask, style or Blend If, unclipped, a Pass Through group made isolated)
//! over its union extent. The leaf takes the group's slot and level and
//! keeps its name, id, label, visibility, opacity, blend mode (Pass Through
//! becomes Normal), style (Fill Opacity included), Blend If, mask, clipping,
//! locks and link — so an ISOLATED group's composite is unchanged by the
//! merge. A comp entry bound to the group now binds the leaf, which keeps
//! the id and every appearance property the entry records, so the comps are
//! carried as they are (`merged_comps`' reset to Normal at opacity 1 would
//! contradict the kept appearance).
//!
//! # Plans
//!
//! [`PrecisionDocument::stamp_selected_scratch_bytes`] is
//! `merge_scratch_bytes` of the members, and
//! [`PrecisionDocument::merge_group_scratch_bytes`] that of the raw group
//! document's one root; `StructureEdit::scratch_bytes` adds the structure
//! term to both.

use crate::blend::BlendMode;
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::layer_insert::{InsertPosition, InsertSlot};
use crate::precision_document::PrecisionDocument;
use crate::precision_lock::lock_sentence;
use crate::precision_structure_ops::{flatten, parent, rebuild, subtree, FlatRecord};

impl PrecisionDocument {
    /// Stamp Selected's members (module doc), ascending; refuses by name.
    fn stamp_members(&self, layers: &[usize]) -> Result<Vec<usize>, String> {
        let mut members = layers.to_vec();
        members.sort_unstable();
        members.dedup();
        let flat = flatten(&self.layers);
        let Some(&last) = members.last() else {
            return Err("Stamp Selected requires at least one layer".into());
        };
        if last >= flat.len() {
            return Err("high-depth layer index is out of range".into());
        }
        if let [only] = members[..] {
            if flat[only].layer.children().is_none() {
                return Err(format!(
                    "Stamp Selected requires a group or at least two layers; layer {only} is not a group"
                ));
            }
            return Ok(members);
        }
        let depth = flat[members[0]].depth;
        let group = parent(&flat, members[0]);
        if members
            .iter()
            .any(|&i| flat[i].depth != depth || parent(&flat, i) != group)
        {
            return Err("Stamp Selected requires layers that share one parent".into());
        }
        Ok(members)
    }

    /// Stamp Selected (module doc).
    pub(crate) fn stamp_selected(
        &self,
        layers: &[usize],
        name: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let members = self.stamp_members(layers)?;
        let top = *members
            .last()
            .ok_or("Stamp Selected requires at least one layer")?;
        // The slot is resolved before anything renders, so a refused anchor
        // costs no projection (`stamp_visible`'s order).
        let slot = self.insert_slot(top, InsertPosition::Above)?;
        let frame = self.merge_frame(&members)?;
        self.preflight_raster_addition(frame.1, name)?;
        let leaf = self.merged_record(name, &members, limit, other, job)?;
        self.inserting_record(
            leaf,
            InsertSlot {
                clipped: false,
                ..slot
            },
        )
        .map(Some)
    }

    /// Stamp Selected's plan beside the structure term (module doc).
    pub(crate) fn stamp_selected_scratch_bytes(&self, layers: &[usize]) -> Result<u64, String> {
        let members = self.stamp_members(layers)?;
        self.merge_scratch_bytes(&members)
    }

    /// Merge Group's record, refused by name unless it is a visible group.
    fn merge_group_record(&self, layer: usize) -> Result<(), String> {
        let record = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        if record.children().is_none() || !record.visible() {
            return Err(format!(
                "Merge Group requires a visible group; layer {layer} is not one"
            ));
        }
        Ok(())
    }

    /// Merge Group (module doc).
    pub(crate) fn merge_group(
        &self,
        layer: usize,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.merge_group_record(layer)?;
        let blocking = self.lock_block(layer, EditKind::Merge);
        if blocking != 0 {
            return Err(lock_sentence(
                layer,
                self.blocking_owner(layer, blocking),
                blocking,
            ));
        }
        let group = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        let raw = self.raw_group_document(layer)?;
        let root = raw.layer_count() - 1;
        let mut leaf = raw.merged_record(group.name(), &[root], limit, other, job)?;
        leaf.id = group.id();
        leaf.label = group.label();
        leaf.visible = group.visible();
        leaf.opacity = group.opacity();
        leaf.blend_mode = if group.is_pass_through() || group.blend_mode == BlendMode::PassThrough {
            BlendMode::Normal
        } else {
            group.blend_mode
        };
        leaf.style = group.style.clone();
        leaf.blend_if = group.blend_if;
        leaf.mask = group.mask.clone();
        leaf.clipped = group.clipped();
        leaf.locks = group.locks();
        leaf.link = group.link();
        let flat = flatten(&self.layers);
        let depth = flat[layer].depth;
        let range = subtree(&flat, layer);
        let mut flat = flat;
        flat.splice(range, std::iter::once(FlatRecord { layer: leaf, depth }));
        // `rebuilt` carries the comps: an entry bound to the group now binds
        // the leaf, which keeps the group's id and every appearance property
        // a comp records, so no entry changes (module doc).
        self.rebuilt(
            self.width,
            self.height,
            rebuild(flat)?,
            self.channels.as_ref().clone(),
        )
        .map(Some)
    }

    /// Merge Group's plan beside the structure term (module doc).
    pub(crate) fn merge_group_scratch_bytes(&self, layer: usize) -> Result<u64, String> {
        self.merge_group_record(layer)?;
        let raw = self.raw_group_document(layer)?;
        raw.merge_scratch_bytes(&[raw.layer_count() - 1])
    }
}
