//! KERNEL: Auto-Blend Layers ▸ Panorama's seams: which frame each
//! canvas pixel shows, chosen by one minimum cut through each overlap.
//!
//! Free functions over one canvas rectangle. A frame is only a row reader
//! (`focus_stack::ReadRow`, the adapters' reader with Seamless Tones' gains
//! already applied), and the answer has `focus_stack::winners`' contract: a
//! frame index or `NO_WINNER` per canvas pixel, label `k` only where frame
//! `k` has alpha above 0.
//!
//! # The analysis grid
//!
//! The canvas is cut on a grid of square blocks, `analysis_step` pixels a
//! side: the least step whose grid has at most [`ANALYSIS_PIXELS`] blocks.
//! A block's coverage by a frame is the mean alpha over the block's canvas
//! pixels, and its colour the alpha-weighted mean RGB; the frame COVERS the
//! block when that coverage is at least ½. The grid caps the cut's memory
//! and time whatever the canvas, at the price of a seam placed to a block.
//!
//! # The seams, bottom up
//!
//! The composite starts empty. Frames are taken in ascending order (bottom
//! first) in one streamed pass each. With `P` the composite's covered blocks
//! and `K` frame `k`'s: `K ∖ P` takes `k`; the overlap `O = P ∩ K` gets one
//! minimum cut ([`graph_cut`]) over its bounding box grown by one block. The
//! free nodes are `O`; `P ∖ K` is fixed source (keep the composite), `K ∖ P`
//! fixed sink (take `k`), and any other box block fixed source with no link.
//! A sink block takes `k`, and the composite's colour there becomes frame
//! `k`'s.
//!
//! The link between two blocks costs `e(p) + e(q) + ε`, ε = 1/255, so a seam
//! is never free and the shortest one wins on flat ground. `e = D + G`
//! where both cover: `D` the mean absolute RGB difference between the
//! composite's colour and frame `k`'s, `G` the larger luma gradient of the
//! two (luma by the caller's `luma` weights: `blend::LUMA_*` on RGB, L\*
//! alone on a Lab document, `lab_stack::LIGHTNESS_LUMA`; central differences over covered
//! neighbours, one-sided at an edge, the magnitude `hypot`). Where only one
//! covers, `e` is that image's `G`; where none does, 0. So the seam runs
//! where the two frames agree AND neither has an edge: through sky, not
//! through a face or a building's edge.
//!
//! The unary term places a seam where the links leave a choice. With `dP`
//! and `dK` the depth of a block inside `P` and inside `K` (the
//! `style_stroke_distance` chamfer distance to the nearest block outside it
//! in the box, the grid's diagonal when nothing is outside) and λ = 4/255:
//! under [`SeamPreference::Centre`] taking `k` costs `λ·dP/(dP+dK)` and keeping
//! the composite `λ·dK/(dP+dK)`, so on flat ground the seam runs midway
//! through the overlap; `Upper` takes at 0 and keeps at λ, `Lower` the
//! reverse. `Lower` is in force (take2's W): on flat ground the
//! composite keeps the whole overlap and the seam runs along the
//! composite's own edge, as Photoshop's did in that take. λ is four of ε: a
//! seam bends around anything that costs more than four codes a block along
//! it rather than cut through it. Every cost is `round(4096 · x)` as a `u32`.
//!
//! # Full resolution
//!
//! A canvas pixel takes its block's label. A pixel whose label's frame has
//! alpha 0 there, or with no label while a frame covers it, takes the frame
//! of largest alpha there (ties the lower index); none covers →
//! `NO_WINNER`. That pass reads every frame once.
//!
//! # Working set
//!
//! Per grid block: the composite's label and colour (13), frame `k`'s
//! coverage and colour (13), the cut grid (17: fixed side, two unary costs,
//! two links), the energy (4) and the two distance planes with their inputs
//! (10) — [`SEAM_BYTES_PER_GRID_PIXEL`] — plus `graph_cut::cut_bytes`.
//! Per canvas pixel: the labels (1). Plus a few rows.

use crate::focus_stack::{ReadRow, MAX_FRAMES, NO_WINNER};
use crate::graph_cut::{self, CutGrid};
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::style_stroke_distance::chamfer_distance_to_inside;

/// The analysis grid's block cap (module doc): 4 Mi blocks, so a 24 MP
/// frame is cut at full resolution and a 4 × 24 MP strip at 5 pixels.
pub(crate) const ANALYSIS_PIXELS: u64 = 1 << 22;

/// The live planes per grid block (module doc). The admission drive holds
/// the whole edit's plan; a miss there raises a canvas-scaled constant, never
/// this one (`focus_stack`'s records).
const SEAM_BYTES_PER_GRID_PIXEL: u64 = 57;

