//! KERNEL: the law `B` of the Smooth bevels (#2), measured against
//! Photoshop, in all four styles: a box-pair height, a slope of Depth · W,
//! the NORMALIZED shading and Soften on the shading. The adapter is
//! `style_fx_bevel_emboss`, which renders a Chisel through the Chisel law
//! (`style_bevel_chisel`, its shipped arms) or today's kernel;
//! every depth (8, 16, 32 bits and CMYK ink) renders through that one u8
//! kernel, so the law has no depth-specific code.
//!
//! # The law
//!
//! With `W = law_width(style, size)` and the coverage `a = coverage / 255`:
//!
//! - WIDTH ([`law_width`]): `pair_width(size)` for an Inner and an Outer
//!   Bevel; `pair_width(size / 2)` for Emboss and Pillow Emboss, whose
//!   height is `B`'s at half the Size, centred on the edge.
//! - HEIGHT ([`smooth_height_with_job`]): `H = box_pair(a, W)`, two passes
//!   of a `W`-wide box (`sampling_box::box_pair`, the inner glow's, one
//!   implementation), unmasked, for every style: an Inner and an Outer
//!   Bevel share it and the band (the shape, or its inverse) picks the half
//!   that draws; Emboss draws both halves. Pillow Emboss shades
//!   [`pillow_height`] instead, `H·a + (1 − H)(1 − a)`: the inside falls
//!   toward the edge and the outside rises away from it.
//! - SLOPE and SHADING ([`smooth_shade_with_job`]): `hx = c · depth · W ·
//!   (h[x+1] − h[x−1]) / 2` and the same in y, reads clamped to the plane,
//!   with `c` = [`SMOOTH_GAIN`]; a Texture adds its slopes
//!   (`style_texture::texture_slope`); direction `down` negates both; then
//!   `style_texture::shade`, the NORMALIZED law, with or without a texture.
//! - SOFTEN ([`soften_shading_with_job`]): the signed shading through a box
//!   pair of `pair_width(soften)`, nothing at width 0.
//! - OUTSIDE CUT ([`outside_cut`], [`cut_outside_with_job`]): a Smooth Outer
//!   Bevel lit from an altitude of at least [`CUT_ALTITUDE`] keeps its
//!   shading outside the shape only within `n_out = W − 1` px of it. After
//!   the Soften, `d` is zeroed at every pixel of coverage < 128 whose `(2 ·
//!   n_out + 1)²` box holds no pixel of coverage ≥ 128: a chessboard
//!   distance to the shape greater than `n_out`, the box pair's own square
//!   support, counted by `sampling_box::box_sum` (one implementation).
//!   Below that altitude, and for every other style and technique, nothing
//!   is cut.
//!
//! On a straight edge the slope is the closed form `depth · (W − 0.5 − k) /
//! W` for the pixel index `k < W` from the edge, and 0 beyond.
//!
//! A textured bevel's texture is masked by `round(255 · c(H))`, `c` the
//! Contour (identity when off), for every style: a Pillow's shading height
//! is 1 far outside the shape, so masking by it would texture the whole
//! padded plane past [`smooth_reach`] and make the render depend on the pad.
//!
//! # Why (the measurement)
//!
//! `scripts/photoshop-probe/bevel_578_law_fit.py` describes the identify
//! take (`~/devel/doctored-fixtures/photoshop/identify-578/take1`, Inner
//! Bevel, Up, 8 and 16 bits): the angle-0 highlight inverts to that closed
//! form within 0.0046 at every Size and is linear in Depth, Soften blurs the
//! shading rather than the height, and the law is separable, so the disc
//! needs no distance. Its FIT chose c = 1.00 (mean of row means 0.013 on 40
//! fit rows). `--judge` kept today for every technique because Emboss and
//! Pillow Emboss rose under `B`, a height Photoshop builds differently; the
//! controller's style split (`--judge-style-split`, the runner's STYLE
//! SPLIT JUDGE and RECORDED) shipped `B` for Smooth Inner and Outer: no
//! existing Inner or Outer row rises, all 46 fall (inner mean of row means
//! 18.63 → 0.63 codes, outer 8.94 → 4.01), and 142 of the 144 identify rows
//! meet 2 / 0.5 (the two Size 32 disc overhead shadows read 1.0 / 0.64,
//! `tests/style_bevel_identify_parity.rs` pins them).
//!
//! The second identify take (`identify-578/take2`, the runner's TAKE2
//! DESCRIPTION) measured Emboss as `B`'s height at half the Size (its slope
//! a box pair of `pair_width(size / 2)` at gain depth · W', Size 1 and 4 one
//! profile) and Pillow Emboss as that height with its outside reversed. The
//! runner's TAKE2 MEMBERS (`--judge-take2`, declared before scoring, no free
//! number) shipped both by rule (ii): no existing row rises and all 16 of
//! each style fall (style-parity-v1x take2 and bevel-chisel-578 take1:
//! Emboss mean of row means 7.48 → 1.10 codes, Pillow 19.45 → 4.93), and on
//! take2's 112 rows per style the mean of row means falls from 3.80 to 0.085
//! (Emboss, 76 meet 2 / 0.5) and from 4.77 to 0.32 (Pillow, 56 meet);
//! `tests/style_bevel_emboss_take2_parity.rs` pins the rest.
//!
//! The outside cut is `scripts/photoshop-probe/effect_kernels_578_p2_take.py`'s
//! SO1 (#2 part 2 phase 2). The third identify take measured Smooth Outer's
//! outside count as `W − 1` at 45 and 60 degrees and `W` at 30; a Euclidean
//! cut at that count (the bevel runner's E1) was right on the half plane but
//! dark on the disc's shadow, where Photoshop keeps the pixels a square
//! support reaches. Declared before scoring, with no free number, SO1 shipped
//! offline by rule (ii): all 16 existing Outer rows fall (mean of row means
//! 4.01 → 3.82 codes) and none rises, and on take3's 160 Outer rows the mean
//! falls 0.265 → 0.203 with none rising. The confirm take
//! (`effect-kernels-578-p2/take1`, an ellipse and a rect at altitude 40, 50
//! and 70, Size 6 and 12, Soften 0 and 3) CONFIRMED it by rule (ii): no row
//! rises and every row today misses falls (1.38 → 1.06), while the Euclidean
//! twin raised ten of the ellipse's shadow rows. Photoshop's darker outside
//! half at high altitudes is not modelled; `tests/style_bevel_kernels_578_take.rs`
//! pins the rows it leaves.
//!
//! # Locality
//!
//! [`smooth_reach`] is the pad `Effect::reach` reports: the box pair reads
//! `W − 1` px each way and the central difference one more, plus one for
//! the coverage's anti-aliased edge, plus the soften box pair's `Wf − 1`.
//! The outside cut reads `n_out < W` px each way, inside that pad.
//! Every per-pixel loop that takes a `job` polls it once per row (13A-5b-i);
//! the adapter polls before and after [`pillow_height`]'s one pass.

