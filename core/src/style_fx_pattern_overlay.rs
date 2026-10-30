//! Pattern overlay (Layer Style > Pattern Overlay): the
//! shape filled with a tiled pattern ABOVE the pixels, below a Gradient and a
//! Colour Overlay (the dialog order), composited with the effect's
//! OWN blend mode and opacity — the `style_fx_gradient_overlay` shape.
//!
//! The tile, its PYRAMID level for the effect's scale and the sampler are
//! `pattern_tile`; where the tile sits is `style_pattern`'s FOLLOW rule,
//! `PatternFill::origin` in LAYER px — the layer's offset from
//! `ctx.layer_offset` enters only for an unlinked overlay, which is what
//! `LayerStyle::needs_canvas_alignment` keys the cache on. The level is built
//! once per render and shared by the plane's colour
//! ([`PlaneColor::Pattern`], evaluated per pixel at composite time, like a
//! gradient) and the coverage computed here: `round(shape coverage × tile
//! alpha)`, so a transparent texel lets the pixels show through and the
//! Interior invariant `coverage <= shape.coverage` holds.
//!
//! Only the layer rect of the plane is visited, polling `job` once per row.
//! Nothing renders at opacity 0 or over a shape with no coverage.

use crate::job_control::JobContext;
use crate::pattern_tile::PatternSampler;
use crate::style::{EffectKind, PatternOverlay};
use crate::style_render::{Contribution, Part, Placement, PlaneColor, RenderContext, Shape};

/// Renders `fx` over `shape`: one Interior contribution (kind PatternOverlay,
/// the placed tile as its colour, coverage = shape coverage × tile alpha, the
/// effect's blend and opacity, no shift, no knock-out). Empty at opacity 0 or
/// over a shape with no coverage.
pub(crate) fn render(
    fx: &PatternOverlay,
    shape: &Shape,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    if fx.skipped_by_settings() {
        return Ok(Vec::new());
    }
    job.check()?;
    if shape.coverage.iter().all(|&c| c == 0) {
        return Ok(Vec::new());
    }
    let sampler = crate::lab_style::painted_sampler(&fx.fill, ctx);
    job.check()?;
    let mut coverage = shape.coverage.clone();
    apply_tile_alpha(&sampler, &mut coverage, shape, shape.layer_rect(), job)?;
    Ok(vec![Contribution {
        kind: EffectKind::PatternOverlay,
        part: Part::Main,
        coverage,
        color: PlaneColor::Pattern(sampler),
        blend: fx.blend,
        opacity: fx.opacity,
        placement: Placement::Interior,
        shift: (0, 0),
        knocked_out_by_shape: false,
        instance: 0,
    }])
}

/// The ONE coverage × tile-alpha loop, shared by the overlay and a
/// Pattern stroke's band (`style_fx_stroke`): every nonzero `coverage` byte
/// of the plane inside `rect` (`(x0, y0, x1, y1)`, plane px, end exclusive)
/// becomes `round(c × a)`, `a` the placed tile's alpha at that pixel's layer
/// grid point. A byte outside `rect` is left as it is, so the caller's
/// `rect` must hold every nonzero byte. Polls `job` once per row.
pub(crate) fn apply_tile_alpha(
    sampler: &PatternSampler,
    coverage: &mut [u8],
    shape: &Shape,
    rect: (u32, u32, u32, u32),
    job: JobContext<'_>,
) -> Result<(), String> {
    let (x0, y0, x1, y1) = rect;
    for y in y0..y1 {
        job.check()?;
        for x in x0..x1 {
            let i = shape.idx(x, y);
            let cov = coverage[i];
            if cov == 0 {
                continue;
            }
            let lx = i64::from(x) + shape.origin.0;
            let ly = i64::from(y) + shape.origin.1;
            let a = sampler.at(lx, ly)[3];
            coverage[i] = (f32::from(cov) * a).round() as u8;
        }
    }
    Ok(())
}
