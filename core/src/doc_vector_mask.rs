//! VECTOR MASKS at 8 bits — a layer mask whose coverage is the fill of
//! a Bézier shape, kept editable. The typed twin is `precision_vector_mask`.
//!
//! **A vector mask is a BAKED ordinary mask.** The compositor, the style
//! cache, the regional renderers and every exporter see only the layer's
//! ordinary mask slot (`Layer::mask`, `mask_enabled`, `mask_properties`);
//! nothing in them knows a shape exists. What makes the mask a vector mask is
//! a [`VectorMaskRecord`] in `DocPaths::vector_masks`, bound to the layer by
//! id: the shape, its `enabled` and `linked` flags, the user's own RASTER
//! mask when the layer has one beside the shape ([`RasterPart`]), and the
//! plane the record put in the slot.
//!
//! **The slot** is `raster effective coverage × path coverage`
//! ([`DrDocument::baked_slot`]), where the raster factor is 1 when there is
//! no raster part or it is disabled, and the path factor is 1 when the vector
//! mask is disabled or its shape is empty (Reveal All):
//!
//! * the path factor on — a plane over [`bake_rect`] (the shape's pixel box
//!   inside the layer's pixel rect; a group's is the canvas), outside
//!   coverage 0, density 1, feather 0, `linked` from the record. When the
//!   raster part is enabled, unfeathered, at density 1 and outside coverage 0
//!   the plane takes the RASTER part's rectangle instead, so a host that read
//!   the slot's geometry and writes a mask edit back addresses the same
//!   rectangle the edit lands in;
//! * the path factor off, a raster part — the raster part put back VERBATIM
//!   (its plane, properties and enabled flag);
//! * the vector mask disabled, no raster part — the path plane, kept in the
//!   slot DISABLED: compositing ignores it and it still travels with the
//!   layer, which is what keeps a disabled shape attached through a move;
//! * an empty shape, no raster part — no mask at all.
//!
//! **Live or dead.** A record is LIVE while its layer exists and the layer's
//! slot plane is pointer-equal to the plane the record baked (both absent
//! counts). Every op that knows about vector masks rebakes and stores the new
//! plane; every op that does not either leaves the plane alone (the record
//! stays live) or replaces it (the record is dead and the layer simply keeps
//! the raster mask it has). A slot OFFSET that differs from the baked offset
//! is a translation — a move, a crop, a canvas resize — and is read as one:
//! [`DrDocument::live_vector_mask`] shifts the shape and the raster part by
//! the delta. A dead record is pruned by the next vector-mask or path write
//! and is never saved.
//!
//! **Mask edits** (`doc_lock::under_locks` with `EditKind::Mask`) on a layer
//! with a live record run on [`DrDocument::exposing_vector_mask`]'s document,
//! whose slot is the raster part alone, and
//! [`DrDocument::rebaking_vector_mask`] takes the result's slot as the new
//! raster part: painting, inverting, adding, deleting and re-describing the
//! layer mask all edit the raster part and leave the shape alone. APPLYING
//! the mask is not wrapped: it bakes the slot — both factors — into the
//! layer's alpha and removes both.
//!
//! **Following geometry.** After a free transform of a linked mask, a quarter
//! turn or flip of the canvas and Image Size, a record with NO raster part
//! has its shape mapped by the op's own map and is rebaked
//! ([`DrDocument::following_vector_masks`]), so the mask stays a sharp shape
//! instead of a resampled plane. A record WITH a raster part dies there: the
//! layer keeps the resampled product. Depth conversion re-sets raster-less
//! records on the converted document through the same seeds.
//!
//! **Where it lives.** This module holds the records, the bake and the edits.
//! The depth-neutral half both depths share (the host's view, the seed, the
//! bake rectangle, the table check) is `vector_mask_common`; the seeds, the
//! follow hook and the `.rz` PATH table are `doc_vector_mask_carry`.

use std::sync::Arc;

use image::GrayImage;

use crate::doc::{translated_offset, DrDocument, Layer, LayerKind};
use crate::doc_lock::EditKind;
use crate::doc_mask::MaskProperties;
use crate::job_control::JobContext;
use crate::path_geometry::PathShape;
use crate::vector_mask_common::{
    anchor_refusal, bake_rect, path_on, shifted, union_rect, RasterPartView, VectorMaskView,
};

