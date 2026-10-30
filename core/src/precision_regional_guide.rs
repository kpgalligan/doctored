//! The 16-bit regional compositor's STAGED window loop (Phase 13A-4a at the
//! root, 13A-4b-i in isolated groups), the typed twin of
//! `doc_regional_guide`: a level holding a lone guide-reading
//! Shadows/Highlights is evaluated in stages over the planes
//! `regional_guide::plan_levels` declares — stage 0 fills `P_0` over `R_0`
//! at divisor 1 (`zero_plane`) with the nodes below the first guided node;
//! each later stage builds that node's guide from the previous plane
//! (`PrecisionAdjustment::guide` — `render_layers`' own call, in its order:
//! the guide under `progress.job.range`, then its work advanced), hands it to
//! the node (`Node::guide`) and evaluates the nodes up to the next guided
//! node over the next plane — the last one over the OUTPUT at the request's
//! sampling at the root ([`Last::Output`]), or over the level's `target` at
//! divisor 1 in a nested level ([`Last::Region`]), whose plane is then HELD
//! in `Node::staged` and read in place of the group's children
//! ([`stage_nested`], before the root loop). D5's Background split runs in
//! the root's stage 0 (node 0 alone, its sample snapshotted into `S`, every
//! later node of EVERY stage against `S` as both planes). The k-th guided
//! node is the k-th path of `guided_paths` — an adjustment with a guide
//! radius and no members, at the level or (13A-4b-ii-a) directly inside its
//! PASS-THROUGH children, or (13A-4b-ii-b-1) a guided member of a clip base
//! (since 13A-4b-ii-b-2 a styled one too, `styled_sample16`'s member loop or
//! the package's copy), its path ending in the member's position, row order (the scene
//! is bottom-first over the same visible records the walk admitted, and the
//! walk absorbed the same pass-through levels); a count mismatch with the
//! level's stages, or a path that does not end at an adjustment, is
//! `Err("regional stage mismatch")`. A clip unit holding a guided member
//! holds the sample as it began from the fill that enters it; every later
//! fill re-runs it from there (`precision_regional_sample::sample_between`).
//! The stage plane holds the working sample at the cursor (the pass-through
//! arm's canvas-sized copy), and the sample as each pass-through node's
//! children began is HELD as a plane (`regional_guide::Holds`, a
//! `zero_plane`) from the fill that enters it to the fill that finishes it
//! (`gate_pass_through_sample16`). The nested levels are matched to the
//! plan's PRE-ORDER levels by popping them in the walk's order. Index
//! arithmetic only: every sample is
//! `precision_regional_tree::sample_between` over `sample_layers`.
//! The contract is `regional`'s module doc; the geometry and exactness
//! argument `regional_guide`'s.

use crate::precision_image::reserved_vec;
use crate::precision_regional_sample::{guided, guided_paths, node_at_mut, sample_between};
use crate::precision_regional_tree::Node;
use crate::precision_render::{zero_plane, Kind, Progress};
use crate::precision_style_render::StyleEnv;
use crate::regional::RegionalPlan;
use crate::regional_guide::{index_in, Holds, Last, PreparedGuide, StagedLevel, StagedPlane};
use crate::regional_knockout::isolated_env;
use crate::regional_plan::{PixelRegion, CHECKPOINT_PIXELS};
use crate::style_knockout::KnockoutBackdrop;

const MISMATCH: &str = "regional stage mismatch";

fn pixels(region: PixelRegion) -> usize {
    region.width as usize * region.height as usize
}

/// The canvas pixel of plane index `p` over `region`.
fn at(region: PixelRegion, p: usize) -> (i64, i64) {
    let w = region.width as usize;
    (
        i64::from(region.x) + (p % w) as i64,
        i64::from(region.y) + (p / w) as i64,
    )
}

/// The typed HOLD SCHEDULE before one fill from cursor `from` to cursor
/// `to` over `region` (13A-4b-ii-a): every proper prefix of `to` that is not
/// a proper prefix of `from` — a pass-through node this fill ENTERS; the
/// typed arm always copies, so every one is held — gets a plane over
/// `region`, filled by `zero_plane` (advancing the plan's hold work).
fn enter_holds(
    holds: &mut Holds,
    from: &[usize],
    to: &[usize],
    region: PixelRegion,
    progress: &mut Progress<'_>,
) -> Result<(), String> {
    for len in 1..to.len() {
        let p = &to[..len];
        if !(from.len() > len && from[..len] == *p) {
            holds.enter(p, region, zero_plane(pixels(region), progress)?);
        }
    }
    Ok(())
}

