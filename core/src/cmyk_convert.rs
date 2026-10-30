//! Image ▸ Mode ▸ CMYK Color… and RGB Color… (#1 phase 1): the two whole
//! document conversions, their plans and their refusals.
//!
//! * **RGB → CMYK** admits a u16 RGB document whose records are raster
//!   layers and groups the CMYK mode can hold (`cmyk_document`), carried as
//!   a tree by `cmyk_convert_tree` (masks, clipping, modes and Fill
//!   Opacity included). Anything else is named — the first blocker in
//!   postorder — with `flatten_required`, and `flatten: true` runs the
//!   existing Flatten Image (`flatten_image`) first. A 32-bit float document
//!   refuses: convert to 16 Bits/Channel first. Alpha channels, paths and
//!   vector masks carry (the channels batch). Each layer's samples go through
//!   `cmyk_color::RgbToCmyk` (relative colorimetric with BPC unless the
//!   options say otherwise) into the destination profile — the working CMYK
//!   profile when the host passes none — at the declared depth: depth 8
//!   rounds every component to the 8-bit grid (×257), which is what an 8-bit
//!   RGB document converted in Photoshop holds. Since #1 phase 5b it also
//!   takes an 8/16-bit Gray document directly (Mode ▸ CMYK Color from
//!   Grayscale): its storage is the twin's R = G = B, so the same
//!   `RgbToCmyk` from `source.profile()` (the twin) converts it, with the
//!   same blockers; the `gray` policy is not carried. Since #1 Lab phase
//!   5a it takes an 8/16-bit Lab document too (Mode ▸ CMYK Color from Lab
//!   Color): the Lab document, flattened first when asked, becomes its
//!   16-bit ProPhoto RGB stage (`lab_cmyk`, CMYK → Lab's rule reversed),
//!   which the same body converts; its blocker is `lab_cmyk::first_blocker`
//!   (a Blend If named Lightness, a or b) and its plan adds the stage's
//!   (`lab_cmyk::stage_scratch_bytes`).
//! * **CMYK → RGB** yields a u16 RGB document in the destination profile
//!   (the built-in sRGB when the host passes none) with every layer kept,
//!   through `cmyk_color::CmykToRgb`; `flatten: true` flattens on paper
//!   first. The tree is carried as it is toward CMYK. A CMYK adjustment
//!   record (`cmyk_adjust_layer`: Levels or Curves on ink, which no RGB
//!   record means), at any depth, is a blocker, named with
//!   `flatten_required` exactly as an RGB blocker is the other way.
//!
//! Layer names, opacity, visibility, offsets, ids, labels, locks, links,
//! layer masks (u16 coverage, colour-free), groups, clipping, blend modes,
//! layer styles (effects, knockout, Blend Interior, Fill; toward RGB without
//! their inks) carry over both ways; so do alpha channels (verbatim),
//! paths, raster-less vector masks (re-set from their shapes),
//! metadata, resolution, guides, the ruler origin and the global light.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::cmyk_color::{ink_key, CmykToCmyk, CmykToRgb, ConversionOptions, RgbToCmyk};
use crate::cmyk_composite::{code, codes, on_paper, unit};
use crate::cmyk_convert_tree::{
    first_blocker, first_rgb_blocker, leaf_count, map_tree, without_inks,
};
use crate::cmyk_document::{refused_sentence, CmykDepth, CmykPolicy};
use crate::cmyk_image::CmykImage;
use crate::icc::IccProfile;
use crate::icc_intent::RenderingIntent;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES};
use crate::soft_proof::{memo_peak_bytes, memo_peak_bytes_for};
use crate::tile_store::TilePlane;

/// Bytes a converted pixel the RGB → CMYK plan charges beside the colour
/// memo, the profile and the view: the CMYK output (10 B/px) and the result's
/// display preparation the host runs right after (its render, display bytes
/// and image). Fitted to `scripts/cmyk-admission/measure.py` (24 MP: 591 MB
/// measured, so plan ≥ ⌈1.25 ×⌉); a miss raises this term.
pub(crate) const TO_CMYK_BYTES_PER_PIXEL: u64 = 28;

/// The same for CMYK → RGB: the 16-bit RGB output (8 B/px) and the RGB
/// result's display preparation (the typed render's f32 accumulator among
/// it; 769 MB measured at 24 MP).
pub(crate) const TO_RGB_BYTES_PER_PIXEL: u64 = 38;

