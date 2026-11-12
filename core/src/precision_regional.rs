//! The 16-bit and 32-bit float regional compositor (Phase 13A-1, masks, clip
//! units and Blend If since 13A-2a, groups since 13A-3a, layer styles since
//! 13A-3b-i, styled groups with effects — materialized — since 13A-3b-ii,
//! knockout and grouped interiors since 13A-3c):
//! `PrecisionDocument::render_region` and its plan. The contract is
//! `regional`'s module doc, uniform with the byte twin `doc_regional`; this
//! file is the plan and the chunked window loop. The admission walk over the
//! record TREE — every level's units formed exactly as
//! `precision_document::render_layers` forms them, recursing into each
//! contributing group — lives in `precision_regional_walk` (13A-4b-ii-a).
//! The document's own scene is prepared and sampled by
//! `precision_regional_tree`, which holds no admission logic; none of these
//! files holds pixel math of its own, and the store is the full render's
//! unseeded pair (`from_float_samples_unseeded` /
//! `from_rendered_float_unseeded`, 13B-3c-2b): the window is display output
//! read only through tile reads, so it seeds no view.

use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, pixel_count, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_regional_guide::{render_staged, stage_nested};
use crate::precision_regional_tree::{prepare, sample_layers};
use crate::precision_regional_walk::Walk;
use crate::precision_render::{zero_plane, Progress};
use crate::precision_style_knockout::{
    background_record, document_wants_background16, KnockoutSeed,
};
use crate::precision_style_render::StyleEnv;
use crate::regional::{RegionRequest, RegionalPlan};
use crate::regional_guide::{input_halo, plan_levels, Enclosure, Last};
use crate::style_composite::Cut;
use crate::style_knockout::KnockoutBackdrop;

/// Samples per cancellation checkpoint and progress step, the full render's
/// own chunk (`render_layers` advances every 4096 pixels).
const CHUNK: usize = crate::regional_plan::CHECKPOINT_PIXELS;

impl PrecisionDocument {
    /// The admitted plan for `request`, or the refusal the render would raise:
    /// the evaluation policy's walk FIRST (`transfer`, the call `scene_for`
    /// makes, which is also the float refusal), then the fixed-order walk
    /// over units. Allocates no pixels.
    pub fn render_region_plan(&self, request: RegionRequest) -> Result<RegionalPlan, String> {
        if self.is_cmyk() {
            return self.cmyk_render_region_plan(request);
        }
        self.transfer()?;
        let mut walk = Walk {
            canvas: (self.width, self.height),
            ..Walk::default()
        };
        let record = walk.level(&self.layers, 0, None, 0, None, Enclosure::Root)?;
        let output = request.output_dims((self.width, self.height))?;
        let pixels = u64::from(output.0) * u64::from(output.1);
        let storage = self.render_storage();
        let output_bytes = pixels * storage.bytes_per_pixel();
        // 13A-4a / 13A-4b-i: the staged levels (`regional_guide::
        // plan_levels`), D5 decided as the scene decides it; the input halo
        // composes each row's own with the guides above it.
        let background = document_wants_background16(&self.layers, 0, Cut::NONE)
            && background_record(
                &self.layers,
                (self.width, self.height),
                Cut::NONE,
                self.float.is_some(),
                self.background,
            );
        let (levels, stage_scratch, stage_work) =
            plan_levels(&record, request.region(), &walk.needs, pixels, background)?;
        // Every staged level fills its `P_0` with `zero_plane`, which
        // advances it.
        let fill = levels
            .iter()
            .filter_map(|level| level.stages.first())
            .map(|s| u64::from(s.plane.width) * u64::from(s.plane.height))
            .fold(0u64, u64::saturating_add);
        let halo_pixels = input_halo(&walk.needs, &levels);
        Ok(RegionalPlan {
            request,
            depth: if storage == PrecisionStorage::Rgba32Float {
                "rgba32f"
            } else {
                "rgba16"
            },
            reference: "dr_precision_doc_render",
            output,
            output_bytes,
            // The f32 accumulator, the stored output, the fixed allowance,
            // EVERY contributing record's prepared mask, every styled
            // raster's planes and every materialized group (its buffer, its
            // nest and its planes) — all are prepared before the loop and
            // held together.
            scratch_bytes: (pixels * 16 + output_bytes + BOOKKEEPING_BYTES)
                .saturating_add(walk.mask_scratch)
                .saturating_add(walk.style_scratch)
                .saturating_add(walk.materialization_scratch)
                .saturating_add(stage_scratch),
            // The accumulator fill, one pass per contributing layer over its
            // stage's plane (the output without a guide; group rows
            // included), every guide and `P_0`'s fill, the mask preparation
            // `prepare_mask` advances, the plane rendering `prepare` advances
            // and every materialization's `render_layers` and planes.
            work_units: stage_work
                .saturating_add(pixels)
                .saturating_add(fill)
                .saturating_add(walk.mask_work)
                .saturating_add(walk.style_work)
                .saturating_add(walk.materialization_work),
            max_output_bytes: None,
            layers: walk.needs,
            halo_pixels,
            canvas: (self.width, self.height),
            group_depth: walk.group_depth,
            global: walk.global,
            levels,
        })
    }

