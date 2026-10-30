//! Deliberate flattened-appearance depth conversion. Legacy compositing runs
//! before byte promotion; typed compositing runs before byte reduction. Neither
//! operation reinterprets a tree in the other evaluation policy. Only the one
//! resulting raster, ICC, PPI, capture packets, canvas chrome and the saved
//! channels survive (alpha and spot, #1: `v · 257` up, nearest down — the
//! selection masks' own rounding); editable structures are intentionally discarded. The
//! GUIDES and the RULER ORIGIN cross in both directions since 12E-1c — the
//! canvas does not move, so `doc::flattening`'s rule applies verbatim and
//! both depths now have somewhere to put them (`precision_guide`).
//!
//! Promotion reserves 64 times aggregate raw layer-description bytes independently of canvas
//! pixels; exact old evaluation precedes multiplication by 257. Contracts and core qualification:
//! `designs/phase-7-conversion-readout.md`.

use image::RgbaImage;
use serde_json::{json, Value};

use crate::doc::DrDocument;
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_raster::LayerRaster;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_quantize::PrecisionDither;
use crate::resources::Inventory;

/// Includes planning, output and simultaneous temporary payloads, separately
/// from the retained immutable input. The host adds its other live owners once.
#[derive(Clone, Copy, Debug)]
pub struct PrecisionConversionPlan {
    pub retained_bytes: u64,
    pub scratch_bytes: u64,
    pub output_bytes: u64,
    pub cooperative_render: bool,
}

impl PrecisionConversionPlan {
    /// A float downconversion holds its f32 projection AND the u16 plane the
    /// tone map writes before either is released; charge both.
    fn widened_for_float(mut self, float: bool, pixels: u64) -> Self {
        if float {
            self.scratch_bytes = self.scratch_bytes.saturating_add(pixels * 8);
        }
        self
    }
    pub fn json(self) -> Value {
        json!({"retained_bytes":self.retained_bytes,"scratch_bytes":self.scratch_bytes,
            "output_bytes":self.output_bytes,"flattened":true,
            "preserved":["rendering_evaluation_before_quantization","icc_profile","print_resolution","capture_metadata","guides","ruler_origin","global_light","saved_channels"],
            "discarded":["editable_layers","live_adjustments","layer_masks"],
            "rgb_quantization":"explicit none or ordered_8x8_v1 on downconversion",
            "alpha_quantization":"nearest, never dithered",
            "channel_quantization":"8→16 v·257; →8 nearest (v+128)/257, never dithered",
            "render_cancellation":"cooperative"})
    }
}

