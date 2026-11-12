//! The 8-bit regional compositor's STAGED window loop (Phase 13A-4a at the
//! root, 13A-4b-i in isolated groups): a level holding a guide-reading
//! Shadows/Highlights (lone or a baseless clipped entry) is evaluated in
//! stages over the planes `regional_guide::plan_levels` declares — stage 0
//! fills `P_0` over `R_0` at divisor 1 with the units below the first guided
//! unit; each later stage builds that unit's guide from the previous plane
//! (`Adjustment::guide_with_job`, the reference's own `adjust_guide::build`),
//! hands it to the unit (`ResolvedLevel::set_guide`) and evaluates the units
//! up to the next guided unit over the next plane — the last one over the
//! OUTPUT at the request's sampling at the root ([`Last::Output`]), or over
//! the level's `target` at divisor 1 in a nested level ([`Last::Region`]),
//! whose plane is then HELD and read in place of the group's children
//! (`Source::Staged`, [`stage_nested`], before the root loop). D5's
//! Background split runs in the root's stage 0 (unit 0 alone, its sample
//! snapshotted into `S`, every later unit of EVERY stage against `S` as both
//! planes — `ResolvedLevel::sample`'s split); a nested level samples under
//! `KnockoutBackdrop::NONE` (an isolated buffer's hard stop). Since
//! 13A-4b-ii-a a guided unit may sit directly inside PASS-THROUGH children of
//! the level (plain or gated, nested): it is reached by a CURSOR, the stage
//! plane holds the working sample there (the enclosing accumulator, or the
//! gated copy of it), and the sample as each such group's children began is
//! HELD as a plane (`regional_guide::Holds`) from the fill that enters the
//! group to the fill that finishes it. Since 13A-4b-ii-b-1 a guided unit may
//! be a CLIP MEMBER of an unstyled base — since 13A-4b-ii-b-2 of a styled one
//! too (the styled-direct and routed member loops, `styled_sample` with
//! `stop`; a packaged base refuses): its cursor ends in the member's step,
//! the sample as the unit began is held from the fill that enters the unit,
//! and every later fill re-runs the unit from it. Since 13A-5a each HOLD
//! fill advances `run.done` and reports per 4096-sample chunk, and since
//! 13A-5b-iii-a so do stage 0's `prev` and D5 `snapshot` fills, while the
//! guide build reports INSIDE ITS RANGE (`job.range(done / total, (done +
//! g) / total)`, `g` its plan work) before the `run.done` advance after it
//! (the `regional` CANCELLATION MODEL). Since 13A-5b-iii-b-2 the shared
//! [`Run`] lives in `doc_regional_progress`, where the unstaged fill and the
//! per-stage preparation ranges use it too; every report here stays stage 6.
//! Index arithmetic only:
//! every sample is `ResolvedLevel::sample_between`, which runs the
//! reference's per-pixel bodies. The contract is `regional`'s module doc; the
//! geometry and exactness argument `regional_guide`'s.

use crate::doc_evaluation::ByteTransfer;
use crate::doc_regional_progress::Run;
use crate::doc_regional_tree::{ResolvedLevel, ResolvedNode, ResolvedStep, ResolvedUnit, Source};
use crate::job_control::JobContext;
use crate::regional::RegionalPlan;
use crate::regional_guide::{
    index_in, intersect, Holds, Last, PreparedGuide, StagedLevel, StagedPlane,
};
use crate::regional_plan::PixelRegion;
use crate::style_knockout::KnockoutBackdrop;

const MISMATCH: &str = "regional stage mismatch";

/// A plane of `n` samples, reserved first and filled in 4096-sample chunks,
/// polling `job` (`render_region`'s accumulator fill). With `Some(run)` —
/// a hold (13A-5a), or the stage-0 `prev` and D5 `snapshot` fills
/// (13A-5b-iii-a), each of whose fills the plan counts — each chunk adds its
/// samples to `run.done` and reports `run.done / run.total` (the typed
/// `zero_plane` → `Progress::advance` twin).
fn plane(
    n: usize,
    job: JobContext<'_>,
    mut run: Option<&mut Run<'_>>,
) -> Result<Vec<[f32; 4]>, String> {
    let mut out: Vec<[f32; 4]> = Vec::new();
    out.try_reserve_exact(n)
        .map_err(|_| "could not allocate regional render workspace".to_string())?;
    while out.len() < n {
        job.every(out.len())?;
        let added = (n - out.len()).min(4096);
        out.extend(std::iter::repeat_n([0.0f32; 4], added));
        if let Some(run) = run.as_deref_mut() {
            *run.done += added as f64;
            job.report(*run.done / run.total, 6)?;
        }
    }
    Ok(out)
}

