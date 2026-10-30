//! Complete scoped statistics over authoritative codes. Selection is a rational
//! weight, never an alpha multiplier or a hard contour. Display stride affects
//! only plot bins; endpoint counts and sums always inspect the complete scope.
//! An optional `auto` (backdrop histogram at stride 1 only) adds a Levels
//! Auto answer beside the histogram (`scoped_auto`); `evaluate` never sees it.
//!
//! Canvas-coordinate raw leaves copy no display plate; native group/backdrop reads use typed
//! compositing, byte cuts preserve compositor isolation/clipping. Exact integer sums and endpoint
//! counts scan all positive coverage independently of plot stride, with separate alpha eligibility
//! and coverage weight. Readouts include hidden RGB. `PrecisionImage::scoped_analysis` answers
//! composite requests over a supplied (retained) projection with the same schema
//! (`dr_precision_image_scoped_analysis{,_plan}`), and
//! `DrDocument::projection_scoped_analysis` the byte twin over a supplied canvas-sized 8-bit
//! projection (`dr_doc_projection_scoped_analysis{,_plan}`; hand-written beside the macro,
//! two handles), both planned by the one `projection_plan` and the byte arms evaluated by the one
//! `evaluate_rgba8`; layer/backdrop refuse before admission. The image-only schema1 RAW inspection
//! ABI remains unchanged.
//! `float_bins: "unit"` (histograms only) asks the float arm for 0…1 unit bins, the Levels/Curves
//! sheet axis; absent or `"stops"` keeps stop bins, and the byte and u16 arms ignore it.

use crate::job_control::JobContext;
use serde::Deserialize;
use serde_json::{json, Value};

pub const COVERAGE_DENOMINATOR: u64 = 65535;
pub const STATISTICS_SCRATCH: u64 = 2 * 1024 * 1024;
/// The one refusal of a sample whose region holds no positively weighted
/// pixel, at every storage (the float evaluator in `float_stats` shares it).
pub(crate) const EMPTY_SAMPLE: &str = "no positively weighted pixels in the sample region";

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisSource {
    Composite,
    Layer,
    Backdrop,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisOperation {
    Histogram,
    Sample,
}

/// The plot axis of a float histogram (`float_stats`): `Stops` (the
/// default, Info and MCP) or `Unit`, the linear 0…1 axis a float Levels or
/// Curves sheet's handles sit on. Code-storage histograms already bin on 0…1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatBins {
    #[default]
    Stops,
    Unit,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisRequest {
    pub operation: AnalysisOperation,
    pub source: AnalysisSource,
    pub layer: Option<usize>,
    pub below: Option<usize>,
    #[serde(default = "one")]
    pub stride: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    #[serde(default)]
    pub reach: u32,
    #[serde(default)]
    pub auto: Option<crate::scoped_auto::AutoAnalysis>,
    #[serde(default)]
    pub float_bins: Option<FloatBins>,
}
fn one() -> u32 {
    1
}

impl AnalysisRequest {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 4096 {
            return Err("analysis request exceeds 4096 bytes".into());
        }
        let result: Self = serde_json::from_str(text)
            .map_err(|e| format!("invalid scoped analysis request: {e}"))?;
        match result.source {
            AnalysisSource::Composite if result.layer.is_some() || result.below.is_some() => {
                return Err("composite analysis takes no layer or below boundary".into())
            }
            AnalysisSource::Layer if result.layer.is_none() || result.below.is_some() => {
                return Err("layer analysis requires layer and no below boundary".into())
            }
            AnalysisSource::Backdrop if result.below.is_none() || result.layer.is_some() => {
                return Err("backdrop analysis requires below and no layer".into())
            }
            _ => {}
        }
        match result.operation {
            AnalysisOperation::Histogram
                if result.x.is_some()
                    || result.y.is_some()
                    || result.reach != 0
                    || result.stride == 0 =>
            {
                return Err("histogram requires positive stride and no sample coordinates".into())
            }
            AnalysisOperation::Sample
                if result.x.is_none()
                    || result.y.is_none()
                    || result.reach > 2
                    || result.stride != 1 =>
            {
                return Err("sample requires x/y, reach 0...2 and stride 1".into())
            }
            _ => {}
        }
        if result.float_bins.is_some() && matches!(result.operation, AnalysisOperation::Sample) {
            return Err("float_bins requires a histogram".into());
        }
        if let Some(auto) = &result.auto {
            if !matches!(result.source, AnalysisSource::Backdrop)
                || !matches!(result.operation, AnalysisOperation::Histogram)
                || result.stride != 1
            {
                return Err("auto requires a backdrop histogram at stride 1".into());
            }
            if !auto.clip.is_finite() || !(0.0..=0.1).contains(&auto.clip) {
                return Err("auto clip must be in 0...0.1".into());
            }
        }
        Ok(result)
    }

    pub fn scope(&self) -> Value {
        match self.source {
            AnalysisSource::Composite => {
                json!({"source":"composite","semantics":"complete visible document composite in document profile"})
            }
            AnalysisSource::Layer => {
                json!({"source":"layer","layer":self.layer,"semantics":"raw layer pixels in canvas coordinates; a group is its raw projection (children over transparent, as Copy reads it); own opacity/blend/mask/style/Blend If/clipping/visibility excluded"})
            }
            AnalysisSource::Backdrop => {
                json!({"source":"backdrop","below":self.below,"semantics":"visible contributions with bottom-first record index less than below; enclosing group opacity/mask/isolation retained"})
            }
        }
    }
}

