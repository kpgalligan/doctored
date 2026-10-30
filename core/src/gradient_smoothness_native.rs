//! The conditional GRADIENT SMOOTHNESS word: word 103, set when some
//! stored style holds a stroke or gradient overlay gradient whose Smoothness
//! is not 100 % (`GradientFill::smoothness`), or some Gradient Fill layer's
//! description does. Both ride inside JSON already stored (the style's
//! `smoothness` key, `style_json::gradient`; the description's
//! `gradient.smoothness`), so there is no table; the word exists so that a
//! build that knows word 102 but not Smoothness refuses the file by number
//! instead of drawing the gradient at 100 %.
//!
//! It mirrors `gradient_options_native`'s word 94: one number at both depths
//! — in a byte file a word inside 45 (after 105, ahead of 102), in a typed
//! file the word after the CMYK slot (after 105, ahead of 102), a CMYK file's
//! too — and the claim is checked both ways before any pixel decodes: the word
//! without such a gradient is [`INVALID_WORD`], such a gradient without the
//! word is [`WORD_REFUSAL`]. No file before this word holds a `smoothness`
//! key, so the second check refuses no prior file. A document with none
//! writes nothing, so its bytes are the prior writer's. `DRDC_VERSION` stays
//! 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, LayerStyle};

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 103;

/// A Smoothness below 100 % in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native gradient smoothness requires DRDC 103";

/// The word in a file holding no Smoothness below 100 %.
pub(crate) const INVALID_WORD: &str = "invalid native gradient smoothness word";

/// Whether `style` holds, on any effect enabled or not, a stroke gradient
/// (whatever the stroke's fill type, since the gradient rides in the JSON
/// either way) or a gradient overlay gradient whose Smoothness is not 1.
pub(crate) fn holds_smoothness(style: &LayerStyle) -> bool {
    style.effects.iter().any(|e| match e {
        Effect::Stroke(s) => s.gradient.smoothness != 1.0,
        Effect::GradientOverlay(g) => g.gradient.smoothness != 1.0,
        _ => false,
    })
}

/// Whether `text` is a Gradient Fill layer's description (`"type":"fill"`,
/// `"fill":"gradient"`) whose `gradient.smoothness` is present and not 1.
/// Anything else, malformed JSON included, claims nothing.
pub(crate) fn describes_smoothness(text: &str) -> bool {
    if !text.contains("smoothness") {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    value.get("type").and_then(serde_json::Value::as_str) == Some("fill")
        && value.get("fill").and_then(serde_json::Value::as_str) == Some("gradient")
        && value
            .get("gradient")
            .and_then(|g| g.get("smoothness"))
            .and_then(serde_json::Value::as_f64)
            .is_some_and(|s| s != 1.0)
}

/// Whether some comp's recorded appearance holds such a gradient (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_smoothness(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_smoothness(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps: a
/// stored style, a layer meta's fill description or a comp.
pub(crate) fn pending_smoothness(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers.iter().any(|l| {
        l.style.as_deref().is_some_and(holds_smoothness)
            || l.meta.as_deref().is_some_and(describes_smoothness)
    }) || comps_smoothness(comps)
}

/// The byte word's predicate (module doc).
pub(crate) fn styles_smoothness(doc: &DrDocument) -> bool {
    pending_smoothness(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree's style, some record's
/// artwork description, or some comp.
pub(crate) fn styles_smoothness16(doc: &PrecisionDocument) -> bool {
    records_smoothness16(&doc.layers)
        || (0..doc.layer_count())
            .any(|i| doc.artwork_description(i).is_some_and(describes_smoothness))
        || comps_smoothness(&doc.comps)
}

fn records_smoothness16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_smoothness(s))
            || l.children().is_some_and(records_smoothness16)
    })
}
