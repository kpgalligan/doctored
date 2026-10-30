//! The conditional NOISE word (Layer Style Noise): word
//! 97, set when some stored style holds a Drop Shadow, Inner Shadow, Outer
//! Glow or Inner Glow whose `noise` is above 0, enabled or not
//! ([`carries_noise`]). The key rides inside each style's JSON, so there is
//! no table; the word exists so that a build that knows word 96 but not
//! noise refuses the file by number instead of rendering every grain away.
//!
//! It mirrors `contour_native`'s word 96 one for one: one number at both
//! depths — in a byte file the FIRST word inside 45 (ahead of 96), in a
//! typed file the word right after the CMYK slot (ahead of 96), a CMYK
//! file's too — and the claim is checked both ways before any pixel decodes:
//! the word without such an effect is [`INVALID_WORD`], such an effect
//! without the word is [`WORD_REFUSAL`]. A document with none writes
//! nothing, so its bytes are the prior writer's. `DRDC_VERSION` stays 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, LayerStyle};

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 97;

/// Noise in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native layer style noise requires DRDC 97";

/// The word in a file holding no noise.
pub(crate) const INVALID_WORD: &str = "invalid native noise word";

/// Whether `effect` writes the `noise` key: one of the four kinds with noise
/// above 0, enabled or not.
pub(crate) fn carries_noise(effect: &Effect) -> bool {
    match effect {
        Effect::DropShadow(e) => e.noise > 0.0,
        Effect::InnerShadow(e) => e.noise > 0.0,
        Effect::OuterGlow(e) => e.noise > 0.0,
        Effect::InnerGlow(e) => e.noise > 0.0,
        _ => false,
    }
}

/// Whether `style` holds, enabled or not, an effect carrying noise.
pub(crate) fn holds_noise(style: &LayerStyle) -> bool {
    style.effects.iter().any(carries_noise)
}

/// Whether some comp's recorded appearance holds such an effect (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_noise(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_noise(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_noise(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_noise))
        || comps_noise(comps)
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds such an effect.
pub(crate) fn styles_noise(doc: &DrDocument) -> bool {
    pending_noise(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree, or some comp.
pub(crate) fn styles_noise16(doc: &PrecisionDocument) -> bool {
    records_noise16(&doc.layers) || comps_noise(&doc.comps)
}

fn records_noise16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_noise(s)) || l.children().is_some_and(records_noise16)
    })
}
