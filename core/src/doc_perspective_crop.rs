//! Perspective Crop on the byte document: the thin adapter over
//! `perspective_crop`'s request and homography, resampling through
//! `doc_transform`'s shared `SourceMap::projective` / `resample_rgba` /
//! `resample_mask_outside` walks — the ones Edit ▸ Transform ▸ Perspective
//! and the Crop tool's straighten use.
//!
//! The result is the request's `W × H` canvas. Every raster that is not an
//! adjustment layer becomes a canvas-sized plane at `(0, 0)`, every mask
//! (linked or not, on a raster, a group or an adjustment — the straighten's
//! "all masks" rule, since the whole picture moves) a canvas-sized plane at
//! `(0, 0)` reading its outside coverage where the quad leaves the source,
//! and every saved channel the same with 0 outside. Guides are cleared and
//! the ruler origin returns to `(0, 0)`: neither has a meaning through a
//! perspective map. Colour samplers follow the picture through the inverse
//! map and drop off the new canvas. Layer comps take Crop's rule: a moved
//! raster's recorded offset shifts by the same amount its layer moved. Styles
//! on moved rasters scale by `PerspectiveCropRequest::style_factor`. Layer
//! `meta` is kept: the host drops a described layer's description inside
//! the same edit, as it does for every destructive path.
//!
//! Lock-exempt like every other whole-canvas op (`doc_align::
//! straighten_layers`' note). A layer with a retained source refuses the
//! whole op with `perspective_crop::SOURCE_LAYERS`.

use std::sync::Arc;

use crate::doc::{translated_offset, DrDocument, LayerKind, LayerRaster};
use crate::doc_geometry_set::is_adjustment;
use crate::doc_transform::{resample_mask_outside, resample_rgba_with_context, SourceMap};
use crate::job_control::{DrJobControl, JobContext};
use crate::perspective_crop::{plan_scratch, PerspectiveCropRequest, SOURCE_LAYERS};
use crate::precision_image::admit;
use crate::retained_source::SourcePlan;
use crate::style::scaled_style;

/// Whether the op resamples this entry's pixels: a raster that is not an
/// adjustment layer.
fn moves(kind: LayerKind, meta: Option<&str>) -> bool {
    kind == LayerKind::Raster && !is_adjustment(meta)
}

impl DrDocument {
    /// The plan of [`Self::perspective_crop`] as the shared plan JSON
    /// (`retained_bytes`, `scratch_bytes`, `work_units`). `Err` with the
    /// request's own sentence for every refusal `perspective_crop` gives.
    pub fn perspective_crop_plan(
        &self,
        request: &PerspectiveCropRequest,
    ) -> Result<serde_json::Value, String> {
        Ok(self.perspective_crop_source_plan(request)?.json())
    }

    fn perspective_crop_source_plan(
        &self,
        request: &PerspectiveCropRequest,
    ) -> Result<SourcePlan, String> {
        request.source_map()?;
        // The whole canvas at its own size moves nothing, so there is nothing
        // to refuse and nothing to charge beyond the fixed slack: the edit
        // answers `None`, with no error, whatever the layers hold.
        if request.is_identity((self.width, self.height)) {
            let mut inventory = crate::resources::Inventory::default();
            inventory.document(self);
            return Ok(SourcePlan {
                retained_bytes: inventory.unique_bytes(),
                scratch_bytes: plan_scratch(0, 0, 0),
                work_units: 0,
            });
        }
        if self.has_any_effective_source() {
            return Err(SOURCE_LAYERS.into());
        }
        let (w, h) = (request.width, request.height);
        let pixels = request.pixels();
        if !crate::doc_channel::channels_fit_at(w, h, self.channels.len()) {
            return Err(
                "the perspective crop canvas would exceed the saved-channel pixel budget".into(),
            );
        }
        let dims = self.layers.iter().map(|l| {
            if moves(l.kind, l.meta.as_deref()) {
                (w, h)
            } else {
                l.pixels.dimensions()
            }
        });
        let mask_dims: Vec<(u32, u32)> = self
            .layers
            .iter()
            .filter(|l| l.mask.is_some())
            .map(|_| (w, h))
            .collect();
        if !self.native_layers_fit_dimensions(dims) || !self.native_masks_fit_dimensions(mask_dims)
        {
            return Err("the perspective crop result exceeds the native document capacity".into());
        }
        if self
            .layers
            .iter()
            .any(|l| l.mask.is_some() && !l.mask_properties.valid((w, h)))
        {
            return Err("a layer mask cannot be carried onto the perspective crop canvas".into());
        }
        let mut destinations = pixels.saturating_mul(self.channels.len() as u64);
        let mut transient = 0_u64;
        let mut work = 0_u64;
        let taps = request.filter.taps();
        for layer in &self.layers {
            if moves(layer.kind, layer.meta.as_deref()) {
                destinations = destinations.saturating_add(pixels.saturating_mul(4));
                // The contiguous source copy and the contiguous output are
                // both live while the output is tiled into the stored plane.
                let (lw, lh) = layer.pixels.dimensions();
                let source = u64::from(lw) * u64::from(lh) * 4;
                transient = transient.max(source.saturating_add(pixels.saturating_mul(4)));
                work = work.saturating_add(pixels.saturating_mul(taps * taps));
            }
            if layer.mask.is_some() {
                destinations = destinations.saturating_add(pixels);
                work = work.saturating_add(pixels.saturating_mul(taps * taps));
            }
        }
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        Ok(SourcePlan {
            retained_bytes: inventory.unique_bytes(),
            scratch_bytes: plan_scratch(destinations, transient, self.cloned_structure_bytes()),
            work_units: work,
        })
    }

