//! Named analysis sources reuse each compositor's own contribution semantics.
//! Raw leaves are read without a canvas copy. Partial backdrops retain enclosing
//! groups; a cut never changes clipping/group isolation decisions at either depth.
//! A supplied projection answers the composite scope without rendering again,
//! at 16 bits and at 8 bits alike, over one shared plan. A
//! float document's sources (and a float projection) answer through the
//! same plan, admission and source walk, counted by `float_stats`.
//! A float document's `sample` reply adds `center_display_codes`, the centre's
//! R1 display code (`display_encoding`); a bare projection carries no
//! policy, so its answer is the document's without that key (the host's Color
//! Range eyedropper asks the document at float).
//! A backdrop request with `auto` adds `scoped_auto`'s answer, its
//! weights (`auto_weights`, per depth) resolved before the backdrop render.
//!
//! Typed sources dispatch on storage in `evaluate_precision`: u16 through
//! `scoped_statistics::evaluate`, f32 through `float_stats::evaluate`, which shares `envelope`,
//! `weight`, `validate_coverage` and `EMPTY_SAMPLE`; the document plan charges the render at
//! `render_storage().bytes_per_pixel()` (16 at float).

use crate::doc::{DrDocument, LayerKind};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES};
use crate::precision_structure_ops::flatten;
use crate::scoped_auto::{self, AutoAnalysis};
use crate::scoped_statistics::{self, AnalysisRequest, AnalysisSource, STATISTICS_SCRATCH};
use image::RgbaImage;
use serde_json::{json, Value};

fn fields(result: &mut Value, profile: &crate::icc::IccProfile) {
    result["profile_name"] = json!(profile.name().chars().take(256).collect::<String>());
    result["profile_bytes"] = json!(profile.bytes().len());
}

fn mapped_xy(
    i: usize,
    width: u32,
    dimensions: (u32, u32),
    offset: (i32, i32),
) -> Option<(u32, u32)> {
    let x = (i % width as usize) as i64 - i64::from(offset.0);
    let y = (i / width as usize) as i64 - i64::from(offset.1);
    if x < 0 || y < 0 || x >= i64::from(dimensions.0) || y >= i64::from(dimensions.1) {
        None
    } else {
        Some((x as u32, y as u32))
    }
}

/// One typed source counted in canvas coordinates, dispatching on storage:
/// u16 codes through `scoped_statistics::evaluate`, f32 linear samples
/// through `float_stats::evaluate`. Positions outside the source read
/// as transparent zero at either storage.
fn evaluate_precision(
    request: &AnalysisRequest,
    canvas: (u32, u32),
    image: &PrecisionImage,
    offset: (i32, i32),
    coverage: &[u16],
    job: JobContext<'_>,
) -> Result<Value, String> {
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => scoped_statistics::evaluate(
            request,
            canvas,
            u16::MAX,
            coverage,
            |i| {
                mapped_xy(i, canvas.0, image.dimensions(), offset)
                    .and_then(|(x, y)| image.rgba16_at(x, y))
                    .unwrap_or([0; 4])
            },
            job,
        ),
        PrecisionStorage::Rgba32Float => crate::float_stats::evaluate(
            request,
            canvas,
            coverage,
            |i| {
                mapped_xy(i, canvas.0, image.dimensions(), offset)
                    .and_then(|(x, y)| image.sample_at(x, y))
                    .unwrap_or([0.0; 4])
            },
            job,
        ),
    }
}

/// A float `sample` reply's `center_display_codes`: the centre
/// pixel's R1 display code (`display_encoding::display_code`), the
/// numbers the Color Range sheet's eyedropper and the smart selections
/// compare; `null` off the canvas. Nothing for a reply with no centre (a
/// histogram).
fn center_display_codes(result: &mut Value, policy: &crate::float_document::FloatPolicy) {
    let Some(center) = result.get("center_rgba32") else {
        return;
    };
    let codes = serde_json::from_value::<[f32; 4]>(center.clone())
        .ok()
        .map(|pixel| crate::display_encoding::display_code(policy.encoder(), pixel));
    result["center_display_codes"] = json!(codes);
}

/// Pixel `i` of a canvas-sized straight RGBA8 image, codes widened.
fn byte_codes(image: &RgbaImage, i: usize) -> [u16; 4] {
    let p = &image.as_raw()[i * 4..i * 4 + 4];
    [
        u16::from(p[0]),
        u16::from(p[1]),
        u16::from(p[2]),
        u16::from(p[3]),
    ]
}

