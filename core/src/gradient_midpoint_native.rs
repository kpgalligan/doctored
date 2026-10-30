//! The conditional GRADIENT MIDPOINT word: word 104, set when some
//! stored style holds a stroke or gradient overlay gradient with a stop whose
//! `midpoint` or `opacity_midpoint` is not 50 % (`GradientStop::midpoint`),
//! or some Gradient Fill layer's description does. Both ride inside JSON
//! already stored (the style's stop keys, `style_json::gradient`; the
//! description's `gradient.stops[*]`), so there is no table; the word exists
//! so that a build that knows word 103 but not midpoints refuses the file by
//! number instead of drawing every midpoint at 50 %.
//!
//! It mirrors `gradient_smoothness_native`'s word 103: one number at both
//! depths — in a byte file a word inside 45 (after 105, ahead of 103), in a
//! typed file the word after the CMYK slot (after 105, ahead of 103), a CMYK
//! file's too — and the claim is checked both ways before any pixel decodes:
//! the word without such a gradient is [`INVALID_WORD`], such a gradient
//! without the word is [`WORD_REFUSAL`]. No file before this word holds a
//! `midpoint` or `opacity_midpoint` key, so the second check refuses no
//! prior file. A document with none writes nothing, so its bytes are the
//! prior writer's. `DRDC_VERSION` stays 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, GradientFill, LayerStyle};

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 104;

/// A midpoint other than 50 % in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native gradient midpoints require DRDC 104";

/// The word in a file holding no midpoint other than 50 %.
pub(crate) const INVALID_WORD: &str = "invalid native gradient midpoint word";

/// Whether `fill` has a stop whose midpoint or opacity midpoint is not 0.5.
fn fill_midpoints(fill: &GradientFill) -> bool {
    fill.stops
        .iter()
        .any(|s| s.midpoint != 0.5 || s.opacity_midpoint != 0.5)
}

/// Whether `style` holds, on any effect enabled or not, a stroke gradient
/// (whatever the stroke's fill type, since the gradient rides in the JSON
/// either way) or a gradient overlay gradient with a stop whose midpoint or
/// opacity midpoint is not 0.5.
pub(crate) fn holds_midpoints(style: &LayerStyle) -> bool {
    style.effects.iter().any(|e| match e {
        Effect::Stroke(s) => fill_midpoints(&s.gradient),
        Effect::GradientOverlay(g) => fill_midpoints(&g.gradient),
        _ => false,
    })
}

/// Whether `text` is a Gradient Fill layer's description (`"type":"fill"`,
/// `"fill":"gradient"`) with a `gradient.stops[*].midpoint` or
/// `opacity_midpoint` present and not 0.5. Anything else, malformed JSON
/// included, claims nothing.
pub(crate) fn describes_midpoints(text: &str) -> bool {
    if !text.contains("midpoint") {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let off_half = |stop: &serde_json::Value, key: &str| {
        stop.get(key)
            .and_then(serde_json::Value::as_f64)
            .is_some_and(|m| m != 0.5)
    };
    value.get("type").and_then(serde_json::Value::as_str) == Some("fill")
        && value.get("fill").and_then(serde_json::Value::as_str) == Some("gradient")
        && value
            .get("gradient")
            .and_then(|g| g.get("stops"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|stops| {
                stops
                    .iter()
                    .any(|s| off_half(s, "midpoint") || off_half(s, "opacity_midpoint"))
            })
}

/// Whether some comp's recorded appearance holds such a gradient (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_midpoints(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_midpoints(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps: a
/// stored style, a layer meta's fill description or a comp.
pub(crate) fn pending_midpoints(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers.iter().any(|l| {
        l.style.as_deref().is_some_and(holds_midpoints)
            || l.meta.as_deref().is_some_and(describes_midpoints)
    }) || comps_midpoints(comps)
}

/// The byte word's predicate (module doc).
pub(crate) fn styles_midpoints(doc: &DrDocument) -> bool {
    pending_midpoints(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree's style, some record's
/// artwork description, or some comp.
pub(crate) fn styles_midpoints16(doc: &PrecisionDocument) -> bool {
    records_midpoints16(&doc.layers)
        || (0..doc.layer_count())
            .any(|i| doc.artwork_description(i).is_some_and(describes_midpoints))
        || comps_midpoints(&doc.comps)
}

fn records_midpoints16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_midpoints(s))
            || l.children().is_some_and(records_midpoints16)
    })
}
