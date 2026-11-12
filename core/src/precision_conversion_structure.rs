//! Structure-preserving precision conversion, 12H-1: an 8-bit document
//! becomes a 16-bit document with every supported record carried, beside
//! (never instead of) the flattened conversion in `precision_conversion`.
//!
//! The per-structure table is `designs/phase-12h-conversion-status.md`; this
//! module implements its ONE widening direction, 8→16 (12H-2 carries retained
//! sources and warp bindings too; the narrowings are
//! `precision_conversion_narrow`). Every byte code `v`
//! becomes `v · 257` — the flattened path's own `promote_samples` for RGBA,
//! the same product for mask and channel planes — so a sample is widened
//! exactly and never re-evaluated. What is NOT exact is disclosed per record
//! as a RENDERING DIFFERENCE (an isolated group composites in f32 rather than
//! with byte rounding, an adjustment evaluates typed f32 math rather than the
//! byte LUT, a feathered or partial-density mask, a layer style through the
//! typed planes, Blend If on the 16-bit ramp), what the typed model has no
//! field for would be DISCARDED by name (nothing today: the global light
//! carries), and what cannot be carried BLOCKS by name with the input
//! untouched: host metadata that is neither an adjustment nor a text/shape
//! description, an adjustment the typed interpreter refuses, a blend mode the
//! typed target refuses, an oversize style plane, a linear-light policy the
//! typed walk refuses, and more records than a 16-bit document holds.
//!
//! The PLAN and the OP share one walk ([`Planes`]): the plan builds the same
//! records over 1×1 placeholder planes — no canvas-sized allocation — and
//! runs the typed constructors, the evaluation switch and the native capacity
//! plan on that SHAPE with the real extents, so a refusal the op would meet
//! is a plan row first. The op refuses with the plan's first row before any
//! pixel is promoted.
//!
//! Indices: a byte document stores its stack bottom-first with a per-entry
//! level, which is exactly the typed postorder numbering
//! (`precision_structure_ops`), so record `i` here is record `i` there and
//! every id is set directly from the byte layer — never re-minted
//! (`doc_identity`). Channel ids are session-only at both depths and are
//! minted fresh.
//!
//! 12H-1's structure-preserving 8→16 conversion beside the untouched flattened one, reached through
//! `dr_doc_promote_structure_plan` / `dr_doc_promote_structure` /
//! `dr_doc_promote_structure_preview` in `ffi_precision_conversion.rs`. ONE walk builds a
//! `precision_structure_ops::FlatRecord` per byte entry (index `i` is record `i`; every id set
//! directly, never re-minted) and `rebuild`s: rasters through
//! `precision_conversion::promote_samples` (×257), masks/channels by the same product, adjustments
//! under the typed envelope (`precision_v1`, and `legacy_srgb` stated for Exposure/White Balance),
//! text/shape descriptions verbatim, styles via `with_layer_style` (a styled Pass Through group
//! stays Pass Through and composites isolated); then `build`, `with_evaluation`,
//! comps, guides and ruler origin. The PLAN
//! runs that same walk over 1×1 placeholder planes and then `native_fits_geometry` with the real
//! extents, so every refusal is a named `blocked` row first — including a document over
//! `MAX_RECORDS` (the FIRST row, at index 1024) — and the op refuses with it before promoting a
//! plane. The per-structure table for all six directions is
//! `designs/phase-12h-conversion-status.md`; the conversions into float are
//! `precision_conversion_float`, which runs this walk's SHAPE ([`StructureShape`]) for its 8→float
//! plan and [`DrDocument::promote_structure_with_context`] as its first step. Since 12H-2 a
//! retained source and a warp binding CARRY 8→16 (`carry_bindings`): the typed binding's `rendered`
//! planes are the very allocations the record holds (`effective_source` / `precision_warp`'s guard
//! compare POINTERS), originals are widened ×257, a stale binding is disclosed and not carried, and
//! a sourced record's SHAPE has its real extent because `build` validates the fallback. A
//! document-level blocked row has id 0 and `refusal()` names no layer.

use std::sync::Arc;

use image::GrayImage;
use serde_json::{json, Value};

use crate::blend::BlendMode;
use crate::doc::{DrDocument, Layer, LayerKind, MAX_PIXELS};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_adjustment::{AdjustmentAdmission, PrecisionAdjustmentState};
use crate::precision_conversion::promote_raster;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, process_working_limit, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES,
};
use crate::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use crate::precision_render::MAX_RECORDS;
use crate::precision_structure_ops::{rebuild, FlatRecord};
use crate::resources::Inventory;
use crate::retained_source::RetainedSource16;
use crate::warp::WarpBinding16;

