//! The conditional CONTOUR word (Layer Style Contour):
//! word 96, set when some stored style holds an effect that writes a
//! contour-family key — a contour that is not Linear, an Anti-aliased, a
//! glow's Range or the bevel's Contour sub-panel, enabled or not
//! (`style_contour_json::carries_contour`). The keys ride inside each
//! style's JSON, so there is no table; the word exists so that a build that
//! knows word 95 but not contours refuses the file by number instead of
//! rendering every contour as Linear.
//!
//! It mirrors `stroke_pattern_native`'s word 95 one for one: one number at
//! both depths — in a byte file the FIRST word inside 45 (ahead of 95), in a
//! typed file the word right after the CMYK slot (ahead of 95), a CMYK
//! file's too — and the claim is checked both ways before any pixel decodes:
//! the word without such an effect is [`INVALID_WORD`], such an effect
//! without the word is [`WORD_REFUSAL`]. A document with none writes
//! nothing, so its bytes are the prior writer's. `DRDC_VERSION` stays 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::LayerStyle;
use crate::style_contour_json::carries_contour;

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 96;

/// A contour in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native layer style contours require DRDC 96";

/// The word in a file holding no contour.
pub(crate) const INVALID_WORD: &str = "invalid native contour word";

/// Whether `style` holds, enabled or not, an effect that writes a
/// contour-family key (module doc).
pub(crate) fn holds_contour(style: &LayerStyle) -> bool {
    style.effects.iter().any(carries_contour)
}

/// Whether some comp's recorded appearance holds such an effect (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_contour(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_contour(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_contour(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_contour))
        || comps_contour(comps)
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds such an effect.
pub(crate) fn styles_contour(doc: &DrDocument) -> bool {
    pending_contour(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree, or some comp.
pub(crate) fn styles_contour16(doc: &PrecisionDocument) -> bool {
    records_contour16(&doc.layers) || comps_contour(&doc.comps)
}

fn records_contour16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_contour(s)) || l.children().is_some_and(records_contour16)
    })
}
