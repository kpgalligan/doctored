//! KERNEL: Black & White's 8-bit Tint on integers (#2 phase 4, h8a-LE),
//! the form Photoshop 26.10.0 computes on a byte document.
//!
//! [`gray8`] is the six-weight grey on the pixel's CODES with the weights in
//! whole percent, rounded half to EVEN; [`tint8`] sets the tint's numbers to
//! that grey through an integer luminosity `L8 = (77·N_r + 150·N_g + 28·N_b +
//! 128) >> 8`, then ClipColor in f64 with a one-code lift on every component
//! below the grey. Fitted offline (`scripts/photoshop-probe/
//! eight_bit_578_take.py`, from `psd_appearance_578_take.py`'s members): 1
//! code on 4 of 790 take1 samples and exact on take6's 792, where the 16-bit
//! SetLum form at 8 bits (`setlum-clip8`) missed by up to 10; one take on
//! four unused sets confirmed it (`eight-bit-578/take1`: 1 / 0.016 codes,
//! `setlum-clip8` 3 / 0.58). `adjust_mix::BlackAndWhite` runs it only in its
//! 8-bit form, with Tint on.

use crate::adjust_math::decompose;

/// The grey of one pixel's codes at the six weights in whole percent
/// ([`crate::adjust_math::HUE_WHEEL`] order): `g100 = 100·min + (max −
/// mid)·P[primary] + (mid − min)·P[secondary]` over [`decompose`] (which
/// reads the codes' order and differences, exact in f32), clamped to
/// 0..=25500 and divided by 100 rounding half to even.
pub(crate) fn gray8(codes: [u8; 3], percents: [i32; 6]) -> u8 {
    let parts = decompose(codes.map(f32::from));
    let g100 = 100 * parts.neutral as i32
        + parts.primary_amount as i32 * percents[parts.primary]
        + parts.secondary_amount as i32 * percents[parts.secondary];
    let g100 = g100.clamp(0, 25500);
    let (whole, rest) = (g100 / 100, g100 % 100);
    let up = rest > 50 || (rest == 50 && whole % 2 == 1);
    (whole + i32::from(up)) as u8
}

/// The tint's `numbers` set to the grey `g8`: `c = g8 + N − L8` with
/// `L8 = (77·N_r + 150·N_g + 28·N_b + 128) >> 8`, returned as is when every
/// component lies in 0..=255. Otherwise ClipColor in f64 about `l = g8`: a
/// component below 0 pulls every one by `l/(l − min)`, then a maximum above
/// 255 (and above `l`) by `(255 − l)/(max − l)`; each is rounded half up,
/// one code is added (at most up to `g8`) to a component below `l`, and the
/// result is clamped to 0..=255. The ClipColor is a variant of
/// [`crate::blend::set_lum_within`], its master, on codes with the ceiling at
/// 255: the same two branches in the same order and expression order.
pub(crate) fn tint8(numbers: [u8; 3], g8: u8) -> [u8; 3] {
    let n = numbers.map(i32::from);
    let big_l = (77 * n[0] + 150 * n[1] + 28 * n[2] + 128) >> 8;
    let g = i32::from(g8);
    let c = n.map(|k| g + k - big_l);
    if c.iter().all(|k| (0..=255).contains(k)) {
        return c.map(|k| k as u8);
    }
    let l = f64::from(g8);
    let mut c = c.map(f64::from);
    let low = c[0].min(c[1]).min(c[2]);
    if low < 0.0 {
        c = c.map(|k| l + (k - l) * l / (l - low));
    }
    let high = c[0].max(c[1]).max(c[2]);
    if high > 255.0 && high > l {
        c = c.map(|k| l + (k - l) * (255.0 - l) / (high - l));
    }
    c.map(|k| {
        let mut q = (k + 0.5).floor() as i32;
        if k < l {
            q = (q + 1).min(g);
        }
        q.clamp(0, 255) as u8
    })
}
