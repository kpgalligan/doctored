//! Bevel & Emboss (Layer Style > Bevel & Emboss): the shape is
//! treated as a relief whose height field the Technique builds (a box pair
//! of the coverage for Smooth, a clamped distance ramp for the two
//! Chisels), lit from (`angle`, `altitude`), and the lit and unlit slopes
//! become a highlight and a shadow contribution.
//!
//! # Smooth bevels (#578)
//!
//! Every Smooth bevel renders through the measured law `B`
//! (`style_bevel_smooth`, whose module doc holds the law per style and
//! why): the coverage through a box pair of width `W = law_width(style,
//! size)` (`pair_width(size)` for an Inner and an Outer Bevel,
//! `pair_width(size / 2)` for Emboss and Pillow Emboss), the slope
//! `SMOOTH_GAIN · depth · W · (h[x+1] − h[x−1]) / 2`, the NORMALIZED shading
//! (`style_texture::shade`) with or without a texture, and Soften as a box
//! pair on the shading ([`law_shading`]), after which a Smooth Outer Bevel
//! lit from 45 degrees or higher is cut to `W − 1` px outside the shape
//! (`outside_cut`, `cut_outside_with_job`). Pillow Emboss shades
//! `pillow_height(H)`, `H·a + (1 − H)(1 − a)`. The Contour sub-panel maps
//! the unit height before the shading (`map_unit_plane_with_job`), and a
//! texture is masked by `round(255 · c(H))`, the contour of the unreflected
//! height, for every style. Everything after the shading (the gloss, the
//! highlight and shadow bytes, the bands) is the shared tail below, and
//! [`reach`] is the law's `smooth_reach`. The distance height, the LATERAL
//! shading, the height soften and the stencil below cover the Chisels outside
//! the Chisel law only, byte for byte as before #578 shipped the law.
//!
//! # The Chisel law (#578 bevel phase 2)
//!
//! A Chisel arm `style_bevel_chisel::chisel_law_applies` names (Pillow
//! Emboss, Hard and Soft) renders through [`law_shading`] too, in the order
//! that module's doc gives: THE distance transform, the facet height `m`
//! (`facet_height_with_job`), Chisel Soft's blur (`plane_blur::floats` at
//! `sigma_for_size(size · CHISEL_SOFT_RATIO)`), the texture's mask `round(255
//! · c(m_s))`, the Pillow reflection (`pillow_height`), Soften as the box
//! pair on that height (`soften_shading_with_job`), the Contour, and the
//! NORMALIZED shading at `facet_gain`, with no Soften on the shading;
//! [`reach`] is its `chisel_law_reach`.
//!
//! # Technique (#578)
//!
//! `technique` picks the height field; everything after it (the Contour
//! sub-panel, `soften`, the shading, the texture and the banding) is one
//! shared tail. Smooth is the law above. The two Chisels
//! measure distance instead of blurring, through THE Euclidean distance
//! transform (`doc_select::signed_distance_field_with_context`: `s` positive
//! inside, membership at coverage ≥ 128, `s + 0.5` the distance to the
//! nearest outside pixel and `0.5 − s` to the nearest inside one):
//!
//! - `di = clamp((s + 0.5) / size, 0, 1)` inside (0 outside) and
//!   `do = clamp((0.5 − s) / size, 0, 1)` outside (0 inside): a straight
//!   ramp that reaches full height `size` px from the contour, which is
//!   Chisel Hard — a planar facet, so its shading is constant
//!   across the band instead of Smooth's rounded fall-off.
//! - Per style: inner `h = di`, outer `h = 1 − do` (the shape a plateau at
//!   full height, so the first outside pixel is no ridge), emboss `h = (1 +
//!   di − do) / 2`, pillow emboss `h = (1 − di − do) / 2`; stored as
//!   `round(255·h)`.
//! - Chisel Soft is Chisel Hard's height through THE blur at
//!   `sigma_for_size(size · CHISEL_SOFT_RATIO)`, which rounds the facet's
//!   crease and foot while keeping its planar middle.
//!
//! [`height_reach`] is the height field's locality for `Effect::reach`: a
//! Chisel's `ceil(size) + 1` (a distance beyond
//! `size` clamps, so an input change moves heights no further), plus Chisel
//! Soft's blur reach. The EDT's two f32 planes fall under `style_scratch`'s
//! float plan.
//!
//! # Shading
//!
//! The surface normal comes from central differences of `h`:
//! `n = normalize(−depth·k·∂h/∂x, −depth·k·∂h/∂y, 1)` with `k = size`. The
//! height is unit-scaled (0..1 regardless of `size`), so its slope shrinks
//! as the bevel widens; multiplying by `size` restores the slope of a bevel
//! whose height grows with its width, which is what makes a 20 px bevel
//! shade like a 5 px one instead of fading out. `depth` (1 = 100 %) scales
//! the slope, i.e. the contrast.
//!
//! The light vector is `L = (cos alt · cos ang, −cos alt · sin ang, sin alt)`
//! from `resolved_angle` / `resolved_altitude` (y-down: 0° lights from the
//! right, 90° from the top). Shading is `d = n.x·L.x + n.y·L.y` — the
//! LATERAL part of `n·L` only. Dropping `n.z·L.z` makes three things shade
//! to exactly 0: flat ground (`n = (0, 0, 1)`), slopes perpendicular to the
//! light's azimuth, and altitude 90 (`L = (0, 0, 1)`), whereas the full
//! `n·L − L.z` would darken every slope. Direction `down` negates `d`.
//! Highlight coverage is `round(255·clamp(d, 0, 1))`, shadow
//! `round(255·clamp(−d, 0, 1))`, each restricted to the bevel band by a
//! multiply with the shape (inner half → `coverage`, outer half →
//! `inverted`); the inner halves are Interior contributions, the outer
//! halves Below, all of kind `BevelEmboss` with the highlight's and the
//! shadow's own blend, colour and opacity.
//!
//! # Contours (`style_contour`)
//!
//! - The Contour sub-panel (`contour_enabled`) maps the height plane in
//!   place, every pixel, through the contour with its Range, after `soften`
//!   (`BEVEL_BEFORE_SOFTEN` false: the take scored both sides the same, and
//!   only after keeps a lifted ground flat at the plane border).
//! - The Gloss Contour maps the signed shading `d` (`map_gloss`) before it
//!   splits into the highlight and the shadow, inside the row loop that
//!   builds both planes, so it needs no buffer of its own. Its support is
//!   decided ONCE per pixel as the union of the two pre-gloss bytes,
//!   `max(coverage_byte(d), coverage_byte(-d)) >= SUPPORT_MIN`, and that one
//!   test restricts both halves (a gloss may flip a pixel between them);
//!   outside it `d' = 0`, so flat ground never fills the pad. Linear skips
//!   either pass.
//!
//! # Texture (`style_texture`)
//!
//! With `texture_enabled`, the texture's height plane is built ONCE per
//! render, after [`height_field`] returns (so it never exists during the
//! blurs) and before [`shade`]: `t` from `texture.fill.sampler(ctx.
//! layer_offset)` at each plane pixel's layer coordinate, masked by the
//! shape's coverage (or the bevel's height, per `TEXTURE_MASK_HEIGHT`). In
//! `shade` its slopes (`texture_slope`, the kernel's stencil and gain) are
//! ADDED to the height's own, and the signed shading is the kernel's law
//! (`style_texture::shade`) instead of the lateral one. Direction `down`
//! inverts the whole relief, the texture's slopes with the bevel's (for the
//! lateral law that is the same as negating `d`). Texture off takes the
//! lateral path below byte for byte. The texture's stencil is never wider
//! than `stencil_half_width`, so `Effect::reach` is unchanged.
//!
//! # Numeric choices
//!
//! - `size == 0` renders nothing: `k = 0` flattens every normal, and a bevel
//!   of no width has nothing to light.
//! - The height field is a u8 plane (the one-blur rule), so a central
//!   difference over ±`s` px resolves `k·∂h/∂x` in steps of
//!   `size / (255 · 2s)`. With `s = 1` a 4 px bevel has ~100 steps across
//!   its ramp's ~0.8 peak, but a 60 px one only ~17 and a 250 px one four —
//!   and the staircase shows as mottling wherever the ramp is nearly flat.
//!   The stencil therefore grows with the size, `s = round(size / 8)`
//!   (at least 1, so sizes below 12 keep the plain central difference the
//!   tests restate): the step stays about 1/50 of the peak slope at every
//!   size. The lip this makes at an inner bevel's cliff (`s` px of steeper
//!   slope where the stencil straddles the edge) reads as a rounded edge.
//! - Exactness at the plane border comes from the pad alone:
//!   `Effect::reach` for a bevel is the two blurs' reach plus the stencil
//!   half-width `s` (`stencil_half_width`, the same function `shade`
//!   uses), so the height field is zero for at least `s + 1` px inside
//!   every border — every read lands on the plane (reads are clamped
//!   anyway, but a clamped read of a non-zero border would be wrong) and
//!   no pixel outside the plane would have had a non-zero derivative. The
//!   stencil is therefore a function of `size` alone, never of where the
//!   shape sits on the plane, which is what lets the cache re-render a
//!   sub-plane of a brush stroke and get the full plane's bytes.
//! - Non-finite inputs (impossible from the parser, which clamps; only a
//!   directly constructed effect can carry them) shade to nothing: NaN
//!   falls through `clamp` and `as u8` to coverage 0, never a panic.
//! - A contribution whose plane is all zero (altitude 90, a slope facing
//!   nowhere) is dropped rather than composited for nothing.

