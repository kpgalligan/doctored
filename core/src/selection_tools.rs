//! Profile-aware Color Range and seeded Quick Selection. Both consume an
//! immutable captured projection; neither flattens the document a second time.
//! Color distances are CIE76 in the document's D50 Lab, sharing `lab` and ICC
//! conversion with the Info panel. LUT profiles are refused, never assumed sRGB.
//!
//! Quick Selection is bounded four-connected region growth: an accepted edge
//! must satisfy BOTH local ΔE and distance from the originating brush seed.
//! The seed anchor stops a shallow gradient walking indefinitely; the local
//! test stops a hard edge even when both sides fit the wider seed tolerance.
//! Multiple seeds compete in input order (deterministic breadth first claims).
//! It is color-guided selection, not a semantic subject or matting model.
//! On a Lab Color document (its profile the Lab twin, #547 phase 6b-ii) the
//! ΔE76 reads the codes' own L\*a\*b\*; any other profile with no matrix/TRC
//! model refuses.
//! The growth, the brush disc seeding (`radius`), the range-gated
//! Lab pre-smoothing (`smooth`) and the enclosed-hole fill are the
//! `selection_quick` kernel; this module parses the parameters, converts the
//! captured projection to Lab and reserves the scratch.
//!
//! Borrowed captured projections avoid implicit flattening; single-use job controls bound
//! scratch/cancellation and caller output publishes only on success.
//!
//! **Color Range's strict JSON** (`dr_selection_color_range` and the native
//! `{"kind":"color_range","params":…}`; keys `mode, samples, tolerance,
//! softness, lower, upper, band, invert, points, cluster_range, fuzziness`):
//!
//! * `mode` `sampled`: `samples` (1…32 RGB code triples), `tolerance` (ΔE76
//!   0…200, default 20) and `softness` (0…100, default 10, the linear shoulder
//!   past the tolerance); a pixel takes its nearest sample's ramp. With
//!   `points` (one integer canvas `[x, y]` per sample, in order) it is
//!   Localized Color Clusters: coverage is the maximum over samples of the
//!   sample's ramp times `color_range_band::cluster_weight` of the pixel's
//!   distance to that sample's point, at `cluster_range` (0…100, default 100,
//!   only with `points`) percent of the canvas's longer side. On a Lab Color
//!   document (its profile the Lab twin, #547 phase 4b-ii) the samples are
//!   its L\*a\*b\* codes and the distance is `lab_select::RANGE_RULE`'s, the
//!   largest per-channel difference in L\*a\*b\* units (measured). Since
//!   phase 6b-i every other mode runs there too: `tonal` reads the codes'
//!   own L\*, and `hue` and `skin` read the codes' 16-bit ProPhoto RGB
//!   encoding (the twin's relative pipeline, Hue/Saturation's space), skin's
//!   Lab the codes' own; the profile's `lab` is never asked of the twin. That
//!   pipeline is built only when the selection runs (`color_range_samples`),
//!   never by the parameters' validation (6b-ii).
//! * `mode` `tonal`: Highlights / Midtones / Shadows / Custom Lightness, the
//!   Lab L* window `lower`…`upper` (0…100) with `softness` shoulders.
//! * `mode` `hue`: Reds … Magentas, `band` one of
//!   `adjust_math::HUE_WHEEL`, coverage `color_range_band::band_coverage` of
//!   the pixel's encoded codes; it takes only `band` and `invert`.
//! * `mode` `skin`: Skin Tones, coverage
//!   `color_range_band::skin_coverage` of the pixel's encoded codes and its
//!   document-profile Lab at `fuzziness` (0…200, default 40); it takes only
//!   `fuzziness` and `invert`, and `fuzziness` in any other mode is refused.
//! * `invert` (every mode, default false): `1 − coverage`, applied before the
//!   source alpha scales it, so a transparent pixel stays outside.

use crate::color_range_band::ramp;
use crate::selection_quick::QuickParameters;
use crate::{doc::DrDocument, job_control::JobContext, DrImage};
use serde_json::{Map, Value};

