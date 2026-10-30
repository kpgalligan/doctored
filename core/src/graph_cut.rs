//! KERNEL: one binary minimum s-t cut on a 4-connected pixel grid
//! (Auto-Blend Layers ▸ Panorama's seams, `panorama_seam`).
//!
//! # The problem
//!
//! A [`CutGrid`] labels every pixel source (`false`) or sink (`true`). A
//! FIXED pixel keeps the side it names. A free pixel pays `unary[i][0]` on
//! the sink side and `unary[i][1]` on the source side, and every link whose
//! two pixels take different sides pays its capacity (`right[i]` for the
//! pixel to the right, `down[i]` for the pixel below; symmetric). [`min_cut`]
//! answers a labelling of least total cost.
//!
//! # The algorithm
//!
//! Boykov and Kolmogorov's augmenting-path max-flow ("An Experimental
//! Comparison of Min-Cut/Max-Flow Algorithms for Energy Minimization in
//! Vision", PAMI 2004; the steps of their `maxflow` 3.0): a search tree grows
//! from each terminal, a path is found where the trees touch, the path is
//! saturated, and the nodes it cut off are re-adopted or freed. Chosen over
//! push-relabel because a seam grid is a planar 4-connected grid with short
//! augmenting paths, where the two reused trees do the least work, and over
//! Dinic because it rebuilds no level graph per phase. A free pixel's two
//! unary costs are one signed terminal capacity (`unary[0] − unary[1]`
//! toward the source when positive, toward the sink when negative): the
//! common part is a constant of every labelling. A fixed pixel is a terminal
//! capacity no path can saturate ([`FIXED`]).
//!
//! # The answer
//!
//! After the flow is maximal, the free pixels still reachable from the
//! source in the residual graph (exactly the source tree, because no active
//! node is left) are source; every other free pixel is sink, so a free pixel
//! with no cost and no link is sink. Fixed pixels keep their side.
//!
//! # Working set
//!
//! [`CUT_BYTES_PER_NODE`] per pixel: four residual capacities (16), the
//! terminal capacity (8), the time stamp and distance (8), the parent and the
//! tree and active flags (3), the active queue and the orphan list at their
//! bound of one entry a node (16) and the answer (1).

use std::collections::VecDeque;

use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;

/// A link's capacity is clamped here, so a residual capacity (at most the
/// link's two directions summed) always fits a `u32`.
const MAX_LINK: u32 = u32::MAX / 2;

/// A fixed pixel's terminal capacity. Every finite capacity of a grid of
/// `usize` pixels sums to under `2^62` (each at most 2 × `u32::MAX` per
/// pixel and direction), so no augmenting path's bottleneck is ever this.
const FIXED: i64 = 1 << 62;

/// [`cut_bytes`]' charge per node (module doc).
const CUT_BYTES_PER_NODE: u64 = 52;

/// The four neighbours, as `(dx, dy)`; a direction's opposite is `d ^ 1`.
const LEFT: usize = 0;
const RIGHT: usize = 1;
const UP: usize = 2;
const DOWN: usize = 3;

/// A node's parent: none (free), a terminal, an orphan, or the neighbour in
/// direction `0..4` (the arc from the node to its parent).
const NONE: u8 = 4;
const TERMINAL: u8 = 5;
const ORPHAN: u8 = 6;

/// One grid to cut (module doc). Every plane is `width × height`, row-major.
#[derive(Clone, Debug, Default)]
pub struct CutGrid {
    pub width: usize,
    pub height: usize,
    /// `Some(false)` a fixed source pixel, `Some(true)` a fixed sink pixel,
    /// `None` a free one.
    pub fixed: Vec<Option<bool>>,
    /// `[cost if sink, cost if source]`; read only for a free pixel.
    pub unary: Vec<[u32; 2]>,
    /// The link between pixel `i` and `i + 1`; the last column is unused.
    pub right: Vec<u32>,
    /// The link between pixel `i` and `i + width`; the last row is unused.
    pub down: Vec<u32>,
}

/// The solver's own bytes for `nodes` pixels (module doc), beside the grid.
pub(crate) fn cut_bytes(nodes: u64) -> u64 {
    nodes.saturating_mul(CUT_BYTES_PER_NODE)
}

