//! Photoshop's `patternFill` layer effect and its `fxrp` reference point:
//! the Pattern Overlay's PSD form in both directions, the same pattern fill
//! inside a Pattern Stroke's `FrFX`, and a Bevel & Emboss texture inside its
//! `ebbl`. The pattern raster itself travels in the `Patt` table
//! (`psd_patterns`); the effect names it by `Nm  ` and `Idnt`.
//!
//! `patternFill` = Objc `patternFill` {`enab`, `Md  ` enum `BlnM`, `Opct`
//! `#Prc`, `Ptrn` Objc `Ptrn` {`Nm  ` TEXT, `Idnt` TEXT}, `Angl` `#Ang` = the
//! angle (read from `UntF` or `doub`, the same sign as the JSON's `angle`),
//! `Scl ` `#Prc` scale × 100, `Algn` bool = Link with Layer, `phase` Objc
//! `Pnt ` {`Hrzn`, `Vrtc` doub}, `present`/`showInDialog` true}. It is one
//! single key in both `lfx2` and `lmfx` (not a repeating kind).
//!
//! A Pattern Stroke's `FrFX` (single or in `frameFXMulti`) is `PntT` enum
//! `FrFl` `Ptrn` followed by the SAME fill items ([`fill_items`] /
//! [`read_fill`]) with one difference: Link with Layer is `Lnkd` (bool,
//! absent = linked), not `Algn` — `stroke_fill_take.py`'s take1 readback,
//! where `Angl` is also present on every pattern stroke. Its tile rides in
//! the same `Patt`/`Pat2` table, and its linked anchor follows the same
//! `fxrp` rule.
//!
//! A bevel's texture ([`texture_items`] / [`read_texture`]) is `ebbl`'s
//! `useTexture` bool ↔ `texture_enabled`, then, when it is on, the fill
//! items with `Algn` as the link key (absent = linked) and no `Angl` (the
//! texture has none), with `textureDepth` `#Prc` (depth × 100) after `Scl `
//! and `InvT` bool ↔ `invert`. The texture is read whenever `Ptrn` is
//! present: a pattern the file does not carry declines with
//! [`TEXTURE_MISSING`] when `useTexture` is true and drops the texture when
//! it is false. Its tile rides in `Patt`/`Pat2`, and its linked anchor and
//! the record's `fxrp` follow the overlay's rule.
//!
//! `fxrp` = two big-endian f64, x then y: the record's point, moved by its
//! translations since the layer was MADE (take2 measured Photoshop writing it
//! on every record: P0 (0, 0), P2 and P5 (7, 3)) and mapped by Image Size,
//! Canvas Size, Crop, the angled crop, turns, flips and Free Transform
//! (`reference_point`; Photoshop keeps the fraction), the place its linked
//! tile follows; a turn's or a flip's turned tile is a new `Patt` entry under
//! the same name. Reading
//! sets every record's reference point (`Layer::reference`, rounded and
//! clamped; (0, 0) without one, `psd_group_records`), and a linked overlay's
//! anchor = the record's top left − round(fxrp), which renders Photoshop's
//! files within 0.004 codes (READ); Photoshop reads ours back the same
//! (take2's inspect). A linked overlay in a record without `fxrp` takes
//! anchor = top left. Writing ([`record_reference`]): beside a linked
//! placed tile (the first in effect order: an overlay, a Pattern stroke or an
//! enabled bevel texture, `style_pattern_fill::placed_fill`) offset −
//! anchor (MOVE), else the record's reference when it moved, on
//! every raster and group record of every mode, so an unstyled moved layer
//! keeps its point through a round trip; a record that never moved writes
//! none, so its bytes are the prior writer's.
//!
//! A Pattern Fill LAYER's `PtFl` record (`psd_fill_layer`)
//! holds the same fill keys ([`fill_items`] / [`read_fill`]) in its own
//! descriptor, its tile in the same table.
//!
//! Import declines, each naming its reason: a pattern the file does not
//! carry (the overlay's [`MISSING`], the stroke's [`STROKE_MISSING`], an
//! enabled texture's [`TEXTURE_MISSING`]), and a pattern whose tile
//! `psd_patterns` could not read.

use serde_json::{json, Map, Value};

use crate::psd_descriptor::{Descriptor, Item};
use crate::psd_layer_effects::Block;
use crate::psd_layer_effects_fx as fx;
use crate::psd_patterns::{pattern_id, PatternTable};
use crate::style::{BevelTexture, LayerStyle, PatternFill, PatternOverlay};
use crate::style_pattern_fill::{pattern_value, placed_fill};

const MISSING: &str = "pattern overlay names a pattern the file does not carry";

/// A Pattern Stroke's missing-pattern decline (module doc).
pub(crate) const STROKE_MISSING: &str = "stroke names a pattern the file does not carry";

/// An enabled bevel texture's missing-pattern decline (module doc).
pub(crate) const TEXTURE_MISSING: &str = "bevel texture names a pattern the file does not carry";

