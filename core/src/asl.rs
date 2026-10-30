//! Photoshop STYLE LIBRARIES (`.asl`): the reader the Styles
//! panel's Import Styles… and MCP `import_style_presets` share (its writer,
//! Export Styles…, is `asl_write`). Framing only:
//! every descriptor goes through `psd_descriptor`, every effect object
//! through `psd_layer_effects::read_effect_list` (so an effect reads exactly
//! as it does in a PSD's `lfx2`), and the pattern section through
//! `psd_patterns::read_table`.
//!
//! # The layout (big-endian)
//!
//! `u16` 2, `8BSL`, `u16` 3, `u32` the pattern section's length and that
//! many bytes (the PSD `Patt` block's records, `psd_patterns`), then `u32`
//! the style count (at most [`MAX_ASL_STYLES`]) and per style `u32` its
//! length and that many bytes, padded to a multiple of 4 from the file's
//! start. The length may count that padding (Photoshop's form, and
//! `asl_write`'s: the record ends in 0–3 zero bytes and nothing
//! follows it) or not (the earlier writer's, the pad outside the record);
//! both read. A style record is two version-16 descriptors back to back: the
//! IDENTITY (`null`: `Nm  ` the name, `Idnt` its id) and the STYLE (`Styl`:
//! `documentMode`, `Lefx` the effects, `blendOptions` the blending), read
//! with `obj ` references (`psd_descriptor::read_with_references`): a Blend
//! If entry names its channel by one.
//!
//! # A style
//!
//! - `Lefx` is an `lfx2`/`lmfx` body's descriptor: its effects are read
//!   LENIENTLY (`read_effect_list` with a `dropped` list: an effect Doctored
//!   does not model, Stroke Emboss for one, is skipped and named "<Title>:
//!   <reason>"), colours as sRGB (`FxColours::Rgb(None)`: a style library
//!   carries no document), patterns from the file's own section, no
//!   reference point (`psd_pattern_fill`: a preset keeps no anchor anyway).
//!   `Scl ` other than 100 scales the style as the PSD importer does;
//!   `masterFXSwitch` false drops every effect, each named; a `Lefx` key no
//!   reader knows is named "Layer effects: <key>".
//! - `blendOptions`: `Md  ` and `Opct` are the style's BLENDING (the layer
//!   mode and opacity a preset that includes blending options sets);
//!   `fillOpacity`, `knockout` and `blendInterior` join the style; a key
//!   holding Photoshop's default (`blendClipped` true,
//!   `transparencyShapesLayer` true, `layerMaskAsGlobalMask` and
//!   `vectorMaskAsGlobalMask` false, `channelRestrictions` of three
//!   channels) is not a loss and is not named; every other key is named
//!   "Blending Options: <key>". `Blnd` is Blend If: a list of entries, each
//!   `Chnl` (a reference whose `Enmr` is `Gry `, `Rd  `, `Grn ` or `Bl  `,
//!   [`BLND_CHANNELS`]) and the two ramps as longs, `this_layer` =
//!   `[SrcB, Srcl, SrcW, Srcm]` and `underlying` = `[DstB, Dstl, DstW,
//!   Dstt]` ([`BLND_RAMPS`], settled by a Photoshop-authored library;
//!   Photoshop writes only the restricted entries). Entries at the full ramp `[0, 0, 255, 255]` are skipped; EVERY
//!   other entry becomes one range of the style's `blend_if`, canonicalized
//!   by `BlendIf::from_ranges` (several restricted channels give `extra`).
//!   A channel named twice, another channel, or a ramp out of order is named
//!   "Blending Options: Blnd".
//! - The result is parsed strictly and canonicalized as every preset is
//!   (`style_presets::preset_style`); a style that parses to an identity is
//!   `None`, as is one that does not parse (its reason named "Style:
//!   <reason>"), and the host skips it.
//!
//! A file that is not this layout, or is truncated anywhere, is `Err` with a
//! lowercase sentence; nothing panics.

use serde_json::{json, Map, Value};

use crate::blend::BlendMode;
use crate::psd_descriptor::{self, Descriptor, Item};
use crate::psd_layer_effects::{effect_title, read_effect_list, FxColours, PatternContext};
use crate::psd_layer_effects_fx as fx;
use crate::psd_patterns::{read_table, PatternTable};
use crate::style::{
    blend_mode_from_name, blend_mode_name, BlendIf, BlendIfRange, LayerStyle, Strictness,
};
use crate::style_names::{enum_name, CHANNELS};
use crate::style_presets::preset_style;

