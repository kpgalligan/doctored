//! JPEG ExtendedXMP fixtures written independently of the core: an APP1
//! segment builder, an extension-chunk splitter, a JPEG assembler, a walker
//! that finds a file's standard XMP packet and its extension chunks, a
//! realistic packet generator, and a start-tag lexer that lists every
//! attribute name, so a test can check that no tag the core wrote repeats
//! one. Shared by `metadata_extended_xmp_read.rs` and
//! `metadata_extended_xmp_write.rs`; the identifiers and limits below are
//! the XMP Specification Part 3's, restated here rather than read from the
//! core.
#![allow(dead_code)]

use image::{ExtendedColorType, ImageEncoder};

use super::metadata_fixtures::jpeg_segments;

/// The standard XMP packet's APP1 identifier.
pub const XMP_ID: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// One ExtendedXMP chunk's APP1 identifier.
pub const EXT_ID: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
/// 65 533 payload bytes per segment minus the 29-byte identifier.
pub const STANDARD_LIMIT: usize = 65504;
/// 65 533 minus the 35-byte identifier, the 32-digit GUID and two u32s.
pub const CHUNK_DATA: usize = 65458;
/// The largest JPEG segment payload.
pub const SEGMENT_PAYLOAD: usize = 65533;
pub const NOTE_NS: &str = "http://ns.adobe.com/xmp/note/";
pub const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// The namespace declarations most fixtures put on their Description.
pub const NAMESPACES: &str = concat!(
    " xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
    " xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\"",
    " xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"",
    " xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"",
    " xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\""
);

// ------------------------------------------------------------ segments --

/// One JPEG marker segment: `FF`, `marker`, the length, `payload`.
pub fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= SEGMENT_PAYLOAD, "a fixture segment fits");
    let mut out = vec![0xFF, marker];
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// The standard packet's APP1.
pub fn standard_segment(packet: &[u8]) -> Vec<u8> {
    segment(0xE1, &[XMP_ID, packet].concat())
}

/// One extension chunk's APP1: identifier, GUID, full length and offset
/// (big-endian), data.
pub fn extension_segment(guid: &[u8], full_len: u32, offset: u32, data: &[u8]) -> Vec<u8> {
    let mut payload = EXT_ID.to_vec();
    payload.extend_from_slice(guid);
    payload.extend_from_slice(&full_len.to_be_bytes());
    payload.extend_from_slice(&offset.to_be_bytes());
    payload.extend_from_slice(data);
    segment(0xE1, &payload)
}

/// `extension` cut into chunks of at most `size` bytes, in offset order.
pub fn extension_segments(extension: &[u8], guid: &[u8], size: usize) -> Vec<Vec<u8>> {
    extension
        .chunks(size)
        .enumerate()
        .map(|(i, data)| extension_segment(guid, extension.len() as u32, (i * size) as u32, data))
        .collect()
}

/// The uppercase-hex MD5 GUID of `extension`, through the core's seam
/// (which `metadata_extended_xmp_read.rs` checks against RFC 1321 and
/// against the system's own `md5`).
pub fn guid_of(extension: &[u8]) -> Vec<u8> {
    doctored_core::md5::md5(extension)
        .iter()
        .flat_map(|b| format!("{b:02X}").into_bytes())
        .collect()
}

/// A real 8x8 baseline JPEG with `segments` inserted straight after SOI.
pub fn jpeg_with(segments: &[Vec<u8>]) -> Vec<u8> {
    let pixels: Vec<u8> = (0..8 * 8 * 3).map(|i| (i * 7 % 256) as u8).collect();
    let mut base = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut base, 90)
        .write_image(&pixels, 8, 8, ExtendedColorType::Rgb8)
        .unwrap();
    let mut out = base[..2].to_vec();
    for s in segments {
        out.extend_from_slice(s);
    }
    out.extend_from_slice(&base[2..]);
    out
}

