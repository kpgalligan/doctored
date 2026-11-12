//! Byte whole-document source preservation. Exact permutations carry their
//! fallback codes and rebind the immutable master; filtered resize evaluates
//! the original asset under the accumulated canvas map.
//!
//! # Admission: a filtered turn's destination
//! A filtered Rotate Canvas / Straighten charges its destination once
//! (`CANVAS_TURN_DESTINATION_COPIES`, was 6). Measured at 4000×3000, 17°,
//! bilinear, 3 runs each, one case per process, E = `/usr/bin/time -l` peak −
//! pre-edit `ri_phys_footprint` (bytes): rotate peaks 298,500,672 / 298,451,520
//! / 298,500,672 over pre 97,681,792 / 97,714,560 / 97,698,176, max E
//! 200,818,880; straighten (window 3600×2700) max E 200,818,880; straighten
//! with 40 saved channels max E 1,060,160,352 (peak 1,651,361,400, pre
//! 591,201,048). Baseline peaks sit at pre (97.7 MB / 591.2 MB). With copies 1
//! the plans are 692,260,864 (rotate; HEAD 2,212,593,664), 608,210,944
//! (straighten; HEAD 1,708,294,144) and 1,477,010,944 (40 channels, charged
//! `(canvas + output) × C`; HEAD 4,041,094,144), each ≥ 1.25 × max E. The
//! 16-bit plans (404,282,288 both, max E 401,129,832) now take × 5/4
//! (`precision_geometry_plan`'s filtered turn): 505,352,860.
//!
//! # Admission: a filtered turn's layer and canvas terms
//! A filtered turn charges `a·pixels + (destination − channels) + 5/4 ·
//! (canvas + output) · C + b·canvas + cloned structure + source scratch +
//! 4 MiB`, with
//! `CANVAS_TURN_LAYER_FACTOR` a = 2 (was 6) and `CANVAS_TURN_CANVAS_BYTES`
//! b = 0 (was 8): the smallest T5 plan over a ∈ 0..=6, b ∈ 0..=8 that keeps
//! every case ≥ 1.25 × max E (the method above; every case peak above its
//! baseline's). Max E / plan in bytes, 4000×3000 unless named, 17° bilinear:
//! T1 rotate 200,786,112 / 404,262,338; T2 straighten 200,802,496 /
//! 320,212,418; T3 rotate, 4 masked layers 506,429,824 / 1,320,383,225; T4
//! its straighten 506,446,208 / 1,124,266,745; T5 rotate at 8000×6000
//! 801,817,176 / 1,603,902,466 (HEAD 2,755,900,992, refused); T6 straighten,
//! 40 channels 1,060,193,144 / 1,406,214,538; T7 rotate, 32 channels
//! 809,763,464 / 1,644,430,434; T8 straighten, one 8000×6000 layer
//! overhanging the canvas 754,467,344 / 980,122,443 (it sets a ≥ 2); T9 its
//! rotate 753,828,392 / 1,148,222,283. A channel costs 21,484,766 B
//! straightened (≤ canvas + output, 21,720,000) and 19,030,542 B rotated (>
//! the 19,004,160-pixel output), so both turns charge `(canvas + output) × C`.
//! Every nested reservation stays under the plan: a sourced layer's re-render
//! reserves `max(own plan, floor)`, and its own plan, `source_scratch + mask
//! pixels + cloned structure`, is a part of the plan.
//!
//! # Admission: the one-shot set transform
//! A ONE-SHOT set transform (a commit, `precision_geometry::one_shot`) once
//! skipped a selected placed layer's current raster — its re-render
//! (`transform_layer_unlocked_with_job`, a non-interactive source
//! `transform`) never reads it — and charged the member by its own source
//! plan with the commit's 5/4, S'_i, and its held output O_i instead. That
//! decision was reversed: the skip existed only to admit 24 MP work at
//! 2 GiB. Every selected layer's raster and destination are charged again,
//! so the transform arm is `pixels·6 + total·6 + canvas·8 + max_i S'_i +
//! 4 MiB` (a nested re-render still reserves S'_i).
//! A preview (`interactive`), Resize and the turns are unchanged. Measured
//! 2026-10-03 (Apple M2 Max, release, one case per process, three runs,
//! `MallocLargeCache=0`, cold cache, E = `ri_interval_max_phys_footprint` −
//! pre, a baseline twin per case reading E 0): a white 6000×4000 layer under
//! a placed 6000×4000 plasma Display P3 master in an sRGB document,
//! `affine_set` ×0.9 Lanczos3 about (3000, 2000). The E column stands; the
//! retained and scratch figures are from before the reversal, and
//! today's plans are larger. The σ4 rows had a live filter on the placed
//! layer; live filters, and `filtered_source_ops_24mp`, the test that
//! printed the plans, were removed on 2026-10-10.
//!
//! | case | E (three runs) | retained | scratch | scratch / max E |
//! |---|---|---|---|---|
//! | G8 σ4 | 252,362,920 / 252,346,536 / 252,346,536 | 288,029,863 | 1,213,147,904 | 4.807 |
//! | G8-0 no filter | 252,002,472 / 252,018,856 / 251,986,088 | 288,030,186 | 1,093,147,904 | 4.338 |
//! | G8-2L σ4 + an unfiltered placed layer, both moving | 425,509,176 / 425,492,792 / 425,525,560 | 480,037,536 | 1,290,907,904 | 3.034 |
//! | G16 (typed `affine_set`, σ4; plan unchanged here) | 541,376,872 / 541,393,256 / 541,393,256 | 385,254,767 | 881,336,205 | 1.628 |
//!
//! At 23d7775 G8 planned 2,167,517,184 (the app's "2.02 GB on its own") and
//! G8-0 2,071,517,184.
//! Above 2.0 the dominating terms are the unselected white layer's ×6 and
//! the canvas ×8. Before the reversal G8-2L − G8 was
//! exactly O_2 = 77,760,000; now it is six copies of the
//! second member's raster and six of O_2. At 45 MP (8256×5504, G8-45) the
//! plan was 545,330,335 + 4,261,671,840 = 4,807,002,175, within a 16 GB
//! Mac's 8 GiB working limit.
//!
//! `geometry_edit_plan` charges a filtered Rotate Canvas / Straighten destination once
//! (`CANVAS_TURN_DESTINATION_COPIES`, measured; every other edit keeps ×6), and its layer and
//! canvas terms are measured too (`CANVAS_TURN_LAYER_FACTOR` 2, `CANVAS_TURN_CANVAS_BYTES` 0, was
//! 6 and 8) and both turns charge saved channels at 5/4 of `(canvas + output) × C`; the raw
//! numbers are in this module doc's `# Admission: a filtered turn's destination` and
//! `# Admission: a filtered turn's layer and canvas terms`.

