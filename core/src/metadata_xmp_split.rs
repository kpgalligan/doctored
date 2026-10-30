//! The WRITE half of JPEG ExtendedXMP: an XMP packet too large for one APP1
//! split into a standard packet that fits, pointing at an extended packet
//! carried in `http://ns.adobe.com/xmp/extension/` chunks — the form
//! Photoshop and Lightroom write, and the form `metadata_xmp_extended`
//! reads back. Before this, an oversize packet was dropped whole.
//!
//! # What moves
//!
//! XMP Part 3 leaves the choice to the writer and recommends keeping the
//! small, commonly read properties in the standard packet, where every
//! reader that ignores ExtendedXMP still finds them. So whole PROPERTIES
//! move, never bytes: a Description's property attributes (everything but
//! namespace declarations, `rdf:*` and `xml:lang`) and its child elements,
//! `photoshop:History` first (Adobe's own first choice — a long, rarely
//! read edit log), then the largest first, ties in document order, one at a
//! time until the standard packet fits [`STANDARD_LIMIT`]. A small `dc:`
//! title or creator therefore stays where a plain reader looks. A
//! Description other than the first that loses every property is removed
//! from the standard packet whole, so export→reopen→export is stable.
//!
//! The extension is a complete packet of its own: one
//! `rdf:Description` per source Description that lost a property, carrying
//! its `rdf:about` and every namespace declaration and `xml:lang` in scope
//! where the property sat (each name once, the Description's own winning
//! over an inherited one, exactly as `xmp_rdf::Scope` resolves them), so
//! every moved property means in the extension what it meant at home.
//!
//! # The note and the GUID
//!
//! The standard packet's first Description gets
//! `xmpNote:HasExtendedXMP="<GUID>"`, the GUID being the uppercase MD5 of
//! the extended packet (`md5`); every note the packet already carried is
//! dropped first, so an export never states two. A prefix already bound to
//! the note namespace in scope is reused; otherwise `xmlns:xmpNote` is
//! declared, unless `xmpNote` is already bound in scope (on that
//! Description, `rdf:RDF` or `x:xmpmeta`) to something else, which refuses
//! the split.
//!
//! # When it refuses
//!
//! `None` — and the caller then drops the XMP and reports it dropped, as
//! before — when the packet does not scan (`xmp_rdf`), when moving every
//! property still leaves the standard packet too large, when the extension
//! would not fit a u32 length, or when the two together pass the DRDC blob
//! cap: the writer enforces the reader's cap, so every file it writes merges
//! back on open.
//!
//! `metadata_xmp_split` splits an oversize packet on JPEG export into a ≤ 65 504-byte standard
//! packet with `xmpNote:HasExtendedXMP` plus chunks (`EncodeSidecar.jpeg_xmp`, built once in
//! `doc_color`).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::md5::md5;
use crate::metadata::{XMP_EXT_ID, XMP_ID};
use crate::metadata_write::{jpeg_app1_xmp, jpeg_segment, JPEG_MAX_PAYLOAD};
use crate::metadata_xmp_extended::{note_spans, NOTE_NS};
use crate::xmp_rdf::{self, Description, Rdf, Scope};

/// The largest standard packet: one APP1 payload minus its identifier,
/// 65 533 − 29 = 65 504.
pub(crate) const STANDARD_LIMIT: usize = JPEG_MAX_PAYLOAD - XMP_ID.len();

/// Extension bytes per chunk: one APP1 payload minus the 35-byte identifier,
/// the 32-digit GUID and the two u32s, 65 533 − 35 − 32 − 8 = 65 458.
pub(crate) const CHUNK_DATA: usize = JPEG_MAX_PAYLOAD - XMP_EXT_ID.len() - 32 - 8;

/// `photoshop:History`'s namespace.
const PHOTOSHOP_NS: &[u8] = b"http://ns.adobe.com/photoshop/1.0/";

/// The extension's opening and closing wrapper (no `<?xpacket?>`: XMP Part
/// 3 leaves it out of an extended packet).
const EXTENSION_OPEN: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">";
const EXTENSION_CLOSE: &[u8] = b"</rdf:RDF></x:xmpmeta>";

