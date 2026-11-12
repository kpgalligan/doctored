//! Immutable typed geometry. Every stored raster/mask destination and native
//! reopen capacity is planned before allocation. Document crop retains latent
//! off-canvas storage; leaf transforms leave unlinked masks in canvas space.
//!
//! There is no float geometry gate:
//! exact quarters still delegate, and every filtered op (resize, straighten, RotateCanvas,
//! affine/perspective and their sets, mask transforms) resamples premultiplied extended-range f32
//! through the shared walks (`sampling::{accumulate_extended, straight_extended}`, `straight_unorm`
//! the master), dropping an output alpha below 2^-24 (`sampling::EXTENDED_ALPHA_FLOOR`, so a mask
//! or opacity never pushes it under the renderer's 2^-96 refusal floor) and clamping RGB to the
//! layer's own visible range (`precision_sampling::visible_range`, `float_document`'s D3); the plan
//! charges the float source copy and adoption at 16 B/px. Warp takes the same finish
//! (`precision_warp`, applied at once, no field), and so does a 32-bit export with a size mode
//! (`doc_export`). Exact typed execution compares stored sample bits (including float
//! signed zero) and returns `None` only for complete unchanged state. WORK: only a PREVIEW — a
//! request carrying `"interactive":true` (the 16-bit Crop tool's straighten preview, every ⌘T
//! preview; `GeometryEdit::interactive()` reads the flag) — keeps the interactive
//! `MAX_WORK_UNITS`. Every other typed edit takes its caller's cap (the one-shot jobs' rule,
//! widened to all of them): a run under a job control has no work ceiling
//! (`sampling_gaussian::destructive_work_cap`), an uncontrolled run keeps
//! `sampling_gaussian::MAX_DESTRUCTIVE_WORK`, and the job-less plan carries no work refusal
//! (`geometry_plan_with_cap`). A filtered turn and a transform COMMIT (`affine`, `perspective`,
//! `affine_set`, `perspective_set`) also take a measured 5/4 scratch headroom. The work limit and
//! headroom rule and their measurement records live in `precision_geometry_budget.rs` (a `#[path]`
//! child of `precision_geometry`). A PLACED
//! image (a retained source) follows: each re-rendered source layer's plan is
//! `retained_source_render::work_plan_within` at the edit's `work_limit` (`work_plan` is the
//! `CONTROLLED_JOB_WORK` wrapper its other callers use), the direct source `transform` the 16-bit ⌘T of
//! a single placed image sends takes the transform cap and `source_transform_scratch` (5/4)
//! unless it carries `"interactive":true` (`SourceEdit::Transform.interactive`, forwarded by
//! `retained_affine`; honoured at both depths). A re-rendered source layer's
//! unexecuted raster plane adds no work units (`Execution::range` takes the source render's units
//! for it), but its scratch is charged in every edit (the skip under a one-shot edit or a
//! preview was removed). With the measured 27 B/px decode term and the exact native PNG bound a
//! 24 MP placed image commits, and with the banded native writer it saves at 16 bits.
use crate::doc_transform::SourceMap;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage};
use crate::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use crate::precision_sampling::{self, GeometryFilter};
use crate::sampling_linear::LinearCodes;
use crate::tile_store::Sample;
use serde::Deserialize;
use std::sync::Arc;

