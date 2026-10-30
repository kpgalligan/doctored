//! KERNEL: HDR Toning's Local Adaptation — the request, the
//! log range, the edge-aware base/detail split, the
//! per-pixel map, the Toning Curve's histogram and the post-render plan. Free
//! functions over linear RGB; the document adapter is `doc_hdr_toning`, the
//! request's other methods `hdr_toning`. Measured against Photoshop 26.10.0
//! at its defaults (`hdr-toning-564/take3`): it differs, 19.3 / 11.0
//! codes (max / mean, 8-bit scale) on the chart and 14.4 / 11.0 on the merged
//! bracket against a gate of 2 / 0.5, with Photoshop's controls other than
//! radius, strength, gamma and exposure at its own values; it ships as
//! declared. Photoshop applies a gamma of at most 2 (3.25 tones as 2 does).
//!
//! # The request
//!
//! `{"method":"local_adaptation", …}` with every key optional (defaults in
//! parentheses): `radius` 1…500 px (16), `strength` 0.1…4 (0.52),
//! `smooth_edges` (false), `gamma` 0.1…2 (1), `exposure` −5…5 stops (0),
//! `detail` −100…300 (30), `shadow` and `highlight` −100…100 (0), `vibrance`
//! −100…100 (0), `saturation` −100…100 (20) and `toning_curve`, 2–16
//! `[in, out]` points in 0…255 (the `curves` op's `rgb` list; the identity by
//! default). An unknown key and every out-of-range value refuse by name.
//!
//! # The map
//!
//! Each visible pixel (alpha > 0) has linear `c`, clamped to ≥ 0.
//!
//! 1. `Y = LUMA·c`, `ℓ = log2(max(Y, LOG_FLOOR))`. The range [`log_range`]
//!    is `low` (the 0.1 % alpha-weighted quantile of `ℓ`) and `span`
//!    (`max ℓ − low`, at least [`MIN_SPAN_STOPS`]).
//! 2. `t = clamp((ℓ − low)/span, 0, 1)`; `t′` the Toning Curve at `t`.
//! 3. `d = B(t′) − t′`, the guided base at the Radius
//!    (`sampling_presence::base_delta_in`, the same filter as Presence, Dehaze
//!    and Smart Sharpen), with an epsilon level of a step of `strength` stops
//!    (halved with Smooth Edges). Base `B = t′ + d`, detail `D = −d`.
//! 4. `L = (B − 1)·span·k + (1 + detail/100)·D·span + exposure`, with
//!    `k = min(1, DISPLAY_STOPS/span)/gamma`; `Y′ = 2^L`.
//! 5. `c′ = c·Y′/Y` (a neutral `Y′` for `Y = 0`), then Vibrance and
//!    Saturation (`adjust_color::Vibrance`, linear), then the caller's
//!    [`DisplayCurve`] (clip and encode).
//! 6. Shadow and Highlight: `ShadowsHighlights::sliders` on the encoded
//!    triple, guided by the encoded BASE luminance
//!    `2^((B − 1)·span·k + exposure)` clamped to 0…1, so the guide is the
//!    neighbourhood's tone rather than the pixel's own detail.

use serde_json::{Map, Value};

use crate::adjust_color::Vibrance;
use crate::adjust_curves::CurveLuts;
use crate::adjust_tone::ShadowsHighlights;
use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::equalize::Histogram;
use crate::filter_kernels::SampleRange;
use crate::hdr_toning::{DisplayCurve, LOG_FLOOR};
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec};
use crate::precision_sampling::adoption_copy_bytes;
use crate::sampling_gaussian::CONTROLLED_JOB_WORK;
use crate::sampling_presence::{base_delta_in, base_plan, RADIUS_LIMIT};

/// The fraction of the visible weight whose log luminance may sit below
/// `low` and clip to `t = 0`. With the plain minimum, one code-0 pixel
/// (`ℓ = −20`) would stretch every pixel's compression over twenty stops; at
/// a thousandth, a stray dark pixel or a sliver of black border moves `low`
/// by at most one range-histogram level.
const LOW_QUANTILE: f64 = 0.001;

/// The narrowest log range: a flat or nearly flat image would otherwise
/// divide by a span near zero, so its few hundredths of a stop of variation
/// would be stretched over the whole output range and the Strength epsilon
/// (in stops over the span) would grow without bound.
const MIN_SPAN_STOPS: f32 = 1.0;

/// The tone range the base is compressed into: eight stops, about what an
/// 8-bit display shows between black and white. A span within it is left at
/// its own contrast (`k = 1/gamma`); a wider one is scaled down to fit.
const DISPLAY_STOPS: f64 = 8.0;

/// The range histogram's levels: `equalize::Histogram`'s 16-bit count, so
/// one level is under 1/2600 of a stop across the 25-stop range a float file
/// spans.
const RANGE_LEVELS: usize = 65536;

