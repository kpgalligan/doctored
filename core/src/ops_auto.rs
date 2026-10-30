//! Auto Tone, Auto Contrast and Auto Color: the three menu commands that
//! derive LEVELS parameters from an image's own histogram and then run the
//! existing levels math (`ops_filters::levels_channels`). They are commands,
//! not adjustment layers — there is nothing to re-edit once the numbers are
//! derived.
//!
//! Everything here reads [`ops_stats::histogram`] and its ONE counting rule,
//! so "the image's own histogram" has a single definition in this build and
//! a transparent surround cannot pin the black point at 0.
//!
//! # The three modes
//!
//! * **Auto Tone** — per channel, a black and a white point that clip about
//!   `clip` of the counted pixels at each end ([`tone_endpoints`]: the
//!   count rule's black and the white walk fitted to Photoshop);
//!   gamma 1. Stretches each channel independently, so it also removes a
//!   colour cast.
//! * **Auto Contrast** — one black/white pair taken from the LUMA histogram
//!   and applied to all three channels, so the colour balance is untouched;
//!   gamma 1.
//! * **Auto Color** — Auto Tone's per-channel endpoints plus a per-channel
//!   gamma that snaps the MIDTONES neutral.
//!
//! # Why Auto Color is a neutral-candidate snap and not gray-world
//!
//! Gray-world — force the three channel means to agree — neutralises a
//! sunset, a forest or a brick wall, whose channel means are legitimately
//! unequal. Instead only pixels that ALREADY look neutral vote: counted
//! pixels whose post-stretch `max - min` is under 0.1 and whose Rec. 709
//! luma is in 0.25..=0.75. If fewer than 0.5 % of the counted pixels
//! qualify, the picture has no neutral to snap to and every gamma stays 1.
//!
//! The gamma itself is `ln(m_c) / ln(m)`, which is the correct inversion for
//! this codebase's `out = t^(1/gamma)` (`ops_filters::levels_channels`) above
//! the Levels toe (`adjust_levels::stage_curve`, #578), which at the gammas
//! this reaches touches only t < 1/8 (x0's maximum, at γ = 2), never the
//! midtones it snaps, and it is CLAMPED into [0.5, 2.0] — deliberately
//! tight, because it bounds how far an automatic command may rewrite the
//! colour of a scene. The logarithms
//! are guarded BEFORE they are taken rather than by clamping afterwards:
//! `f32::clamp` propagates NaN, and a NaN gamma would fail
//! `levels_channels`' range check and silently refuse the whole op.

use image::RgbaImage;

use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::ops_stats::{counted, histogram};

/// Which of the three commands. Mirrors `DrAutoMode` in the C header.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoMode {
    Tone,
    Contrast,
    Color,
}

impl AutoMode {
    /// Maps a raw `DrAutoMode` value coming across the FFI.
    pub(crate) fn from_c(value: i32) -> Option<AutoMode> {
        match value {
            0 => Some(AutoMode::Tone),
            1 => Some(AutoMode::Contrast),
            2 => Some(AutoMode::Color),
            _ => None,
        }
    }
}

/// Share of the counted pixels a candidate set must reach before Auto
/// Color's midtone snap is trusted at all.
const MIN_CANDIDATE_SHARE: f64 = 0.005;

/// How near-neutral a pixel must be to vote, as post-stretch `max - min`.
const NEUTRAL_SPREAD: f32 = 0.1;

/// How far an automatic command may bend a channel's midtones.
const GAMMA_LIMIT: f32 = 2.0;

