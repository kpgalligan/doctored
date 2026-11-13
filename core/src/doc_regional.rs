//! The 8-bit regional compositor (Phase 13A-1, masks and clip units since
//! 13A-2a, effect-less styles since 13A-2b, groups since 13A-3a, raster
//! styles with effects since 13A-3b-i, styled groups with effects —
//! materialized — since 13A-3b-ii, knockout and grouped interiors since
//! 13A-3c):
//! `DrDocument::render_region` and its plan. The contract — admitted tree,
//! units, refusal order, sampling, evaluation needs — is `regional`'s module
//! doc; this file is the plan and the window loop (run-major within a row
//! since 13A-latency: `doc_regional_run`). The
//! admission walk over the bottom-first stack's LEVELS (recursing into each
//! contributing group, as `doc_group::composite_level_into` does) lives in
//! `doc_regional_walk` (13A-4b-ii-a). The tree it builds and every per-sample
//! statement live in `doc_regional_tree`, which holds no admission logic;
//! none of these files holds pixel math of its own. The window is quantized
//! by `doc_evaluation::encode_projection`, the full projection's own
//! quantizer. Since 13A-5c-1 the admission's description preparation
//! (`validate_context_with_job`) runs under the render's job with its parse
//! scratch reserved and planned — since 13A-5c-3
//! `adjust_calibrated::parse_scratch` under every profile, since the walk
//! parses each adjustment node's description whatever the profile. Since
//! 13A-5c-2b the admission takes a `Cut` (`Cut::NONE` for every regional
//! render) so the retouch sampling plate (`doc_regional_plate`) resolves the
//! same tree over a cut stack. Since 13A-5c-3 each materialization's term
//! carries the reference's uncharged per-unit buffers inside the nest.

use image::RgbaImage;

use crate::adjust_calibrated::{parse_scratch, validate_context_with_job};
use crate::doc::DrDocument;
use crate::doc_evaluation::ByteTransfer;
use crate::doc_group::{self, RenderBudget, MAX_GROUP_BUFFER_PIXELS};
use crate::doc_regional_budget::materialization_charges;
use crate::doc_regional_guide::{render_staged, stage_nested};
use crate::doc_regional_progress::Run;
use crate::doc_regional_run::SampleRun;
use crate::doc_regional_tree::Level;
use crate::doc_regional_walk::{renders_planes, style_of, Walk};
use crate::job_control::{DrJobControl, JobContext, EVERY_INTERVAL};
use crate::precision_style_render::style_scratch;
use crate::regional::{RegionRequest, RegionalPlan};
use crate::regional_guide::{input_halo, plan_levels, Enclosure, Last};
use crate::regional_plan::area;
use crate::style_composite::{CompositeEnv, Cut};
use crate::style_knockout::{background_entry, document_wants_background, KnockoutBackdrop};

/// The shared admission's result (13A-5c-2b): the plan, the tree it walked,
/// the transfer the render composites under and the preparation's scratch.
pub(crate) struct Admission<'a> {
    pub(crate) plan: RegionalPlan,
    pub(crate) level: Level<'a>,
    pub(crate) transfer: Option<&'a ByteTransfer>,
    /// walk.mask_scratch + walk.style_scratch + walk.materialization_scratch + context:
    /// every term of `plan.scratch_bytes` except the accumulator, the output and the stages.
    pub(crate) preparation_scratch: u64,
}

impl DrDocument {
    /// The admitted plan for `request`, or the refusal the render would raise.
    /// `max_output_bytes` bounds the RGBA8 output (Phase 4's wording refuses a
    /// larger one). Allocates no pixels.
    pub fn render_region_plan(
        &self,
        request: RegionRequest,
        max_output_bytes: u64,
    ) -> Result<RegionalPlan, String> {
        self.render_region_plan_with_group_budget(
            request,
            max_output_bytes,
            MAX_GROUP_BUFFER_PIXELS,
        )
    }

    /// The same plan with a reduced group-buffer pixel budget, for capacity
    /// qualification without gigabyte fixtures (`flattened_with_group_budget`'s
    /// twin). Cannot raise the production cap.
    #[doc(hidden)]
    pub fn render_region_plan_with_group_budget(
        &self,
        request: RegionRequest,
        max_output_bytes: u64,
        group_pixels: u64,
    ) -> Result<RegionalPlan, String> {
        self.regional_admission(
            request,
            max_output_bytes,
            group_pixels,
            Cut::NONE,
            0,
            JobContext::NONE,
        )
        .map(|admission| admission.plan)
    }

