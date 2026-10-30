//! The regional compositor's PLAN types (moved out of `regional` in
//! 13A-4b-ii-b-1, verbatim): the window request, each contributing layer's
//! declared evaluation need and the admitted plan with its owned JSON. The
//! contract they declare is `regional`'s module doc.
//! The staged-level planner (`plan_levels`, `plan_level`) and the plan's
//! `input_halo` moved here from `regional_guide` in 13A-4b-ii-b-2, verbatim.
//! It also owns the shared canvas rectangle [`PixelRegion`] and the
//! cancellation cadence [`CHECKPOINT_PIXELS`], moved from the deleted Phase 4
//! `render_request`.

use serde_json::{json, Value};

use crate::regional_guide::{guide_geometry, LevelRecord, Stage, StagedLevel, BORDER_POLICY};

/// Check cancellation every 4096 output pixels even for a one-row image. This
/// keeps the bound independent of source width; it is a work bound rather than
/// a scheduler-dependent wall-clock promise.
pub(crate) const CHECKPOINT_PIXELS: usize = 4096;

#[derive(Clone, Copy)]
pub(crate) struct PixelRegion {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl PixelRegion {
    pub(crate) fn json(self) -> Value {
        json!({"x": self.x, "y": self.y, "width": self.width, "height": self.height})
    }
}

/// A canvas window and its preview divisor.
#[derive(Clone, Copy)]
pub struct RegionRequest {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub preview_divisor: u32,
}

impl RegionRequest {
    /// The output dimensions, ceil(w/d) × ceil(h/d), after the request checks
    /// of the Phase 4 contract (`designs/phase-4-rendering-contract.md`), in
    /// its order and with its sentences (pinned by `regional_document.rs`,
    /// `regional_precision.rs` and `precision_compositor_parity.rs`): a
    /// nonzero window, inside the canvas (overflow-checked), divisor ≥ 1.
    pub fn output_dims(self, canvas: (u32, u32)) -> Result<(u32, u32), String> {
        if self.width == 0 || self.height == 0 {
            return Err("render region must have nonzero dimensions".into());
        }
        if self
            .x
            .checked_add(self.width)
            .is_none_or(|end| end > canvas.0)
            || self
                .y
                .checked_add(self.height)
                .is_none_or(|end| end > canvas.1)
        {
            return Err("render region is outside the source image".into());
        }
        if self.preview_divisor == 0 {
            return Err("render preview divisor must be at least 1".into());
        }
        Ok((
            (self.width - 1) / self.preview_divisor + 1,
            (self.height - 1) / self.preview_divisor + 1,
        ))
    }

    /// The canvas pixel output sample (i, j) reads: (x + i·d, y + j·d). In
    /// i64, so no product can wrap.
    pub fn sample(self, i: u32, j: u32) -> (i64, i64) {
        let d = i64::from(self.preview_divisor);
        (
            i64::from(self.x) + i64::from(i) * d,
            i64::from(self.y) + i64::from(j) * d,
        )
    }