/// The largest file the reader accepts: the host refuses a bigger one
/// before reading it (`dr_asl_decode`). 64 MiB holds a thousand styles each
/// with a full-size custom pattern.
pub(crate) const MAX_ASL_BYTES: usize = 64 << 20;

/// The most styles one library may declare.
pub(crate) const MAX_ASL_STYLES: usize = 1000;

const NOT_ASL: &str = "file is not a Photoshop style library (.asl)";

/// The `Chnl` enum ids of a `Blnd` entry and the Blend If channel names they
/// map to (module doc), shared with `asl_write`.
pub(crate) const BLND_CHANNELS: [(&[u8], &str); 4] = [
    (b"Gry ", "gray"),
    (b"Rd  ", "red"),
    (b"Grn ", "green"),
    (b"Bl  ", "blue"),
];

/// A `Blnd` entry's `this_layer` keys, then its `underlying` keys, each in
/// ramp order `[black min, black max, white min, white max]` (module doc),
/// shared with `asl_write`. The white pair is `SrcW` (`srcWhiteMin`) then
/// `Srcm` (`srcWhiteMax`), and `DstW` then `Dstt`: Photoshop 26.10's own
/// `.asl` (the `styles_asl_take.py` take, run
/// rz-557-asl-20261017T030137Z-3a9be5b2) holds S9's ranges only under this
/// order.
pub(crate) const BLND_RAMPS: [[&[u8]; 4]; 2] = [
    [b"SrcB", b"Srcl", b"SrcW", b"Srcm"],
    [b"DstB", b"Dstl", b"DstW", b"Dstt"],
];
const SHORT: &str = "style library is shorter than its layout";
const NO_NAME: &str = "a style in the library has no readable name";

/// One style of a library.
pub(crate) struct AslStyle {
    pub name: String,
    /// `None` when nothing Doctored models is left (module doc).
    pub style: Option<LayerStyle>,
    /// The layer mode and opacity (0..=1) of a style saved with its blending
    /// options.
    pub blending: Option<(BlendMode, f32)>,
    pub dropped: Vec<String>,
}

/// The refusal for a file of `len` bytes past [`MAX_ASL_BYTES`].
pub(crate) fn too_large(len: usize) -> String {
    format!("style library is {len} bytes, over the 64 MiB limit")
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or(SHORT)?;
        let out = self.bytes.get(self.at..end).ok_or(SHORT)?;
        self.at = end;
        Ok(out)
    }
    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Every style of the library `bytes` (module doc).
pub(crate) fn read(bytes: &[u8]) -> Result<Vec<AslStyle>, String> {
    if bytes.len() > MAX_ASL_BYTES {
        return Err(too_large(bytes.len()));
    }
    let mut c = Cursor { bytes, at: 0 };
    if c.u16().map_err(|_| NOT_ASL)? != 2 || c.take(4).map_err(|_| NOT_ASL)? != b"8BSL" {
        return Err(NOT_ASL.to_string());
    }
    if c.u16()? != 3 {
        return Err(NOT_ASL.to_string());
    }
    let pattern_len = c.u32()? as usize;
    let patterns = read_table(c.take(pattern_len)?);
    let count = c.u32()? as usize;
    if count > MAX_ASL_STYLES {
        return Err(format!(
            "style library declares {count} styles, more than the {MAX_ASL_STYLES} it may hold"
        ));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let length = c.u32()? as usize;
        let record = c.take(length)?;
        let pad = (4 - c.at % 4) % 4;
        // The last record's padding may be missing at the end of the file.
        c.at = (c.at + pad).min(bytes.len());
        out.push(style_record(record, &patterns)?);
    }
    Ok(out)
}

/// One style record: the identity descriptor, then the style's.
fn style_record(record: &[u8], patterns: &PatternTable) -> Result<AslStyle, String> {
    let (identity, used) = psd_descriptor::read_prefix(record)
        .map_err(|reason| format!("a style's identity does not read: {reason}"))?;
    let name = identity
        .items
        .iter()
        .find_map(|(key, item)| match (key.as_slice(), item) {
            (b"Nm  ", Item::Text(text)) => Some(text.clone()),
            _ => None,
        })
        .ok_or(NO_NAME)?;
    let mut dropped = Vec::new();
    let styl = match psd_descriptor::read_with_references(&record[used..]) {
        Ok(styl) => styl,
        Err(reason) => {
            dropped.push(format!("Style: {reason}"));
            return Ok(AslStyle {
                name,
                style: None,
                blending: None,
                dropped,
            });
        }
    };
    let (style, blending) = style_of(&styl, patterns, &mut dropped);
    Ok(AslStyle {
        name,
        style,
        blending,
        dropped,
    })
}

