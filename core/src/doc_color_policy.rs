//! The Phase 11C conversion POLICY on a byte document: what intent and
//! black-point compensation a Convert to Profile runs under, what happens to
//! the adjustment layers while it does, and the two explicit Assign
//! workflows. The numerics are `icc_intent`; the typed twins are
//! `precision_color_policy`; `doc_color` keeps the unconditional
//! `assign_profile` / `convert_to_profile` entry points and delegates here.
//!
//! # Keep parameters is still the default
//!
//! [`AdjustmentPolicy::KeepParameters`] is what Doctored has always done and
//! what every old document keeps: an adjustment's numbers are PARAMETERS, not
//! colours, so a conversion leaves them exactly as authored and applies them
//! to numbers that have moved (`doc_color`'s module doc says why). Each layer
//! keeps its appearance; a stack whose result is computed from the numbers
//! does not, and the host says so.
//!
//! # Preserve appearance, and the per-kind rule
//!
//! [`AdjustmentPolicy::PreserveAppearance`] is the explicit alternative P7-22
//! asks for. It is never silent, it never flattens the master, and it is
//! decided per adjustment KIND:
//!
//! | kind | action | why |
//! |---|---|---|
//! | `exposure` / `white_balance` with `evaluation: "document_v2"` | **kept** into a matrix/TRC destination; **baked** into one with no matrix/TRC model (keep-parameters refuses there, naming the layer — `doc_color_destination`) | both are LINEAR-LIGHT operations evaluated through the document's own profile, so the same parameters produce the same appearance in any space whose matrix/TRC model can evaluate them. A LUT-only destination cannot, so there the appearance is baked or the conversion refuses. |
//! | every other op, and `legacy_srgb` evaluation | **baked** | the op is defined on the ENCODED numbers of whatever space it runs in (a curve point, a levels endpoint, a saturation amount), so there is no parameter that would mean the same thing after the conversion. |
//!
//! A baked entry is replaced, IN PLACE, by one raster layer named
//! `"<name> (baked)"` holding the projection of everything up to and
//! including it — `flattened_cut` with every entry above it excluded — at
//! Normal, 100 %, no mask, visible. The layers below it stay exactly where
//! they are: the master's structure is preserved and nothing is flattened.
//! That substitution is an identity only if the projection beneath is fully
//! opaque, which is why a cut projection with any alpha below 255 REFUSES,
//! naming the layer.
//!
//! Two more refusals, both naming the layer: a to-be-baked adjustment nested
//! inside a group, and a clipped one. Both would need the bake to reproduce
//! the group's own opacity/blend/mask or the clip base's footprint, which is
//! a different (and much larger) operation than this one. A HIDDEN
//! adjustment is left alone — it contributes nothing to the appearance, so
//! there is nothing to preserve.
//!
//! Bakes run BOTTOM-UP on the running document, so an adjustment above one
//! that has already been baked sees the baked pixels rather than a stale
//! stack.
//!
//! # Assign, and the 8A rule
//!
//! Assign retags without touching samples, so an 8A retained source — whose
//! fallback pixels are a RENDER of an original asset in a specific profile —
//! cannot survive it: reinterpreting the fallback while the original still
//! claims its own profile is exactly the silent reinterpretation Phase 8A
//! refuses. [`DrDocument::assign_refusal`] names the count, and
//! [`DrDocument::assign_profile_rasterizing`] is the disclosed way through:
//! every bound layer is rasterized (`SourceEdit::Rasterize`) and then the
//! profile is assigned, as ONE result the host commits as ONE undo step.
//!
//! # Retained sources (RAW layers)
//!
//! A source always re-renders relative colorimetric with BPC off
//! (`retained_source_render`: the shipped matrix/TRC transforms, or
//! `lut_pipeline` when a side has no matrix/TRC model). So
//! [`ConversionOptions::source_refusal`] — the ONE home of this rule, asked
//! by both depths' preflights — admits a matrix/TRC pair at relative,
//! perceptual or saturation (`icc_intent`'s fast path ignores a non-absolute
//! intent, so all three are the source render's bytes) and a LUT side only
//! at relative; black-point compensation and absolute refuse everywhere. The
//! refusal ([`settings_sentence`]) names the setting that keeps the sources
//! bound — relative colorimetric with BPC off — which holds for both kinds of
//! pair. Preserve appearance is admitted beside a source. At 16 bits a LUT
//! side still refuses at the typed colour plan (high-depth conversion needs a
//! matrix/TRC destination).
//!
//! [`ConversionOptions::rasterize_sources`] is the disclosed way past
//! that refusal, as `assign_profile_rasterizing` is for Assign: when the
//! settings would refuse, both depths' `convert_to_profile_with` rasterize
//! every bound layer first ([`DrDocument::rasterizing_sources`]) and convert
//! the result, as ONE result the host commits as ONE undo step. Settings the
//! sources can follow keep them bound whatever the flag says. A bound source
//! therefore never needs another intent persisted.
//!
//! `ConversionOptions` (intent, BPC, adjustment policy; every default is today's behaviour and
//! `is_default()` is what the byte-identity claim keys on), the preflight and its
//! `ConversionReport` (the document's own profile BYTES are no change at any options, before every
//! refusal at both depths — at 16 bits after the plan's own admission —: an empty report and
//! `Ok(None)` — different bytes for the same space keep the `is_identity` path). A
//! preserve-appearance bake replaces ONE entry with one raster layer holding
//! `flattened_cut`/`render_cut_with_context` of everything up to and including it and leaves every
//! other layer where it is; it refuses, naming the layer, when that entry is nested, clipped or
//! sits over any alpha below the maximum, because the substitution is an identity only over an
//! opaque backdrop. Assign refuses by name while a retained source is present (at 16 bits too,
//! through `dr_precision_doc_assign_profile`) and `assign_profile_rasterizing` is the
//! disclosed way through — and its admission limit is the process working limit
//! (`precision_image::process_working_limit()`). `ConversionOptions::source_refusal` is the one
//! home of the Convert rule on a document holding a retained source (both depths' preflights ask
//! it), and `rasterize_sources` its disclosed way through.

