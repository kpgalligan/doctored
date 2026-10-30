//! Native artwork coverage from vector outlines, independent of authored ink.
//! Curves flatten to at most 1/4096 pixel control-polygon distance. Linear
//! edges integrate analytically between every row, vertex, column crossing and
//! edge intersection. Midpoint integration is exact on those linear intervals;
//! there is no fixed subpixel grid. The final area rounds once to u16.
//!
//! Since 13A-6b the scan is an ACTIVE-EDGE SWEEP. The edges are ordered by
//! their top (`total_cmp` on `min y`, through an index permutation so each
//! keeps its loop index); that sort is UNCHARGED — at most E·log₂E ≈ 19·E
//! comparisons, bounded by `MAX_PATH_EDGES` like the outline itself. Rows
//! above the outline's first row and below its last are zero and are skipped
//! (one poll and one report per non-empty skipped block); inside `[first,
//! last)` a cursor admits each edge whose top lies above the row's bottom, a
//! retain drops each whose bottom lies at or above the row's top, and
//! `row_coverage` — THE row body — integrates the row. The active SET per row
//! is exactly the full scan's (`min(max y, row + 1) > max(min y, row)` iff
//! `max y > row` and `min y < row + 1`; an edge ending exactly on the row's
//! top or starting exactly on its bottom is excluded by both), and because the
//! cursor pushes in top order and the retain keeps order, the active list
//! stays SORTED BY `min y` — `row_coverage`'s precondition.
//!
//! The row body is a y-ordered sweep, so a row does not pay `active²`. The
//! retained full-scan reference `reference_rasterize` runs the earlier
//! all-pairs body verbatim, `reference_row_coverage` (all pairs, every active
//! edge per interval); both bodies share `edge_breaks`, `accumulate_interval`
//! and `emit_row`. The PAIR SWEEP walks the edges in order, keeping an OPEN
//! list: before edge `b` it drops each open `a` whose `min(max y_a, row + 1)`
//! is at or below `max(min y_b, row)`, crosses `b` with every open `a` that
//! remains, then opens `b`. The INTERVAL SWEEP walks the sorted breakpoints'
//! midpoints `y` with a SPANNING list: a cursor admits edges with `min y ≤ y`,
//! then each with `max y ≤ y` is dropped, and the survivors' intersections
//! are accumulated.
//!
//! The row body first splits the active edges into X-CLUSTERS and
//! runs both sweeps per cluster (`cluster_coverage`), on the cluster's OWN
//! breakpoints, into the row's shared accumulation planes. `outline` tags
//! every edge with its CLOSED LOOP (a new index at every move and after every
//! close; the implicit close belongs to its loop); a loop's x-extent within the
//! row is the range of its active edges' x at their clipped ends, and extents
//! that overlap STRICTLY are merged into one cluster. Before the clusters one
//! long line of glyphs merged every glyph's breakpoints into one row-wide
//! list, so each of its ≈ `active` intervals visited ≈ every glyph's spanning
//! edges and the row cost grew with the square of the line length; now a row
//! costs the sum of its glyphs' own costs, linear in the line's length.
//!
//! EXACTNESS. A closed loop's winding number at a point of the row is the
//! signed count of its crossings on the horizontal ray to the left, all of
//! them on active edges; left of the loop's x-extent there are none, and right
//! of it there are all of them, which sum to zero. So a loop contributes
//! winding only inside its extent, the winding inside a cluster is its own
//! loops' sum, and the row's covered area is the sum of the clusters' — exact
//! in real arithmetic, under nonzero and even-odd alike (clusters only touch
//! at a point). Within a cluster, neither the edge order nor the sweeps reach
//! a sample. The breakpoints are sorted and deduplicated before use, so their
//! push order is irrelevant. A pair's crossing `low − d0·(high − low)/(d1 −
//! d0)` is exact under operand swap (IEEE negation is exact, `(−a)/(−b) =
//! a/b`, and `x − y = −(y − x)`), and the sweep evaluates it with the
//! reference's own `low`/`high`/crossing expressions. The PAIR SET equals the
//! reference's `high > low` set over the cluster: with `min y_a ≤ min y_b`
//! (the order), `low = max(min y_b, row)`, and `max y_b > low` holds for every
//! active, non-horizontal `b`, so `high > low` iff `min(max y_a, row + 1) >
//! low` — the open-list predicate — and because `low` never decreases along
//! the order, an `a` dropped once never pairs with a later `b`. The
//! PER-INTERVAL SET is exactly `min y ≤ y < max y` (the reference's test): the
//! midpoints strictly increase over sorted, deduplicated breaks, so an
//! admitted edge stays admitted and a dropped one never returns. Equal-x
//! intersections bound empty spans and are sorted by x before the winding
//! walk. A ONE-CLUSTER row (every row of a single glyph, every row whose loops
//! all overlap) therefore equals the full scan sample for sample — its cluster
//! is the active list in the same order; a row of several clusters integrates
//! each on its own breakpoints, a different floating-point summation of the
//! same area, and may differ by at most one code.
//!
//! MONOTONICITY. The sweep charges ONLY `n` up front, then `active` per
//! visited row, then the row body's charges; the full scan charges `n + E·H`
//! up front (plus 2 per skipped row), then per row the column crossings (the
//! same set; within a row they come in sorted-by-top order, not outline
//! order), `active²` and `breaks·(active + 1)`. Per row of `A` active edges,
//! `B` reference breakpoints and `C` clusters of `A_c` edges and `B_c`
//! breakpoints: the pair sweep charges `2·|open| + 1` per edge with `|open|`
//! taken before the drop, at most `2i + 1` for a cluster's `i`-th edge, so at
//! most `Σ A_c² ≤ A²` (equality when every edge overlaps every other in y in
//! one cluster); the interval sweep charges `|spanning|` per interval and 1 per
//! cluster, at most `Σ (B_c − 1)·A_c + C ≤ (B − 1)·A + A = B·A < B(A + 1)`,
//! because a cluster's breakpoints are a subset of the reference's (the same
//! expressions over a subset of its pairs and edges) and each cluster holds
//! at least one edge. The grouping's sorts are uncharged, at most
//! `A·log₂A` comparisons under the row's charged `A`. `Σ_rows active ≤ E·H`,
//! so at every `charge` checkpoint the sweep's running total is at most the
//! full scan's: nothing the full scan accepts is refused, and the caps and
//! the refusal sentence are unchanged. The at-the-cap boundary fixture keeps
//! its verdict, and the 18,000-edge pair-term refusal too: its loops all
//! overlap, so they are one cluster. (Charging `n + E` up front would break
//! this: an outline whose every edge spans every row has `Σ active = E·H`, and
//! the full scan's total may sit exactly at `MAX_SCAN_WORK`, which `charge`
//! accepts.) A line of separate glyphs is charged `Σ_rows (active + C)` plus
//! its glyphs' own pair and interval terms, so a long line is bounded by
//! `MAX_PATH_EDGES` before the work cap. Polling: one `every(i)` per edge in
//! the grouping, edge and pair loops and one `check()` per interval.
//!
//! Analytic flattened-edge area handles fractional x/y coverage, winding and crossings; curve
//! flatness uses point-to-segment distance so backtracking controls cannot collapse. Since 13A-6b
//! `artwork_coverage::rasterize` is an active-edge sweep (edges sorted by top, rows outside the
//! outline skipped, `n` charged up front then the active count per row) over `row_coverage`, THE
//! row body; that body is a y-ordered sweep (a PAIR SWEEP over an open list charged
//! `2·|open| + 1` per edge, then an INTERVAL SWEEP over a spanning list), and it runs
//! per X-CLUSTER of closed loops (`|spanning|` per interval, 1 per cluster), so one long line of
//! glyphs costs linear in its length. The `#[doc(hidden)]` full-scan reference
//! `reference_rasterize` (the pre-13A-6b scan over a row band, the
//! `tests/bounded_artwork_coverage.rs` oracle) runs the earlier all-pairs body
//! `reference_row_coverage`; both share `edge_breaks` / `accumulate_interval` / `emit_row` — this
//! module doc is the exactness and never-charged-above-the-reference argument. Caller coverage
//! outputs remain untouched on refusal/cancellation; flattening, area, assembly and publication
//! have admission/work/polling bounds.
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec};

