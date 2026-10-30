//! Dehaze: the haze model `I = J·t + A·(1 − t)` on one plane of
//! straight samples — `I` the picture as stored, `J` the scene, `A` the
//! airlight (the colour of the haze itself) and `t` the transmission, the share
//! of the scene's light that reaches the camera. A positive amount estimates
//! `A` and `1 − t` and solves for `J`; a negative amount runs the model
//! forward, mixing the picture toward `A`.
//!
//! The estimate is the dark-channel prior: in a haze-free picture nearly every
//! neighbourhood holds a pixel with one colour channel near 0, so whatever the
//! darkest channel shows in a hazy one is haze. He, Sun and Tang take that
//! statistic as a minimum over a square patch and then refine the blocky result
//! with a guided filter steered by the picture. This crate has neither a
//! minimum filter nor a cross-guided filter, and writing either would be a
//! second neighbourhood beside the ones it has. It does have
//! `sampling_denoise`'s self-guided filter, whose cost per pixel does not depend
//! on its radius, so the route here is Tarel and Hautière's form of the same
//! prior: take each pixel's own dark value `W = min_c(I_c / A_c)`, smooth it
//! with that one edge-aware pass (`sampling_presence::base_delta`), and keep the
//! smaller of the smooth value and `W` itself. One pass is then both the patch
//! statistic (texture inside a region averages away) and the edge-aware
//! refinement (a depth edge is a step in `W`, which the guided filter keeps),
//! and `min(B, W)` holds the veil under every pixel's own dark value, so the
//! recovery cannot push a channel below 0 by more than the airlight's own
//! estimate allows.
//!
//! The model is applied to the stored encoded samples: the kernel takes no
//! profile, as every other destructive filter on this route does not.
//!
//! **D, Dehaze at 32 bits.** Dehaze is display-referred (its veil
//! epsilon, divisor floor and transmission floor are display levels), so on a
//! float layer it runs on the display-encoded copy (`display_encoding`), like
//! Texture and Clarity. The airlight and the veil are measured on that copy
//! with RGB clamped to 0…1 ([`unit_rgb`]): the picture the 16-bit filter sees
//! with above white clipped, so `A ≤ 1` in every channel and an above-white
//! highlight cannot drag the airlight. At a positive amount, with `t` as at
//! 16 bits, a component `I ≤ 1` takes the 16-bit expression
//! `clamp((I − A)/t + A, min(I, 0), 1)` and a component `I > 1` is returned
//! as read: with
//! `A ≤ 1` the recovery maps white to 1 after its clamp, so white is a fixed
//! point and above white is held there. At a negative amount the 16-bit mix
//! `I + (A − I)·mix` runs unclamped, a convex step toward `A ≤ 1`. No
//! component moves above `max(I, 1)` at any amount ([`dehaze_in`]).
//! Linear-light application is not used: a veil measured on encoded values
//! overstates the linear veil below the airlight (0.3 under a 4.0 grey haze at
//! amount 50 would recover to −0.125).
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::sampling_presence::base_delta;

/// The airlight is the mean colour of the pixels whose dark value is in this
/// top share (He, Sun and Tang's 0.1%): the haziest pixels of a picture are its
/// most distant, and enough of them are averaged that one bright speck does not
/// set the colour.
const AIRLIGHT_TOP_SHARE: f64 = 0.001;
/// Bins of the dark-value histogram that finds that share: one per 16-bit
/// code, so the threshold is as fine as either depth this filter runs at.
const DARK_BINS: usize = 65_536;
/// The floor of the airlight where the veil divides by it, and nowhere else.
/// `W = min_c(I_c / A_c)` is undefined for a channel the airlight does not
/// have (a picture whose haziest pixels are pure red has `A_g = A_b = 0`), and
/// a quotient by a channel a few codes above 0 would turn sample rounding into
/// a full veil; 0.05, about 13 8-bit levels, is below the airlight of any
/// picture that is actually hazy. It is on the DIVISOR ONLY: the recovery and
/// the forward mix take the airlight as estimated, because a floored airlight
/// there would move a pixel that equals the airlight — every pixel of a dark
/// flat picture — toward a colour the picture does not contain.
const AIRLIGHT_DIVISOR_FLOOR: f64 = 0.05;
/// The guided filter's epsilon, as the level `sampling_denoise` squares: a step
/// in the dark value taller than about a tenth of the range is a depth edge and
/// is kept; anything lower is scene texture and is averaged into the veil.
const VEIL_EPSILON_LEVEL: f64 = 0.1;
/// The share of the estimated veil that amount 100 removes. He, Sun and Tang
/// keep 5% of the haze so that distance still reads as distance; taking all of
/// it also divides by a transmission of 0 wherever the veil is complete.
const DEHAZE_STRENGTH: f64 = 0.95;
/// The least transmission the recovery divides by (He, Sun and Tang's `t0`):
/// below a tenth, `(I − A)/t` multiplies the sample rounding of a sky by more
/// than ten and shows it as noise.
const TRANSMISSION_FLOOR: f64 = 0.1;
/// How far amount −100 mixes the picture toward the airlight: 0.6 leaves a
/// heavy fog in which the scene is still legible, where 1.0 would be the flat
/// airlight colour, a picture with nothing in it.
const HAZE_MIX: f64 = 0.6;

/// A pixel's dark value on the airlight's scale, `min_c(min(1, I_c / max(A_c,
/// AIRLIGHT_DIVISOR_FLOOR)))`: 1 where the pixel is the airlight or brighter in
/// every channel, 0 where one channel is 0.
fn dark(sample: [f32; 4], airlight: [f64; 3]) -> f64 {
    let mut least = 1.0_f64;
    for c in 0..3 {
        least = least.min(f64::from(sample[c]) / airlight[c].max(AIRLIGHT_DIVISOR_FLOOR));
    }
    least.max(0.0)
}

