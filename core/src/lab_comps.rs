//! Layer comps on a Lab Color document (#1 Lab phase 6a-ii,
//! `designs/lab-mode.md`): the two questions a Lab Apply asks.
//!
//! Every comp RULE is `doc_comps`'s, restated over typed layers by
//! `precision_comps` (binding, capture, apply, lock, purity, the payload
//! cap); this module adds only what the mode cannot hold:
//!
//! * **Apply** skips a recorded blend mode outside
//!   `lab_document::LAB_BLEND_MODES` (`lab_document::blend_refusal`) and a
//!   recorded style with an effect mode Lab refuses
//!   (`lab_style::style_refusal`) or a Blend If on a channel Lab refuses
//!   (`lab_style::blend_if_channel_refusal`: Gray or a plate), leaving the
//!   layer's own value — `doc_comps`' "an incompatible mode does not move"
//!   rule, as a Gray document skips (`gray_comps`). A comp's recorded style
//!   is the merged style and Blend If (`reported_style`), so a refused
//!   Blend If skips the whole recorded style. A comp recorded on a Lab
//!   document holds only what the mode admits; these arms meet one carried
//!   from a mode that held more.
//! * **RGB / Gray ↔ Lab** carries every comp unchanged
//!   (`lab_convert::carried`): the ids bind because
//!   `cmyk_convert_tree::map_tree` carries each record's id, and a Lab
//!   layer's stored style is authored sRGB (`lab_style`'s module doc), so a
//!   recorded style carries unchanged, the rule the layers' own styles take.
//!   A flatten clears the comps first, so a flattening conversion carries
//!   the flattened source's (none), as Gray. Lab → Gray carries through
//!   `gray_comps` already (`gray_convert`), and the carry needs no plan
//!   term: the comps are cloned, as Assign and Convert to Profile clone them
//!   (`precision_color`).

use crate::blend::BlendMode;
use crate::style::LayerStyle;

/// Whether a Lab document holds a recorded blend mode:
/// `lab_document::blend_refusal(mode).is_none()`.
pub(crate) fn admits_mode(mode: BlendMode) -> bool {
    crate::lab_document::blend_refusal(mode).is_none()
}

/// Whether a Lab document holds a recorded style: no effect mode
/// `lab_style::style_refusal` refuses and no Blend If channel
/// `lab_style::blend_if_channel_refusal` refuses.
pub(crate) fn admits_style(style: &LayerStyle) -> bool {
    crate::lab_style::style_refusal(style).is_none()
        && style
            .blend_if
            .as_ref()
            .is_none_or(|b| crate::lab_style::blend_if_channel_refusal(b).is_none())
}
