//! The Lab Color document's filters (#547 phases 2c, 2d, 2f and 4a-ii,
//! `designs/lab-mode.md`): the filter gates of the layer and of a mask
//! or alpha channel, and the one colour rule a filter request needs.
//!
//! # The channel form, no Lab kernel
//!
//! A Lab document stores L\*, a\* and b\* in the R, G and B samples
//! (`lab_document`'s module doc). Every admitted filter runs its
//! `SelectedFilter` kernel on those codes unchanged (`selected_edit_unlocked`,
//! the RGB selected-edit plan): its kernels are per-channel — each output
//! channel reads only that channel and alpha — so the result on L\*, a\* and
//! b\* is the kernel's one-channel result on each. Add Noise draws per
//! channel, so a\* and b\* get colour
//! noise, as RGB's do. High Pass's 0.5 pivot is the unit code 0.5, which
//! sits 0.5 a\* below the neutral 0x8080 (Invert's 0x7F7F, `lab_tone`'s
//! module doc, is the same half-code offset).
//!
//! Five kernels read a Rec. 709 luma across the three samples — Emboss,
//! Edge Detect, Lens Blur (its Brightness), Reduce Noise and Smart Sharpen —
//! so on a Lab layer they would read a\* as green. Since phase 2f they run
//! in `lab_channel_filter`'s pass form: one gray pass per channel (Emboss and
//! Edge Detect one Lightness pass, a\* = b\* = 0), as does Offset's `fill`
//! edge, whose one value is not a Lab colour (its a\* and b\* passes fill
//! the neutral). `selected_adjustment` routes them there. Since phase 4a-ii
//! Liquify runs too: its RGB kernel on the codes, each channel resampled
//! alike, as the geometry of phase 4a.
//!
//! # A mask or alpha channel (phase 2d)
//!
//! The scalar path (`precision_scalar_filter`) runs a filter on the one
//! plane as the gray `[g, g, g]` and keeps its luma, exactly as on RGB, so
//! there the pass filters, Offset's fill edge and Clouds run unconverted: a
//! plane is no Lab colour. [`scalar_refusal`] refuses only the rows that
//! refuse for good (`Unavailable`, `DevelopFilter`, `RgbOnly`); Liquify there answers
//! the scalar path's own refusal, as on RGB (phase 4a-ii).
//!
//! A colour the REQUEST names — Clouds' foreground and background — becomes
//! Lab codes by Mode ▸ Lab Color's rule before the kernel ([`lab_hex`], Gray's
//! `gray_adjust::grey_hex` shape), as paint inks do.
//!
//! # The table
//!
//! The table records phase 2's take (`lab-mode-547/take1`):
//! [`LabFilter::Unavailable`] filters are unavailable in a Lab document and
//! refuse for good; [`LabFilter::DevelopFilter`] are Develop Filter controls,
//! unavailable in Lab; [`LabFilter::RgbOnly`] is Doctored's own.

use crate::lab_adjust::{rgb_only_sentence, unavailable_sentence};
use crate::precision_document::PrecisionDocument;
use crate::selected_adjustment::SelectedEdit;

/// What a Lab document does with one filter op of [`LAB_FILTERS`]; a filter
/// outside it runs (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabFilter {
    /// Disabled in a Lab document: refused for good.
    Unavailable,
    /// A Develop Filter control, disabled in Lab.
    DevelopFilter,
    /// A Doctored filter offered on RGB documents only.
    RgbOnly,
}

/// (op, name, availability); names are the host's `FloatDocument.filterNames`.
pub(crate) const LAB_FILTERS: &[(&str, &str, LabFilter)] = &[
    ("lens_correction", "Lens Correction", LabFilter::Unavailable),
    ("develop_filter", "Develop Filter", LabFilter::Unavailable),
    (
        "difference_clouds",
        "Difference Clouds",
        LabFilter::Unavailable,
    ),
    ("presence", "Presence", LabFilter::DevelopFilter),
    ("add_grain", "Add Grain", LabFilter::DevelopFilter),
    ("depth_blur", "Depth Blur", LabFilter::RgbOnly),
];

