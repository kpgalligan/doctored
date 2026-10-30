//! Edit ▸ Fill… on the document: one solid colour, through a blend mode and
//! an opacity, over the selection's coverage (or the whole canvas).
//!
//! There is no blend math here. The fill is the colour at each pixel's
//! selection coverage, handed as a straight INK to
//! [`DrDocument::paint_walk`] — the paint tools' one compositing walk — under
//! the layer's locks. No canvas-sized overlay is built, and a non-Normal fill
//! blends the exact colour, not one un-premultiplied out of a low-coverage
//! byte.
//!
//! The layer never grows: the walk is clipped to the layer's own buffer
//! (the painting path's clipping), so a fill on a smaller, offset layer
//! paints only the pixels that layer already has.
//!
//! Preserve Transparency (and a transparency lock) is the frozen-alpha rule:
//! the walk leaves an alpha-0 pixel alone and gives a semi-transparent one
//! `lerp(Cb, B(Cb, Cs), sa)` at its own alpha — the fill at its opacity ×
//! coverage, not boosted. `doc_lock`'s one alpha restore
//! ([`DrDocument::restoring_alpha`]) still runs after it, as a backstop.
//!
//! Edit ▸ Stroke… is [`stroke_mask`] plus that same fill: the
//! selection is turned into a canvas-sized band —
//! `style_plane_ops::outside_stroke_band` / `inside_stroke_band`; Layer
//! Style ▸ Stroke shares the inside band, and its outside band differs from
//! this one only at corners, where it takes Photoshop's chamfer distance
//! (`style_stroke_distance`, #2) — and the band is the mask handed to
//! [`DrDocument::fill_selection`]. The band is therefore clipped to the
//! layer's own pixels like any fill: on a smaller or offset layer the part
//! of the band outside its buffer is dropped.
//!
//! The canvas edge: Edit ▸ Stroke… treats the canvas boundary as a contour
//! for Inside (and Center's inner half) — a virtual outside ring one pixel
//! beyond the canvas — so Select All ▸ Stroke Inside draws a frame. The
//! rule is Edit ▸ Stroke-only: Grow/Shrink/Border
//! (`doc_select`) and Layer Style ▸ Stroke keep their own (the canvas edge
//! is not a contour there).
//!
//! With nothing selected, Edit ▸ Stroke… outlines the layer's OWN
//! alpha edge in LAYER space ([`DrDocument::stroke_layer_edge`]), including
//! any part of it beyond the canvas. The layer's alpha > 0 bounds `B` (the
//! mask excluded), grown by `ceil(width) + 2` on every side, is the frame
//! `F`: a u8 plane of `F` holds the layer's alpha over `B` and 0 elsewhere,
//! [`stroke_mask`] turns it into the band unchanged, and the band is filled
//! through the same fill laid out over `F` (`paint_walk` over that frame).
//! When the layer's alpha is free to change (no Preserve Transparency, no
//! transparency lock) the layer first GROWS to its union with the band's
//! bounds — which may reach past the canvas, as a layer keeps its
//! off-canvas pixels — through `retouch_transform::grown_layer`, the retouch
//! expansion's own growth (comps follow it). The selection path above is
//! unchanged. The alpha bounds are ONE scan per layer allocation: they come
//! from `doc_group_query::alpha_edge_bounds`, the content-box memo with a
//! mask-less key, so the host's scratch queries and the stroke share it, and
//! a stroke polls its job once per row of it, so a cancel lands inside the
//! scan and a cancelled scan caches nothing. The stroke then copies the
//! alpha over `B` row by row from the layer's own runs, with no RGBA
//! intermediate.
//!
//! `fill_selection` hands one straight colour at the selection's coverage, as an INK, to
//! `doc_paint`'s `paint_walk` (the brush's and Clone Stamp's one compositing walk, under
//! `under_locks`) — no blend math and no canvas-sized overlay of its own, and the exact colour in
//! every blend mode; Preserve Transparency (and a transparency lock) is the walk's
//! frozen-alpha rule (`blend::composite_source_frozen`), with `doc_lock::restoring_alpha`,
//! the one alpha restore `freezing_alpha` also calls, run after it as a backstop. The layer never
//! grows; an unchanged result is `None`. Edit ▸ Stroke… is `stroke_mask` /
//! `dr_selection_stroke`: the selection mask rewritten in place as the stroke bands
//! (`style_plane_ops::outside_stroke_band` / `inside_stroke_band`), with the canvas edge as a
//! contour for Inside (an Edit ▸ Stroke-only rule; `coverage_byte` gives the ring), then filled
//! through `fill_selection`. With nothing selected `stroke_layer_edge` /
//! `dr_doc_stroke_layer_edge` (scratch `dr_doc_stroke_layer_edge_scratch`) strokes the layer's OWN
//! alpha edge in LAYER space, beyond the canvas included: the alpha > 0 bounds
//! (`doc_group_query::alpha_edge_bounds`: the content-box memo with a mask-less key, polled per
//! row, shared with the scratch query) grown by `ceil(width) + 2` is the frame `F`,
//! `stroke_mask` runs unchanged on a u8 plane of `F` (its canvas-edge ring is inert there), and the
//! band is filled over `F` through the same walk (`doc_paint::OverlayFrame`; the canvas callers
//! pass `OverlayFrame::canvas`). Unless Preserve Transparency or a transparency lock keeps the
//! alpha, the layer first grows to its union with the band's bounds — past the canvas if need be —
//! through `retouch_transform::grown_layer` (`dr_doc_expand_retouch_destination` uses it too),
//! whose comp shift is `comps_geometry`'s `comps_after_growth`, the one rule every origin-moving
//! storage growth calls at both depths (`comps_geometry`'s module doc lists them). A
//! retained-source layer refuses by name. The 16-bit twins are `precision_fill_stroke.rs`, the
//! selection-plane `stroke` kind and native paint's `preserve_alpha`.

