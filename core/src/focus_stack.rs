//! Focus stacking's pixel math (Edit ▸ Auto-Blend Layers ▸ Stack Images):
//! which frame is sharpest at each canvas pixel ([`winners`]) and the
//! Laplacian-pyramid blend of the frames through that choice ([`blend`]).
//!
//! Free functions over one canvas rectangle. A frame is only ever a row
//! reader ([`ReadRow`]): nothing here holds a frame, so the working set does
//! not grow with the frame count, and the adapters (`doc_auto_blend`,
//! `precision_auto_blend`) decide what a sample is (a byte or u16 code over
//! its range, or a float document's retouch code).
//!
//! # The sharpness score
//!
//! Luma is the caller's `luma` weights over R, G and B (Rec. 709 on an RGB
//! document; a Lab document's L\* alone, `lab_stack::LIGHTNESS_LUMA`).
//! Per frame: luma, minus its alpha-weighted box mean, squared, box-mean
//! smoothed (again alpha-weighted), times the pixel's own alpha. That is run
//! at two radii and summed. The alpha weighting is what keeps the edge
//! between picture and transparency from counting as detail: a transparent
//! pixel's RGB never enters a mean, so a frame is not "sharp" along its own
//! outline or around a hole. A pixel a frame does not cover (alpha 0) can
//! never be won by it, whatever the score.
//!
//! The labels then take one majority vote over a box, so a pixel takes the
//! label most of its neighbourhood chose (a tie keeps its own), restricted to
//! frames that cover it. Every box sum is `sampling_box::BoxRows`, the one
//! sliding box sum.
//!
//! # The blend
//!
//! One member and one colour channel at a time: the member's channel is
//! reduced level by level (`sampling::resize_fields`, the one field resize,
//! both ways), each Laplacian level is weighted by the same level of the
//! member's 0/1 mask and added to the channel's accumulator pyramid, and the
//! accumulator is collapsed into the output. Where a frame is transparent its
//! sample is first mixed toward the hard composite (the winner's colour) by
//! `1 − alpha`, so the hidden RGB of a transparent pixel never enters.
//!
//! # Working set, in bytes per canvas pixel
//!
//! `winners`: the label plane (1), the running best score (4) and one frame's
//! fine-band score (4), 9 in all, plus the rings of four `BoxRows` and a
//! luma/alpha delay. `blend`: the output (16), the labels it borrows (1), the
//! channel's accumulator pyramid (4 × 4/3), the member's level and the next
//! one (4 + 1), the member's mask level (4), the expanded next level (4) and
//! `resize_fields`' own intermediate (2), 37.4 in all, plus one row.
//! [`WINNERS_BYTES_PER_PIXEL`] and [`BLEND_BYTES_PER_PIXEL`] are what the
//! plans charge instead: each is set by the admission drive's measurement of
//! the whole edit (see each constant).

use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::sampling::resize_fields;
use crate::sampling_box::BoxRows;

/// The label of a pixel no frame covers.
pub(crate) const NO_WINNER: u8 = u8::MAX;

/// A label is one byte and [`NO_WINNER`] takes its last value.
pub(crate) const MAX_FRAMES: usize = 255;

/// `(frame, canvas row y, out)`: one canvas row of straight RGBA, alpha
/// 0…1, zeros outside the frame's rect.
pub(crate) type ReadRow<'a> = &'a dyn Fn(usize, u32, &mut [[f32; 4]]);

