//! Auto-Align Layers' Geometric Distortion (phase 2b-4): one radial
//! coefficient fitted from patch matches in the registered overlaps of a
//! stack, and the refined maps between the corrected frames. Plain numbers
//! only; the adapters are `doc_align_distortion` (both depths), which read
//! each layer's linear light through a closure, and the reprojection applies
//! the coefficient through `align_projection` (`lens_model::radial_map`, Lens
//! Correction's own radial map).
//!
//! THE MODEL (take2's D re-read, `scripts/photoshop-probe/
//! panorama_lens_reread.py`: three frames of one lens, one correction each).
//! ONE coefficient `k` for the stack. A frame's lens is centred on its rect,
//! c = the rect's centre, normalised by R = max(w, h) / 2 (Lens Correction's
//! normalisation, [`lens_frame`]), and a layer pixel p is the SOURCE of the
//! corrected point u: p = `radial_map(c, R, u, [k, 0, 0], 1)`, so u =
//! `radial_unmap(c, R, p, [k, 0, 0])`. Negative k is barrel distortion
//! (straight lines bow outward), positive k pincushion.
//!
//! THE MATCHES. Each later frame i pairs with its anchor a (the reference or
//! the neighbour the host chained it to) through A = H_a⁻¹·H_i, the maps
//! taking a layer's canvas point to the reference's (`align_vignette`'s
//! overlap probe and helpers). A grid over the part of frame i the probe
//! finds inside frame a, inset by (PATCH + SEARCH + 1)·s so every patch and
//! search window can lie inside frame i, is spaced so the pairs share
//! [`MAX_MATCHES`] in proportion to their overlap area; s = [`step`] of frame
//! i. A patch samples the luminance Y (the mean of the linear channels) of
//! frame i at steps of s, 15 × 15 samples, read once into a buffer, warped by
//! the prediction's local Jacobian (`align_distortion_match`); one whose
//! standard deviation is below [`MIN_CONTRAST`] is skipped. Level 1 searches
//! frame a at steps of s within ±[`SEARCH`] steps around the prediction, by
//! normalised cross-correlation (NCC) over one buffered window; level 2
//! searches at step 1 within ±s of level 1's best, over a second buffered
//! window ([`refine_bytes`]); a parabola through the peak's two neighbours on
//! each axis gives the sub-pixel position. A match is kept when both peaks
//! are interior (neither on its search's edge, every neighbour scored) and
//! its NCC is at least [`MIN_CORRELATION`].
//!
//! TWO PASSES. Pass 1 predicts through A, the host's lens-free maps; a lens
//! moves a frame's edge by more than the ±8-step search (k −0.05 moves a
//! 600 px frame's edge midpoint 16 px), so pass 1's matches cluster where
//! the lens moved little. Pass 2 matches the same grid again, predicted
//! through pass 1's coefficient and corrected maps (undistort, map,
//! distort), so it reaches the whole overlap; its answer is the fit's. Pass
//! 1's matches are dropped before pass 2's are made, so the scratch is one
//! pass's.
//!
//! THE FIT (each pass). Each pair's homography is fitted to its corrected
//! matches (raw at k = 0 in pass 1, at pass 1's k in pass 2), and a match
//! farther from it than 4 × the pair's median residual + s px is dropped (a
//! repeated texture's wrong peak); after the grid below, the same cut runs
//! once more at the grid's best k, because at k = 0 the lens itself
//! inflates the median and lets wrong peaks a lens's width away through. A
//! pair under [`MIN_PAIR_MATCHES`] before or after a cut refuses
//! [`TOO_LITTLE_OVERLAP`]. For a coefficient k
//! both points of every match are undistorted, each with its own frame's c
//! and R, each pair's homography P is fitted to the corrected points by a
//! Hartley-normalised DLT with h33 = 1 (8 × 8 normal equations), and the
//! objective is Σ|P(u_i) − u_a|². A k at which [`folds`] holds for either
//! frame of a pair, or at which any kept match has no corrected point,
//! scores +∞. k is the best of 41 evenly spaced values over
//! [`COEFFICIENT_RANGE`] (searched again after the second cut), refined by
//! 40 golden-section steps between that value's neighbours
//! (`align_vignette`'s pattern). The maps compose along
//! the anchors: maps[0] = I, maps[i] = maps[a]·P_i, each a frame's corrected
//! canvas point to the reference's corrected point; `residual` is the RMS
//! transfer error, in pixels, at k.
//!
//! WHY THE NUMBERS:
//! - [`COEFFICIENT_RANGE`] ±0.25: at R = max(w, h)/2 a 3:2 frame's corner is
//!   at r² 1.44, where 0.25 moves the corner by more than a third of R: far
//!   past any camera lens, and past it [`folds`] refuses anyway.
//! - [`MAX_MATCHES`] 4096: each match is two points, so a few thousand pin a
//!   one-parameter radial curve and 8-parameter homographies far below a
//!   pixel, while the search stays a fraction of a second at 24 MP.
//! - [`MIN_PAIR_MATCHES`] 24: three times the eight unknowns of the pair's
//!   homography, so the coefficient is not absorbed by a pair that fits any
//!   map exactly.
//! - [`PATCH`] 7 (15 × 15 samples) and [`SEARCH`] 8 steps: a patch of 225
//!   samples gives an NCC whose noise is far below the 0.9 cut on textured
//!   content, and ±8 steps covers the host's registration error (a few
//!   pixels at s = 1) with room for the lens's own displacement.
//! - [`MIN_CORRELATION`] 0.9 (NCC): a true match of the same lens's pixels
//!   correlates above 0.95 even under a few percent of local scale; 0.9
//!   keeps those and drops repeated or flat content.
//! - [`MIN_CONTRAST`] 0.004 (std of linear Y): about one 8-bit code in the
//!   mid-tones; below it the patch is flat and its NCC is noise.
//! - [`step`] max(w, h)/600: the patch and the search scale with the frame,
//!   so a 24 MP frame searches the same share of itself as a 600 px one.
use crate::align_distortion_match::{homography, match_one, Buffers, PATCH_SCRATCH_BYTES};
use crate::align_vignette::{apply, invert, overlap, product, VignetteFrame};
use crate::job_control::JobContext;
use crate::lens_model::{radial_folds, radial_map, radial_monotone, radial_unmap};
use crate::puppet_mesh::Rect;