/// The CMYK Flatten Image's charge a canvas pixel: the new plane (10 B/px)
/// and the flattened document's display preparation (428 MB measured at
/// 24 MP). Every CMYK merge's plan charges it too (`cmyk_merge`): phase
/// 4c's `merge-group-16` measured 505–515 MB over three runs at 24 MP,
/// which 24 missed (581 MB < ⌈1.25 × 515 MB⌉ = 644 MB); 27 is the smallest
/// integer that clears the largest run and 28 keeps the plan above that
/// run-to-run spread, so it was raised to 28. The styles batch's SEEDED
/// Merge Down (`merge-knockout-16`, a deep knockout over the Background)
/// measured 587.8–588.6 MB at 24 MP, which 28 missed (675 MB <
/// ⌈1.25 × 588.6 MB⌉ = 736 MB; the unseeded twin `merge-down-16` measured
/// 433 MB); 31 is the smallest integer that clears it, so it was raised to 31.
pub(crate) const FLATTEN_BYTES_PER_PIXEL: u64 = 31;

/// The mode conversion's options (module doc).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ModeOptions {
    pub conversion: ConversionOptions,
    pub flatten: bool,
    pub depth: Option<CmykDepth>,
}

impl ModeOptions {
    /// `{"intent","black_point_compensation","flatten","depth"}`, every key
    /// optional; `depth` (8 or 16) only toward CMYK. An unknown key refuses.
    pub(crate) fn parse(text: Option<&str>, to_cmyk: bool) -> Result<Self, String> {
        let mut out = ModeOptions {
            conversion: ConversionOptions::default(),
            flatten: false,
            depth: None,
        };
        let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
            return Ok(out);
        };
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("mode options are not valid JSON: {e}"))?;
        let object: &Map<String, Value> = value
            .as_object()
            .ok_or("mode options must be a JSON object")?;
        for key in object.keys() {
            let known = matches!(
                key.as_str(),
                "intent" | "black_point_compensation" | "flatten"
            ) || (to_cmyk && key == "depth");
            if !known {
                return Err(format!("mode options have an unknown key \"{key}\""));
            }
        }
        if let Some(intent) = object.get("intent").filter(|v| !v.is_null()) {
            let text = intent
                .as_str()
                .ok_or("mode option intent must be a string")?;
            out.conversion.intent = RenderingIntent::parse(text).ok_or_else(|| {
                format!(
                    "intent \"{text}\" is not one of perceptual, relative, saturation or absolute"
                )
            })?;
        }
        let flag = |key: &str| -> Result<Option<bool>, String> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Bool(v)) => Ok(Some(*v)),
                Some(_) => Err(format!("mode option {key} must be a boolean")),
            }
        };
        if let Some(bpc) = flag("black_point_compensation")? {
            out.conversion.black_point_compensation = bpc;
        }
        out.flatten = flag("flatten")?.unwrap_or(false);
        if let Some(depth) = object.get("depth").filter(|v| !v.is_null()) {
            let bits = depth.as_u64().ok_or("mode option depth must be 8 or 16")?;
            out.depth =
                Some(CmykDepth::from_bits(bits as u32).ok_or("mode option depth must be 8 or 16")?);
        }
        Ok(out)
    }
}

/// The destination CMYK policy: `icc`, else the working CMYK profile.
fn cmyk_destination(icc: Option<&[u8]>, depth: CmykDepth) -> Result<CmykPolicy, String> {
    match icc {
        Some(bytes) => CmykPolicy::new(bytes, depth),
        None => {
            let bytes = crate::working_cmyk::working_cmyk_bytes().ok_or(
                "no working CMYK profile is set, so CMYK Color needs a CMYK profile to convert into",
            )?;
            CmykPolicy::new(&bytes, depth)
        }
    }
}

/// The destination RGB profile: `icc` (an RGB profile a document can hold
/// and encode into), else the built-in sRGB.
fn rgb_destination(icc: Option<&[u8]>) -> Result<Arc<IccProfile>, String> {
    let profile = match icc {
        Some(bytes) => Arc::new(IccProfile::parse(bytes).ok_or(
            "RGB Color needs an RGB profile a Doctored document can hold to convert into",
        )?),
        None => IccProfile::srgb(),
    };
    if !profile.can_encode() {
        return Err(format!(
            "the RGB profile \"{}\" cannot be converted into",
            profile.name()
        ));
    }
    Ok(profile)
}

