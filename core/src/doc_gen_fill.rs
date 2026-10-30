//! Generative Fill (M3b): the adapters over `gen_fill` at 8 bits
//! (`DrDocument`) and 16 bits (`PrecisionDocument`, RGB here and 8-bit CMYK
//! through `cmyk_gen_fill`). The host calls them in one job, around the
//! provider's request:
//!
//! - [`gen_fill_frame`] — the native size and the crop for a coverage, before
//!   anything is planned (no document: the host needs the native size to
//!   plan).
//! - `gen_fill_plan` — the job's working set for `variations` pictures: the
//!   kernel's own figure (`gen_fill::scratch_bytes`, which stores each
//!   picture) plus the plate's term. The plate is the full composite with no
//!   cut: byte the canvas plate `4n` plus `inpaint_source::plate_scratch_bound`
//!   (`remove_model_plan`'s byte term); typed the canvas plate in the render
//!   storage, four bookkeeping records and the render plan's scratch
//!   (`retouch_source_plan`'s non-Current arm, without its raster-anchor
//!   check, so any anchor works at both depths).
//! - `gen_fill_input` — the plate (byte `inpaint_source::sampling_source` with
//!   `RetouchSampling::All`; typed `render_cut_with_context` with an empty
//!   cut) read one crop row at a time into `remove_model::native_mean`, then
//!   the two RGBA8 pictures.
//! - `gen_fill_apply` — a NEW canvas-sized raster directly above the anchor
//!   (`InsertPosition::Above`): the provider's FIRST picture, opaque, on the
//!   covered rect and transparent elsewhere, masked by the selection's own
//!   coverage (u16 at 16 bits; at 8 bits each sample rounded to a byte as Sky
//!   Replacement's Sky mask is), named `name` and tagged with `tag` (a
//!   `generated` artwork description: the byte `meta`, the typed
//!   description). Every picture (1 to 4) is kept as the layer's variations
//!   (`doc_gen_variations`), the first one selected. Nothing outside the
//!   selection changes, since the mask is 0 there.
//!
//! `layer` is only the anchor: any record kind (raster, adjustment, group)
//! at both depths. Every call admits its plan against `limit` beside `other`
//! and reserves it on its job control before allocating; the inputs are never
//! changed. A 32-bit float or 16-bit CMYK document refuses by name.
//! On a Grayscale document (#547 phase 6b) the plate is R = G = B in the
//! twin, and the rendered picture, whose colour comes from the provider, is
//! folded to its gray by the Mode rule (`gray_artwork::folded_result`) before
//! the record and its binding are built; the plan carries the fold's image
//! (`gray_artwork::fold_bytes`). On a Lab Color document (#547 Lab phase
//! 6b-iii) the plate's codes pass through the Display P3 view
//! (`lab_neural::view_samples`, Remove's rule) before the provider sees
//! them, and the picture is rendered in Display P3
//! (`lab_neural::result_profile`) and returned to L\*a\*b\* codes by the Mode
//! rule (`lab_neural::published`) before the record and its binding (in the
//! twin) are built; the plan carries the view's tables and that image
//! (`lab_neural::view_tables`, `lab_neural::published_bytes`, which is
//! Gray's fold on Gray).

use std::sync::Arc;

use image::{GrayImage, RgbaImage};
use serde::Deserialize;

use crate::doc::{DrDocument, Layer, LayerRaster};
use crate::doc_gen_variations::{GeneratedBinding16, GeneratedBinding8, GeneratedPlane16};
use crate::doc_mask::MaskProperties;
use crate::gen_fill::{
    check_native, crop, encode_native, fill_at, pick_native, scratch_bytes, RESULT_SIDES,
};
use crate::gen_variations::{check_caps, GeneratedVariations, Variation};
use crate::inpaint_source::{plate_scratch_bound, sampling_source};
use crate::job_control::{DrJobControl, JobContext, JobRun};
use crate::lab_neural;
use crate::layer_insert::InsertPosition;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_mask::{PrecisionMask, PrecisionPlane};
use crate::remove_model::{covered_rect, native_mean, Rect};
use crate::retouch_brush_sampling::plate_samples;
use crate::retouch_sampling::RetouchSampling;
use crate::style_composite::Cut;