#[path = "precision_geometry_budget.rs"]
mod budget;
#[path = "precision_geometry_canvas.rs"]
mod canvas;
#[path = "precision_geometry_cmyk.rs"]
mod cmyk;
#[path = "precision_geometry_plan.rs"]
mod plan;
#[path = "precision_geometry_set.rs"]
mod set;
#[path = "precision_source_geometry.rs"]
mod source;
pub(crate) use budget::{one_shot, source_transform_scratch};
pub(crate) use canvas::trims;
pub use plan::GeometryPlan;
use plan::{PlaneWork, WorkKind};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryTarget {
    Pixels,
    Mask,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum GeometryEdit {
    Affine {
        layer: usize,
        target: GeometryTarget,
        matrix: [f64; 6],
        filter: GeometryFilter,
        /// A PREVIEW sets it and keeps the interactive `MAX_WORK_UNITS`
        /// (the host answers that refusal with a reduced proxy or the commit
        /// request as a cancellable preview job);
        /// a commit omits it and is bounded by `MAX_DESTRUCTIVE_WORK` when
        /// uncontrolled (`precision_geometry_budget`). The byte request bounds a placed
        /// layer's source render by the same choice.
        #[serde(default)]
        interactive: bool,
    },
    Perspective {
        layer: usize,
        target: GeometryTarget,
        quad: [f64; 8],
        filter: GeometryFilter,
        /// `Affine`'s `interactive`.
        #[serde(default)]
        interactive: bool,
    },
    AffineSet {
        layers: Vec<usize>,
        matrix: [f64; 6],
        filter: GeometryFilter,
        /// Whether membership — and so the position-lock gate, which walks
        /// the same members — expands through LINK ids as well as subtrees.
        /// `false` is the typed twin of byte `dr_doc_with_layer_offset`: a
        /// property write moves only the entry (a group: its subtree), never
        /// its link partners. The byte request refuses `false`.
        #[serde(default = "follows_links")]
        follow_links: bool,
        /// `Affine`'s `interactive`.
        #[serde(default)]
        interactive: bool,
    },
    PerspectiveSet {
        layers: Vec<usize>,
        source_rect: [f64; 4],
        quad: [f64; 8],
        filter: GeometryFilter,
        /// `Affine`'s `interactive`.
        #[serde(default)]
        interactive: bool,
    },
    Resize {
        width: u32,
        height: u32,
        filter: GeometryFilter,
    },
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    /// Canvas Size (12F-1, G1): `(x, y)` is where the OLD canvas's top-left
    /// corner lands in the new one — the byte `origin`. Its contract is
    /// `precision_geometry_canvas`'s module doc.
    CanvasSize {
        width: u32,
        height: u32,
        x: i32,
        y: i32,
    },
    /// STRAIGHTEN (12F-1b2, G6): the Crop tool's straighten and the agent
    /// `crop` tool's, as ONE typed op equal to the byte composition
    /// `straighten_layers(m)` + `transform_channels(m)` + `crop(x, y, w, h)`.
    /// `matrix` turns the picture in CANVAS coordinates and `(x, y, width,
    /// height)` is the window that becomes the new canvas; its contract is
    /// `precision_geometry_canvas`'s module doc.
    Straighten {
        matrix: [f64; 6],
        filter: GeometryFilter,
        /// Signed: the window may lie past any canvas edge
        /// (`crop_background::window_fits`).
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        /// The 16-bit Crop tool's PREVIEW sets it and keeps the
        /// interactive `MAX_WORK_UNITS` (a preview is redone on every angle
        /// change; over the budget the host shows its reduced proxy); the one-shot
        /// commit omits it and is bounded by `MAX_DESTRUCTIVE_WORK` when
        /// uncontrolled (`precision_geometry_budget`). The byte straighten has no work cap
        /// of its own; it bounds a placed layer's source render by the same
        /// choice.
        #[serde(default)]
        interactive: bool,
        /// The Crop tool's Delete Cropped Pixels — each eligible
        /// raster is clipped to the new canvas
        /// (`precision_geometry_canvas::deleting_cropped`).
        #[serde(default)]
        delete_cropped: bool,
        /// The Background (Default) fill of a crop past the canvas — a
        /// qualifying opaque Background is padded with this colour before the
        /// op runs (`doc_crop_extend`). Absent: nothing is padded.
        #[serde(default)]
        background: Option<CropBackground>,
    },
    RotateCanvas {
        degrees: f64,
        filter: GeometryFilter,
    },
    Rotate90,
    Rotate180,
    Rotate270,
    FlipHorizontal,
    FlipVertical,
}
/// A straighten's Background (Default) fill colour: `color` (RGB in
/// 0…1, in `color_space`, `document` unless named) on an RGB document, `ink`
/// (`[c, m, y, k]` in 0…1, ink 1 being 100 %) on a CMYK one. JSON
/// `{"color":[r,g,b],"color_space":"document"}` or `{"ink":[c,m,y,k]}`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CropBackground {
    #[serde(default)]
    pub color: Option<[f64; 3]>,
    #[serde(default)]
    pub color_space: Option<crate::precision_paint::PaintColorSpace>,
    #[serde(default)]
    pub ink: Option<[f64; 4]>,
}
/// A crop past the canvas whose saved channels would break their budget at
/// the window's size (`crop_background::window_fits`, both depths).
pub(crate) const CROP_CHANNELS_DO_NOT_FIT: &str =
    "the saved alpha channels do not fit the extended canvas: delete some first";
