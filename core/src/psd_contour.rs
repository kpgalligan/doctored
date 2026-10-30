//! The `ShpC` contour of Photoshop's layer effects (`style_contour`), both
//! ways, and the contour-family keys around it
//! (`AntA`, `Inpr`, `useShape`, `antialiasGloss`). `psd_layer_effects_fx`
//! calls in here for every effect that has a contour; the mapping table is
//! `psd_layer_effects`' module doc.
//!
//! `ShpC` = {`Nm ` TEXT, optional `Crv ` VlLs of `CrPt` {`Hrzn` doub,
//! `Vrtc` doub, optional `Cnty` bool (false = a corner)}}.
//!
//! - **Writing.** Linear writes the exact item every earlier writer wrote
//!   (`Nm ` "Linear" plus the two points), so every PSD golden stays
//!   byte-identical; another preset writes Photoshop's own name form,
//!   `$$$/Contours/Defaults/<key>=<title>`, with no curve (Photoshop 26.10.0
//!   resolves a preset by its name, `contour_take.py`); a custom curve
//!   writes `Nm ` "Custom" plus `Crv `, `Cnty` false on its corner points.
//!   `AntA` is `anti_aliased || contour is Linear`, which keeps the `true`
//!   every earlier writer wrote.
//! - **Reading.** The name is matched after `$$$/Contours/Defaults/…=`, or
//!   as plain text, against the ten preset titles (behind the prefix the
//!   key before `=` matches too, for a localized title), so a preset's `Crv` is
//!   ignored; any other name reads its `Crv` as a custom curve; a name with
//!   no curve declines (`contour <name> has no curve`), as does a curve of
//!   fewer than 2 or more than 16 points. `AntA` / `antialiasGloss` are
//!   carried only when their contour is not Linear. Each read contour is a
//!   style-JSON contour object; `LayerStyle::from_value` clamps, quantizes
//!   and sorts it.

use serde_json::{json, Map, Value};

use crate::psd_descriptor::{Descriptor, Item};
use crate::style::{Contour, ContourPreset};

/// Each preset's Photoshop name: the `$$$/Contours/Defaults/` key and the
/// title after `=` (the take's readbacks).
const PSD_NAMES: [(ContourPreset, &str, &str); 10] = [
    (ContourPreset::Linear, "Linear", "Linear"),
    (ContourPreset::Cone, "Cone", "Cone"),
    (
        ContourPreset::ConeInverted,
        "ConeInverted",
        "Cone - Inverted",
    ),
    (ContourPreset::Gaussian, "Gaussian", "Gaussian"),
    (ContourPreset::HalfRound, "HalfRound", "Half Round"),
    (ContourPreset::Ring, "Ring", "Ring"),
    (ContourPreset::RingDouble, "DoubleRing", "Ring - Double"),
    (
        ContourPreset::RollingSlopeDescending,
        "RollingSlopeDescending",
        "Rolling Slope - Descending",
    ),
    (ContourPreset::RoundedSteps, "RoundedSteps", "Rounded Steps"),
    (ContourPreset::Sawtooth1, "Sawtooth1", "Sawtooth 1"),
];

const PREFIX: &str = "$$$/Contours/Defaults/";

fn id(name: &[u8]) -> Vec<u8> {
    name.to_vec()
}

fn point(x: f64, y: f64, corner: bool) -> Item {
    let mut items = vec![
        (id(b"Hrzn"), Item::Double(x)),
        (id(b"Vrtc"), Item::Double(y)),
    ];
    if corner {
        items.push((id(b"Cnty"), Item::Bool(false)));
    }
    Item::Object(Descriptor {
        class: id(b"CrPt"),
        items,
    })
}

fn shpc(name: String, curve: Option<Vec<Item>>) -> Item {
    let mut items = vec![(id(b"Nm  "), Item::Text(name))];
    if let Some(curve) = curve {
        items.push((id(b"Crv "), Item::List(curve)));
    }
    Item::Object(Descriptor {
        class: id(b"ShpC"),
        items,
    })
}

/// The `ShpC` item of `contour` (module doc, Writing).
pub(crate) fn shpc_item(contour: &Contour) -> Item {
    match contour {
        Contour::Preset(ContourPreset::Linear) => shpc(
            "Linear".into(),
            Some(vec![point(0.0, 0.0, false), point(255.0, 255.0, false)]),
        ),
        Contour::Preset(p) => {
            let (_, key, title) = PSD_NAMES
                .iter()
                .find(|(preset, _, _)| preset == p)
                .copied()
                .unwrap_or(PSD_NAMES[0]);
            shpc(format!("{PREFIX}{key}={title}"), None)
        }
        Contour::Custom(curve) => shpc(
            "Custom".into(),
            Some(
                curve
                    .points()
                    .iter()
                    .map(|p| point(f64::from(p.x), f64::from(p.y), p.corner))
                    .collect(),
            ),
        ),
    }
}

/// `AntA` (or `antialiasGloss`) for `contour` (module doc, Writing).
pub(crate) fn anti_alias_item(anti_aliased: bool, contour: &Contour) -> Item {
    Item::Bool(anti_aliased || contour.is_linear())
}