use crate::blend::BlendMode;
use crate::doc::{DrDocument, Layer};
use crate::doc_group_query::{alpha_edge_bounds, edge_bounds, runs_extent};
use crate::doc_lock::EditKind;
use crate::doc_paint::{Ink, OverlayFrame, PaintMode};
use crate::doc_select::coverage_byte;
use crate::job_control::JobContext;
use crate::retouch_transform::{grown_layer, merged_extent, StorageExtent};
use crate::style::StrokePosition;
use crate::style_plane_ops::{inside_stroke_band, outside_stroke_band};

/// Replaces a canvas-sized u8 selection `mask` in place with the Edit ▸
/// Stroke… band `width_px` wide at `location` (module doc), like the
/// `dr_selection_*` family:
///
/// - Outside: `dilate(mask, w) × (255 − mask)`.
/// - Inside: `mask − erode(mask, w)`, where the erosion also sees the canvas
///   edge as a contour (a virtual outside ring one pixel beyond the canvas),
///   so a selection touching the edge gets a band along it.
/// - Center: Outside at `w/2` plus Inside at `w/2`.
///
/// The band is later clipped to the target layer's own pixels by the fill.
/// A zero dimension, a length ≠ `width·height`, or a non-finite width is a
/// no-op; `width_px <= 0` zeroes the mask (an untouched mask would fill the
/// whole selection).
pub fn stroke_mask(
    mask: &mut [u8],
    width: u32,
    height: u32,
    width_px: f32,
    location: StrokePosition,
) {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || mask.len() != w * h || !width_px.is_finite() {
        return;
    }
    if width_px <= 0.0 {
        mask.fill(0);
        return;
    }
    let band = match location {
        StrokePosition::Outside => outside(mask, width, height, width_px),
        StrokePosition::Inside => inside(mask, width, height, width_px),
        StrokePosition::Center => {
            let half = width_px / 2.0;
            let mut band = outside(mask, width, height, half);
            for (b, v) in band.iter_mut().zip(inside(mask, width, height, half)) {
                *b = b.saturating_add(v);
            }
            band
        }
    };
    mask.copy_from_slice(&band);
}

/// The shared outside band, uncancellable.
fn outside(mask: &[u8], width: u32, height: u32, px: f32) -> Vec<u8> {
    outside_stroke_band(mask, width, height, px, JobContext::NONE).expect("uncancellable context")
}