/// What the preserve conversion carries, in the status table's order.
const PRESERVED: [&str; 29] = [
    "raster_pixels_times_257",
    "layer_offsets",
    "names",
    "opacity",
    "visibility",
    "blend_modes",
    "groups",
    "pass_through_groups",
    "group_open_state",
    "adjustment_layers",
    "text_shape_descriptions",
    "layer_masks",
    "saved_channels",
    "layer_styles",
    "global_light",
    "blend_if",
    "knockout",
    "clipping",
    "locks",
    "links",
    "labels",
    "layer_ids",
    "layer_comps",
    "guides",
    "ruler_origin",
    "print_resolution",
    "icc_profile",
    "capture_metadata",
    "evaluation_policy",
];

/// The typed `linear_light` transfer the op builds when the policy carries:
/// three 65536-entry f32 decode tables plus the shared encoder, rounded up.
const TRANSFER_BYTES: u64 = 4 * 1024 * 1024;

/// Which direction a structure plan describes: the 12H-1 widening, one of
/// the three 12H-2 narrowings (`precision_conversion_narrow`), or one of the
/// two conversions into float (`precision_conversion_float`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionDirection {
    Rgba8ToRgba16,
    Rgba16ToRgba8,
    FloatToRgba16,
    FloatToRgba8,
    Rgba16ToFloat,
    Rgba8ToFloat,
}

impl ConversionDirection {
    fn name(self) -> &'static str {
        match self {
            Self::Rgba8ToRgba16 => "rgba8_to_rgba16",
            Self::Rgba16ToRgba8 => "rgba16_to_rgba8",
            Self::FloatToRgba16 => "rgba32_float_to_rgba16",
            Self::FloatToRgba8 => "rgba32_float_to_rgba8",
            Self::Rgba16ToFloat => "rgba16_to_rgba32_float",
            Self::Rgba8ToFloat => "rgba8_to_rgba32_float",
        }
    }

    /// True for a conversion INTO float.
    fn into_float(self) -> bool {
        matches!(self, Self::Rgba16ToFloat | Self::Rgba8ToFloat)
    }
}

/// The 8→16 note of a carried Blend If, which an 8→float plan replaces with
/// its own linear-light row (`precision_conversion_float`).
pub(crate) const BLEND_IF_16_NOTE: &str = "Blend If evaluates on the 16-bit ramp (10D)";

/// The note of a binding that is not carried, at every direction: the 8→16
/// walk's and the float conversion's (`precision_conversion_float`).
pub(crate) const STALE_NOTE: &str = "stale binding not carried; a save drops it too";

/// The 8→16 note of a carried retained source, which an 8→float plan
/// replaces with its own linear-light row.
pub(crate) const SOURCE_16_NOTE: &str =
    "source re-renders at 16 bits on its next edit; its cached rendering is widened ×257";

/// The 8→16 note of a carried warp, which an 8→float plan replaces with its
/// blocked row (a float document holds no warp binding).
pub(crate) const WARP_16_NOTE: &str = "warp re-edits at 16 bits from the widened original";

/// The 8→16 plan with the typed SHAPE it was checked on: `document` is the
/// 1×1-plane typed document at the real canvas, `Some` exactly when nothing
/// blocked, and `rasters`, `masks` and `channels` are the real extents its
/// native capacity was checked with.
pub(crate) struct StructureShape {
    pub plan: StructureConversionPlan,
    pub document: Option<PrecisionDocument>,
    pub rasters: Vec<(u32, u32)>,
    pub masks: Vec<(u32, u32)>,
    pub channels: usize,
}

/// One disclosed record: a rendering difference (`note`) or a blocking
/// reason (`note` again, reported as `reason`).
#[derive(Clone, Debug)]
pub struct StructureRow {
    pub index: usize,
    pub id: u64,
    pub name: String,
    pub note: String,
}

/// Retained input, working scratch and output bytes separately, the
/// `PrecisionConversionPlan` convention, plus the per-record disclosure.
#[derive(Clone, Debug)]
pub struct StructureConversionPlan {
    pub direction: ConversionDirection,
    /// The float source's map, reported beside a float direction.
    pub tone_map: Option<crate::float_document::ToneMap>,
    pub retained_bytes: u64,
    pub scratch_bytes: u64,
    pub output_bytes: u64,
    pub discarded: Vec<&'static str>,
    pub rendering_differences: Vec<StructureRow>,
    pub blocked: Vec<StructureRow>,
}

