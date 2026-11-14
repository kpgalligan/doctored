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
//! Each visible pixel (alpha > 0) has linear `c`, clamped to ≥ 0; `Yˢ` is
//! `LUMA·c` of the unclamped triple.
//!
//! 1. `Y = LUMA·c`, `ℓ = log2(max(Y, LOG_FLOOR))`. The range [`log_range`]
//!    is `low` (the 0.1 % alpha-weighted quantile of `ℓ`) and `span`
//!    (`max ℓ − low`, at least [`MIN_SPAN_STOPS`]). The black floor `F`
//!    ([`FloorEvidence`]): `ℓ₀` is the same quantile over the visible pixels
//!    that are not exactly black (every channel 0), and the band is those of
//!    them with `ℓ < ℓ₀ +` [`FLOOR_BAND_STOPS`]. When the band's pixels with a
//!    channel below zero carry at least [`FLOOR_SHARE`] of its weight, `F` is
//!    the alpha-weighted median of their `|Yˢ|`, read from 256 levels of
//!    `log2`, eight a stop up from `LOG_FLOOR`, at the level's centre, and it
//!    is kept when `2^(max ℓ)` is at least [`FLOOR_TOP_SCALE`]·κF (κ of
//!    step 5); otherwise `F = 0`.
//! 2. `t = clamp((ℓ − low)/span, 0, 1)`; `t′` the Toning Curve at `t`.
//! 3. `d = B(t′) − t′`, the guided base at the Radius
//!    (`sampling_presence::base_delta_in`, the same filter as Presence, Dehaze
//!    and Smart Sharpen), with an epsilon level of a step of `strength` stops
//!    (halved with Smooth Edges). Base `B = t′ + d`, detail `D = −d`.
//! 4. `L = (B − 1)·span·k + (1 + detail/100)·D·span + exposure`, with
//!    `k = min(1, DISPLAY_STOPS/span)/gamma`; `Y′ = 2^L`.
//! 5. `S = max(Yˢ, 0)` and the chroma weight `w = S²/(S² + (κF)²)`, κ =
//!    [`FLOOR_CHROMA_SCALE`]: `w = 0` at `S = 0`, and `w = 1` for `S > 0`
//!    when `F = 0`. `c′ = Y′·((1 − w) + w·c/Y)` (`c·Y′/Y` at `w = 1`; its
//!    luminance is `Y′` at every `w`), a neutral `Y′` for `Y = 0`; then
//!    Vibrance and Saturation (`adjust_color::Vibrance`, linear), then the
//!    caller's [`DisplayCurve`] (clip and encode).
//! 6. Shadow and Highlight: `ShadowsHighlights::sliders` on the encoded
//!    triple, guided by the encoded BASE luminance
//!    `2^((B − 1)·span·k + exposure)` clamped to 0…1, so the guide is the
//!    neighbourhood's tone rather than the pixel's own detail.
//!
//! # The black floor
//!
//! A pixel keeps its colour `c/Y` at its new luminance, and the map lifts the
//! bottom of the range the most, so a pixel whose channels are a capture's
//! black level and noise rather than light would come out a saturated
//! primary. A capture shows where that floor is: its black level and noise
//! push channels below zero there, while the darkest colours a scene holds
//! keep every channel at or above zero. So the floor is read from the
//! darkest stop of the range, and only when pixels below zero make up a real
//! share of it: a stray pixel, a sliver of colour outside the gamut or an
//! exact-black border sets no floor, and the median keeps a few large
//! outliers from setting its level. A pixel whose signed luminance is not
//! above zero holds no light and lifts as a neutral, as a black one does. The
//! floor lifts as a neutral grey, and a colour keeps more of its chroma the
//! further it lies above it: `w` is 0.06 at `2F`, a half at `κF` (`8F`) and
//! 0.98 at `7κF` (`56F`, about six stops up). A floor lies far under the
//! picture's light, so one is kept only when the range's top reaches `7κF`:
//! the fade never reaches the brightest colours, and a picture of a few
//! stops whose darkest stop is a colour outside the gamut (a field of one
//! saturated colour) sets none and keeps its colour. A picture of wider range
//! whose darkest stop is at least a quarter colour outside the gamut (a
//! render, a document converted to a narrower profile) does read that stop as
//! a floor, and its colours there fade over the six stops above it as a
//! capture's noise does. The tone (`Y`, `ℓ`, the range, the base and the
//! histogram) does not read `F`: only chroma fades. A document with no
//! channel below zero (every 8- and 16-bit source, every non-negative float)
//! has `F = 0` and keeps `c·Y′/Y` exactly. A float file whose noise was
//! clipped at zero before it arrived shows no floor and is mapped as before.

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

/// The black floor's band (module doc, "The black floor"): the darkest stop
/// above `ℓ₀`. A capture's noise spreads its floor over about a stop, and the
/// darkest colours a scene keeps sit above it.
const FLOOR_BAND_STOPS: f32 = 1.0;

