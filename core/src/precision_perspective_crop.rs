//! Perspective Crop on the typed document, 16-bit and 32-bit float:
//! the twin of `doc_perspective_crop`, whose module doc is the contract,
//! resampling through `precision_sampling::transform_image` /
//! `transform_plane_outside` with `perspective_crop`'s one homography.
//!
//! Every `Content::Raster` record (and, #1 phase 4b, every CMYK document's
//! `Content::CmykRaster` through `transform_cmyk`) becomes a canvas-sized plane at `(0, 0)`
//! and DROPS its artwork description (as typed Straighten does: resampled
//! pixels are no longer the rendering of the description). Every mask, on
//! any record, becomes a canvas-sized plane reading its outside coverage
//! beyond the source; every saved channel the same with 0 outside. Groups,
//! Pass Through groups and adjustments keep everything but their masks.
//! Chrome, samplers, comps and Scale Effects follow the byte rule.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::doc_transform::SourceMap;
use crate::job_control::JobContext;
use crate::perspective_crop::{plan_scratch, PerspectiveCropRequest, SOURCE_LAYERS};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionStorage};
use crate::precision_mask::PrecisionMask;
use crate::precision_sampling::{
    adoption_copy_bytes, cmyk_adoption_copy_bytes, cmyk_source_copy_bytes,
    float_adoption_copy_bytes, float_source_copy_bytes, source_copy_bytes, transform_cmyk,
    transform_image, transform_plane_outside,
};

/// Bytes the typed plan charges: retained and scratch.
struct Plan {
    retained: u64,
    scratch: u64,
}

/// Every record of `layers`, depth first.
fn each<'a>(layers: &'a [PrecisionDocumentLayer], out: &mut Vec<&'a PrecisionDocumentLayer>) {
    for layer in layers {
        if let Content::Group(children) | Content::PassThrough(children) = &layer.content {
            each(children, out);
        }
        out.push(layer);
    }
}

/// The values one resample walk shares.
struct Walk<'a> {
    map: [f64; 9],
    size: (u32, u32),
    request: &'a PerspectiveCropRequest,
    factor: f64,
    /// The document's linear-light tables (`PrecisionDocument::linear_codes`).
    codes: Option<&'a dyn crate::sampling_linear::LinearCodes<u16>>,
    job: JobContext<'a>,
}

impl Walk<'_> {
    fn layers(
        &self,
        layers: &[PrecisionDocumentLayer],
    ) -> Result<Vec<PrecisionDocumentLayer>, String> {
        let mut result = Vec::with_capacity(layers.len());
        for layer in layers {
            self.job.check()?;
            let mut next = layer.clone();
            match &layer.content {
                // A CMYK document's ink (#1 phase 4b): the raster arm below
                // through `transform_cmyk`, `transform_image`'s CMYK twin.
                Content::CmykRaster { image, offset } => {
                    let map = SourceMap::projective(&self.map, *offset, (0, 0));
                    let image =
                        transform_cmyk(image, self.size, &map, self.request.filter, self.job)?;
                    next.content = Content::CmykRaster {
                        image,
                        offset: (0, 0),
                    };
                    next.description = None;
                    next.style = crate::style::scaled_style(&layer.style, self.factor);
                }
                Content::Group(children) => {
                    next.content = Content::Group(Arc::new(self.layers(children)?));
                }
                Content::PassThrough(children) => {
                    next.content = Content::PassThrough(Arc::new(self.layers(children)?));
                }
                Content::Raster { image, offset } => {
                    let map = SourceMap::projective(&self.map, *offset, (0, 0));
                    let image = transform_image(
                        image,
                        self.size,
                        &map,
                        self.request.filter,
                        self.codes,
                        self.job,
                    )?;
                    next.content = Content::Raster {
                        image,
                        offset: (0, 0),
                    };
                    next.description = None;
                    next.style = crate::style::scaled_style(&layer.style, self.factor);
                }
                Content::Adjustment(_) | Content::CmykAdjustment(_) => {}
            }
            if let Some(mask) = &layer.mask {
                let map = SourceMap::projective(&self.map, mask.properties.offset, (0, 0));
                let plane = transform_plane_outside(
                    &mask.plane,
                    self.size,
                    &map,
                    self.request.filter,
                    u16::from(mask.properties.outside_coverage) * 257,
                    self.job,
                )?;
                let mut properties = mask.properties.clone();
                properties.offset = (0, 0);
                next.mask = Some(PrecisionMask::new(plane, properties, mask.enabled)?);
            }
            result.push(next);
        }
        Ok(result)
    }
}

impl PrecisionDocument {
    /// The plan JSON `dr_precision_doc_edit_plan` answers for a
    /// `perspective_crop` request: `retained_bytes`, `scratch_bytes`,
    /// `input_samples` 0.
    pub fn perspective_crop_plan(&self, request: &PerspectiveCropRequest) -> Result<Value, String> {
        let plan = self.perspective_crop_bytes(request)?;
        Ok(json!({
            "retained_bytes": plan.retained,
            "scratch_bytes": plan.scratch,
            "input_samples": 0,
        }))
    }

