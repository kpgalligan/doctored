//! Assign Profile and Convert to Profile in a 32-bit float document:
//! the float arms of `precision_color`'s `color_plan` and
//! `color_edit_with_context`, as a thin adapter over the ONE float
//! conversion, `precision_clipboard_promote::promoted_image`. No pixel math
//! lives here.
//!
//! # The rule
//!
//! A float document stores LINEAR light in its profile's primaries
//! (`float_document`), so a profile edit never runs a tone curve.
//!
//! - ASSIGN relabels: every record keeps its samples bit for bit (shared, not
//!   copied) under the new profile (`precision_color::rebind_layers`, which
//!   clears each record's description as the 16-bit Assign does; the host
//!   re-renders artwork from the descriptions it captured). The numbers now
//!   mean the new profile's primaries.
//! - CONVERT keeps the colour: each distinct raster plane goes through
//!   `promoted_image` under Convert once — the matrix
//!   `inv(destination) · source` on linear RGB in f64, one rounding to f32,
//!   alpha kept, NO clamp. A colour outside the destination gamut keeps a
//!   negative component and a value above white stays above white. A record
//!   bound to a master (a RAW layer) is re-rendered from it
//!   (`retained_source_float::render_typed`) and keeps its binding; every
//!   other record keeps its description, mask, style and every other field.
//!   Adjustment records are rebound to the destination with their parameters
//!   unchanged.
//!
//! Both keep the document's tone map and rebuild its policy for the new
//! profile (`from_parts_float`), and carry the import warnings, guides, ruler
//! origin, colour samplers, paths, global light and layer comps.
//!
//! # Refusals
//!
//! The document's own profile bytes are no change (`Ok(None)`), before every
//! refusal but the plan's admission. Otherwise, by name:
//!
//! - a profile with no matrix/TRC model, Assign or Convert
//!   (`icc_encode_refusal::float_model_sentence`): a LUT has no primaries for
//!   the linear values to be defined by;
//! - Assign of a profile whose tone curves fail the precision check
//!   (`float_assign_curve_sentence`): display and SDR export would have to
//!   encode through them. Convert into one answers the existing
//!   `destination_sentence("convert", ..)`;
//! - Convert with absolute colorimetric intent, black-point compensation or
//!   preserve-appearance adjustments ([`options_refusal`]): each needs a rule
//!   for linear extended-range values that is not decided. Relative,
//!   perceptual and saturation with BPC off and keep-parameters are admitted
//!   and give the same samples (a matrix pair has one conversion);
//! - a converted component outside ±65504 ([`range_refusal`]), refused rather
//!   than clipped (the float storage rule). A bound record whose master
//!   leaves the range when re-rendered into the destination answers the same
//!   sentence, not the place's.
//!
//! # Admission
//!
//! Scratch is `2·BOOKKEEPING_BYTES` plus the rebound adjustment records
//! (`AdjustmentAdmission`, as `color_plan` charges them) plus, for a Convert
//! to another profile, `P + ⌈P/4⌉`: `P` is every distinct raster plane at
//! 16 B/px plus the largest plane `FLOAT_REBUILD_COPIES` more times (its
//! contiguous vector is alive while its tiles are built). A bound record
//! counts as a plane of its render's dimensions, and the largest such
//! record's render scratch (`retained_source_render::work_plan` at fallback
//! depth 32 plus `retained_source_float::float_render_scratch`, the terms
//! `layer_clipboard_float::promotion_terms` charges a re-rendered record) is
//! added on top. An Assign converts nothing and charges no plane.
//!
//! Measured with `MallocLargeCache=0 DR_FLOAT_CASE=convert make t
//! F=float_profile_edit T=admission_measure A='--ignored --nocapture'`
//! (Release, one case per process, three runs, working limit 8,589,934,592):
//! a 4000×3000 sRGB float document with one opaque non-uniform RGBA32F layer,
//! converted to Display P3 through `dr_precision_doc_convert_to_profile`.
//! `E` is `ri_interval_max_phys_footprint − ri_phys_footprint` after
//! `proc_reset_footprint_interval`, with the result held until it is read.
//!
//! | case     | scratch (plan) | max E of 3  | ⌈1.25 · max E⌉ |
//! |----------|----------------|-------------|----------------|
//! | baseline | —              | 0           | —              |
//! | convert  | 483,158,548    | 385,007,904 | 481,259,880    |
//!
//! `baseline` is the twin: the same fixture with no edit run. The plan is
//! `dr_precision_doc_color_plan`'s `scratch_bytes` (convert true), which adds
//! the profile argument's allowance to the formula above. E is the converted
//! plane beside its tiles (384,000,000) plus under 1 MB, and the plan passed
//! its floor in the first round, so `FLOAT_REBUILD_COPIES` stays 1. Four
//! rounds (the third after the walk gained its per-plane progress reports,
//! the fourth on 2026-10-12 with max E 384,975,136) agree within 33 KB; the
//! table holds the largest.
//!
//! `core/tests/float_profile_edit.rs`'s `plans_cover_the_measured_peaks`
//! holds the same `max E` and fails when the plan drops below its floor.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::doc_color_policy::{AdjustmentPolicy, ConversionOptions};
use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::float_document::FLOAT_RANGE_MAX;
use crate::icc::IccProfile;
use crate::icc_encode_refusal::{
    destination_sentence, fails_precision, float_assign_curve_sentence, float_model_sentence,
};
use crate::job_control::JobContext;
use crate::precision_adjustment::AdjustmentAdmission;
use crate::precision_clipboard::PasteProfile;
use crate::precision_clipboard_promote::{promoted_image, RANGE_REFUSAL_PREFIX};
use crate::precision_color::{rebind_layers, PrecisionColorPlan};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::resources::Inventory;
use crate::retained_source_float::{
    float_render_scratch, render_typed, FLOAT_MASTER_RANGE_REFUSAL,
};
use crate::retained_source_render::{profile_cost, work_plan, SourceSampling, SourceStage};