use std::sync::Arc;

use image::RgbaImage;
use serde_json::{json, Map, Value};

use crate::adjust_calibrated::evaluates_in;
use crate::doc::{DrDocument, Layer, LayerKind};
use crate::doc_color_destination::{calibrated_bake_reason, calibrated_sentence};
use crate::icc::IccProfile;
use crate::icc_intent::{ColorPipeline, RenderingIntent};
use crate::job_control::JobContext;
use crate::retained_source::SourceEdit;
use crate::style_composite::Cut;

/// What a conversion does to the document's adjustment layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AdjustmentPolicy {
    /// Today's policy, and the default: the parameters are left exactly as
    /// authored.
    #[default]
    KeepParameters,
    /// The explicit P7-22 alternative (module doc).
    PreserveAppearance,
}

impl AdjustmentPolicy {
    pub fn name(self) -> &'static str {
        match self {
            AdjustmentPolicy::KeepParameters => "keep",
            AdjustmentPolicy::PreserveAppearance => "preserve_appearance",
        }
    }

    pub fn parse(text: &str) -> Option<AdjustmentPolicy> {
        match text {
            "keep" => Some(AdjustmentPolicy::KeepParameters),
            "preserve_appearance" => Some(AdjustmentPolicy::PreserveAppearance),
            _ => None,
        }
    }
}

/// Everything a Convert to Profile is parameterized by beyond the
/// destination. Every field is optional in JSON and every default is TODAY's
/// behaviour, which is what makes "the default conversion is byte-identical"
/// a checkable claim rather than a promise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConversionOptions {
    pub intent: RenderingIntent,
    pub black_point_compensation: bool,
    pub adjustments: AdjustmentPolicy,
    /// Rasterize every bound layer first when the settings would refuse to
    /// keep the sources bound (module doc). JSON `"rasterize_sources"`.
    pub rasterize_sources: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        ConversionOptions::DEFAULT
    }
}

