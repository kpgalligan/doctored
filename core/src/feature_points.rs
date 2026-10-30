//! Feature points for Auto-Align's feature registration:
//! multi-scale oriented patches (Brown, Szeliski and Winder's MOPS) on one
//! grey frame. Plain numbers only; `feature_register` matches two frames'
//! features and fits the homography.
//!
//! THE PYRAMID. Level 0 is the frame's grey bytes over 255. Each next level
//! is the previous one blurred by [`PYRAMID_SIGMA`] (taps from
//! `sampling_gaussian::gaussian_taps`, the crate's one Gaussian kernel) and
//! averaged 2 × 2, so a level pixel's centre `i + 0.5` is level-0 point
//! `(i + 0.5)·2^l` exactly. Levels stop before the short side would pass
//! below [`MIN_LEVEL_SIDE`], at most [`MAX_LEVELS`]. A frame whose longer
//! side is under [`UPSAMPLE_BELOW`] is first detected on level 0 upsampled
//! 2× (bilinear, its pixel centre `i + 0.5` at level-0 point `(i + 0.5)/2`),
//! so the margin and the descriptor window halve in frame pixels;
//! that level shares the feature budget in proportion to its area too.
//!
//! THE CORNERS. On each level, the harmonic-mean Harris response
//! `det(S) / tr(S)` of the structure tensor S (gradients of the level blurred
//! by [`DERIVATIVE_SIGMA`], their products blurred by [`INTEGRATION_SIGMA`]);
//! its 3 × 3 maxima over [`RESPONSE_THRESHOLD`], far enough from the edge
//! that the descriptor's window fits ([`MARGIN`]), refined to sub-pixel by a
//! parabola per axis. Adaptive non-maximal suppression keeps each level's
//! share of [`MAX_FEATURES`] (in proportion to its area), the corners
//! farthest from any clearly stronger one, so they spread over the frame
//! instead of crowding its most textured part.
//!
//! THE DESCRIPTOR. The orientation is the gradient of the level blurred by
//! [`ORIENTATION_SIGMA`] at the corner. 8 × 8 samples, [`DESCRIPTOR_SPACING`]
//! level pixels apart on a grid turned to that orientation, bilinear on the
//! level blurred by [`DESCRIPTOR_SIGMA`], made zero-mean and unit-variance
//! (so a gain or offset between the frames cancels).
use crate::sampling_gaussian::gaussian_taps;

/// A descriptor's values: 8 × 8.
pub(crate) const DESCRIPTOR_LEN: usize = DESCRIPTOR_SIDE * DESCRIPTOR_SIDE;
const DESCRIPTOR_SIDE: usize = 8;

/// The most features one frame keeps, over all levels. Why 2000: the
/// matcher compares every pair (2000² descriptor distances, a fraction of a
/// second in a release build), and a frame that shares a sixth of its area
/// with the other still keeps a few hundred corners in the overlap, many
/// times the RANSAC fit's `MIN_INLIERS`.
pub(crate) const MAX_FEATURES: usize = 2000;

/// The shortest level side the pyramid builds. A 64-px level holds about
/// one descriptor window (40 px) plus its margin; a smaller one has no room
/// for a corner.
const MIN_LEVEL_SIDE: usize = 64;

/// At most five levels: a 2048-px frame's fifth is 128 px, already a
/// sixteenth of the frame, and coarser levels add no corners that matter.
const MAX_LEVELS: usize = 5;

/// The blur before each halving: MOPS's σ_p = 1, enough that the 2 × 2
/// average does not alias the next level.
const PYRAMID_SIGMA: f64 = 1.0;

/// The gradient scale (MOPS's σ_d = 1): the level's own finest structure.
const DERIVATIVE_SIGMA: f64 = 1.0;

/// The structure tensor's window (MOPS's σ_i = 1.5): a corner's neighbourhood
/// of about 9 px, small enough that two corners a descriptor apart stay two.
const INTEGRATION_SIGMA: f64 = 1.5;

/// The orientation's scale (MOPS's σ_o = 4.5): a corner's dominant gradient
/// over about its descriptor's inner half, stable under a few degrees of
/// rotation and a pixel of position error.
const ORIENTATION_SIGMA: f64 = 4.5;

