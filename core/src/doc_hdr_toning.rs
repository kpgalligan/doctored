//! Image ▸ Adjustments ▸ HDR Toning… over a document (phases 4a and 4b):
//! the adapters around the `hdr_toning` and `hdr_local_adaptation` kernels,
//! and the Toning Curve's histogram. HDR Toning FLATTENS: the document's
//! composite is rendered, tone mapped at native depth and kept as one raster
//! "Background" in a NEW u16 document
//! (`PrecisionDocument::flattened_u16_document`, the carry `float_to_u16`
//! shares). The source is not changed; a request that changes no sample
//! still answers a new flattened document, since the flatten is a change.
//!
//! # The pixels
//!
//! A float document's render is already linear light. A u16 document's
//! render is decoded to linear light through the ONE typed transfer of its
//! profile (`precision_clipboard_promote::promoted_image` under Preserve).
//! The kernel's `analyse` then reads the linear plane (two fresh passes for
//! Equalize Histogram) and `map_pixel` writes each encoded triple through
//! `PrecisionImage::from_float_samples_unseeded` at RGBA16, alpha kept. Local
//! Adaptation reads it through `hdr_local_adaptation::adapt` instead (the
//! range passes, the `(t′, α)` plane and the guided pass, which drops the
//! plane) and maps each pixel by its raster index. The
//! [`DisplayCurve`] is the profile's own `FloatPolicy` under the clip map
//! (`encode_sdr`), so a profile that cannot carry float refuses here too.
//!
//! A float source's result evaluates in linear light (`float_to_u16`'s
//! rule); a u16 source's keeps its own evaluation. An 8-bit document is
//! promoted (`DrDocument::promote_flattened`) and the result narrowed
//! (`downconvert`, no dither) by the host around this call; the 8-bit plan
//! here states all three stages.
//!
//! # Grayscale
//!
//! A Gray document (#1 phase 2e) at 8 or 16 bits tones through the same
//! kernels on its R = G = B composite; each mapped triple is folded back to
//! one channel by the filters' rule (`gray_filter::fold`: the value when the
//! three agree bit for bit, else the Rec. 709 luma), and the flattened result
//! keeps the document's Gray policy (the same profile handle) and its depth:
//! the plan's `bit_depth` must be the document's own, its output 8 B/px (the
//! u16 storage an 8-bit Gray document also holds), and nothing is narrowed.
//! A 32-bit Gray document (phase 5a) tones the same way into a 16-bit Gray
//! document, its `bit_depth` 8 or 16; at 8 the host narrows the result by the
//! Gray depth edit (`gray_profile`) inside the same job, so the 8-bit plan
//! states that stage too (`gray_profile::depth_eight_bytes`).
//!
//! # Refusals
//!
//! `Err`: a CMYK document ("hdr toning is unavailable in a CMYK document"),
//! the kernel's request refusals, a bit depth other than 8 or 16 (8 only for
//! a float source), a profile with no matrix/TRC model, a working-memory
//! refusal and a cancel.
//!
//! # Memory
//!
//! The plan is the largest stage with the outputs held before it: the
//! render (its own plan, as `downconvert_plan` states it) or the kernel's
//! post-render stages (`hdr_toning::plan_bytes`, which takes the request),
//! whichever is larger; at 8 bits beside the narrowing of the one-raster
//! result. `hdr_toning` admits the source's retained bytes plus the tone
//! stage against the limit and reserves it, as a floor for every nested
//! reservation, before it renders. `hdr_toning_histogram` admits and reserves
//! the tone stage of the DEFAULT Local Adaptation request, an upper bound of
//! its own render, decode and two passes.

use serde::Serialize;

use crate::doc::DrDocument;
use crate::doc_evaluation::Evaluation;
use crate::float_document::{FloatPolicy, ToneMap};
use crate::hdr_local_adaptation;
use crate::hdr_toning::{self, DisplayCurve, HdrRequest};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_clipboard::PasteProfile;
use crate::precision_clipboard_promote::promoted_image;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, pixel_count, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::sampling_gaussian::destructive_work_cap;

/// The largest request the parser reads: Local Adaptation's eleven keys and
/// a sixteen-point Toning Curve are under a kilobyte.
pub const MAX_REQUEST_BYTES: usize = 4096;

/// What a toning admits and what it produces, as `*_hdr_toning_plan`
/// answers it.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct HdrToningPlan {
    pub working_memory_plan_bytes: u64,
    pub output_bytes: u64,
}

impl HdrToningPlan {
    pub fn json(self) -> String {
        serde_json::json!(self).to_string()
    }
}

