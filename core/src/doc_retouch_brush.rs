//! The byte adapters of the Blur, Sharpen and Sponge brushes
//! (`retouch_brush`, the kernel) and of the History Brush, on the
//! layered document.
//!
//! The stroke arrives as the SAME canvas-frame premultiplied RGBA8 overlay
//! `dr_doc_dodge_burn_layer` takes, and as there ONLY its alpha is read, as
//! per-pixel stroke coverage. Both ops walk the overlay's nonzero rect on the
//! layer (`doc_paint::overlay_rect`), write it back through `with_rect`, never
//! grow the layer, and return `None` when no byte changes. Blur, Sharpen and
//! Sponge also take two options: the Sponge's
//! Vibrance and Blur and Sharpen's Sample All Layers (`retouch_brush`'s module
//! doc; the picture is `retouch_brush_sampling`'s).
use crate::adjust_vibrance::BoundVibrance;
use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::doc_paint::overlay_rect;
use crate::job_control::JobContext;
use crate::retouch_brush::{
    halo, mix, mix_over, refuse_options, sponge_slider, targets, RetouchBrushKind, Vibrancy,
};
use crate::retouch_brush_sampling::canvas_reach;

/// A straight byte pixel in unit range.
fn unit(p: [u8; 4]) -> [f32; 4] {
    p.map(|v| f32::from(v) / 255.0)
}

/// Round half-up back to a byte, as `dodge_burn_layer` does.
fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

impl DrDocument {
    /// Blurs, sharpens, saturates or desaturates layer `idx` where a stroke
    /// overlay covers it.
    ///
    /// `overlay` is the canvas-frame premultiplied RGBA8 buffer
    /// `dodge_burn_layer` takes (`w`/`h` must equal the canvas size); ONLY its
    /// alpha is read, as stroke coverage 0..=255. `kind` is 0 blur, 1 sharpen,
    /// 2 saturate, 3 desaturate (`retouch_brush`'s targets). Each covered
    /// pixel moves toward its target by `coverage · strength`, `strength`
    /// clamped to [0, 1]. The targets read the layer's own pixels as they were
    /// before the stroke, clamped at the layer's edges. Layer alpha is never
    /// touched, fully transparent pixels are skipped, and the layer does not
    /// grow.
    ///
    /// `vibrance` (Sponge only) takes the Vibrance target bound to the
    /// document's profile (`retouch_brush::Vibrancy::Bound`, the adjustment's
    /// own 8-bit binding `BoundVibrance::in_profile`). `sample_all_layers` (Blur and
    /// Sharpen only) reads the targets from the sampled picture
    /// (`retouch_brush_sampling::SAMPLE_ALL_LAYERS`) over the walk rect and its
    /// halo clipped to the canvas, reads clamped to that rect, and writes each
    /// covered pixel as `retouch_brush::mix_over` of the layer and `[target,
    /// sampled α]` by `coverage · strength`: alpha-0 pixels are written too,
    /// under a transparency lock alpha is kept and alpha-0 pixels stay, and
    /// the layer still does not grow.
    ///
    /// `Ok(None)`, a domain refusal: `idx` out of range or a group; `w`/`h`
    /// not the canvas size; `overlay` shorter than `w*h*4`; non-finite
    /// `strength`; `kind > 3`; an overlay that reaches no layer pixel; or no
    /// byte actually changing (strength 0, a flat area under Blur, a grey one
    /// under Sponge) — an identical copy would register a phantom undo step.
    /// A PIXELS edit under `doc_lock`. `Err`: an option on a kind it does not
    /// apply to (`retouch_brush::VIBRANCE_REFUSAL`, `SAMPLE_ALL_LAYERS_REFUSAL`)
    /// and the sampled picture's own render refusals.
    #[allow(clippy::too_many_arguments)]
    pub fn retouch_brush_layer(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        kind: u8,
        strength: f32,
        vibrance: bool,
        sample_all_layers: bool,
    ) -> Result<Option<Self>, String> {
        if let Some(kind) = RetouchBrushKind::from_code(kind) {
            refuse_options(kind, vibrance, sample_all_layers)?;
        }
        self.under_locks_fallible(idx, EditKind::Pixels, |doc| {
            doc.retouch_brush_layer_unlocked(
                idx,
                overlay,
                (w, h),
                kind,
                strength,
                vibrance,
                sample_all_layers,
            )
        })
    }

