//! The Magnetic Lasso's cost rule (`designs/phase-6-selection-mask-status.md`,
//! "Magnetic Lasso cost rule") through `dr_selection_trace_edge`: an oracle
//! written from the rule alone (its own Sobel, corridor, trail distance,
//! Dijkstra and vertex placement) against the tracer on a textured picture,
//! the direction term
//! keeping the wire off hatching it would have to cross, the sub-pixel term
//! choosing the edge that runs through the pixel centres, and the
//! per-capture edge map (`dr_edge_map_create`) changing no trace.

use crate::edge_trace_support::{
    image, raw_call_through, samples, segment_distance, trace, trace_through, Image,
};
use doctored_core::ffi_selection_tools::{
    dr_edge_map_create, dr_edge_map_free, edge_map_cached_samples, edge_map_create_capped,
};
use doctored_core::DrEdgeMap;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::f64::consts::{PI, SQRT_2};

/// The rule's constants, restated.
const LAMBDA: f64 = 0.05;
const TRAIL_PULL: f64 = 0.02;
const SUBPIXEL: f64 = 0.1;

/// A straight RGBA8 picture with its handle.
struct Picture {
    w: i64,
    h: i64,
    px: Vec<[u8; 4]>,
    handle: Image,
}

fn picture(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Picture {
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            px.push(f(x, y));
        }
    }
    let handle = image(w, h, |x, y| px[(y * w + x) as usize]);
    Picture {
        w: i64::from(w),
        h: i64::from(h),
        px,
        handle,
    }
}

/// The rule's edge sample at pixel `(x, y)`: the Sobel gradient of the first
/// of R·a, G·a, B·a and A (unit range, borders replicated) with the largest
/// magnitude. In f32, the samples' own precision, accumulated in kernel order.
fn gradient(p: &Picture, x: i64, y: i64) -> [f32; 2] {
    const KX: [[f32; 3]; 3] = [[-1.0, 0.0, 1.0], [-2.0, 0.0, 2.0], [-1.0, 0.0, 1.0]];
    const KY: [[f32; 3]; 3] = [[-1.0, -2.0, -1.0], [0.0, 0.0, 0.0], [1.0, 2.0, 1.0]];
    let mut best = ([0f32; 2], -1f32);
    for c in 0..4 {
        let (mut gx, mut gy) = (0f32, 0f32);
        for (ky, dy) in (-1i64..=1).enumerate() {
            for (kx, dx) in (-1i64..=1).enumerate() {
                let (sx, sy) = ((x + dx).clamp(0, p.w - 1), (y + dy).clamp(0, p.h - 1));
                let q = p.px[(sy * p.w + sx) as usize];
                let a = f32::from(q[3]) / 255.0;
                let v = if c == 3 {
                    a
                } else {
                    f32::from(q[c]) / 255.0 * a
                };
                gx += KX[ky][kx] * v;
                gy += KY[ky][kx] * v;
            }
        }
        let m = gx * gx + gy * gy;
        if m > best.1 {
            best = ([gx, gy], m);
        }
    }
    best.0
}

fn strength(g: [f32; 2]) -> f32 {
    ((g[0] * g[0] + g[1] * g[1]).sqrt() / 4.0).min(1.0)
}

/// The rule's lattice normal nearest `g`: horizontal within 22.5° of it,
/// vertical likewise, else the diagonal whose sign is that of `gx·gy`.
fn lattice_normal(g: [f32; 2]) -> (i64, i64) {
    let (gx, gy) = (f64::from(g[0]), f64::from(g[1]));
    let k = SQRT_2 - 1.0;
    if gy.abs() <= k * gx.abs() {
        (1, 0)
    } else if gx.abs() <= k * gy.abs() {
        (0, 1)
    } else if gx * gy > 0.0 {
        (1, 1)
    } else {
        (1, -1)
    }
}

