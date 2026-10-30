//! The Magnetic Lasso tracer's corridor (`edge_trace`, steps 1 and 3): the
//! pixels whose centre lies within `width` of the trail polyline, built by
//! stamping each segment's capsule as one exact x-interval per pixel row it
//! touches and merging the intervals per row — work O(Σ(segment length +
//! 2·width) plus the corridor's pixels), never a per-pixel × per-segment
//! test — plus the precomputed neighbour cursors the chamfer scan and
//! Dijkstra walk, and the chamfer trail distance itself.
//!
//! the corridor (pixels within `width` of the trail polyline, per-row capsule interval stamping,
//! refused past `REFUSAL_CAP` = 8 × `WORK_BUDGET`, the header's `DR_EDGE_TRACE_WORK_BUDGET`) lives
//! in the CORRIDOR module with its precomputed 8-neighbour table and chamfer trail distance.

use crate::edge_trace::REFUSAL_CAP;
use std::f64::consts::SQRT_2;

/// The corridor: its pixels densely indexed in raster order.
pub(crate) struct Corridor {
    y0: i64,
    /// Per row from `y0`, the range of `pixels` it owns.
    rows: Vec<(usize, usize)>,
    /// Corridor index → pixel.
    pub(crate) pixels: Vec<(i64, i64)>,
    /// Per pixel, the corridor indices of its eight neighbours in the slot
    /// order up-left, up, up-right, left, right, down-left, down, down-right
    /// ([`NONE`] outside the corridor). Built once, in raster order, so the
    /// chamfer scan and Dijkstra read one contiguous row per pixel instead of
    /// searching; the first four slots precede the pixel in raster order and
    /// the last four follow it.
    pub(crate) neighbours: Vec<[u32; 8]>,
}

/// An empty neighbour slot.
pub(crate) const NONE: u32 = u32::MAX;
/// Each neighbour slot's step length: orthogonal 1, diagonal √2.
pub(crate) const STEPS: [f64; 8] = [SQRT_2, 1.0, SQRT_2, 1.0, 1.0, SQRT_2, 1.0, SQRT_2];

impl Corridor {
    pub(crate) fn index(&self, x: i64, y: i64) -> Option<usize> {
        let row = usize::try_from(y - self.y0).ok()?;
        let &(a, b) = self.rows.get(row)?;
        let k = self.pixels[a..b].binary_search_by_key(&x, |p| p.0).ok()?;
        Some(a + k)
    }
}

/// One segment's capsule of radius `r` — the union of the two end discs and
/// the swept rectangle — with the per-segment terms of [`Capsule::row`]
/// computed once rather than once per row.
struct Capsule {
    a: [f64; 2],
    b: [f64; 2],
    r2: f64,
    d: [f64; 2],
    l2: f64,
    /// `r · |b − a|`.
    reach: f64,
}

impl Capsule {
    fn new(a: [f64; 2], b: [f64; 2], r: f64) -> Self {
        let d = [b[0] - a[0], b[1] - a[1]];
        let l2 = d[0] * d[0] + d[1] * d[1];
        Capsule {
            a,
            b,
            r2: r * r,
            d,
            l2,
            reach: r * l2.sqrt(),
        }
    }

