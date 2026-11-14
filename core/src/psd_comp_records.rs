//! Photoshop's LAYER COMPS in a PSD, as bytes (KERNEL): image resource 1065
//! (the comp list) and each layer record's `shmd` block holding a `cmls`
//! item (that layer's settings per comp), both directions, over
//! `psd_descriptor`. `psd_comps` is the adapter that binds them to a
//! document's layers.
//!
//! Evidence: `tests/fixtures/psd_photoshop/layer_comps/source.psd` and the
//! `authored_exchange` family (comments, moves, opacity, fill, mode and styles in
//! RGB, Gray and Lab, and held-out documents), authored by Photoshop 26.10.0
//! (`provenance.json` beside each). The same take's CMYK files, at 8 and 16
//! bits, are in its receipts and not in the tree.
//!
//! * Resource 1065 is `u32 16` then a descriptor of class `CompList` whose
//!   `list` is a `VlLs` of `Comp` objects: `Nm  ` (TEXT), an optional
//!   `comment` (TEXT, a string id), `compID` (long) and `capturedInfo`
//!   (long: bit 0 visibility, bit 1 position, bit 2 appearance). Photoshop
//!   writes `list` and `Comp` as STRING ids of length 4, so the writer names
//!   them (`psd_descriptor::write_with_string_ids`) and reproduces the
//!   authored bytes.
//! * `shmd` is `u32 count` then count × (`8BIM`, key, copy byte, three pad
//!   bytes, `u32 length`, data). Its `cmls` item is `u32 16` then a `null`
//!   descriptor of `origFXRefPoint` (`Hrzn`/`Vrtc` doubles, equal to compID
//!   0's offset), `LyrI` (long, the record's `lyid`) and `layerSettings`: a
//!   `VlLs` of objects, one per comp plus compID 0 (the last document state)
//!   last, each a `compList` (`VlLs` of compIDs) and the keys it sets:
//!   `enab` (bool), `FXRefPoint` (`Hrzn`/`Vrtc` doubles: the layer's offset
//!   from a reference point the record's entries share), `blendOptions`
//!   (`Opct` and `fillOpacity` percent units, `Md  ` a `BlnM` enum) and
//!   `Lefx` (the whole layer-effects descriptor, the items `lfx2` frames);
//!   a shape layer's entry also holds `vectorMask` { `Ofst` }, its shape's
//!   position.
//!   Every other `shmd` item is left alone on read; the writer writes the
//!   one `cmls` item, its data zero-padded to a multiple of four as
//!   Photoshop's are.
//! * **The chain.** Photoshop writes an entry's key only when the value
//!   changed since the PREVIOUS entry of the list (deleting a comp rewrites
//!   the next one), and `enab` whenever the entry sets anything else, so an
//!   absent key means the previous entry's value, and the first entry starts
//!   from a shown layer at offset (0, 0), 100 % opacity and fill, Normal and
//!   no style ([`chain`]). Applying each comp of every file of the family in
//!   Photoshop gave exactly the visibility, opacity, fill, mode and drop
//!   shadow this chain reads; reading an absent key as the default instead
//!   missed (take 2 of `psd-exchange-13`). A style removed between captures
//!   is not recorded: the chain keeps the earlier style, and so did
//!   Photoshop.
//! * The writer writes every key on every entry ([`write_shmd`]) but `Lefx`
//!   on the entry of a comp that records no appearance (`psd_comps`), so the
//!   chain and any other reading of absent keys agree on the rest; an
//!   appearance comp with no style writes a `Lefx` whose `masterFXSwitch` is
//!   off.
//!
//! Never a refusal: a body that does not decode (a non-finite opacity or
//! fill among them) is an `Err` reason the caller states as one loss
//! sentence and imports no setting of that record from.

use crate::blend::BlendMode;
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

