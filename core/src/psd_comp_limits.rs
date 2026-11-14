//! What a PSD's layer comps and the model's cannot hold of each other, in
//! numbers: the bookkeeping `psd_comps` (whose module doc states the rules)
//! reads on both sides of the exchange.
//!
//! * **The three keys.** A style's Blend If, knockout and Blend Interior
//!   Effects as Group are a record's own blocks; a comp's setting holds none
//!   of them ([`Keys`]). The import puts the layer's own on every entry
//!   ([`with_layer_keys`]); the export counts the comps whose recorded three
//!   are not their layer's ([`narrowed`]).
//! * **The export's narrowing.** [`narrowed`] counts, for the preflight's
//!   sentences, the comps that record a style the effects codec refuses, an
//!   appearance on a record whose pixels hold the layer's (a rasterized
//!   style, an adjustment layer's stamp), and differing keys; [`recorded_styles`] is the styles whose patterns the writers add
//!   to the file's pattern table.
//! * **The import's weighing.** [`weights`] is what each comp of a file
//!   weighs against the `LCMP` payload cap, from the records alone, so the
//!   import report and the open decide alike.

use crate::doc_comps::LayerComp;
use crate::psd_comps::Owner;
use crate::psd_group_records::GroupRecords;
use crate::style::{BlendIf, Knockout, LayerStyle};

/// A style's Blend If, knockout and interior flag: what a record's own
/// blocks hold and a comp's setting does not (module doc).
pub(crate) type Keys = (Option<BlendIf>, Knockout, bool);

pub(crate) fn keys_of(style: Option<&LayerStyle>) -> Keys {
    style.map_or((None, Knockout::None, false), |style| {
        (
            style.blend_if.filter(|ranges| !ranges.is_identity()),
            style.knockout,
            style.blend_interior_as_group,
        )
    })
}

pub(crate) fn parsed(style: Option<&str>) -> Option<LayerStyle> {
    style.and_then(|json| LayerStyle::from_json(json).ok())
}

/// What one imported entry is weighed as: the `LCMP` payload's braces, its
/// `id`, `name` and `visible` keys with their values (40 bytes with an id of
/// six digits) and a layer name of 24 bytes.
pub(crate) const ENTRY_BYTES: u64 = 64;
/// What an entry's appearance adds beside its style: the `opacity`, `blend`
/// and `style` keys and the two numbers (under 50 bytes), with the same
/// again for a longer name.
pub(crate) const APPEARANCE_BYTES: u64 = 64;

/// Per compID, how many records' settings name it and the bytes of style
/// those settings record, as the `LCMP` payload holds a style (a JSON
/// string, so each quote and backslash is two bytes): what
/// `psd_comps::file_comps` weighs a comp by, from the records alone. A style that does not decode
/// is weighed as the record's own, which the entry then holds, and a record
/// whose own style carries a Blend If or knockout adds an allowance for the
/// copy each entry takes.
pub(crate) fn weights(records: &GroupRecords<'_>) -> std::collections::HashMap<i32, (u64, u64)> {
    use crate::psd_layer_effects::StyleBody;
    let in_payload = |json: &str| {
        (json.len() + json.bytes().filter(|b| matches!(b, b'"' | b'\\')).count()) as u64
    };
    let mut out = std::collections::HashMap::new();
    let chains = records
        .layer_comps
        .iter()
        .zip(&records.layer_styles)
        .chain(records.group_comps.iter().zip(&records.group_styles));
    for (chain, own) in chains {
        let Some(Ok(chain)) = chain else { continue };
        let own = match own {
            StyleBody::Carried(carried) => Some(&carried.style),
            _ => None,
        };
        let keys = if keys_of(own) == keys_of(None) {
            0
        } else {
            1024
        };
        let mut own_bytes = None;
        for state in chain {
            let style = match &state.style {
                Ok(Some(json)) => in_payload(json),
                Ok(None) => 0,
                Err(_) => *own_bytes
                    .get_or_insert_with(|| own.map_or(0, |style| in_payload(&style.to_json()))),
            };
            for id in &state.comps {
                let weight: &mut (u64, u64) = out.entry(*id).or_default();
                weight.0 += 1;
                weight.1 += style + keys;
            }
        }
    }
    out
}