pub(crate) fn object(json: &str, allowed: &[&str]) -> Result<Map<String, Value>, String> {
    if json.len() > 128 * 1024 {
        return Err("selection parameters exceed 128 KiB".into());
    }
    let value: Value =
        serde_json::from_str(json).map_err(|e| format!("invalid selection parameters: {e}"))?;
    let Value::Object(map) = value else {
        return Err("selection parameters must be an object".into());
    };
    if let Some(key) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("unknown selection parameter: {key}"));
    }
    Ok(map)
}

pub(crate) fn number(
    map: &Map<String, Value>,
    key: &str,
    default: f32,
    low: f32,
    high: f32,
) -> Result<f32, String> {
    let value = match map.get(key) {
        None => return Ok(default),
        Some(v) => v
            .as_f64()
            .ok_or_else(|| format!("{key} must be a number"))? as f32,
    };
    if !value.is_finite() || value < low || value > high {
        return Err(format!("{key} must be between {low} and {high}"));
    }
    Ok(value)
}

fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

fn lab<T: SelectionSample>(profile: &crate::icc::IccProfile, rgb: &[T]) -> [f32; 3] {
    // Caller establishes a supported matrix/TRC model once before pixel work.
    let model = profile.model().expect("validated profile");
    crate::lab::xyz_to_lab(
        model.to_pcs_xyz([rgb[0], rgb[1], rgb[2]].map(|v| v.code() / T::MAX as f32)),
    )
}

pub(crate) fn validate_source(doc: &DrDocument, image: &DrImage) -> Result<(), String> {
    if image.pixels.dimensions() != (doc.width, doc.height) {
        return Err("selection source dimensions do not match the document".into());
    }
    if doc.profile.model().is_none() {
        return Err("Color Range and Quick Selection require a matrix/TRC RGB profile; convert the document profile first".into());
    }
    Ok(())
}

pub(crate) fn color_range(
    doc: &DrDocument,
    image: &DrImage,
    json: &str,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    validate_source(doc, image)?;
    color_range_samples(
        &doc.profile,
        image.pixels.dimensions(),
        |i| {
            let p = &image.pixels.as_raw()[i * 4..i * 4 + 4];
            [p[0], p[1], p[2], p[3]]
        },
        json,
        job,
    )
}

