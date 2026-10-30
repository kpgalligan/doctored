//! KERNEL: the CMYK document's ink adjustments (#547) — the [`InkKernel`]
//! trait every ink op beyond Levels, Curves and Hue/Saturation evaluates
//! through, [`ink_tone`] (the ONE CMYK tone formula), and the five NATIVE ink
//! ops with their CMYK parameter parse: Channel Mixer, Selective Color,
//! Threshold, Gradient Map and Replace Color. The ops that bind an RGB kernel
//! to ink are `cmyk_adjust_rgb`'s `InkRgb`; the record and the destructive
//! apply are `cmyk_adjust_layer`'s; Photoshop's evidence is
//! `scripts/photoshop-probe/cmyk_adjust_filter_take.py` (takes 1 and 2).
//!
//! # The tone
//!
//! `ink_tone(ink) = luma709(1 − c, 1 − m, 1 − y) · (1 − k)`: the Rec. 709 luma
//! of the CMY plates' stored domain `L = 1 − ink`, darkened by the K plate's
//! `L`. A K-only or rich black reads dark, paper reads 1. Threshold, Gradient
//! Map and Shadows/Highlights' guide read it; nothing else computes a CMYK
//! tone.
//!
//! # The native ops
//!
//! * **Channel Mixer** (params `cyan`, `magenta`, `yellow`, `black`, `gray`,
//!   each `{c, m, y, k, constant}` in [−2, 2], identity rows by default and
//!   `gray` {0, 0, 0, 1, 0}; `monochrome`): every coefficient truncated to
//!   1/1024 as Photoshop keeps it (`adjust_mix::truncate_1024`, the RGB
//!   mixer's own rule) and `ink_o = Σ q(w_j)·ink_j + q(constant)` per output
//!   plate, on the INK — the member take1 FITTED and take2 confirmed (M-I:
//!   max 0 / 1 code against H0's 51 on `L`); Monochrome sends the gray row to
//!   all four outputs. An RGB row (`red`, `green`, `blue`) refuses by its key.
//! * **Selective Color** (the RGB schema): the nine memberships
//!   `SelectiveColor::weights` of the `L` triple `[L_c, L_m, L_y]`, the
//!   nudges applied to the stored ink by `SelectiveColor::nudged` — the RGB
//!   op runs both on its naive separation; here the separation is the ink
//!   itself, so K is the document's own.
//! * **Threshold** (`level` in [0, 1], default 0.5): `ink_tone ≥ level` is
//!   paper, else K 100 %, C/M/Y 0 (Photoshop's CMYK Threshold, the take's T-K).
//! * **Gradient Map** (`gradient.stops[]` of `{position, ink, opacity,
//!   midpoint, opacity_midpoint}`, 2…32 of them, `gradient.reverse`,
//!   `gradient.smoothness`, `method`; `dither` accepted and not applied):
//!   `t = ink_tone` (`1 − t` reversed), the bracketing stops found by
//!   `gradient_field::segment` (the ONE bracket search), and the pixel moved
//!   toward the mapped ink by the stop opacity — `adjust_map`'s rule (opacity
//!   blends the original with the mapped colour). With no `method` (legacy)
//!   the stop inks and opacities are lerped and the midpoints and smoothness
//!   are ignored; `method: "classic"` (`adjust_map`'s module doc) maps
//!   the four inks through `gradient_field::smooth_color::<4>` at the
//!   Smoothness (0…1, default 1) with the bracket bound and stop `i`'s
//!   `midpoint` (0.05…0.95, default 0.5) on segment `i..i + 1`, and the
//!   opacity linearly through stop `i`'s `opacity_midpoint`.
//!   `method: "perceptual"` refuses by name ("The gradient map's Perceptual
//!   method is unavailable in a CMYK document"): its Oklab law is an sRGB
//!   law and no CMYK row was taken. Any other method refuses. A stop
//!   `color` refuses: a CMYK stop is ink.
//! * **Replace Color** (`inks`, 1…8 quartets in [0, 1]; `fuzziness` [0, 200]
//!   default 40; `hue`, `saturation`, `lightness` as Hue/Saturation's master
//!   sliders): the weight is `adjust_replace_color::falloff` of the Euclidean
//!   distance over C, M, Y and K in 8-bit code units to the nearest ink; the
//!   edit is `HueSaturation`'s master on the `L` triple with K held, and the
//!   pixel moves `w` of the way to it. `colors` (RGB numbers) refuses.
//!   Replace Color applies to pixels only, so it refuses as an adjustment
//!   layer ([`InkKernel::layer_refusal`]).

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::adjust_hue_saturation::HueSaturation;
use crate::adjust_map::MapMethod;
use crate::adjust_math::luma;
use crate::adjust_mix::{truncate_1024, SelectiveColor};
use crate::adjust_parse::{boolean, num_in, num_in_f64};
use crate::adjust_replace_color::falloff;
use crate::cmyk_document::refused_sentence;
use crate::gradient_field::{
    midpoint_fraction, midpoint_valid, round_midpoint, segment, smooth_color, ClassicBound,
};