/// A `background` whose shape does not match the document's mode.
pub(crate) const CROP_BACKGROUND_SHAPE: &str =
    "crop background takes color on an RGB document and ink on a CMYK one";
/// A straighten window whose edges leave `i32` (so `-x` could wrap).
pub(crate) const CROP_WINDOW_OUT_OF_RANGE: &str =
    "the crop window's edges must lie within the signed 32-bit range";
/// A Background pad (`crop_background::pad_rect`) past the pixel limit, the
/// `i32` range or the native save cap.
pub(crate) const CROP_PAD_TOO_LARGE: &str = "the crop reaches too far past the canvas for the Background to grow: choose a window nearer the canvas";
/// An affine set follows links unless the request says otherwise.
fn follows_links() -> bool {
    true
}

impl GeometryEdit {
    pub fn parse(request: &str) -> Result<Self, String> {
        if request.len() > 64 * 1024 {
            return Err("typed geometry request exceeds 64 KiB".into());
        }
        let edit: Self = serde_json::from_str(request)
            .map_err(|e| format!("invalid typed geometry request: {e}"))?;
        // Serde's internally tagged unit variants otherwise ignore extra keys.
        if matches!(
            edit,
            Self::Rotate90
                | Self::Rotate180
                | Self::Rotate270
                | Self::FlipHorizontal
                | Self::FlipVertical
        ) {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct ExactRequest {
                #[serde(rename = "op")]
                _op: String,
            }
            serde_json::from_str::<ExactRequest>(request)
                .map_err(|e| format!("invalid typed geometry request: {e}"))?;
        }
        Ok(edit)
    }
    /// Whether the request is a PREVIEW: the `interactive` flag
    /// of a transform or a straighten, `false` for every other op.
    pub(crate) fn interactive(&self) -> bool {
        match self {
            Self::Affine { interactive, .. }
            | Self::Perspective { interactive, .. }
            | Self::AffineSet { interactive, .. }
            | Self::PerspectiveSet { interactive, .. }
            | Self::Straighten { interactive, .. } => *interactive,
            _ => false,
        }
    }
}

impl PlaneWork {
    fn map(&self) -> Option<SourceMap> {
        let source = (self.source.0, self.source.1);
        let dest = (self.dest.0, self.dest.1);
        match self.kind {
            WorkKind::Affine { inverse, .. } => Some(SourceMap::new(&inverse, source, dest)),
            WorkKind::Perspective { inverse, .. } => {
                Some(SourceMap::projective(&inverse, source, dest))
            }
            _ => None,
        }
    }
    /// The resampled raster; `codes` is a linear-light document's
    /// (`PrecisionDocument::linear_codes`).
    fn image(
        &self,
        source: &PrecisionImage,
        codes: Option<&dyn LinearCodes<u16>>,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        match self.kind {
            WorkKind::Keep => Ok(source.clone()),
            WorkKind::Resize(filter) => precision_sampling::resize_image(
                source,
                self.dest.2,
                self.dest.3,
                filter,
                codes,
                job,
            ),
            WorkKind::Affine { filter, .. } | WorkKind::Perspective { filter, .. } => {
                precision_sampling::transform_image(
                    source,
                    (self.dest.2, self.dest.3),
                    &self.map().unwrap(),
                    filter,
                    codes,
                    job,
                )
            }
        }
    }
    fn plane(
        &self,
        source: &PrecisionPlane,
        outside: u16,
        job: JobContext<'_>,
    ) -> Result<PrecisionPlane, String> {
        match self.kind {
            WorkKind::Keep => Ok(source.clone()),
            WorkKind::Resize(filter) => {
                precision_sampling::resize_plane(source, self.dest.2, self.dest.3, filter, job)
            }
            WorkKind::Affine { filter, .. } | WorkKind::Perspective { filter, .. } => {
                precision_sampling::transform_plane_outside(
                    source,
                    (self.dest.2, self.dest.3),
                    &self.map().unwrap(),
                    filter,
                    outside,
                    job,
                )
            }
        }
    }
}

