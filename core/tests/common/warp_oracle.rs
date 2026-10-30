//! The independent binary64 warp oracle: lattice nodes, the bilinear
//! displacement field, a Newton inverse, the reference kernel taps and the
//! destination bounds. Shared by the persisted (8/16-bit) and float warp
//! numeric suites.
use serde_json::Value;

pub fn nodes(request: &Value) -> (usize, usize, Vec<[f64; 2]>, bool) {
    let c = request["columns"].as_u64().unwrap() as usize;
    let r = request["rows"].as_u64().unwrap() as usize;
    let nodes = request["displacements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| [v[0].as_f64().unwrap(), v[1].as_f64().unwrap()])
        .collect();
    (c, r, nodes, request["filter"] == "bicubic")
}
pub fn displacement(
    c: usize,
    r: usize,
    n: &[[f64; 2]],
    rect: (i32, i32, u32, u32),
    p: [f64; 2],
) -> [f64; 2] {
    let u = ((p[0] - f64::from(rect.0)) * (c - 1) as f64 / f64::from(rect.2))
        .clamp(0.0, (c - 1) as f64);
    let v = ((p[1] - f64::from(rect.1)) * (r - 1) as f64 / f64::from(rect.3))
        .clamp(0.0, (r - 1) as f64);
    let x = (u.floor() as usize).min(c - 2);
    let y = (v.floor() as usize).min(r - 2);
    let a = u - x as f64;
    let b = v - y as f64;
    let at = |xx: usize, yy: usize, k: usize| n[yy * c + xx][k];
    [0, 1].map(|k| {
        (1.0 - b) * ((1.0 - a) * at(x, y, k) + a * at(x + 1, y, k))
            + b * ((1.0 - a) * at(x, y + 1, k) + a * at(x + 1, y + 1, k))
    })
}
pub fn inverse(
    c: usize,
    r: usize,
    n: &[[f64; 2]],
    rect: (i32, i32, u32, u32),
    q: [f64; 2],
) -> [f64; 2] {
    let mut p = q;
    for _ in 0..32 {
        let d = displacement(c, r, n, rect, p);
        let f = [p[0] + d[0] - q[0], p[1] + d[1] - q[1]];
        if f[0].abs().max(f[1].abs()) < 1e-12 {
            return p;
        }
        let h = 1e-5;
        let x1 = displacement(c, r, n, rect, [p[0] + h, p[1]]);
        let x0 = displacement(c, r, n, rect, [p[0] - h, p[1]]);
        let y1 = displacement(c, r, n, rect, [p[0], p[1] + h]);
        let y0 = displacement(c, r, n, rect, [p[0], p[1] - h]);
        let a = 1.0 + (x1[0] - x0[0]) / (2.0 * h);
        let b = (y1[0] - y0[0]) / (2.0 * h);
        let c2 = (x1[1] - x0[1]) / (2.0 * h);
        let d2 = 1.0 + (y1[1] - y0[1]) / (2.0 * h);
        let det = a * d2 - b * c2;
        p[0] -= (d2 * f[0] - b * f[1]) / det;
        p[1] -= (a * f[1] - c2 * f[0]) / det;
    }
    panic!("reference Newton inverse did not converge")
}
pub fn weight(distance: f64, bicubic: bool) -> f64 {
    let x = distance.abs();
    if !bicubic {
        (1.0 - x).max(0.0)
    } else if x < 1.0 {
        ((1.5 * x - 2.5) * x) * x + 1.0
    } else if x < 2.0 {
        ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
    } else {
        0.0
    }
}
pub fn taps(s: f64, bicubic: bool) -> Vec<(i64, f64)> {
    let p = s - 0.5;
    let base = p.floor() as i64;
    let lead = if bicubic { 1 } else { 0 };
    let count = if bicubic { 4 } else { 2 };
    let mut result: Vec<_> = (0..count)
        .map(|t| {
            let index = base + t - lead;
            (index, weight(p - index as f64, bicubic))
        })
        .collect();
    let sum: f64 = result.iter().map(|(_, w)| w).sum();
    if sum != 0.0 && sum != 1.0 {
        for (_, w) in &mut result {
            *w /= sum;
        }
    }
    result
}
pub fn bounds(
    c: usize,
    r: usize,
    n: &[[f64; 2]],
    rect: (i32, i32, u32, u32),
) -> (i32, i32, u32, u32) {
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for y in 0..r {
        for x in 0..c {
            let p = [
                f64::from(rect.0) + f64::from(rect.2) * x as f64 / (c - 1) as f64,
                f64::from(rect.1) + f64::from(rect.3) * y as f64 / (r - 1) as f64,
            ];
            let d = displacement(c, r, n, rect, p);
            for k in 0..2 {
                low[k] = low[k].min(p[k] + d[k]);
                high[k] = high[k].max(p[k] + d[k]);
            }
        }
    }
    let x = low[0].floor() as i32;
    let y = low[1].floor() as i32;
    (
        x,
        y,
        (high[0].ceil() as i32 - x) as u32,
        (high[1].ceil() as i32 - y) as u32,
    )
}