/// The user's own raster mask, kept beside the shape.
#[derive(Clone)]
pub(crate) struct RasterPart {
    pub(crate) plane: Arc<GrayImage>,
    pub(crate) properties: MaskProperties,
    pub(crate) enabled: bool,
}

/// One layer's vector mask.
#[derive(Clone)]
pub(crate) struct VectorMaskRecord {
    /// The layer the record belongs to (`Layer::id`).
    pub(crate) layer_id: u64,
    /// Canvas coordinates as of the bake.
    pub(crate) shape: PathShape,
    pub(crate) enabled: bool,
    /// Whether the mask moves and transforms with the layer's pixels.
    pub(crate) linked: bool,
    pub(crate) raster: Option<RasterPart>,
    /// The plane this record put in the layer's slot; `None` when it left
    /// the slot empty.
    pub(crate) baked: Option<Arc<GrayImage>>,
    /// The slot's offset as of the bake.
    pub(crate) baked_offset: (i32, i32),
}

impl std::fmt::Debug for VectorMaskRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorMaskRecord")
            .field("layer_id", &self.layer_id)
            .field("anchors", &self.shape.anchor_count())
            .field("enabled", &self.enabled)
            .field("linked", &self.linked)
            .field("raster", &self.raster.is_some())
            .finish()
    }
}

/// What a bake puts in the layer's slot.
struct Slot {
    mask: Option<Arc<GrayImage>>,
    enabled: bool,
    properties: MaskProperties,
}

/// A u16 coverage sample as a byte, rounded to nearest.
fn narrowed(sample: u16) -> u8 {
    ((u32::from(sample) * 255 + 32767) / 65535) as u8
}

/// Whether `layer`'s slot still holds the plane `record` baked.
fn slot_matches(layer: &Layer, record: &VectorMaskRecord) -> bool {
    match (&layer.mask, &record.baked) {
        (None, None) => true,
        (Some(slot), Some(baked)) => Arc::ptr_eq(slot, baked),
        _ => false,
    }
}

/// `properties`' semantic fields on a value with caches of its own, so a
/// bake's feathered plane is dropped with the bake.
fn uncached(properties: &MaskProperties) -> MaskProperties {
    let mut copy = MaskProperties::at(properties.offset);
    copy.linked = properties.linked;
    copy.density = properties.density;
    copy.feather = properties.feather;
    copy.outside_coverage = properties.outside_coverage;
    copy
}

impl DrDocument {
    /// Entry `idx`'s LIVE vector-mask record as of now: a slot the layer has
    /// been moved with since the bake has that translation applied to the
    /// shape and to the raster part. `None` for an out-of-range index, an
    /// entry with no record, and a dead record.
    pub(crate) fn live_vector_mask(&self, idx: usize) -> Option<VectorMaskRecord> {
        let layer = self.layers.get(idx)?;
        let record = self
            .paths
            .vector_masks
            .iter()
            .find(|r| r.layer_id == layer.id)?;
        if !slot_matches(layer, record) {
            return None;
        }
        let mut record = record.clone();
        if layer.mask.is_some() {
            let slot = layer.mask_properties.offset;
            let dx = i64::from(slot.0) - i64::from(record.baked_offset.0);
            let dy = i64::from(slot.1) - i64::from(record.baked_offset.1);
            if (dx, dy) != (0, 0) {
                record.shape = shifted(&record.shape, dx, dy);
                if let Some(raster) = &mut record.raster {
                    raster.properties.offset = translated_offset(raster.properties.offset, dx, dy);
                }
                record.baked_offset = slot;
            }
        }
        Some(record)
    }

    /// The stored records that are live, as stored (no translation applied),
    /// leaving out `except`'s — what a vector-mask write carries forward.
    fn kept_vector_masks(&self, except: Option<u64>) -> Vec<VectorMaskRecord> {
        self.paths
            .vector_masks
            .iter()
            .filter(|record| Some(record.layer_id) != except)
            .filter(|record| {
                self.layers
                    .iter()
                    .find(|l| l.id == record.layer_id)
                    .is_some_and(|layer| slot_matches(layer, record))
            })
            .cloned()
            .collect()
    }

