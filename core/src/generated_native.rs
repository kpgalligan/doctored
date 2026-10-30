//! Conditional `GVAR` table (M3b phase 2): a document's Generative
//! Layer VARIATIONS in `.rz`, at both depths, with one framing
//! (`path_native`'s precedent).
//!
//! Word [`WORD`] 92 is a conditional word inside the identity wrapper 45,
//! after 93 and ahead of 91 (`45 -> (93) -> (92) -> (91) -> (90) -> (89) ->
//! …`), and in a typed file the word after the CMYK slot inside 46 in the
//! same place (`46 -> [CMYK rung] -> (93) -> (92) -> (91) -> …`), a CMYK
//! document's too: as with 93, 91 and 90 there is no CMYK rung for it, and
//! a CMYK entry's `ink` is 1. Byte `GVAR` follows `PATH` and precedes `LREF`,
//! the last table before `LIDS`; typed it follows `PATH` and precedes `LREF`
//! and the `CMYK` table. A document holding no live
//! variations (`doc_gen_variations`) writes neither the word nor the table,
//! so its bytes stay exactly the prior writer's.
//!
//! Wire format, little-endian: `GVAR`, u32 schema = 1, u32 entry count, then
//! per entry u32 layer index (byte: the layer index; typed: the bottom-first
//! record index `vector_mask_table` uses), u32 raster w, h, the covered rect
//! and the crop (four u32 each, x, y, w, h), u32 native w, h, u8 ink, u32
//! selected, u32 count, then per variation u32 w, h, u32 tag length and the
//! UTF-8 tag, u32 PNG length and the PNG (straight RGBA8).
//!
//! The reader checks every entry before any layer pixel decodes: the index
//! names a raster of a generated tag whose dimensions are `raster`, masked by
//! a `raster`-sized plane at (0, 0) — the live rule, so a reopened entry is
//! live; 1 ≤ count ≤ 24; selected < count; each PNG's IHDR equals its size
//! inside `gen_fill::RESULT_SIDES`; each tag passes `doc_gen_fill::check_tag`;
//! the caps (`gen_variations::check_caps`, and the document's 1 GiB); no
//! index repeats; `ink` matches the document's model. A word with no table
//! refuses as [`INVALID_TABLE`] and a table with no entry as
//! [`INVALID_WORD`]; an entry's own fault answers "invalid native generative
//! variations table: {reason}". The word has no "requires" sentence: a file
//! stripped of 92 meets `GVAR` where the next table's tag should be and
//! refuses there. An older build refuses 92 by its number.

use std::collections::HashSet;
use std::sync::Arc;

use crate::artwork_document::MAX_DESCRIPTION;
use crate::doc_gen_fill::check_tag;
use crate::drdc::Reader;
use crate::gen_fill::{check_native, RESULT_SIDES};
use crate::gen_variations::{
    check_caps, check_document, png_size, GeneratedVariations, Variation, MAX_VARIATIONS,
    MAX_VARIATION_PNG_BYTES,
};

/// The conditional `.rz` word at both depths, CMYK included (module doc).
pub(crate) const WORD: u32 = 92;
/// The word with no table where its table belongs.
pub(crate) const INVALID_TABLE: &str = "invalid native generative variations table tag or schema";
/// The word over a table holding no entry.
pub(crate) const INVALID_WORD: &str = "invalid native generative variations word";

/// The tag, the schema and the entry count.
const FRAMING_BYTES: u64 = 12;
/// One entry's fixed fields.
const ENTRY_BYTES: u64 = 61;
/// One variation's size and its two lengths.
const VARIATION_BYTES: u64 = 16;

/// One live entry the writer stores: its layer index and its table.
pub(crate) type Entry = (usize, Arc<GeneratedVariations>);

/// What a layer offers the reader's checks: its raster's dimensions and
/// whether it holds a generated tag and a mask the live rule admits.
#[derive(Clone, Copy)]
pub(crate) struct Candidate {
    pub(crate) raster: (u32, u32),
    pub(crate) tagged: bool,
    pub(crate) mask: Option<((u32, u32), (i32, i32))>,
}

/// The table's encoded size; 0 when there is nothing to write.
pub(crate) fn encoded_len(entries: &[Entry]) -> u64 {
    if entries.is_empty() {
        return 0;
    }
    entries.iter().fold(FRAMING_BYTES, |n, (_, table)| {
        table.items.iter().fold(n + ENTRY_BYTES, |n, v| {
            n + VARIATION_BYTES + v.tag.len() as u64 + v.png.len() as u64
        })
    })
}

