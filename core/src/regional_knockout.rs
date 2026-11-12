//! The regional compositor's KNOCKOUT UNIT PASS (Phase 13A-3c): D3's
//! interior unit — `style_knockout::composite_interior_unit_into` at 8 bits,
//! `precision_style_knockout::composite_interior_unit_into` at 16 — run ONE
//! SAMPLE at a time, so a knockout (Shallow / Deep) and "Blend Interior
//! Effects as Group" render sample for sample as the full projection renders
//! them. The rules — D3's order, D4's backdrops, D5's Background, D6's
//! grouped interiors — are `style_knockout`'s; the per-sample backdrop is the
//! reference's own `style_knockout::KnockoutBackdrop` over ONE-element
//! slices, read by `style_knockout::backdrop_at` at index 0. Where each
//! backdrop is born (the root's D5 split, a pass-through level's Shallow
//! plane, an isolated buffer's hard stop, a clip unit's base-only sample) is
//! the trees' index arithmetic (`doc_regional_tree`, `doc_regional_style`,
//! `precision_regional_tree`, `precision_regional`).
//!
//! Like the trees, this file holds NO admission logic (the walks decide every
//! refusal, and since 13A-3c neither refuses a knockout or a grouped
//! interior) and NO pixel math of its own: every statement is index
//! arithmetic around the reference's per-sample bodies —
//! `style_composite::composite_pixels_sample` /
//! `precision_style_render::composite_pixels_sample16` for the pixel pass
//! into the unit (at the unit's fill, Normal, opacity 1, `direct`),
//! `style_composite::composite_contribution_sample` (through
//! `doc_regional_style::contributions_sample`) for a grouped contribution,
//! the member composites the trees already run (`ResolvedNode::composite`,
//! `precision_regional_tree::sample_layers`), `style_knockout::backdrop_at`
//! for `B0`, `blend::composite_source_into` for `K = over(B0, U, q, M)` and
//! `style_composite::mix_premultiplied` for `mix(acc, K, s·opacity)`. The
//! predicates are the reference's verbatim: `style_knockout::routes_unit`,
//! `grouped_interior` and `is_grouped_interior` (decided once per node, in
//! `Styled::of` / `prepare_style`), and
//! `precision_render::members_want_base_snapshot16`.
//!
//! since 13A-3c, KNOCKOUT (Shallow/Deep) and "Blend Interior Effects as Group" wherever a style
//! sits: a style the reference routes (`style_knockout::routes_unit`) runs
//! `composite_interior_unit_into` ONE SAMPLE at a time (`regional_knockout.rs`: byte
//! `ResolvedNode::unit_pass_sample`, typed `unit_pass_sample16`, `grouped_interior_sample`) against
//! the reference's own `KnockoutBackdrop` over one-element slices (`backdrop_at` at index 0), born
//! where the reference births it — D5 at the root (8 bits `composite_projection_into`'s test on
//! `Level::background`; typed the scene's `KnockoutSeed`), a pass-through level's Shallow plane
//! (the sample as its children began, handed always, as the typed arm does), `NONE` into
//! isolated/materialized groups, the base-only sample for clip members (all three member loops).

use crate::blend::{blend_kind, composite_source_into, BlendMode};
use crate::blend_fill::fill_blend;
use crate::clip_unit::{clip_alpha, ClipStep};
use crate::doc::MAX_PIXELS;
use crate::doc_evaluation::ByteTransfer;
use crate::doc_regional_style::contributions_sample;
use crate::doc_regional_tree::{ResolvedNode, ResolvedStep, Source};
use crate::precision_regional_tree::{sample_layers, Node, PreparedStyle};
use crate::precision_render::{members_want_base_snapshot16, Kind};
use crate::precision_style_render::{composite_pixels_sample16, PixelPass16, StyleEnv};
use crate::style::Knockout;
use crate::style_composite::{
    composite_pixels_sample, mix_premultiplied, mix_premultiplied_in, PixelPass,
};
use crate::style_knockout::{backdrop_at, is_grouped_interior, KnockoutBackdrop};
use crate::style_render::{Contribution as PlaneContribution, Ink, InkDecode, Shape};

