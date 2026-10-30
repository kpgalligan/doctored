//! The anti-aliased-contour band (#578): a band grown `n` px into a byte
//! coverage `u` (the Inner Glow's `shape.inverted()`) measured from the
//! ANTI-ALIASED contour rather than the contour binarized at 1/2. Photoshop
//! reads it so (the fourth identify take, `identify_578_take4.py`): a pixel
//! of partial coverage enters the band at its own coverage, so the band's
//! edge moves with the edge pixel's coverage instead of jumping a whole
//! pixel where the binarized band does.
//!
//! The model it mirrors is the runner's `Field.aa` and INNER JUDGE's BAND
//! TERMS (`scripts/photoshop-probe/glow_aa_578_law_fit.py`), in units of `u
//! / 255`:
//!
//! - `b_full = clamp(n + 1 − d(p, {u = 1}))`: today's band of the fully
//!   covered pixels, `dilate_plane_with_job` on `{u == 255}` (on a binary
//!   plane exactly today's band bytes);
//! - `b_part = clamp(n − min(|p − q| − u_q))` over the partial pixels q (0 <
//!   u_q < 1) with `|p − q| ≤ n + 1`, 0 if none;
//! - the band is `max(b_full, b_part)` ([`AaBand::joined`]).
//!
//! Each term is rounded once to a byte (`round(255 · b)`), as today's band
//! is. No free number: nothing here was fitted.
//!
//! Why the search is bounded: with `d_part(p)` the distance from p's centre
//! to the nearest partial pixel's centre (0 on a partial pixel), every
//! partial q has `|p − q| − u_q > |p − q| − 1 ≥ d_part − 1` and the nearest
//! one gives at most `d_part`, so `min(|p − q| − u_q)` lies in
//! `(d_part − 1, d_part]`. Where `d_part ≤ n − 1` the term is 1 (255) and
//! where `d_part ≥ n + 1` it is 0, with no search; the 1e-4 margin covers
//! `d_part`'s f32 rounding (`doc_select`'s EDT, the master distance, returns
//! `0.5 − dist` off its members; `d_part = 0.5 − s`). Only the shell
//! `n − 1 < d_part < n + 1` is searched, and only where `b_full` is below
//! 255 (the joined band is 255 there whatever `b_part` is; its partial byte
//! stays 0). The search
//! walks the offsets within `n + 1` sorted by length from `d_part − 1e-4`
//! and stops at the length `min(n + 1, best + 1)`, past which no partial
//! pixel can lower `best`. On a wide feathered rim that is O(perimeter · n)
//! offsets, not the rim's area (`style_inner_glow_aa_parity`'s bench twin
//! holds the cost to its declared bound).
//!
//! Memory: the input plane, the two output byte planes, and the EDT's two
//! f32 planes while it runs (`full`'s dilation frees its own before the
//! partial EDT starts), at most `12·P` bytes for a `P`-pixel plane, inside
//! `precision_style_render::style_scratch`'s charge.

use crate::doc_select::signed_distance_from_membership;
use crate::job_control::JobContext;
use crate::style_plane_ops::dilate_plane_with_job;

/// The band's two terms (module doc), each a byte plane of the input's size.
/// `partial` is not searched where `full` is 255.
pub(crate) struct AaBand {
    pub(crate) full: Vec<u8>,
    pub(crate) partial: Vec<u8>,
}

impl AaBand {
    /// The band: the per-pixel max of the two terms.
    pub(crate) fn joined(&self) -> Vec<u8> {
        self.full
            .iter()
            .zip(&self.partial)
            .map(|(&a, &b)| a.max(b))
            .collect()
    }
}

/// A partial-coverage byte: neither empty nor full.
fn is_partial(v: u8) -> bool {
    (1..=254).contains(&v)
}

/// Whether some byte of `u` is partial (1..=254). A plane with none takes
/// today's binarized band, which equals the anti-aliased one there.
pub(crate) fn has_partial(u: &[u8]) -> bool {
    u.iter().any(|&v| is_partial(v))
}

/// The search margin around `d_part` (module doc): `d_part` comes from an
/// f32 EDT, good to about 1.5e-5 px at a distance of 250.
const SHELL_MARGIN: f64 = 1e-4;

/// The anti-aliased band of `u` grown by `n` px (module doc), polling `job`
/// in every pixel loop and inside both distance transforms.
pub(crate) fn aa_band_with_job(
    u: &[u8],
    w: u32,
    h: u32,
    n: f32,
    job: JobContext<'_>,
) -> Result<AaBand, String> {
    let (wu, hu) = (w as usize, h as usize);
    let len = wu * hu;
    if u.len() != len {
        return Err("internal error: anti-aliased band plane size".into());
    }
    let n = if n.is_finite() { n.max(0.0) } else { 0.0 };
    job.check()?;
    let mut full = vec![0u8; len];
    for (i, (f, &v)) in full.iter_mut().zip(u).enumerate() {
        job.every(i)?;
        if v == 255 {
            *f = 255;
        }
    }
    dilate_plane_with_job(&mut full, w, h, n, job)?;
    let mut partial = vec![0u8; len];
    if !has_partial(u) {
        return Ok(AaBand { full, partial });
    }
    let s = signed_distance_from_membership(wu, hu, |i| is_partial(u[i]), job)?;
    let nn = f64::from(n);
    let offsets = offsets_within(nn + 1.0);
    for i in 0..len {
        job.every(i)?;
        let d_part = if is_partial(u[i]) {
            0.0
        } else {
            0.5 - f64::from(s[i])
        };
        if d_part <= nn - 1.0 - SHELL_MARGIN {
            partial[i] = 255;
            continue;
        }
        if d_part >= nn + 1.0 + SHELL_MARGIN || full[i] == 255 {
            continue;
        }
        let (x, y) = ((i % wu) as i64, (i / wu) as i64);
        let first = offsets.partition_point(|o| o.2 < d_part - SHELL_MARGIN);
        let mut best = f64::INFINITY;
        for &(dx, dy, length) in &offsets[first..] {
            if length >= (nn + 1.0).min(best + 1.0) {
                break;
            }
            let (qx, qy) = (x + i64::from(dx), y + i64::from(dy));
            if qx < 0 || qy < 0 || qx >= wu as i64 || qy >= hu as i64 {
                continue;
            }
            let uq = u[qy as usize * wu + qx as usize];
            if is_partial(uq) {
                best = best.min(length - f64::from(uq) / 255.0);
            }
        }
        let b = (nn - best).clamp(0.0, 1.0);
        partial[i] = (255.0 * b).round() as u8;
    }
    Ok(AaBand { full, partial })
}

/// Every integer offset strictly within `reach` of the origin, with its
/// length, sorted by length (built once per band; `reach` ≤ 251 for a
/// Size of 250).
fn offsets_within(reach: f64) -> Vec<(i32, i32, f64)> {
    let r = reach.floor() as i32;
    let mut out = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            let length = f64::from(dx).hypot(f64::from(dy));
            if length < reach {
                out.push((dx, dy, length));
            }
        }
    }
    out.sort_by(|a, b| a.2.total_cmp(&b.2));
    out
}
