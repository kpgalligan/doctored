//! Auto-Align's feature registration: the homography taking
//! one grey frame's points to another's, from `feature_points`' corners.
//! Plain numbers only; the FFI is `ffi_feature_register`. The host runs it
//! beside Vision's registration, cross-checks either answer on its inliers
//! and vouches for its own with the same agreement score as Vision's
//! (`AutoAlign.register`).
//!
//! THE LENS TEST (rulings 3 and 4). Two models fitted to the inliers: H, the plain
//! homography (k = 0), and Geometric Distortion's model, one radial
//! coefficient k shared by both frames' lenses and the homography P_k
//! between the corrected points (`align_distortion_pair::pair_coefficient`, its
//! one implementation). The lens model predicts a floating point p at
//! D_k(P_k(D_k⁻¹(p))), D_k the frame's radial map about its centre. The
//! DEPARTURE is the WORST inlier's distance between the two predictions, in
//! frame pixels (ruling 4: the quantity ruling 2's evidence measured; an RMS
//! averages a lens's growth toward the edge with the flat centre): what the lens term changes where the
//! matches are. It is measured at the matches, not at the corner, because k
//! alone is not pinned when the frames barely move (with no motion every k
//! predicts the same), while the departure then is near zero. The host
//! decides whether it is significant.
//!
//! THE MATCHES. Each floating feature's two nearest reference descriptors by
//! squared L2 distance; a pair is kept when it is MUTUAL (the reference
//! feature's nearest floating descriptor is this one) and passes the ratio
//! test, nearest < [`RATIO`] × second nearest (as distances).
//!
//! THE FIT. RANSAC over the matches: samples of four drawn from
//! `rng::SplitMix64` at a fixed seed (so a registration is reproducible),
//! a sample with three nearly collinear points on either side skipped, each
//! fitted by `align_distortion_match::homography` (the crate's one DLT); a
//! match is an inlier within [`INLIER_DISTANCE`]. At most [`MAX_ITERATIONS`],
//! stopping early once the best inlier share makes an all-inlier sample
//! [`CONFIDENCE`] likely. The best sample's inliers are refitted, gathered
//! again under the refit, and refitted once more. Fewer than
//! [`MIN_INLIERS`], or a map that is not finite, is no estimate.
use crate::align_distortion::lens_frame;
use crate::align_distortion_match::homography;
use crate::align_distortion_pair::{pair_coefficient, pair_coefficient_bytes};
use crate::align_vignette::apply;
use crate::feature_points::{detect, detect_scratch_bytes, Feature, MAX_FEATURES};
use crate::lens_model::{radial_map, radial_unmap};
use crate::puppet_mesh::Rect;
use crate::rng::SplitMix64;

/// The smallest frame side registered: a frame this small holds no
/// descriptor window, so anything smaller is a caller's mistake.
pub(crate) const MIN_SIDE: usize = 16;

/// The largest frame side registered. Auto-Align's analysis frames are at
/// most 2048 px; twice that bounds the kernel's planes (6 × 64 MB at 4096²)
/// whatever a caller passes.
pub(crate) const MAX_SIDE: usize = 4096;

/// Lowe's ratio test at 0.8: a match whose second-best candidate is nearly as
/// close is ambiguous (repeated texture, a cloud field) and would mostly add
/// outliers to RANSAC.
const RATIO: f32 = 0.8;

/// The inlier distance, frame pixels. A corner's sub-pixel position is good
/// to about half a pixel at level 0 and a pixel or two on coarser levels;
/// 3 px keeps those while rejecting a wrong match, which lands tens of
/// pixels away.
pub(crate) const INLIER_DISTANCE: f64 = 3.0;

/// The fewest inliers an estimate is taken on. Four points always fit some
/// homography; 24 consistent matches (six times the minimum sample) do not
/// happen by chance among a few hundred mutual ratio-test matches, while a
/// real overlap of a sixth of a 600 × 400 frame keeps several dozen.
pub(crate) const MIN_INLIERS: usize = 24;

/// RANSAC's iteration cap. At a 15 % inlier share an all-inlier sample of
/// four needs about 13,000 draws for 0.999, but a share that low is mostly
/// an unrelated pair; 4096 covers a 20 % share (4,300 at 0.999 is within
/// a few percent) and bounds the work at about 8 million transforms.
const MAX_ITERATIONS: usize = 4096;

/// The adaptive stop: the probability that some sample drawn so far was all
/// inliers, given the best share found.
const CONFIDENCE: f64 = 0.999;

