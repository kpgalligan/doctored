//! The JSON parser for [`LayerStyle`]: ONE parser (`Strict` for the FFI
//! setter, `Lenient` for the DRDC reader — see `style`'s module doc, which
//! is the schema table), the clamp table it shares with `LayerStyle::scaled`,
//! and the four-decimal quantization applied at parse time so the stored
//! value IS its canonical form and a `to_json` echo (`style_json_write`)
//! re-parses to an equal style. The name tables are `style_names`.

use serde_json::{Map, Value};

use crate::authored_color::{hex_equivalent, srgb_numbers, LINEAR_LIMIT};
use crate::blend::BlendMode;
use crate::style::{
    BevelEmboss, BlendIf, BlendIfChannel, BlendIfRange, ColorOverlay, DropShadow, Effect,
    EffectKind, GradientFill, GradientOverlay, GradientStop, InnerGlow, InnerShadow, Knockout,
    LayerStyle, OuterGlow, Satin, Strictness, Stroke,
};
use crate::style_contour_json::parse_contour;
use crate::style_names::{
    parse_color, BEVEL_DIRECTIONS, BEVEL_STYLES, BEVEL_TECHNIQUES, BLEND_NAMES, CHANNELS,
    GLOW_FILLS, GLOW_SOURCES, GRADIENT_STYLES, KNOCKOUTS, STROKE_FILLS, STROKE_GRADIENT_STYLES,
    STROKE_POSITIONS,
};

/// Quantizes to four decimals (in f64, so the rounding is exact for every
/// value the schema allows) and narrows to the f32 the model stores. Four
/// decimals is finer than any dialog shows and coarse enough that the
/// decimal string serde_json prints re-parses to the same f32.
pub(crate) fn q4(v: f64) -> f32 {
    ((v * 1e4).round() / 1e4) as f32
}

