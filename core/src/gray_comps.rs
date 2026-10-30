//! Layer comps on a Grayscale document (#547 phase 6a-ii,
//! `designs/grayscale-mode.md`): RGB → Gray's comp carry and the two
//! questions a Gray Apply asks.
//!
//! Every comp RULE is `doc_comps`'s, restated over typed layers by
//! `precision_comps` (binding, capture, apply, lock, purity, the payload
//! cap); this module adds only what the mode cannot hold:
//!
//! * **Apply** skips a recorded blend mode outside the Grayscale list
//!   (`gray_document::blend_refusal`; at 32 bits the float list) and a
//!   recorded style with an effect mode the list refuses
//!   (`gray_style::style_refusal`) or a Blend If on a channel other than
//!   Gray, leaving the layer's own value — `doc_comps`' "an incompatible
//!   mode does not move" rule, as a FLOAT document skips a style. A comp
//!   recorded on a Gray document holds only what the mode admits; these
//!   arms meet one carried from a mode that held more.
//! * **RGB → Gray** carries every comp, its entries as they are (the ids
//!   bind because `cmyk_convert_tree::map_tree` carries each record's id),
//!   each recorded style greyed by the Mode rule the layers' own styles take
//!   (`gray_style::greyed_json`, every custom tile texel mapped, `tiles_in`
//!   the source profile), so applying the comp after the conversion sets
//!   the style the layer itself would have carried. A flatten clears the
//!   comps first, so a flattening conversion carries none. Gray → RGB,
//!   Assign and Convert to Profile and Bits/Channel carry comps already
//!   (`PrecisionDocument::rebuilt`).
//!
//! The carry's bytes are a term of the existing RGB → Gray plan
//! ([`carried_bytes`]), bounded by `comps_native::MAX_COMPS_JSON_LEN`.

use crate::blend::BlendMode;
use crate::doc_comps::LayerComp;
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::icc::IccProfile;
use crate::style::{BlendIfChannel, LayerStyle};

/// RGB → Gray's comps: entries as they are (ids carried by `map_tree`), each
/// recorded style greyed by `gray_style::greyed_json` (`tiles_in` = the
/// source profile).
pub(crate) fn carried_comps(
    comps: &[LayerComp],
    tiles_in: &IccProfile,
    policy: &GrayPolicy,
) -> Result<Vec<LayerComp>, String> {
    let mut out = comps.to_vec();
    for comp in &mut out {
        for entry in &mut comp.entries {
            if let Some(style) = entry.appearance.as_mut().and_then(|a| a.style.as_mut()) {
                *style = crate::gray_style::greyed_json(style, tiles_in, policy)
                    .map_err(|e| format!("layer comp \"{}\": {e}", comp.name))?;
            }
        }
    }
    Ok(out)
}

/// The bytes [`carried_comps`] allocates: 4 × the summed byte length of
/// every recorded style JSON (the parse, the greyed copy, its
/// serialization, the replaced string).
pub(crate) fn carried_bytes(comps: &[LayerComp]) -> u64 {
    comps
        .iter()
        .flat_map(|comp| &comp.entries)
        .filter_map(|entry| entry.appearance.as_ref()?.style.as_ref())
        .map(|style| 4 * style.len() as u64)
        .fold(0u64, u64::saturating_add)
}

/// Whether a Gray document holds a recorded blend mode:
/// `gray_document::blend_refusal(mode, depth == 32).is_none()`.
pub(crate) fn admits_mode(policy: &GrayPolicy, mode: BlendMode) -> bool {
    crate::gray_document::blend_refusal(mode, policy.depth() == GrayDepth::ThirtyTwo).is_none()
}

/// Whether a Gray document holds a recorded style: no effect mode
/// `gray_style::style_refusal` refuses and no Blend If channel but Gray.
pub(crate) fn admits_style(policy: &GrayPolicy, style: &LayerStyle) -> bool {
    let float = policy.depth() == GrayDepth::ThirtyTwo;
    crate::gray_style::style_refusal(style, float).is_none()
        && style
            .blend_if
            .as_ref()
            .is_none_or(|b| b.ranges().all(|r| r.channel == BlendIfChannel::Gray))
}
