//! Stacked layer-effect INSTANCES: the rules that let one
//! style hold several Drop Shadows, Inner Shadows, Strokes, Colour Overlays or
//! Gradient Overlays — the "+" in the Layer Style dialog — and the
//! `.rz` word that claims a document holds such a style. Rules and the word
//! only; no pixel math (the renderer draws every enabled effect in the
//! style's order, `style_render`).
//!
//! # The model
//!
//! `LayerStyle::effects` stays stable-sorted by kind. Within a kind, ARRAY
//! ORDER IS RENDER ORDER: a later instance draws on top of an earlier one
//! (`render_effects_with_job` stamps each contribution with its ordinal among
//! the ENABLED instances of its kind, `Contribution::instance`). A kind that
//! [`EffectKind::repeats`] holds at most [`MAX_INSTANCES`]; every other kind
//! holds at most one. Photoshop's own `<kind>Multi` list is the SAME
//! instances top first (`stacked-effects-557/take1`: list[0] draws on top),
//! so the PSD codec reverses it in one place (`psd_layer_effects`).
//!
//! Two Inside Strokes are two bands, and each REPLACES everything of the
//! layer under it in render order, over the one B0 captured beneath the
//! layer (take1's rule R; `style_composite::BandBackdrop`).
//!
//! # The JSON (`style_json`)
//!
//! STRICT: a second instance of a single kind keeps `effects[i].type
//! duplicates an earlier effect`; an 11th instance of a repeating kind is
//! `effects[i].type exceeds 10 instances of <type>`. LENIENT (the `.rz`
//! readers): the first instance of a single kind is kept, and the first ten
//! of a repeating one. The writer is unchanged: its sort is already stable,
//! so instances are written in render order.
//!
//! # The word
//!
//! [`WORD`] 89 is ONE number at both depths (`look_native` 71's precedent):
//! the FIRST word inside 45 in a byte file, and in a typed file the word after
//! the CMYK slot inside 46 (a CMYK document claims its stacked styles with
//! `cmyk_native`'s rung 88 instead). It has no table: the instances already
//! ride inside each record's style JSON, and the word is the claim that some
//! stored style — a layer's, or a comp's recorded appearance — is
//! [`is_stacked`]. Both directions are checked before any pixel decodes: the
//! word without such a style is [`INVALID_WORD`], such a style without the
//! word is [`WORD_REFUSAL`]. An older build refuses the word by its number,
//! which is what keeps it from opening a file whose styles it would silently
//! cut to one instance a kind. A document with no stacked style writes
//! nothing here, so its bytes stay exactly the prior writer's.

use crate::doc::DrDocument;
use crate::doc_comps::LayerComp;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{EffectKind, LayerStyle};

/// The most instances one repeating kind may hold.
pub const MAX_INSTANCES: usize = 10;

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 89;

/// A stacked style in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native stacked layer effects require DRDC 89";

/// The word in a file holding no stacked style.
pub(crate) const INVALID_WORD: &str = "invalid native stacked effects word";

/// Whether `style` is STACKED: some kind holds two or more instances,
/// enabled or not (a disabled instance is stored, so it needs the word too).
pub fn is_stacked(style: &LayerStyle) -> bool {
    EffectKind::ALL
        .into_iter()
        .any(|kind| instance_count(style, kind) >= 2)
}

/// How many instances of `kind` the style holds, enabled or not.
pub(crate) fn instance_count(style: &LayerStyle, kind: EffectKind) -> usize {
    style.effects.iter().filter(|e| e.kind() == kind).count()
}

/// What one more instance of `kind` would do to a parse that already holds
/// `held` of it: `None` admits it, `Some(sentence)` refuses it, the sentence
/// reading after the effect's `type` path (module doc).
pub(crate) fn refusal(kind: EffectKind, held: usize) -> Option<String> {
    if held == 0 {
        return None;
    }
    if !kind.repeats() {
        return Some("duplicates an earlier effect".to_string());
    }
    (held >= MAX_INSTANCES)
        .then(|| format!("exceeds {MAX_INSTANCES} instances of {}", kind.type_name()))
}

/// Whether some comp's recorded appearance holds a stacked style. A comp
/// stores its style as canonical JSON, read here as the `.rz` readers read
/// it (lenient); one that does not parse claims nothing.
pub(crate) fn comps_stacked(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| is_stacked(&style))
        })
    })
}

/// The byte word's predicate, asked by the writer to decide it and by the
/// reader to check it: some layer's STORED style (an adjustment layer's
/// ignored one included, as `style_knockout` reads it) or some comp's
/// recorded appearance is stacked.
pub(crate) fn styles_stacked(doc: &DrDocument) -> bool {
    doc.layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(is_stacked))
        || comps_stacked(&doc.comps)
}

/// The byte predicate over a reader's pending records and parsed comps,
/// before the document exists.
pub(crate) fn pending_stacked(layers: &[crate::doc::Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(is_stacked))
        || comps_stacked(comps)
}

/// The typed predicate: some record of the tree, or some comp, holds a
/// stacked style.
pub(crate) fn styles_stacked16(doc: &PrecisionDocument) -> bool {
    records_stacked16(&doc.layers) || comps_stacked(&doc.comps)
}

fn records_stacked16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| is_stacked(s)) || l.children().is_some_and(records_stacked16)
    })
}