/// [`q4`]'s f64 twin: the same four-decimal quantization, KEEPING the f64 —
/// for the geometric quantities the model stores at full precision because
/// they must survive a round trip through an Image Size (`doc_guide`'s guide
/// positions and ruler origin). It lives here, beside `q4`, so the crate has
/// exactly ONE four-decimal quantizer rather than a second copy elsewhere;
/// the reason for quantizing at all is `metadata`'s — so a host that echoes a
/// reported value back through the setter is refused as unchanged instead of
/// registering a phantom edit.
///
/// A magnitude large enough that `v * 1e4` overflows answers infinity rather
/// than panicking; every caller either clamps into the canvas afterwards or
/// range-checks and refuses.
pub(crate) fn q4_f64(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

/// An inclusive clamp range for one numeric key; [`Range::apply`] is the
/// parser's and `LayerStyle::scaled`'s shared "clamp then quantize".
#[derive(Clone, Copy, Debug)]
pub(crate) struct Range {
    pub lo: f32,
    pub hi: f32,
}

impl Range {
    pub(crate) fn apply(self, v: f64) -> f32 {
        let v = if v.is_finite() { v } else { f64::from(self.lo) };
        q4(v.clamp(f64::from(self.lo), f64::from(self.hi)))
    }
}

/// The one clamp table, keyed by what a number means. Bounds: size 250 px
/// (also what bounds every blur kernel), distance 30000 px, soften 16 px,
/// depth 1..1000 %, altitude 0..90°, gradient scale 10..150 %.
pub(crate) struct Ranges {
    /// Opacities, spread/choke fractions, gradient stop positions.
    pub unit: Range,
    pub distance: Range,
    pub size: Range,
    pub soften: Range,
    pub depth: Range,
    pub altitude: Range,
    pub gradient_scale: Range,
    /// A pattern overlay's scale: 1..1000 %.
    pub pattern_scale: Range,
    /// A pattern overlay's phase, each axis, in px.
    pub phase: Range,
    /// A glow's `range` and a bevel's `contour_range`: 1..100 %.
    pub contour_range: Range,
    /// A bevel texture's `depth`: −1000..1000 %.
    pub texture_depth: Range,
    /// A gradient stop's `midpoint` and `opacity_midpoint`:
    /// 5..95 % (`gradient_field::MIDPOINT_MIN..=MIDPOINT_MAX`).
    pub midpoint: Range,
}

pub(crate) const RANGES: Ranges = Ranges {
    unit: Range { lo: 0.0, hi: 1.0 },
    distance: Range {
        lo: 0.0,
        hi: 30000.0,
    },
    size: Range { lo: 0.0, hi: 250.0 },
    soften: Range { lo: 0.0, hi: 16.0 },
    depth: Range { lo: 0.01, hi: 10.0 },
    altitude: Range { lo: 0.0, hi: 90.0 },
    gradient_scale: Range { lo: 0.1, hi: 1.5 },
    pattern_scale: Range { lo: 0.01, hi: 10.0 },
    phase: Range {
        lo: -30000.0,
        hi: 30000.0,
    },
    contour_range: Range { lo: 0.01, hi: 1.0 },
    texture_depth: Range {
        lo: -10.0,
        hi: 10.0,
    },
    midpoint: Range {
        lo: crate::gradient_field::MIDPOINT_MIN,
        hi: crate::gradient_field::MIDPOINT_MAX,
    },
};

/// How many stops a gradient may have, everywhere one is parsed: a style's
/// or a fill layer's `GradientFill` ([`gradient`]) and a stops gradient of
/// the Gradient tool (`gradient_field::FieldStop` lists).
pub(crate) const GRADIENT_STOPS: std::ops::RangeInclusive<usize> = 2..=32;

// ------------------------------------------------------------- parsing --

/// `key` as reported in errors: `fill_opacity` at the root,
/// `effects[1].size` inside an effect.
pub(crate) fn at(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// `obj[key]` as a finite number clamped into `range` and quantized;
/// `default` when absent. Present but not a number (JSON cannot encode NaN
/// or infinities, so "number" means finite) is malformed.
pub(crate) fn num(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    default: f32,
    range: Range,
) -> Result<f32, String> {
    match obj.get(key) {
        None => Ok(range.apply(f64::from(default))),
        Some(v) => v
            .as_f64()
            .filter(|f| f.is_finite())
            .map(|f| range.apply(f))
            .ok_or_else(|| format!("{} must be a number", at(path, key))),
    }
}

/// `obj[key]` as an angle in degrees: any finite number, normalized into
/// [-180, 180) and quantized.
pub(crate) fn angle(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    default: f32,
) -> Result<f32, String> {
    let raw = match obj.get(key) {
        None => f64::from(default),
        Some(v) => v
            .as_f64()
            .filter(|f| f.is_finite())
            .ok_or_else(|| format!("{} must be a number", at(path, key)))?,
    };
    Ok(canonical_angle(raw))
}

/// Normalize into [-180, 180), quantize, and keep the quantized value in
/// range (179.99996 would round up to 180). The normalization runs in f64
/// BEFORE the narrowing to f32: JSON allows any finite double, and an
/// angle beyond the f32 range (1e39) narrowed first would become infinite,
/// its remainder NaN — a value the writer prints as `null`, the reader then
/// refuses, and `PartialEq` never matches (phantom undo steps). Every
/// finite double has a finite remainder, so the stored angle is always a
/// finite f32 in range.
pub(crate) fn canonical_angle(deg: f64) -> f32 {
    let normalized = (deg + 180.0).rem_euclid(360.0) - 180.0;
    let normalized = if normalized.is_finite() && normalized < 180.0 {
        normalized
    } else {
        -180.0
    };
    let q = q4(normalized);
    if q >= 180.0 {
        -180.0
    } else {
        q
    }
}

pub(crate) fn boolean(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    default: bool,
) -> Result<bool, String> {
    match obj.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("{} must be a boolean", at(path, key))),
    }
}

/// `obj[key]` as one of `table`'s names. Unknown: malformed in strict mode,
/// `default` in lenient mode. A non-string is malformed in both.
pub(crate) fn string_enum<T: Copy>(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    default: T,
    table: &[(&str, T)],
    strictness: Strictness,
) -> Result<T, String> {
    let Some(v) = obj.get(key) else {
        return Ok(default);
    };
    let name = v
        .as_str()
        .ok_or_else(|| format!("{} must be a string", at(path, key)))?;
    match table.iter().find(|(n, _)| *n == name) {
        Some((_, value)) => Ok(*value),
        None => match strictness {
            Strictness::Strict => {
                let names: Vec<&str> = table.iter().map(|(n, _)| *n).collect();
                Err(format!(
                    "{} must be one of {}",
                    at(path, key),
                    names.join(", ")
                ))
            }
            Strictness::Lenient => Ok(default),
        },
    }
}

