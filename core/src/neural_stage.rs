//! One neural filter STAGE (M4) at 8 and 16 bits: the classical Noise
//! Reduction on the source, the model over `neural_tiling`'s grid through the
//! host's predict callback, the stitch, and the classical Sharpen on the
//! stitched output. Super Zoom runs one stage (or two, with Remove JPEG
//! Artifacts' FBCNN stage first); JPEG Artifacts Removal runs one at scale 1.
//!
//! **The request** (`Stage::parse`): `{"tile", "pad", "scale",
//! "model_scale", "model": bool, "noise_reduction": 0…100, "sharpen":
//! 0…100}`, the last two optional (0). The grid's own refusals are
//! `TileGrid::new`'s; an output past `doc::MAX_PIXELS` refuses by its size.
//!
//! **Noise Reduction** `v > 0` is Filter ▸ Noise ▸ Reduce Noise at radius 2,
//! luminance `0.25·v/100` (its range's top at 100), detail 0.5 and colour
//! `0.5·v/100`, parsed by `SelectedFilter::parse`, run on the SOURCE before the
//! model sees it, through the existing runners: at 8 bits
//! `ops_spatial::filter_op` (so the bytes are `dr_image_filter_op`'s
//! `reduce_noise`), at 16 bits `SelectedFilter::plane` and `evaluate` with the
//! selected edit's full-coverage write (binary64, clamped, rounded), as
//! `export_sharpen` does for Smart Sharpen. **Sharpen** `v > 0` is the export
//! workspace's Unsharp Mask (`export_sharpen::OutputSharpen::UnsharpMask`,
//! radius 1, amount `v/50`, threshold 0) on the stitched OUTPUT, so it
//! sharpens what the user sees at the new size.
//!
//! **The model** runs once per tile, in index order, on the calling thread;
//! a callback answering false, or a plane holding a non-finite value, refuses
//! "the model failed on tile N". The job control is polled and reported per
//! tile. A stage that cannot change anything (no model, scale 1, neither
//! slider) answers `None`.
//!
//! **The plan** (`Stage::plan`) is the largest of the run's three phases,
//! each over what is alive at once, plus `BOOKKEEPING_BYTES`: Noise
//! Reduction (its own scratch: byte `filter_op_scratch_bytes`, which holds its
//! output; typed the kernel's plan or the n×16 plane beside the n×8 codes,
//! the larger, plus the n×8 tiled copy, plus a quarter as the byte figure
//! adds), the stitch (the reduced source if any, the f32 input and plane,
//! the output codes, and at 16 bits the contiguous n×8 copy of the source
//! codes and the tiled copy the output image makes of its codes) and the Sharpen (the stitched output beside
//! `OutputSharpen::plan_bytes`, which holds its own output, plus the typed
//! tiled copy). The input image is the caller's retained bytes.
use image::RgbaImage;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::export_sharpen::OutputSharpen;
use crate::job_control::{DrJobControl, JobContext, JobRun};
use crate::neural_tiling::{tile_input, write_tile, TileGrid};
use crate::precision_image::{
    admit, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::sampling::Unorm;
use crate::sampling_gaussian::CONTROLLED_JOB_WORK;
use crate::selected_filter::SelectedFilter;

/// The host's model: `(tile index, input, plane)`; false is a failure.
pub(crate) type Predict<'a> = dyn FnMut(u64, &[f32], &mut [f32]) -> bool + 'a;

/// A float refuses by name.
const NOT_FLOAT: &str = "neural filters are not available in a 32-bit document";

/// One stage's request (module doc).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stage {
    tile: u32,
    pad: u32,
    scale: u32,
    model_scale: u32,
    model: bool,
    noise_reduction: f64,
    sharpen: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    tile: u32,
    pad: u32,
    scale: u32,
    model_scale: u32,
    model: bool,
    #[serde(default)]
    noise_reduction: f64,
    #[serde(default)]
    sharpen: f64,
}