    /// The exact x-interval of the horizontal line `y = cy` inside the
    /// capsule, which is one interval because the capsule is convex.
    fn row(&self, cy: f64) -> Option<(f64, f64)> {
        let (a, [dx, dy], l2) = (self.a, self.d, self.l2);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut add = |l: f64, h: f64| {
            if l <= h {
                lo = lo.min(l);
                hi = hi.max(h);
            }
        };
        for c in [a, self.b] {
            let q = self.r2 - (cy - c[1]) * (cy - c[1]);
            if q >= 0.0 {
                let s = q.sqrt();
                add(c[0] - s, c[0] + s);
            }
        }
        if l2 > 0.0 {
            let ey = cy - a[1];
            let (mut rl, mut rh) = (f64::NEG_INFINITY, f64::INFINITY);
            // The projection parameter in [0, 1]: (x − ax)·dx + ey·dy ∈ [0, l²].
            if dx != 0.0 {
                let (p, q) = ((-ey * dy) / dx + a[0], (l2 - ey * dy) / dx + a[0]);
                rl = rl.max(p.min(q));
                rh = rh.min(p.max(q));
            } else if !(0.0..=l2).contains(&(ey * dy)) {
                rl = f64::INFINITY;
            }
            // The perpendicular distance: |(x − ax)·dy − ey·dx| ≤ r·l.
            if dy != 0.0 {
                let (p, q) = (
                    (ey * dx - self.reach) / dy + a[0],
                    (ey * dx + self.reach) / dy + a[0],
                );
                rl = rl.max(p.min(q));
                rh = rh.min(p.max(q));
            } else if (ey * dx).abs() > self.reach {
                rl = f64::INFINITY;
            }
            add(rl, rh);
        }
        (lo <= hi).then_some((lo, hi))
    }
}

fn segments(poly: &[[f64; 2]]) -> Vec<([f64; 2], [f64; 2])> {
    if poly.len() == 1 {
        return vec![(poly[0], poly[0])];
    }
    poly.windows(2).map(|w| (w[0], w[1])).collect()
}

pub(crate) fn corridor(poly: &[[f64; 2]], r: f64, w: u32, h: u32) -> Result<Corridor, String> {
    let (wi, hi) = (i64::from(w), i64::from(h));
    let (y_min, y_max) = poly
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, high), p| {
            (lo.min(p[1]), high.max(p[1]))
        });
    let y0 = ((y_min - r - 0.5).ceil() as i64).max(0);
    let y1 = ((y_max + r - 0.5).floor() as i64).min(hi - 1);
    // One bucket per row; a stamp overlapping the row's latest one merges
    // into it at once, since consecutive segments overlap almost entirely.
    let mut buckets: Vec<Vec<(i64, i64)>> =
        vec![Vec::new(); usize::try_from(y1 - y0 + 1).unwrap_or(0)];
    for (a, b) in segments(poly) {
        let capsule = Capsule::new(a, b, r);
        let y_lo = ((a[1].min(b[1]) - r - 0.5).ceil() as i64).max(y0);
        let y_hi = ((a[1].max(b[1]) + r - 0.5).floor() as i64).min(y1);
        for y in y_lo..=y_hi {
            let Some((lo, high)) = capsule.row(y as f64 + 0.5) else {
                continue;
            };
            let x_lo = ((lo - 0.5).ceil() as i64).max(0);
            let x_hi = ((high - 0.5).floor() as i64).min(wi - 1);
            if x_lo > x_hi {
                continue;
            }
            let row = &mut buckets[(y - y0) as usize];
            match row.last_mut() {
                Some(last) if x_lo <= last.1 + 1 && x_hi + 1 >= last.0 => {
                    *last = (last.0.min(x_lo), last.1.max(x_hi));
                }
                _ => row.push((x_lo, x_hi)),
            }
        }
    }
    let mut count = 0u64;
    for row in &mut buckets {
        row.sort_unstable();
        let mut merged: Vec<(i64, i64)> = Vec::with_capacity(row.len());
        for &(lo, high) in row.iter() {
            match merged.last_mut() {
                Some(last) if lo <= last.1 + 1 => last.1 = last.1.max(high),
                _ => merged.push((lo, high)),
            }
        }
        count += merged
            .iter()
            .map(|&(lo, high)| (high - lo + 1) as u64)
            .sum::<u64>();
        *row = merged;
    }
    if count > REFUSAL_CAP {
        return Err(format!(
            "the edge trace corridor covers {count} pixels, over the {REFUSAL_CAP}-pixel limit; \
             narrow the width or shorten the trail"
        ));
    }
    let mut rows = Vec::with_capacity(buckets.len());
    let mut pixels = Vec::with_capacity(count as usize);
    for (k, row) in buckets.iter().enumerate() {
        let start = pixels.len();
        for &(lo, high) in row {
            pixels.extend((lo..=high).map(|x| (x, y0 + k as i64)));
        }
        rows.push((start, pixels.len()));
    }
    // One two-pointer walk per adjacent row pair finds, per pixel, the first
    // index of the row above / below at or past x − 1; the neighbour table is
    // read off those cursors, so no neighbour lookup ever searches.
    let mut above = vec![NONE; pixels.len()];
    let mut below = vec![NONE; pixels.len()];
    for r in 0..rows.len() {
        if r > 0 {
            link(&pixels, rows[r], rows[r - 1], &mut above);
        }
        if let Some(&next) = rows.get(r + 1) {
            link(&pixels, rows[r], next, &mut below);
        }
    }
    let neighbours = (0..pixels.len())
        .map(|i| {
            let (x, y) = pixels[i];
            let [ul, up, ur] = near(&pixels, above[i], x, y - 1);
            let beside = |j: usize, side: i64| match pixels.get(j) {
                Some(&p) if p == (side, y) => j as u32,
                _ => NONE,
            };
            let left = i.checked_sub(1).map_or(NONE, |j| beside(j, x - 1));
            let right = beside(i + 1, x + 1);
            let [dl, down, dr] = near(&pixels, below[i], x, y + 1);
            [ul, up, ur, left, right, dl, down, dr]
        })
        .collect();
    Ok(Corridor {
        y0,
        rows,
        pixels,
        neighbours,
    })
}

