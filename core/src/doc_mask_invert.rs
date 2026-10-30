//! Byte layer-mask INVERT over the whole canvas (`dr_doc_invert_mask`).
//!
//! A mask's coverage is its stored plane inside the stored rect and the
//! constant `outside_coverage` everywhere else, so an Invert that only
//! rewrote the stored samples left the rest of the canvas as it was. This op
//! inverts the coverage the canvas sees:
//!
//! - No selection: every stored sample becomes `255 − v` and
//!   `outside_coverage` flips 0 ↔ 255. The stored rect, offset, link,
//!   density, feather and enabled flag are kept.
//! - With a selection: a raster entry's stored plane first grows to the union
//!   of its rect and the canvas, filled with `outside_coverage`
//!   (`mask_invert`), which changes no effective coverage; each stored sample
//!   then becomes `scalar_result(v, 255 − v, coverage)`, `doc_scope`'s scoped
//!   scalar writeback with its off-canvas-is-0 rule. `outside_coverage` is
//!   kept: nothing outside the canvas is selected. A GROUP's mask does not
//!   grow: it is canvas-sized and positioned by the group's offset
//!   (`doc_align`), so its rect is not this op's to change.
//!
//! A Mask edit under `doc_lock` (only Lock All refuses). The typed twin is
//! `precision_scalar_edit`'s mask-invert rule.

use std::sync::Arc;

use image::GrayImage;

use crate::doc::{DrDocument, LayerKind};
use crate::doc_lock::EditKind;
use crate::doc_scope::scalar_result;
use crate::mask_invert::{grown, union_with_canvas, PlaneRect};

impl DrDocument {
    /// Inverts entry `idx`'s mask coverage over the whole canvas (module
    /// doc); `selection` is canvas-sized coverage or `None` for no selection.
    /// `None` for an out-of-range index, a missing mask, a selection that is
    /// not canvas-sized, Lock All, a grown plane the mask model or the native
    /// mask budget refuses, or a selection that changes no sample.
    pub fn invert_mask(&self, idx: usize, selection: Option<&[u8]>) -> Option<Self> {
        self.under_locks(idx, EditKind::Mask, |doc| {
            let layer = doc.layers.get(idx)?;
            let before = layer.mask.as_deref()?;
            let canvas = (doc.width, doc.height);
            let Some(selection) = selection else {
                let mut plane = before.clone();
                for value in plane.iter_mut() {
                    *value = 255 - *value;
                }
                let mut out = doc.clone();
                let next = &mut out.layers[idx];
                next.mask = Some(Arc::new(plane));
                next.mask_properties.outside_coverage =
                    255 - layer.mask_properties.outside_coverage;
                return Some(out);
            };
            if selection.len() != canvas.0 as usize * canvas.1 as usize {
                return None;
            }
            let from: PlaneRect = (before.dimensions(), layer.mask_properties.offset);
            let to = if layer.kind == LayerKind::Group {
                from
            } else {
                union_with_canvas(from, canvas)?
            };
            if to != from
                && (!layer.mask_properties.valid(to.0)
                    || !doc.native_masks_fit_dimensions(doc.layers.iter().enumerate().filter_map(
                        |(i, l)| {
                            l.mask
                                .as_ref()
                                .map(|m| if i == idx { to.0 } else { m.dimensions() })
                        },
                    )))
            {
                return None;
            }
            let mut samples = grown(
                before.as_raw(),
                from,
                to,
                layer.mask_properties.outside_coverage,
            )?;
            let ((w, _), (ox, oy)) = to;
            let mut changed = false;
            for (i, value) in samples.iter_mut().enumerate() {
                let cx = (i % w as usize) as i64 + i64::from(ox);
                let cy = (i / w as usize) as i64 + i64::from(oy);
                if cx < 0 || cy < 0 || cx >= i64::from(canvas.0) || cy >= i64::from(canvas.1) {
                    continue;
                }
                let coverage = selection[cy as usize * canvas.0 as usize + cx as usize];
                let after = scalar_result(*value, 255 - *value, coverage);
                changed |= after != *value;
                *value = after;
            }
            if !changed {
                return None;
            }
            let mut out = doc.clone();
            let next = &mut out.layers[idx];
            next.mask = Some(Arc::new(GrayImage::from_raw(to.0 .0, to.0 .1, samples)?));
            next.mask_properties.offset = to.1;
            Some(out)
        })
    }
}