fn develop_filter_sentence(name: &str) -> String {
    format!(
        "{name} is unavailable in a Lab Color document: it is a Develop Filter control, and the Develop Filter is unavailable in Lab."
    )
}

/// The sentence of a [`LAB_FILTERS`] row: the one wording both gates answer.
fn row_sentence(name: &str, availability: LabFilter) -> String {
    match availability {
        LabFilter::Unavailable => unavailable_sentence(name),
        LabFilter::DevelopFilter => develop_filter_sentence(name),
        LabFilter::RgbOnly => rgb_only_sentence(name),
    }
}

/// A Lab mask, alpha channel or plane `scalar_edit` operation's refusal (module doc). For
/// `{"kind":"filter","edit":{"op":..}}` it answers the sentence of a
/// [`LAB_FILTERS`] row. Otherwise `None`:
/// the pass filters, Offset's fill edge and Clouds run unconverted on the
/// `[g, g, g]` plane, as on RGB, and every other operation (an adjustment
/// parses unbound; Liquify, `precision_scalar_filter::LIQUIFY_REFUSAL`)
/// answers as on RGB.
pub(crate) fn scalar_refusal(operation: &serde_json::Value) -> Option<String> {
    if operation["kind"].as_str() != Some("filter") {
        return None;
    }
    let op = operation["edit"]["op"].as_str()?;
    let &(_, name, availability) = LAB_FILTERS.iter().find(|(o, _, _)| *o == op)?;
    Some(row_sentence(name, availability))
}

/// Lab filter gate, asked by `lab_adjust::admit_selected` for every op that
/// is not `adjustment`, Auto, `equalize` or `levels_channels`; no-op off
/// Lab. A [`LAB_FILTERS`] row → its sentence; `clouds` → both colour keys set
/// to [`lab_hex`] (an absent key: of #000000 / #ffffff, the kernel's
/// defaults); every other op admitted (its parse refuses an unknown one), the
/// pass filters and Offset's fill edge to `lab_channel_filter` (phase 2f),
/// Liquify to the RGB selected edit (phase 4a-ii).
pub(crate) fn admit_filter(doc: &PrecisionDocument, edit: &mut SelectedEdit) -> Result<(), String> {
    if !doc.is_lab() {
        return Ok(());
    }
    let op = edit.op.as_str();
    if let Some(&(_, name, availability)) = LAB_FILTERS.iter().find(|(o, _, _)| *o == op) {
        return Err(row_sentence(name, availability));
    }
    match op {
        "clouds" => {
            for (key, default) in [
                ("foreground_color", "#000000"),
                ("background_color", "#ffffff"),
            ] {
                let hex = match edit.params.get(key) {
                    None => default,
                    Some(value) => match value.as_str() {
                        Some(hex) => hex,
                        // Not a string: the kernel's parser refuses it.
                        None => continue,
                    },
                };
                if let Some(lab) = lab_hex(doc, hex) {
                    edit.params.insert(key.into(), lab.into());
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `#rrggbb` (sRGB) as the 8-bit `#LLaabb` of its Lab codes, by Mode ▸ Lab
/// Color's rule: `authored_color::authored_color(twin, true, rgb / 255)`,
/// `LabPolicy::code` at the document's depth, rounded to 8 bits (`/ 257`).
/// `None` off Lab or for a colour that does not parse.
pub(crate) fn lab_hex(doc: &PrecisionDocument, hex: &str) -> Option<String> {
    let policy = doc.lab_policy()?;
    let rgb = crate::style_names::parse_color(hex)?;
    let unit = rgb.map(|c| f32::from(c) / 255.0);
    let lab = crate::authored_color::authored_color(policy.twin(), true, unit)?;
    let codes = lab.map(|v| {
        let code = policy.code(f64::from(v));
        (f64::from(code) / 257.0).round() as u8
    });
    Some(crate::style_names::color_hex(codes))
}
