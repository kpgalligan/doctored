//! VECTOR MASKS at 8 bits, what CARRIES them across an op that does
//! not know about them: the seeds a geometry op or a depth conversion re-sets,
//! the follow hook, and the `.rz` PATH table written and adopted. Split from
//! `doc_vector_mask`, whose module doc is the specification (the follow rule
//! is its "Following geometry"); the typed twin is
//! `precision_vector_mask_carry`.

use std::sync::Arc;

use image::GrayImage;

use crate::doc::{DrDocument, Layer};
use crate::doc_vector_mask::{RasterPart, VectorMaskRecord};
use crate::job_control::JobContext;
use crate::path_geometry::PathShape;
use crate::path_native::{TableMask, TablePlane, TableRaster};
use crate::vector_mask_common::{table_mask_agrees, VectorMaskSeed, TABLE_MISMATCH};

impl DrDocument {
    /// The live raster-less vector masks, each with its shape through `map`
    /// — what a geometry op or a depth conversion re-sets on its result.
    pub(crate) fn vector_mask_seeds(
        &self,
        map: impl Fn(&PathShape) -> PathShape,
    ) -> Vec<VectorMaskSeed> {
        self.live_vector_masks()
            .into_iter()
            .filter(|(_, record)| record.raster.is_none() && !record.shape.subpaths.is_empty())
            .map(|(_, record)| VectorMaskSeed {
                layer_id: record.layer_id,
                shape: map(&record.shape),
                enabled: record.enabled,
                linked: record.linked,
            })
            .collect()
    }

    /// `self` with each seed baked onto the layer of its id, replacing that
    /// layer's mask. A seed whose layer is gone or whose record is already
    /// live here (the op only translated the plane) is skipped, and one whose
    /// bake fails leaves the layer the mask it has.
    pub(crate) fn seeding_vector_masks(mut self, seeds: Vec<VectorMaskSeed>) -> Self {
        for seed in seeds {
            let Some(idx) = self.layer_index_by_id(seed.layer_id) else {
                continue;
            };
            if self.live_vector_mask(idx).is_some() {
                continue;
            }
            let record = VectorMaskRecord {
                layer_id: seed.layer_id,
                shape: seed.shape,
                enabled: seed.enabled,
                linked: seed.linked,
                raster: None,
                baked: None,
                baked_offset: (0, 0),
            };
            if let Ok(next) = self.installing_vector_mask(idx, record, JobContext::NONE) {
                self = next;
            }
        }
        self
    }

    /// The follow hook: `after` — a geometry op's result on `self` — with
    /// every raster-less vector mask the op resampled rebuilt from its shape
    /// through `map`, the op's own point map (the module doc).
    pub(crate) fn following_vector_masks(
        &self,
        after: Self,
        map: impl Fn(f64, f64) -> (f64, f64),
    ) -> Self {
        if self.paths.vector_masks.is_empty() {
            return after;
        }
        after.seeding_vector_masks(self.vector_mask_seeds(|shape| shape.mapped(&map)))
    }

    /// The live vector masks as `path_native` stores them. A raster part
    /// that is the slot's own plane is written without one.
    pub(crate) fn vector_mask_table(&self) -> Vec<TableMask<'static>> {
        self.live_vector_masks()
            .into_iter()
            .map(|(idx, record)| {
                let slot = self.layers[idx].mask.as_ref();
                TableMask {
                    layer: idx,
                    shape: record.shape,
                    enabled: record.enabled,
                    linked: record.linked,
                    raster: record.raster.map(|raster| TableRaster {
                        plane: (!slot.is_some_and(|slot| Arc::ptr_eq(slot, &raster.plane))).then(
                            || {
                                (
                                    raster.plane.dimensions(),
                                    TablePlane::Bytes(Arc::clone(&raster.plane)),
                                )
                            },
                        ),
                        offset: raster.properties.offset,
                        linked: raster.properties.linked,
                        density: raster.properties.density,
                        feather: raster.properties.feather,
                        outside: raster.properties.outside_coverage,
                        enabled: raster.enabled,
                    }),
                }
            })
            .collect()
    }
}

/// The records a file's PATH table describes, over the layers the reader
/// decoded — each layer's stored mask is the plane its record baked. The
/// reader has already asked [`table_mask_agrees`] of every record.
pub(crate) fn adopted_records(
    layers: &[Layer],
    masks: Vec<TableMask<'_>>,
) -> Result<Vec<VectorMaskRecord>, String> {
    let wrong = || TABLE_MISMATCH.to_string();
    let mut records = Vec::with_capacity(masks.len());
    for mask in masks {
        let layer = layers.get(mask.layer).ok_or_else(wrong)?;
        if !table_mask_agrees(&mask, layer.mask.as_ref().map(|m| m.dimensions())) {
            return Err(wrong());
        }
        let raster = match mask.raster {
            None => None,
            Some(raster) => Some(match &raster.plane {
                Some(((w, h), TablePlane::Raw(bytes))) => RasterPart {
                    plane: Arc::new(GrayImage::from_raw(*w, *h, bytes.to_vec()).ok_or_else(wrong)?),
                    properties: raster.properties(),
                    enabled: raster.enabled,
                },
                Some(_) => return Err(wrong()),
                // The raster part IS the stored mask, properties included.
                None => RasterPart {
                    plane: Arc::clone(layer.mask.as_ref().ok_or_else(wrong)?),
                    properties: layer.mask_properties.clone(),
                    enabled: layer.mask_enabled,
                },
            }),
        };
        records.push(VectorMaskRecord {
            layer_id: layer.id,
            shape: mask.shape,
            enabled: mask.enabled,
            linked: mask.linked,
            raster,
            baked: layer.mask.clone(),
            baked_offset: layer.mask_properties.offset,
        });
    }
    Ok(records)
}
