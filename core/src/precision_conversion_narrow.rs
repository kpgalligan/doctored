//! Structure-preserving precision NARROWING, 12H-2: a 16-bit document becomes
//! an 8-bit one, and a 32-bit float document becomes a 16-bit or an 8-bit
//! one, with every supported record carried — beside (never instead of) the
//! flattened conversions in `precision_conversion` and `float_document`.
//!
//! The per-structure table is `designs/phase-12h-conversion-status.md`. The
//! three directions are ONE walk over `precision_structure_ops::flatten` with
//! three per-leaf sample mappings, each an existing implementation:
//!
//! * u16 → u8: `PrecisionImage::quantize_with_context` (RGB nearest or
//!   ordered 8×8, alpha nearest, never dithered) per record — the flattened
//!   path's quantizer; masks, channels and warp originals nearest
//!   (`precision_quantize::nearest`).
//! * f32 → u16: `PrecisionDocument::tone_mapped_u16` per record — the float
//!   document's own map and forward curve.
//! * f32 → u8: that tone map, then that quantizer.
//!
//! What is NOT exact is disclosed per record as a RENDERING DIFFERENCE (an
//! isolated group composites with byte rounding, an adjustment evaluates the
//! byte LUT, every float raster is tone-mapped BEFORE compositing rather than
//! after), what the target has no field for is DISCARDED by name (a float
//! document's extended range), and what cannot be carried BLOCKS by name
//! with the input untouched: a record nested deeper than an 8-bit document
//! holds (the byte `doc_group::MAX_GROUP_DEPTH` is 10, the typed one 16), an
//! adjustment the byte interpreter cannot read, and a result past the
//! target's native capacity.
//!
//! The PLAN and the OP share the walk ([`Planes`]): the plan builds the same
//! records over 1×1 placeholder planes and runs the assembly, the evaluation
//! switch and the native capacity checks on that SHAPE with the real extents;
//! the op refuses with the plan's first row before any plane is built.
//!
//! Indices: the flattened postorder IS the byte stack's order, so record `i`
//! here is record `i` there and every id is set directly from the typed
//! record (`doc_identity`). Channel ids are session-only and minted fresh.
//!
//! Reached through `dr_precision_doc_narrow_structure_plan` / `_to_u8` / `_to_u16` / `_preview`.
//! The plan walks 1×1 SHAPES, blocks a record nested deeper than the byte
//! `doc_group::MAX_GROUP_DEPTH` (10) FIRST, then assembles the shaped target and runs its native
//! capacity (`native_layers_fit_dimensions` / `native_masks_fit_dimensions` / `channels_fit_at`, or
//! `native_fits_geometry`). A carried byte binding's `rendered_pixels` / `rendered_mask` are the
//! landed `Layer.pixels` / `Layer.mask` Arcs and its `profile` the assembled document's, so
//! `detach_invalid_sources` drops nothing. A MASKED adjustment lands on a canvas-sized transparent
//! plane, because a byte adjustment's mask covers only its pixel extent
//! (`doc::composite_adjustment_into`); an unmasked one keeps 1×1. Every float raster carries a
//! rendering-difference row (tone-mapped BEFORE compositing); `extended_range` is discarded; the
//! u16 result is `linear_light`. The byte result passes `native_save_preflight` as a backstop.
//! A float text or shape layer keeps its description (measured against Photoshop
//! 26.10.0, which keeps both kinds through 32 -> 16 with its colour re-coded to look the same)
//! with the note [`DESCRIBED_NOTE`]; description bytes ride verbatim; the host reads them in the
//! destination's ink (receipt take2 PT/PSH, NR).
//! A float record with a layer style or Blend If carries [`STYLE_NOTE`]: the narrowed document
//! is linear light, where Blend If reads encoded values (`doc_evaluation::encoded_view`).
//! A binding whose master is 32-bit float rides too (measured against
//! Photoshop 26.10.0, which keeps 32-bit contents through
//! Convert to 16 and to 8 bits and renders them clipped: receipt
//! `float-low-depth-600/take1`, NR16 and NR8): the binding keeps its
//! tone-mapped leaf as `rendered` and re-renders by the low-depth rule
//! (`retained_source_render`'s `float_master_codes`) on its next edit.
//! A binding whose 32-bit float master is a Camera RAW development
//! BLOCKS instead ([`FLOAT_RAW_NARROW_REFUSAL`]): the master is a scene-linear
//! render a 16-bit development of the same recipe does not equal, and an 8- or
//! 16-bit document's RAW layer redevelops to a u16 master, so the record never
//! leaves float documents; it narrows once redeveloped as a 16-bit SDR master.
//!
//! # Admission
//!
//! The plan is scratch + output. A raster leaf at 8 bits also charges the
//! tiling copies that run beside it and land in no output: the tone-mapped
//! u16 plane's (8 bytes a pixel, float only; float→16 keeps that copy as its
//! output) and the quantized bytes' (4 bytes a pixel, both directions; and
//! the quantized warp original's). Before, a bare 24 MP float document's
//! float→8 plan was 292,194,304 against a measured 385,483,064 (0.758×).
//! Measured 2026-10-10, Release, `MallocLargeCache=0`, limit 8,589,934,592,
//! one case per process, three runs, a baseline twin at E 0 ×3 (receipt
//! `convert-smart-object-158/admission-600p3b/runs/20261010-120706`): a white
//! 6000×4000 float parent holding a 6000×2000 float file as layered
//! smart-object contents (a build before 2026-10-10), narrowed to 16 and to
//! 8 bits, and that parent alone to 8 bits.
//!
//! | case | A | max E | A / max E |
//! | --- | --- | --- | --- |
//! | narrow16c | 580,194,304 | 383,172,896 | 1.514 |
//! | narrow8c | 868,194,304 | 385,450,248 | 2.252 |
//! | narrow8w | 580,194,304 | 385,483,064 | 1.505 |
//!
//! `tests/float_master_24mp.rs` pins the floors (⌈1.25 × max E⌉).

