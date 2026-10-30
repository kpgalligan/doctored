//! Layer Style Noise: the Noise slider (0-100 %)
//! on Drop Shadow, Inner Shadow, Outer Glow and Inner Glow, as an in-place
//! pass over an effect's u8 coverage plane.
//!
//! # The law
//!
//! Each non-zero plane value `v` becomes `round(clamp(v (1 - A n + B n (2u -
//! 1)) + C n (2u - 1), 0, 255))`, `n` the amount in `0..=1` and `u` uniform
//! in `[0, 1)`; a zero stays zero, so the noise never paints where the
//! effect has no coverage. Take 2 (`scripts/photoshop-probe/noise_take.py`,
//! Photoshop 26.10.0, run rz-557-noise-20261016T145020Z-b618314e, verdict
//! VALID) chose ADD among MUL (A ½, B ½), SYM (A 0, B 1), ADD (A 0, B 0, C
//! 255) and a fitted multiplicative FIT: Photoshop ADDS a uniform of
//! half-width `255 n` to the coverage, clipped at both ends — at Noise 50 %
//! an alpha-64 shadow reads mean 72.7 and SD 63.6, where scaling the
//! coverage would give SD 9. ADD met every judged bin of all six rows (drop
//! shadow at 25/50/100 %, both glows and the inner shadow at 50 %; worst row
//! mean 1.61 codes, max 4.16 within the bound plus three standard errors);
//! MUL's worst row mean was 68.9, SYM's 41.4, FIT's 36.4. So [`NOISE_BIAS`]
//! and [`NOISE_SPREAD`] are 0 and [`NOISE_ADDITIVE`] is 255.
//!
//! # The field
//!
//! `u` is drawn POSITIONALLY from the crate's one generator,
//! `SplitMix64::at(NOISE_SEED, lx, ly, stream)`, at the pixel's LAYER
//! coordinate `(lx, ly)` (`style_render`'s plane model: plane pixel `p` is
//! layer pixel `p + origin`) and the effect kind as the stream. So the grain
//! is a fixed function of where the pixel sits on the layer: a sub-plane (the
//! cache's incremental re-render) draws exactly the full plane's grain, a
//! Move reuses the cached plane, the same picture always shows the same
//! grain (a replayed Action is identical, as Photoshop's REPEAT row was),
//! and a drop shadow and an outer glow on one layer do not share a field.
//! Two stacked instances of one kind share theirs (the stream is the kind
//! only). The take's MOVE row found Photoshop's field fixed to the CANVAS
//! (a moved layer's grain stays put); keeping the planes layer-local is the
//! recorded residual, since a canvas-keyed field would make every Move
//! re-render the style.
//!
//! # The site and the Below cap
//!
//! The pass runs on the plane AFTER the effect's contour has mapped it
//! ([`NOISE_AFTER_CONTOUR`]: take 2's SITE row, Cone at 50 % noise, met at
//! 0.31 codes where noising before the contour missed by 57) and before the
//! Interior coverage multiply or the outer glow's knock-out, so the noise
//! never reaches past the shape's own clip. A Below effect (drop shadow,
//! outer glow) also holds its grain to `style_contour`'s Below cap `k R(v)`
//! of its own plane ([`apply_noise_below_with_job`]): on the take's outer
//! glow the codes under 32, which the bound did not judge, read about 2.5 c
//! where the bare law gives 32, and the glow's cap (k 4.125) restates them
//! within 1.35 codes per pixel (no cap 23.6, k 8.25 10.1), while the inner
//! glow and inner shadow follow the bare law (1.94 and 2.44) — phase 1's
//! cap hypothesis, carried onto the noise until a fresh take re-judges it.
//! The drop shadow's plane is noised before its composite-time shift, so the
//! grain is the plane's. Every renderer (8, 16 and 32 bits, CMYK) consumes
//! the same u8 planes, so the noise is one implementation at every depth.

use crate::job_control::JobContext;
use crate::rng::SplitMix64;

/// The fixed stream the grain is drawn from. Fixed rather than a parameter
/// because the grain is a look, not a choice: the same layer and settings
/// always give the same grain, so an Action replays exactly (the precedent
/// is `sampling_radial::RADIAL_DRAFT_SEED`). The bytes spell "RZ NOISE".
pub(crate) const NOISE_SEED: u64 = 0x525A_204E_4F49_5345;

