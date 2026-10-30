//! The 8-bit regional compositor's PER-SAMPLE bodies (moved out of
//! `doc_regional_tree` in 13A-4b-i, verbatim): one sample of
//! `doc_group::composite_level_into` over a resolved level
//! (`ResolvedLevel::sample`, D5's Background split at the root), the staged
//! drivers' cursor range (`ResolvedLevel::sample_between`, 13A-4b-ii-a:
//! through pass-through children, their held samples; 13A-4b-ii-b-1: ending
//! in a clip unit's member step, ENTERED and RESUMED through
//! `ResolvedUnit::sample_clip_until`) and one sample of `composite_unit_into`
//! (`ResolvedUnit::sample`). The cursor block (`path_of_layer` …
//! `snapshot_at`, `clip_unit_at`) moved here from `doc_regional_tree` in
//! 13A-4b-ii-b-1. Index arithmetic only
//! around the reference's per-pixel bodies — `blend::composite_source_into`,
//! `blend::gate_source_into` and the node statements in `doc_regional_tree` /
//! `doc_regional_style`. The contract is `regional`'s module doc.

use crate::adjust::Adjustment;
use crate::blend::{blend_kind, composite_source_into, gate_source_into, BlendMode};
use crate::clip_unit::{clip_alpha, ClipStep};
use crate::doc_evaluation::ByteTransfer;
use crate::doc_regional_tree::{ResolvedLevel, ResolvedNode, ResolvedStep, ResolvedUnit, Source};
use crate::regional_guide::{Holds, PreparedGuide};
use crate::regional_knockout::base_only_backdrop;
use crate::style_knockout::KnockoutBackdrop;

impl<'r> ResolvedLevel<'r> {
    /// Unit positions from this level down through `Source::PassThrough`
    /// children to the base whose layer index is `layer` (13A-4b-ii-a), or
    /// to a clip unit's MEMBER step `[.., u, m]` whose node is `layer`
    /// (13A-4b-ii-b-1: a member cursor is always terminal).
    pub(crate) fn path_of_layer(&self, layer: usize) -> Option<Vec<usize>> {
        for (u, unit) in self.units.iter().enumerate() {
            if self.layer_units[u] == layer {
                return Some(vec![u]);
            }
            if let Source::PassThrough(child) = &unit.base.source {
                if let Some(mut path) = child.path_of_layer(layer) {
                    path.insert(0, u);
                    return Some(path);
                }
            }
            for (m, step) in unit.steps.iter().enumerate() {
                if matches!(step, ResolvedStep::Member(node) if node.index == layer) {
                    return Some(vec![u, m]);
                }
            }
        }
        None
    }

