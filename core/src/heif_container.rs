//! The primary item's colour facts, read from an ISO-BMFF/HEIF file's own
//! boxes. This is a bounded box reader, not a decoder: it walks the
//! top-level boxes to `meta`, then reads `pitm`, `iprp` (`ipco` + every
//! `ipma`) and `iref`, and answers two facts about the primary item — the
//! `matrix_coefficients` of the first `colr`/`nclx` property associated with
//! it, and whether an `iref` `prem` reference starts at it.
//!
//! Why the host needs them: macOS's ImageIO decodes a 12-bit AVIF whose
//! primary item declares the identity matrix (`nclx` matrix 0) LEFT-ALIGNED
//! in 16 bits (`v << 4`, white 65520), while every other 12-bit AVIF comes
//! back full scale, and no ImageIO property tells the two apart. A
//! premultiplied (`prem`) file is the exception: ImageIO returns it full
//! scale whatever the matrix. So the host asks the container.
//!
//! Bounds: it never recurses below `meta` → `iprp` → `ipco`, never allocates
//! (declared counts are iterated, and each iteration consumes bytes or
//! refuses as truncated), and never panics — every read is a checked slice.
//! A size smaller than its header or past its parent, a truncated field, a
//! property index past `ipco` and a file that does not begin with `ftyp` are
//! `Err`; a well-formed file lacking `meta`, `pitm`, `iprp` or an `nclx`
//! answers `None` / `false`.
//!
//! Bounded: no recursion below `meta` → `iprp` → `ipco`, no allocation (declared counts iterated),
//! checked reads only; a short/overrunning size, a truncated field, a property index past `ipco` or
//! a file not beginning with `ftyp` is `Err`, a file lacking a part answers `None`/`false`. The
//! host reads it to choose the `_left12` host-pixel token for a 12-bit identity-matrix AVIF
//! (`HostStillDecoder`). Tests: `tests/heif_container.rs`.
//!
//! [`premultiplied_reference`] is the same walk answering WHERE that `prem` reference's
//! `from_item_ID` field is: ImageIO drops the alpha of a premultiplied AVIF, so the host decodes
//! a copy whose field no longer names the primary item and declares the samples premultiplied
//! itself (`HostStillDecoder`, the `_premultiplied` host-pixel tokens).

const TRUNCATED: &str = "heif box is truncated";

/// The two facts. `matrix_coefficients` is the first `colr`/`nclx` that
/// `ipma` associates with the primary item (`None` if none);
/// `premultiplied_alpha` is true when an `iref` `prem` child has the primary
/// item as its `from_item_ID` (it lists the alpha item among its
/// `to_item_IDs`), which is libavif's direction (`colorItem->premByID = toID`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryColorFacts {
    pub matrix_coefficients: Option<u16>,
    pub premultiplied_alpha: bool,
}

/// Reads the primary item's colour facts from `bytes` (a whole file).
pub fn primary_color_facts(bytes: &[u8]) -> Result<PrimaryColorFacts, String> {
    let none = PrimaryColorFacts {
        matrix_coefficients: None,
        premultiplied_alpha: false,
    };
    let Some(parts) = primary_parts(bytes)? else {
        return Ok(none);
    };
    let matrix_coefficients = match parts.iprp {
        Some(iprp) => primary_matrix(iprp, parts.primary)?,
        None => None,
    };
    let premultiplied_alpha = match parts.iref {
        Some(iref) => premultiplied_from(iref, parts.primary)?.is_some(),
        None => false,
    };
    Ok(PrimaryColorFacts {
        matrix_coefficients,
        premultiplied_alpha,
    })
}

/// Where the primary item's `prem` reference names it. `Ok(None)`: no
/// `iref` `prem` child whose `from_item_ID` is the primary item — the files
/// [`primary_color_facts`] answers `premultiplied_alpha: false` for.
/// `Some((offset, width))`: the byte offset in `bytes` of that child's
/// `from_item_ID` field and the field's width, 2 for an `iref` of version 0
/// and 4 otherwise. Only the FIRST such child is located; a file with a
/// second one still reads as premultiplied after the first is detached.
/// Refusals and bounds are `primary_color_facts`'s: nothing is allocated and
/// every `iref` child is still checked for a complete `to_item_IDs` list.
pub fn premultiplied_reference(bytes: &[u8]) -> Result<Option<(usize, usize)>, String> {
    let Some(parts) = primary_parts(bytes)? else {
        return Ok(None);
    };
    let Some(iref) = parts.iref else {
        return Ok(None);
    };
    let Some((within, width)) = premultiplied_from(iref, parts.primary)? else {
        return Ok(None);
    };
    // `iref` is a sub-slice of `bytes`, so the difference of the two starts
    // is its offset in the file.
    let base = (iref.as_ptr() as usize)
        .checked_sub(bytes.as_ptr() as usize)
        .ok_or(TRUNCATED)?;
    Ok(Some((base + within, width)))
}

