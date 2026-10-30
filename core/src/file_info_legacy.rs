//! File Info's two legacy carriers: the IIM datasets inside the 8BIM
//! image-resource run and three ASCII tags of the EXIF packet's IFD0. XMP is
//! the authority (`file_info`); this module reads these as fallbacks and
//! keeps them from contradicting it. Free functions over packet bytes.
//!
//! # IIM: read, then dropped
//!
//! Resource 0x0404 of the run holds IIM datasets (`1C`, record, dataset, a
//! two-byte length, the value). Record 2 carries the four fields: 2:80
//! By-line, 2:116 Copyright Notice, 2:120 Caption/Abstract and 2:25
//! Keywords. A File Info edit DROPS those four datasets and resource 0x0425
//! (the digest of the IIM block, which is how a reader decides the IIM copy
//! is current) instead of rewriting them: IIM text has no declared encoding
//! most readers honour and a 2000-byte caption limit, so a rewritten copy
//! could only say less than the XMP beside it. A run that does not walk
//! cleanly is left exactly as it is.
//!
//! # EXIF: appended, never moved
//!
//! IFD0's ImageDescription (270), Artist (315, creators joined with "; ")
//! and Copyright (33432) are set as ASCII, or removed when the field is
//! empty. An EXIF block is full of offsets measured from its own header
//! (`metadata_write`'s module doc), and adding an entry to IFD0 in place
//! would shift every datum after it. So nothing moves: the new strings and a
//! REWRITTEN IFD0 (its other entries copied as they stand, its next-IFD link
//! copied) are appended at the end of the packet and the header's IFD0
//! offset is repointed. The old IFD0 becomes unreferenced bytes.
//!
//! Three rules keep that from costing anything on a repeat edit or an
//! export:
//!
//! * a packet whose three tags already hold the wanted values is returned
//!   untouched;
//! * **tail reuse** — when IFD0's link field ends the packet and the bytes
//!   just before IFD0 are exactly the out-of-line values of those three tags
//!   (which is the tail this module writes), the packet is cut there before
//!   the new tail is written, so a second edit replaces the first instead of
//!   stacking on it. The cut is taken only when no other datum reachable from
//!   IFD0 (its own values, the Exif, GPS and Interop IFDs, IFD1 and its
//!   thumbnail) reaches into the part that goes;
//! * **the JPEG bound** — a JPEG APP1 holds `JPEG_MAX_PAYLOAD` bytes and the
//!   export drops an EXIF packet that does not fit
//!   (`doc_color::image_sidecar_with_policy`). When the appended result would
//!   pass that bound and the stored packet does not, no string is written:
//!   the three tags are removed from the stored IFD0 IN PLACE (the kept
//!   entries closed up, the count lowered, the link rewritten after the last
//!   kept entry, the length unchanged) and XMP alone carries the fields. An
//!   edit therefore never makes a JPEG export drop an EXIF packet it carried
//!   before.
//!
//! A packet that does not parse is left as it is.

use std::ops::Range;

use crate::file_info::{FileInfo, Found};
use crate::metadata_write::{JPEG_EXIF_PREFIX, JPEG_MAX_PAYLOAD};
use crate::tiff_view::{Entry, Tiff, MAX_IFD_ENTRIES, TAG_EXIF_IFD};

/// The 8BIM resource holding the IIM datasets, and the one holding their
/// digest.
const IIM_RESOURCE: u16 = 0x0404;
const IIM_DIGEST_RESOURCE: u16 = 0x0425;

/// IIM record 2 (the application record) and its four datasets.
const IIM_APPLICATION: u8 = 2;
const IIM_KEYWORDS: u8 = 25;
const IIM_BYLINE: u8 = 80;
const IIM_COPYRIGHT: u8 = 116;
const IIM_CAPTION: u8 = 120;

/// IFD0's three text tags, in tag order, and the pointers to the other IFDs
/// an EXIF packet holds.
const TAG_DESCRIPTION: u16 = 270;
const TAG_ARTIST: u16 = 315;
const TAG_COPYRIGHT: u16 = 33432;
const TAG_GPS_IFD: u16 = 34853;
const TAG_INTEROP_IFD: u16 = 40965;
const TAG_THUMBNAIL_OFFSET: u16 = 513;
const TAG_THUMBNAIL_LENGTH: u16 = 514;
const TYPE_ASCII: u16 = 2;

