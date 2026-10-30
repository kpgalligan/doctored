//! The conditional GRADIENT MAP METHOD word: word 111, set when some
//! Gradient Map adjustment's description holds a `method` key, whatever its
//! value (`"classic"` or `"perceptual"`, `adjust_map::MapMethod`: the Classic
//! ramp with its Smoothness, its bracket bound and its stop midpoints, in
//! sRGB or over Oklab stops). The key rides inside the
//! description already stored, so there is no table; the word exists so that
//! a build that knows word 110 but not the method refuses the file by number
//! instead of drawing the map linear with its midpoints ignored.
//!
//! It mirrors `fill_align_native`'s word 105 in placement, one ahead of it
//! (no table, one number at both depths: in a byte file the FIRST word inside
//! 45, ahead of 105; in a typed file the word right after the CMYK slot,
//! ahead of 105, a CMYK file's too) and `levels_native`'s table-less
//! adjustment words in its predicate (`levels_native::params_carry`, KEY
//! PRESENCE), checked both ways before any pixel decodes: the word without
//! such a description is [`INVALID_WORD`], such a description without the
//! word is [`WORD_REFUSAL`]. No description written before this word holds a
//! `method` key, so the second check refuses no prior file. A legacy map (no
//! `method`) writes nothing, so its bytes are the prior writer's.
//! `DRDC_VERSION` stays 89.

use crate::doc::Layer;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 111;

/// A Gradient Map method in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native gradient map method requires DRDC 111";

/// The word in a file holding no Gradient Map method.
pub(crate) const INVALID_WORD: &str = "invalid native gradient map method word";

/// Whether `meta` is a `gradient_map` adjustment (byte or typed form, RGB or
/// CMYK) whose params hold a `method` key.
pub(crate) fn describes_method(meta: &str) -> bool {
    crate::levels_native::params_carry(meta, "gradient_map", &["method"])
}

/// The byte predicate over a document's (or a reader's pending) layers.
pub(crate) fn byte_carries(layers: &[Layer]) -> bool {
    crate::levels_native::byte_any(layers, describes_method)
}

/// The typed predicate: some adjustment record of the tree, groups walked,
/// RGB and CMYK records alike.
pub(crate) fn precision_carries(doc: &PrecisionDocument) -> bool {
    fn walk(layers: &[PrecisionDocumentLayer]) -> bool {
        layers.iter().any(|layer| match &layer.content {
            Content::Adjustment(state) => describes_method(state.description()),
            Content::CmykAdjustment(state) => describes_method(state.description()),
            Content::Group(children) | Content::PassThrough(children) => walk(children),
            Content::Raster { .. } | Content::CmykRaster { .. } => false,
        })
    }
    walk(&doc.layers)
}