/// The byte twin of [`evaluate_precision`] over a canvas-sized straight RGBA8
/// image (a render or a supplied projection): codes widened, never scaled.
fn evaluate_rgba8(
    request: &AnalysisRequest,
    canvas: (u32, u32),
    image: &RgbaImage,
    coverage: &[u16],
    job: JobContext<'_>,
) -> Result<Value, String> {
    scoped_statistics::evaluate(
        request,
        canvas,
        255,
        coverage,
        |i| byte_codes(image, i),
        job,
    )
}

/// The backdrop `auto` of a request, if any (`parse` admits it only there).
fn backdrop_auto(request: &AnalysisRequest) -> Option<(&AutoAnalysis, usize)> {
    match (&request.auto, request.source, request.below) {
        (Some(auto), AnalysisSource::Backdrop, Some(below)) => Some((auto, below)),
        _ => None,
    }
}

/// The plan of a composite-only analysis over a supplied (retained)
/// projection, at either depth: no render, so the statistics scratch plus
/// bookkeeping; every other source refuses before admission.
fn projection_plan(request: &AnalysisRequest) -> Result<u64, String> {
    if !matches!(request.source, AnalysisSource::Composite) {
        return Err("projection analysis takes the composite source only".into());
    }
    Ok(STATISTICS_SCRATCH + BOOKKEEPING_BYTES)
}

impl PrecisionImage {
    /// Composite-only analysis over a supplied (retained) projection: no
    /// render, so the plan is the statistics scratch plus bookkeeping.
    pub fn scoped_analysis_plan(&self, request: &AnalysisRequest) -> Result<u64, String> {
        projection_plan(request)
    }

    pub fn scoped_analysis(
        &self,
        request: &AnalysisRequest,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (width, height) = self.dimensions();
        scoped_statistics::validate_coverage(coverage, width, height)?;
        let scratch = self.scoped_analysis_plan(request)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let mut result =
            evaluate_precision(request, self.dimensions(), self, (0, 0), coverage, job)?;
        fields(&mut result, self.profile());
        Ok(result)
    }
}

impl PrecisionDocument {
    pub fn scoped_analysis_plan(&self, request: &AnalysisRequest) -> Result<u64, String> {
        let render = match request.source {
            AnalysisSource::Composite => true,
            AnalysisSource::Layer => {
                let layer = self
                    .layer(request.layer.ok_or("missing analysis layer")?)
                    .ok_or("analysis layer is out of range")?;
                if matches!(layer.content, Content::Adjustment(_)) {
                    return Err("an adjustment has no raw pixels; inspect its backdrop".into());
                }
                layer.source().is_none()
            }
            AnalysisSource::Backdrop => {
                if request.below.ok_or("missing backdrop boundary")? > self.records {
                    return Err("backdrop boundary is out of range".into());
                }
                true
            }
        };
        let mut plan = STATISTICS_SCRATCH
            + BOOKKEEPING_BYTES
            + if render {
                self.render_plan()?.scratch_bytes
                    + u64::from(self.width)
                        * u64::from(self.height)
                        * self.render_storage().bytes_per_pixel()
            } else {
                0
            };
        if let Some((auto, below)) = backdrop_auto(request) {
            if self.render_storage() == PrecisionStorage::Rgba32Float {
                return Err(crate::float_document::auto_refusal());
            }
            // A Lab Color document (#547 phase 2f): Auto Color refuses;
            // Auto Tone acts on Lightness (`scoped_auto::derive`).
            if self.is_lab() && auto.mode == scoped_auto::AutoAnalysisMode::Color {
                return Err(crate::lab_adjust::unavailable_sentence("Auto Color"));
            }
            let flat = flatten(&self.layers);
            let edit = self.auto_edits(&flat, auto, below)?;
            plan += scoped_auto::scratch(u16::MAX, (self.width, self.height));
            if let Some(mask) = edit
                .then(|| flat[below].layer.mask())
                .flatten()
                .filter(|m| m.enabled())
            {
                plan += mask.plan()?.0;
            }
        }
        Ok(plan)
    }

