//! The Lab Color layer's luma-reading filters (#547 phase 2f,
//! `designs/lab-mode.md`): Emboss, Edge Detect, Lens Blur, Reduce
//! Noise and Smart Sharpen, and Offset's `fill` edge, run through the RGB
//! selected-filter path (`selected_adjustment` / `selected_filter`,
//! unchanged) one Lab channel at a time. `cmyk_filter` is the template.
//!
//! # Why passes
//!
//! These kernels read a Rec. 709 luma across the three samples
//! (`lab_filter`'s module doc), so run on `[L, a, b]` they would read a\* as
//! green. Pass k is a one-layer RGBA16 document of the layer's own size,
//! `[c_k, c_k, c_k, α]`, tagged with the built-in sRGB (the numbers are Lab
//! codes, not a colour): on a gray sample every luma reading is the channel
//! itself, so the pass's G is the filter's one-channel result. Each pass
//! runs `selected_edit_with_context` on its layer 0 with the canvas
//! coverage remapped into the layer's frame
//! (`coverage_at`), so the frames, banding, work caps, seeds and the no-op
//! rule are the RGB path's verbatim. The result is the source's own profile
//! handle with L\*, a\* and b\* = the passes' G and α = pass 0's α; a
//! transparency lock is `under_locks`' (a Lab raster is an RGB raster).
//!
//! # The rules that are Doctored's, not measured
//!
//! * **Emboss and Edge Detect** ([`LIGHTNESS_ONLY`]) run pass 0 alone: their
//!   kernels add 0.5 (Emboss) or take a gradient magnitude (Edge Detect), so
//!   a flat a\* or b\* plane would clamp to code 65535 or 0. On RGB they
//!   write a gray `[v, v, v, 1]`, whose Lab is a\* = b\* = 0, so each a\* and
//!   b\* code is mixed toward `lab_tone::NEUTRAL` by the coverage
//!   (`mix_codes`; `lab_tone`'s Threshold rule): the neutral exactly at full
//!   coverage.
//! * **Reduce Noise** runs all three passes with the request's parameters.
//!   Each pass is gray, so the chroma half's U = V = 0 and `color` has no
//!   effect on Lab; the luminance half runs on L\*, a\* and b\*. A request
//!   whose luminance half is off (`sampling_denoise::halves`) changes
//!   nothing.
//! * **Offset's `fill`** is `fill` on the L\* pass and the neutral on the
//!   a\* and b\* passes: the neutral colour of Lightness `fill`.
//!
//! Every pass unchanged, or no stored code changed, is `Ok(None)`.
//!
//! # The plan
//!
//! Pass 0's RGB `scratch_bytes` over a layer-sized stand-in, plus layer px ×
//! [`PASS_BYTES_PER_PIXEL`] (the result 8, one pass input 8, its output 8,
//! the remapped coverage 2: one pass is held at a time, so the Lightness-only
//! form has the same peak), canvas × [`VIEW_BYTES_PER_PIXEL`] (the host's
//! display preparation of the result) and two bookkeeping terms;
//! `output_bytes` is layer px × 8. `scripts/lab-admission/measure.py` holds
//! it to ⌈1.25 × the measured peak⌉; a miss raises one of the two terms.

use serde_json::{json, Map, Value};

use crate::doc_lock::EditKind;
use crate::filter_kernels::Neighborhood;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::lab_tone::NEUTRAL;
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::selected_adjustment::{coverage_at, mix_codes, validate_coverage, SelectedEdit};
use crate::selected_filter::SelectedFilter;

/// The filters a Lab layer runs in passes (module doc); Offset joins them
/// with `undefined_areas` "fill" ([`runs_in_passes`]).
pub(crate) const PASS_FILTERS: &[&str] = &[
    "emboss",
    "edge_detect",
    "lens_blur",
    "reduce_noise",
    "smart_sharpen",
];

/// One L\* pass; a\* and b\* are mixed toward `lab_tone::NEUTRAL`
/// (`lab_tone`'s Threshold rule).
pub(crate) const LIGHTNESS_ONLY: &[&str] = &["emboss", "edge_detect"];

/// Layer px: the result 8 + one pass input 8 + its output 8 + the remapped
/// coverage 2. A miss raises this term.
pub(crate) const PASS_BYTES_PER_PIXEL: u64 = 26;