/// The corridor indices of `(x − 1, y)`, `(x, y)` and `(x + 1, y)` from a
/// cursor at or before the first of them (`above` / `below`): corridor
/// indices run in raster order, so those three are among the cursor's next
/// three pixels.
fn near(pixels: &[(i64, i64)], cursor: u32, x: i64, y: i64) -> [u32; 3] {
    let mut out = [NONE; 3];
    let start = cursor as usize;
    for (k, &(px, py)) in pixels
        .get(start..)
        .unwrap_or(&[])
        .iter()
        .take(3)
        .enumerate()
    {
        if py == y && (x - 1..=x + 1).contains(&px) {
            out[(px - x + 1) as usize] = (start + k) as u32;
        }
    }
    out
}

/// For each pixel of the `here` row, the first index of the `there` row
/// whose x is at least the pixel's x − 1 (a two-pointer walk).
fn link(pixels: &[(i64, i64)], here: (usize, usize), there: (usize, usize), out: &mut [u32]) {
    let mut j = there.0;
    for i in here.0..here.1 {
        let x = pixels[i].0;
        while j < there.1 && pixels[j].0 < x - 1 {
            j += 1;
        }
        out[i] = j as u32;
    }
}

pub(crate) fn pixel_of(p: [f64; 2], w: u32, h: u32) -> (i64, i64) {
    (
        (p[0].floor() as i64).min(i64::from(w) - 1),
        (p[1].floor() as i64).min(i64::from(h) - 1),
    )
}

/// The chamfer distance (orthogonal 1, diagonal √2) of every corridor pixel
/// from the polyline's rasterized pixels: a two-pass multi-source raster scan
/// over the corridor, O(corridor px). Unreached pixels stay infinite.
pub(crate) fn trail_distance(c: &Corridor, poly: &[[f64; 2]], w: u32, h: u32) -> Vec<f32> {
    let mut d = vec![f32::INFINITY; c.pixels.len()];
    for (a, b) in segments(poly) {
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let steps = (len * 2.0).ceil().max(1.0) as usize;
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let (x, y) = pixel_of([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], w, h);
            if let Some(i) = c.index(x, y) {
                d[i] = 0.0;
            }
        }
    }
    // Forward: the four neighbours before the pixel in raster order;
    // backward: the four after it.
    for forward in [true, false] {
        let slots = if forward { 0..4 } else { 4..8 };
        for k in 0..d.len() {
            let i = if forward { k } else { d.len() - 1 - k };
            let mut best = d[i];
            for slot in slots.clone() {
                let q = c.neighbours[i][slot];
                if q != NONE {
                    best = best.min(d[q as usize] + STEPS[slot] as f32);
                }
            }
            d[i] = best;
        }
    }
    d
}
