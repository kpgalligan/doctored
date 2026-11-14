//! Select ▸ Focus Area's pixel math (`designs/focus-area.md`): byte coverage
//! of the in-focus part of one straight RGBA8 picture.
//!
//! 1. **Measure.** `focus_stack::sharpness`, the two-band score Stack Images
//!    ranks frames by, over Rec. 709 luma (`blend::LUMA_*`): local detail
//!    energy in unit² per pixel, 0 where the picture is transparent.
//! 2. **Floor.** Energy below [`FLOOR`] is no detail at all: a flat area can
//!    be in focus or not and nothing in it says which, so the measure never
//!    selects it (step 4 may).
//! 3. **Threshold.** In `ln(score)` from `ln(FLOOR)` to the picture's largest:
//!    `range` 0…100 puts the cut that fraction of the way up, so a lower
//!    `range` selects more. Without one, Auto is Otsu's cut over a
//!    [`BINS`]-bin histogram of the opaque pixels above the floor, reported as
//!    `100·(k + 1)/BINS` — a dyadic fraction, so the reported `range` maps
//!    back onto the very same cut and a recorded step replays bit for bit.
//!    When Otsu's two classes' mean `ln(score)` differ by less than
//!    [`ONE_CLASS`], the picture is one class (in focus everywhere it has
//!    detail, or nowhere) and Auto takes range 0. No pixel above the floor:
//!    nothing is selected, and Auto reports 100.
//! 4. **Regions.** One majority vote over the stack's own vote box (a pixel
//!    is selected when more than half of the opaque pixels in its box are; a
//!    tie keeps its own), then every unselected opaque region that touches
//!    neither the canvas border nor a transparent pixel is filled
//!    (`selection_quick::fill_holes`, uncapped): a sharp subject's smooth
//!    interior belongs to it. Coverage is the source alpha.
//! 5. **Edge.** `soften_edge` feathers by the coarse band's radius
//!    (`gaussian_plane::feather`), then caps each pixel by its alpha, so a
//!    transparent pixel stays outside.
use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::job_control::JobContext;
use crate::sampling_box::box_sum;
use crate::selection_tools::object;
use serde_json::Value;

/// The detail energy below which a pixel has no detail: a luma deviation of
/// one 8-bit code RMS, `(1/255)²`. Quantization alone leaves a smooth
/// gradient well under it.
const FLOOR: f64 = 1.0 / (255.0 * 255.0);
/// Auto's histogram bins over `ln(FLOOR) … ln(max)`.
const BINS: usize = 256;
/// Otsu's classes closer than this in mean `ln(score)` are one class:
/// `ln 4`, a fourfold energy ratio, twice the RMS detail. A defocused region
/// keeps far less than half its detail; a split closer than that is noise in
/// a picture with one focus.
const ONE_CLASS: f64 = std::f64::consts::LN_2 * 2.0;

/// A parsed request: `range` None is Auto.
pub(crate) struct FocusAreaParams {
    range: Option<f64>,
    soften_edge: bool,
}

/// Strict JSON `{range?: 0..100, soften_edge?: bool}`.
pub(crate) fn parse(json: &str) -> Result<FocusAreaParams, String> {
    let map = object(json, &["range", "soften_edge"])?;
    // Read in binary64, not through `selection_tools::number`'s binary32, so
    // a range the reply reported comes back as the very same cut.
    let range = match map.get("range") {
        None | Some(Value::Null) => None,
        Some(value) => match value.as_f64() {
            Some(range) if (0.0..=100.0).contains(&range) => Some(range),
            _ => return Err("range must be a number from 0 to 100".into()),
        },
    };
    let soften_edge = match map.get("soften_edge") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("soften_edge must be true or false".into()),
    };
    Ok(FocusAreaParams { range, soften_edge })
}

/// Scratch bytes beyond the output plane: the score (4), the two box sums
/// (16, one at a time with the vote plane, 1), the hole fill's component
/// list and bitset (8 and 1/8), the feather's plane (4), plus the sharpness
/// rings.
pub(crate) fn plan(size: (u32, u32)) -> u64 {
    let n = u64::from(size.0) * u64::from(size.1);
    n.saturating_mul(4 + 16 + 1 + 8 + 4 + 1)
        .saturating_add(crate::focus_stack::sharpness_ring_bytes(size))
        .saturating_add(1 << 20)
}