impl StructureConversionPlan {
    pub fn json(&self) -> Value {
        let rows = |rows: &[StructureRow], key: &str| -> Vec<Value> {
            rows.iter()
                .map(|row| json!({"index":row.index,"id":row.id,"name":row.name,key:row.note}))
                .collect()
        };
        let mut preserved = PRESERVED.to_vec();
        preserved[0] = match self.direction {
            ConversionDirection::Rgba8ToRgba16 => PRESERVED[0],
            ConversionDirection::FloatToRgba16 => "raster_pixels_tone_mapped",
            ConversionDirection::Rgba16ToFloat | ConversionDirection::Rgba8ToFloat => {
                "raster_pixels_decoded_to_linear_light"
            }
            _ => "raster_pixels_quantized",
        };
        if self.direction.into_float() {
            // A float document is linear light by construction: the source's
            // policy is not carried, it is replaced (a rendering-difference
            // row says so when it was encoded).
            preserved.retain(|name| *name != "evaluation_policy");
        }
        let mut value = json!({"flattened":false,"structure":"preserve",
            "direction":self.direction.name(),"retained_bytes":self.retained_bytes,
            "scratch_bytes":self.scratch_bytes,"output_bytes":self.output_bytes,
            "preserved":preserved,"discarded":self.discarded,
            "rendering_differences":rows(&self.rendering_differences, "note"),
            "blocked":rows(&self.blocked, "reason")});
        match self.direction {
            ConversionDirection::Rgba8ToRgba16 => {
                value["widening"] = json!("code_times_257");
            }
            ConversionDirection::FloatToRgba16 => {}
            ConversionDirection::Rgba16ToFloat | ConversionDirection::Rgba8ToFloat => {
                value["decoding"] = json!("profile_trc_to_linear_light");
                value["tone_map"] = json!(crate::float_document::ToneMap::Clip);
            }
            _ => {
                value["rgb_quantization"] = json!("explicit none or ordered_8x8_v1 per record");
                value["alpha_quantization"] = json!("nearest, never dithered");
            }
        }
        if let Some(map) = self.tone_map {
            value["tone_map"] = json!(map);
        }
        value
    }

    /// The first blocked row as the sentence a host shows, or `None` when
    /// the conversion may run.
    pub fn refusal(&self) -> Option<String> {
        self.blocked.first().map(|row| {
            if row.id == 0 {
                // A document-level row names no layer.
                return format!("Preserve Layers cannot convert this document: {}", row.note);
            }
            format!(
                "Preserve Layers cannot convert this document: layer {} \"{}\": {}",
                row.index, row.name, row.note
            )
        })
    }
}

/// Which planes the one walk materializes: the plan's 1×1 SHAPE, or the
/// op's promoted planes under its job.
#[derive(Clone, Copy)]
enum Planes<'a> {
    Shape,
    Promoted(JobContext<'a>),
}

#[derive(Default)]
struct Walk {
    flat: Vec<FlatRecord>,
    differences: Vec<StructureRow>,
    blocked: Vec<StructureRow>,
    /// The adjustment records: each measured record plus the peak
    /// preparation allowance, not one allowance per record.
    adjustments: AdjustmentAdmission,
    /// Bytes of the full-extent SHAPES a sourced record needs in the plan.
    shape: u64,
    output: u64,
    rasters: Vec<(u32, u32)>,
    masks: Vec<(u32, u32)>,
    /// The plan's admission refusal before an adjustment is prepared:
    /// the whole plan's `Err`, never a blocked row.
    admission_refusal: Option<String>,
}

fn row(index: usize, layer: &Layer, note: impl Into<String>) -> StructureRow {
    StructureRow {
        index,
        id: layer.id,
        name: layer.name.clone(),
        note: note.into(),
    }
}

/// `v · 257` over one byte coverage plane — `promote_samples`' product for a
/// scalar plane.
fn promoted_plane(plane: &GrayImage, job: JobContext<'_>) -> Result<PrecisionPlane, String> {
    let (w, h) = plane.dimensions();
    let mut out = reserved_vec(plane.as_raw().len())?;
    for (i, v) in plane.as_raw().iter().enumerate() {
        job.every(i)?;
        out.push(u16::from(*v) * 257);
    }
    PrecisionPlane::adopt(w, h, out)
}

fn placeholder_plane() -> Result<PrecisionPlane, String> {
    PrecisionPlane::adopt(1, 1, vec![0])
}

