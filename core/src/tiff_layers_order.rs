//! The ONE byte-order walk of a layered TIFF's tag-37724 block, in
//! both directions: [`reframe_info`] (the layer information — count,
//! records, channel data) and [`reframe_entries`] (the tagged entries after
//! the block's prefix, as key and reframed body) turn a little-endian (`II`,
//! "IBM PC") block into its big-endian form and back.
//!
//! **The little-endian form**, as Photoshop 26.10.0 wrote its own
//! `layered32.tif` (take1, `tests/fixtures/psd_photoshop/float32_679`):
//! every field of the block is in the file's byte order — the record count
//! (i16), each rectangle (4 × i32), each channel id (i16) and length (u32),
//! each length field — and every four-character code (an entry's or a
//! record's `8BIM`, a blend key, a block key, `lsct`'s `8BIM` and blend key)
//! is stored as a little-endian u32, i.e. byte-reversed (`MIB8`, `mron`,
//! `23rL`); entries are still padded to 4. A record's mask body (0, 20 or 36
//! bytes; mask parameters are refused, as `psd_raster_masks` drops them)
//! swaps its rectangles; its Pascal name, padded to 4, is verbatim; its
//! blending ranges are verbatim only when every range is Photoshop's default
//! `00 00 ff ff` (the same bytes in either order), else refused by name.
//!
//! **Block bodies** are swapped field by field where this crate knows them —
//! `luni` (u32 count and UTF-16 units), `lyid` and `lspf` (u32), `lclr`
//! (4 × u16), `lsct` (u32, then its `8BIM` and blend key, then a u32
//! sub-type) and `fxrp` (2 × f64) — and copied verbatim where they are
//! single bytes (`iOpa`, `clbl`, `infx`, `knko`) or are never interpreted
//! (`shmd`, and the document-level `LMsk`, `cinf`, `CAI `, `GenI`, `FMsk`);
//! a pattern table (`Patt`, `Pat2`, `Pat3`) or `Mt32` is carried only when
//! empty. Any other key — every descriptor block (`lfx2`, fills,
//! adjustments) — refuses by name, in the reader's words when reading
//! (`II` → `MM`) and the writer's when writing (`MM` → `II`).
//!
//! **Channel data.** Photoshop writes a little-endian 32-bit file's channels
//! as compression word 3 (ZIP with its byte-plane prediction), whose payload
//! is the same bytes in either order; reading passes it, and an empty word-0
//! plane, through with the word swapped, and refuses any other word by name.
//! Writing rewrites each non-empty Raw f32 plane as word 3
//! (`psd_zip::zip_predicted32`, row width from the record's or the mask's
//! rectangle).
//!
//! [`big_endian_form`] is the reader's entry: a little-endian 32-bit layered
//! TIFF (`tiff_layers::locate_ordered`) re-laid as the big-endian layer-only
//! file the existing layered reader reads (`tiff_layers::write_with`, no
//! strip). [`layered_tiff_little_endian`] is the test and take seam: one of
//! Doctored's own big-endian 32-bit layered TIFFs written little-endian, as
//! Photoshop would; it is not an export and admits no memory plan.

use crate::psd_layer_effects::Block;
use crate::psd_zip::zip_predicted32;
use crate::tiff_layers::{locate, locate_ordered, write_with, ByteOrder, LayeredTiff};

/// A record's channel-count bound, as the PSD header's channel count.
const MAX_CHANNELS: u16 = 56;
/// The entry walk's bound: Photoshop writes a handful of entries.
const MAX_ENTRIES: usize = 4096;

fn other(order: ByteOrder) -> ByteOrder {
    match order {
        ByteOrder::Big => ByteOrder::Little,
        ByteOrder::Little => ByteOrder::Big,
    }
}

/// Why `thing` stops the walk, in the reader's words (`from` little-endian)
/// or the little-endian writer's (`from` big-endian).
fn refusal(from: ByteOrder, thing: &str) -> String {
    match from {
        ByteOrder::Little => format!(
            "a little-endian (IBM PC) layered TIFF's {thing} is not read; open it flat, or resave it from Photoshop with Macintosh byte order"
        ),
        ByteOrder::Big => format!("a little-endian layered TIFF does not carry a {thing}"),
    }
}

fn record_refusal(from: ByteOrder, key: [u8; 4]) -> String {
    refusal(
        from,
        &format!("“{}” record", String::from_utf8_lossy(&key).trim_end()),
    )
}

fn truncated() -> String {
    "the layered TIFF's layer block is truncated".into()
}