/// One ink op on a CMYK document: straight ink quadruples in [0, 1].
pub(crate) trait InkKernel: Send + Sync {
    /// The op name the inspection reports (`levels`, `channel_mixer`, …).
    fn operation(&self) -> &'static str;
    /// One straight ink quadruple (C, M, Y, K) in [0, 1].
    fn apply(&self, ink: [f32; 4]) -> [f32; 4];
    /// Whether the op changes nothing whatever the ink.
    fn is_identity(&self) -> bool;
    /// The heap bytes the parsed op holds.
    fn heap_bytes(&self) -> u64;
    /// The refusal of this op as an adjustment LAYER (it applies to pixels
    /// only); `None` when it may be a layer.
    fn layer_refusal(&self) -> Option<String> {
        None
    }
    /// The radius of the guide plane [`InkKernel::apply_guided`] reads
    /// (Shadows/Highlights); `None` for a pointwise op.
    fn guide_radius(&self) -> Option<f32> {
        None
    }
    /// [`InkKernel::apply`] with this pixel's value in the guide plane.
    fn apply_guided(&self, ink: [f32; 4], _guide: f32) -> [f32; 4] {
        self.apply(ink)
    }
}

/// The ONE CMYK tone formula (module doc).
pub(crate) fn ink_tone(ink: [f32; 4]) -> f32 {
    luma([1.0 - ink[0], 1.0 - ink[1], 1.0 - ink[2]]) * (1.0 - ink[3])
}

/// The refusal of an RGB key in a CMYK op's params.
fn rgb_key(key: &str) -> String {
    refused_sentence(&format!("The RGB channel key \"{key}\""))
}

fn unknown(op: &str, key: &str) -> String {
    format!("unknown CMYK {op} parameter {key}")
}

fn invalid(op: &str) -> String {
    format!("invalid CMYK {op} parameters")
}

/// Parses `op` when it is one of the native ink ops (module doc); `None`
/// for every other op.
pub(crate) fn parse_native(
    op: &str,
    params: &Map<String, Value>,
) -> Option<Result<Arc<dyn InkKernel>, String>> {
    let kernel: Result<Arc<dyn InkKernel>, String> = match op {
        "channel_mixer" => InkMixer::parse(params).map(|k| Arc::new(k) as Arc<dyn InkKernel>),
        "selective_color" => InkSelective::parse(params).map(|k| Arc::new(k) as _),
        "threshold" => InkThreshold::parse(params).map(|k| Arc::new(k) as _),
        "gradient_map" => InkGradientMap::parse(params).map(|k| Arc::new(k) as _),
        "replace_color" => InkReplaceColor::parse(params).map(|k| Arc::new(k) as _),
        _ => return None,
    };
    Some(kernel)
}

// ---------------------------------------------------------- channel mixer --

const MIXER_KEYS: [&str; 5] = ["c", "m", "y", "k", "constant"];
const MIXER_ROWS: [(&str, [f64; 5]); 4] = [
    ("cyan", [1.0, 0.0, 0.0, 0.0, 0.0]),
    ("magenta", [0.0, 1.0, 0.0, 0.0, 0.0]),
    ("yellow", [0.0, 0.0, 1.0, 0.0, 0.0]),
    ("black", [0.0, 0.0, 0.0, 1.0, 0.0]),
];
const MIXER_GRAY: [f64; 5] = [0.0, 0.0, 0.0, 1.0, 0.0];

/// Channel Mixer on ink (module doc): four output rows of four source
/// coefficients and a constant, each already truncated to 1/1024.
pub(crate) struct InkMixer {
    rows: [[f32; 5]; 4],
}