    pub(crate) fn region(self) -> PixelRegion {
        PixelRegion {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }

    /// The window grown by `halo` pixels on every side, clamped to the
    /// canvas: the input a spatial neighbourhood reads.
    pub(crate) fn grown(self, halo: u32, canvas: (u32, u32)) -> PixelRegion {
        let x0 = self.x.saturating_sub(halo);
        let y0 = self.y.saturating_sub(halo);
        let x1 =
            (u64::from(self.x) + u64::from(self.width) + u64::from(halo)).min(u64::from(canvas.0));
        let y1 =
            (u64::from(self.y) + u64::from(self.height) + u64::from(halo)).min(u64::from(canvas.1));
        PixelRegion {
            x: x0,
            y: y0,
            width: (x1 - u64::from(x0)) as u32,
            height: (y1 - u64::from(y0)) as u32,
        }
    }
}

/// What one layer's evaluation reads around each output sample.
#[derive(Clone, Copy)]
pub(crate) enum EvaluationNeed {
    /// The sample's own backdrop pixel alone.
    Pointwise,
    /// A neighbourhood `halo_pixels` wide on every side: a feathered layer
    /// mask's blur reach (13A-2a) or a staged guide's halo (13A-4a).
    Spatial { halo_pixels: u32 },
    /// A whole-layer or full-frame analysis pass before evaluation: a
    /// raster's style planes (13A-3b-i, [`STYLE_PLANES_ANALYSIS`]) or a
    /// styled group's materialization (13A-3b-ii,
    /// [`MATERIALIZATION_ANALYSIS`]); no document-compositor operator is
    /// global (13A-4a's recorded decision).
    Global { analysis: &'static str },
}

/// The analysis an `"effects"` row declares: the style's planes are rendered
/// from the whole layer before the loop (module doc).
pub(crate) const STYLE_PLANES_ANALYSIS: &str = "layer_style_planes";

/// The analysis a materialized styled GROUP's row declares (13A-3b-ii): the
/// group is rendered over its whole extent before the loop (8 bits:
/// `rendered_group`; typed: `render_styled`'s canvas-sized buffer).
pub(crate) const MATERIALIZATION_ANALYSIS: &str = "group_materialization";

impl EvaluationNeed {
    fn neighborhood(self) -> Value {
        match self {
            Self::Pointwise => json!({"kind": "pointwise", "halo_pixels": 0}),
            Self::Spatial { halo_pixels } => {
                json!({"kind": "spatial", "halo_pixels": halo_pixels})
            }
            Self::Global { .. } => json!({"kind": "global", "halo_pixels": null}),
        }
    }

    fn global_analysis(self) -> Value {
        match self {
            Self::Global { analysis } => json!(analysis),
            _ => json!(false),
        }
    }
}

/// One contributing layer's declaration. `index` is the byte bottom-first
/// stack index, or the typed postorder record index.
pub(crate) struct LayerNeed {
    pub index: usize,
    pub name: String,
    /// `"raster"`, `"adjustment"` or `"group"`.
    pub kind: &'static str,
    /// `"composite"` for a raster; the adjustment's op name; `"isolated"` or
    /// `"pass_through"` for a group.
    pub operator: String,
    pub need: EvaluationNeed,
    /// The unit base's index on a clip-run member; `None` on every base and
    /// on every baseless clipped entry.
    pub clip_base: Option<usize>,
    /// The enclosing group's index (byte bottom-first / typed postorder);
    /// `None` at the root.
    pub group: Option<usize>,
    /// An ENABLED layer mask contributes.
    pub mask: bool,
    /// A non-identity Blend If contributes: the typed layer's own, or at 8
    /// bits the admitted style's.
    pub blend_if: bool,
    /// `None` without a rendered style; `"fill"` for a style that renders no
    /// planes (fill opacity, Blend If, or effects whose plane would exceed
    /// `MAX_PIXELS` at 8 bits); `"effects"` for one that renders planes.
    pub style: Option<&'static str>,
}

/// The admitted plan: what the render will allocate, how much work it does and
/// what every contributing layer needs.
pub struct RegionalPlan {
    pub(crate) request: RegionRequest,
    /// `"rgba8"`, `"rgba16"` or `"rgba32f"`.
    pub(crate) depth: &'static str,
    /// The full-frame export this render equals.
    pub(crate) reference: &'static str,
    pub(crate) output: (u32, u32),
    pub(crate) output_bytes: u64,
    pub(crate) scratch_bytes: u64,
    pub(crate) work_units: u64,
    /// The caller's output budget, at 8 bits only (the typed render is
    /// admitted against its working limit instead).
    pub(crate) max_output_bytes: Option<u64>,
    /// One row per contributing layer, in evaluation order — which is
    /// ascending index at both depths (a group's row follows its children's).
    pub(crate) layers: Vec<LayerNeed>,
    /// The input halo (`regional_guide::input_halo`): the largest of each
    /// row's own halo plus the halos of the guides above it; 0 when every
    /// layer is pointwise and no guide is staged.
    pub(crate) halo_pixels: u32,
    /// The canvas the grown input region is clamped to.
    pub(crate) canvas: (u32, u32),
    /// The deepest contributing group nesting: 0 with no group, 1 for a
    /// root-level group, and so on.
    pub(crate) group_depth: u32,
    /// The distinct `Global` analyses the rows declare, in row order; empty
    /// when none does.
    pub(crate) global: Vec<&'static str>,
    /// The STAGED levels (13A-4a at the root, 13A-4b-i below it), PRE-ORDER:
    /// exactly the levels holding a staged guided row whose last region is
    /// not empty (`regional_guide::plan_levels`); empty without one.
    pub(crate) levels: Vec<StagedLevel>,
}

impl RegionalPlan {
    /// The ROOT's staged level: `levels[0]` iff it is the root's (`level`
    /// is `None`); `None` when the root holds no guided row.
    pub(crate) fn root(&self) -> Option<&StagedLevel> {
        self.levels.first().filter(|level| level.level.is_none())
    }

