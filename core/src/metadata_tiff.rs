//! TIFF capture metadata (classic or BigTIFF), separated from image layout.
//! A TIFF's EXIF pointers address the entire image file, so copying its IFD
//! bytes as an EXIF packet would retain stale strip offsets and broken
//! pointers. We relocate a bounded graph of standard IFD0/EXIF/GPS/Interop
//! values into a classic EXIF packet; a BigTIFF file is read through
//! `tiff_view::Tiff::file`, while EXIF blobs (and so export) stay classic.
//! A MakerNote (37500, as BYTE or UNDEFINED) relocates as ONE OPAQUE VALUE,
//! byte for byte: its maker's own structure is never read, so a note
//! that addresses itself from its own start survives a TIFF open and a TIFF
//! export, and one that holds absolute file offsets is as stale as it is
//! after any other program moves it. Other private EXIF tags can hide
//! undocumented offsets and have no such convention: import omits them with
//! a report; TIFF export refuses the entire EXIF packet if relocation would
//! lose one. JPEG/PNG retain their existing verbatim-packet policy. No pixel
//! storage or precision is involved here.
//!
//! bounded TIFF (classic or BigTIFF) capture-IFD relocation (standard IFD0/EXIF/GPS/Interop, and a
//! MakerNote as one opaque value), ICC/PPI/XMP/IPTC extraction and typed TIFF encoding. Other
//! private capture fields are disclosed on import and refuse the entire EXIF packet on export.

use std::collections::HashSet;
use std::io::{Cursor, Seek, Write};
use std::sync::Arc;

use tiff::encoder::{DirectoryEncoder, TiffEncoder, TiffKindStandard};
use tiff::tags::{Tag, Type};
use tiff::Directory;

use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::metadata::Scan;
use crate::tiff_view::{resolution_of, type_size, Entry, Tiff};

const ICC: u16 = 34675;
const XMP: u16 = 700;
const IPTC: u16 = 33723;
const PHOTOSHOP: u16 = 34377;
const EXIF: u16 = 34665;
const GPS: u16 = 34853;
const INTEROP: u16 = 40965;
const CAP: usize = MAX_DRDC_BLOB_LEN as usize;

/// Classic (`*`, 42) or BigTIFF (`+`, 43) in either byte order. Applied to
/// whole files only: an EXIF blob is always classic.
pub(crate) fn is_tiff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"II\x2a\0")
        || bytes.starts_with(b"MM\0\x2a")
        || bytes.starts_with(b"II\x2b\0")
        || bytes.starts_with(b"MM\0\x2b")
}

#[derive(Default)]
pub(crate) struct Node {
    values: Vec<(u16, u16, Vec<u8>)>,
    children: Vec<(u16, Node)>,
}

#[derive(Clone, Copy)]
enum Kind {
    Root,
    Exif,
    Gps,
    Interop,
}

/// Standard root fields that describe capture rather than TIFF storage.
/// Layout, compression, previews, ICC and XMP live elsewhere and are never
/// serialized into an EXIF packet.
fn root_field(tag: u16) -> bool {
    matches!(
        tag,
        256 | 257
            | 269
            | 270
            | 271
            | 272
            | 274
            | 282
            | 283
            | 285
            | 296
            | 305
            | 306
            | 315
            | 316
            | 33432
    )
}

/// The Exif IFD's MakerNote, carried as one opaque value.
const MAKER_NOTE: u16 = 37500;

/// Whether a field relocates. `field_type` matters for the MakerNote alone:
/// only its two byte-array types (BYTE 1, UNDEFINED 7) are opaque bytes that
/// can be copied verbatim.
fn supported(kind: Kind, tag: u16, field_type: u16) -> bool {
    match kind {
        Kind::Root => root_field(tag),
        Kind::Gps => tag <= 31,
        Kind::Interop => matches!(tag, 1 | 2 | 4096..=4098),
        Kind::Exif if tag == MAKER_NOTE => matches!(field_type, 1 | 7),
        Kind::Exif => matches!(tag, 33434 | 33437 | 34850 | 34852 | 34855 | 34856 |
            34864..=34869 | 36864 | 36867..=36870 | 37121 | 37122 | 37377..=37386 |
            37396..=37401 | 37510 | 37520..=37522 | 37888..=37893 | 40960..=40964 |
            41483 | 41484 | 41486..=41488 | 41492..=41495 | 41728..=41730 |
            41985..=41996 | 42016 | 42032..=42038 | 42080..=42082 | 42240),
    }
}

fn child_kind(kind: Kind, tag: u16) -> Option<Kind> {
    match (kind, tag) {
        (Kind::Root, EXIF) => Some(Kind::Exif),
        (Kind::Root, GPS) => Some(Kind::Gps),
        (Kind::Exif, INTEROP) => Some(Kind::Interop),
        _ => None,
    }
}

