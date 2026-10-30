//! A Duotone document out of Duotone (#547, `designs/duotone-mode.md`):
//! the render through the inks, its plan, and Image ▸ Mode ▸ RGB Color.
//! Grayscale and Bitmap are `duotone_gray`'s, through the same tree
//! assembly ([`PrecisionDocument::duotone_leaves_mapped`]).
//!
//! * **The render** ([`PrecisionDocument::duotone_render`]) is an RGB
//!   document in the twin: `cmyk_convert_tree::map_tree` over the record
//!   tree, each raster leaf through the Duotone view
//!   (`duotone_document::duotone_view_mapped`, the ONE mix: `duotone_ink`
//!   through the view's table, built once a render on its source, phase 2a)
//!   into an RGBA16 raster, its description and
//!   generated binding dropped (a new raster carries neither). Groups, masks,
//!   clipping, blend modes and a Gray Blend If carry (`map_tree`'s `carry`),
//!   and so do the comps, alpha channels, guides, the ruler origin, paths,
//!   colour samplers, the light, metadata and resolution; the Gray policy and
//!   its inks do not. Each layer is rendered on its own, so a layered
//!   document's blend modes composite rendered colours, as a Duotone →
//!   RGB conversion of layers does.
//! * **Blockers** ([`first_render_blocker`], postorder, as
//!   `gray_convert::first_gray_blocker` walks): an adjustment layer, whose
//!   settings act on gray codes, and a layer style with effects, whose
//!   colours composite in gray. Either is the plan's `refusal` with
//!   `flatten_required`; `flatten: true` runs Flatten Image first (the gray
//!   composite, then the view), as Mode ▸ Grayscale does.
//! * **The plan**: the raster pixels (the canvas when flattening) ×
//!   [`RENDER_BYTES_PER_PIXEL`], the view's table (`VIEW_TABLE_BYTES`),
//!   Flatten Image's render scratch when flattening, and bookkeeping. Held by
//!   `scripts/duotone-admission/measure.py`; a miss raises this term or the
//!   view's.
//! * **RGB Color** ([`PrecisionDocument::convert_duotone_to_rgb`]): the
//!   render, then Convert to Profile from the twin into `icc` (Gray → RGB's
//!   body: `gray_convert::rgb_options`, `gray_rgb_destination`, an identity
//!   pipeline assigning the destination's name), admitted beside the source
//!   it still holds (phase 2a). Its plan is the render's
//!   plus Gray → RGB's. CMYK, Lab and Indexed Color take the rendered
//!   document through their own RGB routes in the host.

use serde_json::{json, Map, Value};

use crate::cmyk_convert_tree::{leaf_count, map_tree};
use crate::duotone_document::VIEW_TABLE_BYTES;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, BOOKKEEPING_BYTES};

/// Bytes the render charges a raster pixel: its RGBA16 output. A miss of the
/// admission drive raises this term.
pub(crate) const RENDER_BYTES_PER_PIXEL: u64 = 8;

/// The refusal of a document that is not Duotone.
const NOT_DUOTONE: &str = "the Duotone render takes a Duotone document";

/// The first record (postorder) the render cannot carry, as the sentence the
/// host shows beside its "flatten the image" choice (module doc).
pub(crate) fn first_render_blocker(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    for layer in layers {
        if let Some(reason) = layer.children().and_then(first_render_blocker) {
            return Some(reason);
        }
        let what = if matches!(layer.content, Content::Adjustment(_)) {
            Some("an adjustment layer")
        } else if layer
            .style
            .as_deref()
            .is_some_and(|s| !s.effects.is_empty())
        {
            Some("a layer with a layer style")
        } else {
            None
        };
        if let Some(what) = what {
            return Some(format!(
                "“{}” is {what}, which a Duotone document's conversion through its inks cannot carry; flatten the image to convert it",
                layer.name
            ));
        }
    }
    None
}

/// `options` without its `flatten` key, and that key (false when absent).
pub(crate) fn split_flatten(options: &str) -> Result<(bool, String), String> {
    if options.trim().is_empty() {
        return Ok((false, String::new()));
    }
    let value: Value = serde_json::from_str(options)
        .map_err(|e| format!("mode options are not valid JSON: {e}"))?;
    let mut object: Map<String, Value> = value
        .as_object()
        .cloned()
        .ok_or("mode options must be a JSON object")?;
    let flatten = match object.remove("flatten") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(v)) => v,
        Some(_) => return Err("mode option flatten must be a boolean".into()),
    };
    Ok((flatten, Value::Object(object).to_string()))
}

impl PrecisionDocument {
    /// The render's scratch (module doc); `flattens` when Flatten Image runs
    /// first.
    fn duotone_render_scratch(&self, flattens: bool) -> u64 {
        let pixels = if flattens {
            u64::from(self.width) * u64::from(self.height)
        } else {
            Self::gray_raster_pixels(self.root_layers())
        };
        let flattening = if flattens {
            self.render_plan().map(|p| p.scratch_bytes).unwrap_or(0)
        } else {
            0
        };
        pixels
            .saturating_mul(RENDER_BYTES_PER_PIXEL)
            .saturating_add(VIEW_TABLE_BYTES)
            .saturating_add(flattening)
            .saturating_add(BOOKKEEPING_BYTES)
    }

    /// The render's plan:
    /// `{"working_memory_plan_bytes","refusal","flatten_required","flattens"}`.
    pub fn duotone_render_plan(&self, flatten: bool) -> Result<Value, String> {
        let blocker = first_render_blocker(self.root_layers());
        let refusal = if self.is_duotone() {
            blocker.clone().filter(|_| !flatten)
        } else {
            Some(NOT_DUOTONE.to_string())
        };
        let flattens = blocker.is_some() && flatten;
        Ok(json!({
            "working_memory_plan_bytes": self.duotone_render_scratch(flattens),
            "refusal": refusal,
            "flatten_required": blocker.is_some(),
            "flattens": flattens,
        }))
    }