/// The histogram bin of a pixel's raw dark value `min(R, G, B)`.
fn bin(sample: [f32; 4]) -> usize {
    let least = sample[0].min(sample[1]).min(sample[2]);
    (f64::from(least).clamp(0.0, 1.0) * (DARK_BINS - 1) as f64).round() as usize
}

/// The airlight of the `n` samples `read` answers: the mean colour of the
/// pixels with alpha above 0 whose dark value `min(R, G, B)` is in the top
/// `AIRLIGHT_TOP_SHARE`, at least one pixel. A histogram finds the bin where
/// that share is reached and every pixel at or above the bin counts, so a
/// picture of one colour answers that colour. The mean is UNFLOORED (see
/// `AIRLIGHT_DIVISOR_FLOOR`). `None`: no pixel has alpha above 0.
pub(crate) fn airlight(
    read: impl Fn(usize) -> [f32; 4],
    n: usize,
    job: JobContext<'_>,
) -> Result<Option<[f64; 3]>, String> {
    let mut bins: Vec<u64> = reserved_vec(DARK_BINS)?;
    bins.resize(DARK_BINS, 0);
    let mut visible = 0_u64;
    for i in 0..n {
        job.every(i)?;
        let sample = read(i);
        if sample[3] > 0.0 {
            bins[bin(sample)] += 1;
            visible += 1;
        }
    }
    if visible == 0 {
        return Ok(None);
    }
    let wanted = ((visible as f64 * AIRLIGHT_TOP_SHARE) as u64).max(1);
    let (mut threshold, mut seen) = (0_usize, 0_u64);
    for (index, count) in bins.iter().enumerate().rev() {
        seen += count;
        if seen >= wanted {
            threshold = index;
            break;
        }
    }
    drop(bins);
    let (mut sum, mut count) = ([0.0_f64; 3], 0_u64);
    for i in 0..n {
        job.every(i)?;
        let sample = read(i);
        if sample[3] > 0.0 && bin(sample) >= threshold {
            for c in 0..3 {
                sum[c] += f64::from(sample[c]);
            }
            count += 1;
        }
    }
    Ok(Some(sum.map(|s| s / count as f64)))
}

/// The veil, the estimate of `1 − t`, one value per pixel: with `W` the
/// pixel's dark value on the airlight's scale and `B` its edge-aware smooth at
/// `radius` (`W + base_delta` of the grey plane `[W, W, W, alpha]`),
/// `clamp(min(B, W), 0, 1)`. Takes the unfloored airlight and floors only its
/// own divisor. `W` is recomputed from `read` rather than held, so the pass
/// holds one guided transient and the plane it returns.
pub(crate) fn veil(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    airlight: [f64; 3],
    radius: u32,
    cap: u64,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let grey = |i: usize| {
        let sample = read(i);
        let w = dark(sample, airlight) as f32;
        [w, w, w, sample[3]]
    };
    let mut plane = base_delta(grey, dimensions, radius, VEIL_EPSILON_LEVEL, cap, job)?;
    for (i, value) in plane.iter_mut().enumerate() {
        job.every(i)?;
        let w = dark(read(i), airlight);
        *value = (w + f64::from(*value).min(0.0)).clamp(0.0, 1.0) as f32;
    }
    Ok(plane)
}

/// One sample with its RGB clamped to 0…1, alpha kept: what the airlight and
/// the veil read at 32 bits (module doc, D).
pub(crate) fn unit_rgb(sample: [f32; 4]) -> [f32; 4] {
    [
        sample[0].clamp(0.0, 1.0),
        sample[1].clamp(0.0, 1.0),
        sample[2].clamp(0.0, 1.0),
        sample[3],
    ]
}

/// One pixel dehazed by `amount` (−100…100; 0 and an alpha-0 pixel are
/// returned as read). At `Unit`, the 8- and 16-bit expression: positive, `t =
/// max(1 − DEHAZE_STRENGTH·(amount/100)·veil, TRANSMISSION_FLOOR)` and `out_c
/// = clamp((I_c − A_c)/t + A_c, 0, 1)`; negative, `out_c = I_c + (A_c −
/// I_c)·HAZE_MIX·|amount|/100`, which reads no veil. Takes the unfloored
/// airlight and never divides by it, so a pixel with `I = A` comes back as
/// read at every amount. At `Extended`, rule D (module doc) on a
/// display-encoded sample with an airlight measured through [`unit_rgb`]: at
/// a positive amount a component above 1 is returned as read and one at or
/// below 1 is clamped to `min(I, 0)…1`; the negative mix is the same.
pub(crate) fn dehaze_in(
    sample: [f32; 4],
    airlight: [f64; 3],
    veil: f32,
    amount: f64,
    range: &SampleRange,
) -> [f32; 4] {
    if amount == 0.0 || sample[3] <= 0.0 {
        return sample;
    }
    let extended = matches!(range, SampleRange::Extended { .. });
    let mut out = sample;
    if amount > 0.0 {
        let t =
            (1.0 - DEHAZE_STRENGTH * (amount / 100.0) * f64::from(veil)).max(TRANSMISSION_FLOOR);
        for c in 0..3 {
            let value = f64::from(sample[c]);
            if extended && value > 1.0 {
                continue;
            }
            let floor = if extended { value.min(0.0) } else { 0.0 };
            out[c] = ((value - airlight[c]) / t + airlight[c]).clamp(floor, 1.0) as f32;
        }
    } else {
        let mix = HAZE_MIX * -amount / 100.0;
        for c in 0..3 {
            let value = f64::from(sample[c]);
            out[c] = (value + (airlight[c] - value) * mix) as f32;
        }
    }
    out
}
