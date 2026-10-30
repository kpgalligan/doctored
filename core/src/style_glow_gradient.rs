//! KERNEL: the Outer and Inner Glow's Gradient fill and its Jitter:
//! where on the gradient each pixel of a glow sits, how the stops' opacity
//! becomes the glow's coverage, and how Jitter scatters the sampling.
//!
//! # The ramp
//!
//! The glow's plane `v` (0..255, after its blur and the contour/Range map and
//! noise pass) places each pixel on the gradient at `t = 1 − v / 255`
//! ([`GLOW_RAMP_EDGE_FIRST`]): the stop at location 0 sits where the glow is
//! strongest, the end stop where it fades out. The glow's coverage is then
//! the STOPS' opacity, not the plane's falloff ([`GLOW_RAMP_ALPHA_GLOW`]
//! false): `round(min(255 a(t), cap(v₀)))` ([`GLOW_RAMP_ALPHA_MIN`]), `a`
//! linear between the stops at the unsmoothed `t`
//! ([`GLOW_RAMP_SMOOTH_OPACITY`] false), with `cap(v₀) = min(255, round(k
//! v₀))` on the Outer Glow — `style_contour`'s Below cap `k`
//! ([`crate::style_contour::BELOW_CAP_OUTER_GLOW`]), the same bound its colour
//! plane and its noise are held to, read on the plane BEFORE the contour/Range
//! map ([`GLOW_CAP_BEFORE_CONTOUR`]) as the noise's is — and 255 wherever
//! `v > 0` on the Inner Glow. So the default Foreground to Transparent
//! glow draws the colour glow's falloff. The COLOUR is read at the smoothed
//! `t'`: between the bracketing stops `p_i < p_j` the fraction `f` becomes
//! `S(f) = (f + 3f² − 2f³) / 2` ([`GLOW_RAMP_SMOOTH`]), the curve Smoothness
//! 100 % (`Intr` 4096, which every glow gradient Photoshop reads or writes
//! carries) draws. `t'` is what the per-pixel field
//! (`style_gradient::BurstField`, shared with the Stroke's Shape Burst)
//! stores, so every renderer samples the colour through the ONE
//! `GradientSampler` and interpolates linearly from there.
//!
//! Why: `scripts/photoshop-probe/glow_gradient_take.py` fitted the law on
//! the RECORD rows noise take 2 and texture take 3 hold (Photoshop 26.10.0),
//! each against its black colour twin's plane. Every declared candidate
//! missed (EDGE-FIRST with the stops' opacity was the best, worst-row mean
//! 14.8 codes); the REFIT declared before take 4 (`a(t') cap(v)`, both read
//! at the smoothed `t'`, the cap on the mapped plane) met every fit row, and
//! take 4 (run rz-557-glowgrad-20261016T181746Z-c1baddc1) confirmed it at
//! size 20, on a red ramp and on an Inner Glow Center, and Jitter's SPREAD-1,
//! but MISSED its opacity row (100 → 50 %: max 7 codes) and both Range rows
//! (Range 25: max 25, mean 4.7). The law above was read off take 4's own
//! tables AFTER the take — the opacity is not smoothed, it MEETS the cap
//! rather than multiplying it, and the cap reads the unmapped plane — and
//! meets every row of all three takes (worst max 6, mean 1.0; Range 25 3 /
//! 0.26). It ships as a HYPOTHESIS a fresh take would confirm (noise take 2's
//! FIT precedent); the take's own winner is one constant flip away. The
//! Stroke's and the Gradient Overlay's Linear, Angle, Reflected and Radial fills, and
//! the Stroke's Shape Burst, read the Classic ramp
//! (`gradient_field::classic_color`), whose two-stop form is this `S`;
//! a glow keeps its own per-segment `S`, already in its field, so its
//! sampler (a field attached, style Linear) stays linear.
//!
//! # Jitter
//!
//! `t` is scattered BEFORE the smoothing: `t + j (2u − 1)`, clamped to
//! 0..1, `j` the Jitter in 0..1 and `u` uniform in [0, 1)
//! ([`GLOW_JITTER_LAW`]: SPREAD-1, the fit's and take 4's, against half that
//! spread and against resampling `t` outright with probability `j`). `u` is
//! drawn POSITIONALLY from the crate's one generator, `SplitMix64::at(
//! GLOW_JITTER_SEED, lx, ly, stream)`, at the pixel's LAYER coordinate and
//! the effect kind as the stream — `style_noise`'s field, with its own seed
//! — so the scatter moves with the layer, a sub-plane draws exactly the full
//! plane's values, and an Action replays it exactly (Photoshop's REPEAT
//! rows were identical across two takes). Take 4's MOVE row found
//! Photoshop's Jitter fixed to the CANVAS, as its Noise is; keeping it
//! layer-local is the recorded residual (a canvas field would make every
//! Move re-render the style).
//!
//! # The site
//!
//! The ramp reads `t` from the plane after the contour/Range map
//! ([`GLOW_RAMP_SITE_AFTER_CONTOUR`], take 4's SITE best fit) and runs
//! before the outer glow's knock-out or the inner glow's coverage multiply,
//! so neither reaches past the shape's own clip.