/// The fixed seed: a registration answers the same on every run.
const SEED: u64 = 0x5EED_0564_FEA7_0001;

/// A sample triple is collinear when twice its triangle's area is under this
/// many square pixels: its homography is ill-conditioned.
const COLLINEAR_AREA: f64 = 4.0;

/// One registration's answer: `map` row-major (m[8] = 1), floating to
/// reference, continuous pixels; `pairs` its RANSAC inliers, (floating point,
/// reference point) in match order. The host scores its candidates on them
/// (`AutoAlign`'s cross-check and coverage gate, AutoAlignFeatures.swift).
/// `departure` is the lens test's departure on those inliers (module doc,
/// both frames at (0, 0, width, height)), `None` when no lens model fits;
/// the host decides whether it is significant.
pub(crate) struct FeatureFit {
    pub(crate) map: [f64; 9],
    pub(crate) pairs: Vec<Pair>,
    pub(crate) departure: Option<f64>,
}

pub(crate) type Pair = ([f64; 2], [f64; 2]);

fn distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// The mutual ratio-test matches, floating point to reference point.
fn matches(floating: &[Feature], reference: &[Feature]) -> Vec<Pair> {
    if reference.len() < 2 {
        return Vec::new();
    }
    // Per floating feature: best index, best and second distance. Per
    // reference feature: the nearest floating index.
    let mut rows = Vec::with_capacity(floating.len());
    let mut columns = vec![(f32::INFINITY, usize::MAX); reference.len()];
    for (i, f) in floating.iter().enumerate() {
        let (mut best, mut first, mut second) = (usize::MAX, f32::INFINITY, f32::INFINITY);
        for (j, r) in reference.iter().enumerate() {
            let d = distance(&f.descriptor, &r.descriptor);
            if d < first {
                second = first;
                first = d;
                best = j;
            } else if d < second {
                second = d;
            }
            if d < columns[j].0 {
                columns[j] = (d, i);
            }
        }
        rows.push((best, first, second));
    }
    rows.iter()
        .enumerate()
        .filter(|(i, (best, first, second))| {
            *best != usize::MAX && columns[*best].1 == *i && *first < RATIO * RATIO * *second
        })
        .map(|(i, (best, _, _))| {
            (
                [floating[i].x, floating[i].y],
                [reference[*best].x, reference[*best].y],
            )
        })
        .collect()
}

fn collinear(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() < COLLINEAR_AREA
}

fn degenerate(points: &[[f64; 2]; 4]) -> bool {
    (0..4).any(|skip| {
        let t: Vec<[f64; 2]> = (0..4).filter(|&k| k != skip).map(|k| points[k]).collect();
        collinear(t[0], t[1], t[2])
    })
}

/// The indices of the pairs `map` carries within [`INLIER_DISTANCE`].
fn inliers(map: &[f64; 9], pairs: &[Pair]) -> Vec<usize> {
    let limit = INLIER_DISTANCE * INLIER_DISTANCE;
    pairs
        .iter()
        .enumerate()
        .filter(|(_, (p, q))| {
            apply(map, p[0], p[1]).is_some_and(|m| {
                let (dx, dy) = (m[0] - q[0], m[1] - q[1]);
                dx * dx + dy * dy <= limit
            })
        })
        .map(|(i, _)| i)
        .collect()
}

/// RANSAC, then the two refits (module doc).
fn fit(pairs: &[Pair]) -> Option<FeatureFit> {
    let n = pairs.len();
    if n < MIN_INLIERS {
        return None;
    }
    let mut rng = SplitMix64::new(SEED);
    let mut best: Option<([f64; 9], usize)> = None;
    let mut needed = MAX_ITERATIONS;
    let mut iteration = 0;
    while iteration < needed.min(MAX_ITERATIONS) {
        iteration += 1;
        let mut pick = [0usize; 4];
        let mut k = 0;
        while k < 4 {
            let candidate = rng.below(n);
            if !pick[..k].contains(&candidate) {
                pick[k] = candidate;
                k += 1;
            }
        }
        let sample = pick.map(|i| pairs[i]);
        if degenerate(&sample.map(|p| p.0)) || degenerate(&sample.map(|p| p.1)) {
            continue;
        }
        let Some(map) = homography(&sample) else {
            continue;
        };
        let count = inliers(&map, pairs).len();
        if best.is_none_or(|(_, most)| count > most) {
            best = Some((map, count));
            let share = count as f64 / n as f64;
            let all = share.powi(4);
            needed = if all >= 1.0 {
                0
            } else if all <= 0.0 {
                MAX_ITERATIONS
            } else {
                let draws = (1.0 - CONFIDENCE).ln() / (1.0 - all).ln();
                if draws.is_finite() {
                    draws.ceil().min(MAX_ITERATIONS as f64) as usize
                } else {
                    MAX_ITERATIONS
                }
            };
        }
    }
    let (mut map, _) = best?;
    for _ in 0..2 {
        let kept: Vec<Pair> = inliers(&map, pairs).into_iter().map(|i| pairs[i]).collect();
        if kept.len() < MIN_INLIERS {
            return None;
        }
        map = homography(&kept)?;
    }
    let kept: Vec<Pair> = inliers(&map, pairs).into_iter().map(|i| pairs[i]).collect();
    (kept.len() >= MIN_INLIERS && map.iter().all(|v| v.is_finite())).then_some(FeatureFit {
        map,
        pairs: kept,
        departure: None,
    })
}