use image::imageops::FilterType;

use crate::doc::{DrDocument, Geometry};
use crate::doc_geometry_set::is_adjustment;
use crate::doc_perspective::{
    parallelogram_affine, solve_rect_to_quad, transformed_projective_bounds,
};
use crate::doc_transform::transformed_plane_bounds;
use crate::doc_transform::Affine;
use crate::job_control::JobContext;
use crate::precision_geometry::{GeometryEdit, GeometryTarget};
use crate::precision_image::admit;
use crate::precision_sampling::GeometryFilter;
use crate::retained_source::RetainedSource8;
use crate::retained_source::SourcePlan;
use crate::retained_source_render::{self, SourceSampling, SourceStage};
use crate::source_transform::{self, SourceTransform};

/// Destination copies a filtered whole-canvas turn charges (measured).
const CANVAS_TURN_DESTINATION_COPIES: u64 = 1;
/// Bytes a filtered turn charges per byte of layer pixels + masks (measured; was 6).
/// A single layer overhanging the canvas far beyond T8's 4× area is an
/// extrapolation: its plan stays above the peak but drifts toward ~1.19×.
const CANVAS_TURN_LAYER_FACTOR: u64 = 2;
/// Bytes a filtered turn charges per canvas pixel (measured; was 8).
const CANVAS_TURN_CANVAS_BYTES: u64 = 0;

/// 5/4 of `bytes`, rounded up: saved channels' own 1.25× headroom.
fn channel_headroom(bytes: u64) -> u64 {
    bytes.saturating_mul(5).div_ceil(4)
}

/// `channels` = the `output_canvas × C` term the whole-document branch adds (0 when skipped).
struct DestinationBytes {
    total: u64,
    channels: u64,
    source_scratch: u64,
    source_work: u64,
}

impl DrDocument {
    /// Bounded byte geometry request. The byte document's existing pure
    /// kernels own each ordinary whole edit; the new set perspective owns one
    /// shared homography across its complete expanded selection.
    pub fn geometry_edit_plan(&self, edit: &GeometryEdit) -> Result<SourcePlan, String> {
        // The job-less plan carries no work refusal.
        self.geometry_edit_plan_with_cap(edit, crate::sampling_gaussian::CONTROLLED_JOB_WORK)
    }

