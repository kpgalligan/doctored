//! The Lab Color document's adjustments (#1 phases 2b to 2d, 2f): which
//! ops run on a Lab document and in which [`LabForm`], and which refuse for
//! good (`designs/lab-mode.md`).
//!
//! # No Lab kernel, no rewritten description
//!
//! Every admitted op runs the existing `PrecisionAdjustment` kernel through
//! `lab_tone`: `PrecisionAdjustment::parse` binds the op's form from
//! [`rule`] when the profile it is parsed against is the Lab twin
//! (`lab_document::is_lab_twin`), with the [`LabBinding`] the form needs, so
//! the destructive, adjustment-layer and selected paths agree and the stored
//! description is the RGB one unchanged (no DRDC change). The forms are
//! `lab_tone`'s (its module doc): Levels and Curves on the composite and
//! Brightness/Contrast act on Lightness, their r, g and b settings on
//! Lightness, a and b; Invert and Posterize on every channel; Threshold
//! makes a black and white image; a Brightness/Contrast saturation (Doctored's
//! own control) scales a\* and b\* about 0; Hue/Saturation's master
//! saturation −1 alone is Desaturate (L\* kept); and the colour ops run on
//! the sample's colour in the op's RGB space (`Color`).
//!
//! # The colour ops' space (phase 2d)
//!
//! All seven colour ops act on colour, none per Lab channel, so each runs
//! through an RGB intermediate ([`color_space`]). The
//! five that adjust the input colour (Hue/Saturation, Color Balance, Photo
//! Filter, Shadows/Highlights, Replace Color) run in the built-in 16-bit
//! ProPhoto RGB, ENCODED (gamma 461/256, which the kernels' tonal weights,
//! guide luma and code distances are written for): it holds every real
//! surface colour, so a small adjustment no longer clips a Lab colour outside
//! sRGB, and the result is "as on a 16-bit ProPhoto RGB document". Color
//! Lookup and Gradient Map look their result colour up (a `.cube` table, the
//! gradient's stops), whose numbers are sRGB on Lab, so they keep sRGB. A
//! request colour reaches the kernel in the space's numbers
//! ([`kernel_params`]): Photo Filter's is authored sRGB, as Clouds' is;
//! Replace Color's `colors` are the document's own numbers (`adjust`'s
//! schema), the 8-bit `#LLaabb` codes on Lab.
//!
//! # The table
//!
//! The table is the mode's own: `Unavailable` ops are unavailable in a
//! Lab document and refuse permanently (`unavailable_sentence`);
//! `RgbOnly` ops are Doctored's own colour
//! adjustments, refused for good too; `Pixels` ops run on pixels only, never
//! as an adjustment layer (Shadows/Highlights, Replace Color: Gray's rule).
//! An op outside the
//! table is unknown and answers as on RGB ("unsupported high-depth adjustment
//! {op}", `adjust_precision`'s). Auto Tone, Auto Contrast and Equalize act
//! on Lightness (`selected_auto_levels`, `equalize_apply`); a selected edit
//! that is none of these is a filter, `lab_filter`'s gate. The scalar mask
//! and alpha-channel path never binds a form
//! (`PrecisionAdjustment::parse_unbound`): it answers as on RGB.
//!
//! Lab → RGB carries every admitted layer's description unchanged
//! (`lab_convert`), and RGB/Gray → Lab carries an adjustment layer the same
//! way, rebound to the twin (phase 2d), except one [`lab_carry_refusal`]
//! names. Since phase 2f a Levels or Curves layer with channel settings
//! carries too, byte-equal, matched by channel index (Red ↔ Lightness,
//! Green ↔ a, Blue ↔ b): Doctored's rule, not measured — a round trip
//! restores the record, and a PSD stores these records by channel index.

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::icc::IccProfile;
use crate::icc_intent::{ColorPipeline, RenderingIntent};
use crate::lab_tone::LabForm;
use crate::precision_document::PrecisionDocument;
use crate::selected_adjustment::SelectedEdit;

/// What a Lab document does with one adjustment op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabAvailability {
    /// Runs on the L\*a\*b\* codes in this form, destructively and as a layer.
    Runs(LabForm),
    /// Runs in this form on pixels only, never as a layer.
    Pixels(LabForm),
    /// Disabled in a Lab document: refused for good.
    Unavailable,
    /// A Doctored colour adjustment, offered on RGB documents only.
    RgbOnly,
}

use LabAvailability::{Pixels, RgbOnly, Runs, Unavailable};