/// The primary item's ID and the `iprp` and `iref` bodies of the file's
/// `meta` box.
struct PrimaryParts<'a> {
    primary: u32,
    iprp: Option<&'a [u8]>,
    iref: Option<&'a [u8]>,
}

/// Walks the top-level boxes to `meta` and reads `pitm`. `Ok(None)`: a
/// well-formed file with no `meta` or no `pitm`.
fn primary_parts(bytes: &[u8]) -> Result<Option<PrimaryParts<'_>>, String> {
    let mut top = Boxes::new(bytes);
    match top.next() {
        Some(Ok((kind, _))) if &kind == b"ftyp" => {}
        Some(Err(error)) => return Err(error),
        _ => return Err("not an iso-bmff file: it does not begin with an ftyp box".into()),
    }
    let mut meta = None;
    for item in top {
        let (kind, body) = item?;
        if &kind == b"meta" {
            meta = Some(body);
            break;
        }
    }
    let Some(meta) = meta else { return Ok(None) };
    // `meta` is a FullBox: version and flags precede its children.
    let children = meta.get(4..).ok_or(TRUNCATED)?;
    let (mut pitm, mut iprp, mut iref) = (None, None, None);
    for child in Boxes::new(children) {
        let (kind, body) = child?;
        match &kind {
            b"pitm" if pitm.is_none() => {
                let mut cursor = Cursor::new(body);
                let version = cursor.u8()?;
                cursor.skip(3)?;
                pitm = Some(cursor.id(version)?);
            }
            b"iprp" if iprp.is_none() => iprp = Some(body),
            b"iref" if iref.is_none() => iref = Some(body),
            _ => {}
        }
    }
    Ok(pitm.map(|primary| PrimaryParts {
        primary,
        iprp,
        iref,
    }))
}

/// The first `nclx` matrix among the primary item's associations, every
/// `ipma` association's index checked against the `ipco` property count.
fn primary_matrix(iprp: &[u8], primary: u32) -> Result<Option<u16>, String> {
    let mut ipco = None;
    for child in Boxes::new(iprp) {
        let (kind, body) = child?;
        if &kind == b"ipco" && ipco.is_none() {
            ipco = Some(body);
        }
    }
    let ipco = ipco.unwrap_or(&[]);
    let mut properties = 0usize;
    for child in Boxes::new(ipco) {
        child?;
        properties += 1;
    }
    let mut found = None;
    for child in Boxes::new(iprp) {
        let (kind, body) = child?;
        if &kind != b"ipma" {
            continue;
        }
        let mut cursor = Cursor::new(body);
        let version = cursor.u8()?;
        let flags = cursor.u24()?;
        let entries = cursor.u32()?;
        for _ in 0..entries {
            let item = cursor.id(version)?;
            let associations = cursor.u8()?;
            for _ in 0..associations {
                // The top bit is `essential`; the rest is the 1-based index.
                let index = if flags & 1 != 0 {
                    usize::from(cursor.u16()? & 0x7fff)
                } else {
                    usize::from(cursor.u8()? & 0x7f)
                };
                if index == 0 {
                    continue;
                }
                if index > properties {
                    return Err("heif property association points past ipco".into());
                }
                if found.is_none() && item == primary {
                    found = nclx_matrix(ipco, index)?;
                }
            }
        }
    }
    Ok(found)
}