/// Focus Area's coverage over `rgba` (straight RGBA8, `size`), and the
/// `range` it used.
pub(crate) fn focus_area(
    rgba: &[u8],
    size: (u32, u32),
    params: &FocusAreaParams,
    job: JobContext<'_>,
) -> Result<(Vec<u8>, f64), String> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let n = w * h;
    if n == 0 || rgba.len() != n * 4 {
        return Err("focus area source does not match its dimensions".into());
    }
    job.reserve(plan(size))?;
    let alpha = |i: usize| rgba[i * 4 + 3];
    let read = |_frame: usize, y: u32, row: &mut [[f32; 4]]| {
        let at = y as usize * w * 4;
        for (x, out) in row.iter_mut().enumerate() {
            *out = std::array::from_fn(|c| f32::from(rgba[at + x * 4 + c]) / 255.0);
        }
    };
    let (score, coarse, vote) =
        crate::focus_stack::sharpness(size, &read, [LUMA_R, LUMA_G, LUMA_B], job.range(0.0, 0.6))?;
    let level = |i: usize| -> Option<f64> {
        let s = f64::from(score[i]);
        (alpha(i) > 0 && s >= FLOOR).then(|| s.ln())
    };
    let low = FLOOR.ln();
    let high = (0..n).filter_map(level).fold(f64::NEG_INFINITY, f64::max);
    if high == f64::NEG_INFINITY {
        // No detail anywhere: nothing says any of it is in focus.
        let mut none = crate::precision_image::reserved_vec::<u8>(n)?;
        none.resize(n, 0);
        return Ok((none, params.range.unwrap_or(100.0)));
    }
    let range = match params.range {
        Some(range) => range,
        None if high > low => otsu_range(&level, n, low, high),
        None => 0.0,
    };
    let cut = low + (high - low) * (range / 100.0);
    let mut chosen: Vec<u8> = crate::precision_image::reserved_vec(n)?;
    for i in 0..n {
        job.every(i)?;
        chosen.push(u8::from(level(i).is_some_and(|l| l >= cut)));
    }
    drop(score);
    job.report(0.7, 6)?;
    // The vote: selected and opaque counts over the box, one plane at a time.
    let selected = box_sum(|i| f64::from(chosen[i]), w, h, vote, job)?;
    let mut coverage = crate::precision_image::reserved_vec::<u8>(n)?;
    {
        let opaque = box_sum(|i| f64::from(u8::from(alpha(i) > 0)), w, h, vote, job)?;
        for i in 0..n {
            job.every(i)?;
            let twice = 2.0 * selected[i];
            let on = alpha(i) > 0 && (twice > opaque[i] || (twice == opaque[i] && chosen[i] == 1));
            coverage.push(if on { alpha(i) } else { 0 });
        }
    }
    drop(selected);
    drop(chosen);
    job.report(0.8, 6)?;
    let mut list = Vec::new();
    crate::selection_quick::fill_holes(&mut coverage, (w, h), &alpha, n, &mut list, job)?;
    drop(list);
    if params.soften_edge {
        crate::gaussian_plane::feather(&mut coverage, size.0, size.1, coarse as f32, job)?;
        for (i, value) in coverage.iter_mut().enumerate() {
            *value = (*value).min(alpha(i));
        }
    }
    Ok((coverage, range))
}

/// Auto's `range`: Otsu's cut over the opaque pixels above the floor, or 0
/// when the two classes are one (module doc, step 3).
fn otsu_range(level: &impl Fn(usize) -> Option<f64>, n: usize, low: f64, high: f64) -> f64 {
    let mut histogram = [0f64; BINS];
    let scale = BINS as f64 / (high - low);
    for l in (0..n).filter_map(level) {
        histogram[(((l - low) * scale) as usize).min(BINS - 1)] += 1.0;
    }
    let total: f64 = histogram.iter().sum();
    let sum: f64 = histogram
        .iter()
        .enumerate()
        .map(|(k, c)| k as f64 * c)
        .sum();
    let (mut weight, mut below) = (0.0, 0.0);
    let mut best: Option<(f64, usize, f64)> = None;
    for (k, &count) in histogram.iter().enumerate().take(BINS - 1) {
        weight += count;
        below += k as f64 * count;
        let above = total - weight;
        if weight == 0.0 || above == 0.0 {
            continue;
        }
        let (mean_low, mean_high) = (below / weight, (sum - below) / above);
        let between = weight * above * (mean_high - mean_low).powi(2);
        if best.is_none_or(|(b, _, _)| between > b) {
            best = Some((between, k, mean_high - mean_low));
        }
    }
    match best {
        // The class means are in bins; one bin is `(high − low) / BINS`.
        Some((_, k, gap)) if gap / scale >= ONE_CLASS => 100.0 * (k + 1) as f64 / BINS as f64,
        _ => 0.0,
    }
}
