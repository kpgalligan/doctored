//! Auto-Align Layers' Vignette Removal (phase 2b-3): one radial falloff
//! fitted to the registered overlaps of a stack, and the per-pixel stops that
//! divide it out. Plain numbers only; the adapters are `doc_align_vignette`
//! and `precision_align_vignette`, which read each layer's linear light
//! through a closure and apply the stops with `sampling_lens::
//! apply_vignette_stops` (Lens Correction's per-pixel gain).
//!
//! THE MODEL (take2's V re-read, `scripts/photoshop-probe/
//! panorama_lens_reread.py`: Photoshop's three frames share one curve). ONE
//! coefficient `a` for the stack and the falloff V(ρ) = `lens_model::
//! pa_falloff([a, 0, 0], ρ²)` = 1 + a·ρ², ρ the distance of a pixel centre
//! from its layer rect's centre over `lens_model::vignette_radius` (so the
//! farthest pixel centre is at ρ = 1). It acts on linear light, the luminance
//! Y being the mean of the three linear channels. Every layer is multiplied
//! by `gain / V(ρ)`, where `gain` is the mean of V over every pixel centre of
//! every listed layer (Photoshop scaled each frame by about its mean falloff,
//! 0.9 on take2). A per-layer log exposure is a nuisance of the fit only and
//! is never applied (that is Seamless Tones' job).
//!
//! THE FIT. Every ordered pair i < j of frames samples a grid over the part
//! of frame i's rect that a 64 × 64 probe finds inside frame j (through
//! `H_j⁻¹·H_i`, each `map` taking a layer's canvas point to the reference's),
//! its spacing set so the pairs share [`MAX_SAMPLES`] in proportion to their
//! overlap area. Each grid pixel and the NEAREST pixel of frame j it maps to
//! (no interpolation: the falloff is smooth and the picture is not) are kept
//! when both are present and both luminances are in (DARK, BRIGHT), and give
//! d = ln Y_i − ln Y_j with ρ_i², ρ_j². For a coefficient `a` the exposures
//! e (the first frame's pinned at 0) are the least-squares solution of
//! d − ln V(ρ_i) + ln V(ρ_j) = e_i − e_j, with a 1e-9 ridge so a frame no
//! sample reaches stays at 0; the objective is the residual. Its normal
//! matrix does not depend on `a`, so it is factored once. `a` is the best of
//! 65 evenly spaced values over [`COEFFICIENT_RANGE`] refined by 50
//! golden-section steps between that value's neighbours
//! (`AutoAlignSphere.focal`'s pattern).
//!
//! WHY THE NUMBERS:
//! - [`COEFFICIENT_RANGE`] −15/16…1: V(1) never falls below
//!   `lens_model`'s 1/16 floor (four stops, the darkest falloff a profile
//!   corrects), and a lens that brightens its corners by more than double
//!   is not a vignette.
//! - [`DARK`] 0.01 and [`BRIGHT`] 0.9 (linear Y): below 1% a code step is a
//!   large share of the value and ln Y is noise; near white the picture
//!   clips, so its ratio no longer follows the falloff.
//! - [`MAX_SAMPLES`] 2^18: each sample is one ratio of two pixels, so a
//!   quarter-million of them measure a smooth two-parameter curve far below
//!   a code, while the fit stays a few MiB and a fraction of a second.
//! - [`MIN_SAMPLES`] 1,000: fewer ratios than that is an overlap of a few
//!   hundred pixels or a picture that is all clipped or black, too little to
//!   tell a falloff from the scene.
//! - The 1e-9 ridge: the normal matrix's entries are sample counts (at least
//!   1), so 1e-9 moves a solved exposure by far less than any sample's
//!   rounding, and it keeps a frame with no samples from making the matrix
//!   singular.
use crate::job_control::JobContext;
use crate::lens_model::{pa_falloff, vignette_radius};
use crate::puppet_mesh::Rect;