/// The seam's preference where the links leave a choice (module doc).
/// Centre was the default until a take's W said otherwise; Upper and Lower
/// are the take's other build rows.
#[allow(dead_code)]
pub(crate) enum SeamPreference {
    Centre,
    Upper,
    Lower,
}

/// The preference in force: take2's W is Lower. On two identical flat
/// frames Photoshop's upper frame shows only from the overlap's far edge
/// (its mask starts at x 799 of an 400..800 overlap, share 0.0025), so the
/// composite below keeps the whole overlap where nothing else decides.
const SEAM_PREFERENCE: SeamPreference = SeamPreference::Lower;

/// ε (module doc): a seam is never free.
const EPSILON: f32 = 1.0 / 255.0;

/// λ (module doc): four of ε.
const LAMBDA: f32 = 4.0 / 255.0;

/// A cost's fixed-point scale.
const COST_SCALE: f32 = 4096.0;

/// The least step `s ≥ 1` with `⌈w/s⌉ · ⌈h/s⌉ ≤ ANALYSIS_PIXELS`.
pub(crate) fn analysis_step(size: (u32, u32)) -> u32 {
    let (w, h) = (u64::from(size.0.max(1)), u64::from(size.1.max(1)));
    // Below `√(wh / cap)` the grid cannot fit, so the search starts there.
    let mut s = (((w * h) as f64 / ANALYSIS_PIXELS as f64).sqrt().floor() as u64).max(1);
    while w.div_ceil(s) * h.div_ceil(s) > ANALYSIS_PIXELS {
        s += 1;
    }
    s as u32
}

