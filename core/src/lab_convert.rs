//! Image ▸ Mode ▸ Lab Color and RGB Color on a Lab document (#1 phase 1):
//! the two whole-document conversions, their plans and their refusals.
//!
//! One rule both ways: each u16 pixel of every raster runs through a
//! `ColorPipeline` (the general path, the Lab twin being a LUT side), its
//! output clamped to 0…1 and quantized — onto the ×257 grid for an 8-bit
//! Lab document — with alpha kept (rounded to the same grid at 8 bits).
//!
//! * **RGB / Gray → Lab** is relative colorimetric from the document profile
//!   into the twin, no BPC ([`to_lab_pipeline`]); a Gray document converts
//!   from its RGB twin, which its storage is in. a\*/b\* past −128…127 clip
//!   (the PCS encoding clamps). The record tree is carried by
//!   `cmyk_convert_tree::map_tree` (groups, masks, clipping, every panel
//!   property), every leaf in the twin; alpha channels, paths, guides,
//!   samplers, metadata, resolution and the light carry, as
//!   `gray_convert::convert_to_gray` carries them. Since phase 2d an
//!   adjustment layer carries too, its description unchanged and rebound to
//!   the twin (which binds its Lab form), unless
//!   `lab_adjust::lab_carry_refusal` names it (an op Lab refuses, a
//!   pixels-only op); since phase 2f a Red, Green or Blue Levels or Curves
//!   stage carries as a Lightness, a or b one. The first record
//!   (postorder) a Lab document cannot hold is named
//!   ([`first_lab_blocker`]) with `flatten_required`, and `flatten: true`
//!   runs Flatten Image first. Since phase 2e a layer style with effects
//!   carries unchanged (`lab_style`: its colours convert at composite),
//!   unless `lab_style::style_refusal` names an effect mode (since 2f's
//!   second part a pattern carries too, its sRGB tile unchanged).
//!   Since phase 2f a Blend If (the layer's or the style's) carries, its red,
//!   green and blue ranges becoming Lightness, a and b, unless
//!   `lab_style::blend_if_channel_refusal` names a Gray or plate range.
//!   Since phase 6a-i a type, shape or fill layer converts as its pixels,
//!   its description dropped (as `cmyk_convert_tree` and `gray_convert` do),
//!   both ways; the host re-renders it from its description in the
//!   destination (`lab_artwork`'s module doc). Since phase 6a-ii the layer
//!   comps carry unchanged (`lab_comps`); a flatten clears them first. Since
//!   the close a Generative Layer converts as its pixels and its variations
//!   detach, as Convert to Profile detaches them on RGB.
//! * **Lab → RGB** runs the twin into the destination profile under the
//!   request's intent and black point compensation; the policy drops, and
//!   an adjustment layer carries, rebound to the destination (phase 2b), a
//!   Levels or Curves layer on the Lightness, a or b channel too (phase 2f:
//!   its stages become Red, Green and Blue by channel index). A layer style
//!   and a Blend If carry unchanged (phase 2f: Lightness, a and b ranges
//!   become Red, Green and Blue), and so do the layer comps (phase 6a-ii).
//! * **Lab → Gray** is `gray_convert`'s body with the Lab arm of `lab_gray`
//!   (phase 5a-ii): each pixel's L\* through the Gray profile's tone curve.
//! * **The view** ([`lab_display`]) runs a Lab projection through the same
//!   loop into the built-in Display P3 at u16, before the host narrows it.
//!   Its pipeline ([`view_pipeline`]) and the way back
//!   ([`from_view_pipeline`]) are also Remove's (`doc_remove_model`, 2f's
//!   third part): the model sees the view and its fill returns to Lab codes.
//!   Since phase 6b-iii they are also the neural and generative tools'
//!   (`lab_neural`): the model or provider sees the view, and its picture
//!   returns to Lab codes by the Mode rule.

use std::sync::{Arc, OnceLock};

use serde_json::{json, Map, Value};

use crate::cmyk_convert_tree::{leaf_count, map_tree};
use crate::doc_color_policy::ConversionOptions;
use crate::icc::IccProfile;
use crate::icc_intent::{ColorPipeline, RenderingIntent};
use crate::job_control::{DrJobControl, JobContext};
use crate::lab_document::{lab_twin, LabDepth, LabPolicy};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, PrecisionImage, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::tile_store::TilePlane;