/// Levels parameters — `(black, white, gamma)`, three floats each — derived
/// from `img`'s own histogram, clipping `clip` of the counted pixels at each
/// end. `mask` is optional (`None` = the whole image).
///
/// `None` when `clip` is outside [0, 0.1] or not finite, when nothing is
/// counted (an empty or wholly transparent image, or a mask that covers
/// nothing), or when the result would be the IDENTITY — a command that
/// changes nothing must register no undo step.
pub(crate) fn auto_levels(
    img: &RgbaImage,
    mask: Option<&[u8]>,
    mode: AutoMode,
    clip: f32,
) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
    if !clip.is_finite() || !(0.0..=0.1).contains(&clip) {
        return None;
    }
    let (bins, total) = histogram(img, mask, 1);
    if total == 0 {
        return None;
    }
    let cut = f64::from(clip) * total as f64;

    let mut gamma = [1.0f32; 3];
    let (black, white) = if mode == AutoMode::Contrast {
        // Auto Contrast takes one pair from the luma histogram (plane 3) and
        // gives it to all three channels; the other two are per channel.
        match contrast_endpoints(&bins[768..1024], cut, 1.0) {
            Some((lo, hi)) => ([lo; 3], [hi; 3]),
            None => ([0.0; 3], [1.0; 3]),
        }
    } else {
        let planes = [&bins[0..256], &bins[256..512], &bins[512..768]];
        tone_endpoints(planes, cut, 1.0, |add| {
            for (i, px) in img.pixels().enumerate() {
                if counted(px, mask, i) {
                    add([px[0], px[1], px[2]].map(usize::from), 1.0);
                }
            }
            Ok(())
        })
        .ok()?
    };
    if mode == AutoMode::Color {
        gamma = neutral_gamma(img, mask, black, white, total);
    }

    let identity = black == [0.0; 3] && white == [1.0; 3] && gamma == [1.0; 3];
    (!identity).then_some((black, white, gamma))
}

/// The white's walk: the share of the crossing bin that must lie
/// inside `T` for the walk to clip it. The fit (`scripts/
/// verify_levels_auto_photoshop.py`, take 4) ties every value in 0.66-0.75;
/// 0.70 is the coarse one.
const WHITE_THETA: f64 = 0.7;
/// The white's target, `T = cut + 3·unit`: three pixels past the clip
/// fraction (take 4's fit, where takes 2 and 3 had read two).
const WHITE_BETA: f64 = 3.0;
/// The white sits on the walk's stop bin when that bin holds at least three
/// pixels' mass, else one code below the last clipped bin (MIX3).
const WHITE_STOP_PIXELS: f64 = 3.0;

/// The black code of one histogram plane at a clipping mass `cut`: the
/// lowest code whose running mass from the bottom EXCEEDS `cut` (with `cut`
/// 0, the darkest occupied code). Today's rule, kept: Photoshop's
/// black is not C3's when a dark tail ends short of `T` and a gap follows
/// (take 5 missed by up to 31 codes), so the count rule returned. `None` for
/// an empty plane.
fn black_code<T: Copy + Into<u64>>(plane: &[T], cut: f64) -> Option<usize> {
    let mut run = 0f64;
    for (v, count) in plane.iter().enumerate() {
        run += (*count).into() as f64;
        if run > cut {
            return Some(v);
        }
    }
    None
}

/// The 8-bit bin of a code of a `size`-bin plane: itself at 8 bits, and
/// `round(c/257)` (half up) at 16 bits — Photoshop walks the white over 256
/// bins at either depth (the fit's B8 binning).
fn bin8(code: usize, size: usize) -> usize {
    if size <= 256 {
        code
    } else {
        (2 * code + 257) / 514
    }
}

/// One plane binned to the white walk's 256 bins.
fn bins8<T: Copy + Into<u64>>(plane: &[T]) -> [f64; 256] {
    let mut out = [0f64; 256];
    for (v, count) in plane.iter().enumerate() {
        out[bin8(v, plane.len())] += (*count).into() as f64;
    }
    out
}

/// Where the white walk stopped: the first bin it did not clip, that bin's
/// mass, and the lowest bin it clipped.
struct WhiteWalk {
    stop: usize,
    mass: f64,
    last: Option<usize>,
}

