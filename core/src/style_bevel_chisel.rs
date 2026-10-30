//! KERNEL: the Chisel law of the Chisel Hard and Chisel Soft bevels (#2),
//! measured against Photoshop, for each (style, technique) arm the runner
//! shipped: a facet height from the distance to the contour, the NORMALIZED
//! shading at a facet-slope scale `k` per technique, and Soften as a box pair
//! on the height. The adapter is `style_fx_bevel_emboss`, whose
//! `law_shading` runs the law's steps through the Smooth law's own
//! primitives (`style_bevel_smooth`: `pillow_height`,
//! `soften_shading_with_job`, `smooth_shade_with_job`) and keeps today's
//! lateral Chisel kernel for every arm [`chisel_law_applies`] refuses. Every
//! depth (8, 16, 32 bits and CMYK ink) renders through that one u8 kernel.
//!
//! # The law (`CH3` Hard, `CS3` Soft)
//!
//! - FACET ([`facet_size`]): `S_f` = `size` for an Inner and an Outer Bevel
//!   and `size / 2` for Emboss and Pillow Emboss (half-Size facets either
//!   side of the edge, as Smooth Emboss is `B` at half the Size).
//! - HEIGHT `m` ([`facet_height_with_job`]), from THE signed distance field
//!   (`doc_select::signed_distance_field_with_context`: `s + 0.5` the
//!   distance from an inside pixel to the nearest outside one, `0.5 − s` the
//!   reverse): `di = clamp((s + 0.5) / S_f, 0, 1)` inside, `do = clamp((0.5 −
//!   s) / S_f, 0, 1)` outside; inner `di`, outer `1 − do`, emboss and pillow
//!   `(1 + di − do) / 2`.
//! - SOFT: Chisel Soft blurs `m` with THE blur at `sigma_for_size(size ·
//!   CHISEL_SOFT_RATIO)` (the shipped ratio on the full Size), giving `m_s`.
//! - ORDER: `m` → SOFT → the Pillow reflection (`pillow_height`, `m·a + (1 −
//!   m)(1 − a)`, the Smooth Pillow's reversal on the facet) → SOFTEN (a box
//!   pair of `pair_width(soften)` on that height) → the Contour → the slopes.
//!   No Soften on the shading.
//! - SCALE ([`facet_gain`]): `k = (W_f − delta) / (2 W_f)` with `W_f =
//!   pair_width(S_f)`, the gain `k · depth · W_f`, doubled for Emboss and
//!   Pillow Emboss (the facet rises 1/2 per side); the slope is `gain · (h[x+1]
//!   − h[x−1]) / 2`, negated for direction down, then NORMALIZED.
//! - A texture is masked by `round(255 · c(m_s))`, the contour of the
//!   unreflected height after SOFT and before SOFTEN, as Smooth's `c(H)`.
//!
//! # Why (the measurement)
//!
//! `scripts/photoshop-probe/bevel_578_law_fit.py` (TAKE3 CHISEL MEMBERS,
//! declared before scoring) fitted one `delta` per technique on identify-578
//! take3's 32 half-plane rows at altitude 30 and Soften 0, all four styles:
//! [`CHISEL_HARD_DELTA`] 0.86 and [`CHISEL_SOFT_DELTA`] 0.48 (Hard's `k` is
//! 0.39 at `W_f` 4 and 0.47 at `W_f` 16: the facet just under half of `B`'s
//! scale, nearer half as it widens). `--judge-chisel-take3` SHIPPED Pillow Emboss
//! for both techniques by rule (ii): all 8 of its bevel-chisel-578 take1 rows
//! fall (mean of row means 25.50 → 6.61 codes Hard, 24.10 → 6.39 Soft) and
//! its 88 held-out take3 rows fall from 7.93 to 2.31 and 8.03 to 1.89. Inner,
//! Outer and Emboss keep today's kernel: some existing rows rise (Inner 8 and
//! 4, Outer 4 and 4, Emboss 4 and 2, Hard and Soft).
//! `tests/style_bevel_chisel_take3_parity.rs` pins the take3 rows.
//!
//! # Locality
//!
//! [`chisel_law_reach`] is the pad `Effect::reach` reports: the distance
//! clamps at `S_f`, so a coverage change moves `m` no further than
//! `ceil(S_f) + 1`; Chisel Soft's blur adds its reach, the Soften box pair
//! `pair_width(soften) − 1` and the central difference one more. `facet_height_with_job`
//! polls `job` once per row (13A-5b-i).

use crate::job_control::JobContext;
use crate::style_bevel_smooth::pair_width;
use crate::style_fx_bevel_emboss::CHISEL_SOFT_RATIO;
use crate::style_model::{BevelStyle, BevelTechnique};
use crate::style_plane_ops::{blur_reach, sigma_for_size};

/// Chisel Hard's `delta` (module doc): the runner's FIT, 0.86 on the grid
/// 0.00..=1.60 step 0.02.
pub(crate) const CHISEL_HARD_DELTA: f32 = 0.86;

