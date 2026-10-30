//! Native retouch commands over captured sampling plates. Source interpolation,
//! destination storage growth and output placement are explicit; every result
//! is immutable, capacity checked, cancellable, and exact outside write coverage.
//!
//! Since 13B-3d-2 the IN-PLACE arm (a Layer target at its own extent) is a
//! RECT writer: the op's kernel (`precision_retouch_apply`) bounds its write,
//! one `read_rect_rgba16` + `with_rect_rgba16` (no view, ≤ 4 new tiles per
//! dab); a new layer or a grown extent keeps the whole buffer (rows in).
//!
//! Scalar stroke coverage stays separate from source pixels. A compound job's reservation floor
//! keeps nested sampling/render stages from hiding already-live buffers. Expansion preserves
//! existing samples and independent mask placement; healing retains destination alpha.
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage,
    BOOKKEEPING_BYTES,
};
use crate::precision_paint::{canvas_index, PaintPlan};
use crate::precision_retouch_apply::RetouchKernel;
use crate::retouch_sampling::RetouchSampling;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RetouchOp {
    RetouchClone,
    RetouchHeal,
    SpotHeal,
    ContentAwareFill,
    RedEye,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RetouchOutput {
    #[default]
    Layer,
    NewLayer,
}
fn current() -> String {
    "current".into()
}
fn identity() -> [f64; 6] {
    [1., 0., 0., 1., 0., 0.]
}
fn one() -> f32 {
    1.
}
fn half() -> f32 {
    0.5
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetouchEdit {
    pub op: RetouchOp,
    pub layer: usize,
    #[serde(default)]
    pub source_layer: Option<usize>,
    #[serde(default = "current")]
    pub sample: String,
    #[serde(default)]
    pub ignore_adjustments: bool,
    #[serde(default)]
    pub output: RetouchOutput,
    #[serde(default)]
    pub expand_destination: bool,
    #[serde(default)]
    pub selection: bool,
    #[serde(default = "identity")]
    pub matrix: [f64; 6],
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub ring: u32,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub source_mask: Option<crate::inpaint_source_mask::SourceMask>,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default = "one")]
    pub pupil_size: f32,
    #[serde(default = "half")]
    pub darken: f32,
    /// `DrBlendMode` raw value the cloned paint composites through
    /// (`retouch_clone` only, in place; 0 = Normal).
    #[serde(default)]
    pub blend_mode: u32,
    /// The Spot Healing Brush's Type (`spot_heal` only; absent =
    /// Content-Aware, the meaning every earlier request had).
    #[serde(default, rename = "type")]
    pub spot_type: crate::spot_heal_type::SpotHealType,
    /// The hole's covered count (`content_aware_fill` only): the request's
    /// coverage bytes at or above `doc_inpaint::Caller::Fill`'s hole threshold
    /// (1, so a feathered ramp counts), which sets the admission's
    /// `inpaint_lift` before the coverage is read; the edit refuses a count
    /// that is not its coverage's own (`doc_content_aware`'s `# Admission`).
    #[serde(default)]
    pub covered: Option<u64>,
    /// Content-Aware Fill Transparent Areas' pad (`content_aware_fill` with
    /// `output: "layer"` only): the hole's transparent
    /// destination pixels are made opaque before the fill writes them
    /// (`precision_inpaint::opaque_hole`), so the fill shows there. Absent =
    /// false, every earlier request's meaning: a hole over transparent pixels
    /// stays transparent.
    #[serde(default)]
    pub fill_transparent: bool,
}
const FILL_TRANSPARENT_ONLY: &str =
    "fill_transparent applies only to content_aware_fill with output layer";
