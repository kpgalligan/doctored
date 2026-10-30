//! The one-way normalization the XMP packet gets on the way out: the
//! `tiff:Orientation` and `tiff:XResolution` / `tiff:YResolution` /
//! `tiff:ResolutionUnit` properties rewritten to what THIS document says,
//! exactly as `metadata_write::exif_normalized` rewrites the EXIF copies of
//! the same four numbers. Capture stays byte-exact; only the export is
//! rewritten, so the stored packet and the DRDC round trip are untouched.
//!
//! # Why the second copy has to be reset too
//!
//! `dr_image::open_bytes` bakes the camera's rotation into the pixels, so a
//! preserved `Orientation: 6` double-rotates the picture in every viewer.
//! That is why the EXIF copy is reset — and a file written by Lightroom or
//! Photoshop carries the SAME property a second time, in its XMP packet, as
//! `tiff:Orientation`. The XMP Specification Part 3 ("Reconciliation") makes
//! the XMP value authoritative for a property present in both places when
//! the file was last written by an XMP-aware application, and Bridge and
//! Adobe Camera Raw follow it, so resetting only the EXIF copy left the picture
//! rotated 90 degrees and at the source file's ppi in exactly the toolchain
//! Doctored's files have to interoperate with.
//!
//! # Why a text edit is safe here and an IFD edit is not
//!
//! An XMP packet is UTF-8 XML with **no internal offsets**: nothing in it
//! points at a byte position, so a value may be replaced by a longer or
//! shorter one and everything around it still means what it meant. An EXIF
//! IFD is the opposite — every out-of-line datum is addressed from the
//! block's own header — which is why `metadata_write` never moves a byte and
//! this module may. The packet is edited as BYTES rather than as a `String`:
//! every pattern matched here is ASCII, which cannot occur inside a UTF-8
//! multi-byte sequence, so a packet with invalid UTF-8 in a description
//! field is still edited correctly instead of being dropped.
//!
//! # What it does not do
//!
//! * A property that is ABSENT is never inserted. The packet is a
//!   description of the file, not a place to publish new claims, and the
//!   EXIF block and the container density already state the same numbers.
//! * Only the conventional `tiff:` prefix is recognized. Every writer that
//!   emits these properties (Adobe's included) binds the TIFF namespace to
//!   it; a packet that binds it to some other prefix keeps its stale values,
//!   which is the situation the whole file was in before this module
//!   existed.
//! * `tiff:ImageWidth` / `tiff:ImageLength` and `exif:PixelXDimension` /
//!   `exif:PixelYDimension` are left alone, like their EXIF counterparts:
//!   they are advisory, and no reader rotates or rescales a picture because
//!   of them.
//! * It does not carry Google's pixel-bound properties into a flat export.
//!   A Pixel camera's `GImage:` (the uncropped original, base64) and
//!   `GDepth:` (the depth map) properties describe the picture the CAMERA
//!   took, not the edited one, and run to megabytes; every property
//!   resolving to `http://ns.google.com/photos/1.0/image/` or
//!   `…/1.0/depthmap/` is removed, as is a stale `xmpNote:HasExtendedXMP`
//!   (the only note an export carries is the one `metadata_xmp_split`
//!   writes). This happens on the way OUT only: the stored packet, and so
//!   the `.rz` master, keeps them. A packet holding none of them, or one
//!   `xmp_rdf` cannot scan, is normalized exactly as before.
//!
//! the XMP packet's own copies of the orientation and resolution, rewritten as text (an XMP packet
//! has no internal offsets, which is why it may be resized and an IFD may not). `xmp_normalized`
//! drops Google's `GImage:`/`GDepth:` properties and any stale note from every flat export while
//! the stored packet keeps them.

use std::ops::Range;

use crate::export_metadata::Stamp;
use crate::metadata::Resolution;
use crate::metadata_xmp_extended::note_spans;
use crate::xmp_rdf;

/// The namespaces of Google's pixel-bound camera properties (`GImage:`,
/// `GDepth:`), which a flat export drops.
const GOOGLE_PIXEL_NAMESPACES: [&[u8]; 2] = [
    b"http://ns.google.com/photos/1.0/image/",
    b"http://ns.google.com/photos/1.0/depthmap/",
];