impl ConversionOptions {
    /// The defaults, as a constant for callers that cannot call `default()`
    /// in a const context.
    pub const DEFAULT: ConversionOptions = ConversionOptions {
        intent: RenderingIntent::RelativeColorimetric,
        black_point_compensation: false,
        adjustments: AdjustmentPolicy::KeepParameters,
        rasterize_sources: false,
    };

    /// True when nothing about this request differs from today's conversion.
    pub fn is_default(&self) -> bool {
        *self == ConversionOptions::DEFAULT
    }

    /// Parses `{"intent","black_point_compensation","adjustments",
    /// "rasterize_sources"}`. An empty
    /// object, an empty string and `null` all mean the defaults; an unknown
    /// key is refused rather than ignored.
    pub fn parse(json: &str) -> Result<ConversionOptions, String> {
        if json.trim().is_empty() {
            return Ok(ConversionOptions::DEFAULT);
        }
        let value: Value = serde_json::from_str(json)
            .map_err(|e| format!("conversion options are not valid JSON: {e}"))?;
        ConversionOptions::from_value(&value)
    }

    pub fn from_value(value: &Value) -> Result<ConversionOptions, String> {
        if value.is_null() {
            return Ok(ConversionOptions::DEFAULT);
        }
        let object = value
            .as_object()
            .ok_or("conversion options must be a JSON object")?;
        for key in object.keys() {
            if ![
                "intent",
                "black_point_compensation",
                "adjustments",
                "rasterize_sources",
            ]
            .contains(&key.as_str())
            {
                return Err(format!("conversion options have an unknown key \"{key}\""));
            }
        }
        let intent = match object.get("intent") {
            None | Some(Value::Null) => RenderingIntent::RelativeColorimetric,
            Some(Value::String(text)) => RenderingIntent::parse(text).ok_or_else(|| {
                format!(
                    "intent \"{text}\" is not one of perceptual, relative, saturation or absolute"
                )
            })?,
            Some(_) => return Err("conversion option intent must be a string".into()),
        };
        let black_point_compensation = match object.get("black_point_compensation") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(value)) => *value,
            Some(_) => {
                return Err("conversion option black_point_compensation must be a boolean".into())
            }
        };
        let adjustments = match object.get("adjustments") {
            None | Some(Value::Null) => AdjustmentPolicy::KeepParameters,
            Some(Value::String(text)) => AdjustmentPolicy::parse(text).ok_or_else(|| {
                format!("adjustments \"{text}\" is not one of keep or preserve_appearance")
            })?,
            Some(_) => return Err("conversion option adjustments must be a string".into()),
        };
        let rasterize_sources = match object.get("rasterize_sources") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(value)) => *value,
            Some(_) => return Err("conversion option rasterize_sources must be a boolean".into()),
        };
        Ok(ConversionOptions {
            intent,
            black_point_compensation,
            adjustments,
            rasterize_sources,
        })
    }

    /// Why these options cannot convert `sources` retained sources from `src`
    /// into `dst` while keeping them bound; `None` when there is no source or
    /// the options are safe on one (module doc). It does not read
    /// `rasterize_sources`: the conversions ask it to decide whether to
    /// rasterize first.
    pub(crate) fn source_refusal(
        &self,
        src: &IccProfile,
        dst: &IccProfile,
        sources: usize,
    ) -> Option<String> {
        if sources == 0 {
            return None;
        }
        let lut_side = src.model().is_none() || dst.model().is_none();
        if self.black_point_compensation
            || self.intent.is_absolute()
            || (lut_side && self.intent != RenderingIntent::RelativeColorimetric)
        {
            // Sources render relative with BPC off (module doc): refused HERE
            // rather than discovered as a half-converted document.
            return Some(settings_sentence(sources));
        }
        None
    }

    /// Whether a conversion of `sources` retained sources from `src` into
    /// `dst` rasterizes them first: the settings refuse to keep them bound
    /// and `rasterize_sources` consents (module doc).
    pub(crate) fn rasterizes(&self, src: &IccProfile, dst: &IccProfile, sources: usize) -> bool {
        self.rasterize_sources && self.source_refusal(src, dst, sources).is_some()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "intent": self.intent.name(),
            "black_point_compensation": self.black_point_compensation,
            "adjustments": self.adjustments.name(),
            "rasterize_sources": self.rasterize_sources,
        })
    }
}

