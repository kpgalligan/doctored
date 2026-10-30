//! The Lab Color document's stack-tool pixel math (#1 phase 6b-ii): what
//! Auto-Align Layers' Vignette Removal and Geometric Distortion read from a
//! Lab layer, how Vignette Removal writes its gain into the L\*a\*b\* codes,
//! and the Lightness-only rules of Auto-Blend Layers. A kernel: it names no
//! document or profile type (`scripts/check_layers.py`).
//!
//! **The reader.** Vignette Removal and Geometric Distortion fit linear light;
//! a Lab layer's is its CIE Y (D50), `icc_lut::lab_to_xyz` of the codes'
//! L\*a\*b\* ([`linear_y`]), handed to the fit in all three channels.
//!
//! **The write.** A vignette gain of `2^stops` multiplies XYZ; any linear RGB
//! is a matrix of XYZ, so this is the same gain an RGB document applies in its
//! own linear light. [`vignette_codes`] takes the codes through XYZ and back
//! by the one Lab⇄XYZ pair (`icc_lut`), rounding to the nearest u16 code; an
//! 8-bit result can land off the ×257 grid, as paint does.
//!
//! **Auto-Blend.** Stack Images scores and Panorama's seams take their
//! gradient on L\* alone ([`LIGHTNESS_LUMA`] as the kernels' luma weights),
//! and Seamless Tones keeps each frame's Lightness gain alone
//! ([`lightness_gains`]), so a\* and b\* keep their codes.

use crate::icc_lut::{lab_to_xyz, xyz_to_lab};
use crate::lab_select::{codes, units};

/// The luma weights over the stored channels that read L\* alone (the red
/// codes): `focus_stack::winners`' and `panorama_seam::seams`' `luma`.
pub(crate) const LIGHTNESS_LUMA: [f32; 3] = [1.0, 0.0, 0.0];

/// The codes' L\*a\*b\* as `f64`.
fn lab(codes: [u16; 4]) -> [f64; 3] {
    units(codes).map(f64::from)
}

/// `[Y, Y, Y]`: Y = `icc_lut::lab_to_xyz(lab_select::units(codes))[1]`, D50
/// linear light (module doc, **The reader**). Alpha is ignored.
pub(crate) fn linear_y(codes: [u16; 4]) -> [f64; 3] {
    let y = lab_to_xyz(lab(codes))[1];
    [y, y, y]
}

/// The codes with their XYZ multiplied by `2^stops` (module doc, **The
/// write**): codes → L\*a\*b\* → XYZ × 2^stops → L\*a\*b\* →
/// `lab_select::codes`; alpha kept. `stops == 0` returns `codes` unchanged.
pub(crate) fn vignette_codes(pixel: [u16; 4], stops: f32) -> [u16; 4] {
    if stops == 0.0 {
        return pixel;
    }
    let gain = f64::from(stops).exp2();
    let xyz = lab_to_xyz(lab(pixel)).map(|v| v * gain);
    let out = codes(xyz_to_lab(xyz).map(|v| v as f32));
    [out[0], out[1], out[2], pixel[3]]
}

/// Each frame's Seamless Tones gain becomes `[g[0], 1, 1]`: the Lightness
/// gain alone (module doc, **Auto-Blend**).
pub(crate) fn lightness_gains(gains: &mut [[f32; 3]]) {
    for gain in gains {
        gain[1] = 1.0;
        gain[2] = 1.0;
    }
}