/// The strength of pixel `(x, y)`, its coordinates clamped to the picture.
fn strength_at(p: &Picture, x: i64, y: i64) -> f32 {
    strength(gradient(p, x.clamp(0, p.w - 1), y.clamp(0, p.h - 1)))
}

/// The rule's vertex for path pixel `(x, y)` ("Vertex placement"): with
/// `s₋, s₀, s₊` the strengths across its edge along `n`, a pixel at least
/// `contrast/100` strong that is a maximum with `k = s₋ − 2s₀ + s₊ < 0` sits
/// at `centre + δ·n`, `δ = (s₊ − s₋)/(−2k)` within ±½ (a located vertex,
/// the `bool`); any other at its centre; each coordinate written in
/// `[i, i + 1)`.
fn placed(p: &Picture, x: i64, y: i64, contrast: f64) -> ([f32; 2], bool) {
    let g = gradient(p, x, y);
    let n = lattice_normal(g);
    let here = strength(g);
    let (minus, plus) = (
        f64::from(strength_at(p, x - n.0, y - n.1)),
        f64::from(strength_at(p, x + n.0, y + n.1)),
    );
    let s = f64::from(here);
    let k = minus - 2.0 * s + plus;
    let located = here >= (contrast / 100.0) as f32 && s >= minus && s >= plus && k < 0.0;
    let delta = if located {
        ((plus - minus) / (-2.0 * k)).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let write = |i: i64, v: f64| {
        let top = (i + 1) as f32;
        let v = v as f32;
        if v >= top {
            top.next_down()
        } else {
            v
        }
    };
    (
        [
            write(x, x as f64 + 0.5 + delta * n.0 as f64),
            write(y, y as f64 + 0.5 + delta * n.1 as f64),
        ],
        located,
    )
}

/// The rule's cheapest path, from `from` along the one horizontal trail
/// segment to the explicit `end` on the same row (Frequency 0): the corridor
/// by brute force, the trail distance as the octile distance to the
/// segment's pixels (exact for one straight source in a convex corridor),
/// the step costs in f64 and a binary-heap Dijkstra ordered by
/// `(cost, pixel index)`.
fn oracle(
    p: &Picture,
    from: [f64; 2],
    end: [f64; 2],
    width: f64,
    contrast: f64,
) -> Vec<(i64, i64)> {
    assert_eq!(from[1], end[1], "the oracle takes one horizontal segment");
    let mut index = vec![usize::MAX; (p.w * p.h) as usize];
    let mut pixels = Vec::new();
    for y in 0..p.h {
        for x in 0..p.w {
            if segment_distance([x as f64 + 0.5, y as f64 + 0.5], from, end) <= width {
                index[(y * p.w + x) as usize] = pixels.len();
                pixels.push((x, y));
            }
        }
    }
    let row = from[1].floor() as i64;
    let (x0, x1) = (from[0].floor() as i64, end[0].floor() as i64);
    let octile = |x: i64, y: i64| {
        let dx = (x0 - x).max(x - x1).max(0) as f64;
        let dy = (y - row).abs() as f64;
        dx.max(dy) + (SQRT_2 - 1.0) * dx.min(dy)
    };
    let at = |x: i64, y: i64| strength_at(p, x, y);
    let g: Vec<[f32; 2]> = pixels.iter().map(|&(x, y)| gradient(p, x, y)).collect();
    let s: Vec<f32> = g.iter().map(|&g| strength(g)).collect();
    let t = (contrast / 100.0) as f32;
    let c_max = s.iter().copied().fold(0f32, f32::max);
    let (c45, c22) = ((PI / 4.0).cos(), (PI / 8.0).cos());
    // The cost of entering corridor pixel `i` by the step (dx, dy).
    let unit = |i: usize, dx: i64, dy: i64| -> f64 {
        let (x, y) = pixels[i];
        let attraction = if s[i] >= t && c_max > 0.0 {
            f64::from(s[i]) / f64::from(c_max)
        } else {
            0.0
        };
        let (gx, gy) = (f64::from(g[i][0]), f64::from(g[i][1]));
        let magnitude = gx.hypot(gy);
        // Direction: |cos| of the angle between the step and the tangent
        // (-gy, gx), full to 22.5°, nothing from 45°, linear in cos between.
        let direction = if magnitude > 0.0 {
            let length = ((dx * dx + dy * dy) as f64).sqrt();
            let along = ((dx as f64 * -gy + dy as f64 * gx) / (length * magnitude)).abs();
            ((along - c45) / (c22 - c45)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Sub-pixel: the lattice normal nearest g, the strengths either side.
        let n = lattice_normal(g[i]);
        let (minus, here, plus) = (
            f64::from(at(x - n.0, y - n.1)),
            f64::from(s[i]),
            f64::from(at(x + n.0, y + n.1)),
        );
        let curve = minus - 2.0 * here + plus;
        let offset = if here >= minus && here >= plus && curve < 0.0 {
            (minus - plus).abs() / -curve
        } else {
            1.0
        };
        let centred = 1.0 - SUBPIXEL * offset;
        let pull = octile(x, y).min(width) / width;
        LAMBDA + (1.0 - attraction * direction * centred) + TRAIL_PULL * pull
    };
    let locate = |q: [f64; 2]| index[(q[1].floor() as i64 * p.w + q[0].floor() as i64) as usize];
    let (start, goal) = (locate(from), locate(end));
    let mut dist = vec![f64::INFINITY; pixels.len()];
    let mut prev = vec![usize::MAX; pixels.len()];
    let mut heap = BinaryHeap::new();
    dist[start] = 0.0;
    heap.push(Reverse((0f64.to_bits(), start)));
    while let Some(Reverse((bits, i))) = heap.pop() {
        if i == goal {
            break;
        }
        let cost = f64::from_bits(bits);
        if cost > dist[i] {
            continue;
        }
        let (x, y) = pixels[i];
        for (dx, dy) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let (qx, qy) = (x + dx, y + dy);
            if qx < 0 || qy < 0 || qx >= p.w || qy >= p.h {
                continue;
            }
            let j = index[(qy * p.w + qx) as usize];
            if j == usize::MAX {
                continue;
            }
            let step = if dx != 0 && dy != 0 { SQRT_2 } else { 1.0 };
            let next = cost + step * unit(j, dx, dy);
            if next < dist[j] {
                dist[j] = next;
                prev[j] = i;
                heap.push(Reverse((next.to_bits(), j)));
            }
        }
    }
    let mut path = vec![goal];
    while path[path.len() - 1] != start {
        path.push(prev[path[path.len() - 1]]);
    }
    path.reverse();
    path.into_iter().map(|i| pixels[i]).collect()
}

/// A deterministic hash in 0..256 per pixel and salt.
fn noise(x: u32, y: u32, salt: u32) -> f64 {
    let mut v = x
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(y.wrapping_mul(0x85EB_CA77))
        .wrapping_add(salt.wrapping_mul(0xC2B2_AE3D));
    v ^= v >> 15;
    v = v.wrapping_mul(0x2C1B_3C6D);
    v ^= v >> 12;
    f64::from(v & 0xFF)
}

/// Soft blobs, whose edges run at every angle and between pixel centres,
/// over hash noise in every channel, so every pixel has a gradient of its
/// own direction.
fn textured() -> Picture {
    picture(96, 64, |x, y| {
        let (fx, fy) = (f64::from(x), f64::from(y));
        let blob = |cx: f64, cy: f64, r: f64| {
            (-((fx - cx).powi(2) + (fy - cy).powi(2)) / (2.0 * r * r)).exp()
        };
        let base = 70.0 + 120.0 * blob(26.0, 26.0, 5.0) + 100.0 * blob(52.0, 39.0, 7.0)
            - 50.0 * blob(70.0, 29.0, 4.0)
            + 90.0 * blob(84.0, 36.0, 6.0);
        let channel = |salt: u32, scale: f64| {
            (base * scale + (noise(x, y, salt) - 128.0) / 10.0).clamp(0.0, 255.0) as u8
        };
        [channel(1, 1.0), channel(2, 0.9), channel(3, 0.8), 255]
    })
}

#[test]
fn the_traced_path_is_the_cheapest_under_the_written_rule() {
    let p = textured();
    let (from, end, width, contrast) = ([6.5, 32.5], [89.5, 32.5], 12.3, 1.0);
    let expected = oracle(&p, from, end, width, contrast);
    let t = trace(
        &p.handle,
        &format!(
            r#"{{"from":[{},{}],"trail":[[{},{}]],"end":[{},{}],"width":{width},"contrast":{contrast},"frequency":0}}"#,
            from[0], from[1], end[0], end[1], end[0], end[1]
        ),
    )
    .unwrap();
    // The oracle's own placement of each pixel of its path: at Contrast 1
    // nearly every pixel of this texture is a located crest.
    let (vertices, located): (Vec<[f32; 2]>, Vec<bool>) = expected
        .iter()
        .map(|&(x, y)| placed(&p, x, y, contrast))
        .unzip();
    let off_centre = expected
        .iter()
        .zip(&vertices)
        .filter(|(&(x, y), v)| **v != [x as f32 + 0.5, y as f32 + 0.5])
        .count();
    assert!(off_centre > 0, "the oracle placed no vertex off its centre");
    // The same arithmetic in another order may differ by an f32 step.
    let near =
        |a: &[f32; 2], b: &[f32; 2]| (a[0] - b[0]).abs() <= 1e-5 && (a[1] - b[1]).abs() <= 1e-5;
    let (first, last) = (vertices[0], vertices[vertices.len() - 1]);
    assert!(near(&t.points[0], &first), "{:?} vs {first:?}", t.points[0]);
    assert!(
        near(&t.points[t.points.len() - 1], &last),
        "{:?} vs {last:?}",
        t.points
    );
    // Every vertex is a placed pixel of the oracle's path, in order.
    let mut cursor = 0;
    for vertex in &t.points {
        let found = vertices[cursor..].iter().position(|c| near(c, vertex));
        assert!(
            found.is_some(),
            "vertex {vertex:?} is not on the oracle's path after pixel {cursor}: {:?}\noracle {:?}",
            t.points,
            expected
        );
        cursor += found.unwrap_or(0);
    }
    // And the simplified wire passes within each placed pixel's own
    // tolerance of it: a quarter pixel for a located vertex, half a pixel
    // for a centre (Douglas–Peucker runs on the placed points).
    let wire: Vec<[f64; 2]> = t
        .points
        .iter()
        .map(|p| [f64::from(p[0]), f64::from(p[1])])
        .collect();
    for (c, &on_edge) in vertices.iter().zip(&located) {
        let c = [f64::from(c[0]), f64::from(c[1])];
        let near = wire
            .windows(2)
            .map(|s| segment_distance(c, s[0], s[1]))
            .fold(f64::INFINITY, f64::min);
        let tolerance = if on_edge { 0.25 } else { 0.5 };
        assert!(
            near <= tolerance + 1e-5,
            "oracle vertex {c:?} (located {on_edge}) is {near} px off the wire {:?}",
            t.points
        );
    }
}

/// Light above y = 60 and dark from it, with a dark tab over x 60..100
/// reaching up to y = 40, and across the tab's mouth (x 62..98, y 56..64)
/// a band of 2-px light bars: every pixel there is a full-strength edge
/// whose tangent is vertical.
fn hatched_tab() -> Picture {
    picture(160, 100, |x, y| {
        let light = [220, 220, 220, 255];
        let dark = [30, 30, 30, 255];
        let tab = (60..100).contains(&x) && (40..60).contains(&y);
        let bar = (62..98).contains(&x) && (56..64).contains(&y) && ((x - 62) / 2) % 2 == 0;
        if bar || !(tab || y >= 60) {
            light
        } else {
            dark
        }
    })
}

#[test]
fn a_wire_follows_the_edge_around_hatching_it_would_have_to_cross() {
    // Crossing the 36-px band costs flat ground's 1.05 a pixel, every step
    // across it being at least 45° off the bars' vertical tangent; going
    // round the tab follows about 80 px of hard pixel-aligned edge at
    // LAMBDA + SUBPIXEL = 0.15. Without the direction term the bars would
    // cost LAMBDA a pixel and the wire would cut straight across them.
    let p = hatched_tab();
    let t = trace(
        &p.handle,
        r#"{"from":[20,60],"trail":[[20,50],[140,50]],"end":[140,60],"width":15,"frequency":0}"#,
    )
    .unwrap();
    let wire = samples(&t.points);
    for s in &wire {
        assert!(
            !(s[0] > 62.0 && s[0] < 98.0 && s[1] > 57.0 && s[1] < 63.0),
            "the wire crosses the hatching at {s:?}: {:?}",
            t.points
        );
    }
    let top = wire.iter().map(|s| s[1]).fold(f64::INFINITY, f64::min);
    assert!(
        top <= 41.0,
        "the wire reaches only y = {top}: {:?}",
        t.points
    );
}

/// Two horizontal edges of equal crest strength (80 levels): above, an
/// anti-aliased one whose middle row 31 is half-way (40 → 80 → 120), so its
/// crest runs through row 31's pixel centres; below, a hard step between
/// rows 69 and 70 (120 → 200), whose two crest rows each lie half a pixel
/// off it. `flip` turns the picture upside down.
fn two_parallel_edges(flip: bool) -> Picture {
    picture(200, 100, move |_, y| {
        let y = if flip { 99 - y } else { y };
        let v = match y {
            0..=30 => 40,
            31 => 80,
            32..=69 => 120,
            _ => 200,
        };
        [v, v, v, 255]
    })
}

#[test]
fn an_edge_through_the_pixel_centres_wins_over_an_equal_one_between_rows() {
    // The trail runs on row 51: 20 rows from the centred crest, 18 from the
    // hard step's nearer crest row, so reaching the hard step is cheaper and
    // the trail pulls toward it. Along the edges the centred crest costs
    // LAMBDA a pixel and the hard step's rows LAMBDA + SUBPIXEL, which over
    // the 180-px run outweighs both.
    for flip in [false, true] {
        let p = two_parallel_edges(flip);
        let (trail_y, centred_y) = if flip { (48.5, 68.5) } else { (51.5, 31.5) };
        let t = trace(
            &p.handle,
            &format!(
                r#"{{"from":[10,{trail_y}],"trail":[[190,{trail_y}]],"end":[190,{trail_y}],"width":25,"frequency":0}}"#
            ),
        )
        .unwrap();
        for s in samples(&t.points) {
            if (40.0..=160.0).contains(&s[0]) {
                assert!(
                    (s[1] - centred_y).abs() <= 1.0,
                    "flip {flip}: the wire is at {s:?}, not on the centred edge: {:?}",
                    t.points
                );
            }
        }
    }
}

/// An owned edge map.
struct Map(*mut DrEdgeMap);

impl Map {
    fn of(img: &Image) -> Map {
        let map = unsafe { dr_edge_map_create(img.0) };
        assert!(!map.is_null());
        Map(map)
    }

    fn capped(img: &Image, cap: usize) -> Map {
        let map = unsafe { edge_map_create_capped(img.0, cap) };
        assert!(!map.is_null());
        Map(map)
    }

    fn cached(&self) -> usize {
        unsafe { edge_map_cached_samples(self.0) }
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        unsafe { dr_edge_map_free(self.0) };
    }
}

/// Requests over [`textured`] that touch tiles in different orders: snap
/// and point ends, widths from 1 to 30, a fasten, and a loop (out round the
/// picture and back near the anchor), which runs as two legs.
const REQUESTS: [&str; 5] = [
    r#"{"from":[6.5,32.5],"trail":[[40,30],[70,36]],"end":"snap","width":10,"frequency":0}"#,
    r#"{"from":[6.5,32.5],"trail":[[89.5,32.5]],"end":[89.5,32.5],"width":12.3,"contrast":1,"frequency":0}"#,
    r#"{"from":[10,10],"trail":[[80,10],[80,50],[10,50],[12,14]],"end":"snap","width":4,"frequency":0}"#,
    r#"{"from":[90,60],"trail":[[50,5],[5,40]],"end":[5,40],"width":1,"frequency":57}"#,
    r#"{"from":[48,32],"trail":[[60,20]],"end":"snap","width":30,"frequency":100}"#,
];

type Outcome = (Vec<[f32; 2]>, Option<usize>, u64);

fn outcome(img: &Image, map: &Map, json: &str) -> Outcome {
    let t = trace_through(img, map.0, json).unwrap();
    (t.points, t.fasten, t.corridor)
}

#[test]
fn a_trace_is_identical_with_no_map_a_fresh_map_and_a_warm_one() {
    let p = textured();
    let none: Vec<Outcome> = REQUESTS
        .iter()
        .map(|json| {
            let t = trace(&p.handle, json).unwrap();
            (t.points, t.fasten, t.corridor)
        })
        .collect();
    assert!(
        none[2].1.is_some(),
        "the loop request fastens at its junction"
    );
    let fresh: Vec<Outcome> = REQUESTS
        .iter()
        .map(|json| outcome(&p.handle, &Map::of(&p.handle), json))
        .collect();
    assert_eq!(fresh, none, "a fresh map per trace");
    let warm = Map::of(&p.handle);
    let forward: Vec<Outcome> = REQUESTS
        .iter()
        .map(|json| outcome(&p.handle, &warm, json))
        .collect();
    assert_eq!(forward, none, "one map, every request in order");
    let mut backward: Vec<Outcome> = REQUESTS
        .iter()
        .rev()
        .map(|json| outcome(&p.handle, &warm, json))
        .collect();
    backward.reverse();
    assert_eq!(backward, none, "the same map, the requests again backwards");
    // Two tiles: nearly every touch drops what the map holds.
    let small = Map::capped(&p.handle, 512);
    let starved: Vec<Outcome> = REQUESTS
        .iter()
        .map(|json| outcome(&p.handle, &small, json))
        .collect();
    assert_eq!(starved, none, "a map that holds two tiles");
    assert!(small.cached() <= 512, "{} samples", small.cached());
}

#[test]
fn a_map_keeps_the_samples_it_computed() {
    let p = textured();
    let map = Map::of(&p.handle);
    assert_eq!(map.cached(), 0);
    let first = outcome(&p.handle, &map, REQUESTS[0]);
    let kept = map.cached();
    // Tiles of 16×16 samples, every one the trace touched.
    assert!(kept > 0 && kept.is_multiple_of(256), "{kept} samples");
    let again = outcome(&p.handle, &map, REQUESTS[0]);
    assert_eq!(again, first);
    assert_eq!(map.cached(), kept, "a repeated trace computes nothing new");
}

#[test]
fn a_map_of_another_image_refuses_and_leaves_the_outputs_untouched() {
    let p = textured();
    let twin = textured();
    let map = Map::of(&twin.handle);
    let (ok, count, fasten, corridor, _, message) =
        raw_call_through(&p.handle, map.0, REQUESTS[0], 16);
    assert!(!ok);
    assert_eq!(message, "edge map was made for another image");
    assert_eq!(
        (count, fasten, corridor),
        (usize::MAX, i32::MIN, u64::MAX),
        "a refusal wrote outputs"
    );
    assert_eq!(map.cached(), 0, "the refusal sampled nothing");
}
