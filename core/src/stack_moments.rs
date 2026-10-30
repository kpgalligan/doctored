//! The per-pixel statistic of a Stack Mode: one gathered pixel's
//! members in, one output sample out. A kernel: it names no document type;
//! `stack_stat` gathers the members and owns the request, the extent and the
//! admission, and its module doc holds the shipped six modes' rule.
//!
//! # The rule of the twelve
//!
//! Members arrive as S₀ (alpha > 0). The output alpha is the largest alpha
//! in S₀. Mean reads S₀ alpha-weighted; every other mode reads S, the
//! members at least half covered (S₀ when none is), unweighted and per
//! colour channel. With v a member's stored value, n = |S| and F the full
//! code (255, 65535, or 1.0 at float):
//!
//! - **Summation** is Σv.
//! - **Variance** is the population variance m₂ divided by F, so it is in
//!   codes (normalised σ² × F). At 8 and 16 bits it is exact:
//!   `(n·Σv² − (Σv)²) / (n²·F)` rounded half up, the radicand in u128.
//! - **Skewness** is `F/2 · (1 + s/s_max)`, s = m₃/m₂^{3/2} and
//!   s_max(n) = (n − 2)/√(n − 1), the largest |s| n values can reach (one
//!   member apart from the rest), so it spans [0, F] with F/2 for a
//!   symmetric sample.
//! - **Kurtosis** is `F · (k − 1)/(k_max − 1)`, k = m₄/m₂² (non-excess) and
//!   k_max(n) = (n² − 3n + 3)/(n − 1), reached by the same one-apart sample,
//!   so it spans [0, F]: 0 for two equally sized clusters, F for one outlier.
//! - When m₂ = 0 (every value equal) or n ≤ 2 (s_max = 0, k ≡ 1), s is 0:
//!   Skewness reads F/2 and Kurtosis 0.
//! - **Entropy** is `H₂ / log₂ min(n, 256) × F`, H₂ the base-2 Shannon
//!   entropy of the values over a fixed histogram of 256 equal bins of
//!   [0, F] at every depth: an integer value's bin is ⌊v·256/(F + 1)⌋ (the
//!   code itself at 8 bits, `v >> 8` at 16), a float's
//!   min(255, ⌊clamp(v, 0, 1)·256⌋, the clamp for binning only. n = 1 reads 0.
//! - **Majority** is the most frequent value; a tie takes the Median rule
//!   (the middle one, or the half-up mean of the two middle ones) over the
//!   tied values, ascending.
//!
//! Integers: every f64 result rounds half up (`⌊x + ½⌋`) and is clamped to
//! [0, F] (Summation clamps at F). Float: f64, stored f32 unclamped, so a
//! Summation above white survives; the typed adapter's `bounded` holds it to
//! the storage bound.
//!
//! Why the bounds and the bins: an unscaled kurtosis is at least 1, so k × F
//! clamped to F would be white wherever members differ at all — a two-value
//! mask — and an unscaled skewness is signed; dividing by the bound for n
//! members keeps the whole range in the result. A histogram of exact values
//! has as many bins as distinct values, so at 16 bits and float almost every
//! pixel whose members differ by a code would read full entropy (flat
//! white); 256 bins at every depth make the three depths agree on the same
//! picture. Dimensionless results (Skewness, Kurtosis, Entropy) are a
//! fraction of white; not measured against Photoshop.

use crate::stack_stat::StackMode;

