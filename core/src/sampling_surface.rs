//! Surface Blur: an edge-preserving mean whose neighbours are weighted by how
//! close their VALUE is to the centre's, run per colour channel over a square
//! `(2r+1)²` window with clamped edges:
//!
//! `out = Σ α·w(v−c)·v / Σ α·w(v−c)`, `w(d) = max(0, 1 − |d| / (2.5·T))`,
//!
//! with `d` and the threshold `T` in 8-bit LEVELS (a u16 code difference
//! divided by 257). The tent reaches zero at 2.5·T rather than at T: a
//! step of T levels keeps 60% weight, so the threshold reads as "how big an
//! edge is still smoothed a little", and a step of 2.5·T or more is left
//! exactly alone, which is what preserves an edge.
//!
//! The window is a Perreault/Hébert constant-time median layout: one 256-bin
//! histogram per COLUMN of Σα and Σα·(v & 255), slid down a row by one removal
//! and one addition, and a KERNEL histogram slid across each row by subtracting
//! the column leaving and adding the column entering. The per-pixel cost is
//! therefore 256 bin updates plus the bins inside ±2.5·T of the centre,
//! independent of the radius. The column bins hold only each code's offset
//! within its bin, so both fit an exact u32 (at most 201·65535·255 ≈ 3.4e9),
//! which halves the column bytes the bandwidth-bound slide reads; the u64
//! kernel bin rebuilds Σα·v = bin·256·Σα + Σα·(v & 255) exactly. Each bin is
//! weighted at its own MEAN Σαv/Σα: a bin is 256 u16 codes wide, so at 8 bits
//! (codes `v·257`) every bin holds one level and the result is the direct tent
//! filter exactly; at 16 bits the error is the tent's slope across less than
//! one level, which no quantized output can show.
//!
//! Every bin inside ±2.5·T is evaluated, empty ones (Σα = 0) too, where they
//! add an exact +0.0: skipping them made the cost depend on the content,
//! because a window whose bins are randomly half empty mispredicts the skip on
//! every other bin (r = 5, T = 255 on noise measured 13.8 s against 9.5 s at
//! r = 100), so no corner of the calibration bounded the interior. Branch-free,
//! the cost is linear in the two counts the charge names.
//!
//! Alpha is not filtered. A transparent neighbour has no weight, and the plane
//! carries the source alpha so `ops_spatial`'s combine can pass it through.
//!
//! Surface Blur (`sampling_surface`) is the exact per-channel tent `w(d) = max(0, 1
//! − |d|/(2.5·T))`, `T` in 8-bit levels, over Perreault column histograms weighted at each bin's
//! mean, branch-free over the bins in reach and charged by two calibrated coefficients,
//! `SURFACE_SLIDE_WORK_PER_1024` (644) and `SURFACE_EVALUATE_WORK_PER_1024` (1826), the measured
//! 560 / 1587 times an explicit 1.15 safety factor; 24 MP is admitted at T = 15 and refused at T
//! = 255. `surface_plane_in` is the one-band wrapper of `surface_band`: a band re-seeds its
//! column histograms at its first row, so any row range is bit-identical to the whole plane, and
//! `surface_band_plan` charges one band's scratch with every band's re-seeding work (`surface_plan`
//! is its `band_rows = h` case, unchanged).
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use std::ops::Range;

/// Work units per 1024 SLIDE bin operations (column and kernel histogram
/// updates), and per 1024 EVALUATED bin operations (the ±2.5·T tent sum, with
/// its per-bin divide). Two coefficients because the two costs differ: one
/// constant frozen at the evaluation-heavy corner overcharged the slide-bound
/// default threshold by about half. Frozen from the `#[ignore]`d probe in
/// `tests/spatial_filters.rs` (3000×2000, against the Gaussian σ16 reference's
/// own charge): a least-squares fit of time = a·slide + b·evaluate over the
/// four corners r ∈ {1,100} × T ∈ {2,255}, both coefficients then scaled by the
/// one factor that makes the charge at least the measured time at every corner
/// and at four interior checks. The kernel is branch-free over the bins in
/// reach, so time is linear in the two counts and a charge that covers the
/// corners covers everything between. Five runs implied 554/1587, 547/1537,
/// 560/1520, 547/1537 and 546/1524 (factors 1.006–1.023); the largest of each,
/// 560/1587, is then multiplied by an explicit safety factor of 1.15
/// (`SURFACE_SAFETY_FACTOR` in the probe), giving 644/1826. The margin is
/// needed because the measured pair sits only 2–6% above 24 MP wall-clock
/// times, the reference itself varies about 2% between runs, and the 16-bit
/// read path (which feeds every histogram shift) is not calibrated. With 644/1826
/// 24 MP is admitted at T = 15 at every radius (charge 2.22e10 of 2.4e10; about
/// 26 MP is the 3:2 limit) and refused at T = 255 (about 12.7 MP). The record
/// is in `designs/phase-9f-selected-tone-status.md`.
pub(crate) const SURFACE_SLIDE_WORK_PER_1024: u64 = 644;
pub(crate) const SURFACE_EVALUATE_WORK_PER_1024: u64 = 1826;

