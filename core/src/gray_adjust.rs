//! The Grayscale document's adjustments (#547 phase 2b): which ops run on a
//! Gray document, which refuse for good, and the one rewrite an admitted
//! description needs (`designs/grayscale-mode.md`).
//!
//! # Why no one-channel kernel
//!
//! A Gray document's storage is R = G = B (`gray_document`'s module doc), so
//! an adjustment whose per-channel arithmetic is the same for R, G and B
//! keeps it: the existing `PrecisionAdjustment` runs unchanged, and its
//! result on the Gray document IS its result on the RGB twin. The table
//! below admits exactly those ops (`Runs`), each proved against the twin in
//! `core/tests/gray_adjust.rs`. Two parameter shapes would break the
//! equality and refuse with their own sentence instead: Levels and Curves
//! with different `r`, `g` and `b` (the channel rule; a Gray document has
//! one channel), and a Gradient Map's coloured stops, which are greyed
//! rather than refused — each stop's `#rrggbb` becomes the gray of its
//! luminance by Mode ▸ Grayscale's rule (`GrayPolicy::luminance_code`; a
//! map's stop numbers are the document's own, `adjust_map`'s doc).
//!
//! # The table
//!
//! The table is the mode's own: `Unavailable` ops are unavailable in a
//! Grayscale document and refuse permanently (`unavailable_sentence`);
//! `RgbOnly` ops are Doctored's own colour
//! adjustments, refused for good too; `Runs { layer: false }` run on pixels
//! only, never as an adjustment layer (Shadows/Highlights, Replace Color). An op
//! outside the table is one RGB does not know either, and refuses with RGB's
//! own error ("unsupported high-depth adjustment {op}"). A selected edit that is not an
//! adjustment, an Auto or Equalize is a filter, `gray_filter`'s gate (phase
//! 2c).

use std::borrow::Cow;

use serde_json::Value;

use crate::gray_document::GrayPolicy;
use crate::icc::IccProfile;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::selected_adjustment::SelectedEdit;

/// What a Gray document does with one adjustment op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrayAvailability {
    /// Runs on the gray values; `layer` = also as an adjustment layer.
    Runs { layer: bool },
    /// Disabled in a Grayscale document: refused for good.
    Unavailable,
    /// A Doctored colour adjustment, offered on RGB documents only.
    RgbOnly,
}

use GrayAvailability::{RgbOnly, Runs, Unavailable};

/// (op, display name, availability), Doctored's own colour adjustments
/// last. The names are the host's `AdjustmentLayerOp.displayName`.
pub(crate) const GRAY_ADJUSTMENTS: &[(&str, &str, GrayAvailability)] = &[
    (
        "bcs",
        "Brightness/Contrast/Saturation",
        Runs { layer: true },
    ),
    ("levels", "Levels", Runs { layer: true }),
    ("curves", "Curves", Runs { layer: true }),
    ("exposure", "Exposure", Runs { layer: true }),
    ("vibrance", "Vibrance", Unavailable),
    ("hue_saturation", "Hue/Saturation", Unavailable),
    ("color_balance", "Color Balance", Unavailable),
    ("black_and_white", "Black & White", Unavailable),
    ("photo_filter", "Photo Filter", Unavailable),
    ("channel_mixer", "Channel Mixer", Unavailable),
    ("color_lookup", "Color Lookup", Unavailable),
    ("invert", "Invert", Runs { layer: true }),
    ("posterize", "Posterize", Runs { layer: true }),
    ("threshold", "Threshold", Runs { layer: true }),
    ("gradient_map", "Gradient Map", Runs { layer: true }),
    ("selective_color", "Selective Color", Unavailable),
    (
        "shadows_highlights",
        "Shadows/Highlights",
        Runs { layer: false },
    ),
    ("replace_color", "Replace Color", Runs { layer: false }),
    ("hue_rotate", "Hue Rotate", RgbOnly),
    ("grayscale", "Grayscale", RgbOnly),
    ("sepia", "Sepia", RgbOnly),
    ("white_balance", "White Balance", RgbOnly),
];

/// The most stops a Gradient Map carries (`style_json::gradient`'s cap); the
/// stop rewrite never walks more.
const MAX_STOPS: usize = 32;

fn unavailable_sentence(name: &str) -> String {
    format!("{name} is unavailable in a Grayscale document.")
}

fn rgb_only_sentence(name: &str) -> String {
    format!(
        "{name} is unavailable in a Grayscale document: Doctored offers it on RGB documents only."
    )
}