/// The descriptor's pre-blur: half its sample spacing, so the 5-px samples
/// do not alias and a sub-pixel shift moves a sample by a fraction of its
/// value (MOPS samples a level two octaves coarser; this is the same band).
const DESCRIPTOR_SIGMA: f64 = 2.5;

/// The descriptor's sample spacing in level pixels (MOPS's s = 5): the
/// window spans 40 px, wide enough to be distinctive on soft texture (cloud).
const DESCRIPTOR_SPACING: f64 = 5.0;

/// The least corner response kept, on 0…1 levels. The response is about the
/// smaller eigenvalue of the smoothed squared gradient, so 1e-6 is a gradient
/// of about a quarter of a 8-bit level per pixel: only areas flat to the
/// quantization (a clipped sky, the mid-grey surround) are dropped, and ANMS
/// picks the strong, well-spread corners from the rest. MOPS's 10 on 0…255
/// levels (10 / 255² here) was too high for soft texture: a 600 × 400 view
/// of the cloud fixture kept 137 corners and its 30° neighbour did not
/// register (`tests/feature_register.rs`); at 1e-6 they keep about 1,950
/// each and register on 344 inliers.
const RESPONSE_THRESHOLD: f32 = 1e-6;

/// ANMS's robustness (MOPS's c_robust = 0.9): a corner is suppressed only by
/// one at least a tenth stronger, so two near-equal neighbours both count.
const ROBUST: f32 = 0.9;

/// How many of a level's strongest candidates ANMS considers per feature it
/// keeps. It is quadratic in the candidates; four times the budget keeps it
/// at a few million comparisons while leaving it plenty to spread.
const CANDIDATES_PER_FEATURE: usize = 4;

/// The distance from a level's edge a corner must keep, in level pixels: the
/// turned descriptor grid's corner sample is 17.5·√2 ≈ 24.7 px from its
/// centre, plus one for the bilinear neighbour and one for rounding.
const MARGIN: f64 = 27.0;

/// A frame whose longer side is under this many pixels is detected on its
/// 2× upsampled level first ([`upsampled`]). Why 512: on a small frame a
/// narrow overlap is narrower than level 0's margin on both sides plus a
/// descriptor window (37 and 57 px at 15 % of 250 and 382 px, against
/// 2 × 27 + 40), so no corner fits in both frames; the upsampled level halves
/// both in frame pixels. Below 512 px the upsampled plane's four times the
/// pixels stay under a megapixel, the size of an ordinary level 0; a larger
/// frame keeps its own level 0 as the finest.
pub(crate) const UPSAMPLE_BELOW: usize = 512;

/// One feature: its position in level-0 continuous pixels (pixel i's centre
/// at i + 0.5) and its descriptor.
pub(crate) struct Feature {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) descriptor: [f32; DESCRIPTOR_LEN],
}