fn image_equal(
    a: &PrecisionImage,
    b: &PrecisionImage,
    job: JobContext<'_>,
) -> Result<bool, String> {
    if a.dimensions() != b.dimensions() {
        return Ok(false);
    }
    // At 32-bit float every geometry op is admitted (the filtered ones and
    // warp included), and a layer whose output lands at its OWN offset — a
    // turn or a flip of a full-canvas layer, or a resample that happens to
    // keep its extent — reaches this comparison. Bit equality, not `==`: a
    // permutation copies bits, so `to_bits` is the honest question and it
    // says nothing about NaN (`float_document` refuses one long before here).
    if let (Some(x), Some(y)) = (a.runs_rgba32(), b.runs_rgba32()) {
        if a.plane_key() == b.plane_key() {
            return Ok(true);
        }
        return samples_equal(x.flatten(), y.flatten(), job);
    }
    let x = a
        .runs_rgba16()
        .ok_or("typed geometry requires RGBA16 storage")?;
    let y = b
        .runs_rgba16()
        .ok_or("typed geometry requires RGBA16 storage")?;
    if a.plane_key() == b.plane_key() {
        return Ok(true);
    }
    samples_equal(x.flatten(), y.flatten(), job)
}

/// `image_equal`'s walk over two equal-length sample sequences read through
/// the tiles (no view): one `job.check()` per 4096-sample chunk reached,
/// BEFORE comparing that chunk's first sample, and `Ok(false)` at the first
/// `Sample::same_bits` difference.
fn samples_equal<'s, S: Sample>(
    a: impl Iterator<Item = &'s S>,
    b: impl Iterator<Item = &'s S>,
    job: JobContext<'_>,
) -> Result<bool, String> {
    for (i, (x, y)) in a.zip(b).enumerate() {
        if i % 4096 == 0 {
            job.check()?;
        }
        if !x.same_bits(*y) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether two optional styles are the same allocation (or both absent).
fn same_style(
    a: &Option<Arc<crate::style::LayerStyle>>,
    b: &Option<Arc<crate::style::LayerStyle>>,
) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

struct Execution<'a> {
    plan: &'a GeometryPlan,
    /// The whole-canvas edit's family and canvas map
    /// (`doc_reference::edit_map`): every record's reference point and linked
    /// anchors follow it (`doc_reference::follow_record`).
    follow: Option<(crate::reference_point::ReferenceOp, [f64; 6])>,
    /// The edit's one set of turned tiles (`doc_reference::TurnedTiles`),
    /// shared by every record the recursion reaches.
    tiles: crate::doc_reference::TurnedTiles,
    preserve_exact_set_description: bool,
    /// The document's linear-light tables (`PrecisionDocument::linear_codes`):
    /// every u16 raster and re-rendered source resamples through them.
    codes: Option<&'a dyn LinearCodes<u16>>,
    index: usize,
    completed: u64,
    changed: bool,
    job: JobContext<'a>,
}
impl Execution<'_> {
    fn range(&mut self, units: u64) -> JobContext<'_> {
        let start = self.completed as f64 / self.plan.work_units.max(1) as f64;
        self.completed += units;
        let end = self.completed as f64 / self.plan.work_units.max(1) as f64;
        self.job.range(start, end)
    }
    fn layers(
        &mut self,
        layers: &[PrecisionDocumentLayer],
    ) -> Result<Vec<PrecisionDocumentLayer>, String> {
        let mut result = Vec::with_capacity(layers.len());
        for layer in layers {
            self.job.check()?;
            let mut next = layer.clone();
            match &layer.content {
                Content::Group(children) => {
                    next.content = Content::Group(Arc::new(self.layers(children)?))
                }
                Content::PassThrough(children) => {
                    next.content = Content::PassThrough(Arc::new(self.layers(children)?))
                }
                _ => {}
            }
            let work = self.plan.layers[self.index].clone();
            self.index += 1;
            // The CMYK arm (#1 phase 4b): the raster arm below over
            // CMYKA16 ink (`PlaneWork::cmyk_image`), with no retained source
            // (a CMYK record holds none); a changed raster drops its
            // description exactly where the RGB arm does (#1's artwork
            // batch).
            if let (Content::CmykRaster { image, offset }, Some(image_work)) =
                (&layer.content, &work.image)
            {
                let job = self.range(image_work.work);
                let location = (image_work.dest.0, image_work.dest.1);
                let output = image_work.cmyk_image(image, job)?;
                let equal = location == *offset
                    && (matches!(image_work.kind, WorkKind::Keep)
                        || (image.dimensions() == output.dimensions()
                            && image.same_samples(&output)));
                if !equal {
                    self.changed = true;
                    if !(self.preserve_exact_set_description
                        && matches!(image_work.kind, WorkKind::Keep))
                    {
                        next.description = None;
                    }
                    next.content = Content::CmykRaster {
                        image: output,
                        offset: location,
                    };
                }
            }
            if let (Content::Raster { image, offset }, Some(image_work)) =
                (&layer.content, work.image)
            {
                // A re-rendered layer's progress share is its source render's
                // units, which is all the plan counts for it.
                let units = match work.source_cost {
                    Some(cost) if !work.source_exact => cost.work_units,
                    _ => image_work.work,
                };
                let codes = self.codes;
                let job = self.range(units);
                let location = (image_work.dest.0, image_work.dest.1);
                let (output, binding_changed) = if let Some(transform) = work.source_transform {
                    let previous = layer
                        .effective_source()
                        .ok_or("retained source binding became invalid")?;
                    let mut source = previous.clone();
                    source.transform = transform;
                    source.filter = work
                        .source_filter
                        .ok_or("missing retained source geometry sampler")?;
                    if work.source_exact {
                        source.rendered = image_work.image(image, codes, job)?;
                        source.rendered_offset = location;
                    } else {
                        source = crate::retained_source_float::render_typed(
                            source,
                            image.profile(),
                            image.storage()
                                == crate::precision_image::PrecisionStorage::Rgba32Float,
                            codes,
                            job,
                        )?;
                        if source.rendered_offset != location
                            || source.rendered.dimensions()
                                != (image_work.dest.2, image_work.dest.3)
                        {
                            return Err("retained source geometry plan and result disagree".into());
                        }
                    }
                    let changed = source.transform != previous.transform
                        || std::mem::discriminant(&source.filter)
                            != std::mem::discriminant(&previous.filter);
                    let output = source.rendered.clone();
                    next.retained_source = Some(source);
                    (output, changed)
                } else {
                    (image_work.image(image, codes, job)?, false)
                };
                // A `Keep` plane IS the input allocation, so it is equal by
                // construction and never reaches `image_equal`.
                let equal = location == *offset
                    && (matches!(image_work.kind, WorkKind::Keep)
                        || image_equal(image, &output, self.job)?);
                if !equal || binding_changed {
                    self.changed = true;
                    if work.source_transform.is_none()
                        && !(self.preserve_exact_set_description
                            && matches!(image_work.kind, WorkKind::Keep))
                    {
                        next.description = None;
                    }
                    next.content = Content::Raster {
                        image: output,
                        offset: location,
                    };
                }
            }
            // Scale Effects: the plan's factor, re-quantized by the parser's
            // rules; the same Arc comes back when nothing changes.
            if layer.style.is_some() {
                let scaled = crate::style::scaled_style(&layer.style, work.style_factor);
                // Delete Cropped Pixels' trim rebases a pattern anchor.
                let scaled = crate::style_pattern::rebased_arc(&scaled, work.anchor_shift);
                if !matches!((&scaled, &layer.style), (Some(a), Some(b)) if Arc::ptr_eq(a, b)) {
                    self.changed = true;
                }
                next.style = scaled;
            }
            // The reference rule: a Free Transform record by its
            // matrix, a whole-canvas edit by its map, against the final
            // (trimmed) offset. A moved reference alone is not an edit
            // (`changed`); a replaced style (a turned tile, a moved linked
            // anchor) is, as Scale Effects' is above.
            let styled = next.style.clone();
            if let Some(matrix) = work.reference_map {
                let op = crate::reference_point::ReferenceOp::Transform;
                crate::doc_reference::follow_record(layer, &mut next, op, matrix, &self.tiles);
            }
            if let Some((op, matrix)) = self.follow {
                crate::doc_reference::follow_record(layer, &mut next, op, matrix, &self.tiles);
            }
            if !same_style(&styled, &next.style) {
                self.changed = true;
            }
            if let (Some(mask), Some(work)) = (&layer.mask, work.mask) {
                let job = self.range(work.work);
                let output = work.plane(
                    &mask.plane,
                    u16::from(mask.properties.outside_coverage) * 257,
                    job,
                )?;
                let mut properties = mask.properties.clone();
                properties.offset = (work.dest.0, work.dest.1);
                let equal =
                    properties == mask.properties && mask.plane.equals(&output, self.job)?;
                if !equal {
                    self.changed = true;
                    next.mask = Some(PrecisionMask::new(output, properties, mask.enabled)?);
                }
            }
            result.push(next);
        }
        Ok(result)
    }
}

