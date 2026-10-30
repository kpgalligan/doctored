//! Edit ▸ Puppet Warp at 16 bits and at 32-bit float: the typed
//! document's thin adapter over the kernels, the twin of `doc_puppet` with
//! the same request, refusals, plan shape and write-back. The 16-bit arm
//! resamples with the persisted warp's RGBA16 accumulate/finish
//! (`precision_warp`); the float arm accumulates premultiplied extended
//! samples in binary64 (`precision_warp::accumulate_extended_f64`) and
//! finishes through `sampling::straight_extended` against the layer's own
//! visible range (`float_document`'s D3), as the float warp does. A linked
//! mask warps through the same map at u16; the persisted warp binding is
//! dropped. Family `geometry_puppet` (`precision_capability`). On a CMYK
//! document (#547 phase 4b) the record is straight CMYKA16 ink
//! ([`MeshPixels::Cmyk`]): the same `puppet_render::render` walk with the
//! five-plane binary64 adapter (`precision_warp::accumulate_planes16` /
//! `finish_planes16`, C = 5, whose C = 4 is the RGBA16 arm), adopted by
//! `CmykImage::from_contiguous`; the plan charges 10 B/px plus the CMYK
//! source and adoption copies.
use serde_json::Value;

use crate::cmyk_image::{CmykImage, CMYK_BYTES_PER_PIXEL};
use crate::doc_lock::EditKind;
use crate::doc_puppet::{
    common_scratch, dest_bytes, plan_json, prepare, Prepared, PuppetWarpRequest,
};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage};
use crate::precision_mask::PrecisionPlane;
use crate::precision_sampling::{
    cmyk_adoption_copy_bytes, cmyk_source_copy_bytes, float_adoption_copy_bytes,
    float_source_copy_bytes, visible_range,
};
use crate::precision_structure_ops::{flatten, rebuild};
use crate::precision_warp::{
    accumulate_extended_f64, accumulate_mask16, accumulate_planes16, accumulate_rgba16,
    finish_mask16, finish_planes16, finish_rgba16,
};
use crate::puppet_mesh::{build, mesh_plan_bytes, MeshOptions, PuppetMesh, Rect};
use crate::puppet_render::{render, InverseMap, PoseMap};
use crate::sampling::straight_extended;

/// The feature name the shared refusals carry.
const NAME: &str = "Puppet Warp";

/// The pixels of the record a mesh warp edits: RGBA (16-bit or float) or a
/// CMYK document's CMYKA16 ink (#547 phase 4b).
pub(crate) enum MeshPixels<'a> {
    Typed(&'a PrecisionImage),
    Cmyk(&'a CmykImage),
}

impl MeshPixels<'_> {
    /// Whether `(x, y)` holds any alpha: the mesh's opacity test.
    pub(crate) fn opaque(&self, x: u32, y: u32) -> bool {
        match self {
            Self::Typed(image) => image.sample_at(x, y).is_some_and(|p| p[3] > 0.0),
            Self::Cmyk(image) => image.sample_at(x, y).is_some_and(|p| p[4] > 0),
        }
    }
}

impl PrecisionDocument {
    /// The raster record a mesh warp (`name`) edits and its canvas rect, or
    /// the named refusal (the byte twin's order).
    pub(crate) fn mesh_warp_record(
        &self,
        index: usize,
        position: bool,
        name: &str,
    ) -> Result<(MeshPixels<'_>, Rect), String> {
        let record = self
            .layer(index)
            .ok_or_else(|| format!("{name} layer index is out of range"))?;
        let image = match &record.content {
            Content::Raster { image, .. } => MeshPixels::Typed(image),
            Content::CmykRaster { image, .. } => MeshPixels::Cmyk(image),
            Content::Adjustment(_) | Content::CmykAdjustment(_) => {
                return Err(format!(
                    "{name} needs a pixel layer, and layer {index} is an adjustment layer"
                ))
            }
            Content::Group(_) | Content::PassThrough(_) => {
                return Err(format!(
                    "{name} needs a pixel layer, and layer {index} is a group"
                ))
            }
        };
        if record.description.is_some() || record.retained_source.is_some() {
            return Err(format!("Rasterize layer {index} before {name}"));
        }
        self.lock_refusal(index, EditKind::Pixels)?;
        if position {
            self.lock_refusal(index, EditKind::Position)?;
        }
        let (w, h) = match &image {
            MeshPixels::Typed(image) => image.dimensions(),
            MeshPixels::Cmyk(image) => image.dimensions(),
        };
        let offset = record.offset();
        Ok((image, (i64::from(offset.0), i64::from(offset.1), w, h)))
    }