/// One pass over `input` in `from`'s order, writing `out` in the other.
struct Reframe<'a> {
    input: &'a [u8],
    at: usize,
    from: ByteOrder,
    to: ByteOrder,
    out: Vec<u8>,
}

impl<'a> Reframe<'a> {
    fn new(input: &'a [u8], from: ByteOrder) -> Self {
        Self {
            input,
            at: 0,
            from,
            to: other(from),
            out: Vec::with_capacity(input.len()),
        }
    }

    fn remaining(&self) -> usize {
        self.input.len() - self.at
    }

    /// The next `n` input bytes, not written.
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or_else(truncated)?;
        let bytes = self.input.get(self.at..end).ok_or_else(truncated)?;
        self.at = end;
        Ok(bytes)
    }

    fn copy(&mut self, n: usize) -> Result<(), String> {
        let bytes = self.take(n)?;
        self.out.extend_from_slice(bytes);
        Ok(())
    }

    fn u16(&mut self) -> Result<u16, String> {
        let value = self.from.u16(self.take(2)?);
        self.out.extend_from_slice(&self.to.put16(value));
        Ok(value)
    }

    /// A u32 read, not written.
    fn read32(&mut self) -> Result<u32, String> {
        Ok(self.from.u32(self.take(4)?))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let value = self.read32()?;
        self.out.extend_from_slice(&self.to.put32(value));
        Ok(value)
    }

    /// An eight-byte field (an f64): its bytes reversed.
    fn u64(&mut self) -> Result<(), String> {
        let bytes = self.take(8)?;
        self.out.extend(bytes.iter().rev());
        Ok(())
    }

    /// A four-character code, returned in its big-endian (readable) form.
    fn code(&mut self) -> Result<[u8; 4], String> {
        let bytes = self.take(4)?;
        let code = self.from.code([bytes[0], bytes[1], bytes[2], bytes[3]]);
        self.out.extend_from_slice(&self.to.code(code));
        Ok(code)
    }

    /// Every input byte left, verbatim.
    fn rest(&mut self) -> Result<(), String> {
        self.copy(self.remaining())
    }
}

/// One tagged block's body (record or document level) in the other order,
/// the same length (module doc's key table).
fn reframe_body(key: [u8; 4], body: &[u8], from: ByteOrder) -> Result<Vec<u8>, String> {
    let mut walk = Reframe::new(body, from);
    match &key {
        b"luni" => {
            let count = walk.u32()? as usize;
            for _ in 0..count.min(walk.remaining() / 2) {
                walk.u16()?;
            }
        }
        b"lyid" | b"lspf" => {
            walk.u32()?;
        }
        b"lclr" => {
            for _ in 0..4 {
                walk.u16()?;
            }
        }
        b"lsct" => {
            walk.u32()?;
            if walk.remaining() >= 8 {
                walk.code()?;
                walk.code()?;
            }
            if walk.remaining() >= 4 {
                walk.u32()?;
            }
        }
        b"fxrp" => {
            walk.u64()?;
            walk.u64()?;
        }
        b"iOpa" | b"clbl" | b"infx" | b"knko" | b"shmd" | b"LMsk" | b"cinf" | b"CAI " | b"GenI"
        | b"FMsk" => {}
        b"Patt" | b"Pat2" | b"Pat3" | b"Mt32" if body.is_empty() => {}
        _ => return Err(record_refusal(from, key)),
    }
    walk.rest()?;
    Ok(walk.out)
}

/// One record's channel as the record walk found it: its id, the input
/// length, where its length field sits in the output, and the plane's
/// width and rows.
struct Channel {
    length: usize,
    field: usize,
    size: Option<(usize, usize)>,
}

/// A rectangle's width and rows, `None` when reversed.
fn size_of(rect: [i32; 4]) -> Option<(usize, usize)> {
    let [top, left, bottom, right] = rect;
    let rows = usize::try_from(i64::from(bottom) - i64::from(top)).ok()?;
    let width = usize::try_from(i64::from(right) - i64::from(left)).ok()?;
    Some((width, rows))
}

fn rect(walk: &mut Reframe<'_>) -> Result<[i32; 4], String> {
    let mut out = [0i32; 4];
    for edge in &mut out {
        *edge = walk.u32()? as i32;
    }
    Ok(out)
}

