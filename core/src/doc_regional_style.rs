//! The 8-bit regional compositor's STYLED per-sample bodies (Phase 13A-3b-i,
//! split out of `doc_regional_tree` in 13A-3b-ii): an admitted style resolved
//! once per node ([`Styled`], `Styled::of` — the planes `LayerStyle::rendered_with_job`
//! builds, the reference's own cached preparation) and one sample of
//! `composite_styled_into` — the Below contributions, the pixel pass (a clip
//! base: into a private sample clamped to the planes' shape after every
//! step), the Interior contributions and the package. Like the tree, it holds
//! NO admission logic and NO pixel math of its own: every statement is index
//! arithmetic around `style_composite::composite_pixels_sample`,
//! `style_composite::composite_contribution_sample`,
//! `style_composite::replace_under_band_sample` (the inside band, over the
//! sample's own B0), `style_composite::mix_premultiplied`, `blend::composite_source_into` and
//! `Shape::coverage_byte` / `Shape::at`. Since 13A-3c a ROUTED style
//! (`style_knockout::routes_unit`: a knockout, or a grouped interior) takes
//! the unit pass (`regional_knockout`) in place of the pixel step, the
//! grouped Interior contributions staying inside it, and every member loop
//! hands its members the base-only sample (`member_env`'s rule). Since
//! 13A-3b-ii it also holds the
//! MATERIALIZED group: `resolve_materialized` (the synthetic raster
//! `doc_group::rendered_group` itself builds, charged against the render's
//! `RenderBudget`); the plan's dry run of the reference's budget and nesting
//! decisions, `materialization_charges`, lives in `doc_regional_budget`
//! (13A-4b-ii-b-2). The contract is `regional`'s module doc.

use std::sync::Arc;

use crate::blend::{blend_kind, composite_source_into, BlendMode};
use crate::blend_fill::fill_blend;
use crate::clip_unit::{clip_alpha, ClipStep};
use crate::doc::Layer;
use crate::doc_evaluation::ByteTransfer;
use crate::doc_group::{self, RenderBudget};
use crate::doc_regional_progress::Run;
use crate::doc_regional_tree::{Node, Rect, ResolvedNode, ResolvedStep, Source};
use crate::job_control::JobContext;
use crate::regional_knockout::base_only_backdrop;
use crate::style::{BlendIf, Knockout, LayerStyle};
use crate::style_composite::{
    affected_window, composite_contribution_sample, composite_pixels_sample, mix_premultiplied,
    occluding_fill, replace_under_band_sample, CompositeEnv, PixelPass,
};
use crate::style_effects::EffectKind;
use crate::style_knockout::{
    backdrop_at, grouped_interior, is_grouped_interior, routes_unit, KnockoutBackdrop,
};
use crate::style_render::{Contribution as PlaneContribution, Ink, RenderContext, StyleColors};
use crate::style_render::{RenderedEffects, Shape};

/// Every contribution of `list` (with its [`Ink`]) whose plane pixel
/// lands on `canvas_xy`, through `composite_contribution_sample` at
/// `c.opacity · opacity` after the reference loop's early return:
/// `composite_contribution`'s placement — plane pixel `(px, py)` of a layer
/// at `offset` lands at `offset + shape.origin + (px, py) + shift` — inverted
/// at one sample. Index arithmetic only; shared with the typed tree
/// (`precision_regional_tree`), whose contributions are the same u8 planes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn contributions_sample(
    target: &mut [[f32; 4]],
    ti: usize,
    shape: &Shape,
    list: &[PlaneContribution],
    inks: &[Ink],
    offset: (i32, i32),
    opacity: f32,
    occluding_fill: f32,
    canvas_xy: (i64, i64),
) {
    for (c, ink) in list.iter().zip(inks) {
        let scale = c.opacity * opacity;
        if scale.is_nan() || scale <= 0.0 || c.coverage.len() != shape.coverage.len() {
            continue;
        }
        let px = canvas_xy.0 - i64::from(offset.0) - shape.origin.0 - i64::from(c.shift.0);
        let py = canvas_xy.1 - i64::from(offset.1) - shape.origin.1 - i64::from(c.shift.1);
        if px < 0 || py < 0 || px >= i64::from(shape.w) || py >= i64::from(shape.h) {
            continue;
        }
        composite_contribution_sample(
            target,
            ti,
            shape,
            c,
            ink,
            (px, py),
            scale,
            occluding_fill,
            canvas_xy,
        );
    }
}

