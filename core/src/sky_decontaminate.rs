//! Sky Replacement's edge decontamination: the old sky's colour taken
//! out of the horizon's edge pixels before they are composited over the new
//! sky. Without it Fade Edge lets the source's partly-sky pixels through at
//! partial coverage, and a bright old sky shows as a light halo along the
//! horizon of a dark replacement. Free functions and one value type over a
//! canvas-sized picture; `sky_harmonize::prepare` builds it, and both
//! outputs and the preview read the decontaminated colour through
//! `Prepared::foreground`, so the New Layers group's "Foreground Edge" child
//! (`sky_group::edge_planes`) and the Duplicate Layer pixel never disagree.
//!
//! # The model
//!
//! An edge pixel's source colour is the mix `C = α·F + (1 − α)·B` of the
//! foreground `F` and the old sky `B`, with `α = 1 − m0` from the sky
//! model's coverage `m0`. Two coarse grids of [`CELLS_PER_SIDE`]² cells at
//! most hold the alpha-weighted sums of the source's PURE sky samples
//! (`m0 ≥` [`PURE`]) and PURE foreground samples (`m0 ≤ 1 −` [`PURE`]). At a
//! pixel the bilinear tent over the four nearest cells estimates `B` (and the
//! nearby foreground); the solve is `F_s = clamp((C − (1 − α)·B)/α, 0, 1)`.
//! Below [`SOLVE_FLOOR`] the solve blends toward the nearby foreground
//! estimate (see the constants).
//!
//! # The band
//!
//! [`Decontamination::in_band`] is where the solve applies: model coverage
//! above `1 − PURE` (the exact complement of the foreground-sample test, so
//! a pixel counted as a foreground sample is never solved and never enters
//! the edge layer's mask), a refined mask `m` strictly between 0 and 1 (a
//! pixel the refined mask fully excludes — the strip a negative Shift Edge
//! hands back to the source — keeps its own colour, which is what Shift Edge
//! is for; one it fully covers shows only the sky) and a visible pixel.
//!
//! Every value is display-encoded RGB in 0…1, like `sky_harmonize`'s.

use crate::job_control::JobContext;

/// The coverage a sample must reach to count as pure sky (`m0 ≥ PURE`), or
/// stay under to count as pure foreground (`m0 ≤ 1 − PURE`). The model's
/// coverage is soft for a few pixels on each side of the horizon; 2 % keeps
/// those mixed pixels out of both estimates, so a cell's sky mean is the old
/// sky and not a blend that already carries the foreground, while still
/// counting the near-saturated interior of each region (the model rarely
/// reports exactly 0 or 65535 over a large area).
pub(crate) const PURE: f32 = 0.98;
/// The foreground fraction `α` below which the solve is blended toward the
/// nearby foreground estimate (weight `α / SOLVE_FLOOR`). The solve divides
/// by `α`, so at small `α` it multiplies the source's quantization (and any
/// error in the sky estimate) by `1/α`: at `α = 0.25` one code of error is
/// four. Below a quarter the pixel is mostly sky, and the colour that keeps
/// no halo is the foreground's own — the Decontaminate Colors rule of
/// taking nearby foreground colour at the edge. Pure old-sky pixels the fade
/// lets through (`m0 = 1`, `0 < m < 1`) have `α = 0` and no foreground to
/// solve, so they take that estimate whole.
pub(crate) const SOLVE_FLOOR: f32 = 0.25;
/// The grids' cells along the picture's long side. The estimate must follow
/// the old sky's gradient (a sunset is brighter near the horizon than at the
/// zenith) without being so local that one cell holds no pure sample: 64
/// cells give 94-pixel cells at 6000 pixels, a few edge widths
/// (`sky_harmonize::edge_radii`'s `R` is 64 there), and at most 4096 cells a
/// grid (256 KiB both, inside `sky_harmonize`'s bookkeeping term, so no plan
/// term). A cell is at least 2 pixels; a reduced preview gets fewer pixels a
/// cell, so it frames the cells as the commit does.
pub(crate) const CELLS_PER_SIDE: u32 = 64;

/// The two estimate grids over one picture (module doc).
pub(crate) struct Decontamination {
    dims: (u32, u32),
    /// Pixels per cell side.
    cell: u32,
    cols: u32,
    rows: u32,
    /// Per cell `[Σa·R, Σa·G, Σa·B, Σa]` of the pure sky samples.
    sky: Vec<[f64; 4]>,
    /// The same of the pure foreground samples.
    fore: Vec<[f64; 4]>,
    /// The whole picture's sums of each grid, the fallback where the four
    /// nearest cells hold no weight.
    sky_total: [f64; 4],
    fore_total: [f64; 4],
}

/// `m0` over 65535, the one form both the sample test and the band use.
fn unit(m0: u16) -> f32 {
    f32::from(m0) / 65535.0
}

