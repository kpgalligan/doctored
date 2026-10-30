//! The 16-bit and float regional compositor's ADMISSION WALK (moved out of
//! `precision_regional` in 13A-4b-ii-a, verbatim): the fixed-order walk over
//! the record TREE — every level's units formed exactly as
//! `precision_document::render_layers` forms them, recursing into each
//! contributing group — that builds the plan rows, the materialized
//! subtrees' mirror of `PrecisionScene::plan`'s `collect` and the
//! `LevelRecord`s `regional_guide::plan_levels` reads. The contract is
//! `regional`'s module doc. No pixel math.

use crate::blend::BlendMode;
use crate::precision_document::{Content, PrecisionDocumentLayer};
use crate::precision_render::intersection;
use crate::precision_style_knockout::knockout_scratch;
use crate::precision_style_render::style_scratch;
use crate::regional::{EvaluationNeed, LayerNeed, MATERIALIZATION_ANALYSIS, STYLE_PLANES_ANALYSIS};
use crate::regional_guide::{guide_geometry, ClipUnit, Enclosure, GuidedRow, LevelRecord};
use crate::regional_plan::PixelRegion;
use crate::style::Knockout;

/// The admission walk's state: the plan rows and the running scratch, work,
/// halo and nesting figures.
#[derive(Default)]
pub(crate) struct Walk {
    pub(crate) needs: Vec<LayerNeed>,
    pub(crate) mask_scratch: u64,
    pub(crate) mask_work: u64,
    pub(crate) style_scratch: u64,
    pub(crate) style_work: u64,
    /// Every materialized group's retained buffer, the transient buffers
    /// `render_layers` allocates inside it and the planes (13A-3b-ii).
    pub(crate) materialization_scratch: u64,
    /// The work `render_layers` and the planes advance for them.
    pub(crate) materialization_work: u64,
    pub(crate) group_depth: u32,
    /// The distinct `Global` analyses, in row order.
    pub(crate) global: Vec<&'static str>,
    pub(crate) canvas: (u32, u32),
}

/// One materialized subtree's mirror of `PrecisionScene::plan`'s `collect`
/// (`precision_render.rs`): the deepest nesting of buffers below the
/// materialized group (a contributing isolated or pass-through group and a
/// clip unit with a visible member each hold one canvas plane — the
/// reference's `isolated_normal` / `with_clipped_members` `group_depth`),
/// the deepest count of live clip levels (each holds its base's alpha
/// record), and the style scratch — with each row's D8 knockout term
/// (`knockout_scratch`, 13A-3c) — and work summed over every row. Sums, not
/// the reference's peaks: conservative. `below` / `clips` are the walk's
/// cursors.
#[derive(Default)]
pub(crate) struct Subtree {
    depth: u32,
    clip_levels: u32,
    style_scratch: u64,
    work: u64,
    below: u32,
    clips: u32,
}

