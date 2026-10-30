//! The gray a swatch becomes on ONE colour channel of a layer: what Fill and
//! Stroke write there and what a moved channel leaves in its vacated
//! area, the kernel of `selected_color_plane` and of the plane fills.
//!
//! Photoshop 26.10.0 uses one luma for all of it, `0.30 r + 0.59 g + 0.11 b`
//! (`blend::lum`'s W3C weights), and differs only in the quantization
//! (`scripts/photoshop-probe/plane_gray.py`, receipt
//! `plane-gray-431-633/take1`; `plane_fill.py`, `plane-fill-431/take1`):
//!
//! * FILL / STROKE at 8 bits (`fill8` `3059r`): the luma of the swatch's
//!   8-bit codes rounded to a code — (0, 0, 255) 28, (250, 10, 0) 81,
//!   (90, 200, 40) 149 — [`plane_ink_u8`].
//! * FILL / STROKE at 16 bits (`fill16` `3059x`): the same luma unrounded,
//!   over the unit range, as a 16-bit sample (7208, 20792 and 38395 read for
//!   those swatches) — [`plane_ink_u16`].
//! * FILL / STROKE at 32 bits (`3059x`, form `raw`): the luma of the ink's
//!   components, unquantized and unconverted — [`plane_fill_value`].
//! * THE VACATED AREA (`hole8`, `hole16`, `hole32` all `3059r`, `form32`
//!   `raw`): the 8-bit rule's code over 255 at every depth, the 32-bit value
//!   with no profile decode — [`color_plane_hole`]. Take2's `bg.601f`
//!   (the Rec. 601 luma floored) agreed with it on that take's three pairs
//!   (102, 114, 84 under both) and is not what Photoshop does.
//!
//! None of these is weighted by the pixel's own transparency (`soft8`,
//! `soft16` `written`, as 32 bits' `soft`).

use crate::blend::LUM100;

/// The code Fill and Stroke write into one colour channel of an 8-bit layer:
/// `⌊(30 r + 59 g + 11 b + 50) / 100⌋` over the swatch's 8-bit codes, the
/// 30/59/11 luma rounded half up in whole numbers (a tie is unmeasured).
pub(crate) fn plane_ink_u8(swatch: [u8; 3]) -> u8 {
    let [r, g, b] = swatch.map(u32::from);
    ((LUM100[0] * r + LUM100[1] * g + LUM100[2] * b + 50) / 100) as u8
}

/// The sample Fill and Stroke write into one colour channel of a 16-bit
/// layer: [`plane_fill_value`] of the swatch's unit components, unrounded
/// until the one quantization to a 16-bit code.
pub(crate) fn plane_ink_u16(rgb: [f64; 3]) -> u16 {
    let value = f64::from(plane_fill_value(rgb.map(|c| c as f32)));
    (value.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// The vacated value of a Red, Green or Blue channel, in the document's unit
/// range (0…1): [`plane_ink_u8`] of the background swatch's 8-bit codes, over
/// 255. The same at 8, 16 and 32 bits.
pub(crate) fn color_plane_hole(swatch: [u8; 3]) -> f32 {
    f32::from(plane_ink_u8(swatch)) / 255.0
}

/// The value Fill and Stroke write into ONE colour channel of a 32-bit float
/// layer: `0.30 r + 0.59 g + 0.11 b` over the ink's components,
/// unquantized and unconverted — `blend::lum`, whose W3C weights are the
/// ones Photoshop 26.10.0 uses here (`scripts/photoshop-probe/plane_fill.py`,
/// receipt `plane-fill-431/take1`: `3059x` is the only candidate at 16 bits
/// and 32 bits reads it `raw`, for the Red, Green and Blue channel alike).
/// `rgb` is the swatch's document-encoded components, or linear light for an
/// above-white foreground (the take's `hdr` verdict `lin`: (4, 0.5, 0.25)
/// writes 1.5225).
pub(crate) fn plane_fill_value(rgb: [f32; 3]) -> f32 {
    crate::blend::lum(rgb)
}
