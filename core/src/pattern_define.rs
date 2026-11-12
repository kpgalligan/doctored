//! KERNEL: Edit ▸ Define Pattern's tile from a 32-bit float composite.
//!
//! A pattern tile is straight RGBA8 at every depth (`pattern_tile`), and a
//! 32-bit document reads a texel's code / 255 as LINEAR light (a Pattern
//! Overlay's and Fill…'s 32-bit rule). The float row of Define Pattern is
//! therefore that reading's inverse: each linear component, alpha included,
//! clamped to 0…1 and rounded to the nearest code, `round(255 · clamp(v, 0,
//! 1))`, with no display curve and no tone map, so a tile defined from
//! values on the k / 255 grid fills back exactly. What the tile cannot hold
//! is clipped: light above white and the steps between codes. Every other
//! depth and mode defines its tile through an existing conversion (the
//! clipboard payload's 8-bit form, `dr_clipboard_payload_rgba8`).

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
