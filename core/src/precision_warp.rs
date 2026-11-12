//! Typed document owner for displacement fields. At 16 bits a field is
//! persisted beside its originals; at 32-bit float a set is applied at
//! once and keeps no binding (11F: a float layer carries no warp), so
//! `warp_info` reports `null`, clear and bake are no-ops and each set warps the
//! current pixels again. The float pixel arm accumulates premultiplied
//! extended-range samples in binary64 and finishes through
//! `sampling::straight_extended`: an alpha below 2^-24 becomes transparent and
//! RGB is clamped to the source layer's own visible range
//! (`precision_sampling::visible_range`, `float_document`'s D3).
//!
//! A CMYK document (#1 phase 4b) takes the float rule through `cmyk_warp`:
//! applied at once, no binding. A Grayscale document (#1 phase 4) takes the
//! same rule here: its set warps R = G = B through this kernel and drops the
//! binding in the same edit, so `warp_info` reports `null`. A Lab Color
//! document (#1 Lab phase 4a-ii) takes Gray's arm: its set warps the
//! L\*a\*b\* codes through this kernel and keeps no binding either. An
//! Indexed Color document (#1 Indexed phase 2d) runs a set on its RGB
//! storage (or its index carrier) through `warp_edit_with_context` and is
//! re-indexed by `indexed_transform`, with no binding.
//!
//! At 32-bit float `precision_warp`'s
//! float arm accumulates premultiplied extended-range samples in binary64
//! (`accumulate_extended_f64`, whose master is `sampling::accumulate_extended`) and finishes
//! through `sampling::straight_extended` against `precision_sampling::visible_range` (D2/D3); the
//! plan charges the f32 output at 16 B/px plus the float source copy and adoption;
//! `warp_native::typed_capacity`'s float refusal stays as the backstop (`tests/float_warp.rs`).
use std::sync::Arc;

use serde_json::{json, Value};

use crate::doc_lock::EditKind;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage};
use crate::precision_mask::PrecisionPlane;
use crate::precision_sampling::{
    float_adoption_copy_bytes, float_source_copy_bytes, visible_range,
};
use crate::precision_structure_ops::{flatten, rebuild};
use crate::retained_source::SourcePlan;
use crate::sampling::{straight_extended, warp_into};
use crate::warp::{WarpBinding16, WarpEdit, WarpField, WarpTarget};

