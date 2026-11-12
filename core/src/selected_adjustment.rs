//! Captured destructive native tone/filter requests. The source neighborhood and
//! automatic analysis are independent of canvas write coverage. A zero coverage
//! sample is copied as original codes; partial alpha-changing results mix in
//! premultiplied space, with a single terminal native quantization.
//!
//! Neighborhoods read complete original storage while optional canvas u16 coverage gates writeback
//! once, preserving exact zero-coverage codes. Scalar tone uses the precision interpreter. Strict
//! plans precede coverage allocation; single-use workers poll and retain their full compound
//! reservation. `selected_edit_plan` charges the n×8 tiled copy of the output
//! codes `rebuilt_rgba16` makes while they are still held: additively on the tone, auto and banded
//! arms, and on the whole-plane arm as max(kernel scratch, held n×16 plane + copy), since the
//! kernel's transient buffers are freed before the copy is made; the plan reports `output_bytes`
//! (n×8: the rewritten layer's codes the result keeps beyond its source), which the host's preview
//! reservation reads.
use crate::adjust_precision::PrecisionAdjustment;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, reserved_vec, PrecisionStorage, BOOKKEEPING_BYTES};
use crate::selected_auto_levels::AutoLevels;
use crate::selected_filter::{SelectedFilter, BAND_ROWS};
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisScope {
    #[default]
    WholeTarget,
    Selection,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedEdit {
    pub layer: usize,
    pub op: String,
    #[serde(default)]
    pub params: Map<String, Value>,
    pub description: Option<String>,
    #[serde(default)]
    pub analysis_scope: AnalysisScope,
}
impl SelectedEdit {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 20 * 1024 * 1024 {
            return Err("selected edit request exceeds 20 MiB".into());
        }
        let edit: Self = serde_json::from_str(text)
            .map_err(|e| format!("invalid selected edit request: {e}"))?;
        if edit.op != "adjustment" && edit.description.is_some() {
            return Err("description belongs only to adjustment requests".into());
        }
        if !edit.is_auto() && edit.analysis_scope != AnalysisScope::WholeTarget {
            return Err("analysis_scope belongs only to automatic tone requests".into());
        }
        if edit.op == "adjustment" {
            if !edit.params.is_empty() || edit.description.is_none() {
                return Err("adjustment requires description and no params".into());
            }
        } else if edit.op == "equalize" {
            crate::equalize::parse_scope(&edit.params)?;
        } else if edit.is_auto() {
            AutoLevels::validate(&edit)?;
        } else {
            // A CMYK document's Clouds take ink in place of the colour keys
            // (`cmyk_filter`); the rest of the request validates as RGB, and
            // an RGB document's own parse refuses the ink keys.
            let mut params = edit.params.clone();
            if matches!(edit.op.as_str(), "clouds" | "difference_clouds") {
                params.remove("foreground_ink");
                params.remove("background_ink");
            }
            SelectedFilter::parse(&edit.op, &params)?;
        }
        Ok(edit)
    }
    pub fn is_auto(&self) -> bool {
        matches!(
            self.op.as_str(),
            "auto_tone" | "auto_contrast" | "auto_color"
        )
    }
}
impl PrecisionDocument {
    /// A CMYK document's selected adjustment (#1 phase 2): a description
    /// applied to the ink through `cmyk_adjust_layer`; Auto refuses by name
    /// (the host runs CMYK Auto as Levels). `liquify` (phase 4b) is
    /// `cmyk_warp`'s, over the ink's five planes. Since the adjustments batch
    /// every other op is a filter, `cmyk_filter`'s.
    fn cmyk_selected_description<'a>(&self, edit: &'a SelectedEdit) -> Result<&'a str, String> {
        if edit.op != "adjustment" {
            return Err(crate::cmyk_document::refused_sentence(&format!(
                "The \"{}\" edit",
                edit.op
            )));
        }
        edit.description
            .as_deref()
            .ok_or_else(|| "adjustment description is required".to_string())
    }

    pub fn selected_edit_plan(&self, edit: &SelectedEdit) -> Result<Value, String> {
        // A Bitmap document (#1 Bitmap phase 3): the Gray storage's edit,
        // then thresholded (`bitmap_tools`).
        if self.is_bitmap() {
            return crate::bitmap_tools::selected_plan(self, edit);
        }
        // Equalize (#1) at every typed depth and on CMYK: `equalize_apply`.
        if edit.op == "equalize" {
            return self.equalize_plan(edit);
        }
        if self.is_cmyk() && edit.op == "liquify" {
            let strokes = crate::liquify::parse_strokes(&edit.params)?;
            return self.cmyk_liquify_plan(edit.layer, &strokes);
        }
        if self.is_cmyk() && edit.op != "adjustment" && !edit.is_auto() {
            return self.cmyk_filter_plan(edit);
        }
        if self.is_cmyk() {
            let description = self.cmyk_selected_description(edit)?;
            let mut plan = self.cmyk_adjustment_plan(Some(edit.layer), description, true)?;
            let (cw, ch) = self.dimensions();
            let (w, h) = self
                .layer(edit.layer)
                .and_then(crate::precision_document::PrecisionDocumentLayer::cmyk_source)
                .map(|image| image.dimensions())
                .ok_or("a destructive CMYK adjustment requires a CMYK raster layer")?;
            plan["output_bytes"] = json!(u64::from(w) * u64::from(h) * 10);
            plan["input_samples"] = json!(u64::from(cw) * u64::from(ch));
            plan["input_width"] = json!(cw);
            plan["input_height"] = json!(ch);
            return Ok(plan);
        }
        if self.is_lab() && crate::lab_channel_filter::runs_in_passes(edit) {
            return self.lab_channel_filter_plan(edit);
        }
        let image = self
            .layer_source(edit.layer)
            .ok_or("selected edit requires a raster layer")?;
        if image.storage() == crate::precision_image::PrecisionStorage::Rgba32Float {
            return self.float_selected_edit_plan(edit);
        }
        if image.storage() != crate::precision_image::PrecisionStorage::Rgba16Unorm {
            return Err("selected edits require native u16 storage".into());
        }
        let (w, h) = image.dimensions();
        let count = u64::from(w) * u64::from(h);
        // The n×8 tiled copy `rebuilt_rgba16` makes of the output codes while
        // they are still held (measured at 24 MP as a missing term). It
        // is additive on the tone, auto and banded arms, whose state is still
        // live; a whole-plane kernel frees its transient scratch before the
        // copy, so that arm max()es the copy against it.
        let output = count * 8;
        let mut scratch = 6 * BOOKKEEPING_BYTES
            + count * 8
            + edit.description.as_ref().map_or(0, |s| s.len() as u64 * 2);
        if edit.op == "adjustment" {
            let description = edit
                .description
                .as_deref()
                .ok_or("adjustment description is required")?;
            scratch += PrecisionAdjustment::prepare_scratch_bytes(description, self.profile())?;
            let (bytes, _) = PrecisionAdjustment::guide_plan_for_description(description, w, h)?;
            scratch += bytes + output;
        } else if edit.is_auto() {
            AutoLevels::validate(edit)?;
            scratch += 4 * 65536 * 8 + output;
        } else {
            // A banded filter holds one band of its plane, never the
            // whole: `selected_edit_unlocked` makes the same decision. The
            // frame is not known here, and the frameless plan bounds any frame.
            let filter = SelectedFilter::parse(&edit.op, &edit.params)?.with_frame(None);
            // Planned at `CONTROLLED_JOB_WORK`: a plan carries no work
            // refusal; an uncontrolled run keeps `MAX_DESTRUCTIVE_WORK`.
            let cap = crate::sampling_gaussian::CONTROLLED_JOB_WORK;
            scratch += match filter.band_scratch(image.dimensions(), cap)? {
                Some(band) => band + output,
                None => {
                    // Only the n×16 plane `plane()` returns outlives the
                    // kernel's scratch beside the copy.
                    let held = match filter {
                        _ if filter.identity() => 0,
                        SelectedFilter::Noise { .. } | SelectedFilter::LevelsChannels(_) => 0,
                        _ => count * 16,
                    };
                    filter.scratch(image.dimensions(), cap)?.max(held + output)
                }
            };
        }
        let (cw, ch) = self.dimensions();
        // The optional borrowed coverage is included because the host constructs
        // it only after this plan, and retains it through candidate completion.
        scratch += u64::from(cw) * u64::from(ch) * 2;
        // `output_bytes` is what the result keeps beyond its source once the
        // edit returns: the rewritten layer's u16 codes.
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "output_bytes":output,
            "input_samples":u64::from(cw)*u64::from(ch),"input_width":cw,"input_height":ch}),
        )
    }
    pub fn selected_edit(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.selected_edit_with_context(edit, coverage, limit, other, job)
    }
    /// The LOCK gate (`precision_lock`): a selected destructive edit writes
    /// the target's pixels, so it runs under a `Pixels` edit.
    pub(crate) fn selected_edit_with_context(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if self.is_bitmap() {
            return crate::bitmap_tools::selected_edit(self, edit, coverage, limit, other, job);
        }
        if edit.op == "equalize" {
            return self.equalize(edit, coverage, limit, other, job);
        }
        if self.is_cmyk() && edit.op == "liquify" {
            let strokes = crate::liquify::parse_strokes(&edit.params)?;
            return self.cmyk_liquify(edit.layer, &strokes, coverage, limit, other, job);
        }
        if self.is_cmyk() && edit.op != "adjustment" && !edit.is_auto() {
            return self.cmyk_filter(edit, coverage, limit, other, job);
        }
        if self.is_cmyk() {
            let description = self.cmyk_selected_description(edit)?;
            return self.cmyk_apply_adjustment(
                edit.layer,
                description,
                coverage,
                limit,
                other,
                job,
            );
        }
        if self.is_lab() && crate::lab_channel_filter::runs_in_passes(edit) {
            return self.lab_channel_filter(edit, coverage, limit, other, job);
        }
        self.under_locks(edit.layer, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.selected_edit_unlocked(edit, coverage, limit, other, job)
        })
    }

    fn selected_edit_unlocked(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if self
            .layer_source(edit.layer)
            .is_some_and(|image| image.storage() == PrecisionStorage::Rgba32Float)
        {
            return self.float_selected_edit_unlocked(edit, coverage, limit, other, job);
        }
        let scratch = self.selected_edit_plan(edit)?["scratch_bytes"]
            .as_u64()
            .unwrap();
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        validate_coverage(self, coverage)?;
        let layer = self.layer(edit.layer).ok_or("invalid selected layer")?;
        let image = layer
            .source()
            .ok_or("selected edit requires raster source")?;
        let runs = image
            .runs_rgba16()
            .ok_or("selected edit requires u16 source")?;
        let dimensions = image.dimensions();
        let (w, h) = dimensions;
        let tone = if edit.op == "adjustment" {
            Some(PrecisionAdjustment::parse(
                edit.description.as_deref().unwrap(),
                self.profile(),
            )?)
        } else {
            None
        };
        if tone.as_ref().is_some_and(PrecisionAdjustment::is_identity) {
            job.report(1., 5)?;
            return Ok(None);
        }
        let guide = tone
            .as_ref()
            .map(|tone| tone.guide(w, h, &|i| image.pixel(i), job.range(0.02, 0.35)))
            .transpose()?
            .flatten();
        let filter = if tone.is_none() {
            Some(if edit.is_auto() {
                let auto = AutoLevels::derive(self, edit, coverage, job.range(0.02, 0.3))?;
                if auto.identity() {
                    job.report(1., 5)?;
                    return Ok(None);
                }
                SelectedFilter::LevelsChannels(auto)
            } else {
                let filter = SelectedFilter::parse(&edit.op, &edit.params)?;
                // Radial Blur and Offset act within the selection's bounding
                // box on this layer's own raster, derived now, never recorded.
                let frame = if filter.framed() && !coverage.is_empty() {
                    crate::ops_spatial::coverage_frame(dimensions, |i| {
                        coverage_at(coverage, self.dimensions(), dimensions, layer.offset(), i)
                    })
                } else {
                    None
                };
                filter.with_frame(frame)
            })
        } else {
            None
        };
        if filter.as_ref().is_some_and(SelectedFilter::identity) {
            job.report(1., 5)?;
            return Ok(None);
        }
        // A banded filter computes each BAND_ROWS band of its plane just
        // before the pixel loop reaches it, over that band's row fraction of
        // the 0.05..0.65 plane stage; any other filter computes its whole plane.
        let banded = match filter.as_ref() {
            Some(f)
                if f.band_scratch(
                    dimensions,
                    crate::sampling_gaussian::destructive_work_cap(job),
                )?
                .is_some() =>
            {
                Some(f)
            }
            _ => None,
        };
        let mut plane = if banded.is_some() {
            None
        } else {
            filter
                .as_ref()
                .map(|f| f.plane(image, self.linear_unit(), job.range(0.05, 0.65)))
                .transpose()?
                .flatten()
        };
        let (mut band_start, mut band_end) = (0_usize, 0_usize);
        let mut rng = crate::rng::SplitMix64::new(filter.as_ref().map_or(0, SelectedFilter::seed));
        let mut pixels = reserved_vec(w as usize * h as usize)?;
        let mut changed = false;
        let mut next = 0;
        let noise = matches!(filter, Some(SelectedFilter::Noise { .. }));
        let fold = (self.is_gray() && tone.is_none()).then_some(noise); // #1 2c, `gray_filter`
        for run in runs {
            for before in run {
                let i = next;
                next += 1;
                if let Some(f) = banded.filter(|_| i == band_end) {
                    let first = (i / w as usize) as u32;
                    let rows = first..(first + BAND_ROWS).min(h);
                    let stage = |row: u32| 0.05 + 0.60 * f64::from(row) / f64::from(h);
                    let range = job.range(stage(rows.start), stage(rows.end));
                    // Drop the previous band before computing the next.
                    drop(plane.take());
                    band_end = rows.end as usize * w as usize;
                    plane = Some(f.plane_band(image, rows, range)?);
                    band_start = i;
                }
                job.every(i)?;
                let original = image.pixel(i);
                // Evaluate before gating so deterministic noise consumes every source
                // sample and selection can never change subsequent random values.
                let adjusted = if let Some(tone) = &tone {
                    if before[3] == 0 {
                        original
                    } else {
                        let x = (i % w as usize) as u32;
                        let y = (i / w as usize) as u32;
                        let rgb = tone.apply_at(
                            [original[0], original[1], original[2]],
                            guide.as_ref().map_or(0., |g| g.at(x, y)),
                            (i64::from(x), i64::from(y)),
                        );
                        [rgb[0], rgb[1], rgb[2], original[3]]
                    }
                } else {
                    filter.as_ref().unwrap().evaluate(
                        i - band_start,
                        original,
                        plane.as_deref(),
                        &mut rng,
                    )
                };
                let adjusted = fold.map_or(adjusted, |n| crate::gray_filter::fold(adjusted, n));
                let weight =
                    coverage_at(coverage, self.dimensions(), dimensions, layer.offset(), i);
                let after = mix_codes(*before, adjusted, weight)?;
                changed |= after != *before;
                pixels.push(after);
            }
        }
        job.check()?;
        let out = if changed {
            let image = image.rebuilt_rgba16(w, h, pixels, self.profile().clone())?;
            self.replacing_content(
                edit.layer,
                Content::Raster {
                    image,
                    offset: layer.offset(),
                },
            )?
        } else {
            None
        };
        job.report(1., 5)?;
        Ok(out)
    }
}
pub(crate) fn validate_coverage(doc: &PrecisionDocument, coverage: &[u16]) -> Result<(), String> {
    let (w, h) = doc.dimensions();
    if !coverage.is_empty() && coverage.len() != w as usize * h as usize {
        return Err("selection coverage must be empty or canvas-sized u16 samples".into());
    }
    Ok(())
}
pub(crate) fn coverage_at(
    coverage: &[u16],
    canvas: (u32, u32),
    source: (u32, u32),
    offset: (i32, i32),
    i: usize,
) -> u16 {
    if coverage.is_empty() {
        return u16::MAX;
    }
    let x = (i % source.0 as usize) as i64 + i64::from(offset.0);
    let y = (i / source.0 as usize) as i64 + i64::from(offset.1);
    if x < 0 || y < 0 || x >= i64::from(canvas.0) || y >= i64::from(canvas.1) {
        0
    } else {
        coverage[y as usize * canvas.0 as usize + x as usize]
    }
}
pub(crate) fn mix_codes(
    before: [u16; 4],
    after: [f32; 4],
    weight: u16,
) -> Result<[u16; 4], String> {
    if weight == 0 {
        return Ok(before);
    }
    if after.iter().any(|v| !v.is_finite()) {
        return Err("selected filter produced a nonfinite sample".into());
    }
    let preserves_alpha = after[3] == f32::from(before[3]) / 65535.;
    let after = after.map(|v| f64::from(v.clamp(0., 1.)));
    let old = before.map(|v| f64::from(v) / 65535.);
    let t = f64::from(weight) / 65535.;
    let mut result = [0.; 4];
    result[3] = old[3] + t * (after[3] - old[3]);
    if preserves_alpha {
        result[3] = old[3];
        for c in 0..3 {
            result[c] = old[c] + t * (after[c] - old[c]);
        }
    } else if result[3] > 0. {
        for c in 0..3 {
            result[c] = (old[c] * old[3] * (1. - t) + after[c] * after[3] * t) / result[3];
        }
    }
    Ok(result.map(|v| (v.clamp(0., 1.) * 65535.).round() as u16))
}
