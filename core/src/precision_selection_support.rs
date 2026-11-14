//! Support-bounded evaluation of the typed (u16) Modify Selection kernels
//! (Phase 13A-6a). Feather, Smooth, Grow, Shrink and Border — and Refine's
//! stages, which ARE those kernels — run over the selection's SUPPORT (the
//! bounding rect of its nonzero samples) grown by the kernel's REACH and
//! clamped to the canvas, then the result is embedded in a zeroed canvas
//! plane. This module is index arithmetic only (the rect, the reach, the work
//! figure, the crop and the embed); the kernel bodies stay in
//! `PlaneOperation::spatial_kernel`, the one implementation both this path and
//! the whole-plane reference (`precision_selection_plane::reference_apply`)
//! run. The bounded path has no work refusal (it runs under its
//! caller's job control); the work figure here is what the whole-plane
//! reference still compares to `precision_render::MAX_WORK_UNITS`.
//!
//! EXACTNESS — the bounded result equals the whole-plane kernel sample for
//! sample:
//!
//! * Feather / Smooth. `gaussian_plane::feather` accumulates `k · sample` in
//!   a fixed tap order with edge clamps and no absolute-coordinate
//!   arithmetic. With the window the support grown by `taps / 2`, every
//!   clamped tap of the crop reads a sample that is 0 in both the crop and the
//!   full plane (or the same canvas-edge sample, where the window meets the
//!   canvas), and the horizontal intermediate is exactly 0.0 on every row
//!   outside the support's rows, so the crop is f32-bit-identical inside the
//!   window and the full plane is exactly 0 outside it (Smooth maps 0 → 0).
//!   The argument is the kernel's, not the sample type's: the BYTE feather
//!   and smooth (`doc_select::feather_mask_with_context`, 8-bit Select ▸
//!   Modify and Refine) run the same `support`, `gaussian_reach`, `window`
//!   and `crop` over u8 coverage, where `Scalar::from_filtered(0.0)` is 0 too.
//! * Grow / Shrink / Border (needs the f64 envelope in `doc_select::edt_1d`).
//!   Membership is `sample >= 32768`, so every inside pixel lies in the
//!   support. A window pixel's nearest inside pixel is therefore in the crop,
//!   and its nearest outside pixel is strictly nearer than any pixel beyond
//!   the window: the ring at Chebyshev distance 1 around the support lies in
//!   the window (reach ≥ 2) and beats anything ≥ reach + 1 away. With exact
//!   lower envelopes each path computes the exact 2-D minimum over its own
//!   site set, so the winning squared distance is the same integer, exact in
//!   f32 while it is < 2^24; and every code depends only on distances
//!   ≤ r + 1 ≤ 251 (beyond that `(d + 0.5).clamp(0, 1)` saturates
//!   identically). Pixels outside the window are ≥ r + 2 from the support, so
//!   their code is 0 on the whole plane too.
//! * Stroke (Edit ▸ Stroke…'s band, `precision_selection_plane::stroke_band`).
//!   Its Outside and Inside halves are the Grow and Shrink arms above at the
//!   stroke width (Center at half of it), so they are exact by the Grow /
//!   Shrink argument with reach `ceil(width) + 1`. The one extra term is the
//!   plane-edge ring `cov ⊖ cov16((k + 1) − 0.5 − width)`, `k` measured to
//!   the edge of the plane the kernel sees, here the WINDOW. An unclamped
//!   window edge is ≥ reach = `ceil(width) + 1` from every nonzero sample, so
//!   there `(k + 1) − 0.5 − width ≥ 1.5`, the ring's coverage is 65535 and it
//!   subtracts everything: the ring is 0, as it is on the whole plane, where
//!   that pixel is at least as far from every canvas edge. A clamped window
//!   edge IS the canvas edge, so `k` is the same number on both paths. A zero
//!   sample's ring is 0 on both.
//! * Refine. Each stage is exact over the canvas by the above, so the
//!   composition equals the whole-plane `reference_refine`, whose summed work
//!   check (the oracle's own, at the canvas) charges stage i at the support
//!   grown by the cumulative reach of stages 1..=i.
//! * A full-support input takes the window = the canvas: the whole-plane
//!   arithmetic on the same plane.
//!
//! CANCELLATION: the support scan, crop and embed poll every 4096 samples;
//! the kernel keeps its own polls (the Gaussian per row and per 4096 cells,
//! the distance transform per column and row), now over the crop's rows.
//!
//! Phase 13A-6a's SUPPORT-BOUNDED typed Modify Selection: index arithmetic only (the nonzero
//! support rect, each kind's reach — Feather/Smooth the Gaussian half width, Grow/Shrink/Border
//! `ceil(r) + 1` — the window, its work figure, the crop and the embed, all polled per 4096
//! samples). `PlaneOperation::apply` runs Feather/Smooth/Grow/Shrink/Border through
//! `spatial_kernel` (the ONE kernel body) over the support grown by the reach and embeds the
//! result; neither `plan` (scratch only) nor `apply` nor `precision_selection_refine::refine`
//! refuses for work (each runs under a job control). This module doc
//! is the exactness argument — equal sample for sample to the whole plane, which needs
//! `doc_select::edt_1d`'s f64 envelope — and `precision_selection_plane::reference_apply`
//! (`#[doc(hidden)]`, the whole-plane path over `spatial_kernel` / `reference_refine`) is the test
//! oracle `tests/bounded_selection_support.rs` compares against.
use crate::gaussian_plane::nearest_odd;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::precision_selection_plane::PlaneOperation;

