//! WHERE a newly created entry lands in the stack, at BOTH depths — the one
//! slot rule, written once here and read by the byte insert below and by
//! `precision_insert`'s typed one.
//!
//! # The rule
//!
//! An insertion names an ANCHOR entry and a [`InsertPosition`]. Both depths
//! store their stack as a bottom-first POSTORDER list with a per-entry LEVEL
//! (the byte `Layer::depth`, the typed `FlatRecord::depth`), so the rule is
//! expressed once over a plain `&[usize]` of levels:
//!
//! ```text
//! Above      at = subtree(anchor).end      level = level[anchor]
//! Top        at = levels.len()             level = 0
//! IntoGroup  at = anchor                   level = level[anchor] + 1
//! ```
//!
//! * **Above** is the position every existing insert already used
//!   (`DrDocument::adding_image_layer`): a subtree ENDS with its own record,
//!   so `anchor + 1` is immediately above the anchor's WHOLE subtree at the
//!   anchor's own level — "above a group" means above the group, not inside
//!   it.
//! * **Top** is the top of the ROOT level, whatever the anchor is. The anchor
//!   is still range-checked, so a caller cannot smuggle a bad index through.
//! * **IntoGroup** requires the anchor to BE a group and lands the new entry
//!   as that group's TOP CHILD: a group's children sit immediately below its
//!   own record, so the group's own index is exactly the slot above all of
//!   them, one level deeper.
//!
//! # The clip run, and why the new entry JOINS it
//!
//! [`InsertSlot::clipped`] is `true` when the NEXT SIBLING at the slot's
//! level — the first record at or above `at` whose level equals `level`,
//! stopping at anything shallower, which is the end of the level — is itself
//! clipped. The new record then takes that flag too, so it JOINS the clip run
//! rather than splitting it.
//!
//! It is the only choice that leaves the neighbours alone. A clipped run is
//! purely positional at both depths (see `Layer::clipped`): its BASE is the
//! first unclipped sibling beneath it. An unclipped record dropped into the
//! middle of a run would become the new base of everything above it,
//! silently re-basing entries the caller never named; joining the run
//! re-bases nothing.
//!
//! # Refusals and exemptions
//!
//! An out-of-range anchor, an `IntoGroup` whose anchor is not a group, and a
//! level past the depth's own ceiling are each refused by name. Insertion is
//! STRUCTURE, so it is lock-EXEMPT — `doc_lock`'s list ("locks never block
//! delete, duplicate, reorder, group/ungroup, rename, opacity, blend mode,
//! visibility or style") is the same reason an insert is not gated: it writes
//! no existing layer's pixels, alpha or position.
//!
//! **Phase 12E-2b** makes `InsertPosition` `serde::Deserialize` (snake_case, so the JSON spellings
//! and the header's code name the same three slots), adds the byte `inserting_layer` (the twin of
//! `inserting_record`: set the level and clip flag, splice, `doc_group::validated`) with
//! `adding_layer_at` expressed over it (12E-2b also added an `adding_image_layer_at`; it never
//! gained a caller, an export or a test and **12E-2c DELETED it** — do not re-add it), and adds the
//! typed `add_layer_at` — a transparent canvas-sized leaf whose storage follows the DOCUMENT
//! through `precision_merge::merged_leaf`'s own switch (u16 zeros, or the filled f32 plane through
//! `raster_float` at 32-bit float), reached as `StructureEdit::AddLayer` and admitted as
//! family `layer_add` targeting `document` in BOTH capability tables and in `float_document`'s
//! `ADMITTED`.

use crate::doc::{DrDocument, Layer, LayerKind};
use crate::doc_group::{self, MAX_GROUP_DEPTH};

/// Where a newly created entry lands, relative to an ANCHOR entry. The raw
/// values are the header's `DrInsertPosition`; the JSON spellings a request
/// carries are `"above"`, `"top"` and `"into_group"`, so a typed request and
/// the FFI code name the same three slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertPosition {
    /// Immediately above the anchor's whole subtree, at the anchor's level.
    Above = 0,
    /// The top of the ROOT level.
    Top = 1,
    /// The top CHILD of the anchor, which must be a group.
    IntoGroup = 2,
}

impl InsertPosition {
    /// The FFI's `uint32_t` position code; an unknown code is refused rather
    /// than defaulted, so a newer host's position can never be silently read
    /// as `Above`.
    pub fn from_code(code: u32) -> Result<Self, String> {
        match code {
            0 => Ok(Self::Above),
            1 => Ok(Self::Top),
            2 => Ok(Self::IntoGroup),
            _ => Err("unsupported layer insert position".into()),
        }
    }
}

/// The resolved slot: the index the new record is inserted at, the level it
/// carries there, and whether it joins a clip run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertSlot {
    /// The index the record is spliced in at.
    pub at: usize,
    /// The nesting level the record carries (0 at the top level).
    pub level: usize,
    /// Whether the record takes the `clipped` flag, because the next sibling
    /// at its level carries one.
    pub clipped: bool,
}

