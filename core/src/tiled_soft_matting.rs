//! Experimental full-resolution local soft-prior matting with bounded tiles.
//!
//! Each 256px output tile solves the shared closed-form objective with a 16px
//! context halo, then copies its central samples. This is a declared local
//! approximation: tile convergence is not convergence of the global system.
//! No resizing, automatic hard labels, overlap blending or reference input.
//! Aggregate stencil-work admission includes every repeated halo pixel and
//! lowers the iteration ceiling to remain inside the original 20B budget.
//!
//! `tiled_soft_matting.rs` adds bounded native-resolution local solves with a halo and aggregate
//! work admission; local convergence is not global convergence.
use crate::{
    closed_form_matting::{
        estimate_input_job, scratch_estimate, ClosedFormParams, MAX_STENCIL_TERMS,
    },
    doc::MAX_PIXELS,
    job_control::{DrJobControl, JobContext},
    matting_input::{ConstraintInput, SoftPriorInput},
};
use image::GrayImage;

const TILE: u32 = 256;
const HALO: u32 = 16;

pub struct TiledSoftMatte {
    pub alpha: GrayImage,
    pub tile_count: usize,
    pub processed_tile_pixels: u64,
    pub iteration_limit: usize,
    pub iterations_total: usize,
    pub maximum_tile_residual_rms: f64,
    pub estimated_stencil_terms: u64,
}

#[derive(Clone, Copy)]
struct Tile {
    output: [u32; 4],
    sample: [u32; 4],
}

fn tiles(width: u32, height: u32) -> impl Iterator<Item = Tile> {
    (0..height).step_by(TILE as usize).flat_map(move |y| {
        (0..width).step_by(TILE as usize).map(move |x| {
            let right = (x + TILE).min(width);
            let bottom = (y + TILE).min(height);
            Tile {
                output: [x, y, right, bottom],
                sample: [
                    x.saturating_sub(HALO),
                    y.saturating_sub(HALO),
                    (right + HALO).min(width),
                    (bottom + HALO).min(height),
                ],
            }
        })
    })
}

/// Pure full-resolution experimental API. Only explicit user constraints are
/// exact. Every local solve must converge, or the entire call returns an error.
/// Borrowed full source/prior/constraints are excluded from scratch admission.
pub fn estimate(
    colors: &[[f32; 3]],
    (width, height): (u32, u32),
    prior: &[u8],
    input: SoftPriorInput<'_>,
    params: &ClosedFormParams,
    control: Option<&DrJobControl>,
) -> Result<TiledSoftMatte, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    input.validate()?;
    params.validate()?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0
        || pixels > MAX_PIXELS
        || colors.len() != pixels as usize
        || prior.len() != colors.len()
        || input
            .user_constraints
            .is_some_and(|p| p.len() != colors.len())
    {
        return Err("tiled matting dimensions or buffer lengths are invalid".into());
    }
    let mut tile_count = 0;
    let mut processed = 0_u64;
    let mut largest = 0_u64;
    for tile in tiles(width, height) {
        job.check()?;
        let [x, y, right, bottom] = tile.sample;
        let area = u64::from(right - x) * u64::from(bottom - y);
        largest = largest.max(area);
        processed += area;
        tile_count += 1;
    }
    let iteration_limit = ((MAX_STENCIL_TERMS / (25 * processed)).saturating_sub(3) / 5)
        .min(params.max_iterations as u64) as usize;
    if iteration_limit == 0 {
        return Err(
            "tiled matting exceeds the aggregate bounded solver work budget; narrow the unknown band or refine a smaller area"
                .into(),
        );
    }
    let estimated_stencil_terms = processed * 25 * (5 * iteration_limit as u64 + 3);
    // One full output, per-tile borrowed-field copies (12-byte RGB + 2 masks),
    // and the existing solver's conservative 265-byte per-pixel peak.
    let reservation = pixels
        + 14 * largest
        + scratch_estimate(largest as u32, 1, largest as usize)
            .ok_or("tiled matting scratch overflow")?;
    job.reserve(reservation)?;
    let job = job.with_reservation_floor(reservation);
    let mut output = vec![0; pixels as usize];
    let mut iterations_total = 0;
    let mut max_residual = 0_f64;
    let local_params = ClosedFormParams {
        max_iterations: iteration_limit,
        ..*params
    };
    for (number, tile) in tiles(width, height).enumerate() {
        let [x, y, right, bottom] = tile.sample;
        let (w, h) = (right - x, bottom - y);
        let area = w as usize * h as usize;
        let mut tile_colors = Vec::with_capacity(area);
        let mut tile_prior = Vec::with_capacity(area);
        let mut tile_labels = Vec::with_capacity(area);
        for row in y..bottom {
            job.check()?;
            let start = row as usize * width as usize + x as usize;
            let range = start..start + w as usize;
            tile_colors.extend_from_slice(&colors[range.clone()]);
            tile_prior.extend_from_slice(&prior[range.clone()]);
            if let Some(labels) = input.user_constraints {
                tile_labels.extend_from_slice(&labels[range]);
            } else {
                tile_labels.resize(tile_labels.len() + w as usize, 128);
            }
        }
        let local_input = SoftPriorInput {
            user_constraints: Some(&tile_labels),
            ..input
        };
        let result = estimate_input_job(
            &tile_colors,
            (w, h),
            ConstraintInput::Soft(local_input),
            &tile_prior,
            &local_params,
            job.range(
                number as f64 / tile_count as f64,
                (number + 1) as f64 / tile_count as f64,
            ),
        )
        .map_err(|error| format!("tiled matting tile {number} at {x},{y} refused: {error}"))?;
        iterations_total += result.iterations;
        max_residual = max_residual.max(result.residual_rms);
        let [out_x, out_y, out_right, out_bottom] = tile.output;
        for row in out_y..out_bottom {
            job.check()?;
            let dst = row as usize * width as usize + out_x as usize;
            let src = (row - y) as usize * w as usize + (out_x - x) as usize;
            let count = (out_right - out_x) as usize;
            output[dst..dst + count].copy_from_slice(&result.alpha.as_raw()[src..src + count]);
        }
    }
    job.report(1.0, 5)?;
    Ok(TiledSoftMatte {
        alpha: GrayImage::from_raw(width, height, output).ok_or("invalid tiled matting output")?,
        tile_count,
        processed_tile_pixels: processed,
        iteration_limit,
        iterations_total,
        maximum_tile_residual_rms: max_residual,
        estimated_stencil_terms,
    })
}