/// `(x0, y0, x1, y1)`, `x1` / `y1` exclusive.
pub(crate) type Rect = (u32, u32, u32, u32);

/// Bounding rect of the NONZERO samples; `None` when every sample is 0.
/// Polls `job.every(i)`. Generic over the sample so the byte feather
/// (`doc_select::feather_mask_with_context`) runs the same scan on u8.
pub(crate) fn support<T: Copy + Default + PartialEq>(
    input: &[T],
    width: u32,
    height: u32,
    job: JobContext<'_>,
) -> Result<Option<Rect>, String> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 {
        return Ok(None);
    }
    let zero = T::default();
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    let (mut x, mut y) = (0usize, 0usize);
    for (i, &value) in input.iter().take(w * h).enumerate() {
        job.every(i)?;
        if value != zero {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
        x += 1;
        if x == w {
            x = 0;
            y += 1;
        }
    }
    Ok((x1 > 0).then_some((x0 as u32, y0 as u32, x1 as u32, y1 as u32)))
}

/// How far a kernel's output can reach beyond its input's support.
/// Feather/Smooth: the Gaussian's half width `nearest_odd(radius·2+1).max(3)
/// / 2`; Grow/Shrink/Border: `ceil(radius) + 1`; Stroke: `ceil(width) + 1`;
/// other kinds: `None`.
pub(crate) fn reach(op: &PlaneOperation) -> Option<u32> {
    match op {
        PlaneOperation::Feather { radius } | PlaneOperation::Smooth { radius } => {
            Some(gaussian_reach(*radius))
        }
        PlaneOperation::Grow { radius }
        | PlaneOperation::Shrink { radius }
        | PlaneOperation::Border { radius } => Some(radius.ceil() as u32 + 1),
        PlaneOperation::Stroke { width, .. } => Some(width.ceil() as u32 + 1),
        _ => None,
    }
}

/// The Gaussian's half width at `radius`, `nearest_odd(radius·2+1).max(3) /
/// 2` (`gaussian_plane::feather`'s taps): Feather's and Smooth's reach at
/// both depths.
pub(crate) fn gaussian_reach(radius: f32) -> u32 {
    nearest_odd(radius * 2. + 1.).max(3) / 2
}

/// `support` grown by `reach` on every side, clamped to the canvas.
pub(crate) fn window(support: Rect, reach: u32, width: u32, height: u32) -> Rect {
    (
        support.0.saturating_sub(reach),
        support.1.saturating_sub(reach),
        support.2.saturating_add(reach).min(width),
        support.3.saturating_add(reach).min(height),
    )
}

/// `plan`'s pre-13A-6a work expression with the canvas pixel count replaced by
/// the window's: Feather/Smooth `|W|·2·taps`, Grow/Shrink/Border `|W|·32`,
/// Stroke `|W|·64` (any other kind `|W|`).
pub(crate) fn work(op: &PlaneOperation, window: Rect) -> u64 {
    let pixels = u64::from(window.2 - window.0) * u64::from(window.3 - window.1);
    match op {
        PlaneOperation::Feather { radius } | PlaneOperation::Smooth { radius } => pixels
            .saturating_mul(2)
            .saturating_mul(u64::from(nearest_odd(radius * 2. + 1.).max(3))),
        PlaneOperation::Grow { .. }
        | PlaneOperation::Shrink { .. }
        | PlaneOperation::Border { .. } => pixels.saturating_mul(32),
        PlaneOperation::Stroke { .. } => pixels.saturating_mul(64),
        _ => pixels,
    }
}

/// The window's samples of a `width`-wide plane, row-major (u16 here, u8 in
/// the byte feather).
pub(crate) fn crop<T: Copy>(
    input: &[T],
    width: u32,
    window: Rect,
    job: JobContext<'_>,
) -> Result<Vec<T>, String> {
    let (x0, y0, x1, y1) = (
        window.0 as usize,
        window.1 as usize,
        window.2 as usize,
        window.3 as usize,
    );
    let w = width as usize;
    let mut out = reserved_vec((x1 - x0) * (y1 - y0))?;
    let mut i = 0usize;
    for y in y0..y1 {
        for &value in &input[y * w + x0..y * w + x1] {
            job.every(i)?;
            out.push(value);
            i += 1;
        }
    }
    Ok(out)
}

/// A zeroed `width`×`height` plane with `samples` (window-sized, row-major)
/// written into `window`. An empty window gives the zeroed plane.
pub(crate) fn embed(
    samples: &[u16],
    width: u32,
    height: u32,
    window: Rect,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let (x0, y0, x1, y1) = (
        window.0 as usize,
        window.1 as usize,
        window.2 as usize,
        window.3 as usize,
    );
    let (w, h) = (width as usize, height as usize);
    let ww = x1.saturating_sub(x0);
    let mut out = reserved_vec(w * h)?;
    let mut i = 0usize;
    for y in 0..h {
        let inside = (y0..y1).contains(&y);
        for x in 0..w {
            job.every(i)?;
            out.push(if inside && (x0..x1).contains(&x) {
                samples[(y - y0) * ww + (x - x0)]
            } else {
                0
            });
            i += 1;
        }
    }
    Ok(out)
}