/// Chisel Soft's `delta` (module doc): the runner's FIT, 0.48.
pub(crate) const CHISEL_SOFT_DELTA: f32 = 0.48;

/// Whether the Chisel law renders this bevel (module doc): one arm per
/// (style, technique), true only for the arms `--judge-chisel-take3`
/// shipped (Pillow Emboss, Hard and Soft).
pub(crate) fn chisel_law_applies(style: BevelStyle, technique: BevelTechnique) -> bool {
    match (style, technique) {
        (_, BevelTechnique::Smooth) => false,
        (BevelStyle::InnerBevel, BevelTechnique::ChiselHard) => false,
        (BevelStyle::InnerBevel, BevelTechnique::ChiselSoft) => false,
        (BevelStyle::OuterBevel, BevelTechnique::ChiselHard) => false,
        (BevelStyle::OuterBevel, BevelTechnique::ChiselSoft) => false,
        (BevelStyle::Emboss, BevelTechnique::ChiselHard) => false,
        (BevelStyle::Emboss, BevelTechnique::ChiselSoft) => false,
        (BevelStyle::PillowEmboss, BevelTechnique::ChiselHard) => true,
        (BevelStyle::PillowEmboss, BevelTechnique::ChiselSoft) => true,
    }
}

/// The facet's width `S_f` in px (module doc): `size` for an Inner and an
/// Outer Bevel, `size / 2` for Emboss and Pillow Emboss.
pub(crate) fn facet_size(style: BevelStyle, size: f32) -> f32 {
    match style {
        BevelStyle::InnerBevel | BevelStyle::OuterBevel => size,
        BevelStyle::Emboss | BevelStyle::PillowEmboss => size / 2.0,
    }
}

/// The slope gain `k · depth · W_f` (doubled for Emboss and Pillow Emboss)
/// with `k = (W_f − delta) / (2 W_f)` and `W_f = pair_width(facet_size)`
/// (module doc); 0 where `W_f` is 0 (no facet). `depth` is 1 at 100 %.
pub(crate) fn facet_gain(
    style: BevelStyle,
    technique: BevelTechnique,
    size: f32,
    depth: f32,
) -> f32 {
    let width = pair_width(facet_size(style, size));
    if width == 0 {
        return 0.0;
    }
    let delta = match technique {
        BevelTechnique::ChiselSoft => CHISEL_SOFT_DELTA,
        BevelTechnique::Smooth | BevelTechnique::ChiselHard => CHISEL_HARD_DELTA,
    };
    let w_f = width as f32;
    let k = (w_f - delta) / (2.0 * w_f);
    let per_side = match style {
        BevelStyle::InnerBevel | BevelStyle::OuterBevel => 1.0,
        BevelStyle::Emboss | BevelStyle::PillowEmboss => 2.0,
    };
    k * depth * w_f * per_side
}

/// The unreflected facet height `m` per pixel (module doc's HEIGHT) from the
/// signed distance field `signed` (`w` px per row), before Chisel Soft's
/// blur. A facet of no width (a size that is not finite and positive) is a
/// flat plane (0 Inner, 1 Outer, 0.5 Emboss and Pillow). Polls `job` once per row.
pub(crate) fn facet_height_with_job(
    signed: &[f32],
    w: u32,
    style: BevelStyle,
    size: f32,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let s_f = facet_size(style, size);
    let ramp = |d: f32| {
        if s_f.is_finite() && s_f > 0.0 {
            (d / s_f).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let mut out = Vec::with_capacity(signed.len());
    for row in signed.chunks((w as usize).max(1)) {
        job.check()?;
        for &s in row {
            let inside = if s > 0.0 { ramp(s + 0.5) } else { 0.0 };
            let outside = if s < 0.0 { ramp(0.5 - s) } else { 0.0 };
            out.push(match style {
                BevelStyle::InnerBevel => inside,
                BevelStyle::OuterBevel => 1.0 - outside,
                BevelStyle::Emboss | BevelStyle::PillowEmboss => (1.0 + inside - outside) / 2.0,
            });
        }
    }
    Ok(out)
}

/// How far the Chisel law reads or writes from any pixel (module doc's
/// Locality): `ceil(S_f) + 1` (0 for a facet size that is not finite), plus
/// Chisel Soft's blur reach, plus the Soften box pair's `pair_width(soften) −
/// 1`, plus the central difference's 1. Every addition saturates.
pub(crate) fn chisel_law_reach(
    style: BevelStyle,
    technique: BevelTechnique,
    size: f32,
    soften: f32,
) -> u32 {
    let s_f = facet_size(style, size);
    let clamp = if s_f.is_finite() {
        (s_f.max(0.0).ceil() as u32).saturating_add(1)
    } else {
        0
    };
    let soft = match technique {
        BevelTechnique::ChiselSoft => blur_reach(sigma_for_size(size * CHISEL_SOFT_RATIO)),
        BevelTechnique::Smooth | BevelTechnique::ChiselHard => 0,
    };
    clamp
        .saturating_add(soft)
        .saturating_add(pair_width(soften).saturating_sub(1))
        .saturating_add(1)
}