    /// [`Self::geometry_edit_plan`] with a one-shot commit's placed-source
    /// renders bounded by `one_shot_cap` (`destructive_work_cap(job)` for a
    /// run).
    fn geometry_edit_plan_with_cap(
        &self,
        edit: &GeometryEdit,
        one_shot_cap: u64,
    ) -> Result<SourcePlan, String> {
        // The Background fill is planned over the padded document's
        // stand-in, plus the pad itself (`doc_crop_extend`).
        if let Some((padded, raster)) = self.background_plan_input(edit)? {
            let mut plan = padded.geometry_edit_plan_with_cap(edit, one_shot_cap)?;
            plan.retained_bytes = plan.retained_bytes.saturating_add(raster);
            plan.scratch_bytes = plan
                .scratch_bytes
                .saturating_add(self.background_pad_bytes(edit)?);
            return Ok(plan);
        }
        match edit {
            GeometryEdit::AffineSet {
                layers,
                matrix,
                follow_links,
                ..
            } => {
                // The byte no-link offset is `dr_doc_with_layer_offset`; the
                // flag exists only for the typed depth, which has no twin.
                if !follow_links {
                    return Err(
                        "a byte affine set always follows links; dr_doc_with_layer_offset is the byte no-link offset"
                            .into(),
                    );
                }
                self.expanded_geometry_set(layers)?;
                let affine = Affine::from_array(*matrix);
                if !affine.is_finite() || affine.inverse().is_none() {
                    return Err("affine set matrix is nonfinite or singular".into());
                }
            }
            GeometryEdit::PerspectiveSet {
                layers,
                source_rect,
                quad,
                ..
            } => {
                self.expanded_geometry_set(layers)?;
                if !source_rect.iter().chain(quad.iter()).all(|v| v.is_finite())
                    || source_rect[2] <= 0.
                    || source_rect[3] <= 0.
                    || crate::doc_perspective::solve_rect_to_quad(
                        (
                            source_rect[0],
                            source_rect[1],
                            source_rect[2],
                            source_rect[3],
                        ),
                        quad,
                    )
                    .is_none()
                {
                    return Err("perspective set rectangle or quad is invalid".into());
                }
            }
            GeometryEdit::Straighten {
                x,
                y,
                width,
                height,
                ..
            } => {
                // Refused here, before admission, with the typed op's
                // sentences (the window may lie past any edge).
                crate::crop_background::window_fits(*x, *y, *width, *height, self.channels.len())?;
            }
            _ => {}
        }
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        let retained = inventory.unique_bytes();
        let pixels = self
            .layers
            .iter()
            .map(|layer| {
                let (w, h) = layer.pixels.dimensions();
                u64::from(w) * u64::from(h) * 4
                    + layer
                        .mask
                        .as_ref()
                        .map_or(0, |mask| u64::from(mask.width()) * u64::from(mask.height()))
            })
            .sum::<u64>();
        let canvas = u64::from(self.width) * u64::from(self.height);
        // A set can enlarge a tiny source into a very large destination. The
        // old input-only estimate admitted that edit under a small limit and
        // then allocated the enlarged output without accounting for it. Walk
        // the same destination extents as execution before admission.
        let destination = self.geometry_destination_bytes(edit, one_shot_cap)?;
        // A turn charges its measured terms. Saved channels take
        // their own 5/4: each is resampled on the full canvas, then cropped
        // or copied out, so both planes are live (measured, rotate too).
        let scratch = if matches!(
            edit,
            GeometryEdit::Straighten { .. } | GeometryEdit::RotateCanvas { .. }
        ) {
            let input = canvas.saturating_mul(self.channels.len() as u64);
            pixels
                .saturating_mul(CANVAS_TURN_LAYER_FACTOR)
                .saturating_add(
                    (destination.total.saturating_sub(destination.channels))
                        .saturating_mul(CANVAS_TURN_DESTINATION_COPIES),
                )
                .saturating_add(channel_headroom(input.saturating_add(destination.channels)))
                .saturating_add(canvas.saturating_mul(CANVAS_TURN_CANVAS_BYTES))
                .saturating_add(self.cloned_structure_bytes())
        } else {
            // Every selected layer's raster, a re-rendered placed layer's too, is
            // charged at six copies with its destination, beside the largest source
            // render's scratch.
            pixels
                .saturating_mul(6)
                .saturating_add(destination.total.saturating_mul(6))
                .saturating_add(canvas.saturating_mul(8))
        };
        let scratch = scratch
            .saturating_add(destination.source_scratch)
            .saturating_add(4 * 1024 * 1024);
        Ok(SourcePlan {
            retained_bytes: retained,
            scratch_bytes: scratch,
            work_units: pixels
                .saturating_mul(32)
                .saturating_add(destination.source_work),
        })
    }