pub(crate) fn rect(offset: (i32, i32), size: (u32, u32)) -> (i32, i32, u32, u32) {
    (offset.0, offset.1, size.0, size.1)
}
pub(crate) fn shifted(offset: (i32, i32), delta: (i32, i32)) -> Result<(i32, i32), String> {
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
fn same_image(a: &PrecisionImage, b: &PrecisionImage) -> bool {
    a.storage() == crate::precision_image::PrecisionStorage::Rgba16Unorm
        && PrecisionImage::ptr_eq(a, b)
}
impl PrecisionDocumentLayer {
    pub(crate) fn effective_warp(
        &self,
        profile: &Arc<crate::icc::IccProfile>,
    ) -> Option<WarpBinding16> {
        let b = self.warp.as_ref()?;
        if !Arc::ptr_eq(profile, &b.profile)
            || b.source_mask.is_some()
                && self
                    .mask
                    .as_ref()
                    .map_or(0, |m| m.properties.outside_coverage)
                    != b.mask_outside
        {
            return None;
        }
        if b.target == WarpTarget::Pixels
            && self.mask.is_some()
            && self.mask.as_ref()?.properties.linked != b.linked_mask
        {
            return None;
        }
        if let Some(p) = &b.rendered_pixels {
            if !same_image(self.source()?, p)
                || self.offset() != b.rendered_offset
                || self.description.is_some()
                || self.retained_source.is_some()
            {
                return None;
            }
        }
        if let Some(m) = &b.rendered_mask {
            let actual = self.mask.as_ref()?;
            if !Arc::ptr_eq(&actual.plane.pixels, &m.pixels)
                || actual.properties.offset != b.rendered_mask_offset
            {
                return None;
            }
        }
        Some(b.clone())
    }
}
/// The warped image. `transfer` is a linear-light document's
/// (`sampling_linear`): `Some` interpolates u16 colour in premultiplied
/// LINEAR light; a float source is linear already.
fn image(
    source: &PrecisionImage,
    origin: (i32, i32),
    dest: (i32, i32, u32, u32),
    field: &WarpField,
    transfer: Option<&dyn crate::sampling_linear::LinearCodes<u16>>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    if source.storage() == PrecisionStorage::Rgba32Float {
        let samples = source.transient_rgba32()?;
        let range = visible_range(samples.iter().copied(), job)?;
        let mut out = vec![[0f32; 4]; dest.2 as usize * dest.3 as usize];
        warp_into(
            rect(origin, source.dimensions()),
            dest,
            field,
            |i| samples[i],
            accumulate_extended_f64,
            |acc| straight_extended(acc.map(|v| v as f32), &range),
            |i, p| out[i] = p,
            job,
        )?;
        return PrecisionImage::from_rgba32_extended(
            dest.2,
            dest.3,
            out,
            source.profile().clone(),
            None,
        );
    }
    let samples = source.transient_rgba16()?;
    let mut out = vec![[0u16; 4]; dest.2 as usize * dest.3 as usize];
    let source_rect = rect(origin, source.dimensions());
    match transfer {
        None => warp_into(
            source_rect,
            dest,
            field,
            |i| samples[i],
            accumulate_rgba16,
            finish_rgba16,
            |i, p| out[i] = p,
            job,
        )?,
        Some(codes) => warp_into(
            source_rect,
            dest,
            field,
            |i| samples[i],
            |acc, p, weight| crate::sampling_linear::accumulate_linear_f64(acc, p, weight, codes),
            |acc| crate::sampling_linear::straight_linear_f64(acc, codes),
            |i, p| out[i] = p,
            job,
        )?,
    }
    PrecisionImage::from_rgba16_unseeded(dest.2, dest.3, out, source.profile().clone(), None)
}
/// The binary64 premultiplied RGBA16 accumulation every typed warp walk
/// shares (the persisted warp and Puppet Warp).
pub(crate) fn accumulate_rgba16(acc: &mut [f64; 4], p: [u16; 4], weight: f64) {
    accumulate_planes16::<4>(acc, p, weight);
}

/// [`accumulate_rgba16`] over any number of planes, alpha LAST: the same
/// binary64 operations in the same order (C = 4 is RGBA bit for bit, C = 5
/// a CMYK document's CMYKA16 ink, #1 phase 4b).
pub(crate) fn accumulate_planes16<const C: usize>(acc: &mut [f64; C], p: [u16; C], weight: f64) {
    let a = f64::from(p[C - 1]);
    let pm = weight * a / 65535.0;
    for c in 0..C - 1 {
        acc[c] += pm * f64::from(p[c]);
    }
    acc[C - 1] += weight * a;
}

/// [`accumulate_rgba16`]'s finish: unpremultiply by the raw alpha, then
/// clamp and round once.
pub(crate) fn finish_rgba16(acc: [f64; 4]) -> [u16; 4] {
    finish_planes16::<4>(acc)
}

/// [`finish_rgba16`] over any number of planes, alpha LAST.
pub(crate) fn finish_planes16<const C: usize>(acc: [f64; C]) -> [u16; C] {
    let a = acc[C - 1].clamp(0.0, 65535.0).round() as u16;
    if a == 0 {
        return [0; C];
    }
    let scale = 65535.0 / acc[C - 1];
    let mut out = [a; C];
    for c in 0..C - 1 {
        out[c] = (acc[c] * scale).clamp(0.0, 65535.0).round() as u16;
    }
    out
}

/// The u16 mask accumulation relative to the mask's `outside` coverage.
pub(crate) fn accumulate_mask16(acc: &mut [f64; 1], p: u16, weight: f64, outside: u16) {
    acc[0] += weight * (f64::from(p) - f64::from(outside));
}

/// [`accumulate_mask16`]'s finish.
pub(crate) fn finish_mask16(acc: [f64; 1], outside: u16) -> u16 {
    (acc[0] + f64::from(outside)).clamp(0.0, 65535.0).round() as u16
}

/// The binary64 twin of `sampling::accumulate_extended`, the master copy:
/// the same premultiplied accumulation over straight coverage, in the f64
/// accumulator `warp_into` hands every adapter.
pub(crate) fn accumulate_extended_f64(acc: &mut [f64; 4], pixel: [f32; 4], weight: f64) {
    let pm = weight * f64::from(pixel[3]);
    for c in 0..3 {
        acc[c] += pm * f64::from(pixel[c]);
    }
    acc[3] += weight * f64::from(pixel[3]);
}
/// A mask plane through `field` relative to its `outside` coverage (the
/// persisted warp's, the float warp's and, #1 phase 4b, the CMYK warp's).
pub(crate) fn mask(
    source: &PrecisionPlane,
    origin: (i32, i32),
    dest: (i32, i32, u32, u32),
    field: &WarpField,
    outside: u16,
    job: JobContext<'_>,
) -> Result<PrecisionPlane, String> {
    let mut out = vec![outside; dest.2 as usize * dest.3 as usize];
    warp_into(
        rect(origin, source.dimensions()),
        dest,
        field,
        |i| source.samples()[i],
        |acc: &mut [f64; 1], p, weight| accumulate_mask16(acc, p, weight, outside),
        |acc| finish_mask16(acc, outside),
        |i, p| out[i] = p,
        job,
    )?;
    PrecisionPlane::adopt(dest.2, dest.3, out)
}
impl PrecisionDocument {
    pub fn warp_info(&self, index: usize) -> Result<Value, String> {
        if self.is_cmyk() {
            return self.cmyk_warp_info(index);
        }
        let layer = self.layer(index).ok_or("invalid warp layer index")?;
        let field = layer.effective_warp(&self.profile).map(|b| {
            let source_rect = match b.target {
                WarpTarget::Pixels => rect(
                    b.source_offset,
                    b.source_pixels
                        .as_ref()
                        .expect("validated warp pixels")
                        .dimensions(),
                ),
                WarpTarget::Mask => rect(
                    b.source_mask_offset,
                    b.source_mask
                        .as_ref()
                        .expect("validated warp mask")
                        .dimensions(),
                ),
            };
            b.field.json(b.target, source_rect, b.linked_mask)
        });
        Ok(json!({"schema_version":1,"field":field}))
    }
    pub fn warp_plan(&self, edit: &WarpEdit) -> Result<SourcePlan, String> {
        if self.is_cmyk() {
            return self.cmyk_warp_plan(edit);
        }
        // #1 Indexed phase 2d: the RGB storage's warp, re-indexed.
        if self.is_indexed() {
            return crate::indexed_transform::warp_plan(self, edit);
        }
        let layer = self.layer(edit.layer()).ok_or("invalid warp layer index")?;
        let current = layer.effective_warp(&self.profile);
        let mut output = 0u64;
        let mut work = 0u64;
        // At float the pixel output is 16 B/px, and the source's transient
        // contiguous copy is still bound while the output is tiled
        // (`precision_sampling::transform_image`'s two copies).
        let mut copies = 0u64;
        if let WarpEdit::Set { target, field, .. } = edit {
            if current.as_ref().is_some_and(|b| b.target != *target) {
                return Err("clear or bake the current warp before switching target".into());
            }
            if *target == WarpTarget::Pixels {
                if layer.source().is_none() {
                    return Err("warp pixels require a raster layer".into());
                }
                if layer.description.is_some() || layer.retained_source.is_some() {
                    return Err(
                        "rasterize the retained source or artwork before warping pixels".into(),
                    );
                }
                self.lock_refusal(edit.layer(), EditKind::Pixels)?;
                self.lock_refusal(edit.layer(), EditKind::Position)?;
            } else {
                self.lock_refusal(edit.layer(), EditKind::Mask)?;
            }
            let (size, offset) = match target {
                WarpTarget::Pixels => (
                    current
                        .as_ref()
                        .and_then(|b| b.source_pixels.as_ref())
                        .or_else(|| layer.source())
                        .ok_or("warp pixels require a raster layer")?
                        .dimensions(),
                    current.as_ref().map_or(layer.offset(), |b| b.source_offset),
                ),
                WarpTarget::Mask => (
                    current
                        .as_ref()
                        .and_then(|b| b.source_mask.as_ref())
                        .or_else(|| layer.mask.as_ref().map(|m| &m.plane))
                        .ok_or("warp target has no mask")?
                        .dimensions(),
                    current.as_ref().map_or(
                        layer
                            .mask
                            .as_ref()
                            .ok_or("warp target has no mask")?
                            .properties
                            .offset,
                        |b| b.source_mask_offset,
                    ),
                ),
            };
            let dest = field.bounds(rect(offset, size))?;
            let float_pixels = self.float.is_some() && *target == WarpTarget::Pixels;
            if field.translation().is_none() {
                output = u64::from(dest.2)
                    * u64::from(dest.3)
                    * if float_pixels {
                        16
                    } else if *target == WarpTarget::Pixels {
                        8
                    } else {
                        2
                    };
                if float_pixels {
                    copies = float_source_copy_bytes(size)?
                        .saturating_add(float_adoption_copy_bytes((dest.2, dest.3))?);
                }
                work = u64::from(dest.2)
                    * u64::from(dest.3)
                    * u64::from(field.filter.kernel().taps() as u32).pow(2)
                    * 64;
            }
            if *target == WarpTarget::Pixels
                && layer.mask.as_ref().is_some_and(|m| m.properties.linked)
            {
                let m = current
                    .as_ref()
                    .and_then(|b| b.source_mask.as_ref())
                    .or_else(|| layer.mask.as_ref().map(|m| &m.plane))
                    .ok_or("warp linked mask is missing")?;
                let mo = current.as_ref().map_or(
                    layer
                        .mask
                        .as_ref()
                        .ok_or("warp linked mask is missing")?
                        .properties
                        .offset,
                    |b| b.source_mask_offset,
                );
                let d = field.bounds(rect(mo, m.dimensions()))?;
                if field.translation().is_none() {
                    output = output.saturating_add(u64::from(d.2) * u64::from(d.3) * 2);
                }
                if field.translation().is_none() {
                    work = work.saturating_add(u64::from(d.2) * u64::from(d.3) * 64);
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
        Ok(SourcePlan {
            retained_bytes: self
                .retained_bytes()
                .saturating_add(output)
                .saturating_add(64 * 1024),
            scratch_bytes: output.saturating_add(64 * 1024).saturating_add(copies),
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
        // #1 Indexed phase 2d: the RGB storage's warp, re-indexed.
        if self.is_indexed() {
            job.check()?;
            return crate::indexed_transform::warp_edit(
                self,
                edit,
                limit,
                other_retained_bytes,
                job,
            );
        }
        self.warp_edit_with_context(edit, limit, other_retained_bytes, job)
    }

    /// [`Self::warp_edit`] under a begun job (the Indexed adapter's inner
    /// run, `indexed_transform`).
    pub(crate) fn warp_edit_with_context(
        &self,
        edit: &WarpEdit,
        limit: u64,
        other_retained_bytes: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        job.check()?;
        if self.is_cmyk() {
            return self.cmyk_warp_edit(edit, limit, other_retained_bytes, job);
        }
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
        let mut flat = flatten(&self.layers);
        let layer = &mut flat
            .get_mut(edit.layer())
            .ok_or("invalid warp layer index")?
            .layer;
        let current = layer.effective_warp(&self.profile);
        match edit {
            WarpEdit::Clear { .. } => {
                let Some(b) = current else { return Ok(None) };
                if let Some(p) = b.source_pixels {
                    layer.content = Content::Raster {
                        image: p,
                        offset: b.source_offset,
                    };
                }
                if let Some(m) = b.source_mask {
                    if let Some(mask) = &mut layer.mask {
                        mask.plane = m;
                        mask.properties.offset = b.source_mask_offset;
                    }
                }
                layer.warp = None;
            }
            WarpEdit::Bake { .. } => {
                if current.is_none() {
                    return Ok(None);
                }
                layer.warp = None;
            }
            WarpEdit::Set { target, field, .. } => {
                if field.identity() {
                    let Some(b) = current else { return Ok(None) };
                    if let Some(p) = b.source_pixels {
                        layer.content = Content::Raster {
                            image: p,
                            offset: b.source_offset,
                        };
                    }
                    if let Some(m) = b.source_mask {
                        if let Some(mask) = &mut layer.mask {
                            mask.plane = m;
                            mask.properties.offset = b.source_mask_offset;
                        }
                    }
                    layer.warp = None;
                } else {
                    let mut normalized = field.clone();
                    normalized.normalize();
                    if current
                        .as_ref()
                        .is_some_and(|b| b.field.equivalent(&normalized))
                    {
                        return Ok(None);
                    }
                    let sp = if *target == WarpTarget::Pixels {
                        Some(
                            current
                                .as_ref()
                                .and_then(|b| b.source_pixels.clone())
                                .or_else(|| layer.source().cloned())
                                .ok_or("warp pixels require raster")?,
                        )
                    } else {
                        None
                    };
                    let sm = if *target == WarpTarget::Mask
                        || layer.mask.as_ref().is_some_and(|m| m.properties.linked)
                    {
                        Some(
                            current
                                .as_ref()
                                .and_then(|b| b.source_mask.clone())
                                .or_else(|| layer.mask.as_ref().map(|m| m.plane.clone()))
                                .ok_or("warp target has no mask")?,
                        )
                    } else {
                        None
                    };
                    let so = current.as_ref().map_or(layer.offset(), |b| b.source_offset);
                    let mo = current.as_ref().map_or(
                        layer.mask.as_ref().map_or((0, 0), |m| m.properties.offset),
                        |b| b.source_mask_offset,
                    );
                    let linked = *target == WarpTarget::Pixels && sm.is_some();
                    let mut b = WarpBinding16 {
                        field: Arc::new(normalized),
                        target: *target,
                        source_pixels: sp,
                        source_offset: so,
                        source_mask: sm,
                        source_mask_offset: mo,
                        rendered_pixels: None,
                        rendered_offset: (0, 0),
                        rendered_mask: None,
                        rendered_mask_offset: (0, 0),
                        profile: self.profile.clone(),
                        linked_mask: linked,
                        mask_outside: layer
                            .mask
                            .as_ref()
                            .map_or(0, |m| m.properties.outside_coverage),
                    };
                    if let Some(p) = &b.source_pixels {
                        let (out, offset) = if let Some(delta) = b.field.translation() {
                            (p.clone(), shifted(so, delta)?)
                        } else {
                            let d = b.field.bounds(rect(so, p.dimensions()))?;
                            let codes = self.linear_codes();
                            (image(p, so, d, &b.field, codes, job)?, (d.0, d.1))
                        };
                        b.rendered_pixels = Some(out.clone());
                        b.rendered_offset = offset;
                        layer.content = Content::Raster { image: out, offset };
                    }
                    if let Some(m) = &b.source_mask {
                        let (out, offset) = if let Some(delta) = b.field.translation() {
                            (m.clone(), shifted(mo, delta)?)
                        } else {
                            let d = b.field.bounds(rect(mo, m.dimensions()))?;
                            (
                                mask(m, mo, d, &b.field, u16::from(b.mask_outside) * 257, job)?,
                                (d.0, d.1),
                            )
                        };
                        b.rendered_mask = Some(out.clone());
                        b.rendered_mask_offset = offset;
                        let mask = layer.mask.as_mut().ok_or("warp target has no mask")?;
                        mask.plane = out;
                        mask.properties.offset = offset;
                    }
                    // 11F: a float layer carries no warp, so a float set
                    // is applied at once and keeps no binding; a
                    // Gray or Lab document's set is baked in the same edit
                    // too (#1 phase 4 and Lab phase 4a-ii: no persisted
                    // Gray or Lab binding).
                    if self.is_gray() || self.is_lab() {
                        layer.warp = None;
                    } else if self.float.is_none() {
                        layer.warp = Some(b);
                    }
                }
            }
        }
        let next = self.rebuilt(
            self.width,
            self.height,
            rebuild(flat)?,
            self.channels.as_ref().clone(),
        )?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
