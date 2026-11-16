//! KERNEL: brush dynamics' TEXTURE, a pattern tile that modulates each dab's
//! coverage through Depth (`designs/brush-dynamics.md`, slice 3). The
//! rasterizer that applies it is `brush_coverage` (THE TEXTURED STROKE); the
//! handle is `ffi_brush_texture`.
//!
//! # The rule
//!
//! `t(x, y)` at canvas pixel (x, y) is the Gray luminosity (`blend::lum`) of
//! the placed tile's straight RGB times its alpha: [`PatternSampler::at`] over
//! `pattern_tile::tile_level(tile, scale, 0.0)` with the tile's origin at the
//! canvas origin (0, 0), read clamped (never `linear`). It is the read
//! `style_texture::texture_heights_with_job` makes, the master copy of that one
//! expression. Only the tile's RGBA8 raster is read: a tile's native samples
//! (`rgb16`, `rgb32`, `cmyk16`) are dropped before the level is built, so one
//! texture reads the same numbers at every depth and colour mode.
//!
//! Each dab's coverage at a written pixel is multiplied by
//! `1 − depth · (1 − t)` ([`BrushTexture::factor`]) before the stroke's
//! source-over accumulation: `c' = c · d[3] · (1 − depth · (1 − t))`, then
//! `old += c'·(1 − old)` and the stroke's one quantization, unchanged. Depth
//! scales how far a dark texel holds paint back: at depth 1 a black or
//! transparent texel takes no paint and a white opaque one takes the dab's
//! whole coverage; at depth 0 the factor is exactly 1. An opaque white tile's
//! `t` is exactly 1 (`lum(1, 1, 1)` rounds to 1.0 in f32), so depth 0 and a
//! white tile both give the plain stroke bit for bit. The factor lies in
//! `1 − depth … 1`, so a texture can only thin a stroke.
//!
//! Scale is the PYRAMID's (`pattern_tile`'s one sampler), from [`MIN_SCALE`]
//! to [`MAX_SCALE`]; there is no angle. A pixel or aliased request refuses a
//! texture ([`TEXTURED_TIP_REFUSAL`]): a snapped square or a Pencil dab is a
//! hard two-valued mark that a texture would turn into a third value.
use crate::blend::lum;
use crate::pattern_tile::{tile_level, PatternSampler, PatternTile, MAX_TILE_SIDE};
use std::sync::Arc;

/// The smallest texture scale: the Pattern Overlay's floor (`mip_level`'s
/// six halvings at most).
pub const MIN_SCALE: f64 = 0.01;
/// The largest texture scale: a tile drawn ten times its size.
pub const MAX_SCALE: f64 = 10.0;
/// The texture's refusal: a tile side outside 1…512, a scale outside
/// 0.01…10 or a depth outside 0…1 (not finite included), and a NULL tile or
/// handle.
pub const TEXTURE_REFUSAL: &str = "native brush texture holds a tile of 1 to 512 pixels a side, a scale from 0.01 to 10 and a depth from 0 to 1";
/// A texture on a pixel or aliased request (module doc).
pub const TEXTURED_TIP_REFUSAL: &str =
    "native brush texture requires a stamped tip that is not aliased";

/// A texture placed on the canvas: the tile's PYRAMID level at its scale,
/// anchored at the canvas origin, and the depth (module doc).
pub struct BrushTexture {
    sampler: PatternSampler,
    depth: f64,
}

impl BrushTexture {
    /// The texture of `tile`'s RGBA8 raster at `scale` and `depth`; refuses
    /// with [`TEXTURE_REFUSAL`] a side outside 1…512, a raster of any other
    /// length than `width · height · 4`, and a scale or depth out of range.
    pub fn new(tile: &PatternTile, scale: f64, depth: f64) -> Result<Self, String> {
        let sides = 1..=MAX_TILE_SIDE;
        // A NaN lies in neither closed range.
        if !sides.contains(&tile.width)
            || !sides.contains(&tile.height)
            || tile.rgba.len() != tile.width as usize * tile.height as usize * 4
            || !(MIN_SCALE..=MAX_SCALE).contains(&scale)
            || !(0.0..=1.0).contains(&depth)
        {
            return Err(TEXTURE_REFUSAL.into());
        }
        // The raster only (module doc): without samples `tile_level` builds
        // the level from the RGBA8 bytes.
        let raster = PatternTile {
            samples: None,
            ..tile.clone()
        };
        Ok(Self {
            sampler: PatternSampler {
                level: Arc::new(tile_level(&raster, scale, 0.0)),
                origin: (0.0, 0.0),
                linear: false,
            },
            depth,
        })
    }

    /// `1 − depth·(1 − t)` at canvas pixel (x, y), in 0…1 (module doc).
    pub fn factor(&self, x: i64, y: i64) -> f64 {
        let p = self.sampler.at(x, y);
        // `style_texture::texture_heights_with_job`'s read, its master copy.
        let t = f64::from(lum([p[0], p[1], p[2]]) * p[3]);
        1.0 - self.depth * (1.0 - t)
    }
}
