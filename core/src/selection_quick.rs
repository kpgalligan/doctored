//! The Quick Selection kernel: a sized brush, growth through similar
//! colour that stops at edges, and enclosed-hole fill. Free functions over a
//! Lab plane; `selection_tools::quick_samples` (byte and u16) is the adapter
//! that converts the captured projection and reserves the scratch.
//!
//! The stages, in order:
//!
//! 1. PRE-SMOOTHING (`smooth` s > 0). Each opaque Lab sample becomes the mean
//!    of the opaque samples in its (2s+1)² window, clipped to the canvas,
//!    whose squared distance from that sample's ORIGINAL value is at most
//!    `edge`. The gate is the growth's own step rule, and it is why this is
//!    not a plain box mean: a box mean puts the column beside a hard edge a
//!    third of the way to the other colour, far above `edge`, and the growth
//!    then stops one pixel short of the edge. Gated, a hard edge is not
//!    blurred across and noise within `edge` is averaged away. It runs in
//!    place over a ring of `s + 1` saved original rows (rows below the current
//!    one are still original).
//! 2. SEEDING. Every opaque pixel whose centre is within `radius` of a seed
//!    centre is selected and anchors its own growth, in seed order and then
//!    row-major. Radius 0 is the seed pixel alone. The summed disc boxes are
//!    bounded by [`MAX_DISC_VISITS`] ([`disc_visits`]).
//! 3. GROWTH. Bounded four-connected breadth-first growth: a neighbour joins
//!    when its local step from the claiming pixel is at most `edge` AND its
//!    distance from that pixel's anchor is at most `tolerance` (both squared
//!    ΔE76). The first claim wins; transparent pixels are barriers.
//! 4. HOLE FILL (`radius` > 0). A four-connected set of unselected opaque
//!    pixels is selected, with its own alpha as coverage, only when it holds
//!    at most ⌈π·radius²⌉ pixels (a brush dab's area: a larger gap is a region
//!    the user did not paint), touches no canvas border, and every
//!    four-neighbour outside the set is a selected pixel. A set with any
//!    transparent neighbour is therefore an island, never a hole.
//!
//! With `radius` 0 and `smooth` 0 only stage 3 runs from the seed pixels, and
//! the output and the checkpoint sequence are the earlier seed-only growth's.
//!
//! MEMORY. The caller reserves the larger of the two stages' peaks, plus
//! 1 MiB: `max(n·(20 + size_of::<T>()), 12·n + 12·w·min(s + 1, h))`. The first
//! is growth: this Lab plane (12n), the exact-capacity queue (8n) and the
//! output. The second is pre-smoothing: the Lab plane and the ring of
//! `min(s + 1, h)` saved rows, which is freed before the queue and the output
//! are allocated (with `s` 0 there is no ring and the term is 12n). The Lab
//! plane is dropped before stage 4, whose visited bitset (n/8) lives in that
//! freed budget and whose component list reuses the queue.
use crate::job_control::JobContext;
use crate::selection_tools::SelectionSample;

/// Parsed Quick Selection parameters. `tolerance` and `edge` are SQUARED ΔE76;
/// `radius` is in canvas pixels; `seeds` are row-major pixel indexes.
pub(crate) struct QuickParameters {
    pub tolerance: f32,
    pub edge: f32,
    pub radius: f32,
    pub smooth: u32,
    pub seeds: Vec<usize>,
}

/// The cap on the summed clipped disc boxes of one call's seeds. A stroke
/// seeds a disc at every kept point, so the seeding pass costs seeds × brush
/// size² in the worst case; 64 million box pixels is a fraction of a second
/// and admits 700 seeds of a 300-pixel brush. The host thins a drag to a
/// quarter of the brush size, so an ordinary stroke stays far below it.
pub(crate) const MAX_DISC_VISITS: u64 = 64_000_000;

fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// The clipped box `[x0, x1) × [y0, y1)` of the disc of `radius` around the
/// pixel `seed`.
fn disc_box(seed: usize, (w, h): (usize, usize), radius: f32) -> [usize; 4] {
    let reach = radius.floor() as usize;
    let (x, y) = (seed % w, seed / w);
    [
        x.saturating_sub(reach),
        (x + reach + 1).min(w),
        y.saturating_sub(reach),
        (y + reach + 1).min(h),
    ]
}

/// The summed clipped disc boxes of `seeds`, or the refusal naming the brush
/// size (`2·radius + 1`) and the seed count when they pass
/// [`MAX_DISC_VISITS`]. Called before any pixel work.
pub(crate) fn disc_visits(
    dimensions: (u32, u32),
    seeds: &[usize],
    radius: f32,
) -> Result<u64, String> {
    let size = (dimensions.0 as usize, dimensions.1 as usize);
    let mut visits = 0_u64;
    for &seed in seeds {
        let [x0, x1, y0, y1] = disc_box(seed, size, radius);
        visits = visits.saturating_add((x1 - x0) as u64 * (y1 - y0) as u64);
        if visits > MAX_DISC_VISITS {
            return Err(format!(
                "Quick Selection brush size {} with {} seeds exceeds the limit of 64 million brush pixels; use a smaller brush or fewer seeds",
                2.0 * radius + 1.0,
                seeds.len()
            ));
        }
    }
    Ok(visits)
}

/// Stage 1 (module doc), in place.
fn smooth<T: SelectionSample>(
    colors: &mut [[f32; 3]],
    (w, h): (usize, usize),
    alpha_at: &impl Fn(usize) -> T,
    reach: usize,
    edge: f32,
    job: JobContext<'_>,
) -> Result<(), String> {
    let rows = (reach + 1).min(h);
    // `ring[(y % rows)·w ..]` holds the original row `y` for the last `rows` rows.
    let mut ring = vec![[0.0_f32; 3]; rows * w];
    for y in 0..h {
        ring[(y % rows) * w..(y % rows + 1) * w].copy_from_slice(&colors[y * w..(y + 1) * w]);
        for x in 0..w {
            job.every(y * w + x)?;
            if alpha_at(y * w + x).code() == 0. {
                continue;
            }
            let centre = ring[(y % rows) * w + x];
            let (mut sum, mut count) = ([0.0_f32; 3], 0.0_f32);
            for yy in y.saturating_sub(reach)..=(y + reach).min(h - 1) {
                for xx in x.saturating_sub(reach)..=(x + reach).min(w - 1) {
                    if alpha_at(yy * w + xx).code() == 0. {
                        continue;
                    }
                    let sample = if yy <= y {
                        ring[(yy % rows) * w + xx]
                    } else {
                        colors[yy * w + xx]
                    };
                    if distance_squared(sample, centre) <= edge {
                        for c in 0..3 {
                            sum[c] += sample[c];
                        }
                        count += 1.0;
                    }
                }
            }
            // The centre always passes its own gate, so `count` is at least 1.
            colors[y * w + x] = sum.map(|v| v / count);
        }
    }
    Ok(())
}

