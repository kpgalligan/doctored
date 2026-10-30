//! The ONE clamped-edge sliding box sum, streamed a row at a
//! time. `BoxRows` takes input rows in order (`push`) and hands back output
//! rows in order (`emit`) as soon as the rows they read are in, holding only a
//! ring of `min(2r+2, h)` horizontal sums and one running column — never a
//! whole n×8 plane. That is what lets Reduce Noise's guided filter keep its
//! binary64 window statistics at 24 MP: each of its box sums is a ring, and a
//! later stage consumes a row the moment the earlier one emits it.
//!
//! The arithmetic is the whole-plane box sum's, bit for bit, and must stay so:
//! every Reduce Noise and Box Blur output is pinned against it
//! (`tests/banded_reduce_noise.rs`). Each horizontal row is seeded `0.0`
//! plus the `2r+1` clamped reads in order, then slides by subtracting the
//! sample that leaves and adding the one that enters; the column is seeded
//! `0.0` plus the `2r+1` clamped horizontal rows in order, then slides by
//! adding the DIFFERENCE `H[add] − H[drop]` (difference first — summing the
//! two separately rounds differently). Re-seeding per row and once per plane
//! bounds the drift by one line rather than the whole plane.
//!
//! Why `2r + 2` ring rows: when output row `y` is emitted the column reads
//! input rows `y − 1 − r` (dropped) through `y + r` (added), `2r + 2` rows.
//! The next push writes row `y + r + 1` into the slot of row `y − r − 1`
//! only once `y` has been emitted, so no live row is ever overwritten; `push`
//! refuses rather than corrupting if a caller breaks that order.
//!
//! The window may be offset (`BoxRows::window`, `box_sum_window`): output
//! `(x, y)` sums inputs `x − before ..= x + after` by `y − before ..= y +
//! after`, seeded in the order `−before ..= after`, sliding by dropping `x − 1
//! − before` and adding `x + after`, on a ring of `min(before + after + 2, h)`
//! rows. The inner glow's two-pass box of an EVEN width `2m` is one `(m, m −
//! 1)` pass then one `(m − 1, m)` pass, so the pair is centred (#578). With
//! `before == after == r` every read, sum and order is the symmetric box's, so
//! `new` and `box_sum` are the `window` forms at `(r, r)`, bit for bit.
//! [`box_pair`] is that two-pass box, shared by the inner glow's law and the
//! Smooth bevels' (`style_bevel_smooth`, #578).
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;

/// A streamed clamped-edge box sum over a `w × h` scalar field, reading
/// `before` samples behind and `after` ahead on each axis (`r`, `r` for the
/// symmetric box of radius `r`).
pub(crate) struct BoxRows {
    w: usize,
    h: usize,
    before: usize,
    after: usize,
    slots: usize,
    ring: Vec<f64>,
    column: Vec<f64>,
    pushed: usize,
    emitted: usize,
}

impl BoxRows {
    /// `w, h ≥ 1`. Allocates `min(2r+2, h)` ring rows and the column row.
    pub(crate) fn new(w: usize, h: usize, r: usize) -> Result<Self, String> {
        Self::window(w, h, r, r)
    }

    /// The offset window: rows and columns `y − before ..= y + after`.
    /// Allocates `min(before + after + 2, h)` ring rows and the column row.
    pub(crate) fn window(w: usize, h: usize, before: usize, after: usize) -> Result<Self, String> {
        if w == 0 || h == 0 {
            return Err("box sum needs a non-empty plane".into());
        }
        let slots = before.saturating_add(after).saturating_add(2).min(h);
        let len = slots
            .checked_mul(w)
            .ok_or("could not allocate precision output")?;
        let mut ring: Vec<f64> = reserved_vec(len)?;
        ring.resize(len, 0.0);
        let mut column: Vec<f64> = reserved_vec(w)?;
        column.resize(w, 0.0);
        Ok(Self {
            w,
            h,
            before,
            after,
            slots,
            ring,
            column,
            pushed: 0,
            emitted: 0,
        })
    }

    /// What `new` allocates: `(min(2r+2, h) + 1)·w·8`, saturating.
    pub(crate) fn bytes(w: u64, h: u64, r: u64) -> u64 {
        r.saturating_mul(2)
            .saturating_add(2)
            .min(h)
            .saturating_add(1)
            .saturating_mul(w)
            .saturating_mul(8)
    }

    /// Whether the next output row can be emitted: every input row it reads
    /// (through `min(y + after, h − 1)`) is in.
    pub(crate) fn ready(&self) -> bool {
        self.emitted < self.h && self.pushed > (self.emitted + self.after).min(self.h - 1)
    }

    /// The lowest input row the next emission reads.
    fn lowest_needed(&self) -> usize {
        if self.emitted == 0 {
            0
        } else {
            (self.emitted - 1).saturating_sub(self.before)
        }
    }

