//! The 8-bit regional plan's BUDGET DRY RUN (moved verbatim out of
//! `doc_regional_style` in 13A-4b-ii-b-2): `materialization_charges` mirrors,
//! for one MATERIALIZED group, the reference's own charge and nesting points
//! (`doc_group::rendered_group`, `composite_level_into`, `composite_unit_into`,
//! `resolved_member`) against the render's group budget, allocating no
//! pixels, so plan and render refuse with the reference's sentences. Since
//! 13A-5c-3 the same walk also answers, beside the charged peak, the peak
//! BYTES the reference allocates inside the nest WITHOUT a budget charge —
//! each materialized node's synthetic, mask crop and planes, and the per-unit
//! buffers of `composite_clip_group_into` / `composite_styled_into` /
//! `composite_package_into` / `composite_interior_unit_into` and a masked
//! pass-through's owned mask crop (`unit_buffers`) — which the plan adds to
//! the nest's scratch; the `RenderBudget` charges are untouched. The
//! contract is `regional`'s module doc.

use crate::adjust::Adjustment;
use crate::doc::{Layer, LayerKind};
use crate::doc_group::{self, RenderBudget, MAX_GROUP_DEPTH};
use crate::doc_regional_tree::Rect;
use crate::doc_regional_walk::{renders_planes, style_of};
use crate::precision_style_render::style_scratch;
use crate::style::{Knockout, LayerStyle};
use crate::style_composite::{holds_inside_band, CompositeEnv, BAND_BACKDROP_BYTES};
use crate::style_knockout::{has_grouped_interior_effect, level_wants_shallow};

/// The reference's budget and nesting decisions for ONE materialized group,
/// allocating no pixels: `Ok(None)` when it renders nothing, else its rect
/// (extent ∩ `clip`), the PEAK pixels its nest holds at once and (13A-5c-3)
/// the peak BYTES the reference allocates inside the nest without a budget
/// charge. It mirrors, statement by statement and in their order, only the
/// charge and refusal points of `doc_group::rendered_group` (the nesting
/// limit at `depth`, the empty extent, `budget.charge` of the buffer held
/// while the children run at `depth + 1`), `composite_level_into` (its own
/// `depth > MAX_GROUP_DEPTH` check, the `visible` guard),
/// `composite_unit_into` (a pass-through group:
/// `style_knockout::level_wants_shallow`'s snapshot charge around the plain
/// pass, `composite_gated_pass`'s copy; an isolated group base: recurse, and
/// `None` skips the unit's members) and `resolved_member` (a visible group
/// member: recurse, sequentially). Guarded by `visible`, never by opacity —
/// the reference charges a visible zero-opacity isolated group — and, as the
/// reference's guards are, by `env.cut` (13A-5c-2b: `Cut::NONE` for every
/// regional render, the sampling plate's cut for `regional_cut_plate`).
/// Raising only the reference's own sentences, it refuses exactly when the
/// reference would inside this nest; the budget dry run is byte-identical to
/// 13A-5c-2b's.
pub(crate) fn materialization_charges(
    layers: &[Layer],
    at: doc_group::Unit,
    env: CompositeEnv<'_>,
    clip: Rect,
    depth: u16,
    budget: &RenderBudget,
) -> Result<Option<(Rect, u64, u64)>, String> {
    // `rendered_group`.
    let Some(entry) = layers.get(at.idx) else {
        return Ok(None);
    };
    if entry.kind != LayerKind::Group {
        return Ok(None);
    }
    if depth >= MAX_GROUP_DEPTH {
        return Err("rendering exceeds the group nesting limit".into());
    }
    let Some((ex0, ey0, ex1, ey1)) = doc_group::group_extent(layers, at, env) else {
        return Ok(None);
    };
    let rect = (
        ex0.max(clip.0),
        ey0.max(clip.1),
        ex1.min(clip.2),
        ey1.min(clip.3),
    );
    if rect.2 <= rect.0 || rect.3 <= rect.1 {
        return Ok(None);
    }
    let pixels = ((rect.2 - rect.0) as u64)
        .checked_mul((rect.3 - rect.1) as u64)
        .ok_or("rendering group dimensions overflow")?;
    let _charge = budget.charge(pixels)?;
    let children = doc_group::units(layers, at.start..at.idx, entry.depth.saturating_add(1));
    // A nest root is `KnockoutBackdrop::NONE` (`rendered_group`'s D4).
    let (inner, uncharged) = level_charges(layers, &children, env, rect, depth + 1, false, budget)?;
    Ok(Some((rect, pixels + inner, uncharged)))
}

