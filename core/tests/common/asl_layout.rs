//! The Photoshop STYLE LIBRARY layout rules (`.asl`): an independent
//! walker of a library's bytes and the record structure Photoshop 26.10's
//! own libraries (`fixtures/psd_photoshop/styles_asl/styles_a.asl` and
//! `styles_b.asl`) keep. It is the oracle `psd_asl_layout.rs` holds every
//! export to, so it reads descriptors itself and never through the crate.
//!
//! The rules, each a [`Violation`]'s id:
//! - `framing`: `u16` 2, `8BSL`, `u16` 3; the pattern section's records are
//!   each `u32` length + record padded to 4 and fill the declared section
//!   exactly; the style count equals the records present; the file ends
//!   exactly after the last record.
//! - `descriptor-version`: a record is two descriptors, each after `u32` 16
//!   (a descriptor that does not walk is named here too).
//! - `record-padding`: the length is a multiple of 4 and only 0–3 zero bytes
//!   follow the two descriptors inside it.
//! - `text`: every TEXT counts its terminating NUL.
//! - `identity`: class `null`, items exactly `Nm  ` and `Idnt`, both TEXT.
//! - `idnt-uuid`: `Idnt` is 36 chars, `8-4-4-4-12` lowercase hex.
//! - `styl`: class `Styl`; items `documentMode`, `Lefx`, then an optional
//!   `blendOptions`, nothing else.
//! - `document-mode`: `documentMode` is an `Objc` of class `documentMode`
//!   with no items.
//! - `lefx`: class `Lefx`, its first item `Scl ` UntF `#Prc`.
//! - `lefx-master-switch` / `lefx-num-modifying`: no `masterFXSwitch` /
//!   `numModifyingFX` in `Lefx`.
//! - `blend-options-empty`: a `blendOptions` holds at least one item.
//! - `blend-options-order`: its keys are a subsequence of `Opct, Md  ,
//!   fillOpacity, blendInterior, knockout, Blnd`.
//! - `blend-options-full`: a `blendOptions` holds exactly those six keys,
//!   each once, in that order, of Photoshop's kinds (S9 of `styles_b.asl`):
//!   `Opct` and `fillOpacity` UntF `#Prc`, `Md  ` enum, `knockout` enum of
//!   the stringID type `knockout` whose id is the charID `None` or the
//!   stringID `shallow` or `deep` (S9 writes `shallow` with its length; a
//!   charID `deep` fails), `blendInterior` bool, `Blnd` a list (empty
//!   passes). Photoshop 26.10
//!   crashed loading a `blendOptions` without `Opct`/`Md  ` (the H11 hand
//!   load, `hand/H11.asl`).
#![allow(dead_code)]

/// (rule id, detail naming the record and the offset).
pub type Violation = (&'static str, String);

/// Photoshop's `blendOptions` key order (S9 of `styles_b.asl`).
pub const BLEND_OPTIONS_ORDER: [&[u8]; 6] = [
    b"Opct",
    b"Md  ",
    b"fillOpacity",
    b"blendInterior",
    b"knockout",
    b"Blnd",
];

/// A walked descriptor: its class and its items in file order.
#[derive(Debug, Clone)]
pub struct Desc {
    pub class: Vec<u8>,
    pub items: Vec<(Vec<u8>, Val)>,
}

/// A walked value: what the rules look at, the rest by its type alone.
#[derive(Debug, Clone)]
pub enum Val {
    Object(Desc),
    List(Vec<Val>),
    /// The UTF-16 units with the NUL stripped.
    Text(String),
    UnitFloat(Vec<u8>, f64),
    /// An `enum`: its type id and enum id, each as written.
    Enum(RawId, RawId),
    Other(Vec<u8>),
}

/// An id as written: its declared length (0 for a four-char charID) and its
/// bytes.
pub type RawId = (u32, Vec<u8>);

struct Walk<'a> {
    bytes: &'a [u8],
    at: usize,
    /// The file offset of `bytes[0]`.
    base: usize,
    /// The offsets of TEXT items that do not count their NUL.
    bare_text: Vec<usize>,
}