/// The integer rule over the gathered S₀ (`members`, alpha > 0 each), with
/// `full` the full alpha code (module doc).
pub(crate) fn integer_statistic(
    mode: StackMode,
    members: &mut Vec<[u64; 4]>,
    order: &mut Vec<u64>,
    full: u64,
) -> [u64; 4] {
    if members.is_empty() {
        return [0; 4];
    }
    let alpha = members.iter().map(|m| m[3]).max().unwrap_or(0);
    let mut out = [0, 0, 0, alpha];
    if mode == StackMode::Mean {
        let den: u64 = members.iter().map(|m| m[3]).sum();
        for (c, slot) in out.iter_mut().take(3).enumerate() {
            let num: u64 = members.iter().map(|m| m[3] * m[c]).sum();
            *slot = (2 * num + den) / (2 * den);
        }
        return out;
    }
    if members.iter().any(|m| 2 * m[3] >= full) {
        members.retain(|m| 2 * m[3] >= full);
    }
    let n = members.len();
    for (c, slot) in out.iter_mut().take(3).enumerate() {
        order.clear();
        order.extend(members.iter().map(|m| m[c]));
        let low = order.iter().copied().min().unwrap_or(0);
        let high = order.iter().copied().max().unwrap_or(0);
        *slot = match mode {
            StackMode::Mean => unreachable!("answered above"),
            StackMode::Median => {
                order.sort_unstable();
                if n % 2 == 1 {
                    order[n / 2]
                } else {
                    (order[n / 2 - 1] + order[n / 2]).div_ceil(2)
                }
            }
            StackMode::Minimum => low,
            StackMode::Maximum => high,
            StackMode::Range => high - low,
            StackMode::StandardDeviation => {
                // σ = √(n·Σv² − (Σv)²) / n: the radicand is an exact integer,
                // so the only rounding is the square root's and the final one.
                let sigma = (radicand(order) as f64).sqrt() / n as f64;
                (sigma + 0.5).floor() as u64
            }
            StackMode::Summation => order.iter().sum::<u64>().min(full),
            StackMode::Variance => {
                // m₂ / F = (n·Σv² − (Σv)²) / (n²·F), half up through the
                // quotient: exact, with no f64 step.
                let den = n as u128 * n as u128 * u128::from(full);
                let quotient = (2 * radicand(order) + den) / (2 * den);
                (quotient as u64).min(full)
            }
            StackMode::Skewness | StackMode::Kurtosis => {
                let values = order.iter().map(|&v| v as f64);
                let fraction = shape_fraction(mode, n, low == high, values);
                code(full as f64 * fraction, full)
            }
            StackMode::Entropy => {
                // The bin of a code: ⌊v·256/(F + 1)⌋, the code at 8 bits.
                for v in order.iter_mut() {
                    *v = *v * 256 / (full + 1);
                }
                order.sort_unstable();
                code(full as f64 * entropy_fraction(order), full)
            }
            StackMode::Majority => {
                order.sort_unstable();
                let (lo, hi) = majority(order);
                (lo + hi).div_ceil(2)
            }
        };
    }
    out
}

/// The float rule over the gathered S₀, in f64 (module doc).
pub(crate) fn real_statistic(
    mode: StackMode,
    members: &mut Vec<[f64; 4]>,
    order: &mut Vec<f64>,
) -> [f64; 4] {
    if members.is_empty() {
        return [0.0; 4];
    }
    let alpha = members.iter().map(|m| m[3]).fold(0.0, f64::max);
    let mut out = [0.0, 0.0, 0.0, alpha];
    if mode == StackMode::Mean {
        let den: f64 = members.iter().map(|m| m[3]).sum();
        for (c, slot) in out.iter_mut().take(3).enumerate() {
            let num: f64 = members.iter().map(|m| m[3] * m[c]).sum();
            *slot = num / den;
        }
        return out;
    }
    if members.iter().any(|m| 2.0 * m[3] >= 1.0) {
        members.retain(|m| 2.0 * m[3] >= 1.0);
    }
    let n = members.len();
    for (c, slot) in out.iter_mut().take(3).enumerate() {
        order.clear();
        order.extend(members.iter().map(|m| m[c]));
        let low = order.iter().copied().fold(f64::INFINITY, f64::min);
        let high = order.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        *slot = match mode {
            StackMode::Mean => unreachable!("answered above"),
            StackMode::Median => {
                order.sort_unstable_by(f64::total_cmp);
                if n % 2 == 1 {
                    order[n / 2]
                } else {
                    (order[n / 2 - 1] + order[n / 2]) / 2.0
                }
            }
            StackMode::Minimum => low,
            StackMode::Maximum => high,
            StackMode::Range => high - low,
            StackMode::StandardDeviation => central_moments(order.iter().copied()).0.sqrt(),
            StackMode::Summation => order.iter().sum(),
            // m₂ / F with F = 1.
            StackMode::Variance => central_moments(order.iter().copied()).0,
            StackMode::Skewness | StackMode::Kurtosis => {
                shape_fraction(mode, n, low == high, order.iter().copied())
            }
            StackMode::Entropy => {
                for v in order.iter_mut() {
                    // The clamp is for binning only (module doc).
                    *v = (v.clamp(0.0, 1.0) * 256.0).floor().min(255.0);
                }
                order.sort_unstable_by(f64::total_cmp);
                entropy_fraction(order)
            }
            StackMode::Majority => {
                order.sort_unstable_by(f64::total_cmp);
                let (lo, hi) = majority(order);
                (lo + hi) / 2.0
            }
        };
    }
    out
}

