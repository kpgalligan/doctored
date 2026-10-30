//! A bounded byte scanner over the `rdf:RDF` of one XMP packet — just enough
//! structure to move whole properties between packets and to find one by
//! its namespace, which is what JPEG ExtendedXMP needs on both sides
//! (`metadata_xmp_extended` reads, `metadata_xmp_split` writes) and what the
//! export's removal of Google's pixel-bound properties needs
//! (`metadata_xmp::xmp_normalized`).
//!
//! # What it accepts
//!
//! The shape every XMP writer emits: an optional `x:xmpmeta` start tag
//! before the first `<rdf:RDF` (both found by their literal, conventional
//! names, as `metadata_xmp` keys on the literal `tiff:` prefix), whose
//! `rdf:RDF` holds only whitespace, comments and `rdf:Description`
//! elements, and whose Descriptions hold only whitespace, comments,
//! processing instructions and child elements. Inside a child element
//! anything well-formed goes: text (a `>` in text is legal XML), nested
//! elements matched by name on a stack, comments, CDATA sections and
//! processing instructions.
//!
//! Anything else answers `None` — a packet it cannot describe exactly is a
//! packet no caller may rewrite, and every caller then leaves the bytes as
//! they were. That covers an unexpected byte, an unterminated tag, comment,
//! CDATA section or processing instruction, a mismatched end tag, two
//! attributes with no whitespace between them, and nesting deeper than
//! [`MAX_DEPTH`].
//!
//! # Cost
//!
//! One forward pass. Every byte is visited a bounded number of times —
//! attribute values are skipped through `metadata_xmp::attribute_value`,
//! which honours a `>` inside quotes — so the scan is linear in the packet,
//! and it never panics: every index is checked.
//!
//! It is a scanner, not an XML parser: entities are not decoded (a namespace
//! URI is compared as its raw bytes, which is how every writer spells one),
//! and nothing outside `rdf:RDF` is interpreted.
//!
//! JPEG ExtendedXMP: `xmp_rdf` is the bounded, linear, never-panicking scanner over a
//! packet's `rdf:RDF` (Descriptions, their property attributes and child elements, namespace
//! resolution).

use std::collections::HashMap;
use std::ops::Range;

use crate::metadata_xmp::{attribute_value, find, skip_space};

/// The deepest element nesting inside one Description child. Real packets
/// nest a handful of levels (`rdf:Seq` in an `rdf:li` in a struct); the cap
/// only keeps a crafted packet's stack bounded.
pub(crate) const MAX_DEPTH: usize = 128;

/// One attribute of a start tag. All offsets index the scanned packet.
#[derive(Clone, Debug)]
pub(crate) struct Attr {
    /// From the whitespace that precedes the name to the closing quote, so
    /// removing it leaves a well-formed tag and copying it carries its own
    /// separator.
    pub span: Range<usize>,
    /// Empty when the name has no prefix.
    pub prefix: Range<usize>,
    pub local: Range<usize>,
    /// Between the quotes, entities undecoded.
    pub value: Range<usize>,
}

impl Attr {
    /// The whole qualified name.
    pub(crate) fn name<'p>(&self, packet: &'p [u8]) -> &'p [u8] {
        let start = if self.prefix.is_empty() {
            self.local.start
        } else {
            self.prefix.start
        };
        &packet[start..self.local.end]
    }

    /// `xmlns` or `xmlns:*` — a namespace declaration, never a property.
    pub(crate) fn is_declaration(&self, packet: &[u8]) -> bool {
        let prefix = &packet[self.prefix.clone()];
        prefix == b"xmlns" || (prefix.is_empty() && &packet[self.local.clone()] == b"xmlns")
    }

    /// The prefix a declaration binds: `P` for `xmlns:P`, empty for
    /// `xmlns`; `None` for any other attribute.
    pub(crate) fn declared_prefix<'p>(&self, packet: &'p [u8]) -> Option<&'p [u8]> {
        let (prefix, local) = (&packet[self.prefix.clone()], &packet[self.local.clone()]);
        if prefix == b"xmlns" {
            Some(local)
        } else if prefix.is_empty() && local == b"xmlns" {
            Some(&[])
        } else {
            None
        }
    }

    /// `xml:lang`, which is inherited like a declaration.
    pub(crate) fn is_lang(&self, packet: &[u8]) -> bool {
        &packet[self.prefix.clone()] == b"xml" && &packet[self.local.clone()] == b"lang"
    }
}