/// The coefficients the fit searches; see the module doc.
pub(crate) const COEFFICIENT_RANGE: (f64, f64) = (-0.25, 0.25);
/// Every pair's candidate points together; see the module doc.
pub(crate) const MAX_MATCHES: usize = 4096;
/// The fewest kept matches a pair may have; see the module doc.
pub(crate) const MIN_PAIR_MATCHES: usize = 24;
/// A patch's half side, in steps: 15 × 15 samples; see the module doc.
pub(crate) const PATCH: i64 = 7;
/// Level 1's search, ± steps around the map's prediction; see the module doc.
pub(crate) const SEARCH: i64 = 8;
/// The least NCC a kept match has; see the module doc.
pub(crate) const MIN_CORRELATION: f64 = 0.9;
/// The least standard deviation of a patch's linear Y; see the module doc.
pub(crate) const MIN_CONTRAST: f64 = 0.004;
/// The evenly spaced coefficients tried before the golden-section search.
pub(crate) const GRID: usize = 41;
/// The golden-section steps: the 0.025-wide starting bracket shrinks by
/// 0.618 each, so 40 take it below 1e-9.
const GOLDEN_STEPS: usize = 40;

/// The refusal when a pair keeps fewer than [`MIN_PAIR_MATCHES`] matches.
pub(crate) const TOO_LITTLE_OVERLAP: &str = "Geometric Distortion found too little overlap \
     between the layers to measure the distortion. Nothing was changed.";

/// One kept match: its pair, the two raw canvas points (frame i's, its
/// anchor's), and the corrected points the objective writes for each k.
#[derive(Clone, Copy)]
pub(crate) struct Match {
    pub(crate) pair: u32,
    pub(crate) from: [f64; 2],
    pub(crate) to: [f64; 2],
    pub(crate) from_u: [f64; 2],
    pub(crate) to_u: [f64; 2],
}

/// The fit's scratch at its largest: [`MAX_MATCHES`] match records, one
/// patch and one level-1 window of f64 (`align_distortion_match`'s
/// [`PATCH_SCRATCH_BYTES`]). Level 2's window grows with the step and is
/// [`refine_bytes`].
pub(crate) const FIT_SCRATCH_BYTES: u64 =
    MAX_MATCHES as u64 * std::mem::size_of::<Match>() as u64 + PATCH_SCRATCH_BYTES;

/// Level 2's buffered window for a frame of step `step`: (16·s + 1)² f64
/// (the patch's 15 samples at step s, searched ±s at step 1).
pub(crate) fn refine_bytes(step: i64) -> u64 {
    let side = (2 * PATCH + 2) as u64 * step.max(1) as u64 + 1;
    side.saturating_mul(side).saturating_mul(8)
}