/// What separates creators in the one Artist string.
const ARTIST_SEPARATOR: &str = "; ";

/// Legacy text as a string: UTF-8 when the bytes are valid UTF-8, else
/// Latin-1 (every byte its own code point), with trailing NULs trimmed.
/// Neither carrier reliably declares an encoding, and those two cover what
/// writers put there.
fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |at| at + 1);
    let bytes = &bytes[..end];
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

// ------------------------------------------------------------------ IIM --

/// One 8BIM image resource: its id, everything before its size field, and
/// its data without the pad byte.
struct Resource<'a> {
    id: u16,
    head: &'a [u8],
    data: &'a [u8],
    whole: &'a [u8],
}

/// The resources of an 8BIM run, or `None` when the run is malformed. The
/// same walk as `metadata_write::iptc_filtered`, which is the master copy:
/// `'8BIM'`, a u16 id, a Pascal name whose whole field is padded to an even
/// length, a u32 size, data padded to an even length.
fn resources(run: &[u8]) -> Option<Vec<Resource<'_>>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < run.len() {
        if run.len() - i < 12 || &run[i..i + 4] != b"8BIM" {
            return None;
        }
        let id = u16::from_be_bytes([run[i + 4], run[i + 5]]);
        let name_field = (1 + usize::from(run[i + 6])).next_multiple_of(2);
        let size_at = i.checked_add(6)?.checked_add(name_field)?;
        if size_at.checked_add(4)? > run.len() {
            return None;
        }
        let size = u32::from_be_bytes([
            run[size_at],
            run[size_at + 1],
            run[size_at + 2],
            run[size_at + 3],
        ]) as usize;
        let data_at = size_at + 4;
        let end = data_at.checked_add(size)?.checked_add(size % 2)?;
        if end > run.len() {
            return None;
        }
        out.push(Resource {
            id,
            head: &run[i..size_at],
            data: &run[data_at..data_at + size],
            whole: &run[i..end],
        });
        i = end;
    }
    Some(out)
}

/// One IIM dataset: its record and dataset numbers, its whole extent and
/// its value, both as ranges of the resource's data.
struct Dataset {
    record: u8,
    number: u8,
    whole: Range<usize>,
    value: Range<usize>,
}

/// The datasets of an IIM block, or `None` when it is malformed. A dataset
/// is the tag marker `1C`, a record number, a dataset number and a length:
/// two bytes, or with the top bit set the count (1 to 4) of length bytes
/// that follow. Trailing zero bytes are padding (a TIFF stores the block in
/// LONGs) and end the walk.
fn datasets(data: &[u8]) -> Option<Vec<Dataset>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        if data[i..].iter().all(|&b| b == 0) {
            break;
        }
        if data.len() - i < 5 || data[i] != 0x1C {
            return None;
        }
        let short = usize::from(u16::from_be_bytes([data[i + 3], data[i + 4]]));
        let (header, length) = if short & 0x8000 == 0 {
            (5usize, short)
        } else {
            let count = short & 0x7FFF;
            if !(1..=4).contains(&count) {
                return None;
            }
            let bytes = data.get(i + 5..i + 5 + count)?;
            let length = bytes
                .iter()
                .fold(0usize, |sum, &b| (sum << 8) | usize::from(b));
            (5 + count, length)
        };
        let start = i.checked_add(header)?;
        let end = start.checked_add(length)?;
        if end > data.len() {
            return None;
        }
        out.push(Dataset {
            record: data[i + 1],
            number: data[i + 2],
            whole: i..end,
            value: start..end,
        });
        i = end;
    }
    Some(out)
}

fn is_field_dataset(dataset: &Dataset) -> bool {
    dataset.record == IIM_APPLICATION
        && matches!(
            dataset.number,
            IIM_KEYWORDS | IIM_BYLINE | IIM_COPYRIGHT | IIM_CAPTION
        )
}

