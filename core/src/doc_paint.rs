//! The byte paint walk: brush, eraser, Clone Stamp and Edit ▸ Fill… / Stroke…
//! all composite through ONE per-pixel walk, [`DrDocument::paint_walk`].
//!
//! A caller hands the walk an INK closure rather than a buffer: `ink(i)`
//! answers the paint at frame sample `i`, either a premultiplied RGBA8
//! overlay sample (the brush's CoreGraphics surface, Clone Stamp's sampled
//! source) or a straight colour with a coverage alpha (Fill…'s colour at the
//! selection's coverage, the colour brush's blend modes). So Fill builds no
//! canvas-sized overlay at all, and a non-Normal fill or colour-brush stroke
//! blends the EXACT straight colour instead of one un-premultiplied back out
//! of a low-alpha byte. Only the rect of samples whose premultiplied
//! ink is nonzero ([`source_rect_in`]) is read and written.
//!
//! Three modes: OVER is source-over through `blend::paint_pixel`, the one
//! premultiplied compositing kernel, so the classic brush and a Normal fill
//! are byte-for-byte what they always were; ERASE takes the ink's alpha off
//! the layer's; BLEND pushes every covered pixel through the SAME W3C kernel
//! the layer projection uses (`blend::composite_source_into`), so a brush
//! set to Multiply lands exactly what a Multiply layer holding the stroke
//! would flatten to against the layer's current pixels. Erase has no blend
//! counterpart — the eraser never blends.
//!
//! With `preserve` (Preserve Transparency, or a transparency lock) Over and
//! Blend write the frozen-alpha rule through `blend::composite_source_frozen`:
//! an alpha-0 pixel stays as it is, a semi-transparent one takes
//! `lerp(Cb, B(Cb, Cs), sa)` and keeps its alpha, an opaque one is unchanged
//! from the free path. `doc_lock`'s alpha restore still runs after the walk
//! and is then a backstop that finds nothing to undo.

use crate::blend::{
    blend_kind, composite_source_frozen, composite_source_into, paint_pixel, BlendMode,
};
use crate::doc::{DrDocument, Layer};
use crate::doc_lock::EditKind;
use crate::ops::CompositeMode;

/// The canvas-coordinate rect an overlay buffer covers: `width·height`
/// samples, row-major, whose top-left sample sits at canvas `(x, y)` (either
/// may be negative, so an overlay can reach a layer's pixels beyond the
/// canvas). The paint tools' overlays are [`OverlayFrame::canvas`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OverlayFrame {
    pub(crate) x: i64,
    pub(crate) y: i64,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl OverlayFrame {
    /// The canvas itself: origin (0, 0), the canvas size.
    pub(crate) fn canvas(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

/// The paint at one frame sample, as the walk's ink closure answers it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Ink {
    /// A premultiplied RGBA8 overlay sample (the brush, Clone Stamp).
    Premultiplied([u8; 4]),
    /// A straight colour at a coverage alpha (Fill…, the colour brush's
    /// blend modes).
    Straight { rgb: [u8; 3], alpha: u8 },
}

impl Ink {
    /// The premultiplied RGBA8 form Over and Erase composite: a straight
    /// ink's `c' = (c·alpha + 127) / 255`, alpha unchanged — the ONE fill
    /// premultiply, so a Normal fill lands the bytes its overlay always did.
    pub(crate) fn premultiplied(self) -> [u8; 4] {
        match self {
            Ink::Premultiplied(p) => p,
            Ink::Straight { rgb, alpha } => {
                let a = u32::from(alpha);
                let c = |v: u8| ((u32::from(v) * a + 127) / 255) as u8;
                [c(rgb[0]), c(rgb[1]), c(rgb[2]), alpha]
            }
        }
    }

    /// The straight colour Blend composites, in [0, 1]: a straight ink's own
    /// colour exactly; a premultiplied sample divided by its own alpha (`min`
    /// guards malformed colour > alpha; an alpha-0 sample has no colour).
    pub(crate) fn straight(self) -> [f32; 3] {
        match self {
            Ink::Premultiplied(p) if p[3] == 0 => [0.0; 3],
            Ink::Premultiplied(p) => {
                let c = |v: u8| (f32::from(v) / f32::from(p[3])).min(1.0);
                [c(p[0]), c(p[1]), c(p[2])]
            }
            Ink::Straight { rgb, .. } => rgb.map(|v| f32::from(v) / 255.0),
        }
    }
}

/// How the walk composites each ink sample. `Blend` never carries Normal
/// (that IS `Over`) or a group-only mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaintMode {
    Over,
    Erase,
    Blend(BlendMode),
}