/// Bytes a converted pixel either Mode conversion charges: the converted
/// output (8 B/px; the pipeline is prepared once, each pixel computed once)
/// and the result's display preparation the host runs right after — Gray's
/// measured term (`gray_convert::TO_GRAY_BYTES_PER_PIXEL`, the same RGBA16
/// shape), held by `scripts/lab-admission/measure.py` to ⌈1.25 × the
/// measured peak⌉; a miss raises this term.
pub(crate) const TO_LAB_BYTES_PER_PIXEL: u64 = 41;

/// Bytes the Lab view charges a pixel: its Display P3 RGBA16 output, built
/// tile by tile from the source's rows (no second copy of either).
pub(crate) const VIEW_BYTES_PER_PIXEL: u64 = 8;

/// The Lab Color document's VIEW: `image` — a Lab document's u16
/// projection or layer source, its samples twin codes — through the twin
/// into the built-in Display P3 at u16, relative colorimetric without BPC
/// ([`converted_image`], full-range codes), alpha kept, the result tagged
/// Display P3: the space a CMYK document's view is in (`cmyk_document`),
/// which the host narrows and draws as it does any RGB image.
///
/// Why the core and not ColorSync drawing the twin: ColorSync decodes every
/// Lab a\*/b\* half a unit high (D50 white reads (100, −0.5, −0.5)), and a
/// byte preview of the Lab codes rounds a\*/b\* to whole units before any
/// conversion; together they moved a saturated sRGB patch by 13 codes and
/// tinted a neutral. Converting the u16 codes here, before any narrowing,
/// leaves only the display's own 8-bit rounding.
///
/// Refuses an image whose profile is not the twin (its samples are not Lab
/// codes), float storage, scratch past `limit` and a cancel.
pub fn lab_display(
    image: &PrecisionImage,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<PrecisionImage, String> {
    if image.profile().bytes() != lab_twin().bytes() {
        return Err("the Lab Color view takes a Lab Color image".into());
    }
    if image.storage() != PrecisionStorage::Rgba16Unorm {
        return Err("the Lab Color view takes a 16-bit image".into());
    }
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let view = IccProfile::display_p3();
    let pipeline = view_pipeline()?;
    let (w, h) = image.dimensions();
    let scratch = view_scratch_bytes(w, h)?;
    admit(image.retained_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    let wide = |unit: f64| (unit.clamp(0.0, 1.0) * 65535.0).round() as u16;
    converted_image(image, &pipeline, &view, &wide, job)
}

/// What [`lab_display`] reserves for a `width` × `height` image: its output
/// at [`VIEW_BYTES_PER_PIXEL`], the view pipeline's tables and bookkeeping.
/// `PrecisionDocument::display_scratch_bytes` charges it for a Lab document
/// (#1 Lab phase 4a-ii: the host bounds each display stage by that plan).
/// Both profiles are built in, so the tables' bytes are computed once.
pub(crate) fn view_scratch_bytes(width: u32, height: u32) -> Result<u64, String> {
    static TABLES: OnceLock<u64> = OnceLock::new();
    let tables = match TABLES.get() {
        Some(&bytes) => bytes,
        None => {
            let bytes = view_pipeline()?.scratch_bytes();
            *TABLES.get_or_init(|| bytes)
        }
    };
    Ok((u64::from(width) * u64::from(height))
        .saturating_mul(VIEW_BYTES_PER_PIXEL)
        .saturating_add(tables)
        .saturating_add(BOOKKEEPING_BYTES))
}

/// The view's conversion ([`lab_display`]): the twin into the built-in
/// Display P3, relative colorimetric, no BPC.
pub(crate) fn view_pipeline() -> Result<ColorPipeline, String> {
    ColorPipeline::new(
        &lab_twin(),
        &IccProfile::display_p3(),
        RenderingIntent::RelativeColorimetric,
        false,
    )
}

/// The view's way back: the built-in Display P3 into the twin, under
/// [`view_pipeline`]'s options.
pub(crate) fn from_view_pipeline() -> Result<ColorPipeline, String> {
    ColorPipeline::new(
        &IccProfile::display_p3(),
        &lab_twin(),
        RenderingIntent::RelativeColorimetric,
        false,
    )
}

/// RGB/Gray → Lab: relative colorimetric from `from` into the twin, no BPC
/// (module doc). New Lab Color converts its sRGB fill through the same.
pub(crate) fn to_lab_pipeline(from: &IccProfile) -> Result<ColorPipeline, String> {
    ColorPipeline::new(
        from,
        &lab_twin(),
        RenderingIntent::RelativeColorimetric,
        false,
    )
}

/// The first record (postorder) a Lab document cannot hold, as the
/// sentence the host shows beside its "flatten the image" choice: an
/// adjustment record `lab_adjust::lab_carry_refusal` names (its sentence
/// as-is; phase 2d), a style `lab_style::style_refusal` refuses (an effect
/// mode outside the Lab list: phase 2e; a Pattern Overlay or Pattern
/// stroke carries since 2f's second part), a Blend If (the layer's or the
/// style's) with a range `lab_style::blend_if_channel_refusal` refuses (a
/// Gray or plate channel: phase 2f), a CMYK adjustment layer, a layer in a
/// blend mode `lab_document::blend_refusal` refuses, a placed image or a
/// warp (a Lab layer keeps neither binding). A Fill Opacity or knockout
/// style, every other layer effect (phase 2e: its stored style unchanged,
/// its colours converted at composite), a red, green or blue Blend If
/// (phase 2f), the modes of `lab_document::LAB_BLEND_MODES` (phase 1b) and
/// every other adjustment record (phase 2d) carry; a type, shape or fill
/// layer converts as its pixels, its description dropped (phase 6a-i,
/// module doc). A Generative Layer converts as its pixels too: the fresh
/// raster carries no binding, so its variations detach, as Convert to
/// Profile detaches them on RGB (the close).
pub(crate) fn first_lab_blocker(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    for layer in layers {
        if let Some(reason) = layer.children().and_then(first_lab_blocker) {
            return Some(reason);
        }
        if let Content::Adjustment(state) = &layer.content {
            if let Some(reason) =
                crate::lab_adjust::lab_carry_refusal(&layer.name, state.description())
            {
                return Some(reason);
            }
        }
        if let Some(reason) = layer
            .style
            .as_deref()
            .and_then(crate::lab_style::style_refusal)
        {
            return Some(format!(
                "“{}” cannot convert to Lab Color: {reason} Flatten the image to convert it.",
                layer.name
            ));
        }
        if let Some(reason) = layer
            .blend_if
            .iter()
            .chain(layer.style.as_ref().and_then(|s| s.blend_if.as_ref()))
            .find_map(crate::lab_style::blend_if_channel_refusal)
        {
            return Some(format!(
                "“{}” cannot convert to Lab Color: {reason} Flatten the image to convert it.",
                layer.name
            ));
        }
        let mode = layer.blend_mode;
        let what = if matches!(layer.content, Content::CmykAdjustment(_)) {
            Some("an adjustment layer".to_string())
        } else if crate::lab_document::blend_refusal(mode).is_some() {
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
                "“{}” is {what}, which a Lab Color document cannot hold; flatten the image to convert it",
                layer.name
            ));
        }
    }
    None
}

