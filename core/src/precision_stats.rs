//! Authoritative u16 sample statistics. Display bins are derived directly from
//! original codes; their end buckets are never substituted for exact clipping.
//! RGB/luma ignore alpha-zero pixels, matching the established histogram rule.
//! Alpha counts every sampled position. A stride reports sampled counts only.
//!
//! Histogram display bins are separate from original-code endpoint counts; larger strides describe
//! sampled positions only, and selection gating is unavailable.

use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::{
    admit, PrecisionImage, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::tile_store::TILE_EDGE;
use serde_json::{json, Value};

fn profile_fields(image: &PrecisionImage, value: &mut Value) {
    let name = image.profile().name();
    value["profile_name"] = json!(name.chars().take(256).collect::<String>());
    value["profile_name_truncated"] = json!(name.chars().nth(256).is_some());
    value["profile_bytes"] = json!(image.profile().bytes().len());
    value["storage"] = json!("rgba16_unorm");
    value["schema_version"] = json!(1);
}

impl PrecisionImage {
    /// Exact point codes and a rational straight sample mean. Reach 0/1/2 means
    /// 1/3/5 pixels per side; outside positions are omitted, never clamped.
    pub fn sample(&self, x: i32, y: i32, reach: u32) -> Result<Value, String> {
        if self.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("precision sampling requires rgba16_unorm".into());
        }
        if reach > 2 {
            return Err("precision sample reach must be 0, 1 or 2".into());
        }
        let (w, h) = self.dimensions();
        let mut sum = [0_u64; 4];
        let mut count = 0_u64;
        let radius = i64::from(reach);
        for py in i64::from(y) - radius..=i64::from(y) + radius {
            for px in i64::from(x) - radius..=i64::from(x) + radius {
                if px >= 0 && py >= 0 && px < i64::from(w) && py < i64::from(h) {
                    let Some(pixel) = self.rgba16_at(px as u32, py as u32) else {
                        continue;
                    };
                    for (slot, code) in sum.iter_mut().zip(pixel) {
                        *slot += u64::from(code);
                    }
                    count += 1;
                }
            }
        }
        if count == 0 {
            return Err("precision sample is outside the image".into());
        }
        let center = if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
            self.rgba16_at(x as u32, y as u32)
        } else {
            None
        };
        let means = sum.map(|v| v as f64 / count as f64);
        let mut value = json!({"x":x,"y":y,"reach":reach,"center_rgba16":center,
            "sum_rgba16":sum,"sample_count":count,"mean_rgba16":means,
            "normalized_rgba":means.map(|v|v/65535.0),
            "counting":"all in-bounds pixels including alpha-zero hidden RGB; straight arithmetic mean"});
        profile_fields(self, &mut value);
        Ok(value)
    }

    /// Code/endpoint counters under an explicit host live-set budget. A fixed
    /// 1 MiB reservation covers count arrays and JSON construction. The host
    /// supplies the authoritative captured projection, never its preview.
    pub fn histogram_checked(
        &self,
        stride: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        if self.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("precision histogram requires rgba16_unorm".into());
        }
        let (width, height) = self.dimensions();
        let count = width as usize * height as usize;
        admit(self.retained_bytes(), BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let mut bins = [[0_u64; 256]; 4];
        let mut alpha_bins = [0_u64; 256];
        let mut zero = [0_u64; 4];
        let mut maximum = [0_u64; 4];
        let (mut total, mut sampled, mut luma_zero, mut luma_max) = (0_u64, 0_u64, 0_u64, 0_u64);
        let step = stride.max(1) as usize;
        // Walked in TILE-BAND order (each 256-row band, each 256-column
        // span, each row's run) — a row across every tile column costs ~20%
        // at 24 MP. Every counter is order-independent and the reports read
        // only `visited`, so the sampled set (flat index a multiple of
        // `step`) and the report sequence are the flat walk's.
        let edge = TILE_EDGE as usize;
        let mut visited = 0_usize;
        for band in (0..height).step_by(edge) {
            for x0 in (0..width).step_by(edge) {
                let x1 = x0.saturating_add(TILE_EDGE).min(width);
                for y in band..band.saturating_add(TILE_EDGE).min(height) {
                    for (rx, run) in self.row_segments(y, x0, x1) {
                        let PrecisionRun::Rgba16(run) = run else {
                            continue;
                        };
                        let flat = y as usize * width as usize + rx as usize;
                        let start = ((step - flat % step) % step).min(run.len());
                        let mut rest = run[start..].iter().step_by(step);
                        // Reports hoisted to each 4096-sample boundary: the
                        // same `visited & 4095 == 0` positions as a per-sample test.
                        while rest.len() > 0 {
                            if visited & 4095 == 0 {
                                let i = visited * step;
                                job.report(i as f64 / count as f64, 6)?;
                            }
                            let (room, before) = (4096 - (visited & 4095), rest.len());
                            for &p in rest.by_ref().take(room) {
                                sampled += 1;
                                alpha_bins[display_bin(p[3])] += 1;
                                zero[3] += u64::from(p[3] == 0);
                                maximum[3] += u64::from(p[3] == u16::MAX);
                                if p[3] == 0 {
                                    continue;
                                }
                                total += 1;
                                for c in 0..3 {
                                    bins[c][display_bin(p[c])] += 1;
                                    zero[c] += u64::from(p[c] == 0);
                                    maximum[c] += u64::from(p[c] == u16::MAX);
                                }
                                // Shared Rec.709 coefficients; f64 scaling never materializes byte RGB.
                                let luma = f64::from(crate::blend::LUMA_R) * f64::from(p[0])
                                    + f64::from(crate::blend::LUMA_G) * f64::from(p[1])
                                    + f64::from(crate::blend::LUMA_B) * f64::from(p[2]);
                                bins[3][(luma / 257.0).round().clamp(0.0, 255.0) as usize] += 1;
                                luma_zero += u64::from(p[..3].iter().all(|v| *v == 0));
                                luma_max += u64::from(p[..3].iter().all(|v| *v == u16::MAX));
                            }
                            visited += before - rest.len();
                        }
                    }
                }
            }
        }
        job.check()?;
        let rows: Vec<&[u64]> = bins.iter().map(|row| row.as_slice()).collect();
        let mut value = json!({"bins":rows,"alpha_bins":alpha_bins.as_slice(),
            "channels":["red","green","blue","encoded_rec709_luma"],
            "total":total,"sampled_pixels":sampled,"stride":step,
            "exact_zero_rgba":zero,"exact_max_rgba":maximum,
            "exact_zero_luma":luma_zero,"exact_max_luma":luma_max,
            "display_bin_rule":"round(u16_code / 257); luma before binning",
            "clipping_scope":if step==1 {"all pixels"} else {"sampled positions only"},
            "rgb_counting":"alpha greater than zero; no selection restriction",
            "alpha_counting":"all sampled positions"});
        profile_fields(self, &mut value);
        job.report(1.0, 5)?;
        Ok(value)
    }
}

fn display_bin(value: u16) -> usize {
    ((u32::from(value) + 128) / 257) as usize
}