impl ResolvedNode<'_> {
    /// One sample of the byte `composite_interior_unit_into` onto
    /// `target[ti]`: nothing outside the (synthetic) layer's rect (the mix
    /// loop covers the layer rect only) or where the shape `s = alpha·mask`
    /// is 0; the pixel pass into a transparent unit sample at the unit's fill
    /// (1 under D6), its Blend If reading `under` — the full-rect snapshot's
    /// value; then — a clip base whose run holds a visible member (`steps`
    /// non-empty) — each member onto the unit with the base-only sample as
    /// BOTH its backdrops when some member asks for one, the alpha clamped to
    /// the footprint (or reset) after every step; then D6's grouped
    /// contributions (the alpha restored after each); then `K = over(B0, U, q,
    /// M)` with `B0 = backdrop_at(knockout, …)` and `mix(target, K,
    /// s·opacity)`. With `stop` (13A-4b-ii-b-2, `Some(m)` only from a staged
    /// ENTER / RESUME to a member cursor) the unit is computed at EVERY
    /// sample — outside the rect or where `s` is 0 the members composite
    /// over the transparent unit, clamped to the footprint — through members
    /// `[..m]`, left in `target[ti]`, and the body returns before the grouped
    /// contributions and `K`;
    /// without it the early returns come first, as ever.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn unit_pass_sample(
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
        let coverage = match pixel {
            Some(_) => self
                .mask_byte(canvas_xy)
                .map_or(1.0, |m| f32::from(m) / 255.0),
            None => 0.0,
        };
        let s = pixel.map_or(0.0, |px| f32::from(px[3]) / 255.0 * coverage);
        if stop.is_none() && (pixel.is_none() || s <= 0.0) {
            // Outside the layer rect the mix loop never reaches the sample;
            // where the shape is 0 it mixes nothing.
            return;
        }
        // D6: with the grouped interiors inside the unit the pixels enter at
        // fill 1 and the fill is paid once by the whole unit.
        let (fill_in, fill_out) = if style.grouped {
            (1.0, style.fill)
        } else {
            (style.fill, 1.0)
        };
        let mut unit = [[0.0f32; 4]];
        // `composite_pixels_into`'s early return, then its sample.
        if let Some(px) = pixel.filter(|_| s > 0.0 && fill_in > 0.0) {
            let pass = PixelPass {
                joint: crate::opacity_byte::joint(fill_in, 1.0, false),
                blend_if: style.blend_if.as_ref(),
                kind: blend_kind(BlendMode::Normal),
                direct: true,
                opacity: 1.0,
                fill_blend: fill_blend(BlendMode::Normal, fill_in, false),
                transfer,
            };
            composite_pixels_sample(&mut unit, 0, &px, 0, coverage, under, &pass, canvas_xy);
        }
        let a = unit[0][3];
        if let Some(steps) = steps.filter(|steps| !steps.is_empty()) {
            let limit = self.footprint_limit(pixel, canvas_xy);
            // `clip_unit`'s Open against the footprint (#2), then D4: the
            // members punch to the unit as it stands RIGHT NOW — after the
            // base's pixel pass, opened, before any member.
            let footprint = limit.unwrap_or(a);
            unit[0][3] = clip_alpha(unit[0][3], footprint, ClipStep::Open);
            let base_only = [unit[0]];
            let backdrop =
                base_only_backdrop(steps.iter().any(ResolvedStep::wants_base), &base_only);
            let end = stop.map_or(steps.len(), |m| m.min(steps.len()));
            for step in &steps[..end] {
                if let ResolvedStep::Member(member) = step {
                    let under = unit[0];
                    let pixel = member.pixel(transfer, canvas_xy);
                    member.composite(
                        &mut unit,
                        0,
                        under,
                        crate::opacity_byte::quantized(member.layer().opacity),
                        member.blend,
                        pixel,
                        backdrop,
                        transfer,
                        canvas_xy,
                    );
                }
                // `clip_unit`'s Settle — run for a clamp-only step too.
                unit[0][3] = clip_alpha(unit[0][3], footprint, ClipStep::Settle);
            }
            // A stop is a member cursor: the unit as it stands there, still
            // open, before D6's grouped contributions, which follow every
            // member (P7-39).
            if stop.is_some() {
                target[ti] = unit[0];
                return;
            }
            unit[0][3] = clip_alpha(unit[0][3], footprint, ClipStep::Close);
        }
        if stop.is_some() {
            target[ti] = unit[0];
            return;
        }
        if style.grouped {
            if let Some(planes) = self.planes() {
                let (_, interior_inks) = style.inks.split_at(planes.below.len());
                grouped_interior_sample(
                    &mut unit,
                    0,
                    &planes.shape,
                    &planes.interior,
                    interior_inks,
                    InkDecode::byte(transfer),
                    self.layer().offset,
                    canvas_xy,
                );
            }
        }
        let u = unit[0];
        let q = (fill_out * (u[3] / s).min(1.0)).clamp(0.0, 1.0);
        // A `q` of 0 is the HOLE: `K` stays `B0`.
        let mut k = [backdrop_at(knockout, style.knockout, target[ti], 0)];
        if q > 0.0 {
            composite_source_into(
                &mut k,
                0,
                [u[0], u[1], u[2]],
                q,
                blend_kind(self.blend),
                canvas_xy,
            );
        }
        target[ti] = mix_premultiplied(target[ti], k[0], s * opacity);
    }
}