/// THE slot rule, over a bottom-first postorder level list. `is_group` and
/// `is_clipped` answer for an index into that same list; `max_level` is the
/// depth's own nesting ceiling.
pub(crate) fn slot(
    levels: &[usize],
    is_group: &dyn Fn(usize) -> bool,
    is_clipped: &dyn Fn(usize) -> bool,
    anchor: usize,
    position: InsertPosition,
    max_level: usize,
) -> Result<InsertSlot, String> {
    let Some(&anchor_level) = levels.get(anchor) else {
        return Err("layer insert anchor is out of range".into());
    };
    let (at, level) = match position {
        // A subtree ENDS with its own record (`doc_group::subtree`), so the
        // slot immediately above the anchor's whole subtree is `anchor + 1`.
        InsertPosition::Above => (anchor + 1, anchor_level),
        InsertPosition::Top => (levels.len(), 0),
        InsertPosition::IntoGroup => {
            if !is_group(anchor) {
                return Err("into_group requires a group layer".into());
            }
            // A group's children sit immediately below its own record, so the
            // group's index is the slot above all of them: the TOP child.
            (anchor, anchor_level + 1)
        }
    };
    if level > max_level {
        return Err("layer insert nesting exceeds its limit".into());
    }
    Ok(InsertSlot {
        at,
        level,
        clipped: next_sibling(levels, at, level).is_some_and(is_clipped),
    })
}

/// The first record at or above `at` sitting at exactly `level` — the new
/// record's next SIBLING — or `None` when the level ends first (a shallower
/// record closes it) or the list does.
fn next_sibling(levels: &[usize], at: usize, level: usize) -> Option<usize> {
    for (j, &value) in levels.iter().enumerate().skip(at) {
        if value < level {
            return None;
        }
        if value == level {
            return Some(j);
        }
    }
    None
}

/// `slot(.., anchor, Above, ..)`, re-anchored on that slot's next sibling for
/// as long as the slot's `clipped` is true: the slot directly above the clip
/// run that contains or sits on `anchor`. The result's `clipped` is false, so
/// a record placed there joins no run and the members of the run keep their
/// base. Auto-Blend's merged layer is its one caller; every other insert
/// keeps joining a run through [`slot`].
pub(crate) fn slot_above_clip_run(
    levels: &[usize],
    is_group: &dyn Fn(usize) -> bool,
    is_clipped: &dyn Fn(usize) -> bool,
    anchor: usize,
    max_level: usize,
) -> Result<InsertSlot, String> {
    let above = |anchor: usize| {
        slot(
            levels,
            is_group,
            is_clipped,
            anchor,
            InsertPosition::Above,
            max_level,
        )
    };
    let mut found = above(anchor)?;
    while found.clipped {
        // A slot's `clipped` is its next sibling's flag, so the sibling is
        // there; the `else` arm only keeps the loop total.
        let Some(next) = next_sibling(levels, found.at, found.level) else {
            found.clipped = false;
            break;
        };
        found = above(next)?;
    }
    Ok(found)
}

impl DrDocument {
    /// [`slot_above_clip_run`] over this document's entries: the refusals
    /// are [`slot`]'s, by name.
    pub(crate) fn insert_slot_above_clip_run(&self, anchor: usize) -> Result<InsertSlot, String> {
        let levels: Vec<usize> = self.layers.iter().map(|l| usize::from(l.depth)).collect();
        slot_above_clip_run(
            &levels,
            &|i| self.layers[i].kind == LayerKind::Group,
            &|i| self.layers[i].clipped,
            anchor,
            usize::from(MAX_GROUP_DEPTH),
        )
    }

    /// Where an entry inserted at `position` relative to `anchor` would land.
    /// The refusals are [`slot`]'s, by name.
    pub fn insert_slot(
        &self,
        anchor: usize,
        position: InsertPosition,
    ) -> Result<InsertSlot, String> {
        let levels: Vec<usize> = self.layers.iter().map(|l| usize::from(l.depth)).collect();
        slot(
            &levels,
            &|i| self.layers[i].kind == LayerKind::Group,
            &|i| self.layers[i].clipped,
            anchor,
            position,
            usize::from(MAX_GROUP_DEPTH),
        )
    }

    /// Splices one prepared layer into the flat stack at `slot`. The layer
    /// takes the slot's LEVEL and its `clipped` flag, so it JOINS a clip run
    /// rather than re-basing the members above it ([`slot`]'s rule); every
    /// other field is the caller's. The byte twin of
    /// [`crate::precision_document::PrecisionDocument::inserting_record`].
    ///
    /// `None` for a level past the byte depth's own `u16`, for a slot past the
    /// stack and for a result whose structure would be malformed.
    pub(crate) fn inserting_layer(&self, layer: Layer, slot: InsertSlot) -> Option<Self> {
        if slot.at > self.layers.len() {
            return None;
        }
        let mut layer = layer;
        layer.depth = u16::try_from(slot.level).ok()?;
        layer.clipped = slot.clipped;
        let mut doc = self.clone();
        doc.layers.insert(slot.at, layer);
        doc_group::validated(doc)
    }

    /// Inserts a transparent CANVAS-SIZED layer at the slot `position` names
    /// relative to `anchor`. The positional twin of
    /// [`DrDocument::adding_layer`], which stays exactly as it was: this is
    /// the only entry point that can land a layer inside a group or at the
    /// top of the root level.
    ///
    /// `None` for a refused slot, for growth past native capacity (checked
    /// before allocation, like `adding_layer`) and for a result whose
    /// structure would be malformed.
    pub fn adding_layer_at(
        &self,
        anchor: usize,
        position: InsertPosition,
        name: &str,
    ) -> Option<Self> {
        let slot = self.insert_slot(anchor, position).ok()?;
        if !self.native_layers_fit_dimensions(
            self.layers
                .iter()
                .map(|l| l.pixels.dimensions())
                .chain(std::iter::once((self.width, self.height))),
        ) {
            return None;
        }
        self.inserting_layer(self.transparent_canvas_layer(name), slot)
    }
}
