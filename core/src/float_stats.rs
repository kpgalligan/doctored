//! Extended-range readouts for 32-bit float storage (Phase 11F): the
//! synchronous composite point sample (`PrecisionImage::float_sample`) and
//! the float arm of the schema2 scoped analysis (`evaluate`), which
//! `precision_scoped_source` dispatches to for f32 storage so Composite,
//! Active layer and Below active layer, with or without selection weighting,
//! share the u16/byte request parsing, coverage validation, envelope,
//! weighting, planning and admission in `scoped_statistics`.
//!
//! Two things make these different from the u16 readouts rather than a
//! rescaling of them. First, there is no maximum code: "clipping" is not a
//! meaningful count, so what is reported instead is ABOVE WHITE (`v > 1.0`)
//! and BELOW ZERO (`v < 0.0`) per channel, plus the exact minimum and maximum
//! seen. Second, a linear-light histogram binned linearly would put almost
//! every photographic sample in the bottom bin, so the bins are STOPS: 256
//! bins uniformly covering −12 EV to +6 EV, which is 18 stops of scene range
//! and four of them above diffuse white.
//!
//! `float_stats` is STOP-BINNED (−12…+6 EV, 256 bins) with above-white/below-zero counts instead of
//! clipping counts, because there is no maximum code; its histogram and weighted sample are the
//! float arm of the schema2 scoped analysis (`float_stats::evaluate`, dispatched by
//! `precision_scoped_source::evaluate_precision` on storage), so every source and selection
//! weighting answer at 32 bits, and `float_sample` stays the synchronous composite readout.
//!
//! A histogram request with `float_bins: "unit"` bins instead on the linear 0…1 axis,
//! `round(clamp(v, 0, 1) · 255)` (the u16 rule for `v = code / 65535`): that is the plot axis of a
//! float Levels or Curves sheet, whose 0…255 handles sit at linear 0…1, so a stop or
//! display-encoded plot would put each handle under other tones. Endpoint counts, weights, sums and
//! min/max are the same either way; Info and MCP `histogram` keep stops.

use serde_json::{json, Value};

use crate::job_control::JobContext;
use crate::precision_image::{PrecisionImage, PrecisionStorage};
use crate::scoped_statistics::{
    envelope, validate_coverage, weight, AnalysisOperation, AnalysisRequest, FloatBins,
    COVERAGE_DENOMINATOR, EMPTY_SAMPLE,
};

/// The histogram's exposure window, in stops relative to diffuse white.
const MIN_EV: f64 = -12.0;
const MAX_EV: f64 = 6.0;
const BINS: usize = 256;

/// The stop bin for one linear value. Values at or below `2^MIN_EV` (and
/// every negative one, which has no logarithm) land in bin 0; everything
/// from `2^MAX_EV` up lands in bin 255.
fn stop_bin(value: f32) -> usize {
    let ev = f64::from(value).max(2.0_f64.powf(MIN_EV)).log2();
    let position = (ev - MIN_EV) / (MAX_EV - MIN_EV) * BINS as f64;
    position.floor().clamp(0.0, (BINS - 1) as f64) as usize
}

/// The unit bin for one linear value: `round(clamp(v, 0, 1) · 255)` in f64,
/// so below 0 lands in bin 0 and above 1 in bin 255 (a NaN, which `clamp`
/// keeps, saturates to bin 0 in the cast).
fn unit_bin(value: f32) -> usize {
    (f64::from(value).clamp(0.0, 1.0) * 255.0).round() as usize
}

/// The sample readout's stop figure: `log2(max(v, 2^-16))`, so a zero or
/// negative mean reads as the floor rather than as −infinity.
fn stops(value: f64) -> f64 {
    value.max(2.0_f64.powi(-16)).log2()
}

fn profile_fields(image: &PrecisionImage, value: &mut Value) {
    let name = image.profile().name();
    value["profile_name"] = json!(name.chars().take(256).collect::<String>());
    value["profile_name_truncated"] = json!(name.chars().nth(256).is_some());
    value["profile_bytes"] = json!(image.profile().bytes().len());
    value["storage"] = json!(crate::float_document::STORAGE_FLOAT);
    value["schema_version"] = json!(1);
}