fn color(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    default: [u8; 3],
) -> Result<[u8; 3], String> {
    match obj.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_str()
            .and_then(parse_color)
            .ok_or_else(|| format!("{} must be a #rrggbb color", at(path, key))),
    }
}

/// `obj[key]` as a straight CMYK ink `[c, m, y, k]` (#547): exactly four
/// finite numbers, each clamped into `0..=1` and quantized like every unit
/// value (`RANGES.unit`); `None` when absent or null. Anything else is
/// malformed in both modes.
fn ink(obj: &Map<String, Value>, path: &str, key: &str) -> Result<Option<[f32; 4]>, String> {
    let malformed = || format!("{} must be an array of 4 numbers", at(path, key));
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let list = v
                .as_array()
                .filter(|l| l.len() == 4)
                .ok_or_else(malformed)?;
            let mut out = [0f32; 4];
            for (slot, entry) in out.iter_mut().zip(list) {
                let f = entry
                    .as_f64()
                    .filter(|f| f.is_finite())
                    .ok_or_else(malformed)?;
                *slot = RANGES.unit.apply(f);
            }
            Ok(Some(out))
        }
    }
}

/// One colour slot's `color` and its wide colour under `linear_key`
/// (`style`'s WIDE COLOURS): exactly three finite numbers, each
/// clamped into `±LINEAR_LIMIT`; `None` when absent or null; anything else is
/// malformed in both modes. Normalized by [`normalized_wide`], so the
/// returned `color` always agrees with the wide colour.
fn wide(
    obj: &Map<String, Value>,
    path: &str,
    color_key: &str,
    linear_key: &str,
    default: [u8; 3],
) -> Result<([u8; 3], Option<[f32; 3]>), String> {
    let hex = color(obj, path, color_key, default)?;
    let malformed = || format!("{} must be an array of 3 numbers", at(path, linear_key));
    let list = match obj.get(linear_key) {
        None | Some(Value::Null) => return Ok((hex, None)),
        Some(v) => v
            .as_array()
            .filter(|l| l.len() == 3)
            .ok_or_else(malformed)?,
    };
    let mut linear = [0f32; 3];
    for (slot, entry) in linear.iter_mut().zip(list) {
        let f = entry
            .as_f64()
            .filter(|f| f.is_finite())
            .ok_or_else(malformed)?;
        let limit = f64::from(LINEAR_LIMIT);
        *slot = f.clamp(-limit, limit) as f32;
    }
    Ok(normalized_wide(linear))
}

/// A wide colour as the slot's `(color, linear)`: one that IS an sRGB
/// byte ([`hex_equivalent`]) is that byte and no wide colour; any other keeps
/// its components and takes the nearest byte of its clamped sRGB numbers as
/// `color`, so every reader of `color` (a CMYK document's ink, a PSD `RGBC`
/// with no conversion) draws the same colour.
pub(crate) fn normalized_wide(linear: [f32; 3]) -> ([u8; 3], Option<[f32; 3]>) {
    match hex_equivalent(linear) {
        Some(byte) => (byte, None),
        None => (
            srgb_numbers(linear).map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8),
            Some(linear),
        ),
    }
}

fn blend(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    default: BlendMode,
    strictness: Strictness,
) -> Result<BlendMode, String> {
    string_enum(obj, path, key, default, &BLEND_NAMES, strictness)
}

fn object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{path} must be an object"))
}

/// The ONE gradient parser. Its second caller is the `gradient_map`
/// adjustment (`adjust_map`), whose meta stores the SAME object so the core
/// has one gradient spelling and one editor — with one difference to keep in
/// mind: a style's stop colours are AUTHORED sRGB, converted into the
/// document's space at composite time, while a map's are the DOCUMENT's own
/// numbers, converted nowhere (see `adjust`'s colour note).
pub(crate) fn gradient(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    strictness: Strictness,
) -> Result<GradientFill, String> {
    gradient_with(obj, path, key, strictness, &GRADIENT_STYLES, false)
}

/// A layer EFFECT's gradient: [`gradient`] plus the two keys only an
/// effect reads — `dither` (a bool, default false) on `stroke` and
/// `gradient_overlay`, and the `shape_burst` style, admitted when `stroke`
/// (an overlay holding it is refused by the five-name sentence in strict
/// mode and reads as linear in lenient). The gradient map and artwork keep
/// [`gradient`], so neither option reaches them.
pub(crate) fn effect_gradient(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    strictness: Strictness,
    stroke: bool,
) -> Result<GradientFill, String> {
    let styles: &[(&str, crate::style::GradientStyle)] = if stroke {
        &STROKE_GRADIENT_STYLES
    } else {
        &GRADIENT_STYLES
    };
    gradient_with(obj, path, key, strictness, styles, true)
}