/// One grey plane, row-major.
struct Plane {
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl Plane {
    /// Bilinear at the continuous point (x, y), pixel i's centre at i + 0.5,
    /// clamped at the edges.
    fn sample(&self, x: f64, y: f64) -> f32 {
        let fx = (x - 0.5).clamp(0.0, (self.width - 1) as f64);
        let fy = (y - 0.5).clamp(0.0, (self.height - 1) as f64);
        let x0 = (fx as usize).min(self.width.saturating_sub(2));
        let y0 = (fy as usize).min(self.height.saturating_sub(2));
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
        let at = |x: usize, y: usize| self.values[y * self.width + x];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        top + (bottom - top) * ty
    }
}

/// `values` (`width × height`) blurred by `sigma`, separably, the edges
/// clamped. The taps are `gaussian_taps`'s; this is the one-channel twin of
/// `sampling_gaussian::blur_band`'s FIR (the master), which reads RGBA.
fn blur(values: &[f32], width: usize, height: usize, sigma: f64) -> Option<Vec<f32>> {
    let taps: Vec<f32> = gaussian_taps(sigma)
        .ok()?
        .into_iter()
        .map(|t| t as f32)
        .collect();
    let half = (taps.len() / 2) as i64;
    let mut across = vec![0.0f32; values.len()];
    for y in 0..height {
        let row = &values[y * width..(y + 1) * width];
        for x in 0..width {
            let mut sum = 0.0;
            for (k, tap) in taps.iter().enumerate() {
                let sx = (x as i64 + k as i64 - half).clamp(0, width as i64 - 1) as usize;
                sum += tap * row[sx];
            }
            across[y * width + x] = sum;
        }
    }
    let mut out = vec![0.0f32; values.len()];
    for y in 0..height {
        for (k, tap) in taps.iter().enumerate() {
            let sy = (y as i64 + k as i64 - half).clamp(0, height as i64 - 1) as usize;
            let source = &across[sy * width..(sy + 1) * width];
            for (o, s) in out[y * width..(y + 1) * width].iter_mut().zip(source) {
                *o += tap * s;
            }
        }
    }
    Some(out)
}

/// The next pyramid level: `plane` blurred by [`PYRAMID_SIGMA`], 2 × 2
/// averaged (an odd last row or column dropped).
fn halve(plane: &Plane) -> Option<Plane> {
    let blurred = blur(&plane.values, plane.width, plane.height, PYRAMID_SIGMA)?;
    let (width, height) = (plane.width / 2, plane.height / 2);
    let mut values = vec![0.0f32; width * height];
    for y in 0..height {
        for x in 0..width {
            let i = 2 * y * plane.width + 2 * x;
            values[y * width + x] = 0.25
                * (blurred[i]
                    + blurred[i + 1]
                    + blurred[i + plane.width]
                    + blurred[i + plane.width + 1]);
        }
    }
    Some(Plane {
        width,
        height,
        values,
    })
}

/// `plane` at twice its size, bilinear through [`Plane::sample`]: the new
/// level's pixel centre `i + 0.5` is `plane`'s point `(i + 0.5) / 2`.
fn upsampled(plane: &Plane) -> Plane {
    let (width, height) = (2 * plane.width, 2 * plane.height);
    let mut values = Vec::with_capacity(width * height);
    for y in 0..height {
        let sy = (y as f64 + 0.5) / 2.0;
        for x in 0..width {
            values.push(plane.sample((x as f64 + 0.5) / 2.0, sy));
        }
    }
    Plane {
        width,
        height,
        values,
    }
}

/// The harmonic-mean Harris response `det(S) / tr(S)` of `plane` (0 where
/// the trace is 0).
fn response(plane: &Plane) -> Option<Vec<f32>> {
    let (w, h) = (plane.width, plane.height);
    let smooth = blur(&plane.values, w, h, DERIVATIVE_SIGMA)?;
    let mut gx = vec![0.0f32; w * h];
    let mut gy = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (l, r) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let (u, d) = (y.saturating_sub(1), (y + 1).min(h - 1));
            gx[y * w + x] = 0.5 * (smooth[y * w + r] - smooth[y * w + l]);
            gy[y * w + x] = 0.5 * (smooth[d * w + x] - smooth[u * w + x]);
        }
    }
    drop(smooth);
    let gxy: Vec<f32> = gx.iter().zip(&gy).map(|(a, b)| a * b).collect();
    gx.iter_mut().for_each(|v| *v *= *v);
    gy.iter_mut().for_each(|v| *v *= *v);
    let sxx = blur(&gx, w, h, INTEGRATION_SIGMA)?;
    drop(gx);
    let syy = blur(&gy, w, h, INTEGRATION_SIGMA)?;
    drop(gy);
    let sxy = blur(&gxy, w, h, INTEGRATION_SIGMA)?;
    drop(gxy);
    Some(
        sxx.iter()
            .zip(&syy)
            .zip(&sxy)
            .map(|((a, b), c)| {
                let trace = a + b;
                if trace > 1e-12 {
                    (a * b - c * c) / trace
                } else {
                    0.0
                }
            })
            .collect(),
    )
}

/// A corner candidate: level continuous position and response.
#[derive(Clone, Copy)]
struct Candidate {
    x: f32,
    y: f32,
    strength: f32,
}

/// The parabola vertex through (−1, a), (0, b), (1, c), held within ±0.5.
fn vertex(a: f32, b: f32, c: f32) -> f32 {
    let curve = a - 2.0 * b + c;
    if curve.is_nan() || curve >= 0.0 {
        return 0.0;
    }
    (0.5 * (a - c) / curve).clamp(-0.5, 0.5)
}