/// Canvas px: the host's display preparation of the result (the Lab view),
/// CMYK's measured 21 as the start. A miss raises this term.
pub(crate) const VIEW_BYTES_PER_PIXEL: u64 = 21;

/// True for an op of [`PASS_FILTERS`], or `offset` with `undefined_areas`
/// "fill".
pub(crate) fn runs_in_passes(edit: &SelectedEdit) -> bool {
    PASS_FILTERS.contains(&edit.op.as_str())
        || (edit.op == "offset"
            && edit.params.get("undefined_areas").and_then(Value::as_str) == Some("fill"))
}

/// `edit` as pass `k` runs it: layer 0, and an Offset `fill` of the neutral
/// on the a\* and b\* passes (module doc).
fn pass_edit(edit: &SelectedEdit, k: usize) -> SelectedEdit {
    let mut params = edit.params.clone();
    if k > 0 && edit.op == "offset" && params.contains_key("fill") {
        params.insert("fill".into(), json!(f64::from(NEUTRAL)));
    }
    SelectedEdit {
        layer: 0,
        op: edit.op.clone(),
        params,
        description: None,
        analysis_scope: edit.analysis_scope,
    }
}

/// A one-layer RGBA16 pass document of `pixels` (module doc).
fn pass_document(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> Result<PrecisionDocument, String> {
    let image = PrecisionImage::from_rgba16_unseeded(w, h, pixels, IccProfile::srgb(), None)?;
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
}

impl PrecisionDocument {
    /// The source image of a pass filter: the layer's u16 raster.
    fn lab_pass_source(&self, layer: usize) -> Result<&PrecisionImage, String> {
        self.layer_source(layer)
            .filter(|image| image.storage() == PrecisionStorage::Rgba16Unorm)
            .ok_or_else(|| "selected edit requires a raster layer".to_string())
    }

    /// The plan of a pass filter (`selected_edit_plan`'s keys; module doc).
    pub(crate) fn lab_channel_filter_plan(&self, edit: &SelectedEdit) -> Result<Value, String> {
        let (w, h) = self.lab_pass_source(edit.layer)?.dimensions();
        let layer = pixel_count(w, h)? as u64;
        let (cw, ch) = self.dimensions();
        let canvas = pixel_count(cw, ch)? as u64;
        // The RGB path's own plan for pass 0 over a plane of the layer's
        // size (a filled stand-in: the plan reads only its dimensions).
        let stand_in = PrecisionImage::filled_rgba16(w, h, [0; 4], IccProfile::srgb())?;
        let pass =
            PrecisionDocument::from_image(stand_in, Metadata::default(), Resolution::default())?;
        let rgb = pass.selected_edit_plan(&pass_edit(edit, 0))?;
        let pass_scratch = rgb["scratch_bytes"]
            .as_u64()
            .ok_or("invalid Lab filter plan")?;
        let scratch = pass_scratch
            + layer * PASS_BYTES_PER_PIXEL
            + canvas * VIEW_BYTES_PER_PIXEL
            + 2 * BOOKKEEPING_BYTES;
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "output_bytes":layer * 8,"input_samples":canvas,"input_width":cw,
            "input_height":ch}),
        )
    }

    /// Runs `edit`'s pass filter on Lab raster `edit.layer` (module doc)
    /// within the canvas `coverage` (empty is the whole layer). `Ok(None)`
    /// when no stored code changes.
    pub(crate) fn lab_channel_filter(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        validate_coverage(self, coverage)?;
        self.under_locks(edit.layer, EditKind::Pixels, |doc| {
            doc.lab_channel_filter_unlocked(edit, coverage, limit, other, job)
        })
    }

    fn lab_channel_filter_unlocked(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let plan = self.lab_channel_filter_plan(edit)?;
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .ok_or("invalid Lab filter plan")?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        // Reduce Noise with its luminance half off changes nothing on gray
        // passes (module doc).
        if let SelectedFilter::Photo {
            neighborhood: Neighborhood::Denoise { luma: false, .. },
            ..
        } = SelectedFilter::parse(&edit.op, &edit.params)?
        {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let entry = self
            .layer(edit.layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = self.lab_pass_source(edit.layer)?;
        let offset = entry.offset();
        let (w, h) = image.dimensions();
        let n = pixel_count(w, h)?;
        let canvas = self.dimensions();
        let local: Vec<u16> = if coverage.is_empty() {
            Vec::new()
        } else {
            let mut out = reserved_vec(n)?;
            out.extend((0..n).map(|i| coverage_at(coverage, canvas, (w, h), offset, i)));
            out
        };
        let mut result = reserved_vec(n)?;
        for run in image
            .runs_rgba16()
            .ok_or("a Lab filter requires u16 storage")?
        {
            result.extend_from_slice(run);
        }
        let lightness_only = LIGHTNESS_ONLY.contains(&edit.op.as_str());
        let passes = if lightness_only { 1 } else { 3 };
        // The result's 8 + the remapped coverage's 2, held beside each pass.
        let held = self.retained_bytes() + other + n as u64 * 10;
        for k in 0..passes {
            job.check()?;
            let range = if lightness_only {
                job.range(0.0, 0.9)
            } else {
                job.range(0.3 * k as f64, 0.3 * (k + 1) as f64)
            };
            let mut input = reserved_vec(n)?;
            for run in image
                .runs_rgba16()
                .ok_or("a Lab filter requires u16 storage")?
            {
                input.extend(run.iter().map(|p| [p[k], p[k], p[k], p[3]]));
            }
            let pass = pass_document(w, h, input)?;
            let out =
                pass.selected_edit_with_context(&pass_edit(edit, k), &local, limit, held, range)?;
            let Some(out) = out else {
                continue;
            };
            let ran = out
                .layer(0)
                .and_then(|l| l.source())
                .ok_or("the Lab filter's pass lost its layer")?;
            if ran.dimensions() != (w, h) {
                return Err("the Lab filter's pass changed the layer's size".into());
            }
            let runs = ran
                .runs_rgba16()
                .ok_or("a Lab filter requires u16 storage")?;
            let mut i = 0;
            for run in runs {
                for p in run {
                    result[i][k] = p[1];
                    if k == 0 {
                        result[i][3] = p[3];
                    }
                    i += 1;
                }
            }
            // The pass and its output drop here, before the next pass.
        }
        if lightness_only {
            // a* and b* toward the neutral by the coverage, at the source's
            // alpha (module doc).
            let mut i = 0;
            for run in image
                .runs_rgba16()
                .ok_or("a Lab filter requires u16 storage")?
            {
                for p in run {
                    job.every(i)?;
                    let weight = coverage_at(coverage, canvas, (w, h), offset, i);
                    for c in 1..3 {
                        let alpha = f32::from(p[3]) / 65535.0;
                        let mixed = mix_codes(
                            [p[c], p[c], p[c], p[3]],
                            [NEUTRAL, NEUTRAL, NEUTRAL, alpha],
                            weight,
                        )?;
                        result[i][c] = mixed[1];
                    }
                    i += 1;
                }
            }
        }
        job.check()?;
        let mut changed = false;
        let mut i = 0;
        for run in image
            .runs_rgba16()
            .ok_or("a Lab filter requires u16 storage")?
        {
            for p in run {
                changed |= result[i] != *p;
                i += 1;
            }
        }
        let out = if changed {
            let rebuilt = image.rebuilt_rgba16(w, h, result, image.profile().clone())?;
            self.replacing_content(
                edit.layer,
                Content::Raster {
                    image: rebuilt,
                    offset,
                },
            )?
        } else {
            None
        };
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The descriptor's `lab.filter_plans` (module doc): each op of
    /// [`PASS_FILTERS`] with no params on the top Lab raster, its plan's
    /// `scratch_bytes`, or `null` where none or the plan refuses.
    pub(crate) fn lab_filter_descriptor_plans(&self) -> Map<String, Value> {
        let top = (0..self.layer_count())
            .rev()
            .find(|&i| self.lab_pass_source(i).is_ok());
        let mut out = Map::new();
        for op in PASS_FILTERS {
            let plan = top.and_then(|layer| {
                let edit = SelectedEdit {
                    layer,
                    op: (*op).to_string(),
                    params: Map::new(),
                    description: None,
                    analysis_scope: Default::default(),
                };
                self.lab_channel_filter_plan(&edit)
                    .ok()
                    .and_then(|plan| plan["scratch_bytes"].as_u64())
            });
            out.insert((*op).into(), plan.map_or(Value::Null, Value::from));
        }
        out
    }
}
