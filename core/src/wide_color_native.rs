//! The conditional WIDE COLOUR word (wide-gamut style colours): word
//! 100, set when some stored style holds a colour slot carrying a wide
//! colour, enabled or not ([`carries_wide`]: a `linear` beside one of the
//! seven effects' `color`, or a bevel's `highlight_linear` /
//! `shadow_linear`). The keys ride inside each style's JSON, so there is no
//! table; the word exists so that a build that knows word 99 but not wide
//! colours refuses the file by number instead of painting the slot's nearest
//! sRGB byte.
//!
//! It mirrors `glow_gradient_native`'s word 99 one for one: one number at
//! both depths — in a byte file the word inside 45 after 101
//! (`blend_if_channels_native`) and ahead of 99, in a typed file the word
//! after the CMYK slot and 101 (ahead of 99), a CMYK file's too — and the claim is checked both ways before any pixel decodes:
//! the word without such a slot is [`INVALID_WORD`], such a slot without the
//! word is [`WORD_REFUSAL`]. A document with none writes nothing, so its
//! bytes are the prior writer's. `DRDC_VERSION` stays 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, LayerStyle};

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 100;

/// A wide style colour in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native wide style colour requires DRDC 100";

/// The word in a file holding no wide style colour.
pub(crate) const INVALID_WORD: &str = "invalid native wide colour word";

/// Whether `effect` writes a wide colour, enabled or not.
pub(crate) fn carries_wide(effect: &Effect) -> bool {
    match effect {
        Effect::DropShadow(e) => e.linear.is_some(),
        Effect::InnerShadow(e) => e.linear.is_some(),
        Effect::OuterGlow(e) => e.linear.is_some(),
        Effect::InnerGlow(e) => e.linear.is_some(),
        Effect::Stroke(e) => e.linear.is_some(),
        Effect::ColorOverlay(e) => e.linear.is_some(),
        Effect::Satin(e) => e.linear.is_some(),
        Effect::BevelEmboss(e) => e.highlight_linear.is_some() || e.shadow_linear.is_some(),
        Effect::GradientOverlay(_) | Effect::PatternOverlay(_) => false,
    }
}

/// Whether `style` holds, enabled or not, a slot carrying a wide colour.
pub(crate) fn holds_wide(style: &LayerStyle) -> bool {
    style.effects.iter().any(carries_wide)
}

/// Whether some comp's recorded appearance holds such a slot (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_wide(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_wide(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_wide(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_wide))
        || comps_wide(comps)
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds such a slot.
pub(crate) fn styles_wide(doc: &DrDocument) -> bool {
    pending_wide(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree, or some comp.
pub(crate) fn styles_wide16(doc: &PrecisionDocument) -> bool {
    records_wide16(&doc.layers) || comps_wide(&doc.comps)
}

fn records_wide16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_wide(s)) || l.children().is_some_and(records_wide16)
    })
}