pub const MAX_PATH_POINTS: usize = 262_144;
/// The flattened outline's edge bound, and each row's breakpoint bound.
/// 524,288 since the x-clusters (262,144 before): at the 1/4096 px flatness
/// a prose glyph at 24 px is about 450 edges, so once the scan work stopped
/// growing with the square of a line's length the old bound was what refused
/// a line, at about 580 characters; this one carries about 1,160. Every scratch
/// buffer of the scan holds at most this many entries, which is what
/// `dr_artwork_path_coverage` reserves (`ffi_artwork.rs`), so the two move
/// together.
pub const MAX_PATH_EDGES: usize = 524_288;
const MAX_SCAN_WORK: u64 = 256_000_000;
type Point = [f64; 2];
type Edge = [Point; 2];

fn midpoint(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
/// The flattened outline under construction: its edges and, beside each, the
/// index of the CLOSED LOOP it belongs to — a new index at every move
/// and after every close, the implicit close belonging to its own loop.
struct Outline {
    edges: Vec<Edge>,
    loops: Vec<u32>,
    current: u32,
}
fn edge(outline: &mut Outline, a: Point, b: Point) -> Result<(), String> {
    if outline.edges.len() >= MAX_PATH_EDGES {
        return Err("artwork outline exceeds the flattened edge limit".into());
    }
    if a != b && a[1] != b[1] {
        outline.edges.push([a, b]);
        outline.loops.push(outline.current);
    }
    Ok(())
}
pub(crate) fn distance(p: Point, a: Point, b: Point) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}
fn cubic(edges: &mut Outline, p: [Point; 4], depth: u8, job: JobContext<'_>) -> Result<(), String> {
    job.every(edges.edges.len())?;
    if distance(p[1], p[0], p[3]).max(distance(p[2], p[0], p[3])) <= 1.0 / 4096.0 {
        return edge(edges, p[0], p[3]);
    }
    if depth >= 20 {
        return Err("artwork curve exceeds the subdivision limit".into());
    }
    let a = midpoint(p[0], p[1]);
    let b = midpoint(p[1], p[2]);
    let c = midpoint(p[2], p[3]);
    let d = midpoint(a, b);
    let e = midpoint(b, c);
    let f = midpoint(d, e);
    cubic(edges, [p[0], a, d, f], depth + 1, job)?;
    cubic(edges, [f, e, c, p[3]], depth + 1, job)
}
/// Verbs 0 move, 1 line, 2 quadratic, 3 cubic, 4 close. Points are already in
/// output pixel coordinates. Nonfinite/excessive geometry refuses before work.
/// Returns the edges and, beside each, its closed loop's index.
pub(crate) fn outline(
    commands: &[u8],
    points: &[Point],
    job: JobContext<'_>,
) -> Result<(Vec<Edge>, Vec<u32>), String> {
    if commands.len() > MAX_PATH_POINTS
        || points.len() > MAX_PATH_POINTS
        || points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
    {
        return Err("artwork path exceeds its finite geometry bounds".into());
    }
    let mut edges = Outline {
        edges: Vec::new(),
        loops: Vec::new(),
        current: 0,
    };
    let mut index = 0;
    let mut start = None;
    let mut current = None;
    for &command in commands {
        job.check()?;
        let n = match command {
            0 | 1 => 1,
            2 => 2,
            3 => 3,
            4 => 0,
            _ => return Err("unknown artwork path command".into()),
        };
        let end = index + n;
        let p = points
            .get(index..end)
            .ok_or("artwork path point count mismatch")?;
        match command {
            0 => {
                if let (Some(a), Some(b)) = (current, start) {
                    edge(&mut edges, a, b)?;
                }
                edges.current += 1;
                start = Some(p[0]);
                current = start;
            }
            1 => {
                let a = current.ok_or("artwork path needs a move command")?;
                edge(&mut edges, a, p[0])?;
                current = Some(p[0]);
            }
            2 => {
                let a = current.ok_or("artwork path needs a move command")?;
                let b = [
                    a[0] + (p[0][0] - a[0]) * 2.0 / 3.0,
                    a[1] + (p[0][1] - a[1]) * 2.0 / 3.0,
                ];
                let c = [
                    p[1][0] + (p[0][0] - p[1][0]) * 2.0 / 3.0,
                    p[1][1] + (p[0][1] - p[1][1]) * 2.0 / 3.0,
                ];
                cubic(&mut edges, [a, b, c, p[1]], 0, job)?;
                current = Some(p[1]);
            }
            3 => {
                let a = current.ok_or("artwork path needs a move command")?;
                cubic(&mut edges, [a, p[0], p[1], p[2]], 0, job)?;
                current = Some(p[2]);
            }
            4 => {
                let a = current.ok_or("artwork path needs a move command")?;
                edge(&mut edges, a, start.ok_or("artwork path needs an origin")?)?;
                edges.current += 1;
                current = start;
            }
            _ => unreachable!(),
        }
        index = end;
    }
    if index != points.len() {
        return Err("artwork path has unused points".into());
    }
    if let (Some(a), Some(b)) = (current, start) {
        edge(&mut edges, a, b)?;
    }
    Ok((edges.edges, edges.loops))
}

