//! Immutable byte-fallback source edits. Original asset depth is independent;
//! all conversion, geometry, native capacity and work is planned before pixels.
//!
//! Since 12E-2b `SourceEdit::Place` takes an OPTIONAL `anchor`/`position` pair (`parse` refuses one
//! without the other; both absent is the legacy top-root push byte for byte) and `Prepared.slot` is
//! resolved ONCE in `prepare_source` so the PLAN's hypothetical and the EDIT splice at the SAME
//! slot — the `dest` substitution keys on `prepared.index`, so a positioned plan that still pushed
//! would forecast a different document. A plan GUARD backs it: the hypothetical's record at
//! `prepared.index` must carry a retained source, else `"invalid source index"`. A byte source
//! `transform` without `interactive` (a commit) takes its caller's cap and
//! `source_transform_scratch`'s 5/4 as the typed one does, and the byte geometry plan bounds each
//! placed layer's source render by the edit's own limit
//! (`doc_source_geometry::source_geometry_work`). Only a source `transform` PREVIEW
//! (`interactive`) keeps `MAX_WORK_UNITS`; every other source edit takes the caller's cap
//! (`CONTROLLED_JOB_WORK` for a plan). A one-shot commit charges a selected placed layer's raster
//! and destination beside the largest source plan's 5/4 (`doc_source_geometry`'s
//! `# Admission: the one-shot set transform`).
use crate::doc::{DrDocument, Layer, LayerRaster};
use crate::doc_lock::EditKind;
use crate::doc_transform::{
    exact_form, resample_mask_outside, transformed_plane_bounds, Affine, Exact, SourceMap,
};
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::InsertSlot;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::precision_sampling::GeometryFilter;
use crate::resources::Inventory;
use crate::retained_source::{
    self, compose, source_bounds, DepthConversion, RetainedSource8, SourceAsset, SourceEdit,
    SourceInput, SourceLink, SourcePlan,
};
use crate::retained_source_render::{profile_cost, render8, work_plan_within, SourceSampling};
use crate::source_transform::{self, SourceTransform};
use serde_json::{json, Value};
use std::sync::Arc;

