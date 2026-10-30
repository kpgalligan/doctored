//! The Magnetic Lasso's live-wire edge tracer: given where the
//! last fastened anchor is (`from`), the pointer's recent positions (`trail`)
//! and the tool's Width / Contrast / Frequency options, find the cheapest
//! 8-connected pixel path from `from` to the end, preferring strong edges,
//! and say where along it the host should fasten a new anchor.
//!
//! The pipeline, each step bounded by the corridor's pixel count:
//!
//! 1. **Corridor.** Only pixels whose centre lies within `width` of the
//!    polyline `from → trail… → (explicit end)` are considered — the Width
//!    option. Each segment's capsule is stamped as one exact x-interval
//!    per pixel row it touches, then the intervals are sorted and merged per
//!    row, so the work is O(Σ(segment length + 2·width) + corridor px) and
//!    never a per-pixel × per-segment test (`edge_trace_corridor`, split out
//!    for the module size cap). A corridor above [`REFUSAL_CAP`] refuses
//!    before any pixel is read.
//! 2. **Edge strength** `s(q)` in 0…1: the largest Sobel magnitude (the one
//!    implementation, `ops_filters::sobel_gradient`) over R·a, G·a, B·a and A
//!    in unit range, with replicated borders, divided by 4 — a full unit
//!    step's Sobel magnitude is exactly 4 (`1 + 2 + 1`), so a black/white or
//!    opaque/clear step reads 1. Colour is weighted by alpha so a colour
//!    change under fully transparent pixels is not an edge, and alpha is its
//!    own channel so a cutout's silhouette attracts.
//! 3. **Trail distance** `d(q)`: the chamfer distance (orthogonal 1, diagonal
//!    √2) from the polyline's rasterized pixels, by a two-pass multi-source
//!    raster scan over the corridor (O(corridor px), `edge_trace_corridor`).
//!    A pixel the scan cannot reach counts as `width` away.
//! 4. **Cost.** With `t = contrast / 100` and `c_max` the strongest `s` in the
//!    corridor, a step of length ℓ (1 or √2) into `q` costs
//!    `ℓ·(LAMBDA + (s(q) ≥ t ? 1 − s(q)/c_max : 1) + TRAIL_PULL·d(q)/width)`.
//!    Normalizing by `c_max` makes the corridor's own strongest edge the
//!    cheapest, so a soft photo edge attracts as firmly as a hard synthetic
//!    one; Contrast is the floor below which a gradient is not an edge at
//!    all. [`LAMBDA`] keeps a perfect edge pixel from being free, so path
//!    length still matters and the path does not wander through edge clutter.
//!    [`TRAIL_PULL`] only breaks the octile ties of a flat stretch toward the
//!    trail: at ≤ 0.02 per step it can never pay for a detour step (≥ 1.05).
//! 5. **Dijkstra**, 8-connected inside the corridor, heap ordered by
//!    `(cost, pixel index)` so equal-cost paths resolve deterministically.
//!    The end is the explicit point's pixel, or for `"snap"` the strongest
//!    pixel with `s ≥ t` whose centre is within `width` of the trail's last
//!    point (ties: nearer, then lower pixel index), else that point's pixel.
//! 6. **Fasten.** [`fasten_distance`] maps Frequency to a path length `D`; the
//!    fasten pixel is the first path pixel whose cumulative length is ≥ `D`
//!    and whose remaining length is ≥ `min(width, 32)` — an anchor fastened
//!    right under the pointer would jump as the pointer moves. A trail that
//!    goes more than `2·width` from `from`, comes back within `2·width` of
//!    it and encloses an area (shoelace, `from → trail… → from`) of at
//!    least `width ×` its farthest distance — so an overshoot retraced back
//!    is not one — is a loop, at any Frequency, 0 included: the shortest
//!    path home would cut the loop off, so the trace runs as two legs — `from` to the
//!    trail point farthest from `from` (snapping there, so the junction sits
//!    on the edge), then on from the junction pixel to the end — joined
//!    there, and the junction is a fasten candidate under the same
//!    tail rule; the earlier of it and the `D` fasten wins. The host fastens
//!    against [`WORK_BUDGET`] too, from its own corridor estimate
//!    `π·width² + 2·width·trail length`; at Width ≥ ~205 the disc alone
//!    exceeds the budget, so every pointer move fastens whatever the
//!    Frequency, until Part B's cached per-capture edge map lifts the
//!    budget.
//! 7. **Output.** Pixel centres (`i + 0.5`), collinear runs dropped, then
//!    Douglas–Peucker at 0.5 px — half a pixel, below the pixel-centre
//!    quantization the path already has — run separately on `[0, fasten]`
//!    and `[fasten, end]` so the start, the end and the fasten vertex survive.
//!
//! Strict JSON `{from, trail, end, width, contrast, frequency}`; edge strength is
//! `ops_filters::sobel_gradient` (the one Sobel, shared with Find Edges) over R·a, G·a, B·a, A; an
//! exact `(cost, pixel index)`-ordered Dijkstra (a bucket ring, not a heap — same order);
//! Frequency's fasten distance and the Douglas–Peucker output. Every numeric choice's why is in
//! this `//!` doc. Pure and deterministic; tests `core/tests/edge_trace.rs` (its timing probe runs
//! in `make cancel-bench`).