/// `A`: the coverage's fall per unit of noise (module doc, take 2's ADD: 0).
pub(crate) const NOISE_BIAS: f32 = 0.0;

/// `B`: the half-width of the coverage's scaling per unit of noise (module
/// doc, take 2's ADD: 0).
pub(crate) const NOISE_SPREAD: f32 = 0.0;

/// `C`: the half-width, in codes, of the uniform added per unit of noise
/// (module doc, take 2's ADD: 255).
pub(crate) const NOISE_ADDITIVE: f32 = 255.0;

/// Take 2's SITE winner: the noise runs after the contour's map (module
/// doc).
pub(crate) const NOISE_AFTER_CONTOUR: bool = true;

/// `u` in `[0, 1)` at layer pixel `(lx, ly)` of `stream` (module doc).
pub(crate) fn noise_unit(stream: u64, lx: i64, ly: i64) -> f32 {
    SplitMix64::at(NOISE_SEED, lx as u64, ly as u64, stream).next_unit_f32()
}

/// `round(clamp(v (1 - A n + B n (2u - 1)) + C n (2u - 1), 0, 255))` in f32
/// (module doc); `v = 0` stays 0 and `amount <= 0` (or NaN) returns `v`.
pub(crate) fn noise_value(v: u8, amount: f32, u: f32) -> u8 {
    if v == 0 || amount.is_nan() || amount <= 0.0 {
        return v;
    }
    let n = amount.min(1.0);
    let t = 2.0 * u - 1.0;
    let value =
        f32::from(v) * (1.0 - NOISE_BIAS * n + NOISE_SPREAD * n * t) + NOISE_ADDITIVE * n * t;
    value.clamp(0.0, 255.0).round() as u8
}

/// The pass, in place, over a plane `w` pixels wide whose pixel `(x, y)` is
/// layer pixel `(x + origin.0, y + origin.1)`. A no-op at `amount <= 0` (or
/// NaN); polls `job` once per row. Empty pixels draw no number at all.
pub(crate) fn apply_noise_with_job(
    plane: &mut [u8],
    w: u32,
    origin: (i64, i64),
    amount: f32,
    stream: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if amount.is_nan() || amount <= 0.0 || w == 0 {
        return Ok(());
    }
    for (y, row) in plane.chunks_mut(w as usize).enumerate() {
        job.check()?;
        let ly = y as i64 + origin.1;
        for (x, v) in row.iter_mut().enumerate() {
            if *v != 0 {
                *v = noise_value(*v, amount, noise_unit(stream, x as i64 + origin.0, ly));
            }
        }
    }
    Ok(())
}

/// A Below effect's contour map and noise in ONE pass (module doc, the
/// site and the Below cap): plane value `s` becomes `min(noise(map[s]),
/// cap[s])` at [`NOISE_AFTER_CONTOUR`] (`min(map[noise(s)], cap[s])`
/// otherwise), `map` the effect's `below_lut` (`None` = Linear) and `cap`
/// its `below_cap_table`. Fused because the cap reads the plane BEFORE the
/// contour mapped it. A no-op at `amount <= 0` (or NaN) — the caller then
/// maps the contour alone; polls `job` once per row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_noise_below_with_job(
    plane: &mut [u8],
    w: u32,
    origin: (i64, i64),
    amount: f32,
    stream: u64,
    map: Option<&[u8; 256]>,
    cap: &[u8; 256],
    job: JobContext<'_>,
) -> Result<(), String> {
    if amount.is_nan() || amount <= 0.0 || w == 0 {
        return Ok(());
    }
    let mapped = |v: u8| map.map_or(v, |lut| lut[usize::from(v)]);
    for (y, row) in plane.chunks_mut(w as usize).enumerate() {
        job.check()?;
        let ly = y as i64 + origin.1;
        for (x, v) in row.iter_mut().enumerate() {
            let s = *v;
            if s == 0 {
                continue;
            }
            let u = noise_unit(stream, x as i64 + origin.0, ly);
            let noised = if NOISE_AFTER_CONTOUR {
                noise_value(mapped(s), amount, u)
            } else {
                mapped(noise_value(s, amount, u))
            };
            *v = noised.min(cap[usize::from(s)]);
        }
    }
    Ok(())
}