fn span(left: f64, right: f64, weight: f64, width: usize, ends: &mut [f64], fills: &mut [f64]) {
    let a = left.clamp(0.0, width as f64);
    let b = right.clamp(0.0, width as f64);
    if b <= a {
        return;
    }
    let first = a.floor() as usize;
    let last = b.ceil() as usize - 1;
    if first == last {
        ends[first] += (b - a) * weight;
        return;
    }
    ends[first] += (first as f64 + 1.0 - a) * weight;
    ends[last] += (b - last as f64) * weight;
    if first + 1 < last {
        fills[first + 1] += weight;
        fills[last] -= weight;
    }
}
fn charge(work: &mut u64, amount: u64) -> Result<(), String> {
    *work = work.saturating_add(amount);
    if *work > MAX_SCAN_WORK {
        return Err(
            "artwork outline exceeds the bounded area-integration work limit; simplify the path or reduce its size"
                .into(),
        );
    }
    Ok(())
}
fn x_at(e: Edge, y: f64) -> f64 {
    e[0][0] + (y - e[0][1]) * (e[1][0] - e[0][0]) / (e[1][1] - e[0][1])
}
fn add_break(breaks: &mut Vec<f64>, y: f64, top: f64, bottom: f64) -> Result<(), String> {
    if y > top && y < bottom {
        if breaks.len() >= MAX_PATH_EDGES {
            return Err("artwork row exceeds its integration breakpoint limit".into());
        }
        breaks.push(y);
    }
    Ok(())
}