use crate::edge_trace_corridor::{corridor, pixel_of, trail_distance, Corridor, NONE, STEPS};
use crate::ops_filters::sobel_gradient;
use crate::selection_tools::{number, object};
use image::RgbaImage;
use serde_json::Value;

/// Corridor pixels per pointer move the host fastens against before tracing
/// further (`DR_EDGE_TRACE_WORK_BUDGET` in the C header, which hosts read).
/// 2¹⁷ px: the `make cancel-bench` probe holds one trace of a corridor this
/// size under its declared 16 ms (one frame), so hover tracing keeps pace.
pub(crate) const WORK_BUDGET: u64 = 131_072;
/// The largest corridor a single trace accepts: eight work budgets, room for
/// the one long segment a host may send past the budget (a straight click
/// across the image), while a runaway request refuses before allocating.
/// A looping trace (module doc, step 6) checks it per leg.
pub(crate) const REFUSAL_CAP: u64 = 8 * WORK_BUDGET;
/// The per-unit-length floor of every step's cost (see the module doc).
pub(crate) const LAMBDA: f64 = 0.05;
/// The trail-distance weight per unit length (see the module doc).
pub(crate) const TRAIL_PULL: f64 = 0.02;
/// Trail points per request, the `dr_selection_quick` seed bound.
const MAX_TRAIL: usize = 4096;
/// Douglas–Peucker tolerance in pixels.
const SIMPLIFY_TOLERANCE: f64 = 0.5;
/// The fasten tail never needs more than this many pixels of path beyond it.
const TAIL_CAP: f64 = 32.0;

/// Where the trace ends.
pub(crate) enum TraceEnd {
    /// The strongest qualifying edge pixel near the trail's last point.
    Snap,
    /// Exactly this point's pixel.
    Point([f64; 2]),
}

/// A parsed, validated trace request.
pub(crate) struct TraceParams {
    from: [f64; 2],
    trail: Vec<[f64; 2]>,
    end: TraceEnd,
    width: f64,
    contrast: f64,
    frequency: f64,
}

/// A traced path.
pub(crate) struct EdgeTrace {
    /// Path vertices at pixel centres, `from`'s pixel first.
    pub(crate) vertices: Vec<[f32; 2]>,
    /// The vertex the host should fasten at, if any.
    pub(crate) fasten: Option<usize>,
    /// The corridor's pixel count, the work this trace did.
    pub(crate) corridor_px: u64,
}

