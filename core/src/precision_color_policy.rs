//! The typed twins of `doc_color_policy`: the same four operations on a
//! [`PrecisionDocument`], under the same rules, with the typed document's own
//! admission and cancellation contract added. The policy, the per-kind table
//! and every refusal sentence live in `doc_color_policy` and are read from
//! here, so the two depths cannot drift.
//!
//! The one real difference is where the bake's pixels come from:
//! `render_cut_with_context` rather than `flattened_cut`, at the document's
//! own depth, so a preserve-appearance bake on a 16-bit master stays 16-bit.
//!
//! The per-kind rule is in `doc_color_policy`'s module doc and the typed twin READS it —
//! `adjustment_op`, `appearance_invariant` and `source_sentence` are shared, so the two depths
//! cannot word one rule two ways.
//!
//! # Admission
//!
//! A preserve-appearance Convert that bakes k > 0 adjustments admits
//! `conversion_plan`, not the colour plan, which never saw the bakes. With
//! B = 8·w·h (one canvas-sized u16 plane), R the document's render scratch,
//! C₀ the colour plan's scratch and B_max the largest raster plane C₀
//! converts:
//!
//! - bake phase: R + (k − 1)·B — the i-th bake renders while the i earlier
//!   baked layers are alive (each bake reserves under `with_held_bytes`);
//! - conversion phase: C₀ + 2·k·B + (B − B_max, when positive) — the k
//!   baked layers the source now holds and their k converted outputs; C₀
//!   already charges the rebuild's contiguous buffer held beside its tiles
//!   (`rebuilt_rgba16`) at the largest converted plane, so only the canvas's
//!   excess over it is added;
//! - scratch = S + ⌈S/4⌉ with S the larger phase;
//!   retained = the colour plan's; converted pixels = the colour plan's + k·B.
//!
//! k = 0 (keep parameters, the document's own bytes, or nothing to bake) is
//! the colour plan unchanged.
//!
//! The colour plan of a conversion (`color_plan`, convert true, another
//! profile) charges that rebuild buffer too — the largest converted plane
//! once more — and adds a quarter to its pixel sum (the converted
//! planes plus that buffer; the rebound records stay as assignment charges
//! them): before this the default 16-bit Convert was admitted at
//! 0.52 × its measured footprint, and with the plane alone at 1.016.
//!
//! Measured 2026-10-10 on an Apple M2 Max (96 GB, macOS 26.5.1), Release, one
//! case per process, `MallocLargeCache=0`, process working limit
//! 8,589,934,592, three runs plus a baseline twin; E =
//! `ri_interval_max_phys_footprint` − the `ri_phys_footprint` read right
//! after `proc_reset_footprint_interval` (receipt
//! `doctored-fixtures/admission-636-610`, run `20261010-070858`). Fixture: a
//! 6000×4000 opaque u16 sRGB raster (`tests/conversion_bake_24mp.rs`' `base`
//! pattern) under three Curves `{"rgb":[[0,0],[128,160],[255,255]]}`
//! (precision_v1); the call is `dr_precision_doc_convert_to_profile_with` to
//! Display P3 at 8 GiB under a control whose scratch limit is the plan.
//! convert16p bakes all three (`{"adjustments":"preserve_appearance"}`) and
//! lands with three `(baked)` layers; convert16 is the default options.
//!
//! | case | pre / E, run 1 | run 2 | run 3 | plan | plan ÷ max E |
//! | --- | --- | --- | --- | --- | --- |
//! | convert16p | 386,531,904 / 1,351,140,456 | 386,548,312 / 1,351,107,664 | 386,531,904 / 1,351,091,280 | 2,051,003,224 | 1.518 |
//! | convert16 | 386,531,904 / 386,810,144 | 386,515,520 / 386,793,760 | 386,564,696 / 386,810,144 | 489,014,858 | 1.264 |
//!
//! Plans: `dr_precision_doc_conversion_plan`'s `scratch_bytes` (preserve;
//! NULL options, which equals the convert colour plan). Baseline twin (the
//! setup, no call): E = 0 in all three runs. Both plans are at least
//! ⌈1.25 × max E⌉ (1,688,925,570; 483,512,680), the floors
//! `tests/conversion_bake_24mp.rs` pins.