    /// Drops every dead record — what a path write does on its way through.
    pub(crate) fn prune_vector_masks(&mut self) {
        if self.paths.vector_masks.is_empty() {
            return;
        }
        let kept = self.kept_vector_masks(None);
        if kept.len() != self.paths.vector_masks.len() {
            self.paths.vector_masks = Arc::new(kept);
        }
    }

    /// Every live record with its entry's index, bottom first.
    pub(crate) fn live_vector_masks(&self) -> Vec<(usize, VectorMaskRecord)> {
        if self.paths.vector_masks.is_empty() {
            return Vec::new();
        }
        (0..self.layers.len())
            .filter_map(|idx| Some((idx, self.live_vector_mask(idx)?)))
            .collect()
    }

    /// The anchors of the document's live vector-mask shapes, which share
    /// `MAX_DOCUMENT_PATH_ANCHORS` with the path list.
    pub(crate) fn vector_mask_anchor_count(&self) -> usize {
        self.live_vector_masks()
            .iter()
            .map(|(_, record)| record.shape.anchor_count())
            .sum()
    }

    /// The live raster parts that are planes of their own (not the slot's),
    /// as dimensions: each is one more mask plane of its layer in every
    /// capacity total.
    pub(crate) fn vector_mask_raster_dimensions(&self) -> Vec<(u32, u32)> {
        self.live_vector_masks()
            .iter()
            .filter_map(|(idx, record)| {
                let raster = record.raster.as_ref()?;
                let shared = self.layers[*idx]
                    .mask
                    .as_ref()
                    .is_some_and(|slot| Arc::ptr_eq(slot, &raster.plane));
                (!shared).then(|| raster.plane.dimensions())
            })
            .collect()
    }

    /// Entry `idx`'s vector mask, or `None` when it has none.
    pub fn vector_mask(&self, idx: usize) -> Option<VectorMaskView> {
        let record = self.live_vector_mask(idx)?;
        Some(VectorMaskView {
            shape: record.shape,
            enabled: record.enabled,
            linked: record.linked,
            has_raster: record.raster.is_some(),
            raster_enabled: record.raster.as_ref().is_some_and(|r| r.enabled),
            raster: record
                .raster
                .as_ref()
                .map(|r| RasterPartView::of(&r.properties, r.plane.dimensions())),
        })
    }

    /// The canvas rectangle a path plane for entry `idx` may cover: the
    /// layer's pixel rect, or the canvas for a group. An UNLINKED mask and an
    /// adjustment layer's take the canvas as well, since the mask can show
    /// through wherever the pixels are later moved (or, for an adjustment,
    /// everywhere).
    fn vector_mask_extent(&self, layer: &Layer, linked: bool) -> (i64, i64, i64, i64) {
        let canvas = (0, 0, i64::from(self.width), i64::from(self.height));
        if layer.kind == LayerKind::Group {
            return canvas;
        }
        let (w, h) = layer.pixels.dimensions();
        let rect = (
            i64::from(layer.offset.0),
            i64::from(layer.offset.1),
            i64::from(w),
            i64::from(h),
        );
        if !linked || crate::doc_geometry_set::is_adjustment(layer.meta.as_deref()) {
            union_rect(rect, canvas)
        } else {
            rect
        }
    }