/// Parses the strict JSON request `{from, trail, end, width, contrast,
/// frequency}` against a `dims` image. Unknown keys, out-of-range numbers,
/// a trail outside 1…4096 points and coordinates outside `[0,w]×[0,h]`
/// refuse.
pub(crate) fn parse(json: &str, dims: (u32, u32)) -> Result<TraceParams, String> {
    let map = object(
        json,
        &["from", "trail", "end", "width", "contrast", "frequency"],
    )?;
    if dims.0 == 0 || dims.1 == 0 {
        return Err("edge trace source is empty".into());
    }
    let (w, h) = (f64::from(dims.0), f64::from(dims.1));
    let point = |value: &Value, what: &str| -> Result<[f64; 2], String> {
        let pair = value
            .as_array()
            .filter(|pair| pair.len() == 2)
            .ok_or_else(|| format!("{what} must be an [x, y] pair"))?;
        let (x, y) = match (pair[0].as_f64(), pair[1].as_f64()) {
            (Some(x), Some(y)) => (x, y),
            _ => return Err(format!("{what} must be an [x, y] pair of numbers")),
        };
        if !((0.0..=w).contains(&x) && (0.0..=h).contains(&y)) {
            return Err(format!(
                "{what} ({x}, {y}) lies outside the {}×{} image",
                dims.0, dims.1
            ));
        }
        Ok([x, y])
    };
    let from = point(map.get("from").ok_or("from is required")?, "from")?;
    let trail = map
        .get("trail")
        .and_then(Value::as_array)
        .ok_or("trail must be an array of [x, y] pairs")?;
    if trail.is_empty() || trail.len() > MAX_TRAIL {
        return Err(format!(
            "trail must hold 1 to {MAX_TRAIL} points, not {}",
            trail.len()
        ));
    }
    let trail = trail
        .iter()
        .map(|p| point(p, "trail point"))
        .collect::<Result<Vec<_>, _>>()?;
    let end = match map.get("end") {
        Some(Value::String(s)) if s == "snap" => TraceEnd::Snap,
        Some(value @ Value::Array(_)) => TraceEnd::Point(point(value, "end")?),
        _ => return Err("end must be \"snap\" or an [x, y] pair".into()),
    };
    Ok(TraceParams {
        from,
        trail,
        end,
        width: f64::from(number(&map, "width", 10.0, 1.0, 256.0)?),
        contrast: f64::from(number(&map, "contrast", 10.0, 1.0, 100.0)?),
        frequency: f64::from(number(&map, "frequency", 57.0, 0.0, 100.0)?),
    })
}

/// Frequency → the path length after which the tracer fastens: none at 0
/// (where only a loop's junction fastens, module doc step 6), then
/// log-linear from 800 px at 1 to 40 px at the default 57 and on to 8 px
/// at 100. Log-linear because the slider's useful range spans two orders of
/// magnitude; 8 px at 100 still leaves a few simplified vertices between
/// anchors.
pub(crate) fn fasten_distance(frequency: f64) -> Option<f64> {
    if frequency <= 0.0 {
        return None;
    }
    let lerp = |a: f64, b: f64, t: f64| (a.ln() + (b.ln() - a.ln()) * t.clamp(0.0, 1.0)).exp();
    Some(if frequency <= 57.0 {
        lerp(800.0, 40.0, (frequency - 1.0) / 56.0)
    } else {
        lerp(40.0, 8.0, (frequency - 57.0) / 43.0)
    })
}

/// Edge strength of every corridor pixel (module doc, step 2). The 3×3
/// neighbourhood slides along each run of consecutive pixels in a row, so
/// each step converts one new column of samples rather than nine pixels.
fn strengths(raw: &[u8], w: i64, h: i64, pixels: &[(i64, i64)]) -> Vec<f32> {
    // One column's samples at rows y − 1, y, y + 1 per channel (R·a, G·a,
    // B·a, A in unit range), borders replicated.
    let column = |x: i64, y: i64| {
        let mut out = [[0f32; 3]; 4];
        let px = x.clamp(0, w - 1);
        for (row, dy) in (-1..=1).enumerate() {
            let i = (((y + dy).clamp(0, h - 1) * w + px) * 4) as usize;
            let a = f32::from(raw[i + 3]) / 255.0;
            for (c, plane) in out.iter_mut().enumerate() {
                plane[row] = if c == 3 {
                    a
                } else {
                    f32::from(raw[i + c]) / 255.0 * a
                };
            }
        }
        out
    };
    // Per channel, the three columns x − 1, x, x + 1 of three rows each.
    let mut window = [[[0f32; 3]; 3]; 4];
    let mut last: Option<(i64, i64)> = None;
    let mut out = Vec::with_capacity(pixels.len());
    for &(x, y) in pixels {
        if last == Some((x - 1, y)) {
            let next = column(x + 1, y);
            for (plane, new) in window.iter_mut().zip(next) {
                *plane = [plane[1], plane[2], new];
            }
        } else {
            let columns = [column(x - 1, y), column(x, y), column(x + 1, y)];
            for (c, plane) in window.iter_mut().enumerate() {
                *plane = [columns[0][c], columns[1][c], columns[2][c]];
            }
        }
        last = Some((x, y));
        let mut best = 0f32;
        for plane in &window {
            let [gx, gy] = sobel_gradient(|dx, dy| plane[(dx + 1) as usize][(dy + 1) as usize]);
            best = best.max(gx * gx + gy * gy);
        }
        out.push((best.sqrt() / 4.0).min(1.0));
    }
    out
}