/// The inside band `c` (with its [`Ink`]) at one sample: `replace_under_band`'s
/// placement inverted at `canvas_xy` exactly as [`contributions_sample`]
/// inverts `composite_contribution`'s, then `replace_under_band_sample` over
/// `b0`, the sample's own B0 (taken after the Below contributions, before the
/// pixel pass). Index arithmetic only; shared with the typed tree.
#[allow(clippy::too_many_arguments)]
pub(crate) fn band_sample(
    target: &mut [[f32; 4]],
    ti: usize,
    b0: [f32; 4],
    shape: &Shape,
    c: &PlaneContribution,
    ink: &Ink,
    offset: (i32, i32),
    opacity: f32,
    canvas_xy: (i64, i64),
) {
    // `replace_under_band`'s early return: a band at opacity 0 still replaces.
    let scale = c.opacity * opacity;
    if scale.is_nan() || scale < 0.0 || c.coverage.len() != shape.coverage.len() {
        return;
    }
    let px = canvas_xy.0 - i64::from(offset.0) - shape.origin.0 - i64::from(c.shift.0);
    let py = canvas_xy.1 - i64::from(offset.1) - shape.origin.1 - i64::from(c.shift.1);
    if px < 0 || py < 0 || px >= i64::from(shape.w) || py >= i64::from(shape.h) {
        return;
    }
    replace_under_band_sample(target, ti, b0, shape, c, ink, (px, py), scale, canvas_xy);
}

/// An admitted style resolved once per node: `composite_pixels_into`'s
/// sanitized fill and its non-identity Blend If, and — a raster, or a
/// materialized group's synthetic raster, whose style renders effects —
/// `composite_styled_into`'s planes (`LayerStyle::rendered_with_job`,
/// the reference's own cached preparation), each contribution's [`Ink`]
/// resolved once (Below then Interior, `composite_contribution`'s
/// `Contribution::ink`), the package window (`affected_window` within the enclosing
/// accumulator, canvas coordinates), whether the package model applies and
/// — 13A-3c — the style's knockout, whether it takes the unit pass
/// (`routes_unit`) and whether its grouped interiors move into the unit
/// (`grouped_interior`), both over the planes.
pub(crate) struct Styled {
    pub(crate) fill: f32,
    pub(crate) blend_if: Option<BlendIf>,
    /// `style_composite::occluding_fill` of the style and the layer's mode:
    /// what a Below stroke band divides by (#578).
    pub(crate) occluding_fill: f32,
    pub(crate) planes: Option<Arc<RenderedEffects>>,
    pub(crate) inks: Vec<Ink>,
    pub(crate) window: Option<Rect>,
    pub(crate) package: bool,
    pub(crate) knockout: Knockout,
    pub(crate) routed: bool,
    pub(crate) grouped: bool,
}

impl Styled {
    /// `composite_styled_into`'s inputs for `layer` (a raster, or a group's
    /// synthetic raster) under `style`: the pixel pass at the sanitized fill
    /// with the non-identity Blend If, and the planes only with an enabled
    /// effect — `None` (a plane past `MAX_PIXELS`) renders exactly as an
    /// effect-less style. `clip` is the enclosing accumulator's canvas rect.
    /// The planes report
    /// inside `run.range(job, work)` (13A-5b-iii-b-2: `work` the plan's
    /// `style_scratch(..).1` for them, 0 without), then `run` advances by
    /// `work`; a cancelled render caches nothing (`rendered_with_job`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn of(
        layer: &Layer,
        style: &LayerStyle,
        env: CompositeEnv<'_>,
        clip: Rect,
        job: JobContext<'_>,
        run: &mut Run<'_>,
        work: u64,
    ) -> Result<Styled, String> {
        let mut out = Styled {
            fill: crate::opacity_byte::quantized(style.fill_opacity),
            blend_if: style.blend_if.filter(|b| !b.is_identity()),
            occluding_fill: occluding_fill(
                style,
                style.blend_if.as_ref(),
                layer.blend,
                false,
                // A styled adjustment is refused before the tree is built
                // (`regional::STYLED_ADJUSTMENT`).
                false,
            ),
            planes: None,
            inks: Vec::new(),
            window: None,
            package: false,
            knockout: style.knockout,
            routed: false,
            grouped: false,
        };
        let planes = if style.has_enabled_effects() {
            style.rendered_with_job(
                layer,
                &RenderContext {
                    light: env.light,
                    canvas: env.canvas,
                    layer_offset: layer.offset,
                    lab_tiles: false,
                },
                run.range(job, work),
            )?
        } else {
            None
        };
        run.advance(job, work)?;
        out.routed = routes_unit(style, planes.as_deref());
        out.grouped = grouped_interior(style, planes.as_deref());
        if let Some(planes) = planes {
            out.inks = planes
                .below
                .iter()
                .chain(&planes.interior)
                .map(|c| c.ink(StyleColors::Encoded(env.colors)))
                .collect();
            // `rel` against origin (0, 0) is canvas coordinates; the
            // reference clips the window to its accumulator.
            let (x0, y0, x1, y1) = affected_window(layer.offset, &planes.shape, &planes.below);
            out.window = Some((
                x0.max(clip.0),
                y0.max(clip.1),
                x1.min(clip.2),
                y1.min(clip.3),
            ));
            out.package = crate::opacity_byte::quantized(layer.opacity) < 1.0
                && layer.blend != BlendMode::Dissolve;
            out.planes = Some(planes);
        }
        Ok(out)
    }
}