use crate::blend::BlendMode;
use crate::job_control::JobContext;
use crate::style::{BevelDirection, BevelEmboss, BevelStyle, BevelTechnique, EffectKind};
use crate::style_bevel_chisel::{
    chisel_law_applies, chisel_law_reach, facet_gain, facet_height_with_job,
};
use crate::style_bevel_smooth::{
    applies, cut_outside_with_job, law_width, outside_cut, pair_width, pillow_height,
    smooth_height_with_job, smooth_reach, smooth_shade_with_job, soften_shading_with_job,
    SMOOTH_GAIN,
};
use crate::style_contour::{
    bevel_contour_lut, effect_lut, map_gloss, map_plane_with_job, map_unit_plane_with_job,
    BEVEL_BEFORE_SOFTEN, SUPPORT_MIN,
};
use crate::style_render::{
    blur_plane_with_job, blur_reach, multiply_planes, sigma_for_size, slot_color, Contribution,
    Part, Placement, PlaneColor, RenderContext, Shape,
};
use crate::style_texture::{
    mask_heights, texture_gain, texture_heights_with_job, texture_slope, TEXTURE_MASK_HEIGHT,
    TEXTURE_STENCIL,
};

/// Pixels of bevel per pixel of derivative stencil (module doc): the
/// stencil half-width is `round(size / STENCIL_DIVISOR)`, at least 1.
const STENCIL_DIVISOR: f32 = 8.0;