    pub fn scoped_analysis(
        &self,
        request: &AnalysisRequest,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        scoped_statistics::validate_coverage(coverage, self.width, self.height)?;
        let scratch = self.scoped_analysis_plan(request)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let auto = backdrop_auto(request);
        let weights = match auto {
            Some((auto, below)) => self.auto_weights(
                &flatten(&self.layers),
                auto,
                below,
                coverage,
                job.range(0., 0.1),
            )?,
            None => Vec::new(),
        };
        let (source_job, evaluate_job) = if auto.is_some() {
            (job.range(0.1, 0.6), job.range(0.6, 0.8))
        } else {
            (job.range(0., 0.6), job.range(0.6, 1.))
        };
        let (image, offset) = self.analysis_source(
            request,
            limit,
            other + STATISTICS_SCRATCH + weights.len() as u64 * 2,
            source_job,
        )?;
        let mut result = evaluate_precision(
            request,
            (self.width, self.height),
            &image,
            offset,
            coverage,
            evaluate_job,
        )?;
        if let Some(policy) = &self.float {
            center_display_codes(&mut result, policy);
        }
        if let Some((auto, _)) = auto {
            let canvas = (self.width, self.height);
            result["auto"] = scoped_auto::derive(
                auto,
                canvas,
                u16::MAX,
                &weights,
                &|i| {
                    mapped_xy(i, canvas.0, image.dimensions(), offset)
                        .and_then(|(x, y)| image.rgba16_at(x, y))
                        .unwrap_or([0; 4])
                },
                self.is_lab(),
                job.range(0.8, 1.),
            )?;
        }
        fields(&mut result, &self.profile);
        Ok(result)
    }

    pub(crate) fn analysis_source(
        &self,
        request: &AnalysisRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(PrecisionImage, (i32, i32)), String> {
        match request.source {
            AnalysisSource::Composite => Ok((self.render_with_context(limit, other, job)?, (0, 0))),
            AnalysisSource::Layer => {
                let index = request.layer.ok_or("missing layer")?;
                let layer = self.layer(index).ok_or("analysis layer is out of range")?;
                if let Some(image) = layer.source() {
                    return Ok((image.clone(), layer.offset()));
                }
                // A group reads its RAW projection, the one rule Copy uses
                // (`raw_group_document`, the master).
                let source = self.raw_group_document(index)?;
                Ok((source.render_with_context(limit, other, job)?, (0, 0)))
            }
            AnalysisSource::Backdrop => {
                // The same walk as the retouch cut: a postorder exclusion that
                // gates contribution only and never touches `visible`.
                fn cut(
                    nodes: &[PrecisionDocumentLayer],
                    cursor: &mut usize,
                    below: usize,
                    out: &mut Vec<bool>,
                ) {
                    for node in nodes {
                        let start = *cursor;
                        if let Some(children) = node.children() {
                            cut(children, cursor, below, out);
                        }
                        let index = *cursor;
                        *cursor += 1;
                        if index >= below && start >= below {
                            out[index] = true;
                        }
                    }
                }
                let mut excluded = vec![false; self.records];
                cut(
                    &self.layers,
                    &mut 0,
                    request.below.ok_or("missing boundary")?,
                    &mut excluded,
                );
                Ok((
                    self.render_cut_with_context(
                        crate::style_composite::Cut::new(&excluded),
                        limit,
                        other,
                        job,
                    )?,
                    (0, 0),
                ))
            }
        }
    }
}

impl DrDocument {
    pub fn scoped_analysis_plan(&self, request: &AnalysisRequest) -> Result<u64, String> {
        match request.source {
            AnalysisSource::Layer => {
                let layer = self
                    .layers
                    .get(request.layer.ok_or("missing layer")?)
                    .ok_or("analysis layer is out of range")?;
                if crate::retouch_sampling::is_adjustment(layer) {
                    return Err("an adjustment has no raw pixels; inspect its backdrop".into());
                }
            }
            AnalysisSource::Backdrop
                if request.below.ok_or("missing boundary")? > self.layers.len() =>
            {
                return Err("backdrop boundary is out of range".into())
            }
            _ => {}
        }
        let mut plan = STATISTICS_SCRATCH
            + BOOKKEEPING_BYTES
            + crate::render_scratch::projection_scratch_bytes(self);
        if let Some((auto, below)) = backdrop_auto(request) {
            let edit = self.auto_edits(auto, below)?;
            plan += scoped_auto::scratch(255, (self.width, self.height));
            let layer = self.layers.get(below).filter(|l| edit && l.mask_enabled);
            if let Some((layer, raw)) = layer.and_then(|l| l.mask.as_ref().map(|m| (l, m))) {
                // One canvas crop plus the derived (feathered) plane.
                plan += u64::from(self.width) * u64::from(self.height)
                    + if layer.mask_properties.feather != 0.0 {
                        layer.mask_properties.scratch_bytes(raw.dimensions())
                    } else {
                        0
                    };
            }
        }
        Ok(plan)
    }

