//! Small immutable typed layer properties, the five pure STRUCTURE edits
//! (`precision_structure_ops`), the three MERGES (`precision_merge`), STAMP
//! VISIBLE and FLATTEN (`precision_stamp_flatten`), STAMP SELECTED and MERGE
//! GROUP (`precision_merge_selected`) and the LOCK/LINK edits
//! (`precision_lock`). Pixels, masks and channel storage remain shared;
//! bounded ancestor metadata is admitted before cloning.
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::job_control::JobContext;
use crate::precision_document::{scalar_and_name, PrecisionDocument};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};

/// Where a sibling reorder puts a record within its own level — the typed
/// spelling of `doc_structure::Arrange` and of `DrArrange` in the C header.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ArrangeHow {
    /// Above every sibling.
    Front,
    /// One place up.
    Forward,
    /// One place down.
    Backward,
    /// Below every sibling.
    Back,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StructureEdit {
    RenameLayer {
        layer: usize,
        name: String,
    },
    LayerVisible {
        layer: usize,
        visible: bool,
    },
    LayerClipped {
        layer: usize,
        clipped: bool,
    },
    /// The colour label, 0..=7 (`doc_label`). Lock-exempt like the rename.
    LayerLabel {
        layer: usize,
        label: u8,
    },
    /// A group's disclosure, the byte `Layer::open` twin — panel bookkeeping
    /// that changes no pixel. Groups only; lock-exempt like the rename.
    LayerOpen {
        layer: usize,
        open: bool,
    },
    /// The structural move: `from`'s whole subtree lands at `to` — an index
    /// into the record list with that subtree ALREADY taken out — at nesting
    /// level `depth`.
    MoveLayerTo {
        from: usize,
        to: usize,
        depth: usize,
    },
    /// The sibling reorder; never leaves the record's own level.
    ArrangeLayer {
        layer: usize,
        how: ArrangeHow,
    },
    /// Subset grouping: any set sharing ONE parent.
    GroupLayers {
        layers: Vec<usize>,
        name: String,
        #[serde(default = "default_true")]
        pass_through: bool,
    },
    /// Dissolves one group; its children take its level and its slot.
    UngroupLayer {
        layer: usize,
    },
    /// The set delete; records subsumed by another given subtree are dropped.
    RemoveLayers {
        layers: Vec<usize>,
    },
    /// Layer ▸ Duplicate to New Document: the given records (subtrees
    /// included, subsumed records dropped) as a NEW document
    /// (`doc_duplicate_document`). The source is never changed.
    DuplicateToNewDocument {
        layers: Vec<usize>,
    },
    /// Merge Layers: the given records, sharing ONE parent, become one
    /// union-extent raster record at the lowest member's slot
    /// (`precision_merge`).
    MergeLayers {
        layers: Vec<usize>,
    },
    /// Merge Down: `layer` into its previous SIBLING.
    MergeDown {
        layer: usize,
    },
    /// Merge Visible: every contributing record into one. An EMPTY STRUCT
    /// variant, not a unit one: "Empty struct variant preserves
    /// deny_unknown_fields" (`raw_local_coverage.rs`) — an internally tagged
    /// serde UNIT variant silently discards extra keys.
    MergeVisible {},
    /// The four `doc_lock::LOCK_*` bits on one record; reserved bits are
    /// masked off (`precision_lock`).
    LayerLock {
        layer: usize,
        locks: u32,
    },
    /// Links the given records under the smallest unused link id.
    LinkLayers {
        layers: Vec<usize>,
    },
    /// Unlinks the given records.
    UnlinkLayers {
        layers: Vec<usize>,
    },
    /// A NEW transparent canvas-sized leaf at the slot `position` names
    /// relative to `anchor` — the ONE slot rule (`layer_insert`), through
    /// `precision_insert::add_layer_at`. Insertion is lock-EXEMPT, like every
    /// other structure op here.
    AddLayer {
        name: String,
        anchor: usize,
        position: crate::layer_insert::InsertPosition,
    },
    /// Stamp Visible: the projection as ONE new canvas-sized leaf directly
    /// above `anchor`'s subtree, leaving every existing record alone
    /// (`precision_stamp_flatten`). Lock-exempt, like every other insertion here.
    StampVisible {
        name: String,
        anchor: usize,
    },
    /// Flatten Image: every record replaced by ONE leaf named "Background"
    /// holding the projection (`precision_stamp_flatten`). An EMPTY STRUCT variant,
    /// not a unit one: "Empty struct variant preserves deny_unknown_fields"
    /// (`raw_local_coverage.rs`) — an internally tagged serde UNIT variant
    /// silently discards extra keys.
    Flatten {},
    /// Stamp Selected: ONE group, or two or more records sharing one parent,
    /// as ONE new union-extent leaf directly above the topmost member's
    /// subtree, every original untouched (`precision_merge_selected`).
    /// Lock-exempt, like every insertion.
    StampSelected {
        layers: Vec<usize>,
        name: String,
    },
    /// Merge Group: a visible group replaced by ONE leaf holding its raw
    /// projection, keeping the group's slot and properties
    /// (`precision_merge_selected`).
    MergeGroup {
        layer: usize,
    },
}
impl StructureEdit {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid high-depth layer property: {e}"))
    }
    /// Validates every index the request names against the document and
    /// answers the extra name bytes the edit carries.
    fn validated_extra(&self, document: &PrecisionDocument) -> Result<u64, String> {
        let present = |index: usize| {
            document
                .layer(index)
                .ok_or_else(|| "high-depth layer index is out of range".to_string())
        };
        match self {
            Self::RenameLayer { layer, name } => {
                let old = present(*layer)?;
                scalar_and_name(name, old.opacity())?;
                Ok(name.len() as u64)
            }
            Self::LayerLabel { layer, label } => {
                present(*layer)?;
                if *label > crate::doc_label::MAX_LABEL {
                    return Err("high-depth layer label is out of range".into());
                }
                Ok(0)
            }
            Self::LayerOpen { layer, .. } => {
                if present(*layer)?.children().is_none() {
                    return Err("high-depth layer open state applies to groups only".into());
                }
                Ok(0)
            }
            Self::LayerVisible { layer, .. }
            | Self::LayerClipped { layer, .. }
            | Self::ArrangeLayer { layer, .. }
            | Self::UngroupLayer { layer } => {
                present(*layer)?;
                Ok(0)
            }
            Self::MoveLayerTo { from, to, .. } => {
                present(*from)?;
                present(*to)?;
                Ok(0)
            }
            Self::GroupLayers { layers, name, .. } => {
                if layers.is_empty() {
                    return Err("high-depth grouping requires at least one layer".into());
                }
                for &index in layers {
                    present(index)?;
                }
                scalar_and_name(name, 1.0)?;
                Ok(name.len() as u64)
            }
            Self::RemoveLayers { layers } => {
                if layers.is_empty() {
                    return Err("high-depth delete requires at least one layer".into());
                }
                for &index in layers {
                    present(index)?;
                }
                Ok(0)
            }
            Self::DuplicateToNewDocument { layers } => {
                if layers.is_empty() {
                    return Err("high-depth duplicate requires at least one layer".into());
                }
                for &index in layers {
                    present(index)?;
                }
                Ok(0)
            }
            Self::MergeLayers { layers } => {
                if layers.len() < 2 {
                    return Err("high-depth merge requires at least two layers".into());
                }
                for &index in layers {
                    present(index)?;
                }
                Ok(0)
            }
            Self::MergeDown { layer } | Self::MergeGroup { layer } => {
                present(*layer)?;
                Ok(0)
            }
            Self::StampSelected { layers, name } => {
                if layers.is_empty() {
                    return Err("Stamp Selected requires at least one layer".into());
                }
                for &index in layers {
                    present(index)?;
                }
                scalar_and_name(name, 1.0)?;
                Ok(name.len() as u64)
            }
            Self::MergeVisible {} | Self::Flatten {} => Ok(0),
            Self::StampVisible { name, anchor } => {
                present(*anchor)?;
                scalar_and_name(name, 1.0)?;
                Ok(name.len() as u64)
            }
            Self::LayerLock { layer, .. } => {
                present(*layer)?;
                Ok(0)
            }
            Self::LinkLayers { layers } => {
                if layers.len() < 2 {
                    return Err("high-depth linking requires at least two layers".into());
                }
                for &index in layers {
                    present(index)?;
                }
                Ok(0)
            }
            Self::UnlinkLayers { layers } => {
                if layers.is_empty() {
                    return Err("high-depth unlinking requires at least one layer".into());
                }
                for &index in layers {
                    present(index)?;
                }
                Ok(0)
            }
            Self::AddLayer { name, anchor, .. } => {
                present(*anchor)?;
                scalar_and_name(name, 1.0)?;
                Ok(name.len() as u64)
            }
        }
    }
    pub(crate) fn scratch_bytes(&self, document: &PrecisionDocument) -> Result<u64, String> {
        let name = self.validated_extra(document)?;
        // Rebuilding an ancestor currently copies its name. All names are a
        // conservative superset of that path, independent of tree depth — and
        // a STRUCTURAL edit rebuilds every level it touches, so the same
        // superset bounds it too.
        let ancestor_names: u64 = (0..document.layer_count())
            .filter_map(|i| document.layer(i))
            .map(|layer| layer.name().len() as u64)
            .sum();
        let structure = 4 * BOOKKEEPING_BYTES
            + (name + ancestor_names) * 2
            + document.layer_count() as u64 * 4096
            + document.channels().len() as u64 * 1024;
        // A MERGE additionally renders: the members' own document projection
        // plus the leaf it retains — union-extent, or canvas-sized for Merge
        // Visible (`precision_merge`). Merge Visible's members are the whole
        // document, which an empty list names; Merge DOWN's are the previous sibling and the named record,
        // both of them, which `merge_down_scratch_bytes` resolves.
        // An ADD additionally allocates ONE canvas-sized transparent plane: 8
        // bytes a pixel at u16 (four u16 components), 16 at float. A CMYK
        // document's bare paper is at most four shared tiles
        // (`CmykImage::filled`), so its charge is the host's display
        // preparation of the result (`cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL`,
        // measured as `new-layer-16`), above the plane's own 10 B/px.
        let pixels = match self {
            // A CMYK document flattens its ink onto paper, admitted by its
            // own plan (`cmyk_convert::cmyk_flatten_image`).
            Self::Flatten {} if document.is_cmyk() => document.cmyk_flatten_scratch_bytes(),
            Self::MergeLayers { layers } => document.merge_scratch_bytes(layers)?,
            Self::MergeDown { layer } => document.merge_down_scratch_bytes(*layer)?,
            // Stamp Selected renders its members as Merge Layers does; Merge
            // Group renders the raw group document's one root
            // (`precision_merge_selected`).
            Self::StampSelected { layers, .. } => document.stamp_selected_scratch_bytes(layers)?,
            Self::MergeGroup { layer } => document.merge_group_scratch_bytes(*layer)?,
            // STAMP and FLATTEN render the document's own projection too,
            // and each retains one canvas-sized leaf, so the charge is Merge
            // Visible's exactly (`precision_stamp_flatten`, rule 8).
            Self::MergeVisible {} | Self::StampVisible { .. } | Self::Flatten {} => {
                document.merge_scratch_bytes(&[])?
            }
            Self::AddLayer { .. } => {
                let (width, height) = document.dimensions();
                u64::from(width)
                    .checked_mul(u64::from(height))
                    .and_then(|n| {
                        n.checked_mul(if document.is_float() {
                            16
                        } else if document.is_cmyk() {
                            crate::cmyk_image::CMYK_BYTES_PER_PIXEL
                                .max(crate::cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL)
                        } else {
                            8
                        })
                    })
                    .ok_or_else(|| "high-depth structural byte calculation overflow".to_string())?
            }
            _ => 0,
        };
        structure
            .checked_add(pixels)
            .ok_or_else(|| "high-depth structural byte calculation overflow".to_string())
    }

    /// What a structural edit changed BEYOND the nesting, for the plan reply:
    /// grouping's `{group, cleared_clip, reordered}` and ungrouping's
    /// `{cleared_clip, landed}`. `None` for every other edit, and for a
    /// structural one whose result is the document it was given.
    pub(crate) fn report(&self, document: &PrecisionDocument) -> Result<Option<Value>, String> {
        Ok(match self {
            Self::GroupLayers {
                layers,
                name,
                pass_through,
            } => document
                .group_layers(layers, name, *pass_through)?
                .map(|(_, report)| {
                    json!({"group": report.group, "cleared_clip": report.cleared_clip,
                           "reordered": report.reordered})
                }),
            Self::UngroupLayer { layer } => document.ungroup_layer(*layer)?.map(
                |(_, report)| json!({"cleared_clip": report.cleared_clip, "landed": report.landed}),
            ),
            _ => None,
        })
    }

    pub(crate) fn apply(
        &self,
        document: &PrecisionDocument,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let scratch = self.scratch_bytes(document)?;
        admit(document.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        // The five STRUCTURE edits rebuild the record tree through
        // `precision_structure_ops`; the four property edits replace one
        // record in place.
        let result = match self {
            Self::MoveLayerTo { from, to, depth } => document.move_layer_to(*from, *to, *depth)?,
            Self::ArrangeLayer { layer, how } => document.arrange_layer(*layer, *how)?,
            Self::GroupLayers {
                layers,
                name,
                pass_through,
            } => document
                .group_layers(layers, name, *pass_through)?
                .map(|(next, _)| next),
            Self::UngroupLayer { layer } => document.ungroup_layer(*layer)?.map(|(next, _)| next),
            Self::RemoveLayers { layers } => document.remove_layers(layers)?,
            Self::DuplicateToNewDocument { layers } => {
                Some(document.duplicate_to_new_document(layers)?)
            }
            Self::RenameLayer { layer, name } => {
                self.replacing(document, *layer, |next| next.name = Arc::new(name.clone()))?
            }
            Self::LayerVisible { layer, visible } => {
                self.replacing(document, *layer, |next| next.visible = *visible)?
            }
            Self::LayerClipped { layer, clipped } => {
                self.replacing(document, *layer, |next| next.clipped = *clipped)?
            }
            Self::LayerLabel { layer, label } => {
                self.replacing(document, *layer, |next| next.label = *label)?
            }
            Self::LayerOpen { layer, open } => {
                self.replacing(document, *layer, |next| next.open = *open)?
            }
            Self::MergeLayers { layers } => document.merge_layers(layers, limit, other, job)?,
            Self::MergeDown { layer } => document.merge_down(*layer, limit, other, job)?,
            Self::MergeVisible {} => document.merge_visible(limit, other, job)?,
            Self::StampVisible { name, anchor } => {
                document.stamp_visible(name, *anchor, limit, other, job)?
            }
            Self::Flatten {} => document.flatten_image(limit, other, job)?,
            Self::StampSelected { layers, name } => {
                document.stamp_selected(layers, name, limit, other, job)?
            }
            Self::MergeGroup { layer } => document.merge_group(*layer, limit, other, job)?,
            Self::LayerLock { layer, locks } => document.with_layer_locks(*layer, *locks)?,
            Self::LinkLayers { layers } => document.link_layers(layers)?,
            Self::UnlinkLayers { layers } => document.unlink_layers(layers)?,
            Self::AddLayer {
                name,
                anchor,
                position,
            } => document.add_layer_at(name, *anchor, *position)?,
        };
        job.check()?;
        Ok(result)
    }

    /// The property path: an edit that changes nothing answers `None` (the
    /// core's purity rule), otherwise one record is replaced.
    fn replacing(
        &self,
        document: &PrecisionDocument,
        index: usize,
        change: impl FnOnce(&mut crate::precision_document::PrecisionDocumentLayer),
    ) -> Result<Option<PrecisionDocument>, String> {
        let old = document
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let same = match self {
            Self::RenameLayer { name, .. } => name == old.name(),
            Self::LayerVisible { visible, .. } => *visible == old.visible(),
            Self::LayerClipped { clipped, .. } => *clipped == old.clipped(),
            Self::LayerLabel { label, .. } => *label == old.label(),
            Self::LayerOpen { open, .. } => *open == old.open(),
            _ => false,
        };
        if same {
            return Ok(None);
        }
        let mut layer = old.clone();
        change(&mut layer);
        document.replacing_layer(index, layer)
    }
}