/// The byte adjustment meta as a typed description: the typed envelope's
/// `evaluation: precision_v1`, and — for Exposure and White Balance, whose
/// typed default is `document_v2` — the byte default `legacy_srgb` stated
/// explicitly, so the carried record keeps the meaning it had at 8 bits.
fn typed_adjustment(meta: &str) -> Result<(String, String), String> {
    let mut value: Value =
        serde_json::from_str(meta).map_err(|e| format!("invalid adjustment JSON: {e}"))?;
    let root = value
        .as_object_mut()
        .ok_or("adjustment description is not an object")?;
    let op = root
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    root.insert("evaluation".into(), Value::String("precision_v1".into()));
    if matches!(op.as_str(), "exposure" | "white_balance") {
        if let Some(params) = root
            .entry("params")
            .or_insert_with(|| Value::Object(Default::default()))
            .as_object_mut()
        {
            params
                .entry("evaluation")
                .or_insert_with(|| Value::String("legacy_srgb".into()));
        }
    }
    Ok((op, value.to_string()))
}

impl DrDocument {
    /// The one walk, bottom-first. In `Shape` a record that cannot be carried
    /// becomes a blocked row; in `Promoted` every error is the op's error.
    /// `admission` seeds the Shape walk's per-preparation admission;
    /// the Promoted walk passes the default and never calls `next`.
    fn structure_walk(
        &self,
        planes: Planes<'_>,
        admission: AdjustmentAdmission,
    ) -> Result<Walk, String> {
        let mut walk = Walk {
            adjustments: admission,
            ..Walk::default()
        };
        let count = self.layers.len().max(1) as f64;
        for (index, layer) in self.layers.iter().enumerate() {
            let planes = match planes {
                Planes::Promoted(job) => {
                    Planes::Promoted(job.range(index as f64 / count, (index + 1) as f64 / count))
                }
                Planes::Shape => Planes::Shape,
            };
            let result = self.structure_record(index, layer, planes, &mut walk);
            if let Some(reason) = walk.admission_refusal.take() {
                return Err(reason);
            }
            match result {
                Ok(record) => walk.flat.push(FlatRecord {
                    layer: record,
                    depth: usize::from(layer.depth),
                }),
                Err(reason) => match planes {
                    Planes::Shape => walk.blocked.push(row(index, layer, reason)),
                    Planes::Promoted(_) => return Err(reason),
                },
            }
        }
        Ok(walk)
    }