use crate::job_control::JobContext;
use crate::sampling_box::{box_pair, box_sum};
use crate::style_model::{BevelStyle, BevelTechnique};
use crate::style_texture::{shade, texture_slope, TEXTURE_STENCIL};

/// The slope's gain `c` (module doc): the runner's FIT, 1.00 on the grid
/// 0.90..=1.10.
pub(crate) const SMOOTH_GAIN: f32 = 1.0;

/// The lowest light altitude, in degrees, at which a Smooth Outer Bevel's
/// outside is cut (module doc's OUTSIDE CUT). The third identify take
/// measured the outside count `W` at 30 degrees and `W − 1` at 45 and 60,
/// with no take between, so the cut starts at the lowest altitude where
/// `W − 1` was measured; the confirm take's 40 and 50 bracket it.
pub(crate) const CUT_ALTITUDE: f32 = 45.0;

/// The minimum coverage byte of a shape pixel for the outside cut (module
/// doc): the 50 % threshold every distance in the crate measures from.
const CUT_MEMBER: u8 = 128;

/// The outside count `n_out` of the module doc's OUTSIDE CUT: `law_width(
/// style, size) − 1` for a Smooth Outer Bevel lit from at least
/// [`CUT_ALTITUDE`] degrees (`altitude` the resolved one); `None` for every
/// other bevel, a non-finite altitude, or a width of 0 (a size of 0, which
/// renders nothing).
pub(crate) fn outside_cut(
    style: BevelStyle,
    technique: BevelTechnique,
    size: f32,
    altitude: f32,
) -> Option<u32> {
    let cut = technique == BevelTechnique::Smooth
        && style == BevelStyle::OuterBevel
        && altitude.is_finite()
        && altitude >= CUT_ALTITUDE;
    if !cut {
        return None;
    }
    law_width(style, size).checked_sub(1)
}

