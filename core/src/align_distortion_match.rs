//! Geometric Distortion's patch matcher and homography fit (phase
//! 2b-4), split from `align_distortion` (the master of the model, the
//! objective and the search, whose module doc describes the matches): the
//! normalised cross-correlation of a luminance patch against a buffered
//! window, the sub-pixel vertex, and the Hartley-normalised DLT. Plain
//! numbers only.
//!
//! THE PATCH is warped by the prediction's local inverse Jacobian (one-pixel
//! forward differences of the prediction), sampled bilinearly in frame i, so
//! it lines up with the anchor's regular grid however the two views differ in
//! local scale (23% at the far edge of take2's ±20° overlaps); the anchor's
//! windows stay unresampled and buffered. A Jacobian whose scale passes
//! [`MAX_SHEAR`] either way is not a registration and skips the candidate.
//! A level-2 peak with a neighbour whose window left the anchor has no
//! sub-pixel position and is skipped.
use crate::align_distortion::{MIN_CONTRAST, MIN_CORRELATION, PATCH, SEARCH};
use crate::align_vignette::{apply, invert, product};

/// A patch's samples per side.
const SIDE: usize = (2 * PATCH + 1) as usize;
/// Level 1's window's samples per side.
const WINDOW: usize = (2 * (PATCH + SEARCH) + 1) as usize;
/// The largest local scale between a frame and its anchor the patch is
/// warped across; past it the prediction is not a registration.
const MAX_SHEAR: f64 = 2.0;

/// One patch and one level-1 window of f64: the matcher's fixed buffers.
pub(crate) const PATCH_SCRATCH_BYTES: u64 =
    ((SIDE * SIDE + WINDOW * WINDOW) * std::mem::size_of::<f64>()) as u64;

/// The NCC of the patch (deviations `patch`, root sum of squares `norm`)
/// against the window `window` (row length `stride`) read at `at` (the
/// window index of the patch's first sample) with sample spacing `spacing`.
fn correlation(
    patch: &[f64],
    norm: f64,
    window: &[f64],
    stride: usize,
    at: usize,
    spacing: usize,
) -> f64 {
    let mut sum = 0.0;
    let mut squares = 0.0;
    let mut cross = 0.0;
    for row in 0..SIDE {
        for column in 0..SIDE {
            let b = window[at + row * spacing * stride + column * spacing];
            sum += b;
            squares += b * b;
            cross += patch[row * SIDE + column] * b;
        }
    }
    let n = (SIDE * SIDE) as f64;
    let variance = squares - sum * sum / n;
    // Σ(a − ā)(b − b̄) = Σ(a − ā)·b, since Σ(a − ā) = 0.
    if variance.is_nan() || variance <= 0.0 || variance.is_infinite() {
        return f64::NAN;
    }
    cross / (norm * variance.sqrt())
}

/// The sub-pixel offset of a parabola's vertex through (−1, a), (0, b),
/// (1, c), held within ±0.5.
fn vertex(a: f64, b: f64, c: f64) -> f64 {
    let curve = a - 2.0 * b + c;
    if curve.is_nan() || curve >= 0.0 {
        return 0.0;
    }
    (0.5 * (a - c) / curve).clamp(-0.5, 0.5)
}

