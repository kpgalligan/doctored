//! Photoshop's layer block inside a TIFF: the locator a layered open reads it
//! through, and the big-endian writer a layered TIFF export uses.
//!
//! Photoshop saves a TIFF "with layers" as an ordinary flat TIFF plus three
//! private tags, described in Adobe's "TIFF Technical Note 3"-era Photoshop
//! notes and its file format specification
//! (https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/, "TIFF"):
//!
//! | tag | holds |
//! |---|---|
//! | 34377 | the image resources, exactly as a PSD's resource section |
//! | 34675 | the ICC profile |
//! | 37724 | `"Adobe Photoshop Document Data Block\0"`, then tagged blocks: `8BIM`, a four-byte key, a u32 length and the body, each padded to a multiple of 4 |
//!
//! The block keyed `Layr` (8-bit), `Lr16` (16-bit) or `Lr32` (32-bit IEEE
//! float) holds the layer information — record count, records, channel
//! data — framed exactly as a classic PSD's
//! (`psd_framing::LayeredContainer::Tiff` is not `large`). A reader that
//! knows nothing of tag 37724 sees a plain flat TIFF, which is why this file
//! opens flat everywhere else.
//!
//! **Byte order** ([`ByteOrder`]). In a little-endian (`II`, "IBM PC") file
//! Photoshop writes tag 37724's entries byte-reversed: every four-character
//! code is stored as a little-endian u32 (`MIB8`, `23rL`) and every length and
//! field in the file's order, each entry still padded to 4 (its own
//! `layered32.tif`, take1). `tiff_layers_order` is the one walk that
//! turns such a block into its big-endian form and back.
//!
//! **What is read.** The scan answers ONLY for a classic TIFF whose first
//! directory is RGB (PhotometricInterpretation 2, 3 or more samples), CMYK
//! (PhotometricInterpretation 5, InkSet absent or 1, 4 or more samples; #547
//! phase 3), gray (PhotometricInterpretation 1 BlackIsZero, 1 or more
//! samples; #547 phase 3a) or Lab (PhotometricInterpretation 8 CIELab or 9
//! ICCLab, 3 or more samples; #547 Lab phase 3a), unsigned (SampleFormat
//! absent or 1; for CIELab also [1, 2, 2] and [1, 2, 2, 1], its signed
//! a\*/b\* — the block's records store the PSD encoding whatever the flat
//! image's, `psd_import_lab`), 8 or 16 bits on every sample, big-endian
//! (`MM`), and whose tag 37724 holds the block for that depth; or an
//! RGB TIFF of 32 bits and SampleFormat 3 (IEEE float) on every sample whose
//! block is `Lr32`, in EITHER byte order — the only little-endian file read.
//! [`TiffLayers::color`] is the PSD colour mode that model is (3, 4, 1 or 9),
//! which is how the block's records are read. Palette, a 16-bit float,
//! little-endian at 8 or 16 bits and BigTIFF answer nothing, so such a file
//! opens flat exactly as it always did: the layer records carry no colour
//! model of their own. One scan serves every entry point — [`locate`] over
//! bytes already read (big-endian only, so every `psd_container::Container`
//! reader sees no `II` input), [`locate_ordered`] (both orders, for
//! `tiff_layers_order::big_endian_form`), and [`has_layers`] over a file
//! (both orders), which reads the header, the first directory and the block
//! keys and never a pixel strip.
//!
//! **What is written.** [`write_with`] lays down the header (`MM\0*`, or
//! `II*\0` for [`ByteOrder::Little`] at 32 bits only), the strip (one
//! uncompressed strip of straight RGBA — or, for `color` 4, straight CMYK ink
//! and alpha under Photometric 5 and InkSet 1 — ExtraSamples 2 = unassociated
//! alpha; at depth 32 the RGBA is IEEE float, SampleFormat 3 on every sample;
//! for `color` 1 (#547 phase 3b) ONE white-matted gray sample under
//! Photometric 1 with no ExtraSamples, as Photoshop writes its own gray
//! TIFF; for `color` 9 (#547 Lab phase 3b) three CIELab samples matted over
//! Lab white under Photometric 8 with no ExtraSamples and no SampleFormat, as
//! Photoshop writes its own Lab TIFF), the out-of-line values and ONE
//! directory, into ONE buffer of the file's exact size: the layer information
//! and the strip are appended straight into it by the caller, as
//! `psd_export_encode::write_psd32` writes a 32-bit PSD. A SHORT array is
//! inline when it fits the entry's four bytes (TIFF's rule, which `scan`
//! reads), else out of line. Every offset is a u32, so a file past 4 GiB
//! refuses. [`write`] is the big-endian form every 8- and 16-bit writer
//! calls, its samples already big-endian. [`write_spots`] (#547 1c-2b) is
//! [`write`] with an RGB or CMYK document's spot channels at 8 or 16 bits
//! as trailing extra samples after the alpha, ExtraSamples 0 each, the flat
//! TIFF's rule (`tiff_spot`). All live in `tiff_layers_write` and are
//! re-exported here.

