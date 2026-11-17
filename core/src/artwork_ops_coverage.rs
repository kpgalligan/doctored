//! KERNEL: the analytic area coverage of a path whose subpaths fold by HELD
//! OPERATIONS, one `path_geometry::SubpathOp` per subpath
//! (`designs/v1-paths-held-subpaths.md`, HS-2). A path without operations is
//! `artwork_coverage::rasterize`'s, whose samples and cost this module never
//! touches.
//!
//! THE FOLD. Each subpath's own region is taken under the fill rule on its
//! OWN winding (nonzero: not zero; even-odd: odd), and the regions fold in
//! order: union, difference, intersection, symmetric difference
//! (`r ∨ s`, `r ∧ ¬s`, `r ∧ s`, `r ⊕ s`). The fold starts from everything
//! when the first subpath subtracts, otherwise from nothing, so a first unite
//! is that subpath's region. The subpath index is the moves seen before an
//! edge, less one (`artwork_coverage::outline_by_subpath`), never the loop
//! arithmetic of `PathShape::commands`.
//!
//! THE WALK. The edges are swept by row exactly as `rasterize` sweeps them
//! (sorted by top, an active list kept in `min y` order). Each row's
//! breakpoints are `artwork_coverage::edge_breaks` and the pair stage
//! (`artwork_pair_sweep::pair_breaks`), sorted and deduplicated; per interval
//! between two breakpoints a spanning list gives each crossing `(x,
//! direction, subpath)`, sorted by x. Between two consecutive crossings no
//! subpath's winding changes, so every subpath is wholly inside or outside on
//! the run and so is the fold; the run is spanned (`artwork_coverage::span`)
//! where the fold holds, the run left of the first crossing and right of the
//! last included. The folded region within an interval is a union of runs,
//! integrated exactly as the single winding is, and a shared edge bounds an
//! empty run, so two subpaths sharing an edge leave no seam. The sort and the
//! run walk duplicate `artwork_coverage::accumulate_interval`, the master
//! copy, with one winding per subpath in place of one sum.
//!
//! ONE CLUSTER PER ROW. `rasterize`'s x-clusters rest on a loop adding
//! winding only inside its x-extent and the row's area being the SUM of the
//! clusters'. A fold is not a sum: an intersect clears what lies outside its
//! subpath, and a first subtract covers the whole row. So each row here runs
//! as ONE cluster, the reference body's shape.
//!
//! ROWS A SUBPATH MISSES. A subpath with no active edge in a row is outside
//! everywhere on it, so it folds as a constant: a unite, subtract or exclude
//! of nothing changes nothing, and an intersect with nothing clears everything
//! before it. A row therefore starts from `false` after its LAST absent
//! intersect (or from the fold's start when there is none) and visits only
//! the present subpaths after it. Finding that intersect walks the intersects
//! back from the last, past present ones only, so it costs at most the row's
//! present subpaths plus one, under the row's charged active edges. A row with
//! no active edge, and every row above the outline's first and below its
//! last, is the fold of every subpath outside: 65535 when the first subpath
//! subtracts and no subpath intersects, otherwise 0.
//!
//! THE CHARGE (`artwork_coverage::charge`, `MAX_SCAN_WORK`, its refusal
//! sentence): the window's pixels up front, each visited row's active edges,
//! `edge_breaks`' and `pair_breaks`' own charges, and per interval
//! `|spanning| + runs × present` (`runs` the crossings plus one, `present`
//! the row's present subpaths: the windings reset and the fold per run). The
//! present list's sort is uncharged, at most `A·log₂A` comparisons under the
//! row's charged `A` active edges. Polling: a check per row and per interval,
//! the per-edge polls of the two break stages. More moves than
//! `path_geometry::MAX_SUBPATHS`, or an operation count other than the moves,
//! refuses once the outline is walked, before the scan charges or allocates.
//!
//! SCRATCH, per edge of the flattened outline (at most `MAX_PATH_EDGES`),
//! against `ffi_artwork::dr_artwork_path_coverage`'s 196 bytes, the figure
//! `path_geometry::coverage_scratch` reserves: the edges 32, their subpath
//! tags 4, the top-order permutation 4, the active edges 32 and their tags 4,
//! the spanning list 4, the breakpoints 8, the crossings 16 (an `f64`, an
//! `i32` and a `u32`), and the pair stage's open, full and merge lists 12 and
//! its x at the row's top and bottom 16: 132 bytes. No cluster buffer is
//! allocated. Per subpath (at most 4,096) a row stamp, a winding, a present
//! entry and an intersect entry, 16 bytes, 64 KiB in all, inside the growth
//! margin; per column the two accumulation planes, 16 bytes of the 24
//! reserved.

use crate::artwork_coverage::{
    charge, edge_breaks, emit_row, max_y, min_y, outline_by_subpath, span, x_at, Edge,
};
use crate::artwork_pair_sweep::{pair_breaks, PairScratch};
use crate::job_control::JobContext;
use crate::path_geometry::{too_many_subpaths, SubpathOp, MAX_SUBPATHS};
use crate::precision_image::{pixel_count, reserved_vec};

