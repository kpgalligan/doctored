//! Conditional word 71: a `.rz` document holding a Color Lookup
//! ABSTRACT look, whose params keep the abstract profile's bytes under
//! `abstract_profile` (`icc_lookup::ABSTRACT_KEY`), at both depths.
//!
//! Word **71** is ONE number at both depths and never a first word: it is
//! the first word inside the identity wrapper 45 at 8 bits (`45 -> (71) ->
//! (69) -> (65) -> (63) -> (61) -> …`) and inside 46 at high depth (`46 ->
//! (71) -> (70) -> (68) -> (67) -> (66) -> (64) -> …`), so the wrapper alone says which
//! depth the file is. It has no table — the profile rides inside the
//! adjustment meta / description each record already carries, exactly as
//! `curves_native`'s maps do. Both 12E-1 invariants hold: 45/46 is still the
//! first word of every file and `LIDS` the last table.
//!
//! A word is written iff [`byte_carries`] / [`precision_carries`]: some
//! `color_lookup` meta's `params` holds `abstract_profile` (KEY PRESENCE,
//! `icc_lookup::is_abstract_look`). A look-free document — every document an
//! earlier build could write — writes no word, so its bytes are exactly the
//! prior writer's.
//!
//! The word exists because the meta alone cannot protect itself: an older
//! build ignores the unknown key, so it would open the file and keep a table
//! baked for one profile through every later Assign or Convert, and resave
//! the file without the profile. With the word an older byte build refuses
//! the file by name (`unsupported DRDC version 71`), and an older typed
//! build with its generic sentence. This build checks the claim in BOTH
//! directions before a single PNG decodes: the word with no such meta is
//! `invalid native look word`, and such a meta with no word is `Color
//! Lookup abstract looks require DRDC 71`. The predicates are
//! `levels_native`'s shared ones.

use crate::doc::Layer;
use crate::levels_native::{byte_any, precision_any};
use crate::precision_document::PrecisionDocument;

/// The word, at both depths.
pub(crate) const VERSION: u32 = 71;

/// The predicate both depths share: `meta` is a `color_lookup` adjustment
/// whose `params` holds `abstract_profile`.
pub(crate) fn meta_carries(meta: &str) -> bool {
    crate::icc_lookup::is_abstract_look(meta)
}

/// The byte word's predicate, asked by the writer to decide the word and by
/// the reader to check it.
pub(crate) fn byte_carries(layers: &[Layer]) -> bool {
    byte_any(layers, meta_carries)
}

/// The typed word's predicate over the document's records, groups included.
pub(crate) fn precision_carries(document: &PrecisionDocument) -> bool {
    precision_any(document, meta_carries)
}

/// The header bytes the word adds to a save plan: the version word alone,
/// since there is no table.
pub(crate) fn header_bytes(carries: bool) -> u64 {
    if carries {
        4
    } else {
        0
    }
}
