//! The request half of Filters ▸ Develop Filter's masks: one
//! `masks` entry parsed into a [`FilterLocal`], strict about its keys, with
//! the shapes, brush strokes, range and controls validated in the words of
//! the kernels they reach. `develop_filter_masks` owns the limits, the plan and
//! the evaluation; the mappings (0…1 coordinates, feather, range sample) are
//! described in its module doc.
//!
//! A mask's adjustment keys are [`LOCAL_KEYS`] (the global controls but
//! Vibrance), read through `SelectedFilter::parse` so their ranges and
//! sentences are the global controls'. `sharpness` (−100…100) and
//! `noise_reduction` (0…100) are the local detail ([`LocalDetail`], the
//! `local_detail` module), absent keys 0.
use crate::develop_filter_masks::MAX_MAP_SAMPLES;
use crate::local_detail::LocalDetail;
use crate::ops_spatial::SpatialKernel;
use crate::raw_local_coverage::{BrushMode, Dab};
use crate::raw_local_range::{ColorRange, LocalRange, LuminanceRange};
use crate::sampling_develop_filter::DevelopFilter;
use crate::selected_filter::SelectedFilter;
use serde::Deserialize;
use serde_json::{Map, Value};

/// The local adjustment keys: the global ones but Vibrance.
const LOCAL_KEYS: [&str; 12] = [
    "temperature",
    "tint",
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "texture",
    "clarity",
    "dehaze",
    "saturation",
];
/// The mask object's own keys beside [`LOCAL_KEYS`]: the coverage recipe and
/// the local detail.
const MASK_KEYS: [&str; 8] = [
    "shape",
    "brush",
    "invert",
    "density",
    "feather",
    "range",
    "sharpness",
    "noise_reduction",
];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Generated {
    Subject,
    People,
    Sky,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum ShapeSpec {
    // Empty struct variants keep deny_unknown_fields (`raw_local_coverage`).
    Full {},
    Empty {},
    Radial {
        center: [f64; 2],
        radius: [f64; 2],
        hardness: f64,
    },
    Linear {
        from: [f64; 2],
        to: [f64; 2],
    },
    Subject {},
    People {},
    Sky {},
    Map {
        // Which model made the map: validated, not read (the samples are
        // the coverage whatever made them).
        #[serde(rename = "generated")]
        _generated: Generated,
        width: u32,
        height: u32,
        samples: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrokeSpec {
    mode: BrushMode,
    dabs: Vec<Dab>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColorSpec {
    #[serde(default)]
    lab: Option<[f64; 3]>,
    #[serde(default)]
    sample: Option<[f64; 2]>,
    amount: f64,
    falloff: f64,
    invert: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeSpec {
    #[serde(default)]
    luminance: Option<LuminanceRange>,
    #[serde(default)]
    color: Option<ColorSpec>,
}

/// A mask's shape, in 0…1 target coordinates until planned.
pub(crate) enum Shape {
    Full,
    Empty,
    Radial {
        center: [f64; 2],
        radius: [f64; 2],
        hardness: f64,
    },
    Linear {
        from: [f64; 2],
        to: [f64; 2],
    },
    /// Resolved by the app before the filter runs.
    Unresolved,
    /// A host-made coverage map, `width × height` bytes.
    Map {
        width: u32,
        height: u32,
        samples: Vec<u8>,
    },
}

/// A colour range's colour: as given, or the base's mean about a point.
pub(crate) enum RangeColor {
    Lab([f64; 3]),
    Sample([f64; 2]),
}

pub(crate) struct Range {
    pub(crate) luminance: Option<LuminanceRange>,
    pub(crate) color: Option<(RangeColor, f64, f64, bool)>,
}

impl Range {
    /// The `LocalRange` with `lab` standing for a sampled colour.
    pub(crate) fn resolved(&self, lab: [f64; 3]) -> LocalRange {
        LocalRange {
            luminance: self.luminance,
            color: self
                .color
                .as_ref()
                .map(|(colour, amount, falloff, invert)| ColorRange {
                    lab: match colour {
                        RangeColor::Lab(lab) => *lab,
                        RangeColor::Sample(_) => lab,
                    },
                    amount: *amount,
                    falloff: *falloff,
                    invert: *invert,
                }),
        }
    }
}

/// One mask: its coverage recipe, density, feather, range and value.
pub(crate) struct FilterLocal {
    pub(crate) shape: Shape,
    /// Each stroke's mode and dabs in 0…1 coordinates (radius: a share of
    /// the longer side).
    pub(crate) strokes: Vec<(bool, Vec<Dab>)>,
    pub(crate) invert: bool,
    pub(crate) density: f64,
    /// 0…100.
    pub(crate) feather: f64,
    pub(crate) range: Option<Range>,
    pub(crate) adjust: DevelopFilter,
    /// Sharpness and Noise Reduction, run over the adjusted value.
    pub(crate) detail: LocalDetail,
}

/// `p` as a `develop_filter` parameter map through `SelectedFilter::parse`, so
/// the keys' ranges and sentences are the global controls'.
pub(crate) fn controls(p: Map<String, Value>) -> Result<DevelopFilter, String> {
    match SelectedFilter::parse("develop_filter", &p)? {
        SelectedFilter::Spatial(SpatialKernel::DevelopFilter(develop_filter)) => Ok(develop_filter),
        _ => Err("develop_filter masks could not read their controls".into()),
    }
}

fn unit(v: f64) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}

pub(crate) fn parse_local(index: usize, object: &Value) -> Result<FilterLocal, String> {
    let named = |e: String| format!("develop_filter mask {index}: {e}");
    let object = object
        .as_object()
        .ok_or_else(|| named("a mask must be an object".into()))?;
    if let Some(key) = object
        .keys()
        .find(|k| !MASK_KEYS.contains(&k.as_str()) && !LOCAL_KEYS.contains(&k.as_str()))
    {
        return Err(named(format!("unexpected mask parameter {key}")));
    }
    let typed = |key: &str| -> Option<Value> { object.get(key).cloned() };
    let shape: ShapeSpec =
        serde_json::from_value(typed("shape").ok_or_else(|| named("shape is required".into()))?)
            .map_err(|e| named(format!("invalid shape: {e}")))?;
    let invalid_shape = || named("shape coordinates, radius or hardness are invalid".into());
    let point = |p: [f64; 2]| unit(p[0]) && unit(p[1]);
    let shape = match shape {
        ShapeSpec::Full {} => Shape::Full,
        ShapeSpec::Empty {} => Shape::Empty,
        ShapeSpec::Radial {
            center,
            radius,
            hardness,
        } => {
            if !point(center)
                || !radius
                    .iter()
                    .all(|r| r.is_finite() && *r > 0.0 && *r <= 2.0)
                || !unit(hardness)
            {
                return Err(invalid_shape());
            }
            Shape::Radial {
                center,
                radius,
                hardness,
            }
        }
        ShapeSpec::Linear { from, to } => {
            if !point(from) || !point(to) || from == to {
                return Err(invalid_shape());
            }
            Shape::Linear { from, to }
        }
        ShapeSpec::Subject {} | ShapeSpec::People {} | ShapeSpec::Sky {} => Shape::Unresolved,
        ShapeSpec::Map {
            _generated,
            width,
            height,
            samples,
        } => {
            let count = u64::from(width) * u64::from(height);
            if count == 0 || count > MAX_MAP_SAMPLES {
                return Err(named(format!(
                    "a map has 1 to {MAX_MAP_SAMPLES} samples (1024 × 1024)"
                )));
            }
            let samples = crate::adjust_lut::base64_decode(&samples)
                .ok_or_else(|| named("map samples must be base64".into()))?;
            if samples.len() as u64 != count {
                return Err(named("map samples differ from its width and height".into()));
            }
            Shape::Map {
                width,
                height,
                samples,
            }
        }
    };
    let strokes: Vec<StrokeSpec> = match typed("brush") {
        None => Vec::new(),
        Some(v) => serde_json::from_value(v).map_err(|e| named(format!("invalid brush: {e}")))?,
    };
    let strokes = strokes
        .into_iter()
        .map(|s| (matches!(s.mode, BrushMode::Add), s.dabs))
        .collect();
    let flag = |key: &str| -> Result<bool, String> {
        object.get(key).map_or(Ok(false), |v| {
            v.as_bool()
                .ok_or_else(|| named(format!("{key} must be true or false")))
        })
    };
    let numeric = |key: &str, default: f64| -> Result<f64, String> {
        object.get(key).map_or(Ok(default), |v| {
            v.as_f64()
                .ok_or_else(|| named(format!("{key} must be numeric")))
        })
    };
    let number = |key: &str, default: f64, hi: f64| -> Result<f64, String> {
        let n = numeric(key, default)?;
        if !n.is_finite() || !(0.0..=hi).contains(&n) {
            return Err(named(format!("{key} must be finite in [0,{hi}]")));
        }
        Ok(n)
    };
    let range = match typed("range") {
        None => None,
        Some(v) => {
            let spec: RangeSpec =
                serde_json::from_value(v).map_err(|e| named(format!("invalid range: {e}")))?;
            let color = match spec.color {
                None => None,
                Some(c) => {
                    let colour = match (c.lab, c.sample) {
                        (Some(lab), None) => RangeColor::Lab(lab),
                        (None, Some(at)) if point(at) => RangeColor::Sample(at),
                        (None, Some(_)) => {
                            return Err(named("a range sample point is outside the target".into()))
                        }
                        _ => return Err(named("a colour range takes one of lab or sample".into())),
                    };
                    Some((colour, c.amount, c.falloff, c.invert))
                }
            };
            let range = Range {
                luminance: spec.luminance,
                color,
            };
            // A sampled colour validates at a neutral stand-in; its own mean
            // is a clamped L* and finite a*, b*, inside every bound.
            range.resolved([50.0, 0.0, 0.0]).validate().map_err(named)?;
            Some(range)
        }
    };
    let adjustments: Map<String, Value> = LOCAL_KEYS
        .iter()
        .filter_map(|k| object.get(*k).map(|v| ((*k).to_string(), v.clone())))
        .collect();
    let invert = flag("invert")?;
    let density = number("density", 1.0, 1.0)?;
    let feather = number("feather", 0.0, 100.0)?;
    let adjust = controls(adjustments).map_err(named)?;
    let detail = LocalDetail {
        sharpness: numeric("sharpness", 0.0)?,
        noise_reduction: numeric("noise_reduction", 0.0)?,
    };
    detail.validate().map_err(named)?;
    Ok(FilterLocal {
        shape,
        strokes,
        invert,
        density,
        feather,
        range,
        adjust,
        detail,
    })
}