    fn perspective_crop_bytes(&self, request: &PerspectiveCropRequest) -> Result<Plan, String> {
        request.source_map()?;
        // The byte adapter's rule (`doc_perspective_crop`): the whole canvas
        // at its own size moves nothing, so it is never refused and charges
        // only the fixed slack. `dr_precision_doc_edit` plans before it
        // edits, so the identity has to be answered here too.
        if request.is_identity(self.dimensions()) {
            return Ok(Plan {
                retained: self.retained_bytes(),
                scratch: plan_scratch(0, 0, 0),
            });
        }
        let mut records = Vec::with_capacity(self.records);
        each(&self.layers, &mut records);
        if records.iter().any(|l| l.effective_source().is_some()) {
            return Err(SOURCE_LAYERS.into());
        }
        let size = (request.width, request.height);
        let pixels = request.pixels();
        let mut rasters = Vec::new();
        let mut masks = Vec::new();
        let mut destinations = pixels.saturating_mul(2 * self.channels.len() as u64);
        // A plane's resample holds its contiguous output while it is tiled.
        let mut transient = pixels.saturating_mul(2);
        for layer in &records {
            if let Content::Raster { image, .. } = &layer.content {
                rasters.push(size);
                let (copy, adoption) = if image.storage() == PrecisionStorage::Rgba32Float {
                    (
                        float_source_copy_bytes(image.dimensions())?,
                        float_adoption_copy_bytes(size)?,
                    )
                } else {
                    (
                        source_copy_bytes(image.dimensions())?,
                        adoption_copy_bytes(size)?,
                    )
                };
                destinations = destinations.saturating_add(adoption);
                transient = transient.max(copy.saturating_add(adoption));
            }
            // A CMYK raster (#1 phase 4b): the same two copies at 10 B/px.
            if let Content::CmykRaster { image, .. } = &layer.content {
                rasters.push(size);
                let adoption = cmyk_adoption_copy_bytes(size)?;
                destinations = destinations.saturating_add(adoption);
                transient = transient
                    .max(cmyk_source_copy_bytes(image.dimensions())?.saturating_add(adoption));
            }
            if let Some(mask) = &layer.mask {
                if !mask.properties.valid(size) {
                    return Err(
                        "a layer mask cannot be carried onto the perspective crop canvas".into(),
                    );
                }
                masks.push(size);
                destinations = destinations.saturating_add(pixels.saturating_mul(2));
            }
        }
        self.native_fits_geometry(size, &rasters, &masks, self.channels.len())?;
        // `DrDocument::cloned_structure_bytes`' formula (the master) over the
        // typed records: three record lengths plus the names.
        let structure = records.iter().fold(
            (3 * records.len() * std::mem::size_of::<PrecisionDocumentLayer>()) as u64,
            |sum, l| sum.saturating_add(l.name.len() as u64),
        );
        Ok(Plan {
            retained: self.retained_bytes(),
            scratch: plan_scratch(destinations, transient, structure),
        })
    }

    /// Perspective Crop at 16 bits and float (module doc): plan, admit
    /// against `limit` beside `other`, reserve and check before any pixel
    /// work. `Ok(None)` for the whole canvas at its own size.
    pub(crate) fn perspective_crop(
        &self,
        request: &PerspectiveCropRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // Validate, then the identity before the source refusal and
        // admission, as the byte adapter does.
        let map = request.source_map()?;
        if request.is_identity(self.dimensions()) {
            return Ok(None);
        }
        let plan = self.perspective_crop_bytes(request)?;
        admit(plan.retained, plan.scratch, other, limit)?;
        job.reserve(plan.scratch)?;
        job.check()?;
        let size = (request.width, request.height);
        let walk = Walk {
            map,
            size,
            request,
            factor: request.style_factor(),
            codes: self.linear_codes(),
            job,
        };
        let layers = walk.layers(&self.layers)?;
        let canvas_map = SourceMap::projective(&walk.map, (0, 0), (0, 0));
        let mut channels = Vec::with_capacity(self.channels.len());
        for channel in self.channels.iter() {
            let mut next = channel.clone();
            next.plane =
                transform_plane_outside(&channel.plane, size, &canvas_map, request.filter, 0, job)?;
            channels.push(next);
        }
        job.check()?;
        let mut result = self.rebuilt(size.0, size.1, layers, channels)?;
        result.guides = Vec::new();
        result.ruler_origin = (0.0, 0.0);
        result.color_samplers = crate::doc_sampler::mapped_dropping(
            &self.color_samplers,
            size.0,
            size.1,
            request.destination_map()?,
        );
        result.paths.set_entries(crate::doc_path::mapped(
            &self.paths.entries,
            request.destination_map()?,
        ));
        result.metadata = crate::tiff_paths::canvas_moved(&self.metadata);
        let raster_offset = |i: usize| match self.layer(i).map(|l| &l.content) {
            Some(Content::Raster { offset, .. } | Content::CmykRaster { offset, .. }) => {
                Some(*offset)
            }
            _ => None,
        };
        result.comps = self.remapped_comps(
            |i, rec| {
                let offset = raster_offset(i)?;
                Some(crate::doc::translated_offset(
                    rec,
                    -i64::from(offset.0),
                    -i64::from(offset.1),
                ))
            },
            None,
            |i| {
                if raster_offset(i).is_some() {
                    walk.factor
                } else {
                    1.0
                }
            },
        );
        job.report(1.0, 5)?;
        Ok(Some(result))
    }
}