impl ResolvedNode<'_> {
    /// One sample of `composite_styled_into` WITH planes, or ROUTED (13A-3c),
    /// onto `target[ti]`: the package (planes, opacity below 1, not Dissolve)
    /// — outside the affected window the sample is untouched, inside it the
    /// direct steps run at opacity 1 on a copy that `mix_premultiplied`
    /// mixes back, always — or the direct steps at `opacity`. `under` is
    /// `target[ti]` before this node; `steps` is the clip run of a base
    /// (`None` for a lone node or a member); `knockout` the backdrop pair in
    /// play (the package's strip window of it is the same sample). `stop`
    /// (13A-4b-ii-b-2, `Some(m)` only from a staged ENTER / RESUME to a
    /// member cursor) leaves the unit's private buffer after `steps[..m]` in
    /// `target[ti]` (`direct_sample`); under the package it is unreachable
    /// (the walk refuses `PACKAGED_BASE`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn styled_sample(
        &self,
        target: &mut [[f32; 4]],
        ti: usize,
        under: [f32; 4],
        opacity: f32,
        pixel: Option<[u8; 4]>,
        steps: Option<&[ResolvedStep<'_>]>,
        knockout: KnockoutBackdrop<'_>,
        transfer: Option<&ByteTransfer>,
        canvas_xy: (i64, i64),
        stop: Option<usize>,
    ) {
        let Some(style) = self.style.as_ref() else {
            return;
        };
        if !style.package {
            self.direct_sample(
                target, ti, under, opacity, pixel, steps, knockout, transfer, canvas_xy, stop,
            );
            return;
        }
        if stop.is_some() {
            // The byte PACKAGE runs its members per strip: refused by the
            // walk (`PACKAGED_BASE`, owner 13B).
            debug_assert!(false, "a packaged base is never staged");
            return;
        }
        let (x, y) = canvas_xy;
        let Some((x0, y0, x1, y1)) = style.window else {
            return;
        };
        if x < x0 || y < y0 || x >= x1 || y >= y1 {
            return; // outside the affected window: untouched
        }
        let mut copy = [target[ti]];
        self.direct_sample(
            &mut copy, 0, under, 1.0, pixel, steps, knockout, transfer, canvas_xy, None,
        );
        target[ti] = mix_premultiplied(target[ti], copy[0], opacity);
    }

    /// `composite_direct_into` with planes, or routed, at one sample: the
    /// Below contributions, the pixel pass (a clip base: into a private
    /// sample at Normal/1, every step with the base-only backdrop, the alpha
    /// clamped to the planes' shape after each, then
    /// `composite_buffer_into`'s statement) — or, ROUTED, the unit pass
    /// (`unit_pass_sample`) — then the Interior contributions, less the
    /// grouped ones the unit already holds (D6). Blend If reads `under`, the
    /// snapshot's value. With `stop` (13A-4b-ii-b-2) the Below contributions
    /// run, then the clip branch's members `[..m]` (or the unit pass's), the
    /// private buffer left in `target[ti]`, and the body returns before
    /// `composite_source_into` and the Interior contributions.
    #[allow(clippy::too_many_arguments)]
    fn direct_sample(
        &self,
        target: &mut [[f32; 4]],
        ti: usize,
        under: [f32; 4],
        opacity: f32,
        pixel: Option<[u8; 4]>,
        steps: Option<&[ResolvedStep<'_>]>,
        knockout: KnockoutBackdrop<'_>,
        transfer: Option<&ByteTransfer>,
        canvas_xy: (i64, i64),
        stop: Option<usize>,
    ) {
        let Some(style) = self.style.as_ref() else {
            return;
        };
        let planes = self.planes();
        let (below_inks, interior_inks) = style
            .inks
            .split_at(planes.map_or(0, |planes| planes.below.len()));
        if let Some(planes) = planes {
            contributions_sample(
                target,
                ti,
                &planes.shape,
                &planes.below,
                below_inks,
                self.layer().offset,
                opacity,
                style.occluding_fill,
                canvas_xy,
            );
        }
        // The inside band's B0 (`composite_direct_into`, step 4): this
        // sample after the Below contributions, before the pixel pass.
        let b0 = backdrop_at(knockout, style.knockout, target[ti], 0);
        if style.routed {
            self.unit_pass_sample(
                target, ti, under, opacity, pixel, steps, knockout, transfer, canvas_xy, stop,
            );
        } else {
            match steps {
                None => self.pixel_pass(target, ti, under, opacity, self.blend, pixel, canvas_xy),
                Some(steps) => {
                    let mut buf = [[0.0f32; 4]];
                    self.pixel_pass(&mut buf, 0, under, 1.0, BlendMode::Normal, pixel, canvas_xy);
                    let a = buf[0][3];
                    let limit = self.footprint_limit(pixel, canvas_xy);
                    // `clip_unit`'s Open against the footprint (#578).
                    // Open only with a member step; a stop leaves it open
                    // (`doc_regional_sample::sample_clip_until`).
                    let footprint = limit.unwrap_or(a);
                    let opened = !steps.is_empty();
                    if opened {
                        buf[0][3] = clip_alpha(buf[0][3], footprint, ClipStep::Open);
                    }
                    // D4's clipped-member rule, the STYLED base's loop: a
                    // member with a knockout punches to the buffer as it
                    // stands after the base's pixel pass.
                    let base_only = [buf[0]];
                    let members =
                        base_only_backdrop(steps.iter().any(ResolvedStep::wants_base), &base_only);
                    let end = stop.map_or(steps.len(), |m| m.min(steps.len()));
                    for step in &steps[..end] {
                        if let ResolvedStep::Member(member) = step {
                            let under = buf[0];
                            let pixel = member.pixel(transfer, canvas_xy);
                            member.composite(
                                &mut buf,
                                0,
                                under,
                                crate::opacity_byte::quantized(member.layer().opacity),
                                member.blend,
                                pixel,
                                members,
                                transfer,
                                canvas_xy,
                            );
                        }
                        // `clip_unit`'s Settle, run for a clamp-only step
                        // too.
                        buf[0][3] = clip_alpha(buf[0][3], footprint, ClipStep::Settle);
                    }
                    if stop.is_some() {
                        target[ti] = buf[0];
                        return;
                    }
                    if opened {
                        buf[0][3] = clip_alpha(buf[0][3], footprint, ClipStep::Close);
                    }
                    let src = buf[0];
                    let sa = src[3] * opacity;
                    if sa > 0.0 {
                        composite_source_into(
                            target,
                            ti,
                            [src[0], src[1], src[2]],
                            sa,
                            blend_kind(self.blend),
                            canvas_xy,
                        );
                    }
                }
            }
        }
        if stop.is_some() {
            return;
        }
        if let Some(planes) = planes {
            // Under D6 the five GROUPED contributions were composited inside
            // the unit; only the rest remain out here.
            // A band is an interior Stroke (`inside_bands`' rule), tested per
            // contribution so the per-sample loop allocates nothing.
            for (c, ink) in planes
                .interior
                .iter()
                .zip(interior_inks)
                .filter(|(c, _)| !(style.grouped && is_grouped_interior(c.kind)))
            {
                if c.kind == EffectKind::Stroke {
                    band_sample(
                        target,
                        ti,
                        b0,
                        &planes.shape,
                        c,
                        ink,
                        self.layer().offset,
                        opacity,
                        canvas_xy,
                    );
                    continue;
                }
                contributions_sample(
                    target,
                    ti,
                    &planes.shape,
                    std::slice::from_ref(c),
                    std::slice::from_ref(ink),
                    self.layer().offset,
                    opacity,
                    style.occluding_fill,
                    canvas_xy,
                );
            }
        }
    }

    /// `composite_pixels_into`'s sample for an admitted style: nothing outside
    /// the (synthetic) layer's rect, the early return on a zero fill or
    /// opacity, then `composite_pixels_sample` with the Blend If reading
    /// `under`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pixel_pass(
        &self,
        target: &mut [[f32; 4]],
        ti: usize,
        under: [f32; 4],
        opacity: f32,
        mode: BlendMode,
        pixel: Option<[u8; 4]>,
        canvas_xy: (i64, i64),
    ) {
        let (Some(style), Some(px)) = (self.style.as_ref(), pixel) else {
            return;
        };
        // `composite_pixels_into`'s early return. A styled layer is refused
        // by the 11E walk, so `transfer` is `None` here and the pass decodes
        // nothing.
        if style.fill <= 0.0 || opacity <= 0.0 {
            return;
        }
        let coverage = self
            .mask_byte(canvas_xy)
            .map_or(1.0, |m| f32::from(m) / 255.0);
        let pass = PixelPass {
            joint: crate::opacity_byte::joint(style.fill, opacity, false),
            blend_if: style.blend_if.as_ref(),
            kind: blend_kind(mode),
            direct: mode == BlendMode::Normal,
            opacity,
            fill_blend: fill_blend(mode, style.fill, false),
        };
        composite_pixels_sample(target, ti, &px, 0, coverage, under, &pass, canvas_xy);
    }

    /// `Shape::of_layer(layer, 0)`'s coverage at `canvas_xy` over the
    /// (synthetic) layer's `pixel`: its alpha times the enabled mask's
    /// effective coverage; 0 outside the layer rect (the pad-0 plane's
    /// `Shape::at`).
    pub(crate) fn shape_byte(&self, pixel: Option<[u8; 4]>, canvas_xy: (i64, i64)) -> u8 {
        pixel.map_or(0, |px| {
            Shape::coverage_byte(px[3], self.mask_byte(canvas_xy))
        })
    }
}