impl InkMixer {
    fn parse(params: &Map<String, Value>) -> Result<Self, String> {
        for (key, value) in params {
            match key.as_str() {
                "red" | "green" | "blue" => return Err(rgb_key(key)),
                "cyan" | "magenta" | "yellow" | "black" | "gray" => {
                    let row = value
                        .as_object()
                        .ok_or_else(|| format!("CMYK channel_mixer {key} must be an object"))?;
                    if let Some(k) = row.keys().find(|k| !MIXER_KEYS.contains(&k.as_str())) {
                        return Err(unknown("channel_mixer", k));
                    }
                }
                "monochrome" => {}
                _ => return Err(unknown("channel_mixer", key)),
            }
        }
        let row = |name: &str, defaults: [f64; 5]| {
            crate::adjust_parse::nested_f64(params, name, MIXER_KEYS, -2.0..=2.0, defaults)
                .ok_or_else(|| invalid("channel_mixer"))
        };
        let mut rows = [[0.0f64; 5]; 4];
        for (slot, (name, defaults)) in rows.iter_mut().zip(MIXER_ROWS) {
            *slot = row(name, defaults)?;
        }
        let gray = row("gray", MIXER_GRAY)?;
        if boolean(params, "monochrome", false).ok_or_else(|| invalid("channel_mixer"))? {
            rows = [gray; 4];
        }
        Ok(Self {
            rows: rows.map(|r| r.map(truncate_1024)),
        })
    }
}

impl InkKernel for InkMixer {
    fn operation(&self) -> &'static str {
        "channel_mixer"
    }

    fn apply(&self, ink: [f32; 4]) -> [f32; 4] {
        self.rows.map(|r| {
            (r[0] * ink[0] + r[1] * ink[1] + r[2] * ink[2] + r[3] * ink[3] + r[4]).clamp(0.0, 1.0)
        })
    }

    fn is_identity(&self) -> bool {
        self.rows.iter().enumerate().all(|(o, r)| {
            r.iter()
                .enumerate()
                .all(|(j, v)| *v == if j == o { 1.0 } else { 0.0 })
        })
    }

    fn heap_bytes(&self) -> u64 {
        0
    }
}

// -------------------------------------------------------- selective color --

const SELECTIVE_KEYS: [&str; 10] = [
    "reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks",
    "method",
];

/// Selective Color on ink (module doc).
pub(crate) struct InkSelective {
    kernel: SelectiveColor,
}

impl InkSelective {
    fn parse(params: &Map<String, Value>) -> Result<Self, String> {
        for (key, value) in params {
            if !SELECTIVE_KEYS.contains(&key.as_str()) {
                return Err(unknown("selective_color", key));
            }
            if let Some(range) = value.as_object() {
                if let Some(k) = range
                    .keys()
                    .find(|k| !["c", "m", "y", "k"].contains(&k.as_str()))
                {
                    return Err(unknown("selective_color", k));
                }
            }
        }
        SelectiveColor::parse(params)
            .map(|kernel| Self { kernel })
            .ok_or_else(|| invalid("selective_color"))
    }
}

impl InkKernel for InkSelective {
    fn operation(&self) -> &'static str {
        "selective_color"
    }

    fn apply(&self, ink: [f32; 4]) -> [f32; 4] {
        let weights = SelectiveColor::weights([1.0 - ink[0], 1.0 - ink[1], 1.0 - ink[2]]);
        self.kernel.nudged(ink, weights)
    }

    fn is_identity(&self) -> bool {
        self.kernel.is_identity()
    }

    fn heap_bytes(&self) -> u64 {
        0
    }
}

// --------------------------------------------------------------- threshold --

/// Threshold on ink (module doc).
pub(crate) struct InkThreshold {
    level: f32,
}

impl InkThreshold {
    fn parse(params: &Map<String, Value>) -> Result<Self, String> {
        if let Some(key) = params.keys().find(|k| k.as_str() != "level") {
            return Err(unknown("threshold", key));
        }
        let level = num_in(params, "level", 0.0..=1.0, 0.5).ok_or_else(|| invalid("threshold"))?;
        Ok(Self { level })
    }
}

impl InkKernel for InkThreshold {
    fn operation(&self) -> &'static str {
        "threshold"
    }

    fn apply(&self, ink: [f32; 4]) -> [f32; 4] {
        if ink_tone(ink) >= self.level {
            [0.0; 4]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    }

    fn is_identity(&self) -> bool {
        false
    }

    fn heap_bytes(&self) -> u64 {
        0
    }
}

// ------------------------------------------------------------ gradient map --

/// The most stops a map holds (the style gradient's own cap).
const MAX_STOPS: usize = 32;

/// One gradient-map stop: position, ink and opacity, each in [0, 1], and
/// its colour and opacity midpoints toward the next stop (module doc).
#[derive(Clone, Copy)]
struct InkStop {
    position: f32,
    ink: [f32; 4],
    opacity: f32,
    midpoint: f32,
    opacity_midpoint: f32,
}