/// The per-cluster working buffers, reused across clusters and rows: the
/// pair sweep's open list and the interval sweep's spanning list (indices
/// into the cluster's edges), the breakpoints, the per-interval intersections
/// and the two accumulation planes (shared by every cluster of a row).
struct BodyScratch {
    open: Vec<u32>,
    spanning: Vec<u32>,
    breaks: Vec<f64>,
    intersections: Vec<(f64, i32)>,
    ends: Vec<f64>,
    fills: Vec<f64>,
}

/// One row's working buffers, reused across rows: the active edges (filled by
/// the caller) and, for the sweep, each one's loop index (`loops`, kept in
/// step by the caller); the x-cluster grouping (`order`: (loop, then cluster;
/// active index), `extents`: one loop's x-extent and its `order` range,
/// `cluster`: one cluster's edges in `min y` order); and the body buffers.
struct RowScratch {
    active: Vec<Edge>,
    loops: Vec<u32>,
    order: Vec<(u32, u32)>,
    extents: Vec<(f64, f64, u32, u32)>,
    cluster: Vec<Edge>,
    body: BodyScratch,
}

impl RowScratch {
    /// `sweep` false (the reference) leaves the cluster buffers empty.
    fn new(width: usize, edges: usize, sweep: bool) -> Self {
        let tagged = if sweep { edges } else { 0 };
        Self {
            active: Vec::with_capacity(edges),
            loops: Vec::with_capacity(tagged),
            order: Vec::with_capacity(tagged),
            extents: Vec::with_capacity(tagged),
            cluster: Vec::with_capacity(tagged),
            body: BodyScratch {
                open: Vec::with_capacity(edges),
                spanning: Vec::with_capacity(edges),
                breaks: Vec::new(),
                intersections: Vec::with_capacity(edges),
                ends: vec![0.0; width],
                fills: vec![0.0; width + 1],
            },
        }
    }
}