/// Complete JPEG APP1 segments (FF E1 + length + identifier + payload) for
/// `packet`: the standard segment first, then the extension chunks by
/// ascending offset. One segment, byte-identical to `jpeg_app1_xmp`, when
/// the packet fits; `None` when it can be neither written whole nor split
/// (see the module doc).
pub(crate) fn jpeg_xmp_segments(packet: &[u8]) -> Option<Vec<Vec<u8>>> {
    if let Some(segment) = jpeg_app1_xmp(packet) {
        return Some(vec![segment]);
    }
    // The padding Adobe leaves before the trailer for in-place edits is
    // whitespace by definition, and often all the packet is over by.
    let trimmed = padding_trimmed(packet);
    let packet: &[u8] = trimmed.as_deref().unwrap_or(packet);
    if trimmed.is_some() {
        if let Some(segment) = jpeg_app1_xmp(packet) {
            return Some(vec![segment]);
        }
    }
    split(packet)
}

/// `packet` with the whitespace immediately before a trailing
/// `<?xpacket end=` removed; `None` when there is none.
fn padding_trimmed(packet: &[u8]) -> Option<Vec<u8>> {
    const TRAILER: &[u8] = b"<?xpacket end=";
    let at = packet.windows(TRAILER.len()).rposition(|w| w == TRAILER)?;
    let start = packet[..at]
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);
    (start < at).then(|| [&packet[..start], &packet[at..]].concat())
}

/// One property that can move.
struct Unit {
    description: usize,
    span: Range<usize>,
    /// An attribute (goes in the start tag) rather than a child element.
    attribute: bool,
    history: bool,
}

fn split(packet: &[u8]) -> Option<Vec<Vec<u8>>> {
    let rdf = xmp_rdf::scan(packet)?;
    let first = rdf.descriptions.first()?;
    let mut notes = note_spans(packet, &rdf);
    notes.sort_by_key(|r| r.start);
    let without_notes = packet.len() - notes.iter().map(|r| r.len()).sum::<usize>();
    if without_notes <= STANDARD_LIMIT {
        // Only a stale note was over the line; nothing needs to move.
        return Some(vec![jpeg_app1_xmp(&xmp_rdf::without(packet, &notes))?]);
    }
    let (prefix, declare) = note_prefix(packet, &rdf, first)?;
    // The note's exact bytes but for the GUID, which has a fixed length.
    let note = |guid: &[u8]| {
        let mut text = Vec::new();
        if declare {
            text.extend_from_slice(b" xmlns:xmpNote=\"");
            text.extend_from_slice(NOTE_NS);
            text.push(b'"');
        }
        text.push(b' ');
        text.extend_from_slice(prefix);
        text.extend_from_slice(b":HasExtendedXMP=\"");
        text.extend_from_slice(guid);
        text.push(b'"');
        text
    };
    let note_len = note(&[b'0'; 32]).len();

    let units = units(packet, &rdf, &notes);
    let mut order: Vec<usize> = (0..units.len()).collect();
    order.sort_by_key(|&i| {
        (
            !units[i].history,
            std::cmp::Reverse(units[i].span.len()),
            units[i].span.start,
        )
    });
    // A Description other than the first whose every property moves is
    // removed whole: it then states nothing, and leaving its wrapper would
    // add one per export→reopen cycle (the merge appends the extension's
    // copy). Its bytes beyond its units and notes leave `length` when its
    // last unit moves, so the count stays exact.
    let mut left = vec![0usize; rdf.descriptions.len()];
    let mut inner = vec![0usize; rdf.descriptions.len()];
    for u in &units {
        left[u.description] += 1;
        inner[u.description] += u.span.len();
    }
    let notes_in = |r: &Range<usize>| -> usize {
        let from = notes.partition_point(|n| n.start < r.start);
        notes[from..]
            .iter()
            .take_while(|n| n.end <= r.end)
            .map(|n| n.len())
            .sum()
    };
    let mut moved = vec![false; units.len()];
    let mut length = without_notes + note_len;
    for i in order {
        if length <= STANDARD_LIMIT {
            break;
        }
        moved[i] = true;
        length -= units[i].span.len();
        let d = units[i].description;
        left[d] -= 1;
        if d > 0 && left[d] == 0 {
            let span = rdf.descriptions[d].span();
            length -= span.len() - inner[d] - notes_in(&span);
        }
    }
    if length > STANDARD_LIMIT {
        return None;
    }

    let extension = extension(packet, &rdf, &units, &moved)?;
    let full = u32::try_from(extension.len()).ok()?;
    let guid: Vec<u8> = md5(&extension)
        .iter()
        .flat_map(|b| format!("{b:02X}").into_bytes())
        .collect();
    let emptied: Vec<Range<usize>> = (1..rdf.descriptions.len())
        .filter(|&d| left[d] == 0 && inner[d] > 0)
        .map(|d| rdf.descriptions[d].span())
        .collect();
    let inside = |r: &Range<usize>| {
        let k = emptied.partition_point(|e| e.start <= r.start);
        k > 0 && r.end <= emptied[k - 1].end
    };
    let mut removals: Vec<Range<usize>> = notes;
    removals.extend(
        units
            .iter()
            .zip(&moved)
            .filter(|(_, &m)| m)
            .map(|(u, _)| u.span.clone()),
    );
    removals.retain(|r| !inside(r));
    removals.extend(emptied.iter().cloned());
    removals.sort_by_key(|r| r.start);
    let standard = rebuilt(packet, &removals, first.insert_at, &note(&guid))?;
    if standard.len() > STANDARD_LIMIT
        || standard.len() as u64 + u64::from(full) > MAX_DRDC_BLOB_LEN as u64
    {
        return None;
    }
    let mut segments = vec![jpeg_app1_xmp(&standard)?];
    for (i, data) in extension.chunks(CHUNK_DATA).enumerate() {
        let offset = u32::try_from(i * CHUNK_DATA).ok()?;
        let mut payload = Vec::with_capacity(40 + data.len());
        payload.extend_from_slice(&guid);
        payload.extend_from_slice(&full.to_be_bytes());
        payload.extend_from_slice(&offset.to_be_bytes());
        payload.extend_from_slice(data);
        segments.push(jpeg_segment(0xE1, XMP_EXT_ID, &payload)?);
    }
    Some(segments)
}