    /// The region (or preview) of the projection `dr_doc_flattened` returns,
    /// sample for sample. `control` makes it cancellable (a checkpoint every
    /// 4096 samples of every loop and fill, and — 13A-5a — inside each
    /// mask's feather blur and the byte guide's blur, 13A-5b-i inside each
    /// style plane render and 13A-5b-ii-a inside each materialized group's
    /// `rendered_group`) and reports progress against the plan's
    /// `work_units`: the guide's blur inside its range since 13A-5b-iii-a,
    /// and since 13A-5b-iii-b-2 the unstaged accumulator fill per chunk and
    /// every mask blur, style plane render and materialization inside its own
    /// range (stage 1, "preparing"; inner blurs report stage 6); since
    /// 13A-5c-1 the description preparation polls before and after each
    /// layer's parse and the walk once per node; cancellation drops every
    /// buffer and caches no partial mask or plane.
    pub fn render_region(
        &self,
        request: RegionRequest,
        max_output_bytes: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        self.render_region_with_group_budget(
            request,
            max_output_bytes,
            MAX_GROUP_BUFFER_PIXELS,
            control,
        )
    }

    /// The same render with a reduced group-buffer pixel budget
    /// (`flattened_with_group_budget`'s twin). Cannot raise the production
    /// cap.
    #[doc(hidden)]
    pub fn render_region_with_group_budget(
        &self,
        request: RegionRequest,
        max_output_bytes: u64,
        group_pixels: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let Admission {
            plan,
            level,
            transfer,
            ..
        } = self.regional_admission(request, max_output_bytes, group_pixels, Cut::NONE, 0, job)?;
        job.reserve(plan.scratch_bytes)?;
        let (ow, oh) = plan.output;
        let count = usize::try_from(u64::from(ow) * u64::from(oh))
            .map_err(|_| "render output size exceeds addressable memory".to_string())?;
        // `done`: work units completed, shared by the preparation, the nested
        // stages and the root loop so the reported fraction stays monotonic;
        // `total`: the plan's work in both branches (13A-5b-iii-b-2).
        let mut done = 0f64;
        let total = plan.work_units.max(1) as f64;
        if let Some(staged) = plan.root() {
            // 13A-4a: a guided root row — the STAGED loop over the planes the
            // plan declares (`doc_regional_guide`), then the same quantizer;
            // the nested staged levels (13A-4b-i) first.
            let colors = self.style_colors();
            let env = self.composite_env(colors.as_ref(), transfer);
            let clip = (0, 0, i64::from(self.width), i64::from(self.height));
            let budget = RenderBudget::new(group_pixels);
            let mut root = {
                let mut run = Run {
                    sample: 0,
                    done: &mut done,
                    total,
                };
                level.resolve(&self.layers, env, clip, &budget, job, &mut run)?
            };
            stage_nested(
                &mut root,
                &plan,
                request.region(),
                Some(staged),
                &mut 0,
                &mut done,
                env.transfer,
                job,
            )?;
            let acc = render_staged(
                &mut root,
                level.background,
                staged,
                Last::Output(request),
                &plan,
                &mut done,
                env.transfer,
                job,
            )?;
            job.check()?;
            let out = crate::doc_evaluation::encode_projection(&acc, ow, oh, transfer);
            job.report(1.0, 5)?;
            return Ok(out);
        }
        let mut acc: Vec<[f32; 4]> = Vec::new();
        acc.try_reserve_exact(count)
            .map_err(|_| "could not allocate regional render workspace".to_string())?;
        let colors = self.style_colors();
        let env = self.composite_env(colors.as_ref(), transfer);
        let clip = (0, 0, i64::from(self.width), i64::from(self.height));
        // Every materialized group is rendered here by `rendered_group`
        // itself, charged against the render's own budget exactly as the
        // reference charges it (the plan's dry run has already refused a
        // render that would exceed it).
        let budget = RenderBudget::new(group_pixels);
        let mut root = {
            let mut run = Run {
                sample: 0,
                done: &mut done,
                total,
            };
            // 13A-5b-iii-b-2: the fill advances and reports per 4096-sample
            // chunk (`pixels` in the plan's work), stage 1.
            while acc.len() < count {
                job.every(acc.len())?;
                let added = (count - acc.len()).min(4096);
                acc.extend(std::iter::repeat_n([0.0f32; 4], added));
                run.advance(job, added as u64)?;
            }
            level.resolve(&self.layers, env, clip, &budget, job, &mut run)?
        };
        // 13A-4b-i: every nested staged level's planes, before the loop.
        stage_nested(
            &mut root,
            &plan,
            request.region(),
            None,
            &mut 0,
            &mut done,
            env.transfer,
            job,
        )?;
        let rows = plan.layers.len() as f64;
        let mut sample = 0usize;
        // RUN-MAJOR within a row (13A-latency, `doc_regional_run`): each row
        // is cut into runs at the multiples of 4096 of the root sample
        // counter, and `job.every` is called at each run's first sample, so
        // the checkpoints fire at the same root samples, as many times, as a
        // per-sample loop's — a cancel still returns within 4096 ROOT samples
        // through the whole tree. With D5's Background the root is sampled
        // per sample (its later units read a per-sample snapshot). Index
        // arithmetic only; the bodies are the reference's.
        if !plan.layers.is_empty() {
            let (ow_us, d) = (ow as usize, i64::from(request.preview_divisor));
            for j in 0..oh {
                let mut i = 0usize;
                while i < ow_us {
                    job.every(sample)?;
                    let count = (ow_us - i).min(EVERY_INTERVAL - sample % EVERY_INTERVAL);
                    let first = j as usize * ow_us + i;
                    let origin = request.sample(i as u32, j);
                    if root.background {
                        for k in 0..count {
                            root.sample(
                                &mut acc,
                                first + k,
                                env.transfer,
                                (origin.0 + k as i64 * d, origin.1),
                                KnockoutBackdrop::NONE,
                            );
                        }
                    } else {
                        let run = SampleRun {
                            first,
                            count,
                            origin,
                            step: d,
                        };
                        root.sample_run(&mut acc, run, env.transfer);
                    }
                    sample += count;
                    i += count;
                }
                done += rows * f64::from(ow);
                job.report(done / total, 6)?;
            }
        }
        job.check()?;
        let out = crate::doc_evaluation::encode_projection(&acc, ow, oh, transfer);
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The shared admission: the projection's own preflight
    /// (`validate_context`, then the evaluation policy's refusal walk, whose
    /// transfer is returned for the render), the fixed-order walk over the
    /// tree of levels (`regional`'s module doc), the dry run of every
    /// materialized group's budget against `group_pixels`, the request and
    /// the budget. 13A-5c-1: the description preparation's parse scratch
    /// (13A-5c-3: `parse_scratch` under every profile, one parse at a time
    /// covering both the preflight's and the walk's `Adjustment::from_meta`)
    /// is forecast and reserved on `job` before `validate_context_with_job`
    /// parses under it, and the walk polls `job`
    /// once per node; the plan passes `JobContext::NONE`, the render its job
    /// (whose later `reserve(plan.scratch_bytes)` replaces this one).
    /// 13A-5c-2b: `cut` gates the walk, the dry run and D5's Background as
    /// the reference's `env.cut` does (`Cut::NONE` for every regional
    /// render), and `reserved` — the caller's live working set — is reserved
    /// beside the parse scratch. 13A-5c-3: each materialization's term also
    /// carries the reference's UNCHARGED per-unit buffers inside the nest
    /// (`materialization_charges`' third figure).
    pub(crate) fn regional_admission<'a>(
        &'a self,
        request: RegionRequest,
        max_output_bytes: u64,
        group_pixels: u64,
        cut: Cut<'a>,
        reserved: u64,
        job: JobContext<'a>,
    ) -> Result<Admission<'a>, String> {
        let context =
            parse_scratch(&self.layers).ok_or("layer metadata exceeds the native slot limit")?;
        job.reserve(reserved.saturating_add(context))?;
        validate_context_with_job(&self.layers, &self.profile, job)?;
        let transfer = self.transfer()?;
        let mut walk = Walk {
            doc: self,
            env: CompositeEnv {
                job,
                cut,
                ..self.composite_env(None, None)
            },
            needs: Vec::new(),
            mask_scratch: 0,
            style_scratch: 0,
            materialization_scratch: 0,
            materializations: Vec::new(),
            group_depth: 0,
            global: Vec::new(),
            mask_work: 0,
            style_work: 0,
            materialization_work: 0,
            inside_prep: 0,
        };
        let canvas = (0, 0, i64::from(self.width), i64::from(self.height));
        let (mut level, record) = walk.level(
            0..self.layers.len(),
            0,
            None,
            0,
            Some(canvas),
            true,
            Enclosure::Root,
        )?;
        // D5: `composite_projection_into`'s test verbatim — some visible
        // style asks for a knockout, entry 0 qualifies as the Background and
        // the root level's first unit is entry 0 alone.
        level.background = document_wants_background(&self.layers)
            && background_entry(
                &self.layers,
                (self.width, self.height),
                cut,
                self.background,
            )
            .is_some()
            && doc_group::top_level(&self.layers)
                .first()
                .is_some_and(|u| u.idx == 0 && u.start == 0);
        // The walk is complete, so every structure refusal has been raised;
        // now the reference's own budget and nesting decisions for each
        // materialized group, in render order, against one budget (each
        // releases before the next, as `rendered_group` does).
        let budget = RenderBudget::new(group_pixels);
        for &(at, clip, depth) in &walk.materializations {
            let Some((rect, peak, uncharged)) =
                materialization_charges(&self.layers, at, walk.env, clip, depth, &budget)?
            else {
                continue;
            };
            let entry = &self.layers[at.idx];
            let pixels = ((rect.2 - rect.0) as u64) * ((rect.3 - rect.1) as u64);
            // The f32 buffers of the nest at its peak, the synthetic's RGBA8,
            // its cropped mask and — only when the reference renders them —
            // its planes (`style_scratch` refuses past `MAX_PIXELS`, where
            // `rendered` renders no effect), and (13A-5c-3) the buffers the
            // reference allocates inside the nest without a budget charge.
            let planes = match style_of(entry) {
                Some(style) if renders_planes(entry, style, Some(rect)) => {
                    let dims = ((rect.2 - rect.0) as u32, (rect.3 - rect.1) as u32);
                    style_scratch(style, dims, false, None)?.0
                }
                _ => 0,
            };
            let mask = u64::from(entry.mask_enabled && entry.mask.is_some()) * pixels;
            walk.materialization_scratch = walk.materialization_scratch.saturating_add(
                peak.saturating_mul(16)
                    .saturating_add(pixels * 4)
                    .saturating_add(mask)
                    .saturating_add(planes)
                    .saturating_add(uncharged),
            );
        }
        let output = request.output_dims((self.width, self.height))?;
        let pixels = u64::from(output.0) * u64::from(output.1);
        let output_bytes = pixels * 4;
        if output_bytes > max_output_bytes {
            return Err(format!(
                "render output requires {output_bytes} bytes; budget is {max_output_bytes} bytes"
            ));
        }
        // 13A-4a / 13A-4b-i: the staged levels (the root's and every
        // isolated group's holding a guided row), their planes' and guides'
        // scratch and the staged work (`regional_guide::plan_levels`); the
        // input halo composes each row's own with the guides above it.
        let (levels, stage_scratch, stage_work) = plan_levels(
            &record,
            request.region(),
            &walk.needs,
            pixels,
            level.background,
        )?;
        let halo_pixels = input_halo(&walk.needs, &levels);
        // 13A-5b-iii-a: every staged level's stage-0 `prev` fill over `R_0`,
        // and the root's D5 `snapshot` fill beside it (the typed `fill`'s
        // twin), each reported by `render_staged`.
        let root_snapshot = level.background && levels.first().is_some_and(|l| l.level.is_none());
        let fill = levels
            .iter()
            .map(|l| l.stages.first().map_or(0, |s| area(s.plane)))
            .fold(0u64, u64::saturating_add)
            .saturating_add(if root_snapshot {
                levels[0].stages.first().map_or(0, |s| area(s.plane))
            } else {
                0
            });
        let preparation_scratch = walk
            .mask_scratch
            .saturating_add(walk.style_scratch)
            .saturating_add(walk.materialization_scratch)
            .saturating_add(context);
        let plan = RegionalPlan {
            request,
            depth: "rgba8",
            reference: "dr_doc_flattened",
            output,
            output_bytes,
            // The f32 accumulator, the RGBA8 output it quantizes into, every
            // contributing feathered mask's derived plane (groups included),
            // every styled raster's planes (`style_scratch`) and every
            // materialized group's nest, synthetic, mask crop, planes and
            // uncharged per-unit buffers (13A-5c-3), and every staged plane
            // and guide (13A-4a; nested levels' held planes, 13A-4b-i), and
            // one description parse (`parse_scratch` under every profile,
            // 13A-5c-1 / 13A-5c-3). No other per-level buffer exists
            // (`regional`'s module doc).
            scratch_bytes: (pixels * 16 + output_bytes)
                .saturating_add(preparation_scratch)
                .saturating_add(stage_scratch),
            // The accumulator fill plus one pass per contributing layer over
            // its stage's plane (the output without a guide), every group row
            // included, every guide and every staged level's stage-0 fills
            // (13A-5b-iii-a), and — 13A-5b-iii-b-2, the typed twin's terms —
            // every resolved mask blur (`MaskProperties::work_units`), styled
            // raster's or materialized group's planes (`style_scratch(..).1`)
            // and materialization (`|rect|·(1 + rows inside)` plus the
            // subtree's mask/plane work and the group's own mask blur).
            work_units: stage_work
                .saturating_add(pixels)
                .saturating_add(fill)
                .saturating_add(walk.mask_work)
                .saturating_add(walk.style_work)
                .saturating_add(walk.materialization_work),
            max_output_bytes: Some(max_output_bytes),
            layers: walk.needs,
            halo_pixels,
            canvas: (self.width, self.height),
            group_depth: walk.group_depth,
            global: walk.global,
            levels,
        };
        Ok(Admission {
            plan,
            level,
            transfer,
            preparation_scratch,
        })
    }
}
