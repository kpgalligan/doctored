//! Byte SET MASK: replace one raster entry's mask with caller coverage
//! at an arbitrary canvas rectangle, linked flag and outside coverage.
//!
//! The byte twin of the typed `MaskEdit::SetMask` (`precision_mask_edit`),
//! which is the master copy of the rule: the mask is replaced and enabled,
//! its properties are `MaskProperties::at(offset)` with the given `linked`
//! and `outside_coverage`, density 1 and feather 0. Unlike `add_mask` the
//! rectangle is the CALLER's, not the layer's pixel rect, so a canvas-sized
//! selection mask is never cropped to the layer — which is what lets it be
//! painted, or the pixels moved, beyond the layer's original extent.
//!
//! A GROUP keeps `add_mask`: its offset positions its mask (`doc_align`), so
//! an arbitrary rectangle has no meaning there. A MASK edit under
//! `doc_lock` (only Lock All refuses), and `None` for an exact no-op.
//!
//! `doc_mask_set.rs` is the byte SET MASK (`dr_doc_set_mask`): caller coverage at an
//! arbitrary canvas rect with linked and outside coverage, never cropped to the layer (groups keep
//! `add_mask`), the byte twin of the typed `set_mask` edit (which takes `outside_coverage` too).

use std::sync::Arc;

use image::GrayImage;

use crate::doc::{DrDocument, LayerKind};
use crate::doc_lock::EditKind;
use crate::doc_mask::MaskProperties;

impl DrDocument {
    /// Replaces entry `idx`'s mask with `coverage` placed at canvas `offset`
    /// and enables it. `None` for an out-of-range or GROUP index, an empty
    /// plane, invalid properties (`outside_coverage` other than 0/255), a
    /// native mask-budget refusal, Lock All, or an identical mask.
    pub fn set_mask(
        &self,
        idx: usize,
        coverage: GrayImage,
        offset: (i32, i32),
        linked: bool,
        outside_coverage: u8,
    ) -> Option<Self> {
        self.under_locks(idx, EditKind::Mask, |doc| {
            let layer = doc.layers.get(idx)?;
            if layer.kind == LayerKind::Group {
                return None;
            }
            let dims = coverage.dimensions();
            if dims.0 == 0 || dims.1 == 0 {
                return None;
            }
            let mut properties = MaskProperties::at(offset);
            properties.linked = linked;
            properties.outside_coverage = outside_coverage;
            if !properties.valid(dims)
                || !doc.native_masks_fit_dimensions(doc.layers.iter().enumerate().filter_map(
                    |(i, l)| {
                        if i == idx {
                            Some(dims)
                        } else {
                            l.mask.as_ref().map(|m| m.dimensions())
                        }
                    },
                ))
            {
                return None;
            }
            let mut out = doc.clone();
            let next = &mut out.layers[idx];
            next.mask = Some(Arc::new(coverage));
            next.mask_enabled = true;
            next.mask_properties = properties;
            if crate::doc_lock::same_mask(layer, &out.layers[idx]) {
                return None;
            }
            Some(out)
        })
    }
}