/// The central-difference stencil half-width for a bevel of `size` px
/// (module doc): `round(size / 8)` clamped to 1..=64. Shared with
/// `Effect::reach`, which pads the plane by it.
pub(crate) fn stencil_half_width(size: f32) -> u32 {
    if size.is_finite() {
        (size / STENCIL_DIVISOR).round().clamp(1.0, 64.0) as u32
    } else {
        1
    }
}

/// Chisel Soft's blur size as a fraction of the bevel's `size` (module doc's
/// Technique): Chisel Hard's height blurred at `sigma_for_size(size · r)`.
/// Declared 0.5 before the #578 take (`bevel_chisel_578_take.py`), which
/// judged r from 0.1..=1.0 and chose 0.1, the lowest mean of row means (the
/// best declared member ships whatever its residual, which that runner
/// records and `style_bevel_chisel_parity.rs` pins); always in (0, 1], so
/// the blur never reaches further than Smooth's own. The blur raises a sigma
/// below its 3-tap minimum, so under size 13 the core blurs more than the
/// runner's model of CS(0.1); the pins measure the core's own render.
pub(crate) const CHISEL_SOFT_RATIO: f32 = 0.1;

/// How far, in px, the height field of `fx` moves when the coverage changes
/// (module doc's Technique): Smooth's blur reach, a Chisel's distance clamp
/// `ceil(size) + 1` (0 for a non-finite size), plus Chisel Soft's blur
/// reach. `Effect::reach` adds the soften blur and the stencil.
pub(crate) fn height_reach(fx: &BevelEmboss) -> u32 {
    let clamp = || {
        if fx.size.is_finite() {
            fx.size.max(0.0).ceil() as u32 + 1
        } else {
            0
        }
    };
    match fx.technique {
        // `applies` routes every Smooth bevel through the law, whose reach
        // is `smooth_reach`: no Smooth height is built here.
        BevelTechnique::Smooth => 0,
        BevelTechnique::ChiselHard => clamp(),
        BevelTechnique::ChiselSoft => {
            clamp() + blur_reach(sigma_for_size(fx.size * CHISEL_SOFT_RATIO))
        }
    }
}

