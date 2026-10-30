//! Immutable RZLM1 remapping. The canonical payload and header stay native u16.
//!
//! `resized` carries a mask to another frame size in normalised coordinates (the
//! RAW-settings sync's locals). It runs `sampling::resize_into` with the bilinear kernel, not
//! the affine `remap`: resize weights are clipped to the source and renormalised, widen their
//! support on reduction and have no negative lobe, so full coverage stays 65535 up to the
//! border and fine detail is anti-aliased when shrinking. `map_into` drops out-of-bounds taps
//! without renormalising (the edges dim on enlargement) and does not widen on reduction.
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::raw_local_mask::{dimensions, RawLocalMask, RAW_LOCAL_HEADER};
use crate::raw_source_geometry::RawSourceMap;
use crate::resources::Inventory;
use serde_json::Value;
use std::sync::Arc;

impl RawLocalMask {
    pub fn remap_plan(
        &self,
        request: &str,
        channel: u32,
        limit: u64,
        other: u64,
    ) -> Result<Value, String> {
        let plan = RawSourceMap::parse(request)?.plan(self.dimensions(), channel, false)?;
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        admit(inventory.unique_bytes(), plan.scratch, other, limit)?;
        Ok(plan.report())
    }

    pub fn remap(
        &self,
        request: &str,
        channel: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let plan = RawSourceMap::parse(request)?.plan(self.dimensions(), channel, false)?;
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        admit(inventory.unique_bytes(), plan.scratch, other, limit)?;
        job.reserve(plan.scratch)?;
        if plan.unchanged {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let (dw, dh) = plan.destination;
        let n = dimensions(dw, dh)?;
        let mut bytes = Self::empty_bytes(dw, dh)?;
        if let Some(exact) = plan.exact {
            for y in 0..dh {
                job.check()?;
                for x in 0..dw {
                    job.every(x as usize)?;
                    let (sx, sy) = exact.source(x, y);
                    let value = if sx >= 0
                        && sy >= 0
                        && sx < i64::from(plan.source.0)
                        && sy < i64::from(plan.source.1)
                    {
                        self.at(sy as usize * plan.source.0 as usize + sx as usize)
                    } else {
                        0
                    };
                    let i = y as usize * dw as usize + x as usize;
                    bytes[RAW_LOCAL_HEADER + i * 2..RAW_LOCAL_HEADER + i * 2 + 2]
                        .copy_from_slice(&value.to_le_bytes());
                }
                job.report(f64::from(y + 1) / f64::from(dh), 6)?;
            }
        } else {
            crate::sampling::map_into::<_, _, 1>(
                plan.source,
                plan.destination,
                plan.kernel(),
                |x, y, _| plan.source_at(x, y, channel),
                |i| self.at(i),
                |acc, p, weight| acc[0] += f64::from(p) * weight,
                |acc| acc[0].clamp(0.0, 65535.0).round() as u16,
                |i, value| {
                    bytes[RAW_LOCAL_HEADER + i * 2..RAW_LOCAL_HEADER + i * 2 + 2]
                        .copy_from_slice(&value.to_le_bytes())
                },
                job,
            )?;
        }
        debug_assert_eq!(bytes.len(), RAW_LOCAL_HEADER + n * 2);
        job.check()?;
        job.report(1.0, 5)?;
        Self::from_blob(Arc::new(bytes)).map(Some)
    }
    /// A new mask of `width` x `height` whose coverage is this one's in normalised coordinates
    /// (each axis scaled independently), resampled with the edge-normalised bilinear resize.
    /// `Ok(None)` when the size is unchanged (nothing to publish). `Err` for a zero or
    /// over-64-megapixel size, a refused admission or a cancelled control. The plan is the
    /// output payload, the f32 intermediate (`4 · source width · height`) and bookkeeping.
    pub fn resized(
        &self,
        width: u32,
        height: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let n = dimensions(width, height)?;
        if (width, height) == self.dimensions() {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let intermediate = 4 * u64::from(self.width) * u64::from(height);
        let scratch = (RAW_LOCAL_HEADER + n * 2) as u64 + intermediate + BOOKKEEPING_BYTES;
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        admit(inventory.unique_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let mut bytes = Self::empty_bytes(width, height)?;
        crate::sampling::resize_into::<u16, 1>(
            self.dimensions(),
            (width, height),
            crate::sampling::Kernel::Bilinear,
            |i| self.at(i),
            |acc, p, weight| acc[0] += f32::from(p) * weight,
            |acc| acc[0].clamp(0.0, 65535.0).round() as u16,
            |i, value| {
                bytes[RAW_LOCAL_HEADER + i * 2..RAW_LOCAL_HEADER + i * 2 + 2]
                    .copy_from_slice(&value.to_le_bytes())
            },
            true,
            job,
        )?;
        job.check()?;
        job.report(1.0, 5)?;
        Self::from_blob(Arc::new(bytes)).map(Some)
    }
}