    fn geometry_destination_bytes(
        &self,
        edit: &GeometryEdit,
        one_shot_cap: u64,
    ) -> Result<DestinationBytes, String> {
        let mut selected = vec![false; self.layers.len()];
        let map = match edit {
            GeometryEdit::AffineSet { layers, matrix, .. } => {
                for index in self.expanded_geometry_set(layers)? {
                    selected[index] = true;
                }
                Some(SourceTransform::Affine(*matrix))
            }
            GeometryEdit::PerspectiveSet {
                layers,
                source_rect,
                quad,
                ..
            } => {
                for index in self.expanded_geometry_set(layers)? {
                    selected[index] = true;
                }
                let rect = (
                    source_rect[0],
                    source_rect[1],
                    source_rect[2],
                    source_rect[3],
                );
                if (quad[0] - quad[2] + quad[4] - quad[6]).abs()
                    <= crate::doc_transform::EXACT_EPSILON
                    && (quad[1] - quad[3] + quad[5] - quad[7]).abs()
                        <= crate::doc_transform::EXACT_EPSILON
                {
                    let affine = parallelogram_affine(rect, quad);
                    Some(SourceTransform::Affine([
                        affine.a, affine.b, affine.c, affine.d, affine.tx, affine.ty,
                    ]))
                } else {
                    let forward = solve_rect_to_quad(rect, quad)
                        .ok_or("perspective set rectangle or quad is invalid")?;
                    Some(SourceTransform::Projective(forward.0))
                }
            }
            GeometryEdit::Affine {
                layer,
                target,
                matrix,
                ..
            } => {
                if *layer >= selected.len() {
                    return Err("geometry layer index is out of range".into());
                }
                selected[*layer] = true;
                if matches!(target, GeometryTarget::Pixels) {
                    Some(SourceTransform::Affine(*matrix))
                } else {
                    None
                }
            }
            GeometryEdit::Perspective {
                layer,
                target,
                quad,
                ..
            } => {
                let entry = self
                    .layers
                    .get(*layer)
                    .ok_or("geometry layer index is out of range")?;
                selected[*layer] = true;
                if matches!(target, GeometryTarget::Pixels) {
                    let rect = (
                        f64::from(entry.offset.0),
                        f64::from(entry.offset.1),
                        f64::from(entry.pixels.width()),
                        f64::from(entry.pixels.height()),
                    );
                    Some(SourceTransform::Projective(
                        solve_rect_to_quad(rect, quad)
                            .ok_or("perspective rectangle or quad is invalid")?
                            .0,
                    ))
                } else {
                    None
                }
            }
            _ => None,
        };
        let mut total = 0_u64;
        let mut channels = 0_u64;
        let mut source_scratch = 0_u64;
        let mut source_work = 0_u64;
        let filter = geometry_filter(edit);
        let one_shot = crate::precision_geometry::one_shot(edit);
        // Only a preview (`interactive`) keeps `MAX_WORK_UNITS`;
        // every other edit takes its caller's cap.
        let work_limit = if edit.interactive() {
            crate::precision_render::MAX_WORK_UNITS
        } else {
            one_shot_cap
        };
        for (index, layer) in self.layers.iter().enumerate() {
            if !selected[index] {
                continue;
            }
            if let Some(map) = map {
                if layer.kind == crate::doc::LayerKind::Raster
                    && !is_adjustment(layer.meta.as_deref())
                {
                    let bounds = if let Some(source) = layer.effective_source(&self.profile) {
                        let accumulated = source_transform::compose(map, source.transform)?;
                        let bounds =
                            source_transform::bounds(source.asset.dimensions(), accumulated)?;
                        // A commit's one-shot work limit (`precision_geometry_budget`).
                        let cost = source_geometry_work(
                            &source,
                            &self.profile,
                            accumulated,
                            (bounds.2, bounds.3),
                            filter,
                            work_limit,
                        )?;
                        source_work = source_work.saturating_add(cost.work_units);
                        if one_shot {
                            // A commit re-renders this layer through a
                            // non-interactive source `transform`
                            // (`transform_layer_unlocked_with_job`), whose own
                            // nested reservation is S'_i = 5/4 · S_i, so the
                            // plan charges the largest S'_i. Its raster and
                            // destination stay in `total`.
                            let scaled = crate::precision_geometry::source_transform_scratch(
                                true,
                                cost.scratch_bytes,
                            )?;
                            source_scratch = source_scratch.max(scaled);
                        } else {
                            source_scratch = source_scratch.max(cost.scratch_bytes);
                        }
                        bounds
                    } else {
                        mapped_bounds(layer.pixels.dimensions(), layer.offset, map)?
                    };
                    total = total
                        .checked_add(plane_bytes(bounds, 4)?)
                        .ok_or("byte geometry destination size overflow")?;
                }
                if let Some(mask) = &layer.mask {
                    if layer.mask_properties.linked {
                        let bounds =
                            mapped_bounds(mask.dimensions(), layer.mask_properties.offset, map)?;
                        total = total
                            .checked_add(plane_bytes(bounds, 1)?)
                            .ok_or("byte geometry destination size overflow")?;
                    }
                }
            } else if let GeometryEdit::Affine {
                target: GeometryTarget::Mask,
                matrix,
                ..
            } = edit
            {
                if let Some(mask) = &layer.mask {
                    let bounds = mapped_bounds(
                        mask.dimensions(),
                        layer.mask_properties.offset,
                        SourceTransform::Affine(*matrix),
                    )?;
                    total = total
                        .checked_add(plane_bytes(bounds, 1)?)
                        .ok_or("byte geometry destination size overflow")?;
                }
            }
        }
        // Whole-document output includes newly allocated canvas channels and
        // potentially expanded rasters. This is a conservative envelope; the
        // set/leaf branch above uses exact solved destinations.
        let (output_w, output_h) = match edit {
            GeometryEdit::Resize { width, height, .. }
            | GeometryEdit::Crop { width, height, .. }
            | GeometryEdit::CanvasSize { width, height, .. }
            | GeometryEdit::Straighten { width, height, .. } => (*width, *height),
            GeometryEdit::Rotate90 | GeometryEdit::Rotate270 => (self.height, self.width),
            GeometryEdit::RotateCanvas { degrees, .. } => {
                let (kind, angle) = crate::doc_rotate::rotation_kind(*degrees)
                    .ok_or("rotation degrees must be finite")?;
                let rotated = crate::doc_rotate::rotated_canvas(self.width, self.height, angle)
                    .ok_or("the rotated canvas exceeds supported bounds")?;
                if kind == crate::doc_rotate::RotationKind::Filtered
                    && !crate::doc_channel::channels_fit_at(
                        rotated.width,
                        rotated.height,
                        self.channels.len(),
                    )
                {
                    return Err(
                        "the rotated canvas would exceed the saved-channel pixel budget".into(),
                    );
                }
                (rotated.width, rotated.height)
            }
            _ => (self.width, self.height),
        };
        if map.is_none()
            && !matches!(
                edit,
                GeometryEdit::Affine { .. } | GeometryEdit::Perspective { .. }
            )
        {
            let source_canvas = u64::from(self.width) * u64::from(self.height);
            let output_canvas = u64::from(output_w) * u64::from(output_h);
            let growth = output_canvas.max(source_canvas);
            total = total.saturating_add(growth.saturating_mul(8));
            channels = output_canvas.saturating_mul(self.channels.len() as u64);
            total = total.saturating_add(channels);
            for layer in &self.layers {
                let bytes = u64::from(layer.pixels.width()) * u64::from(layer.pixels.height()) * 4
                    + layer
                        .mask
                        .as_ref()
                        .map_or(0, |mask| u64::from(mask.width()) * u64::from(mask.height()));
                total = total
                    .saturating_add(bytes.saturating_mul(growth).div_ceil(source_canvas.max(1)));
            }
            let whole_map = whole_filtered_map(edit, self.width, self.height)?;
            if let Some(whole_map) = whole_map {
                for layer in &self.layers {
                    if layer.kind != crate::doc::LayerKind::Raster
                        || is_adjustment(layer.meta.as_deref())
                    {
                        continue;
                    }
                    let bounds = mapped_bounds(layer.pixels.dimensions(), layer.offset, whole_map)?;
                    total = total.saturating_add(plane_bytes(bounds, 4)?);
                    if let Some(source) = layer.effective_source(&self.profile) {
                        let accumulated = source_transform::compose(whole_map, source.transform)?;
                        let source_bounds =
                            source_transform::bounds(source.asset.dimensions(), accumulated)?;
                        total = total.saturating_add(plane_bytes(source_bounds, 4)?);
                        // A commit's one-shot work limit (`precision_geometry_budget`).
                        let cost = source_geometry_work(
                            &source,
                            &self.profile,
                            accumulated,
                            (source_bounds.2, source_bounds.3),
                            filter,
                            work_limit,
                        )?;
                        source_scratch = source_scratch.max(cost.scratch_bytes);
                        source_work = source_work.saturating_add(cost.work_units);
                    }
                }
            }
        }
        Ok(DestinationBytes {
            total,
            channels,
            source_scratch,
            source_work,
        })
    }