/// The Toning Curve histogram's bins: one per 8-bit code of the curve editor.
const HISTOGRAM_BINS: usize = 256;

/// The Local Adaptation request (module doc).
#[derive(Clone)]
pub(crate) struct LocalAdaptation {
    radius: f64,
    strength: f64,
    smooth_edges: bool,
    gamma: f64,
    exposure: f64,
    detail: f64,
    /// `None` is the identity curve, skipped.
    curve: Option<CurveLuts>,
    /// Vibrance and Saturation as `adjust_color` parses them (the sliders
    /// over 100), built once.
    color: Vibrance,
    /// Shadow and Highlight through `ShadowsHighlights::sliders`; `None` when
    /// both are 0.
    tone: Option<ShadowsHighlights>,
}

/// One number key in `range`, its default when absent; a refusal names it.
fn number(
    object: &Map<String, Value>,
    key: &str,
    range: std::ops::RangeInclusive<f64>,
    default: f64,
    unit: &str,
) -> Result<f64, String> {
    let Some(value) = object.get(key) else {
        return Ok(default);
    };
    match value.as_f64() {
        Some(v) if v.is_finite() && range.contains(&v) => Ok(v),
        _ => Err(format!(
            "hdr toning {key} must be a number in {}…{}{unit}",
            range.start(),
            range.end()
        )),
    }
}

/// The Toning Curve: 2–16 `[in, out]` pairs, each value in 0…255, through
/// `CurveLuts::parse_precision` as the `curves` op's `rgb` list. `None` for
/// the identity.
fn toning_curve(value: &Value) -> Result<Option<CurveLuts>, String> {
    let refusal =
        || "hdr toning toning_curve must be 2 to 16 [in, out] points in 0…255".to_string();
    let points = value.as_array().ok_or_else(refusal)?;
    if !(2..=16).contains(&points.len()) {
        return Err(refusal());
    }
    for point in points {
        let pair = point.as_array().ok_or_else(refusal)?;
        let inside = |v: &Value| v.as_f64().is_some_and(|v| (0.0..=255.0).contains(&v));
        if pair.len() != 2 || !pair.iter().all(inside) {
            return Err(refusal());
        }
    }
    let mut params = Map::new();
    params.insert("rgb".into(), value.clone());
    let luts = CurveLuts::parse_precision(&params).ok_or_else(|| {
        "hdr toning toning_curve needs at least two points with different inputs".to_string()
    })?;
    Ok((!luts.is_identity()).then_some(luts))
}

impl LocalAdaptation {
    /// The request's keys besides `method` (module doc).
    pub(crate) fn parse(object: &Map<String, Value>) -> Result<Self, String> {
        const KEYS: [&str; 12] = [
            "method",
            "radius",
            "strength",
            "smooth_edges",
            "gamma",
            "exposure",
            "detail",
            "shadow",
            "highlight",
            "vibrance",
            "saturation",
            "toning_curve",
        ];
        if let Some(key) = object.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(format!(
                "\"{key}\" is not an argument of hdr toning's local_adaptation method"
            ));
        }
        let smooth_edges = match object.get("smooth_edges") {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("hdr toning smooth_edges must be true or false".into()),
        };
        let vibrance = number(object, "vibrance", -100.0..=100.0, 0.0, "")?;
        let saturation = number(object, "saturation", -100.0..=100.0, 20.0, "")?;
        let shadow = number(object, "shadow", -100.0..=100.0, 0.0, "")?;
        let highlight = number(object, "highlight", -100.0..=100.0, 0.0, "")?;
        let mut color = Map::new();
        color.insert("vibrance".into(), Value::from(vibrance / 100.0));
        color.insert("saturation".into(), Value::from(saturation / 100.0));
        let color =
            Vibrance::parse(&color).ok_or("hdr toning vibrance or saturation is out of range")?;
        Ok(Self {
            radius: number(object, "radius", 1.0..=500.0, 16.0, " pixels")?,
            strength: number(object, "strength", 0.1..=4.0, 0.52, "")?,
            smooth_edges,
            gamma: number(object, "gamma", 0.1..=2.0, 1.0, "")?,
            exposure: number(object, "exposure", -5.0..=5.0, 0.0, " stops")?,
            detail: number(object, "detail", -100.0..=300.0, 30.0, "")?,
            curve: object
                .get("toning_curve")
                .map(toning_curve)
                .transpose()?
                .flatten(),
            color,
            tone: (shadow != 0.0 || highlight != 0.0)
                .then(|| ShadowsHighlights::sliders(shadow, highlight, 0.0)),
        })
    }

    /// The guided filter's box half-width: the Radius rounded, at most
    /// `sampling_presence::RADIUS_LIMIT` (192), where its sigma reaches
    /// `sampling_denoise::plan_bounded`'s limit. A Radius above 192 runs at
    /// 192 — a recorded decision, not a refusal, so the full 1…500 range
    /// stays enterable.
    pub(crate) fn guided_radius(&self) -> u32 {
        (self.radius.round() as u32).clamp(1, RADIUS_LIMIT)
    }

    /// The guided filter's epsilon level on the `t` scale: a step of
    /// `strength` stops is `strength/span` there, so a window whose log
    /// luminance varies by much less is averaged into the base and a step
    /// much taller is followed. Smooth Edges halves it, so more edges are
    /// followed and fewer halos form.
    pub(crate) fn epsilon_level(&self, span: f32) -> f64 {
        let level = self.strength / f64::from(span);
        if self.smooth_edges {
            level / 2.0
        } else {
            level
        }
    }

    /// `k = min(1, DISPLAY_STOPS/span)/gamma` (module doc, step 4).
    fn compression(&self, span: f32) -> f64 {
        (DISPLAY_STOPS / f64::from(span)).min(1.0) / self.gamma
    }
}