/// The Dublin Core namespace `dc:creator` and `dc:rights` live in — the two
/// properties a metadata policy's stamp writes, and the only two this module
/// ever inserts. File Info (`file_info`) writes its four `dc:` properties
/// under the same declaration.
pub(crate) const DC_NAMESPACE: &[u8] = b"xmlns:dc=\"http://purl.org/dc/elements/1.1/\"";

/// The XMP packet as it should be written: the document's orientation,
/// resolution and resolution unit, in both spellings XMP uses for a simple
/// property — an `rdf:Description` attribute and a child element.
///
/// Always returns a packet: a property this module does not recognize is
/// left exactly as it was, so the worst case is the packet the source file
/// carried.
pub(crate) fn xmp_normalized(blob: &[u8], dpi: Resolution) -> Vec<u8> {
    let mut out = without_pixel_bound(blob).unwrap_or_else(|| blob.to_vec());
    // The rationals first is not significant — the four properties have
    // distinct names and no edit can create or destroy another's spelling.
    for (name, value) in [
        // The rotation is already in the pixels.
        ("tiff:Orientation", "1".to_string()),
        ("tiff:XResolution", rational(dpi.x)),
        ("tiff:YResolution", rational(dpi.y)),
        // 2 = inches, which is what the two rationals above are counted in.
        ("tiff:ResolutionUnit", "2".to_string()),
    ] {
        out = set_property(&out, name.as_bytes(), value.as_bytes());
    }
    out
}

/// `blob` without its Google pixel-bound properties and its
/// `HasExtendedXMP` notes (see the module doc); `None` when it carries none
/// of them or does not scan, so the bytes go on unchanged.
fn without_pixel_bound(blob: &[u8]) -> Option<Vec<u8>> {
    // A byte search first: both the namespace and the note's local name
    // must be spelled out, so a packet without either is never scanned.
    if find(blob, b"http://ns.google.com/photos/1.0/", 0).is_none()
        && find(blob, b"HasExtendedXMP", 0).is_none()
    {
        return None;
    }
    let rdf = xmp_rdf::scan(blob)?;
    let google = |ns: Option<&[u8]>| ns.is_some_and(|ns| GOOGLE_PIXEL_NAMESPACES.contains(&ns));
    let mut spans: Vec<Range<usize>> = note_spans(blob, &rdf);
    for d in &rdf.descriptions {
        let scope = xmp_rdf::Scope::new(blob, &rdf, d);
        for a in &d.attrs {
            if a.prefix.is_empty() || a.is_declaration(blob) {
                continue;
            }
            if google(scope.resolve(blob, &[], &blob[a.prefix.clone()])) {
                spans.push(a.span.clone());
            }
        }
        for c in &d.children {
            let ns = scope.resolve(blob, &c.declarations, &blob[c.prefix.clone()]);
            if google(ns) {
                spans.push(c.span.clone());
            }
        }
    }
    if spans.is_empty() {
        return None;
    }
    spans.sort_by_key(|r| r.start);
    Some(xmp_rdf::without(blob, &spans))
}

