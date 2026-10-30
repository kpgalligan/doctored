//! Delete Cropped Pixels: the Crop tool's option that TRIMS layer
//! storage to the canvas after a crop, where [`DrDocument::crop`] keeps the
//! off-canvas pixels as latent storage.
//!
//! The rule, shared with the typed straighten's `delete_cropped` flag
//! (`precision_geometry_canvas::deleting_cropped`):
//!
//! * every ELIGIBLE raster — `LayerKind::Raster`, no `meta` (so no
//!   adjustment and no described text/shape/photo layer) and no retained
//!   source or warp binding — is cut to its intersection with the canvas.
//!   One wholly outside the canvas has no intersection and becomes a 1×1
//!   transparent pixel at offset (0, 0), the empty-layer shape every other
//!   op leaves;
//! * masks, groups, adjustments, described or sourced layers, channels,
//!   guides and samplers are untouched. A mask is placed in canvas space by
//!   its own `mask_properties.offset`, so trimming the pixels under it moves
//!   nothing it hides;
//! * lock-EXEMPT, like the crop it follows: it changes the document's frame,
//!   not a layer;
//! * every layer comp entry bound to a trimmed layer that recorded an offset
//!   shifts by the same delta as the layer's pixel origin (trimmed origin
//!   minus untrimmed origin), so a comp shows the same pixels where it did.
//!
//! The pixel copy is `ops::crop`; nothing here is new pixel math.

use std::collections::HashMap;

use image::RgbaImage;

use crate::doc::{translated_offset, DrDocument, Layer, LayerKind, LayerRaster};

/// Whether Delete Cropped Pixels trims `layer` (the module doc's rule).
pub(crate) fn eligible(layer: &Layer) -> bool {
    layer.kind == LayerKind::Raster
        && layer.meta.is_none()
        && layer.retained_source.is_none()
        && layer.warp.is_none()
}

impl DrDocument {
    /// The document with every eligible raster trimmed to the canvas (the
    /// module doc's rule). `None` when no layer reaches outside the canvas —
    /// an identical copy would register a phantom undo step in the host.
    pub fn delete_cropped_pixels(&self) -> Option<Self> {
        let (cw, ch) = (i64::from(self.width), i64::from(self.height));
        let mut deltas: HashMap<u64, (i64, i64)> = HashMap::new();
        let mut layers = Vec::with_capacity(self.layers.len());
        for layer in &self.layers {
            let mut next = layer.clone();
            if eligible(layer) {
                let (lw, lh) = layer.pixels.dimensions();
                let (ox, oy) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
                let (x0, y0) = (ox.max(0), oy.max(0));
                let (x1, y1) = ((ox + i64::from(lw)).min(cw), (oy + i64::from(lh)).min(ch));
                let inside =
                    x0 == ox && y0 == oy && x1 == ox + i64::from(lw) && y1 == oy + i64::from(lh);
                if !inside {
                    let (pixels, origin) = if x0 < x1 && y0 < y1 {
                        let image = layer.pixels.contiguous_image();
                        let cut = crate::ops::crop(
                            &image,
                            u32::try_from(x0 - ox).ok()?,
                            u32::try_from(y0 - oy).ok()?,
                            u32::try_from(x1 - x0).ok()?,
                            u32::try_from(y1 - y0).ok()?,
                        )?;
                        (cut, (x0, y0))
                    } else {
                        (RgbaImage::new(1, 1), (0, 0))
                    };
                    next.pixels = LayerRaster::unseeded(pixels);
                    next.offset = (i32::try_from(origin.0).ok()?, i32::try_from(origin.1).ok()?);
                    deltas.insert(layer.id, (origin.0 - ox, origin.1 - oy));
                    // The offset moved and the content did not: a linked
                    // pattern overlay's anchor moves with the offset
                    // (`style_pattern`'s Placement).
                    next.style = crate::style_pattern::rebased_arc(
                        &layer.style,
                        (origin.0 - ox, origin.1 - oy),
                    );
                }
            }
            layers.push(next);
        }
        if deltas.is_empty() {
            return None;
        }
        let comps = self.remapped_comps(
            |l, rec| {
                deltas
                    .get(&l.id)
                    .map(|&(dx, dy)| translated_offset(rec, dx, dy))
            },
            None,
            |_| 1.0,
        );
        let mut out = self.clone();
        out.layers = layers;
        out.comps = comps;
        Some(out)
    }
}