/// The log range (module doc, step 1).
#[derive(Clone, Copy, Debug)]
pub(crate) struct LogRange {
    pub(crate) low: f32,
    pub(crate) span: f32,
}

impl LogRange {
    /// `t` of a log luminance, clamped (module doc, step 2).
    fn position(self, log: f32) -> f32 {
        ((log - self.low) / self.span).clamp(0.0, 1.0)
    }
}

/// A pixel's clamped linear triple, its luminance and its log luminance.
fn luminance(linear: [f32; 3]) -> ([f32; 3], f32, f32) {
    let c = linear.map(|v| v.max(0.0));
    let y = LUMA_R * c[0] + LUMA_G * c[1] + LUMA_B * c[2];
    (c, y, y.max(LOG_FLOOR).log2())
}

/// The module doc's range from two fresh passes over the pixels: the first
/// takes the smallest and largest log luminance, the second bins each into a
/// 65536-level `equalize::Histogram` over that span, weighted by alpha, and
/// `low` is the lower edge of the level `Histogram::quantile(LOW_QUANTILE)`
/// answers. Levels are `floor` bins, so every pixel below that edge is in a
/// lower level and their weight is under the quantile. No visible pixel, or a
/// single log level, answers its minimum (0 for none).
pub(crate) fn log_range<I: Iterator<Item = [f32; 4]>>(pixels: impl Fn() -> I) -> LogRange {
    let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
    for pixel in pixels().filter(|p| p[3] > 0.0) {
        let (_, _, log) = luminance([pixel[0], pixel[1], pixel[2]]);
        min = min.min(log);
        max = max.max(log);
    }
    if min > max {
        return LogRange {
            low: 0.0,
            span: MIN_SPAN_STOPS,
        };
    }
    let mut low = min;
    if max > min {
        if let Ok(mut histogram) = Histogram::new(RANGE_LEVELS) {
            let width = max - min;
            let scale = RANGE_LEVELS as f32 / width;
            for pixel in pixels().filter(|p| p[3] > 0.0) {
                let (_, _, log) = luminance([pixel[0], pixel[1], pixel[2]]);
                let level = (((log - min) * scale).max(0.0) as usize).min(RANGE_LEVELS - 1);
                histogram.add(level, f64::from(pixel[3]));
            }
            if let Some(level) = histogram.quantile(LOW_QUANTILE) {
                low = min + level as f32 * width / RANGE_LEVELS as f32;
            }
        }
    }
    LogRange {
        low,
        span: (max - low).max(MIN_SPAN_STOPS),
    }
}

/// What [`adapt`] keeps for the map pass: the range and the base delta `d`
/// per pixel, in raster order.
pub(crate) struct Adapted {
    range: LogRange,
    delta: Vec<f32>,
}

/// `t′` of one linear triple: the Toning Curve at `t` (module doc, step 2).
fn toned(la: &LocalAdaptation, range: LogRange, log: f32) -> f32 {
    let t = range.position(log);
    match &la.curve {
        Some(curve) => curve.apply([t; 3])[0],
        None => t,
    }
}

