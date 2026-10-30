//! The export workspace's output-sharpening stage (`doc_export`): the
//! request's `sharpen` object, its plan term and its run at 8 and 16 bits.
//!
//! Two methods. Unsharp Mask (`{"radius", "amount", "threshold"}`, also with
//! `"method": "unsharp_mask"`) is the shipped stage, byte for byte:
//! `ops_photographic::unsharp_mask_samples` on the 8-bit deliverable and
//! `unsharp_mask_runs` on u16 codes. Smart Sharpen (`"method":
//! "smart_sharpen"` with any of `sampling_smart_sharpen::SMART_SHARPEN_KEYS`)
//! is Filter ▸ Sharpen ▸ Smart Sharpen's `SpatialKernel` arm, parsed by
//! `SelectedFilter::parse` with the filter's keys, ranges and defaults: at 8
//! bits `ops_spatial::spatial_bytes` (so the bytes are `dr_image_filter_op`'s
//! `smart_sharpen`), at 16 bits the same plane and combine over the u16 codes,
//! quantized as the selected edit's full-coverage write
//! (`selected_adjustment::mix_codes`), so the export equals the filter applied
//! then exported. A 32-bit export refuses either method (`doc_export`).
use std::sync::Arc;

use image::RgbaImage;
use serde_json::{Map, Value};

use crate::doc_export::{known_keys, number_of, object_of};
use crate::filter_kernels::Neighborhood;
use crate::job_control::JobContext;
use crate::ops_spatial::{spatial_bytes, SpatialKernel};
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage};
use crate::sampling_gaussian::{destructive_work_cap, CONTROLLED_JOB_WORK};
use crate::sampling_smart_sharpen::SmartSharpen;
use crate::selected_filter::SelectedFilter;

/// The optional output-sharpening stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum OutputSharpen {
    /// The shipped Unsharp Mask controls.
    UnsharpMask {
        radius: f64,
        amount: f64,
        threshold: f64,
    },
    /// Filter ▸ Sharpen ▸ Smart Sharpen.
    SmartSharpen(SmartSharpen),
}

