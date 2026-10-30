//! Allocation/work planning for the shared typed tone guide. The selective
//! descriptor reader skips unrelated payloads, including large LUT strings.
//!
//! `adjust_precision_plan.rs` selectively reads guide parameters for allocation/work planning.
use crate::precision_image::pixel_count;
use crate::style_render::{downsample_factor, feather_radius, reduced_sigma, sigma_for_size};
use serde::Deserialize;
use std::borrow::Cow;

#[derive(Default, Deserialize)]
struct Band {
    amount: Option<f32>,
}
#[derive(Default, Deserialize)]
struct Params {
    radius: Option<f32>,
    shadows: Option<Band>,
    highlights: Option<Band>,
}
#[derive(Deserialize)]
struct Description<'a> {
    #[serde(borrow, rename = "type")]
    kind: Cow<'a, str>,
    #[serde(borrow)]
    evaluation: Cow<'a, str>,
    #[serde(borrow)]
    op: Cow<'a, str>,
    #[serde(default)]
    params: Params,
}
pub(super) fn description(meta: &str, w: u32, h: u32) -> Result<(u64, u64), String> {
    let value: Description<'_> = serde_json::from_str(meta)
        .map_err(|e| format!("invalid high-depth adjustment plan: {e}"))?;
    if value.kind != "adjust" || value.evaluation != "precision_v1" {
        return Err("high-depth adjustment requires evaluation:precision_v1".into());
    }
    if value.op != "shadows_highlights" {
        return Ok((0, 0));
    }
    let shadow = value.params.shadows.and_then(|v| v.amount).unwrap_or(0.35);
    let highlight = value
        .params
        .highlights
        .and_then(|v| v.amount)
        .unwrap_or(0.0);
    if ![shadow, highlight]
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    {
        return Err("invalid high-depth guide amount".into());
    }
    if shadow == 0.0 && highlight == 0.0 {
        return Ok((0, 0));
    }
    guide(w, h, value.params.radius.unwrap_or(30.0))
}
pub(crate) fn guide(w: u32, h: u32, radius: f32) -> Result<(u64, u64), String> {
    pixel_count(w, h)?;
    if !radius.is_finite() || !(0.0..=1000.0).contains(&radius) {
        return Err("invalid high-depth guide radius".into());
    }
    let work = GuideWork::of(w, h, radius);
    // Three stored planes, three temporary cell-sum arrays in the reduced
    // branch, then one separable Gaussian temporary. Includes returned guide.
    Ok((work.cells * 24 + work.taps * 4 + 4096, work.total()))
}
/// The guide build's work terms — the ONE home of [`guide`]'s work figure and
/// of the two grid blurs' sub-ranges `adjust_guide::build` reports under
/// (Phase 13A-5b-iii-c-1): the pixel pass, the per-cell means, each blur's
/// `2 · cells · taps`, the combine.
pub(crate) struct GuideWork {
    pub(crate) pixels: u64,
    pub(crate) cells: u64,
    pub(crate) taps: u64,
}
impl GuideWork {
    pub(crate) fn of(w: u32, h: u32, radius: f32) -> GuideWork {
        let sigma = sigma_for_size(radius);
        let factor = downsample_factor(sigma);
        let cells = u64::from(w.div_ceil(factor).max(1))
            .saturating_mul(u64::from(h.div_ceil(factor).max(1)));
        let grid_sigma = if factor == 1 {
            sigma
        } else {
            reduced_sigma(sigma, factor)
        };
        let taps = if grid_sigma <= 0.0 {
            0
        } else {
            u64::from(
                crate::gaussian_plane::nearest_odd(feather_radius(grid_sigma) * 2.0 + 1.0).max(3),
            )
        };
        GuideWork {
            pixels: u64::from(w).saturating_mul(u64::from(h)),
            cells,
            taps,
        }
    }
    pub(crate) fn total(&self) -> u64 {
        self.pixels.saturating_add(
            self.cells
                .saturating_mul(self.taps.saturating_mul(4).saturating_add(2)),
        )
    }
    /// The two grid blurs' sub-ranges as fractions of [`total`](Self::total):
    /// the pixel pass and per-cell means are flat before the first, each blur
    /// covers its `2 · cells · taps`, and the combine's `cells` follow the
    /// second.
    pub(crate) fn blur_ranges(&self) -> [(f64, f64); 2] {
        let t = self.total().max(1) as f64;
        let first = self.pixels.saturating_add(self.cells);
        let blur = self.cells.saturating_mul(self.taps).saturating_mul(2);
        let second = first.saturating_add(blur);
        let end = self.total().saturating_sub(self.cells);
        [
            (first as f64 / t, second as f64 / t),
            (second as f64 / t, end as f64 / t),
        ]
    }
}