/// A channel's plane in the other order (module doc).
fn reframe_channel(
    data: &[u8],
    size: Option<(usize, usize)>,
    from: ByteOrder,
) -> Result<Vec<u8>, String> {
    if data.len() < 2 {
        return Err(truncated());
    }
    let to = other(from);
    let word = from.u16(data);
    let payload = &data[2..];
    let name = || match word {
        0 => "Raw 32-bit channel".to_string(),
        1 => "RLE channel".to_string(),
        2 => "ZIP channel".to_string(),
        other => format!("compression {other} channel"),
    };
    let framed = |word: u16, payload: &[u8]| {
        let mut out = to.put16(word).to_vec();
        out.extend_from_slice(payload);
        out
    };
    match (word, from) {
        (0, _) if payload.is_empty() => Ok(framed(0, payload)),
        (3, _) => Ok(framed(3, payload)),
        (0, ByteOrder::Big) => {
            let (width, rows) = size.ok_or_else(|| refusal(from, &name()))?;
            if width.checked_mul(rows).and_then(|n| n.checked_mul(4)) != Some(payload.len()) {
                return Err("a Raw 32-bit channel does not match its rectangle".into());
            }
            Ok(framed(3, &zip_predicted32(payload, width)))
        }
        _ => Err(refusal(from, &name())),
    }
}

/// The layer information — count, records, channel data — in the other byte
/// order (module doc). `Err` names what this walk does not carry.
pub(crate) fn reframe_info(info: &[u8], from: ByteOrder) -> Result<Vec<u8>, String> {
    let mut walk = Reframe::new(info, from);
    let count = walk.u16()? as i16;
    let mut channels: Vec<Channel> = Vec::new();
    for _ in 0..count.unsigned_abs() {
        let bounds = rect(&mut walk)?;
        let n = walk.u16()?;
        if n > MAX_CHANNELS {
            return Err("a layer record declares too many channels".into());
        }
        let mut ids = Vec::with_capacity(usize::from(n));
        for _ in 0..n {
            let id = walk.u16()? as i16;
            let field = walk.out.len();
            let length = walk.u32()? as usize;
            ids.push((id, length, field));
        }
        if walk.code()? != *b"8BIM" {
            return Err("a layer record has an invalid signature".into());
        }
        walk.code()?; // the blend key
        walk.copy(4)?; // opacity, clipping, flags, filler
        let extra = walk.u32()? as usize;
        let end = walk.at.checked_add(extra).ok_or_else(truncated)?;
        if end > info.len() {
            return Err(truncated());
        }
        let (mut mask, mut real) = (None, None);
        match walk.u32()? {
            0 => {}
            length @ (20 | 36) => {
                mask = size_of(rect(&mut walk)?);
                let flags = walk.take(2)?;
                walk.out.extend_from_slice(flags);
                if flags[1] & 16 != 0 {
                    return Err(refusal(from, "mask parameter block"));
                }
                // 20: two padding bytes; 36: the real flags and default,
                // then the real rectangle.
                walk.copy(2)?;
                if length == 36 {
                    real = size_of(rect(&mut walk)?);
                }
            }
            _ => return Err(refusal(from, "mask")),
        }
        let ranges = walk.u32()? as usize;
        let ranges = walk.take(ranges)?;
        if !ranges.len().is_multiple_of(4) || ranges.chunks(4).any(|r| r != [0, 0, 0xff, 0xff]) {
            return Err(refusal(from, "non-default blending range"));
        }
        walk.out.extend_from_slice(ranges);
        let name = usize::from(*info.get(walk.at).ok_or_else(truncated)?);
        walk.copy((1 + name).next_multiple_of(4))?;
        while walk.at < end {
            if walk.code()? != *b"8BIM" {
                return Err("a layer record's block has an invalid signature".into());
            }
            let key = walk.code()?;
            let length = walk.u32()? as usize;
            let body = walk.take(length)?;
            let body = reframe_body(key, body, from)?;
            walk.out.extend_from_slice(&body);
        }
        if walk.at != end {
            return Err(truncated());
        }
        for (id, length, field) in ids {
            let size = match id {
                -2 => mask,
                -3 => real,
                _ => size_of(bounds),
            };
            channels.push(Channel {
                length,
                field,
                size,
            });
        }
    }
    let to = other(from);
    for channel in channels {
        let data = walk.take(channel.length)?;
        let plane = reframe_channel(data, channel.size, from)?;
        let length = u32::try_from(plane.len()).map_err(|_| truncated())?;
        walk.out[channel.field..channel.field + 4].copy_from_slice(&to.put32(length));
        walk.out.extend_from_slice(&plane);
    }
    // At most the writer's padding follows the channel data.
    if walk.remaining() > 3 {
        return Err("the layered TIFF's layer block has trailing data".into());
    }
    if !walk.out.len().is_multiple_of(2) {
        walk.out.push(0);
    }
    Ok(walk.out)
}

