//! The conditional Pattern Overlay ANGLE word: word 93, set
//! when some stored style holds a pattern overlay whose `angle` is not 0.
//! The angle rides inside each style's JSON (`style_pattern`), so there is no
//! table; the word exists so that a build that knows word 90 but not the
//! angle refuses the file by number instead of drawing the overlay unrotated.
//!
//! It mirrors `style_pattern`'s word 90 one for one: one number at both
//! depths — in a byte file the FIRST word inside 45 (ahead of 91), in a typed
//! file the word right after the CMYK slot (ahead of 91), a CMYK file's too —
//! and the claim is checked both ways before any pixel decodes: the word
//! without an angled overlay is [`INVALID_WORD`], an angled overlay without
//! the word is [`WORD_REFUSAL`]. An angled overlay is a pattern overlay, so
//! 93 implies 90 and both checks run. A document with no angled overlay
//! writes nothing, so its bytes are the prior writer's. `DRDC_VERSION` stays
//! 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, LayerStyle};

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 93;

/// An angled pattern overlay in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native pattern overlay angles require DRDC 93";

/// The word in a file holding no angled pattern overlay.
pub(crate) const INVALID_WORD: &str = "invalid native pattern overlay angle word";

/// Whether `style` holds a pattern overlay, enabled or not, whose angle is
/// not 0.
pub(crate) fn holds_angle(style: &LayerStyle) -> bool {
    style
        .effects
        .iter()
        .any(|e| matches!(e, Effect::PatternOverlay(p) if p.fill.angle != 0.0))
}

/// Whether some comp's recorded appearance holds an angled pattern overlay
/// (read leniently, as the `.rz` readers read it; one that does not parse
/// claims nothing).
pub(crate) fn comps_angle(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_angle(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_angle(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_angle))
        || comps_angle(comps)
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds an angled pattern overlay.
pub(crate) fn styles_angle(doc: &DrDocument) -> bool {
    pending_angle(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree, or some comp.
pub(crate) fn styles_angle16(doc: &PrecisionDocument) -> bool {
    records_angle16(&doc.layers) || comps_angle(&doc.comps)
}

fn records_angle16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_angle(s)) || l.children().is_some_and(records_angle16)
    })
}