const NOT_FLOAT: &str = "generative fill is not available in a 32-bit float document";
const NOT_CMYK16: &str = "generative fill is not available in a 16-bit CMYK document";
const NO_COVERAGE: &str = "the selection covers no pixel of the picture";
const BAD_TAG: &str = "generative fill requires a generated artwork description";
/// The most pictures one request answers (fal's `num_images` cap).
pub(crate) const MAX_RESULTS: usize = 4;

/// One provider picture: straight RGBA8 bytes and its size.
pub(crate) type GenResult<'a> = (&'a [u8], (u32, u32));

pub(crate) fn out_of_range(layer: usize) -> String {
    format!("generative fill layer {layer} is out of range")
}

/// `coverage` holds exactly `n` samples, else the refusal.
fn coverage_length(coverage: &[u16], n: u64) -> Result<(), String> {
    if coverage.len() as u64 != n {
        return Err("invalid generative fill coverage buffer".into());
    }
    Ok(())
}

/// The covered rect, `Err(NO_COVERAGE)` when nothing is covered.
fn covered(coverage: &[u16], width: u32, job: JobContext<'_>) -> Result<Rect, String> {
    Ok(covered_rect(coverage, width, job)?.ok_or(NO_COVERAGE)?)
}

/// A table's `[x, y, w, h]` as a kernel rect.
pub(crate) fn rect(r: [u32; 4]) -> Rect {
    (r[0], r[1], r[2], r[3])
}

/// The native size and crop for a canvas-sized `coverage` on a `canvas`,
/// choosing among the provider's `sizes`: `[crop x, y, w, h, native w,
/// native h]`. `Err` for a coverage of the wrong length, no covered pixel,
/// no size or a size outside `gen_fill::NATIVE_SIDES`.
pub fn gen_fill_frame(
    coverage: &[u16],
    canvas: (u32, u32),
    sizes: &[(u32, u32)],
) -> Result<[u32; 6], String> {
    coverage_length(coverage, u64::from(canvas.0) * u64::from(canvas.1))?;
    for &size in sizes {
        check_native(size)?;
    }
    let rect = covered(coverage, canvas.0, JobContext::NONE)?;
    let native = pick_native(rect, sizes)?;
    let (x, y, w, h) = crop(rect, canvas, native);
    Ok([x, y, w, h, native.0, native.1])
}

/// `tag` is a valid artwork description of kind `generated`, else
/// `BAD_TAG` (or `validate`'s own sentence for a malformed one). The ONE
/// kind check: the variation bindings' live rule asks it too.
pub(crate) fn check_tag(tag: &str) -> Result<(), String> {
    #[derive(Deserialize)]
    struct Kind {
        #[serde(rename = "type")]
        kind: String,
    }
    crate::artwork_document::validate(tag)?;
    let kind: Kind = serde_json::from_str(tag).map_err(|_| BAD_TAG.to_string())?;
    if kind.kind != "generated" {
        return Err(BAD_TAG.into());
    }
    Ok(())
}

/// The name and tag checks every apply makes first.
fn check_output(name: &str, tag: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("generative fill needs a layer name".into());
    }
    check_tag(tag)
}

/// `result` holds a straight RGBA8 picture of `size`, each side inside
/// `gen_fill::RESULT_SIDES`.
fn check_result(result: &[u8], size: (u32, u32)) -> Result<(), String> {
    if !RESULT_SIDES.contains(&size.0) || !RESULT_SIDES.contains(&size.1) {
        return Err(format!(
            "generative fill result must be {}…{} on each side",
            RESULT_SIDES.start(),
            RESULT_SIDES.end()
        ));
    }
    if result.len() as u64 != 4 * u64::from(size.0) * u64::from(size.1) {
        return Err("invalid generative fill result buffer".into());
    }
    Ok(())
}