/// The `Lefx` keys a reader knows (module doc).
fn known_effect_key(key: &[u8]) -> bool {
    const OTHERS: [&[u8]; 10] = [
        b"Scl ",
        b"masterFXSwitch",
        b"numModifyingFX",
        b"patternFill",
        b"dropShadowMulti",
        b"innerShadowMulti",
        b"frameFXMulti",
        b"solidFillMulti",
        b"gradientFillMulti",
        b"null",
    ];
    fx::CLASSES.iter().any(|(k, _)| k.as_slice() == key) || OTHERS.contains(&key)
}

/// Whether a `blendOptions` item holds Photoshop's default (module doc).
fn default_blending(key: &[u8], item: &Item) -> bool {
    match (key, item) {
        (b"blendClipped" | b"transparencyShapesLayer", Item::Bool(true)) => true,
        (b"layerMaskAsGlobalMask" | b"vectorMaskAsGlobalMask", Item::Bool(false)) => true,
        (b"channelRestrictions", Item::List(list)) => list.len() == 3,
        _ => false,
    }
}

/// The knockout a `knockout` enum id names.
fn knockout_name(id: &[u8]) -> Option<&'static str> {
    match id {
        b"none" | b"knockoutNone" | b"None" => Some("none"),
        b"shallow" | b"knockoutShallow" => Some("shallow"),
        b"deep" | b"knockoutDeep" => Some("deep"),
        _ => None,
    }
}

fn key_text(key: &[u8]) -> String {
    String::from_utf8_lossy(key).trim_end().to_string()
}

/// The blending options: the style JSON keys they add and the layer mode
/// and opacity (module doc).
fn blend_options(
    b: &Descriptor,
    root: &mut Map<String, Value>,
    dropped: &mut Vec<String>,
) -> Option<(BlendMode, f32)> {
    let mut mode = None;
    let mut opacity = None;
    for (key, item) in &b.items {
        let mapped = match key.as_slice() {
            b"Md  " => fx::blend(b, key)
                .ok()
                .flatten()
                .and_then(blend_mode_from_name)
                .map(|m| mode = Some(m))
                .is_some(),
            b"Opct" => fx::fraction(b, key)
                .ok()
                .flatten()
                .filter(|v| v.is_finite())
                .map(|v| opacity = Some(v.clamp(0.0, 1.0) as f32))
                .is_some(),
            b"fillOpacity" => fx::fraction(b, key)
                .ok()
                .flatten()
                .filter(|v| v.is_finite())
                .map(|v| root.insert("fill_opacity".into(), json!(v.clamp(0.0, 1.0))))
                .is_some(),
            b"knockout" => b
                .enumeration(key)
                .ok()
                .flatten()
                .and_then(knockout_name)
                .map(|k| root.insert("knockout".into(), json!(k)))
                .is_some(),
            b"blendInterior" => b
                .bool(key)
                .ok()
                .flatten()
                .map(|on| root.insert("blend_interior_as_group".into(), json!(on)))
                .is_some(),
            b"Blnd" => blend_if(b, root),
            _ => default_blending(key, item),
        };
        if !mapped {
            dropped.push(format!("Blending Options: {}", key_text(key)));
        }
    }
    match (mode, opacity) {
        (None, None) => None,
        (mode, opacity) => Some((mode.unwrap_or(BlendMode::Normal), opacity.unwrap_or(1.0))),
    }
}