/// The module doc's OUTSIDE CUT on the softened shading `d` (consumed; a new
/// plane is returned): every pixel of coverage < 128 with no pixel of
/// coverage ≥ 128 in its `(2 · n_out + 1)²` box (`sampling_box::box_sum`,
/// reads clamped to the plane) becomes 0. Polls `job` inside the box sum
/// and once per row.
pub(crate) fn cut_outside_with_job(
    d: Vec<f32>,
    coverage: &[u8],
    w: u32,
    h: u32,
    n_out: u32,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    job.check()?;
    let members = box_sum(
        |i| f64::from(u8::from(coverage[i] >= CUT_MEMBER)),
        w as usize,
        h as usize,
        n_out as usize,
        job,
    )?;
    let row = (w as usize).max(1);
    let mut out = Vec::with_capacity(d.len());
    for ((shading, cover), count) in d
        .chunks(row)
        .zip(coverage.chunks(row))
        .zip(members.chunks(row))
    {
        job.check()?;
        out.extend(shading.iter().zip(cover).zip(count).map(|((&v, &c), &n)| {
            if c < CUT_MEMBER && n == 0.0 {
                0.0
            } else {
                v
            }
        }));
    }
    Ok(out)
}

/// Whether the law renders this bevel (module doc): Smooth, in every style
/// the runner shipped, one arm each (Inner and Outer Bevel by the style
/// split, Emboss and Pillow Emboss by TAKE2 MEMBERS).
pub(crate) fn applies(style: BevelStyle, technique: BevelTechnique) -> bool {
    technique == BevelTechnique::Smooth
        && match style {
            BevelStyle::InnerBevel => true,
            BevelStyle::OuterBevel => true,
            BevelStyle::Emboss => true,
            BevelStyle::PillowEmboss => true,
        }
}

/// The law's box-pair width for a bevel of `style` and `size` (module doc):
/// `pair_width(size)` for an Inner or Outer Bevel, `pair_width(size / 2)`
/// for Emboss and Pillow Emboss (the height at half the Size, centred on
/// the edge). It sets both the height's width and the slope's gain.
pub(crate) fn law_width(style: BevelStyle, size: f32) -> u32 {
    match style {
        BevelStyle::InnerBevel | BevelStyle::OuterBevel => pair_width(size),
        BevelStyle::Emboss | BevelStyle::PillowEmboss => pair_width(size / 2.0),
    }
}

/// The box pair's width for a size or soften of `v` px: 0 when `v` is not
/// finite or ≤ 0; else `S = max(1, floor(v + 0.5))`, and 2 where `S = 1`.
/// The inner glow's `law_split` rule (a one-pixel box pair would be no blur
/// at all; the take shows Size 1 and Size 2 as one profile).
pub(crate) fn pair_width(v: f32) -> u32 {
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    let s = (f64::from(v) + 0.5).floor().max(1.0) as u32;
    if s == 1 {
        2
    } else {
        s
    }
}

/// How far the law reads or writes from any pixel (module doc's Locality):
/// `W + 1 + Wf − 1`, with `W = law_width(style, size)` and `Wf =
/// pair_width(soften)` (0 adds nothing).
pub(crate) fn smooth_reach(style: BevelStyle, size: f32, soften: f32) -> u32 {
    law_width(style, size)
        .saturating_add(1)
        .saturating_add(pair_width(soften).saturating_sub(1))
}