/// This bevel's reach in px for `Effect::reach` (module doc): the law's
/// `smooth_reach` for a Smooth bevel (its style picks the width), the Chisel
/// law's `chisel_law_reach` for its arms; otherwise the height field's own
/// reach (`height_reach`: a Chisel's distance clamp,
/// plus Chisel Soft's blur), plus the soften blur and the central
/// difference's stencil half-width, which reads that many neighbours
/// further out.
pub(crate) fn reach(fx: &BevelEmboss) -> u32 {
    if applies(fx.style, fx.technique) {
        smooth_reach(fx.style, fx.size, fx.soften)
    } else if chisel_law_applies(fx.style, fx.technique) {
        chisel_law_reach(fx.style, fx.technique, fx.size, fx.soften)
    } else {
        height_reach(fx) + blur_reach(sigma_for_size(fx.soften)) + stencil_half_width(fx.size)
    }
}

/// Renders `fx` over `shape`: up to four contributions — the inner
/// highlight and shadow (Interior) and the outer highlight and shadow
/// (Below), whichever the style has. Nothing at size 0, with both
/// opacities 0, or on an empty plane. Polls `job` before every primitive,
/// inside both blurs and once per ROW of every per-pixel loop (13A-5b-i).
pub(crate) fn render(
    fx: &BevelEmboss,
    shape: &Shape,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    let lit = |opacity: f32| opacity.is_finite() && opacity > 0.0;
    let (highlight_on, shadow_on) = (lit(fx.highlight_opacity), lit(fx.shadow_opacity));
    let (w, h) = (shape.w, shape.h);
    if !(highlight_on || shadow_on)
        || fx.size.is_nan()
        || fx.size <= 0.0
        || w == 0
        || h == 0
        || shape.coverage.len() != w as usize * h as usize
    {
        return Ok(Vec::new());
    }

    job.check()?;
    let inverted = shape.inverted();
    let shading = if applies(fx.style, fx.technique) || chisel_law_applies(fx.style, fx.technique) {
        law_shading(fx, shape, ctx, job)?
    } else {
        let height = height_field(fx, shape, job)?;
        let texture = if fx.texture_enabled {
            Some(texture_plane(fx, shape, &height, ctx, job)?)
        } else {
            None
        };
        shade(&height, texture.as_ref(), w, h, fx, ctx, job)?
    };
    // Both maps in one row loop: each vector gets the same bytes in the
    // same order as its own whole-plane map.
    // The gloss contour, when there is one, maps `d` first (module doc).
    let gloss = effect_lut(&fx.gloss_contour, fx.gloss_anti_aliased, None);
    let mut highlight: Vec<u8> = Vec::with_capacity(shading.len());
    let mut shadow: Vec<u8> = Vec::with_capacity(shading.len());
    for row in shading.chunks_exact(w as usize) {
        job.check()?;
        match &gloss {
            None => {
                highlight.extend(row.iter().map(|&d| coverage_byte(d)));
                shadow.extend(row.iter().map(|&d| coverage_byte(-d)));
            }
            Some(lut) => {
                for &d in row {
                    let supported = coverage_byte(d).max(coverage_byte(-d)) >= SUPPORT_MIN;
                    let d = if supported { map_gloss(d, lut) } else { 0.0 };
                    highlight.push(coverage_byte(d));
                    shadow.push(coverage_byte(-d));
                }
            }
        }
    }

    // Which halves the style has: the inner band is the shape, the outer
    // band its inverse.
    let (interior, below): (Option<&[u8]>, Option<&[u8]>) = match fx.style {
        BevelStyle::InnerBevel => (Some(&shape.coverage), None),
        BevelStyle::OuterBevel => (None, Some(&inverted)),
        BevelStyle::Emboss | BevelStyle::PillowEmboss => (Some(&shape.coverage), Some(&inverted)),
    };
    let mut out = Vec::with_capacity(4);
    for (band, placement) in [(below, Placement::Below), (interior, Placement::Interior)] {
        let Some(band) = band else {
            continue;
        };
        if highlight_on {
            push_banded(
                &mut out,
                &highlight,
                band,
                Part::Highlight,
                slot_color(fx.highlight_color, fx.highlight_linear),
                fx.highlight_blend,
                fx.highlight_opacity,
                placement,
                job,
            )?;
        }
        if shadow_on {
            push_banded(
                &mut out,
                &shadow,
                band,
                Part::Shadow,
                slot_color(fx.shadow_color, fx.shadow_linear),
                fx.shadow_blend,
                fx.shadow_opacity,
                placement,
                job,
            )?;
        }
    }
    Ok(out)
}

