//! The lens test's one-pair fit (phase 3a, `feature_register`): the
//! radial coefficient of `align_distortion`'s THE MODEL fitted to one pair's
//! matches by THE FIT's own search, and the homography between the corrected
//! points. Split from `align_distortion` (its soft size cap); it builds that
//! module's one-pair `Model` and routes through its `search` and `refine`, so
//! the objective and the search have one implementation.
use crate::align_distortion::{
    lens_frame, step, Match, Model, PairGrid, COEFFICIENT_RANGE, GRID, MIN_PAIR_MATCHES,
};
use crate::align_vignette::VignetteFrame;
use crate::job_control::JobContext;
use crate::puppet_mesh::Rect;

/// [`pair_coefficient`]'s working bytes for `count` pairs: a match record
/// each, and one evaluation's corrected points.
pub(crate) fn pair_coefficient_bytes(count: usize) -> u64 {
    let each = std::mem::size_of::<Match>() + std::mem::size_of::<([f64; 2], [f64; 2])>();
    (count as u64).saturating_mul(each as u64)
}

/// The lens test's model: the k of THE MODEL fitted to one pair's matches
/// `pairs` (a point of the frame at `from`, its place in the frame at `to`),
/// each point undistorted about its own frame's lens, by THE FIT's search
/// (the grid over [`COEFFICIENT_RANGE`], then `Model::refine`), and the
/// homography between the corrected points at that k. There is no outlier
/// cut: the caller's pairs are already a model's inliers. `None` under
/// [`MIN_PAIR_MATCHES`] pairs, or when every coefficient scores +∞.
pub(crate) fn pair_coefficient(
    from: Rect,
    to: Rect,
    pairs: &[([f64; 2], [f64; 2])],
) -> Option<(f64, [f64; 9])> {
    if pairs.len() < MIN_PAIR_MATCHES {
        return None;
    }
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let frames = [
        VignetteFrame {
            rect: to,
            map: identity,
        },
        VignetteFrame {
            rect: from,
            map: identity,
        },
    ];
    // One pair, frame 1 against frame 0; only `i` and `anchor` are read
    // outside the matching, which this fit does not run.
    let model = Model {
        frames: &frames,
        grids: vec![PairGrid {
            i: 1,
            anchor: 0,
            map: identity,
            step: step(from),
            origin: [0, 0],
            size: [0, 0],
            count: [0, 0],
        }],
        lenses: frames.iter().map(|f| lens_frame(f.rect)).collect(),
        job: JobContext::NONE,
    };
    let mut matches: Vec<Match> = pairs
        .iter()
        .map(|&(p, q)| Match {
            pair: 0,
            from: p,
            to: q,
            from_u: p,
            to_u: q,
        })
        .collect();
    let (low, high) = COEFFICIENT_RANGE;
    let values: Vec<f64> = (0..GRID)
        .map(|k| low + (high - low) * k as f64 / (GRID - 1) as f64)
        .collect();
    let best = model.search(&values, &mut matches).ok()?;
    let (coefficient, (maps, _)) = model.refine(&values, best, &mut matches).ok()?;
    let map = *maps.first()?;
    coefficient.is_finite().then_some((coefficient, map))
}