    fn structure_record(
        &self,
        index: usize,
        layer: &Layer,
        planes: Planes<'_>,
        walk: &mut Walk,
    ) -> Result<PrecisionDocumentLayer, String> {
        let canvas = (self.width, self.height);
        let profile = &self.profile;
        let mut notes: Vec<String> = Vec::new();
        let styled_effects = layer
            .style
            .as_deref()
            .is_some_and(|s| s.has_enabled_effects());
        let (record, carries_style) = match layer.kind {
            LayerKind::Group => {
                if layer.meta.is_some() {
                    return Err("a group carrying host metadata has no 16-bit record".into());
                }
                // A styled Pass Through group stays Pass Through and
                // composites isolated at both depths.
                let styled = layer.renders_style().is_some();
                let pass_through = layer.blend == BlendMode::PassThrough;
                if !pass_through || styled {
                    notes.push("isolated group composites in f32 instead of byte rounding".into());
                }
                let group = PrecisionDocumentLayer::group(
                    &layer.name,
                    Vec::new(),
                    layer.opacity,
                    layer.visible,
                    pass_through,
                )?;
                (group.with_blend_mode(layer.blend)?, true)
            }
            LayerKind::Raster
                if layer
                    .meta
                    .as_deref()
                    .and_then(crate::adjust::Adjustment::from_meta)
                    .is_some() =>
            {
                let meta = layer.meta.as_deref().unwrap_or_default();
                let (op, description) = typed_adjustment(meta)?;
                let refused =
                    |e: String| format!("the {op} adjustment has no 16-bit evaluation: {e}");
                if let Planes::Shape = planes {
                    // Admit this preparation BEFORE building it —
                    // the retained input, the records prepared so far, this
                    // one's allowance, and the bookkeeping plus the
                    // full-size shapes earlier sourced records still hold.
                    let bound =
                        PrecisionAdjustmentState::prepare_scratch_bytes(&description, profile)
                            .map_err(refused)?;
                    walk.adjustments.scratch = walk.shape.saturating_add(4 * BOOKKEEPING_BYTES);
                    if let Err(reason) = walk.adjustments.next(bound, JobContext::NONE) {
                        walk.admission_refusal = Some(reason.clone());
                        return Err(reason);
                    }
                }
                let record = PrecisionDocumentLayer::adjustment(
                    &layer.name,
                    &description,
                    layer.opacity,
                    layer.visible,
                    profile,
                )
                .map_err(refused)?;
                if let Content::Adjustment(state) = &record.content {
                    walk.adjustments.plan_add(state, profile)?;
                }
                notes.push("adjustment evaluates typed f32 math instead of the byte LUT".into());
                // An adjustment carries its style (and Blend If) at both
                // depths (`precision_style_adjustment`).
                (record.with_blend_mode(layer.blend)?, true)
            }
            LayerKind::Raster => {
                if let Some(meta) = layer.meta.as_deref() {
                    crate::artwork_document::validate(meta).map_err(|_| {
                        "a host record (Live Photo, host media or an unknown kind) that a 16-bit document cannot hold"
                            .to_string()
                    })?;
                }
                let (w, h) = layer.pixels.dimensions();
                pixel_count(w, h)?;
                if let Some(style) = layer.style.as_deref().filter(|_| styled_effects) {
                    if crate::precision_style_render::plane_pixels((w, h), style.pad())?
                        > MAX_PIXELS
                    {
                        return Err("its layer style's padded plane exceeds the pixel limit".into());
                    }
                }
                let image = match planes {
                    // A carried source's binding is validated against its
                    // fallback's extent by `build`, so that one record's
                    // shape has the real extent. Those shapes stay live
                    // until the walk ends, so their running total is
                    // admitted before each allocation and charged to the
                    // plan's scratch.
                    Planes::Shape if layer.effective_source(profile).is_some() => {
                        walk.shape = walk.shape.saturating_add(u64::from(w) * u64::from(h) * 8);
                        admit(0, walk.shape, 0, process_working_limit())?;
                        PrecisionImage::filled_rgba16(w, h, [0; 4], profile.clone())?
                    }
                    Planes::Shape => PrecisionImage::from_rgba16_unseeded(
                        1,
                        1,
                        vec![[0; 4]],
                        profile.clone(),
                        None,
                    )?,
                    Planes::Promoted(job) => PrecisionImage::from_rgba16_unseeded(
                        w,
                        h,
                        promote_raster(&layer.pixels, job.range(0.0, 0.9))?,
                        profile.clone(),
                        None,
                    )?,
                };
                walk.output = walk.output.saturating_add(u64::from(w) * u64::from(h) * 8);
                walk.rasters.push((w, h));
                let mut record = PrecisionDocumentLayer::raster(
                    &layer.name,
                    image,
                    layer.offset,
                    layer.opacity,
                    layer.visible,
                )?;
                record.description = layer.meta.as_ref().map(|m| Arc::new(m.clone()));
                (record.with_blend_mode(layer.blend)?, true)
            }
        };
        let mut record = if carries_style {
            let style = layer.style.as_deref();
            if let Some(style) = style {
                if style.fill_opacity < 1.0
                    || styled_effects
                    || style.knockout != crate::style::Knockout::None
                {
                    notes.push("layer style renders through the typed style planes".into());
                }
                if style.blend_if.is_some_and(|b| !b.is_identity()) {
                    notes.push(BLEND_IF_16_NOTE.into());
                }
            }
            record.with_layer_style(style, canvas)?
        } else {
            record
        };
        if let Some(mask) = &layer.mask {
            let dims = mask.dimensions();
            if !layer.mask_properties.valid(dims) {
                return Err("its layer mask properties exceed the typed mask limits".into());
            }
            let plane = match planes {
                Planes::Shape => placeholder_plane()?,
                Planes::Promoted(job) => promoted_plane(mask, job.range(0.9, 1.0))?,
            };
            walk.output = walk
                .output
                .saturating_add(u64::from(dims.0) * u64::from(dims.1) * 2);
            walk.masks.push(dims);
            if layer.mask_properties.feather > 0.0 || layer.mask_properties.density < 1.0 {
                notes.push("mask feather/density evaluate in f32 instead of byte rounding".into());
            }
            record = record.with_mask(Some(PrecisionMask::new(
                plane,
                layer.mask_properties.clone(),
                layer.mask_enabled,
            )?));
        }
        self.carry_bindings(layer, planes, &mut record, &mut notes, walk)?;
        record.id = layer.id;
        record.clipped = layer.clipped;
        record.label = layer.label;
        record.reference = layer.reference;
        record.locks = layer.locks & crate::doc_lock::LOCK_ALL;
        record.link = layer.link;
        record = record.with_open(layer.open);
        walk.differences
            .extend(notes.into_iter().map(|note| row(index, layer, note)));
        Ok(record)
    }