pub fn validate_coverage(coverage: &[u16], width: u32, height: u32) -> Result<(), String> {
    if !coverage.is_empty() && coverage.len() != width as usize * height as usize {
        return Err(
            "analysis coverage must be empty or one native u16 value per canvas pixel".into(),
        );
    }
    Ok(())
}

pub(crate) fn evaluate(
    request: &AnalysisRequest,
    dimensions: (u32, u32),
    maximum: u16,
    coverage: &[u16],
    read: impl Fn(usize) -> [u16; 4],
    job: JobContext<'_>,
) -> Result<Value, String> {
    let (w, h) = dimensions;
    validate_coverage(coverage, w, h)?;
    let mut result = match request.operation {
        AnalysisOperation::Histogram => {
            histogram(request.stride, dimensions, maximum, coverage, &read, job)?
        }
        AnalysisOperation::Sample => sample(request, dimensions, maximum, coverage, &read, job)?,
    };
    envelope(request, coverage, &mut result);
    result["maximum_code"] = json!(maximum);
    result["storage"] = json!(if maximum == u16::MAX {
        "rgba16_unorm"
    } else {
        "rgba8_unorm"
    });
    Ok(result)
}

/// The schema2 envelope every scoped answer carries, whatever its storage:
/// the version, the named scope and source, whether coverage weighted the
/// count and the coverage denominator.
pub(crate) fn envelope(request: &AnalysisRequest, coverage: &[u16], result: &mut Value) {
    result["schema_version"] = json!(2);
    result["scope"] = request.scope();
    result["source"] = result["scope"]["source"].clone();
    result["selection"] = json!(!coverage.is_empty());
    result["coverage_denominator"] = json!(COVERAGE_DENOMINATOR);
}

/// The coverage weight of canvas position `i`: its u16 coverage, or the full
/// denominator when no coverage was supplied.
pub(crate) fn weight(coverage: &[u16], i: usize) -> u64 {
    coverage
        .get(i)
        .map_or(COVERAGE_DENOMINATOR, |v| u64::from(*v))
}

