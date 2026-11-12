//! An 8- or 16-bit document converted into a 32-bit FLOAT document
//! with every record carried — Image ▸ Mode ▸ 32 Bits/Channel…, MCP
//! `convert_precision mode:"rgba32"`, and File ▸ New at 32 bits (the host
//! builds the 16-bit blank and converts it here).
//!
//! # The rule
//!
//! - Each u16 RGB code `c` becomes `TypedTransfer::for_profile(P).decode(c /
//!   65535)` as f32, with P the document's own profile; alpha is `c / 65535`.
//!   This is the layers paste's own promotion under Preserve
//!   (`layer_clipboard_float::promoted_layers` over
//!   `precision_clipboard_promote::promoted_image`): no decode is written
//!   here.
//! - An 8-bit source first takes the exact 12H-1 widening, `v · 257`
//!   (`DrDocument::promote_structure_with_context`), then the same decode.
//! - The result is a float document in the same profile under the `clip`
//!   tone map, linear light by construction (`float_document`).
//! - Masks and channels stay u16 coverage. A 16-bit source's planes are
//!   shared with the result; an 8-bit source's are its ×257 planes.
//! - The way back is the existing narrowing (`precision_conversion_narrow`):
//!   its `clip` tone map and the profile's forward curve return every code a
//!   16-bit document held, and nearest returns every byte.
//!
//! # What blocks, by name, before any plane is built
//!
//! Per record in postorder, on the record AS IT IS (the paste's resets never
//! fire, because the refusal runs first): `float_document::refused_operator`
//! (a blend mode, an effect blend mode or an adjustment the float mode
//! cannot hold) and an EFFECTIVE warp binding ("a 32-bit float document
//! holds no warp binding"; a stale one is disclosed and dropped, as a save
//! drops it). Per document (id 0): a profile with no matrix/TRC model or one
//! that fails the precision check (`TypedTransfer::for_profile`,
//! `FloatPolicy::for_profile`), and — only when nothing else blocked — a
//! result too large to save and reopen at 32 bits
//! (`PrecisionDocument::native_fits_geometry_as_float`, the existing native
//! geometry plan charged as float, over the source's own extents or, from 8
//! bits, the 16-bit shape's). The op refuses with the plan's first row.
//! A float source is an `Err`, not a row.
//!
//! The rendering differences: the document composites in linear light (one
//! row when the source composited encoded values), each adjustment applies
//! its float form, a style or Blend If composites in linear light, a text or
//! shape re-renders at 32 bits on its next edit, a live retained source
//! re-renders into linear light, a stale binding is not carried. The 8→float
//! plan keeps the 8→16 rows except the three its float rows replace
//! (`BLEND_IF_16_NOTE`, `SOURCE_16_NOTE`, `WARP_16_NOTE`).
//!
//! # Admission
//!
//! 16→float: `retained` = the source's retained bytes; with `t` =
//! `layer_clipboard_float::promotion_terms` under Preserve, `output` =
//! `t.held` (16 B/px of every converting plane) and `scratch` =
//! `t.conversion − t.held + LAYER_PROMOTE_MARGIN_PER_PIXEL · t.pixels + 4 ·
//! BOOKKEEPING_BYTES`. 8→float: `retained` is the 8→16 plan's; `t` =
//! `promotion_bytes(Σ raster pixels, true, 0)`; `scratch` = the 8→16 plan's
//! scratch and output + `t.conversion − t.held` + the margin; `output` =
//! `t.held`. The op admits and reserves the whole plan before the first
//! step; the 16→float step of an 8-bit source runs with the byte document's
//! retained bytes counted as held elsewhere.
//!
//! The measurement record is below, beside the floors
//! `tests/float_promote_24mp.rs` pins.
//!
//! # Measurement record (2026-10-12)
//!
//! `tests/float_promote_24mp.rs`'s ignored `measure`: Release, one case per
//! process, `MallocLargeCache=0`, process limit 8,589,934,592, three runs and
//! a baseline twin; E = `ri_interval_max_phys_footprint` − the footprint read
//! after `proc_reset_footprint_interval`, around ONE conversion with the
//! result held. Sources are opaque sRGB ramps built in the measured process:
//! 6000×4000 (24 MP) and 9568×6376 (61 MP), at 16 bits (`p16`) and 8 bits
//! (`p8`). A = the plan's scratch + output.
//!
//! With `LAYER_PROMOTE_MARGIN_PER_PIXEL` at 0 every case missed: A / max E was
//! 1.0095 (p16_24), 1.0022 (p16_61), 1.0109 (p8_24) and 1.0025 (p8_61) — the
//! peak is the new f32 plane and its adoption copy, 32.0 B/px from 16 bits and
//! 39.9 from 8 (the ×257 planes on top). The smallest integer covering ⌈1.25 ×
//! max E⌉ is 8 for 16→float and 10 for 8→float (9.91 B/px at 61 MP), so the
//! shared constant is 10 (`layer_clipboard_float`). Re-measured with 10:
//!
//! | case | A | E (three runs) | ⌈1.25 × max E⌉ | A / max E |
//! | --- | --- | --- | --- | --- |
//! | base | — | 0 ×3 | — | — |
//! | p16_24 | 1,015,655,460 | 768,377,384 ×2 / 768,393,768 | 960,492,210 | 1.3218 |
//! | p16_61 | 2,569,889,316 | 1,955,513,736 / 1,955,464,584 / 1,955,497,352 | 2,444,392,170 | 1.3142 |
//! | p8_24 | 1,207,655,460 | 957,252,280 / 957,268,664 ×2 | 1,196,585,830 | 1.2616 |
//! | p8_61 | 3,057,933,860 | 2,441,742,016 ×3 | 3,052,177,520 | 1.2524 |
//!
//! Command: `DR_FP_CASE=<case> MallocLargeCache=0 cargo test --manifest-path
//! core/Cargo.toml --release --test float_promote_24mp measure -- --ignored
//! --exact --nocapture`, three times per case.

