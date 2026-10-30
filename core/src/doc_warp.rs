//! Byte document owner for persisted displacement fields.
use std::sync::Arc;

use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};

use crate::doc::{DrDocument, Layer, LayerKind, LayerRaster};
use crate::doc_lock::EditKind;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::admit;
use crate::retained_source::SourcePlan;
use crate::sampling::warp_into;
use crate::warp::{WarpBinding8, WarpEdit, WarpField, WarpTarget};

fn byte_rect(offset: (i32, i32), size: (u32, u32)) -> (i32, i32, u32, u32) {
    (offset.0, offset.1, size.0, size.1)
}

fn add_offset(offset: (i32, i32), delta: (i32, i32)) -> Result<(i32, i32), String> {
    Ok((
        offset
            .0
            .checked_add(delta.0)
            .ok_or("warp offset exceeds signed bounds")?,
        offset
            .1
            .checked_add(delta.1)
            .ok_or("warp offset exceeds signed bounds")?,
    ))
}

impl Layer {
    pub(crate) fn effective_warp(
        &self,
        profile: &Arc<crate::icc::IccProfile>,
    ) -> Option<WarpBinding8> {
        let binding = self.warp.as_ref()?;
        if !Arc::ptr_eq(profile, &binding.profile)
            || binding.source_mask.is_some()
                && self.mask_properties.outside_coverage != binding.mask_outside
            || self.mask_properties.linked != binding.linked_mask
                && binding.target == WarpTarget::Pixels
                && self.mask.is_some()
        {
            return None;
        }
        if let Some(rendered) = &binding.rendered_pixels {
            if !LayerRaster::ptr_eq(rendered, &self.pixels)
                || self.offset != binding.rendered_offset
                || self.meta.is_some()
                || self.retained_source.is_some()
            {
                return None;
            }
        }
        if let Some(rendered) = &binding.rendered_mask {
            let actual = self.mask.as_ref()?;
            if !Arc::ptr_eq(rendered, actual)
                || self.mask_properties.offset != binding.rendered_mask_offset
            {
                return None;
            }
        }
        Some(binding.clone())
    }
}

/// The binary64 premultiplied RGBA8 accumulation every byte warp walk
/// shares (the persisted warp and Puppet Warp).
pub(crate) fn accumulate_rgba8(acc: &mut [f64; 4], p: [u8; 4], weight: f64) {
    let a = f64::from(p[3]);
    let pm = weight * a / 255.0;
    for c in 0..3 {
        acc[c] += pm * f64::from(p[c]);
    }
    acc[3] += weight * a;
}

/// [`accumulate_rgba8`]'s finish: unpremultiply by the raw alpha, then
/// clamp and round once.
pub(crate) fn finish_rgba8(acc: [f64; 4]) -> [u8; 4] {
    let alpha = acc[3].clamp(0.0, 255.0).round() as u8;
    if alpha == 0 {
        return [0; 4];
    }
    let scale = 255.0 / acc[3];
    [
        (acc[0] * scale).clamp(0.0, 255.0).round() as u8,
        (acc[1] * scale).clamp(0.0, 255.0).round() as u8,
        (acc[2] * scale).clamp(0.0, 255.0).round() as u8,
        alpha,
    ]
}

/// The byte mask accumulation relative to the mask's `outside` coverage, so
/// a tap off the plane reads as outside.
pub(crate) fn accumulate_mask8(acc: &mut [f64; 1], p: u8, weight: f64, outside: u8) {
    acc[0] += weight * (f64::from(p) - f64::from(outside));
}

/// [`accumulate_mask8`]'s finish.
pub(crate) fn finish_mask8(acc: [f64; 1], outside: u8) -> u8 {
    (acc[0] + f64::from(outside)).clamp(0.0, 255.0).round() as u8
}

fn render_pixels(
    source: &LayerRaster,
    origin: (i32, i32),
    dest: (i32, i32, u32, u32),
    field: &WarpField,
    job: JobContext<'_>,
) -> Result<LayerRaster, String> {
    let (sw, sh) = source.dimensions();
    let image = source.to_contiguous_with_job(job)?;
    let mut out = vec![0u8; dest.2 as usize * dest.3 as usize * 4];
    warp_into(
        byte_rect(origin, (sw, sh)),
        dest,
        field,
        |i| {
            let p = &image.as_raw()[i * 4..i * 4 + 4];
            [p[0], p[1], p[2], p[3]]
        },
        accumulate_rgba8,
        finish_rgba8,
        |i, p| out[i * 4..i * 4 + 4].copy_from_slice(&p),
        job,
    )?;
    Ok(LayerRaster::unseeded(
        RgbaImage::from_raw(dest.2, dest.3, out)
            .ok_or("warp RGBA output dimensions are invalid")?,
    ))
}

