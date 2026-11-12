//! The conditional CLIPPING PATH word: word 114, set when some path of the
//! document is its clipping path (`doc_path::ClippingPath`). The designation
//! rides inside the `PATH` table's entry JSON (`path_native`: the entry's
//! `clipping` key, its tail as hex), so there is no table; the word exists
//! so that a build that knows word 113 but not the key refuses the file by
//! number instead of opening it with the designation silently dropped (its
//! entry reader ignores a key it does not know).
//!
//! It mirrors `gradient_map_method_native`'s word 111 (no table, one number
//! at both depths): in a byte file the FIRST word inside 45, ahead of 113;
//! in a typed file the word right after the CMYK slot, ahead of 113, a
//! colour mode's file too. Checked both ways once the `PATH` table is read,
//! before any pixel decodes: the word with no clipping entry is
//! [`INVALID_WORD`], a clipping entry without the word is [`WORD_REFUSAL`].
//! No entry written before this word holds the key, so the second check
//! refuses no prior file. A document with no clipping path writes nothing
//! here, so its bytes are the prior writer's. `DRDC_VERSION` stays 89.

use crate::doc_path::PathEntry;

/// The conditional `.doctored` word at both depths (module doc).
pub(crate) const WORD: u32 = 114;

/// A clipping path in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native clipping path requires DRDC 114";

/// The word in a file holding no clipping path.
pub(crate) const INVALID_WORD: &str = "invalid native clipping path word";

/// Whether some entry is the clipping path: the writer's predicate.
pub(crate) fn carries(entries: &[PathEntry]) -> bool {
    entries.iter().any(|entry| entry.clipping.is_some())
}

/// The reader's two-way check of the word against the table's entries.
pub(crate) fn check(word: bool, entries: &[PathEntry]) -> Result<(), String> {
    match (word, carries(entries)) {
        (true, false) => Err(INVALID_WORD.into()),
        (false, true) => Err(WORD_REFUSAL.into()),
        _ => Ok(()),
    }
}

/// The word's size when the document writes it, else 0.
pub(crate) fn header_bytes(entries: &[PathEntry]) -> u64 {
    if carries(entries) {
        4
    } else {
        0
    }
}