/// A glow's gradient (`style_glow_gradient`): [`gradient`]'s parse, then
/// every key but `stops` reset to its default, and every stop's midpoints
/// to 0.5 — a glow's ramp follows its
/// own plane, so the box, angle, scale, reverse, alignment and dither mean
/// nothing there, and keeping them would make two equal-looking glows
/// differ.
fn glow_gradient(
    obj: &Map<String, Value>,
    path: &str,
    strictness: Strictness,
) -> Result<GradientFill, String> {
    let mut stops = gradient(obj, path, "gradient", strictness)?.stops;
    // A glow's ramp ignores midpoints, like every other key.
    for stop in &mut stops {
        stop.midpoint = 0.5;
        stop.opacity_midpoint = 0.5;
    }
    Ok(GradientFill {
        stops,
        ..GradientFill::default()
    })
}

/// A gradient stop's `midpoint` or `opacity_midpoint`: [`num`] in
/// `RANGES.midpoint`, 0.5 when absent, then rounded to 0.01 (a whole
/// percent, as a PSD's `Mdpn` holds it).
fn midpoint(obj: &Map<String, Value>, path: &str, key: &str) -> Result<f32, String> {
    let v = num(obj, path, key, 0.5, RANGES.midpoint)?;
    Ok(crate::gradient_field::round_midpoint(v))
}

/// The ONE gradient body: `styles` is the style table the caller admits and
/// `dither` whether it reads the `dither` key.
fn gradient_with(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    strictness: Strictness,
    styles: &[(&str, crate::style::GradientStyle)],
    dither: bool,
) -> Result<GradientFill, String> {
    let default = GradientFill::default();
    let Some(v) = obj.get(key) else {
        return Ok(default);
    };
    let gpath = at(path, key);
    let g = object(v, &gpath)?;
    let stops = match g.get("stops") {
        None => default.stops.clone(),
        Some(list) => {
            let spath = at(&gpath, "stops");
            let list = list
                .as_array()
                .ok_or_else(|| format!("{spath} must be an array"))?;
            if !GRADIENT_STOPS.contains(&list.len()) {
                return Err(format!("{spath} must have 2 to 32 entries"));
            }
            let mut stops = Vec::with_capacity(list.len());
            for (i, entry) in list.iter().enumerate() {
                let epath = format!("{spath}[{i}]");
                let e = object(entry, &epath)?;
                stops.push(GradientStop {
                    position: num(e, &epath, "position", 0.0, RANGES.unit)?,
                    color: color(e, &epath, "color", [0, 0, 0])?,
                    opacity: num(e, &epath, "opacity", 1.0, RANGES.unit)?,
                    ink: ink(e, &epath, "ink")?,
                    midpoint: midpoint(e, &epath, "midpoint")?,
                    opacity_midpoint: midpoint(e, &epath, "opacity_midpoint")?,
                });
            }
            stops.sort_by(|a, b| a.position.total_cmp(&b.position));
            // The last stop's midpoints govern no segment: reset, so
            // two gradients that draw alike compare and write alike.
            if let Some(last) = stops.last_mut() {
                last.midpoint = 0.5;
                last.opacity_midpoint = 0.5;
            }
            stops
        }
    };
    Ok(GradientFill {
        stops,
        style: string_enum(g, &gpath, "style", default.style, styles, strictness)?,
        angle: angle(g, &gpath, "angle", default.angle)?,
        scale: num(g, &gpath, "scale", default.scale, RANGES.gradient_scale)?,
        reverse: boolean(g, &gpath, "reverse", default.reverse)?,
        align_with_layer: boolean(g, &gpath, "align_with_layer", default.align_with_layer)?,
        dither: dither && boolean(g, &gpath, "dither", default.dither)?,
        smoothness: num(g, &gpath, "smoothness", default.smoothness, RANGES.unit)?,
    })
}