/// D6's grouped contributions at one sample of the unit: each of the four
/// grouped interior contributions (`is_grouped_interior`) whose plane pixel
/// lands on `canvas_xy`, through `contributions_sample` at scale 1 — the
/// reference's `composite_contribution(.., 1.0, ..)` — with the unit's alpha
/// restored after each (they recolour, never add coverage). `inks` are the
/// Interior contributions' [`Ink`]s, index for index; `decode` the scene's.
#[allow(clippy::too_many_arguments)]
pub(crate) fn grouped_interior_sample(
    unit: &mut [[f32; 4]],
    ti: usize,
    shape: &Shape,
    interior: &[PlaneContribution],
    inks: &[Ink],
    decode: InkDecode<'_>,
    offset: (i32, i32),
    canvas_xy: (i64, i64),
) {
    let a = unit[ti][3];
    for (c, ink) in interior
        .iter()
        .zip(inks)
        .filter(|(c, _)| is_grouped_interior(c.kind))
    {
        // A grouped interior: `occluding_fill` is read for a Below stroke
        // alone.
        contributions_sample(
            unit,
            ti,
            shape,
            std::slice::from_ref(c),
            std::slice::from_ref(ink),
            decode,
            offset,
            1.0,
            0.0,
            canvas_xy,
        );
        unit[ti][3] = a;
    }
}

/// One sample of the typed `composite_interior_unit_into` onto `target[ti]`:
/// the byte unit pass's statements over the typed pixel twin — `pixel` the
/// layer's (or its group buffer's) sample and 16-bit alpha code, `None`
/// outside a raster's rect; `coverage` its prepared mask's value; `s =
/// alpha·coverage` in f32; the pixel pass at the unit's fill through
/// `composite_pixels_sample16` with the layer's Blend If reading `under`;
/// each prepared member through `sample_layers` with the base-only sample as
/// BOTH its backdrops when `members_want_base_snapshot16` asks, clamped to
/// `limit` (the footprint: the planes' shape or the pad-0 `typed_shape`'s
/// byte, `None` past `MAX_PIXELS`, where the alpha is reset), then D6's
/// grouped contributions; then `B0 =
/// backdrop_at(env.knockout, …)`, `K` and the mix. With `stop`
/// (13A-4b-ii-b-2) the byte pass's rule: the unit at every sample through
/// members `[..m]`, left in `target[ti]`, returning before the grouped
/// contributions and `K`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn unit_pass_sample16(
    target: &mut [[f32; 4]],
    ti: usize,
    node: &Node<'_>,
    style: &PreparedStyle,
    opacity: f32,
    pixel: Option<([f32; 4], f32)>,
    coverage: f32,
    under: [f32; 4],
    limit: Option<f32>,
    env: &StyleEnv<'_>,
    xy: (i64, i64),
    stop: Option<usize>,
) -> Result<(), String> {
    let layer = node.layer;
    let s = pixel.map_or(0.0, |(px, _)| px[3] * coverage);
    if stop.is_none() && (pixel.is_none() || s <= 0.0) {
        // Outside the layer rect the mix loop never reaches the sample;
        // where the shape is 0 it mixes nothing.
        return Ok(());
    }
    let (fill_in, fill_out) = if style.grouped {
        (1.0, style.fill)
    } else {
        (style.fill, 1.0)
    };
    let mut unit = [[0.0f32; 4]];
    if let Some((px, _)) = pixel.filter(|_| s > 0.0 && fill_in > 0.0) {
        let pass = PixelPass16 {
            joint: crate::opacity_byte::joint(fill_in, 1.0, env.float),
            blend_if: layer.blend_if.as_ref(),
            kind: blend_kind(BlendMode::Normal),
            direct: true,
            opacity: 1.0,
            float: env.float,
            fill_blend: fill_blend(BlendMode::Normal, fill_in, env.float),
            transfer: env.transfer,
            // A raster's pixel is its stored sample; a group's is composited.
            stored: matches!(layer.kind, Kind::Raster { .. }),
        };
        composite_pixels_sample16(&mut unit, 0, px, coverage, under, &pass, xy)?;
    }
    let a = unit[0][3];
    if !node.members.is_empty() {
        // `clip_unit`'s Open against the footprint (#2), then D4: the
        // members punch to the unit as it stands after the base's pixel
        // pass, opened, before any member.
        let footprint = limit.unwrap_or(a);
        unit[0][3] = clip_alpha(unit[0][3], footprint, ClipStep::Open);
        let base_only = [unit[0]];
        let member_env = StyleEnv {
            knockout: base_only_backdrop(
                members_want_base_snapshot16(&layer.clipped_members),
                &base_only,
            ),
            ..*env
        };
        let end = stop.map_or(node.members.len(), |m| m.min(node.members.len()));
        for member in &node.members[..end] {
            sample_layers(std::slice::from_ref(member), &mut unit, 0, &member_env, xy)?;
            unit[0][3] = clip_alpha(unit[0][3], footprint, ClipStep::Settle);
        }
        // A stop is a member cursor (still open), before the grouped
        // contributions (P7-39).
        if stop.is_some() {
            target[ti] = unit[0];
            return Ok(());
        }
        unit[0][3] = clip_alpha(unit[0][3], footprint, ClipStep::Close);
    }
    if stop.is_some() {
        target[ti] = unit[0];
        return Ok(());
    }
    if style.grouped {
        if let Some(planes) = &style.planes {
            let offset = match &layer.kind {
                Kind::Raster { offset, .. } => *offset,
                _ => (0, 0),
            };
            let (_, interior_inks) = style.inks.split_at(planes.below.len());
            grouped_interior_sample(
                &mut unit,
                0,
                &planes.shape,
                &planes.interior,
                interior_inks,
                env.style_colors().decode(),
                offset,
                xy,
            );
        }
    }
    let u = unit[0];
    let q = (fill_out * (u[3] / s).min(1.0)).clamp(0.0, 1.0);
    let mut k = [backdrop_at(env.knockout, style.knockout, target[ti], 0)];
    if q > 0.0 {
        composite_source_into(
            &mut k,
            0,
            [u[0], u[1], u[2]],
            q,
            env.kind(layer.blend_mode),
            xy,
        );
    }
    target[ti] = mix_premultiplied_in(target[ti], k[0], s * opacity, !env.float);
    Ok(())
}