use crate::doc::DrDocument;
use crate::doc_evaluation::Evaluation;
use crate::float_document::{refused_operator, FloatPolicy, ToneMap};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_clipboard_float::{
    promoted_layers, promotion_bytes, promotion_terms, LAYER_PROMOTE_MARGIN_PER_PIXEL,
};
use crate::precision_clipboard::PasteProfile;
use crate::precision_conversion_structure::{
    ConversionDirection, StructureConversionPlan, StructureRow, BLEND_IF_16_NOTE, SOURCE_16_NOTE,
    STALE_NOTE, WARP_16_NOTE,
};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_evaluation::TypedTransfer;
use crate::precision_image::{admit, pixel_count, process_working_limit, BOOKKEEPING_BYTES};
use crate::precision_structure_ops::flatten;
use std::sync::Arc;

/// The `Err` of a conversion asked of a float document.
const ALREADY_FLOAT: &str = "the document already stores 32-bit float pixels";

/// The blocked row of a record whose warp binding is effective.
const WARP_REFUSAL: &str = "a 32-bit float document holds no warp binding";

const LINEAR_NOTE: &str = "composites in linear light; this document composited encoded values, so blended or partly opaque layers render differently";
const ADJUSTMENT_NOTE: &str = "applies its 32-bit float form in linear light";
const STYLE_NOTE: &str = "style and Blend If composite in linear light";
const DESCRIBED_NOTE: &str =
    "text or shape keeps its description; it re-renders at 32 bits on the next edit";
const SOURCE_NOTE: &str = "source re-renders into linear light on the next edit";

fn row(index: usize, layer: &PrecisionDocumentLayer, note: impl Into<String>) -> StructureRow {
    StructureRow {
        index,
        id: layer.id,
        name: layer.name().to_string(),
        note: note.into(),
    }
}

fn document_row(index: usize, note: impl Into<String>) -> StructureRow {
    StructureRow {
        index,
        id: 0,
        name: "document".into(),
        note: note.into(),
    }
}

/// The rows a plan into float discloses.
#[derive(Default)]
struct Rows {
    blocked: Vec<StructureRow>,
    differences: Vec<StructureRow>,
}

