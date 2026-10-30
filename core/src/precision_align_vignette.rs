//! Edit ▸ Auto-Align Layers' Vignette Removal at 16 bits
//! and at 32-bit float: the typed document's thin adapter, the twin of
//! `doc_align_vignette` with the same request, refusals, plan shape and
//! `Ok(None)` rule. A CMYK document refuses by name.
//!
//! The fit reads linear light: at 16 bits through each layer's image
//! profile (`MatrixTrc::decode_rgb`; a profile with no matrix/TRC model
//! refuses `adjust_calibrated::PROFILE_REFUSAL`), at float the stored values,
//! which are linear. The write: at 16 bits `sampling_lens::
//! apply_vignette_stops` with `selected_filter::profile_linear_gain` (the
//! 16-bit Lens Correction profile arm's basis), clamped and rounded to its
//! code; at float RGB × 2^stops, unclamped (a corner lifted above white is a
//! value a float layer keeps). A Lab layer (#1 phase 6b-ii) is fitted on
//! its codes' CIE Y and written by `lab_stack::vignette_codes`, the gain on
//! its XYZ, never through `profile_linear_gain`.
//!
//! THE PLAN's write term for the largest listed layer: at 16 bits its owned
//! copy (`source_copy_bytes`, 8 bytes a pixel), the tiles' adoption copy
//! (`adoption_copy_bytes`) and `PrecisionToneEncoder::scratch_bytes()`; at
//! float its copy (16 bytes a pixel) and `float_adoption_copy_bytes`. The
//! output is every listed layer at 8 or 16 bytes a pixel.
use serde_json::Value;

use crate::align_vignette::{fit, Radial, VignetteFit, VignetteFrame};
use crate::doc_align_vignette::NAME;
use crate::doc_align_vignette::{
    layer_refusal, local, lock_reason, plan_scratch, plan_value, RemoveVignetteRequest,
};
use crate::doc_lock::EditKind;
use crate::icc_transform::PrecisionToneEncoder;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage};
use crate::precision_sampling::{
    adoption_copy_bytes, float_adoption_copy_bytes, float_source_copy_bytes, source_copy_bytes,
};
use crate::precision_structure_ops::{flatten, rebuild};
use crate::sampling_lens::apply_vignette_stops;
use crate::selected_filter::profile_linear_gain;

/// A lens correction `name`'s refusal in a CMYK document.
pub(crate) fn cmyk_refusal(name: &str) -> String {
    format!("{name} does not work in CMYK.")
}

/// The linear-light reader of a typed stack's `images` (in request order)
/// for `align_vignette::fit` and `align_distortion::fit`: at 16 bits through
/// each image's profile (a profile with no matrix/TRC model refuses
/// `adjust_calibrated::PROFILE_REFUSAL`), at float the stored values; a pixel
/// below full alpha reads as absent.
pub(crate) fn typed_linear<'a>(
    images: &'a [(&'a PrecisionImage, (i32, i32))],
) -> Result<impl Fn(usize, i64, i64) -> Option<[f64; 3]> + 'a, String> {
    /// How one frame reads linear light.
    #[derive(Clone, Copy)]
    enum Reader<'m> {
        Float,
        Model(&'m crate::icc_transform::MatrixTrc),
        /// A Lab layer (its profile the Lab twin): the codes' CIE Y
        /// (`lab_stack::linear_y`, #1 phase 6b-ii).
        Lab,
    }
    let mut readers = Vec::with_capacity(images.len());
    for (image, _) in images {
        readers.push(if image.storage() == PrecisionStorage::Rgba32Float {
            Reader::Float
        } else if crate::lab_document::is_lab_twin(image.profile()) {
            Reader::Lab
        } else {
            Reader::Model(
                image
                    .profile()
                    .model()
                    .ok_or(crate::adjust_calibrated::PROFILE_REFUSAL)?,
            )
        });
    }
    Ok(move |frame: usize, x: i64, y: i64| {
        let (image, offset) = images.get(frame)?;
        let (lx, ly) = local(*offset, x, y)?;
        let p = image.sample_at(lx, ly)?;
        if p[3] != 1.0 {
            return None;
        }
        Some(match readers[frame] {
            Reader::Model(model) => model.decode_rgb([p[0], p[1], p[2]]),
            Reader::Float => [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])],
            Reader::Lab => crate::lab_stack::linear_y(p.map(crate::precision_image::unit_to_u16)),
        })
    })
}