/// Traces one live-wire path (module doc). Pure: reads `image` only.
pub(crate) fn trace(image: &RgbaImage, p: &TraceParams) -> Result<EdgeTrace, String> {
    // A trail that turned back toward `from` is traced as two legs joined
    // at its turning point (module doc, step 6), so the loop is not cut
    // short by the shortest way home.
    let (path, corridor_px, junction) = match loop_turn(p) {
        Some(far) => {
            // The out leg snaps near the turning point like any live wire,
            // so the junction sits on the edge rather than out on the trail;
            // the way back starts from that very pixel.
            let (mut path, out) = trace_leg(image, p, p.from, &p.trail[..=far], &TraceEnd::Snap)?;
            let junction = path.len() - 1;
            let turn = [path[junction].0 as f64 + 0.5, path[junction].1 as f64 + 0.5];
            let (back, home) = trace_leg(image, p, turn, &p.trail[far + 1..], &p.end)?;
            path.extend(back.into_iter().skip(1));
            (path, out + home, Some(junction))
        }
        None => {
            let (path, n) = trace_leg(image, p, p.from, &p.trail, &p.end)?;
            (path, n, None)
        }
    };
    // Cumulative path length at each pixel after the first.
    let lengths: Vec<f64> = path
        .windows(2)
        .scan(0.0, |total, pair| {
            let diagonal = pair[0].0 != pair[1].0 && pair[0].1 != pair[1].1;
            *total += if diagonal {
                std::f64::consts::SQRT_2
            } else {
                1.0
            };
            Some(*total)
        })
        .collect();
    let total = lengths.last().copied().unwrap_or(0.0);
    // Path pixel `k` (≥ 1) leaves a tail long enough to fasten at.
    let has_tail = |k: usize| k > 0 && total - lengths[k - 1] >= p.width.min(TAIL_CAP);
    let by_frequency = fasten_distance(p.frequency).and_then(|limit| {
        let k = lengths.iter().position(|&l| l >= limit)? + 1;
        has_tail(k).then_some(k)
    });
    let fasten = by_frequency
        .into_iter()
        .chain(junction.filter(|&k| has_tail(k)))
        .min();
    let (vertices, fasten) = match fasten {
        Some(k) => {
            let head = simplify(&path[..=k]);
            let rest = simplify(&path[k..]);
            let at = head.len() - 1;
            (
                head.into_iter().chain(rest.into_iter().skip(1)).collect(),
                Some(at),
            )
        }
        None => (simplify(&path), None),
    };
    Ok(EdgeTrace {
        vertices: vertices
            .into_iter()
            .map(|(x, y)| [x as f32 + 0.5, y as f32 + 0.5])
            .collect(),
        fasten,
        corridor_px,
    })
}