/// The Toning Curve's histogram, as `dr_precision_doc_hdr_toning_histogram`
/// answers it: 256 alpha-weighted bins over the log range, whose ends are
/// `low_stops` and `high_stops` (log2 of linear luminance).
#[derive(Clone, Debug, Serialize)]
pub struct HdrToningHistogram {
    pub bins: Vec<f64>,
    pub low_stops: f32,
    pub high_stops: f32,
}

impl HdrToningHistogram {
    pub fn json(self) -> String {
        serde_json::json!(self).to_string()
    }
}

/// The request the histogram is admitted under: Local Adaptation at its
/// defaults, whose plan bounds the histogram's own render and passes.
const DEFAULT_LOCAL_ADAPTATION: &str = r#"{"method":"local_adaptation"}"#;

impl DisplayCurve for FloatPolicy {
    fn encode(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.encode_sdr(rgb)
    }
}

const CMYK_REFUSAL: &str = "hdr toning is unavailable in a CMYK document";

fn overflow() -> String {
    "hdr toning working-memory calculation overflow".into()
}

/// The request through the kernel's parser, its length bounded first.
fn parsed(request: &str) -> Result<HdrRequest, String> {
    if request.len() > MAX_REQUEST_BYTES {
        return Err("hdr toning request exceeds the supported byte limit".into());
    }
    hdr_toning::parse(request)
}

/// The render of a one-raster u16 document over `count` pixels:
/// `PrecisionScene::plan`'s u16 finalization (the 16 B/px accumulator beside
/// the 8 B/px output) and its bookkeeping unit — the master copy of the
/// figure is there.
fn one_raster_render(count: u64) -> Option<u64> {
    count.checked_mul(24)?.checked_add(BOOKKEEPING_BYTES)
}

/// The 8-bit narrowing of the one-raster u16 result (`downconvert_plan` over
/// it): the result held (8 B/px), its render or the 12 B/px quantize,
/// whichever is larger, and the 4 B/px output.
fn narrow_stage(count: u64) -> Option<u64> {
    let quantize = count.checked_mul(12)?.checked_add(4 * BOOKKEEPING_BYTES)?;
    count
        .checked_mul(12)?
        .checked_add(one_raster_render(count)?.max(quantize))
}

/// Each call is a fresh pass over the linear plane's tiles in raster order;
/// the take_while ends a pass early once a cancel is asked, and the caller's
/// check answers it.
fn linear_pixels<'a>(
    linear: &'a PrecisionImage,
    job: JobContext<'a>,
) -> impl Iterator<Item = [f32; 4]> + 'a {
    linear
        .runs_rgba32()
        .into_iter()
        .flatten()
        .flatten()
        .copied()
        .enumerate()
        .take_while(move |(i, _)| i & 4095 != 0 || !job.cancelled())
        .map(|(_, pixel)| pixel)
}

impl PrecisionDocument {
    /// The tone stage's working memory (module doc, "Memory").
    fn hdr_tone_bytes(&self, request: &HdrRequest) -> Result<u64, String> {
        let render = self.render_plan()?;
        let render_stage = render
            .scratch_bytes
            .checked_add(
                render
                    .unique_retained_bytes
                    .saturating_sub(self.retained_bytes()),
            )
            .ok_or_else(overflow)?;
        let (width, height) = self.dimensions();
        Ok(render_stage.max(hdr_toning::plan_bytes(
            request,
            width,
            height,
            self.is_float(),
        )?))
    }

    /// The composite rendered into linear light (module doc, "The pixels"):
    /// a float render as is, a u16 render decoded through its profile.
    fn hdr_linear(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let rendered = self.render_with_context(limit, other, job.range(0.0, 0.6))?;
        let linear = if self.is_float() {
            rendered
        } else {
            let promoted = promoted_image(
                &rendered,
                self.profile(),
                PasteProfile::Preserve,
                job.range(0.6, 0.7),
            )?;
            drop(rendered);
            promoted
        };
        if linear.storage() != PrecisionStorage::Rgba32Float {
            return Err("hdr toning requires a linear rgba32_float plane".into());
        }
        Ok(linear)
    }

    /// The Toning Curve's histogram of this document's composite
    /// (`hdr_local_adaptation::histogram`): admitted against `limit` with
    /// `other` held elsewhere and reserved on `control` under the default
    /// Local Adaptation plan before it renders; polls cancellation through
    /// every pass. A CMYK document refuses.
    pub fn hdr_toning_histogram(
        &self,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<HdrToningHistogram, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        if self.is_cmyk() {
            return Err(CMYK_REFUSAL.into());
        }
        // #1 Indexed phase 3b: HDR Toning refuses in Indexed Color, for
        // good.
        if self.is_indexed() {
            return Err(crate::indexed_document::refused_sentence("hdr_toning"));
        }
        FloatPolicy::for_profile(self.profile(), ToneMap::Clip)?;
        let working = self.hdr_tone_bytes(&parsed(DEFAULT_LOCAL_ADAPTATION)?)?;
        admit(self.retained_bytes(), working, other, limit)?;
        job.reserve(working)?;
        let job = job.with_reservation_floor(working);
        let linear = self.hdr_linear(limit, other, job)?;
        let (bins, range) = hdr_local_adaptation::histogram(|| linear_pixels(&linear, job));
        job.check()?;
        job.report(1.0, 5)?;
        Ok(HdrToningHistogram {
            bins: bins.to_vec(),
            low_stops: range.low,
            high_stops: range.low + range.span,
        })
    }