use std::sync::Arc;

use crate::doc_color_policy::{
    adjustment_op, appearance_invariant, source_sentence, AdjustmentOutcome, AdjustmentPolicy,
    ConversionOptions, ConversionReport,
};
use crate::icc::IccProfile;
use crate::icc_intent::ColorPipeline;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_color::PrecisionColorPlan;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, reserved_vec, PrecisionImage};
use crate::retained_source::SourceEdit;
use crate::style_composite::Cut;

impl PrecisionDocument {
    /// Layers still holding a retained original source.
    pub(crate) fn retained_source_count(&self) -> usize {
        (0..self.layer_count())
            .filter(|&index| {
                self.layer(index)
                    .and_then(|l| l.effective_source())
                    .is_some()
            })
            .count()
    }

    /// Why an Assign would reinterpret a retained source, or `None`.
    pub fn assign_refusal(&self) -> Option<String> {
        let sources = self.retained_source_count();
        (sources > 0).then(|| source_sentence(sources, "assigning a profile"))
    }

    /// `assign_refusal()` when `profile`'s bytes differ from the document's; `None` for its own bytes (the no-op).
    pub(crate) fn assign_refusal_for(&self, profile: &IccProfile) -> Option<String> {
        if self.profile.bytes() == profile.bytes() {
            return None;
        }
        self.assign_refusal()
    }

    /// Rasterize every bound layer, then assign — the disclosed Assign, as
    /// ONE result.
    pub fn assign_profile_rasterizing(
        &self,
        profile: &Arc<IccProfile>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.rasterizing_sources(limit, other, job)?
            .color_edit_with_context(profile, false, limit, other, job)
    }

    /// Every bound layer rasterized (`SourceEdit::Rasterize`, the SHIPPED
    /// typed rule), bottom first: the step `assign_profile_rasterizing` and a
    /// `rasterize_sources` conversion share (`doc_color_policy`'s module
    /// doc). A nested control token would consume the compound's, so each
    /// step runs untokened, with `job` checked between them.
    pub(crate) fn rasterizing_sources(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let mut running = self.clone();
        loop {
            job.check()?;
            let next = (0..running.layer_count()).find(|i| {
                running
                    .layer(*i)
                    .and_then(|l| l.effective_source())
                    .is_some()
            });
            let Some(index) = next else { break };
            match running.source_edit(
                None,
                &SourceEdit::Rasterize { layer: index },
                limit,
                other,
                None,
            )? {
                Some(next) => running = next,
                None => break,
            }
        }
        Ok(running)
    }

