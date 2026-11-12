//! The 8-bit regional compositor's TREE (Phase 13A-3a): the levels, units and
//! nodes `doc_regional`'s admission walk builds, their one-time resolution
//! (each enabled mask's derived plane, each admitted style — `Styled::of`,
//! 13A-3b-i — and each isolated group's extent) and the per-node sample
//! statements. One sample of a level (`doc_group::composite_level_into`) and
//! of a unit lives in `doc_regional_sample`, the styled per-sample bodies in
//! `doc_regional_style`, the knockout unit pass in `regional_knockout`; an
//! isolated group whose level holds a staged guide is replaced by its held
//! plane (`Source::Staged`, 13A-4b-i, `doc_regional_guide::stage_nested`); the
//! staged driver reaches a guided unit through pass-through children by a
//! cursor (`path_of_layer`, `guided_paths`, 13A-4b-ii-a) and holds the sample
//! as such a group's children began where it is read
//! (`pass_through_holds`, `ResolvedNode::snapshot`). Since
//! 13A-3c every sample carries the reference's own
//! `style_knockout::KnockoutBackdrop` over one-element slices, born where the
//! reference births it: D5's Background split at the ROOT (`Level::
//! background`, `composite_projection_into`'s test), a pass-through level's
//! Shallow plane (the sample as the children began, as the typed arm always
//! hands it), an isolated buffer's hard stop (`NONE`) and a clip unit's
//! base-only sample. It holds NO admission logic (the walk decides every
//! refusal) and NO pixel math of its own: every statement is index
//! arithmetic around the reference's per-pixel bodies —
//! `doc::composite_layer_sample`, `doc::adjust_backdrop_sample`,
//! `blend::composite_source_into`, `blend::gate_source_into`,
//! `doc_evaluation::encode_sample` and `Layer::effective_coverage_at`. The
//! contract is `regional`'s module doc.

use std::sync::Arc;

use image::GrayImage;

use crate::adjust::Adjustment;
use crate::blend::{blend_kind, BlendMode};
use crate::doc::{adjust_backdrop_sample, composite_layer_sample, Layer};
use crate::doc_evaluation::{encode_sample, ByteTransfer};
use crate::doc_group;
use crate::doc_group::RenderBudget;
use crate::doc_regional_progress::{PrepWork, Run};
use crate::doc_regional_style::{resolve_materialized, Styled};
use crate::job_control::JobContext;
use crate::regional_guide::{PreparedGuide, StagedPlane};
use crate::style_composite::CompositeEnv;
use crate::style_knockout::KnockoutBackdrop;
use crate::style_render::RenderedEffects;

/// A canvas rect `(x0, y0, x1, y1)`, exclusive — the enclosing accumulator a
/// level composites into, as `rendered_group`'s `clip`.
pub(crate) type Rect = (i64, i64, i64, i64);

/// `composite_unit_into`'s reasons the sample as a PASS-THROUGH group's
/// children began is READ (13A-4b-ii-a): the gate — an opacity below 1 or an
/// enabled mask (`doc_group::composite_unit_into`'s pass-through branch;
/// the regional sampler's own test) — whose FINISH lerps back to it, or a
/// visible Shallow child (`style_knockout::level_wants_shallow` over the
/// group's own level), which punches to it.
pub(crate) fn pass_through_holds(layers: &[Layer], at: doc_group::Unit, layer: &Layer) -> bool {
    crate::opacity_byte::quantized(layer.opacity) < 1.0
        || (layer.mask_enabled && layer.mask.is_some())
        || crate::style_knockout::level_wants_shallow(
            layers,
            &doc_group::units(layers, at.start..at.idx, layer.depth.saturating_add(1)),
        )
}