/// The shared inside band, joined with the canvas-edge ring (module doc):
/// `max(band, mask − coverage_byte((k + 1) − 0.5 − px))`, `k` the pixel's
/// distance in whole pixels to the nearest canvas edge. `(k + 1) − 0.5` is
/// the signed distance a virtual outside ring one pixel beyond the canvas
/// gives, and since `coverage_byte` is monotone and
/// `c − min(a, b) = max(c − a, c − b)` the result is exactly the erosion
/// under that ring.
fn inside(mask: &[u8], width: u32, height: u32, px: f32) -> Vec<u8> {
    let mut band = inside_stroke_band(mask, width, height, px, JobContext::NONE)
        .expect("uncancellable context");
    let (w, h) = (width as usize, height as usize);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let k = x.min(w - 1 - x).min(y).min(h - 1 - y);
            let ring = mask[i].saturating_sub(coverage_byte((k + 1) as f32 - 0.5 - px));
            band[i] = band[i].max(ring);
        }
    }
    band
}

/// The sentence a retained-source layer's layer-edge stroke refuses with.
const RETAINED_SOURCE: &str = "Rasterize the retained source before stroking its pixels";

/// One layer's edge (module doc): the layer, its alpha > 0 bounds `B`
/// (layer space, `(x0, y0, x1, y1)` exclusive) and the frame `F` — or why `F`
/// cannot be represented.
struct LayerEdge<'a> {
    layer: &'a Layer,
    bounds: (u32, u32, u32, u32),
    frame: Result<(OverlayFrame, StorageExtent), String>,
}

impl LayerEdge<'_> {
    /// The working memory a stroke of this edge reserves:
    /// `24·|F| + 4·|layer rect ∪ F| + 4096` bytes, saturating; `u64::MAX`
    /// when the frame or the union cannot be represented.
    fn scratch(&self) -> u64 {
        let Ok((frame, rect)) = &self.frame else {
            return u64::MAX;
        };
        let current = (self.layer.pixels.dimensions(), self.layer.offset);
        let Ok(((mw, mh), _)) = merged_extent(current, *rect) else {
            return u64::MAX;
        };
        let f = u64::from(frame.width) * u64::from(frame.height);
        let m = u64::from(mw) * u64::from(mh);
        f.saturating_mul(24)
            .saturating_add(m.saturating_mul(4))
            .saturating_add(4096)
    }
}

/// `F`: `bounds` grown by `pad` on every side, in canvas coordinates (the
/// layer sits at `offset`), as an overlay frame and as a storage extent.
fn edge_frame(
    offset: (i32, i32),
    (x0, y0, x1, y1): (u32, u32, u32, u32),
    pad: i64,
) -> Result<(OverlayFrame, StorageExtent), String> {
    let too_large = || "the stroke's frame exceeds the supported size".to_string();
    let x = (i64::from(offset.0) + i64::from(x0)).saturating_sub(pad);
    let y = (i64::from(offset.1) + i64::from(y0)).saturating_sub(pad);
    let side = |span: u32| i64::from(span).saturating_add(pad.saturating_mul(2));
    let width = u32::try_from(side(x1 - x0)).map_err(|_| too_large())?;
    let height = u32::try_from(side(y1 - y0)).map_err(|_| too_large())?;
    crate::precision_image::pixel_count(width, height).map_err(|_| too_large())?;
    let origin = (
        i32::try_from(x).map_err(|_| too_large())?,
        i32::try_from(y).map_err(|_| too_large())?,
    );
    let frame = OverlayFrame {
        x,
        y,
        width,
        height,
    };
    Ok((frame, ((width, height), origin)))
}

impl DrDocument {
    /// Fills layer `idx` with straight `rgba` through `mode` at `opacity`
    /// (clamped to [0, 1]), weighted by `mask` — a canvas-sized u8 selection
    /// coverage buffer, or `None` for the whole canvas. With
    /// `preserve_transparency` (or a transparency lock) every pixel keeps its
    /// alpha and a semi-transparent one mixes the colour in at the fill's own
    /// opacity × coverage (module doc).
    ///
    /// Domain refusals (`None`): `idx` out of range or not a raster layer; a
    /// `mask` that is not exactly canvas w·h bytes; NaN `opacity`; a
    /// group-only `mode` (Pass Through); a pixel-locked layer; a layer extent
    /// that misses the canvas; and — since an identical copy would register a
    /// phantom undo step — any fill that leaves the layer's bytes unchanged
    /// (an empty mask, a same-colour Normal fill, a preserved fill over a
    /// fully transparent area).
    pub fn fill_selection(
        &self,
        idx: usize,
        mask: Option<&[u8]>,
        rgba: [u8; 4],
        mode: BlendMode,
        opacity: f32,
        preserve_transparency: bool,
    ) -> Option<Self> {
        let frame = OverlayFrame::canvas(self.width, self.height);
        self.fill_in(idx, mask, frame, rgba, mode, opacity, preserve_transparency)
    }

