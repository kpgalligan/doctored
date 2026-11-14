//! KERNEL: the pairwise-crossing stage of one x-cluster of an artwork row
//! (`artwork_coverage::cluster_coverage`): every crossing of two of the
//! cluster's edges strictly inside the row, pushed as a breakpoint.
//!
//! A FULL edge spans the whole row (`min y ≤ top` and `max y ≥ top + 1`); a
//! PARTIAL edge has a vertex inside it. The earlier stage crossed every edge
//! with every OPEN edge (the y-ordered open list), charged `2·|open| + 1` an
//! edge, which is `active²` whenever the cluster's edges all overlap in y,
//! crossing or not: an outline of a few hundred parallel edges per row (a
//! frame around a line of text, a plate with holes, a hatch) refused at
//! `MAX_SCAN_WORK` with no crossing to find. Now:
//!
//! * a PARTIAL edge `b` is crossed, as before, with every open edge: the open
//!   partial edges (dropped as before) and every full edge before it (a full
//!   edge is never dropped, because its clipped bottom is the row's bottom,
//!   below every later floor);
//! * a FULL edge `b` is crossed with the open partial edges only;
//! * FULL × FULL pairs come from an INVERSION SWEEP: each full edge's x at
//!   the row's top and bottom, `x0` and `x1`; the full edges sorted by `(x0,
//!   x1)`; then a bottom-up merge sort on `x1` that, when it takes an element
//!   from a right run, reports every element still in the left run: exactly
//!   the pairs whose `x1` order is STRICTLY the reverse of their `x0` order.
//!   Each reported pair runs the reference's own crossing expression.
//!
//! EXACTNESS: the breakpoint set is the reference's, so every sample is. For
//! two full edges the reference's `low` is `top` and its `high` is `bottom`,
//! so `high − low` is exactly 1 (rows are integers below 2⁵³) and the break is
//! `top − fl(d0 / D)`, `D = fl(d1 − d0)`, where `d0 = fl(x0_a − x0_b)` and `d1
//! = fl(x1_a − x1_b)` are differences of the per-edge values above. A float
//! difference has its operands' order's sign and is zero only for equal
//! operands. When `d0` and `d1` have the same strict sign the reference adds
//! no break: for `|d1| < |d0|`, `|D| ≤ |d0|` (rounding is monotone and `|d0|`
//! is a float), so `fl(d0 / D) ≤ −1` and the break lands at or past `bottom`;
//! for `|d1| > |d0|`, `D` has `d0`'s sign, `fl(d0 / D) > 0` and the break lands
//! at or before `top`; `add_break` rejects both. `d0 = 0` gives exactly `top`,
//! `d1 = 0` exactly `top + 1 = bottom`, and `d0 = d1` is skipped. So a full
//! pair yields a break only when `x0` and `x1` order it strictly oppositely,
//! which is the reported set, and its value is the reference's (the
//! expression is exact under operand swap: IEEE negation is exact). Pairs with
//! a partial edge are the open-list sweep's set (`artwork_coverage`'s module
//! doc). The breakpoints are sorted and deduplicated before use, so the order
//! they are found in is irrelevant.
//!
//! WORK: a partial `b` is charged `2·|walked| + 1` before its walk, the
//! earlier `2·|open| + 1`; a full `b` `2·|open partials| + 1`; each reported
//! full pair 2, before it runs. The earlier stage charged `2·C(F, 2)` for a
//! cluster's `F` full edges' pairs, and at most `C(F, 2)` are reported, so
//! this stage never charges more than the earlier one and
//! `artwork_coverage`'s MONOTONICITY argument holds as written. The two sorts
//! are uncharged, at most `F·log₂F` comparisons each under the row's charged
//! `active`. A genuinely quadratic cluster, every full pair crossing, is still
//! charged `F·(F − 1)` and still refuses.
//!
//! CANCELLATION: one counter over the cluster's pair tests polls every 4096
//! tests (`JobContext::every`), in the open-list walk and the inversion
//! report alike, besides the per-edge poll.
use crate::artwork_coverage::{add_break, charge, max_y, min_y, x_at, Edge};
use crate::job_control::JobContext;

/// The stage's buffers, reused across clusters and rows: the open partial
/// edges, the full edges (cluster indices, in `min y` order and then sorted),
/// the merge sort's other buffer, and each full edge's `x0` and `x1` by
/// cluster index. Each holds at most one entry per edge of the outline
/// (`MAX_PATH_EDGES`), which `dr_artwork_path_coverage` reserves.
pub(crate) struct PairScratch {
    open: Vec<u32>,
    fulls: Vec<u32>,
    merge: Vec<u32>,
    x0: Vec<f64>,
    x1: Vec<f64>,
}

impl PairScratch {
    pub(crate) fn new(edges: usize) -> Self {
        Self {
            open: Vec::with_capacity(edges),
            fulls: Vec::with_capacity(edges),
            merge: Vec::with_capacity(edges),
            x0: Vec::with_capacity(edges),
            x1: Vec::with_capacity(edges),
        }
    }
}

