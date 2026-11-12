//! The ONE reader/writer for Photoshop's descriptor structure (Adobe's
//! "Descriptor structure", the `Objc` grammar, always version 16 here). It is
//! framing only: `psd_adjustment` (adjustment/fill bodies) and
//! `psd_layer_effects`/`psd_layer_effects_fx` (the `lfx2` layer-effects body)
//! are its callers and own the meaning of every class and key, so a new
//! descriptor-body record adds arms there, not here.
//!
//! Grammar (big-endian): a unicode string is `u32 count` then `count × u16`
//! (a trailing 0 is stripped on read, written back on write); an id (class or
//! key) is `u32 len` then the bytes, where `len = 0` means four raw bytes; a
//! descriptor is name, classID, `u32 count`, then `count` items; an item is an
//! id, a four-byte OSType and the value. Values read into `Item`: `long`
//! (i32), `doub` (f64), `bool` (u8), `TEXT` (unicode), `Objc`/`GlbO`
//! (descriptor), `UntF` (unit + f64), `enum` (typeID + enumID), `VlLs`
//! (`u32 count` × (OSType + value)). Walked and skipped as `Item::Other`:
//! `tdta`/`alis` (`u32 len` + bytes), `comp` (8 bytes), `type`/`GlbC`
//! (unicode + id). `obj `, `ObAr` and any other OSType decline the whole body
//! — except under [`read_with_references`] (an `.asl` style), which
//! reads an `obj ` reference into `Item::Reference`: each `Enmr` item as
//! [`RefItem::Enumerated`] (class, type and enum ids), each `prop`, `Clss`,
//! `rele`, `Idnt`, `indx` and `name` item walked as [`RefItem::Other`], any
//! other form declining. The writer always writes a reference: its `Enmr`
//! items, `RefItem::Other` skipped.
//!
//! Reader discipline: every declared length is checked against the remaining
//! slice before use, truncation anywhere is the shared shorter-than-layout
//! reason, nesting past 16 and counts past 4096 decline, and nothing panics.
//! The writer emits a four-byte id as `len = 0` + the bytes and any other
//! length explicitly (an [`Item::EnumStringId`]'s enum id always
//! explicitly, and under [`write_with_string_ids`] each id it names, the way
//! Photoshop writes resource 1065's `list` and `Comp`, `psd_comp_records`),
//! unicode as `chars + 1` with the trailing 0, and skips `Item::Other`
//! without counting it.
//!
//! `psd_descriptor.rs` is the ONE Photoshop descriptor-structure reader/writer (version 16, the
//! `Objc` grammar: `long`/`doub`/`bool`/`TEXT`/`Objc`/`UntF`/`enum`/`VlLs` read,
//! `tdta`/`alis`/`comp`/`type` skipped, `obj `/`ObAr` declined, bounded nesting and counts, every
//! length checked before use); it is framing only and its callers — `psd_adjustment` and
//! `psd_layer_effects`/`psd_layer_effects_fx` — own the meaning of every class and key.

/// One descriptor: its class id and its items in file order.
pub(crate) struct Descriptor {
    pub class: Vec<u8>,
    pub items: Vec<(Vec<u8>, Item)>,
}

/// One item value.
pub(crate) enum Item {
    Long(i32),
    Double(f64),
    Bool(bool),
    Text(String),
    Object(Descriptor),
    Unit([u8; 4], f64),
    Enum(Vec<u8>, Vec<u8>),
    /// An `enum` whose enum id is a stringID even at four bytes (`deep`,
    /// which has no four-char code): written with its length, never as a
    /// charID. Written only; the reader returns every `enum` as
    /// [`Item::Enum`].
    EnumStringId(Vec<u8>, Vec<u8>),
    List(Vec<Item>),
    /// A walked-and-skipped value (`tdta`, `alis`, `comp`, `type`, `GlbC`).
    Other,
    /// An `obj ` reference, read only by [`read_with_references`] (module
    /// doc).
    Reference(Vec<RefItem>),
}

/// One item of an `obj ` reference (module doc).
pub(crate) enum RefItem {
    /// `Enmr`: the class id, the enum's type id and its value.
    Enumerated {
        class: Vec<u8>,
        kind: Vec<u8>,
        value: Vec<u8>,
    },
    /// Any other form, walked and skipped.
    Other,
}

const SHORT: &str = "body is shorter than its layout";
const UNSUPPORTED: &str = "descriptor uses an item type this importer does not read";
const DEEP: &str = "descriptor is nested too deeply";
const VERSION: &str = "descriptor version is not 16";
const MAX_DEPTH: usize = 16;
const MAX_COUNT: u32 = 4096;