/// The law's signed shading (module doc's Smooth bevels), in order: the
/// height `H`, the contour `c`, the texture (masked by `round(255 · c(H))`),
/// the shading height (`c(H)`, or a Pillow's `c(pillow_height(H))`), the
/// shading at `SMOOTH_GAIN · depth · W` with `W = law_width(style, size)`,
/// the height and texture dropped, then the soften, then the outside cut
/// where `outside_cut` names one. The Chisel law's arm
/// (module doc's Chisel law) takes `H = m_s` from [`chisel_law_height`],
/// softens the shading height before the contour instead of the shading, and
/// shades at `facet_gain`. Polls `job` before every primitive and once per
/// ROW.
fn law_shading(
    fx: &BevelEmboss,
    shape: &Shape,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let (w, h) = (shape.w, shape.h);
    let chisel = chisel_law_applies(fx.style, fx.technique);
    let raw = if chisel {
        chisel_law_height(fx, shape, job)?
    } else {
        smooth_height_with_job(&shape.coverage, w, h, fx.style, fx.size, job)?
    };
    let contour = if fx.contour_enabled {
        bevel_contour_lut(&fx.contour, fx.contour_anti_aliased, fx.contour_range)
    } else {
        None
    };
    let mapped = |mut plane: Vec<f32>| -> Result<Vec<f32>, String> {
        if let Some(lut) = &contour {
            job.check()?;
            map_unit_plane_with_job(&mut plane, w, lut, job)?;
        }
        Ok(plane)
    };
    // `c(H)`, the texture's mask for every style; a Pillow shades its own
    // height, which is 1 far outside the shape (module doc).
    let pillow = fx.style == BevelStyle::PillowEmboss;
    // The Chisel law softens the shading height before the contour, so a
    // textured, softened Chisel keeps its unsoftened `m_s` for the mask.
    let soften_height = chisel && pair_width(fx.soften) > 0;
    let (mask_source, shading_height) = if pillow {
        job.check()?;
        let shading_height = pillow_height(&raw, &shape.coverage);
        (fx.texture_enabled.then_some(raw), shading_height)
    } else if soften_height && fx.texture_enabled {
        (Some(raw.clone()), raw)
    } else {
        (None, raw)
    };
    let shading_height = if soften_height {
        soften_shading_with_job(shading_height, w, h, fx.soften, job)?
    } else {
        shading_height
    };
    job.check()?;
    let height = mapped(shading_height)?;
    let mask_height = match mask_source {
        Some(source) => Some(mapped(source)?),
        None => None,
    };
    let texture = if fx.texture_enabled {
        let source = mask_height.as_deref().unwrap_or(&height);
        let mut mask: Vec<u8> = Vec::with_capacity(source.len());
        for row in source.chunks_exact(w as usize) {
            job.check()?;
            mask.extend(row.iter().map(|&v| coverage_byte(v)));
        }
        drop(mask_height);
        Some(texture_plane(fx, shape, &mask, ctx, job)?)
    } else {
        None
    };
    let gain = if chisel {
        facet_gain(fx.style, fx.technique, fx.size, fx.depth)
    } else {
        SMOOTH_GAIN * fx.depth * law_width(fx.style, fx.size) as f32
    };
    let shading = smooth_shade_with_job(
        &height,
        texture.as_ref().map(|t| (t.t.as_slice(), t.gain)),
        w,
        h,
        gain,
        light_vector(fx, ctx),
        fx.direction == BevelDirection::Down,
        job,
    )?;
    drop(height);
    drop(texture);
    if chisel {
        return Ok(shading);
    }
    let shading = soften_shading_with_job(shading, w, h, fx.soften, job)?;
    let altitude = fx.resolved_altitude(ctx.light);
    match outside_cut(fx.style, fx.technique, fx.size, altitude) {
        Some(n_out) => cut_outside_with_job(shading, &shape.coverage, w, h, n_out, job),
        None => Ok(shading),
    }
}

