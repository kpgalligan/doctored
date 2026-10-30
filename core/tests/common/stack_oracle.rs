//! An independent reference for Stack Modes, written from the rule's
//! prose and never from the core: every extent pixel is looked up member by
//! member through each member's own rectangle (no row buffers), the integer
//! statistics round through quotient and remainder, and the float ones run in
//! f64. A suite root mounts it with
//! `#[path = "common/stack_oracle.rs"] mod stack_oracle;`.
//!
//! The rule: the extent is the union of the members' rectangles. Per pixel,
//! the present members are those with alpha > 0; none leaves the pixel all
//! zero. The output alpha is the largest present alpha. Mean is, per colour
//! channel, the alpha-weighted mean over the present members. Every other
//! mode reads the members at least half covered (all the present ones when
//! none is), unweighted: median, minimum, maximum, range, population σ;
//! summation; the population variance over the full value F; skewness as
//! F/2·(1 + s/s_max) with s_max = (n − 2)/√(n − 1); kurtosis (non-excess k)
//! as F·(k − 1)/(k_max − 1) with k_max = (n² − 3n + 3)/(n − 1), both read as
//! the flat case (F/2, 0) for n ≤ 2 or equal values; base-2 entropy over 256
//! equal bins of [0, F] divided by log₂ min(n, 256), times F (0 for one
//! value); the most frequent value, a tie taking the median of the tied
//! values. Integers round every real result half up and clamp to [0, F].
#![allow(dead_code)]

use std::collections::BTreeMap;

/// The twelve request names: the shipped six, then six more.
pub const MODES: [&str; 12] = [
    "mean",
    "median",
    "minimum",
    "maximum",
    "range",
    "standard_deviation",
    "summation",
    "variance",
    "entropy",
    "kurtosis",
    "skewness",
    "majority",
];

/// The modes whose integer result goes through an f64 step the oracle
/// recomputes in its own order: a caller allows one code either side.
pub const REAL_STEP_MODES: [&str; 4] = ["standard_deviation", "skewness", "kurtosis", "entropy"];

/// The shape statistics as a fraction of the full value: `(skewness,
/// kurtosis)` of `values`, written straight from the moments' definitions.
pub fn shape(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let flat = values.iter().all(|&v| v == values[0]);
    if values.len() <= 2 || flat {
        return (0.5, 0.0);
    }
    let mean = values.iter().sum::<f64>() / n;
    let moment = |p: i32| values.iter().map(|v| (v - mean).powi(p)).sum::<f64>() / n;
    let (m2, m3, m4) = (moment(2), moment(3), moment(4));
    let s = m3 / (m2 * m2.sqrt());
    let s_max = (n - 2.0) / (n - 1.0).sqrt();
    let k = m4 / (m2 * m2);
    let k_max = (n * n - 3.0 * n + 3.0) / (n - 1.0);
    (0.5 + 0.5 * s / s_max, (k - 1.0) / (k_max - 1.0))
}

/// Entropy as a fraction of the full value over the 256 bins `bin` names.
pub fn entropy(values: &[f64], bin: impl Fn(f64) -> usize) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for &v in values {
        counts[bin(v)] += 1;
    }
    let n = values.len() as f64;
    let h: f64 = counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum();
    h / (values.len().min(256) as f64).log2()
}

/// The tied most frequent values, ascending (one value when there is no
/// tie), keyed by their bits so float values count exactly.
fn modal(values: &[f64]) -> Vec<f64> {
    let mut counts: BTreeMap<u64, usize> = BTreeMap::new();
    for &v in values {
        *counts.entry(v.to_bits()).or_default() += 1;
    }
    let most = *counts.values().max().unwrap();
    let mut tied: Vec<f64> = counts
        .iter()
        .filter(|(_, &c)| c == most)
        .map(|(&bits, _)| f64::from_bits(bits))
        .collect();
    tied.sort_by(|a, b| a.partial_cmp(b).unwrap());
    tied
}

