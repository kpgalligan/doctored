//! The READ half of JPEG ExtendedXMP: the `http://ns.adobe.com/xmp/extension/`
//! APP1 chunks a JPEG carries when its XMP is larger than one segment, put
//! back together and merged into the document's ONE logical XMP packet.
//! The write half is `metadata_xmp_split`.
//!
//! # The format (XMP Specification Part 3, §1.1.3.1)
//!
//! A JPEG segment holds at most 65 533 payload bytes, so a writer with more
//! XMP than that keeps a STANDARD packet in the ordinary
//! `http://ns.adobe.com/xap/1.0/` APP1 and serializes the rest as a second,
//! EXTENDED packet. That one is cut into chunks, each its own APP1:
//! the identifier `http://ns.adobe.com/xmp/extension/\0`, then 32 ASCII
//! hex digits naming the extended packet (the MD5 of its bytes, which only
//! identifies it — see `md5`), then the extended packet's FULL length and
//! THIS chunk's offset into it, both big-endian u32s, then the data. The
//! standard packet points at the extension it belongs to with the property
//! `xmpNote:HasExtendedXMP`, whose value is that GUID. Chunks may arrive in
//! any order and before the standard packet.
//!
//! # What a merge does
//!
//! The extension's `rdf:Description`s are copied verbatim into the standard
//! packet just before its `</rdf:RDF>` — RDF allows any number of
//! Descriptions, and that is exactly the form the split took the properties
//! out of — and the `HasExtendedXMP` note is removed, because the merged
//! packet HAS no extension any more. Each copied Description first gets the
//! namespace declarations and `xml:lang` it inherited from the extension's
//! own wrapper and does not declare itself, so its prefixes still resolve
//! where it now sits.
//!
//! # When it does not
//!
//! Every failure keeps the standard packet exactly as read and says why in
//! one of three sentences: chunks that do not tile their declared length
//! exactly, a GUID that is not 32 hex digits, an extension that does not
//! scan, or more than [`MAX_EXTENSION_CHUNKS`] chunks are
//! [`W_INCOMPLETE`]; a merge that would pass the DRDC blob cap — checked
//! BEFORE anything is allocated, and again on the merged bytes — is
//! [`W_TOO_LARGE`]; chunks that name no extension the standard packet points
//! at are [`W_ORPHAN`], and are reported beside a merge that does succeed.
//! The MD5 is never verified: a tool that edited the extension without
//! recomputing it leaves a stale digest, and the GUID's job is to match
//! chunks to a note, which it still does.
//!
//! `metadata_xmp_extended` merges a JPEG's `http://ns.adobe.com/xmp/extension/` chunks into the
//! standard packet on open (the XMP slot holds the LOGICAL packet; a failed merge keeps the
//! standard packet and adds one of three import-report warnings).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::xmp_rdf::{self, Rdf};

/// The namespace `xmpNote:HasExtendedXMP` lives in.
pub(crate) const NOTE_NS: &[u8] = b"http://ns.adobe.com/xmp/note/";

/// The note's local name.
const NOTE_LOCAL: &[u8] = b"HasExtendedXMP";

/// The most extension chunks one open collects. 4096 full chunks is
/// 256 MiB, sixteen times the blob cap, so no packet this crate could keep
/// needs more; the cap only bounds the list a crafted file can grow.
pub(crate) const MAX_EXTENSION_CHUNKS: usize = 4096;

pub(crate) const W_INCOMPLETE: &str =
    "Extended XMP is incomplete or inconsistent; only the standard XMP packet was read";
pub(crate) const W_TOO_LARGE: &str =
    "Extended XMP exceeds 16 MiB; only the standard XMP packet was read";
pub(crate) const W_ORPHAN: &str =
    "Extended XMP segments did not match the XMP packet and were not read";

/// One extension chunk, borrowed from the file's bytes.
pub(crate) struct ExtensionChunk<'a> {
    /// The 32 ASCII hex digits naming the extended packet.
    pub guid: &'a [u8],
    /// The extended packet's whole length.
    pub full_len: u32,
    /// Where `data` sits in it.
    pub offset: u32,
    pub data: &'a [u8],
}