/// A TEXT item's string.
fn text<'a>(d: &'a Descriptor, key: &[u8]) -> Option<&'a str> {
    d.items.iter().find_map(|(k, item)| match item {
        Item::Text(s) if k == key => Some(s.as_str()),
        _ => None,
    })
}

/// A number stored as `doub` or `UntF`.
fn number(d: &Descriptor, key: &[u8]) -> Result<Option<f64>, String> {
    match d.double(key) {
        Ok(Some(v)) => Ok(Some(v)),
        Ok(None) => Ok(None),
        Err(_) => d.unit(key),
    }
}

/// The `patternFill` object as `pattern_overlay` style JSON (module doc).
/// `reference` is the record's `fxrp`, `top_left` its rectangle's corner.
pub(crate) fn read(
    d: &Descriptor,
    patterns: &PatternTable,
    reference: Option<(f64, f64)>,
    top_left: (i32, i32),
) -> Result<Value, String> {
    let mut o = Map::new();
    o.insert("type".into(), json!("pattern_overlay"));
    fx::put(&mut o, "enabled", d.bool(b"enab")?);
    fx::put(&mut o, "blend", fx::blend(d, b"Md  ")?);
    fx::put(&mut o, "opacity", fx::fraction(d, b"Opct")?);
    o.extend(read_fill(
        d, b"Algn", patterns, reference, top_left, MISSING,
    )?);
    Ok(Value::Object(o))
}

/// A pattern fill's keys read from `d` (module doc) as `style_pattern_fill`
/// JSON: `Ptrn` resolved through `patterns` (`missing` when the file does
/// not carry it), `Angl`, `Scl `, `link_key` (`Algn` on an overlay, `Lnkd`
/// on a stroke; absent = linked) and `phase`, and a linked fill's anchor =
/// `top_left` − round(`reference`).
pub(crate) fn read_fill(
    d: &Descriptor,
    link_key: &[u8; 4],
    patterns: &PatternTable,
    reference: Option<(f64, f64)>,
    top_left: (i32, i32),
    missing: &str,
) -> Result<Map<String, Value>, String> {
    let mut o = Map::new();
    // Canonicalized into -180..180 by the style parser (`style_json`).
    if let Some(angle) = number(d, b"Angl")?.filter(|a| a.is_finite()) {
        o.insert("angle".into(), json!(angle));
    }
    let reference_pattern = d.object(b"Ptrn")?.ok_or(missing)?;
    let name = text(reference_pattern, b"Nm  ").unwrap_or("");
    let id = text(reference_pattern, b"Idnt").unwrap_or("");
    let (_, stored_name, tile) = patterns.find(id, name).ok_or(missing)?;
    let tile = tile.as_ref().map_err(Clone::clone)?;
    // A preset Doctored wrote comes back as that preset.
    let preset = crate::pattern_tile::PRESETS.iter().find_map(|(p, _)| {
        crate::style::Pattern::preset(p)
            .filter(|pattern| pattern_id(pattern) == id && *pattern.tile == *tile)
    });
    let pattern = preset.unwrap_or_else(|| crate::style::Pattern {
        source: crate::style::PatternSource::Custom {
            name: if name.is_empty() {
                stored_name.clone()
            } else {
                name.to_string()
            },
        },
        tile: std::sync::Arc::new(tile.clone()),
    });
    o.insert("pattern".into(), pattern_value(&pattern));
    fx::put(&mut o, "scale", fx::fraction(d, b"Scl ")?);
    let linked = d.bool(link_key)?.unwrap_or(true);
    o.insert("link_with_layer".into(), json!(linked));
    if let Some(phase) = d.object(b"phase")? {
        let x = number(phase, b"Hrzn")?.unwrap_or(0.0);
        let y = number(phase, b"Vrtc")?.unwrap_or(0.0);
        o.insert("phase".into(), json!([x, y]));
    }
    if linked {
        let (mx, my) = reference
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .map_or((0, 0), |(x, y)| (x.round() as i64, y.round() as i64));
        let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX));
        o.insert(
            "anchor".into(),
            json!([
                clamp(i64::from(top_left.0) - mx),
                clamp(i64::from(top_left.1) - my)
            ]),
        );
    }
    Ok(o)
}