    pub fn geometry_edit(
        &self,
        edit: &GeometryEdit,
        limit: u64,
        other_retained: u64,
        control: Option<&crate::job_control::DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control
            .map(crate::job_control::DrJobControl::begin)
            .transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.geometry_edit_with_context(edit, limit, other_retained, job)
    }

    fn geometry_edit_with_context(
        &self,
        edit: &GeometryEdit,
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // The Background pre-pass, admitted before the pad is made.
        let pad = self.background_pad_bytes(edit)?;
        if pad > 0 {
            let mut inventory = crate::resources::Inventory::default();
            inventory.document(self);
            admit(inventory.unique_bytes(), pad, other_retained, limit)?;
            job.reserve(pad)?;
            if let Some(padded) = self.background_padded(edit)? {
                return padded.geometry_edit_with_context(
                    edit,
                    limit,
                    other_retained.saturating_add(pad),
                    job.with_held_bytes(pad),
                );
            }
        }
        let plan = self.geometry_edit_plan_with_cap(
            edit,
            crate::sampling_gaussian::destructive_work_cap(job),
        )?;
        admit(
            plan.retained_bytes,
            plan.scratch_bytes,
            other_retained,
            limit,
        )?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let output = match edit {
            GeometryEdit::AffineSet {
                layers,
                matrix,
                filter,
                ..
            } => {
                if matrix
                    .iter()
                    .zip([1., 0., 0., 1., 0., 0.])
                    .all(|(a, b)| (*a - b).abs() <= crate::doc_transform::EXACT_EPSILON)
                {
                    None
                } else {
                    self.transform_layers_with_job(
                        layers,
                        Affine::from_array(*matrix),
                        crate::doc_geometry_set::filter_type(*filter),
                        job,
                        plan.scratch_bytes,
                    )?
                }
            }
            GeometryEdit::PerspectiveSet {
                layers,
                source_rect,
                quad,
                filter,
                ..
            } => {
                return self.perspective_set(
                    layers,
                    *source_rect,
                    *quad,
                    *filter,
                    job,
                    plan.scratch_bytes,
                );
            }
            GeometryEdit::Affine {
                layer,
                target,
                matrix,
                filter,
                ..
            } => match target {
                GeometryTarget::Pixels => self.transform_layer(
                    *layer,
                    Affine::from_array(*matrix),
                    crate::doc_geometry_set::filter_type(*filter),
                ),
                GeometryTarget::Mask => self.transform_mask(
                    *layer,
                    Affine::from_array(*matrix),
                    crate::doc_geometry_set::filter_type(*filter),
                ),
            },
            GeometryEdit::Perspective {
                layer,
                target,
                quad,
                filter,
                ..
            } => match target {
                GeometryTarget::Pixels => {
                    if self.has_retained_source(*layer) {
                        return Err(
                            "retained source perspective requires the set geometry path".into()
                        );
                    }
                    self.perspective_layer(
                        *layer,
                        *quad,
                        crate::doc_geometry_set::filter_type(*filter),
                    )
                }
                GeometryTarget::Mask => {
                    return Err("byte perspective mask geometry is unavailable".into())
                }
            },
            GeometryEdit::Resize {
                width,
                height,
                filter,
            } => self.resize(
                *width,
                *height,
                crate::doc_geometry_set::filter_type(*filter),
            ),
            GeometryEdit::Crop {
                x,
                y,
                width,
                height,
            } => self.crop(*x, *y, *width, *height),
            GeometryEdit::CanvasSize {
                width,
                height,
                x,
                y,
            } => self.canvas_resize(*width, *height, (*x, *y)),
            GeometryEdit::Straighten {
                matrix,
                filter,
                x,
                y,
                width,
                height,
                ..
            } => {
                let moved = {
                    // The full window `(0, 0, W, H)` is straighten-only: the
                    // crop to the unchanged canvas would refuse as a no-op, so the
                    // straightened document is the answer — and an identity
                    // matrix there moves nothing, which is `None` before any work
                    // rather than an identical copy (purity). An identity matrix
                    // over an inner window is the plain crop with no resample,
                    // the typed rule `an_identity_matrix_over_a_window_is_the_
                    // crop_bit_for_bit` (a retained source's identity edit is
                    // `None`, so this is also what keeps that layer bound).
                    let full = (*x, *y, *width, *height) == (0, 0, self.width, self.height);
                    let identity = matrix
                        .iter()
                        .zip([1., 0., 0., 1., 0., 0.])
                        .all(|(a, b)| (*a - b).abs() <= crate::doc_transform::EXACT_EPSILON);
                    // A window past an edge (`doc_crop_extend`).
                    let inside = *x >= 0
                        && *y >= 0
                        && i64::from(*x) + i64::from(*width) <= i64::from(self.width)
                        && i64::from(*y) + i64::from(*height) <= i64::from(self.height);
                    let (cx, cy) = (x.unsigned_abs(), y.unsigned_abs());
                    // The reference-point hook (`doc_reference`), from `self`:
                    // a turned window is one Straighten, superseding the
                    // inner crop's; the identity past the canvas is a Crop.
                    let turned = crate::doc_reference::windowed(*matrix, *x, *y);
                    let op = crate::reference_point::ReferenceOp::Straighten;
                    if !inside && identity {
                        self.extended_crop(*x, *y, *width, *height, None)
                            .map(|out| {
                                let crop = crate::reference_point::ReferenceOp::Crop;
                                self.following_translation(
                                    out,
                                    crop,
                                    -i64::from(*x),
                                    -i64::from(*y),
                                )
                            })
                    } else if !inside {
                        let window = (*x, *y, *width, *height);
                        self.extended_straighten(*matrix, *filter, window, job, plan.scratch_bytes)?
                            .map(|out| self.following_references(out, op, turned))
                    } else if identity {
                        if full {
                            return Ok(None);
                        }
                        self.crop(cx, cy, *width, *height)
                    } else {
                        let matrix = Affine::from_array(*matrix);
                        let filter = crate::doc_geometry_set::filter_type(*filter);
                        let members = self.layers.len() + self.channels.len();
                        let s = if members == 0 {
                            1.0
                        } else {
                            self.layers.len() as f64 / members as f64
                        };
                        // `None` is a refusal unless no member can be written at
                        // all (mask-less groups and adjustment layers only), when
                        // the layers are simply unchanged.
                        let straightened = match self.straighten_layers_with_job(
                            matrix,
                            filter,
                            job.range(0.0, s),
                            plan.scratch_bytes,
                        )? {
                            Some(doc) => Some(doc),
                            None if self.straighten_writes_nothing() => None,
                            None => {
                                return Err("byte straighten could not transform all layers".into())
                            }
                        };
                        let channels = straightened
                            .as_ref()
                            .unwrap_or(self)
                            .transform_channels_with_job(matrix, filter, job.range(s, 1.0))?;
                        let result = channels.or(straightened);
                        if full {
                            result
                        } else {
                            result
                                .as_ref()
                                .unwrap_or(self)
                                .crop(cx, cy, *width, *height)
                        }
                        .map(|out| self.following_references(out, op, turned))
                    }
                };
                // The samplers follow the content through the matrix and the
                // window (`doc_sampler::edited_samplers`), whatever the
                // layer chain above answered; when no layer or channel could
                // move over the full window, a moved sampler list is still
                // the edit.
                let samplers = crate::doc_sampler::edited_samplers(
                    &self.color_samplers,
                    edit,
                    self.width,
                    self.height,
                )?;
                // The paths follow the same way (`precision_path::edited_paths`).
                let paths = crate::precision_path::edited_paths(
                    &self.paths.entries,
                    edit,
                    self.width,
                    self.height,
                )?;
                // …and the run's unread path records leave it
                // (`tiff_paths::canvas_moved`).
                let metadata = crate::tiff_paths::canvas_moved(&self.metadata);
                match moved {
                    Some(mut doc) => {
                        doc.color_samplers = samplers;
                        doc.paths.set_entries(paths);
                        doc.metadata = metadata;
                        Some(doc)
                    }
                    None if (*x, *y, *width, *height) == (0, 0, self.width, self.height)
                        && (samplers != self.color_samplers
                            || paths != *self.paths.entries
                            || metadata != self.metadata) =>
                    {
                        Some(Self {
                            color_samplers: samplers,
                            paths: self.paths.with_entries(paths),
                            metadata,
                            ..self.clone()
                        })
                    }
                    None => None,
                }
            }
            GeometryEdit::RotateCanvas { degrees, filter } => {
                self.rotate_canvas_with_job(*degrees, *filter, job, plan.scratch_bytes)?
            }
            GeometryEdit::Rotate90 => self.rotate90(),
            GeometryEdit::Rotate180 => self.rotate180(),
            GeometryEdit::Rotate270 => self.rotate270(),
            GeometryEdit::FlipHorizontal => self.flip_horizontal(),
            GeometryEdit::FlipVertical => self.flip_vertical(),
        };
        job.check()?;
        Ok(output)
    }
    pub(crate) fn has_any_effective_source(&self) -> bool {
        self.layers
            .iter()
            .any(|layer| layer.effective_source(&self.profile).is_some())
    }