impl RetouchEdit {
    pub(crate) fn output_name(&self) -> &'static str {
        if self.op == RetouchOp::ContentAwareFill {
            "Content-Aware Fill"
        } else {
            "Retouch"
        }
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 64 * 1024 {
            return Err("retouch request exceeds 64 KiB".into());
        }
        let edit: Self = serde_json::from_str(text)
            .map_err(|e| format!("invalid native retouch request: {e}"))?;
        let fields: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // The transparent pad is the in-place fill's alone, named before any
        // other key so the refusal does not depend on the keys' order.
        if fields.get("fill_transparent").is_some() && edit.op != RetouchOp::ContentAwareFill {
            return Err(FILL_TRANSPARENT_ONLY.into());
        }
        for key in fields
            .as_object()
            .ok_or("retouch request must be an object")?
            .keys()
        {
            let allowed = match edit.op {
                RetouchOp::RedEye => [
                    "op",
                    "layer",
                    "x",
                    "y",
                    "width",
                    "height",
                    "pupil_size",
                    "darken",
                ]
                .contains(&key.as_str()),
                RetouchOp::ContentAwareFill => [
                    "op",
                    "layer",
                    "ring",
                    "seed",
                    "sample",
                    "ignore_adjustments",
                    "output",
                    "source_mask",
                    "x",
                    "y",
                    "width",
                    "height",
                    "covered",
                    "fill_transparent",
                ]
                .contains(&key.as_str()),
                RetouchOp::SpotHeal => {
                    !["matrix", "pupil_size", "darken", "source_mask", "covered"]
                        .contains(&key.as_str())
                }
                _ => ![
                    "ring",
                    "seed",
                    "pupil_size",
                    "darken",
                    "source_mask",
                    "covered",
                ]
                .contains(&key.as_str()),
            };
            // A blend mode composites a SOURCE colour; only the clone has one.
            let allowed = allowed && (key != "blend_mode" || edit.op == RetouchOp::RetouchClone);
            // The Type is the Spot Healing Brush's alone.
            let allowed = allowed && (key != "type" || edit.op == RetouchOp::SpotHeal);
            if !allowed {
                return Err(format!(
                    "{key} is not an argument of this retouch operation"
                ));
            }
        }
        if edit.fill_transparent && edit.output != RetouchOutput::Layer {
            return Err(FILL_TRANSPARENT_ONLY.into());
        }
        Ok(edit)
    }
    pub(crate) fn mode(&self) -> Result<RetouchSampling, String> {
        match self.sample.as_str() {
            "current" => Ok(RetouchSampling::Current),
            "current_and_below" => Ok(RetouchSampling::CurrentAndBelow),
            "all" => Ok(RetouchSampling::All),
            _ => Err("sample must be current, current_and_below or all".into()),
        }
    }
    pub(crate) fn extent(
        &self,
        doc: &PrecisionDocument,
    ) -> Result<crate::retouch_transform::StorageExtent, String> {
        let target = doc
            .layer(self.layer)
            .ok_or("retouch target layer is out of range")?;
        let image = target.source().ok_or("retouch requires a raster target")?;
        if self.output == RetouchOutput::NewLayer {
            return Ok((doc.dimensions(), (0, 0)));
        }
        if self.expand_destination {
            crate::retouch_transform::expanded_extent(
                doc.dimensions(),
                image.dimensions(),
                target.offset(),
            )
        } else {
            Ok((image.dimensions(), target.offset()))
        }
    }
    pub(crate) fn bounds(&self, doc: &PrecisionDocument) -> Result<(u32, u32, u32, u32), String> {
        if self.x < 0
            || self.y < 0
            || self.width == 0
            || self.height == 0
            || u64::from(self.x as u32) + u64::from(self.width) > u64::from(doc.width)
            || u64::from(self.y as u32) + u64::from(self.height) > u64::from(doc.height)
        {
            return Err("retouch bounds must be a nonempty rectangle inside the canvas".into());
        }
        Ok((self.x as u32, self.y as u32, self.width, self.height))
    }
}
impl PrecisionDocument {
    pub fn retouch_plan(&self, edit: &RetouchEdit) -> Result<PaintPlan, String> {
        if self.is_cmyk() {
            return self.cmyk_retouch_plan(edit);
        }
        // A 32-bit float document's own refusal first: a clone mode
        // outside the float blend set.
        let float = self.is_float();
        if float {
            crate::float_retouch::plan_refusal(edit)?;
        }
        let mode = edit.mode()?;
        if !edit.opacity.is_finite() || !(0.0..=1.0).contains(&edit.opacity) || edit.ring > 512 {
            return Err("retouch opacity must be normalized; ring must be 0 through 512".into());
        }
        if edit.source_layer.is_some() && mode != RetouchSampling::Current {
            return Err("source_layer requires sample=current".into());
        }
        // The blend enum's own answer, as the Clone arm and `gradient_stops`
        // ask it: an unknown value, or the group-only Pass Through.
        match crate::blend::BlendMode::from_c(edit.blend_mode as i32) {
            Some(mode) if !mode.is_group_only() => {}
            _ => return Err("retouch blend mode is invalid".into()),
        }
        if edit.blend_mode != 0 && edit.output == RetouchOutput::NewLayer {
            return Err(
                "a clone onto a new layer has nothing to blend with; blend_mode requires output layer"
                    .into(),
            );
        }
        crate::retouch_transform::validate(&edit.matrix)?;
        let target = self
            .layer(edit.layer)
            .ok_or("retouch target layer is out of range")?;
        if target.retained_source.is_some() && edit.output == RetouchOutput::Layer {
            return Err("Rasterize the retained source before retouching its pixels".into());
        }
        let (dimensions, _) = edit.extent(self)?;
        let n = pixel_count(self.width, self.height)? as u64;
        let output = pixel_count(dimensions.0, dimensions.1)? as u64;
        let buffers = n * (if edit.selection { 44 } else { 40 }) + output * 16;
        let mut scratch = buffers + 8 * BOOKKEEPING_BYTES;
        if float {
            // The pixel buffers at f32 width (`float_retouch`'s module doc
            // holds the measurement), and for the ops that solve in W codes
            // (`retouch_encoding`) the encoded copy of the destination.
            scratch += buffers;
            if matches!(
                edit.op,
                RetouchOp::RetouchHeal | RetouchOp::SpotHeal | RetouchOp::ContentAwareFill
            ) {
                scratch += output * 16;
            }
        }
        if edit.op != RetouchOp::RedEye {
            scratch += self.retouch_source_plan(edit.source_layer.unwrap_or(edit.layer), mode)?;
        }
        if edit.output == RetouchOutput::NewLayer {
            self.retouch_output_refusal(edit.layer)?;
            self.preflight_raster_addition(self.dimensions(), edit.output_name())?;
            scratch += self.retouch_source_plan(edit.layer, RetouchSampling::CurrentAndBelow)?;
        } else {
            let (mut r, m) = self.native_dimensions();
            let raster_index = (0..edit.layer)
                .filter(|i| self.layer_source(*i).is_some())
                .count();
            r[raster_index] = dimensions;
            self.native_fits_geometry(self.dimensions(), &r, &m, self.channels.len())?;
        }
        if edit.op == RetouchOp::RetouchHeal {
            let area = if edit.width > 0 && edit.height > 0 {
                let (_, _, w, h) = edit.bounds(self)?;
                u64::from(w) * u64::from(h)
            } else {
                n
            };
            let padded = if edit.width > 0 && edit.height > 0 {
                u64::from(edit.width.saturating_add(2).min(self.width))
                    * u64::from(edit.height.saturating_add(2).min(self.height))
            } else {
                area
            };
            scratch += crate::precision_heal::plan(padded)?.0;
        }
        if matches!(edit.op, RetouchOp::SpotHeal | RetouchOp::ContentAwareFill) {
            let bounds = edit.bounds(self)?;
            let caller = if edit.op == RetouchOp::ContentAwareFill {
                crate::doc_inpaint::Caller::Fill
            } else {
                crate::doc_inpaint::Caller::SpotHeal
            };
            scratch += crate::precision_inpaint::estimate(
                self.dimensions(),
                bounds,
                u64::from(bounds.2) * u64::from(bounds.3),
                edit.ring,
                caller,
                edit.spot_type,
                3,
            )?
            .0;
        }
        if let Some(mask) = &edit.source_mask {
            mask.validate(self.width, self.height)?;
        }
        if edit.op == RetouchOp::RedEye {
            if edit.width == 0 || edit.height == 0 {
                return Err("red eye rectangle must be nonempty".into());
            }
            if !edit.pupil_size.is_finite()
                || !(0.01..=1.).contains(&edit.pupil_size)
                || !edit.darken.is_finite()
                || !(0.0..=1.).contains(&edit.darken)
            {
                return Err(
                    "red eye pupil_size and darken are outside their normalized ranges".into(),
                );
            }
            if edit.output != RetouchOutput::Layer
                || edit.expand_destination
                || edit.source_layer.is_some()
                || mode != RetouchSampling::Current
                || edit.ignore_adjustments
            {
                return Err("red eye corrects its current raster target in place".into());
            }
            // A Lab layer's ProPhoto work copy and pipelines (#1 Lab 2f).
            if self.is_lab() {
                scratch =
                    scratch.saturating_add(crate::lab_retouch::red_eye_scratch_bytes(output)?);
            }
        }
        Ok(PaintPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes: scratch,
            input_samples: n as usize * if edit.selection { 2 } else { 1 },
        })
    }
    pub fn retouch(
        &self,
        edit: &RetouchEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        self.retouch_with_context(
            edit,
            coverage,
            limit,
            other,
            control.map_or(JobContext::NONE, JobContext::new),
        )
    }
    /// The LOCK gate (`precision_lock`): a retouch writes its target's
    /// pixels, so it runs under a `Pixels` edit. NEW-LAYER output only
    /// CONSULTS the lock and never restores the target's alpha, exactly as
    /// `doc_retouch_output::retouch_onto_new_layer` does: the target's own
    /// buffer is not written, so there is nothing to freeze — and its own
    /// no-op latch would otherwise throw the new layer away.
    pub(crate) fn retouch_with_context(
        &self,
        edit: &RetouchEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if edit.output == RetouchOutput::NewLayer {
            self.lock_refusal(edit.layer, crate::doc_lock::EditKind::Pixels)?;
            return self.retouching_unlocked(self, edit, coverage, limit, other, job);
        }
        self.under_locks(edit.layer, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.retouching_unlocked(doc, edit, coverage, limit, other, job)
        })
    }

    /// `retouch`, with the sampling plate rendered from `plates` instead of
    /// from `self`: Content-Aware Move heals the moved pixels onto the
    /// FILLED document while reading them from the picture before the fill.
    /// Only the plate comes from `plates`; the lock gate, the plan, the
    /// admission and the rect writer are `self`'s, so `retouch_from(self, …)`
    /// is `retouch` to the byte. The plate's samples are read as they are:
    /// `plates` is taken to be in `self`'s working space.
    ///
    /// `Err` unless op is `retouch_clone` or `retouch_heal`, output is
    /// `layer`, `expand_destination` is false, and `plates` has `self`'s
    /// canvas size, its bit depth (16-bit or 32-bit float) and a raster
    /// layer at the sampled index (`source_layer`, else `layer`). `Ok(None)`
    /// when nothing would change, as `retouch`.
    pub fn retouch_from(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        self.retouch_from_refusal(plates, edit)?;
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.under_locks(edit.layer, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.retouching_unlocked(plates, edit, coverage, limit, other, job)
        })
    }

    /// The refusals [`Self::retouch_from`] documents, ahead of any plan: the
    /// export asks them first so a request it cannot run is named as such
    /// rather than by whatever its plan would have refused.
    pub(crate) fn retouch_from_refusal(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
    ) -> Result<(), String> {
        if self.is_cmyk() {
            return self.cmyk_retouch_from_refusal(plates, edit);
        }
        if !matches!(edit.op, RetouchOp::RetouchClone | RetouchOp::RetouchHeal) {
            return Err(
                "retouch from another document's plates requires retouch_clone or retouch_heal"
                    .into(),
            );
        }
        if edit.output != RetouchOutput::Layer || edit.expand_destination {
            return Err(
                "retouch from another document's plates writes its target layer in place: output must be layer and expand_destination false"
                    .into(),
            );
        }
        if plates.dimensions() != self.dimensions() {
            return Err("retouch plates must have the document's canvas size".into());
        }
        if plates.is_float() != self.is_float() {
            return Err("retouch plates must have the document's bit depth".into());
        }
        if plates
            .layer_source(edit.source_layer.unwrap_or(edit.layer))
            .is_none()
        {
            return Err("retouch plates have no raster layer at the sampled index".into());
        }
        Ok(())
    }

    /// `plates` is the document the sampling plate is rendered from: `self`
    /// for `retouch`, the caller's for `retouch_from`. Nothing else reads it.
    /// A CMYK document's two-pass retouch (`cmyk_retouch`) runs it on its
    /// pass documents.
    pub(crate) fn retouching_unlocked(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let plan = self.retouch_plan(edit)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        let job = job.with_reservation_floor(plan.scratch_bytes);
        if coverage.len() != plan.input_samples {
            return Err("retouch coverage extent mismatch".into());
        }
        let n = pixel_count(self.width, self.height)?;
        let selection = edit.selection.then(|| &coverage[n..]);
        let coverage = &coverage[..n];
        // Synthesis retains the published half-coverage stroke contour. A tiny
        // positive selection remains in the domain and fades only at writeback.
        let domain = if let Some(selection) = selection {
            let mut domain = reserved_vec(n)?;
            for (i, &c) in coverage.iter().enumerate() {
                job.every(i)?;
                domain.push(if selection[i] == 0 { 0 } else { c });
            }
            Some(domain)
        } else {
            None
        };
        let coverage = domain.as_deref().unwrap_or(coverage);
        if edit.width > 0 && edit.height > 0 && edit.op != RetouchOp::RedEye {
            let (x, y, w, h) = edit.bounds(self)?;
            for (i, c) in coverage.iter().enumerate() {
                job.every(i)?;
                if *c > 0 {
                    let (cx, cy) = (
                        (i % self.width as usize) as u32,
                        (i / self.width as usize) as u32,
                    );
                    if cx < x || cy < y || cx >= x + w || cy >= y + h {
                        return Err("retouch coverage exceeds its admitted bounds".into());
                    }
                }
            }
        }
        if edit.opacity == 0. || !coverage.iter().any(|v| *v > 0) {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        if self.is_cmyk() {
            return self.cmyk_retouching(
                plates,
                edit,
                coverage,
                selection,
                plan.scratch_bytes,
                limit,
                other,
                job,
            );
        }
        if self.is_float() {
            // The f32 arm (`float_retouch`), after the shared prelude above.
            return self.float_retouching(
                plates,
                edit,
                coverage,
                selection,
                plan.scratch_bytes,
                limit,
                other,
                job,
            );
        }
        let target = self.layer(edit.layer).ok_or("retouch target missing")?;
        let original = target.source().ok_or("retouch target requires pixels")?;
        if original.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("retouch requires u16 storage".into());
        }
        let (dimensions, offset) = edit.extent(self)?;
        let in_place = edit.output == RetouchOutput::Layer
            && (dimensions, offset) == (original.dimensions(), target.offset());
        let staged = if in_place {
            None
        } else {
            Some(self.retouch_destination(edit, target, (dimensions, offset), limit, other, job)?)
        };
        let source = if edit.op != RetouchOp::RedEye {
            Some(plates.retouch_source_with_context(
                edit.source_layer.unwrap_or(edit.layer),
                edit.mode()?,
                edit.ignore_adjustments,
                limit,
                other,
                job.range(0.08, 0.2),
            )?)
        } else {
            None
        };
        job.reserve(plan.scratch_bytes)?;
        // A plate render reaching its own end is only preparation of this
        // operation. Preserve room for synthesis and atomic publication.
        let edit_job = job.range(0.2, 0.98);
        edit_job.report(0.0, 2)?;
        let src = source.as_ref();
        let kernel =
            RetouchKernel::prepare(self, edit, src, coverage, plan.scratch_bytes, edit_job)?;
        let Some((mut pixels, baseline, backdrop)) = staged else {
            let io = (src, coverage, selection);
            return self.retouched_in_place(edit, &kernel, io, target, [job, edit_job]);
        };
        let plane = (dimensions, offset);
        kernel.apply(
            self,
            edit,
            src,
            coverage,
            selection,
            &mut pixels,
            plane,
            edit_job,
        )?;
        let mut changed = false;
        for (i, (a, b)) in pixels.iter().zip(&baseline).enumerate() {
            job.every(i)?;
            changed |= a != b;
        }
        if !changed {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        if backdrop.is_some() {
            for (i, p) in pixels.iter_mut().enumerate() {
                job.every(i)?;
                let threshold = if edit.op == RetouchOp::ContentAwareFill {
                    1
                } else {
                    32768
                };
                if coverage[i] < threshold || *p == baseline[i] {
                    *p = [0; 4];
                }
            }
        }
        let result = if edit.output == RetouchOutput::NewLayer {
            let image = PrecisionImage::from_rgba16_unseeded(
                dimensions.0,
                dimensions.1,
                pixels,
                self.profile.clone(),
                None,
            )?;
            Some(self.retouch_insert(edit.layer, image, edit.output_name())?)
        } else {
            let image = original.rebuilt_rgba16(
                dimensions.0,
                dimensions.1,
                pixels,
                self.profile.clone(),
            )?;
            let mut next = self.replacing_content(edit.layer, Content::Raster { image, offset })?;
            if let Some(next) = next.as_mut() {
                if let Some(comps) = self.comps_after_growth(edit.layer, offset) {
                    next.comps = comps;
                }
            }
            next
        };
        job.report(1., 5)?;
        Ok(result)
    }

    /// A NOT-in-place destination (a new layer, or a grown extent): HEAD's
    /// extent buffer, its baseline copy and the new layer's backdrop plate.
    #[allow(clippy::type_complexity)]
    fn retouch_destination(
        &self,
        edit: &RetouchEdit,
        target: &PrecisionDocumentLayer,
        (dimensions, offset): ((u32, u32), (i32, i32)),
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(Vec<[u16; 4]>, Vec<[u16; 4]>, Option<PrecisionImage>), String> {
        let original = target.source().ok_or("retouch target requires pixels")?;
        let mut pixels = reserved_vec(pixel_count(dimensions.0, dimensions.1)?)?;
        pixels.resize(dimensions.0 as usize * dimensions.1 as usize, [0; 4]);
        let backdrop =
            if edit.output == RetouchOutput::NewLayer && edit.op != RetouchOp::RetouchClone {
                Some(self.retouch_source_with_context(
                    edit.layer,
                    RetouchSampling::CurrentAndBelow,
                    false,
                    limit,
                    other,
                    job.range(0.0, 0.08),
                )?)
            } else {
                None
            };
        if let Some(plate) = &backdrop {
            if plate.storage() != PrecisionStorage::Rgba16Unorm {
                return Err("retouch backdrop requires u16".into());
            }
            let (pw, ph) = plate.dimensions();
            debug_assert_eq!((pw, ph), dimensions);
            let stride = dimensions.0 as usize;
            for y in 0..ph {
                for (x, run) in plate.row_segments(y, 0, pw) {
                    let PrecisionRun::Rgba16(run) = run else {
                        continue;
                    };
                    let at = y as usize * stride + x as usize;
                    pixels[at..at + run.len()].copy_from_slice(run);
                }
            }
        } else if edit.output == RetouchOutput::Layer {
            let ow = original.dimensions().0;
            for y in 0..original.dimensions().1 {
                for (x, run) in original.row_segments(y, 0, ow) {
                    let PrecisionRun::Rgba16(run) = run else {
                        return Err("retouch requires u16 storage".into());
                    };
                    for (k, p) in run.iter().enumerate() {
                        let i = y as usize * ow as usize + x as usize + k;
                        job.every(i)?;
                        let dx = (i % ow as usize) as i64 + i64::from(target.offset().0)
                            - i64::from(offset.0);
                        let dy = (i / ow as usize) as i64 + i64::from(target.offset().1)
                            - i64::from(offset.1);
                        if let Some(di) = canvas_index(dx, dy, dimensions.0, dimensions.1) {
                            pixels[di] = *p;
                        }
                    }
                }
            }
        }
        let baseline = pixels.clone();
        Ok((pixels, baseline, backdrop))
    }

    /// The IN-PLACE arm: the kernel's `bound` on the layer is ONE rect read
    /// and written back through `with_rect_rgba16` (the changed latch); a
    /// bound missing the layer runs the write over an EMPTY plane (HEAD's
    /// solve and refusals, nothing written) and answers `Ok(None)`.
    fn retouched_in_place(
        &self,
        edit: &RetouchEdit,
        kernel: &RetouchKernel,
        (source, coverage, selection): (Option<&PrecisionImage>, &[u16], Option<&[u16]>),
        target: &PrecisionDocumentLayer,
        [job, edit_job]: [JobContext<'_>; 2],
    ) -> Result<Option<Self>, String> {
        let original = target.source().ok_or("retouch target requires pixels")?;
        let (canvas, offset) = (self.dimensions(), target.offset());
        let rect = kernel
            .bound(edit, coverage, canvas)
            .and_then(|b| canvas_rect_on_plane(canvas, original.dimensions(), offset, b));
        let (mut pixels, plane) = match rect {
            Some((x, y, w, h)) => (
                original
                    .read_rect_rgba16(x, y, w, h)
                    .ok_or("could not allocate precision output")?,
                ((w, h), plane_rect_offset(offset, (x, y, w, h))),
            ),
            None => (Vec::new(), ((0, 0), offset)),
        };
        kernel.apply(
            self,
            edit,
            source,
            coverage,
            selection,
            &mut pixels,
            plane,
            edit_job,
        )?;
        let written = rect.map(|(x, y, w, h)| original.with_rect_rgba16(x, y, w, h, &pixels));
        let Some(image) = written.transpose()?.flatten() else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let result = self.replacing_content(edit.layer, Content::Raster { image, offset })?;
        job.report(1.0, 5)?;
        Ok(result)
    }
}