/// One Blend If ramp: exactly four integers in 0..=255 (a float with a zero
/// fraction is accepted; out-of-range values clamp) in non-decreasing order.
fn ramp(obj: &Map<String, Value>, path: &str, key: &str) -> Result<[u8; 4], String> {
    let Some(v) = obj.get(key) else {
        return Ok(crate::style::FULL_RAMP);
    };
    let rpath = at(path, key);
    let list = v
        .as_array()
        .filter(|l| l.len() == 4)
        .ok_or_else(|| format!("{rpath} must be an array of 4 integers"))?;
    let mut out = [0u8; 4];
    for (slot, entry) in out.iter_mut().zip(list) {
        let f = entry
            .as_f64()
            .filter(|f| f.is_finite() && f.fract() == 0.0)
            .ok_or_else(|| format!("{rpath} must be an array of 4 integers"))?;
        *slot = f.clamp(0.0, 255.0) as u8;
    }
    if !(out[0] <= out[1] && out[1] <= out[2] && out[2] <= out[3]) {
        return Err(format!("{rpath} must be in non-decreasing order"));
    }
    Ok(out)
}

pub(crate) fn parse_blend_if(
    value: &Value,
    strictness: Strictness,
) -> Result<Option<BlendIf>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let b = value
        .as_object()
        .ok_or_else(|| "blend_if must be an object or null".to_string())?;
    let first = blend_if_range(b, "blend_if", strictness)?;
    let mut rest = Vec::new();
    if let Some(extra) = b.get("extra") {
        let list = extra
            .as_array()
            .ok_or_else(|| "blend_if.extra must be an array".to_string())?;
        for (i, entry) in list.iter().enumerate() {
            let path = format!("blend_if.extra[{i}]");
            let o = entry
                .as_object()
                .ok_or_else(|| format!("{path} must be an object"))?;
            rest.push(blend_if_range(o, &path, strictness)?);
        }
    }
    BlendIf::from_ranges(first, &rest).map(Some)
}

/// One Blend If range object (`channel`, `this_layer`, `underlying`): the
/// primary keys of `blend_if`, or one entry of its `extra`.
fn blend_if_range(
    o: &Map<String, Value>,
    path: &str,
    strictness: Strictness,
) -> Result<BlendIfRange, String> {
    Ok(BlendIfRange {
        channel: string_enum(
            o,
            path,
            "channel",
            BlendIfChannel::Gray,
            &CHANNELS,
            strictness,
        )?,
        this_layer: ramp(o, path, "this_layer")?,
        underlying: ramp(o, path, "underlying")?,
    })
}

/// Every range's channel of a `blend_if` value AS WRITTEN — the primary,
/// then each `extra` entry in order, identity ranges included — read the
/// way [`parse_blend_if`] reads it (an absent channel is Gray). The
/// setters' mode check runs on this before canonicalization drops identity
/// ranges (#547: a channel the mode lacks refuses even as an identity ramp).
/// Called only on a value `parse_blend_if` accepted; empty for null.
pub(crate) fn requested_blend_if_channels(blend_if: &Value) -> Vec<BlendIfChannel> {
    let Some(b) = blend_if.as_object() else {
        return Vec::new();
    };
    let channel = |o: &Map<String, Value>| {
        o.get("channel")
            .and_then(Value::as_str)
            .and_then(|name| CHANNELS.iter().find(|(n, _)| *n == name))
            .map_or(BlendIfChannel::Gray, |(_, c)| *c)
    };
    let mut out = vec![channel(b)];
    if let Some(list) = b.get("extra").and_then(Value::as_array) {
        out.extend(list.iter().filter_map(Value::as_object).map(channel));
    }
    out
}

