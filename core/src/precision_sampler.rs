//! COLOUR SAMPLERS at high depth — the typed twin of `doc_sampler`'s
//! `impl DrDocument` block, and nothing more (the `precision_guide`
//! pattern). Every rule — the pixel-index convention, the ten-sampler cap,
//! the 1/3/5 sample sizes, the geometry maps and the drop — is
//! `doc_sampler`'s; the typed geometry fold (`precision_geometry`) asks
//! `doc_sampler::edited_samplers` once per edit.

use crate::doc_sampler::{validated, ColorSampler};
use crate::precision_document::PrecisionDocument;

impl PrecisionDocument {
    /// The document's colour samplers, in label order (#1 first).
    pub fn color_samplers(&self) -> &[ColorSampler] {
        &self.color_samplers
    }

    /// `DrDocument::with_color_samplers` at high depth: `None` for a list
    /// `doc_sampler::validated` refuses or the list the document already
    /// holds.
    pub fn with_color_samplers(&self, list: &[ColorSampler]) -> Option<Self> {
        let (w, h) = self.dimensions();
        let list = validated(list, w, h)?;
        if list == self.color_samplers {
            return None;
        }
        Some(Self {
            color_samplers: list,
            ..self.clone()
        })
    }
}