/// The Chisel law's `m_s` (module doc's Chisel law): THE distance transform,
/// the facet height `m` (the field dropped), and for Chisel Soft THE blur at
/// `sigma_for_size(size · CHISEL_SOFT_RATIO)`. Polls `job` inside each.
fn chisel_law_height(
    fx: &BevelEmboss,
    shape: &Shape,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let (w, h) = (shape.w, shape.h);
    job.check()?;
    let signed = crate::doc_select::signed_distance_field_with_context(
        &shape.coverage,
        w as usize,
        h as usize,
        job,
    )?;
    let mut m = facet_height_with_job(&signed, w, fx.style, fx.size, job)?;
    drop(signed);
    if fx.technique == BevelTechnique::ChiselSoft {
        job.check()?;
        crate::plane_blur::floats(
            &mut m,
            w,
            h,
            sigma_for_size(fx.size * CHISEL_SOFT_RATIO),
            job,
        )?;
    }
    Ok(m)
}

/// The light `(lx, ly, lz)` of the module doc's Shading, from
/// `resolved_angle` / `resolved_altitude` (y-down).
fn light_vector(fx: &BevelEmboss, ctx: &RenderContext) -> (f32, f32, f32) {
    let angle = fx.resolved_angle(ctx.light).to_radians();
    let altitude = fx.resolved_altitude(ctx.light).to_radians();
    (
        altitude.cos() * angle.cos(),
        -altitude.cos() * angle.sin(),
        altitude.sin(),
    )
}