    /// Horizontal sum of input row `pushed`; `read` takes the ABSOLUTE index
    /// `row·w + x`.
    pub(crate) fn push(
        &mut self,
        read: impl Fn(usize) -> f64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        if self.pushed >= self.h {
            return Err("box sum received more rows than its plane holds".into());
        }
        if self.pushed >= self.slots
            && self.emitted < self.h
            && self.pushed - self.slots >= self.lowest_needed()
        {
            return Err("box sum row pushed before the row it replaces was emitted".into());
        }
        job.check()?;
        let (w, wi) = (self.w, self.w as i64);
        let (bi, ai) = (self.before as i64, self.after as i64);
        let row = self.pushed * w;
        let slot = (self.pushed % self.slots) * w;
        let out = &mut self.ring[slot..slot + w];
        let mut acc = 0.0;
        for dx in -bi..=ai {
            acc += read(row + dx.clamp(0, wi - 1) as usize);
        }
        out[0] = acc;
        for (x, value) in out.iter_mut().enumerate().skip(1) {
            job.every(x)?;
            let xi = x as i64;
            acc -= read(row + (xi - 1 - bi).clamp(0, wi - 1) as usize);
            acc += read(row + (xi + ai).clamp(0, wi - 1) as usize);
            *value = acc;
        }
        self.pushed += 1;
        Ok(())
    }

    /// Output row `emitted`: the running column, valid until the next call.
    pub(crate) fn emit(&mut self, job: JobContext<'_>) -> Result<&[f64], String> {
        if !self.ready() {
            return Err("box sum row emitted before its input rows".into());
        }
        job.check()?;
        let (w, hi) = (self.w, self.h as i64);
        let (bi, ai) = (self.before as i64, self.after as i64);
        let slot = |row: usize| (row % self.slots) * w;
        let y = self.emitted;
        if y == 0 {
            for dy in -bi..=ai {
                let at = slot(dy.clamp(0, hi - 1) as usize);
                for (x, value) in self.column.iter_mut().enumerate() {
                    *value += self.ring[at + x];
                }
            }
        } else {
            let drop = slot((y as i64 - 1 - bi).clamp(0, hi - 1) as usize);
            let add = slot((y as i64 + ai).clamp(0, hi - 1) as usize);
            for (x, value) in self.column.iter_mut().enumerate() {
                *value += self.ring[add + x] - self.ring[drop + x];
            }
        }
        self.emitted += 1;
        Ok(&self.column)
    }
}

/// The whole plane (Box Blur's form): every row pushed and emitted in order,
/// collecting the `w·h` sums.
pub(crate) fn box_sum(
    read: impl Fn(usize) -> f64,
    w: usize,
    h: usize,
    r: usize,
    job: JobContext<'_>,
) -> Result<Vec<f64>, String> {
    box_sum_window(read, w, h, r, r, job)
}

/// `box_sum` over the offset window `−before ..= after` on each axis.
pub(crate) fn box_sum_window(
    read: impl Fn(usize) -> f64,
    w: usize,
    h: usize,
    before: usize,
    after: usize,
    job: JobContext<'_>,
) -> Result<Vec<f64>, String> {
    window_rows(read, w, h, before, after, job, false)
}

/// [`box_sum_window`]'s loop; with `report`, each emitted row also reports
/// `rows / h` on `job`'s range at stage 6 ([`box_pair`]'s passes).
fn window_rows(
    read: impl Fn(usize) -> f64,
    w: usize,
    h: usize,
    before: usize,
    after: usize,
    job: JobContext<'_>,
    report: bool,
) -> Result<Vec<f64>, String> {
    let mut rows = BoxRows::window(w, h, before, after)?;
    let mut out: Vec<f64> = reserved_vec(w * h)?;
    let mut emitted = 0usize;
    for _ in 0..h {
        rows.push(&read, job)?;
        while rows.ready() {
            out.extend_from_slice(rows.emit(job)?);
            emitted += 1;
            if report {
                job.report(emitted as f64 / h as f64, 6)?;
            }
        }
    }
    Ok(out)
}

/// Two passes of a `width`-wide box over `read(i)` (`i` the absolute index
/// `row·w + x`), each divided by `width²`: per axis the triangle `(width −
/// |j|) / width²`, unit sum, edge-clamped. An odd width `2k + 1` is the `(k,
/// k)` window twice; an even width `2m` is `(m, m − 1)` then `(m − 1, m)`, so
/// the pair is centred. The sums are [`box_sum_window`]'s, the crate's one
/// sliding box sum (the master); the law runners' `box_pair`
/// (`scripts/photoshop-probe/inner_glow_578_law_fit.py`) is the model it
/// mirrors. `width ≥ 1`. Reports per output row at stage 6 as the Gaussian
/// it replaced did (`gaussian_plane::feather`): the first pass over `0 ..
/// 1/2` of `job`'s range, the second over `1/2 .. 1`.
pub(crate) fn box_pair(
    read: impl Fn(usize) -> f64,
    w: usize,
    h: usize,
    width: u32,
    job: JobContext<'_>,
) -> Result<Vec<f64>, String> {
    let half = (width / 2) as usize;
    let (first, second) = if width % 2 == 1 {
        ((half, half), (half, half))
    } else {
        ((half, half - 1), (half - 1, half))
    };
    let area = f64::from(width) * f64::from(width);
    let (a, b) = (job.range(0.0, 0.5), job.range(0.5, 1.0));
    let mut pass = window_rows(read, w, h, first.0, first.1, a, true)?;
    pass.iter_mut().for_each(|v| *v /= area);
    job.check()?;
    let mut out = window_rows(|i| pass[i], w, h, second.0, second.1, b, true)?;
    out.iter_mut().for_each(|v| *v /= area);
    Ok(out)
}