/// One effect object. `Ok(None)` means "skipped" (an unknown `type` in
/// lenient mode); a structural problem is `Err` in both modes.
fn effect(value: &Value, path: &str, strictness: Strictness) -> Result<Option<Effect>, String> {
    let o = object(value, path)?;
    let type_key = at(path, "type");
    let type_name = o
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{type_key} must be a string"))?;
    let Some(kind) = EffectKind::from_type_name(type_name) else {
        return match strictness {
            Strictness::Strict => Err(format!(
                "{type_key} must be one of {}",
                EffectKind::ALL
                    .iter()
                    .map(|k| k.type_name())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Strictness::Lenient => Ok(None),
        };
    };
    let enabled = boolean(o, path, "enabled", true)?;
    let unit = RANGES.unit;
    let parsed = match kind {
        EffectKind::DropShadow => {
            let d = DropShadow::default();
            let (color, linear) = wide(o, path, "color", "linear", d.color)?;
            Effect::DropShadow(DropShadow {
                enabled,
                blend: blend(o, path, "blend", d.blend, strictness)?,
                color,
                ink: ink(o, path, "ink")?,
                linear,
                opacity: num(o, path, "opacity", d.opacity, unit)?,
                angle: angle(o, path, "angle", d.angle)?,
                use_global_light: boolean(o, path, "use_global_light", d.use_global_light)?,
                distance: num(o, path, "distance", d.distance, RANGES.distance)?,
                spread: num(o, path, "spread", d.spread, unit)?,
                size: num(o, path, "size", d.size, RANGES.size)?,
                layer_knocks_out: boolean(o, path, "layer_knocks_out", d.layer_knocks_out)?,
                contour: parse_contour(o, path, "contour", strictness)?,
                anti_aliased: boolean(o, path, "anti_aliased", d.anti_aliased)?,
                noise: num(o, path, "noise", d.noise, unit)?,
            })
        }
        EffectKind::InnerShadow => {
            let d = InnerShadow::default();
            let (color, linear) = wide(o, path, "color", "linear", d.color)?;
            Effect::InnerShadow(InnerShadow {
                enabled,
                blend: blend(o, path, "blend", d.blend, strictness)?,
                color,
                ink: ink(o, path, "ink")?,
                linear,
                opacity: num(o, path, "opacity", d.opacity, unit)?,
                angle: angle(o, path, "angle", d.angle)?,
                use_global_light: boolean(o, path, "use_global_light", d.use_global_light)?,
                distance: num(o, path, "distance", d.distance, RANGES.distance)?,
                choke: num(o, path, "choke", d.choke, unit)?,
                size: num(o, path, "size", d.size, RANGES.size)?,
                contour: parse_contour(o, path, "contour", strictness)?,
                anti_aliased: boolean(o, path, "anti_aliased", d.anti_aliased)?,
                noise: num(o, path, "noise", d.noise, unit)?,
            })
        }
        EffectKind::OuterGlow => {
            let d = OuterGlow::default();
            let (color, linear) = wide(o, path, "color", "linear", d.color)?;
            Effect::OuterGlow(OuterGlow {
                enabled,
                blend: blend(o, path, "blend", d.blend, strictness)?,
                color,
                ink: ink(o, path, "ink")?,
                linear,
                opacity: num(o, path, "opacity", d.opacity, unit)?,
                spread: num(o, path, "spread", d.spread, unit)?,
                size: num(o, path, "size", d.size, RANGES.size)?,
                contour: parse_contour(o, path, "contour", strictness)?,
                anti_aliased: boolean(o, path, "anti_aliased", d.anti_aliased)?,
                range: num(o, path, "range", d.range, RANGES.contour_range)?,
                noise: num(o, path, "noise", d.noise, unit)?,
                fill_type: string_enum(o, path, "fill_type", d.fill_type, &GLOW_FILLS, strictness)?,
                gradient: glow_gradient(o, path, strictness)?,
                jitter: num(o, path, "jitter", d.jitter, unit)?,
            })
        }
        EffectKind::InnerGlow => {
            let d = InnerGlow::default();
            let (color, linear) = wide(o, path, "color", "linear", d.color)?;
            Effect::InnerGlow(InnerGlow {
                enabled,
                blend: blend(o, path, "blend", d.blend, strictness)?,
                color,
                ink: ink(o, path, "ink")?,
                linear,
                opacity: num(o, path, "opacity", d.opacity, unit)?,
                choke: num(o, path, "choke", d.choke, unit)?,
                size: num(o, path, "size", d.size, RANGES.size)?,
                source: string_enum(o, path, "source", d.source, &GLOW_SOURCES, strictness)?,
                contour: parse_contour(o, path, "contour", strictness)?,
                anti_aliased: boolean(o, path, "anti_aliased", d.anti_aliased)?,
                range: num(o, path, "range", d.range, RANGES.contour_range)?,
                noise: num(o, path, "noise", d.noise, unit)?,
                fill_type: string_enum(o, path, "fill_type", d.fill_type, &GLOW_FILLS, strictness)?,
                gradient: glow_gradient(o, path, strictness)?,
                jitter: num(o, path, "jitter", d.jitter, unit)?,
            })
        }
        EffectKind::Stroke => {
            let d = Stroke::default();
            let (color, linear) = wide(o, path, "color", "linear", d.color)?;
            Effect::Stroke(Stroke {
                enabled,
                blend: blend(o, path, "blend", d.blend, strictness)?,
                opacity: num(o, path, "opacity", d.opacity, unit)?,
                size: num(o, path, "size", d.size, RANGES.size)?,
                position: string_enum(
                    o,
                    path,
                    "position",
                    d.position,
                    &STROKE_POSITIONS,
                    strictness,
                )?,
                fill_type: string_enum(
                    o,
                    path,
                    "fill_type",
                    d.fill_type,
                    &STROKE_FILLS,
                    strictness,
                )?,
                color,
                ink: ink(o, path, "ink")?,
                linear,
                gradient: effect_gradient(o, path, "gradient", strictness, true)?,
                pattern_fill: match o.get("pattern_fill") {
                    None => d.pattern_fill,
                    Some(v) => {
                        let key = at(path, "pattern_fill");
                        crate::style_pattern_fill::parse_fill(object(v, &key)?, &key, strictness)?
                    }
                },
            })
        }
        EffectKind::ColorOverlay => {
            let d = ColorOverlay::default();
            let (color, linear) = wide(o, path, "color", "linear", d.color)?;
            Effect::ColorOverlay(ColorOverlay {
                enabled,
                blend: blend(o, path, "blend", d.blend, strictness)?,
                color,
                ink: ink(o, path, "ink")?,
                linear,
                opacity: num(o, path, "opacity", d.opacity, unit)?,
            })
        }
        EffectKind::GradientOverlay => {
            let d = GradientOverlay::default();
            Effect::GradientOverlay(GradientOverlay {
                enabled,
                blend: blend(o, path, "blend", d.blend, strictness)?,
                opacity: num(o, path, "opacity", d.opacity, unit)?,
                gradient: effect_gradient(o, path, "gradient", strictness, false)?,
            })
        }
        EffectKind::PatternOverlay => Effect::PatternOverlay(crate::style_pattern::parse(
            o,
            path,
            enabled,
            blend(o, path, "blend", BlendMode::Normal, strictness)?,
            num(o, path, "opacity", 1.0, unit)?,
            strictness,
        )?),
        EffectKind::BevelEmboss => {
            let d = BevelEmboss::default();
            let (highlight_color, highlight_linear) = wide(
                o,
                path,
                "highlight_color",
                "highlight_linear",
                d.highlight_color,
            )?;
            let (shadow_color, shadow_linear) =
                wide(o, path, "shadow_color", "shadow_linear", d.shadow_color)?;
            Effect::BevelEmboss(BevelEmboss {
                enabled,
                style: string_enum(o, path, "style", d.style, &BEVEL_STYLES, strictness)?,
                technique: string_enum(
                    o,
                    path,
                    "technique",
                    d.technique,
                    &BEVEL_TECHNIQUES,
                    strictness,
                )?,
                depth: num(o, path, "depth", d.depth, RANGES.depth)?,
                direction: string_enum(
                    o,
                    path,
                    "direction",
                    d.direction,
                    &BEVEL_DIRECTIONS,
                    strictness,
                )?,
                size: num(o, path, "size", d.size, RANGES.size)?,
                soften: num(o, path, "soften", d.soften, RANGES.soften)?,
                angle: angle(o, path, "angle", d.angle)?,
                use_global_light: boolean(o, path, "use_global_light", d.use_global_light)?,
                altitude: num(o, path, "altitude", d.altitude, RANGES.altitude)?,
                highlight_blend: blend(o, path, "highlight_blend", d.highlight_blend, strictness)?,
                highlight_color,
                highlight_ink: ink(o, path, "highlight_ink")?,
                highlight_linear,
                highlight_opacity: num(o, path, "highlight_opacity", d.highlight_opacity, unit)?,
                shadow_blend: blend(o, path, "shadow_blend", d.shadow_blend, strictness)?,
                shadow_color,
                shadow_ink: ink(o, path, "shadow_ink")?,
                shadow_linear,
                shadow_opacity: num(o, path, "shadow_opacity", d.shadow_opacity, unit)?,
                gloss_contour: parse_contour(o, path, "gloss_contour", strictness)?,
                gloss_anti_aliased: boolean(o, path, "gloss_anti_aliased", d.gloss_anti_aliased)?,
                contour_enabled: boolean(o, path, "contour_enabled", d.contour_enabled)?,
                contour: parse_contour(o, path, "contour", strictness)?,
                contour_anti_aliased: boolean(
                    o,
                    path,
                    "contour_anti_aliased",
                    d.contour_anti_aliased,
                )?,
                contour_range: num(
                    o,
                    path,
                    "contour_range",
                    d.contour_range,
                    RANGES.contour_range,
                )?,
                texture_enabled: boolean(o, path, "texture_enabled", d.texture_enabled)?,
                texture: match o.get("texture") {
                    None => d.texture,
                    Some(v) => {
                        let key = at(path, "texture");
                        crate::style_pattern_fill::parse_texture(
                            object(v, &key)?,
                            &key,
                            strictness,
                        )?
                    }
                },
            })
        }
        EffectKind::Satin => {
            let d = Satin::default();
            let (color, linear) = wide(o, path, "color", "linear", d.color)?;
            Effect::Satin(Satin {
                enabled,
                blend: blend(o, path, "blend", d.blend, strictness)?,
                color,
                ink: ink(o, path, "ink")?,
                linear,
                opacity: num(o, path, "opacity", d.opacity, unit)?,
                angle: angle(o, path, "angle", d.angle)?,
                distance: num(o, path, "distance", d.distance, RANGES.distance)?,
                size: num(o, path, "size", d.size, RANGES.size)?,
                invert: boolean(o, path, "invert", d.invert)?,
                contour: parse_contour(o, path, "contour", strictness)?,
                anti_aliased: boolean(o, path, "anti_aliased", d.anti_aliased)?,
            })
        }
    };
    Ok(Some(parsed))
}

impl LayerStyle {
    /// Parses the style JSON in STRICT mode (the FFI setter's rules — see
    /// `style`'s module doc). The message of an `Err` names the offending
    /// key (`effects[1].size must be a number`) or says `not valid JSON`.
    pub fn from_json(json: &str) -> Result<LayerStyle, String> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| format!("not valid JSON: {e}"))?;
        LayerStyle::from_value(&value, Strictness::Strict)
    }

    /// Parses in LENIENT mode (the DRDC reader's rules): an unknown effect
    /// `type` is skipped, a duplicate keeps the first (a repeating kind its
    /// first ten, `style_instances`), an unknown enum
    /// string takes the default; structure, types, finiteness and ramp
    /// order stay strict.
    pub fn from_json_lenient(json: &str) -> Result<LayerStyle, String> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| format!("not valid JSON: {e}"))?;
        LayerStyle::from_value(&value, Strictness::Lenient)
    }

    /// The one parser behind both `from_json` variants, over an
    /// already-decoded value (so the FFI can word a syntax error and a
    /// schema error differently).
    pub(crate) fn from_value(value: &Value, strictness: Strictness) -> Result<LayerStyle, String> {
        let root = value
            .as_object()
            .ok_or_else(|| "style must be a JSON object".to_string())?;
        let fill_opacity = num(root, "", "fill_opacity", 1.0, RANGES.unit)?;
        let blend_if = match root.get("blend_if") {
            None => None,
            Some(v) => parse_blend_if(v, strictness)?,
        };
        // The two BLENDING OPTIONS beside `fill_opacity`/`blend_if`
        // (`style_knockout`). Both take their default when absent, which is
        // what makes the writer's conditional emission a round trip.
        let knockout = string_enum(root, "", "knockout", Knockout::None, &KNOCKOUTS, strictness)?;
        let blend_interior_as_group = boolean(root, "", "blend_interior_as_group", false)?;
        let mut effects: Vec<Effect> = Vec::new();
        if let Some(list) = root.get("effects") {
            let list = list
                .as_array()
                .ok_or_else(|| "effects must be an array".to_string())?;
            for (i, entry) in list.iter().enumerate() {
                let path = format!("effects[{i}]");
                let Some(parsed) = effect(entry, &path, strictness)? else {
                    continue;
                };
                // The instance rules (`style_instances`): a single kind holds
                // one, a repeating kind up to ten; lenient keeps the first.
                let held = effects.iter().filter(|e| e.kind() == parsed.kind()).count();
                if let Some(sentence) = crate::style_instances::refusal(parsed.kind(), held) {
                    match strictness {
                        Strictness::Strict => {
                            return Err(format!("{} {sentence}", at(&path, "type")))
                        }
                        Strictness::Lenient => continue,
                    }
                }
                effects.push(parsed);
            }
        }
        effects.sort_by_key(Effect::kind);
        Ok(LayerStyle {
            fill_opacity,
            blend_if,
            knockout,
            blend_interior_as_group,
            effects,
            cache: Default::default(),
        })
    }
}
