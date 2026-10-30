//! The spot-channel OVERPRINT (#547): how one spot channel shows on the
//! document's composite for display. A spot channel's ink is overprinted on
//! the composite; Doctored's rule for that, declared and unmeasured
//! (`designs/spot-channels.md`), is the ink colour source-over the
//! composite at a weight of its COVERAGE, with a blend that Solidity moves
//! from Multiply toward Normal: per plane
//! `B = (1 − s)·Multiply(Cb, ink) + s·ink`. Solidity is the ink's opacity on
//! screen, 0 % a transparent ink that shows the colour beneath and 100 % an
//! ink that covers it; so a spot channel at the default 0 % still shows, as
//! Multiply at its coverage, and a 100 % ink replaces the colour where it is
//! laid. A host runs it once per spot channel, in the channel list's order.
//! It changes the displayed pixels only, never a document's stored ones.
//!
//! The pixels are straight (non-premultiplied) RGBA in the composite's own
//! ENCODED values: the ink arrives already converted into that encoding by
//! the caller, so the multiply runs on the same numbers the screen shows,
//! which is what a printed overprint of an ink over a separation looks like
//! to the eye at the level of a preview. Values are read as `v / max`, run
//! through the one W3C step ([`composite_blended`] with the Solidity mix of
//! [`b_multiply`] and the ink per plane, the master copies in `blend`), and
//! written back by the arm's own quantization: RGBA8 and RGBA16 with
//! `clamp × max` rounded, `source_over_rgba8`'s rule; f32 RGBA as computed,
//! NOT clamped, so a float composite's range above white survives the step
//! (the ink itself is always in `[0, 1]`).
//!
//! The CMYK arm (#547 1c-1) is ink on paper: each plate's `L = 1 − ink`
//! (the domain `cmyk_filter`'s passes and the selection a plate loads use)
//! is the backdrop, and the spot ink's own plates, as `L`, are the ink. The
//! one body runs twice a pixel: (C, M, Y, A) with `(1 − c, 1 − m, 1 − y)`,
//! then (K, K, K, A) with `1 − k`, and each `L` goes back as `1 − L`. A
//! Multiply of `L` values is the product of the transmissions, which is how
//! two inks laid on one another darken the paper.
//!
//! A coverage of 0 leaves the pixel exactly as it was at any solidity, so an
//! untouched plane costs no rounding anywhere. Over a transparent
//! pixel the step deposits the ink at the weight's alpha, as any source-over
//! does.

use crate::blend::{b_multiply, composite_blended};

