//! Exact pixel writes for the agent's `put_pixels` (pixel art): one canvas
//! rect of straight RGBA8 samples written onto one raster layer with REPLACE
//! semantics. A written sample takes all four bytes verbatim — no blend, no
//! coverage weighting — so alpha 0 writes transparency and alpha 128 writes
//! exactly 128. An optional per-sample mask says which samples of the rect
//! are written; the rest of the layer keeps every byte.
//!
//! The rect is in CANVAS pixels and is mapped through the layer offset with
//! the shared `layer_rect` clip; samples that fall outside the layer's pixel
//! rect are ignored and the layer never grows (the host grows it first when
//! it must). The write goes through `with_layer_raster`, so it is lock-gated
//! like every pixel writer and a transparency lock restores alpha.
//!
//! `put_pixels` writes one canvas rect of straight RGBA8 onto one raster layer with REPLACE
//! semantics (a written sample takes all four bytes, no blend; an optional per-sample mask says
//! which are written), clipped through `layer_rect`, written back through `LayerRaster::with_rect`
//! and gated by `with_layer_raster` (locks; a transparency lock restores alpha). The layer never
//! grows; no byte changed is `None`. Tests: `tests/pixel_art.rs`.

use crate::doc::DrDocument;
use crate::layer_rect::{canvas_rect_on_layer, rect_image};

impl DrDocument {
    /// Writes `rgba` (`w * h * 4` straight RGBA8 bytes, row-major, row 0 top)
    /// over the canvas rect `(x, y, w, h)` of raster layer `idx`, replacing
    /// every sample where `mask` is `None` or its byte is nonzero (`mask`:
    /// `w * h` bytes). Samples outside the layer's pixel rect are ignored.
    ///
    /// `None` for a non-raster or out-of-range `idx`, an empty rect, a rect
    /// not fully inside the canvas, an `rgba` or `mask` of the wrong length,
    /// a rect that misses the layer, a pixel-locked layer, or a write that
    /// changes no byte (after a transparency lock restores alpha).
    pub fn put_pixels(
        &self,
        idx: usize,
        rect: (u32, u32, u32, u32),
        rgba: &[u8],
        mask: Option<&[u8]>,
    ) -> Option<Self> {
        let layer = self.raster_layer(idx)?;
        let (x, y, w, h) = rect;
        if w == 0
            || h == 0
            || u64::from(x) + u64::from(w) > u64::from(self.width)
            || u64::from(y) + u64::from(h) > u64::from(self.height)
        {
            return None;
        }
        let count = (w as usize).checked_mul(h as usize)?;
        if rgba.len() != count.checked_mul(4)? || mask.is_some_and(|m| m.len() != count) {
            return None;
        }
        let (x0, y0) = (i64::from(x), i64::from(y));
        let on_layer = canvas_rect_on_layer(
            (self.width, self.height),
            layer,
            (x0, y0, x0 + i64::from(w), y0 + i64::from(h)),
        )?;
        let (lx, ly, lw, lh) = on_layer;
        let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        let mut pixels = rect_image(&layer.pixels, on_layer)?;
        for (px_x, px_y, px) in pixels.enumerate_pixels_mut() {
            // The layer sample's canvas position, then its index in the
            // caller's rect. `canvas_rect_on_layer` clipped to that rect, so
            // both differences are in 0..w and 0..h.
            let sx = (i64::from(lx + px_x) + off_x - x0) as usize;
            let sy = (i64::from(ly + px_y) + off_y - y0) as usize;
            let i = sy * w as usize + sx;
            if mask.is_some_and(|m| m[i] == 0) {
                continue;
            }
            px.0.copy_from_slice(&rgba[i * 4..i * 4 + 4]);
        }
        let raw = pixels.into_raw();
        let raster = match layer
            .pixels
            .with_rect(lx, ly, lw, lh, raw.as_chunks::<4>().0)
        {
            Ok(Some(raster)) => raster,
            Ok(None) | Err(_) => return None,
        };
        self.with_layer_raster(idx, raster)
    }
}
