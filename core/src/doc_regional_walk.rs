//! The 8-bit regional compositor's ADMISSION WALK (moved out of
//! `doc_regional` in 13A-4b-ii-a, verbatim): the fixed-order walk over the
//! bottom-first stack's LEVELS (recursing into each contributing group, as
//! `doc_group::composite_level_into` does) that builds the tree
//! (`doc_regional_tree`), the plan rows and the `LevelRecord`s
//! `regional_guide::plan_levels` reads. The contract — admitted tree, units,
//! refusal order, evaluation needs — is `regional`'s module doc. No pixel
//! math.

use std::ops::Range;

use crate::adjust::Adjustment;
use crate::blend::BlendMode;
use crate::doc::{DrDocument, Layer, LayerKind};
use crate::doc_group::{self, MAX_GROUP_DEPTH};
use crate::doc_regional_progress::PrepWork;
use crate::doc_regional_tree::{pass_through_holds, Contribution, Level, Node, Rect, Step, Unit};
use crate::precision_style_render::{plane_pixels, style_scratch};
use crate::regional::{
    refusal, EvaluationNeed, LayerNeed, MATERIALIZATION_ANALYSIS, PACKAGED_BASE, STYLED_ADJUSTMENT,
    STYLE_PLANES_ANALYSIS,
};
use crate::regional_guide::{guide_geometry, ClipUnit, Enclosure, GuidedRow, LevelRecord};
use crate::regional_plan::PixelRegion;
use crate::style::LayerStyle;
use crate::style_composite::CompositeEnv;

/// An entry the reference composites: visible and at a positive sanitized
/// opacity (`composite_level_into`'s `visible` guard, `composite_layer_into` /
/// `composite_clip_group_into`'s opacity return).
pub(crate) fn contributes(layer: &Layer) -> bool {
    layer.visible && crate::opacity_byte::quantized(layer.opacity) > 0.0
}

/// The style the reference renders for a contributing entry: a group's is
/// `Layer::style` (the one its synthetic layer carries; its meta is never
/// read), a raster's `renders_style`.
pub(crate) fn style_of(layer: &Layer) -> Option<&LayerStyle> {
    match layer.kind {
        LayerKind::Group => layer.style.as_deref(),
        LayerKind::Raster => layer.renders_style(),
    }
}

/// Whether a style renders PLANES: an enabled effect and a padded plane
/// within `MAX_PIXELS` — exactly when `Shape::of_layer` (and so
/// `LayerStyle::rendered_with_job`) is `Some`; otherwise the reference renders no
/// effect at all. A raster's plane is its pixels'; a (materialized) group's
/// is its synthetic raster's, `rect` — none when the extent is empty.
pub(crate) fn renders_planes(layer: &Layer, style: &LayerStyle, rect: Option<Rect>) -> bool {
    let dims = match layer.kind {
        LayerKind::Raster => Some(layer.pixels.dimensions()),
        LayerKind::Group => rect.map(|r| ((r.2 - r.0) as u32, (r.3 - r.1) as u32)),
    };
    style.has_enabled_effects()
        && dims.is_some_and(|dims| {
            plane_pixels(dims, style.pad()).is_ok_and(|p| p <= crate::doc::MAX_PIXELS)
        })
}