use std::iter;
use std::sync::Arc;

use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};

use crate::blend::BlendMode;
use crate::doc::{DrDocument, Layer, LayerKind, LayerRaster};
use crate::doc_channel::Channel;
use crate::doc_evaluation::Evaluation;
use crate::doc_group::MAX_GROUP_DEPTH;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_adjustment::{AdjustmentAdmission, PrecisionAdjustmentState};
use crate::precision_conversion_structure::{
    ConversionDirection, StructureConversionPlan, StructureRow,
};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, process_working_limit, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES,
};
use crate::precision_mask::{PrecisionChannel, PrecisionPlane};
use crate::precision_quantize::{nearest, PrecisionDither};
use crate::precision_structure_ops::{flatten, rebuild, FlatRecord};
use crate::retained_source::RetainedSource8;
use crate::style::LayerStyle;
use crate::warp::WarpBinding8;

/// The head of the note a float text or shape record carries: ``text or
/// shape keeps its description; it re-renders at {16|8} bits on the next
/// edit``.
const DESCRIBED_NOTE: &str = "text or shape keeps its description; it re-renders at";

/// The note of a float record with a layer style or Blend If: the narrowed
/// document evaluates in linear light at 8 or 16 bits, where Blend If reads
/// encoded values and a gradient effect interpolates its encoded ramp, while
/// the float document reads stored linear values.
const STYLE_NOTE: &str = "style and Blend If composite in linear light at the narrowed depth, where Blend If reads encoded values and a gradient effect its encoded ramp; the float document read linear values";

/// The row note and the op's refusal for a record whose retained source is
/// a 32-bit float Camera RAW development (module doc).
pub(crate) const FLOAT_RAW_NARROW_REFUSAL: &str = "holds a 32-bit float Camera RAW development; redevelop it as a 16-bit SDR master or rasterize it first";

/// Whether `layer`'s live binding is a 32-bit float master with a camera
/// original ([`FLOAT_RAW_NARROW_REFUSAL`]). A stale binding answers false and
/// is dropped by the walk as any stale binding is.
fn float_raw_master(layer: &PrecisionDocumentLayer) -> bool {
    layer
        .effective_source()
        .is_some_and(|source| source.asset.depth() == 32 && source.asset.raw.is_some())
}

/// The narrowing's target depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NarrowTarget {
    Rgba8,
    Rgba16,
}

