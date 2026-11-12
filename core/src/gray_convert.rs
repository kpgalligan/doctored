//! Image ▸ Mode ▸ Grayscale and RGB Color on a Gray document (#1 phase
//! 1a): the two whole-document conversions, their plans and their refusals.
//!
//! * **RGB → Gray** discards colour by the rule measured against Photoshop
//!   (`gray_working`'s take): each pixel's relative PCS luminance through the
//!   SOURCE profile, carried into the Gray profile. At 8 and 16 bits (u16
//!   storage) `y =
//!   cmyk_color::document_xyz(source, rgb)[1]`, encoded through the Gray
//!   policy's twin (`GrayPolicy::encode`) and written R = G = B, rounded to
//!   the ×257 grid at depth 8; at 32 bits (linear-light float) `y` is the
//!   source profile's Y row over the linear channels, written R = G = B as
//!   linear `Y` and never encoded, and the float policy is rebuilt for the
//!   twin under the source's tone map (`FloatPolicy::for_profile`, as every
//!   float document's is built for its own profile), so the display, the SDR
//!   exports and the display-referred edits encode through the Gray profile's
//!   curve, not the RGB source's. Alpha is kept. The record tree is carried
//!   by `cmyk_convert_tree::map_tree` (groups, masks, clipping, modes, every
//!   panel property), every leaf in the twin profile; alpha channels, paths,
//!   guides, samplers, metadata, resolution and the light carry, and every
//!   style carries with its colours greyed by the same rule
//!   (`gray_style::grey_records`, phase 2d). Since phase 2e an adjustment
//!   layer the Gray table admits carries too, prepared against the twin, a
//!   Gradient Map's stops greyed by the same rule
//!   (`gray_adjust::converted_description`). Since phase 6a-ii a type,
//!   shape or fill layer converts as its pixels, its description dropped,
//!   as `cmyk_convert` does (the host re-renders it in the Gray result,
//!   its colours greyed), and the layer comps carry, their recorded styles
//!   greyed (`gray_comps`). A Generative Layer converts as its pixels
//!   (the close): from a document in the Gray profile's twin its binding
//!   stays live, the variations' picture codes reading the same in the
//!   result; from any other profile its variations are detached, as Convert
//!   to Profile detaches them on RGB (`doc_gen_variations`), since a picture
//!   is read in the document's profile. The first record (postorder) a Gray
//!   document cannot hold is named ([`first_gray_blocker`]) with
//!   `flatten_required`, and `flatten: true` runs Flatten Image first
//!   (`designs/grayscale-mode.md`).
//! * **Lab → Gray** (#1 Lab phase 5a-ii, `lab_gray`) is the same body
//!   from a Lab Color document: each raster's L\* through the Gray profile's
//!   tone curve (`lab_gray::gray_image`), the record tree, styles (their
//!   colours authored sRGB) and comps carried as above, an adjustment layer
//!   blocked by `lab_gray::first_blocker` and a Blend If channel named as a
//!   Lab document names it.
//! * **Gray → RGB** is Convert to Profile from the twin
//!   (`convert_to_profile_with`, whose float arm takes a 32-bit document):
//!   the policy is dropped and the colours stay neutral.
//!
//! [`gray_image`] is also the artwork fold's (`gray_artwork`, phase 6a-i),
//! whose `keep_neutral` keeps an R = G = B pixel's value (snapped to the
//! ×257 grid at 8 bits) and greys only the coloured ones.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::blend::BlendMode;
use crate::cmyk_convert_tree::{leaf_count, map_tree};
use crate::doc_color_policy::ConversionOptions;
use crate::doc_gen_variations::{GeneratedBinding16, GeneratedPlane16};
use crate::float_document::FloatPolicy;
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::gray_working::GrayWorkingSpace;
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES};
use crate::tile_store::TilePlane;