/// The RGB/Gray → Lab options: `{"flatten"?:bool,"depth"?:8|16}` (default
/// 16); an unknown key refuses.
struct LabOptions {
    flatten: bool,
    depth: LabDepth,
}

impl LabOptions {
    fn parse(text: &str) -> Result<Self, String> {
        let mut out = LabOptions {
            flatten: false,
            depth: LabDepth::Sixteen,
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
            if !matches!(key.as_str(), "flatten" | "depth") {
                return Err(format!("mode options have an unknown key \"{key}\""));
            }
        }
        match object.get("flatten") {
            None | Some(Value::Null) => {}
            Some(Value::Bool(v)) => out.flatten = *v,
            Some(_) => return Err("mode option flatten must be a boolean".into()),
        }
        if let Some(depth) = object.get("depth").filter(|v| !v.is_null()) {
            out.depth = depth
                .as_u64()
                .and_then(|bits| LabDepth::from_bits(bits as u32))
                .ok_or("mode option depth must be 8 or 16")?;
        }
        Ok(out)
    }
}

/// The RGB Color options: `{"intent"?,"black_point_compensation"?}`; an
/// unknown key refuses.
fn rgb_options(text: &str) -> Result<ConversionOptions, String> {
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

/// Σ `rebind_scratch_bytes(twin)` over every adjustment record of `layers`
/// (groups included): what carrying them into the Lab twin prepares. A
/// record whose reserve does not parse counts 0; its rebind refuses.
fn rebind_bytes(layers: &[PrecisionDocumentLayer], twin: &Arc<IccProfile>) -> u64 {
    layers
        .iter()
        .map(|layer| match &layer.content {
            Content::Adjustment(state) => state.rebind_scratch_bytes(twin).unwrap_or(0),
            Content::Group(children) | Content::PassThrough(children) => {
                rebind_bytes(children, twin)
            }
            _ => 0,
        })
        .fold(0u64, u64::saturating_add)
}

/// The raster pixels of every leaf (a CMYK leaf's too, so a CMYK document's
/// plan answers the Lab stage of its route through RGB).
fn raster_pixels(layers: &[PrecisionDocumentLayer]) -> u64 {
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
            Content::Group(children) | Content::PassThrough(children) => raster_pixels(children),
            _ => 0,
        })
        .sum()
}