/// One child element of a Description — a property in element form.
#[derive(Clone, Debug)]
pub(crate) struct Child {
    /// From its `<` to the end of its end tag, or of its `/>`.
    pub span: Range<usize>,
    pub prefix: Range<usize>,
    pub local: Range<usize>,
    /// The namespace declarations on its own start tag.
    pub declarations: Vec<Attr>,
}

/// One `rdf:Description`.
#[derive(Clone, Debug)]
pub(crate) struct Description {
    pub start_tag: Range<usize>,
    /// Where an attribute can be inserted: the offset of the start tag's
    /// `/>` or `>`.
    pub insert_at: usize,
    /// `None` for a self-closing Description.
    pub end_tag: Option<Range<usize>>,
    pub attrs: Vec<Attr>,
    pub children: Vec<Child>,
}

impl Description {
    /// The whole element.
    pub(crate) fn span(&self) -> Range<usize> {
        let end = self
            .end_tag
            .as_ref()
            .map_or(self.start_tag.end, |tag| tag.end);
        self.start_tag.start..end
    }
}

/// What [`scan`] found.
#[derive(Clone, Debug)]
pub(crate) struct Rdf {
    /// The `xmlns*` and `xml:lang` attributes of an enclosing `x:xmpmeta`,
    /// then of `rdf:RDF`, in that order.
    pub inherited: Vec<Attr>,
    /// The offset of `</rdf:RDF>`.
    pub rdf_end: usize,
    pub descriptions: Vec<Description>,
}

/// The structure of `packet`'s `rdf:RDF`, or `None` when it is not one this
/// scanner can describe exactly (see the module doc).
pub(crate) fn scan(packet: &[u8]) -> Option<Rdf> {
    let rdf_at = find_element(packet, b"<rdf:RDF", packet.len())?;
    let mut inherited = Vec::new();
    if let Some(meta_at) = find_element(packet, b"<x:xmpmeta", rdf_at) {
        let meta = start_tag(packet, meta_at)?;
        if meta.self_closing || meta.end > rdf_at {
            return None;
        }
        inherited.extend(scoped(packet, meta.attrs));
    }
    let rdf = start_tag(packet, rdf_at)?;
    if rdf.self_closing {
        return None;
    }
    inherited.extend(scoped(packet, rdf.attrs));
    let mut descriptions = Vec::new();
    let mut i = rdf.end;
    loop {
        i = skip_space(packet, i);
        let rest = packet.get(i..)?;
        if rest.starts_with(b"<!--") {
            i = skip_past(packet, i + 4, b"-->")?;
        } else if rest.starts_with(b"</") {
            let (name, _) = end_tag(packet, i)?;
            if &packet[name] != b"rdf:RDF" {
                return None;
            }
            return Some(Rdf {
                inherited,
                rdf_end: i,
                descriptions,
            });
        } else if element_at(rest, b"<rdf:Description") {
            let (description, next) = description(packet, i)?;
            descriptions.push(description);
            i = next;
        } else {
            return None;
        }
    }
}

/// The namespace bindings in scope for the properties of one Description,
/// built ONCE per Description: the declarations of `rdf.inherited`, then
/// the Description's own, the LAST of each prefix winning. Asking per
/// property instead re-walked the Description's attributes for every one of
/// them, which is quadratic in a packet with 10⁵ attributes.
pub(crate) struct Scope<'p> {
    bindings: HashMap<&'p [u8], &'p [u8]>,
}

impl<'p> Scope<'p> {
    pub(crate) fn new(packet: &'p [u8], rdf: &Rdf, d: &Description) -> Self {
        let mut bindings = HashMap::new();
        for a in rdf.inherited.iter().chain(&d.attrs) {
            if let Some(prefix) = a.declared_prefix(packet) {
                bindings.insert(prefix, &packet[a.value.clone()]);
            }
        }
        Scope { bindings }
    }

    /// The URI bound to `prefix` where a property sits: `own` (a Child's
    /// declarations, else empty) first, its LAST declaration winning, then
    /// this scope. An empty `prefix` asks for the default namespace
    /// (`xmlns`).
    pub(crate) fn resolve(
        &self,
        packet: &'p [u8],
        own: &[Attr],
        prefix: &[u8],
    ) -> Option<&'p [u8]> {
        own.iter()
            .rev()
            .find(|a| a.declared_prefix(packet) == Some(prefix))
            .map(|a| &packet[a.value.clone()])
            .or_else(|| self.bindings.get(prefix).copied())
    }
}

