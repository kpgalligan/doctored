//! VECTOR MASKS at high depth, what CARRIES them across an op that
//! does not know about them: the seeds a geometry op or a depth conversion
//! re-sets, the follow hook of the typed geometry entry point, and the `.rz`
//! PATH table written and adopted. The typed twin of `doc_vector_mask_carry`;
//! split from `precision_vector_mask`, which holds the records, the bake and
//! the edits.

use std::sync::Arc;

use crate::doc_path::{PathEntry, PathKind};
use crate::doc_rotate::RotationKind;
use crate::job_control::JobContext;
use crate::path_geometry::PathShape;
use crate::path_native::{TableMask, TablePlane, TableRaster};
use crate::precision_document::PrecisionDocument;
use crate::precision_geometry::{GeometryEdit, GeometryTarget};
use crate::precision_mask::{PrecisionMask, PrecisionPlane};
use crate::precision_vector_mask::TypedVectorMaskRecord;
use crate::vector_mask_common::{table_mask_agrees, VectorMaskSeed, TABLE_MISMATCH};

/// A vector-mask shape after `edit` on a `width` x `height` canvas, or
/// `None` where the mask does not follow and the layer keeps its resampled
/// plane: a perspective, a straighten and a filtered canvas rotation. A
/// per-layer affine of the pixels maps the shape by its matrix (the mask
/// follows only when linked, and an unlinked one is never asked); every
/// exact whole-canvas edit maps it the way it maps the path list
/// (`precision_path::edited_paths`). A MASK-target edit is not asked either:
/// it runs under the lock wrap, on the raster part.
fn followed_shape(
    shape: &PathShape,
    edit: &GeometryEdit,
    width: u32,
    height: u32,
) -> Option<PathShape> {
    match edit {
        GeometryEdit::Affine {
            target: GeometryTarget::Pixels,
            matrix,
            ..
        }
        | GeometryEdit::AffineSet { matrix, .. } => Some(shape.transformed(*matrix)),
        GeometryEdit::Affine { .. }
        | GeometryEdit::Perspective { .. }
        | GeometryEdit::PerspectiveSet { .. }
        | GeometryEdit::Straighten { .. } => None,
        GeometryEdit::RotateCanvas { degrees, .. }
            if !matches!(
                crate::doc_rotate::rotation_kind(*degrees),
                Some((
                    RotationKind::Identity
                        | RotationKind::Rotate90
                        | RotationKind::Rotate180
                        | RotationKind::Rotate270,
                    _
                ))
            ) =>
        {
            None
        }
        _ => {
            let entry = PathEntry {
                id: 0,
                name: String::new(),
                kind: PathKind::Saved,
                shape: shape.clone(),
                clipping: None,
            };
            crate::precision_path::edited_paths(&[entry], edit, width, height)
                .ok()?
                .pop()
                .map(|entry| entry.shape)
        }
    }
}

impl PrecisionDocument {
    /// `DrDocument::vector_mask_seeds` at high depth.
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

    /// `DrDocument::seeding_vector_masks` at high depth, each bake admitted
    /// against `limit` / `other`: a seed whose layer is gone or whose record
    /// is already live is skipped, and one whose admission or bake fails
    /// leaves the layer the mask it has.
    pub(crate) fn seeding_vector_masks(
        mut self,
        seeds: Vec<VectorMaskSeed>,
        limit: u64,
        other: u64,
    ) -> Self {
        for seed in seeds {
            let Some(index) = self.layer_index_by_id(seed.layer_id) else {
                continue;
            };
            if self.live_vector_mask(index).is_some() {
                continue;
            }
            let record = TypedVectorMaskRecord {
                layer_id: seed.layer_id,
                shape: seed.shape,
                enabled: seed.enabled,
                linked: seed.linked,
                raster: None,
                baked: None,
                baked_offset: (0, 0),
            };
            let job = JobContext::NONE;
            let next = self
                .admit_bake(index, &record, limit, other, job)
                .and_then(|()| self.installing_vector_mask(index, record, job));
            if let Ok(next) = next {
                self = next;
            }
        }
        self
    }

    /// The follow hook of the typed geometry entry point: `after` — `edit`'s
    /// result on `self` — with every raster-less vector mask the edit
    /// resampled rebuilt from its shape through the edit's own point map
    /// ([`followed_shape`]). A record the edit left alone or only translated
    /// is still live in `after` and is skipped.
    pub(crate) fn following_geometry(
        &self,
        after: Self,
        edit: &GeometryEdit,
        limit: u64,
        other: u64,
    ) -> Self {
        if self.paths.vector_masks.is_empty() {
            return after;
        }
        let (width, height) = self.dimensions();
        let seeds = self
            .vector_mask_seeds(PathShape::clone)
            .into_iter()
            .filter_map(|seed| {
                Some(VectorMaskSeed {
                    shape: followed_shape(&seed.shape, edit, width, height)?,
                    ..seed
                })
            })
            .collect();
        after.seeding_vector_masks(seeds, limit, other)
    }

    /// The live vector masks as `path_native` stores them
    /// (`DrDocument::vector_mask_table`).
    pub(crate) fn vector_mask_table(&self) -> Vec<TableMask<'static>> {
        self.live_vector_masks()
            .into_iter()
            .map(|(index, record)| {
                let slot = self.layer(index).and_then(|layer| layer.mask.as_ref());
                TableMask {
                    layer: index,
                    shape: record.shape,
                    enabled: record.enabled,
                    linked: record.linked,
                    raster: record.raster.map(|raster| TableRaster {
                        plane: (!slot.is_some_and(|slot| {
                            Arc::ptr_eq(&slot.plane.pixels, &raster.plane.pixels)
                        }))
                        .then(|| {
                            (
                                raster.plane.dimensions(),
                                TablePlane::Words(Arc::clone(&raster.plane.pixels)),
                            )
                        }),
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

    /// Binds the records a file's PATH table describes to the records the
    /// reader built (`doc_vector_mask::adopted_records`): each stored mask is
    /// the plane its record baked. `planes` turns a stored plane's bytes
    /// into a typed plane.
    pub(crate) fn adopt_vector_masks(
        &mut self,
        masks: Vec<TableMask<'_>>,
        planes: impl Fn(&[u8], (u32, u32)) -> Result<PrecisionPlane, String>,
    ) -> Result<(), String> {
        let wrong = || TABLE_MISMATCH.to_string();
        let mut records = Vec::with_capacity(masks.len());
        for mask in masks {
            let layer = self.layer(mask.layer).ok_or_else(wrong)?;
            let stored = layer.mask.as_ref().map(|m| m.plane.dimensions());
            if !table_mask_agrees(&mask, stored) {
                return Err(wrong());
            }
            let raster = match mask.raster {
                None => None,
                Some(raster) => Some(match &raster.plane {
                    Some((dimensions, TablePlane::Raw(bytes))) => PrecisionMask::new(
                        planes(bytes, *dimensions)?,
                        raster.properties(),
                        raster.enabled,
                    )?,
                    Some(_) => return Err(wrong()),
                    None => layer.mask.clone().ok_or_else(wrong)?,
                }),
            };
            records.push(TypedVectorMaskRecord {
                layer_id: layer.id,
                shape: mask.shape,
                enabled: mask.enabled,
                linked: mask.linked,
                raster,
                baked: layer.mask.as_ref().map(|m| Arc::clone(&m.plane.pixels)),
                baked_offset: layer.mask.as_ref().map_or((0, 0), |m| m.properties.offset),
            });
        }
        self.paths.vector_masks = Arc::new(records);
        Ok(())
    }
}
