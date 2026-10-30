//! The PSD/PSB/layered-TIFF PATTERN table: the `Patt` tail block a Pattern
//! Overlay's or a Pattern Stroke's `Ptrn` reference resolves through, read
//! and written.
//! Framing only; the `patternFill` object is `psd_pattern_fill`.
//!
//! # The block
//!
//! A tail block of the layer-and-mask section (after the global layer mask;
//! in a layered TIFF a block of the tag-37724 list, after `Layr`/`Lr16`): a run
//! of records, each `u32 length` + the record, padded to a multiple of 4.
//! Adobe's "Patterns" record: `u32` version 1, `u32` colour mode, `u16`
//! height then width, the Unicode name (`u32` count, UTF-16BE, a trailing 0),
//! the Pascal id (`u8` length + bytes), then the Virtual Memory Array List:
//! `u32` version 3, `u32` length, the rectangle (top, left, bottom, right as
//! `u32`), `u32` channel count, then channel count + 2 arrays (the colour
//! channels, the user mask, the sheet mask). An array is `u32` written (0 =
//! absent and nothing follows), `u32` length (0 = empty), `u32` depth, the
//! rectangle, `u16` pixel depth, `u8` compression (0 raw, 1 RLE: `u16` row
//! counts then PackBits, `psd_framing`) and the data.
//!
//! READ: RGB (3) and Gray (1) at 8 or 16 bits, raw or RLE; the first written
//! array after the colour channels is the alpha. Any other record is kept as
//! an entry whose tile is the reason, so a reference to it declines by name.
//! The importer reads `Patt`, `Pat2` and `Pat3` alike: take2 measured
//! Photoshop 26.10 writing the SAME 8-bit records under `Patt` in an 8-bit
//! file and under `Pat2` in a 16-bit one (its own arrays: 24 channels, raw
//! and RLE mixed). A Pattern Fill layer in a 16-bit document stores its tile
//! at 16 bits (the Pattern Fill take1's P16: depth and pixel depth 16, raw),
//! in Photoshop's internal 16-bit range 0…32768 — tile T's 8-bit code 10 is
//! stored 1285 = round(10 · 32768 / 255) — so a 16-bit sample v reads as
//! the 8-bit code round(min(v, 32768) · 255 / 32768), exactly the code it
//! was made from.
//! WRITE: RGB, raw, one record per distinct pattern, the alpha written as the
//! user mask only when some texel is not opaque, keyed `Patt` at 8 bits and
//! `Pat2` at 16 (Photoshop opened both keys in take2; `Pat2` is its own form).
//! A Pattern Fill layer's `PtFl` (`psd_fill_layer`) names its tile in the
//! same table: [`tail_blocks_with`] and [`tail_bytes_with`] take the fill
//! layers' patterns after the styles'. A file without a pattern overlay, a
//! Pattern stroke or a pattern fill layer writes no block at all.

use std::sync::Arc;

use crate::pattern_tile::{PatternTile, MAX_TILE_SIDE};
use crate::psd_layer_effects::Block;
use crate::style::{LayerStyle, Pattern, PatternFill, PatternSource};
use crate::style_pattern_fill::placed_fill;

/// Why a record's tile is not read.
const MODE: &str = "pattern uses a colour mode Doctored does not read";
const SIDE: &str = "pattern is larger than 512 px on a side";
const DEPTH: &str = "pattern samples are not 8- or 16-bit";

/// Photoshop's internal 16-bit full scale, the range a 16-bit pattern
/// array's samples are stored in (module doc's READ).
const PHOTOSHOP_16_BIT_WHITE: u32 = 32768;
const COMPRESSION: &str = "pattern uses a compression Doctored does not read";
const INCOMPLETE: &str = "pattern data is incomplete";

/// The patterns a file carries: `(Idnt, name, tile or the reason it was not
/// read)` in file order.
#[derive(Default)]
pub(crate) struct PatternTable {
    pub entries: Vec<(String, String, Result<PatternTile, String>)>,
}