fn pixels(region: PixelRegion) -> usize {
    region.width as usize * region.height as usize
}

/// The HOLD SCHEDULE before one fill from cursor `from` to cursor `to` over
/// `region` (13A-4b-ii-a): every proper prefix `p` of `to` (outermost first)
/// that is not a proper prefix of `from` — a pass-through unit this fill
/// ENTERS — and whose sample as its children began is read
/// (`snapshot_at`), or — 13A-4b-ii-b-1 — the CLIP unit whose member step
/// ends `to` (`clip_unit_at`: the sample as the unit began, which every
/// later fill re-runs it from), gets a plane over `region`, filled by
/// [`plane`] (polling every 4096 samples and, since 13A-5a, advancing
/// `run.done` by each chunk and reporting), which the fill's samples write as
/// they enter it.
fn enter_holds(
    level: &ResolvedLevel<'_>,
    holds: &mut Holds,
    from: &[usize],
    to: &[usize],
    region: PixelRegion,
    run: &mut Run<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    for len in 1..to.len() {
        let p = &to[..len];
        let resumed = from.len() > len && from[..len] == *p;
        let clip = p.len() + 1 == to.len() && level.clip_unit_at(p);
        if !resumed && (level.snapshot_at(p) || clip) {
            holds.enter(p, region, plane(pixels(region), job, Some(&mut *run))?);
        }
    }
    Ok(())
}

/// One later stage's loop: the units from cursor `from` to cursor `to` over
/// `dims` samples at `xy_of(i, j)`, each seeded from `prev` (built over
/// `built`) and, under D5, against the snapshot over `r0` as both planes.
#[allow(clippy::too_many_arguments)]
fn evaluate(
    level: &ResolvedLevel<'_>,
    (from, to): (&[usize], &[usize]),
    holds: &mut Holds,
    prev: &[[f32; 4]],
    built: PixelRegion,
    dims: (u32, u32),
    xy_of: impl Fn(u32, u32) -> (i64, i64),
    snapshot: Option<(&[[f32; 4]], PixelRegion)>,
    rows: u64,
    run: &mut Run<'_>,
    transfer: Option<&ByteTransfer>,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let mut out: Vec<[f32; 4]> = Vec::new();
    out.try_reserve_exact(dims.0 as usize * dims.1 as usize)
        .map_err(|_| "could not allocate regional render workspace".to_string())?;
    let mut prefix = Vec::new();
    for j in 0..dims.1 {
        for i in 0..dims.0 {
            job.every(run.sample)?;
            run.sample += 1;
            let xy = xy_of(i, j);
            out.push(prev[index_in(built, xy)]);
            let q = out.len() - 1;
            let snap = snapshot.map(|(s, r0)| [s[index_in(r0, xy)]]);
            let knockout = snap
                .as_ref()
                .map_or(KnockoutBackdrop::NONE, |s| KnockoutBackdrop {
                    shallow: Some(s),
                    deep: Some(s),
                });
            level.sample_between(
                from,
                to,
                &mut prefix,
                &mut out,
                q,
                transfer,
                xy,
                knockout,
                holds,
            );
        }
        *run.done += rows as f64 * f64::from(dims.0);
        job.report(*run.done / run.total, 6)?;
    }
    Ok(out)
}