/// The white walk (take 4's member): the occupied bins from the top,
/// `run` the mass above a bin of mass `b`, and a bin is clipped while `T −
/// run ≥ θ·b` (so a whole bin inside `T` always is, and the bin `T` crosses
/// is when at least θ of it lies inside); the plane's lowest occupied bin is
/// never clipped. `None` for an empty plane.
fn white_walk(bins: &[f64; 256], cut: f64, unit: f64) -> Option<WhiteWalk> {
    let t = cut + WHITE_BETA * unit;
    let lowest = (0..256).find(|&v| bins[v] > 0.0)?;
    let mut run = 0f64;
    let mut last = None;
    for v in (lowest..256).rev() {
        let b = bins[v];
        if b == 0.0 {
            continue;
        }
        if v != lowest && t - run >= WHITE_THETA * b {
            run += b;
            last = Some(v);
            continue;
        }
        return Some(WhiteWalk {
            stop: v,
            mass: b,
            last,
        });
    }
    None
}

/// MIX3: the stop code when it holds three pixels' mass, else one below the
/// last clipped code (the stop code when nothing was clipped).
fn place(stop: usize, mass: f64, last: Option<usize>, unit: f64) -> usize {
    match last {
        Some(code) if mass < WHITE_STOP_PIXELS * unit => code - 1,
        _ => stop,
    }
}

/// Auto Tone's (and Auto Color's) black and white per channel, as fractions
/// of each plane's code range, every caller's ONE endpoint rule.
/// `planes` are the three channels' histograms (256 or 65536 bins), `cut`
/// the clipping mass and `unit` one whole counted pixel's mass (1 for
/// counts, 255 or 65535 for a coverage-weighted plane). `visit` replays the
/// counted pixels — each one's codes at the planes' depth and its mass — for
/// the joint white; it is not called at `cut` 0.
///
/// * black: [`black_code`] on each plane, today's count rule.
/// * white, clip 0: the brightest occupied code.
/// * white, clip > 0 (`scripts/verify_levels_auto_photoshop.py`, takes 4 and
///   5; member Ns β 3 θ 0.70 MIX3 J1 B8 E0): each channel's [`white_walk`]
///   over its 8-bit bins decides which codes it clips; a pixel clipped in ANY
///   channel then leaves every channel's kept set (J1: take 4 found blue's
///   white pulled down by the red and green heads), and each channel's white
///   is [`place`] over its kept pixels — their highest bin and its mass,
///   `last` the lowest removed bin above it. Take 5: white max 14, mean 0.42
///   codes against today's 38 / 4.0; no declared member held every endpoint
///   within ±1.
///
/// A channel whose black is not below its white stays at (0, 1): a flat
/// plane is left alone rather than divided by a zero range.
pub(crate) fn tone_endpoints<T: Copy + Into<u64>>(
    planes: [&[T]; 3],
    cut: f64,
    unit: f64,
    visit: impl FnOnce(&mut dyn FnMut([usize; 3], f64)) -> Result<(), String>,
) -> Result<([f32; 3], [f32; 3]), String> {
    let size = planes[0].len();
    let scale = (size - 1) as f32;
    let mut black = [0f32; 3];
    let mut white = [1f32; 3];
    let lows = planes.map(|plane| black_code(plane, cut));
    let mut whites = [None; 3];
    if cut <= 0.0 {
        for c in 0..3 {
            whites[c] = (0..size)
                .rev()
                .find(|&v| planes[c][v].into() > 0)
                .map(|v| v as f32 / scale);
        }
    } else {
        let walks = planes.map(|plane| white_walk(&bins8(plane), cut, unit));
        if let [Some(r), Some(g), Some(b)] = &walks {
            let stops = [r.stop, g.stop, b.stop];
            let mut kept = [[0f64; 256]; 3];
            let mut removed = [[0f64; 256]; 3];
            visit(&mut |codes, mass| {
                let bins = codes.map(|code| bin8(code, size));
                let clipped = (0..3).any(|c| bins[c] > stops[c]);
                let target = if clipped { &mut removed } else { &mut kept };
                for c in 0..3 {
                    target[c][bins[c]] += mass;
                }
            })?;
            for (c, walk) in walks.iter().enumerate() {
                let walk = walk.as_ref().expect("every walk is Some here");
                let code = match (0..256).rev().find(|&v| kept[c][v] > 0.0) {
                    Some(top) => {
                        let last = (top + 1..256).find(|&v| removed[c][v] > 0.0);
                        place(top, kept[c][top], last, unit)
                    }
                    None => place(walk.stop, walk.mass, walk.last, unit),
                };
                whites[c] = Some(code as f32 / 255.0);
            }
        }
    }
    for c in 0..3 {
        if let (Some(lo), Some(hi)) = (lows[c], whites[c]) {
            let lo = lo as f32 / scale;
            if lo < hi {
                black[c] = lo;
                white[c] = hi;
            }
        }
    }
    Ok((black, white))
}