pub(crate) fn color_range_samples<T: SelectionSample>(
    profile: &crate::icc::IccProfile,
    dimensions: (u32, u32),
    pixel_at: impl Fn(usize) -> [T; 4],
    json: &str,
    job: JobContext<'_>,
) -> Result<Vec<T>, String> {
    let ColorRangeParameters {
        tonal,
        band,
        skin,
        softness,
        tolerance,
        lower,
        upper,
        samples,
        invert,
        points,
        radius,
        lab: lab_rule,
    } = color_range_parameters::<T>(profile, dimensions, json)?;
    // #547 phase 6b-i: on a Lab document Hues and Skin Tones read the codes'
    // 16-bit ProPhoto RGB encoding, Hue/Saturation's space
    // (`lab_adjust::color_space`), through the twin's relative pipeline. Built
    // here, after the parameters validate, so a plan builds none (6b-ii).
    let lab_rgb = if lab_rule.is_some() && (band.is_some() || skin.is_some()) {
        crate::retained_source_render::lut_pipeline(
            &crate::lab_document::lab_twin(),
            &crate::icc::IccProfile::prophoto_rgb(),
        )?
    } else {
        None
    };
    let n = dimensions.0 as usize * dimensions.1 as usize;
    let width = dimensions.0 as usize;
    let pipeline_scratch = lab_rgb.as_ref().map_or(0, |p| p.scratch_bytes());
    job.reserve(n as u64 * std::mem::size_of::<T>() as u64 + 256 * 1024 + pipeline_scratch)?;
    // The pixel's encoded RGB units: its own codes, or on a Lab document the
    // codes' ProPhoto RGB (phase 6b-i).
    let encoded = |pixel: &[T; 4]| {
        let unit = [pixel[0], pixel[1], pixel[2]].map(|v| v.code() / T::MAX as f32);
        match &lab_rgb {
            Some(pipeline) => pipeline.color(unit).map(|v| v.clamp(0.0, 1.0)),
            None => unit,
        }
    };
    let mut out = vec![T::zero(); n];
    for (i, output) in out.iter_mut().enumerate() {
        let pixel = pixel_at(i);
        job.every(i)?;
        if i & 65535 == 0 {
            job.report(i as f64 / n as f64, 6)?;
        }
        if pixel[3].code() == 0. {
            continue;
        }
        let coverage = if let Some(band) = band {
            crate::color_range_band::band_coverage(band, encoded(&pixel))
        } else if let Some(fuzziness) = skin {
            let color = match lab_rule {
                Some(_) => lab_units(&pixel),
                None => lab(profile, &pixel),
            };
            crate::color_range_band::skin_coverage(encoded(&pixel), color, fuzziness)
        } else if tonal {
            // A Lab document's L* is its codes' own (phase 6b-i).
            let color = match lab_rule {
                Some(_) => lab_units(&pixel),
                None => lab(profile, &pixel),
            };
            ramp(
                (lower - color[0]).max(color[0] - upper).max(0.0),
                0.0,
                softness,
            )
        } else if let (Some(rule), true) = (lab_rule, points.is_empty()) {
            // #547 phase 4b-ii: a Lab document's codes by its measured rule.
            let color = lab_units(&pixel);
            let d = samples
                .iter()
                .map(|&s| crate::lab_select::distance(rule, color, s))
                .fold(f32::INFINITY, f32::min);
            ramp(d, tolerance, softness)
        } else if points.is_empty() {
            let color = lab(profile, &pixel);
            let d = samples
                .iter()
                .map(|&s| distance_squared(color, s))
                .fold(f32::INFINITY, f32::min)
                .sqrt();
            ramp(d, tolerance, softness)
        } else {
            let color = match lab_rule {
                Some(_) => lab_units(&pixel),
                None => lab(profile, &pixel),
            };
            let distance = |s: [f32; 3]| match lab_rule {
                Some(rule) => crate::lab_select::distance(rule, color, s),
                None => distance_squared(color, s).sqrt(),
            };
            let (x, y) = ((i % width) as f32, (i / width) as f32);
            samples
                .iter()
                .zip(&points)
                .map(|(&s, &(px, py))| {
                    let (dx, dy) = (x - px as f32, y - py as f32);
                    let weight =
                        crate::color_range_band::cluster_weight((dx * dx + dy * dy).sqrt(), radius);
                    if weight == 0.0 {
                        0.0
                    } else {
                        ramp(distance(s), tolerance, softness) * weight
                    }
                })
                .fold(0.0, f32::max)
        };
        let coverage = if invert { 1.0 - coverage } else { coverage };
        *output = T::quantize(coverage * pixel[3].code());
    }
    job.report(1.0, 5)?;
    Ok(out)
}

pub(crate) struct ColorRangeParameters {
    tonal: bool,
    /// `mode` `hue`'s `adjust_math::HUE_WHEEL` slot.
    band: Option<usize>,
    /// `mode` `skin`'s Fuzziness (0…200).
    skin: Option<f32>,
    softness: f32,
    tolerance: f32,
    lower: f32,
    upper: f32,
    samples: Vec<[f32; 3]>,
    invert: bool,
    /// Localized Color Clusters: one canvas pixel per sample, or empty.
    points: Vec<(u32, u32)>,
    /// `cluster_range` as `color_range_band::cluster_radius` pixels.
    radius: f32,
    /// A Lab Color document's rule (#547 phase 4b-ii, `lab_select`): the
    /// samples are its L\*a\*b\* codes as units and the distance is the
    /// rule's; `None` everywhere else.
    lab: Option<crate::lab_select::LabRule>,
}

/// A sample's L\*a\*b\* units, from its codes widened to u16 Lab codes (a
/// precision document's already are).
fn lab_units<T: SelectionSample>(p: &[T]) -> [f32; 3] {
    let scale = 65535.0 / T::MAX as f32;
    let wide = |v: T| (v.code() * scale).round() as u16;
    crate::lab_select::units([wide(p[0]), wide(p[1]), wide(p[2]), 0])
}

