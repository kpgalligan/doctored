//! Experimental source-only joint Bayesian alpha/foreground/background model.
//!
//! Chuang et al. (CVPR 2001) supplies the Gaussian compositing objective. This
//! bounded adaptation uses a finite automatic alpha prior and source-derived
//! color supports, not a supplied ground-truth trimap. Automatic supports are
//! uncertain distribution observations; only explicit user labels pin alpha.
//! Every automatic endpoint is solved. No source/reference names enter this API.
//!
//! Uncertain eroded source-color supports feed shared bounded spatial/color sampling and one
//! weighted Gaussian per class; no reference or fixture name enters inference. Conditional Gaussian
//! color solves share the existing3x3 SPD factor; bounded scalar alpha search is not a
//! convergence/opacity claim. Whole-call objective-evaluation and measured spatial-node caps are
//! separate. Exact known labels, source endpoints, input purity and cancellation are independently
//! tested.
use crate::{
    bayesian_color::{self, Gaussian},
    doc::MAX_PIXELS,
    doc_select::signed_distance_from_membership,
    job_control::{DrJobControl, JobContext},
    matting_samples::{Nearby, Sample, SampleIndex, CANDIDATES},
    sampled_matting::MAX_BOUNDARY_SAMPLES,
};
use image::GrayImage;

pub const SUPPORT_DISTANCE: f32 = 8.0;
/// Objective evaluations only; spatial lookup has its own measured cap.
pub const MAX_JOINT_EVALUATIONS: u64 = 200_000_000;
pub const MAX_SPATIAL_VISITS: u64 = 2_000_000_000;
const SCRATCH_OVERHEAD: u64 = 16 * 1024 * 1024;

pub struct BayesianMatte {
    pub alpha: GrayImage,
    pub foreground: Vec<[f32; 3]>,
    pub foreground_samples: usize,
    pub background_samples: usize,
    pub automatic_foreground_support: usize,
    pub automatic_background_support: usize,
    pub joint_evaluations: u64,
    pub spatial_node_visits: u64,
    pub alpha_changed_pixels: usize,
    pub high_prior_alpha_changed_pixels: usize,
    pub foreground_changed_pixels: usize,
}