/// The row bounds, every edge of `active`'s clipped ends and its column
/// crossings (charged per crossing, as before 13A-6b) into a cleared
/// `breaks`, unsorted. Shared by `cluster_coverage` (one x-cluster's edges)
/// and `reference_row_coverage` (every active edge).
fn edge_breaks(
    top: f64,
    bottom: f64,
    width: u32,
    active: &[Edge],
    breaks: &mut Vec<f64>,
    work: &mut u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    breaks.clear();
    breaks.extend_from_slice(&[top, bottom]);
    for (i, &e) in active.iter().enumerate() {
        job.every(i)?;
        let low = e[0][1].min(e[1][1]).max(top);
        let high = e[0][1].max(e[1][1]).min(bottom);
        if high <= low {
            continue;
        }
        add_break(breaks, low, top, bottom)?;
        add_break(breaks, high, top, bottom)?;
        let a = x_at(e, low);
        let b = x_at(e, high);
        if a != b {
            let start = a.min(b).max(0.0).ceil() as i64;
            let end = a.max(b).min(f64::from(width)).floor() as i64;
            if end >= start {
                charge(work, (end - start + 1) as u64)?;
            }
            for column in start..=end {
                let y = low + (column as f64 - a) * (high - low) / (b - a);
                add_break(breaks, y, low, high)?;
            }
        }
    }
    Ok(())
}

/// One interval's intersections (any order) sorted by x, then the winding
/// walk spanning each inside run at `weight`. Shared by both row bodies.
fn accumulate_interval(
    intersections: &mut [(f64, i32)],
    weight: f64,
    even_odd: bool,
    width: usize,
    ends: &mut [f64],
    fills: &mut [f64],
) {
    intersections.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut winding = 0_i32;
    let mut previous = 0.0;
    for &(x, direction) in intersections.iter() {
        if if even_odd {
            winding % 2 != 0
        } else {
            winding != 0
        } {
            span(previous, x, weight, width, ends, fills);
        }
        winding += direction;
        previous = x;
    }
}

/// The accumulated row as `ends.len()` u16 samples appended to `output`.
fn emit_row(ends: &[f64], fills: &[f64], output: &mut Vec<u16>) {
    let mut interior = 0.0;
    for (x, &end) in ends.iter().enumerate() {
        interior += fills[x];
        output.push(((end + interior).clamp(0.0, 1.0) * 65535.0).round() as u16);
    }
}