fn render_mask(
    source: &Arc<GrayImage>,
    origin: (i32, i32),
    dest: (i32, i32, u32, u32),
    field: &WarpField,
    outside: u8,
    job: JobContext<'_>,
) -> Result<Arc<GrayImage>, String> {
    let mut out = vec![outside; dest.2 as usize * dest.3 as usize];
    warp_into(
        byte_rect(origin, source.dimensions()),
        dest,
        field,
        |i| source.as_raw()[i],
        |acc: &mut [f64; 1], p, weight| accumulate_mask8(acc, p, weight, outside),
        |acc| finish_mask8(acc, outside),
        |i, p| out[i] = p,
        job,
    )?;
    Ok(Arc::new(
        GrayImage::from_raw(dest.2, dest.3, out)
            .ok_or("warp mask output dimensions are invalid")?,
    ))
}

impl DrDocument {
    pub fn warp_info(&self, layer: usize) -> Result<Value, String> {
        let layer = self.layers.get(layer).ok_or("invalid warp layer index")?;
        let field = layer.effective_warp(&self.profile).map(|binding| {
            let rect = match binding.target {
                WarpTarget::Pixels => byte_rect(
                    binding.source_offset,
                    binding
                        .source_pixels
                        .as_ref()
                        .expect("validated warp pixels")
                        .dimensions(),
                ),
                WarpTarget::Mask => byte_rect(
                    binding.source_mask_offset,
                    binding
                        .source_mask
                        .as_ref()
                        .expect("validated warp mask")
                        .dimensions(),
                ),
            };
            binding
                .field
                .json(binding.target, rect, binding.linked_mask)
        });
        Ok(json!({"schema_version":1,"field":field}))
    }

    pub fn warp_plan(&self, edit: &WarpEdit) -> Result<SourcePlan, String> {
        let layer = self
            .layers
            .get(edit.layer())
            .ok_or("invalid warp layer index")?;
        let current = layer.effective_warp(&self.profile);
        let mut output = 0u64;
        let mut work = 0u64;
        if let WarpEdit::Set { target, field, .. } = edit {
            if current.as_ref().is_some_and(|b| b.target != *target) {
                return Err("clear or bake the current warp before switching target".into());
            }
            if *target == WarpTarget::Pixels {
                if layer.kind != LayerKind::Raster {
                    return Err("warp pixels require a raster layer".into());
                }
                if layer.meta.is_some() || layer.retained_source.is_some() {
                    return Err(
                        "rasterize the retained source or artwork before warping pixels".into(),
                    );
                }
                if self.lock_block(edit.layer(), EditKind::Position) != 0
                    || self.lock_block(edit.layer(), EditKind::Pixels) != 0
                {
                    return Err("warp pixels are blocked by a layer lock".into());
                }
            } else if self.lock_block(edit.layer(), EditKind::Mask) != 0 {
                return Err("warp mask is blocked by a layer lock".into());
            }
            let (size, offset) = match target {
                WarpTarget::Pixels => (
                    current
                        .as_ref()
                        .and_then(|b| b.source_pixels.as_ref())
                        .unwrap_or(&layer.pixels)
                        .dimensions(),
                    current.as_ref().map_or(layer.offset, |b| b.source_offset),
                ),
                WarpTarget::Mask => {
                    let mask = current
                        .as_ref()
                        .and_then(|b| b.source_mask.as_ref())
                        .or(layer.mask.as_ref())
                        .ok_or("warp target has no mask")?;
                    (
                        mask.dimensions(),
                        current
                            .as_ref()
                            .map_or(layer.mask_properties.offset, |b| b.source_mask_offset),
                    )
                }
            };
            let rect = byte_rect(offset, size);
            field.validate_for_rect(rect)?;
            let dest = field.bounds(rect)?;
            let exact = field.translation().is_some();
            if !exact {
                output = u64::from(dest.2)
                    * u64::from(dest.3)
                    * if *target == WarpTarget::Pixels { 4 } else { 1 };
                work = u64::from(dest.2)
                    * u64::from(dest.3)
                    * u64::from(field.filter.kernel().taps() as u32).pow(2)
                    * 64;
            }
            if *target == WarpTarget::Pixels && layer.mask.is_some() && layer.mask_properties.linked
            {
                let mask = current
                    .as_ref()
                    .and_then(|b| b.source_mask.as_ref())
                    .or(layer.mask.as_ref())
                    .ok_or("warp linked mask is missing")?;
                let rect = byte_rect(
                    current
                        .as_ref()
                        .map_or(layer.mask_properties.offset, |b| b.source_mask_offset),
                    mask.dimensions(),
                );
                let dest = field.bounds(rect)?;
                if !exact {
                    output = output.saturating_add(u64::from(dest.2) * u64::from(dest.3));
                }
                if !exact {
                    work = work.saturating_add(u64::from(dest.2) * u64::from(dest.3) * 64);
                }
            }
            if field.identity() && current.is_none()
                || current.as_ref().is_some_and(|b| b.field.equivalent(field))
            {
                return Ok(SourcePlan {
                    retained_bytes: 0,
                    scratch_bytes: 0,
                    work_units: 0,
                });
            }
        } else if current.is_none() {
            return Ok(SourcePlan {
                retained_bytes: 0,
                scratch_bytes: 0,
                work_units: 0,
            });
        }
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        let retained = inventory
            .unique_bytes()
            .saturating_add(output)
            .saturating_add(64 * 1024);
        Ok(SourcePlan {
            retained_bytes: retained,
            scratch_bytes: output.saturating_add(64 * 1024),
            work_units: work,
        })
    }

