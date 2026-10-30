//! Versioned, cancellable source-filter Gaussian sampling: the one
//! neighbourhood the destructive Gaussian-kernel filters (byte Unsharp Mask /
//! High Pass, export output sharpening, the 16-bit selected Photo filters)
//! share. It was written for the live Gaussian Blur, Unsharp Mask and High
//! Pass, removed on 2026-10-10; the `LIVE_` names and the "live clause"
//! below are kept from then. Sigma is binary64 source pixels with
//! clamped-edge extension.
//!
//! Below [`LIVE_FAST_SIGMA`] it is the normalized ceil(3*sigma) FIR
//! ([`gaussian_taps`]). Its vertical pass reads a ring of min(taps, h)
//! horizontal rows instead of a whole horizontal plane, each row computed
//! once in the same tap order, so the output is bit-identical. That pass is
//! [`blur_band`], which computes any `BAND_ROWS`-aligned row range of the
//! same plane bit for bit; [`blur_bounded`] is its whole-plane call.
//! From [`LIVE_FAST_SIGMA`] up it runs `sampling_gaussian_fast`, whose cost
//! does not grow with sigma. Its live clause was measured against an
//! UNTRUNCATED sampled Gaussian (binary64, ±ceil(8σ) taps, clamped,
//! premultiplied unit scale) on four 512² fixtures at both depths' values:
//! max ≤ 1/255 and mean ≤ 0.1/255 at every σ in
//! {4, 5, 6, 8, 12, 16, 25, 40, 64} (`designs/phase-9-live-filters-status.md`,
//! "Live filters at 24 MP, both depths"). So every output from σ4 up changes
//! slightly; below σ4 nothing does.
//!
//! That is 8 and 16 bits (`SampleRange::Unit`). A 32-bit float layer
//! (`Extended`) runs the FIR at every sigma, with a kernel cut where its
//! dropped tails are under 1e-7 of the weight: [`runs_fast`] holds the
//! measurement that decided it and [`half_width`] the cost.
//!
//! The legacy `sampling::blur` keeps its own family for existing destructive
//! Actions: the image crate's kernel-width/opaque-byte policy below σ4 and the
//! same `sampling_gaussian_fast` from σ4 up. This shared kernel is distinct
//! from mask feather's radius-to-sigma and multiresolution law.
//!
//! Three operators share the Gaussian neighborhood (`sampling_gaussian.rs`): `gaussian_blur`,
//! `unsharp_mask` (amount/threshold) and `high_pass`, keyed to binary64 sigma0…64 source pixels
//! with normalized ceil(3*sigma) support below `sampling_gaussian::LIVE_FAST_SIGMA` (4; the FIR's
//! vertical pass reads a ring of min(taps, h) horizontal rows, bit-identical) and
//! `sampling_gaussian_fast` from it up (held to max ≤ 1/255 and mean ≤ 0.1/255
//! against an untruncated Gaussian; the destructive twins, export sharpening and 16-bit selected
//! Photo follow the same switch), clamped edges and premultiplied encoded working-profile sampling.
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::selected_filter::BAND_ROWS;
use std::ops::Range;

/// A destructive pass runs once, so it declares a twelve-times larger budget
/// than the interactive `MAX_WORK_UNITS` instead of refusing an ordinary
/// photographic radius on a full-size layer; only genuinely extreme
/// combinations refuse. Neither figure is an operating-system memory limit.
/// The typed filtered canvas turn (`precision_geometry_budget`: Straighten and
/// a filtered Rotate Canvas, but not the Crop tool's `interactive` preview) is
/// a second one-shot caller: a 24 MP 16-bit photo turns by 17° at
/// Catmull-Rom in ≈ 2.78e9 units. A typed transform COMMIT — `affine`,
/// `perspective`, `affine_set`, `perspective_set` without `interactive` — is
/// a third: a 24 MP 16-bit layer turns by 17° at Catmull-Rom in
/// ≈ 2.78e9 units, while every ⌘T preview keeps `MAX_WORK_UNITS`. Those
/// commits and turns of a PLACED image (a retained source, re-rendered from
/// its master) take it too, as does the direct source `transform` of a 16-bit
/// ⌘T commit. It is the ceiling of an UNCONTROLLED one-shot call only: a
/// destructive filter, geometry, transform or placed-source commit running
/// under a host job control can be cancelled, so it runs with no work ceiling
/// and is admitted by memory alone ([`destructive_work_cap`]). That covers
/// every typed geometry edit that is not a preview and export sharpening too
/// (`ops_photographic::unsharp_mask_runs`), and who still takes this budget
/// is: a call that passes no job control (the app has none: every filter,
/// geometry and export call passes one), and the float layers paste's placed
/// re-render (`layer_clipboard_float`: `dr_precision_doc_paste_layers` takes
/// no control), which names it directly.
pub(crate) const MAX_DESTRUCTIVE_WORK: u64 = 24_000_000_000;