fn checked(size: (u32, u32), frames: usize) -> Result<(usize, usize, u64), String> {
    if size.0 == 0 || size.1 == 0 {
        return Err("panorama seams need a non-empty canvas".into());
    }
    if frames == 0 || frames > MAX_FRAMES {
        return Err(format!(
            "panorama seams take between 1 and {MAX_FRAMES} frames"
        ));
    }
    let pixels = u64::from(size.0) * u64::from(size.1);
    if usize::try_from(pixels).is_err() {
        return Err("panorama canvas exceeds addressable memory".into());
    }
    Ok((size.0 as usize, size.1 as usize, pixels))
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, String> {
    let mut out = reserved_vec(count)?;
    out.resize(count, value);
    Ok(out)
}

/// `(scratch bytes, work units)` of [`seams`] (module doc, "Working set").
/// The work is two passes a frame over the canvas (the blocks and the full
/// resolution) and one cut a frame over the grid.
pub(crate) fn seams_plan(size: (u32, u32), frames: usize) -> Result<(u64, u64), String> {
    let (_, _, pixels) = checked(size, frames)?;
    let step = u64::from(analysis_step(size));
    let blocks = u64::from(size.0).div_ceil(step) * u64::from(size.1).div_ceil(step);
    let w = u64::from(size.0);
    Ok((
        blocks
            .saturating_mul(SEAM_BYTES_PER_GRID_PIXEL)
            .saturating_add(graph_cut::cut_bytes(blocks))
            .saturating_add(pixels)
            // The full-resolution pass's per-column buffers: `row` 16,
            // `kept` 1 and `best` 8 (an `(f32, u8)` pads to 8) bytes.
            .saturating_add(w.saturating_mul(25))
            .saturating_add(w.div_ceil(step).saturating_mul(32)),
        pixels
            .saturating_mul(2 * frames as u64)
            .saturating_add(blocks.saturating_mul(frames as u64)),
    ))
}

/// The analysis grid's geometry, and the caller's luma weights the gradient
/// reads.
#[derive(Clone, Copy)]
struct Grid {
    step: usize,
    w: usize,
    h: usize,
    gw: usize,
    gh: usize,
    luma: [f32; 3],
}

fn luma(weights: [f32; 3], c: [f32; 3]) -> f32 {
    weights[0] * c[0] + weights[1] * c[1] + weights[2] * c[2]
}

/// One frame's blocks: covered (coverage ≥ ½) and the alpha-weighted mean
/// colour, in one streamed pass.
fn frame_blocks(
    grid: Grid,
    frame: usize,
    read: ReadRow<'_>,
    job: JobContext<'_>,
) -> Result<(Vec<bool>, Vec<[f32; 3]>), String> {
    let n = grid.gw * grid.gh;
    let mut covered = filled(n, false)?;
    let mut colour = filled(n, [0.0f32; 3])?;
    let mut row = filled(grid.w, [0.0f32; 4])?;
    let mut alpha = filled(grid.gw, 0.0f64)?;
    let mut sums = filled(grid.gw, [0.0f64; 3])?;
    for by in 0..grid.gh {
        alpha.fill(0.0);
        sums.fill([0.0; 3]);
        let top = by * grid.step;
        let bottom = (top + grid.step).min(grid.h);
        for y in top..bottom {
            read(frame, y as u32, &mut row);
            for (bx, chunk) in row.chunks(grid.step).enumerate() {
                for p in chunk {
                    let a = f64::from(p[3].clamp(0.0, 1.0));
                    alpha[bx] += a;
                    for c in 0..3 {
                        sums[bx][c] += a * f64::from(p[c]);
                    }
                }
            }
        }
        for bx in 0..grid.gw {
            let width = (grid.w - bx * grid.step).min(grid.step);
            let count = (width * (bottom - top)) as f64;
            let i = by * grid.gw + bx;
            covered[i] = alpha[bx] / count >= 0.5;
            if alpha[bx] > 0.0 {
                colour[i] = sums[bx].map(|s| (s / alpha[bx]) as f32);
            }
        }
        job.report((by + 1) as f64 / grid.gh as f64, 6)?;
    }
    Ok((covered, colour))
}

/// The luma gradient's magnitude at block `i` of an image covering
/// `covers` (module doc): central differences over covered neighbours,
/// one-sided at an edge, 0 along an axis with no covered neighbour.
fn gradient(grid: Grid, colour: &[[f32; 3]], covers: &dyn Fn(usize) -> bool, i: usize) -> f32 {
    let (x, y) = (i % grid.gw, i / grid.gw);
    let l = |j: usize| luma(grid.luma, colour[j]);
    let axis = |before: Option<usize>, after: Option<usize>| {
        let before = before.filter(|&j| covers(j));
        let after = after.filter(|&j| covers(j));
        match (before, after) {
            (Some(b), Some(a)) => (l(a) - l(b)) / 2.0,
            (None, Some(a)) => l(a) - l(i),
            (Some(b), None) => l(i) - l(b),
            (None, None) => 0.0,
        }
    };
    let gx = axis((x > 0).then(|| i - 1), (x + 1 < grid.gw).then(|| i + 1));
    let gy = axis(
        (y > 0).then(|| i - grid.gw),
        (y + 1 < grid.gh).then(|| i + grid.gw),
    );
    gx.hypot(gy)
}

fn cost(x: f32) -> u32 {
    (COST_SCALE * x).round() as u32
}

/// The depth of each box block inside the set `inside` marks: the chamfer
/// distance to the nearest box block outside it, `diagonal` when none is.
fn depth(
    inside: &[bool],
    bw: usize,
    bh: usize,
    diagonal: f32,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let mut outside: Vec<u8> = reserved_vec(inside.len())?;
    outside.extend(inside.iter().map(|&v| if v { 0 } else { 255 }));
    let mut d = chamfer_distance_to_inside(&outside, bw, bh, job)?;
    for v in &mut d {
        if !v.is_finite() {
            *v = diagonal;
        }
    }
    Ok(d)
}

/// One frame's cut against the composite (module doc, "The seams").
fn cut_overlap(
    grid: Grid,
    label: &mut [u8],
    composite: &mut [[f32; 3]],
    frame: u8,
    covered: &[bool],
    colour: &[[f32; 3]],
    job: JobContext<'_>,
) -> Result<(), String> {
    let n = grid.gw * grid.gh;
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for i in 0..n {
        job.every(i)?;
        if covered[i] && label[i] != NO_WINNER {
            let (x, y) = (i % grid.gw, i / grid.gw);
            bounds = Some(bounds.map_or((x, y, x, y), |(x0, y0, x1, y1)| {
                (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
            }));
        }
    }
    let Some((x0, y0, x1, y1)) = bounds else {
        return Ok(());
    };
    let (x0, y0) = (x0.saturating_sub(1), y0.saturating_sub(1));
    let (x1, y1) = ((x1 + 2).min(grid.gw), (y1 + 2).min(grid.gh));
    let (bw, bh) = (x1 - x0, y1 - y0);
    let m = bw * bh;
    let at = |q: usize| (y0 + q / bw) * grid.gw + x0 + q % bw;
    let in_p = |g: usize| label[g] != NO_WINNER;
    let in_k = |g: usize| covered[g];

    let mut energy: Vec<f32> = reserved_vec(m)?;
    for q in 0..m {
        job.every(q)?;
        let g = at(q);
        let gp = || gradient(grid, composite, &in_p, g);
        let gk = || gradient(grid, colour, &in_k, g);
        energy.push(match (in_p(g), in_k(g)) {
            (true, true) => {
                let d = (0..3)
                    .map(|c| (composite[g][c] - colour[g][c]).abs())
                    .sum::<f32>()
                    / 3.0;
                d + gp().max(gk())
            }
            (true, false) => gp(),
            (false, true) => gk(),
            (false, false) => 0.0,
        });
    }
    let diagonal = (grid.gw as f32).hypot(grid.gh as f32);
    let mut inside: Vec<bool> = reserved_vec(m)?;
    inside.extend((0..m).map(|q| in_p(at(q))));
    let d_p = depth(&inside, bw, bh, diagonal, job)?;
    inside.clear();
    inside.extend((0..m).map(|q| in_k(at(q))));
    let d_k = depth(&inside, bw, bh, diagonal, job)?;
    drop(inside);

    let mut cut = CutGrid {
        width: bw,
        height: bh,
        fixed: reserved_vec(m)?,
        unary: reserved_vec(m)?,
        right: filled(m, 0u32)?,
        down: filled(m, 0u32)?,
    };
    for q in 0..m {
        job.every(q)?;
        let g = at(q);
        let (p, k) = (in_p(g), in_k(g));
        cut.fixed.push(match (p, k) {
            (true, true) => None,
            (false, true) => Some(true),
            _ => Some(false),
        });
        let (take, keep) = if p && k {
            match SEAM_PREFERENCE {
                SeamPreference::Centre => {
                    let total = d_p[q] + d_k[q];
                    (LAMBDA * d_p[q] / total, LAMBDA * d_k[q] / total)
                }
                SeamPreference::Upper => (0.0, LAMBDA),
                SeamPreference::Lower => (LAMBDA, 0.0),
            }
        } else {
            (0.0, 0.0)
        };
        cut.unary.push([cost(take), cost(keep)]);
        let linked = |r: usize| (in_p(r) || in_k(r)) && (p || k);
        if q % bw + 1 < bw && linked(at(q + 1)) {
            cut.right[q] = cost(energy[q] + energy[q + 1] + EPSILON);
        }
        if q / bw + 1 < bh && linked(at(q + bw)) {
            cut.down[q] = cost(energy[q] + energy[q + bw] + EPSILON);
        }
    }
    drop((energy, d_p, d_k));
    let sides = graph_cut::min_cut_with_context(&cut, job)?;
    drop(cut);
    for (q, &sink) in sides.iter().enumerate() {
        let g = at(q);
        if sink && in_k(g) {
            label[g] = frame;
            composite[g] = colour[g];
        }
    }
    Ok(())
}

/// The frame each canvas pixel shows (module doc), row-major: a frame
/// index, or `NO_WINNER` where no frame has alpha above 0. `seams[p] == k`
/// implies frame `k` has alpha above 0 at `p`. `luma` weights R, G and B
/// into the gradient's luma (module doc). `Err` for a refused size, a
/// refused allocation or a cancel.
pub(crate) fn seams(
    size: (u32, u32),
    frames: usize,
    read: ReadRow<'_>,
    luma: [f32; 3],
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let (w, h, _) = checked(size, frames)?;
    let step = analysis_step(size) as usize;
    let grid = Grid {
        step,
        w,
        h,
        gw: w.div_ceil(step),
        gh: h.div_ceil(step),
        luma,
    };
    let n = grid.gw * grid.gh;
    let mut label = filled(n, NO_WINNER)?;
    let mut composite = filled(n, [0.0f32; 3])?;
    let share = 1.0 / (frames + 1) as f64;
    for frame in 0..frames {
        let part = job.range(frame as f64 * share, (frame + 1) as f64 * share);
        let (covered, colour) = frame_blocks(grid, frame, read, part.range(0.0, 0.5))?;
        let label_k = frame as u8;
        cut_overlap(
            grid,
            &mut label,
            &mut composite,
            label_k,
            &covered,
            &colour,
            part.range(0.5, 1.0),
        )?;
        for i in 0..n {
            if covered[i] && label[i] == NO_WINNER {
                label[i] = label_k;
                composite[i] = colour[i];
            }
        }
    }
    drop(composite);
    let last = job.range(frames as f64 * share, 1.0);
    let mut out: Vec<u8> = reserved_vec(w * h)?;
    let mut row = filled(w, [0.0f32; 4])?;
    let mut kept = filled(w, false)?;
    let mut best = filled(w, (0.0f32, NO_WINNER))?;
    for y in 0..h {
        let blocks = &label[(y / step) * grid.gw..(y / step + 1) * grid.gw];
        kept.fill(false);
        best.fill((0.0, NO_WINNER));
        for frame in 0..frames {
            read(frame, y as u32, &mut row);
            let k = frame as u8;
            for x in 0..w {
                let a = row[x][3];
                if a > 0.0 {
                    if blocks[x / step] == k {
                        kept[x] = true;
                    }
                    if a > best[x].0 {
                        best[x] = (a, k);
                    }
                }
            }
        }
        out.extend((0..w).map(|x| if kept[x] { blocks[x / step] } else { best[x].1 }));
        last.report((y + 1) as f64 / h as f64, 6)?;
    }
    Ok(out)
}