/// `blob` with the policy's credit inserted as `dc:creator` (an `rdf:Seq`)
/// and `dc:rights` (an `rdf:Alt` with the conventional `x-default` entry),
/// as the first children of the first `rdf:Description` element.
///
/// `None` when the packet has no `rdf:Description` OPENING tag — a
/// self-closing `<rdf:Description …/>` counts as none, because it has no
/// content model to put a property into and rewriting it into a container
/// would be authoring a packet rather than stamping one. The caller then
/// writes the normalized packet unchanged and reports `stamped: false`.
///
/// The namespace declaration is added to that same element unless the packet
/// already binds the `dc:` prefix anywhere, which is the case for every
/// packet Adobe's tools write.
///
/// A packet that ALREADY states a credit the stamp sets — a `dc:creator` or
/// `dc:rights` authored through File Info, or a camera's own — has it
/// replaced rather than joined by a second element of the same name, which
/// no reader is obliged to resolve in the stamp's favour: every stamped
/// property is removed and written once through File Info's writer
/// (`file_info::xmp_replaced`).
pub(crate) fn xmp_stamped(blob: &[u8], stamp: &Stamp) -> Option<Vec<u8>> {
    let stated = crate::file_info::xmp_fields(blob);
    if (stamp.author.is_some() && stated.creator.is_some())
        || (stamp.copyright.is_some() && stated.copyright.is_some())
    {
        return crate::file_info::xmp_replaced(blob, &stamp_edit(stamp)).ok();
    }
    const OPEN: &[u8] = b"<rdf:Description";
    let mut at = 0usize;
    let close = loop {
        let found = find(blob, OPEN, at)?;
        // The name must be whole: `<rdf:DescriptionX` is a different element.
        let after = found + OPEN.len();
        let whole = matches!(blob.get(after), Some(b) if
            b.is_ascii_whitespace() || *b == b'>' || *b == b'/');
        let end = after + blob.get(after..)?.iter().position(|&b| b == b'>')?;
        if whole && blob.get(end.checked_sub(1)?) != Some(&b'/') {
            break end;
        }
        at = after;
    };

    let mut out = Vec::with_capacity(blob.len() + 256);
    out.extend_from_slice(&blob[..close]);
    if find(blob, b"xmlns:dc=", 0).is_none() {
        out.push(b' ');
        out.extend_from_slice(DC_NAMESPACE);
    }
    out.push(b'>');
    properties(&mut out, stamp);
    out.extend_from_slice(&blob[close + 1..]);
    Some(out)
}

/// A complete `x:xmpmeta` packet holding nothing but the stamp — what an
/// export writes when the document carries no XMP of its own. It states
/// nothing about the picture, so it cannot contradict the EXIF block or the
/// container density the same save writes.
pub(crate) fn xmp_minimal(stamp: &Stamp) -> Vec<u8> {
    xmp_minimal_holding(|out| properties(out, stamp))
}

/// The minimal packet around whatever `properties` writes: the ONE packet
/// this crate authors from nothing, shared by the stamp ([`xmp_minimal`])
/// and by File Info (`file_info::write`).
pub(crate) fn xmp_minimal_holding(properties: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    out.extend_from_slice(b"<?xpacket begin=\"\xEF\xBB\xBF\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>");
    out.extend_from_slice(b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">");
    out.extend_from_slice(b"<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">");
    dc_description(&mut out, properties);
    out.extend_from_slice(b"</rdf:RDF></x:xmpmeta>");
    out.extend_from_slice(b"<?xpacket end=\"w\"?>");
    out
}

/// One `rdf:Description` that binds the `dc:` prefix and holds whatever
/// `properties` writes.
pub(crate) fn dc_description(out: &mut Vec<u8>, properties: impl FnOnce(&mut Vec<u8>)) {
    out.extend_from_slice(b"<rdf:Description rdf:about=\"\" ");
    out.extend_from_slice(DC_NAMESPACE);
    out.push(b'>');
    properties(out);
    out.extend_from_slice(b"</rdf:Description>");
}

/// The stamp as the Dublin Core edit it is: `dc:creator` with the one
/// author, `dc:rights` with the copyright, the other two properties left
/// alone.
fn stamp_edit(stamp: &Stamp) -> crate::file_info::DcEdit<'_> {
    crate::file_info::DcEdit {
        creator: stamp.author.as_ref().map(std::slice::from_ref),
        rights: stamp.copyright.as_deref(),
        description: None,
        subject: None,
    }
}

/// The two property elements, in the containers the XMP specification gives
/// them: `dc:creator` is an ordered list, `dc:rights` a language alternative.
/// Written by File Info's writer, the one home of those shapes.
fn properties(out: &mut Vec<u8>, stamp: &Stamp) {
    stamp_edit(stamp).properties(out);
}

/// The five XML character entities. The stamp is caller text, so it must not
/// be able to close an element or open an attribute.
pub(crate) fn escaped(out: &mut Vec<u8>, text: &[u8]) {
    for &byte in text {
        match byte {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' => out.extend_from_slice(b"&apos;"),
            other => out.push(other),
        }
    }
}

/// A ppi as the `n/d` XMP TIFF schema spells a Rational — the same choice
/// `metadata_write::rational_bytes` makes for the EXIF copy, so the two
/// statements in one file read identically: an integral ppi is `300/1`
/// rather than `3000000/10000`, and anything else keeps the four decimals
/// `Resolution::sane` stores.
fn rational(ppi: f32) -> String {
    let v = f64::from(ppi);
    if (v - v.round()).abs() < 1e-4 {
        format!("{}/1", v.round().max(1.0) as u32)
    } else {
        format!("{}/10000", (v * 10_000.0).round().max(1.0) as u32)
    }
}