/// `Blnd` as the style's `blend_if` (module doc); false when it does not map.
fn blend_if(b: &Descriptor, root: &mut Map<String, Value>) -> bool {
    let Ok(Some(entries)) = b.list(b"Blnd") else {
        return false;
    };
    let mut found = Vec::new();
    for entry in entries {
        let Item::Object(entry) = entry else {
            return false;
        };
        let mut ramps = [[0u8; 4]; 2];
        for (ramp, keys) in ramps.iter_mut().zip(BLND_RAMPS) {
            for (slot, key) in ramp.iter_mut().zip(keys) {
                match entry.long(key) {
                    Ok(Some(v)) if (0..=255).contains(&v) => *slot = v as u8,
                    _ => return false,
                }
            }
        }
        if ramps == [crate::style::FULL_RAMP; 2] {
            continue;
        }
        let channel = entry
            .enumerated_reference(b"Chnl")
            .ok()
            .flatten()
            .and_then(|id| BLND_CHANNELS.iter().find(|(c, _)| *c == id))
            .and_then(|(_, name)| CHANNELS.iter().find(|(n, _)| n == name))
            .map(|(_, channel)| *channel);
        let ordered = ramps
            .iter()
            .all(|r| r[0] <= r[1] && r[1] <= r[2] && r[2] <= r[3]);
        match (channel, ordered) {
            (Some(channel), true) => found.push(BlendIfRange {
                channel,
                this_layer: ramps[0],
                underlying: ramps[1],
            }),
            _ => return false,
        }
    }
    let Some((first, rest)) = found.split_first() else {
        return true;
    };
    // The one canonicalizer: a channel named twice refuses the key.
    let Ok(blend_if) = BlendIf::from_ranges(*first, rest) else {
        return false;
    };
    let range = |r: BlendIfRange| {
        json!({"channel": enum_name(&CHANNELS, r.channel),
               "this_layer": r.this_layer, "underlying": r.underlying})
    };
    let mut value = range(blend_if.primary());
    if blend_if.is_multichannel() {
        let extra: Vec<Value> = blend_if.extra.iter().flatten().map(|r| range(*r)).collect();
        value["extra"] = Value::Array(extra);
    }
    root.insert("blend_if".into(), value);
    true
}

/// A `Styl` descriptor as a preset's style and blending (module doc).
fn style_of(
    styl: &Descriptor,
    patterns: &PatternTable,
    dropped: &mut Vec<String>,
) -> (Option<LayerStyle>, Option<(BlendMode, f32)>) {
    let mut root = Map::new();
    let blending = match styl.object(b"blendOptions") {
        Ok(Some(b)) => blend_options(b, &mut root, dropped),
        Ok(None) => None,
        Err(reason) => {
            dropped.push(format!("Blending Options: {reason}"));
            None
        }
    };
    let pattern = PatternContext {
        patterns,
        reference: None,
        top_left: (0, 0),
    };
    let mut scale = None;
    match styl.object(b"Lefx") {
        Ok(Some(lefx)) => {
            for (key, _) in &lefx.items {
                if !known_effect_key(key) {
                    dropped.push(format!("Layer effects: {}", key_text(key)));
                }
            }
            let mut read = Vec::new();
            let effects = read_effect_list(lefx, FxColours::Rgb(None), &pattern, Some(&mut read))
                .unwrap_or_default();
            if lefx.bool(b"masterFXSwitch").ok().flatten() == Some(false) {
                for effect in &effects {
                    let name = effect.get("type").and_then(Value::as_str).unwrap_or("");
                    dropped.push(format!(
                        "{}: layer effects are switched off",
                        effect_title(name)
                    ));
                }
            } else {
                root.insert("effects".into(), Value::Array(effects));
                scale = lefx.unit(b"Scl ").ok().flatten();
            }
            dropped.extend(read);
        }
        Ok(None) => {}
        Err(reason) => dropped.push(format!("Layer effects: {reason}")),
    }
    let parsed =
        LayerStyle::from_value(&Value::Object(root), Strictness::Strict).map(|style| match scale {
            Some(s) if s.is_finite() && s > 0.0 && (s - 100.0).abs() > f64::EPSILON => {
                style.scaled(s / 100.0)
            }
            _ => style,
        });
    let style = match parsed.and_then(|style| preset_style(&style.to_json())) {
        Ok(style) => Some(style).filter(|s| !s.is_identity()),
        Err(reason) => {
            dropped.push(format!("Style: {reason}"));
            None
        }
    };
    (style, blending)
}

/// `{"styles": [{"name", "style": object | null, "blending": {"mode",
/// "opacity"} | null, "dropped": [sentence]}]}` (`dr_asl_decode`).
pub(crate) fn report_json(styles: &[AslStyle]) -> String {
    let list: Vec<Value> = styles
        .iter()
        .map(|s| {
            let style = s
                .style
                .as_ref()
                .and_then(|style| serde_json::from_str::<Value>(&style.to_json()).ok())
                .unwrap_or(Value::Null);
            let blending = s.blending.map_or(Value::Null, |(mode, opacity)| {
                json!({"mode": blend_mode_name(mode),
                       "opacity": crate::style_json_write::number(opacity)})
            });
            json!({"name": s.name, "style": style, "blending": blending, "dropped": s.dropped})
        })
        .collect();
    json!({ "styles": list }).to_string()
}