/// The layer-local `(x, y, w, h)` bounding box of the `frame` samples for
/// which `nonzero(i)` holds (`i = sy·frame.width + sx`), intersected with the
/// layer's intersection with the FRAME — THE paint clip (the master copy;
/// `layer_rect`'s plane forms mirror it). `None` when no such sample reaches
/// the layer. Every paint walk runs over this rect alone: a sample whose
/// premultiplied ink is all zero changes nothing in any of them.
pub(crate) fn source_rect_in(
    frame: OverlayFrame,
    layer: &Layer,
    nonzero: impl Fn(usize) -> bool,
) -> Option<(u32, u32, u32, u32)> {
    let (lw, lh) = layer.pixels.dimensions();
    let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
    // The frame's origin in layer coordinates.
    let (fx, fy) = (frame.x - off_x, frame.y - off_y);
    let lx0 = fx.max(0);
    let ly0 = fy.max(0);
    let lx1 = (fx + i64::from(frame.width)).min(i64::from(lw));
    let ly1 = (fy + i64::from(frame.height)).min(i64::from(lh));
    if lx0 >= lx1 || ly0 >= ly1 {
        return None;
    }
    let (sx0, sx1) = ((lx0 - fx) as usize, (lx1 - fx) as usize);
    let mut bounds: Option<(usize, usize, i64, i64)> = None;
    for ly in ly0..ly1 {
        let at = (ly - fy) as usize * frame.width as usize;
        let Some(first) = (sx0..sx1).position(|sx| nonzero(at + sx)) else {
            continue;
        };
        let last = (sx0..sx1).rposition(|sx| nonzero(at + sx)).unwrap_or(first);
        bounds = Some(match bounds {
            None => (first, last, ly, ly),
            Some((a, b, top, _)) => (a.min(first), b.max(last), top, ly),
        });
    }
    let (first, last, top, bottom) = bounds?;
    Some((
        (lx0 + first as i64) as u32,
        top as u32,
        (last - first + 1) as u32,
        (bottom - top + 1) as u32,
    ))
}

/// The layer-local bounding box of the canvas-frame overlay samples (`src`,
/// canvas w·h·4 bytes) with ANY nonzero byte: [`overlay_rect_in`] with the
/// canvas frame.
pub(crate) fn overlay_rect(
    src: &[u8],
    (width, height): (u32, u32),
    layer: &Layer,
) -> Option<(u32, u32, u32, u32)> {
    overlay_rect_in(src, OverlayFrame::canvas(width, height), layer)
}

/// [`overlay_rect`] for an overlay laid out over `frame` (`src` is
/// `frame.width·frame.height·4` bytes): [`source_rect_in`] over the nonzero
/// overlay samples.
pub(crate) fn overlay_rect_in(
    src: &[u8],
    frame: OverlayFrame,
    layer: &Layer,
) -> Option<(u32, u32, u32, u32)> {
    source_rect_in(frame, layer, |i| overlay_sample(src, i) != [0; 4])
}

/// Overlay sample `i` (4 bytes at `4i`).
fn overlay_sample(src: &[u8], i: usize) -> [u8; 4] {
    [src[4 * i], src[4 * i + 1], src[4 * i + 2], src[4 * i + 3]]
}

/// Whether `src` is exactly `frame.width·frame.height·4` bytes.
fn frame_sized(src: &[u8], frame: OverlayFrame) -> bool {
    (frame.width as usize)
        .checked_mul(frame.height as usize)
        .and_then(|n| n.checked_mul(4))
        == Some(src.len())
}