/// Every pair's samples together; see the module doc.
pub(crate) const MAX_SAMPLES: usize = 1 << 18;
/// The fewest kept samples the fit measures from; see the module doc.
pub(crate) const MIN_SAMPLES: usize = 1_000;
/// The coefficients the fit searches; see the module doc.
pub(crate) const COEFFICIENT_RANGE: (f64, f64) = (-15.0 / 16.0, 1.0);
/// The darkest linear luminance a sample may have; see the module doc.
pub(crate) const DARK: f64 = 0.01;
/// The brightest linear luminance a sample may have; see the module doc.
pub(crate) const BRIGHT: f64 = 0.9;
/// The evenly spaced coefficients tried before the golden-section search.
const GRID: usize = 65;
/// The golden-section steps: the bracket shrinks by 0.618 each, so 50 take
/// the 0.06-wide starting bracket below 1e-11.
const GOLDEN_STEPS: usize = 50;
/// The exposure solve's ridge; see the module doc.
const RIDGE: f64 = 1e-9;
/// The overlap probe's cells per side.
const PROBE: u32 = 64;

/// The refusal when the kept samples are fewer than [`MIN_SAMPLES`].
pub(crate) const TOO_LITTLE_OVERLAP: &str = "Vignette Removal found too little overlap between \
     the layers to measure the vignette. Nothing was changed.";

/// One kept ratio: the two frames, d = ln Y_i − ln Y_j, and ρ_i², ρ_j².
#[derive(Clone, Copy)]
struct Sample {
    i: u32,
    j: u32,
    d: f64,
    ri: f64,
    rj: f64,
}

/// The fit's sample list at its largest: [`MAX_SAMPLES`] records of 32
/// bytes (two u32 frame indices and three f64), 8 MiB. The normal matrix,
/// which grows with the layer count, is [`system_bytes`].
pub(crate) const FIT_SCRATCH_BYTES: u64 = MAX_SAMPLES as u64 * std::mem::size_of::<Sample>() as u64;

/// The exposure system's bytes for `frames` frames: the normal matrix and
/// its Cholesky factor (n² f64 each) and three vectors of n.
pub(crate) fn system_bytes(frames: usize) -> u64 {
    let n = frames as u64;
    n.saturating_mul(n)
        .saturating_mul(16)
        .saturating_add(n.saturating_mul(24))
}

/// One layer of the fit: its canvas rect and its map (row-major, the layer's
/// canvas point to the reference's).
pub(crate) struct VignetteFrame {
    pub rect: Rect,
    pub map: [f64; 9],
}

/// The fitted falloff: `coefficient` is `a`, `gain` the mean of V over every
/// pixel centre of every frame, `samples` the ratios kept.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VignetteFit {
    pub coefficient: f64,
    pub gain: f64,
    pub samples: u64,
}

/// A frame's radial geometry: its centre in canvas pixels and 1/R².
#[derive(Clone, Copy)]
pub(crate) struct Radial {
    centre: [f64; 2],
    inverse_r2: f64,
}

impl Radial {
    pub(crate) fn of(rect: Rect) -> Self {
        let (w, h) = (rect.2, rect.3);
        let local = [f64::from(w) / 2.0, f64::from(h) / 2.0];
        let radius = vignette_radius(local, w, h);
        Self {
            centre: [rect.0 as f64 + local[0], rect.1 as f64 + local[1]],
            inverse_r2: if radius > 0.0 {
                1.0 / (radius * radius)
            } else {
                0.0
            },
        }
    }

    /// ρ² of the canvas point (x, y).
    pub(crate) fn rho2(&self, x: f64, y: f64) -> f64 {
        let (dx, dy) = (x - self.centre[0], y - self.centre[1]);
        (dx * dx + dy * dy) * self.inverse_r2
    }

    /// The correction in stops at the canvas point (x, y) of this frame's
    /// layer: log2(gain) − log2 V(ρ), V held at `lens_model`'s 1/16 floor
    /// (which [`COEFFICIENT_RANGE`] already keeps it above at every pixel
    /// centre).
    pub(crate) fn stops(&self, fit: &VignetteFit, x: f64, y: f64) -> f32 {
        let falloff = pa_falloff([fit.coefficient, 0.0, 0.0], self.rho2(x, y));
        (fit.gain.log2() - falloff.max(1.0 / 16.0).log2()) as f32
    }
}

pub(crate) fn product(a: &[f64; 9], b: &[f64; 9]) -> [f64; 9] {
    let mut out = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    out
}