/// One contributing entry, classified exactly as the reference classifies it:
/// `composite_layer_into`'s adjustment test for a leaf, `is_isolated` (or the
/// member rule) for a group.
pub(crate) enum Contribution<'a> {
    Raster,
    Adjustment(Adjustment),
    /// Rendered into a private buffer and composited as a synthetic raster
    /// (`rendered_group`).
    Isolated(Level<'a>),
    /// Composited straight (or gated) onto the enclosing accumulator
    /// (`composite_unit_into`'s pass-through branch).
    PassThrough(Level<'a>),
    /// An isolated group whose style has an enabled effect (13A-3b-ii):
    /// MATERIALIZED before the loop by `doc_group::rendered_group` itself
    /// (polling the render's job since 13A-5b-ii-a) and composited as that
    /// synthetic STYLED raster. Its children level is
    /// walked for the refusals and rows only; the reference renders it.
    Materialized,
}

/// One contributing entry: its layer, its subtree (`doc_group::Unit`, which
/// `group_extent` reads) and its classification.
pub(crate) struct Node<'a> {
    pub(crate) layer: &'a Layer,
    pub(crate) at: doc_group::Unit,
    pub(crate) contribution: Contribution<'a>,
    /// extent ∩ clip for an Isolated/Materialized group with a non-empty
    /// result (the walk's one computation — `doc_group::group_extent` within
    /// the enclosing rect, `rendered_group`'s intersection); None otherwise.
    pub(crate) rect: Option<Rect>,
    /// The preparation work the walk counted for this node (13A-5b-iii-b-2).
    pub(crate) prep: PrepWork,
}

/// One step of a clip run's member loop — the reference's
/// `group.iter().filter(|l| l.visible)` (`doc.rs:593`, `style_composite.rs`):
/// a contributing member, or a visible zero-opacity member, for which
/// `composite_layer_into` returns at once but the post-member alpha statement
/// still runs. Hidden members are dropped.
pub(crate) enum Step<'a> {
    Member(Node<'a>),
    ClampOnly,
}

/// One unit of a level. `clip_run` is POSITIONAL — the reference's run,
/// hidden members included (`composite_clip_group_into`'s `group.is_empty()`);
/// `steps` are the run's VISIBLE members in order. `steps` is EMPTY whenever
/// `base.contribution` is `PassThrough` (a visible member would have forced
/// isolation), and the sampler never reads `clip_run` for such a base.
pub(crate) struct Unit<'a> {
    pub(crate) base: Node<'a>,
    pub(crate) steps: Vec<Step<'a>>,
    pub(crate) clip_run: bool,
}

/// One level's units, bottom-first.
pub(crate) struct Level<'a> {
    pub(crate) units: Vec<Unit<'a>>,
    /// D5 (`style_knockout::composite_projection_into`'s test): the ROOT
    /// level's entry 0 is the BACKGROUND and some visible style asks for a
    /// knockout, so the first unit samples with no backdrop and every later
    /// one against the sample right after it. False at every other level.
    pub(crate) background: bool,
}

/// A node's contribution, resolved.
pub(crate) enum Source<'r> {
    Raster,
    Adjustment(&'r Adjustment),
    /// The synthetic raster's canvas rect (the group's extent within the
    /// enclosing accumulator — never empty) and the children level.
    Isolated(Rect, ResolvedLevel<'r>),
    PassThrough(ResolvedLevel<'r>),
    /// `rendered_group`'s synthetic raster (owned; boxed, it is large).
    Materialized(Box<Layer>),
    /// An ISOLATED group whose level holds a staged guide (13A-4b-i): the
    /// same rect, the children level replaced by its held final plane
    /// (`doc_regional_guide::stage_nested`), read at every sample inside it.
    Staged(Rect, StagedPlane),
}

/// A node ready for the sample loop: its layer, its source, its enabled
/// mask's derived plane and pad, its admitted style and the blend mode it
/// composites with.
pub(crate) struct ResolvedNode<'r> {
    pub(crate) layer: &'r Layer,
    /// The entry's LAYER index (`Node.at.idx`): a staged level's `level`.
    pub(crate) index: usize,
    pub(crate) source: Source<'r>,
    pub(crate) mask: Option<(Arc<GrayImage>, u32)>,
    pub(crate) style: Option<Styled>,
    pub(crate) blend: BlendMode,
    /// A STYLED node clamps every visible member to its SHAPE — its planes'
    /// shape, else a pad-0 `Shape::of_layer`, which exists unless the
    /// (synthetic) layer exceeds `MAX_PIXELS`; otherwise the alpha is forced
    /// back to the base's own. Read only when the node is a unit base.
    pub(crate) shaped: bool,
    /// A guided ROOT adjustment's staged guide (13A-4a, set by the staged
    /// driver through `ResolvedLevel::set_guide`); `None` everywhere else.
    pub(crate) guide: Option<PreparedGuide>,
    /// A PASS-THROUGH node whose sample as its children began is READ
    /// ([`pass_through_holds`], 13A-4b-ii-a): the staged driver HOLDS it as
    /// a plane when the node lies on a stage's path. False unless
    /// PassThrough.
    pub(crate) snapshot: bool,
}

// A member step is a whole node, built once per render and never moved in a
// loop; boxing it would add an indirection to every member sample for no gain.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ResolvedStep<'r> {
    Member(ResolvedNode<'r>),
    ClampOnly,
}

pub(crate) struct ResolvedUnit<'r> {
    pub(crate) base: ResolvedNode<'r>,
    pub(crate) steps: Vec<ResolvedStep<'r>>,
    pub(crate) clip_run: bool,
    /// The byte opacity of the base (`opacity_byte::quantized`).
    pub(crate) opacity: f32,
}