impl PrecisionDocument {
    fn retained_affine(&self, edit: &GeometryEdit) -> Option<crate::retained_source::SourceEdit> {
        if let GeometryEdit::Affine {
            layer,
            target: GeometryTarget::Pixels,
            matrix,
            filter,
            interactive,
        } = edit
        {
            self.layer(*layer)?.effective_source()?;
            Some(crate::retained_source::SourceEdit::Transform {
                layer: *layer,
                matrix: *matrix,
                filter: *filter,
                interactive: *interactive,
            })
        } else {
            None
        }
    }
    /// The job-less plan (the FFI plan): every edit but a preview is planned
    /// at `CONTROLLED_JOB_WORK`, so it carries no work refusal.
    pub fn geometry_plan(&self, edit: &GeometryEdit) -> Result<GeometryPlan, String> {
        self.geometry_plan_with_cap(edit, crate::sampling_gaussian::CONTROLLED_JOB_WORK)
    }
    /// The plan with a non-preview edit's work bounded by `one_shot_cap`
    /// (`destructive_work_cap(job)` for a run).
    pub(crate) fn geometry_plan_with_cap(
        &self,
        edit: &GeometryEdit,
        one_shot_cap: u64,
    ) -> Result<GeometryPlan, String> {
        // A crop past the canvas with the Background fill is planned
        // over the padded document's stand-in, plus the pad itself.
        if let Some((padded, raster)) = self.background_plan_input(edit)? {
            let mut plan = padded.geometry_plan_with_cap(edit, one_shot_cap)?;
            plan.retained_bytes = plan.retained_bytes.saturating_add(raster);
            plan.scratch_bytes = plan
                .scratch_bytes
                .saturating_add(self.background_pad_bytes(edit)?);
            return Ok(plan);
        }
        if let Some(edit) = self.retained_affine(edit) {
            let source = self.source_plan_input(None, &edit, one_shot_cap)?;
            return Ok(GeometryPlan {
                width: self.width,
                height: self.height,
                retained_bytes: source.retained_bytes,
                scratch_bytes: source.scratch_bytes,
                work_units: source.work_units,
                layers: Vec::new(),
                channels: Vec::new(),
                transpose_resolution: false,
            });
        }
        plan::build(self, edit, None, one_shot_cap)
    }
    pub fn geometry(
        &self,
        edit: &GeometryEdit,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.geometry_with_context(edit, limit, other_retained, job)
    }
    /// The LOCK gate (`precision_lock`). A per-layer affine or perspective
    /// is a POSITION edit and never a Pixels one — it resamples the whole
    /// buffer INCLUDING its alpha, so "the alpha channel is frozen" has no
    /// meaning there, and Lock Position is the flag that refuses it
    /// (`doc_lock`'s module doc, and `doc_transform` / `doc_perspective`'s
    /// own gates). Transforming a layer's MASK is a `Mask` edit. The
    /// whole-CANVAS ops — resize, crop, rotate, flip — are ungated at both
    /// depths: they move the document, not a layer, and a locked layer
    /// inside one is not a layer edit.
    pub(crate) fn geometry_with_context(
        &self,
        edit: &GeometryEdit,
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // The Background pre-pass, admitted before the pad is made; the
        // op then runs over the padded document with the pad held.
        let pad = self.background_pad_bytes(edit)?;
        if pad > 0 {
            admit(self.retained_bytes(), pad, other_retained, limit)?;
            job.reserve(pad)?;
            if let Some(padded) = self.background_padded(edit)? {
                return padded.geometry_with_context(
                    edit,
                    limit,
                    other_retained.saturating_add(pad),
                    job.with_held_bytes(pad),
                );
            }
        }
        let gate = match edit {
            GeometryEdit::Affine { layer, target, .. }
            | GeometryEdit::Perspective { layer, target, .. } => Some(match target {
                GeometryTarget::Pixels => (*layer, crate::doc_lock::EditKind::Position),
                GeometryTarget::Mask => (*layer, crate::doc_lock::EditKind::Mask),
            }),
            _ => None,
        };
        if matches!(
            edit,
            GeometryEdit::AffineSet { .. } | GeometryEdit::PerspectiveSet { .. }
        ) {
            for index in set::expanded(self, edit)? {
                self.lock_refusal(index, crate::doc_lock::EditKind::Position)?;
            }
        }
        let Some((index, kind)) = gate else {
            return self.geometry_unlocked(edit, limit, other_retained, job);
        };
        self.under_locks(index, kind, |doc| {
            doc.geometry_unlocked(edit, limit, other_retained, job)
        })
    }

