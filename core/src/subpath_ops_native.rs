//! The conditional PER-SUBPATH OPERATIONS word: word 116, set when some path
//! of the document, or some live vector mask, holds per-subpath operations
//! (`path_geometry::PathShape::ops`). The operations ride inside the `PATH`
//! table's JSON (`path_native`: each subpath's `op` key), so there is no
//! table; the word exists so that a build that knows word 115 but not the
//! key refuses the file by number instead of opening the outline under one
//! fill rule (its shape reader ignores a key it does not know).
//!
//! It mirrors `clipping_path_native`'s word 114 (no table, one number at
//! both depths): in a byte file the FIRST word inside 45, ahead of 115; in a
//! typed file the word right after the CMYK slot, ahead of 115, a colour
//! mode's file too. Checked both ways once the `PATH` table is read, before
//! any pixel decodes: the word with no shape holding operations is
//! [`INVALID_WORD`], a shape holding them without the word is
//! [`WORD_REFUSAL`]. No shape written before this word holds the key, so the
//! second check refuses no prior file. A document with no operations writes
//! nothing here, so its bytes are the prior writer's. `DRDC_VERSION` stays
//! 89.

use crate::doc_path::PathEntry;
use crate::path_native::TableMask;

/// The conditional `.doctored` word at both depths (module doc).
pub(crate) const WORD: u32 = 116;

/// A shape holding operations in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native subpath operations require DRDC 116";

/// The word in a file whose shapes hold no operations.
pub(crate) const INVALID_WORD: &str = "invalid native subpath operations word";

/// Whether some path or vector mask holds operations: the writer's
/// predicate.
pub(crate) fn carries(entries: &[PathEntry], masks: &[TableMask<'_>]) -> bool {
    entries.iter().any(|entry| entry.shape.ops.is_some())
        || masks.iter().any(|mask| mask.shape.ops.is_some())
}

/// The reader's two-way check of the word against the table's shapes.
pub(crate) fn check(
    word: bool,
    entries: &[PathEntry],
    masks: &[TableMask<'_>],
) -> Result<(), String> {
    match (word, carries(entries, masks)) {
        (true, false) => Err(INVALID_WORD.into()),
        (false, true) => Err(WORD_REFUSAL.into()),
        _ => Ok(()),
    }
}

/// The word's size when the document writes it, else 0.
pub(crate) fn header_bytes(entries: &[PathEntry], masks: &[TableMask<'_>]) -> u64 {
    if carries(entries, masks) {
        4
    } else {
        0
    }
}