/// Stage 4 (module doc). `list` is the growth's queue, reused as the
/// component list; it is left empty.
fn fill_holes<T: SelectionSample>(
    out: &mut [T],
    (w, h): (usize, usize),
    alpha_at: &impl Fn(usize) -> T,
    radius: f32,
    list: &mut Vec<(u32, u32)>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let n = w * h;
    let cap = (std::f64::consts::PI * f64::from(radius) * f64::from(radius)).ceil() as usize;
    let mut visited = vec![0_u8; n.div_ceil(8)];
    let open = |out: &[T], i: usize| out[i].code() == 0. && alpha_at(i).code() > 0.;
    for start in 0..n {
        job.every(start)?;
        if visited[start / 8] & (1 << (start % 8)) != 0 || !open(out, start) {
            continue;
        }
        list.clear();
        list.push((start as u32, 0));
        visited[start / 8] |= 1 << (start % 8);
        let mut hole = true;
        let mut head = 0;
        while head < list.len() {
            job.every(head)?;
            let index = list[head].0 as usize;
            head += 1;
            let (x, y) = (index % w, index / w);
            if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                hole = false;
            }
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx >= w || ny >= h {
                    continue;
                }
                let next = ny * w + nx;
                if out[next].code() != 0. {
                    continue;
                }
                if alpha_at(next).code() == 0. {
                    // A transparent neighbour: an island, not a hole.
                    hole = false;
                    continue;
                }
                if visited[next / 8] & (1 << (next % 8)) == 0 {
                    visited[next / 8] |= 1 << (next % 8);
                    list.push((next as u32, 0));
                }
            }
        }
        if hole && list.len() <= cap {
            for &(index, _) in list.iter() {
                out[index as usize] = alpha_at(index as usize);
            }
        }
    }
    list.clear();
    Ok(())
}

/// Quick Selection over the Lab plane `colors` (one sample per canvas pixel,
/// row-major; samples at transparent pixels are never read as colour):
/// the module doc's four stages. `alpha_at(i)` is pixel `i`'s source alpha,
/// which becomes its coverage when selected. `Err`: the [`disc_visits`]
/// refusal, or a cancelled job.
pub(crate) fn grow<T: SelectionSample>(
    mut colors: Vec<[f32; 3]>,
    dimensions: (u32, u32),
    alpha_at: impl Fn(usize) -> T,
    parameters: &QuickParameters,
    job: JobContext<'_>,
) -> Result<Vec<T>, String> {
    let (w, h) = dimensions;
    let size = (w as usize, h as usize);
    let n = size.0 * size.1;
    let (tolerance, edge, radius) = (parameters.tolerance, parameters.edge, parameters.radius);
    disc_visits(dimensions, &parameters.seeds, radius)?;
    if parameters.smooth > 0 {
        smooth(
            &mut colors,
            size,
            &alpha_at,
            parameters.smooth as usize,
            edge,
            job,
        )?;
    }
    let mut out = vec![T::zero(); n];
    let mut queue: Vec<(u32, u32)> = Vec::with_capacity(n);
    for &seed in &parameters.seeds {
        if radius <= 0.0 {
            if out[seed].code() == 0. && alpha_at(seed).code() > 0. {
                out[seed] = alpha_at(seed);
                queue.push((seed as u32, seed as u32));
            }
            continue;
        }
        job.check()?;
        let [x0, x1, y0, y1] = disc_box(seed, size, radius);
        let (cx, cy) = ((seed % size.0) as f32, (seed / size.0) as f32);
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                let at = y * size.0 + x;
                if dx * dx + dy * dy <= radius * radius
                    && out[at].code() == 0.
                    && alpha_at(at).code() > 0.
                {
                    out[at] = alpha_at(at);
                    queue.push((at as u32, at as u32));
                }
            }
        }
    }
    let mut head = 0;
    while head < queue.len() {
        job.every(head)?;
        if head & 65535 == 0 {
            job.report(0.45 + 0.55 * head as f64 / n as f64, 6)?;
        }
        let (index, anchor) = queue[head];
        head += 1;
        let (x, y) = (index % w, index / w);
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx >= w || ny >= h {
                continue;
            }
            let next = ny as usize * w as usize + nx as usize;
            let alpha = alpha_at(next);
            if out[next].code() != 0. || alpha.code() == 0. {
                continue;
            }
            if distance_squared(colors[next], colors[index as usize]) <= edge
                && distance_squared(colors[next], colors[anchor as usize]) <= tolerance
            {
                out[next] = alpha;
                queue.push((next as u32, anchor));
            }
        }
    }
    drop(colors);
    if radius > 0.0 {
        fill_holes(&mut out, size, &alpha_at, radius, &mut queue, job)?;
    }
    job.report(1.0, 5)?;
    Ok(out)
}