    fn geometry_unlocked(
        &self,
        edit: &GeometryEdit,
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.geometry_resolved(edit, None, limit, other_retained, job)
    }

    /// The geometry body, with a set's per-record mappings (`translate_records`).
    fn geometry_resolved(
        &self,
        edit: &GeometryEdit,
        set: Option<set::ResolvedSet>,
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if let (None, Some(source_edit)) = (&set, self.retained_affine(edit)) {
            // The byte twin's retained-source return (`doc_transform`): the
            // record's reference point and linked anchors follow M.
            let step = match edit {
                GeometryEdit::Affine { layer, matrix, .. } => Some((*layer, *matrix)),
                _ => None,
            };
            let Some(doc) =
                self.source_edit_with_context(None, &source_edit, limit, other_retained, job)?
            else {
                return Ok(None);
            };
            let Some((index, matrix)) = step else {
                return Ok(Some(doc));
            };
            let (Some(before), Some(record)) = (self.layer(index), doc.layer(index)) else {
                return Ok(Some(doc));
            };
            let mut record = record.clone();
            crate::doc_reference::follow_record(
                before,
                &mut record,
                crate::reference_point::ReferenceOp::Transform,
                matrix,
                &crate::doc_reference::TurnedTiles::default(),
            );
            // Only the reference and the anchors change, so the record list
            // keeps its shape and extents: the unchecked replacement is exact.
            return Ok(Some(doc.replacing_layer_for_plan(index, record)?));
        }
        admit(
            self.retained_bytes(),
            4 * 1024 * 1024,
            other_retained,
            limit,
        )?;
        job.reserve(4 * 1024 * 1024)?;
        let cap = crate::sampling_gaussian::destructive_work_cap(job);
        let plan = match set {
            Some(set) => plan::build(self, edit, Some(set), cap)?,
            None => self.geometry_plan_with_cap(edit, cap)?,
        };
        admit(
            plan.retained_bytes,
            plan.scratch_bytes,
            other_retained,
            limit,
        )?;
        job.reserve(plan.scratch_bytes)?;
        // The canvas CHROME this edit moves (`precision_guide`), decided
        // once here because the typed depth has one geometry entry point
        // where the byte depth has four ops. It counts toward `changed` for
        // the ordinary reason: a crop that moved nothing but a guide is
        // still an edit, and answering `None` would drop it.
        let (guides, ruler_origin) = self.geometry_chrome(edit)?;
        // The COLOUR SAMPLERS this edit moves (`doc_sampler`), counted toward
        // `changed` like the chrome: a crop that dropped only a sampler is
        // still an edit.
        let color_samplers = crate::doc_sampler::edited_samplers(
            &self.color_samplers,
            edit,
            self.width,
            self.height,
        )?;
        // The PATHS this edit moves (`precision_path`), counted the same way.
        let paths = crate::precision_path::edited_paths(
            &self.paths.entries,
            edit,
            self.width,
            self.height,
        )?;
        // The LAYER COMPS a whole-canvas edit remaps (`comps_geometry`): each
        // bound raster entry through the very plane rule its layer takes, a
        // retained source through the source bounds. Per-layer edits leave
        // them alone. Counted toward `changed` like the chrome.
        let canvas = self.dimensions();
        let delete_cropped = matches!(
            edit,
            GeometryEdit::Straighten {
                delete_cropped: true,
                ..
            }
        );
        let comps = match edit {
            GeometryEdit::Resize { .. }
            | GeometryEdit::Crop { .. }
            | GeometryEdit::CanvasSize { .. }
            | GeometryEdit::Straighten { .. }
            | GeometryEdit::RotateCanvas { .. }
            | GeometryEdit::Rotate90
            | GeometryEdit::Rotate180
            | GeometryEdit::Rotate270
            | GeometryEdit::FlipHorizontal
            | GeometryEdit::FlipVertical => self.remapped_comps(
                |i, rec| {
                    let image = plan.layers.get(i)?.image.as_ref()?;
                    let s = image.source;
                    let dest = plan::whole((rec.0, rec.1, s.2, s.3), edit, canvas, 4)
                        .ok()
                        .map(|w| (w.dest.0, w.dest.1))?;
                    // A clipped layer's comp offsets move by the same
                    // delta as its pixel origin — clipped dest minus the
                    // unclipped dest of the layer's own extent — so a comp
                    // recorded elsewhere keeps its relative placement.
                    if !delete_cropped || !canvas::trims(self.layer(i)?) {
                        return Some(dest);
                    }
                    let full = plan::whole(s, edit, canvas, 4).ok()?.dest;
                    let (dx, dy) = (
                        i64::from(image.dest.0) - i64::from(full.0),
                        i64::from(image.dest.1) - i64::from(full.1),
                    );
                    Some((
                        i32::try_from(i64::from(dest.0) + dx).ok()?,
                        i32::try_from(i64::from(dest.1) + dy).ok()?,
                    ))
                },
                source::incremental(edit, canvas, None).ok(),
                |i| plan.layers.get(i).map_or(1.0, |w| w.style_factor),
            ),
            GeometryEdit::Affine { .. }
            | GeometryEdit::Perspective { .. }
            | GeometryEdit::AffineSet { .. }
            | GeometryEdit::PerspectiveSet { .. } => self.comps.clone(),
        };
        let mut execution = Execution {
            plan: &plan,
            follow: crate::doc_reference::edit_map(edit, self.width, self.height)?,
            tiles: crate::doc_reference::TurnedTiles::default(),
            preserve_exact_set_description: matches!(
                edit,
                GeometryEdit::AffineSet { .. } | GeometryEdit::PerspectiveSet { .. }
            ),
            codes: self.linear_codes(),
            index: 0,
            completed: 0,
            changed: self.dimensions() != (plan.width, plan.height)
                || self.resolution != self.resolution.transposed_if(plan.transpose_resolution)
                || guides != self.guides
                || ruler_origin != self.ruler_origin
                || color_samplers != self.color_samplers
                || paths != *self.paths.entries
                || comps != self.comps,
            job,
        };
        let layers = execution.layers(&self.layers)?;
        let mut channels: Vec<PrecisionChannel> = Vec::with_capacity(self.channels.len());
        for (channel, work) in self.channels.iter().zip(&plan.channels) {
            let mut next = channel.clone();
            if let Some(work) = work {
                let range = execution.range(work.work);
                let output = work.plane(&channel.plane, 0, range)?;
                if !channel.plane.equals(&output, job)? {
                    next.plane = output;
                    execution.changed = true;
                }
            }
            channels.push(next);
        }
        job.check()?;
        let result = if execution.changed {
            let mut result = self.rebuilt(plan.width, plan.height, layers, channels)?;
            result.resolution = self.resolution.transposed_if(plan.transpose_resolution);
            // `rebuilt` carried the chrome VERBATIM; this is the one caller
            // that also moved the canvas, so it overwrites both with what
            // `geometry_chrome` decided.
            result.guides = guides;
            result.ruler_origin = ruler_origin;
            result.color_samplers = color_samplers;
            result.paths.set_entries(paths);
            result.comps = comps;
            // The vector-mask follow hook (`precision_vector_mask`).
            Some(self.following_geometry(result, edit, limit, other_retained))
        } else {
            None
        };
        job.report(1.0, 5)?;
        Ok(result)
    }
}