/// Bytes a converted pixel either Mode conversion charges: the converted
/// output (8 B/px at u16, 16 at float; no transform is prepared, each pixel
/// is computed once) and the result's display preparation the host runs right
/// after. RGB Color charges it beside Convert to Profile's own plan, as the
/// larger of the two, since the conversion's scratch is gone before the
/// display preparation starts. Held by `scripts/gray-admission/measure.py` to
/// ⌈1.25 × the measured peak⌉ (`to-gray-16` and `to-rgb-16` both measured
/// 769 MB at 24 MP, 32 B/px; 40 cleared it by 0.1 %, so 41 keeps the plan
/// above the run-to-run spread); a miss raises this term.
pub(crate) const TO_GRAY_BYTES_PER_PIXEL: u64 = 41;

/// The first record (postorder) a Gray document cannot hold, as the
/// sentence the host shows beside its "flatten the image" choice: an
/// adjustment layer the Gray table refuses (`gray_adjust::record_refusal`:
/// the `Unavailable` ops, the RGB-only ops, Levels or Curves with unlike
/// channels; named first, in its own sentence), then for every layer — a
/// carried adjustment layer too (phase 2e) — a layer effect in a blend mode outside
/// `gray_document::GRAY_BLEND_MODES`, a Blend If on a channel other than
/// Gray, a layer in a blend mode outside that list (Darker Color, Lighter
/// Color, Hue, Saturation, Color, Luminosity: read off the list, so the
/// blocker cannot drift from it; Pass Through is a group's), a placed image
/// or a warp. A Generative Layer converts (module doc). A style carries
/// (blending options since phase 1b; effects, their colours greyed by
/// `gray_style`, and a Gray Blend If since phase 2d), and a type, shape or
/// fill layer converts as its pixels, its description dropped (phase 6a-ii;
/// the host re-renders it).
pub(crate) fn first_gray_blocker(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    first_gray_blocker_with(layers, &crate::style::BlendIfChannel::display_name, false)
}

/// [`first_gray_blocker`] with a Blend If channel named by `channel_name`
/// (a Lab document's Red, Green and Blue are Lightness, a and b) and, when
/// `lab`, every adjustment layer a blocker: a Lab record's settings act on
/// L\*a\*b\* codes, not Gray codes (`lab_gray::first_blocker`, #1 Lab
/// phase 5a-ii).
pub(crate) fn first_gray_blocker_with(
    layers: &[PrecisionDocumentLayer],
    channel_name: &dyn Fn(crate::style::BlendIfChannel) -> &'static str,
    lab: bool,
) -> Option<String> {
    for layer in layers {
        if let Some(reason) = layer
            .children()
            .and_then(|children| first_gray_blocker_with(children, channel_name, lab))
        {
            return Some(reason);
        }
        let mode = layer.blend_mode;
        let refused_mode = mode != BlendMode::PassThrough
            && !crate::gray_document::GRAY_BLEND_MODES.contains(&mode);
        let effect_mode = layer.style.as_deref().and_then(|s| {
            s.effects.iter().find_map(|effect| {
                crate::float_document::effect_blend_modes(effect)
                    .into_iter()
                    .flatten()
                    .find(|m| crate::gray_document::blend_refusal(*m, false).is_some())
            })
        });
        let blend_if_channel = layer
            .blend_if
            .iter()
            .chain(layer.style.as_ref().and_then(|s| s.blend_if.as_ref()))
            .flat_map(|b| b.ranges())
            .map(|r| r.channel)
            .find(|c| *c != crate::style::BlendIfChannel::Gray);
        if lab && matches!(layer.content, Content::Adjustment(_)) {
            return Some(format!(
                "“{}” is a Lab Color adjustment layer, which a Grayscale document cannot hold; flatten the image to convert it",
                layer.name
            ));
        }
        if let Content::Adjustment(state) = &layer.content {
            if let Some(reason) = crate::gray_adjust::record_refusal(state.description()) {
                return Some(format!(
                    "“{}” cannot become Grayscale: {reason} Flatten the image to convert it",
                    layer.name
                ));
            }
        }
        let what = if let Some(mode) = effect_mode {
            Some(format!(
                "a layer with a layer effect in the {} blend mode",
                crate::cmyk_document::mode_name(mode)
            ))
        } else if let Some(channel) = blend_if_channel {
            Some(format!(
                "a layer with Blend If on the {} channel",
                channel_name(channel)
            ))
        } else if refused_mode {
            Some(format!(
                "a layer in the {} blend mode",
                crate::cmyk_document::mode_name(mode)
            ))
        } else if layer.retained_source.is_some() {
            Some("a placed image".to_string())
        } else if layer.warp.is_some() {
            Some("a warped layer".to_string())
        } else {
            None
        };
        if let Some(what) = what {
            return Some(format!(
                "“{}” is {what}, which a Grayscale document cannot hold; flatten the image to convert it",
                layer.name
            ));
        }
    }
    None
}

