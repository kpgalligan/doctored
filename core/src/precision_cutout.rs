//! Native cutout assembly: Select and Mask's New Layer outputs at 16 bits
//! (the sheet and the `refine_selection` MCP tool).
//!
//! Mirrors doc_cutout.rs's new top-root/mask and original-root hiding rules.
//! Every foreground RGBA16 component survives unchanged, including tiny source
//! alpha. A separate u16 refinement matte remains independently editable; alpha
//! multiplication belongs to precision_render, never to assembly or persistence.
//!
//! `precision_cutout.rs` / `ffi_precision_cutout.rs` independently assemble RGBA16 with unchanged
//! source alpha and a separate native u16 refinement mask. New top-root output hides and retains
//! old roots, shares exact source/profile storage, admits the borrowed matte plus copy, and uses
//! existing native capacity/persistence. Basic typed refinement into that mask is verified end to
//! end (`designs/phase-9i-typed-cutouts-status.md`).
//!
//! **C, at 32 bits.** In a 32-bit float document the foreground is the
//! float composite, RGBA32 float with every bit kept (8.0 stays 8.0), and
//! the u16 matte is its mask, as at 16 bits; New Layer then bakes through the
//! float Apply Layer Mask. The plan's formula is unchanged: the foreground is
//! shared, never copied. The foreground's storage must be the document's own
//! (`Rgba32Float` in a float document, `Rgba16Unorm` otherwise) and the plan
//! reports it.
use crate::{
    doc_mask::MaskProperties,
    job_control::{DrJobControl, JobContext, EVERY_INTERVAL},
    precision_document::{PrecisionDocument, PrecisionDocumentLayer},
    precision_image::{admit, PrecisionImage, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES},
    precision_mask::{PrecisionMask, PrecisionPlane},
    precision_render::MAX_RECORDS,
    resources::Inventory,
    tile_store::TILE_EDGE,
};
use serde_json::{json, Value};

impl PrecisionDocument {
    /// Metadata/resource preflight. Foreground is borrowed and later shared;
    /// the raw caller matte and its new native copy are budgeted separately.
    pub fn refinement_cutout_plan(&self, foreground: &PrecisionImage) -> Result<Value, String> {
        let storage = self.cutout_storage();
        if foreground.dimensions() != self.dimensions()
            || foreground.storage() != storage
            || foreground.profile().bytes() != self.profile().bytes()
        {
            return Err(if self.is_float() {
                "native refinement requires canvas-sized RGBA32 float in the exact document profile"
                    .into()
            } else {
                "native refinement requires canvas-sized RGBA16 in the exact document profile"
                    .into()
            });
        }
        if self.layer_count() >= MAX_RECORDS {
            return Err("native refinement output exceeds the document record limit".into());
        }
        let (mut rasters, mut masks) = self.native_dimensions();
        rasters.push(self.dimensions());
        masks.push(self.dimensions());
        self.native_fits_geometry(self.dimensions(), &rasters, &masks, self.channels.len())?;
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        foreground.record_resources(&mut inventory);
        let matte_bytes = u64::from(self.width) * u64::from(self.height) * 2;
        // Existing group/metadata/native preflight paths are bounded by record
        // caps. No old pixel, channel, mask or retained-original payload clones.
        let scratch = matte_bytes + 8 * BOOKKEEPING_BYTES + self.layer_count() as u64 * 4096;
        Ok(
            json!({"schema_version":1,"retained_bytes":inventory.unique_bytes(),"scratch_bytes":scratch,
            "borrowed_matte_bytes":matte_bytes,"dimensions":[self.width,self.height],"new_layer_index":self.layer_count(),
            "foreground_storage":if self.is_float() { "rgba32_float" } else { "rgba16_unorm" },"matte_storage":"u16_unorm",
            "alpha_contract":"source alpha unchanged at the document depth (RGBA16 or RGBA32 float); separate refinement matte; no premultiplication or early product quantization",
            "advanced_controls_enabled":false}),
        )
    }