fn layer_sentence(name: &str) -> String {
    format!(
        "A {name} adjustment layer is unavailable in a Grayscale document: {name} applies to pixels only."
    )
}

fn channels_sentence(name: &str) -> String {
    format!(
        "{name} in a Grayscale document has one channel, Gray: set the composite, or r, g and b alike."
    )
}

/// True when `keys` of `params` are all absent or all present and equal.
fn alike(params: &Value, keys: [&str; 3]) -> bool {
    let [a, b, c] = keys.map(|key| params.get(key));
    match (a, b, c) {
        (None, None, None) => true,
        (Some(a), Some(b), Some(c)) => a == b && b == c,
        _ => false,
    }
}

/// The table's and the channel rule's decision for `op` with `params`;
/// `layer` = as an adjustment layer.
fn rule(op: &str, params: &Value, layer: bool) -> Result<(), String> {
    let Some(&(_, name, availability)) = GRAY_ADJUSTMENTS.iter().find(|(o, _, _)| *o == op) else {
        // RGB's own error for an op it does not know (`adjust_precision`).
        return Err(format!("unsupported high-depth adjustment {op}"));
    };
    match availability {
        Unavailable => return Err(unavailable_sentence(name)),
        RgbOnly => return Err(rgb_only_sentence(name)),
        Runs { layer: false } if layer => return Err(layer_sentence(name)),
        Runs { .. } => {}
    }
    let channels = match op {
        "levels" => alike(params, ["r", "g", "b"]),
        "curves" => alike(params, ["r", "g", "b"]) && alike(params, ["r_map", "g_map", "b_map"]),
        _ => true,
    };
    if !channels {
        return Err(channels_sentence(name));
    }
    Ok(())
}

/// The op and params of a description, or `None` for one the kernel's own
/// parser must refuse (not JSON, not an object, no op).
fn parts(description: &str) -> Option<(String, Value)> {
    let value: Value = serde_json::from_str(description).ok()?;
    let op = value.get("op")?.as_str()?.to_string();
    Some((op, value))
}

/// `#rrggbb`, a colour in `from`, as the `#gggggg` of its luminance under
/// `policy`, by Mode ▸ Grayscale's rule (`GrayPolicy::luminance_code`,
/// rounded to 8 bits); `None` for a colour that does not parse. The one
/// helper of [`grey_hex`] and [`converted_description`].
fn grey_in(from: &IccProfile, policy: &GrayPolicy, hex: &str) -> Option<String> {
    let rgb = crate::style_names::parse_color(hex)?;
    let unit = rgb.map(|c| f64::from(c) / 255.0);
    let code = policy.luminance_code(from, unit);
    let g = (f64::from(code) / 257.0).round() as u8;
    Some(crate::style_names::color_hex([g; 3]))
}

/// `#rrggbb` as the `#gggggg` of its luminance on a Gray document, by Mode ▸
/// Grayscale's rule (`GrayPolicy::luminance_code` through the document's
/// profile, rounded to 8 bits): a Gradient Map's stops and Clouds' colours.
/// `None` off Gray or for a colour that does not parse.
pub(crate) fn grey_hex(doc: &PrecisionDocument, hex: &str) -> Option<String> {
    grey_in(doc.profile(), doc.gray_policy()?, hex)
}

/// An admitted adjustment layer's description carried by Image ▸ Mode ▸
/// Grayscale (#547 phase 2e, `gray_convert`): a `gradient_map`'s stops become
/// the `#gggggg` of their luminance from `from`, the RGB document's profile,
/// under `policy` (the Mode rule, [`grey_in`]); every other description
/// unchanged. `Err` only when the rewritten description cannot be written.
pub(crate) fn converted_description(
    description: &str,
    from: &IccProfile,
    policy: &GrayPolicy,
) -> Result<String, String> {
    let Some((op, mut value)) = parts(description) else {
        return Ok(description.to_string());
    };
    if op == "gradient_map" && grey_stops(from, policy, &mut value) {
        return serde_json::to_string(&value)
            .map_err(|e| format!("cannot write the Grayscale gradient map: {e}"));
    }
    Ok(description.to_string())
}

