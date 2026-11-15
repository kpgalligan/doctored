//! KERNEL: Edit ▸ Define Pattern's tile from a 32-bit float composite, and
//! the native samples a defined tile keeps (`designs/pattern-tiles.md`).
//!
//! # The raster
//!
//! A pattern tile's raster is straight RGBA8 at every depth (`pattern_tile`),
//! and a 32-bit document reads a texel's code / 255 as LINEAR light (a
//! Pattern Overlay's and Fill…'s 32-bit rule). The float row of Define
//! Pattern's raster is therefore that reading's inverse: each linear
//! component, alpha included, clamped to 0…1 and rounded to the nearest code,
//! `round(255 · clamp(v, 0, 1))`, with no display curve and no tone map.
//! Every other depth and mode defines its raster through an existing
//! conversion (the clipboard payload's 8-bit form,
//! `dr_clipboard_payload_rgba8`).
//!
//! # The samples
//!
//! What the raster cannot hold, the tile keeps as native samples
//! (`pattern_samples`), so a fill in the same kind of document gives the
//! source back exactly: a CMYK composite ALWAYS keeps its ink (`cmyk16`; the
//! raster is sRGB, never ink); a 16-bit RGB or Gray composite keeps `rgb16`
//! unless every code is a multiple of 257 (an 8-bit code, which the raster
//! holds); a float composite keeps `rgb32` unless every component reads back
//! from its raster code exactly (`code / 255 == v`). The float samples are
//! the stored range's: NaN read as 0, a colour component held to ±65504 and
//! alpha to 0…1, as a float document holds them. A Lab composite keeps
//! none. So a pattern defined from 8-bit-representable pixels is the tile
//! it always was.

use crate::float_document::FLOAT_RANGE_MAX;
use crate::pattern_samples::TileSamples;

/// `linear` (straight RGBA, linear light, one pixel per entry) as tile bytes
/// (module doc): four codes per pixel, a NaN component read as 0.
pub fn tile_from_linear(linear: &[[f32; 4]]) -> Vec<u8> {
    linear.iter().flat_map(|pixel| pixel.map(code)).collect()
}

/// One component's code: `round(255 · clamp(v, 0, 1))`, NaN as 0.
fn code(v: f32) -> u8 {
    if v.is_nan() {
        return 0;
    }
    (f64::from(v).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A 16-bit RGB or Gray composite's samples (module doc): `None` when every
/// code is a multiple of 257.
pub fn samples_from_rgba16(pixels: &[[u16; 4]]) -> Option<TileSamples> {
    pixels
        .iter()
        .flatten()
        .any(|c| !c.is_multiple_of(257))
        .then(|| TileSamples::Rgb16(pixels.iter().flatten().copied().collect()))
}

/// A float composite's samples (module doc), each component held to the
/// stored range first: `None` when every one reads back from its raster
/// code exactly.
pub fn samples_from_linear(linear: &[[f32; 4]]) -> Option<TileSamples> {
    let held: Vec<f32> = linear
        .iter()
        .flat_map(|pixel| {
            let colour = |v: f32| {
                if v.is_nan() {
                    0.0
                } else {
                    v.clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX)
                }
            };
            let alpha = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
            [
                colour(pixel[0]),
                colour(pixel[1]),
                colour(pixel[2]),
                alpha(pixel[3]),
            ]
        })
        .collect();
    held.iter()
        .any(|&v| f32::from(code(v)) / 255.0 != v)
        .then_some(TileSamples::Rgb32(held))
}

/// A CMYK composite's samples (module doc): its C, M, Y, K and alpha, always.
pub fn samples_from_cmyk(pixels: &[[u16; 5]]) -> TileSamples {
    TileSamples::Cmyk16(pixels.iter().flatten().copied().collect())
}