    /// `ResolvedUnit::guided` units of this level and, recursively, of every
    /// `Source::PassThrough` base's child level, as paths, unit order — the
    /// twin of the typed `guided_paths` and `stage_node`'s guard — and
    /// (13A-4b-ii-b-1) `[u, m]` for every `ResolvedUnit::guided_member`
    /// step, after the base's own subtree paths.
    pub(crate) fn guided_paths(&self) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        for (u, unit) in self.units.iter().enumerate() {
            if unit.guided() {
                out.push(vec![u]);
            }
            if let Source::PassThrough(child) = &unit.base.source {
                out.extend(child.guided_paths().into_iter().map(|mut path| {
                    path.insert(0, u);
                    path
                }));
            }
            out.extend(
                (0..unit.steps.len())
                    .filter(|&m| unit.guided_member(m))
                    .map(|m| vec![u, m]),
            );
        }
        out
    }

    /// The unit base at `path` (unit positions through pass-through bases),
    /// or the member node of a clip unit's step `[.., u, m]` (13A-4b-ii-b-1).
    fn node_at(&self, path: &[usize]) -> Option<&ResolvedNode<'r>> {
        let (&u, rest) = path.split_first()?;
        let unit = self.units.get(u)?;
        match (&unit.base.source, rest) {
            (_, []) => Some(&unit.base),
            (Source::PassThrough(child), _) => child.node_at(rest),
            (_, [m]) => match unit.steps.get(*m)? {
                ResolvedStep::Member(node) => Some(node),
                ResolvedStep::ClampOnly => None,
            },
            _ => None,
        }
    }

    fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut ResolvedNode<'r>> {
        let (&u, rest) = path.split_first()?;
        let unit = self.units.get_mut(u)?;
        if rest.is_empty() {
            return Some(&mut unit.base);
        }
        match (&mut unit.base.source, rest) {
            (Source::PassThrough(child), _) => child.node_at_mut(rest),
            (_, [m]) => match unit.steps.get_mut(*m)? {
                ResolvedStep::Member(node) => Some(node),
                ResolvedStep::ClampOnly => None,
            },
            _ => None,
        }
    }

    /// Whether the unit at `path` (unit positions through pass-through
    /// bases) is a CLIP unit — a non-pass-through base over a positional
    /// run — with a non-empty resolved run (13A-4b-ii-b-1).
    pub(crate) fn clip_unit_at(&self, path: &[usize]) -> bool {
        let Some((&u, rest)) = path.split_first() else {
            return false;
        };
        let Some(unit) = self.units.get(u) else {
            return false;
        };
        match (&unit.base.source, rest.is_empty()) {
            (Source::PassThrough(_), true) => false,
            (_, true) => unit.clip_run && !unit.steps.is_empty(),
            (Source::PassThrough(child), false) => child.clip_unit_at(rest),
            _ => false,
        }
    }

    /// Hands the unit base at `path` its staged guide.
    pub(crate) fn set_guide(&mut self, path: &[usize], guide: PreparedGuide) {
        if let Some(node) = self.node_at_mut(path) {
            node.guide = Some(guide);
        }
    }

    /// The adjustment of the unit base at `path`, if it is one.
    pub(crate) fn unit_adjustment(&self, path: &[usize]) -> Option<&Adjustment> {
        match &self.node_at(path)?.source {
            Source::Adjustment(adjustment) => Some(adjustment),
            _ => None,
        }
    }

    /// Whether the unit base at `path` is a pass-through node whose sample
    /// as its children began is held (`ResolvedNode::snapshot`).
    pub(crate) fn snapshot_at(&self, path: &[usize]) -> bool {
        self.node_at(path).is_some_and(|node| node.snapshot)
    }
}