/// The 3 × 3 maxima of `f` over the threshold, inside the margin, at
/// sub-pixel positions.
fn maxima(f: &[f32], width: usize, height: usize) -> Vec<Candidate> {
    let mut out = Vec::new();
    let lo = MARGIN as usize;
    if width <= 2 * lo || height <= 2 * lo {
        return out;
    }
    for y in lo..height - lo {
        for x in lo..width - lo {
            let v = f[y * width + x];
            if v.is_nan() || v <= RESPONSE_THRESHOLD {
                continue;
            }
            let mut peak = true;
            'around: for dy in 0..3 {
                for dx in 0..3 {
                    if (dx, dy) == (1, 1) {
                        continue;
                    }
                    let n = f[(y + dy - 1) * width + x + dx - 1];
                    // Ties go to the first in raster order.
                    if n > v || (n == v && (dy, dx) < (1, 1)) {
                        peak = false;
                        break 'around;
                    }
                }
            }
            if peak {
                let i = y * width + x;
                out.push(Candidate {
                    x: x as f32 + 0.5 + vertex(f[i - 1], v, f[i + 1]),
                    y: y as f32 + 0.5 + vertex(f[i - width], v, f[i + width]),
                    strength: v,
                });
            }
        }
    }
    out
}

/// Adaptive non-maximal suppression: the `keep` candidates with the largest
/// distance to a candidate more than 1/[`ROBUST`] as strong.
fn suppress(mut candidates: Vec<Candidate>, keep: usize) -> Vec<Candidate> {
    candidates.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    candidates.truncate(keep.saturating_mul(CANDIDATES_PER_FEATURE));
    let mut radii: Vec<(f32, usize)> = Vec::with_capacity(candidates.len());
    for (i, c) in candidates.iter().enumerate() {
        let mut nearest = f32::INFINITY;
        // Sorted strongest first: only earlier candidates can be stronger.
        for d in &candidates[..i] {
            if c.strength < ROBUST * d.strength {
                let (dx, dy) = (c.x - d.x, c.y - d.y);
                nearest = nearest.min(dx * dx + dy * dy);
            }
        }
        radii.push((nearest, i));
    }
    // Largest radius first; ties keep the stronger (earlier) one.
    radii.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    radii.truncate(keep);
    radii.into_iter().map(|(_, i)| candidates[i]).collect()
}

/// The oriented descriptor at level point (x, y), or `None` on a patch with
/// no variance.
fn describe(oriented: &Plane, patch: &Plane, x: f64, y: f64) -> Option<[f32; DESCRIPTOR_LEN]> {
    let gx = f64::from(oriented.sample(x + 1.0, y) - oriented.sample(x - 1.0, y));
    let gy = f64::from(oriented.sample(x, y + 1.0) - oriented.sample(x, y - 1.0));
    let norm = gx.hypot(gy);
    let (c, s) = if norm > 1e-12 {
        (gx / norm, gy / norm)
    } else {
        (1.0, 0.0)
    };
    let mut d = [0.0f32; DESCRIPTOR_LEN];
    let centre = (DESCRIPTOR_SIDE as f64 - 1.0) / 2.0;
    for j in 0..DESCRIPTOR_SIDE {
        for i in 0..DESCRIPTOR_SIDE {
            let u = (i as f64 - centre) * DESCRIPTOR_SPACING;
            let v = (j as f64 - centre) * DESCRIPTOR_SPACING;
            // The grid's x axis along the gradient, its y axis across it.
            d[j * DESCRIPTOR_SIDE + i] = patch.sample(x + c * u - s * v, y + s * u + c * v);
        }
    }
    let n = DESCRIPTOR_LEN as f32;
    let mean = d.iter().sum::<f32>() / n;
    let variance = d.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
    if variance.is_nan() || variance <= 1e-10 {
        return None;
    }
    let scale = 1.0 / variance.sqrt();
    d.iter_mut().for_each(|v| *v = (*v - mean) * scale);
    Some(d)
}

/// The pyramid's level sizes for a frame.
fn levels(width: usize, height: usize) -> Vec<(usize, usize)> {
    let mut out = vec![(width, height)];
    while out.len() < MAX_LEVELS {
        let (w, h) = out[out.len() - 1];
        if w.min(h) / 2 < MIN_LEVEL_SIDE {
            break;
        }
        out.push((w / 2, h / 2));
    }
    out
}