/// The prefix the note is written under on `d`, and whether
/// `xmlns:xmpNote` has to be declared for it. `None` when `xmpNote` is
/// bound in `d`'s scope to another namespace.
fn note_prefix<'p>(packet: &'p [u8], rdf: &Rdf, d: &Description) -> Option<(&'p [u8], bool)> {
    let scope = Scope::new(packet, rdf, d);
    for a in d.attrs.iter().chain(&rdf.inherited) {
        let prefix = &packet[a.local.clone()];
        if &packet[a.prefix.clone()] == b"xmlns"
            && &packet[a.value.clone()] == NOTE_NS
            && scope.resolve(packet, &[], prefix) == Some(NOTE_NS)
        {
            return Some((prefix, false));
        }
    }
    // `xmpNote` bound in scope (on `d`, `rdf:RDF` or `x:xmpmeta`) to another
    // namespace: declaring it on `d` would rebind that one's properties.
    let rebinds = scope
        .resolve(packet, &[], b"xmpNote")
        .is_some_and(|ns| ns != NOTE_NS);
    (!rebinds).then_some((b"xmpNote".as_slice(), true))
}

/// Every property that may move, in document order.
fn units(packet: &[u8], rdf: &Rdf, notes: &[Range<usize>]) -> Vec<Unit> {
    let note_starts: HashSet<usize> = notes.iter().map(|r| r.start).collect();
    let history = |ns: Option<&[u8]>, local: &Range<usize>| {
        ns == Some(PHOTOSHOP_NS) && &packet[local.clone()] == b"History"
    };
    let mut out = Vec::new();
    for (index, d) in rdf.descriptions.iter().enumerate() {
        let scope = Scope::new(packet, rdf, d);
        for a in &d.attrs {
            let prefix = &packet[a.prefix.clone()];
            if a.is_declaration(packet)
                || a.is_lang(packet)
                || prefix == b"rdf"
                || note_starts.contains(&a.span.start)
            {
                continue;
            }
            let ns = scope.resolve(packet, &[], prefix);
            out.push(Unit {
                description: index,
                span: a.span.clone(),
                attribute: true,
                history: history(ns, &a.local),
            });
        }
        for c in &d.children {
            if note_starts.contains(&c.span.start) {
                continue;
            }
            let ns = scope.resolve(packet, &c.declarations, &packet[c.prefix.clone()]);
            out.push(Unit {
                description: index,
                span: c.span.clone(),
                attribute: false,
                history: history(ns, &c.local),
            });
        }
    }
    out
}