/// Auto Contrast's one black/white pair from a luma plane, as fractions:
/// [`black_code`] and, at clip > 0, the white walk placed on the plane alone
/// (one plane has nothing to share clipping with). `None` for an empty or
/// flat plane.
pub(crate) fn contrast_endpoints<T: Copy + Into<u64>>(
    plane: &[T],
    cut: f64,
    unit: f64,
) -> Option<(f32, f32)> {
    contrast_endpoints_clipped(plane, cut, cut, unit)
}

/// [`contrast_endpoints`] with the two ends clipped by different masses —
/// Auto Options' separate Shadows and Highlights clip percentages (a CMYK
/// document's Levels/Curves Auto, `cmyk_tone_analysis`): the black at
/// `shadows`, the white walk at `highlights`. The one rule; equal masses
/// are [`contrast_endpoints`] exactly.
pub(crate) fn contrast_endpoints_clipped<T: Copy + Into<u64>>(
    plane: &[T],
    shadows: f64,
    highlights: f64,
    unit: f64,
) -> Option<(f32, f32)> {
    let scale = (plane.len() - 1) as f32;
    let lo = black_code(plane, shadows)? as f32 / scale;
    let hi = if highlights <= 0.0 {
        (0..plane.len()).rev().find(|&v| plane[v].into() > 0)? as f32 / scale
    } else {
        let walk = white_walk(&bins8(plane), highlights, unit)?;
        place(walk.stop, walk.mass, walk.last, unit) as f32 / 255.0
    };
    (lo < hi).then_some((lo, hi))
}

/// Auto Color's per-channel midtone gamma (module doc). Returns all ones
/// when too few pixels look neutral or a mean sits against an end.
fn neutral_gamma(
    img: &RgbaImage,
    mask: Option<&[u8]>,
    black: [f32; 3],
    white: [f32; 3],
    total: u64,
) -> [f32; 3] {
    neutral_gamma_weighted(img, black, white, total, |i, px| {
        u64::from(counted(px, mask, i))
    })
}

fn neutral_gamma_weighted(
    img: &RgbaImage,
    black: [f32; 3],
    white: [f32; 3],
    total: u64,
    weight: impl Fn(usize, &image::Rgba<u8>) -> u64,
) -> [f32; 3] {
    let mut sums = [0f64; 3];
    let mut candidates = 0u64;
    for (i, px) in img.pixels().enumerate() {
        let mass = weight(i, px);
        if mass == 0 {
            continue;
        }
        let mut stretched = [0f32; 3];
        for (c, slot) in stretched.iter_mut().enumerate() {
            *slot = ((f32::from(px[c]) / 255.0 - black[c]) / (white[c] - black[c])).clamp(0.0, 1.0);
        }
        if !neutral_candidate(stretched) {
            continue;
        }
        for (slot, v) in sums.iter_mut().zip(stretched) {
            *slot += f64::from(v) * mass as f64;
        }
        candidates += mass;
    }
    neutral_gamma_from_sums(sums, candidates, total)
}