/// The refusal of an operation count other than the subpaths'.
const MISMATCH: &str = "artwork path operations do not match its subpaths";

/// Whether a winding is inside under the fill rule.
fn inside(winding: i32, even_odd: bool) -> bool {
    if even_odd {
        winding % 2 != 0
    } else {
        winding != 0
    }
}

/// One subpath's region (`own`) folded into the regions before it.
fn apply(op: SubpathOp, folded: bool, own: bool) -> bool {
    match op {
        SubpathOp::Unite => folded || own,
        SubpathOp::Subtract => folded && !own,
        SubpathOp::Intersect => folded && own,
        SubpathOp::Exclude => folded != own,
    }
}

/// The fold's fixed facts: the operations, the fill rule, where it starts,
/// the intersecting subpaths in order, and its value with every subpath
/// outside.
struct Fold<'a> {
    ops: &'a [SubpathOp],
    even_odd: bool,
    start: bool,
    intersects: Vec<u32>,
    outside: bool,
}

impl<'a> Fold<'a> {
    fn new(ops: &'a [SubpathOp], even_odd: bool) -> Self {
        let start = ops.first() == Some(&SubpathOp::Subtract);
        let intersects: Vec<u32> = (0..ops.len() as u32)
            .filter(|&i| ops[i as usize] == SubpathOp::Intersect)
            .collect();
        let outside = ops.iter().fold(start, |r, &op| apply(op, r, false));
        Self {
            ops,
            even_odd,
            start,
            intersects,
            outside,
        }
    }

    /// The fold on one run: `base` folded with each `visited` subpath's
    /// region at its current winding.
    fn at(&self, base: bool, visited: &[u32], windings: &[i32]) -> bool {
        visited.iter().fold(base, |r, &s| {
            let own = inside(windings[s as usize], self.even_odd);
            apply(self.ops[s as usize], r, own)
        })
    }
}

/// The scan's working buffers, reused across rows (module doc, SCRATCH).
struct Scratch {
    active: Vec<Edge>,
    tags: Vec<u32>,
    seen: Vec<u32>,
    present: Vec<u32>,
    windings: Vec<i32>,
    pairs: PairScratch,
    spanning: Vec<u32>,
    breaks: Vec<f64>,
    crossings: Vec<(f64, i32, u32)>,
    ends: Vec<f64>,
    fills: Vec<f64>,
}

/// One row with active edges: `scratch.active` holds exactly the edges
/// overlapping `[row, row + 1)`, SORTED BY `min y`, and `scratch.tags` each
/// one's subpath. Appends `width` samples to `output` (module doc).
fn row_coverage(
    row: usize,
    width: u32,
    fold: &Fold<'_>,
    scratch: &mut Scratch,
    output: &mut Vec<u16>,
    work: &mut u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let w = width as usize;
    let top = row as f64;
    let bottom = top + 1.0;
    let Scratch {
        active,
        tags,
        seen,
        present,
        windings,
        pairs,
        spanning,
        breaks,
        crossings,
        ends,
        fills,
    } = scratch;
    ends.fill(0.0);
    fills.fill(0.0);
    // The row's present subpaths, ascending; a stamp of row + 1 marks one
    // seen in this row.
    let stamp = row as u32 + 1;
    present.clear();
    for &s in tags.iter() {
        if seen[s as usize] != stamp {
            seen[s as usize] = stamp;
            present.push(s);
        }
    }
    present.sort_unstable();
    // The constant prefix: from nothing after the last absent intersect.
    let absent = fold
        .intersects
        .iter()
        .rev()
        .find(|&&k| seen[k as usize] != stamp);
    let (base, visited) = match absent {
        Some(&k) => (false, &present[present.partition_point(|&s| s <= k)..]),
        None => (fold.start, &present[..]),
    };
    edge_breaks(top, bottom, width, active, breaks, work, job)?;
    pair_breaks(top, active, pairs, breaks, work, job)?;
    breaks.sort_unstable_by(f64::total_cmp);
    breaks.dedup();
    spanning.clear();
    let mut cursor = 0;
    for pair in breaks.windows(2) {
        job.check()?;
        let y = (pair[0] + pair[1]) * 0.5;
        let weight = pair[1] - pair[0];
        while cursor < active.len() && min_y(&active[cursor]) <= y {
            spanning.push(cursor as u32);
            cursor += 1;
        }
        charge(work, spanning.len() as u64)?;
        spanning.retain(|&e| max_y(&active[e as usize]) > y);
        crossings.clear();
        for &index in spanning.iter() {
            let e = active[index as usize];
            let direction = if e[1][1] > e[0][1] { 1 } else { -1 };
            crossings.push((x_at(e, y), direction, tags[index as usize]));
        }
        let runs = crossings.len() as u64 + 1;
        charge(work, runs.saturating_mul(present.len() as u64))?;
        // `accumulate_interval`'s walk, one winding per subpath.
        crossings.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        for &s in present.iter() {
            windings[s as usize] = 0;
        }
        let mut previous = 0.0;
        for &(x, direction, s) in crossings.iter() {
            if fold.at(base, visited, windings) {
                span(previous, x, weight, w, ends, fills);
            }
            windings[s as usize] += direction;
            previous = x;
        }
        if fold.at(base, visited, windings) {
            span(previous, f64::from(width), weight, w, ends, fills);
        }
    }
    emit_row(ends, fills, output);
    Ok(())
}