    /// The NESTED staged levels, pre-order (every level but the root's).
    pub(crate) fn nested(&self) -> std::slice::Iter<'_, StagedLevel> {
        let skip = usize::from(self.root().is_some());
        self.levels[skip..].iter()
    }

    /// The owned plan JSON both depths return (`contract_version` 1),
    /// declaring output, scratch (a feathered mask's plane is its padded
    /// extent; a styled raster's effect planes; a materialized group's nest,
    /// synthetic raster, mask crop and planes), work units, the 4096-sample
    /// cancellation checkpoint cadence and each contributing layer's index,
    /// name, kind, operator, neighborhood (a feathered enabled mask is
    /// "spatial" with its blur reach as halo; a raster whose style renders
    /// effect planes is "global" with global_analysis "layer_style_planes" —
    /// the planes are rendered from the whole layer before the loop; a
    /// non-empty materialized group is "global" with global_analysis
    /// "group_materialization" — the group is rendered over its whole extent
    /// before the loop), clip_base (the base's index on a member, else null),
    /// group (the enclosing group's index, else null), mask, blend_if and
    /// style (null, "fill" or "effects"), plus group_depth. The top-level
    /// global_analysis is false, or the list of the rows' distinct analyses;
    /// with none, the top-level neighborhood and required_input_region carry
    /// the input halo (each row's own halo plus the halos of the guides above
    /// it), clamped to the canvas, and with one required_input_region is the
    /// WHOLE canvas. "stages" lists each staged guide, level by level in
    /// pre-order (the root's first) and bottom-first within a level ({level,
    /// buffer, layer, plane, cell_pixels, reach_cells}: level null at the
    /// root, else the group's index; buffer the clamp rect its planes are
    /// aligned to — the canvas at the root and at 16 bits, the group's rect at
    /// 8 bits; empty without one) and "border_policy" is "canvas_clamped"; a
    /// guided row is "spatial" with its guide's halo. "depth" is the
    /// document's storage depth. The typed plan omits max_output_bytes.
    pub fn json(&self) -> Value {
        let region = self.request.region().json();
        let d = self.request.preview_divisor;
        let neighborhood = if self.halo_pixels > 0 {
            EvaluationNeed::Spatial {
                halo_pixels: self.halo_pixels,
            }
        } else {
            EvaluationNeed::Pointwise
        };
        // A `Global` row reads the whole layer before the loop, so the input
        // is the whole canvas; otherwise the window grown by the largest halo.
        let (global_analysis, required_input) = if self.global.is_empty() {
            (
                json!(false),
                self.request.grown(self.halo_pixels, self.canvas),
            )
        } else {
            (
                json!(self.global),
                PixelRegion {
                    x: 0,
                    y: 0,
                    width: self.canvas.0,
                    height: self.canvas.1,
                },
            )
        };
        let mut out = json!({
            "contract_version": 1,
            "operation": "composite",
            "depth": self.depth,
            "reference": self.reference,
            "requested_region": region,
            "required_input_region": required_input.json(),
            "neighborhood": neighborhood.neighborhood(),
            "global_analysis": global_analysis,
            "full_frame_fallback": false,
            "preview_divisor": d,
            "preview_only": d != 1,
            "sampling": "nearest_top_left_cell",
            "output": {"width": self.output.0, "height": self.output.1, "bytes": self.output_bytes},
            "scratch_heap_bytes": self.scratch_bytes,
            "work_units": self.work_units,
            "cancellation_checkpoint_pixels": CHECKPOINT_PIXELS,
            "group_depth": self.group_depth,
            "stages": self.levels.iter().flat_map(|level| level.stages.iter().map(|stage| json!({
                "level": level.level,
                "buffer": level.buffer.json(),
                "layer": stage.layer,
                "plane": stage.plane.json(),
                "cell_pixels": stage.geometry.factor,
                "reach_cells": stage.geometry.reach_cells,
                "through": stage.through.iter().map(|t| t.group).collect::<Vec<_>>(),
            }))).collect::<Vec<_>>(),
            "border_policy": BORDER_POLICY,
            "layers": self.layers.iter().map(|layer| json!({
                "index": layer.index,
                "name": layer.name,
                "kind": layer.kind,
                "operator": layer.operator,
                "neighborhood": layer.need.neighborhood(),
                "global_analysis": layer.need.global_analysis(),
                "clip_base": layer.clip_base,
                "group": layer.group,
                "mask": layer.mask,
                "blend_if": layer.blend_if,
                "style": layer.style,
            })).collect::<Vec<_>>(),
        });
        if let Some(budget) = self.max_output_bytes {
            out["max_output_bytes"] = json!(budget);
        }
        out
    }
}

/// The pixel count of `r` (moved from `regional_guide` in 13A-4b-ii-b-1).
pub(crate) fn area(r: PixelRegion) -> u64 {
    u64::from(r.width) * u64::from(r.height)
}

/// `a ∩ b`, `None` when empty.
pub(crate) fn intersect(a: PixelRegion, b: PixelRegion) -> Option<PixelRegion> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (u64::from(a.x) + u64::from(a.width)).min(u64::from(b.x) + u64::from(b.width));
    let y1 = (u64::from(a.y) + u64::from(a.height)).min(u64::from(b.y) + u64::from(b.height));
    (x1 > u64::from(x0) && y1 > u64::from(y0)).then(|| PixelRegion {
        x: x0,
        y: y0,
        width: (x1 - u64::from(x0)) as u32,
        height: (y1 - u64::from(y0)) as u32,
    })
}

/// The plan's staged levels over the walk's `record` tree, PRE-ORDER, with
/// their EXTRA scratch and the staged WORK. A level with no `buffer` or an
/// empty `R_m = target ∩ buffer` contributes nothing, its subtree included.
/// A level emits itself only when it holds a guided row: its planes from the
/// LAST guide backwards (`R_{k−1} = plane_region_k(R_k, buffer)`), `rows_k`
/// the plan rows with index in `[g_k, g_{k+1})` (`g_0` the range's start,
/// `g_{m+1}` its end — descendants included, so an enclosing level still
/// counts them over its own plane: conservative, `report` clamps) — for a
/// CLIP MEMBER's stage from its unit's first row instead of `g_k`
/// (13A-4b-ii-b-1: every sample of that fill re-runs the unit from its base,
/// so the rows it evaluates are charged, never under). At the
/// ROOT `R_m` is the request window and the last stage samples the OUTPUT —
/// scratch `Σ_{k<m} |R_k|·16` plus every guide's (`adjust_precision::plan::
/// guide`, the f32 guide's figure: conservative at 8 bits) plus, under D5,
/// the Background snapshot `|R_0|·16`; work `Σ_{k<m} |R_k|·rows_k +
/// output_pixels·rows_m` plus every guide's; without a root guide
/// `output_pixels · rows`. NESTED, every stage is a divisor-1 region and the
/// final plane `R_m` is HELD: scratch `Σ_{k≤m} |R_k|·16` plus the guides',
/// work `Σ_{k≤m} |R_k|·rows_k` plus the guides'. Either way each distinct
/// HELD pass-through group over the stages' `through` adds scratch
/// `16·|R_0|` and work `|R_0|` (13A-4b-ii-a: the hold's plane and its fill),
/// and so does each distinct CLIP UNIT holding a guided member (its began
/// sample, 13A-4b-ii-b-1).
/// Every child is planned with the target its unit is evaluated over —
/// `stages[count].plane` below the last guide, else `R_m` — whether or not
/// this level emits.
pub(crate) fn plan_levels(
    record: &LevelRecord,
    target: PixelRegion,
    rows: &[LayerNeed],
    output_pixels: u64,
    background: bool,
) -> Result<(Vec<StagedLevel>, u64, u64), String> {
    let mut out = Vec::new();
    let mut scratch = 0u64;
    let mut work = 0u64;
    plan_level(
        record,
        target,
        rows,
        Some((output_pixels, background)),
        &mut out,
        &mut scratch,
        &mut work,
    )?;
    Ok((out, scratch, work))
}

/// One level of [`plan_levels`]; `root` carries the output's pixels and D5
/// at the root only.
fn plan_level(
    record: &LevelRecord,
    target: PixelRegion,
    rows: &[LayerNeed],
    root: Option<(u64, bool)>,
    out: &mut Vec<StagedLevel>,
    scratch: &mut u64,
    work: &mut u64,
) -> Result<(), String> {
    let Some(buffer) = record.buffer else {
        return Ok(());
    };
    let Some(last) = intersect(target, buffer) else {
        return Ok(());
    };
    let count = |lo: usize, hi: usize| {
        rows.iter()
            .filter(|r| r.index >= lo && r.index < hi)
            .count() as u64
    };
    let m = record.guided.len();
    // g_0 … g_{m+1}: the range's start, the guided rows, the range's end.
    let mut bounds = Vec::with_capacity(m + 2);
    bounds.push(record.range.start);
    bounds.extend(record.guided.iter().map(|row| row.layer));
    bounds.push(record.range.end);
    let lower = |k: usize| match k {
        0 => bounds[0],
        _ => record.guided[k - 1]
            .clip_unit
            .map_or(bounds[k], |unit| unit.start),
    };
    let rows_k = |k: usize| count(lower(k), bounds[k + 1]);
    // Planes from the LAST guide backwards, anchored at the buffer.
    let mut inner = last;
    let mut stages: Vec<Stage> = Vec::with_capacity(m);
    for k in (1..=m).rev() {
        let row = &record.guided[k - 1];
        let geometry = guide_geometry(row.radius);
        let plane = geometry.plane_region(inner, buffer);
        inner = plane;
        stages.push(Stage {
            layer: row.layer,
            plane,
            geometry,
            rows: rows_k(k),
            through: row.through.clone(),
            clip_unit: row.clip_unit,
        });
    }
    stages.reverse();
    let rows_0 = rows_k(0);
    match (root, stages.first()) {
        (Some((output_pixels, _)), None) => {
            *work = work.saturating_add(output_pixels.saturating_mul(rows_0));
        }
        (Some((output_pixels, background)), Some(first)) => {
            *work = work.saturating_add(area(first.plane).saturating_mul(rows_0));
            for (k, stage) in stages.iter().enumerate() {
                let radius = record.guided[k].radius;
                let (guide_scratch, guide_work) = crate::adjust_precision::plan::guide(
                    stage.plane.width,
                    stage.plane.height,
                    radius,
                )?;
                let evaluated = stages
                    .get(k + 1)
                    .map_or(output_pixels, |next| area(next.plane));
                *scratch = scratch
                    .saturating_add(area(stage.plane).saturating_mul(16))
                    .saturating_add(guide_scratch);
                *work = work
                    .saturating_add(evaluated.saturating_mul(stage.rows))
                    .saturating_add(guide_work);
            }
            if background {
                *scratch = scratch.saturating_add(area(first.plane).saturating_mul(16));
            }
        }
        (None, None) => {}
        (None, Some(first)) => {
            // Every stage a divisor-1 region, the final plane HELD.
            *work = work.saturating_add(area(first.plane).saturating_mul(rows_0));
            for (k, stage) in stages.iter().enumerate() {
                let radius = record.guided[k].radius;
                let (guide_scratch, guide_work) = crate::adjust_precision::plan::guide(
                    stage.plane.width,
                    stage.plane.height,
                    radius,
                )?;
                let evaluated = stages.get(k + 1).map_or(last, |next| next.plane);
                *scratch = scratch
                    .saturating_add(area(stage.plane).saturating_mul(16))
                    .saturating_add(guide_scratch);
                *work = work
                    .saturating_add(area(evaluated).saturating_mul(stage.rows))
                    .saturating_add(guide_work);
            }
            *scratch = scratch.saturating_add(area(last).saturating_mul(16));
        }
    }
    if let Some(first) = stages.first() {
        // 13A-4b-ii-a: one plane over `R_0` per distinct HELD pass-through
        // group on the stages' paths, and its fill; 13A-4b-ii-b-1: one per
        // distinct clip unit holding a guided member (its began sample).
        let mut held: Vec<usize> = Vec::new();
        let groups = stages
            .iter()
            .flat_map(|stage| &stage.through)
            .filter(|through| through.held)
            .map(|through| through.group);
        let units = stages
            .iter()
            .filter_map(|stage| stage.clip_unit.map(|u| u.base));
        for group in groups.chain(units) {
            if !held.contains(&group) {
                held.push(group);
            }
        }
        let holds = area(first.plane).saturating_mul(held.len() as u64);
        *scratch = scratch.saturating_add(holds.saturating_mul(16));
        *work = work.saturating_add(holds);
    }
    let targets: Vec<PixelRegion> = stages.iter().map(|stage| stage.plane).collect();
    if !stages.is_empty() {
        out.push(StagedLevel {
            level: record.group,
            buffer,
            target: last,
            rows_0,
            stages,
        });
    }
    for (below, child) in &record.children {
        let child_target = targets.get(*below).copied().unwrap_or(last);
        plan_level(child, child_target, rows, None, out, scratch, work)?;
    }
    Ok(())
}

/// The plan's top-level halo: the max over rows `r` of `own_r` plus the
/// `halo_pixels` of every stage — of ANY level — whose layer is STRICTLY
/// ABOVE `r` in row order (`own_r` the row's `Spatial` halo, else 0). A
/// nested level's planes lie within its enclosing level's plane (or window)
/// grown by its own stages, so the sum over every stage above a row bounds
/// every plane it is evaluated over (conservative across sibling levels).
/// One reverse pass. With no stage it is the largest `Spatial` halo.
pub(crate) fn input_halo(rows: &[LayerNeed], levels: &[StagedLevel]) -> u32 {
    let mut above = 0u32;
    let mut halo = 0u32;
    for row in rows.iter().rev() {
        let own = match row.need {
            EvaluationNeed::Spatial { halo_pixels } => halo_pixels,
            _ => 0,
        };
        halo = halo.max(own.saturating_add(above));
        if let Some(stage) = levels
            .iter()
            .flat_map(|level| &level.stages)
            .find(|stage| stage.layer == row.index)
        {
            above = above.saturating_add(stage.geometry.halo_pixels());
        }
    }
    halo
}