/// The bin count a ±2.5·T window can touch. At 16 bits a window that straddles
/// bin edges reaches one bin more than its width in levels, hence the `+2`.
fn evaluated_bins(threshold: f64) -> u64 {
    (2 * (2.5 * threshold).ceil() as u64 + 2).min(256)
}

/// `(scratch, work)` of the whole plane: one band of every row.
pub(crate) fn surface_plan(
    dimensions: (u32, u32),
    radius: u32,
    threshold: f64,
    cap: u64,
) -> Result<(u64, u64), String> {
    surface_band_plan(dimensions, radius, threshold, dimensions.1, cap)
}

/// `(scratch of ONE band of band_rows rows, work of ALL ceil(h/band_rows)
/// bands)`. Refuses before anything is allocated. Each band re-seeds its
/// column histograms from the `2r+1` rows around its first row, which is the
/// only work banding adds (`w·bands` in the slide term); with `band_rows = h`
/// this is the whole-plane charge exactly.
pub(crate) fn surface_band_plan(
    dimensions: (u32, u32),
    radius: u32,
    threshold: f64,
    band_rows: u32,
    cap: u64,
) -> Result<(u64, u64), String> {
    if !(1..=100).contains(&radius) || !threshold.is_finite() || !(2.0..=255.0).contains(&threshold)
    {
        return Err("surface blur needs radius 1-100 and threshold 2-255 levels".into());
    }
    let (w, h) = (u128::from(dimensions.0), u128::from(dimensions.1));
    let n = pixel_count(dimensions.0, dimensions.1)? as u128;
    if band_rows == 0 {
        return Err("surface blur needs at least one row per band".into());
    }
    let bands = h.div_ceil(u128::from(band_rows));
    let span = 2 * u128::from(radius) + 1;
    let slide = 3 * (n * 258 + span * (256 * h + w * bands));
    let evaluate = 3 * n * u128::from(evaluated_bins(threshold));
    let work = (slide * u128::from(SURFACE_SLIDE_WORK_PER_1024)
        + evaluate * u128::from(SURFACE_EVALUATE_WORK_PER_1024))
    .div_ceil(1024)
        + 4 * n;
    if work > u128::from(cap) {
        return Err("surface blur exceeds the bounded pixel-work limit".into());
    }
    // One band of the output plane, both column-histogram planes and the
    // kernel histogram.
    let scratch =
        w * u128::from(band_rows) * 16 + w * 256 * 8 + 256 * 16 + 2 * u128::from(BOOKKEEPING_BYTES);
    Ok((scratch as u64, work as u64))
}

/// One u16 code from a straight [0,1] component.
fn code(value: f32) -> u64 {
    code_of(f64::from(value))
}

/// One u16 code from a [0,1] value already in binary64.
fn code_of(value: f64) -> u64 {
    (value.clamp(0.0, 1.0) * 65535.0).round() as u64
}

/// The colour values the histogram's 65 536 codes span: `lo + span·k/65535`.
/// At 8 and 16 bits `lo` 0 and `span` 1, where every map below is the
/// expression it replaces bit for bit (`v − 0` and `v / 1` are `v`, `0 + 1·q`
/// is `q`).
#[derive(Clone, Copy)]
struct Headroom {
    lo: f64,
    span: f64,
}

impl Headroom {
    const UNIT: Self = Self { lo: 0.0, span: 1.0 };

    /// `range`'s headroom: `Unit` is 0…1; `Extended { visible }` is
    /// `[min(0, lowest visible), max(1, highest visible)]` over all three
    /// channels.
    fn of(range: &SampleRange) -> Self {
        match range {
            SampleRange::Unit => Self::UNIT,
            SampleRange::Extended { visible } => {
                let lo = visible
                    .min
                    .iter()
                    .fold(0.0_f64, |m, v| m.min(f64::from(*v)));
                let hi = visible
                    .max
                    .iter()
                    .fold(1.0_f64, |m, v| m.max(f64::from(*v)));
                Self { lo, span: hi - lo }
            }
        }
    }

    /// A colour component as its code in the histogram.
    fn code(&self, value: f32) -> u64 {
        code_of((f64::from(value) - self.lo) / self.span)
    }
}