/// The unit-scaled height field of the module doc as a u8 plane: the
/// technique's height shaped per style, then the shared tail (contour,
/// soften). Polls `job` before every primitive and inside every blur and
/// distance transform. `render` guarantees `w`, `h` > 0 and a plane of
/// `w·h` bytes.
fn height_field(fx: &BevelEmboss, shape: &Shape, job: JobContext<'_>) -> Result<Vec<u8>, String> {
    let (w, h) = (shape.w, shape.h);
    let mut height = match fx.technique {
        // `applies` routes every Smooth bevel through the law
        // (`law_shading`), so this arm never runs: a flat 0 plane.
        BevelTechnique::Smooth => vec![0; shape.coverage.len()],
        BevelTechnique::ChiselHard => chisel_height(fx, shape, job)?,
        BevelTechnique::ChiselSoft => {
            let mut height = chisel_height(fx, shape, job)?;
            job.check()?;
            blur_plane_with_job(
                &mut height,
                w,
                h,
                sigma_for_size(fx.size * CHISEL_SOFT_RATIO),
                job,
            )?;
            height
        }
    };
    let contour = if fx.contour_enabled {
        bevel_contour_lut(&fx.contour, fx.contour_anti_aliased, fx.contour_range)
    } else {
        None
    };
    if let (Some(lut), true) = (&contour, BEVEL_BEFORE_SOFTEN) {
        map_plane_with_job(&mut height, w, lut, None, job)?;
    }
    job.check()?;
    blur_plane_with_job(&mut height, w, h, sigma_for_size(fx.soften), job)?;
    if let (Some(lut), false) = (&contour, BEVEL_BEFORE_SOFTEN) {
        map_plane_with_job(&mut height, w, lut, None, job)?;
    }
    Ok(height)
}