/// What [`winners_plan`] charges per canvas pixel. The live planes are 9
/// bytes (module doc), and that is what the footprint reads while the frames
/// are scored. The constant is 35 because, when no blend follows, it and the
/// adapters' mask planes are the whole plan of the edit, and the admission
/// drive holds that plan to 1.25 times the peak of the whole call. That peak
/// is not inside [`winners`], whose planes are the same 9 bytes at every
/// depth: it reads 16 to 24 bytes a pixel over the masks, and in the 8-bit
/// case, sampled every 20 ms, it comes after [`winners`] has returned, while
/// the masks are set and the edited document's display is prepared. A plan
/// that blends takes the larger of the two charges, which is
/// [`BLEND_BYTES_PER_PIXEL`]'s. A miss in the drive raises this constant.
///
/// MEASUREMENT (the drive and the conditions of the record below, with
/// `seamless` off, so the call scores the frames and sets the masks; read
/// with this constant at 35, after all three cases missed at 12, at 0.723,
/// 0.706 and 0.836 ×):
///   8-bit, eight 6000 × 4000 layers: plan 1,038,600,000; E 673,268,192 /
///     673,120,736 / 673,497,568; 1.542 ×
///   16-bit, eight 6000 × 4000 layers: plan 1,230,600,000; E 961,495,736 /
///     961,413,816 / 961,495,736; 1.280 ×
///   float, four 4032 × 3024 layers: plan 526,788,864; E 294,568,104 /
///     294,715,560 / 294,486,184; 1.787 ×
/// E is 28.1, 40.1 and 24.2 bytes a pixel, of which the mask planes are 8,
/// 16 and 8. 34 is the least constant that holds 1.25 in the 16-bit case
/// (1.255 ×); 35 leaves it more than a run's spread. Since the adapters
/// charge each mask plane at 5/4 (`doc_auto_blend::mask_charge`) the three
/// plans above read 1,086,600,000, 1,326,600,000 and 551,174,400 by
/// arithmetic (1.613, 1.380 and 1.870 ×).
///
/// A mask plane carries its own quarter, and this constant is at least 1.25
/// times the rest of the peak, so the plan holds 1.25 × at any frame count.
/// Measured at 32 frames (the same drive, 2000 × 1500 jpeg8 layers, case
/// `masks-16-32x`): plan 346,240,000; E 265,830,640 / 265,535,728 /
/// 265,896,176; 1.302 ×, E 88.6 bytes a pixel of which the masks are 64.
///
/// Auto-Blend's Panorama with `seamless` off charges this constant too
/// (`doc_auto_blend::kernel_plan`). MEASUREMENT (2026-10-16,
/// the same drive's `panorama-masks-` cases: four 6000 × 4000 jpeg8 frames
/// stepped 4400 px along a 19200 × 4000 strip; baseline twins E ≤ 16,384):
///   8-bit: plan 3,093,120,000; E 1,940,686,192 / 1,940,768,160 /
///     1,940,669,832; 1.594 ×
///   16-bit: plan 3,477,120,000; E 1,806,386,328 / 2,459,059,952 /
///     1,806,320,816; 1.414 ×
/// E is 25.3 and 32.0 bytes a canvas pixel, of which the masks are 4 and 8:
/// the constant holds, unchanged.
const WINNERS_BYTES_PER_PIXEL: u64 = 35;