/// (op, display name, availability), Doctored's own colour adjustments
/// last. The names are `gray_adjust::GRAY_ADJUSTMENTS`' (the host's
/// `AdjustmentLayerOp.displayName`).
pub(crate) const LAB_ADJUSTMENTS: &[(&str, &str, LabAvailability)] = &[
    (
        "bcs",
        "Brightness/Contrast/Saturation",
        Runs(LabForm::Lightness),
    ),
    ("levels", "Levels", Runs(LabForm::Lightness)),
    ("curves", "Curves", Runs(LabForm::Lightness)),
    ("exposure", "Exposure", Unavailable),
    ("vibrance", "Vibrance", Unavailable),
    ("hue_saturation", "Hue/Saturation", Runs(LabForm::Color)),
    ("color_balance", "Color Balance", Runs(LabForm::Color)),
    ("black_and_white", "Black & White", Unavailable),
    ("photo_filter", "Photo Filter", Runs(LabForm::Color)),
    ("channel_mixer", "Channel Mixer", Unavailable),
    ("color_lookup", "Color Lookup", Runs(LabForm::Color)),
    ("invert", "Invert", Runs(LabForm::Channels)),
    ("posterize", "Posterize", Runs(LabForm::Channels)),
    ("threshold", "Threshold", Runs(LabForm::Threshold)),
    ("gradient_map", "Gradient Map", Runs(LabForm::Color)),
    ("selective_color", "Selective Color", Unavailable),
    (
        "shadows_highlights",
        "Shadows/Highlights",
        Pixels(LabForm::Color),
    ),
    ("replace_color", "Replace Color", Pixels(LabForm::Color)),
    ("hue_rotate", "Hue Rotate", RgbOnly),
    ("grayscale", "Grayscale", RgbOnly),
    ("sepia", "Sepia", RgbOnly),
    ("white_balance", "White Balance", RgbOnly),
];

/// What a bound form carries beyond the one kernel (`adjust_precision`
/// builds it at parse on the Lab twin; `lab_tone` evaluates it).
pub(crate) struct LabBinding {
    pub(crate) form: LabForm,
    /// Split: the kernel of `params` without the composite keys (levels:
    /// black, white, gamma, output_black, output_white; curves: rgb,
    /// rgb_map).
    pub(crate) channels: Option<Box<crate::adjust::Adjustment>>,
    /// Chroma: the bcs kernel's `saturation_scale` (Bcs or BcsModern); 1
    /// otherwise.
    pub(crate) chroma: f32,
    /// Color: [twin → space, space → twin], the space [`color_space`]'s,
    /// each built once by `retained_source_render::lut_pipeline`.
    pub(crate) color: Option<Box<[crate::icc_intent::ColorPipeline; 2]>>,
}

/// The composite keys a Split form's channel kernel drops ([`LabBinding`]).
pub(crate) fn composite_keys(op: &str) -> &'static [&'static str] {
    match op {
        "levels" => &["black", "white", "gamma", "output_black", "output_white"],
        "curves" => &["rgb", "rgb_map"],
        _ => &[],
    }
}

pub(crate) fn unavailable_sentence(name: &str) -> String {
    format!("{name} is unavailable in a Lab Color document.")
}

pub(crate) fn rgb_only_sentence(name: &str) -> String {
    format!(
        "{name} is unavailable in a Lab Color document: Doctored offers it on RGB documents only."
    )
}

fn layer_sentence(name: &str) -> String {
    format!(
        "A {name} adjustment layer is unavailable in a Lab Color document: {name} applies to pixels only."
    )
}

/// True when Hue/Saturation's `params` are its master saturation −1 alone
/// (the host's Image ▸ Adjustments ▸ Desaturate): `hue` and `lightness`
/// absent or 0, `colorize` absent or false (its `colorize_*` settings then
/// read nothing), `bands` absent or empty.
fn is_desaturate(params: &Map<String, Value>) -> bool {
    let zero = |key: &str| params.get(key).is_none_or(|v| v.as_f64() == Some(0.0));
    params.get("saturation").and_then(Value::as_f64) == Some(-1.0)
        && zero("hue")
        && zero("lightness")
        && params
            .get("colorize")
            .is_none_or(|v| v.as_bool() == Some(false))
        && params
            .get("bands")
            .is_none_or(|v| v.as_object().is_some_and(Map::is_empty))
}

