//! Positional insertion at HIGH DEPTH: the typed twin of
//! `layer_insert`'s byte `adding_layer_at`.
//!
//! Every rule is `layer_insert`'s — that module doc IS the specification, and
//! nothing about WHERE a record lands is decided here. This file only
//! expresses the rule over typed records, through `precision_structure_ops`'
//! flat postorder list (`flatten` → splice one `FlatRecord` → `rebuild`), the
//! same model the five structure ops and the three merges already share.
//!
//! [`PrecisionDocument::add_adjustment_at`] keeps
//! `PrecisionDocument::add_adjustment`'s admission and job sequence verbatim —
//! the same plan, the same `admit`, the same reserve/check/report order — and
//! differs from it in exactly one statement: the prepared record is SPLICED at
//! the resolved slot instead of pushed onto the root level. `add_adjustment`
//! itself is untouched, so every existing caller's bytes and refusals are
//! unchanged.
//!
//! `precision_insert` expresses the rule over `precision_structure_ops`' flat list (`flatten` →
//! splice one `FlatRecord` → `rebuild`) and `add_adjustment_at` keeps `add_adjustment`'s admission
//! and job sequence VERBATIM, differing in one statement; `add_adjustment`, `adding_layer` and
//! `adding_image_layer` are untouched, and no export was re-signed. `inserting_record_for_plan` is
//! `inserting_record`'s measurement-only twin: the same splice assembled WITHOUT `rebuilt`'s
//! validation, because a placement's plan hypothetical carries the master's unrendered pixels
//! beside a transform the edit has not applied yet — exactly the state the legacy PUSH hypothetical
//! is assembled around.

use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::{slot, InsertPosition, InsertSlot};
use crate::precision_adjustment::PrecisionAdjustmentState;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage};
use crate::precision_render::MAX_GROUP_DEPTH;
use crate::precision_structure_ops::{flatten, rebuild, FlatRecord};

impl PrecisionDocument {
    /// Where a record inserted at `position` relative to `anchor` would land,
    /// in the postorder record numbering `PrecisionDocument::layer` indexes.
    /// The refusals are `layer_insert::slot`'s, by name.
    pub fn insert_slot(
        &self,
        anchor: usize,
        position: InsertPosition,
    ) -> Result<InsertSlot, String> {
        let flat = flatten(&self.layers);
        let levels: Vec<usize> = flat.iter().map(|r| r.depth).collect();
        slot(
            &levels,
            &|i| flat[i].layer.children().is_some(),
            &|i| flat[i].layer.clipped(),
            anchor,
            position,
            MAX_GROUP_DEPTH,
        )
    }

    /// `layer_insert::slot_above_clip_run` over typed records: the slot
    /// directly above the clip run that contains or sits on `anchor`, in the
    /// postorder record numbering. Auto-Blend's merged record is its one
    /// caller.
    pub(crate) fn insert_slot_above_clip_run(&self, anchor: usize) -> Result<InsertSlot, String> {
        let flat = flatten(&self.layers);
        let levels: Vec<usize> = flat.iter().map(|r| r.depth).collect();
        crate::layer_insert::slot_above_clip_run(
            &levels,
            &|i| flat[i].layer.children().is_some(),
            &|i| flat[i].layer.clipped(),
            anchor,
            MAX_GROUP_DEPTH,
        )
    }

    /// Splices one prepared record into the flat list at `slot` and rebuilds
    /// the tree. The record takes the slot's `clipped` flag, so it JOINS a
    /// clip run rather than re-basing the members above it
    /// (`layer_insert`'s rule); every other field is the caller's.
    pub(crate) fn inserting_record(
        &self,
        record: PrecisionDocumentLayer,
        slot: InsertSlot,
    ) -> Result<Self, String> {
        let layers = self.spliced(record, slot)?;
        self.rebuilt(
            self.width,
            self.height,
            layers,
            self.channels.as_ref().clone(),
        )
    }