/// Greys a Gradient Map's stops in place (module doc), each a colour in
/// `from`, under `policy`; true when a stop changed. A stop whose colour is
/// not `#rrggbb` is left for the kernel's parser to refuse.
fn grey_stops(from: &IccProfile, policy: &GrayPolicy, value: &mut Value) -> bool {
    let Some(stops) = value
        .pointer_mut("/params/gradient/stops")
        .and_then(Value::as_array_mut)
    else {
        return false;
    };
    let mut changed = false;
    for stop in stops.iter_mut().take(MAX_STOPS) {
        let Some(color) = stop.get("color").and_then(Value::as_str) else {
            continue;
        };
        let Some(grey) = grey_in(from, policy, color) else {
            continue;
        };
        if crate::style_names::parse_color(color) != crate::style_names::parse_color(&grey) {
            stop["color"] = Value::from(grey);
            changed = true;
        }
    }
    changed
}

/// Borrowed for a non-Gray document; for a Gray one the admitted
/// description (Gradient Map stops greyed, so Owned) or the refusal.
/// `layer` = an adjustment layer (add, add_at, set, plan !destructive).
pub(crate) fn admitted<'a>(
    doc: &PrecisionDocument,
    description: &'a str,
    layer: bool,
) -> Result<Cow<'a, str>, String> {
    if !doc.is_gray() {
        return Ok(Cow::Borrowed(description));
    }
    let Some((op, mut value)) = parts(description) else {
        return Ok(Cow::Borrowed(description));
    };
    rule(&op, &value["params"], layer)?;
    let Some(policy) = doc.gray_policy() else {
        return Ok(Cow::Borrowed(description));
    };
    if op == "gradient_map" && grey_stops(doc.profile(), policy, &mut value) {
        return Ok(Cow::Owned(value.to_string()));
    }
    Ok(Cow::Borrowed(description))
}

/// No-op off Gray. `adjustment` → [`admitted`] (not a layer) written back
/// into `edit.description`; `auto_tone`, `auto_contrast` and `equalize`
/// admitted; `levels_channels` admitted when its `black`, `white` and `gamma`
/// arrays each hold equal r, g, b values, else the Levels channels sentence
/// (the host's Auto Tone/Contrast write step,
/// `AgentServer+SelectedAdjustment.swift`); `auto_color` the
/// `unavailable_sentence` refusal; any other op (a filter)
/// `gray_filter::admit_filter`.
pub(crate) fn admit_selected(
    doc: &PrecisionDocument,
    edit: &mut SelectedEdit,
) -> Result<(), String> {
    if !doc.is_gray() {
        return Ok(());
    }
    match edit.op.as_str() {
        "adjustment" => {
            let Some(description) = edit.description.as_deref() else {
                return Ok(());
            };
            if let Cow::Owned(greyed) = admitted(doc, description, false)? {
                edit.description = Some(greyed);
            }
            Ok(())
        }
        "auto_tone" | "auto_contrast" | "equalize" => Ok(()),
        "levels_channels" => {
            let alike = ["black", "white", "gamma"].iter().all(|key| {
                edit.params.get(*key).is_none_or(|array| {
                    array
                        .as_array()
                        .is_some_and(|a| a.len() == 3 && a[0] == a[1] && a[1] == a[2])
                })
            });
            if alike {
                Ok(())
            } else {
                Err(channels_sentence("Levels"))
            }
        }
        "auto_color" => Err(unavailable_sentence("Auto Color")),
        _ => crate::gray_filter::admit_filter(doc, edit),
    }
}

/// Why a Gray document cannot hold an adjustment layer of `description` (the
/// table and the channel rule, as a layer), or `None`: a gray PSD's carried
/// adjustment record is omitted by name when it refuses (#547 phase 3a,
/// `psd_import_gray`). A description the kernel's own parser must refuse
/// answers `None` here.
pub(crate) fn record_refusal(description: &str) -> Option<String> {
    let (op, value) = parts(description)?;
    rule(&op, &value["params"], true).err()
}

/// Every `Content::Adjustment` of `layers` (recursing groups) passes the
/// table and the channel rule as a layer (not the stop greying, which every
/// admitted description already carries).
pub(crate) fn check_records(layers: &[PrecisionDocumentLayer]) -> Result<(), String> {
    for layer in layers {
        match &layer.content {
            Content::Adjustment(state) => {
                if let Some((op, value)) = parts(state.description()) {
                    rule(&op, &value["params"], true)?;
                }
            }
            Content::Group(children) | Content::PassThrough(children) => {
                check_records(children)?;
            }
            _ => {}
        }
    }
    Ok(())
}