/// All RGB is finite normalized encoded RGB from an opaque observed composite.
/// Unknown user labels are128; only0/255 are hard constraints. No automatic
/// seed is a hard constraint or a claim of physical opacity. Missing either
/// color population, memory/work excess and cancellation refuse atomically.
/// `working_limit` includes borrowed colors/prior/labels plus returned buffers.
pub fn estimate(
    colors: &[[f32; 3]],
    size: (u32, u32),
    prior: &[u8],
    user: &[u8],
    working_limit: u64,
    control: Option<&DrJobControl>,
) -> Result<BayesianMatte, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let (w, h) = (size.0 as usize, size.1 as usize);
    let n = u64::from(size.0) * u64::from(size.1);
    if n == 0
        || n > MAX_PIXELS
        || colors.len() != n as usize
        || prior.len() != colors.len()
        || user.len() != colors.len()
    {
        return Err("bayesian matting dimensions or buffers are invalid".into());
    }
    let mut unknown = 0_u64;
    for (i, (color, &label)) in colors.iter().zip(user).enumerate() {
        job.every(i)?;
        if color
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || ![0, 128, 255].contains(&label)
        {
            return Err(
                "bayesian matting requires normalized RGB and categorical user labels".into(),
            );
        }
        unknown += u64::from(label == 128);
    }
    let forecast = unknown
        .checked_mul(bayesian_color::MAX_EVALUATIONS as u64)
        .ok_or("bayesian matting work overflow")?;
    if forecast > MAX_JOINT_EVALUATIONS {
        return Err(format!(
            "bayesian matting joint work {forecast} exceeds {MAX_JOINT_EVALUATIONS}; narrow the unknown band or refine a smaller area"
        ));
    }
    // 14N borrowed, 13N output, sequential 8N distance scratch, support bytes,
    // and a bounded spatial/color sample index. Factor2 covers vector capacity
    // and the existing distance transform's row/column scratch even for1xN.
    let scratch = 64 * n + SCRATCH_OVERHEAD;
    if working_limit == 0 || scratch + 14 * n > working_limit {
        return Err("bayesian matting exceeds its complete working-memory allowance".into());
    }
    job.reserve(scratch)?;
    if unknown == 0 {
        let mut alpha = Vec::with_capacity(n as usize);
        let mut foreground = Vec::with_capacity(n as usize);
        let (mut changed, mut high_changed) = (0, 0);
        for (i, (&label, &color)) in user.iter().zip(colors).enumerate() {
            job.every(i)?;
            alpha.push(label);
            foreground.push(color);
            changed += usize::from(label != prior[i]);
            high_changed += usize::from(label != prior[i] && prior[i] >= 247);
        }
        job.report(1.0, 5)?;
        return Ok(BayesianMatte {
            alpha: GrayImage::from_raw(size.0, size.1, alpha).ok_or("invalid hard matte")?,
            foreground,
            foreground_samples: 0,
            background_samples: 0,
            automatic_foreground_support: 0,
            automatic_background_support: 0,
            joint_evaluations: 0,
            spatial_node_visits: 0,
            alpha_changed_pixels: changed,
            high_prior_alpha_changed_pixels: high_changed,
            foreground_changed_pixels: 0,
        });
    }
    let mut support = vec![128_u8; n as usize];
    let mut populations = [0; 2];
    for (class, label) in [0, 255].into_iter().enumerate() {
        let distance = signed_distance_from_membership(
            w,
            h,
            |i| {
                user[i] == label
                    || (user[i] == 128
                        && if label == 0 {
                            prior[i] <= 8
                        } else {
                            prior[i] >= 247
                        })
            },
            job.range(0.0, 0.1),
        )?;
        for (i, &d) in distance.iter().enumerate() {
            job.every(i)?;
            if user[i] == label || (user[i] == 128 && d >= SUPPORT_DISTANCE) {
                support[i] = label;
                populations[class] += usize::from(user[i] == 128);
            }
        }
    }
    let mut counts = [0; 2];
    for (i, &label) in support.iter().enumerate() {
        job.every(i)?;
        if label != 128 && boundary(&support, w, h, i) {
            counts[usize::from(label == 255)] += 1;
        }
    }
    if counts.contains(&0) {
        return Err(format!("bayesian matting lacks source-derived color support: foreground {}, background {}; automatic support is not observed opaque truth", counts[1], counts[0]));
    }
    if counts.iter().sum::<usize>() > MAX_BOUNDARY_SAMPLES {
        return Err(
            "bayesian matting exceeds the unchanged boundary-sample cap; narrow the unknown band or refine a smaller area"
                .into(),
        );
    }
    let mut points = [Vec::with_capacity(counts[0]), Vec::with_capacity(counts[1])];
    for (i, &label) in support.iter().enumerate() {
        job.every(i)?;
        if label != 128 && boundary(&support, w, h, i) {
            points[usize::from(label == 255)].push(Sample {
                x: (i % w) as u32,
                y: (i / w) as u32,
                rgb: colors[i],
            });
        }
    }
    let [background_points, foreground_points] = points;
    let foreground = SampleIndex::new(foreground_points, job)?;
    let background = SampleIndex::new(background_points, job)?;
    let mut alpha = Vec::with_capacity(n as usize);
    let mut output = Vec::with_capacity(n as usize);
    let mut near_f = [Nearby::default(); CANDIDATES];
    let mut near_b = [Nearby::default(); CANDIDATES];
    let mut evaluations = 0_u64;
    let mut remaining_visits = MAX_SPATIAL_VISITS;
    let (mut changed, mut high_changed, mut color_changed) = (0, 0, 0);
    for i in 0..n as usize {
        if i & 63 == 0 {
            job.check()?;
        }
        let (a, f) = if user[i] != 128 {
            (user[i], colors[i])
        } else {
            let (x, y) = ((i % w) as u32, (i / w) as u32);
            let nf = foreground.nearby_bounded(x, y, &mut near_f, job, &mut remaining_visits)?;
            let nb = background.nearby_bounded(x, y, &mut near_b, job, &mut remaining_visits)?;
            let f_model = distribution(&near_f[..nf]);
            let b_model = distribution(&near_b[..nb]);
            let result = bayesian_color::estimate(
                colors[i].map(f64::from),
                f64::from(prior[i]) / 255.0,
                &f_model,
                &b_model,
                None,
            )?;
            evaluations += result.evaluations as u64;
            let a = (result.alpha * 255.0).round() as u8;
            // Endpoint source RGB is exact. This is a supplied-alpha contract,
            // not detection of reference opacity; original gates test it anyway.
            let f = if a == 0 || a == 255 {
                colors[i]
            } else {
                result.foreground.map(|v| v as f32)
            };
            (a, f)
        };
        changed += usize::from(a != prior[i]);
        high_changed += usize::from(a != prior[i] && prior[i] >= 247);
        color_changed +=
            usize::from((0..3).any(|c| (f[c] * 255.0).round() != (colors[i][c] * 255.0).round()));
        alpha.push(a);
        output.push(f);
        if i % w == w - 1 {
            job.report(0.1 + 0.9 * (i + 1) as f64 / n as f64, 6)?;
        }
    }
    job.report(1.0, 5)?;
    Ok(BayesianMatte {
        alpha: GrayImage::from_raw(size.0, size.1, alpha).ok_or("invalid bayesian matte")?,
        foreground: output,
        foreground_samples: counts[1],
        background_samples: counts[0],
        automatic_foreground_support: populations[1],
        automatic_background_support: populations[0],
        joint_evaluations: evaluations,
        spatial_node_visits: MAX_SPATIAL_VISITS - remaining_visits,
        alpha_changed_pixels: changed,
        high_prior_alpha_changed_pixels: high_changed,
        foreground_changed_pixels: color_changed,
    })
}