    /// The typed twin of `DrDocument::mesh_warp_plan`: `(retained,
    /// scratch)` for record `index` standing at `rect`, at its storage's
    /// sample size (with the float arm's source and adoption copies).
    pub(crate) fn mesh_warp_plan(
        &self,
        index: usize,
        rect: Rect,
        common: u64,
        dest: Option<Rect>,
    ) -> Result<(u64, u64), String> {
        let float = match self.layer(index).map(|r| &r.content) {
            Some(Content::Raster { image, .. }) => image.storage() == PrecisionStorage::Rgba32Float,
            _ => false,
        };
        // A CMYK record (#547 phase 4b): 10 B/px output, and both CMYK
        // copies (`to_contiguous`, `from_contiguous`) as at float.
        let cmyk = matches!(
            self.layer(index).map(|r| &r.content),
            Some(Content::CmykRaster { .. })
        );
        let mut retained = self.retained_bytes();
        let source_copy = if float {
            float_source_copy_bytes((rect.2, rect.3))?
        } else if cmyk {
            cmyk_source_copy_bytes((rect.2, rect.3))?
        } else {
            u64::from(rect.2) * u64::from(rect.3) * 8
        };
        let mut scratch = common.saturating_add(source_copy);
        if let Some(dest) = dest {
            let bytes = if float {
                16
            } else if cmyk {
                CMYK_BYTES_PER_PIXEL
            } else {
                8
            };
            let mut output = u64::from(dest.2) * u64::from(dest.3) * bytes;
            scratch = scratch.saturating_add(dest_bytes(dest, bytes));
            if float {
                scratch = scratch.saturating_add(float_adoption_copy_bytes((dest.2, dest.3))?);
            }
            if cmyk {
                scratch = scratch.saturating_add(cmyk_adoption_copy_bytes((dest.2, dest.3))?);
            }
            let linked = self
                .layer(index)
                .and_then(|r| r.mask.as_ref())
                .is_some_and(|m| m.properties.linked);
            if linked {
                output = output.saturating_add(u64::from(dest.2) * u64::from(dest.3) * 2);
                scratch = scratch.saturating_add(dest_bytes(dest, 2));
            }
            retained = retained.saturating_add(output);
        }
        Ok((retained, scratch))
    }

    /// The typed twin of `DrDocument::mesh_warp_through`.
    pub(crate) fn mesh_warp_through<M: InverseMap + ?Sized>(
        &self,
        index: usize,
        rect: Rect,
        map: &M,
        dest: Rect,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let count = dest.2 as usize * dest.3 as usize;
        let source = match self.layer(index).map(|r| &r.content) {
            Some(Content::Raster { image, .. }) => image,
            Some(Content::CmykRaster { image, .. }) => {
                // The RGBA16 arm below over five planes (#547 phase 4b).
                let samples = image.to_contiguous()?;
                let mut out = vec![[0u16; 5]; count];
                render(
                    map,
                    rect,
                    dest,
                    |i| samples[i],
                    accumulate_planes16::<5>,
                    finish_planes16::<5>,
                    |i, p| out[i] = p,
                    job,
                )?;
                drop(samples);
                let image = CmykImage::from_contiguous(dest.2, dest.3, &out, job)?;
                return self.mesh_warp_write(
                    index,
                    Content::CmykRaster {
                        image,
                        offset: (dest.0 as i32, dest.1 as i32),
                    },
                    map,
                    dest,
                    job,
                );
            }
            _ => return Err("mesh warp needs a raster layer".into()),
        };
        let image = if source.storage() == PrecisionStorage::Rgba32Float {
            let samples = source.transient_rgba32()?;
            let range = visible_range(samples.iter().copied(), job)?;
            let mut out = vec![[0f32; 4]; count];
            render(
                map,
                rect,
                dest,
                |i| samples[i],
                accumulate_extended_f64,
                |acc| straight_extended(acc.map(|v| v as f32), &range),
                |i, p| out[i] = p,
                job,
            )?;
            drop(samples);
            PrecisionImage::from_rgba32_extended(
                dest.2,
                dest.3,
                out,
                source.profile().clone(),
                None,
            )?
        } else {
            let samples = source.transient_rgba16()?;
            let mut out = vec![[0u16; 4]; count];
            render(
                map,
                rect,
                dest,
                |i| samples[i],
                accumulate_rgba16,
                finish_rgba16,
                |i, p| out[i] = p,
                job,
            )?;
            drop(samples);
            PrecisionImage::from_rgba16_unseeded(
                dest.2,
                dest.3,
                out,
                source.profile().clone(),
                None,
            )?
        };
        self.mesh_warp_write(
            index,
            Content::Raster {
                image,
                offset: (dest.0 as i32, dest.1 as i32),
            },
            map,
            dest,
            job,
        )
    }