    /// Perspective Crop: the canvas becomes `request.width × request.height`
    /// and the source quad is resampled onto it (module doc). Runs
    /// `geometry_edit`'s prologue — begin the run, plan, admit against
    /// `limit` beside `other_retained`, reserve, check — before any pixel
    /// work. `Ok(None)` for the whole canvas at its own size, decided right
    /// after the request is validated (before the source refusal and
    /// admission); `Err` for every other refusal and for a cancel.
    pub fn perspective_crop(
        &self,
        request: &PerspectiveCropRequest,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        // Validate the request, then answer the identity before the source
        // refusal and admission: a request that changes nothing is `None`
        // with no error, never a refusal.
        let g = request.source_map()?;
        if request.is_identity((self.width, self.height)) {
            return Ok(None);
        }
        let plan = self.perspective_crop_source_plan(request)?;
        admit(
            plan.retained_bytes,
            plan.scratch_bytes,
            other_retained,
            limit,
        )?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let sampler = request.filter.sampler();
        let (w, h) = (request.width, request.height);
        let dest = (0, 0, w, h);
        let factor = request.style_factor();
        let planes = self.layers.iter().filter(|l| l.mask.is_some()).count()
            + self
                .layers
                .iter()
                .filter(|l| moves(l.kind, l.meta.as_deref()))
                .count()
            + self.channels.len();
        let n = planes.max(1) as f64;
        let mut done = 0.0;
        let mut step = || {
            let range = job.range(done / n, (done + 1.0) / n);
            done += 1.0;
            range
        };
        let mut doc = self.clone();
        for (i, layer) in self.layers.iter().enumerate() {
            job.check()?;
            let target = &mut doc.layers[i];
            if moves(layer.kind, layer.meta.as_deref()) {
                let image = layer.pixels.to_contiguous_with_job(job)?;
                let map = SourceMap::projective(&g, layer.offset, (0, 0));
                let out = resample_rgba_with_context(&image, dest, &map, sampler, false, step())?;
                drop(image);
                target.pixels = LayerRaster::unseeded(out);
                target.offset = (0, 0);
                target.style = scaled_style(&layer.style, factor);
            }
            if let Some(raw) = &layer.mask {
                let map = SourceMap::projective(&g, layer.mask_properties.offset, (0, 0));
                let out = resample_mask_outside(
                    raw,
                    dest,
                    &map,
                    sampler,
                    layer.mask_properties.outside_coverage,
                    step(),
                )?;
                target.mask = Some(Arc::new(out));
                target.mask_properties.offset = (0, 0);
            }
        }
        let canvas_map = SourceMap::projective(&g, (0, 0), (0, 0));
        for channel in &mut doc.channels {
            let out = resample_mask_outside(&channel.data, dest, &canvas_map, sampler, 0, step())?;
            channel.data = Arc::new(out);
        }
        let to_destination = request.destination_map()?;
        doc.color_samplers =
            crate::doc_sampler::mapped_dropping(&self.color_samplers, w, h, to_destination);
        doc.paths.set_entries(crate::doc_path::mapped(
            &self.paths.entries,
            request.destination_map()?,
        ));
        doc.comps = self.remapped_comps(
            |layer, rec| {
                moves(layer.kind, layer.meta.as_deref()).then(|| {
                    translated_offset(rec, -i64::from(layer.offset.0), -i64::from(layer.offset.1))
                })
            },
            None,
            |layer| {
                if moves(layer.kind, layer.meta.as_deref()) {
                    factor
                } else {
                    1.0
                }
            },
        );
        doc.width = w;
        doc.height = h;
        doc.guides = Vec::new();
        doc.ruler_origin = (0.0, 0.0);
        job.check()?;
        Ok(Some(doc))
    }
}