/// A comp's recorded style under its layer's own `keys` (module doc).
pub(crate) fn with_layer_keys(recorded: Option<&str>, keys: &Keys) -> Option<String> {
    if *keys == keys_of(None) {
        return recorded.map(str::to_string);
    }
    let mut style = parsed(recorded).unwrap_or_default();
    (
        style.blend_if,
        style.knockout,
        style.blend_interior_as_group,
    ) = *keys;
    let style = style.normalized();
    (!style.is_identity()).then(|| style.to_json())
}

/// What the layered writers do not write of the comps' recorded styles
/// (module doc), as how many comps each holds for: a style the effects
/// codec refuses, an appearance on a record whose pixels hold the layer's
/// (`Owner::rasterized`), and a Blend If, knockout or interior flag that is
/// not its layer's own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Narrowed {
    pub refused: usize,
    pub rasterized: usize,
    pub keys: usize,
}

/// [`Narrowed`] for `comps` over the writer's `owners`
/// (`psd_comps::stamp_records`');
/// `refuses` is whether the writer's effects codec refuses a style. An entry
/// no record owns is written nowhere and counts for nothing.
pub(crate) fn narrowed(
    comps: &[LayerComp],
    owners: &[Option<Owner>],
    refuses: &dyn Fn(&LayerStyle) -> bool,
) -> Narrowed {
    let mut out = Narrowed::default();
    if comps.is_empty() {
        return out;
    }
    let owners: std::collections::HashMap<u64, (bool, Keys)> = owners
        .iter()
        .flatten()
        .map(|owner| {
            let keys = keys_of(parsed(owner.style.as_deref()).as_ref());
            (owner.id, (owner.rasterized(), keys))
        })
        .collect();
    for comp in comps {
        let (mut refused, mut rasterized, mut keys) = (false, false, false);
        for entry in &comp.entries {
            let (Some(appearance), Some((baked, own))) =
                (&entry.appearance, owners.get(&entry.layer_id))
            else {
                continue;
            };
            if *baked {
                rasterized = true;
                continue;
            }
            let style = parsed(appearance.style.as_deref());
            refused |= style.as_ref().is_some_and(refuses);
            keys |= keys_of(style.as_ref()) != *own;
        }
        out.refused += usize::from(refused);
        out.rasterized += usize::from(rasterized);
        out.keys += usize::from(keys);
    }
    out
}

/// The comps' recorded styles that place a pattern and that a setting of a
/// record that is not rasterized writes, each once: the layered writers add
/// their patterns to the file's pattern table after the layers' own
/// (`psd_patterns`), so a comp's `Lefx` never names a pattern the file lacks.
pub(crate) fn recorded_styles(comps: &[LayerComp], owners: &[Option<Owner>]) -> Vec<LayerStyle> {
    if comps.is_empty() {
        return Vec::new();
    }
    let written: std::collections::HashSet<u64> = owners
        .iter()
        .flatten()
        .filter(|owner| !owner.rasterized())
        .map(|owner| owner.id)
        .collect();
    let mut seen = std::collections::HashSet::new();
    comps
        .iter()
        .flat_map(|comp| &comp.entries)
        .filter(|entry| written.contains(&entry.layer_id))
        .filter_map(|entry| entry.appearance.as_ref()?.style.as_deref())
        // Every placed pattern's JSON holds this key (`style_json_write`).
        .filter(|json| json.contains("\"pattern\"") && seen.insert(*json))
        .filter_map(|json| LayerStyle::from_json(json).ok())
        .collect()
}