/// The table plus the parameter rules for `op` with `params`: the form to
/// bind, or the sentence. Levels with any of `g`, `b`, and Curves with any
/// of `g`, `b`, `g_map`, `b_map`, bind [`LabForm::Split`] (`r` and `r_map`
/// are Lightness, the channel stage under the composite); `bcs` with a
/// `saturation` present and not 0 binds [`LabForm::Chroma`]; Hue/Saturation
/// of master saturation −1 alone binds [`LabForm::Desaturate`].
pub(crate) fn rule(op: &str, params: &Map<String, Value>) -> Result<LabForm, String> {
    let Some(&(_, name, availability)) = LAB_ADJUSTMENTS.iter().find(|(o, _, _)| *o == op) else {
        return Err(format!("unsupported high-depth adjustment {op}"));
    };
    let form = match availability {
        Runs(form) | Pixels(form) => form,
        Unavailable => return Err(unavailable_sentence(name)),
        RgbOnly => return Err(rgb_only_sentence(name)),
    };
    let any = |keys: &[&str]| keys.iter().any(|key| params.contains_key(*key));
    Ok(match op {
        "levels" if any(&["g", "b"]) => LabForm::Split,
        "curves" if any(&["g", "b", "g_map", "b_map"]) => LabForm::Split,
        "bcs"
            if params
                .get("saturation")
                .is_some_and(|v| v.as_f64() != Some(0.0)) =>
        {
            LabForm::Chroma
        }
        "hue_saturation" if is_desaturate(params) => LabForm::Desaturate,
        _ => form,
    })
}

/// A description as JSON with a string `op` and an object (or absent)
/// `params`, or `None` for one the kernel's own parser must refuse.
fn parts(description: &str) -> Option<Value> {
    let value = serde_json::from_str::<Value>(description).ok()?;
    value.get("op")?.as_str()?;
    match value.get("params") {
        None => {}
        Some(params) => {
            params.as_object()?;
        }
    }
    Some(value)
}

/// The op and params of a [`parts`] value.
fn op_params(value: &Value) -> (&str, &Map<String, Value>) {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    let op = value["op"].as_str().unwrap_or("");
    let params = value
        .get("params")
        .and_then(Value::as_object)
        .unwrap_or_else(|| EMPTY.get_or_init(Map::new));
    (op, params)
}

/// No-op off Lab; else [`rule`] on the description's op and params, and as
/// an adjustment layer (`layer`: add, add_at, set, plan !destructive) a
/// `Pixels` op's layer sentence. A description the kernel's own parser must
/// refuse is left to it.
pub(crate) fn admit(doc: &PrecisionDocument, description: &str, layer: bool) -> Result<(), String> {
    if !doc.is_lab() {
        return Ok(());
    }
    let Some(value) = parts(description) else {
        return Ok(());
    };
    let (op, params) = op_params(&value);
    rule(op, params)?;
    if layer {
        if let Some(&(_, name, Pixels(_))) = LAB_ADJUSTMENTS.iter().find(|(o, _, _)| *o == op) {
            return Err(layer_sentence(name));
        }
    }
    Ok(())
}

/// Why a Lab document cannot hold an adjustment layer of `description` (the
/// table as a layer: [`rule`], then a `Pixels` op's layer sentence), or
/// `None`: a Lab PSD's carried adjustment record is omitted by name when it
/// refuses (#1 Lab phase 3a, `psd_import_lab`). A description the
/// kernel's own parser must refuse answers `None` here.
pub(crate) fn record_refusal(description: &str) -> Option<String> {
    let value = parts(description)?;
    let (op, params) = op_params(&value);
    if let Err(sentence) = rule(op, params) {
        return Some(sentence);
    }
    match LAB_ADJUSTMENTS.iter().find(|(o, _, _)| *o == op) {
        Some(&(_, name, Pixels(_))) => Some(layer_sentence(name)),
        _ => None,
    }
}

/// No-op off Lab. `adjustment` → [`admit`] (not a layer); `auto_tone`,
/// `auto_contrast`, `equalize` and `levels_channels` admitted (they act on
/// Lightness: `selected_auto_levels`, `equalize_apply`); `auto_color` →
/// `unavailable_sentence` ("Auto Color"); every other op (a filter) →
/// `lab_filter::admit_filter`, which may rewrite Clouds' colours.
pub(crate) fn admit_selected(
    doc: &PrecisionDocument,
    edit: &mut SelectedEdit,
) -> Result<(), String> {
    if !doc.is_lab() {
        return Ok(());
    }
    match edit.op.as_str() {
        "adjustment" => edit
            .description
            .as_deref()
            .map_or(Ok(()), |description| admit(doc, description, false)),
        "auto_tone" | "auto_contrast" | "equalize" | "levels_channels" => Ok(()),
        "auto_color" => Err(unavailable_sentence("Auto Color")),
        _ => crate::lab_filter::admit_filter(doc, edit),
    }
}

/// True for an op whose table form is [`LabForm::Color`].
fn has_color_form(op: &str) -> bool {
    LAB_ADJUSTMENTS
        .iter()
        .any(|(o, _, a)| *o == op && matches!(a, Runs(LabForm::Color) | Pixels(LabForm::Color)))
}