    /// What `record` puts in entry `idx`'s slot (the module doc's rule).
    /// `Err` is the rasterizer's refusal or a cancellation.
    fn baked_slot(
        &self,
        idx: usize,
        record: &VectorMaskRecord,
        job: JobContext<'_>,
    ) -> Result<Slot, String> {
        let layer = self.layers.get(idx).ok_or("no such layer")?;
        let on = path_on(&record.shape, record.enabled);
        if !on {
            if let Some(raster) = &record.raster {
                return Ok(Slot {
                    mask: Some(Arc::clone(&raster.plane)),
                    enabled: raster.enabled,
                    properties: raster.properties.clone(),
                });
            }
            if record.shape.subpaths.is_empty() {
                return Ok(Slot {
                    mask: None,
                    enabled: true,
                    properties: MaskProperties::default(),
                });
            }
        }
        let raster = record.raster.as_ref().filter(|r| r.enabled && on);
        let group = layer.kind == LayerKind::Group;
        let aligned = raster.filter(|r| {
            !group
                && r.properties.outside_coverage == 0
                && r.properties.feather == 0.0
                && r.properties.density == 1.0
        });
        let (x, y, w, h) = if let Some(raster) = aligned {
            let (w, h) = raster.plane.dimensions();
            (raster.properties.offset.0, raster.properties.offset.1, w, h)
        } else if group {
            // A group's mask is canvas-sized and positioned by the group's
            // own offset (`doc_align`).
            (0, 0, self.width, self.height)
        } else {
            bake_rect(&record.shape, self.vector_mask_extent(layer, record.linked))
        };
        let coverage = record.shape.coverage((x, y), w, h, true, job)?;
        let mut plane: Vec<u8> = coverage.iter().map(|&sample| narrowed(sample)).collect();
        drop(coverage);
        if let Some(raster) = raster {
            // The raster part's EFFECTIVE coverage over the same rectangle,
            // through the one mask evaluation (`doc_mask`).
            let mut holder = layer.clone();
            holder.mask = Some(Arc::clone(&raster.plane));
            holder.mask_properties = uncached(&raster.properties);
            let effective = holder
                .effective_mask_rect_with_job((x, y), w, h, job)?
                .ok_or("internal error: vector mask raster part")?;
            for (sample, keep) in plane.iter_mut().zip(effective.as_raw()) {
                *sample = ((u32::from(*sample) * u32::from(*keep) + 127) / 255) as u8;
            }
        }
        let mut properties = MaskProperties::at((x, y));
        properties.linked = record.linked;
        if !properties.valid((w, h)) {
            return Err("the vector mask's extent is not a valid mask".into());
        }
        let plane =
            GrayImage::from_raw(w, h, plane).ok_or("internal error: vector mask plane size")?;
        Ok(Slot {
            mask: Some(Arc::new(plane)),
            enabled: on,
            properties,
        })
    }

    /// The document with `record` baked into entry `idx`'s slot and stored,
    /// dead records pruned on the way.
    pub(crate) fn installing_vector_mask(
        &self,
        idx: usize,
        mut record: VectorMaskRecord,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let slot = self.baked_slot(idx, &record, job)?;
        let mut out = self.clone();
        record.baked.clone_from(&slot.mask);
        record.baked_offset = slot.properties.offset;
        let layer = &mut out.layers[idx];
        layer.mask = slot.mask;
        layer.mask_enabled = slot.enabled;
        layer.mask_properties = slot.properties;
        if layer.kind == LayerKind::Group {
            layer.offset = if layer.mask.is_some() {
                layer.mask_properties.offset
            } else {
                (0, 0)
            };
        }
        let mut records = self.kept_vector_masks(Some(record.layer_id));
        records.push(record);
        out.paths.vector_masks = Arc::new(records);
        Ok(out)
    }

    /// Gives entry `idx` the vector mask `shape` — or replaces the shape of
    /// the one it has, keeping its flags and raster part. A layer that
    /// already has an ordinary mask keeps it as the raster part. An empty
    /// shape is Reveal All. `Ok(None)` for an out-of-range index, Lock All,
    /// and the shape the mask already has; `Err` for a shape the model
    /// refuses, the document anchor cap's sentence, the rasterizer's refusal
    /// and a cancellation. `pub(crate)` because `JobContext` is; the FFI
    /// export is the public door.
    pub(crate) fn set_vector_mask(
        &self,
        idx: usize,
        shape: &PathShape,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let Some(layer) = self.layers.get(idx) else {
            return Ok(None);
        };
        if self.lock_block(idx, EditKind::Mask) != 0 {
            return Ok(None);
        }
        let shape = shape.clone().validated()?;
        let current = self.live_vector_mask(idx);
        if current.as_ref().is_some_and(|r| r.shape == shape) {
            return Ok(None);
        }
        let held = self.vector_mask_anchor_count()
            - current.as_ref().map_or(0, |r| r.shape.anchor_count())
            + self
                .paths
                .entries
                .iter()
                .map(|e| e.shape.anchor_count())
                .sum::<usize>();
        if let Some(reason) = anchor_refusal(held, &shape) {
            return Err(reason);
        }
        let record = match current {
            Some(mut record) => {
                record.shape = shape;
                record
            }
            None => VectorMaskRecord {
                layer_id: layer.id,
                shape,
                enabled: true,
                // The link is one flag for both halves: a layer whose mask
                // is unlinked gets an unlinked vector mask.
                linked: layer.mask.is_none() || layer.mask_properties.linked,
                raster: layer.mask.as_ref().map(|plane| RasterPart {
                    plane: Arc::clone(plane),
                    properties: layer.mask_properties.clone(),
                    enabled: layer.mask_enabled,
                }),
                baked: None,
                baked_offset: (0, 0),
            },
        };
        self.installing_vector_mask(idx, record, job).map(Some)
    }