    pub(crate) fn exact_source_geometry(&self, geometry: Geometry) -> Option<Self> {
        let plain = self.without_source_bindings();
        let mut result = match geometry {
            Geometry::Rotate90 => plain.rotate90()?,
            Geometry::Rotate180 => plain.rotate180()?,
            Geometry::Rotate270 => plain.rotate270()?,
            Geometry::FlipH => plain.flip_horizontal()?,
            Geometry::FlipV => plain.flip_vertical()?,
        };
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let matrix = match geometry {
            Geometry::Rotate90 => [0., 1., -1., 0., h, 0.],
            Geometry::Rotate180 => [-1., 0., 0., -1., w, h],
            Geometry::Rotate270 => [0., -1., 1., 0., 0., w],
            Geometry::FlipH => [-1., 0., 0., 1., w, 0.],
            Geometry::FlipV => [1., 0., 0., -1., 0., h],
        };
        for (index, old) in self.layers.iter().enumerate() {
            let Some(mut source) = old.effective_source(&self.profile) else {
                continue;
            };
            source.transform =
                source_transform::compose(SourceTransform::Affine(matrix), source.transform)
                    .ok()?;
            source.rendered = result.layers[index].pixels.clone();
            source.rendered_offset = result.layers[index].offset;
            result.layers[index].retained_source = Some(source);
        }
        result.native_save_preflight().ok()?;
        Some(result)
    }