impl PrecisionDocument {
    /// Why this document cannot be converted to CMYK even with a flatten,
    /// or `None`.
    fn to_cmyk_hard_refusal(&self) -> Option<String> {
        if self.is_cmyk() {
            return Some("the document is already CMYK".into());
        }
        if self.is_float() {
            return Some(
                "CMYK Color is unavailable in a 32-bit float document; convert to 16 Bits/Channel first"
                    .into(),
            );
        }
        None
    }

    /// The first record the CMYK mode cannot hold ([`first_blocker`], a Lab
    /// document's `lab_cmyk::first_blocker`); alpha channels, paths and
    /// vector masks carry.
    fn to_cmyk_blocker(&self) -> Option<String> {
        if self.is_lab() {
            crate::lab_cmyk::first_blocker(self.root_layers())
        } else {
            first_blocker(self.root_layers())
        }
    }

    /// The Lab source's ProPhoto stage over `pixels` (`lab_cmyk`); 0 for
    /// every other source.
    fn lab_stage_scratch(&self, pixels: u64) -> u64 {
        if self.is_lab() {
            crate::lab_cmyk::stage_scratch_bytes(pixels)
        } else {
            0
        }
    }

    /// The raster pixels of every leaf, at every depth.
    fn raster_pixels(&self) -> u64 {
        fn pixels(layers: &[PrecisionDocumentLayer]) -> u64 {
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
                    Content::Group(children) | Content::PassThrough(children) => pixels(children),
                    _ => 0,
                })
                .sum()
        }
        pixels(self.root_layers())
    }

    /// `dr_precision_doc_convert_to_cmyk_plan`'s answer.
    pub fn convert_to_cmyk_plan(&self, icc: Option<&[u8]>, options: &str) -> Result<Value, String> {
        let options = ModeOptions::parse(Some(options), true)?;
        let depth = options.depth.unwrap_or(CmykDepth::Sixteen);
        let destination = cmyk_destination(icc, depth);
        let blocker = self.to_cmyk_blocker();
        let refusal = self
            .to_cmyk_hard_refusal()
            .or(destination.as_ref().err().cloned())
            .or(blocker.clone().filter(|_| !options.flatten));
        let pixels = if blocker.is_some() {
            u64::from(self.width) * u64::from(self.height)
        } else {
            self.raster_pixels()
        };
        let flatten = if blocker.is_some() && options.flatten {
            self.render_plan().map(|p| p.scratch_bytes).unwrap_or(0)
        } else {
            0
        };
        let bytes = self
            .to_cmyk_scratch(pixels, flatten, destination.as_ref().ok())
            .saturating_add(self.lab_stage_scratch(pixels));
        // `flattens`: whether the conversion flattens first, exactly
        // `convert_to_cmyk`'s condition (a blocker under `flatten`).
        Ok(json!({"working_memory_plan_bytes":bytes,"refusal":refusal,
            "flatten_required":blocker.is_some(),
            "flattens":blocker.is_some() && options.flatten}))
    }

    fn to_cmyk_scratch(&self, pixels: u64, flatten: u64, policy: Option<&CmykPolicy>) -> u64 {
        let output = pixels.saturating_mul(TO_CMYK_BYTES_PER_PIXEL);
        let profile = policy.map_or(0, |p| p.profile.bytes() + p.profile_bytes().len() as u64);
        // `RgbToCmyk`'s memo holds `[f64; 4]` ink, not the proofer's entry.
        let memo = memo_peak_bytes_for::<[f64; 4]>(pixels);
        flatten.max(output.saturating_add(memo))
            + profile
            + CmykPolicy::view_bytes()
            + BOOKKEEPING_BYTES
    }

    /// RGB → CMYK (module doc).
    pub fn convert_to_cmyk(
        &self,
        icc: Option<&[u8]>,
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let options = ModeOptions::parse(Some(options), true)?;
        if let Some(reason) = self.to_cmyk_hard_refusal() {
            return Err(reason);
        }
        let depth = options.depth.unwrap_or(CmykDepth::Sixteen);
        let policy = cmyk_destination(icc, depth)?;
        let blocker = self.to_cmyk_blocker();
        if let (Some(reason), false) = (&blocker, options.flatten) {
            return Err(reason.clone());
        }
        let flatten_scratch = if blocker.is_some() {
            self.render_plan()?.scratch_bytes
        } else {
            0
        };
        let pixels = if blocker.is_some() {
            u64::from(self.width) * u64::from(self.height)
        } else {
            self.raster_pixels()
        };
        let scratch = self
            .to_cmyk_scratch(pixels, flatten_scratch, Some(&policy))
            .saturating_add(self.lab_stage_scratch(pixels));
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        // A Lab source flattens over 0…0.15 and stages over 0.15…0.3.
        let flatten_end = if self.is_lab() { 0.15 } else { 0.3 };
        let flattened;
        let source = if blocker.is_some() {
            flattened = self
                .flatten_image(limit, other, job.range(0.0, flatten_end))?
                .ok_or("flattening the image changed nothing")?;
            &flattened
        } else {
            self
        };
        let staged;
        let source = if source.is_lab() {
            staged = source.lab_cmyk_stage(limit, other, job.range(0.15, 0.3))?;
            &staged
        } else {
            source
        };
        let out = Self::rgb_to_cmyk(source, policy, &options, depth, (limit, other), job)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// RGB → CMYK's conversion once admitted: every leaf of `source` (an RGB
    /// or Gray document, or a Lab document's ProPhoto stage) through
    /// `RgbToCmyk` into `policy` at `depth` (module doc); progress over
    /// 0.3…1.0.
    fn rgb_to_cmyk(
        source: &Self,
        policy: CmykPolicy,
        options: &ModeOptions,
        depth: CmykDepth,
        (limit, other): (u64, u64),
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let mut leg = RgbToCmyk::new(
            source.profile().clone(),
            &policy.profile,
            options.conversion,
        )?;
        let count = leaf_count(source.root_layers()).max(1) as f64;
        let mut index = 0usize;
        let layers = map_tree(source.root_layers(), &mut |layer| {
            let Content::Raster { image, offset } = &layer.content else {
                return Err(first_blocker(std::slice::from_ref(layer))
                    .unwrap_or_else(|| refused_sentence("A non-raster record")));
            };
            let part = job.range(
                0.3 + 0.7 * index as f64 / count,
                0.3 + 0.7 * (index + 1) as f64 / count,
            );
            index += 1;
            let converted = rgb16_to_cmyk_image(image, &mut leg, depth, part)?;
            PrecisionDocumentLayer::raster_cmyk(
                &layer.name,
                converted,
                *offset,
                layer.opacity,
                layer.visible,
            )
        })?;
        let mut out = PrecisionDocument::build_cmyk(
            source.width,
            source.height,
            source.metadata.clone(),
            source.resolution,
            layers,
            source.channels().to_vec(),
            Arc::new(policy),
        )?;
        out.guides.clone_from(&source.guides);
        out.ruler_origin = source.ruler_origin;
        // The styles ride along (`cmyk_convert_tree`), so their light does.
        out.global_light = source.global_light;
        out.paths.set_entries(source.paths.entries.to_vec());
        // Raster-less vector masks are re-set from their shapes, as the RGB
        // depth conversions re-set them (`precision_vector_mask_carry`).
        Ok(out.seeding_vector_masks(source.vector_mask_seeds(Clone::clone), limit, other))
    }

    /// The first record RGB Color cannot carry (a CMYK adjustment record or
    /// a plate Blend If), as a sentence (module doc).
    fn to_rgb_blocker(&self) -> Option<String> {
        first_rgb_blocker(self.root_layers())
    }

    /// `dr_precision_doc_convert_cmyk_to_rgb_plan`'s answer.
    pub fn convert_cmyk_to_rgb_plan(
        &self,
        icc: Option<&[u8]>,
        options: &str,
    ) -> Result<Value, String> {
        let options = ModeOptions::parse(Some(options), false)?;
        let blocker = self.to_rgb_blocker();
        let refusal = if self.is_cmyk() {
            rgb_destination(icc)
                .err()
                .or(blocker.clone().filter(|_| !options.flatten))
        } else {
            Some("the document is not CMYK".to_string())
        };
        let flatten =
            options.flatten && (self.root_layers().len() > 1 || self.to_rgb_blocker().is_some());
        let pixels = if flatten {
            u64::from(self.width) * u64::from(self.height)
        } else {
            self.raster_pixels()
        };
        // `flattens` is `convert_cmyk_to_rgb`'s own condition.
        Ok(
            json!({"working_memory_plan_bytes":self.to_rgb_scratch(pixels, flatten),
            "refusal":refusal,"flatten_required":blocker.is_some(),"flattens":flatten}),
        )
    }

    fn to_rgb_scratch(&self, pixels: u64, flatten: bool) -> u64 {
        let canvas = u64::from(self.width) * u64::from(self.height);
        pixels.saturating_mul(TO_RGB_BYTES_PER_PIXEL)
            + memo_peak_bytes(pixels)
            + if flatten {
                canvas * 10 + u64::from(self.width) * 20
            } else {
                0
            }
            + BOOKKEEPING_BYTES
    }

    /// CMYK → RGB (module doc).
    pub fn convert_cmyk_to_rgb(
        &self,
        icc: Option<&[u8]>,
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let options = ModeOptions::parse(Some(options), false)?;
        let policy = self.require_cmyk_policy("RGB Color")?.clone();
        let destination = rgb_destination(icc)?;
        if let (Some(reason), false) = (self.to_rgb_blocker(), options.flatten) {
            return Err(reason);
        }
        let flatten =
            options.flatten && (self.root_layers().len() > 1 || self.to_rgb_blocker().is_some());
        let pixels = if flatten {
            u64::from(self.width) * u64::from(self.height)
        } else {
            self.raster_pixels()
        };
        let scratch = self.to_rgb_scratch(pixels, flatten);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let flattened;
        let source = if flatten {
            flattened = self.cmyk_flatten(job.range(0.0, 0.3))?;
            &flattened
        } else {
            self
        };
        let mut leg = CmykToRgb::new(&policy.profile, &destination, options.conversion)?;
        let count = leaf_count(source.root_layers()).max(1) as f64;
        let mut index = 0usize;
        let mut layers = map_tree(source.root_layers(), &mut |layer| {
            let image = layer
                .cmyk_source()
                .ok_or_else(|| refused_sentence("A non-CMYK record"))?;
            let part = job.range(
                0.3 + 0.7 * index as f64 / count,
                0.3 + 0.7 * (index + 1) as f64 / count,
            );
            index += 1;
            let image = cmyk_to_rgb16_image(image, &mut leg, &destination, part)?;
            PrecisionDocumentLayer::raster(
                &layer.name,
                image,
                layer.offset(),
                layer.opacity,
                layer.visible,
            )
        })?;
        // An RGB document holds no inks (`style`'s CMYK INKS).
        without_inks(&mut layers);
        let mut out = PrecisionDocument::from_parts_and_channels(
            source.width,
            source.height,
            destination,
            source.metadata.clone(),
            source.resolution,
            layers,
            source.channels().to_vec(),
        )?;
        out.guides.clone_from(&source.guides);
        out.ruler_origin = source.ruler_origin;
        out.global_light = source.global_light;
        out.paths.set_entries(source.paths.entries.to_vec());
        let out = out.seeding_vector_masks(source.vector_mask_seeds(Clone::clone), limit, other);
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The CMYK Flatten Image: one opaque "Background" record holding the
    /// composite on paper (`cmyk_composite::on_paper`).
    pub(crate) fn cmyk_flatten(&self, job: JobContext<'_>) -> Result<Self, String> {
        self.require_cmyk_policy("Flatten Image")?;
        let (w, h) = self.dimensions();
        let mut stack = self.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        let image = CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                stack.row(i64::from(y), 0, &mut acc);
                for (dst, ink) in row.iter_mut().zip(acc.iter()) {
                    let paper = on_paper(*ink);
                    *dst = codes([paper[0], paper[1], paper[2], paper[3], 1.0]);
                }
                job.every(y as usize * w as usize)
            },
            job,
        )?;
        let leaf = PrecisionDocumentLayer::raster_cmyk("Background", image, (0, 0), 1.0, true)?;
        // Alpha channels survive a flatten, as in RGB.
        let mut next = self.rebuilt(w, h, vec![leaf], self.channels().to_vec())?;
        next.comps.clear();
        Ok(next)
    }

    /// The CMYK Flatten Image's admission (`flatten_image`'s CMYK arm): the
    /// new canvas plane and one ink row.
    pub(crate) fn cmyk_flatten_image(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let scratch = self.cmyk_flatten_scratch_bytes();
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        self.cmyk_flatten(job)
    }

    /// The CMYK Flatten Image's working memory: [`FLATTEN_BYTES_PER_PIXEL`]
    /// a canvas pixel, one ink row, the stack's masks and bookkeeping.
    pub(crate) fn cmyk_flatten_scratch_bytes(&self) -> u64 {
        let (w, h) = self.dimensions();
        u64::from(w) * u64::from(h) * FLATTEN_BYTES_PER_PIXEL
            + u64::from(w) * 20
            + self.cmyk_stack_scratch_bytes().unwrap_or(u64::MAX / 4)
            + BOOKKEEPING_BYTES
    }
}