/// The least-cost labelling of `grid`, true = sink (module doc). A grid
/// whose planes are not `width × height` answers an empty labelling.
pub fn min_cut(grid: &CutGrid) -> Vec<bool> {
    min_cut_with_context(grid, JobContext::NONE).unwrap_or_default()
}

struct Flow {
    w: usize,
    h: usize,
    /// Residual capacity of the arc from a node toward each neighbour.
    cap: Vec<[u32; 4]>,
    /// Residual terminal capacity: positive from the source, negative to the
    /// sink.
    tr: Vec<i64>,
    parent: Vec<u8>,
    sink: Vec<bool>,
    active: Vec<bool>,
    ts: Vec<u32>,
    dist: Vec<u32>,
    queue: VecDeque<usize>,
    orphans: VecDeque<usize>,
    time: u32,
}

/// [`min_cut`] polling `job`; `Err` for planes that are not `width ×
/// height`, a refused allocation or a cancel.
pub(crate) fn min_cut_with_context(
    grid: &CutGrid,
    job: JobContext<'_>,
) -> Result<Vec<bool>, String> {
    let (w, h) = (grid.width, grid.height);
    let n = w
        .checked_mul(h)
        .ok_or("graph cut grid exceeds addressable memory")?;
    if grid.fixed.len() != n
        || grid.unary.len() != n
        || grid.right.len() != n
        || grid.down.len() != n
    {
        return Err("graph cut planes differ from the grid".into());
    }
    let mut flow = Flow::new(grid, job)?;
    flow.run(job)?;
    let mut out: Vec<bool> = reserved_vec(n)?;
    for i in 0..n {
        out.push(match grid.fixed[i] {
            Some(side) => side,
            None => flow.parent[i] == NONE || flow.sink[i],
        });
    }
    Ok(out)
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, String> {
    let mut out = reserved_vec(count)?;
    out.resize(count, value);
    Ok(out)
}

impl Flow {
    fn new(grid: &CutGrid, job: JobContext<'_>) -> Result<Self, String> {
        let (w, h) = (grid.width, grid.height);
        let n = w * h;
        let mut cap = filled(n, [0u32; 4])?;
        let mut tr = filled(n, 0i64)?;
        for i in 0..n {
            job.every(i)?;
            let (x, y) = (i % w, i / w);
            tr[i] = match grid.fixed[i] {
                Some(false) => FIXED,
                Some(true) => -FIXED,
                None => i64::from(grid.unary[i][0]) - i64::from(grid.unary[i][1]),
            };
            // A link between two fixed pixels is a constant of every
            // labelling, so it carries no flow.
            let link = |j: usize, value: u32| {
                if grid.fixed[i].is_some() && grid.fixed[j].is_some() {
                    0
                } else {
                    value.min(MAX_LINK)
                }
            };
            if x + 1 < w {
                let c = link(i + 1, grid.right[i]);
                cap[i][RIGHT] = c;
                cap[i + 1][LEFT] = c;
            }
            if y + 1 < h {
                let c = link(i + w, grid.down[i]);
                cap[i][DOWN] = c;
                cap[i + w][UP] = c;
            }
        }
        let mut queue = VecDeque::new();
        queue
            .try_reserve_exact(n)
            .map_err(|_| "could not allocate the graph cut queue".to_string())?;
        let mut orphans = VecDeque::new();
        orphans
            .try_reserve_exact(n)
            .map_err(|_| "could not allocate the graph cut orphans".to_string())?;
        let mut flow = Self {
            w,
            h,
            cap,
            tr,
            parent: filled(n, NONE)?,
            sink: filled(n, false)?,
            active: filled(n, false)?,
            ts: filled(n, 0u32)?,
            dist: filled(n, 0u32)?,
            queue,
            orphans,
            time: 0,
        };
        for i in 0..n {
            if flow.tr[i] != 0 {
                flow.sink[i] = flow.tr[i] < 0;
                flow.parent[i] = TERMINAL;
                flow.dist[i] = 1;
                flow.set_active(i);
            }
        }
        Ok(flow)
    }

    /// The neighbour of `i` in direction `d`, when the grid has one.
    fn neighbour(&self, i: usize, d: usize) -> Option<usize> {
        let (x, y) = (i % self.w, i / self.w);
        match d {
            LEFT => (x > 0).then(|| i - 1),
            RIGHT => (x + 1 < self.w).then(|| i + 1),
            UP => (y > 0).then(|| i - self.w),
            DOWN => (y + 1 < self.h).then(|| i + self.w),
            _ => None,
        }
    }

    fn set_active(&mut self, i: usize) {
        if !self.active[i] {
            self.active[i] = true;
            self.queue.push_back(i);
        }
    }

    fn next_active(&mut self) -> Option<usize> {
        while let Some(i) = self.queue.pop_front() {
            self.active[i] = false;
            if self.parent[i] != NONE {
                return Some(i);
            }
        }
        None
    }

    /// The node `i`'s parent arc points at.
    fn parent_of(&self, i: usize) -> usize {
        self.neighbour(i, usize::from(self.parent[i])).unwrap_or(i)
    }

    fn run(&mut self, job: JobContext<'_>) -> Result<(), String> {
        let mut current: Option<usize> = None;
        let mut steps = 0usize;
        loop {
            steps += 1;
            job.every(steps)?;
            let mut node = current.take();
            if let Some(i) = node {
                self.active[i] = false;
                if self.parent[i] == NONE {
                    node = None;
                }
            }
            let i = match node {
                Some(i) => i,
                None => match self.next_active() {
                    Some(i) => i,
                    None => return Ok(()),
                },
            };
            // The arc where the trees touch: from a source-tree node toward
            // a sink-tree node, as `(tail, direction)`.
            let mut touch: Option<(usize, usize)> = None;
            for d in 0..4 {
                let Some(j) = self.neighbour(i, d) else {
                    continue;
                };
                let back = (d ^ 1) as u8;
                if !self.sink[i] {
                    if self.cap[i][d] == 0 {
                        continue;
                    }
                    if self.parent[j] == NONE {
                        self.sink[j] = false;
                        self.parent[j] = back;
                        self.ts[j] = self.ts[i];
                        self.dist[j] = self.dist[i] + 1;
                        self.set_active(j);
                    } else if self.sink[j] {
                        touch = Some((i, d));
                        break;
                    } else if self.ts[j] <= self.ts[i] && self.dist[j] > self.dist[i] {
                        self.parent[j] = back;
                        self.ts[j] = self.ts[i];
                        self.dist[j] = self.dist[i] + 1;
                    }
                } else {
                    if self.cap[j][d ^ 1] == 0 {
                        continue;
                    }
                    if self.parent[j] == NONE {
                        self.sink[j] = true;
                        self.parent[j] = back;
                        self.ts[j] = self.ts[i];
                        self.dist[j] = self.dist[i] + 1;
                        self.set_active(j);
                    } else if !self.sink[j] {
                        touch = Some((j, d ^ 1));
                        break;
                    } else if self.ts[j] <= self.ts[i] && self.dist[j] > self.dist[i] {
                        self.parent[j] = back;
                        self.ts[j] = self.ts[i];
                        self.dist[j] = self.dist[i] + 1;
                    }
                }
            }
            self.time = self.time.wrapping_add(1);
            if let Some((tail, d)) = touch {
                // `i` stays current (and flagged active, so it is not queued
                // twice) until it has no more paths.
                self.active[i] = true;
                current = Some(i);
                self.augment(tail, d);
                self.adopt(job)?;
            }
        }
    }

    fn augment(&mut self, tail: usize, d: usize) {
        let head = self.neighbour(tail, d).unwrap_or(tail);
        let mut bottleneck = i64::from(self.cap[tail][d]);
        let mut i = tail;
        while self.parent[i] != TERMINAL {
            let p = self.parent_of(i);
            let into = usize::from(self.parent[i]) ^ 1;
            bottleneck = bottleneck.min(i64::from(self.cap[p][into]));
            i = p;
        }
        bottleneck = bottleneck.min(self.tr[i]);
        let mut i = head;
        while self.parent[i] != TERMINAL {
            let up = usize::from(self.parent[i]);
            bottleneck = bottleneck.min(i64::from(self.cap[i][up]));
            i = self.parent_of(i);
        }
        bottleneck = bottleneck.min(-self.tr[i]);
        let b = bottleneck as u32;

        self.cap[head][d ^ 1] += b;
        self.cap[tail][d] -= b;
        let mut i = tail;
        while self.parent[i] != TERMINAL {
            let up = usize::from(self.parent[i]);
            let p = self.parent_of(i);
            self.cap[i][up] += b;
            self.cap[p][up ^ 1] -= b;
            if self.cap[p][up ^ 1] == 0 {
                self.parent[i] = ORPHAN;
                self.orphans.push_front(i);
            }
            i = p;
        }
        self.tr[i] -= bottleneck;
        if self.tr[i] == 0 {
            self.parent[i] = ORPHAN;
            self.orphans.push_front(i);
        }
        let mut i = head;
        while self.parent[i] != TERMINAL {
            let up = usize::from(self.parent[i]);
            let p = self.parent_of(i);
            self.cap[p][up ^ 1] += b;
            self.cap[i][up] -= b;
            if self.cap[i][up] == 0 {
                self.parent[i] = ORPHAN;
                self.orphans.push_front(i);
            }
            i = p;
        }
        self.tr[i] += bottleneck;
        if self.tr[i] == 0 {
            self.parent[i] = ORPHAN;
            self.orphans.push_front(i);
        }
    }

    /// Whether an arc of residual capacity joins `i` to its tree through `j`:
    /// `j → i` in the source tree, `i → j` in the sink tree.
    fn feeds(&self, i: usize, j: usize, d: usize) -> bool {
        if self.sink[i] {
            self.cap[i][d] > 0
        } else {
            self.cap[j][d ^ 1] > 0
        }
    }

    fn adopt(&mut self, job: JobContext<'_>) -> Result<(), String> {
        let mut steps = 0usize;
        while let Some(i) = self.orphans.pop_front() {
            steps += 1;
            job.every(steps)?;
            let tree = self.sink[i];
            let mut best: Option<(u8, u32)> = None;
            for d in 0..4 {
                let Some(j) = self.neighbour(i, d) else {
                    continue;
                };
                if self.sink[j] != tree || self.parent[j] == NONE || !self.feeds(i, j, d) {
                    continue;
                }
                // The distance of `j` to its terminal, or none when its
                // path meets an orphan.
                let mut k = j;
                let mut distance = 0u32;
                let rooted = loop {
                    if self.ts[k] == self.time {
                        distance += self.dist[k];
                        break true;
                    }
                    distance += 1;
                    match self.parent[k] {
                        TERMINAL => {
                            self.ts[k] = self.time;
                            self.dist[k] = 1;
                            break true;
                        }
                        ORPHAN | NONE => break false,
                        _ => k = self.parent_of(k),
                    }
                };
                if !rooted {
                    continue;
                }
                if best.is_none_or(|(_, least)| distance < least) {
                    best = Some((d as u8, distance));
                }
                // Stamp the path so later searches stop on it.
                let mut k = j;
                let mut left = distance;
                while self.ts[k] != self.time {
                    self.ts[k] = self.time;
                    self.dist[k] = left;
                    left = left.saturating_sub(1);
                    k = self.parent_of(k);
                }
            }
            if let Some((d, distance)) = best {
                self.parent[i] = d;
                self.ts[i] = self.time;
                self.dist[i] = distance + 1;
                continue;
            }
            // No parent: `i` is freed, its tree neighbours that could feed
            // it become active, and its children become orphans.
            for d in 0..4 {
                let Some(j) = self.neighbour(i, d) else {
                    continue;
                };
                if self.sink[j] != tree || self.parent[j] == NONE {
                    continue;
                }
                if self.feeds(i, j, d) {
                    self.set_active(j);
                }
                let up = self.parent[j];
                if up != TERMINAL && up != ORPHAN && self.parent_of(j) == i {
                    self.parent[j] = ORPHAN;
                    self.orphans.push_back(j);
                }
            }
            self.parent[i] = NONE;
        }
        Ok(())
    }
}