/// The matrix of the `index`-th (1-based) `ipco` property when it is a
/// `colr` of colour type `nclx`: primaries u16, transfer u16, matrix u16,
/// then the full-range flag byte.
fn nclx_matrix(ipco: &[u8], index: usize) -> Result<Option<u16>, String> {
    let Some(property) = Boxes::new(ipco).nth(index - 1) else {
        return Err("heif property association points past ipco".into());
    };
    let (kind, body) = property?;
    if &kind != b"colr" {
        return Ok(None);
    }
    let mut cursor = Cursor::new(body);
    if cursor.take(4)? != b"nclx" {
        return Ok(None);
    }
    cursor.skip(4)?;
    let matrix = cursor.u16()?;
    cursor.u8()?;
    Ok(Some(matrix))
}

/// `Some` when some `iref` `prem` child's `from_item_ID` is `primary`: the
/// offset within `iref` of the FIRST such child's `from_item_ID` field and
/// the field's width (2 for version 0, else 4). Every child is checked for a
/// complete `to_item_IDs` list, also after a match.
fn premultiplied_from(iref: &[u8], primary: u32) -> Result<Option<(usize, usize)>, String> {
    let mut cursor = Cursor::new(iref);
    let version = cursor.u8()?;
    cursor.skip(3)?;
    let width = if version == 0 { 2 } else { 4 };
    let mut found = None;
    for child in Boxes::new(cursor.rest()) {
        let (kind, body) = child?;
        let mut reference = Cursor::new(body);
        let from = reference.id(version)?;
        let count = usize::from(reference.u16()?);
        reference.skip(count * width)?;
        if &kind == b"prem" && from == primary && found.is_none() {
            // `body` is a sub-slice of `iref` that starts at the field.
            let within = (body.as_ptr() as usize)
                .checked_sub(iref.as_ptr() as usize)
                .ok_or(TRUNCATED)?;
            found = Some((within, width));
        }
    }
    Ok(found)
}

/// Sibling boxes over one parent's body. A box is a u32 size and a 4CC; size
/// 1 means a u64 largesize follows, size 0 that the box runs to the end of
/// its parent. A malformed box ends the walk with its `Err`.
struct Boxes<'a> {
    rest: &'a [u8],
}
impl<'a> Boxes<'a> {
    fn new(body: &'a [u8]) -> Self {
        Self { rest: body }
    }
}
impl<'a> Iterator for Boxes<'a> {
    type Item = Result<([u8; 4], &'a [u8]), String>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        match split_box(self.rest) {
            Ok((kind, body, rest)) => {
                self.rest = rest;
                Some(Ok((kind, body)))
            }
            Err(error) => {
                self.rest = &[];
                Some(Err(error))
            }
        }
    }
}

type SplitBox<'a> = ([u8; 4], &'a [u8], &'a [u8]);

fn split_box(data: &[u8]) -> Result<SplitBox<'_>, String> {
    let mut cursor = Cursor::new(data);
    let size = cursor.u32()?;
    let kind: [u8; 4] = cursor.take(4)?.try_into().map_err(|_| TRUNCATED)?;
    let (header, size) = match size {
        0 => (8u64, data.len() as u64),
        1 => (16u64, cursor.u64()?),
        size => (8u64, u64::from(size)),
    };
    if size < header {
        return Err("heif box size is smaller than its header".into());
    }
    if size > data.len() as u64 {
        return Err("heif box overruns its parent".into());
    }
    let (header, size) = (header as usize, size as usize);
    Ok((kind, &data[header..size], &data[size..]))
}

/// Big-endian field reads; every read is checked and a short body is
/// `TRUNCATED`.
struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(count).ok_or(TRUNCATED)?;
        let bytes = self.data.get(self.at..end).ok_or(TRUNCATED)?;
        self.at = end;
        Ok(bytes)
    }
    fn skip(&mut self, count: usize) -> Result<(), String> {
        self.take(count).map(|_| ())
    }
    fn rest(&self) -> &'a [u8] {
        self.data.get(self.at..).unwrap_or(&[])
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }
    fn u24(&mut self) -> Result<u32, String> {
        let bytes = self.take(3)?;
        Ok(u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
    fn u64(&mut self) -> Result<u64, String> {
        let bytes = self.take(8)?;
        let mut word = [0u8; 8];
        word.copy_from_slice(bytes);
        Ok(u64::from_be_bytes(word))
    }
    /// An item ID: u16 in a version-0 FullBox, u32 otherwise.
    fn id(&mut self, version: u8) -> Result<u32, String> {
        if version == 0 {
            self.u16().map(u32::from)
        } else {
            self.u32()
        }
    }
}