/// THE row body (see the module doc). `scratch.active` holds
/// exactly the edges overlapping `[row, row + 1)`, SORTED BY `min y` (ties in
/// any order), and `scratch.loops` each one's loop index. The active edges are
/// grouped into X-CLUSTERS — each loop's x-extent within the row, strictly
/// overlapping extents merged — and each cluster, its edges still in `min y`
/// order, is integrated on its own by `cluster_coverage` into the shared
/// accumulation planes (charged 1 per cluster); then `width` u16 samples are
/// appended to `output`. The grouping's sorts are uncharged: at most
/// `active·log₂(active)` comparisons, under the row's charged `active`.
/// `reference_row_coverage` is its independent all-pairs oracle.
fn row_coverage(
    row: usize,
    width: u32,
    even_odd: bool,
    scratch: &mut RowScratch,
    output: &mut Vec<u16>,
    work: &mut u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let top = row as f64;
    let bottom = top + 1.0;
    let RowScratch {
        active,
        loops,
        order,
        extents,
        cluster,
        body,
    } = scratch;
    body.ends.fill(0.0);
    body.fills.fill(0.0);
    // Group by loop, then each loop's x-extent within the row: the x of each
    // of its edges at the edge's clipped ends (an edge is linear in y).
    order.clear();
    order.extend(loops.iter().enumerate().map(|(i, &l)| (l, i as u32)));
    order.sort_unstable();
    extents.clear();
    let mut start = 0;
    while start < order.len() {
        job.every(start)?;
        let id = order[start].0;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut end = start;
        while end < order.len() && order[end].0 == id {
            let e = active[order[end].1 as usize];
            for y in [min_y(&e).max(top), max_y(&e).min(bottom)] {
                let x = x_at(e, y);
                lo = lo.min(x);
                hi = hi.max(x);
            }
            end += 1;
        }
        extents.push((lo, hi, start as u32, end as u32));
        start = end;
    }
    // Merge STRICTLY overlapping extents into clusters, relabelling each
    // loop's `order` entries with its cluster, then order by (cluster, active
    // index): the active index keeps `min y` order inside a cluster.
    extents.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut id = 0_u32;
    let mut reach = f64::NEG_INFINITY;
    for (k, &(lo, hi, from, to)) in extents.iter().enumerate() {
        if k > 0 && lo >= reach {
            id += 1;
            reach = hi;
        } else {
            reach = reach.max(hi);
        }
        for entry in &mut order[from as usize..to as usize] {
            entry.0 = id;
        }
    }
    order.sort_unstable();
    let mut start = 0;
    while start < order.len() {
        let id = order[start].0;
        cluster.clear();
        while start < order.len() && order[start].0 == id {
            cluster.push(active[order[start].1 as usize]);
            start += 1;
        }
        charge(work, 1)?;
        cluster_coverage(top, width, even_odd, cluster, body, work, job)?;
    }
    emit_row(&body.ends, &body.fills, output);
    Ok(())
}

/// One x-cluster of a row (the y-ordered sweep body, run per cluster): `edges`
/// SORTED BY `min y` (ties in any order); the cluster's own edge breakpoints,
/// then a PAIR SWEEP for its pairwise crossings (each edge charged `2·|open|
/// + 1`) and an INTERVAL SWEEP over its own breakpoints (each interval charged
/// `|spanning|`), accumulated into `body`'s planes.
fn cluster_coverage(
    top: f64,
    width: u32,
    even_odd: bool,
    edges: &[Edge],
    body: &mut BodyScratch,
    work: &mut u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let w = width as usize;
    let bottom = top + 1.0;
    let BodyScratch {
        open,
        spanning,
        breaks,
        intersections,
        ends,
        fills,
    } = body;
    edge_breaks(top, bottom, width, edges, breaks, work, job)?;
    open.clear();
    for (i, &b) in edges.iter().enumerate() {
        job.every(i)?;
        charge(work, 2 * open.len() as u64 + 1)?;
        let floor = min_y(&b).max(top);
        open.retain(|&a| max_y(&edges[a as usize]).min(bottom) > floor);
        for &index in open.iter() {
            let a = edges[index as usize];
            let low = top.max(a[0][1].min(a[1][1])).max(b[0][1].min(b[1][1]));
            let high = bottom.min(a[0][1].max(a[1][1])).min(b[0][1].max(b[1][1]));
            if high <= low {
                continue;
            }
            let d0 = x_at(a, low) - x_at(b, low);
            let d1 = x_at(a, high) - x_at(b, high);
            if d0 != d1 {
                add_break(breaks, low - d0 * (high - low) / (d1 - d0), low, high)?;
            }
        }
        open.push(i as u32);
    }
    breaks.sort_unstable_by(f64::total_cmp);
    breaks.dedup();
    spanning.clear();
    let mut cursor = 0;
    for pair in breaks.windows(2) {
        job.check()?;
        let y = (pair[0] + pair[1]) * 0.5;
        let weight = pair[1] - pair[0];
        while cursor < edges.len() && min_y(&edges[cursor]) <= y {
            spanning.push(cursor as u32);
            cursor += 1;
        }
        charge(work, spanning.len() as u64)?;
        spanning.retain(|&e| max_y(&edges[e as usize]) > y);
        intersections.clear();
        for &index in spanning.iter() {
            let e = edges[index as usize];
            intersections.push((x_at(e, y), if e[1][1] > e[0][1] { 1 } else { -1 }));
        }
        accumulate_interval(intersections, weight, even_odd, w, ends, fills);
    }
    Ok(())
}