/// THE LENS TEST's departure (module doc) of the pairs between two frames at
/// `frame`: the worst inlier's distance between the lens model's prediction
/// and the plain homography's, frame pixels. `None` when no lens model or no plain
/// homography fits, or a prediction leaves the finite numbers.
fn lens_departure(frame: Rect, pairs: &[Pair]) -> Option<f64> {
    let (k, corrected) = pair_coefficient(frame, frame, pairs)?;
    let plain = homography(pairs)?;
    let (centre, radius) = lens_frame(frame);
    let radial = [k, 0.0, 0.0];
    let mut worst = 0.0f64;
    for (p, _) in pairs {
        let h = apply(&plain, p[0], p[1])?;
        let u = radial_unmap(centre, radius, *p, radial)?;
        let v = apply(&corrected, u[0], u[1])?;
        let lens = radial_map(centre, radius, v, radial, 1.0);
        let distance = (lens[0] - h[0]).hypot(lens[1] - h[1]);
        if !distance.is_finite() {
            return None;
        }
        worst = worst.max(distance);
    }
    Some(worst)
}

/// The homography taking `floating`'s points to `reference`'s (both grey,
/// `width × height`, row-major), or `None` when the sizes are outside
/// [`MIN_SIDE`]…[`MAX_SIDE`], a buffer is the wrong length, or no estimate
/// has [`MIN_INLIERS`] inliers. Deterministic.
pub(crate) fn register(
    floating: &[u8],
    reference: &[u8],
    width: usize,
    height: usize,
) -> Option<FeatureFit> {
    if !(MIN_SIDE..=MAX_SIDE).contains(&width) || !(MIN_SIDE..=MAX_SIDE).contains(&height) {
        return None;
    }
    let len = width * height;
    if floating.len() != len || reference.len() != len {
        return None;
    }
    let first = detect(floating, width, height);
    if first.len() < MIN_INLIERS {
        return None;
    }
    let second = detect(reference, width, height);
    let mut found = fit(&matches(&first, &second))?;
    // Both frames are the same size at the origin: the lens is centred on
    // each (`align_distortion::lens_frame`).
    let frame = (0, 0, width as u32, height as u32);
    found.departure = lens_departure(frame, &found.pairs);
    Some(found)
}

/// [`register`]'s peak working bytes at `width × height`: one detection
/// with the other frame's features kept, then the matches (a row and a
/// column entry per feature, a pair each) and RANSAC's inlier indices, or
/// the lens test after them (`pair_coefficient_bytes` for at most
/// `MAX_FEATURES` inliers), whichever is larger. 0 outside the size range.
pub(crate) fn scratch_bytes(width: usize, height: usize) -> u64 {
    if !(MIN_SIDE..=MAX_SIDE).contains(&width) || !(MIN_SIDE..=MAX_SIDE).contains(&height) {
        return 0;
    }
    let features = (MAX_FEATURES * std::mem::size_of::<Feature>()) as u64;
    let matching = (MAX_FEATURES
        * (std::mem::size_of::<(usize, f32, f32)>()
            + std::mem::size_of::<(f32, usize)>()
            + 2 * std::mem::size_of::<Pair>()
            + std::mem::size_of::<usize>())) as u64;
    let lens =
        (MAX_FEATURES * std::mem::size_of::<Pair>()) as u64 + pair_coefficient_bytes(MAX_FEATURES);
    detect_scratch_bytes(width, height)
        .saturating_add(features)
        .saturating_add(matching.max(lens))
}