/// `composite_level_into`'s charge points over `level` onto an accumulator
/// covering `acc`: the peak of its units (each releases before the next),
/// as `(charged pixels, uncharged bytes)`. `shallow`: this level composites
/// under a Shallow snapshot (it is the children of a pass-through unit whose
/// `level_wants_shallow` holds — plain or gated pass — or of a pass-through
/// inside such a level); `false` at a nest root.
fn level_charges(
    layers: &[Layer],
    level: &[doc_group::Unit],
    env: CompositeEnv<'_>,
    acc: Rect,
    depth: u16,
    shallow: bool,
    budget: &RenderBudget,
) -> Result<(u64, u64), String> {
    if depth > MAX_GROUP_DEPTH {
        return Err("rendering exceeds the group nesting limit".into());
    }
    let mut peak = 0u64;
    let mut uncharged = 0u64;
    let mut i = 0;
    while i < level.len() {
        let unit = level[i];
        let entry = &layers[unit.idx];
        if entry.clipped {
            if entry.visible && !env.cut.excludes(unit.idx) {
                let (p, u) =
                    unit_charges(layers, unit, &[], true, env, acc, depth, shallow, budget)?;
                peak = peak.max(p);
                uncharged = uncharged.max(u);
            }
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < level.len() && layers[level[end].idx].clipped {
            end += 1;
        }
        if entry.visible && !env.cut.excludes(unit.idx) {
            let (p, u) = unit_charges(
                layers,
                unit,
                &level[i + 1..end],
                false,
                env,
                acc,
                depth,
                shallow,
                budget,
            )?;
            peak = peak.max(p);
            uncharged = uncharged.max(u);
        }
        i = end;
    }
    Ok((peak, uncharged))
}

/// `composite_unit_into`'s charge points for one unit (and
/// `resolved_member`'s for its members): the peak pixels it holds at once,
/// and the peak uncharged bytes — every materialized node's `synth` (alive
/// through the unit) plus the larger of its nests' own uncharged peaks and
/// the unit's `unit_buffers`; a pass-through unit's owned mask crop beside
/// its children's figure.
#[allow(clippy::too_many_arguments)]
fn unit_charges(
    layers: &[Layer],
    unit: doc_group::Unit,
    members: &[doc_group::Unit],
    baseless: bool,
    env: CompositeEnv<'_>,
    acc: Rect,
    depth: u16,
    shallow: bool,
    budget: &RenderBudget,
) -> Result<(u64, u64), String> {
    let entry = &layers[unit.idx];
    let visible_members = members.iter().any(|m| layers[m.idx].visible);
    let acc_pixels = ((acc.2 - acc.0) as u64).saturating_mul((acc.3 - acc.1) as u64);
    if entry.kind == LayerKind::Group && !doc_group::is_isolated(entry, baseless, visible_members) {
        let children =
            doc_group::units(layers, unit.start..unit.idx, entry.depth.saturating_add(1));
        let opacity = crate::opacity_byte::quantized(entry.opacity);
        let masked = entry.mask_enabled && entry.mask.is_some();
        // The owned mask crop `composite_unit_into` takes before the opacity
        // check (charged at `A` even where the reference borrows it).
        let crop = u64::from(masked) * acc_pixels;
        // Both the plain and the gated pass hand the children a Shallow
        // plane when the level wants one; otherwise the enclosing one rides.
        let child_shallow = shallow || level_wants_shallow(layers, &children);
        if opacity >= 1.0 && !masked {
            if level_wants_shallow(layers, &children) {
                let _charge = budget.charge(acc_pixels)?;
                let (inner, u) = level_charges(
                    layers,
                    &children,
                    env,
                    acc,
                    depth + 1,
                    child_shallow,
                    budget,
                )?;
                return Ok((acc_pixels + inner, u));
            }
            return level_charges(
                layers,
                &children,
                env,
                acc,
                depth + 1,
                child_shallow,
                budget,
            );
        }
        if opacity > 0.0 {
            // `composite_gated_pass`.
            let _charge = budget.charge(acc_pixels)?;
            let (inner, u) = level_charges(
                layers,
                &children,
                env,
                acc,
                depth + 1,
                child_shallow,
                budget,
            )?;
            return Ok((acc_pixels + inner, crop.saturating_add(u)));
        }
        return Ok((0, crop));
    }
    let mut peak = 0u64;
    let mut synth = 0u64;
    let mut nests = 0u64;
    let base_dims = if entry.kind == LayerKind::Group {
        match materialization_charges(layers, unit, env, acc, depth, budget)? {
            Some((rect, base, inner)) => {
                peak = base;
                synth = synth.saturating_add(synthetic_bytes(entry, rect)?);
                nests = inner;
                dims(rect)
            }
            None => return Ok((0, 0)),
        }
    } else {
        entry.pixels.dimensions()
    };
    // The members `composite_clip_group_into` composites: visible, not cut,
    // and — a group — rendered to something (`resolved_member`).
    let mut drawn: Vec<(&Layer, (u32, u32))> = Vec::new();
    for &member in members {
        let layer = &layers[member.idx];
        if !layer.visible || env.cut.excludes(member.idx) {
            continue;
        }
        if layer.kind == LayerKind::Group {
            if let Some((rect, p, inner)) =
                materialization_charges(layers, member, env, acc, depth, budget)?
            {
                peak = peak.max(p);
                synth = synth.saturating_add(synthetic_bytes(layer, rect)?);
                nests = nests.max(inner);
                drawn.push((layer, dims(rect)));
            }
        } else {
            drawn.push((layer, layer.pixels.dimensions()));
        }
    }
    let buffers = unit_buffers(entry, base_dims, members.is_empty(), &drawn, acc, shallow)?;
    Ok((peak, synth.saturating_add(nests.max(buffers))))
}

fn dims(rect: Rect) -> (u32, u32) {
    ((rect.2 - rect.0) as u32, (rect.3 - rect.1) as u32)
}

/// `synth(g)`: a materialized group's synthetic RGBA8, its cropped mask and —
/// only when the reference renders them — its planes over `rect`, all alive
/// through the unit that composites it.
fn synthetic_bytes(group: &Layer, rect: Rect) -> Result<u64, String> {
    let pixels = ((rect.2 - rect.0) as u64).saturating_mul((rect.3 - rect.1) as u64);
    let mask = u64::from(group.mask_enabled && group.mask.is_some()) * pixels;
    let planes = match style_of(group) {
        Some(style) if renders_planes(group, style, Some(rect)) => {
            style_scratch(style, dims(rect), false, None)?.0
        }
        _ => 0,
    };
    Ok(pixels
        .saturating_mul(4)
        .saturating_add(mask)
        .saturating_add(planes))
}

/// The per-unit buffers `composite_unit_into` → `composite_clip_group_into` /
/// `composite_styled_into` / `composite_package_into` /
/// `composite_interior_unit_into` allocate WITHOUT a budget charge, in bytes,
/// for one non-pass-through unit over accumulator `acc`: `base` (its
/// synthetic's dims for a group) and the `drawn` members (visible, not cut,
/// rendered) of a run that is `empty` or not — the reference branches on the
/// positional run, hidden members included. `shallow`: the level's Shallow
/// plane is present (`env.knockout` of the base). A conservative upper bound
/// where `regional`'s module doc says so; never under a buffer the reference
/// holds.
fn unit_buffers(
    base: &Layer,
    base_dims: (u32, u32),
    empty: bool,
    drawn: &[(&Layer, (u32, u32))],
    acc: Rect,
    shallow: bool,
) -> Result<u64, String> {
    let a = ((acc.2 - acc.0) as u64).saturating_mul((acc.3 - acc.1) as u64);
    let kplanes = u64::from(shallow);
    if empty {
        return lone(base, base_dims, acc, kplanes);
    }
    // `composite_clip_group_into` returns before allocating.
    if crate::opacity_byte::quantized(base.opacity) <= 0.0 {
        return Ok(0);
    }
    // `members_want_base_snapshot`: the base-only clone, and both member
    // planes `member_env` sets.
    let snapshot = drawn
        .iter()
        .any(|(m, _)| style_of(m).is_some_and(|s| s.knockout != Knockout::None));
    let member_planes = 2 * u64::from(snapshot);
    let mut total = a
        .saturating_mul(20)
        .saturating_add(guide(base, acc)?)
        .saturating_add(package(base, base_dims, acc, kplanes))
        .saturating_add(u64::from(snapshot) * 16 * a);
    if let Some(style) = style_of(base) {
        let l = area(base_dims);
        let blend_if = style.blend_if.is_some_and(|b| !b.is_identity());
        let planes = renders_planes(base, style, Some(rect_of(base_dims)));
        total = total
            .saturating_add(u64::from(blend_if) * 16 * l.min(a))
            .saturating_add(band_store(style, l.min(a)))
            .saturating_add(u64::from(!planes) * l);
    }
    let mut members = 0u64;
    for &(m, m_dims) in drawn {
        members = members.max(lone(m, m_dims, acc, member_planes)?);
    }
    Ok(total.saturating_add(members))
}

/// `lone(x)`: one layer composited alone onto an accumulator covering `acc`
/// (a lone unit, or one member over the unit buffer): its guide, its package
/// strips and their `kplanes` knockout windows, and — styled — the Blend If
/// snapshot and the routed unit pass's buffer, `base_alpha` and pad-0 shape.
/// `routed` stays at or above the reference's `routes_unit`: the grouped
/// half asks `has_grouped_interior_effect`, which is false only where the
/// rendered planes hold no grouped contribution. The typed walk stays
/// flag-only (`precision_regional_walk`'s `row`, `precision_render`'s
/// `collect`), a coarser upper bound.
fn lone(x: &Layer, x_dims: (u32, u32), acc: Rect, kplanes: u64) -> Result<u64, String> {
    let a = ((acc.2 - acc.0) as u64).saturating_mul((acc.3 - acc.1) as u64);
    let mut total = guide(x, acc)?.saturating_add(package(x, x_dims, acc, kplanes));
    if let Some(style) = style_of(x) {
        let l = area(x_dims);
        let planes = renders_planes(x, style, Some(rect_of(x_dims)));
        let blend_if = style.blend_if.is_some_and(|b| !b.is_identity());
        let routed = style.knockout != Knockout::None
            || (style.blend_interior_as_group && planes && has_grouped_interior_effect(style));
        total = total
            .saturating_add(u64::from(blend_if) * 16 * l.min(a))
            .saturating_add(band_store(style, l.min(a)));
        if routed {
            total = total
                .saturating_add(a.saturating_mul(20))
                .saturating_add(u64::from(!planes) * l);
        }
    }
    Ok(total)
}

/// The inside band's B0 store (`style_composite::BandBackdrop`) over at
/// most `pixels` band pixels, when `style` can emit an inside band.
fn band_store(style: &LayerStyle, pixels: u64) -> u64 {
    u64::from(holds_inside_band(style)).saturating_mul(BAND_BACKDROP_BYTES.saturating_mul(pixels))
}

/// `pkg(x)`: the package path's strip copy (`PACKAGE_STRIP_ROWS` = 256 rows
/// of at most the accumulator's width) and `style_knockout::windowed`'s
/// per-plane strip copies, charged at `S = min(A, W·256)`.
fn package(x: &Layer, x_dims: (u32, u32), acc: Rect, kplanes: u64) -> u64 {
    let Some(style) = style_of(x) else {
        return 0;
    };
    let packaged = renders_planes(x, style, Some(rect_of(x_dims)))
        && crate::opacity_byte::quantized(x.opacity) < 1.0
        && x.blend != crate::blend::BlendMode::Dissolve;
    if !packaged {
        return 0;
    }
    let a = ((acc.2 - acc.0) as u64).saturating_mul((acc.3 - acc.1) as u64);
    let s = a.min(((acc.2 - acc.0) as u64).saturating_mul(256));
    s.saturating_mul(16).saturating_mul(1 + kplanes)
}

/// `guide(x)`: a guide-reading adjustment's plane over the accumulator
/// (`adjust_precision::plan::guide`, as `regional_plan` sizes a stage's);
/// 0 for anything else.
fn guide(x: &Layer, acc: Rect) -> Result<u64, String> {
    if x.kind == LayerKind::Group {
        return Ok(0);
    }
    let Some(radius) = x
        .meta
        .as_deref()
        .and_then(Adjustment::from_meta)
        .and_then(|op| op.guide_radius())
    else {
        return Ok(0);
    };
    let (w, h) = dims(acc);
    Ok(crate::adjust_precision::plan::guide(w, h, radius)?.0)
}

fn area((w, h): (u32, u32)) -> u64 {
    u64::from(w) * u64::from(h)
}

fn rect_of((w, h): (u32, u32)) -> Rect {
    (0, 0, i64::from(w), i64::from(h))
}