/// The RGB → Gray options (module doc).
struct GrayOptions {
    space: Option<GrayWorkingSpace>,
    flatten: bool,
    depth: Option<GrayDepth>,
}

impl GrayOptions {
    /// `{"space"?,"flatten"?:bool,"depth"?:8|16}`; an unknown key refuses.
    fn parse(text: &str) -> Result<Self, String> {
        let mut out = GrayOptions {
            space: None,
            flatten: false,
            depth: None,
        };
        if text.trim().is_empty() {
            return Ok(out);
        }
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("mode options are not valid JSON: {e}"))?;
        let object: &Map<String, Value> = value
            .as_object()
            .ok_or("mode options must be a JSON object")?;
        for key in object.keys() {
            if !matches!(key.as_str(), "space" | "flatten" | "depth") {
                return Err(format!("mode options have an unknown key \"{key}\""));
            }
        }
        if let Some(space) = object.get("space").filter(|v| !v.is_null()) {
            let name = space.as_str().ok_or("mode option space must be a string")?;
            out.space = Some(
                GrayWorkingSpace::from_name(name)
                    .ok_or_else(|| format!("\"{name}\" is not a Gray working space"))?,
            );
        }
        match object.get("flatten") {
            None | Some(Value::Null) => {}
            Some(Value::Bool(v)) => out.flatten = *v,
            Some(_) => return Err("mode option flatten must be a boolean".into()),
        }
        if let Some(depth) = object.get("depth").filter(|v| !v.is_null()) {
            out.depth = Some(
                depth
                    .as_u64()
                    .filter(|bits| matches!(bits, 8 | 16))
                    .and_then(|bits| GrayDepth::from_bits(bits as u32))
                    .ok_or("mode option depth must be 8 or 16")?,
            );
        }
        Ok(out)
    }
}