/// The plane `[r, g, b, source alpha]` over a histogram spanning `range`.
/// A pixel whose window holds no weight — only possible when its own alpha
/// is zero — keeps its own colour. At `Unit` it is the 8- and 16-bit plane.
///
/// Surface Blur at 32 bits: the float route
/// runs it on the layer's DISPLAY-ENCODED copy (`display_encoding`), where a
/// threshold in 8-bit levels means what it means at 16 bits. The kernel is a
/// 65 536-code histogram, so the encoded copy is mapped affinely into it:
/// `lo = min(0, lowest visible code)`, `hi = max(1, highest visible code)`
/// over all channels, the tent's reach divided by `hi − lo` so it spans the
/// same encoded distance, and the output mapped back. The tent is linear in
/// the code, so the map is exact for it; only the bins widen, to `hi − lo`
/// levels each. This is the one place a float filter reads the layer's
/// headroom, because only this kernel quantizes to a fixed code range. An
/// SDR layer has `lo` 0 and `hi` 1: the 16-bit arithmetic.
pub(crate) fn surface_plane_in(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    radius: u32,
    threshold: f64,
    cap: u64,
    range: &SampleRange,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let h = dimensions.1;
    let headroom = Headroom::of(range);
    surface_rows(
        read,
        dimensions,
        radius,
        threshold,
        0..h,
        h,
        cap,
        headroom,
        job,
    )
}

/// Rows `rows` of `surface_plane_in`'s plane (at `Unit`), bit for bit: `rows.len()·w`
/// samples, row-major. The band seeds its column histograms at `rows.start`
/// from the clamped rows `rows.start − r ..= rows.start + r`, which hold the
/// same exact integer sums the whole-plane slide reaches there, so every
/// output is identical. Progress is band-local:
/// `(channel·len + (y − rows.start) + 1) / (3·len)`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn surface_band(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    radius: u32,
    threshold: f64,
    rows: Range<u32>,
    band_rows: u32,
    cap: u64,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    surface_rows(
        read,
        dimensions,
        radius,
        threshold,
        rows,
        band_rows,
        cap,
        Headroom::UNIT,
        job,
    )
}

/// [`surface_band`] over a histogram spanning `headroom`
/// ([`surface_plane_in`]).
#[allow(clippy::too_many_arguments)]
fn surface_rows(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    radius: u32,
    threshold: f64,
    rows: Range<u32>,
    band_rows: u32,
    cap: u64,
    headroom: Headroom,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    surface_band_plan(dimensions, radius, threshold, band_rows, cap)?;
    if rows.start >= rows.end || rows.end > dimensions.1 || rows.end - rows.start > band_rows {
        return Err("surface blur band rows lie outside the plane".into());
    }
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let (first, end) = (rows.start as usize, rows.end as usize);
    let len = end - first;
    let r = radius as i64;
    let (wi, hi) = (w as i64, h as i64);
    let mut out: Vec<[f32; 4]> = reserved_vec(len * w)?;
    for i in first * w..end * w {
        job.every(i)?;
        out.push(read(i));
    }
    let mut column_alpha: Vec<u32> = reserved_vec(w * 256)?;
    column_alpha.resize(w * 256, 0);
    let mut column_value: Vec<u32> = reserved_vec(w * 256)?;
    column_value.resize(w * 256, 0);
    // The tent's reach in u16 codes: 2.5·T levels of 257 codes each, over
    // a histogram spanning `headroom.span` (1 at 8 and 16 bits).
    let reach = 2.5 * threshold * 257.0 / headroom.span;
    for channel in 0..3 {
        column_alpha.fill(0);
        column_value.fill(0);
        let sample = |index: usize| {
            let p = read(index);
            (headroom.code(p[channel]), code(p[3]))
        };
        // One row of one column into (or out of) that column's histogram.
        let shift = |alpha_sums: &mut [u32], value_sums: &mut [u32], x: usize, row: i64, add| {
            let (value, alpha) = sample(row.clamp(0, hi - 1) as usize * w + x);
            let bin = x * 256 + (value >> 8) as usize;
            accumulate(alpha_sums, value_sums, bin, alpha, value, add);
        };
        for x in 0..w {
            job.every(x)?;
            for dy in -r..=r {
                shift(
                    &mut column_alpha,
                    &mut column_value,
                    x,
                    first as i64 + dy,
                    true,
                );
            }
        }
        for y in first..end {
            job.check()?;
            if y > first {
                for x in 0..w {
                    let (leave, enter) = (y as i64 - 1 - r, y as i64 + r);
                    shift(&mut column_alpha, &mut column_value, x, leave, false);
                    shift(&mut column_alpha, &mut column_value, x, enter, true);
                }
            }
            let mut kernel_alpha = [0_u64; 256];
            let mut kernel_value = [0_u64; 256];
            for dx in -r..=r {
                slide(&mut kernel_alpha, column(&column_alpha, dx, wi), None);
                slide(&mut kernel_value, column(&column_value, dx, wi), None);
            }
            for x in 0..w {
                if x > 0 {
                    let (enter, leave) = (x as i64 + r, x as i64 - 1 - r);
                    let alpha = (
                        column(&column_alpha, enter, wi),
                        column(&column_alpha, leave, wi),
                    );
                    slide(&mut kernel_alpha, alpha.0, Some(alpha.1));
                    let value = (
                        column(&column_value, enter, wi),
                        column(&column_value, leave, wi),
                    );
                    slide(&mut kernel_value, value.0, Some(value.1));
                }
                let index = y * w + x;
                let centre = sample(index).0 as f64;
                let low = ((centre - reach).max(0.0) as usize) >> 8;
                let high = ((centre + reach).min(65535.0) as usize) >> 8;
                let (mut numerator, mut denominator) = (0.0_f64, 0.0_f64);
                for b in low..=high {
                    let alpha = kernel_alpha[b];
                    // Exact: at most 65280·201²·65535 + 201²·65535·255 < 2^53.
                    let value = (b as u64 * 256) * alpha + kernel_value[b];
                    let (alpha, value) = (alpha as f64, value as f64);
                    // The bin's own mean stands for every code in it; see the
                    // module doc for why that is exact at 8 bits. An empty bin
                    // (alpha and value 0) and a bin past the tent's reach
                    // (weight clamped to 0) both add exactly +0.0, with no
                    // branch for the predictor to miss.
                    let mean = value / alpha.max(1.0);
                    let weight = (1.0 - (mean - centre).abs() / reach).max(0.0);
                    numerator += weight * value;
                    denominator += weight * alpha;
                }
                if denominator > 0.0 {
                    let unit = numerator / denominator / 65535.0;
                    out[index - first * w][channel] = (headroom.lo + headroom.span * unit) as f32;
                }
            }
            job.report(
                (channel * len + (y - first) + 1) as f64 / (3 * len) as f64,
                6,
            )?;
        }
    }
    Ok(out)
}

