//! The set DUPLICATE and DELETE (`duplicate_layers` / `remove_layers`),
//! moved out of `doc_structure` verbatim. Each is ONE call over the
//! whole set and never a host loop — `doc_structure`'s module doc says why.
//!
//! # The set's shape
//!
//! Both take any set and drop the entries SUBSUMED by another given entry's
//! subtree — selecting a group and one of its children means the group, since
//! duplicating or deleting the group already carries the child. The
//! subsumption is `doc_structure::independent_roots`, which `doc_align`
//! shares.
//!
//! # Why TOP-DOWN
//!
//! Each op's internal loop runs TOP-DOWN, because an edit above an index never
//! moves the entries below it, so every root still to go keeps the number it
//! was given.
//!
//! Its module doc holds the subsumption rule (a given entry inside another given entry's subtree is
//! dropped, through `doc_structure::independent_roots`, which stays in `doc_structure` because
//! `doc_align` shares it) and why each internal loop runs TOP-DOWN.

use crate::doc::DrDocument;
use crate::doc_group::{subtree, validated};
use crate::doc_structure::independent_roots;

impl DrDocument {
    /// Duplicates every given entry (a group with its whole subtree),
    /// each copy immediately above its original — Duplicate Layers over a
    /// multi-selection, and the ONE call a host makes for it.
    ///
    /// Entries subsumed by another given entry's subtree are dropped (see the
    /// module doc). `None` for an empty or out-of-range set.
    pub fn duplicate_layers(&self, idx: &[usize]) -> Option<Self> {
        let roots = independent_roots(&self.layers, idx)?;
        let mut doc = self.clone();
        // TOP-DOWN: a duplicate inserted above `i` never moves an index below
        // it, so every remaining root keeps the number it was given.
        for &i in roots.iter().rev() {
            doc = doc.duplicating_layer(i)?;
        }
        validated(doc)
    }

    /// Removes every given entry (a group with its whole subtree). Entries
    /// subsumed by another given entry's subtree are dropped (see the module
    /// doc). `None` for an empty or out-of-range set and when the call would
    /// leave the document with no entries at all — the same floor
    /// [`DrDocument::removing_layer`] enforces.
    pub fn remove_layers(&self, idx: &[usize]) -> Option<Self> {
        let roots = independent_roots(&self.layers, idx)?;
        let total: usize = roots
            .iter()
            .map(|&i| subtree(&self.layers, i).len())
            .sum::<usize>();
        if total >= self.layers.len() {
            return None;
        }
        let mut doc = self.clone();
        // TOP-DOWN, so a removal above never renumbers the entries still to
        // go; the floor above guarantees no intermediate step empties the
        // document, so `removing_layer`'s own refusal can never fire here.
        for &i in roots.iter().rev() {
            doc = doc.removing_layer(i)?;
        }
        validated(doc)
    }
}