impl Walk {
    /// One LEVEL of records — `layers[0]`'s first record at postorder index
    /// `start` — in the fixed refusal order (`regional`'s module doc), over
    /// the units the scene builder forms: (1) the unit skip (`contributes` is
    /// `visible`), (2) every contributing record's row — a group's children
    /// first, an adjustment's operator — in order. `materialized`
    /// is the enclosing materialized group's `Subtree`, updated as
    /// `PrecisionScene::plan`'s `collect` charges; `enclosure` is where the
    /// level sits (13A-4b-i — `Materialized` exactly when `materialized` is
    /// `Some`). The returned record carries the level's staged guided rows
    /// and its Isolated / PassThrough child levels for `plan_levels`, its
    /// `buffer` the canvas (every typed accumulator is canvas-sized).
    pub(crate) fn level(
        &mut self,
        layers: &[PrecisionDocumentLayer],
        start: usize,
        group: Option<usize>,
        nesting: u32,
        mut materialized: Option<&mut Subtree>,
        enclosure: Enclosure,
    ) -> Result<LevelRecord, String> {
        // The postorder index of each record's FIRST record, as
        // `precision_document::render_layers` numbers them.
        let mut starts = Vec::with_capacity(layers.len());
        let mut offset = start;
        for layer in layers {
            starts.push(offset);
            offset += layer.records;
        }
        let mut record = LevelRecord {
            group,
            range: start..offset,
            buffer: Some(PixelRegion {
                x: 0,
                y: 0,
                width: self.canvas.0,
                height: self.canvas.1,
            }),
            guided: Vec::new(),
            children: Vec::new(),
        };
        let index_of = |i: usize| starts[i] + layers[i].records - 1;
        let mut i = 0;
        while i < layers.len() {
            // The unit the scene builder forms at `i`: a baseless clipped
            // record alone, else the base and the clipped run above it.
            let baseless = layers[i].clipped;
            let end = if baseless {
                i + 1
            } else {
                let mut end = i + 1;
                while end < layers.len() && layers[end].clipped {
                    end += 1;
                }
                end
            };
            // (1) the unit skip: a hidden base drops its whole unit (and
            // every subtree in it) unchecked; hidden members are dropped.
            if !layers[i].visible {
                i = end;
                continue;
            }
            let rows: Vec<usize> = std::iter::once(i)
                .chain((i + 1..end).filter(|&j| layers[j].visible))
                .collect();
            // (2) the rows. A Pass Through base is forced isolated by a
            // visible member, a member always (the builder's `isolated`).
            // Every style is admitted since 13A-3c — a knockout and a grouped
            // interior take the unit pass (`regional_knockout`).
            let isolated = !baseless && rows.len() > 1;
            // D8's clip-unit snapshot question
            // (`members_want_base_snapshot16` over the visible run).
            let knockout_member = rows[1..].iter().any(|&m| {
                layers[m]
                    .style
                    .as_deref()
                    .is_some_and(|s| s.knockout != Knockout::None)
            });
            let canvas = u64::from(self.canvas.0) * u64::from(self.canvas.1);
            if isolated {
                // `collect`'s clip unit: one buffer level, one live clip
                // level for its members, three passes over the canvas.
                if let Some(sub) = materialized.as_deref_mut() {
                    sub.work = sub.work.saturating_add(canvas * 3);
                    sub.below += 1;
                    sub.depth = sub.depth.max(sub.below);
                    sub.clip_levels = sub.clip_levels.max(sub.clips + 1);
                }
            }
            for &j in &rows {
                let forced = if j == i { isolated } else { true };
                let clip_base = (j != i).then(|| index_of(i));
                let member = isolated && j != i;
                if let Some(sub) = materialized.as_deref_mut().filter(|_| member) {
                    sub.clips += 1;
                }
                self.row(
                    &layers[j],
                    starts[j],
                    index_of(j),
                    forced,
                    clip_base,
                    group,
                    nesting,
                    materialized.as_deref_mut(),
                    j == i && knockout_member,
                    enclosure,
                    &mut record,
                )?;
                if let Some(sub) = materialized.as_deref_mut().filter(|_| member) {
                    sub.clips -= 1;
                }
                // 13A-4a / 13A-4b-i / 13A-4b-ii-a: a guided base with no
                // visible member (lone, the typed lone arm — a hidden-only
                // run included — or a baseless clipped record) at the ROOT,
                // in an ISOLATED group's level or directly in a PASS-THROUGH
                // level (absorbed into the enclosing level's record) is
                // STAGED, keyed by its record index, and its row declares the
                // guide's halo. Inside a materialized subtree the reference
                // renders it (no stage, the halo declared). Over a visible
                // run it contributes nothing (the clip buffer is transparent)
                // and keeps its ordinary row.
                let radius = match &layers[j].content {
                    Content::Adjustment(state) if j == i => state.kernel.guide_radius(),
                    _ => None,
                };
                let staged = rows.len() == 1
                    && matches!(
                        enclosure,
                        Enclosure::Root | Enclosure::Isolated | Enclosure::PassThrough
                    );
                if let Some(radius) = radius {
                    if staged {
                        record.guided.push(GuidedRow {
                            layer: index_of(j),
                            radius,
                            through: Vec::new(),
                            clip_unit: None,
                        });
                    }
                }
                // 13A-4b-ii-b-1: a guided ADJUSTMENT member (a zero-opacity
                // one included: the scene keeps it, the render skips it) of
                // a clip base is STAGED by this level, keyed by its record
                // index, with its unit (the base and the unit's first
                // record) — since 13A-4b-ii-b-2 under a STYLED base too
                // (direct, routed or packaged: the typed package is one
                // canvas-sized copy); inside a materialized subtree the
                // reference renders it (no stage). Either way its row
                // declares the guide's halo.
                let member = match &layers[j].content {
                    Content::Adjustment(state) if j != i && !state.kernel.is_pointwise() => {
                        state.kernel.guide_radius()
                    }
                    _ => None,
                };
                if let Some(radius) = member.filter(|_| enclosure != Enclosure::Materialized) {
                    record.guided.push(GuidedRow {
                        layer: index_of(j),
                        radius,
                        through: Vec::new(),
                        clip_unit: Some(ClipUnit {
                            base: index_of(i),
                            start: starts[i],
                        }),
                    });
                }
                if let Some(radius) = radius
                    .filter(|_| staged || enclosure == Enclosure::Materialized)
                    .or(member)
                {
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
            if let Some(sub) = materialized.as_deref_mut().filter(|_| isolated) {
                sub.below -= 1;
            }
            i = end;
        }
        Ok(record)
    }

    /// One contributing record's row, after its children's. A `Group` whose
    /// style has an enabled effect is MATERIALIZED (13A-3b-ii) — rendered by
    /// `render_layers` into a canvas-sized buffer before the loop — unless it
    /// already sits inside one (`materialized`), where it is one more level
    /// of `collect`'s nest. `knockout_member` is the base's D8 clip-unit
    /// question (false on a member), which `collect`'s knockout term reads. `enclosure` / `record` are the
    /// ENCLOSING level's: a group's children level is walked under
    /// `Materialized` when the enclosure is (sticky) or the group
    /// materializes, else `Isolated` (`forced` or a `Group`) or
    /// `PassThrough`; an Isolated record is pushed on `record` with the
    /// number of guided rows recorded so far (13A-4b-i), a PassThrough one
    /// ABSORBED into it (always held, 13A-4b-ii-a), a Materialized one
    /// dropped. A pass-through clip base with a visible member (or a member)
    /// is `forced` → Isolated, as the scene builder renders it
    /// (`isolated_normal`, `Kind::Group`).
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        layer: &PrecisionDocumentLayer,
        first: usize,
        index: usize,
        forced: bool,
        clip_base: Option<usize>,
        group: Option<usize>,
        nesting: u32,
        mut materialized: Option<&mut Subtree>,
        knockout_member: bool,
        enclosure: Enclosure,
        record: &mut LevelRecord,
    ) -> Result<(), String> {
        let canvas = u64::from(self.canvas.0) * u64::from(self.canvas.1);
        let effects = layer
            .style
            .as_deref()
            .filter(|style| style.has_enabled_effects());
        let mut root: Option<Subtree> = None;
        let (kind, operator) = match &layer.content {
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                return Err(crate::cmyk_document::refused_sentence(
                    "Regional compositing",
                ))
            }
            // A guide is admitted at the ROOT (13A-4a), in an isolated
            // group's level (13A-4b-i), directly in a pass-through level
            // (13A-4b-ii-a) and as a clip member of an unstyled base
            // (13A-4b-ii-b-1, checked by `level`), where the enclosing level
            // stages it, and anywhere inside a materialized subtree, which
            // the reference renders.
            // A STYLED adjustment (a style or a Blend If) refuses by
            // name outside a materialized subtree.
            Content::Adjustment(_)
                if enclosure != Enclosure::Materialized
                    && (layer.style.is_some() || layer.blend_if.is_some()) =>
            {
                return Err(crate::regional::refusal(
                    index,
                    layer.name(),
                    crate::regional::STYLED_ADJUSTMENT,
                ));
            }
            Content::Adjustment(state) => ("adjustment", state.kernel.operation().to_string()),
            Content::Raster { .. } => ("raster", "composite".to_string()),
            Content::Group(children) | Content::PassThrough(children) => {
                self.group_depth = self.group_depth.max(nesting + 1);
                let child_enclosure = if enclosure == Enclosure::Materialized
                    || effects.is_some() && layer.isolates()
                {
                    Enclosure::Materialized
                } else if forced || layer.isolates() {
                    Enclosure::Isolated
                } else {
                    Enclosure::PassThrough
                };
                let (child, e) = (Some(index), child_enclosure);
                let child = match materialized.as_deref_mut() {
                    Some(sub) => {
                        // `collect`'s group arm: one buffer level, two passes.
                        sub.below += 1;
                        sub.depth = sub.depth.max(sub.below);
                        let child =
                            self.level(children, first, child, nesting + 1, Some(&mut *sub), e)?;
                        sub.below -= 1;
                        sub.work = sub.work.saturating_add(canvas * 2);
                        child
                    }
                    None if effects.is_some() && layer.isolates() => {
                        let mut sub = Subtree::default();
                        let child =
                            self.level(children, first, child, nesting + 1, Some(&mut sub), e)?;
                        root = Some(sub);
                        child
                    }
                    None => self.level(children, first, child, nesting + 1, None, e)?,
                };
                // A typed pass-through always copies (`gate_pass_through_
                // sample16` over a canvas-sized copy), so its hold is always
                // read (13A-4b-ii-a).
                match child_enclosure {
                    Enclosure::Isolated => record.children.push((record.guided.len(), child)),
                    Enclosure::PassThrough => record.absorb(index, true, child),
                    Enclosure::Root | Enclosure::Materialized => {}
                }
                let isolated = forced || layer.isolates();
                (
                    "group",
                    if isolated { "isolated" } else { "pass_through" }.to_string(),
                )
            }
        };
        let enabled_mask = layer.mask.as_ref().filter(|mask| mask.enabled());
        let mut need = EvaluationNeed::Pointwise;
        if let Some(mask) = &layer.mask {
            // Every contributing mask is charged here, and every one the
            // loop reads is prepared before it and held at once with every
            // other (a whole-layer-skipped adjustment's is charged but never
            // prepared — the reference never prepares it).
            let (scratch, work) = mask.plan()?;
            self.mask_scratch = self.mask_scratch.saturating_add(scratch);
            self.mask_work = self.mask_work.saturating_add(work);
        }
        // A raster whose style has an enabled effect renders PLANES from the
        // whole layer before the loop (`typed_shape` + `render_effects_with_job`), and
        // a group with one is materialized over the whole canvas: each a
        // `Global` row, which contributes no halo (its mask's feather is
        // subsumed by the whole-canvas input).
        let planes = effects;
        if let Some(mask) = enabled_mask {
            let properties = mask.properties();
            if properties.feather > 0.0 && planes.is_none() {
                let pad = properties
                    .padded_dimensions(mask.plane().dimensions())
                    .map_or(0, |(_, _, pad)| pad);
                need = EvaluationNeed::Spatial { halo_pixels: pad };
            }
        }
        if let (Some(style), Content::Raster { image, .. }) = (planes, &layer.content) {
            // What the u8 renderer allocates for the planes and the work
            // `prepare` advances for them (`style_scratch`).
            let (scratch, work) = style_scratch(style, image.dimensions(), false, None)?;
            self.style_scratch = self.style_scratch.saturating_add(scratch);
            self.style_work = self.style_work.saturating_add(work);
            need = EvaluationNeed::Global {
                analysis: STYLE_PLANES_ANALYSIS,
            };
            if !self.global.contains(&STYLE_PLANES_ANALYSIS) {
                self.global.push(STYLE_PLANES_ANALYSIS);
            }
        }
        if planes.is_some() && layer.isolates() {
            need = EvaluationNeed::Global {
                analysis: MATERIALIZATION_ANALYSIS,
            };
            if !self.global.contains(&MATERIALIZATION_ANALYSIS) {
                self.global.push(MATERIALIZATION_ANALYSIS);
            }
        }
        if let (Some(style), Some(sub)) = (planes, root) {
            // The retained group buffer and the nest `render_layers`
            // allocates inside it (`precision_render.rs`'s `zero_plane` per
            // isolated group and clip unit, the pass-through copy, each clip
            // level's alpha record), every styled row's planes inside, and
            // this group's own planes (`style_scratch` over the canvas-sized
            // buffer); the buffer fill, the subtree's work and the planes'.
            let (scratch, work) = style_scratch(style, self.canvas, false, None)?;
            self.materialization_scratch = self.materialization_scratch.saturating_add(
                (canvas * 16)
                    .saturating_mul(1 + u64::from(sub.depth))
                    .saturating_add(canvas * 4 * u64::from(sub.clip_levels))
                    .saturating_add(sub.style_scratch)
                    .saturating_add(scratch),
            );
            self.materialization_work = self
                .materialization_work
                .saturating_add(canvas)
                .saturating_add(sub.work)
                .saturating_add(work);
        }
        if let Some(sub) = materialized {
            // `collect`'s per-row charges inside a materialized subtree.
            let added = match &layer.content {
                Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                    return Err(crate::cmyk_document::refused_sentence(
                        "Regional compositing",
                    ))
                }
                Content::Raster { image, offset } => {
                    let (x0, y0, x1, y1) = intersection(image.dimensions(), *offset, self.canvas);
                    (x1 - x0) * (y1 - y0)
                }
                Content::Adjustment(state) => canvas.saturating_add(
                    state
                        .kernel
                        .guide_work_units(self.canvas.0, self.canvas.1)?,
                ),
                // Charged with the group's level above.
                Content::Group(_) | Content::PassThrough(_) => 0,
            };
            sub.work = sub.work.saturating_add(added);
            // `collect`'s D8 term (`knockout_scratch`): the routed unit
            // buffer, the grouped-interior alpha record and the clip unit's
            // base-only snapshot, as `collect` decides them.
            let routed = layer
                .style
                .as_deref()
                .is_some_and(|s| s.knockout != Knockout::None || s.blend_interior_as_group);
            let grouped = layer
                .style
                .as_deref()
                .is_some_and(|s| s.blend_interior_as_group);
            sub.style_scratch = sub.style_scratch.saturating_add(knockout_scratch(
                routed,
                grouped,
                knockout_member,
                self.canvas,
            ));
            if let Some(style) = layer.style.as_deref() {
                let plane = match &layer.content {
                    Content::Raster { image, .. } => image.dimensions(),
                    _ => self.canvas,
                };
                let package = (layer.opacity < 1.0
                    && layer.blend_mode != BlendMode::Dissolve
                    && style.has_enabled_effects())
                .then_some(self.canvas);
                let (scratch, work) =
                    style_scratch(style, plane, layer.blend_if.is_some(), package)?;
                sub.style_scratch = sub.style_scratch.saturating_add(scratch);
                sub.work = sub.work.saturating_add(work);
            }
        }
        self.needs.push(LayerNeed {
            index,
            name: layer.name.to_string(),
            kind,
            operator,
            need,
            clip_base,
            group,
            mask: enabled_mask.is_some(),
            blend_if: layer.blend_if.is_some(),
            style: layer
                .style
                .as_ref()
                .map(|_| if planes.is_some() { "effects" } else { "fill" }),
        });
        Ok(())
    }
}