fn begin<'a>(
    control: Option<&'a DrJobControl>,
) -> Result<(Option<JobRun<'a>>, JobContext<'a>), String> {
    let run = control.map(DrJobControl::begin).transpose()?;
    Ok((run, control.map_or(JobContext::NONE, JobContext::new)))
}

impl Stage {
    /// The request, refusing malformed JSON, an unknown key and a slider
    /// outside 0…100.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 4096 {
            return Err("neural stage request exceeds 4 KiB".into());
        }
        let request: Request =
            serde_json::from_str(text).map_err(|e| format!("invalid neural stage request: {e}"))?;
        for (name, value) in [
            ("noise_reduction", request.noise_reduction),
            ("sharpen", request.sharpen),
        ] {
            if !value.is_finite() || !(0.0..=100.0).contains(&value) {
                return Err(format!("neural stage {name} must be 0 to 100"));
            }
        }
        Ok(Self {
            tile: request.tile,
            pad: request.pad,
            scale: request.scale,
            model_scale: request.model_scale,
            model: request.model,
            noise_reduction: request.noise_reduction,
            sharpen: request.sharpen,
        })
    }

    /// The grid over a `width × height` source, refusing an output past
    /// `doc::MAX_PIXELS` by its size.
    pub(crate) fn grid(&self, width: u32, height: u32) -> Result<TileGrid, String> {
        let grid = TileGrid::new(
            width,
            height,
            self.tile,
            self.pad,
            self.scale,
            self.model_scale,
        )?;
        let (w, h) = grid.output();
        if w * h > crate::doc::MAX_PIXELS {
            return Err(format!(
                "the zoomed image would be {w} × {h} ({:.1} megapixels), past the \
                 100-megapixel limit: choose a smaller zoom",
                (w * h) as f64 / 1e6
            ));
        }
        Ok(grid)
    }

    /// Reduce Noise's parameters, or `None` at 0.
    fn noise(&self) -> Option<Map<String, Value>> {
        if self.noise_reduction == 0.0 {
            return None;
        }
        let v = self.noise_reduction / 100.0;
        let params = json!({"radius": 2, "luminance": 0.25 * v, "detail": 0.5, "color": 0.5 * v});
        params.as_object().cloned()
    }

    /// The Unsharp Mask, or `None` at 0.
    fn output_sharpen(&self) -> Option<OutputSharpen> {
        (self.sharpen > 0.0).then_some(OutputSharpen::UnsharpMask {
            radius: 1.0,
            amount: self.sharpen / 50.0,
            threshold: 0.0,
        })
    }

    /// True when the stage cannot change a pixel.
    fn identity(&self) -> bool {
        !self.model && self.scale == 1 && self.noise_reduction == 0.0 && self.sharpen == 0.0
    }

    /// The run's working set at `bytes_per_sample` 1 or 2 (module doc).
    pub(crate) fn plan(&self, grid: &TileGrid, bytes_per_sample: u32) -> Result<u64, String> {
        if !matches!(bytes_per_sample, 1 | 2) {
            return Err("bytes_per_sample must be 1 or 2".into());
        }
        let wide = bytes_per_sample == 2;
        let pixel = 4 * u64::from(bytes_per_sample);
        let dimensions = (grid.width, grid.height);
        let n = u64::from(grid.width) * u64::from(grid.height);
        let (ow, oh) = grid.output();
        let m = ow * oh;
        let overflow = || "neural stage plan overflow".to_string();
        let (reduce, reduced) = match self.noise() {
            None => (0, 0),
            Some(params) if !wide => (
                crate::ops_spatial::filter_op_scratch_bytes(dimensions, "reduce_noise", &params)?,
                n * pixel,
            ),
            Some(params) => {
                let kernel = SelectedFilter::parse("reduce_noise", &params)?
                    .scratch(dimensions, CONTROLLED_JOB_WORK)?;
                let base = kernel
                    .max(n * (16 + 8))
                    .checked_add(n * 8 + 4 * BOOKKEEPING_BYTES)
                    .ok_or_else(overflow)?;
                (base + base.div_ceil(4), n * pixel)
            }
        };
        let buffers = if self.model {
            4 * (grid.input_len() as u64 + grid.plane_len() as u64)
        } else {
            0
        };
        // The typed stitch reads a contiguous copy of the tiled source codes,
        // and the output image tiles a copy of the output codes.
        let copy = if wide { m * 8 } else { 0 };
        let source = if wide { n * 8 } else { 0 };
        let stitch = reduced + source + buffers + m * pixel + copy;
        let sharpen = match self.output_sharpen() {
            None => 0,
            Some(sharpen) => {
                let out = (
                    u32::try_from(ow).map_err(|_| overflow())?,
                    u32::try_from(oh).map_err(|_| overflow())?,
                );
                m * pixel + sharpen.plan_bytes(out)? + copy
            }
        };
        reduce
            .max(stitch)
            .max(sharpen)
            .checked_add(BOOKKEEPING_BYTES)
            .ok_or_else(overflow)
    }

    /// `dr_neural_stage_info`'s answer for a `width × height` source.
    pub(crate) fn info(
        &self,
        width: u32,
        height: u32,
        bytes_per_sample: u32,
    ) -> Result<Value, String> {
        let grid = self.grid(width, height)?;
        let (w, h) = grid.output();
        Ok(json!({
            "tiles": grid.count(),
            "output_width": w,
            "output_height": h,
            "input_len": grid.input_len(),
            "plane_len": grid.plane_len(),
            "planned_bytes": self.plan(&grid, bytes_per_sample)?,
        }))
    }

    /// The model over every tile into `out`, `job` reporting per tile.
    fn stitch<T: Unorm>(
        &self,
        src: &[[T; 4]],
        grid: &TileGrid,
        predict: &mut Option<&mut Predict<'_>>,
        out: &mut [[T; 4]],
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let (mut input, mut plane) = (Vec::new(), Vec::new());
        if self.model {
            input = reserved_vec(grid.input_len())?;
            input.resize(grid.input_len(), 0.0_f32);
            plane = reserved_vec(grid.plane_len())?;
            plane.resize(grid.plane_len(), 0.0_f32);
        }
        let count = grid.count();
        for index in 0..count {
            job.check()?;
            let tile = grid.tile(index).ok_or("neural tile is out of range")?;
            let model = if self.model {
                tile_input(src, grid, &tile, &mut input);
                let predict = predict
                    .as_mut()
                    .ok_or("a neural stage with the model needs a predict callback")?;
                if !predict(index, &input, &mut plane) || plane.iter().any(|v| !v.is_finite()) {
                    return Err(format!("the model failed on tile {index}"));
                }
                Some(plane.as_slice())
            } else {
                None
            };
            write_tile(src, grid, &tile, model, out);
            job.report((index + 1) as f64 / count as f64, 3)?;
        }
        Ok(())
    }

    fn check_predict(&self, predict: &Option<&mut Predict<'_>>) -> Result<(), String> {
        if self.model && predict.is_none() {
            return Err("a neural stage with the model needs a predict callback".into());
        }
        Ok(())
    }
}