/// How many more times the plan charges the LARGEST converted plane on top
/// of the sum of every plane (module doc's `# Admission`): its contiguous
/// vector beside the tiles built from it. The measured-admission table is
/// what it is checked against; a miss raises this constant.
///
/// Re-measured 2026-10-12 (baseline twin E = 0; three convert runs, each
/// E = 384,975,136): max E 384,975,136, floor ⌈1.25 · max E⌉ = 481,218,920,
/// plan 483,158,548, so the plan clears its floor by 1,939,628 bytes. Against
/// the largest max E of every round (385,007,904, the module doc's table;
/// floor 481,259,880) the margin is 1,898,668 bytes. The constant stays 1.
const FLOAT_REBUILD_COPIES: u64 = 1;

const OVERFLOW: &str = "float color byte calculation overflow";
const STORAGE: &str = "a 32-bit float color edit requires rgba32_float sources";
/// `precision_color`'s `MISMATCH` (the master copy), for the twin walk below.
const MISMATCH: &str = "internal error: rebound layer tree does not match";
/// The refusal of a Convert OUT of a float document whose own profile has no
/// matrix/TRC model, in Convert words (the 16-bit plan's source sentence),
/// asked by the plan so the edit never reaches `promoted_image`'s clipboard
/// wording. Not reachable today: `FloatPolicy::for_profile` already requires
/// a model of every float document's profile.
const SOURCE_MODEL: &str =
    "converting a 32-bit float document requires an RGB matrix/TRC source profile";

/// Why a float document cannot take `profile` by Assign (`convert` false) or
/// Convert (`convert` true), or `None`: no matrix/TRC model, then tone curves
/// that fail the precision check (module doc's `# Refusals`).
pub(crate) fn profile_refusal(profile: &IccProfile, convert: bool) -> Option<String> {
    if profile.model().is_none() {
        return Some(float_model_sentence(convert, profile.name()));
    }
    fails_precision(profile).then(|| {
        if convert {
            destination_sentence("convert", profile.name())
        } else {
            float_assign_curve_sentence(profile.name())
        }
    })
}

/// Why a float Convert cannot run under `options`, or `None`: the first of
/// absolute colorimetric intent, black-point compensation and
/// preserve-appearance adjustments (module doc's `# Refusals`).
/// `rasterize_sources` is not an option of the conversion itself.
pub(crate) fn options_refusal(options: &ConversionOptions) -> Option<String> {
    let what = if options.intent.is_absolute() {
        "absolute colorimetric intent"
    } else if options.black_point_compensation {
        "black-point compensation"
    } else if options.adjustments == AdjustmentPolicy::PreserveAppearance {
        "preserve-appearance adjustments"
    } else {
        return None;
    };
    Some(format!(
        "Convert to Profile in a 32-bit float document converts in linear light with no clamp \
         (relative colorimetric, no black-point compensation): {what} is unavailable there."
    ))
}

/// The refusal of a Convert into the profile `name` whose result leaves the
/// stored float range: `promoted_image`'s clipboard sentence reworded, and a
/// bound record's `FLOAT_MASTER_RANGE_REFUSAL` (its master re-rendered into
/// the destination) on the same terms.
pub(crate) fn range_refusal(name: &str) -> String {
    format!(
        "the document's colours convert to a value outside the 32-bit float range \
         (±{FLOAT_RANGE_MAX}) in \u{201c}{name}\u{201d}; the conversion is refused rather than \
         clipped"
    )
}