/// The text content of a simple-valued child element, `None` when it is
/// self-closing or holds markup.
pub(crate) fn element_text(packet: &[u8], child: &Child) -> Option<Range<usize>> {
    let tag = start_tag(packet, child.span.start)?;
    if tag.self_closing {
        return None;
    }
    let body = packet.get(tag.end..child.span.end)?;
    let close = body.windows(2).rposition(|w| w == b"</")?;
    let text = tag.end..tag.end + close;
    (!packet[text.clone()].contains(&b'<')).then_some(text)
}

/// `packet` with every span in `spans` cut out. The spans must be sorted
/// and disjoint, which every caller's spans are (they come from one scan).
pub(crate) fn without(packet: &[u8], spans: &[Range<usize>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(packet.len());
    let mut cursor = 0usize;
    for span in spans {
        if span.start < cursor || span.end > packet.len() {
            continue;
        }
        out.extend_from_slice(&packet[cursor..span.start]);
        cursor = span.end;
    }
    out.extend_from_slice(&packet[cursor..]);
    out
}

// ---------------------------------------------------------------- lexing --

/// A lexed start tag.
struct StartTag {
    name: Range<usize>,
    prefix: Range<usize>,
    local: Range<usize>,
    attrs: Vec<Attr>,
    /// The offset of `/>` or `>`.
    close: usize,
    /// Just past the tag.
    end: usize,
    self_closing: bool,
}

/// The first `open` (an element's `<` and name) before `end` whose name is
/// whole — followed by whitespace, `>` or `/`.
fn find_element(packet: &[u8], open: &[u8], end: usize) -> Option<usize> {
    let hay = packet.get(..end)?;
    let mut from = 0usize;
    loop {
        let at = find(hay, open, from)?;
        if element_at(&packet[at..], open) {
            return Some(at);
        }
        from = at + 1;
    }
}

/// True when `rest` starts with `open` as a whole element name.
fn element_at(rest: &[u8], open: &[u8]) -> bool {
    rest.starts_with(open)
        && matches!(rest.get(open.len()), Some(b) if
            b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
}

/// Just past the first `needle` at or after `from`.
fn skip_past(packet: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    Some(find(packet, needle, from)? + needle.len())
}

/// The end of a name starting at `at`: the first whitespace, `>`, `/` or
/// `=`.
fn name_end(packet: &[u8], at: usize) -> usize {
    let len = packet.get(at..).map_or(0, |rest| {
        rest.iter()
            .position(|b| b.is_ascii_whitespace() || matches!(b, b'>' | b'/' | b'='))
            .unwrap_or(rest.len())
    });
    at + len
}

/// A qualified name split at its first colon. `None` for an empty name, an
/// empty half around the colon, or a byte no name may hold.
fn split(packet: &[u8], name: Range<usize>) -> Option<(Range<usize>, Range<usize>)> {
    let bytes = packet.get(name.clone())?;
    if bytes.is_empty()
        || bytes
            .iter()
            .any(|b| matches!(b, b'<' | b'"' | b'\'' | b'!' | b'?'))
    {
        return None;
    }
    match bytes.iter().position(|&b| b == b':') {
        Some(colon) => {
            let prefix = name.start..name.start + colon;
            let local = name.start + colon + 1..name.end;
            (!prefix.is_empty() && !local.is_empty()).then_some((prefix, local))
        }
        None => Some((name.start..name.start, name)),
    }
}

/// The start tag whose `<` is at `at`.
fn start_tag(packet: &[u8], at: usize) -> Option<StartTag> {
    if packet.get(at) != Some(&b'<') {
        return None;
    }
    let name = at + 1..name_end(packet, at + 1);
    let (prefix, local) = split(packet, name.clone())?;
    let mut attrs = Vec::new();
    let mut i = name.end;
    loop {
        let separator = i;
        i = skip_space(packet, i);
        match *packet.get(i)? {
            b'>' => {
                return Some(StartTag {
                    name,
                    prefix,
                    local,
                    attrs,
                    close: i,
                    end: i + 1,
                    self_closing: false,
                });
            }
            b'/' => {
                if packet.get(i + 1) != Some(&b'>') {
                    return None;
                }
                return Some(StartTag {
                    name,
                    prefix,
                    local,
                    attrs,
                    close: i,
                    end: i + 2,
                    self_closing: true,
                });
            }
            _ => {
                // XML requires whitespace between attributes.
                if i == separator {
                    return None;
                }
                let attr_name = i..name_end(packet, i);
                let (attr_prefix, attr_local) = split(packet, attr_name.clone())?;
                let value = attribute_value(packet, attr_name.end)?;
                attrs.push(Attr {
                    span: separator..value.end + 1,
                    prefix: attr_prefix,
                    local: attr_local,
                    value: value.clone(),
                });
                i = value.end + 1;
            }
        }
    }
}

/// The end tag whose `</` is at `at`: its name and the offset just past it.
fn end_tag(packet: &[u8], at: usize) -> Option<(Range<usize>, usize)> {
    let name = at + 2..name_end(packet, at + 2);
    split(packet, name.clone())?;
    let close = skip_space(packet, name.end);
    (packet.get(close) == Some(&b'>')).then_some((name, close + 1))
}

/// The declarations and `xml:lang` among a wrapper's attributes.
fn scoped(packet: &[u8], attrs: Vec<Attr>) -> impl Iterator<Item = Attr> + '_ {
    attrs
        .into_iter()
        .filter(move |a| a.is_declaration(packet) || a.is_lang(packet))
}

/// The `rdf:Description` whose `<` is at `at`, and the offset just past it.
fn description(packet: &[u8], at: usize) -> Option<(Description, usize)> {
    let tag = start_tag(packet, at)?;
    if tag.self_closing {
        return Some((
            Description {
                start_tag: at..tag.end,
                insert_at: tag.close,
                end_tag: None,
                attrs: tag.attrs,
                children: Vec::new(),
            },
            tag.end,
        ));
    }
    let mut children = Vec::new();
    let mut i = tag.end;
    loop {
        i = skip_space(packet, i);
        let rest = packet.get(i..)?;
        if rest.starts_with(b"<!--") {
            i = skip_past(packet, i + 4, b"-->")?;
        } else if rest.starts_with(b"<?") {
            i = skip_past(packet, i + 2, b"?>")?;
        } else if rest.starts_with(b"</") {
            let (name, end) = end_tag(packet, i)?;
            if &packet[name] != b"rdf:Description" {
                return None;
            }
            return Some((
                Description {
                    start_tag: at..tag.end,
                    insert_at: tag.close,
                    end_tag: Some(i..end),
                    attrs: tag.attrs,
                    children,
                },
                end,
            ));
        } else if rest.starts_with(b"<") {
            let (child, end) = element(packet, i)?;
            children.push(child);
            i = end;
        } else {
            return None;
        }
    }
}

/// The element whose `<` is at `at`, and the offset just past it.
fn element(packet: &[u8], at: usize) -> Option<(Child, usize)> {
    let tag = start_tag(packet, at)?;
    let declarations: Vec<Attr> = tag
        .attrs
        .into_iter()
        .filter(|a| a.is_declaration(packet))
        .collect();
    let child = |end: usize| Child {
        span: at..end,
        prefix: tag.prefix.clone(),
        local: tag.local.clone(),
        declarations: declarations.clone(),
    };
    if tag.self_closing {
        return Some((child(tag.end), tag.end));
    }
    let mut open: Vec<Range<usize>> = vec![tag.name.clone()];
    let mut i = tag.end;
    loop {
        let lt = i + packet.get(i..)?.iter().position(|&b| b == b'<')?;
        let rest = &packet[lt..];
        if rest.starts_with(b"<!--") {
            i = skip_past(packet, lt + 4, b"-->")?;
        } else if rest.starts_with(b"<![CDATA[") {
            i = skip_past(packet, lt + 9, b"]]>")?;
        } else if rest.starts_with(b"<?") {
            i = skip_past(packet, lt + 2, b"?>")?;
        } else if rest.starts_with(b"</") {
            let (name, end) = end_tag(packet, lt)?;
            let opened = open.pop()?;
            if packet[opened] != packet[name] {
                return None;
            }
            if open.is_empty() {
                return Some((child(end), end));
            }
            i = end;
        } else {
            let inner = start_tag(packet, lt)?;
            if !inner.self_closing {
                if open.len() >= MAX_DEPTH {
                    return None;
                }
                open.push(inner.name);
            }
            i = inner.end;
        }
    }
}
