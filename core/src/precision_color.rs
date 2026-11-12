//! Typed profile assignment/conversion around the shared ICC transform.
//! Assignment shares samples; conversion writes each distinct source once,
//! including latent RGB, and preserves alpha exactly. Adjustment parameters
//! stay unchanged and are rebound to the destination profile. As in the byte
//! document, nonlinear live adjustments can therefore change final appearance.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::adjust_calibrated::calibrated_refusal;
use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::icc::IccProfile;
use crate::icc_encode_refusal::{
    artwork_assign_refusal, calibrated_assign_refusal, linear_light_assign_refusal,
};
use crate::icc_transform::PrecisionTransform;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_adjustment::AdjustmentAdmission;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::resources::Inventory;

/// Existing input union and conservative additional peak for an immutable edit.
#[derive(Clone, Copy, Debug)]
pub struct PrecisionColorPlan {
    pub retained_bytes: u64,
    pub scratch_bytes: u64,
    pub converted_pixel_bytes: u64,
}

impl PrecisionDocument {
    /// Borrowing-only preparation budget; never creates an ICC transform or
    /// reparses a stored operator. A later prepared edit validates all new
    /// contexts and native capacity before allocating any converted pixels.
    /// A conversion refuses here when the DESTINATION's matrix model fails
    /// `validate_precision` and is encoded into, so the preflight names
    /// the same sentence the transform would; a failing SOURCE converts out
    /// through its decoder's repairs. Assignment validates only where
    /// the document must then encode into the destination: a
    /// `linear_light` document, then one holding text or shape artwork, then
    /// one holding a calibrated (`document_v2`) Exposure/White Balance layer,
    /// refuses a failing destination in `icc_encode_refusal`'s Assign
    /// words, before any rebind could answer the own-profile sentence. A
    /// 32-bit float document answers `float_color`'s plan instead, and an
    /// Indexed Color document `indexed_profile::plan` (#1 Indexed phase 3a).
    pub fn color_plan(
        &self,
        profile: &Arc<IccProfile>,
        convert: bool,
    ) -> Result<PrecisionColorPlan, String> {
        if self.is_indexed() {
            return crate::indexed_profile::plan(self, profile, convert);
        }
        if self.is_float() {
            return self.float_color_plan(profile, convert);
        }
        if profile.bytes().len() > MAX_DRDC_BLOB_LEN as usize {
            return Err("high-depth profile exceeds the native byte limit".into());
        }
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        inventory.object(&**profile, "profile", false);
        profile.record_resources(&mut inventory);
        let mut scratch = 2 * BOOKKEEPING_BYTES;
        let mut converted = 0;
        // The largest converted plane (`nodes`), charged once more below.
        let mut largest = 0_u64;
        let mut work = 0;
        if self.profile.bytes() != profile.bytes() {
            if !convert {
                if self.linear_light.is_some() {
                    if let Some(refusal) = linear_light_assign_refusal(profile) {
                        return Err(refusal);
                    }
                }
                fn holds_artwork(layers: &[PrecisionDocumentLayer]) -> bool {
                    layers.iter().any(|layer| {
                        layer
                            .description
                            .as_deref()
                            .is_some_and(|text| crate::artwork_document::rerenders(text))
                            || match &layer.content {
                                Content::CmykRaster { .. } | Content::CmykAdjustment(_) => false,
                                Content::Group(children) | Content::PassThrough(children) => {
                                    holds_artwork(children)
                                }
                                Content::Raster { .. } | Content::Adjustment(_) => false,
                            }
                    })
                }
                if holds_artwork(&self.layers) {
                    if let Some(refusal) = artwork_assign_refusal(profile) {
                        return Err(refusal);
                    }
                }
                // The first calibrated layer whose op `destination` refuses
                // (`calibrated_refusal`: a falling-boundary profile admits
                // Exposure but not White Balance, #2).
                fn first_calibrated<'a>(
                    layers: &'a [PrecisionDocumentLayer],
                    destination: &IccProfile,
                ) -> Option<&'a str> {
                    layers.iter().find_map(|layer| match &layer.content {
                        Content::CmykRaster { .. } | Content::CmykAdjustment(_) => None,
                        Content::Adjustment(state) => (state.kernel.reads_profile()
                            && calibrated_refusal(destination, state.kernel.operation()).is_some())
                        .then_some(layer.name.as_str()),
                        Content::Group(children) | Content::PassThrough(children) => {
                            first_calibrated(children, destination)
                        }
                        Content::Raster { .. } => None,
                    })
                }
                if let Some(layer) = first_calibrated(&self.layers, profile) {
                    if let Some(refusal) = calibrated_assign_refusal(profile, layer) {
                        return Err(refusal);
                    }
                }
            }
            if convert {
                let src = self
                    .profile
                    .model()
                    .ok_or("high-depth conversion requires an RGB matrix/TRC source profile")?;
                let dst = profile.model().ok_or(
                    "high-depth conversion requires an RGB matrix/TRC destination profile",
                )?;
                if !src.exact_eq(dst) {
                    dst.validate_precision()?;
                }
                scratch += PrecisionTransform::prepare_scratch_bytes(src, dst);
            }
            /// Each source's interactive re-render plan is summed.
            #[allow(clippy::too_many_arguments)]
            fn nodes(
                layers: &[PrecisionDocumentLayer],
                profile: &Arc<IccProfile>,
                convert: bool,
                seen: &mut HashSet<usize>,
                converted: &mut u64,
                scratch: &mut u64,
                work: &mut u64,
                largest: &mut u64,
                adjustments: &mut AdjustmentAdmission,
            ) -> Result<(), String> {
                for layer in layers {
                    if convert {
                        if let Some(source) = layer.effective_source() {
                            let stage = crate::retained_source_render::SourceStage {
                                asset: &source.asset,
                            };
                            let sampling = crate::retained_source_render::SourceSampling::new(
                                source.asset.dimensions(),
                                source.rendered.dimensions(),
                                source.transform,
                                source.filter,
                            );
                            let color = crate::retained_source_render::profile_cost(
                                source.asset.profile(),
                                profile,
                                true,
                            )?;
                            let dimensions = source.rendered.dimensions();
                            let additional = crate::retained_source_render::work_plan(
                                0, stage, dimensions, 0, sampling, 16, color,
                            )?;
                            *scratch = scratch
                                .checked_add(additional.scratch_bytes)
                                .ok_or("retained source color scratch overflow")?;
                            *work = work
                                .checked_add(additional.work_units)
                                .ok_or("retained source color work overflow")?;
                        }
                    }
                    match &layer.content {
                        Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                            return Err(crate::cmyk_document::refused_sentence(
                                "Convert to Profile",
                            ))
                        }
                        Content::Raster { image, .. } => {
                            if image.storage() != PrecisionStorage::Rgba16Unorm {
                                return Err("high-depth color edit requires a u16 source".into());
                            }
                            if convert && seen.insert(image.plane_key()) {
                                let (w, h) = image.dimensions();
                                let plane = u64::from(w) * u64::from(h) * 8;
                                *converted += plane;
                                *largest = (*largest).max(plane);
                            }
                        }
                        Content::Group(children) | Content::PassThrough(children) => nodes(
                            children,
                            profile,
                            convert,
                            seen,
                            converted,
                            scratch,
                            work,
                            largest,
                            adjustments,
                        )?,
                        Content::Adjustment(state) => adjustments.plan_rebind(state, profile)?,
                    }
                }
                Ok(())
            }
            let mut adjustments = AdjustmentAdmission::default();
            nodes(
                &self.layers,
                profile,
                convert,
                &mut HashSet::new(),
                &mut converted,
                &mut scratch,
                &mut work,
                &mut largest,
                &mut adjustments,
            )?;
            // The rebound records plus ONE preparation allowance (they
            // run one at a time), not one allowance per record. A conversion
            // rebinds ONCE too: `convert_layers` reuses the records
            // `rebind_layers` prepared, so assign and convert charge the
            // same figure.
            let rebound = adjustments.total()?;
            scratch = scratch
                .checked_add(rebound)
                .ok_or("high-depth color preparation byte calculation overflow")?;
        }
        // The converted planes, plus the largest once more: its contiguous
        // buffer is alive while `rebuilt_rgba16` builds the tiled copy. That
        // pixel sum is exact to within a few MB of the measured peak, so it
        // takes a quarter on top (measured in
        // `precision_color_policy`'s `# Admission`). The rebound
        // records stay outside it, charged as assignment charges them.
        let pixels = converted
            .checked_add(largest)
            .ok_or("high-depth color byte calculation overflow")?;
        scratch = scratch
            .checked_add(pixels)
            .and_then(|v| v.checked_add(pixels.div_ceil(4)))
            .ok_or("high-depth color byte calculation overflow")?;
        Ok(PrecisionColorPlan {
            retained_bytes: inventory.unique_bytes(),
            scratch_bytes: scratch,
            converted_pixel_bytes: converted,
        })
    }

    /// Explicit interpretation change. Exact ICC bytes return no edit; no
    /// approximate profile equality is used for high-depth documents.
    ///
    /// Since Phase 11C a RETAINED SOURCE refuses with a sentence rather than
    /// being dropped: the fallback pixels are a render of an original asset
    /// in a specific profile, so retagging them reinterprets a master. The
    /// disclosed way through is `assign_profile_rasterizing`
    /// (`precision_color_policy`).
    pub fn assign_profile(
        &self,
        profile: &Arc<IccProfile>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        if let Some(reason) = self.assign_refusal_for(profile) {
            return Err(reason);
        }
        self.color_edit(profile, false, limit, other, control)
    }

    /// Convert every distinct stored RGB source, preserving alpha and source
    /// sharing. Matrix/TRC conversion is bounded SDR with destination gamut
    /// clipping; no implicit HDR mapping or byte preview participates.
    /// Since Phase 11C this is `convert_to_profile_with` at its DEFAULT
    /// options, which takes the branch below unchanged — the bytes an
    /// existing 16-bit conversion writes are its bytes.
    pub fn convert_to_profile(
        &self,
        profile: &Arc<IccProfile>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        Ok(self
            .convert_to_profile_with(
                profile,
                &crate::doc_color_policy::ConversionOptions::DEFAULT,
                limit,
                other,
                control,
            )?
            .map(|(document, _)| document))
    }

    fn color_edit(
        &self,
        profile: &Arc<IccProfile>,
        convert: bool,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.color_edit_with_context(profile, convert, limit, other, job)
    }

    pub(crate) fn color_edit_with_context(
        &self,
        profile: &Arc<IccProfile>,
        convert: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // An Indexed Color document assigns over its storage and converts its
        // colour table (#1 Indexed phase 3a, `indexed_profile`).
        if self.is_indexed() {
            if !convert {
                return crate::indexed_profile::assign(self, profile, limit, other, job);
            }
            return Ok(crate::indexed_profile::convert(
                self,
                profile,
                &crate::doc_color_policy::ConversionOptions::DEFAULT,
                limit,
                other,
                job,
            )?
            .map(|(document, _)| document));
        }
        if self.is_float() {
            return self.float_color_edit(profile, convert, limit, other, job);
        }
        job.check()?;
        let plan = self.color_plan(profile, convert)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        if self.profile.bytes() == profile.bytes() {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        // Rebind all operators first, including hidden nodes, and preflight the
        // exact resulting native layout. Pixel conversion begins only after
        // every target context is known to be valid and persistable.
        let assigned_layers = rebind_layers(&self.layers, profile, job)?;
        let mut next = Self::from_parts_and_channels(
            self.width,
            self.height,
            profile.clone(),
            self.metadata.clone(),
            self.resolution,
            assigned_layers,
            self.channels.as_ref().clone(),
        )?;
        next.import_warnings = self.import_warnings.clone();
        // The evaluation policy's tables belong to the profile they were built
        // from, so a profile write rebuilds them (`precision_evaluation`).
        next.linear_light = self.linear_light.clone();
        // …and the canvas chrome: a profile edit moves no pixel and no
        // canvas, so the guides and the ruler origin ride along
        // (`precision_guide`).
        next.guides.clone_from(&self.guides);
        next.ruler_origin = self.ruler_origin;
        next.color_samplers.clone_from(&self.color_samplers);
        next.paths.clone_from(&self.paths);
        next.global_light = self.global_light;
        // …and the LAYER COMPS, for the same reason: a profile edit changes
        // no record, so every comp entry still binds by the id its layer
        // kept (`precision_comps`).
        next.comps.clone_from(&self.comps);
        next.background = self.background;
        next.rebind_evaluation()?;
        if convert {
            let src = self
                .profile
                .model()
                .ok_or("high-depth conversion requires an RGB matrix/TRC source profile")?;
            let dst = profile
                .model()
                .ok_or("high-depth conversion requires an RGB matrix/TRC destination profile")?;
            let transform = PrecisionTransform::new(src, dst)?;
            job.report(0.05, 2)?;
            let mut outputs = HashMap::new();
            // The rebound tree: its adjustment records are reused, not
            // prepared a second time.
            let rebound = next.layers.clone();
            let layers = convert_layers(
                &self.layers,
                &rebound,
                profile,
                &transform,
                &mut outputs,
                job,
                plan.converted_pixel_bytes / 8,
            )?;
            next.layers = Arc::new(layers);
            next.native_preflight()?;
        }
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}

pub(crate) fn rebind_layers(
    layers: &[PrecisionDocumentLayer],
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<Vec<PrecisionDocumentLayer>, String> {
    let mut out = reserved_vec(layers.len())?;
    for layer in layers {
        job.check()?;
        let mut next = layer.clone();
        // Assignment reinterprets the fallback. It is an explicit destructive
        // boundary for retained sources; conversion below starts from originals.
        next.retained_source = None;
        next.description = None;
        next.content = match &layer.content {
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                return Err(crate::cmyk_document::refused_sentence("Assign Profile"))
            }
            Content::Raster { image, offset } => Content::Raster {
                image: image.with_profile(profile.clone()),
                offset: *offset,
            },
            Content::Group(children) => {
                Content::Group(Arc::new(rebind_layers(children, profile, job)?))
            }
            Content::PassThrough(children) => {
                Content::PassThrough(Arc::new(rebind_layers(children, profile, job)?))
            }
            Content::Adjustment(state) => Content::Adjustment(state.rebind(profile)?),
        };
        out.push(next);
    }
    Ok(out)
}

const MISMATCH: &str = "internal error: rebound layer tree does not match";

/// `rebound` is `rebind_layers`' output for the same `layers`: each
/// adjustment record is taken from it (an `Arc` clone, no preparation) and
/// groups recurse on the paired children, so a conversion prepares every
/// record once.
fn convert_layers(
    layers: &[PrecisionDocumentLayer],
    rebound: &[PrecisionDocumentLayer],
    profile: &Arc<IccProfile>,
    transform: &PrecisionTransform,
    outputs: &mut HashMap<usize, PrecisionImage>,
    job: JobContext<'_>,
    total_pixels: u64,
) -> Result<Vec<PrecisionDocumentLayer>, String> {
    if rebound.len() != layers.len() {
        return Err(MISMATCH.into());
    }
    let mut out = reserved_vec(layers.len())?;
    for (layer, paired) in layers.iter().zip(rebound) {
        job.check()?;
        let mut next = layer.clone();
        if let Some(source) = layer.effective_source() {
            let source = crate::retained_source_render::render16(source, profile, job)?;
            next.content = Content::Raster {
                image: source.rendered.clone(),
                offset: source.rendered_offset,
            };
            next.retained_source = Some(source);
            out.push(next);
            continue;
        }
        next.retained_source = None;
        next.content = match &layer.content {
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                return Err(crate::cmyk_document::refused_sentence("Convert to Profile"))
            }
            Content::Raster { image, offset } => {
                if image.storage() != PrecisionStorage::Rgba16Unorm {
                    return Err("high-depth color edit requires a u16 source".into());
                }
                let key = image.plane_key();
                let converted = if let Some(prior) = outputs.get(&key) {
                    prior.clone()
                } else {
                    let mut pixels = image.to_rgba16()?;
                    let completed: u64 = outputs
                        .values()
                        .map(|image| {
                            let (w, h) = image.dimensions();
                            u64::from(w) * u64::from(h)
                        })
                        .sum();
                    convert_u16_pixels(&mut pixels, transform, job, completed, total_pixels)?;
                    let (w, h) = image.dimensions();
                    let converted = image.rebuilt_rgba16(w, h, pixels, profile.clone())?;
                    outputs.insert(key, converted.clone());
                    converted
                };
                Content::Raster {
                    image: converted,
                    offset: *offset,
                }
            }
            Content::Group(children) => {
                let Content::Group(paired) = &paired.content else {
                    return Err(MISMATCH.into());
                };
                Content::Group(Arc::new(convert_layers(
                    children,
                    paired,
                    profile,
                    transform,
                    outputs,
                    job,
                    total_pixels,
                )?))
            }
            Content::PassThrough(children) => {
                let Content::PassThrough(paired) = &paired.content else {
                    return Err(MISMATCH.into());
                };
                Content::PassThrough(Arc::new(convert_layers(
                    children,
                    paired,
                    profile,
                    transform,
                    outputs,
                    job,
                    total_pixels,
                )?))
            }
            Content::Adjustment(_) => {
                let Content::Adjustment(state) = &paired.content else {
                    return Err(MISMATCH.into());
                };
                Content::Adjustment(state.clone())
            }
        };
        out.push(next);
    }
    Ok(out)
}