impl Decontamination {
    /// One pass over `m0` (`dimensions` sized), reading each sample through
    /// `read`; cancellable through `job`.
    pub(crate) fn new(
        read: impl Fn(usize) -> [f32; 4],
        m0: &[u16],
        dimensions: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<Decontamination, String> {
        let (w, h) = (dimensions.0.max(1), dimensions.1.max(1));
        let long = w.max(h);
        let cell = long.div_ceil(CELLS_PER_SIDE).max(2);
        let (cols, rows) = (w.div_ceil(cell), h.div_ceil(cell));
        let cells = cols as usize * rows as usize;
        let mut sky = vec![[0.0f64; 4]; cells];
        let mut fore = vec![[0.0f64; 4]; cells];
        let width = dimensions.0 as usize;
        for (i, &c) in m0.iter().enumerate() {
            job.every(i)?;
            let coverage = unit(c);
            let grid = if coverage >= PURE {
                &mut sky
            } else if coverage <= 1.0 - PURE {
                &mut fore
            } else {
                continue;
            };
            let f = read(i);
            let a = f64::from(f[3]);
            if a <= 0.0 {
                continue;
            }
            let (x, y) = ((i % width) as u32, (i / width) as u32);
            let at = (y / cell) as usize * cols as usize + (x / cell) as usize;
            let sums = &mut grid[at];
            for ch in 0..3 {
                sums[ch] += a * f64::from(f[ch]);
            }
            sums[3] += a;
        }
        let total = |grid: &[[f64; 4]]| {
            grid.iter().fold([0.0f64; 4], |mut t, s| {
                for ch in 0..4 {
                    t[ch] += s[ch];
                }
                t
            })
        };
        let (sky_total, fore_total) = (total(&sky), total(&fore));
        Ok(Decontamination {
            dims: dimensions,
            cell,
            cols,
            rows,
            sky,
            fore,
            sky_total,
            fore_total,
        })
    }

    /// The picture's width, for callers holding a flat index.
    pub(crate) fn width(&self) -> usize {
        self.dims.0 as usize
    }

    /// Whether the solve applies at a pixel of model coverage `m0`, refined
    /// mask `m` and alpha `alpha` (module doc, "The band").
    pub(crate) fn in_band(m0: u16, m: u16, alpha: f32) -> bool {
        unit(m0) > 1.0 - PURE && m > 0 && m < 65535 && alpha > 0.0
    }

    /// One grid's estimate at pixel `(x, y)`: the bilinear tent over the four
    /// cells nearest the pixel centre (cell centres at `(k + 0.5)·cell`,
    /// indices clamped into the grid), `Σt·rgb / Σt·a`; the grid's whole
    /// mean where those cells hold no weight; `None` for an empty grid.
    fn estimate(&self, grid: &[[f64; 4]], total: &[f64; 4], x: u32, y: u32) -> Option<[f64; 3]> {
        if total[3] <= 0.0 {
            return None;
        }
        let axis = |p: u32, count: u32| {
            let u = (f64::from(p) + 0.5) / f64::from(self.cell) - 0.5;
            let base = u.floor();
            let frac = u - base;
            let last = i64::from(count) - 1;
            let at = |k: i64| k.clamp(0, last) as usize;
            [(at(base as i64), 1.0 - frac), (at(base as i64 + 1), frac)]
        };
        let (us, vs) = (axis(x, self.cols), axis(y, self.rows));
        let mut sum = [0.0f64; 4];
        for (row, tv) in vs {
            for (col, tu) in us {
                let t = tu * tv;
                let cell = &grid[row * self.cols as usize + col];
                for ch in 0..4 {
                    sum[ch] += t * cell[ch];
                }
            }
        }
        let from = if sum[3] > 0.0 { &sum } else { total };
        Some([from[0] / from[3], from[1] / from[3], from[2] / from[3]])
    }

    /// The decontaminated colour of source pixel `fg` at `(x, y)` with model
    /// coverage `m0` and refined mask `m`; `fg` itself outside the band. The
    /// alpha is the source's.
    pub(crate) fn foreground(&self, x: u32, y: u32, fg: [f32; 4], m0: u16, m: u16) -> [f32; 4] {
        if !Self::in_band(m0, m, fg[3]) {
            return fg;
        }
        let alpha = 1.0 - f64::from(unit(m0));
        // Where the model says the pixel is a mix, invert that mix: the old
        // sky's share `(1 − α)·B` comes off and the rest is scaled back up.
        let solved: [f64; 3] = match self.estimate(&self.sky, &self.sky_total, x, y) {
            Some(b) if alpha > 0.0 => std::array::from_fn(|ch| {
                ((f64::from(fg[ch]) - (1.0 - alpha) * b[ch]) / alpha).clamp(0.0, 1.0)
            }),
            _ => std::array::from_fn(|ch| f64::from(fg[ch])),
        };
        // Below the floor the 1/α gain amplifies quantization, so the solve
        // hands over to the nearby foreground (module doc, `SOLVE_FLOOR`).
        let w = (alpha / f64::from(SOLVE_FLOOR)).min(1.0);
        let partner = self
            .estimate(&self.fore, &self.fore_total, x, y)
            .unwrap_or_else(|| std::array::from_fn(|ch| f64::from(fg[ch])));
        let mut out = [0.0f32; 4];
        for ch in 0..3 {
            out[ch] = (w * solved[ch] + (1.0 - w) * partner[ch]) as f32;
        }
        out[3] = fg[3];
        out
    }
}