/// The pair-test counter: polls every 4096 tests (module doc).
struct Tests<'a> {
    count: usize,
    job: JobContext<'a>,
}

impl Tests<'_> {
    fn next(&mut self) -> Result<(), String> {
        self.count += 1;
        self.job.every(self.count)
    }
}

/// The reference's crossing of `a` and `b` inside the row `[top, bottom)`,
/// pushed when it lies strictly inside the pair's shared span.
fn cross(a: Edge, b: Edge, top: f64, bottom: f64, breaks: &mut Vec<f64>) -> Result<(), String> {
    let low = top.max(a[0][1].min(a[1][1])).max(b[0][1].min(b[1][1]));
    let high = bottom.min(a[0][1].max(a[1][1])).min(b[0][1].max(b[1][1]));
    if high <= low {
        return Ok(());
    }
    let d0 = x_at(a, low) - x_at(b, low);
    let d1 = x_at(a, high) - x_at(b, high);
    if d0 != d1 {
        add_break(breaks, low - d0 * (high - low) / (d1 - d0), low, high)?;
    }
    Ok(())
}

/// Pushes every crossing of two of `edges` (one x-cluster, SORTED BY `min y`,
/// ties in any order) strictly inside the row `[top, top + 1)` into `breaks`,
/// charging `work` (module doc).
pub(crate) fn pair_breaks(
    top: f64,
    edges: &[Edge],
    scratch: &mut PairScratch,
    breaks: &mut Vec<f64>,
    work: &mut u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let bottom = top + 1.0;
    let PairScratch {
        open,
        fulls,
        merge,
        x0,
        x1,
    } = scratch;
    open.clear();
    fulls.clear();
    x0.clear();
    x0.resize(edges.len(), 0.0);
    x1.clear();
    x1.resize(edges.len(), 0.0);
    let mut tests = Tests { count: 0, job };
    for (i, &b) in edges.iter().enumerate() {
        job.every(i)?;
        let full = min_y(&b) <= top && max_y(&b) >= bottom;
        let walked = if full {
            open.len()
        } else {
            open.len() + fulls.len()
        };
        charge(work, 2 * walked as u64 + 1)?;
        let floor = min_y(&b).max(top);
        open.retain(|&a| max_y(&edges[a as usize]).min(bottom) > floor);
        if !full {
            for &a in fulls.iter() {
                tests.next()?;
                cross(edges[a as usize], b, top, bottom, breaks)?;
            }
        }
        for &a in open.iter() {
            tests.next()?;
            cross(edges[a as usize], b, top, bottom, breaks)?;
        }
        if full {
            x0[i] = x_at(b, top);
            x1[i] = x_at(b, bottom);
            fulls.push(i as u32);
        } else {
            open.push(i as u32);
        }
    }
    let sweep = FullSweep { edges, top, x0, x1 };
    sweep.inversions(fulls, merge, breaks, work, &mut tests)
}

/// The inversion sweep's fixed inputs (module doc).
struct FullSweep<'a> {
    edges: &'a [Edge],
    top: f64,
    x0: &'a [f64],
    x1: &'a [f64],
}

impl FullSweep<'_> {
    /// Sorts `fulls` by `(x0, x1)`, then merge-sorts it on `x1` through
    /// `merge`, crossing every strictly inverted pair (module doc).
    fn inversions(
        &self,
        fulls: &mut Vec<u32>,
        merge: &mut Vec<u32>,
        breaks: &mut Vec<f64>,
        work: &mut u64,
        tests: &mut Tests<'_>,
    ) -> Result<(), String> {
        let count = fulls.len();
        if count < 2 {
            return Ok(());
        }
        let (x0, x1) = (self.x0, self.x1);
        let bottom = self.top + 1.0;
        fulls.sort_unstable_by(|&a, &b| {
            x0[a as usize]
                .total_cmp(&x0[b as usize])
                .then(x1[a as usize].total_cmp(&x1[b as usize]))
        });
        merge.clear();
        merge.resize(count, 0);
        let (mut from, mut to) = (fulls, merge);
        let mut run = 1;
        while run < count {
            let mut start = 0;
            while start < count {
                let mid = (start + run).min(count);
                let end = (start + 2 * run).min(count);
                let (mut i, mut j, mut k) = (start, mid, start);
                while i < mid && j < end {
                    let right = from[j];
                    if x1[right as usize] < x1[from[i] as usize] {
                        charge(work, 2 * (mid - i) as u64)?;
                        for &left in &from[i..mid] {
                            tests.next()?;
                            let (a, b) = (self.edges[left as usize], self.edges[right as usize]);
                            cross(a, b, self.top, bottom, breaks)?;
                        }
                        to[k] = right;
                        j += 1;
                    } else {
                        to[k] = from[i];
                        i += 1;
                    }
                    k += 1;
                }
                to[k..k + (mid - i)].copy_from_slice(&from[i..mid]);
                k += mid - i;
                to[k..k + (end - j)].copy_from_slice(&from[j..end]);
                start = end;
            }
            std::mem::swap(&mut from, &mut to);
            run *= 2;
        }
        Ok(())
    }
}