/// Parses and validates Color Range's strict JSON (module doc) against a
/// `dimensions` canvas, before any pixel work.
pub(crate) fn color_range_parameters<T: SelectionSample>(
    profile: &crate::icc::IccProfile,
    dimensions: (u32, u32),
    json: &str,
) -> Result<ColorRangeParameters, String> {
    // #547 phase 4b-ii: a Lab document (its profile IS the Lab twin) runs
    // sampled Color Range by `lab_select`'s measured rule on its codes.
    let lab_rule =
        crate::lab_document::is_lab_twin(profile).then_some(crate::lab_select::RANGE_RULE);
    if profile.model().is_none() && lab_rule.is_none() {
        return Err("selection source requires a matrix/TRC RGB profile".into());
    }
    let p = object(
        json,
        &[
            "mode",
            "samples",
            "tolerance",
            "softness",
            "lower",
            "upper",
            "band",
            "invert",
            "points",
            "cluster_range",
            "fuzziness",
        ],
    )?;
    // An unknown or missing mode refuses before any mode-specific key check,
    // so a stray `fuzziness` or `band` never names a mode the request lacks.
    let mode = p
        .get("mode")
        .and_then(Value::as_str)
        .filter(|mode| ["sampled", "tonal", "hue", "skin"].contains(mode))
        .ok_or("Color Range mode must be sampled, tonal, hue or skin")?;
    let invert = match p.get("invert") {
        None => false,
        Some(v) => v.as_bool().ok_or("invert must be a boolean")?,
    };
    if mode == "skin" {
        if p.keys()
            .any(|key| !["mode", "fuzziness", "invert"].contains(&key.as_str()))
        {
            return Err("skin Color Range accepts only fuzziness and invert".into());
        }
        let fuzziness = number(
            &p,
            "fuzziness",
            crate::color_range_band::SKIN_DEFAULT_FUZZINESS,
            0.0,
            200.0,
        )?;
        return Ok(ColorRangeParameters {
            tonal: false,
            band: None,
            skin: Some(fuzziness),
            softness: 0.0,
            tolerance: 0.0,
            lower: 0.0,
            upper: 0.0,
            samples: Vec::new(),
            invert,
            points: Vec::new(),
            radius: 0.0,
            lab: lab_rule,
        });
    }
    // Checked before the hue branch's own key check, so the sentence names
    // the one mode that takes it.
    if p.contains_key("fuzziness") {
        return Err("Color Range fuzziness needs mode skin".into());
    }
    if mode == "hue" {
        if p.keys()
            .any(|key| !["mode", "band", "invert"].contains(&key.as_str()))
        {
            return Err("hue Color Range accepts only band and invert".into());
        }
        let band = p
            .get("band")
            .and_then(Value::as_str)
            .and_then(crate::color_range_band::band_index)
            .ok_or("Color Range band must be reds, yellows, greens, cyans, blues or magentas")?;
        return Ok(ColorRangeParameters {
            tonal: false,
            band: Some(band),
            skin: None,
            softness: 0.0,
            tolerance: 0.0,
            lower: 0.0,
            upper: 0.0,
            samples: Vec::new(),
            invert,
            points: Vec::new(),
            radius: 0.0,
            lab: lab_rule,
        });
    }
    if p.contains_key("band") {
        return Err("Color Range band needs mode hue".into());
    }
    let softness = number(&p, "softness", 10.0, 0.0, 100.0)?;
    let tolerance = number(&p, "tolerance", 20.0, 0.0, 200.0)?;
    let lower = number(&p, "lower", 0.0, 0.0, 100.0)?;
    let upper = number(&p, "upper", 100.0, 0.0, 100.0)?;
    if lower > upper {
        return Err("tonal lower bound exceeds upper bound".into());
    }
    let cluster_range = number(&p, "cluster_range", 100.0, 0.0, 100.0)?;
    let mut samples = Vec::new();
    let mut points = Vec::new();
    match mode {
        "sampled" => {
            if p.contains_key("lower") || p.contains_key("upper") {
                return Err("sampled Color Range does not accept tonal bounds".into());
            }
            let values = p
                .get("samples")
                .and_then(Value::as_array)
                .ok_or("sampled Color Range needs RGB samples")?;
            if values.is_empty() || values.len() > 32 {
                return Err("Color Range needs 1 to 32 RGB samples".into());
            }
            for value in values {
                let rgb = value
                    .as_array()
                    .filter(|a| a.len() == 3)
                    .ok_or("Color Range samples must contain three RGB bytes")?;
                let mut bytes = [T::zero(); 3];
                for (out, v) in bytes.iter_mut().zip(rgb) {
                    *out = T::quantize(
                        v.as_u64()
                            .filter(|v| *v <= T::MAX)
                            .ok_or("Color Range samples exceed source code range")?
                            as f32,
                    );
                }
                samples.push(match lab_rule {
                    Some(_) => lab_units(&bytes),
                    None => lab(profile, &bytes),
                });
            }
            if let Some(values) = p.get("points") {
                points = cluster_points(values, samples.len(), dimensions)?;
            }
        }
        "tonal" => {
            if p.contains_key("samples") || p.contains_key("tolerance") {
                return Err("tonal Color Range does not accept samples or tolerance".into());
            }
            if p.contains_key("points") {
                return Err("Color Range points need mode sampled".into());
            }
        }
        _ => return Err("Color Range mode must be sampled, tonal, hue or skin".into()),
    }
    if p.contains_key("cluster_range") && points.is_empty() {
        return Err("cluster_range needs points".into());
    }
    Ok(ColorRangeParameters {
        tonal: mode == "tonal",
        band: None,
        skin: None,
        softness,
        tolerance,
        lower,
        upper,
        samples,
        invert,
        radius: crate::color_range_band::cluster_radius(cluster_range, dimensions),
        points,
        lab: lab_rule,
    })
}