/// Tag 37724's entries after its prefix, each reframed: `(key, body)` with
/// the body in the other order ([`reframe_info`] for `Lr32`) and the key in
/// its readable (big-endian) form, for `tiff_layers::write_with` to frame in
/// the order it writes.
pub(crate) fn reframe_entries(entries: &[u8], from: ByteOrder) -> Result<Vec<Block>, String> {
    let mut walk = Reframe::new(entries, from);
    let mut out = Vec::new();
    while walk.remaining() >= 12 {
        if out.len() == MAX_ENTRIES {
            return Err("the layered TIFF's block list has too many entries".into());
        }
        if walk.code()? != *b"8BIM" {
            return Err("the layered TIFF's block list has an invalid signature".into());
        }
        let key = walk.code()?;
        let length = walk.read32()? as usize;
        let body = walk.take(length)?;
        let body = match &key {
            b"Lr32" => reframe_info(body, from)?,
            _ => reframe_body(key, body, from)?,
        };
        out.push((key, body));
        // Every entry is padded to 4; the last one may end the list early.
        walk.take((length.wrapping_neg() & 3).min(walk.remaining()))?;
    }
    Ok(out)
}

/// The big-endian, layer-only form of a little-endian 32-bit layered TIFF —
/// what `psd_import::report` and the precision PSD open read in its place
/// (module doc) — or `None` for every other file, which reads as before.
/// `Err` names a block this walk does not read; such a file opens flat.
pub(crate) fn big_endian_form(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let Some((ByteOrder::Little, layers)) = locate_ordered(bytes) else {
        return Ok(None);
    };
    let mut info = None;
    let mut tail = Vec::new();
    for (key, body) in reframe_entries(layers.blocks, ByteOrder::Little)? {
        if key == *b"Lr32" && info.is_none() {
            info = Some(body);
        } else {
            tail.push((key, body));
        }
    }
    let info = info.ok_or("the layered TIFF has no Lr32 block")?;
    write_with(
        &LayeredTiff {
            width: layers.width,
            height: layers.height,
            depth: 32,
            color: 3,
            icc: layers.icc.unwrap_or(&[]),
            resolution: layers.resolution.unwrap_or_default(),
            resources: layers.resources,
            tail: &tail,
            order: ByteOrder::Big,
            spots: 0,
        },
        info.len(),
        &|out| {
            out.extend_from_slice(&info);
            Ok(())
        },
        None,
        &|_| Ok(()),
    )
    .map(Some)
}

/// One of Doctored's own big-endian 32-bit layered TIFFs written
/// little-endian, as Photoshop writes byte order IBM PC (module doc): the
/// layer information through [`reframe_info`], every flat-image f32
/// byte-reversed, the resources and profile verbatim. A test and Photoshop
/// take seam (`examples/tiff32_little.rs`), not an export: no admission.
/// `Err` for any other file, and by name for a record or document-level
/// block the little-endian form does not carry (an empty pattern table or
/// `Mt32` is dropped).
#[doc(hidden)]
pub fn layered_tiff_little_endian(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let layers = locate(bytes)
        .filter(|layers| layers.depth == 32)
        .ok_or("not a big-endian 32-bit layered TIFF")?;
    let strip = layers
        .strip
        .ok_or("the 32-bit layered TIFF's flat image is not one strip")?;
    let mut info = None;
    for (key, body) in reframe_entries(layers.blocks, ByteOrder::Big)? {
        match &key {
            b"Lr32" if info.is_none() => info = Some(body),
            b"Patt" | b"Pat2" | b"Pat3" | b"Mt32" if body.is_empty() => {}
            _ => return Err(record_refusal(ByteOrder::Big, key)),
        }
    }
    let info = info.ok_or("the layered TIFF has no Lr32 block")?;
    write_with(
        &LayeredTiff {
            width: layers.width,
            height: layers.height,
            depth: 32,
            color: 3,
            icc: layers.icc.unwrap_or(&[]),
            resolution: layers.resolution.unwrap_or_default(),
            resources: layers.resources,
            tail: &[],
            order: ByteOrder::Little,
            spots: 0,
        },
        info.len(),
        &|out| {
            out.extend_from_slice(&info);
            Ok(())
        },
        Some(strip.len()),
        &|out| {
            for sample in strip.chunks_exact(4) {
                out.extend(sample.iter().rev());
            }
            Ok(())
        },
    )
}