/// One sample into (or out of) one column bin: Σα and Σα·(v & 255), the code's
/// offset within its bin. Wrapping, because the running sums are exact u32
/// integers whose true value never leaves range (a column holds at most 201
/// samples) even when a removal is applied before the matching addition.
fn accumulate(
    alpha_sums: &mut [u32],
    value_sums: &mut [u32],
    bin: usize,
    alpha: u64,
    value: u64,
    add: bool,
) {
    // alpha ≤ 65535 and the offset ≤ 255, so both terms fit u32.
    let (alpha, weighted) = (alpha as u32, (alpha * (value & 255)) as u32);
    if add {
        alpha_sums[bin] = alpha_sums[bin].wrapping_add(alpha);
        value_sums[bin] = value_sums[bin].wrapping_add(weighted);
    } else {
        alpha_sums[bin] = alpha_sums[bin].wrapping_sub(alpha);
        value_sums[bin] = value_sums[bin].wrapping_sub(weighted);
    }
}

/// Column `x`'s 256 bins, the column index clamped to the plane (clamped edges).
fn column(sums: &[u32], x: i64, wi: i64) -> &[u32] {
    let start = x.clamp(0, wi - 1) as usize * 256;
    &sums[start..start + 256]
}

/// The kernel histogram plus one entering column, minus one leaving column.
/// Written over fixed 256-element slices so the compiler can vectorize it: it
/// is the per-pixel term every Surface Blur pays whatever its threshold.
/// The column bins are widened to u64 here; a kernel bin holds up to 201²
/// columns' sums, which no longer fits u32.
fn slide(kernel: &mut [u64; 256], enter: &[u32], leave: Option<&[u32]>) {
    let enter: &[u32; 256] = enter.try_into().unwrap_or(&[0; 256]);
    match leave {
        None => {
            for (k, e) in kernel.iter_mut().zip(enter) {
                *k = k.wrapping_add(u64::from(*e));
            }
        }
        Some(leave) => {
            let leave: &[u32; 256] = leave.try_into().unwrap_or(&[0; 256]);
            for ((k, e), l) in kernel.iter_mut().zip(enter).zip(leave) {
                *k = k.wrapping_add(u64::from(*e)).wrapping_sub(u64::from(*l));
            }
        }
    }
}