/// Reads the exact typed payload, excluding inline padding. Multi-byte
/// numbers are converted to native order once for the typed TIFF encoder.
/// A RATIONAL contains two 32-bit numbers, not one 64-bit number.
fn datum(t: &Tiff<'_>, e: &Entry) -> Option<Vec<u8>> {
    let size = type_size(e.kind, t.big)?.checked_mul(usize::try_from(e.count).ok()?)?;
    if size > CAP {
        return None;
    }
    let range = t.data_range(e)?;
    let mut out = t
        .bytes
        .get(range.start..range.start.checked_add(size)?)?
        .to_vec();
    if t.big_endian != cfg!(target_endian = "big") {
        let unit = match e.kind {
            3 | 8 => 2,
            4 | 5 | 9 | 10 | 11 => 4,
            12 => 8,
            _ => 1,
        };
        for item in out.chunks_exact_mut(unit) {
            item.reverse();
        }
    }
    Some(out)
}

struct Walk {
    seen: HashSet<usize>,
    bytes: usize,
    dropped: bool,
}

fn node(t: &Tiff<'_>, at: usize, kind: Kind, walk: &mut Walk, strict: bool) -> Option<Node> {
    // Only four kinds of directory are supported. Eight admits independent
    // standard children while bounding cycles and aliasing before recursion.
    if walk.seen.len() >= 8 || !walk.seen.insert(at) {
        return None;
    }
    let (entries, _) = t.ifd(at)?;
    let mut out = Node::default();
    let mut tags = HashSet::new();
    for e in entries {
        if !tags.insert(e.tag) {
            return None;
        }
        if let Some(kind) = child_kind(kind, e.tag) {
            let child = node(t, t.pointer(&e)?, kind, walk, strict)?;
            out.children.push((e.tag, child));
        } else if supported(kind, e.tag, e.kind) {
            // The 64-bit and IFD types are sizable only in a BigTIFF, and the
            // classic EXIF packet writes types 1..12 only: such a value is
            // dropped and disclosed, never relocated.
            if t.big && matches!(e.kind, 13 | 16..=18) {
                walk.dropped = true;
                if strict {
                    return None;
                }
                continue;
            }
            let size = type_size(e.kind, t.big)?.checked_mul(usize::try_from(e.count).ok()?)?;
            walk.bytes = walk.bytes.checked_add(size)?.checked_add(12)?;
            if walk.bytes > CAP {
                return None;
            }
            out.values.push((e.tag, e.kind, datum(t, &e)?));
        } else {
            // Unsupported root TIFF fields are normally image layout;
            // private fields and unknown EXIF/GPS fields merit disclosure.
            let unsupported = !matches!(kind, Kind::Root)
                || (e.tag >= 32768 && !matches!(e.tag, ICC | IPTC | PHOTOSHOP));
            walk.dropped |= unsupported;
            if strict {
                return None;
            }
        }
    }
    Some(out)
}

/// Strict relocation preflight shared by TIFF export and preservation query.
pub(crate) fn export_exif(blob: &[u8]) -> Option<Node> {
    let t = Tiff::new(blob)?;
    node(
        &t,
        t.ifd0,
        Kind::Root,
        &mut Walk {
            seen: HashSet::new(),
            bytes: 8,
            dropped: false,
        },
        true,
    )
}

pub(crate) fn write_values<W: Write + Seek>(
    writer: &mut DirectoryEncoder<'_, W, TiffKindStandard>,
    node: &Node,
    children: &[(u16, u32)],
) -> tiff::TiffResult<()> {
    let mut directory = Directory::empty();
    for (tag, kind, bytes) in &node.values {
        // Parsing accepted only types 1..12, all defined by the TIFF crate.
        let ty = Type::from_u16(*kind).ok_or_else(|| {
            tiff::TiffError::IoError(std::io::Error::other("unsupported capture field type"))
        })?;
        let entry = writer.write_entry_bytes(ty, bytes)?;
        directory.extend([(Tag::from_u16_exhaustive(*tag), entry)]);
    }
    writer.extend_from(&directory);
    for (tag, offset) in children {
        writer.write_tag(Tag::from_u16_exhaustive(*tag), *offset)?;
    }
    Ok(())
}

pub(crate) fn write_children<W: Write + Seek>(
    encoder: &mut TiffEncoder<W>,
    node: &Node,
) -> tiff::TiffResult<Vec<(u16, u32)>> {
    let mut children = Vec::new();
    for (tag, child) in &node.children {
        let descendants = write_children(encoder, child)?;
        let mut directory = encoder.extra_directory()?;
        write_values(&mut directory, child, &descendants)?;
        children.push((*tag, directory.finish_with_offsets()?.offset));
    }
    Ok(children)
}

