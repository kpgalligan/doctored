//! Stroke (Layer Style > Stroke): a band of constant width along
//! the shape's contour, filled with a colour, a gradient or a pattern, composited with
//! the effect's own blend mode and opacity.
//!
//! # The band
//!
//! The contour is the shape's 50 % coverage line. On a binary shape the
//! inside band is built from the morphology primitives in `style_render`
//! (`doc_select`'s `shrink_mask`), so an inside stroke and a shrunk
//! selection of the same width agree pixel for pixel; the outside band, and
//! the inside band of an anti-aliased shape, measure their distance by
//! #2's measured rule:
//!
//! - Outside: `style_stroke_distance::outside_corner_band` — the shape grown
//!   by `size` with the distance taken as the 5 × 5 Euclidean-weight
//!   CHAMFER distance seeded from every touched pixel at `1 − c / 255`
//!   (`grown = coverage_byte(size + 0.5 − D)`), knocked out by the shape's
//!   own coverage (`× (255 − coverage)`, `Shape::inverted`) — every pixel
//!   within `size` px of the shape's coverage-weighted edge, with a ~1 px
//!   anti-aliased rim, where the layer is transparent. Along a straight edge
//!   the chamfer distance IS the Euclidean one, so the band there is
//!   `grow_mask`'s; the corner arc is the measured one (every rect corner of
//!   `stroke_exposure_578_take.py`'s take1 within 1.02 codes at 1–10 px, 8
//!   and 16 bits, where the Euclidean grow missed by up to 70), and so is the
//!   band under an anti-aliased edge (`stroke_exposure_578_take2.py`: a disc,
//!   a soft edge, a half-alpha rect and a 0 %-fill layer). Band and layer
//!   join as a disjoint union, so under a partly covered pixel the
//!   composite lands the band at `b / (1 − c·f)` before the layer draws over
//!   it (`style_stroke_distance::band_under_layer`, #2; within about one
//!   code of `stroke_composite_578_take.py`'s take where drawing the layer
//!   over the plain band was 64 off). A Below contribution: the pixels draw over it, so an
//!   opaque layer hides the band's inner edge (the outside stroke sits
//!   under the fill), a layer at 0 % fill shows the band as an outline,
//!   never a filled shape, and a layer painted at 50 % opacity carries at
//!   most a quarter of the band's tint under its
//!   interior rather than a solid fill of stroke colour. The proportion is
//!   the drop shadow's knock-out (`knocked_out_by_shape`), so every
//!   exterior effect knocks out alike; an anti-aliased edge pixel at
//!   alpha `a` sits over `1 − a` of the band and fades the stroke out
//!   smoothly instead of at a hard threshold.
//! - Inside: the `size` px of the shape nearest its edge. On a shape with a
//!   partial pixel the band is measured from the ANTI-ALIASED contour by NA1
//!   (`style_stroke_distance::inside_corner_band`, Center's inner half, #2):
//!   the fourth Stroke take (`stroke_aa_578_take4.py`) read every
//!   anti-aliased straight edge so within 1.02 codes where the 50 % threshold
//!   missed by 128, and the confirm take (`stroke_aa_578_confirm5.py`, on
//!   held-out discs, a rounded rect and a half plane at other coverages)
//!   read all 160 rows within 1.02 / 0.25, the curved contours included,
//!   where the `style_aa_band` band it replaces kept up to 10 codes at the
//!   band's inner edge; on a binary shape it is `coverage − erode(coverage,
//!   size)`, byte for byte as before. An
//!   Interior contribution, bounded by the shape per pixel (`min(c, band)`,
//!   `a.saturating_sub(b) <= a`); an anti-aliased edge pixel keeps its own
//!   alpha here, since the band IS the shape's edge. It is the
//!   INSIDE BAND: at composite time it replaces the layer under it rather than
//!   drawing over it (`style_composite::replace_under_band`) — with
//!   `s = band / coverage`, the share of the shape the band holds, the
//!   pixel becomes `lerp(X, over(B0, stroke), s)`, `B0` the stack beneath
//!   the layer — so a stroke at 50 % shows what lies under the layer, as
//!   measured against Photoshop at 8, 16 and 32 bits, never the layer's own
//!   colour.
//! - Center: the two halves at `size / 2` each, emitted as SEPARATE
//!   contributions so each lands in its own group (the outer half Below, the
//!   inner half Interior, which is the inside band and replaces the layer
//!   under it as an Inside stroke does). The band is centred on the
//!   contour, half outside and half inside; splitting at exactly half keeps
//!   `outside 2 + inside 2 == center 4` for a hard edge, which is the oracle
//!   the tests pin. The outer half is the outside band above (take1's
//!   Center 4 and 5 px rows: 1.02 codes, against the Euclidean grow's 18;
//!   take2's anti-aliased disc Center row ranks it above the old seeds). The
//!   inner half is `style_stroke_distance::inside_corner_band`, measured from
//!   the anti-aliased edge (#2: the take's disc Center 6 within one code
//!   where the 50 % threshold missed by 115; take2's disc Center 4 at 50 %
//!   keeps 63 and disc Center 3 at 50 % Fill 43, residuals of the inside
//!   band's replacement; identical on a hard rect). The fourth Stroke take
//!   kept it: Photoshop's Center fills its partial pixels with the whole
//!   stroke (`o`, not `o · c`), which the one replacement body
//!   (`style_composite::replace_under_band_sample`) cannot give Center alone
//!   while Inside reads `o · c`. Inside on an anti-aliased shape takes the
//!   same kernel, so Center's inner half and that Inside band are one band.
//!
//! On a binary shape the Inside band is `style_plane_ops::inside_stroke_band`,
//! which Edit ▸ Stroke… (`doc_fill_stroke::stroke_mask`) shares, unchanged by
//! the NA1 band above. Edit ▸ Stroke… keeps the
//! Euclidean `outside_stroke_band` (no Photoshop evidence for its corners),
//! so an outside layer-style stroke and an outside-stroked selection now
//! differ only where the two measures differ: at corners.
//!
//! `size == 0` draws nothing (a stroke of no width has no band), and a band
//! that ends up all zero — an empty or fully transparent shape — is dropped
//! rather than composited. At opacity 0 the inside band is still emitted and
//! still replaces the layer under it (it shows B0 there, as measured against
//! Photoshop — `style_v1x_take.py`'s `Ocut`); an outside band, or a centre stroke's outer half,
//! at opacity 0 is not emitted.
//!
//! # The fill
//!
//! `Color` is one solid colour. `Gradient` evaluates
//! `style_gradient::GradientSampler` (`t`, `color_with`; the opacity
//! `gradient_color`), the ONE gradient implementation, shared with the
//! gradient overlay, handed every band pixel's CENTRE, `(x + 0.5, y + 0.5)`
//! in layer coordinates; a Linear, Angle, Reflected or Radial fill reads the pixel's top-left
//! corner from there (`style_gradient`'s module doc). The box is
//! `style_stroke_fill::stroke_box`: with `align_with_layer` (the
//! default) the bounding box of the layer's non-transparent pixels GROWN by
//! the band's outward reach (`size` Outside, `size / 2` Center, 0 Inside) —
//! the box `stroke_fill_take.py`'s take chose over the Gradient Overlay's
//! unchanged content box; an outside band beyond the box's ends
//! holds the end stops' colours, which is what the sampler's clamp gives.
//! Without `align_with_layer` the box is the CANVAS rect, and the render
//! cache keys on the canvas and the offset for such styles
//! (`LayerStyle::needs_canvas_alignment`). A SHAPE BURST gradient ignores
//! the box: its `t` is the band's own distance field
//! (`style_stroke_fill::burst_field`), built once per stroke and handed to
//! each of its contributions, so Center's two halves share one field, and its
//! colour is the Classic ramp of that `t` (`style_stroke_fill`). The stop opacity
//! multiplies the band coverage, `round(coverage × a)`, so a transparent
//! stop lets the backdrop (or the pixels, for an inside band) show through;
//! the colour itself is a [`PlaneColor::Gradient`] evaluated per pixel at
//! composite time rather than a stored colour plane.
//!
//! `Pattern` is the Pattern Overlay's fill (`style_pattern_fill`): its
//! placed PYRAMID level (`PatternFill::sampler`, the TWO-PASS rotation and
//! the FOLLOW / AT-CREATION origin) built once per stroke and handed to each
//! contribution, the band's coverage multiplied by the tile's alpha through
//! `style_fx_pattern_overlay::apply_tile_alpha` (the one loop), and the
//! colour a [`PlaneColor::Pattern`]. The band is a window onto the overlay's
//! plane: `stroke_fill_take.py`'s take1 (PLACE = OVERLAY) matched every
//! pattern stroke row exactly.