/// Localized Color Clusters' `points`: exactly `count` integer `[x, y]`
/// canvas pixels, paired in order with the samples.
fn cluster_points(
    values: &Value,
    count: usize,
    (w, h): (u32, u32),
) -> Result<Vec<(u32, u32)>, String> {
    const PAIRING: &str = "Color Range points must pair one canvas pixel with each sample";
    let values = values
        .as_array()
        .filter(|a| a.len() == count)
        .ok_or(PAIRING)?;
    let mut points = Vec::with_capacity(count);
    for value in values {
        let pair = value
            .as_array()
            .filter(|a| a.len() == 2 && a.iter().all(|v| v.as_i64().is_some()))
            .ok_or(PAIRING)?;
        let (x, y) = (
            pair[0].as_i64().unwrap_or(-1),
            pair[1].as_i64().unwrap_or(-1),
        );
        if x < 0 || y < 0 || x >= i64::from(w) || y >= i64::from(h) {
            return Err("Color Range point lies outside the canvas".into());
        }
        points.push((x as u32, y as u32));
    }
    Ok(points)
}

pub(crate) fn quick(
    doc: &DrDocument,
    image: &DrImage,
    json: &str,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    validate_source(doc, image)?;
    quick_samples(
        &doc.profile,
        image.pixels.dimensions(),
        |i| {
            let p = &image.pixels.as_raw()[i * 4..i * 4 + 4];
            [p[0], p[1], p[2], p[3]]
        },
        json,
        job,
    )
}

pub(crate) fn quick_samples<T: SelectionSample>(
    profile: &crate::icc::IccProfile,
    dimensions: (u32, u32),
    pixel_at: impl Fn(usize) -> [T; 4],
    json: &str,
    job: JobContext<'_>,
) -> Result<Vec<T>, String> {
    // #547 phase 6b-ii: a Lab document's ΔE76 reads its codes' own L*a*b*.
    let lab_codes = crate::lab_document::is_lab_twin(profile);
    if profile.model().is_none() && !lab_codes {
        return Err("selection source requires a matrix/TRC RGB profile".into());
    }
    let parameters = quick_parameters(dimensions, json)?;
    let n = dimensions.0 as usize * dimensions.1 as usize;
    // The exact peak of the two stages (`selection_quick` module doc, MEMORY),
    // plus a megabyte for the seed vectors and allocator padding. Growth holds
    // the Lab plane (12n), the exact-capacity queue (8n) and the output; the
    // hole-fill bitset comes after the Lab plane is dropped. Pre-smoothing
    // comes before the queue and the output exist and holds the Lab plane
    // beside its ring of `min(smooth + 1, h)` saved rows (12 bytes a sample),
    // which on a wide, short canvas is the larger of the two.
    let (width, height) = (u64::from(dimensions.0), u64::from(dimensions.1));
    let ring = if parameters.smooth > 0 {
        12 * width * (u64::from(parameters.smooth) + 1).min(height)
    } else {
        0
    };
    let growth = n as u64 * (20 + std::mem::size_of::<T>() as u64);
    job.reserve(growth.max(12 * n as u64 + ring) + 1024 * 1024)?;
    let mut colors = Vec::with_capacity(n);
    for i in 0..n {
        let pixel = pixel_at(i);
        job.every(i)?;
        colors.push(if lab_codes {
            lab_units(&pixel)
        } else {
            lab(profile, &pixel)
        });
    }
    job.report(0.45, 6)?;
    crate::selection_quick::grow(colors, dimensions, |i| pixel_at(i)[3], &parameters, job)
}

