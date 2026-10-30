//! The working encoding W of a 32-bit float retouch: the code units a
//! heal, spot heal or Content-Aware Fill compares and solves in when the
//! document stores linear light.
//!
//! `W(v) = sign(v) · S · f(|v|)`, with `f` the sRGB OETF up to 1 and
//! `f(u) = 1 + (1.055 / 2.4) · ln u` above it, and `S = 65535 / f(65504)`.
//!
//! Why an encoding at all. PatchMatch ranks candidate patches by squared
//! differences, the membrane spreads a boundary mismatch, and the
//! soft-coverage and opacity mixes interpolate between two values. Run on
//! linear light, every one of them is ruled by the brightest samples: a patch
//! distance between two skies at 8.0 and 9.0 outweighs every shadow in the
//! window, and a mix reads half way in light rather than half way to the eye.
//! The 8- and 16-bit kernels work on display-encoded codes, where equal steps
//! are roughly equal visible steps, and their constants were measured there.
//!
//! Why this curve. Up to diffuse white it IS the sRGB curve, so an SDR
//! picture held in a float document heals as its 16-bit twin does. Above
//! white the OETF's power law would spend most of the code range on values no
//! display shows, so the curve continues as a logarithm: equal ratios of
//! light are equal steps of code. The slope `1.055 / 2.4` is the OETF's own
//! slope at 1, so the join is C1 and no edge appears at white. The curve is
//! odd in `v`, so a negative sample (stored and admitted by every float
//! operator) keeps its sign and its distance from zero.
//!
//! Why this scale. `S` puts the largest stored magnitude
//! (`float_document::FLOAT_RANGE_MAX`, 65504) on 65535, so the whole stored
//! range lands on the 16-bit code range: `W(1) ≈ 11,155`. The membrane's
//! residual gate (`poisson::precision_refine`, 1/16 code) and the synthesis
//! constants were measured in those units and are used unchanged.
//!
//! The result is decoded ONCE, at the write. [`decode_changed`] decodes only
//! the samples whose code the kernel changed, so a pixel a retouch does not
//! write is never round-tripped and keeps its bits. [`decode`] answers inside
//! `±FLOAT_RANGE_MAX`: with the f32 membrane sample's clamp
//! (`heal_writeback`'s `HealSample for f32`) it is the float retouch's one
//! range bound, the stored range itself, as the 8- and 16-bit writeback clamps
//! to its own.
//!
//! Arithmetic is binary64 between the f32 ends; the two sRGB halves are
//! `adjust_math`'s, the one copy of that curve.

use std::sync::OnceLock;

use crate::adjust_math::{linear_to_srgb, srgb_to_linear};
use crate::float_document::FLOAT_RANGE_MAX;

/// The logarithm's slope above white: the sRGB OETF's own derivative at 1
/// (`1.055 · (1 / 2.4)`), which is what makes the join C1.
const SHOULDER: f64 = 1.055 / 2.4;

/// `S`: the code of one unit of `f`, so that `f(FLOAT_RANGE_MAX)` is 65535.
fn scale() -> f64 {
    static SCALE: OnceLock<f64> = OnceLock::new();
    *SCALE.get_or_init(|| 65535.0 / (1.0 + SHOULDER * f64::from(FLOAT_RANGE_MAX).ln()))
}

/// One linear sample as its W code (module doc). Odd in `linear`; a sample
/// inside the stored range lands inside ±65535.
pub(crate) fn encode(linear: f32) -> f32 {
    let u = f64::from(linear.abs());
    let f = if u <= 1.0 {
        f64::from(linear_to_srgb(u as f32))
    } else {
        1.0 + SHOULDER * u.ln()
    };
    (scale() * f).copysign(f64::from(linear)) as f32
}

/// One W code as its linear sample, the inverse of [`encode`], answered
/// inside `±FLOAT_RANGE_MAX` whatever the code.
pub(crate) fn decode(code: f32) -> f32 {
    let f = f64::from(code.abs()) / scale();
    let u = if f <= 1.0 {
        f64::from(srgb_to_linear(f as f32))
    } else {
        ((f - 1.0) / SHOULDER).exp()
    };
    u.min(f64::from(FLOAT_RANGE_MAX)).copysign(f64::from(code)) as f32
}

/// `linear` with every RGB sample encoded and alpha kept: the plane a float
/// heal or fill hands its kernel. `Err` when it cannot be allocated.
pub(crate) fn encode_plane(linear: &[[f32; 4]]) -> Result<Vec<[f32; 4]>, String> {
    let mut work = Vec::new();
    work.try_reserve_exact(linear.len())
        .map_err(|_| "could not allocate precision output".to_string())?;
    work.extend(
        linear
            .iter()
            .map(|p| [encode(p[0]), encode(p[1]), encode(p[2]), p[3]]),
    );
    Ok(work)
}

/// The write: each RGB sample of `linear` whose code in `work` differs
/// bitwise from its own encoding becomes `decode` of that code. A sample the
/// kernel left alone is not decoded, so it keeps its bits; alpha is never
/// written (the kernels keep it).
pub(crate) fn decode_changed(work: &[[f32; 4]], linear: &mut [[f32; 4]]) {
    for (code, pixel) in work.iter().zip(linear.iter_mut()) {
        for c in 0..3 {
            if encode(pixel[c]).to_bits() != code[c].to_bits() {
                pixel[c] = decode(code[c]);
            }
        }
    }
}