/// One ink quantized at the declared depth: `code`, then at depth 8 the
/// nearest multiple of 257 — what an 8-bit document holds.
fn quantized(v: u16, depth: CmykDepth) -> u16 {
    match depth {
        CmykDepth::Sixteen => v,
        CmykDepth::Eight => ((u32::from(v) + 128) / 257 * 257) as u16,
    }
}

/// One u16 RGB image's samples as CMYK ink through `leg` at `depth` (depth 8
/// rounds every component, alpha too, to the ×257 grid): the per-leaf loop
/// of [`PrecisionDocument::convert_to_cmyk`], shared with the clipboard's
/// RGB → CMYK paste (`cmyk_payload_convert`).
pub(crate) fn rgb16_to_cmyk_image(
    image: &PrecisionImage,
    leg: &mut RgbToCmyk<'_>,
    depth: CmykDepth,
    job: JobContext<'_>,
) -> Result<CmykImage, String> {
    let (w, h) = image.dimensions();
    CmykImage::from_rows_with_job(
        w,
        h,
        |y, row| {
            let mut x = 0usize;
            for (_, run) in image.row_segments(y, 0, w) {
                let PrecisionRun::Rgba16(run) = run else {
                    return Err("CMYK Color requires a 16-bit RGB document".to_string());
                };
                for pixel in run {
                    let key =
                        u64::from(pixel[0]) << 32 | u64::from(pixel[1]) << 16 | u64::from(pixel[2]);
                    let ink = leg.ink(key, std::array::from_fn(|c| f64::from(pixel[c]) / 65535.0));
                    row[x] = [
                        quantized(code(ink[0] as f32), depth),
                        quantized(code(ink[1] as f32), depth),
                        quantized(code(ink[2] as f32), depth),
                        quantized(code(ink[3] as f32), depth),
                        quantized(pixel[3], depth),
                    ];
                    x += 1;
                }
            }
            job.every(y as usize * w as usize)
        },
        job,
    )
}