/// The RGB Color options: `{"intent"?,"black_point_compensation"?}`; an
/// unknown key refuses.
pub(crate) fn rgb_options(text: &str) -> Result<ConversionOptions, String> {
    if text.trim().is_empty() {
        return Ok(ConversionOptions::DEFAULT);
    }
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("mode options are not valid JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or("mode options must be a JSON object")?;
    for key in object.keys() {
        if !matches!(key.as_str(), "intent" | "black_point_compensation") {
            return Err(format!("mode options have an unknown key \"{key}\""));
        }
    }
    ConversionOptions::from_value(&value)
}

impl PrecisionDocument {
    /// Why this document cannot become Gray even with a flatten, or `None`.
    fn to_gray_hard_refusal(&self, options: &GrayOptions) -> Option<String> {
        if self.is_gray() {
            return Some("the document is already Grayscale".into());
        }
        if self.is_cmyk() {
            return Some(
                "Grayscale converts an RGB or Lab Color document; convert to RGB Color first"
                    .into(),
            );
        }
        if self.is_float() && options.depth.is_some() {
            return Some(
                "a 32-bit document converts to 32-bit Grayscale; depth does not apply".into(),
            );
        }
        None
    }

    /// The first record of `layers` this document's Gray conversion cannot
    /// carry: `lab_gray::first_blocker` on a Lab document, else
    /// [`first_gray_blocker`].
    fn to_gray_blocker(&self, layers: &[PrecisionDocumentLayer]) -> Option<String> {
        if self.is_lab() {
            crate::lab_gray::first_blocker(layers)
        } else {
            first_gray_blocker(layers)
        }
    }

    /// The destination policy: `icc`, else the option's space.
    fn gray_destination(
        &self,
        icc: Option<&[u8]>,
        options: &GrayOptions,
    ) -> Result<GrayPolicy, String> {
        let depth = if self.is_float() {
            GrayDepth::ThirtyTwo
        } else {
            options.depth.unwrap_or(GrayDepth::Sixteen)
        };
        match (icc, options.space) {
            (Some(bytes), _) => GrayPolicy::new(bytes, depth),
            (None, Some(space)) => GrayPolicy::for_space(space, depth),
            (None, None) => Err("Grayscale needs a Gray profile or space to convert into".into()),
        }
    }

    /// The raster pixels of every leaf, at every depth (a CMYK leaf's too,
    /// so a CMYK document's plan answers the Gray stage of its route through
    /// RGB).
    pub(crate) fn gray_raster_pixels(layers: &[PrecisionDocumentLayer]) -> u64 {
        layers
            .iter()
            .map(|layer| match &layer.content {
                Content::Raster { image, .. } => {
                    let (w, h) = image.dimensions();
                    u64::from(w) * u64::from(h)
                }
                Content::CmykRaster { image, .. } => {
                    let (w, h) = image.dimensions();
                    u64::from(w) * u64::from(h)
                }
                Content::Group(children) | Content::PassThrough(children) => {
                    Self::gray_raster_pixels(children)
                }
                _ => 0,
            })
            .sum()
    }

    /// The RGB → Gray plan: [`TO_GRAY_BYTES_PER_PIXEL`] a converted pixel
    /// (or the larger Flatten Image's render first), the greyed style tiles
    /// (`gray_style::tile_bytes`), the preparation of each carried adjustment
    /// layer (`PrecisionAdjustmentState::prepare_scratch_bytes` against the
    /// twin; none when flattening), the carried comps' greyed styles
    /// (`gray_comps::carried_bytes`; none when flattening), the destination's
    /// twin and encoder, and bookkeeping. No colour plan: the conversion prepares
    /// no transform, it reads each source pixel's `Y` and writes the plane once.
    /// A Lab source adds its own term (`lab_gray::extra_scratch_bytes`).
    fn to_gray_scratch(&self, policy: Option<&GrayPolicy>, flatten: bool) -> u64 {
        let canvas = u64::from(self.width) * u64::from(self.height);
        let pixels = if flatten {
            canvas
        } else {
            Self::gray_raster_pixels(self.root_layers())
        };
        // A float source also builds the twin's float policy, one more encoder.
        let encoders = if self.is_float() { 2 } else { 1 };
        let profile = policy.map_or(0, |p| {
            p.twin().bytes().len() as u64
                + p.bytes().len() as u64
                + encoders * crate::icc_transform::PrecisionToneEncoder::scratch_bytes()
        });
        let flattening = if flatten {
            self.render_plan().map(|p| p.scratch_bytes).unwrap_or(0)
        } else {
            0
        };
        // The greyed copy of every custom tile the style pass makes
        // (`gray_style`); a flatten bakes the styles, so it greys none.
        let tiles = if flatten {
            0
        } else {
            crate::gray_style::tile_bytes(self.root_layers())
        };
        let adjustments = match (policy, flatten) {
            (Some(policy), false) => Self::carried_adjustment_bytes(self.root_layers(), policy),
            _ => 0,
        };
        // A flatten clears the comps, so it carries none.
        let comps = if flatten {
            0
        } else {
            crate::gray_comps::carried_bytes(&self.comps)
        };
        let lab = if self.is_lab() {
            crate::lab_gray::extra_scratch_bytes(pixels)
        } else {
            0
        };
        flattening.max(pixels.saturating_mul(TO_GRAY_BYTES_PER_PIXEL))
            + tiles
            + adjustments
            + comps
            + profile
            + lab
            + BOOKKEEPING_BYTES
    }

    /// The preparation of every adjustment record of `layers` (recursing
    /// groups) carried into `policy`'s twin (the plan's term, module doc).
    fn carried_adjustment_bytes(layers: &[PrecisionDocumentLayer], policy: &GrayPolicy) -> u64 {
        layers
            .iter()
            .map(|layer| match &layer.content {
                Content::Adjustment(state) => {
                    crate::precision_adjustment::PrecisionAdjustmentState::prepare_scratch_bytes(
                        state.description(),
                        policy.twin(),
                    )
                    .unwrap_or(0)
                }
                Content::Group(children) | Content::PassThrough(children) => {
                    Self::carried_adjustment_bytes(children, policy)
                }
                _ => 0,
            })
            .fold(0u64, u64::saturating_add)
    }

    /// `dr_precision_doc_convert_to_gray_plan`'s answer (CMYK's keys).
    pub fn convert_to_gray_plan(&self, icc: Option<&[u8]>, options: &str) -> Result<Value, String> {
        let options = GrayOptions::parse(options)?;
        let destination = self.gray_destination(icc, &options);
        let blocker = self.to_gray_blocker(self.root_layers());
        let refusal = self
            .to_gray_hard_refusal(&options)
            .or(destination.as_ref().err().cloned())
            .or(blocker.clone().filter(|_| !options.flatten));
        let flattens = blocker.is_some() && options.flatten;
        Ok(json!({
            "working_memory_plan_bytes": self.to_gray_scratch(destination.as_ref().ok(), flattens),
            "refusal": refusal,
            "flatten_required": blocker.is_some(),
            "flattens": flattens,
        }))
    }

    /// RGB → Gray (module doc).
    pub fn convert_to_gray(
        &self,
        icc: Option<&[u8]>,
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let options = GrayOptions::parse(options)?;
        if let Some(reason) = self.to_gray_hard_refusal(&options) {
            return Err(reason);
        }
        let policy = self.gray_destination(icc, &options)?;
        let blocker = self.to_gray_blocker(self.root_layers());
        if let (Some(reason), false) = (&blocker, options.flatten) {
            return Err(reason.clone());
        }
        let scratch = self.to_gray_scratch(Some(&policy), blocker.is_some());
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let flattened;
        let source = if blocker.is_some() {
            flattened = self
                .flatten_image(limit, other, job.range(0.0, 0.3))?
                .ok_or("flattening the image changed nothing")?;
            &flattened
        } else {
            self
        };
        let from = source.profile().clone();
        // A Lab document's style tiles and effect colours are authored sRGB.
        let tiles_in = if source.is_lab() {
            IccProfile::srgb()
        } else {
            from.clone()
        };
        let count = leaf_count(source.root_layers()).max(1) as f64;
        let mut index = 0usize;
        let mut layers = map_tree(source.root_layers(), &mut |layer| {
            // An admitted adjustment layer is carried (phase 2e), its
            // description converted by the Mode rule; `map_tree`'s carry
            // keeps its mask, clipping, mode and style.
            if let Content::Adjustment(state) = &layer.content {
                let description =
                    crate::gray_adjust::converted_description(state.description(), &from, &policy)?;
                return PrecisionDocumentLayer::adjustment(
                    &layer.name,
                    &description,
                    layer.opacity,
                    layer.visible,
                    policy.twin(),
                );
            }
            let Content::Raster { image, offset } = &layer.content else {
                return Err(source
                    .to_gray_blocker(std::slice::from_ref(layer))
                    .unwrap_or_else(|| "A non-raster record cannot become Grayscale".into()));
            };
            let part = job.range(
                0.3 + 0.7 * index as f64 / count,
                0.3 + 0.7 * (index + 1) as f64 / count,
            );
            index += 1;
            let converted = if source.is_lab() {
                crate::lab_gray::gray_image(image, &policy, part)?
            } else {
                gray_image(image, &from, &policy, false, part)?
            };
            let mut record = if source.is_float() {
                PrecisionDocumentLayer::raster_float(
                    &layer.name,
                    converted,
                    *offset,
                    layer.opacity,
                    layer.visible,
                )
            } else {
                PrecisionDocumentLayer::raster(
                    &layer.name,
                    converted,
                    *offset,
                    layer.opacity,
                    layer.visible,
                )
            }?;
            // A Generative Layer from the twin stays live (module doc): its
            // tag, and its binding to the converted raster in the twin. From
            // any other profile it is the plain converted raster.
            if let Some(binding) = layer.effective_generated(&from, false) {
                if from.bytes() == policy.twin().bytes() {
                    if let Content::Raster { image, .. } = &record.content {
                        record.description = layer.description.clone();
                        record.generated = Some(GeneratedBinding16 {
                            table: binding.table,
                            rendered: GeneratedPlane16::Rgb(image.clone()),
                            rendered_offset: *offset,
                            profile: policy.twin().clone(),
                        });
                    }
                }
            }
            Ok(record)
        })?;
        crate::gray_style::grey_records(&mut layers, &tiles_in, &policy)?;
        // The float policy follows the profile the result carries (module doc).
        let float = source
            .float
            .as_ref()
            .map(|float| FloatPolicy::for_profile(policy.twin(), float.tone_map()).map(Arc::new))
            .transpose()?;
        let mut out = PrecisionDocument::build(
            source.width,
            source.height,
            policy.twin().clone(),
            source.metadata.clone(),
            source.resolution,
            layers,
            source.channels().to_vec(),
            float,
        )?;
        out.guides.clone_from(&source.guides);
        out.ruler_origin = source.ruler_origin;
        out.color_samplers.clone_from(&source.color_samplers);
        out.global_light = source.global_light;
        out.paths.set_entries(source.paths.entries.to_vec());
        out.comps = crate::gray_comps::carried_comps(&source.comps, &tiles_in, &policy)?;
        out.background = source.background;
        let out = out
            .seeding_vector_masks(source.vector_mask_seeds(Clone::clone), limit, other)
            .with_gray(Arc::new(policy))?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The RGB destination of a Gray document: `icc`, an RGB profile a
    /// document can hold.
    pub(crate) fn gray_rgb_destination(icc: &[u8]) -> Result<Arc<IccProfile>, String> {
        let profile = Arc::new(IccProfile::parse(icc).ok_or(
            "RGB Color needs an RGB profile a Doctored document can hold to convert into",
        )?);
        if !profile.can_encode() {
            return Err(format!(
                "the RGB profile \"{}\" cannot be converted into",
                profile.name()
            ));
        }
        Ok(profile)
    }

    /// `dr_precision_doc_convert_gray_to_rgb_plan`'s answer.
    pub fn convert_gray_to_rgb_plan(&self, icc: &[u8], options: &str) -> Result<Value, String> {
        let conversion = rgb_options(options)?;
        let destination = Self::gray_rgb_destination(icc);
        let rgb = self.without_gray();
        let plan = destination
            .as_ref()
            .ok()
            .map(|profile| rgb.conversion_plan(profile, &conversion));
        let refusal = if !self.is_gray() {
            Some("the document is not Grayscale".to_string())
        } else {
            destination
                .as_ref()
                .err()
                .cloned()
                .or(plan.as_ref().and_then(|p| p.as_ref().err().cloned()))
        };
        // Under options other than the defaults the conversion reserves the
        // prepared pipeline's own scratch (`converting`'s `ColorPipeline`
        // arm), which a near-identity pair (sGray's twin → sRGB) can make
        // larger than the colour plan; the plan charges the larger.
        let pipeline = match (&destination, conversion.is_default() || rgb.is_float()) {
            (Ok(profile), false) => crate::icc_intent::ColorPipeline::new(
                rgb.profile(),
                profile,
                conversion.intent,
                conversion.black_point_compensation,
            )
            .map_or(0, |p| p.scratch_bytes()),
            _ => 0,
        };
        // …and the RGB result's display preparation afterwards
        // ([`TO_GRAY_BYTES_PER_PIXEL`], `to-rgb-16`).
        let display =
            Self::gray_raster_pixels(self.root_layers()).saturating_mul(TO_GRAY_BYTES_PER_PIXEL);
        let bytes = plan
            .and_then(Result::ok)
            .map_or(0, |p| p.scratch_bytes)
            .max(pipeline)
            .max(display)
            + BOOKKEEPING_BYTES;
        Ok(json!({
            "working_memory_plan_bytes": bytes,
            "refusal": refusal,
            "flatten_required": false,
            "flattens": false,
        }))
    }

    /// Gray → RGB (module doc): Convert to Profile from the twin, the policy
    /// dropped even when no pixel changes.
    pub fn convert_gray_to_rgb(
        &self,
        icc: &[u8],
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let conversion = rgb_options(options)?;
        if !self.is_gray() {
            return Err("the document is not Grayscale".into());
        }
        let destination = Self::gray_rgb_destination(icc)?;
        let rgb = self.without_gray();
        match rgb.convert_to_profile_with(&destination, &conversion, limit, other, control)? {
            Some((document, _)) => Ok(document),
            // No pixel changes (the same bytes, or an identity pipeline such as
            // sGray's twin into sRGB): the document takes the destination's
            // name by assignment, which keeps every sample.
            None => Ok(rgb
                .assign_profile(&destination, limit, other, None)?
                .unwrap_or(rgb)),
        }
    }
}

/// One RGB image's luminance as a Gray layer image in `policy`'s twin
/// (module doc): u16 storage → the encoded gray code, float → linear `Y`.
/// Shared with the clipboard's Gray conversion (`gray_clipboard`) and the
/// artwork fold (`gray_artwork`), which passes `keep_neutral`: a pixel with
/// R = G = B keeps its R (bit for bit at 16 and 32 bits, on the ×257 grid
/// at 8) instead of being re-encoded, since `from` is then the twin, whose
/// neutral value already is its gray code.
pub(crate) fn gray_image(
    image: &PrecisionImage,
    from: &Arc<IccProfile>,
    policy: &GrayPolicy,
    keep_neutral: bool,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = image.dimensions();
    let twin = policy.twin().clone();
    match image.storage() {
        crate::precision_image::PrecisionStorage::Rgba16Unorm => {
            let eight = policy.depth() == GrayDepth::Eight;
            let code = |pixel: [u16; 4]| {
                if keep_neutral && pixel[0] == pixel[1] && pixel[1] == pixel[2] {
                    if eight {
                        crate::cmyk_profile::eight(pixel[0])
                    } else {
                        pixel[0]
                    }
                } else {
                    let rgb = [pixel[0], pixel[1], pixel[2]].map(|c| f64::from(c) / 65535.0);
                    policy.luminance_code(from, rgb)
                }
            };
            gray_rows_u16(image, policy, &code, job)
        }
        crate::precision_image::PrecisionStorage::Rgba32Float => {
            // Refuses a source with no matrix/TRC model before any row.
            GrayPolicy::linear_luminance(from, [0.0; 3])?;
            let plane = TilePlane::<[f32; 4]>::from_rows_with_job(
                w,
                h,
                |y, row| {
                    let mut x = 0usize;
                    for (_, run) in image.row_segments(y, 0, w) {
                        let PrecisionRun::Rgba32(run) = run else {
                            return Err("Grayscale requires a 16-bit or float document".into());
                        };
                        for pixel in run {
                            let luminance =
                                if keep_neutral && pixel[0] == pixel[1] && pixel[1] == pixel[2] {
                                    pixel[0]
                                } else {
                                    let rgb = [pixel[0], pixel[1], pixel[2]].map(f64::from);
                                    GrayPolicy::linear_luminance(from, rgb)? as f32
                                };
                            row[x] = [luminance, luminance, luminance, pixel[3]];
                            x += 1;
                        }
                    }
                    job.every(y as usize * w as usize)
                },
                job,
            )?;
            PrecisionImage::from_rgba32_tiles_decoded(plane, twin)
        }
    }
}

/// The u16 arm's row loop of [`gray_image`] (and of `lab_gray::gray_image`):
/// each pixel's gray `code` written R = G = B, alpha kept (on the ×257 grid
/// at depth 8), the result in `policy`'s twin.
pub(crate) fn gray_rows_u16(
    image: &PrecisionImage,
    policy: &GrayPolicy,
    code: &dyn Fn([u16; 4]) -> u16,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = image.dimensions();
    let eight = policy.depth() == GrayDepth::Eight;
    let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
        w,
        h,
        |y, row| {
            let mut x = 0usize;
            for (_, run) in image.row_segments(y, 0, w) {
                let PrecisionRun::Rgba16(run) = run else {
                    return Err("Grayscale requires a 16-bit or float document".into());
                };
                for pixel in run {
                    let gray = code(*pixel);
                    let alpha = if eight {
                        ((u32::from(pixel[3]) + 128) / 257 * 257) as u16
                    } else {
                        pixel[3]
                    };
                    row[x] = [gray, gray, gray, alpha];
                    x += 1;
                }
            }
            job.every(y as usize * w as usize)
        },
        job,
    )?;
    PrecisionImage::from_rgba16_tiles_unseeded(plane, policy.twin().clone())
}
