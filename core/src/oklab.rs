//! KERNEL: Oklab (Björn Ottosson, "A perceptual color space for image
//! processing", 2020) to and from LINEAR sRGB, the ONE Oklab in the core. It
//! imports nothing from the crate: the sRGB transfer pair is the caller's
//! (`adjust_math::srgb_to_linear` / `linear_to_srgb`), never a copy here.
//!
//! The forward is `lms = M1 · rgb`, `(L, a, b) = M2 · cbrt(lms)`; the inverse
//! is `lms' = M2⁻¹ · (L, a, b)`, `rgb = M1⁻¹ · lms'³`, with Ottosson's
//! published matrices AND his published inverses (not inverses computed
//! here), so the numbers are the ones every other implementation of the
//! space carries. The arithmetic is f32; the constants are written with the
//! published digits (as f64, so no digit is dropped in the source) and
//! rounded to f32 where they are used.
//!
//! The first caller is the Gradient Map's Perceptual method
//! (`adjust_map`'s module doc): the Perceptual interpolation is the Classic
//! law over the stops' Oklab values, as measured against Photoshop.

/// Linear sRGB to LMS (Ottosson's M1).
const M1: [[f64; 3]; 3] = [
    [0.412_221_470_8, 0.536_332_536_3, 0.051_445_992_9],
    [0.211_903_498_2, 0.680_699_545_1, 0.107_396_956_6],
    [0.088_302_461_9, 0.281_718_837_6, 0.629_978_700_5],
];

/// Cube-rooted LMS to (L, a, b) (Ottosson's M2).
const M2: [[f64; 3]; 3] = [
    [0.210_454_255_3, 0.793_617_785_0, -0.004_072_046_8],
    [1.977_998_495_1, -2.428_592_205_0, 0.450_593_709_9],
    [0.025_904_037_1, 0.782_771_766_2, -0.808_675_766_0],
];

/// (L, a, b) to cube-rooted LMS (Ottosson's published M2⁻¹).
const M2_INV: [[f64; 3]; 3] = [
    [1.0, 0.396_337_777_4, 0.215_803_757_3],
    [1.0, -0.105_561_345_8, -0.063_854_172_8],
    [1.0, -0.089_484_177_5, -1.291_485_548_0],
];

/// LMS to linear sRGB (Ottosson's published M1⁻¹).
const M1_INV: [[f64; 3]; 3] = [
    [4.076_741_662_1, -3.307_711_591_3, 0.230_969_929_2],
    [-1.268_438_004_6, 2.609_757_401_1, -0.341_319_396_5],
    [-0.004_196_086_3, -0.703_418_614_7, 1.707_614_701_0],
];

/// `m · v` in f32, each constant rounded to f32 at use.
fn mul(m: &[[f64; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r][0] as f32 * v[0] + m[r][1] as f32 * v[1] + m[r][2] as f32 * v[2])
}

/// A LINEAR sRGB triple to Oklab (L, a, b): white is (1, 0, 0). `cbrt` is
/// odd, so a negative LMS (an out-of-gamut input) stays finite.
pub(crate) fn linear_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    mul(&M2, mul(&M1, rgb).map(f32::cbrt))
}

/// Oklab (L, a, b) back to LINEAR sRGB, the inverse of [`linear_to_oklab`]
/// within f32 rounding. An Oklab point between in-gamut colours may land a
/// little outside [0, 1]: the caller's encode clamps.
pub(crate) fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    mul(&M1_INV, mul(&M2_INV, lab).map(|v| v * v * v))
}
