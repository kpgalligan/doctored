//! KERNEL: Image ▸ Adjustments ▸ HDR Toning… — the request,
//! the three GLOBAL methods, the plane statistics they read and the
//! post-render memory plan. Free functions over linear RGB triples; the
//! document adapters are `doc_hdr_toning`. Local Adaptation (the fourth
//! method, spatial, phase 4b) parses here into [`HdrRequest::Local`] and is
//! the `hdr_local_adaptation` kernel.
//!
//! # The request
//!
//! JSON, read by hand so an unknown key refuses:
//! `{"method":"exposure_gamma","exposure":f,"gamma":f}` (exposure in −20…20
//! stops, gamma 0.01…9.99, defaults 0 and 1 — `adjust_tone::Exposure::parse`'s
//! own ranges, read through it), `{"method":"highlight_compression"}`,
//! `{"method":"equalize_histogram"}` and `{"method":"local_adaptation", …}`
//! (its keys and ranges are `hdr_local_adaptation`'s module doc).
//!
//! # The maps
//!
//! Each map takes a LINEAR triple in the document's primaries and answers an
//! ENCODED triple in [0, 1] (the document profile's own forward curve, the
//! caller's [`DisplayCurve`]); alpha never passes through a map. These are the
//! declared default members of the Photoshop take
//! (`scripts/photoshop-probe/hdr_toning_564_take.py`):
//!
//! * **Exposure and Gamma (EG-LIN)** — `adjust_tone::Exposure::apply_linear`
//!   per channel (gain `2^exposure`, no pedestal, the reciprocal power
//!   `1/gamma`, `adjust_tone::Exposure`'s direction), then the clip and the
//!   curve. Measured against Photoshop 26.10.0 (`hdr-toning-564/take3`;
//!   max / mean codes at 8-bit scale, chart and merged bracket, gate 2 / 0.5):
//!   (0, 1) matches (0.02 / 0.002); (+1.5, 1.8) misses by up to 117 / 54 and
//!   (−2, 0.6) by up to 108 / 54. Highlight Compression misses (85 / 5.4,
//!   21 / 1.9) and Equalize Histogram misses (70 / 16, 21 / 13). No fitted
//!   family met the gate on the held-out merged rows, so these members ship
//!   as declared, their numbers pinned by `float_hdr_toning_photoshop.rs`.
//! * **Highlight Compression (HC-RW)** — the extended Reinhard curve
//!   (`float_document::ToneMap::Reinhard`) per channel with its white point the
//!   image's largest RGB sample, held to [1, `FLOAT_RANGE_MAX`]: the brightest
//!   sample lands exactly at white and nothing clips. An image whose samples
//!   are all at or below 1 has white 1, which is then the clip's own curve
//!   bent below 1 — the method always compresses.
//! * **Equalize Histogram (EH-LOGSUM)** — one `equalize::Histogram` of 65536
//!   levels over every R, G and B sample's `log2(max(v, 2⁻²⁰))`, spread
//!   linearly over the image's own log range, each sample weighted by its
//!   alpha (alpha 0 adds nothing). The table's `lut / 65535` IS the encoded
//!   value of each channel: equalization spreads the image's log tones over
//!   the whole output range, so it is the output curve itself. With fewer
//!   than two occupied levels there is nothing to spread and every sample
//!   maps through the clip and the curve.
//!
//! # Two passes, no buffer
//!
//! Equalize Histogram needs the log range before any sample can be binned,
//! so [`analyse`] takes a source it can read again — `pixels` returns a fresh
//! iterator over the rendered runs on each call — and makes a range pass and
//! then a histogram pass. No per-sample buffer exists, so [`plan_bytes`]
//! counts none for the global methods; Local Adaptation's guided pass holds
//! its own planes (`hdr_local_adaptation::plan_bytes`).

use serde_json::{Map, Value};

use crate::adjust_tone::Exposure;
use crate::equalize::Histogram;
use crate::float_document::{ToneMap, FLOAT_RANGE_MAX};
use crate::hdr_local_adaptation::{self, LocalAdaptation};
use crate::precision_image::{pixel_count, BOOKKEEPING_BYTES};
use crate::precision_sampling::{adoption_copy_bytes, float_adoption_copy_bytes};

/// The feature's own allowance per canvas pixel on top of the stage terms
/// [`plan_bytes`] states: allocator rounding of the tiled planes, the
/// transient iterator state the terms do not name, and the display
/// preparation and publication inside the same measured call. Held to the
/// house rule by `scripts/stack-admission/measure.py`'s tone cases (gate
/// M-tone, a Release app at 24 MP, three runs): the 16-bit source's peak of
/// 965,067,472 bytes needs ⌈1.25 ×⌉ = 1,206,334,340, and 11 is the smallest
/// integer that passes it (10 does not; the 8-bit source needs 8). A miss
/// raises this constant, never a shared one.
pub(crate) const HDR_TONING_MARGIN_BYTES_PER_PIXEL: u64 = 11;