    /// A retained source and a warp binding, carried with every guard input
    /// tied to the record they land on (`retained_source::effective_source`,
    /// `precision_warp`'s guard); a stale binding is disclosed, not carried.
    fn carry_bindings(
        &self,
        layer: &Layer,
        planes: Planes<'_>,
        record: &mut PrecisionDocumentLayer,
        notes: &mut Vec<String>,
        walk: &mut Walk,
    ) -> Result<(), String> {
        let profile = &self.profile;
        // A live Generative Layer's variations rebind to the widened raster:
        // the pictures stay in the document's encoding (`doc_gen_variations`).
        if let (Some(binding), Some(rendered)) =
            (layer.effective_generated(profile), record.source().cloned())
        {
            record.generated = Some(crate::doc_gen_variations::GeneratedBinding16 {
                table: binding.table,
                rendered: crate::doc_gen_variations::GeneratedPlane16::Rgb(rendered),
                rendered_offset: layer.offset,
                profile: profile.clone(),
            });
        }
        if layer.retained_source.is_some() {
            match (layer.effective_source(profile), record.source().cloned()) {
                (Some(source), Some(rendered)) => {
                    notes.push(SOURCE_16_NOTE.into());
                    record.retained_source = Some(RetainedSource16 {
                        asset: source.asset,
                        link: source.link,
                        transform: source.transform,
                        filter: source.filter,
                        rendered,
                        rendered_offset: layer.offset,
                    });
                }
                _ => notes.push(STALE_NOTE.into()),
            }
        }
        let Some(binding) = layer.warp.as_ref().map(|_| layer.effective_warp(profile)) else {
            return Ok(());
        };
        let Some(binding) = binding else {
            notes.push(STALE_NOTE.into());
            return Ok(());
        };
        let job = match planes {
            Planes::Promoted(job) => Some(job),
            Planes::Shape => None,
        };
        let mut source_pixels = None;
        if let Some(pixels) = &binding.source_pixels {
            let (w, h) = pixels.dimensions();
            walk.output = walk.output.saturating_add(u64::from(w) * u64::from(h) * 8);
            source_pixels = Some(match job {
                None => {
                    PrecisionImage::from_rgba16_unseeded(1, 1, vec![[0; 4]], profile.clone(), None)?
                }
                Some(job) => PrecisionImage::from_rgba16_unseeded(
                    w,
                    h,
                    promote_raster(pixels, job)?,
                    profile.clone(),
                    None,
                )?,
            });
        }
        let mut source_mask = None;
        if let Some(mask) = &binding.source_mask {
            let (w, h) = mask.dimensions();
            walk.output = walk.output.saturating_add(u64::from(w) * u64::from(h) * 2);
            source_mask = Some(match job {
                None => placeholder_plane()?,
                Some(job) => promoted_plane(mask, job)?,
            });
        }
        notes.push(WARP_16_NOTE.into());
        record.warp = Some(WarpBinding16 {
            field: binding.field.clone(),
            target: binding.target,
            source_pixels,
            source_offset: binding.source_offset,
            source_mask,
            source_mask_offset: binding.source_mask_offset,
            rendered_pixels: binding.rendered_pixels.and(record.source().cloned()),
            rendered_offset: layer.offset,
            rendered_mask: binding
                .rendered_mask
                .and(record.mask().map(|m| m.plane().clone())),
            rendered_mask_offset: layer.mask_properties.offset,
            profile: profile.clone(),
            linked_mask: binding.linked_mask,
            mask_outside: binding.mask_outside,
        });
        Ok(())
    }

    /// Canvas-sized channels, promoted or (for the plan) name-checked only.
    fn structure_channels(&self, planes: Planes<'_>) -> Result<Vec<PrecisionChannel>, String> {
        let mut out = Vec::with_capacity(self.channels.len());
        for channel in &self.channels {
            let plane = match planes {
                Planes::Shape => placeholder_plane()?,
                Planes::Promoted(job) => promoted_plane(&channel.data, job)?,
            };
            let mut typed = PrecisionChannel::new(&channel.name, plane)?;
            typed.metadata.overlay_color = channel.overlay_color;
            typed.metadata.overlay_opacity = channel.overlay_opacity;
            typed.metadata.color_indicates_selected = channel.color_indicates_selected;
            typed.metadata.spot = channel.spot;
            out.push(typed);
        }
        Ok(out)
    }