    /// Pure internal assembly. A zero mathematical source-alpha/matte product
    /// everywhere is a no-op. A tiny positive product is retained even when an
    /// eventual RGBA16 display render rounds below one code. Borrowed inputs and
    /// original layer/group/source state survive success, cancellation and error.
    pub fn add_refinement_cutout(
        &self,
        foreground: &PrecisionImage,
        matte: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let plan = self.refinement_cutout_plan(foreground)?;
        let n = self.width as usize * self.height as usize;
        if matte.len() != n {
            return Err("native refinement matte must match the canvas".into());
        }
        if foreground.storage() != self.cutout_storage() {
            return Err("native refinement foreground must be the document's storage".into());
        }
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .ok_or("invalid native refinement plan")?;
        let retained = plan["retained_bytes"]
            .as_u64()
            .ok_or("invalid native refinement plan")?;
        let borrowed = plan["borrowed_matte_bytes"]
            .as_u64()
            .ok_or("invalid native refinement plan")?;
        admit(
            retained,
            scratch,
            other
                .checked_add(borrowed)
                .ok_or("native refinement memory overflow")?,
            limit,
        )?;
        job.reserve(scratch)?;
        // TILE-BAND order (each 256-row band, each 256-column span, each
        // row's run), the matte by flat index, every sample visited. The
        // flat walk's `job.every(i)` is hoisted to each run: one check per
        // multiple of `EVERY_INTERVAL` in the run's visit range, so the
        // checkpoints are the flat walk's (13B-3e-3a).
        let (w, h) = foreground.dimensions();
        let mut nonempty = false;
        let mut visited = 0_usize;
        for band in (0..h).step_by(TILE_EDGE as usize) {
            for x0 in (0..w).step_by(TILE_EDGE as usize) {
                let x1 = x0.saturating_add(TILE_EDGE).min(w);
                for y in band..band.saturating_add(TILE_EDGE).min(h) {
                    for (x, run) in foreground.row_segments(y, x0, x1) {
                        let len = match run {
                            PrecisionRun::Rgba16(run) => run.len(),
                            PrecisionRun::Rgba32(run) => run.len(),
                        };
                        let end = visited + len;
                        let mut at = visited.next_multiple_of(EVERY_INTERVAL);
                        while at < end {
                            job.check()?;
                            at += EVERY_INTERVAL;
                        }
                        visited = end;
                        let row = y as usize * w as usize + x as usize;
                        let coverage = &matte[row..row + len];
                        match run {
                            PrecisionRun::Rgba16(run) => {
                                for (pixel, &c) in run.iter().zip(coverage) {
                                    nonempty |= pixel[3] != 0 && c != 0;
                                }
                            }
                            PrecisionRun::Rgba32(run) => {
                                for (pixel, &c) in run.iter().zip(coverage) {
                                    nonempty |= pixel[3] != 0.0 && c != 0;
                                }
                            }
                        }
                    }
                }
            }
        }
        if !nonempty {
            return Ok(None);
        }
        let plane = PrecisionPlane::copy(self.width, self.height, matte, job.range(0.1, 0.8))?;
        let mask = PrecisionMask::new(plane, MaskProperties::at((0, 0)), true)?;
        let image = foreground.with_profile(self.profile.clone());
        let cutout = if self.is_float() {
            PrecisionDocumentLayer::raster_float("Refined Cutout", image, (0, 0), 1.0, true)?
        } else {
            PrecisionDocumentLayer::raster("Refined Cutout", image, (0, 0), 1.0, true)?
        }
        .with_mask(Some(mask));
        let mut layers = Vec::with_capacity(self.layers.len() + 1);
        for layer in self.layers.iter() {
            job.check()?;
            let mut hidden = layer.clone();
            hidden.visible = false;
            layers.push(hidden);
        }
        layers.push(cutout);
        let result = self.rebuilt(
            self.width,
            self.height,
            layers,
            self.channels.as_ref().clone(),
        )?;
        job.report(1.0, 5)?;
        Ok(Some(result))
    }

    /// The foreground storage a cutout takes (module doc, C).
    fn cutout_storage(&self) -> PrecisionStorage {
        if self.is_float() {
            PrecisionStorage::Rgba32Float
        } else {
            PrecisionStorage::Rgba16Unorm
        }
    }
}
