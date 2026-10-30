//! KERNEL: the Bevel & Emboss TEXTURE: the Texture
//! sub-panel as a height plane read from a placed pattern, whose slopes add
//! to the bevel's own, and the shading law the bevel uses while it is on.
//! The model (`BevelTexture`) and its placement are `style_pattern_fill`'s
//! (one sampler, one level, one anchor rule); the adapter is
//! `style_fx_bevel_emboss`.
//!
//! # The heights
//!
//! `t` per plane pixel is the Gray luminosity (`blend::lum`, 0.30 / 0.59 /
//! 0.11, the fitted Gray) of the sampler's straight RGB times its alpha, at
//! the pixel's LAYER coordinate — the tile a Pattern Overlay with the same
//! fill would draw, read as height in 0..=1 ([`texture_heights_with_job`]).
//! The plane is masked by the bevel's own height / 255
//! ([`TEXTURE_MASK_HEIGHT`]), so the texture follows the relief: it fades
//! with an inner bevel's ramp and reaches into an outer bevel's band.
//!
//! # The slopes and the law (`texture_take.py`, the fit on noise take 2)
//!
//! The slope of `t` is the central difference over ONE pixel
//! ([`TEXTURE_STENCIL`]), `gain (t[x+1] − t[x−1]) / 2` and the same in y,
//! with `gain` = [`TEXTURE_GAIN`] × Depth (1 = +100 %) × −1 for Invert × the
//! bevel's Depth × its Size / 16 ([`texture_gain`]); it is ADDED to the
//! bevel's own slope. While the
//! texture is on, the bevel shades with the NORMALIZED law: `nl = n·L`, `d =
//! (nl − lz) / (1 − lz)` where `nl >= lz` and `(nl − lz) / lz` below it
//! ([`shade`]), so flat ground is 0, a face turned to the light full
//! highlight and one turned fully away full shadow. A Smooth bevel, in every
//! style, shades NORMALIZED with or without a texture (its measured law,
//! `style_bevel_smooth`, #578), and so does a Chisel the Chisel law renders
//! (`style_bevel_chisel`); every other Chisel keeps its LATERAL law byte for
//! byte while the texture is off.
//!
//! The declared fit (Rec. 709 luma, stencil ONE or the bevel's own, LATERAL
//! or FULL `n·L − lz`) missed every take-2 row by 28 to 180 codes mean;
//! Photoshop's readings on the flat interior show a FULL-type response whose
//! lit and unlit sides are scaled apart (+18 against −197 codes for equal
//! and opposite slopes) and R and G slopes weighed as Gray weighs them. The
//! REFIT (Gray, ONE, NORMALIZED, G −16.1), declared before take 3, meets
//! every take-2 row: tx_s100 max 2 / mean 0.57 codes, Scale 50 % 0.68,
//! Invert and Depth −100 % 0.90 (both negate the gain), and a linked tile
//! follows a Move while an unlinked one stays on the canvas (`PatternFill::
//! origin`: 0.57 and 0.40). G is negative: Photoshop raises the DARK texels.
//! At altitude 30 NORMALIZED equals `2 (nl − lz)`; take 3's held-out
//! altitude-60 row chose NORMALIZED (max 1 / mean 0.54 codes; DOUBLE 41.1),
//! and its angle-90 row holds too (0.17).
//!
//! # Take 3 (`texture_take.py`, run rz-557-texture-20261016T161801Z-f21d13b9,
//! verdict MISSES)
//!
//! - COUPLING BOTH met: the gain scales with the bevel's Depth
//!   ([`TEXTURE_BY_DEPTH`]) and its Size / 16 ([`TEXTURE_BY_SIZE`]): size 8,
//!   size 32, bevel depth 300 % and texture depth 300 % within 2 codes max,
//!   0.36 to 0.93 mean (NONE 41.7, DEPTH 23.6, SIZE 41.7).
//! - MASK missed under both rules on Outer Bevel, Emboss and Pillow Emboss,
//!   judged on the texture's delta outside the shape (SHAPE worst mean 99.7,
//!   HEIGHT 61.5); HEIGHT ships ([`TEXTURE_MASK_HEIGHT`]), the smaller miss.
//!   Photoshop textures an outer bevel's band too. A recorded residual.
//! - Scale 75 % misses (61 / 20.7 codes): Photoshop reads a scaled texture as
//!   an area-filtered tile where the one PYRAMID sampler keeps the bilinear
//!   texels. A recorded residual (one sampler per pattern fill).
//!
//! Every renderer (8, 16 and 32 bits, CMYK) consumes the bevel's u8 planes,
//! so the texture is one implementation at every depth. Its sampler polls
//! nothing; [`texture_heights_with_job`] polls `job` once per row.

use crate::blend::lum;
use crate::job_control::JobContext;
use crate::pattern_tile::PatternSampler;

/// The texture's slope gain per unit of `t` (module doc): the REFIT's G,
/// fitted on take 2's tx_s100 over the grid −50..50 step 0.01. Negative:
/// Photoshop raises the dark texels.
pub(crate) const TEXTURE_GAIN: f32 = -16.1;