pub(crate) fn invert(m: &[f64; 9]) -> Option<[f64; 9]> {
    let cofactor = [
        m[4] * m[8] - m[5] * m[7],
        m[2] * m[7] - m[1] * m[8],
        m[1] * m[5] - m[2] * m[4],
        m[5] * m[6] - m[3] * m[8],
        m[0] * m[8] - m[2] * m[6],
        m[2] * m[3] - m[0] * m[5],
        m[3] * m[7] - m[4] * m[6],
        m[1] * m[6] - m[0] * m[7],
        m[0] * m[4] - m[1] * m[3],
    ];
    let det = m[0] * cofactor[0] + m[1] * cofactor[3] + m[2] * cofactor[6];
    if !det.is_finite() || det == 0.0 {
        return None;
    }
    let out = cofactor.map(|v| v / det);
    out.iter().all(|v| v.is_finite()).then_some(out)
}

pub(crate) fn apply(m: &[f64; 9], x: f64, y: f64) -> Option<[f64; 2]> {
    let w = m[6] * x + m[7] * y + m[8];
    if !w.is_finite() || w <= 0.0 {
        return None;
    }
    let p = [
        (m[0] * x + m[1] * y + m[2]) / w,
        (m[3] * x + m[4] * y + m[5]) / w,
    ];
    p.iter().all(|v| v.is_finite()).then_some(p)
}

fn inside(rect: Rect, p: [f64; 2]) -> bool {
    let (x0, y0) = (rect.0 as f64, rect.1 as f64);
    p[0] >= x0 && p[1] >= y0 && p[0] <= x0 + f64::from(rect.2) && p[1] <= y0 + f64::from(rect.3)
}

/// One pair's sampling grid: the pixel box of frame i it walks and its
/// counts per side.
struct PairGrid {
    i: usize,
    j: usize,
    map: [f64; 9],
    origin: [i64; 2],
    size: [f64; 2],
    count: [usize; 2],
}

/// The part of frame `i`'s rect the probe finds inside frame `j` through
/// `map`: its pixel box (x0, y0, w, h), a cell beyond the outermost hit on
/// each side and clamped to the rect, and its estimated area in pixels.
pub(crate) fn overlap(from: Rect, to: Rect, map: &[f64; 9]) -> Option<([i64; 2], [f64; 2], f64)> {
    let (x0, y0, w, h) = from;
    if w == 0 || h == 0 {
        return None;
    }
    let (nx, ny) = (PROBE.min(w), PROBE.min(h));
    let (cw, ch) = (f64::from(w) / f64::from(nx), f64::from(h) / f64::from(ny));
    let (mut hits, mut low, mut high) = (0u64, [u32::MAX; 2], [0u32; 2]);
    for gy in 0..ny {
        for gx in 0..nx {
            let x = x0 as f64 + (f64::from(gx) + 0.5) * cw;
            let y = y0 as f64 + (f64::from(gy) + 0.5) * ch;
            if apply(map, x, y).is_some_and(|q| inside(to, q)) {
                hits += 1;
                low = [low[0].min(gx), low[1].min(gy)];
                high = [high[0].max(gx), high[1].max(gy)];
            }
        }
    }
    if hits == 0 {
        return None;
    }
    let first = [low[0].saturating_sub(1), low[1].saturating_sub(1)];
    let last = [(high[0] + 2).min(nx), (high[1] + 2).min(ny)];
    let left = (f64::from(first[0]) * cw).floor();
    let top = (f64::from(first[1]) * ch).floor();
    let right = (f64::from(last[0]) * cw).ceil().min(f64::from(w));
    let bottom = (f64::from(last[1]) * ch).ceil().min(f64::from(h));
    let area = hits as f64 / f64::from(nx * ny) * f64::from(w) * f64::from(h);
    Some((
        [x0 + left as i64, y0 + top as i64],
        [right - left, bottom - top],
        area,
    ))
}

/// The Cholesky factor (lower, row-major) of the symmetric positive
/// definite `m × m` matrix `a`, or `None` when a pivot is not positive.
fn cholesky(a: &[f64], m: usize) -> Option<Vec<f64>> {
    let mut l = vec![0.0; m * m];
    for i in 0..m {
        for j in 0..=i {
            let mut sum = a[i * m + j];
            for k in 0..j {
                sum -= l[i * m + k] * l[j * m + k];
            }
            if i == j {
                if sum.is_nan() || sum <= 0.0 {
                    return None;
                }
                l[i * m + i] = sum.sqrt();
            } else {
                l[i * m + j] = sum / l[j * m + j];
            }
        }
    }
    Some(l)
}

