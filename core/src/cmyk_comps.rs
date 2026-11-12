//! Layer comps on a CMYK document (#1, `designs/cmyk-mode-status.md`): the
//! one question a CMYK Apply asks and what each conversion does to a comp's
//! recorded style.
//!
//! Every comp RULE is `doc_comps`'s, restated over typed layers by
//! `precision_comps` (binding, capture, apply, lock, purity, the payload
//! cap), position included: a CMYK leaf is a pixel leaf, so its offset is
//! recorded, applied, remapped by whole-canvas geometry and shifted by
//! storage growth as an RGB raster's is (`comps_geometry`). This module adds
//! only what the mode cannot hold and how the recorded styles cross:
//!
//! * **Apply** skips a recorded style the mode cannot hold
//!   (`cmyk_document::style_refusal`: a Blend If on Red, Green or Blue),
//!   leaving the layer's own — `doc_comps`' "an incompatible mode does not
//!   move" rule, as Gray and Lab skip. A leaf or a group with a leaf already
//!   refuses such a Blend If inside `with_blend_if` (its mode hint), which
//!   Apply turns into the same skip; an EMPTY group has no hint, so without
//!   this question the record would reach `build_cmyk` and fail the whole
//!   Apply. No recorded blend mode is skipped: `CMYK_BLEND_MODES` lists every
//!   leaf mode, and Pass Through is a group's, restored as on RGB.
//! * **RGB, Gray or Lab → CMYK** carries every comp unchanged
//!   (`cmyk_convert::rgb_to_cmyk`): the ids bind because
//!   `cmyk_convert_tree::map_tree` carries each record's id, and a recorded
//!   style crosses as the layers' own styles do, its authored hex converted
//!   at composite time. A flatten clears the comps first, so a flattening
//!   conversion carries none.
//! * **CMYK → RGB** carries every comp with each recorded style's inks
//!   dropped ([`rgb_carried`]), the rule `cmyk_convert_tree::without_inks`
//!   gives the layers' own styles; a flatten carries none. A recorded Blend If
//!   on a plate crosses too (a layer's own would block the conversion), and
//!   an RGB Apply skips that style ([`rgb_admits_style`]).
//! * **Convert to Profile** maps each recorded ink through the conversion's
//!   leg and **Assign** re-canonicalises each recorded ink's hex
//!   ([`inks_mapped`], `cmyk_profile`), exactly what each does to a layer's
//!   own style; 8/16 Bits touches no style.
//!
//! So applying a comp after any of these sets the style the layer itself
//! would have carried. The carries that rewrite a recorded style charge
//! `gray_comps::carried_bytes` (4 × the recorded style JSON), bounded by
//! `comps_native::MAX_COMPS_JSON_LEN`.

use crate::doc_comps::LayerComp;
use crate::style::LayerStyle;

/// Whether a CMYK document holds a recorded style:
/// `cmyk_document::style_refusal(style).is_none()`.
pub(crate) fn admits_style(style: &LayerStyle) -> bool {
    crate::cmyk_document::style_refusal(style).is_none()
}

/// Whether an RGB document holds a recorded style: no Blend If on a plate.
/// Only a comp carried from CMYK records one (a layer's own plate Blend If
/// blocks RGB Color instead), and an RGB Apply skips it as a CMYK one skips
/// Red, Green or Blue — an empty group would otherwise take it.
pub(crate) fn rgb_admits_style(style: &LayerStyle) -> bool {
    !style.has_cmyk_blend_if()
}

/// `comps` with every recorded style that carries an ink rewritten by `f`;
/// an ink-less style is left byte for byte. A recorded style that does not
/// parse refuses, naming its comp (`gray_comps::carried_comps`' rule).
pub(crate) fn inks_mapped(
    comps: &[LayerComp],
    f: &mut dyn FnMut(&LayerStyle) -> Result<LayerStyle, String>,
) -> Result<Vec<LayerComp>, String> {
    let mut out = comps.to_vec();
    for comp in &mut out {
        for entry in &mut comp.entries {
            let Some(json) = entry.appearance.as_mut().and_then(|a| a.style.as_mut()) else {
                continue;
            };
            let style = LayerStyle::from_json(json)
                .map_err(|e| format!("layer comp \"{}\": {e}", comp.name))?;
            if style.has_inks() {
                *json = f(&style)?.to_json();
            }
        }
    }
    Ok(out)
}

/// CMYK → RGB's comps (module doc): every recorded ink dropped
/// (`LayerStyle::without_inks`), the authored hex kept.
pub(crate) fn rgb_carried(comps: &[LayerComp]) -> Result<Vec<LayerComp>, String> {
    inks_mapped(comps, &mut |style| Ok(style.without_inks()))
}