/// The texture's central-difference half-width in px (module doc): ONE.
/// Take 2's seams (the tile's wrap) read on exactly the two pixels a
/// one-pixel stencil straddles, where the bevel's own `round(size / 8)`
/// would spread them over four.
pub(crate) const TEXTURE_STENCIL: i64 = 1;

/// The textured bevel's law (module doc): false is the REFIT's NORMALIZED
/// (the lit side over its headroom `1 − lz`, the unlit side over `lz`),
/// true is DOUBLE, `2 (n·L − lz)`, which equals it at altitude 30; take 3's
/// altitude-60 row chose NORMALIZED.
pub(crate) const TEXTURE_SHADE_DOUBLE: bool = false;

/// Whether the gain also scales with the bevel's Depth (module doc: take 3's
/// COUPLING BOTH).
pub(crate) const TEXTURE_BY_DEPTH: bool = true;

/// Whether the gain also scales with the bevel's Size / 16 (module doc: take
/// 3's COUPLING BOTH; the REFIT's G was fitted at size 16).
pub(crate) const TEXTURE_BY_SIZE: bool = true;

/// Whether the texture is masked by the bevel's height / 255 instead of the
/// shape's coverage / 255 (module doc: take 3's MASK, the smaller miss).
pub(crate) const TEXTURE_MASK_HEIGHT: bool = true;

/// `t` in 0..=1 per plane pixel of a `w` x `h` plane whose pixel (0, 0) is
/// LAYER pixel `origin`: the Gray luminosity of the sampler's straight RGB
/// times its alpha at layer (x + origin.0, y + origin.1) (module doc). Polls
/// `job` once per row.
pub(crate) fn texture_heights_with_job(
    sampler: &PatternSampler,
    w: u32,
    h: u32,
    origin: (i64, i64),
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let mut out = Vec::with_capacity(w as usize * h as usize);
    for y in 0..i64::from(h) {
        job.check()?;
        for x in 0..i64::from(w) {
            let p = sampler.at(x + origin.0, y + origin.1);
            out.push(lum([p[0], p[1], p[2]]) * p[3]);
        }
    }
    Ok(out)
}

/// `t` multiplied in place by `mask / 255` (module doc: the coverage, or the
/// bevel's height under [`TEXTURE_MASK_HEIGHT`]). The caller owns `t`.
pub(crate) fn mask_heights(t: &mut [f32], mask: &[u8]) {
    for (v, &m) in t.iter_mut().zip(mask) {
        *v *= f32::from(m) / 255.0;
    }
}

/// The texture's slope gain (module doc): G · `depth` · (−1 when `invert`)
/// · (`bevel_depth` when [`TEXTURE_BY_DEPTH`]) · (`size` / 16 when
/// [`TEXTURE_BY_SIZE`]).
pub(crate) fn texture_gain(depth: f32, invert: bool, bevel_depth: f32, size: f32) -> f32 {
    let mut gain = TEXTURE_GAIN * depth;
    if invert {
        gain = -gain;
    }
    if TEXTURE_BY_DEPTH {
        gain *= bevel_depth;
    }
    if TEXTURE_BY_SIZE {
        gain *= size / 16.0;
    }
    gain
}

/// `(gain (t[x+k] − t[x−k]) / 2k, gain (t[y+k] − t[y−k]) / 2k)` at plane
/// pixel (x, y) of the `w` x `h` plane `t`, reads clamped to the plane (`k`
/// at least 1).
pub(crate) fn texture_slope(
    t: &[f32],
    w: u32,
    h: u32,
    x: i64,
    y: i64,
    k: i64,
    gain: f32,
) -> (f32, f32) {
    let (wi, hi) = (i64::from(w), i64::from(h));
    if wi == 0 || hi == 0 || t.len() != w as usize * h as usize {
        return (0.0, 0.0);
    }
    let k = k.max(1);
    let at = |x: i64, y: i64| -> f32 {
        let (x, y) = (x.clamp(0, wi - 1), y.clamp(0, hi - 1));
        t[(y * wi + x) as usize]
    };
    let scale = gain / (2 * k) as f32;
    (
        (at(x + k, y) - at(x - k, y)) * scale,
        (at(x, y + k) - at(x, y - k)) * scale,
    )
}

/// The textured bevel's signed shading for slopes (`hx`, `hy`) under the
/// light `(lx, ly, lz)` ([`TEXTURE_SHADE_DOUBLE`]; module doc). A light with no
/// height (`lz` 0) or straight overhead (`lz` 1) leaves the side it cannot
/// reach at 0 rather than dividing by zero.
pub(crate) fn shade(hx: f32, hy: f32, light: (f32, f32, f32)) -> f32 {
    let (lx, ly, lz) = light;
    let inv_len = 1.0 / (1.0 + hx * hx + hy * hy).sqrt();
    let nl = (-hx * lx - hy * ly + lz) * inv_len;
    if TEXTURE_SHADE_DOUBLE {
        2.0 * (nl - lz)
    } else if nl >= lz {
        if lz < 1.0 {
            (nl - lz) / (1.0 - lz)
        } else {
            0.0
        }
    } else if lz > 0.0 {
        (nl - lz) / lz
    } else {
        0.0
    }
}