/// Chisel Hard's height (module doc's Technique) as a u8 plane, from THE
/// distance transform; polls `job` inside it and once per ROW.
fn chisel_height(fx: &BevelEmboss, shape: &Shape, job: JobContext<'_>) -> Result<Vec<u8>, String> {
    let row = shape.w as usize;
    job.check()?;
    let signed = crate::doc_select::signed_distance_field_with_context(
        &shape.coverage,
        row,
        shape.h as usize,
        job,
    )?;
    let size = fx.size;
    let mut height = Vec::with_capacity(signed.len());
    for distances in signed.chunks_exact(row) {
        job.check()?;
        for &s in distances {
            let inside = if s > 0.0 {
                ((s + 0.5) / size).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let outside = if s < 0.0 {
                ((0.5 - s) / size).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let unit = match fx.style {
                BevelStyle::InnerBevel => inside,
                BevelStyle::OuterBevel => 1.0 - outside,
                BevelStyle::Emboss => (1.0 + inside - outside) / 2.0,
                BevelStyle::PillowEmboss => (1.0 - inside - outside) / 2.0,
            };
            height.push((255.0 * unit).round() as u8);
        }
    }
    Ok(height)
}

/// The texture's height plane and slope gain (module doc's Texture).
struct TexturePlane {
    t: Vec<f32>,
    gain: f32,
}

/// `t` at each plane pixel's layer coordinate, masked by the coverage (or
/// the height), and its gain. Polls `job` once per row.
fn texture_plane(
    fx: &BevelEmboss,
    shape: &Shape,
    height: &[u8],
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<TexturePlane, String> {
    let sampler = fx.texture.fill.sampler(ctx.layer_offset);
    let mut t = texture_heights_with_job(&sampler, shape.w, shape.h, shape.origin, job)?;
    job.check()?;
    mask_heights(
        &mut t,
        if TEXTURE_MASK_HEIGHT {
            height
        } else {
            &shape.coverage
        },
    );
    Ok(TexturePlane {
        t,
        gain: texture_gain(fx.texture.depth, fx.texture.invert, fx.depth, fx.size),
    })
}

/// The shading `d` of the module doc for every plane pixel: the lateral
/// dot product of the surface normal with the light, negated for
/// direction `down`; with a texture, its slopes added and the kernel's law
/// (module doc's Texture). Polls `job` before the all-zero scan and once
/// per ROW (13A-5b-i).
fn shade(
    height: &[u8],
    texture: Option<&TexturePlane>,
    w: u32,
    h: u32,
    fx: &BevelEmboss,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let (wi, hi) = (i64::from(w), i64::from(h));
    job.check()?;
    if height.iter().all(|&v| v == 0) {
        job.check()?;
        return Ok(vec![0.0; height.len()]);
    }
    let s = i64::from(stencil_half_width(fx.size));
    // depth·k·∂h/∂x with ∂h/∂x = (h[x+s] − h[x−s]) / (255 · 2s) — the u8
    // height's unit scale and the stencil width folded into one gain.
    let gain = fx.depth * fx.size / (255.0 * 2.0 * s as f32);
    let (lx, ly, lz) = light_vector(fx, ctx);
    let sign = match fx.direction {
        BevelDirection::Up => 1.0,
        BevelDirection::Down => -1.0,
    };
    let at = |x: i64, y: i64| -> f32 {
        let (x, y) = (x.clamp(0, wi - 1), y.clamp(0, hi - 1));
        f32::from(height[(y * wi + x) as usize])
    };
    let mut out = Vec::with_capacity(height.len());
    if let Some(texture) = texture {
        for y in 0..hi {
            job.check()?;
            for x in 0..wi {
                let (tx, ty) = texture_slope(&texture.t, w, h, x, y, TEXTURE_STENCIL, texture.gain);
                // Direction `down` inverts the whole relief (module doc).
                let hx = ((at(x + s, y) - at(x - s, y)) * gain + tx) * sign;
                let hy = ((at(x, y + s) - at(x, y - s)) * gain + ty) * sign;
                out.push(crate::style_texture::shade(hx, hy, (lx, ly, lz)));
            }
        }
        return Ok(out);
    }
    for y in 0..hi {
        job.check()?;
        for x in 0..wi {
            let hx = (at(x + s, y) - at(x - s, y)) * gain;
            let hy = (at(x, y + s) - at(x, y - s)) * gain;
            // n = (−hx, −hy, 1) / |(−hx, −hy, 1)|; d = n.x·L.x + n.y·L.y.
            let inv_len = 1.0 / (1.0 + hx * hx + hy * hy).sqrt();
            out.push(-(hx * lx + hy * ly) * inv_len * sign);
        }
    }
    Ok(out)
}

/// `round(255 · clamp(d, 0, 1))`; NaN clamps to NaN and casts to 0.
fn coverage_byte(d: f32) -> u8 {
    (255.0 * d.clamp(0.0, 1.0)).round() as u8
}

/// `plane × band / 255` as one solid contribution tagged `part` (the
/// highlight or the shadow, so a re-stamp can tell them apart), unless it
/// is all zero. Polls `job` before the copy, the multiply and the scan.
#[allow(clippy::too_many_arguments)]
fn push_banded(
    out: &mut Vec<Contribution>,
    plane: &[u8],
    band: &[u8],
    part: Part,
    color: PlaneColor,
    blend: BlendMode,
    opacity: f32,
    placement: Placement,
    job: JobContext<'_>,
) -> Result<(), String> {
    job.check()?;
    let mut coverage = plane.to_vec();
    job.check()?;
    multiply_planes(&mut coverage, band);
    job.check()?;
    if coverage.iter().all(|&v| v == 0) {
        return Ok(());
    }
    let mut c = Contribution::solid(
        EffectKind::BevelEmboss,
        coverage,
        color,
        blend,
        opacity,
        placement,
    );
    c.part = part;
    out.push(c);
    Ok(())
}