/// A MATERIALIZED group's node (13A-3b-ii): `doc_group::rendered_group` — the
/// reference's own materialization, charged against `budget` exactly as the
/// full render charges it — over the group's extent within `clip`, and the
/// resulting synthetic raster resolved as a STYLED RASTER: its own (cropped,
/// `MaskProperties::at(origin)`) mask derived, its style (`renders_style` —
/// the synthetic has no meta) through `Styled::of` within `clip`, its blend
/// (`rendered_group`'s Normal for a group-only mode). `None` — nothing
/// visible inside, or an empty extent — drops the unit or the step exactly as
/// an empty isolated group's. `rendered_group` itself (13A-5b-ii-a: the
/// env's job, the ONE live-job entry into the byte reference chain) polls
/// `job` under `run.range(job, prep.materialization)` (13A-5b-iii-b-2: every
/// inner call site keeps its own `checkpoints_only`, so the chain reports
/// nothing and every inner blur's reports — the children's and the group's
/// own mask — land at the range's start), then `run` advances by it; the
/// synthetic's mask (feather 0: no blur) polls `job` (checkpoints only) and
/// its style's planes report inside their own range (`Styled::of`).
pub(crate) fn resolve_materialized<'a>(
    node: &Node<'a>,
    layers: &'a [Layer],
    env: CompositeEnv<'_>,
    clip: Rect,
    budget: &RenderBudget,
    job: JobContext<'_>,
    run: &mut Run<'_>,
) -> Result<Option<ResolvedNode<'a>>, String> {
    let window = (
        clip.0 as i32,
        clip.1 as i32,
        (clip.2 - clip.0) as u32,
        (clip.3 - clip.1) as u32,
    );
    let chain = CompositeEnv {
        job: run.range(job, node.prep.materialization),
        ..env
    };
    let rendered =
        doc_group::rendered_group(layers, node.at, chain, window, node.layer.depth, budget)?;
    run.advance(job, node.prep.materialization)?;
    let Some(synthetic) = rendered else {
        return Ok(None);
    };
    let mask = synthetic
        .mask
        .as_ref()
        .filter(|_| synthetic.mask_enabled)
        .map(|raw| {
            synthetic
                .mask_properties
                .derived_with_job(raw, job.checkpoints_only())
        })
        .transpose()?;
    let style = synthetic
        .renders_style()
        .map(|style| Styled::of(&synthetic, style, env, clip, job, run, node.prep.style))
        .transpose()?;
    let blend = synthetic.blend;
    let mut resolved = ResolvedNode {
        layer: node.layer,
        index: node.at.idx,
        source: Source::Materialized(Box::new(synthetic)),
        mask,
        style,
        blend,
        shaped: false,
        guide: None,
        snapshot: false,
    };
    resolved.shaped = resolved.is_shaped();
    Ok(Some(resolved))
}