/// 1 to [`MAX_RESULTS`] pictures, each one [`check_result`] admits.
pub(crate) fn check_results(results: &[GenResult<'_>]) -> Result<(), String> {
    if results.is_empty() || results.len() > MAX_RESULTS {
        return Err(format!("generative fill takes 1 to {MAX_RESULTS} results"));
    }
    for (pixels, size) in results {
        check_result(pixels, *size)?;
    }
    Ok(())
}

/// Every picture of `results` as a stored variation tagged `tag`, polling
/// `job` between them.
pub(crate) fn encode_results(
    results: &[GenResult<'_>],
    tag: &str,
    job: JobContext<'_>,
) -> Result<Vec<Variation>, String> {
    let mut out = Vec::with_capacity(results.len());
    for (pixels, size) in results {
        job.check()?;
        out.push(crate::gen_variations::encode(pixels, *size, tag)?);
    }
    Ok(out)
}

/// [`check_caps`] over `items`.
pub(crate) fn check_items(items: &[Variation]) -> Result<(), String> {
    let lengths: Vec<u64> = items.iter().map(|v| v.png.len() as u64).collect();
    check_caps(&lengths)
}

/// The native planes for `native`, allocated against the reservation.
fn planes(native: (u32, u32)) -> Result<(Vec<f32>, Vec<f32>), String> {
    let cells = native.0 as usize * native.1 as usize;
    let mut image: Vec<f32> = reserved_vec(3 * cells)?;
    image.resize(3 * cells, 0.0);
    let mut mask: Vec<f32> = reserved_vec(cells)?;
    mask.resize(cells, 0.0);
    Ok((image, mask))
}

/// `image_out` / `mask_out` hold `4·nw·nh` bytes each.
fn check_outputs(native: (u32, u32), image_out: &[u8], mask_out: &[u8]) -> Result<(), String> {
    let bytes = 4 * u64::from(native.0) * u64::from(native.1);
    if image_out.len() as u64 != bytes || mask_out.len() as u64 != bytes {
        return Err("invalid generative fill input buffers".into());
    }
    Ok(())
}

pub(crate) fn begin<'a>(
    control: Option<&'a DrJobControl>,
) -> Result<(Option<JobRun<'a>>, JobContext<'a>), String> {
    let run = control.map(DrJobControl::begin).transpose()?;
    Ok((run, control.map_or(JobContext::NONE, JobContext::new)))
}

/// The opaque fill of the covered rect, written through `put(x, y, rgb)`
/// for every pixel of it.
pub(crate) fn fill_rect(
    rect: Rect,
    crop: Rect,
    result: &[u8],
    size: (u32, u32),
    job: JobContext<'_>,
    mut put: impl FnMut(u32, u32, [f64; 3]),
) -> Result<(), String> {
    let (x0, y0, w, h) = rect;
    for y in y0..y0 + h {
        job.check()?;
        for x in x0..x0 + w {
            put(x, y, fill_at(result, size, crop, x, y));
        }
    }
    Ok(())
}

/// The byte raster of `table` from one picture: transparent outside `rect`,
/// the opaque read-back inside it.
pub(crate) fn render_byte(
    table: &GeneratedVariations,
    result: &[u8],
    size: (u32, u32),
    job: JobContext<'_>,
) -> Result<RgbaImage, String> {
    let mut image = RgbaImage::new(table.raster.0, table.raster.1);
    fill_rect(
        rect(table.rect),
        rect(table.crop),
        result,
        size,
        job,
        |x, y, rgb| {
            let [r, g, b] = rgb.map(|v| (v * 255.0).round() as u8);
            image.put_pixel(x, y, image::Rgba([r, g, b, 255]));
        },
    )?;
    Ok(image)
}

/// The u16 raster of `table` from one picture in `profile`: transparent
/// outside `rect`, the opaque read-back inside it.
pub(crate) fn render_rgb16(
    table: &GeneratedVariations,
    result: &[u8],
    size: (u32, u32),
    profile: &Arc<crate::icc::IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = table.raster;
    let width = w as usize;
    let n = width * h as usize;
    let mut codes: Vec<[u16; 4]> = reserved_vec(n)?;
    codes.resize(n, [0; 4]);
    let code = |v: f64| (v * 65535.0).round() as u16;
    fill_rect(
        rect(table.rect),
        rect(table.crop),
        result,
        size,
        job,
        |x, y, rgb| {
            let [r, g, b] = rgb.map(code);
            codes[y as usize * width + x as usize] = [r, g, b, 65535];
        },
    )?;
    PrecisionImage::from_rgba16_unseeded(w, h, codes, profile.clone(), None)
}

impl DrDocument {
    pub(crate) fn gen_fill_retained(&self) -> u64 {
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        inventory.unique_bytes()
    }

    /// The Generative Fill job's working set at `native` for `variations`
    /// pictures (module doc). `Err` for a native size outside
    /// `gen_fill::NATIVE_SIDES` and a metadata parse the plate bound refuses.
    pub fn gen_fill_plan(&self, native: (u32, u32), variations: u32) -> Result<u64, String> {
        check_native(native)?;
        let n = u64::from(self.width) * u64::from(self.height);
        Ok(
            scratch_bytes(self.width, self.height, native, 4, 1, u64::from(variations))
                .saturating_add(n.saturating_mul(4))
                .saturating_add(plate_scratch_bound(self)?),
        )
    }

    /// The provider's two pictures for the canvas-sized `coverage` at
    /// `native` (module doc): writes `image_out` and `mask_out` (RGBA8,
    /// `4·nw·nh` each) and answers the crop `[x, y, w, h]`. `Err` for an
    /// out-of-range anchor, a bad native size or buffer, no covered pixel, a
    /// refused plan and a cancel.
    #[allow(clippy::too_many_arguments)]
    pub fn gen_fill_input(
        &self,
        layer: usize,
        coverage: &[u16],
        native: (u32, u32),
        image_out: &mut [u8],
        mask_out: &mut [u8],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<[u32; 4], String> {
        self.gen_fill_input_at(
            layer, coverage, native, None, 0, image_out, mask_out, limit, other, control,
        )
    }

    /// [`Self::gen_fill_input`] over `fixed` (the crop a table stored) when
    /// given, else the crop the coverage frames; `extra` is the caller's own
    /// working set beside the plan.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn gen_fill_input_at(
        &self,
        layer: usize,
        coverage: &[u16],
        native: (u32, u32),
        fixed: Option<Rect>,
        extra: u64,
        image_out: &mut [u8],
        mask_out: &mut [u8],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<[u32; 4], String> {
        if layer >= self.layers.len() {
            return Err(out_of_range(layer));
        }
        let canvas = (self.width, self.height);
        coverage_length(coverage, u64::from(canvas.0) * u64::from(canvas.1))?;
        let plan = self.gen_fill_plan(native, 0)?.saturating_add(extra);
        check_outputs(native, image_out, mask_out)?;
        let (_run, job) = begin(control)?;
        let crop = match fixed {
            Some(fixed) => fixed,
            None => crop(covered(coverage, canvas.0, job)?, canvas, native),
        };
        admit(self.gen_fill_retained(), plan, other, limit)?;
        job.reserve(plan)?;
        let job = job.with_reservation_floor(plan);
        let plate = sampling_source(
            self,
            layer,
            RetouchSampling::All,
            false,
            self.gen_fill_plan(native, 0)? - plate_scratch_bound(self)?,
            job.range(0.0, 0.5),
        )?
        .ok_or("generative fill could not read the picture")?;
        let (cx, cy, cw, _) = crop;
        let stride = canvas.0 as usize * 4;
        let raw = plate.as_raw();
        let row = |r: u32| {
            let at = (cy + r) as usize * stride + cx as usize * 4;
            Ok(raw[at..at + cw as usize * 4]
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2], p[3]].map(|v| f32::from(v) / 255.0))
                .collect())
        };
        let (mut image, mut mask) = planes(native)?;
        native_mean(
            coverage,
            canvas.0,
            crop,
            native,
            row,
            &mut image,
            &mut mask,
            job.range(0.5, 0.9),
        )?;
        drop(plate);
        let cells = native.0 as usize * native.1 as usize;
        encode_native(
            &image,
            &mask,
            cells,
            image_out,
            mask_out,
            job.range(0.9, 1.0),
        )?;
        job.report(1.0, 5)?;
        Ok([crop.0, crop.1, crop.2, crop.3])
    }

    /// The provider's `results` (1 to 4 straight RGBA8 pictures) on a new
    /// masked layer directly above `layer` (module doc), the first one
    /// rendered and every one kept as a variation, and the new layer's
    /// index. `Err` for an out-of-range anchor, a bad native size, result,
    /// name or tag, no covered pixel, growth past native capacity, a refused
    /// plan, a variation cap, a cancel and a result the `.rz` writer would
    /// refuse.
    #[allow(clippy::too_many_arguments)]
    pub fn gen_fill_apply(
        &self,
        layer: usize,
        coverage: &[u16],
        native: (u32, u32),
        results: &[GenResult<'_>],
        name: &str,
        tag: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<(DrDocument, usize)>, String> {
        if layer >= self.layers.len() {
            return Err(out_of_range(layer));
        }
        let canvas = (self.width, self.height);
        coverage_length(coverage, u64::from(canvas.0) * u64::from(canvas.1))?;
        check_native(native)?;
        check_results(results)?;
        check_output(name, tag)?;
        let slot = self.insert_slot(layer, InsertPosition::Above)?;
        if !self.native_layers_fit_dimensions(
            self.layers
                .iter()
                .map(|l| l.pixels.dimensions())
                .chain(std::iter::once(canvas)),
        ) || !self.native_masks_fit_dimensions(
            self.layers
                .iter()
                .filter_map(|l| l.mask.as_ref().map(|m| m.dimensions()))
                .chain(std::iter::once(canvas)),
        ) {
            return Err("the edited document exceeds native capacity".into());
        }
        let (_run, job) = begin(control)?;
        let covered = covered(coverage, canvas.0, job)?;
        let crop = crop(covered, canvas, native);
        let plan = scratch_bytes(canvas.0, canvas.1, native, 4, 1, results.len() as u64);
        admit(self.gen_fill_retained(), plan, other, limit)?;
        job.reserve(plan)?;
        let items = encode_results(results, tag, job)?;
        check_items(&items)?;
        let table = GeneratedVariations {
            raster: canvas,
            rect: [covered.0, covered.1, covered.2, covered.3],
            crop: [crop.0, crop.1, crop.2, crop.3],
            native,
            ink: false,
            selected: 0,
            items: Arc::new(items),
        };
        self.check_variation_room(table.png_bytes())?;
        let image = render_byte(&table, results[0].0, results[0].1, job)?;
        let n = canvas.0 as usize * canvas.1 as usize;
        let mut mask: Vec<u8> = reserved_vec(n)?;
        for (i, c) in coverage.iter().enumerate() {
            job.every(i)?;
            mask.push(crate::doc_sky_replace::byte(f32::from(*c) / 65535.0));
        }
        let mask = GrayImage::from_raw(canvas.0, canvas.1, mask)
            .ok_or("generative fill could not build its mask")?;
        let mut entry = Layer::from_raster(LayerRaster::unseeded(image), name);
        entry.mask = Some(Arc::new(mask));
        entry.mask_enabled = true;
        entry.mask_properties = MaskProperties::at((0, 0));
        entry.meta = Some(tag.into());
        entry.generated = Some(GeneratedBinding8 {
            table: Arc::new(table),
            rendered: entry.pixels.clone(),
            rendered_offset: entry.offset,
            profile: self.profile.clone(),
        });
        let doc = self
            .inserting_layer(entry, slot)
            .ok_or("generative fill could not insert its layer")?;
        doc.native_save_preflight()?;
        job.report(1.0, 5)?;
        Ok(Some((doc, slot.at)))
    }
}

impl PrecisionDocument {
    /// A float or 16-bit CMYK document refuses by name; 8-bit CMYK runs
    /// through `cmyk_gen_fill`.
    pub(crate) fn gen_fill_depth(&self) -> Result<(), String> {
        if self.is_float() {
            return Err(NOT_FLOAT.into());
        }
        if self
            .cmyk_policy()
            .is_some_and(|p| p.depth() != crate::cmyk_document::CmykDepth::Eight)
        {
            return Err(NOT_CMYK16.into());
        }
        Ok(())
    }

    /// The full composite's plate term (module doc).
    pub(crate) fn gen_fill_plate_term(&self) -> Result<u64, String> {
        let n = u64::from(self.width) * u64::from(self.height);
        Ok(n * self.render_storage().bytes_per_pixel()
            + 4 * BOOKKEEPING_BYTES
            + self.render_plan()?.scratch_bytes)
    }

    /// The 16-bit twin of [`DrDocument::gen_fill_plan`]; a float or 16-bit
    /// CMYK document refuses by name.
    pub fn gen_fill_plan(&self, native: (u32, u32), variations: u32) -> Result<u64, String> {
        self.gen_fill_depth()?;
        check_native(native)?;
        if self.is_cmyk() {
            return self.cmyk_gen_fill_plan(native, variations);
        }
        let (w, h) = self.dimensions();
        Ok(scratch_bytes(w, h, native, 8, 2, u64::from(variations))
            .saturating_add(self.gen_fill_plate_term()?)
            .saturating_add(lab_neural::view_tables(self))
            .saturating_add(lab_neural::published_bytes(
                self,
                u64::from(w) * u64::from(h),
            )))
    }

    /// The 16-bit twin of [`DrDocument::gen_fill_input`].
    #[allow(clippy::too_many_arguments)]
    pub fn gen_fill_input(
        &self,
        layer: usize,
        coverage: &[u16],
        native: (u32, u32),
        image_out: &mut [u8],
        mask_out: &mut [u8],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<[u32; 4], String> {
        self.gen_fill_input_at(
            layer, coverage, native, None, 0, image_out, mask_out, limit, other, control,
        )
    }

    /// The 16-bit twin of [`DrDocument::gen_fill_input_at`].
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn gen_fill_input_at(
        &self,
        layer: usize,
        coverage: &[u16],
        native: (u32, u32),
        fixed: Option<Rect>,
        extra: u64,
        image_out: &mut [u8],
        mask_out: &mut [u8],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<[u32; 4], String> {
        self.gen_fill_depth()?;
        self.layer(layer).ok_or_else(|| out_of_range(layer))?;
        let canvas = self.dimensions();
        coverage_length(coverage, u64::from(canvas.0) * u64::from(canvas.1))?;
        let plan = self.gen_fill_plan(native, 0)?.saturating_add(extra);
        check_outputs(native, image_out, mask_out)?;
        let (_run, job) = begin(control)?;
        let crop = match fixed {
            Some(fixed) => fixed,
            None => crop(covered(coverage, canvas.0, job)?, canvas, native),
        };
        admit(self.retained_bytes(), plan, other, limit)?;
        job.reserve(plan)?;
        let job = job.with_reservation_floor(plan);
        let (mut image, mut mask) = planes(native)?;
        if self.is_cmyk() {
            self.cmyk_gen_fill_planes(
                coverage,
                crop,
                native,
                &mut image,
                &mut mask,
                job.range(0.0, 0.9),
            )?;
        } else {
            let excluded = vec![false; self.records];
            let plate = self.render_cut_with_context(
                Cut::new(&excluded),
                limit,
                other,
                job.range(0.0, 0.5),
            )?;
            let (cx, cy, cw, _) = crop;
            // A Lab plate's codes through the document's view (module doc).
            let view = if self.is_lab() {
                Some(crate::lab_convert::view_pipeline()?)
            } else {
                None
            };
            let row = |r: u32| {
                let mut samples = plate_samples(&plate, (cx, cy + r, cw, 1))?;
                if let Some(view) = &view {
                    lab_neural::view_samples(view, &mut samples);
                }
                Ok(samples)
            };
            native_mean(
                coverage,
                canvas.0,
                crop,
                native,
                row,
                &mut image,
                &mut mask,
                job.range(0.5, 0.9),
            )?;
        }
        let cells = native.0 as usize * native.1 as usize;
        encode_native(
            &image,
            &mask,
            cells,
            image_out,
            mask_out,
            job.range(0.9, 1.0),
        )?;
        job.report(1.0, 5)?;
        Ok([crop.0, crop.1, crop.2, crop.3])
    }

    /// The 16-bit twin of [`DrDocument::gen_fill_apply`]: the new record is a
    /// u16 raster in the document's profile with a u16 mask, the tag its
    /// typed description (on 8-bit CMYK an ink raster, `cmyk_gen_fill`).
    #[allow(clippy::too_many_arguments)]
    pub fn gen_fill_apply(
        &self,
        layer: usize,
        coverage: &[u16],
        native: (u32, u32),
        results: &[GenResult<'_>],
        name: &str,
        tag: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<(PrecisionDocument, usize)>, String> {
        self.gen_fill_depth()?;
        self.layer(layer).ok_or_else(|| out_of_range(layer))?;
        let canvas = self.dimensions();
        coverage_length(coverage, u64::from(canvas.0) * u64::from(canvas.1))?;
        check_native(native)?;
        check_results(results)?;
        check_output(name, tag)?;
        let slot = self.insert_slot(layer, InsertPosition::Above)?;
        self.preflight_raster_addition(canvas, name)?;
        let (mut rasters, mut masks) = self.native_dimensions();
        rasters.push(canvas);
        masks.push(canvas);
        self.native_fits_geometry(canvas, &rasters, &masks, self.channels.len())?;
        let (_run, job) = begin(control)?;
        let covered = covered(coverage, canvas.0, job)?;
        let crop = crop(covered, canvas, native);
        let cmyk = self.is_cmyk();
        let plan = if cmyk {
            self.cmyk_gen_fill_apply_plan(native, results.len() as u64)?
        } else {
            scratch_bytes(canvas.0, canvas.1, native, 8, 2, results.len() as u64).saturating_add(
                lab_neural::published_bytes(self, u64::from(canvas.0) * u64::from(canvas.1)),
            )
        };
        admit(self.retained_bytes(), plan, other, limit)?;
        job.reserve(plan)?;
        let items = encode_results(results, tag, job)?;
        check_items(&items)?;
        let table = GeneratedVariations {
            raster: canvas,
            rect: [covered.0, covered.1, covered.2, covered.3],
            crop: [crop.0, crop.1, crop.2, crop.3],
            native,
            ink: cmyk,
            selected: 0,
            items: Arc::new(items),
        };
        self.check_variation_room(table.png_bytes())?;
        let (pixels, size) = results[0];
        let plane = if cmyk {
            GeneratedPlane16::Cmyk(self.cmyk_gen_fill_render(&table, pixels, size, job)?)
        } else {
            let profile = lab_neural::result_profile(self);
            let rendered = render_rgb16(&table, pixels, size, &profile, job)?;
            GeneratedPlane16::Rgb(lab_neural::published(self, rendered, job)?)
        };
        let n = canvas.0 as usize * canvas.1 as usize;
        let mut samples: Vec<u16> = reserved_vec(n)?;
        samples.extend_from_slice(coverage);
        let mask = PrecisionMask::new(
            PrecisionPlane::adopt(canvas.0, canvas.1, samples)?,
            MaskProperties::at((0, 0)),
            true,
        )?;
        let record = match &plane {
            GeneratedPlane16::Rgb(image) => {
                PrecisionDocumentLayer::raster(name, image.clone(), (0, 0), 1.0, true)?
            }
            GeneratedPlane16::Cmyk(image) => {
                PrecisionDocumentLayer::raster_cmyk(name, image.clone(), (0, 0), 1.0, true)?
            }
        };
        let mut record = record.with_mask(Some(mask));
        record.description = Some(Arc::new(tag.into()));
        record.generated = Some(GeneratedBinding16 {
            table: Arc::new(table),
            rendered: plane,
            rendered_offset: (0, 0),
            profile: self.profile().clone(),
        });
        job.check()?;
        let doc = self.inserting_record(record, slot)?;
        job.report(1.0, 5)?;
        Ok(Some((doc, slot.at)))
    }
}