/// The share of the band's weight its pixels with a channel below zero must
/// carry to set a floor. At a RAW's floor developed into linear light most of
/// the darkest stop's pixels hold one (half to three quarters measured on a
/// real file); a stray pixel or a few colours outside the gamut among the
/// darkest are a small share of a stop.
const FLOOR_SHARE: f64 = 0.25;

/// The floor histogram's levels: `equalize::Histogram`'s 256, eight a stop
/// up from `LOG_FLOOR` (32 stops; a larger `|Yˢ|` counts at the top), so the
/// median is read to an eighth of a stop.
const FLOOR_LEVELS: usize = 256;
const FLOOR_LEVELS_PER_STOP: f32 = 8.0;

/// κ, the chroma weight's scale over the floor (module doc, step 5): a pixel
/// at `S = κF` keeps half its chroma. At a capture's floor one channel runs
/// several times the luminance (a RAW's blue up to about eight times), so a
/// pixel's chroma is the floor's until its luminance is several times the
/// floor's: at 8 the weight is 0.06 at twice the floor and 0.98 at 56 times.
const FLOOR_CHROMA_SCALE: f64 = 8.0;

/// How far over the floor the range's top must reach for `F` to be kept, as a
/// multiple of `κF` (module doc, "The black floor"): at `7κF` the chroma
/// weight is 0.98, so the fade never reaches a picture's brightest colours.
/// A capture's floor lies over twelve stops under its highlights on the real
/// file the rule was measured on, against the 5.8 this asks; a field of one
/// saturated colour outside the gamut has its darkest stop at its top.
const FLOOR_TOP_SCALE: f64 = 7.0;

/// The request's largest `gamma` (module doc). `hdr_toning_preset_file`
/// clamps a preset file's larger gamma to it.
pub(crate) const MAX_GAMMA: f64 = 2.0;

/// The point counts the request's `toning_curve` takes (module doc).
pub(crate) const TONING_CURVE_POINTS: std::ops::RangeInclusive<usize> = 2..=16;

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
    if !TONING_CURVE_POINTS.contains(&points.len()) {
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
            gamma: number(object, "gamma", 0.1..=MAX_GAMMA, 1.0, "")?,
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
    /// `ℓ₀`, the black floor's band starts from it (module doc, step 1).
    floor_low: f32,
    /// `max ℓ`, the top a black floor must lie under (module doc, step 1).
    high: f32,
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

/// `Yˢ`, the luminance of the unclamped triple (module doc, "The map").
fn signed_luminance(linear: [f32; 3]) -> f32 {
    LUMA_R * linear[0] + LUMA_G * linear[1] + LUMA_B * linear[2]
}

/// The module doc's range from two fresh passes over the pixels: the first
/// takes the smallest and largest log luminance, the second bins each into a
/// 65536-level `equalize::Histogram` over that span, weighted by alpha, and
/// `low` is the lower edge of the level `Histogram::quantile(LOW_QUANTILE)`
/// answers. Levels are `floor` bins, so every pixel below that edge is in a
/// lower level and their weight is under the quantile. `ℓ₀` reads the same
/// histogram past the exact-black pixels' weight, which all sits in the
/// lowest level. No visible pixel, or a single log level, answers its
/// minimum (0 for none) for both; the top is the largest (0 for none).
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
            floor_low: 0.0,
            high: 0.0,
        };
    }
    let (mut low, mut floor_low) = (min, min);
    if max > min {
        if let Ok(mut histogram) = Histogram::new(RANGE_LEVELS) {
            let width = max - min;
            let scale = RANGE_LEVELS as f32 / width;
            let (mut total, mut black) = (0.0_f64, 0.0_f64);
            for pixel in pixels().filter(|p| p[3] > 0.0) {
                let (_, _, log) = luminance([pixel[0], pixel[1], pixel[2]]);
                let level = (((log - min) * scale).max(0.0) as usize).min(RANGE_LEVELS - 1);
                let weight = f64::from(pixel[3]);
                histogram.add(level, weight);
                total += weight;
                if pixel[..3] == [0.0; 3] {
                    black += weight;
                }
            }
            let edge = |level: usize| min + level as f32 * width / RANGE_LEVELS as f32;
            if let Some(level) = histogram.quantile(LOW_QUANTILE) {
                low = edge(level);
            }
            floor_low = low;
            if black > 0.0 && total > black {
                let fraction = (black + LOW_QUANTILE * (total - black)) / total;
                if let Some(level) = histogram.quantile(fraction) {
                    floor_low = edge(level);
                }
            }
        }
    }
    LogRange {
        low,
        span: (max - low).max(MIN_SPAN_STOPS),
        floor_low,
        high: max,
    }
}