use std::sync::Arc;

use crate::job_control::JobContext;
use crate::rng::SplitMix64;
use crate::style_gradient::{gradient_color, BurstField, GradientSampler};
use crate::style_model::{GlowFill, GradientFill};
use crate::style_render::PlaneColor;

/// The fixed stream Jitter is drawn from (module doc): the bytes spell
/// "RZJITTER". Fixed for the reason `style_noise::NOISE_SEED` is.
pub(crate) const GLOW_JITTER_SEED: u64 = 0x525A_4A49_5454_4552;

/// The SITE: true reads `t` from the plane after the contour/Range map
/// (module doc), false from the blurred plane before it.
pub(crate) const GLOW_RAMP_SITE_AFTER_CONTOUR: bool = true;

/// The MAP: true is `t = 1 − v` (EDGE-FIRST), false `t = v` (module doc).
pub(crate) const GLOW_RAMP_EDGE_FIRST: bool = true;

/// The ALPHA: true multiplies the plane by the stop opacity (`v a(t)`),
/// false takes the stop opacity as the coverage, held to the Below cap
/// where there is one (module doc).
pub(crate) const GLOW_RAMP_ALPHA_GLOW: bool = false;

/// How the stop opacity meets the cap: true `min(255 a, cap)` (the
/// post-take hypothesis), false `a cap` (take 4's REFIT; module doc).
pub(crate) const GLOW_RAMP_ALPHA_MIN: bool = true;

/// The RAMP: true reads the colour through the smoothing `S(f)`
/// within each segment (module doc), false interpolates linearly.
pub(crate) const GLOW_RAMP_SMOOTH: bool = true;

/// Whether the opacity is read at the smoothed `t'` too (take 4's REFIT)
/// rather than at `t` (the post-take hypothesis; module doc).
pub(crate) const GLOW_RAMP_SMOOTH_OPACITY: bool = false;

/// Whether the Outer Glow's cap reads the plane before the contour/Range
/// map (the post-take hypothesis) rather than after it (module doc).
pub(crate) const GLOW_CAP_BEFORE_CONTOUR: bool = true;

/// How Jitter scatters `t` (module doc).
#[derive(Clone, Copy)]
pub(crate) enum JitterLaw {
    /// `t + j k (2u − 1)`: SPREAD-1 is `k` 1, SPREAD-HALF `k` ½.
    Spread(f32),
    /// `t := u₂` with probability `j`, else `t` (the fit's losing candidate,
    /// kept so the constant below can name it).
    #[allow(dead_code)]
    Resample,
}

/// The fit's and take 4's JITTER winner (module doc).
pub(crate) const GLOW_JITTER_LAW: JitterLaw = JitterLaw::Spread(1.0);

/// The Outer Glow's Below cap (module doc): its `k`, and the plane it reads
/// (`None`: the ramped plane itself).
#[derive(Clone, Copy)]
pub(crate) struct GlowCap<'a> {
    pub k: f64,
    pub plane: Option<&'a [u8]>,
}

/// The gradient parameter of plane value `v` under [`GLOW_RAMP_EDGE_FIRST`],
/// scattered by `jitter` (0..=1; 0 or NaN draws nothing) under
/// [`GLOW_JITTER_LAW`] from `draw`, clamped to 0..1 (module doc). Not yet
/// smoothed ([`smooth_t`]).
pub(crate) fn glow_t(v: u8, jitter: f32, draw: &mut SplitMix64) -> f32 {
    let v = f32::from(v) / 255.0;
    let t = if GLOW_RAMP_EDGE_FIRST { 1.0 - v } else { v };
    if jitter.is_nan() || jitter <= 0.0 {
        return t;
    }
    let j = jitter.min(1.0);
    match GLOW_JITTER_LAW {
        JitterLaw::Spread(k) => {
            let u = draw.next_unit_f32();
            (t + j * k * (2.0 * u - 1.0)).clamp(0.0, 1.0)
        }
        JitterLaw::Resample => {
            let u = draw.next_unit_f32();
            let u2 = draw.next_unit_f32();
            if u < j {
                u2
            } else {
                t
            }
        }
    }
}