/// The staged render of `nodes` for `staged` (whose `stages` are non-empty):
/// at the root ([`Last::Output`]) the output accumulator, `plan.output`
/// samples at the request's sampling; nested ([`Last::Region`]) the held
/// final plane over the region at divisor 1. Since 13A-4b-ii-a each guided
/// node is reached by a CURSOR (`guided_paths`, through pass-through
/// children) and every fill runs `sample_between` from one cursor to the
/// next, the pass-through samples entered and not yet finished held in
/// [`Holds`] for the whole call.
pub(crate) fn render_staged(
    nodes: &mut [Node<'_>],
    background: bool,
    staged: &StagedLevel,
    last: Last,
    plan: &RegionalPlan,
    env: &StyleEnv<'_>,
    progress: &mut Progress<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let stages = &staged.stages;
    let paths = guided_paths(nodes);
    if paths.len() != stages.len() || stages.is_empty() || (background && paths[0][0] == 0) {
        return Err(MISMATCH.into());
    }
    let rows_0 = staged.rows_0 as usize;
    let mut holds = Holds(Vec::new());
    let mut prefix = Vec::new();
    // Stage 0: P_0 over R_0 at divisor 1, the nodes below the first guide.
    let r0 = stages[0].plane;
    let n0 = pixels(r0);
    let mut prev = zero_plane(n0, progress)?;
    let mut snapshot: Vec<[f32; 4]> = if background {
        reserved_vec(n0)?
    } else {
        Vec::new()
    };
    enter_holds(&mut holds, &[], &paths[0], r0, progress)?;
    for start in (0..n0).step_by(CHECKPOINT_PIXELS) {
        let end = (start + CHECKPOINT_PIXELS).min(n0);
        for p in start..end {
            let xy = at(r0, p);
            if background {
                let (from, to) = (&[][..], &[1][..]);
                sample_between(
                    nodes,
                    from,
                    to,
                    &mut prefix,
                    &mut prev,
                    p,
                    env,
                    xy,
                    &mut holds,
                )?;
                snapshot.push(prev[p]);
                let snap = [prev[p]];
                let seeded = StyleEnv {
                    knockout: KnockoutBackdrop {
                        shallow: Some(&snap),
                        deep: Some(&snap),
                    },
                    ..*env
                };
                let (from, to) = (&[1][..], paths[0].as_slice());
                sample_between(
                    nodes,
                    from,
                    to,
                    &mut prefix,
                    &mut prev,
                    p,
                    &seeded,
                    xy,
                    &mut holds,
                )?;
            } else {
                let to = paths[0].as_slice();
                sample_between(
                    nodes,
                    &[],
                    to,
                    &mut prefix,
                    &mut prev,
                    p,
                    env,
                    xy,
                    &mut holds,
                )?;
            }
        }
        progress.advance((end - start) * rows_0)?;
    }
    for (k, stage) in stages.iter().enumerate() {
        let from = paths[k].as_slice();
        let to = paths.get(k + 1).map_or(&[][..], Vec::as_slice);
        let built = stage.plane;
        // `render_layers`' own order: the guide under its progress range,
        // then its work advanced.
        let layer = node_at_mut(nodes, from).ok_or(MISMATCH)?.layer;
        let Kind::Adjustment(state) = &layer.kind else {
            return Err(MISMATCH.into());
        };
        let guide_work = state.kernel.guide_work_units(built.width, built.height)?;
        let start = progress.completed as f64 / progress.total as f64;
        let end = (progress.completed + guide_work) as f64 / progress.total as f64;
        let guide = state
            .kernel
            .guide(
                built.width,
                built.height,
                &|i| prev[i],
                progress.job.range(start, end),
            )?
            .ok_or(MISMATCH)?;
        progress.advance(guide_work as usize)?;
        node_at_mut(nodes, from).ok_or(MISMATCH)?.guide = Some(PreparedGuide::new(guide, built));
        let rows = stage.rows as usize;
        // P_k over R_k at divisor 1, a nested level's held R_m, or the
        // OUTPUT at the request's sampling.
        let (count, width, region, request) = match (stages.get(k + 1), &last) {
            (Some(next), _) => (pixels(next.plane), 0, Some(next.plane), None),
            (None, Last::Region(region)) => (pixels(*region), 0, Some(*region), None),
            (None, Last::Output(request)) => {
                let (ow, oh) = plan.output;
                (ow as usize * oh as usize, ow as usize, None, Some(*request))
            }
        };
        let mut out = match region {
            Some(_) => reserved_vec(count)?,
            None => zero_plane(count, progress)?,
        };
        // The OUTPUT's `to` is empty: nothing is entered there.
        if let Some(region) = region {
            enter_holds(&mut holds, from, to, region, progress)?;
        }
        let nodes: &[Node<'_>] = nodes;
        for start in (0..count).step_by(CHECKPOINT_PIXELS) {
            let end = (start + CHECKPOINT_PIXELS).min(count);
            for q in start..end {
                let xy = match (region, request) {
                    (Some(region), _) => at(region, q),
                    (None, Some(request)) => request.sample((q % width) as u32, (q / width) as u32),
                    (None, None) => return Err(MISMATCH.into()),
                };
                let copied = prev[index_in(built, xy)];
                match region {
                    Some(_) => out.push(copied),
                    None => out[q] = copied,
                }
                if background {
                    let snap = [snapshot[index_in(r0, xy)]];
                    let seeded = StyleEnv {
                        knockout: KnockoutBackdrop {
                            shallow: Some(&snap),
                            deep: Some(&snap),
                        },
                        ..*env
                    };
                    sample_between(
                        nodes,
                        from,
                        to,
                        &mut prefix,
                        &mut out,
                        q,
                        &seeded,
                        xy,
                        &mut holds,
                    )?;
                } else {
                    sample_between(
                        nodes,
                        from,
                        to,
                        &mut prefix,
                        &mut out,
                        q,
                        env,
                        xy,
                        &mut holds,
                    )?;
                }
            }
            progress.advance((end - start) * rows)?;
        }
        prev = out;
    }
    Ok(prev)
}

/// 13A-4b-i: stages every NESTED level below `nodes` before the root loop.
/// Each node is evaluated over `mine.stages[*passed].plane` (`*passed` the
/// guided lone nodes of `mine`'s level already passed — 13A-4b-ii-a:
/// wherever they sit, pass-through children included) or `target`; a node's
/// own subtree is entered before its members' (the walk's order). A
/// pass-through child recurses with the SAME `mine`, `passed` and `target`
/// (its nodes are the enclosing level's: the plan absorbed it); a
/// materialized child (`style.buffer`) has no prepared children and is never
/// entered; an ISOLATED child whose children hold a guided node ANYWHERE on a
/// path (`guided_paths`, through its pass-through children) POPS the next
/// pending level BEFORE recursing (so the pops follow the plan's pre-order),
/// recurses with it (its own `passed` from 0) and then renders it over its
/// `target` (the region) and holds the final plane in `Node::staged`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stage_nested(
    nodes: &mut [Node<'_>],
    plan: &RegionalPlan,
    target: PixelRegion,
    mine: Option<&StagedLevel>,
    passed: &mut usize,
    pending: &mut std::slice::Iter<'_, StagedLevel>,
    env: &StyleEnv<'_>,
    progress: &mut Progress<'_>,
) -> Result<(), String> {
    for node in nodes.iter_mut() {
        let region = mine
            .and_then(|m| m.stages.get(*passed))
            .map_or(target, |stage| stage.plane);
        let at = (region, target, mine);
        stage_node(node, plan, at, passed, pending, env, progress)?;
        if guided(node) {
            *passed += 1;
        }
    }
    Ok(())
}

/// [`stage_nested`]'s descent into one node evaluated over `region` (`at`:
/// the region, the enclosing level's target and staged level), then into its
/// members, each over the region after the guided members before it
/// (13A-4b-ii-b-1).
fn stage_node(
    node: &mut Node<'_>,
    plan: &RegionalPlan,
    (region, target, mine): (PixelRegion, PixelRegion, Option<&StagedLevel>),
    passed: &mut usize,
    pending: &mut std::slice::Iter<'_, StagedLevel>,
    env: &StyleEnv<'_>,
    progress: &mut Progress<'_>,
) -> Result<(), String> {
    let materialized = node.style.as_ref().is_some_and(|s| s.buffer.is_some());
    match &node.layer.kind {
        _ if materialized => {}
        Kind::PassThrough(_) => {
            stage_nested(
                &mut node.children,
                plan,
                target,
                mine,
                passed,
                pending,
                env,
                progress,
            )?;
        }
        Kind::Group(_) => {
            let mine = if guided_paths(&node.children).is_empty() {
                None
            } else {
                let staged = pending.next().ok_or(MISMATCH)?;
                let t = staged.target;
                if (t.x, t.y, t.width, t.height)
                    != (region.x, region.y, region.width, region.height)
                {
                    return Err(MISMATCH.into());
                }
                Some(staged)
            };
            // An isolated buffer is D4's hard stop.
            let inner = isolated_env(env);
            stage_nested(
                &mut node.children,
                plan,
                region,
                mine,
                &mut 0,
                pending,
                &inner,
                progress,
            )?;
            if let Some(staged) = mine {
                let plane = render_staged(
                    &mut node.children,
                    false,
                    staged,
                    Last::Region(staged.target),
                    plan,
                    &inner,
                    progress,
                )?;
                node.staged = Some(StagedPlane {
                    region: staged.target,
                    plane,
                });
            }
        }
        _ => {}
    }
    // 13A-4b-ii-b-1: each member over the plane after the guided members
    // before it, a guided member counted as passed.
    for m in 0..node.members.len() {
        let region = mine
            .and_then(|staged| staged.stages.get(*passed))
            .map_or(target, |stage| stage.plane);
        let guided = guided(&node.members[m]);
        stage_node(
            &mut node.members[m],
            plan,
            (region, target, mine),
            passed,
            pending,
            env,
            progress,
        )?;
        if guided {
            *passed += 1;
        }
    }
    Ok(())
}
