//! Byte set perspective applies one solved canvas homography to every member.
//! All destinations and locks are checked before any member is rendered; the
//! immutable result is published only after every raster and mask succeeds.
//!
//! A controlled byte affine/perspective set polls its job through every member's
//! resample, linked/group mask and retained re-render (`transform_layers_with_job`, stage 6 per
//! member and per destination row of each raster resample inside that member's slice —
//! `LINKED_MASK_RASTER_SHARE` (0.8) of it when a linked mask follows, since the mask resample
//! reports no rows, so straighten layers report rows too; the job-less names are `NONE` wrappers),
//! `Err` only for the cancel (`core/tests/byte_geometry_cancel.rs`). The byte
//! whole-canvas `straighten` and `rotate_canvas` requests poll the same way
//! (`straighten_layers_with_job` through `transformed_entries`, then `transform_channels_with_job`
//! per channel; `rotate_canvas_with_job`), `s = L/(L+C)` of the range to the L layers and the rest
//! to the C channels; a full-window straighten is the straighten alone and an identity at the full
//! window is `None` (`core/tests/byte_canvas_geometry_cancel.rs`).

use image::imageops::FilterType;
use std::sync::Arc;

use crate::doc::{DrDocument, LayerKind, LayerRaster};
use crate::doc_lock::EditKind;
use crate::doc_perspective::{
    parallelogram_affine, solve_rect_to_quad, transformed_projective_bounds,
};
use crate::doc_transform::{
    resample_mask_outside, resample_rgba_with_context, SourceMap, EXACT_EPSILON,
    LINKED_MASK_RASTER_SHARE,
};
use crate::geometry_set::{expand, SetEntry};
use crate::job_control::JobContext;
use crate::precision_sampling::GeometryFilter;
use crate::source_transform::{self, SourceTransform};

impl DrDocument {
    pub(crate) fn expanded_geometry_set(&self, roots: &[usize]) -> Result<Vec<usize>, String> {
        let entries: Vec<_> = self
            .layers
            .iter()
            .map(|layer| SetEntry {
                depth: usize::from(layer.depth),
                group: layer.kind == LayerKind::Group,
                link: layer.link,
            })
            .collect();
        let selected = expand(&entries, roots)?;
        for &index in &selected {
            if self.lock_block(index, EditKind::Position) != 0 {
                return Err(format!("geometry set layer {index} is position locked"));
            }
        }
        Ok(selected)
    }