/// The earlier all-pairs row body, kept verbatim as the independent oracle for
/// `row_coverage` (the master; the 13A-6c `brush_coverage::reference_rasterize`
/// precedent): `scratch.active` in ANY order, every pair tested (`active²`
/// charged up front) and every active edge scanned per interval
/// (`breaks·(active + 1)` charged up front). Used only by
/// `reference_rasterize`.
fn reference_row_coverage(
    row: usize,
    width: u32,
    even_odd: bool,
    scratch: &mut RowScratch,
    output: &mut Vec<u16>,
    work: &mut u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let w = width as usize;
    let top = row as f64;
    let bottom = top + 1.0;
    let RowScratch { active, body, .. } = scratch;
    let BodyScratch {
        breaks,
        intersections,
        ends,
        fills,
        ..
    } = body;
    ends.fill(0.0);
    fills.fill(0.0);
    edge_breaks(top, bottom, width, active, breaks, work, job)?;
    charge(
        work,
        (active.len() as u64).saturating_mul(active.len() as u64),
    )?;
    for (i, &a) in active.iter().enumerate() {
        job.every(i)?;
        for &b in &active[i + 1..] {
            let low = top.max(a[0][1].min(a[1][1])).max(b[0][1].min(b[1][1]));
            let high = bottom.min(a[0][1].max(a[1][1])).min(b[0][1].max(b[1][1]));
            if high <= low {
                continue;
            }
            let d0 = x_at(a, low) - x_at(b, low);
            let d1 = x_at(a, high) - x_at(b, high);
            if d0 != d1 {
                add_break(breaks, low - d0 * (high - low) / (d1 - d0), low, high)?;
            }
        }
    }
    breaks.sort_unstable_by(f64::total_cmp);
    breaks.dedup();
    charge(
        work,
        (breaks.len() as u64).saturating_mul(active.len() as u64 + 1),
    )?;
    for pair in breaks.windows(2) {
        job.check()?;
        let y = (pair[0] + pair[1]) * 0.5;
        let weight = pair[1] - pair[0];
        intersections.clear();
        for &e in active.iter() {
            if y >= e[0][1].min(e[1][1]) && y < e[0][1].max(e[1][1]) {
                intersections.push((x_at(e, y), if e[1][1] > e[0][1] { 1 } else { -1 }));
            }
        }
        accumulate_interval(intersections, weight, even_odd, w, ends, fills);
    }
    emit_row(ends, fills, output);
    Ok(())
}

fn min_y(e: &Edge) -> f64 {
    e[0][1].min(e[1][1])
}
fn max_y(e: &Edge) -> f64 {
    e[0][1].max(e[1][1])
}