/// Solves the 8 × 8 system `a·x = b` by Gaussian elimination with partial
/// pivoting; `None` when it is singular.
fn solve8(mut a: [[f64; 8]; 8], mut b: [f64; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let pivot = (col..8).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))?;
        if a[pivot][col].is_nan() || a[pivot][col].abs() <= 1e-300 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let lead = a[col];
        for row in col + 1..8 {
            let f = a[row][col] / lead[col];
            if f != 0.0 {
                for (cell, above) in a[row][col..].iter_mut().zip(&lead[col..]) {
                    *cell -= f * above;
                }
                b[row] -= f * b[col];
            }
        }
    }
    let mut x = [0.0; 8];
    for row in (0..8).rev() {
        let mut sum = b[row];
        for k in row + 1..8 {
            sum -= a[row][k] * x[k];
        }
        x[row] = sum / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Hartley's normalisation of `points`: the similarity taking their centroid
/// to 0 and their mean distance from it to √2.
fn normalise(points: impl Iterator<Item = [f64; 2]> + Clone) -> Option<[f64; 9]> {
    let n = points.clone().count() as f64;
    let (sx, sy) = points
        .clone()
        .fold((0.0, 0.0), |s, p| (s.0 + p[0], s.1 + p[1]));
    let (mx, my) = (sx / n, sy / n);
    let mean = points.map(|p| (p[0] - mx).hypot(p[1] - my)).sum::<f64>() / n;
    if mean.is_nan() || mean <= 0.0 || mean.is_infinite() {
        return None;
    }
    let s = std::f64::consts::SQRT_2 / mean;
    Some([s, 0.0, -s * mx, 0.0, s, -s * my, 0.0, 0.0, 1.0])
}

/// The homography taking each `pairs` point `.0` to `.1` in the least-squares
/// sense (Hartley-normalised DLT, h33 = 1), scaled to m[8] = 1.
pub(crate) fn homography(pairs: &[([f64; 2], [f64; 2])]) -> Option<[f64; 9]> {
    let from = normalise(pairs.iter().map(|p| p.0))?;
    let to = normalise(pairs.iter().map(|p| p.1))?;
    let mut normal = [[0.0; 8]; 8];
    let mut right = [0.0; 8];
    for (p, q) in pairs {
        let u = apply(&from, p[0], p[1])?;
        let v = apply(&to, q[0], q[1])?;
        let rows = [
            (
                [u[0], u[1], 1.0, 0.0, 0.0, 0.0, -u[0] * v[0], -u[1] * v[0]],
                v[0],
            ),
            (
                [0.0, 0.0, 0.0, u[0], u[1], 1.0, -u[0] * v[1], -u[1] * v[1]],
                v[1],
            ),
        ];
        for (row, value) in rows {
            for r in 0..8 {
                right[r] += row[r] * value;
                for c in 0..8 {
                    normal[r][c] += row[r] * row[c];
                }
            }
        }
    }
    let h = solve8(normal, right)?;
    let fitted = [h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0];
    let m = product(&invert(&to)?, &product(&fitted, &from));
    if m[8].is_nan() || m[8].abs() <= 1e-300 {
        return None;
    }
    let out = m.map(|v| v / m[8]);
    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// The 2 × 2 inverse Jacobian (row-major) of the prediction `predict` at
/// `p`, whose image is `q`, by forward differences of one pixel; `None` when
/// it is singular, not finite, reflects, or scales by more than
/// [`MAX_SHEAR`] either way.
fn local_inverse(
    predict: &impl Fn([f64; 2]) -> Option<[f64; 2]>,
    p: [f64; 2],
    q: [f64; 2],
) -> Option<[f64; 4]> {
    let right = predict([p[0] + 1.0, p[1]])?;
    let down = predict([p[0], p[1] + 1.0])?;
    let (a, b) = (right[0] - q[0], down[0] - q[0]);
    let (c, d) = (right[1] - q[1], down[1] - q[1]);
    let det = a * d - b * c;
    if !det.is_finite() || det <= 0.0 || !(1.0 / MAX_SHEAR..=MAX_SHEAR).contains(&det.sqrt()) {
        return None;
    }
    Some([d / det, -b / det, -c / det, a / det])
}

/// Frame `frame`'s luminance at the continuous pixel position (x, y) (pixel
/// i's centre at i), bilinear over its four neighbours; `None` when one is
/// absent.
fn bilinear(
    luminance: &impl Fn(usize, i64, i64) -> Option<f64>,
    frame: usize,
    x: f64,
    y: f64,
) -> Option<f64> {
    let (x0, y0) = (x.floor(), y.floor());
    if !(x0.abs() < 9e15 && y0.abs() < 9e15) {
        return None;
    }
    let (t, u) = (x - x0, y - y0);
    let (i, j) = (x0 as i64, y0 as i64);
    let top = luminance(frame, i, j)? * (1.0 - t) + luminance(frame, i + 1, j)? * t;
    let bottom = luminance(frame, i, j + 1)? * (1.0 - t) + luminance(frame, i + 1, j + 1)? * t;
    Some(top * (1.0 - u) + bottom * u)
}

/// The matcher's buffers: the patch, level 1's window, level 2's and level
/// 2's scores.
pub(crate) struct Buffers {
    patch: Vec<f64>,
    window: Vec<f64>,
    refine: Vec<f64>,
    scores: Vec<f64>,
}

impl Buffers {
    pub(crate) fn new() -> Self {
        Self {
            patch: vec![0.0; SIDE * SIDE],
            window: vec![0.0; WINDOW * WINDOW],
            refine: Vec::new(),
            scores: Vec::new(),
        }
    }
}

/// One candidate (`align_distortion`'s module doc): frame `frame`'s pixel
/// `[px, py]` matched in frame `anchor` around `predict`'s image of its
/// centre, the patch at step `s`; the two canvas points, or `None`.
pub(crate) fn match_one(
    (frame, anchor, s): (usize, usize, i64),
    [px, py]: [i64; 2],
    predict: &impl Fn([f64; 2]) -> Option<[f64; 2]>,
    luminance: &impl Fn(usize, i64, i64) -> Option<f64>,
    buffers: &mut Buffers,
) -> Option<([f64; 2], [f64; 2])> {
    let centre = [px as f64 + 0.5, py as f64 + 0.5];
    let q = predict(centre)?;
    if q.iter().any(|v| v.abs() > 9e15) {
        return None;
    }
    let shear = local_inverse(predict, centre, q)?;
    // The patch, read once: frame i sampled along the prediction's inverse
    // Jacobian, so it lines up with the anchor's regular grid.
    let patch = &mut buffers.patch;
    for (k, slot) in patch.iter_mut().enumerate() {
        let dy = ((k / SIDE) as i64 - PATCH) as f64 * s as f64;
        let dx = ((k % SIDE) as i64 - PATCH) as f64 * s as f64;
        let x = px as f64 + shear[0] * dx + shear[1] * dy;
        let y = py as f64 + shear[2] * dx + shear[3] * dy;
        *slot = bilinear(luminance, frame, x, y)?;
    }
    let mean = patch.iter().sum::<f64>() / (SIDE * SIDE) as f64;
    if !mean.is_finite() {
        return None;
    }
    for v in patch.iter_mut() {
        *v -= mean;
    }
    let norm = patch.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm / ((SIDE * SIDE) as f64).sqrt() < MIN_CONTRAST {
        return None;
    }
    let (bx, by) = (q[0].floor() as i64, q[1].floor() as i64);

    // Level 1: steps of s around the prediction.
    let reach = PATCH + SEARCH;
    for (k, slot) in buffers.window.iter_mut().enumerate() {
        let ty = (k / WINDOW) as i64 - reach;
        let tx = (k % WINDOW) as i64 - reach;
        *slot = luminance(anchor, bx + tx * s, by + ty * s).unwrap_or(f64::NAN);
    }
    let mut best = (f64::NEG_INFINITY, 0i64, 0i64);
    for oy in -SEARCH..=SEARCH {
        for ox in -SEARCH..=SEARCH {
            let at = ((oy + SEARCH) as usize) * WINDOW + (ox + SEARCH) as usize;
            let c = correlation(patch, norm, &buffers.window, WINDOW, at, 1);
            if c > best.0 {
                best = (c, ox, oy);
            }
        }
    }
    if !best.0.is_finite() || best.1.abs() == SEARCH || best.2.abs() == SEARCH {
        return None;
    }

    // Level 2: step 1 within ±s of level 1's best.
    let (cx, cy) = (bx + best.1 * s, by + best.2 * s);
    let half = (PATCH + 1) * s;
    let side = ((2 * PATCH + 2) * s + 1) as usize;
    buffers.refine.clear();
    buffers.refine.resize(side * side, 0.0);
    for (k, slot) in buffers.refine.iter_mut().enumerate() {
        let ty = (k / side) as i64 - half;
        let tx = (k % side) as i64 - half;
        *slot = luminance(anchor, cx + tx, cy + ty).unwrap_or(f64::NAN);
    }
    let span = (2 * s + 1) as usize;
    let scores = &mut buffers.scores;
    scores.clear();
    scores.resize(span * span, f64::NAN);
    let mut top = (f64::NEG_INFINITY, 0i64, 0i64);
    for ey in -s..=s {
        for ex in -s..=s {
            // The patch's first sample sits at (−7s + e) from the centre,
            // i.e. index (s + e) from the window's edge.
            let at = ((ey + s) as usize) * side + (ex + s) as usize;
            let c = correlation(patch, norm, &buffers.refine, side, at, s as usize);
            scores[((ey + s) as usize) * span + (ex + s) as usize] = c;
            if c > top.0 {
                top = (c, ex, ey);
            }
        }
    }
    if top.0.is_nan() || top.0 < MIN_CORRELATION || top.1.abs() == s || top.2.abs() == s {
        return None;
    }
    let score = |ex: i64, ey: i64| scores[((ey + s) as usize) * span + (ex + s) as usize];
    let (ex, ey) = (top.1, top.2);
    let around = [
        score(ex - 1, ey),
        score(ex + 1, ey),
        score(ex, ey - 1),
        score(ex, ey + 1),
    ];
    // A neighbour whose window left the anchor has no score, and the peak
    // beside it no sub-pixel position.
    if around.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let dx = vertex(around[0], top.0, around[1]);
    let dy = vertex(around[2], top.0, around[3]);
    Some((
        centre,
        [(cx + ex) as f64 + dx + 0.5, (cy + ey) as f64 + dy + 0.5],
    ))
}