    /// Every saved channel promoted `v · 257` with its options and ink: the
    /// flattened 8→16 conversion's channels (`precision_conversion`, #1).
    pub(crate) fn promoted_channels(
        &self,
        job: JobContext<'_>,
    ) -> Result<Vec<PrecisionChannel>, String> {
        self.structure_channels(Planes::Promoted(job))
    }

    /// The typed document from the walked records: the one construction
    /// body, then the policy, the comps and the canvas chrome carried.
    fn structure_document(
        &self,
        flat: Vec<FlatRecord>,
        channels: Vec<PrecisionChannel>,
    ) -> Result<PrecisionDocument, String> {
        let layers = rebuild(flat)?;
        let mut result = PrecisionDocument::build(
            self.width,
            self.height,
            self.profile.clone(),
            self.metadata.clone(),
            self.resolution,
            layers,
            channels,
            None,
        )?;
        // The policy carries and its tables are rebuilt at the TARGET depth
        // (`doc_evaluation`); a typed refusal is an error, never a silent
        // policy change.
        if let Some(switched) = result.with_evaluation(self.evaluation())? {
            result = switched;
        }
        result.comps.clone_from(&self.comps);
        result.background = self.background;
        result.guides.clone_from(&self.guides);
        result.ruler_origin = self.ruler_origin;
        result.color_samplers = self.color_samplers().to_vec();
        result.paths.entries.clone_from(&self.paths.entries);
        result.global_light = self.global_light;
        Ok(result)
    }

    /// The plan: every record walked over its SHAPE, the typed document
    /// built from that shape, and the native capacity plan run on it with
    /// the real extents. No canvas-sized plane is allocated.
    pub fn promote_structure_plan(&self) -> Result<StructureConversionPlan, String> {
        self.promote_structure_plan_within(process_working_limit(), 0)
    }

    /// The plan against a caller's `limit` and `other`: each adjustment
    /// preparation is admitted before it is built, and a refusal is
    /// the whole plan's `Err`.
    fn promote_structure_plan_within(
        &self,
        limit: u64,
        other: u64,
    ) -> Result<StructureConversionPlan, String> {
        Ok(self.promote_structure_shape(limit, other)?.plan)
    }

    /// The plan with the typed shape it checked ([`StructureShape`]): the
    /// 8→float plan (`precision_conversion_float`) reads its rows over that
    /// shape and charges its capacity as a float document.
    pub(crate) fn promote_structure_shape(
        &self,
        limit: u64,
        other: u64,
    ) -> Result<StructureShape, String> {
        pixel_count(self.width, self.height)?;
        let mut blocked = Vec::new();
        // FIRST: the byte document has no in-memory record cap
        // (`MAX_DRDC_LAYERS` applies only at save), so the typed cap is
        // named here rather than surfacing as the constructor's sentence.
        if self.layers.len() > MAX_RECORDS {
            let layer = &self.layers[MAX_RECORDS];
            blocked.push(row(
                MAX_RECORDS,
                layer,
                format!(
                    "{} layers exceed the {MAX_RECORDS}-record limit of a 16-bit document",
                    self.layers.len()
                ),
            ));
        }
        let mut inventory = Inventory::default();
        inventory.document(self);
        crate::resources::cache_inventory(&[self], &mut inventory);
        let retained_bytes = inventory.unique_bytes();
        let walk = self.structure_walk(
            Planes::Shape,
            AdjustmentAdmission {
                base: retained_bytes,
                scratch: 4 * BOOKKEEPING_BYTES,
                other,
                limit,
                prepared: 0,
                peak: 0,
            },
        )?;
        blocked.extend(walk.blocked);
        let document_row = |note: String| StructureRow {
            index: self.layers.len(),
            id: 0,
            name: "document".into(),
            note,
        };
        let channels = match self.structure_channels(Planes::Shape) {
            Ok(_) => self.channels.len(),
            Err(reason) => {
                blocked.push(document_row(reason));
                0
            }
        };
        let mut document = None;
        if blocked.is_empty() {
            let shaped = self
                .structure_document(walk.flat, Vec::new())
                .and_then(|shape| {
                    shape.native_fits_geometry(
                        (self.width, self.height),
                        &walk.rasters,
                        &walk.masks,
                        channels,
                    )?;
                    Ok(shape)
                });
            match shaped {
                Ok(shape) => document = Some(shape),
                Err(reason) => blocked.push(document_row(reason)),
            }
        }
        let canvas = u64::from(self.width) * u64::from(self.height);
        let output = walk
            .output
            .saturating_add(canvas.saturating_mul(2 * self.channels.len() as u64));
        let transfer = if self.evaluation() == crate::doc_evaluation::Evaluation::LinearLight {
            TRANSFER_BYTES
        } else {
            0
        };
        let plan = StructureConversionPlan {
            direction: ConversionDirection::Rgba8ToRgba16,
            tone_map: None,
            retained_bytes,
            scratch_bytes: walk
                .adjustments
                .total()?
                .saturating_add(walk.shape)
                .saturating_add(transfer)
                .saturating_add(4 * BOOKKEEPING_BYTES),
            output_bytes: output,
            discarded: Vec::new(),
            rendering_differences: walk.differences,
            blocked,
        };
        Ok(StructureShape {
            plan,
            document,
            rasters: walk.rasters,
            masks: walk.masks,
            channels,
        })
    }