use std::sync::Arc;

use crate::job_control::JobContext;
use crate::pattern_tile::PatternSampler;
use crate::style::{EffectKind, Stroke, StrokeFill, StrokePosition};
use crate::style_aa_band::has_partial;
use crate::style_fx_pattern_overlay::apply_tile_alpha;
use crate::style_gradient::{gradient_color, BurstField, GradientSampler};
use crate::style_render::{
    inside_stroke_band, slot_color, Contribution, Part, Placement, PlaneColor, RenderContext, Shape,
};
use crate::style_stroke_distance::{inside_corner_band, outside_corner_band};
use crate::style_stroke_fill::{burst_field, stroke_box};

/// Renders `fx` over `shape`: one Below (outside), one Interior (inside) or
/// both (center — the outer half Below, the inner half Interior)
/// contributions, each with the effect's own blend and opacity, no shift
/// and no composite-time knock-out (an outside band is already knocked
/// out by the shape in-plane).
/// Nothing at size 0, or when the band covers no pixel. At opacity 0 the
/// INSIDE band is still emitted (at opacity 0): an inside stroke at 0 %
/// still replaces the layer under its band with what lies beneath
/// (measured against Photoshop:
/// `scripts/photoshop-probe/style_v1x_take.py`'s `Ocut`), which is
/// `style_composite::replace_under_band` at a band of no colour; a Below
/// band at 0 % draws nothing and is not emitted.
/// Polls `job` before every primitive, inside the dilate/erode and once per
/// ROW of the gradient fill (13A-5b-i).
pub(crate) fn render(
    fx: &Stroke,
    shape: &Shape,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    if fx.opacity.is_nan() || fx.opacity < 0.0 || !fx.size.is_finite() || fx.size <= 0.0 {
        return Ok(Vec::new());
    }
    // Whether a Below band would draw anything (module doc of `render`).
    let lit = fx.opacity > 0.0;
    // A Shape Burst fill's field, once per stroke (module doc).
    let burst = burst_field(fx, shape, job)?;
    // A Pattern fill's placed level, once per stroke (module doc), so Center's
    // two halves share one level and the plan charges it once.
    let pattern = (fx.fill_type == StrokeFill::Pattern)
        .then(|| crate::lab_style::painted_sampler(&fx.pattern_fill, ctx));
    let mut out = Vec::with_capacity(2);
    match fx.position {
        StrokePosition::Outside if !lit => {}
        StrokePosition::Outside => {
            let band = outside_corner_band(&shape.coverage, shape.w, shape.h, fx.size, job)?;
            out.extend(contribution(
                fx,
                shape,
                ctx,
                band,
                Placement::Below,
                burst.clone(),
                pattern.clone(),
                job,
            )?);
        }
        StrokePosition::Inside => {
            let band = inner_band(fx.position, shape, fx.size, job)?;
            out.extend(contribution(
                fx,
                shape,
                ctx,
                band,
                Placement::Interior,
                burst.clone(),
                pattern.clone(),
                job,
            )?);
        }
        StrokePosition::Center => {
            // Half the width on each side of the contour (module doc).
            let half = fx.size / 2.0;
            if lit {
                let band = outside_corner_band(&shape.coverage, shape.w, shape.h, half, job)?;
                out.extend(contribution(
                    fx,
                    shape,
                    ctx,
                    band,
                    Placement::Below,
                    burst.clone(),
                    pattern.clone(),
                    job,
                )?);
            }
            let band = inner_band(fx.position, shape, half, job)?;
            out.extend(contribution(
                fx,
                shape,
                ctx,
                band,
                Placement::Interior,
                burst.clone(),
                pattern.clone(),
                job,
            )?);
        }
    }
    Ok(out)
}