/// The coverage of `commands` / `points` (`artwork_coverage`'s verbs, in
/// output pixel coordinates) over a `width` × `height` window, row-major u16,
/// the subpaths folded by `ops`, one per move (module doc). Refuses an
/// operation count other than the moves, more than `MAX_SUBPATHS` moves, and
/// everything `rasterize` refuses, with its sentences.
pub(crate) fn rasterize_ops(
    commands: &[u8],
    points: &[[f64; 2]],
    ops: &[SubpathOp],
    width: u32,
    height: u32,
    even_odd: bool,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let n = pixel_count(width, height)?;
    let (edges, tags, moves) = outline_by_subpath(commands, points, job)?;
    if moves as usize > MAX_SUBPATHS {
        return Err(too_many_subpaths());
    }
    if ops.len() != moves as usize {
        return Err(MISMATCH.into());
    }
    let mut work = 0;
    charge(&mut work, n as u64)?;
    let fold = Fold::new(ops, even_odd);
    let constant = if fold.outside { u16::MAX } else { 0 };
    let w = width as usize;
    let h = f64::from(height);
    let mut output = reserved_vec(n)?;
    if edges.is_empty() {
        output.resize(n, constant);
        job.check()?;
        job.report(1.0, 5)?;
        return Ok(output);
    }
    // Uncharged, as in `rasterize`: at most E·log₂E comparisons.
    let mut sorted: Vec<u32> = (0..edges.len() as u32).collect();
    sorted
        .sort_unstable_by(|&a, &b| min_y(&edges[a as usize]).total_cmp(&min_y(&edges[b as usize])));
    let top = min_y(&edges[sorted[0] as usize]);
    let bottom = edges.iter().map(max_y).fold(f64::NEG_INFINITY, f64::max);
    let first = top.floor().clamp(0.0, h) as usize;
    let last = bottom.ceil().clamp(0.0, h) as usize;
    if first > 0 {
        output.resize(first * w, constant);
        job.check()?;
        job.report(first as f64 / h, 5)?;
    }
    let subpaths = moves as usize;
    let mut scratch = Scratch {
        active: Vec::with_capacity(edges.len()),
        tags: Vec::with_capacity(edges.len()),
        seen: vec![0; subpaths],
        present: Vec::with_capacity(subpaths),
        windings: vec![0; subpaths],
        pairs: PairScratch::new(edges.len()),
        spanning: Vec::with_capacity(edges.len()),
        breaks: Vec::new(),
        crossings: Vec::with_capacity(edges.len()),
        ends: vec![0.0; w],
        fills: vec![0.0; w + 1],
    };
    let mut cursor = 0;
    for row in first..last {
        job.check()?;
        let next = (row + 1) as f64;
        while cursor < sorted.len() && min_y(&edges[sorted[cursor] as usize]) < next {
            let index = sorted[cursor] as usize;
            scratch.active.push(edges[index]);
            scratch.tags.push(tags[index]);
            cursor += 1;
        }
        // The two lists in step, order kept: `active` stays sorted by `min y`.
        let row_top = row as f64;
        let mut kept = 0;
        for i in 0..scratch.active.len() {
            if max_y(&scratch.active[i]) > row_top {
                scratch.active[kept] = scratch.active[i];
                scratch.tags[kept] = scratch.tags[i];
                kept += 1;
            }
        }
        scratch.active.truncate(kept);
        scratch.tags.truncate(kept);
        charge(&mut work, kept as u64)?;
        if kept == 0 {
            output.resize(output.len() + w, constant);
        } else {
            row_coverage(row, width, &fold, &mut scratch, &mut output, &mut work, job)?;
        }
        job.report((row + 1) as f64 / h, 5)?;
    }
    if last < height as usize {
        output.resize(n, constant);
        job.check()?;
        job.report(1.0, 5)?;
    }
    Ok(output)
}

/// [`rasterize_ops`] with no job: the seam a black-box test reads the
/// routine through (`tests/path_shared_edge.rs`).
#[doc(hidden)]
pub fn ops_coverage(
    commands: &[u8],
    points: &[[f64; 2]],
    ops: &[SubpathOp],
    width: u32,
    height: u32,
    even_odd: bool,
) -> Result<Vec<u16>, String> {
    rasterize_ops(
        commands,
        points,
        ops,
        width,
        height,
        even_odd,
        JobContext::NONE,
    )
}