impl PatternTable {
    /// The entry whose id is `id`, else the first whose name is `name`.
    pub(crate) fn find(
        &self,
        id: &str,
        name: &str,
    ) -> Option<&(String, String, Result<PatternTile, String>)> {
        self.entries
            .iter()
            .find(|(i, _, _)| !id.is_empty() && i == id)
            .or_else(|| self.entries.iter().find(|(_, n, _)| n == name))
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let out = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(out)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// The table of a `Patt` (or `Pat2`/`Pat3`) body. Never panics: a truncated
/// record ends the walk with the entries read so far.
pub(crate) fn read_table(body: &[u8]) -> PatternTable {
    let mut table = PatternTable::default();
    let mut at = 0usize;
    while at + 4 <= body.len() && table.entries.len() < 4096 {
        let length =
            u32::from_be_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]]) as usize;
        let Some(record) = at
            .checked_add(4 + length)
            .and_then(|end| body.get(at + 4..end))
        else {
            break;
        };
        match read_record(record) {
            Some(entry) => table.entries.push(entry),
            None => break,
        }
        at += 4 + length;
        at += at.wrapping_neg() & 3;
    }
    table
}

/// One record: `None` only when its head (through the id) is truncated.
fn read_record(record: &[u8]) -> Option<(String, String, Result<PatternTile, String>)> {
    let mut c = Cursor {
        bytes: record,
        at: 0,
    };
    let _version = c.u32()?;
    let mode = c.u32()?;
    let height = u32::from(c.u16()?);
    let width = u32::from(c.u16()?);
    let count = c.u32()? as usize;
    let units: Vec<u16> = c
        .take(count.checked_mul(2)?)?
        .chunks_exact(2)
        .map(|u| u16::from_be_bytes([u[0], u[1]]))
        .take_while(|&u| u != 0)
        .collect();
    let name = String::from_utf16_lossy(&units);
    let id_len = usize::from(c.u8()?);
    let id = String::from_utf8_lossy(c.take(id_len)?).into_owned();
    let tile = read_tile(&mut c, mode, width, height);
    Some((id, name, tile))
}

/// The record's pixels (module doc's READ).
fn read_tile(
    c: &mut Cursor<'_>,
    mode: u32,
    width: u32,
    height: u32,
) -> Result<PatternTile, String> {
    let colours = match mode {
        3 => 3,
        1 => 1,
        _ => return Err(MODE.to_string()),
    };
    if width == 0 || height == 0 || width > MAX_TILE_SIDE || height > MAX_TILE_SIDE {
        return Err(SIDE.to_string());
    }
    let incomplete = || INCOMPLETE.to_string();
    let _version = c.u32().ok_or_else(incomplete)?;
    let _length = c.u32().ok_or_else(incomplete)?;
    c.take(16).ok_or_else(incomplete)?;
    let channels = c.u32().ok_or_else(incomplete)? as usize;
    let plane = width as usize * height as usize;
    let mut planes: Vec<Vec<u8>> = Vec::new();
    for _ in 0..channels.min(64) + 2 {
        if planes.len() > colours {
            break;
        }
        let Some(written) = c.u32() else { break };
        if written == 0 {
            continue;
        }
        let length = c.u32().ok_or_else(incomplete)? as usize;
        if length == 0 {
            continue;
        }
        let array = c.take(length).ok_or_else(incomplete)?;
        planes.push(read_array(array, width, height)?);
    }
    if planes.len() < colours || planes.iter().any(|p| p.len() != plane) {
        return Err(incomplete());
    }
    let mut rgba = Vec::with_capacity(plane * 4);
    for i in 0..plane {
        let (r, g, b) = if colours == 3 {
            (planes[0][i], planes[1][i], planes[2][i])
        } else {
            (planes[0][i], planes[0][i], planes[0][i])
        };
        let a = planes.get(colours).map_or(255, |p| p[i]);
        rgba.extend_from_slice(&[r, g, b, a]);
    }
    Ok(PatternTile {
        width,
        height,
        rgba: rgba.into(),
    })
}

/// One array's plane (after its `length` word): depth, rectangle, pixel
/// depth, compression, data.
fn read_array(array: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut c = Cursor {
        bytes: array,
        at: 0,
    };
    let incomplete = || INCOMPLETE.to_string();
    let depth = c.u32().ok_or_else(incomplete)?;
    c.take(16).ok_or_else(incomplete)?;
    let pixel_depth = c.u16().ok_or_else(incomplete)?;
    let compression = c.u8().ok_or_else(incomplete)?;
    let sample = match (depth, pixel_depth) {
        (8, 8) => 1,
        (16, 16) => 2,
        _ => return Err(DEPTH.to_string()),
    };
    let data = &array[c.at..];
    let plane = width as usize * height as usize;
    let bytes = read_samples(data, width, height, sample, compression)?;
    if sample == 1 {
        return Ok(bytes);
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|v| {
            let v = u32::from(u16::from_be_bytes([v[0], v[1]])).min(PHOTOSHOP_16_BIT_WHITE);
            ((v * 255 + PHOTOSHOP_16_BIT_WHITE / 2) / PHOTOSHOP_16_BIT_WHITE) as u8
        })
        .take(plane)
        .collect())
}

