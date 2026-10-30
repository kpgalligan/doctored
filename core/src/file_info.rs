//! File Info: the four fields a person authors about a picture —
//! creator, copyright notice, description and keywords — read out of, and
//! written into, the three metadata packets a document already carries.
//! Free functions over [`Metadata`]; the document adapters are
//! `doc_file_info`.
//!
//! # Where each field lives
//!
//! XMP is the authority: `dc:creator` (an `rdf:Seq`), `dc:rights` and
//! `dc:description` (an `rdf:Alt`, its `x-default` entry) and `dc:subject`
//! (an `rdf:Bag`). An XMP packet has no internal offsets, so it can hold any
//! text and is rewritten freely (`metadata_xmp`'s module doc). The same
//! fields exist twice more in older forms, which `file_info_legacy` handles:
//! IIM datasets inside the 8BIM run (read as a fallback, DROPPED on write
//! together with the digest that says they are current, so no reader prefers
//! a stale copy) and three IFD0 ASCII tags of the EXIF packet (kept in step
//! where the packet can hold them).
//!
//! # Reading
//!
//! Per field: the XMP element when the packet has one, else the IIM dataset,
//! else the EXIF tag. What comes back is tidied for display — control
//! characters removed, entries trimmed, blank entries dropped — because the
//! stored bytes are another program's.
//!
//! # Writing
//!
//! [`write`] removes every `dc:creator`, `dc:rights`, `dc:description` and
//! `dc:subject` element from the packet (their spans come from `xmp_rdf`,
//! the crate's one RDF scanner) and inserts ONE `rdf:Description` holding
//! the non-empty fields before `</rdf:RDF>`. A new Description rather than
//! an edit of the first one, because the first may be self-closing
//! (attributes only), which has no content to add to. A Description the
//! removal leaves with nothing but `rdf:about` and namespace declarations is
//! cut whole, so a repeat edit replaces its predecessor's element instead of
//! leaving an empty one behind each time. A document with no XMP packet gets
//! `metadata_xmp`'s minimal packet, and none at all when every field is
//! empty. A packet `xmp_rdf` cannot describe (no `</rdf:RDF>` it can find)
//! refuses the edit rather than guessing where to write.
//!
//! # Limits
//!
//! In characters: a list entry 256, 256 entries per list, the copyright
//! notice 2000, the description 8000. They bound what a downstream reader is
//! handed, as `export_metadata::MAX_STAMP_CHARS` does for a stamp. Control
//! characters refuse, except a line feed in the description. Every refusal
//! is an `Err` starting "file info".

use std::ops::Range;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::doc_color::check_metadata_packet;
use crate::file_info_legacy as legacy;
use crate::metadata::Metadata;
use crate::metadata_xmp::{
    attribute_value, dc_description, escaped, find, xmp_minimal_holding, DC_NAMESPACE,
};
use crate::xmp_rdf;

/// The longest creator or keyword entry, in characters.
pub const MAX_ENTRY_CHARS: usize = 256;
/// The most creators, and the most keywords.
pub const MAX_ENTRIES: usize = 256;
/// The longest copyright notice, in characters.
pub const MAX_COPYRIGHT_CHARS: usize = 2000;
/// The longest description, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 8000;

/// The four JSON keys, which are also the four fields.
const KEYS: [&str; 4] = ["creator", "copyright", "description", "keywords"];

/// The authored fields. An empty list or string means the field is absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileInfo {
    pub creator: Vec<String>,
    pub copyright: String,
    pub description: String,
    pub keywords: Vec<String>,
}