/// The trail index to split a looping trail at (module doc, step 6): the
/// point farthest from `from`, when it lies more than `2·width` away, the
/// trail's last point is back within `2·width` of `from`, the farthest
/// point is not the last, and the closed polygon `from → trail… → from`
/// encloses at least `width ×` that farthest distance — so a pointer that
/// overshot and backed up along its own path is not a loop.
fn loop_turn(p: &TraceParams) -> Option<usize> {
    let reach = |q: [f64; 2]| ((q[0] - p.from[0]).powi(2) + (q[1] - p.from[1]).powi(2)).sqrt();
    let (far, distance) =
        p.trail
            .iter()
            .enumerate()
            .fold((0, f64::NEG_INFINITY), |best, (i, &q)| {
                let d = reach(q);
                if d > best.1 {
                    (i, d)
                } else {
                    best
                }
            });
    let back = 2.0 * p.width;
    let last = p.trail.len() - 1;
    // Twice the shoelace area of the closed polygon `from → trail… → from`:
    // a pointer that backed up along its own path encloses next to nothing.
    let ring = || std::iter::once(&p.from).chain(&p.trail);
    let twice_area = ring()
        .zip(ring().skip(1).chain(std::iter::once(&p.from)))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>()
        .abs();
    let encloses = twice_area / 2.0 >= p.width * distance;
    (distance > back && reach(p.trail[last]) <= back && far < last && encloses).then_some(far)
}

/// One Dijkstra leg `from → trail… → end` (module doc, steps 1–5),
/// answering the path's pixels and its corridor's pixel count.
fn trace_leg(
    image: &RgbaImage,
    p: &TraceParams,
    from: [f64; 2],
    trail: &[[f64; 2]],
    end: &TraceEnd,
) -> Result<(Vec<(i64, i64)>, u64), String> {
    let (w, h) = image.dimensions();
    let (wi, hi) = (i64::from(w), i64::from(h));
    let mut poly = Vec::with_capacity(trail.len() + 2);
    poly.push(from);
    poly.extend_from_slice(trail);
    if let TraceEnd::Point(end) = *end {
        poly.push(end);
    }
    let c = corridor(&poly, p.width, w, h)?;
    let n = c.pixels.len();
    let raw = image.as_raw();
    let s = strengths(raw, wi, hi, &c.pixels);
    let d = trail_distance(&c, &poly, w, h);
    let t = (p.contrast / 100.0) as f32;
    let c_max = s.iter().copied().fold(0f32, f32::max);
    let unit: Vec<f64> = (0..n)
        .map(|q| {
            let edge = if s[q] >= t && c_max > 0.0 {
                1.0 - f64::from(s[q]) / f64::from(c_max)
            } else {
                1.0
            };
            let pull = f64::from(d[q]).min(p.width) / p.width;
            LAMBDA + edge + TRAIL_PULL * pull
        })
        .collect();

    let locate = |point: [f64; 2]| {
        let (x, y) = pixel_of(point, w, h);
        c.index(x, y)
            .ok_or_else(|| "edge trace point lies outside its corridor".to_string())
    };
    let start = locate(from)?;
    let end = match *end {
        TraceEnd::Point(point) => locate(point)?,
        TraceEnd::Snap => {
            let last = trail[trail.len() - 1];
            let reach = |q: usize| {
                let (x, y) = c.pixels[q];
                ((x as f64 + 0.5 - last[0]).powi(2) + (y as f64 + 0.5 - last[1]).powi(2)).sqrt()
            };
            // The strongest, then the nearer, then the lower index.
            let snapped = (0..n)
                .filter(|&q| s[q] >= t && reach(q) <= p.width)
                .min_by(|&a, &b| {
                    (s[b].total_cmp(&s[a]))
                        .then(reach(a).total_cmp(&reach(b)))
                        .then(a.cmp(&b))
                });
            snapped.map_or_else(|| locate(last), Ok)?
        }
    };
    let path = shortest_path(&c, &unit, start, end)?
        .into_iter()
        .map(|q| c.pixels[q])
        .collect();
    Ok((path, n as u64))
}