/// One entry of a record's `layerSettings` as the writer writes it: every
/// key the entry sets.
pub(crate) struct Setting {
    /// The compIDs it applies to.
    pub comps: Vec<i32>,
    pub enabled: bool,
    /// `FXRefPoint`.
    pub offset: Option<(f64, f64)>,
    /// `blendOptions`: opacity and fill in percent, and the mode.
    pub blend: Option<(f64, f64, BlendMode)>,
    /// `Lefx`'s items (`psd_layer_effects::effect_items`), or `Some(empty)`
    /// for no style (written with `masterFXSwitch` off).
    pub effects: Option<Vec<(Vec<u8>, Item)>>,
}

/// The state after one `layerSettings` entry ([`chain`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChainState<S> {
    pub comps: Vec<i32>,
    pub visible: bool,
    pub offset: (f64, f64),
    /// Percent.
    pub opacity: f64,
    /// Percent.
    pub fill: f64,
    pub blend: BlendMode,
    /// The caller's reading of the style the chain holds.
    pub style: S,
    /// The entry, or one before it, set a key the chain does not read.
    pub unread: bool,
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
                    Item::Text(text) if key == b"comment" => Some(text.clone()),
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

/// Resource 1065's body for `comps`, in Photoshop's key order (module doc).
pub(crate) fn write_comps(comps: &[FileComp]) -> Vec<u8> {
    let list = comps
        .iter()
        .map(|comp| {
            let mut items = vec![(b"Nm  ".to_vec(), Item::Text(comp.name.clone()))];
            if !comp.comment.is_empty() {
                items.push((b"comment".to_vec(), Item::Text(comp.comment.clone())));
            }
            items.push((b"compID".to_vec(), Item::Long(comp.id)));
            items.push((b"capturedInfo".to_vec(), Item::Long(comp.captured)));
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

/// The `cmls` item's data of a record's `shmd` body, or `None` when the body
/// holds no `cmls` item.
fn cmls(shmd: &[u8]) -> Result<Option<&[u8]>, String> {
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
            return Ok(Some(data));
        }
        at = start + len;
    }
    Ok(None)
}

/// Whether a record's `shmd` body carries `cmls` settings that do not
/// decode: what the import report counts.
pub(crate) fn settings_unreadable(shmd: &[u8]) -> bool {
    chain(shmd, |_, _| ()).is_err()
}

/// A point's `Hrzn`/`Vrtc` doubles.
fn point(d: &Descriptor, key: &[u8]) -> Result<Option<(f64, f64)>, String> {
    let Some(object) = d.object(key)? else {
        return Ok(None);
    };
    let h = object.double(b"Hrzn")?.unwrap_or(0.0);
    let v = object.double(b"Vrtc")?.unwrap_or(0.0);
    Ok(Some((h, v)))
}

/// The `cmls` settings of a record's `shmd` body read as the CHAIN (module
/// doc): the state after each `layerSettings` entry, in file order, `None`
/// when the body holds no `cmls` item. `style(effects, fill)` reads the
/// chain's `Lefx` (`None` before any entry set one) at the chain's fill
/// percent for each entry, so the caller's codec decodes the style.
pub(crate) fn chain<S>(
    shmd: &[u8],
    mut style: impl FnMut(Option<&Descriptor>, f64) -> S,
) -> Result<Option<Vec<ChainState<S>>>, String> {
    let Some(data) = cmls(shmd)? else {
        return Ok(None);
    };
    let descriptor = psd_descriptor::read(data)?;
    let list = descriptor
        .list(b"layerSettings")?
        .ok_or("layer comp settings have no layerSettings")?;
    let mut visible = true;
    let mut offset = (0.0, 0.0);
    let (mut opacity, mut fill) = (100.0, 100.0);
    let mut blend = BlendMode::Normal;
    let mut effects: Option<&Descriptor> = None;
    let mut unread = false;
    let mut out = Vec::with_capacity(list.len());
    for item in list {
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
        if let Some(enabled) = setting.bool(b"enab")? {
            visible = enabled;
        }
        if let Some(at) = point(setting, b"FXRefPoint")? {
            offset = at;
        }
        if let Some(options) = setting.object(b"blendOptions")? {
            // A percentage that is no number is no setting: kept, it would
            // be an opacity the native comp table cannot hold.
            for (key, slot) in [(&b"Opct"[..], &mut opacity), (b"fillOpacity", &mut fill)] {
                if let Some(value) = options.unit(key)? {
                    if !value.is_finite() {
                        return Err("layer comp setting has a non-finite opacity".to_string());
                    }
                    *slot = value;
                }
            }
            if let Some(mode) = crate::psd_layer_effects_fx::blend_mode(options, b"Md  ")? {
                blend = mode;
            }
            unread |= options
                .items
                .iter()
                .any(|(key, _)| !matches!(key.as_slice(), b"Opct" | b"fillOpacity" | b"Md  "));
        }
        if let Some(lefx) = setting.object(b"Lefx")? {
            effects = Some(lefx);
        }
        unread |= setting.items.iter().any(|(key, _)| {
            !matches!(
                key.as_slice(),
                b"compList" | b"enab" | b"FXRefPoint" | b"blendOptions" | b"Lefx" | b"vectorMask"
            )
        });
        out.push(ChainState {
            comps,
            visible,
            offset,
            opacity,
            fill,
            blend,
            style: style(effects, fill),
            unread,
        });
    }
    Ok(Some(out))
}

/// A point object of `Hrzn`/`Vrtc` doubles.
fn point_item((h, v): (f64, f64)) -> Item {
    Item::Object(Descriptor {
        class: b"null".to_vec(),
        items: vec![
            (b"Hrzn".to_vec(), Item::Double(h)),
            (b"Vrtc".to_vec(), Item::Double(v)),
        ],
    })
}

/// A `shmd` body holding one `cmls` item: record `layer` (its `lyid`),
/// `origin` its `origFXRefPoint`, and `settings` in order (module doc).
pub(crate) fn write_shmd(layer: i32, origin: (f64, f64), settings: Vec<Setting>) -> Vec<u8> {
    let list = settings
        .into_iter()
        .map(|setting| {
            let mut items = vec![(b"enab".to_vec(), Item::Bool(setting.enabled))];
            if let Some((opacity, fill, mode)) = setting.blend {
                items.push((
                    b"blendOptions".to_vec(),
                    Item::Object(Descriptor {
                        class: b"null".to_vec(),
                        items: vec![
                            (b"Opct".to_vec(), Item::Unit(*b"#Prc", opacity)),
                            (b"fillOpacity".to_vec(), Item::Unit(*b"#Prc", fill)),
                            (
                                b"Md  ".to_vec(),
                                crate::psd_layer_effects_fx::comp_blend_item(mode),
                            ),
                        ],
                    }),
                ));
            }
            if let Some(effects) = setting.effects {
                let items_of = if effects.is_empty() {
                    vec![
                        (b"Scl ".to_vec(), Item::Unit(*b"#Prc", 100.0)),
                        (b"masterFXSwitch".to_vec(), Item::Bool(false)),
                    ]
                } else {
                    effects
                };
                items.push((
                    b"Lefx".to_vec(),
                    Item::Object(Descriptor {
                        class: b"null".to_vec(),
                        items: items_of,
                    }),
                ));
            }
            if let Some(offset) = setting.offset {
                items.push((b"FXRefPoint".to_vec(), point_item(offset)));
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
            (b"origFXRefPoint".to_vec(), point_item(origin)),
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

/// The chain's states of a record's `shmd` body as
/// `(compIDs, visible, offset, opacity %, fill %, blend code)`, styles not
/// read: the seam a black-box test checks the chain against Photoshop's own
/// step table with.
/// One state of [`chained_settings`]: compIDs, visible, offset, opacity %,
/// fill %, blend code.
pub type ChainedSetting = (Vec<i32>, bool, (f64, f64), f64, f64, i32);

#[doc(hidden)]
pub fn chained_settings(shmd: &[u8]) -> Result<Option<Vec<ChainedSetting>>, String> {
    Ok(chain(shmd, |_, _| ())?.map(|states| {
        states
            .into_iter()
            .map(|s| {
                (
                    s.comps,
                    s.visible,
                    s.offset,
                    s.opacity,
                    s.fill,
                    s.blend.to_c(),
                )
            })
            .collect()
    }))
}