impl OutputSharpen {
    /// The request's `sharpen` object. Without `method`, or with
    /// `"unsharp_mask"`, the Unsharp Mask keys and refusals are unchanged;
    /// `"smart_sharpen"` takes the filter's keys and refuses in the filter's
    /// words after `export sharpen: `.
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        let object = object_of(value, "sharpen")?;
        let method = match object.get("method") {
            None | Some(Value::Null) => "unsharp_mask",
            Some(Value::String(text)) if text == "unsharp_mask" || text == "smart_sharpen" => {
                text.as_str()
            }
            Some(_) => {
                return Err("export sharpen method must be unsharp_mask or smart_sharpen".into())
            }
        };
        if method == "smart_sharpen" {
            let params: Map<String, Value> = object
                .iter()
                .filter(|(key, _)| key.as_str() != "method")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            return match SelectedFilter::parse("smart_sharpen", &params)
                .map_err(|e| format!("export sharpen: {e}"))?
            {
                SelectedFilter::Spatial(SpatialKernel::SmartSharpen(smart)) => {
                    Ok(Self::SmartSharpen(smart))
                }
                _ => Err("export sharpen: smart sharpen did not parse".into()),
            };
        }
        known_keys(
            object,
            &["method", "radius", "amount", "threshold"],
            "sharpen",
        )?;
        let radius = number_of(object, "radius")?.unwrap_or(0.0);
        let amount = number_of(object, "amount")?.unwrap_or(0.0);
        let threshold = number_of(object, "threshold")?.unwrap_or(0.0);
        if !(0.0..=64.0).contains(&radius) {
            return Err("export sharpen radius must be in [0,64] pixels".into());
        }
        if !(0.0..=5.0).contains(&amount) {
            return Err("export sharpen amount must be in [0,5]".into());
        }
        if !(0.0..=1.0).contains(&threshold) {
            return Err("export sharpen threshold must be in [0,1] normalized units".into());
        }
        Ok(Self::UnsharpMask {
            radius,
            amount,
            threshold,
        })
    }

    /// True when this sharpen would change nothing: Unsharp Mask at radius
    /// 0 or amount 0, exactly the bypass `ops_photographic` reports as
    /// `None`. Smart Sharpen never is one (its amount is at least 1 %).
    pub(crate) fn is_bypass(self) -> bool {
        match self {
            Self::UnsharpMask { radius, amount, .. } => radius == 0.0 || amount == 0.0,
            Self::SmartSharpen(_) => false,
        }
    }

    /// The typed export plan's sharpen term over the output `dimensions`, 0
    /// on a bypass. Unsharp Mask: the Gaussian's own plan, plus the straight
    /// f32 input the kernel builds (16 B/px) and the u16 output it fills (8
    /// B/px). Smart Sharpen: its kernel plan (which includes the n×16 plane
    /// the combine overwrites) plus the u16 output (8 B/px). A plan carries no
    /// work refusal.
    pub(crate) fn plan_bytes(self, dimensions: (u32, u32)) -> Result<u64, String> {
        if self.is_bypass() {
            return Ok(0);
        }
        let out = pixel_count(dimensions.0, dimensions.1)? as u64;
        Ok(match self {
            Self::UnsharpMask { radius, .. } => {
                crate::filter_kernels::neighborhood_plan(
                    Neighborhood::Gaussian { sigma: radius },
                    dimensions,
                    CONTROLLED_JOB_WORK,
                )?
                .0 + out * 16
                    + out * 8
            }
            Self::SmartSharpen(smart) => smart.plan(dimensions, CONTROLLED_JOB_WORK)?.0 + out * 8,
        })
    }

    /// The 8-bit stage: the sharpened deliverable, or `image` itself on a
    /// bypass.
    pub(crate) fn bytes(
        self,
        image: Arc<RgbaImage>,
        job: JobContext<'_>,
    ) -> Result<Arc<RgbaImage>, String> {
        let (w, h) = image.dimensions();
        match self {
            Self::UnsharpMask {
                radius,
                amount,
                threshold,
            } => {
                let samples = image.as_raw().as_chunks::<4>().0;
                match crate::ops_photographic::unsharp_mask_samples::<u8>(
                    samples,
                    (w, h),
                    radius,
                    amount,
                    threshold,
                    job,
                )? {
                    Some(out) => {
                        let mut bytes: Vec<u8> = reserved_vec(out.len() * 4)?;
                        for pixel in &out {
                            bytes.extend_from_slice(pixel);
                        }
                        Ok(Arc::new(
                            RgbaImage::from_raw(w, h, bytes)
                                .ok_or("export sharpening produced invalid dimensions")?,
                        ))
                    }
                    None => Ok(image),
                }
            }
            Self::SmartSharpen(smart) => Ok(Arc::new(spatial_bytes(
                &image,
                SpatialKernel::SmartSharpen(smart),
                job,
            )?)),
        }
    }

    /// The 16-bit stage, on the deliverable's own u16 codes: the sharpened
    /// image, or `image` itself on a bypass.
    pub(crate) fn wide(
        self,
        image: PrecisionImage,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let (w, h) = image.dimensions();
        let out = match self {
            Self::UnsharpMask {
                radius,
                amount,
                threshold,
            } => {
                let runs = image
                    .runs_rgba16()
                    .ok_or("high-depth export requires RGBA16 UNORM storage")?;
                match crate::ops_photographic::unsharp_mask_runs::<u16>(
                    runs,
                    w as usize * h as usize,
                    (w, h),
                    radius,
                    amount,
                    threshold,
                    job,
                )? {
                    Some(out) => out,
                    None => return Ok(image),
                }
            }
            Self::SmartSharpen(smart) => {
                if image.runs_rgba16().is_none() {
                    return Err("high-depth export requires RGBA16 UNORM storage".into());
                }
                let kernel = SpatialKernel::SmartSharpen(smart);
                let cap = destructive_work_cap(job);
                kernel.plan((w, h), cap)?;
                let plane = kernel.plane(|i| image.pixel(i), (w, h), cap, job)?;
                let mut out: Vec<[u16; 4]> = reserved_vec(plane.len())?;
                for (i, sample) in plane.iter().enumerate() {
                    job.every(i)?;
                    let value = kernel.combine(image.pixel(i), *sample);
                    if value.iter().any(|v| !v.is_finite()) {
                        return Err("filter produced an invalid component".into());
                    }
                    // The selected edit's full-coverage write: binary64,
                    // clamped, rounded.
                    out.push(
                        value.map(|v| (f64::from(v).clamp(0.0, 1.0) * 65535.0).round() as u16),
                    );
                }
                out
            }
        };
        PrecisionImage::from_rgba16_unseeded(w, h, out, image.profile().clone(), None)
    }
}