/// The active-edge sweep (13A-6b; see the module doc): charges `n` up front,
/// then per visited row the active count and the row body's charges. Rows
/// outside the outline's `[first, last)` are zero and only report.
pub(crate) fn rasterize(
    commands: &[u8],
    points: &[Point],
    width: u32,
    height: u32,
    even_odd: bool,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let n = pixel_count(width, height)?;
    let (edges, loops) = outline(commands, points, job)?;
    let mut work = 0;
    charge(&mut work, n as u64)?;
    let w = width as usize;
    let h = f64::from(height);
    let mut output = reserved_vec(n)?;
    if edges.is_empty() {
        output.resize(n, 0);
        job.check()?;
        job.report(1.0, 5)?;
        return Ok(output);
    }
    // Uncharged: at most E·log₂E comparisons, bounded by `MAX_PATH_EDGES`.
    // A permutation, so each edge keeps its loop index.
    let mut sorted: Vec<u32> = (0..edges.len() as u32).collect();
    sorted
        .sort_unstable_by(|&a, &b| min_y(&edges[a as usize]).total_cmp(&min_y(&edges[b as usize])));
    let top = min_y(&edges[sorted[0] as usize]);
    let bottom = edges.iter().map(max_y).fold(f64::NEG_INFINITY, f64::max);
    let first = top.floor().clamp(0.0, h) as usize;
    let last = bottom.ceil().clamp(0.0, h) as usize;
    if first > 0 {
        output.resize(first * w, 0);
        job.check()?;
        job.report(first as f64 / h, 5)?;
    }
    let mut scratch = RowScratch::new(w, edges.len(), true);
    let mut cursor = 0;
    for row in first..last {
        job.check()?;
        let next = (row + 1) as f64;
        while cursor < sorted.len() && min_y(&edges[sorted[cursor] as usize]) < next {
            let index = sorted[cursor] as usize;
            scratch.active.push(edges[index]);
            scratch.loops.push(loops[index]);
            cursor += 1;
        }
        // `retain` over the two lists in step: order is kept, so `active`
        // stays sorted by `min y`.
        let top = row as f64;
        let mut kept = 0;
        for i in 0..scratch.active.len() {
            if max_y(&scratch.active[i]) > top {
                scratch.active[kept] = scratch.active[i];
                scratch.loops[kept] = scratch.loops[i];
                kept += 1;
            }
        }
        scratch.active.truncate(kept);
        scratch.loops.truncate(kept);
        charge(&mut work, scratch.active.len() as u64)?;
        row_coverage(
            row,
            width,
            even_odd,
            &mut scratch,
            &mut output,
            &mut work,
            job,
        )?;
        job.report((row + 1) as f64 / h, 5)?;
    }
    if last < height as usize {
        output.resize(n, 0);
        job.check()?;
        job.report(1.0, 5)?;
    }
    Ok(output)
}

/// The pre-13A-6b FULL SCAN over rows `[rows.0, rows.1)` ⊆ `[0, height)` in
/// absolute coordinates: charges `E·(rows.1 − rows.0) + width·(rows.1 −
/// rows.0)` up front, then selects every edge overlapping each row in OUTLINE
/// order and runs the all-pairs row body, `reference_row_coverage`. No job control; returns `(rows.1 −
/// rows.0)·width` samples. A black-box seam for tests only.
#[doc(hidden)]
pub fn reference_rasterize(
    commands: &[u8],
    points: &[[f64; 2]],
    width: u32,
    height: u32,
    even_odd: bool,
    rows: (u32, u32),
) -> Result<Vec<u16>, String> {
    pixel_count(width, height)?;
    if rows.0 > rows.1 || rows.1 > height {
        return Err("reference rows lie outside the canvas".into());
    }
    let job = JobContext::NONE;
    let (edges, _) = outline(commands, points, job)?;
    let band = u64::from(rows.1 - rows.0);
    let mut work = 0;
    charge(
        &mut work,
        (edges.len() as u64)
            .saturating_mul(band)
            .saturating_add(u64::from(width).saturating_mul(band)),
    )?;
    let w = width as usize;
    let mut output = Vec::with_capacity(w * band as usize);
    let mut scratch = RowScratch::new(w, edges.len(), false);
    for row in rows.0 as usize..rows.1 as usize {
        scratch.active.clear();
        let top = row as f64;
        let bottom = top + 1.0;
        for &e in &edges {
            let low = e[0][1].min(e[1][1]).max(top);
            let high = e[0][1].max(e[1][1]).min(bottom);
            if high > low {
                scratch.active.push(e);
            }
        }
        reference_row_coverage(
            row,
            width,
            even_odd,
            &mut scratch,
            &mut output,
            &mut work,
            job,
        )?;
    }
    Ok(output)
}