use std::io::{Read, Seek, SeekFrom};

use crate::metadata::Resolution;

/// The fixed prefix of tag 37724's value.
pub(crate) const BLOCK_HEADER: &[u8] = b"Adobe Photoshop Document Data Block\0";

pub(crate) const TAG_RESOURCES: u16 = 34377;
pub(crate) const TAG_ICC: u16 = 34675;
pub(crate) const TAG_LAYERS: u16 = 37724;

/// A directory this large is not a photograph's; refuse rather than walk it.
const MAX_ENTRIES: usize = 4096;
/// The block walk's bound: Photoshop writes a handful of blocks.
const MAX_BLOCKS: usize = 4096;

/// A TIFF's byte order (module doc): `MM` big-endian, `II` little-endian.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ByteOrder {
    Big,
    Little,
}

impl ByteOrder {
    /// A u16 from the first two bytes; the caller guarantees them.
    pub(crate) fn u16(self, bytes: &[u8]) -> u16 {
        let pair = [bytes[0], bytes[1]];
        match self {
            Self::Big => u16::from_be_bytes(pair),
            Self::Little => u16::from_le_bytes(pair),
        }
    }

    /// A u32 from the first four bytes; the caller guarantees them.
    pub(crate) fn u32(self, bytes: &[u8]) -> u32 {
        let quad = [bytes[0], bytes[1], bytes[2], bytes[3]];
        match self {
            Self::Big => u32::from_be_bytes(quad),
            Self::Little => u32::from_le_bytes(quad),
        }
    }

    pub(crate) fn put16(self, value: u16) -> [u8; 2] {
        match self {
            Self::Big => value.to_be_bytes(),
            Self::Little => value.to_le_bytes(),
        }
    }

    pub(crate) fn put32(self, value: u32) -> [u8; 4] {
        match self {
            Self::Big => value.to_be_bytes(),
            Self::Little => value.to_le_bytes(),
        }
    }

    /// A four-character code as this order stores it: verbatim big-endian,
    /// reversed little-endian (`8BIM` → `MIB8`).
    pub(crate) fn code(self, code: [u8; 4]) -> [u8; 4] {
        match self {
            Self::Big => code,
            Self::Little => [code[3], code[2], code[1], code[0]],
        }
    }
}

/// A layered TIFF's layer information as a PSD container reads it.
pub(crate) struct TiffLayers<'a> {
    pub width: u32,
    pub height: u32,
    /// 8, 16 or 32.
    pub depth: u16,
    /// The PSD colour mode of the first directory's model: 3 RGB, 4 CMYK,
    /// 1 gray, 9 Lab.
    pub color: u16,
    /// The `Layr`/`Lr16`/`Lr32` block's body: count, records, channel data,
    /// in the file's byte order.
    pub info: &'a [u8],
    /// The whole tagged-block list after the block header, for the other
    /// document-level blocks (`psd_container::Container::tail_block`).
    pub blocks: &'a [u8],
    /// Tag 34377, empty when absent.
    pub resources: &'a [u8],
    /// Tag 34675.
    pub icc: Option<&'a [u8]>,
    /// The flat image when it is ONE strip (StripOffsets and StripByteCounts
    /// of count 1), as `tiff_layers_order`'s little-endian seam re-lays it.
    pub strip: Option<&'a [u8]>,
    /// XResolution and YResolution, when both are readable.
    pub resolution: Option<Resolution>,
}

/// Where the scan found everything, as byte ranges of the file.
struct Located {
    order: ByteOrder,
    width: u32,
    height: u32,
    depth: u16,
    color: u16,
    info: (u64, u64),
    /// Every tagged block after the header (`Layr`/`Lr16`/`Lr32` and the
    /// others).
    blocks: (u64, u64),
    resources: Option<(u64, u64)>,
    icc: Option<(u64, u64)>,
    strip: Option<(u64, u64)>,
    resolution: Option<Resolution>,
}