/// Dijkstra over the corridor from `start` to `end` (module doc, step 5),
/// answering the corridor indices along the path.
///
/// The queue pops exactly in `(cost, corridor index)` order — corridor
/// indices run in raster order, so ties resolve by pixel index — but is a
/// ring of cost buckets `BUCKET` wide rather than a binary heap, with which
/// the `make cancel-bench` probe's trace measured slower. It is an exact
/// replacement: every step costs at least `LAMBDA = 2·BUCKET`, so nothing is
/// ever pushed into the bucket being drained, each bucket is sorted by
/// `(cost bits, index)` (finite non-negative costs order like their IEEE
/// bits) before it drains, and a step costs at most
/// `√2·(LAMBDA + 1 + TRAIL_PULL)` < `RING` buckets.
fn shortest_path(
    c: &Corridor,
    unit: &[f64],
    start: usize,
    end: usize,
) -> Result<Vec<usize>, String> {
    const BUCKET: f64 = LAMBDA / 2.0;
    const RING: usize = 128;
    // Multiplying by the reciprocal is monotone too, and every step still
    // moves at least one bucket on (it is ≥ 2·BUCKET).
    let key = |cost: f64| (cost * (1.0 / BUCKET)) as u64;
    let n = c.pixels.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut prev = vec![usize::MAX; n];
    let mut ring: Vec<Vec<(u64, usize)>> = vec![Vec::new(); RING];
    dist[start] = 0.0;
    ring[0].push((0f64.to_bits(), start));
    let (mut current, mut pending) = (0u64, 1usize);
    'drain: while pending > 0 {
        let slot = (current % RING as u64) as usize;
        let mut batch = std::mem::take(&mut ring[slot]);
        pending -= batch.len();
        batch.sort_unstable();
        for &(bits, idx) in &batch {
            if idx == end {
                break 'drain;
            }
            let cost = f64::from_bits(bits);
            if cost > dist[idx] {
                continue;
            }
            for (&q, &step) in c.neighbours[idx].iter().zip(&STEPS) {
                if q == NONE {
                    continue;
                }
                let q = q as usize;
                let next = cost + step * unit[q];
                if next < dist[q] {
                    dist[q] = next;
                    prev[q] = idx;
                    ring[(key(next) % RING as u64) as usize].push((next.to_bits(), q));
                    pending += 1;
                }
            }
        }
        batch.clear();
        ring[slot] = batch;
        current += 1;
    }
    if !dist[end].is_finite() {
        return Err("edge trace found no path inside its corridor".into());
    }
    let mut path = vec![end];
    while let Some(&last) = path.last() {
        if last == start {
            break;
        }
        match prev.get(last) {
            Some(&before) if before != usize::MAX => path.push(before),
            _ => return Err("internal error: edge trace path is broken".into()),
        }
    }
    path.reverse();
    Ok(path)
}

/// Drops collinear runs, then Douglas–Peucker at [`SIMPLIFY_TOLERANCE`];
/// the first and last pixels always survive.
fn simplify(path: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut bent: Vec<(i64, i64)> = Vec::with_capacity(path.len());
    for (i, &p) in path.iter().enumerate() {
        if i == 0 || i + 1 == path.len() {
            bent.push(p);
            continue;
        }
        let (a, b) = (path[i - 1], path[i + 1]);
        if (p.0 - a.0) * (b.1 - p.1) != (p.1 - a.1) * (b.0 - p.0) {
            bent.push(p);
        }
    }
    if bent.len() <= 2 {
        return bent;
    }
    let mut keep = vec![false; bent.len()];
    keep[0] = true;
    keep[bent.len() - 1] = true;
    let mut stack = vec![(0, bent.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut far = (0.0, 0);
        for k in a + 1..b {
            let dist = segment_distance(bent[k], bent[a], bent[b]);
            if dist > far.0 {
                far = (dist, k);
            }
        }
        if far.0 > SIMPLIFY_TOLERANCE {
            keep[far.1] = true;
            stack.push((a, far.1));
            stack.push((far.1, b));
        }
    }
    bent.into_iter()
        .zip(keep)
        .filter_map(|(p, kept)| kept.then_some(p))
        .collect()
}

fn segment_distance(p: (i64, i64), a: (i64, i64), b: (i64, i64)) -> f64 {
    let (px, py) = (p.0 as f64, p.1 as f64);
    let (ax, ay) = (a.0 as f64, a.1 as f64);
    let (dx, dy) = (b.0 as f64 - ax, b.1 as f64 - ay);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
}