struct Prepared<'a> {
    frames: Vec<VignetteFrame>,
    images: Vec<(&'a PrecisionImage, (i32, i32))>,
    retained: u64,
    scratch: u64,
    output: u64,
}

impl PrecisionDocument {
    /// The raster record a lens correction `name` reads or changes and its
    /// offset, or its refusal (the byte twin's order).
    pub(crate) fn lens_record(
        &self,
        index: usize,
        name: &str,
    ) -> Result<(&PrecisionImage, (i32, i32)), String> {
        let record = self
            .layer(index)
            .ok_or_else(|| layer_refusal(name, index, "there is no such layer"))?;
        let (image, offset) = match &record.content {
            Content::Raster { image, offset } => (image, *offset),
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                return Err(cmyk_refusal(name))
            }
            Content::Adjustment(_) => {
                return Err(layer_refusal(
                    name,
                    index,
                    "it is an adjustment layer, not a raster layer",
                ))
            }
            Content::Group(_) | Content::PassThrough(_) => {
                return Err(layer_refusal(
                    name,
                    index,
                    "it is a group, not a raster layer",
                ))
            }
        };
        if record.description.is_some() || record.retained_source.is_some() {
            return Err(layer_refusal(
                name,
                index,
                "it holds artwork or a placed source; rasterize it first",
            ));
        }
        let blocking = self.lock_block(index, EditKind::Pixels);
        if blocking != 0 {
            let owner = self.blocking_owner(index, blocking);
            return Err(layer_refusal(name, index, &lock_reason(index, Some(owner))));
        }
        Ok((image, offset))
    }

    fn vignette_prepare(&self, request: &RemoveVignetteRequest) -> Result<Prepared<'_>, String> {
        if self.is_cmyk() {
            return Err(cmyk_refusal(NAME));
        }
        let float = self.is_float();
        let mut frames = Vec::with_capacity(request.layers.len());
        let mut images = Vec::with_capacity(request.layers.len());
        let (mut output, mut largest) = (0u64, (0u32, 0u32));
        for (index, map) in &request.layers {
            let (image, offset) = self.lens_record(*index, NAME)?;
            let (w, h) = image.dimensions();
            let bytes = if image.storage() == PrecisionStorage::Rgba32Float {
                16
            } else {
                8
            };
            output = output.saturating_add(u64::from(w) * u64::from(h) * bytes);
            if u64::from(w) * u64::from(h) > u64::from(largest.0) * u64::from(largest.1) {
                largest = (w, h);
            }
            frames.push(VignetteFrame {
                rect: (i64::from(offset.0), i64::from(offset.1), w, h),
                map: *map,
            });
            images.push((image, offset));
        }
        let write = if float {
            float_source_copy_bytes(largest)?.saturating_add(float_adoption_copy_bytes(largest)?)
        } else {
            source_copy_bytes(largest)?
                .saturating_add(adoption_copy_bytes(largest)?)
                .saturating_add(PrecisionToneEncoder::scratch_bytes())
        };
        Ok(Prepared {
            frames,
            images,
            retained: self.retained_bytes().saturating_add(output),
            scratch: plan_scratch(write, request.layers.len()),
            output,
        })
    }

    fn vignette_fit(
        &self,
        prepared: &Prepared<'_>,
        job: JobContext<'_>,
    ) -> Result<VignetteFit, String> {
        fit(&prepared.frames, typed_linear(&prepared.images)?, job)
    }

    /// The typed twin of `DrDocument::remove_vignette_plan`.
    pub fn remove_vignette_plan(
        &self,
        request: &RemoveVignetteRequest,
        _limit: u64,
        _other: u64,
    ) -> Result<Value, String> {
        let prepared = self.vignette_prepare(request)?;
        let fit = self.vignette_fit(&prepared, JobContext::NONE)?;
        Ok(plan_value(
            prepared.retained,
            prepared.scratch,
            prepared.output,
            &fit,
            request,
        ))
    }

    /// The typed twin of `DrDocument::remove_vignette`.
    pub fn remove_vignette(
        &self,
        request: &RemoveVignetteRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let prepared = self.vignette_prepare(request)?;
        admit(prepared.retained, prepared.scratch, other, limit)?;
        job.reserve(prepared.scratch)?;
        let fit = self.vignette_fit(&prepared, job.range(0.0, 0.5))?;
        let mut written: Vec<(usize, PrecisionImage)> = Vec::new();
        let count = request.layers.len() as f64;
        for (k, ((index, _), frame)) in request.layers.iter().zip(&prepared.frames).enumerate() {
            let part = job.range(
                0.5 + 0.5 * k as f64 / count,
                0.5 + 0.5 * (k + 1) as f64 / count,
            );
            let (image, _) = prepared.images[k];
            let radial = Radial::of(frame.rect);
            let (w, h) = image.dimensions();
            let width = w as usize;
            let point = |i: usize| {
                (
                    frame.rect.0 as f64 + (i % width) as f64 + 0.5,
                    frame.rect.1 as f64 + (i / width) as f64 + 0.5,
                )
            };
            let mut changed = false;
            let next = if image.storage() == PrecisionStorage::Rgba32Float {
                let mut pixels = image.to_rgba32()?;
                for (i, p) in pixels.iter_mut().enumerate() {
                    part.every(i)?;
                    if p[3] <= 0.0 {
                        continue;
                    }
                    let (x, y) = point(i);
                    let before = *p;
                    apply_vignette_stops(p, radial.stops(&fit, x, y), true, None);
                    changed |= (0..3).any(|c| p[c].to_bits() != before[c].to_bits());
                }
                part.report(1.0, 4)?;
                changed
                    .then(|| {
                        PrecisionImage::from_rgba32_extended(
                            w,
                            h,
                            pixels,
                            image.profile().clone(),
                            None,
                        )
                    })
                    .transpose()?
            } else if crate::lab_document::is_lab_twin(image.profile()) {
                // A Lab layer: the gain multiplies the codes' XYZ (module doc).
                let mut pixels = image.to_rgba16()?;
                for (i, px) in pixels.iter_mut().enumerate() {
                    part.every(i)?;
                    if px[3] == 0 {
                        continue;
                    }
                    let (x, y) = point(i);
                    let next = crate::lab_stack::vignette_codes(*px, radial.stops(&fit, x, y));
                    if next != *px {
                        *px = next;
                        changed = true;
                    }
                }
                part.report(1.0, 4)?;
                changed
                    .then(|| {
                        PrecisionImage::from_rgba16_unseeded(
                            w,
                            h,
                            pixels,
                            image.profile().clone(),
                            None,
                        )
                    })
                    .transpose()?
            } else {
                let gain = profile_linear_gain(image)?;
                let mut pixels = image.to_rgba16()?;
                for (i, px) in pixels.iter_mut().enumerate() {
                    part.every(i)?;
                    if px[3] == 0 {
                        continue;
                    }
                    let (x, y) = point(i);
                    let mut p = px.map(|c| f32::from(c) / 65535.0);
                    apply_vignette_stops(&mut p, radial.stops(&fit, x, y), false, Some(&gain));
                    for (code, value) in px.iter_mut().zip(p).take(3) {
                        let value = (value * 65535.0).round() as u16;
                        if value != *code {
                            *code = value;
                            changed = true;
                        }
                    }
                }
                part.report(1.0, 4)?;
                changed
                    .then(|| {
                        PrecisionImage::from_rgba16_unseeded(
                            w,
                            h,
                            pixels,
                            image.profile().clone(),
                            None,
                        )
                    })
                    .transpose()?
            };
            if let Some(next) = next {
                written.push((*index, next));
            }
        }
        if written.is_empty() {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let mut flat = flatten(&self.layers);
        for (index, image) in written {
            let record = &mut flat
                .get_mut(index)
                .ok_or_else(|| format!("{NAME} layer index is out of range"))?
                .layer;
            if let Content::Raster { image: slot, .. } = &mut record.content {
                *slot = image;
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