/// The work ceiling of a one-shot destructive call under a host job
/// control: none. Plans of those calls use it too, so a plan carries no work
/// refusal.
pub(crate) const CONTROLLED_JOB_WORK: u64 = u64::MAX;

/// A one-shot destructive pass's work ceiling: [`CONTROLLED_JOB_WORK`] under
/// a host job control (cancellable), [`MAX_DESTRUCTIVE_WORK`] without one.
pub(crate) fn destructive_work_cap(job: JobContext<'_>) -> u64 {
    if job.is_controlled() {
        CONTROLLED_JOB_WORK
    } else {
        MAX_DESTRUCTIVE_WORK
    }
}

/// This family's switch to `sampling_gaussian_fast`: equal to the legacy
/// family's on purpose, so one σ decides for every Gaussian in the app. The
/// live clause passed from here up against an untruncated Gaussian (module
/// doc); below it the FIR is kept bit-identical.
pub(crate) const LIVE_FAST_SIGMA: f64 = crate::sampling_gaussian_fast::LEGACY_FAST_SIGMA as f64;

/// (scratch bytes, work units) of [`blur_bounded`] in `range`, refused above
/// `cap`. σ0 is a copy. The FIR ([`runs_fast`] says when) holds its output
/// plane, a ring of min(taps, h) horizontal rows and the tap vector, with
/// [`half_width`]'s taps; the fast arm is `sampling_gaussian_fast::plan` over
/// the whole plane in one band. Source/native outputs and cache retention are
/// the caller's.
pub(crate) fn plan_bounded(
    width: u32,
    height: u32,
    sigma: f64,
    range: &SampleRange,
    cap: u64,
) -> Result<(u64, u64), String> {
    if !sigma.is_finite() || !(0.0..=64.0).contains(&sigma) {
        return Err("Gaussian sigma must be finite and in [0,64] source pixels".into());
    }
    let n = pixel_count(width, height)? as u64;
    if sigma == 0.0 {
        return Ok((n * 16 + BOOKKEEPING_BYTES, n));
    }
    let (bytes, work) = if runs_fast(sigma, range) {
        crate::sampling_gaussian_fast::plan((width, height), sigma, height)?
    } else {
        let taps = 2 * half_width(sigma, range) as u64 + 1;
        let ring = taps.min(u64::from(height)) * u64::from(width) * 16;
        (
            n * 16 + ring + taps * 8 + 2 * BOOKKEEPING_BYTES,
            n * (8 * taps + 12),
        )
    };
    if work > cap {
        return Err("Gaussian source filter exceeds the bounded pixel-work limit".into());
    }
    Ok((bytes, work))
}

/// Whether σ runs `sampling_gaussian_fast` in `range`: from
/// [`LIVE_FAST_SIGMA`] up at 8 and 16 bits (`Unit`), never in a 32-bit float
/// layer (`Extended`), which runs the FIR at every σ.
///
/// Why float does not take the recursive kernel: its error is a fraction of
/// the BRIGHTEST sample it reads, not of the local value. At 8 and 16 bits
/// that is at most 1/255 of white, the bound the live clause held it to. A
/// float layer has no white: beside a highlight of 5000 the same fraction is
/// larger than a 0.1 surround. It was measured before the choice
/// (2026-10-12, `tests/float_filters_highlight.rs`, the recursive kernel with
/// its final clamp widened to the range of the samples it read): on a 0.1
/// background, against the sampled Gaussian out to ±ceil(8σ), the bound being
/// 2 % of the reference at every pixel,
///
/// | σ | one 5000 pixel | a 5000 disc of radius 6 |
/// |---|---|---|
/// | 8 | 1.42 % | 34.7 % |
/// | 32 | 0.09 % | 7.7 % |
/// | 64 | 0.06 % | 0.15 % |
///
/// (σ 64 is the largest the filter takes). It missed at σ 8 and 32, so float
/// runs the FIR, whose worst error on the same fixture is the dropped tail:
/// the test holds it to the same 2 % and prints what it measures.
fn runs_fast(sigma: f64, range: &SampleRange) -> bool {
    sigma >= LIVE_FAST_SIGMA && matches!(range, SampleRange::Unit)
}