impl DrDocument {
    /// THE byte paint walk (module doc): composites `ink` over layer `idx`
    /// through `mode`, its alpha scaled by `alpha` (clamped to [0, 1]), with
    /// `preserve` freezing the layer's alpha for Over and Blend (Erase
    /// ignores it). `ink(i)` takes the frame sample index
    /// `i = sy·frame.width + sx`; the frame is mapped through the layer's
    /// offset, so it can reach pixels beyond the canvas, and the layer never
    /// grows.
    ///
    /// Unlocked: callers wrap it in `under_locks`. Refusals (`None`): a
    /// non-raster `idx`, NaN `alpha`, a layer extent that misses the frame,
    /// and — Blend only — no byte changing. Over and Erase with no nonzero
    /// ink on the layer return the layer reidentified, which the callers'
    /// latches turn into their own no-op answer.
    pub(crate) fn paint_walk(
        &self,
        idx: usize,
        frame: OverlayFrame,
        mode: PaintMode,
        alpha: f32,
        preserve: bool,
        ink: impl Fn(usize) -> Ink,
    ) -> Option<Self> {
        let layer = self.raster_layer(idx)?;
        if alpha.is_nan() {
            return None;
        }
        let a = alpha.clamp(0.0, 1.0);
        let (lw, lh) = layer.pixels.dimensions();
        let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        // The frame's origin in layer coordinates.
        let (fx, fy) = (frame.x - off_x, frame.y - off_y);
        let lx0 = fx.max(0);
        let ly0 = fy.max(0);
        let lx1 = (fx + i64::from(frame.width)).min(i64::from(lw));
        let ly1 = (fy + i64::from(frame.height)).min(i64::from(lh));
        if lx0 >= lx1 || ly0 >= ly1 {
            // The frame cannot reach any layer pixel; fail instead of minting
            // an unchanged copy (which would register a phantom undo step in
            // the app).
            return None;
        }
        let Some((rx, ry, rw, rh)) =
            source_rect_in(frame, layer, |i| ink(i).premultiplied() != [0; 4])
        else {
            return match mode {
                PaintMode::Blend(_) => None,
                _ => self.with_layer_raster(idx, layer.pixels.reidentified()),
            };
        };
        let kind = blend_kind(match mode {
            PaintMode::Blend(m) => m,
            _ => BlendMode::Normal,
        });
        // With the alpha frozen only RGB is written back; the walk's alpha
        // byte is the layer's own.
        let channels = if preserve { 3 } else { 4 };
        let mut px = layer.pixels.read_rect(rx, ry, rw, rh)?;
        let mut changed = false;
        for (row, line) in px.chunks_exact_mut(rw as usize).enumerate() {
            let ly = i64::from(ry) + row as i64;
            for (column, dp) in line.iter_mut().enumerate() {
                let lx = i64::from(rx) + column as i64;
                let (sx, sy) = ((lx - fx) as u64, (ly - fy) as u64);
                let sample = ink((sy * u64::from(frame.width) + sx) as usize);
                match mode {
                    PaintMode::Erase => {
                        paint_pixel(&mut dp[..], sample.premultiplied(), CompositeMode::Erase, a);
                        continue;
                    }
                    PaintMode::Over if !preserve || dp[3] == 255 => {
                        paint_pixel(&mut dp[..], sample.premultiplied(), CompositeMode::Over, a);
                        continue;
                    }
                    // Nothing there to recolour, and the alpha may not move.
                    PaintMode::Over if dp[3] == 0 => continue,
                    PaintMode::Over | PaintMode::Blend(_) => {}
                }
                let coverage = sample.premultiplied()[3];
                if coverage == 0 {
                    // Nothing painted here; a premultiplied sample with junk
                    // colour bytes at alpha 0 contributes nothing either.
                    continue;
                }
                let sa = (f32::from(coverage) / 255.0) * a;
                if sa <= 0.0 {
                    continue;
                }
                let mut acc = dp.map(|v| f32::from(v) / 255.0);
                // Canvas-absolute, so the dither is the projection's; a pixel
                // beyond the canvas has a negative coordinate, which
                // `dissolve_threshold` wraps.
                let xy = (lx + off_x, ly + off_y);
                if preserve {
                    composite_source_frozen(&mut acc, sample.straight(), sa, kind, xy);
                } else {
                    let acc = std::slice::from_mut(&mut acc);
                    composite_source_into(acc, 0, sample.straight(), sa, kind, xy);
                }
                for c in 0..channels {
                    let v = (acc[c].clamp(0.0, 1.0) * 255.0).round() as u8;
                    if dp[c] != v {
                        dp[c] = v;
                        changed = true;
                    }
                }
            }
        }
        if let PaintMode::Blend(_) = mode {
            if !changed {
                return None;
            }
        }
        let pixels = match layer.pixels.with_rect(rx, ry, rw, rh, &px) {
            Err(_) => return None,
            Ok(None) => layer.pixels.reidentified(),
            Ok(Some(pixels)) => pixels,
        };
        self.with_layer_raster(idx, pixels)
    }