impl Rows {
    /// The per-record rows over `doc`'s records in postorder (module doc).
    /// A blocked record discloses no rendering difference.
    fn records(&mut self, doc: &PrecisionDocument) {
        for (index, record) in flatten(doc.root_layers()).iter().enumerate() {
            let layer = &record.layer;
            let mut blocked = Vec::new();
            let mut notes = Vec::new();
            if let Some(reason) = refused_operator(layer) {
                blocked.push(reason);
            }
            if layer.warp.is_some() {
                if layer.effective_warp(doc.profile()).is_some() {
                    blocked.push(WARP_REFUSAL.to_string());
                } else {
                    notes.push(STALE_NOTE);
                }
            }
            if !blocked.is_empty() {
                self.blocked
                    .extend(blocked.into_iter().map(|note| row(index, layer, note)));
                continue;
            }
            if matches!(layer.content, Content::Adjustment(_)) {
                notes.push(ADJUSTMENT_NOTE);
            }
            if layer.style.is_some() || layer.blend_if.is_some() {
                notes.push(STYLE_NOTE);
            }
            if layer.description.is_some() {
                notes.push(DESCRIBED_NOTE);
            }
            if layer.retained_source.is_some() {
                notes.push(if layer.effective_source().is_some() {
                    SOURCE_NOTE
                } else {
                    STALE_NOTE
                });
            }
            self.differences
                .extend(notes.into_iter().map(|note| row(index, layer, note)));
        }
    }

    /// The document rows (id 0, index `records`): the profile the float mode
    /// needs, and the linear-light row of an encoded source.
    fn document(&mut self, profile: &Arc<IccProfile>, evaluation: Evaluation, records: usize) {
        if let Err(reason) = TypedTransfer::for_profile(profile) {
            self.blocked.push(document_row(records, reason));
        } else if let Err(reason) = FloatPolicy::for_profile(profile, ToneMap::Clip) {
            self.blocked.push(document_row(records, reason));
        }
        if evaluation != Evaluation::LinearLight {
            self.differences.push(document_row(records, LINEAR_NOTE));
        }
    }

    /// The capacity row, only when nothing else blocked.
    fn capacity(&mut self, records: usize, fits: impl FnOnce() -> Result<(), String>) {
        if self.blocked.is_empty() {
            if let Err(reason) = fits() {
                self.blocked.push(document_row(records, reason));
            }
        }
    }
}

/// `t.conversion − t.held + margin · t.pixels`: the promotion's scratch
/// beside the planes it keeps (module doc).
fn promotion_scratch(conversion: u64, held: u64, pixels: u64) -> u64 {
    conversion
        .saturating_sub(held)
        .saturating_add(LAYER_PROMOTE_MARGIN_PER_PIXEL.saturating_mul(pixels))
}

impl PrecisionDocument {
    /// The 16→float plan (module doc). `Err` for a float document.
    pub fn promote_to_float_plan(&self) -> Result<StructureConversionPlan, String> {
        if self.is_float() {
            return Err(ALREADY_FLOAT.into());
        }
        pixel_count(self.width, self.height)?;
        let records = self.layer_count();
        let mut rows = Rows::default();
        rows.records(self);
        rows.document(&self.profile, self.evaluation(), records);
        rows.capacity(records, || {
            let (r, m) = self.native_dimensions();
            self.native_fits_geometry_as_float(self.dimensions(), &r, &m, self.channels.len())
        });
        let terms = promotion_terms(self, &self.profile, PasteProfile::Preserve)?;
        Ok(StructureConversionPlan {
            direction: ConversionDirection::Rgba16ToFloat,
            tone_map: None,
            retained_bytes: self.retained_bytes(),
            scratch_bytes: promotion_scratch(terms.conversion, terms.held, terms.pixels)
                .saturating_add(4 * BOOKKEEPING_BYTES),
            output_bytes: terms.held,
            discarded: Vec::new(),
            rendering_differences: rows.differences,
            blocked: rows.blocked,
        })
    }

    /// 16→float with every supported record carried. Refuses with the
    /// plan's first blocked row before any plane is built; the input is
    /// never mutated.
    pub fn promote_to_float(
        &self,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionDocument, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.promote_to_float_with_context(limit, other, job)
    }

