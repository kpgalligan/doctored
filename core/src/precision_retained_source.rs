//! Typed fallback editing over genuine byte/u16 immutable source assets. Source
//! replacement retains its affine and layer properties; links never trigger IO.
use crate::doc_lock::EditKind;
use crate::doc_transform::{exact_form, transformed_plane_bounds, Affine, Exact, SourceMap};
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::InsertSlot;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_mask::PrecisionMask;
use crate::precision_sampling::{self, GeometryFilter};
use crate::resources::Inventory;
use crate::retained_source::{
    self, compose, source_bounds, DepthConversion, RetainedSource16, SourceAsset, SourceEdit,
    SourceInput, SourceLink, SourcePlan,
};
use crate::retained_source_float::{float_render_scratch, render_typed};
use crate::retained_source_render::{profile_cost, work_plan_within, SourceSampling};
use crate::source_transform::{self, SourceTransform};
use serde_json::{json, Value};
use std::sync::Arc;

/// The one resolved form of a source edit, read by BOTH the plan and the
/// edit — the typed twin of `doc_retained_source::Prepared`, whose doc comment
/// states the `slot` rule: `Some` ⇒ `index == slot.at` and the record is
/// SPLICED there, `None` ⇒ `index == layer_count()` and the record is PUSHED.
struct Prepared<'a> {
    source: RetainedSource16,
    index: usize,
    name: Option<&'a str>,
    incremental: Option<[f64; 6]>,
    slot: Option<InsertSlot>,
}
fn mask_bounds(
    layer: &PrecisionDocumentLayer,
    matrix: [f64; 6],
) -> Result<Option<(i32, i32, u32, u32)>, String> {
    layer
        .mask
        .as_ref()
        .filter(|m| m.properties.linked)
        .map(|mask| {
            let bounds = transformed_plane_bounds(
                mask.plane.dimensions(),
                mask.properties.offset,
                &retained_source::validate_matrix(matrix)?,
            )
            .ok_or("retained source linked mask exceeds supported bounds")?;
            if !mask.properties.valid((bounds.2, bounds.3)) {
                return Err(
                    "retained source linked mask properties exceed supported bounds".into(),
                );
            }
            Ok(bounds)
        })
        .transpose()
}
fn mask_dimensions(
    layers: &[PrecisionDocumentLayer],
    target: *const PrecisionDocumentLayer,
    replacement: Option<(u32, u32)>,
    out: &mut Vec<(u32, u32)>,
) {
    for layer in layers {
        if let Some(mask) = &layer.mask {
            out.push(if std::ptr::eq(layer, target) {
                replacement.unwrap_or_else(|| mask.plane.dimensions())
            } else {
                mask.plane.dimensions()
            });
        }
        if let Some(children) = layer.children() {
            mask_dimensions(children, target, replacement, out);
        }
    }
}
impl PrecisionDocument {
    /// The depth a placed image's fallback renders at: 32 in a float
    /// document (`retained_source_float`), 16 otherwise.
    pub(crate) fn source_fallback_depth(&self) -> u32 {
        if self.is_float() {
            32
        } else {
            16
        }
    }
    pub fn has_retained_source(&self, index: usize) -> bool {
        self.layer(index)
            .is_some_and(|l| l.effective_source().is_some())
    }
    pub fn source_info_scratch_bytes(&self, index: usize) -> u64 {
        self.layer(index)
            .and_then(|l| l.effective_source())
            .map_or(64 * 1024, |s| {
                retained_source::info_scratch(&s.asset, s.link.as_deref())
            })
    }
    pub fn source_info(&self, index: usize) -> Result<Value, String> {
        let layer = self
            .layer(index)
            .ok_or("source layer index is out of range")?;
        Ok(layer.effective_source().map_or_else(
            || json!({"valid":false}),
            |s| {
                retained_source::info(
                    &s.asset,
                    s.link.as_deref(),
                    s.transform,
                    s.filter,
                    self.source_fallback_depth(),
                )
            },
        ))
    }
    fn prepare_source<'a>(
        &self,
        input: Option<SourceInput<'_>>,
        edit: &'a SourceEdit,
    ) -> Result<Prepared<'a>, String> {
        let required = |index| {
            self.layer(index)
                .and_then(|l| l.effective_source())
                .ok_or("layer has no retained ordinary source")
        };
        match edit {
            SourceEdit::CaptureRaster {
                layer,
                source_id,
                revision_id,
                source_transform,
                depth_conversion,
                link,
            } => {
                let old = self.layer(*layer).ok_or("capture target is out of range")?;
                let rendered = old.source().ok_or("capture requires an ordinary raster")?;
                if old.retained_source.is_some() {
                    return Err("capture target already has a retained binding".into());
                }
                if (0..self.layer_count())
                    .filter_map(|i| self.layer(i)?.effective_source())
                    .any(|s| s.asset.id == *source_id)
                {
                    return Err("source identity already exists in this document".into());
                }
                let asset = input
                    .ok_or("source capture requires decoded input")?
                    .asset(source_id, revision_id.as_deref().unwrap_or(source_id), None)?;
                depth_conversion.require(&asset, self.source_fallback_depth())?;
                retained_source::validate_matrix(*source_transform)?;
                if let Some(link) = link {
                    link.validate()?;
                }
                // The LOCK gate, the byte twin's rule verbatim: a capture
                // upgrades the record's own pixels (`Merge`), and it is a
                // POSITION edit as well when the rendered extent or offset
                // would move.
                self.lock_refusal(*layer, EditKind::Merge)?;
                let dest = source_bounds(asset.dimensions(), *source_transform)?;
                if (dest.0, dest.1) != old.offset() || (dest.2, dest.3) != rendered.dimensions() {
                    self.lock_refusal(*layer, EditKind::Position)?;
                }
                Ok(Prepared {
                    source: RetainedSource16 {
                        asset,
                        link: link.clone().map(Arc::new),
                        transform: SourceTransform::Affine(*source_transform),
                        filter: GeometryFilter::Lanczos3,
                        rendered: rendered.clone(),
                        rendered_offset: old.offset(),
                    },
                    index: *layer,
                    name: None,
                    incremental: None,
                    slot: None,
                })
            }
            SourceEdit::Retain {
                source_id,
                revision_id,
            } => {
                if input.is_some() {
                    return Err("retain source accepts no input document".into());
                }
                let asset = SourceInput::Typed(self).asset(
                    source_id,
                    revision_id.as_deref().unwrap_or(source_id),
                    None,
                )?;
                // A float document's one raster is an f32 master; retained
                // in place it is its own fallback, as a u16 master is (the
                // 32-bit float RAW open).
                let rendered = asset
                    .rgba16()
                    .or(asset.rgba32())
                    .ok_or("retain requires genuine u16 or f32 pixels")?
                    .clone();
                let offset = self.layer(0).ok_or("retain requires one raster")?.offset();
                Ok(Prepared {
                    source: RetainedSource16 {
                        asset,
                        link: None,
                        transform: SourceTransform::Affine([
                            1.,
                            0.,
                            0.,
                            1.,
                            f64::from(offset.0),
                            f64::from(offset.1),
                        ]),
                        filter: GeometryFilter::Lanczos3,
                        rendered,
                        rendered_offset: offset,
                    },
                    index: 0,
                    name: None,
                    incremental: None,
                    slot: None,
                })
            }
            SourceEdit::Place {
                source_id,
                revision_id,
                name,
                x,
                y,
                link,
                depth_conversion,
                source_transform,
                anchor,
                position,
                profile_refusal,
            } => {
                if (0..self.layer_count())
                    .filter_map(|i| self.layer(i)?.effective_source())
                    .any(|s| s.asset.id == *source_id)
                {
                    return Err("source identity already exists in this document".into());
                }
                // The ONE slot rule (`layer_insert`), resolved before anything
                // is decoded. Absent is the legacy push.
                let slot = match (anchor, position) {
                    (Some(anchor), Some(position)) => Some(self.insert_slot(*anchor, *position)?),
                    (None, None) => None,
                    _ => {
                        return Err(
                            "placement position requires both an anchor layer and a position"
                                .into(),
                        )
                    }
                };
                crate::precision_document::scalar_and_name(name, 1.)?;
                if let Some(link) = link {
                    link.validate()?;
                }
                let asset = input
                    .ok_or("place source requires a source document")?
                    .asset(
                        source_id,
                        revision_id.as_deref().unwrap_or(source_id),
                        profile_refusal.as_deref(),
                    )?;
                depth_conversion.require(&asset, self.source_fallback_depth())?;
                let rendered = if self.is_float() {
                    PrecisionImage::from_rgba32_extended(
                        1,
                        1,
                        vec![[0.; 4]],
                        self.profile.clone(),
                        None,
                    )?
                } else if let Some(image) = asset.rgba16() {
                    image.with_profile(self.profile.clone())
                } else {
                    PrecisionImage::from_rgba16_unseeded(
                        1,
                        1,
                        vec![[0; 4]],
                        self.profile.clone(),
                        None,
                    )?
                };
                Ok(Prepared {
                    source: RetainedSource16 {
                        asset,
                        link: link.clone().map(Arc::new),
                        transform: SourceTransform::Affine(compose(
                            [1., 0., 0., 1., f64::from(*x), f64::from(*y)],
                            source_transform.unwrap_or([1., 0., 0., 1., 0., 0.]),
                        )),
                        filter: GeometryFilter::Lanczos3,
                        rendered,
                        rendered_offset: (*x, *y),
                    },
                    index: slot.map_or(self.layer_count(), |s| s.at),
                    name: Some(name),
                    incremental: None,
                    slot,
                })
            }
            SourceEdit::Replace {
                layer,
                revision_id,
                link,
                depth_conversion,
                source_transform,
                source_coordinate_map,
                profile_refusal,
            } => {
                // The LOCK gate, the byte twin's rule verbatim.
                self.lock_refusal(*layer, EditKind::Merge)?;
                let source = required(*layer)?;
                if (0..self.layer_count())
                    .filter_map(|i| self.layer(i)?.effective_source())
                    .any(|s| s.asset.id == source.asset.id && s.asset.revision_id == *revision_id)
                {
                    return Err("replacement requires a new source revision identity".into());
                }
                if let Some(link) = link {
                    link.validate()?;
                }
                let asset = input
                    .ok_or("replace source requires a source document")?
                    .asset(&source.asset.id, revision_id, profile_refusal.as_deref())?;
                Ok(Prepared {
                    source: self.rebound_source(
                        *layer,
                        asset,
                        link.as_ref(),
                        *depth_conversion,
                        *source_transform,
                        *source_coordinate_map,
                    )?,
                    index: *layer,
                    name: None,
                    incremental: None,
                    slot: None,
                })
            }
            SourceEdit::Transform {
                layer,
                matrix,
                filter,
                ..
            } => {
                // The LOCK gate: "a transform is a POSITION edit and never a
                // Pixels one" (`doc_lock`). `precision_geometry` already wraps
                // the per-layer affine; this consult covers the direct source
                // transform request, which writes the fallback itself.
                self.lock_refusal(*layer, EditKind::Position)?;
                let mut source = required(*layer)?;
                source.transform =
                    source_transform::compose(SourceTransform::Affine(*matrix), source.transform)?;
                source.filter = *filter;
                Ok(Prepared {
                    source,
                    index: *layer,
                    name: None,
                    incremental: Some(*matrix),
                    slot: None,
                })
            }
            _ => Err("source edit does not require pixel rendering".into()),
        }
    }
    /// Record `layer`'s binding rebound to `asset`: the typed twin of
    /// `DrDocument::rebound_source` (Replace's rule), refusing through the
    /// typed lock sentences.
    pub(crate) fn rebound_source(
        &self,
        layer: usize,
        asset: Arc<SourceAsset>,
        link: Option<&SourceLink>,
        depth_conversion: DepthConversion,
        source_transform: Option<[f64; 6]>,
        coordinate_map: Option<[f64; 6]>,
    ) -> Result<RetainedSource16, String> {
        self.lock_refusal(layer, EditKind::Merge)?;
        let mut source = self
            .layer(layer)
            .and_then(|l| l.effective_source())
            .ok_or("layer has no retained ordinary source")?;
        source.asset = asset;
        depth_conversion.require(&source.asset, self.source_fallback_depth())?;
        if let Some(matrix) = source_transform {
            retained_source::validate_matrix(matrix)?;
            source.transform = SourceTransform::Affine(matrix);
        } else if let Some(matrix) = coordinate_map {
            source.transform =
                source_transform::compose(source.transform, SourceTransform::Affine(matrix))?;
        }
        source.link = link.cloned().map(Arc::new);
        // A replacement that MOVES the record's extent or offset is a
        // POSITION edit as well, as at byte depth.
        let dest = source_transform::bounds(source.asset.dimensions(), source.transform)?;
        let old = self.layer(layer).ok_or("invalid source index")?;
        if (dest.0, dest.1) != old.offset()
            || old.source().map(PrecisionImage::dimensions) != Some((dest.2, dest.3))
        {
            self.lock_refusal(layer, EditKind::Position)?;
        }
        Ok(source)
    }
    /// Replace's plan for record `layer` rebound to `source`, with
    /// `retained` the caller's inventory: the typed twin of
    /// `DrDocument::replacement_plan`.
    pub(crate) fn replacement_plan(
        &self,
        layer: usize,
        source: &RetainedSource16,
        retained: u64,
    ) -> Result<SourcePlan, String> {
        let dest = source_transform::bounds(source.asset.dimensions(), source.transform)?;
        let sampling = SourceSampling::new(
            source.asset.dimensions(),
            (dest.2, dest.3),
            source.transform,
            source.filter,
        );
        let icc = profile_cost(source.asset.profile(), &self.profile, true)?;
        let mut record = self.layer(layer).ok_or("invalid source index")?.clone();
        record.retained_source = Some(source.clone());
        let hypothetical = self.replacing_layer_for_plan(layer, record)?;
        if hypothetical
            .layer(layer)
            .and_then(|l| l.retained_source.as_ref())
            .is_none()
        {
            return Err("invalid source index".into());
        }
        let mut rasters = Vec::new();
        for i in 0..hypothetical.layer_count() {
            if let Some(image) = hypothetical.layer_source(i) {
                rasters.push(if i == layer {
                    (dest.2, dest.3)
                } else {
                    image.dimensions()
                });
            }
        }
        let mut masks = Vec::new();
        mask_dimensions(
            &hypothetical.layers,
            hypothetical.layer(layer).ok_or("invalid source index")?,
            None,
            &mut masks,
        );
        hypothetical.native_fits_geometry(
            self.dimensions(),
            &rasters,
            &masks,
            self.channels.len(),
        )?;
        let mut plan = work_plan_within(
            crate::sampling_gaussian::CONTROLLED_JOB_WORK,
            retained,
            crate::retained_source_render::SourceStage {
                asset: &source.asset,
            },
            (dest.2, dest.3),
            0,
            sampling,
            self.source_fallback_depth(),
            icc,
        )?;
        if self.is_float() {
            plan.scratch_bytes = plan
                .scratch_bytes
                .checked_add(float_render_scratch(
                    source.asset.dimensions(),
                    (dest.2, dest.3),
                )?)
                .ok_or("source render size overflow")?;
        }
        plan.scratch_bytes =
            crate::precision_geometry::source_transform_scratch(false, plan.scratch_bytes)?;
        Ok(plan)
    }
    fn source_metadata_edit(&self, edit: &SourceEdit) -> Result<Option<Self>, String> {
        let index = match edit {
            SourceEdit::Rasterize { layer } => *layer,
            _ => return Err("expected source metadata edit".into()),
        };
        let mut layer = self
            .layer(index)
            .ok_or("source layer index is out of range")?
            .clone();
        if layer.effective_source().is_none() {
            return Ok(None);
        }
        // The LOCK gate, AFTER the no-source no-op so a locked layer with
        // nothing to rasterize still answers `None` rather than a refusal —
        // the byte twin's order. Dropping the binding freezes the fallback as
        // the record's own pixels, so it is a `Pixels` edit, and a CONSULT
        // because `under_locks`' latch compares pixels and mask, neither of
        // which this changes.
        self.lock_refusal(index, EditKind::Pixels)?;
        layer.retained_source = None;
        self.replacing_layer(index, layer)
    }
    pub fn source_plan(
        &self,
        input: Option<&Self>,
        edit: &SourceEdit,
    ) -> Result<SourcePlan, String> {
        // The job-less plan carries no work refusal.
        self.source_plan_input(
            input.map(SourceInput::Typed),
            edit,
            crate::sampling_gaussian::CONTROLLED_JOB_WORK,
        )
    }
    /// The plan; every edit but an `interactive` `Transform` (a preview,
    /// `MAX_WORK_UNITS`) is bounded by `transform_cap`
    /// (`CONTROLLED_JOB_WORK` for a plan, `destructive_work_cap(job)` for a
    /// run).
    pub fn source_plan_input(
        &self,
        input: Option<SourceInput<'_>>,
        edit: &SourceEdit,
        transform_cap: u64,
    ) -> Result<SourcePlan, String> {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        if matches!(edit, SourceEdit::Rasterize { .. }) {
            self.source_metadata_edit(edit)?;
            return Ok(SourcePlan {
                retained_bytes: inventory.unique_bytes(),
                scratch_bytes: 4 * BOOKKEEPING_BYTES,
                work_units: 0,
            });
        }
        if let Some(input) = input {
            input.record_resources(&mut inventory);
        }
        let prepared = self.prepare_source(input, edit)?;
        if matches!(edit, SourceEdit::Replace { .. }) {
            return self.replacement_plan(
                prepared.index,
                &prepared.source,
                inventory.unique_bytes(),
            );
        }
        let source = &prepared.source;
        let dest = source_transform::bounds(source.asset.dimensions(), source.transform)?;
        let sampling = SourceSampling::new(
            source.asset.dimensions(),
            (dest.2, dest.3),
            source.transform,
            source.filter,
        );
        // A Place on a Lab Color document (#1, the close) renders into the
        // built-in ProPhoto RGB, not the twin, and converts that raster to Lab
        // codes by the Mode rule, so the colour visit is priced against
        // ProPhoto, the profile the edit renders into. An asset already in the
        // twin (Load Files into Stack's Lab clip, converted by the Mode rules
        // first) holds Lab codes: it renders into the twin unchanged, as before.
        let lab_place = self.lab_policy().is_some()
            && matches!(edit, SourceEdit::Place { .. })
            && source.asset.profile().bytes() != self.profile.bytes();
        let render_profile = if lab_place {
            crate::icc::IccProfile::prophoto_rgb()
        } else {
            self.profile.clone()
        };
        let icc = profile_cost(source.asset.profile(), &render_profile, true)?;
        let mask_dest = if let Some(matrix) = prepared.incremental {
            mask_bounds(
                self.layer(prepared.index).ok_or("invalid source index")?,
                matrix,
            )?
        } else {
            None
        };
        let hypothetical = if let Some(name) = prepared.name {
            let leaf = if self.is_float() {
                PrecisionDocumentLayer::raster_float
            } else {
                PrecisionDocumentLayer::raster
            };
            let mut layer = leaf(
                name,
                source.rendered.clone(),
                source.rendered_offset,
                1.,
                true,
            )?;
            layer.retained_source = Some(source.clone());
            // PLAN ≡ EDIT: the hypothetical splices through the SAME slot the
            // edit will, so `prepared.index` is the placed record's index in
            // both. An absent slot is the legacy push, byte for byte.
            if let Some(slot) = prepared.slot {
                self.inserting_record_for_plan(layer, slot)?
            } else {
                let mut layers = self.layers.as_ref().clone();
                layers.push(layer);
                let mut next = self.clone();
                next.records = crate::precision_document::shape(&layers)?.0;
                next.layers = Arc::new(layers);
                next
            }
        } else {
            let mut layer = self
                .layer(prepared.index)
                .ok_or("invalid source index")?
                .clone();
            layer.retained_source = Some(source.clone());
            if matches!(edit, SourceEdit::CaptureRaster { .. }) {
                layer.description = None;
            }
            self.replacing_layer_for_plan(prepared.index, layer)?
        };
        // The plan GUARD: everything below keys on `prepared.index`, so the
        // hypothetical's record there must be the one that carries the source.
        // It never fires on the legacy paths (a push makes `index` the last
        // record, and the other edits write the source AT `index`).
        if hypothetical
            .layer(prepared.index)
            .and_then(|l| l.retained_source.as_ref())
            .is_none()
        {
            return Err("invalid source index".into());
        }
        let mut rasters = Vec::new();
        for i in 0..hypothetical.layer_count() {
            if let Some(image) = hypothetical.layer_source(i) {
                rasters.push(if i == prepared.index {
                    (dest.2, dest.3)
                } else {
                    image.dimensions()
                });
            }
        }
        let mut masks = Vec::new();
        mask_dimensions(
            &hypothetical.layers,
            hypothetical
                .layer(prepared.index)
                .ok_or("invalid source index")?,
            mask_dest.map(|d| (d.2, d.3)),
            &mut masks,
        );
        hypothetical.native_fits_geometry(
            self.dimensions(),
            &rasters,
            &masks,
            self.channels.len(),
        )?;
        // A typed source `transform` without `interactive` is a COMMIT
        // (16-bit ⌘T of a placed image). It takes its caller's cap
        // (`transform_cap`: none under a job control) with the
        // transform commit's headroom. A Replace plans in `replacement_plan`.
        // The ⌘T preview and every other source edit keep their unscaled
        // figure (`precision_geometry_budget`).
        let transform = matches!(
            edit,
            SourceEdit::Transform {
                interactive: false,
                ..
            }
        );
        // Only the ⌘T PREVIEW keeps `MAX_WORK_UNITS`; every
        // other source edit takes its caller's cap.
        let preview = matches!(
            edit,
            SourceEdit::Transform {
                interactive: true,
                ..
            }
        );
        let work_limit = if preview {
            crate::precision_render::MAX_WORK_UNITS
        } else {
            transform_cap
        };
        let mut plan = work_plan_within(
            work_limit,
            inventory.unique_bytes(),
            crate::retained_source_render::SourceStage {
                asset: &source.asset,
            },
            (dest.2, dest.3),
            mask_dest.map_or(0, |d| u64::from(d.2) * u64::from(d.3)),
            sampling,
            self.source_fallback_depth(),
            icc,
        )?;
        if self.is_float() {
            plan.scratch_bytes = plan
                .scratch_bytes
                .checked_add(float_render_scratch(
                    source.asset.dimensions(),
                    (dest.2, dest.3),
                )?)
                .ok_or("source render size overflow")?;
        }
        plan.scratch_bytes =
            crate::precision_geometry::source_transform_scratch(transform, plan.scratch_bytes)?;
        // A Place on a Grayscale document (#1, the close) folds the
        // rendered raster to gray before it publishes it: the one image
        // `gray_artwork::folded_result` may build, 0 on any other document.
        if prepared.name.is_some() && matches!(edit, SourceEdit::Place { .. }) {
            plan.scratch_bytes = plan
                .scratch_bytes
                .checked_add(crate::gray_artwork::fold_bytes(
                    self,
                    u64::from(dest.2) * u64::from(dest.3),
                ))
                .ok_or("source render size overflow")?;
        }
        // A Place on a Lab document (#1, the close) converts the ProPhoto
        // render to Lab codes: the converted RGBA16 image (8 bytes a pixel)
        // beside the render, plus the conversion pipeline's own scratch.
        if prepared.name.is_some() && lab_place {
            let converted = u64::from(dest.2)
                .checked_mul(u64::from(dest.3))
                .and_then(|pixels| pixels.checked_mul(8))
                .ok_or("source render size overflow")?;
            let pipeline = crate::lab_convert::to_lab_pipeline(&render_profile)?.scratch_bytes();
            plan.scratch_bytes = plan
                .scratch_bytes
                .checked_add(converted)
                .and_then(|bytes| bytes.checked_add(pipeline))
                .ok_or("source render size overflow")?;
        }
        Ok(plan)
    }
    pub fn source_edit(
        &self,
        input: Option<&Self>,
        edit: &SourceEdit,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        self.source_edit_input(input.map(SourceInput::Typed), edit, limit, other, control)
    }
    pub fn source_edit_input(
        &self,
        input: Option<SourceInput<'_>>,
        edit: &SourceEdit,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        self.source_edit_input_with_context(
            input,
            edit,
            limit,
            other,
            control.map_or(JobContext::NONE, JobContext::new),
        )
    }
    pub(crate) fn source_edit_with_context(
        &self,
        input: Option<&Self>,
        edit: &SourceEdit,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.source_edit_input_with_context(input.map(SourceInput::Typed), edit, limit, other, job)
    }
    pub(crate) fn source_edit_input_with_context(
        &self,
        input: Option<SourceInput<'_>>,
        edit: &SourceEdit,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        job.check()?;
        let plan = self.source_plan_input(
            input,
            edit,
            crate::sampling_gaussian::destructive_work_cap(job),
        )?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        if matches!(edit, SourceEdit::Rasterize { .. }) {
            let result = self.source_metadata_edit(edit)?;
            job.report(1., 5)?;
            return Ok(result);
        }
        if let SourceEdit::Transform { matrix, .. } = edit {
            if retained_source::identity(*matrix) {
                job.report(1., 5)?;
                return Ok(None);
            }
        }
        let prepared = self.prepare_source(input, edit)?;
        // A Place on a Lab document renders into ProPhoto RGB (the plan's
        // `lab_place`, with its twin-asset exception), then converts to Lab
        // codes below.
        let lab_place = self.lab_policy().is_some()
            && matches!(edit, SourceEdit::Place { .. })
            && prepared.source.asset.profile().bytes() != self.profile.bytes();
        let render_profile = if lab_place {
            crate::icc::IccProfile::prophoto_rgb()
        } else {
            self.profile.clone()
        };
        // A Lab placement renders into ProPhoto, which no transfer belongs to.
        let codes = if lab_place { None } else { self.linear_codes() };
        let source = render_typed(
            prepared.source,
            &render_profile,
            self.is_float(),
            codes,
            job,
        )?;
        let result = if let Some(name) = prepared.name {
            let leaf = if self.is_float() {
                PrecisionDocumentLayer::raster_float
            } else {
                PrecisionDocumentLayer::raster
            };
            // A Place on a Grayscale document (#1, the close) publishes the
            // rendered pixels folded to gray and keeps no retained source: a
            // re-render from the colour master would bring colour back, so
            // the layer is a plain raster (the host's chained `rasterize`
            // then changes nothing).
            let gray_place =
                self.gray_policy().is_some() && matches!(edit, SourceEdit::Place { .. });
            // A Place on a Lab document (#1, the close) publishes the
            // ProPhoto render converted to Lab codes by Mode ▸ Lab Color's
            // rule, and, as Gray's, keeps no retained source.
            let pixels = if gray_place {
                crate::gray_artwork::folded_result(self, source.rendered.clone(), job)?
            } else if let Some(policy) = self.lab_policy().filter(|_| lab_place) {
                crate::lab_convert::converted_image(
                    &source.rendered,
                    &crate::lab_convert::to_lab_pipeline(&render_profile)?,
                    policy.twin(),
                    &|unit| policy.code(unit),
                    job,
                )?
            } else {
                source.rendered.clone()
            };
            let mut layer = leaf(name, pixels, source.rendered_offset, 1., true)?;
            if !gray_place && !lab_place {
                layer.retained_source = Some(source);
            }
            // The same splice the plan's hypothetical made, at the same slot.
            if let Some(slot) = prepared.slot {
                Some(self.inserting_record(layer, slot)?)
            } else {
                let mut layers = self.layers.as_ref().clone();
                layers.push(layer);
                Some(self.rebuilt(
                    self.width,
                    self.height,
                    layers,
                    self.channels.as_ref().clone(),
                )?)
            }
        } else {
            let old = self.layer(prepared.index).ok_or("invalid source index")?;
            let mut target = old.clone();
            if matches!(edit, SourceEdit::CaptureRaster { .. }) {
                target.description = None;
            }
            if let Some(matrix) = prepared.incremental {
                if let (Some(mask), Some(bounds)) = (&old.mask, mask_bounds(old, matrix)?) {
                    let m = Affine::from_array(matrix);
                    let exact = exact_form(&m);
                    let plane = if matches!(exact, Some(Exact::Translate)) {
                        mask.plane.clone()
                    } else {
                        precision_sampling::transform_plane_outside(
                            &mask.plane,
                            (bounds.2, bounds.3),
                            &SourceMap::new(
                                &m.inverse().ok_or("singular linked-mask matrix")?,
                                mask.properties.offset,
                                (bounds.0, bounds.1),
                            ),
                            if exact.is_some() {
                                GeometryFilter::Nearest
                            } else {
                                source.filter
                            },
                            u16::from(mask.properties.outside_coverage) * 257,
                            job,
                        )?
                    };
                    let mut properties = mask.properties.clone();
                    properties.offset = (bounds.0, bounds.1);
                    target.mask = Some(PrecisionMask::new(plane, properties, mask.enabled)?);
                }
            }
            target.content = Content::Raster {
                image: source.rendered.clone(),
                offset: source.rendered_offset,
            };
            target.retained_source = Some(source);
            self.replacing_layer(prepared.index, target)?
        };
        job.report(1., 5)?;
        Ok(result)
    }
}