/// The four fields as the run's IIM datasets state them. A malformed run or
/// block states nothing.
pub(crate) fn iim_fields(run: &[u8]) -> Found {
    let mut found = Found::default();
    let Some(resources) = resources(run) else {
        return found;
    };
    for resource in resources.iter().filter(|r| r.id == IIM_RESOURCE) {
        let Some(datasets) = datasets(resource.data) else {
            continue;
        };
        for dataset in datasets.iter().filter(|d| d.record == IIM_APPLICATION) {
            let value = text(&resource.data[dataset.value.clone()]);
            match dataset.number {
                // Repeatable datasets: one per creator, one per keyword.
                IIM_BYLINE => found.creator.get_or_insert_with(Vec::new).push(value),
                IIM_KEYWORDS => found.keywords.get_or_insert_with(Vec::new).push(value),
                IIM_COPYRIGHT if found.copyright.is_none() => found.copyright = Some(value),
                IIM_CAPTION if found.description.is_none() => found.description = Some(value),
                _ => {}
            }
        }
    }
    found
}

/// The run without its four File Info datasets and without the IIM digest
/// resource; an IIM resource left with no dataset goes too. `None` when the
/// run or its IIM block is malformed, which leaves the run as it is. A run
/// holding none of them comes back byte for byte.
pub(crate) fn iim_dropped(run: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(run.len());
    for resource in resources(run)? {
        match resource.id {
            IIM_DIGEST_RESOURCE => {}
            IIM_RESOURCE => {
                let datasets = datasets(resource.data)?;
                if !datasets.iter().any(is_field_dataset) {
                    out.extend_from_slice(resource.whole);
                    continue;
                }
                let mut data = Vec::with_capacity(resource.data.len());
                let mut cursor = 0usize;
                for dataset in datasets.iter().filter(|d| is_field_dataset(d)) {
                    data.extend_from_slice(&resource.data[cursor..dataset.whole.start]);
                    cursor = dataset.whole.end;
                }
                // What follows the last dropped dataset, minus any padding.
                let rest = &resource.data[cursor..];
                if rest.iter().any(|&b| b != 0) {
                    data.extend_from_slice(rest);
                }
                if data.is_empty() {
                    continue;
                }
                out.extend_from_slice(resource.head);
                out.extend_from_slice(&u32::try_from(data.len()).ok()?.to_be_bytes());
                out.extend_from_slice(&data);
                if !data.len().is_multiple_of(2) {
                    out.push(0);
                }
            }
            _ => out.extend_from_slice(resource.whole),
        }
    }
    Some(out)
}

// ----------------------------------------------------------------- EXIF --

fn is_field_tag(tag: u16) -> bool {
    matches!(tag, TAG_DESCRIPTION | TAG_ARTIST | TAG_COPYRIGHT)
}

/// The bytes of an ASCII entry, its terminating NUL included.
fn ascii_bytes<'a>(t: &Tiff<'a>, e: &Entry) -> Option<&'a [u8]> {
    if e.kind != TYPE_ASCII {
        return None;
    }
    let count = usize::try_from(e.count).ok()?;
    let start = t.data_range(e)?.start;
    t.bytes.get(start..start.checked_add(count)?)
}

/// An ASCII value without its trailing NULs.
fn trimmed(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |at| at + 1);
    &bytes[..end]
}

/// The three fields IFD0 states. A packet that does not parse states
/// nothing; keywords have no EXIF tag.
pub(crate) fn exif_fields(blob: &[u8]) -> Found {
    let mut found = Found::default();
    let Some(t) = Tiff::new(blob) else {
        return found;
    };
    let Some((entries, _)) = t.ifd(t.ifd0) else {
        return found;
    };
    let ascii = |tag: u16| {
        entries
            .iter()
            .find(|e| e.tag == tag)
            .and_then(|e| ascii_bytes(&t, e))
            .map(text)
    };
    found.description = ascii(TAG_DESCRIPTION);
    found.copyright = ascii(TAG_COPYRIGHT);
    found.creator = ascii(TAG_ARTIST).map(|artist| {
        artist
            .split(';')
            .map(|name| name.trim().to_string())
            .collect()
    });
    found
}