fn packet(node: &Node) -> Option<Vec<u8>> {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut encoder = TiffEncoder::new(&mut buffer).ok()?;
        let children = write_children(&mut encoder, node).ok()?;
        let mut root = encoder.image_directory().ok()?;
        write_values(&mut root, node, &children).ok()?;
        root.finish().ok()?;
    }
    let bytes = buffer.into_inner();
    (bytes.len() <= CAP).then_some(bytes)
}

/// TIFF scan and diagnostics use one walk. ICC is extracted independently
/// because image 0.25's TIFF adapter does not surface its own written tag.
pub(crate) fn scan(bytes: &[u8]) -> (Scan, Vec<&'static str>) {
    let mut scan = Scan::default();
    let mut warnings = Vec::new();
    let Some(t) = Tiff::file(bytes) else {
        return (scan, vec!["TIFF metadata header is malformed"]);
    };
    let Some((entries, link)) = t.ifd(t.ifd0) else {
        return (
            scan,
            vec!["TIFF metadata directory is malformed or exceeds the entry limit"],
        );
    };
    scan.resolution = resolution_of(&t);
    let get = |tag| entries.iter().find(|e| e.tag == tag);
    for (tag, slot, label) in [
        (
            XMP,
            &mut scan.metadata.xmp,
            "TIFF XMP exceeds the packet limit or is malformed",
        ),
        (
            PHOTOSHOP,
            &mut scan.metadata.iptc,
            "TIFF IPTC exceeds the packet limit or is malformed",
        ),
    ] {
        if let Some(e) = get(tag) {
            if matches!(e.kind, 1 | 7) {
                *slot = datum(&t, e).map(Arc::from);
            }
            if slot.is_none() {
                warnings.push(label);
            }
        }
    }
    if let Some(e) = get(ICC) {
        if matches!(e.kind, 1 | 7) {
            scan.icc = datum(&t, e);
        }
        if scan.icc.is_none() {
            warnings.push("TIFF ICC profile exceeds the packet limit or is malformed");
        }
    }
    if scan.metadata.iptc.is_none() {
        if let Some(e) = get(IPTC) {
            // RichTIFFIPTC uses BYTE or LONG arrays. LONG payload bytes are
            // IIM bytes, not TIFF numbers; libtiff stores their on-disk order.
            let data = t.data_range(e).and_then(|r| {
                let count =
                    type_size(e.kind, t.big)?.checked_mul(usize::try_from(e.count).ok()?)?;
                (count <= CAP.saturating_sub(12)).then_some(())?;
                bytes.get(r.start..r.start.checked_add(count)?)
            });
            if let Some(data) = data.filter(|_| matches!(e.kind, 1 | 4 | 7)) {
                let mut run = b"8BIM\x04\x04\0\0".to_vec();
                run.extend_from_slice(&(data.len() as u32).to_be_bytes());
                run.extend_from_slice(data);
                if data.len() % 2 != 0 {
                    run.push(0);
                }
                scan.metadata.iptc = Some(Arc::from(run));
            } else {
                warnings.push("TIFF IPTC exceeds the packet limit or is malformed");
            }
        }
    }
    let mut walk = Walk {
        seen: HashSet::new(),
        bytes: 8,
        dropped: false,
    };
    match node(&t, t.ifd0, Kind::Root, &mut walk, false) {
        Some(root) => {
            scan.metadata.exif = packet(&root).map(Arc::from);
            if scan.metadata.exif.is_none() {
                warnings.push("TIFF EXIF exceeds the packet limit");
            }
        }
        None => warnings.push("TIFF EXIF is malformed or exceeds the metadata limits"),
    }
    if walk.dropped {
        warnings.push("TIFF private capture fields were not retained");
    }
    if t.offset_at(link).is_some_and(|next| next != 0) {
        warnings.push("Only the first TIFF image is imported; additional images or thumbnails are not retained");
    }
    (scan, warnings)
}

/// Orientation is a scalar TIFF tag; this supplements the image adapter's
/// optional orientation implementation without interpreting EXIF twice.
/// For an EXIF blob, which is always classic.
pub(crate) fn orientation(blob: &[u8]) -> Option<image::metadata::Orientation> {
    orientation_in(&Tiff::new(blob)?)
}

/// [`orientation`] for a whole TIFF file, classic or BigTIFF.
pub(crate) fn orientation_of_file(bytes: &[u8]) -> Option<image::metadata::Orientation> {
    orientation_in(&Tiff::file(bytes)?)
}

fn orientation_in(t: &Tiff<'_>) -> Option<image::metadata::Orientation> {
    let (entries, _) = t.ifd(t.ifd0)?;
    let e = entries.iter().find(|e| e.tag == 274 && e.count == 1)?;
    image::metadata::Orientation::from_exif(t.scalar(e)?.try_into().ok()?)
}