impl ResolvedLevel<'_> {
    /// One sample of `composite_level_into` onto `acc[ai]` at `canvas_xy`:
    /// every unit, bottom-first, against `knockout`. At the ROOT with D5's
    /// Background (`composite_projection_into`): the first unit — entry 0 —
    /// with no backdrop, the sample right after it snapshotted, and every
    /// later unit against it as BOTH planes (Shallow at the root is Deep).
    pub(crate) fn sample(
        &self,
        acc: &mut [[f32; 4]],
        ai: usize,
        transfer: Option<&ByteTransfer>,
        canvas_xy: (i64, i64),
        knockout: KnockoutBackdrop<'_>,
    ) {
        if let (true, Some((first, rest))) = (self.background, self.units.split_first()) {
            first.sample(acc, ai, transfer, canvas_xy, KnockoutBackdrop::NONE);
            let snap = [acc[ai]];
            let backdrop = KnockoutBackdrop {
                shallow: Some(&snap),
                deep: Some(&snap),
            };
            for unit in rest {
                unit.sample(acc, ai, transfer, canvas_xy, backdrop);
            }
            return;
        }
        for unit in &self.units {
            unit.sample(acc, ai, transfer, canvas_xy, knockout);
        }
    }

    /// Units from cursor `from` (inclusive; `&[]` = the start) to cursor `to`
    /// (exclusive; `&[]` = the end), one sample, against `knockout` — no D5
    /// logic: the staged driver splits the Background itself (13A-4b-ii-a,
    /// replacing 13A-4a's unit range). A cursor is unit positions down
    /// through pass-through children. The unit `u = from[0]` with
    /// `from.len() > 1` is RESUMED: its pass-through child level from
    /// `from[1..]` to (`to[0] == u` ? `to[1..]` : `&[]`), Shallow
    /// `holds.at(prefix+[u], xy)` (None when not held), Deep `knockout.deep`;
    /// unless `u == to[0]` it is then FINISHED (gated: `a = S;
    /// gate_source_into(&mut a, acc[ai], coverage·opacity); acc[ai] = a`;
    /// plain: nothing — `composite_gated_pass`'s lerp back onto the sample as
    /// the children began, which the plane held; the children composited
    /// onto the working sample stand in for its copy). The unit `u = to[0]`
    /// with `to.len() > 1` is ENTERED: `holds.write(prefix+[u], canvas_xy,
    /// acc[ai])`, then its child level from the start to `to[1..]` with
    /// Shallow the sample as it stood (`ResolvedUnit::sample`'s `before`) and
    /// Deep `knockout.deep`. Every other unit in `[from[0], to[0])` is
    /// `ResolvedUnit::sample`. `prefix` is the path to this level (pushed and
    /// popped on descent).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sample_between(
        &self,
        from: &[usize],
        to: &[usize],
        prefix: &mut Vec<usize>,
        acc: &mut [[f32; 4]],
        ai: usize,
        transfer: Option<&ByteTransfer>,
        canvas_xy: (i64, i64),
        knockout: KnockoutBackdrop<'_>,
        holds: &mut Holds,
    ) {
        let resumed = (from.len() > 1).then(|| from[0]);
        let entered = (to.len() > 1).then(|| to[0]);
        if let Some(u) = resumed {
            if let Some(unit) = self.units.get(u) {
                if let Source::PassThrough(level) = &unit.base.source {
                    prefix.push(u);
                    let held = holds.at(prefix, canvas_xy).map(|s| [s]);
                    let children = KnockoutBackdrop {
                        shallow: held.as_ref().map(|s| &s[..]),
                        deep: knockout.deep,
                    };
                    let inner = if entered == Some(u) { &to[1..] } else { &[] };
                    level.sample_between(
                        &from[1..],
                        inner,
                        prefix,
                        acc,
                        ai,
                        transfer,
                        canvas_xy,
                        children,
                        holds,
                    );
                    prefix.pop();
                    let gated = !(unit.opacity >= 1.0 && unit.base.mask.is_none());
                    // A gated group read back here is always held (the
                    // hold schedule enters every gated prefix).
                    debug_assert!(!gated || held.is_some() || entered == Some(u));
                    if let (true, true, Some([s])) = (entered != Some(u), gated, held) {
                        let coverage = unit
                            .base
                            .mask_byte(canvas_xy)
                            .map_or(1.0, |m| f32::from(m) / 255.0);
                        let mut a = s;
                        gate_source_into(&mut a, acc[ai], coverage * unit.opacity);
                        acc[ai] = a;
                    }
                } else if self.clip_unit_at(&[u]) {
                    // A RESUMED clip unit (13A-4b-ii-b-1): RE-RUN from the
                    // sample as it began, every guided member's guide set,
                    // to the next member cursor in it or to its FINISH.
                    prefix.push(u);
                    let began = holds.at(prefix, canvas_xy);
                    prefix.pop();
                    debug_assert!(began.is_some(), "a resumed clip unit is held");
                    let Some(began) = began else {
                        return;
                    };
                    let stop = (to.len() > 1 && to[0] == u).then(|| to[1]);
                    unit.sample_clip_until(acc, ai, began, stop, knockout, transfer, canvas_xy);
                }
            }
        }
        let start = resumed.map_or(from.first().copied().unwrap_or(0), |u| u + 1);
        let end = to
            .first()
            .copied()
            .unwrap_or(self.units.len())
            .min(self.units.len());
        for unit in &self.units[start.min(end)..end] {
            unit.sample(acc, ai, transfer, canvas_xy, knockout);
        }
        if let Some(u) = entered.filter(|&u| resumed != Some(u)) {
            if let Some(unit) = self.units.get(u) {
                if let Source::PassThrough(level) = &unit.base.source {
                    prefix.push(u);
                    holds.write(prefix, canvas_xy, acc[ai]);
                    let before = [acc[ai]];
                    let children = KnockoutBackdrop {
                        shallow: Some(&before),
                        deep: knockout.deep,
                    };
                    level.sample_between(
                        &[],
                        &to[1..],
                        prefix,
                        acc,
                        ai,
                        transfer,
                        canvas_xy,
                        children,
                        holds,
                    );
                    prefix.pop();
                } else if self.clip_unit_at(&[u]) {
                    // An ENTERED clip unit (13A-4b-ii-b-1): the sample as it
                    // began held, then its unit buffer at the member cursor.
                    prefix.push(u);
                    holds.write(prefix, canvas_xy, acc[ai]);
                    prefix.pop();
                    let began = acc[ai];
                    unit.sample_clip_until(
                        acc,
                        ai,
                        began,
                        Some(to[1]),
                        knockout,
                        transfer,
                        canvas_xy,
                    );
                }
            }
        }
    }
}

