//! Full native-code auto endpoints and selection-weighted neutral candidates.
//! Scope selects the analysis population; it never changes write coverage.
//!
//! Automatic analysis explicitly chooses whole-target or weighted selection and derives full
//! native-code endpoints independently of write scope.
use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::job_control::{JobContext, EVERY_INTERVAL};
use crate::ops_auto::{
    contrast_endpoints, neutral_candidate, neutral_gamma_from_sums, tone_endpoints,
};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{reserved_vec, PrecisionRun, PrecisionStorage};
use crate::selected_adjustment::{coverage_at, validate_coverage, AnalysisScope, SelectedEdit};
use crate::tile_store::TILE_EDGE;
use serde_json::{json, Value};
#[derive(Clone, Debug)]
pub(crate) struct AutoLevels {
    pub black: [f32; 3],
    pub white: [f32; 3],
    pub gamma: [f32; 3],
}
impl AutoLevels {
    pub fn identity(&self) -> bool {
        self.black == [0.; 3] && self.white == [1.; 3] && self.gamma == [1.; 3]
    }
    pub fn json(&self) -> Value {
        json!({"black":self.black,"white":self.white,"gamma":self.gamma,"identity":self.identity()})
    }
    pub fn validate(edit: &SelectedEdit) -> Result<f64, String> {
        if edit.params.keys().any(|k| k != "clip") {
            return Err("automatic tone only accepts clip".into());
        }
        let clip = edit
            .params
            .get("clip")
            .map_or(Ok(0.001), |v| v.as_f64().ok_or("clip must be numeric"))?;
        if !(0.0..=0.1).contains(&clip) {
            return Err("clip must be in [0,0.1]".into());
        }
        Ok(clip)
    }
    pub fn derive(
        doc: &PrecisionDocument,
        edit: &SelectedEdit,
        coverage: &[u16],
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let clip = Self::validate(edit)?;
        validate_coverage(doc, coverage)?;
        job.check()?;
        let layer = doc
            .layer(edit.layer)
            .ok_or("invalid automatic tone layer")?;
        let image = layer
            .source()
            .ok_or("automatic tone requires raster source")?;
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("automatic tone requires native u16".into());
        }
        let mut bins = reserved_vec(4 * 65536)?;
        bins.resize(4 * 65536, 0_u64);
        let weight = |i| {
            if edit.analysis_scope == AnalysisScope::Selection {
                coverage_at(
                    coverage,
                    doc.dimensions(),
                    image.dimensions(),
                    layer.offset(),
                    i,
                )
            } else {
                u16::MAX
            }
        };
        let mut total = 0_u64;
        // TILE-BAND order (each 256-row band, each 256-column span, each
        // row's run): the counters are order-independent u64 sums. The flat
        // walk's `job.every(i)` is hoisted to each run: one check per
        // multiple of `EVERY_INTERVAL` in the run's visit range, so the
        // checkpoints are the flat walk's (13B-3e-3a).
        let (w, h) = image.dimensions();
        let mut visited = 0_usize;
        for band in (0..h).step_by(TILE_EDGE as usize) {
            for x0 in (0..w).step_by(TILE_EDGE as usize) {
                let x1 = x0.saturating_add(TILE_EDGE).min(w);
                for y in band..band.saturating_add(TILE_EDGE).min(h) {
                    for (x, run) in image.row_segments(y, x0, x1) {
                        let PrecisionRun::Rgba16(run) = run else {
                            return Err("automatic tone requires native u16".into());
                        };
                        let end = visited + run.len();
                        let mut at = visited.next_multiple_of(EVERY_INTERVAL);
                        while at < end {
                            job.check()?;
                            at += EVERY_INTERVAL;
                        }
                        visited = end;
                        let row = y as usize * w as usize + x as usize;
                        for (k, p) in run.iter().enumerate() {
                            if p[3] == 0 {
                                continue;
                            }
                            let n = u64::from(weight(row + k));
                            total += n;
                            for c in 0..3 {
                                bins[c * 65536 + usize::from(p[c])] += n;
                            }
                            let luma = (f64::from(LUMA_R) * f64::from(p[0])
                                + f64::from(LUMA_G) * f64::from(p[1])
                                + f64::from(LUMA_B) * f64::from(p[2]))
                            .round()
                            .clamp(0., 65535.) as usize;
                            bins[3 * 65536 + luma] += n;
                        }
                    }
                }
            }
        }
        let mut result = Self {
            black: [0.; 3],
            white: [1.; 3],
            gamma: [1.; 3],
        };
        if total == 0 {
            return Ok(result);
        }
        let cut = clip * total as f64;
        let unit = f64::from(u16::MAX);
        // A Lab Color document (#1 phase 2c): both Autos stretch the
        // Lightness plane alone — a\* and b\* are not tonal channels — so
        // a\* and b\* keep (0, 1, 1).
        if doc.is_lab() {
            if let Some((lo, hi)) = contrast_endpoints(&bins[..65536], cut, unit) {
                result.black[0] = lo;
                result.white[0] = hi;
            }
            job.check()?;
            return Ok(result);
        }
        if edit.op == "auto_contrast" {
            if let Some((lo, hi)) = contrast_endpoints(&bins[3 * 65536..], cut, unit) {
                result.black = [lo; 3];
                result.white = [hi; 3];
            }
        } else {
            let planes = [
                &bins[..65536],
                &bins[65536..2 * 65536],
                &bins[2 * 65536..3 * 65536],
            ];
            (result.black, result.white) = tone_endpoints(planes, cut, unit, |add| {
                // The joint white's pixel pass, in the Auto Color pass's
                // row-major order with its `job` checkpoints.
                let runs = image
                    .runs_rgba16()
                    .ok_or("automatic tone requires native u16")?;
                let mut next = 0_usize;
                for run in runs {
                    let end = next + run.len();
                    let mut at = next.next_multiple_of(EVERY_INTERVAL);
                    while at < end {
                        job.check()?;
                        at += EVERY_INTERVAL;
                    }
                    for p in run {
                        let i = next;
                        next += 1;
                        if p[3] == 0 {
                            continue;
                        }
                        let n = weight(i);
                        if n > 0 {
                            add(std::array::from_fn(|c| usize::from(p[c])), f64::from(n));
                        }
                    }
                }
                Ok(())
            })?;
        }
        if edit.op == "auto_color" {
            let mut sums = [0_f64; 3];
            let mut candidates = 0_u64;
            // Row-major: the f64 sums are order-dependent. `job.every(i)` is
            // hoisted to each run through `EVERY_INTERVAL` as in the
            // histogram pass.
            let runs = image
                .runs_rgba16()
                .ok_or("automatic tone requires native u16")?;
            let mut next = 0_usize;
            for run in runs {
                let end = next + run.len();
                let mut at = next.next_multiple_of(EVERY_INTERVAL);
                while at < end {
                    job.check()?;
                    at += EVERY_INTERVAL;
                }
                for p in run {
                    let i = next;
                    next += 1;
                    if p[3] == 0 {
                        continue;
                    }
                    let n = weight(i);
                    if n == 0 {
                        continue;
                    }
                    let rgb = std::array::from_fn::<_, 3, _>(|c| {
                        ((f32::from(p[c]) / 65535. - result.black[c])
                            / (result.white[c] - result.black[c]))
                            .clamp(0., 1.)
                    });
                    if !neutral_candidate(rgb) {
                        continue;
                    }
                    candidates += u64::from(n);
                    for c in 0..3 {
                        sums[c] += f64::from(rgb[c]) * f64::from(n);
                    }
                }
            }
            result.gamma = neutral_gamma_from_sums(sums, candidates, total);
        }
        job.check()?;
        Ok(result)
    }
}