/// The inside band of `position` at `n` px (module doc): NA1
/// (`inside_corner_band`) for Center's inner half, and for Inside when the
/// shape has a partial pixel; else (Inside on a binary shape)
/// `inside_stroke_band`, byte for byte as before.
///
/// `has_partial` gates the whole plane, so one partial pixel moves the
/// shape's binary regions to NA1 too. Along a hard axis-aligned edge the two
/// bands are equal at an integer `n` (every dialog Size; the confirm
/// runner's selftest checks n = 2, 3, 5, 8, 12): the nearest transparent
/// pixel is axial, so the chamfer distance is the Euclidean one. On a hard
/// diagonal or curved edge, and at a fractional `n` (an MCP Size), the two
/// can differ by the chamfer's off-axis lengths (`style_stroke_distance`'s
/// module doc); no take measured that.
fn inner_band(
    position: StrokePosition,
    shape: &Shape,
    n: f32,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    if position == StrokePosition::Center || has_partial(&shape.coverage) {
        return inside_corner_band(&shape.coverage, shape.w, shape.h, n, job);
    }
    inside_stroke_band(&shape.coverage, shape.w, shape.h, n, job)
}

/// One contribution of `band` at `placement` with the stroke's fill, or
/// `None` when the band covers nothing. `burst` is the stroke's Shape Burst
/// field (`None` for any other fill) and `pattern` its Pattern fill's placed
/// level (`None` for any other fill), each shared by its contributions.
#[allow(clippy::too_many_arguments)]
fn contribution(
    fx: &Stroke,
    shape: &Shape,
    ctx: &RenderContext,
    mut band: Vec<u8>,
    placement: Placement,
    burst: Option<Arc<BurstField>>,
    pattern: Option<PatternSampler>,
    job: JobContext<'_>,
) -> Result<Option<Contribution>, String> {
    job.check()?;
    if band.iter().all(|&v| v == 0) {
        return Ok(None);
    }
    let color = match fx.fill_type {
        StrokeFill::Color => slot_color(fx.color, fx.linear),
        StrokeFill::Gradient => {
            if fx.gradient.stops.is_empty() {
                // The parser requires two stops; a direct construction with
                // none has no colour to draw.
                return Ok(None);
            }
            gradient_fill(fx, shape, ctx, &mut band, burst, job)?
        }
        StrokeFill::Pattern => {
            let Some(sampler) = pattern else {
                return Ok(None);
            };
            // The band over the whole plane: an outside band reaches past the
            // layer rect (`apply_tile_alpha`'s rect holds every nonzero byte).
            apply_tile_alpha(&sampler, &mut band, shape, (0, 0, shape.w, shape.h), job)?;
            PlaneColor::Pattern(sampler)
        }
    };
    Ok(Some(Contribution {
        kind: EffectKind::Stroke,
        part: Part::Main,
        coverage: band,
        color,
        blend: fx.blend,
        opacity: fx.opacity,
        placement,
        shift: (0, 0),
        knocked_out_by_shape: false,
        instance: 0,
    }))
}