/// One member: its canvas rectangle and its stored samples as f64 (codes at
/// 8 and 16 bits, linear values at float), row-major.
pub struct Member {
    pub offset: (i32, i32),
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f64; 4]>,
}

impl Member {
    fn at(&self, cx: i64, cy: i64) -> Option<[f64; 4]> {
        let (lx, ly) = (cx - i64::from(self.offset.0), cy - i64::from(self.offset.1));
        if lx < 0 || ly < 0 || lx >= i64::from(self.width) || ly >= i64::from(self.height) {
            return None;
        }
        Some(self.pixels[(ly * i64::from(self.width) + lx) as usize])
    }
}

/// The union of the members' rectangles as `(x, y, width, height)`.
pub fn extent(members: &[Member]) -> (i32, i32, u32, u32) {
    let x0 = members.iter().map(|m| m.offset.0).min().unwrap();
    let y0 = members.iter().map(|m| m.offset.1).min().unwrap();
    let x1 = members
        .iter()
        .map(|m| i64::from(m.offset.0) + i64::from(m.width))
        .max()
        .unwrap();
    let y1 = members
        .iter()
        .map(|m| i64::from(m.offset.1) + i64::from(m.height))
        .max()
        .unwrap();
    (
        x0,
        y0,
        (x1 - i64::from(x0)) as u32,
        (y1 - i64::from(y0)) as u32,
    )
}

/// The members present at each extent pixel (alpha > 0), row-major.
fn present(members: &[Member]) -> Vec<Vec<[f64; 4]>> {
    let (x0, y0, w, h) = extent(members);
    let mut out = Vec::new();
    for y in 0..i64::from(h) {
        for x in 0..i64::from(w) {
            out.push(
                members
                    .iter()
                    .filter_map(|m| m.at(i64::from(x0) + x, i64::from(y0) + y))
                    .filter(|p| p[3] > 0.0)
                    .collect(),
            );
        }
    }
    out
}

/// `num / den` rounded half up, through the quotient and the remainder.
fn half_up(num: u64, den: u64) -> u64 {
    let (q, r) = (num / den, num % den);
    if 2 * r >= den {
        q + 1
    } else {
        q
    }
}

/// The integer statistic over the extent: exact for every mode but the
/// standard deviation, which is the nearest code to the f64 value (a caller
/// allows one code either side).
pub fn integer(mode: &str, members: &[Member], full: u64) -> Vec<[u64; 4]> {
    present(members)
        .into_iter()
        .map(|here| {
            let here: Vec<[u64; 4]> = here.iter().map(|p| p.map(|v| v as u64)).collect();
            if here.is_empty() {
                return [0; 4];
            }
            let alpha = here.iter().map(|p| p[3]).max().unwrap();
            let mut out = [0, 0, 0, alpha];
            let half: Vec<[u64; 4]> = here.iter().copied().filter(|p| p[3] * 2 >= full).collect();
            let voters = if half.is_empty() { here.clone() } else { half };
            for c in 0..3 {
                let mut values: Vec<u64> = voters.iter().map(|p| p[c]).collect();
                values.sort();
                let n = values.len();
                out[c] = match mode {
                    "mean" => half_up(
                        here.iter().map(|p| p[3] * p[c]).sum(),
                        here.iter().map(|p| p[3]).sum(),
                    ),
                    "median" if n % 2 == 1 => values[n / 2],
                    "median" => half_up(values[n / 2 - 1] + values[n / 2], 2),
                    "minimum" => values[0],
                    "maximum" => values[n - 1],
                    "range" => values[n - 1] - values[0],
                    "standard_deviation" => {
                        let mean = values.iter().sum::<u64>() as f64 / n as f64;
                        let variance = values
                            .iter()
                            .map(|&v| (v as f64 - mean).powi(2))
                            .sum::<f64>()
                            / n as f64;
                        (variance.sqrt() + 0.5).floor() as u64
                    }
                    "summation" => values.iter().sum::<u64>().min(full),
                    "variance" => {
                        // n²·m₂ = n·Σv² − (Σv)², exactly.
                        let n = n as u128;
                        let sum: u128 = values.iter().map(|&v| v as u128).sum();
                        let squares: u128 = values.iter().map(|&v| (v * v) as u128).sum();
                        let num = n * squares - sum * sum;
                        let den = n * n * full as u128;
                        let (q, r) = (num / den, num % den);
                        (if 2 * r >= den { q + 1 } else { q }).min(full as u128) as u64
                    }
                    "skewness" | "kurtosis" | "entropy" => {
                        let real: Vec<f64> = values.iter().map(|&v| v as f64).collect();
                        let (skew, kurt) = shape(&real);
                        let fraction = match mode {
                            "skewness" => skew,
                            "kurtosis" => kurt,
                            _ => entropy(&real, |v| (v as u64 * 256 / (full + 1)) as usize),
                        };
                        ((full as f64 * fraction + 0.5).floor()).clamp(0.0, full as f64) as u64
                    }
                    "majority" => {
                        let real: Vec<f64> = values.iter().map(|&v| v as f64).collect();
                        let tied = modal(&real);
                        let t = tied.len();
                        if t % 2 == 1 {
                            tied[t / 2] as u64
                        } else {
                            half_up(tied[t / 2 - 1] as u64 + tied[t / 2] as u64, 2)
                        }
                    }
                    other => panic!("unknown mode {other}"),
                };
            }
            out
        })
        .collect()
}