/// What the policy decided about ONE adjustment layer.
#[derive(Clone, Debug)]
pub struct AdjustmentOutcome {
    /// Index in the document's flattened stack, bottom first.
    pub index: usize,
    pub name: String,
    /// The `op` its meta declares.
    pub op: String,
    /// `"kept"` or `"baked"`.
    pub action: &'static str,
    /// One sentence naming WHY, which is what the host shows.
    pub reason: String,
}

/// Every adjustment layer's outcome, in stack order. Reported by the
/// preflight (before anything happens) and by the conversion (what actually
/// happened) — the same list, so a preview cannot describe a different
/// result from the one that lands.
#[derive(Clone, Debug, Default)]
pub struct ConversionReport {
    pub adjustments: Vec<AdjustmentOutcome>,
}

impl ConversionReport {
    pub fn to_json(&self) -> Value {
        Value::Array(
            self.adjustments
                .iter()
                .map(|a| {
                    json!({
                        "index": a.index,
                        "name": a.name,
                        "op": a.op,
                        "action": a.action,
                        "reason": a.reason,
                    })
                })
                .collect(),
        )
    }

    /// True when at least one entry will be replaced by pixels — the bit a
    /// host uses to decide whether to warn.
    pub fn bakes(&self) -> bool {
        self.adjustments.iter().any(|a| a.action == "baked")
    }
}

/// The `op` and `evaluation` of a layer whose meta IS an adjustment
/// description; `None` for every other layer. Reads the JSON envelope only —
/// the parameters are `adjust.rs`'s business.
pub(crate) fn adjustment_op(meta: Option<&String>) -> Option<(String, Option<String>)> {
    let root: Value = serde_json::from_str(meta?).ok()?;
    let object = root.as_object()?;
    if object.get("type")?.as_str()? != "adjust" {
        return None;
    }
    let op = object.get("op")?.as_str()?.to_string();
    let evaluation = object
        .get("params")
        .and_then(Value::as_object)
        .unwrap_or(&Map::new())
        .get("evaluation")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some((op, evaluation))
}

/// True for the two ops the per-kind rule KEEPS: light-linear operations
/// whose `document_v2` evaluation already runs through the document's own
/// profile.
pub(crate) fn appearance_invariant(op: &str, evaluation: Option<&str>) -> bool {
    matches!(op, "exposure" | "white_balance") && evaluation == Some("document_v2")
}

/// The one sentence a retained-source refusal says. Shared by the preflight,
/// the assign refusal and the typed twins so the host can never see two
/// different wordings of one rule.
pub(crate) fn source_sentence(sources: usize, what: &str) -> String {
    format!(
        "{what} requires rasterizing {} first",
        count_phrase(sources)
    )
}

/// The one sentence [`ConversionOptions::source_refusal`] answers for
/// settings a source render cannot follow; it names the setting that keeps
/// the sources bound instead of rasterizing, which Convert cannot do.
pub(crate) fn settings_sentence(sources: usize) -> String {
    format!(
        "converting with these settings cannot keep {} bound; convert at relative colorimetric with black point compensation off",
        count_phrase(sources)
    )
}

/// "1 image source", "2 image sources".
fn count_phrase(sources: usize) -> String {
    format!(
        "{sources} image source{}",
        if sources == 1 { "" } else { "s" }
    )
}

impl DrDocument {
    /// How many layers still hold a retained original source.
    pub(crate) fn retained_source_count(&self) -> usize {
        self.layers
            .iter()
            .filter(|layer| layer.effective_source(&self.profile).is_some())
            .count()
    }

    /// Why an Assign would reinterpret a retained source, or `None` when it
    /// would not. This is the sentence the sheet and `assign_profile` show
    /// beside the "Rasterize N image sources, then assign" choice.
    pub fn assign_refusal(&self) -> Option<String> {
        let sources = self.retained_source_count();
        (sources > 0).then(|| source_sentence(sources, "assigning a profile"))
    }