/// The admission walk's state: the plan rows and the running scratch, halo
/// and nesting figures.
pub(crate) struct Walk<'a> {
    pub(crate) doc: &'a DrDocument,
    /// `composite_env(None, None)`'s geometry — what `group_extent` reads —
    /// plus the admission's live job, which the walk polls once per node, and
    /// its `Cut` (13A-5c-2b: `Cut::NONE` for every regional render, the
    /// sampling plate's cut for `regional_cut_plate`).
    pub(crate) env: CompositeEnv<'a>,
    pub(crate) needs: Vec<LayerNeed>,
    pub(crate) mask_scratch: u64,
    pub(crate) style_scratch: u64,
    /// Every materialized group's peak nest, synthetic, mask crop and planes,
    /// plus (13A-5c-3) the reference's uncharged per-unit buffers inside the
    /// nest (`regional_admission`, after the walk).
    pub(crate) materialization_scratch: u64,
    /// The materialized groups the reference renders, in walk order: the
    /// group, the enclosing rect and its nesting depth — the dry run's input.
    pub(crate) materializations: Vec<(doc_group::Unit, Rect, u16)>,
    pub(crate) group_depth: u32,
    /// The distinct `Global` analyses, in row order.
    pub(crate) global: Vec<&'static str>,
    /// The preparation WORK the render advances (13A-5b-iii-b-2): every
    /// resolved node's mask blur (`MaskProperties::work_units`), styled
    /// raster's or materialized group's planes (`style_scratch(..).1`) and
    /// materialization — each a node's [`PrepWork`], counted exactly when
    /// the render resolves the node.
    pub(crate) mask_work: u64,
    pub(crate) style_work: u64,
    pub(crate) materialization_work: u64,
    /// The mask and plane work of every node inside a materialized subtree so
    /// far — the reference chain spends it inside `rendered_group`, so each
    /// materialized group's term takes the difference across its children.
    pub(crate) inside_prep: u64,
}

