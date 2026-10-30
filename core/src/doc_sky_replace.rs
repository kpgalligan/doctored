//! Edit ▸ Sky Replacement (M2a): the adapters over `sky_harmonize`.
//! The doc ops read one raster layer on the canvas, replace its sky and add
//! the result as ONE new canvas-sized raster layer at (0, 0), Normal, opacity
//! 1, directly above the source (`InsertPosition::Above`), named
//! `params.name` — the "Duplicate Layer" output. The image op is the
//! dialog's preview: any foreground size, the coverage read bilinearly and
//! the edge widths taken from the coverage's size (`sky_harmonize::
//! edge_radii`), so a reduced proxy shows the commit's widths.
//!
//! At 8 bits the doc op is the image op on `layer_canvas_image` of the
//! source, byte for byte. At 16 bits the record is read in place at its
//! offset (no canvas copy). At 32 bits and in CMYK the command refuses by
//! name. A group or adjustment layer
//! refuses by name. Every op is planned (`sky_harmonize::scratch_bytes`),
//! admitted against `limit` beside `other` and reserved on its job control
//! before any plane is allocated; the inputs are never changed.
//!
//! `params.output` `"new_layers"` (M2b) dispatches both doc ops to
//! `doc_sky_group`'s `sky_replace_group` right after the layer, coverage and
//! sky-size checks, so the refusals of the inputs are the same for both
//! outputs and the Duplicate Layer path's slot, plan and job never run for
//! it. The image op (the preview) ignores `output`: the group composites to
//! the same picture.
//!
//! `params.decontaminate` needs nothing here: `sky_harmonize::
//! replace` composites `Prepared::foreground`, so the preview and the
//! Duplicate Layer output carry the decontaminated edge, the colour the New
//! Layers group's "Foreground Edge" child holds.

use image::RgbaImage;

use crate::doc::{DrDocument, Layer, LayerRaster};
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::InsertPosition;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, reserved_vec, unit_to_u16, PrecisionImage};
use crate::sky_harmonize::{coverage_at, replace, scratch_bytes, SkyOutput, SkyParams, NO_SKY};

/// The refusal for a group or adjustment layer.
pub(crate) fn not_raster(layer: usize) -> String {
    format!("sky replacement needs a raster layer; layer {layer} is not one")
}

/// The kernel's no-sky sentence with the layer named; anything else as is.
pub(crate) fn named(error: String, layer: usize) -> String {
    if error == NO_SKY {
        format!("{NO_SKY} on layer {layer}")
    } else {
        error
    }
}

fn coverage_length(coverage: &[u16], dimensions: (u32, u32)) -> Result<(), String> {
    if coverage.len() as u64 != u64::from(dimensions.0) * u64::from(dimensions.1) {
        return Err("invalid sky replacement coverage buffer".into());
    }
    Ok(())
}