fn boundary(support: &[u8], w: usize, h: usize, i: usize) -> bool {
    let (x, y) = (i % w, i / w);
    x == 0
        || y == 0
        || x + 1 == w
        || y + 1 == h
        || support[i - 1] != support[i]
        || support[i + 1] != support[i]
        || support[i - w] != support[i]
        || support[i + w] != support[i]
}

fn distribution(samples: &[Nearby]) -> Gaussian {
    let nearest = f64::from(samples[0].distance).powi(2);
    let scale = f64::from(SUPPORT_DISTANCE).powi(2);
    // Bounded inverse-quadratic spatial weights retain distant color support;
    // one Gaussian per class is an explicit approximation to Bayesian mixtures.
    let weight = |s: &Nearby| (nearest + scale) / (f64::from(s.distance).powi(2) + scale);
    let sum: f64 = samples.iter().map(weight).sum();
    let mean = std::array::from_fn(|c| {
        samples
            .iter()
            .map(|s| weight(s) * f64::from(s.sample.rgb[c]))
            .sum::<f64>()
            / sum
    });
    let mut covariance = [[0.0; 3]; 3];
    for c in 0..3 {
        for d in 0..=c {
            let value = samples
                .iter()
                .map(|s| {
                    weight(s)
                        * (f64::from(s.sample.rgb[c]) - mean[c])
                        * (f64::from(s.sample.rgb[d]) - mean[d])
                })
                .sum::<f64>()
                / sum
                + if c == d {
                    bayesian_color::OBSERVATION_VARIANCE
                } else {
                    0.0
                };
            // One rounding path preserves exact symmetry for the shared SPD
            // factor, including correlated observed color populations.
            covariance[c][d] = value;
            covariance[d][c] = value;
        }
    }
    Gaussian { mean, covariance }
}