impl FileInfo {
    /// `{"creator":[string],"copyright":string,"description":string,
    /// "keywords":[string]}` with exactly those four keys, checked against
    /// the limits (see [`FileInfo::checked`]). `Err` names the first thing
    /// wrong.
    pub fn parse(json: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| format!("file info is not valid json: {e}"))?;
        let object = value.as_object().ok_or("file info must be a json object")?;
        for key in object.keys() {
            if !KEYS.contains(&key.as_str()) {
                return Err(format!("file info has an unknown key \"{key}\""));
            }
        }
        let list = |key: &str| -> Result<Vec<String>, String> {
            let wrong = || format!("file info {key} must be an array of strings");
            match object.get(key) {
                None => Err(format!("file info requires the key \"{key}\"")),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned).ok_or_else(wrong))
                    .collect(),
                Some(_) => Err(wrong()),
            }
        };
        let text = |key: &str| -> Result<String, String> {
            match object.get(key) {
                None => Err(format!("file info requires the key \"{key}\"")),
                Some(Value::String(text)) => Ok(text.clone()),
                Some(_) => Err(format!("file info {key} must be a string")),
            }
        };
        FileInfo {
            creator: list("creator")?,
            copyright: text("copyright")?,
            description: text("description")?,
            keywords: list("keywords")?,
        }
        .checked()
    }

    /// The canonical JSON: all four keys, always.
    pub fn to_json(&self) -> String {
        json!({
            "creator": self.creator,
            "copyright": self.copyright,
            "description": self.description,
            "keywords": self.keywords,
        })
        .to_string()
    }

    /// True when no field holds anything.
    pub fn is_empty(&self) -> bool {
        self.creator.is_empty()
            && self.copyright.is_empty()
            && self.description.is_empty()
            && self.keywords.is_empty()
    }

    /// The value as it is stored: every entry and both strings trimmed,
    /// blank entries dropped, then the limits. `Err` for a control character
    /// (a line feed is allowed in the description), an entry past
    /// [`MAX_ENTRY_CHARS`], a list past [`MAX_ENTRIES`], a copyright notice
    /// past [`MAX_COPYRIGHT_CHARS`] or a description past
    /// [`MAX_DESCRIPTION_CHARS`].
    pub fn checked(self) -> Result<Self, String> {
        let list = |name: &str, entries: Vec<String>| -> Result<Vec<String>, String> {
            let entries: Vec<String> = entries
                .iter()
                .map(|entry| entry.trim().to_string())
                .filter(|entry| !entry.is_empty())
                .collect();
            if entries.len() > MAX_ENTRIES {
                return Err(format!(
                    "file info {name} must hold at most {MAX_ENTRIES} entries"
                ));
            }
            for entry in &entries {
                if entry.chars().count() > MAX_ENTRY_CHARS {
                    return Err(format!(
                        "file info {name} entries must be at most {MAX_ENTRY_CHARS} characters"
                    ));
                }
                if entry.chars().any(char::is_control) {
                    return Err(format!(
                        "file info {name} must not contain control characters"
                    ));
                }
            }
            Ok(entries)
        };
        let text = |name: &str, value: &str, most: usize, line_feed: bool| {
            let value = value.trim().to_string();
            if value.chars().count() > most {
                return Err(format!(
                    "file info {name} must be at most {most} characters"
                ));
            }
            if value
                .chars()
                .any(|c| c.is_control() && !(line_feed && c == '\n'))
            {
                return Err(format!(
                    "file info {name} must not contain control characters{}",
                    if line_feed {
                        " other than a line feed"
                    } else {
                        ""
                    }
                ));
            }
            Ok(value)
        };
        Ok(FileInfo {
            creator: list("creator", self.creator)?,
            copyright: text("copyright", &self.copyright, MAX_COPYRIGHT_CHARS, false)?,
            description: text(
                "description",
                &self.description,
                MAX_DESCRIPTION_CHARS,
                true,
            )?,
            keywords: list("keywords", self.keywords)?,
        })
    }
}

/// What one packet states, field by field. `None` is "this packet has no
/// such element", which is what lets [`read`] fall through to the next
/// packet; `Some` of an empty value is an element that is present and empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Found {
    pub creator: Option<Vec<String>>,
    pub copyright: Option<String>,
    pub description: Option<String>,
    pub keywords: Option<Vec<String>>,
}

/// The document's file info: per field the XMP element when present, else
/// the IIM dataset, else the EXIF tag, tidied for display (see the module
/// doc). Never fails: a packet that cannot be read states nothing.
pub fn read(metadata: &Metadata) -> FileInfo {
    let xmp = metadata.xmp.as_deref().map(xmp_fields).unwrap_or_default();
    let iim = metadata
        .iptc
        .as_deref()
        .map(legacy::iim_fields)
        .unwrap_or_default();
    let exif = metadata
        .exif
        .as_deref()
        .map(legacy::exif_fields)
        .unwrap_or_default();
    let list = |entries: Option<Vec<String>>| -> Vec<String> {
        entries
            .unwrap_or_default()
            .iter()
            .map(|entry| tidy(entry, false))
            .filter(|entry| !entry.is_empty())
            .collect()
    };
    FileInfo {
        creator: list(xmp.creator.or(iim.creator).or(exif.creator)),
        copyright: tidy(
            &xmp.copyright
                .or(iim.copyright)
                .or(exif.copyright)
                .unwrap_or_default(),
            false,
        ),
        description: tidy(
            &xmp.description
                .or(iim.description)
                .or(exif.description)
                .unwrap_or_default(),
            true,
        ),
        keywords: list(xmp.keywords.or(iim.keywords)),
    }
}