/// Quick Selection's strict JSON: `seeds` (1..4096 integer `[x, y]` inside the
/// canvas), `tolerance` and `edge` (ΔE76, 0..200, returned squared), `radius`
/// (the brush radius in canvas pixels, 0..256, default 0 = the seed pixel
/// alone) and `smooth` (an integer 0..4, default 0: the pre-smoothing reach).
/// Refuses a brush whose summed disc boxes pass
/// `selection_quick::MAX_DISC_VISITS`, before any pixel work.
pub(crate) fn quick_parameters(
    dimensions: (u32, u32),
    json: &str,
) -> Result<QuickParameters, String> {
    let p = object(json, &["seeds", "tolerance", "edge", "radius", "smooth"])?;
    let tolerance = number(&p, "tolerance", 25.0, 0.0, 200.0)?.powi(2);
    let edge = number(&p, "edge", 12.0, 0.0, 200.0)?.powi(2);
    let radius = number(&p, "radius", 0.0, 0.0, 256.0)?;
    let smooth = match p.get("smooth") {
        None => 0,
        Some(v) => v
            .as_u64()
            .filter(|v| *v <= 4)
            .ok_or("smooth must be an integer between 0 and 4")? as u32,
    };
    let values = p
        .get("seeds")
        .and_then(Value::as_array)
        .ok_or("Quick Selection needs seed coordinates")?;
    if values.is_empty() || values.len() > 4096 {
        return Err("Quick Selection needs 1 to 4096 seed coordinates".into());
    }
    let (w, h) = dimensions;
    let mut seeds = Vec::with_capacity(values.len());
    for value in values {
        let pair = value
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("Quick Selection seeds must be [x,y]")?;
        let x = pair[0]
            .as_u64()
            .filter(|x| *x < u64::from(w))
            .ok_or("Quick Selection seed x is outside the canvas")? as usize;
        let y = pair[1]
            .as_u64()
            .filter(|y| *y < u64::from(h))
            .ok_or("Quick Selection seed y is outside the canvas")? as usize;
        seeds.push(y * w as usize + x);
    }
    crate::selection_quick::disc_visits(dimensions, &seeds, radius)?;
    Ok(QuickParameters {
        tolerance,
        edge,
        radius,
        smooth,
        seeds,
    })
}

/// Storage adapters preserve the legacy byte rounding and lift the SAME Lab
/// distance/ramp/growth algorithm to authoritative native source codes.
pub(crate) trait SelectionSample: Copy {
    const MAX: u64;
    fn zero() -> Self;
    fn code(self) -> f32;
    fn quantize(value: f32) -> Self;
}
impl SelectionSample for u8 {
    const MAX: u64 = 255;
    fn zero() -> Self {
        0
    }
    fn code(self) -> f32 {
        f32::from(self)
    }
    fn quantize(value: f32) -> Self {
        value.round() as u8
    }
}
impl SelectionSample for u16 {
    const MAX: u64 = 65535;
    fn zero() -> Self {
        0
    }
    fn code(self) -> f32 {
        f32::from(self)
    }
    fn quantize(value: f32) -> Self {
        value.round() as u16
    }
}