/// What [`blend_plan`] charges per canvas pixel. The live planes are 37.4
/// bytes (module doc); the constant is 48 so the plan holds a 1.25 margin
/// over the measured peak with the adapters' new mask planes beside it. A
/// miss in the drive below raises this constant.
///
/// MEASUREMENT (`scripts/auto-blend-admission/measure.py`, a Release
/// app, a fresh process per case at an 8 GiB working limit,
/// `MallocLargeCache=0`, three runs; E is the interval's peak phys_footprint
/// over its start around one `auto_blend_layers` call with `seamless` on,
/// the reusable caches evicted first, each layer blurred everywhere but its
/// own strip so every frame is a member of the blend; every baseline twin
/// read E ≤ 28,262,424):
///   8-bit, eight 6000 × 4000 layers: plan 1,440,096,000; E 1,087,980,264 /
///     1,087,718,120 / 853,148,176; 1.324 ×
///   16-bit, eight 6000 × 4000 layers: plan 1,728,096,000; E 1,279,443,832 /
///     1,279,689,592 / 1,279,689,616; 1.350 ×
///   float, four 4032 × 3024 layers: plan 877,943,808; E 552,681,808 /
///     552,730,960 / 552,665,424; 1.588 ×
/// E is the kernel's 37.3 bytes a pixel plus the mask planes (8, 16 and 8
/// bytes a pixel in the three cases) to within 0.1%: the peak is inside
/// [`blend`], and the publication after it stays under it. Since the adapters
/// charge each mask plane at 5/4 (`doc_auto_blend::mask_charge`) those plans
/// read 1,488,096,000, 1,824,096,000 and 902,329,344 by arithmetic (1.368,
/// 1.425 and 1.632 ×).
///
/// A mask plane carries its own quarter and this constant is above 1.25
/// times the kernel's 37.3, so the plan holds 1.25 × at any frame count, where
/// a plane charged only its own bytes let it fall under past 21 frames at 8
/// bits, 18 at 16 and 34 at float. Measured at 32 frames (the same drive,
/// 2000 × 1500 jpeg8 layers, the float stack the scene and 32 of them):
///   8-bit `blend-8-32x`: plan 276,032,000; E 208,945,344 / 208,994,496 /
///     196,755,672; 1.321 ×
///   16-bit `blend-16-32x`: plan 408,032,000; E 305,332,488 / 305,234,184 /
///     305,234,184; 1.336 ×
///   float `blend-float-32x`: plan 432,032,000; E 306,577,672 / 306,610,440 /
///     274,301,192; 1.409 ×
/// E is 69.7, 101.8 and 102.2 bytes a pixel: the kernel's 37.3 plus 32 or 64
/// of masks.
///
/// Auto-Blend's Panorama with `seamless` charges this constant over its
/// gains and seams, plus the masks and the written-back frames
/// (`doc_auto_blend::kernel_plan`). MEASUREMENT (2026-10-16,
/// the same drive's `panorama-` cases; baseline twins E ≤ 16,384):
///   8-bit, four 6000 × 4000 frames on a 19200 × 4000 strip: plan
///     4,454,707,200; E 3,175,319,808 / 3,175,254,224 / 3,175,172,352; 1.403 ×
///   16-bit, the same: plan 5,222,707,200; E 3,482,192,344 / 2,829,322,112 /
///     3,482,274,264; 1.500 ×
///   float, four 4032 × 3024 frames on a 12528 × 3024 strip: plan
///     2,977,848,576; E 1,718,322,376 / 1,718,600,904 / 1,718,486,216; 1.733 ×
/// The constant holds, unchanged.
const BLEND_BYTES_PER_PIXEL: u64 = 48;

/// The fine band's radius is one pixel per this many pixels of the shorter
/// canvas side, at least 1. Defocus removes the finest detail first, and the
/// finest detail a sensor resolves is a pixel or two across whatever the
/// frame size, so the radius grows slowly: 1 below a 4000-pixel side, 2 from
/// a 24 MP frame's 4000, 3 from 6000.
const FINE_SIDE_PER_PIXEL: u32 = 2000;

/// The fine radius stops growing here: past a 9-pixel window the "fine" band
/// is no longer the detail defocus removes first.
const MAX_FINE_RADIUS: u32 = 4;

/// The coarse band's radius over the fine one. Where a subject has no fine
/// texture (skin, a petal, a painted wall) the fine band holds only noise,
/// which is the same in every frame; the coarse band still separates a
/// focused edge from a defocused one there. Two octaves apart, so the two
/// bands measure different detail rather than the same detail twice.
const COARSE_RATIO: usize = 4;

/// The smoothing box's radius over its band's radius. A band's energy is
/// zero at every zero crossing of the detail it measures, so it is averaged
/// over a window a few periods wide before it is compared; twice the band
/// radius is two periods of the coarsest detail the band passes.
const SMOOTH_RATIO: usize = 2;