impl Descriptor {
    fn find(&self, key: &[u8]) -> Option<&Item> {
        self.items
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, item)| item)
    }

    fn unexpected(key: &[u8]) -> String {
        format!(
            "descriptor key {} has an unexpected type",
            String::from_utf8_lossy(key).trim_end()
        )
    }

    /// `Ok(None)` when absent; the unexpected-type reason when present with
    /// another type.
    pub fn long(&self, key: &[u8]) -> Result<Option<i32>, String> {
        match self.find(key) {
            None => Ok(None),
            Some(Item::Long(value)) => Ok(Some(*value)),
            Some(_) => Err(Self::unexpected(key)),
        }
    }

    pub fn double(&self, key: &[u8]) -> Result<Option<f64>, String> {
        match self.find(key) {
            None => Ok(None),
            Some(Item::Double(value)) => Ok(Some(*value)),
            Some(_) => Err(Self::unexpected(key)),
        }
    }

    pub fn bool(&self, key: &[u8]) -> Result<Option<bool>, String> {
        match self.find(key) {
            None => Ok(None),
            Some(Item::Bool(value)) => Ok(Some(*value)),
            Some(_) => Err(Self::unexpected(key)),
        }
    }

    pub fn object(&self, key: &[u8]) -> Result<Option<&Descriptor>, String> {
        match self.find(key) {
            None => Ok(None),
            Some(Item::Object(value)) => Ok(Some(value)),
            Some(_) => Err(Self::unexpected(key)),
        }
    }

    /// A `UntF` value WITHOUT its unit tag: every caller knows which unit its
    /// key carries, and Photoshop writes the same key with `#Prc`, `#Pxl` or
    /// `#Ang` interchangeably across versions.
    pub fn unit(&self, key: &[u8]) -> Result<Option<f64>, String> {
        match self.find(key) {
            None => Ok(None),
            Some(Item::Unit(_, value)) => Ok(Some(*value)),
            Some(_) => Err(Self::unexpected(key)),
        }
    }

    /// An `enum` value's ENUM id (its type id is the key's own vocabulary).
    pub fn enumeration(&self, key: &[u8]) -> Result<Option<&[u8]>, String> {
        match self.find(key) {
            None => Ok(None),
            Some(Item::Enum(_, value)) => Ok(Some(value)),
            Some(_) => Err(Self::unexpected(key)),
        }
    }

    pub fn list(&self, key: &[u8]) -> Result<Option<&[Item]>, String> {
        match self.find(key) {
            None => Ok(None),
            Some(Item::List(values)) => Ok(Some(values)),
            Some(_) => Err(Self::unexpected(key)),
        }
    }

    /// An `obj ` reference's first `Enmr` value (a Blend If entry's `Chnl`,
    /// `asl`); the unexpected-type reason for any other value, or a
    /// reference without one.
    pub fn enumerated_reference(&self, key: &[u8]) -> Result<Option<&[u8]>, String> {
        match self.find(key) {
            None => Ok(None),
            Some(Item::Reference(items)) => items
                .iter()
                .find_map(|item| match item {
                    RefItem::Enumerated { value, .. } => Some(Some(value.as_slice())),
                    RefItem::Other => None,
                })
                .ok_or_else(|| Self::unexpected(key)),
            Some(_) => Err(Self::unexpected(key)),
        }
    }
}