/// Gradient Map on ink (module doc).
pub(crate) struct InkGradientMap {
    stops: Vec<InkStop>,
    reverse: bool,
    smoothness: f32,
    method: Option<MapMethod>,
}

impl InkGradientMap {
    fn parse(params: &Map<String, Value>) -> Result<Self, String> {
        if let Some(key) = params
            .keys()
            .find(|k| !matches!(k.as_str(), "gradient" | "dither" | "method"))
        {
            return Err(unknown("gradient_map", key));
        }
        boolean(params, "dither", true).ok_or_else(|| invalid("gradient_map"))?;
        let method = MapMethod::parse(params)
            .ok_or("a CMYK gradient map method must be \"classic\" when given")?;
        if method == Some(MapMethod::Perceptual) {
            return Err(refused_sentence("The gradient map's Perceptual method"));
        }
        let gradient = params
            .get("gradient")
            .and_then(Value::as_object)
            .ok_or("a CMYK gradient map requires a gradient object")?;
        if let Some(key) = gradient.keys().find(|k| {
            !matches!(
                k.as_str(),
                "stops"
                    | "style"
                    | "angle"
                    | "scale"
                    | "reverse"
                    | "align_with_layer"
                    | "smoothness"
            )
        }) {
            return Err(unknown("gradient_map", key));
        }
        let reverse = boolean(gradient, "reverse", false).ok_or_else(|| invalid("gradient_map"))?;
        let smoothness = num_in(gradient, "smoothness", 0.0..=1.0, 1.0)
            .ok_or_else(|| invalid("gradient_map"))?;
        let listed = gradient
            .get("stops")
            .and_then(Value::as_array)
            .filter(|s| (2..=MAX_STOPS).contains(&s.len()))
            .ok_or("a CMYK gradient map requires 2 to 32 stops")?;
        let mut stops = Vec::with_capacity(listed.len());
        for stop in listed {
            let stop = stop
                .as_object()
                .ok_or("a CMYK gradient stop must be an object")?;
            for key in stop.keys() {
                match key.as_str() {
                    "position" | "ink" | "opacity" | "midpoint" | "opacity_midpoint" => {}
                    "color" => {
                        return Err(refused_sentence(
                            "A gradient stop \"color\" (give the stop's \"ink\")",
                        ))
                    }
                    _ => return Err(unknown("gradient_map stop", key)),
                }
            }
            let position =
                num_in(stop, "position", 0.0..=1.0, 0.0).ok_or_else(|| invalid("gradient_map"))?;
            let opacity =
                num_in(stop, "opacity", 0.0..=1.0, 1.0).ok_or_else(|| invalid("gradient_map"))?;
            let ink = parse_ink(stop.get("ink")).ok_or("a CMYK gradient stop requires its ink")?;
            // Checked in f32, the midpoint's own type, so 0.05 and 0.95
            // themselves are admitted; then rounded as the RGB map's parser
            // rounds, so a `grdm` round trip renders the same.
            let midpoint = |key: &str| {
                num_in(stop, key, 0.0..=1.0, 0.5)
                    .filter(|m| midpoint_valid(*m))
                    .map(round_midpoint)
                    .ok_or_else(|| invalid("gradient_map"))
            };
            let (midpoint, opacity_midpoint) =
                (midpoint("midpoint")?, midpoint("opacity_midpoint")?);
            stops.push(InkStop {
                position,
                ink,
                opacity,
                midpoint,
                opacity_midpoint,
            });
        }
        // Stable: of two stops at one position the later stays later; each
        // stop carries its own midpoints through the sort.
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        Ok(Self {
            stops,
            reverse,
            smoothness,
            method,
        })
    }
}

/// Four numbers in [0, 1].
fn parse_ink(value: Option<&Value>) -> Option<[f32; 4]> {
    let list = value?.as_array()?;
    if list.len() != 4 {
        return None;
    }
    let mut out = [0.0f32; 4];
    for (slot, v) in out.iter_mut().zip(list) {
        let n = v.as_f64()?;
        if !(0.0..=1.0).contains(&n) {
            return None;
        }
        *slot = n as f32;
    }
    Some(out)
}