    /// [`Self::fill_selection`] with the mask and the walk laid out over
    /// `frame` (`mask` exactly `frame.width·frame.height` bytes) instead of
    /// the canvas; the same refusals, read against the frame.
    #[allow(clippy::too_many_arguments)]
    fn fill_in(
        &self,
        idx: usize,
        mask: Option<&[u8]>,
        frame: OverlayFrame,
        rgba: [u8; 4],
        mode: BlendMode,
        opacity: f32,
        preserve_transparency: bool,
    ) -> Option<Self> {
        self.raster_layer(idx)?;
        let pixels = (frame.width as usize).checked_mul(frame.height as usize)?;
        if mask.is_some_and(|m| m.len() != pixels) || opacity.is_nan() || mode.is_group_only() {
            return None;
        }
        let paint = if mode == BlendMode::Normal {
            PaintMode::Over
        } else {
            PaintMode::Blend(mode)
        };
        let rgb = [rgba[0], rgba[1], rgba[2]];
        let mut out = self.under_locks(idx, EditKind::Pixels, |doc| {
            let frozen = preserve_transparency || doc.alpha_frozen(idx);
            doc.paint_walk(idx, frame, paint, opacity, frozen, |i| {
                let coverage = mask.map_or(255u32, |m| u32::from(m[i]));
                Ink::Straight {
                    rgb,
                    alpha: ((u32::from(rgba[3]) * coverage + 127) / 255) as u8,
                }
            })
        })?;
        if preserve_transparency {
            out = self.restoring_alpha(idx, out)?;
        }
        if out.layers[idx].pixels.same_bytes(&self.layers[idx].pixels) {
            return None;
        }
        Some(out)
    }