/// Shared neutral membership, independent of storage depth and coverage units.
pub(crate) fn neutral_candidate(stretched: [f32; 3]) -> bool {
    let spread = stretched[0].max(stretched[1]).max(stretched[2])
        - stretched[0].min(stretched[1]).min(stretched[2]);
    let luma = LUMA_R * stretched[0] + LUMA_G * stretched[1] + LUMA_B * stretched[2];
    spread < NEUTRAL_SPREAD && (0.25..=0.75).contains(&luma)
}

/// Shared weighted neutral gamma. Unit-mass legacy callers retain exactly the
/// original summation, f32 conversion and bounded-gamma rounding order.
pub(crate) fn neutral_gamma_from_sums(sums: [f64; 3], candidates: u64, total: u64) -> [f32; 3] {
    if (candidates as f64) < MIN_CANDIDATE_SHARE * total as f64 || candidates == 0 {
        return [1.0; 3];
    }
    let means = sums.map(|s| s / candidates as f64);
    let mean = (means[0] + means[1] + means[2]) / 3.0;
    if !(0.001..=0.999).contains(&mean) {
        return [1.0; 3];
    }
    let mut gamma = [1.0f32; 3];
    for (slot, m) in gamma.iter_mut().zip(means) {
        if !(0.001..=0.999).contains(&m) {
            continue;
        }
        *slot = ((m.ln() / mean.ln()) as f32).clamp(1.0 / GAMMA_LIMIT, GAMMA_LIMIT);
    }
    gamma
}

/// New explicit selected analysis: coverage is a rational weight, including1.
/// Empty coverage delegates the historical whole-target routine exactly.
type LevelsParameters = ([f32; 3], [f32; 3], [f32; 3]);

pub(crate) fn selected_auto_levels(
    img: &RgbaImage,
    coverage: &[u8],
    mode: AutoMode,
    clip: f32,
) -> Result<Option<LevelsParameters>, String> {
    if !clip.is_finite() || !(0.0..=0.1).contains(&clip) {
        return Err("clip must be0...0.1".into());
    }
    if coverage.is_empty() {
        return Ok(auto_levels(img, None, mode, clip));
    }
    if coverage.len() != img.width() as usize * img.height() as usize {
        return Err("selected Auto coverage must match source dimensions".into());
    }
    let weight = |i: usize, px: &image::Rgba<u8>| {
        if px[3] == 0 {
            0
        } else {
            u64::from(coverage[i])
        }
    };
    let mut bins = [0u64; 1024];
    let mut total = 0u64;
    for (i, px) in img.pixels().enumerate() {
        let mass = weight(i, px);
        for c in 0..3 {
            bins[c * 256 + usize::from(px[c])] += mass;
        }
        let luma =
            LUMA_R * f32::from(px[0]) + LUMA_G * f32::from(px[1]) + LUMA_B * f32::from(px[2]);
        bins[768 + (luma.round().max(0.0) as usize).min(255)] += mass;
        total += mass;
    }
    if total == 0 {
        return Ok(None);
    }
    let cut = f64::from(clip) * total as f64;
    let (black, white) = if mode == AutoMode::Contrast {
        match contrast_endpoints(&bins[768..1024], cut, 255.0) {
            Some((lo, hi)) => ([lo; 3], [hi; 3]),
            None => ([0.; 3], [1.; 3]),
        }
    } else {
        let planes = [&bins[0..256], &bins[256..512], &bins[512..768]];
        tone_endpoints(planes, cut, 255.0, |add| {
            for (i, px) in img.pixels().enumerate() {
                let mass = weight(i, px);
                if mass > 0 {
                    add([px[0], px[1], px[2]].map(usize::from), mass as f64);
                }
            }
            Ok(())
        })?
    };
    let gamma = if mode == AutoMode::Color {
        neutral_gamma_weighted(img, black, white, total, weight)
    } else {
        [1.; 3]
    };
    Ok((black != [0.; 3] || white != [1.; 3] || gamma != [1.; 3]).then_some((black, white, gamma)))
}