/// What the plan's walk sums.
struct Count<'a> {
    profile: &'a Arc<IccProfile>,
    convert: bool,
    seen: HashSet<usize>,
    /// Every converted plane, bytes.
    converted: u64,
    /// The largest converted plane, bytes.
    largest: u64,
    /// The largest bound record's render scratch, bytes.
    render: u64,
    adjustments: AdjustmentAdmission,
}

fn count(layers: &[PrecisionDocumentLayer], state: &mut Count<'_>) -> Result<(), String> {
    for layer in layers {
        match &layer.content {
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                return Err(crate::cmyk_document::refused_sentence("Convert to Profile"))
            }
            Content::Raster { image, .. } => {
                if image.storage() != PrecisionStorage::Rgba32Float {
                    return Err(STORAGE.into());
                }
                if !state.convert {
                    continue;
                }
                let pixels = if let Some(source) = layer.effective_source() {
                    // A bound record is re-rendered from its master: a plane
                    // of its render's dimensions and the render's own scratch
                    // (`layer_clipboard_float::promotion_terms`' terms; the
                    // Convert runs under its caller's job control, so the
                    // plan carries no work refusal).
                    let size = source.asset.dimensions();
                    let dest = crate::source_transform::bounds(size, source.transform)?;
                    let plan = work_plan(
                        0,
                        SourceStage {
                            asset: &source.asset,
                        },
                        (dest.2, dest.3),
                        0,
                        SourceSampling::new(
                            size,
                            (dest.2, dest.3),
                            source.transform,
                            source.filter,
                        ),
                        32,
                        profile_cost(source.asset.profile(), state.profile, true)?,
                    )?;
                    let render = plan
                        .scratch_bytes
                        .checked_add(float_render_scratch(size, (dest.2, dest.3))?)
                        .ok_or(OVERFLOW)?;
                    state.render = state.render.max(render);
                    pixel_count(dest.2, dest.3)?
                } else if state.seen.insert(image.plane_key()) {
                    let (w, h) = image.dimensions();
                    pixel_count(w, h)?
                } else {
                    continue;
                };
                let plane = (pixels as u64)
                    .checked_mul(PrecisionStorage::Rgba32Float.bytes_per_pixel())
                    .ok_or(OVERFLOW)?;
                state.converted = state.converted.checked_add(plane).ok_or(OVERFLOW)?;
                state.largest = state.largest.max(plane);
            }
            Content::Group(children) | Content::PassThrough(children) => count(children, state)?,
            Content::Adjustment(adjustment) => {
                state.adjustments.plan_rebind(adjustment, state.profile)?
            }
        }
    }
    Ok(())
}