/// The staged render of `level` for `staged` (whose `stages` are non-empty):
/// at the root ([`Last::Output`]) the output accumulator, `plan.output`
/// samples at the request's sampling; nested ([`Last::Region`]) the held
/// final plane over the region at divisor 1. `done` is the render's shared
/// completed work, reported against `plan.work_units`. Since 13A-4b-ii-a each
/// guided unit is reached by a CURSOR (`path_of_layer`: unit positions down
/// through pass-through children) and every fill runs `sample_between` from
/// one cursor to the next, the pass-through samples entered and not yet
/// finished held in [`Holds`] for the whole call (the hold schedule:
/// [`enter_holds`] before each fill, each hold's fill reporting its samples
/// into `done` — 13A-5a; stage 0's `prev` and D5 `snapshot` fills too since
/// 13A-5b-iii-a). Each guide is built under `job.range(start, end)` over its
/// plan work `g` (`start = done / total`, `end = (done + g) / total`, the
/// typed `precision_regional_guide` pattern), then `done` advances by `g`
/// and reports (13A-5b-iii-a).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_staged(
    level: &mut ResolvedLevel<'_>,
    background: bool,
    staged: &StagedLevel,
    last: Last,
    plan: &RegionalPlan,
    done: &mut f64,
    transfer: Option<&ByteTransfer>,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let stages = &staged.stages;
    let first = stages.first().ok_or(MISMATCH)?;
    // The cursor of each guided layer, strictly increasing (an adjustment
    // base never resolves to `None`, so a failure here is defensive).
    let mut paths: Vec<Vec<usize>> = Vec::with_capacity(stages.len());
    for stage in stages {
        let path = level.path_of_layer(stage.layer).ok_or(MISMATCH)?;
        if paths.last().is_some_and(|before| path <= *before) {
            return Err(MISMATCH.into());
        }
        paths.push(path);
    }
    if background && paths[0][0] == 0 {
        return Err(MISMATCH.into());
    }
    let mut run = Run {
        sample: 0,
        done,
        total: plan.work_units.max(1) as f64,
    };
    let rows_0 = staged.rows_0;
    let mut holds = Holds(Vec::new());
    let mut prefix = Vec::new();
    // Stage 0: P_0 over R_0 at divisor 1, the units below the first guide.
    let r0 = first.plane;
    let mut prev = plane(pixels(r0), job, Some(&mut run))?;
    let mut snapshot = if background {
        plane(pixels(r0), job, Some(&mut run))?
    } else {
        Vec::new()
    };
    enter_holds(level, &mut holds, &[], &paths[0], r0, &mut run, job)?;
    for y in 0..r0.height {
        for x in 0..r0.width {
            job.every(run.sample)?;
            run.sample += 1;
            let xy = (i64::from(r0.x + x), i64::from(r0.y + y));
            let p = y as usize * r0.width as usize + x as usize;
            if background {
                let none = KnockoutBackdrop::NONE;
                level.sample_between(
                    &[],
                    &[1],
                    &mut prefix,
                    &mut prev,
                    p,
                    transfer,
                    xy,
                    none,
                    &mut holds,
                );
                snapshot[p] = prev[p];
                let snap = [snapshot[p]];
                let backdrop = KnockoutBackdrop {
                    shallow: Some(&snap),
                    deep: Some(&snap),
                };
                level.sample_between(
                    &[1],
                    &paths[0],
                    &mut prefix,
                    &mut prev,
                    p,
                    transfer,
                    xy,
                    backdrop,
                    &mut holds,
                );
            } else {
                let none = KnockoutBackdrop::NONE;
                level.sample_between(
                    &[],
                    &paths[0],
                    &mut prefix,
                    &mut prev,
                    p,
                    transfer,
                    xy,
                    none,
                    &mut holds,
                );
            }
        }
        *run.done += rows_0 as f64 * f64::from(r0.width);
        job.report(*run.done / run.total, 6)?;
    }
    // Stages 1..=m: the guide from the previous plane, then its units over
    // the next plane (or the level's last region).
    for (k, stage) in stages.iter().enumerate() {
        let from = paths[k].as_slice();
        let to = paths.get(k + 1).map_or(&[][..], Vec::as_slice);
        let built = stage.plane;
        let adjustment = level.unit_adjustment(from).ok_or(MISMATCH)?;
        let radius = adjustment.guide_radius().ok_or(MISMATCH)?;
        let g = crate::adjust_precision::plan::guide(built.width, built.height, radius)?.1 as f64;
        let (start, end) = (*run.done / run.total, (*run.done + g) / run.total);
        let guide = adjustment
            .guide_with_job(
                built.width,
                built.height,
                &|i| crate::doc_evaluation::guide_view(prev[i], transfer),
                job.range(start, end),
            )?
            .ok_or(MISMATCH)?;
        level.set_guide(from, PreparedGuide::new(guide, built));
        *run.done += g;
        job.report(*run.done / run.total, 6)?;
        let snap = background.then_some((snapshot.as_slice(), r0));
        let region = match (stages.get(k + 1), &last) {
            // P_k over R_k at divisor 1, or a nested level's held R_m.
            (Some(next), _) => Some(next.plane),
            (None, Last::Region(region)) => Some(*region),
            (None, Last::Output(_)) => None,
        };
        let next = match (region, &last) {
            (Some(region), _) => {
                enter_holds(level, &mut holds, from, to, region, &mut run, job)?;
                evaluate(
                    level,
                    (from, to),
                    &mut holds,
                    &prev,
                    built,
                    (region.width, region.height),
                    |i, j| (i64::from(region.x + i), i64::from(region.y + j)),
                    snap,
                    stage.rows,
                    &mut run,
                    transfer,
                    job,
                )?
            }
            // The OUTPUT accumulator at the request's sampling (`to` is
            // empty: nothing is entered there).
            (None, Last::Output(request)) => {
                let request = *request;
                evaluate(
                    level,
                    (from, to),
                    &mut holds,
                    &prev,
                    built,
                    plan.output,
                    |i, j| request.sample(i, j),
                    snap,
                    stage.rows,
                    &mut run,
                    transfer,
                    job,
                )?
            }
            (None, Last::Region(_)) => return Err(MISMATCH.into()),
        };
        prev = next;
    }
    Ok(prev)
}