// --------------------------------------------------------------- reading --

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// Whether an `obj ` reference reads ([`read_with_references`]) or
    /// declines the body.
    references: bool,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(count).ok_or(SHORT)?;
        let out = self.bytes.get(self.at..end).ok_or(SHORT)?;
        self.at = end;
        Ok(out)
    }
    fn four(&mut self) -> Result<[u8; 4], String> {
        let bytes = self.take(4)?;
        Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.four()?))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(self.four()?))
    }
    fn f64(&mut self) -> Result<f64, String> {
        let bytes = self.take(8)?;
        Ok(f64::from_be_bytes(bytes.try_into().map_err(|_| SHORT)?))
    }
    fn unicode(&mut self) -> Result<String, String> {
        let count = self.u32()? as usize;
        let bytes = self.take(count.checked_mul(2).ok_or(SHORT)?)?;
        let mut units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        if units.last() == Some(&0) {
            units.pop();
        }
        Ok(String::from_utf16_lossy(&units))
    }
    fn id(&mut self) -> Result<Vec<u8>, String> {
        let len = self.u32()?;
        let len = if len == 0 { 4 } else { len as usize };
        Ok(self.take(len)?.to_vec())
    }
    fn descriptor(&mut self, depth: usize) -> Result<Descriptor, String> {
        if depth > MAX_DEPTH {
            return Err(DEEP.to_string());
        }
        let _name = self.unicode()?;
        let class = self.id()?;
        let count = self.u32()?;
        if count > MAX_COUNT {
            return Err(UNSUPPORTED.to_string());
        }
        let mut items = Vec::new();
        for _ in 0..count {
            let key = self.id()?;
            let kind = self.four()?;
            let item = self.value(kind, depth)?;
            items.push((key, item));
        }
        Ok(Descriptor { class, items })
    }
    fn value(&mut self, kind: [u8; 4], depth: usize) -> Result<Item, String> {
        Ok(match &kind {
            b"long" => Item::Long(self.i32()?),
            b"doub" => Item::Double(self.f64()?),
            b"bool" => Item::Bool(self.u8()? != 0),
            b"TEXT" => Item::Text(self.unicode()?),
            b"Objc" | b"GlbO" => Item::Object(self.descriptor(depth + 1)?),
            b"UntF" => {
                let unit = self.four()?;
                Item::Unit(unit, self.f64()?)
            }
            b"enum" => {
                let type_id = self.id()?;
                Item::Enum(type_id, self.id()?)
            }
            b"VlLs" => {
                if depth >= MAX_DEPTH {
                    return Err(DEEP.to_string());
                }
                let count = self.u32()?;
                if count > MAX_COUNT {
                    return Err(UNSUPPORTED.to_string());
                }
                let mut values = Vec::new();
                for _ in 0..count {
                    let kind = self.four()?;
                    values.push(self.value(kind, depth + 1)?);
                }
                Item::List(values)
            }
            b"tdta" | b"alis" => {
                let len = self.u32()? as usize;
                self.take(len)?;
                Item::Other
            }
            b"comp" => {
                self.take(8)?;
                Item::Other
            }
            b"type" | b"GlbC" => {
                self.unicode()?;
                self.id()?;
                Item::Other
            }
            b"obj " if self.references => {
                let count = self.u32()?;
                if count > MAX_COUNT {
                    return Err(UNSUPPORTED.to_string());
                }
                let mut items = Vec::new();
                for _ in 0..count {
                    items.push(self.reference_item()?);
                }
                Item::Reference(items)
            }
            _ => return Err(UNSUPPORTED.to_string()),
        })
    }
    /// One `obj ` item: its form, then the form's fields.
    fn reference_item(&mut self) -> Result<RefItem, String> {
        let form = self.four()?;
        Ok(match &form {
            b"Enmr" => {
                self.unicode()?;
                let class = self.id()?;
                let kind = self.id()?;
                let value = self.id()?;
                RefItem::Enumerated { class, kind, value }
            }
            b"prop" => {
                self.unicode()?;
                self.id()?;
                self.id()?;
                RefItem::Other
            }
            b"Clss" => {
                self.unicode()?;
                self.id()?;
                RefItem::Other
            }
            b"rele" => {
                self.unicode()?;
                self.id()?;
                self.i32()?;
                RefItem::Other
            }
            b"Idnt" | b"indx" => {
                self.u32()?;
                RefItem::Other
            }
            b"name" => {
                self.unicode()?;
                self.id()?;
                self.unicode()?;
                RefItem::Other
            }
            _ => return Err(UNSUPPORTED.to_string()),
        })
    }
}

/// `u32 version` (must be 16) then the structure; any tail is ignored. `Err`
/// is the decline reason, read after "whose" in the import report.
pub(crate) fn read(body: &[u8]) -> Result<Descriptor, String> {
    let mut r = Reader {
        bytes: body,
        at: 0,
        references: false,
    };
    if r.u32()? != 16 {
        return Err(VERSION.to_string());
    }
    r.descriptor(0)
}

/// [`read`] that also returns how many bytes of `body` the version and the
/// structure took — for a caller whose descriptors follow one another (an
/// `.asl` style record's two, `asl`).
pub(crate) fn read_prefix(body: &[u8]) -> Result<(Descriptor, usize), String> {
    let mut r = Reader {
        bytes: body,
        at: 0,
        references: false,
    };
    if r.u32()? != 16 {
        return Err(VERSION.to_string());
    }
    let descriptor = r.descriptor(0)?;
    Ok((descriptor, r.at))
}