/// The chunk in an APP1 payload, given the bytes after its identifier;
/// `None` when it is shorter than its 40-byte header.
pub(crate) fn extension_chunk(after_id: &[u8]) -> Option<ExtensionChunk<'_>> {
    let header = after_id.get(..40)?;
    Some(ExtensionChunk {
        guid: &header[..32],
        full_len: u32::from_be_bytes([header[32], header[33], header[34], header[35]]),
        offset: u32::from_be_bytes([header[36], header[37], header[38], header[39]]),
        data: &after_id[40..],
    })
}

/// What [`merged`] concluded.
pub(crate) struct ExtendedRead {
    /// `Some` = the merged logical packet, to replace the standard one.
    pub packet: Option<Vec<u8>>,
    pub warnings: Vec<&'static str>,
}

/// The logical packet a JPEG's standard XMP and its extension chunks make
/// (see the module doc). `overflow` says the walk saw more than
/// [`MAX_EXTENSION_CHUNKS`] chunks.
pub(crate) fn merged(
    standard: Option<&[u8]>,
    chunks: &[ExtensionChunk<'_>],
    overflow: bool,
) -> ExtendedRead {
    let mut warnings = Vec::new();
    let packet = merge(standard, chunks, overflow, &mut warnings);
    ExtendedRead { packet, warnings }
}

/// Spans ([`xmp_rdf::Attr::span`] / [`xmp_rdf::Child::span`]) of every
/// property resolving to [`NOTE_NS`] + `HasExtendedXMP`, in every
/// Description, in document order.
pub(crate) fn note_spans(packet: &[u8], rdf: &Rdf) -> Vec<Range<usize>> {
    notes(packet, rdf).into_iter().map(|n| n.span).collect()
}

/// One `HasExtendedXMP` property: where it is and where its value is.
struct Note {
    span: Range<usize>,
    value: Option<Range<usize>>,
}

fn notes(packet: &[u8], rdf: &Rdf) -> Vec<Note> {
    let mut out = Vec::new();
    for d in &rdf.descriptions {
        let scope = xmp_rdf::Scope::new(packet, rdf, d);
        for a in &d.attrs {
            // An unprefixed attribute is in NO namespace — the default
            // namespace does not apply to attributes.
            if a.prefix.is_empty() || a.is_declaration(packet) {
                continue;
            }
            if &packet[a.local.clone()] == NOTE_LOCAL
                && scope.resolve(packet, &[], &packet[a.prefix.clone()]) == Some(NOTE_NS)
            {
                out.push(Note {
                    span: a.span.clone(),
                    value: Some(a.value.clone()),
                });
            }
        }
        for c in &d.children {
            if &packet[c.local.clone()] == NOTE_LOCAL
                && scope.resolve(packet, &c.declarations, &packet[c.prefix.clone()])
                    == Some(NOTE_NS)
            {
                out.push(Note {
                    span: c.span.clone(),
                    value: xmp_rdf::element_text(packet, c),
                });
            }
        }
    }
    out
}

/// The note's GUID: its value with surrounding whitespace trimmed, when
/// that is exactly 32 ASCII hex digits.
fn guid<'p>(packet: &'p [u8], note: &Note) -> Option<&'p [u8]> {
    let value = packet.get(note.value.clone()?)?.trim_ascii();
    (value.len() == 32 && value.iter().all(u8::is_ascii_hexdigit)).then_some(value)
}