/// A frame's patch step: max(1, round(max(w, h) / 600)); see the module doc.
pub(crate) fn step(rect: Rect) -> i64 {
    let longer = f64::from(rect.2.max(rect.3));
    ((longer / 600.0).round() as i64).max(1)
}

/// A frame's lens centre and normalising radius: the rect's centre and
/// max(w, h) / 2, in canvas pixels.
pub(crate) fn lens_frame(rect: Rect) -> ([f64; 2], f64) {
    let (w, h) = (f64::from(rect.2), f64::from(rect.3));
    (
        [rect.0 as f64 + w / 2.0, rect.1 as f64 + h / 2.0],
        w.max(h) / 2.0,
    )
}

/// Whether the coefficient `k` folds the frame at `rect`: `radial_folds` at
/// the corner's normalised radius, or a corner with no corrected point (for
/// k < 0 that is k < −4/(27·r_c²), tighter than `radial_folds`). The one fold
/// test: `project_layer`'s refusal and the fit's +∞. k = 0 never folds.
pub(crate) fn folds(rect: Rect, k: f64) -> bool {
    if k == 0.0 {
        return false;
    }
    let (centre, radius) = lens_frame(rect);
    if !k.is_finite() || radius <= 0.0 {
        return true;
    }
    let (w, h) = (f64::from(rect.2), f64::from(rect.3));
    let corner = w.hypot(h) / 2.0 / radius;
    if radial_folds([k, 0.0, 0.0], corner * corner) {
        return true;
    }
    let (x0, y0) = (rect.0 as f64, rect.1 as f64);
    [[x0, y0], [x0 + w, y0], [x0 + w, y0 + h], [x0, y0 + h]]
        .into_iter()
        .any(|p| radial_unmap(centre, radius, p, [k, 0.0, 0.0]).is_none())
}

/// The fitted distortion: `coefficient` is k, `maps` each frame's corrected
/// canvas point to the reference's (row-major, m[8] = 1), `matches` the kept
/// matches and `residual` the RMS transfer error in pixels at k.
#[derive(Clone, Debug)]
pub(crate) struct DistortionFit {
    pub coefficient: f64,
    pub maps: Vec<[f64; 9]>,
    pub matches: u64,
    pub residual: f64,
}

/// One pair's candidate grid over frame `i`'s inset overlap with its anchor.
pub(crate) struct PairGrid {
    pub(crate) i: usize,
    pub(crate) anchor: usize,
    pub(crate) map: [f64; 9],
    pub(crate) step: i64,
    pub(crate) origin: [i64; 2],
    pub(crate) size: [i64; 2],
    pub(crate) count: [usize; 2],
}

/// The fit's state: the frames, the pair grids, each frame's lens, the job.
pub(crate) struct Model<'a> {
    pub(crate) frames: &'a [VignetteFrame],
    pub(crate) grids: Vec<PairGrid>,
    pub(crate) lenses: Vec<([f64; 2], f64)>,
    pub(crate) job: JobContext<'a>,
}

/// Every pair's homography between the corrected points and the summed
/// squared transfer error.
pub(crate) type PairFit = (Vec<[f64; 9]>, f64);

/// One pass's answer: k, each pair's homography between the corrected
/// points, the summed squared transfer error and the matches kept.
struct Solved {
    coefficient: f64,
    pairs: Vec<[f64; 9]>,
    total: f64,
    matches: Vec<Match>,
}