    /// The body of [`Self::retouch_brush_layer`], outside the lock gate.
    #[allow(clippy::too_many_arguments)]
    fn retouch_brush_layer_unlocked(
        &self,
        idx: usize,
        overlay: &[u8],
        (w, h): (u32, u32),
        kind: u8,
        strength: f32,
        vibrance: bool,
        sample_all_layers: bool,
    ) -> Result<Option<Self>, String> {
        let Some(layer) = self.raster_layer(idx) else {
            return Ok(None);
        };
        let Some(kind) = RetouchBrushKind::from_code(kind) else {
            return Ok(None);
        };
        if w != self.width || h != self.height || !strength.is_finite() {
            return Ok(None);
        }
        let expected = (w as usize)
            .checked_mul(h as usize)
            .and_then(|v| v.checked_mul(4));
        if expected.is_none_or(|expected| overlay.len() < expected) {
            return Ok(None);
        }
        let strength = strength.clamp(0.0, 1.0);
        // The same offset mapping and rect as `dodge_burn_layer`
        // (`doc_retouch`), whose comment names the master copy.
        let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        let Some((rx, ry, rw, rh)) = overlay_rect(overlay, (self.width, self.height), layer) else {
            return Ok(None);
        };
        let (lw, lh) = layer.pixels.dimensions();
        let reach = halo(kind);
        let bound =
            vibrance.then(|| BoundVibrance::srgb(sponge_slider(kind)).in_profile(&self.profile));
        let vibrancy = bound.as_ref().map_or(Vibrancy::Off, Vibrancy::Bound);
        let coverage_at = |row: usize, column: usize| -> u8 {
            let cy = (i64::from(ry) + row as i64 + off_y) as u64;
            let cx = (i64::from(rx) + column as i64 + off_x) as u64;
            overlay[((cy * u64::from(self.width) + cx) * 4) as usize + 3]
        };
        let Some(mut px) = layer.pixels.read_rect(rx, ry, rw, rh) else {
            return Ok(None);
        };
        let mut changed = false;
        if sample_all_layers {
            // The walk rect and its halo on the canvas: what the targets read
            // of the sampled picture, reads clamped to it.
            let offset = (layer.offset.0, layer.offset.1);
            let Some((sx, sy, sw, sh)) =
                canvas_reach((rx, ry, rw, rh), reach, offset, (self.width, self.height))
            else {
                return Ok(None);
            };
            let picture = self.sampled_region(idx, (sx, sy, sw, sh))?;
            let sampled = |x: i64, y: i64| -> [f32; 4] {
                let cx = (i64::from(rx) + x + off_x).clamp(i64::from(sx), i64::from(sx + sw) - 1);
                let cy = (i64::from(ry) + y + off_y).clamp(i64::from(sy), i64::from(sy + sh) - 1);
                unit(
                    picture
                        .get_pixel((cx - i64::from(sx)) as u32, (cy - i64::from(sy)) as u32)
                        .0,
                )
            };
            let target = targets(kind, vibrancy, (rw, rh), sampled, JobContext::NONE)?;
            let frozen = self.alpha_frozen(idx);
            for row in 0..rh as usize {
                for column in 0..rw as usize {
                    let coverage = coverage_at(row, column);
                    let at = row * rw as usize + column;
                    let old = px[at];
                    if coverage == 0 || (frozen && old[3] == 0) {
                        continue;
                    }
                    let amount = f32::from(coverage) / 255.0 * strength;
                    let alpha = sampled(column as i64, row as i64)[3];
                    let t = target[at];
                    let out = mix_over(unit(old), [t[0], t[1], t[2], alpha], amount, frozen);
                    let mut new = [byte(out[0]), byte(out[1]), byte(out[2]), old[3]];
                    if !frozen {
                        new[3] = byte(out[3]);
                        if new[3] == 0 {
                            // Nothing visible is left: transparent black, or
                            // the pixel as it was when it already held nothing.
                            new = if old[3] == 0 { old } else { [0; 4] };
                        }
                    }
                    if new != old {
                        px[at] = new;
                        changed = true;
                    }
                }
            }
        } else {
            // The rect plus the kind's halo, clipped to the layer: what the
            // targets read.
            let (hx, hy) = (rx.saturating_sub(reach), ry.saturating_sub(reach));
            let hw = (rx + rw).saturating_add(reach).min(lw) - hx;
            let hh = (ry + rh).saturating_add(reach).min(lh) - hy;
            let Some(source) = layer.pixels.read_rect(hx, hy, hw, hh) else {
                return Ok(None);
            };
            let read = |x: i64, y: i64| -> [f32; 4] {
                let lx = (i64::from(rx) + x).clamp(0, i64::from(lw) - 1) - i64::from(hx);
                let ly = (i64::from(ry) + y).clamp(0, i64::from(lh) - 1) - i64::from(hy);
                unit(source[ly as usize * hw as usize + lx as usize])
            };
            let target = targets(kind, vibrancy, (rw, rh), read, JobContext::NONE)?;
            for row in 0..rh as usize {
                for column in 0..rw as usize {
                    let coverage = coverage_at(row, column);
                    let at = row * rw as usize + column;
                    let old = px[at];
                    if coverage == 0 || old[3] == 0 {
                        continue;
                    }
                    let amount = f32::from(coverage) / 255.0 * strength;
                    let out = mix(unit(old), target[at], amount);
                    let new = [byte(out[0]), byte(out[1]), byte(out[2]), old[3]];
                    if new != old {
                        px[at] = new;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            // Identity result: refuse rather than mint an unchanged copy.
            return Ok(None);
        }
        let Some(pixels) = layer.pixels.with_rect(rx, ry, rw, rh, &px)? else {
            return Ok(None);
        };
        Ok(self.with_layer_raster(idx, pixels))
    }

    /// Paints layer `idx` back toward layer `source_idx` of `source` (an
    /// earlier state of this document) where a stroke overlay covers it.
    ///
    /// `overlay` is the canvas-frame premultiplied RGBA8 buffer
    /// `dodge_burn_layer` takes; ONLY its alpha is read, as stroke coverage.
    /// Each covered pixel is the premultiplied mix of all four channels toward
    /// the source layer's pixel at the same CANVAS position (transparent where
    /// the source layer has none) by `coverage · opacity`, `opacity` clamped to
    /// [0, 1]. The layer does not grow: source pixels outside its extent are
    /// not brought back. Under a transparency lock each pixel keeps its alpha
    /// and takes only the mixed colour, and alpha-0 pixels stay as they are.
    ///
    /// Domain refusals (`None`): either index out of range or a group; `w`/`h`
    /// not the canvas size, or `source` of a different canvas size; `overlay`
    /// shorter than `w*h*4`; non-finite `opacity`; an overlay that reaches no
    /// layer pixel; or no byte changing (the layer already equals the source
    /// under the stroke). A PIXELS edit under `doc_lock`.
    #[allow(clippy::too_many_arguments)]
    pub fn history_brush_layer(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        source: &DrDocument,
        source_idx: usize,
        opacity: f32,
    ) -> Option<Self> {
        self.under_locks(idx, EditKind::Pixels, |doc| {
            doc.history_brush_layer_unlocked(idx, overlay, w, h, source, source_idx, opacity)
        })
    }

    /// The body of [`Self::history_brush_layer`], outside the lock gate.
    #[allow(clippy::too_many_arguments)]
    fn history_brush_layer_unlocked(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        source: &DrDocument,
        source_idx: usize,
        opacity: f32,
    ) -> Option<Self> {
        let layer = self.raster_layer(idx)?;
        let from = source.raster_layer(source_idx)?;
        if w != self.width
            || h != self.height
            || source.width != self.width
            || source.height != self.height
            || !opacity.is_finite()
        {
            return None;
        }
        let expected = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        if overlay.len() < expected {
            return None;
        }
        let opacity = opacity.clamp(0.0, 1.0);
        let frozen = self.alpha_frozen(idx);
        let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        let (rx, ry, rw, rh) = overlay_rect(overlay, (self.width, self.height), layer)?;
        // The rect's canvas position in the source layer's own pixels.
        let (sx, sy) = (
            i64::from(rx) + off_x - i64::from(from.offset.0),
            i64::from(ry) + off_y - i64::from(from.offset.1),
        );
        let (sw, sh) = from.pixels.dimensions();
        // The part of that rect the source layer holds, read once.
        let (ix0, iy0) = (sx.max(0), sy.max(0));
        let (ix1, iy1) = (
            (sx + i64::from(rw)).min(i64::from(sw)),
            (sy + i64::from(rh)).min(i64::from(sh)),
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
        let mut px = layer.pixels.read_rect(rx, ry, rw, rh)?;
        let mut changed = false;
        for row in 0..rh as usize {
            let cy = (i64::from(ry) + row as i64 + off_y) as u64;
            let source_y = sy + row as i64;
            for column in 0..rw as usize {
                let cx = (i64::from(rx) + column as i64 + off_x) as u64;
                let coverage = overlay[((cy * u64::from(self.width) + cx) * 4) as usize + 3];
                if coverage == 0 {
                    continue;
                }
                let at = row * rw as usize + column;
                let old = px[at];
                if frozen && old[3] == 0 {
                    continue;
                }
                let source_x = sx + column as i64;
                let wanted = if (ix0..ix1).contains(&source_x) && (iy0..iy1).contains(&source_y) {
                    held[(source_y - iy0) as usize * (ix1 - ix0) as usize
                        + (source_x - ix0) as usize]
                } else {
                    [0; 4]
                };
                let t = f32::from(coverage) / 255.0 * opacity;
                let (a, b) = (unit(old), unit(wanted));
                // Premultiplied mix: a transparent side contributes no colour.
                let alpha = (1.0 - t) * a[3] + t * b[3];
                let mut new = old;
                if alpha > 0.0 {
                    for c in 0..3 {
                        new[c] = byte(((1.0 - t) * a[c] * a[3] + t * b[c] * b[3]) / alpha);
                    }
                }
                if !frozen {
                    new[3] = byte(alpha);
                    if new[3] == 0 {
                        // Nothing visible is left: transparent black, or the
                        // pixel as it was when it already held nothing.
                        new = if old[3] == 0 { old } else { [0; 4] };
                    }
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
        let pixels = layer.pixels.with_rect(rx, ry, rw, rh, &px).ok()??;
        self.with_layer_raster(idx, pixels)
    }
}