/// The ONE scan. `read(offset, length)` returns exactly `length` bytes of the
/// file or `None`; it is asked only for the header, the first directory, the
/// out-of-line sample descriptions and resolutions, and the block headers.
fn scan(read: &mut dyn FnMut(u64, usize) -> Option<Vec<u8>>) -> Option<Located> {
    let header = read(0, 8)?;
    let order = match &header[..4] {
        b"MM\0*" => ByteOrder::Big,
        b"II*\0" => ByteOrder::Little,
        _ => return None,
    };
    let directory = u64::from(order.u32(&header[4..]));
    let count = usize::from(order.u16(&read(directory, 2)?));
    if count > MAX_ENTRIES {
        return None;
    }
    let entries = read(directory + 2, count * 12)?;
    let (mut width, mut height) = (None, None);
    let (mut bits, mut samples, mut photometric) = (None, None, None);
    let (mut format, mut ink_set) = (None, None);
    let (mut resources, mut icc, mut block) = (None, None, None);
    let (mut strip_at, mut strip_len, mut x_at, mut y_at) = (None, None, None, None);
    for entry in entries.chunks_exact(12) {
        let (tag, kind) = (order.u16(entry), order.u16(&entry[2..]));
        let count = u64::from(order.u32(&entry[4..]));
        let value = [entry[8], entry[9], entry[10], entry[11]];
        // A SHORT (3) or LONG (4) scalar, as dimensions and enumerations are.
        let scalar = || match kind {
            3 => Some(u32::from(order.u16(&value))),
            4 => Some(order.u32(&value)),
            _ => None,
        };
        let offset = u64::from(order.u32(&value));
        // A byte-typed (BYTE 1, UNDEFINED 7) out-of-line value's range.
        let range = || matches!(kind, 1 | 7).then_some((offset, count));
        // One RATIONAL's offset.
        let rational = || (kind == 5 && count == 1).then_some(offset);
        match tag {
            256 => width = scalar(),
            257 => height = scalar(),
            258 => bits = Some((kind, count, value)),
            262 => photometric = scalar(),
            273 => strip_at = (count == 1).then(scalar).flatten(),
            277 => samples = scalar(),
            279 => strip_len = (count == 1).then(scalar).flatten(),
            282 => x_at = rational(),
            283 => y_at = rational(),
            332 => ink_set = scalar(),
            339 => format = Some((kind, count, value)),
            TAG_RESOURCES => resources = range(),
            TAG_ICC => icc = range(),
            TAG_LAYERS => block = range(),
            _ => {}
        }
    }
    // Every value of a SHORT array tag: inline for up to two, else at its
    // offset.
    let mut shorts = |spec: (u16, u64, [u8; 4])| -> Option<Vec<u16>> {
        let (kind, count, value) = spec;
        if kind != 3 || count == 0 || count > 64 {
            return None;
        }
        let bytes = if count <= 2 {
            value[..count as usize * 2].to_vec()
        } else {
            read(u64::from(order.u32(&value)), count as usize * 2)?
        };
        Some(bytes.chunks_exact(2).map(|pair| order.u16(pair)).collect())
    };
    let samples = samples?;
    let bits = shorts(bits?)?;
    let color = match photometric {
        Some(2) if samples >= 3 => 3,
        // InkSet 1 is CMYK; absent is its default.
        Some(5) if samples >= 4 && ink_set.is_none_or(|set| set == 1) => 4,
        // BlackIsZero gray (#547 phase 3a).
        Some(1) if samples >= 1 => 1,
        // CIELab or ICCLab (#547 Lab phase 3a).
        Some(8 | 9) if samples >= 3 => 9,
        _ => return None,
    };
    if bits.len() != samples as usize {
        return None;
    }
    let depth = bits[0];
    if bits.iter().any(|b| *b != depth) {
        return None;
    }
    let formats = match format {
        Some(format) => Some(shorts(format)?),
        None => None,
    };
    if depth == 32 {
        // RGB, IEEE float on every sample (an absent SampleFormat is
        // unsigned integer), in either byte order.
        let float = formats
            .as_ref()
            .is_some_and(|formats| formats.iter().all(|f| *f == 3));
        if color != 3 || !float {
            return None;
        }
    } else {
        // 8 and 16 bits are read big-endian only.
        if order != ByteOrder::Big || !matches!(depth, 8 | 16) {
            return None;
        }
        if let Some(formats) = formats {
            // CIELab's signed a*/b* (#547 Lab phase 3a): [1, 2, 2] or [1, 2, 2, 1].
            let cielab =
                photometric == Some(8) && matches!(formats.as_slice(), [1, 2, 2] | [1, 2, 2, 1]);
            if !cielab && formats.iter().any(|f| *f != 1) {
                return None;
            }
        }
    }
    let wanted = order.code(match depth {
        8 => *b"Layr",
        16 => *b"Lr16",
        _ => *b"Lr32",
    });
    let signature = order.code(*b"8BIM");
    let (start, length) = block?;
    let end = start.checked_add(length)?;
    if read(start, BLOCK_HEADER.len())? != BLOCK_HEADER {
        return None;
    }
    let mut cursor = start + BLOCK_HEADER.len() as u64;
    let blocks = (cursor, end - cursor);
    let mut info = None;
    for _ in 0..MAX_BLOCKS {
        if cursor.checked_add(12)? > end {
            break;
        }
        let head = read(cursor, 12)?;
        if head[..4] != signature {
            return None;
        }
        let body = u64::from(order.u32(&head[8..]));
        let body_start = cursor + 12;
        if body_start.checked_add(body)? > end {
            return None;
        }
        if head[4..8] == wanted {
            info = Some((body_start, body));
            break;
        }
        cursor = body_start + body + (body.wrapping_neg() & 3);
    }
    // Each axis's numerator over its denominator; a zero denominator is no
    // resolution.
    let mut ppi = |at: Option<u64>| -> Option<f32> {
        let pair = read(at?, 8)?;
        let (numerator, denominator) = (order.u32(&pair), order.u32(&pair[4..]));
        (denominator != 0).then(|| (f64::from(numerator) / f64::from(denominator)) as f32)
    };
    let resolution = match (ppi(x_at), ppi(y_at)) {
        (Some(x), Some(y)) => Some(Resolution { x, y }),
        _ => None,
    };
    Some(Located {
        order,
        width: width?,
        height: height?,
        depth,
        color,
        info: info?,
        blocks,
        resources,
        icc,
        strip: strip_at
            .zip(strip_len)
            .map(|(at, len)| (u64::from(at), u64::from(len))),
        resolution,
    })
}