    /// Every bound layer rasterized (`SourceEdit::Rasterize`, the SHIPPED
    /// rule, lock check included, rather than a second copy of it), bottom
    /// first: the step `assign_profile_rasterizing` and a `rasterize_sources`
    /// conversion share. `Ok(None)` when a rasterization step answers
    /// `Ok(None)`; `Err` is its sentence (the layer's pixels are locked).
    /// The limit is the process working limit, the bound the shipped
    /// admission uses where no caller limit is passed.
    pub(crate) fn rasterizing_sources(&self) -> Result<Option<Self>, String> {
        let mut running = self.clone();
        loop {
            let next = (0..running.layers.len()).find(|i| {
                running.layers[*i]
                    .effective_source(&running.profile)
                    .is_some()
            });
            let Some(index) = next else { break };
            let Some(rasterized) = running.source_edit(
                None,
                &SourceEdit::Rasterize { layer: index },
                crate::precision_image::process_working_limit(),
                0,
                None,
            )?
            else {
                return Ok(None);
            };
            running = rasterized;
        }
        Ok(Some(running))
    }

    /// The disclosed Assign: rasterize every bound layer, then assign.
    /// `Ok(None)` when the assign itself would change nothing (the same
    /// profile bytes) or a rasterization step answers `Ok(None)`. `Err` is
    /// `assign_profile_refusal`'s sentence when the destination cannot
    /// evaluate the document, the rasterization's own sentence when a bound
    /// layer cannot be rasterized (its pixels are locked), or a failed
    /// rebuild of the evaluation tables. With nothing bound it is
    /// [`DrDocument::assign_profile`] without the rasterization step.
    pub fn assign_profile_rasterizing(&self, p: &Arc<IccProfile>) -> Result<Option<Self>, String> {
        if Arc::ptr_eq(&self.profile, p) || self.profile.bytes() == p.bytes() {
            return Ok(None);
        }
        if let Some(refusal) = self.assign_profile_refusal(p) {
            return Err(refusal);
        }
        let Some(mut out) = self.rasterizing_sources()? else {
            return Ok(None);
        };
        out.profile = Arc::clone(p);
        for layer in &mut out.layers {
            layer.retained_source = None;
        }
        // The evaluation policy's tables belong to the profile they were built
        // from, so a profile write rebuilds them (`doc_evaluation`).
        out.rebind_evaluation()?;
        Ok(Some(out))
    }