// -------------------------------------------------------------- walker --

/// One extension chunk as a file carries it.
pub struct Chunk {
    pub guid: Vec<u8>,
    pub full_len: u32,
    pub offset: u32,
    pub data: Vec<u8>,
    /// The whole segment payload's length, identifier included.
    pub payload_len: usize,
}

/// A JPEG's XMP as the file carries it.
pub struct Found {
    /// Every standard packet, in file order.
    pub standards: Vec<Vec<u8>>,
    /// Every extension chunk, in file order.
    pub chunks: Vec<Chunk>,
    /// The segment index of each XMP-related APP1, standard and chunks.
    pub indices: Vec<usize>,
}

impl Found {
    /// The one standard packet.
    pub fn standard(&self) -> &[u8] {
        assert_eq!(self.standards.len(), 1, "exactly one standard XMP APP1");
        &self.standards[0]
    }
}

/// Walks `jpeg` with the test-side segment walker.
pub fn xmp_in(jpeg: &[u8]) -> Found {
    let mut found = Found {
        standards: Vec::new(),
        chunks: Vec::new(),
        indices: Vec::new(),
    };
    for (i, (marker, payload)) in jpeg_segments(jpeg).into_iter().enumerate() {
        if marker != 0xE1 {
            continue;
        }
        if let Some(rest) = payload.strip_prefix(XMP_ID) {
            found.standards.push(rest.to_vec());
            found.indices.push(i);
        } else if let Some(rest) = payload.strip_prefix(EXT_ID) {
            assert!(rest.len() >= 40, "a chunk carries its 40-byte header");
            found.chunks.push(Chunk {
                guid: rest[..32].to_vec(),
                full_len: u32::from_be_bytes(rest[32..36].try_into().unwrap()),
                offset: u32::from_be_bytes(rest[36..40].try_into().unwrap()),
                data: rest[40..].to_vec(),
                payload_len: payload.len(),
            });
            found.indices.push(i);
        }
    }
    found
}

/// The extended packet the chunks make, asserting that they agree on one
/// GUID and one length, fit their segments and tile it exactly.
pub fn reassembled(chunks: &[Chunk]) -> Vec<u8> {
    assert!(!chunks.is_empty(), "there are extension chunks");
    let guid = &chunks[0].guid;
    let full = chunks[0].full_len;
    let mut sorted: Vec<&Chunk> = chunks.iter().collect();
    sorted.sort_by_key(|c| c.offset);
    let mut out = Vec::new();
    for c in sorted {
        assert_eq!(&c.guid, guid, "one GUID");
        assert_eq!(c.full_len, full, "one full length");
        assert_eq!(c.offset as usize, out.len(), "chunks tile the packet");
        assert!(c.data.len() <= CHUNK_DATA, "chunk data fits");
        assert!(c.payload_len <= SEGMENT_PAYLOAD, "chunk segment fits");
        out.extend_from_slice(&c.data);
    }
    assert_eq!(out.len(), full as usize, "the chunks cover the full length");
    out
}

// ------------------------------------------------------------- packets --

/// A complete packet with one Description carrying `namespaces` and `attrs`
/// in its start tag and `body` as its children.
pub fn xmp_packet(namespaces: &str, attrs: &str, body: &str) -> Vec<u8> {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"fixture\">\n \
         <rdf:RDF xmlns:rdf=\"{RDF_NS}\">\n  \
         <rdf:Description rdf:about=\"\"{namespaces}{attrs}>\n{body}\n  \
         </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>"
    )
    .into_bytes()
}

/// An extended packet in the form Adobe writes (no `<?xpacket?>`), holding
/// `descriptions` verbatim inside its `rdf:RDF`, which carries
/// `rdf_attrs`.
pub fn extension_packet(rdf_attrs: &str, descriptions: &str) -> Vec<u8> {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF_NS}\"{rdf_attrs}>\
         {descriptions}</rdf:RDF></x:xmpmeta>"
    )
    .into_bytes()
}