/// The one resolved form of a source edit, read by BOTH the plan and the
/// edit. `index` is the placed or targeted layer's index IN THE RESULT, and
/// `slot` is the placement it came from: `Some` ⇒ `index == slot.at` and the
/// record is SPLICED there, `None` ⇒ `index == layers.len()` and the record is
/// PUSHED, which is what every pre-12E-2b caller does. Both are resolved once
/// in `prepare_source` so the plan's hypothetical and the edit's result can
/// never disagree about where the layer lands — the `dest` substitution below
/// keys on `index`, so a positioned plan that still pushed would forecast a
/// different document.
struct Prepared<'a> {
    source: RetainedSource8,
    index: usize,
    name: Option<&'a str>,
    incremental: Option<[f64; 6]>,
    slot: Option<InsertSlot>,
}
/// Replace's merge-lock refusal, shared with Edit Contents' commit.
const REPLACE_LOCKED: &str = "source replacement is blocked by pixel or transparency locks";
fn claims_adjustment(meta: Option<&str>) -> bool {
    #[derive(serde::Deserialize)]
    struct Kind {
        #[serde(rename = "type")]
        kind: String,
    }
    meta.and_then(|text| serde_json::from_str::<Kind>(text).ok())
        .is_some_and(|value| value.kind == "adjust")
}
fn mask_bounds(layer: &Layer, matrix: [f64; 6]) -> Result<Option<(i32, i32, u32, u32)>, String> {
    if !layer.mask_properties.linked {
        return Ok(None);
    }
    layer
        .mask
        .as_ref()
        .map(|mask| {
            let bounds = transformed_plane_bounds(
                mask.dimensions(),
                layer.mask_properties.offset,
                &retained_source::validate_matrix(matrix)?,
            )
            .ok_or("retained source linked mask exceeds supported bounds")?;
            if !layer.mask_properties.valid((bounds.2, bounds.3)) {
                return Err(
                    "retained source linked mask properties exceed supported bounds".into(),
                );
            }
            Ok(bounds)
        })
        .transpose()
}
impl DrDocument {
    pub fn has_retained_source(&self, index: usize) -> bool {
        self.layers
            .get(index)
            .is_some_and(|l| l.effective_source(&self.profile).is_some())
    }
    pub fn source_info_scratch_bytes(&self, index: usize) -> u64 {
        self.layers
            .get(index)
            .and_then(|l| l.effective_source(&self.profile))
            .map_or(64 * 1024, |s| {
                retained_source::info_scratch(&s.asset, s.link.as_deref())
            })
    }
    pub fn source_info(&self, index: usize) -> Result<Value, String> {
        let layer = self
            .layers
            .get(index)
            .ok_or("source layer index is out of range")?;
        Ok(layer.effective_source(&self.profile).map_or_else(
            || json!({"valid":false}),
            |s| retained_source::info(&s.asset, s.link.as_deref(), s.transform, s.filter, 8),
        ))
    }
    fn prepare_source<'a>(
        &self,
        input: Option<SourceInput<'_>>,
        edit: &'a SourceEdit,
    ) -> Result<Prepared<'a>, String> {
        let required = |index: usize| {
            self.layers
                .get(index)
                .and_then(|l| l.effective_source(&self.profile))
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
                let old = self
                    .layers
                    .get(*layer)
                    .ok_or("capture target is out of range")?;
                if old.kind != crate::doc::LayerKind::Raster
                    || old.retained_source.is_some()
                    || claims_adjustment(old.meta.as_deref())
                {
                    return Err(
                        "capture requires an ordinary raster without a retained binding".into(),
                    );
                }
                if self.lock_block(*layer, EditKind::Merge) != 0 {
                    return Err("source capture is blocked by pixel or transparency locks".into());
                }
                if self
                    .layers
                    .iter()
                    .filter_map(|l| l.effective_source(&self.profile))
                    .any(|s| s.asset.id == *source_id)
                {
                    return Err("source identity already exists in this document".into());
                }
                let asset = input
                    .ok_or("source capture requires decoded input")?
                    .asset(source_id, revision_id.as_deref().unwrap_or(source_id), None)?;
                depth_conversion.require(&asset, 8)?;
                if let Some(link) = link {
                    link.validate()?;
                }
                let dest = source_bounds(asset.dimensions(), *source_transform)?;
                if self.lock_block(*layer, EditKind::Position) != 0
                    && ((dest.0, dest.1) != old.offset
                        || (dest.2, dest.3) != old.pixels.dimensions())
                {
                    return Err("source capture would change a position-locked layer extent".into());
                }
                Ok(Prepared {
                    source: RetainedSource8 {
                        asset,
                        link: link.clone().map(Arc::new),
                        transform: SourceTransform::Affine(*source_transform),
                        filter: GeometryFilter::Lanczos3,
                        rendered: old.pixels.clone(),
                        rendered_offset: old.offset,
                        rendered_profile: self.profile.clone(),
                    },
                    index: *layer,
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
                if self
                    .layers
                    .iter()
                    .filter_map(|l| l.effective_source(&self.profile))
                    .any(|s| s.asset.id == *source_id)
                {
                    return Err("source identity already exists in this document".into());
                }
                // The ONE slot rule (`layer_insert`), resolved before anything
                // is decoded so a refused position costs no asset. Absent is
                // the legacy push.
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
                depth_conversion.require(&asset, 8)?;
                let rendered = asset
                    .rgba8()
                    .cloned()
                    .unwrap_or_else(|| LayerRaster::unseeded(image::RgbaImage::new(1, 1)));
                Ok(Prepared {
                    source: RetainedSource8 {
                        asset,
                        link: link.clone().map(Arc::new),
                        transform: SourceTransform::Affine(compose(
                            [1., 0., 0., 1., f64::from(*x), f64::from(*y)],
                            source_transform.unwrap_or([1., 0., 0., 1., 0., 0.]),
                        )),
                        filter: GeometryFilter::Lanczos3,
                        rendered,
                        rendered_offset: (*x, *y),
                        rendered_profile: self.profile.clone(),
                    },
                    index: slot.map_or(self.layers.len(), |s| s.at),
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
                if self.lock_block(*layer, EditKind::Merge) != 0 {
                    return Err(REPLACE_LOCKED.into());
                }
                let source = required(*layer)?;
                if self
                    .layers
                    .iter()
                    .filter_map(|l| l.effective_source(&self.profile))
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
                if self.lock_block(*layer, EditKind::Position) != 0 {
                    return Err("source layer position is locked".into());
                }
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
    /// Layer `layer`'s binding rebound to `asset`: Replace's rule. Refuses a
    /// layer whose locks refuse a merge, one with no effective source, a
    /// depth change without `ConvertFallback`, and a position-locked layer
    /// whose extent would move. `source_transform` replaces the accumulated affine;
    /// `coordinate_map` (new master coordinates into the old) is precomposed
    /// with it. The link becomes `link`.
    pub(crate) fn rebound_source(
        &self,
        layer: usize,
        asset: Arc<SourceAsset>,
        link: Option<&SourceLink>,
        depth_conversion: DepthConversion,
        source_transform: Option<[f64; 6]>,
        coordinate_map: Option<[f64; 6]>,
    ) -> Result<RetainedSource8, String> {
        if self.lock_block(layer, EditKind::Merge) != 0 {
            return Err(REPLACE_LOCKED.into());
        }
        let mut source = self
            .layers
            .get(layer)
            .and_then(|l| l.effective_source(&self.profile))
            .ok_or("layer has no retained ordinary source")?;
        source.asset = asset;
        depth_conversion.require(&source.asset, 8)?;
        if let Some(matrix) = source_transform {
            retained_source::validate_matrix(matrix)?;
            source.transform = SourceTransform::Affine(matrix);
        } else if let Some(matrix) = coordinate_map {
            source.transform =
                source_transform::compose(source.transform, SourceTransform::Affine(matrix))?;
        }
        source.link = link.cloned().map(Arc::new);
        let dest = source_transform::bounds(source.asset.dimensions(), source.transform)?;
        let old = &self.layers[layer];
        if self.lock_block(layer, EditKind::Position) != 0
            && ((dest.0, dest.1) != old.offset || (dest.2, dest.3) != old.pixels.dimensions())
        {
            return Err("source replacement would change a position-locked layer extent".into());
        }
        Ok(source)
    }
    /// Replace's plan for layer `layer` rebound to `source`, with `retained`
    /// the caller's inventory: the native capacity of the result and the
    /// render's work and scratch. A plan: no work refusal
    /// (`CONTROLLED_JOB_WORK`).
    pub(crate) fn replacement_plan(
        &self,
        layer: usize,
        source: &RetainedSource8,
        retained: u64,
    ) -> Result<SourcePlan, String> {
        let dest = source_transform::bounds(source.asset.dimensions(), source.transform)?;
        let sampling = SourceSampling::new(
            source.asset.dimensions(),
            (dest.2, dest.3),
            source.transform,
            source.filter,
        );
        let icc = profile_cost(
            source.asset.profile(),
            &self.profile,
            source.asset.depth() >= 16,
        )?;
        let mut hypothetical = self.source_capacity_view();
        hypothetical
            .layers
            .get_mut(layer)
            .ok_or("invalid source index")?
            .retained_source = Some(source.clone());
        if !self.native_source_layers_fit_dimensions(
            &hypothetical,
            hypothetical.layers.iter().enumerate().map(|(i, l)| {
                if i == layer {
                    (dest.2, dest.3)
                } else {
                    l.pixels.dimensions()
                }
            }),
        ) {
            return Err("retained source exceeds native layer or asset capacity".into());
        }
        let mut plan = work_plan_within(
            crate::sampling_gaussian::CONTROLLED_JOB_WORK,
            retained,
            crate::retained_source_render::SourceStage {
                asset: &source.asset,
            },
            (dest.2, dest.3),
            0,
            sampling,
            8,
            icc,
        )?;
        plan.scratch_bytes = plan
            .scratch_bytes
            .checked_add(self.cloned_structure_bytes())
            .ok_or("source clone scratch overflow")?;
        plan.scratch_bytes =
            crate::precision_geometry::source_transform_scratch(false, plan.scratch_bytes)?;
        Ok(plan)
    }
    fn source_metadata_edit(
        &self,
        edit: &SourceEdit,
        planning: bool,
    ) -> Result<Option<Self>, String> {
        let index = match edit {
            SourceEdit::Rasterize { layer } => *layer,
            _ => return Err("expected source metadata edit".into()),
        };
        let old = self
            .layers
            .get(index)
            .ok_or("source layer index is out of range")?;
        if old.effective_source(&self.profile).is_none() {
            return Ok(None);
        }
        if self.lock_block(index, EditKind::Pixels) != 0 {
            return Err("source layer pixels are locked".into());
        }
        let mut next = if planning {
            self.source_capacity_view()
        } else {
            self.clone()
        };
        next.layers[index].retained_source = None;
        if !self.native_edit_fits(&next) {
            return Err("source metadata exceeds native capacity".into());
        }
        next.detach_invalid_sources();
        Ok(Some(next))
    }
    pub fn source_plan(
        &self,
        input: Option<&Self>,
        edit: &SourceEdit,
    ) -> Result<SourcePlan, String> {
        // The job-less plan carries no work refusal.
        self.source_plan_input(
            input.map(SourceInput::Byte),
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
        inventory.document(self);
        if matches!(edit, SourceEdit::Rasterize { .. }) {
            self.source_metadata_edit(edit, true)?;
            return Ok(SourcePlan {
                retained_bytes: inventory.unique_bytes(),
                scratch_bytes: 4 * BOOKKEEPING_BYTES + self.cloned_structure_bytes(),
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
        let icc = profile_cost(
            source.asset.profile(),
            &self.profile,
            source.asset.depth() >= 16,
        )?;
        let mask_pixels = if let Some(matrix) = prepared.incremental {
            if !self.mask_transform_fits(prepared.index, &Affine::from_array(matrix)) {
                return Err("source transform exceeds native mask capacity".into());
            }
            mask_bounds(&self.layers[prepared.index], matrix)?
                .map_or(0, |(_, _, w, h)| u64::from(w) * u64::from(h))
        } else {
            0
        };
        // The placeholder is only metadata. Final fallback extents and genuine
        // asset depths are supplied to the shared native capacity checks.
        let mut hypothetical = self.source_capacity_view();
        if let Some(name) = prepared.name {
            let mut layer = Layer::new(image::RgbaImage::new(1, 1), name);
            layer.pixels = source.rendered.clone();
            layer.offset = source.rendered_offset;
            layer.retained_source = Some(source.clone());
            // PLAN ≡ EDIT: the hypothetical splices through the SAME slot the
            // edit will, so `prepared.index` is the placed layer's index in
            // both. An absent slot is the legacy push, byte for byte.
            if let Some(slot) = prepared.slot {
                hypothetical = hypothetical
                    .inserting_layer(layer, slot)
                    .ok_or("placement position could not be inserted")?;
            } else {
                hypothetical.layers.push(layer);
            }
        } else {
            hypothetical.layers[prepared.index].retained_source = Some(source.clone());
            if matches!(edit, SourceEdit::CaptureRaster { .. }) {
                hypothetical.layers[prepared.index].meta = None;
            }
        }
        // The plan GUARD: everything below keys on `prepared.index`, so the
        // hypothetical's record there must be the one that carries the source.
        // It never fires on the legacy paths (a push makes `index == len - 1`
        // in the result, and the other edits write the source AT `index`).
        if hypothetical
            .layers
            .get(prepared.index)
            .and_then(|l| l.retained_source.as_ref())
            .is_none()
        {
            return Err("invalid source index".into());
        }
        if !self.native_source_layers_fit_dimensions(
            &hypothetical,
            hypothetical.layers.iter().enumerate().map(|(i, l)| {
                if i == prepared.index {
                    (dest.2, dest.3)
                } else {
                    l.pixels.dimensions()
                }
            }),
        ) {
            return Err("retained source exceeds native layer or asset capacity".into());
        }
        // A byte source `transform` without `interactive` is a commit.
        // It takes its caller's cap (`transform_cap`: none under a job
        // control) with the transform commit's headroom, as the typed
        // one does (`precision_geometry_budget`). A Replace plans in
        // `replacement_plan`; every other source edit keeps its unscaled
        // figure.
        let transform = matches!(
            edit,
            SourceEdit::Transform {
                interactive: false,
                ..
            }
        );
        // Only a transform PREVIEW keeps `MAX_WORK_UNITS`; every
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
            mask_pixels,
            sampling,
            8,
            icc,
        )?;
        plan.scratch_bytes = plan
            .scratch_bytes
            .checked_add(self.cloned_structure_bytes())
            .ok_or("source clone scratch overflow")?;
        // Measured (M3, `precision_geometry_budget`): the unscaled figure
        // covered only ~1.007× the byte commit's measured peak.
        plan.scratch_bytes =
            crate::precision_geometry::source_transform_scratch(transform, plan.scratch_bytes)?;
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
        self.source_edit_input(input.map(SourceInput::Byte), edit, limit, other, control)
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
            let result = self.source_metadata_edit(edit, false)?;
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
        let source = render8(prepared.source, &self.profile, job)?;
        let mut next = self.clone();
        if let Some(name) = prepared.name {
            let mut layer = Layer::new(image::RgbaImage::new(1, 1), name);
            layer.pixels = source.rendered.clone();
            layer.offset = source.rendered_offset;
            layer.retained_source = Some(source);
            // The same splice the plan's hypothetical made, at the same slot.
            if let Some(slot) = prepared.slot {
                next = next
                    .inserting_layer(layer, slot)
                    .ok_or("placement position could not be inserted")?;
            } else {
                next.layers.push(layer);
            }
        } else {
            let target = &mut next.layers[prepared.index];
            if matches!(edit, SourceEdit::CaptureRaster { .. }) {
                target.meta = None;
            }
            if let Some(matrix) = prepared.incremental {
                let old = &self.layers[prepared.index];
                if let (Some(raw), Some(bounds)) = (&old.mask, mask_bounds(old, matrix)?) {
                    let m = Affine::from_array(matrix);
                    let exact = exact_form(&m);
                    target.mask = Some(if matches!(exact, Some(Exact::Translate)) {
                        raw.clone()
                    } else {
                        Arc::new(resample_mask_outside(
                            raw,
                            bounds,
                            &SourceMap::new(
                                &m.inverse().ok_or("singular linked-mask matrix")?,
                                old.mask_properties.offset,
                                (bounds.0, bounds.1),
                            ),
                            if exact.is_some() {
                                GeometryFilter::Nearest.sampler()
                            } else {
                                source.filter.sampler()
                            },
                            old.mask_properties.outside_coverage,
                            job,
                        )?)
                    });
                    target.mask_properties.offset = (bounds.0, bounds.1);
                }
                target.style = crate::style::scaled_style(
                    &target.style,
                    Affine::from_array(matrix).determinant().abs().sqrt(),
                );
            }
            target.pixels = source.rendered.clone();
            target.offset = source.rendered_offset;
            target.retained_source = Some(source);
        }
        if !self.native_edit_fits(&next) {
            return Err("retained source result exceeds native capacity".into());
        }
        next.detach_invalid_sources();
        job.report(1., 5)?;
        Ok(Some(next))
    }
}