pub(crate) fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The dialog's preview: `foreground` with its sky replaced by `sky`, the
/// coverage (`coverage_dims` sized, any size) read at each foreground pixel
/// centre. `Err` for a refused plan, a bad coverage buffer, bad params, no
/// sky weight and a cancel.
#[allow(clippy::too_many_arguments)]
pub fn sky_replace_image(
    foreground: &RgbaImage,
    sky: &RgbaImage,
    coverage: &[u16],
    coverage_dims: (u32, u32),
    params: &str,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<RgbaImage, String> {
    let p = SkyParams::parse(params)?;
    coverage_length(coverage, coverage_dims)?;
    let dims = foreground.dimensions();
    crate::precision_image::pixel_count(dims.0, dims.1)?;
    crate::precision_image::pixel_count(sky.width(), sky.height())?;
    let scratch = scratch_bytes(dims, sky.dimensions(), 4);
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let retained = (foreground.as_raw().len() + sky.as_raw().len()) as u64;
    admit(retained, scratch, other, limit)?;
    job.reserve(scratch)?;
    let image = replaced_bytes(foreground, sky, coverage, coverage_dims, &p, job).map_err(|e| {
        if e == NO_SKY {
            format!("{NO_SKY} in the image")
        } else {
            e
        }
    })?;
    job.report(1.0, 5)?;
    Ok(image)
}

/// The byte replacement of `foreground`, quantized once.
fn replaced_bytes(
    foreground: &RgbaImage,
    sky: &RgbaImage,
    coverage: &[u16],
    coverage_dims: (u32, u32),
    p: &SkyParams,
    job: JobContext<'_>,
) -> Result<RgbaImage, String> {
    let dims = foreground.dimensions();
    let m0 = coverage_at(coverage, coverage_dims, dims, job.checkpoints_only())?;
    let source = foreground.as_raw();
    let read = |i: usize| {
        let at = i * 4;
        [
            f32::from(source[at]) / 255.0,
            f32::from(source[at + 1]) / 255.0,
            f32::from(source[at + 2]) / 255.0,
            f32::from(source[at + 3]) / 255.0,
        ]
    };
    let mut out: Vec<u8> = reserved_vec(source.len())?;
    replace(
        read,
        dims,
        &m0,
        coverage_dims,
        sky.as_raw(),
        sky.dimensions(),
        p,
        job,
        |_, pixel| out.extend(pixel.map(byte)),
    )?;
    RgbaImage::from_raw(dims.0, dims.1, out).ok_or_else(|| "sky replacement output".into())
}

impl DrDocument {
    pub(crate) fn sky_retained(&self) -> u64 {
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        inventory.unique_bytes()
    }

    /// Sky Replacement on layer `layer` (module doc) with the canvas-sized
    /// `coverage`: the document with the new layer (or, for New Layers, the
    /// new group) above it. `Err` for an
    /// out-of-range or non-raster layer, bad params or coverage, growth past
    /// native capacity, a refused plan, no sky weight and a cancel.
    #[allow(clippy::too_many_arguments)]
    pub fn sky_replace(
        &self,
        layer: usize,
        sky: &RgbaImage,
        coverage: &[u16],
        params: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<DrDocument, String> {
        let p = SkyParams::parse(params)?;
        let source = self
            .layers
            .get(layer)
            .ok_or_else(|| format!("sky replacement layer {layer} is out of range"))?;
        if self.raster_layer(layer).is_none() || crate::retouch_sampling::is_adjustment(source) {
            return Err(not_raster(layer));
        }
        let dims = (self.width, self.height);
        coverage_length(coverage, dims)?;
        crate::precision_image::pixel_count(sky.width(), sky.height())?;
        if p.output == SkyOutput::NewLayers {
            return self.sky_replace_group(layer, sky, coverage, &p, limit, other, control);
        }
        let slot = self.insert_slot(layer, InsertPosition::Above)?;
        if !self.native_layers_fit_dimensions(
            self.layers
                .iter()
                .map(|l| l.pixels.dimensions())
                .chain(std::iter::once(dims)),
        ) {
            return Err("the edited document exceeds native capacity".into());
        }
        let scratch = scratch_bytes(dims, sky.dimensions(), 4);
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        admit(self.sky_retained(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let canvas = self
            .layer_canvas_image(layer)
            .ok_or_else(|| not_raster(layer))?;
        let image =
            replaced_bytes(&canvas, sky, coverage, dims, &p, job).map_err(|e| named(e, layer))?;
        drop(canvas);
        let added = Layer::from_raster(LayerRaster::unseeded(image), &p.name);
        let doc = self
            .inserting_layer(added, slot)
            .ok_or("sky replacement could not insert its layer")?;
        job.report(1.0, 5)?;
        Ok(doc)
    }
}

impl PrecisionDocument {
    /// Sky Replacement on a 16-bit document (module doc): the new record is a
    /// u16 raster in the document's profile. `Err` as the byte op's, plus a
    /// CMYK, 32-bit float or Lab Color document by name.
    #[allow(clippy::too_many_arguments)]
    pub fn sky_replace(
        &self,
        layer: usize,
        sky: &RgbaImage,
        coverage: &[u16],
        params: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionDocument, String> {
        if self.is_cmyk() {
            return Err("sky replacement is not available in a CMYK document".into());
        }
        if self.is_float() {
            return Err("sky replacement is not available in a 32-bit document".into());
        }
        // #1 Lab phase 6b-iii: refused for good, a Doctored decision
        // (`designs/lab-mode.md`).
        if self.is_lab() {
            return Err("sky replacement is not available in a Lab Color document".into());
        }
        let p = SkyParams::parse(params)?;
        let record = self
            .layer(layer)
            .ok_or_else(|| format!("sky replacement layer {layer} is out of range"))?;
        if record.children().is_some() {
            return Err(not_raster(layer));
        }
        let source = record.source().ok_or_else(|| not_raster(layer))?;
        let dims = self.dimensions();
        coverage_length(coverage, dims)?;
        crate::precision_image::pixel_count(sky.width(), sky.height())?;
        if p.output == SkyOutput::NewLayers {
            return self.sky_replace_group(layer, sky, coverage, &p, limit, other, control);
        }
        let slot = self.insert_slot(layer, InsertPosition::Above)?;
        self.preflight_raster_addition(dims, &p.name)?;
        let scratch = scratch_bytes(dims, sky.dimensions(), 8);
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let m0 = coverage_at(coverage, dims, dims, job.checkpoints_only())?;
        let (offset, (sw, sh)) = (record.offset(), source.dimensions());
        let width = dims.0 as usize;
        let read = |i: usize| {
            let x = (i % width) as i64 - i64::from(offset.0);
            let y = (i / width) as i64 - i64::from(offset.1);
            if x < 0 || y < 0 || x >= i64::from(sw) || y >= i64::from(sh) {
                return [0.0; 4];
            }
            source.sample_at(x as u32, y as u32).unwrap_or([0.0; 4])
        };
        let mut codes: Vec<[u16; 4]> = reserved_vec(m0.len())?;
        replace(
            read,
            dims,
            &m0,
            dims,
            sky.as_raw(),
            sky.dimensions(),
            &p,
            job,
            |_, pixel| codes.push(pixel.map(unit_to_u16)),
        )
        .map_err(|e| named(e, layer))?;
        drop(m0);
        let image = PrecisionImage::from_rgba16_unseeded(
            dims.0,
            dims.1,
            codes,
            self.profile().clone(),
            None,
        )?;
        let added = PrecisionDocumentLayer::raster(&p.name, image, (0, 0), 1.0, true)?;
        let doc = self.inserting_record(added, slot)?;
        job.report(1.0, 5)?;
        Ok(doc)
    }
}