impl<'a> Walk<'a> {
    fn offset(&self) -> usize {
        self.base + self.at
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or_else(|| format!("short at {:#x}", self.offset()))?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// A key or class id: length 0 means four bytes.
    fn key(&mut self) -> Result<Vec<u8>, String> {
        Ok(self.raw_id()?.1)
    }
    /// An id with its declared length kept (charID 0, stringID its length).
    fn raw_id(&mut self) -> Result<RawId, String> {
        let n = self.u32()?;
        let bytes = self.take(if n == 0 { 4 } else { n as usize })?.to_vec();
        Ok((n, bytes))
    }
    /// A unicode string; `Err` past the bytes. Returns the units and whether
    /// the last one is the NUL.
    fn unicode(&mut self) -> Result<(String, bool), String> {
        let n = self.u32()? as usize;
        let raw = self.take(n.checked_mul(2).ok_or("unicode length overflows")?)?;
        let units: Vec<u16> = raw
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        let terminated = units.last() == Some(&0);
        let text = String::from_utf16_lossy(&units);
        Ok((text.trim_end_matches('\0').to_string(), terminated))
    }
    fn descriptor(&mut self) -> Result<Desc, String> {
        self.unicode()?;
        let class = self.key()?;
        let count = self.u32()?;
        let mut items = Vec::new();
        for _ in 0..count {
            let key = self.key()?;
            let kind = self.take(4)?.to_vec();
            items.push((key, self.value(&kind)?));
        }
        Ok(Desc { class, items })
    }
    fn value(&mut self, kind: &[u8]) -> Result<Val, String> {
        let at = self.offset();
        Ok(match kind {
            b"Objc" | b"GlbO" => Val::Object(self.descriptor()?),
            b"VlLs" => {
                let n = self.u32()?;
                let mut list = Vec::new();
                for _ in 0..n {
                    let k = self.take(4)?.to_vec();
                    list.push(self.value(&k)?);
                }
                Val::List(list)
            }
            b"TEXT" => {
                let (text, terminated) = self.unicode()?;
                if !terminated {
                    self.bare_text.push(at);
                }
                Val::Text(text)
            }
            b"UntF" => {
                let unit = self.take(4)?.to_vec();
                let b = self.take(8)?;
                let v = f64::from_be_bytes(b.try_into().map_err(|_| "short double")?);
                Val::UnitFloat(unit, v)
            }
            b"doub" | b"comp" => {
                self.take(8)?;
                Val::Other(kind.to_vec())
            }
            b"long" => {
                self.take(4)?;
                Val::Other(kind.to_vec())
            }
            b"bool" => {
                self.take(1)?;
                Val::Other(kind.to_vec())
            }
            b"enum" => Val::Enum(self.raw_id()?, self.raw_id()?),
            b"type" | b"GlbC" => {
                self.unicode()?;
                self.key()?;
                Val::Other(kind.to_vec())
            }
            b"alis" | b"tdta" | b"Pth " => {
                let n = self.u32()? as usize;
                self.take(n)?;
                Val::Other(kind.to_vec())
            }
            b"UnFl" => {
                self.take(4)?;
                let n = self.u32()? as usize;
                self.take(n.checked_mul(8).ok_or("unit list overflows")?)?;
                Val::Other(kind.to_vec())
            }
            b"obj " => {
                self.reference()?;
                Val::Other(kind.to_vec())
            }
            other => {
                return Err(format!(
                    "unknown type {:?} at {at:#x}",
                    String::from_utf8_lossy(other)
                ))
            }
        })
    }
    fn reference(&mut self) -> Result<(), String> {
        let n = self.u32()?;
        for _ in 0..n {
            let at = self.offset();
            match self.take(4)? {
                b"prop" => {
                    self.unicode()?;
                    self.key()?;
                    self.key()?;
                }
                b"Clss" => {
                    self.unicode()?;
                    self.key()?;
                }
                b"Enmr" => {
                    self.unicode()?;
                    self.key()?;
                    self.key()?;
                    self.key()?;
                }
                b"Idnt" | b"indx" => {
                    self.u32()?;
                }
                b"name" => {
                    self.unicode()?;
                    self.key()?;
                    self.unicode()?;
                }
                b"rele" => {
                    self.unicode()?;
                    self.key()?;
                    self.u32()?;
                }
                _ => return Err(format!("unknown reference form at {at:#x}")),
            }
        }
        Ok(())
    }
}

/// Every rule the library `bytes` breaks (module doc), in file order.
pub fn violations(bytes: &[u8]) -> Vec<Violation> {
    let mut out = Vec::new();
    let be32 = |at: usize| -> Option<usize> {
        let b = bytes.get(at..at + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let header = bytes.get(..8) == Some(b"\x00\x028BSL\x00\x03".as_slice());
    if !header {
        out.push((
            "framing",
            "the header is not u16 2, 8BSL, u16 3".to_string(),
        ));
        return out;
    }
    let Some(section) = be32(8) else {
        out.push(("framing", "no pattern section length at 0x8".to_string()));
        return out;
    };
    let start = 12;
    let end = start + section;
    if end > bytes.len() {
        out.push((
            "framing",
            format!("the pattern section passes the file at {start:#x}"),
        ));
        return out;
    }
    let mut at = start;
    while at < end {
        let Some(len) = be32(at) else {
            out.push((
                "framing",
                format!("pattern record at {at:#x} has no length"),
            ));
            break;
        };
        let next = at + 4 + len;
        let padded = next.next_multiple_of(4);
        if padded > end || bytes[next..padded].iter().any(|&b| b != 0) {
            out.push((
                "framing",
                format!("pattern record at {at:#x} does not end padded inside the section"),
            ));
            break;
        }
        at = padded;
    }
    if at != end {
        out.push((
            "framing",
            format!("the pattern records end at {at:#x}, not {end:#x}"),
        ));
    }
    let Some(count) = be32(end) else {
        out.push(("framing", format!("no style count at {end:#x}")));
        return out;
    };
    let mut at = end + 4;
    let mut present = 0;
    while at < bytes.len() {
        let Some(len) = be32(at) else {
            out.push((
                "framing",
                format!("record {present} at {at:#x} has no length"),
            ));
            break;
        };
        let Some(record) = bytes.get(at + 4..at + 4 + len) else {
            out.push((
                "framing",
                format!("record {present} at {at:#x} passes the file"),
            ));
            break;
        };
        record_violations(record, present, at + 4, &mut out);
        present += 1;
        let next = at + 4 + len;
        let padded = next.next_multiple_of(4);
        match bytes.get(next..padded) {
            Some(pad) if pad.iter().all(|&b| b == 0) => at = padded,
            _ => {
                out.push((
                    "framing",
                    format!("record {} ends at {next:#x} without its pad", present - 1),
                ));
                at = bytes.len();
            }
        }
    }
    if count != present {
        out.push((
            "framing",
            format!("the count is {count}, the file holds {present} records"),
        ));
    }
    out
}

/// The rules of one record at file offset `base` (module doc).
fn record_violations(record: &[u8], index: usize, base: usize, out: &mut Vec<Violation>) {
    let here = |what: &str| format!("record {index} at {base:#x}: {what}");
    let mut walk = Walk {
        bytes: record,
        at: 0,
        base,
        bare_text: Vec::new(),
    };
    let mut descriptors = Vec::new();
    for label in ["identity", "style"] {
        let version = walk.u32();
        if version != Ok(16) {
            out.push((
                "descriptor-version",
                here(&format!("{label} is not version 16")),
            ));
            return;
        }
        match walk.descriptor() {
            Ok(d) => descriptors.push(d),
            Err(reason) => {
                out.push(("descriptor-version", here(&format!("{label}: {reason}"))));
                return;
            }
        }
    }
    let rest = &record[walk.at..];
    if !record.len().is_multiple_of(4) {
        out.push((
            "record-padding",
            here(&format!("length {} is not a multiple of 4", record.len())),
        ));
    }
    if rest.len() > 3 || rest.iter().any(|&b| b != 0) {
        out.push((
            "record-padding",
            here(&format!("{} bytes follow the descriptors", rest.len())),
        ));
    }
    for at in &walk.bare_text {
        out.push(("text", here(&format!("TEXT at {at:#x} has no NUL"))));
    }
    identity_violations(&descriptors[0], &here, out);
    styl_violations(&descriptors[1], &here, out);
}

fn keys(desc: &Desc) -> Vec<&[u8]> {
    desc.items.iter().map(|(k, _)| k.as_slice()).collect()
}

fn item<'d>(desc: &'d Desc, key: &[u8]) -> Option<&'d Val> {
    desc.items
        .iter()
        .find(|(k, _)| k.as_slice() == key)
        .map(|(_, v)| v)
}

fn identity_violations(desc: &Desc, here: &dyn Fn(&str) -> String, out: &mut Vec<Violation>) {
    let texts = desc.items.iter().all(|(_, v)| matches!(v, Val::Text(_)));
    if desc.class != b"null" || keys(desc) != [b"Nm  ".as_slice(), b"Idnt"] || !texts {
        out.push((
            "identity",
            here("the identity is not null {Nm  , Idnt} TEXT"),
        ));
    }
    if let Some(Val::Text(id)) = item(desc, b"Idnt") {
        if !is_uuid(id) {
            out.push((
                "idnt-uuid",
                here(&format!("Idnt {id:?} is not 8-4-4-4-12 hex")),
            ));
        }
    }
}

/// Whether `text` is a lowercase `8-4-4-4-12` hex UUID.
pub fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups.iter().all(|g| {
            g.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

fn styl_violations(desc: &Desc, here: &dyn Fn(&str) -> String, out: &mut Vec<Violation>) {
    let k = keys(desc);
    let shape = k == [b"documentMode".as_slice(), b"Lefx"]
        || k == [b"documentMode".as_slice(), b"Lefx", b"blendOptions"];
    if desc.class != b"Styl" || !shape {
        out.push((
            "styl",
            here("the style is not Styl {documentMode, Lefx[, blendOptions]}"),
        ));
    }
    match item(desc, b"documentMode") {
        Some(Val::Object(d)) if d.class == b"documentMode" && d.items.is_empty() => {}
        _ => out.push((
            "document-mode",
            here("documentMode is not an empty documentMode object"),
        )),
    }
    if let Some(lefx) = item(desc, b"Lefx") {
        lefx_violations(lefx, here, out);
    }
    if let Some(options) = item(desc, b"blendOptions") {
        blend_options_violations(options, here, out);
    }
}

fn lefx_violations(lefx: &Val, here: &dyn Fn(&str) -> String, out: &mut Vec<Violation>) {
    let Val::Object(d) = lefx else {
        out.push(("lefx", here("Lefx is not an object")));
        return;
    };
    let scale_first = matches!(
        d.items.first(),
        Some((k, Val::UnitFloat(unit, _))) if k == b"Scl " && unit == b"#Prc"
    );
    if d.class != b"Lefx" || !scale_first {
        out.push(("lefx", here("Lefx does not start with Scl  UntF #Prc")));
    }
    if item(d, b"masterFXSwitch").is_some() {
        out.push(("lefx-master-switch", here("Lefx holds masterFXSwitch")));
    }
    if item(d, b"numModifyingFX").is_some() {
        out.push(("lefx-num-modifying", here("Lefx holds numModifyingFX")));
    }
}

/// Whether a `knockout` enum id is in S9's form: an id with a four-char code
/// as that charID (`None`, the stringID `none`), `shallow` and `deep` as
/// stringIDs with their length (a charID `deep` is not one).
fn knockout_id(id: &RawId) -> bool {
    matches!(
        (id.0, id.1.as_slice()),
        (0, b"None") | (7, b"shallow") | (4, b"deep")
    )
}

fn blend_options_violations(
    options: &Val,
    here: &dyn Fn(&str) -> String,
    out: &mut Vec<Violation>,
) {
    let Val::Object(d) = options else {
        out.push(("blend-options-empty", here("blendOptions is not an object")));
        return;
    };
    if d.items.is_empty() {
        out.push(("blend-options-empty", here("blendOptions holds no item")));
    }
    let mut order = BLEND_OPTIONS_ORDER.iter();
    let in_order = keys(d)
        .into_iter()
        .all(|key| order.by_ref().any(|k| *k == key));
    let names: Vec<String> = keys(d)
        .iter()
        .map(|k| String::from_utf8_lossy(k).into_owned())
        .collect();
    let full = keys(d) == BLEND_OPTIONS_ORDER
        && d.items.iter().all(|(key, value)| match key.as_slice() {
            b"Opct" | b"fillOpacity" => matches!(value, Val::UnitFloat(u, _) if u == b"#Prc"),
            b"Md  " => matches!(value, Val::Enum(..)),
            b"knockout" => matches!(value, Val::Enum(t, id)
                if (t.0, t.1.as_slice()) == (8, b"knockout") && knockout_id(id)),
            b"blendInterior" => matches!(value, Val::Other(k) if k == b"bool"),
            b"Blnd" => matches!(value, Val::List(_)),
            _ => false,
        });
    if !full {
        out.push((
            "blend-options-full",
            here(&format!(
                "blendOptions keys {names:?} are not Photoshop's six, in order and of its kinds"
            )),
        ));
    }
    if !in_order {
        out.push((
            "blend-options-order",
            here(&format!(
                "blendOptions keys {names:?} are out of Photoshop's order"
            )),
        ));
    }
}