/// The range pass, then the `(t′, α)` plane, then the guided pass over it at
/// [`LocalAdaptation::guided_radius`] in `SampleRange::Unit` (`t′` is in 0…1)
/// with the work ceiling `cap`; the plane is dropped before this returns.
/// `pixels` yields the linear plane in raster order on each call.
pub(crate) fn adapt<I: Iterator<Item = [f32; 4]>>(
    la: &LocalAdaptation,
    pixels: impl Fn() -> I,
    dimensions: (u32, u32),
    cap: u64,
    job: JobContext<'_>,
) -> Result<Adapted, String> {
    let range = log_range(&pixels);
    job.check()?;
    job.range(0.0, 0.2).report(1.0, 5)?;
    let count = pixel_count(dimensions.0, dimensions.1)?;
    let mut plane: Vec<[f32; 2]> = reserved_vec(count)?;
    for (i, pixel) in pixels().enumerate() {
        job.every(i)?;
        let (_, _, log) = luminance([pixel[0], pixel[1], pixel[2]]);
        plane.push([toned(la, range, log), pixel[3]]);
    }
    if plane.len() != count {
        job.check()?;
        return Err("hdr toning read a plane of the wrong size".into());
    }
    let delta = base_delta_in(
        |i| {
            let [t, alpha] = plane[i];
            [t, t, t, alpha]
        },
        dimensions,
        la.guided_radius(),
        la.epsilon_level(range.span),
        cap,
        &SampleRange::Unit,
        job.range(0.2, 1.0),
    )?;
    drop(plane);
    Ok(Adapted { range, delta })
}

/// One LINEAR triple as an ENCODED triple in [0, 1] (module doc, steps 1–6);
/// `index` is its raster position in the plane [`adapt`] read.
pub(crate) fn map_pixel(
    la: &LocalAdaptation,
    adapted: &Adapted,
    index: usize,
    curve: &dyn DisplayCurve,
    linear: [f32; 3],
) -> [f32; 3] {
    let range = adapted.range;
    let (c, y, log) = luminance(linear);
    let t = f64::from(toned(la, range, log));
    let d = f64::from(adapted.delta.get(index).copied().unwrap_or(0.0));
    let span = f64::from(range.span);
    let k = la.compression(range.span);
    let base = (t + d - 1.0) * span * k + la.exposure;
    let stops = base + (1.0 + la.detail / 100.0) * -d * span;
    let target = stops.exp2();
    let scaled = if y > 0.0 {
        let gain = target / f64::from(y);
        c.map(|v| (f64::from(v) * gain) as f32)
    } else {
        [target as f32; 3]
    };
    let colored = la.color.saturation_linear(la.color.vibrance_linear(scaled));
    let encoded = curve.encode(colored);
    match &la.tone {
        Some(tone) => {
            let guide = (base.exp2() as f32).clamp(0.0, 1.0);
            tone.apply(encoded, curve.encode([guide; 3])[0])
        }
        None => encoded,
    }
}

/// The Toning Curve's histogram (module doc, step 1's range): 256
/// alpha-weighted bins of `round(255·t)`, `t` clamped and taken BEFORE the
/// curve (the editor draws the curve over its own input), and the range whose
/// ends are the axis' stops.
pub(crate) fn histogram<I: Iterator<Item = [f32; 4]>>(
    pixels: impl Fn() -> I,
) -> ([f64; HISTOGRAM_BINS], LogRange) {
    let range = log_range(&pixels);
    let mut bins = [0.0_f64; HISTOGRAM_BINS];
    for pixel in pixels().filter(|p| p[3] > 0.0) {
        let (_, _, log) = luminance([pixel[0], pixel[1], pixel[2]]);
        let bin = (range.position(log) * (HISTOGRAM_BINS - 1) as f32).round() as usize;
        bins[bin.min(HISTOGRAM_BINS - 1)] += f64::from(pixel[3]);
    }
    (bins, range)
}

/// The kernel's peak AFTER the render over a `width` × `height` canvas, with
/// the linear plane (16 B/px) held throughout: the larger of the guided stage
/// (the 8 B/px `(t′, α)` plane beside `sampling_presence::base_plan` at the
/// guided radius, under no work ceiling, `CONTROLLED_JOB_WORK`) and the map
/// stage (the 4 B/px delta beside the 8 B/px u16 output and its tiling,
/// `adoption_copy_bytes`); then the range histogram and the 256 bins.
pub(crate) fn plan_bytes(la: &LocalAdaptation, width: u32, height: u32) -> Result<u64, String> {
    let overflow = || "hdr toning working-memory calculation overflow".to_string();
    let count = pixel_count(width, height)? as u64;
    let (guided, _) = base_plan((width, height), la.guided_radius(), CONTROLLED_JOB_WORK)?;
    let guided_stage = count
        .checked_mul(8)
        .and_then(|bytes| bytes.checked_add(guided))
        .ok_or_else(overflow)?;
    let map_stage = count
        .checked_mul(4 + 8)
        .and_then(|bytes| bytes.checked_add(adoption_copy_bytes((width, height)).ok()?))
        .ok_or_else(overflow)?;
    count
        .checked_mul(16)
        .and_then(|linear| linear.checked_add(guided_stage.max(map_stage)))
        .and_then(|bytes| bytes.checked_add(RANGE_LEVELS as u64 * 8))
        .and_then(|bytes| bytes.checked_add(HISTOGRAM_BINS as u64 * 8))
        .ok_or_else(overflow)
}