/// The share of the sampled Gaussian's weight a float layer's FIR may drop,
/// both tails together. The kernel is a mean, so what it drops from a pixel
/// is at most this share of the brightest sample beyond its reach: 5e-4 for
/// a highlight of 5000, which is 0.5 % of a 0.1 surround, inside the 2 %
/// [`runs_fast`] holds the float Gaussian to. `ceil(3σ)`, the other depths'
/// FIR, drops 2.7e-3 of the weight: invisible under a 0…1 clamp, and 135 times
/// that surround here.
pub(crate) const EXTENDED_TAIL: f64 = 1e-7;

/// The FIR's half-width in taps. `Unit`: `ceil(3σ)`, the kernel 8 and 16 bits
/// have always run below [`LIVE_FAST_SIGMA`]. `Extended`: the smallest
/// half-width whose dropped tails are under [`EXTENDED_TAIL`] of the sampled
/// Gaussian's total weight (the total is summed out to `ceil(8σ)`, where a
/// weight is below e^-32). That is about 5.33σ: 43 taps a side at σ 8 and 341
/// at σ 64, the largest σ the filters take.
///
/// What it costs: the work is `n·(8·taps + 12)` units ([`plan_bounded`]), so
/// it grows with σ where the recursive kernel's does not. At 6000×4000 that
/// is 1.7e10 units at σ 8 and 1.3e11 at σ 64. Timed there (2026-10-12, Apple
/// M2 Max, release, the whole float `blur` edit of
/// `tests/float_adjust_apply_24mp.rs::measure`, other lanes building at the
/// time): 2.8 s at σ 2, 10 s at σ 8 and 83 to 112 s at σ 64. A float filter
/// always runs under a job control, so it is cancellable, reports progress
/// row by row, and has no work ceiling ([`destructive_work_cap`]).
fn half_width(sigma: f64, range: &SampleRange) -> i64 {
    match range {
        SampleRange::Unit => (3.0 * sigma).ceil() as i64,
        SampleRange::Extended { .. } => {
            let weight = |k: i64| (-0.5 * (k as f64 / sigma).powi(2)).exp();
            let far = (8.0 * sigma).ceil() as i64;
            let total = 1.0 + 2.0 * (1..=far).map(weight).sum::<f64>();
            let mut dropped = 0.0;
            let mut half = far;
            while half > 0 {
                let next = dropped + 2.0 * weight(half);
                if next >= EXTENDED_TAIL * total {
                    break;
                }
                dropped = next;
                half -= 1;
            }
            half
        }
    }
}

/// The premultiplied Gaussian of the plane `read` yields (premultiplied f32,
/// unit-scale at 8 and 16 bits, row-major index) in `range`, refused as
/// [`plan_bounded`] refuses. It is [`blur_band`] over every row, reporting
/// into 0..0.9.
pub(crate) fn blur_bounded(
    width: u32,
    height: u32,
    sigma: f64,
    cap: u64,
    range: &SampleRange,
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    blur_band(
        width,
        height,
        sigma,
        cap,
        0..height,
        range,
        read,
        job.range(0.0, 0.9),
    )
}