    pub fn warp_edit(
        &self,
        edit: &WarpEdit,
        limit: u64,
        other_retained_bytes: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let plan = self.warp_plan(edit)?;
        if plan.scratch_bytes == 0 {
            return Ok(None);
        }
        admit(
            plan.retained_bytes,
            plan.scratch_bytes,
            other_retained_bytes,
            limit,
        )?;
        job.reserve(plan.scratch_bytes)?;
        let index = edit.layer();
        let layer = &self.layers[index];
        let current = layer.effective_warp(&self.profile);
        let mut next = self.clone();
        let target_layer = &mut next.layers[index];
        match edit {
            WarpEdit::Clear { .. } => {
                let Some(binding) = current else {
                    return Ok(None);
                };
                if let Some(p) = binding.source_pixels {
                    target_layer.pixels = p;
                    target_layer.offset = binding.source_offset;
                }
                if let Some(m) = binding.source_mask {
                    target_layer.mask = Some(m);
                    target_layer.mask_properties.offset = binding.source_mask_offset;
                }
                target_layer.warp = None;
            }
            WarpEdit::Bake { .. } => {
                if current.is_none() {
                    return Ok(None);
                }
                target_layer.warp = None;
            }
            WarpEdit::Set { target, field, .. } => {
                if field.identity() {
                    if let Some(binding) = current {
                        if let Some(p) = binding.source_pixels {
                            target_layer.pixels = p;
                            target_layer.offset = binding.source_offset;
                        }
                        if let Some(m) = binding.source_mask {
                            target_layer.mask = Some(m);
                            target_layer.mask_properties.offset = binding.source_mask_offset;
                        }
                        target_layer.warp = None;
                        return Ok(Some(next));
                    }
                    return Ok(None);
                }
                let mut normalized = field.clone();
                normalized.normalize();
                if current
                    .as_ref()
                    .is_some_and(|b| b.field.equivalent(&normalized))
                {
                    return Ok(None);
                }
                let source_pixels = if *target == WarpTarget::Pixels {
                    Some(
                        current
                            .as_ref()
                            .and_then(|b| b.source_pixels.clone())
                            .unwrap_or_else(|| layer.pixels.clone()),
                    )
                } else {
                    None
                };
                let source_mask = if *target == WarpTarget::Mask
                    || layer.mask.is_some() && layer.mask_properties.linked
                {
                    Some(
                        current
                            .as_ref()
                            .and_then(|b| b.source_mask.clone())
                            .or_else(|| layer.mask.clone())
                            .ok_or("warp target has no mask")?,
                    )
                } else {
                    None
                };
                let source_offset = current.as_ref().map_or(layer.offset, |b| b.source_offset);
                let source_mask_offset = current
                    .as_ref()
                    .map_or(layer.mask_properties.offset, |b| b.source_mask_offset);
                let linked = *target == WarpTarget::Pixels && source_mask.is_some();
                let mut binding = WarpBinding8 {
                    field: Arc::new(normalized),
                    target: *target,
                    source_pixels,
                    source_offset,
                    source_mask,
                    source_mask_offset,
                    rendered_pixels: None,
                    rendered_offset: (0, 0),
                    rendered_mask: None,
                    rendered_mask_offset: (0, 0),
                    profile: self.profile.clone(),
                    linked_mask: linked,
                    mask_outside: layer.mask_properties.outside_coverage,
                };
                if let Some(p) = &binding.source_pixels {
                    let rect = byte_rect(source_offset, p.dimensions());
                    if let Some(delta) = binding.field.translation() {
                        target_layer.pixels = p.clone();
                        target_layer.offset = add_offset(source_offset, delta)?;
                    } else {
                        let dest = binding.field.bounds(rect)?;
                        target_layer.pixels =
                            render_pixels(p, source_offset, dest, &binding.field, job)?;
                        target_layer.offset = (dest.0, dest.1);
                    }
                    binding.rendered_pixels = Some(target_layer.pixels.clone());
                    binding.rendered_offset = target_layer.offset;
                }
                if let Some(m) = &binding.source_mask {
                    let rect = byte_rect(source_mask_offset, m.dimensions());
                    if let Some(delta) = binding.field.translation() {
                        target_layer.mask = Some(m.clone());
                        target_layer.mask_properties.offset =
                            add_offset(source_mask_offset, delta)?;
                    } else {
                        let dest = binding.field.bounds(rect)?;
                        target_layer.mask = Some(render_mask(
                            m,
                            source_mask_offset,
                            dest,
                            &binding.field,
                            binding.mask_outside,
                            job,
                        )?);
                        target_layer.mask_properties.offset = (dest.0, dest.1);
                    }
                    binding.rendered_mask = target_layer.mask.clone();
                    binding.rendered_mask_offset = target_layer.mask_properties.offset;
                }
                target_layer.warp = Some(binding);
            }
        }
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