/// A bevel's `ebbl` texture keys (module doc) as its `texture` JSON object:
/// `None` when `Ptrn` is absent, or when `useTexture` is false and any part
/// of the texture fails to read; [`TEXTURE_MISSING`] when the pattern is
/// missing and `useTexture` is true. `reference` and `top_left` as [`read_fill`].
pub(crate) fn read_texture(
    d: &Descriptor,
    patterns: &PatternTable,
    reference: Option<(f64, f64)>,
    top_left: (i32, i32),
) -> Result<Option<Map<String, Value>>, String> {
    if d.object(b"Ptrn")?.is_none() {
        return Ok(None);
    }
    let used = d.bool(b"useTexture")?.unwrap_or(false);
    let mut o = match read_fill(d, b"Algn", patterns, reference, top_left, TEXTURE_MISSING) {
        Ok(o) => o,
        // A switched-off texture never renders, so any unreadable part of it
        // (a missing or unsupported tile, a malformed key) drops the texture
        // rather than declining the whole style.
        Err(_) if !used => return Ok(None),
        Err(reason) => return Err(reason),
    };
    o.remove("angle");
    fx::put(&mut o, "depth", fx::fraction(d, b"textureDepth")?);
    fx::put(&mut o, "invert", d.bool(b"InvT")?);
    Ok(Some(o))
}

/// A bevel texture's items after `useTexture` (module doc): [`fill_items`]
/// with `Algn` and no `Angl`, `textureDepth` and `InvT` after `Scl `.
pub(crate) fn texture_items(t: &BevelTexture) -> Vec<(Vec<u8>, Item)> {
    let mut items = Vec::new();
    for (key, item) in fill_items(&t.fill, b"Algn") {
        match key.as_slice() {
            b"Angl" => {}
            b"Scl " => {
                items.push((key, item));
                items.push((b"textureDepth".to_vec(), fx::percent(t.depth)));
                items.push((b"InvT".to_vec(), Item::Bool(t.invert)));
            }
            _ => items.push((key, item)),
        }
    }
    items
}

/// The overlay as its `lfx2`/`lmfx` item (module doc).
pub(crate) fn write(p: &PatternOverlay) -> (Vec<u8>, Item) {
    let mut items = fx::head(p.enabled, p.blend, p.opacity);
    items.extend(fill_items(&p.fill, b"Algn"));
    fx::tail(&mut items);
    (
        b"patternFill".to_vec(),
        Item::Object(Descriptor {
            class: b"patternFill".to_vec(),
            items,
        }),
    )
}

/// A pattern fill's items (module doc): `Ptrn` {`Nm  `, `Idnt`}, `Angl`,
/// `Scl `, `link_key` (`Algn` on an overlay, `Lnkd` on a stroke) and
/// `phase`.
pub(crate) fn fill_items(fill: &PatternFill, link_key: &[u8; 4]) -> Vec<(Vec<u8>, Item)> {
    vec![
        (
            b"Ptrn".to_vec(),
            Item::Object(Descriptor {
                class: b"Ptrn".to_vec(),
                items: vec![
                    (b"Nm  ".to_vec(), Item::Text(fill.pattern.name())),
                    (b"Idnt".to_vec(), Item::Text(pattern_id(&fill.pattern))),
                ],
            }),
        ),
        (
            b"Angl".to_vec(),
            Item::Unit(*b"#Ang", f64::from(fill.angle)),
        ),
        (b"Scl ".to_vec(), fx::percent(fill.scale)),
        (link_key.to_vec(), Item::Bool(fill.link_with_layer)),
        (
            b"phase".to_vec(),
            Item::Object(Descriptor {
                class: b"Pnt ".to_vec(),
                items: vec![
                    (b"Hrzn".to_vec(), Item::Double(f64::from(fill.phase[0]))),
                    (b"Vrtc".to_vec(), Item::Double(f64::from(fill.phase[1]))),
                ],
            }),
        ),
    ]
}

/// The record's `fxrp` body (module doc): when `style` holds a LINKED
/// placed tile (the first in effect order, an overlay's, a Pattern
/// stroke's or an enabled bevel texture's), offset − anchor (the move its
/// tile follows); else, when the
/// record moved since it was made, its `reference`; else `None`.
pub(crate) fn record_reference(
    style: Option<&LayerStyle>,
    offset: (i32, i32),
    reference: (i32, i32),
) -> Option<Block> {
    let linked = style.and_then(|style| {
        style
            .effects
            .iter()
            .filter_map(placed_fill)
            .find(|fill| fill.link_with_layer)
    });
    let (x, y) = match linked {
        Some(p) => {
            let anchor = p.anchor.unwrap_or([offset.0, offset.1]);
            (
                f64::from(offset.0) - f64::from(anchor[0]),
                f64::from(offset.1) - f64::from(anchor[1]),
            )
        }
        None if reference != (0, 0) => (f64::from(reference.0), f64::from(reference.1)),
        None => return None,
    };
    let mut body = Vec::with_capacity(16);
    body.extend_from_slice(&x.to_be_bytes());
    body.extend_from_slice(&y.to_be_bytes());
    Some((*b"fxrp", body))
}

/// A record's `fxrp` body as (x, y); `None` unless it is two f64.
pub(crate) fn reference_of(body: &[u8]) -> Option<(f64, f64)> {
    let x = f64::from_be_bytes(body.get(0..8)?.try_into().ok()?);
    let y = f64::from_be_bytes(body.get(8..16)?.try_into().ok()?);
    Some((x, y))
}