/// The height `H = box_pair(coverage / 255, W)` as f32, unmasked, with `W =
/// law_width(style, size)` (module doc): the style picks only the width.
/// `W` is at least 2 for a size > 0; a size of 0 is a flat 0 plane.
pub(crate) fn smooth_height_with_job(
    coverage: &[u8],
    w: u32,
    h: u32,
    style: BevelStyle,
    size: f32,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let width = law_width(style, size);
    job.check()?;
    if width == 0 {
        return Ok(vec![0.0; coverage.len()]);
    }
    let pair = box_pair(
        |i| f64::from(coverage[i]) / 255.0,
        w as usize,
        h as usize,
        width,
        job,
    )?;
    narrowed_with_job(&pair, w, job)
}

/// Pillow Emboss's shading height from `H` (module doc): `H·a + (1 − H)(1 −
/// a)` per pixel with `a = coverage / 255`, in f64, then narrowed. The inside
/// falls toward the edge and the outside rises away from it; far outside
/// the shape it is 1, flat.
pub(crate) fn pillow_height(height: &[f32], coverage: &[u8]) -> Vec<f32> {
    height
        .iter()
        .zip(coverage)
        .map(|(&h, &c)| {
            let (h, a) = (f64::from(h), f64::from(c) / 255.0);
            (h * a + (1.0 - h) * (1.0 - a)) as f32
        })
        .collect()
}

/// `plane` as f32, one row of `w` at a time, polling `job` per row.
fn narrowed_with_job(plane: &[f64], w: u32, job: JobContext<'_>) -> Result<Vec<f32>, String> {
    let mut out = Vec::with_capacity(plane.len());
    for row in plane.chunks((w as usize).max(1)) {
        job.check()?;
        out.extend(row.iter().map(|&v| v as f32));
    }
    Ok(out)
}

/// The signed shading `d` per pixel (module doc): the central differences
/// of `height` times `gain / 2` (reads clamped), plus the texture's slopes
/// (`(t, gain)`) when there is one, negated for `down`, through the
/// NORMALIZED law under `light` `(lx, ly, lz)`. Polls `job` once per row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn smooth_shade_with_job(
    height: &[f32],
    texture: Option<(&[f32], f32)>,
    w: u32,
    h: u32,
    gain: f32,
    light: (f32, f32, f32),
    down: bool,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let (wi, hi) = (i64::from(w), i64::from(h));
    let at = |x: i64, y: i64| -> f32 {
        let (x, y) = (x.clamp(0, wi - 1), y.clamp(0, hi - 1));
        height[(y * wi + x) as usize]
    };
    let half = gain / 2.0;
    let sign = if down { -1.0 } else { 1.0 };
    let mut out = Vec::with_capacity(height.len());
    for y in 0..hi {
        job.check()?;
        for x in 0..wi {
            let mut hx = (at(x + 1, y) - at(x - 1, y)) * half;
            let mut hy = (at(x, y + 1) - at(x, y - 1)) * half;
            if let Some((t, tgain)) = texture {
                let (tx, ty) = texture_slope(t, w, h, x, y, TEXTURE_STENCIL, tgain);
                hx += tx;
                hy += ty;
            }
            out.push(shade(hx * sign, hy * sign, light));
        }
    }
    Ok(out)
}

/// Soften on the shading (module doc): `d` through a box pair of
/// `pair_width(soften)`; `d` unchanged at width 0.
pub(crate) fn soften_shading_with_job(
    d: Vec<f32>,
    w: u32,
    h: u32,
    soften: f32,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let width = pair_width(soften);
    if width == 0 {
        return Ok(d);
    }
    job.check()?;
    let pair = box_pair(|i| f64::from(d[i]), w as usize, h as usize, width, job)?;
    drop(d);
    narrowed_with_job(&pair, w, job)
}