/// `n` bytes of printable, non-repeating-looking text with no markup.
pub fn filler(seed: u32, n: usize) -> String {
    let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            alphabet[(state % alphabet.len() as u32) as usize] as char
        })
        .collect()
}

/// The realistic oversize packet the write tests split: small `dc:`
/// properties, a large `photoshop:History`, a large `crs:` element and a
/// large attribute property — about `history + crs + attribute` bytes.
pub struct Realistic {
    pub bytes: Vec<u8>,
    /// Text that must survive somewhere: each property's distinctive value.
    pub markers: Vec<String>,
}

pub fn realistic(history: usize, crs: usize, attribute: usize) -> Realistic {
    let history_text = filler(1, history);
    let crs_text = filler(2, crs);
    let attribute_text = filler(3, attribute);
    let attrs =
        format!(" dc:format=\"image/jpeg\" xmp:Rating=\"4\" crs:Large=\"{attribute_text}\"");
    let body = format!(
        "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Small title</rdf:li></rdf:Alt></dc:title>\n\
         \x20  <photoshop:History>{history_text}</photoshop:History>\n\
         \x20  <crs:ToneCurvePV2012><rdf:Seq><rdf:li>{crs_text}</rdf:li></rdf:Seq></crs:ToneCurvePV2012>"
    );
    Realistic {
        bytes: xmp_packet(NAMESPACES, &attrs, &body),
        markers: vec![
            "image/jpeg".into(),
            "xmp:Rating=\"4\"".into(),
            "Small title".into(),
            history_text,
            crs_text,
            attribute_text,
        ],
    }
}

/// How many times `needle` occurs in `haystack`.
pub fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    count(haystack, needle) > 0
}

// --------------------------------------------------------- tag lexer --

/// Every start tag's attribute names, in document order — a lexer written
/// here, skipping comments, processing instructions, CDATA and end tags,
/// and honouring quotes.
pub fn start_tag_attributes(xml: &[u8]) -> Vec<Vec<Vec<u8>>> {
    let mut tags = Vec::new();
    let mut i = 0usize;
    let skip_to = |from: usize, needle: &[u8]| {
        xml[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|p| from + p + needle.len())
            .expect("a terminated construct")
    };
    while i < xml.len() {
        if xml[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &xml[i..];
        if rest.starts_with(b"<!--") {
            i = skip_to(i, b"-->");
        } else if rest.starts_with(b"<![CDATA[") {
            i = skip_to(i, b"]]>");
        } else if rest.starts_with(b"<?") {
            i = skip_to(i, b"?>");
        } else if rest.starts_with(b"</") {
            i = skip_to(i, b">");
        } else {
            let mut j = i + 1;
            while j < xml.len() && !xml[j].is_ascii_whitespace() && xml[j] != b'>' && xml[j] != b'/'
            {
                j += 1;
            }
            let mut names = Vec::new();
            loop {
                while xml[j].is_ascii_whitespace() {
                    j += 1;
                }
                if xml[j] == b'>' || xml[j] == b'/' {
                    break;
                }
                let start = j;
                while xml[j] != b'=' && !xml[j].is_ascii_whitespace() {
                    j += 1;
                }
                names.push(xml[start..j].to_vec());
                while xml[j] != b'"' && xml[j] != b'\'' {
                    j += 1;
                }
                let quote = xml[j];
                j += 1;
                while xml[j] != quote {
                    j += 1;
                }
                j += 1;
            }
            tags.push(names);
            i = j;
        }
    }
    tags
}

/// The first attribute name repeated within one start tag, if any.
pub fn repeated_attribute(xml: &[u8]) -> Option<String> {
    for names in start_tag_attributes(xml) {
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            if !seen.insert(name) {
                return Some(String::from_utf8_lossy(name).into_owned());
            }
        }
    }
    None
}