    pub(crate) fn perspective_set(
        &self,
        roots: &[usize],
        rect: [f64; 4],
        quad: [f64; 8],
        filter: GeometryFilter,
        job: JobContext<'_>,
        floor: u64,
    ) -> Result<Option<Self>, String> {
        let selected = self.expanded_geometry_set(roots)?;
        if !rect
            .iter()
            .chain(quad.iter())
            .all(|value| value.is_finite())
            || rect[2] <= 0.
            || rect[3] <= 0.
        {
            return Err("perspective set rectangle or corners are invalid".into());
        }
        let type_filter = filter_type(filter);
        if (quad[0] - quad[2] + quad[4] - quad[6]).abs() <= EXACT_EPSILON
            && (quad[1] - quad[3] + quad[5] - quad[7]).abs() <= EXACT_EPSILON
        {
            let matrix = parallelogram_affine((rect[0], rect[1], rect[2], rect[3]), &quad);
            if [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
                .iter()
                .zip([1., 0., 0., 1., 0., 0.])
                .all(|(a, b)| (*a - b).abs() <= EXACT_EPSILON)
            {
                return Ok(None);
            }
            return self.transform_layers_with_job(roots, matrix, type_filter, job, floor);
        }
        let forward = solve_rect_to_quad((rect[0], rect[1], rect[2], rect[3]), &quad)
            .ok_or("perspective set quad is singular, concave or crosses the horizon")?;
        let inverse = forward
            .visible_inverse()
            .ok_or("perspective set inverse is invalid")?;
        let mut pixel_dims = Vec::with_capacity(self.layers.len());
        let mut mask_dims = Vec::new();
        let mut destinations = vec![None; self.layers.len()];
        let mut mask_destinations = vec![None; self.layers.len()];
        for (index, layer) in self.layers.iter().enumerate() {
            job.check()?;
            let selected = selected.binary_search(&index).is_ok();
            let pixels = if selected
                && layer.kind == LayerKind::Raster
                && !is_adjustment(layer.meta.as_deref())
            {
                let source = layer.effective_source(&self.profile);
                let destination = if let Some(source) = source {
                    let map = source_transform::compose(
                        SourceTransform::Projective(forward.0),
                        source.transform,
                    )?;
                    source_transform::bounds(source.asset.dimensions(), map)?
                } else {
                    transformed_projective_bounds(
                        layer.pixels.dimensions(),
                        layer.offset,
                        &forward.0,
                    )
                    .ok_or(
                        "perspective set raster crosses the horizon or exceeds supported bounds",
                    )?
                };
                destinations[index] = Some(destination);
                (destination.2, destination.3)
            } else {
                layer.pixels.dimensions()
            };
            pixel_dims.push(pixels);
            if let Some(mask) = &layer.mask {
                let dimensions = if selected && layer.mask_properties.linked {
                    let destination = transformed_projective_bounds(
                        mask.dimensions(),
                        layer.mask_properties.offset,
                        &forward.0,
                    )
                    .ok_or(
                        "perspective set mask crosses the horizon or exceeds supported bounds",
                    )?;
                    if !layer.mask_properties.valid((destination.2, destination.3)) {
                        return Err(
                            "perspective set mask properties exceed supported bounds".into()
                        );
                    }
                    mask_destinations[index] = Some(destination);
                    (destination.2, destination.3)
                } else {
                    mask.dimensions()
                };
                mask_dims.push(dimensions);
            }
        }
        if !self.native_layers_fit_dimensions(pixel_dims)
            || !self.native_masks_fit_dimensions(mask_dims)
        {
            return Err("perspective set exceeds native layer or mask capacity".into());
        }
        let mut next = self.clone();
        let mut changed = false;
        let n = selected.len() as f64;
        for (k, &index) in selected.iter().enumerate() {
            job.report(k as f64 / n, 6)?;
            let member = job.range(k as f64 / n, (k + 1) as f64 / n);
            let old = &self.layers[index];
            let target = &mut next.layers[index];
            if let Some(destination) = destinations[index] {
                if let Some(mut source) = old.effective_source(&self.profile) {
                    source.transform = source_transform::compose(
                        SourceTransform::Projective(forward.0),
                        source.transform,
                    )?;
                    source.filter = filter;
                    source = crate::retained_source_render::render8(source, &self.profile, member)?;
                    target.pixels = source.rendered.clone();
                    target.offset = source.rendered_offset;
                    target.retained_source = Some(source);
                } else {
                    let map =
                        SourceMap::projective(&inverse, old.offset, (destination.0, destination.1));
                    let image = old.pixels.to_contiguous_with_job(member)?;
                    let rows = if mask_destinations[index].is_some() {
                        member.range(0.0, LINKED_MASK_RASTER_SHARE)
                    } else {
                        member
                    };
                    target.pixels = LayerRaster::unseeded(resample_rgba_with_context(
                        &image,
                        destination,
                        &map,
                        filter.sampler(),
                        true,
                        rows,
                    )?);
                    target.offset = (destination.0, destination.1);
                }
                let quad = projected_quad(&forward.0, old.offset, old.pixels.dimensions())?;
                target.style = crate::style::scaled_style(
                    &target.style,
                    crate::style::quad_mean_scale(quad, old.pixels.width(), old.pixels.height()),
                );
                changed = true;
            }
            if let (Some(mask), Some(destination)) = (&old.mask, mask_destinations[index]) {
                let map = SourceMap::projective(
                    &inverse,
                    old.mask_properties.offset,
                    (destination.0, destination.1),
                );
                target.mask = Some(Arc::new(resample_mask_outside(
                    mask,
                    destination,
                    &map,
                    filter.sampler(),
                    old.mask_properties.outside_coverage,
                    member,
                )?));
                target.mask_properties.offset = (destination.0, destination.1);
                changed = true;
            }
        }
        if !changed {
            return Ok(None);
        }
        next.native_save_preflight()?;
        Ok(Some(next))
    }
}

pub(crate) fn filter_type(filter: GeometryFilter) -> FilterType {
    match filter {
        GeometryFilter::Nearest => FilterType::Nearest,
        GeometryFilter::Bilinear => FilterType::Triangle,
        GeometryFilter::Bicubic => FilterType::CatmullRom,
        GeometryFilter::Lanczos3 => FilterType::Lanczos3,
    }
}

pub(crate) fn is_adjustment(meta: Option<&str>) -> bool {
    meta.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .is_some_and(|value| value["type"] == "adjust")
}

fn projected_quad(
    matrix: &[f64; 9],
    offset: (i32, i32),
    dimensions: (u32, u32),
) -> Result<[f64; 8], String> {
    let x = f64::from(offset.0);
    let y = f64::from(offset.1);
    let w = f64::from(dimensions.0);
    let h = f64::from(dimensions.1);
    let mut quad = [0.; 8];
    for (index, (x, y)) in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        .into_iter()
        .enumerate()
    {
        let denominator = matrix[6] * x + matrix[7] * y + matrix[8];
        if !denominator.is_finite() || denominator <= 0. {
            return Err("perspective set raster crosses the horizon".into());
        }
        quad[index * 2] = (matrix[0] * x + matrix[1] * y + matrix[2]) / denominator;
        quad[index * 2 + 1] = (matrix[3] * x + matrix[4] * y + matrix[5]) / denominator;
    }
    Ok(quad)
}