/// The RGB space a Color form's kernel runs in (module doc):
/// `IccProfile::prophoto_rgb()` for hue_saturation, color_balance,
/// photo_filter, shadows_highlights, replace_color; `IccProfile::srgb()` for
/// color_lookup and gradient_map (and any other op, which has no Color form).
pub(crate) fn color_space(op: &str) -> Arc<IccProfile> {
    match op {
        "hue_saturation" | "color_balance" | "photo_filter" | "shadows_highlights"
        | "replace_color" => IccProfile::prophoto_rgb(),
        _ => IccProfile::srgb(),
    }
}

/// `params` cloned with each request colour in `space`'s numbers (module
/// doc): photo_filter `color` (authored sRGB; absent: "#ec8a00", the
/// kernel's default, written converted) through `ColorPipeline::new(srgb,
/// space, RelativeColorimetric, false).color8`; replace_color `colors[]`
/// (`#LLaabb`, the Lab document's own 8-bit codes) through `from_twin` (the
/// bound twin → space pipeline), rounded to 8 bits. A value that does not
/// parse is left for the kernel's parser. `Err` is `ColorPipeline::new`'s
/// sentence.
pub(crate) fn kernel_params(
    op: &str,
    params: &Map<String, Value>,
    space: &IccProfile,
    from_twin: &ColorPipeline,
) -> Result<Map<String, Value>, String> {
    use crate::style_names::{color_hex, parse_color};
    let mut out = params.clone();
    match op {
        "photo_filter" => {
            let hex = match params.get("color") {
                None => "#ec8a00",
                Some(value) => match value.as_str() {
                    Some(hex) => hex,
                    None => return Ok(out),
                },
            };
            if let Some(rgb) = parse_color(hex) {
                let pipeline = ColorPipeline::new(
                    &IccProfile::srgb(),
                    space,
                    RenderingIntent::RelativeColorimetric,
                    false,
                )?;
                out.insert("color".into(), color_hex(pipeline.color8(rgb)).into());
            }
        }
        "replace_color" => {
            if let Some(Value::Array(colors)) = out.get_mut("colors") {
                for color in colors.iter_mut() {
                    let Some(codes) = color.as_str().and_then(parse_color) else {
                        continue;
                    };
                    let rgb = from_twin.color(codes.map(|c| f32::from(c) / 255.0));
                    let rgb8 = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                    *color = color_hex(rgb8).into();
                }
            }
        }
        _ => {}
    }
    Ok(out)
}

/// One L\*, a\*, b\* (D50; clamped to 0…100, −128…127) as [`color_space`]'s
/// encoded 0…1 RGB, clamped: the codes by `lab_document`'s encoding (L\* /
/// 100, (a\* + 128) / 255) through `lut_pipeline(&lab_twin(),
/// &color_space(op))`, the pipeline a bound Color form's kernel reads
/// through. `None` for an op without the Color form, a non-finite component
/// or a pipeline failure.
pub(crate) fn kernel_rgb(op: &str, lab: [f32; 3]) -> Option<[f32; 3]> {
    if !has_color_form(op) || lab.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let codes = [
        lab[0].clamp(0.0, 100.0) / 100.0,
        (lab[1].clamp(-128.0, 127.0) + 128.0) / 255.0,
        (lab[2].clamp(-128.0, 127.0) + 128.0) / 255.0,
    ];
    let twin = crate::lab_document::lab_twin();
    let pipeline = crate::retained_source_render::lut_pipeline(&twin, &color_space(op)).ok()??;
    Some(pipeline.color(codes).map(|v| v.clamp(0.0, 1.0)))
}

/// RGB/Gray → Lab (`lab_convert::first_lab_blocker`): `Some(sentence)` for
/// an adjustment record a Lab document cannot carry, `None` to carry it
/// (rebound to the twin, which binds its Lab form). An op [`rule`] refuses,
/// or a `Pixels` op (no such layer), answers that sentence; a Levels or
/// Curves record with channel settings carries, its Red, Green and Blue
/// stages becoming Lightness, a and b (phase 2f, module doc). A description
/// the kernel's own parser must refuse is left to the rebind.
pub(crate) fn lab_carry_refusal(layer_name: &str, description: &str) -> Option<String> {
    let value = parts(description)?;
    let (op, params) = op_params(&value);
    let sentence = match rule(op, params) {
        Err(sentence) => sentence,
        Ok(_) => match LAB_ADJUSTMENTS.iter().find(|(o, _, _)| *o == op) {
            Some(&(_, name, Pixels(_))) => layer_sentence(name),
            _ => return None,
        },
    };
    Some(format!(
        "“{layer_name}” cannot convert to Lab Color: {sentence} Flatten the image to convert it."
    ))
}