impl DrDocument {
    pub fn promote_flattened_plan(&self) -> Result<PrecisionConversionPlan, String> {
        let n = pixel_count(self.width, self.height)? as u64;
        crate::precision_native_plan::flattened_plan(
            (self.width, self.height),
            &self.profile,
            &self.metadata,
            self.channels.len(),
        )?;
        // Each saved channel is promoted into one `reserved_vec` of n u16
        // samples (`promoted_channels`), held beside the promoted raster.
        let channel_planes = n.saturating_mul(2 * self.channels.len() as u64);
        let mut inventory = Inventory::default();
        inventory.document(self);
        crate::resources::cache_inventory(&[self], &mut inventory);
        let cooperative = crate::inpaint_source::pointwise(self);
        let render = if cooperative {
            // 64K-pixel tiles: accumulator, clipping scratch and byte tile,
            // plus the effective-mask derivations the tiles may build.
            (n * 4 + 4 * BOOKKEEPING_BYTES).saturating_add(
                crate::inpaint_source::tile_scratch_bytes(
                    &self.layers,
                    crate::style_composite::Cut::NONE,
                ),
            )
        } else {
            // The Tree arm (13A-5c-2b) reserves `base + 20·tile + preparation`,
            // `base = 4n + 4·BK`, and `preparation ≤ projection_scratch_bytes −
            // 20n` term by term: masks are the same `MaskProperties::
            // scratch_bytes` plus plane; styles `style_scratch(..).0` ≤ 128·P,
            // 0 for an effect-less style where the projection still charges
            // 16·P (`render_scratch.rs`, `doc_regional_walk::renders_planes`);
            // materialization `16·peak + 5·pixels + planes` ≤ 56·depth·n. So
            // this arm covers the Tree reservation exactly when it also covers
            // `20·tile` against the accumulator's 16n — which fails only where
            // `20·tile > 16n` (W < 1280 with H ≤ 64, or W ≤ 1024 with H < 80),
            // hence the difference added here. The FullFrame fallback's
            // reservation is `projection_scratch_bytes` less the plate.
            let tile = crate::doc_regional_plate::tile_pixels((self.width, self.height));
            crate::render_scratch::projection_scratch_bytes(self)
                .saturating_add(tile.saturating_mul(20).saturating_sub(n.saturating_mul(16)))
        };
        // The description-context parse `sampling_source` reserves in either
        // arm (string lengths only; parses nothing), so the plan covers every
        // reservation the render makes.
        let context = crate::adjust_calibrated::context_scratch(&self.layers, &self.profile)
            .ok_or("layer metadata exceeds the native slot limit")?;
        // Legacy adjustment detection parses arbitrary stored metadata as a
        // serde Value before recognizing the operation. On a tiny canvas that
        // allocation can dwarf all pixel scratch. Charge raw-description size
        // independently, including Vec/map growth and retained parsed LUTs.
        // This conversion-only bound does not change the legacy interpreter.
        let metadata_parse = self
            .layers
            .iter()
            .filter_map(|layer| layer.meta.as_ref())
            .fold(0_u64, |sum, metadata| {
                sum.saturating_add((metadata.len() as u64).saturating_mul(64))
            });
        Ok(PrecisionConversionPlan {
            retained_bytes: inventory.unique_bytes(),
            scratch_bytes: render
                .max(n * 12)
                .saturating_add(metadata_parse)
                .saturating_add(context)
                .saturating_add(4 * BOOKKEEPING_BYTES)
                .saturating_add(channel_planes),
            output_bytes: (n * 8).saturating_add(channel_planes),
            cooperative_render: cooperative,
        })
    }

    pub fn promote_flattened(
        &self,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionDocument, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let plan = self.promote_flattened_plan()?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        // The shared source walk already preserves legacy tile coordinates,
        // clipping/Dissolve and exact full-frame fallback for spatial content.
        let base = u64::from(self.width) * u64::from(self.height) * 4 + 4 * BOOKKEEPING_BYTES;
        let flat = crate::inpaint_source::sampling_source(
            self,
            0,
            crate::retouch_sampling::RetouchSampling::All,
            false,
            base,
            job.range(0.0, 0.6),
        )?
        .ok_or("legacy document has no flattened appearance")?;
        job.reserve(plan.scratch_bytes)?;
        let samples = promote_samples(&flat, job.range(0.6, 0.95))?;
        drop(flat);
        let channels = self.promoted_channels(job.range(0.95, 1.0))?;
        let image = PrecisionImage::from_rgba16_unseeded(
            self.width,
            self.height,
            samples,
            self.profile.clone(),
            None,
        )?;
        let profile = image.profile().clone();
        let mut result = PrecisionDocument::from_parts_and_channels(
            self.width,
            self.height,
            profile,
            self.metadata.clone(),
            self.resolution,
            vec![PrecisionDocumentLayer::raster(
                "Background",
                image,
                (0, 0),
                1.0,
                true,
            )?],
            channels,
        )?;
        // A DEPTH CONVERSION carries the evaluation policy, and the tables are
        // per-depth: the typed transfer is built here for the target depth
        // rather than translated (`doc_evaluation`). The promoted document is
        // one flat raster, so the policy's walk has nothing to refuse.
        if let Some(switched) = result.with_evaluation(self.evaluation())? {
            result = switched;
        }
        // The canvas does not move, so the guides and the ruler origin cross
        // VERBATIM — `doc::flattening`'s reason, now that the typed document
        // has the same two fields (`precision_guide`).
        result.guides.clone_from(&self.guides);
        result.ruler_origin = self.ruler_origin;
        result.color_samplers.clone_from(&self.color_samplers);
        result.paths.entries.clone_from(&self.paths.entries);
        // …and the global light, as the byte flatten keeps it.
        result.global_light = self.global_light;
        job.report(1.0, 5)?;
        Ok(result)
    }
}

