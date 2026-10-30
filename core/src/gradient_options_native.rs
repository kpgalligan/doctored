//! The conditional GRADIENT OPTIONS word: word 94, set when
//! some stored style holds a stroke gradient whose style is Shape Burst, or a
//! stroke or gradient overlay gradient with Dither on. Both ride inside each
//! style's JSON (`style_json::effect_gradient`), so there is no table; the
//! word exists so that a build that knows word 93 but not these options
//! refuses the file by number instead of drawing a Shape Burst stroke as a
//! linear one or dropping Dither.
//!
//! It mirrors `pattern_angle_native`'s word 93 one for one: one number at
//! both depths — in a byte file the FIRST word inside 45 (ahead of 93), in a
//! typed file the word right after the CMYK slot (ahead of 93), a CMYK
//! file's too — and the claim is checked both ways before any pixel decodes:
//! the word without such a gradient is [`INVALID_WORD`], such a gradient
//! without the word is [`WORD_REFUSAL`]. A document with none writes
//! nothing, so its bytes are the prior writer's. `DRDC_VERSION` stays 89.

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, GradientStyle, LayerStyle};

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 94;

/// A Shape Burst or dithered gradient in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native shape burst and dithered gradients require DRDC 94";

/// The word in a file holding no Shape Burst or dithered gradient.
pub(crate) const INVALID_WORD: &str = "invalid native gradient options word";

/// Whether `style` holds, on any effect enabled or not, a stroke gradient
/// whose style is Shape Burst, or a stroke or gradient overlay gradient with
/// Dither on — whatever the stroke's fill type, since the gradient rides in
/// the JSON either way.
pub(crate) fn holds_options(style: &LayerStyle) -> bool {
    style.effects.iter().any(|e| match e {
        Effect::Stroke(s) => s.gradient.style == GradientStyle::ShapeBurst || s.gradient.dither,
        Effect::GradientOverlay(g) => g.gradient.dither,
        _ => false,
    })
}

/// Whether some comp's recorded appearance holds such a gradient (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_options(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_options(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_options(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_options))
        || comps_options(comps)
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds such a gradient.
pub(crate) fn styles_options(doc: &DrDocument) -> bool {
    pending_options(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree, or some comp.
pub(crate) fn styles_options16(doc: &PrecisionDocument) -> bool {
    records_options16(&doc.layers) || comps_options(&doc.comps)
}

fn records_options16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_options(s)) || l.children().is_some_and(records_options16)
    })
}