    pub fn scoped_analysis(
        &self,
        request: &AnalysisRequest,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        scoped_statistics::validate_coverage(coverage, self.width, self.height)?;
        let scratch = self.scoped_analysis_plan(request)?;
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        admit(inventory.unique_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let mut result = if matches!(request.source, AnalysisSource::Layer)
            && self.layers[request.layer.unwrap()].kind == LayerKind::Raster
        {
            let layer = &self.layers[request.layer.unwrap()];
            scoped_statistics::evaluate(
                request,
                (self.width, self.height),
                255,
                coverage,
                |i| {
                    mapped_xy(i, self.width, layer.pixels.dimensions(), layer.offset)
                        .and_then(|(x, y)| layer.pixels.sample_at(x, y))
                        .map_or([0; 4], |p| {
                            [
                                u16::from(p[0]),
                                u16::from(p[1]),
                                u16::from(p[2]),
                                u16::from(p[3]),
                            ]
                        })
                },
                job,
            )?
        } else {
            let auto = backdrop_auto(request);
            let weights = match auto {
                Some((auto, below)) => self.auto_weights(auto, below, coverage, job)?,
                None => Vec::new(),
            };
            // Byte spatial/group compositing retains its existing declared
            // full-frame noncancellable stage, bracketed by token checks.
            let image = match request.source {
                AnalysisSource::Composite => self.flattened()?,
                AnalysisSource::Layer => self
                    .layer_canvas_image(request.layer.ok_or("missing layer")?)
                    .ok_or("could not render raw group pixels")?,
                AnalysisSource::Backdrop => {
                    let below = request.below.ok_or("missing boundary")?;
                    let excluded: Vec<bool> = (0..self.layers.len())
                        .map(|i| {
                            i >= below && crate::doc_group::subtree(&self.layers, i).start >= below
                        })
                        .collect();
                    self.flattened_cut(
                        crate::doc_group::MAX_GROUP_BUFFER_PIXELS,
                        crate::style_composite::Cut::new(&excluded),
                    )?
                }
            };
            job.check()?;
            let mut result =
                evaluate_rgba8(request, (self.width, self.height), &image, coverage, job)?;
            if let Some((auto, _)) = auto {
                result["auto"] = scoped_auto::derive(
                    auto,
                    (self.width, self.height),
                    255,
                    &weights,
                    &|i| byte_codes(&image, i),
                    false,
                    job,
                )?;
            }
            result
        };
        fields(&mut result, &self.profile);
        Ok(result)
    }

    /// The composite scope's plan over a supplied 8-bit projection:
    /// [`projection_plan`], so no render is charged.
    pub fn projection_scoped_analysis_plan(
        &self,
        request: &AnalysisRequest,
    ) -> Result<u64, String> {
        projection_plan(request)
    }

    /// The composite scope counted over a supplied (retained) 8-bit
    /// projection instead of rendering again — the same answers as
    /// [`Self::scoped_analysis`]'s composite arm when `projection` is this
    /// document's render. Refuses other sources ([`projection_plan`]), a
    /// projection that is not canvas-sized and coverage that is not
    /// canvas-sized; admits the projection's pixels plus the plan against
    /// `limit` beside `other`.
    pub fn projection_scoped_analysis(
        &self,
        projection: &RgbaImage,
        request: &AnalysisRequest,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        scoped_statistics::validate_coverage(coverage, self.width, self.height)?;
        let scratch = projection_plan(request)?;
        if projection.dimensions() != (self.width, self.height) {
            return Err("projection is not the canvas size".into());
        }
        // The projection's pixels as the resource report records an
        // `DrImage` (`image_pixels`).
        let mut inventory = crate::resources::Inventory::default();
        inventory.vec(projection.as_raw(), "image_pixels", false);
        admit(inventory.unique_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let mut result = evaluate_rgba8(
            request,
            (self.width, self.height),
            projection,
            coverage,
            job,
        )?;
        fields(&mut result, &self.profile);
        Ok(result)
    }
}