/// The preset a contour name names, after the `$$$` prefix or as plain
/// text; `None` for any other name. Behind the prefix the KEY before `=`
/// decides too, so a localized title still finds its preset.
fn preset_named(name: &str) -> Option<ContourPreset> {
    let (named_key, title) = match name.strip_prefix(PREFIX) {
        Some(rest) => rest.split_once('=').map_or((rest, rest), |(k, t)| (k, t)),
        None => (name, name),
    };
    PSD_NAMES
        .iter()
        .find(|(_, key, t)| *t == title || *key == title || *key == named_key)
        .map(|(preset, _, _)| *preset)
}

/// The JSON id of a preset (`style_names::CONTOUR_PRESETS`).
fn preset_id(preset: ContourPreset) -> &'static str {
    crate::style_names::enum_name(&crate::style_names::CONTOUR_PRESETS, preset)
}

/// `d[key]` (a `ShpC`) as a style-JSON contour object (module doc,
/// Reading); `None` when absent. `Err` is the decline reason.
fn read_shpc(d: &Descriptor, key: &[u8]) -> Result<Option<Value>, String> {
    let Some(shpc) = d.object(key)? else {
        return Ok(None);
    };
    let name = shpc
        .items
        .iter()
        .find_map(|(k, item)| match item {
            Item::Text(text) if k == b"Nm  " => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default();
    if let Some(preset) = preset_named(&name) {
        return Ok(Some(json!({ "preset": preset_id(preset) })));
    }
    let Some(curve) = shpc.list(b"Crv ")? else {
        return Err(format!("contour {name} has no curve"));
    };
    let mut points = Vec::with_capacity(curve.len());
    for item in curve {
        let Item::Object(p) = item else {
            return Err(format!("contour {name} has a malformed curve"));
        };
        let x = p.double(b"Hrzn")?.filter(|v| v.is_finite());
        let y = p.double(b"Vrtc")?.filter(|v| v.is_finite());
        let (Some(x), Some(y)) = (x, y) else {
            return Err(format!("contour {name} has a malformed curve"));
        };
        let mut o = Map::new();
        o.insert("x".into(), json!(x));
        o.insert("y".into(), json!(y));
        if p.bool(b"Cnty")? == Some(false) {
            o.insert("corner".into(), Value::Bool(true));
        }
        points.push((x, Value::Object(o)));
    }
    let mut xs: Vec<f64> = points.iter().map(|(x, _)| x.clamp(0.0, 255.0)).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    if !(2..=crate::style::MAX_CONTOUR_POINTS).contains(&points.len()) || xs.len() < 2 {
        return Err(format!(
            "contour {name} does not have between 2 and 16 points"
        ));
    }
    Ok(Some(
        json!({ "points": points.into_iter().map(|(_, p)| p).collect::<Vec<_>>() }),
    ))
}

fn is_linear(contour: Option<&Value>) -> bool {
    contour.is_none_or(|c| c.get("preset").and_then(Value::as_str) == Some("linear"))
}

/// Reads `d[shape]` into `o[key]` and, when that contour is not Linear,
/// `d[aa]` into `o[aa_key]`.
fn read_pair(
    d: &Descriptor,
    o: &mut Map<String, Value>,
    shape: &[u8],
    key: &str,
    aa: &[u8],
    aa_key: &str,
) -> Result<(), String> {
    let contour = read_shpc(d, shape)?;
    if !is_linear(contour.as_ref()) {
        if let Some(flag) = d.bool(aa)? {
            o.insert(aa_key.into(), Value::Bool(flag));
        }
    }
    if let Some(contour) = contour {
        o.insert(key.into(), contour);
    }
    Ok(())
}

/// One `#Prc` key as a 0..1 fraction into `o[key]`.
fn read_range(d: &Descriptor, o: &mut Map<String, Value>, key: &str) -> Result<(), String> {
    if let Some(range) = crate::psd_layer_effects_fx::fraction(d, b"Inpr")? {
        o.insert(key.into(), json!(range));
    }
    Ok(())
}

/// The contour-family keys of one effect class into its style-JSON object
/// (module doc, and `psd_layer_effects`' table). `Err` is the decline reason.
pub(crate) fn read_keys(
    class: &[u8; 4],
    d: &Descriptor,
    o: &mut Map<String, Value>,
) -> Result<(), String> {
    match class {
        b"DrSh" | b"IrSh" => read_pair(d, o, b"TrnS", "contour", b"AntA", "anti_aliased"),
        b"OrGl" | b"IrGl" => {
            read_pair(d, o, b"TrnS", "contour", b"AntA", "anti_aliased")?;
            // Carried as written, on enabled and disabled slots alike.
            read_range(d, o, "range")
        }
        b"ChFX" => read_pair(d, o, b"MpgS", "contour", b"AntA", "anti_aliased"),
        b"ebbl" => {
            read_pair(
                d,
                o,
                b"TrnS",
                "gloss_contour",
                b"antialiasGloss",
                "gloss_anti_aliased",
            )?;
            let enabled = d.bool(b"useShape")? == Some(true);
            if enabled {
                o.insert("contour_enabled".into(), Value::Bool(true));
                read_pair(d, o, b"MpgS", "contour", b"AntA", "contour_anti_aliased")?;
                read_range(d, o, "contour_range")?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