/// The extended packet holding every moved unit (see the module doc);
/// `None` once it passes the blob cap, which the caller would refuse
/// anyway — checked as it grows, because every Description repeats the
/// declarations in scope.
fn extension(packet: &[u8], rdf: &Rdf, units: &[Unit], moved: &[bool]) -> Option<Vec<u8>> {
    let cap = MAX_DRDC_BLOB_LEN as usize;
    let mut out = EXTENSION_OPEN.to_vec();
    // `units` is in document order, so each Description's units are one run.
    let pairs: Vec<(&Unit, bool)> = units.iter().zip(moved.iter().copied()).collect();
    for group in pairs.chunk_by(|a, b| a.0.description == b.0.description) {
        let here: Vec<&Unit> = group.iter().filter(|(_, m)| *m).map(|(u, _)| *u).collect();
        let Some(first) = here.first() else {
            continue;
        };
        let d = rdf.descriptions.get(first.description)?;
        out.extend_from_slice(b"<rdf:Description");
        match d
            .attrs
            .iter()
            .find(|a| &packet[a.prefix.clone()] == b"rdf" && &packet[a.local.clone()] == b"about")
        {
            Some(about) => out.extend_from_slice(&packet[about.span.clone()]),
            None => out.extend_from_slice(b" rdf:about=\"\""),
        }
        // In scope: inherited, then the Description's own, each name once
        // and the later (inner) declaration winning, in first-seen order.
        let mut names: Vec<&[u8]> = Vec::new();
        let mut spans: HashMap<&[u8], Range<usize>> = HashMap::new();
        for a in rdf.inherited.iter().chain(
            d.attrs
                .iter()
                .filter(|a| a.is_declaration(packet) || a.is_lang(packet)),
        ) {
            let name = a.name(packet);
            if spans.insert(name, a.span.clone()).is_none() {
                names.push(name);
            }
        }
        for name in &names {
            if let Some(span) = spans.get(name) {
                out.extend_from_slice(&packet[span.clone()]);
            }
        }
        for unit in here.iter().filter(|u| u.attribute) {
            out.extend_from_slice(&packet[unit.span.clone()]);
        }
        out.push(b'>');
        for unit in here.iter().filter(|u| !u.attribute) {
            out.extend_from_slice(&packet[unit.span.clone()]);
        }
        out.extend_from_slice(b"</rdf:Description>");
        if out.len() > cap {
            return None;
        }
    }
    out.extend_from_slice(EXTENSION_CLOSE);
    Some(out)
}

/// `packet` with `removals` (sorted, disjoint) cut out and `text` inserted
/// at `insert_at`, which no removal covers.
fn rebuilt(
    packet: &[u8],
    removals: &[Range<usize>],
    insert_at: usize,
    text: &[u8],
) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(packet.len() + text.len());
    let mut cursor = 0usize;
    let mut inserted = false;
    for r in removals {
        if !inserted && insert_at <= r.start {
            out.extend_from_slice(packet.get(cursor..insert_at)?);
            out.extend_from_slice(text);
            cursor = insert_at;
            inserted = true;
        }
        out.extend_from_slice(packet.get(cursor..r.start)?);
        cursor = r.end;
    }
    if !inserted {
        out.extend_from_slice(packet.get(cursor..insert_at)?);
        out.extend_from_slice(text);
        cursor = insert_at;
    }
    out.extend_from_slice(packet.get(cursor..)?);
    Some(out)
}
