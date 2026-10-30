//! Conditional words 63 / 64: a `.rz` document holding a
//! MAP-FORM Curves adjustment (`adjust_curves`' `rgb_map`, `r_map`, `g_map`
//! or `b_map` — a pencil-drawn curve), at both depths.
//!
//! Byte word **63** is the second word inside the identity wrapper 45, after
//! the legacy word 65 (`native_sources`) and ahead of `levels_native`'s 61
//! (`45 -> (65) -> (63) -> (61) -> (59) -> …`), and typed word **64** the
//! second inside 46 (`46 -> (66) -> (64) -> (62) -> (60) -> …`).
//! Neither word has a table — the maps ride inside the adjustment meta /
//! description each record already carries, exactly as 61/62's Levels keys
//! do. Both 12E-1 invariants hold: 45/46 is still the first word of every
//! file and `LIDS` the last table.
//!
//! A word is written iff [`byte_carries`] / [`precision_carries`]: some
//! curves meta's `params` holds any `*_map` key (KEY PRESENCE, whatever the
//! value). A map-free document — every document an earlier build could
//! write — writes neither word, so its bytes are exactly the prior writer's.
//!
//! The word exists because the meta alone cannot protect itself: an older
//! build reads a curves meta with only map keys as the IDENTITY curve (it
//! ignores unknown params keys), so it would open the file and render and
//! resave a different picture with no word of warning. With the word an
//! older byte build refuses the file by name (`unsupported DRDC version
//! 63`), and an older typed build with its generic sentence. This build
//! checks the claim in BOTH directions before a single PNG decodes: the
//! word with no such meta is `invalid native curves word`, and such a meta
//! with no word is `native curve maps require DRDC 63` (64 at high depth).
//! The predicates are `levels_native`'s shared ones.
//!
//! `curves_native` is the conditional `.rz` word **63** (byte, the second word inside 45: `45 → (65)
//! → (63) → (61) → …`) / **64** (typed, second inside 46), with NO table — written iff some curves meta's
//! params hold a `*_map` key, checked both ways before any PNG decodes (`invalid native curves
//! word` / `native curve maps require DRDC 63|64`); its predicates are `levels_native`'s shared
//! `params_carry` / `byte_any` / `precision_any`, and a map-free document writes the prior writer's
//! bytes.

use crate::adjust_curves::MAP_KEYS;
use crate::doc::Layer;
use crate::levels_native::{byte_any, params_carry, precision_any};
use crate::precision_document::PrecisionDocument;

pub(crate) const BYTE_VERSION: u32 = 63;
pub(crate) const PRECISION_VERSION: u32 = 64;

/// The predicate both words share: `meta` is a `curves` adjustment whose
/// `params` holds any map key.
pub(crate) fn meta_carries(meta: &str) -> bool {
    params_carry(meta, "curves", &MAP_KEYS)
}

/// Byte word 63's predicate, asked by the writer to decide the word and by
/// the reader to check it.
pub(crate) fn byte_carries(layers: &[Layer]) -> bool {
    byte_any(layers, meta_carries)
}

/// Typed word 64's predicate over the document's records, groups included.
pub(crate) fn precision_carries(document: &PrecisionDocument) -> bool {
    precision_any(document, meta_carries)
}

/// The header bytes word 64 adds to a typed save plan: the version word
/// alone, since there is no table.
pub(crate) fn header_bytes(carries: bool) -> u64 {
    if carries {
        4
    } else {
        0
    }
}