/// A level resolved once, before the loop.
pub(crate) struct ResolvedLevel<'r> {
    pub(crate) units: Vec<ResolvedUnit<'r>>,
    /// `Level::background`, carried.
    pub(crate) background: bool,
    /// Each kept unit's base LAYER index (`Node.at.idx`), beside `units`:
    /// the one map from the plan's layer indices to RESOLVED positions
    /// (`path_of_layer`).
    pub(crate) layer_units: Vec<usize>,
}

impl<'a> Level<'a> {
    /// The derived masks, the admitted styles, each isolated group's extent
    /// (the walk's `Node::rect`) and each materialized group's synthetic
    /// raster (`rendered_group`, charged against `budget`), resolved once. A
    /// base group whose extent is empty drops its whole unit
    /// (`composite_unit_into`'s `None => return`); a member group whose
    /// extent is empty is dropped from the loop (`resolved_member` keeps it
    /// only as an INVISIBLE placeholder). Each mask's feather blur (13A-5a),
    /// each style's plane render (`Styled::of`, 13A-5b-i) and each
    /// `rendered_group` (13A-5b-ii-a, through the env's job) polls `job` —
    /// since 13A-5b-iii-b-2 inside its own `run.range`, then advancing `run`
    /// by its node's `prep` work.
    pub(crate) fn resolve(
        &self,
        layers: &'a [Layer],
        env: CompositeEnv<'_>,
        clip: Rect,
        budget: &RenderBudget,
        job: JobContext<'_>,
        run: &mut Run<'_>,
    ) -> Result<ResolvedLevel<'_>, String> {
        let mut units = Vec::with_capacity(self.units.len());
        let mut layer_units = Vec::with_capacity(self.units.len());
        for unit in &self.units {
            let Some(base) = unit.base.resolve(layers, env, clip, budget, job, run)? else {
                continue;
            };
            let mut steps = Vec::with_capacity(unit.steps.len());
            for step in &unit.steps {
                match step {
                    Step::Member(node) => {
                        if let Some(member) = node.resolve(layers, env, clip, budget, job, run)? {
                            steps.push(ResolvedStep::Member(member));
                        }
                    }
                    Step::ClampOnly => steps.push(ResolvedStep::ClampOnly),
                }
            }
            layer_units.push(unit.base.at.idx);
            units.push(ResolvedUnit {
                opacity: crate::opacity_byte::quantized(base.layer().opacity),
                base,
                steps,
                clip_run: unit.clip_run,
            });
        }
        Ok(ResolvedLevel {
            units,
            background: self.background,
            layer_units,
        })
    }
}

