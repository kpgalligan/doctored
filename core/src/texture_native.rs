//! The conditional TEXTURE word (the Bevel & Emboss Texture):
//! word 98, set when some stored style holds a Bevel & Emboss that carries
//! a texture, enabled or not ([`carries_texture`]: `texture_enabled`, or a
//! texture that is not the default). The keys ride inside each style's JSON,
//! so there is no table; the word exists so that a build that knows word 97
//! but not the texture refuses the file by number instead of rendering a
//! flat bevel.
//!
//! It mirrors `noise_native`'s word 97 one for one: one number at both
//! depths — in a byte file the FIRST word inside 45 (ahead of 97), in a
//! typed file the word right after the CMYK slot (ahead of 97), a CMYK
//! file's too — and the claim is checked both ways before any pixel decodes:
//! the word without such an effect is [`INVALID_WORD`], such an effect
//! without the word is [`WORD_REFUSAL`]. A document with none writes
//! nothing, so its bytes are the prior writer's. `DRDC_VERSION` stays 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, LayerStyle};

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 98;

/// A bevel texture in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native bevel texture requires DRDC 98";

/// The word in a file holding no bevel texture.
pub(crate) const INVALID_WORD: &str = "invalid native texture word";

/// Whether `effect` writes the bevel's texture keys: a Bevel & Emboss that
/// `bevel_carries_texture`, enabled or not.
pub(crate) fn carries_texture(effect: &Effect) -> bool {
    match effect {
        Effect::BevelEmboss(b) => crate::style_pattern_fill::bevel_carries_texture(b),
        _ => false,
    }
}

/// Whether `style` holds, enabled or not, a bevel carrying a texture.
pub(crate) fn holds_texture(style: &LayerStyle) -> bool {
    style.effects.iter().any(carries_texture)
}

/// Whether some comp's recorded appearance holds such a bevel (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_texture(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_texture(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_texture(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_texture))
        || comps_texture(comps)
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds such a bevel.
pub(crate) fn styles_texture(doc: &DrDocument) -> bool {
    pending_texture(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree, or some comp.
pub(crate) fn styles_texture16(doc: &PrecisionDocument) -> bool {
    records_texture16(&doc.layers) || comps_texture(&doc.comps)
}

fn records_texture16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_texture(s)) || l.children().is_some_and(records_texture16)
    })
}
