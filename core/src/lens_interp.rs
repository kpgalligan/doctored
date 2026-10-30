//! Interpolation of one Lensfun lens's calibration rows (v1-lensfun), split
//! from `lens_db` for size. Lensfun documents only that values are
//! interpolated; these are our rules: distortion and TCA are linear in focal
//! length between the bracketing rows when both use the same model, else the
//! nearer row; outside the calibrated range the nearest end. `pa` vignetting
//! is, at each bracketing focal length, an inverse-distance (power 2)
//! weighting over `(1/N, 1/d)` (an exact hit takes that row), then linear in
//! focal length.
use std::collections::BTreeSet;

/// One distortion or TCA row at its focal length, as the profile JSON's
/// model object.
#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) focal: f64,
    pub(crate) model: &'static str,
    pub(crate) keys: &'static [&'static str],
    pub(crate) terms: Vec<f64>,
}

/// Distortion or TCA at `focal` (this module's doc): linear between the bracketing
/// rows of one model, else the nearer row; the nearest end outside the range.
pub(crate) fn interpolate_rows(rows: &[Row], focal: f64) -> Option<Row> {
    let lower = rows.iter().rev().find(|r| r.focal <= focal);
    let upper = rows.iter().find(|r| r.focal >= focal);
    match (lower, upper) {
        (None, None) => None,
        (Some(r), None) | (None, Some(r)) => Some(r.clone()),
        (Some(lo), Some(hi)) if lo.focal == hi.focal => Some(lo.clone()),
        (Some(lo), Some(hi)) if lo.model == hi.model => {
            let t = (focal - lo.focal) / (hi.focal - lo.focal);
            Some(Row {
                focal,
                terms: lo
                    .terms
                    .iter()
                    .zip(&hi.terms)
                    .map(|(a, b)| a + (b - a) * t)
                    .collect(),
                ..lo.clone()
            })
        }
        (Some(lo), Some(hi)) => Some(if focal - lo.focal <= hi.focal - focal {
            lo.clone()
        } else {
            hi.clone()
        }),
    }
}

/// `pa` at `focal`, f-number `n` and distance `d` (this module's doc).
pub(crate) fn pa_at(rows: &[[f64; 6]], focal: f64, n: f64, d: f64) -> Option<[f64; 3]> {
    let focals: BTreeSet<u64> = rows.iter().map(|r| r[0].to_bits()).collect();
    let focals: Vec<f64> = {
        let mut f: Vec<f64> = focals.into_iter().map(f64::from_bits).collect();
        f.sort_by(f64::total_cmp);
        f
    };
    let lower = focals.iter().rev().find(|f| **f <= focal).copied();
    let upper = focals.iter().find(|f| **f >= focal).copied();
    let at = |f: f64| -> [f64; 3] {
        let target = [1.0 / n, 1.0 / d];
        let mut sum = [0.0; 3];
        let mut weights = 0.0;
        for r in rows.iter().filter(|r| r[0] == f) {
            let dx = 1.0 / r[1] - target[0];
            let dy = 1.0 / r[2] - target[1];
            let distance2 = dx * dx + dy * dy;
            if distance2 == 0.0 {
                return [r[3], r[4], r[5]];
            }
            let w = 1.0 / distance2;
            weights += w;
            for (s, k) in sum.iter_mut().zip(&r[3..6]) {
                *s += w * k;
            }
        }
        sum.map(|s| s / weights)
    };
    match (lower, upper) {
        (None, None) => None,
        (Some(f), None) | (None, Some(f)) => Some(at(f)),
        (Some(lo), Some(hi)) if lo == hi => Some(at(lo)),
        (Some(lo), Some(hi)) => {
            let (a, b) = (at(lo), at(hi));
            let t = (focal - lo) / (hi - lo);
            Some(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t))
        }
    }
}