/// The layer block of a TIFF already in memory in EITHER byte order, or
/// `None` when this is not a layered TIFF this crate reads (module doc).
pub(crate) fn locate_ordered(bytes: &[u8]) -> Option<(ByteOrder, TiffLayers<'_>)> {
    let slice = |range: (u64, u64)| -> Option<&[u8]> {
        let start = usize::try_from(range.0).ok()?;
        let end = start.checked_add(usize::try_from(range.1).ok()?)?;
        bytes.get(start..end)
    };
    let located = scan(&mut |offset, length| slice((offset, length as u64)).map(<[u8]>::to_vec))?;
    Some((
        located.order,
        TiffLayers {
            width: located.width,
            height: located.height,
            depth: located.depth,
            color: located.color,
            info: slice(located.info)?,
            blocks: slice(located.blocks)?,
            resources: match located.resources {
                Some(range) => slice(range)?,
                None => &[],
            },
            icc: match located.icc {
                Some(range) => Some(slice(range)?),
                None => None,
            },
            strip: located.strip.and_then(slice),
            resolution: located.resolution,
        },
    ))
}

/// The layer block of a BIG-endian TIFF already in memory, or `None` when
/// this is not a layered TIFF this crate reads or it is little-endian (module
/// doc); such a file opens flat, or (`II`, 32 bits) reaches the layered open
/// through `tiff_layers_order::big_endian_form` instead.
pub(crate) fn locate(bytes: &[u8]) -> Option<TiffLayers<'_>> {
    locate_ordered(bytes).and_then(|(order, layers)| (order == ByteOrder::Big).then_some(layers))
}

/// Whether the file at `path` is a layered TIFF this crate reads, in either
/// byte order (module doc). False for an unreadable path. Reads the header,
/// the first directory, the resolutions and the block keys; never a pixel
/// strip.
pub(crate) fn has_layers(path: &str) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(size) = file.metadata().map(|m| m.len()) else {
        return false;
    };
    let mut read = |offset: u64, length: usize| -> Option<Vec<u8>> {
        // Bounded by the file, so a forged count cannot size an allocation.
        if offset.checked_add(length as u64)? > size {
            return None;
        }
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut out = vec![0u8; length];
        file.read_exact(&mut out).ok()?;
        Some(out)
    };
    scan(&mut read).is_some_and(|located| {
        let inside = |range: (u64, u64)| range.0.checked_add(range.1).is_some_and(|e| e <= size);
        inside(located.info)
            && located.resources.is_none_or(inside)
            && located.icc.is_none_or(inside)
    })
}

// The writer lives in `tiff_layers_write`; its callers reach it here.
pub(crate) use crate::tiff_layers_write::{write, write_spots, write_with, LayeredTiff};