    /// The plan of toning this document into `bit_depth` bits (16, or 8 for
    /// a float source), after every refusal `hdr_toning` gives before it
    /// allocates.
    pub fn hdr_toning_plan(&self, json: &str, bit_depth: u32) -> Result<HdrToningPlan, String> {
        if self.is_cmyk() {
            return Err(CMYK_REFUSAL.into());
        }
        // #1 Indexed phase 3b: HDR Toning refuses in Indexed Color, for
        // good.
        if self.is_indexed() {
            return Err(crate::indexed_document::refused_sentence("hdr_toning"));
        }
        let request = parsed(json)?;
        if let Some(policy) = self.gray_policy() {
            let bits = policy.depth().bits();
            if bits == 32 {
                if !matches!(bit_depth, 8 | 16) {
                    return Err(format!(
                        "hdr toning converts a 32-bit Grayscale document to 8 or 16 bits per channel; {bit_depth} was given"
                    ));
                }
            } else if bit_depth != bits {
                return Err(format!(
                    "a {bits}-bit Grayscale document keeps its depth through hdr toning"
                ));
            }
            FloatPolicy::for_profile(self.profile(), ToneMap::Clip)?;
            let (width, height) = self.dimensions();
            let count = pixel_count(width, height)? as u64;
            let tone = self.hdr_tone_bytes(&request)?;
            // At 8 bits from 32 the host's Gray depth edit narrows the toned
            // one-raster result inside the same job (module doc, "Grayscale").
            let working = if bits == 32 && bit_depth == 8 {
                let narrow = crate::gray_profile::depth_eight_bytes(width, height)
                    .and_then(|bytes| bytes.checked_add(crate::gray_profile::policy_bytes(policy)))
                    .and_then(|bytes| bytes.checked_add(BOOKKEEPING_BYTES))
                    .ok_or_else(overflow)?;
                tone.max(narrow)
            } else {
                tone
            };
            return Ok(HdrToningPlan {
                working_memory_plan_bytes: working,
                output_bytes: count.checked_mul(8).ok_or_else(overflow)?,
            });
        }
        match bit_depth {
            16 => {}
            8 if self.is_float() => {}
            8 => {
                return Err("a 16-bit document keeps its depth through hdr toning".into());
            }
            other => {
                return Err(format!(
                    "hdr toning converts to 8 or 16 bits per channel; {other} was given"
                ))
            }
        }
        FloatPolicy::for_profile(self.profile(), ToneMap::Clip)?;
        let (width, height) = self.dimensions();
        let count = pixel_count(width, height)? as u64;
        let tone = self.hdr_tone_bytes(&request)?;
        let (working, output) = if bit_depth == 8 {
            (
                tone.max(narrow_stage(count).ok_or_else(overflow)?),
                count.checked_mul(4).ok_or_else(overflow)?,
            )
        } else {
            (tone, count.checked_mul(8).ok_or_else(overflow)?)
        };
        Ok(HdrToningPlan {
            working_memory_plan_bytes: working,
            output_bytes: output,
        })
    }