    /// [`Self::promote_to_float`] under a caller's job: the 8→float
    /// conversion's second step.
    pub(crate) fn promote_to_float_with_context(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionDocument, String> {
        job.check()?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let plan = self.promote_to_float_plan()?;
        if let Some(reason) = plan.refusal() {
            return Err(reason);
        }
        let scratch = plan.scratch_bytes.saturating_add(plan.output_bytes);
        admit(plan.retained_bytes, scratch, other, limit)?;
        job.reserve(scratch)?;
        let (layers, _) = promoted_layers(
            self,
            &self.profile,
            PasteProfile::Preserve,
            job.range(0.0, 0.95),
        )?;
        let mut result = PrecisionDocument::from_parts_float(
            self.width,
            self.height,
            self.profile.clone(),
            self.metadata.clone(),
            self.resolution,
            layers,
            self.channels.as_ref().clone(),
            ToneMap::Clip,
        )?;
        result.comps.clone_from(&self.comps);
        result.background = self.background;
        result.guides.clone_from(&self.guides);
        result.ruler_origin = self.ruler_origin;
        result.color_samplers.clone_from(&self.color_samplers);
        // The vector-mask records cross with the list: each record keeps its
        // id and its mask plane, so each stays live (`narrow_typed_document`).
        result.paths.clone_from(&self.paths);
        result.global_light = self.global_light;
        result.import_warnings = self.import_warnings.clone();
        // The chrome was attached after `build`'s own preflight.
        result.native_preflight()?;
        job.report(1.0, 5)?;
        Ok(result)
    }
}

impl DrDocument {
    /// The 8→float plan (module doc).
    pub fn promote_to_float_plan(&self) -> Result<StructureConversionPlan, String> {
        self.promote_to_float_plan_within(process_working_limit(), 0)
    }

    fn promote_to_float_plan_within(
        &self,
        limit: u64,
        other: u64,
    ) -> Result<StructureConversionPlan, String> {
        let shape = self.promote_structure_shape(limit, other)?;
        let sixteen = shape.plan;
        let records = self.layers.len();
        let mut rows = Rows {
            blocked: sixteen.blocked,
            differences: sixteen
                .rendering_differences
                .into_iter()
                .filter(|row| {
                    ![BLEND_IF_16_NOTE, SOURCE_16_NOTE, WARP_16_NOTE].contains(&row.note.as_str())
                })
                .collect(),
        };
        if let Some(document) = &shape.document {
            rows.records(document);
        }
        rows.document(&self.profile, self.evaluation(), records);
        if let Some(document) = &shape.document {
            rows.capacity(records, || {
                document.native_fits_geometry_as_float(
                    (self.width, self.height),
                    &shape.rasters,
                    &shape.masks,
                    shape.channels,
                )
            });
        }
        let pixels = shape
            .rasters
            .iter()
            .map(|(w, h)| u64::from(*w) * u64::from(*h))
            .fold(0_u64, u64::saturating_add);
        let terms = promotion_bytes(pixels, true, 0)?;
        Ok(StructureConversionPlan {
            direction: ConversionDirection::Rgba8ToFloat,
            tone_map: None,
            retained_bytes: sixteen.retained_bytes,
            scratch_bytes: sixteen
                .scratch_bytes
                .saturating_add(sixteen.output_bytes)
                .saturating_add(promotion_scratch(
                    terms.conversion,
                    terms.held,
                    terms.pixels,
                )),
            output_bytes: terms.held,
            discarded: Vec::new(),
            rendering_differences: rows.differences,
            blocked: rows.blocked,
        })
    }

    /// 8→float with every supported record carried: the 8→16 conversion,
    /// then the 16→float one. Refuses with the plan's first blocked row
    /// before any plane is built; the input is never mutated.
    pub fn promote_to_float(
        &self,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionDocument, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let plan = self.promote_to_float_plan_within(limit, other)?;
        if let Some(reason) = plan.refusal() {
            return Err(reason);
        }
        let whole = plan.scratch_bytes.saturating_add(plan.output_bytes);
        admit(plan.retained_bytes, whole, other, limit)?;
        job.reserve(whole)?;
        // Both steps keep the whole plan reserved.
        let job = job.with_reservation_floor(whole);
        let sixteen = self.promote_structure_with_context(limit, other, job.range(0.0, 0.4))?;
        sixteen.promote_to_float_with_context(
            limit,
            other.saturating_add(plan.retained_bytes),
            job.range(0.4, 1.0),
        )
    }
}
