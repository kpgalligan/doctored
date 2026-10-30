//! Conditional words 61 / 62: a `.rz` document holding a
//! Levels adjustment that uses its OUTPUT range or a per-channel stage
//! (`adjust_levels`), at both depths.
//!
//! Byte word **61** is the third word inside the identity wrapper 45,
//! after the legacy word 65 (`native_sources`) and `curves_native`'s 63 and ahead of
//! every other conditional word (`45 -> (65) -> (63) -> (61) -> (59) -> (56)
//! -> …`), and typed word **62** the third inside 46 (`46 -> (66) -> (64) ->
//! (62) -> (60) -> (58) -> …`). Neither
//! word has a table — the keys ride inside the adjustment meta /
//! description each record already carries, exactly as word 49's
//! knockout keys ride inside the style JSON (`style_knockout`, D10). Both
//! 12E-1 invariants hold: 45/46 is still the first word of every file and
//! `LIDS` the last table.
//!
//! A word is written iff [`byte_carries`] / [`precision_carries`]: some
//! levels meta's `params` holds any of `output_black`, `output_white`, `r`,
//! `g` or `b` (KEY PRESENCE, whatever the value). A master-only Levels
//! document — every document an earlier build could write — writes neither
//! word, so its bytes are exactly the prior writer's.
//!
//! The word exists because the meta alone cannot protect itself: an older
//! build ignores unknown params keys, so it would open the file, drop the
//! output range and every channel stage, and render (and resave) a
//! different picture with no word of warning. With the word an older byte
//! build refuses the file by name (`unsupported DRDC version 61`), and an
//! older typed build with its generic "not a supported … high-depth DRDC
//! document" sentence. This build checks the claim in BOTH directions
//! before a single PNG decodes: the word with no such meta is
//! `invalid native levels word`, and such a meta with no word is
//! `native per-channel levels require DRDC 61` (62 at high depth).
//!
//! `levels_native` is the conditional `.rz` word **61** (byte, the next word inside 45 after
//! `curves_native`'s 63: `45 → (65) → (63) → (61) → (59) → …`) / **62** (typed, after 64 inside 46), with
//! NO table — written iff some levels meta's params hold `output_black`, `output_white`, `r`, `g`
//! or `b` (key presence), checked both ways before any PNG decodes (`invalid native levels word` /
//! `native per-channel levels require DRDC 61|62`).

use crate::doc::Layer;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};

pub(crate) const BYTE_VERSION: u32 = 61;
pub(crate) const PRECISION_VERSION: u32 = 62;

/// The five keys whose presence a master-only build cannot honour.
const CARRIED_KEYS: [&str; 5] = ["output_black", "output_white", "r", "g", "b"];

/// The predicate both words share: `meta` is a `levels` adjustment
/// (`type` adjust, `op` levels, byte or typed form) whose `params` holds any
/// of [`CARRIED_KEYS`].
pub(crate) fn meta_carries(meta: &str) -> bool {
    params_carry(meta, "levels", &CARRIED_KEYS)
}

/// Byte word 61's predicate, asked by the writer to decide the word and by
/// the reader to check it. Reads the stored meta of every layer, as the
/// file stores it.
pub(crate) fn byte_carries(layers: &[Layer]) -> bool {
    byte_any(layers, meta_carries)
}

/// Typed word 62's predicate over the document's records, walking into
/// every group: an adjustment nested in a group carries the word too.
pub(crate) fn precision_carries(document: &PrecisionDocument) -> bool {
    precision_any(document, meta_carries)
}

/// The shape every table-less adjustment word asks (this module's 61/62 and
/// `curves_native`'s 63/64): `meta` is an adjustment (`type` adjust, byte
/// or typed form) of `op` whose `params` holds any of `keys` (KEY PRESENCE,
/// whatever the value). A meta that does not mention `"{op}"` at all is
/// answered without parsing, so a 16 MiB lookup-table meta costs a
/// substring scan rather than a JSON parse on every save and open.
pub(crate) fn params_carry(meta: &str, op: &str, keys: &[&str]) -> bool {
    if !meta.contains(&format!("\"{op}\"")) {
        return false;
    }
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str(meta) else {
        return false;
    };
    if root.get("type").and_then(|v| v.as_str()) != Some("adjust")
        || root.get("op").and_then(|v| v.as_str()) != Some(op)
    {
        return false;
    }
    root.get("params")
        .and_then(|p| p.as_object())
        .is_some_and(|params| keys.iter().any(|key| params.contains_key(*key)))
}

/// A byte word's predicate over the stored meta of every layer.
pub(crate) fn byte_any(layers: &[Layer], carries: fn(&str) -> bool) -> bool {
    layers
        .iter()
        .any(|layer| layer.meta.as_deref().is_some_and(carries))
}

/// A typed word's predicate over the document's records, walking into
/// every group.
pub(crate) fn precision_any(document: &PrecisionDocument, carries: fn(&str) -> bool) -> bool {
    fn walk(layers: &[PrecisionDocumentLayer], carries: fn(&str) -> bool) -> bool {
        layers.iter().any(|layer| match &layer.content {
            // A CMYK record never carries words 62, 64 or 71 (`cmyk_native`).
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => false,
            Content::Adjustment(state) => carries(state.description()),
            Content::Group(children) | Content::PassThrough(children) => walk(children, carries),
            Content::Raster { .. } => false,
        })
    }
    walk(&document.layers, carries)
}

/// The header bytes word 62 adds to a typed save plan: the version word
/// alone, since there is no table.
pub(crate) fn header_bytes(carries: bool) -> u64 {
    if carries {
        4
    } else {
        0
    }
}