/// `t` read through the smoothing within its segment (module doc):
/// `p_i + (p_j − p_i) S(f)`, the bracket from `gradient_field::segment` (the
/// one search of a stops list); `t` at or past an end stop is unchanged.
pub(crate) fn smooth_t(fill: &GradientFill, t: f32) -> f32 {
    let stops = &fill.stops;
    match crate::gradient_field::segment(stops.len(), t, |i| stops[i].position) {
        Some((i, j, f)) if i != j => {
            let (a, b) = (stops[i].position, stops[j].position);
            a + (b - a) * (f + 3.0 * f * f - 2.0 * f * f * f) / 2.0
        }
        _ => t,
    }
}

/// The stop opacity of `fill` at `t`, clamped to 0..1 (0 for NaN).
fn opacity_at(fill: &GradientFill, t: f32) -> f64 {
    let (_, a) = gradient_color(fill, t);
    if a.is_finite() {
        f64::from(a.clamp(0.0, 1.0))
    } else {
        0.0
    }
}

/// The glow's per-pixel `t` field and its coverage, in one pass over a plane
/// `w` pixels wide whose pixel `(x, y)` is layer pixel `(x + origin.0, y +
/// origin.1)` (module doc). Returns `round(65535 t')` per pixel, `t'` the
/// jittered and smoothed parameter the colour is read at (0 where the plane
/// is 0), and rewrites `plane` to the coverage (module doc, the ramp): the
/// stop opacity `a` against `cap`'s `min(255, round(k v₀))`, `v₀` the cap's
/// own plane (`None`: the plane itself), or 255 without a cap. `source` is
/// the plane `t` is read from when the site is BEFORE the contour (`None`:
/// the plane itself). `stream` is the effect kind (module doc, Jitter).
/// Polls `job` once per row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ramp_field_with_job(
    plane: &mut [u8],
    source: Option<&[u8]>,
    w: u32,
    origin: (i64, i64),
    fill: &GradientFill,
    jitter: f32,
    stream: u64,
    cap: Option<GlowCap<'_>>,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let mut field = vec![0u16; plane.len()];
    if w == 0 {
        return Ok(field);
    }
    let wu = w as usize;
    for (y, row) in plane.chunks_mut(wu).enumerate() {
        job.check()?;
        let ly = y as i64 + origin.1;
        for (x, v) in row.iter_mut().enumerate() {
            if *v == 0 {
                continue;
            }
            let i = y * wu + x;
            let s = source.and_then(|src| src.get(i).copied()).unwrap_or(*v);
            let mut draw = SplitMix64::at(
                GLOW_JITTER_SEED,
                (x as i64 + origin.0) as u64,
                ly as u64,
                stream,
            );
            let t = glow_t(s, jitter, &mut draw);
            let smoothed = if GLOW_RAMP_SMOOTH {
                smooth_t(fill, t)
            } else {
                t
            };
            let code = (smoothed.clamp(0.0, 1.0) * 65535.0).round() as u16;
            field[i] = code;
            let a = if GLOW_RAMP_SMOOTH_OPACITY {
                // The stored parameter, as the colour reads it.
                opacity_at(fill, f32::from(code) / 65535.0)
            } else {
                opacity_at(fill, t)
            };
            *v = if GLOW_RAMP_ALPHA_GLOW {
                (f64::from(*v) * a).round()
            } else {
                let limit = cap.map_or(255.0, |cap| {
                    let v0 = cap.plane.and_then(|p| p.get(i).copied()).unwrap_or(*v);
                    (cap.k * f64::from(v0)).round().min(255.0)
                });
                if GLOW_RAMP_ALPHA_MIN {
                    (255.0 * a).min(limit).round()
                } else {
                    (limit * a).round()
                }
            }
            .clamp(0.0, 255.0) as u8;
        }
    }
    Ok(field)
}

/// Whether a glow's `gradient` is part of what it stores: its Fill is
/// Gradient, or the stops are not the default (a Color glow that was once a
/// Gradient one keeps its stops for the sheet). The JSON writer, the colour
/// slot walk and `.rz` word 99 key on it.
pub(crate) fn glow_carries_stops(fill_type: GlowFill, gradient: &GradientFill) -> bool {
    fill_type == GlowFill::Gradient || gradient.stops != GradientFill::default().stops
}

/// A gradient glow's colour (module doc): the ONE `GradientSampler` over
/// `fill` reading `t` (from [`ramp_field_with_job`]) on a `w × h` plane whose
/// pixel (0, 0) is layer pixel `origin`. The box is unused: an attached
/// field wins whatever the style (`style_gradient`).
pub(crate) fn ramp_color(
    fill: &GradientFill,
    origin: (i64, i64),
    w: u32,
    h: u32,
    t: Vec<u16>,
) -> PlaneColor {
    let field = Arc::new(BurstField {
        origin,
        w,
        h,
        t: Arc::from(t),
    });
    PlaneColor::Gradient(GradientSampler::new(fill, 0.0, 0.0, 1.0, 1.0).with_burst(field))
}