/// Rows `rows` (`rows.start % BAND_ROWS == 0`, `rows.end <= h`) of exactly
/// [`blur_bounded`]'s plane, bit for bit. `read` takes the ABSOLUTE row-major
/// index and is only called for rows within the kernel's halo of `rows`, clamped
/// to [0, h). Progress is reported 0..1 over its own rows. The FIR keeps
/// a ring of min(taps, h) horizontal rows, filled from the window's
/// first row, so each output sums the same horizontal rows in the same tap
/// order as the whole plane; the fast arm is `sampling_gaussian_fast::band`,
/// whose aligned ranges equal the whole plane's rows by its contract.
/// `range` chooses between them ([`runs_fast`]) and sets the FIR's reach
/// ([`half_width`]). Refused as [`plan_bounded`] refuses.
#[allow(clippy::too_many_arguments)]
pub(crate) fn blur_band(
    width: u32,
    height: u32,
    sigma: f64,
    cap: u64,
    rows: Range<u32>,
    range: &SampleRange,
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    plan_bounded(width, height, sigma, range, cap)?;
    if !rows.start.is_multiple_of(BAND_ROWS) || rows.start >= rows.end || rows.end > height {
        return Err("Gaussian band rows lie outside the plane".into());
    }
    let (w, h) = (width as usize, height as usize);
    let (r0, r1) = (rows.start as usize, rows.end as usize);
    let len = r1 - r0;
    if sigma == 0.0 {
        let mut out = reserved_vec(len * w)?;
        for i in r0 * w..r1 * w {
            job.every(i)?;
            out.push(read(i));
        }
        return Ok(out);
    }
    if runs_fast(sigma, range) {
        return crate::sampling_gaussian_fast::band((width, height), sigma, rows, read, job);
    }
    let half = half_width(sigma, range);
    let weights = taps_of(sigma, half)?;
    // A ring of min(taps, h) horizontal rows: output row y reads rows
    // clamp(y − half ..= y + half), which all lie in the last `taps` rows
    // computed. Each row is computed once per call, in HEAD's tap order, so
    // the output is bit-identical to the former whole horizontal plane. A
    // band starts the ring at its first window row.
    let slots = weights.len().min(h);
    let mut ring: Vec<[f32; 4]> = reserved_vec(slots * w)?;
    ring.resize(slots * w, [0.0; 4]);
    let mut next = (r0 as i64 - half).max(0) as usize;
    let mut out = reserved_vec(len * w)?;
    for y in r0..r1 {
        let need = (y as i64 + half).min(h as i64 - 1) as usize;
        while next <= need {
            job.check()?;
            let slot = (next % slots) * w;
            for x in 0..w {
                job.every(x)?;
                let mut acc = [0.0_f64; 4];
                for (j, weight) in weights.iter().enumerate() {
                    let sx = (x as i64 + j as i64 - half).clamp(0, w as i64 - 1) as usize;
                    let pixel = read(next * w + sx);
                    for c in 0..4 {
                        acc[c] += weight * f64::from(pixel[c]);
                    }
                }
                ring[slot + x] = acc.map(|v| v as f32);
            }
            next += 1;
        }
        job.check()?;
        for x in 0..w {
            job.every(x)?;
            let mut acc = [0.0_f64; 4];
            for (j, weight) in weights.iter().enumerate() {
                let sy = (y as i64 + j as i64 - half).clamp(0, h as i64 - 1) as usize;
                let sample = ring[(sy % slots) * w + x];
                for c in 0..4 {
                    acc[c] += weight * f64::from(sample[c]);
                }
            }
            out.push(acc.map(|v| v as f32));
        }
        job.report((y + 1 - r0) as f64 / len as f64, 6)?;
    }
    Ok(out)
}

/// The normalized binary64 taps of this module's Gaussian: `2*ceil(3*sigma)+1`
/// weights centred on the middle one. The one copy of the tap arithmetic —
/// [`blur_bounded`] and Add Grain's scalar blur (`ops_spatial`) both read it,
/// so the two can never disagree about what a sigma means.
pub(crate) fn gaussian_taps(sigma: f64) -> Result<Vec<f64>, String> {
    gaussian_taps_in(sigma, &SampleRange::Unit)
}

/// [`gaussian_taps`] in `range`: `2*half+1` weights with [`half_width`]'s
/// reach, so a 32-bit float layer (`Extended`) gets the wider kernel whose
/// dropped tails are under [`EXTENDED_TAIL`]. The Blur and Sharpen brushes
/// (`retouch_brush`) read it at either depth.
pub(crate) fn gaussian_taps_in(sigma: f64, range: &SampleRange) -> Result<Vec<f64>, String> {
    taps_of(sigma, half_width(sigma, range))
}

/// The `2*half+1` normalized taps of a Gaussian of `sigma`: [`gaussian_taps`]
/// at its own `ceil(3*sigma)`, and a float layer's wider kernel
/// ([`half_width`]).
fn taps_of(sigma: f64, half: i64) -> Result<Vec<f64>, String> {
    if !sigma.is_finite() || sigma <= 0.0 {
        return Err("Gaussian sigma must be finite and positive".into());
    }
    let mut weights = reserved_vec((2 * half + 1) as usize)?;
    for k in -half..=half {
        weights.push((-0.5 * (k as f64 / sigma).powi(2)).exp());
    }
    let sum: f64 = weights.iter().sum();
    for weight in &mut weights {
        *weight /= sum;
    }
    Ok(weights)
}