impl PrecisionDocument {
    /// Why this document cannot become Lab even with a flatten, or `None`.
    fn to_lab_hard_refusal(&self) -> Option<String> {
        if self.is_lab() {
            return Some("the document is already Lab Color".into());
        }
        if self.is_cmyk() {
            return Some(
                "Lab Color converts an RGB or Grayscale document; convert to RGB Color first"
                    .into(),
            );
        }
        if self.is_float() {
            return Some(
                "Lab Color is 8 or 16 bits per channel; convert to 16 Bits/Channel first".into(),
            );
        }
        None
    }

    /// The RGB/Gray → Lab plan: [`TO_LAB_BYTES_PER_PIXEL`] a converted pixel
    /// (or the larger Flatten Image's render first), the prepared pipeline's
    /// tables, the twin, each carried adjustment record's rebind
    /// (`PrecisionAdjustmentState::rebind_scratch_bytes`, phase 2d) and
    /// bookkeeping. A source profile no pipeline can
    /// read (a CMYK document's, whose host route runs through ProPhoto RGB
    /// first) is charged as sRGB's: every matrix source prepares the same.
    fn to_lab_scratch(&self, flatten: bool) -> u64 {
        let canvas = u64::from(self.width) * u64::from(self.height);
        let pixels = if flatten {
            canvas
        } else {
            raster_pixels(self.root_layers())
        };
        let pipeline = to_lab_pipeline(self.profile())
            .or_else(|_| to_lab_pipeline(&IccProfile::srgb()))
            .map_or(0, |p| p.scratch_bytes());
        let flattening = if flatten {
            self.render_plan().map(|p| p.scratch_bytes).unwrap_or(0)
        } else {
            0
        };
        let twin = lab_twin();
        let records = if flatten {
            0
        } else {
            rebind_bytes(self.root_layers(), &twin)
        };
        flattening.max(pixels.saturating_mul(TO_LAB_BYTES_PER_PIXEL))
            + pipeline
            + twin.bytes().len() as u64
            + records
            + BOOKKEEPING_BYTES
    }

    /// `dr_precision_doc_convert_to_lab_plan`'s answer (Gray's keys).
    pub fn convert_to_lab_plan(&self, options: &str) -> Result<Value, String> {
        let options = LabOptions::parse(options)?;
        let blocker = first_lab_blocker(self.root_layers());
        let refusal = self
            .to_lab_hard_refusal()
            .or(blocker.clone().filter(|_| !options.flatten));
        let flattens = blocker.is_some() && options.flatten;
        Ok(json!({
            "working_memory_plan_bytes": self.to_lab_scratch(flattens),
            "refusal": refusal,
            "flatten_required": blocker.is_some(),
            "flattens": flattens,
        }))
    }

