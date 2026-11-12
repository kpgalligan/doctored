//! The BACKGROUND MARK: whether a document's bottom layer is a Background or
//! an ordinary layer, for the documents whose source file said so.
//!
//! Doctored's rule. A document may carry one mark: a layer id and a role,
//! [`BackgroundRole::Background`] or [`BackgroundRole::Ordinary`]. The mark
//! APPLIES only while the layer it names is entry 0 (the bottom root entry).
//! With no applicable mark, D5 (`style_knockout`) is the structural rule: an
//! entry 0 that is visible, Normal, 100 %, unmasked, unstyled, canvas-sized and
//! at the origin is the Background a Deep knockout punches to. An applicable
//! `Ordinary` mark VETOES D5, so a knockout over that layer punches to
//! transparency. An applicable `Background` mark changes nothing in D5 (the
//! structural clauses must still hold, because the snapshot is taken after
//! entry 0 composites alone) and is what the PSD writer reads
//! (`psd_background`).
//!
//! Three states rather than a flag: every document made in Doctored, and
//! every one opened from a file that does not say, keeps the structural rule
//! and so renders exactly as it always did. Only a PSD import sets a mark
//! (`psd_background::import_role`): `Background` for Photoshop's Background
//! record, `Ordinary` for any other kept bottom record.
//!
//! The mark follows the LAYER ID, like a vector-mask record, so no layer
//! literal carries it: an edit that keeps entry 0's id keeps the mark (a
//! paint stroke, a merge INTO the bottom entry, which keeps the lowest merged
//! id per `doc_identity`, a depth or mode conversion, Duplicate Document),
//! and one that mints a new bottom id drops it (Flatten Image, a duplicate
//! layer moved to the bottom). A mark whose layer is gone is dropped at the
//! next native save (`background_native`).

use crate::doc::DrDocument;
use crate::precision_document::PrecisionDocument;

/// The role a mark gives the layer it names (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundRole {
    /// The file's Background layer: written back as one.
    Background,
    /// An ordinary bottom layer: D5 never treats it as the Background.
    Ordinary,
}

impl BackgroundRole {
    /// The FFI and native code: 1 Background, 2 Ordinary (0 is "no mark").
    pub fn code(self) -> u32 {
        match self {
            Self::Background => 1,
            Self::Ordinary => 2,
        }
    }

    /// The inverse of [`Self::code`]; `None` for any other value.
    pub(crate) fn from_code(code: u32) -> Option<Self> {
        match code {
            1 => Some(Self::Background),
            2 => Some(Self::Ordinary),
            _ => None,
        }
    }
}

/// A document's one mark: the layer it names, by stable id, and its role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BackgroundMark {
    pub layer: u64,
    pub role: BackgroundRole,
}

/// The role `mark` gives the entry whose id is `bottom`: `Some` only when the
/// mark names that entry.
pub(crate) fn applied(mark: Option<BackgroundMark>, bottom: Option<u64>) -> Option<BackgroundRole> {
    let mark = mark?;
    (Some(mark.layer) == bottom).then_some(mark.role)
}

/// D5's one veto, shared by the byte and typed predicates: an applicable
/// `Ordinary` mark.
pub(crate) fn vetoes(mark: Option<BackgroundMark>, bottom: Option<u64>) -> bool {
    applied(mark, bottom) == Some(BackgroundRole::Ordinary)
}

impl DrDocument {
    /// The role of entry 0, when the document's mark applies to it.
    pub fn background_role(&self) -> Option<BackgroundRole> {
        applied(self.background, self.layers.first().map(|l| l.id))
    }

    /// Marks layer `index` with `role` (the PSD importer's one call).
    pub(crate) fn mark_background(&mut self, index: usize, role: BackgroundRole) {
        self.background = self.layers.get(index).map(|layer| BackgroundMark {
            layer: layer.id,
            role,
        });
    }
}

impl PrecisionDocument {
    /// The role of root entry 0, when the document's mark applies to it.
    pub fn background_role(&self) -> Option<BackgroundRole> {
        applied(self.background, self.layers.first().map(|l| l.id))
    }

    /// Marks root entry 0 with `role` (the PSD importer's one call).
    pub(crate) fn mark_bottom(&mut self, role: BackgroundRole) {
        self.background = self.layers.first().map(|layer| BackgroundMark {
            layer: layer.id,
            role,
        });
    }
}