/// Stored text as it is shown: a CR LF pair or a bare CR becomes a line feed
/// where line feeds are kept, every other control character becomes a space,
/// and the ends are trimmed.
fn tidy(text: &str, line_feeds: bool) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    unified
        .chars()
        .map(|c| {
            if c.is_control() && !(line_feeds && c == '\n') {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// `metadata` with `info` written into its packets (see the module doc and
/// `file_info_legacy`). `Err` for a value past the limits, an XMP packet
/// with no `</rdf:RDF>` this crate can find, or a result past the packet
/// cap. The EXIF and IPTC packets are only ever edited when present; an XMP
/// packet is created when the document has none and a field is not empty.
pub fn write(metadata: &Metadata, info: &FileInfo) -> Result<Metadata, String> {
    let info = info.clone().checked()?;
    let edit = DcEdit {
        creator: Some(&info.creator),
        rights: Some(&info.copyright),
        description: Some(&info.description),
        subject: Some(&info.keywords),
    };
    let mut out = metadata.clone();
    match metadata.xmp.as_deref() {
        Some(packet) => store(&mut out.xmp, xmp_replaced(packet, &edit)?)?,
        None if !info.is_empty() => store(
            &mut out.xmp,
            xmp_minimal_holding(|packet| edit.properties(packet)),
        )?,
        None => {}
    }
    if let Some(run) = metadata.iptc.as_deref() {
        match legacy::iim_dropped(run) {
            // Nothing is left of the run, so there is no packet to keep.
            Some(next) if next.is_empty() => out.iptc = None,
            Some(next) => store(&mut out.iptc, next)?,
            None => {}
        }
    }
    if let Some(blob) = metadata.exif.as_deref() {
        if let Some(next) = legacy::exif_written(blob, &info) {
            store(&mut out.exif, next)?;
        }
    }
    Ok(out)
}

/// Puts a rewritten packet in its slot, keeping the stored allocation when
/// the bytes did not change and refusing one past the packet cap.
fn store(slot: &mut Option<Arc<[u8]>>, bytes: Vec<u8>) -> Result<(), String> {
    if slot.as_deref() == Some(&bytes[..]) {
        return Ok(());
    }
    check_metadata_packet(Some(&bytes)).map_err(|e| format!("file info cannot be written: {e}"))?;
    *slot = Some(Arc::from(bytes));
    Ok(())
}

// ------------------------------------------------------------------ XMP --

/// The Dublin Core namespace URI: [`DC_NAMESPACE`] (its one home) between
/// the quotes of the declaration.
fn dc_uri() -> &'static [u8] {
    &DC_NAMESPACE[b"xmlns:dc=\"".len()..DC_NAMESPACE.len() - 1]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Creator,
    Rights,
    Description,
    Subject,
}

/// Which of the four properties a Description child is, when its prefix
/// resolves to the Dublin Core namespace where it sits.
fn dc_field(packet: &[u8], scope: &xmp_rdf::Scope<'_>, child: &xmp_rdf::Child) -> Option<Field> {
    let uri = scope.resolve(packet, &child.declarations, &packet[child.prefix.clone()])?;
    if uri != dc_uri() {
        return None;
    }
    match &packet[child.local.clone()] {
        b"creator" => Some(Field::Creator),
        b"rights" => Some(Field::Rights),
        b"description" => Some(Field::Description),
        b"subject" => Some(Field::Subject),
        _ => None,
    }
}

/// The four fields as an XMP packet states them: the first element of each
/// name wins. A packet `xmp_rdf` cannot scan states nothing.
pub(crate) fn xmp_fields(packet: &[u8]) -> Found {
    let mut found = Found::default();
    let Some(rdf) = xmp_rdf::scan(packet) else {
        return found;
    };
    for d in &rdf.descriptions {
        let scope = xmp_rdf::Scope::new(packet, &rdf, d);
        for child in &d.children {
            let Some(field) = dc_field(packet, &scope, child) else {
                continue;
            };
            let items = list_items(packet, child);
            // A language alternative: its `x-default` entry, else its first.
            let chosen = || {
                items
                    .iter()
                    .find(|item| item.0)
                    .or(items.first())
                    .map(|item| item.1.clone())
                    .unwrap_or_default()
            };
            let all = || items.iter().map(|item| item.1.clone()).collect();
            match field {
                Field::Creator if found.creator.is_none() => found.creator = Some(all()),
                Field::Subject if found.keywords.is_none() => found.keywords = Some(all()),
                Field::Rights if found.copyright.is_none() => found.copyright = Some(chosen()),
                Field::Description if found.description.is_none() => {
                    found.description = Some(chosen());
                }
                _ => {}
            }
        }
    }
    found
}

/// The `rdf:li` entries of one property element, each with whether it is
/// the `x-default` language entry. An element holding plain text instead of
/// a container is one entry. Entities are decoded; an entry holding markup
/// other than one CDATA section is skipped.
fn list_items(packet: &[u8], child: &xmp_rdf::Child) -> Vec<(bool, String)> {
    const OPEN: &[u8] = b"<rdf:li";
    const CLOSE: &[u8] = b"</rdf:li";
    let end = child.span.end.min(packet.len());
    let body = &packet[..end];
    let mut items = Vec::new();
    let mut at = child.span.start;
    while let Some(open) = find(body, OPEN, at) {
        let after = open + OPEN.len();
        at = after;
        // The name must be whole: `<rdf:list` is a different element.
        if !matches!(body.get(after), Some(b) if b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
        {
            continue;
        }
        let Some(tag_end) = tag_close(body, after) else {
            break;
        };
        at = tag_end + 1;
        if body[tag_end - 1] == b'/' {
            continue;
        }
        let Some(close) = find(body, CLOSE, tag_end + 1) else {
            break;
        };
        let default_lang = find(&body[..tag_end], b"xml:lang", after)
            .and_then(|lang| attribute_value(body, lang + b"xml:lang".len()))
            .is_some_and(|value| &body[value] == b"x-default");
        if let Some(text) = item_text(&body[tag_end + 1..close]) {
            items.push((default_lang, text));
        }
        at = close + CLOSE.len();
    }
    if items.is_empty() && find(body, OPEN, child.span.start).is_none() {
        if let Some(text) = xmp_rdf::element_text(packet, child) {
            let text = unescaped(&packet[text]);
            if !text.trim().is_empty() {
                items.push((true, text));
            }
        }
    }
    items
}

/// The offset of the `>` that closes a start tag, given an offset inside
/// it; a `>` inside a quoted attribute value does not count.
fn tag_close(body: &[u8], from: usize) -> Option<usize> {
    let mut quote = None;
    for (index, byte) in body.iter().enumerate().skip(from) {
        match quote {
            Some(q) if *byte == q => quote = None,
            Some(_) => {}
            None if *byte == b'"' || *byte == b'\'' => quote = Some(*byte),
            None if *byte == b'>' => return Some(index),
            None => {}
        }
    }
    None
}

/// The text of one `rdf:li`: plain text with its entities decoded, or the
/// inside of a single CDATA section. `None` for any other markup.
fn item_text(raw: &[u8]) -> Option<String> {
    const CDATA: &[u8] = b"<![CDATA[";
    if !raw.contains(&b'<') {
        return Some(unescaped(raw));
    }
    let trimmed = raw.trim_ascii();
    let inner = trimmed.strip_prefix(CDATA)?.strip_suffix(b"]]>")?;
    (find(inner, b"]]>", 0).is_none()).then(|| String::from_utf8_lossy(inner).into_owned())
}

/// XML character data with the five named entities and numeric character
/// references decoded — the inverse of `metadata_xmp::escaped`. Anything
/// that is not a well-formed reference is kept as it stands.
fn unescaped(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    while i < raw.len() {
        if raw[i] == b'&' {
            // The longest reference is `&#x10FFFF;`.
            let semi = raw[i..].iter().take(12).position(|&b| b == b';');
            if let Some(semi) = semi {
                let name = &raw[i + 1..i + semi];
                let decoded = match name {
                    b"amp" => Some('&'),
                    b"lt" => Some('<'),
                    b"gt" => Some('>'),
                    b"quot" => Some('"'),
                    b"apos" => Some('\''),
                    _ => std::str::from_utf8(name).ok().and_then(|name| {
                        let code = match name.strip_prefix("#x").or(name.strip_prefix("#X")) {
                            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                            None => name.strip_prefix('#')?.parse::<u32>().ok()?,
                        };
                        char::from_u32(code)
                    }),
                };
                if let Some(c) = decoded {
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                    i += semi + 1;
                    continue;
                }
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Which Dublin Core properties an edit replaces, and with what. `None`
/// leaves that property's elements exactly as they are; `Some` removes every
/// one of them and writes the value when it is not empty.
pub(crate) struct DcEdit<'a> {
    pub creator: Option<&'a [String]>,
    pub rights: Option<&'a str>,
    pub description: Option<&'a str>,
    pub subject: Option<&'a [String]>,
}

impl DcEdit<'_> {
    fn replaces(&self, field: Field) -> bool {
        match field {
            Field::Creator => self.creator.is_some(),
            Field::Rights => self.rights.is_some(),
            Field::Description => self.description.is_some(),
            Field::Subject => self.subject.is_some(),
        }
    }

    /// True when at least one replaced property has a value to write.
    fn has_content(&self) -> bool {
        self.creator.is_some_and(|list| !list.is_empty())
            || self.rights.is_some_and(|text| !text.is_empty())
            || self.description.is_some_and(|text| !text.is_empty())
            || self.subject.is_some_and(|list| !list.is_empty())
    }

    /// The property elements, in the containers the XMP specification gives
    /// them: `dc:creator` an ordered list, `dc:rights` and `dc:description`
    /// language alternatives with the conventional `x-default` entry,
    /// `dc:subject` an unordered list. The ONE writer of these four shapes;
    /// a metadata policy's stamp goes through it too
    /// (`metadata_xmp::properties`).
    pub(crate) fn properties(&self, out: &mut Vec<u8>) {
        let list = |out: &mut Vec<u8>, name: &str, container: &str, entries: &[String]| {
            if entries.is_empty() {
                return;
            }
            out.extend_from_slice(format!("<dc:{name}><rdf:{container}>").as_bytes());
            for entry in entries {
                out.extend_from_slice(b"<rdf:li>");
                escaped(out, entry.as_bytes());
                out.extend_from_slice(b"</rdf:li>");
            }
            out.extend_from_slice(format!("</rdf:{container}></dc:{name}>").as_bytes());
        };
        let alternative = |out: &mut Vec<u8>, name: &str, text: &str| {
            if text.is_empty() {
                return;
            }
            out.extend_from_slice(
                format!("<dc:{name}><rdf:Alt><rdf:li xml:lang=\"x-default\">").as_bytes(),
            );
            escaped(out, text.as_bytes());
            out.extend_from_slice(format!("</rdf:li></rdf:Alt></dc:{name}>").as_bytes());
        };
        list(out, "creator", "Seq", self.creator.unwrap_or_default());
        alternative(out, "rights", self.rights.unwrap_or_default());
        alternative(out, "description", self.description.unwrap_or_default());
        list(out, "subject", "Bag", self.subject.unwrap_or_default());
    }
}

/// `packet` with the properties `edit` replaces removed wherever they are
/// and written once, in a new `rdf:Description` before `</rdf:RDF>` (see the
/// module doc). `Err` when `xmp_rdf` cannot describe the packet.
pub(crate) fn xmp_replaced(packet: &[u8], edit: &DcEdit<'_>) -> Result<Vec<u8>, String> {
    let rdf = xmp_rdf::scan(packet).ok_or(
        "file info cannot be written: the document's xmp packet has no rdf element this build can edit",
    )?;
    let mut spans: Vec<Range<usize>> = Vec::new();
    for d in &rdf.descriptions {
        let scope = xmp_rdf::Scope::new(packet, &rdf, d);
        let own: Vec<Range<usize>> = d
            .children
            .iter()
            .filter(|child| dc_field(packet, &scope, child).is_some_and(|f| edit.replaces(f)))
            .map(|child| child.span.clone())
            .collect();
        // A Description left with nothing but `rdf:about` and namespace
        // declarations states nothing, so it goes with its last property.
        let bare = d
            .attrs
            .iter()
            .all(|a| a.is_declaration(packet) || a.name(packet) == b"rdf:about");
        if !own.is_empty() && own.len() == d.children.len() && bare && d.end_tag.is_some() {
            spans.push(d.span());
        } else {
            spans.extend(own);
        }
    }
    spans.sort_by_key(|span| span.start);
    // Every span lies inside `rdf:RDF`, before its end tag.
    let removed: usize = spans.iter().map(|span| span.len()).sum();
    let at = rdf
        .rdf_end
        .checked_sub(removed)
        .ok_or("file info cannot be written: the xmp packet's structure is inconsistent")?;
    let mut out = xmp_rdf::without(packet, &spans);
    if edit.has_content() && at <= out.len() {
        let mut description = Vec::with_capacity(256);
        dc_description(&mut description, |properties| edit.properties(properties));
        out.splice(at..at, description);
    }
    Ok(out)
}