    pub(crate) fn resize_source_geometry(
        &self,
        width: u32,
        height: u32,
        filter: FilterType,
    ) -> Option<Self> {
        let plain = self.without_source_bindings();
        let mut result = plain.resize(width, height, filter)?;
        let fx = f64::from(width) / f64::from(self.width);
        let fy = f64::from(height) / f64::from(self.height);
        let map = SourceTransform::Affine([fx, 0., 0., fy, 0., 0.]);
        let sampler = match filter {
            FilterType::Nearest => GeometryFilter::Nearest,
            FilterType::Triangle => GeometryFilter::Bilinear,
            FilterType::CatmullRom => GeometryFilter::Bicubic,
            FilterType::Lanczos3 => GeometryFilter::Lanczos3,
            _ => return None,
        };
        for (index, old) in self.layers.iter().enumerate() {
            let Some(mut source) = old.effective_source(&self.profile) else {
                continue;
            };
            source.transform = source_transform::compose(map, source.transform).ok()?;
            source.filter = sampler;
            source = crate::retained_source_render::render8(
                source,
                &self.profile,
                self.linear_light.as_deref(),
                JobContext::NONE,
            )
            .ok()?;
            result.layers[index].pixels = source.rendered.clone();
            result.layers[index].offset = source.rendered_offset;
            result.layers[index].retained_source = Some(source);
        }
        result.comps = self.remapped_comps(
            |_, rec| Some(crate::doc::scaled_offset(rec, fx, fy)),
            Some(map),
            |_| (fx * fy).sqrt(),
        );
        result.native_save_preflight().ok()?;
        Some(result)
    }

