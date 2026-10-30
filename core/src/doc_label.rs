//! Per-layer COLOUR LABELS: the panel's find-me-again mark, and the third
//! member of the `doc_lock` / `doc_group_query` family of small per-entry
//! bookkeeping bits.
//!
//! A label is a number 0..=7 on every entry, raster and group alike. 0 is
//! "no label"; 1..=7 are exactly Photoshop's `lclr` palette codes, in
//! Photoshop's own order (red, orange, yellow, green, blue, violet, gray), so
//! the PSD importer and exporter carry the number itself with no mapping
//! table of their own. Nothing about a label reaches the compositor: it is
//! metadata the panel filters on, which is why it is EXEMPT from the layer
//! locks — like a rename, marking a locked layer changes no pixel.
//!
//! Persistence is the conditional `label_native` wrapper (byte 41 / typed 42),
//! never a field inside a layer record, so a document with no label at all is
//! byte-for-byte the file the prior writer made.
//!
//! THE MERGE RULE is `doc_identity`'s: a merge keeps the LOWEST merged
//! entry's label, because the merged entry takes that member's slot, name,
//! locks, link and id — it IS that layer with new pixels. A FLATTEN makes one
//! new picture out of everything, so its entry is unlabelled (`Layer::new`
//! sets 0 and nothing overrides it).
//!
//! `Layer.label` / `PrecisionDocumentLayer.label`, 0 none and 1…7 exactly Photoshop's `lclr`
//! palette codes in Photoshop's own order, so PSD import and export carry the NUMBER with no
//! mapping table. `with_layer_label` follows the purity rule (the same label answers `None`);
//! duplicate, group/ungroup, `document_clone` and every geometry op carry it; a MERGE keeps the
//! LOWEST merged entry's label (`doc_identity`'s rule) and a FLATTEN is a new picture, so
//! unlabelled.

use crate::doc::DrDocument;

/// The largest label code; the FFI, the header and the reader all restate it.
pub const MAX_LABEL: u8 = 7;

/// The seven label names, indexed by code − 1. The host spells labels with
/// these words and so does the `.rz`/PSD documentation; the numbers are the
/// wire form.
pub const LABEL_NAMES: [&str; MAX_LABEL as usize] =
    ["red", "orange", "yellow", "green", "blue", "violet", "gray"];

/// The name of a code, or `None` for 0 (no label) and anything above 7.
pub fn label_name(code: u8) -> Option<&'static str> {
    (1..=MAX_LABEL)
        .contains(&code)
        .then(|| LABEL_NAMES[code as usize - 1])
}

/// The code a name spells, or `None`. Exact lowercase spellings only — a
/// label is a wire value, not a user string.
pub fn label_code(name: &str) -> Option<u8> {
    LABEL_NAMES
        .iter()
        .position(|&n| n == name)
        .map(|i| i as u8 + 1)
}

impl DrDocument {
    /// Layer `idx`'s colour label; 0 for an out-of-range index.
    pub fn layer_label(&self, idx: usize) -> u8 {
        self.layers.get(idx).map_or(0, |l| l.label)
    }

    /// Sets layer `idx`'s colour label. `None` for an out-of-range index, a
    /// code above [`MAX_LABEL`], and for the label the layer already carries
    /// (an identical copy would register a phantom undo step). Deliberately
    /// NOT gated by the locks: a label is panel bookkeeping, like a rename.
    pub fn with_layer_label(&self, idx: usize, label: u8) -> Option<Self> {
        if label > MAX_LABEL {
            return None;
        }
        let layer = self.layers.get(idx)?;
        if layer.label == label {
            return None;
        }
        let mut doc = self.clone();
        doc.layers[idx].label = label;
        Some(doc)
    }
}