/// The darkest linear value Equalize Histogram distinguishes, 2⁻²⁰: twenty
/// stops below diffuse white, past the darkest step a 16-bit sRGB code
/// decodes to (about 2⁻¹⁹·⁶), so every stored code keeps its own log level
/// while zero and the negatives a float file may carry share the floor.
/// Local Adaptation's log luminance shares it (`hdr_local_adaptation`).
pub(crate) const LOG_FLOOR: f32 = 1.0 / 1_048_576.0;

/// The equalization's level count: the 16-bit output's own code count.
const LEVELS: usize = 65536;

/// The heap the Equalize Histogram statistics hold: the f64 bins and the u16
/// table beside them.
const HISTOGRAM_BYTES: u64 = (LEVELS as u64) * (8 + 2);

/// One parsed HDR Toning request: a global method, or Local Adaptation.
pub(crate) enum HdrRequest {
    Global(HdrMethod),
    Local(Box<LocalAdaptation>),
}

/// One parsed global HDR Toning method.
#[derive(Clone)]
pub(crate) enum HdrMethod {
    ExposureGamma(Exposure),
    HighlightCompression,
    EqualizeHistogram,
}

/// The method's wire name — the request's `method`.
fn method_name(method: &HdrMethod) -> &'static str {
    match method {
        HdrMethod::ExposureGamma(_) => "exposure_gamma",
        HdrMethod::HighlightCompression => "highlight_compression",
        HdrMethod::EqualizeHistogram => "equalize_histogram",
    }
}

