//! The conditional PATTERN STROKE word: word 95, set when some
//! stored style holds a stroke that carries a pattern fill — its Fill Type is
//! Pattern, or its `pattern_fill` is not the default
//! (`style_pattern_fill::stroke_carries_pattern`). The fill rides inside each
//! style's JSON (the stroke's `pattern_fill`), so there is no table; the word
//! exists so that a build that knows word 94 but not the Pattern fill refuses
//! the file by number instead of reading the stroke's unknown `"pattern"`
//! Fill Type as Color.
//!
//! It mirrors `gradient_options_native`'s word 94 one for one: one number at
//! both depths — in a byte file the FIRST word inside 45 (ahead of 94), in a
//! typed file the word right after the CMYK slot (ahead of 94), a CMYK
//! file's too — and the claim is checked both ways before any pixel decodes:
//! the word without such a stroke is [`INVALID_WORD`], such a stroke without
//! the word is [`WORD_REFUSAL`]. A document with none writes nothing, so its
//! bytes are the prior writer's. `DRDC_VERSION` stays 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, LayerStyle};
use crate::style_pattern_fill::stroke_carries_pattern;

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 95;

/// A pattern stroke in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native pattern strokes require DRDC 95";

/// The word in a file holding no pattern stroke.
pub(crate) const INVALID_WORD: &str = "invalid native pattern stroke word";

/// Whether `style` holds, enabled or not, a stroke that carries a pattern
/// fill (module doc).
pub(crate) fn holds_stroke_pattern(style: &LayerStyle) -> bool {
    style
        .effects
        .iter()
        .any(|e| matches!(e, Effect::Stroke(s) if stroke_carries_pattern(s)))
}

/// Whether some comp's recorded appearance holds such a stroke (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_stroke_pattern(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_stroke_pattern(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_stroke_pattern(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_stroke_pattern))
        || comps_stroke_pattern(comps)
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds such a stroke.
pub(crate) fn styles_stroke_pattern(doc: &DrDocument) -> bool {
    pending_stroke_pattern(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree, or some comp.
pub(crate) fn styles_stroke_pattern16(doc: &PrecisionDocument) -> bool {
    records_stroke_pattern16(&doc.layers) || comps_stroke_pattern(&doc.comps)
}

fn records_stroke_pattern16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_stroke_pattern(s))
            || l.children().is_some_and(records_stroke_pattern16)
    })
}