    /// What a conversion to `profile` under `options` WOULD do, plus the plan
    /// its admission needs. The document's own profile bytes are no change at
    /// any options, before every refusal but the plan's own admission:
    /// the admitted plan and an empty report. The baking projections run
    /// under `job`. Under `rasterize_sources` the source refusal is
    /// passed: the conversion rasterizes first.
    pub(crate) fn conversion_preflight(
        &self,
        profile: &Arc<IccProfile>,
        options: &ConversionOptions,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(PrecisionColorPlan, ConversionReport), String> {
        if self.profile().bytes() == profile.bytes() {
            let plan = self.color_plan(profile, true)?;
            admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
            return Ok((plan, ConversionReport::default()));
        }
        // A float Convert has one conversion (`float_color`): the options it
        // cannot follow refuse here, right after the own-bytes rule.
        if self.is_float() {
            if let Some(refusal) = crate::float_color::options_refusal(options) {
                return Err(refusal);
            }
        }
        if let Some(refusal) = crate::icc_encode_refusal::encode_into_refusal(profile, "convert") {
            return Err(refusal);
        }
        let sources = self.retained_source_count();
        if let Some(refusal) = options.source_refusal(self.profile(), profile, sources) {
            if !options.rasterize_sources {
                return Err(refusal);
            }
        }
        let plan = self.color_plan(profile, true)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        let mut report = ConversionReport::default();
        for index in 0..self.layer_count() {
            let Some(layer) = self.layer(index) else {
                continue;
            };
            let Content::Adjustment(state) = &layer.content else {
                continue;
            };
            let description = state.description().to_string();
            let Some((op, evaluation)) = adjustment_op(Some(&description)) else {
                continue;
            };
            report.adjustments.push(self.adjustment_outcome(
                index,
                layer,
                &op,
                evaluation.as_deref(),
                options,
                limit,
                other,
                job,
            )?);
        }
        Ok((plan, report))
    }

    /// The admission plan of `convert_to_profile_with`: the colour
    /// plan when nothing is baked, otherwise the bake phase and the
    /// conversion phase as a max, plus a quarter. The formula and its
    /// measurement are in the module doc's `# Admission`.
    pub fn conversion_plan(
        &self,
        profile: &Arc<IccProfile>,
        options: &ConversionOptions,
    ) -> Result<PrecisionColorPlan, String> {
        const OVERFLOW: &str = "high-depth conversion plan calculation overflow";
        if self.is_float() && self.profile().bytes() != profile.bytes() {
            if let Some(refusal) = crate::float_color::options_refusal(options) {
                return Err(refusal);
            }
        }
        let color = self.color_plan(profile, true)?;
        if self.profile().bytes() == profile.bytes()
            || options.adjustments != AdjustmentPolicy::PreserveAppearance
        {
            return Ok(color);
        }
        let mut baked = 0_u64;
        for index in 0..self.layer_count() {
            let Some(layer) = self.layer(index) else {
                continue;
            };
            let Content::Adjustment(state) = &layer.content else {
                continue;
            };
            let description = state.description().to_string();
            let Some((op, evaluation)) = adjustment_op(Some(&description)) else {
                continue;
            };
            if keep_reason(layer, &op, evaluation.as_deref(), options).is_none() {
                baked += 1;
            }
        }
        if baked == 0 {
            return Ok(color);
        }
        let canvas = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|v| v.checked_mul(8))
            .ok_or(OVERFLOW)?;
        // The largest plane `convert_pixels` rebuilds: its contiguous buffer
        // is alive while `rebuilt_rgba16` builds the tiled copy. The colour
        // plan already charges the largest plane it converts; the baked
        // layers are canvas-sized, so only the canvas's excess over it is
        // added. `largest_plane` counts exactly the planes `color_plan`'s
        // `nodes` counts.
        fn largest_plane(layers: &[PrecisionDocumentLayer], largest: u64) -> u64 {
            layers
                .iter()
                .fold(largest, |largest, layer| match &layer.content {
                    Content::CmykRaster { .. } | Content::CmykAdjustment(_) => largest,
                    Content::Raster { image, .. } => {
                        let (w, h) = image.dimensions();
                        largest.max(u64::from(w) * u64::from(h) * 8)
                    }
                    Content::Adjustment(_) => largest,
                    Content::Group(children) | Content::PassThrough(children) => {
                        largest_plane(children, largest)
                    }
                })
        }
        let rebuild = canvas.saturating_sub(largest_plane(&self.layers, 0));
        let render = self.render_plan()?.scratch_bytes;
        let bake = canvas
            .checked_mul(baked - 1)
            .and_then(|held| held.checked_add(render))
            .ok_or(OVERFLOW)?;
        let convert = canvas
            .checked_mul(2 * baked)
            .and_then(|v| v.checked_add(color.scratch_bytes))
            .and_then(|v| v.checked_add(rebuild))
            .ok_or(OVERFLOW)?;
        let peak = bake.max(convert);
        Ok(PrecisionColorPlan {
            retained_bytes: color.retained_bytes,
            scratch_bytes: peak.checked_add(peak.div_ceil(4)).ok_or(OVERFLOW)?,
            converted_pixel_bytes: canvas
                .checked_mul(baked)
                .and_then(|v| v.checked_add(color.converted_pixel_bytes))
                .ok_or(OVERFLOW)?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn adjustment_outcome(
        &self,
        index: usize,
        layer: &PrecisionDocumentLayer,
        op: &str,
        evaluation: Option<&str>,
        options: &ConversionOptions,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<AdjustmentOutcome, String> {
        let name = layer.name.to_string();
        let keep = |reason: String| AdjustmentOutcome {
            index,
            name: name.clone(),
            op: op.to_string(),
            action: "kept",
            reason,
        };
        if let Some(reason) = keep_reason(layer, op, evaluation, options) {
            return Ok(keep(reason));
        }
        if layer.depth != 0 {
            return Err(format!(
                "\"{name}\" is inside a group, which preserve-appearance cannot bake"
            ));
        }
        if layer.clipped {
            return Err(format!(
                "\"{name}\" is clipped, which preserve-appearance cannot bake"
            ));
        }
        // The transparent-backdrop refusal belongs HERE, in the preflight,
        // for the reason the byte twin gives: a refusal discovered at bake
        // time would have already reported the layer as "baked" and left the
        // host's Apply enabled. `bake_adjustment` keeps the same check.
        self.baked_projection(index, &name, limit, other, job)?;
        Ok(AdjustmentOutcome {
            index,
            name: name.clone(),
            op: op.to_string(),
            action: "baked",
            reason: format!(
                "{op} is defined on the encoded numbers of its own space, so it is baked into \"{name} (baked)\""
            ),
        })
    }

    /// Convert to `profile` under `options`. `Ok(None)` is a refusal that
    /// changes nothing; with the options at their defaults this is exactly
    /// `convert_to_profile`'s existing arithmetic. The document's own profile
    /// bytes are no change at any options, before every refusal but the plan's
    /// admission and scratch reserve. Under `rasterize_sources`,
    /// settings that refuse to keep the sources bound convert
    /// [`PrecisionDocument::rasterizing_sources`]'s result instead
    /// (`doc_color_policy`'s module doc).
    pub fn convert_to_profile_with(
        &self,
        profile: &Arc<IccProfile>,
        options: &ConversionOptions,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<(Self, ConversionReport)>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        // An Indexed Color document converts its colour table (#1 Indexed
        // phase 3a, `indexed_profile`).
        if self.is_indexed() {
            return crate::indexed_profile::convert(self, profile, options, limit, other, job);
        }
        // A float document refuses the options its one conversion cannot
        // follow before anything is rasterized (`float_color`).
        if self.is_float() && self.profile().bytes() != profile.bytes() {
            if let Some(refusal) = crate::float_color::options_refusal(options) {
                return Err(refusal);
            }
        }
        if self.profile().bytes() != profile.bytes()
            && options.rasterizes(self.profile(), profile, self.retained_source_count())
        {
            return self
                .rasterizing_sources(limit, other, job)?
                .converting(profile, options, limit, other, job);
        }
        self.converting(profile, options, limit, other, job)
    }

    /// [`Self::convert_to_profile_with`]'s body under `job`.
    pub(crate) fn converting(
        &self,
        profile: &Arc<IccProfile>,
        options: &ConversionOptions,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<(Self, ConversionReport)>, String> {
        let (_, report) =
            self.conversion_preflight(profile, options, limit, other, job.checkpoints_only())?;
        let plan = self.conversion_plan(profile, options)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        if self.profile().bytes() == profile.bytes() {
            return Ok(None);
        }
        if options.is_default() || self.is_float() {
            // The shipped typed path, statement for statement — the bytes an
            // existing 16-bit conversion writes are its bytes. A float
            // document has one conversion at every admitted option
            // (`float_color`), reached through the same call.
            return Ok(self
                .color_edit_with_context(profile, true, limit, other, job)?
                .map(|document| (document, report)));
        }
        let pipeline = ColorPipeline::new(
            self.profile(),
            profile,
            options.intent,
            options.black_point_compensation,
        )?;
        if pipeline.is_identity() {
            return Ok(None);
        }
        job.reserve(pipeline.scratch_bytes())?;
        let mut source = self.clone();
        // Each earlier bake's layer stays live under the next bake's render
        // (`conversion_plan`'s R + (k − 1)·B).
        let canvas = u64::from(self.width) * u64::from(self.height) * 8;
        let mut held = 0_u64;
        for outcome in report.adjustments.iter().filter(|a| a.action == "baked") {
            source = source.bake_adjustment(
                outcome.index,
                &outcome.name,
                limit,
                other,
                job.with_held_bytes(held),
            )?;
            held = held.saturating_add(canvas);
            job.check()?;
        }
        if self.retained_source_count() > 0 && pipeline.is_shipped() {
            // The source preflight admits nothing else on a bound document,
            // and the shipped pipeline's bytes are the default conversion's,
            // so the shipped typed path converts it: bindings kept, each one
            // re-rendered into the destination (`render16`).
            return Ok(source
                .color_edit_with_context(profile, true, limit, other, job)?
                .map(|document| (document, report)));
        }
        let converted = source.convert_pixels(profile, &pipeline, job)?;
        Ok(Some((converted, report)))
    }

    /// Every distinct stored raster through `pipeline`, alpha exact, with the
    /// adjustment operators rebound to the destination profile.
    fn convert_pixels(
        &self,
        profile: &Arc<IccProfile>,
        pipeline: &ColorPipeline,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        fn walk(
            layers: &[PrecisionDocumentLayer],
            profile: &Arc<IccProfile>,
            pipeline: &ColorPipeline,
            job: JobContext<'_>,
        ) -> Result<Vec<PrecisionDocumentLayer>, String> {
            let mut out = reserved_vec(layers.len())?;
            for layer in layers {
                job.check()?;
                let mut next = layer.clone();
                next.retained_source = None;
                next.description = None;
                let content = match &layer.content {
                    Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                        return Err(crate::cmyk_document::refused_sentence("A colour edit"))
                    }
                    Content::Raster { image, offset } => {
                        let runs = image
                            .runs_rgba16()
                            .ok_or("high-depth color edit requires a u16 source")?;
                        let (w, h) = image.dimensions();
                        let mut pixels = reserved_vec(w as usize * h as usize)?;
                        let mut count = 0usize;
                        for run in runs {
                            for pixel in run {
                                let i = count;
                                count += 1;
                                if i & 4095 == 0 {
                                    job.check()?;
                                }
                                let rgb = pipeline
                                    .color(std::array::from_fn(|c| f32::from(pixel[c]) / 65535.0));
                                if rgb
                                    .iter()
                                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                                {
                                    return Err("high-depth color transform produced a nonfinite or out-of-range component".into());
                                }
                                pixels.push([
                                    (rgb[0] * 65535.0).round() as u16,
                                    (rgb[1] * 65535.0).round() as u16,
                                    (rgb[2] * 65535.0).round() as u16,
                                    pixel[3],
                                ]);
                            }
                        }
                        Content::Raster {
                            image: image.rebuilt_rgba16(w, h, pixels, profile.clone())?,
                            offset: *offset,
                        }
                    }
                    Content::Group(children) => {
                        Content::Group(Arc::new(walk(children, profile, pipeline, job)?))
                    }
                    Content::PassThrough(children) => {
                        Content::PassThrough(Arc::new(walk(children, profile, pipeline, job)?))
                    }
                    Content::Adjustment(state) => Content::Adjustment(state.rebind(profile)?),
                };
                next.content = content;
                out.push(next);
            }
            Ok(out)
        }
        let layers = walk(&self.layers, profile, pipeline, job)?;
        let mut next = Self::from_parts_and_channels(
            self.width,
            self.height,
            profile.clone(),
            self.metadata().clone(),
            self.resolution(),
            layers,
            self.channels().to_vec(),
        )?;
        next.import_warnings = self.import_warnings.clone();
        // The evaluation policy's tables belong to the profile they were built
        // from, so a profile write rebuilds them (`precision_evaluation`).
        next.linear_light = self.linear_light.clone();
        // …and the canvas chrome: a profile edit moves no pixel and no
        // canvas, so the guides and the ruler origin ride along
        // (`precision_guide`).
        next.guides.clone_from(&self.guides);
        next.ruler_origin = self.ruler_origin;
        next.color_samplers.clone_from(&self.color_samplers);
        next.paths.clone_from(&self.paths);
        next.global_light = self.global_light;
        // …and the LAYER COMPS, for the same reason: a profile edit changes
        // no record, so every comp entry still binds by the id its layer
        // kept (`precision_comps`).
        next.comps.clone_from(&self.comps);
        next.background = self.background;
        next.rebind_evaluation()?;
        next.native_preflight()?;
        Ok(next)
    }

    /// Replaces the adjustment at postorder `index` with one raster layer
    /// holding the typed projection of everything up to and including it.
    /// The typed projection a bake at `index` would install, or the refusal
    /// it raises when that projection is not fully opaque. Run by the
    /// preflight and again by the bake itself.
    fn baked_projection(
        &self,
        index: usize,
        name: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let mut excluded = vec![false; self.layer_count()];
        for slot in excluded.iter_mut().skip(index + 1) {
            *slot = true;
        }
        let baked = self.render_cut_with_context(Cut::new(&excluded), limit, other, job)?;
        if baked
            .runs_rgba16()
            .ok_or("a typed bake requires RGBA16 UNORM storage")?
            .any(|run| run.iter().any(|p| p[3] != u16::MAX))
        {
            return Err(format!(
                "\"{name}\" sits over transparent pixels, which preserve-appearance cannot bake"
            ));
        }
        Ok(baked)
    }

    fn bake_adjustment(
        &self,
        index: usize,
        name: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let baked = self.baked_projection(index, name, limit, other, job)?;
        let layer =
            PrecisionDocumentLayer::raster(&format!("{name} (baked)"), baked, (0, 0), 1.0, true)?;
        // Baking only ever targets a depth-0 entry (the preflight refuses a
        // nested one), so the postorder index IS a position in the top level
        // once its preceding subtrees are counted.
        let mut layers = (*self.layers).clone();
        let mut cursor = 0usize;
        let mut placed = false;
        for slot in layers.iter_mut() {
            cursor += slot.records;
            if cursor - 1 == index {
                *slot = layer;
                placed = true;
                break;
            }
        }
        if !placed {
            return Err(format!("\"{name}\" is not a top-level entry"));
        }
        let mut next = Self::from_parts_and_channels(
            self.width,
            self.height,
            self.profile().clone(),
            self.metadata().clone(),
            self.resolution(),
            layers,
            self.channels().to_vec(),
        )?;
        next.import_warnings = self.import_warnings.clone();
        // A bake keeps the profile, so it keeps the policy's tables too.
        next.linear_light = self.linear_light.clone();
        // …and the canvas chrome: a profile edit moves no pixel and no
        // canvas, so the guides and the ruler origin ride along
        // (`precision_guide`).
        next.guides.clone_from(&self.guides);
        next.ruler_origin = self.ruler_origin;
        next.color_samplers.clone_from(&self.color_samplers);
        next.paths.clone_from(&self.paths);
        next.global_light = self.global_light;
        // …and the LAYER COMPS, for the same reason: a profile edit changes
        // no record, so every comp entry still binds by the id its layer
        // kept (`precision_comps`).
        next.comps.clone_from(&self.comps);
        next.background = self.background;
        Ok(next)
    }
}

/// Why preserve-appearance KEEPS this adjustment's parameters, or `None` when
/// it must be baked (or refused): the policy, a hidden layer, an
/// appearance-invariant operator — in that order. Shared by the preflight's
/// report and `conversion_plan`'s count of bakes, so the two cannot disagree.
fn keep_reason(
    layer: &PrecisionDocumentLayer,
    op: &str,
    evaluation: Option<&str>,
    options: &ConversionOptions,
) -> Option<String> {
    if options.adjustments == AdjustmentPolicy::KeepParameters {
        return Some(
            "the keep-parameters policy leaves every adjustment's parameters unchanged".into(),
        );
    }
    if !layer.visible {
        return Some("a hidden adjustment contributes nothing to the appearance".into());
    }
    if appearance_invariant(op, evaluation) {
        return Some(format!(
            "{op} with document_v2 evaluation is a light-linear operation through the document's own profile"
        ));
    }
    None
}
