//! Indexed Color's colour table (#1 Indexed phase 1c, KERNEL): Adobe's
//! `.act` file, the table's JSON, and the Custom palette
//! (`designs/indexed-mode.md`). `indexed_table` is the adapter that
//! reads and replaces a document's table; `indexed_convert` builds the
//! Custom palette through [`custom`].
//!
//! # The `.act` layout
//!
//! Adobe Color Table: 256 RGB triples, 768 bytes, entry i at `3i`. A file of
//! 772 bytes adds two big-endian `u16`s: the count of entries in use at 768
//! (2..=256 here, the palette's own range) and the transparent index at 770
//! (`0xFFFF` for none, else an index below the count). A 768-byte file is a
//! full 256-entry table with no transparent index. Every other length is
//! refused. The writer always writes 772 bytes, the unused entries zero, so
//! the count and the transparent index survive the round trip.

use serde_json::{json, Map, Value};

use crate::indexed_palette::{Rgb, MAX_COLORS, MIN_COLORS};

/// The bytes of the 256 triples.
pub(crate) const ACT_TABLE_BYTES: usize = 768;

/// The bytes of a table with its count and transparent index.
pub(crate) const ACT_FILE_BYTES: usize = 772;

/// The transparent-index word meaning "none".
pub(crate) const ACT_NO_TRANSPARENT: u16 = 0xFFFF;

/// The table-size sentence (`ColorTable::new`, [`custom`], the `table`
/// option and `set_color_table` share it).
pub(crate) const TABLE_SIZE_SENTENCE: &str = "a colour table holds 2 to 256 colours";

/// The Custom palette's refusal when its table leaves no room for the
/// transparent entry.
pub(crate) const CUSTOM_FULL_SENTENCE: &str =
    "The Custom palette holds 256 colours, leaving no room for the transparent colour; turn \
     Transparency off or use 255 colours.";

/// An Indexed Color table: 2..=256 entries and an optional transparent index
/// below their count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColorTable {
    pub(crate) colors: Vec<Rgb>,
    pub(crate) transparent: Option<u8>,
}

impl ColorTable {
    pub(crate) fn new(colors: Vec<Rgb>, transparent: Option<u8>) -> Result<Self, String> {
        if !(MIN_COLORS..=MAX_COLORS).contains(&colors.len()) {
            return Err(TABLE_SIZE_SENTENCE.into());
        }
        if transparent.is_some_and(|t| usize::from(t) >= colors.len()) {
            return Err("the colour table's transparent index is outside the table".into());
        }
        Ok(Self {
            colors,
            transparent,
        })
    }
}

fn be16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

/// The table an `.act` file holds (module doc).
pub(crate) fn parse_act(bytes: &[u8]) -> Result<ColorTable, String> {
    let (count, transparent) = match bytes.len() {
        ACT_TABLE_BYTES => (MAX_COLORS, None),
        ACT_FILE_BYTES => {
            let count = usize::from(be16(bytes, ACT_TABLE_BYTES));
            if !(MIN_COLORS..=MAX_COLORS).contains(&count) {
                return Err(format!(
                    "the colour table file holds {count} colours; a colour table holds 2 to 256"
                ));
            }
            let transparent = match be16(bytes, ACT_TABLE_BYTES + 2) {
                ACT_NO_TRANSPARENT => None,
                t if usize::from(t) < count => Some(t as u8),
                t => {
                    return Err(format!(
                        "the colour table file's transparent index {t} is outside its {count} \
                         colours"
                    ))
                }
            };
            (count, transparent)
        }
        n => {
            return Err(format!(
                "a colour table file is {ACT_TABLE_BYTES} or {ACT_FILE_BYTES} bytes, not {n}"
            ))
        }
    };
    let colors = bytes[..count * 3]
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    ColorTable::new(colors, transparent)
}

/// The 772-byte `.act` of `table` (module doc).
pub(crate) fn encode_act(table: &ColorTable) -> Vec<u8> {
    let mut out = Vec::with_capacity(ACT_FILE_BYTES);
    for rgb in &table.colors {
        out.extend_from_slice(rgb);
    }
    out.resize(ACT_TABLE_BYTES, 0);
    out.extend_from_slice(&(table.colors.len() as u16).to_be_bytes());
    let transparent = table.transparent.map_or(ACT_NO_TRANSPARENT, u16::from);
    out.extend_from_slice(&transparent.to_be_bytes());
    out
}

/// One `[r, g, b]` of codes 0..=255.
fn rgb_of(value: &Value) -> Option<Rgb> {
    let items = value.as_array().filter(|items| items.len() == 3)?;
    let mut rgb = [0u8; 3];
    for (slot, item) in rgb.iter_mut().zip(items) {
        *slot = u8::try_from(item.as_u64()?).ok()?;
    }
    Some(rgb)
}

/// A JSON list of `[r, g, b]` entries, 2..=256 of them; `what` names it in
/// the shape error.
pub(crate) fn colors_from_json(value: &Value, what: &str) -> Result<Vec<Rgb>, String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("{what} must be a list of [r, g, b] codes 0 to 255"))?;
    let colors = items
        .iter()
        .map(rgb_of)
        .collect::<Option<Vec<Rgb>>>()
        .ok_or_else(|| format!("{what} must be a list of [r, g, b] codes 0 to 255"))?;
    if !(MIN_COLORS..=MAX_COLORS).contains(&colors.len()) {
        return Err(TABLE_SIZE_SENTENCE.into());
    }
    Ok(colors)
}

/// `{"colors":[[r,g,b],…],"transparent_index":n|null}` as a table; an
/// absent `transparent_index` is none, an unknown key refuses.
pub(crate) fn table_from_json(text: &str) -> Result<ColorTable, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| format!("the colour table is not valid JSON: {e}"))?;
    let object: &Map<String, Value> = value
        .as_object()
        .ok_or("the colour table must be a JSON object")?;
    if let Some(key) = object
        .keys()
        .find(|k| !matches!(k.as_str(), "colors" | "transparent_index"))
    {
        return Err(format!("the colour table has an unknown key \"{key}\""));
    }
    let colors = colors_from_json(
        object.get("colors").unwrap_or(&Value::Null),
        "the colour table's colors",
    )?;
    let transparent = match object.get("transparent_index") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|t| u8::try_from(t).ok())
                .ok_or("the colour table's transparent_index must be 0 to 255 or null")?,
        ),
    };
    ColorTable::new(colors, transparent)
}

/// `table` as the JSON [`table_from_json`] reads.
pub(crate) fn table_json(table: &ColorTable) -> Value {
    json!({
        "colors": table.colors,
        "transparent_index": table.transparent,
    })
}

/// The Custom palette: `table` as it is (2..=256 entries), the transparent
/// entry appended after it by the caller as for every other palette, so a
/// 256-entry table refuses when the image holds a transparent pixel.
pub(crate) fn custom(table: &[Rgb], transparent_pixel: bool) -> Result<Vec<Rgb>, String> {
    if !(MIN_COLORS..=MAX_COLORS).contains(&table.len()) {
        return Err(TABLE_SIZE_SENTENCE.into());
    }
    if transparent_pixel && table.len() == MAX_COLORS {
        return Err(CUSTOM_FULL_SENTENCE.into());
    }
    Ok(table.to_vec())
}
