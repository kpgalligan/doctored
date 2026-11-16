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
        let canvas = (self.width, self.height);
        let Some(plane) = &layer.mask else {
            return Some(align_box(canvas, None));
        };
        // The mask read in place, one sample at a time: no copy.
        let (w, h) = plane.dimensions();
        let covered = |i: usize| {
            let (x, y) = (i % w as usize, i / w as usize);
            plane.sample_at(x as u32, y as u32).is_some_and(|v| v > 0)
        };
        let mask = MaskCoverage {
            covered: &covered,
            samples: u64::from(w) * u64::from(h),
            dimensions: (w, h),
            offset: layer.mask_properties.offset,
            outside: layer.mask_properties.outside_coverage,
            enabled: layer.mask_enabled,
        };
        Some(align_box(canvas, Some(mask)))
    }
}

impl PrecisionDocument {
    /// [`DrDocument::fill_align_box`]'s typed twin over record `idx`
    /// (postorder) and its u16 mask plane, 16-bit, 32-bit float and CMYK
    /// alike. `None` only for an out-of-range index.
    pub fn fill_align_box(&self, idx: usize) -> Option<[i32; 4]> {
        let record = self.layer(idx)?;
        let Some(mask) = &record.mask else {
            return Some(align_box(self.dimensions(), None));
        };
        // The mask plane read in place, one sample at a time: no copy.
        let plane = mask.plane();
        let (w, h) = plane.dimensions();
        let covered = |i: usize| {
            let (x, y) = (i % w as usize, i / w as usize);
            plane.sample_at(x as u32, y as u32).is_some_and(|v| v > 0)
        };
        let coverage = MaskCoverage {
            covered: &covered,
            samples: u64::from(w) * u64::from(h),
            dimensions: (w, h),
            offset: mask.properties().offset,
            outside: mask.properties().outside_coverage,
            enabled: mask.enabled(),
        };
        Some(align_box(self.dimensions(), Some(coverage)))
    }
}