impl<'a> Walk<'a> {
    /// One LEVEL — the units `composite_level_into` forms over `range` at
    /// `depth` — in the fixed refusal order (`regional`'s module doc):
    /// (1) the unit skip, (2) every contributing entry's node — a group's
    /// children level first, an adjustment's operator — in order.
    /// `group` is the enclosing group's index, `nesting` its nesting (0 at
    /// the root), `clip` the enclosing accumulator's rect (`None` inside an
    /// isolated group whose extent is empty). `collect`: this subtree's
    /// materialized groups are pushed to `materializations` for the dry run.
    /// False inside a materialized subtree (the dry run recurses there
    /// itself) and for the member run of a unit whose base is a group node
    /// with `rect == None` (the reference never resolves those members).
    /// `enclosure` is where the level sits (13A-4b-i); the returned record
    /// carries its staged guided rows and its Isolated / PassThrough child
    /// levels for `plan_levels`, its `buffer` being `clip` — the group's
    /// rect for an isolated level, the enclosing one for a pass-through level.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn level(
        &mut self,
        range: Range<usize>,
        depth: u16,
        group: Option<usize>,
        nesting: u32,
        clip: Option<Rect>,
        collect: bool,
        enclosure: Enclosure,
    ) -> Result<(Level<'a>, LevelRecord), String> {
        let layers = &self.doc.layers;
        let mut record = LevelRecord {
            group,
            range: range.clone(),
            buffer: clip.map(|r| PixelRegion {
                x: r.0 as u32,
                y: r.1 as u32,
                width: (r.2 - r.0) as u32,
                height: (r.3 - r.1) as u32,
            }),
            guided: Vec::new(),
            children: Vec::new(),
        };
        let level = doc_group::units(layers, range, depth);
        let mut units = Vec::new();
        let mut i = 0;
        while i < level.len() {
            let unit = level[i];
            let entry = &layers[unit.idx];
            // The unit `composite_level_into` forms at `i`: a baseless
            // clipped entry alone (`i += 1`), else the base and the
            // positional clipped run above it (`doc_group.rs:777-780`).
            let baseless = entry.clipped;
            let end = if baseless {
                i + 1
            } else {
                let mut end = i + 1;
                while end < level.len() && layers[level[end].idx].clipped {
                    end += 1;
                }
                end
            };
            let members = &level[i + 1..end];
            let clip_run = !members.is_empty();
            // (1) the unit skip: a hidden, zero-opacity or CUT base skips its
            // whole run and every subtree in it unchecked (the reference's
            // `entry.visible && !env.cut.excludes(unit.idx)`, 13A-5c-2b).
            if !contributes(entry) || self.env.cut.excludes(unit.idx) {
                i = end;
                continue;
            }
            // (2) the nodes, base first. A group base isolates exactly when
            // `composite_unit_into` isolates it: the DOCUMENT's member flags
            // (`visible`), and `baseless` for a bottom clipped entry.
            // Every style is admitted since 13A-3c — a knockout and a
            // grouped interior take the unit pass (`regional_knockout`).
            let visible_members = members.iter().any(|m| layers[m.idx].visible);
            let isolated = doc_group::is_isolated(entry, baseless, visible_members);
            let base = self.node(
                unit,
                isolated,
                None,
                group,
                nesting,
                clip,
                collect,
                enclosure,
                &mut record,
            )?;
            // 13A-4a / 13A-4b-i / 13A-4b-ii-a: a guided base with no
            // positional run (lone, or a baseless clipped entry —
            // `composite_level_into`'s `&[]`) at the ROOT, in an ISOLATED
            // group's level or directly in a PASS-THROUGH level (absorbed
            // into the enclosing level's record) is STAGED, keyed by its
            // LAYER index, and its row declares the guide's halo. Inside a
            // materialized subtree the reference renders it (no stage, the
            // halo declared). Over a non-empty run it contributes nothing
            // (the private buffer is transparent, so `adjust_backdrop_sample`
            // returns at once) and keeps its ordinary row.
            let radius = match &base.contribution {
                Contribution::Adjustment(adjustment) => adjustment.guide_radius(),
                _ => None,
            };
            if let Some(radius) = radius {
                let staged = !clip_run
                    && matches!(
                        enclosure,
                        Enclosure::Root | Enclosure::Isolated | Enclosure::PassThrough
                    );
                if staged {
                    record.guided.push(GuidedRow {
                        layer: unit.idx,
                        radius,
                        through: Vec::new(),
                        clip_unit: None,
                    });
                }
                if staged || enclosure == Enclosure::Materialized {
                    let halo = guide_geometry(radius).halo_pixels();
                    if let Some(row) = self.needs.last_mut() {
                        let own = match row.need {
                            EvaluationNeed::Spatial { halo_pixels } => halo_pixels,
                            _ => 0,
                        };
                        row.need = EvaluationNeed::Spatial {
                            halo_pixels: own.max(halo),
                        };
                    }
                }
            }
            // `composite_unit_into`'s `None => return Ok(())` precedes
            // `resolved_member`: under a base group with an empty extent the
            // reference never renders a member, so none is collected.
            let empty_base = matches!(
                base.contribution,
                Contribution::Isolated(_) | Contribution::Materialized
            ) && base.rect.is_none();
            let member_collect = collect && !empty_base;
            // Nor does it ever render a member's level: the member groups'
            // records under an empty base are dropped after the loop.
            let mark = record.children.len();
            // A hidden member is dropped; a visible zero-opacity one is a
            // clamp-only step, never inspected; a contributing one is a
            // member — always ISOLATED if it is a group (`resolved_member`).
            // A CUT member is dropped as a hidden one is (`resolved_member`
            // marks it invisible, 13A-5c-2b); the unit's structure above
            // keeps reading the document's flags.
            let mut steps = Vec::new();
            for &member in members {
                let layer = &layers[member.idx];
                if self.env.cut.excludes(member.idx) {
                    continue;
                }
                if contributes(layer) {
                    let node = self.node(
                        member,
                        true,
                        Some(unit.idx),
                        group,
                        nesting,
                        clip,
                        member_collect,
                        enclosure,
                        &mut record,
                    )?;
                    // 13A-4b-ii-b-1: a guided ADJUSTMENT member of a clip
                    // base is STAGED by this level, keyed by its LAYER
                    // index, with its unit (the base and the unit's first
                    // row) — never under an empty base group, whose unit
                    // the reference drops; since 13A-4b-ii-b-2 under a
                    // STYLED base too (the styled-direct and routed member
                    // loops), except a PACKAGED one (`Styled::of`'s
                    // `package`: planes, opacity below 1, not Dissolve —
                    // `composite_package_into` runs its members per strip),
                    // which refuses (owner 13B); inside a materialized
                    // subtree the reference renders it (no stage). Staged or
                    // materialized, its row declares the guide's halo.
                    let radius = match &node.contribution {
                        Contribution::Adjustment(adjustment) if !adjustment.is_pointwise() => {
                            adjustment.guide_radius()
                        }
                        _ => None,
                    };
                    if let Some(radius) = radius {
                        let mut declared = enclosure == Enclosure::Materialized;
                        if !declared {
                            let packaged = style_of(entry).is_some_and(|s| {
                                renders_planes(entry, s, base.rect)
                                    && crate::opacity_byte::quantized(entry.opacity) < 1.0
                                    && entry.blend != BlendMode::Dissolve
                            });
                            if packaged {
                                return Err(refusal(member.idx, &layer.name, PACKAGED_BASE));
                            }
                            if !empty_base {
                                record.guided.push(GuidedRow {
                                    layer: member.idx,
                                    radius,
                                    through: Vec::new(),
                                    clip_unit: Some(ClipUnit {
                                        base: unit.idx,
                                        start: unit.start,
                                    }),
                                });
                                declared = true;
                            }
                        }
                        if declared {
                            let halo = guide_geometry(radius).halo_pixels();
                            if let Some(row) = self.needs.last_mut() {
                                let own = match row.need {
                                    EvaluationNeed::Spatial { halo_pixels } => halo_pixels,
                                    _ => 0,
                                };
                                row.need = EvaluationNeed::Spatial {
                                    halo_pixels: own.max(halo),
                                };
                            }
                        }
                    }
                    steps.push(Step::Member(node));
                } else if layer.visible {
                    steps.push(Step::ClampOnly);
                }
            }
            if empty_base {
                record.children.truncate(mark);
            }
            units.push(Unit {
                base,
                steps,
                clip_run,
            });
            i = end;
        }
        Ok((
            Level {
                units,
                background: false,
            },
            record,
        ))
    }

    /// One contributing entry's node and plan row. A group's children level
    /// is walked first and its row pushed after theirs (ascending index); an
    /// adjustment is classified by `composite_layer_into`'s chain and its
    /// operator checked. `isolated` is the reference's isolation decision for
    /// a group (ignored for a leaf). An isolated group whose style has an
    /// enabled effect is MATERIALIZED (13A-3b-ii): its children are still
    /// walked (refusals and rows) with `collect` off, and — non-empty and
    /// collected — it is pushed for the dry run. `enclosure` is the ENCLOSING
    /// level's and `record` its record: a group child's level is walked
    /// under `Materialized` when the enclosure is (sticky) or the group
    /// materializes, else `Isolated` / `PassThrough` by `isolated`; an
    /// Isolated record is pushed on `record` with the number of guided rows
    /// recorded so far (13A-4b-i), a PassThrough one ABSORBED into it with
    /// its hold (`pass_through_holds`, 13A-4b-ii-a), a Materialized one
    /// dropped.
    #[allow(clippy::too_many_arguments)]
    fn node(
        &mut self,
        at: doc_group::Unit,
        isolated: bool,
        clip_base: Option<usize>,
        group: Option<usize>,
        nesting: u32,
        clip: Option<Rect>,
        collect: bool,
        enclosure: Enclosure,
        record: &mut LevelRecord,
    ) -> Result<Node<'a>, String> {
        self.env.job.check()?;
        let doc = self.doc;
        let index = at.idx;
        let layer = &doc.layers[index];
        let mut rect = None;
        // A materialized group's rows and preparation work inside it
        // (13A-5b-iii-b-2): the differences across its children's walk.
        let (mut rows_inside, mut inside_delta) = (0u64, 0u64);
        let (kind, operator, contribution) = if layer.kind == LayerKind::Group {
            // The reference refuses a group level past its nesting limit
            // (`rendered_group`, `composite_level_into`), in its own words.
            if layer.depth >= MAX_GROUP_DEPTH {
                return Err("rendering exceeds the group nesting limit".into());
            }
            self.group_depth = self.group_depth.max(nesting + 1);
            let materialized =
                isolated && style_of(layer).is_some_and(LayerStyle::has_enabled_effects);
            if isolated {
                // `rendered_group`'s intersection: the extent within the
                // enclosing rect, `None` when either is empty.
                rect = clip.and_then(|c| {
                    let (ex0, ey0, ex1, ey1) = doc_group::group_extent(&doc.layers, at, self.env)?;
                    let r = (ex0.max(c.0), ey0.max(c.1), ex1.min(c.2), ey1.min(c.3));
                    (r.2 > r.0 && r.3 > r.1).then_some(r)
                });
            }
            let child_enclosure = if enclosure == Enclosure::Materialized || materialized {
                Enclosure::Materialized
            } else if isolated {
                Enclosure::Isolated
            } else {
                Enclosure::PassThrough
            };
            let (rows_before, inside_before) = (self.needs.len(), self.inside_prep);
            let (children, child) = self.level(
                at.start..at.idx,
                layer.depth + 1,
                Some(index),
                nesting + 1,
                if isolated { rect } else { clip },
                collect && !materialized,
                child_enclosure,
            )?;
            rows_inside = (self.needs.len() - rows_before) as u64;
            inside_delta = self.inside_prep - inside_before;
            match child_enclosure {
                Enclosure::Isolated => record.children.push((record.guided.len(), child)),
                Enclosure::PassThrough => {
                    record.absorb(index, pass_through_holds(&doc.layers, at, layer), child);
                }
                Enclosure::Root | Enclosure::Materialized => {}
            }
            if materialized {
                if let (true, Some(_), Some(c)) = (collect, rect, clip) {
                    self.materializations.push((at, c, layer.depth));
                }
                ("group", "isolated", Contribution::Materialized)
            } else if isolated {
                ("group", "isolated", Contribution::Isolated(children))
            } else {
                ("group", "pass_through", Contribution::PassThrough(children))
            }
        } else {
            let adjustment = layer
                .meta
                .as_deref()
                .and_then(Adjustment::from_meta)
                .and_then(|op| op.in_profile(&doc.profile).ok());
            match adjustment {
                // A guide is admitted at the ROOT (13A-4a), in an isolated
                // group's level (13A-4b-i), directly in a pass-through level
                // (13A-4b-ii-a) and as a clip member of an unstyled base
                // (13A-4b-ii-b-1, checked by `level`), where the enclosing
                // level stages it, and anywhere inside a materialized
                // subtree, which the reference renders. A STYLED adjustment
                // refuses by name outside one.
                Some(_)
                    if enclosure != Enclosure::Materialized
                        && layer.renders_adjustment_style().is_some() =>
                {
                    return Err(refusal(index, &layer.name, STYLED_ADJUSTMENT));
                }
                Some(adjustment) => (
                    "adjustment",
                    adjustment.op_name(),
                    Contribution::Adjustment(adjustment),
                ),
                None => ("raster", "composite", Contribution::Raster),
            }
        };
        let operator = operator.to_string();
        let enabled_mask = layer.mask.as_ref().filter(|_| layer.mask_enabled);
        let style = style_of(layer);
        // A raster whose style renders planes reads the WHOLE layer before
        // the loop, and a non-empty materialized group runs `rendered_group`
        // over its WHOLE extent before the loop: each a `Global` row, which
        // contributes no halo (its mask's feather is subsumed by the
        // whole-canvas input).
        let planes = style
            .filter(|style| layer.kind == LayerKind::Raster && renders_planes(layer, style, None));
        let materialized = matches!(contribution, Contribution::Materialized)
            .then_some(rect)
            .flatten();
        let global = planes.is_some() || materialized.is_some();
        let mut need = EvaluationNeed::Pointwise;
        let mut mask_w = 0u64;
        if let Some(raw) = enabled_mask {
            let properties = &layer.mask_properties;
            if properties.feather > 0.0 {
                mask_w = properties.work_units(raw.dimensions());
                // The feathered plane is the mask's padded extent
                // (`derived_with_job`), never the canvas.
                self.mask_scratch = self
                    .mask_scratch
                    .saturating_add(properties.scratch_bytes(raw.dimensions()));
                let pad = properties
                    .padded_dimensions(raw.dimensions())
                    .map_or(0, |(_, _, pad)| pad);
                if !global {
                    need = EvaluationNeed::Spatial { halo_pixels: pad };
                }
            }
        }
        let mut style_w = 0u64;
        if let Some(style) = planes {
            // What the u8 renderer allocates for the planes (the typed
            // plan's own `style_scratch`, which `rendered` spends), and the
            // work it counts (13A-5b-iii-b-2).
            let (scratch, work) = style_scratch(style, layer.pixels.dimensions(), false, None)?;
            self.style_scratch = self.style_scratch.saturating_add(scratch);
            style_w = work;
            need = EvaluationNeed::Global {
                analysis: STYLE_PLANES_ANALYSIS,
            };
            if !self.global.contains(&STYLE_PLANES_ANALYSIS) {
                self.global.push(STYLE_PLANES_ANALYSIS);
            }
        }
        let mut mat_w = 0u64;
        if let Some(r) = materialized {
            // Its scratch is the dry run's (`regional_admission`).
            need = EvaluationNeed::Global {
                analysis: MATERIALIZATION_ANALYSIS,
            };
            if !self.global.contains(&MATERIALIZATION_ANALYSIS) {
                self.global.push(MATERIALIZATION_ANALYSIS);
            }
            // The synthetic's planes, when the reference renders them (a
            // plane `renders_planes` admits never refuses here; the work term
            // adds no refusal of its own).
            let dims = ((r.2 - r.0) as u32, (r.3 - r.1) as u32);
            style_w = style
                .filter(|style| renders_planes(layer, style, Some(r)))
                .and_then(|style| style_scratch(style, dims, false, None).ok())
                .map_or(0, |(_, work)| work);
            // `rendered_group` over the rect: its buffer, one pass per row
            // inside, the subtree's mask and plane work and the group's own
            // mask blur (derived inside it, `Node::resolve`).
            let pixels = ((r.2 - r.0) as u64).saturating_mul((r.3 - r.1) as u64);
            mat_w = pixels
                .saturating_mul(1 + rows_inside)
                .saturating_add(inside_delta)
                .saturating_add(mask_w);
        }
        // 13A-5b-iii-b-2: the node's preparation work. Inside a materialized
        // subtree the chain spends it (the enclosing group's term); a node the
        // render resolves counts it; any other node is never resolved.
        let prep = if enclosure == Enclosure::Materialized {
            self.inside_prep = self
                .inside_prep
                .saturating_add(mask_w.saturating_add(style_w));
            PrepWork::default()
        } else {
            let prep = PrepWork {
                mask: if matches!(contribution, Contribution::Materialized) {
                    0
                } else {
                    mask_w
                },
                style: style_w,
                materialization: mat_w,
            };
            if collect && clip.is_some() {
                self.mask_work = self.mask_work.saturating_add(prep.mask);
                self.style_work = self.style_work.saturating_add(prep.style);
                self.materialization_work = self
                    .materialization_work
                    .saturating_add(prep.materialization);
            }
            prep
        };
        self.needs.push(LayerNeed {
            index,
            name: layer.name.clone(),
            kind,
            operator,
            need,
            clip_base,
            group,
            mask: enabled_mask.is_some(),
            blend_if: style
                .and_then(|style| style.blend_if)
                .is_some_and(|b| !b.is_identity()),
            style: style.map(|style| {
                let effects = planes.is_some()
                    || matches!(contribution, Contribution::Materialized)
                        && renders_planes(layer, style, rect);
                if effects {
                    "effects"
                } else {
                    "fill"
                }
            }),
        });
        Ok(Node {
            layer,
            at,
            contribution,
            rect,
            prep,
        })
    }
}