/// The black floor's evidence (module doc, step 1), gathered in [`adapt`]'s
/// plane pass: the band's weight, and the `|Yˢ|` levels of its pixels with a
/// channel below zero. Its 2 KB is `plan_bytes`'s 256-bin term.
struct FloorEvidence {
    band_top: f32,
    range_top: f32,
    band: f64,
    below_zero: f64,
    levels: Histogram,
}

impl FloorEvidence {
    fn new(range: LogRange) -> Result<Self, String> {
        Ok(Self {
            band_top: range.floor_low + FLOOR_BAND_STOPS,
            range_top: range.high,
            band: 0.0,
            below_zero: 0.0,
            levels: Histogram::new(FLOOR_LEVELS)?,
        })
    }

    /// One pixel: its linear triple, its `ℓ` and its alpha.
    fn add(&mut self, linear: [f32; 3], log: f32, alpha: f32) {
        if alpha.is_nan() || alpha <= 0.0 || log >= self.band_top || linear == [0.0; 3] {
            return;
        }
        let weight = f64::from(alpha);
        self.band += weight;
        if linear.iter().any(|&v| v < 0.0) {
            self.below_zero += weight;
            let magnitude = signed_luminance(linear).abs().max(LOG_FLOOR);
            let level = (magnitude.log2() - LOG_FLOOR.log2()) * FLOOR_LEVELS_PER_STOP;
            self.levels.add(level as usize, weight);
        }
    }

    /// `F`, or 0 when the band's pixels below zero carry under
    /// [`FLOOR_SHARE`] of its weight or the range's top is under
    /// [`FLOOR_TOP_SCALE`]·κF.
    fn floor(&self) -> f32 {
        if !(self.below_zero > 0.0 && self.below_zero >= FLOOR_SHARE * self.band) {
            return 0.0;
        }
        let Some(level) = self.levels.quantile(0.5) else {
            return 0.0;
        };
        let floor = (LOG_FLOOR.log2() + (level as f32 + 0.5) / FLOOR_LEVELS_PER_STOP).exp2();
        let reach = FLOOR_TOP_SCALE * FLOOR_CHROMA_SCALE * f64::from(floor);
        if reach > f64::from(self.range_top).exp2() {
            return 0.0;
        }
        floor
    }
}

/// What [`adapt`] keeps for the map pass: the range, the base delta `d` per
/// pixel in raster order, and the black floor `F` (0 for none).
pub(crate) struct Adapted {
    range: LogRange,
    delta: Vec<f32>,
    floor: f32,
}

/// `t′` of one linear triple: the Toning Curve at `t` (module doc, step 2).
fn toned(la: &LocalAdaptation, range: LogRange, log: f32) -> f32 {
    let t = range.position(log);
    match &la.curve {
        Some(curve) => curve.apply([t; 3])[0],
        None => t,
    }
}

/// The range pass, then the `(t′, α)` plane with the black floor's evidence,
/// then the guided pass over the plane at [`LocalAdaptation::guided_radius`]
/// in `SampleRange::Unit` (`t′` is in 0…1) with the work ceiling `cap`; the
/// plane is dropped before this returns. `pixels` yields the linear plane in
/// raster order on each call.
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
    let mut evidence = FloorEvidence::new(range)?;
    for (i, pixel) in pixels().enumerate() {
        job.every(i)?;
        let linear = [pixel[0], pixel[1], pixel[2]];
        let (_, _, log) = luminance(linear);
        plane.push([toned(la, range, log), pixel[3]]);
        evidence.add(linear, log, pixel[3]);
    }
    let floor = evidence.floor();
    drop(evidence);
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
    Ok(Adapted {
        range,
        delta,
        floor,
    })
}

/// The share of a pixel's own chroma the map keeps (module doc, step 5):
/// `S²/(S² + (κF)²)` on `S = max(Yˢ, 0)`; 0 at `S = 0`, and 1 above it when
/// `floor` is 0.
fn chroma_weight(linear: [f32; 3], floor: f32) -> f64 {
    let signal = f64::from(signed_luminance(linear));
    if signal <= 0.0 {
        0.0
    } else if floor > 0.0 {
        let noise = FLOOR_CHROMA_SCALE * f64::from(floor);
        signal * signal / (signal * signal + noise * noise)
    } else {
        1.0
    }
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
    let weight = chroma_weight(linear, adapted.floor);
    let scaled = if y > 0.0 && weight > 0.0 {
        let gain = target / f64::from(y);
        if weight < 1.0 {
            c.map(|v| (target + (f64::from(v) * gain - target) * weight) as f32)
        } else {
            c.map(|v| (f64::from(v) * gain) as f32)
        }
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
/// `adoption_copy_bytes`); then the range histogram and 256 levels (the
/// Toning Curve's bins, or the black floor's histogram in the plane pass).
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