impl<'a> Node<'a> {
    fn resolve(
        &self,
        layers: &'a [Layer],
        env: CompositeEnv<'_>,
        clip: Rect,
        budget: &RenderBudget,
        job: JobContext<'_>,
        run: &mut Run<'_>,
    ) -> Result<Option<ResolvedNode<'_>>, String> {
        // A materialized group derives its own mask inside `rendered_group`
        // (under its materialization range), so it is dispatched first.
        if let Contribution::Materialized = &self.contribution {
            return resolve_materialized(self, layers, env, clip, budget, job, run);
        }
        let layer = self.layer;
        // `active_mask`'s rule: an ENABLED mask only. `derived_with_job` is
        // the reference's own preparation stage (`effective_mask_rect`) —
        // the same bytes, cached exactly as the full render caches it (only
        // once complete) — its feather blur reporting inside its range
        // (13A-5b-iii-b-2: `prep.mask`, then the advance).
        let mask = layer
            .mask
            .as_ref()
            .filter(|_| layer.mask_enabled)
            .map(|raw| {
                layer
                    .mask_properties
                    .derived_with_job(raw, run.range(job, self.prep.mask))
            })
            .transpose()?;
        run.advance(job, self.prep.mask)?;
        let (source, style, blend) = match &self.contribution {
            Contribution::Raster => (
                Source::Raster,
                layer
                    .renders_style()
                    .map(|s| Styled::of(layer, s, env, clip, job, run, self.prep.style))
                    .transpose()?,
                layer.blend,
            ),
            Contribution::Adjustment(adjustment) => {
                (Source::Adjustment(adjustment), None, layer.blend)
            }
            Contribution::Isolated(level) => {
                let Some(rect) = self.rect else {
                    return Ok(None);
                };
                (
                    Source::Isolated(rect, level.resolve(layers, env, rect, budget, job, run)?),
                    // A group's style is the one its synthetic layer carries
                    // (`Layer::style` — the synthetic has no meta, so it
                    // always renders); with an enabled effect the group is
                    // `Materialized` instead (the walk).
                    layer
                        .style
                        .as_deref()
                        .map(|s| Styled::of(layer, s, env, clip, job, run, self.prep.style))
                        .transpose()?,
                    // Reachable only when something else forced isolation,
                    // and then the group composites as an ordinary Normal
                    // layer (`rendered_group`).
                    if layer.blend.is_group_only() {
                        BlendMode::Normal
                    } else {
                        layer.blend
                    },
                )
            }
            Contribution::PassThrough(level) => (
                Source::PassThrough(level.resolve(layers, env, clip, budget, job, run)?),
                None,
                layer.blend,
            ),
            // Dispatched above, before the mask derivation.
            Contribution::Materialized => {
                return resolve_materialized(self, layers, env, clip, budget, job, run);
            }
        };
        let snapshot = matches!(self.contribution, Contribution::PassThrough(_))
            && pass_through_holds(layers, self.at, layer);
        let mut node = ResolvedNode {
            layer,
            index: self.at.idx,
            source,
            mask,
            style,
            blend,
            shaped: false,
            guide: None,
            snapshot,
        };
        node.shaped = node.is_shaped();
        Ok(Some(node))
    }
}

impl ResolvedUnit<'_> {
    /// A GUIDED lone unit: an adjustment base with a guide radius and no
    /// positional run — the walk's staged predicate at an admitted level.
    pub(crate) fn guided(&self) -> bool {
        !self.clip_run
            && matches!(&self.base.source, Source::Adjustment(adjustment)
                if adjustment.guide_radius().is_some())
    }
}