    /// Removes entry `idx`'s vector mask: the raster part alone goes back in
    /// the slot, or nothing. `None` for an entry with no vector mask and
    /// under Lock All.
    pub fn remove_vector_mask(&self, idx: usize) -> Option<Self> {
        let record = self.live_vector_mask(idx)?;
        if self.lock_block(idx, EditKind::Mask) != 0 {
            return None;
        }
        let mut out = self.clone();
        out.expose(idx, &record);
        Some(out)
    }

    /// Sets the vector mask's two flags and rebakes. `Ok(None)` for an entry
    /// with no vector mask, Lock All, and the flags it already has.
    pub(crate) fn set_vector_mask_properties(
        &self,
        idx: usize,
        enabled: bool,
        linked: bool,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let Some(mut record) = self.live_vector_mask(idx) else {
            return Ok(None);
        };
        if self.lock_block(idx, EditKind::Mask) != 0
            || (record.enabled == enabled && record.linked == linked)
        {
            return Ok(None);
        }
        record.enabled = enabled;
        record.linked = linked;
        // The link is one flag for both halves (`rebaking_vector_mask`).
        if let Some(raster) = &mut record.raster {
            raster.properties.linked = linked;
        }
        self.installing_vector_mask(idx, record, job).map(Some)
    }

    /// Puts `record`'s raster part alone in entry `idx`'s slot and drops the
    /// record (and every dead one).
    fn expose(&mut self, idx: usize, record: &VectorMaskRecord) {
        let records = self.kept_vector_masks(Some(record.layer_id));
        let layer = &mut self.layers[idx];
        match &record.raster {
            Some(raster) => {
                layer.mask = Some(Arc::clone(&raster.plane));
                layer.mask_enabled = raster.enabled;
                layer.mask_properties = raster.properties.clone();
            }
            None => {
                layer.mask = None;
                layer.mask_enabled = true;
                layer.mask_properties = MaskProperties::default();
            }
        }
        if layer.kind == LayerKind::Group {
            layer.offset = if layer.mask.is_some() {
                layer.mask_properties.offset
            } else {
                (0, 0)
            };
        }
        self.paths.vector_masks = Arc::new(records);
    }

    /// The document a MASK edit of entry `idx` runs on, and the record to
    /// rebake afterwards: the slot holds the raster part alone. `None` when
    /// the entry has no live vector mask (the edit runs on `self`).
    pub(crate) fn exposing_vector_mask(&self, idx: usize) -> Option<(Self, VectorMaskRecord)> {
        let record = self.live_vector_mask(idx)?;
        let mut exposed = self.clone();
        exposed.expose(idx, &record);
        Some((exposed, record))
    }

    /// `edited` — a mask edit's result on the exposed document — with its
    /// slot taken as `record`'s new raster part and the product baked back.
    /// A result whose layer is gone is returned as it is.
    pub(crate) fn rebaking_vector_mask(
        edited: Self,
        mut record: VectorMaskRecord,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let Some(idx) = edited.layer_index_by_id(record.layer_id) else {
            return Ok(edited);
        };
        let layer = &edited.layers[idx];
        let was_linked = record.raster.as_ref().map(|r| r.properties.linked);
        record.raster = layer.mask.as_ref().map(|plane| RasterPart {
            plane: Arc::clone(plane),
            properties: layer.mask_properties.clone(),
            enabled: layer.mask_enabled,
        });
        // ONE link flag for both halves: a mask edit that toggled the raster
        // part's link (Layer ▸ Mask ▸ Link with Content) toggles the vector
        // mask's, which is the one the baked mask moves by.
        if let (Some(before), Some(raster)) = (was_linked, &record.raster) {
            if raster.properties.linked != before {
                record.linked = raster.properties.linked;
            }
        }
        edited.installing_vector_mask(idx, record, job)
    }
}