/// [`read`] that also reads `obj ` references (module doc): an `.asl`
/// style's `Styl`, whose Blend If entries name their channel by one.
pub(crate) fn read_with_references(body: &[u8]) -> Result<Descriptor, String> {
    let mut r = Reader {
        bytes: body,
        at: 0,
        references: true,
    };
    if r.u32()? != 16 {
        return Err(VERSION.to_string());
    }
    r.descriptor(0)
}

// --------------------------------------------------------------- writing --

/// `string_ids` are the four-byte ids written as STRING ids (an explicit
/// length) rather than as char ids (length 0), [`write_with_string_ids`].
fn push_id(out: &mut Vec<u8>, id: &[u8], string_ids: &[[u8; 4]]) {
    let char_id = id.len() == 4 && !string_ids.iter().any(|s| s.as_slice() == id);
    let len = if char_id { 0 } else { id.len() as u32 };
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(id);
}

fn push_unicode(out: &mut Vec<u8>, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    out.extend_from_slice(&((units.len() + 1) as u32).to_be_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out.extend_from_slice(&[0, 0]);
}

fn written(item: &Item) -> bool {
    !matches!(item, Item::Other)
}

fn write_descriptor(out: &mut Vec<u8>, descriptor: &Descriptor, ids: &[[u8; 4]]) {
    push_unicode(out, "");
    push_id(out, &descriptor.class, ids);
    let count = descriptor
        .items
        .iter()
        .filter(|(_, item)| written(item))
        .count();
    out.extend_from_slice(&(count as u32).to_be_bytes());
    for (key, item) in &descriptor.items {
        if written(item) {
            push_id(out, key, ids);
            write_item(out, item, ids);
        }
    }
}

fn write_item(out: &mut Vec<u8>, item: &Item, ids: &[[u8; 4]]) {
    match item {
        Item::Long(value) => {
            out.extend_from_slice(b"long");
            out.extend_from_slice(&value.to_be_bytes());
        }
        Item::Double(value) => {
            out.extend_from_slice(b"doub");
            out.extend_from_slice(&value.to_be_bytes());
        }
        Item::Bool(value) => {
            out.extend_from_slice(b"bool");
            out.push(u8::from(*value));
        }
        Item::Text(text) => {
            out.extend_from_slice(b"TEXT");
            push_unicode(out, text);
        }
        Item::Object(descriptor) => {
            out.extend_from_slice(b"Objc");
            write_descriptor(out, descriptor, ids);
        }
        Item::Unit(unit, value) => {
            out.extend_from_slice(b"UntF");
            out.extend_from_slice(unit);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Item::Enum(type_id, enum_id) => {
            out.extend_from_slice(b"enum");
            push_id(out, type_id, ids);
            push_id(out, enum_id, ids);
        }
        Item::EnumStringId(type_id, enum_id) => {
            out.extend_from_slice(b"enum");
            push_id(out, type_id, ids);
            out.extend_from_slice(&(enum_id.len() as u32).to_be_bytes());
            out.extend_from_slice(enum_id);
        }
        Item::List(values) => {
            out.extend_from_slice(b"VlLs");
            let count = values.iter().filter(|value| written(value)).count();
            out.extend_from_slice(&(count as u32).to_be_bytes());
            for value in values {
                if written(value) {
                    write_item(out, value, ids);
                }
            }
        }
        Item::Reference(items) => {
            out.extend_from_slice(b"obj ");
            let count = items
                .iter()
                .filter(|item| matches!(item, RefItem::Enumerated { .. }))
                .count();
            out.extend_from_slice(&(count as u32).to_be_bytes());
            for item in items {
                if let RefItem::Enumerated { class, kind, value } = item {
                    out.extend_from_slice(b"Enmr");
                    push_unicode(out, "");
                    push_id(out, class, ids);
                    push_id(out, kind, ids);
                    push_id(out, value, ids);
                }
            }
        }
        Item::Other => {}
    }
}

/// Version 16 then the structure, name "" on every descriptor. Never fails.
pub(crate) fn write(descriptor: &Descriptor) -> Vec<u8> {
    write_with_string_ids(descriptor, &[])
}

/// [`write`], with each four-byte id in `string_ids` written as a STRING
/// id (explicit length 4) wherever it appears as a class or key: Photoshop
/// writes `list` and `Comp` that way in resource 1065 (`psd_comp_records`),
/// where a char id would be a different id.
pub(crate) fn write_with_string_ids(descriptor: &Descriptor, string_ids: &[[u8; 4]]) -> Vec<u8> {
    let mut out = 16u32.to_be_bytes().to_vec();
    write_descriptor(&mut out, descriptor, string_ids);
    out
}