/// The EXIF packet with IFD0's three text tags brought to `info` (see the
/// module doc), or `None` when the packet is to be left as it is: it does
/// not parse, it already says exactly this, or the JPEG bound rules out the
/// strings and there is no tag to remove.
pub(crate) fn exif_written(blob: &[u8], info: &FileInfo) -> Option<Vec<u8>> {
    let t = Tiff::new(blob)?;
    let (entries, link) = t.ifd(t.ifd0)?;
    let wanted: [(u16, Vec<u8>); 3] = [
        (TAG_DESCRIPTION, info.description.clone().into_bytes()),
        (TAG_ARTIST, info.creator.join(ARTIST_SEPARATOR).into_bytes()),
        (TAG_COPYRIGHT, info.copyright.clone().into_bytes()),
    ];
    let holds = wanted.iter().all(
        |(tag, bytes)| match entries.iter().find(|e| e.tag == *tag) {
            None => bytes.is_empty(),
            Some(e) => {
                !bytes.is_empty() && ascii_bytes(&t, e).map(trimmed) == Some(bytes.as_slice())
            }
        },
    );
    if holds {
        return None;
    }
    let appended = exif_appended(&t, &entries, link, &wanted)?;
    if appended.len() + JPEG_EXIF_PREFIX > JPEG_MAX_PAYLOAD
        && blob.len() + JPEG_EXIF_PREFIX <= JPEG_MAX_PAYLOAD
    {
        return exif_without(&t, &entries, link);
    }
    Some(appended)
}

/// One classic IFD entry as its twelve stored bytes.
fn record<'a>(t: &Tiff<'a>, e: &Entry) -> Option<&'a [u8]> {
    t.bytes
        .get(e.field.checked_sub(8)?..e.field.checked_add(4)?)
}

/// The packet with the strings and a rewritten IFD0 at its end, and the
/// header repointed. Entries stay in tag order, which TIFF requires.
fn exif_appended(
    t: &Tiff<'_>,
    entries: &[Entry],
    link: usize,
    wanted: &[(u16, Vec<u8>); 3],
) -> Option<Vec<u8>> {
    let blob = t.bytes;
    let cut = reusable_tail(t, entries, link).unwrap_or(blob.len());
    let mut out = blob[..cut].to_vec();
    // TIFF offsets are word-aligned.
    let align = |out: &mut Vec<u8>| {
        if !out.len().is_multiple_of(2) {
            out.push(0);
        }
    };
    align(&mut out);
    let mut records: Vec<(u16, [u8; 12])> = Vec::with_capacity(entries.len() + 3);
    for e in entries.iter().filter(|e| !is_field_tag(e.tag)) {
        records.push((e.tag, record(t, e)?.try_into().ok()?));
    }
    for (tag, bytes) in wanted.iter().filter(|(_, bytes)| !bytes.is_empty()) {
        // The count includes the terminating NUL.
        let count = bytes.len() + 1;
        let mut entry = [0u8; 12];
        entry[0..2].copy_from_slice(&t.u16_bytes(*tag));
        entry[2..4].copy_from_slice(&t.u16_bytes(TYPE_ASCII));
        entry[4..8].copy_from_slice(&t.u32_bytes(u32::try_from(count).ok()?));
        if count <= 4 {
            entry[8..8 + bytes.len()].copy_from_slice(bytes);
        } else {
            entry[8..12].copy_from_slice(&t.u32_bytes(u32::try_from(out.len()).ok()?));
            out.extend_from_slice(bytes);
            out.push(0);
            align(&mut out);
        }
        records.push((*tag, entry));
    }
    // A stable sort: the copied entries were in order already.
    records.sort_by_key(|(tag, _)| *tag);
    if records.len() > MAX_IFD_ENTRIES {
        return None;
    }
    let ifd0 = u32::try_from(out.len()).ok()?;
    out.extend_from_slice(&t.u16_bytes(records.len() as u16));
    for (_, entry) in &records {
        out.extend_from_slice(entry);
    }
    out.extend_from_slice(blob.get(link..link.checked_add(4)?)?);
    out.get_mut(4..8)?.copy_from_slice(&t.u32_bytes(ifd0));
    Some(out)
}