fn histogram(
    stride: u32,
    dimensions: (u32, u32),
    maximum_code: u16,
    coverage: &[u16],
    read: &impl Fn(usize) -> [u16; 4],
    job: JobContext<'_>,
) -> Result<Value, String> {
    let count = dimensions.0 as usize * dimensions.1 as usize;
    let mut bins = [[0u64; 256]; 4];
    let mut alpha_bins = [0u64; 256];
    let mut zero = [0u64; 4];
    let mut maximum = [0u64; 4];
    let mut zero_weight = [0u64; 4];
    let mut maximum_weight = [0u64; 4];
    let mut sums = [0u64; 4];
    let mut total_weight = 0u64;
    let mut alpha_weight = 0u64;
    let mut sampled_weight = 0u64;
    let mut sampled_pixels = 0u64;
    let mut measured = 0u64;
    let mut luma_zero = 0u64;
    let mut luma_max = 0u64;
    let mut luma_zero_weight = 0u64;
    let mut luma_max_weight = 0u64;
    let bin = |code: u16| (f64::from(code) * 255. / f64::from(maximum_code)).round() as usize;
    for i in 0..count {
        if i & 4095 == 0 {
            job.report(i as f64 / count as f64, 6)?;
        }
        let weight = weight(coverage, i);
        if weight == 0 {
            continue;
        }
        let p = read(i);
        let sampled = i % stride as usize == 0;
        alpha_weight += weight;
        sums[3] += u64::from(p[3]) * weight;
        zero[3] += u64::from(p[3] == 0);
        maximum[3] += u64::from(p[3] == maximum_code);
        zero_weight[3] += if p[3] == 0 { weight } else { 0 };
        maximum_weight[3] += if p[3] == maximum_code { weight } else { 0 };
        if sampled {
            alpha_bins[bin(p[3])] += weight;
            sampled_pixels += 1;
        }
        if p[3] == 0 {
            continue;
        }
        measured += 1;
        total_weight += weight;
        for c in 0..3 {
            sums[c] += u64::from(p[c]) * weight;
            zero[c] += u64::from(p[c] == 0);
            maximum[c] += u64::from(p[c] == maximum_code);
            zero_weight[c] += if p[c] == 0 { weight } else { 0 };
            maximum_weight[c] += if p[c] == maximum_code { weight } else { 0 };
            if sampled {
                bins[c][bin(p[c])] += weight;
            }
        }
        let black = p[..3].iter().all(|v| *v == 0);
        let white = p[..3].iter().all(|v| *v == maximum_code);
        luma_zero += u64::from(black);
        luma_max += u64::from(white);
        luma_zero_weight += if black { weight } else { 0 };
        luma_max_weight += if white { weight } else { 0 };
        if sampled {
            sampled_weight += weight;
            let luma = f64::from(crate::blend::LUMA_R) * f64::from(p[0])
                + f64::from(crate::blend::LUMA_G) * f64::from(p[1])
                + f64::from(crate::blend::LUMA_B) * f64::from(p[2]);
            bins[3][(luma * 255. / f64::from(maximum_code))
                .round()
                .clamp(0., 255.) as usize] += weight;
        }
    }
    let rows: Vec<&[u64]> = bins.iter().map(|r| r.as_slice()).collect();
    let displayed: Vec<Vec<f64>> = bins
        .iter()
        .map(|r| {
            r.iter()
                .map(|v| *v as f64 / COVERAGE_DENOMINATOR as f64)
                .collect()
        })
        .collect();
    job.report(1., 5)?;
    Ok(
        json!({"bins":displayed,"bin_weight_units":rows,"alpha_bin_weight_units":alpha_bins.as_slice(),
        "channels":["red","green","blue","encoded_rec709_luma"],"stride":stride,
        "total":sampled_weight as f64 / COVERAGE_DENOMINATOR as f64,"sampled_pixels":sampled_pixels,
        "sampled_rgb_weight_units":sampled_weight,"full_rgb_weight_units":total_weight,
        "full_alpha_weight_units":alpha_weight,"full_rgb_pixel_count":measured,
        "weighted_code_sums_rgba":sums,
        "exact_zero_rgba":zero,"exact_max_rgba":maximum,
        "exact_zero_weight_units_rgba":zero_weight,"exact_max_weight_units_rgba":maximum_weight,
        "exact_zero_luma":luma_zero,"exact_max_luma":luma_max,
        "exact_zero_weight_units_luma":luma_zero_weight,"exact_max_weight_units_luma":luma_max_weight,
        "endpoint_scope":"complete scoped source, independent of display stride",
        "rgb_counting":"alpha > 0 is eligibility only; selection is rational coverage weight",
        "alpha_counting":"all positions with positive selection coverage, including alpha zero",
        "display_bin_rule":"round(native_code * 255 / maximum_code); luma before binning"}),
    )
}

fn sample(
    request: &AnalysisRequest,
    dimensions: (u32, u32),
    maximum: u16,
    coverage: &[u16],
    read: &impl Fn(usize) -> [u16; 4],
    job: JobContext<'_>,
) -> Result<Value, String> {
    let (w, h) = dimensions;
    let (x, y) = (
        request.x.ok_or("missing sample x")?,
        request.y.ok_or("missing sample y")?,
    );
    let radius = i64::from(request.reach);
    let mut sums = [0u64; 4];
    let mut units = 0u64;
    let mut count = 0u64;
    for py in i64::from(y) - radius..=i64::from(y) + radius {
        for px in i64::from(x) - radius..=i64::from(x) + radius {
            job.check()?;
            if px < 0 || py < 0 || px >= i64::from(w) || py >= i64::from(h) {
                continue;
            }
            let index = py as usize * w as usize + px as usize;
            let weight = weight(coverage, index);
            if weight == 0 {
                continue;
            }
            for (sum, code) in sums.iter_mut().zip(read(index)) {
                *sum += u64::from(code) * weight;
            }
            units += weight;
            count += 1;
        }
    }
    if units == 0 {
        return Err(EMPTY_SAMPLE.into());
    }
    let center = if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
        Some(read(y as usize * w as usize + x as usize))
    } else {
        None
    };
    let means = sums.map(|v| v as f64 / units as f64);
    job.report(1., 5)?;
    Ok(
        json!({"x":x,"y":y,"reach":request.reach,"sample_count":count,
        "center_rgba_codes":center,"center_rgba16":if maximum==u16::MAX {center} else {None},
        "weighted_sum_rgba_codes":sums,"weight_units":units,"mean_rgba_codes":means,
        "mean_rgba16":if maximum==u16::MAX {Some(means)} else {None},
        "normalized_rgba":means.map(|v|v/f64::from(maximum)),
        "counting":"all positively weighted in-bounds pixels including hidden RGB at alpha zero; straight selection-weighted arithmetic mean"}),
    )
}