/// D4's clipped-member backdrop, the ONE construction all six member loops
/// share (`style_knockout::member_env`'s rule): the base-only sample as BOTH
/// planes when some member asks for it (`wants`), else `NONE`.
pub(crate) fn base_only_backdrop(wants: bool, base_only: &[[f32; 4]; 1]) -> KnockoutBackdrop<'_> {
    if wants {
        KnockoutBackdrop {
            shallow: Some(base_only),
            deep: Some(base_only),
        }
    } else {
        KnockoutBackdrop::NONE
    }
}

/// `env` inside an ISOLATED group's buffer: D4's hard stop, both backdrops
/// transparent.
pub(crate) fn isolated_env<'e>(env: &StyleEnv<'e>) -> StyleEnv<'e> {
    StyleEnv {
        knockout: KnockoutBackdrop::NONE,
        ..*env
    }
}

impl ResolvedStep<'_> {
    /// `members_want_base_snapshot`'s question for one step: a contributing
    /// member whose style carries a knockout.
    pub(crate) fn wants_base(&self) -> bool {
        matches!(self, ResolvedStep::Member(member)
            if member.style.as_ref().is_some_and(|s| s.knockout != Knockout::None))
    }
}

impl ResolvedNode<'_> {
    /// Whether this node, as a unit base, clamps its members to a SHAPE
    /// (`shaped`'s rule): planes, or a style on a (synthetic) layer within
    /// `MAX_PIXELS`.
    pub(crate) fn is_shaped(&self) -> bool {
        self.planes().is_some()
            || self.style.is_some()
                && match &self.source {
                    Source::Raster | Source::Materialized(_) => {
                        let (lw, lh) = self.layer().pixels.dimensions();
                        u64::from(lw) * u64::from(lh) <= MAX_PIXELS
                    }
                    Source::Isolated(rect, _) | Source::Staged(rect, _) => {
                        ((rect.2 - rect.0) as u64).saturating_mul((rect.3 - rect.1) as u64)
                            <= MAX_PIXELS
                    }
                    _ => false,
                }
    }

    /// The footprint a unit base clamps its members to at `canvas_xy`: the
    /// planes' `Shape::at`, else — `shaped` — the pad-0 shape's byte, else
    /// `None` (the alpha is reset instead). Over `/255`.
    pub(crate) fn footprint_limit(
        &self,
        pixel: Option<[u8; 4]>,
        canvas_xy: (i64, i64),
    ) -> Option<f32> {
        match self.planes() {
            Some(planes) => {
                let shape = &planes.shape;
                Some(
                    f32::from(shape.at(
                        canvas_xy.0 - i64::from(self.layer().offset.0) - shape.origin.0,
                        canvas_xy.1 - i64::from(self.layer().offset.1) - shape.origin.1,
                    )) / 255.0,
                )
            }
            None => self
                .shaped
                .then(|| f32::from(self.shape_byte(pixel, canvas_xy)) / 255.0),
        }
    }
}