    fn without_source_bindings(&self) -> Self {
        let mut plain = self.clone();
        for layer in &mut plain.layers {
            layer.retained_source = None;
        }
        plain
    }
}

fn mapped_bounds(
    dimensions: (u32, u32),
    offset: (i32, i32),
    map: SourceTransform,
) -> Result<(i32, i32, u32, u32), String> {
    match map {
        SourceTransform::Affine(matrix) => {
            transformed_plane_bounds(dimensions, offset, &Affine::from_array(matrix))
        }
        SourceTransform::Projective(matrix) => {
            transformed_projective_bounds(dimensions, offset, &matrix)
        }
    }
    .ok_or_else(|| "byte geometry destination exceeds supported bounds".into())
}

fn plane_bytes(bounds: (i32, i32, u32, u32), channels: u64) -> Result<u64, String> {
    u64::from(bounds.2)
        .checked_mul(u64::from(bounds.3))
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| "byte geometry destination size overflow".into())
}

fn geometry_filter(edit: &GeometryEdit) -> GeometryFilter {
    match edit {
        GeometryEdit::AffineSet { filter, .. }
        | GeometryEdit::PerspectiveSet { filter, .. }
        | GeometryEdit::Affine { filter, .. }
        | GeometryEdit::Perspective { filter, .. }
        | GeometryEdit::Resize { filter, .. }
        | GeometryEdit::Straighten { filter, .. }
        | GeometryEdit::RotateCanvas { filter, .. } => *filter,
        _ => GeometryFilter::Nearest,
    }
}

fn whole_filtered_map(
    edit: &GeometryEdit,
    width: u32,
    height: u32,
) -> Result<Option<SourceTransform>, String> {
    let map = match edit {
        GeometryEdit::Resize {
            width: next_w,
            height: next_h,
            ..
        } => [
            f64::from(*next_w) / f64::from(width),
            0.,
            0.,
            f64::from(*next_h) / f64::from(height),
            0.,
            0.,
        ],
        GeometryEdit::Straighten { matrix, x, y, .. } => [
            matrix[0],
            matrix[1],
            matrix[2],
            matrix[3],
            matrix[4] - f64::from(*x),
            matrix[5] - f64::from(*y),
        ],
        GeometryEdit::RotateCanvas { degrees, .. } => {
            let (kind, angle) = crate::doc_rotate::rotation_kind(*degrees)
                .ok_or("rotation degrees must be finite")?;
            // An exact turn runs `rotate90`/`rotate180`/`rotate270`
            // verbatim (`rotate_canvas_with_job`) and renders no source, so
            // it charges no source render, plane or work, like
            // `{"op":"rotate90"}`.
            if kind != crate::doc_rotate::RotationKind::Filtered {
                return Ok(None);
            }
            crate::doc_rotate::rotated_canvas(width, height, angle)
                .ok_or("the rotated canvas exceeds supported bounds")?
                .matrix
        }
        _ => return Ok(None),
    };
    Ok(Some(SourceTransform::Affine(map)))
}

fn source_geometry_work(
    source: &RetainedSource8,
    profile: &std::sync::Arc<crate::icc::IccProfile>,
    accumulated: SourceTransform,
    destination: (u32, u32),
    filter: GeometryFilter,
    work_limit: u64,
) -> Result<SourcePlan, String> {
    retained_source_render::work_plan_within(
        work_limit,
        0,
        SourceStage {
            asset: &source.asset,
        },
        destination,
        0,
        SourceSampling::new(source.asset.dimensions(), destination, accumulated, filter),
        8,
        retained_source_render::profile_cost(
            source.asset.profile(),
            profile,
            source.asset.depth() >= 16,
        )?,
    )
}