/// The float twin of `precision_color::convert_layers`, which is the master
/// copy of this walk: the ORIGINAL records paired with `rebound`
/// (`rebind_layers`' output for the same `layers`). Each record starts from
/// its original, so its description, mask, style and every other field are
/// kept; only adjustment records are taken from the rebound tree. A bound
/// record is re-rendered from its master and keeps its binding; every other
/// raster plane goes through `promoted_image` once per `plane_key`. Progress
/// is reported after each converted plane, at
/// `0.05 + 0.9 · (bytes of the planes in outputs) / total_bytes`, the master
/// copy's scale by plane rather than by pixel (`promoted_image` polls
/// cancellation and reports nothing); `total_bytes` is the plan's
/// `converted_pixel_bytes`.
fn convert_layers(
    layers: &[PrecisionDocumentLayer],
    rebound: &[PrecisionDocumentLayer],
    profile: &Arc<IccProfile>,
    outputs: &mut HashMap<usize, PrecisionImage>,
    job: JobContext<'_>,
    total_bytes: u64,
) -> Result<Vec<PrecisionDocumentLayer>, String> {
    if rebound.len() != layers.len() {
        return Err(MISMATCH.into());
    }
    let mut out = reserved_vec(layers.len())?;
    for (layer, paired) in layers.iter().zip(rebound) {
        job.check()?;
        let mut next = layer.clone();
        if let Some(source) = layer.effective_source() {
            let source = render_typed(source, profile, true, job)?;
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
                if image.storage() != PrecisionStorage::Rgba32Float {
                    return Err(STORAGE.into());
                }
                let key = image.plane_key();
                let converted = match outputs.get(&key) {
                    Some(prior) => prior.clone(),
                    None => {
                        let converted = promoted_image(image, profile, PasteProfile::Convert, job)?;
                        outputs.insert(key, converted.clone());
                        let completed: u64 = outputs
                            .values()
                            .map(|image| {
                                let (w, h) = image.dimensions();
                                u64::from(w)
                                    * u64::from(h)
                                    * PrecisionStorage::Rgba32Float.bytes_per_pixel()
                            })
                            .sum();
                        job.report(0.05 + 0.9 * completed as f64 / total_bytes.max(1) as f64, 6)?;
                        converted
                    }
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
                    outputs,
                    job,
                    total_bytes,
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
                    outputs,
                    job,
                    total_bytes,
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

impl PrecisionDocument {
    /// `color_plan`'s float arm (module doc's `# Refusals` and
    /// `# Admission`). Borrowing only: nothing is converted or prepared.
    pub(crate) fn float_color_plan(
        &self,
        profile: &Arc<IccProfile>,
        convert: bool,
    ) -> Result<PrecisionColorPlan, String> {
        if profile.bytes().len() > MAX_DRDC_BLOB_LEN as usize {
            return Err("high-depth profile exceeds the native byte limit".into());
        }
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        inventory.object(&**profile, "profile", false);
        profile.record_resources(&mut inventory);
        let mut scratch = 2 * BOOKKEEPING_BYTES;
        let mut converted = 0;
        if self.profile.bytes() != profile.bytes() {
            if let Some(refusal) = profile_refusal(profile, convert) {
                return Err(refusal);
            }
            if convert && self.profile.model().is_none() {
                return Err(SOURCE_MODEL.into());
            }
            let mut state = Count {
                profile,
                convert,
                seen: HashSet::new(),
                converted: 0,
                largest: 0,
                render: 0,
                adjustments: AdjustmentAdmission::default(),
            };
            count(&self.layers, &mut state)?;
            let pixels = state
                .largest
                .checked_mul(FLOAT_REBUILD_COPIES)
                .and_then(|v| v.checked_add(state.converted))
                .ok_or(OVERFLOW)?;
            scratch = scratch
                .checked_add(state.adjustments.total()?)
                .and_then(|v| v.checked_add(pixels))
                .and_then(|v| v.checked_add(pixels.div_ceil(4)))
                .and_then(|v| v.checked_add(state.render))
                .ok_or(OVERFLOW)?;
            converted = state.converted;
        }
        Ok(PrecisionColorPlan {
            retained_bytes: inventory.unique_bytes(),
            scratch_bytes: scratch,
            converted_pixel_bytes: converted,
        })
    }

    /// `color_edit_with_context`'s float arm: Assign (`convert` false) or
    /// Convert (`convert` true) under the module doc's rule. `Ok(None)` for
    /// the document's own profile bytes.
    pub(crate) fn float_color_edit(
        &self,
        profile: &Arc<IccProfile>,
        convert: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        job.check()?;
        let plan = self.float_color_plan(profile, convert)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        if self.profile.bytes() == profile.bytes() {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let tone_map = self
            .tone_map()
            .ok_or("a float color edit requires a 32-bit float document")?;
        // Every operator is rebound first, hidden nodes included, so a
        // refused rebind costs no converted plane.
        let rebound = rebind_layers(&self.layers, profile, job)?;
        let layers = if convert {
            job.report(0.05, 2)?;
            convert_layers(
                &self.layers,
                &rebound,
                profile,
                &mut HashMap::new(),
                job,
                plan.converted_pixel_bytes,
            )
            .map_err(|error| {
                // A raster plane answers the clipboard's sentence and a bound
                // record's re-render the place's: both are this Convert's
                // range refusal.
                if error.starts_with(RANGE_REFUSAL_PREFIX) || error == FLOAT_MASTER_RANGE_REFUSAL {
                    range_refusal(profile.name())
                } else {
                    error
                }
            })?
        } else {
            rebound
        };
        let mut next = Self::from_parts_float(
            self.width,
            self.height,
            profile.clone(),
            self.metadata.clone(),
            self.resolution,
            layers,
            self.channels.as_ref().clone(),
            tone_map,
        )?;
        // What `color_edit_with_context` carries: a profile edit moves no
        // canvas and changes no record's identity.
        next.import_warnings = self.import_warnings.clone();
        next.guides.clone_from(&self.guides);
        next.ruler_origin = self.ruler_origin;
        next.color_samplers.clone_from(&self.color_samplers);
        next.paths.clone_from(&self.paths);
        next.global_light = self.global_light;
        next.comps.clone_from(&self.comps);
        next.background = self.background;
        next.native_preflight()?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