pub(crate) fn promote_samples(
    flat: &RgbaImage,
    job: JobContext<'_>,
) -> Result<Vec<[u16; 4]>, String> {
    let count = flat.width() as usize * flat.height() as usize;
    let len = (count * 4).min(flat.as_raw().len());
    promote_runs(
        std::iter::once(flat.as_raw()[..len].as_chunks::<4>().0),
        count,
        job,
    )
}

/// [`promote_samples`] over a layer raster's tiles in place: the runs of each
/// row in plane order, so no whole-plane view is bound.
pub(crate) fn promote_raster(
    raster: &LayerRaster,
    job: JobContext<'_>,
) -> Result<Vec<[u16; 4]>, String> {
    let (width, height) = raster.dimensions();
    let count = width as usize * height as usize;
    promote_runs(raster.runs(), count, job)
}

/// The `v · 257` promotion over runs that concatenate in plane order; `i` is
/// the RUNNING sample index, so every progress report lands on the sample
/// the single-run walk reported at.
fn promote_runs<'a>(
    runs: impl Iterator<Item = &'a [[u8; 4]]>,
    count: usize,
    job: JobContext<'_>,
) -> Result<Vec<[u16; 4]>, String> {
    let mut out = reserved_vec(count)?;
    let mut i = 0usize;
    for run in runs {
        for pixel in run {
            if i & 4095 == 0 {
                job.report(i as f64 / count as f64, 6)?;
            }
            out.push(pixel.map(|v| u16::from(v) * 257));
            i += 1;
        }
    }
    Ok(out)
}

impl PrecisionDocument {
    pub fn downconvert_plan(&self) -> Result<PrecisionConversionPlan, String> {
        let render = self.render_plan()?;
        let n = pixel_count(self.width, self.height)? as u64;
        // Each saved channel narrows into one `reserved_vec` of n bytes
        // (`narrow_channels`), held beside the quantized raster.
        let channel_planes = n.saturating_mul(self.channels.len() as u64);
        Ok(PrecisionConversionPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes: (render.scratch_bytes
                + render
                    .unique_retained_bytes
                    .saturating_sub(self.retained_bytes()))
            .max(n * 12 + 4 * BOOKKEEPING_BYTES)
            .saturating_add(channel_planes),
            output_bytes: (n * 4).saturating_add(channel_planes),
            cooperative_render: true,
        }
        .widened_for_float(self.is_float(), n))
    }

    pub fn downconvert(
        &self,
        dither: PrecisionDither,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<DrDocument, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let plan = self.downconvert_plan()?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        let mut image = self.render_with_context(limit, other, job.range(0.0, 0.8))?;
        // A FLOAT document's projection is extended-range f32: the DELIBERATE
        // tone map is what turns it into SDR codes, and it runs here so the
        // 8-bit conversion and the 16-bit one use the same map
        // (`float_document::tone_mapped_u16`).
        if self.is_float() {
            image = self.tone_mapped_u16(&image, job.range(0.8, 0.85))?;
        }
        debug_assert_eq!(image.storage(), PrecisionStorage::Rgba16Unorm);
        job.reserve(plan.scratch_bytes)?;
        let pixels = image.quantize_with_context(dither, job.range(0.85, 0.95))?;
        let channels = self.narrow_channels(job.range(0.95, 1.0))?;
        let mut result = DrDocument {
            channels,
            profile: self.profile.clone(),
            metadata: self.metadata.clone(),
            resolution: self.resolution,
            ..DrDocument::from_pixels(pixels)
        };
        // The byte twin of the promotion above: the policy carries, its
        // tables are rebuilt at the TARGET depth (`doc_evaluation`), and so
        // do the guides and the ruler origin.
        if let Some(switched) = result.with_evaluation(self.evaluation())? {
            result = switched;
        }
        result.guides = self.guides().to_vec();
        result.ruler_origin = self.ruler_origin();
        result.color_samplers = self.color_samplers().to_vec();
        result.paths.entries.clone_from(&self.paths.entries);
        result.global_light = self.global_light;
        job.check()?;
        Ok(result)
    }
}
