//! Motion Blur: the equal-weight premultiplied mean of `distance + 1`
//! bilinear samples along a straight line through each pixel, a `SpatialKernel`
//! arm on the one spatial route (`SelectedFilter::Spatial` at 16 bits,
//! `filter_op` at 8; no new export).
//!
//! With `p` the pixel centre `(x + ½, y + ½)` and `θ` the angle, sample `k` of
//! `0..=distance` stands at `p + (k − distance/2)·(cos θ, −sin θ)`: one sample
//! per pixel of path, symmetric about the pixel so the blur does not drift, and
//! `−sin θ` because a positive angle is counterclockwise on a y-down canvas.
//! Reads clamp at the plane's edges through `sampling_radial::sample` (the
//! master copy of the bilinear read). Alpha is filtered, as in Box Blur, whose
//! combine and transparent floor this shares. It takes no frame: a selection
//! confines the result by coverage, not the samples.
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling_radial::{sample, sample_exact, RADIAL_BILINEAR_WORK_PER_SAMPLE};

/// Motion Blur's per-pixel plan term: the kernel holds the n×16 output plane
/// and nothing else that grows with the picture (the sample offsets are
/// `distance + 1` pairs, inside the bookkeeping term).
///
/// Record (the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, a fresh
/// instance per case, `MallocLargeCache=0`, E =
/// `ri_interval_max_phys_footprint` − pre around the one `apply_filter` call,
/// 3 runs + a baseline twin at E 0; a 6000×4000 layer of two-colour Clouds,
/// distance 200):
/// - 16-bit: E 586,203,568 / 585,515,416 / 585,466,264 against
///   `selected_edit_plan` 822,291,456 (1.403×). Its 24n combine stage is above
///   this kernel's 16n, so the term does not bind there; an `apply_filter`
///   invert on the same document peaks at 577,061,272.
/// - 8-bit: E 880,149,104 / 879,444,568 / 873,661,040, beside invert's
///   864,780,888: the peak is the commit's, which the host charges
///   (`ByteFilterAdmission.commitStageBytes` 1,020,000,000, and the
///   96,003,794 the edit leaves retained: 1.268×, `ProjectionMargin`'s rule).
///   `filter_op_scratch_bytes` is 605,242,880, its 20n combine stage.
const MOTION_BLUR_BYTES_PER_PIXEL: u64 = 16;

/// `(scratch, work)`, refused before any allocation: `distance` outside
/// `1..=2000`, or `n·(distance + 1)` bilinear samples above `cap`.
pub(crate) fn motion_plan(
    dimensions: (u32, u32),
    distance: u32,
    cap: u64,
) -> Result<(u64, u64), String> {
    let n = pixel_count(dimensions.0, dimensions.1)? as u64;
    if !(1..=2000).contains(&distance) {
        return Err("motion blur distance must be an integer in [1,2000]".into());
    }
    let work = n
        .saturating_mul(u64::from(distance) + 1)
        .saturating_mul(RADIAL_BILINEAR_WORK_PER_SAMPLE);
    if work > cap {
        return Err("motion blur exceeds the bounded pixel-work limit".into());
    }
    Ok((
        n * MOTION_BLUR_BYTES_PER_PIXEL + 2 * BOOKKEEPING_BYTES,
        work,
    ))
}

/// The plane: the premultiplied mean along the line (below
/// `range.alpha_floor()` of alpha it is transparent black, `box_plane`'s
/// floor: half a 16-bit code at 8 and 16 bits, 2^-24 in a float layer).
/// `ops_spatial`'s Box Blur combine turns it back to straight samples. A
/// float layer (`Extended`) places and weighs each sample in binary64
/// (`sampling_radial::sample_exact`, which says why); 8 and 16 bits keep
/// `sample`.
pub(crate) fn motion_plane(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    angle: f64,
    distance: u32,
    range: &SampleRange,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let alpha_floor = range.alpha_floor();
    let exact = matches!(range, SampleRange::Extended { .. });
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let (sin, cos) = angle.to_radians().sin_cos();
    let count = distance as usize + 1;
    let half = f64::from(distance) / 2.0;
    let mut steps: Vec<(f64, f64)> = reserved_vec(count)?;
    for k in 0..count {
        let t = k as f64 - half;
        steps.push((t * cos, -t * sin));
    }
    let mut out = reserved_vec(w * h)?;
    for y in 0..h {
        job.check()?;
        for x in 0..w {
            job.every(y * w + x)?;
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let mut acc = [0.0_f64; 4];
            for &(dx, dy) in &steps {
                let p = if exact {
                    sample_exact(&read, (w, h), (px + dx, py + dy), true)
                } else {
                    sample(&read, (w, h), ((px + dx) as f32, (py + dy) as f32), true).map(f64::from)
                };
                for c in 0..4 {
                    acc[c] += p[c];
                }
            }
            let mean = acc.map(|v| v / count as f64);
            out.push(if mean[3] < alpha_floor {
                [0.0; 4]
            } else {
                mean.map(|v| v as f32)
            });
        }
        job.report((y + 1) as f64 / h as f64, 6)?;
    }
    Ok(out)
}
