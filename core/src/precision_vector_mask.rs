//! VECTOR MASKS at high depth — the typed twin of `doc_vector_mask`,
//! whose module doc is the specification: the baked slot and its four cases,
//! the live-or-dead rule, the translation read off the slot's offset, the
//! wrap of a MASK edit, the follow hook and the seeds a depth conversion
//! re-sets. Nothing about what a vector mask MEANS is decided here; only how
//! it is expressed over typed records.
//!
//! What differs:
//!
//! * The slot is a `PrecisionMask`, and the plane a record bakes is that
//!   mask's u16 sample allocation (`PrecisionPlane::pixels`), which every
//!   typed op that keeps a mask keeps by `Arc` — so pointer equality of that
//!   allocation is the liveness test.
//! * Path coverage is the rasterizer's own u16, so nothing is narrowed; the
//!   raster factor is `PreparedMask::at`, the one typed mask evaluation.
//! * A group's mask is not tied to the group's offset, so a group's plane is
//!   baked over the shape's pixel box inside the canvas like an adjustment's.
//! * The ops are admitted: [`bake_scratch`] is charged against the caller's
//!   working limit before any plane is allocated, and a refusal is a named
//!   `Err` (the typed convention) where the byte twin answers `None`.
//!
//! A 32-bit float document runs this module unchanged: its masks are the
//! same u16 planes.
//!
//! The seeds, the follow hook and the `.rz` PATH table are
//! `precision_vector_mask_carry`; the depth-neutral half both depths share
//! is `vector_mask_common`.

use std::sync::Arc;

use crate::doc_lock::EditKind;
use crate::doc_mask::MaskProperties;
use crate::job_control::{DrJobControl, JobContext};
use crate::path_geometry::{coverage_scratch, PathShape};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::precision_mask::{PrecisionMask, PrecisionPlane};
use crate::vector_mask_common::{
    anchor_refusal, bake_rect, path_on, shifted, union_rect, RasterPartView, VectorMaskView,
};

/// One typed layer's vector mask (`doc_vector_mask::VectorMaskRecord`).
#[derive(Clone)]
pub(crate) struct TypedVectorMaskRecord {
    pub(crate) layer_id: u64,
    pub(crate) shape: PathShape,
    pub(crate) enabled: bool,
    pub(crate) linked: bool,
    /// The user's own raster mask, kept beside the shape.
    pub(crate) raster: Option<PrecisionMask>,
    /// The sample allocation of the plane this record put in the slot;
    /// `None` when it left the slot empty.
    pub(crate) baked: Option<Arc<Vec<u16>>>,
    pub(crate) baked_offset: (i32, i32),
}

impl std::fmt::Debug for TypedVectorMaskRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedVectorMaskRecord")
            .field("layer_id", &self.layer_id)
            .field("anchors", &self.shape.anchor_count())
            .field("enabled", &self.enabled)
            .field("linked", &self.linked)
            .field("raster", &self.raster.is_some())
            .finish()
    }
}

/// The working memory one bake over a `width` x `height` rectangle may hold
/// beside the document: the rasterizer's scratch and the u16 plane it
/// answers (`path_geometry::coverage_scratch`), one more plane while the
/// product is written, and bookkeeping. The raster part's feather scratch is
/// added by the caller, which knows the part.
pub(crate) fn bake_scratch(width: u32, height: u32) -> u64 {
    coverage_scratch(width, height) + u64::from(width) * u64::from(height) * 2 + BOOKKEEPING_BYTES
}

/// Whether `layer`'s slot still holds the plane `record` baked.
fn slot_matches(layer: &PrecisionDocumentLayer, record: &TypedVectorMaskRecord) -> bool {
    match (&layer.mask, &record.baked) {
        (None, None) => true,
        (Some(slot), Some(baked)) => Arc::ptr_eq(&slot.plane.pixels, baked),
        _ => false,
    }
}