/// The table, or an empty Vec when there is nothing to write. It re-checks
/// the reader's caps so the writer never emits a file its reader refuses.
pub(crate) fn encode(entries: &[Entry]) -> Result<Vec<u8>, String> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let mut total = 0u64;
    for (_, table) in entries {
        let lengths: Vec<u64> = table.items.iter().map(|v| v.png.len() as u64).collect();
        check_caps(&lengths)?;
        if table.items.is_empty()
            || table.selected >= table.items.len()
            || table.items.iter().any(|v| v.tag.len() > MAX_DESCRIPTION)
        {
            return Err("a generative layer's variations are not a valid table".into());
        }
        total = total.saturating_add(table.png_bytes());
    }
    check_document(total)?;
    let mut out = crate::drdc_save_plan::reserved(encoded_len(entries))?;
    out.extend_from_slice(b"GVAR");
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (layer, table) in entries {
        let words = [
            *layer as u32,
            table.raster.0,
            table.raster.1,
            table.rect[0],
            table.rect[1],
            table.rect[2],
            table.rect[3],
            table.crop[0],
            table.crop[1],
            table.crop[2],
            table.crop[3],
            table.native.0,
            table.native.1,
        ];
        for word in words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.push(u8::from(table.ink));
        out.extend_from_slice(&(table.selected as u32).to_le_bytes());
        out.extend_from_slice(&(table.items.len() as u32).to_le_bytes());
        for v in table.items.iter() {
            out.extend_from_slice(&v.size.0.to_le_bytes());
            out.extend_from_slice(&v.size.1.to_le_bytes());
            out.extend_from_slice(&(v.tag.len() as u32).to_le_bytes());
            out.extend_from_slice(v.tag.as_bytes());
            out.extend_from_slice(&(v.png.len() as u32).to_le_bytes());
            out.extend_from_slice(&v.png);
        }
    }
    Ok(out)
}

fn invalid(reason: &str) -> String {
    format!("invalid native generative variations table: {reason}")
}

/// `inner` lies inside a `raster`-sized plane and is not empty.
fn inside(inner: [u32; 4], raster: (u32, u32)) -> bool {
    inner[2] > 0
        && inner[3] > 0
        && u64::from(inner[0]) + u64::from(inner[2]) <= u64::from(raster.0)
        && u64::from(inner[1]) + u64::from(inner[3]) <= u64::from(raster.1)
}

/// Reads the table of a document of `layer_count` records whose model is
/// CMYK when `ink`; `candidate` answers what record `i` offers. Copies each
/// picture once; decodes none.
pub(crate) fn parse(
    r: &mut Reader<'_>,
    layer_count: usize,
    ink: bool,
    candidate: impl Fn(usize) -> Option<Candidate>,
) -> Result<Vec<(usize, GeneratedVariations)>, String> {
    if r.take(4)? != b"GVAR" || r.u32()? != 1 {
        return Err(INVALID_TABLE.into());
    }
    let count = r.u32()? as usize;
    if count == 0 {
        return Err(INVALID_WORD.into());
    }
    if count > layer_count {
        return Err(invalid("more entries than layers"));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(count);
    let mut total = 0u64;
    for _ in 0..count {
        let layer = r.u32()? as usize;
        if !seen.insert(layer) {
            return Err(invalid("a layer is listed twice"));
        }
        let mut word = || r.u32();
        let raster = (word()?, word()?);
        let rect = [word()?, word()?, word()?, word()?];
        let crop = [word()?, word()?, word()?, word()?];
        let native = (word()?, word()?);
        let entry_ink = r.u8()? != 0;
        let selected = r.u32()? as usize;
        let items = r.u32()? as usize;
        let offered = (layer < layer_count)
            .then(|| candidate(layer))
            .flatten()
            .ok_or_else(|| invalid("an entry names no generated raster"))?;
        if !offered.tagged || offered.raster != raster || offered.mask != Some((raster, (0, 0))) {
            return Err(invalid("an entry names no generated raster"));
        }
        if entry_ink != ink {
            return Err(invalid("an entry's colour model is not the document's"));
        }
        check_native(native).map_err(|e| invalid(&e))?;
        if !inside(rect, raster) || !inside(crop, raster) {
            return Err(invalid("an entry's frame lies outside its raster"));
        }
        if items == 0 || items > MAX_VARIATIONS || selected >= items {
            return Err(invalid("an entry's count or selection is out of range"));
        }
        let mut variations = Vec::with_capacity(items);
        let mut lengths = Vec::with_capacity(items);
        for _ in 0..items {
            let size = (r.u32()?, r.u32()?);
            if !RESULT_SIDES.contains(&size.0) || !RESULT_SIDES.contains(&size.1) {
                return Err(invalid("a variation's size is out of range"));
            }
            let tag_len = r.u32()? as usize;
            if tag_len > MAX_DESCRIPTION {
                return Err(invalid("a variation's tag is too long"));
            }
            let tag = std::str::from_utf8(r.take(tag_len)?)
                .map_err(|_| invalid("a variation's tag is not UTF-8"))?;
            check_tag(tag).map_err(|e| invalid(&e))?;
            let png_len = u64::from(r.u32()?);
            if png_len > MAX_VARIATION_PNG_BYTES {
                return Err(invalid("a generative variation is over 80 MiB"));
            }
            let png = r.take(png_len as usize)?;
            if png_size(png) != Some(size) {
                return Err(invalid("a variation's PNG does not match its size"));
            }
            lengths.push(png_len);
            variations.push(Variation {
                png: Arc::from(png),
                size,
                tag: Arc::from(tag),
            });
        }
        check_caps(&lengths).map_err(|e| invalid(&e))?;
        total = total.saturating_add(lengths.iter().sum::<u64>());
        check_document(total).map_err(|e| invalid(&e))?;
        out.push((
            layer,
            GeneratedVariations {
                raster,
                rect,
                crop,
                native,
                ink,
                selected,
                items: Arc::new(variations),
            },
        ));
    }
    Ok(out)
}

/// The stored bytes the parsed entries hold (their pictures and tags).
pub(crate) fn held_bytes(entries: &[(usize, GeneratedVariations)]) -> u64 {
    entries
        .iter()
        .flat_map(|(_, t)| t.items.iter())
        .map(|v| v.png.len() as u64 + v.tag.len() as u64)
        .sum()
}
