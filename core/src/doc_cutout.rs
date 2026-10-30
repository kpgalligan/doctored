//! Refinement output assembly: Select and Mask's New Layer outputs (the
//! sheet and the `refine_selection` MCP tool).
//!
//! A canvas-sized foreground image and separate alpha become one new top-level
//! raster layer with an editable mask. The foreground's bytes are kept exactly,
//! its own alpha included (a composite that is not opaque, such as a previous
//! cutout, is a valid foreground); the refinement alpha is the mask and is
//! never multiplied into the pixels. Original top-level entries are hidden;
//! their pixels, masks, descriptions, structure and other properties survive.
//! The caller commits the returned document in one undo step.
//!
//! Shared native capacity rules, conservative scratch query, bounded
//! copy/cancellation, and a no-op when the product of the mask and the
//! foreground's alpha is zero everywhere (the typed rule,
//! `precision_cutout.rs`).

use crate::{doc::DrDocument, job_control::JobContext, DrImage};
use image::{GrayImage, RgbaImage};
use std::sync::Arc;

impl DrDocument {
    /// New RGBA/mask storage plus cloned document records/strings. Existing
    /// pixel planes, profile, styles and capture metadata remain Arc-shared.
    /// Three layer-record lengths cover insertion reallocation; 1 MiB covers
    /// fixed handle/Arc/cache overhead and capacity-check dimensions vectors.
    /// Borrowed foreground/alpha and later display rendering are excluded.
    pub fn refinement_cutout_scratch_bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 5
            + self.cloned_structure_bytes()
            + 1_048_576
    }

    /// The foreground must match the canvas and contain RGB in this
    /// document's profile (a caller precondition: DrImage has no profile); its
    /// own alpha is kept on the new layer. `alpha · pixel alpha == 0`
    /// everywhere is a no-op; malformed/capacity/cancel inputs are errors.
    /// RGBA and alpha copy in bounded chunks with no partially published output.
    pub(crate) fn add_refinement_cutout(
        &self,
        foreground: &DrImage,
        alpha: &[u8],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        job.check()?;
        let n = self.width as usize * self.height as usize;
        if foreground.pixels.dimensions() != (self.width, self.height) || alpha.len() != n {
            return Err("refinement foreground and alpha must match the document canvas".into());
        }
        let mut nonempty = false;
        for (i, (pixel, &a)) in foreground.pixels.pixels().zip(alpha).enumerate() {
            job.every(i)?;
            nonempty |= a != 0 && pixel[3] != 0;
        }
        if !nonempty {
            return Ok(None);
        }
        let top = self
            .layers
            .len()
            .checked_sub(1)
            .ok_or("refinement output requires a document layer")?;
        let dimensions = (self.width, self.height);
        if !self.native_layers_fit_dimensions(
            self.layers
                .iter()
                .map(|l| l.pixels.dimensions())
                .chain(std::iter::once(dimensions)),
        ) || !self.native_masks_fit_dimensions(
            self.layers
                .iter()
                .filter_map(|l| l.mask.as_ref().map(|m| m.dimensions()))
                .chain(std::iter::once(dimensions)),
        ) {
            return Err("refinement output exceeds native layer or mask capacity".into());
        }
        job.reserve(self.refinement_cutout_scratch_bytes())?;
        // Allocate once, then touch pages in bounded chunks so cancellation
        // does not wait for a full-canvas clone or zero-filled image pass.
        let mut rgba = Vec::with_capacity(n * 4);
        let mut mask = Vec::with_capacity(n);
        for (i, chunk) in foreground.pixels.as_raw().chunks(16_384).enumerate() {
            job.report(0.2 + 0.5 * (i * 16_384) as f64 / (n * 4) as f64, 4)?;
            rgba.extend_from_slice(chunk);
        }
        for (i, chunk) in alpha.chunks(4096).enumerate() {
            job.report(0.7 + 0.2 * (i * 4096) as f64 / n as f64, 4)?;
            mask.extend_from_slice(chunk);
        }
        let pixels = RgbaImage::from_raw(self.width, self.height, rgba)
            .ok_or("invalid refinement foreground dimensions")?;
        let mask = GrayImage::from_raw(self.width, self.height, mask)
            .ok_or("invalid refinement alpha dimensions")?;
        let mut result = self
            .adding_image_layer_with_job(top, pixels, "Refined Cutout", Some(job.range(0.9, 0.98)))?
            .ok_or("refinement layer could not be inserted")?;
        // The existing insertion path owns group invariants and the one plain
        // layer constructor. At the very top the new layer is always depth 0.
        for (i, layer) in result.layers.iter_mut().enumerate() {
            job.every(i)?;
            if i <= top && layer.depth == 0 {
                layer.visible = false;
            }
        }
        let added = result
            .layers
            .last_mut()
            .ok_or("missing refinement output layer")?;
        added.mask = Some(Arc::new(mask));
        if !self.native_edit_fits(&result) {
            return Err("refinement output exceeds native document capacity".into());
        }
        job.report(1.0, 5)?;
        Ok(Some(result))
    }
}