impl PrecisionImage {
    /// Exact point samples and a straight arithmetic mean over the same
    /// 1/3/5-per-side neighbourhood `PrecisionImage::sample` reads; outside
    /// positions are omitted, never clamped. `above_white`/`below_zero` are
    /// true when ANY sampled position's channel is outside [0, 1].
    pub fn float_sample(&self, x: i32, y: i32, reach: u32) -> Result<Value, String> {
        if self.storage() != PrecisionStorage::Rgba32Float {
            return Err("float sampling requires rgba32_float".into());
        }
        if reach > 2 {
            return Err("precision sample reach must be 0, 1 or 2".into());
        }
        let (w, h) = self.dimensions();
        let mut sum = [0_f64; 4];
        let mut count = 0_u64;
        let mut above = [false; 3];
        let mut below = [false; 3];
        let radius = i64::from(reach);
        for py in i64::from(y) - radius..=i64::from(y) + radius {
            for px in i64::from(x) - radius..=i64::from(x) + radius {
                if px < 0 || py < 0 || px >= i64::from(w) || py >= i64::from(h) {
                    continue;
                }
                let Some(pixel) = self.sample_at(px as u32, py as u32) else {
                    continue;
                };
                for (slot, value) in sum.iter_mut().zip(pixel) {
                    *slot += f64::from(value);
                }
                for c in 0..3 {
                    above[c] |= pixel[c] > 1.0;
                    below[c] |= pixel[c] < 0.0;
                }
                count += 1;
            }
        }
        if count == 0 {
            return Err("precision sample is outside the image".into());
        }
        let center = if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
            self.sample_at(x as u32, y as u32)
        } else {
            None
        };
        let means = sum.map(|v| v / count as f64);
        let mut value = json!({"x":x,"y":y,"reach":reach,"center_rgba32":center,
            "mean_rgba32":means,"sample_count":count,
            "above_white":above,"below_zero":below,
            "stops":[stops(means[0]),stops(means[1]),stops(means[2])],
            "counting":"all in-bounds pixels including alpha-zero hidden RGB; straight arithmetic mean of linear components"});
        profile_fields(self, &mut value);
        Ok(value)
    }
}