impl Model<'_> {
    /// Every grid's candidates matched through `predict(pair, point)`, the
    /// progress reported over `from..to`.
    fn collect(
        &self,
        capacity: usize,
        predict: impl Fn(usize, [f64; 2]) -> Option<[f64; 2]>,
        luminance: &impl Fn(usize, i64, i64) -> Option<f64>,
        (from, to): (f64, f64),
    ) -> Result<Vec<Match>, String> {
        let mut matches: Vec<Match> = Vec::new();
        matches
            .try_reserve_exact(capacity.min(MAX_MATCHES))
            .map_err(|_| "could not allocate the distortion fit".to_string())?;
        let mut buffers = Buffers::new();
        let mut walked = 0usize;
        let walk = capacity.max(1) as f64;
        for (index, grid) in self.grids.iter().enumerate() {
            let [nx, ny] = grid.count;
            let pair = |p: [f64; 2]| predict(index, p);
            for gy in 0..ny {
                let py =
                    grid.origin[1] + ((gy as f64 + 0.5) * grid.size[1] as f64 / ny as f64) as i64;
                for gx in 0..nx {
                    self.job.every(walked)?;
                    walked += 1;
                    if matches.len() == matches.capacity() {
                        break;
                    }
                    let px = grid.origin[0]
                        + ((gx as f64 + 0.5) * grid.size[0] as f64 / nx as f64) as i64;
                    if let Some((from_point, to_point)) = match_one(
                        (grid.i, grid.anchor, grid.step),
                        [px, py],
                        &pair,
                        luminance,
                        &mut buffers,
                    ) {
                        matches.push(Match {
                            pair: index as u32,
                            from: from_point,
                            to: to_point,
                            from_u: from_point,
                            to_u: to_point,
                        });
                    }
                }
                self.job
                    .report(from + (to - from) * walked as f64 / walk, 1)?;
            }
        }
        Ok(matches)
    }

    /// Writes every match's corrected points at k; `false` (+∞) where a
    /// frame folds or a point has no corrected preimage.
    fn correct(&self, k: f64, matches: &mut [Match]) -> Result<bool, String> {
        let frames = self.frames;
        if self
            .grids
            .iter()
            .any(|g| folds(frames[g.i].rect, k) || folds(frames[g.anchor].rect, k))
        {
            return Ok(false);
        }
        let radial = [k, 0.0, 0.0];
        for (index, m) in matches.iter_mut().enumerate() {
            self.job.every(index)?;
            let grid = &self.grids[m.pair as usize];
            let (ci, ri) = self.lenses[grid.i];
            let (ca, ra) = self.lenses[grid.anchor];
            let (Some(u), Some(v)) = (
                radial_unmap(ci, ri, m.from, radial),
                radial_unmap(ca, ra, m.to, radial),
            ) else {
                return Ok(false);
            };
            m.from_u = u;
            m.to_u = v;
        }
        Ok(true)
    }

    /// Every pair's homography at k and the summed squared transfer error,
    /// or `None` (+∞) where the module doc says so.
    fn evaluate(&self, k: f64, matches: &mut [Match]) -> Result<Option<PairFit>, String> {
        if !self.correct(k, matches)? {
            return Ok(None);
        }
        let mut maps = Vec::with_capacity(self.grids.len());
        let mut total = 0.0;
        for index in 0..self.grids.len() {
            let mine = corrected(matches, index);
            let Some(h) = homography(&mine) else {
                return Ok(None);
            };
            for (p, q) in &mine {
                let Some(r) = apply(&h, p[0], p[1]) else {
                    return Ok(None);
                };
                total += (r[0] - q[0]).powi(2) + (r[1] - q[1]).powi(2);
            }
            maps.push(h);
        }
        Ok(total.is_finite().then_some((maps, total)))
    }

    fn objective(&self, k: f64, matches: &mut [Match]) -> Result<f64, String> {
        Ok(self
            .evaluate(k, matches)?
            .map_or(f64::INFINITY, |(_, total)| total))
    }

    /// The outlier pass at k: each pair's homography on its corrected
    /// points, and a match farther from it than 4 × the pair's median
    /// residual + s px dropped; a pair under [`MIN_PAIR_MATCHES`] before or
    /// after refuses.
    fn reject(&self, k: f64, mut matches: Vec<Match>) -> Result<Vec<Match>, String> {
        if !self.correct(k, &mut matches)? {
            return Err(TOO_LITTLE_OVERLAP.into());
        }
        let mut kept: Vec<Match> = Vec::with_capacity(matches.len());
        for (index, grid) in self.grids.iter().enumerate() {
            self.job.check()?;
            let mine = corrected(&matches, index);
            if mine.len() < MIN_PAIR_MATCHES {
                return Err(TOO_LITTLE_OVERLAP.into());
            }
            let h = homography(&mine).ok_or(TOO_LITTLE_OVERLAP)?;
            let residuals: Vec<f64> = mine
                .iter()
                .map(|(p, q)| {
                    apply(&h, p[0], p[1])
                        .map_or(f64::INFINITY, |r| (r[0] - q[0]).hypot(r[1] - q[1]))
                })
                .collect();
            let mut sorted = residuals.clone();
            sorted.sort_by(f64::total_cmp);
            let cut = 4.0 * sorted[sorted.len() / 2] + grid.step as f64;
            let before = kept.len();
            kept.extend(
                matches
                    .iter()
                    .filter(|m| m.pair as usize == index)
                    .zip(&residuals)
                    .filter(|(_, r)| **r <= cut)
                    .map(|(m, _)| *m),
            );
            if kept.len() - before < MIN_PAIR_MATCHES {
                return Err(TOO_LITTLE_OVERLAP.into());
            }
        }
        Ok(kept)
    }

    /// The best of the [`GRID`] values' index, or the refusal when every
    /// value is +∞.
    pub(crate) fn search(&self, values: &[f64], matches: &mut [Match]) -> Result<usize, String> {
        let mut scores = Vec::with_capacity(values.len());
        for &value in values {
            scores.push(self.objective(value, matches)?);
        }
        let best = (0..values.len())
            .min_by(|&x, &y| scores[x].total_cmp(&scores[y]))
            .unwrap_or(0);
        if scores[best].is_finite() {
            Ok(best)
        } else {
            Err(TOO_LITTLE_OVERLAP.into())
        }
    }

    /// One pass (module doc): the outliers dropped at `start`, the grid, the
    /// outliers dropped again at the grid's best, the grid again, then the
    /// golden-section search between the best value's neighbours.
    fn solve(
        &self,
        start: f64,
        matches: Vec<Match>,
        (from, to): (f64, f64),
    ) -> Result<Solved, String> {
        let (low, high) = COEFFICIENT_RANGE;
        let values: Vec<f64> = (0..GRID)
            .map(|k| low + (high - low) * k as f64 / (GRID - 1) as f64)
            .collect();
        let mut matches = self.reject(start, matches)?;
        let first = self.search(&values, &mut matches)?;
        let mut matches = self.reject(values[first], matches)?;
        let best = self.search(&values, &mut matches)?;
        self.job.report(from + (to - from) * 0.5, 3)?;
        let (coefficient, (pairs, total)) = self.refine(&values, best, &mut matches)?;
        self.job.report(to, 3)?;
        Ok(Solved {
            coefficient,
            pairs,
            total,
            matches,
        })
    }

    /// The golden-section search between the grid's best value's
    /// neighbours, then the evaluation at its answer (the module doc's
    /// FIT): k and every pair's homography and summed squared error.
    pub(crate) fn refine(
        &self,
        values: &[f64],
        best: usize,
        matches: &mut [Match],
    ) -> Result<(f64, PairFit), String> {
        let (mut a, mut b) = (
            values[best.saturating_sub(1)],
            values[(best + 1).min(GRID - 1)],
        );
        let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
        let (mut c, mut d) = (b - ratio * (b - a), a + ratio * (b - a));
        let (mut ec, mut ed) = (self.objective(c, matches)?, self.objective(d, matches)?);
        for _ in 0..GOLDEN_STEPS {
            if ec < ed {
                b = d;
                d = c;
                ed = ec;
                c = b - ratio * (b - a);
                ec = self.objective(c, matches)?;
            } else {
                a = c;
                c = d;
                ec = ed;
                d = a + ratio * (b - a);
                ed = self.objective(d, matches)?;
            }
        }
        let mut coefficient = (a + b) / 2.0;
        let mut result = self.evaluate(coefficient, matches)?;
        if result.is_none() {
            // The bracket's midpoint can only fail at the range's folding
            // edge; the grid's best is the coefficient then.
            coefficient = values[best];
            result = self.evaluate(coefficient, matches)?;
        }
        Ok((coefficient, result.ok_or(TOO_LITTLE_OVERLAP)?))
    }
}