/// Where the packet may be cut before a new tail is written: the start of
/// the three tags' out-of-line values when they sit directly before an IFD0
/// whose link field ends the packet (a pad byte may separate them), and
/// nothing else reachable from IFD0 reaches past that point. `None` when the
/// packet has no such tail, or when it cannot be shown that nothing else
/// lives there.
fn reusable_tail(t: &Tiff<'_>, entries: &[Entry], link: usize) -> Option<usize> {
    let blob = t.bytes;
    if link.checked_add(4)? != blob.len() {
        return None;
    }
    let mut own: Vec<Range<usize>> = entries
        .iter()
        .filter(|e| is_field_tag(e.tag))
        .filter_map(|e| out_of_line(t, e))
        .collect();
    let mut cut = t.ifd0;
    while let Some(index) = own.iter().position(|r| {
        r.end == cut || (r.end.checked_add(1) == Some(cut) && blob.get(r.end) == Some(&0))
    }) {
        cut = own.swap_remove(index).start;
    }
    let foreign = foreign_ranges(t, entries, link)?;
    (cut >= 8 && foreign.iter().all(|r| r.end <= cut)).then_some(cut)
}

/// An entry's data range when it is stored out of line.
fn out_of_line(t: &Tiff<'_>, e: &Entry) -> Option<Range<usize>> {
    let range = t.data_range(e)?;
    (range.start != e.field && !range.is_empty()).then_some(range)
}

/// Every byte range reachable from IFD0 other than IFD0 itself and its
/// three text tags' values: the out-of-line values of its other entries;
/// the Exif, GPS and Interop IFDs with their values; IFD1 with its values
/// and its thumbnail. `None` when one of those directories does not parse,
/// so nothing can be said about what it reaches.
fn foreign_ranges(t: &Tiff<'_>, entries: &[Entry], link: usize) -> Option<Vec<Range<usize>>> {
    let mut out: Vec<Range<usize>> = entries
        .iter()
        .filter(|e| !is_field_tag(e.tag))
        .filter_map(|e| out_of_line(t, e))
        .collect();
    let directory = |at: usize, out: &mut Vec<Range<usize>>| -> Option<Vec<Entry>> {
        let (entries, link) = t.ifd(at)?;
        out.push(at..link.checked_add(4)?);
        out.extend(entries.iter().filter_map(|e| out_of_line(t, e)));
        Some(entries)
    };
    let pointer = |entries: &[Entry], tag: u16| {
        entries
            .iter()
            .find(|e| e.tag == tag)
            .map(|e| t.scalar(e).map(|at| at as usize))
    };
    if let Some(at) = pointer(entries, TAG_EXIF_IFD) {
        let sub = directory(at?, &mut out)?;
        if let Some(at) = pointer(&sub, TAG_INTEROP_IFD) {
            directory(at?, &mut out)?;
        }
    }
    if let Some(at) = pointer(entries, TAG_GPS_IFD) {
        directory(at?, &mut out)?;
    }
    let next = t.u32_at(link)? as usize;
    if next != 0 {
        let ifd1 = directory(next, &mut out)?;
        let scalar = |tag: u16| {
            ifd1.iter()
                .find(|e| e.tag == tag)
                .and_then(|e| t.scalar(e))
                .map(|value| value as usize)
        };
        if let (Some(at), Some(length)) =
            (scalar(TAG_THUMBNAIL_OFFSET), scalar(TAG_THUMBNAIL_LENGTH))
        {
            out.push(at..at.checked_add(length)?);
        }
    }
    Some(out)
}

/// The packet with the three text tags removed from IFD0 in place: the kept
/// entries closed up, the count lowered, the link rewritten after the last
/// kept entry and the freed bytes zeroed. Same length, no offset moved.
/// `None` when IFD0 holds none of the three.
fn exif_without(t: &Tiff<'_>, entries: &[Entry], link: usize) -> Option<Vec<u8>> {
    let kept: Vec<&Entry> = entries.iter().filter(|e| !is_field_tag(e.tag)).collect();
    if kept.len() == entries.len() {
        return None;
    }
    let blob = t.bytes;
    let end = link.checked_add(4)?;
    let mut table = Vec::with_capacity(end - t.ifd0);
    table.extend_from_slice(&t.u16_bytes(kept.len() as u16));
    for e in kept {
        table.extend_from_slice(record(t, e)?);
    }
    table.extend_from_slice(blob.get(link..end)?);
    table.resize(end - t.ifd0, 0);
    let mut out = blob.to_vec();
    out.get_mut(t.ifd0..end)?.copy_from_slice(&table);
    Some(out)
}
