//! The conditional LARGE DESCRIPTION word (pattern fill layers): word
//! 102, set when some artwork description is longer than
//! `artwork_document::MAX_DESCRIPTION` (64 KiB) — a pattern fill layer
//! carrying its custom tile inline, as a Pattern Overlay's style does. Such a
//! description is admitted up to `MAX_LARGE_DESCRIPTION`, the 16 MiB meta cap
//! (`drdc::MAX_DRDC_META_LEN`). The word exists so that a build that caps
//! descriptions at 64 KiB refuses the file by number instead of with its
//! opaque size sentence.
//!
//! It mirrors `blend_if_channels_native`'s word 101: one number at both
//! depths, no table — in a byte file the FIRST word inside 45 (ahead of 101),
//! in a typed file the word right after the CMYK slot (ahead of 101), a CMYK
//! file's too. A document with none writes nothing, so its bytes are the prior
//! writer's. `DRDC_VERSION` stays 89.
//!
//! The claim is checked differently at the two depths. A TYPED file's `ADSC`
//! entries are capped at 64 KiB without the word (the entry keeps the reader's
//! size sentence) and at the meta cap with it, and the word without a large
//! entry is [`INVALID_WORD`]. A BYTE file's layer meta has always been opaque
//! up to the meta cap, so only the word without a large description is
//! refused.

use crate::artwork_document::{validate, MAX_DESCRIPTION};
use crate::doc::{DrDocument, Layer};
use crate::precision_document::PrecisionDocument;

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 102;

/// The word in a file holding no description over 64 KiB.
pub(crate) const INVALID_WORD: &str = "invalid native large artwork description word";

/// Whether `text` needs the word: longer than the word-less slot.
pub(crate) fn is_large(text: &str) -> bool {
    text.len() > MAX_DESCRIPTION
}

/// The byte predicate over a reader's pending records: some layer meta is a
/// valid artwork description and [`is_large`]. Other metas (adjustment
/// parameters, which may be longer) claim nothing.
pub(crate) fn pending_large(layers: &[Layer]) -> bool {
    layers.iter().any(|l| {
        l.meta
            .as_deref()
            .is_some_and(|text| is_large(text) && validate(text).is_ok())
    })
}

/// The byte word's predicate (module doc).
pub(crate) fn descriptions_large(doc: &DrDocument) -> bool {
    pending_large(&doc.layers)
}

/// The typed word's predicate: some record's artwork description
/// [`is_large`].
pub(crate) fn descriptions_large16(doc: &PrecisionDocument) -> bool {
    (0..doc.layer_count()).any(|i| doc.artwork_description(i).is_some_and(is_large))
}