/// Pair `index`'s corrected points.
fn corrected(matches: &[Match], index: usize) -> Vec<([f64; 2], [f64; 2])> {
    matches
        .iter()
        .filter(|m| m.pair as usize == index)
        .map(|m| (m.from_u, m.to_u))
        .collect()
}

/// The fit (module doc). `frames` lists each layer's rect and Vision map
/// (its canvas point to the reference's); `anchors[i]` is the index of the
/// frame frame `i` pairs with, earlier than `i` (anchors[0] is ignored).
/// `linear(frame, x, y)` is frame `frame`'s pixel at canvas pixel (x, y) in
/// linear light, or `None` outside its rect or below full alpha. Refuses
/// [`TOO_LITTLE_OVERLAP`] for a pair under [`MIN_PAIR_MATCHES`].
pub(crate) fn fit(
    frames: &[VignetteFrame],
    anchors: &[usize],
    linear: impl Fn(usize, i64, i64) -> Option<[f64; 3]>,
    job: JobContext<'_>,
) -> Result<DistortionFit, String> {
    let n = frames.len();
    if n < 2 || anchors.len() != n || (1..n).any(|i| anchors[i] >= i) {
        return Err(TOO_LITTLE_OVERLAP.into());
    }
    // ---- the grids
    let mut found = Vec::with_capacity(n - 1);
    for i in 1..n {
        job.check()?;
        let a = anchors[i];
        let inverse = invert(&frames[a].map).ok_or(TOO_LITTLE_OVERLAP)?;
        let map = product(&inverse, &frames[i].map);
        let s = step(frames[i].rect);
        let inset = (PATCH + SEARCH + 1) * s;
        let Some((origin, size, area)) = overlap(frames[i].rect, frames[a].rect, &map) else {
            return Err(TOO_LITTLE_OVERLAP.into());
        };
        let size = [size[0] as i64 - 2 * inset, size[1] as i64 - 2 * inset];
        if size[0] < 1 || size[1] < 1 {
            return Err(TOO_LITTLE_OVERLAP.into());
        }
        let origin = [origin[0] + inset, origin[1] + inset];
        found.push((i, a, map, s, origin, size, area));
    }
    let total: f64 = found.iter().map(|f| f.6).sum();
    let mut grids = Vec::with_capacity(found.len());
    let mut capacity = 0usize;
    for (i, anchor, map, step, origin, size, area) in found {
        let budget = (MAX_MATCHES as f64 * area / total).floor();
        if budget.is_nan() || budget < 1.0 {
            return Err(TOO_LITTLE_OVERLAP.into());
        }
        let (w, h) = (size[0] as f64, size[1] as f64);
        let spacing = (w * h / budget).sqrt().max(1.0);
        let nx = ((w / spacing).floor() as usize).clamp(1, size[0] as usize);
        let ny = ((h / spacing).floor() as usize)
            .clamp(1, size[1] as usize)
            .min((budget as usize / nx).max(1));
        capacity += nx * ny;
        grids.push(PairGrid {
            i,
            anchor,
            map,
            step,
            origin,
            size,
            count: [nx, ny],
        });
    }
    let model = Model {
        frames,
        grids,
        lenses: frames.iter().map(|f| lens_frame(f.rect)).collect(),
        job,
    };
    let luminance = |frame: usize, x: i64, y: i64| -> Option<f64> {
        linear(frame, x, y).map(|p| (p[0] + p[1] + p[2]) / 3.0)
    };

    // ---- pass 1: predicted by the host's lens-free maps
    let matches = model.collect(
        capacity,
        |pair, p| {
            let map = &model.grids[pair].map;
            apply(map, p[0], p[1])
        },
        &luminance,
        (0.0, 0.3),
    )?;
    let first = model.solve(0.0, matches, (0.3, 0.5))?;

    // ---- pass 2: predicted through pass 1's lens and corrected maps, so
    // the search reaches the whole overlap however far the lens moved it.
    let k1 = first.coefficient;
    let maps1 = first.pairs;
    drop(first.matches);
    let radial = [k1, 0.0, 0.0];
    let matches = model.collect(
        capacity,
        |pair, p| {
            let grid = &model.grids[pair];
            let (ci, ri) = model.lenses[grid.i];
            let (ca, ra) = model.lenses[grid.anchor];
            let u = radial_unmap(ci, ri, p, radial)?;
            let v = apply(&maps1[pair], u[0], u[1])?;
            let r2 = ((v[0] - ca[0]) / ra).powi(2) + ((v[1] - ca[1]) / ra).powi(2);
            radial_monotone(r2, radial).then(|| radial_map(ca, ra, v, radial, 1.0))
        },
        &luminance,
        (0.5, 0.8),
    )?;
    let solved = model.solve(k1, matches, (0.8, 1.0))?;

    // ---- the maps along the anchors
    let mut maps = vec![[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]; n];
    for (grid, pair) in model.grids.iter().zip(&solved.pairs) {
        let m = product(&maps[grid.anchor], pair);
        if m[8].is_nan() || m[8].abs() <= 1e-300 {
            return Err(TOO_LITTLE_OVERLAP.into());
        }
        maps[grid.i] = m.map(|v| v / m[8]);
    }
    let count = solved.matches.len() as u64;
    Ok(DistortionFit {
        coefficient: solved.coefficient,
        maps,
        matches: count,
        residual: (solved.total / count.max(1) as f64).sqrt(),
    })
}