/// The module doc's request. `Err` names what is wrong: not JSON, not an
/// object, no or an unknown `method`, a key the method does not take, and a
/// value outside its range (Local Adaptation's are
/// `LocalAdaptation::parse`'s).
pub(crate) fn parse(json: &str) -> Result<HdrRequest, String> {
    let value: Value = serde_json::from_str(json)
        .map_err(|e| format!("hdr toning request is not valid JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or("an hdr toning request must be a JSON object")?;
    let name = object
        .get("method")
        .and_then(Value::as_str)
        .ok_or("an hdr toning request requires a \"method\" name")?;
    let method = match name {
        "exposure_gamma" => HdrMethod::ExposureGamma(exposure(object)?),
        "highlight_compression" => HdrMethod::HighlightCompression,
        "equalize_histogram" => HdrMethod::EqualizeHistogram,
        "local_adaptation" => {
            return Ok(HdrRequest::Local(Box::new(LocalAdaptation::parse(
                object,
            )?)))
        }
        other => {
            return Err(format!(
                "hdr toning method \"{other}\" is not one of equalize_histogram, exposure_gamma, highlight_compression or local_adaptation"
            ))
        }
    };
    for key in object.keys() {
        let admitted = key == "method"
            || (matches!(method, HdrMethod::ExposureGamma(_))
                && (key == "exposure" || key == "gamma"));
        if !admitted {
            return Err(format!(
                "\"{key}\" is not an argument of hdr toning's {} method",
                method_name(&method)
            ));
        }
    }
    Ok(HdrRequest::Global(method))
}

/// Exposure and Gamma's two values through `Exposure::parse`, each checked
/// alone first so a refusal names the one out of range.
fn exposure(object: &Map<String, Value>) -> Result<Exposure, String> {
    let alone = |key: &str| {
        let mut map = Map::new();
        if let Some(value) = object.get(key) {
            map.insert(key.into(), value.clone());
        }
        Exposure::parse(&map)
    };
    alone("exposure").ok_or("hdr toning exposure must be a number in -20…20 stops")?;
    alone("gamma").ok_or("hdr toning gamma must be a number in 0.01…9.99")?;
    let mut map = Map::new();
    for key in ["exposure", "gamma"] {
        if let Some(value) = object.get(key) {
            map.insert(key.into(), value.clone());
        }
    }
    Exposure::parse(&map).ok_or_else(|| "hdr toning exposure or gamma is out of range".into())
}

/// The document profile's forward curve, as the caller builds it: a LINEAR
/// triple clipped to [0, 1] and encoded (`FloatPolicy::encode_sdr` under the
/// clip map). The kernel holds no curve code of its own.
pub(crate) trait DisplayCurve {
    fn encode(&self, rgb: [f32; 3]) -> [f32; 3];
}

/// What a method reads of the whole plane before it maps a pixel.
pub(crate) struct PlaneStats {
    /// The largest RGB sample of a pixel with alpha above 0; 0 for none.
    max: f32,
    /// Equalize Histogram's log range and table; `None` with fewer than two
    /// occupied levels, and for the other methods.
    equalize: Option<Equalized>,
}

struct Equalized {
    low: f32,
    span: f32,
    lut: Vec<u16>,
}

impl Equalized {
    fn level(&self, value: f32) -> usize {
        let log = value.max(LOG_FLOOR).log2();
        let t = ((log - self.low) / self.span).clamp(0.0, 1.0);
        (t * (LEVELS - 1) as f32).round() as usize
    }
}

/// The statistics `method` reads (module doc), from fresh passes over the
/// pixels: none for Exposure and Gamma, the largest RGB sample for Highlight
/// Compression, the log range and then the weighted log histogram for
/// Equalize Histogram. A sample with alpha 0 never counts.
pub(crate) fn analyse<I: Iterator<Item = [f32; 4]>>(
    method: &HdrMethod,
    pixels: impl Fn() -> I,
) -> PlaneStats {
    let mut stats = PlaneStats {
        max: 0.0,
        equalize: None,
    };
    match method {
        HdrMethod::ExposureGamma(_) => {}
        HdrMethod::HighlightCompression => {
            for pixel in pixels().filter(|p| p[3] > 0.0) {
                stats.max = stats.max.max(pixel[0]).max(pixel[1]).max(pixel[2]);
            }
        }
        HdrMethod::EqualizeHistogram => {
            let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
            for pixel in pixels().filter(|p| p[3] > 0.0) {
                for v in &pixel[..3] {
                    let log = v.max(LOG_FLOOR).log2();
                    low = low.min(log);
                    high = high.max(log);
                }
            }
            if high > low {
                let Ok(mut histogram) = Histogram::new(LEVELS) else {
                    return stats;
                };
                let mut table = Equalized {
                    low,
                    span: high - low,
                    lut: Vec::new(),
                };
                for pixel in pixels().filter(|p| p[3] > 0.0) {
                    for v in &pixel[..3] {
                        histogram.add(table.level(*v), f64::from(pixel[3]));
                    }
                }
                if let Some(lut) = histogram.lut() {
                    table.lut = lut;
                    stats.equalize = Some(table);
                }
            }
        }
    }
    stats
}

/// One LINEAR triple through `method` as an ENCODED triple in [0, 1]
/// (module doc, "The maps").
pub(crate) fn map_pixel(
    method: &HdrMethod,
    stats: &PlaneStats,
    curve: &dyn DisplayCurve,
    linear: [f32; 3],
) -> [f32; 3] {
    match method {
        HdrMethod::ExposureGamma(exposure) => {
            let mapped = exposure.apply_linear(linear.map(f64::from));
            curve.encode(mapped.map(|v| (v as f32).clamp(0.0, 1.0)))
        }
        HdrMethod::HighlightCompression => {
            let white = stats.max.clamp(1.0, FLOAT_RANGE_MAX);
            curve.encode(ToneMap::Reinhard { white }.apply(linear))
        }
        HdrMethod::EqualizeHistogram => match &stats.equalize {
            Some(table) => linear.map(|v| f32::from(table.lut[table.level(v)]) / 65535.0),
            None => curve.encode(linear.map(|v| v.clamp(0.0, 1.0))),
        },
    }
}

/// The peak of the stages AFTER the render over a `width` × `height` canvas,
/// with the rendered plane counted as held. A float source holds its f32
/// render (16 B/px) while the u16 result is written contiguously (8 B/px) and
/// tiled (`adoption_copy_bytes`). A u16 source first decodes its u16 render
/// (8 B/px, held) into a contiguous f32 plane (16 B/px) and tiles it
/// (`float_adoption_copy_bytes`), the larger of its two stages. Then the
/// histogram, four bookkeeping units and the feature margin. Local
/// Adaptation's map stage is `hdr_local_adaptation::plan_bytes` (the linear
/// plane held beside its guided pass or its map, and its two histograms) in
/// place of the global map and Equalize Histogram's statistics.
pub(crate) fn plan_bytes(
    request: &HdrRequest,
    width: u32,
    height: u32,
    source_float: bool,
) -> Result<u64, String> {
    let overflow = || "hdr toning working-memory calculation overflow".to_string();
    let count = pixel_count(width, height)? as u64;
    let (map, statistics) = match request {
        HdrRequest::Global(_) => {
            let output = count
                .checked_mul(8)
                .and_then(|bytes| bytes.checked_add(adoption_copy_bytes((width, height)).ok()?))
                .ok_or_else(overflow)?;
            let linear = count.checked_mul(16).ok_or_else(overflow)?;
            (
                linear.checked_add(output).ok_or_else(overflow)?,
                HISTOGRAM_BYTES,
            )
        }
        HdrRequest::Local(la) => (hdr_local_adaptation::plan_bytes(la, width, height)?, 0),
    };
    let stage = if source_float {
        map
    } else {
        let promote = count
            .checked_mul(8 + 16)
            .and_then(|bytes| bytes.checked_add(float_adoption_copy_bytes((width, height)).ok()?))
            .ok_or_else(overflow)?;
        promote.max(map)
    };
    count
        .checked_mul(HDR_TONING_MARGIN_BYTES_PER_PIXEL)
        .and_then(|margin| margin.checked_add(stage))
        .and_then(|bytes| bytes.checked_add(statistics))
        .and_then(|bytes| bytes.checked_add(4 * BOOKKEEPING_BYTES))
        .ok_or_else(overflow)
}