    /// The region (or preview) of the projection `dr_precision_doc_render`
    /// returns, sample for sample, in the document's render storage. Admitted
    /// against `limit` with `other_retained` exactly as the full render is;
    /// cancellable every 4096 samples.
    pub fn render_region(
        &self,
        request: RegionRequest,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionImage, String> {
        // A CMYK document's window is its own view (`cmyk_render`).
        if self.is_cmyk() {
            return self.cmyk_render_region(request, limit, other_retained, control);
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let plan = self.render_region_plan(request)?;
        admit(0, BOOKKEEPING_BYTES, other_retained, limit)?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let (scene, _) = self.scene()?;
        let storage = self.render_storage();
        admit(
            self.retained_bytes(),
            plan.scratch_bytes,
            other_retained,
            limit,
        )?;
        job.reserve(plan.scratch_bytes)?;
        let (ow, oh) = plan.output;
        let count = pixel_count(ow, oh)?;
        let width = ow as usize;
        let mut progress = Progress {
            job,
            completed: 0,
            total: plan.work_units,
        };
        // 13A-4a: a guided root row takes the STAGED loop, which fills the
        // output itself (`precision_regional_guide`).
        let staged = plan.root().is_some();
        let mut acc = if staged {
            Vec::new()
        } else {
            zero_plane(count, &mut progress)?
        };
        let colors = StyleEnv::colors_for(&scene.profile);
        let float_style = StyleEnv::float_style_for(&scene.profile, scene.float)?;
        let env = scene.style_env(colors.as_ref(), float_style.as_ref(), None);
        // The scene prepared ONCE (every mask the loop reads, every styled
        // raster's planes, every materialized group's buffer and planes),
        // then sampled
        // SAMPLE-MAJOR: each root sample evaluates the whole tree —
        // `render_layers` one sample at a time — and progress advances per
        // 4096 root samples, the checkpoint that bounds a cancel.
        let mut nodes = prepare(&scene.layers, &env, &mut progress)?;
        // D5 (`precision_style_knockout::render_projection`): the scene's own
        // seed says whether record 0 is the Background; then the first node
        // samples with no backdrop and every later one against the sample
        // right after it, as BOTH planes (Shallow at the root is Deep).
        let background = matches!(
            scene.knockout_seed,
            KnockoutSeed::Projection { background: true }
        ) && !nodes.is_empty();
        let rows = plan.layers.len();
        // 13A-4b-i: every nested staged level's plane, before the root loop,
        // matched to the plan's pre-order levels by popping them in order.
        let mut pending = plan.nested();
        stage_nested(
            &mut nodes,
            &plan,
            request.region(),
            plan.root(),
            &mut 0,
            &mut pending,
            &env,
            &mut progress,
        )?;
        if pending.next().is_some() {
            return Err("regional stage mismatch".into());
        }
        if let Some(root) = plan.root() {
            acc = render_staged(
                &mut nodes,
                background,
                root,
                Last::Output(request),
                &plan,
                &env,
                &mut progress,
            )?;
        } else {
            for start in (0..count).step_by(CHUNK) {
                let end = (start + CHUNK).min(count);
                for ai in start..end {
                    let xy = request.sample((ai % width) as u32, (ai / width) as u32);
                    if background {
                        sample_layers(&nodes[..1], &mut acc, ai, &env, xy)?;
                        let snap = [acc[ai]];
                        let seeded = StyleEnv {
                            knockout: KnockoutBackdrop {
                                shallow: Some(&snap),
                                deep: Some(&snap),
                            },
                            ..env
                        };
                        sample_layers(&nodes[1..], &mut acc, ai, &seeded, xy)?;
                    } else {
                        sample_layers(&nodes, &mut acc, ai, &env, xy)?;
                    }
                }
                progress.advance((end - start) * rows)?;
            }
        }
        drop(nodes);
        // The full render's ONE re-encode of a linear-light accumulator,
        // verbatim (`PrecisionScene::render_with_context`).
        if let Some(transfer) = &scene.linear_light {
            for pixel in acc.iter_mut() {
                let encoded = transfer.encode([pixel[0], pixel[1], pixel[2]]);
                pixel[..3].copy_from_slice(&encoded);
            }
            progress.job.check()?;
        }
        let output = if storage == PrecisionStorage::Rgba32Float {
            PrecisionImage::from_rendered_float_unseeded(
                ow,
                oh,
                scene.profile.clone(),
                acc,
                scene.float,
                job,
            )?
        } else {
            PrecisionImage::from_float_samples_unseeded(
                ow,
                oh,
                scene.profile.clone(),
                storage,
                acc.into_iter(),
                job,
            )?
        };
        job.report(1.0, 5)?;
        Ok(output)
    }
}
