//! The Perspective Crop oracle, shared by `perspective_crop_tests.rs`
//! and `precision_perspective_crop.rs`: a homography solved by Gaussian
//! elimination over the eight direct-linear-transform equations — not the
//! core's closed form — and the nearest resample's covering source sample.
//! A suite root mounts it with
//! `#[path = "common/perspective_oracle.rs"] mod perspective_oracle;`.
#![allow(dead_code)]

/// The homography taking the rectangle `(0, 0, w, h)` onto `quad` (TL, TR,
/// BR, BL), as `[h0..h7, 1]`: eight DLT equations solved by Gaussian
/// elimination with partial pivoting.
pub fn dlt(w: f64, h: f64, quad: [f64; 8]) -> [f64; 9] {
    let from = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    solve(
        from,
        [
            (quad[0], quad[1]),
            (quad[2], quad[3]),
            (quad[4], quad[5]),
            (quad[6], quad[7]),
        ],
    )
}

/// The homography taking the four `from` points onto the four `to` points.
pub fn solve(from: [(f64, f64); 4], to: [(f64, f64); 4]) -> [f64; 9] {
    let mut a = [[0.0f64; 9]; 8];
    for k in 0..4 {
        let ((x, y), (u, v)) = (from[k], to[k]);
        a[2 * k] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        a[2 * k + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for col in 0..8 {
        let pivot = (col..8)
            .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))
            .unwrap();
        a.swap(col, pivot);
        let pivot_row = a[col];
        for (row, line) in a.iter_mut().enumerate() {
            if row != col {
                let f = line[col] / pivot_row[col];
                for (value, p) in line.iter_mut().zip(pivot_row).skip(col) {
                    *value -= f * p;
                }
            }
        }
    }
    let mut h = [1.0; 9];
    for (i, (out, line)) in h.iter_mut().zip(&a).enumerate() {
        *out = line[8] / line[i];
    }
    h
}

pub fn project(h: &[f64; 9], x: f64, y: f64) -> (f64, f64) {
    let w = h[6] * x + h[7] * y + h[8];
    (
        (h[0] * x + h[1] * y + h[2]) / w,
        (h[3] * x + h[4] * y + h[5]) / w,
    )
}

/// Whether a source coordinate sits so close to a pixel boundary that the
/// floor is not a fact the two independent solves must agree on.
pub fn ambiguous(v: f64) -> bool {
    (v - v.round()).abs() < 1e-6
}

/// The nearest resample of a `dims` plane at `offset` onto `(0, 0, w, h)`:
/// `Some(index)` of the covering source sample, `None` outside the source,
/// and the outer `None` for a boundary-ambiguous destination.
pub fn nearest_index(
    h: &[f64; 9],
    dims: (u32, u32),
    offset: (i32, i32),
    i: u32,
    j: u32,
) -> Option<Option<usize>> {
    let (x, y) = project(h, f64::from(i) + 0.5, f64::from(j) + 0.5);
    let (u, v) = (x - f64::from(offset.0), y - f64::from(offset.1));
    if ambiguous(u) || ambiguous(v) {
        return None;
    }
    let (fu, fv) = (u.floor(), v.floor());
    if fu < 0.0 || fv < 0.0 || fu >= f64::from(dims.0) || fv >= f64::from(dims.1) {
        return Some(None);
    }
    Some(Some(fv as usize * dims.0 as usize + fu as usize))
}
