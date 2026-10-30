//! The conditional MULTICHANNEL BLEND IF word (Blend If on
//! several channels at once): word 101, set when some stored Blend If holds
//! more than one range ([`BlendIf::is_multichannel`]: Gray plus Red / Green /
//! Blue, or Gray plus the CMYK plates). A byte document's ranges ride inside
//! each style's JSON (`blend_if.extra`), which an older reader would drop
//! silently; a typed document's ride in `LBIF` schema 2
//! (`precision_blend_if_native`), which the same word claims. The word exists
//! so that a build that knows word 99 but not the extra ranges refuses the
//! file by number instead of rendering one channel's gate.
//!
//! It mirrors `glow_gradient_native`'s word 99 one for one: one number at
//! both depths — in a byte file the FIRST word inside 45 (ahead of 100), in
//! a typed file the word right after the CMYK slot (ahead of 100), a CMYK
//! file's too — and the claim is checked both ways before any pixel decodes:
//! the word without a multichannel Blend If is [`INVALID_WORD`], one without
//! the word is [`WORD_REFUSAL`]. A document with none writes nothing, so its
//! bytes are the prior writer's. `DRDC_VERSION` stays 89. Word 100
//! (`wide_color_native`, wide-gamut style colours) is the next word after
//! it.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::PrecisionDocument;
use crate::style::LayerStyle;

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 101;

/// A multichannel Blend If in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native multichannel blend if requires DRDC 101";

/// The word in a file holding no multichannel Blend If.
pub(crate) const INVALID_WORD: &str = "invalid native multichannel blend if word";

/// Whether `style`'s Blend If holds more than one range.
pub(crate) fn holds_multichannel(style: &LayerStyle) -> bool {
    style.blend_if.is_some_and(|b| b.is_multichannel())
}

/// Whether some comp's recorded appearance holds a multichannel Blend If
/// (read leniently, as the `.rz` readers read it; one that does not parse
/// claims nothing).
pub(crate) fn comps_multichannel(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_multichannel(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_multichannel(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_multichannel))
        || comps_multichannel(comps)
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds a multichannel Blend If.
pub(crate) fn styles_multichannel(doc: &DrDocument) -> bool {
    pending_multichannel(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree (its own Blend If, the
/// `LBIF` field; a typed style never holds one), or some comp.
pub(crate) fn styles_multichannel16(doc: &PrecisionDocument) -> bool {
    crate::precision_blend_if_native::precision_entries(doc)
        .iter()
        .any(|(_, b)| b.is_multichannel())
        || comps_multichannel(&doc.comps)
}