    /// RGB/Gray → Lab (module doc).
    pub fn convert_to_lab(
        &self,
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let options = LabOptions::parse(options)?;
        if let Some(reason) = self.to_lab_hard_refusal() {
            return Err(reason);
        }
        let blocker = first_lab_blocker(self.root_layers());
        if let (Some(reason), false) = (&blocker, options.flatten) {
            return Err(reason.clone());
        }
        let scratch = self.to_lab_scratch(blocker.is_some());
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
        let policy = LabPolicy::for_depth(options.depth);
        let pipeline = to_lab_pipeline(source.profile())?;
        let layers = convert_tree(
            source,
            &pipeline,
            policy.twin(),
            &|unit| policy.code(unit),
            job,
        )?;
        let out = carried(source, layers, policy.twin().clone(), limit, other)?
            .with_lab(Arc::new(policy))?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The RGB destination of a Lab document: `icc`, an RGB profile a
    /// document can hold and this build can encode into.
    fn lab_rgb_destination(icc: &[u8]) -> Result<Arc<IccProfile>, String> {
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

    /// The Lab → RGB pipeline: the twin into `destination` under `options`.
    fn lab_rgb_pipeline(
        destination: &IccProfile,
        options: &ConversionOptions,
    ) -> Result<ColorPipeline, String> {
        ColorPipeline::new(
            &lab_twin(),
            destination,
            options.intent,
            options.black_point_compensation,
        )
    }

    /// `dr_precision_doc_convert_lab_to_rgb_plan`'s answer.
    pub fn convert_lab_to_rgb_plan(&self, icc: &[u8], options: &str) -> Result<Value, String> {
        let conversion = rgb_options(options)?;
        let pipeline = Self::lab_rgb_destination(icc)
            .and_then(|profile| Self::lab_rgb_pipeline(&profile, &conversion));
        let refusal = if !self.is_lab() {
            Some("the document is not Lab Color".to_string())
        } else {
            pipeline.as_ref().err().cloned()
        };
        let bytes = raster_pixels(self.root_layers()).saturating_mul(TO_LAB_BYTES_PER_PIXEL)
            + pipeline.as_ref().map_or(0, ColorPipeline::scratch_bytes)
            + icc.len() as u64
            + BOOKKEEPING_BYTES;
        Ok(json!({
            "working_memory_plan_bytes": bytes,
            "refusal": refusal,
            "flatten_required": false,
            "flattens": false,
        }))
    }

    /// Lab → RGB (module doc).
    pub fn convert_lab_to_rgb(
        &self,
        icc: &[u8],
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let conversion = rgb_options(options)?;
        if !self.is_lab() {
            return Err("the document is not Lab Color".into());
        }
        let destination = Self::lab_rgb_destination(icc)?;
        let pipeline = Self::lab_rgb_pipeline(&destination, &conversion)?;
        let scratch = raster_pixels(self.root_layers()).saturating_mul(TO_LAB_BYTES_PER_PIXEL)
            + pipeline.scratch_bytes()
            + icc.len() as u64
            + BOOKKEEPING_BYTES;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let out = self.lab_to_rgb_with_pipeline(&destination, &pipeline, limit, other, job)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Lab → RGB's conversion once admitted: every leaf through the twin
    /// into `destination` under `options`, at u16, never narrowed. The
    /// caller has admitted and reserved its scratch; Lab → CMYK's ProPhoto
    /// stage runs it too (`lab_cmyk`, #1 Lab phase 5a).
    pub(crate) fn lab_to_rgb_with_context(
        &self,
        destination: &Arc<IccProfile>,
        options: &ConversionOptions,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let pipeline = Self::lab_rgb_pipeline(destination, options)?;
        self.lab_to_rgb_with_pipeline(destination, &pipeline, limit, other, job)
    }

    /// [`Self::lab_to_rgb_with_context`] over an already prepared `pipeline`
    /// (the twin into `destination`), so `convert_lab_to_rgb` builds the one
    /// it charges and converts by once.
    fn lab_to_rgb_with_pipeline(
        &self,
        destination: &Arc<IccProfile>,
        pipeline: &ColorPipeline,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let wide = |unit: f64| (unit.clamp(0.0, 1.0) * 65535.0).round() as u16;
        // The policy drops: the result is the RGB document its storage is,
        // re-encoded into the destination.
        let source = self.without_lab();
        let layers = convert_tree(&source, pipeline, destination, &wide, job)?;
        carried(&source, layers, destination.clone(), limit, other)
    }
}

/// Every leaf of `source` through `pipeline` into `profile` (module doc),
/// `code` quantizing each unit sample; progress over 0.3…1.0.
fn convert_tree(
    source: &PrecisionDocument,
    pipeline: &ColorPipeline,
    profile: &Arc<IccProfile>,
    code: &dyn Fn(f64) -> u16,
    job: JobContext<'_>,
) -> Result<Vec<PrecisionDocumentLayer>, String> {
    let count = leaf_count(source.root_layers()).max(1) as f64;
    let mut index = 0usize;
    map_tree(source.root_layers(), &mut |layer| {
        // An adjustment record (Lab → RGB, and since phase 2d RGB/Gray → Lab,
        // where `first_lab_blocker` names the ones Lab cannot carry first) is
        // the same record rebound to `profile`, so its settings act on the
        // converted composite; rebound to the twin it binds its Lab form.
        if let Content::Adjustment(state) = &layer.content {
            let mut out = layer.clone();
            out.content = Content::Adjustment(state.rebind(profile)?);
            return Ok(out);
        }
        let Content::Raster { image, offset } = &layer.content else {
            return Err(first_lab_blocker(std::slice::from_ref(layer))
                .unwrap_or_else(|| "A non-raster record cannot change colour mode".into()));
        };
        let part = job.range(
            0.3 + 0.7 * index as f64 / count,
            0.3 + 0.7 * (index + 1) as f64 / count,
        );
        index += 1;
        let converted = converted_image(image, pipeline, profile, code, part)?;
        PrecisionDocumentLayer::raster(
            &layer.name,
            converted,
            *offset,
            layer.opacity,
            layer.visible,
        )
    })
}

/// `layers` as a document in `profile`, carrying `source`'s channels,
/// metadata, resolution, guides, ruler origin, samplers, light, paths,
/// vector-mask seeds (`gray_convert::convert_to_gray`'s list) and layer
/// comps (phase 6a-ii, `lab_comps`).
fn carried(
    source: &PrecisionDocument,
    layers: Vec<PrecisionDocumentLayer>,
    profile: Arc<IccProfile>,
    limit: u64,
    other: u64,
) -> Result<PrecisionDocument, String> {
    let mut out = PrecisionDocument::build(
        source.width,
        source.height,
        profile,
        source.metadata.clone(),
        source.resolution,
        layers,
        source.channels().to_vec(),
        source.float.clone(),
    )?;
    out.guides.clone_from(&source.guides);
    out.ruler_origin = source.ruler_origin;
    out.color_samplers.clone_from(&source.color_samplers);
    out.global_light = source.global_light;
    out.paths.set_entries(source.paths.entries.to_vec());
    // The ids bind (`map_tree` carries each record's id) and a recorded
    // style is authored sRGB both ways, so the comps carry unchanged and
    // need no plan term (`lab_comps`).
    out.comps.clone_from(&source.comps);
    out.background = source.background;
    Ok(out.seeding_vector_masks(source.vector_mask_seeds(Clone::clone), limit, other))
}

/// One u16 image through `pipeline` into `profile` (module doc): each
/// colour clamped and quantized by `code`, alpha by the same.
pub(crate) fn converted_image(
    image: &PrecisionImage,
    pipeline: &ColorPipeline,
    profile: &Arc<IccProfile>,
    code: &dyn Fn(f64) -> u16,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = image.dimensions();
    let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
        w,
        h,
        |y, row| {
            let mut x = 0usize;
            for (_, run) in image.row_segments(y, 0, w) {
                let PrecisionRun::Rgba16(run) = run else {
                    return Err("Lab Color requires a 16-bit document".into());
                };
                for pixel in run {
                    let out = pipeline.color([pixel[0], pixel[1], pixel[2]].map(|c| {
                        // u16 → unit f32 is exact to well under a code.
                        f32::from(c) / 65535.0
                    }));
                    row[x] = [
                        code(f64::from(out[0])),
                        code(f64::from(out[1])),
                        code(f64::from(out[2])),
                        code(f64::from(pixel[3]) / 65535.0),
                    ];
                    x += 1;
                }
            }
            job.every(y as usize * w as usize)
        },
        job,
    )?;
    PrecisionImage::from_rgba16_tiles_unseeded(plane, profile.clone())
}