/// The 8-bit stage over `image` (module doc), admitted against `limit` beside
/// `other` and reserved on `control`.
pub(crate) fn run_bytes(
    image: &RgbaImage,
    stage: &Stage,
    mut predict: Option<&mut Predict<'_>>,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<RgbaImage>, String> {
    let (width, height) = image.dimensions();
    let grid = stage.grid(width, height)?;
    stage.check_predict(&predict)?;
    if stage.identity() {
        return Ok(None);
    }
    let plan = stage.plan(&grid, 1)?;
    let (_run, job) = begin(control)?;
    admit(image.as_raw().len() as u64, plan, other, limit)?;
    job.reserve(plan)?;
    let job = job.with_reservation_floor(plan);
    let reduced = match stage.noise() {
        Some(params) => {
            crate::ops_spatial::filter_op(image, "reduce_noise", &params, job.range(0.0, 0.1))?
        }
        None => None,
    };
    let source = reduced.as_ref().unwrap_or(image);
    let (ow, oh) = grid.output();
    let mut bytes: Vec<u8> = reserved_vec(ow as usize * oh as usize * 4)?;
    bytes.resize(ow as usize * oh as usize * 4, 0);
    stage.stitch(
        source.as_raw().as_chunks::<4>().0,
        &grid,
        &mut predict,
        bytes.as_chunks_mut::<4>().0,
        job.range(0.1, 0.9),
    )?;
    drop(reduced);
    let out = RgbaImage::from_raw(ow as u32, oh as u32, bytes)
        .ok_or("neural stage produced invalid dimensions")?;
    let out = match stage.output_sharpen() {
        Some(sharpen) => {
            let shared = sharpen.bytes(std::sync::Arc::new(out), job.range(0.9, 1.0))?;
            std::sync::Arc::try_unwrap(shared).unwrap_or_else(|shared| (*shared).clone())
        }
        None => out,
    };
    job.report(1.0, 5)?;
    Ok(Some(out))
}

