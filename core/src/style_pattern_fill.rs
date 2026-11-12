//! The PATTERN FILL: the tile, angle, scale, link, phase and anchor a
//! Pattern Overlay, a Stroke of Fill Type Pattern and a Bevel & Emboss
//! TEXTURE share — one model, one parser, one writer and one placement, so
//! the three cannot drift.
//!
//! # The JSON
//!
//! A Pattern Overlay holds the keys FLAT in its effect object; a Stroke holds
//! them in its `pattern_fill` object. The keys, ranges, defaults and refusals
//! are the same in both (`style`'s module doc is the table): `pattern` is
//! `{"preset": id}` or `{"name", "width", "height", "rgba"}` (standard base64
//! of `width × height × 4` straight RGBA8 bytes, each side 1..=512), the
//! writer emitting exactly one form; `angle` a number (0), always written;
//! `scale` 0.01..=10 (1.0); `link_with_layer` (true); `phase` [x, y] px
//! ([0, 0]); `anchor` two integers, written only when set. An unknown preset
//! id is refused in strict mode and takes the default (checkerboard) in
//! lenient mode; every other malformed pattern or anchor is refused in both.
//! A Stroke writes `pattern_fill` only when [`stroke_carries_pattern`], so
//! every Color or Gradient stroke's JSON is the prior writer's.
//!
//! # The bevel's texture
//!
//! A Bevel & Emboss holds `texture_enabled` (written only when true) and a
//! `texture` object ([`BevelTexture`]: the fill's `pattern`, `scale`,
//! `link_with_layer`, `phase`, `anchor`, plus `depth` −10..=10, 1 = +100 %,
//! and `invert`), written only when [`bevel_carries_texture`]. The PSD
//! bevel texture has no Angle, so `angle` is not a texture key: a parsed
//! one is discarded and none is written. The texture is HEIGHT, never
//! colour (`style_texture`): it paints nothing, and a CMYK document's ink
//! pyramid never reads it. [`pattern_fill`] therefore keeps its meaning
//! (the fills that paint); [`placed_fill`] adds the enabled texture for the
//! callers that only need a placed tile (the level's bytes, the canvas
//! alignment, the PSD pattern list and reference, the resource inventory).
//!
//! # Placement
//!
//! Placement is `style_pattern`'s FOLLOW / AT-CREATION rule for both
//! effects: [`PatternFill::origin`]. A stroke's tile sits exactly where a
//! Pattern Overlay's would (`stroke_fill_take.py`'s take1, PLACE = OVERLAY:
//! every pattern stroke row at Outside, Inside, Center, unlinked-and-moved
//! and 16 bits exact), so the band is a window onto the overlay's plane.

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::adjust_lut::{base64_decode, base64_encode};
use crate::pattern_tile::{tile_level, PatternSampler, PatternTile, MAX_TILE_SIDE, PRESETS};
use crate::style::{BevelEmboss, Effect, Strictness, Stroke, StrokeFill};
use crate::style_json::{angle, at, boolean, num, Range, RANGES};
use crate::style_pattern::{Pattern, PatternSource};

/// A pattern fill (module doc).
#[derive(Clone, Debug, PartialEq)]
pub struct PatternFill {
    pub pattern: Pattern,
    /// The Angle in degrees, canonical [-180, 180)
    /// (`style_json::canonical_angle`); 0 draws the tile unrotated
    /// (`style_pattern`'s Angle).
    pub angle: f32,
    /// 0.01..=10 (1..1000 %).
    pub scale: f32,
    /// "Link with Layer".
    pub link_with_layer: bool,
    /// The tile's offset in canvas px, each −30000..=30000.
    pub phase: [f32; 2],
    /// The layer's offset when the link was made (`style_pattern`'s
    /// Placement); `None` when unlinked, or linked and not yet set on a layer.
    pub anchor: Option<[i32; 2]>,
}

impl Default for PatternFill {
    fn default() -> Self {
        PatternFill {
            pattern: Pattern::default(),
            angle: 0.0,
            scale: 1.0,
            link_with_layer: true,
            phase: [0.0, 0.0],
            anchor: None,
        }
    }
}

impl PatternFill {
    /// The tile origin in LAYER px for a layer at `layer_offset`
    /// (`style_pattern`'s Placement): phase − anchor when linked (a missing
    /// anchor counts as the current offset), phase − offset when not.
    pub(crate) fn origin(&self, layer_offset: (i32, i32)) -> (f64, f64) {
        let offset = [layer_offset.0, layer_offset.1];
        let base = match (self.link_with_layer, self.anchor) {
            (true, Some(anchor)) => anchor,
            _ => offset,
        };
        (
            f64::from(self.phase[0]) - f64::from(base[0]),
            f64::from(self.phase[1]) - f64::from(base[1]),
        )
    }

