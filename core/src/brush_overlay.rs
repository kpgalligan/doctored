//! Native brush coverage as the byte path's paint overlay: the canvas u16
//! coverage `brush_coverage::rasterize` produces, one straight RGB8 ink and an
//! optional canvas u8 selection become the premultiplied RGBA8 overlay
//! `dr_doc_painting_layer` and its siblings take. The 8-bit Pencil commits
//! through it (`dr_brush_overlay_rgba8`), so its pixels are the core's aliased
//! coverage rather than a CoreGraphics rasterization, and the per-pixel work
//! stays out of Swift.
//!
//! THE RULE, per pixel, each step rounded half up once:
//! `a = round(coverage · 255 / 65535)` (the u16 code to its byte, exact at 0
//! and 65535), then with a selection byte `s`, `a = round(a · s / 255)`, then
//! each colour byte `c = round(ink · a / 255)` (premultiplication, so `c ≤ a`).
//! A selection multiplies rather than clips, which is the selection's own
//! coverage meaning: an anti-aliased marquee edge passes that fraction of the
//! stroke. Integer arithmetic throughout: `(x + h) / d` with `h = (d − 1) / 2`
//! is round-half-up of `x / d` for an odd `d` (255 and 65535 are odd, so no
//! integer `x` lands exactly on a half).

/// `round(x / d)`, half up, for an odd divisor `d` (module doc).
fn rounded(x: u32, d: u32) -> u32 {
    (x + (d - 1) / 2) / d
}

/// Writes the premultiplied RGBA8 overlay of `coverage` in `ink` into `out`
/// (`4 · coverage.len()` bytes), multiplied by `selection` when given (one byte
/// per coverage sample). The caller has checked every length.
pub fn overlay_rgba8(coverage: &[u16], ink: [u8; 3], selection: Option<&[u8]>, out: &mut [u8]) {
    for (i, (&sample, pixel)) in coverage.iter().zip(out.chunks_exact_mut(4)).enumerate() {
        let mut alpha = rounded(u32::from(sample) * 255, 65_535);
        if let Some(selection) = selection {
            alpha = rounded(alpha * u32::from(selection[i]), 255);
        }
        for (channel, &value) in pixel.iter_mut().zip(ink.iter()) {
            *channel = rounded(u32::from(value) * alpha, 255) as u8;
        }
        pixel[3] = alpha as u8;
    }
}