/// The ONE typed profile-conversion kernel over u16 samples, in place: each
/// RGB triple through `transform`, rounded once to u16, with alpha kept
/// exactly. Shared by `convert_layers` and the clipboard's convert arm
/// (`precision_clipboard_convert`). Progress runs over
/// `0.05 + 0.9 · (completed + i) / total_pixels`.
pub(crate) fn convert_u16_pixels(
    pixels: &mut [[u16; 4]],
    transform: &PrecisionTransform,
    job: JobContext<'_>,
    completed: u64,
    total_pixels: u64,
) -> Result<(), String> {
    for (i, pixel) in pixels.iter_mut().enumerate() {
        if i & 4095 == 0 {
            job.report(
                0.05 + 0.9 * (completed + i as u64) as f64 / total_pixels.max(1) as f64,
                6,
            )?;
        }
        let rgb = transform.color(std::array::from_fn(|c| f32::from(pixel[c]) / 65535.0));
        if rgb
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(
                "high-depth color transform produced a nonfinite or out-of-range component".into(),
            );
        }
        *pixel = [
            (rgb[0] * 65535.0).round() as u16,
            (rgb[1] * 65535.0).round() as u16,
            (rgb[2] * 65535.0).round() as u16,
            pixel[3],
        ];
    }
    Ok(())
}