/// Every occurrence of `name` rewritten to `value`, in both spellings:
/// `name="…"` (or `name='…'`) and `<name …>…</name>`.
fn set_property(blob: &[u8], name: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(blob.len());
    let mut i = 0usize;
    while i < blob.len() {
        let Some(found) = find(blob, name, i) else {
            break;
        };
        // The name must be whole: `tiff:Orientation` inside a longer name is
        // a different property, and rewriting part of one would corrupt it.
        // What comes BEFORE it decides which spelling this is — `<` for an
        // element, anything else for an attribute — and a name that is a
        // suffix of a longer one (`xmp:tiff:Orientation` is not a thing, but
        // costs nothing to exclude) is skipped.
        let after = found + name.len();
        let before = blob[..found].last().copied();
        let opens = before == Some(b'<');
        let attribute = matches!(before, Some(b) if b.is_ascii_whitespace());
        let boundary = matches!(blob.get(after), Some(b) if
            b.is_ascii_whitespace() || *b == b'=' || *b == b'>' || *b == b'/');
        let end = if boundary && opens {
            element_value(blob, after, name).map(|span| replace(&mut out, blob, i, span, value))
        } else if boundary && attribute {
            attribute_value(blob, after).map(|span| replace(&mut out, blob, i, span, value))
        } else {
            None
        };
        match end {
            Some(end) => i = end,
            None => {
                // Not a spelling we rewrite: copy the name through and carry
                // on past it, so a second occurrence still gets its chance.
                out.extend_from_slice(&blob[i..after]);
                i = after;
            }
        }
    }
    out.extend_from_slice(&blob[i.min(blob.len())..]);
    out
}

/// Copies `blob[at..span.start]`, then `value`, and answers where to carry
/// on from.
fn replace(
    out: &mut Vec<u8>,
    blob: &[u8],
    at: usize,
    span: std::ops::Range<usize>,
    value: &[u8],
) -> usize {
    out.extend_from_slice(&blob[at..span.start]);
    out.extend_from_slice(value);
    span.end
}

/// The value span of `name="…"`, given the offset just past the name.
/// `None` unless what follows is optional whitespace, `=`, optional
/// whitespace and a quote with a closing partner.
pub(crate) fn attribute_value(blob: &[u8], after_name: usize) -> Option<std::ops::Range<usize>> {
    let mut i = skip_space(blob, after_name);
    if *blob.get(i)? != b'=' {
        return None;
    }
    i = skip_space(blob, i + 1);
    let quote = *blob.get(i)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let start = i + 1;
    let end = start + blob.get(start..)?.iter().position(|&b| b == quote)?;
    Some(start..end)
}

/// The text span of `<name …>text</name>`, given the offset just past the
/// name in the OPENING tag. `None` for a self-closing element (which has no
/// text to rewrite), for an unterminated tag, or when the matching close tag
/// is missing — every one of those is a packet we leave as we found it.
fn element_value(blob: &[u8], after_name: usize, name: &[u8]) -> Option<std::ops::Range<usize>> {
    let close = after_name + blob.get(after_name..)?.iter().position(|&b| b == b'>')?;
    if blob.get(close.checked_sub(1)?) == Some(&b'/') {
        return None;
    }
    let start = close + 1;
    let mut end_tag = Vec::with_capacity(name.len() + 3);
    end_tag.extend_from_slice(b"</");
    end_tag.extend_from_slice(name);
    end_tag.push(b'>');
    let end = find(blob, &end_tag, start)?;
    // An element holding markup rather than plain text is not a simple
    // property; replacing across it would produce nonsense.
    if blob[start..end].contains(&b'<') {
        return None;
    }
    Some(start..end)
}

pub(crate) fn skip_space(blob: &[u8], mut at: usize) -> usize {
    while matches!(blob.get(at), Some(b) if b.is_ascii_whitespace()) {
        at += 1;
    }
    at
}

/// The first offset at or after `from` where `needle` occurs.
pub(crate) fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() || needle.is_empty() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|at| at + from)
}