/// Overprints one ink onto straight RGBA8 `composite` pixels, `coverage`
/// holding one byte a pixel (255 = full ink), at weight `coverage / 255`.
/// `ink` is the ink's RGB in `[0, 1]` in the composite's encoding and
/// `solidity` in `[0, 1]` the blend's mix, 0 Multiply and 1 Normal (module
/// doc); values outside are clamped and a non-finite one reads as 0.
/// `false`, with nothing written, when `composite.len() != 4 *
/// coverage.len()`.
pub fn overprint_rgba8(
    composite: &mut [u8],
    coverage: &[u8],
    ink: [f32; 3],
    solidity: f32,
) -> bool {
    overprint(composite, coverage, ink, solidity, 255.0, 255.0, |v| {
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

/// [`overprint_rgba8`] at 16 bits: straight RGBA16 pixels and one u16
/// coverage sample a pixel (65535 = full ink), at weight `coverage / 65535`;
/// `solidity` mixes Multiply toward Normal as there.
pub fn overprint_rgba16(
    composite: &mut [u16],
    coverage: &[u16],
    ink: [f32; 3],
    solidity: f32,
) -> bool {
    overprint(composite, coverage, ink, solidity, 65535.0, 65535.0, |v| {
        (v.clamp(0.0, 1.0) * 65535.0).round() as u16
    })
}

/// [`overprint_rgba8`] on straight f32 RGBA pixels (a 32-bit document's
/// composite, `v` read as itself) with one u16 coverage sample a pixel
/// (65535 = full ink). The result is NOT clamped: a sample above white or
/// below black stays where the step puts it (module doc).
pub fn overprint_rgba32f(
    composite: &mut [f32],
    coverage: &[u16],
    ink: [f32; 3],
    solidity: f32,
) -> bool {
    overprint(composite, coverage, ink, solidity, 1.0, 65535.0, |v| v)
}

/// The CMYK arm (module doc): straight CMYKA16 `pixels` (ink codes, 65535 =
/// full ink), one u16 coverage sample a pixel, `ink` the spot ink's C, M, Y
/// and K in `[0, 1]`. Every plate and the alpha are rounded back to codes.
/// `false`, with nothing written, when the lengths differ.
pub fn overprint_cmyka16(
    pixels: &mut [[u16; 5]],
    coverage: &[u16],
    ink: [f32; 4],
    solidity: f32,
) -> bool {
    if pixels.len() != coverage.len() {
        return false;
    }
    let solidity = unit(solidity);
    let ink = ink.map(unit);
    let colour = [1.0 - ink[0], 1.0 - ink[1], 1.0 - ink[2]];
    let key = [1.0 - ink[3]; 3];
    let code = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    for (px, &c) in pixels.iter_mut().zip(coverage) {
        let w = f32::from(c) / 65535.0;
        if w <= 0.0 {
            continue;
        }
        let light = |v: u16| 1.0 - f32::from(v) / 65535.0;
        let alpha = f32::from(px[4]) / 65535.0;
        let mut plates = [light(px[0]), light(px[1]), light(px[2]), alpha];
        blend_pixel(&mut plates, &colour, solidity, w);
        let k = light(px[3]);
        let mut black = [k, k, k, alpha];
        blend_pixel(&mut black, &key, solidity, w);
        *px = [
            code(1.0 - plates[0]),
            code(1.0 - plates[1]),
            code(1.0 - plates[2]),
            code(1.0 - black[0]),
            code(plates[3]),
        ];
    }
    true
}

/// A unit value as given, clamped into `[0, 1]`; not finite reads as 0.
fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The one step on one straight pixel of unit values (`dst`, colour then
/// alpha): the ink at weight `w` > 0, its blend Solidity's mix of Multiply
/// toward the ink. `ink` and `solidity` are already in `[0, 1]`.
fn blend_pixel(dst: &mut [f32; 4], ink: &[f32; 3], solidity: f32, w: f32) {
    // Solidity mixes the Multiply result toward the ink itself (Normal):
    // 0 % shows the colour beneath through the ink, 100 % covers it.
    let mix = |cb: f32, ink: f32| (1.0 - solidity) * b_multiply(cb, ink) + solidity * ink;
    let blended = [
        mix(dst[0], ink[0]),
        mix(dst[1], ink[1]),
        mix(dst[2], ink[2]),
    ];
    composite_blended(dst, ink, &blended, w);
}

/// The one body of the RGBA arms: `max` is the sample's full scale,
/// `coverage_max` the coverage's, and `quantize` turns an unclamped unit
/// value back into a sample, clamping as its arm requires.
fn overprint<T: Copy + Into<f32>, C: Copy + Into<f32>>(
    composite: &mut [T],
    coverage: &[C],
    ink: [f32; 3],
    solidity: f32,
    max: f32,
    coverage_max: f32,
    quantize: fn(f32) -> T,
) -> bool {
    if coverage.len().checked_mul(4) != Some(composite.len()) {
        return false;
    }
    let solidity = unit(solidity);
    let ink = ink.map(unit);
    for (px, &c) in composite.chunks_exact_mut(4).zip(coverage) {
        let w = c.into() / coverage_max;
        if w <= 0.0 {
            continue;
        }
        let mut dst = [
            px[0].into() / max,
            px[1].into() / max,
            px[2].into() / max,
            px[3].into() / max,
        ];
        blend_pixel(&mut dst, &ink, solidity, w);
        for (out, v) in px.iter_mut().zip(dst) {
            *out = quantize(v);
        }
    }
    true
}