impl ResolvedNode<'_> {
    /// The layer every pixel, mask, offset and opacity read goes through:
    /// a materialized group's synthetic raster, else the entry itself.
    pub(crate) fn layer(&self) -> &Layer {
        match &self.source {
            Source::Materialized(synthetic) => synthetic,
            _ => self.layer,
        }
    }

    /// The planes an effect-bearing style rendered, if any.
    pub(crate) fn planes(&self) -> Option<&RenderedEffects> {
        self.style.as_ref().and_then(|s| s.planes.as_deref())
    }

    /// Whether the style takes the knockout UNIT PASS (13A-3c): the
    /// reference's `routes_unit`, decided once in `Styled::of`.
    pub(crate) fn routed(&self) -> bool {
        self.style.as_ref().is_some_and(|s| s.routed)
    }

    /// The enabled mask's effective coverage byte at `canvas_xy`
    /// (`effective_coverage_at`), or `None` without one.
    pub(crate) fn mask_byte(&self, canvas_xy: (i64, i64)) -> Option<u8> {
        self.mask.as_ref().map(|mask| {
            self.layer()
                .effective_coverage_at(&mask.0, mask.1, canvas_xy.0, canvas_xy.1)
        })
    }

    /// The RGBA8 raster sample this node composites at `canvas_xy`: a
    /// raster's own stored pixel, or an isolated group's synthetic pixel —
    /// its children evaluated on a transparent sample and encoded as
    /// `rendered_group` encodes its buffer. `None` outside the (synthetic)
    /// layer's rect, where the reference leaves the target untouched, and for
    /// an adjustment or pass-through node, which has no pixels.
    pub(crate) fn pixel(
        &self,
        transfer: Option<&ByteTransfer>,
        canvas_xy: (i64, i64),
    ) -> Option<[u8; 4]> {
        match &self.source {
            Source::Raster | Source::Materialized(_) => {
                let layer = self.layer();
                let (lw, lh) = layer.pixels.dimensions();
                let lx = canvas_xy.0 - i64::from(layer.offset.0);
                let ly = canvas_xy.1 - i64::from(layer.offset.1);
                if lx < 0 || ly < 0 || lx >= i64::from(lw) || ly >= i64::from(lh) {
                    return None;
                }
                layer.pixels.sample_at(lx as u32, ly as u32)
            }
            Source::Isolated(rect, level) => {
                let (x, y) = canvas_xy;
                if x < rect.0 || y < rect.1 || x >= rect.2 || y >= rect.3 {
                    return None;
                }
                // D4: an isolated buffer is a HARD STOP for both modes.
                let mut g = [[0.0f32; 4]];
                level.sample(&mut g, 0, transfer, canvas_xy, KnockoutBackdrop::NONE);
                Some(encode_sample(g[0], transfer))
            }
            Source::Staged(rect, staged) => {
                let (x, y) = canvas_xy;
                if x < rect.0 || y < rect.1 || x >= rect.2 || y >= rect.3 {
                    return None;
                }
                // The children's sample, held: the staged level's final plane.
                Some(encode_sample(staged.at(canvas_xy), transfer))
            }
            Source::Adjustment(_) | Source::PassThrough(_) => None,
        }
    }

    /// One sample of this node composited onto `target[ti]` with `opacity`
    /// and `mode`: `composite_layer_into`'s raster loop body over `pixel`,
    /// `composite_pixels_into`'s (a styled raster or group, whose Blend If
    /// reads `under`), `composite_styled_into`'s (a styled raster with
    /// planes, or a routed style against `knockout` — 13A-3c) or
    /// `composite_adjustment_into`'s, index arithmetic only.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite(
        &self,
        target: &mut [[f32; 4]],
        ti: usize,
        under: [f32; 4],
        opacity: f32,
        mode: BlendMode,
        pixel: Option<[u8; 4]>,
        knockout: KnockoutBackdrop<'_>,
        transfer: Option<&ByteTransfer>,
        canvas_xy: (i64, i64),
    ) {
        let kind = blend_kind(mode);
        match &self.source {
            Source::Raster
            | Source::Isolated(..)
            | Source::Staged(..)
            | Source::Materialized(_) => {
                if self.planes().is_some() || self.routed() {
                    // A styled MEMBER with planes or a routed style (a lone
                    // or base node is routed by `ResolvedUnit::sample`): its
                    // whole styled composite onto the unit buffer.
                    self.styled_sample(
                        target, ti, under, opacity, pixel, None, knockout, transfer, canvas_xy,
                        None,
                    );
                    return;
                }
                if self.style.is_some() {
                    self.pixel_pass(target, ti, under, opacity, mode, pixel, transfer, canvas_xy);
                    return;
                }
                let Some(px) = pixel else {
                    return; // outside the layer's extent: untouched
                };
                // The aligned mask covers exactly the (synthetic) layer's
                // extent (`aligned_mask`; a group's is `effective_mask_rect`
                // cropped to it); inside it a sample is
                // `effective_coverage_at`.
                let coverage = self
                    .mask_byte(canvas_xy)
                    .map_or(1.0, |m| f32::from(m) / 255.0);
                composite_layer_sample(
                    target, ti, &px, 0, coverage, opacity, kind, transfer, canvas_xy,
                );
            }
            Source::Adjustment(adjustment) => {
                let coverage = match &self.mask {
                    None => 1.0,
                    Some(_) => {
                        let layer = self.layer;
                        let (lw, lh) = layer.pixels.dimensions();
                        let lx = canvas_xy.0 - i64::from(layer.offset.0);
                        let ly = canvas_xy.1 - i64::from(layer.offset.1);
                        if lx < 0 || ly < 0 || lx >= i64::from(lw) || ly >= i64::from(lh) {
                            return; // outside the mask's extent: coverage 0
                        }
                        self.mask_byte(canvas_xy)
                            .map_or(1.0, |m| f32::from(m) / 255.0)
                    }
                };
                let k = coverage * opacity;
                if k <= 0.0 {
                    return;
                }
                // A guided ROOT adjustment reads its staged guide (13A-4a,
                // `composite_adjustment_into`'s `plane.at`); every other
                // adjustment is pointwise and its 0.0 is never read.
                let guide = self.guide.as_ref().map_or(0.0, |g| g.at(canvas_xy));
                adjust_backdrop_sample(
                    &mut target[ti],
                    adjustment,
                    k,
                    guide,
                    kind,
                    transfer,
                    canvas_xy,
                );
            }
            // A pass-through node is only ever a unit base, sampled by
            // `ResolvedUnit::sample` before this is reached.
            Source::PassThrough(_) => {}
        }
    }
}
