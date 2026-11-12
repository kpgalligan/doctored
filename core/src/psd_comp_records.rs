//! Photoshop's LAYER COMPS in a PSD, as bytes (KERNEL): image resource 1065
//! (the comp list) and each layer record's `shmd` block holding a `cmls`
//! item (that layer's settings per comp), both directions, over
//! `psd_descriptor`. `psd_comps` is the adapter that binds them to a
//! document's layers.
//!
//! Evidence: `tests/fixtures/psd_photoshop/layer_comps/source.psd`, authored by
//! Photoshop 26.10.0 (`provenance.json` beside it).
//!
//! * Resource 1065 is `u32 16` then a descriptor of class `CompList` whose
//!   `list` is a `VlLs` of `Comp` objects: `Nm  ` (TEXT), `compID` (long) and
//!   `capturedInfo` (long: bit 0 visibility, bit 1 position, bit 2
//!   appearance). Photoshop writes `list` and `Comp` as STRING ids of length
//!   4, so the writer names them (`psd_descriptor::write_with_string_ids`)
//!   and reproduces the authored bytes. A comp's comment has no authored
//!   sample: the reader accepts `comment` or `Msge` (TEXT), the writer
//!   writes `comment`.
//! * `shmd` is `u32 count` then count × (`8BIM`, key, copy byte, three pad
//!   bytes, `u32 length`, data). Its `cmls` item is `u32 16` then a `null`
//!   descriptor of `origFXRefPoint` (`Hrzn`/`Vrtc` doubles), `LyrI` (long, the
//!   record's `lyid`) and `layerSettings`: a `VlLs` of objects, each a
//!   `compList` (`VlLs` of compIDs, 0 being the last document state) with an
//!   optional `enab` (bool; absent means shown) and other recorded settings
//!   (`blendOptions`, a moved layer's offset, a style), which are counted but
//!   not decoded. Every other `shmd` item is left alone on read; the writer
//!   writes the one `cmls` item, its data zero-padded to a multiple of four
//!   as Photoshop's are.
//!
//! Never a refusal: a body that does not decode is an `Err` reason the
//! caller states as one loss sentence and imports no comps from.

use crate::psd_descriptor::{self, Descriptor, Item};

/// `capturedInfo`'s bits.
pub(crate) const CAPTURES_VISIBILITY: i32 = 1;
pub(crate) const CAPTURES_POSITION: i32 = 2;
pub(crate) const CAPTURES_APPEARANCE: i32 = 4;

/// One comp of resource 1065.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileComp {
    pub name: String,
    pub comment: String,
    pub id: i32,
    pub captured: i32,
}

/// One entry of a record's `layerSettings`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Setting {
    /// The compIDs it applies to.
    pub comps: Vec<i32>,
    /// `enab`, when written.
    pub enabled: Option<bool>,
}