    /// [`Self::mesh_warp_through`]'s write-back: the warped `content`, the
    /// linked mask through the same map, no warp binding.
    fn mesh_warp_write<M: InverseMap + ?Sized>(
        &self,
        index: usize,
        content: Content,
        map: &M,
        dest: Rect,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let count = dest.2 as usize * dest.3 as usize;
        let origin = (dest.0 as i32, dest.1 as i32);
        let mut flat = flatten(&self.layers);
        let layer = &mut flat
            .get_mut(index)
            .ok_or("mesh warp layer index is out of range")?
            .layer;
        layer.content = content;
        if let Some(mask) = layer.mask.as_mut().filter(|m| m.properties.linked) {
            let outside = u16::from(mask.properties.outside_coverage) * 257;
            let plane = &mask.plane;
            let (mw, mh) = plane.dimensions();
            let mo = mask.properties.offset;
            let mut warped = vec![outside; count];
            render(
                map,
                (i64::from(mo.0), i64::from(mo.1), mw, mh),
                dest,
                |i| plane.samples()[i],
                |acc: &mut [f64; 1], p, w| accumulate_mask16(acc, p, w, outside),
                |acc| finish_mask16(acc, outside),
                |i, p| warped[i] = p,
                job,
            )?;
            mask.plane = PrecisionPlane::adopt(dest.2, dest.3, warped)?;
            mask.properties.offset = origin;
        }
        layer.warp = None;
        self.rebuilt(
            self.width,
            self.height,
            rebuild(flat)?,
            self.channels.as_ref().clone(),
        )
    }

    /// The typed twin of `DrDocument::puppet_mesh`.
    pub(crate) fn puppet_mesh(
        &self,
        layer: usize,
        coverage: Option<&[u16]>,
        options: &MeshOptions,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PuppetMesh, String> {
        let (image, rect) = self.mesh_warp_record(layer, coverage.is_none(), NAME)?;
        let opaque = |x, y| image.opaque(x, y);
        match coverage {
            None => {
                admit(0, mesh_plan_bytes(rect, options)?, other, limit)?;
                build(rect, opaque, options, job)
            }
            Some(coverage) => crate::selected_pixels_puppet::float_mesh(
                rect,
                self.dimensions(),
                coverage,
                opaque,
                options,
                limit,
                other,
                job,
            ),
        }
    }

    fn puppet_prepare(
        &self,
        request: &PuppetWarpRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(Rect, Prepared, Value), String> {
        let (image, rect) = self.mesh_warp_record(request.layer, true, NAME)?;
        let prepared = prepare(
            &request.spec,
            rect,
            |x, y| image.opaque(x, y),
            limit,
            other,
            job,
        )?;
        let common = common_scratch(rect, &request.spec.options)?;
        let ready = prepared.ready();
        let (retained, scratch) =
            self.mesh_warp_plan(request.layer, rect, common, ready.map(|(d, _)| d))?;
        let plan = plan_json(retained, scratch, ready, rect);
        Ok((rect, prepared, plan))
    }

    /// The typed twin of `DrDocument::puppet_warp_plan`. An Indexed Color
    /// document refuses for good (#547 Indexed phase 3a).
    pub fn puppet_warp_plan(
        &self,
        request: &PuppetWarpRequest,
        limit: u64,
        other: u64,
    ) -> Result<Value, String> {
        if self.is_indexed() {
            return Err(crate::indexed_document::refused_sentence("puppet_warp"));
        }
        Ok(self
            .puppet_prepare(request, limit, other, JobContext::NONE)?
            .2)
    }

    /// The typed twin of `DrDocument::puppet_warp`. An Indexed Color
    /// document refuses for good (#547 Indexed phase 3a).
    pub fn puppet_warp(
        &self,
        request: &PuppetWarpRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        if self.is_indexed() {
            return Err(crate::indexed_document::refused_sentence("puppet_warp"));
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let (rect, prepared, plan) = self.puppet_prepare(request, limit, other, job)?;
        let Prepared::Ready { pose, dest, .. } = prepared else {
            return Ok(None);
        };
        let retained = plan["retained_bytes"].as_u64().unwrap_or(0);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        admit(retained, scratch, other, limit)?;
        job.reserve(scratch)?;
        let map = PoseMap::new(&pose)?;
        let next = self.mesh_warp_through(request.layer, rect, &map, dest, job)?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
