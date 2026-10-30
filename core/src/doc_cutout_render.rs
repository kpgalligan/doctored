//! Controlled projection for the exact byte-cutout assembly shape
//! (`doc_cutout.rs`).
//!
//! This is a bounded driver of the existing compositor and quantizer, not a
//! second compositing formula. Hidden original roots cannot contribute; the one
//! visible normal raster — with whatever alpha its foreground carried — and its
//! identity-aligned mask render in <=4096-pixel chunks, so the projection
//! equals `flattened()`. A plain mask borrows its Arc in Layer::active_mask, never a new plane.
//!
//! Default mask alignment shares the raw Arc; no per-chunk full mask allocation. Output capacity
//! plus chunk and context-parse workspace are planned before allocation; non-matrix profiles
//! forecast the largest capped metadata slot (the shared `adjust_calibrated::context_scratch`),
//! never parse JSON in the plan. Shared `adjust_calibrated::validate_context_with_job` preserves
//! hidden adjustment/profile refusals and checks between admitted <=16MiB metadata parses; each
//! individual parser call remains noncancellable. The optional-job image-layer insertion uses
//! `document_clone::clone_with_job` for <=64KiB UTF-8-safe owned-string copies while Arc-sharing
//! all immutable planes/sources/blobs. Uncontrolled insertion and general compositor defaults are
//! unchanged.
use crate::{
    blend::BlendMode,
    doc::{composite_layer_into, DrDocument, Layer, LayerKind, MAX_PIXELS},
    drdc::MAX_DRDC_LAYERS,
    job_control::JobContext,
};
use image::RgbaImage;

const CHUNK_PIXELS: usize = 4096;
const BOOKKEEPING_BYTES: u64 = 1_048_576;

impl DrDocument {
    fn refinement_render_layer(&self) -> Result<&Layer, String> {
        let count = u64::from(self.width) * u64::from(self.height);
        if self.width == 0
            || self.height == 0
            || count > MAX_PIXELS
            || self.layers.len() < 2
            || self.layers.len() > MAX_DRDC_LAYERS as usize
            || !crate::doc_group::validate_structure(&self.layers)
        {
            return Err("invalid cutout projection document structure".into());
        }
        let layer = self
            .layers
            .last()
            .ok_or("missing cutout projection layer")?;
        let mask = layer
            .mask
            .as_ref()
            .ok_or("cutout projection requires a mask")?;
        let properties = &layer.mask_properties;
        if layer.kind != LayerKind::Raster
            || layer.depth != 0
            || !layer.visible
            || layer.pixels.dimensions() != (self.width, self.height)
            || layer.offset != (0, 0)
            || layer.opacity != 1.0
            || layer.blend != BlendMode::Normal
            || layer.clipped
            || layer.meta.is_some()
            || layer.style.is_some()
            || layer.retained_source.is_some()
            || !layer.mask_enabled
            || mask.dimensions() != (self.width, self.height)
            || properties.offset != (0, 0)
            || properties.density != 1.0
            || properties.feather != 0.0
            || self.layers[..self.layers.len() - 1]
                .iter()
                .any(|l| l.depth == 0 && l.visible)
        {
            return Err(
                "cutout projection requires one plain visible canvas root with its default mask"
                    .into(),
            );
        }
        Ok(layer)
    }

    fn refinement_context_scratch(&self) -> Result<u64, String> {
        crate::adjust_calibrated::context_scratch(&self.layers, &self.profile)
            .ok_or_else(|| "cutout projection metadata exceeds its native slot limit".into())
    }

    /// Final RGBA8 output, one bounded f32/quantized chunk and any forecast
    /// hidden-metadata parse workspace. Borrowed records/planes and later host
    /// CGImage storage are accounted separately; the plan parses no JSON.
    pub fn refinement_cutout_render_scratch_bytes(&self) -> Result<u64, String> {
        self.refinement_render_layer()?;
        Ok(u64::from(self.width) * u64::from(self.height) * 4
            + CHUNK_PIXELS as u64 * 20
            + BOOKKEEPING_BYTES
            + self.refinement_context_scratch()?)
    }

    pub(crate) fn render_refinement_cutout(
        &self,
        job: JobContext<'_>,
    ) -> Result<RgbaImage, String> {
        job.check()?;
        let scratch = self.refinement_cutout_render_scratch_bytes()?;
        job.reserve(scratch)?;
        // Preserve flattened()'s hidden calibrated-adjustment refusal, but only
        // after the complete parse/output peak is reserved. The plan never
        // materializes metadata JSON. Each native slot is capped at 16 MiB.
        crate::adjust_calibrated::validate_context_with_job(&self.layers, &self.profile, job)?;
        let layer = self
            .layers
            .last()
            .ok_or("missing cutout projection layer")?;
        let count = self.width as usize * self.height as usize;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(count * 4)
            .map_err(|_| "could not allocate cutout projection")?;
        // The evaluation policy's transfer and its refusal walk
        // (`doc_evaluation`), beside the hidden calibrated-adjustment refusal
        // above: both are conditions on the projection this builds.
        let transfer = self.transfer()?;
        let env = self.composite_env(None, transfer);
        for y in 0..self.height {
            let mut x = 0;
            while x < self.width {
                let width = (self.width - x).min(CHUNK_PIXELS as u32);
                let first = y as usize * self.width as usize + x as usize;
                job.report(first as f64 / count as f64, 6)?;
                let mut acc = vec![[0.0_f32; 4]; width as usize];
                composite_layer_into(&mut acc, width, 1, (x as i32, y as i32), layer, env)?;
                let chunk = crate::doc_evaluation::encode_projection(&acc, width, 1, transfer);
                bytes.extend_from_slice(chunk.as_raw());
                x += width;
            }
        }
        job.report(1.0, 5)?;
        RgbaImage::from_raw(self.width, self.height, bytes)
            .ok_or("invalid cutout projection dimensions".into())
    }
}
