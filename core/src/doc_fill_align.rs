//! A Gradient Fill layer's Align with Layer box on a document: the two
//! thin adapters of `fill_align_box::align_box` (its module doc holds the
//! rule), the byte `DrDocument` over a layer's u8 mask and the typed
//! `PrecisionDocument` over its u16 `PrecisionMask` (CMYK included). The host
//! asks before it renders an aligned fill (`dr_doc_fill_align_box`,
//! `dr_precision_doc_fill_align_box`), so the import and the host span the
//! gradient over the same box.

use crate::doc::DrDocument;
use crate::fill_align_box::{align_box, MaskCoverage};
use crate::precision_document::PrecisionDocument;

impl DrDocument {
    /// Entry `idx`'s Align with Layer box `[x, y, w, h]` in canvas pixels
    /// (`fill_align_box::align_box` over its mask's raw coverage, its enabled
    /// state and its outside coverage; the canvas when it has none). `None`
    /// only for an out-of-range index.
    pub fn fill_align_box(&self, idx: usize) -> Option<[i32; 4]> {
        let layer = self.layers.get(idx)?;
        let mask = layer.mask.as_ref().map(|plane| MaskCoverage {
            samples: plane.as_raw().as_slice(),
            dimensions: plane.dimensions(),
            offset: layer.mask_properties.offset,
            outside: layer.mask_properties.outside_coverage,
            enabled: layer.mask_enabled,
        });
        Some(align_box((self.width, self.height), mask))
    }
}

impl PrecisionDocument {
    /// [`DrDocument::fill_align_box`]'s typed twin over record `idx`
    /// (postorder) and its u16 mask plane, 16-bit, 32-bit float and CMYK
    /// alike. `None` only for an out-of-range index.
    pub fn fill_align_box(&self, idx: usize) -> Option<[i32; 4]> {
        let record = self.layer(idx)?;
        let mask = record.mask.as_ref().map(|mask| MaskCoverage {
            samples: mask.plane().samples(),
            dimensions: mask.plane().dimensions(),
            offset: mask.properties().offset,
            outside: mask.properties().outside_coverage,
            enabled: mask.enabled(),
        });
        Some(align_box(self.dimensions(), mask))
    }
}