/// Solves L·Lᵀ·x = b in place.
fn solve(l: &[f64], m: usize, b: &mut [f64]) {
    for i in 0..m {
        let mut sum = b[i];
        for k in 0..i {
            sum -= l[i * m + k] * b[k];
        }
        b[i] = sum / l[i * m + i];
    }
    for i in (0..m).rev() {
        let mut sum = b[i];
        for k in i + 1..m {
            sum -= l[k * m + i] * b[k];
        }
        b[i] = sum / l[i * m + i];
    }
}

/// The fit (module doc). `linear(frame, x, y)` is frame `frame`'s pixel at
/// canvas pixel (x, y) in linear light, or `None` outside its rect or below
/// full alpha. Refuses [`TOO_LITTLE_OVERLAP`] below [`MIN_SAMPLES`].
pub(crate) fn fit(
    frames: &[VignetteFrame],
    linear: impl Fn(usize, i64, i64) -> Option<[f64; 3]>,
    job: JobContext<'_>,
) -> Result<VignetteFit, String> {
    let n = frames.len();
    if n < 2 {
        return Err(TOO_LITTLE_OVERLAP.into());
    }
    let radial: Vec<Radial> = frames.iter().map(|f| Radial::of(f.rect)).collect();
    let inverses: Vec<Option<[f64; 9]>> = frames.iter().map(|f| invert(&f.map)).collect();
    let mut found = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            job.check()?;
            let Some(inverse) = &inverses[j] else {
                continue;
            };
            let map = product(inverse, &frames[i].map);
            if let Some((origin, size, area)) = overlap(frames[i].rect, frames[j].rect, &map) {
                found.push((i, j, map, origin, size, area));
            }
        }
    }
    let total: f64 = found.iter().map(|f| f.5).sum();
    let mut grids = Vec::new();
    let mut capacity = 0usize;
    for (i, j, map, origin, size, area) in found {
        let budget = (MAX_SAMPLES as f64 * area / total).floor();
        if budget.is_nan() || budget < 1.0 {
            continue;
        }
        let spacing = (size[0] * size[1] / budget).sqrt().max(1.0);
        let nx = ((size[0] / spacing).floor() as usize).clamp(1, size[0] as usize);
        let ny = ((size[1] / spacing).floor() as usize)
            .clamp(1, size[1] as usize)
            .min((budget as usize / nx).max(1));
        capacity += nx * ny;
        grids.push(PairGrid {
            i,
            j,
            map,
            origin,
            size,
            count: [nx, ny],
        });
    }
    let mut samples: Vec<Sample> = Vec::new();
    samples
        .try_reserve_exact(capacity.min(MAX_SAMPLES))
        .map_err(|_| "could not allocate the vignette fit".to_string())?;
    let mut walked = 0usize;
    'pairs: for grid in &grids {
        let [nx, ny] = grid.count;
        for gy in 0..ny {
            let py = grid.origin[1] + ((gy as f64 + 0.5) * grid.size[1] / ny as f64) as i64;
            for gx in 0..nx {
                job.every(walked)?;
                walked += 1;
                if samples.len() == samples.capacity() {
                    break 'pairs;
                }
                let px = grid.origin[0] + ((gx as f64 + 0.5) * grid.size[0] / nx as f64) as i64;
                let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
                let Some(q) = apply(&grid.map, cx, cy) else {
                    continue;
                };
                let (qx, qy) = (q[0].floor(), q[1].floor());
                if qx.abs() > 9e15 || qy.abs() > 9e15 {
                    continue;
                }
                let (qx, qy) = (qx as i64, qy as i64);
                let Some(a) = linear(grid.i, px, py) else {
                    continue;
                };
                let Some(b) = linear(grid.j, qx, qy) else {
                    continue;
                };
                let ya = (a[0] + a[1] + a[2]) / 3.0;
                let yb = (b[0] + b[1] + b[2]) / 3.0;
                if !(ya > DARK && ya < BRIGHT && yb > DARK && yb < BRIGHT) {
                    continue;
                }
                samples.push(Sample {
                    i: grid.i as u32,
                    j: grid.j as u32,
                    d: ya.ln() - yb.ln(),
                    ri: radial[grid.i].rho2(cx, cy),
                    rj: radial[grid.j].rho2(qx as f64 + 0.5, qy as f64 + 0.5),
                });
            }
        }
    }
    if samples.len() < MIN_SAMPLES {
        return Err(TOO_LITTLE_OVERLAP.into());
    }
    job.report(0.5, 2)?;

    // The normal matrix of the exposures, the first frame's removed.
    let m = n - 1;
    let mut normal = vec![0.0; m * m];
    for s in &samples {
        let (i, j) = (s.i as usize, s.j as usize);
        if i > 0 {
            normal[(i - 1) * m + i - 1] += 1.0;
        }
        if j > 0 {
            normal[(j - 1) * m + j - 1] += 1.0;
        }
        if i > 0 && j > 0 {
            normal[(i - 1) * m + j - 1] -= 1.0;
            normal[(j - 1) * m + i - 1] -= 1.0;
        }
    }
    let laplacian = normal.clone();
    for k in 0..m {
        normal[k * m + k] += RIDGE;
    }
    let factor = cholesky(&normal, m).ok_or("the vignette fit's exposure system is singular")?;
    drop(normal);

    // The residual Σ(t − e_i + e_j)² expanded: Σt² − 2·e·B + eᵀ·L·e, with
    // B the normal equations' right side; e (first frame 0) solves them.
    let objective = |a: f64| -> Result<f64, String> {
        let k = [a, 0.0, 0.0];
        let mut squares = 0.0;
        let mut b = vec![0.0; n];
        for (index, s) in samples.iter().enumerate() {
            job.every(index)?;
            let t = s.d - pa_falloff(k, s.ri).ln() + pa_falloff(k, s.rj).ln();
            squares += t * t;
            b[s.i as usize] += t;
            b[s.j as usize] -= t;
        }
        let right = b[1..].to_vec();
        let mut e = right.clone();
        solve(&factor, m, &mut e);
        let along: f64 = e.iter().zip(&right).map(|(x, y)| x * y).sum();
        let mut quadratic = 0.0;
        for r in 0..m {
            let row: f64 = (0..m).map(|c| laplacian[r * m + c] * e[c]).sum();
            quadratic += e[r] * row;
        }
        Ok(squares - 2.0 * along + quadratic)
    };
    let (low, high) = COEFFICIENT_RANGE;
    let values: Vec<f64> = (0..GRID)
        .map(|k| low + (high - low) * k as f64 / (GRID - 1) as f64)
        .collect();
    let mut scores = Vec::with_capacity(GRID);
    for (k, &a) in values.iter().enumerate() {
        scores.push(objective(a)?);
        job.report(0.5 + 0.25 * (k + 1) as f64 / GRID as f64, 2)?;
    }
    let best = (0..GRID)
        .min_by(|&x, &y| scores[x].total_cmp(&scores[y]))
        .unwrap_or(0);
    let (mut a, mut b) = (
        values[best.saturating_sub(1)],
        values[(best + 1).min(GRID - 1)],
    );
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let (mut c, mut d) = (b - ratio * (b - a), a + ratio * (b - a));
    let (mut ec, mut ed) = (objective(c)?, objective(d)?);
    for step in 0..GOLDEN_STEPS {
        if ec < ed {
            b = d;
            d = c;
            ed = ec;
            c = b - ratio * (b - a);
            ec = objective(c)?;
        } else {
            a = c;
            c = d;
            ec = ed;
            d = a + ratio * (b - a);
            ed = objective(d)?;
        }
        job.report(0.75 + 0.25 * (step + 1) as f64 / GOLDEN_STEPS as f64, 2)?;
    }
    let coefficient = (a + b) / 2.0;

    // The mean of V over every pixel centre: V is affine in ρ², so it is V
    // at each frame's mean ρ², whose closed form is ((w² − 1)/12 +
    // (h² − 1)/12)/R² (the variance of w evenly spaced centres is
    // (w² − 1)/12).
    let (mut weighted, mut pixels) = (0.0, 0.0);
    for (frame, geometry) in frames.iter().zip(&radial) {
        let (w, h) = (f64::from(frame.rect.2), f64::from(frame.rect.3));
        let mean_r2 = ((w * w - 1.0) / 12.0 + (h * h - 1.0) / 12.0) * geometry.inverse_r2;
        weighted += w * h * pa_falloff([coefficient, 0.0, 0.0], mean_r2);
        pixels += w * h;
    }
    let gain = if pixels > 0.0 { weighted / pixels } else { 1.0 };
    Ok(VignetteFit {
        coefficient,
        gain,
        samples: samples.len() as u64,
    })
}