/// The features of the grey frame `samples` (`width × height`, row-major),
/// at most [`MAX_FEATURES`]. Empty when the frame is smaller than a level or
/// `samples` is not `width × height` long.
pub(crate) fn detect(samples: &[u8], width: usize, height: usize) -> Vec<Feature> {
    let mut out = Vec::new();
    if width < 2 || height < 2 || samples.len() != width.saturating_mul(height) {
        return out;
    }
    let sizes = levels(width, height);
    let upsample = width.max(height) < UPSAMPLE_BELOW;
    let finer = if upsample { 4 * width * height } else { 0 };
    let area: usize = finer + sizes.iter().map(|(w, h)| w * h).sum::<usize>();
    let mut plane = Plane {
        width,
        height,
        values: samples.iter().map(|&v| f32::from(v) / 255.0).collect(),
    };
    if upsample {
        let up = upsampled(&plane);
        if !detect_level(&up, 0.5, (MAX_FEATURES * finer).div_ceil(area), &mut out) {
            return out;
        }
    }
    for (level, &(w, h)) in sizes.iter().enumerate() {
        if level > 0 {
            match halve(&plane) {
                Some(next) => plane = next,
                None => break,
            }
        }
        let budget = (MAX_FEATURES * w * h).div_ceil(area);
        if !detect_level(&plane, (1usize << level) as f64, budget, &mut out) {
            break;
        }
    }
    out.truncate(MAX_FEATURES);
    out
}

/// One level's features, at most `budget`, appended to `out` at level-0
/// positions (a level point times `to_frame`). false when a blur fails, which
/// ends the detection.
fn detect_level(plane: &Plane, to_frame: f64, budget: usize, out: &mut Vec<Feature>) -> bool {
    let (w, h) = (plane.width, plane.height);
    let Some(f) = response(plane) else {
        return false;
    };
    let kept = suppress(maxima(&f, w, h), budget);
    drop(f);
    if kept.is_empty() {
        return true;
    }
    let (Some(oriented), Some(patch)) = (
        blur(&plane.values, w, h, ORIENTATION_SIGMA),
        blur(&plane.values, w, h, DESCRIPTOR_SIGMA),
    ) else {
        return false;
    };
    let oriented = Plane {
        width: w,
        height: h,
        values: oriented,
    };
    let patch = Plane {
        width: w,
        height: h,
        values: patch,
    };
    for c in kept {
        let (x, y) = (f64::from(c.x), f64::from(c.y));
        if let Some(descriptor) = describe(&oriented, &patch, x, y) {
            out.push(Feature {
                x: x * to_frame,
                y: y * to_frame,
                descriptor,
            });
        }
    }
    true
}

/// [`detect`]'s peak working bytes for a `width × height` frame: the finest
/// level's plane and, at most, five more planes of its size alive at once
/// (the response's gradients, products and blur pass; the orientation and
/// patch planes with their blur pass come after the response is dropped),
/// level 0's plane beside them when the finest is the upsampled level
/// ([`UPSAMPLE_BELOW`], four times the pixels), the finest level's local
/// maxima (at most a quarter of its pixels, 12 bytes each, twice over for
/// the vector's growth), ANMS's radii and the features.
pub(crate) fn detect_scratch_bytes(width: usize, height: usize) -> u64 {
    let pixels = (width as u64).saturating_mul(height as u64);
    let float = std::mem::size_of::<f32>() as u64;
    let (finest, beside) = if width.max(height) < UPSAMPLE_BELOW {
        (pixels.saturating_mul(4), pixels)
    } else {
        (pixels, 0)
    };
    let planes = finest
        .saturating_mul(6)
        .saturating_add(beside)
        .saturating_mul(float);
    let maxima = (finest / 4).saturating_mul(2 * std::mem::size_of::<Candidate>() as u64);
    let anms = (MAX_FEATURES * CANDIDATES_PER_FEATURE * std::mem::size_of::<(f32, usize)>()) as u64;
    let features = (MAX_FEATURES * std::mem::size_of::<Feature>()) as u64;
    planes
        .saturating_add(maxima)
        .saturating_add(anms)
        .saturating_add(features)
}