/// 13A-4b-i: stages every NESTED level below `level` before the root loop.
/// Each unit is evaluated over `mine.stages[*passed].plane` (`*passed` the
/// guided units of `mine`'s level already passed — 13A-4b-ii-a: wherever
/// they sit, pass-through children included; 13A-4b-ii-b-1: each member
/// step over the plane after the guided members before it, a guided member
/// counted as passed) or `target`; a pass-through
/// child recurses with the SAME `mine`, `passed` and `target` (its units are
/// the enclosing level's: the plan absorbed it), a materialized child is
/// never entered (the reference renders it), and an ISOLATED child over `R =
/// region ∩ rect` — empty: skipped entirely, as the plan skips it — recurses
/// first (its own `passed` from 0), then, when its level holds a guided unit
/// ANYWHERE on a path (`guided_paths`, through its pass-through children),
/// renders its staged level (looked up by the group's index) over `R` and
/// holds the final plane in place of the children (`Source::Staged`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn stage_nested(
    level: &mut ResolvedLevel<'_>,
    plan: &RegionalPlan,
    target: PixelRegion,
    mine: Option<&StagedLevel>,
    passed: &mut usize,
    done: &mut f64,
    transfer: Option<&ByteTransfer>,
    job: JobContext<'_>,
) -> Result<(), String> {
    for unit in &mut level.units {
        let region = mine
            .and_then(|m| m.stages.get(*passed))
            .map_or(target, |stage| stage.plane);
        let at = (region, target, mine);
        stage_node(&mut unit.base, plan, at, passed, done, transfer, job)?;
        for m in 0..unit.steps.len() {
            let region = mine
                .and_then(|m| m.stages.get(*passed))
                .map_or(target, |stage| stage.plane);
            let guided = unit.guided_member(m);
            if let ResolvedStep::Member(member) = &mut unit.steps[m] {
                let at = (region, target, mine);
                stage_node(member, plan, at, passed, done, transfer, job)?;
            }
            if guided {
                *passed += 1;
            }
        }
        if ResolvedUnit::guided(unit) {
            *passed += 1;
        }
    }
    Ok(())
}

/// [`stage_nested`]'s descent into one node evaluated over `region` (`at`:
/// the region, the enclosing level's target and staged level).
fn stage_node(
    node: &mut ResolvedNode<'_>,
    plan: &RegionalPlan,
    (region, target, mine): (PixelRegion, PixelRegion, Option<&StagedLevel>),
    passed: &mut usize,
    done: &mut f64,
    transfer: Option<&ByteTransfer>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let index = node.index;
    let (rect, child) = match &mut node.source {
        Source::PassThrough(child) => {
            return stage_nested(child, plan, target, mine, passed, done, transfer, job);
        }
        Source::Isolated(rect, child) => (*rect, child),
        _ => return Ok(()),
    };
    let bounds = PixelRegion {
        x: rect.0 as u32,
        y: rect.1 as u32,
        width: (rect.2 - rect.0) as u32,
        height: (rect.3 - rect.1) as u32,
    };
    let Some(r) = intersect(region, bounds) else {
        return Ok(());
    };
    let mine = plan.levels.iter().find(|l| l.level == Some(index));
    stage_nested(child, plan, r, mine, &mut 0, done, transfer, job)?;
    if child.guided_paths().is_empty() {
        return Ok(());
    }
    let staged = mine.ok_or(MISMATCH)?;
    let t = staged.target;
    if (t.x, t.y, t.width, t.height) != (r.x, r.y, r.width, r.height) {
        return Err(MISMATCH.into());
    }
    // An isolated buffer is D4's hard stop: no D5, `KnockoutBackdrop::NONE`.
    let plane = render_staged(
        child,
        false,
        staged,
        Last::Region(t),
        plan,
        done,
        transfer,
        job,
    )?;
    node.source = Source::Staged(rect, StagedPlane { region: t, plane });
    Ok(())
}