    /// What a conversion to `p` under `options` WOULD do, without doing any
    /// of it. `Err` is the refusal the sheet disables its OK button with and
    /// the MCP tool reports. The document's own profile bytes are no change
    /// at any options, before every refusal: an empty report. The
    /// baking projections run under `job`, so a cancel ends the check
    /// with `operation cancelled`. Under `rasterize_sources` the source
    /// refusal is passed: the conversion rasterizes first.
    pub(crate) fn conversion_preflight(
        &self,
        p: &Arc<IccProfile>,
        options: &ConversionOptions,
        job: JobContext<'_>,
    ) -> Result<ConversionReport, String> {
        if self.profile.bytes() == p.bytes() {
            return Ok(ConversionReport::default());
        }
        if let Some(refusal) = crate::icc_encode_refusal::encode_into_refusal(p, "convert") {
            return Err(refusal);
        }
        let sources = self.retained_source_count();
        if let Some(refusal) = options.source_refusal(&self.profile, p, sources) {
            if !options.rasterize_sources {
                return Err(refusal);
            }
        }
        if let Some(refusal) = self.destination_refusal(p) {
            return Err(refusal);
        }
        let mut report = ConversionReport::default();
        for (index, layer) in self.layers.iter().enumerate() {
            let Some((op, evaluation)) = adjustment_op(layer.meta.as_ref()) else {
                continue;
            };
            let outcome =
                self.adjustment_outcome(index, layer, &op, evaluation.as_deref(), options, p, job)?;
            report.adjustments.push(outcome);
        }
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    fn adjustment_outcome(
        &self,
        index: usize,
        layer: &Layer,
        op: &str,
        evaluation: Option<&str>,
        options: &ConversionOptions,
        p: &IccProfile,
        job: JobContext<'_>,
    ) -> Result<AdjustmentOutcome, String> {
        let keep = |reason: String| AdjustmentOutcome {
            index,
            name: layer.name.clone(),
            op: op.to_string(),
            action: "kept",
            reason,
        };
        // A document_v2 Exposure/White Balance the destination cannot
        // evaluate (`doc_color_destination`): keeping it would land a
        // document that refuses to render, so keep refuses, offering the
        // bake only where it would succeed, and preserve bakes it.
        let unevaluable = appearance_invariant(op, evaluation) && !evaluates_in(p);
        if unevaluable && options.adjustments == AdjustmentPolicy::KeepParameters {
            let offer_bake = layer.visible
                && layer.depth == 0
                && !layer.clipped
                && self.baked_projection(index, &layer.name, job).is_ok();
            // A cancelled bake folds into `is_ok() == false` above; surface
            // it as the cancel the header promises, not as a refusal.
            job.check()?;
            return Err(calibrated_sentence(
                &layer.name,
                p.name(),
                "convert to",
                offer_bake,
            ));
        }
        if unevaluable && !layer.visible {
            return Err(calibrated_sentence(
                &layer.name,
                p.name(),
                "convert to",
                false,
            ));
        }
        if options.adjustments == AdjustmentPolicy::KeepParameters {
            return Ok(keep(
                "the keep-parameters policy leaves every adjustment's parameters unchanged".into(),
            ));
        }
        if !unevaluable && !layer.visible {
            return Ok(keep(
                "a hidden adjustment contributes nothing to the appearance".into(),
            ));
        }
        if !unevaluable && appearance_invariant(op, evaluation) {
            return Ok(keep(format!(
                "{op} with document_v2 evaluation is a light-linear operation through the document's own profile"
            )));
        }
        if layer.depth != 0 {
            return Err(format!(
                "\"{}\" is inside a group, which preserve-appearance cannot bake",
                layer.name
            ));
        }
        if layer.clipped {
            return Err(format!(
                "\"{}\" is clipped, which preserve-appearance cannot bake",
                layer.name
            ));
        }
        // The transparent-backdrop refusal belongs HERE, not at bake time:
        // the sheet disables Apply on a preflight refusal, so discovering it
        // inside the bake would report the layer as "baked" and then do
        // nothing. `bake_adjustment` recomputes the projection and keeps the
        // same check as its own backstop.
        self.baked_projection(index, &layer.name, job)?;
        let reason = if unevaluable {
            calibrated_bake_reason(op, &layer.name, p.name())
        } else {
            format!(
                "{op} is defined on the encoded numbers of its own space, so it is baked into \"{} (baked)\"",
                layer.name
            )
        };
        Ok(AdjustmentOutcome {
            index,
            name: layer.name.clone(),
            op: op.to_string(),
            action: "baked",
            reason,
        })
    }

    /// Convert to `p` under `options`. `Ok(None)` is a refusal that changes
    /// nothing (the same space, or a profile the document already carries);
    /// `Err` is the preflight's sentence or a failure while rendering. The
    /// document's own profile bytes are no change at any options, before
    /// every refusal.
    ///
    /// With `options` at its defaults and both profiles matrix/TRC this runs
    /// exactly the arithmetic `convert_to_profile` has always run — see
    /// `icc_intent`'s fast path. Under `rasterize_sources`, settings that
    /// refuse to keep the sources bound convert
    /// [`DrDocument::rasterizing_sources`]'s result instead (module doc).
    /// `pub(crate)` only because [`JobContext`] is: the FFI shim beside it
    /// is the public surface, and the black-box tests reach it there.
    pub(crate) fn convert_to_profile_with(
        &self,
        p: &Arc<IccProfile>,
        options: &ConversionOptions,
        job: JobContext<'_>,
    ) -> Result<Option<(Self, ConversionReport)>, String> {
        if self.profile.bytes() == p.bytes() {
            return Ok(None);
        }
        if options.rasterizes(&self.profile, p, self.retained_source_count()) {
            let Some(rasterized) = self.rasterizing_sources()? else {
                return Ok(None);
            };
            return rasterized.convert_to_profile_with(p, options, job);
        }
        let report = self.conversion_preflight(p, options, job.checkpoints_only())?;
        job.check()?;
        let pipeline = ColorPipeline::new(
            &self.profile,
            p,
            options.intent,
            options.black_point_compensation,
        )?;
        if pipeline.is_identity() {
            return Ok(None);
        }
        job.reserve(pipeline.scratch_bytes())?;

        // 1. Bake, bottom-up, in the document's CURRENT space: the appearance
        //    being preserved is the one the user is looking at now.
        let mut source = self.clone();
        for outcome in report.adjustments.iter().filter(|a| a.action == "baked") {
            source = source.bake_adjustment(outcome.index, &outcome.name, job)?;
            job.check()?;
        }

        // 2. Convert every layer's pixels. A RAW layer re-renders in the
        //    DESTINATION, so under linear light it resamples through the
        //    destination's tables, which then become the document's
        //    (`rebind_evaluation` finds them current).
        let destination = match &self.linear_light {
            Some(_) => Some(Arc::new(crate::doc_evaluation::ByteTransfer::for_profile(
                p,
            )?)),
            None => None,
        };
        let mut out = source.clone();
        for layer in &mut out.layers {
            if let Some(retained) = layer.effective_source(&source.profile) {
                let rendered = crate::retained_source_render::render8(
                    retained,
                    p,
                    destination.as_deref(),
                    job,
                )?;
                layer.pixels = rendered.rendered.clone();
                layer.offset = rendered.rendered_offset;
                layer.retained_source = Some(rendered);
                continue;
            }
            layer.retained_source = None;
            if layer.kind == LayerKind::Group {
                continue;
            }
            let mut pixels = layer.pixels.to_contiguous_with_job(job)?;
            for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
                if i & 4095 == 0 {
                    job.check()?;
                }
                let rgb = pipeline.color8([px[0], px[1], px[2]]);
                px[..3].copy_from_slice(&rgb);
            }
            layer.pixels = layer.pixels.rebuilt(pixels);
        }
        out.profile = Arc::clone(p);
        if destination.is_some() {
            out.linear_light = destination;
        }
        // The evaluation policy's tables belong to the profile they were built
        // from, so a profile write rebuilds them (`doc_evaluation`).
        out.rebind_evaluation()?;
        Ok(Some((out, report)))
    }

