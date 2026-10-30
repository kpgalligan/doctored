//! BYTE Opacity and Fill (#2, measured against Photoshop): below 32 bits a
//! layer's Opacity and Fill composite at `code / 255`, never at the exact
//! unit value the layer stores.
//!
//! The evidence (`scripts/photoshop-probe/effects_578_take.py`, its RECORDED
//! section): stroke-composite-578 take1's 16-bit interior of a rect at a 50 %
//! Fill reads 151.093 / 207.304 / 167.148, which 128 / 255 predicts (151.094 /
//! 207.314 / 167.157) and an exact 0.5 does not (151.5); at 30 % it reads
//! 192.479, which 77 / 255 predicts (192.494). Fitted offline over that take's
//! Fill rows and cmyk-blend-547 take1's opacity-50 rows, the byte rounded half
//! up won every cell (Fill and Opacity at 8 and 16 bits) against the exact
//! value and the truncated byte.
//!
//! What reads it: every 8-bit, 16-bit and CMYK composite path quantizes a
//! layer's, group's or clip base's opacity and a style's Fill here, and every
//! predicate on them (`< 1`, `> 0`, `>= 1`) reads the same quantized value, so
//! the regional renderer decides exactly what the full one composites. 32-bit
//! float composites the exact value (`composite(…, true)`). The stored value
//! never changes: only composites and the PSD bytes (`code`, so the byte
//! written is the byte composited) read it. Effect opacities are untouched.
//!
//! Where the Fill and the Opacity scale the same contribution together, the
//! product of the two bytes is itself rounded to a byte ([`joint`], fitted
//! on effects-578 take1's `both_50`).
//!
//! Fill and Opacity quantize identically, which keeps
//! `style_composite::composite_pixels_into`'s identity: a Fill edit and an
//! opacity edit of the same value composite byte for byte alike.

/// Codes added before the floor. WHY: the layer stores an f32, and its f64
/// widening of a whole percent can land just under a half code — f64(f32
/// 0.7)·255 = 178.499997 — which a plain half-up rounding would send to 178
/// where Photoshop's integer percent gives 179. 1e-3 code is far above that
/// widening error (≈ 2e-5 code) and far below any value a user can type
/// (a percent is 2.55 codes; 1/255 is one).
pub(crate) const TIE_SLACK: f64 = 1e-3;

/// Clamps to [0, 1], mapping non-finite values to 1. Restates
/// `doc::sane_opacity`, the master copy, so this leaf module imports nothing
/// from the crate.
fn sane(unit: f32) -> f32 {
    if unit.is_finite() {
        unit.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// The byte for a unit opacity or Fill: `floor(v·255 + 0.5 +
/// TIE_SLACK)` on the sanitized value widened to f64. Idempotent on `n/255`.
pub(crate) fn code(unit: f32) -> u8 {
    let scaled = f64::from(sane(unit)) * 255.0 + 0.5 + TIE_SLACK;
    // In [0.501, 255.501], so the floor is a valid byte.
    scaled.floor() as u8
}

/// The unit value a composite below 32 bits reads: `code(unit) / 255`.
pub(crate) fn quantized(unit: f32) -> f32 {
    f32::from(code(unit)) / 255.0
}

/// The unit value a composite reads: exact (sanitized) when `float` (32-bit
/// float), else [`quantized`].
pub(crate) fn composite(unit: f32, float: bool) -> f32 {
    if float {
        sane(unit)
    } else {
        quantized(unit)
    }
}

/// The ONE weight where a Fill and an Opacity scale the same contribution in
/// one product — a layer's own pixels composited straight onto the
/// backdrop: below 32 bits the product of the two bytes rounded to a byte
/// again, `code(fill_byte · opacity_byte / 255) / 255`; in a float scene
/// the exact product. `fill` and `opacity` are the values a composite
/// already reads ([`composite`]), so at either one 1 this returns the other
/// unchanged and every single-component composite is the byte rule above.
/// Effects, which the Fill does not scale, read the opacity alone; a
/// package, a clip unit or a routed knockout unit (`style_knockout`,
/// `precision_style_knockout`) pays the Fill inside its buffer and the
/// opacity on the buffer, two separate products, and never meets this.
///
/// FITTED on effects-578 take1's `both_50` (Opacity 50 % and Fill 50 %),
/// which reads 64 / 255 = `round(128 · 128 / 255) / 255` (0 / 0 codes at 8
/// bits, 0.008 / 0.002 at 16) where the unrounded 128² / 255² misses by a
/// code; confirmed by the single-component rows, where it reduces to
/// [`quantized`]. The product's own rounding is not independently
/// confirmed. WHY `quantized` is exact here: on two bytes `a`, `b` the
/// product's fraction in codes is `(a·b mod 255) / 255`, never within 0.0019
/// of a half, so neither the f32 product nor [`TIE_SLACK`] moves a code.
pub(crate) fn joint(fill: f32, opacity: f32, float: bool) -> f32 {
    if float {
        fill * opacity
    } else {
        quantized(fill * opacity)
    }
}