/// `mask` moved by a whole-pixel delta.
fn moved(mask: &PrecisionMask, dx: i64, dy: i64) -> PrecisionMask {
    let mut next = mask.clone();
    next.properties.offset = crate::doc::translated_offset(mask.properties.offset, dx, dy);
    next
}

impl PrecisionDocument {
    /// `DrDocument::live_vector_mask` at high depth.
    pub(crate) fn live_vector_mask(&self, index: usize) -> Option<TypedVectorMaskRecord> {
        if self.paths.vector_masks.is_empty() {
            return None;
        }
        let layer = self.layer(index)?;
        let record = self
            .paths
            .vector_masks
            .iter()
            .find(|r| r.layer_id == layer.id)?;
        if !slot_matches(layer, record) {
            return None;
        }
        let mut record = record.clone();
        if let Some(slot) = &layer.mask {
            let at = slot.properties.offset;
            let dx = i64::from(at.0) - i64::from(record.baked_offset.0);
            let dy = i64::from(at.1) - i64::from(record.baked_offset.1);
            if (dx, dy) != (0, 0) {
                record.shape = shifted(&record.shape, dx, dy);
                record.raster = record.raster.as_ref().map(|r| moved(r, dx, dy));
                record.baked_offset = at;
            }
        }
        Some(record)
    }