    /// 8→16 with every supported record carried. Refuses with the plan's
    /// first blocked row before any plane is promoted; the input is never
    /// mutated.
    pub fn promote_structure(
        &self,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionDocument, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.promote_structure_with_context(limit, other, job)
    }

    /// [`Self::promote_structure`] under a caller's job: the 8→float
    /// conversion runs it as its first step (`precision_conversion_float`).
    pub(crate) fn promote_structure_with_context(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionDocument, String> {
        job.check()?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let plan = self.promote_structure_plan_within(limit, other)?;
        if let Some(reason) = plan.refusal() {
            return Err(reason);
        }
        let scratch = plan.scratch_bytes.saturating_add(plan.output_bytes);
        admit(plan.retained_bytes, scratch, other, limit)?;
        job.reserve(scratch)?;
        let walk = self.structure_walk(
            Planes::Promoted(job.range(0.0, 0.9)),
            AdjustmentAdmission::default(),
        )?;
        let channels = self.structure_channels(Planes::Promoted(job.range(0.9, 0.95)))?;
        // Raster-less vector masks are re-set on the typed document from
        // their shapes (`precision_vector_mask`); one with a raster part
        // crosses as the promoted plane it baked.
        let result = self
            .structure_document(walk.flat, channels)?
            .seeding_vector_masks(self.vector_mask_seeds(Clone::clone), limit, other);
        // The comps and chrome were attached after `build`'s own preflight.
        result.native_preflight()?;
        job.report(1.0, 5)?;
        Ok(result)
    }

    /// Per-channel |a − b| on straight u16 codes between this document's
    /// 8-bit projection ×257 and the converted document's 16-bit render;
    /// RGB is skipped where BOTH alphas are 0 (latent colour, invisible).
    pub fn promote_structure_preview(
        &self,
        converted: &PrecisionDocument,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        if converted.dimensions() != (self.width, self.height) || converted.is_float() {
            return Err("the converted document is not this document's 16-bit twin".into());
        }
        let before = self.flattened()?;
        job.report(0.2, 5)?;
        let after = converted.render_with_context(limit, other, job.range(0.2, 0.9))?;
        let runs = after
            .runs_rgba16()
            .ok_or("the converted projection is not 16-bit")?;
        let (mut max, mut sum, mut compared, mut skipped) = (0_u16, 0_u64, 0_u64, 0_u64);
        let mut bytes = before.as_raw().chunks_exact(4);
        let mut i = 0_usize;
        'walk: for run in runs {
            for a in run {
                let Some(b) = bytes.next() else {
                    break 'walk;
                };
                job.every(i)?;
                i += 1;
                let wide: [u16; 4] = std::array::from_fn(|c| u16::from(b[c]) * 257);
                let channels = if wide[3] == 0 && a[3] == 0 {
                    skipped += 3;
                    3..4
                } else {
                    0..4
                };
                for c in channels {
                    let delta = wide[c].abs_diff(a[c]);
                    max = max.max(delta);
                    sum += u64::from(delta);
                    compared += 1;
                }
            }
        }
        let mean = if compared == 0 {
            0.0
        } else {
            sum as f64 / compared as f64
        };
        job.report(1.0, 5)?;
        Ok(json!({"scale":65535,"max_delta":max,"mean_delta":mean,
            "compared_samples":compared,"skipped_latent_rgb":skipped}))
    }
}
