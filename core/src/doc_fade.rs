//! Edit ▸ Fade on the byte document: the `fade` kernel walked over one
//! layer, `self` being the faded step's result and `before` the state before
//! it (same layer index in both). `precision_fade` is the 16-bit and float
//! twin; the per-pixel rule is the kernel's module doc.
//!
//! The walk covers the whole of the result's layer rect and never grows it;
//! `before` is read at the same CANVAS position, transparent outside its own
//! layer. The step's weight per pixel is the canvas-sized `coverage` (a
//! region step's selection; 1 everywhere when there is none) or, for a
//! stroke (`changed_only`), 1 where the stored pixel differs from `before`'s
//! and 0 elsewhere.
use crate::blend::BlendMode;
use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::fade::{fade_blend, fade_pixel};

fn unit(p: [u8; 4]) -> [f32; 4] {
    p.map(|v| f32::from(v) / 255.0)
}

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl DrDocument {
    /// Fades layer `idx` of this document (the result of the step being
    /// faded) back toward the same layer of `before` (the state before that
    /// step) at `opacity` in `mode` — Edit ▸ Fade. `coverage` is
    /// the canvas-sized u8 weight of a region step (`None`: 1 everywhere);
    /// `changed_only` weighs a stroke instead: 1 where the layer's pixel
    /// differs from `before`'s, 0 elsewhere (then `coverage` is ignored).
    /// Under a transparency lock the layer keeps its alpha. The layer does
    /// not grow.
    ///
    /// Domain refusals (`None`): `idx` out of range or not a raster layer in
    /// either document; a retained source on the layer; `before` of another
    /// canvas size; Pass Through; `opacity` non-finite or outside [0, 1];
    /// `coverage` not the canvas pixel count; a lock that forbids a pixel
    /// edit; or no byte changing (Normal at opacity 1, say) — an identical
    /// copy would register a phantom undo step.
    pub fn fade(
        &self,
        before: &DrDocument,
        idx: usize,
        mode: BlendMode,
        opacity: f32,
        coverage: Option<&[u8]>,
        changed_only: bool,
    ) -> Option<DrDocument> {
        self.under_locks(idx, EditKind::Pixels, |doc| {
            doc.fade_unlocked(before, idx, mode, opacity, coverage, changed_only)
        })
    }

    fn fade_unlocked(
        &self,
        before: &DrDocument,
        idx: usize,
        mode: BlendMode,
        opacity: f32,
        coverage: Option<&[u8]>,
        changed_only: bool,
    ) -> Option<DrDocument> {
        let layer = self.raster_layer(idx)?;
        let from = before.raster_layer(idx)?;
        if layer.retained_source.is_some()
            || before.width != self.width
            || before.height != self.height
            || !opacity.is_finite()
            || !(0.0..=1.0).contains(&opacity)
        {
            return None;
        }
        let blend = fade_blend(mode, false)?;
        let n = (self.width as usize).checked_mul(self.height as usize)?;
        let coverage = match coverage {
            Some(plane) if plane.len() != n => return None,
            other => other,
        };
        let (lw, lh) = layer.pixels.dimensions();
        if lw == 0 || lh == 0 {
            return None;
        }
        let (ox, oy) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        // The layer rect's position in `before`'s layer, and the part of it
        // that layer holds, read once.
        let (sx, sy) = (ox - i64::from(from.offset.0), oy - i64::from(from.offset.1));
        let (sw, sh) = from.pixels.dimensions();
        let (ix0, iy0) = (sx.max(0), sy.max(0));
        let (ix1, iy1) = (
            (sx + i64::from(lw)).min(i64::from(sw)),
            (sy + i64::from(lh)).min(i64::from(sh)),
        );
        let held = if ix0 < ix1 && iy0 < iy1 {
            from.pixels.read_rect(
                ix0 as u32,
                iy0 as u32,
                (ix1 - ix0) as u32,
                (iy1 - iy0) as u32,
            )?
        } else {
            Vec::new()
        };
        let frozen = self.alpha_frozen(idx);
        let mut px = layer.pixels.read_rect(0, 0, lw, lh)?;
        let mut changed = false;
        for row in 0..lh as usize {
            let cy = oy + row as i64;
            let source_y = sy + row as i64;
            for column in 0..lw as usize {
                let cx = ox + column as i64;
                let at = row * lw as usize + column;
                let old = px[at];
                let source_x = sx + column as i64;
                let prior = if (ix0..ix1).contains(&source_x) && (iy0..iy1).contains(&source_y) {
                    held[(source_y - iy0) as usize * (ix1 - ix0) as usize
                        + (source_x - ix0) as usize]
                } else {
                    [0; 4]
                };
                let weight = if changed_only {
                    if prior == old {
                        0.0
                    } else {
                        1.0
                    }
                } else if let Some(plane) = coverage {
                    let inside = (0..i64::from(self.width)).contains(&cx)
                        && (0..i64::from(self.height)).contains(&cy);
                    if inside {
                        f32::from(plane[cy as usize * self.width as usize + cx as usize]) / 255.0
                    } else {
                        0.0
                    }
                } else {
                    1.0
                };
                if prior[3] == 0 && old[3] == 0 {
                    continue;
                }
                let out = fade_pixel(
                    unit(prior),
                    unit(old),
                    &blend,
                    weight,
                    opacity,
                    frozen,
                    (cx, cy),
                );
                let mut new = out.map(byte);
                if frozen {
                    new[3] = old[3];
                }
                if new != old {
                    px[at] = new;
                    changed = true;
                }
            }
        }
        if !changed {
            return None;
        }
        let pixels = layer.pixels.with_rect(0, 0, lw, lh, &px).ok()??;
        self.with_layer_raster(idx, pixels)
    }
}