/// Multiplies every covered pixel of `band` by the stop opacity of `fx`'s
/// gradient at its centre (layer coordinates) and returns the gradient as
/// the band's per-pixel colour, over the stroke's box (`stroke_box`) or, for a
/// Shape Burst, reading `burst`. Only covered pixels are visited — with a
/// large shadow beside the stroke the padding can be most of the plane.
/// Polls `job` once per ROW (13A-5b-i).
fn gradient_fill(
    fx: &Stroke,
    shape: &Shape,
    ctx: &RenderContext,
    band: &mut [u8],
    burst: Option<Arc<BurstField>>,
    job: JobContext<'_>,
) -> Result<PlaneColor, String> {
    let fill = &fx.gradient;
    let (bx, by, bw, bh) = stroke_box(fill, fx, shape, ctx);
    let mut sampler = GradientSampler::new(fill, bx, by, bw, bh);
    if let Some(field) = burst {
        sampler = sampler.with_burst(field);
    }
    for y in 0..shape.h {
        job.check()?;
        for x in 0..shape.w {
            let i = shape.idx(x, y);
            let cov = band[i];
            if cov == 0 {
                continue;
            }
            let lx = (i64::from(x) + shape.origin.0) as f32 + 0.5;
            let ly = (i64::from(y) + shape.origin.1) as f32 + 0.5;
            let (_, a) = gradient_color(fill, sampler.t(lx, ly));
            let a = if a.is_finite() {
                a.clamp(0.0, 1.0)
            } else {
                0.0
            };
            band[i] = (f32::from(cov) * a).round() as u8;
        }
    }
    Ok(PlaneColor::Gradient(sampler))
}