/// An array's stored bytes, `width × height` samples of `sample` bytes:
/// raw, or RLE rows of `width · sample` bytes.
fn read_samples(
    data: &[u8],
    width: u32,
    height: u32,
    sample: usize,
    compression: u8,
) -> Result<Vec<u8>, String> {
    let incomplete = || INCOMPLETE.to_string();
    let plane = width as usize * height as usize * sample;
    match compression {
        0 => data.get(..plane).map(<[u8]>::to_vec).ok_or_else(incomplete),
        1 => {
            // Photoshop may pad the array; the rows end where their counts say.
            let table = height as usize * 2;
            let total: usize = data
                .get(..table)
                .ok_or_else(incomplete)?
                .chunks_exact(2)
                .map(|n| usize::from(u16::from_be_bytes([n[0], n[1]])))
                .sum();
            let used = data.get(..table + total).ok_or_else(incomplete)?;
            let rows = crate::psd_framing::rle_rows(used, height as usize, false)?;
            let mut out = Vec::with_capacity(plane);
            for row in rows {
                out.extend(crate::psd_framing::unpack_packbits(
                    row,
                    width as usize * sample,
                )?);
            }
            Ok(out)
        }
        _ => Err(COMPRESSION.to_string()),
    }
}

/// FNV-1a over 128 bits: the custom pattern id's hash (stable across builds).
pub(crate) fn fnv1a128(parts: &[&[u8]]) -> u128 {
    let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013B;
    for part in parts {
        for &b in *part {
            hash ^= u128::from(b);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

/// The pattern's `Idnt`: a preset's fixed UUID; a custom tile's FNV-1a-128 of
/// (name, width, height, rgba) as 8-4-4-4-12 hex.
pub(crate) fn pattern_id(pattern: &Pattern) -> String {
    let hash = match &pattern.source {
        PatternSource::Preset(id) => fnv1a128(&[b"rasterize-preset-pattern:", id.as_bytes()]),
        PatternSource::Custom { name } => fnv1a128(&[
            name.as_bytes(),
            &pattern.tile.width.to_be_bytes(),
            &pattern.tile.height.to_be_bytes(),
            &pattern.tile.rgba,
        ]),
    };
    uuid_text(hash)
}

/// `hash` as Photoshop's lowercase 8-4-4-4-12 hex id text (a pattern's and
/// a style library record's `Idnt`).
pub(crate) fn uuid_text(hash: u128) -> String {
    let hex = format!("{hash:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// One raw 8-bit array of `plane` (module doc).
fn array(out: &mut Vec<u8>, plane: &[u8], width: u32, height: u32) {
    put_u32(out, 1);
    put_u32(out, 23 + plane.len() as u32);
    put_u32(out, 8);
    for v in [0, 0, height, width] {
        put_u32(out, v);
    }
    out.extend_from_slice(&8u16.to_be_bytes());
    out.push(0);
    out.extend_from_slice(plane);
}

/// One pattern's record, unframed.
fn record(pattern: &Pattern) -> Vec<u8> {
    let tile = &pattern.tile;
    let (w, h) = (tile.width, tile.height);
    let mut out = Vec::new();
    put_u32(&mut out, 1);
    put_u32(&mut out, 3);
    out.extend_from_slice(&(h as u16).to_be_bytes());
    out.extend_from_slice(&(w as u16).to_be_bytes());
    let units: Vec<u16> = pattern.name().encode_utf16().chain([0]).collect();
    put_u32(&mut out, units.len() as u32);
    for u in units {
        out.extend_from_slice(&u.to_be_bytes());
    }
    let id = pattern_id(pattern);
    out.push(id.len() as u8);
    out.extend_from_slice(id.as_bytes());
    let plane = |c: usize| -> Vec<u8> { tile.rgba.chunks_exact(4).map(|p| p[c]).collect() };
    let opaque = tile.rgba.chunks_exact(4).all(|p| p[3] == 255);
    let mut list = Vec::new();
    for v in [0, 0, h, w] {
        put_u32(&mut list, v);
    }
    put_u32(&mut list, 3);
    for c in 0..3 {
        array(&mut list, &plane(c), w, h);
    }
    if opaque {
        put_u32(&mut list, 0);
    } else {
        array(&mut list, &plane(3), w, h);
    }
    put_u32(&mut list, 0);
    put_u32(&mut out, 3);
    put_u32(&mut out, list.len() as u32);
    out.extend_from_slice(&list);
    out
}

/// The pattern tail block for every distinct pattern the styles' overlays
/// hold (enabled or not), each once in first-seen order; empty without one.
/// Keyed `Patt` in an 8-bit file and `Pat2` in a 16-bit one (`sixteen`), as
/// Photoshop keys the same records (take2's KEY16; module doc).
pub(crate) fn tail_blocks<'a>(
    styles: impl IntoIterator<Item = &'a LayerStyle>,
    sixteen: bool,
) -> Vec<Block> {
    tail_blocks_with(styles, &[], sixteen)
}

/// [`tail_blocks`] with the pattern fill layers' fills after the styles'
/// (module doc).
pub(crate) fn tail_blocks_with<'a>(
    styles: impl IntoIterator<Item = &'a LayerStyle>,
    fills: &[&'a PatternFill],
    sixteen: bool,
) -> Vec<Block> {
    let seen = distinct(styles, fills);
    if seen.is_empty() {
        return Vec::new();
    }
    let mut body = Vec::new();
    for (_, _, pattern) in &seen {
        let rec = record(pattern);
        put_u32(&mut body, rec.len() as u32);
        body.extend_from_slice(&rec);
        while !body.len().is_multiple_of(4) {
            body.push(0);
        }
    }
    vec![(if sixteen { *b"Pat2" } else { *b"Patt" }, body)]
}

/// Every distinct pattern the styles' pattern fills hold — an overlay's or a
/// Pattern stroke's, enabled or not — then the fill layers' `fills`, each
/// once in first-seen order, keyed by [`pattern_id`].
fn distinct<'a>(
    styles: impl IntoIterator<Item = &'a LayerStyle>,
    fills: &[&'a PatternFill],
) -> Vec<(String, Arc<PatternTile>, Pattern)> {
    let mut seen: Vec<(String, Arc<PatternTile>, Pattern)> = Vec::new();
    let styled = styles
        .into_iter()
        .flat_map(|style| style.effects.iter().filter_map(placed_fill));
    for fill in styled.chain(fills.iter().copied()) {
        let id = pattern_id(&fill.pattern);
        if !seen.iter().any(|(i, _, _)| *i == id) {
            seen.push((id, Arc::clone(&fill.pattern.tile), fill.pattern.clone()));
        }
    }
    seen
}

/// An upper bound on the length [`encode_tail`] gives [`tail_blocks_with`]'
/// block for these styles and fills, without encoding a tile: the export
/// preflight's worst case adds it (`psd_export_preflight`). Each record is
/// counted with its alpha array, which an opaque tile's record replaces by a
/// 4-byte zero (27 + w·h bytes fewer), so the bound never under-counts; the
/// rest mirrors [`record`] and [`array`] field for field.
pub(crate) fn tail_bytes_with<'a>(
    styles: impl IntoIterator<Item = &'a LayerStyle>,
    fills: &[&'a PatternFill],
) -> u64 {
    let seen = distinct(styles, fills);
    if seen.is_empty() {
        return 0;
    }
    // The block framing: `8BIM`, the key and the `u32` length.
    let mut total = 12u64;
    for (id, tile, pattern) in &seen {
        let plane = u64::from(tile.width) * u64::from(tile.height);
        // An array: version, length, depth, the rect, the `u16` depth and the
        // compression byte (31 bytes), then the plane.
        let array = 31 + plane;
        // The list: the rect, the channel count, four arrays, the trailing 0.
        let list = 16 + 4 + 4 * array + 4;
        let units = pattern.name().encode_utf16().count() as u64 + 1;
        // Version, mode, the two `u16` sides, the name, the id, the list's
        // version and length, then the list.
        let record = 12 + 4 + 2 * units + 1 + id.len() as u64 + 8 + list;
        // The record's `u32` length and its padding to a multiple of 4.
        total += (4 + record).next_multiple_of(4);
    }
    total
}

/// `tail` as tagged blocks (`8BIM`, key, `u32` length, body, padded to a
/// multiple of 4): what follows the global layer mask in a PSD/PSB and the
/// `Layr`/`Lr16` block in a layered TIFF. Empty for an empty `tail`.
pub(crate) fn encode_tail(tail: &[Block]) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, body) in tail {
        out.extend_from_slice(b"8BIM");
        out.extend_from_slice(key);
        put_u32(&mut out, body.len() as u32);
        out.extend_from_slice(body);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    out
}