/// `n·Σv² − (Σv)²` for the integer values, exact in u128.
fn radicand(values: &[u64]) -> u128 {
    let sum: u128 = values.iter().map(|&v| u128::from(v)).sum();
    let squares: u128 = values.iter().map(|&v| u128::from(v) * u128::from(v)).sum();
    (values.len() as u128 * squares).saturating_sub(sum * sum)
}

/// `x` rounded half up and held to [0, `full`].
fn code(x: f64, full: u64) -> u64 {
    (x + 0.5).floor().clamp(0.0, full as f64) as u64
}

/// The population central moments (m₂, m₃, m₄), in two passes (the mean,
/// then the deviations): the one-pass form cancels badly for values far
/// above white.
fn central_moments(values: impl Iterator<Item = f64> + Clone) -> (f64, f64, f64) {
    let (count, sum) = values
        .clone()
        .fold((0usize, 0.0), |(k, s), v| (k + 1, s + v));
    if count == 0 {
        return (0.0, 0.0, 0.0);
    }
    let n = count as f64;
    let mean = sum / n;
    let (m2, m3, m4) = values.fold((0.0, 0.0, 0.0), |(a, b, c), v| {
        let d = v - mean;
        let d2 = d * d;
        (a + d2, b + d2 * d, c + d2 * d2)
    });
    (m2 / n, m3 / n, m4 / n)
}

/// Skewness or Kurtosis as a fraction of white (module doc): `flat` is
/// every value equal, the m₂ = 0 case, decided exactly rather than from a
/// rounded m₂.
fn shape_fraction(
    mode: StackMode,
    n: usize,
    flat: bool,
    values: impl Iterator<Item = f64> + Clone,
) -> f64 {
    let skew = mode == StackMode::Skewness;
    let symmetric = if skew { 0.5 } else { 0.0 };
    if n <= 2 || flat {
        return symmetric;
    }
    let (m2, m3, m4) = central_moments(values);
    if m2 <= 0.0 {
        return symmetric;
    }
    let count = n as f64;
    if skew {
        let s = m3 / m2.powf(1.5);
        let s_max = (count - 2.0) / (count - 1.0).sqrt();
        0.5 * (1.0 + s / s_max)
    } else {
        let k = m4 / (m2 * m2);
        let k_max = (count * count - 3.0 * count + 3.0) / (count - 1.0);
        (k - 1.0) / (k_max - 1.0)
    }
}

/// H₂ / log₂ min(n, 256) of SORTED bin indices (module doc); 0 for one.
fn entropy_fraction<T: PartialEq + Copy>(bins: &[T]) -> f64 {
    let n = bins.len();
    if n <= 1 {
        return 0.0;
    }
    let total = n as f64;
    let mut entropy = 0.0;
    for run in bins.chunk_by(|a, b| a == b) {
        let p = run.len() as f64 / total;
        entropy -= p * p.log2();
    }
    entropy / (n.min(256) as f64).log2()
}

/// The two values the Majority rule combines from SORTED values: the most
/// frequent value twice, or the middle tied value twice, or the two middle
/// tied values of an even tie (module doc).
fn majority<T: PartialEq + Copy>(sorted: &[T]) -> (T, T) {
    let most = sorted
        .chunk_by(|a, b| a == b)
        .map(<[T]>::len)
        .max()
        .unwrap_or(0);
    let tied = sorted
        .chunk_by(|a, b| a == b)
        .filter(|run| run.len() == most);
    let count = tied.clone().count();
    let mut pick = tied.map(|run| run[0]).skip((count - 1) / 2);
    let lo = pick.next().unwrap_or(sorted[0]);
    let hi = if count % 2 == 1 {
        lo
    } else {
        pick.next().unwrap_or(lo)
    };
    (lo, hi)
}