    /// The render (module doc): an RGB document in the twin, each raster
    /// leaf through the inks. Refuses a document that is not Duotone, a
    /// blocker without `flatten`, scratch past `limit` and a cancel.
    pub fn duotone_render(
        &self,
        flatten: bool,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.duotone_render_job(flatten, limit, other, job)
    }

    /// [`Self::duotone_render`]'s body under `job`.
    fn duotone_render_job(
        &self,
        flatten: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        if !self.is_duotone() {
            return Err(NOT_DUOTONE.into());
        }
        let blocker = first_render_blocker(self.root_layers());
        if let (Some(reason), false) = (&blocker, flatten) {
            return Err(reason.clone());
        }
        let flattens = blocker.is_some();
        let scratch = self.duotone_render_scratch(flattens);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let flattened;
        let source = if flattens {
            flattened = self
                .flatten_image(limit, other, job.range(0.0, 0.3))?
                .ok_or("flattening the image changed nothing")?;
            &flattened
        } else {
            self
        };
        // One table for every leaf (phase 2a): built on `source`, whose
        // policy (the flattened document's too) carries the inks.
        let table = source.duotone_view_table(job.range(0.3, 0.32))?;
        let out = source.duotone_leaves_mapped(
            limit,
            other,
            job.range(0.32, 1.0),
            &mut |image, part| source.duotone_view_mapped(image, &table, part),
        )?;
        job.report(1.0, 5)?;
        Ok(out.without_gray())
    }

    /// The tree assembly the render and the Grayscale conversion share
    /// (`duotone_gray`): `map_tree` over this document's records, each raster
    /// leaf's image through `map` (its slice of `job`) into a new raster of
    /// the same name, offset, opacity and visibility, its description and
    /// generated binding dropped; then the chrome carried (module doc). The
    /// result is in this document's profile and carries no Gray policy; a
    /// record that is not a raster refuses by [`first_render_blocker`].
    pub(crate) fn duotone_leaves_mapped(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
        map: &mut dyn FnMut(&PrecisionImage, JobContext<'_>) -> Result<PrecisionImage, String>,
    ) -> Result<Self, String> {
        let count = leaf_count(self.root_layers()).max(1) as f64;
        let mut index = 0usize;
        let layers = map_tree(self.root_layers(), &mut |layer| {
            let Content::Raster { image, offset } = &layer.content else {
                return Err(
                    first_render_blocker(std::slice::from_ref(layer)).unwrap_or_else(|| {
                        "a record that is not a raster cannot be rendered through the inks".into()
                    }),
                );
            };
            let part = job.range(index as f64 / count, (index + 1) as f64 / count);
            index += 1;
            let mapped = map(image, part)?;
            PrecisionDocumentLayer::raster(
                &layer.name,
                mapped,
                *offset,
                layer.opacity,
                layer.visible,
            )
        })?;
        let mut out = PrecisionDocument::build(
            self.width,
            self.height,
            self.profile().clone(),
            self.metadata.clone(),
            self.resolution,
            layers,
            self.channels().to_vec(),
            None,
        )?;
        out.guides.clone_from(&self.guides);
        out.ruler_origin = self.ruler_origin;
        out.color_samplers.clone_from(&self.color_samplers);
        out.global_light = self.global_light;
        out.paths.set_entries(self.paths.entries.to_vec());
        out.comps.clone_from(&self.comps);
        Ok(out.seeding_vector_masks(self.vector_mask_seeds(Clone::clone), limit, other))
    }

    /// Duotone → RGB's plan (module doc): the render's plus Gray → RGB's,
    /// under `{"intent"?,"black_point_compensation"?,"flatten"?}`.
    pub fn convert_duotone_to_rgb_plan(&self, icc: &[u8], options: &str) -> Result<Value, String> {
        let (flatten, rest) = split_flatten(options)?;
        let render = self.duotone_render_plan(flatten)?;
        let rgb = self.convert_gray_to_rgb_plan(icc, &rest)?;
        let bytes = |plan: &Value| plan["working_memory_plan_bytes"].as_u64().unwrap_or(0);
        let refusal = if render["refusal"].is_null() {
            rgb["refusal"].clone()
        } else {
            render["refusal"].clone()
        };
        Ok(json!({
            "working_memory_plan_bytes": bytes(&render).saturating_add(bytes(&rgb)),
            "refusal": refusal,
            "flatten_required": render["flatten_required"],
            "flattens": render["flattens"],
        }))
    }

    /// Duotone → RGB (module doc): the render, then Convert to Profile from
    /// the twin into `icc`.
    pub fn convert_duotone_to_rgb(
        &self,
        icc: &[u8],
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let (flatten, rest) = split_flatten(options)?;
        let conversion = crate::gray_convert::rgb_options(&rest)?;
        let destination = Self::gray_rgb_destination(icc)?;
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let rendered = self.duotone_render_job(flatten, limit, other, job.range(0.0, 0.6))?;
        // The second stage runs while this document is still held (the
        // caller's source), so it is admitted beside it (phase 2a).
        let beside = other.saturating_add(self.retained_bytes());
        match rendered.converting(
            &destination,
            &conversion,
            limit,
            beside,
            job.range(0.6, 1.0),
        )? {
            Some((document, _)) => Ok(document),
            // An identity pipeline: the destination's name by assignment,
            // every sample kept (Gray → RGB's rule).
            None => Ok(rendered
                .assign_profile(&destination, limit, beside, None)?
                .unwrap_or(rendered)),
        }
    }
}