    /// [`Self::inserting_record`]'s PLAN twin: the SAME splice at the same
    /// slot, assembled without `rebuilt`'s validation.
    ///
    /// A placement's hypothetical is a MEASUREMENT, not a document: its
    /// placed record carries the master's own unrendered pixels beside a
    /// transform the edit has not applied yet, so `rebuilt`'s retained-source
    /// binding check would refuse a state the edit reaches only after
    /// `render16`. `precision_retained_source`'s legacy hypothetical assembles
    /// its PUSH the same way and for the same reason; this is that assembly
    /// with the record spliced instead of appended, so plan and edit agree
    /// about WHERE the record is while the plan stays as cheap as it was.
    /// Never a document a caller sees.
    pub(crate) fn inserting_record_for_plan(
        &self,
        record: PrecisionDocumentLayer,
        slot: InsertSlot,
    ) -> Result<Self, String> {
        let layers = self.spliced(record, slot)?;
        let mut next = self.clone();
        next.records = crate::precision_document::shape(&layers)?.0;
        next.layers = std::sync::Arc::new(layers);
        Ok(next)
    }

    /// The splice itself: one prepared record into the flat list at `slot`,
    /// taking the slot's level and `clipped` flag, then rebuilt into a tree.
    fn spliced(
        &self,
        record: PrecisionDocumentLayer,
        slot: InsertSlot,
    ) -> Result<Vec<PrecisionDocumentLayer>, String> {
        let mut flat = flatten(&self.layers);
        if slot.at > flat.len() {
            return Err("layer insert anchor is out of range".into());
        }
        flat.insert(
            slot.at,
            FlatRecord {
                layer: record.with_clipped(slot.clipped),
                depth: slot.level,
            },
        );
        rebuild(flat)
    }

    /// A transparent CANVAS-SIZED leaf at the slot `position` names relative
    /// to `anchor` — the typed twin of `DrDocument::adding_layer_at`, and the
    /// only typed path that creates an empty raster record.
    ///
    /// The slot is resolved BEFORE anything is allocated, so a refused
    /// position costs no plane. Storage follows the DOCUMENT
    /// (`precision_merge::merged_leaf`'s switch): u16 zeros at 16 bits, f32
    /// zeros at float, bare paper (`cmyk_image::PAPER`) in a CMYK document,
    /// which `PrecisionDocument::build`'s one storage gate (`build_cmyk` for
    /// CMYK) then accepts. The record's scratch is charged by its caller
    /// (`precision_structure::StructureEdit::scratch_bytes`), as every other
    /// structure op's is.
    pub fn add_layer_at(
        &self,
        name: &str,
        anchor: usize,
        position: InsertPosition,
    ) -> Result<Option<Self>, String> {
        let slot = self.insert_slot(anchor, position)?;
        let (width, height) = self.dimensions();
        self.preflight_raster_addition((width, height), name)?;
        crate::precision_image::pixel_count(width, height)?;
        let record = if self.is_cmyk() {
            let image =
                crate::cmyk_image::CmykImage::filled(width, height, crate::cmyk_image::PAPER)?;
            PrecisionDocumentLayer::raster_cmyk(name, image, (0, 0), 1.0, true)?
        } else if self.is_float() {
            let image = PrecisionImage::filled_rgba32_extended(
                width,
                height,
                [0.0; 4],
                self.profile.clone(),
            )?;
            PrecisionDocumentLayer::raster_float(name, image, (0, 0), 1.0, true)?
        } else {
            let image = PrecisionImage::filled_rgba16(width, height, [0; 4], self.profile.clone())?;
            PrecisionDocumentLayer::raster(name, image, (0, 0), 1.0, true)?
        };
        self.inserting_record(record, slot).map(Some)
    }

    /// `add_adjustment` at a chosen stack position. The admission and job
    /// sequence is that function's, statement for statement; only the final
    /// placement differs.
    #[allow(clippy::too_many_arguments)]
    pub fn add_adjustment_at(
        &self,
        name: &str,
        description: &str,
        opacity: f32,
        anchor: usize,
        position: InsertPosition,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        crate::precision_document::scalar_and_name(name, opacity)?;
        // The slot is resolved BEFORE anything is admitted or prepared, so a
        // refused position costs no operator table.
        let slot = self.insert_slot(anchor, position)?;
        let scratch = self.adjustment_scratch_bytes(None, description, false)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let state = PrecisionAdjustmentState::new(description, &self.profile)?;
        job.check()?;
        let record = PrecisionDocumentLayer::prepared_adjustment(name, state, opacity, true)?;
        let next = self.inserting_record(record, slot)?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