/// One CMYK image's ink as u16 RGB in `destination` through `leg`, alpha
/// kept: the per-leaf loop of [`PrecisionDocument::convert_cmyk_to_rgb`],
/// shared with the clipboard's CMYK → RGB paste (`cmyk_payload_convert`).
pub(crate) fn cmyk_to_rgb16_image(
    image: &CmykImage,
    leg: &mut CmykToRgb<'_>,
    destination: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = image.dimensions();
    let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
        w,
        h,
        |y, row| {
            let mut x = 0usize;
            for (_, run) in image.row_segments(y, 0, w) {
                for sample in run {
                    let ink4 = [sample[0], sample[1], sample[2], sample[3]];
                    let rgb = leg.rgb(ink_key(ink4), ink4.map(|v| f64::from(unit(v))));
                    let c = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
                    row[x] = [c(rgb[0]), c(rgb[1]), c(rgb[2]), sample[4]];
                    x += 1;
                }
            }
            job.every(y as usize * w as usize)
        },
        job,
    )?;
    PrecisionImage::from_rgba16_tiles_unseeded(plane, destination.clone())
}

/// One CMYK image's ink in another profile through `leg` at `depth` (depth
/// 8 rounds every component, alpha too, to the ×257 grid, as
/// [`rgb16_to_cmyk_image`] does): a paste between CMYK documents of
/// different profiles (#1 phase 4c).
pub(crate) fn cmyk_to_cmyk_image(
    image: &CmykImage,
    leg: &mut CmykToCmyk<'_>,
    depth: CmykDepth,
    job: JobContext<'_>,
) -> Result<CmykImage, String> {
    let (w, h) = image.dimensions();
    CmykImage::from_rows_with_job(
        w,
        h,
        |y, row| {
            let mut x = 0usize;
            for (_, run) in image.row_segments(y, 0, w) {
                for sample in run {
                    let ink4 = [sample[0], sample[1], sample[2], sample[3]];
                    let ink = leg.ink(ink_key(ink4), ink4.map(|v| f64::from(unit(v))));
                    row[x] = [
                        quantized(code(ink[0] as f32), depth),
                        quantized(code(ink[1] as f32), depth),
                        quantized(code(ink[2] as f32), depth),
                        quantized(code(ink[3] as f32), depth),
                        quantized(sample[4], depth),
                    ];
                    x += 1;
                }
            }
            job.every(y as usize * w as usize)
        },
        job,
    )
}