    /// The projection a bake at `index` would install, or the refusal it
    /// raises when that projection is not fully opaque — an adjustment over
    /// transparent pixels has no single appearance to preserve. Run by the
    /// preflight and again by the bake itself, under `job`.
    fn baked_projection(
        &self,
        index: usize,
        name: &str,
        job: JobContext<'_>,
    ) -> Result<RgbaImage, String> {
        let mut excluded = vec![false; self.layers.len()];
        for slot in excluded.iter_mut().skip(index + 1) {
            *slot = true;
        }
        let baked = self.flattened_cut_with_job(
            crate::doc_group::MAX_GROUP_BUFFER_PIXELS,
            Cut::new(&excluded),
            job,
        )?;
        if baked.pixels().any(|p| p[3] != 255) {
            return Err(format!(
                "\"{name}\" sits over transparent pixels, which preserve-appearance cannot bake"
            ));
        }
        Ok(baked)
    }

    /// Replaces the adjustment at `index` with one raster layer holding the
    /// projection of everything up to and including it (module doc).
    fn bake_adjustment(
        &self,
        index: usize,
        name: &str,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let baked = self.baked_projection(index, name, job)?;
        let old = &self.layers[index];
        let mut layer = Layer::new(baked, &format!("{name} (baked)"));
        layer.depth = old.depth;
        layer.link = old.link;
        layer.locks = old.locks;
        let mut out = self.clone();
        out.layers[index] = layer;
        Ok(out)
    }
}