    /// The document flattened and tone mapped by `json`'s method into a NEW
    /// one-raster u16 document (module doc). Admitted against `limit` with
    /// `other` held elsewhere and reserved on `control` before it renders;
    /// polls cancellation through every pass.
    pub fn hdr_toning(
        &self,
        json: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        if self.is_cmyk() {
            return Err(CMYK_REFUSAL.into());
        }
        // #1 Indexed phase 3b: HDR Toning refuses in Indexed Color, for
        // good.
        if self.is_indexed() {
            return Err(crate::indexed_document::refused_sentence("hdr_toning"));
        }
        let request = parsed(json)?;
        let gray = self.gray_policy().is_some();
        // A Gray document's mapped triple folds back to one channel by the
        // filters' rule (module doc, "Grayscale"); it allocates nothing.
        let folded = |rgb: [f32; 3], alpha: f32| {
            let sample = [rgb[0], rgb[1], rgb[2], alpha];
            if gray {
                crate::gray_filter::fold(sample, false)
            } else {
                sample
            }
        };
        let policy = FloatPolicy::for_profile(self.profile(), ToneMap::Clip)?;
        let working = self.hdr_tone_bytes(&request)?;
        admit(self.retained_bytes(), working, other, limit)?;
        job.reserve(working)?;
        let job = job.with_reservation_floor(working);
        let linear = self.hdr_linear(limit, other, job)?;
        let (width, height) = linear.dimensions();
        let runs = || linear.runs_rgba32().into_iter().flatten().flatten();
        let image = match &request {
            HdrRequest::Global(method) => {
                let stats = hdr_toning::analyse(method, || linear_pixels(&linear, job));
                job.check()?;
                job.range(0.0, 0.75).report(1.0, 5)?;
                let samples = runs().map(|pixel| {
                    let rgb = hdr_toning::map_pixel(
                        method,
                        &stats,
                        &policy,
                        [pixel[0], pixel[1], pixel[2]],
                    );
                    folded(rgb, pixel[3])
                });
                PrecisionImage::from_float_samples_unseeded(
                    width,
                    height,
                    self.profile().clone(),
                    PrecisionStorage::Rgba16Unorm,
                    samples,
                    job.range(0.75, 0.95),
                )?
            }
            HdrRequest::Local(la) => {
                let adapted = hdr_local_adaptation::adapt(
                    la,
                    || linear_pixels(&linear, job),
                    (width, height),
                    destructive_work_cap(job),
                    job.range(0.7, 0.9),
                )?;
                job.check()?;
                let samples = runs().enumerate().map(|(i, pixel)| {
                    let rgb = hdr_local_adaptation::map_pixel(
                        la,
                        &adapted,
                        i,
                        &policy,
                        [pixel[0], pixel[1], pixel[2]],
                    );
                    folded(rgb, pixel[3])
                });
                PrecisionImage::from_float_samples_unseeded(
                    width,
                    height,
                    self.profile().clone(),
                    PrecisionStorage::Rgba16Unorm,
                    samples,
                    job.range(0.9, 0.97),
                )?
            }
        };
        drop(linear);
        let evaluation = if self.is_float() {
            Evaluation::LinearLight
        } else {
            self.evaluation()
        };
        let out = self.flattened_u16_document(image, evaluation)?;
        // A 32-bit Gray document's result is 16-bit Gray (module doc).
        let out = match self.gray_policy() {
            Some(policy) if self.is_float() => out.with_gray(std::sync::Arc::new(
                policy.with_depth(crate::gray_document::GrayDepth::Sixteen),
            ))?,
            Some(policy) => out.with_gray(policy.clone())?,
            None => out,
        };
        job.report(1.0, 5)?;
        Ok(out)
    }
}

impl DrDocument {
    /// The plan of toning this 8-bit document as the host runs it: the
    /// flattened promotion (`promote_flattened_plan`, its output 8 B/px plus
    /// 2 B/px per saved channel),
    /// then the tone stage of the one-raster u16 document with the promotion
    /// held, then the narrowing back to 8 bits beside both.
    pub fn hdr_toning_plan(&self, json: &str) -> Result<HdrToningPlan, String> {
        let request = parsed(json)?;
        FloatPolicy::for_profile(&self.profile, ToneMap::Clip)?;
        let promote = self.promote_flattened_plan()?;
        let count = pixel_count(self.width, self.height)? as u64;
        // The promotion and the narrowing both carry the saved channels
        // (#1): n u16 samples per channel held, n bytes per channel out.
        let channels = self.channels.len() as u64;
        let narrowed_channels = count.checked_mul(channels).ok_or_else(overflow)?;
        let held = count
            .checked_mul(8)
            .and_then(|bytes| bytes.checked_add(narrowed_channels.checked_mul(2)?))
            .ok_or_else(overflow)?;
        let promote_stage = promote
            .scratch_bytes
            .checked_add(promote.output_bytes)
            .ok_or_else(overflow)?;
        let tone = one_raster_render(count)
            .ok_or_else(overflow)?
            .max(hdr_toning::plan_bytes(
                &request,
                self.width,
                self.height,
                false,
            )?)
            .checked_add(held)
            .ok_or_else(overflow)?;
        let narrow = narrow_stage(count)
            .and_then(|bytes| bytes.checked_add(held))
            .and_then(|bytes| bytes.checked_add(narrowed_channels))
            .ok_or_else(overflow)?;
        Ok(HdrToningPlan {
            working_memory_plan_bytes: promote_stage.max(tone).max(narrow),
            output_bytes: count
                .checked_mul(4)
                .and_then(|bytes| bytes.checked_add(narrowed_channels))
                .ok_or_else(overflow)?,
        })
    }
}