/// The float arm of the schema2 scoped analysis over a canvas-sized reader
/// (`read(i)` answers the straight linear sample at canvas index `i`, zero
/// outside the source). The loop shape, weighting and envelope are
/// `scoped_statistics::evaluate`'s; only the per-sample arithmetic differs:
/// stop bins instead of code bins, and below-zero / above-white endpoints
/// instead of zero / maximum codes.
pub(crate) fn evaluate(
    request: &AnalysisRequest,
    dimensions: (u32, u32),
    coverage: &[u16],
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Value, String> {
    let (w, h) = dimensions;
    validate_coverage(coverage, w, h)?;
    let mut result = match request.operation {
        AnalysisOperation::Histogram => {
            let bins = request.float_bins.unwrap_or_default();
            histogram(request.stride, bins, dimensions, coverage, &read, job)?
        }
        AnalysisOperation::Sample => sample(request, dimensions, coverage, &read, job)?,
    };
    envelope(request, coverage, &mut result);
    result["storage"] = json!(crate::float_document::STORAGE_FLOAT);
    Ok(result)
}

/// The weighted stop-binned histogram. RGB, luma and their endpoint counts
/// take alpha > 0 as eligibility (the schema2 rule); alpha counts every
/// positively weighted position. Stride thins only the plotted bins and the
/// `sampled_*` / `total` figures; every endpoint count, weight, sum and the
/// minimum / maximum inspect the complete scope. `bins_axis` picks only the
/// RGB and luma plot axis (stops or unit); alpha always bins on 0…1.
fn histogram(
    stride: u32,
    bins_axis: FloatBins,
    dimensions: (u32, u32),
    coverage: &[u16],
    read: &impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Value, String> {
    let count = dimensions.0 as usize * dimensions.1 as usize;
    let mut bins = [[0u64; BINS]; 4];
    let mut alpha_bins = [0u64; BINS];
    let mut zero = [0u64; 4];
    let mut below = [0u64; 4];
    let mut above = [0u64; 4];
    let mut one = [0u64; 4];
    let mut zero_weight = [0u64; 4];
    let mut one_weight = [0u64; 4];
    let mut below_weight = [0u64; 4];
    let mut above_weight = [0u64; 4];
    let mut sums = [0f64; 4];
    let mut minimum = [f32::INFINITY; 4];
    let mut maximum = [f32::NEG_INFINITY; 4];
    let mut total_weight = 0u64;
    let mut alpha_weight = 0u64;
    let mut sampled_weight = 0u64;
    let mut sampled_pixels = 0u64;
    let mut measured = 0u64;
    let bin = |v: f32| match bins_axis {
        FloatBins::Stops => stop_bin(v),
        FloatBins::Unit => unit_bin(v),
    };
    let mut count_channel = |c: usize, v: f32, weight: u64| {
        sums[c] += f64::from(v) * weight as f64;
        minimum[c] = minimum[c].min(v);
        maximum[c] = maximum[c].max(v);
        zero[c] += u64::from(v == 0.0);
        one[c] += u64::from(v == 1.0);
        below[c] += u64::from(v < 0.0);
        above[c] += u64::from(v > 1.0);
        zero_weight[c] += if v == 0.0 { weight } else { 0 };
        one_weight[c] += if v == 1.0 { weight } else { 0 };
        below_weight[c] += if v < 0.0 { weight } else { 0 };
        above_weight[c] += if v > 1.0 { weight } else { 0 };
    };
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
        count_channel(3, p[3], weight);
        if sampled {
            alpha_bins[(f64::from(p[3].clamp(0.0, 1.0)) * 255.0).round() as usize] += weight;
            sampled_pixels += 1;
        }
        if p[3] <= 0.0 {
            continue;
        }
        measured += 1;
        total_weight += weight;
        for c in 0..3 {
            count_channel(c, p[c], weight);
            if sampled {
                bins[c][bin(p[c])] += weight;
            }
        }
        if sampled {
            sampled_weight += weight;
            // Rec.709 over the LINEAR components, which is what luminance
            // means in a scene-linear document; the shared constants.
            let luma = crate::blend::LUMA_R * p[0].max(0.0)
                + crate::blend::LUMA_G * p[1].max(0.0)
                + crate::blend::LUMA_B * p[2].max(0.0);
            bins[3][bin(luma)] += weight;
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
    let finite = |values: [f32; 4]| values.map(|v| if v.is_finite() { v } else { 0.0 });
    job.report(1., 5)?;
    let mut result = json!({"bins":displayed,"bin_weight_units":rows,"alpha_bin_weight_units":alpha_bins.as_slice(),
        "channels":["red","green","blue","linear_rec709_luma"],
        "range":{"min_ev":MIN_EV,"max_ev":MAX_EV},"stride":stride,
        "total":sampled_weight as f64 / COVERAGE_DENOMINATOR as f64,"sampled_pixels":sampled_pixels,
        "sampled_rgb_weight_units":sampled_weight,"full_rgb_weight_units":total_weight,
        "full_alpha_weight_units":alpha_weight,"full_rgb_pixel_count":measured,
        "weighted_linear_sums_rgba":sums,
        "exact_zero_rgba":zero,"below_zero_rgba":below,"above_white_rgba":above,
        "exact_zero_weight_units_rgba":zero_weight,"below_zero_weight_units_rgba":below_weight,
        "above_white_weight_units_rgba":above_weight,
        "exact_one_rgba":one,"exact_one_weight_units_rgba":one_weight,
        "min_rgba":finite(minimum),"max_rgba":finite(maximum),
        "endpoint_scope":"complete scoped source, independent of display stride",
        "rgb_counting":"alpha > 0 is eligibility only; selection is rational coverage weight",
        "alpha_counting":"all positions with positive selection coverage, including alpha zero",
        "display_bin_rule":"stop bins: floor((log2(max(v, 2^-12)) + 12) / 18 * 256), clamped to [0,255]; luma before binning"});
    if bins_axis == FloatBins::Unit {
        result["float_bins"] = json!("unit");
        result["range"] = json!({"min":0.0,"max":1.0});
        result["display_bin_rule"] = json!("unit bins: round(clamp(v, 0, 1) * 255), below 0 in bin 0 and above 1 in bin 255; luma before binning");
    }
    Ok(result)
}

/// The weighted float sample over the 1/3/5-per-side neighbourhood: a
/// straight selection-weighted mean of the linear components (hidden RGB at
/// alpha zero included), with `above_white` / `below_zero` true when any
/// positively weighted position's channel is outside [0, 1].
fn sample(
    request: &AnalysisRequest,
    dimensions: (u32, u32),
    coverage: &[u16],
    read: &impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Value, String> {
    let (w, h) = dimensions;
    let (x, y) = (
        request.x.ok_or("missing sample x")?,
        request.y.ok_or("missing sample y")?,
    );
    let radius = i64::from(request.reach);
    let mut sums = [0f64; 4];
    let mut units = 0u64;
    let mut count = 0u64;
    let mut above = [false; 3];
    let mut below = [false; 3];
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
            let p = read(index);
            for (sum, value) in sums.iter_mut().zip(p) {
                *sum += f64::from(value) * weight as f64;
            }
            for c in 0..3 {
                above[c] |= p[c] > 1.0;
                below[c] |= p[c] < 0.0;
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
    let means = sums.map(|v| v / units as f64);
    job.report(1., 5)?;
    Ok(
        json!({"x":x,"y":y,"reach":request.reach,"sample_count":count,
        "center_rgba32":center,"weighted_sum_rgba32":sums,"weight_units":units,
        "mean_rgba32":means,"above_white":above,"below_zero":below,
        "stops":[stops(means[0]),stops(means[1]),stops(means[2])],
        "counting":"all positively weighted in-bounds pixels including hidden RGB at alpha zero; straight selection-weighted arithmetic mean of linear components"}),
    )
}