/// Reduce Noise on 16-bit codes (module doc), or `None` on a bypass.
fn reduce_noise_wide(
    image: &PrecisionImage,
    params: &Map<String, Value>,
    job: JobContext<'_>,
) -> Result<Option<PrecisionImage>, String> {
    let filter = SelectedFilter::parse("reduce_noise", params)?;
    if filter.identity() {
        return Ok(None);
    }
    let Some(plane) = filter.plane(image, job.range(0.0, 0.8))? else {
        return Ok(None);
    };
    let (w, h) = image.dimensions();
    let mut rng = crate::rng::SplitMix64::new(filter.seed());
    let mut out: Vec<[u16; 4]> = reserved_vec(plane.len())?;
    for i in 0..plane.len() {
        job.every(i)?;
        let value = filter.evaluate(i, image.pixel(i), Some(&plane), &mut rng);
        if value.iter().any(|v| !v.is_finite()) {
            return Err("filter produced an invalid component".into());
        }
        out.push(value.map(|v| (f64::from(v).clamp(0.0, 1.0) * 65535.0).round() as u16));
    }
    drop(plane);
    PrecisionImage::from_rgba16_unseeded(w, h, out, image.profile().clone(), None).map(Some)
}

/// The 16-bit twin of [`run_bytes`]: RGBA16 only, a float refuses by name.
pub(crate) fn run_wide(
    image: &PrecisionImage,
    stage: &Stage,
    mut predict: Option<&mut Predict<'_>>,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionImage>, String> {
    if image.storage() == PrecisionStorage::Rgba32Float {
        return Err(NOT_FLOAT.into());
    }
    let (width, height) = image.dimensions();
    let grid = stage.grid(width, height)?;
    stage.check_predict(&predict)?;
    if stage.identity() {
        return Ok(None);
    }
    let plan = stage.plan(&grid, 2)?;
    let (_run, job) = begin(control)?;
    admit(image.retained_bytes(), plan, other, limit)?;
    job.reserve(plan)?;
    let job = job.with_reservation_floor(plan);
    let reduced = match stage.noise() {
        Some(params) => reduce_noise_wide(image, &params, job.range(0.0, 0.1))?,
        None => None,
    };
    let source = reduced.as_ref().unwrap_or(image);
    let codes = source.transient_rgba16()?;
    let (ow, oh) = grid.output();
    let mut out: Vec<[u16; 4]> = reserved_vec(ow as usize * oh as usize)?;
    out.resize(ow as usize * oh as usize, [0; 4]);
    stage.stitch(&codes, &grid, &mut predict, &mut out, job.range(0.1, 0.9))?;
    drop(codes);
    let profile = image.profile().clone();
    drop(reduced);
    let out = PrecisionImage::from_rgba16_unseeded(ow as u32, oh as u32, out, profile, None)?;
    let out = match stage.output_sharpen() {
        Some(sharpen) => sharpen.wide(out, job.range(0.9, 1.0))?,
        None => out,
    };
    job.report(1.0, 5)?;
    Ok(Some(out))
}