/// The pyramid stops halving when the shorter side would drop under this.
/// The coarsest level then still has at least 16 samples across, so it is a
/// picture of the frame's large-scale tone and not one average.
const MIN_LEVEL_SIDE: u32 = 32;

/// At most this many halvings. Seven is a 128-pixel sample: a tone
/// difference between frames is spread over a few hundred pixels at the
/// coarsest level, which is as wide as a mask boundary's neighbourhood
/// needs; deeper levels would trade whole-frame exposure between regions.
const MAX_HALVINGS: usize = 7;

struct Radii {
    fine: usize,
    coarse: usize,
    /// The vote's box is the coarse band's smoothing box: a region the score
    /// could not resolve is no larger than the window it was averaged over.
    vote: usize,
}

fn radii(size: (u32, u32)) -> Radii {
    let fine = (size.0.min(size.1) / FINE_SIDE_PER_PIXEL).clamp(1, MAX_FINE_RADIUS) as usize;
    let coarse = fine * COARSE_RATIO;
    Radii {
        fine,
        coarse,
        vote: coarse * SMOOTH_RATIO,
    }
}

fn checked(size: (u32, u32), frames: usize) -> Result<(usize, usize, u64), String> {
    if size.0 == 0 || size.1 == 0 {
        return Err("focus stacking needs a non-empty canvas".into());
    }
    if frames == 0 || frames > MAX_FRAMES {
        return Err(format!(
            "focus stacking takes between 1 and {MAX_FRAMES} frames"
        ));
    }
    let pixels = u64::from(size.0) * u64::from(size.1);
    if usize::try_from(pixels).is_err() {
        return Err("focus stacking canvas exceeds addressable memory".into());
    }
    Ok((size.0 as usize, size.1 as usize, pixels))
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, String> {
    let mut out = reserved_vec(count)?;
    out.resize(count, value);
    Ok(out)
}

/// The rings one band pass holds: four box sums, the luma/alpha delay and
/// its three row buffers.
fn band_ring_bytes(w: u64, h: u64, radius: u64) -> u64 {
    let smooth = radius * SMOOTH_RATIO as u64;
    BoxRows::bytes(w, h, radius)
        .saturating_mul(2)
        .saturating_add(BoxRows::bytes(w, h, smooth).saturating_mul(2))
        .saturating_add((radius + smooth + 2).min(h).saturating_mul(w) * 8)
        .saturating_add(w.saturating_mul(16 + 8 + 4))
}

/// `(scratch bytes, work units)` of [`winners`]: the per-pixel constant times
/// the canvas plus the larger of the coarse band's rings and the vote's. The
/// work is three passes a frame (two bands and the vote). Auto-Blend's
/// Panorama plan takes these bytes too, as the per-canvas-pixel charge of the
/// masks-only peak that its capped analysis grid cannot carry
/// (`doc_auto_blend::kernel_plan`).
pub(crate) fn winners_plan(size: (u32, u32), frames: usize) -> Result<(u64, u64), String> {
    let (_, _, pixels) = checked(size, frames)?;
    let (w, h) = (u64::from(size.0), u64::from(size.1));
    let radii = radii(size);
    let rings = band_ring_bytes(w, h, radii.coarse as u64)
        .max(BoxRows::bytes(w, h, radii.vote as u64).saturating_add(w.saturating_mul(16)));
    Ok((
        pixels
            .saturating_mul(WINNERS_BYTES_PER_PIXEL)
            .saturating_add(rings),
        pixels.saturating_mul(3 * frames as u64),
    ))
}

/// The frame each canvas pixel takes, row-major: a frame index, or
/// [`NO_WINNER`] where no frame has alpha above 0. `winners[p] == k` implies
/// frame `k` has alpha above 0 at `p`. Equal scores keep the earlier frame.
/// `luma` weights R, G and B into the scored luma (module doc).
/// `Err` for a refused size, a refused allocation or a cancel.
pub(crate) fn winners(
    size: (u32, u32),
    frames: usize,
    read: ReadRow<'_>,
    luma: [f32; 3],
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let (w, h, _) = checked(size, frames)?;
    let n = w * h;
    let radii = radii(size);
    let mut labels = filled(n, NO_WINNER)?;
    let mut best = filled(n, 0.0f32)?;
    let mut score = filled(n, 0.0f32)?;
    // Two of the three passes a frame are the score.
    let scoring = job.range(0.0, 2.0 / 3.0);
    for frame in 0..frames {
        let label = frame as u8;
        let part = |i: usize| {
            let total = (2 * frames) as f64;
            scoring.range(
                (2 * frame + i) as f64 / total,
                (2 * frame + i + 1) as f64 / total,
            )
        };
        band(
            (w, h),
            frame,
            radii.fine,
            read,
            luma,
            &mut |y, energy, _| score[y * w..(y + 1) * w].copy_from_slice(energy),
            part(0),
        )?;
        band(
            (w, h),
            frame,
            radii.coarse,
            read,
            luma,
            &mut |y, energy, samples| {
                let at = y * w;
                for x in 0..w {
                    // The fine band is inside the coarse one's pass band, so
                    // the sum counts the finest detail twice: the detail
                    // defocus removes first weighs the most.
                    let total = score[at + x] + energy[x];
                    if samples[x][1] > 0.0 && (labels[at + x] == NO_WINNER || total > best[at + x])
                    {
                        best[at + x] = total;
                        labels[at + x] = label;
                    }
                }
            },
            part(1),
        )?;
    }
    drop(score);
    vote(
        (w, h),
        frames,
        &labels,
        best,
        radii.vote,
        read,
        job.range(2.0 / 3.0, 1.0),
    )
}

/// One picture's sharpness score, row-major: the fine and the coarse band
/// summed exactly as [`winners`] ranks a frame by (module doc), 0 where the
/// picture is transparent. Select ▸ Focus Area's measure
/// (`focus_area`): one score, so the two features can never disagree on
/// what is sharp. `read` is frame 0's row reader. Also answers the coarse
/// band's radius and the vote's box radius at `size`.
pub(crate) fn sharpness(
    size: (u32, u32),
    read: ReadRow<'_>,
    luma: [f32; 3],
    job: JobContext<'_>,
) -> Result<(Vec<f32>, usize, usize), String> {
    let (w, h, _) = checked(size, 1)?;
    let radii = radii(size);
    let mut score = filled(w * h, 0.0f32)?;
    band(
        (w, h),
        0,
        radii.fine,
        read,
        luma,
        &mut |y, energy, _| score[y * w..(y + 1) * w].copy_from_slice(energy),
        job.range(0.0, 0.5),
    )?;
    band(
        (w, h),
        0,
        radii.coarse,
        read,
        luma,
        &mut |y, energy, _| {
            for (total, e) in score[y * w..(y + 1) * w].iter_mut().zip(energy) {
                *total += e;
            }
        },
        job.range(0.5, 1.0),
    )?;
    Ok((score, radii.coarse, radii.vote))
}

/// [`sharpness`]'s scratch bytes beyond its output plane: the coarse band's
/// rings.
pub(crate) fn sharpness_ring_bytes(size: (u32, u32)) -> u64 {
    band_ring_bytes(
        u64::from(size.0),
        u64::from(size.1),
        radii(size).coarse as u64,
    )
}

/// What [`band`] hands each finished row to: `(y, energy, samples)`.
type RowSink<'a> = &'a mut dyn FnMut(usize, &[f32], &[[f32; 2]]);

/// One band of one frame, streamed: `sink(y, energy, samples)` once per row
/// in order, `energy[x]` the smoothed band energy times the pixel's alpha and
/// `samples[x]` its `[luma, alpha]`.
///
/// Stage A is the alpha-weighted box mean of luma at `radius`; a row leaves
/// it up to `radius` rows after it was read. Stage B is the alpha-weighted
/// box mean of the squared difference at `radius × SMOOTH_RATIO`, up to that
/// many rows later again. The delay ring holds the `[luma, alpha]` rows both
/// stages still read.
fn band(
    (w, h): (usize, usize),
    frame: usize,
    radius: usize,
    read: ReadRow<'_>,
    luma: [f32; 3],
    sink: RowSink<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let smooth = radius * SMOOTH_RATIO;
    let rows = (radius + smooth + 2).min(h);
    let mut ring = filled(rows * w, [0.0f32; 2])?;
    let mut row = filled(w, [0.0f32; 4])?;
    let mut energy = filled(w, 0.0f64)?;
    let mut out = filled(w, 0.0f32)?;
    let mut mean_luma = BoxRows::new(w, h, radius)?;
    let mut mean_alpha = BoxRows::new(w, h, radius)?;
    let mut sum_energy = BoxRows::new(w, h, smooth)?;
    let mut sum_alpha = BoxRows::new(w, h, smooth)?;
    let (mut centred, mut written) = (0usize, 0usize);
    for y in 0..h {
        read(frame, y as u32, &mut row);
        let at = (y % rows) * w;
        for (slot, p) in ring[at..at + w].iter_mut().zip(&row) {
            *slot = [luma[0] * p[0] + luma[1] * p[1] + luma[2] * p[2], p[3]];
        }
        let base = y * w;
        let line = &ring[at..at + w];
        mean_luma.push(
            |i| f64::from(line[i - base][0]) * f64::from(line[i - base][1]),
            job,
        )?;
        mean_alpha.push(|i| f64::from(line[i - base][1]), job)?;
        while mean_luma.ready() {
            let at = (centred % rows) * w;
            let line = &ring[at..at + w];
            let (luma, alpha) = (mean_luma.emit(job)?, mean_alpha.emit(job)?);
            for x in 0..w {
                let [value, coverage] = line[x];
                // A covered pixel is inside its own window, so `alpha[x]` is
                // at least `coverage` there and the mean is defined.
                energy[x] = if coverage > 0.0 && alpha[x] > 0.0 {
                    let d = f64::from(value) - luma[x] / alpha[x];
                    f64::from(coverage) * d * d
                } else {
                    0.0
                };
            }
            let base = centred * w;
            sum_energy.push(|i| energy[i - base], job)?;
            sum_alpha.push(|i| f64::from(line[i - base][1]), job)?;
            centred += 1;
            while sum_energy.ready() {
                let at = (written % rows) * w;
                let line = &ring[at..at + w];
                let (total, alpha) = (sum_energy.emit(job)?, sum_alpha.emit(job)?);
                for x in 0..w {
                    out[x] = if line[x][1] > 0.0 && alpha[x] > 0.0 {
                        (total[x] / alpha[x]) as f32 * line[x][1]
                    } else {
                        0.0
                    };
                }
                sink(written, &out, line);
                written += 1;
            }
        }
        job.report((y + 1) as f64 / h as f64, 6)?;
    }
    Ok(())
}

/// One majority vote over the labels: a pixel takes the label with the most
/// pixels in its box among the frames that cover it, and keeps its own on a
/// tie. A [`NO_WINNER`] pixel stays one (no frame covers it) and a covered
/// pixel never becomes one (its own label always has a vote). `best` is the
/// score plane, reused for the running count.
fn vote(
    (w, h): (usize, usize),
    frames: usize,
    labels: &[u8],
    mut best: Vec<f32>,
    radius: usize,
    read: ReadRow<'_>,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let mut voted: Vec<u8> = reserved_vec(labels.len())?;
    voted.extend_from_slice(labels);
    best.fill(0.0);
    let mut present = [false; 256];
    for &label in labels {
        present[usize::from(label)] = true;
    }
    let mut row = filled(w, [0.0f32; 4])?;
    for (frame, &won) in present.iter().enumerate().take(frames) {
        job.report(frame as f64 / frames as f64, 6)?;
        if !won {
            continue;
        }
        let label = frame as u8;
        let mut count = BoxRows::new(w, h, radius)?;
        let mut emitted = 0usize;
        for _ in 0..h {
            count.push(|i| if labels[i] == label { 1.0 } else { 0.0 }, job)?;
            while count.ready() {
                // Sums of 0 and 1 are exact in binary64, so equal counts
                // compare equal.
                let votes = count.emit(job)?;
                read(frame, emitted as u32, &mut row);
                let at = emitted * w;
                for x in 0..w {
                    let i = at + x;
                    let votes = votes[x] as f32;
                    if row[x][3] > 0.0
                        && labels[i] != NO_WINNER
                        && (votes > best[i] || (votes == best[i] && labels[i] == label))
                    {
                        best[i] = votes;
                        voted[i] = label;
                    }
                }
                emitted += 1;
            }
        }
    }
    job.report(1.0, 6)?;
    Ok(voted)
}

/// The pyramid's level sizes, level 0 first.
fn level_sizes(size: (u32, u32)) -> Vec<(u32, u32)> {
    let mut sizes = vec![size];
    let mut at = size;
    while at.0.min(at.1) >= MIN_LEVEL_SIDE && sizes.len() <= MAX_HALVINGS {
        at = (at.0.div_ceil(2), at.1.div_ceil(2));
        sizes.push(at);
    }
    sizes
}

/// `(scratch bytes, work units)` of [`blend`]: the per-pixel constant times
/// the canvas plus one row. The work is three channels over every frame and
/// the uncovered member, whether or not this stack has one.
pub(crate) fn blend_plan(size: (u32, u32), frames: usize) -> Result<(u64, u64), String> {
    let (_, _, pixels) = checked(size, frames)?;
    Ok((
        pixels
            .saturating_mul(BLEND_BYTES_PER_PIXEL)
            .saturating_add(u64::from(size.0).saturating_mul(16)),
        pixels.saturating_mul(3 * (frames as u64 + 1)),
    ))
}

/// The frames blended through `winners` (what [`winners`] answered for the
/// same size, frames and reader): straight RGBA, row-major. Alpha is the
/// winner's alpha and a [`NO_WINNER`] pixel is `[0, 0, 0, 0]`. RGB may leave
/// the frames' own range by a little (a Laplacian sum overshoots at an
/// edge); the caller rounds it into its storage. `Err` for a label outside
/// the frames, a refused size or allocation, or a cancel.
pub(crate) fn blend(
    size: (u32, u32),
    frames: usize,
    winners: &[u8],
    read: ReadRow<'_>,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let (w, h, _) = checked(size, frames)?;
    let n = w * h;
    if winners.len() != n {
        return Err("focus stack labels differ from the canvas".into());
    }
    let mut present = [false; 256];
    for (i, &label) in winners.iter().enumerate() {
        job.every(i)?;
        if usize::from(label) >= frames && label != NO_WINNER {
            return Err("focus stack label is outside the frames".into());
        }
        present[usize::from(label)] = true;
    }
    let mut row = filled(w, [0.0f32; 4])?;
    // The hard composite: each pixel from its winner, zero where none.
    let mut out = filled(n, [0.0f32; 4])?;
    for y in 0..h {
        job.check()?;
        let labels = &winners[y * w..(y + 1) * w];
        let mut in_row = [false; 256];
        for &label in labels {
            in_row[usize::from(label)] = true;
        }
        for frame in (0..frames).filter(|&frame| in_row[frame]) {
            read(frame, y as u32, &mut row);
            for x in 0..w {
                if usize::from(labels[x]) == frame {
                    out[y * w + x] = row[x];
                }
            }
        }
    }
    // The members: every frame that won a pixel and, when any pixel has no
    // winner, the uncovered set as one more member. Its image is the hard
    // composite itself and its mask is the NO_WINNER set.
    //
    // Why a member and not a divide. The masks of the winning frames sum to 1
    // only where a pixel is covered. `resize_fields`' weights are normalised,
    // so the reduced masks of a PARTITION still sum to 1 at every level;
    // without this member they would sum to less than 1 beside an uncovered
    // region and the output there would fall off toward black. Dividing each
    // level by its mask sum leaves a level whose sum is 0 undefined, and the
    // expand step carries those samples back onto covered pixels. With the
    // member no level is divided by anything, and where one frame wins a
    // whole neighbourhood beside uncovered pixels that frame's image and the
    // member's image are the same picture there, so the output is that frame.
    let mut members: Vec<u8> = (0..frames)
        .filter(|&frame| present[frame])
        .map(|frame| frame as u8)
        .collect();
    if present[usize::from(NO_WINNER)] {
        members.push(NO_WINNER);
    }
    // One member's weights are 1 at every level, so the pyramid would only
    // rebuild the hard composite.
    if members.len() <= 1 {
        job.report(1.0, 6)?;
        return Ok(out);
    }
    let sizes = level_sizes(size);
    let steps = (3 * members.len() * sizes.len()) as f64;
    let mut done = 0.0f64;
    for channel in 0..3 {
        let mut pyramid: Vec<Vec<[f32; 1]>> = Vec::with_capacity(sizes.len());
        for &(lw, lh) in &sizes {
            pyramid.push(filled(lw as usize * lh as usize, [0.0f32])?);
        }
        for &member in &members {
            let mut image: Vec<[f32; 1]> = reserved_vec(n)?;
            for y in 0..h {
                job.check()?;
                let hard = &out[y * w..(y + 1) * w];
                if member == NO_WINNER {
                    image.extend(hard.iter().map(|p| [p[channel]]));
                } else {
                    read(usize::from(member), y as u32, &mut row);
                    // A transparent sample is the hard composite, so no
                    // hidden RGB of a transparent pixel enters the pyramid.
                    image.extend(
                        row.iter()
                            .zip(hard)
                            .map(|(p, hard)| [p[3] * p[channel] + (1.0 - p[3]) * hard[channel]]),
                    );
                }
            }
            let mut mask: Vec<[f32; 1]> = reserved_vec(n)?;
            mask.extend(
                winners
                    .iter()
                    .map(|&label| [if label == member { 1.0 } else { 0.0 }]),
            );
            for level in 0..sizes.len() {
                job.check()?;
                let target = &mut pyramid[level];
                if level + 1 == sizes.len() {
                    for (acc, (value, weight)) in target.iter_mut().zip(image.iter().zip(&mask)) {
                        acc[0] += weight[0] * value[0];
                    }
                } else {
                    let (here, next) = (sizes[level], sizes[level + 1]);
                    let reduced = resize_fields(&image, here, next, job)?;
                    let expanded = resize_fields(&reduced, next, here, job)?;
                    for (i, acc) in target.iter_mut().enumerate() {
                        acc[0] += mask[i][0] * (image[i][0] - expanded[i][0]);
                    }
                    drop(expanded);
                    mask = resize_fields(&mask, here, next, job)?;
                    image = reduced;
                }
                done += 1.0;
                job.report(done / steps, 6)?;
            }
        }
        let mut collapsed = pyramid.pop().unwrap_or_default();
        for level in (0..pyramid.len()).rev() {
            let mut expanded = resize_fields(&collapsed, sizes[level + 1], sizes[level], job)?;
            let band = pyramid.pop().unwrap_or_default();
            for (value, add) in expanded.iter_mut().zip(&band) {
                value[0] += add[0];
            }
            collapsed = expanded;
        }
        for (i, pixel) in out.iter_mut().enumerate() {
            job.every(i)?;
            pixel[channel] = if winners[i] == NO_WINNER {
                0.0
            } else {
                collapsed[i][0]
            };
        }
    }
    job.report(1.0, 6)?;
    Ok(out)
}