/// The float statistic over the extent in f64, with the number of members
/// the pixel's statistic read (0 for a transparent pixel): an odd count's
/// median, and every minimum and maximum, is one member's own value.
pub fn real(mode: &str, members: &[Member]) -> Vec<([f64; 4], usize)> {
    present(members)
        .into_iter()
        .map(|here| {
            if here.is_empty() {
                return ([0.0; 4], 0);
            }
            let alpha = here.iter().map(|p| p[3]).fold(0.0, f64::max);
            let mut out = [0.0, 0.0, 0.0, alpha];
            let half: Vec<[f64; 4]> = here.iter().copied().filter(|p| p[3] >= 0.5).collect();
            let voters = if half.is_empty() { here.clone() } else { half };
            let n = if mode == "mean" {
                here.len()
            } else {
                voters.len()
            };
            for c in 0..3 {
                let mut values: Vec<f64> = voters.iter().map(|p| p[c]).collect();
                values.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let k = values.len();
                out[c] = match mode {
                    "mean" => {
                        here.iter().map(|p| p[3] * p[c]).sum::<f64>()
                            / here.iter().map(|p| p[3]).sum::<f64>()
                    }
                    "median" if k % 2 == 1 => values[k / 2],
                    "median" => (values[k / 2 - 1] + values[k / 2]) / 2.0,
                    "minimum" => values[0],
                    "maximum" => values[k - 1],
                    "range" => values[k - 1] - values[0],
                    "standard_deviation" => {
                        let mean = values.iter().sum::<f64>() / k as f64;
                        (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / k as f64).sqrt()
                    }
                    "summation" => values.iter().sum(),
                    "variance" => {
                        let mean = values.iter().sum::<f64>() / k as f64;
                        values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / k as f64
                    }
                    "skewness" => shape(&values).0,
                    "kurtosis" => shape(&values).1,
                    "entropy" => {
                        entropy(&values, |v| ((v.clamp(0.0, 1.0) * 256.0) as usize).min(255))
                    }
                    "majority" => {
                        let tied = modal(&values);
                        let t = tied.len();
                        if t % 2 == 1 {
                            tied[t / 2]
                        } else {
                            (tied[t / 2 - 1] + tied[t / 2]) / 2.0
                        }
                    }
                    other => panic!("unknown mode {other}"),
                };
            }
            (out, n)
        })
        .collect()
}