impl InkKernel for InkGradientMap {
    fn operation(&self) -> &'static str {
        "gradient_map"
    }

    fn apply(&self, ink: [f32; 4]) -> [f32; 4] {
        let mut t = ink_tone(ink).clamp(0.0, 1.0);
        if self.reverse {
            t = 1.0 - t;
        }
        let Some((i, j, f)) = segment(self.stops.len(), t, |i| self.stops[i].position) else {
            return ink;
        };
        let (a, b) = (self.stops[i], self.stops[j]);
        let Some(MapMethod::Classic) = self.method else {
            let opacity = (a.opacity + (b.opacity - a.opacity) * f).clamp(0.0, 1.0);
            return std::array::from_fn(|p| {
                let mapped = a.ink[p] + (b.ink[p] - a.ink[p]) * f;
                ink[p] + (mapped - ink[p]) * opacity
            });
        };
        let fo = if i == j {
            f
        } else {
            midpoint_fraction(f, a.opacity_midpoint)
        };
        let opacity = (a.opacity + (b.opacity - a.opacity) * fo).clamp(0.0, 1.0);
        let stops = &self.stops;
        let mapped = smooth_color::<4>(
            stops.len(),
            t,
            self.smoothness,
            ClassicBound::Bracket,
            |s| stops[s].position,
            |s| stops[s].midpoint,
            |s| stops[s].ink,
        );
        std::array::from_fn(|p| ink[p] + (mapped[p] - ink[p]) * opacity)
    }

    fn is_identity(&self) -> bool {
        self.stops.iter().all(|s| s.opacity == 0.0)
    }

    fn heap_bytes(&self) -> u64 {
        (self.stops.capacity() * std::mem::size_of::<InkStop>()) as u64
    }
}

// ----------------------------------------------------------- replace color --

/// The most inks one Replace Color lists (the RGB op's `MAX_COLORS`).
const MAX_INKS: usize = 8;

/// Replace Color on ink (module doc).
pub(crate) struct InkReplaceColor {
    /// The listed inks in 8-bit code units.
    inks: Vec<[f32; 4]>,
    fuzziness: f32,
    edit: HueSaturation,
}

impl InkReplaceColor {
    fn parse(params: &Map<String, Value>) -> Result<Self, String> {
        let mut sliders = Map::new();
        for (key, value) in params {
            match key.as_str() {
                "colors" => {
                    return Err(refused_sentence(
                        "The RGB \"colors\" key (give Replace Color's \"inks\")",
                    ))
                }
                "inks" | "fuzziness" => {}
                "hue" | "saturation" | "lightness" => {
                    sliders.insert(key.clone(), value.clone());
                }
                _ => return Err(unknown("replace_color", key)),
            }
        }
        let listed = params
            .get("inks")
            .and_then(Value::as_array)
            .filter(|l| (1..=MAX_INKS).contains(&l.len()))
            .ok_or("CMYK replace_color requires 1 to 8 inks")?;
        let inks = listed
            .iter()
            .map(|v| parse_ink(Some(v)).map(|ink| ink.map(|c| c * 255.0)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid("replace_color"))?;
        let fuzziness = num_in_f64(params, "fuzziness", 0.0..=200.0, 40.0)
            .ok_or_else(|| invalid("replace_color"))? as f32;
        let edit = HueSaturation::parse(&sliders).ok_or_else(|| invalid("replace_color"))?;
        Ok(Self {
            inks,
            fuzziness,
            edit,
        })
    }

    fn weight(&self, ink: [f32; 4]) -> f32 {
        let code = ink.map(|v| v * 255.0);
        let d = self
            .inks
            .iter()
            .map(|c| {
                (0..4)
                    .map(|p| (code[p] - c[p]) * (code[p] - c[p]))
                    .sum::<f32>()
                    .sqrt()
            })
            .fold(f32::INFINITY, f32::min);
        falloff(d, self.fuzziness)
    }
}

impl InkKernel for InkReplaceColor {
    fn operation(&self) -> &'static str {
        "replace_color"
    }

    fn apply(&self, ink: [f32; 4]) -> [f32; 4] {
        let w = self.weight(ink);
        if w <= 0.0 {
            return ink;
        }
        let rgb = self.edit.apply([1.0 - ink[0], 1.0 - ink[1], 1.0 - ink[2]]);
        let edited = [1.0 - rgb[0], 1.0 - rgb[1], 1.0 - rgb[2], ink[3]];
        if w >= 1.0 {
            return edited;
        }
        std::array::from_fn(|p| ink[p] + w * (edited[p] - ink[p]))
    }

    fn is_identity(&self) -> bool {
        self.edit.is_identity()
    }

    fn heap_bytes(&self) -> u64 {
        (self.inks.capacity() * std::mem::size_of::<[f32; 4]>()
            + self.edit.table().capacity() * std::mem::size_of::<f32>()) as u64
    }

    fn layer_refusal(&self) -> Option<String> {
        Some(
            "A Replace Color adjustment layer is unavailable in a CMYK document: Replace Color \
             applies to pixels only"
                .into(),
        )
    }
}