    /// Paints a canvas-frame PREMULTIPLIED RGBA8 overlay (`src`, exactly
    /// canvas w*h*4 bytes, row-major with no padding) onto layer `idx`,
    /// mapped through the layer's offset. `alpha` is clamped to [0, 1] and
    /// scales the overlay's alpha (NaN is refused); OVER paints the overlay
    /// over the layer, ERASE uses the overlay's alpha to erase layer alpha
    /// and ignores its colour, and a fully transparent overlay pixel passes
    /// the layer's bytes through exactly. Overlay areas outside the
    /// layer's extent are ignored; if the layer's extent does not intersect
    /// the canvas at all (no pixel could change) the paint is refused
    /// (`None`) rather than returning an unchanged copy, and so is a GROUP
    /// index (a group has no pixels of its own). A PIXELS edit under
    /// `doc_lock`: a pixel-locked layer refuses it, and on a
    /// transparency-locked one an Over stroke keeps every pixel's alpha and
    /// mixes into a semi-transparent pixel's colour at its own strength.
    /// (`pub(crate)` because `CompositeMode` is a crate-private type;
    /// the public surface is `dr_doc_painting_layer`.)
    pub(crate) fn painting_layer(
        &self,
        idx: usize,
        src: &[u8],
        mode: CompositeMode,
        alpha: f32,
    ) -> Option<Self> {
        let frame = OverlayFrame::canvas(self.width, self.height);
        self.painting_layer_in(idx, src, frame, mode, alpha)
    }

    /// [`Self::painting_layer`] for an overlay laid out over `frame` (`src`
    /// exactly `frame.width·frame.height·4` bytes, its top-left sample at
    /// canvas `(frame.x, frame.y)`), so it can reach a layer's pixels beyond
    /// the canvas; the extent-miss refusal is against the frame.
    pub(crate) fn painting_layer_in(
        &self,
        idx: usize,
        src: &[u8],
        frame: OverlayFrame,
        mode: CompositeMode,
        alpha: f32,
    ) -> Option<Self> {
        let mode = match mode {
            CompositeMode::Over => PaintMode::Over,
            CompositeMode::Erase => PaintMode::Erase,
        };
        self.under_locks(idx, EditKind::Pixels, |doc| {
            if !frame_sized(src, frame) {
                return None;
            }
            doc.paint_walk(idx, frame, mode, alpha, doc.alpha_frozen(idx), |i| {
                Ink::Premultiplied(overlay_sample(src, i))
            })
        })
    }