    /// The placed PYRAMID level (`pattern_tile::tile_level`) for a layer at
    /// `layer_offset`: what a plane's colour and coverage both read.
    pub(crate) fn sampler(&self, layer_offset: (i32, i32)) -> PatternSampler {
        PatternSampler {
            level: Arc::new(tile_level(
                &self.pattern.tile,
                f64::from(self.scale),
                f64::from(self.angle),
            )),
            origin: self.origin(layer_offset),
        }
    }

    /// "Scale Effects" (`LayerStyle::scaled`): `scale`, `phase` and `anchor`
    /// (rounded) multiplied by `factor`, re-clamped and re-quantized exactly
    /// as the parser would.
    pub(crate) fn scaled(&self, factor: f64) -> PatternFill {
        let px = |v: f32, range: Range| range.apply(f64::from(v) * factor);
        PatternFill {
            scale: px(self.scale, RANGES.pattern_scale),
            phase: self.phase.map(|v| px(v, RANGES.phase)),
            anchor: self.anchor.map(|a| {
                a.map(|v| {
                    (f64::from(v) * factor)
                        .round()
                        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
                })
            }),
            ..self.clone()
        }
    }
}

/// A Bevel & Emboss texture (module doc): the placed tile (its `angle`
/// always 0) read as height, its Depth (1 = +100 %) and Invert.
#[derive(Clone, Debug, PartialEq)]
pub struct BevelTexture {
    pub fill: PatternFill,
    /// −10..=10 (−1000..1000 %).
    pub depth: f32,
    pub invert: bool,
}

impl Default for BevelTexture {
    fn default() -> Self {
        BevelTexture {
            fill: PatternFill::default(),
            depth: 1.0,
            invert: false,
        }
    }
}

/// The fill an effect paints with: a Pattern Overlay's, or a Stroke's when its
/// Fill Type is Pattern; `None` for every other effect.
pub(crate) fn pattern_fill(effect: &Effect) -> Option<&PatternFill> {
    match effect {
        Effect::PatternOverlay(p) => Some(&p.fill),
        Effect::Stroke(s) if s.fill_type == StrokeFill::Pattern => Some(&s.pattern_fill),
        _ => None,
    }
}

/// Every tile an effect PLACES: [`pattern_fill`], else a bevel's texture
/// fill when `texture_enabled` (module doc).
pub(crate) fn placed_fill(effect: &Effect) -> Option<&PatternFill> {
    match effect {
        Effect::BevelEmboss(b) if b.texture_enabled => Some(&b.texture.fill),
        other => pattern_fill(other),
    }
}

/// Whether a bevel's `texture` is part of what it stores: Texture is on, or
/// the texture is not the default (a bevel whose Texture was switched off
/// keeps its tile for the sheet). The JSON writer and `.rz` word 98 key on
/// it.
pub(crate) fn bevel_carries_texture(b: &BevelEmboss) -> bool {
    b.texture_enabled || b.texture != BevelTexture::default()
}

/// Whether a stroke's `pattern_fill` is part of what it stores: its Fill Type
/// is Pattern, or the fill is not the default (a Color stroke that was once
/// a Pattern one keeps its tile for the sheet). The JSON writer and `.rz`
/// word 95 key on it.
pub(crate) fn stroke_carries_pattern(s: &Stroke) -> bool {
    s.fill_type == StrokeFill::Pattern || s.pattern_fill != PatternFill::default()
}

// ----------------------------------------------------------------- JSON --

/// The fill's keys read from `o` (an overlay's effect object, or a stroke's
/// `pattern_fill` object), reported at `path` (module doc).
pub(crate) fn parse_fill(
    o: &Map<String, Value>,
    path: &str,
    strictness: Strictness,
) -> Result<PatternFill, String> {
    let d = PatternFill::default();
    let link_with_layer = match o.get("link_with_layer") {
        None => d.link_with_layer,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("{} must be a boolean", at(path, "link_with_layer")))?,
    };
    Ok(PatternFill {
        pattern: parse_pattern(o, path, strictness)?,
        angle: angle(o, path, "angle", d.angle)?,
        scale: num(o, path, "scale", d.scale, RANGES.pattern_scale)?,
        link_with_layer,
        phase: parse_phase(o, path)?,
        anchor: parse_anchor(o, path)?,
    })
}

/// A bevel's `texture` object read from `o`, reported at `path` (module
/// doc): the fill's keys with `angle` discarded, plus `depth` and `invert`.
pub(crate) fn parse_texture(
    o: &Map<String, Value>,
    path: &str,
    strictness: Strictness,
) -> Result<BevelTexture, String> {
    let d = BevelTexture::default();
    let fill = parse_fill(o, path, strictness)?;
    Ok(BevelTexture {
        fill: PatternFill { angle: 0.0, ..fill },
        depth: num(o, path, "depth", d.depth, RANGES.texture_depth)?,
        invert: boolean(o, path, "invert", d.invert)?,
    })
}