fn merge(
    standard: Option<&[u8]>,
    chunks: &[ExtensionChunk<'_>],
    overflow: bool,
    warnings: &mut Vec<&'static str>,
) -> Option<Vec<u8>> {
    let has_chunks = !chunks.is_empty() || overflow;
    // The common case — a JPEG with no ExtendedXMP at all — costs a byte
    // search, never a scan: a note has to SPELL its local name.
    let mentions_note =
        standard.is_some_and(|s| crate::metadata_xmp::find(s, NOTE_LOCAL, 0).is_some());
    if !has_chunks && !mentions_note {
        return None;
    }
    let found = standard.and_then(|s| Some((s, xmp_rdf::scan(s)?)));
    let found = found.and_then(|(s, rdf)| {
        let note = notes(s, &rdf).into_iter().next()?;
        Some((s, rdf, note))
    });
    let Some((standard, rdf, note)) = found else {
        // Nothing points at an extension, so whatever chunks there are
        // belong to no packet this file states.
        if has_chunks {
            warnings.push(W_ORPHAN);
        }
        return None;
    };
    let Some(guid) = guid(standard, &note) else {
        warnings.push(W_INCOMPLETE);
        return None;
    };
    let mut ours: Vec<&ExtensionChunk<'_>> = chunks
        .iter()
        .filter(|c| c.guid.eq_ignore_ascii_case(guid))
        .collect();
    if ours.len() < chunks.len() {
        warnings.push(W_ORPHAN);
    }
    if ours.is_empty() || overflow {
        warnings.push(W_INCOMPLETE);
        return None;
    }
    let full_len = ours[0].full_len;
    if full_len == 0 || ours.iter().any(|c| c.full_len != full_len) {
        warnings.push(W_INCOMPLETE);
        return None;
    }
    // Checked before a byte is allocated: the declared length is the
    // file's claim, and the reassembly buffer is sized from it.
    let cap = MAX_DRDC_BLOB_LEN as u64;
    if standard.len() as u64 + u64::from(full_len) > cap {
        warnings.push(W_TOO_LARGE);
        return None;
    }
    // The chunks must tile [0, full_len) exactly: no gap, no overlap, none
    // empty, none past the end.
    ours.sort_by_key(|c| c.offset);
    let mut expected = 0u64;
    for c in &ours {
        let end = u64::from(c.offset) + c.data.len() as u64;
        if u64::from(c.offset) != expected || c.data.is_empty() || end > u64::from(full_len) {
            warnings.push(W_INCOMPLETE);
            return None;
        }
        expected = end;
    }
    if expected != u64::from(full_len) {
        warnings.push(W_INCOMPLETE);
        return None;
    }
    let mut extension = Vec::with_capacity(full_len as usize);
    for c in &ours {
        extension.extend_from_slice(c.data);
    }
    let Some(ext_rdf) = xmp_rdf::scan(&extension) else {
        warnings.push(W_INCOMPLETE);
        return None;
    };
    match spliced(standard, &rdf, &extension, &ext_rdf) {
        Ok(out) => Some(out),
        Err(warning) => {
            warnings.push(warning);
            None
        }
    }
}

/// The standard packet with its notes removed and the extension's
/// Descriptions inserted before its `</rdf:RDF>`. [`W_TOO_LARGE`] when the
/// result would pass the blob cap.
fn spliced(
    standard: &[u8],
    rdf: &Rdf,
    extension: &[u8],
    ext_rdf: &Rdf,
) -> Result<Vec<u8>, &'static str> {
    let cap = MAX_DRDC_BLOB_LEN as usize;
    let mut removals = note_spans(standard, rdf);
    removals.sort_by_key(|r| r.start);
    let head = standard.get(..rdf.rdf_end).ok_or(W_INCOMPLETE)?;
    let mut out = xmp_rdf::without(head, &removals);
    // The extension wrapper's declarations, the innermost (last) one of
    // each name winning, in the order the names first appear.
    let mut names: Vec<&[u8]> = Vec::new();
    let mut spans: HashMap<&[u8], Range<usize>> = HashMap::new();
    for a in &ext_rdf.inherited {
        let name = a.name(extension);
        if spans.insert(name, a.span.clone()).is_none() {
            names.push(name);
        }
    }
    for d in &ext_rdf.descriptions {
        let own: HashSet<&[u8]> = d.attrs.iter().map(|a| a.name(extension)).collect();
        out.extend_from_slice(slice(extension, d.start_tag.start..d.insert_at)?);
        for name in &names {
            if let (false, Some(span)) = (own.contains(name), spans.get(name)) {
                out.extend_from_slice(slice(extension, span.clone())?);
            }
        }
        out.extend_from_slice(slice(extension, d.insert_at..d.span().end)?);
        if out.len() > cap {
            return Err(W_TOO_LARGE);
        }
    }
    out.extend_from_slice(slice(standard, rdf.rdf_end..standard.len())?);
    if out.len() > cap {
        return Err(W_TOO_LARGE);
    }
    Ok(out)
}

/// `bytes[range]`, or [`W_INCOMPLETE`] for a range a scan could not have
/// produced.
fn slice(bytes: &[u8], range: Range<usize>) -> Result<&[u8], &'static str> {
    bytes.get(range).ok_or(W_INCOMPLETE)
}