/// A narrowed document handed back to [`PrecisionDocument::narrow_structure_preview`].
pub enum Narrowed<'a> {
    Rgba8(&'a DrDocument),
    Rgba16(&'a PrecisionDocument),
}

/// Which planes the one walk materializes: the plan's 1×1 SHAPE, or the
/// op's real planes under its job.
#[derive(Clone, Copy)]
enum Planes<'a> {
    Shape,
    Real(JobContext<'a>),
}

#[derive(Default)]
struct Walk {
    bytes: Vec<Layer>,
    typed: Vec<FlatRecord>,
    differences: Vec<StructureRow>,
    blocked: Vec<StructureRow>,
    scratch: u64,
    /// The typed (float→16) adjustment records: each measured record plus
    /// the peak preparation allowance, not one allowance per record.
    adjustments: AdjustmentAdmission,
    output: u64,
    rasters: Vec<(u32, u32)>,
    masks: Vec<(u32, u32)>,
    /// The plan's admission refusal before an adjustment is prepared:
    /// the whole plan's `Err`, never a blocked row.
    admission_refusal: Option<String>,
}

fn row(index: usize, layer: &PrecisionDocumentLayer, note: impl Into<String>) -> StructureRow {
    StructureRow {
        index,
        id: layer.id,
        name: layer.name().to_string(),
        note: note.into(),
    }
}

fn area((w, h): (u32, u32)) -> u64 {
    u64::from(w) * u64::from(h)
}

/// One u16 coverage plane as bytes, `precision_quantize::nearest` per sample.
fn nearest_plane(plane: &PrecisionPlane, job: JobContext<'_>) -> Result<Arc<GrayImage>, String> {
    let (w, h) = plane.dimensions();
    let mut out = reserved_vec(plane.samples().len())?;
    for (i, v) in plane.samples().iter().enumerate() {
        job.every(i)?;
        out.push(nearest(*v));
    }
    Ok(Arc::new(
        GrayImage::from_raw(w, h, out).ok_or("invalid narrowed plane dimensions")?,
    ))
}

/// A fresh 1×1 plane: a group's private pixels, an adjustment's unused
/// raster, and every placeholder the plan's SHAPE holds. Fresh per call so
/// no two records share an allocation (`doc::Layer::kind`'s reason).
fn unit_pixels() -> LayerRaster {
    LayerRaster::unseeded(RgbaImage::new(1, 1))
}

impl PrecisionDocument {
    fn direction(&self, target: NarrowTarget) -> Result<ConversionDirection, String> {
        match (self.is_float(), target) {
            (false, NarrowTarget::Rgba8) => Ok(ConversionDirection::Rgba16ToRgba8),
            (false, NarrowTarget::Rgba16) => Err(
                "the document already stores 16-bit pixels; Preserve Layers narrows it to 8 bits"
                    .into(),
            ),
            (true, NarrowTarget::Rgba16) => Ok(ConversionDirection::FloatToRgba16),
            (true, NarrowTarget::Rgba8) => Ok(ConversionDirection::FloatToRgba8),
        }
    }

    /// The one walk, bottom-first over the flattened postorder. In `Shape` a
    /// record that cannot be carried becomes a blocked row; in `Real` every
    /// error is the op's error. `admission` seeds the Shape walk's
    /// per-preparation admission; the Real walk passes the default
    /// and never calls `next`.
    fn narrow_walk(
        &self,
        flat: &[FlatRecord],
        target: NarrowTarget,
        dither: PrecisionDither,
        planes: Planes<'_>,
        admission: AdjustmentAdmission,
    ) -> Result<Walk, String> {
        let mut walk = Walk {
            adjustments: admission,
            ..Walk::default()
        };
        let count = flat.len().max(1) as f64;
        for (index, record) in flat.iter().enumerate() {
            let planes = match planes {
                Planes::Real(job) => {
                    Planes::Real(job.range(index as f64 / count, (index + 1) as f64 / count))
                }
                Planes::Shape => Planes::Shape,
            };
            let mut notes = Vec::new();
            let landed = match target {
                NarrowTarget::Rgba8 => self
                    .narrow_to_byte(record, dither, planes, &mut notes, &mut walk)
                    .map(|layer| walk.bytes.push(layer)),
                NarrowTarget::Rgba16 => self
                    .narrow_to_typed(record, planes, &mut notes, &mut walk)
                    .map(|layer| {
                        walk.typed.push(FlatRecord {
                            layer,
                            depth: record.depth,
                        })
                    }),
            };
            if let Some(reason) = walk.admission_refusal.take() {
                return Err(reason);
            }
            match (landed, planes) {
                (Ok(()), _) => walk
                    .differences
                    .extend(notes.into_iter().map(|n| row(index, &record.layer, n))),
                (Err(reason), Planes::Shape) => {
                    walk.blocked.push(row(index, &record.layer, reason))
                }
                (Err(reason), Planes::Real(_)) => return Err(reason),
            }
        }
        Ok(walk)
    }

    /// The note a float text or shape record carries when it narrows to
    /// `bits` (module doc), pushed onto `notes`.
    fn described_note(&self, layer: &PrecisionDocumentLayer, bits: u32, notes: &mut Vec<String>) {
        if self.is_float() && layer.description.is_some() {
            notes.push(format!("{DESCRIBED_NOTE} {bits} bits on the next edit"));
        }
    }

    /// The note a float record with a layer style or Blend If carries
    /// ([`STYLE_NOTE`]), pushed onto `notes`: the narrowed document is
    /// linear light, where a style's effects and Blend If read ENCODED values
    /// (`doc_evaluation::encoded_view`), not the float document's stored
    /// linear ones.
    fn style_note(&self, layer: &PrecisionDocumentLayer, notes: &mut Vec<String>) {
        if self.is_float() && (layer.style.is_some() || layer.blend_if.is_some()) {
            notes.push(STYLE_NOTE.into());
        }
    }

    /// A float raster as u16 codes (`tone_mapped_u16`), a u16 one as itself.
    fn leaf_u16(
        &self,
        image: &PrecisionImage,
        planes: Planes<'_>,
        notes: &mut Vec<String>,
        walk: &mut Walk,
    ) -> Result<Option<PrecisionImage>, String> {
        if !self.is_float() {
            return Ok(Some(image.clone()));
        }
        notes.push("tone-mapped per record before compositing; where a sample exceeds SDR white or the map is not clip the composite differs from the flattened conversion".into());
        walk.scratch = walk.scratch.saturating_add(area(image.dimensions()) * 8);
        match planes {
            Planes::Shape => Ok(None),
            Planes::Real(job) => self.tone_mapped_u16(image, job).map(Some),
        }
    }

    /// One record at 8 bits.
    fn narrow_to_byte(
        &self,
        record: &FlatRecord,
        dither: PrecisionDither,
        planes: Planes<'_>,
        notes: &mut Vec<String>,
        walk: &mut Walk,
    ) -> Result<Layer, String> {
        let layer = &record.layer;
        if self.is_float() && layer.warp.is_some() {
            return Err("a 32-bit float document holds no warp binding".into());
        }
        // Before any charge or shape lands: the carry below never sees it.
        if float_raw_master(layer) {
            return Err(FLOAT_RAW_NARROW_REFUSAL.into());
        }
        let job = match planes {
            Planes::Real(job) => Some(job),
            Planes::Shape => None,
        };
        let (kind, pixels, offset, meta, blend) = match &layer.content {
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                return Err(crate::cmyk_document::refused_sentence(
                    "Convert to 8 Bits/Channel",
                ))
            }
            Content::Raster { image, offset } => {
                let dims = image.dimensions();
                pixel_count(dims.0, dims.1)?;
                let pixels = match (self.leaf_u16(image, planes, notes, walk)?, job) {
                    (Some(wide), Some(job)) => {
                        LayerRaster::unseeded(wide.quantize_with_context(dither, job)?)
                    }
                    _ => unit_pixels(),
                };
                // Two tiling copies run here and land in no output: the
                // tone-mapped u16 plane tiles beside its Vec
                // (`from_float_samples_unseeded`; the copy float→16 keeps as
                // its output), and the quantized bytes tile beside theirs
                // (`LayerRaster::unseeded`). Uncharged, a 24 MP float→8 plan
                // was 0.758× its measured peak (module doc).
                let tiling = if self.is_float() { 8 + 4 } else { 4 };
                walk.scratch = walk.scratch.saturating_add(area(dims) * tiling);
                walk.output = walk.output.saturating_add(area(dims) * 4);
                walk.rasters.push(dims);
                self.described_note(layer, 8, notes);
                let meta = layer.description.as_deref().cloned();
                (LayerKind::Raster, pixels, *offset, meta, layer.blend_mode)
            }
            Content::Adjustment(state) => {
                let description = state.description();
                if crate::adjust::Adjustment::from_meta(description).is_none() {
                    return Err(format!(
                        "the {} adjustment has no 8-bit evaluation",
                        state.kernel.operation()
                    ));
                }
                walk.scratch = walk
                    .scratch
                    .saturating_add((description.len() as u64).saturating_mul(64));
                // A byte adjustment's MASK only covers its pixel extent
                // (`doc::composite_adjustment_into`), so a masked one gets the
                // canvas-sized transparent plane a byte host creates; an
                // unmasked one reads no extent and keeps 1×1.
                let dims = if layer.mask.is_some() {
                    (self.width, self.height)
                } else {
                    (1, 1)
                };
                walk.output = walk.output.saturating_add(area(dims) * 4);
                walk.rasters.push(dims);
                notes.push("adjustment evaluates the byte LUT instead of typed f32 math".into());
                if self.is_float() {
                    notes.push("adjustment applies after the per-record tone map".into());
                }
                let pixels = match job {
                    Some(_) if dims != (1, 1) => LayerRaster::filled(dims.0, dims.1, [0; 4]),
                    _ => unit_pixels(),
                };
                let meta = Some(description.to_string());
                (LayerKind::Raster, pixels, (0, 0), meta, layer.blend_mode)
            }
            Content::Group(_) | Content::PassThrough(_) => {
                walk.rasters.push((1, 1));
                let blend = if layer.is_pass_through() {
                    BlendMode::PassThrough
                } else {
                    layer.blend_mode
                };
                // A styled Pass Through group isolates too.
                if layer.isolates() {
                    notes
                        .push("isolated group composites with byte rounding instead of f32".into());
                }
                let offset = layer.mask.as_ref().map_or((0, 0), |m| m.properties.offset);
                (LayerKind::Group, unit_pixels(), offset, None, blend)
            }
        };
        let (mask, mask_properties, mask_enabled) = match &layer.mask {
            Some(mask) => {
                let dims = mask.plane.dimensions();
                walk.output = walk.output.saturating_add(area(dims));
                walk.masks.push(dims);
                let plane = match job {
                    Some(job) => nearest_plane(&mask.plane, job)?,
                    None => Arc::new(GrayImage::new(1, 1)),
                };
                (Some(plane), mask.properties.clone(), mask.enabled)
            }
            None => (None, Default::default(), true),
        };
        // The `reported_style` literal: both typed fields in ONE byte style.
        let style = (layer.style.is_some() || layer.blend_if.is_some()).then(|| {
            Arc::new(LayerStyle {
                fill_opacity: layer.style.as_ref().map_or(1.0, |s| s.fill_opacity),
                blend_if: layer.blend_if,
                knockout: layer
                    .style
                    .as_ref()
                    .map_or(crate::style::Knockout::None, |s| s.knockout),
                blend_interior_as_group: layer
                    .style
                    .as_ref()
                    .is_some_and(|s| s.blend_interior_as_group),
                effects: layer
                    .style
                    .as_ref()
                    .map(|s| s.effects.clone())
                    .unwrap_or_default(),
                cache: Default::default(),
            })
        });
        let mut out = Layer {
            id: layer.id,
            retained_source: None,
            warp: None,
            generated: None,
            pixels,
            offset,
            name: layer.name().to_string(),
            opacity: layer.opacity,
            blend,
            visible: layer.visible,
            mask,
            mask_enabled,
            mask_properties,
            meta,
            clipped: layer.clipped,
            style,
            kind,
            depth: u16::try_from(record.depth).unwrap_or(u16::MAX),
            locks: layer.locks,
            link: layer.link,
            open: layer.open,
            label: layer.label,
            reference: layer.reference,
        };
        self.style_note(layer, notes);
        self.carry_to_byte(layer, job, &mut out, notes, walk)?;
        Ok(out)
    }

    /// A retained source and a warp binding at 8 bits, with EVERY guard input
    /// tied to the byte layer they land on (`retained_source::effective_source`,
    /// `doc_warp`'s `effective_warp`): the rendered planes are that layer's
    /// own allocations and the profile is the assembled document's `Arc`.
    fn carry_to_byte(
        &self,
        layer: &PrecisionDocumentLayer,
        job: Option<JobContext<'_>>,
        out: &mut Layer,
        notes: &mut Vec<String>,
        walk: &mut Walk,
    ) -> Result<(), String> {
        const STALE: &str = "stale binding not carried; a save drops it too";
        // A live Generative Layer's variations rebind to the narrowed raster
        // (`doc_gen_variations`).
        if let Some(binding) = layer.effective_generated(&self.profile, false) {
            out.generated = Some(crate::doc_gen_variations::GeneratedBinding8 {
                table: binding.table,
                rendered: out.pixels.clone(),
                rendered_offset: out.offset,
                profile: self.profile.clone(),
            });
        }
        if layer.retained_source.is_some() {
            match layer.effective_source() {
                Some(source) => {
                    notes.push(
                        "source re-renders and quantizes its fallback at 8 bits on the next edit"
                            .into(),
                    );
                    out.retained_source = Some(RetainedSource8 {
                        asset: source.asset,
                        link: source.link,
                        transform: source.transform,
                        filter: source.filter,
                        rendered: out.pixels.clone(),
                        rendered_offset: out.offset,
                        rendered_profile: self.profile.clone(),
                    });
                }
                None => notes.push(STALE.into()),
            }
        }
        let Some(binding) = layer
            .warp
            .as_ref()
            .map(|_| layer.effective_warp(&self.profile))
        else {
            return Ok(());
        };
        let Some(binding) = binding else {
            notes.push(STALE.into());
            return Ok(());
        };
        let mut source_pixels = None;
        if let Some(pixels) = &binding.source_pixels {
            // The quantized original tiles beside its Vec too (the raster
            // leaf's tiling copy).
            walk.scratch = walk.scratch.saturating_add(area(pixels.dimensions()) * 4);
            walk.output = walk.output.saturating_add(area(pixels.dimensions()) * 4);
            source_pixels = Some(match job {
                Some(job) => {
                    LayerRaster::unseeded(pixels.quantize_with_context(PrecisionDither::None, job)?)
                }
                None => unit_pixels(),
            });
        }
        let mut source_mask = None;
        if let Some(mask) = &binding.source_mask {
            walk.output = walk.output.saturating_add(area(mask.dimensions()));
            source_mask = Some(match job {
                Some(job) => nearest_plane(mask, job)?,
                None => Arc::new(GrayImage::new(1, 1)),
            });
        }
        notes.push("warp's retained original quantized to 8 bits".into());
        out.warp = Some(WarpBinding8 {
            field: binding.field.clone(),
            target: binding.target,
            source_pixels,
            source_offset: binding.source_offset,
            source_mask,
            source_mask_offset: binding.source_mask_offset,
            rendered_pixels: binding.rendered_pixels.as_ref().map(|_| out.pixels.clone()),
            rendered_offset: out.offset,
            rendered_mask: binding.rendered_mask.as_ref().and(out.mask.clone()),
            rendered_mask_offset: out.mask_properties.offset,
            profile: self.profile.clone(),
            linked_mask: binding.linked_mask,
            mask_outside: binding.mask_outside,
        });
        Ok(())
    }

    /// One float record at 16 bits: the node itself — its id, mask, flags
    /// and every other field verbatim — with a tone-mapped raster or a
    /// re-created adjustment as its content.
    fn narrow_to_typed(
        &self,
        record: &FlatRecord,
        planes: Planes<'_>,
        notes: &mut Vec<String>,
        walk: &mut Walk,
    ) -> Result<PrecisionDocumentLayer, String> {
        let layer = &record.layer;
        if layer.warp.is_some() {
            return Err("a 32-bit float document holds no warp binding".into());
        }
        if float_raw_master(layer) {
            return Err(FLOAT_RAW_NARROW_REFUSAL.into());
        }
        let mut node = layer.clone();
        match &layer.content {
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                return Err(crate::cmyk_document::refused_sentence(
                    "Convert to 8 Bits/Channel",
                ))
            }
            Content::Raster { image, offset } => {
                let dims = image.dimensions();
                pixel_count(dims.0, dims.1)?;
                let wide = match self.leaf_u16(image, planes, notes, walk)? {
                    Some(wide) => wide,
                    None => PrecisionImage::from_rgba16_unseeded(
                        1,
                        1,
                        vec![[0; 4]],
                        self.profile.clone(),
                        None,
                    )?,
                };
                walk.output = walk.output.saturating_add(area(dims) * 8);
                walk.rasters.push(dims);
                self.described_note(layer, 16, notes);
                // A placed image's binding rides with the tone-mapped leaf as
                // its fallback; a stale one is dropped. The
                // plan's 1×1 SHAPE leaf cannot hold the binding's extent, so
                // only the real walk carries it (its result is preflighted).
                if layer.retained_source.is_some() {
                    let carried = layer.effective_source();
                    notes.push(match &carried {
                        Some(_) => "source re-renders at 16 bits on the next edit".into(),
                        None => "stale binding not carried; a save drops it too".into(),
                    });
                    node.retained_source = match planes {
                        Planes::Real(_) => carried.map(|mut source| {
                            source.rendered = wide.clone();
                            source.rendered_offset = *offset;
                            source
                        }),
                        Planes::Shape => None,
                    };
                }
                node.content = Content::Raster {
                    image: wide,
                    offset: *offset,
                };
            }
            Content::Adjustment(state) => {
                let description = state.description();
                let refused = |e: String| {
                    format!(
                        "the {} adjustment has no 16-bit evaluation: {e}",
                        state.kernel.operation()
                    )
                };
                if let Planes::Shape = planes {
                    // Admit this preparation BEFORE building it — the
                    // retained input, the records prepared so far, this
                    // one's allowance and the bookkeeping.
                    let bound =
                        PrecisionAdjustmentState::prepare_scratch_bytes(description, &self.profile)
                            .map_err(refused)?;
                    if let Err(reason) = walk.adjustments.next(bound, JobContext::NONE) {
                        walk.admission_refusal = Some(reason.clone());
                        return Err(reason);
                    }
                }
                let fresh = PrecisionDocumentLayer::adjustment(
                    layer.name(),
                    description,
                    layer.opacity,
                    layer.visible,
                    &self.profile,
                )
                .map_err(refused)?;
                if let Content::Adjustment(prepared) = &fresh.content {
                    walk.adjustments.plan_add(prepared, &self.profile)?;
                }
                node.content = fresh.content;
                notes.push("adjustment applies after the per-record tone map".into());
            }
            Content::Group(_) | Content::PassThrough(_) => {}
        }
        self.style_note(layer, notes);
        if let Some(mask) = &layer.mask {
            let dims = mask.plane.dimensions();
            walk.output = walk.output.saturating_add(area(dims) * 2);
            walk.masks.push(dims);
        }
        Ok(node)
    }

    /// Canvas-sized byte channels, nearest per sample.
    pub(crate) fn narrow_channels(&self, job: JobContext<'_>) -> Result<Vec<Channel>, String> {
        let mut out = Vec::with_capacity(self.channels.len());
        for channel in self.channels.iter() {
            let mut byte = Channel::new(channel.name(), nearest_plane(channel.plane(), job)?);
            byte.overlay_color = channel.metadata.overlay_color;
            byte.overlay_opacity = channel.metadata.overlay_opacity;
            byte.color_indicates_selected = channel.metadata.color_indicates_selected;
            byte.spot = channel.metadata.spot;
            out.push(byte);
        }
        Ok(out)
    }

    /// The byte document from the walked layers: validated, then the policy
    /// carried with its tables rebuilt at 8 bits (`doc_evaluation`).
    fn narrow_byte_document(
        &self,
        layers: Vec<Layer>,
        channels: Vec<Channel>,
    ) -> Result<DrDocument, String> {
        let document = DrDocument {
            width: self.width,
            height: self.height,
            layers,
            global_light: self.global_light,
            channels,
            guides: self.guides.clone(),
            ruler_origin: self.ruler_origin,
            color_samplers: self.color_samplers.clone(),
            paths: crate::doc_path::DocPaths {
                entries: self.paths.entries.clone(),
                // The typed records do not cross: `narrow_structure_to_u8`
                // re-sets the raster-less ones on the byte document.
                vector_masks: Arc::default(),
            },
            profile: self.profile.clone(),
            metadata: self.metadata.clone(),
            resolution: self.resolution,
            linear_light: None,
            comps: self.comps.clone(),
            // Layer ids survive the narrowing, so the mark does too.
            background: self.background,
        };
        let document = crate::doc_group::validated(document)
            .ok_or("the narrowed layer structure is malformed")?;
        Ok(match document.with_evaluation(self.evaluation())? {
            Some(switched) => switched,
            None => document,
        })
    }

    /// The u16 document from the walked float records: `build` (u16 mode),
    /// the document's own evaluation, then the comps and canvas chrome.
    fn narrow_typed_document(
        &self,
        flat: Vec<FlatRecord>,
        channels: Vec<PrecisionChannel>,
    ) -> Result<PrecisionDocument, String> {
        let mut result = PrecisionDocument::build(
            self.width,
            self.height,
            self.profile.clone(),
            self.metadata.clone(),
            self.resolution,
            rebuild(flat)?,
            channels,
            None,
        )?;
        if self.evaluation() == Evaluation::LinearLight {
            if let Some(switched) = result.with_evaluation(Evaluation::LinearLight)? {
                result = switched;
            }
        }
        result.comps.clone_from(&self.comps);
        result.background = self.background;
        result.guides.clone_from(&self.guides);
        result.ruler_origin = self.ruler_origin;
        result.color_samplers.clone_from(&self.color_samplers);
        // The vector-mask records cross with the list: a float record's mask
        // is carried as the same plane, so each record stays live.
        result.paths.clone_from(&self.paths);
        result.global_light = self.global_light;
        result.import_warnings = self.import_warnings.clone();
        Ok(result)
    }

    /// The plan: the depth gate FIRST, every record walked over its SHAPE,
    /// the target document assembled from that shape and the target's
    /// native capacity checked with the real extents. `Err` — not a row —
    /// for `Rgba16` on a document that already stores 16 bits.
    pub fn narrow_structure_plan(
        &self,
        target: NarrowTarget,
    ) -> Result<StructureConversionPlan, String> {
        self.narrow_structure_plan_within(target, process_working_limit(), 0)
    }

    /// The plan against a caller's `limit` and `other`: each float→16
    /// adjustment preparation is admitted before it is built, and a
    /// refusal is the whole plan's `Err`.
    fn narrow_structure_plan_within(
        &self,
        target: NarrowTarget,
        limit: u64,
        other: u64,
    ) -> Result<StructureConversionPlan, String> {
        let direction = self.direction(target)?;
        pixel_count(self.width, self.height)?;
        let flat = flatten(&self.layers);
        let mut blocked = Vec::new();
        if target == NarrowTarget::Rgba8 {
            for (index, record) in flat.iter().enumerate() {
                if record.depth > usize::from(MAX_GROUP_DEPTH) {
                    blocked.push(row(
                        index,
                        &record.layer,
                        format!(
                            "nested {} deep; an 8-bit document holds {MAX_GROUP_DEPTH} levels",
                            record.depth
                        ),
                    ));
                }
            }
        }
        let retained_bytes = self.retained_bytes();
        let walk = self.narrow_walk(
            &flat,
            target,
            PrecisionDither::None,
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
            index: flat.len(),
            id: 0,
            name: "document".into(),
            note,
        };
        let (w, h) = (self.width, self.height);
        let channels = self.channels.len();
        if blocked.is_empty() {
            let shaped = match target {
                NarrowTarget::Rgba8 => {
                    self.narrow_byte_document(walk.bytes, Vec::new())
                        .and_then(|shape| {
                            let fits = shape
                                .native_layers_fit_dimensions(walk.rasters.iter().copied())
                                && shape.native_masks_fit_dimensions(walk.masks.iter().copied())
                                && crate::doc_channel::channels_fit_at(w, h, channels);
                            if fits {
                                Ok(())
                            } else {
                                Err("the converted document exceeds the 8-bit native capacity"
                                    .into())
                            }
                        })
                }
                NarrowTarget::Rgba16 => self
                    .narrow_typed_document(walk.typed, Vec::new())
                    .and_then(|shape| {
                        shape.native_fits_geometry((w, h), &walk.rasters, &walk.masks, channels)
                    }),
            };
            if let Err(reason) = shaped {
                blocked.push(document_row(reason));
            }
        }
        let per_channel = match target {
            NarrowTarget::Rgba8 => 1,
            NarrowTarget::Rgba16 => 2,
        };
        let output = walk
            .output
            .saturating_add(area((w, h)).saturating_mul(per_channel * channels as u64));
        Ok(StructureConversionPlan {
            direction,
            tone_map: self.tone_map(),
            retained_bytes,
            scratch_bytes: walk
                .scratch
                .saturating_add(walk.adjustments.total()?)
                .saturating_add(4 * BOOKKEEPING_BYTES),
            output_bytes: output,
            discarded: if self.is_float() {
                vec!["extended_range"]
            } else {
                Vec::new()
            },
            rendering_differences: walk.differences,
            blocked,
        })
    }

    /// The plan's refusal, admission and reservation, shared by both ops.
    fn narrow_admit(
        &self,
        target: NarrowTarget,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        job.check()?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let plan = self.narrow_structure_plan_within(target, limit, other)?;
        if let Some(reason) = plan.refusal() {
            return Err(reason);
        }
        let scratch = plan.scratch_bytes.saturating_add(plan.output_bytes);
        admit(plan.retained_bytes, scratch, other, limit)?;
        job.reserve(scratch)
    }

    /// 16→8 or float→8 with every supported record carried. Refuses with the
    /// plan's first blocked row before any plane is built; the input is never
    /// mutated.
    pub fn narrow_structure_to_u8(
        &self,
        dither: PrecisionDither,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<DrDocument, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.narrow_admit(NarrowTarget::Rgba8, limit, other, job)?;
        let flat = flatten(&self.layers);
        let walk = self.narrow_walk(
            &flat,
            NarrowTarget::Rgba8,
            dither,
            Planes::Real(job.range(0.0, 0.9)),
            AdjustmentAdmission::default(),
        )?;
        let channels = self.narrow_channels(job.range(0.9, 0.95))?;
        // Raster-less vector masks are re-set on the byte document from
        // their shapes (`doc_vector_mask`); one with a raster part crosses as
        // the narrowed plane it baked.
        let result = self
            .narrow_byte_document(walk.bytes, channels)?
            .seeding_vector_masks(self.vector_mask_seeds(Clone::clone));
        result.native_save_preflight()?;
        // Every carried binding answers its effective query on the result,
        // so the next mutation's `detach_invalid_sources` drops nothing.
        debug_assert!(result.layers.iter().all(|layer| {
            (layer.retained_source.is_none() || layer.effective_source(&result.profile).is_some())
                && (layer.warp.is_none() || layer.effective_warp(&result.profile).is_some())
        }));
        job.report(1.0, 5)?;
        Ok(result)
    }

    /// float→16 with every supported record carried, each raster tone-mapped
    /// per record. Refuses a document that already stores 16 bits.
    pub fn narrow_structure_to_u16(
        &self,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionDocument, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.narrow_admit(NarrowTarget::Rgba16, limit, other, job)?;
        let flat = flatten(&self.layers);
        let walk = self.narrow_walk(
            &flat,
            NarrowTarget::Rgba16,
            PrecisionDither::None,
            Planes::Real(job.range(0.0, 0.95)),
            AdjustmentAdmission::default(),
        )?;
        let result = self.narrow_typed_document(walk.typed, self.channels.as_ref().clone())?;
        result.native_preflight()?;
        job.report(1.0, 5)?;
        Ok(result)
    }

    /// Per-channel |a − b| on straight u16 codes between this document's
    /// render (tone-mapped when float) and the narrowed document's — its
    /// 8-bit projection ×257, or its 16-bit render; RGB is skipped where
    /// BOTH alphas are 0 (latent colour, invisible). 12H-1's shape.
    pub fn narrow_structure_preview(
        &self,
        converted: Narrowed<'_>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let twin = match converted {
            Narrowed::Rgba8(document) => (document.width, document.height) == self.dimensions(),
            Narrowed::Rgba16(document) => {
                document.dimensions() == self.dimensions() && !document.is_float()
            }
        };
        if !twin {
            return Err("the converted document is not this document's narrowed twin".into());
        }
        let mut reference = self.render_with_context(limit, other, job.range(0.0, 0.4))?;
        if self.is_float() {
            reference = self.tone_mapped_u16(&reference, job.range(0.4, 0.5))?;
        }
        // The after side's storage outlives the paired walk's borrows.
        let widened: Vec<[u16; 4]>;
        let rendered: PrecisionImage;
        let before = reference
            .runs_rgba16()
            .ok_or("the reference projection is not 16-bit")?;
        let (mut max, mut sum, mut compared, mut skipped) = (0_u16, 0_u64, 0_u64, 0_u64);
        let mut i = 0_usize;
        let mut visit = |before: &[[u16; 4]], after: &[[u16; 4]]| -> Result<(), String> {
            for (b, a) in before.iter().zip(after) {
                job.every(i)?;
                i += 1;
                let channels = if b[3] == 0 && a[3] == 0 {
                    skipped += 3;
                    3..4
                } else {
                    0..4
                };
                for c in channels {
                    let delta = b[c].abs_diff(a[c]);
                    max = max.max(delta);
                    sum += u64::from(delta);
                    compared += 1;
                }
            }
            Ok(())
        };
        match converted {
            Narrowed::Rgba8(document) => {
                widened = document
                    .flattened()?
                    .pixels()
                    .map(|p| p.0.map(|v| u16::from(v) * 257))
                    .collect();
                paired_runs(before, iter::once(widened.as_slice()), &mut visit)?;
            }
            Narrowed::Rgba16(document) => {
                rendered = document.render_with_context(limit, other, job.range(0.5, 0.9))?;
                let after = rendered
                    .runs_rgba16()
                    .ok_or("the converted projection is not 16-bit")?;
                paired_runs(before, after, &mut visit)?;
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

/// Visits two flat u16 run streams as paired slices of equal length: two
/// cursors, each pair split at the shorter run (`dr_precision_image_compare`'s
/// pairing), ending when either stream ends (zip semantics).
fn paired_runs<'a>(
    mut left: impl Iterator<Item = &'a [[u16; 4]]>,
    mut right: impl Iterator<Item = &'a [[u16; 4]]>,
    mut visit: impl FnMut(&'a [[u16; 4]], &'a [[u16; 4]]) -> Result<(), String>,
) -> Result<(), String> {
    let (mut l_run, mut r_run): (&[[u16; 4]], &[[u16; 4]]) = (&[], &[]);
    loop {
        if l_run.is_empty() {
            let Some(run) = left.next() else { break };
            l_run = run;
        }
        if r_run.is_empty() {
            let Some(run) = right.next() else { break };
            r_run = run;
        }
        let n = l_run.len().min(r_run.len());
        let ((l_head, l_tail), (r_head, r_tail)) = (l_run.split_at(n), r_run.split_at(n));
        (l_run, r_run) = (l_tail, r_tail);
        visit(l_head, r_head)?;
    }
    Ok(())
}