fn parse_phase(o: &Map<String, Value>, path: &str) -> Result<[f32; 2], String> {
    let Some(v) = o.get("phase") else {
        return Ok([0.0, 0.0]);
    };
    let malformed = || format!("{} must be an array of 2 numbers", at(path, "phase"));
    let list = v
        .as_array()
        .filter(|l| l.len() == 2)
        .ok_or_else(malformed)?;
    let mut out = [0f32; 2];
    for (slot, entry) in out.iter_mut().zip(list) {
        let f = entry
            .as_f64()
            .filter(|f| f.is_finite())
            .ok_or_else(malformed)?;
        *slot = RANGES.phase.apply(f);
    }
    Ok(out)
}

fn parse_anchor(o: &Map<String, Value>, path: &str) -> Result<Option<[i32; 2]>, String> {
    let malformed = || format!("{} must be two integers", at(path, "anchor"));
    match o.get("anchor") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let list = v
                .as_array()
                .filter(|l| l.len() == 2)
                .ok_or_else(malformed)?;
            let mut out = [0i32; 2];
            for (slot, entry) in out.iter_mut().zip(list) {
                *slot = entry
                    .as_i64()
                    .and_then(|i| i32::try_from(i).ok())
                    .ok_or_else(malformed)?;
            }
            Ok(Some(out))
        }
    }
}

pub(crate) fn parse_pattern(
    o: &Map<String, Value>,
    path: &str,
    strictness: Strictness,
) -> Result<Pattern, String> {
    let key = at(path, "pattern");
    let Some(v) = o.get("pattern") else {
        return Ok(Pattern::default());
    };
    pattern_from_value(v, &key, strictness)
}

/// A native paint request's optional `pattern` (Edit ▸ Fill…'s Pattern
/// contents, `precision_paint_tools` and `cmyk_paint`): absent or null is
/// `None`, anything else the one `pattern` object read strictly
/// ([`pattern_from_value`], an unknown preset refused).
pub(crate) fn paint_pattern<'de, D>(deserializer: D) -> Result<Option<Pattern>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(None),
        Some(v) => pattern_from_value(&v, "pattern", Strictness::Strict)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// One `pattern` object (module doc) read from `v`, its errors reported at
/// `key`: the one reader every effect, fill layer and Fill… shares.
pub(crate) fn pattern_from_value(
    v: &Value,
    key: &str,
    strictness: Strictness,
) -> Result<Pattern, String> {
    let p = v
        .as_object()
        .ok_or_else(|| format!("{key} must be an object"))?;
    if let Some(id) = p.get("preset") {
        let id = id
            .as_str()
            .ok_or_else(|| format!("{key}.preset must be a string"))?;
        return match Pattern::preset(id) {
            Some(pattern) => Ok(pattern),
            None => match strictness {
                Strictness::Strict => {
                    let ids: Vec<&str> = PRESETS.iter().map(|(p, _)| *p).collect();
                    Err(format!("{key}.preset must be one of {}", ids.join(", ")))
                }
                Strictness::Lenient => Ok(Pattern::default()),
            },
        };
    }
    let Some(rgba) = p.get("rgba") else {
        return Err(format!("{key} must hold either preset or rgba"));
    };
    let side = |name: &str| -> Result<u32, String> {
        p.get(name)
            .and_then(Value::as_u64)
            .filter(|v| (1..=u64::from(MAX_TILE_SIDE)).contains(v))
            .map(|v| v as u32)
            .ok_or_else(|| format!("{key}.{name} must be an integer 1..={MAX_TILE_SIDE}"))
    };
    let (width, height) = (side("width")?, side("height")?);
    let name = match p.get("name") {
        None => "Custom".to_string(),
        Some(n) => n
            .as_str()
            .ok_or_else(|| format!("{key}.name must be a string"))?
            .to_string(),
    };
    let bytes = rgba
        .as_str()
        .and_then(base64_decode)
        .filter(|b| b.len() == width as usize * height as usize * 4)
        .ok_or_else(|| format!("{key}.rgba must be base64 of width x height x 4 bytes"))?;
    Ok(Pattern {
        source: PatternSource::Custom { name },
        tile: Arc::new(PatternTile {
            width,
            height,
            rgba: bytes.into(),
        }),
    })
}

/// The canonical `pattern` object: exactly one form, keys sorted.
pub(crate) fn pattern_value(pattern: &Pattern) -> Value {
    let mut o = Map::new();
    match &pattern.source {
        PatternSource::Preset(id) => {
            o.insert("preset".into(), Value::from(*id));
        }
        PatternSource::Custom { name } => {
            o.insert("name".into(), Value::from(name.clone()));
            o.insert("width".into(), Value::from(pattern.tile.width));
            o.insert("height".into(), Value::from(pattern.tile.height));
            o.insert(
                "rgba".into(),
                Value::from(base64_encode(&pattern.tile.rgba)),
            );
        }
    }
    Value::Object(o)
}
