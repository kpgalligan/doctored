//! The conditional GLOW GRADIENT word (gradient glows with Jitter):
//! word 99, set when some stored style holds an Outer or Inner Glow that
//! carries a gradient or a Jitter, enabled or not ([`carries_glow_gradient`]:
//! `fill_type` gradient, stops that are not the default, or `jitter` above
//! 0). The keys ride inside each style's JSON, so there is no table; the
//! word exists so that a build that knows word 98 but not the glow's
//! gradient refuses the file by number instead of painting a colour glow.
//!
//! It mirrors `texture_native`'s word 98 one for one: one number at both
//! depths — in a byte file the word inside 45 after 101 and 100 and ahead
//! of 98, in a typed file the word after the CMYK slot, 101 and 100 (ahead
//! of 98), a CMYK file's too — and the claim is checked both ways before any pixel decodes:
//! the word without such an effect is [`INVALID_WORD`], such an effect
//! without the word is [`WORD_REFUSAL`]. A document with none writes
//! nothing, so its bytes are the prior writer's. `DRDC_VERSION` stays 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, LayerStyle};
use crate::style_glow_gradient::glow_carries_stops;

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 99;

/// A gradient glow in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native gradient glow requires DRDC 99";

/// The word in a file holding no gradient glow.
pub(crate) const INVALID_WORD: &str = "invalid native glow gradient word";

/// Whether `effect` writes a glow's gradient keys: an Outer or Inner Glow
/// whose `fill_type` is gradient, whose stops are not the default, or whose
/// `jitter` is above 0, enabled or not.
pub(crate) fn carries_glow_gradient(effect: &Effect) -> bool {
    match effect {
        Effect::OuterGlow(g) => glow_carries_stops(g.fill_type, &g.gradient) || g.jitter > 0.0,
        Effect::InnerGlow(g) => glow_carries_stops(g.fill_type, &g.gradient) || g.jitter > 0.0,
        _ => false,
    }
}

/// Whether `style` holds, enabled or not, a glow carrying a gradient.
pub(crate) fn holds_glow_gradient(style: &LayerStyle) -> bool {
    style.effects.iter().any(carries_glow_gradient)
}

/// Whether some comp's recorded appearance holds such a glow (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_glow_gradient(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_glow_gradient(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_glow_gradient(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_glow_gradient))
        || comps_glow_gradient(comps)
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds such a glow.
pub(crate) fn styles_glow_gradient(doc: &DrDocument) -> bool {
    pending_glow_gradient(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree, or some comp.
pub(crate) fn styles_glow_gradient16(doc: &PrecisionDocument) -> bool {
    records_glow_gradient16(&doc.layers) || comps_glow_gradient(&doc.comps)
}

fn records_glow_gradient16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_glow_gradient(s))
            || l.children().is_some_and(records_glow_gradient16)
    })
}