    /// Layer `idx`'s edge (module doc), or `Ok(None)` for an index that is
    /// out of range or not raster, a width that is non-finite or ≤ 0, or a
    /// layer with no alpha > 0 pixel. The bounds are
    /// [`alpha_edge_bounds`]'s memoized scan, polled by `job`; `Err` is a
    /// cancel.
    fn layer_edge(
        &self,
        idx: usize,
        width_px: f32,
        job: JobContext<'_>,
    ) -> Result<Option<LayerEdge<'_>>, String> {
        let Some(layer) = self.raster_layer(idx) else {
            return Ok(None);
        };
        if !width_px.is_finite() || width_px <= 0.0 {
            return Ok(None);
        }
        let Some(bounds) = alpha_edge_bounds(layer, job)? else {
            return Ok(None);
        };
        let pad = (width_px.ceil() as i64).saturating_add(2);
        Ok(Some(LayerEdge {
            layer,
            bounds,
            frame: edge_frame(layer.offset, bounds, pad),
        }))
    }

    /// The working memory [`Self::stroke_layer_edge`] reserves:
    /// `24·|F| + 4·|layer rect ∪ F| + 4096` bytes, saturating. 0 for an index
    /// that is out of range or not raster, a width that is non-finite or
    /// ≤ 0, or a layer with no alpha > 0 pixel (nothing to outline);
    /// `u64::MAX` when the frame or the union cannot be represented.
    pub(crate) fn stroke_layer_edge_scratch(&self, idx: usize, width_px: f32) -> u64 {
        self.layer_edge(idx, width_px, JobContext::NONE)
            .ok()
            .flatten()
            .map_or(0, |e| e.scratch())
    }

    /// Edit ▸ Stroke… with nothing selected (module doc): layer
    /// `idx`'s own alpha edge stroked `width_px` wide at `location` in LAYER
    /// space, with straight `rgba` through `mode` at `opacity` (clamped to
    /// [0, 1]). The layer grows to its union with the band's bounds — past
    /// the canvas if the band reaches there — unless `preserve_transparency`
    /// or a transparency lock keeps its alpha, in which case nothing grows.
    ///
    /// `Err`: an index that is out of range or not raster; a layer bound to a
    /// retained source ("Rasterize the retained source before stroking its
    /// pixels"); a pixel-locked layer; a width that is non-finite or ≤ 0; NaN
    /// `opacity`; a group-only `mode`; the frame past the supported size; a
    /// reservation refusal or cancellation. `Ok(None)`: a layer with no
    /// alpha > 0 pixel, an empty band, or a result byte-identical to the
    /// (grown) input.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn stroke_layer_edge(
        &self,
        idx: usize,
        width_px: f32,
        location: StrokePosition,
        rgba: [u8; 4],
        mode: BlendMode,
        opacity: f32,
        preserve_transparency: bool,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let layer = self
            .raster_layer(idx)
            .ok_or("Stroke… outlines a raster layer; this index names none")?;
        if layer.retained_source.is_some() {
            return Err(RETAINED_SOURCE.into());
        }
        if self.lock_block(idx, EditKind::Pixels) != 0 {
            return Err("The layer's pixels are locked".into());
        }
        if !width_px.is_finite() || width_px <= 0.0 {
            return Err("The stroke width must be a finite number of pixels above 0".into());
        }
        if opacity.is_nan() {
            return Err("The stroke opacity must be a number".into());
        }
        if mode.is_group_only() {
            return Err("Pass Through is a group's mode, not a stroke blend mode".into());
        }
        let edge = self.layer_edge(idx, width_px, job)?;
        job.reserve(edge.as_ref().map_or(0, LayerEdge::scratch))?;
        let Some(edge) = edge else {
            return Ok(None);
        };
        let (frame, _) = edge.frame?;
        let (x0, y0, x1, y1) = edge.bounds;
        let fw = frame.width as usize;
        // B's top-left in the frame: exactly the pad, by construction.
        let px0 = (i64::from(layer.offset.0) + i64::from(x0) - frame.x) as usize;
        let py0 = (i64::from(layer.offset.1) + i64::from(y0) - frame.y) as usize;
        let mut plane = vec![0u8; fw * frame.height as usize];
        for y in y0..y1 {
            job.check()?;
            let row = (py0 + (y - y0) as usize) * fw + px0;
            for (x, run) in layer.pixels.row_segments(y, x0, x1) {
                let at = row + (x - x0) as usize;
                for (dst, p) in plane[at..at + run.len()].iter_mut().zip(run) {
                    *dst = p[3];
                }
            }
        }
        // `stroke_mask`'s canvas-edge ring is inert here: every nonzero
        // sample sits at least `ceil(width) + 2` pixels from F's edge, so the
        // ring's coverage there is `coverage_byte(≥ 2.5) = 255` and it
        // subtracts everything; and every band pixel, which lies within
        // `width + 0.5` of the contour, falls inside F.
        stroke_mask(&mut plane, frame.width, frame.height, width_px, location);
        let Some((bx0, by0, bx1, by1)) =
            edge_bounds(frame.width, frame.height, job, |y, x0, x1| {
                let row = y as usize * fw;
                let run = &plane[row + x0 as usize..row + x1 as usize];
                runs_extent(std::iter::once((x0, run)), |_, v| *v != 0)
            })?
        else {
            return Ok(None);
        };
        job.check()?;
        let grow = !preserve_transparency && !self.alpha_frozen(idx);
        let grown;
        let base = if grow {
            let too_large = || "the stroke's frame exceeds the supported size".to_string();
            let band: StorageExtent = (
                (bx1 - bx0, by1 - by0),
                (
                    i32::try_from(frame.x + i64::from(bx0)).map_err(|_| too_large())?,
                    i32::try_from(frame.y + i64::from(by0)).map_err(|_| too_large())?,
                ),
            );
            let current = (layer.pixels.dimensions(), layer.offset);
            let extent = merged_extent(current, band)?;
            if extent == current {
                self
            } else {
                grown = grown_layer(self, idx, extent, job)?;
                &grown
            }
        } else {
            self
        };
        job.check()?;
        Ok(base.fill_in(
            idx,
            Some(&plane),
            frame,
            rgba,
            mode,
            opacity,
            preserve_transparency,
        ))
    }
}
