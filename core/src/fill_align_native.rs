//! The conditional GRADIENT FILL ALIGNMENT word: word 105, set when
//! some Gradient Fill layer's description holds `gradient.align_with_layer`
//! true (Align with Layer: the gradient spans the box `fill_align_box`
//! computes, not the canvas). The flag rides inside the description already
//! stored, so there is no table; the word exists so that a build that knows
//! word 103 but not alignment refuses the file by number instead of spanning
//! the gradient over the canvas.
//!
//! It mirrors `large_description_native`'s word 102 in placement (no table,
//! one number at both depths: in a byte file the FIRST word inside 45, ahead
//! of 103; in a typed file the word right after the CMYK slot, ahead of 103,
//! a CMYK file's too) and `gradient_options_native`'s word 94 in its claim,
//! checked both ways before any pixel decodes: the word without such a
//! description is [`INVALID_WORD`], such a description without the word is
//! [`WORD_REFUSAL`]. Every description written before this word spells
//! `align_with_layer` false (the host's one spelling, and the PSD import's),
//! so the second check refuses no prior file. A layer STYLE's
//! `align_with_layer` is a different, older key and claims nothing. A document
//! with none writes nothing, so its bytes are the prior writer's.
//! `DRDC_VERSION` stays 89.

use crate::doc::{DrDocument, Layer};
use crate::precision_document::PrecisionDocument;

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 105;

/// An aligned gradient fill in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native gradient fill alignment requires DRDC 105";

/// The word in a file holding no aligned gradient fill.
pub(crate) const INVALID_WORD: &str = "invalid native gradient fill alignment word";

/// Whether `text` is a Gradient Fill layer's description (`"type":"fill"`,
/// `"fill":"gradient"`) whose `gradient.align_with_layer` is `true`. Anything
/// else, malformed JSON and an absent key included, claims nothing.
pub(crate) fn describes_align(text: &str) -> bool {
    if !text.contains("align_with_layer") {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    value.get("type").and_then(serde_json::Value::as_str) == Some("fill")
        && value.get("fill").and_then(serde_json::Value::as_str) == Some("gradient")
        && value
            .get("gradient")
            .and_then(|g| g.get("align_with_layer"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
}

/// The byte predicate over a reader's pending records: some layer meta is
/// such a description.
pub(crate) fn pending_align(layers: &[Layer]) -> bool {
    layers
        .iter()
        .any(|l| l.meta.as_deref().is_some_and(describes_align))
}

/// The byte word's predicate (module doc).
pub(crate) fn descriptions_align(doc: &DrDocument) -> bool {
    pending_align(&doc.layers)
}

/// The typed word's predicate: some record's artwork description.
pub(crate) fn descriptions_align16(doc: &PrecisionDocument) -> bool {
    (0..doc.layer_count()).any(|i| doc.artwork_description(i).is_some_and(describes_align))
}