/// Resource 1065's comps, in file order.
pub(crate) fn read_comps(body: &[u8]) -> Result<Vec<FileComp>, String> {
    let descriptor = psd_descriptor::read(body)?;
    let list = descriptor
        .list(b"list")?
        .ok_or("layer comp list has no list")?;
    list.iter()
        .map(|item| {
            let Item::Object(comp) = item else {
                return Err("layer comp list holds a non-object".to_string());
            };
            let name = match comp.items.iter().find(|(key, _)| key == b"Nm  ") {
                Some((_, Item::Text(name))) => name.clone(),
                _ => return Err("layer comp has no name".to_string()),
            };
            let comment = comp
                .items
                .iter()
                .find_map(|(key, item)| match item {
                    Item::Text(text) if key == b"comment" || key == b"Msge" => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            Ok(FileComp {
                name,
                comment,
                id: comp.long(b"compID")?.ok_or("layer comp has no compID")?,
                captured: comp
                    .long(b"capturedInfo")?
                    .ok_or("layer comp has no capturedInfo")?,
            })
        })
        .collect()
}

/// Resource 1065's body for `comps` (module doc).
pub(crate) fn write_comps(comps: &[FileComp]) -> Vec<u8> {
    let list = comps
        .iter()
        .map(|comp| {
            let mut items = vec![
                (b"Nm  ".to_vec(), Item::Text(comp.name.clone())),
                (b"compID".to_vec(), Item::Long(comp.id)),
                (b"capturedInfo".to_vec(), Item::Long(comp.captured)),
            ];
            if !comp.comment.is_empty() {
                items.push((b"comment".to_vec(), Item::Text(comp.comment.clone())));
            }
            Item::Object(Descriptor {
                class: b"Comp".to_vec(),
                items,
            })
        })
        .collect();
    psd_descriptor::write_with_string_ids(
        &Descriptor {
            class: b"CompList".to_vec(),
            items: vec![(b"list".to_vec(), Item::List(list))],
        },
        &[*b"list", *b"Comp"],
    )
}

/// The `cmls` item's settings of a record's `shmd` body, or `None` when the
/// body holds no `cmls` item.
pub(crate) fn read_settings(shmd: &[u8]) -> Result<Option<Vec<Setting>>, String> {
    const SHORT: &str = "layer metadata is shorter than its layout";
    let word = |at: usize| -> Result<usize, String> {
        let bytes = shmd.get(at..at + 4).ok_or(SHORT)?;
        Ok(u32::from_be_bytes(bytes.try_into().map_err(|_| SHORT)?) as usize)
    };
    let count = word(0)?;
    let mut at = 4usize;
    for _ in 0..count.min(4096) {
        if shmd.get(at..at + 4) != Some(b"8BIM") {
            return Err("layer metadata item has no 8BIM signature".into());
        }
        let key = shmd.get(at + 4..at + 8).ok_or(SHORT)?;
        let len = word(at + 12)?;
        let start = at + 16;
        let data = shmd
            .get(start..start.checked_add(len).ok_or(SHORT)?)
            .ok_or(SHORT)?;
        if key == b"cmls" {
            return settings(data).map(Some);
        }
        at = start + len;
    }
    Ok(None)
}

fn settings(data: &[u8]) -> Result<Vec<Setting>, String> {
    let descriptor = psd_descriptor::read(data)?;
    let list = descriptor
        .list(b"layerSettings")?
        .ok_or("layer comp settings have no layerSettings")?;
    list.iter()
        .map(|item| {
            let Item::Object(setting) = item else {
                return Err("layer comp setting is not an object".to_string());
            };
            let comps = setting
                .list(b"compList")?
                .ok_or("layer comp setting has no compList")?
                .iter()
                .map(|id| match id {
                    Item::Long(id) => Ok(*id),
                    _ => Err("layer comp setting names a non-integer comp".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Setting {
                comps,
                enabled: setting.bool(b"enab")?,
            })
        })
        .collect()
}

/// A `shmd` body holding one `cmls` item: record `layer` (its `lyid`) and
/// `settings` (module doc).
pub(crate) fn write_shmd(layer: i32, settings: &[Setting]) -> Vec<u8> {
    let reference = Descriptor {
        class: b"null".to_vec(),
        items: vec![
            (b"Hrzn".to_vec(), Item::Double(0.0)),
            (b"Vrtc".to_vec(), Item::Double(0.0)),
        ],
    };
    let list = settings
        .iter()
        .map(|setting| {
            let mut items = Vec::new();
            if let Some(enabled) = setting.enabled {
                items.push((b"enab".to_vec(), Item::Bool(enabled)));
            }
            items.push((
                b"compList".to_vec(),
                Item::List(setting.comps.iter().map(|id| Item::Long(*id)).collect()),
            ));
            Item::Object(Descriptor {
                class: b"null".to_vec(),
                items,
            })
        })
        .collect();
    let mut data = psd_descriptor::write(&Descriptor {
        class: b"null".to_vec(),
        items: vec![
            (b"origFXRefPoint".to_vec(), Item::Object(reference)),
            (b"LyrI".to_vec(), Item::Long(layer)),
            (b"layerSettings".to_vec(), Item::List(list)),
        ],
    });
    data.resize(data.len().next_multiple_of(4), 0);
    let mut out = 1u32.to_be_bytes().to_vec();
    out.extend_from_slice(b"8BIMcmls");
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&data);
    out
}

/// `read_comps` then `write_comps`: the codec-identity seam a black-box test
/// checks Photoshop's own 1065 bytes against.
#[doc(hidden)]
pub fn rewritten_comps(body: &[u8]) -> Result<Vec<u8>, String> {
    read_comps(body).map(|comps| write_comps(&comps))
}