impl ResolvedUnit<'_> {
    /// Whether step `step` is a GUIDED clip member (13A-4b-ii-b-1): an
    /// adjustment with a guide radius — over any base the walk admits, a
    /// styled one included since 13A-4b-ii-b-2 (a packaged base refuses) —
    /// the walk's staged predicate for a member at an admitted level.
    pub(crate) fn guided_member(&self, step: usize) -> bool {
        matches!(self.steps.get(step), Some(ResolvedStep::Member(node))
            if matches!(&node.source, Source::Adjustment(a) if a.guide_radius().is_some()))
    }

    /// One sample of `composite_unit_into`. The FIRST branch is the base's
    /// contribution, as the reference tests `!is_isolated` before anything
    /// else; `clip_run` is inspected only for a raster, adjustment or
    /// isolated base.
    fn sample(
        &self,
        acc: &mut [[f32; 4]],
        ai: usize,
        transfer: Option<&ByteTransfer>,
        canvas_xy: (i64, i64),
        knockout: KnockoutBackdrop<'_>,
    ) {
        let base = &self.base;
        let opacity = self.opacity;
        if let Source::PassThrough(level) = &base.source {
            // PASS THROUGH: the children straight onto this sample, or — a
            // mask or a sub-1 opacity — onto a copy of it lerped back by
            // `coverage · opacity` (`composite_gated_pass`). D4: a Shallow
            // child punches to the sample as the children began (`before`,
            // which the gated copy also leaves in `acc`); Deep rides through.
            // Handed ALWAYS: only a visible Shallow entry of THIS level reads
            // the plane, which is exactly when the reference snapshots it
            // (`level_wants_shallow`) — the typed arm's rule.
            let before = [acc[ai]];
            let children = KnockoutBackdrop {
                shallow: Some(&before),
                deep: knockout.deep,
            };
            if opacity >= 1.0 && base.mask.is_none() {
                level.sample(acc, ai, transfer, canvas_xy, children);
            } else if opacity > 0.0 {
                let mut buf = [acc[ai]];
                level.sample(&mut buf, 0, transfer, canvas_xy, children);
                let coverage = base
                    .mask_byte(canvas_xy)
                    .map_or(1.0, |m| f32::from(m) / 255.0);
                let t = coverage * opacity;
                gate_source_into(&mut acc[ai], buf[0], t);
            }
            return;
        }
        let under = acc[ai];
        let styled = base.planes().is_some() || base.routed();
        if self.clip_run {
            // `composite_clip_group_into`'s private-buffer path, or a styled
            // base's member loop ([`Self::sample_clip_until`], 13A-4b-ii-b-2);
            // nothing at opacity 0.
            if opacity > 0.0 {
                self.sample_clip_until(acc, ai, under, None, knockout, transfer, canvas_xy);
            }
            return;
        }
        let pixel = base.pixel(transfer, canvas_xy);
        if styled {
            // `composite_styled_into` with planes, or ROUTED to the unit
            // pass (13A-3c): the Below, pixel (or unit) and Interior steps
            // at this sample.
            base.styled_sample(
                acc, ai, under, opacity, pixel, None, knockout, transfer, canvas_xy, None,
            );
            return;
        }
        // `composite_clip_group_into` with an EMPTY group is
        // `composite_layer_into` on the base (a styled one's Blend If
        // reads the accumulator itself).
        base.composite(
            acc, ai, under, opacity, base.blend, pixel, knockout, transfer, canvas_xy,
        );
    }

    /// `composite_clip_group_into`'s private-buffer path, one sample
    /// (extracted from [`Self::sample`] in 13A-4b-ii-b-1): the base at full
    /// opacity in Normal onto a transparent buffer against `under` (its mask
    /// applies; a styled base's Blend If reads `under`, the accumulator as it
    /// stood before the unit), its footprint recorded, each step of
    /// `steps[..stop]` (all without `stop`) composited onto the base with its
    /// alpha frozen (`clip_unit`'s Open / Settle / Close against the
    /// footprint: a styled base's shape, else its alpha). With `stop` the unit
    /// buffer at that member's cursor is left in `acc[ai]`; without it
    /// `acc[ai]` is reset to `under` and the buffer composited with the
    /// base's mode and opacity (`composite_buffer_into`'s statement).
    /// Ordinary sampling passes `under == acc[ai]`, so the reset is a no-op.
    /// A base with planes or a routed style (13A-4b-ii-b-2) is
    /// `composite_styled_into`'s: `acc[ai] = under`, then `styled_sample`
    /// over the run with `stop` (its direct or routed member loop); a
    /// fill-only base keeps this body, already the styled-direct loop
    /// without planes.
    #[allow(clippy::too_many_arguments)]
    fn sample_clip_until(
        &self,
        acc: &mut [[f32; 4]],
        ai: usize,
        under: [f32; 4],
        stop: Option<usize>,
        knockout: KnockoutBackdrop<'_>,
        transfer: Option<&ByteTransfer>,
        canvas_xy: (i64, i64),
    ) {
        let base = &self.base;
        let opacity = self.opacity;
        let pixel = base.pixel(transfer, canvas_xy);
        if base.planes().is_some() || base.routed() {
            acc[ai] = under;
            base.styled_sample(
                acc,
                ai,
                under,
                opacity,
                pixel,
                Some(&self.steps),
                knockout,
                transfer,
                canvas_xy,
                stop,
            );
            return;
        }
        let mut buf = [[0.0f32; 4]];
        base.composite(
            &mut buf,
            0,
            under,
            1.0,
            BlendMode::Normal,
            pixel,
            knockout,
            transfer,
            canvas_xy,
        );
        let a = buf[0][3];
        let limit = base
            .shaped
            .then(|| f32::from(base.shape_byte(pixel, canvas_xy)) / 255.0);
        // `clip_unit`'s footprint: the styled base's shape, else the base's
        // own alpha (#578: the members land with the base's alpha frozen).
        // Only a run with a member step opens the unit (`style_composite`'s
        // `any(visible)`); a stop leaves it OPEN, the buffer a member at that
        // cursor composites onto in the reference.
        let footprint = limit.unwrap_or(a);
        let opened = !self.steps.is_empty();
        if opened {
            buf[0][3] = clip_alpha(buf[0][3], footprint, ClipStep::Open);
        }
        // D4's clipped-member rule (`member_env`): a member with a knockout
        // punches to the buffer as it stands after the base alone rendered —
        // `members_want_base_snapshot`'s question over the resolved run (a
        // placeholder or hidden member never counts; a clamp-only one never
        // reads it).
        let base_only = [buf[0]];
        let members =
            base_only_backdrop(self.steps.iter().any(ResolvedStep::wants_base), &base_only);
        let end = stop.map_or(self.steps.len(), |m| m.min(self.steps.len()));
        for step in &self.steps[..end] {
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
            // The post-member alpha statement, run for a clamp-only step
            // too: `clip_unit`'s Settle.
            buf[0][3] = clip_alpha(buf[0][3], footprint, ClipStep::Settle);
        }
        if stop.is_some() {
            acc[ai] = buf[0];
            return;
        }
        if opened {
            buf[0][3] = clip_alpha(buf[0][3], footprint, ClipStep::Close);
        }
        acc[ai] = under;
        let src = buf[0];
        let sa = src[3] * opacity;
        if sa > 0.0 {
            composite_source_into(
                acc,
                ai,
                [src[0], src[1], src[2]],
                sa,
                blend_kind(base.blend),
                canvas_xy,
            );
        }
    }
}