    /// The stored records that are live, as stored, leaving out `except`'s.
    fn kept_vector_masks(&self, except: Option<u64>) -> Vec<TypedVectorMaskRecord> {
        self.paths
            .vector_masks
            .iter()
            .filter(|record| Some(record.layer_id) != except)
            .filter(|record| {
                self.layer_index_by_id(record.layer_id)
                    .and_then(|index| self.layer(index))
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

    /// Every live record with its record index, bottom first.
    pub(crate) fn live_vector_masks(&self) -> Vec<(usize, TypedVectorMaskRecord)> {
        if self.paths.vector_masks.is_empty() {
            return Vec::new();
        }
        (0..self.layer_count())
            .filter_map(|index| Some((index, self.live_vector_mask(index)?)))
            .collect()
    }

    /// `DrDocument::vector_mask_anchor_count` at high depth.
    pub(crate) fn vector_mask_anchor_count(&self) -> usize {
        self.live_vector_masks()
            .iter()
            .map(|(_, record)| record.shape.anchor_count())
            .sum()
    }

    /// The live raster parts that are planes of their own, with the record
    /// index of their layer: each is one more mask plane of that layer in
    /// every capacity, plan and resource total.
    pub(crate) fn vector_mask_raster_parts(&self) -> Vec<(usize, PrecisionMask)> {
        self.live_vector_masks()
            .into_iter()
            .filter_map(|(index, record)| {
                let raster = record.raster?;
                let shared = self
                    .layer(index)
                    .and_then(|layer| layer.mask.as_ref())
                    .is_some_and(|slot| Arc::ptr_eq(&slot.plane.pixels, &raster.plane.pixels));
                (!shared).then_some((index, raster))
            })
            .collect()
    }

    /// Record `index`'s vector mask, or `None` when it has none.
    pub fn vector_mask(&self, index: usize) -> Option<VectorMaskView> {
        let record = self.live_vector_mask(index)?;
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

    /// The rectangle `record`'s path plane is baked over, or `None` when the
    /// slot takes no new plane (the raster part verbatim, or nothing).
    fn vector_mask_bake_rect(
        &self,
        layer: &PrecisionDocumentLayer,
        record: &TypedVectorMaskRecord,
    ) -> Option<(i32, i32, u32, u32)> {
        let on = path_on(&record.shape, record.enabled);
        if !on && (record.raster.is_some() || record.shape.subpaths.is_empty()) {
            return None;
        }
        let raster = record.raster.as_ref().filter(|r| r.enabled && on);
        if let Some(raster) = raster.filter(|r| {
            r.properties.outside_coverage == 0
                && r.properties.feather == 0.0
                && r.properties.density == 1.0
        }) {
            let (w, h) = raster.plane.dimensions();
            return Some((raster.properties.offset.0, raster.properties.offset.1, w, h));
        }
        let canvas = (0, 0, i64::from(self.width), i64::from(self.height));
        // A CMYK raster (#547) bakes over its own rect exactly as an RGB one.
        let dimensions = layer
            .source()
            .map(|image| image.dimensions())
            .or_else(|| layer.cmyk_source().map(|image| image.dimensions()));
        let extent = match dimensions {
            Some((w, h)) => {
                let (x, y) = layer.offset();
                let rect = (i64::from(x), i64::from(y), i64::from(w), i64::from(h));
                if record.linked {
                    rect
                } else {
                    union_rect(rect, canvas)
                }
            }
            None => canvas,
        };
        Some(bake_rect(&record.shape, extent))
    }

    /// What `record` puts in record `index`'s slot (`doc_vector_mask`'s
    /// rule). `Err` is the rasterizer's refusal or a cancellation.
    fn baked_slot(
        &self,
        index: usize,
        record: &TypedVectorMaskRecord,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionMask>, String> {
        let layer = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let Some((x, y, w, h)) = self.vector_mask_bake_rect(layer, record) else {
            return Ok(record.raster.clone());
        };
        let on = path_on(&record.shape, record.enabled);
        let mut plane = record.shape.coverage((x, y), w, h, true, job)?;
        if let Some(raster) = record.raster.as_ref().filter(|r| r.enabled && on) {
            let prepared = raster.prepare(job)?;
            for row in 0..h {
                job.check()?;
                let cy = i64::from(y) + i64::from(row);
                let start = row as usize * w as usize;
                for (column, sample) in plane[start..start + w as usize].iter_mut().enumerate() {
                    let keep = prepared.at(i64::from(x) + column as i64, cy);
                    *sample = (f32::from(*sample) * keep).round().clamp(0.0, 65535.0) as u16;
                }
            }
        }
        let mut properties = MaskProperties::at((x, y));
        properties.linked = record.linked;
        PrecisionMask::new(PrecisionPlane::adopt(w, h, plane)?, properties, on).map(Some)
    }

    /// The charge of baking `record` onto record `index`, admitted against
    /// the caller's working limit before any plane is allocated.
    pub(crate) fn admit_bake(
        &self,
        index: usize,
        record: &TypedVectorMaskRecord,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let layer = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let mut scratch = 4 * BOOKKEEPING_BYTES;
        if let Some((_, _, w, h)) = self.vector_mask_bake_rect(layer, record) {
            scratch += bake_scratch(w, h);
            if let Some(raster) = &record.raster {
                scratch += raster.plan()?.0;
            }
        }
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)
    }

    /// The document with `record` baked into record `index`'s slot and
    /// stored, dead records pruned on the way. The result passes the native
    /// preflight with the raster part counted.
    pub(crate) fn installing_vector_mask(
        &self,
        index: usize,
        mut record: TypedVectorMaskRecord,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let slot = self.baked_slot(index, &record, job)?;
        record.baked = slot.as_ref().map(|mask| Arc::clone(&mask.plane.pixels));
        record.baked_offset = slot.as_ref().map_or((0, 0), |mask| mask.properties.offset);
        let mut layer = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?
            .clone();
        layer.mask = slot;
        let mut records = self.kept_vector_masks(Some(record.layer_id));
        records.push(record);
        let mut out = self
            .replacing_layer(index, layer)?
            .ok_or("internal error: vector mask layer")?;
        out.paths.vector_masks = Arc::new(records);
        out.native_preflight()?;
        Ok(out)
    }

    /// `DrDocument::set_vector_mask` at high depth, admitted against `limit`
    /// / `other`. `Ok(None)` for the shape the mask already has; `Err` for
    /// an out-of-range index, a lock, a shape the model refuses, the
    /// document anchor cap's sentence, a refused admission, the rasterizer's
    /// refusal and a cancellation.
    pub fn set_vector_mask(
        &self,
        index: usize,
        shape: &PathShape,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let layer = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        self.lock_refusal(index, EditKind::Mask)?;
        let shape = shape.clone().validated()?;
        let current = self.live_vector_mask(index);
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
            None => TypedVectorMaskRecord {
                layer_id: layer.id,
                shape,
                enabled: true,
                // One link flag for both halves (`doc_vector_mask`).
                linked: layer.mask.as_ref().is_none_or(|m| m.properties.linked),
                raster: layer.mask.clone(),
                baked: None,
                baked_offset: (0, 0),
            },
        };
        self.admit_bake(index, &record, limit, other, job)?;
        let out = self.installing_vector_mask(index, record, job)?;
        job.report(1.0, 5)?;
        Ok(Some(out))
    }

    /// `DrDocument::remove_vector_mask` at high depth: the raster part alone
    /// goes back in the slot, or nothing. `Ok(None)` for a record with no
    /// vector mask; `Err` for a lock and a refused admission.
    pub fn remove_vector_mask(
        &self,
        index: usize,
        limit: u64,
        other: u64,
    ) -> Result<Option<Self>, String> {
        let Some(record) = self.live_vector_mask(index) else {
            return Ok(None);
        };
        self.lock_refusal(index, EditKind::Mask)?;
        admit(self.retained_bytes(), 4 * BOOKKEEPING_BYTES, other, limit)?;
        self.exposed(index, &record).map(Some)
    }

    /// `DrDocument::set_vector_mask_properties` at high depth. `Ok(None)`
    /// for a record with no vector mask and for the flags it already has.
    pub fn set_vector_mask_properties(
        &self,
        index: usize,
        enabled: bool,
        linked: bool,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let Some(mut record) = self.live_vector_mask(index) else {
            return Ok(None);
        };
        self.lock_refusal(index, EditKind::Mask)?;
        if record.enabled == enabled && record.linked == linked {
            return Ok(None);
        }
        record.enabled = enabled;
        record.linked = linked;
        // The link is one flag for both halves (`doc_vector_mask`).
        if let Some(raster) = &mut record.raster {
            raster.properties.linked = linked;
        }
        self.admit_bake(index, &record, limit, other, job)?;
        let out = self.installing_vector_mask(index, record, job)?;
        job.report(1.0, 5)?;
        Ok(Some(out))
    }

    /// The document with `record`'s raster part alone in record `index`'s
    /// slot and the record dropped (with every dead one).
    fn exposed(&self, index: usize, record: &TypedVectorMaskRecord) -> Result<Self, String> {
        let mut layer = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?
            .clone();
        layer.mask.clone_from(&record.raster);
        let records = self.kept_vector_masks(Some(record.layer_id));
        let mut out = self
            .replacing_layer(index, layer)?
            .ok_or("internal error: vector mask layer")?;
        out.paths.vector_masks = Arc::new(records);
        Ok(out)
    }

    /// `DrDocument::exposing_vector_mask` at high depth.
    pub(crate) fn exposing_vector_mask(
        &self,
        index: usize,
    ) -> Result<Option<(Self, TypedVectorMaskRecord)>, String> {
        let Some(record) = self.live_vector_mask(index) else {
            return Ok(None);
        };
        Ok(Some((self.exposed(index, &record)?, record)))
    }

    /// `DrDocument::rebaking_vector_mask` at high depth.
    pub(crate) fn rebaking_vector_mask(
        edited: Self,
        mut record: TypedVectorMaskRecord,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let Some(index) = edited.layer_index_by_id(record.layer_id) else {
            return Ok(edited);
        };
        let was_linked = record.raster.as_ref().map(|r| r.properties.linked);
        record.raster = edited.layer(index).and_then(|layer| layer.mask.clone());
        // One link flag for both halves (`doc_vector_mask`).
        if let (Some(before), Some(raster)) = (was_linked, &record.raster) {
            if raster.properties.linked != before {
                record.linked = raster.properties.linked;
            }
        }
        edited.installing_vector_mask(index, record, job)
    }
}