    /// Paints a canvas-frame PREMULTIPLIED RGBA8 overlay (`src`, exactly
    /// canvas w*h*4 bytes) onto layer `idx` through blend mode `mode`,
    /// scaled by `alpha` (clamped to [0, 1]) — the overlay-ink form, which
    /// Clone Stamp's Blend option uses.
    ///
    /// `BlendMode::Normal` IS source-over, so it delegates to
    /// [`DrDocument::painting_layer`] — keeping the classic path the single
    /// implementation and the two byte-identical. Every other mode
    /// un-premultiplies each covered overlay pixel back to straight color
    /// and composites it with the W3C formula (`sa*(1-ab)*Cs + sa*ab*B(Cb,
    /// Cs) + (1-sa)*ab*Cb`, all in f32) onto the layer's straight pixel;
    /// Dissolve keeps its canvas-absolute deterministic dither, so a
    /// dissolve stroke lands the same speckle a dissolve layer would.
    ///
    /// Domain refusals (`None`): `idx` out of range; NaN `alpha`; a Pass
    /// Through `mode` (a group's declaration, not a blend function —
    /// [`BlendMode::is_group_only`]); `src` not
    /// exactly canvas-sized; a layer extent that misses the canvas; or —
    /// for every mode but Normal — no pixel actually changing (e.g.
    /// multiply by white), since an identical copy would register a
    /// phantom undo step in the app. Normal delegates wholesale, so it
    /// keeps `painting_layer`'s behavior: byte-identical output and a
    /// refusal only on the extent miss, never on a no-change overlay.
    /// (`pub(crate)` like `painting_layer`; the public surface is
    /// `dr_doc_painting_layer_blend`.)
    pub(crate) fn painting_layer_blend(
        &self,
        idx: usize,
        src: &[u8],
        mode: BlendMode,
        alpha: f32,
    ) -> Option<Self> {
        let frame = OverlayFrame::canvas(self.width, self.height);
        self.painting_layer_blend_in(idx, src, frame, mode, alpha)
    }

    /// [`Self::painting_layer_blend`] for an overlay laid out over `frame`
    /// (`src` exactly `frame.width·frame.height·4` bytes), which may reach a
    /// layer's pixels beyond the canvas. The refusals read "the frame" for
    /// "the canvas"; Dissolve's dither stays canvas-absolute.
    pub(crate) fn painting_layer_blend_in(
        &self,
        idx: usize,
        src: &[u8],
        frame: OverlayFrame,
        mode: BlendMode,
        alpha: f32,
    ) -> Option<Self> {
        if mode == BlendMode::Normal {
            return self.painting_layer_in(idx, src, frame, CompositeMode::Over, alpha);
        }
        if mode.is_group_only() {
            return None;
        }
        self.under_locks(idx, EditKind::Pixels, |doc| {
            if !frame_sized(src, frame) {
                return None;
            }
            let mode = PaintMode::Blend(mode);
            doc.paint_walk(idx, frame, mode, alpha, doc.alpha_frozen(idx), |i| {
                Ink::Premultiplied(overlay_sample(src, i))
            })
        })
    }

    /// The colour brush's Blend option: [`Self::painting_layer_blend`] with
    /// the STRAIGHT ink `rgb` and only the canvas-frame overlay's alpha
    /// (`src[4i + 3]`) read, as coverage — so a low-coverage stroke blends the
    /// exact colour, not one un-premultiplied out of a few overlay codes.
    /// Normal delegates to [`Self::painting_layer`] with the overlay
    /// itself (premultiplied, byte-identical); the refusals are
    /// `painting_layer_blend`'s. (`pub(crate)`; the public surface is
    /// `dr_doc_painting_layer_blend_color`.)
    pub(crate) fn painting_layer_blend_color(
        &self,
        idx: usize,
        src: &[u8],
        rgb: [u8; 3],
        mode: BlendMode,
        alpha: f32,
    ) -> Option<Self> {
        if mode == BlendMode::Normal {
            return self.painting_layer(idx, src, CompositeMode::Over, alpha);
        }
        if mode.is_group_only() {
            return None;
        }
        let frame = OverlayFrame::canvas(self.width, self.height);
        self.under_locks(idx, EditKind::Pixels, |doc| {
            if !frame_sized(src, frame) {
                return None;
            }
            let mode = PaintMode::Blend(mode);
            doc.paint_walk(idx, frame, mode, alpha, doc.alpha_frozen(idx), |i| {
                Ink::Straight {
                    rgb,
                    alpha: src[4 * i + 3],
                }
            })
        })
    }
}
